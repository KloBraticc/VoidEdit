use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, Rect, RichText, Sense, Vec2};
use egui_phosphor::regular as ph;
use serde::{Deserialize, Serialize};
use voidedit_core::{timecode, Project};

use crate::settings::Settings;
use crate::{widgets, App};

const LABELS: [[u8; 3]; 7] = [[230, 80, 90], [240, 150, 60], [235, 205, 70], [80, 195, 120], [70, 170, 240], [150, 100, 235], [235, 110, 190]];
const PRESETS: [(&str, u32, u32); 8] = [("1080p", 1920, 1080), ("1440p", 2560, 1440), ("4K", 3840, 2160), ("Vertical", 1080, 1920), ("Square", 1080, 1080), ("Portrait 4:5", 1080, 1350), ("Ultrawide", 2560, 1080), ("Cinema 4K", 4096, 2160)];
const SORTS: [(&str, Sort); 6] = [("Last opened", Sort::Opened), ("Date created", Sort::Created), ("Name", Sort::Name), ("Length", Sort::Length), ("Clips", Sort::Clips), ("File size", Sort::Size)];
const THUMB_WIDTH: usize = 320;
const SCAN_DEPTH: u32 = 6;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub path: String,
    pub name: String,
    pub created: u64,
    pub opened: u64,
    pub favorite: bool,
    pub color: Option<[u8; 3]>,
    pub tags: Vec<String>,
    pub collection: String,
    pub trashed: bool,
    pub notes: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames: u32,
    pub clips: usize,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub entries: Vec<Entry>,
    pub collections: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Sort {
    #[default]
    Opened,
    Created,
    Name,
    Length,
    Clips,
    Size,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeSettings {
    pub sort: Sort,
    pub ascending: bool,
    pub list: bool,
    pub tile: f32,
    pub details: bool,
    pub group: bool,
    pub actions: bool,
}

impl Default for HomeSettings {
    fn default() -> Self {
        Self { sort: Sort::Opened, ascending: false, list: false, tile: 190.0, details: true, group: false, actions: true }
    }
}

#[derive(Clone, PartialEq, Default)]
pub enum Section {
    #[default]
    All,
    Favorites,
    Recent,
    Unsorted,
    Collection(String),
    Trash,
}

pub struct Draft {
    name: String,
    width: u32,
    height: u32,
    fps: u32,
    collection: String,
}

#[derive(Default)]
pub struct HomeState {
    section: Section,
    search: String,
    tag: Option<String>,
    color: Option<[u8; 3]>,
    draft: Option<Draft>,
    rename: Option<(String, String)>,
    tagging: Option<(String, String)>,
    notes: Option<(String, String)>,
    new_collection: String,
    confirm_delete: Option<String>,
    empty_trash: bool,
    textures: HashMap<String, Option<egui::TextureHandle>>,
}

enum Action {
    Open(String),
    Rename(String),
    Tag(String),
    Notes(String),
    Duplicate(String),
    Favorite(String),
    Color(String, Option<[u8; 3]>),
    Collection(String, String),
    Reveal(String),
    Trash(String, bool),
    Forget(String),
    Delete(String),
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |time| time.as_secs())
}

fn ago(seconds: u64) -> String {
    let passed = now().saturating_sub(seconds);
    let plural = |count: u64, word: &str| format!("{count} {word}{} ago", if count == 1 { "" } else { "s" });
    match passed {
        0..=59 => "Just now".to_string(),
        60..=3599 => plural(passed / 60, "minute"),
        3600..=86_399 => plural(passed / 3600, "hour"),
        86_400..=2_591_999 => plural(passed / 86_400, "day"),
        2_592_000..=31_535_999 => plural(passed / 2_592_000, "month"),
        _ => plural(passed / 31_536_000, "year"),
    }
}

fn size_text(bytes: u64) -> String {
    match bytes {
        0..=1_048_575 => format!("{:.0} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

fn thumb_path(project: &str) -> Option<PathBuf> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    project.hash(&mut hasher);
    let folder = Settings::folder()?.join("thumbs");
    std::fs::create_dir_all(&folder).ok()?;
    Some(folder.join(format!("{:016x}.png", hasher.finish())))
}

fn projects_folder() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    let folder = home.map(|home| home.join("Documents").join("Void Edit")).or_else(|| Settings::folder().map(|folder| folder.join("projects")))?;
    std::fs::create_dir_all(&folder).ok()?;
    Some(folder)
}

fn unique_path(folder: &Path, name: &str) -> PathBuf {
    let clean: String = name.chars().filter(|c| c.is_alphanumeric() || " -_()".contains(*c)).collect();
    let clean = if clean.trim().is_empty() { "Untitled".to_string() } else { clean.trim().to_string() };
    let mut path = folder.join(format!("{clean}.voidedit"));
    let mut number = 2;
    while path.exists() {
        path = folder.join(format!("{clean} {number}.voidedit"));
        number += 1;
    }
    path
}

fn scan(folder: &Path, depth: u32, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 {
                scan(&path, depth - 1, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "voidedit") {
            found.push(path);
        }
    }
}

pub(crate) fn reveal(path: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(parent) = Path::new(path).parent() {
        let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
    }
}

impl Library {
    pub fn load() -> Self {
        Settings::folder().and_then(|folder| std::fs::read_to_string(folder.join("projects.json")).ok()).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let (Some(folder), Ok(text)) = (Settings::folder(), serde_json::to_string_pretty(self)) {
            let temporary = folder.join("projects.json.tmp");
            if std::fs::write(&temporary, text).is_ok() {
                let _ = std::fs::rename(&temporary, folder.join("projects.json"));
            }
        }
    }

    fn entry(&mut self, path: &str) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|entry| entry.path == path)
    }

    pub fn record(&mut self, path: &Path, project: &Project, opened: bool) {
        let text = path.to_string_lossy().into_owned();
        if self.entry(&text).is_none() {
            self.entries.push(Entry { path: text.clone(), created: now(), opened: now(), ..Entry::default() });
        }
        if let Some(entry) = self.entry(&text) {
            entry.name = path.file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
            if opened {
                entry.opened = now();
            }
            entry.width = project.width;
            entry.height = project.height;
            entry.fps = project.fps;
            entry.frames = project.end_frame();
            entry.clips = project.clips.len();
        }
        self.save();
    }
}

impl App {
    pub(crate) fn save_thumbnail(&self) {
        let Some(path) = self.path.as_ref().and_then(|path| thumb_path(&path.to_string_lossy())) else { return };
        let (width, height) = (self.preview.width, self.preview.height);
        if width == 0 || height == 0 || self.preview.pixels.chunks_exact(4).all(|pixel| pixel[0] < 6 && pixel[1] < 6 && pixel[2] < 6) {
            return;
        }
        let out_width = THUMB_WIDTH.min(width);
        let out_height = (height * out_width / width).max(1);
        let mut pixels = Vec::with_capacity(out_width * out_height * 4);
        for y in 0..out_height {
            for x in 0..out_width {
                let start = (y * height / out_height * width + x * width / out_width) * 4;
                pixels.extend_from_slice(&self.preview.pixels[start..start + 4]);
            }
        }
        let _ = image::save_buffer(path, &pixels, out_width as u32, out_height as u32, image::ColorType::Rgba8);
    }

    pub(crate) fn go_home(&mut self) {
        self.playing = false;
        if self.path.is_none() && !self.project.clips.is_empty() {
            self.path = projects_folder().map(|folder| unique_path(&folder, "Untitled"));
        }
        if let Some(path) = self.path.clone() {
            if self.write_project(&path).is_ok() {
                self.projects.record(&path, &self.project, false);
                self.save_thumbnail();
                self.home.textures.remove(&path.to_string_lossy().into_owned());
            }
        }
        self.show_home = true;
    }

    fn create_project(&mut self, draft: &Draft) {
        let Some(folder) = projects_folder() else {
            self.status = "Could not create the projects folder".to_string();
            return;
        };
        let path = unique_path(&folder, &draft.name);
        self.project = Project { width: draft.width.max(16) & !1, height: draft.height.max(16) & !1, fps: draft.fps.clamp(1, 240), ..Project::default() };
        self.committed = self.project.clone();
        self.path = Some(path.clone());
        self.selected = None;
        self.frame = 0;
        match self.write_project(&path) {
            Ok(()) => {
                self.projects.record(&path, &self.project, true);
                if let Some(entry) = self.projects.entry(&path.to_string_lossy()) {
                    entry.collection = draft.collection.clone();
                }
                self.projects.save();
                self.show_home = false;
            }
            Err(error) => self.status = format!("Could not create the project: {error}"),
        }
    }

    fn add_existing(&mut self, path: &Path) {
        if let Some(project) = std::fs::read_to_string(path).ok().and_then(|text| Project::from_json(&text).ok()) {
            let known = self.projects.entry(&path.to_string_lossy()).is_some();
            self.projects.record(path, &project, false);
            if !known {
                if let Some(entry) = self.projects.entry(&path.to_string_lossy()) {
                    entry.opened = std::fs::metadata(path).ok().and_then(|data| data.modified().ok()).and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |time| time.as_secs());
                }
            }
        }
    }

    fn matches(&self, entry: &Entry) -> bool {
        let home = &self.home;
        let section = match &home.section {
            Section::All => !entry.trashed,
            Section::Favorites => !entry.trashed && entry.favorite,
            Section::Recent => !entry.trashed && now().saturating_sub(entry.opened) < 7 * 86_400,
            Section::Unsorted => !entry.trashed && entry.collection.is_empty(),
            Section::Collection(name) => !entry.trashed && &entry.collection == name,
            Section::Trash => entry.trashed,
        };
        let search = home.search.to_lowercase();
        section
            && (search.is_empty() || entry.name.to_lowercase().contains(&search) || entry.notes.to_lowercase().contains(&search) || entry.tags.iter().any(|tag| tag.to_lowercase().contains(&search)))
            && home.tag.as_ref().is_none_or(|tag| entry.tags.contains(tag))
            && home.color.is_none_or(|color| entry.color == Some(color))
    }

    fn visible(&self) -> Vec<Entry> {
        let mut list: Vec<Entry> = self.projects.entries.iter().filter(|entry| self.matches(entry)).cloned().collect();
        let view = &self.settings.home;
        let size = |entry: &Entry| std::fs::metadata(&entry.path).map_or(0, |data| data.len());
        match view.sort {
            Sort::Opened => list.sort_by_key(|entry| entry.opened),
            Sort::Created => list.sort_by_key(|entry| entry.created),
            Sort::Name => list.sort_by_key(|entry| entry.name.to_lowercase()),
            Sort::Length => list.sort_by_key(|entry| entry.frames as u64 * 1000 / entry.fps.max(1) as u64),
            Sort::Clips => list.sort_by_key(|entry| entry.clips),
            Sort::Size => list.sort_by_key(size),
        }
        if !view.ascending {
            list.reverse();
        }
        list
    }

    fn thumb(&mut self, ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
        if let Some(found) = self.home.textures.get(path) {
            return found.clone();
        }
        let texture = thumb_path(path).and_then(|file| image::open(file).ok()).map(|picture| {
            let picture = picture.into_rgba8();
            let size = [picture.width() as usize, picture.height() as usize];
            ctx.load_texture(format!("project-{path}"), egui::ColorImage::from_rgba_unmultiplied(size, picture.as_raw()), egui::TextureOptions::LINEAR)
        });
        self.home.textures.insert(path.to_string(), texture.clone());
        texture
    }

    pub(crate) fn show_home(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let accent = self.accent();
        let theme = self.settings.theme.clone();
        let [r, g, b] = theme.card;
        let card = egui::Frame::new().fill(Color32::from_rgba_unmultiplied(r, g, b, self.card_alpha())).corner_radius(theme.radius).stroke(egui::Stroke::new(1.0_f32, Color32::from_gray(12))).inner_margin(10);
        let bar = egui::Frame::new().fill(self.glass(0.5)).inner_margin(egui::Margin { left: 10, right: 0, top: 0, bottom: 0 });
        egui::Panel::top("home-bar").exact_size(crate::TOP_BAR_HEIGHT).show_separator_line(false).frame(bar).show_inside(root, |ui| {
            self.title_drag(ui);
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new("Void Edit").strong());
                if !self.status.is_empty() {
                    ui.label(RichText::new(&self.status).small().weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.window_buttons(ui);
                    if widgets::head(ui, ph::GEAR, "Settings", false).clicked() {
                        self.show_settings = true;
                    }
                });
            });
        });
        egui::Panel::left("home-nav").resizable(false).default_size(210.0).show_separator_line(false).frame(card).show_inside(root, |ui| self.home_nav(ui, accent));
        egui::CentralPanel::default().frame(card).show_inside(root, |ui| {
            if self.settings.home.actions {
                self.home_actions(ui, accent);
                ui.add_space(10.0);
            }
            self.home_toolbar(ui, accent);
            ui.add_space(6.0);
            let action = self.home_projects(ui, &ctx, accent);
            if let Some(action) = action {
                self.home_action(action);
            }
        });
        self.home_dialogs(&ctx, accent);
    }

    fn home_nav(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let count = |app: &App, section: Section| {
            app.projects.entries.iter().filter(|entry| match &section {
                Section::All => !entry.trashed,
                Section::Favorites => !entry.trashed && entry.favorite,
                Section::Recent => !entry.trashed && now().saturating_sub(entry.opened) < 7 * 86_400,
                Section::Unsorted => !entry.trashed && entry.collection.is_empty(),
                Section::Collection(name) => !entry.trashed && &entry.collection == name,
                Section::Trash => entry.trashed,
            }).count()
        };
        let row = |app: &mut App, ui: &mut egui::Ui, icon: &str, label: &str, section: Section| {
            let number = count(app, section.clone());
            ui.horizontal(|ui| {
                if widgets::pill(ui, icon, label, app.home.section == section, accent).clicked() {
                    app.home.section = section;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(number.to_string()).small().weak());
                });
            });
        };
        egui::ScrollArea::vertical().id_salt("home-nav-scroll").show(ui, |ui| {
            ui.label(RichText::new("Library").small().weak());
            row(self, ui, ph::SQUARES_FOUR, "All projects", Section::All);
            row(self, ui, ph::CLOCK, "Recent", Section::Recent);
            row(self, ui, ph::STAR, "Favorites", Section::Favorites);
            row(self, ui, ph::TRAY, "Unsorted", Section::Unsorted);
            row(self, ui, ph::TRASH, "Trash", Section::Trash);
            ui.add_space(10.0);
            ui.label(RichText::new("Collections").small().weak());
            for name in self.projects.collections.clone() {
                let response = ui.horizontal(|ui| row(self, ui, ph::FOLDER, &name, Section::Collection(name.clone()))).response;
                response.context_menu(|ui| {
                    if ui.button("Delete collection").clicked() {
                        self.projects.collections.retain(|item| item != &name);
                        for entry in &mut self.projects.entries {
                            if entry.collection == name {
                                entry.collection.clear();
                            }
                        }
                        if self.home.section == Section::Collection(name.clone()) {
                            self.home.section = Section::All;
                        }
                        self.projects.save();
                        ui.close();
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.home.new_collection).hint_text("New collection").desired_width(130.0));
                let name = self.home.new_collection.trim().to_string();
                if widgets::pill(ui, ph::PLUS, "", false, accent).on_hover_text("Add collection").clicked() && !name.is_empty() && !self.projects.collections.contains(&name) {
                    self.projects.collections.push(name);
                    self.projects.save();
                    self.home.new_collection.clear();
                }
            });
            ui.add_space(10.0);
            ui.label(RichText::new("Color labels").small().weak());
            ui.horizontal_wrapped(|ui| {
                for color in LABELS {
                    let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
                    let selected = self.home.color == Some(color);
                    ui.painter().circle_filled(rect.center(), if selected { 9.0 } else { 7.0 }, Color32::from_rgb(color[0], color[1], color[2]));
                    if response.clicked() {
                        self.home.color = if selected { None } else { Some(color) };
                    }
                }
            });
            let mut tags: Vec<String> = self.projects.entries.iter().flat_map(|entry| entry.tags.iter().cloned()).collect();
            tags.sort_unstable();
            tags.dedup();
            if !tags.is_empty() {
                ui.add_space(10.0);
                ui.label(RichText::new("Tags").small().weak());
                ui.horizontal_wrapped(|ui| {
                    for tag in tags {
                        let selected = self.home.tag.as_ref() == Some(&tag);
                        if widgets::pill(ui, "", &tag, selected, accent).clicked() {
                            self.home.tag = if selected { None } else { Some(tag) };
                        }
                    }
                });
            }
        });
    }

    fn home_actions(&mut self, ui: &mut egui::Ui, accent: Color32) {
        ui.horizontal(|ui| {
            if widgets::primary_card(ui, ph::PLUS, "New project", "Choose a size and frame rate", accent, true).clicked() {
                self.home.draft = Some(Draft { name: "Untitled".to_string(), width: 1920, height: 1080, fps: 30, collection: String::new() });
            }
            if widgets::primary_card(ui, ph::FOLDER_OPEN, "Open file", "Open a .voidedit project", accent, false).clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("Void Edit project", &["voidedit"]).pick_file() {
                    self.open_path(path);
                }
            }
            if widgets::primary_card(ui, ph::SPARKLE, "Import animation", "Lottie exported from After Effects", accent, false).clicked() {
                self.import_lottie();
            }
            if widgets::primary_card(ui, ph::MAGNIFYING_GLASS, "Find projects", "Scan a folder and add them all", accent, false).clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    let mut found = Vec::new();
                    scan(&folder, SCAN_DEPTH, &mut found);
                    for path in &found {
                        self.add_existing(path);
                    }
                    self.status = format!("Found {} projects", found.len());
                }
            }
        });
    }

    fn home_toolbar(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Projects").strong().size(15.0));
            ui.add_space(8.0);
            ui.add(egui::TextEdit::singleline(&mut self.home.search).hint_text("Search names, tags and notes").desired_width(230.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let view = &mut self.settings.home;
                ui.menu_button(format!("{}  View", ph::SLIDERS), |ui| {
                    changed |= ui.checkbox(&mut view.details, "Show details on tiles").changed();
                    changed |= ui.checkbox(&mut view.group, "Group by collection").changed();
                    changed |= ui.checkbox(&mut view.actions, "Show action cards").changed();
                    changed |= ui.add(egui::Slider::new(&mut view.tile, 130.0..=340.0).text("Tile size")).changed();
                });
                if widgets::pill(ui, ph::LIST, "", view.list, accent).on_hover_text("List").clicked() {
                    view.list = true;
                    changed = true;
                }
                if widgets::pill(ui, ph::SQUARES_FOUR, "", !view.list, accent).on_hover_text("Grid").clicked() {
                    view.list = false;
                    changed = true;
                }
                if widgets::pill(ui, if view.ascending { ph::SORT_ASCENDING } else { ph::SORT_DESCENDING }, "", false, accent).on_hover_text("Reverse order").clicked() {
                    view.ascending = !view.ascending;
                    changed = true;
                }
                let current = SORTS.iter().find(|item| item.1 == view.sort).map_or("Sort", |item| item.0);
                egui::ComboBox::from_id_salt("home-sort").selected_text(current).show_ui(ui, |ui| {
                    for (label, sort) in SORTS {
                        changed |= ui.selectable_value(&mut view.sort, sort, label).changed();
                    }
                });
                if self.home.section == Section::Trash && widgets::pill(ui, ph::TRASH, "Empty trash", false, accent).clicked() {
                    self.home.empty_trash = true;
                }
            });
        });
        if changed {
            self.settings.save();
        }
    }

    fn home_projects(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) -> Option<Action> {
        let list = self.visible();
        let mut action = None;
        egui::ScrollArea::vertical().id_salt("home-projects").auto_shrink([false, false]).show(ui, |ui| {
            if list.is_empty() {
                ui.add_space(50.0);
                ui.vertical_centered(|ui| {
                    widgets::paint_icon(ui.painter(), ui.cursor().center_top() + Vec2::new(0.0, 20.0), 36.0, ph::FILM_SLATE, Color32::GRAY);
                    ui.add_space(46.0);
                    ui.label(RichText::new("No projects here yet").weak());
                });
                return;
            }
            let groups: Vec<String> = if self.settings.home.group {
                let mut names: Vec<String> = list.iter().map(|entry| entry.collection.clone()).collect();
                names.sort_unstable();
                names.dedup();
                names
            } else {
                vec![String::new()]
            };
            let grouped = self.settings.home.group;
            for group in groups {
                if grouped {
                    ui.add_space(6.0);
                    ui.label(RichText::new(if group.is_empty() { "Unsorted" } else { group.as_str() }).strong());
                }
                let items = list.iter().filter(|entry| !grouped || entry.collection == group);
                if self.settings.home.list {
                    for entry in items {
                        let response = self.list_row(ui, ctx, entry, accent);
                        self.project_menu(&response, entry, &mut action);
                    }
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(10.0);
                        for entry in items {
                            let response = self.tile(ui, ctx, entry, accent);
                            self.project_menu(&response, entry, &mut action);
                        }
                    });
                }
            }
        });
        action
    }

    fn tile(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, entry: &Entry, accent: Color32) -> egui::Response {
        let width = self.settings.home.tile;
        let details = self.settings.home.details;
        let picture = width * 9.0 / 16.0;
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, picture + if details { 52.0 } else { 26.0 }), Sense::click());
        let hover = ctx.animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.14);
        let painter = ui.painter_at(rect.expand(3.0));
        let missing = !Path::new(&entry.path).exists();
        painter.rect_filled(rect, 6.0_f32, widgets::mix(self.shade(0.3), accent, hover * 0.16));
        let image = Rect::from_min_size(rect.min, Vec2::new(width, picture));
        painter.rect_filled(image, 6.0_f32, Color32::from_gray(12));
        if let Some(texture) = self.thumb(ctx, &entry.path) {
            let [w, h] = texture.size().map(|side| side as f32);
            let scale = (image.width() / w).min(image.height() / h);
            let shown = Rect::from_center_size(image.center(), Vec2::new(w, h) * scale);
            painter.image(texture.id(), shown, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
        } else {
            widgets::paint_icon(&painter, image.center(), 30.0, if missing { ph::WARNING } else { ph::FILM_SLATE }, Color32::from_gray(90));
        }
        if let Some(color) = entry.color {
            painter.rect_filled(Rect::from_min_size(image.min, Vec2::new(width, 3.0)), 0.0_f32, Color32::from_rgb(color[0], color[1], color[2]));
        }
        if entry.favorite {
            widgets::paint_icon(&painter, image.right_top() + Vec2::new(-14.0, 14.0), 16.0, ph::STAR, Color32::from_rgb(240, 200, 70));
        }
        painter.text(image.right_bottom() - Vec2::new(6.0, 5.0), egui::Align2::RIGHT_BOTTOM, timecode(entry.frames, entry.fps.max(1)), egui::FontId::monospace(10.0), Color32::WHITE);
        let text = ui.visuals().text_color();
        let bottom = image.left_bottom() + Vec2::new(7.0, 6.0);
        let name = painter.layout(entry.name.clone(), egui::FontId::proportional(13.0), text, width - 14.0);
        painter.galley(bottom, name.clone(), text);
        if details {
            let line = if missing { "File is missing".to_string() } else { format!("{} x {}, {}", entry.width, entry.height, ago(entry.opened)) };
            painter.text(bottom + Vec2::new(0.0, name.size().y.min(18.0) + 3.0), egui::Align2::LEFT_TOP, line, egui::FontId::proportional(11.0), Color32::GRAY);
        }
        response
    }

    fn list_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, entry: &Entry, accent: Color32) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::click());
        let hover = ctx.animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.14);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect.shrink(1.0), 4.0_f32, accent.gamma_multiply(hover * 0.16));
        let image = Rect::from_min_size(rect.min + Vec2::new(4.0, 4.0), Vec2::new(68.0, 38.0));
        painter.rect_filled(image, 3.0_f32, Color32::from_gray(12));
        if let Some(texture) = self.thumb(ctx, &entry.path) {
            painter.image(texture.id(), image, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        if let Some(color) = entry.color {
            painter.circle_filled(rect.left_center() + Vec2::new(84.0, 0.0), 4.0, Color32::from_rgb(color[0], color[1], color[2]));
        }
        let text = ui.visuals().text_color();
        painter.text(rect.left_center() + Vec2::new(96.0, 0.0), egui::Align2::LEFT_CENTER, &entry.name, egui::FontId::proportional(13.0), text);
        let size = std::fs::metadata(&entry.path).map_or_else(|_| "Missing".to_string(), |data| size_text(data.len()));
        let line = format!("{}   {} x {}   {} fps   {} clips   {}   {}", timecode(entry.frames, entry.fps.max(1)), entry.width, entry.height, entry.fps, entry.clips, size, ago(entry.opened));
        painter.text(rect.right_center() - Vec2::new(10.0, 0.0), egui::Align2::RIGHT_CENTER, line, egui::FontId::proportional(11.0), Color32::GRAY);
        if entry.favorite {
            widgets::paint_icon(&painter, rect.right_center() - Vec2::new(400.0, 0.0), 14.0, ph::STAR, Color32::from_rgb(240, 200, 70));
        }
        response
    }

    fn project_menu(&self, response: &egui::Response, entry: &Entry, action: &mut Option<Action>) {
        let path = &entry.path;
        if response.clicked() && !entry.trashed {
            *action = Some(Action::Open(path.clone()));
        }
        let hover = if entry.notes.is_empty() && entry.tags.is_empty() { String::new() } else { format!("{}\n{}", entry.tags.join(", "), entry.notes) };
        if !hover.is_empty() {
            let _ = response.clone().on_hover_text(hover);
        }
        response.context_menu(|ui| {
            let mut choose = |ui: &mut egui::Ui, label: String, chosen: Action| {
                if ui.button(label).clicked() {
                    *action = Some(chosen);
                    ui.close();
                }
            };
            if entry.trashed {
                choose(ui, format!("{}  Restore", ph::ARROW_U_UP_LEFT), Action::Trash(path.clone(), false));
                choose(ui, format!("{}  Delete forever", ph::TRASH), Action::Delete(path.clone()));
                return;
            }
            choose(ui, format!("{}  Open", ph::FOLDER_OPEN), Action::Open(path.clone()));
            choose(ui, format!("{}  {}", ph::STAR, if entry.favorite { "Remove favorite" } else { "Favorite" }), Action::Favorite(path.clone()));
            choose(ui, format!("{}  Rename", ph::PENCIL_SIMPLE), Action::Rename(path.clone()));
            choose(ui, format!("{}  Duplicate", ph::COPY), Action::Duplicate(path.clone()));
            choose(ui, format!("{}  Tags", ph::TAG), Action::Tag(path.clone()));
            choose(ui, format!("{}  Notes", ph::NOTE), Action::Notes(path.clone()));
            ui.menu_button(format!("{}  Color label", ph::PALETTE), |ui| {
                for color in LABELS {
                    let (rect, clicked) = ui.allocate_exact_size(Vec2::new(90.0, 18.0), Sense::click());
                    ui.painter().rect_filled(rect.shrink(2.0), 3.0_f32, Color32::from_rgb(color[0], color[1], color[2]));
                    if clicked.clicked() {
                        *action = Some(Action::Color(path.clone(), Some(color)));
                        ui.close();
                    }
                }
                if ui.button("None").clicked() {
                    *action = Some(Action::Color(path.clone(), None));
                    ui.close();
                }
            });
            ui.menu_button(format!("{}  Move to collection", ph::FOLDER), |ui| {
                if ui.button("None").clicked() {
                    *action = Some(Action::Collection(path.clone(), String::new()));
                    ui.close();
                }
                for name in &self.projects.collections {
                    if ui.button(name).clicked() {
                        *action = Some(Action::Collection(path.clone(), name.clone()));
                        ui.close();
                    }
                }
            });
            if ui.button(format!("{}  Show in folder", ph::ARROW_SQUARE_OUT)).clicked() {
                *action = Some(Action::Reveal(path.clone()));
                ui.close();
            }
            if ui.button(format!("{}  Remove from list", ph::X)).on_hover_text("The project file stays on disk").clicked() {
                *action = Some(Action::Forget(path.clone()));
                ui.close();
            }
            if ui.button(format!("{}  Move to trash", ph::TRASH)).clicked() {
                *action = Some(Action::Trash(path.clone(), true));
                ui.close();
            }
        });
    }

    fn home_action(&mut self, action: Action) {
        match action {
            Action::Open(path) => {
                if Path::new(&path).exists() {
                    self.open_path(PathBuf::from(path));
                } else {
                    self.status = "That project file is missing".to_string();
                }
            }
            Action::Rename(path) => {
                let name = self.projects.entry(&path).map_or_else(String::new, |entry| entry.name.clone());
                self.home.rename = Some((path, name));
            }
            Action::Tag(path) => {
                let tags = self.projects.entry(&path).map_or_else(String::new, |entry| entry.tags.join(", "));
                self.home.tagging = Some((path, tags));
            }
            Action::Notes(path) => {
                let notes = self.projects.entry(&path).map_or_else(String::new, |entry| entry.notes.clone());
                self.home.notes = Some((path, notes));
            }
            Action::Duplicate(path) => {
                let source = PathBuf::from(&path);
                if let (Some(folder), Some(entry)) = (source.parent().map(Path::to_path_buf), self.projects.entry(&path).cloned()) {
                    let copy = unique_path(&folder, &format!("{} copy", entry.name));
                    if std::fs::copy(&source, &copy).is_ok() {
                        self.projects.entries.push(Entry { path: copy.to_string_lossy().into_owned(), name: copy.file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()), created: now(), opened: now(), favorite: false, ..entry });
                        if let (Some(from), Some(to)) = (thumb_path(&path), thumb_path(&copy.to_string_lossy())) {
                            let _ = std::fs::copy(from, to);
                        }
                    } else {
                        self.status = "Could not duplicate the project".to_string();
                    }
                }
            }
            Action::Favorite(path) => {
                if let Some(entry) = self.projects.entry(&path) {
                    entry.favorite = !entry.favorite;
                }
            }
            Action::Color(path, color) => {
                if let Some(entry) = self.projects.entry(&path) {
                    entry.color = color;
                }
            }
            Action::Collection(path, name) => {
                if let Some(entry) = self.projects.entry(&path) {
                    entry.collection = name;
                }
            }
            Action::Reveal(path) => reveal(&path),
            Action::Trash(path, trashed) => {
                if let Some(entry) = self.projects.entry(&path) {
                    entry.trashed = trashed;
                }
            }
            Action::Forget(path) => self.projects.entries.retain(|entry| entry.path != path),
            Action::Delete(path) => self.home.confirm_delete = Some(path),
        }
        self.projects.save();
    }

    fn delete_project(&mut self, path: &str) {
        let _ = std::fs::remove_file(path);
        if let Some(thumb) = thumb_path(path) {
            let _ = std::fs::remove_file(thumb);
        }
        self.projects.entries.retain(|entry| entry.path != path);
        self.home.textures.remove(path);
    }

    fn home_dialogs(&mut self, ctx: &egui::Context, accent: Color32) {
        if let Some(mut draft) = self.home.draft.take() {
            let mut open = true;
            let mut create = false;
            let mut cancel = false;
            egui::Window::new("New project").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.set_width(428.0);
                widgets::setting(ui, "Name", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(320.0));
                });
                ui.add_space(8.0);
                ui.label("Format");
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(8.0);
                    for (label, width, height) in PRESETS {
                        if widgets::format_card(ui, label, width, height, draft.width == width && draft.height == height, accent).clicked() {
                            draft.width = width;
                            draft.height = height;
                        }
                    }
                });
                ui.add_space(8.0);
                widgets::setting(ui, "Size", |ui| {
                    ui.add(egui::DragValue::new(&mut draft.height).range(16..=8192).suffix(" px"));
                    ui.label("x");
                    ui.add(egui::DragValue::new(&mut draft.width).range(16..=8192).suffix(" px"));
                });
                widgets::setting(ui, "Frame rate", |ui| {
                    egui::ComboBox::from_id_salt("draft-fps").width(110.0).selected_text(format!("{} fps", draft.fps)).show_ui(ui, |ui| {
                        for fps in [24, 25, 30, 48, 50, 60, 120] {
                            ui.selectable_value(&mut draft.fps, fps, format!("{fps} fps"));
                        }
                    });
                });
                if !self.projects.collections.is_empty() {
                    widgets::setting(ui, "Collection", |ui| {
                        let shown = if draft.collection.is_empty() { "None".to_string() } else { draft.collection.clone() };
                        egui::ComboBox::from_id_salt("draft-collection").width(110.0).selected_text(shown).show_ui(ui, |ui| {
                            ui.selectable_value(&mut draft.collection, String::new(), "None");
                            for name in &self.projects.collections {
                                ui.selectable_value(&mut draft.collection, name.clone(), name);
                            }
                        });
                    });
                }
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    create = widgets::accent_button(ui, "Create", accent).clicked();
                    if widgets::plain_button(ui, "Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            if create {
                self.create_project(&draft);
            } else if open && !cancel {
                self.home.draft = Some(draft);
            }
        }
        if let Some((path, mut name)) = self.home.rename.take() {
            let mut keep = true;
            let mut done = false;
            egui::Window::new("Rename project").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.text_edit_singleline(&mut name);
                ui.horizontal(|ui| {
                    done = ui.button("Rename").clicked();
                    keep = !ui.button("Cancel").clicked();
                });
            });
            if done {
                let source = PathBuf::from(&path);
                if let Some(folder) = source.parent() {
                    let target = unique_path(folder, &name);
                    if std::fs::rename(&source, &target).is_ok() {
                        let text = target.to_string_lossy().into_owned();
                        if let (Some(from), Some(to)) = (thumb_path(&path), thumb_path(&text)) {
                            let _ = std::fs::rename(from, to);
                        }
                        if self.path.as_deref() == Some(source.as_path()) {
                            self.path = Some(target.clone());
                        }
                        if let Some(entry) = self.projects.entry(&path) {
                            entry.path = text;
                            entry.name = target.file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
                        }
                        self.home.textures.remove(&path);
                        self.projects.save();
                    } else {
                        self.status = "Could not rename the project".to_string();
                    }
                }
            } else if keep {
                self.home.rename = Some((path, name));
            }
        }
        if let Some((path, mut tags)) = self.home.tagging.take() {
            let mut keep = true;
            egui::Window::new("Tags").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.add(egui::TextEdit::singleline(&mut tags).hint_text("Separate tags with commas"));
                if ui.button("Done").clicked() {
                    keep = false;
                }
            });
            if keep {
                self.home.tagging = Some((path, tags));
            } else {
                if let Some(entry) = self.projects.entry(&path) {
                    entry.tags = tags.split(',').map(|tag| tag.trim().to_string()).filter(|tag| !tag.is_empty()).collect();
                }
                self.projects.save();
            }
        }
        if let Some((path, mut notes)) = self.home.notes.take() {
            let mut keep = true;
            egui::Window::new("Notes").collapsible(false).show(ctx, |ui| {
                ui.add(egui::TextEdit::multiline(&mut notes).desired_rows(5));
                if ui.button("Done").clicked() {
                    keep = false;
                }
            });
            if keep {
                self.home.notes = Some((path, notes));
            } else {
                if let Some(entry) = self.projects.entry(&path) {
                    entry.notes = notes;
                }
                self.projects.save();
            }
        }
        if let Some(path) = self.home.confirm_delete.take() {
            let mut keep = true;
            egui::Window::new("Delete forever").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label("This deletes the project file from your computer. Your media files are not touched.");
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        self.delete_project(&path);
                        self.projects.save();
                        keep = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
            if keep {
                self.home.confirm_delete = Some(path);
            }
        }
        if self.home.empty_trash {
            egui::Window::new("Empty trash").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label("This deletes every project file in the trash from your computer. Your media files are not touched.");
                ui.horizontal(|ui| {
                    if ui.button("Delete all").clicked() {
                        let paths: Vec<String> = self.projects.entries.iter().filter(|entry| entry.trashed).map(|entry| entry.path.clone()).collect();
                        for path in paths {
                            self.delete_project(&path);
                        }
                        self.projects.save();
                        self.home.empty_trash = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.home.empty_trash = false;
                    }
                });
            });
        }
    }
}
