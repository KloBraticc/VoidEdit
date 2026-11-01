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

struct Software(voidedit_ui::App);

impl egui_software_backend::App for Software {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _backend: &mut egui_software_backend::SoftwareBackend) {
        self.0.show(ui);
    }

    fn on_exit(&mut self, _context: &eframe::egui::Context) {
        self.0.save_now();
    }
}

#[cfg(windows)]
fn placed(builder: eframe::egui::ViewportBuilder) -> eframe::egui::ViewportBuilder {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetSystemMetrics(index: i32) -> i32;
    }
    let (screen_width, screen_height) = unsafe { (GetSystemMetrics(0) as f32, GetSystemMetrics(1) as f32) };
    let (width, height) = (1500.0_f32.min(screen_width * 0.92), 950.0_f32.min(screen_height * 0.88));
    builder.with_decorations(false).with_inner_size([width, height]).with_position([(screen_width - width) / 2.0, (screen_height - height) / 2.0])
}

#[cfg(not(windows))]
fn placed(builder: eframe::egui::ViewportBuilder) -> eframe::egui::ViewportBuilder {
    builder.with_inner_size([1500.0, 950.0])
}

fn viewport(transparent: bool) -> eframe::egui::ViewportBuilder {
    placed(eframe::egui::ViewportBuilder::default().with_title("Void Edit").with_transparent(transparent).with_min_inner_size([900.0, 600.0]).with_icon(icon()))
}

fn build_app(context: &eframe::egui::Context, status: Arc<Mutex<String>>, project: &Option<std::ffi::OsString>) -> voidedit_ui::App {
    let mut app = voidedit_ui::App::new(context, status);
    if let Some(path) = project {
        app.open_path(path.into());
    }
    app
}

fn run(accelerated: bool, project: Option<std::ffi::OsString>) -> bool {
    let status = Arc::new(Mutex::new(String::new()));
    let updates = status.clone();
    let _ = std::thread::Builder::new().name("update".into()).spawn(move || {
        if let Ok(Some(version)) = voidedit_update::check_and_install(env!("CARGO_PKG_VERSION")) {
            if let Ok(mut text) = updates.lock() {
                *text = format!("Updated to {version}, restart to use it");
            }
        }
    });
    if accelerated {
        let options = eframe::NativeOptions { renderer: eframe::Renderer::Glow, viewport: viewport(true), centered: true, ..Default::default() };
        return eframe::run_native("Void Edit", options, Box::new(move |context| Ok(Box::new(build_app(&context.egui_ctx, status, &project))))).is_ok();
    }
    let mut settings = egui_software_backend::SoftwareBackendAppConfiguration::new();
    settings.viewport_builder = viewport(false);
    settings.allow_raster_opt = false;
    egui_software_backend::run_app_with_software_backend(settings, move |context| Software(build_app(&context, status.clone(), &project))).is_ok()
}

fn main() {
    let arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let is_fallback = arguments.iter().any(|argument| argument == FALLBACK_ARGUMENT);
    let project = arguments.iter().find(|argument| *argument != FALLBACK_ARGUMENT).cloned();
    let accelerated = voidedit_ui::wants_backdrop() != is_fallback;
    let attempt = project.clone();
    let failed = !matches!(std::panic::catch_unwind(move || run(accelerated, attempt)), Ok(true));
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
