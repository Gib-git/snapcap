// Icons drawn procedurally with anti-aliased signed distance functions, so the app ships
// no image assets and every icon is crisp at any size.
#pragma once

#include <cstdint>
#include <filesystem>
#include <span>
#include <vector>

#include "snapcap/error.h"
#include "snapcap/image.h"

namespace snapcap {

// Full-colour application icon: gradient squircle, white viewfinder, coral record dot.
RgbaImage app_icon(uint32_t size);

// Tray icon. Idle: a monochrome viewfinder (black for a macOS template image, white
// elsewhere). Recording: a coral dot with a white stop square.
RgbaImage tray_icon(uint32_t size, bool recording, bool black = true);

// A Windows .ico holding PNG-compressed app icons of the given sizes.
std::vector<uint8_t> ico_bytes(std::span<const uint32_t> sizes);

// `snapcap --export-icons <dir>`: writes the icon files the packaging scripts use.
Result<void> export_icons(const std::filesystem::path& dir);

}  // namespace snapcap
