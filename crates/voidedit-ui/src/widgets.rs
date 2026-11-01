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

pub fn transport(ui: &mut egui::Ui, icon: &str, primary: bool, accent: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(if primary { 44.0 } else { 34.0 }), Sense::click());
    let (hover, press) = motion(ui, &response);
    let radius = rect.width() / 2.0 - press;
    if primary {
        ui.painter().circle_filled(rect.center(), radius, mix(accent, Color32::WHITE, hover * 0.12));
    } else {
        ui.painter().circle_filled(rect.center(), radius, Color32::WHITE.gamma_multiply(0.08 * hover));
    }
    let color = if primary { Color32::WHITE } else { mix(IDLE, Color32::WHITE, hover) };
    paint_icon(ui.painter(), rect.center(), if primary { 22.0 } else { 19.0 }, icon, color);
    response
}

pub fn scrubber(ui: &mut egui::Ui, value: &mut f32, end: f32, accent: Color32) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::click_and_drag());
    let mut changed = false;
    if let Some(pointer) = response.interact_pointer_pos() {
        *value = ((pointer.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0) * end;
        changed = true;
    }
    let hover = ui.ctx().animate_bool_with_time(response.id.with("hover"), response.hovered() || response.dragged(), 0.12);
    let x = rect.left() + rect.width() * (*value / end.max(1.0)).clamp(0.0, 1.0);
    let y = rect.center().y;
    let track = egui::Rect::from_min_max(egui::pos2(rect.left(), y - 1.5), egui::pos2(rect.right(), y + 1.5));
    ui.painter().rect_filled(track, 2.0_f32, Color32::from_gray(70));
    ui.painter().rect_filled(egui::Rect::from_min_max(track.min, egui::pos2(x, track.bottom())), 2.0_f32, accent);
    ui.painter().circle_filled(egui::pos2(x, y), 4.5 + hover * 2.0, Color32::WHITE);
    changed
}

pub fn primary_card(ui: &mut egui::Ui, icon: &str, title: &str, detail: &str, accent: Color32, primary: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(250.0, 66.0), Sense::click());
    let (hover, press) = motion(ui, &response);
    let text = ui.visuals().text_color();
    let base = if primary { accent } else { Color32::from_gray(44) };
    ui.painter().rect_filled(rect.shrink(press), 8.0_f32, mix(base, Color32::WHITE, hover * 0.08));
    let (title_color, detail_color) = if primary { (Color32::WHITE, Color32::from_white_alpha(190)) } else { (text, Color32::GRAY) };
    paint_icon(ui.painter(), rect.left_center() + Vec2::new(30.0, 0.0), 24.0, icon, title_color);
    ui.painter().text(rect.left_center() + Vec2::new(58.0, -10.0), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(15.0), title_color);
    ui.painter().text(rect.left_center() + Vec2::new(58.0, 11.0), egui::Align2::LEFT_CENTER, detail, egui::FontId::proportional(11.0), detail_color);
    response
}

pub fn heading(ui: &mut egui::Ui, icon: &str, label: &str, accent: Color32) {
    ui.add_space(6.0);
    ui.label(RichText::new(format!("{icon}  {label}")).strong().color(accent));
    ui.add_space(2.0);
}
