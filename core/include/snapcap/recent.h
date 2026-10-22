// Recent captures shown in the main window.
#pragma once

#include <filesystem>
#include <optional>
#include <string_view>
#include <vector>

namespace snapcap {

inline constexpr size_t kMaxRecent = 12;

enum class MediaKind { Png, Mp4, Gif };

std::optional<MediaKind> media_kind(const std::filesystem::path& p);
std::string_view label(MediaKind k);

// Newest media files (by modification time) in `dir`, at most kMaxRecent.
std::vector<std::filesystem::path> scan_recent(const std::filesystem::path& dir);

}  // namespace snapcap
