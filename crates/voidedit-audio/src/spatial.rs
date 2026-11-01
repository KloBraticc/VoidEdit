use serde::{Deserialize, Serialize};

const HEAD_RADIUS: f32 = 0.0875;
const SPEED_OF_SOUND: f32 = 343.0;
const MAX_DELAY: usize = 256;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Spatial {
    pub enabled: bool,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub reference: f32,
    pub rolloff: f32,
    pub head_shadow: f32,
    pub spread: f32,
}

impl Default for Spatial {
    fn default() -> Self {
        Self { enabled: false, x: 0.0, y: 0.0, z: 1.0, reference: 1.0, rolloff: 1.0, head_shadow: 1.0, spread: 0.0 }
    }
}

#[derive(Clone)]
pub struct SpatialState {
    line: Vec<f32>,
    write: usize,
    near: f32,
    far: f32,
    tilt: f32,
}

impl Default for SpatialState {
    fn default() -> Self {
        Self { line: vec![0.0; MAX_DELAY], write: 0, near: 0.0, far: 0.0, tilt: 0.0 }
    }
}

impl Spatial {
    pub fn distance(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn gain(&self) -> f32 {
        let reference = self.reference.max(0.01);
        reference / (reference + self.rolloff.max(0.0) * (self.distance().max(reference) - reference))
    }

    pub fn process(&self, state: &mut SpatialState, left: &mut [f32], right: &mut [f32], rate: u32) {
        let rate = rate.max(1) as f32;
        let horizontal = (self.x * self.x + self.z * self.z).sqrt().max(0.001);
        let side = (self.x / horizontal).clamp(-1.0, 1.0);
        let azimuth = side.asin();
        let delay = ((HEAD_RADIUS / SPEED_OF_SOUND * (azimuth.abs() + azimuth.abs().sin()) * rate) as usize).min(MAX_DELAY - 1);
        let behind = if self.z < 0.0 { 0.75 } else { 1.0 };
        let gain = self.gain() * behind;
        let shadow = self.head_shadow.clamp(0.0, 1.0);
        let far_level = 1.0 - side.abs() * 0.45 * shadow;
        let cutoff = 20_000.0 - side.abs() * 16_000.0 * shadow;
        let rear_cutoff = if self.z < 0.0 { 9_000.0_f32 } else { 20_000.0 };
        let far_coefficient = 1.0 - (-std::f32::consts::TAU * cutoff.min(rear_cutoff) / rate).exp();
        let near_coefficient = 1.0 - (-std::f32::consts::TAU * rear_cutoff / rate).exp();
        let elevation = (self.y / self.distance().max(0.001)).clamp(-1.0, 1.0);
        let spread = self.spread.clamp(0.0, 1.0);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (dry_l, dry_r) = (*l, *r);
            let input = (dry_l + dry_r) * 0.5;
            state.tilt += (input - state.tilt) * 0.08;
            let shaped = input + (input - state.tilt) * elevation * 0.35;
            state.line[state.write] = shaped;
            let delayed = state.line[(state.write + MAX_DELAY - delay) % MAX_DELAY];
            state.write = (state.write + 1) % MAX_DELAY;
            state.near += (shaped - state.near) * near_coefficient;
            state.far += (delayed - state.far) * far_coefficient;
            let (near, far) = (state.near, state.far * far_level);
            let (out_l, out_r) = if side >= 0.0 { (far, near) } else { (near, far) };
            *l = (out_l * (1.0 - spread) + dry_l * spread) * gain;
            *r = (out_r * (1.0 - spread) + dry_r * spread) * gain;
        }
    }
}
