use std::f32::consts::{PI, TAU};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Gain,
    Balance,
    Mono,
    SwapChannels,
    InvertPhase,
    DcRemove,
    LowPass,
    HighPass,
    BandPass,
    Notch,
    LowShelf,
    HighShelf,
    PeakEq,
    ParametricEq,
    GraphicEq,
    Telephone,
    Compressor,
    Limiter,
    Gate,
    Expander,
    DeEsser,
    Transient,
    Delay,
    PingPong,
    Slapback,
    Reverb,
    Hall,
    Room,
    Plate,
    Chorus,
    Flanger,
    Phaser,
    Tremolo,
    Vibrato,
    AutoPan,
    RingMod,
    Overdrive,
    Fuzz,
    Bitcrush,
    Exciter,
    PitchShift,
    Width,
    Haas,
}

type Param = (&'static str, f32, f32, f32);

pub struct Spec {
    pub kind: Kind,
    pub name: &'static str,
    pub category: &'static str,
    pub params: &'static [Param],
}

const fn spec(kind: Kind, name: &'static str, category: &'static str, params: &'static [Param]) -> Spec {
    Spec { kind, name, category, params }
}

const FREQ_Q: &[Param] = &[("Frequency Hz", 20.0, 20000.0, 1000.0), ("Q", 0.1, 18.0, 0.7)];
const SHELF: &[Param] = &[("Frequency Hz", 20.0, 20000.0, 1000.0), ("Gain dB", -24.0, 24.0, 0.0)];
const VERB: &[Param] = &[("Size", 0.0, 1.0, 0.6), ("Damping", 0.0, 1.0, 0.4), ("Width", 0.0, 1.0, 1.0), ("Mix", 0.0, 1.0, 0.3)];
const MODULATION: &[Param] = &[("Rate Hz", 0.01, 20.0, 1.0), ("Depth", 0.0, 1.0, 0.5), ("Feedback", 0.0, 0.95, 0.3), ("Mix", 0.0, 1.0, 0.5)];

pub const SPECS: &[Spec] = &[
    spec(Kind::Gain, "Gain", "Utility", &[("Gain dB", -60.0, 24.0, 0.0)]),
    spec(Kind::Balance, "Balance", "Utility", &[("Balance", -1.0, 1.0, 0.0)]),
    spec(Kind::Mono, "Mono", "Utility", &[]),
    spec(Kind::SwapChannels, "Swap Channels", "Utility", &[]),
    spec(Kind::InvertPhase, "Invert Phase", "Utility", &[("Left", 0.0, 1.0, 1.0), ("Right", 0.0, 1.0, 1.0)]),
    spec(Kind::DcRemove, "DC Remove", "Utility", &[]),
    spec(Kind::LowPass, "Low Pass", "Filter", FREQ_Q),
    spec(Kind::HighPass, "High Pass", "Filter", FREQ_Q),
    spec(Kind::BandPass, "Band Pass", "Filter", FREQ_Q),
    spec(Kind::Notch, "Notch", "Filter", FREQ_Q),
    spec(Kind::Telephone, "Telephone", "Filter", &[("Low Hz", 100.0, 1000.0, 400.0), ("High Hz", 1000.0, 8000.0, 3000.0)]),
    spec(Kind::LowShelf, "Low Shelf", "EQ", SHELF),
    spec(Kind::HighShelf, "High Shelf", "EQ", SHELF),
    spec(Kind::PeakEq, "Peak EQ", "EQ", &[("Frequency Hz", 20.0, 20000.0, 1000.0), ("Gain dB", -24.0, 24.0, 0.0), ("Q", 0.1, 18.0, 1.0)]),
    spec(Kind::ParametricEq, "Parametric EQ", "EQ", &[("Low dB", -24.0, 24.0, 0.0), ("Low Mid Hz", 80.0, 1000.0, 300.0), ("Low Mid dB", -24.0, 24.0, 0.0), ("Mid Hz", 300.0, 5000.0, 1200.0), ("Mid dB", -24.0, 24.0, 0.0), ("High Mid Hz", 1000.0, 16000.0, 5000.0), ("High Mid dB", -24.0, 24.0, 0.0), ("High dB", -24.0, 24.0, 0.0)]),
    spec(Kind::GraphicEq, "Graphic EQ", "EQ", &[("31 Hz", -24.0, 24.0, 0.0), ("62 Hz", -24.0, 24.0, 0.0), ("125 Hz", -24.0, 24.0, 0.0), ("250 Hz", -24.0, 24.0, 0.0), ("500 Hz", -24.0, 24.0, 0.0), ("1 kHz", -24.0, 24.0, 0.0), ("2 kHz", -24.0, 24.0, 0.0), ("4 kHz", -24.0, 24.0, 0.0), ("8 kHz", -24.0, 24.0, 0.0), ("16 kHz", -24.0, 24.0, 0.0)]),
    spec(Kind::Compressor, "Compressor", "Dynamics", &[("Threshold dB", -60.0, 0.0, -18.0), ("Ratio", 1.0, 20.0, 4.0), ("Attack ms", 0.1, 200.0, 10.0), ("Release ms", 5.0, 2000.0, 120.0), ("Makeup dB", 0.0, 30.0, 0.0)]),
    spec(Kind::Limiter, "Limiter", "Dynamics", &[("Ceiling dB", -24.0, 0.0, -1.0), ("Release ms", 5.0, 1000.0, 80.0)]),
    spec(Kind::Gate, "Noise Gate", "Dynamics", &[("Threshold dB", -90.0, 0.0, -45.0), ("Attack ms", 0.1, 100.0, 2.0), ("Release ms", 5.0, 2000.0, 120.0), ("Floor dB", -90.0, 0.0, -80.0)]),
    spec(Kind::Expander, "Expander", "Dynamics", &[("Threshold dB", -80.0, 0.0, -35.0), ("Ratio", 1.0, 10.0, 2.0), ("Attack ms", 0.1, 100.0, 5.0), ("Release ms", 5.0, 2000.0, 150.0)]),
    spec(Kind::DeEsser, "De Esser", "Dynamics", &[("Frequency Hz", 2000.0, 12000.0, 6000.0), ("Threshold dB", -60.0, 0.0, -28.0), ("Amount", 0.0, 1.0, 0.7)]),
    spec(Kind::Transient, "Transient Shaper", "Dynamics", &[("Attack", -1.0, 1.0, 0.0), ("Sustain", -1.0, 1.0, 0.0)]),
    spec(Kind::Delay, "Delay", "Time", &[("Time ms", 1.0, 2000.0, 350.0), ("Feedback", 0.0, 0.95, 0.35), ("Mix", 0.0, 1.0, 0.3)]),
    spec(Kind::PingPong, "Ping Pong Delay", "Time", &[("Time ms", 1.0, 2000.0, 300.0), ("Feedback", 0.0, 0.95, 0.45), ("Mix", 0.0, 1.0, 0.3)]),
    spec(Kind::Slapback, "Slapback", "Time", &[("Time ms", 20.0, 200.0, 90.0), ("Mix", 0.0, 1.0, 0.35)]),
    spec(Kind::Reverb, "Reverb", "Reverb", VERB),
    spec(Kind::Hall, "Concert Hall", "Reverb", &[("Size", 0.0, 1.0, 0.92), ("Damping", 0.0, 1.0, 0.3), ("Width", 0.0, 1.0, 1.0), ("Mix", 0.0, 1.0, 0.35)]),
    spec(Kind::Room, "Small Room", "Reverb", &[("Size", 0.0, 1.0, 0.3), ("Damping", 0.0, 1.0, 0.6), ("Width", 0.0, 1.0, 0.7), ("Mix", 0.0, 1.0, 0.2)]),
    spec(Kind::Plate, "Plate", "Reverb", &[("Size", 0.0, 1.0, 0.7), ("Damping", 0.0, 1.0, 0.1), ("Width", 0.0, 1.0, 1.0), ("Mix", 0.0, 1.0, 0.3)]),
    spec(Kind::Chorus, "Chorus", "Modulation", MODULATION),
    spec(Kind::Flanger, "Flanger", "Modulation", MODULATION),
    spec(Kind::Phaser, "Phaser", "Modulation", MODULATION),
    spec(Kind::Tremolo, "Tremolo", "Modulation", &[("Rate Hz", 0.1, 30.0, 5.0), ("Depth", 0.0, 1.0, 0.6)]),
    spec(Kind::Vibrato, "Vibrato", "Modulation", &[("Rate Hz", 0.1, 15.0, 5.0), ("Depth", 0.0, 1.0, 0.4)]),
    spec(Kind::AutoPan, "Auto Pan", "Modulation", &[("Rate Hz", 0.05, 20.0, 1.0), ("Depth", 0.0, 1.0, 1.0)]),
    spec(Kind::RingMod, "Ring Modulator", "Modulation", &[("Frequency Hz", 1.0, 5000.0, 200.0), ("Mix", 0.0, 1.0, 0.5)]),
    spec(Kind::Overdrive, "Overdrive", "Distortion", &[("Drive dB", 0.0, 40.0, 12.0), ("Tone", 0.0, 1.0, 0.7), ("Mix", 0.0, 1.0, 1.0)]),
    spec(Kind::Fuzz, "Fuzz", "Distortion", &[("Drive dB", 0.0, 60.0, 30.0), ("Mix", 0.0, 1.0, 1.0)]),
    spec(Kind::Bitcrush, "Bitcrusher", "Distortion", &[("Bits", 1.0, 16.0, 8.0), ("Downsample", 1.0, 40.0, 4.0), ("Mix", 0.0, 1.0, 1.0)]),
    spec(Kind::Exciter, "Exciter", "Distortion", &[("Frequency Hz", 1000.0, 12000.0, 4000.0), ("Amount", 0.0, 1.0, 0.3)]),
    spec(Kind::PitchShift, "Pitch Shifter", "Pitch", &[("Semitones", -24.0, 24.0, 0.0), ("Mix", 0.0, 1.0, 1.0)]),
    spec(Kind::Width, "Stereo Width", "Stereo", &[("Width", 0.0, 2.0, 1.0)]),
    spec(Kind::Haas, "Haas Widener", "Stereo", &[("Delay ms", 0.0, 40.0, 15.0), ("Mix", 0.0, 1.0, 1.0)]),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub kind: Kind,
    pub enabled: bool,
    pub values: Vec<f32>,
}

impl Effect {
    pub fn new(kind: Kind) -> Self {
        let values = Self::spec_of(kind).params.iter().map(|param| param.3).collect();
        Self { kind, enabled: true, values }
    }

    pub fn spec_of(kind: Kind) -> &'static Spec {
        SPECS.iter().find(|spec| spec.kind == kind).unwrap_or(&SPECS[0])
    }

    fn at(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or_else(|| Self::spec_of(self.kind).params.get(index).map_or(0.0, |param| param.3))
    }
}

fn db(value: f32) -> f32 {
    10f32.powf(value / 20.0)
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

#[derive(Clone, Copy)]
enum Band {
    Low,
    High,
    Pass,
    Notch,
    LowShelf(f32),
    HighShelf(f32),
    Peak(f32),
}

impl Biquad {
    fn set(&mut self, band: Band, frequency: f32, q: f32, rate: f32) {
        let w = TAU * frequency.clamp(5.0, rate * 0.49) / rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q.max(0.05));
        let (b0, b1, b2, a0, a1, a2) = match band {
            Band::Low => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            Band::High => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            Band::Pass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            Band::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            Band::Peak(gain) => {
                let a = 10f32.powf(gain / 40.0);
                (1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a)
            }
            Band::LowShelf(gain) => {
                let a = 10f32.powf(gain / 40.0);
                let root = 2.0 * a.sqrt() * alpha;
                (a * ((a + 1.0) - (a - 1.0) * cos + root), 2.0 * a * ((a - 1.0) - (a + 1.0) * cos), a * ((a + 1.0) - (a - 1.0) * cos - root), (a + 1.0) + (a - 1.0) * cos + root, -2.0 * ((a - 1.0) + (a + 1.0) * cos), (a + 1.0) + (a - 1.0) * cos - root)
            }
            Band::HighShelf(gain) => {
                let a = 10f32.powf(gain / 40.0);
                let root = 2.0 * a.sqrt() * alpha;
                (a * ((a + 1.0) + (a - 1.0) * cos + root), -2.0 * a * ((a - 1.0) + (a + 1.0) * cos), a * ((a + 1.0) + (a - 1.0) * cos - root), (a + 1.0) - (a - 1.0) * cos + root, 2.0 * ((a - 1.0) - (a + 1.0) * cos), (a + 1.0) - (a - 1.0) * cos - root)
            }
        };
        self.b = [b0 / a0, b1 / a0, b2 / a0];
        self.a = [a1 / a0, a2 / a0];
    }

    fn run(&mut self, input: f32) -> f32 {
        let output = self.b[0] * input + self.z[0];
        self.z[0] = self.b[1] * input - self.a[0] * output + self.z[1];
        self.z[1] = self.b[2] * input - self.a[1] * output;
        if output.is_finite() {
            output
        } else {
            self.z = [0.0; 2];
            0.0
        }
    }
}

#[derive(Clone, Default)]
struct Line {
    data: Vec<f32>,
    write: usize,
}

impl Line {
    fn sized(samples: usize) -> Self {
        Self { data: vec![0.0; samples.max(2)], write: 0 }
    }

    fn read(&self, delay: f32) -> f32 {
        let length = self.data.len();
        let delay = delay.clamp(1.0, (length - 2) as f32);
        let position = self.write as f32 + length as f32 - delay;
        let (index, fraction) = (position as usize, position.fract());
        self.data[index % length] * (1.0 - fraction) + self.data[(index + 1) % length] * fraction
    }

    fn push(&mut self, value: f32) {
        self.data[self.write] = value;
        self.write = (self.write + 1) % self.data.len();
    }
}

#[derive(Clone)]
struct Comb {
    line: Vec<f32>,
    at: usize,
    low: f32,
}

impl Comb {
    fn new(length: usize) -> Self {
        Self { line: vec![0.0; length.max(1)], at: 0, low: 0.0 }
    }

    fn run(&mut self, input: f32, feedback: f32, damp: f32) -> f32 {
        let output = self.line[self.at];
        self.low += (output - self.low) * (1.0 - damp);
        self.line[self.at] = input + self.low * feedback;
        self.at = (self.at + 1) % self.line.len();
        output
    }
}

#[derive(Clone)]
struct Reverb {
    combs: [Vec<Comb>; 2],
    passes: [Vec<Comb>; 2],
}

impl Reverb {
    fn new(rate: f32) -> Self {
        const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        const PASSES: [usize; 4] = [556, 441, 341, 225];
        let scale = rate / 44_100.0;
        let build = |lengths: &[usize], extra: usize| lengths.iter().map(|length| Comb::new(((*length + extra) as f32 * scale) as usize)).collect::<Vec<_>>();
        Self { combs: [build(&COMBS, 0), build(&COMBS, 23)], passes: [build(&PASSES, 0), build(&PASSES, 23)] }
    }

    fn run(&mut self, input: f32, side: usize, feedback: f32, damp: f32) -> f32 {
        let mut sum = 0.0;
        for comb in &mut self.combs[side] {
            sum += comb.run(input * 0.03, feedback, damp);
        }
        for pass in &mut self.passes[side] {
            let delayed = pass.line[pass.at];
            pass.line[pass.at] = sum + delayed * 0.5;
            pass.at = (pass.at + 1) % pass.line.len();
            sum = delayed - sum;
        }
        sum
    }
}

#[derive(Clone, Default)]
struct State {
    filters: Vec<Biquad>,
    lines: Vec<Line>,
    reverb: Option<Box<Reverb>>,
    envelope: f32,
    slow: f32,
    phase: f32,
    hold: [f32; 2],
    count: u32,
    pitch: f32,
}

fn envelope_rate(milliseconds: f32, rate: f32) -> f32 {
    (-1000.0 / (milliseconds.max(0.01) * rate)).exp()
}

impl State {
    fn prepare(kind: Kind, rate: f32) -> Self {
        let mut state = Self::default();
        let seconds = |value: f32| (value * rate) as usize + 8;
        match kind {
            Kind::Delay | Kind::PingPong => state.lines = vec![Line::sized(seconds(2.05)); 2],
            Kind::Slapback => state.lines = vec![Line::sized(seconds(0.21)); 2],
            Kind::Chorus | Kind::Flanger | Kind::Vibrato => state.lines = vec![Line::sized(seconds(0.06)); 2],
            Kind::PitchShift => state.lines = vec![Line::sized(seconds(0.2)); 2],
            Kind::Haas => state.lines = vec![Line::sized(seconds(0.05))],
            Kind::Reverb | Kind::Hall | Kind::Room | Kind::Plate => state.reverb = Some(Box::new(Reverb::new(rate))),
            _ => {}
        }
        state
    }

    fn process(&mut self, effect: &Effect, left: &mut [f32], right: &mut [f32], rate: f32) {
        let value = |index| effect.at(index);
        match effect.kind {
            Kind::Gain => {
                let gain = db(value(0));
                for sample in left.iter_mut().chain(right.iter_mut()) {
                    *sample *= gain;
                }
            }
            Kind::Balance => {
                let (l, r) = ((1.0 - value(0)).min(1.0), (1.0 + value(0)).min(1.0));
                left.iter_mut().for_each(|sample| *sample *= l);
                right.iter_mut().for_each(|sample| *sample *= r);
            }
            Kind::Mono => {
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    let middle = (*l + *r) * 0.5;
                    *l = middle;
                    *r = middle;
                }
            }
            Kind::SwapChannels => left.swap_with_slice(right),
            Kind::InvertPhase => {
                if value(0) >= 0.5 {
                    left.iter_mut().for_each(|sample| *sample = -*sample);
                }
                if value(1) >= 0.5 {
                    right.iter_mut().for_each(|sample| *sample = -*sample);
                }
            }
            Kind::DcRemove => self.run_bands(&[(Band::High, 12.0, 0.7)], left, right, rate),
            Kind::LowPass => self.run_bands(&[(Band::Low, value(0), value(1))], left, right, rate),
            Kind::HighPass => self.run_bands(&[(Band::High, value(0), value(1))], left, right, rate),
            Kind::BandPass => self.run_bands(&[(Band::Pass, value(0), value(1))], left, right, rate),
            Kind::Notch => self.run_bands(&[(Band::Notch, value(0), value(1))], left, right, rate),
            Kind::LowShelf => self.run_bands(&[(Band::LowShelf(value(1)), value(0), 0.7)], left, right, rate),
            Kind::HighShelf => self.run_bands(&[(Band::HighShelf(value(1)), value(0), 0.7)], left, right, rate),
            Kind::PeakEq => self.run_bands(&[(Band::Peak(value(1)), value(0), value(2))], left, right, rate),
            Kind::ParametricEq => self.run_bands(&[(Band::LowShelf(value(0)), 100.0, 0.7), (Band::Peak(value(2)), value(1), 1.0), (Band::Peak(value(4)), value(3), 1.0), (Band::Peak(value(6)), value(5), 1.0), (Band::HighShelf(value(7)), 10_000.0, 0.7)], left, right, rate),
            Kind::GraphicEq => {
                let bands: Vec<(Band, f32, f32)> = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0].iter().enumerate().map(|(index, frequency)| (Band::Peak(value(index)), *frequency, 1.4)).collect();
                self.run_bands(&bands, left, right, rate);
            }
            Kind::Telephone => self.run_bands(&[(Band::High, value(0), 0.8), (Band::Low, value(1), 0.8)], left, right, rate),
            Kind::Compressor | Kind::Expander | Kind::Gate | Kind::Limiter => self.dynamics(effect, left, right, rate),
            Kind::DeEsser => {
                self.pair(Band::Pass, value(0), 1.5, rate);
                let (threshold, amount) = (db(value(1)), value(2));
                let release = envelope_rate(60.0, rate);
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    let (band_l, band_r) = (self.filters[0].run(*l), self.filters[1].run(*r));
                    self.envelope = band_l.abs().max(band_r.abs()).max(self.envelope * release);
                    let over = (self.envelope / threshold.max(0.000_1)).max(1.0);
                    let cut = (1.0 - 1.0 / over) * amount;
                    *l -= band_l * cut;
                    *r -= band_r * cut;
                }
            }
            Kind::Transient => {
                let (fast, slow) = (envelope_rate(2.0, rate), envelope_rate(80.0, rate));
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    self.envelope = l.abs().max(r.abs()).max(self.envelope * fast);
                    self.slow = self.envelope + (self.slow - self.envelope) * slow;
                    let difference = ((self.envelope - self.slow) / self.slow.max(0.000_1)).clamp(-1.0, 1.0);
                    let gain = (1.0 + difference * if difference > 0.0 { value(0) } else { -value(1) }).clamp(0.1, 4.0);
                    *l *= gain;
                    *r *= gain;
                }
            }
            Kind::Delay | Kind::PingPong | Kind::Slapback => {
                let (time, feedback, mix) = if effect.kind == Kind::Slapback { (value(0), 0.0, value(1)) } else { (value(0), value(1), value(2)) };
                let delay = time / 1000.0 * rate;
                let cross = effect.kind == Kind::PingPong;
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    let (wet_l, wet_r) = (self.lines[0].read(delay), self.lines[1].read(delay));
                    if cross {
                        self.lines[0].push((*l + *r) * 0.5 + wet_r * feedback);
                        self.lines[1].push(wet_l * feedback);
                    } else {
                        self.lines[0].push(*l + wet_l * feedback);
                        self.lines[1].push(*r + wet_r * feedback);
                    }
                    *l += wet_l * mix;
                    *r += wet_r * mix;
                }
            }
            Kind::Reverb | Kind::Hall | Kind::Room | Kind::Plate => {
                let Some(reverb) = self.reverb.as_mut() else { return };
                let (feedback, damp, width, mix) = (0.7 + value(0) * 0.28, value(1) * 0.4, value(2), value(3));
                let (same, cross) = (0.5 + width * 0.5, 0.5 - width * 0.5);
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    let input = (*l + *r) * 0.5;
                    let (wet_l, wet_r) = (reverb.run(input, 0, feedback, damp), reverb.run(input, 1, feedback, damp));
                    *l = *l * (1.0 - mix) + (wet_l * same + wet_r * cross) * mix;
                    *r = *r * (1.0 - mix) + (wet_r * same + wet_l * cross) * mix;
                }
            }
            Kind::Chorus | Kind::Flanger | Kind::Vibrato => {
                let vibrato = effect.kind == Kind::Vibrato;
                let (rate_hz, depth) = (value(0), value(1));
                let (feedback, mix) = if vibrato { (0.0, 1.0) } else { (value(2), value(3)) };
                let (base, swing) = match effect.kind {
                    Kind::Chorus => (0.02, 0.008),
                    Kind::Vibrato => (0.006, 0.005),
                    _ => (0.003, 0.0025),
                };
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    self.phase = (self.phase + rate_hz / rate).fract();
                    let wobble = (self.phase * TAU).sin();
                    let quarter = ((self.phase + 0.25) * TAU).sin();
                    let (wet_l, wet_r) = (self.lines[0].read((base + swing * depth * wobble) * rate), self.lines[1].read((base + swing * depth * quarter) * rate));
                    self.lines[0].push(*l + wet_l * feedback);
                    self.lines[1].push(*r + wet_r * feedback);
                    if vibrato {
                        *l = wet_l;
                        *r = wet_r;
                    } else {
                        *l = *l * (1.0 - mix * 0.5) + wet_l * mix;
                        *r = *r * (1.0 - mix * 0.5) + wet_r * mix;
                    }
                }
            }
            Kind::Phaser => {
                self.filters.resize(12, Biquad::default());
                let (rate_hz, depth, feedback, mix) = (value(0), value(1), value(2), value(3));
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    self.phase = (self.phase + rate_hz / rate).fract();
                    let sweep = 300.0 * (1.0 + 7.0 * depth * (0.5 + 0.5 * (self.phase * TAU).sin()));
                    let tangent = (PI * sweep / rate).tan();
                    let coefficient = (tangent - 1.0) / (tangent + 1.0);
                    for side in 0..2 {
                        let dry = if side == 0 { *l } else { *r };
                        let mut signal = dry + self.hold[side] * feedback;
                        for stage in 0..6 {
                            let filter = &mut self.filters[side * 6 + stage];
                            let output = coefficient * signal + filter.z[0];
                            filter.z[0] = signal - coefficient * output;
                            signal = output;
                        }
                        self.hold[side] = signal;
                        let mixed = dry * (1.0 - mix * 0.5) + signal * mix * 0.5;
                        if side == 0 {
                            *l = mixed;
                        } else {
                            *r = mixed;
                        }
                    }
                }
            }
            Kind::Tremolo | Kind::AutoPan | Kind::RingMod => {
                let frequency = value(0);
                let amount = value(1);
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    self.phase = (self.phase + frequency / rate).fract();
                    let wave = (self.phase * TAU).sin();
                    match effect.kind {
                        Kind::Tremolo => {
                            let gain = 1.0 - amount * (0.5 + 0.5 * wave);
                            *l *= gain;
                            *r *= gain;
                        }
                        Kind::AutoPan => {
                            *l *= 1.0 - amount * (0.5 + 0.5 * wave);
                            *r *= 1.0 - amount * (0.5 - 0.5 * wave);
                        }
                        _ => {
                            *l = *l * (1.0 - amount) + *l * wave * amount;
                            *r = *r * (1.0 - amount) + *r * wave * amount;
                        }
                    }
                }
            }
            Kind::Overdrive => {
                let (drive, mix) = (db(value(0)), value(2));
                self.pair(Band::Low, 800.0 + value(1) * 15_000.0, 0.7, rate);
                let makeup = (1.0 / drive.sqrt()).max(0.4);
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    let wet_l = self.filters[0].run((*l * drive).tanh()) * makeup;
                    let wet_r = self.filters[1].run((*r * drive).tanh()) * makeup;
                    *l = *l * (1.0 - mix) + wet_l * mix;
                    *r = *r * (1.0 - mix) + wet_r * mix;
                }
            }
            Kind::Fuzz => {
                let (drive, mix) = (db(value(0)), value(1));
                for sample in left.iter_mut().chain(right.iter_mut()) {
                    *sample = *sample * (1.0 - mix) + (*sample * drive).clamp(-0.5, 0.5) * mix;
                }
            }
            Kind::Bitcrush => {
                let levels = 2f32.powf(value(0).round());
                let hold = value(1).round().max(1.0) as u32;
                let mix = value(2);
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    if self.count % hold == 0 {
                        self.hold = [(*l * levels).round() / levels, (*r * levels).round() / levels];
                    }
                    self.count = self.count.wrapping_add(1);
                    *l = *l * (1.0 - mix) + self.hold[0] * mix;
                    *r = *r * (1.0 - mix) + self.hold[1] * mix;
                }
            }
            Kind::Exciter => {
                self.pair(Band::High, value(0), 0.7, rate);
                let amount = value(1);
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    *l += (self.filters[0].run(*l) * 4.0).tanh() * amount * 0.5;
                    *r += (self.filters[1].run(*r) * 4.0).tanh() * amount * 0.5;
                }
            }
            Kind::PitchShift => {
                let ratio = 2f32.powf(value(0) / 12.0);
                let mix = value(1);
                let window = 0.08 * rate;
                let step = (1.0 - ratio) / window;
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    self.pitch = (self.pitch + step).rem_euclid(1.0);
                    let second = (self.pitch + 0.5).fract();
                    let (fade_a, fade_b) = ((self.pitch * PI).sin().powi(2), (second * PI).sin().powi(2));
                    self.lines[0].push(*l);
                    self.lines[1].push(*r);
                    let wet_l = self.lines[0].read(self.pitch * window) * fade_a + self.lines[0].read(second * window) * fade_b;
                    let wet_r = self.lines[1].read(self.pitch * window) * fade_a + self.lines[1].read(second * window) * fade_b;
                    *l = *l * (1.0 - mix) + wet_l * mix;
                    *r = *r * (1.0 - mix) + wet_r * mix;
                }
            }
            Kind::Width => {
                let width = value(0);
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    let (middle, side) = ((*l + *r) * 0.5, (*l - *r) * 0.5 * width);
                    *l = middle + side;
                    *r = middle - side;
                }
            }
            Kind::Haas => {
                let (delay, mix) = (value(0) / 1000.0 * rate, value(1));
                for r in right.iter_mut() {
                    let wet = self.lines[0].read(delay);
                    self.lines[0].push(*r);
                    *r = *r * (1.0 - mix) + wet * mix;
                }
            }
        }
    }

    fn pair(&mut self, band: Band, frequency: f32, q: f32, rate: f32) {
        self.filters.resize(2, Biquad::default());
        for filter in &mut self.filters {
            filter.set(band, frequency, q, rate);
        }
    }

    fn run_bands(&mut self, bands: &[(Band, f32, f32)], left: &mut [f32], right: &mut [f32], rate: f32) {
        self.filters.resize(bands.len() * 2, Biquad::default());
        for (index, (band, frequency, q)) in bands.iter().enumerate() {
            self.filters[index * 2].set(*band, *frequency, *q, rate);
            self.filters[index * 2 + 1].set(*band, *frequency, *q, rate);
            for sample in left.iter_mut() {
                *sample = self.filters[index * 2].run(*sample);
            }
            for sample in right.iter_mut() {
                *sample = self.filters[index * 2 + 1].run(*sample);
            }
        }
    }

    fn dynamics(&mut self, effect: &Effect, left: &mut [f32], right: &mut [f32], rate: f32) {
        let value = |index| effect.at(index);
        let (threshold, ratio, attack, release, makeup, floor) = match effect.kind {
            Kind::Compressor => (value(0), value(1), value(2), value(3), value(4), -200.0),
            Kind::Limiter => (value(0), 100.0, 0.1, value(1), 0.0, -200.0),
            Kind::Gate => (value(0), 1.0, value(1), value(2), 0.0, value(3)),
            _ => (value(0), value(1), value(2), value(3), 0.0, -80.0),
        };
        let (attack, release) = (envelope_rate(attack, rate), envelope_rate(release, rate));
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let level = 20.0 * l.abs().max(r.abs()).max(0.000_01).log10();
            let coefficient = if level > self.envelope { attack } else { release };
            self.envelope = level + (self.envelope - level) * coefficient;
            let detected = self.envelope;
            let change = match effect.kind {
                Kind::Compressor | Kind::Limiter if detected > threshold => (threshold - detected) * (1.0 - 1.0 / ratio.max(1.0)),
                Kind::Gate if detected < threshold => floor,
                Kind::Expander if detected < threshold => ((threshold - detected) * (1.0 - ratio.max(1.0))).max(floor),
                _ => 0.0,
            };
            let gain = db(change + makeup);
            *l *= gain;
            *r *= gain;
            if effect.kind == Kind::Limiter {
                let ceiling = db(threshold);
                *l = l.clamp(-ceiling, ceiling);
                *r = r.clamp(-ceiling, ceiling);
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct Chain {
    states: Vec<(Kind, State)>,
}

impl Chain {
    pub fn process(&mut self, effects: &[Effect], left: &mut [f32], right: &mut [f32], rate: u32) {
        let rate = rate.max(1) as f32;
        let same = self.states.len() == effects.len() && self.states.iter().zip(effects).all(|(state, effect)| state.0 == effect.kind);
        if !same {
            self.states = effects.iter().map(|effect| (effect.kind, State::prepare(effect.kind, rate))).collect();
        }
        for (state, effect) in self.states.iter_mut().zip(effects) {
            if effect.enabled {
                state.1.process(effect, left, right, rate);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_processor_stays_finite_and_makes_sound() {
        for spec in SPECS {
            let effect = Effect::new(spec.kind);
            assert_eq!(effect.values.len(), spec.params.len());
            let mut chain = Chain::default();
            let mut time = 0.0f32;
            let mut heard = false;
            for _ in 0..40 {
                let mut left: Vec<f32> = (0..1024)
                    .map(|_| {
                        time += 1.0;
                        (time * 0.05).sin() * 0.5
                    })
                    .collect();
                let mut right = left.clone();
                chain.process(std::slice::from_ref(&effect), &mut left, &mut right, 48_000);
                assert!(left.iter().chain(&right).all(|sample| sample.is_finite()), "{}", spec.name);
                heard |= left.iter().chain(&right).any(|sample| sample.abs() > 0.000_1);
            }
            assert!(heard, "{} went silent", spec.name);
        }
    }

    #[test]
    fn low_pass_removes_high_frequencies() {
        let mut effect = Effect::new(Kind::LowPass);
        effect.values[0] = 200.0;
        let mut left: Vec<f32> = (0..4800).map(|index| (index as f32 * TAU * 8000.0 / 48000.0).sin()).collect();
        let mut right = left.clone();
        Chain::default().process(&[effect], &mut left, &mut right, 48_000);
        assert!(left[2000..].iter().fold(0.0f32, |peak, sample| peak.max(sample.abs())) < 0.02);
    }

    #[test]
    fn panner_moves_sound_to_the_correct_ear_and_fades_with_distance() {
        use crate::{Spatial, SpatialState};
        let source: Vec<f32> = (0..4800).map(|index| (index as f32 * 0.1).sin()).collect();
        let energy = |samples: &[f32]| samples[1000..].iter().map(|sample| sample * sample).sum::<f32>();
        let run = |x: f32, z: f32| {
            let (mut left, mut right) = (source.clone(), source.clone());
            let spatial = Spatial { enabled: true, x, z, ..Spatial::default() };
            spatial.process(&mut SpatialState::default(), &mut left, &mut right, 48_000);
            (energy(&left), energy(&right))
        };
        let (left, right) = run(3.0, 0.0);
        assert!(right > left * 1.3);
        let (near, _) = run(0.0, 1.0);
        let (far, _) = run(0.0, 10.0);
        assert!(near > far * 10.0);
    }
}
