use std::path::PathBuf;
use std::time::Instant;

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use voidedit_core::{timecode, Effect, History, Param, Project, Shape, TrackKind};
use voidedit_media::{Audio, AudioClip, FrameWant, Frames, Library};
use voidedit_render::{render, Frame};

const HEADER_WIDTH: f32 = 110.0;
const RULER_HEIGHT: f32 = 26.0;
const TRACK_HEIGHT: f32 = 62.0;
const ACCENT: Color32 = Color32::from_rgb(58, 134, 255);

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Select,
    Razor,
}

#[derive(Clone, Copy)]
enum Drag {
    Move { grab: f32 },
    TrimStart,
    TrimEnd,
}

pub struct App {
    project: Project,
    committed: Project,
    history: History,
    path: Option<PathBuf>,
    frame: u32,
    playing: bool,
    last_tick: Instant,
    remainder: f32,
    selected: Option<usize>,
    tool: Tool,
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
    status: String,
    update_status: std::sync::Arc<std::sync::Mutex<String>>,
}

impl App {
    pub fn new(ctx: &egui::Context, update_status: std::sync::Arc<std::sync::Mutex<String>>) -> Self {
        let library = Library::default();
        let repaint = ctx.clone();
        let frames = Frames::new(library.clone(), Box::new(move || repaint.request_repaint()));
        let audio = Audio::new(library.clone());
        let project = Project::default();
        Self {
            committed: project.clone(),
            project,
            history: History::default(),
            path: None,
            frame: 0,
            playing: false,
            last_tick: Instant::now(),
            remainder: 0.0,
            selected: None,
            tool: Tool::Select,
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
            status: String::new(),
            update_status,
        }
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

    fn open(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Void Edit project", &["voidedit"]).pick_file() else {
            return;
        };
        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| Project::from_json(&t).map_err(|e| e.to_string())) {
            Ok(project) => {
                self.project = project;
                self.committed = self.project.clone();
                self.history = History::default();
                self.path = Some(path);
                self.selected = None;
                self.frame = 0;
            }
            Err(error) => self.status = format!("Could not open the project: {error}"),
        }
    }

    fn save(&mut self, choose: bool) {
        let path = match (&self.path, choose) {
            (Some(path), false) => Some(path.clone()),
            _ => rfd::FileDialog::new().add_filter("Void Edit project", &["voidedit"]).set_file_name("Untitled.voidedit").save_file(),
        };
        let Some(path) = path else { return };
        let temporary = path.with_extension("voidedit.tmp");
        let result = self
            .project
            .to_json()
            .map_err(|e| e.to_string())
            .and_then(|text| std::fs::write(&temporary, text).map_err(|e| e.to_string()))
            .and_then(|()| std::fs::rename(&temporary, &path).map_err(|e| e.to_string()));
        match result {
            Ok(()) => {
                self.status = "Saved".into();
                self.path = Some(path);
            }
            Err(error) => self.status = format!("Could not save: {error}"),
        }
    }

    fn import(&mut self) {
        if let Some(files) = rfd::FileDialog::new().pick_files() {
            for file in files {
                let file = file.to_string_lossy().into_owned();
                if !self.project.media.contains(&file) {
                    self.project.media.push(file);
                }
            }
        }
    }

    fn add_clip(&mut self, shape: Shape) {
        let clip = self.project.new_clip(shape, self.frame);
        self.project.clips.push(clip);
        self.selected = Some(self.project.clips.len() - 1);
    }

    fn delete_selected(&mut self) {
        if let Some(index) = self.selected.take() {
            if index < self.project.clips.len() {
                self.project.clips.remove(index);
            }
        }
    }

    fn input(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let command = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, key));
        let shift_command = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, key));
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
        if plain(egui::Key::Space) {
            self.playing = !self.playing;
        }
        if plain(egui::Key::Delete) || plain(egui::Key::Backspace) {
            self.delete_selected();
        }
        if plain(egui::Key::V) {
            self.tool = Tool::Select;
        }
        if plain(egui::Key::C) {
            self.tool = Tool::Razor;
        }
        if plain(egui::Key::ArrowLeft) {
            self.frame = self.frame.saturating_sub(1);
        }
        if plain(egui::Key::ArrowRight) {
            self.frame = (self.frame + 1).min(self.project.duration - 1);
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
        if self.frame >= self.project.duration {
            self.frame = 0;
        }
        ctx.request_repaint();
    }

    fn sync_audio(&mut self, ctx: &egui::Context) {
        if self.audio_project.as_ref() != Some(&self.project) {
            let fps = self.project.fps as f64;
            let clips = self
                .project
                .clips
                .iter()
                .filter_map(|clip| {
                    Some(AudioClip {
                        path: clip.media.clone()?,
                        start: clip.start as f64 / fps,
                        end: clip.end as f64 / fps,
                        offset: clip.offset as f64 / fps,
                        gain: 1.0,
                    })
                })
                .collect();
            self.audio.set_clips(clips);
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
                    if ui.button("New").clicked() {
                        self.project = Project::default();
                        self.committed = self.project.clone();
                        self.history = History::default();
                        self.path = None;
                        self.selected = None;
                        ui.close_menu();
                    }
                    if ui.button("Open...").clicked() {
                        ui.close_menu();
                        self.open();
                    }
                    if ui.button("Save").clicked() {
                        ui.close_menu();
                        self.save(false);
                    }
                    if ui.button("Save As...").clicked() {
                        ui.close_menu();
                        self.save(true);
                    }
                    if ui.button("Import Media...").clicked() {
                        ui.close_menu();
                        self.import();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui.button("Undo").clicked() {
                        self.undo();
                        ui.close_menu();
                    }
                    if ui.button("Redo").clicked() {
                        self.redo();
                        ui.close_menu();
                    }
                    if ui.button("Delete").clicked() {
                        self.delete_selected();
                        ui.close_menu();
                    }
                });
                ui.menu_button("Clip", |ui| {
                    for (label, shape) in [("New Solid", Shape::Solid), ("New Rectangle", Shape::Rectangle), ("New Ellipse", Shape::Ellipse)] {
                        if ui.button(label).clicked() {
                            self.add_clip(shape);
                            ui.close_menu();
                        }
                    }
                });
                let name = self.path.as_ref().and_then(|p| p.file_name()).map_or("Untitled".to_string(), |n| n.to_string_lossy().into_owned());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(name).weak());
                    if let Ok(update) = self.update_status.lock() {
                        if !update.is_empty() {
                            ui.label(egui::RichText::new(update.as_str()).small().color(ACCENT));
                        }
                    }
                    if !self.status.is_empty() {
                        ui.label(egui::RichText::new(&self.status).small());
                    }
                });
            });
        });
    }

    fn project_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Project");
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            if ui.button("Import").clicked() {
                self.import();
            }
            ui.label(egui::RichText::new(format!("{} items", self.project.media.len())).weak());
        });
        ui.separator();
        let filter = self.search.to_lowercase();
        let mut add = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for file in &self.project.media {
                let name = std::path::Path::new(file).file_name().map_or(file.clone(), |n| n.to_string_lossy().into_owned());
                if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                    continue;
                }
                if ui.selectable_label(false, &name).on_hover_text("Click to add at the playhead").clicked() {
                    add = Some(file.clone());
                }
            }
        });
        if let Some(file) = add {
            match self.library.details(&file) {
                Some(details) => {
                    let size = details.has_video.then_some((details.width, details.height));
                    let clip = self.project.new_media_clip(&file, self.frame, size, details.seconds);
                    self.project.duration = self.project.duration.max(clip.end + self.project.fps);
                    self.project.clips.push(clip);
                    self.selected = Some(self.project.clips.len() - 1);
                }
                None => self.status = "This file could not be opened".into(),
            }
        }
    }

    fn param(ui: &mut egui::Ui, label: &str, param: &mut Param, frame: u32, speed: f32) {
        ui.horizontal(|ui| {
            let keyed = param.has_key(frame);
            let color = if keyed { ACCENT } else if param.keys.is_empty() { Color32::GRAY } else { Color32::LIGHT_GRAY };
            if ui.add(egui::Button::new(egui::RichText::new("\u{25C6}").color(color)).frame(false)).on_hover_text("Add or remove a keyframe here").clicked() {
                param.toggle_key(frame);
            }
            ui.add_sized([72.0, 18.0], egui::Label::new(label));
            let mut value = param.at(frame as f32);
            if ui.add(egui::DragValue::new(&mut value).speed(speed)).changed() {
                param.set(frame, value);
            }
        });
    }

    fn controls_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Effect Controls");
        ui.separator();
        let frame = self.frame;
        let Some(clip) = self.selected.and_then(|index| self.project.clips.get_mut(index)) else {
            ui.label(egui::RichText::new("Select a clip").weak());
            return;
        };
        ui.text_edit_singleline(&mut clip.name);
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::CollapsingHeader::new("Motion").default_open(true).show(ui, |ui| {
                Self::param(ui, "Position X", &mut clip.x, frame, 1.0);
                Self::param(ui, "Position Y", &mut clip.y, frame, 1.0);
                Self::param(ui, "Scale", &mut clip.scale, frame, 0.5);
                Self::param(ui, "Rotation", &mut clip.rotation, frame, 0.5);
            });
            egui::CollapsingHeader::new("Opacity").default_open(true).show(ui, |ui| {
                Self::param(ui, "Opacity", &mut clip.opacity, frame, 0.5);
            });
            if clip.media.is_none() {
                ui.horizontal(|ui| {
                    ui.label("Color");
                    ui.color_edit_button_srgb(&mut clip.color);
                });
            }
            egui::CollapsingHeader::new("Effects").default_open(true).show(ui, |ui| {
                let mut remove = None;
                for (index, effect) in clip.effects.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.strong(effect.name());
                        if ui.small_button("Remove").clicked() {
                            remove = Some(index);
                        }
                    });
                    match effect {
                        Effect::Brightness { amount } => Self::param(ui, "Amount", amount, frame, 0.5),
                        Effect::Tint { amount, color } => {
                            Self::param(ui, "Amount", amount, frame, 0.5);
                            ui.color_edit_button_srgb(color);
                        }
                        Effect::Invert => {}
                    }
                }
                if let Some(index) = remove {
                    clip.effects.remove(index);
                }
                ui.menu_button("Add Effect", |ui| {
                    if ui.button("Brightness").clicked() {
                        clip.effects.push(Effect::Brightness { amount: Param::new(0.0) });
                        ui.close_menu();
                    }
                    if ui.button("Tint").clicked() {
                        clip.effects.push(Effect::Tint { amount: Param::new(50.0), color: [255, 180, 90] });
                        ui.close_menu();
                    }
                    if ui.button("Invert").clicked() {
                        clip.effects.push(Effect::Invert);
                        ui.close_menu();
                    }
                });
            });
        });
    }

    fn monitor(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Program").strong());
        let controls = 54.0;
        let area = Vec2::new(ui.available_width(), (ui.available_height() - controls).max(40.0));
        let aspect = self.project.width as f32 / self.project.height as f32;
        let size = if area.x / area.y > aspect { Vec2::new(area.y * aspect, area.y) } else { Vec2::new(area.x, area.x / aspect) };
        let pixels = ui.ctx().pixels_per_point();
        let scale = (size.x * pixels / self.project.width as f32).clamp(0.05, 1.0);
        let fps = self.project.fps as f64;
        let time = self.frame;
        let wants: Vec<FrameWant> = self
            .project
            .clips
            .iter()
            .filter(|clip| clip.visible && time >= clip.start && time < clip.end && self.project.tracks.get(clip.track).is_some_and(|track| track.kind == TrackKind::Video))
            .filter_map(|clip| {
                let path = clip.media.clone()?;
                Some(FrameWant { path, seconds: (time - clip.start + clip.offset) as f64 / fps, scale })
            })
            .collect();
        self.frames.want(wants);
        let generation = self.frames.generation();
        let stale = self.rendered.as_ref().is_none_or(|(frame, project, s, g)| *frame != self.frame || *s != scale || *g != generation || *project != self.project);
        if stale {
            let frames = self.frames.clone();
            render(&self.project, self.frame, scale, &mut self.preview, &|path| frames.latest(path));
            let image = egui::ColorImage::from_rgba_unmultiplied([self.preview.width, self.preview.height], &self.preview.pixels);
            match &mut self.texture {
                Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                None => self.texture = Some(ui.ctx().load_texture("program", image, egui::TextureOptions::LINEAR)),
            }
            self.rendered = Some((self.frame, self.project.clone(), scale, generation));
        }
        ui.allocate_ui_with_layout(area, egui::Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
            if let Some(texture) = &self.texture {
                ui.add(egui::Image::new((texture.id(), size)).fit_to_exact_size(size));
            }
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(timecode(self.frame, self.project.fps)).color(ACCENT).monospace().size(15.0));
            ui.add_space((ui.available_width() / 2.0 - 90.0).max(0.0));
            if ui.button("|<").on_hover_text("Go to start").clicked() {
                self.frame = 0;
            }
            if ui.button("<").clicked() {
                self.frame = self.frame.saturating_sub(1);
            }
            if ui.button(if self.playing { "Pause" } else { "Play" }).clicked() {
                self.playing = !self.playing;
            }
            if ui.button(">").clicked() {
                self.frame = (self.frame + 1).min(self.project.duration - 1);
            }
            if ui.button(">|").on_hover_text("Go to end").clicked() {
                self.frame = self.project.duration - 1;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(timecode(self.project.duration, self.project.fps)).monospace());
            });
        });
    }

    fn tools(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            for (label, tool, tip) in [("Select", Tool::Select, "Selection (V)"), ("Razor", Tool::Razor, "Razor (C)")] {
                if ui.selectable_label(self.tool == tool, label).on_hover_text(tip).clicked() {
                    self.tool = tool;
                }
            }
        });
    }

    fn timeline(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0_f32, Color32::from_gray(26));
        let zoom = self.zoom;
        let lane = rect.left() + HEADER_WIDTH;
        let to_x = |frame: f32, scroll: f32| lane + (frame - scroll) * zoom;
        if response.hovered() {
            let (scroll_y, modifiers) = ui.input(|i| (i.smooth_scroll_delta.y, i.modifiers));
            if modifiers.command {
                self.zoom = (self.zoom * (1.0 + scroll_y * 0.002)).clamp(0.2, 60.0);
            } else {
                self.scroll = (self.scroll - scroll_y / self.zoom).max(0.0);
            }
        }
        let scroll = self.scroll;
        let step = [1u32, 5, 10, 30, 60, 150, 300, 600, 1800, 3600].into_iter().find(|s| *s as f32 * zoom >= 70.0).unwrap_or(7200);
        let visible_end = scroll + (rect.right() - lane) / zoom;
        let mut tick = (scroll as u32 / step) * step;
        while (tick as f32) < visible_end {
            let x = to_x(tick as f32, scroll);
            if x >= lane {
                painter.line_segment([Pos2::new(x, rect.top() + 16.0), Pos2::new(x, rect.top() + RULER_HEIGHT)], Stroke::new(1.0_f32, Color32::GRAY));
                painter.text(Pos2::new(x + 3.0, rect.top() + 2.0), egui::Align2::LEFT_TOP, timecode(tick, self.project.fps), egui::FontId::proportional(10.0), Color32::LIGHT_GRAY);
            }
            tick += step;
        }
        let order: Vec<usize> = (0..self.project.tracks.len()).rev().collect();
        let row_of = |track: usize| order.iter().position(|t| *t == track).unwrap_or(0);
        let row_rect = |row: usize| Rect::from_min_size(Pos2::new(rect.left(), rect.top() + RULER_HEIGHT + row as f32 * TRACK_HEIGHT), Vec2::new(rect.width(), TRACK_HEIGHT - 1.0));
        for (row, track) in order.iter().enumerate() {
            let line = row_rect(row);
            let data = &self.project.tracks[*track];
            painter.rect_filled(line, 0.0_f32, Color32::from_gray(32));
            painter.rect_filled(Rect::from_min_size(line.min, Vec2::new(HEADER_WIDTH - 2.0, line.height())), 0.0_f32, Color32::from_gray(44));
            painter.text(line.min + Vec2::new(10.0, 22.0), egui::Align2::LEFT_TOP, &data.name, egui::FontId::proportional(13.0), Color32::WHITE);
        }
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        let mut hit = None;
        for (index, clip) in self.project.clips.iter().enumerate() {
            let row = row_rect(row_of(clip.track));
            let left = to_x(clip.start as f32, scroll).max(lane);
            let right = to_x(clip.end as f32, scroll);
            if right <= lane {
                continue;
            }
            let block = Rect::from_min_max(Pos2::new(left, row.top() + 2.0), Pos2::new(right, row.bottom() - 2.0));
            let kind = self.project.tracks[clip.track].kind;
            let fill = match kind {
                TrackKind::Video => Color32::from_rgb(28, 62, 110),
                TrackKind::Audio => Color32::from_rgb(20, 110, 60),
            };
            let selected = self.selected == Some(index);
            let id = egui::Id::new(("clip-glow", index));
            let glow = ui.ctx().animate_bool_with_time(id, selected, 0.12);
            painter.rect_filled(block, 3.0_f32, fill.gamma_multiply(0.8 + glow * 0.4));
            painter.rect_stroke(block, 3.0_f32, Stroke::new(1.0 + glow, Color32::from_gray(90).lerp_to_gamma(Color32::WHITE, glow)), egui::StrokeKind::Inside);
            painter.with_clip_rect(block).text(block.min + Vec2::new(6.0, 4.0), egui::Align2::LEFT_TOP, &clip.name, egui::FontId::proportional(11.0), Color32::WHITE);
            if pointer.is_some_and(|p| block.contains(p)) {
                let p = pointer.unwrap_or_default();
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
        if let Some(pos) = pointer {
            let time = (((pos.x - lane) / zoom + scroll).max(0.0)).min(self.project.duration as f32 - 1.0);
            if response.drag_started() || response.clicked() {
                if pos.y < rect.top() + RULER_HEIGHT {
                    self.frame = time as u32;
                } else if let Some((index, mode)) = hit {
                    if self.tool == Tool::Razor {
                        self.project.split(index, time as u32);
                    } else {
                        self.selected = Some(index);
                        self.drag = response.drag_started().then_some((index, mode));
                    }
                } else if response.clicked() {
                    self.selected = None;
                }
            }
            if response.dragged() {
                if let Some((index, mode)) = self.drag {
                    let row = ((pos.y - rect.top() - RULER_HEIGHT) / TRACK_HEIGHT).max(0.0) as usize;
                    let target = order.get(row).copied();
                    let frames = time as u32;
                    if let Some(clip) = self.project.clips.get_mut(index) {
                        match mode {
                            Drag::Move { grab } => {
                                let length = clip.end - clip.start;
                                clip.start = (time - grab).max(0.0) as u32;
                                clip.end = clip.start + length;
                                if let Some(target) = target {
                                    if self.project.tracks[target].kind == self.project.tracks[clip.track].kind {
                                        clip.track = target;
                                    }
                                }
                            }
                            Drag::TrimStart => {
                                let start = frames.min(clip.end - 1);
                                clip.offset = (clip.offset as i64 + start as i64 - clip.start as i64).max(0) as u32;
                                clip.start = start;
                            }
                            Drag::TrimEnd => clip.end = (frames + 1).max(clip.start + 1),
                        }
                    }
                } else if pos.y < rect.top() + RULER_HEIGHT {
                    self.frame = time as u32;
                }
            }
        }
        if response.drag_stopped() {
            self.drag = None;
        }
        let head = to_x(self.frame as f32, scroll);
        if head >= lane {
            painter.line_segment([Pos2::new(head, rect.top()), Pos2::new(head, rect.bottom())], Stroke::new(1.5_f32, ACCENT));
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.input(ctx);
        self.tick(ctx);
        self.sync_audio(ctx);
        self.menu(ctx);
        egui::TopBottomPanel::bottom("timeline").resizable(true).default_height(320.0).min_height(120.0).show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                self.tools(ui);
                ui.separator();
                let meter = Vec2::new(16.0, ui.available_height());
                let timeline = Vec2::new((ui.available_width() - meter.x - 8.0).max(50.0), ui.available_height());
                ui.allocate_ui(timeline, |ui| self.timeline(ui));
                let (bar, painter) = ui.allocate_painter(meter, Sense::hover());
                painter.rect_filled(bar.rect, 2.0_f32, Color32::from_gray(20));
                let level = Rect::from_min_max(Pos2::new(bar.rect.left(), bar.rect.bottom() - bar.rect.height() * self.meter), bar.rect.max);
                painter.rect_filled(level, 2.0_f32, Color32::from_rgb(70, 210, 110));
            });
        });
        egui::SidePanel::left("project").resizable(true).default_width(300.0).show(ctx, |ui| self.project_panel(ui));
        egui::SidePanel::right("controls").resizable(true).default_width(320.0).show(ctx, |ui| self.controls_panel(ui));
        egui::CentralPanel::default().show(ctx, |ui| self.monitor(ui));
        if !ctx.input(|i| i.pointer.any_down()) && self.project != self.committed {
            let before = std::mem::replace(&mut self.committed, self.project.clone());
            self.history.record(before);
        }
    }
}

pub fn style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_gray(30);
    visuals.window_fill = Color32::from_gray(34);
    visuals.selection.bg_fill = ACCENT;
    ctx.set_visuals(visuals);
}
