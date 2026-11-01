pub mod expression;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Ease {
    Linear,
    Smooth,
    Hold,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub frame: u32,
    pub value: f32,
    pub ease: Ease,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub base: f32,
    pub keys: Vec<Key>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
}

impl Param {
    pub fn new(base: f32) -> Self {
        Self { base, keys: Vec::new(), expression: None }
    }

    pub fn at(&self, frame: f32) -> f32 {
        let value = self.keyed(frame);
        match &self.expression {
            Some(text) => expression::evaluate(text, value, frame).unwrap_or(value),
            None => value,
        }
    }

    fn keyed(&self, frame: f32) -> f32 {
        let (Some(first), Some(last)) = (self.keys.first(), self.keys.last()) else {
            return self.base;
        };
        if frame <= first.frame as f32 {
            return first.value;
        }
        if frame >= last.frame as f32 {
            return last.value;
        }
        let next = self.keys.partition_point(|key| key.frame as f32 <= frame);
        let (a, b) = (self.keys[next - 1], self.keys[next]);
        let t = (frame - a.frame as f32) / (b.frame - a.frame) as f32;
        let t = match a.ease {
            Ease::Linear => t,
            Ease::Smooth => t * t * (3.0 - 2.0 * t),
            Ease::Hold => 0.0,
        };
        a.value + (b.value - a.value) * t
    }

    pub fn has_key(&self, frame: u32) -> bool {
        self.keys.binary_search_by_key(&frame, |key| key.frame).is_ok()
    }

    pub fn set(&mut self, frame: u32, value: f32) {
        if self.keys.is_empty() {
            self.base = value;
            return;
        }
        self.set_key(frame, value);
    }

    pub fn set_key(&mut self, frame: u32, value: f32) {
        match self.keys.binary_search_by_key(&frame, |key| key.frame) {
            Ok(index) => self.keys[index].value = value,
            Err(index) => self.keys.insert(index, Key { frame, value, ease: Ease::Smooth }),
        }
    }

    pub fn toggle_key(&mut self, frame: u32) {
        match self.keys.binary_search_by_key(&frame, |key| key.frame) {
            Ok(index) => {
                self.base = self.keys[index].value;
                self.keys.remove(index);
            }
            Err(_) => self.set_key(frame, self.at(frame as f32)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    Solid,
    Rectangle,
    Ellipse,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    Brightness { amount: Param },
    Tint { amount: Param, color: [u8; 3] },
    Invert,
    Grayscale,
    Sepia,
    Contrast { amount: Param },
    Saturation { amount: Param },
    Exposure { amount: Param },
    Temperature { amount: Param },
    Posterize { levels: Param },
    Vignette { amount: Param },
    Pixelate { size: Param },
    Blur { radius: Param },
    FlipHorizontal,
    FlipVertical,
    Solarize { level: Param },
    Vibrance { amount: Param },
    Duotone { dark: [u8; 3], light: [u8; 3] },
    Library { id: String, name: String, values: std::collections::BTreeMap<String, LibValue> },
    Mirror,
    MotionBlur { length: Param },
    ZoomBlur { amount: Param },
    Ripple { amount: Param, frequency: Param },
    Tile { count: Param },
    Letterbox { amount: Param },
    GradientOverlay { top: [u8; 3], bottom: [u8; 3], amount: Param },
    Border { size: Param, color: [u8; 3] },
    Sketch,
    Emboss,
    ChromaticAberration { amount: Param },
    Scanlines { strength: Param, spacing: Param },
    Swirl { angle: Param },
    Kaleidoscope { segments: Param },
    Fisheye { amount: Param },
    ChannelSwap,
    HueShift { degrees: Param },
    Gamma { amount: Param },
    Levels { black: Param, white: Param },
    ColorBalance { red: Param, green: Param, blue: Param },
    Threshold { level: Param },
    Noise { amount: Param },
    ChromaKey { color: [u8; 3], tolerance: Param, softness: Param },
    ColorWheels { lift: [f32; 3], gamma: [f32; 3], gain: [f32; 3] },
    Curves { shadows: Param, midtones: Param, highlights: Param },
    Lut { path: String },
    Sharpen { amount: Param },
    Glow { amount: Param },
    EdgeDetect,
    DropShadow { x: Param, y: Param, opacity: Param },
    Wave { amplitude: Param, frequency: Param },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LibValue {
    Number(Param),
    Point(Param, Param),
    Color([f32; 4]),
    Check(bool),
    Choice(u32),
}

pub struct RgbaImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Blend {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Add,
    Darken,
    Lighten,
    Difference,
    SoftLight,
}

pub const LOOKS: [&str; 13] = ["Cinematic", "Vintage", "Black and White", "Warm", "Cool", "Vivid", "Faded", "Noir", "Night Vision", "Bleach Bypass", "Cross Process", "Retro TV", "Sunset"];

pub fn look(name: &str) -> Vec<Effect> {
    let p = Param::new;
    match name {
        "Cinematic" => vec![Effect::Contrast { amount: p(15.0) }, Effect::Saturation { amount: p(-10.0) }, Effect::Temperature { amount: p(-8.0) }, Effect::Vignette { amount: p(35.0) }],
        "Vintage" => vec![Effect::Sepia, Effect::Contrast { amount: p(-10.0) }, Effect::Noise { amount: p(6.0) }, Effect::Vignette { amount: p(40.0) }],
        "Black and White" => vec![Effect::Grayscale, Effect::Contrast { amount: p(15.0) }],
        "Warm" => vec![Effect::Temperature { amount: p(25.0) }, Effect::Saturation { amount: p(10.0) }],
        "Cool" => vec![Effect::Temperature { amount: p(-25.0) }, Effect::Saturation { amount: p(5.0) }],
        "Vivid" => vec![Effect::Vibrance { amount: p(50.0) }, Effect::Contrast { amount: p(10.0) }],
        "Faded" => vec![Effect::Levels { black: p(-30.0), white: p(255.0) }, Effect::Saturation { amount: p(-25.0) }],
        "Night Vision" => vec![Effect::Grayscale, Effect::Tint { amount: p(70.0), color: [40, 255, 90] }, Effect::Noise { amount: p(12.0) }, Effect::Vignette { amount: p(70.0) }],
        "Bleach Bypass" => vec![Effect::Saturation { amount: p(-55.0) }, Effect::Contrast { amount: p(35.0) }],
        "Cross Process" => vec![Effect::ColorBalance { red: p(10.0), green: p(5.0), blue: p(-15.0) }, Effect::Contrast { amount: p(20.0) }, Effect::Saturation { amount: p(15.0) }],
        "Retro TV" => vec![Effect::Scanlines { strength: p(30.0), spacing: p(3.0) }, Effect::ChromaticAberration { amount: p(3.0) }, Effect::Noise { amount: p(8.0) }, Effect::Vignette { amount: p(45.0) }],
        "Sunset" => vec![Effect::GradientOverlay { top: [255, 110, 60], bottom: [120, 40, 150], amount: p(25.0) }, Effect::Saturation { amount: p(15.0) }],
        "Noir" => vec![Effect::Grayscale, Effect::Contrast { amount: p(45.0) }, Effect::Vignette { amount: p(60.0) }, Effect::Noise { amount: p(5.0) }],
        _ => Vec::new(),
    }
}

pub const BLENDS: [Blend; 9] = [Blend::Normal, Blend::Multiply, Blend::Screen, Blend::Overlay, Blend::Add, Blend::Darken, Blend::Lighten, Blend::Difference, Blend::SoftLight];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Transition {
    #[default]
    Fade,
    WipeRight,
    WipeDown,
    SlideLeft,
    SlideUp,
    Zoom,
}

pub const TRANSITIONS: [Transition; 6] = [Transition::Fade, Transition::WipeRight, Transition::WipeDown, Transition::SlideLeft, Transition::SlideUp, Transition::Zoom];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MaskShape {
    #[default]
    None,
    Rectangle,
    Ellipse,
    Polygon,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mask {
    pub shape: MaskShape,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub feather: f32,
    pub invert: bool,
    pub points: Vec<[f32; 2]>,
    pub smooth: bool,
}

impl Default for Mask {
    fn default() -> Self {
        Self { shape: MaskShape::None, x: 0.5, y: 0.5, width: 0.6, height: 0.6, feather: 0.05, invert: false, points: Vec::new(), smooth: true }
    }
}

pub const EFFECT_NAMES: [&str; 49] = [
    "Motion Blur", "Zoom Blur", "Ripple", "Tile",
    "Letterbox", "Gradient Overlay", "Border", "Sketch",
    "Emboss", "Chromatic Aberration", "Scanlines", "Swirl", "Kaleidoscope", "Fisheye", "Channel Swap",
    "Solarize", "Vibrance", "Duotone", "Mirror",
    "Color Wheels", "Curves", "LUT", "Sharpen", "Glow", "Edge Detect", "Drop Shadow", "Wave",
    "Hue Shift", "Gamma", "Levels", "Color Balance", "Threshold", "Noise", "Chroma Key",
    "Brightness", "Tint", "Invert", "Grayscale", "Sepia", "Contrast", "Saturation", "Exposure", "Temperature", "Posterize", "Vignette", "Pixelate", "Blur", "Flip Horizontal", "Flip Vertical",
];

impl Effect {
    pub fn create(name: &str) -> Option<Effect> {
        Some(match name {
            "Brightness" => Effect::Brightness { amount: Param::new(0.0) },
            "Tint" => Effect::Tint { amount: Param::new(50.0), color: [255, 180, 90] },
            "Invert" => Effect::Invert,
            "Grayscale" => Effect::Grayscale,
            "Sepia" => Effect::Sepia,
            "Contrast" => Effect::Contrast { amount: Param::new(0.0) },
            "Saturation" => Effect::Saturation { amount: Param::new(0.0) },
            "Exposure" => Effect::Exposure { amount: Param::new(0.0) },
            "Temperature" => Effect::Temperature { amount: Param::new(0.0) },
            "Posterize" => Effect::Posterize { levels: Param::new(6.0) },
            "Vignette" => Effect::Vignette { amount: Param::new(50.0) },
            "Pixelate" => Effect::Pixelate { size: Param::new(16.0) },
            "Blur" => Effect::Blur { radius: Param::new(8.0) },
            "Flip Horizontal" => Effect::FlipHorizontal,
            "Flip Vertical" => Effect::FlipVertical,
            "Color Wheels" => Effect::ColorWheels { lift: [0.0; 3], gamma: [1.0; 3], gain: [1.0; 3] },
            "Curves" => Effect::Curves { shadows: Param::new(0.0), midtones: Param::new(0.0), highlights: Param::new(0.0) },
            "LUT" => Effect::Lut { path: String::new() },
            "Sharpen" => Effect::Sharpen { amount: Param::new(50.0) },
            "Glow" => Effect::Glow { amount: Param::new(40.0) },
            "Edge Detect" => Effect::EdgeDetect,
            "Drop Shadow" => Effect::DropShadow { x: Param::new(12.0), y: Param::new(12.0), opacity: Param::new(60.0) },
            "Wave" => Effect::Wave { amplitude: Param::new(10.0), frequency: Param::new(8.0) },
            "Solarize" => Effect::Solarize { level: Param::new(128.0) },
            "Vibrance" => Effect::Vibrance { amount: Param::new(30.0) },
            "Duotone" => Effect::Duotone { dark: [30, 10, 80], light: [255, 220, 120] },
            "Mirror" => Effect::Mirror,
            "Emboss" => Effect::Emboss,
            "Motion Blur" => Effect::MotionBlur { length: Param::new(16.0) },
            "Zoom Blur" => Effect::ZoomBlur { amount: Param::new(15.0) },
            "Ripple" => Effect::Ripple { amount: Param::new(3.0), frequency: Param::new(30.0) },
            "Tile" => Effect::Tile { count: Param::new(3.0) },
            "Letterbox" => Effect::Letterbox { amount: Param::new(12.0) },
            "Gradient Overlay" => Effect::GradientOverlay { top: [255, 80, 120], bottom: [40, 80, 255], amount: Param::new(40.0) },
            "Border" => Effect::Border { size: Param::new(20.0), color: [255, 255, 255] },
            "Sketch" => Effect::Sketch,
            "Chromatic Aberration" => Effect::ChromaticAberration { amount: Param::new(6.0) },
            "Scanlines" => Effect::Scanlines { strength: Param::new(40.0), spacing: Param::new(4.0) },
            "Swirl" => Effect::Swirl { angle: Param::new(90.0) },
            "Kaleidoscope" => Effect::Kaleidoscope { segments: Param::new(6.0) },
            "Fisheye" => Effect::Fisheye { amount: Param::new(50.0) },
            "Channel Swap" => Effect::ChannelSwap,
            "Hue Shift" => Effect::HueShift { degrees: Param::new(0.0) },
            "Gamma" => Effect::Gamma { amount: Param::new(1.0) },
            "Levels" => Effect::Levels { black: Param::new(0.0), white: Param::new(255.0) },
            "Color Balance" => Effect::ColorBalance { red: Param::new(0.0), green: Param::new(0.0), blue: Param::new(0.0) },
            "Threshold" => Effect::Threshold { level: Param::new(128.0) },
            "Noise" => Effect::Noise { amount: Param::new(10.0) },
            "Chroma Key" => Effect::ChromaKey { color: [0, 255, 0], tolerance: Param::new(30.0), softness: Param::new(10.0) },
            _ => return None,
        })
    }

    pub fn name(&self) -> &str {
        match self {
            Effect::Library { name, .. } => name,
            Effect::Brightness { .. } => "Brightness",
            Effect::Tint { .. } => "Tint",
            Effect::Invert => "Invert",
            Effect::Grayscale => "Grayscale",
            Effect::Sepia => "Sepia",
            Effect::Contrast { .. } => "Contrast",
            Effect::Saturation { .. } => "Saturation",
            Effect::Exposure { .. } => "Exposure",
            Effect::Temperature { .. } => "Temperature",
            Effect::Posterize { .. } => "Posterize",
            Effect::Vignette { .. } => "Vignette",
            Effect::Pixelate { .. } => "Pixelate",
            Effect::Blur { .. } => "Blur",
            Effect::FlipHorizontal => "Flip Horizontal",
            Effect::FlipVertical => "Flip Vertical",
            Effect::ColorWheels { .. } => "Color Wheels",
            Effect::Curves { .. } => "Curves",
            Effect::Lut { .. } => "LUT",
            Effect::Sharpen { .. } => "Sharpen",
            Effect::Glow { .. } => "Glow",
            Effect::EdgeDetect => "Edge Detect",
            Effect::DropShadow { .. } => "Drop Shadow",
            Effect::Wave { .. } => "Wave",
            Effect::Solarize { .. } => "Solarize",
            Effect::Vibrance { .. } => "Vibrance",
            Effect::Duotone { .. } => "Duotone",
            Effect::Mirror => "Mirror",
            Effect::Emboss => "Emboss",
            Effect::MotionBlur { .. } => "Motion Blur",
            Effect::ZoomBlur { .. } => "Zoom Blur",
            Effect::Ripple { .. } => "Ripple",
            Effect::Tile { .. } => "Tile",
            Effect::Letterbox { .. } => "Letterbox",
            Effect::GradientOverlay { .. } => "Gradient Overlay",
            Effect::Border { .. } => "Border",
            Effect::Sketch => "Sketch",
            Effect::ChromaticAberration { .. } => "Chromatic Aberration",
            Effect::Scanlines { .. } => "Scanlines",
            Effect::Swirl { .. } => "Swirl",
            Effect::Kaleidoscope { .. } => "Kaleidoscope",
            Effect::Fisheye { .. } => "Fisheye",
            Effect::ChannelSwap => "Channel Swap",
            Effect::HueShift { .. } => "Hue Shift",
            Effect::Gamma { .. } => "Gamma",
            Effect::Levels { .. } => "Levels",
            Effect::ColorBalance { .. } => "Color Balance",
            Effect::Threshold { .. } => "Threshold",
            Effect::Noise { .. } => "Noise",
            Effect::ChromaKey { .. } => "Chroma Key",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub name: String,
    pub track: usize,
    pub media: Option<String>,
    #[serde(default)]
    pub offset: u32,
    pub shape: Shape,
    pub color: [u8; 3],
    pub start: u32,
    pub end: u32,
    pub width: f32,
    pub height: f32,
    pub x: Param,
    pub y: Param,
    pub scale: Param,
    pub rotation: Param,
    pub opacity: Param,
    pub effects: Vec<Effect>,
    pub visible: bool,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default = "default_text_size")]
    pub text_size: f32,
    #[serde(default)]
    pub fade_in: u32,
    #[serde(default)]
    pub fade_out: u32,
    #[serde(default = "default_volume")]
    pub volume: Param,
    #[serde(default = "default_speed")]
    pub speed: f32,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub blend: Blend,
    #[serde(default)]
    pub transition_in: Transition,
    #[serde(default)]
    pub transition_out: Transition,
    #[serde(default)]
    pub mask: Mask,
    #[serde(default)]
    pub text_stroke: f32,
    #[serde(default)]
    pub text_background: bool,
    #[serde(default)]
    pub typewriter: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested: Option<Box<Project>>,
    #[serde(default = "default_zero")]
    pub rotation_x: Param,
    #[serde(default = "default_zero")]
    pub rotation_y: Param,
    #[serde(default = "default_zero")]
    pub depth: Param,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub pan: f32,
    #[serde(default)]
    pub bass: f32,
    #[serde(default)]
    pub treble: f32,
}

fn default_zero() -> Param {
    Param::new(0.0)
}

fn default_text_size() -> f32 {
    96.0
}

fn default_volume() -> Param {
    Param::new(100.0)
}

fn default_speed() -> f32 {
    1.0
}

impl Clip {
    pub fn fade(&self, frame: f32) -> f32 {
        let (entering, leaving) = self.progress(frame);
        let mut value = 1.0f32;
        if self.transition_in == Transition::Fade {
            value = value.min(entering);
        }
        if self.transition_out == Transition::Fade {
            value = value.min(leaving);
        }
        value
    }

    pub fn progress(&self, frame: f32) -> (f32, f32) {
        let entering = if self.fade_in > 0 { (frame - self.start as f32) / self.fade_in as f32 } else { 1.0 };
        let leaving = if self.fade_out > 0 { (self.end as f32 - frame) / self.fade_out as f32 } else { 1.0 };
        (entering.clamp(0.0, 1.0), leaving.clamp(0.0, 1.0))
    }

    pub fn media_seconds(&self, frame: u32, fps: u32) -> f64 {
        let length = self.end.saturating_sub(self.start) as f64;
        let elapsed = frame.saturating_sub(self.start) as f64;
        let position = if self.reverse { (length - 1.0 - elapsed).max(0.0) } else { elapsed };
        (position * self.speed.max(0.01) as f64 + self.offset as f64) / fps.max(1) as f64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum TrackKind {
    Video,
    Audio,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub name: String,
    pub kind: TrackKind,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "default_track_volume")]
    pub volume: f32,
    #[serde(default)]
    pub pan: f32,
}

fn default_track_volume() -> f32 {
    100.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration: u32,
    pub background: [u8; 3],
    pub tracks: Vec<Track>,
    pub clips: Vec<Clip>,
    pub media: Vec<String>,
    #[serde(default)]
    pub markers: Vec<u32>,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 30,
            duration: 1800,
            background: [0, 0, 0],
            tracks: vec![
                Track::new("Audio 1", TrackKind::Audio),
                Track::new("Video 1", TrackKind::Video),
                Track::new("Video 2", TrackKind::Video),
                Track::new("Video 3", TrackKind::Video),
            ],
            clips: Vec::new(),
            media: Vec::new(),
            markers: Vec::new(),
        }
    }
}

impl Track {
    pub fn new(name: &str, kind: TrackKind) -> Self {
        Self { name: name.into(), kind, muted: false, locked: false, volume: 100.0, pan: 0.0 }
    }
}

impl Project {
    pub fn new_text(&self, start: u32) -> Clip {
        let mut clip = self.new_clip(Shape::Rectangle, start);
        clip.name = "Text".into();
        clip.text = Some("Your text".into());
        clip.color = [255, 255, 255];
        clip
    }

    pub fn nest(&mut self, index: usize) {
        if index >= self.clips.len() {
            return;
        }
        let mut inner_clip = self.clips.remove(index);
        let (start, end) = (inner_clip.start, inner_clip.end);
        let track = inner_clip.track;
        inner_clip.start = 0;
        inner_clip.end = end - start;
        let mut inner = Project { width: self.width, height: self.height, fps: self.fps, duration: end - start + 1, background: [0, 0, 0], media: Vec::new(), markers: Vec::new(), ..Project::default() };
        inner_clip.track = inner.track_of(self.tracks[track].kind);
        inner.clips.push(inner_clip);
        let mut outer = self.new_clip(Shape::Solid, start);
        outer.name = "Nested".into();
        outer.end = end;
        outer.track = track;
        outer.nested = Some(Box::new(inner));
        self.clips.push(outer);
    }

    pub fn wanted_media(&self, frame: u32, depth: u32, found: &mut Vec<(String, f64)>) {
        if depth > MAX_NESTING {
            return;
        }
        for clip in &self.clips {
            if !clip.visible || frame < clip.start || frame >= clip.end || self.tracks.get(clip.track).is_none_or(|track| track.kind != TrackKind::Video) {
                continue;
            }
            if let Some(inner) = &clip.nested {
                inner.wanted_media(frame - clip.start + clip.offset, depth + 1, found);
            } else if let Some(path) = &clip.media {
                found.push((path.clone(), clip.media_seconds(frame, self.fps)));
            }
        }
    }

    pub fn set_aspect(&mut self, width: u32, height: u32) {
        let (old_w, old_h) = (self.width as f32, self.height as f32);
        self.width = width;
        self.height = height;
        let fit = (width as f32 / old_w).min(height as f32 / old_h);
        for clip in &mut self.clips {
            let (x, y) = (clip.x.base, clip.y.base);
            clip.x.base = (x - old_w / 2.0) * fit + width as f32 / 2.0;
            clip.y.base = (y - old_h / 2.0) * fit + height as f32 / 2.0;
            clip.scale.base *= fit;
        }
    }

    pub fn add_track(&mut self, kind: TrackKind) {
        let count = self.tracks.iter().filter(|track| track.kind == kind).count() + 1;
        let label = if kind == TrackKind::Video { "Video" } else { "Audio" };
        let track = Track::new(&format!("{label} {count}"), kind);
        match kind {
            TrackKind::Video => self.tracks.push(track),
            TrackKind::Audio => {
                self.tracks.insert(0, track);
                for clip in &mut self.clips {
                    clip.track += 1;
                }
            }
        }
    }

    pub fn ripple_delete(&mut self, index: usize) {
        if index >= self.clips.len() {
            return;
        }
        let removed = self.clips.remove(index);
        let gap = removed.end - removed.start;
        for clip in &mut self.clips {
            if clip.track == removed.track && clip.start >= removed.end {
                clip.start -= gap;
                clip.end -= gap;
            }
        }
    }

    pub fn snap(&self, frame: u32, skip: Option<usize>, playhead: u32, reach: u32) -> u32 {
        let mut best = frame;
        let mut distance = reach + 1;
        let mut consider = |point: u32| {
            let gap = point.abs_diff(frame);
            if gap < distance {
                distance = gap;
                best = point;
            }
        };
        consider(playhead);
        for (index, clip) in self.clips.iter().enumerate() {
            if Some(index) != skip {
                consider(clip.start);
                consider(clip.end);
            }
        }
        for marker in &self.markers {
            consider(*marker);
        }
        best
    }

    pub fn add_caption(&mut self, from: f64, to: f64, text: &str) {
        let fps = self.fps as f64;
        let mut clip = self.new_text((from * fps) as u32);
        clip.end = ((to * fps) as u32).max(clip.start + 1);
        clip.name = "Caption".into();
        clip.text = Some(text.to_string());
        clip.text_size = 54.0;
        clip.text_stroke = 3.0;
        clip.y = Param::new(self.height as f32 * 0.88);
        clip.track = self.tracks.len() - 1;
        self.duration = self.duration.max(clip.end + self.fps);
        self.clips.push(clip);
    }

    pub fn add_spoken_caption(&mut self, from: f64, to: f64, text: &str) {
        const WORDS_PER_CAPTION: usize = 7;
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            return;
        }
        let pieces: Vec<String> = words.chunks(WORDS_PER_CAPTION).map(|chunk| chunk.join(" ")).collect();
        let total: usize = pieces.iter().map(String::len).sum::<usize>().max(1);
        let mut start = from;
        for piece in pieces {
            let end = start + (to - from) * piece.len() as f64 / total as f64;
            self.add_caption(start, end, &piece);
            start = end;
        }
    }

    pub fn import_srt(&mut self, text: &str) -> usize {
        let mut added = 0;
        let normalized = text.replace("\r\n", "\n");
        for block in normalized.split("\n\n") {
            let mut lines = block.lines().filter(|line| !line.trim().is_empty());
            let first = lines.next();
            let timing = if first.is_some_and(|line| line.contains("-->")) { first } else { lines.next() };
            let Some((from, to)) = timing.and_then(|line| line.split_once("-->")) else { continue };
            let (Some(from), Some(to)) = (srt_seconds(from), srt_seconds(to)) else { continue };
            let caption: Vec<&str> = lines.collect();
            if caption.is_empty() || to <= from {
                continue;
            }
            self.add_caption(from, to, &caption.join("\n"));
            added += 1;
        }
        added
    }

    pub fn end_frame(&self) -> u32 {
        self.clips.iter().map(|clip| clip.end).max().unwrap_or(0)
    }

    fn track_of(&self, kind: TrackKind) -> usize {
        self.tracks.iter().position(|track| track.kind == kind).unwrap_or(0)
    }

    pub fn new_clip(&self, shape: Shape, start: u32) -> Clip {
        let index = self.clips.len() + 1;
        let (name, color, width, height) = match shape {
            Shape::Solid => ("Solid", [38, 92, 150], self.width as f32, self.height as f32),
            Shape::Rectangle => ("Rectangle", [230, 120, 60], 600.0, 340.0),
            Shape::Ellipse => ("Ellipse", [150, 90, 220], 400.0, 400.0),
        };
        Clip {
            name: format!("{name} {index}"),
            track: self.track_of(TrackKind::Video),
            media: None,
            offset: 0,
            shape,
            color,
            start,
            end: start + self.fps * 5,
            width,
            height,
            x: Param::new(self.width as f32 / 2.0),
            y: Param::new(self.height as f32 / 2.0),
            scale: Param::new(100.0),
            rotation: Param::new(0.0),
            opacity: Param::new(100.0),
            effects: Vec::new(),
            visible: true,
            text: None,
            text_size: default_text_size(),
            fade_in: 0,
            fade_out: 0,
            volume: default_volume(),
            speed: 1.0,
            reverse: false,
            blend: Blend::Normal,
            transition_in: Transition::Fade,
            transition_out: Transition::Fade,
            mask: Mask::default(),
            text_stroke: 0.0,
            text_background: false,
            typewriter: false,
            nested: None,
            rotation_x: default_zero(),
            rotation_y: default_zero(),
            depth: default_zero(),
            muted: false,
            pan: 0.0,
            bass: 0.0,
            treble: 0.0,
        }
    }

    pub fn new_media_clip(&self, path: &str, start: u32, size: Option<(u32, u32)>, seconds: f64) -> Clip {
        let mut clip = self.new_clip(Shape::Solid, start);
        match size {
            Some((width, height)) => {
                clip.width = width as f32;
                clip.height = height as f32;
                let fit = (self.width as f32 / clip.width.max(1.0)).min(self.height as f32 / clip.height.max(1.0));
                clip.scale = Param::new(fit * 100.0);
            }
            None => clip.track = self.track_of(TrackKind::Audio),
        }
        clip.end = start + ((seconds * self.fps as f64).ceil() as u32).max(1);
        clip.name = std::path::Path::new(path).file_name().map_or(path.to_string(), |name| name.to_string_lossy().into_owned());
        clip.media = Some(path.to_string());
        clip
    }

    pub fn split(&mut self, index: usize, frame: u32) {
        let Some(clip) = self.clips.get_mut(index) else { return };
        if frame <= clip.start || frame >= clip.end {
            return;
        }
        let mut right = clip.clone();
        clip.end = frame;
        right.offset += frame - right.start;
        right.start = frame;
        self.clips.push(right);
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(text: &str) -> serde_json::Result<Self> {
        let mut project: Self = serde_json::from_str(text)?;
        project.sanitize();
        Ok(project)
    }

    pub fn sanitize(&mut self) {
        self.width = self.width.clamp(16, 7680);
        self.height = self.height.clamp(16, 4320);
        self.fps = self.fps.clamp(1, 240);
        self.duration = self.duration.max(1);
        if self.tracks.is_empty() {
            self.tracks = Self::default().tracks;
        }
        let last = self.tracks.len() - 1;
        for clip in &mut self.clips {
            clip.track = clip.track.min(last);
            clip.end = clip.end.max(clip.start + 1);
            clip.speed = clip.speed.clamp(0.1, 16.0);
            for param in [&mut clip.x, &mut clip.y, &mut clip.scale, &mut clip.rotation, &mut clip.opacity, &mut clip.volume] {
                param.keys.sort_by_key(|key| key.frame);
                param.keys.dedup_by_key(|key| key.frame);
            }
        }
    }
}

pub const MAX_NESTING: u32 = 8;

pub fn media_key(path: &str, seconds: f64) -> String {
    format!("{path}\u{0}{}", (seconds * 1000.0).round() as i64)
}

#[derive(Default)]
pub struct History {
    undo: Vec<Project>,
    redo: Vec<Project>,
}

impl History {
    const LIMIT: usize = 100;

    pub fn record(&mut self, before: Project) {
        if self.undo.len() == Self::LIMIT {
            self.undo.remove(0);
        }
        self.undo.push(before);
        self.redo.clear();
    }

    pub fn undo(&mut self, current: &mut Project) -> bool {
        match self.undo.pop() {
            Some(previous) => {
                self.redo.push(std::mem::replace(current, previous));
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self, current: &mut Project) -> bool {
        match self.redo.pop() {
            Some(next) => {
                self.undo.push(std::mem::replace(current, next));
                true
            }
            None => false,
        }
    }
}

fn srt_seconds(text: &str) -> Option<f64> {
    let cleaned = text.trim().replace(',', ".");
    let mut parts = cleaned.split(':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

pub fn timecode(frame: u32, fps: u32) -> String {
    let fps = fps.max(1);
    let seconds = frame / fps;
    format!("{:02}:{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60, frame % fps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_interpolate_and_clamp() {
        let mut param = Param::new(5.0);
        assert_eq!(param.at(3.0), 5.0);
        param.keys.push(Key { frame: 0, value: 0.0, ease: Ease::Linear });
        param.keys.push(Key { frame: 10, value: 10.0, ease: Ease::Linear });
        assert_eq!(param.at(5.0), 5.0);
        assert_eq!(param.at(20.0), 10.0);
    }

    #[test]
    fn hold_keeps_the_first_value() {
        let mut param = Param::new(0.0);
        param.keys.push(Key { frame: 0, value: 1.0, ease: Ease::Hold });
        param.keys.push(Key { frame: 10, value: 9.0, ease: Ease::Hold });
        assert_eq!(param.at(9.0), 1.0);
    }

    #[test]
    fn undo_and_redo_restore_projects() {
        let mut project = Project::default();
        let mut history = History::default();
        history.record(project.clone());
        project.fps = 60;
        assert!(history.undo(&mut project));
        assert_eq!(project.fps, 30);
        assert!(history.redo(&mut project));
        assert_eq!(project.fps, 60);
    }

    #[test]
    fn broken_files_are_repaired_or_refused() {
        assert!(Project::from_json("{").is_err());
        let mut project = Project { fps: 0, ..Project::default() };
        project.sanitize();
        assert_eq!(project.fps, 1);
    }

    #[test]
    fn fades_ramp_and_ripple_closes_gaps() {
        let mut project = Project::default();
        let mut first = project.new_clip(Shape::Solid, 0);
        first.end = 10;
        first.fade_in = 10;
        assert_eq!(first.fade(5.0), 0.5);
        let mut second = project.new_clip(Shape::Solid, 10);
        second.end = 20;
        project.clips = vec![first, second];
        project.ripple_delete(0);
        assert_eq!((project.clips[0].start, project.clips[0].end), (0, 10));
    }

    #[test]
    fn snapping_finds_close_points() {
        let mut project = Project::default();
        project.clips.push(project.new_clip(Shape::Solid, 100));
        assert_eq!(project.snap(103, None, 0, 5), 100);
        assert_eq!(project.snap(120, None, 0, 5), 120);
    }

    #[test]
    fn old_project_files_still_load() {
        let mut project = Project::default();
        project.clips.push(project.new_clip(Shape::Solid, 0));
        let mut value: serde_json::Value = serde_json::from_str(&project.to_json().unwrap()).unwrap();
        value["clips"][0].as_object_mut().unwrap().remove("volume");
        value.as_object_mut().unwrap().remove("markers");
        let loaded = Project::from_json(&value.to_string()).unwrap();
        assert_eq!(loaded.clips[0].volume.base, 100.0);
    }

    #[test]
    fn srt_captions_become_text_clips() {
        let mut project = Project::default();
        let added = project.import_srt("1\n00:00:01,000 --> 00:00:02,500\nHello there\n\nbroken block\n\n2\n00:00:03,000 --> 00:00:04,000\nSecond\nline\n");
        assert_eq!(added, 2);
        assert_eq!(project.clips[0].start, 30);
        assert_eq!(project.clips[1].text.as_deref(), Some("Second\nline"));
    }

    #[test]
    fn nesting_wraps_a_clip_and_finds_its_media() {
        let mut project = Project::default();
        project.clips.push(project.new_media_clip("a.mp4", 10, Some((16, 16)), 1.0));
        project.nest(0);
        assert_eq!(project.clips.len(), 1);
        assert_eq!(project.clips[0].start, 10);
        let mut found = Vec::new();
        project.wanted_media(15, 0, &mut found);
        assert_eq!(found[0].0, "a.mp4");
        assert!(found[0].1 > 0.1);
    }

    #[test]
    fn looks_exist_and_aspect_recenters() {
        for name in LOOKS {
            assert!(!look(name).is_empty());
        }
        let mut project = Project::default();
        project.clips.push(project.new_clip(Shape::Solid, 0));
        project.set_aspect(1080, 1920);
        assert_eq!(project.clips[0].x.base, 540.0);
        assert_eq!(project.clips[0].y.base, 960.0);
    }

    #[test]
    fn spoken_captions_split_into_short_lines() {
        let mut project = Project::default();
        project.add_spoken_caption(0.0, 10.0, "one two three four five six seven eight nine");
        assert_eq!(project.clips.len(), 2);
        assert_eq!(project.clips[1].text.as_deref(), Some("eight nine"));
    }

    #[test]
    fn split_makes_two_clips() {
        let mut project = Project::default();
        project.clips.push(project.new_clip(Shape::Solid, 0));
        project.split(0, 10);
        assert_eq!(project.clips.len(), 2);
        assert_eq!((project.clips[0].end, project.clips[1].start), (10, 10));
        project.split(0, 500);
        assert_eq!(project.clips.len(), 2);
    }

    #[test]
    fn timecode_formats() {
        assert_eq!(timecode(95, 30), "00:00:03:05");
    }
}
