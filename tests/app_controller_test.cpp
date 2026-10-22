#include <algorithm>
#include <deque>
#include <fstream>
#include <map>
#include <set>

#include <doctest/doctest.h>

#include "snapcap/app_controller.h"
#include "test_util.h"

using namespace snapcap;
using namespace std::chrono_literals;

namespace {

struct FakeRecorder : Recorder {
  struct Shared {
    bool stopped = false;
    bool paused = false;
    bool destroyed = false;
    RecordingDone done;
    RecordConfig config;
  };
  std::shared_ptr<Shared> s;
  explicit FakeRecorder(std::shared_ptr<Shared> shared) : s(std::move(shared)) {}
  ~FakeRecorder() override { s->destroyed = true; }
  void stop() override { s->stopped = true; }
  void toggle_pause() override { s->paused = !s->paused; }
  bool paused() const override { return s->paused; }
  Duration elapsed() const override { return 3s; }
};

// Single-threaded host with virtual time. Background work runs when `drain()` is called.
struct FakeHost : Host {
  std::chrono::milliseconds now{0};
  std::vector<std::pair<std::chrono::milliseconds, std::function<void()>>> timers;
  std::deque<std::function<void()>> posted;
  std::deque<std::function<void()>> background;

  std::vector<MonitorInfo> displays;
  std::optional<PointF> cursor;
  bool permission = true;
  int permission_requests = 0;
  std::vector<std::vector<MonitorInfo>> capture_requests;
  std::function<void(Result<std::vector<Shot>>)> pending_capture;
  std::optional<std::string> recorder_error;
  std::shared_ptr<FakeRecorder::Shared> recorder;

  bool main_visible = false;
  bool dock_visible = false;
  bool selector_visible = false;
  size_t selector_focus = 0;
  std::optional<BarState> bar;
  std::vector<Notice> notices;
  bool tray = true;
  bool tray_recording = false;
  std::map<Action, Shortcut> hotkeys;
  std::set<std::string> taken;  // shortcuts "owned by another app"
  int clipboard_copies = 0;
  std::vector<std::filesystem::path> opened;
  std::vector<std::filesystem::path> recent;
  bool terminated = false;

  void post(std::function<void()> fn) override { posted.push_back(std::move(fn)); }
  void post_delayed(std::chrono::milliseconds d, std::function<void()> fn) override {
    timers.emplace_back(now + d, std::move(fn));
  }
  void run_background(std::function<void()> work) override { background.push_back(std::move(work)); }

  std::vector<MonitorInfo> monitors() override { return displays; }
  std::optional<PointF> cursor_position() override { return cursor; }
  bool has_capture_permission() override { return permission; }
  void request_capture_permission() override { ++permission_requests; }
  void capture(std::vector<MonitorInfo> which, std::function<void(Result<std::vector<Shot>>)> done) override {
    capture_requests.push_back(which);
    pending_capture = std::move(done);
  }
  Result<std::unique_ptr<Recorder>> start_recorder(const RecordConfig& config, RecordingDone done) override {
    if (recorder_error) return fail(*recorder_error);
    recorder = std::make_shared<FakeRecorder::Shared>();
    recorder->done = std::move(done);
    recorder->config = config;
    return std::make_unique<FakeRecorder>(recorder);
  }

  void set_main_window_visible(bool v) override { main_visible = v; }
  void set_dock_visible(bool v) override { dock_visible = v; }
  void show_selector(SelectionPurpose, std::shared_ptr<const std::vector<Shot>>, size_t focus) override {
    selector_visible = true;
    selector_focus = focus;
  }
  void hide_selector() override { selector_visible = false; }
  void show_recording_ui(const CaptureTarget&, const BarState& s) override { bar = s; }
  void hide_recording_ui() override { bar.reset(); }
  void show_notice(const Notice& n) override { notices.push_back(n); }
  void hide_notice() override {}
  void yield_focus() override {}
  void apply_theme(ThemePref) override {}
  void recent_changed(const std::vector<std::filesystem::path>& files) override { recent = files; }
  void state_changed() override {}

  bool tray_available() override { return tray; }
  void set_tray_recording(bool r) override { tray_recording = r; }
  void set_tray_shortcuts(const ShortcutMap&) override {}
  void unregister_hotkeys() override { hotkeys.clear(); }
  Result<void> register_hotkey(Action a, const Shortcut& s) override {
    if (taken.contains(s.to_string())) return fail("in use");
    hotkeys[a] = s;
    return {};
  }
  void copy_png_to_clipboard(std::shared_ptr<const std::vector<uint8_t>>) override { ++clipboard_copies; }
  void open_path(const std::filesystem::path& p) override { opened.push_back(p); }
  void reveal_path(const std::filesystem::path&) override {}
  void terminate() override { terminated = true; }

  // Runs everything that is due: posted callbacks, background work and timers up to `now + d`.
  void advance(std::chrono::milliseconds d = 0ms) {
    auto until = now + d;
    while (true) {
      drain();
      auto next = std::ranges::min_element(timers, {}, &decltype(timers)::value_type::first);
      if (next == timers.end() || next->first > until) break;
      now = next->first;
      auto fn = std::move(next->second);
      timers.erase(next);
      fn();
    }
    now = until;
  }
  void drain() {
    while (!posted.empty() || !background.empty()) {
      while (!background.empty()) {
        auto f = std::move(background.front());
        background.pop_front();
        f();
      }
      while (!posted.empty()) {
        auto f = std::move(posted.front());
        posted.pop_front();
        f();
      }
    }
  }
};

MonitorInfo display(size_t index, uint32_t id, int32_t x, bool primary) {
  MonitorInfo m;
  m.index = index;
  m.id = id;
  m.x = x;
  m.width = 100;
  m.height = 50;
  m.scale = 2.0;
  m.primary = primary;
  return m;
}

std::vector<Shot> shots_for(const std::vector<MonitorInfo>& which) {
  std::vector<Shot> out;
  for (const auto& m : which) out.push_back({m, RgbaImage(200, 100, {50, 60, 70, 255})});
  return out;
}

struct Fixture {
  TempDir dir{"controller"};
  FakeHost host;
  std::shared_ptr<AppController> app;

  explicit Fixture(uint32_t countdown = 0) {
    host.displays = {display(0, 11, 0, true), display(1, 22, 100, false)};
    host.cursor = PointF{150, 10};  // over display 22
    Settings s;
    s.save_dir = dir.path();
    s.countdown_secs = countdown;
    app = AppController::create(host, s);
    app->start(std::nullopt, /*background=*/true);
    host.advance();
  }
};

}  // namespace

TEST_CASE("screenshot of the screen under the cursor") {
  Fixture f;
  f.app->trigger(Action::ShotScreen);
  CHECK(f.app->mode_name() == "preparing");
  f.host.advance(AppController::kHideDelay);
  REQUIRE(f.host.capture_requests.size() == 1);
  CHECK(f.host.capture_requests[0].size() == 1);
  CHECK(f.host.capture_requests[0][0].id == 22);
  CHECK(f.app->mode_name() == "capturing");

  f.host.pending_capture(shots_for(f.host.capture_requests[0]));
  CHECK(f.app->idle());
  f.host.advance();
  REQUIRE(f.host.notices.size() == 1);
  const Notice& n = f.host.notices[0];
  CHECK(n.title == "Screenshot saved");
  CHECK(n.kind == NoticeKind::Success);
  CHECK(n.monitor_id == 22u);
  REQUIRE(n.path.has_value());
  CHECK(std::filesystem::file_size(*n.path) > 0);
  REQUIRE(n.thumbnail);
  CHECK(n.thumbnail->width == 200);
  CHECK(f.host.clipboard_copies == 1);
  CHECK(f.host.recent.front() == *n.path);
}

TEST_CASE("all screens are stitched into one image") {
  Fixture f;
  f.app->trigger(Action::ShotAllScreens);
  f.host.advance(AppController::kHideDelay);
  REQUIRE(f.host.capture_requests.back().size() == 2);
  f.host.pending_capture(shots_for(f.host.capture_requests.back()));
  f.host.advance();
  REQUIRE(f.host.notices.size() == 1);
  CHECK(f.host.notices[0].monitor_id == 11u);  // the primary display
  CHECK(f.host.notices[0].thumbnail->width == 320);
}

TEST_CASE("the main window hides for the capture and comes back") {
  Fixture f;
  f.app->trigger(Action::ShowWindow);
  CHECK(f.host.main_visible);
  f.app->trigger(Action::ShotScreen);
  CHECK_FALSE(f.host.main_visible);
  f.host.advance(AppController::kHideDelay);
  CHECK(f.host.capture_requests.empty());  // waits longer when the window was open
  f.host.advance(AppController::kHideMainDelay);
  REQUIRE(f.host.capture_requests.size() == 1);
  f.host.pending_capture(fail("boom"));
  CHECK(f.host.main_visible);
  REQUIRE(f.host.notices.size() == 1);
  CHECK(f.host.notices[0].kind == NoticeKind::Error);
  CHECK(f.host.notices[0].detail == "boom");
}

TEST_CASE("region screenshot crops the selection") {
  Fixture f;
  f.app->trigger(Action::ShotRegion);
  f.host.advance(AppController::kHideDelay);
  f.host.pending_capture(shots_for(f.host.capture_requests.back()));
  CHECK(f.app->mode_name() == "selecting");
  CHECK(f.host.selector_visible);
  CHECK(f.host.selector_focus == 1);
  f.app->on_selection(Selection{1, PxRect{10, 10, 40, 30}, false});
  CHECK_FALSE(f.host.selector_visible);
  f.host.advance();
  REQUIRE(f.host.notices.size() == 1);
  CHECK(f.host.notices[0].monitor_id == 22u);
  CHECK(f.host.notices[0].thumbnail->width == 40);
  CHECK(f.host.notices[0].thumbnail->height == 30);
}

TEST_CASE("cancelling the selection saves nothing") {
  Fixture f;
  f.app->trigger(Action::ShotRegion);
  f.host.advance(AppController::kHideDelay);
  f.host.pending_capture(shots_for(f.host.capture_requests.back()));
  f.app->on_selection(std::nullopt);
  f.host.advance(1s);
  CHECK(f.app->idle());
  CHECK(f.host.notices.empty());
  f.app->on_selection(Selection{});  // late input is ignored
  CHECK(f.app->idle());
}

TEST_CASE("a capture result that arrives after the mode changed is ignored") {
  Fixture f;
  f.app->trigger(Action::ShotScreen);
  f.host.advance(AppController::kHideDelay);
  auto stale = f.host.pending_capture;
  f.app->on_displays_changed();  // no-op while capturing
  f.app->quit();
  CHECK(f.host.terminated);
  stale(shots_for(f.host.displays));  // must not crash or show anything new
  f.host.advance();
}

TEST_CASE("missing permission asks for it instead of capturing") {
  Fixture f;
  f.host.permission = false;
  f.app->trigger(Action::ShotRegion);
  CHECK(f.host.permission_requests == 1);
  CHECK(f.host.main_visible);
  CHECK(f.app->idle());
  f.host.advance(1s);
  CHECK(f.host.capture_requests.empty());
}

TEST_CASE("recording the screen with a countdown, pause and stop") {
  Fixture f(3);
  f.app->trigger(Action::RecordScreen);
  f.host.advance(AppController::kHideDelay);
  CHECK(f.app->mode_name() == "countdown");
  REQUIRE(f.host.bar);
  CHECK(f.host.bar->countdown == 3);
  f.host.advance(1s);
  CHECK(f.host.bar->countdown == 2);
  f.host.advance(2s);
  CHECK(f.app->recording());
  REQUIRE(f.host.recorder);
  const RecordConfig& cfg = f.host.recorder->config;
  CHECK(cfg.target.monitor.id == 22);
  CHECK(cfg.target.image_width == (kDesktopInPoints ? 200u : 100u));  // 100 points at 2x on macOS
  CHECK(cfg.out_width % 2 == 0);
  CHECK(cfg.path.extension() == ".mp4");
  CHECK(f.host.tray_recording);
  CHECK(f.host.bar->badge == "REC");

  f.app->trigger(Action::TogglePause);
  CHECK(f.host.recorder->paused);
  CHECK(f.host.bar->paused);
  f.host.advance(1s);  // the bar keeps refreshing
  CHECK(f.host.bar->elapsed == 3s);

  f.app->on_bar_action(BarAction::Stop);
  CHECK(f.host.recorder->stopped);
  CHECK(f.app->mode_name() == "finalizing");
  CHECK(f.host.bar->mode == BarMode::Finalizing);
  CHECK_FALSE(f.host.tray_recording);

  std::ofstream(cfg.path) << "x";
  f.host.recorder->done(RecordingResult{cfg.path, 65s, {"Microphone unavailable"}});
  CHECK(f.app->idle());
  CHECK_FALSE(f.host.bar.has_value());
  CHECK(f.host.recorder->destroyed);
  REQUIRE(f.host.notices.size() == 1);
  CHECK(f.host.notices[0].title == "Recording saved");
  CHECK(f.host.notices[0].is_video);
  CHECK(f.host.notices[0].detail.find("1:05") != std::string::npos);
  CHECK(f.host.notices[0].detail.find("Microphone unavailable") != std::string::npos);
  CHECK(f.host.recent.front() == cfg.path);
}

TEST_CASE("stopping during the countdown never starts the recording") {
  Fixture f(3);
  f.app->trigger(Action::RecordScreen);
  f.host.advance(AppController::kHideDelay);
  f.app->trigger(Action::StopRecording);
  CHECK(f.app->idle());
  f.host.advance(10s);  // the stale countdown timers fire and do nothing
  CHECK(f.app->idle());
  CHECK_FALSE(f.host.recorder);
  CHECK_FALSE(f.host.bar.has_value());
}

TEST_CASE("pressing a record shortcut again stops the recording") {
  Fixture f;
  f.app->trigger(Action::RecordScreen);
  f.host.advance(AppController::kHideDelay);
  REQUIRE(f.app->recording());
  f.app->trigger(Action::RecordRegion);
  CHECK(f.host.recorder->stopped);
  f.app->trigger(Action::ShotScreen);  // ignored while finalizing
  CHECK(f.app->mode_name() == "finalizing");
}

TEST_CASE("a recording that never finishes is reported after the watchdog") {
  Fixture f;
  f.app->trigger(Action::RecordScreen);
  f.host.advance(AppController::kHideDelay);
  f.app->trigger(Action::StopRecording);
  f.host.advance(AppController::kFinalizeTimeout);
  CHECK(f.app->idle());
  CHECK(f.host.recorder->destroyed);
  REQUIRE(f.host.notices.size() == 1);
  CHECK(f.host.notices[0].kind == NoticeKind::Error);
}

TEST_CASE("a recorder that fails by itself returns to idle") {
  Fixture f;
  f.app->trigger(Action::RecordScreen);
  f.host.advance(AppController::kHideDelay);
  f.host.recorder->done(fail("encoder died"));
  CHECK(f.app->idle());
  CHECK(f.host.recorder->stopped);
  CHECK_FALSE(f.host.tray_recording);
  REQUIRE(f.host.notices.size() == 1);
  CHECK(f.host.notices[0].detail == "encoder died");
}

TEST_CASE("a recorder that cannot start is reported") {
  Fixture f;
  f.host.recorder_error = "no encoder";
  f.app->trigger(Action::RecordScreen);
  f.host.advance(AppController::kHideDelay);
  CHECK(f.app->idle());
  REQUIRE(f.host.notices.size() == 1);
  CHECK(f.host.notices[0].detail.find("no encoder") != std::string::npos);
}

TEST_CASE("quitting while recording waits for the file, but not forever") {
  SUBCASE("finishes") {
    Fixture f;
    f.app->trigger(Action::RecordScreen);
    f.host.advance(AppController::kHideDelay);
    f.app->quit();
    CHECK(f.host.recorder->stopped);
    CHECK_FALSE(f.host.terminated);
    f.host.recorder->done(fail("cancelled"));
    CHECK(f.host.terminated);
  }
  SUBCASE("times out") {
    Fixture f;
    f.app->trigger(Action::RecordScreen);
    f.host.advance(AppController::kHideDelay);
    f.app->quit();
    f.host.advance(10s);
    CHECK(f.host.terminated);
  }
}

TEST_CASE("disconnecting the recorded display stops the recording") {
  Fixture f;
  f.app->trigger(Action::RecordScreen);
  f.host.advance(AppController::kHideDelay);
  f.host.displays.pop_back();
  f.app->on_displays_changed();
  CHECK(f.host.recorder->stopped);
  CHECK(f.app->mode_name() == "finalizing");
}

TEST_CASE("display changes close the selector") {
  Fixture f;
  f.app->trigger(Action::RecordRegion);
  f.host.advance(AppController::kHideDelay);
  f.host.pending_capture(shots_for(f.host.capture_requests.back()));
  REQUIRE(f.host.selector_visible);
  f.app->on_displays_changed();
  CHECK_FALSE(f.host.selector_visible);
  CHECK(f.app->idle());
}

TEST_CASE("region recording uses the selected rectangle") {
  Fixture f;
  f.app->trigger(Action::RecordRegion);
  f.host.advance(AppController::kHideDelay);
  f.host.pending_capture(shots_for(f.host.capture_requests.back()));
  f.app->on_selection(Selection{0, PxRect{20, 20, 61, 41}, false});
  REQUIRE(f.app->recording());
  const RecordConfig& cfg = f.host.recorder->config;
  CHECK(cfg.target.region == PxRect{20, 20, 61, 41});
  CHECK(cfg.out_width == 60);
  CHECK(cfg.out_height == 40);
}

TEST_CASE("hotkeys: duplicates, unavailable keys and suspension while recording one") {
  Fixture f;
  CHECK(f.host.hotkeys.size() == 6);
  CHECK(f.app->hotkey_errors().empty());

  Settings s = f.app->settings();
  s.shortcuts[Action::ShotAllScreens] = "Alt+Shift+1";  // same as ShotScreen
  s.shortcuts[Action::ShowWindow] = "Ctrl+Alt+S";
  f.host.taken.insert("Ctrl+Alt+S");
  f.app->update_settings(s);
  CHECK(f.app->hotkey_errors().at(Action::ShotScreen).find("Screenshot all screens") != std::string::npos);
  CHECK(f.app->hotkey_errors().at(Action::ShotAllScreens).find("Screenshot current screen") != std::string::npos);
  CHECK(f.app->hotkey_errors().at(Action::ShowWindow).find("Unavailable") != std::string::npos);
  CHECK_FALSE(f.host.hotkeys.contains(Action::ShotScreen));

  f.app->begin_shortcut_capture(Action::ShotRegion);
  CHECK(f.host.hotkeys.empty());
  f.app->end_shortcut_capture();
  CHECK(f.host.hotkeys.size() == 5);
}

TEST_CASE("closing the window keeps running in the tray and shows the hint once") {
  Fixture f;
  f.app->trigger(Action::ShowWindow);
  f.app->on_main_window_closed();
  CHECK_FALSE(f.host.main_visible);
  CHECK_FALSE(f.host.dock_visible);
  CHECK_FALSE(f.host.terminated);
  REQUIRE(f.host.notices.size() == 1);
  CHECK(f.app->settings().tray_hint_shown);
  f.app->trigger(Action::ShowWindow);
  f.app->on_main_window_closed();
  CHECK(f.host.notices.size() == 1);

  f.host.tray = false;
  f.app->trigger(Action::ShowWindow);
  f.app->on_main_window_closed();
  CHECK(f.host.terminated);
}

TEST_CASE("recent captures are scanned from the save folder") {
  TempDir dir("controller-recent");
  std::ofstream(dir.path() / "a.png") << "x";
  std::ofstream(dir.path() / "b.txt") << "x";
  FakeHost host;
  host.displays = {display(0, 1, 0, true)};
  Settings s;
  s.save_dir = dir.path();
  auto app = AppController::create(host, s);
  app->start(std::nullopt, false);
  CHECK(host.main_visible);
  host.advance();
  REQUIRE(host.recent.size() == 1);
  CHECK(host.recent[0].filename() == "a.png");
}

TEST_CASE("callbacks after the controller is gone are harmless") {
  FakeHost host;
  host.displays = {display(0, 1, 0, true)};
  TempDir dir("controller-gone");
  Settings s;
  s.save_dir = dir.path();
  {
    auto app = AppController::create(host, s);
    app->start(Action::ShotScreen, true);
    host.advance(AppController::kHideDelay);
  }
  REQUIRE(host.pending_capture);
  host.pending_capture(shots_for(host.displays));
  host.advance(1min);
  CHECK(host.notices.empty());
}
