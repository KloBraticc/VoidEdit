#![cfg_attr(windows, windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};

fn icon() -> eframe::egui::IconData {
    match image::load_from_memory(include_bytes!("../../../assets/voidedit.ico")) {
        Ok(image) => {
            let image = image.to_rgba8();
            eframe::egui::IconData { width: image.width(), height: image.height(), rgba: image.into_raw() }
        }
        Err(_) => eframe::egui::IconData::default(),
    }
}

fn main() -> eframe::Result {
    let status = Arc::new(Mutex::new(String::new()));
    let updates = status.clone();
    let _ = std::thread::Builder::new().name("update".into()).spawn(move || {
        if let Ok(Some(version)) = voidedit_update::check_and_install(env!("CARGO_PKG_VERSION")) {
            if let Ok(mut text) = updates.lock() {
                *text = format!("Updated to {version}, restart to use it");
            }
        }
    });
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default().with_title("Void Edit").with_inner_size([1500.0, 950.0]).with_min_inner_size([900.0, 600.0]).with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Void Edit",
        options,
        Box::new(move |context| {
            let mut app = voidedit_ui::App::new(&context.egui_ctx, status);
            if let Some(path) = std::env::args_os().nth(1) {
                app.open_path(path.into());
            }
            Ok(Box::new(app))
        }),
    )
}
