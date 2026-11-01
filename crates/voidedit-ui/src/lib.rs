use egui_phosphor::regular as ph;
mod home;
mod widgets;
mod settings;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use settings::Settings;
use voidedit_core::{AnimKind, Animation, Curve, Trigger, timecode, Clip, Ease, Effect, History, MaskShape, Param, Project, Shape, TrackKind, BLENDS, EFFECT_NAMES, LOOKS, TRANSITIONS};
use voidedit_export::{audio_clips, export_with, ExportFormat, ExportOptions, Progress};
use voidedit_media::{decode_now, mono_samples, track_point, waveform, Audio, FrameWant, Frames, Library};
use voidedit_render::{render, Frame};

const SNAP_PIXELS: f32 = 8.0;
const HIT_CAPTION: usize = 2;
const MAX_TEXTURE_SIDE: u32 = 2048;
const HEADER_WIDTH: f32 = 240.0;
const RULER_HEIGHT: f32 = 44.0;
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
    Text,
    Effects,
    Transitions,
    Captions,
    Filters,
}

#[derive(Clone, Copy)]
enum Drag {
    Move { grab: f32 },
    TrimStart,
    TrimEnd,
}

#[derive(Clone, Copy)]
enum Gizmo {
    Move,
    Scale { distance: f32, scale: f32 },
    MaskPoint(usize),
}

#[derive(Default)]
struct CaptionJob {
    message: String,
    segments: Option<Result<Vec<voidedit_speech::Segment>, String>>,
    start: f64,
    offset: f64,
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

pub fn wants_backdrop() -> bool {
    Settings::load().backdrop != settings::Backdrop::None
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
    also_selected: Vec<usize>,
    clipboard: Option<Clip>,
    tool: Tool,
    left: LeftTab,
    dragging_media: Option<String>,
    timeline_rect: Option<(Rect, Vec<usize>)>,
    scroll_y: f32,
    snap_line: Option<u32>,
    menu_clip: Option<usize>,
    menu_opened: Instant,
    tab_changed: Instant,
    show_media: bool,
    show_toolbox: bool,
    show_strip: bool,
    show_inspector: bool,
    bin: usize,
    page: u8,
    page_changed: Instant,
    applied_round: Option<bool>,
    wallpaper_light: f32,
    show_home: bool,
    projects: home::Library,
    home: home::HomeState,
    show_studio: bool,
    studio_tab: usize,
    studio_search: String,
    studio_master: bool,
    sfx: Vec<voidedit_audio::SfxInfo>,
    inspector_tab: usize,
    last_active: Instant,
    trimmed: bool,
    applied_backdrop: Option<settings::Backdrop>,
    wallpaper: Option<(String, egui::TextureHandle)>,
    theme_name: String,
    system_accent: Option<[u8; 3]>,
    drawing_mask: bool,
    captioning: Option<Arc<Mutex<CaptionJob>>>,
    gizmo: Option<Gizmo>,
    gizmo_raw: (f32, f32),
    thumbs: Arc<Mutex<HashMap<String, egui::ColorImage>>>,
    thumb_textures: HashMap<String, egui::TextureHandle>,
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
    job: Option<ExportJob>,
    context: egui::Context,
}

impl App {
    pub fn new(ctx: &egui::Context, update_status: Arc<Mutex<String>>) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        #[cfg(windows)]
        if let Some(font) = std::env::var_os("WINDIR").and_then(|folder| std::fs::read(std::path::Path::new(&folder).join("Fonts").join("segoeui.ttf")).ok()) {
            fonts.font_data.insert("system".to_string(), egui::FontData::from_owned(font).into());
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.insert(0, "system".to_string());
            }
        }
        ctx.set_fonts(fonts);
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
            also_selected: Vec::new(),
            clipboard: None,
            tool: Tool::Select,
            left: LeftTab::Effects,
            dragging_media: None,
            timeline_rect: None,
            scroll_y: 0.0,
            snap_line: None,
            menu_clip: None,
            menu_opened: Instant::now(),
            system_accent: settings::system_accent(),
            drawing_mask: false,
            captioning: None,
            gizmo: None,
            gizmo_raw: (0.0, 0.0),
            thumbs: Arc::default(),
            thumb_textures: HashMap::new(),
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
            tab_changed: Instant::now(),
            show_media: true,
            show_toolbox: true,
            show_strip: true,
            show_inspector: true,
            bin: 0,
            page: 255,
            page_changed: Instant::now(),
            applied_round: None,
            wallpaper_light: 0.5,
            show_home: true,
            projects: home::Library::load(),
            home: home::HomeState::default(),
            show_studio: false,
            studio_tab: 0,
            studio_search: String::new(),
            studio_master: false,
            sfx: Vec::new(),
            inspector_tab: 0,
            last_active: Instant::now(),
            trimmed: false,
            applied_backdrop: None,
            wallpaper: None,
            theme_name: String::new(),
            show_shortcuts: false,
            show_export: false,
            options: ExportOptions::default(),
            job: None,
            context: ctx.clone(),
        };
        app.apply_style();
        app
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
        let [r, g, b] = self.system_accent.filter(|_| self.settings.system_accent).unwrap_or(self.settings.accent);
        Color32::from_rgb(r, g, b)
    }

    pub(crate) fn card_alpha(&self) -> u8 {
        let manual = self.settings.theme.card_opacity;
        if !self.settings.auto_opacity {
            return manual;
        }
        if self.wallpaper.is_some() {
            return (178.0 + self.wallpaper_light * 62.0 * (1.0 - self.settings.wallpaper_dim)) as u8;
        }
        if self.settings.backdrop != settings::Backdrop::None {
            return 150;
        }
        255
    }

    pub(crate) fn glass(&self, amount: f32) -> Color32 {
        let shade = self.shade(amount);
        Color32::from_rgba_unmultiplied(shade.r(), shade.g(), shade.b(), self.card_alpha())
    }

    fn restart(&mut self, ctx: &egui::Context) {
        self.settings.save();
        if self.path.is_some() && !self.show_home {
            self.save(false);
        }
        if let Ok(executable) = std::env::current_exe() {
            let mut command = std::process::Command::new(executable);
            if let Some(path) = self.path.as_ref().filter(|_| !self.show_home) {
                command.arg(path);
            }
            if command.spawn().is_ok() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn page_transition(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        const SECONDS: f32 = 0.32;
        const TRAVEL: f32 = 26.0;
        let page = if self.show_home { 0 } else if self.show_studio { 1 } else { 2 };
        if page != self.page {
            self.page = page;
            self.page_changed = Instant::now();
        }
        let raw = if self.settings.animations { self.page_changed.elapsed().as_secs_f32() / SECONDS } else { 1.0 };
        let eased = 1.0 - (1.0 - raw.min(1.0)).powi(3);
        let shift = (1.0 - eased) * TRAVEL;
        ctx.set_transform_layer(egui::LayerId::background(), egui::emath::TSTransform::from_translation(Vec2::new(0.0, shift)));
        if raw < 1.0 {
            root.multiply_opacity(eased);
            ctx.request_repaint();
        }
    }

    fn import_lottie(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Lottie or Bodymovin", &["json", "lottie"]).pick_file() else { return };
        match std::fs::read_to_string(&path).map_err(|error| error.to_string()).and_then(|text| voidedit_core::lottie::import(&text)) {
            Ok(report) => {
                self.project = report.project;
                self.committed = self.project.clone();
                self.history = History::default();
                self.path = None;
                self.selected = None;
                self.frame = 0;
                self.show_home = false;
                self.status = if report.skipped.is_empty() { format!("Imported {} layers", report.imported) } else { format!("Imported {} layers, skipped {}: {}", report.imported, report.skipped.len(), report.skipped.join(", ")) };
            }
            Err(error) => self.status = format!("Could not import: {error}"),
        }
    }

    fn settings_accent(&self) -> Color32 {
        let [r, g, b] = self.settings.accent;
        Color32::from_rgb(r, g, b)
    }

    fn shade(&self, amount: f32) -> Color32 {
        let [r, g, b] = self.settings.theme.card;
        let [x, y, z] = self.settings.theme.background;
        widgets::mix(Color32::from_rgb(r, g, b), Color32::from_rgb(x, y, z), amount)
    }

    fn title_drag(&self, ui: &mut egui::Ui) {
        if cfg!(windows) {
            let title = ui.interact(ui.max_rect(), egui::Id::new("title-drag"), Sense::click_and_drag());
            if title.double_clicked() {
                let maximized = settings::is_maximized();
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            } else if title.drag_started() {
                settings::drag_window(HIT_CAPTION);
            }
        }
    }

    fn window_buttons(&self, ui: &mut egui::Ui) {
        if cfg!(windows) {
            let context = ui.ctx().clone();
            ui.spacing_mut().item_spacing.x = 0.0;
            if widgets::caption(ui, widgets::Caption::Close).clicked() {
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            let maximized = settings::is_maximized();
            if widgets::caption(ui, if maximized { widgets::Caption::Restore } else { widgets::Caption::Maximize }).clicked() {
                context.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            if widgets::caption(ui, widgets::Caption::Minimize).clicked() {
                context.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            ui.spacing_mut().item_spacing.x = 7.0;
            ui.add_space(10.0);
        }
    }

    fn window_edges(&self, ctx: &egui::Context) {
        const EDGE: f32 = 6.0;
        if !cfg!(windows) || settings::is_maximized() {
            return;
        }
        let Some(pointer) = ctx.input(|input| input.pointer.hover_pos()) else { return };
        let screen = ctx.content_rect();
        let (left, right) = (pointer.x - screen.left() < EDGE, screen.right() - pointer.x < EDGE);
        let (top, bottom) = (pointer.y - screen.top() < EDGE, screen.bottom() - pointer.y < EDGE);
        let (area, icon) = match (left, right, top, bottom) {
            (true, _, true, _) => (13, egui::CursorIcon::ResizeNwSe),
            (_, true, true, _) => (14, egui::CursorIcon::ResizeNeSw),
            (true, _, _, true) => (16, egui::CursorIcon::ResizeNeSw),
            (_, true, _, true) => (17, egui::CursorIcon::ResizeNwSe),
            (true, ..) => (10, egui::CursorIcon::ResizeHorizontal),
            (_, true, ..) => (11, egui::CursorIcon::ResizeHorizontal),
            (_, _, true, _) => (12, egui::CursorIcon::ResizeVertical),
            (_, _, _, true) => (15, egui::CursorIcon::ResizeVertical),
            _ => return,
        };
        ctx.set_cursor_icon(icon);
        if ctx.input(|input| input.pointer.primary_pressed()) {
            settings::drag_window(area);
        }
    }

    fn release_idle_memory(&mut self, ctx: &egui::Context) {
        const IDLE_SECONDS: u64 = 2;
        let idle = std::time::Duration::from_secs(IDLE_SECONDS);
        let busy = self.playing || ctx.input(|input| !input.events.is_empty() || input.pointer.any_down() || input.pointer.is_moving());
        if busy {
            self.last_active = Instant::now();
            self.trimmed = false;
            ctx.request_repaint_after(idle);
            return;
        }
        if !self.trimmed && self.last_active.elapsed() >= idle {
            self.trimmed = true;
            settings::trim_memory();
        } else if !self.trimmed {
            ctx.request_repaint_after(idle);
        }
    }

    fn sync_backdrop(&mut self, frame: &impl raw_window_handle::HasWindowHandle) {
        if self.applied_backdrop == Some(self.settings.backdrop) {
            return;
        }
        self.applied_backdrop = Some(self.settings.backdrop);
        #[cfg(windows)]
        {
            use settings::Backdrop;
            let _ = window_vibrancy::clear_mica(frame);
            let _ = window_vibrancy::clear_tabbed(frame);
            let _ = window_vibrancy::clear_acrylic(frame);
            let _ = window_vibrancy::clear_blur(frame);
            settings::extend_frame(self.settings.backdrop != Backdrop::None);
            let result = match self.settings.backdrop {
                Backdrop::None => Ok(()),
                Backdrop::Mica => window_vibrancy::apply_mica(frame, Some(true)),
                Backdrop::Tabbed => window_vibrancy::apply_tabbed(frame, Some(true)),
                Backdrop::Acrylic => window_vibrancy::apply_acrylic(frame, Some((0, 0, 0, 90))),
                Backdrop::Blur => window_vibrancy::apply_blur(frame, Some((0, 0, 0, 90))),
            };
            if result.is_err() {
                self.status = "This backdrop needs a newer version of Windows".to_string();
            }
        }
        #[cfg(not(windows))]
        let _ = frame;
    }

    fn sync_wallpaper(&mut self, ctx: &egui::Context) {
        let wanted = &self.settings.wallpaper;
        if self.wallpaper.as_ref().map_or(wanted.is_empty(), |(path, _)| path == wanted) {
            return;
        }
        self.wallpaper = None;
        if wanted.is_empty() {
            return;
        }
        const MAX_SIDE: u32 = MAX_TEXTURE_SIDE;
        match image::open(wanted) {
            Ok(picture) => {
                let picture = picture.thumbnail(MAX_SIDE, MAX_SIDE).into_rgba8();
                let size = [picture.width() as usize, picture.height() as usize];
                let total: u64 = picture.pixels().step_by(97).map(|pixel| pixel[0] as u64 * 2 + pixel[1] as u64 * 5 + pixel[2] as u64).sum();
                let count = (picture.pixels().len() / 97).max(1) as u64;
                self.wallpaper_light = (total / count) as f32 / (255.0 * 8.0);
                let texture = ctx.load_texture("wallpaper", egui::ColorImage::from_rgba_unmultiplied(size, picture.as_raw()), egui::TextureOptions::LINEAR);
                self.wallpaper = Some((wanted.clone(), texture));
            }
            Err(_) => {
                self.status = "Could not open that wallpaper".to_string();
                self.settings.wallpaper.clear();
            }
        }
    }

    fn paint_background(&self, ctx: &egui::Context) {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let screen = ctx.content_rect().expand(40.0);
        let [r, g, b] = self.settings.theme.background;
        let alpha = if self.settings.backdrop == settings::Backdrop::None { 255 } else { 110 };
        if let Some((_, texture)) = &self.wallpaper {
            let [width, height] = texture.size().map(|side| side as f32);
            let scale = (screen.width() / width).max(screen.height() / height);
            let (u, v) = (screen.width() / (width * scale), screen.height() / (height * scale));
            let uv = Rect::from_min_max(Pos2::new((1.0 - u) / 2.0, (1.0 - v) / 2.0), Pos2::new((1.0 + u) / 2.0, (1.0 + v) / 2.0));
            painter.image(texture.id(), screen, uv, Color32::WHITE);
            painter.rect_filled(screen, 0.0_f32, Color32::from_black_alpha((self.settings.wallpaper_dim * 255.0) as u8));
        } else {
            painter.rect_filled(screen, 0.0_f32, Color32::from_rgba_unmultiplied(r, g, b, alpha));
        }
    }

    fn apply_style(&self) {
        let ctx = &self.context;
        ctx.set_zoom_factor(self.settings.ui_scale.clamp(0.6, 2.0));
        let mut visuals = egui::Visuals::dark();
        let theme = &self.settings.theme;
        let color = |[r, g, b]: [u8; 3]| Color32::from_rgb(r, g, b);
        let (card, text, accent) = (color(theme.card), color(theme.text), self.accent());
        visuals.override_text_color = Some(text);
        visuals.panel_fill = Color32::TRANSPARENT;
        visuals.window_fill = widgets::mix(card, text, 0.03);
        visuals.faint_bg_color = widgets::mix(card, text, 0.04);
        visuals.extreme_bg_color = widgets::mix(card, color(theme.background), 0.5);
        visuals.selection.bg_fill = accent.gamma_multiply(0.7);
        visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
        visuals.hyperlink_color = accent;
        visuals.slider_trailing_fill = true;
        visuals.handle_shape = egui::style::HandleShape::Circle;
        visuals.window_corner_radius = 8.into();
        visuals.menu_corner_radius = 6.into();
        visuals.window_stroke = Stroke::new(1.0_f32, widgets::mix(card, text, 0.14));
        for (state, fill, border) in [
            (&mut visuals.widgets.noninteractive, card, 0.08),
            (&mut visuals.widgets.inactive, widgets::mix(card, text, 0.08), 0.1),
            (&mut visuals.widgets.hovered, widgets::mix(card, text, 0.13), 0.16),
            (&mut visuals.widgets.active, widgets::mix(card, text, 0.06), 0.1),
            (&mut visuals.widgets.open, widgets::mix(card, text, 0.1), 0.12),
        ] {
            state.corner_radius = 4.into();
            state.bg_stroke = Stroke::new(1.0_f32, widgets::mix(card, text, border));
            state.weak_bg_fill = fill;
            state.bg_fill = fill;
            state.expansion = 0.0;
            state.fg_stroke.color = text;
        }
        ctx.set_visuals(visuals);
        ctx.global_style_mut(|style| {
            style.animation_time = if self.settings.animations { 0.14 } else { 0.0 };
            style.spacing.item_spacing = Vec2::new(7.0, 6.0);
            style.spacing.button_padding = Vec2::new(10.0, 5.0);
            style.spacing.slider_rail_height = 4.0;
            style.spacing.interact_size.y = 24.0;
            style.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(14.0));
            style.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(13.0));
            style.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
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
                self.projects.record(&path, &self.project, true);
                self.path = Some(path);
                self.show_home = false;
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
                self.projects.record(&path, &self.project, false);
                self.path = Some(path);
                self.save_thumbnail();
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

    fn generate_captions(&mut self) {
        if self.captioning.is_some() {
            return;
        }
        let Some(clip) = self.selected.and_then(|index| self.project.clips.get(index)) else {
            self.status = "Select a video or audio clip first".into();
            return;
        };
        let Some(path) = clip.media.clone() else {
            self.status = "Select a video or audio clip first".into();
            return;
        };
        let Some(base) = Settings::folder() else { return };
        let fps = self.project.fps.max(1) as f64;
        let job = Arc::new(Mutex::new(CaptionJob { start: clip.start as f64 / fps, offset: clip.offset as f64 / fps, ..CaptionJob::default() }));
        let (shared, library, repaint) = (job.clone(), self.library.clone(), self.context.clone());
        let language = self.settings.caption_language.clone();
        let spawned = std::thread::Builder::new().name("captions".into()).spawn(move || {
            let say = |text: &str| {
                if let Ok(mut state) = shared.lock() {
                    state.message = text.to_string();
                }
                repaint.request_repaint();
            };
            let folder = voidedit_speech::model_folder(&base);
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                voidedit_speech::ensure_model(&folder, say)?;
                say("Reading the audio");
                let samples = mono_samples(&library, &path, voidedit_speech::SAMPLE_RATE as u32).ok_or("This clip has no audio")?;
                say("Listening for speech");
                voidedit_speech::transcribe(&folder, &samples, &language, |amount| say(&format!("Listening for speech: {:.0}%", amount * 100.0)))
            }))
            .unwrap_or_else(|_| Err("Caption generation stopped unexpectedly".into()));
            if let Ok(mut state) = shared.lock() {
                state.segments = Some(outcome);
            }
            repaint.request_repaint();
        });
        if spawned.is_ok() {
            self.captioning = Some(job);
        }
    }

    fn finish_captions(&mut self) {
        let Some(job) = &self.captioning else { return };
        let Ok(mut state) = job.lock() else { return };
        self.status = state.message.clone();
        let Some(result) = state.segments.take() else { return };
        let (start, offset) = (state.start, state.offset);
        drop(state);
        self.captioning = None;
        match result {
            Ok(segments) => {
                let mut count = 0;
                for segment in segments.iter().filter(|segment| segment.end > offset) {
                    self.project.add_spoken_caption(start + (segment.start - offset).max(0.0), start + segment.end - offset, &segment.text);
                    count += 1;
                }
                self.status = format!("Added captions from {count} spoken parts");
            }
            Err(error) => self.status = format!("Could not make captions: {error}"),
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
        let thumbs = self.thumbs.clone();
        let repaint = self.context.clone();
        let _ = std::thread::Builder::new().name("waveform".into()).spawn(move || {
            let peaks = waveform(&library, &file, WAVEFORM_POINTS);
            if let Ok(mut waves) = waves.lock() {
                waves.insert(file.clone(), peaks);
            }
            let want = FrameWant { path: file.clone(), seconds: 0.5, scale: 0.1 };
            if let Some(image) = decode_now(&library, &want).filter(|image| image.width > 1) {
                let height = 42usize;
                let width = (image.width * height / image.height.max(1)).max(1);
                let mut pixels = Vec::with_capacity(width * height * 4);
                for y in 0..height {
                    for x in 0..width {
                        let start = (y * image.height / height * image.width + x * image.width / width) * 4;
                        pixels.extend_from_slice(&image.pixels[start..start + 4]);
                    }
                }
                if let Ok(mut thumbs) = thumbs.lock() {
                    thumbs.insert(file, egui::ColorImage::from_rgba_unmultiplied([width, height], &pixels));
                }
            }
            repaint.request_repaint();
        });
    }

    fn finish_media_drag(&mut self, ctx: &egui::Context) {
        let Some(file) = self.dragging_media.clone() else { return };
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        if let Some(position) = pointer {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("media-drag")));
            let name = std::path::Path::new(&file).file_name().map_or(file.clone(), |n| n.to_string_lossy().into_owned());
            painter.text(position + Vec2::new(12.0, 8.0), egui::Align2::LEFT_TOP, name, egui::FontId::proportional(13.0), Color32::WHITE);
        }
        if ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        self.dragging_media = None;
        let (Some(position), Some((rect, order))) = (pointer, self.timeline_rect.clone()) else { return };
        if !rect.contains(position) || position.x < rect.left() + HEADER_WIDTH {
            return;
        }
        let frame = (((position.x - rect.left() - HEADER_WIDTH) / self.zoom + self.scroll).max(0.0)) as u32;
        let row = ((position.y - rect.top() - RULER_HEIGHT + self.scroll_y) / TRACK_HEIGHT).max(0.0) as usize;
        let previous = self.frame;
        self.frame = frame;
        self.insert_media(&file);
        self.frame = previous;
        if let (Some(track), Some(clip)) = (order.get(row).copied(), self.selected.and_then(|index| self.project.clips.get_mut(index))) {
            if self.project.tracks.get(track).map(|data| data.kind) == self.project.tracks.get(clip.track).map(|data| data.kind) {
                clip.track = track;
            }
        }
    }

    fn insert_media(&mut self, file: &str) {
        match self.library.details(file) {
            Some(details) => {
                let size = details.has_video.then_some((details.width, details.height));
                if let Some((width, height)) = size.filter(|size| size.0 > 0 && size.1 > 0 && !self.project.clips.iter().any(|clip| clip.media.is_some())) {
                    self.project.width = (width & !1).max(2);
                    self.project.height = (height & !1).max(2);
                }
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
        let mut indices = std::mem::take(&mut self.also_selected);
        indices.extend(self.selected.take());
        indices.sort_unstable();
        indices.dedup();
        for index in indices.into_iter().rev() {
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
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let chord = |settings: &Settings, action: &str| settings.key(action).is_some_and(|key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, key)));
        let shift_command = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, key));
        let shift = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, key));
        let plain = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
        if shift_command(self.settings.key("Undo (Ctrl)").unwrap_or(egui::Key::Z)) {
            self.redo();
        } else if chord(&self.settings, "Undo (Ctrl)") {
            self.undo();
        }
        if shift_command(self.settings.key("Save (Ctrl)").unwrap_or(egui::Key::S)) {
            self.save(true);
        } else if chord(&self.settings, "Save (Ctrl)") {
            self.save(false);
        }
        if chord(&self.settings, "Split (Ctrl)") {
            self.split_at_playhead();
        }
        if chord(&self.settings, "Copy (Ctrl)") {
            self.copy();
        }
        if chord(&self.settings, "Paste (Ctrl)") {
            self.paste();
        }
        if chord(&self.settings, "Duplicate (Ctrl)") {
            self.copy();
            self.paste();
        }
        if chord(&self.settings, "Import (Ctrl)") {
            self.import();
        }
        if chord(&self.settings, "Export (Ctrl)") {
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
        if bound(&self.settings, "Previous frame") {
            self.frame = self.frame.saturating_sub(1);
        }
        if bound(&self.settings, "Next frame") {
            self.frame = (self.frame + 1).min(self.project.duration - 1);
        }
        if bound(&self.settings, "Previous edit") {
            self.jump_edit(false);
        }
        if bound(&self.settings, "Next edit") {
            self.jump_edit(true);
        }
        if bound(&self.settings, "Go to start") {
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
            self.audio.set_clips(audio_clips(&self.project), self.project.master_fx.clone());
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

    fn menu(&mut self, root: &mut egui::Ui) {
        let bar = egui::Frame::new().fill(self.glass(0.5)).inner_margin(egui::Margin { left: 8, right: 0, top: 0, bottom: 0 });
        egui::Panel::top("menu").show_separator_line(false).frame(bar).show_inside(root, |ui| {
            self.title_drag(ui);
            egui::MenuBar::new().ui(ui, |ui| {
                                if widgets::head(ui, ph::HOUSE, "Home", false).on_hover_text("Save and go to your projects").clicked() {
                    self.go_home();
                }
                if widgets::head(ui, ph::IMAGES, "Media Pool", self.show_media).clicked() {
                    self.show_media = !self.show_media;
                }
                if widgets::head(ui, ph::SPARKLE, "Effects", self.show_toolbox).clicked() {
                    self.show_toolbox = !self.show_toolbox;
                }
                if widgets::head(ui, ph::WAVEFORM, "Sound Studio", self.show_studio).clicked() {
                    self.show_studio = !self.show_studio;
                }
                if widgets::head(ui, ph::MUSIC_NOTES, "Sound Library", self.show_studio && self.studio_tab == 2).clicked() {
                    let open = self.show_studio && self.studio_tab == 2;
                    self.show_studio = !open;
                    self.studio_tab = 2;
                }
                ui.menu_button(format!("{}  Menu", ph::LIST), |ui| {
                ui.menu_button("File", |ui| {
                    let items: [(&str, fn(&mut App)); 7] = [
                        ("New", App::new_project),
                        ("Open...", App::open),
                        ("Save", |app| app.save(false)),
                        ("Save As...", |app| app.save(true)),
                        ("Import Media...", App::import),
                        ("Import Lottie...", App::import_lottie),
                        ("Export...", |app| app.show_export = true),
                    ];
                    for (label, action) in items {
                        if ui.button(label).clicked() {
                            ui.close();
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
                            ui.close();
                            action(self);
                        }
                    }
                });
                ui.menu_button("Clip", |ui| {
                    if ui.button("New Text").clicked() {
                        self.add_text();
                        ui.close();
                    }
                    for (label, shape) in [("New Solid", Shape::Solid), ("New Rectangle", Shape::Rectangle), ("New Ellipse", Shape::Ellipse)] {
                        if ui.button(label).clicked() {
                            self.add_clip(shape);
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Compose", |ui| {
                    if ui.add_enabled(self.selected.is_some(), egui::Button::new("Nest Selected Clip")).clicked() {
                        if let Some(index) = self.selected.take() {
                            self.project.nest(index);
                            self.selected = Some(self.project.clips.len() - 1);
                        }
                        ui.close();
                    }
                });
                ui.menu_button("Captions", |ui| {
                    let current = voidedit_speech::LANGUAGES.iter().find(|(_, code)| *code == self.settings.caption_language).map_or("Detect automatically", |(name, _)| *name);
                    ui.label(RichText::new("Spoken language").small().weak());
                egui::ComboBox::from_id_salt("caption-language").width(ui.available_width() - 8.0).selected_text(current).height(320.0).show_ui(ui, |ui| {
                        if ui.selectable_label(self.settings.caption_language == voidedit_speech::AUTO_DETECT, "Detect automatically").clicked() {
                            self.settings.caption_language = voidedit_speech::AUTO_DETECT.to_string();
                            self.settings.save();
                        }
                        for (name, code) in voidedit_speech::LANGUAGES {
                            if ui.selectable_label(self.settings.caption_language == code, name).clicked() {
                                self.settings.caption_language = code.to_string();
                                self.settings.save();
                            }
                        }
                    });
                    if ui.button("Generate from Selected Clip").on_hover_text("Downloads a 144 MB speech model the first time").clicked() {
                        ui.close();
                        self.generate_captions();
                    }
                    if ui.button("Import SRT...").clicked() {
                        ui.close();
                        self.import_captions();
                    }
                });
                ui.menu_button("Sequence", |ui| {
                    if ui.button("Add Video Track").clicked() {
                        self.project.add_track(TrackKind::Video);
                        ui.close();
                    }
                    if ui.button("Add Audio Track").clicked() {
                        self.project.add_track(TrackKind::Audio);
                        ui.close();
                    }
                    if ui.button("Add or Remove Marker").clicked() {
                        self.toggle_marker();
                        ui.close();
                    }
                    ui.separator();
                    for (label, width, height) in [("16:9 Landscape 1080p", 1920, 1080), ("9:16 Vertical 1080p", 1080, 1920), ("1:1 Square", 1080, 1080), ("4:5 Portrait", 1080, 1350), ("4K Landscape", 3840, 2160)] {
                        if ui.button(label).clicked() {
                            self.project.set_aspect(width, height);
                            ui.close();
                        }
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
                    ui.checkbox(&mut self.show_mixer, "Audio mixer");
                    if ui.button("Fit timeline").clicked() {
                        self.scroll = 0.0;
                        self.zoom = (1100.0 / self.project.end_frame().max(30) as f32).clamp(0.2, 60.0);
                        ui.close();
                    }
                    if ui.button("Keyboard shortcuts").clicked() {
                        self.show_shortcuts = true;
                        ui.close();
                    }
                });
                if ui.button(format!("{}  Settings", ph::GEAR)).clicked() {
                    self.show_settings = true;
                    ui.close();
                }
                });
                let name = self.path.as_ref().and_then(|p| p.file_name()).map_or("Untitled".to_string(), |n| n.to_string_lossy().into_owned());
                let changed = self.project != self.committed;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.window_buttons(ui);
                    if widgets::head(ui, ph::SLIDERS_HORIZONTAL, "Inspector", self.show_inspector).clicked() {
                        self.show_inspector = !self.show_inspector;
                    }
                    if widgets::head(ui, ph::FILE_TEXT, "Metadata", self.show_inspector && self.inspector_tab == 5).clicked() {
                        self.show_inspector = true;
                        self.inspector_tab = 5;
                    }
                    if widgets::head(ui, ph::FADERS, "Mixer", self.show_strip).clicked() {
                        self.show_strip = !self.show_strip;
                    }
                    if widgets::head(ui, ph::EXPORT, "Quick Export", false).clicked() {
                        self.show_export = true;
                    }
                    ui.painter().text(ui.ctx().content_rect().center_top() + Vec2::new(0.0, 18.0), egui::Align2::CENTER_CENTER, if changed { format!("{name}   Edited") } else { name }, egui::FontId::proportional(14.0), Color32::WHITE);
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
        let mut relaunch = false;
        egui::Window::new("Settings").open(&mut open).resizable(false).collapsible(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).default_width(460.0).show(ctx, |ui| {
            ui.set_width(460.0);
            let accent = self.accent();
            ui.spacing_mut().slider_width = 170.0;
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.72).show(ui, |ui| {
                widgets::section(ui, "Theme", |ui| {
                    widgets::setting(ui, "Theme", |ui| {
                        egui::ComboBox::from_id_salt("theme-pick").width(170.0).selected_text(self.settings.theme.name.clone()).show_ui(ui, |ui| {
                            for theme in settings::Theme::presets().into_iter().chain(settings::Theme::saved()) {
                                let active = theme.name == self.settings.theme.name;
                                if ui.selectable_label(active, &theme.name).clicked() {
                                    self.settings.theme = theme;
                                    changed = true;
                                }
                            }
                        });
                    });
                    let theme = &mut self.settings.theme;
                    for (label, value) in [("Background", &mut theme.background), ("Panels", &mut theme.card), ("Text", &mut theme.text)] {
                        widgets::setting(ui, label, |ui| changed |= ui.color_edit_button_srgb(value).changed());
                    }
                    if self.system_accent.is_some() {
                        widgets::setting(ui, "Use the Windows accent color", |ui| changed |= widgets::toggle(ui, &mut self.settings.system_accent, accent));
                    }
                    if !(self.system_accent.is_some() && self.settings.system_accent) {
                        widgets::setting(ui, "Accent", |ui| changed |= ui.color_edit_button_srgb(&mut self.settings.accent).changed());
                    }
                    widgets::setting(ui, "Panel corners", |ui| changed |= ui.add(egui::Slider::new(&mut self.settings.theme.radius, 0.0..=16.0).show_value(false)).changed());
                    widgets::setting(ui, "Save as", |ui| {
                        if ui.button("Save").clicked() && !self.theme_name.trim().is_empty() {
                            self.settings.theme.name = self.theme_name.trim().to_string();
                            self.status = if self.settings.theme.store() { "Theme saved".to_string() } else { "Could not save the theme".to_string() };
                            changed = true;
                        }
                        ui.add(egui::TextEdit::singleline(&mut self.theme_name).hint_text("Theme name").desired_width(170.0));
                    });
                    widgets::setting(ui, "Theme file", |ui| {
                        if ui.button("Export").clicked() {
                            if let Some(path) = rfd::FileDialog::new().add_filter("Theme", &["json"]).set_file_name(format!("{}.json", self.settings.theme.name)).save_file() {
                                self.settings.theme.write(&path);
                            }
                        }
                        if ui.button("Import").clicked() {
                            if let Some(theme) = rfd::FileDialog::new().add_filter("Theme", &["json"]).pick_file().and_then(|path| settings::Theme::read(&path)) {
                                theme.store();
                                self.settings.theme = theme;
                                changed = true;
                            }
                        }
                    });
                });
                widgets::section(ui, "Window", |ui| {
                    if cfg!(windows) {
                        widgets::setting(ui, "Rounded corners", |ui| changed |= widgets::toggle(ui, &mut self.settings.rounded, accent));
                        widgets::setting(ui, "Backdrop", |ui| {
                            let current = settings::Backdrop::ALL.iter().find(|item| item.0 == self.settings.backdrop).map_or("None", |item| item.1);
                            egui::ComboBox::from_id_salt("backdrop-pick").width(170.0).selected_text(current).show_ui(ui, |ui| {
                                for (backdrop, label) in settings::Backdrop::ALL {
                                    if ui.selectable_label(self.settings.backdrop == backdrop, label).clicked() && self.settings.backdrop != backdrop {
                                        relaunch = (self.settings.backdrop == settings::Backdrop::None) != (backdrop == settings::Backdrop::None);
                                        self.settings.backdrop = backdrop;
                                        changed = true;
                                    }
                                }
                            });
                        });
                        ui.label(RichText::new("Turning a backdrop on or off restarts the app and uses more memory").small().weak());
                    }
                    widgets::setting(ui, "Wallpaper", |ui| {
                        if !self.settings.wallpaper.is_empty() && ui.button("Remove").clicked() {
                            self.settings.wallpaper.clear();
                            changed = true;
                        }
                        if ui.button("Choose image").clicked() {
                            if let Some(path) = rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg", "webp", "bmp"]).pick_file() {
                                self.settings.wallpaper = path.to_string_lossy().into_owned();
                                changed = true;
                            }
                        }
                    });
                    if !self.settings.wallpaper.is_empty() {
                        widgets::setting(ui, "Wallpaper dim", |ui| changed |= ui.add(egui::Slider::new(&mut self.settings.wallpaper_dim, 0.0..=0.9).show_value(false)).changed());
                    }
                    widgets::setting(ui, "Automatic panel opacity", |ui| changed |= widgets::toggle(ui, &mut self.settings.auto_opacity, accent));
                    if !self.settings.auto_opacity {
                        widgets::setting(ui, "Panel opacity", |ui| changed |= ui.add(egui::Slider::new(&mut self.settings.theme.card_opacity, 60..=255).show_value(false)).changed());
                    }
                });
                widgets::section(ui, "General", |ui| {
                    widgets::setting(ui, "Animations", |ui| changed |= widgets::toggle(ui, &mut self.settings.animations, accent));
                    widgets::setting(ui, "Snap clips while dragging", |ui| changed |= widgets::toggle(ui, &mut self.settings.snapping, accent));
                    widgets::setting(ui, "Interface size", |ui| {
                        let response = ui.add(egui::Slider::new(&mut self.settings.ui_scale, 0.6..=2.0).show_value(false));
                        changed |= response.drag_stopped() || (response.changed() && !response.dragged());
                    });
                    widgets::setting(ui, "Preview quality", |ui| changed |= ui.add(egui::Slider::new(&mut self.settings.preview_quality, 0.25..=1.0).show_value(false)).changed());
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Reset everything").clicked() {
                        relaunch = self.settings.backdrop != settings::Backdrop::None;
                        self.settings = Settings::default();
                        changed = true;
                    }
                });
            });
        });
        if relaunch {
            self.restart(ctx);
        }
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
                let factor = if self.options.format == ExportFormat::Mp4 { self.options.upscale.max(1) } else { 1 };
                ui.label(RichText::new(format!("{} x {}", (width as u32 * factor) & !1, (height as u32 * factor) & !1)).weak());
            }
            if self.options.format == ExportFormat::Mp4 {
                ui.horizontal(|ui| {
                    ui.label("Upscale");
                    for factor in 1..=4u32 {
                        ui.selectable_value(&mut self.options.upscale, factor, format!("{factor}x"));
                    }
                });
                if self.options.upscale > 1 {
                    ui.add(egui::Slider::new(&mut self.options.sharpen, 0.0..=1.5).text("Sharpen"));
                }
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

    fn studio(&mut self, ui: &mut egui::Ui) {
        let accent = self.accent();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Sound Studio").strong().size(15.0));
            ui.add_space(12.0);
            for (index, icon, label) in [(0, ph::SLIDERS, "Effects"), (1, ph::CUBE, "3D Sound"), (2, ph::MUSIC_NOTES, "Sound Library")] {
                if widgets::pill(ui, icon, label, self.studio_tab == index, accent).clicked() {
                    self.studio_tab = index;
                }
            }
        });
        ui.separator();
        match self.studio_tab {
            0 => self.studio_effects(ui),
            1 => self.studio_spatial(ui),
            _ => self.studio_library(ui),
        }
    }

    fn studio_effects(&mut self, ui: &mut egui::Ui) {
        let accent = self.accent();
        ui.horizontal(|ui| {
            if widgets::pill(ui, ph::WAVEFORM, "Selected clip", !self.studio_master, accent).clicked() {
                self.studio_master = false;
            }
            if widgets::pill(ui, ph::FADERS, "Master output", self.studio_master, accent).clicked() {
                self.studio_master = true;
            }
        });
        let master = self.studio_master;
        let selected = self.selected.filter(|index| *index < self.project.clips.len());
        if !master && selected.is_none() {
            ui.add_space(20.0);
            ui.label(RichText::new("Select a clip on the timeline, or choose Master output").weak());
            return;
        }
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(190.0);
                ui.set_min_height(height);
                egui::ScrollArea::vertical().id_salt("studio-add").show(ui, |ui| {
                    let mut last = "";
                    for spec in voidedit_audio::SPECS {
                        if spec.category != last {
                            last = spec.category;
                            ui.label(RichText::new(last).small().weak());
                        }
                        if ui.add(egui::Button::new(spec.name).min_size(Vec2::new(176.0, 22.0))).clicked() {
                            let effect = voidedit_audio::Effect::new(spec.kind);
                            match selected.filter(|_| !master) {
                                Some(index) => self.project.clips[index].audio_fx.push(effect),
                                None => self.project.master_fx.push(effect),
                            }
                        }
                    }
                });
            });
            let effects = match selected.filter(|_| !master) {
                Some(index) => &mut self.project.clips[index].audio_fx,
                None => &mut self.project.master_fx,
            };
            ui.vertical(|ui| {
                egui::ScrollArea::vertical().id_salt("studio-chain").show(ui, |ui| {
                    if effects.is_empty() {
                        ui.label(RichText::new("Pick an effect on the left to add it to the chain").weak());
                    }
                    let mut remove = None;
                    let mut raise = None;
                    for (index, effect) in effects.iter_mut().enumerate() {
                        let spec = voidedit_audio::Effect::spec_of(effect.kind);
                        egui::Frame::new().fill(Color32::from_gray(30)).corner_radius(3).inner_margin(8).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut effect.enabled, RichText::new(spec.name).strong());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if widgets::pill(ui, ph::TRASH, "", false, accent).on_hover_text("Remove").clicked() {
                                        remove = Some(index);
                                    }
                                    if index > 0 && widgets::pill(ui, ph::ARROW_UP, "", false, accent).on_hover_text("Move earlier").clicked() {
                                        raise = Some(index);
                                    }
                                });
                            });
                            for (position, (name, min, max, _)) in spec.params.iter().enumerate() {
                                if let Some(value) = effect.values.get_mut(position) {
                                    let logarithmic = name.ends_with("Hz") && *min > 0.0;
                                    ui.add(egui::Slider::new(value, *min..=*max).logarithmic(logarithmic).text(*name));
                                }
                            }
                        });
                        ui.add_space(4.0);
                    }
                    if let Some(index) = remove {
                        effects.remove(index);
                    } else if let Some(index) = raise {
                        effects.swap(index, index - 1);
                    }
                });
            });
        });
    }

    fn studio_spatial(&mut self, ui: &mut egui::Ui) {
        let accent = self.accent();
        let Some(clip) = self.selected.and_then(|index| self.project.clips.get_mut(index)) else {
            ui.add_space(20.0);
            ui.label(RichText::new("Select a clip on the timeline to place its sound in 3D").weak());
            return;
        };
        let spatial = &mut clip.spatial;
        ui.checkbox(&mut spatial.enabled, "Enable 3D sound");
        ui.horizontal_top(|ui| {
            let side = (ui.available_height() - 8.0).min(ui.available_width() * 0.55).clamp(140.0, 420.0);
            let (response, painter) = ui.allocate_painter(Vec2::splat(side), Sense::click_and_drag());
            let rect = response.rect;
            const RANGE: f32 = 20.0;
            painter.rect_filled(rect, 3.0_f32, Color32::from_gray(20));
            for ring in 1..=4 {
                painter.circle_stroke(rect.center(), side * ring as f32 / 8.0, Stroke::new(1.0_f32, Color32::from_gray(50)));
            }
            painter.circle_filled(rect.center(), 7.0, Color32::LIGHT_GRAY);
            painter.text(rect.center_top() + Vec2::new(0.0, 10.0), egui::Align2::CENTER_CENTER, "Front", egui::FontId::proportional(11.0), Color32::GRAY);
            painter.text(rect.center_bottom() - Vec2::new(0.0, 10.0), egui::Align2::CENTER_CENTER, "Behind", egui::FontId::proportional(11.0), Color32::GRAY);
            if let Some(pointer) = response.interact_pointer_pos() {
                spatial.x = (pointer.x - rect.center().x) / (side / 2.0) * RANGE;
                spatial.z = (rect.center().y - pointer.y) / (side / 2.0) * RANGE;
            }
            let dot = rect.center() + Vec2::new(spatial.x / RANGE * side / 2.0, -spatial.z / RANGE * side / 2.0);
            painter.circle_filled(dot.clamp(rect.min, rect.max), 8.0, accent);
            ui.vertical(|ui| {
                ui.add(egui::Slider::new(&mut spatial.x, -RANGE..=RANGE).text("Left and right (m)"));
                ui.add(egui::Slider::new(&mut spatial.z, -RANGE..=RANGE).text("Back and front (m)"));
                ui.add(egui::Slider::new(&mut spatial.y, -RANGE..=RANGE).text("Height (m)"));
                ui.add(egui::Slider::new(&mut spatial.reference, 0.1..=20.0).text("Reference distance (m)"));
                ui.add(egui::Slider::new(&mut spatial.rolloff, 0.0..=8.0).text("Distance rolloff"));
                ui.add(egui::Slider::new(&mut spatial.head_shadow, 0.0..=1.0).text("Head shadow"));
                ui.add(egui::Slider::new(&mut spatial.spread, 0.0..=1.0).text("Spread"));
                ui.label(RichText::new(format!("Distance {:.1} m, level {:.0}%", spatial.distance(), spatial.gain() * 100.0)).weak());
            });
        });
    }

    fn studio_library(&mut self, ui: &mut egui::Ui) {
        let accent = self.accent();
        if self.sfx.is_empty() {
            self.sfx = voidedit_audio::sfx_library();
        }
        ui.add(egui::TextEdit::singleline(&mut self.studio_search).hint_text(format!("Search {} sounds", self.sfx.len())).desired_width(f32::INFINITY));
        let filter = self.studio_search.to_lowercase();
        let matches: Vec<usize> = (0..self.sfx.len()).filter(|index| filter.is_empty() || self.sfx[*index].name.to_lowercase().contains(&filter) || self.sfx[*index].category.to_lowercase().contains(&filter)).collect();
        let mut preview = None;
        let mut add = None;
        egui::ScrollArea::vertical().id_salt("studio-sfx").show_rows(ui, 28.0, matches.len(), |ui, rows| {
            for position in rows {
                let info = &self.sfx[matches[position]];
                let row = Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), 28.0));
                if ui.rect_contains_pointer(row) {
                    ui.painter().rect_filled(row, 4.0_f32, Color32::WHITE.gamma_multiply(0.05));
                }
                ui.horizontal(|ui| {
                    ui.set_width(row.width());
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if widgets::pill(ui, ph::PLAY, "", false, accent).on_hover_text("Preview").clicked() {
                        preview = Some(info.id.clone());
                    }
                    if widgets::pill(ui, ph::PLUS, "", false, accent).on_hover_text("Add at the playhead").clicked() {
                        add = Some((info.id.clone(), info.name.clone()));
                    }
                    ui.add_space(8.0);
                    ui.label(&info.name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(10.0);
                        ui.label(RichText::new(info.category).small().weak());
                    });
                });
            }
        });
        if let Some(id) = preview {
            if let Some(samples) = voidedit_audio::render_sfx(&id, self.audio.rate()) {
                self.audio.preview(&samples);
            }
        }
        if let Some((id, name)) = add {
            const SFX_RATE: u32 = 48_000;
            let written = Settings::folder().map(|folder| folder.join("sounds")).and_then(|folder| {
                std::fs::create_dir_all(&folder).ok()?;
                let path = folder.join(format!("{name}.wav"));
                if !path.exists() {
                    let samples = voidedit_audio::render_sfx(&id, SFX_RATE)?;
                    std::fs::write(&path, voidedit_audio::wav_bytes(&samples, SFX_RATE)).ok()?;
                }
                Some(path)
            });
            match written {
                Some(path) => self.insert_media(&path.to_string_lossy()),
                None => self.status = "Could not create that sound".to_string(),
            }
        }
    }

    fn media_pool(&mut self, ui: &mut egui::Ui) {
        let accent = self.accent();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(92.0);
                for (index, icon, label) in [(0, ph::FOLDER, "Master"), (1, ph::FILM_STRIP, "Video"), (2, ph::MUSIC_NOTES, "Audio")] {
                    if widgets::pill(ui, icon, label, self.bin == index, accent).clicked() {
                        self.bin = index;
                    }
                }
            });
            ui.vertical(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(f32::INFINITY));
                let filter = self.search.to_lowercase();
                self.media_list(ui, &filter);
            });
        });
    }

    fn toolbox(&mut self, ui: &mut egui::Ui) {
        let accent = self.accent();
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(118.0);
                for (tab, icon, label) in [(LeftTab::Transitions, ph::ARROWS_LEFT_RIGHT, "Transitions"), (LeftTab::Text, ph::TEXT_T, "Titles"), (LeftTab::Effects, ph::SPARKLE, "Effects"), (LeftTab::Filters, ph::SWATCHES, "Filters"), (LeftTab::Captions, ph::SUBTITLES, "Captions")] {
                    if widgets::pill(ui, icon, label, self.left == tab, accent).clicked() && self.left != tab {
                        self.left = tab;
                        self.tab_changed = Instant::now();
                    }
                }
            });
            ui.vertical(|ui| {
                ui.set_min_height(height);
                let progress = if self.settings.animations { self.tab_changed.elapsed().as_secs_f32() / 0.2 } else { 1.0 };
                if progress < 1.0 {
                    ui.ctx().request_repaint();
                    ui.multiply_opacity(progress);
                }
                self.left_content(ui);
            });
        });
    }

    fn mixer(&mut self, ui: &mut egui::Ui) {
        let accent = self.accent();
        ui.label(RichText::new("Mixer").strong());
        ui.add_space(4.0);
        let audio: Vec<usize> = (0..self.project.tracks.len()).filter(|index| self.project.tracks[*index].kind == TrackKind::Audio).collect();
        ui.horizontal_top(|ui| {
            for (position, track) in audio.iter().copied().enumerate() {
                ui.vertical(|ui| {
                    ui.set_width(52.0);
                    ui.label(RichText::new(format!("A{}", position + 1)).small());
                    let muted = self.project.tracks[track].muted;
                    if widgets::pill(ui, if muted { ph::SPEAKER_SLASH } else { ph::SPEAKER_HIGH }, "", muted, accent).on_hover_text("Mute").clicked() {
                        self.project.tracks[track].muted = !muted;
                    }
                    let size = Vec2::new(14.0, (ui.available_height() - 4.0).max(20.0));
                    let (bar, painter) = ui.allocate_painter(size, Sense::hover());
                    painter.rect_filled(bar.rect, 1.0_f32, Color32::from_gray(14));
                    let level = if muted { 0.0 } else { self.meter };
                    let height = bar.rect.height();
                    for (from, to, color) in [(0.0, 0.7, Color32::from_rgb(60, 190, 90)), (0.7, 0.9, Color32::from_rgb(235, 205, 60)), (0.9, 1.0, Color32::from_rgb(230, 60, 50))] {
                        if level > from {
                            let top = bar.rect.bottom() - height * level.min(to);
                            painter.rect_filled(Rect::from_min_max(Pos2::new(bar.rect.left(), top), Pos2::new(bar.rect.right(), bar.rect.bottom() - height * from)), 0.0_f32, color);
                        }
                    }
                });
            }
            ui.vertical(|ui| {
                ui.set_width(40.0);
                ui.label(RichText::new("Bus1").small());
                ui.add_space(32.0);
                let size = Vec2::new(14.0, (ui.available_height() - 4.0).max(20.0));
                let (bar, painter) = ui.allocate_painter(size, Sense::hover());
                painter.rect_filled(bar.rect, 1.0_f32, Color32::from_gray(14));
                let level = Rect::from_min_max(Pos2::new(bar.rect.left(), bar.rect.bottom() - bar.rect.height() * self.meter), bar.rect.max);
                painter.rect_filled(level, 0.0_f32, Color32::from_rgb(60, 190, 90));
            });
        });
    }

    fn left_content(&mut self, ui: &mut egui::Ui) {
        if matches!(self.left, LeftTab::Effects | LeftTab::Filters) {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(f32::INFINITY));
            ui.add_space(4.0);
        }
        let filter = self.search.to_lowercase();
        match self.left {
            LeftTab::Text => {
                for (label, size, typewriter) in [("Default text", 96.0, false), ("Large title", 180.0, false), ("Small subtitle", 54.0, false), ("Typewriter", 96.0, true)] {
                    if ui.add(egui::Button::new(label).min_size(Vec2::new(ui.available_width(), 38.0))).clicked() {
                        self.add_text();
                        if let Some(clip) = self.selected.and_then(|index| self.project.clips.get_mut(index)) {
                            clip.text_size = size;
                            clip.typewriter = typewriter;
                        }
                    }
                }
            }
            LeftTab::Transitions => {
                let selected = self.selected;
                ui.label(RichText::new(if selected.is_some() { "Click to apply to the selected clip" } else { "Select a clip first" }).weak());
                for kind in TRANSITIONS {
                    if ui.add_enabled(selected.is_some(), egui::Button::new(format!("{kind:?}")).min_size(Vec2::new(ui.available_width(), 34.0))).clicked() {
                        if let Some(clip) = selected.and_then(|index| self.project.clips.get_mut(index)) {
                            clip.transition_in = kind;
                            clip.transition_out = kind;
                            let length = 15.min((clip.end - clip.start) / 2);
                            clip.fade_in = length;
                            clip.fade_out = length;
                        }
                    }
                }
            }
            LeftTab::Captions => {
                let current = voidedit_speech::LANGUAGES.iter().find(|(_, code)| *code == self.settings.caption_language).map_or("Detect automatically", |(name, _)| *name);
                egui::ComboBox::from_label("Spoken language").selected_text(current).height(320.0).show_ui(ui, |ui| {
                    if ui.selectable_label(self.settings.caption_language == voidedit_speech::AUTO_DETECT, "Detect automatically").clicked() {
                        self.settings.caption_language = voidedit_speech::AUTO_DETECT.to_string();
                        self.settings.save();
                    }
                    for (name, code) in voidedit_speech::LANGUAGES {
                        if ui.selectable_label(self.settings.caption_language == code, name).clicked() {
                            self.settings.caption_language = code.to_string();
                            self.settings.save();
                        }
                    }
                });
                ui.add_space(6.0);
                if ui.add(egui::Button::new(RichText::new("Generate from selected clip").color(Color32::WHITE)).fill(self.accent()).min_size(Vec2::new(ui.available_width(), 32.0))).on_hover_text("Downloads a 144 MB speech model the first time").clicked() {
                    self.generate_captions();
                }
                if ui.add(egui::Button::new("Import SRT file").min_size(Vec2::new(ui.available_width(), 32.0))).clicked() {
                    self.import_captions();
                }
            }
            LeftTab::Filters => {
                let selected = self.selected;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for name in LOOKS {
                        if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                            continue;
                        }
                        if ui.add_enabled(selected.is_some(), egui::Button::new(name).min_size(Vec2::new(ui.available_width(), 34.0))).clicked() {
                            if let Some(clip) = selected.and_then(|index| self.project.clips.get_mut(index)) {
                                clip.effects.extend(voidedit_core::look(name));
                            }
                        }
                    }
                });
            }
            LeftTab::Effects => {
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
                    ui.separator();
                    ui.strong("Effect Library");
                    let library = voidedit_fx::list();
                    let mut categories: Vec<&str> = library.iter().map(|info| info.category).collect();
                    categories.sort_unstable();
                    categories.dedup();
                    for category in categories {
                        let matches: Vec<&voidedit_fx::FxInfo> = library.iter().filter(|info| info.category == category && (filter.is_empty() || info.name.to_lowercase().contains(&filter))).collect();
                        if matches.is_empty() {
                            continue;
                        }
                        egui::CollapsingHeader::new(format!("{category} ({})", matches.len())).default_open(!filter.is_empty()).show(ui, |ui| {
                            for info in matches {
                                if ui.add_enabled(selected.is_some(), egui::Button::new(info.name).min_size(Vec2::new(ui.available_width(), 22.0))).clicked() {
                                    if let Some(clip) = selected.and_then(|index| self.project.clips.get_mut(index)) {
                                        let size = [clip.width as f64, clip.height as f64];
                                        clip.effects.push(Effect::Library { id: info.id.to_string(), name: info.name.to_string(), values: voidedit_fx::defaults(info.id, size) });
                                    }
                                }
                            }
                        });
                    }
                });
            }
        }
    }

    fn media_list(&mut self, ui: &mut egui::Ui, filter: &str) {
        let bin = self.bin;
        ui.horizontal(|ui| {
            if ui.button("Import").clicked() {
                self.import();
            }
            ui.label(RichText::new(format!("{} items", self.project.media.len())).weak());
        });
        ui.add_space(4.0);
        let mut add = None;
        let mut remove = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.project.media.is_empty() {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| ui.label(RichText::new("Drop video, audio or image files here").weak()));
            }
            ui.horizontal_wrapped(|ui| for file in &self.project.media {
                let path = std::path::Path::new(file);
                let name = path.file_name().map_or(file.clone(), |n| n.to_string_lossy().into_owned());
                let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                let is_audio = matches!(extension.as_str(), "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" | "opus");
                if (bin == 1 && is_audio) || (bin == 2 && !is_audio) || (!filter.is_empty() && !name.to_lowercase().contains(filter)) {
                    continue;
                }
                let response = ui.vertical(|ui| {
                    ui.set_width(92.0);
                    let (cell, painter) = ui.allocate_painter(Vec2::new(92.0, 52.0), Sense::click_and_drag());
                    painter.rect_filled(cell.rect, 2.0_f32, Color32::from_gray(16));
                    if let Some(texture) = self.thumb_textures.get(file) {
                        painter.image(texture.id(), cell.rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                    } else {
                        widgets::paint_icon(&painter, cell.rect.center(), 26.0, if is_audio { ph::MUSIC_NOTES } else { ph::FILM_STRIP }, Color32::GRAY);
                    }
                    ui.add(egui::Label::new(RichText::new(&name).small()).truncate());
                    cell
                });
                let row = response.inner.on_hover_text("Click to add at the playhead, or drag to the timeline");
                if row.clicked() {
                    add = Some(file.clone());
                }
                if row.drag_started() {
                    self.dragging_media = Some(file.clone());
                }
                row.context_menu(|ui| {
                    if ui.button("Remove from project").clicked() {
                        remove = Some(file.clone());
                        ui.close();
                    }
                });
            });
        });
        if let Some(file) = add {
            self.insert_media(&file);
        }
        if let Some(file) = remove {
            self.project.media.retain(|item| item != &file);
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
            if param.has_key(frame) {
                let current = param.keys.iter().find(|key| key.frame == frame).map_or(Ease::Smooth, |key| key.ease);
                egui::ComboBox::from_id_salt((label, "ease")).width(62.0).selected_text(format!("{current:?}")).show_ui(ui, |ui| {
                    for ease in [Ease::Linear, Ease::Smooth, Ease::Hold] {
                        if ui.selectable_label(current == ease, format!("{ease:?}")).clicked() {
                            if let Some(key) = param.keys.iter_mut().find(|key| key.frame == frame) {
                                key.ease = ease;
                            }
                        }
                    }
                });
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
        let frame = self.frame;
        let accent = self.accent();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (index, icon, label) in [(0, ph::FILM_STRIP, "Video"), (1, ph::MUSIC_NOTES, "Audio"), (2, ph::SPARKLE, "Effects"), (3, ph::ARROWS_LEFT_RIGHT, "Transition"), (4, ph::IMAGE, "Image"), (5, ph::FILE, "File")] {
                if widgets::stacked(ui, icon, label, self.inspector_tab == index, accent).clicked() {
                    self.inspector_tab = index;
                }
            }
        });
        ui.separator();
        let tab = self.inspector_tab;
        let names: Vec<String> = self.project.clips.iter().map(|clip| clip.name.clone()).collect();
        let selected = self.selected.filter(|index| *index < self.project.clips.len());
        if selected.is_none() || tab == 5 {
            let name = self.path.as_ref().and_then(|path| path.file_stem()).map_or("Untitled".to_string(), |stem| stem.to_string_lossy().into_owned());
            let rows = [
                ("Name", name),
                ("Resolution", format!("{} x {}", self.project.width, self.project.height)),
                ("Frame rate", format!("{} fps", self.project.fps)),
                ("Length", timecode(self.project.end_frame(), self.project.fps)),
                ("Clips", self.project.clips.len().to_string()),
            ];
            for (label, value) in rows {
                ui.horizontal(|ui| {
                    ui.add_sized([100.0, 20.0], egui::Label::new(RichText::new(label).color(Color32::GRAY)));
                    ui.label(value);
                });
            }
            if selected.is_none() {
                return;
            }
        }
        let Some(clip) = selected.and_then(|index| self.project.clips.get_mut(index)) else { return };
        ui.add(egui::TextEdit::singleline(&mut clip.name).desired_width(f32::INFINITY));
        egui::ScrollArea::vertical().show(ui, |ui| {
            if clip.text.is_some() {
                section(ui, tab, "Text", true, |ui| {
                    if let Some(text) = &mut clip.text {
                        ui.add(egui::TextEdit::multiline(text).desired_rows(2).desired_width(f32::INFINITY));
                    }
                    text_controls(ui, clip);
                });
            }
            section(ui, tab, "Animations", true, |ui| animation_controls(ui, clip, accent));
            section(ui, tab, "Motion", true, |ui| {
                Self::param(ui, accent, "Position X", &mut clip.x, frame, 1.0);
                Self::param(ui, accent, "Position Y", &mut clip.y, frame, 1.0);
                Self::param(ui, accent, "Scale", &mut clip.scale, frame, 0.5);
                Self::param(ui, accent, "Rotation", &mut clip.rotation, frame, 0.5);
                Self::param(ui, accent, "3D Tilt X", &mut clip.rotation_x, frame, 0.5);
                Self::param(ui, accent, "3D Tilt Y", &mut clip.rotation_y, frame, 0.5);
                Self::param(ui, accent, "3D Depth", &mut clip.depth, frame, 2.0);
            });
            section(ui, tab, "Opacity", true, |ui| {
                Self::param(ui, accent, "Opacity", &mut clip.opacity, frame, 0.5);
                egui::ComboBox::from_label("Blend").selected_text(format!("{:?}", clip.blend)).show_ui(ui, |ui| {
                    for mode in BLENDS {
                        ui.selectable_value(&mut clip.blend, mode, format!("{mode:?}"));
                    }
                });
            });
            section(ui, tab, "Transitions", true, |ui| {
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
                section(ui, tab, "Motion Tracker", false, |ui| {
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
            section(ui, tab, "Mask", false, |ui| {
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
                section(ui, tab, "Time and Audio", true, |ui| {
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
            section(ui, tab, format!("Effects ({})", clip.effects.len()), true, |ui| {
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
                        Effect::EdgeDetect | Effect::Mirror | Effect::Emboss | Effect::ChannelSwap | Effect::Sketch => {}
                        Effect::Library { id, values, .. } => {
                            for info in voidedit_fx::params(id) {
                                let Some(value) = values.get_mut(info.id) else { continue };
                                match (value, &info.kind) {
                                    (voidedit_core::LibValue::Number(number), _) => Self::param(ui, accent, info.name, number, frame, 0.25),
                                    (voidedit_core::LibValue::Point(x, y), _) => {
                                        Self::param(ui, accent, &format!("{} X", info.name), x, frame, 1.0);
                                        Self::param(ui, accent, &format!("{} Y", info.name), y, frame, 1.0);
                                    }
                                    (voidedit_core::LibValue::Color(color), _) => {
                                        ui.horizontal(|ui| {
                                            ui.label(info.name);
                                            ui.color_edit_button_rgba_unmultiplied(color);
                                        });
                                    }
                                    (voidedit_core::LibValue::Check(state), _) => {
                                        ui.checkbox(state, info.name);
                                    }
                                    (voidedit_core::LibValue::Choice(choice), voidedit_fx::ParamKind::Choice(options)) => {
                                        let shown = options.get((*choice as usize).saturating_sub(1)).cloned().unwrap_or_default();
                                        egui::ComboBox::from_label(info.name).selected_text(shown).show_ui(ui, |ui| {
                                            for (position, option) in options.iter().enumerate() {
                                                ui.selectable_value(choice, position as u32 + 1, option);
                                            }
                                        });
                                    }
                                    (voidedit_core::LibValue::Choice(choice), _) => {
                                        ui.add(egui::DragValue::new(choice).prefix(format!("{} ", info.name)));
                                    }
                                }
                            }
                        }
                        Effect::MotionBlur { length } => Self::param(ui, accent, "Length", length, frame, 0.25),
                        Effect::ZoomBlur { amount } => Self::param(ui, accent, "Amount", amount, frame, 0.5),
                        Effect::Ripple { amount, frequency } => {
                            Self::param(ui, accent, "Amount", amount, frame, 0.25);
                            Self::param(ui, accent, "Frequency", frequency, frame, 0.5);
                        }
                        Effect::Tile { count } => Self::param(ui, accent, "Count", count, frame, 0.1),
                        Effect::Letterbox { amount } => Self::param(ui, accent, "Amount", amount, frame, 0.25),
                        Effect::GradientOverlay { top, bottom, amount } => {
                            ui.horizontal(|ui| {
                                ui.color_edit_button_srgb(top);
                                ui.color_edit_button_srgb(bottom);
                            });
                            Self::param(ui, accent, "Amount", amount, frame, 0.5);
                        }
                        Effect::Border { size, color } => {
                            Self::param(ui, accent, "Size", size, frame, 0.5);
                            ui.color_edit_button_srgb(color);
                        }
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
                let controls = 70.0;
        let fps = self.project.fps;
        let area = Vec2::new(ui.available_width(), (ui.available_height() - controls).max(40.0));
        let aspect = self.project.width as f32 / self.project.height as f32;
        let size = if area.x / area.y > aspect { Vec2::new(area.y * aspect, area.y) } else { Vec2::new(area.x, area.x / aspect) };
        let pixels = ui.ctx().pixels_per_point();
        let largest = self.project.width.max(self.project.height).max(1) as f32;
        let scale = (size.x * pixels / self.project.width as f32 * self.settings.preview_quality).clamp(0.05, 1.0).min(MAX_TEXTURE_SIDE as f32 / largest);
        let time = self.frame;
        let mut found = Vec::new();
        self.project.wanted_media(time, 0, &mut found);
        self.frames.want(found.into_iter().map(|(path, seconds)| FrameWant { path, seconds, scale }).collect());
        let generation = self.frames.generation();
        let stale = self.rendered.as_ref().is_none_or(|(frame, project, s, g)| *frame != self.frame || *s != scale || *g != generation || *project != self.project);
        if stale {
            let frames = self.frames.clone();
            render(&self.project, self.frame, scale, &mut self.preview, &|key| frames.latest(key));
            let image = egui::ColorImage::from_rgba_unmultiplied([self.preview.width, self.preview.height], &self.preview.pixels);
            self.texture = Some(ui.ctx().load_texture("program", image, egui::TextureOptions::LINEAR));
            self.rendered = Some((self.frame, self.project.clone(), scale, generation));
        }
        ui.allocate_ui_with_layout(area, egui::Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
            let shown = self.texture.as_ref().map(egui::TextureHandle::id);
            if let Some(id) = shown {
                let picture = ui.allocate_rect(Rect::from_center_size(ui.max_rect().center(), size), Sense::click_and_drag());
                ui.painter().image(id, picture.rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                self.preview_overlay(ui, &picture);
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
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 142.0).max(0.0) / 2.0);
                if widgets::transport(ui, widgets::Glyph::Back, 20.0).on_hover_text("Previous edit (Up)").clicked() {
                    self.jump_edit(false);
                }
                if widgets::transport(ui, if self.playing { widgets::Glyph::Pause } else { widgets::Glyph::Play }, 24.0).on_hover_text("Play or pause (Space)").clicked() {
                    self.playing = !self.playing;
                }
                if widgets::transport(ui, widgets::Glyph::Forward, 20.0).on_hover_text("Next edit (Down)").clicked() {
                    self.jump_edit(true);
                }
            });
        });
        ui.horizontal(|ui| {
            let text = |frame: u32| RichText::new(timecode(frame, fps)).monospace().size(12.0);
            ui.label(text(self.frame));
            let remaining = ui.available_width() - 92.0;
            ui.allocate_ui(Vec2::new(remaining.max(40.0), 18.0), |ui| {
                ui.set_width(remaining.max(40.0));
                if widgets::scrubber(ui, &mut position, end as f32, accent) {
                    self.frame = (position as u32).min(self.project.duration - 1);
                }
            });
            ui.label(text(end).weak());
        });
    }

    fn preview_overlay(&mut self, ui: &egui::Ui, picture: &egui::Response) {
        if self.drawing_mask || self.picking_track {
            return;
        }
        let Some(index) = self.selected.filter(|index| *index < self.project.clips.len()) else { return };
        let frame = self.frame;
        let time = frame as f32;
        let ratio = picture.rect.width() / self.project.width as f32;
        let clip = &self.project.clips[index];
        if frame < clip.start || frame >= clip.end {
            return;
        }
        let to_screen = |x: f32, y: f32| picture.rect.min + Vec2::new(x, y) * ratio;
        let scale = clip.scale.at(time).max(0.0) / 100.0;
        let (center_x, center_y) = (clip.x.at(time), clip.y.at(time));
        let (half_w, half_h) = (clip.width * scale / 2.0, clip.height * scale / 2.0);
        let (sin, cos) = clip.rotation.at(time).to_radians().sin_cos();
        let corner = |dx: f32, dy: f32| to_screen(center_x + dx * cos - dy * sin, center_y + dx * sin + dy * cos);
        let corners = [corner(-half_w, -half_h), corner(half_w, -half_h), corner(half_w, half_h), corner(-half_w, half_h)];
        let painter = ui.painter().with_clip_rect(picture.rect);
        let accent = self.accent();
        let polygon = clip.mask.shape == MaskShape::Polygon;
        let mask_points: Vec<Pos2> = if polygon {
            clip.mask.points.iter().map(|point| corner((point[0] - 0.5) * half_w * 2.0, (point[1] - 0.5) * half_h * 2.0)).collect()
        } else {
            Vec::new()
        };
        if clip.text.is_none() {
            for side in 0..4 {
                painter.line_segment([corners[side], corners[(side + 1) % 4]], Stroke::new(1.0_f32, accent));
            }
            for point in corners {
                painter.rect_filled(Rect::from_center_size(point, Vec2::splat(8.0)), 1.0_f32, Color32::WHITE);
            }
        }
        for (position, point) in mask_points.iter().enumerate() {
            painter.circle_filled(*point, 5.0, accent);
            painter.line_segment([*point, mask_points[(position + 1) % mask_points.len()]], Stroke::new(1.0_f32, Color32::WHITE));
        }
        let pointer = picture.interact_pointer_pos();
        if picture.drag_started() {
            if let Some(position) = pointer {
                let center = to_screen(center_x, center_y);
                self.gizmo = if let Some(point) = mask_points.iter().position(|point| point.distance(position) < 9.0) {
                    Some(Gizmo::MaskPoint(point))
                } else if corners.iter().any(|point| point.distance(position) < 10.0) {
                    Some(Gizmo::Scale { distance: center.distance(position).max(1.0), scale: clip.scale.at(time) })
                } else {
                    Some(Gizmo::Move)
                };
            }
        }
        if picture.dragged() {
            let delta = picture.drag_delta() / ratio.max(0.001);
            let center = to_screen(center_x, center_y);
            let clip = &mut self.project.clips[index];
            match self.gizmo {
                Some(Gizmo::Move) => {
                    if picture.drag_started() {
                        self.gizmo_raw = (center_x, center_y);
                    }
                    self.gizmo_raw.0 += delta.x;
                    self.gizmo_raw.1 += delta.y;
                    let (mut next_x, mut next_y) = self.gizmo_raw;
                    if self.settings.snapping && !ui.input(|input| input.modifiers.alt) {
                        let reach = SNAP_PIXELS / ratio.max(0.001);
                        let size = clip.scale.at(time) / 100.0;
                        let (half_w, half_h) = (clip.width * size / 2.0, clip.height * size / 2.0);
                        let (frame_w, frame_h) = (self.project.width as f32, self.project.height as f32);
                        let snap = |value: f32, half: f32, length: f32| {
                            [(length / 2.0, length / 2.0), (half, 0.0), (length - half, length), (length / 2.0 + half, length / 2.0), (length / 2.0 - half, length / 2.0)]
                                .into_iter()
                                .filter(|(target, _)| (value - target).abs() < reach)
                                .min_by(|a, b| (value - a.0).abs().total_cmp(&(value - b.0).abs()))
                        };
                        let painter = ui.painter();
                        let guide = Stroke::new(1.0_f32, self.settings_accent());
                        if let Some((target, line)) = snap(next_x, half_w, frame_w) {
                            next_x = target;
                            painter.line_segment([to_screen(line, 0.0), to_screen(line, frame_h)], guide);
                        }
                        if let Some((target, line)) = snap(next_y, half_h, frame_h) {
                            next_y = target;
                            painter.line_segment([to_screen(0.0, line), to_screen(frame_w, line)], guide);
                        }
                    }
                    let clip = &mut self.project.clips[index];
                    clip.x.set(frame, next_x);
                    clip.y.set(frame, next_y);
                }
                Some(Gizmo::Scale { distance, scale }) => {
                    if let Some(position) = pointer {
                        clip.scale.set(frame, scale * center.distance(position) / distance);
                    }
                }
                Some(Gizmo::MaskPoint(point)) => {
                    if let (Some(target), Some(position)) = (clip.mask.points.get_mut(point), pointer) {
                        let (dx, dy) = ((position.x - center.x) / ratio, (position.y - center.y) / ratio);
                        let (lx, ly) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                        *target = [lx / (half_w * 2.0).max(1.0) + 0.5, ly / (half_h * 2.0).max(1.0) + 0.5];
                    }
                }
                None => {}
            }
        }
        if picture.drag_stopped() {
            self.gizmo = None;
        }
    }

    fn tools(&mut self, ui: &mut egui::Ui) {
        let accent = self.accent();
        ui.horizontal(|ui| {
            for (icon, tool, tip) in [(ph::CURSOR, Tool::Select, "Selection (V)"), (ph::KNIFE, Tool::Razor, "Razor (C)")] {
                if widgets::pill(ui, icon, "", self.tool == tool, accent).on_hover_text(tip).clicked() {
                    self.tool = tool;
                }
            }
            ui.add_space(8.0);
            if widgets::pill(ui, ph::ARROW_U_UP_LEFT, "", false, accent).on_hover_text("Undo").clicked() {
                self.undo();
            }
            if widgets::pill(ui, ph::ARROW_U_UP_RIGHT, "", false, accent).on_hover_text("Redo").clicked() {
                self.redo();
            }
            if widgets::pill(ui, ph::SCISSORS, "", false, accent).on_hover_text("Split at the playhead (Ctrl K)").clicked() {
                self.split_at_playhead();
            }
            if widgets::pill(ui, ph::TRASH, "", false, accent).on_hover_text("Delete").clicked() {
                self.delete_selected(false);
            }
            if widgets::pill(ui, ph::BOOKMARK_SIMPLE, "", false, accent).on_hover_text("Marker (M)").clicked() {
                self.toggle_marker();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().slider_width = 120.0;
                ui.add(egui::Slider::new(&mut self.zoom, 0.2..=60.0).logarithmic(true).show_value(false));
                widgets::pill(ui, ph::MAGNIFYING_GLASS, "", false, accent);
                let blur = self.project.motion_blur.enabled;
                ui.menu_button(format!("{}  Motion blur{}", ph::WIND, if blur { " on" } else { "" }), |ui| {
                    ui.checkbox(&mut self.project.motion_blur.enabled, "Enable motion blur");
                    ui.add(egui::Slider::new(&mut self.project.motion_blur.shutter, 0.05..=1.0).text("Shutter"));
                    ui.add(egui::Slider::new(&mut self.project.motion_blur.samples, 2..=32).text("Export samples"));
                    ui.label(RichText::new("The preview uses up to 4 samples, exports use all of them").small().weak());
                });
                let snapping = self.settings.snapping;
                if widgets::pill(ui, ph::MAGNET, "Snap", snapping, accent).on_hover_text("Clips snap to each other, the playhead and markers").clicked() {
                    self.settings.snapping = !snapping;
                    self.settings.save();
                }
            });
        });
        ui.add_space(2.0);
    }

    fn timeline_menu(&mut self, ui: &mut egui::Ui) {
        if self.menu_opened.elapsed().as_millis() < 250 {
            ui.disable();
            ui.ctx().request_repaint();
        }
        if let Some(index) = self.menu_clip.filter(|index| *index < self.project.clips.len()) {
            if ui.button("Split at playhead").clicked() {
                self.split_at_playhead();
                ui.close();
            }
            if ui.button("Copy").clicked() {
                self.copy();
                ui.close();
            }
            if ui.button("Duplicate").clicked() {
                self.copy();
                self.paste();
                ui.close();
            }
            if ui.button("Nest").clicked() {
                self.project.nest(index);
                self.selected = Some(self.project.clips.len() - 1);
                self.menu_clip = None;
                ui.close();
                return;
            }
            let clip = &mut self.project.clips[index];
            let muted = clip.muted;
            if ui.button(if muted { "Unmute" } else { "Mute" }).clicked() {
                clip.muted = !muted;
                ui.close();
            }
            let visible = clip.visible;
            if ui.button(if visible { "Hide" } else { "Show" }).clicked() {
                clip.visible = !visible;
                ui.close();
            }
            if ui.button("Reset speed").clicked() {
                clip.speed = 1.0;
                ui.close();
            }
            ui.separator();
            if ui.button("Delete").clicked() {
                self.delete_selected(false);
                self.menu_clip = None;
                ui.close();
            }
            if ui.button("Ripple delete").clicked() {
                self.delete_selected(true);
                self.menu_clip = None;
                ui.close();
            }
        } else {
            if ui.add_enabled(self.clipboard.is_some(), egui::Button::new("Paste")).clicked() {
                self.paste();
                ui.close();
            }
            if ui.button("Add text").clicked() {
                self.add_text();
                ui.close();
            }
            if ui.button("Add marker").clicked() {
                self.toggle_marker();
                ui.close();
            }
            ui.separator();
            if ui.button("Add video track").clicked() {
                self.project.add_track(TrackKind::Video);
                ui.close();
            }
            if ui.button("Add audio track").clicked() {
                self.project.add_track(TrackKind::Audio);
                ui.close();
            }
        }
    }

    fn timeline(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0_f32, self.glass(0.7));
        painter.text(rect.min + Vec2::new(HEADER_WIDTH / 2.0, RULER_HEIGHT / 2.0), egui::Align2::CENTER_CENTER, timecode(self.frame, self.project.fps), egui::FontId::proportional(24.0), Color32::WHITE);
        let accent = self.accent();
        let zoom = self.zoom;
        let lane = rect.left() + HEADER_WIDTH;
        if response.hovered() {
            let (delta, modifiers, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.modifiers, i.pointer.hover_pos()));
            if !modifiers.command && !modifiers.shift && delta.y != 0.0 {
                let content = self.project.tracks.len() as f32 * TRACK_HEIGHT + 60.0;
                let visible = rect.height() - RULER_HEIGHT;
                self.scroll_y = (self.scroll_y - delta.y).clamp(0.0, (content - visible).max(0.0));
            }
            if modifiers.command && delta.y != 0.0 {
                let anchor = pointer.map_or(0.0, |p| (p.x - lane) / self.zoom + self.scroll);
                self.zoom = (self.zoom * (1.0 + delta.y * 0.003)).clamp(0.2, 60.0);
                if let Some(p) = pointer {
                    self.scroll = (anchor - (p.x - lane) / self.zoom).max(0.0);
                }
            } else if delta.x != 0.0 || modifiers.shift {
                let sideways = if delta.x != 0.0 { delta.x } else { delta.y };
                self.scroll = (self.scroll - sideways / self.zoom).max(0.0);
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
            content.line_segment([Pos2::new(x, rect.top() + 2.0), Pos2::new(x, rect.top() + 20.0)], Stroke::new(1.0_f32, Color32::GRAY));
            content.text(Pos2::new(x + 3.0, rect.top() + 22.0), egui::Align2::LEFT_TOP, timecode(tick, self.project.fps), egui::FontId::proportional(10.0), Color32::LIGHT_GRAY);
            tick += step;
        }
        let order: Vec<usize> = (0..self.project.tracks.len()).rev().collect();
        let row_of = |track: usize| order.iter().position(|t| *t == track).unwrap_or(0);
        let scroll_y = self.scroll_y;
        let row_rect = |row: usize| Rect::from_min_size(Pos2::new(rect.left(), rect.top() + RULER_HEIGHT - scroll_y + row as f32 * TRACK_HEIGHT), Vec2::new(rect.width(), TRACK_HEIGHT - 1.0));
        self.timeline_rect = Some((rect, order.clone()));
        let below = Rect::from_min_max(Pos2::new(rect.left(), rect.top() + RULER_HEIGHT), rect.max);
        let full = content.clone();
        let painter = painter.with_clip_rect(below);
        let content = content.with_clip_rect(Rect::from_min_max(Pos2::new(lane, below.top()), rect.max));
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        let mut header_click = None;
        for (row, track) in order.iter().enumerate() {
            let line = row_rect(row);
            let data = &self.project.tracks[*track];
            let header = Rect::from_min_size(line.min, Vec2::new(HEADER_WIDTH - 2.0, line.height()));
            painter.rect_filled(header, 0.0_f32, self.glass(0.0));
            painter.line_segment([line.left_bottom(), line.right_bottom()], Stroke::new(1.0_f32, self.shade(1.0)));
            let number = order[row..].iter().filter(|other| self.project.tracks[**other].kind == data.kind).count();
            let code = format!("{}{number}", if data.kind == TrackKind::Video { "V" } else { "A" });
            painter.text(line.min + Vec2::new(10.0, 8.0), egui::Align2::LEFT_TOP, code, egui::FontId::proportional(13.0), Color32::WHITE);
            painter.text(line.min + Vec2::new(40.0, 8.0), egui::Align2::LEFT_TOP, &data.name, egui::FontId::proportional(12.0), Color32::LIGHT_GRAY);
            let mute = Rect::from_min_size(line.min + Vec2::new(10.0, 32.0), Vec2::splat(22.0));
            let lock = Rect::from_min_size(line.min + Vec2::new(38.0, 32.0), Vec2::splat(22.0));
            for (button, on, text) in [(mute, data.muted, ph::SPEAKER_HIGH), (lock, data.locked, ph::LOCK_SIMPLE)] {
                painter.rect_filled(button, 2.0_f32, if on { accent } else { Color32::from_gray(50) });
                widgets::paint_icon(&painter, button.center(), 15.0, text, Color32::from_gray(190));
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
        if let Ok(mut ready) = self.thumbs.lock() {
            for (path, image) in ready.drain() {
                let texture = ui.ctx().load_texture(format!("thumb-{path}"), image, egui::TextureOptions::LINEAR);
                self.thumb_textures.insert(path, texture);
            }
        }
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
                TrackKind::Video if clip.text.is_some() => Color32::from_rgb(200, 85, 150),
                TrackKind::Video if self.project.tracks.iter().position(|item| item.kind == TrackKind::Video) == Some(clip.track) => Color32::from_rgb(125, 75, 215),
                TrackKind::Video => Color32::from_rgb(55, 115, 180),
                TrackKind::Audio if self.project.tracks.iter().position(|item| item.kind == TrackKind::Audio) == Some(clip.track) => Color32::from_rgb(125, 75, 215),
                TrackKind::Audio => Color32::from_rgb(60, 135, 100),
            };
            let selected = self.selected == Some(index) || self.also_selected.contains(&index);
            let glow = ui.ctx().animate_bool_with_time(egui::Id::new(("clip-glow", index)), selected, 0.12);
            let hover = pointer.is_some_and(|p| block.contains(p));
            let lift = ui.ctx().animate_bool_with_time(egui::Id::new(("clip-hover", index)), hover, 0.1);
            content.rect_filled(block, 2.0_f32, widgets::mix(fill, Color32::WHITE, 0.06 * lift));
            content.rect_stroke(block, 2.0_f32, Stroke::new(1.0 + glow, Color32::from_gray(20).lerp_to_gamma(accent, glow)), egui::StrokeKind::Inside);
            let is_video = track.kind == TrackKind::Video;
            let name_bar = Rect::from_min_max(Pos2::new(block.left(), block.bottom() - 18.0), block.max);
            if is_video {
                content.rect_filled(block, 2.0_f32, Color32::from_gray(26));
                content.rect_filled(name_bar, 0.0_f32, fill);
            }
            if let (Some(path), Some(waves)) = (&clip.media, &waves) {
                if let Some(Some(peaks)) = waves.get(path) {
                    let middle = block.center().y;
                    let width = block.width().max(1.0);
                    let color = if is_video { Color32::TRANSPARENT } else { Color32::from_white_alpha(190) };
                    let mut x = block.left().max(lane);
                    while x < block.right().min(rect.right()) {
                        let position = ((x - block.left()) / width * peaks.len() as f32) as usize;
                        let height = peaks.get(position).copied().unwrap_or(0.0).min(1.0) * block.height() * 0.42;
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
            if track.kind == TrackKind::Video {
                if let Some(texture) = clip.media.as_ref().and_then(|path| self.thumb_textures.get(path)) {
                    let [width, height] = texture.size();
                    let tall = block.height() - 20.0;
                    let wide = (width as f32 * tall / height.max(1) as f32).max(8.0);
                    let strip = content.with_clip_rect(block.intersect(content.clip_rect()));
                    let mut x = block.left() + 1.0;
                    while x < block.right().min(rect.right()) {
                        strip.image(texture.id(), Rect::from_min_size(Pos2::new(x, block.top() + 1.0), Vec2::new(wide, tall)), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                        x += wide;
                    }
                }
            }
            for param in [&clip.x, &clip.y, &clip.scale, &clip.rotation, &clip.opacity] {
                for key in &param.keys {
                    let x = to_x(key.frame as f32);
                    let y = block.bottom() - 6.0;
                    content.add(egui::Shape::convex_polygon(vec![Pos2::new(x, y - 4.0), Pos2::new(x + 4.0, y), Pos2::new(x, y + 4.0), Pos2::new(x - 4.0, y)], Color32::from_rgb(255, 215, 90), Stroke::NONE));
                }
            }
            content.with_clip_rect(block.intersect(content.clip_rect())).text(name_bar.left_center() + Vec2::new(6.0, 0.0), egui::Align2::LEFT_CENTER, &clip.name, egui::FontId::proportional(11.0), Color32::WHITE);
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
                        if ui.input(|i| i.modifiers.command) && response.clicked() {
                            if let Some(previous) = self.selected.filter(|previous| *previous != index) {
                                if !self.also_selected.contains(&previous) {
                                    self.also_selected.push(previous);
                                }
                            }
                            self.also_selected.retain(|other| *other != index);
                        } else if !self.also_selected.contains(&index) && self.selected != Some(index) {
                            self.also_selected.clear();
                        }
                        self.selected = Some(index);
                        self.drag = (response.drag_started() && !locked).then_some((index, mode));
                    }
                } else if response.clicked() {
                    self.selected = None;
                    self.also_selected.clear();
                }
            }
            if response.dragged() {
                if let Some((index, mode)) = self.drag {
                    let row = ((pos.y - rect.top() - RULER_HEIGHT + scroll_y) / TRACK_HEIGHT).max(0.0) as usize;
                    let target = order.get(row).copied();
                    let reach = if self.settings.snapping { (10.0 / zoom) as u32 + 1 } else { 0 };
                    let mut line = None;
                    let frames = self.project.snap(time as u32, Some(index), self.frame, reach);
                    let snap_start = self.project.clone();
                    let mut group_shift = 0i64;
                    if let Some(clip) = self.project.clips.get_mut(index) {
                        match mode {
                            Drag::Move { grab } => {
                                let length = clip.end - clip.start;
                                let wanted = (time - grab).max(0.0) as u32;
                                let by_start = snap_start.snap(wanted, Some(index), self.frame, reach);
                                let by_end = snap_start.snap(wanted + length, Some(index), self.frame, reach);
                                let start = if by_start != wanted && (by_end == wanted + length || by_start.abs_diff(wanted) <= by_end.abs_diff(wanted + length)) {
                                    line = Some(by_start);
                                    by_start
                                } else if by_end != wanted + length {
                                    line = Some(by_end);
                                    by_end.saturating_sub(length)
                                } else {
                                    wanted
                                };
                                let shift = start as i64 - clip.start as i64;
                                clip.start = start;
                                clip.end = start + length;
                                group_shift = shift;
                                if let Some(target) = target {
                                    let tracks = &snap_start.tracks;
                                    if tracks[target].kind == tracks[clip.track].kind && !tracks[target].locked {
                                        clip.track = target;
                                    }
                                }
                            }
                            Drag::TrimStart => {
                                let start = frames.min(clip.end - 1);
                                line = (frames != time as u32).then_some(frames);
                                clip.offset = (clip.offset as i64 + start as i64 - clip.start as i64).max(0) as u32;
                                clip.start = start;
                            }
                            Drag::TrimEnd => {
                                line = (frames != time as u32).then_some(frames);
                                clip.end = frames.max(clip.start + 1);
                            }
                        }
                    }
                    self.snap_line = line;
                    if group_shift != 0 {
                        let lowest = self.also_selected.iter().filter_map(|other| self.project.clips.get(*other)).map(|clip| clip.start as i64).min().unwrap_or(0);
                        let shift = group_shift.max(-lowest);
                        for other in self.also_selected.clone() {
                            if let Some(clip) = self.project.clips.get_mut(other) {
                                clip.start = (clip.start as i64 + shift) as u32;
                                clip.end = (clip.end as i64 + shift) as u32;
                            }
                        }
                    }
                } else if in_ruler {
                    self.frame = time as u32;
                }
            }
        }
        if response.drag_stopped() {
            self.drag = None;
            self.snap_line = None;
        }
        if response.secondary_clicked() {
            self.menu_opened = Instant::now();
            self.menu_clip = hit.map(|(index, _)| index);
            if self.menu_clip.is_some() {
                self.selected = self.menu_clip;
            }
        }
        response.context_menu(|ui| self.timeline_menu(ui));
        if self.project.clips.is_empty() {
            full.text(below.center(), egui::Align2::CENTER_CENTER, "Drag media here to start creating", egui::FontId::proportional(14.0), Color32::GRAY);
        }
        if let Some(frame) = self.snap_line {
            let x = to_x(frame as f32);
            full.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.5_f32, Color32::from_rgb(255, 225, 90)));
        }
        let content_height = self.project.tracks.len() as f32 * TRACK_HEIGHT + 60.0;
        if content_height > below.height() {
            let ratio = below.height() / content_height;
            let top = below.top() + self.scroll_y * ratio;
            painter.rect_filled(Rect::from_min_size(Pos2::new(rect.right() - 5.0, top), Vec2::new(4.0, below.height() * ratio)), 2.0_f32, Color32::from_gray(110));
        }
        for marker in &self.project.markers {
            let x = to_x(*marker as f32);
            full.line_segment([Pos2::new(x, rect.top() + RULER_HEIGHT - 8.0), Pos2::new(x, rect.top() + RULER_HEIGHT)], Stroke::new(3.0_f32, Color32::from_rgb(90, 200, 120)));
        }
        let head = to_x(self.frame as f32);
        full.line_segment([Pos2::new(head, rect.top()), Pos2::new(head, rect.bottom())], Stroke::new(1.5_f32, accent));
        full.circle_filled(Pos2::new(head, rect.top() + 6.0), 5.0, accent);
        let bar = Rect::from_min_max(Pos2::new(lane, rect.bottom() - 12.0), rect.max);
        let seen = (rect.right() - lane) / zoom;
        let total = (self.project.end_frame() as f32 * 1.1 + 30.0).max(scroll + seen);
        let track = ui.interact(bar, egui::Id::new("timeline-scrollbar"), Sense::click_and_drag());
        let thumb_width = (seen / total * bar.width()).clamp(30.0, bar.width());
        if let Some(pointer) = track.interact_pointer_pos() {
            let ratio = ((pointer.x - bar.left() - thumb_width / 2.0) / (bar.width() - thumb_width).max(1.0)).clamp(0.0, 1.0);
            self.scroll = ratio * (total - seen).max(0.0);
        }
        let travel = scroll / (total - seen).max(1.0);
        let thumb = Rect::from_min_size(Pos2::new(bar.left() + travel.clamp(0.0, 1.0) * (bar.width() - thumb_width), bar.top() + 2.0), Vec2::new(thumb_width, 8.0));
        ui.painter().rect_filled(bar, 0.0_f32, self.shade(0.8));
        ui.painter().rect_filled(thumb, 4.0_f32, if track.hovered() || track.dragged() { Color32::from_gray(150) } else { Color32::from_gray(95) });
    }
}

fn system_fonts() -> &'static [(String, String)] {
    static FONTS: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
    FONTS.get_or_init(|| {
        let mut fonts = Vec::new();
        let home = std::env::var("HOME").unwrap_or_default();
        let windows = std::env::var("WINDIR").map(|folder| format!("{folder}/Fonts")).unwrap_or_default();
        let local = std::env::var("LOCALAPPDATA").map(|folder| format!("{folder}/Microsoft/Windows/Fonts")).unwrap_or_default();
        let mut folders = vec![windows, local, "/usr/share/fonts".to_string(), "/Library/Fonts".to_string(), "/System/Library/Fonts".to_string(), format!("{home}/.fonts"), format!("{home}/Library/Fonts")];
        while let Some(folder) = folders.pop() {
            let Ok(entries) = std::fs::read_dir(&folder) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    folders.push(path.to_string_lossy().into_owned());
                } else if path.extension().is_some_and(|extension| matches!(extension.to_string_lossy().to_lowercase().as_str(), "ttf" | "otf" | "ttc")) {
                    fonts.push((path.file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()), path.to_string_lossy().into_owned()));
                }
            }
        }
        fonts.sort();
        fonts
    })
}

fn text_controls(ui: &mut egui::Ui, clip: &mut Clip) {
    let style = &mut clip.text_style;
    let current = std::path::Path::new(&style.font).file_stem().map_or_else(|| "Default".to_string(), |stem| stem.to_string_lossy().into_owned());
    ui.horizontal(|ui| {
        ui.label("Font");
        egui::ComboBox::from_id_salt("text-font").width(150.0).selected_text(current).height(320.0).show_ui(ui, |ui| {
            if ui.selectable_label(style.font.is_empty(), "Default").clicked() {
                style.font.clear();
            }
            for (name, path) in system_fonts() {
                if ui.selectable_label(&style.font == path, name).clicked() {
                    style.font = path.clone();
                }
            }
        });
        if ui.button("File").on_hover_text("Use your own font file").clicked() {
            if let Some(path) = rfd::FileDialog::new().add_filter("Font", &["ttf", "otf", "ttc"]).pick_file() {
                style.font = path.to_string_lossy().into_owned();
            }
        }
    });
    ui.add(egui::Slider::new(&mut clip.text_size, 8.0..=400.0).text("Size"));
    ui.add(egui::Slider::new(&mut style.spacing, -20.0..=100.0).text("Letter spacing"));
    ui.add(egui::Slider::new(&mut style.line_height, 0.5..=3.0).text("Line height"));
    ui.add(egui::Slider::new(&mut style.bold, 0.0..=12.0).text("Bold"));
    ui.add(egui::Slider::new(&mut style.slant, -0.6..=0.6).text("Slant"));
    ui.horizontal(|ui| {
        ui.label("Align");
        for (value, icon) in [(0u8, ph::TEXT_ALIGN_LEFT), (1, ph::TEXT_ALIGN_CENTER), (2, ph::TEXT_ALIGN_RIGHT)] {
            ui.selectable_value(&mut style.align, value, icon);
        }
        ui.checkbox(&mut style.uppercase, "Uppercase");
    });
    ui.horizontal(|ui| {
        ui.label("Color");
        ui.color_edit_button_srgb(&mut clip.color);
        let mut gradient = style.gradient.is_some();
        if ui.checkbox(&mut gradient, "Gradient").changed() {
            style.gradient = gradient.then_some(clip.color);
        }
        if let Some(bottom) = &mut style.gradient {
            ui.color_edit_button_srgb(bottom);
        }
    });
    ui.horizontal(|ui| {
        ui.add(egui::Slider::new(&mut clip.text_stroke, 0.0..=16.0).text("Outline"));
        ui.color_edit_button_srgb(&mut style.outline_color);
    });
    ui.horizontal(|ui| {
        ui.add(egui::Slider::new(&mut style.glow, 0.0..=40.0).text("Glow"));
        ui.color_edit_button_srgb(&mut style.glow_color);
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut clip.text_background, "Background");
        ui.color_edit_button_srgb(&mut style.background_color);
    });
    ui.add(egui::Slider::new(&mut style.wave, 0.0..=80.0).text("Letter wave"));
    ui.add(egui::Slider::new(&mut style.wave_speed, 0.0..=20.0).text("Wave speed"));
    ui.checkbox(&mut clip.typewriter, "Typewriter");
}

fn saved_stacks() -> Vec<(String, std::path::PathBuf)> {
    let Some(folder) = Settings::folder().map(|folder| folder.join("animations")) else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut stacks: Vec<(String, std::path::PathBuf)> = entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "json")).map(|path| (path.file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()), path)).collect();
    stacks.sort();
    stacks
}

fn animation_controls(ui: &mut egui::Ui, clip: &mut Clip, accent: Color32) {
    let length = clip.end.saturating_sub(clip.start).max(1);
    ui.horizontal(|ui| {
        ui.menu_button(format!("{}  Add preset", ph::SPARKLE), |ui| {
            for (name, stack) in voidedit_core::animation::presets() {
                if ui.button(name).clicked() {
                    clip.animations.extend(stack);
                    ui.close();
                }
            }
            let saved = saved_stacks();
            if !saved.is_empty() {
                ui.separator();
            }
            for (name, path) in saved {
                if ui.button(&name).clicked() {
                    if let Some(stack) = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Vec<Animation>>(&text).ok()) {
                        clip.animations.extend(stack);
                    }
                    ui.close();
                }
            }
        });
        if ui.button(format!("{}  Add", ph::PLUS)).clicked() {
            clip.animations.push(Animation::new(AnimKind::Fade, Trigger::In, Curve::EaseOut, 0, 15.min(length), 100.0));
        }
        if !clip.animations.is_empty() && ui.button(format!("{}  Save", ph::FLOPPY_DISK)).on_hover_text("Save this stack as a preset").clicked() {
            let folder = Settings::folder().map(|folder| folder.join("animations"));
            if let (Some(folder), Ok(text)) = (folder, serde_json::to_string_pretty(&clip.animations)) {
                let _ = std::fs::create_dir_all(&folder);
                let _ = std::fs::write(folder.join(format!("{}.json", clip.name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], ""))), text);
            }
        }
    });
    let mut remove = None;
    for (index, animation) in clip.animations.iter_mut().enumerate() {
        egui::Frame::new().fill(Color32::WHITE.gamma_multiply(0.035)).corner_radius(4).inner_margin(6).show(ui, |ui| {
            ui.push_id(index, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut animation.enabled, "");
                    egui::ComboBox::from_id_salt("kind").width(110.0).selected_text(animation.kind.name()).show_ui(ui, |ui| {
                        for (kind, name, amount) in AnimKind::ALL {
                            if ui.selectable_label(animation.kind == kind, name).clicked() {
                                animation.kind = kind;
                                animation.amount = amount;
                            }
                        }
                    });
                    egui::ComboBox::from_id_salt("trigger").width(80.0).selected_text(animation.trigger.name()).show_ui(ui, |ui| {
                        for (trigger, name) in Trigger::ALL {
                            ui.selectable_value(&mut animation.trigger, trigger, name);
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::pill(ui, ph::TRASH, "", false, accent).clicked() {
                            remove = Some(index);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("curve").width(110.0).selected_text(animation.curve.name()).show_ui(ui, |ui| {
                        for (curve, name) in Curve::ALL {
                            ui.selectable_value(&mut animation.curve, curve, name);
                        }
                    });
                    ui.add(egui::DragValue::new(&mut animation.delay).prefix("Delay ").suffix(" f").range(0..=length));
                    ui.add(egui::DragValue::new(&mut animation.duration).prefix("Length ").suffix(" f").range(1..=length.max(600)));
                });
                ui.add(egui::Slider::new(&mut animation.amount, -2000.0..=2000.0).clamping(egui::SliderClamping::Never).text("Amount"));
            });
        });
        ui.add_space(3.0);
    }
    if let Some(index) = remove {
        clip.animations.remove(index);
    }
}

fn section_tab(title: &str) -> usize {
    match title {
        "Time and Audio" => 1,
        "Transitions" => 3,
        "Mask" => 4,
        _ if title.starts_with("Effects") => 2,
        _ => 0,
    }
}

fn section(ui: &mut egui::Ui, tab: usize, title: impl Into<String>, open: bool, add: impl FnOnce(&mut egui::Ui)) {
    let title = title.into();
    if section_tab(&title) != tab {
        return;
    }
    egui::CollapsingHeader::new(title).default_open(open).show(ui, add);
    ui.separator();
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.sync_backdrop(frame);
        self.show(ui);
    }
}

impl App {
    pub fn show(&mut self, root: &mut egui::Ui) {
        let ctx = &root.ctx().clone();
        if self.applied_round != Some(self.settings.rounded) {
            self.applied_round = Some(self.settings.rounded);
            settings::set_rounded(self.settings.rounded);
        }
        self.release_idle_memory(ctx);
        self.sync_wallpaper(ctx);
        self.page_transition(root, ctx);
        self.paint_background(ctx);
        if self.show_home {
            self.window_edges(ctx);
            self.windows(ctx);
            self.show_home(root);
            return;
        }
        self.input(ctx);
        self.tick(ctx);
        self.sync_audio(ctx);
        self.autosave();
        self.finish_tracking();
        self.finish_captions();
        self.menu(root);
        self.window_edges(ctx);
        self.windows(ctx);
        let theme = &self.settings.theme;
        let [r, g, b] = theme.card;
        let border = Stroke::new(1.0_f32, Color32::from_gray(12));
        let card = egui::Frame::new().fill(Color32::from_rgba_unmultiplied(r, g, b, self.card_alpha())).corner_radius(theme.radius).stroke(border).inner_margin(6);
        let status = egui::Frame::new().fill(self.glass(0.5)).inner_margin(egui::Margin::symmetric(10, 2));
        egui::Panel::bottom("pages").show_separator_line(false).frame(status).show_inside(root, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::pill(ui, ph::GEAR, "", false, self.accent()).on_hover_text("Settings").clicked() {
                        self.show_settings = true;
                    }
                });
            });
        });
        egui::Panel::bottom("timeline").resizable(true).default_size(430.0).min_size(180.0).show_separator_line(false).frame(card).show_inside(root, |ui| {
            if self.show_strip {
                egui::Panel::right("mixer").resizable(false).default_size(150.0).show_separator_line(false).frame(egui::Frame::new().inner_margin(egui::Margin { left: 8, ..Default::default() })).show_inside(ui, |ui| self.mixer(ui));
            }
            self.tools(ui);
            self.timeline(ui);
        });
        if self.show_media || self.show_toolbox {
            egui::Panel::left("library").resizable(true).default_size(460.0).min_size(300.0).show_separator_line(false).frame(card).show_inside(root, |ui| {
                if self.show_media && self.show_toolbox {
                    let half = ui.available_height() / 2.0;
                    egui::Panel::top("pool").resizable(true).default_size(half).show_separator_line(true).frame(egui::Frame::new()).show_inside(ui, |ui| self.media_pool(ui));
                    self.toolbox(ui);
                } else if self.show_media {
                    self.media_pool(ui);
                } else {
                    self.toolbox(ui);
                }
            });
        }
        if self.show_inspector {
            egui::Panel::right("details").resizable(true).default_size(380.0).min_size(240.0).show_separator_line(false).frame(card).show_inside(root, |ui| self.controls_panel(ui));
        }
        egui::CentralPanel::default().frame(card).show_inside(root, |ui| {
            if self.show_studio {
                self.studio(ui);
            } else {
                self.monitor(ui);
            }
        });
        self.finish_media_drag(ctx);
        if !ctx.input(|i| i.pointer.any_down()) && self.project != self.committed {
            let before = std::mem::replace(&mut self.committed, self.project.clone());
            self.history.record(before);
        }
    }
}
