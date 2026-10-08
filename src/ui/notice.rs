//! Floating capture notice with a thumbnail, shown in a corner after each capture.
//! Like the macOS screenshot thumbnail, it can be dragged onto any app, folder,
//! browser page or chat to drop the file there.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle,
    ViewportBuilder, ViewportCommand, ViewportId,
};

use super::theme;
use crate::platform::DragEnd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Success,
    Error,
}

pub struct Notice {
    pub kind: NoticeKind,
    pub title: String,
    pub detail: String,
    pub path: Option<PathBuf>,
    pub thumb: Option<TextureHandle>,
    pub is_video: bool,
    /// Top-left of the notice window, in points.
    pub pos: Pos2,
    pub until: Instant,
    frames: u32,
    /// An OS drag of the file is in progress.
    dragging: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeAction {
    Open,
    Reveal,
    Dismiss,
}

pub const SIZE: egui::Vec2 = egui::vec2(340.0, 84.0);
const TITLE: &str = "SnapCap Notice";

impl Notice {
    pub fn new(kind: NoticeKind, title: impl Into<String>, detail: impl Into<String>, pos: Pos2) -> Self {
        let secs = if kind == NoticeKind::Error { 7 } else { 5 };
        Self {
            kind,
            title: title.into(),
            detail: detail.into(),
            path: None,
            thumb: None,
            is_video: false,
            pos,
            until: Instant::now() + Duration::from_secs(secs),
            frames: 0,
            dragging: false,
        }
    }

    /// Returns `Some(Dismiss)` once the notice expires.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<NoticeAction> {
        self.frames += 1;
        let vb = ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_active(false)
            .with_position(self.pos)
            .with_inner_size(SIZE);
        let id = ViewportId::from_hash_of("snapcap-notice");
        let out = ctx.show_viewport_immediate(id, vb, |ui, _| self.ui(ui));
        if self.dragging {
            match crate::platform::take_drag_end() {
                Some(DragEnd::Dropped) => return Some(NoticeAction::Dismiss),
                Some(DragEnd::Cancelled) => {
                    self.dragging = false;
                    self.until = Instant::now() + Duration::from_secs(3);
                }
                // Closing the window mid-drag would end the drag.
                None => self.until = self.until.max(Instant::now() + Duration::from_secs(1)),
            }
        }
        if self.frames <= 3 {
            ctx.send_viewport_cmd_to(id, ViewportCommand::ContentProtected(super::protect_from_capture()));
            crate::platform::float_window(TITLE);
        }
        if out.is_none() && Instant::now() >= self.until {
            return Some(NoticeAction::Dismiss);
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        out
    }

    fn ui(&mut self, ui: &mut egui::Ui) -> Option<NoticeAction> {
        let p = theme::current(ui.ctx());
        let full = ui.max_rect();
        let painter = ui.painter().clone();
        let card = full.shrink(2.0);
        painter.rect_filled(card, CornerRadius::same(14), p.surface);
        painter.rect_stroke(card, CornerRadius::same(14), Stroke::new(1.0, p.border), StrokeKind::Inside);

        let sense = if self.path.is_some() { Sense::click_and_drag() } else { Sense::click() };
        let resp = ui.interact(card, ui.id().with("notice"), sense);
        if resp.hovered() {
            // Keep it on screen while the user is looking at it.
            self.until = self.until.max(Instant::now() + Duration::from_secs(2));
        }
        if resp.drag_started() {
            if let Some(path) = self.path.as_deref().filter(|p| p.exists()) {
                self.dragging = crate::platform::start_file_drag(TITLE, path);
            }
        }

        let thumb_rect = Rect::from_min_size(card.min + egui::vec2(12.0, 12.0), egui::vec2(88.0, card.height() - 24.0));
        let accent = if self.kind == NoticeKind::Error { p.danger } else { p.accent };
        match &self.thumb {
            Some(t) => {
                let ts = t.size_vec2();
                let fit = (thumb_rect.width() / ts.x).min(thumb_rect.height() / ts.y);
                let r = Rect::from_center_size(thumb_rect.center(), ts * fit);
                painter.rect_filled(thumb_rect, CornerRadius::same(6), p.bg);
                painter.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                painter.rect_filled(thumb_rect, CornerRadius::same(8), accent.gamma_multiply(0.14));
                let icon = match (self.kind, self.is_video) {
                    (NoticeKind::Error, _) => egui_phosphor::regular::WARNING,
                    (_, true) => egui_phosphor::regular::FILM_STRIP,
                    _ => egui_phosphor::regular::IMAGE,
                };
                painter.text(thumb_rect.center(), Align2::CENTER_CENTER, icon, FontId::proportional(26.0), accent);
            }
        }

        let tx = thumb_rect.right() + 12.0;
        let max_w = card.right() - tx - 12.0;
        painter.text(Pos2::new(tx, card.top() + 24.0), Align2::LEFT_CENTER, &self.title, FontId::proportional(14.5), p.text);
        let detail = painter.layout(self.detail.clone(), FontId::proportional(12.0), p.text_dim, max_w);
        painter.galley(Pos2::new(tx, card.top() + 36.0), detail, p.text_dim);

        let mut action = None;
        if self.path.is_some() && resp.hovered() {
            let reveal = Rect::from_min_size(Pos2::new(card.right() - 112.0, card.bottom() - 30.0), egui::vec2(100.0, 22.0));
            let r = ui.interact(reveal, ui.id().with("reveal"), Sense::click());
            painter.text(reveal.right_center(), Align2::RIGHT_CENTER, "Show in folder", FontId::proportional(12.0), if r.hovered() { p.accent } else { p.text_dim });
            if r.clicked() {
                action = Some(NoticeAction::Reveal);
            }
        }
        if action.is_none() && resp.clicked() {
            action = Some(if self.path.is_some() { NoticeAction::Open } else { NoticeAction::Dismiss });
        }
        if action.is_some() || resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        action
    }
}
