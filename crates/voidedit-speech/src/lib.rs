use std::io::Read;
use std::path::{Path, PathBuf};

use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::{ops::softmax, VarBuilder};
use candle_transformers::models::whisper::{self as whisper, audio, model::Whisper, Config};
use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;

const MODEL_BASE: &str = "https://huggingface.co/openai/whisper-tiny/resolve/169d4a4341b33bc18d8881c4b69c2e104e1cc0af/";
const MODEL_FILES: [(&str, &str); 3] = [("config.json", "ffdccec4f3211f4c63310f2b7098f309fe70f3952cedc5e4d11e43f5b2379b98"), ("tokenizer.json", "27fc476bfe7f17299480be2273fc0608e4d5a99aba2ab5dec5374b4482d1a566"), ("model.safetensors", "7ebd0e69e78190ffe1438491fa05cc1f5c1aa3a4c4db3bc1723adbb551ea2395")];
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
const MAX_TOKENS_PER_WINDOW: usize = 224;

pub const SAMPLE_RATE: usize = whisper::SAMPLE_RATE;

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

pub fn ensure_model(folder: &Path, mut status: impl FnMut(&str)) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    for (name, digest) in MODEL_FILES {
        let target = folder.join(name);
        if target.exists() && matches_digest(&target, digest) {
            continue;
        }
        status(&format!("Downloading the speech model: {name}"));
        let response = ureq::get(&format!("{MODEL_BASE}{name}")).call().map_err(|e| e.to_string())?;
        let temporary = folder.join(format!("{name}.part"));
        let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
        std::io::copy(&mut response.into_reader().take(MAX_DOWNLOAD), &mut file).map_err(|e| e.to_string())?;
        drop(file);
        if !matches_digest(&temporary, digest) {
            let _ = std::fs::remove_file(&temporary);
            return Err(format!("{name} failed its integrity check"));
        }
        std::fs::rename(&temporary, &target).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn matches_digest(path: &Path, digest: &str) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else { return false };
    let mut hasher = Sha256::new();
    if std::io::copy(&mut file, &mut hasher).is_err() {
        return false;
    }
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>() == digest
}

fn hz_to_mel(hz: f64) -> f64 {
    if hz < 1000.0 {
        3.0 * hz / 200.0
    } else {
        15.0 + 27.0 * (hz / 1000.0).ln() / 6.4f64.ln()
    }
}

fn mel_to_hz(mel: f64) -> f64 {
    if mel < 15.0 {
        200.0 * mel / 3.0
    } else {
        1000.0 * 6.4f64.powf((mel - 15.0) / 27.0)
    }
}

pub fn mel_filters(bands: usize, fft: usize, rate: f64) -> Vec<f32> {
    let bins = fft / 2 + 1;
    let top = hz_to_mel(rate / 2.0);
    let points: Vec<f64> = (0..bands + 2).map(|index| mel_to_hz(top * index as f64 / (bands + 1) as f64)).collect();
    let mut filters = vec![0f32; bands * bins];
    for band in 0..bands {
        let (low, middle, high) = (points[band], points[band + 1], points[band + 2]);
        let norm = 2.0 / (high - low);
        for bin in 0..bins {
            let hz = bin as f64 * rate / fft as f64;
            let weight = ((hz - low) / (middle - low)).min((high - hz) / (high - middle)).max(0.0);
            filters[band * bins + bin] = (weight * norm) as f32;
        }
    }
    filters
}

pub fn model_folder(base: &Path) -> PathBuf {
    base.join("whisper-tiny")
}

fn token(tokenizer: &Tokenizer, text: &str) -> Result<u32, String> {
    tokenizer.token_to_id(text).ok_or_else(|| format!("missing token {text}"))
}

pub fn transcribe(folder: &Path, samples: &[f32], mut progress: impl FnMut(f32)) -> Result<Vec<Segment>, String> {
    let error = |e: candle_core::Error| e.to_string();
    let device = Device::Cpu;
    let config: Config = serde_json::from_str(&std::fs::read_to_string(folder.join("config.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let tokenizer = Tokenizer::from_file(folder.join("tokenizer.json")).map_err(|e| e.to_string())?;
    let weights = unsafe { VarBuilder::from_mmaped_safetensors(&[folder.join("model.safetensors")], whisper::DTYPE, &device).map_err(error)? };
    let mut model = Whisper::load(&weights, config.clone()).map_err(error)?;
    let filters = mel_filters(config.num_mel_bins, whisper::N_FFT, SAMPLE_RATE as f64);
    let mel = audio::pcm_to_mel(&config, samples, &filters);
    let frames = mel.len() / config.num_mel_bins;
    let mel = Tensor::from_vec(mel, (1, config.num_mel_bins, frames), &device).map_err(error)?;
    let start_of_text = token(&tokenizer, whisper::SOT_TOKEN)?;
    let english = token(&tokenizer, "<|en|>")?;
    let task = token(&tokenizer, whisper::TRANSCRIBE_TOKEN)?;
    let end_of_text = token(&tokenizer, whisper::EOT_TOKEN)?;
    let no_timestamps = token(&tokenizer, whisper::NO_TIMESTAMPS_TOKEN)?;
    let mut suppress = vec![0f32; config.vocab_size];
    for id in &config.suppress_tokens {
        if let Some(slot) = suppress.get_mut(*id as usize) {
            *slot = f32::NEG_INFINITY;
        }
    }
    suppress[no_timestamps as usize] = f32::NEG_INFINITY;
    let suppress = Tensor::new(suppress.as_slice(), &device).map_err(error)?;
    let mut segments = Vec::new();
    let mut seek = 0usize;
    while seek < frames {
        let window = (frames - seek).min(whisper::N_FRAMES);
        let time_offset = (seek * whisper::HOP_LENGTH) as f64 / SAMPLE_RATE as f64;
        let part = mel.narrow(2, seek, window).map_err(error)?;
        let features = audio_pad(&part, config.num_mel_bins, &device).map_err(error)?;
        let encoded = model.encoder.forward(&features, true).map_err(error)?;
        let mut tokens = vec![start_of_text, english, task];
        for step in 0..MAX_TOKENS_PER_WINDOW {
            let input = Tensor::new(tokens.as_slice(), &device).and_then(|t| t.unsqueeze(0)).map_err(error)?;
            let decoded = model.decoder.forward(&input, &encoded, step == 0).map_err(error)?;
            let (_, count, _) = decoded.dims3().map_err(error)?;
            let logits = model.decoder.final_linear(&decoded.i((.., count - 1..)).map_err(error)?).map_err(error)?.i((0, 0)).map_err(error)?;
            let logits = (&logits + &suppress).map_err(error)?;
            let probabilities = softmax(&logits, 0).map_err(error)?.to_dtype(DType::F32).map_err(error)?.to_vec1::<f32>().map_err(error)?;
            let next = probabilities.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map_or(end_of_text, |(index, _)| index as u32);
            tokens.push(next);
            if next == end_of_text {
                break;
            }
        }
        segments.extend(segments_from_tokens(&tokenizer, &tokens[3..], no_timestamps + 1, end_of_text, time_offset));
        seek += window;
        progress(seek as f32 / frames.max(1) as f32);
    }
    Ok(segments)
}

fn audio_pad(part: &Tensor, bands: usize, device: &Device) -> candle_core::Result<Tensor> {
    let (_, _, frames) = part.dims3()?;
    if frames >= whisper::N_FRAMES {
        return Ok(part.clone());
    }
    let padding = Tensor::zeros((1, bands, whisper::N_FRAMES - frames), part.dtype(), device)?;
    Tensor::cat(&[part, &padding], 2)
}

fn segments_from_tokens(tokenizer: &Tokenizer, tokens: &[u32], first_time: u32, end: u32, offset: f64) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut text = Vec::new();
    let mut started = offset;
    for &id in tokens {
        if id == end {
            break;
        }
        if id >= first_time {
            let time = offset + (id - first_time) as f64 * 0.02;
            if text.is_empty() {
                started = time;
            } else {
                push_segment(tokenizer, &mut out, &mut text, started, time);
                started = time;
            }
        } else {
            text.push(id);
        }
    }
    if !text.is_empty() {
        let finish = started + 3.0;
        push_segment(tokenizer, &mut out, &mut text, started, finish);
    }
    out
}

fn push_segment(tokenizer: &Tokenizer, out: &mut Vec<Segment>, text: &mut Vec<u32>, start: f64, end: f64) {
    if let Ok(decoded) = tokenizer.decode(text, true) {
        let cleaned = decoded.trim();
        if !cleaned.is_empty() && end > start {
            out.push(Segment { start, end, text: cleaned.to_string() });
        }
    }
    text.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_have_expected_shape() {
        let filters = mel_filters(80, 400, 16_000.0);
        assert_eq!(filters.len(), 80 * 201);
        assert!(filters.iter().all(|value| value.is_finite() && *value >= 0.0));
        assert!(filters.iter().any(|value| *value > 0.0));
    }

    #[test]
    fn transcribes_a_given_file_when_one_is_given() {
        let (Ok(folder), Ok(raw)) = (std::env::var("VOIDEDIT_MODEL"), std::env::var("VOIDEDIT_PCM")) else { return };
        let bytes = std::fs::read(raw).unwrap();
        let samples: Vec<f32> = bytes.chunks_exact(2).map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0).collect();
        ensure_model(folder.as_ref(), |_| {}).unwrap();
        let started = std::time::Instant::now();
        let result = transcribe(folder.as_ref(), &samples, |_| {}).unwrap();
        println!("{result:?} in {:?}", started.elapsed());
        assert!(!result.is_empty());
    }
}
