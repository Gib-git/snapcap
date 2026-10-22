#include "snapcap/settings.h"

#include <algorithm>
#include <fstream>
#include <sstream>
#include <system_error>

#include <nlohmann/json.hpp>

#include "snapcap/log.h"
#include "snapcap/paths.h"

namespace snapcap {
namespace fs = std::filesystem;
using nlohmann::json;

double bits_per_pixel(Quality q) {
  switch (q) {
    case Quality::Low: return 0.05;
    case Quality::Medium: return 0.09;
    case Quality::High: return 0.16;
  }
  return 0.09;
}

std::string_view label(Quality q) {
  switch (q) {
    case Quality::Low: return "Small file";
    case Quality::Medium: return "Balanced";
    case Quality::High: return "High quality";
  }
  return "";
}

uint32_t video_bitrate(uint32_t width, uint32_t height, uint32_t fps, Quality q) {
  double bps = double(width) * height * std::min(fps, 30u) * bits_per_pixel(q);
  return static_cast<uint32_t>(std::clamp(bps, 500'000.0, 40'000'000.0));
}

std::optional<uint32_t> max_height(ResolutionCap r) {
  switch (r) {
    case ResolutionCap::Native: return std::nullopt;
    case ResolutionCap::P1440: return 1440;
    case ResolutionCap::P1080: return 1080;
    case ResolutionCap::P720: return 720;
  }
  return std::nullopt;
}

std::string_view label(ResolutionCap r) {
  switch (r) {
    case ResolutionCap::Native: return "Native";
    case ResolutionCap::P1440: return "1440p";
    case ResolutionCap::P1080: return "1080p";
    case ResolutionCap::P720: return "720p";
  }
  return "";
}

ShortcutMap default_shortcuts() {
  ShortcutMap m;
  for (Action a : kAllActions) m[a] = std::string(default_shortcut(a));
  return m;
}

std::string_view Settings::shortcut(Action a) const {
  auto it = shortcuts.find(a);
  return it == shortcuts.end() ? std::string_view() : std::string_view(it->second);
}

namespace {

// Enum <-> serde variant name tables.
template <class E, size_t N>
using Names = std::array<std::pair<E, const char*>, N>;

constexpr Names<VideoFormat, 2> kFormats{{{VideoFormat::Mp4, "Mp4"}, {VideoFormat::Gif, "Gif"}}};
constexpr Names<Quality, 3> kQualities{{{Quality::Low, "Low"}, {Quality::Medium, "Medium"}, {Quality::High, "High"}}};
constexpr Names<ResolutionCap, 4> kResolutions{{{ResolutionCap::Native, "Native"},
                                                {ResolutionCap::P1440, "P1440"},
                                                {ResolutionCap::P1080, "P1080"},
                                                {ResolutionCap::P720, "P720"}}};
constexpr Names<ThemePref, 3> kThemes{{{ThemePref::System, "System"}, {ThemePref::Dark, "Dark"}, {ThemePref::Light, "Light"}}};

template <class E, size_t N>
const char* name_of(const Names<E, N>& names, E value) {
  for (const auto& [e, n] : names) {
    if (e == value) return n;
  }
  return names[0].second;
}

template <class E, size_t N>
void read_enum(const json& j, const char* key, const Names<E, N>& names, E& out) {
  auto it = j.find(key);
  if (it == j.end() || !it->is_string()) return;
  const auto& s = it->template get_ref<const std::string&>();
  for (const auto& [e, n] : names) {
    if (s == n) out = e;
  }
}

void read_bool(const json& j, const char* key, bool& out) {
  auto it = j.find(key);
  if (it != j.end() && it->is_boolean()) out = it->get<bool>();
}

void read_u32(const json& j, const char* key, uint32_t& out) {
  auto it = j.find(key);
  if (it != j.end() && it->is_number_unsigned()) {
    out = static_cast<uint32_t>(std::min<uint64_t>(it->get<uint64_t>(), UINT32_MAX));
  }
}

template <class T>
void read_optional_string(const json& j, const char* key, std::optional<T>& out) {
  auto it = j.find(key);
  if (it == j.end()) return;
  if (it->is_null()) {
    out.reset();
  } else if (it->is_string()) {
    out = T(it->get<std::string>());
  }
}

}  // namespace

Settings Settings::from_json(std::string_view text) {
  Settings s;
  json j = json::parse(text.begin(), text.end(), nullptr, /*allow_exceptions=*/false);
  if (!j.is_object()) {
    log::warn("ignoring malformed settings");
    return s;
  }
  read_optional_string(j, "save_dir", s.save_dir);
  read_enum(j, "video_format", kFormats, s.video_format);
  read_u32(j, "fps", s.fps);
  read_enum(j, "quality", kQualities, s.quality);
  read_enum(j, "resolution", kResolutions, s.resolution);
  read_u32(j, "gif_fps", s.gif_fps);
  read_u32(j, "gif_max_width", s.gif_max_width);
  read_bool(j, "record_mic", s.record_mic);
  read_optional_string(j, "mic_device", s.mic_device);
  read_bool(j, "record_system_audio", s.record_system_audio);
  read_u32(j, "countdown_secs", s.countdown_secs);
  read_bool(j, "show_cursor", s.show_cursor);
  read_bool(j, "copy_to_clipboard", s.copy_to_clipboard);
  read_bool(j, "open_after_capture", s.open_after_capture);
  read_enum(j, "theme", kThemes, s.theme);
  read_bool(j, "tray_hint_shown", s.tray_hint_shown);
  if (auto it = j.find("shortcuts"); it != j.end() && it->is_object()) {
    for (const auto& [key, value] : it->items()) {
      auto action = from_settings_key(key);
      if (action && value.is_string()) s.shortcuts[*action] = value.get<std::string>();
    }
  }
  // Actions added in newer versions get their default shortcut (already present from
  // the defaults above). Keep values in the ranges the UI offers.
  s.fps = std::clamp<uint32_t>(s.fps, 5, 60);
  s.gif_fps = std::clamp<uint32_t>(s.gif_fps, 5, 30);
  s.gif_max_width = std::clamp<uint32_t>(s.gif_max_width, 160, 3840);
  s.countdown_secs = std::min<uint32_t>(s.countdown_secs, 10);
  return s;
}

std::string Settings::to_json() const {
  json shortcuts_json = json::object();
  for (const auto& [action, text] : shortcuts) shortcuts_json[std::string(settings_key(action))] = text;
  json j = {
      {"save_dir", save_dir ? json(save_dir->string()) : json(nullptr)},
      {"video_format", name_of(kFormats, video_format)},
      {"fps", fps},
      {"quality", name_of(kQualities, quality)},
      {"resolution", name_of(kResolutions, resolution)},
      {"gif_fps", gif_fps},
      {"gif_max_width", gif_max_width},
      {"record_mic", record_mic},
      {"mic_device", mic_device ? json(*mic_device) : json(nullptr)},
      {"record_system_audio", record_system_audio},
      {"countdown_secs", countdown_secs},
      {"show_cursor", show_cursor},
      {"copy_to_clipboard", copy_to_clipboard},
      {"open_after_capture", open_after_capture},
      {"theme", name_of(kThemes, theme)},
      {"shortcuts", shortcuts_json},
      {"tray_hint_shown", tray_hint_shown},
  };
  return j.dump(2);
}

std::optional<fs::path> Settings::config_path() {
  return paths::config_dir().transform([](const fs::path& d) { return d / "SnapCap" / "settings.json"; });
}

Settings Settings::load() {
  auto path = config_path();
  if (!path) return {};
  std::ifstream in(*path, std::ios::binary);
  if (!in) return {};
  std::stringstream buf;
  buf << in.rdbuf();
  return from_json(buf.str());
}

bool Settings::save() const {
  auto path = config_path();
  return path && save_to(*path);
}

bool Settings::save_to(const fs::path& path) const {
  std::error_code ec;
  fs::create_directories(path.parent_path(), ec);
  fs::path tmp = path;
  tmp += ".tmp";
  {
    std::ofstream out(tmp, std::ios::binary | std::ios::trunc);
    if (!out) {
      log::error("cannot write {}", tmp.string());
      return false;
    }
    out << to_json();
    out.flush();
    if (!out) {
      log::error("cannot write {}", tmp.string());
      return false;
    }
  }
  fs::rename(tmp, path, ec);
  if (ec) {
    log::error("cannot replace {}: {}", path.string(), ec.message());
    fs::remove(tmp, ec);
    return false;
  }
  return true;
}

}  // namespace snapcap
