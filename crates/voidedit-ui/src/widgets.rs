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

#[derive(Clone, Copy, PartialEq)]
pub enum Glyph {
    Play,
    Pause,
    Back,
    Forward,
}

fn triangle(painter: &egui::Painter, tip: egui::Pos2, base: f32, half: f32, color: Color32) {
    painter.add(egui::Shape::convex_polygon(vec![tip, egui::pos2(base, tip.y + half), egui::pos2(base, tip.y - half)], color, egui::Stroke::NONE));
}

pub fn transport(ui: &mut egui::Ui, glyph: Glyph, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(size + 12.0, size + 8.0), Sense::click());
    let (hover, press) = motion(ui, &response);
    let color = mix(Color32::from_gray(225), Color32::WHITE, hover);
    let center = rect.center();
    let half = size / 2.0 - press;
    let painter = ui.painter();
    match glyph {
        Glyph::Play => {
            let mut points = vec![egui::pos2(center.x - half * 0.75, center.y - half), egui::pos2(center.x + half, center.y), egui::pos2(center.x - half * 0.75, center.y + half)];
            points.reverse();
            painter.add(egui::Shape::convex_polygon(points, color, egui::Stroke::NONE));
        }
        Glyph::Pause => {
            for side in [-1.0, 1.0] {
                let bar = egui::Rect::from_center_size(center + Vec2::new(side * half * 0.5, 0.0), Vec2::new(half * 0.55, half * 2.0));
                painter.rect_filled(bar, 1.0_f32, color);
            }
        }
        Glyph::Back => {
            triangle(painter, egui::pos2(center.x - half, center.y), center.x, half * 0.72, color);
            triangle(painter, egui::pos2(center.x, center.y), center.x + half, half * 0.72, color);
        }
        Glyph::Forward => {
            triangle(painter, egui::pos2(center.x + half, center.y), center.x, half * 0.72, color);
            triangle(painter, egui::pos2(center.x, center.y), center.x - half, half * 0.72, color);
        }
    }
    response
}

#[derive(Clone, Copy)]
pub enum Caption {
    Minimize,
    Maximize,
    Restore,
    Close,
}

pub fn caption(ui: &mut egui::Ui, kind: Caption) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(46.0, crate::TOP_BAR_HEIGHT), Sense::click());
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    let text = ui.visuals().text_color();
    let fill = match (kind, hovered, pressed) {
        (Caption::Close, true, false) => Color32::from_rgb(196, 43, 28),
        (Caption::Close, true, true) => Color32::from_rgb(200, 70, 58),
        (_, true, false) => text.gamma_multiply(0.09),
        (_, true, true) => text.gamma_multiply(0.05),
        _ => Color32::TRANSPARENT,
    };
    ui.painter().rect_filled(rect, 0.0_f32, fill);
    let color = if matches!(kind, Caption::Close) && hovered { Color32::WHITE } else { text };
    let stroke = egui::Stroke::new(1.0_f32, color);
    let center = rect.center();
    let half = 5.0;
    let box_at = |offset: Vec2| egui::Rect::from_center_size(center + offset, Vec2::splat(half * 2.0));
    let painter = ui.painter();
    match kind {
        Caption::Minimize => {
            painter.line_segment([center - Vec2::new(half, 0.0), center + Vec2::new(half, 0.0)], stroke);
        }
        Caption::Maximize => {
            painter.rect_stroke(box_at(Vec2::ZERO), 1.5_f32, stroke, egui::StrokeKind::Middle);
        }
        Caption::Restore => {
            let back = box_at(Vec2::new(1.5, -1.5)).shrink(1.0);
            painter.line_segment([back.left_top(), back.right_top()], stroke);
            painter.line_segment([back.right_top(), back.right_bottom()], stroke);
            painter.rect_stroke(box_at(Vec2::new(-1.0, 1.0)).shrink(1.0), 1.5_f32, stroke, egui::StrokeKind::Middle);
        }
        Caption::Close => {
            painter.line_segment([center - Vec2::splat(half), center + Vec2::splat(half)], stroke);
            painter.line_segment([center + Vec2::new(-half, half), center + Vec2::new(half, -half)], stroke);
        }
    }
    response
}

pub fn toggle(ui: &mut egui::Ui, value: &mut bool, accent: Color32) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(40.0, 20.0), Sense::click());
    if response.clicked() {
        *value = !*value;
    }
    let on = ui.ctx().animate_bool_with_time(response.id.with("on"), *value, 0.12);
    let text = ui.visuals().text_color();
    ui.painter().rect_filled(rect, 10.0_f32, mix(Color32::from_gray(60), accent, on));
    if on < 0.5 {
        ui.painter().rect_stroke(rect, 10.0_f32, egui::Stroke::new(1.0_f32, Color32::from_gray(130)), egui::StrokeKind::Inside);
    }
    let x = rect.left() + 10.0 + on * 20.0;
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 6.0, mix(text, Color32::WHITE, on));
    response.clicked()
}

pub fn icon_toggle(ui: &mut egui::Ui, icon: &str, tip: &str, active: bool, accent: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(30.0, 28.0), Sense::click());
    let (hover, press) = motion(ui, &response);
    let on = ui.ctx().animate_bool_with_time(response.id.with("on"), active, 0.12);
    let shape = rect.shrink(press);
    ui.painter().rect_filled(shape, 5.0_f32, Color32::WHITE.gamma_multiply(0.08 * hover));
    ui.painter().rect_filled(shape, 5.0_f32, accent.gamma_multiply(0.28 * on));
    paint_icon(ui.painter(), rect.center(), 17.0, icon, mix(mix(IDLE, Color32::WHITE, hover), Color32::WHITE, on));
    response.on_hover_text(tip)
}

pub fn segment<T: Copy + PartialEq>(ui: &mut egui::Ui, items: &[(T, &str, &str)], current: T, accent: Color32) -> Option<T> {
    let mut picked = None;
    egui::Frame::new().fill(Color32::WHITE.gamma_multiply(0.05)).corner_radius(7).inner_margin(2).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.horizontal(|ui| {
            let slot = ui.painter().add(egui::Shape::Noop);
            let mut target = None;
            for (value, icon, label) in items {
                let active = *value == current;
                let color = if active { Color32::WHITE } else { IDLE };
                let words = ui.painter().layout_no_wrap((*label).to_string(), egui::FontId::proportional(13.0), color);
                let (rect, response) = ui.allocate_exact_size(Vec2::new(words.size().x + 38.0, 24.0), Sense::click());
                let (hover, _) = motion(ui, &response);
                if active {
                    target = Some(rect);
                } else {
                    ui.painter().rect_filled(rect, 5.0_f32, Color32::WHITE.gamma_multiply(0.06 * hover));
                }
                paint_icon(ui.painter(), rect.left_center() + Vec2::new(15.0, 0.0), 15.0, icon, color);
                let height = words.size().y;
                ui.painter().galley(egui::pos2(rect.left() + 28.0, rect.center().y - height / 2.0), words, color);
                if response.clicked() {
                    picked = Some(*value);
                }
            }
            if let Some(rect) = target {
                let context = ui.ctx().clone();
                let left = context.animate_value_with_time(ui.id().with("slide-left"), rect.left(), 0.18);
                let right = context.animate_value_with_time(ui.id().with("slide-right"), rect.right(), 0.18);
                let sliding = egui::Rect::from_min_max(egui::pos2(left, rect.top()), egui::pos2(right, rect.bottom()));
                ui.painter().set(slot, egui::Shape::rect_filled(sliding, 5.0_f32, accent));
            }
        });
    });
    picked
}

pub fn setting(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(30.0);
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
    });
}

pub fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).strong());
    ui.add_space(3.0);
    egui::Frame::new().fill(Color32::WHITE.gamma_multiply(0.035)).stroke(egui::Stroke::new(1.0_f32, Color32::WHITE.gamma_multiply(0.06))).corner_radius(6).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

pub fn accent_button(ui: &mut egui::Ui, label: &str, accent: Color32) -> Response {
    ui.add(egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(accent).min_size(Vec2::new(78.0, 26.0)))
}

pub fn plain_button(ui: &mut egui::Ui, label: &str) -> Response {
    ui.add(egui::Button::new(label).min_size(Vec2::new(78.0, 26.0)))
}

pub fn format_card(ui: &mut egui::Ui, name: &str, width: u32, height: u32, active: bool, accent: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(98.0, 88.0), Sense::click());
    let (hover, _) = motion(ui, &response);
    let text = ui.visuals().text_color();
    ui.painter().rect_filled(rect, 6.0_f32, Color32::WHITE.gamma_multiply(0.035 + hover * 0.05));
    let border = if active { egui::Stroke::new(2.0_f32, accent) } else { egui::Stroke::new(1.0_f32, Color32::WHITE.gamma_multiply(0.07)) };
    ui.painter().rect_stroke(rect, 6.0_f32, border, egui::StrokeKind::Inside);
    let scale = 34.0 / width.max(height).max(1) as f32;
    let shape = egui::Rect::from_center_size(rect.center_top() + Vec2::new(0.0, 28.0), Vec2::new(width as f32 * scale, height as f32 * scale));
    ui.painter().rect_stroke(shape, 2.0_f32, egui::Stroke::new(1.5_f32, if active { accent } else { Color32::GRAY }), egui::StrokeKind::Middle);
    ui.painter().text(rect.center_bottom() - Vec2::new(0.0, 25.0), egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(12.0), text);
    ui.painter().text(rect.center_bottom() - Vec2::new(0.0, 11.0), egui::Align2::CENTER_CENTER, format!("{width} x {height}"), egui::FontId::proportional(10.0), Color32::GRAY);
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

