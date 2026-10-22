//! Settings page, including the keyboard shortcut editor.

use eframe::egui::{self, Align, CornerRadius, Event, Key, Layout, RichText, Sense, Stroke, StrokeKind, Ui};
use egui_phosphor::regular as icons;

use super::theme;
use super::widgets::{self, group, section_label, segmented, toggle_row};
use crate::actions::Action;
use crate::app::App;
use crate::hotkeys;
use crate::settings::{default_shortcuts, Quality, ResolutionCap, ThemePref, VideoFormat};

impl App {
    pub fn settings_ui(&mut self, ui: &mut Ui) {
        let p = theme::current(ui.ctx());
        ui.add_space(4.0);

        section_label(ui, "General");
        group(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Save captures to");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.settings.save_dir.is_some() && ui.button("Reset").on_hover_text("Use ~/Desktop/screenshots").clicked() {
                        self.settings.save_dir = None;
                    }
                    if ui.button("Change…").clicked() {
                        if let Some(dir) = rfd::FileDialog::new().set_directory(self.save_dir()).pick_folder() {
                            self.settings.save_dir = Some(dir);
                        }
                    }
                });
            });
            let shown = super::main_window::shorten_home(&self.save_dir());
            // Long paths must not widen the card and push the controls off-screen.
            ui.add(egui::Label::new(RichText::new(&shown).small().color(p.text_dim)).truncate()).on_hover_text(&shown);
            ui.separator();
            toggle_row(ui, "Copy screenshots to clipboard", None, &mut self.settings.copy_to_clipboard);
            toggle_row(ui, "Open file after capture", None, &mut self.settings.open_after_capture);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Appearance");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    segmented(ui, &mut self.settings.theme, &[(ThemePref::System, "System"), (ThemePref::Light, "Light"), (ThemePref::Dark, "Dark")]);
                });
            });
        });

        ui.add_space(10.0);
        section_label(ui, "Recording");
        group(ui, |ui| {
            row(ui, "Format", |ui| {
                segmented(ui, &mut self.settings.video_format, &[(VideoFormat::Mp4, "MP4"), (VideoFormat::Gif, "GIF")]);
            });
            if self.settings.video_format == VideoFormat::Mp4 {
                row(ui, "Frame rate", |ui| {
                    segmented(ui, &mut self.settings.fps, &[(15, "15"), (24, "24"), (30, "30"), (60, "60")]);
                });
                row(ui, "Quality", |ui| {
                    segmented(
                        ui,
                        &mut self.settings.quality,
                        &[(Quality::Low, Quality::Low.label()), (Quality::Medium, Quality::Medium.label()), (Quality::High, Quality::High.label())],
                    );
                });
                row(ui, "Maximum resolution", |ui| {
                    let opts = [ResolutionCap::Native, ResolutionCap::P1440, ResolutionCap::P1080, ResolutionCap::P720];
                    let labelled: Vec<(ResolutionCap, &str)> = opts.iter().map(|o| (*o, o.label())).collect();
                    segmented(ui, &mut self.settings.resolution, &labelled);
                });
            } else {
                row(ui, "GIF frame rate", |ui| {
                    segmented(ui, &mut self.settings.gif_fps, &[(10, "10"), (15, "15"), (20, "20"), (25, "25")]);
                });
                row(ui, "GIF width", |ui| {
                    segmented(ui, &mut self.settings.gif_max_width, &[(480, "480"), (640, "640"), (960, "960"), (1280, "1280")]);
                });
            }
            row(ui, "Countdown", |ui| {
                segmented(ui, &mut self.settings.countdown_secs, &[(0, "Off"), (3, "3 s"), (5, "5 s"), (10, "10 s")]);
            });
            toggle_row(ui, "Show mouse cursor", None, &mut self.settings.show_cursor);
        });

        ui.add_space(10.0);
        section_label(ui, "Audio");
        group(ui, |ui| {
            toggle_row(ui, "Record microphone", Some("MP4 recordings only"), &mut self.settings.record_mic);
            if self.settings.record_mic {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Microphone").color(p.text_dim));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let current = self.settings.mic_device.clone().unwrap_or_else(|| "System default".into());
                        egui::ComboBox::from_id_salt("snapcap-mic")
                            .selected_text(current)
                            .width(220.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.settings.mic_device, None, "System default");
                                for name in self.mic_devices.clone() {
                                    ui.selectable_value(&mut self.settings.mic_device, Some(name.clone()), name);
                                }
                            });
                    });
                });
            }
            ui.add_enabled_ui(self.system_audio_ok, |ui| {
                let hint = if !self.system_audio_ok {
                    "Not available: no monitor source found"
                } else if cfg!(target_os = "macos") {
                    "Requires macOS 14.2 or later"
                } else {
                    "Everything you hear on this computer"
                };
                toggle_row(ui, "Record system audio", Some(hint), &mut self.settings.record_system_audio);
            });
        });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            section_label(ui, "Keyboard shortcuts");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.link(RichText::new("Reset to defaults").small()).clicked() {
                    self.settings.shortcuts = default_shortcuts();
                    self.cancel_shortcut_capture();
                }
            });
        });
        self.shortcuts_editor(ui);

        ui.add_space(10.0);
        section_label(ui, "Command line");
        group(ui, |ui| {
            let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "snapcap".into());
            let msg = if self.hotkeys.available() {
                "Script SnapCap or bind these commands in your desktop's keyboard settings:"
            } else {
                "Bind these commands to keys in your desktop's keyboard settings:"
            };
            ui.label(RichText::new(msg).small().color(p.text_dim));
            for args in ["--shot screen", "--shot region", "--record region", "--record stop"] {
                let cmd = format!("\"{exe}\" {args}");
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&cmd).monospace().size(11.5));
                    if widgets::icon_button(ui, icons::COPY, "Copy").clicked() {
                        ui.ctx().copy_text(cmd.clone());
                    }
                });
            }
        });

        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(format!("SnapCap {} · Everything stays on this computer", env!("CARGO_PKG_VERSION")))
                    .small()
                    .color(p.text_dim),
            );
        });
        ui.add_space(8.0);
    }

    fn shortcuts_editor(&mut self, ui: &mut Ui) {
        let p = theme::current(ui.ctx());
        self.capture_shortcut_keys(ui);
        group(ui, |ui| {
            if let Some(reason) = &self.hotkeys.unavailable_reason {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(icons::INFO).color(p.warning));
                    ui.label(RichText::new(format!("{reason} Use the command-line actions below with your desktop's shortcut settings.")).small().color(p.text_dim));
                });
                ui.separator();
            }
            let dups = hotkeys::duplicates(&self.settings.shortcuts);
            for (i, action) in Action::ALL.into_iter().enumerate() {
                if i > 0 {
                    ui.add_space(2.0);
                }
                ui.horizontal(|ui| {
                    ui.label(action.label());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let text = self.settings.shortcut(action).to_owned();
                        let capturing = self.capturing_shortcut == Some(action);
                        if !text.is_empty() && !capturing {
                            if widgets::icon_button(ui, icons::X, "Remove shortcut").clicked() {
                                self.settings.shortcuts.insert(action, String::new());
                            }
                        } else {
                            // Keep the shortcut buttons in one column.
                            ui.add_space(32.0 + ui.spacing().item_spacing.x);
                        }
                        if shortcut_button(ui, &text, capturing).clicked() {
                            if capturing {
                                self.capturing_shortcut = None;
                                self.hotkeys.apply(&self.settings.shortcuts);
                            } else {
                                self.capturing_shortcut = Some(action);
                                // Our own global shortcuts would swallow the keys being recorded.
                                self.hotkeys.suspend();
                            }
                        }
                    });
                });
                let text = self.settings.shortcut(action);
                let problem = if let Some(other) = dups.get(&action) {
                    Some((p.danger, format!("Same as “{}”", other.label())))
                } else if let Some(e) = self.hotkeys.errors.get(&action) {
                    Some((p.danger, e.clone()))
                } else {
                    hotkeys::system_conflict(text).map(|w| (p.warning, format!("{w} — it may not trigger SnapCap")))
                };
                if let Some((color, msg)) = problem {
                    ui.label(RichText::new(format!("{}  {msg}", icons::WARNING)).small().color(color));
                }
            }
        });
    }

    /// While a shortcut is being recorded, turn the next key press into a shortcut string.
    fn capture_shortcut_keys(&mut self, ui: &mut Ui) {
        let Some(action) = self.capturing_shortcut else { return };
        let events = ui.ctx().input_mut(|i| {
            let keys: Vec<Event> = i.events.iter().filter(|e| matches!(e, Event::Key { .. })).cloned().collect();
            i.events.retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_)));
            keys
        });
        for e in events {
            let Event::Key { key, physical_key, pressed: true, modifiers, repeat: false, .. } = e else { continue };
            let no_mods = !modifiers.any();
            match key {
                Key::Escape if no_mods => {}
                Key::Backspace | Key::Delete if no_mods => {
                    self.settings.shortcuts.insert(action, String::new());
                }
                _ => {
                    // Prefer the physical key: Option+digit on macOS produces symbols as the logical key.
                    let Some(s) = hotkeys::from_egui(modifiers, physical_key.unwrap_or(key)) else { continue };
                    self.settings.shortcuts.insert(action, s);
                }
            }
            self.capturing_shortcut = None;
            self.hotkeys.apply(&self.settings.shortcuts);
            if let Some(t) = &self.tray {
                t.set_shortcuts(&self.settings.shortcuts);
            }
            break;
        }
    }
}

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(Layout::right_to_left(Align::Center), add);
    });
}

fn shortcut_button(ui: &mut Ui, text: &str, capturing: bool) -> egui::Response {
    let p = theme::current(ui.ctx());
    let label = if capturing { "Press keys…  (Esc to cancel)".to_owned() } else { hotkeys::display(text) };
    let font = egui::FontId::proportional(13.0);
    let galley = ui.painter().layout_no_wrap(label, font, p.text);
    let size = egui::vec2((galley.size().x + 24.0).max(110.0), 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let (fill, stroke) = if capturing {
        let t = ui.ctx().input(|i| i.time) as f32;
        ui.ctx().request_repaint();
        (p.accent_soft, Stroke::new(1.5, p.accent.gamma_multiply(0.6 + 0.4 * (t * 4.0).sin().abs())))
    } else if resp.hovered() {
        (p.surface_hover, Stroke::new(1.0, p.accent))
    } else {
        (p.surface_hover, Stroke::new(1.0, p.border))
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
    ui.painter().rect_stroke(rect, CornerRadius::same(7), stroke, StrokeKind::Inside);
    let color = if text.is_empty() && !capturing { p.text_dim } else { p.text };
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    resp.on_hover_text(if capturing { "Press the new shortcut, Backspace to remove it, Esc to cancel" } else { "Click to change" })
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

