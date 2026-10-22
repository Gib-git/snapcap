pub mod main_window;
pub mod notice;
pub mod overlay;
pub mod recbar;
pub mod settings_view;
pub mod theme;
pub mod widgets;

use std::sync::atomic::{AtomicU32, Ordering};

use crate::capture::MonitorInfo;

/// Desktop units per egui point when positioning new windows. macOS desktop
/// coordinates are already points; Windows/X11 report physical pixels, which
/// winit converts with the scale of the primary monitor the window starts on.
static DESKTOP_SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0f32

pub fn init_desktop_scale(monitors: &[MonitorInfo]) {
    let scale = if cfg!(target_os = "macos") {
        1.0
    } else {
        monitors.iter().find(|m| m.primary).or(monitors.first()).map(|m| m.scale).unwrap_or(1.0).max(0.5)
    };
    DESKTOP_SCALE.store(scale.to_bits(), Ordering::Relaxed);
}

/// SnapCap hides its own floating windows from screen captures. Setting
/// `SNAPCAP_ALLOW_SELF_CAPTURE=1` disables that (useful for documenting the app).
pub fn protect_from_capture() -> bool {
    static ALLOW: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    !*ALLOW.get_or_init(|| std::env::var_os("SNAPCAP_ALLOW_SELF_CAPTURE").is_some_and(|v| v != "0"))
}

pub fn desktop_to_points(x: f32, y: f32) -> (f32, f32) {
    let s = f32::from_bits(DESKTOP_SCALE.load(Ordering::Relaxed));
    (x / s, y / s)
}
