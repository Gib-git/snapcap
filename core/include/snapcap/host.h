// The interface between the portable app logic (AppController) and an OS backend.
//
// Threading contract: the controller is only ever called on the UI thread, and every
// Host method is called on the UI thread. Callbacks a Host receives (capture results,
// recording completion) must be delivered on the UI thread too; `post` is the one
// method that may be called from any thread.
#pragma once

#include <chrono>
#include <filesystem>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "snapcap/actions.h"
#include "snapcap/clock.h"
#include "snapcap/error.h"
#include "snapcap/geometry.h"
#include "snapcap/hotkeys.h"
#include "snapcap/settings.h"

namespace snapcap {

enum class SelectionPurpose { Screenshot, Record };

// The user's choice in the region selector.
struct Selection {
  size_t shot = 0;    // index into the shots the selector was shown with
  PxRect rect;        // in that shot's image pixels
  bool full = false;  // a click (no drag) selects the whole display
};

enum class NoticeKind { Success, Error };

struct Notice {
  NoticeKind kind = NoticeKind::Success;
  std::string title;
  std::string detail;
  std::optional<std::filesystem::path> path;  // the capture, for open / reveal / drag out
  std::shared_ptr<const RgbaImage> thumbnail;
  bool is_video = false;
  std::optional<uint32_t> monitor_id;  // display to show it on; unset = the primary display
  std::chrono::milliseconds lifetime{5000};
};

enum class BarMode { Countdown, Recording, Finalizing };
enum class BarAction { Stop, Cancel, TogglePause };

struct BarState {
  BarMode mode = BarMode::Recording;
  uint32_t countdown = 0;
  Duration elapsed{0};
  bool paused = false;
  std::string badge;  // "REC" or "GIF"
};

struct AudioSpec {
  bool mic = false;
  std::optional<std::string> mic_device;  // unset = system default input
  bool system = false;
  bool any() const { return mic || system; }
};

struct RecordConfig {
  CaptureTarget target;
  VideoFormat format = VideoFormat::Mp4;
  uint32_t fps = 30;
  Quality quality = Quality::Medium;
  ResolutionCap resolution = ResolutionCap::Native;
  uint32_t gif_max_width = 960;
  AudioSpec audio;
  bool show_cursor = true;
  std::filesystem::path path;
  // The encoded size, from output_size().
  uint32_t out_width = 0;
  uint32_t out_height = 0;
};

struct RecordingResult {
  std::filesystem::path path;
  Duration duration{0};
  std::vector<std::string> warnings;
};

using RecordingDone = std::function<void(Result<RecordingResult>)>;

// A running recording. Destroying it at any time must be safe and must not block for long.
class Recorder {
 public:
  virtual ~Recorder() = default;
  // Stops capturing; the file is finalized in the background and `done` fires once.
  virtual void stop() = 0;
  virtual void toggle_pause() = 0;
  virtual bool paused() const = 0;
  virtual Duration elapsed() const = 0;
};

class Host {
 public:
  virtual ~Host() = default;

  // ---- scheduling
  // Runs `fn` on the UI thread. Thread-safe.
  virtual void post(std::function<void()> fn) = 0;
  // Runs `fn` on the UI thread after `delay`.
  virtual void post_delayed(std::chrono::milliseconds delay, std::function<void()> fn) = 0;
  // Runs `work` on a background thread.
  virtual void run_background(std::function<void()> work) = 0;

  // ---- displays and capture
  virtual std::vector<MonitorInfo> monitors() = 0;
  virtual std::optional<PointF> cursor_position() = 0;
  virtual bool has_capture_permission() = 0;
  virtual void request_capture_permission() = 0;
  // Captures the given displays; `done` runs on the UI thread.
  virtual void capture(std::vector<MonitorInfo> which, std::function<void(Result<std::vector<Shot>>)> done) = 0;
  virtual Result<std::unique_ptr<Recorder>> start_recorder(const RecordConfig& config, RecordingDone done) = 0;

  // ---- windows
  virtual void set_main_window_visible(bool visible) = 0;
  virtual void set_dock_visible(bool visible) = 0;
  virtual void show_selector(SelectionPurpose purpose, std::shared_ptr<const std::vector<Shot>> shots,
                             size_t focus) = 0;
  virtual void hide_selector() = 0;
  virtual void show_recording_ui(const CaptureTarget& target, const BarState& state) = 0;
  virtual void hide_recording_ui() = 0;
  virtual void show_notice(const Notice& notice) = 0;
  virtual void hide_notice() = 0;
  // Hands keyboard focus back to the app the user was in.
  virtual void yield_focus() = 0;
  virtual void apply_theme(ThemePref theme) = 0;
  virtual void recent_changed(const std::vector<std::filesystem::path>& files) = 0;
  // Settings or hotkey problems changed; the UI should refresh.
  virtual void state_changed() = 0;

  // ---- system integration
  virtual bool tray_available() = 0;
  virtual void set_tray_recording(bool recording) = 0;
  virtual void set_tray_shortcuts(const ShortcutMap& shortcuts) = 0;
  virtual void unregister_hotkeys() = 0;
  virtual Result<void> register_hotkey(Action action, const Shortcut& shortcut) = 0;
  virtual void copy_png_to_clipboard(std::shared_ptr<const std::vector<uint8_t>> png) = 0;
  virtual void open_path(const std::filesystem::path& path) = 0;
  virtual void reveal_path(const std::filesystem::path& path) = 0;
  // Persists nothing itself; exits the process's run loop.
  virtual void terminate() = 0;
};

}  // namespace snapcap
