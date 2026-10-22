#pragma once

#include <atomic>
#include <filesystem>
#include <format>
#include <string>
#include <system_error>

#include <unistd.h>

// A fresh temporary directory, removed when the object goes out of scope.
class TempDir {
 public:
  explicit TempDir(const std::string& tag) {
    static std::atomic<int> counter{0};
    path_ = std::filesystem::temp_directory_path() / std::format("snapcap-{}-{}-{}", tag, getpid(), counter++);
    std::error_code ec;
    std::filesystem::remove_all(path_, ec);
    std::filesystem::create_directories(path_);
  }
  ~TempDir() {
    std::error_code ec;
    std::filesystem::remove_all(path_, ec);
  }
  TempDir(const TempDir&) = delete;
  TempDir& operator=(const TempDir&) = delete;
  const std::filesystem::path& path() const { return path_; }

 private:
  std::filesystem::path path_;
};
