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
        .filter(|clip| !clip.reverse && !clip.muted)
        .filter_map(|clip| {
            let (start, end) = (clip.start as f64 / fps, clip.end as f64 / fps);
            Some(AudioClip {
                path: clip.media.clone()?,
                start,
                end,
                offset: clip.offset as f64 / fps,
                gain: Gain { level: clip.volume.base.max(0.0) / 100.0, start, end, fade_in: clip.fade_in as f64 / fps, fade_out: clip.fade_out as f64 / fps },
                speed: clip.speed,
                pan: clip.pan.clamp(-1.0, 1.0),
                bass: clip.bass,
                treble: clip.treble,
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

#[derive(Clone, Copy, PartialEq)]
pub struct ExportOptions {
    pub scale: f32,
    pub quality: f32,
    pub format: ExportFormat,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ExportFormat {
    Mp4,
    PngSequence,
    Wav,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { scale: 1.0, quality: 20.0, format: ExportFormat::Mp4 }
    }
}

fn write_wav(project: &Project, library: &Library, path: &Path, progress: &Arc<Progress>) -> Result<(), String> {
    use std::io::Write;
    let clips = audio_clips(project);
    let total = project.end_frame().max(1);
    progress.total.store(total, Ordering::Relaxed);
    let samples = total as i64 * AUDIO_RATE as i64 / project.fps.max(1) as i64;
    let mut file = BufWriter::new(File::create(path).map_err(|e| e.to_string())?);
    let bytes = (samples * 4) as u32;
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + bytes).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&AUDIO_RATE.to_le_bytes());
    header.extend_from_slice(&(AUDIO_RATE * 4).to_le_bytes());
    header.extend_from_slice(&4u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&bytes.to_le_bytes());
    file.write_all(&header).map_err(|e| e.to_string())?;
    let mut position = 0i64;
    while position < samples {
        if progress.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let frames = (samples - position).min(4800) as usize;
        for sample in mix(library, &clips, position, frames, AUDIO_RATE) {
            file.write_all(&((sample * 32767.0) as i16).to_le_bytes()).map_err(|e| e.to_string())?;
        }
        position += frames as i64;
        progress.done.store((position * total as i64 / samples.max(1)) as u32, Ordering::Relaxed);
    }
    file.flush().map_err(|e| e.to_string())
}

fn write_png_sequence(project: &Project, library: &Library, folder: &Path, scale: f32, progress: &Arc<Progress>) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let total = project.end_frame().max(1);
    progress.total.store(total, Ordering::Relaxed);
    let mut frame = Frame::default();
    for index in 0..total {
        if progress.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let images = gather(project, library, index);
        render(project, index, scale, &mut frame, &|name| images.get(name).cloned());
        let path = folder.join(format!("frame_{index:06}.png"));
        image::save_buffer(&path, &frame.pixels, frame.width as u32, frame.height as u32, image::ColorType::Rgba8).map_err(|e| e.to_string())?;
        progress.done.store(index + 1, Ordering::Relaxed);
    }
    Ok(())
}

fn gather(project: &Project, library: &Library, index: u32) -> HashMap<String, Arc<Image>> {
    let mut images = HashMap::new();
    for clip in project.clips.iter().filter(|clip| wanted_frame(project, clip, index)) {
        if let Some(media) = &clip.media {
            let want = FrameWant { path: media.clone(), seconds: clip.media_seconds(index, project.fps.max(1)), scale: 1.0 };
            if let Some(image) = decode_now(library, &want) {
                images.insert(media.clone(), image);
            }
        }
    }
    images
}

pub fn export_with(project: &Project, library: &Library, path: &Path, options: ExportOptions, progress: &Arc<Progress>) -> Result<(), String> {
    match options.format {
        ExportFormat::Wav => write_wav(project, library, path, progress),
        ExportFormat::PngSequence => write_png_sequence(project, library, path, options.scale, progress),
        ExportFormat::Mp4 => write_mp4(project, library, path, options, progress),
    }
}

pub fn export(project: &Project, library: &Library, path: &Path, progress: &Arc<Progress>) -> Result<(), String> {
    write_mp4(project, library, path, ExportOptions::default(), progress)
}

fn write_mp4(project: &Project, library: &Library, path: &Path, options: ExportOptions, progress: &Arc<Progress>) -> Result<(), String> {
    let scale = options.scale.clamp(0.1, 1.0);
    let width = ((project.width as f32 * scale) as usize) & !1;
    let height = ((project.height as f32 * scale) as usize) & !1;
    let width = width.max(2);
    let height = height.max(2);
    let fps = project.fps.max(1);
    let total = project.end_frame().max(1);
    progress.total.store(total, Ordering::Relaxed);
    let mut config = EncoderConfig::new(width as u32, height as u32, fps, 1);
    config.format = PacketFormat::LengthPrefixed;
    config.bframes = 0;
    config.rate = RateControl::Crf(options.quality.clamp(10.0, 40.0));
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
        let images = gather(project, library, index);
        render(project, index, scale, &mut frame, &|name| images.get(name).cloned());
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
