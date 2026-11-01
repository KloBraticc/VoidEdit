use eframe::egui::{self, Color32, Rect, Response, RichText, Sense, Vec2};

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

pub fn pill(ui: &mut egui::Ui, icon: &str, label: &str, active: bool, accent: Color32) -> Response {
    let text = if label.is_empty() { icon.to_string() } else { format!("{icon}  {label}") };
    let idle = ui.visuals().text_color();
    let galley = ui.painter().layout_no_wrap(text, egui::FontId::proportional(14.0), Color32::PLACEHOLDER);
    let size = Vec2::new((galley.size().x + 20.0).max(32.0), 32.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let (hover, press) = motion(ui, &response);
    let on = ui.ctx().animate_bool_with_time(response.id.with("on"), active, 0.18);
    let shape = rect.shrink(press * 1.5);
    let fill = accent.gamma_multiply(on * 0.9 + (1.0 - on) * hover * 0.28);
    ui.painter().rect_filled(shape, 9.0_f32, fill);
    let color = mix(idle, Color32::WHITE, on);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    response
}

pub fn primary(ui: &mut egui::Ui, icon: &str, label: &str, accent: Color32) -> Response {
    let galley = ui.painter().layout_no_wrap(format!("{icon}  {label}"), egui::FontId::proportional(14.0), Color32::WHITE);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(galley.size().x + 30.0, 32.0), Sense::click());
    let (hover, press) = motion(ui, &response);
    ui.painter().rect_filled(rect.expand(hover * 3.0), 12.0_f32, accent.gamma_multiply(0.25 * hover));
    ui.painter().rect_filled(rect.shrink(press * 1.5), 9.0_f32, mix(accent, Color32::WHITE, hover * 0.12));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
    response
}

pub fn round(ui: &mut egui::Ui, icon: &str, accent: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(42.0), Sense::click());
    let (hover, press) = motion(ui, &response);
    ui.painter().circle_filled(rect.center(), 21.0 + hover * 3.0, accent.gamma_multiply(0.25 * hover));
    ui.painter().circle_filled(rect.center(), 20.0 - press * 2.0, mix(accent, Color32::WHITE, hover * 0.12));
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(19.0), Color32::WHITE);
    response
}

pub fn rail<T: Copy + PartialEq>(ui: &mut egui::Ui, tabs: &[(T, &str, &str)], current: &mut T, accent: Color32) -> bool {
    let mut changed = false;
    let idle = ui.visuals().text_color();
    let slot = ui.painter().add(egui::Shape::Noop);
    let mut target = None;
    let spacing = std::mem::replace(&mut ui.spacing_mut().item_spacing.y, 4.0);
    for (tab, icon, label) in tabs {
        let (rect, response) = ui.allocate_exact_size(Vec2::new(66.0, 56.0), Sense::click());
        let (hover, press) = motion(ui, &response);
        let active = *tab == *current;
        if active {
            target = Some(rect);
        }
        if response.clicked() && !active {
            *current = *tab;
            changed = true;
        }
        ui.painter().rect_filled(rect, 12.0_f32, idle.gamma_multiply(0.07 * hover));
        let on = ui.ctx().animate_bool_with_time(response.id.with("on"), active, 0.18);
        let color = mix(idle.gamma_multiply(0.7), Color32::WHITE, on);
        let lift = hover * 1.5 - press;
        ui.painter().text(rect.center_top() + Vec2::new(0.0, 20.0 - lift), egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(21.0), color);
        ui.painter().text(rect.center_top() + Vec2::new(0.0, 42.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(11.0), color);
    }
    ui.spacing_mut().item_spacing.y = spacing;
    if let Some(rect) = target {
        let top = ui.ctx().animate_value_with_time(ui.id().with("rail"), rect.top(), 0.2);
        let pill = Rect::from_min_size(egui::pos2(rect.left(), top), rect.size());
        ui.painter().set(slot, egui::Shape::rect_filled(pill, 12.0_f32, accent.gamma_multiply(0.9)));
    }
    changed
}

pub fn heading(ui: &mut egui::Ui, icon: &str, label: &str, accent: Color32) {
    ui.add_space(6.0);
    ui.label(RichText::new(format!("{icon}  {label}")).strong().color(accent));
    ui.add_space(2.0);
}
