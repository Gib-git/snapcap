// The macOS implementation of snapcap::Host. Owns every window and system integration
// for the lifetime of the process; all methods run on the main thread.
#pragma once

#import <AppKit/AppKit.h>

#include <memory>

#include "snapcap/app_controller.h"
#include "snapcap/host.h"

namespace snapcap::mac {

class MacHost final : public Host {
 public:
  MacHost();
  ~MacHost() override;
  MacHost(const MacHost&) = delete;
  MacHost& operator=(const MacHost&) = delete;

  // Called once the controller exists; UI callbacks are routed to it.
  void attach(std::shared_ptr<AppController> controller);
  // True once the controller asked to quit; lets applicationShouldTerminate proceed.
  bool termination_allowed() const { return termination_allowed_; }

  // ---- Host
  void post(std::function<void()> fn) override;
  void post_delayed(std::chrono::milliseconds delay, std::function<void()> fn) override;
  void run_background(std::function<void()> work) override;
  std::vector<MonitorInfo> monitors() override;
  std::optional<PointF> cursor_position() override;
  bool has_capture_permission() override;
  void request_capture_permission() override;
  void capture(std::vector<MonitorInfo> which, std::function<void(Result<std::vector<Shot>>)> done) override;
  Result<std::unique_ptr<Recorder>> start_recorder(const RecordConfig& config, RecordingDone done) override;
  void set_main_window_visible(bool visible) override;
  void set_dock_visible(bool visible) override;
  void show_selector(SelectionPurpose purpose, std::shared_ptr<const std::vector<Shot>> shots, size_t focus) override;
  void hide_selector() override;
  void show_recording_ui(const CaptureTarget& target, const BarState& state) override;
  void hide_recording_ui() override;
  void show_notice(const Notice& notice) override;
  void hide_notice() override;
  void yield_focus() override;
  void apply_theme(ThemePref theme) override;
  void recent_changed(const std::vector<std::filesystem::path>& files) override;
  void state_changed() override;
  bool tray_available() override;
  void set_tray_recording(bool recording) override;
  void set_tray_shortcuts(const ShortcutMap& shortcuts) override;
  void unregister_hotkeys() override;
  Result<void> register_hotkey(Action action, const Shortcut& shortcut) override;
  void copy_png_to_clipboard(std::shared_ptr<const std::vector<uint8_t>> png) override;
  void open_path(const std::filesystem::path& path) override;
  void reveal_path(const std::filesystem::path& path) override;
  void terminate() override;

 private:
  struct Impl;
  std::unique_ptr<Impl> impl_;
  bool termination_allowed_ = false;
};

// SnapCap hides its own floating windows from screen captures. Setting
// SNAPCAP_ALLOW_SELF_CAPTURE=1 disables that (useful for documenting the app).
bool protect_from_capture();

}  // namespace snapcap::mac
