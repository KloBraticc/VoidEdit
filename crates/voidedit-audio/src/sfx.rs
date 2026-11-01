use std::f32::consts::TAU;

pub struct SfxInfo {
    pub id: String,
    pub name: String,
    pub category: &'static str,
}

#[derive(Clone, Copy)]
enum Voice {
    Sine,
    Square,
    Saw,
    Noise,
    Fm,
    Pluck,
}

struct Recipe {
    name: &'static str,
    category: &'static str,
    voice: Voice,
    pitch: f32,
    end_pitch: f32,
    length: f32,
    attack: f32,
    decay: f32,
    bright: f32,
    repeats: u32,
}

#[allow(clippy::too_many_arguments)]
const fn recipe(name: &'static str, category: &'static str, voice: Voice, pitch: f32, end_pitch: f32, length: f32, attack: f32, decay: f32, bright: f32, repeats: u32) -> Recipe {
    Recipe { name, category, voice, pitch, end_pitch, length, attack, decay, bright, repeats }
}

const RECIPES: [Recipe; 32] = [
    recipe("Whoosh", "Transitions", Voice::Noise, 400.0, 3000.0, 0.8, 0.45, 2.0, 0.6, 1),
    recipe("Swoosh", "Transitions", Voice::Noise, 1800.0, 300.0, 0.5, 0.2, 3.0, 0.7, 1),
    recipe("Riser", "Transitions", Voice::Saw, 90.0, 1400.0, 3.0, 2.6, 0.3, 0.5, 1),
    recipe("Downer", "Transitions", Voice::Saw, 1200.0, 60.0, 2.0, 0.05, 1.5, 0.4, 1),
    recipe("Impact", "Hits", Voice::Noise, 70.0, 40.0, 1.4, 0.002, 4.0, 0.25, 1),
    recipe("Boom", "Hits", Voice::Sine, 80.0, 30.0, 1.8, 0.002, 2.5, 0.1, 1),
    recipe("Punch", "Hits", Voice::Sine, 160.0, 50.0, 0.3, 0.001, 12.0, 0.3, 1),
    recipe("Slam", "Hits", Voice::Noise, 110.0, 60.0, 0.7, 0.001, 7.0, 0.4, 1),
    recipe("Click", "Interface", Voice::Square, 2200.0, 1800.0, 0.05, 0.001, 70.0, 0.8, 1),
    recipe("Tap", "Interface", Voice::Sine, 900.0, 700.0, 0.09, 0.001, 45.0, 0.5, 1),
    recipe("Pop", "Interface", Voice::Sine, 500.0, 1300.0, 0.12, 0.001, 30.0, 0.3, 1),
    recipe("Notification", "Interface", Voice::Fm, 880.0, 880.0, 0.7, 0.005, 6.0, 0.4, 2),
    recipe("Success", "Interface", Voice::Fm, 660.0, 1320.0, 0.8, 0.005, 5.0, 0.4, 3),
    recipe("Error", "Interface", Voice::Square, 220.0, 180.0, 0.5, 0.005, 5.0, 0.4, 2),
    recipe("Beep", "Interface", Voice::Square, 1000.0, 1000.0, 0.25, 0.003, 8.0, 0.5, 1),
    recipe("Laser", "Sci Fi", Voice::Saw, 3000.0, 200.0, 0.4, 0.002, 7.0, 0.8, 1),
    recipe("Zap", "Sci Fi", Voice::Fm, 1800.0, 300.0, 0.25, 0.001, 14.0, 0.9, 1),
    recipe("Warp", "Sci Fi", Voice::Fm, 200.0, 2400.0, 1.2, 0.2, 3.0, 0.6, 1),
    recipe("Engine Hum", "Sci Fi", Voice::Saw, 55.0, 58.0, 3.0, 0.5, 0.6, 0.2, 1),
    recipe("Glitch", "Sci Fi", Voice::Square, 700.0, 2800.0, 0.5, 0.001, 6.0, 0.9, 6),
    recipe("Kick", "Drums", Voice::Sine, 150.0, 45.0, 0.4, 0.001, 10.0, 0.15, 1),
    recipe("Snare", "Drums", Voice::Noise, 900.0, 700.0, 0.25, 0.001, 16.0, 0.8, 1),
    recipe("Hi Hat", "Drums", Voice::Noise, 8000.0, 8000.0, 0.09, 0.001, 45.0, 1.0, 1),
    recipe("Clap", "Drums", Voice::Noise, 1500.0, 1200.0, 0.2, 0.001, 18.0, 0.7, 3),
    recipe("Tom", "Drums", Voice::Sine, 220.0, 110.0, 0.5, 0.001, 8.0, 0.2, 1),
    recipe("Bell", "Musical", Voice::Fm, 1040.0, 1040.0, 2.2, 0.002, 2.0, 0.5, 1),
    recipe("Chime", "Musical", Voice::Fm, 1560.0, 1560.0, 1.6, 0.002, 3.0, 0.6, 3),
    recipe("Pluck", "Musical", Voice::Pluck, 330.0, 330.0, 1.0, 0.001, 4.0, 0.5, 1),
    recipe("Rain", "Ambience", Voice::Noise, 5000.0, 5000.0, 4.0, 0.6, 0.0, 0.85, 1),
    recipe("Wind", "Ambience", Voice::Noise, 600.0, 900.0, 4.0, 1.2, 0.0, 0.2, 1),
    recipe("Static", "Ambience", Voice::Noise, 7000.0, 7000.0, 2.0, 0.02, 0.0, 1.0, 1),
    recipe("Drone", "Ambience", Voice::Saw, 73.0, 73.0, 4.0, 1.0, 0.0, 0.25, 1),
];

struct Variant {
    name: &'static str,
    pitch: f32,
    length: f32,
    bright: f32,
    echo: f32,
    reverse: bool,
    crush: f32,
}

const fn variant(name: &'static str, pitch: f32, length: f32, bright: f32, echo: f32, reverse: bool, crush: f32) -> Variant {
    Variant { name, pitch, length, bright, echo, reverse, crush }
}

const VARIANTS: [Variant; 17] = [
    variant("Standard", 1.0, 1.0, 1.0, 0.0, false, 0.0),
    variant("Short", 1.0, 0.5, 1.0, 0.0, false, 0.0),
    variant("Long", 1.0, 1.8, 1.0, 0.0, false, 0.0),
    variant("High", 1.6, 1.0, 1.2, 0.0, false, 0.0),
    variant("Low", 0.6, 1.1, 0.8, 0.0, false, 0.0),
    variant("Deep", 0.4, 1.3, 0.6, 0.0, false, 0.0),
    variant("Bright", 1.1, 1.0, 1.8, 0.0, false, 0.0),
    variant("Dark", 0.9, 1.0, 0.4, 0.0, false, 0.0),
    variant("Echo", 1.0, 1.0, 1.0, 0.45, false, 0.0),
    variant("Distant", 1.0, 1.2, 0.35, 0.6, false, 0.0),
    variant("Cinematic", 0.7, 1.6, 0.9, 0.5, false, 0.0),
    variant("Reversed", 1.0, 1.0, 1.0, 0.0, true, 0.0),
    variant("Reverse Echo", 1.0, 1.2, 1.0, 0.5, true, 0.0),
    variant("Crushed", 1.0, 1.0, 1.0, 0.0, false, 0.7),
    variant("Retro", 1.2, 0.9, 1.3, 0.0, false, 0.9),
    variant("Fast", 1.3, 0.35, 1.1, 0.0, false, 0.0),
    variant("Slow", 0.8, 2.2, 0.9, 0.0, false, 0.0),
];

pub fn library() -> Vec<SfxInfo> {
    let mut list = Vec::with_capacity(RECIPES.len() * VARIANTS.len());
    for (base, recipe) in RECIPES.iter().enumerate() {
        for (index, variant) in VARIANTS.iter().enumerate() {
            list.push(SfxInfo { id: format!("{base}-{index}"), name: format!("{} {}", recipe.name, variant.name), category: recipe.category });
        }
    }
    list
}

fn random(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed as f32 / u32::MAX as f32 * 2.0 - 1.0
}

pub fn render(id: &str, rate: u32) -> Option<Vec<f32>> {
    let (base, index) = id.split_once('-')?;
    let (base, index) = (base.parse::<usize>().ok()?, index.parse::<usize>().ok()?);
    let (recipe, variant) = (RECIPES.get(base)?, VARIANTS.get(index)?);
    let rate_f = rate.max(1) as f32;
    let frames = ((recipe.length * variant.length * rate_f) as usize).max(16);
    let mut seed = (base as u32 + 1) * 7919 + index as u32 * 104_729 + 1;
    let mut out = vec![0.0f32; frames];
    let repeats = recipe.repeats.max(1);
    let attack = recipe.attack * variant.length.min(1.5);
    let cutoff = (recipe.bright * variant.bright).clamp(0.02, 1.0);
    let mut low = 0.0f32;
    let mut pluck: Vec<f32> = Vec::new();
    let mut pluck_at = 0usize;
    let mut phase = 0.0f32;
    let mut mod_phase = 0.0f32;
    for hit in 0..repeats {
        let begin = frames * hit as usize / repeats as usize;
        let span = frames - begin;
        let step_pitch = 1.0 + hit as f32 * if repeats > 1 { 0.25 } else { 0.0 };
        if matches!(recipe.voice, Voice::Pluck) {
            let period = (rate_f / (recipe.pitch * variant.pitch)).max(2.0) as usize;
            pluck = (0..period).map(|_| random(&mut seed)).collect();
            pluck_at = 0;
        }
        for offset in 0..span {
            let time = offset as f32 / rate_f;
            let progress = offset as f32 / span as f32;
            let frequency = (recipe.pitch + (recipe.end_pitch - recipe.pitch) * progress) * variant.pitch * step_pitch;
            phase = (phase + frequency / rate_f).fract();
            let raw = match recipe.voice {
                Voice::Sine => (phase * TAU).sin(),
                Voice::Square => if phase < 0.5 { 0.7 } else { -0.7 },
                Voice::Saw => phase * 2.0 - 1.0,
                Voice::Noise => {
                    let noise = random(&mut seed);
                    low += (noise - low) * (frequency / rate_f * TAU).clamp(0.005, 1.0);
                    low * 1.6
                }
                Voice::Fm => {
                    mod_phase = (mod_phase + frequency * 2.76 / rate_f).fract();
                    ((phase * TAU) + (mod_phase * TAU).sin() * 2.2 * (-time * 6.0).exp()).sin()
                }
                Voice::Pluck => {
                    let length = pluck.len();
                    let next = (pluck_at + 1) % length;
                    let value = pluck[pluck_at];
                    pluck[pluck_at] = (value + pluck[next]) * 0.498;
                    pluck_at = next;
                    value
                }
            };
            let rise = if attack > 0.0 { (time / attack).min(1.0) } else { 1.0 };
            let release = ((1.0 - progress) * 12.0).min(1.0);
            let envelope = rise * (-time * recipe.decay / variant.length.max(0.2)).exp() * release;
            out[begin + offset] += raw * envelope;
        }
    }
    let mut smooth = 0.0f32;
    for sample in &mut out {
        smooth += (*sample - smooth) * cutoff;
        *sample = smooth;
    }
    if variant.crush > 0.0 {
        let levels = 2.0 + (1.0 - variant.crush) * 30.0;
        let hold = 1 + (variant.crush * 8.0) as usize;
        let mut value = 0.0;
        for (index, sample) in out.iter_mut().enumerate() {
            if index % hold == 0 {
                value = (*sample * levels).round() / levels;
            }
            *sample = value;
        }
    }
    if variant.echo > 0.0 {
        let delay = (0.19 * rate_f) as usize;
        let tail = out.len() + delay * 5;
        out.resize(tail, 0.0);
        for index in delay..tail {
            out[index] += out[index - delay] * variant.echo;
        }
    }
    if variant.reverse {
        out.reverse();
    }
    let peak = out.iter().fold(0.0f32, |peak, sample| peak.max(sample.abs())).max(0.000_1);
    for sample in &mut out {
        *sample = *sample / peak * 0.85;
    }
    Some(out)
}

pub fn wav_bytes(samples: &[f32], rate: u32) -> Vec<u8> {
    let bytes = samples.len() as u32 * 2;
    let mut wav = Vec::with_capacity(44 + bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&bytes.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_has_over_500_distinct_sounds_that_all_render() {
        let list = library();
        assert!(list.len() > 500);
        let mut names: Vec<&str> = list.iter().map(|info| info.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), list.len());
        for info in &list {
            let samples = render(&info.id, 8_000).unwrap();
            assert!(samples.iter().all(|sample| sample.is_finite()), "{}", info.name);
            assert!(samples.iter().any(|sample| sample.abs() > 0.01), "{}", info.name);
        }
    }
}
