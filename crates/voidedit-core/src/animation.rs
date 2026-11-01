use std::f32::consts::{PI, TAU};

use serde::{Deserialize, Serialize};

use crate::{Clip, Param};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimKind {
    Fade,
    SlideLeft,
    SlideRight,
    SlideUp,
    SlideDown,
    Zoom,
    Spin,
    FlipX,
    FlipY,
    Depth,
    Shake,
    Swing,
    Glitch,
    Flicker,
}

impl AnimKind {
    pub const ALL: [(AnimKind, &'static str, f32); 14] = [
        (AnimKind::Fade, "Fade", 100.0),
        (AnimKind::SlideLeft, "Slide left", 400.0),
        (AnimKind::SlideRight, "Slide right", 400.0),
        (AnimKind::SlideUp, "Slide up", 300.0),
        (AnimKind::SlideDown, "Slide down", 300.0),
        (AnimKind::Zoom, "Zoom", 60.0),
        (AnimKind::Spin, "Spin", 180.0),
        (AnimKind::FlipX, "Flip vertical", 90.0),
        (AnimKind::FlipY, "Flip horizontal", 90.0),
        (AnimKind::Depth, "Depth push", 800.0),
        (AnimKind::Shake, "Shake", 30.0),
        (AnimKind::Swing, "Swing", 20.0),
        (AnimKind::Glitch, "Glitch", 60.0),
        (AnimKind::Flicker, "Flicker", 100.0),
    ];

    pub fn name(self) -> &'static str {
        Self::ALL.iter().find(|item| item.0 == self).map_or("", |item| item.1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trigger {
    In,
    Out,
    Loop,
    Whole,
}

impl Trigger {
    pub const ALL: [(Trigger, &'static str); 4] = [(Trigger::In, "Entrance"), (Trigger::Out, "Exit"), (Trigger::Loop, "Loop"), (Trigger::Whole, "Whole clip")];

    pub fn name(self) -> &'static str {
        Self::ALL.iter().find(|item| item.0 == self).map_or("", |item| item.1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Curve {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    Expo,
    Circ,
    Back,
    Elastic,
    Bounce,
    Steps,
}

impl Curve {
    pub const ALL: [(Curve, &'static str); 10] = [(Curve::Linear, "Linear"), (Curve::EaseIn, "Ease in"), (Curve::EaseOut, "Ease out"), (Curve::EaseInOut, "Ease in and out"), (Curve::Expo, "Expo"), (Curve::Circ, "Circular"), (Curve::Back, "Overshoot"), (Curve::Elastic, "Elastic"), (Curve::Bounce, "Bounce"), (Curve::Steps, "Steps")];

    pub fn name(self) -> &'static str {
        Self::ALL.iter().find(|item| item.0 == self).map_or("", |item| item.1)
    }

    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Curve::Linear => t,
            Curve::EaseIn => t * t * t,
            Curve::EaseOut => 1.0 - (1.0 - t).powi(3),
            Curve::EaseInOut => t * t * (3.0 - 2.0 * t),
            Curve::Expo => {
                if t >= 1.0 {
                    1.0
                } else {
                    1.0 - 2f32.powf(-10.0 * t)
                }
            }
            Curve::Circ => (1.0 - (1.0 - t) * (1.0 - t)).sqrt(),
            Curve::Back => {
                let u = t - 1.0;
                1.0 + 2.70158 * u * u * u + 1.70158 * u * u
            }
            Curve::Elastic => {
                if t <= 0.0 || t >= 1.0 {
                    t
                } else {
                    2f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * TAU / 3.0).sin() + 1.0
                }
            }
            Curve::Bounce => {
                let (n, d) = (7.5625, 2.75);
                if t < 1.0 / d {
                    n * t * t
                } else if t < 2.0 / d {
                    let u = t - 1.5 / d;
                    n * u * u + 0.75
                } else if t < 2.5 / d {
                    let u = t - 2.25 / d;
                    n * u * u + 0.9375
                } else {
                    let u = t - 2.625 / d;
                    n * u * u + 0.984375
                }
            }
            Curve::Steps => (t * 6.0).floor() / 6.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    pub kind: AnimKind,
    pub trigger: Trigger,
    pub curve: Curve,
    pub delay: u32,
    pub duration: u32,
    pub amount: f32,
    pub enabled: bool,
}

impl Animation {
    pub fn new(kind: AnimKind, trigger: Trigger, curve: Curve, delay: u32, duration: u32, amount: f32) -> Self {
        Self { kind, trigger, curve, delay, duration, amount, enabled: true }
    }

    pub fn default_amount(kind: AnimKind) -> f32 {
        AnimKind::ALL.iter().find(|item| item.0 == kind).map_or(100.0, |item| item.2)
    }

    fn weight(&self, local: f32, length: f32) -> f32 {
        let duration = self.duration.max(1) as f32;
        let delay = self.delay as f32;
        match self.trigger {
            Trigger::In => 1.0 - self.curve.apply((local - delay) / duration),
            Trigger::Out => self.curve.apply((local - (length - duration - delay)) / duration),
            Trigger::Whole => self.curve.apply((local - delay) / (length - delay).max(1.0)),
            Trigger::Loop => {
                if local < delay {
                    0.0
                } else {
                    ((local - delay) / duration * TAU).sin()
                }
            }
        }
    }
}

fn noise(seed: f32) -> f32 {
    ((seed * 12.9898).sin() * 43758.547).fract() * 2.0 - 1.0
}

pub fn presets() -> Vec<(&'static str, Vec<Animation>)> {
    use AnimKind::*;
    use Curve as C;
    use Trigger::*;
    let a = Animation::new;
    vec![
        ("Fade in", vec![a(Fade, In, C::EaseOut, 0, 15, 100.0)]),
        ("Fade out", vec![a(Fade, Out, C::EaseIn, 0, 15, 100.0)]),
        ("Pop in", vec![a(Zoom, In, C::Back, 0, 14, -90.0), a(Fade, In, C::Linear, 0, 6, 100.0)]),
        ("Slam", vec![a(Zoom, In, C::Expo, 0, 10, 300.0), a(Fade, In, C::Linear, 0, 4, 100.0), a(Shake, In, C::EaseOut, 8, 12, 40.0)]),
        ("Bounce drop", vec![a(SlideUp, In, C::Bounce, 0, 26, 700.0), a(Fade, In, C::Linear, 0, 5, 100.0)]),
        ("Slide and settle", vec![a(SlideRight, In, C::Back, 0, 20, 500.0), a(Fade, In, C::Linear, 0, 8, 100.0), a(Swing, In, C::EaseOut, 0, 24, 12.0)]),
        ("Spin in", vec![a(Spin, In, C::Expo, 0, 22, 360.0), a(Zoom, In, C::Expo, 0, 22, -100.0)]),
        ("3D flip in", vec![a(FlipY, In, C::Back, 0, 24, 90.0), a(Fade, In, C::Linear, 0, 8, 100.0)]),
        ("Cinematic title", vec![a(Depth, In, C::Expo, 0, 45, 1200.0), a(Fade, In, C::EaseOut, 0, 30, 100.0), a(Zoom, Whole, C::Linear, 0, 1, 12.0), a(Fade, Out, C::EaseIn, 0, 20, 100.0)]),
        ("Glitch hit", vec![a(Glitch, In, C::Linear, 0, 14, 80.0), a(Flicker, In, C::Linear, 0, 14, 100.0), a(Zoom, In, C::Expo, 0, 8, 25.0)]),
        ("Pulse loop", vec![a(Zoom, Loop, C::Linear, 0, 30, 6.0)]),
        ("Float loop", vec![a(SlideUp, Loop, C::Linear, 0, 60, 14.0), a(Swing, Loop, C::Linear, 0, 90, 2.0)]),
        ("Earthquake", vec![a(Shake, Whole, C::Linear, 0, 1, 25.0)]),
        ("Zoom out exit", vec![a(Zoom, Out, C::EaseIn, 0, 16, -100.0), a(Fade, Out, C::EaseIn, 0, 16, 100.0)]),
    ]
}

impl Clip {
    pub fn animated(&self, time: f32) -> Clip {
        let mut clip = self.clone();
        let (mut dx, mut dy, mut scale, mut rotation, mut tilt_x, mut tilt_y, mut depth, mut opacity) = (0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0f32);
        let local = time - self.start as f32;
        let length = (self.end - self.start).max(1) as f32;
        for animation in self.animations.iter().filter(|animation| animation.enabled) {
            let weight = animation.weight(local, length);
            let amount = animation.amount;
            match animation.kind {
                AnimKind::Fade => opacity *= (1.0 - weight.abs() * amount / 100.0).clamp(0.0, 1.0),
                AnimKind::SlideLeft => dx -= amount * weight,
                AnimKind::SlideRight => dx += amount * weight,
                AnimKind::SlideUp => dy -= amount * weight,
                AnimKind::SlideDown => dy += amount * weight,
                AnimKind::Zoom => scale *= (1.0 + amount / 100.0 * weight).max(0.0),
                AnimKind::Spin => rotation += amount * weight,
                AnimKind::FlipX => tilt_x += amount * weight,
                AnimKind::FlipY => tilt_y += amount * weight,
                AnimKind::Depth => depth -= amount * weight,
                AnimKind::Shake => {
                    let strength = amount * weight.abs();
                    dx += noise(time.floor() + 1.3) * strength;
                    dy += noise(time.floor() + 7.9) * strength;
                }
                AnimKind::Swing => rotation += amount * weight * (local * PI / 7.0).cos(),
                AnimKind::Glitch => {
                    let step = (time / 2.0).floor();
                    dx += noise(step + 3.1) * amount * weight.abs();
                    dy += noise(step + 11.7) * amount * 0.2 * weight.abs();
                }
                AnimKind::Flicker => {
                    if weight.abs() > 0.0 && noise(time.floor() + 5.5) < 0.0 {
                        opacity *= 1.0 - (amount / 100.0).clamp(0.0, 1.0);
                    }
                }
            }
        }
        clip.x = Param::new(self.x.at(time) + dx);
        clip.y = Param::new(self.y.at(time) + dy);
        clip.scale = Param::new(self.scale.at(time) * scale);
        clip.rotation = Param::new(self.rotation.at(time) + rotation);
        clip.rotation_x = Param::new(self.rotation_x.at(time) + tilt_x);
        clip.rotation_y = Param::new(self.rotation_y.at(time) + tilt_y);
        clip.depth = Param::new(self.depth.at(time) + depth);
        clip.opacity = Param::new(self.opacity.at(time) * opacity);
        clip
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Project, Shape};

    #[test]
    fn stacked_animations_combine_and_finish_at_rest() {
        let project = Project::default();
        let mut clip = project.new_clip(Shape::Solid, 10);
        let (x, scale) = (clip.x.at(10.0), clip.scale.at(10.0));
        clip.animations = vec![Animation::new(AnimKind::SlideRight, Trigger::In, Curve::Linear, 0, 20, 100.0), Animation::new(AnimKind::Zoom, Trigger::In, Curve::Linear, 0, 20, 100.0)];
        let first = clip.animated(10.0);
        assert!((first.x.at(10.0) - x - 100.0).abs() < 0.01);
        assert!((first.scale.at(10.0) - scale * 2.0).abs() < 0.01);
        let rest = clip.animated(40.0);
        assert!((rest.x.at(40.0) - x).abs() < 0.01);
        assert!((rest.scale.at(40.0) - scale).abs() < 0.01);
    }

    #[test]
    fn every_preset_and_curve_stays_finite() {
        let project = Project::default();
        for (_, stack) in presets() {
            let mut clip = project.new_clip(Shape::Solid, 0);
            clip.animations = stack;
            for frame in 0..200 {
                let moved = clip.animated(frame as f32);
                assert!(moved.x.at(0.0).is_finite() && moved.scale.at(0.0).is_finite() && moved.opacity.at(0.0).is_finite());
            }
        }
        for (curve, _) in Curve::ALL {
            assert!((curve.apply(1.0) - 1.0).abs() < 0.02);
        }
    }
}
