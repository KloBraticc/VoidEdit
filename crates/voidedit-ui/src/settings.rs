use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Backdrop {
    #[default]
    None,
    Mica,
    Tabbed,
    Acrylic,
    Blur,
}

impl Backdrop {
    pub const ALL: [(Backdrop, &'static str); 5] = [(Backdrop::None, "None"), (Backdrop::Mica, "Mica"), (Backdrop::Tabbed, "Mica Alt"), (Backdrop::Acrylic, "Acrylic"), (Backdrop::Blur, "Blur")];
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
    pub background: [u8; 3],
    pub card: [u8; 3],
    pub text: [u8; 3],
    pub card_opacity: u8,
    pub radius: f32,
}

impl Theme {
    fn preset(name: &str, background: [u8; 3], card: [u8; 3], text: [u8; 3]) -> Self {
        Self { name: name.into(), background, card, text, card_opacity: 255, radius: 2.0 }
    }

    pub fn presets() -> Vec<Theme> {
        vec![
            Self::default(),
            Self::preset("Midnight", [8, 14, 28], [16, 26, 46], [226, 232, 244]),
            Self::preset("Graphite", [20, 20, 20], [36, 36, 36], [235, 235, 235]),
            Self::preset("Aurora", [10, 22, 22], [18, 40, 38], [224, 240, 236]),
            Self::preset("Paper", [228, 228, 234], [250, 250, 252], [28, 28, 34]),
        ]
    }

    pub fn folder() -> Option<PathBuf> {
        let folder = Settings::folder()?.join("themes");
        std::fs::create_dir_all(&folder).ok()?;
        Some(folder)
    }

    pub fn saved() -> Vec<Theme> {
        let Some(entries) = Self::folder().and_then(|folder| std::fs::read_dir(folder).ok()) else { return Vec::new() };
        let mut themes: Vec<Theme> = entries.flatten().filter_map(|entry| Self::read(&entry.path())).collect();
        themes.sort_by(|a, b| a.name.cmp(&b.name));
        themes
    }

    pub fn read(path: &std::path::Path) -> Option<Theme> {
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    pub fn write(&self, path: &std::path::Path) -> bool {
        serde_json::to_string_pretty(self).is_ok_and(|text| std::fs::write(path, text).is_ok())
    }

    pub fn store(&self) -> bool {
        let name: String = self.name.chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect();
        !name.trim().is_empty() && Self::folder().is_some_and(|folder| self.write(&folder.join(format!("{}.json", name.trim()))))
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::preset("Studio", [24, 24, 28], [40, 40, 46], [200, 200, 205])
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub accent: [u8; 3],
    pub theme: Theme,
    pub backdrop: Backdrop,
    pub wallpaper: String,
    pub wallpaper_dim: f32,
    pub animations: bool,
    pub ui_scale: f32,
    pub preview_quality: f32,
    pub snapping: bool,
    pub gpu_preview: bool,
    pub system_accent: bool,
    pub caption_language: String,
    pub keys: std::collections::BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { accent: [230, 60, 50], theme: Theme::default(), backdrop: Backdrop::None, wallpaper: String::new(), wallpaper_dim: 0.45, animations: true, ui_scale: 1.0, preview_quality: 1.0, snapping: true, gpu_preview: false, system_accent: false, caption_language: "auto".into(), keys: Default::default() }
    }
}

pub const ACTIONS: [(&str, &str); 18] = [("Undo (Ctrl)", "Z"), ("Save (Ctrl)", "S"), ("Split (Ctrl)", "K"), ("Copy (Ctrl)", "C"), ("Paste (Ctrl)", "V"), ("Duplicate (Ctrl)", "D"), ("Import (Ctrl)", "I"), ("Export (Ctrl)", "M"), ("Play or pause", "Space"), ("Selection tool", "V"), ("Razor tool", "C"), ("Marker", "M"), ("New text", "T"), ("Previous frame", "ArrowLeft"), ("Next frame", "ArrowRight"), ("Previous edit", "ArrowUp"), ("Next edit", "ArrowDown"), ("Go to start", "Home")];

#[cfg(windows)]
pub fn system_accent() -> Option<[u8; 3]> {
    let key = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey("SOFTWARE\\Microsoft\\Windows\\DWM").ok()?;
    let value: u32 = key.get_value("AccentColor").ok()?;
    Some([value as u8, (value >> 8) as u8, (value >> 16) as u8])
}

#[cfg(not(windows))]
pub fn system_accent() -> Option<[u8; 3]> {
    None
}

#[cfg(windows)]
pub fn trim_memory() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
        fn K32EmptyWorkingSet(process: *mut core::ffi::c_void) -> i32;
    }
    unsafe {
        K32EmptyWorkingSet(GetCurrentProcess());
    }
}

#[cfg(not(windows))]
pub fn trim_memory() {}

impl Settings {
    pub fn key(&self, action: &str) -> Option<eframe::egui::Key> {
        let default = ACTIONS.iter().find(|(name, _)| *name == action).map(|(_, key)| *key)?;
        let chosen = self.keys.get(action).map_or(default, String::as_str);
        eframe::egui::Key::from_name(chosen)
    }

    pub fn folder() -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("XDG_CONFIG_HOME")).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        let folder = base.join("VoidEdit");
        std::fs::create_dir_all(&folder).ok()?;
        Some(folder)
    }

    pub fn load() -> Self {
        Self::folder()
            .and_then(|folder| std::fs::read_to_string(folder.join("settings.json")).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let (Some(folder), Ok(text)) = (Self::folder(), serde_json::to_string_pretty(self)) {
            let _ = std::fs::write(folder.join("settings.json"), text);
        }
    }
}
