//! Small OS-specific helpers. Everything here degrades to a no-op where unsupported.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

// ---------------------------------------------------------------- drag and drop

use std::sync::atomic::{AtomicU8, Ordering};

/// How a drag of a capture file out of SnapCap ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragEnd {
    /// Another app or folder accepted the file.
    Dropped,
    Cancelled,
}

const DRAG_IDLE: u8 = 0;
const DRAG_ACTIVE: u8 = 1;
const DRAG_DROPPED: u8 = 2;
const DRAG_CANCELLED: u8 = 3;

/// The OS drag session runs outside egui (asynchronously on macOS, in a modal
/// loop on Windows), so its outcome is handed back through here.
static DRAG: AtomicU8 = AtomicU8::new(DRAG_IDLE);

#[cfg_attr(target_os = "linux", allow(dead_code))]
fn begin_drag() {
    DRAG.store(DRAG_ACTIVE, Ordering::Relaxed);
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
fn finish_drag(dropped: bool) {
    DRAG.store(if dropped { DRAG_DROPPED } else { DRAG_CANCELLED }, Ordering::Relaxed);
}

/// Returns how the last drag ended, once; `None` while it is still in progress.
pub fn take_drag_end() -> Option<DragEnd> {
    match DRAG.load(Ordering::Relaxed) {
        DRAG_DROPPED | DRAG_CANCELLED => match DRAG.swap(DRAG_IDLE, Ordering::Relaxed) {
            DRAG_DROPPED => Some(DragEnd::Dropped),
            DRAG_CANCELLED => Some(DragEnd::Cancelled),
            _ => None,
        },
        _ => None,
    }
}
