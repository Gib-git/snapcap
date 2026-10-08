//! Region selection overlay. Shows a frozen capture of every monitor in a borderless
//! fullscreen window, lets the user drag out a rectangle, and returns it in image pixels.

use eframe::egui::{
    self, Align2, Color32, ColorImage, CornerRadius, CursorIcon, FontId, Id, Key, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, ViewportBuilder, ViewportCommand, ViewportId,
};

use super::theme::{self, CORAL};
use crate::capture::{PxRect, Shot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Screenshot,
    Record,
}

#[derive(Debug, Clone, Copy)]
pub enum Outcome {
    Cancel,
    /// Monitor index into the shots and the chosen rectangle in that image's pixels.
    Confirm { shot: usize, rect: PxRect, full: bool },
}

#[derive(Clone, Copy)]
enum Drag {
    Create { origin: Pos2 },
    Move { grab: egui::Vec2 },
    Resize { left: bool, right: bool, top: bool, bottom: bool, anchor: Rect },
}

pub struct Selector {
    pub purpose: Purpose,
    pub shots: Vec<Shot>,
    textures: Vec<Option<TextureHandle>>,
    /// (shot index, rectangle in image pixels)
    sel: Option<(usize, Rect)>,
    drag: Option<(usize, Drag)>,
    frames: u32,
    focus_index: usize,
}

const HANDLE: f32 = 9.0;
const MIN_PX: f32 = 8.0;

fn title(i: usize) -> String {
    format!("SnapCap Selection {}", i + 1)
}

impl Selector {
    pub fn new(purpose: Purpose, shots: Vec<Shot>, focus_index: usize) -> Self {
        let n = shots.len();
        Self { purpose, shots, textures: vec![None; n], sel: None, drag: None, frames: 0, focus_index }
    }

    /// Renders one overlay window per monitor; returns the outcome once the user decides.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        self.frames += 1;
        let mut outcome = None;
        for i in 0..self.shots.len() {
            let m = &self.shots[i].monitor;
            let mut vb = ViewportBuilder::default()
                .with_title(title(i))
                .with_decorations(false)
                .with_resizable(false)
                .with_taskbar(false)
                .with_always_on_top()
                .with_active(i == self.focus_index);
            let (x, y) = crate::ui::desktop_to_points(m.x as f32, m.y as f32);
            let (w, h) = crate::ui::desktop_to_points(m.width as f32, m.height as f32);
            vb = vb.with_position([x, y]).with_inner_size([w, h]);
            if !cfg!(target_os = "macos") {
                // Borderless fullscreen on the matching monitor (instant on Windows/X11/Wayland).
                // macOS would animate into a new Space, so there we position a borderless window instead.
                vb = vb.with_monitor(i).with_fullscreen(true);
            }
            let id = ViewportId::from_hash_of(("snapcap-overlay", i));
            let r = ctx.show_viewport_immediate(id, vb, |ui, _| self.monitor_ui(ui, i));
            if r.is_some() {
                outcome = r;
            }
        }
        if self.frames <= 4 {
            for (i, shot) in self.shots.iter().enumerate() {
                let m = &shot.monitor;
                crate::platform::raise_overlay(&title(i), (m.x as f64, m.y as f64, m.width as f64, m.height as f64));
            }
            let id = ViewportId::from_hash_of(("snapcap-overlay", self.focus_index));
            ctx.send_viewport_cmd_to(id, ViewportCommand::Focus);
        }
        outcome
    }

    fn texture(&mut self, ctx: &egui::Context, i: usize) -> TextureHandle {
        self.textures[i]
            .get_or_insert_with(|| {
                let img = &self.shots[i].image;
                let color = ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                ctx.load_texture(format!("snapcap-overlay-{i}"), color, TextureOptions::LINEAR)
            })
            .clone()
    }

    fn full_rect(&self, i: usize) -> Rect {
        let img = &self.shots[i].image;
        Rect::from_min_size(Pos2::ZERO, egui::vec2(img.width() as f32, img.height() as f32))
    }

    fn confirm(&self, i: usize, r: Rect) -> Outcome {
        let full = self.full_rect(i);
        let r = r.intersect(full);
        let is_full = (r.width() - full.width()).abs() < 1.0 && (r.height() - full.height()).abs() < 1.0;
        Outcome::Confirm {
            shot: i,
            rect: PxRect {
                x: r.min.x.round() as u32,
                y: r.min.y.round() as u32,
                w: (r.width().round() as u32).max(1),
                h: (r.height().round() as u32).max(1),
            },
            full: is_full,
        }
    }

    fn monitor_ui(&mut self, ui: &mut Ui, i: usize) -> Option<Outcome> {
        let ctx = ui.ctx().clone();
        let tex = self.texture(&ctx, i);
        let screen = ui.max_rect();
        let full = self.full_rect(i);
        let s = full.width() / screen.width().max(1.0);
        let to_px = |p: Pos2| ((p - screen.min) * s).to_pos2().clamp(full.min, full.max);
        let to_pt = |r: Rect| Rect::from_min_max(screen.min + r.min.to_vec2() / s, screen.min + r.max.to_vec2() / s);
        let painter = ui.painter().clone();
        let p = theme::palette(true);

        painter.image(tex.id(), screen, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);

        let resp = ui.interact(screen, Id::new(("snapcap-overlay-area", i)), Sense::click_and_drag());
        let hover = ctx.pointer_hover_pos().filter(|p| screen.contains(*p));
        let sel_here = self.sel.filter(|(m, _)| *m == i).map(|(_, r)| r);

        // ---- input ----
        let mut outcome = None;
        if ctx.input(|inp| inp.key_pressed(Key::Escape)) {
            return Some(Outcome::Cancel);
        }
        if ctx.input(|inp| inp.key_pressed(Key::Enter)) {
            return Some(match self.sel {
                Some((m, r)) => self.confirm(m, r),
                None => self.confirm(i, full),
            });
        }

        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ctx.input(|inp| inp.pointer.press_origin()).unwrap_or(screen.center());
            let px = to_px(origin);
            let drag = match (self.purpose, sel_here) {
                (Purpose::Record, Some(r)) => match handle_at(to_pt(r), origin) {
                    Some((left, right, top, bottom)) => Drag::Resize { left, right, top, bottom, anchor: r },
                    None if r.contains(px) => Drag::Move { grab: px - r.min },
                    None => Drag::Create { origin: px },
                },
                _ => Drag::Create { origin: px },
            };
            self.drag = Some((i, drag));
        }
        if let (Some((di, drag)), Some(pos)) = (self.drag, ctx.pointer_interact_pos()) {
            if di == i {
                let px = to_px(pos);
                let r = match drag {
                    Drag::Create { origin } => Rect::from_two_pos(origin, px),
                    Drag::Move { grab } => {
                        let size = sel_here.map(|r| r.size()).unwrap_or_default();
                        let min = (px - grab).clamp(full.min, full.max - size);
                        Rect::from_min_size(min, size)
                    }
                    Drag::Resize { left, right, top, bottom, anchor } => {
                        let mut r = anchor;
                        if left {
                            r.min.x = px.x.min(anchor.max.x - MIN_PX);
                        }
                        if right {
                            r.max.x = px.x.max(anchor.min.x + MIN_PX);
                        }
                        if top {
                            r.min.y = px.y.min(anchor.max.y - MIN_PX);
                        }
                        if bottom {
                            r.max.y = px.y.max(anchor.min.y + MIN_PX);
                        }
                        r
                    }
                };
                self.sel = Some((i, r));
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
            if let Some((m, r)) = self.sel {
                if r.width() < MIN_PX || r.height() < MIN_PX {
                    self.sel = None;
                } else if self.purpose == Purpose::Screenshot && m == i {
                    outcome = Some(self.confirm(m, r));
                }
            }
        }
        if resp.clicked() && self.drag.is_none() {
            match self.purpose {
                // A plain click captures the whole monitor.
                Purpose::Screenshot => outcome = Some(self.confirm(i, full)),
                Purpose::Record => {
                    if sel_here.is_none() {
                        self.sel = Some((i, full));
                    }
                }
            }
        }
        if resp.double_clicked() && self.purpose == Purpose::Record {
            if let Some((m, r)) = self.sel {
                outcome = Some(self.confirm(m, r));
            }
        }

        // ---- drawing ----
        let sel_here = self.sel.filter(|(m, _)| *m == i).map(|(_, r)| r);
        let dim = Color32::from_black_alpha(115);
        match sel_here {
            Some(r) => {
                let r = to_pt(r);
                for part in [
                    Rect::from_min_max(screen.min, Pos2::new(screen.max.x, r.min.y)),
                    Rect::from_min_max(Pos2::new(screen.min.x, r.max.y), screen.max),
                    Rect::from_min_max(Pos2::new(screen.min.x, r.min.y), Pos2::new(r.min.x, r.max.y)),
                    Rect::from_min_max(Pos2::new(r.max.x, r.min.y), Pos2::new(screen.max.x, r.max.y)),
                ] {
                    painter.rect_filled(part, 0.0, dim);
                }
                painter.rect_stroke(r, 0.0, Stroke::new(1.5, p.accent), StrokeKind::Outside);
                if self.purpose == Purpose::Record && self.drag.is_none() {
                    for h in handles(r) {
                        painter.rect_filled(h, CornerRadius::same(2), Color32::WHITE);
                        painter.rect_stroke(h, CornerRadius::same(2), Stroke::new(1.0, p.accent), StrokeKind::Inside);
                    }
                }
                let px = self.sel.unwrap().1;
                let label = format!("{} × {}", px.width().round() as u32, px.height().round() as u32);
                let pos = if r.top() > screen.top() + 34.0 { r.left_top() + egui::vec2(0.0, -8.0) } else { r.left_top() + egui::vec2(8.0, 30.0) };
                pill(&painter, pos, Align2::LEFT_BOTTOM, &label, 12.5);
            }
            None => {
                painter.rect_filled(screen, 0.0, dim.gamma_multiply(0.75));
            }
        }

        // Crosshair guides and magnifier while choosing.
        let choosing = self.drag.map(|(_, d)| matches!(d, Drag::Create { .. })).unwrap_or(sel_here.is_none());
        if let (Some(hp), true) = (hover, choosing) {
            let guide = Stroke::new(1.0, Color32::from_white_alpha(70));
            painter.line_segment([Pos2::new(screen.left(), hp.y), Pos2::new(screen.right(), hp.y)], guide);
            painter.line_segment([Pos2::new(hp.x, screen.top()), Pos2::new(hp.x, screen.bottom())], guide);
            self.loupe(&painter, &tex, screen, hp, to_px(hp), full);
        }

        // Hint at the top.
        if self.drag.is_none() && (sel_here.is_none() || self.purpose == Purpose::Screenshot) {
            let hint = match self.purpose {
                Purpose::Screenshot => "Drag to capture an area   ·   Click to capture this screen   ·   Esc to cancel",
                Purpose::Record => "Drag to choose the recording area   ·   Click for the whole screen   ·   Esc to cancel",
            };
            pill(&painter, Pos2::new(screen.center().x, screen.top() + 44.0), Align2::CENTER_CENTER, hint, 13.5);
        }

        // Cursor.
        let cursor = match (self.drag, hover, sel_here) {
            (Some((_, Drag::Move { .. })), _, _) => CursorIcon::Grabbing,
            (None, Some(hp), Some(r)) if self.purpose == Purpose::Record => match handle_at(to_pt(r), hp) {
                Some((l, rt, t, b)) => resize_cursor(l, rt, t, b),
                None if to_pt(r).contains(hp) => CursorIcon::Grab,
                None => CursorIcon::Crosshair,
            },
            _ => CursorIcon::Crosshair,
        };
        ctx.set_cursor_icon(cursor);

        // Toolbar for confirming a recording area.
        if let (Purpose::Record, Some(r), None) = (self.purpose, sel_here, self.drag) {
            if let Some(o) = self.record_toolbar(&ctx, i, to_pt(r), screen) {
                outcome = Some(o);
            }
        }

        if self.drag.is_some() || hover.is_some() {
            ctx.request_repaint();
        }
        outcome
    }

    fn loupe(&self, painter: &egui::Painter, tex: &TextureHandle, screen: Rect, at: Pos2, px: Pos2, full: Rect) {
        let size = 116.0;
        let span = 15.0; // image pixels shown across
        let mut pos = at + egui::vec2(22.0, 22.0);
        if pos.x + size > screen.right() - 8.0 {
            pos.x = at.x - 22.0 - size;
        }
        if pos.y + size + 28.0 > screen.bottom() - 8.0 {
            pos.y = at.y - 22.0 - size - 28.0;
        }
        let r = Rect::from_min_size(pos, egui::vec2(size, size));
        let uv = Rect::from_center_size(
            Pos2::new((px.x.floor() + 0.5) / full.width(), (px.y.floor() + 0.5) / full.height()),
            egui::vec2(span / full.width(), span / full.height()),
        );
        painter.rect_filled(r.expand(3.0), CornerRadius::same(8), Color32::from_black_alpha(200));
        painter.image(tex.id(), r, uv, Color32::WHITE);
        let cell = size / span;
        let c = Rect::from_center_size(r.center(), egui::vec2(cell, cell));
        painter.rect_stroke(c, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Outside);
        painter.rect_stroke(r, CornerRadius::same(4), Stroke::new(1.5, Color32::from_white_alpha(200)), StrokeKind::Outside);
        pill(painter, Pos2::new(r.center().x, r.bottom() + 16.0), Align2::CENTER_CENTER, &format!("{}, {}", px.x as u32, px.y as u32), 11.5);
    }

    fn record_toolbar(&mut self, ctx: &egui::Context, i: usize, r: Rect, screen: Rect) -> Option<Outcome> {
        let width = 300.0;
        let height = 46.0;
        let mut pos = Pos2::new(r.center().x - width / 2.0, r.bottom() + 12.0);
        if pos.y + height > screen.bottom() - 12.0 {
            pos.y = r.top() - 12.0 - height;
            if pos.y < screen.top() + 12.0 {
                pos.y = r.bottom() - height - 16.0;
            }
        }
        pos.x = pos.x.clamp(screen.left() + 12.0, screen.right() - width - 12.0);
        let mut out = None;
        egui::Area::new(Id::new(("snapcap-record-toolbar", i)))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(24, 25, 31, 245))
                    .corner_radius(CornerRadius::same(23))
                    .stroke(Stroke::new(1.0, Color32::from_white_alpha(25)))
                    .inner_margin(egui::Margin::symmetric(8, 7))
                    .show(ui, |ui| {
                        ui.set_min_width(width - 16.0);
                        ui.horizontal(|ui| {
                            ui.visuals_mut().override_text_color = Some(Color32::WHITE);
                            let rec = egui::Button::new(
                                RichText::new(format!("{}  Start recording", egui_phosphor::regular::RECORD)).color(Color32::WHITE).strong(),
                            )
                            .fill(CORAL)
                            .corner_radius(CornerRadius::same(16))
                            .min_size(egui::vec2(0.0, 32.0));
                            if ui.add(rec).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                if let Some((m, sr)) = self.sel {
                                    out = Some(self.confirm(m, sr));
                                }
                            }
                            let full = egui::Button::new(RichText::new(egui_phosphor::regular::CORNERS_OUT).size(16.0))
                                .fill(Color32::from_white_alpha(18))
                                .corner_radius(CornerRadius::same(16))
                                .min_size(egui::vec2(36.0, 32.0));
                            if ui.add(full).on_hover_text("Whole screen").clicked() {
                                self.sel = Some((i, self.full_rect(i)));
                            }
                            let cancel = egui::Button::new(RichText::new("Cancel"))
                                .fill(Color32::from_white_alpha(18))
                                .corner_radius(CornerRadius::same(16))
                                .min_size(egui::vec2(0.0, 32.0));
                            if ui.add(cancel).clicked() {
                                out = Some(Outcome::Cancel);
                            }
                        });
                    });
            });
        out
    }
}

fn handles(r: Rect) -> Vec<Rect> {
    let pts = [
        r.left_top(),
        r.center_top(),
        r.right_top(),
        r.left_center(),
        r.right_center(),
        r.left_bottom(),
        r.center_bottom(),
        r.right_bottom(),
    ];
    pts.iter().map(|p| Rect::from_center_size(*p, egui::vec2(HANDLE, HANDLE))).collect()
}

/// Which edges a pointer at `p` would resize, if it is on a handle or edge.
fn handle_at(r: Rect, p: Pos2) -> Option<(bool, bool, bool, bool)> {
    let tol = 8.0;
    if !r.expand(tol).contains(p) {
        return None;
    }
    let left = (p.x - r.left()).abs() <= tol;
    let right = (p.x - r.right()).abs() <= tol;
    let top = (p.y - r.top()).abs() <= tol;
    let bottom = (p.y - r.bottom()).abs() <= tol;
    (left || right || top || bottom).then_some((left, right && !left, top, bottom && !top))
}

fn resize_cursor(l: bool, r: bool, t: bool, b: bool) -> CursorIcon {
    match (l || r, t || b) {
        (true, true) if (l && t) || (r && b) => CursorIcon::ResizeNwSe,
        (true, true) => CursorIcon::ResizeNeSw,
        (true, false) => CursorIcon::ResizeHorizontal,
        _ => CursorIcon::ResizeVertical,
    }
}

fn pill(painter: &egui::Painter, pos: Pos2, align: Align2, text: &str, size: f32) {
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(size), Color32::WHITE);
    let r = align.anchor_size(pos, galley.size() + egui::vec2(20.0, 10.0));
    painter.rect_filled(r, CornerRadius::same(((r.height() / 2.0) as u8).max(1)), Color32::from_rgba_unmultiplied(20, 21, 27, 225));
    painter.galley(r.center() - galley.size() / 2.0, galley, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_detect_corners_and_edges() {
        let r = Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(300.0, 200.0));
        assert_eq!(handle_at(r, Pos2::new(101.0, 99.0)), Some((true, false, true, false)));
        assert_eq!(handle_at(r, Pos2::new(200.0, 201.0)), Some((false, false, false, true)));
        assert_eq!(handle_at(r, Pos2::new(200.0, 150.0)), None);
        assert_eq!(handle_at(r, Pos2::new(10.0, 10.0)), None);
    }
}
