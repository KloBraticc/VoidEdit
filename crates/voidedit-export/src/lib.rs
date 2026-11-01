use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use filmcraft_h264enc::{Encoder, EncoderConfig, PacketFormat, RateControl};
use filmcraft_isobmff::{AvcConfig, Brand, Mp4Writer, SampleEntry, TrackConfig, WriteSample, WriterOptions};
use voidedit_core::Project;
use voidedit_media::{decode_now, mix, AudioClip, FrameWant, Gain, Library};
use voidedit_render::{render, wanted_frame, Frame, Image};

const AUDIO_RATE: u32 = 48_000;

#[derive(Default)]
pub struct Progress {
    pub done: AtomicU32,
    pub total: AtomicU32,
    pub cancel: AtomicBool,
}

pub fn audio_clips(project: &Project) -> Vec<AudioClip> {
    let fps = project.fps.max(1) as f64;
    project
        .clips
        .iter()
        .filter(|clip| !project.tracks.get(clip.track).is_some_and(|track| track.muted))
        .filter(|clip| !clip.reverse)
        .filter_map(|clip| {
            let (start, end) = (clip.start as f64 / fps, clip.end as f64 / fps);
            Some(AudioClip {
                path: clip.media.clone()?,
                start,
                end,
                offset: clip.offset as f64 / fps,
                gain: Gain { level: clip.volume.base.max(0.0) / 100.0, start, end, fade_in: clip.fade_in as f64 / fps, fade_out: clip.fade_out as f64 / fps },
                speed: clip.speed,
            })
        })
        .collect()
}

fn to_yuv(frame: &Frame, width: usize, height: usize, y: &mut [u8], u: &mut [u8], v: &mut [u8]) {
    let at = |column: usize, row: usize| {
        let start = (row * frame.width + column) * 4;
        (frame.pixels[start] as i32, frame.pixels[start + 1] as i32, frame.pixels[start + 2] as i32)
    };
    for row in 0..height {
        for column in 0..width {
            let (r, g, b) = at(column, row);
            y[row * width + column] = (((47 * r + 157 * g + 16 * b + 128) >> 8) + 16) as u8;
        }
    }
    let half = width / 2;
    for row in 0..height / 2 {
        for column in 0..half {
            let (mut r, mut g, mut b) = (0, 0, 0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let pixel = at(column * 2 + dx, row * 2 + dy);
                r += pixel.0;
                g += pixel.1;
                b += pixel.2;
            }
            let (r, g, b) = (r / 4, g / 4, b / 4);
            u[row * half + column] = (((-26 * r - 87 * g + 112 * b + 128) >> 8) + 128) as u8;
            v[row * half + column] = (((112 * r - 102 * g - 10 * b + 128) >> 8) + 128) as u8;
        }
    }
}

pub fn export(project: &Project, library: &Library, path: &Path, progress: &Arc<Progress>) -> Result<(), String> {
    let width = (project.width & !1) as usize;
    let height = (project.height & !1) as usize;
    let fps = project.fps.max(1);
    let total = project.end_frame().max(1);
    progress.total.store(total, Ordering::Relaxed);
    let mut config = EncoderConfig::new(width as u32, height as u32, fps, 1);
    config.format = PacketFormat::LengthPrefixed;
    config.bframes = 0;
    config.rate = RateControl::Crf(20.0);
    let mut encoder = Encoder::new(config).map_err(|error| format!("{error:?}"))?;
    let file = BufWriter::new(File::create(path).map_err(|error| error.to_string())?);
    let mut writer = Mp4Writer::new(file, WriterOptions::new(Brand::Mp4)).map_err(|error| error.to_string())?;
    let avc = AvcConfig::parse(&encoder.avcc()).map_err(|error| format!("{error:?}"))?;
    let video = writer.add_track(TrackConfig::new(SampleEntry::avc(avc, width as u16, height as u16), fps)).map_err(|error| error.to_string())?;
    let clips = audio_clips(project);
    let mut audio = if clips.is_empty() {
        None
    } else {
        let aac = filmcraft_aac::Encoder::new(filmcraft_aac::EncoderConfig::cbr(AUDIO_RATE, 2, 192_000)).map_err(|error| format!("{error:?}"))?;
        let entry = SampleEntry::aac(aac.audio_specific_config(), 2, AUDIO_RATE);
        let track = writer.add_track(TrackConfig::new(entry, AUDIO_RATE)).map_err(|error| error.to_string())?;
        Some((aac, track))
    };
    let mut frame = Frame::default();
    let (mut y, mut u, mut v) = (vec![0; width * height], vec![0; width * height / 4], vec![0; width * height / 4]);
    let mut audio_position = 0i64;
    for index in 0..total {
        if progress.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let mut images: HashMap<String, Arc<Image>> = HashMap::new();
        for clip in project.clips.iter().filter(|clip| wanted_frame(project, clip, index)) {
            if let Some(media) = &clip.media {
                let want = FrameWant { path: media.clone(), seconds: clip.media_seconds(index, fps), scale: 1.0 };
                if let Some(image) = decode_now(library, &want) {
                    images.insert(media.clone(), image);
                }
            }
        }
        render(project, index, 1.0, &mut frame, &|name| images.get(name).cloned());
        let exact = Project { width: width as u32, height: height as u32, ..project.clone() };
        if frame.width != width || frame.height != height {
            render(&exact, index, 1.0, &mut frame, &|name| images.get(name).cloned());
        }
        to_yuv(&frame, width, height, &mut y, &mut u, &mut v);
        let input = filmcraft_h264enc::YuvFrame { y: &y, u: &u, v: &v, y_stride: width, uv_stride: width / 2 };
        for packet in encoder.encode(&input, index as i64).map_err(|error| format!("{error:?}"))? {
            write_video(&mut writer, video, &packet)?;
        }
        if let Some((aac, track)) = &mut audio {
            let until = (index as i64 + 1) * AUDIO_RATE as i64 / fps as i64;
            let frames = (until - audio_position) as usize;
            let mixed = mix(library, &clips, audio_position, frames, AUDIO_RATE);
            audio_position = until;
            let left: Vec<f32> = mixed.iter().step_by(2).copied().collect();
            let right: Vec<f32> = mixed.iter().skip(1).step_by(2).copied().collect();
            for unit in aac.encode(&[&left, &right]) {
                write_audio(&mut writer, *track, &unit)?;
            }
        }
        progress.done.store(index + 1, Ordering::Relaxed);
    }
    for packet in encoder.flush() {
        write_video(&mut writer, video, &packet)?;
    }
    if let Some((aac, track)) = &mut audio {
        for unit in aac.flush() {
            write_audio(&mut writer, *track, &unit)?;
        }
    }
    writer.finish().map_err(|error| error.to_string())?;
    Ok(())
}

fn write_video(writer: &mut Mp4Writer<BufWriter<File>>, track: usize, packet: &filmcraft_h264enc::Packet) -> Result<(), String> {
    writer
        .write_sample(track, WriteSample { data: &packet.data, duration: 1, composition_offset: 0, is_sync: packet.keyframe })
        .map_err(|error| error.to_string())
}

fn write_audio(writer: &mut Mp4Writer<BufWriter<File>>, track: usize, unit: &[u8]) -> Result<(), String> {
    writer.write_sample(track, WriteSample { data: unit, duration: 1024, composition_offset: 0, is_sync: true }).map_err(|error| error.to_string())
}
