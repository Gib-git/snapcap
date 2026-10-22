//! Small custom widgets shared by the windows.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
};

use super::theme;

/// iOS-style switch.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let p = theme::current(ui.ctx());
    let size = egui::vec2(36.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let off_fill = if ui.visuals().dark_mode { Color32::from_rgb(58, 62, 75) } else { Color32::from_rgb(214, 217, 226) };
    let fill = lerp_color(off_fill, p.accent, t);
    ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
    let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, t);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 8.0, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A labelled row with a switch on the right.
pub fn toggle_row(ui: &mut Ui, label: &str, hint: Option<&str>, on: &mut bool) -> Response {
    let p = theme::current(ui.ctx());
    ui.horizontal(|ui| {
        match hint {
            Some(h) => {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(label);
                    ui.label(RichText::new(h).small().color(p.text_dim));
                });
            }
            None => {
                ui.label(label);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| toggle(ui, on)).inner
    })
    .inner
}

/// Pill-shaped segmented control. Returns true when the value changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let p = theme::current(ui.ctx());
    let mut changed = false;
    let font = FontId::proportional(13.0);
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, l)| ui.fonts_mut(|f| f.layout_no_wrap(l.to_string(), font.clone(), Color32::WHITE).size().x) + 22.0)
        .collect();
    let total = widths.iter().sum::<f32>() + 6.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total, 30.0), Sense::hover());
    let track = if ui.visuals().dark_mode { Color32::from_rgb(14, 15, 19) } else { Color32::from_rgb(232, 234, 241) };
    ui.painter().rect_filled(rect, CornerRadius::same(15), track);
    let mut x = rect.left() + 3.0;
    for ((v, label), w) in options.iter().zip(widths) {
        let r = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 3.0), egui::vec2(w, 24.0));
        let resp = ui.interact(r, ui.id().with(("seg", label)), Sense::click());
        let selected = *value == *v;
        if selected {
            ui.painter().rect_filled(r, CornerRadius::same(12), p.surface);
            ui.painter().rect_stroke(r, CornerRadius::same(12), Stroke::new(1.0, p.border), StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, CornerRadius::same(12), p.accent_soft);
        }
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *label, font.clone(), if selected { p.text } else { p.text_dim });
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !selected {
            *value = *v;
            changed = true;
        }
        x += w;
    }
    changed
}

/// Big clickable card used for the main actions.
pub fn action_card(ui: &mut Ui, size: Vec2, icon: &str, title: &str, shortcut: &str, accent: Color32, enabled: bool) -> Response {
    let p = theme::current(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let hovered = enabled && resp.hovered();
    let t = ui.ctx().animate_bool_with_time(resp.id, hovered, 0.12);
    let fill = lerp_color(p.surface, p.surface_hover, t);
    let stroke = Stroke::new(1.0, lerp_color(p.border, accent.gamma_multiply(0.8), t));
    let painter = ui.painter_at(rect.expand(2.0));
    let lift = if resp.is_pointer_button_down_on() { 0.0 } else { -1.5 * t };
    let r = rect.translate(egui::vec2(0.0, lift));
    painter.rect_filled(r, CornerRadius::same(12), fill);
    painter.rect_stroke(r, CornerRadius::same(12), stroke, StrokeKind::Inside);

    let alpha = if enabled { 1.0 } else { 0.4 };
    let badge = egui::Rect::from_center_size(egui::pos2(r.left() + 30.0, r.top() + 30.0), egui::vec2(34.0, 34.0));
    painter.rect_filled(badge, CornerRadius::same(9), accent.gamma_multiply(0.16 * alpha));
    painter.text(badge.center(), Align2::CENTER_CENTER, icon, FontId::new(19.0, FontFamily::Proportional), accent.gamma_multiply(alpha));
    painter.text(
        egui::pos2(r.left() + 14.0, r.bottom() - 30.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(14.0),
        p.text.gamma_multiply(alpha),
    );
    if !shortcut.is_empty() {
        painter.text(
            egui::pos2(r.left() + 14.0, r.bottom() - 13.0),
            Align2::LEFT_CENTER,
            shortcut,
            FontId::proportional(11.5),
            p.text_dim.gamma_multiply(alpha),
        );
    }
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

pub fn section_label(ui: &mut Ui, text: &str) {
    let p = theme::current(ui.ctx());
    ui.label(RichText::new(text.to_uppercase()).size(11.0).strong().color(p.text_dim).extra_letter_spacing(0.8));
}

/// Card-like grouping container for settings.
pub fn group<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = theme::current(ui.ctx());
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn icon_button(ui: &mut Ui, icon: &str, tooltip: &str) -> Response {
    let p = theme::current(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(8), p.surface_hover);
    }
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon, FontId::proportional(18.0), if resp.hovered() { p.text } else { p.text_dim });
    resp.on_hover_text(tooltip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    let p = theme::current(ui.ctx());
    ui.add(
        egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
            .fill(p.accent)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(8)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

pub fn format_duration(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}
