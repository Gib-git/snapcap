// Where captures are written and how they are named.
#pragma once

#include <ctime>
#include <filesystem>
#include <string>
#include <string_view>

#include "snapcap/error.h"
#include "snapcap/settings.h"

namespace snapcap {

enum class CaptureKind { Screenshot, Recording };

// `<Desktop>/screenshots`.
std::filesystem::path default_save_dir();
std::filesystem::path save_dir(const Settings& settings);

// e.g. "Screenshot 2026-10-07 at 18.56.12.png". Dots instead of colons keep it valid on Windows.
std::string capture_file_name(CaptureKind kind, std::string_view ext, const std::tm& when);

// Creates the folder and returns a path that does not exist yet.
Result<std::filesystem::path> new_capture_path(const std::filesystem::path& dir, CaptureKind kind,
                                               std::string_view ext);

// `path`, or "name (2).ext", "name (3).ext"... whichever does not exist yet.
std::filesystem::path unique_path(const std::filesystem::path& path);

}  // namespace snapcap
