#include "snapcap/recent.h"

#include <algorithm>
#include <cctype>
#include <system_error>

namespace snapcap {
namespace fs = std::filesystem;

std::optional<MediaKind> media_kind(const fs::path& p) {
  std::string ext = p.extension().string();
  std::ranges::transform(ext, ext.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
  if (ext == ".png") return MediaKind::Png;
  if (ext == ".mp4") return MediaKind::Mp4;
  if (ext == ".gif") return MediaKind::Gif;
  return std::nullopt;
}

std::string_view label(MediaKind k) {
  switch (k) {
    case MediaKind::Png: return "PNG";
    case MediaKind::Mp4: return "MP4";
    case MediaKind::Gif: return "GIF";
  }
  return "";
}

std::vector<fs::path> scan_recent(const fs::path& dir) {
  std::error_code ec;
  std::vector<std::pair<fs::file_time_type, fs::path>> files;
  for (fs::directory_iterator it(dir, ec), end; !ec && it != end; it.increment(ec)) {
    const fs::path& p = it->path();
    if (!media_kind(p) || !it->is_regular_file(ec)) continue;
    auto t = it->last_write_time(ec);
    if (ec) {
      ec.clear();
      continue;
    }
    files.emplace_back(t, p);
  }
  std::ranges::sort(files, [](const auto& a, const auto& b) { return a.first > b.first; });
  std::vector<fs::path> out;
  for (size_t i = 0; i < files.size() && i < kMaxRecent; ++i) out.push_back(std::move(files[i].second));
  return out;
}

}  // namespace snapcap
