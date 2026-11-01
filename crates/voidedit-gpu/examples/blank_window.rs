use eframe::egui;

struct Blank;

impl eframe::App for Blank {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| ui.label("blank"));
    }
}

fn main() -> eframe::Result {
    let renderer = if std::env::args().any(|arg| arg == "glow") { eframe::Renderer::Glow } else { eframe::Renderer::Wgpu };
    let options = eframe::NativeOptions { renderer, viewport: egui::ViewportBuilder::default().with_inner_size([1500.0, 950.0]), ..Default::default() };
    eframe::run_native("blank", options, Box::new(|_| Ok(Box::new(Blank))))
}
