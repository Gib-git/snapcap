#include "snapcap/log.h"

#include <chrono>
#include <cstdio>
#include <ctime>
#include <mutex>
#include <system_error>

namespace snapcap::log {
namespace {

std::mutex g_mutex;
std::FILE* g_file = nullptr;

void stderr_sink(Level level, std::string_view message) {
  static constexpr const char* kNames[] = {"debug", "info", "warn", "error"};
  std::fprintf(stderr, "snapcap %s: %.*s\n", kNames[static_cast<int>(level)], static_cast<int>(message.size()),
               message.data());
}

Sink& sink() {
  static Sink s = stderr_sink;
  return s;
}

}  // namespace

void set_sink(Sink s) {
  std::lock_guard lock(g_mutex);
  sink() = s ? std::move(s) : Sink(stderr_sink);
}

void open_file(const std::filesystem::path& path) {
  std::error_code ec;
  std::filesystem::create_directories(path.parent_path(), ec);
  if (std::filesystem::file_size(path, ec) > (1u << 20)) {
    std::filesystem::path old = path;
    old += ".old";
    std::filesystem::rename(path, old, ec);
  }
  std::lock_guard lock(g_mutex);
  if (g_file != nullptr) std::fclose(g_file);
  g_file = std::fopen(path.string().c_str(), "a");
}

void write(Level level, std::string_view message) {
  std::lock_guard lock(g_mutex);
  sink()(level, message);
  if (g_file != nullptr) {
    static constexpr const char* kNames[] = {"debug", "info", "warn", "error"};
    // Local time, matching the capture file names.
    auto now = std::chrono::system_clock::now();
    std::time_t secs = std::chrono::system_clock::to_time_t(now);
    auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(now.time_since_epoch()).count() % 1000;
    std::tm local{};
#if defined(_WIN32)
    localtime_s(&local, &secs);
#else
    localtime_r(&secs, &local);
#endif
    char stamp[32];
    std::strftime(stamp, sizeof(stamp), "%Y-%m-%d %H:%M:%S", &local);
    std::string line = std::format("{}.{:03} {} {}\n", stamp, ms, kNames[static_cast<int>(level)], message);
    std::fwrite(line.data(), 1, line.size(), g_file);
    std::fflush(g_file);
  }
}

}  // namespace snapcap::log
