use std::cell::OnceCell;
use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession,
    NSDraggingSource, NSEvent, NSEventModifierFlags, NSEventType, NSImage, NSWindow, NSWindowCollectionBehavior, NSWorkspace,
};
use objc2_foundation::{NSArray, NSPoint, NSProcessInfo, NSRect, NSSize, NSString, NSURL};

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> CGRect;
}

/// Mouse position in global top-left-origin points (the same space xcap uses).
pub fn cursor_position() -> Option<(f64, f64)> {
    let p = NSEvent::mouseLocation();
    let main_h = unsafe { CGDisplayBounds(CGMainDisplayID()) }.size.height;
    Some((p.x, main_h - p.y))
}

pub fn has_screen_capture_access() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Shows the system prompt (first time only) and registers the app in System Settings.
pub fn request_screen_capture_access() -> bool {
    unsafe { CGRequestScreenCaptureAccess() }
}

pub fn open_screen_capture_settings() {
    let _ = open::that("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture");
}

/// Shows or hides the Dock icon. Hidden while SnapCap only lives in the menu bar.
pub fn set_dock_visible(visible: bool) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    let policy = if visible { NSApplicationActivationPolicy::Regular } else { NSApplicationActivationPolicy::Accessory };
    app.setActivationPolicy(policy);
}

fn with_window(title: &str, f: impl Fn(&NSWindow)) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    let wanted = NSString::from_str(title);
    for window in app.windows().iter() {
        if window.title().isEqualToString(&wanted) {
            f(&window);
        }
    }
}

/// Puts the region overlay above the menu bar and Dock, sizes it to exactly cover
/// its display (`frame` in top-left-origin desktop points), and gives it keyboard focus.
pub fn raise_overlay(title: &str, frame: (f64, f64, f64, f64)) {
    let main_h = unsafe { CGDisplayBounds(CGMainDisplayID()) }.size.height;
    let (x, y, w, h) = frame;
    let rect = NSRect::new(NSPoint::new(x, main_h - (y + h)), NSSize::new(w, h));
    with_window(title, |window| {
        window.setLevel(1000); // NSScreenSaverWindowLevel
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        window.setFrame_display(rect, true);
        window.makeKeyAndOrderFront(None);
    });
    if let Some(mtm) = MainThreadMarker::new() {
        #[allow(deprecated)] // `activate` needs macOS 14; this works everywhere.
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }
}

/// Keeps a small window (recording bar, notice) floating above other apps on every Space.
pub fn float_window(title: &str) {
    with_window(title, |window| {
        window.setLevel(25); // NSStatusWindowLevel
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        window.setHidesOnDeactivate(false);
    });
}

/// Hands keyboard focus back to the app the user was in before the overlay.
pub fn yield_focus() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).deactivate();
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and DragSource has no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SnapCapDragSource"]
    struct DragSource;

    unsafe impl NSObjectProtocol for DragSource {}

    unsafe impl NSDraggingSource for DragSource {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn source_operation_mask(&self, _session: &NSDraggingSession, _context: NSDraggingContext) -> NSDragOperation {
            // Copy only: Finder would otherwise move the file out of the captures folder.
            NSDragOperation::Copy
        }

        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn ended(&self, _session: &NSDraggingSession, _point: NSPoint, operation: NSDragOperation) {
            super::finish_drag(operation != NSDragOperation::None);
        }
    }
);

impl DragSource {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    static DRAG_SOURCE: OnceCell<Retained<DragSource>> = const { OnceCell::new() };
}

/// Largest side of the image that follows the pointer, in points.
const DRAG_IMAGE_MAX: f64 = 220.0;

/// Starts a system drag of the file at `path` from the window titled `title`, the
/// same kind of drag Finder starts, so the file can be dropped on any app, folder,
/// browser page or Dock icon. Call while the left mouse button is held down.
/// The drag runs on its own; `take_drag_end` reports how it finished.
pub fn start_file_drag(title: &str, path: &Path) -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let Some(path) = path.to_str() else { return false };
    let wanted = NSString::from_str(title);
    let app = NSApplication::sharedApplication(mtm);
    // The invisible 1×1 root window shares the main window's title. Don't match on the
    // pointer position: a quick flick can leave the window before egui reports the drag.
    let Some(window) = app.windows().iter().find(|w| {
        let f = w.frame();
        w.title().isEqualToString(&wanted) && w.isVisible() && f.size.width > 1.0 && f.size.height > 1.0
    }) else {
        return false;
    };
    let Some(view) = window.contentView() else { return false };

    let path = NSString::from_str(path);
    let url = NSURL::fileURLWithPath(&path);
    let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), ProtocolObject::from_ref(&*url));

    // The capture itself for images; the Finder icon for videos.
    let (image, size) = match NSImage::initWithContentsOfFile(NSImage::alloc(), &path) {
        Some(img) if img.size().width > 0.0 && img.size().height > 0.0 => {
            let s = img.size();
            let fit = (DRAG_IMAGE_MAX / s.width).min(DRAG_IMAGE_MAX / s.height).min(1.0);
            (img, NSSize::new(s.width * fit, s.height * fit))
        }
        _ => (NSWorkspace::sharedWorkspace().iconForFile(&path), NSSize::new(96.0, 96.0)),
    };
    let in_window = window.mouseLocationOutsideOfEventStream();
    let at = view.convertPoint_fromView(in_window, None);
    let frame = NSRect::new(NSPoint::new(at.x - size.width / 2.0, at.y - size.height / 2.0), size);
    let contents: &AnyObject = &image;
    unsafe { item.setDraggingFrame_contents(frame, Some(contents)) };

    // egui runs after AppKit has dispatched the mouse event, so build the drag event here.
    let Some(event) = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        NSEventType::LeftMouseDragged,
        in_window,
        NSEventModifierFlags::empty(),
        NSProcessInfo::processInfo().systemUptime(),
        window.windowNumber(),
        None,
        0,
        1,
        1.0,
    ) else {
        return false;
    };

    let source = DRAG_SOURCE.with(|s| s.get_or_init(|| DragSource::new(mtm)).clone());
    super::begin_drag();
    view.beginDraggingSessionWithItems_event_source(&NSArray::from_retained_slice(&[item]), &event, ProtocolObject::from_ref(&*source));
    true
}

pub fn is_wayland() -> bool {
    false
}

unsafe extern "C" {
    fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
}

/// Returns freed memory to the OS after large one-off allocations (5K screenshots
/// are ~60 MB each); otherwise the allocator keeps it and the idle footprint stays high.
pub fn release_memory() {
    unsafe {
        malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
    }
}
