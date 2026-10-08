/// No portable way to query the pointer on Wayland; callers fall back to the primary display.
pub fn cursor_position() -> Option<(f64, f64)> {
    None
}

pub fn has_screen_capture_access() -> bool {
    true
}

pub fn request_screen_capture_access() -> bool {
    true
}

pub fn open_screen_capture_settings() {}

pub fn set_dock_visible(_visible: bool) {}

pub fn raise_overlay(_title: &str, _frame: (f64, f64, f64, f64)) {}

pub fn float_window(_title: &str) {}

pub fn yield_focus() {}

pub fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").map(|v| v == "wayland").unwrap_or(false)
}

#[cfg(target_env = "gnu")]
unsafe extern "C" {
    fn malloc_trim(pad: usize) -> i32;
}

/// Returns freed memory to the OS after large one-off allocations.
pub fn release_memory() {
    #[cfg(target_env = "gnu")]
    unsafe {
        malloc_trim(0);
    }
}

/// Dragging files out to other apps isn't supported on Linux yet.
pub fn start_file_drag(_title: &str, _path: &std::path::Path) -> bool {
    false
}
