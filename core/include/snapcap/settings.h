// Persistent user settings, stored as JSON in the platform config directory.
// The file format is the one the Rust version wrote, so existing settings carry over.
#pragma once

#include <cstdint>
#include <filesystem>
#include <map>
#include <optional>
#include <string>
#include <string_view>

#include "snapcap/actions.h"
#include "snapcap/os.h"

namespace snapcap {

enum class VideoFormat { Mp4, Gif };

enum class Quality { Low, Medium, High };
// Bits per pixel per frame used to derive the H.264 target bitrate.
// Screen content compresses well, so these are deliberately modest.
double bits_per_pixel(Quality q);
std::string_view label(Quality q);
// Target H.264 bitrate in bits per second for a size, frame rate and quality.
uint32_t video_bitrate(uint32_t width, uint32_t height, uint32_t fps, Quality q);

enum class ResolutionCap { Native, P1440, P1080, P720 };
std::optional<uint32_t> max_height(ResolutionCap r);
std::string_view label(ResolutionCap r);

enum class ThemePref { System, Dark, Light };

using ShortcutMap = std::map<Action, std::string>;
ShortcutMap default_shortcuts();

struct Settings {
  // Unset means the default `~/Desktop/screenshots`.
  std::optional<std::filesystem::path> save_dir;
  VideoFormat video_format = VideoFormat::Mp4;
  uint32_t fps = 30;
  Quality quality = Quality::Medium;
  // macOS encodes in hardware, so full resolution (up to 4K) is cheap there. Elsewhere
  // 1440p keeps software encoding smooth on 4K/5K displays; Native is one click away.
  ResolutionCap resolution = kOs == Os::MacOS ? ResolutionCap::Native : ResolutionCap::P1440;
  uint32_t gif_fps = 15;
  uint32_t gif_max_width = 960;
  bool record_mic = false;
  // Microphone device name; unset = system default input.
  std::optional<std::string> mic_device;
  bool record_system_audio = false;
  uint32_t countdown_secs = 3;
  bool show_cursor = true;
  bool copy_to_clipboard = true;
  bool open_after_capture = false;
  ThemePref theme = ThemePref::System;
  // Action -> shortcut such as "Alt+Shift+2". Empty string = disabled.
  ShortcutMap shortcuts = default_shortcuts();
  // Whether the window was closed to the tray before (shows a one-time hint).
  bool tray_hint_shown = false;

  bool operator==(const Settings&) const = default;

  std::string_view shortcut(Action a) const;

  // Parses settings JSON. Missing or invalid fields keep their defaults, and values
  // are clamped to the supported ranges.
  static Settings from_json(std::string_view text);
  std::string to_json() const;

  static std::optional<std::filesystem::path> config_path();
  // Loads from `config_path()`, or returns defaults.
  static Settings load();
  // Writes atomically (temp file + rename) so a crash never leaves a truncated file.
  bool save() const;
  bool save_to(const std::filesystem::path& path) const;
};

}  // namespace snapcap
