// Minimal logging. Platforms install a sink (os_log on macOS); the default writes to stderr.
#pragma once

#include <filesystem>
#include <format>
#include <functional>
#include <string>
#include <string_view>

namespace snapcap::log {

enum class Level { Debug, Info, Warn, Error };

using Sink = std::function<void(Level, std::string_view)>;

// Replaces the sink. Call once at startup, before other threads log.
void set_sink(Sink sink);
// Also appends every message, timestamped, to `path`. A file over 1 MB is moved to
// `<path>.old` first, so logs never grow without bound.
void open_file(const std::filesystem::path& path);
void write(Level level, std::string_view message);

template <class... Args>
void info(std::format_string<Args...> fmt, Args&&... args) {
  write(Level::Info, std::format(fmt, std::forward<Args>(args)...));
}
template <class... Args>
void warn(std::format_string<Args...> fmt, Args&&... args) {
  write(Level::Warn, std::format(fmt, std::forward<Args>(args)...));
}
template <class... Args>
void error(std::format_string<Args...> fmt, Args&&... args) {
  write(Level::Error, std::format(fmt, std::forward<Args>(args)...));
}

}  // namespace snapcap::log
