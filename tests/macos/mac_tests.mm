// macOS integration tests: AppKit windows, ImageIO interop, key codes, IPC.
// They need a window server session but no Screen Recording permission.
#define DOCTEST_CONFIG_IMPLEMENT
#include <doctest/doctest.h>

#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <ImageIO/ImageIO.h>

#include <atomic>
#include <chrono>
#include <cstring>
#include <cstdlib>
#include <fstream>
#include <thread>

#include "../../platform/posix/ipc.h"
#include "../test_util.h"
#include "keymap.h"
#include "mac_util.h"
#import "main_window.h"
#import "notice_panel.h"
#include "recorder.h"
#import "selector.h"
#include "snapcap/gif.h"
#include "snapcap/icon.h"
#include "snapcap/log.h"
#include "snapcap/png.h"

using namespace snapcap;
using namespace std::chrono_literals;

namespace {

CGImageSourceRef open_image(const std::filesystem::path& p) {
  return CGImageSourceCreateWithURL((__bridge CFURLRef)mac::file_url(p), nullptr);
}

// Renders a view into a PNG, for looking at UI without a screen capture.
void snapshot(NSView* view, const std::filesystem::path& out) {
  NSBitmapImageRep* rep = [view bitmapImageRepForCachingDisplayInRect:view.bounds];
  // Paint the window background first, as the window itself would.
  [NSGraphicsContext saveGraphicsState];
  NSGraphicsContext.currentContext = [NSGraphicsContext graphicsContextWithBitmapImageRep:rep];
  [view.effectiveAppearance performAsCurrentDrawingAppearance:^{
    [NSColor.windowBackgroundColor setFill];
    NSRectFill(NSMakeRect(0, 0, rep.pixelsWide, rep.pixelsHigh));
  }];
  [NSGraphicsContext restoreGraphicsState];
  [view cacheDisplayInRect:view.bounds toBitmapImageRep:rep];
  NSData* png = [rep representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
  [png writeToFile:mac::ns(out.string()) atomically:YES];
}

}  // namespace

TEST_CASE("coordinates round-trip between AppKit and top-left space") {
  RectF r{120, 80, 340, 84};
  CHECK(mac::to_top_left(mac::to_cocoa(r)) == r);
  auto ms = mac::monitors();
  REQUIRE_FALSE(ms.empty());
  int primaries = 0;
  for (const auto& m : ms) primaries += m.primary;
  CHECK(primaries == 1);
  // The primary display's top-left is the origin of the desktop.
  for (const auto& m : ms) {
    if (m.primary) CHECK((m.x == 0 && m.y == 0));
  }
}

TEST_CASE("the notice appears in the bottom-right corner of the capture's display, above the Dock") {
  NoticeCallbacks callbacks{[](const auto&) {}, [](const auto&) {}};
  SNCNoticePanel* panel = [[SNCNoticePanel alloc] initWithCallbacks:callbacks protectFromCapture:NO];
  for (const MonitorInfo& m : mac::monitors()) {
    Notice n;
    n.title = "Screenshot saved";
    n.detail = "Screenshot 2026-10-22 at 10.48.33.png\nCopied to clipboard";
    n.path = "/tmp/Screenshot 2026-10-22 at 10.48.33.png";
    n.thumbnail = std::make_shared<RgbaImage>(app_icon(256));
    n.monitor_id = m.id;
    [panel showNotice:n];

    NSScreen* screen = mac::screen_for_display(m.id);
    bool has_screen = screen != nil;
    REQUIRE(has_screen);
    NSRect visible = screen.visibleFrame;  // excludes the menu bar and the Dock
    NSRect f = panel.frame;
    CAPTURE(m.name);
    CHECK(f.size.width == doctest::Approx(mac::kNoticeWidth));
    CHECK(f.size.height == doctest::Approx(mac::kNoticeHeight));
    CHECK(NSMaxX(f) == doctest::Approx(NSMaxX(visible) - 16));  // right edge
    CHECK(NSMinY(f) == doctest::Approx(NSMinY(visible) + 16));  // bottom edge, above the Dock
    CHECK(NSContainsRect(visible, f));
    CHECK(panel.visible);
  }
  if (const char* dir = std::getenv("SNAPCAP_SNAPSHOT_DIR")) snapshot(panel.contentView, std::filesystem::path(dir) / "notice.png");

  Notice err;
  err.kind = NoticeKind::Error;
  err.title = "Capture failed";
  err.detail = "The screen could not be captured (check screen recording permission)";
  [panel showNotice:err];  // no display id: the primary display
  NSRect visible = NSScreen.screens.firstObject.visibleFrame;
  CHECK(NSMaxX(panel.frame) == doctest::Approx(NSMaxX(visible) - 16));
  CHECK(NSMinY(panel.frame) == doctest::Approx(NSMinY(visible) + 16));
  if (const char* dir = std::getenv("SNAPCAP_SNAPSHOT_DIR")) snapshot(panel.contentView, std::filesystem::path(dir) / "notice-error.png");

  [panel dismiss];
  CHECK_FALSE(panel.visible);
}

TEST_CASE("the notice never becomes key, so it never steals focus") {
  NoticeCallbacks callbacks{[](const auto&) {}, [](const auto&) {}};
  SNCNoticePanel* panel = [[SNCNoticePanel alloc] initWithCallbacks:callbacks protectFromCapture:YES];
  CHECK_FALSE(panel.canBecomeKeyWindow);
  CHECK_FALSE(panel.canBecomeMainWindow);
  CHECK(panel.sharingType == NSWindowSharingNone);
  // Above the Dock (level 20), so a revealed Dock never covers it.
  CHECK(panel.level == NSStatusWindowLevel);
  CHECK(panel.level > CGWindowLevelForKey(kCGDockWindowLevelKey));
  CHECK((panel.styleMask & NSWindowStyleMaskNonactivatingPanel) != 0);
}

TEST_CASE("PNGs from the core encoder decode identically with ImageIO") {
  TempDir dir("mac-png");
  RgbaImage img(97, 41);
  for (size_t i = 0; i < img.pixels.size(); i += 4) {
    img.pixels[i] = static_cast<uint8_t>(i * 7);
    img.pixels[i + 1] = static_cast<uint8_t>(i >> 3);
    img.pixels[i + 2] = static_cast<uint8_t>(255 - i);
    img.pixels[i + 3] = 255;
  }
  auto path = dir.path() / "x.png";
  REQUIRE(write_png(path, img));
  CGImageSourceRef src = open_image(path);
  REQUIRE(src != nullptr);
  CGImageRef cg = CGImageSourceCreateImageAtIndex(src, 0, nullptr);
  REQUIRE(cg != nullptr);
  CHECK(CGImageGetWidth(cg) == 97);
  CHECK(CGImageGetHeight(cg) == 41);
  // Read the raw decoded bytes (no colour management) and compare.
  CFDataRef data = CGDataProviderCopyData(CGImageGetDataProvider(cg));
  const uint8_t* bytes = CFDataGetBytePtr(data);
  size_t stride = CGImageGetBytesPerRow(cg);
  bool same = true;
  for (uint32_t y = 0; y < img.height && same; ++y) {
    same = std::memcmp(bytes + y * stride, img.pixel(0, y), img.stride()) == 0;
  }
  CHECK(same);
  CFRelease(data);
  CGImageRelease(cg);
  CFRelease(src);
}

TEST_CASE("GIFs from the core writer play in ImageIO with the right frames and timing") {
  TempDir dir("mac-gif");
  auto path = dir.path() / "x.gif";
  auto g = GifWriter::create(path, 120, 80);
  REQUIRE(g);
  std::vector<uint8_t> px(120 * 80 * 4);
  for (int f = 0; f < 5; ++f) {
    for (size_t i = 0; i < px.size(); i += 4) {
      size_t p = i / 4, x = p % 120, y = p / 120;
      bool box = x >= size_t(f) * 20 && x < size_t(f) * 20 + 20 && y < 40;
      px[i] = box ? 255 : 30;      // B
      px[i + 1] = box ? 40 : 120;  // G
      px[i + 2] = static_cast<uint8_t>(x * 2);  // R gradient
      px[i + 3] = 255;
    }
    VideoFrame vf{px, 120, 80, 120 * 4, PixelOrder::Bgra, snapcap::Duration{std::chrono::milliseconds(f * 200)}};
    REQUIRE((*g)->push(vf));
  }
  REQUIRE((*g)->finish(1000ms));
  g->reset();

  CGImageSourceRef src = open_image(path);
  REQUIRE(src != nullptr);
  CHECK(CGImageSourceGetCount(src) == 5);
  double total = 0;
  for (size_t i = 0; i < CGImageSourceGetCount(src); ++i) {
    NSDictionary* props = (__bridge_transfer NSDictionary*)CGImageSourceCopyPropertiesAtIndex(src, i, nullptr);
    NSDictionary* gif = props[(__bridge NSString*)kCGImagePropertyGIFDictionary];
    total += [gif[(__bridge NSString*)kCGImagePropertyGIFUnclampedDelayTime] doubleValue];
  }
  CHECK(total == doctest::Approx(1.0).epsilon(0.02));
  CGImageRef last = CGImageSourceCreateImageAtIndex(src, 4, nullptr);
  REQUIRE(last != nullptr);
  CHECK(CGImageGetWidth(last) == 120);
  CGImageRelease(last);
  CFRelease(src);
}

TEST_CASE("every canonical key has a macOS key code and maps back") {
  for (const char* key : {"A", "Z", "0", "9", "F1", "F12", "Space", "Enter", "Tab", "Delete", "Home", "End", "PageUp",
                          "PageDown", "Up", "Down", "Left", "Right", "-", "=", ",", ".", "/", "\\", ";", "'", "`", "[", "]"}) {
    auto code = mac::key_code(key);
    REQUIRE_MESSAGE(code.has_value(), key);
    CHECK(mac::key_name(*code) == std::string(key));
  }
  CHECK_FALSE(mac::key_code("PrintScreen").has_value());
  for (const auto& [action, text] : default_shortcuts()) {
    if (text.empty()) continue;
    auto sc = parse_shortcut(text);
    REQUIRE(sc);
    CHECK(mac::key_code(sc->key).has_value());
    CHECK(mac::key_equivalent(*sc).has_value());
  }
}

TEST_CASE("a second launch forwards its action over IPC") {
  CHECK_FALSE(ipc::send_to_running(Action::ShotRegion));  // nobody listening yet
  std::atomic<int> got{-1};
  ipc::Server server;
  REQUIRE(server.start([&](Action a) { got = static_cast<int>(a); }));
  CHECK(ipc::socket_path().string().size() < 104);
  CHECK(ipc::send_to_running(Action::ShotRegion));
  for (int i = 0; i < 100 && got < 0; ++i) std::this_thread::sleep_for(10ms);
  CHECK(got == static_cast<int>(Action::ShotRegion));
  server.stop();
  CHECK_FALSE(ipc::send_to_running(Action::ShotRegion));
}

namespace {

std::shared_ptr<const std::vector<Shot>> fake_shots() {
  auto shots = std::make_shared<std::vector<Shot>>();
  for (const MonitorInfo& m : mac::monitors()) {
    auto w = static_cast<uint32_t>(m.width * m.scale), h = static_cast<uint32_t>(m.height * m.scale);
    RgbaImage img(w, h, {40, 44, 52, 255});
    for (uint32_t y = 0; y < h; ++y) {
      for (uint32_t x = 0; x < w; ++x) {
        if ((x / 64 + y / 64) % 2 == 0) img.pixel(x, y)[1] = static_cast<uint8_t>(80 + x * 120 / w);
      }
    }
    shots->push_back({m, std::move(img)});
  }
  return shots;
}

NSWindow* overlay_window(size_t i) {
  NSString* title = [NSString stringWithFormat:@"SnapCap Selection %zu", i + 1];
  for (NSWindow* w in NSApp.windows) {
    if ([w.title isEqualToString:title] && w.visible) return w;
  }
  return nil;
}

void send_mouse(NSView* view, NSEventType type, NSPoint view_point, NSInteger clicks = 1) {
  NSPoint p = [view convertPoint:view_point toView:nil];
  NSEvent* e = [NSEvent mouseEventWithType:type
                                  location:p
                             modifierFlags:0
                                 timestamp:0
                              windowNumber:view.window.windowNumber
                                   context:nil
                               eventNumber:0
                                clickCount:clicks
                                  pressure:1];
  switch (type) {
    case NSEventTypeLeftMouseDown: [view mouseDown:e]; break;
    case NSEventTypeLeftMouseDragged: [view mouseDragged:e]; break;
    case NSEventTypeLeftMouseUp: [view mouseUp:e]; break;
    case NSEventTypeMouseMoved: [view mouseMoved:e]; break;
    default: break;
  }
}

void send_key(NSView* view, unsigned short code) {
  NSEvent* e = [NSEvent keyEventWithType:NSEventTypeKeyDown
                                location:NSZeroPoint
                           modifierFlags:0
                               timestamp:0
                            windowNumber:view.window.windowNumber
                                 context:nil
                              characters:@""
             charactersIgnoringModifiers:@""
                               isARepeat:NO
                                 keyCode:code];
  [view keyDown:e];
}

}  // namespace

TEST_CASE("region selector: drag, adjust and confirm a recording area") {
  auto shots = fake_shots();
  std::optional<std::optional<Selection>> result;
  SNCSelector* sel = [[SNCSelector alloc] initWithPurpose:SelectionPurpose::Record
                                                    shots:shots
                                                    focus:0
                                                     done:[&](std::optional<Selection> s) { result = s; }];
  [sel show];
  NSWindow* w = overlay_window(0);
  bool has_window = w != nil;
  REQUIRE(has_window);
  NSScreen* screen = mac::screen_for_display((*shots)[0].monitor.id);
  CHECK(NSEqualRects(w.frame, screen.frame));  // covers the whole display, menu bar and Dock included
  CHECK(w.level == NSScreenSaverWindowLevel);
  NSView* v = w.contentView;

  send_mouse(v, NSEventTypeLeftMouseDown, NSMakePoint(200, 150));
  send_mouse(v, NSEventTypeLeftMouseDragged, NSMakePoint(400, 250));
  send_mouse(v, NSEventTypeLeftMouseDragged, NSMakePoint(700, 450));
  send_mouse(v, NSEventTypeLeftMouseUp, NSMakePoint(700, 450));
  CHECK_FALSE(result.has_value());  // recording waits for confirmation
  send_mouse(v, NSEventTypeMouseMoved, NSMakePoint(900, 300));
  if (const char* dir = std::getenv("SNAPCAP_SNAPSHOT_DIR")) snapshot(v, std::filesystem::path(dir) / "selector-record.png");

  // Move the selection by dragging inside it.
  send_mouse(v, NSEventTypeLeftMouseDown, NSMakePoint(450, 300));
  send_mouse(v, NSEventTypeLeftMouseDragged, NSMakePoint(500, 320));
  send_mouse(v, NSEventTypeLeftMouseUp, NSMakePoint(500, 320));

  send_key(v, 36);  // Return
  REQUIRE(result.has_value());
  REQUIRE(result->has_value());
  double s = (*shots)[0].image.width / v.bounds.size.width;
  const Selection& got = **result;
  CHECK(got.shot == 0);
  CHECK(got.rect.x == static_cast<uint32_t>(std::lround(250 * s)));
  CHECK(got.rect.y == static_cast<uint32_t>(std::lround(170 * s)));
  CHECK(got.rect.w == static_cast<uint32_t>(std::lround(500 * s)));
  CHECK(got.rect.h == static_cast<uint32_t>(std::lround(300 * s)));
  CHECK_FALSE(got.full);
  [sel close];
  CHECK_FALSE(w.visible);
}

TEST_CASE("region selector: Escape cancels and a click takes the whole screen") {
  auto shots = fake_shots();
  {
    std::optional<std::optional<Selection>> result;
    SNCSelector* sel = [[SNCSelector alloc] initWithPurpose:SelectionPurpose::Screenshot
                                                      shots:shots
                                                      focus:0
                                                       done:[&](std::optional<Selection> s) { result = s; }];
    [sel show];
    NSView* v = overlay_window(0).contentView;
    send_mouse(v, NSEventTypeMouseMoved, NSMakePoint(300, 200));
    if (const char* dir = std::getenv("SNAPCAP_SNAPSHOT_DIR")) snapshot(v, std::filesystem::path(dir) / "selector-shot.png");
    send_key(v, 53);  // Escape
    REQUIRE(result.has_value());
    CHECK_FALSE(result->has_value());
    send_key(v, 36);  // further input after the decision is ignored
    [sel close];
  }
  {
    std::optional<std::optional<Selection>> result;
    SNCSelector* sel = [[SNCSelector alloc] initWithPurpose:SelectionPurpose::Screenshot
                                                      shots:shots
                                                      focus:0
                                                       done:[&](std::optional<Selection> s) { result = s; }];
    [sel show];
    NSView* v = overlay_window(0).contentView;
    send_mouse(v, NSEventTypeLeftMouseDown, NSMakePoint(300, 200));
    send_mouse(v, NSEventTypeLeftMouseUp, NSMakePoint(300, 200));
    REQUIRE(result.has_value());
    REQUIRE(result->has_value());
    CHECK((*result)->full);
    [sel close];
  }
}

namespace {

bool can_capture_screen() {
  if (!CGPreflightScreenCaptureAccess()) return false;
  uint32_t n = 0;
  CGGetActiveDisplayList(0, nullptr, &n);
  return n > 0;  // no active display while the screen is locked or asleep
}

// Runs the main run loop until `done` or the timeout.
template <class F>
bool spin_until(F&& done, std::chrono::milliseconds timeout) {
  auto end = std::chrono::steady_clock::now() + timeout;
  while (!done() && std::chrono::steady_clock::now() < end) {
    [NSRunLoop.mainRunLoop runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.02]];
  }
  return done();
}

Result<RecordingResult> record_region(VideoFormat format, const std::filesystem::path& path, RecordConfig& cfg) {
  MonitorInfo m = mac::monitors()[0];
  for (const auto& mon : mac::monitors()) {
    if (mon.primary) m = mon;
  }
  cfg.target = CaptureTarget{m, static_cast<uint32_t>(m.width * m.scale), static_cast<uint32_t>(m.height * m.scale),
                             PxRect{100, 120, 641, 361}};
  cfg.format = format;
  cfg.fps = format == VideoFormat::Gif ? 10 : 30;
  cfg.path = path;
  PxRect r = cfg.target.pixel_rect();
  std::tie(cfg.out_width, cfg.out_height) = output_size(format, ResolutionCap::Native, 320, r.w, r.h);
  std::optional<Result<RecordingResult>> result;
  auto rec = mac::start_recorder(cfg, [&](Result<RecordingResult> r2) { result = std::move(r2); });
  REQUIRE(rec);
  spin_until([] { return false; }, 1500ms);
  (*rec)->toggle_pause();
  CHECK((*rec)->paused());
  spin_until([] { return false; }, 500ms);
  (*rec)->toggle_pause();
  spin_until([] { return false; }, 500ms);
  (*rec)->stop();
  REQUIRE(spin_until([&] { return result.has_value(); }, 15s));
  return std::move(*result);
}

}  // namespace

TEST_CASE("recorder: region MP4 has the right size and excludes the paused time") {
  if (!can_capture_screen()) {
    MESSAGE("skipped: needs Screen Recording permission and an unlocked display");
    return;
  }
  TempDir dir("mac-rec");
  RecordConfig cfg;
  auto result = record_region(VideoFormat::Mp4, dir.path() / "region.mp4", cfg);
  REQUIRE_MESSAGE(result.has_value(), (result ? "" : result.error().message));
  CHECK(cfg.out_width == 640);
  CHECK(cfg.out_height == 360);
  CHECK(result->duration > 1500ms);
  CHECK(result->duration < 2800ms);  // ~2.5 s wall time minus the 0.5 s pause
  AVURLAsset* asset = [AVURLAsset URLAssetWithURL:mac::file_url(result->path) options:nil];
  NSMutableArray<AVAssetTrack*>* tracks = [NSMutableArray array];
  auto loaded = std::make_shared<std::atomic<bool>>(false);
  [asset loadTracksWithMediaType:AVMediaTypeVideo
               completionHandler:^(NSArray<AVAssetTrack*>* t, NSError*) {
                 dispatch_async(dispatch_get_main_queue(), ^{
                   [tracks addObjectsFromArray:t ?: @[]];
                   *loaded = true;
                 });
               }];
  REQUIRE(spin_until([loaded] { return loaded->load(); }, 5s));
  REQUIRE(tracks.count == 1);
  CHECK(tracks[0].naturalSize.width == 640);
  CHECK(tracks[0].naturalSize.height == 360);
}

TEST_CASE("recorder: region GIF decodes with frames of the output size") {
  if (!can_capture_screen()) {
    MESSAGE("skipped: needs Screen Recording permission and an unlocked display");
    return;
  }
  TempDir dir("mac-rec-gif");
  RecordConfig cfg;
  auto result = record_region(VideoFormat::Gif, dir.path() / "region.gif", cfg);
  REQUIRE_MESSAGE(result.has_value(), (result ? "" : result.error().message));
  CHECK(cfg.out_width == 320);
  std::ifstream in(result->path, std::ios::binary);
  std::vector<uint8_t> bytes((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
  auto gif = decode_gif(bytes);
  REQUIRE(gif);
  CHECK(gif->width == cfg.out_width);
  CHECK(gif->height == cfg.out_height);
  CHECK(gif->frames.size() >= 1);
}

namespace {

// A host that does nothing, for exercising windows against a real AppController.
struct StubHost : Host {
  bool permission = true;
  std::vector<std::function<void()>> posted;
  void post(std::function<void()> fn) override { posted.push_back(std::move(fn)); }
  void post_delayed(std::chrono::milliseconds, std::function<void()>) override {}
  void run_background(std::function<void()> work) override { work(); }
  std::vector<MonitorInfo> monitors() override { return mac::monitors(); }
  std::optional<PointF> cursor_position() override { return std::nullopt; }
  bool has_capture_permission() override { return permission; }
  void request_capture_permission() override {}
  void capture(std::vector<MonitorInfo>, std::function<void(Result<std::vector<Shot>>)>) override {}
  Result<std::unique_ptr<Recorder>> start_recorder(const RecordConfig&, RecordingDone) override { return fail("no"); }
  void set_main_window_visible(bool) override {}
  void set_dock_visible(bool) override {}
  void show_selector(SelectionPurpose, std::shared_ptr<const std::vector<Shot>>, size_t) override {}
  void hide_selector() override {}
  void show_recording_ui(const CaptureTarget&, const BarState&) override {}
  void hide_recording_ui() override {}
  void show_notice(const Notice&) override {}
  void hide_notice() override {}
  void yield_focus() override {}
  void apply_theme(ThemePref) override {}
  void recent_changed(const std::vector<std::filesystem::path>&) override {}
  void state_changed() override {}
  bool tray_available() override { return true; }
  void set_tray_recording(bool) override {}
  void set_tray_shortcuts(const ShortcutMap&) override {}
  void unregister_hotkeys() override {}
  Result<void> register_hotkey(Action, const Shortcut&) override { return {}; }
  void copy_png_to_clipboard(std::shared_ptr<const std::vector<uint8_t>>) override {}
  void open_path(const std::filesystem::path&) override {}
  void reveal_path(const std::filesystem::path&) override {}
  void terminate() override {}
};

NSButton* find_button(NSView* root, NSString* tooltip) {
  for (NSView* v in root.subviews) {
    if ([v isKindOfClass:NSButton.class] && [v.toolTip isEqualToString:tooltip]) return (NSButton*)v;
    if (NSButton* b = find_button(v, tooltip)) return b;
  }
  return nil;
}

}  // namespace

TEST_CASE("main window: home and settings pages build and render") {
  TempDir caps("mac-main");
  for (const char* name : {"Screenshot 1.png", "Recording 2.mp4"}) {
    REQUIRE(write_png(caps.path() / name, app_icon(64)));
  }
  StubHost host;
  host.permission = false;
  Settings s;
  s.save_dir = caps.path();
  s.shortcuts[Action::ShotAllScreens] = "Alt+Shift+1";  // a duplicate, to show the warning
  auto controller = AppController::create(host, s);
  controller->start(std::nullopt, true);
  for (size_t i = 0; i < host.posted.size(); ++i) host.posted[i]();  // deliver the recent-files scan
  MainWindowServices services{[](const auto&) {}, [](const auto&) {}, [] { return std::vector<std::string>{"Built-in"}; }};
  SNCMainWindow* w = [[SNCMainWindow alloc] initWithController:controller services:services];
  [w setFrame:NSMakeRect(0, 0, 600, 760) display:NO];
  [w reload];
  [w.contentView layoutSubtreeIfNeeded];
  if (const char* dir = std::getenv("SNAPCAP_SNAPSHOT_DIR")) snapshot(w.contentView, std::filesystem::path(dir) / "main-home.png");

  NSButton* gear = find_button(w.contentView, @"Settings");
  bool has_gear = gear != nil;
  REQUIRE(has_gear);
  [gear performClick:nil];
  [w.contentView layoutSubtreeIfNeeded];
  bool has_back = find_button(w.contentView, @"Back") != nil;
  CHECK(has_back);
  if (const char* dir = std::getenv("SNAPCAP_SNAPSHOT_DIR")) {
    // Render the whole settings page, not just the visible part.
    NSScrollView* scroll = nil;
    for (NSView* v in w.contentView.subviews) {
      if ([v isKindOfClass:NSScrollView.class]) scroll = (NSScrollView*)v;
    }
    if (scroll != nil) snapshot(scroll.documentView, std::filesystem::path(dir) / "main-settings.png");
  }
  [w orderOut:nil];
}

namespace {

// Paints a fake capture for the README's "Recent" row.
void demo_capture(const std::filesystem::path& path, int style) {
  const int W = 1440, H = 900;
  NSBitmapImageRep* rep = [[NSBitmapImageRep alloc] initWithBitmapDataPlanes:nullptr pixelsWide:W pixelsHigh:H
                                                              bitsPerSample:8 samplesPerPixel:4 hasAlpha:YES isPlanar:NO
                                                             colorSpaceName:NSCalibratedRGBColorSpace bytesPerRow:0 bitsPerPixel:0];
  [NSGraphicsContext saveGraphicsState];
  NSGraphicsContext.currentContext = [NSGraphicsContext graphicsContextWithBitmapImageRep:rep];
  auto rgb = [](int r, int g, int b) { return [NSColor colorWithSRGBRed:r / 255.0 green:g / 255.0 blue:b / 255.0 alpha:1]; };
  auto box = [](NSColor* c, NSRect r, CGFloat radius) {
    [c setFill];
    [[NSBezierPath bezierPathWithRoundedRect:r xRadius:radius yRadius:radius] fill];
  };
  auto text = [](NSString* t, NSPoint at, CGFloat size, NSColor* c, NSFontWeight w) {
    [t drawAtPoint:at withAttributes:@{NSFontAttributeName : [NSFont systemFontOfSize:size weight:w], NSForegroundColorAttributeName : c}];
  };
  switch (style) {
    case 0: {  // a light kanban board
      box(rgb(241, 243, 248), NSMakeRect(0, 0, W, H), 0);
      box(rgb(255, 255, 255), NSMakeRect(0, H - 70, W, 70), 0);
      text(@"Sprint board", NSMakePoint(40, H - 50), 28, rgb(30, 32, 40), NSFontWeightBold);
      const char* cols[] = {"To do", "In progress", "Review", "Done"};
      for (int c = 0; c < 4; ++c) {
        CGFloat x = 40 + c * 345;
        box(rgb(228, 231, 240), NSMakeRect(x, 60, 320, H - 160), 14);
        text(@(cols[c]), NSMakePoint(x + 20, H - 140), 20, rgb(80, 84, 100), NSFontWeightSemibold);
        for (int i = 0; i < 4 - (c % 2); ++i) {
          box(NSColor.whiteColor, NSMakeRect(x + 16, H - 250 - i * 130, 288, 110), 10);
          box(@[ rgb(108, 92, 231), rgb(255, 90, 95), rgb(80, 180, 120), rgb(245, 176, 65) ][(c + i) % 4],
              NSMakeRect(x + 32, H - 175 - i * 130, 70, 14), 7);
          box(rgb(220, 223, 232), NSMakeRect(x + 32, H - 210 - i * 130, 220, 12), 6);
          box(rgb(220, 223, 232), NSMakeRect(x + 32, H - 232 - i * 130, 150, 12), 6);
        }
      }
      break;
    }
    case 1: {  // a terminal
      box(rgb(26, 27, 38), NSMakeRect(0, 0, W, H), 0);
      const char* lines[] = {"$ cmake --preset release", "-- Configuring done", "$ cmake --build --preset release",
                             "[42/42] Linking SnapCap.app", "$ ctest --preset release", "100% tests passed, 0 tests failed",
                             "$ open dist/SnapCap.app", "$ _"};
      for (int i = 0; i < 8; ++i) {
        text(@(lines[i]), NSMakePoint(50, H - 90 - i * 52), 28,
             i % 2 == 0 ? rgb(170, 220, 160) : rgb(200, 205, 220), NSFontWeightRegular);
      }
      break;
    }
    case 2: {  // a dashboard
      box(rgb(17, 20, 32), NSMakeRect(0, 0, W, H), 0);
      text(@"Weekly active users", NSMakePoint(60, H - 90), 30, NSColor.whiteColor, NSFontWeightBold);
      for (int i = 0; i < 3; ++i) {
        box(rgb(32, 37, 58), NSMakeRect(60 + i * 450, H - 260, 420, 130), 16);
        text(@[ @"12,480", @"1,894", @"37%" ][i], NSMakePoint(90 + i * 450, H - 220), 40, NSColor.whiteColor, NSFontWeightSemibold);
      }
      int heights[] = {120, 160, 140, 210, 190, 250, 230, 290, 270, 340, 320, 380};
      for (int i = 0; i < 12; ++i) {
        box(rgb(132, 118, 255), NSMakeRect(80 + i * 108, 70, 70, heights[i]), 8);
      }
      break;
    }
    default: {  // a landing page
      NSGradient* g = [[NSGradient alloc] initWithStartingColor:rgb(255, 120, 170) endingColor:rgb(132, 118, 255)];
      [g drawInRect:NSMakeRect(0, 0, W, H) angle:-30];
      text(@"Weekend Trips", NSMakePoint(W / 2 - 300, H / 2 + 20), 96, rgb(40, 20, 60), NSFontWeightHeavy);
      text(@"Plan escapes with friends in minutes.", NSMakePoint(W / 2 - 260, H / 2 - 60), 34, rgb(60, 30, 80), NSFontWeightMedium);
      box(rgb(40, 20, 60), NSMakeRect(W / 2 - 110, H / 2 - 170, 220, 70), 35);
      text(@"Get started", NSMakePoint(W / 2 - 80, H / 2 - 150), 30, NSColor.whiteColor, NSFontWeightSemibold);
      break;
    }
  }
  [NSGraphicsContext restoreGraphicsState];
  [[rep representationUsingType:NSBitmapImageFileTypePNG properties:@{}] writeToFile:mac::ns(path.string()) atomically:YES];
}

// Renders a whole window (title bar included) with rounded corners and a drop shadow.
void window_shot(NSWindow* w, const std::filesystem::path& out) {
  NSView* frame = w.contentView.superview;
  NSBitmapImageRep* rep = [frame bitmapImageRepForCachingDisplayInRect:frame.bounds];
  [NSGraphicsContext saveGraphicsState];
  NSGraphicsContext.currentContext = [NSGraphicsContext graphicsContextWithBitmapImageRep:rep];
  [frame.effectiveAppearance performAsCurrentDrawingAppearance:^{
    [NSColor.windowBackgroundColor setFill];
    NSRectFill(NSMakeRect(0, 0, rep.pixelsWide, rep.pixelsHigh));
  }];
  [NSGraphicsContext restoreGraphicsState];
  [frame cacheDisplayInRect:frame.bounds toBitmapImageRep:rep];

  const CGFloat scale = rep.pixelsWide / frame.bounds.size.width;
  const CGFloat pad = 40 * scale, radius = 12 * scale;
  NSSize size = NSMakeSize(rep.pixelsWide + 2 * pad, rep.pixelsHigh + 2 * pad);
  NSBitmapImageRep* outRep = [[NSBitmapImageRep alloc] initWithBitmapDataPlanes:nullptr pixelsWide:static_cast<NSInteger>(size.width)
                                                                 pixelsHigh:static_cast<NSInteger>(size.height) bitsPerSample:8
                                                            samplesPerPixel:4 hasAlpha:YES isPlanar:NO
                                                             colorSpaceName:NSCalibratedRGBColorSpace bytesPerRow:0 bitsPerPixel:0];
  [NSGraphicsContext saveGraphicsState];
  NSGraphicsContext.currentContext = [NSGraphicsContext graphicsContextWithBitmapImageRep:outRep];
  NSRect r = NSMakeRect(pad, pad, rep.pixelsWide, rep.pixelsHigh);
  NSBezierPath* shape = [NSBezierPath bezierPathWithRoundedRect:r xRadius:radius yRadius:radius];
  NSShadow* shadow = [[NSShadow alloc] init];
  shadow.shadowColor = [NSColor colorWithWhite:0 alpha:0.45];
  shadow.shadowBlurRadius = 24 * scale;
  shadow.shadowOffset = NSMakeSize(0, -8 * scale);
  [NSGraphicsContext saveGraphicsState];
  [shadow set];
  [NSColor.blackColor setFill];
  [shape fill];
  [NSGraphicsContext restoreGraphicsState];
  [NSGraphicsContext saveGraphicsState];
  [shape addClip];
  NSImage* img = [[NSImage alloc] initWithSize:r.size];
  [img addRepresentation:rep];
  [img drawInRect:r];
  [NSGraphicsContext restoreGraphicsState];
  [[NSColor colorWithWhite:1 alpha:0.18] setStroke];
  shape.lineWidth = scale;
  [shape stroke];
  [NSGraphicsContext restoreGraphicsState];
  [[outRep representationUsingType:NSBitmapImageFileTypePNG properties:@{}] writeToFile:mac::ns(out.string()) atomically:YES];
}

}  // namespace

// Opt-in: SNAPCAP_DOC_DIR=docs/screenshots snapcap_mac_tests -tc="README*"
TEST_CASE("README screenshots of the main window") {
  const char* out = std::getenv("SNAPCAP_DOC_DIR");
  if (out == nullptr) return;
  TempDir home("doc-home");
  setenv("HOME", home.path().c_str(), 1);  // shows ~/Desktop/screenshots
  auto caps = home.path() / "Desktop" / "screenshots";
  std::filesystem::create_directories(caps);
  for (int i = 0; i < 4; ++i) {
    auto p = caps / std::format("Screenshot 2026-10-22 at 10.4{}.00.png", i);
    demo_capture(p, 3 - i);
    std::filesystem::last_write_time(p, std::filesystem::file_time_type::clock::now() - std::chrono::minutes(i));
  }
  StubHost host;
  auto controller = AppController::create(host, Settings{});
  controller->start(std::nullopt, true);
  for (size_t i = 0; i < host.posted.size(); ++i) host.posted[i]();
  MainWindowServices services{[](const auto&) {}, [](const auto&) {}, [] { return std::vector<std::string>{}; }};
  SNCMainWindow* w = [[SNCMainWindow alloc] initWithController:controller services:services];
  w.appearance = [NSAppearance appearanceNamed:NSAppearanceNameDarkAqua];
  [w setContentSize:NSMakeSize(600, 700)];
  [w orderFrontRegardless];
  [w makeKeyWindow];
  [w reload];
  spin_until([] { return false; }, 2500ms);  // QuickLook thumbnails
  [w reload];
  [w.contentView layoutSubtreeIfNeeded];
  window_shot(w, std::filesystem::path(out) / "main-window.png");

  NSButton* gear = find_button(w.contentView, @"Settings");
  bool has_gear = gear != nil;
  REQUIRE(has_gear);
  [gear performClick:nil];
  [w.contentView layoutSubtreeIfNeeded];
  spin_until([] { return false; }, 300ms);
  window_shot(w, std::filesystem::path(out) / "settings.png");
  [w orderOut:nil];
}

int main(int argc, char** argv) {
  @autoreleasepool {
    TempDir config("mac-config");
    setenv("SNAPCAP_CONFIG_DIR", config.path().c_str(), 1);
    if (std::getenv("SNAPCAP_TEST_LOG") == nullptr) log::set_sink([](auto, auto) {});
    [NSApplication sharedApplication];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
    doctest::Context context(argc, argv);
    return context.run();
  }
}
