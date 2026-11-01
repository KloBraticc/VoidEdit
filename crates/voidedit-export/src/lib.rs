#[cfg(windows)]
mod hardware;

use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use filmcraft_h264enc::{Encoder, EncoderConfig, PacketFormat, RateControl};
use filmcraft_isobmff::{AvcConfig, Brand, Mp4Writer, SampleEntry, TrackConfig, WriteSample, WriterOptions};
use voidedit_core::Project;
use voidedit_media::{decode_now, AudioClip, FrameWant, Gain, Library, Mixer};
use voidedit_render::{render_final, Frame, Image, upscale};

const AUDIO_RATE: u32 = 48_000;

#[derive(Default)]
pub struct Progress {
    pub done: AtomicU32,
    pub total: AtomicU32,
    pub cancel: AtomicBool,
}

pub fn audio_clips(project: &Project) -> Vec<AudioClip> {
    let fps = project.fps.max(1) as f64;
    let mut nested = Vec::new();
    for clip in project.clips.iter().filter(|clip| !clip.muted && !project.tracks.get(clip.track).is_some_and(|track| track.muted)) {
        if let Some(inner) = &clip.nested {
            let (start, end, offset) = (clip.start as f64 / fps, clip.end as f64 / fps, clip.offset as f64 / fps);
            for mut child in audio_clips(inner) {
                let begin = child.start - offset + start;
                child.offset += (start - begin).max(0.0);
                child.start = begin.max(start);
                child.end = (child.end - offset + start).min(end);
                child.gain.start = child.start;
                child.gain.end = child.end;
                if child.end > child.start {
                    nested.push(child);
                }
            }
        }
    }
    let mut clips: Vec<AudioClip> = project
        .clips
        .iter()
        .filter(|clip| !project.tracks.get(clip.track).is_some_and(|track| track.muted))
        .filter(|clip| !clip.muted)
        .filter_map(|clip| {
            let (start, end) = (clip.start as f64 / fps, clip.end as f64 / fps);
            Some(AudioClip {
                path: clip.media.clone()?,
                start,
                end,
                offset: clip.offset as f64 / fps,
                gain: Gain { level: clip.volume.base.max(0.0) / 100.0 * project.tracks.get(clip.track).map_or(1.0, |track| track.volume.max(0.0) / 100.0), start, end, fade_in: clip.fade_in as f64 / fps, fade_out: clip.fade_out as f64 / fps },
                speed: clip.speed,
                reverse: clip.reverse,
                pan: (clip.pan + project.tracks.get(clip.track).map_or(0.0, |track| track.pan)).clamp(-1.0, 1.0),
                bass: clip.bass,
                treble: clip.treble,
                fx: clip.audio_fx.clone(),
                spatial: clip.spatial.clone(),
            })
        })
        .collect();
    clips.append(&mut nested);
    let master = project.master_volume.max(0.0) / 100.0;
    for clip in &mut clips {
        clip.gain.level *= master;
    }
    clips
}

fn to_yuv(frame: &Frame, width: usize, height: usize, y: &mut [u8], u: &mut [u8], v: &mut [u8]) {
    let at = |column: usize, row: usize| {
        let start = (row * frame.width + column) * 4;
        (frame.pixels[start] as i32, frame.pixels[start + 1] as i32, frame.pixels[start + 2] as i32)
    };
    y.par_chunks_mut(width).enumerate().take(height).for_each(|(row, line)| {
        for (column, out) in line.iter_mut().enumerate() {
            let (r, g, b) = at(column, row);
            *out = (((47 * r + 157 * g + 16 * b + 128) >> 8) + 16) as u8;
        }
    });
    let half = width / 2;
    u.par_chunks_mut(half).zip(v.par_chunks_mut(half)).enumerate().take(height / 2).for_each(|(row, (line_u, line_v))| {
        for column in 0..half {
            let (mut r, mut g, mut b) = (0, 0, 0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let pixel = at(column * 2 + dx, row * 2 + dy);
                r += pixel.0;
                g += pixel.1;
                b += pixel.2;
            }
            let (r, g, b) = (r / 4, g / 4, b / 4);
            line_u[column] = (((-26 * r - 87 * g + 112 * b + 128) >> 8) + 128) as u8;
            line_v[column] = (((112 * r - 102 * g - 10 * b + 128) >> 8) + 128) as u8;
        }
    });
}

#[derive(Clone, Copy, PartialEq)]
pub struct ExportOptions {
    pub scale: f32,
    pub quality: f32,
    pub format: ExportFormat,
    pub hardware: bool,
    pub fps: u32,
    pub upscale: u32,
    pub sharpen: f32,
    pub speed: ExportSpeed,
    pub rate: ExportRate,
    pub bitrate: u32,
    pub keyframes: u32,
    pub main_profile: bool,
    pub audio: bool,
    pub audio_kbps: u32,
    pub first_frame: u32,
    pub last_frame: u32,
    pub open_folder: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportSpeed {
    Fastest,
    Balanced,
    Best,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportRate {
    Quality,
    Constant,
    Variable,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ExportFormat {
    Mp4,
    PngSequence,
    Wav,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { scale: 1.0, quality: 20.0, format: ExportFormat::Mp4, hardware: true, fps: 0, upscale: 1, sharpen: 0.4, speed: ExportSpeed::Fastest, rate: ExportRate::Quality, bitrate: 12_000, keyframes: 120, main_profile: false, audio: true, audio_kbps: 192, first_frame: 0, last_frame: 0, open_folder: true }
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
    let mut mixer = Mixer::default();
    while position < samples {
        if progress.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let frames = (samples - position).min(4800) as usize;
        for sample in mixer.mix(library, &clips, &project.master_fx, position, frames, AUDIO_RATE) {
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
        render_final(project, index, scale, &mut frame, &|name| images.get(name).cloned());
        let path = folder.join(format!("frame_{index:06}.png"));
        image::save_buffer(&path, &frame.pixels, frame.width as u32, frame.height as u32, image::ColorType::Rgba8).map_err(|e| e.to_string())?;
        progress.done.store(index + 1, Ordering::Relaxed);
    }
    Ok(())
}

fn gather(project: &Project, library: &Library, index: u32) -> HashMap<String, Arc<Image>> {
    let mut images = HashMap::new();
    let mut found = Vec::new();
    project.wanted_media(index, 0, &mut found);
    for time in project.shutter_times(index, false) {
        project.wanted_media_at(index, time, 0, &mut found);
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    found.dedup();
    for (path, seconds) in found {
        let key = voidedit_core::media_key(&path, seconds);
        if let Some(image) = decode_now(library, &FrameWant { path, seconds, scale: 1.0 }) {
            images.insert(key, image);
        }
    }
    images
}

pub fn export_with(project: &Project, library: &Library, path: &Path, options: ExportOptions, progress: &Arc<Progress>) -> Result<(), String> {
    match options.format {
        ExportFormat::Wav => write_wav(project, library, path, progress),
        ExportFormat::PngSequence => write_png_sequence(project, library, path, options.scale, progress),
        ExportFormat::Mp4 => {
            #[cfg(windows)]
            if options.hardware && hardware::available() {
                match write_hardware(project, library, path, options, progress) {
                    Ok(()) => return Ok(()),
                    Err(error) if error == "cancelled" => return Err(error),
                    Err(_) => progress.done.store(0, Ordering::Relaxed),
                }
            }
            write_mp4(project, library, path, options, progress)
        }
    }
}

pub fn export(project: &Project, library: &Library, path: &Path, progress: &Arc<Progress>) -> Result<(), String> {
    write_mp4(project, library, path, ExportOptions::default(), progress)
}

type Planes = (Vec<u8>, Vec<u8>, Vec<u8>);
const SOURCE_STEPS: f64 = 240.0;
struct FrameRange {
    first: u32,
    project_fps: u32,
    fps: u32,
    total: u32,
}

fn produce_frames(project: &Project, library: &Library, options: ExportOptions, worker_progress: &Arc<Progress>, sender: std::sync::mpsc::SyncSender<Arc<Planes>>, range: FrameRange, size: (usize, usize, f32, usize)) {
    let FrameRange { first, project_fps, fps, total } = range;
    let (width, height, scale, factor) = size;
    let source_of = |position: u32| first + (position as u64 * project_fps as u64 / fps as u64) as u32;
            let batch_size = std::thread::available_parallelism().map_or(4, |count| count.get()).clamp(2, RENDER_AHEAD);
            let reusable = !project.has_motion();
            let mut previous: Option<(Vec<String>, Arc<Planes>)> = None;
            let mut next = 0u32;
            while next < total {
                if worker_progress.cancel.load(Ordering::Relaxed) {
                    return;
                }
                let mut batch: Vec<(Option<u32>, HashMap<String, Arc<Image>>, Vec<String>)> = Vec::new();
                while next < total && batch.len() < batch_size {
                    if worker_progress.cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let index = source_of(next);
                    next += 1;
                    let mut wanted = Vec::new();
                    project.wanted_media(index, 0, &mut wanted);
                    let mut keys: Vec<String> = wanted.iter().map(|(path, seconds)| voidedit_core::media_key(path, (*seconds * SOURCE_STEPS).floor() / SOURCE_STEPS)).collect();
                    keys.extend(project.clips.iter().enumerate().filter(|(_, clip)| index >= clip.start && index < clip.end).map(|(number, _)| number.to_string()));
                    let last_keys = batch.last().map(|item| &item.2).or(previous.as_ref().map(|item| &item.0));
                    if reusable && last_keys == Some(&keys) {
                        batch.push((None, HashMap::new(), keys));
                    } else {
                        batch.push((Some(index), gather(project, library, index), keys));
                    }
                }
                let finished: Vec<Option<Arc<Planes>>> = batch
                    .par_iter()
                    .map(|(index, images, _)| {
                        let index = (*index)?;
                        if worker_progress.cancel.load(Ordering::Relaxed) {
                            return None;
                        }
                        let mut frame = Frame::default();
                        render_final(project, index, scale, &mut frame, &|name| images.get(name).cloned());
                        let (mut y, mut u, mut v) = (vec![0; width * height], vec![0; width * height / 4], vec![0; width * height / 4]);
                        if factor > 1 {
                            let mut enlarged = Frame::default();
                            upscale(&frame, width, height, options.sharpen, &mut enlarged);
                            to_yuv(&enlarged, width, height, &mut y, &mut u, &mut v);
                        } else {
                            to_yuv(&frame, width, height, &mut y, &mut u, &mut v);
                        }
                        Some(Arc::new((y, u, v)))
                    })
                    .collect();
                for ((_, _, keys), planes) in batch.into_iter().zip(finished) {
                    let Some(planes) = planes.or_else(|| previous.as_ref().map(|item| item.1.clone())) else { return };
                    previous = Some((keys, planes.clone()));
                    if sender.send(planes).is_err() {
                        return;
                    }
                }
            }
        }

#[cfg(windows)]
fn write_hardware(project: &Project, library: &Library, path: &Path, options: ExportOptions, progress: &Arc<Progress>) -> Result<(), String> {
    let scale = options.scale.clamp(0.1, 1.0);
    let factor = options.upscale.clamp(1, 4) as usize;
    let width = (((project.width as f32 * scale) as usize * factor) & !1).max(2);
    let height = (((project.height as f32 * scale) as usize * factor) & !1).max(2);
    let project_fps = project.fps.max(1);
    let fps = if options.fps == 0 { project_fps } else { options.fps.min(project_fps) };
    let project_end = project.end_frame().max(1);
    let last = if options.last_frame == 0 { project_end } else { options.last_frame.min(project_end) };
    let first = options.first_frame.min(last.saturating_sub(1));
    let total = ((last - first) as u64 * fps as u64 / project_fps as u64).max(1) as u32;
    progress.total.store(total, Ordering::Relaxed);
    let bitrate_kbps = match options.rate {
        ExportRate::Quality => {
            let bits_per_pixel = 0.15 * 2f64.powf((18.0 - options.quality.clamp(10.0, 40.0) as f64) / 6.0);
            ((width * height) as f64 * fps as f64 * bits_per_pixel / 1000.0).clamp(1000.0, 200_000.0) as u32
        }
        _ => options.bitrate.max(500),
    };
    let clips = audio_clips(project);
    let audio = (!clips.is_empty() && options.audio).then_some((AUDIO_RATE, options.audio_kbps));
    let settings = hardware::VideoSettings { width: width as u32, height: height as u32, fps, bitrate_kbps, main_profile: options.main_profile };
    let mut writer = hardware::HardwareWriter::new(path, &settings, audio).map_err(|error| error.to_string())?;
    let audio_start = first as i64 * AUDIO_RATE as i64 / project_fps as i64;
    let mut audio_position = audio_start;
    let mut mixer = Mixer::default();
    std::thread::scope(|scope| -> Result<(), String> {
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Arc<Planes>>(RENDER_AHEAD);
        let worker_progress = progress.clone();
        scope.spawn(move || produce_frames(project, library, options, &worker_progress, sender, FrameRange { first, project_fps, fps, total }, (width, height, scale, factor)));
        for (position, planes) in receiver.iter().enumerate() {
            if progress.cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            writer.video(&planes.0, &planes.1, &planes.2).map_err(|error| error.to_string())?;
            if audio.is_some() {
                let until = audio_start + (position as i64 + 1) * AUDIO_RATE as i64 / fps as i64;
                let frames = (until - audio_position) as usize;
                let mixed = mixer.mix(library, &clips, &project.master_fx, audio_position, frames, AUDIO_RATE);
                audio_position = until;
                writer.audio(&mixed).map_err(|error| error.to_string())?;
            }
            progress.done.store(position as u32 + 1, Ordering::Relaxed);
        }
        Ok(())
    })?;
    if progress.cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    writer.finish().map_err(|error| error.to_string())
}

pub fn hardware_available() -> bool {
    #[cfg(windows)]
    {
        hardware::available()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

const RENDER_AHEAD: usize = 8;

fn write_mp4(project: &Project, library: &Library, path: &Path, options: ExportOptions, progress: &Arc<Progress>) -> Result<(), String> {
    let scale = options.scale.clamp(0.1, 1.0);
    let factor = options.upscale.clamp(1, 4) as usize;
    let width = ((project.width as f32 * scale) as usize * factor) & !1;
    let height = ((project.height as f32 * scale) as usize * factor) & !1;
    let width = width.max(2);
    let height = height.max(2);
    let project_fps = project.fps.max(1);
    let fps = if options.fps == 0 { project_fps } else { options.fps.min(project_fps) };
    let project_end = project.end_frame().max(1);
    let last = if options.last_frame == 0 { project_end } else { options.last_frame.min(project_end) };
    let first = options.first_frame.min(last.saturating_sub(1));
    let total = ((last - first) as u64 * fps as u64 / project_fps as u64).max(1) as u32;
    progress.total.store(total, Ordering::Relaxed);
    let mut config = EncoderConfig::new(width as u32, height as u32, fps, 1);
    config.format = PacketFormat::LengthPrefixed;
    config.bframes = 0;
    config.rate = match options.rate {
        ExportRate::Quality => RateControl::Crf(options.quality.clamp(10.0, 40.0)),
        ExportRate::Constant => RateControl::Cbr { kbps: options.bitrate.max(100) },
        ExportRate::Variable => RateControl::Vbr { target_kbps: options.bitrate.max(100), max_kbps: options.bitrate.max(100) * 2 },
    };
    config.preset = match options.speed {
        ExportSpeed::Fastest => filmcraft_h264enc::Preset::Speed,
        ExportSpeed::Balanced => filmcraft_h264enc::Preset::Balanced,
        ExportSpeed::Best => filmcraft_h264enc::Preset::Quality,
    };
    config.threads = 0;
    config.slices = if options.speed == ExportSpeed::Fastest { std::thread::available_parallelism().map_or(4, |count| count.get()).min(16) } else { 0 };
    config.keyint = options.keyframes.max(1);
    if options.main_profile {
        config.profile = filmcraft_h264enc::Profile::Main;
    }
    let mut encoder = Encoder::new(config).map_err(|error| format!("{error:?}"))?;
    let file = BufWriter::new(File::create(path).map_err(|error| error.to_string())?);
    let mut writer = Mp4Writer::new(file, WriterOptions::new(Brand::Mp4)).map_err(|error| error.to_string())?;
    let avc = AvcConfig::parse(&encoder.avcc()).map_err(|error| format!("{error:?}"))?;
    let video = writer.add_track(TrackConfig::new(SampleEntry::avc(avc, width as u16, height as u16), fps)).map_err(|error| error.to_string())?;
    let clips = audio_clips(project);
    let mut audio = if clips.is_empty() || !options.audio {
        None
    } else {
        let aac = filmcraft_aac::Encoder::new(filmcraft_aac::EncoderConfig::cbr(AUDIO_RATE, 2, options.audio_kbps.clamp(64, 320) * 1000)).map_err(|error| format!("{error:?}"))?;
        let entry = SampleEntry::aac(aac.audio_specific_config(), 2, AUDIO_RATE);
        let track = writer.add_track(TrackConfig::new(entry, AUDIO_RATE)).map_err(|error| error.to_string())?;
        Some((aac, track))
    };
    let audio_start = first as i64 * AUDIO_RATE as i64 / project_fps as i64;
    let mut audio_position = audio_start;
    let mut mixer = Mixer::default();
    std::thread::scope(|scope| -> Result<(), String> {
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Arc<Planes>>(RENDER_AHEAD);
        let worker_progress = progress.clone();
        scope.spawn(move || produce_frames(project, library, options, &worker_progress, sender, FrameRange { first, project_fps, fps, total }, (width, height, scale, factor)));
        for (position, planes) in receiver.iter().enumerate() {
            let (y, u, v) = &*planes;
            if progress.cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            let input = filmcraft_h264enc::YuvFrame { y, u, v, y_stride: width, uv_stride: width / 2 };
            for packet in encoder.encode(&input, position as i64).map_err(|error| format!("{error:?}"))? {
                write_video(&mut writer, video, &packet)?;
            }
            if let Some((aac, track)) = &mut audio {
                let until = audio_start + (position as i64 + 1) * AUDIO_RATE as i64 / fps as i64;
                let frames = (until - audio_position) as usize;
                let mixed = mixer.mix(library, &clips, &project.master_fx, audio_position, frames, AUDIO_RATE);
                audio_position = until;
                let left: Vec<f32> = mixed.iter().step_by(2).copied().collect();
                let right: Vec<f32> = mixed.iter().skip(1).step_by(2).copied().collect();
                for unit in aac.encode(&[&left, &right]) {
                    write_audio(&mut writer, *track, &unit)?;
                }
            }
            progress.done.store(position as u32 + 1, Ordering::Relaxed);
        }
        Ok(())
    })?;
    if progress.cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
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
