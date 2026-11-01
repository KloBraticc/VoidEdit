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
}

impl Param {
    pub fn new(base: f32) -> Self {
        Self { base, keys: Vec::new() }
    }

    pub fn at(&self, frame: f32) -> f32 {
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
    HueShift { degrees: Param },
    Gamma { amount: Param },
    Levels { black: Param, white: Param },
    ColorBalance { red: Param, green: Param, blue: Param },
    Threshold { level: Param },
    Noise { amount: Param },
    ChromaKey { color: [u8; 3], tolerance: Param, softness: Param },
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
}

impl Default for Mask {
    fn default() -> Self {
        Self { shape: MaskShape::None, x: 0.5, y: 0.5, width: 0.6, height: 0.6, feather: 0.05, invert: false }
    }
}

pub const EFFECT_NAMES: [&str; 22] = [
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

    pub fn name(&self) -> &'static str {
        match self {
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
        Self { name: name.into(), kind, muted: false, locked: false }
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
