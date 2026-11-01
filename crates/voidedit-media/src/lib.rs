use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use filmcraft_media::{FrameRequest, SharedSource};
use filmcraft_time::Tick;
use voidedit_render::Image;

const AUDIO_CHUNK_SECONDS: f64 = 0.05;
const AUDIO_BUFFER_SECONDS: f64 = 0.2;

pub struct MediaDetails {
    pub width: u32,
    pub height: u32,
    pub seconds: f64,
    pub has_video: bool,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Default, Clone)]
pub struct Library {
    sources: Arc<Mutex<HashMap<String, Option<SharedSource>>>>,
}

impl Library {
    pub fn source(&self, path: &str) -> Option<SharedSource> {
        if let Some(found) = lock(&self.sources).get(path) {
            return found.clone();
        }
        let opened = open(path);
        lock(&self.sources).insert(path.to_string(), opened.clone());
        opened
    }

    pub fn details(&self, path: &str) -> Option<MediaDetails> {
        let source = self.source(path)?;
        let info = source.info();
        let video = info.video.as_ref();
        Some(MediaDetails {
            width: video.map_or(0, |stream| stream.width),
            height: video.map_or(0, |stream| stream.height),
            seconds: info.duration.0 as f64 / filmcraft_time::TICKS_PER_SECOND as f64,
            has_video: video.is_some(),
        })
    }
}

fn open(path: &str) -> Option<SharedSource> {
    let result = std::panic::catch_unwind(|| {
        let file = Path::new(path);
        filmcraft_media::open_path(file, &filmcraft_codecs::openers())
    });
    match result {
        Ok(Ok(source)) => Some(source),
        _ => None,
    }
}

#[derive(Clone, PartialEq)]
pub struct FrameWant {
    pub path: String,
    pub seconds: f64,
    pub scale: f32,
}

#[derive(Default)]
struct FrameState {
    wants: Vec<FrameWant>,
    ready: HashMap<String, (f64, Arc<Image>)>,
}

#[derive(Clone)]
pub struct Frames {
    state: Arc<(Mutex<FrameState>, Condvar)>,
    generation: Arc<AtomicU64>,
}

impl Frames {
    pub fn new(library: Library, on_ready: Box<dyn Fn() + Send>) -> Self {
        let frames = Self { state: Arc::default(), generation: Arc::default() };
        let worker = frames.clone();
        let _ = std::thread::Builder::new().name("decode".into()).spawn(move || worker.run(library, on_ready));
        frames
    }

    pub fn want(&self, wants: Vec<FrameWant>) {
        let (state, signal) = &*self.state;
        let mut state = lock(state);
        if state.wants != wants {
            state.ready.retain(|key, _| wants.iter().any(|want| key.split('\u{0}').next() == Some(want.path.as_str())));
            state.wants = wants;
            signal.notify_one();
        }
    }

    pub fn latest(&self, key: &str) -> Option<Arc<Image>> {
        let state = lock(&self.state.0);
        if let Some((_, image)) = state.ready.get(key) {
            return Some(image.clone());
        }
        let path = key.split('\u{0}').next()?;
        let target = key.rsplit('\u{0}').next()?.parse::<f64>().ok()? / 1000.0;
        state
            .ready
            .iter()
            .filter(|(other, _)| other.split('\u{0}').next() == Some(path))
            .min_by(|a, b| (a.1 .0 - target).abs().total_cmp(&(b.1 .0 - target).abs()))
            .map(|(_, (_, image))| image.clone())
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    fn run(self, library: Library, on_ready: Box<dyn Fn() + Send>) {
        let (state, signal) = &*self.state;
        loop {
            let next = {
                let mut guard = lock(state);
                loop {
                    let pending = guard.wants.iter().find(|want| !guard.ready.contains_key(&voidedit_core::media_key(&want.path, want.seconds))).cloned();
                    match pending {
                        Some(want) => break want,
                        None => guard = signal.wait(guard).unwrap_or_else(|poisoned| poisoned.into_inner()),
                    }
                }
            };
            let image = library.source(&next.path).and_then(|source| decode(&source, &next));
            let image = image.unwrap_or_else(|| Arc::new(Image { width: 1, height: 1, pixels: vec![0, 0, 0, 0] }));
            {
                let mut guard = lock(state);
                guard.ready.insert(voidedit_core::media_key(&next.path, next.seconds), (next.seconds, image));
                let limit = guard.wants.len() * 2 + 2;
                while guard.ready.len() > limit {
                    let wants = guard.wants.clone();
                    let stale = guard.ready.keys().find(|key| !wants.iter().any(|want| &voidedit_core::media_key(&want.path, want.seconds) == *key)).cloned();
                    match stale {
                        Some(key) => {
                            guard.ready.remove(&key);
                        }
                        None => break,
                    }
                }
            }
            self.generation.fetch_add(1, Ordering::Relaxed);
            on_ready();
        }
    }
}

pub fn decode_now(library: &Library, want: &FrameWant) -> Option<Arc<Image>> {
    decode(&library.source(&want.path)?, want)
}

pub fn mono_samples(library: &Library, path: &str, rate: u32) -> Option<Vec<f32>> {
    let source = library.source(path)?;
    let info = source.info();
    info.audio()?;
    let total = (info.duration.0 as f64 / filmcraft_time::TICKS_PER_SECOND as f64 * rate as f64) as usize;
    let mut samples = Vec::with_capacity(total);
    while samples.len() < total {
        let frames = (total - samples.len()).min(rate as usize * 10);
        let buffer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| source.audio(samples.len() as i64, frames, rate))).ok()?.ok()?;
        let channels = buffer.channels.len().max(1) as f32;
        for index in 0..frames {
            samples.push(buffer.channels.iter().filter_map(|channel| channel.get(index)).sum::<f32>() / channels);
        }
    }
    Some(samples)
}

pub fn waveform(library: &Library, path: &str, points: usize) -> Option<Vec<f32>> {
    let source = library.source(path)?;
    let info = source.info();
    info.audio()?;
    let rate = 8_000u32;
    let total = (info.duration.0 as f64 / filmcraft_time::TICKS_PER_SECOND as f64 * rate as f64) as usize;
    let step = (total / points.max(1)).max(1);
    let mut peaks = Vec::with_capacity(points);
    let mut start = 0usize;
    while start < total && peaks.len() < points {
        let frames = step.min(400);
        let buffer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| source.audio(start as i64, frames, rate))).ok()?.ok()?;
        peaks.push(buffer.channels.iter().flat_map(|channel| channel.iter()).fold(0.0f32, |peak, sample| peak.max(sample.abs())));
        start += step;
    }
    Some(peaks)
}

fn decode(source: &SharedSource, want: &FrameWant) -> Option<Arc<Image>> {
    let request = FrameRequest { time: Tick::from_seconds_f64(want.seconds), scale: want.scale };
    let frame = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| source.video_frame(request))).ok()?.ok()?;
    let pixels = frame.to_rgba8();
    Some(Arc::new(Image { width: frame.width as usize, height: frame.height as usize, pixels }))
}

#[derive(Clone)]
pub struct AudioClip {
    pub path: String,
    pub start: f64,
    pub end: f64,
    pub offset: f64,
    pub gain: Gain,
    pub speed: f32,
    pub reverse: bool,
    pub pan: f32,
    pub bass: f32,
    pub treble: f32,
}

#[derive(Clone)]
pub struct Gain {
    pub level: f32,
    pub start: f64,
    pub end: f64,
    pub fade_in: f64,
    pub fade_out: f64,
}

impl Gain {
    pub fn at(&self, time: f64) -> f32 {
        let mut fade = 1.0f64;
        if self.fade_in > 0.0 {
            fade = fade.min((time - self.start) / self.fade_in);
        }
        if self.fade_out > 0.0 {
            fade = fade.min((self.end - time) / self.fade_out);
        }
        self.level * fade.clamp(0.0, 1.0) as f32
    }
}

#[derive(Clone)]
pub struct Audio {
    clips: Arc<Mutex<Vec<AudioClip>>>,
    ring: Arc<Mutex<VecDeque<f32>>>,
    playing: Arc<AtomicBool>,
    position: Arc<AtomicI64>,
    rate: Arc<AtomicU32>,
    peak: Arc<AtomicU32>,
}

impl Audio {
    pub fn new(library: Library) -> Self {
        let audio = Self {
            clips: Arc::default(),
            ring: Arc::default(),
            playing: Arc::default(),
            position: Arc::default(),
            rate: Arc::new(AtomicU32::new(48_000)),
            peak: Arc::default(),
        };
        let worker = audio.clone();
        let _ = std::thread::Builder::new().name("audio".into()).spawn(move || worker.run(library));
        audio
    }

    pub fn set_clips(&self, clips: Vec<AudioClip>) {
        *lock(&self.clips) = clips;
    }

    pub fn play(&self, seconds: f64) {
        lock(&self.ring).clear();
        let rate = self.rate.load(Ordering::Relaxed) as f64;
        self.position.store((seconds * rate) as i64, Ordering::Relaxed);
        self.playing.store(true, Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.playing.store(false, Ordering::Relaxed);
        lock(&self.ring).clear();
    }

    pub fn peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }

    fn open_output(&self) -> Option<cpal::Stream> {
        let device = cpal::default_host().default_output_device()?;
        let config = device.default_output_config().ok()?;
        self.rate.store(config.sample_rate().0, Ordering::Relaxed);
        let channels = config.channels() as usize;
        let ring = self.ring.clone();
        let peak = self.peak.clone();
        let stream = device
            .build_output_stream(
                &config.into(),
                move |output: &mut [f32], _| {
                    let mut ring = lock(&ring);
                    let mut loudest = f32::from_bits(peak.load(Ordering::Relaxed));
                    for frame in output.chunks_mut(channels.max(1)) {
                        let left = ring.pop_front().unwrap_or(0.0);
                        let right = ring.pop_front().unwrap_or(left);
                        loudest = loudest.max(left.abs()).max(right.abs());
                        for (index, sample) in frame.iter_mut().enumerate() {
                            *sample = if index % 2 == 0 { left } else { right };
                        }
                    }
                    peak.store(loudest.to_bits(), Ordering::Relaxed);
                },
                |_| {},
                None,
            )
            .ok()?;
        stream.play().ok()?;
        Some(stream)
    }

    fn run(self, library: Library) {
        let _stream = self.open_output();
        loop {
            let rate = self.rate.load(Ordering::Relaxed);
            let target = (rate as f64 * AUDIO_BUFFER_SECONDS) as usize * 2;
            if !self.playing.load(Ordering::Relaxed) || lock(&self.ring).len() >= target {
                std::thread::sleep(Duration::from_millis(15));
                continue;
            }
            let frames = (rate as f64 * AUDIO_CHUNK_SECONDS) as usize;
            let start = self.position.fetch_add(frames as i64, Ordering::Relaxed);
            let clips = lock(&self.clips).clone();
            let mixed = mix(&library, &clips, start, frames, rate);
            if self.playing.load(Ordering::Relaxed) {
                lock(&self.ring).extend(mixed);
            }
        }
    }
}

pub fn mix(library: &Library, clips: &[AudioClip], start: i64, frames: usize, rate: u32) -> Vec<f32> {
    let mut mixed = vec![0.0f32; frames * 2];
    let time = start as f64 / rate as f64;
    let length = frames as f64 / rate as f64;
    for clip in clips {
        if time + length <= clip.start || time >= clip.end {
            continue;
        }
        let Some(source) = library.source(&clip.path) else { continue };
        if source.info().audio().is_none() {
            continue;
        }
        let speed = clip.speed.clamp(0.1, 16.0) as f64;
        let elapsed = if clip.reverse { clip.end - time - length } else { time - clip.start };
        let from = ((elapsed.max(0.0) * speed + clip.offset) * rate as f64) as i64;
        let wanted = ((frames as f64 * speed).ceil() as usize).max(1);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| source.audio(from.max(0), wanted, rate)));
        let Ok(Ok(buffer)) = result else { continue };
        let left = buffer.channels.first();
        let right = buffer.channels.get(1).or(left);
        if let (Some(left), Some(right)) = (left, right) {
            let (pan_left, pan_right) = ((1.0 - clip.pan).min(1.0), (1.0 + clip.pan).min(1.0));
            let (bass, treble) = (10f32.powf(clip.bass / 20.0), 10f32.powf(clip.treble / 20.0));
            let (mut low_left, mut low_right) = (0.0f32, 0.0f32);
            for index in 0..frames {
                let position = if clip.reverse { frames - 1 - index } else { index };
                let source_index = (position as f64 * speed) as usize;
                let (Some(l), Some(r)) = (left.get(source_index), right.get(source_index)) else { break };
                let gain = clip.gain.at(time + index as f64 / rate as f64);
                low_left += (l - low_left) * 0.05;
                low_right += (r - low_right) * 0.05;
                let shaped_left = low_left * bass + (l - low_left) * treble;
                let shaped_right = low_right * bass + (r - low_right) * treble;
                mixed[index * 2] += shaped_left * gain * pan_left;
                mixed[index * 2 + 1] += shaped_right * gain * pan_right;
            }
        }
    }
    for sample in &mut mixed {
        *sample = sample.clamp(-1.0, 1.0);
    }
    mixed
}

pub fn match_patch(previous: &Image, next: &Image, center: (i64, i64), patch: i64, search: i64) -> Option<(i64, i64)> {
    let luma = |image: &Image, x: i64, y: i64| -> Option<i64> {
        if x < 0 || y < 0 || x >= image.width as i64 || y >= image.height as i64 {
            return None;
        }
        let start = (y as usize * image.width + x as usize) * 4;
        let pixel = image.pixels.get(start..start + 3)?;
        Some(pixel[0] as i64 * 2 + pixel[1] as i64 * 5 + pixel[2] as i64)
    };
    let mut best: Option<(i64, (i64, i64))> = None;
    for dy in -search..=search {
        for dx in -search..=search {
            let mut score = 0i64;
            let mut valid = true;
            'patch: for py in -patch..=patch {
                for px in -patch..=patch {
                    let (Some(a), Some(b)) = (luma(previous, center.0 + px, center.1 + py), luma(next, center.0 + dx + px, center.1 + dy + py)) else {
                        valid = false;
                        break 'patch;
                    };
                    score += (a - b).abs();
                }
            }
            let penalty = (dx.abs() + dy.abs()) * 2;
            if valid && best.is_none_or(|(top, _)| score + penalty < top) {
                best = Some((score + penalty, (center.0 + dx, center.1 + dy)));
            }
        }
    }
    best.map(|(_, point)| point)
}

pub fn track_point(library: &Library, path: &str, fps: u32, first: u32, last: u32, start: (i64, i64), mut report: impl FnMut(u32, (i64, i64)) -> bool) -> Option<()> {
    let source = library.source(path)?;
    let want = |frame: u32| FrameWant { path: path.to_string(), seconds: frame as f64 / fps.max(1) as f64, scale: 1.0 };
    let mut previous = decode(&source, &want(first))?;
    let mut point = start;
    for frame in first + 1..last {
        let next = decode(&source, &want(frame))?;
        point = match_patch(&previous, &next, point, 12, 24)?;
        if !report(frame, point) {
            break;
        }
        previous = next;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracking_follows_a_moving_square() {
        let image = |x0: usize| {
            let mut pixels = vec![0u8; 128 * 128 * 4];
            for y in 50..70 {
                for x in x0..x0 + 20 {
                    pixels[(y * 128 + x) * 4] = 255;
                    pixels[(y * 128 + x) * 4 + 1] = 255;
                }
            }
            Image { width: 128, height: 128, pixels }
        };
        assert_eq!(match_patch(&image(40), &image(47), (50, 60), 12, 24), Some((57, 60)));
    }

    #[test]
    fn missing_files_do_not_crash() {
        let library = Library::default();
        assert!(library.source("does-not-exist.mp4").is_none());
        assert!(library.details("does-not-exist.mp4").is_none());
    }

    #[test]
    fn mix_with_no_clips_is_silent() {
        let mixed = mix(&Library::default(), &[], 0, 8, 48_000);
        assert_eq!(mixed, vec![0.0; 16]);
    }
}
