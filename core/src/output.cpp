#include "snapcap/output.h"

#include <format>
#include <system_error>

#include "snapcap/paths.h"

namespace snapcap {
namespace fs = std::filesystem;

fs::path default_save_dir() {
  auto desktop = paths::desktop_dir();
  return (desktop ? *desktop : fs::path(".")) / "screenshots";
}

fs::path save_dir(const Settings& settings) {
  return settings.save_dir.value_or(default_save_dir());
}

std::string capture_file_name(CaptureKind kind, std::string_view ext, const std::tm& when) {
  const char* prefix = kind == CaptureKind::Screenshot ? "Screenshot" : "Recording";
  return std::format("{} {:04}-{:02}-{:02} at {:02}.{:02}.{:02}.{}", prefix, when.tm_year + 1900, when.tm_mon + 1,
                     when.tm_mday, when.tm_hour, when.tm_min, when.tm_sec, ext);
}

Result<fs::path> new_capture_path(const fs::path& dir, CaptureKind kind, std::string_view ext) {
  std::error_code ec;
  fs::create_directories(dir, ec);
  if (ec || !fs::is_directory(dir)) return fail(std::format("cannot create {}", dir.string()));
  std::time_t now = std::time(nullptr);
  std::tm local{};
#if defined(_WIN32)
  localtime_s(&local, &now);
#else
  localtime_r(&now, &local);
#endif
  return unique_path(dir / capture_file_name(kind, ext, local));
}

fs::path unique_path(const fs::path& path) {
  std::error_code ec;
  if (!fs::exists(path, ec)) return path;
  std::string stem = path.stem().string();
  std::string ext = path.extension().string();
  for (int n = 2;; ++n) {
    fs::path candidate = path.parent_path() / std::format("{} ({}){}", stem, n, ext);
    if (!fs::exists(candidate, ec)) return candidate;
  }
}

}  // namespace snapcap
