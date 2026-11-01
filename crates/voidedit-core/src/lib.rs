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
}

impl Effect {
    pub fn name(&self) -> &'static str {
        match self {
            Effect::Brightness { .. } => "Brightness",
            Effect::Tint { .. } => "Tint",
            Effect::Invert => "Invert",
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
                Track { name: "Audio 1".into(), kind: TrackKind::Audio },
                Track { name: "Video 1".into(), kind: TrackKind::Video },
                Track { name: "Video 2".into(), kind: TrackKind::Video },
                Track { name: "Video 3".into(), kind: TrackKind::Video },
            ],
            clips: Vec::new(),
            media: Vec::new(),
        }
    }
}

impl Project {
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
            for param in [&mut clip.x, &mut clip.y, &mut clip.scale, &mut clip.rotation, &mut clip.opacity] {
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
