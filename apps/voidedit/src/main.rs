#![cfg_attr(windows, windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};

const FALLBACK_ARGUMENT: &str = "--other-renderer";

fn icon() -> eframe::egui::IconData {
    match image::load_from_memory(include_bytes!("../../../assets/voidedit.ico")) {
        Ok(image) => {
            let image = image.to_rgba8();
            eframe::egui::IconData { width: image.width(), height: image.height(), rgba: image.into_raw() }
        }
        Err(_) => eframe::egui::IconData::default(),
    }
}

fn run(renderer: eframe::Renderer, project: Option<std::ffi::OsString>) -> eframe::Result {
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
        renderer,
        viewport: eframe::egui::ViewportBuilder::default().with_title("Void Edit").with_inner_size([1500.0, 950.0]).with_min_inner_size([900.0, 600.0]).with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Void Edit",
        options,
        Box::new(move |context| {
            let mut app = voidedit_ui::App::new(&context.egui_ctx, status);
            if let Some(state) = context.wgpu_render_state.clone() {
                app.use_gpu(state);
            }
            if let Some(path) = project {
                app.open_path(path.into());
            }
            Ok(Box::new(app))
        }),
    )
}

fn main() {
    let arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let is_fallback = arguments.iter().any(|argument| argument == FALLBACK_ARGUMENT);
    let project = arguments.iter().find(|argument| *argument != FALLBACK_ARGUMENT).cloned();
    let prefers_gpu = voidedit_ui::prefers_gpu_preview();
    let renderer = if prefers_gpu != is_fallback { eframe::Renderer::Wgpu } else { eframe::Renderer::Glow };
    let attempt = project.clone();
    let failed = !matches!(std::panic::catch_unwind(move || run(renderer, attempt)), Ok(Ok(())));
    if failed && !is_fallback {
        if let Ok(executable) = std::env::current_exe() {
            let mut command = std::process::Command::new(executable);
            command.arg(FALLBACK_ARGUMENT);
            if let Some(path) = project {
                command.arg(path);
            }
            let _ = command.spawn();
        }
    }
}
