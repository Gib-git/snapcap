//! The main window: capture actions, quick recording options and recent captures.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense, Stroke, StrokeKind, Ui,
};
use egui_phosphor::regular as icons;

use super::theme::{self, CORAL};
use super::widgets::{self, action_card, section_label, segmented};
use crate::actions::Action;
use crate::app::{App, Mode, Page};
use crate::hotkeys;
use crate::recent::{self, MediaKind};
use crate::settings::VideoFormat;

impl App {
    pub fn main_ui(&mut self, ui: &mut Ui) {
        let p = theme::current(ui.ctx());
        egui::Panel::top("snapcap-header")
            .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 20, right: 14, top: 14, bottom: 8 }))
            .show_separator_line(false)
            .show(ui, |ui| self.header(ui));

        egui::Panel::bottom("snapcap-footer")
            .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::symmetric(20, 10)))
            .show_separator_line(false)
            .show(ui, |ui| self.footer(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 20, right: 20, top: 4, bottom: 4 }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.page {
                    Page::Home => self.home(ui),
                    Page::Settings => self.settings_ui(ui),
                });
            });
    }

    fn header(&mut self, ui: &mut Ui) {
        let p = theme::current(ui.ctx());
        ui.horizontal(|ui| {
            if self.page == Page::Settings {
                if widgets::icon_button(ui, icons::ARROW_LEFT, "Back").clicked() {
                    self.page = Page::Home;
                    self.cancel_shortcut_capture();
                }
                ui.label(RichText::new("Settings").size(19.0).strong());
            } else {
                if let Some(logo) = &self.logo {
                    ui.add(egui::Image::new((logo.id(), egui::vec2(30.0, 30.0))));
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new("SnapCap").size(18.0).strong());
                    ui.label(RichText::new("Screenshots & screen recording").size(11.5).color(p.text_dim));
                });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.page == Page::Home && widgets::icon_button(ui, icons::GEAR_SIX, "Settings").clicked() {
                    self.page = Page::Settings;
                    self.refresh_audio_devices();
                }
                if widgets::icon_button(ui, icons::FOLDER_OPEN, "Open screenshots folder").clicked() {
                    let dir = self.save_dir();
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = open::that_detached(dir);
                }
            });
        });
    }

    fn footer(&mut self, ui: &mut Ui) {
        let p = theme::current(ui.ctx());
        ui.horizontal(|ui| {
            let status = match &self.mode {
                Mode::Recording { .. } => Some((CORAL, "Recording…")),
                Mode::Finalizing { .. } => Some((p.warning, "Saving recording…")),
                Mode::Countdown { .. } => Some((p.warning, "Starting…")),
                _ => None,
            };
            match status {
                Some((c, t)) => {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.0, c);
                    ui.label(RichText::new(t).small().color(p.text_dim));
                }
                None => {
                    ui.label(RichText::new(format!("{}  Saving to", icons::FOLDER_OPEN)).small().color(p.text_dim));
                    let dir = self.save_dir();
                    let shown = ellipsize_middle(&shorten_home(&dir), 56);
                    let link = egui::Link::new(RichText::new(&shown).small());
                    if ui.add(link).on_hover_text(format!("{shown}\nClick to open")).clicked() {
                        let _ = std::fs::create_dir_all(&dir);
                        let _ = open::that_detached(dir);
                    }
                }
            }
        });
    }

    fn home(&mut self, ui: &mut Ui) {
        let p = theme::current(ui.ctx());
        if !self.has_permission {
            permission_banner(ui);
            ui.add_space(4.0);
        }
        let busy = !matches!(self.mode, Mode::Idle);
        let recording = matches!(self.mode, Mode::Recording { .. });

        ui.add_space(6.0);
        section_label(ui, "Screenshot");
        let gap = 10.0;
        let w = ((ui.available_width() - gap * 2.0) / 3.0).floor();
        let card = egui::vec2(w, 112.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (action, icon, title) in [
                (Action::ShotScreen, icons::MONITOR, "Screen"),
                (Action::ShotRegion, icons::SELECTION, "Region"),
                (Action::ShotAllScreens, icons::SQUARES_FOUR, "All screens"),
            ] {
                let sc = hotkeys::display(self.settings.shortcut(action));
                let sc = if self.settings.shortcut(action).is_empty() { String::new() } else { sc };
                if action_card(ui, card, icon, title, &sc, p.accent, !busy).clicked() {
                    self.trigger(ui.ctx(), action);
                }
            }
        });

        ui.add_space(10.0);
        section_label(ui, "Record");
        let w2 = ((ui.available_width() - gap) / 2.0).floor();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (action, icon, title) in [
                (Action::RecordScreen, icons::MONITOR_PLAY, "Record screen"),
                (Action::RecordRegion, icons::SELECTION_ALL, "Record region"),
            ] {
                let sc = hotkeys::display(self.settings.shortcut(action));
                let sc = if self.settings.shortcut(action).is_empty() { String::new() } else { sc };
                if action_card(ui, egui::vec2(w2, 112.0), icon, title, &sc, CORAL, !busy).clicked() {
                    self.trigger(ui.ctx(), action);
                }
            }
        });
        if recording {
            ui.add_space(4.0);
            if widgets::primary_button(ui, &format!("{}  Stop recording", icons::STOP)).clicked() {
                self.trigger(ui.ctx(), Action::StopRecording);
            }
        }

        ui.add_space(4.0);
        widgets::group(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                segmented(ui, &mut self.settings.video_format, &[(VideoFormat::Mp4, "MP4"), (VideoFormat::Gif, "GIF")]);
                let mp4 = self.settings.video_format == VideoFormat::Mp4;
                ui.add_enabled_ui(mp4, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(RichText::new(format!("{}  Mic", icons::MICROPHONE)).color(p.text_dim));
                        widgets::toggle(ui, &mut self.settings.record_mic).on_hover_text("Record microphone");
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(RichText::new(format!("{}  System audio", icons::SPEAKER_HIGH)).color(p.text_dim));
                        ui.add_enabled_ui(self.system_audio_ok, |ui| widgets::toggle(ui, &mut self.settings.record_system_audio))
                            .response
                            .on_hover_text(if self.system_audio_ok { "Record system audio" } else { "System audio capture is not available on this system" });
                    });
                });
                if !mp4 {
                    ui.label(RichText::new("GIFs have no audio").small().color(p.text_dim));
                }
            });
        });

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            section_label(ui, "Recent");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !self.recent.is_empty() && ui.link(RichText::new("Show all").small()).clicked() {
                    let dir = self.save_dir();
                    let _ = open::that_detached(dir);
                }
            });
        });
        self.recent_grid(ui);
    }

    fn recent_grid(&mut self, ui: &mut Ui) {
        let p = theme::current(ui.ctx());
        if self.recent.is_empty() {
            widgets::group(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(14.0);
                    ui.label(RichText::new(icons::IMAGE).size(28.0).color(p.text_dim));
                    ui.label(RichText::new("Your captures will appear here").color(p.text_dim));
                    ui.add_space(14.0);
                });
            });
            return;
        }
        let cols = 4;
        let gap = 10.0;
        let w = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        let size = egui::vec2(w, (w * 0.66).round());
        let mut remove = None;
        egui::Grid::new("snapcap-recent").spacing([gap, gap]).show(ui, |ui| {
            for (i, item) in self.recent.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
                if resp.drag_started() && item.path.exists() {
                    // Drop it onto another app or folder, as from Finder.
                    crate::platform::start_file_drag(crate::app::MAIN_TITLE, &item.path);
                }
                let painter = ui.painter_at(rect);
                let hovered = resp.hovered();
                painter.rect_filled(rect, CornerRadius::same(10), p.surface);
                match &item.thumb {
                    Some(t) => {
                        let ts = t.size_vec2();
                        // Cover-fit crop.
                        let scale = (rect.width() / ts.x).max(rect.height() / ts.y);
                        let shown = egui::vec2(rect.width() / (ts.x * scale), rect.height() / (ts.y * scale));
                        let uv = Rect::from_center_size(egui::pos2(0.5, 0.5), shown);
                        egui::Image::new((t.id(), rect.size()))
                            .uv(uv)
                            .corner_radius(CornerRadius::same(10))
                            .paint_at(ui, rect);
                    }
                    None => {
                        let icon = if item.kind == MediaKind::Mp4 { icons::FILM_STRIP } else { icons::IMAGE };
                        painter.text(rect.center(), Align2::CENTER_CENTER, icon, FontId::proportional(26.0), p.text_dim);
                    }
                }
                let badge_col = match item.kind {
                    MediaKind::Png => p.accent,
                    MediaKind::Mp4 | MediaKind::Gif => CORAL,
                };
                let badge = Rect::from_min_size(rect.min + egui::vec2(6.0, 6.0), egui::vec2(34.0, 17.0));
                painter.rect_filled(badge, CornerRadius::same(5), badge_col.gamma_multiply(0.92));
                painter.text(badge.center(), Align2::CENTER_CENTER, item.kind.label(), FontId::proportional(10.0), Color32::WHITE);
                let stroke = if hovered { Stroke::new(2.0, p.accent) } else { Stroke::new(1.0, p.border) };
                painter.rect_stroke(rect, CornerRadius::same(10), stroke, StrokeKind::Inside);

                let name = item.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let resp = resp.on_hover_text(&name).on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    let _ = open::that_detached(&item.path);
                }
                resp.context_menu(|ui| {
                    if ui.button(format!("{}  Open", icons::IMAGE)).clicked() {
                        let _ = open::that_detached(&item.path);
                        ui.close();
                    }
                    if ui.button(format!("{}  Show in folder", icons::FOLDER_OPEN)).clicked() {
                        recent::reveal(&item.path);
                        ui.close();
                    }
                    if ui.button(format!("{}  Copy path", icons::COPY)).clicked() {
                        ui.ctx().copy_text(item.path.display().to_string());
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(RichText::new(format!("{}  Move to trash", icons::TRASH)).color(p.danger)).clicked() {
                        remove = Some(i);
                        ui.close();
                    }
                });
                if (i + 1) % cols == 0 {
                    ui.end_row();
                }
            }
        });
        if let Some(i) = remove {
            let item = self.recent.remove(i);
            if let Err(e) = std::fs::remove_file(&item.path) {
                log::warn!("delete {}: {e}", item.path.display());
            }
        }
    }
}

fn permission_banner(ui: &mut Ui) {
    let p = theme::current(ui.ctx());
    egui::Frame::new()
        .fill(p.warning.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, p.warning.gamma_multiply(0.6)))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(icons::SHIELD_WARNING).size(22.0).color(p.warning));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(RichText::new("Screen Recording permission needed").strong());
                    ui.label(
                        RichText::new("Enable SnapCap under Privacy & Security → Screen & System Audio Recording, then reopen SnapCap.")
                            .small()
                            .color(p.text_dim),
                    );
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, "Open System Settings").clicked() {
                            crate::platform::request_screen_capture_access();
                            crate::platform::open_screen_capture_settings();
                        }
                    });
                });
            });
        });
}

pub fn shorten_home(path: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = path.strip_prefix(&home) {
            let sep = std::path::MAIN_SEPARATOR;
            return format!("~{sep}{}", rest.display());
        }
    }
    path.display().to_string()
}

/// Shortens long paths in the middle so the start and the folder name stay visible.
pub fn ellipsize_middle(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_owned();
    }
    let keep = max.saturating_sub(1);
    let head = keep / 3;
    let tail = keep - head;
    format!("{}…{}", chars[..head].iter().collect::<String>(), chars[chars.len() - tail..].iter().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipsizes_long_paths() {
        assert_eq!(ellipsize_middle("~/Desktop/screenshots", 56), "~/Desktop/screenshots");
        let long = "/very/long/path/that/goes/on/and/on/forever/screenshots";
        let e = ellipsize_middle(long, 20);
        assert_eq!(e.chars().count(), 20);
        assert!(e.starts_with("/very") && e.ends_with("screenshots"));
    }
}
