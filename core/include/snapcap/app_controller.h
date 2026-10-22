// Application state machine: idle -> preparing -> capturing -> selecting -> countdown ->
// recording -> finalizing. Shared by every OS backend through the Host interface.
//
// Robustness rules:
// * Every timer and asynchronous callback is tagged with the epoch of the mode it was
//   started in; when the mode has changed since, the callback is ignored. A stale timer
//   can therefore never act on a newer capture.
// * Callbacks hold a weak reference, so they are harmless after the controller is gone.
// * Finalizing a recording and quitting are bounded by watchdogs.
#pragma once

#include <chrono>
#include <cstdint>
#include <filesystem>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <variant>
#include <vector>

#include "snapcap/host.h"

namespace snapcap {

class AppController : public std::enable_shared_from_this<AppController> {
 public:
  // How long a recording may take to finish writing before it is reported as failed.
  static constexpr std::chrono::milliseconds kFinalizeTimeout{15'000};
  // Delay before capturing, so SnapCap's own windows have disappeared.
  static constexpr std::chrono::milliseconds kHideMainDelay{260};
  static constexpr std::chrono::milliseconds kHideDelay{30};

  static std::shared_ptr<AppController> create(Host& host, Settings settings);

  // Call once the host is ready. Registers hotkeys, scans recent captures, and runs
  // `startup_action` (from the command line) if any.
  void start(std::optional<Action> startup_action, bool background);

  // ---- input from the UI, hotkeys, tray, IPC
  void trigger(Action action);
  void on_selection(std::optional<Selection> selection);
  void on_bar_action(BarAction action);
  void on_main_window_closed();
  void on_displays_changed();
  void open_save_folder();
  void quit();

  // ---- settings
  const Settings& settings() const { return settings_; }
  void update_settings(const Settings& settings);
  // While the user records a new shortcut, global hotkeys are suspended so the key
  // press reaches the recorder.
  void begin_shortcut_capture(Action action);
  void end_shortcut_capture();
  std::optional<Action> capturing_shortcut() const { return capturing_shortcut_; }
  const std::map<Action, std::string>& hotkey_errors() const { return hotkey_errors_; }

  // ---- state for the UI
  const std::vector<std::filesystem::path>& recent() const { return recent_; }
  void refresh_recent();
  bool main_visible() const { return main_visible_; }
  bool has_permission() const { return has_permission_; }
  bool refresh_permission();
  bool idle() const;
  bool recording() const;
  // "idle", "preparing", "capturing", "selecting", "countdown", "recording", "finalizing".
  std::string_view mode_name() const;

 private:
  struct Idle {};
  struct Preparing {
    Action action;
  };
  struct Capturing {
    Action action;
  };
  struct Selecting {
    SelectionPurpose purpose;
    std::shared_ptr<const std::vector<Shot>> shots;
  };
  struct Countdown {
    CaptureTarget target;
    uint32_t remaining;
  };
  struct Recording {
    std::unique_ptr<Recorder> recorder;
    CaptureTarget target;
  };
  struct Finalizing {
    std::unique_ptr<Recorder> recorder;
    CaptureTarget target;
  };
  using Mode = std::variant<Idle, Preparing, Capturing, Selecting, Countdown, Recording, Finalizing>;

  AppController(Host& host, Settings settings);

  void set_mode(Mode mode);
  // Runs `fn` after `delay` if the mode has not changed in between.
  void after(std::chrono::milliseconds delay, std::function<void(AppController&)> fn);
  void run_prepared(Action action);
  void on_captured(Action action, Result<std::vector<Shot>> shots);
  void capture_failed(const std::string& message);
  void finish_capture();
  void save_screenshot(RgbaImage image, MonitorInfo monitor);
  void on_screenshot_saved(const std::filesystem::path& path, std::shared_ptr<const std::vector<uint8_t>> png,
                           std::shared_ptr<const RgbaImage> thumb, uint32_t monitor_id);
  void begin_recording(CaptureTarget target);
  void countdown_tick();
  void start_recording(CaptureTarget target);
  void refresh_bar();
  void bar_tick();
  void stop_recording();
  void on_recording_done(Result<RecordingResult> result);
  void show_main();
  void hide_main();
  void show_error(const std::string& title, const std::string& detail, std::optional<uint32_t> monitor_id);
  void add_recent(const std::filesystem::path& path);
  void apply_hotkeys();
  void maybe_terminate();

  Host& host_;
  Settings settings_;
  Mode mode_ = Idle{};
  uint64_t epoch_ = 0;
  bool main_visible_ = false;
  bool restore_main_ = false;
  bool has_permission_ = true;
  bool quitting_ = false;
  std::optional<Action> capturing_shortcut_;
  std::map<Action, std::string> hotkey_errors_;
  std::vector<std::filesystem::path> recent_;
};

}  // namespace snapcap
