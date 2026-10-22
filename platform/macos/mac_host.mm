#include "mac_host.h"

#import <CoreGraphics/CoreGraphics.h>

#include <cstdlib>

#include "capture.h"
#include "hotkeys.h"
#include "mac_util.h"
#import "main_window.h"
#import "notice_panel.h"
#include "recorder.h"
#import "recording_ui.h"
#import "selector.h"
#import "status_item.h"
#include "snapcap/log.h"

namespace snapcap::mac {

bool protect_from_capture() {
  static const bool allow = [] {
    const char* v = std::getenv("SNAPCAP_ALLOW_SELF_CAPTURE");
    return v != nullptr && *v != '\0' && std::string_view(v) != "0";
  }();
  return !allow;
}

struct MacHost::Impl {
  std::weak_ptr<AppController> controller;
  SNCStatusItem* status = nil;
  std::unique_ptr<GlobalHotkeys> hotkeys;
  SNCNoticePanel* notice = nil;
  SNCMainWindow* main_window = nil;
  SNCSelector* selector = nil;
  SNCRecordingUI* recording_ui = nil;

  template <class F>
  void with_controller(F&& f) {
    if (auto c = controller.lock()) f(*c);
  }
};

MacHost::MacHost() : impl_(std::make_unique<Impl>()) {}

MacHost::~MacHost() = default;

void MacHost::attach(std::shared_ptr<AppController> controller) {
  impl_->controller = controller;
  Impl* impl = impl_.get();
  impl_->hotkeys = std::make_unique<GlobalHotkeys>([impl](Action a) {
    impl->with_controller([a](AppController& c) { c.trigger(a); });
  });
  impl_->status = [[SNCStatusItem alloc] initWithHandler:[impl](const std::string& command) {
    impl->with_controller([&](AppController& c) {
      if (command == "quit") {
        c.quit();
      } else if (command == "open-folder") {
        c.open_save_folder();
      } else if (auto a = from_cli_name(command)) {
        c.trigger(*a);
      }
    });
  }];
  NoticeCallbacks callbacks{
      [this](const std::filesystem::path& p) { open_path(p); },
      [this](const std::filesystem::path& p) { reveal_path(p); },
  };
  impl_->notice = [[SNCNoticePanel alloc] initWithCallbacks:callbacks protectFromCapture:protect_from_capture()];
  impl_->recording_ui = [[SNCRecordingUI alloc] initWithAction:[impl](BarAction a) {
    impl->with_controller([a](AppController& c) { c.on_bar_action(a); });
  } protectFromCapture:protect_from_capture()];
  MainWindowServices services{
      [this](const std::filesystem::path& p) { open_path(p); },
      [this](const std::filesystem::path& p) { reveal_path(p); },
      [] { return microphone_names(); },
  };
  impl_->main_window = [[SNCMainWindow alloc] initWithController:controller services:services];
}

// ------------------------------------------------------------------ scheduling

void MacHost::post(std::function<void()> fn) {
  on_main(std::move(fn));
}

void MacHost::post_delayed(std::chrono::milliseconds delay, std::function<void()> fn) {
  dispatch_after(dispatch_time(DISPATCH_TIME_NOW, static_cast<int64_t>(delay.count()) * NSEC_PER_MSEC),
                 dispatch_get_main_queue(), ^{
                   fn();
                 });
}

void MacHost::run_background(std::function<void()> work) {
  dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
    @autoreleasepool {
      work();
    }
  });
}

// ------------------------------------------------------------------ displays and capture

std::vector<MonitorInfo> MacHost::monitors() { return mac::monitors(); }

std::optional<PointF> MacHost::cursor_position() { return mac::cursor_position(); }

bool MacHost::has_capture_permission() { return CGPreflightScreenCaptureAccess(); }

void MacHost::request_capture_permission() {
  // Shows the system prompt (first time only) and lists SnapCap in System Settings.
  if (!CGRequestScreenCaptureAccess()) {
    [NSWorkspace.sharedWorkspace
        openURL:[NSURL URLWithString:@"x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"]];
  }
}

void MacHost::capture(std::vector<MonitorInfo> which, std::function<void(Result<std::vector<Shot>>)> done) {
  capture_displays(std::move(which), std::move(done));
}

Result<std::unique_ptr<Recorder>> MacHost::start_recorder(const RecordConfig& config, RecordingDone done) {
  return mac::start_recorder(config, std::move(done));
}

// ------------------------------------------------------------------ windows

void MacHost::set_main_window_visible(bool visible) {
  if (visible) {
    [impl_->main_window reload];
    [impl_->main_window makeKeyAndOrderFront:nil];
    [NSApp activateIgnoringOtherApps:YES];
  } else {
    [impl_->main_window orderOut:nil];
  }
}

void MacHost::set_dock_visible(bool visible) {
  // Hidden while SnapCap only lives in the menu bar.
  [NSApp setActivationPolicy:visible ? NSApplicationActivationPolicyRegular : NSApplicationActivationPolicyAccessory];
}

void MacHost::show_selector(SelectionPurpose purpose, std::shared_ptr<const std::vector<Shot>> shots, size_t focus) {
  hide_selector();
  Impl* impl = impl_.get();
  impl_->selector = [[SNCSelector alloc] initWithPurpose:purpose
                                                   shots:std::move(shots)
                                                   focus:focus
                                                    done:[impl](std::optional<Selection> sel) {
                                                      impl->with_controller([&](AppController& c) { c.on_selection(sel); });
                                                    }];
  [impl_->selector show];
}

void MacHost::hide_selector() {
  SNCSelector* selector = impl_->selector;
  if (selector == nil) return;
  impl_->selector = nil;
  [selector close];
  // Release on the next turn of the run loop: we may be inside one of its event handlers.
  dispatch_async(dispatch_get_main_queue(), ^{
    (void)selector;
  });
}

void MacHost::show_recording_ui(const CaptureTarget& target, const BarState& state) {
  [impl_->recording_ui showFor:target state:state];
}

void MacHost::hide_recording_ui() { [impl_->recording_ui hide]; }

void MacHost::show_notice(const Notice& notice) { [impl_->notice showNotice:notice]; }

void MacHost::hide_notice() { [impl_->notice dismiss]; }

void MacHost::yield_focus() { [NSApp deactivate]; }

void MacHost::apply_theme(ThemePref theme) {
  switch (theme) {
    case ThemePref::System: NSApp.appearance = nil; break;
    case ThemePref::Dark: NSApp.appearance = [NSAppearance appearanceNamed:NSAppearanceNameDarkAqua]; break;
    case ThemePref::Light: NSApp.appearance = [NSAppearance appearanceNamed:NSAppearanceNameAqua]; break;
  }
}

void MacHost::recent_changed(const std::vector<std::filesystem::path>& files) { [impl_->main_window setRecent:files]; }

void MacHost::state_changed() { [impl_->main_window reload]; }

// ------------------------------------------------------------------ system integration

bool MacHost::tray_available() { return impl_->status != nil; }

void MacHost::set_tray_recording(bool recording) { [impl_->status setRecording:recording]; }

void MacHost::set_tray_shortcuts(const ShortcutMap& shortcuts) { [impl_->status setShortcuts:shortcuts]; }

void MacHost::unregister_hotkeys() { impl_->hotkeys->clear(); }

Result<void> MacHost::register_hotkey(Action action, const Shortcut& shortcut) {
  return impl_->hotkeys->add(action, shortcut);
}

void MacHost::copy_png_to_clipboard(std::shared_ptr<const std::vector<uint8_t>> png) {
  NSData* data = [NSData dataWithBytes:png->data() length:png->size()];
  NSPasteboard* pb = NSPasteboard.generalPasteboard;
  [pb clearContents];
  if (![pb setData:data forType:NSPasteboardTypePNG]) log::warn("could not copy the screenshot to the clipboard");
}

void MacHost::open_path(const std::filesystem::path& path) {
  [NSWorkspace.sharedWorkspace openURL:file_url(path)];
}

void MacHost::reveal_path(const std::filesystem::path& path) {
  [NSWorkspace.sharedWorkspace activateFileViewerSelectingURLs:@[ file_url(path) ]];
}

void MacHost::terminate() {
  termination_allowed_ = true;
  // Asynchronous: this may run inside applicationShouldTerminate, which must not re-enter.
  dispatch_async(dispatch_get_main_queue(), ^{
    [NSApp terminate:nil];
  });
}

}  // namespace snapcap::mac
