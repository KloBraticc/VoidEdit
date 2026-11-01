use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub accent: [u8; 3],
    pub panel_gray: u8,
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
        Self { accent: [58, 134, 255], panel_gray: 34, ui_scale: 1.0, preview_quality: 1.0, snapping: true, gpu_preview: false, system_accent: true, caption_language: "auto".into(), keys: Default::default() }
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
