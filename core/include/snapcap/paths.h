// Well-known user folders.
#pragma once

#include <filesystem>
#include <optional>

namespace snapcap::paths {

std::optional<std::filesystem::path> home_dir();

// Per-user configuration root: ~/Library/Application Support (macOS), %APPDATA% (Windows),
// $XDG_CONFIG_HOME or ~/.config (Linux). `SNAPCAP_CONFIG_DIR` overrides it (used by tests).
std::optional<std::filesystem::path> config_dir();

// The user's Desktop. On Linux this honours XDG user dirs, which may be localised.
std::optional<std::filesystem::path> desktop_dir();

}  // namespace snapcap::paths
