use eframe::egui::{self, Color32, Response, RichText, Sense, Vec2};

pub fn mix(from: Color32, to: Color32, amount: f32) -> Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let blend = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * amount) as u8;
    Color32::from_rgb(blend(from.r(), to.r()), blend(from.g(), to.g()), blend(from.b(), to.b()))
}

fn motion(ui: &egui::Ui, response: &Response) -> (f32, f32) {
    let context = ui.ctx();
    let hover = context.animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.14);
    let press = context.animate_bool_with_time(response.id.with("press"), response.is_pointer_button_down_on(), 0.08);
    (hover, press)
}

const IDLE: Color32 = Color32::from_rgb(170, 170, 176);

pub fn paint_icon(painter: &egui::Painter, center: egui::Pos2, side: f32, icon: &str, color: Color32) {
    painter.text(center, egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(side), color);
}

fn draw(ui: &mut egui::Ui, icon: &str, label: &str, icon_color: Color32, text_color: Color32) -> Response {
    let words = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(13.0), text_color);
    let side = 18.0;
    let glyph = if icon.is_empty() { 0.0 } else { side };
    let gap = if label.is_empty() || icon.is_empty() { 0.0 } else { 7.0 };
    let width = glyph + gap + words.size().x + 12.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width.max(28.0), 26.0), Sense::click());
    let (hover, press) = motion(ui, &response);
    ui.painter().rect_filled(rect.shrink(press), 2.0_f32, Color32::WHITE.gamma_multiply(0.07 * hover));
    let left = rect.center().x - (glyph + gap + words.size().x) / 2.0;
    if !icon.is_empty() {
        paint_icon(ui.painter(), egui::pos2(left + side / 2.0, rect.center().y), side, icon, icon_color);
    }
    let height = words.size().y;
    ui.painter().galley(egui::pos2(left + glyph + gap, rect.center().y - height / 2.0), words, text_color);
    response
}

pub fn pill(ui: &mut egui::Ui, icon: &str, label: &str, active: bool, accent: Color32) -> Response {
    draw(ui, icon, label, if active { accent } else { IDLE }, if active { Color32::WHITE } else { IDLE })
}

pub fn head(ui: &mut egui::Ui, icon: &str, label: &str, active: bool) -> Response {
    let color = if active { Color32::WHITE } else { IDLE };
    draw(ui, icon, label, color, color)
}

pub fn stacked(ui: &mut egui::Ui, icon: &str, label: &str, active: bool, accent: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(52.0, 44.0), Sense::click());
    let color = if active { Color32::WHITE } else { Color32::from_gray(120) };
    paint_icon(ui.painter(), rect.center_top() + Vec2::new(0.0, 14.0), 20.0, icon, color);
    ui.painter().text(rect.center_top() + Vec2::new(0.0, 33.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(11.0), color);
    if active {
        ui.painter().rect_filled(egui::Rect::from_min_max(rect.left_bottom() - Vec2::new(0.0, 2.0), rect.right_bottom()), 0.0_f32, accent);
    }
    response
}

pub fn heading(ui: &mut egui::Ui, icon: &str, label: &str, accent: Color32) {
    ui.add_space(6.0);
    ui.label(RichText::new(format!("{icon}  {label}")).strong().color(accent));
    ui.add_space(2.0);
}
