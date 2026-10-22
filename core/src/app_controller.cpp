#include "snapcap/app_controller.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <format>

#include "snapcap/log.h"
#include "snapcap/output.h"
#include "snapcap/png.h"
#include "snapcap/recent.h"

namespace snapcap {
namespace fs = std::filesystem;
using namespace std::chrono_literals;

namespace {

constexpr std::chrono::milliseconds kSuccessLifetime{5000};
constexpr std::chrono::milliseconds kErrorLifetime{7000};
constexpr std::chrono::milliseconds kBarRefresh{250};
constexpr std::chrono::milliseconds kQuitTimeout{10'000};

std::string format_duration(Duration d) {
  auto total = std::chrono::duration_cast<std::chrono::seconds>(d).count();
  if (total >= 3600) return std::format("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60);
  return std::format("{}:{:02}", total / 60, total % 60);
}

bool is_capture(Action a) {
  return a == Action::ShotScreen || a == Action::ShotAllScreens || a == Action::ShotRegion ||
         a == Action::RecordScreen || a == Action::RecordRegion;
}

}  // namespace

std::shared_ptr<AppController> AppController::create(Host& host, Settings settings) {
  return std::shared_ptr<AppController>(new AppController(host, std::move(settings)));
}

AppController::AppController(Host& host, Settings settings) : host_(host), settings_(std::move(settings)) {}

void AppController::start(std::optional<Action> startup_action, bool background) {
  has_permission_ = host_.has_capture_permission();
  apply_hotkeys();
  host_.set_tray_shortcuts(settings_.shortcuts);
  host_.apply_theme(settings_.theme);
  refresh_recent();
  if (!background || !host_.tray_available()) {
    show_main();
  } else {
    host_.set_dock_visible(false);
  }
  if (startup_action) trigger(*startup_action);
}

// ------------------------------------------------------------------ helpers

void AppController::set_mode(Mode mode) {
  mode_ = std::move(mode);
  ++epoch_;
}

void AppController::after(std::chrono::milliseconds delay, std::function<void(AppController&)> fn) {
  host_.post_delayed(delay, [weak = weak_from_this(), epoch = epoch_, fn = std::move(fn)] {
    auto self = weak.lock();
    if (self && self->epoch_ == epoch) fn(*self);
  });
}

bool AppController::idle() const { return std::holds_alternative<Idle>(mode_); }

bool AppController::recording() const { return std::holds_alternative<Recording>(mode_); }

std::string_view AppController::mode_name() const {
  static constexpr std::string_view kNames[] = {"idle",      "preparing", "capturing", "selecting",
                                                "countdown", "recording", "finalizing"};
  return kNames[mode_.index()];
}

bool AppController::refresh_permission() {
  bool before = has_permission_;
  has_permission_ = host_.has_capture_permission();
  if (before != has_permission_) host_.state_changed();
  return has_permission_;
}

// ------------------------------------------------------------------ actions

void AppController::trigger(Action action) {
  log::info("action: {}", cli_name(action));
  switch (action) {
    case Action::ShowWindow: show_main(); return;
    case Action::StopRecording: stop_recording(); return;
    case Action::TogglePause:
      if (auto* r = std::get_if<Recording>(&mode_)) {
        r->recorder->toggle_pause();
        refresh_bar();
      }
      return;
    default: break;
  }
  if (!is_capture(action) || quitting_) return;
  if (!idle()) {
    // Pressing a record shortcut again stops the recording.
    if (recording() && (action == Action::RecordScreen || action == Action::RecordRegion)) stop_recording();
    return;
  }
  if (!refresh_permission()) {
    log::warn("screen recording permission is missing; asking for it");
    host_.request_capture_permission();
    show_main();
    return;
  }
  end_shortcut_capture();
  restore_main_ = main_visible_;
  auto delay = main_visible_ ? kHideMainDelay : kHideDelay;
  if (main_visible_) {
    main_visible_ = false;
    host_.set_main_window_visible(false);
  }
  host_.hide_notice();
  set_mode(Preparing{action});
  after(delay, [action](AppController& self) { self.run_prepared(action); });
}

void AppController::run_prepared(Action action) {
  auto monitors = host_.monitors();
  if (monitors.empty()) return capture_failed("No displays found");
  size_t under = monitor_under(monitors, host_.cursor_position());

  if (action == Action::RecordScreen) {
    const MonitorInfo& m = monitors[under];
    double s = m.px_per_unit();
    CaptureTarget target{m, static_cast<uint32_t>(std::lround(m.width * s)),
                         static_cast<uint32_t>(std::lround(m.height * s)), std::nullopt};
    return begin_recording(std::move(target));
  }

  std::vector<MonitorInfo> which;
  if (action == Action::ShotScreen) {
    which.push_back(monitors[under]);
  } else {
    which = monitors;
  }
  set_mode(Capturing{action});
  auto weak = weak_from_this();
  auto epoch = epoch_;
  host_.capture(std::move(which), [weak, epoch, action](Result<std::vector<Shot>> shots) {
    auto self = weak.lock();
    if (self && self->epoch_ == epoch) self->on_captured(action, std::move(shots));
  });
}

void AppController::on_captured(Action action, Result<std::vector<Shot>> shots) {
  if (!shots) return capture_failed(shots.error().message);
  log::info("captured {} display(s)", shots->size());
  if (shots->empty()) return capture_failed("No displays found");
  for (const Shot& s : *shots) {
    if (s.image.empty()) return capture_failed("The screen could not be captured (check screen recording permission)");
  }
  switch (action) {
    case Action::ShotScreen: {
      Shot& s = shots->front();
      return save_screenshot(std::move(s.image), s.monitor);
    }
    case Action::ShotAllScreens: {
      auto primary = std::ranges::find_if(*shots, [](const Shot& s) { return s.monitor.primary; });
      MonitorInfo monitor = (primary != shots->end() ? *primary : shots->front()).monitor;
      return save_screenshot(stitch(*shots), std::move(monitor));
    }
    case Action::ShotRegion:
    case Action::RecordRegion: {
      std::vector<MonitorInfo> infos;
      for (const Shot& s : *shots) infos.push_back(s.monitor);
      size_t focus = monitor_under(infos, host_.cursor_position());
      auto purpose = action == Action::ShotRegion ? SelectionPurpose::Screenshot : SelectionPurpose::Record;
      auto shared = std::make_shared<const std::vector<Shot>>(std::move(*shots));
      set_mode(Selecting{purpose, shared});
      host_.show_selector(purpose, shared, focus);
      return;
    }
    default: return finish_capture();
  }
}

void AppController::on_selection(std::optional<Selection> selection) {
  auto* sel = std::get_if<Selecting>(&mode_);
  if (sel == nullptr) return;
  auto shots = sel->shots;
  auto purpose = sel->purpose;
  host_.hide_selector();
  if (!restore_main_) host_.yield_focus();
  if (!selection || selection->shot >= shots->size()) return finish_capture();

  const Shot& s = (*shots)[selection->shot];
  if (purpose == SelectionPurpose::Screenshot) {
    RgbaImage img = selection->full ? s.image
                                    : crop(s.image, selection->rect.x, selection->rect.y, selection->rect.w,
                                           selection->rect.h);
    if (img.empty()) return finish_capture();
    return save_screenshot(std::move(img), s.monitor);
  }
  CaptureTarget target{s.monitor, s.image.width, s.image.height, std::nullopt};
  if (!selection->full) target.region = selection->rect;
  begin_recording(std::move(target));
}

void AppController::capture_failed(const std::string& message) {
  finish_capture();
  show_error("Capture failed", message, std::nullopt);
}

void AppController::finish_capture() {
  host_.hide_recording_ui();
  set_mode(Idle{});
  if (restore_main_) {
    restore_main_ = false;
    show_main();
  }
}

// ------------------------------------------------------------------ screenshots

void AppController::save_screenshot(RgbaImage image, MonitorInfo monitor) {
  fs::path dir = save_dir(settings_);
  bool want_png_bytes = settings_.copy_to_clipboard;
  uint32_t monitor_id = monitor.id;
  auto weak = weak_from_this();
  Host* host = &host_;
  // The host outlives every background task it runs, so capturing it is safe.
  host_.run_background([host, weak, dir, want_png_bytes, monitor_id, image = std::move(image)]() mutable {
    auto path = new_capture_path(dir, CaptureKind::Screenshot, "png");
    Result<std::vector<uint8_t>> png = fail("");
    Result<void> written;
    if (!path) {
      written = std::unexpected(path.error());
    } else if (png = encode_png(image); !png) {
      written = std::unexpected(png.error());
    } else {
      std::FILE* f = std::fopen(path->string().c_str(), "wb");
      bool ok = f != nullptr && std::fwrite(png->data(), 1, png->size(), f) == png->size();
      if (f != nullptr && std::fclose(f) != 0) ok = false;
      if (!ok) written = fail(std::format("cannot write {}", path->string()));
    }
    auto thumb = std::make_shared<const RgbaImage>(thumbnail(image));
    image = RgbaImage{};  // free the full-size capture before handing back
    if (written) {
      auto bytes = want_png_bytes ? std::make_shared<const std::vector<uint8_t>>(std::move(*png)) : nullptr;
      host->post([weak, p = *path, bytes, thumb, monitor_id] {
        if (auto self = weak.lock()) self->on_screenshot_saved(p, bytes, thumb, monitor_id);
      });
    } else {
      host->post([weak, msg = written.error().message] {
        if (auto self = weak.lock()) self->show_error("Could not save screenshot", msg, std::nullopt);
      });
    }
  });
  finish_capture();
}

void AppController::on_screenshot_saved(const fs::path& path, std::shared_ptr<const std::vector<uint8_t>> png,
                                        std::shared_ptr<const RgbaImage> thumb, uint32_t monitor_id) {
  if (png) host_.copy_png_to_clipboard(std::move(png));
  Notice n;
  n.title = "Screenshot saved";
  n.detail = path.filename().string();
  if (settings_.copy_to_clipboard) n.detail += "\nCopied to clipboard";
  n.path = path;
  n.thumbnail = std::move(thumb);
  n.monitor_id = monitor_id;
  n.lifetime = kSuccessLifetime;
  host_.show_notice(n);
  if (settings_.open_after_capture) host_.open_path(path);
  add_recent(path);
}

// ------------------------------------------------------------------ recording

void AppController::begin_recording(CaptureTarget target) {
  if (settings_.countdown_secs == 0) return start_recording(std::move(target));
  set_mode(Countdown{std::move(target), settings_.countdown_secs});
  refresh_bar();
  after(1s, [](AppController& self) { self.countdown_tick(); });
}

void AppController::countdown_tick() {
  auto* c = std::get_if<Countdown>(&mode_);
  if (c == nullptr) return;
  if (c->remaining <= 1) return start_recording(std::move(c->target));
  --c->remaining;
  refresh_bar();
  after(1s, [](AppController& self) { self.countdown_tick(); });
}

void AppController::start_recording(CaptureTarget target) {
  const Settings& s = settings_;
  const char* ext = s.video_format == VideoFormat::Mp4 ? "mp4" : "gif";
  auto path = new_capture_path(save_dir(s), CaptureKind::Recording, ext);
  if (!path) return capture_failed(path.error().message);

  RecordConfig cfg;
  cfg.target = target;
  cfg.format = s.video_format;
  cfg.fps = s.video_format == VideoFormat::Gif ? std::min(s.gif_fps, 30u) : s.fps;
  cfg.quality = s.quality;
  cfg.resolution = s.resolution;
  cfg.gif_max_width = s.gif_max_width;
  cfg.audio = AudioSpec{s.record_mic, s.mic_device, s.record_system_audio};
  cfg.show_cursor = s.show_cursor;
  cfg.path = *path;
  PxRect r = target.pixel_rect();
  std::tie(cfg.out_width, cfg.out_height) = output_size(s.video_format, s.resolution, s.gif_max_width, r.w, r.h);

  // Not tied to the epoch: the result must be handled whatever the mode is by then.
  auto done = [weak = weak_from_this()](Result<RecordingResult> result) {
    if (auto self = weak.lock()) self->on_recording_done(std::move(result));
  };
  auto recorder = host_.start_recorder(cfg, std::move(done));
  if (!recorder) return capture_failed(std::format("Could not start recording: {}", recorder.error().message));
  host_.set_tray_recording(true);
  set_mode(Recording{std::move(*recorder), std::move(target)});
  bar_tick();
}

void AppController::bar_tick() {
  // Keeps the elapsed time on the bar current; stops with the recording (epoch change).
  refresh_bar();
  after(kBarRefresh, [](AppController& self) { self.bar_tick(); });
}

void AppController::refresh_bar() {
  BarState state;
  const CaptureTarget* target = nullptr;
  if (auto* c = std::get_if<Countdown>(&mode_)) {
    state.mode = BarMode::Countdown;
    state.countdown = c->remaining;
    target = &c->target;
  } else if (auto* r = std::get_if<Recording>(&mode_)) {
    state.mode = BarMode::Recording;
    state.elapsed = r->recorder->elapsed();
    state.paused = r->recorder->paused();
    state.badge = settings_.video_format == VideoFormat::Mp4 ? "REC" : "GIF";
    target = &r->target;
  } else if (auto* f = std::get_if<Finalizing>(&mode_)) {
    state.mode = BarMode::Finalizing;
    target = &f->target;
  }
  if (target != nullptr) host_.show_recording_ui(*target, state);
}

void AppController::on_bar_action(BarAction action) {
  switch (action) {
    case BarAction::TogglePause: trigger(Action::TogglePause); break;
    case BarAction::Stop:
    case BarAction::Cancel: stop_recording(); break;
  }
}

void AppController::stop_recording() {
  if (std::holds_alternative<Countdown>(mode_)) return finish_capture();
  auto* r = std::get_if<Recording>(&mode_);
  if (r == nullptr) return;
  auto recorder = std::move(r->recorder);
  auto target = std::move(r->target);
  recorder->stop();
  host_.set_tray_recording(false);
  set_mode(Finalizing{std::move(recorder), std::move(target)});
  refresh_bar();
  after(kFinalizeTimeout, [](AppController& self) {
    log::error("recording did not finish within {} s", kFinalizeTimeout.count() / 1000);
    self.on_recording_done(fail("The recording did not finish writing in time."));
  });
}

void AppController::on_recording_done(Result<RecordingResult> result) {
  std::optional<uint32_t> monitor_id;
  if (auto* r = std::get_if<Recording>(&mode_)) {
    // The recorder stopped on its own (e.g. an encoder error mid-recording).
    monitor_id = r->target.monitor.id;
    r->recorder->stop();
    host_.set_tray_recording(false);
  } else if (auto* f = std::get_if<Finalizing>(&mode_)) {
    monitor_id = f->target.monitor.id;
  }
  if (recording() || std::holds_alternative<Finalizing>(mode_)) {
    host_.hide_recording_ui();
    set_mode(Idle{});
  }

  if (result) {
    std::string detail = std::format("{} · {}", result->path.filename().string(), format_duration(result->duration));
    for (const auto& w : result->warnings) detail += "\n" + w;
    Notice n;
    n.title = "Recording saved";
    n.detail = detail;
    n.path = result->path;
    n.is_video = media_kind(result->path) == MediaKind::Mp4;
    n.monitor_id = monitor_id;
    n.lifetime = kSuccessLifetime;
    host_.show_notice(n);
    if (settings_.open_after_capture) host_.open_path(result->path);
    add_recent(result->path);
  } else {
    show_error("Recording failed", result.error().message, monitor_id);
  }
  if (restore_main_) {
    restore_main_ = false;
    show_main();
  }
  maybe_terminate();
}

void AppController::on_displays_changed() {
  auto monitors = host_.monitors();
  auto present = [&](uint32_t id) { return std::ranges::any_of(monitors, [id](const auto& m) { return m.id == id; }); };
  if (std::holds_alternative<Selecting>(mode_)) {
    // The frozen screens no longer match the desktop.
    host_.hide_selector();
    finish_capture();
  } else if (auto* c = std::get_if<Countdown>(&mode_); c != nullptr && !present(c->target.monitor.id)) {
    finish_capture();
  } else if (auto* r = std::get_if<Recording>(&mode_); r != nullptr && !present(r->target.monitor.id)) {
    log::warn("the recorded display was disconnected; stopping");
    stop_recording();
  }
}

// ------------------------------------------------------------------ windows

void AppController::show_main() {
  main_visible_ = true;
  host_.set_dock_visible(true);
  host_.set_main_window_visible(true);
}

void AppController::hide_main() {
  main_visible_ = false;
  end_shortcut_capture();
  host_.set_main_window_visible(false);
  if (!host_.tray_available()) return;
  host_.set_dock_visible(false);
  if (!settings_.tray_hint_shown) {
    Settings s = settings_;
    s.tray_hint_shown = true;
    update_settings(s);
    Notice n;
    n.title = "SnapCap is still running";
    n.detail = std::format("Find it in the {}, or use your shortcuts.", kOs == Os::MacOS ? "menu bar" : "system tray");
    n.lifetime = 6s;
    host_.show_notice(n);
  }
}

void AppController::on_main_window_closed() {
  if (host_.tray_available()) {
    hide_main();
  } else {
    main_visible_ = false;
    quit();
  }
}

void AppController::show_error(const std::string& title, const std::string& detail, std::optional<uint32_t> monitor_id) {
  log::error("{}: {}", title, detail);
  Notice n;
  n.kind = NoticeKind::Error;
  n.title = title;
  n.detail = detail;
  n.monitor_id = monitor_id;
  n.lifetime = kErrorLifetime;
  host_.show_notice(n);
}

void AppController::open_save_folder() {
  fs::path dir = save_dir(settings_);
  std::error_code ec;
  fs::create_directories(dir, ec);
  host_.open_path(dir);
}

// ------------------------------------------------------------------ settings

void AppController::update_settings(const Settings& s) {
  if (s == settings_) return;
  Settings old = std::exchange(settings_, s);
  if (old.shortcuts != settings_.shortcuts) {
    if (!capturing_shortcut_) apply_hotkeys();
    host_.set_tray_shortcuts(settings_.shortcuts);
  }
  if (old.theme != settings_.theme) host_.apply_theme(settings_.theme);
  if (old.save_dir != settings_.save_dir) refresh_recent();
  settings_.save();
  host_.state_changed();
}

void AppController::apply_hotkeys() {
  host_.unregister_hotkeys();
  hotkey_errors_.clear();
  auto dups = duplicate_shortcuts(settings_.shortcuts);
  for (const auto& [action, text] : settings_.shortcuts) {
    if (text.find_first_not_of(' ') == std::string::npos) continue;
    if (auto d = dups.find(action); d != dups.end()) {
      hotkey_errors_[action] = std::format("Also used by “{}”", label(d->second));
      continue;
    }
    auto sc = parse_shortcut(text);
    if (!sc) {
      hotkey_errors_[action] = sc.error().message;
      continue;
    }
    if (auto r = host_.register_hotkey(action, *sc); !r) {
      hotkey_errors_[action] = std::format("Unavailable — taken by another app? ({})", r.error().message);
    }
  }
  host_.state_changed();
}

void AppController::begin_shortcut_capture(Action action) {
  capturing_shortcut_ = action;
  host_.unregister_hotkeys();
}

void AppController::end_shortcut_capture() {
  if (!capturing_shortcut_) return;
  capturing_shortcut_.reset();
  apply_hotkeys();
}

// ------------------------------------------------------------------ recent captures

void AppController::refresh_recent() {
  fs::path dir = save_dir(settings_);
  Host* host = &host_;
  host_.run_background([host, weak = weak_from_this(), dir] {
    auto files = scan_recent(dir);
    host->post([weak, dir, files = std::move(files)] {
      auto self = weak.lock();
      // Ignore a scan of a folder that is no longer the save folder.
      if (!self || save_dir(self->settings_) != dir) return;
      self->recent_ = files;
      self->host_.recent_changed(self->recent_);
    });
  });
}

void AppController::add_recent(const fs::path& path) {
  if (!media_kind(path)) return;
  std::erase(recent_, path);
  recent_.insert(recent_.begin(), path);
  if (recent_.size() > kMaxRecent) recent_.resize(kMaxRecent);
  host_.recent_changed(recent_);
}

// ------------------------------------------------------------------ quitting

void AppController::quit() {
  if (quitting_) return;
  quitting_ = true;
  settings_.save();
  if (recording()) {
    stop_recording();
  } else if (!idle() && !std::holds_alternative<Finalizing>(mode_)) {
    // Abandon a capture in progress; its pending callbacks become stale.
    host_.hide_selector();
    host_.hide_recording_ui();
    set_mode(Idle{});
  }
  if (std::holds_alternative<Finalizing>(mode_)) {
    // Let the encoder write the file's index, but never hang on it.
    host_.post_delayed(kQuitTimeout, [weak = weak_from_this()] {
      if (auto self = weak.lock()) self->host_.terminate();
    });
    return;
  }
  host_.terminate();
}

void AppController::maybe_terminate() {
  if (quitting_ && !std::holds_alternative<Finalizing>(mode_)) host_.terminate();
}

}  // namespace snapcap
