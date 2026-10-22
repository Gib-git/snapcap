#include "snapcap/paths.h"

#include <cstdlib>
#include <fstream>
#include <string>

#include "snapcap/os.h"

#if !defined(_WIN32)
#include <pwd.h>
#include <unistd.h>
#endif

namespace snapcap::paths {
namespace fs = std::filesystem;

namespace {

std::optional<std::string> env(const char* name) {
  const char* v = std::getenv(name);  // NOLINT(concurrency-mt-unsafe): read-only after startup
  if (v == nullptr || *v == '\0') return std::nullopt;
  return std::string(v);
}

// Reads XDG_DESKTOP_DIR from user-dirs.dirs, e.g. XDG_DESKTOP_DIR="$HOME/Schreibtisch".
[[maybe_unused]] std::optional<fs::path> xdg_desktop(const fs::path& home, const fs::path& config) {
  std::ifstream in(config / "user-dirs.dirs");
  std::string line;
  while (std::getline(in, line)) {
    constexpr std::string_view kKey = "XDG_DESKTOP_DIR=";
    if (!line.starts_with(kKey)) continue;
    std::string value = line.substr(kKey.size());
    if (value.size() >= 2 && value.front() == '"' && value.back() == '"') value = value.substr(1, value.size() - 2);
    constexpr std::string_view kHome = "$HOME";
    if (value.starts_with(kHome)) return home / fs::path(value.substr(kHome.size())).relative_path();
    if (fs::path(value).is_absolute()) return fs::path(value);
  }
  return std::nullopt;
}

}  // namespace

std::optional<fs::path> home_dir() {
#if defined(_WIN32)
  if (auto p = env("USERPROFILE")) return fs::path(*p);
  return std::nullopt;
#else
  if (auto p = env("HOME")) return fs::path(*p);
  if (const passwd* pw = getpwuid(getuid()); pw != nullptr && pw->pw_dir != nullptr) return fs::path(pw->pw_dir);
  return std::nullopt;
#endif
}

std::optional<fs::path> config_dir() {
  if (auto p = env("SNAPCAP_CONFIG_DIR")) return fs::path(*p);
  if constexpr (kOs == Os::Windows) {
    if (auto p = env("APPDATA")) return fs::path(*p);
    return std::nullopt;
  }
  auto home = home_dir();
  if constexpr (kOs == Os::MacOS) {
    if (!home) return std::nullopt;
    return *home / "Library" / "Application Support";
  }
  if (auto p = env("XDG_CONFIG_HOME")) return fs::path(*p);
  if (!home) return std::nullopt;
  return *home / ".config";
}

std::optional<fs::path> desktop_dir() {
  auto home = home_dir();
  if (!home) return std::nullopt;
  if constexpr (kOs == Os::Linux) {
    fs::path config = env("XDG_CONFIG_HOME").transform([](std::string s) { return fs::path(s); }).value_or(*home / ".config");
    if (auto d = xdg_desktop(*home, config)) return d;
  }
  return *home / "Desktop";
}

}  // namespace snapcap::paths
