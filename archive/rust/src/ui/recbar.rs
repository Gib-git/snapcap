//! Floating recording controls and the border drawn around a recorded region.
//! Both windows are excluded from capture where the OS allows it.

use std::time::Duration;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, ViewportBuilder,
    ViewportCommand, ViewportId,
};

use super::theme::CORAL;
use super::widgets::format_duration;
use crate::capture::CaptureTarget;

pub enum BarState {
    Countdown { remaining: u32 },
    Recording { elapsed: Duration, paused: bool },
    Finalizing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarAction {
    Stop,
    TogglePause,
    Cancel,
}

const BAR: egui::Vec2 = egui::vec2(236.0, 48.0);
const BAR_TITLE: &str = "SnapCap Recording";
const BORDER_TITLE: &str = "SnapCap Recording Area";
const AMBER: Color32 = Color32::from_rgb(245, 176, 65);

/// Monitor and region rectangles in egui points.
fn rects(target: &CaptureTarget) -> (Rect, Rect) {
    let m = &target.monitor;
    let (mx, my) = super::desktop_to_points(m.x as f32, m.y as f32);
    let (mw, mh) = super::desktop_to_points(m.width as f32, m.height as f32);
    let (rx, ry, rw, rh) = target.desktop_rect();
    let (rx, ry) = super::desktop_to_points(rx, ry);
    let (rw, rh) = super::desktop_to_points(rw, rh);
    (Rect::from_min_size(Pos2::new(mx, my), egui::vec2(mw, mh)), Rect::from_min_size(Pos2::new(rx, ry), egui::vec2(rw, rh)))
}

fn bar_position(target: &CaptureTarget) -> Pos2 {
    let (mon, r) = rects(target);
    let margin = 12.0;
    if target.region.is_none() {
        return Pos2::new(mon.center().x - BAR.x / 2.0, mon.bottom() - BAR.y - 96.0);
    }
    let mut y = r.bottom() + margin;
    if y + BAR.y > mon.bottom() - margin {
        y = r.top() - margin - BAR.y;
        if y < mon.top() + 30.0 {
            y = r.bottom() - BAR.y - 20.0;
        }
    }
    let x = (r.center().x - BAR.x / 2.0).clamp(mon.left() + margin, mon.right() - BAR.x - margin);
    Pos2::new(x, y)
}

pub fn show_bar(ctx: &egui::Context, target: &CaptureTarget, state: &BarState, badge: &str, first_frames: bool) -> Option<BarAction> {
    let vb = ViewportBuilder::default()
        .with_title(BAR_TITLE)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_taskbar(false)
        .with_always_on_top()
        .with_active(false)
        .with_position(bar_position(target))
        .with_inner_size(BAR);
    let id = ViewportId::from_hash_of("snapcap-recbar");
    let out = ctx.show_viewport_immediate(id, vb, |ui, _| bar_ui(ui, state, badge));
    if first_frames {
        ctx.send_viewport_cmd_to(id, ViewportCommand::ContentProtected(super::protect_from_capture()));
        crate::platform::float_window(BAR_TITLE);
    }
    out
}

fn bar_ui(ui: &mut Ui, state: &BarState, badge: &str) -> Option<BarAction> {
    let ctx = ui.ctx().clone();
    let full = ui.max_rect();
    let painter = ui.painter().clone();
    painter.rect_filled(full, CornerRadius::same(24), Color32::from_rgba_unmultiplied(22, 23, 29, 242));
    painter.rect_stroke(full, CornerRadius::same(24), Stroke::new(1.0, Color32::from_white_alpha(28)), StrokeKind::Inside);

    // Drag anywhere on the background to move the bar.
    let bg = ui.interact(full, ui.id().with("bar-drag"), Sense::click_and_drag());
    if bg.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }

    let mut action = None;
    let cy = full.center().y;
    let t = ctx.input(|i| i.time) as f32;
    match state {
        BarState::Countdown { remaining } => {
            painter.text(Pos2::new(full.left() + 22.0, cy), Align2::LEFT_CENTER, format!("Starting in {remaining}…"), FontId::proportional(15.0), Color32::WHITE);
            if round_button(ui, Pos2::new(full.right() - 26.0, cy), egui_phosphor::regular::X, Color32::from_white_alpha(28), "Cancel (Esc)") {
                action = Some(BarAction::Cancel);
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        BarState::Recording { elapsed, paused } => {
            let pulse = if *paused { 1.0 } else { 0.55 + 0.45 * (t * 3.0).sin().abs() };
            let dot_color = if *paused { AMBER } else { CORAL.gamma_multiply(pulse) };
            painter.circle_filled(Pos2::new(full.left() + 24.0, cy), 6.0, dot_color);
            painter.text(Pos2::new(full.left() + 40.0, cy), Align2::LEFT_CENTER, format_duration(*elapsed), FontId::monospace(16.0), Color32::WHITE);
            let label = if *paused { "Paused" } else { badge };
            painter.text(
                Pos2::new(full.left() + 106.0, cy),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(11.0),
                if *paused { AMBER } else { Color32::from_white_alpha(140) },
            );
            let icon = if *paused { egui_phosphor::regular::PLAY } else { egui_phosphor::regular::PAUSE };
            if round_button(ui, Pos2::new(full.right() - 66.0, cy), icon, Color32::from_white_alpha(28), if *paused { "Resume" } else { "Pause" }) {
                action = Some(BarAction::TogglePause);
            }
            if round_button(ui, Pos2::new(full.right() - 26.0, cy), egui_phosphor::regular::STOP, CORAL, "Stop and save") {
                action = Some(BarAction::Stop);
            }
            ctx.request_repaint_after(Duration::from_millis(if *paused { 500 } else { 80 }));
        }
        BarState::Finalizing => {
            painter.text(full.center(), Align2::CENTER_CENTER, "Saving…", FontId::proportional(14.0), Color32::WHITE);
        }
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = Some(match state {
            BarState::Countdown { .. } => BarAction::Cancel,
            _ => BarAction::Stop,
        });
    }
    action
}

fn round_button(ui: &mut Ui, center: Pos2, icon: &str, fill: Color32, tip: &str) -> bool {
    let r = Rect::from_center_size(center, egui::vec2(32.0, 32.0));
    let resp = ui.interact(r, ui.id().with(("bar-btn", icon)), Sense::click());
    let fill = if resp.hovered() { fill.gamma_multiply(1.35) } else { fill };
    ui.painter().circle_filled(center, 16.0, fill);
    ui.painter().text(center, Align2::CENTER_CENTER, icon, FontId::proportional(16.0), Color32::WHITE);
    resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(tip).clicked()
}

/// Thin border just outside the recorded region. Click-through.
pub fn show_border(ctx: &egui::Context, target: &CaptureTarget, paused: bool, first_frames: bool) {
    if target.region.is_none() {
        return;
    }
    let (_, r) = rects(target);
    let pad = 4.0;
    let outer = r.expand(pad);
    let vb = ViewportBuilder::default()
        .with_title(BORDER_TITLE)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_taskbar(false)
        .with_always_on_top()
        .with_active(false)
        .with_mouse_passthrough(true)
        .with_position(outer.min)
        .with_inner_size(outer.size());
    let id = ViewportId::from_hash_of("snapcap-border");
    ctx.show_viewport_immediate(id, vb, |ui, _| {
        let full = ui.max_rect();
        let color = if paused { AMBER } else { CORAL };
        let painter = ui.painter();
        painter.rect_stroke(full.shrink(1.0), CornerRadius::same(3), Stroke::new(2.0, color), StrokeKind::Inside);
        // Corner accents.
        let len = 14.0;
        let w = Stroke::new(4.0, color);
        for (c, dx, dy) in [
            (full.left_top(), 1.0, 1.0),
            (full.right_top(), -1.0, 1.0),
            (full.left_bottom(), 1.0, -1.0),
            (full.right_bottom(), -1.0, -1.0),
        ] {
            let c = c + egui::vec2(dx * 2.0, dy * 2.0);
            painter.line_segment([c, c + egui::vec2(dx * len, 0.0)], w);
            painter.line_segment([c, c + egui::vec2(0.0, dy * len)], w);
        }
    });
    if first_frames {
        ctx.send_viewport_cmd_to(id, ViewportCommand::ContentProtected(super::protect_from_capture()));
        crate::platform::float_window(BORDER_TITLE);
    }
}
