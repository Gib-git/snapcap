use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// Mouse position in physical desktop pixels (the process is per-monitor DPI aware).
pub fn cursor_position() -> Option<(f64, f64)> {
    let mut p = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x as f64, p.y as f64))
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
    false
}

/// The Windows heap already decommits large freed blocks.
pub fn release_memory() {}

/// Starts a shell drag of the file at `path` from the window titled `title`, the
/// same kind of drag Explorer starts, so the file can be dropped on any app, folder,
/// browser page or chat. The shell supplies the drag image. Call while the left
/// mouse button is held down; returns once the file is dropped or the drag is
/// cancelled, and `take_drag_end` reports which.
pub fn start_file_drag(title: &str, path: &std::path::Path) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::System::Com::IDataObject;
    use windows::Win32::System::Ole::{IDropSource, OleInitialize, DROPEFFECT_COPY, DROPEFFECT_NONE};
    use windows::Win32::UI::Shell::{BHID_DataObject, IShellItem, SHCreateItemFromParsingName, SHDoDragDrop};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowRect, IsWindowVisible};

    thread_local! {
        static OLE: windows::core::Result<()> = unsafe { OleInitialize(None) };
    }
    if let Err(e) = OLE.with(|r| r.clone()) {
        log::warn!("drag: OleInitialize: {e}");
        return false;
    }
    let data = unsafe {
        SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(path.as_os_str()), None)
            .and_then(|item| item.BindToHandler::<_, IDataObject>(None, &BHID_DataObject))
    };
    let data = match data {
        Ok(d) => d,
        Err(e) => {
            log::warn!("drag {}: {e}", path.display());
            return false;
        }
    };
    // The invisible 1×1 root window shares the main window's title.
    let wanted = HSTRING::from(title);
    let mut hwnd: Option<HWND> = None;
    while let Ok(h) = unsafe { FindWindowExW(None, hwnd, None, &wanted) } {
        let mut r = RECT::default();
        let sized = unsafe { GetWindowRect(h, &mut r) }.is_ok() && r.right - r.left > 1 && r.bottom - r.top > 1;
        hwnd = Some(h);
        if sized && unsafe { IsWindowVisible(h) }.as_bool() {
            break;
        }
    }

    super::begin_drag();
    // Copy only: Explorer would otherwise move the file out of the captures folder.
    let effect = unsafe { SHDoDragDrop(hwnd, &data, None::<&IDropSource>, DROPEFFECT_COPY) };
    super::finish_drag(effect.is_ok_and(|e| e != DROPEFFECT_NONE));
    true
}
