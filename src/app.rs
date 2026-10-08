//! Application state machine.
//!
//! The eframe root viewport is an invisible 1×1 "controller" window that never hides,
//! so app logic keeps running (and can open new windows) while everything visible is
//! closed. The main window, region overlay, recording bar and notices are all child
//! viewports that come and go.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{self, ColorImage, IconData, Pos2, TextureHandle, TextureOptions, ViewportBuilder, ViewportCommand, ViewportId};
use image::RgbaImage;

use crate::actions::Action;
use crate::capture::{self, CaptureTarget, MonitorInfo};
use crate::hotkeys::Hotkeys;
use crate::output::{self, CaptureKind};
use crate::recent::{self, MediaKind};
use crate::record::audio::AudioSpec;
use crate::record::{RecordConfig, Recorder, RecordingResult};
use crate::settings::{Settings, VideoFormat};
use crate::tray::{Tray, TrayCmd};
use crate::ui::notice::{self, Notice, NoticeAction, NoticeKind};
use crate::ui::overlay::{Outcome, Purpose, Selector};
use crate::ui::recbar::{self, BarAction, BarState};
use crate::ui::theme;

/// Title of the main window; platform code finds the native window by it.
pub const MAIN_TITLE: &str = "SnapCap";

pub enum AppEvent {
    Action(Action),
    /// A global shortcut fired; resolved to an action on the UI thread.
    Hotkey(u32),
    Tray(TrayCmd),
    ShotSaved { path: PathBuf, thumb: ColorImage, monitor: MonitorInfo },
    ShotFailed(String),
    RecordingDone(Result<RecordingResult, String>),
    RecentScanned(Vec<PathBuf>),
    Thumb { path: PathBuf, image: ColorImage },
}

pub enum Mode {
    Idle,
    /// Waiting for SnapCap's own windows to disappear before capturing.
    Preparing { action: Action, at: Instant },
    Selecting(Box<Selector>),
    Countdown { target: CaptureTarget, until: Instant, frames: u32 },
    Recording { recorder: Recorder, target: CaptureTarget, frames: u32 },
    Finalizing { target: CaptureTarget },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Settings,
}

pub struct RecentItem {
    pub path: PathBuf,
    pub kind: MediaKind,
    pub thumb: Option<TextureHandle>,
}

pub struct App {
    pub settings: Settings,
    saved_settings: Settings,
    pub tx: Sender<AppEvent>,
    rx: Receiver<AppEvent>,
    pub hotkeys: Hotkeys,
    pub tray: Option<Tray>,
    clipboard: Option<arboard::Clipboard>,
    pub mode: Mode,
    pub page: Page,
    pub main_visible: bool,
    main_pos: Option<Pos2>,
    /// Re-open the main window when the current capture finishes.
    restore_main: bool,
    pub recent: Vec<RecentItem>,
    pub notice: Option<Notice>,
    /// Action whose shortcut is being recorded in Settings.
    pub capturing_shortcut: Option<Action>,
    pub mic_devices: Vec<String>,
    pub system_audio_ok: bool,
    pub has_permission: bool,
    last_permission_check: Instant,
    pub icon: Arc<IconData>,
    pub logo: Option<TextureHandle>,
    startup_action: Option<Action>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings, startup_action: Option<Action>, background: bool) -> Self {
        let ctx = cc.egui_ctx.clone();
        theme::install(&ctx, settings.theme);
        crate::ui::init_desktop_scale(&capture::monitor_infos());

        let (tx, rx) = unbounded();

        // Global shortcuts → events.
        let mut hotkeys = Hotkeys::new();
        hotkeys.apply(&settings.shortcuts);
        {
            let (tx, ctx) = (tx.clone(), ctx.clone());
            global_hotkey::GlobalHotKeyEvent::set_event_handler(Some(move |e: global_hotkey::GlobalHotKeyEvent| {
                if e.state() == global_hotkey::HotKeyState::Pressed {
                    let _ = tx.send(AppEvent::Hotkey(e.id()));
                    ctx.request_repaint();
                }
            }));
        }

        // Tray menu → events.
        let tray = match Tray::new(&settings.shortcuts) {
            Ok(t) => Some(t),
            Err(e) => {
                log::warn!("tray icon unavailable: {e}");
                None
            }
        };
        {
            let (tx, ctx) = (tx.clone(), ctx.clone());
            tray_icon::menu::MenuEvent::set_event_handler(Some(move |e: tray_icon::menu::MenuEvent| {
                if let Some(cmd) = TrayCmd::from_menu_id(&e.id.0) {
                    let _ = tx.send(AppEvent::Tray(cmd));
                    ctx.request_repaint();
                }
            }));
        }
        if cfg!(windows) {
            let (tx, ctx) = (tx.clone(), ctx.clone());
            tray_icon::TrayIconEvent::set_event_handler(Some(move |e: tray_icon::TrayIconEvent| {
                if let tray_icon::TrayIconEvent::DoubleClick { .. } = e {
                    let _ = tx.send(AppEvent::Action(Action::ShowWindow));
                    ctx.request_repaint();
                }
            }));
        }

        // Later launches (`snapcap --shot region`) forward their action here.
        {
            let (tx, ctx) = (tx.clone(), ctx.clone());
            if let Err(e) = crate::ipc::listen(move |a| {
                let _ = tx.send(AppEvent::Action(a));
                ctx.request_repaint();
            }) {
                log::warn!("single-instance listener unavailable: {e}");
            }
        }

        let size = 128;
        let icon = Arc::new(IconData { rgba: crate::icon::app_icon(size), width: size, height: size });
        let logo = Some(ctx.load_texture(
            "snapcap-logo",
            ColorImage::from_rgba_unmultiplied([64, 64], &crate::icon::app_icon(64)),
            TextureOptions::LINEAR,
        ));

        let main_visible = !background || tray.is_none();
        crate::platform::set_dock_visible(main_visible);

        let mut app = Self {
            saved_settings: settings.clone(),
            settings,
            tx,
            rx,
            hotkeys,
            tray,
            clipboard: arboard::Clipboard::new().ok(),
            mode: Mode::Idle,
            page: Page::Home,
            main_visible,
            main_pos: None,
            restore_main: false,
            recent: Vec::new(),
            notice: None,
            capturing_shortcut: None,
            mic_devices: Vec::new(),
            system_audio_ok: true,
            has_permission: crate::platform::has_screen_capture_access(),
            last_permission_check: Instant::now(),
            icon,
            logo,
            startup_action,
        };
        app.refresh_recent(&ctx);
        app.refresh_audio_devices();
        // Development aid for UI review: `SNAPCAP_DEV_PAGE=settings cargo run`.
        if cfg!(debug_assertions) && std::env::var("SNAPCAP_DEV_PAGE").as_deref() == Ok("settings") {
            app.page = Page::Settings;
        }
        app
    }

    pub fn save_dir(&self) -> PathBuf {
        output::save_dir(&self.settings)
    }

    pub fn refresh_audio_devices(&mut self) {
        self.mic_devices = crate::record::audio::input_device_names();
        self.system_audio_ok = crate::record::audio::system_audio_supported();
    }

    pub fn refresh_recent(&mut self, ctx: &egui::Context) {
        let dir = self.save_dir();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let files = recent::scan(&dir);
            let _ = tx.send(AppEvent::RecentScanned(files.clone()));
            ctx.request_repaint();
            for path in files {
                if let Some(image) = recent::load_thumbnail(&path) {
                    let _ = tx.send(AppEvent::Thumb { path, image });
                    ctx.request_repaint();
                }
            }
            crate::platform::release_memory();
        });
    }

    fn add_recent(&mut self, ctx: &egui::Context, path: PathBuf, thumb: Option<ColorImage>) {
        let Some(kind) = MediaKind::from_path(&path) else { return };
        self.recent.retain(|r| r.path != path);
        let thumb = thumb.map(|t| ctx.load_texture(path.to_string_lossy(), t, TextureOptions::LINEAR));
        self.recent.insert(0, RecentItem { path, kind, thumb });
        self.recent.truncate(recent::MAX_RECENT);
    }

    // ------------------------------------------------------------------ actions

    pub fn trigger(&mut self, ctx: &egui::Context, action: Action) {
        log::info!("action: {action:?}");
        match action {
            Action::ShowWindow => self.show_main(),
            Action::StopRecording => self.stop_recording(),
            Action::TogglePause => {
                if let Mode::Recording { recorder, .. } = &self.mode {
                    recorder.toggle_pause();
                }
            }
            _ => {
                if !matches!(self.mode, Mode::Idle) {
                    if matches!(self.mode, Mode::Recording { .. }) && matches!(action, Action::RecordScreen | Action::RecordRegion) {
                        self.stop_recording();
                    }
                    return;
                }
                if !self.has_permission {
                    self.has_permission = crate::platform::has_screen_capture_access();
                    if !self.has_permission {
                        crate::platform::request_screen_capture_access();
                        self.show_main();
                        return;
                    }
                }
                self.cancel_shortcut_capture();
                self.restore_main = self.main_visible;
                let delay = if self.main_visible { Duration::from_millis(260) } else { Duration::from_millis(30) };
                self.main_visible = false;
                self.notice = None;
                self.mode = Mode::Preparing { action, at: Instant::now() + delay };
                ctx.request_repaint_after(delay);
            }
        }
    }

    fn run_prepared(&mut self, ctx: &egui::Context, action: Action) {
        self.mode = Mode::Idle;
        match action {
            Action::ShotScreen => {
                let infos = capture::monitor_infos();
                let idx = capture::monitor_under_cursor(&infos);
                match capture::capture_monitor(idx) {
                    Ok(shot) => self.save_screenshot(ctx, shot.image, shot.monitor),
                    Err(e) => self.capture_failed(&e.to_string()),
                }
            }
            Action::ShotAllScreens => match capture::capture_all() {
                Ok(shots) => {
                    let monitor = shots.iter().find(|s| s.monitor.primary).unwrap_or(&shots[0]).monitor.clone();
                    let img = capture::stitch(&shots);
                    self.save_screenshot(ctx, img, monitor);
                }
                Err(e) => self.capture_failed(&e.to_string()),
            },
            Action::ShotRegion | Action::RecordRegion => match capture::capture_all() {
                Ok(shots) => {
                    let infos: Vec<MonitorInfo> = shots.iter().map(|s| s.monitor.clone()).collect();
                    let focus = capture::monitor_under_cursor(&infos);
                    let purpose = if action == Action::ShotRegion { Purpose::Screenshot } else { Purpose::Record };
                    self.mode = Mode::Selecting(Box::new(Selector::new(purpose, shots, focus)));
                }
                Err(e) => self.capture_failed(&e.to_string()),
            },
            Action::RecordScreen => {
                let infos = capture::monitor_infos();
                if infos.is_empty() {
                    self.capture_failed("no displays found");
                    return;
                }
                let m = infos[capture::monitor_under_cursor(&infos)].clone();
                let s = m.px_per_unit();
                let size = ((m.width as f32 * s).round() as u32, (m.height as f32 * s).round() as u32);
                self.begin_recording(ctx, CaptureTarget { monitor: m, image_size: size, region: None });
            }
            _ => {}
        }
    }

    fn capture_failed(&mut self, msg: &str) {
        self.finish_capture();
        self.show_error("Capture failed", msg, None);
    }

    /// Restores the main window if the capture was started from it.
    fn finish_capture(&mut self) {
        self.mode = Mode::Idle;
        if self.restore_main {
            self.restore_main = false;
            self.show_main();
        }
    }

    fn save_screenshot(&mut self, ctx: &egui::Context, image: RgbaImage, monitor: MonitorInfo) {
        if self.settings.copy_to_clipboard {
            if let Some(cb) = self.clipboard.as_mut() {
                let data = arboard::ImageData {
                    width: image.width() as usize,
                    height: image.height() as usize,
                    bytes: Cow::Borrowed(image.as_raw()),
                };
                if let Err(e) = cb.set_image(data) {
                    log::warn!("clipboard: {e}");
                }
            }
        }
        let dir = self.save_dir();
        let (tx, ctx2) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = output::new_path(&dir, CaptureKind::Screenshot, "png")
                .and_then(|path| output::write_png(&path, image.width(), image.height(), image.as_raw()).map(|_| path));
            let ev = match result {
                Ok(path) => AppEvent::ShotSaved { path, thumb: recent::thumbnail_of(&image), monitor },
                Err(e) => AppEvent::ShotFailed(e.to_string()),
            };
            drop(image);
            crate::platform::release_memory();
            let _ = tx.send(ev);
            ctx2.request_repaint();
        });
        self.finish_capture();
    }

    fn on_selection(&mut self, ctx: &egui::Context, sel: Selector, outcome: Outcome) {
        match outcome {
            Outcome::Cancel => self.finish_capture(),
            Outcome::Confirm { shot, rect, full } => {
                let s = sel.shots.into_iter().nth(shot).expect("valid shot index");
                match sel.purpose {
                    Purpose::Screenshot => {
                        let img = if full {
                            s.image
                        } else {
                            image::imageops::crop_imm(&s.image, rect.x, rect.y, rect.w, rect.h).to_image()
                        };
                        self.save_screenshot(ctx, img, s.monitor);
                    }
                    Purpose::Record => {
                        let size = s.image.dimensions();
                        let region = (!full).then_some(rect);
                        self.begin_recording(ctx, CaptureTarget { monitor: s.monitor, image_size: size, region });
                    }
                }
            }
        }
    }

    fn begin_recording(&mut self, ctx: &egui::Context, target: CaptureTarget) {
        if self.settings.countdown_secs == 0 {
            self.start_recording(ctx, target);
        } else {
            let until = Instant::now() + Duration::from_secs(self.settings.countdown_secs as u64);
            self.mode = Mode::Countdown { target, until, frames: 0 };
        }
    }

    fn start_recording(&mut self, ctx: &egui::Context, target: CaptureTarget) {
        let s = &self.settings;
        let ext = match s.video_format {
            VideoFormat::Mp4 => "mp4",
            VideoFormat::Gif => "gif",
        };
        let path = match output::new_path(&self.save_dir(), CaptureKind::Recording, ext) {
            Ok(p) => p,
            Err(e) => return self.capture_failed(&e.to_string()),
        };
        let cfg = RecordConfig {
            target: target.clone(),
            format: s.video_format,
            fps: if s.video_format == VideoFormat::Gif { s.gif_fps } else { s.fps },
            quality: s.quality,
            resolution: s.resolution,
            gif_max_width: s.gif_max_width,
            audio: AudioSpec {
                mic: s.record_mic.then(|| s.mic_device.clone()),
                system: s.record_system_audio,
            },
            show_cursor: s.show_cursor,
            path,
        };
        let (tx, ctx2) = (self.tx.clone(), ctx.clone());
        let done = Box::new(move |res: crate::error::Result<RecordingResult>| {
            let _ = tx.send(AppEvent::RecordingDone(res.map_err(|e| e.to_string())));
            ctx2.request_repaint();
        });
        match Recorder::start(cfg, done) {
            Ok(recorder) => {
                if let Some(t) = &self.tray {
                    t.set_recording(true);
                }
                self.mode = Mode::Recording { recorder, target, frames: 0 };
            }
            Err(e) => self.capture_failed(&format!("Could not start recording: {e}")),
        }
    }

    fn stop_recording(&mut self) {
        match std::mem::replace(&mut self.mode, Mode::Idle) {
            Mode::Recording { recorder, target, .. } => {
                recorder.stop();
                if let Some(t) = &self.tray {
                    t.set_recording(false);
                }
                self.mode = Mode::Finalizing { target };
            }
            Mode::Countdown { .. } => self.finish_capture(),
            other => self.mode = other,
        }
    }

    // ------------------------------------------------------------------ windows

    pub fn show_main(&mut self) {
        self.main_visible = true;
        crate::platform::set_dock_visible(true);
    }

    /// Stops waiting for a new shortcut and re-registers the global shortcuts it suspended.
    pub fn cancel_shortcut_capture(&mut self) {
        if self.capturing_shortcut.take().is_some() {
            self.hotkeys.apply(&self.settings.shortcuts);
        }
    }

    pub fn hide_main(&mut self) {
        self.main_visible = false;
        self.cancel_shortcut_capture();
        if self.tray.is_some() {
            crate::platform::set_dock_visible(false);
            if !self.settings.tray_hint_shown {
                self.settings.tray_hint_shown = true;
                let where_ = if cfg!(target_os = "macos") { "menu bar" } else { "system tray" };
                let mut n = Notice::new(NoticeKind::Success, "SnapCap is still running", format!("Find it in the {where_}, or use your shortcuts."), self.notice_pos(None));
                n.until = Instant::now() + Duration::from_secs(6);
                self.notice = Some(n);
            }
        }
    }

    fn notice_pos(&self, monitor: Option<&MonitorInfo>) -> Pos2 {
        let infos;
        let m = match monitor {
            Some(m) => m,
            None => {
                infos = capture::monitor_infos();
                match infos.iter().find(|m| m.primary).or(infos.first()) {
                    Some(m) => m,
                    None => return Pos2::new(40.0, 40.0),
                }
            }
        };
        let (x, y) = crate::ui::desktop_to_points(m.x as f32, m.y as f32);
        let (w, h) = crate::ui::desktop_to_points(m.width as f32, m.height as f32);
        let px = x + w - notice::SIZE.x - 16.0;
        let py = if cfg!(target_os = "macos") { y + 34.0 } else { y + h - notice::SIZE.y - 64.0 };
        Pos2::new(px, py)
    }

    fn show_error(&mut self, title: &str, detail: &str, monitor: Option<&MonitorInfo>) {
        log::error!("{title}: {detail}");
        self.notice = Some(Notice::new(NoticeKind::Error, title, detail, self.notice_pos(monitor)));
    }

    // ------------------------------------------------------------------ events

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                AppEvent::Action(a) => self.trigger(ctx, a),
                AppEvent::Hotkey(id) => {
                    if let Some(a) = self.hotkeys.action_for(id) {
                        self.trigger(ctx, a);
                    }
                }
                AppEvent::Tray(cmd) => match cmd {
                    TrayCmd::Action(a) => self.trigger(ctx, a),
                    TrayCmd::OpenFolder => {
                        let dir = self.save_dir();
                        let _ = std::fs::create_dir_all(&dir);
                        let _ = open::that_detached(dir);
                    }
                    TrayCmd::Quit => self.quit(ctx),
                },
                AppEvent::ShotSaved { path, thumb, monitor } => {
                    let tex = ctx.load_texture(format!("notice-{}", path.display()), thumb.clone(), TextureOptions::LINEAR);
                    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    let detail = if self.settings.copy_to_clipboard { format!("{name}\nCopied to clipboard") } else { name };
                    let mut n = Notice::new(NoticeKind::Success, "Screenshot saved", detail, self.notice_pos(Some(&monitor)));
                    n.path = Some(path.clone());
                    n.thumb = Some(tex);
                    self.notice = Some(n);
                    if self.settings.open_after_capture {
                        let _ = open::that_detached(&path);
                    }
                    self.add_recent(ctx, path, Some(thumb));
                }
                AppEvent::ShotFailed(e) => self.show_error("Could not save screenshot", &e, None),
                AppEvent::RecordingDone(res) => {
                    crate::platform::release_memory();
                    let monitor = match &self.mode {
                        Mode::Finalizing { target } | Mode::Recording { target, .. } => Some(target.monitor.clone()),
                        _ => None,
                    };
                    // The worker can also finish on its own when encoding fails mid-recording.
                    match std::mem::replace(&mut self.mode, Mode::Idle) {
                        Mode::Recording { recorder, .. } => {
                            recorder.stop();
                            if let Some(t) = &self.tray {
                                t.set_recording(false);
                            }
                        }
                        Mode::Finalizing { .. } => {}
                        other => self.mode = other,
                    }
                    match res {
                        Ok(r) => {
                            let name = r.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                            let mut detail = format!("{name} · {}", crate::ui::widgets::format_duration(r.duration));
                            for w in &r.warnings {
                                detail.push('\n');
                                detail.push_str(w);
                            }
                            let mut n = Notice::new(NoticeKind::Success, "Recording saved", detail, self.notice_pos(monitor.as_ref()));
                            n.path = Some(r.path.clone());
                            n.is_video = r.path.extension().is_some_and(|e| e == "mp4");
                            let thumb = recent::load_thumbnail(&r.path);
                            n.thumb = thumb.clone().map(|t| ctx.load_texture("notice-rec", t, TextureOptions::LINEAR));
                            self.notice = Some(n);
                            if self.settings.open_after_capture {
                                let _ = open::that_detached(&r.path);
                            }
                            self.add_recent(ctx, r.path, thumb);
                        }
                        Err(e) => self.show_error("Recording failed", &e, monitor.as_ref()),
                    }
                    if self.restore_main {
                        self.restore_main = false;
                        self.show_main();
                    }
                }
                AppEvent::RecentScanned(files) => {
                    self.recent = files
                        .into_iter()
                        .filter_map(|path| Some(RecentItem { kind: MediaKind::from_path(&path)?, path, thumb: None }))
                        .collect();
                }
                AppEvent::Thumb { path, image } => {
                    if let Some(item) = self.recent.iter_mut().find(|r| r.path == path) {
                        item.thumb = Some(ctx.load_texture(path.to_string_lossy(), image, TextureOptions::LINEAR));
                    }
                }
            }
        }
    }

    pub fn quit(&mut self, ctx: &egui::Context) {
        if let Mode::Recording { .. } = self.mode {
            self.stop_recording();
            // Give the encoder a moment to write the index before exiting.
            std::thread::sleep(Duration::from_millis(600));
        }
        self.settings.save();
        ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
    }

    fn persist_settings(&mut self, ctx: &egui::Context) {
        if self.settings == self.saved_settings {
            return;
        }
        if self.settings.shortcuts != self.saved_settings.shortcuts && self.capturing_shortcut.is_none() {
            self.hotkeys.apply(&self.settings.shortcuts);
            if let Some(t) = &self.tray {
                t.set_shortcuts(&self.settings.shortcuts);
            }
        }
        if self.settings.theme != self.saved_settings.theme {
            theme::set_theme(ctx, self.settings.theme);
        }
        if self.settings.save_dir != self.saved_settings.save_dir {
            self.refresh_recent(ctx);
        }
        self.settings.save();
        self.saved_settings = self.settings.clone();
    }

    // ------------------------------------------------------------------ frame

    fn main_window(&mut self, ctx: &egui::Context) {
        let mut vb = ViewportBuilder::default()
            .with_title(MAIN_TITLE)
            .with_app_id("snapcap")
            .with_icon(self.icon.clone())
            .with_inner_size([600.0, 700.0])
            .with_min_inner_size([520.0, 560.0]);
        if let Some(p) = self.main_pos {
            vb = vb.with_position(p);
        }
        if cfg!(debug_assertions) {
            if let Some(h) = std::env::var("SNAPCAP_DEV_HEIGHT").ok().and_then(|h| h.parse::<f32>().ok()) {
                vb = vb.with_inner_size([600.0, h]);
            }
        }
        let id = ViewportId::from_hash_of("snapcap-main");
        let close = ctx.show_viewport_immediate(id, vb, |ui, _| {
            self.main_ui(ui);
            let info = ui.ctx().input(|i| i.viewport().clone());
            if let Some(r) = info.outer_rect {
                self.main_pos = Some(r.min);
            }
            info.close_requested()
        });
        if close {
            if self.tray.is_some() {
                self.hide_main();
            } else {
                self.quit(ctx);
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(a) = self.startup_action.take() {
            self.trigger(&ctx, a);
        }
        self.handle_events(&ctx);

        if !self.has_permission && self.main_visible && self.last_permission_check.elapsed() > Duration::from_secs(2) {
            self.last_permission_check = Instant::now();
            self.has_permission = crate::platform::has_screen_capture_access();
            ctx.request_repaint_after(Duration::from_secs(2));
        }

        // State machine.
        match &mut self.mode {
            Mode::Preparing { action, at } => {
                let (action, at) = (*action, *at);
                if Instant::now() >= at {
                    self.run_prepared(&ctx, action);
                } else {
                    ctx.request_repaint_after(at - Instant::now());
                }
            }
            Mode::Selecting(sel) => {
                if let Some(outcome) = sel.show(&ctx) {
                    if !self.restore_main {
                        crate::platform::yield_focus();
                    }
                    if let Mode::Selecting(sel) = std::mem::replace(&mut self.mode, Mode::Idle) {
                        self.on_selection(&ctx, *sel, outcome);
                    }
                    crate::platform::release_memory();
                }
            }
            Mode::Countdown { target, until, frames } => {
                *frames += 1;
                let left = until.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    let target = target.clone();
                    self.start_recording(&ctx, target);
                } else {
                    let remaining = left.as_secs_f32().ceil() as u32;
                    let target = target.clone();
                    let first = *frames <= 3;
                    let action = recbar::show_bar(&ctx, &target, &BarState::Countdown { remaining }, "", first);
                    recbar::show_border(&ctx, &target, false, first);
                    if action.is_some() {
                        self.finish_capture();
                    }
                }
            }
            Mode::Recording { recorder, target, frames } => {
                *frames += 1;
                let state = BarState::Recording { elapsed: recorder.elapsed(), paused: recorder.is_paused() };
                let badge = match self.settings.video_format {
                    VideoFormat::Mp4 => "REC",
                    VideoFormat::Gif => "GIF",
                };
                let first = *frames <= 3;
                let paused = recorder.is_paused();
                let target = target.clone();
                let action = recbar::show_bar(&ctx, &target, &state, badge, first);
                recbar::show_border(&ctx, &target, paused, first);
                match action {
                    Some(BarAction::Stop) | Some(BarAction::Cancel) => self.stop_recording(),
                    Some(BarAction::TogglePause) => {
                        if let Mode::Recording { recorder, .. } = &self.mode {
                            recorder.toggle_pause();
                        }
                    }
                    None => {}
                }
            }
            Mode::Finalizing { target } => {
                let target = target.clone();
                recbar::show_bar(&ctx, &target, &BarState::Finalizing, "", false);
                ctx.request_repaint_after(Duration::from_millis(200));
            }
            Mode::Idle => {}
        }

        if self.main_visible {
            self.main_window(&ctx);
        }

        if let Some(n) = self.notice.as_mut() {
            match n.show(&ctx) {
                Some(NoticeAction::Open) => {
                    if let Some(p) = &n.path {
                        let _ = open::that_detached(p);
                    }
                    self.notice = None;
                }
                Some(NoticeAction::Reveal) => {
                    if let Some(p) = &n.path {
                        recent::reveal(p);
                    }
                    self.notice = None;
                }
                Some(NoticeAction::Dismiss) => self.notice = None,
                None => {}
            }
            if self.notice.is_none() {
                // egui closes the notice window on the next frame, which nothing else would trigger.
                ctx.request_repaint();
            }
        }

        self.persist_settings(&ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.settings.save();
    }
}
