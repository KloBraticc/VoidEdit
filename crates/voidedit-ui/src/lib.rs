mod settings;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use settings::Settings;
use voidedit_core::{timecode, Clip, Effect, History, MaskShape, Param, Project, Shape, TrackKind, BLENDS, EFFECT_NAMES, TRANSITIONS};
use voidedit_export::{audio_clips, export_with, ExportFormat, ExportOptions, Progress};
use voidedit_gpu::GpuCompositor;
use voidedit_media::{track_point, waveform, Audio, FrameWant, Frames, Library};
use voidedit_render::{render, Frame};

const HEADER_WIDTH: f32 = 128.0;
const RULER_HEIGHT: f32 = 28.0;
const TRACK_HEIGHT: f32 = 64.0;
const AUTOSAVE_SECONDS: f32 = 60.0;
const WAVEFORM_POINTS: usize = 800;

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Select,
    Razor,
}

#[derive(Clone, Copy, PartialEq)]
enum LeftTab {
    Project,
    Effects,
}

#[derive(Clone, Copy)]
enum Drag {
    Move { grab: f32 },
    TrimStart,
    TrimEnd,
}

struct TrackJob {
    points: Arc<Mutex<Vec<(u32, (i64, i64))>>>,
    done: Arc<std::sync::atomic::AtomicBool>,
    target: usize,
    source_clip: usize,
    origin: (f32, f32),
    start: (i64, i64),
}

struct ExportJob {
    progress: Arc<Progress>,
    result: Arc<Mutex<Option<Result<(), String>>>>,
    started: Instant,
}

pub fn prefers_gpu_preview() -> bool {
    Settings::load().gpu_preview
}

pub struct App {
    project: Project,
    committed: Project,
    history: History,
    path: Option<PathBuf>,
    frame: u32,
    playing: bool,
    last_tick: Instant,
    last_autosave: Instant,
    remainder: f32,
    selected: Option<usize>,
    clipboard: Option<Clip>,
    tool: Tool,
    left: LeftTab,
    drawing_mask: bool,
    picking_track: bool,
    tracking: Option<TrackJob>,
    track_target: usize,
    show_mixer: bool,
    drag: Option<(usize, Drag)>,
    zoom: f32,
    scroll: f32,
    search: String,
    preview: Frame,
    texture: Option<egui::TextureHandle>,
    gpu: Option<GpuCompositor>,
    gpu_texture: Option<egui::TextureId>,
    gpu_used: bool,
    render_state: Option<eframe::egui_wgpu::RenderState>,
    rendered: Option<(u32, Project, f32, u64)>,
    library: Library,
    frames: Frames,
    audio: Audio,
    audio_playing: bool,
    audio_project: Option<Project>,
    meter: f32,
    waves: Arc<Mutex<HashMap<String, Option<Vec<f32>>>>>,
    status: String,
    update_status: Arc<Mutex<String>>,
    settings: Settings,
    show_settings: bool,
    show_shortcuts: bool,
    show_export: bool,
    options: ExportOptions,
    show_project: bool,
    show_controls: bool,
    show_timeline: bool,
    job: Option<ExportJob>,
    context: egui::Context,
}

impl App {
    pub fn new(ctx: &egui::Context, update_status: Arc<Mutex<String>>) -> Self {
        let library = Library::default();
        let repaint = ctx.clone();
        let frames = Frames::new(library.clone(), Box::new(move || repaint.request_repaint()));
        let audio = Audio::new(library.clone());
        let project = Project::default();
        let settings = Settings::load();
        let app = Self {
            committed: project.clone(),
            project,
            history: History::default(),
            path: None,
            frame: 0,
            playing: false,
            last_tick: Instant::now(),
            last_autosave: Instant::now(),
            remainder: 0.0,
            selected: None,
            clipboard: None,
            tool: Tool::Select,
            left: LeftTab::Project,
            drawing_mask: false,
            picking_track: false,
            tracking: None,
            track_target: 0,
            show_mixer: false,
            drag: None,
            zoom: 6.0,
            scroll: 0.0,
            search: String::new(),
            preview: Frame::default(),
            texture: None,
            gpu: None,
            gpu_texture: None,
            gpu_used: false,
            render_state: None,
            rendered: None,
            library,
            frames,
            audio,
            audio_playing: false,
            audio_project: None,
            meter: 0.0,
            waves: Arc::default(),
            status: String::new(),
            update_status,
            settings,
            show_settings: false,
            show_shortcuts: false,
            show_export: false,
            options: ExportOptions::default(),
            show_project: true,
            show_controls: true,
            show_timeline: true,
            job: None,
            context: ctx.clone(),
        };
        app.apply_style();
        app
    }

    pub fn use_gpu(&mut self, state: eframe::egui_wgpu::RenderState) {
        self.gpu = Some(GpuCompositor::new(state.device.clone(), state.queue.clone()));
        self.render_state = Some(state);
    }

    fn render_gpu(&mut self, scale: f32) -> bool {
        let frames = self.frames.clone();
        let (Some(gpu), Some(state)) = (&mut self.gpu, &self.render_state) else { return false };
        let Some(resized) = gpu.render(&self.project, self.frame, scale, &|path| frames.latest(path)) else { return false };
        if resized || self.gpu_texture.is_none() {
            let Some(view) = gpu.display_view() else { return false };
            let mut renderer = state.renderer.write();
            if let Some(old) = self.gpu_texture.take() {
                renderer.free_texture(&old);
            }
            self.gpu_texture = Some(renderer.register_native_texture(&state.device, &view, eframe::wgpu::FilterMode::Linear));
        }
        true
    }

    fn start_tracking(&mut self, source_clip: usize, point: (i64, i64)) {
        let Some(clip) = self.project.clips.get(source_clip) else { return };
        let Some(path) = clip.media.clone() else { return };
        let fps = self.project.fps;
        let first = (self.frame.saturating_sub(clip.start) + clip.offset).max(clip.offset);
        let last = clip.end - clip.start + clip.offset;
        let target = self.track_target.min(self.project.clips.len().saturating_sub(1));
        let Some(follower) = self.project.clips.get(target) else { return };
        let origin = (follower.x.at(self.frame as f32), follower.y.at(self.frame as f32));
        let points = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (library, shared, finished, repaint) = (self.library.clone(), points.clone(), done.clone(), self.context.clone());
        let _ = std::thread::Builder::new().name("tracking".into()).spawn(move || {
            let _ = track_point(&library, &path, fps, first, last, point, |frame, found| {
                if let Ok(mut list) = shared.lock() {
                    list.push((frame, found));
                }
                repaint.request_repaint();
                true
            });
            finished.store(true, Ordering::Relaxed);
            repaint.request_repaint();
        });
        self.tracking = Some(TrackJob { points, done, target, source_clip, origin, start: point });
        self.status = "Tracking".into();
    }

    fn finish_tracking(&mut self) {
        let Some(job) = &self.tracking else { return };
        if !job.done.load(Ordering::Relaxed) {
            return;
        }
        let points = job.points.lock().map(|list| list.clone()).unwrap_or_default();
        let (target, source_clip, origin, start) = (job.target, job.source_clip, job.origin, job.start);
        self.tracking = None;
        let Some(source) = self.project.clips.get(source_clip).cloned() else { return };
        let ratio_x = source.scale.at(self.frame as f32) / 100.0;
        if let Some(follower) = self.project.clips.get_mut(target) {
            for (media_frame, point) in &points {
                let frame = media_frame + source.start - source.offset;
                follower.x.set_key(frame, origin.0 + (point.0 - start.0) as f32 * ratio_x);
                follower.y.set_key(frame, origin.1 + (point.1 - start.1) as f32 * ratio_x);
            }
        }
        self.status = format!("Tracked {} frames", points.len());
    }

    fn accent(&self) -> Color32 {
        let [r, g, b] = self.settings.accent;
        Color32::from_rgb(r, g, b)
    }

    fn apply_style(&self) {
        let ctx = &self.context;
        ctx.set_zoom_factor(self.settings.ui_scale.clamp(0.6, 2.0));
        let mut visuals = egui::Visuals::dark();
        let base = self.settings.panel_gray;
        visuals.panel_fill = Color32::from_gray(base);
        visuals.window_fill = Color32::from_gray(base.saturating_add(6));
        visuals.extreme_bg_color = Color32::from_gray(base.saturating_sub(12));
        visuals.selection.bg_fill = self.accent();
        visuals.widgets.hovered.bg_fill = self.accent().gamma_multiply(0.45);
        visuals.widgets.active.bg_fill = self.accent();
        visuals.window_corner_radius = 10.into();
        visuals.menu_corner_radius = 8.into();
        visuals.widgets.noninteractive.corner_radius = 5.into();
        visuals.widgets.inactive.corner_radius = 5.into();
        visuals.widgets.hovered.corner_radius = 5.into();
        visuals.widgets.active.corner_radius = 5.into();
        ctx.set_visuals(visuals);
        ctx.style_mut(|style| {
            style.animation_time = 0.12;
            style.spacing.item_spacing = Vec2::new(7.0, 6.0);
            style.spacing.button_padding = Vec2::new(8.0, 4.0);
        });
    }

    fn undo(&mut self) {
        if self.history.undo(&mut self.project) {
            self.committed = self.project.clone();
            self.selected = None;
        }
    }

    fn redo(&mut self) {
        if self.history.redo(&mut self.project) {
            self.committed = self.project.clone();
            self.selected = None;
        }
    }

    fn new_project(&mut self) {
        self.project = Project::default();
        self.committed = self.project.clone();
        self.history = History::default();
        self.path = None;
        self.selected = None;
        self.frame = 0;
    }

    fn open(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Void Edit project", &["voidedit"]).pick_file() {
            self.open_path(path);
        }
    }

    pub fn open_path(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| Project::from_json(&t).map_err(|e| e.to_string())) {
            Ok(project) => {
                self.project = project;
                self.committed = self.project.clone();
                self.history = History::default();
                self.path = Some(path);
                self.selected = None;
                self.frame = 0;
                for file in self.project.media.clone() {
                    self.add_media(file);
                }
            }
            Err(error) => self.status = format!("Could not open the project: {error}"),
        }
    }

    fn write_project(&self, path: &std::path::Path) -> Result<(), String> {
        let temporary = path.with_extension("voidedit.tmp");
        let text = self.project.to_json().map_err(|e| e.to_string())?;
        std::fs::write(&temporary, text).map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, path).map_err(|e| e.to_string())
    }

    fn save(&mut self, choose: bool) {
        let path = match (&self.path, choose) {
            (Some(path), false) => Some(path.clone()),
            _ => rfd::FileDialog::new().add_filter("Void Edit project", &["voidedit"]).set_file_name("Untitled.voidedit").save_file(),
        };
        let Some(path) = path else { return };
        match self.write_project(&path) {
            Ok(()) => {
                self.status = "Saved".into();
                self.path = Some(path);
            }
            Err(error) => self.status = format!("Could not save: {error}"),
        }
    }

    fn autosave(&mut self) {
        if self.last_autosave.elapsed().as_secs_f32() < AUTOSAVE_SECONDS {
            return;
        }
        self.last_autosave = Instant::now();
        if self.project.clips.is_empty() {
            return;
        }
        let target = self.path.as_ref().map(|path| path.with_extension("autosave.voidedit")).or_else(|| Settings::folder().map(|folder| folder.join("autosave.voidedit")));
        if let Some(target) = target {
            let _ = self.write_project(&target);
        }
    }

    fn import(&mut self) {
        if let Some(files) = rfd::FileDialog::new().pick_files() {
            for file in files {
                self.add_media(file.to_string_lossy().into_owned());
            }
        }
    }

    fn import_captions(&mut self) {
        let Some(file) = rfd::FileDialog::new().add_filter("Captions", &["srt"]).pick_file() else { return };
        match std::fs::read_to_string(&file) {
            Ok(text) => {
                let added = self.project.import_srt(&text);
                self.status = format!("Added {added} captions");
            }
            Err(error) => self.status = format!("Could not read the captions: {error}"),
        }
    }

    fn add_media(&mut self, file: String) {
        if !self.project.media.contains(&file) {
            self.project.media.push(file.clone());
        }
        let library = self.library.clone();
        let waves = self.waves.clone();
        let repaint = self.context.clone();
        let _ = std::thread::Builder::new().name("waveform".into()).spawn(move || {
            let peaks = waveform(&library, &file, WAVEFORM_POINTS);
            if let Ok(mut waves) = waves.lock() {
                waves.insert(file, peaks);
            }
            repaint.request_repaint();
        });
    }

    fn insert_media(&mut self, file: &str) {
        match self.library.details(file) {
            Some(details) => {
                let size = details.has_video.then_some((details.width, details.height));
                let clip = self.project.new_media_clip(file, self.frame, size, details.seconds);
                self.push_clip(clip);
            }
            None => self.status = "This file could not be opened".into(),
        }
    }

    fn push_clip(&mut self, clip: Clip) {
        self.project.duration = self.project.duration.max(clip.end + self.project.fps);
        self.project.clips.push(clip);
        self.selected = Some(self.project.clips.len() - 1);
    }

    fn add_clip(&mut self, shape: Shape) {
        let clip = self.project.new_clip(shape, self.frame);
        self.push_clip(clip);
    }

    fn add_text(&mut self) {
        let clip = self.project.new_text(self.frame);
        self.push_clip(clip);
    }

    fn delete_selected(&mut self, ripple: bool) {
        if let Some(index) = self.selected.take() {
            if ripple {
                self.project.ripple_delete(index);
            } else if index < self.project.clips.len() {
                self.project.clips.remove(index);
            }
        }
    }

    fn copy(&mut self) {
        self.clipboard = self.selected.and_then(|index| self.project.clips.get(index)).cloned();
    }

    fn paste(&mut self) {
        if let Some(mut clip) = self.clipboard.clone() {
            let length = clip.end - clip.start;
            clip.start = self.frame;
            clip.end = self.frame + length;
            self.push_clip(clip);
        }
    }

    fn split_at_playhead(&mut self) {
        let frame = self.frame;
        for index in 0..self.project.clips.len() {
            let clip = &self.project.clips[index];
            if frame > clip.start && frame < clip.end {
                self.project.split(index, frame);
            }
        }
    }

    fn toggle_marker(&mut self) {
        match self.project.markers.binary_search(&self.frame) {
            Ok(index) => {
                self.project.markers.remove(index);
            }
            Err(index) => self.project.markers.insert(index, self.frame),
        }
    }

    fn jump_edit(&mut self, forward: bool) {
        let mut points: Vec<u32> = self.project.clips.iter().flat_map(|clip| [clip.start, clip.end]).chain(self.project.markers.iter().copied()).collect();
        points.push(0);
        points.sort_unstable();
        points.dedup();
        let next = if forward { points.iter().find(|point| **point > self.frame).copied() } else { points.iter().rev().find(|point| **point < self.frame).copied() };
        if let Some(next) = next {
            self.frame = next.min(self.project.duration - 1);
        }
    }

    fn start_export(&mut self) {
        if self.job.is_some() {
            return;
        }
        if self.project.clips.is_empty() {
            self.status = "Add a clip before exporting".into();
            return;
        }
        let dialog = rfd::FileDialog::new();
        let chosen = match self.options.format {
            ExportFormat::Mp4 => dialog.add_filter("MP4 video", &["mp4"]).set_file_name("Export.mp4").save_file(),
            ExportFormat::Wav => dialog.add_filter("WAV audio", &["wav"]).set_file_name("Export.wav").save_file(),
            ExportFormat::PngSequence => dialog.pick_folder(),
        };
        let Some(path) = chosen else {
            return;
        };
        self.show_export = false;
        let options = self.options;
        let progress = Arc::new(Progress::default());
        let result = Arc::new(Mutex::new(None));
        let (project, library, shared, finished, repaint) = (self.project.clone(), self.library.clone(), progress.clone(), result.clone(), self.context.clone());
        let spawned = std::thread::Builder::new().name("export".into()).spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| export_with(&project, &library, &path, options, &shared))).unwrap_or_else(|_| Err("the export stopped unexpectedly".into()));
            if let Ok(mut slot) = finished.lock() {
                *slot = Some(outcome);
            }
            repaint.request_repaint();
        });
        if spawned.is_ok() {
            self.job = Some(ExportJob { progress, result, started: Instant::now() });
        }
    }

    fn input(&mut self, ctx: &egui::Context) {
        for file in ctx.input(|i| i.raw.dropped_files.clone()) {
            if let Some(path) = file.path {
                self.add_media(path.to_string_lossy().into_owned());
            }
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        let command = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, key));
        let shift_command = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, key));
        let shift = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, key));
        let plain = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
        if shift_command(egui::Key::Z) {
            self.redo();
        } else if command(egui::Key::Z) {
            self.undo();
        }
        if shift_command(egui::Key::S) {
            self.save(true);
        } else if command(egui::Key::S) {
            self.save(false);
        }
        if command(egui::Key::K) {
            self.split_at_playhead();
        }
        if command(egui::Key::C) {
            self.copy();
        }
        if command(egui::Key::V) {
            self.paste();
        }
        if command(egui::Key::D) {
            self.copy();
            self.paste();
        }
        if command(egui::Key::I) {
            self.import();
        }
        if command(egui::Key::M) {
            self.show_export = true;
        }
        let bound = |settings: &Settings, action: &str| settings.key(action).is_some_and(|key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)));
        if bound(&self.settings, "Play or pause") {
            self.playing = !self.playing;
        }
        if shift(egui::Key::Delete) {
            self.delete_selected(true);
        } else if plain(egui::Key::Delete) || plain(egui::Key::Backspace) {
            self.delete_selected(false);
        }
        if bound(&self.settings, "Selection tool") {
            self.tool = Tool::Select;
        }
        if bound(&self.settings, "Razor tool") {
            self.tool = Tool::Razor;
        }
        if bound(&self.settings, "Marker") {
            self.toggle_marker();
        }
        if bound(&self.settings, "New text") {
            self.add_text();
        }
        if plain(egui::Key::ArrowLeft) {
            self.frame = self.frame.saturating_sub(1);
        }
        if plain(egui::Key::ArrowRight) {
            self.frame = (self.frame + 1).min(self.project.duration - 1);
        }
        if plain(egui::Key::ArrowUp) {
            self.jump_edit(false);
        }
        if plain(egui::Key::ArrowDown) {
            self.jump_edit(true);
        }
        if plain(egui::Key::Home) {
            self.frame = 0;
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let elapsed = (now - self.last_tick).as_secs_f32();
        self.last_tick = now;
        if !self.playing {
            self.remainder = 0.0;
            return;
        }
        self.remainder += elapsed * self.project.fps as f32;
        let whole = self.remainder as u32;
        self.remainder -= whole as f32;
        self.frame += whole;
        let end = self.project.end_frame().max(1).min(self.project.duration);
        if self.frame >= end {
            self.frame = 0;
            self.playing = false;
        }
        ctx.request_repaint();
    }

    fn sync_audio(&mut self, ctx: &egui::Context) {
        if self.audio_project.as_ref() != Some(&self.project) {
            self.audio.set_clips(audio_clips(&self.project));
            self.audio_project = Some(self.project.clone());
        }
        if self.playing != self.audio_playing {
            self.audio_playing = self.playing;
            if self.playing {
                self.audio.play(self.frame as f64 / self.project.fps as f64);
            } else {
                self.audio.stop();
            }
        }
        let peak = self.audio.peak();
        self.meter = peak.max(self.meter * 0.9).clamp(0.0, 1.0);
        if self.meter > 0.005 {
            ctx.request_repaint();
        }
    }

    fn menu(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    let items: [(&str, fn(&mut App)); 6] = [
                        ("New", App::new_project),
                        ("Open...", App::open),
                        ("Save", |app| app.save(false)),
                        ("Save As...", |app| app.save(true)),
                        ("Import Media...", App::import),
                        ("Export...", |app| app.show_export = true),
                    ];
                    for (label, action) in items {
                        if ui.button(label).clicked() {
                            ui.close_menu();
                            action(self);
                        }
                    }
                });
                ui.menu_button("Edit", |ui| {
                    let items: [(&str, fn(&mut App)); 7] = [
                        ("Undo", App::undo),
                        ("Redo", App::redo),
                        ("Copy", App::copy),
                        ("Paste", App::paste),
                        ("Delete", |app| app.delete_selected(false)),
                        ("Ripple Delete", |app| app.delete_selected(true)),
                        ("Split at Playhead", App::split_at_playhead),
                    ];
                    for (label, action) in items {
                        if ui.button(label).clicked() {
                            ui.close_menu();
                            action(self);
                        }
                    }
                });
                ui.menu_button("Clip", |ui| {
                    if ui.button("New Text").clicked() {
                        self.add_text();
                        ui.close_menu();
                    }
                    for (label, shape) in [("New Solid", Shape::Solid), ("New Rectangle", Shape::Rectangle), ("New Ellipse", Shape::Ellipse)] {
                        if ui.button(label).clicked() {
                            self.add_clip(shape);
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button("Compose", |ui| {
                    if ui.add_enabled(self.selected.is_some(), egui::Button::new("Nest Selected Clip")).clicked() {
                        if let Some(index) = self.selected.take() {
                            self.project.nest(index);
                            self.selected = Some(self.project.clips.len() - 1);
                        }
                        ui.close_menu();
                    }
                });
                ui.menu_button("Captions", |ui| {
                    if ui.button("Import SRT...").clicked() {
                        ui.close_menu();
                        self.import_captions();
                    }
                });
                ui.menu_button("Sequence", |ui| {
                    if ui.button("Add Video Track").clicked() {
                        self.project.add_track(TrackKind::Video);
                        ui.close_menu();
                    }
                    if ui.button("Add Audio Track").clicked() {
                        self.project.add_track(TrackKind::Audio);
                        ui.close_menu();
                    }
                    if ui.button("Add or Remove Marker").clicked() {
                        self.toggle_marker();
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Size");
                        ui.add(egui::DragValue::new(&mut self.project.width).range(16..=7680));
                        ui.add(egui::DragValue::new(&mut self.project.height).range(16..=4320));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Frame rate");
                        ui.add(egui::DragValue::new(&mut self.project.fps).range(1..=240));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Background");
                        ui.color_edit_button_srgb(&mut self.project.background);
                    });
                });
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_project, "Project panel");
                    ui.checkbox(&mut self.show_controls, "Effect Controls panel");
                    ui.checkbox(&mut self.show_timeline, "Timeline");
                    ui.checkbox(&mut self.show_mixer, "Audio mixer");
                    if ui.button("Fit timeline").clicked() {
                        self.scroll = 0.0;
                        self.zoom = (1100.0 / self.project.end_frame().max(30) as f32).clamp(0.2, 60.0);
                        ui.close_menu();
                    }
                    if ui.button("Keyboard shortcuts").clicked() {
                        self.show_shortcuts = true;
                        ui.close_menu();
                    }
                });
                if ui.button("Settings").clicked() {
                    self.show_settings = true;
                }
                let name = self.path.as_ref().and_then(|p| p.file_name()).map_or("Untitled".to_string(), |n| n.to_string_lossy().into_owned());
                let changed = self.project != self.committed;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("Export").color(Color32::WHITE)).fill(self.accent())).clicked() {
                        self.show_export = true;
                    }
                    ui.label(RichText::new(if changed { format!("{name} *") } else { name }).weak());
                    if let Ok(update) = self.update_status.lock() {
                        if !update.is_empty() {
                            ui.label(RichText::new(update.as_str()).small().color(self.accent()));
                        }
                    }
                    if !self.status.is_empty() {
                        ui.label(RichText::new(&self.status).small());
                    }
                });
            });
        });
    }

    fn windows(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        let mut changed = false;
        egui::Window::new("Settings").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Accent color");
                changed |= ui.color_edit_button_srgb(&mut self.settings.accent).changed();
            });
            changed |= ui.add(egui::Slider::new(&mut self.settings.panel_gray, 15..=70).text("Panel shade")).changed();
            changed |= ui.add(egui::Slider::new(&mut self.settings.ui_scale, 0.6..=2.0).text("Interface size")).changed();
            changed |= ui.add(egui::Slider::new(&mut self.settings.preview_quality, 0.25..=1.0).text("Preview quality")).changed();
            changed |= ui.checkbox(&mut self.settings.snapping, "Snap clips while dragging").changed();
            changed |= ui.checkbox(&mut self.settings.gpu_preview, "GPU preview (uses more memory, applies after a restart)").changed();
            if ui.button("Reset").clicked() {
                self.settings = Settings::default();
                changed = true;
            }
        });
        self.show_settings = open;
        if changed {
            self.settings.save();
            self.apply_style();
        }
        let mut show_mixer = self.show_mixer;
        egui::Window::new("Audio Mixer").open(&mut show_mixer).resizable(false).show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                for track in self.project.tracks.iter_mut().filter(|track| track.kind == TrackKind::Audio) {
                    ui.vertical(|ui| {
                        ui.strong(&track.name);
                        ui.add(egui::Slider::new(&mut track.volume, 0.0..=200.0).vertical());
                        ui.add(egui::Slider::new(&mut track.pan, -1.0..=1.0).text("Pan"));
                        ui.checkbox(&mut track.muted, "Mute");
                    });
                }
            });
        });
        self.show_mixer = show_mixer;
        let mut go = false;
        let mut show_export = self.show_export;
        egui::Window::new("Export").open(&mut show_export).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (label, format) in [("MP4 video", ExportFormat::Mp4), ("PNG images", ExportFormat::PngSequence), ("WAV audio", ExportFormat::Wav)] {
                    ui.selectable_value(&mut self.options.format, format, label);
                }
            });
            if self.options.format != ExportFormat::Wav {
                ui.add(egui::Slider::new(&mut self.options.scale, 0.1..=1.0).text("Resolution"));
                let (width, height) = (self.project.width as f32 * self.options.scale, self.project.height as f32 * self.options.scale);
                ui.label(RichText::new(format!("{} x {}", width as u32 & !1, height as u32 & !1)).weak());
            }
            if self.options.format == ExportFormat::Mp4 {
                ui.add(egui::Slider::new(&mut self.options.quality, 40.0..=10.0).text("Quality"));
            }
            go = ui.button("Export").clicked();
        });
        self.show_export = show_export;
        if go {
            self.start_export();
        }
        let mut show_shortcuts = self.show_shortcuts;
        egui::Window::new("Keyboard shortcuts").open(&mut show_shortcuts).collapsible(false).show(ctx, |ui| {
            ui.strong("Change a key");
            for (action, default) in settings::ACTIONS {
                ui.horizontal(|ui| {
                    ui.add_sized([120.0, 16.0], egui::Label::new(action));
                    let mut text = self.settings.keys.get(action).cloned().unwrap_or_else(|| default.to_string());
                    if ui.add(egui::TextEdit::singleline(&mut text).desired_width(70.0)).changed() {
                        self.settings.keys.insert(action.to_string(), text.clone());
                        self.settings.save();
                    }
                    if egui::Key::from_name(&text).is_none() {
                        ui.colored_label(Color32::LIGHT_RED, "Unknown key");
                    }
                });
            }
            ui.separator();
            for (key, action) in [("Space", "Play or pause"), ("Left, Right", "Move one frame"), ("Up, Down", "Previous or next edit"), ("V", "Selection tool"), ("C", "Razor tool"), ("T", "New text"), ("M", "Marker"), ("Delete", "Delete clip"), ("Shift Delete", "Ripple delete"), ("Ctrl K", "Split at playhead"), ("Ctrl C, V, D", "Copy, paste, duplicate"), ("Ctrl Z", "Undo"), ("Ctrl Shift Z", "Redo"), ("Ctrl S", "Save"), ("Ctrl I", "Import"), ("Ctrl M", "Export"), ("Ctrl Scroll", "Zoom the timeline")] {
                ui.horizontal(|ui| {
                    ui.add_sized([120.0, 16.0], egui::Label::new(RichText::new(key).monospace()));
                    ui.label(action);
                });
            }
        });
        self.show_shortcuts = show_shortcuts;
        let mut finished = None;
        if let Some(job) = &self.job {
            let (done, total) = (job.progress.done.load(Ordering::Relaxed), job.progress.total.load(Ordering::Relaxed).max(1));
            let elapsed = job.started.elapsed().as_secs_f32();
            egui::Window::new("Exporting").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                ui.add(egui::ProgressBar::new(done as f32 / total as f32).show_percentage().desired_width(340.0));
                let rate = done as f32 / elapsed.max(0.01);
                ui.label(format!("Frame {done} of {total}, {rate:.1} frames per second"));
                if ui.button("Cancel").clicked() {
                    job.progress.cancel.store(true, Ordering::Relaxed);
                }
            });
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            if let Ok(mut slot) = job.result.lock() {
                finished = slot.take();
            }
        }
        if let Some(result) = finished {
            self.job = None;
            self.status = match result {
                Ok(()) => "Export finished".into(),
                Err(error) => format!("Export failed: {error}"),
            };
        }
    }

    fn tab(&self, ui: &mut egui::Ui, active: bool, label: &str) -> bool {
        let text = if active { RichText::new(label).strong() } else { RichText::new(label).weak() };
        let response = ui.add(egui::Label::new(text).sense(Sense::click()));
        let id = response.id.with("underline");
        let amount = ui.ctx().animate_bool_with_time(id, active, 0.15);
        if amount > 0.0 {
            let rect = response.rect;
            let center = rect.center().x;
            let half = rect.width() / 2.0 * amount;
            ui.painter().line_segment([Pos2::new(center - half, rect.bottom() + 2.0), Pos2::new(center + half, rect.bottom() + 2.0)], Stroke::new(2.0_f32, self.accent()));
        }
        response.clicked()
    }

    fn left_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.tab(ui, self.left == LeftTab::Project, "Project") {
                self.left = LeftTab::Project;
            }
            if self.tab(ui, self.left == LeftTab::Effects, "Effects") {
                self.left = LeftTab::Effects;
            }
        });
        ui.add_space(4.0);
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(f32::INFINITY));
        let filter = self.search.to_lowercase();
        match self.left {
            LeftTab::Project => {
                ui.horizontal(|ui| {
                    if ui.button("Import").clicked() {
                        self.import();
                    }
                    if ui.button("Text").clicked() {
                        self.add_text();
                    }
                    ui.label(RichText::new(format!("{} items", self.project.media.len())).weak());
                });
                ui.separator();
                let mut add = None;
                let mut remove = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.project.media.is_empty() {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| ui.label(RichText::new("Drop video, audio or image files here").weak()));
                    }
                    for file in &self.project.media {
                        let name = std::path::Path::new(file).file_name().map_or(file.clone(), |n| n.to_string_lossy().into_owned());
                        if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                            continue;
                        }
                        let response = ui.selectable_label(false, &name).on_hover_text("Click to add at the playhead");
                        if response.clicked() {
                            add = Some(file.clone());
                        }
                        response.context_menu(|ui| {
                            if ui.button("Remove from project").clicked() {
                                remove = Some(file.clone());
                                ui.close_menu();
                            }
                        });
                    }
                });
                if let Some(file) = add {
                    self.insert_media(&file);
                }
                if let Some(file) = remove {
                    self.project.media.retain(|item| item != &file);
                }
            }
            LeftTab::Effects => {
                ui.separator();
                let selected = self.selected;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for name in EFFECT_NAMES {
                        if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                            continue;
                        }
                        let response = ui.add_enabled(selected.is_some(), egui::Button::new(name).min_size(Vec2::new(ui.available_width(), 24.0)));
                        if response.clicked() {
                            if let (Some(clip), Some(effect)) = (selected.and_then(|index| self.project.clips.get_mut(index)), Effect::create(name)) {
                                clip.effects.push(effect);
                            }
                        }
                    }
                    if selected.is_none() {
                        ui.label(RichText::new("Select a clip to add effects").weak());
                    }
                });
            }
        }
    }

    fn param(ui: &mut egui::Ui, accent: Color32, label: &str, param: &mut Param, frame: u32, speed: f32) {
        ui.horizontal(|ui| {
            let keyed = param.has_key(frame);
            let color = if keyed { accent } else if param.keys.is_empty() { Color32::DARK_GRAY } else { Color32::LIGHT_GRAY };
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::click());
            let center = rect.center();
            let size = if response.hovered() { 6.0 } else { 4.5 };
            ui.painter().add(egui::Shape::convex_polygon(vec![center + Vec2::new(0.0, -size), center + Vec2::new(size, 0.0), center + Vec2::new(0.0, size), center + Vec2::new(-size, 0.0)], color, Stroke::NONE));
            if response.on_hover_text("Add or remove a keyframe here").clicked() {
                param.toggle_key(frame);
            }
            ui.add_sized([86.0, 18.0], egui::Label::new(label));
            let mut value = param.at(frame as f32);
            if ui.add(egui::DragValue::new(&mut value).speed(speed).max_decimals(1)).changed() {
                param.set(frame, value);
            }
            let has_expression = param.expression.is_some();
            let mark = RichText::new("=").color(if has_expression { accent } else { Color32::DARK_GRAY });
            ui.menu_button(mark, |ui| {
                ui.label(RichText::new("value, frame, time, sin, cos, abs, min, max, wiggle").small().weak());
                let mut text = param.expression.clone().unwrap_or_default();
                if ui.add(egui::TextEdit::singleline(&mut text).hint_text("value + sin(time * 2) * 50")).changed() {
                    param.expression = (!text.trim().is_empty()).then_some(text);
                }
            });
        });
    }

    fn controls_panel(&mut self, ui: &mut egui::Ui) {
        self.tab(ui, true, "Effect Controls");
        ui.add_space(4.0);
        ui.separator();
        let frame = self.frame;
        let accent = self.accent();
        let names: Vec<String> = self.project.clips.iter().map(|clip| clip.name.clone()).collect();
        let Some(clip) = self.selected.and_then(|index| self.project.clips.get_mut(index)) else {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("Select a clip").weak()));
            return;
        };
        ui.add(egui::TextEdit::singleline(&mut clip.name).desired_width(f32::INFINITY));
        egui::ScrollArea::vertical().show(ui, |ui| {
            if let Some(text) = &mut clip.text {
                egui::CollapsingHeader::new("Text").default_open(true).show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(text).desired_rows(2).desired_width(f32::INFINITY));
                    ui.add(egui::Slider::new(&mut clip.text_size, 8.0..=400.0).text("Size"));
                    ui.add(egui::Slider::new(&mut clip.text_stroke, 0.0..=16.0).text("Outline"));
                    ui.checkbox(&mut clip.text_background, "Background");
                    ui.horizontal(|ui| {
                        ui.label("Color");
                        ui.color_edit_button_srgb(&mut clip.color);
                    });
                });
            }
            egui::CollapsingHeader::new("Motion").default_open(true).show(ui, |ui| {
                Self::param(ui, accent, "Position X", &mut clip.x, frame, 1.0);
                Self::param(ui, accent, "Position Y", &mut clip.y, frame, 1.0);
                Self::param(ui, accent, "Scale", &mut clip.scale, frame, 0.5);
                Self::param(ui, accent, "Rotation", &mut clip.rotation, frame, 0.5);
                Self::param(ui, accent, "3D Tilt X", &mut clip.rotation_x, frame, 0.5);
                Self::param(ui, accent, "3D Tilt Y", &mut clip.rotation_y, frame, 0.5);
                Self::param(ui, accent, "3D Depth", &mut clip.depth, frame, 2.0);
            });
            egui::CollapsingHeader::new("Opacity").default_open(true).show(ui, |ui| {
                Self::param(ui, accent, "Opacity", &mut clip.opacity, frame, 0.5);
                egui::ComboBox::from_label("Blend").selected_text(format!("{:?}", clip.blend)).show_ui(ui, |ui| {
                    for mode in BLENDS {
                        ui.selectable_value(&mut clip.blend, mode, format!("{mode:?}"));
                    }
                });
            });
            egui::CollapsingHeader::new("Transitions").default_open(true).show(ui, |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("in").selected_text(format!("{:?}", clip.transition_in)).show_ui(ui, |ui| {
                        for kind in TRANSITIONS {
                            ui.selectable_value(&mut clip.transition_in, kind, format!("{kind:?}"));
                        }
                    });
                    ui.label("In");
                    ui.add(egui::DragValue::new(&mut clip.fade_in).suffix(" f"));
                });
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("out").selected_text(format!("{:?}", clip.transition_out)).show_ui(ui, |ui| {
                        for kind in TRANSITIONS {
                            ui.selectable_value(&mut clip.transition_out, kind, format!("{kind:?}"));
                        }
                    });
                    ui.label("Out");
                    ui.add(egui::DragValue::new(&mut clip.fade_out).suffix(" f"));
                });
            });
            if clip.media.is_some() {
                egui::CollapsingHeader::new("Motion Tracker").default_open(false).show(ui, |ui| {
                    egui::ComboBox::from_label("Clip to move").selected_text(names.get(self.track_target).cloned().unwrap_or_default()).show_ui(ui, |ui| {
                        for (index, name) in names.iter().enumerate() {
                            ui.selectable_value(&mut self.track_target, index, name);
                        }
                    });
                    ui.toggle_value(&mut self.picking_track, "Click a point on the preview");
                    if self.tracking.is_some() {
                        ui.spinner();
                    }
                });
            }
            egui::CollapsingHeader::new("Mask").default_open(false).show(ui, |ui| {
                egui::ComboBox::from_label("Shape").selected_text(format!("{:?}", clip.mask.shape)).show_ui(ui, |ui| {
                    for shape in [MaskShape::None, MaskShape::Rectangle, MaskShape::Ellipse, MaskShape::Polygon] {
                        ui.selectable_value(&mut clip.mask.shape, shape, format!("{shape:?}"));
                    }
                });
                if clip.mask.shape == MaskShape::Polygon {
                    ui.horizontal(|ui| {
                        ui.toggle_value(&mut self.drawing_mask, "Draw on preview");
                        if ui.button("Clear points").clicked() {
                            clip.mask.points.clear();
                        }
                        ui.label(format!("{} points", clip.mask.points.len()));
                    });
                    ui.checkbox(&mut clip.mask.smooth, "Smooth curve");
                    ui.checkbox(&mut clip.mask.invert, "Invert");
                } else if clip.mask.shape != MaskShape::None {
                    ui.add(egui::Slider::new(&mut clip.mask.x, 0.0..=1.0).text("X"));
                    ui.add(egui::Slider::new(&mut clip.mask.y, 0.0..=1.0).text("Y"));
                    ui.add(egui::Slider::new(&mut clip.mask.width, 0.01..=2.0).text("Width"));
                    ui.add(egui::Slider::new(&mut clip.mask.height, 0.01..=2.0).text("Height"));
                    ui.add(egui::Slider::new(&mut clip.mask.feather, 0.0..=0.5).text("Feather"));
                    ui.checkbox(&mut clip.mask.invert, "Invert");
                }
            });
            if clip.media.is_some() {
                egui::CollapsingHeader::new("Time and Audio").default_open(true).show(ui, |ui| {
                    ui.add(egui::Slider::new(&mut clip.speed, 0.1..=16.0).logarithmic(true).text("Speed"));
                    ui.checkbox(&mut clip.reverse, "Reverse");
                    let mut volume = clip.volume.base;
                    if ui.add(egui::Slider::new(&mut volume, 0.0..=200.0).text("Volume")).changed() {
                        clip.volume.base = volume;
                    }
                    ui.checkbox(&mut clip.muted, "Mute clip");
                    ui.add(egui::Slider::new(&mut clip.pan, -1.0..=1.0).text("Pan"));
                    ui.add(egui::Slider::new(&mut clip.bass, -12.0..=12.0).text("Bass"));
                    ui.add(egui::Slider::new(&mut clip.treble, -12.0..=12.0).text("Treble"));
                });
            } else if clip.text.is_none() {
                ui.horizontal(|ui| {
                    ui.label("Color");
                    ui.color_edit_button_srgb(&mut clip.color);
                });
            }
            egui::CollapsingHeader::new(format!("Effects ({})", clip.effects.len())).default_open(true).show(ui, |ui| {
                let mut remove = None;
                for (index, effect) in clip.effects.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.strong(effect.name());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Remove").clicked() {
                                remove = Some(index);
                            }
                        });
                    });
                    match effect {
                        Effect::Brightness { amount } | Effect::Contrast { amount } | Effect::Saturation { amount } | Effect::Exposure { amount } | Effect::Temperature { amount } | Effect::Vignette { amount } => Self::param(ui, accent, "Amount", amount, frame, 0.5),
                        Effect::Tint { amount, color } => {
                            Self::param(ui, accent, "Amount", amount, frame, 0.5);
                            ui.color_edit_button_srgb(color);
                        }
                        Effect::Posterize { levels } => Self::param(ui, accent, "Levels", levels, frame, 0.1),
                        Effect::Pixelate { size } => Self::param(ui, accent, "Size", size, frame, 0.25),
                        Effect::Blur { radius } => Self::param(ui, accent, "Radius", radius, frame, 0.25),
                        Effect::ColorWheels { lift, gamma, gain } => {
                            for (name, wheel, low, high) in [("Lift", lift, -0.5, 0.5), ("Gamma", gamma, 0.2, 3.0), ("Gain", gain, 0.0, 3.0)] {
                                ui.label(name);
                                for (channel, label) in wheel.iter_mut().zip(["R", "G", "B"]) {
                                    ui.add(egui::Slider::new(channel, low..=high).text(label));
                                }
                            }
                        }
                        Effect::Curves { shadows, midtones, highlights } => {
                            Self::param(ui, accent, "Shadows", shadows, frame, 0.5);
                            Self::param(ui, accent, "Midtones", midtones, frame, 0.5);
                            Self::param(ui, accent, "Highlights", highlights, frame, 0.5);
                        }
                        Effect::Lut { path } => {
                            if ui.button(if path.is_empty() { "Choose .cube file" } else { path.as_str() }).clicked() {
                                if let Some(file) = rfd::FileDialog::new().add_filter("LUT", &["cube"]).pick_file() {
                                    *path = file.to_string_lossy().into_owned();
                                }
                            }
                        }
                        Effect::Sharpen { amount } | Effect::Glow { amount } => Self::param(ui, accent, "Amount", amount, frame, 0.5),
                        Effect::DropShadow { x, y, opacity } => {
                            Self::param(ui, accent, "Offset X", x, frame, 0.5);
                            Self::param(ui, accent, "Offset Y", y, frame, 0.5);
                            Self::param(ui, accent, "Opacity", opacity, frame, 0.5);
                        }
                        Effect::Wave { amplitude, frequency } => {
                            Self::param(ui, accent, "Amplitude", amplitude, frame, 0.5);
                            Self::param(ui, accent, "Frequency", frequency, frame, 0.1);
                        }
                        Effect::EdgeDetect | Effect::Mirror | Effect::Emboss | Effect::ChannelSwap => {}
                        Effect::ChromaticAberration { amount } => Self::param(ui, accent, "Amount", amount, frame, 0.25),
                        Effect::Scanlines { strength, spacing } => {
                            Self::param(ui, accent, "Strength", strength, frame, 0.5);
                            Self::param(ui, accent, "Spacing", spacing, frame, 0.25);
                        }
                        Effect::Swirl { angle } => Self::param(ui, accent, "Angle", angle, frame, 1.0),
                        Effect::Kaleidoscope { segments } => Self::param(ui, accent, "Segments", segments, frame, 0.1),
                        Effect::Fisheye { amount } => Self::param(ui, accent, "Amount", amount, frame, 0.5),
                        Effect::Solarize { level } => Self::param(ui, accent, "Level", level, frame, 1.0),
                        Effect::Vibrance { amount } => Self::param(ui, accent, "Amount", amount, frame, 0.5),
                        Effect::Duotone { dark, light } => {
                            ui.horizontal(|ui| {
                                ui.color_edit_button_srgb(dark);
                                ui.color_edit_button_srgb(light);
                            });
                        }
                        Effect::HueShift { degrees } => Self::param(ui, accent, "Degrees", degrees, frame, 1.0),
                        Effect::Gamma { amount } => Self::param(ui, accent, "Gamma", amount, frame, 0.01),
                        Effect::Levels { black, white } => {
                            Self::param(ui, accent, "Black", black, frame, 1.0);
                            Self::param(ui, accent, "White", white, frame, 1.0);
                        }
                        Effect::ColorBalance { red, green, blue } => {
                            Self::param(ui, accent, "Red", red, frame, 0.5);
                            Self::param(ui, accent, "Green", green, frame, 0.5);
                            Self::param(ui, accent, "Blue", blue, frame, 0.5);
                        }
                        Effect::Threshold { level } => Self::param(ui, accent, "Level", level, frame, 1.0),
                        Effect::Noise { amount } => Self::param(ui, accent, "Amount", amount, frame, 0.5),
                        Effect::ChromaKey { color, tolerance, softness } => {
                            ui.color_edit_button_srgb(color);
                            Self::param(ui, accent, "Tolerance", tolerance, frame, 0.5);
                            Self::param(ui, accent, "Softness", softness, frame, 0.5);
                        }
                        Effect::Invert | Effect::Grayscale | Effect::Sepia | Effect::FlipHorizontal | Effect::FlipVertical => {}
                    }
                }
                if let Some(index) = remove {
                    clip.effects.remove(index);
                }
            });
        });
    }

    fn monitor(&mut self, ui: &mut egui::Ui) {
        self.tab(ui, true, "Program");
        let controls = 60.0;
        let area = Vec2::new(ui.available_width(), (ui.available_height() - controls).max(40.0));
        let aspect = self.project.width as f32 / self.project.height as f32;
        let size = if area.x / area.y > aspect { Vec2::new(area.y * aspect, area.y) } else { Vec2::new(area.x, area.x / aspect) };
        let pixels = ui.ctx().pixels_per_point();
        let scale = (size.x * pixels / self.project.width as f32 * self.settings.preview_quality).clamp(0.05, 1.0);
        let fps = self.project.fps;
        let time = self.frame;
        let mut found = Vec::new();
        self.project.wanted_media(time, 0, &mut found);
        self.frames.want(found.into_iter().map(|(path, seconds)| FrameWant { path, seconds, scale }).collect());
        let generation = self.frames.generation();
        let stale = self.rendered.as_ref().is_none_or(|(frame, project, s, g)| *frame != self.frame || *s != scale || *g != generation || *project != self.project);
        if stale {
            self.gpu_used = self.render_gpu(scale);
            if !self.gpu_used {
                let frames = self.frames.clone();
                render(&self.project, self.frame, scale, &mut self.preview, &|path| frames.latest(path));
                let image = egui::ColorImage::from_rgba_unmultiplied([self.preview.width, self.preview.height], &self.preview.pixels);
                match &mut self.texture {
                    Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                    None => self.texture = Some(ui.ctx().load_texture("program", image, egui::TextureOptions::LINEAR)),
                }
            }
            self.rendered = Some((self.frame, self.project.clone(), scale, generation));
        }
        ui.allocate_ui_with_layout(area, egui::Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
            let shown = if self.gpu_used { self.gpu_texture } else { self.texture.as_ref().map(egui::TextureHandle::id) };
            if let Some(id) = shown {
                let picture = ui.add(egui::Image::new((id, size)).fit_to_exact_size(size).corner_radius(4).sense(Sense::click()));
                if self.picking_track && picture.clicked() {
                    if let (Some(position), Some(index)) = (picture.interact_pointer_pos(), self.selected) {
                        if let Some(clip) = self.project.clips.get(index) {
                            let px = (position.x - picture.rect.left()) / picture.rect.width() * self.project.width as f32;
                            let py = (position.y - picture.rect.top()) / picture.rect.height() * self.project.height as f32;
                            let ratio = clip.scale.at(self.frame as f32).max(0.01) / 100.0;
                            let x = ((px - clip.x.at(self.frame as f32)) / ratio + clip.width / 2.0) as i64;
                            let y = ((py - clip.y.at(self.frame as f32)) / ratio + clip.height / 2.0) as i64;
                            self.picking_track = false;
                            self.start_tracking(index, (x, y));
                        }
                    }
                } else if self.drawing_mask && picture.clicked() {
                    if let (Some(position), Some(clip)) = (picture.interact_pointer_pos(), self.selected.and_then(|index| self.project.clips.get_mut(index))) {
                        let u = (position.x - picture.rect.left()) / picture.rect.width() * self.project.width as f32;
                        let v = (position.y - picture.rect.top()) / picture.rect.height() * self.project.height as f32;
                        let scale = clip.scale.at(self.frame as f32).max(0.01) / 100.0;
                        let (half_w, half_h) = (clip.width * scale / 2.0, clip.height * scale / 2.0);
                        let (x, y) = (clip.x.at(self.frame as f32), clip.y.at(self.frame as f32));
                        clip.mask.points.push([(u - x + half_w) / (half_w * 2.0).max(1.0), (v - y + half_h) / (half_h * 2.0).max(1.0)]);
                    }
                }
            }
        });
        let accent = self.accent();
        let end = self.project.end_frame().max(1);
        let mut position = self.frame as f32;
        ui.spacing_mut().slider_width = ui.available_width() - 8.0;
        if ui.add(egui::Slider::new(&mut position, 0.0..=(end as f32)).show_value(false)).changed() {
            self.frame = (position as u32).min(self.project.duration - 1);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(timecode(self.frame, fps)).color(accent).monospace().size(15.0));
            ui.add_space((ui.available_width() / 2.0 - 110.0).max(0.0));
            if ui.button("|<").on_hover_text("Previous edit (Up)").clicked() {
                self.jump_edit(false);
            }
            if ui.button("<").on_hover_text("Back one frame (Left)").clicked() {
                self.frame = self.frame.saturating_sub(1);
            }
            if ui.add(egui::Button::new(if self.playing { "Pause" } else { "Play" }).min_size(Vec2::new(64.0, 0.0))).on_hover_text("Space").clicked() {
                self.playing = !self.playing;
            }
            if ui.button(">").on_hover_text("Forward one frame (Right)").clicked() {
                self.frame = (self.frame + 1).min(self.project.duration - 1);
            }
            if ui.button(">|").on_hover_text("Next edit (Down)").clicked() {
                self.jump_edit(true);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(timecode(end, fps)).monospace());
            });
        });
    }

    fn tools(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.set_width(58.0);
            for (label, tool, tip) in [("Select", Tool::Select, "Selection (V)"), ("Razor", Tool::Razor, "Razor (C)")] {
                if ui.selectable_label(self.tool == tool, label).on_hover_text(tip).clicked() {
                    self.tool = tool;
                }
            }
            if ui.button("Text").on_hover_text("New text (T)").clicked() {
                self.add_text();
            }
            if ui.button("Marker").on_hover_text("Marker (M)").clicked() {
                self.toggle_marker();
            }
            ui.separator();
            ui.label(RichText::new("Zoom").small().weak());
            ui.spacing_mut().slider_width = 50.0;
            ui.add(egui::Slider::new(&mut self.zoom, 0.2..=60.0).logarithmic(true).show_value(false));
        });
    }

    fn timeline(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 4.0_f32, Color32::from_gray(self.settings.panel_gray.saturating_sub(8)));
        let accent = self.accent();
        let zoom = self.zoom;
        let lane = rect.left() + HEADER_WIDTH;
        if response.hovered() {
            let (delta, modifiers, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.modifiers, i.pointer.hover_pos()));
            if modifiers.command && delta.y != 0.0 {
                let anchor = pointer.map_or(0.0, |p| (p.x - lane) / self.zoom + self.scroll);
                self.zoom = (self.zoom * (1.0 + delta.y * 0.003)).clamp(0.2, 60.0);
                if let Some(p) = pointer {
                    self.scroll = (anchor - (p.x - lane) / self.zoom).max(0.0);
                }
            } else {
                self.scroll = (self.scroll - (delta.y + delta.x) / self.zoom).max(0.0);
            }
        }
        let zoom = if zoom != self.zoom { self.zoom } else { zoom };
        let scroll = self.scroll;
        let to_x = |frame: f32| lane + (frame - scroll) * zoom;
        let content = painter.with_clip_rect(Rect::from_min_max(Pos2::new(lane, rect.top()), rect.max));
        let step = [1u32, 5, 10, 30, 60, 150, 300, 600, 1800, 3600, 18000].into_iter().find(|s| *s as f32 * zoom >= 80.0).unwrap_or(36000);
        let visible_end = scroll + (rect.right() - lane) / zoom;
        let mut tick = (scroll as u32 / step) * step;
        while (tick as f32) < visible_end {
            let x = to_x(tick as f32);
            content.line_segment([Pos2::new(x, rect.top() + 18.0), Pos2::new(x, rect.top() + RULER_HEIGHT)], Stroke::new(1.0_f32, Color32::GRAY));
            content.text(Pos2::new(x + 3.0, rect.top() + 3.0), egui::Align2::LEFT_TOP, timecode(tick, self.project.fps), egui::FontId::proportional(10.0), Color32::LIGHT_GRAY);
            tick += step;
        }
        let order: Vec<usize> = (0..self.project.tracks.len()).rev().collect();
        let row_of = |track: usize| order.iter().position(|t| *t == track).unwrap_or(0);
        let row_rect = |row: usize| Rect::from_min_size(Pos2::new(rect.left(), rect.top() + RULER_HEIGHT + row as f32 * TRACK_HEIGHT), Vec2::new(rect.width(), TRACK_HEIGHT - 1.0));
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        let mut header_click = None;
        for (row, track) in order.iter().enumerate() {
            let line = row_rect(row);
            let data = &self.project.tracks[*track];
            painter.rect_filled(line, 0.0_f32, Color32::from_gray(self.settings.panel_gray.saturating_add(2)));
            let header = Rect::from_min_size(line.min, Vec2::new(HEADER_WIDTH - 2.0, line.height()));
            painter.rect_filled(header, 0.0_f32, Color32::from_gray(self.settings.panel_gray.saturating_add(14)));
            painter.text(line.min + Vec2::new(10.0, 8.0), egui::Align2::LEFT_TOP, &data.name, egui::FontId::proportional(13.0), Color32::WHITE);
            let mute = Rect::from_min_size(line.min + Vec2::new(10.0, 32.0), Vec2::splat(22.0));
            let lock = Rect::from_min_size(line.min + Vec2::new(38.0, 32.0), Vec2::splat(22.0));
            for (button, on, text) in [(mute, data.muted, "M"), (lock, data.locked, "L")] {
                painter.rect_filled(button, 4.0_f32, if on { accent } else { Color32::from_gray(70) });
                painter.text(button.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(11.0), Color32::WHITE);
            }
            if response.clicked() {
                if let Some(p) = pointer {
                    if mute.contains(p) {
                        header_click = Some((*track, true));
                    } else if lock.contains(p) {
                        header_click = Some((*track, false));
                    }
                }
            }
        }
        if let Some((track, is_mute)) = header_click {
            let data = &mut self.project.tracks[track];
            if is_mute {
                data.muted = !data.muted;
            } else {
                data.locked = !data.locked;
            }
        }
        let mut hit = None;
        let waves = self.waves.lock().ok();
        for (index, clip) in self.project.clips.iter().enumerate() {
            let row = row_rect(row_of(clip.track));
            let left = to_x(clip.start as f32);
            let right = to_x(clip.end as f32);
            if right <= lane || left >= rect.right() {
                continue;
            }
            let block = Rect::from_min_max(Pos2::new(left, row.top() + 2.0), Pos2::new(right, row.bottom() - 2.0));
            let track = &self.project.tracks[clip.track];
            let fill = match track.kind {
                TrackKind::Video if clip.text.is_some() => Color32::from_rgb(120, 70, 150),
                TrackKind::Video => Color32::from_rgb(30, 70, 125),
                TrackKind::Audio => Color32::from_rgb(20, 110, 60),
            };
            let selected = self.selected == Some(index);
            let glow = ui.ctx().animate_bool_with_time(egui::Id::new(("clip-glow", index)), selected, 0.12);
            let hover = pointer.is_some_and(|p| block.contains(p));
            let lift = ui.ctx().animate_bool_with_time(egui::Id::new(("clip-hover", index)), hover, 0.1);
            content.rect_filled(block, 4.0_f32, fill.gamma_multiply(0.8 + 0.15 * lift + glow * 0.25));
            content.rect_stroke(block, 4.0_f32, Stroke::new(1.0 + glow, Color32::from_gray(90).lerp_to_gamma(Color32::WHITE, glow)), egui::StrokeKind::Inside);
            if let (Some(path), Some(waves)) = (&clip.media, &waves) {
                if let Some(Some(peaks)) = waves.get(path) {
                    let middle = block.center().y + 8.0;
                    let width = block.width().max(1.0);
                    let color = Color32::from_white_alpha(70);
                    let mut x = block.left().max(lane);
                    while x < block.right().min(rect.right()) {
                        let position = ((x - block.left()) / width * peaks.len() as f32) as usize;
                        let height = peaks.get(position).copied().unwrap_or(0.0).min(1.0) * 16.0;
                        content.line_segment([Pos2::new(x, middle - height), Pos2::new(x, middle + height)], Stroke::new(1.0_f32, color));
                        x += 2.0;
                    }
                }
            }
            if clip.fade_in > 0 {
                let x = to_x((clip.start + clip.fade_in) as f32);
                content.line_segment([block.left_bottom(), Pos2::new(x, block.top())], Stroke::new(1.0_f32, Color32::WHITE));
            }
            if clip.fade_out > 0 {
                let x = to_x((clip.end - clip.fade_out.min(clip.end - clip.start)) as f32);
                content.line_segment([Pos2::new(x, block.top()), block.right_bottom()], Stroke::new(1.0_f32, Color32::WHITE));
            }
            content.with_clip_rect(block.intersect(content.clip_rect())).text(block.min + Vec2::new(6.0, 4.0), egui::Align2::LEFT_TOP, &clip.name, egui::FontId::proportional(11.0), Color32::WHITE);
            if let Some(p) = pointer.filter(|p| block.contains(*p) && p.x >= lane) {
                let mode = if p.x - block.left() < 7.0 {
                    Drag::TrimStart
                } else if block.right() - p.x < 7.0 {
                    Drag::TrimEnd
                } else {
                    Drag::Move { grab: (p.x - lane) / zoom + scroll - clip.start as f32 }
                };
                hit = Some((index, mode));
            }
        }
        drop(waves);
        if let Some((_, mode)) = hit {
            if self.tool == Tool::Razor {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            } else if matches!(mode, Drag::TrimStart | Drag::TrimEnd) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
        if let Some(pos) = pointer {
            let time = ((pos.x - lane) / zoom + scroll).clamp(0.0, self.project.duration as f32 - 1.0);
            let in_ruler = pos.y < rect.top() + RULER_HEIGHT;
            if (response.drag_started() || response.clicked()) && pos.x >= lane {
                if in_ruler {
                    self.frame = time as u32;
                } else if let Some((index, mode)) = hit {
                    let locked = self.project.tracks[self.project.clips[index].track].locked;
                    if self.tool == Tool::Razor && !locked {
                        self.project.split(index, time as u32);
                    } else {
                        self.selected = Some(index);
                        self.drag = (response.drag_started() && !locked).then_some((index, mode));
                    }
                } else if response.clicked() {
                    self.selected = None;
                }
            }
            if response.dragged() {
                if let Some((index, mode)) = self.drag {
                    let row = ((pos.y - rect.top() - RULER_HEIGHT) / TRACK_HEIGHT).max(0.0) as usize;
                    let target = order.get(row).copied();
                    let reach = if self.settings.snapping { (8.0 / zoom) as u32 + 1 } else { 0 };
                    let frames = self.project.snap(time as u32, Some(index), self.frame, reach);
                    let snap_start = self.project.clone();
                    if let Some(clip) = self.project.clips.get_mut(index) {
                        match mode {
                            Drag::Move { grab } => {
                                let length = clip.end - clip.start;
                                let wanted = (time - grab).max(0.0) as u32;
                                let start = snap_start.snap(wanted, Some(index), self.frame, reach);
                                clip.start = start;
                                clip.end = start + length;
                                if let Some(target) = target {
                                    let tracks = &snap_start.tracks;
                                    if tracks[target].kind == tracks[clip.track].kind && !tracks[target].locked {
                                        clip.track = target;
                                    }
                                }
                            }
                            Drag::TrimStart => {
                                let start = frames.min(clip.end - 1);
                                clip.offset = (clip.offset as i64 + start as i64 - clip.start as i64).max(0) as u32;
                                clip.start = start;
                            }
                            Drag::TrimEnd => clip.end = frames.max(clip.start + 1),
                        }
                    }
                } else if in_ruler {
                    self.frame = time as u32;
                }
            }
        }
        if response.drag_stopped() {
            self.drag = None;
        }
        for marker in &self.project.markers {
            let x = to_x(*marker as f32);
            content.line_segment([Pos2::new(x, rect.top() + RULER_HEIGHT - 8.0), Pos2::new(x, rect.top() + RULER_HEIGHT)], Stroke::new(3.0_f32, Color32::from_rgb(90, 200, 120)));
        }
        let head = to_x(self.frame as f32);
        content.line_segment([Pos2::new(head, rect.top()), Pos2::new(head, rect.bottom())], Stroke::new(1.5_f32, accent));
        content.circle_filled(Pos2::new(head, rect.top() + 6.0), 5.0, accent);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.input(ctx);
        self.tick(ctx);
        self.sync_audio(ctx);
        self.autosave();
        self.finish_tracking();
        self.menu(ctx);
        self.windows(ctx);
        egui::TopBottomPanel::bottom("timeline").resizable(true).default_height(330.0).min_height(120.0).show_animated(ctx, self.show_timeline, |ui| {
            ui.horizontal_top(|ui| {
                self.tools(ui);
                let meter = Vec2::new(16.0, ui.available_height());
                let timeline = Vec2::new((ui.available_width() - meter.x - 8.0).max(50.0), ui.available_height());
                ui.allocate_ui(timeline, |ui| self.timeline(ui));
                let (bar, painter) = ui.allocate_painter(meter, Sense::hover());
                painter.rect_filled(bar.rect, 3.0_f32, Color32::from_gray(18));
                let level = Rect::from_min_max(Pos2::new(bar.rect.left(), bar.rect.bottom() - bar.rect.height() * self.meter), bar.rect.max);
                painter.rect_filled(level, 3.0_f32, Color32::from_rgb(70, 210, 110));
            });
        });
        egui::SidePanel::left("project").resizable(true).default_width(290.0).show_animated(ctx, self.show_project, |ui| self.left_panel(ui));
        egui::SidePanel::right("controls").resizable(true).default_width(330.0).show_animated(ctx, self.show_controls, |ui| self.controls_panel(ui));
        egui::CentralPanel::default().show(ctx, |ui| self.monitor(ui));
        if !ctx.input(|i| i.pointer.any_down()) && self.project != self.committed {
            let before = std::mem::replace(&mut self.committed, self.project.clone());
            self.history.record(before);
        }
    }
}
