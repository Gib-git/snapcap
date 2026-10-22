// RGBA images and the pixel operations shared by screenshots, thumbnails and GIFs.
#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <vector>

namespace snapcap {

// Byte order of 4-byte pixels coming from a capture source.
enum class PixelOrder { Rgba, Bgra };

// Tightly packed, straight-alpha RGBA8.
struct RgbaImage {
  uint32_t width = 0;
  uint32_t height = 0;
  std::vector<uint8_t> pixels;

  RgbaImage() = default;
  RgbaImage(uint32_t w, uint32_t h, std::array<uint8_t, 4> fill = {0, 0, 0, 0});

  bool empty() const { return width == 0 || height == 0; }
  size_t stride() const { return size_t{width} * 4; }
  const uint8_t* pixel(uint32_t x, uint32_t y) const { return pixels.data() + (size_t{y} * width + x) * 4; }
  uint8_t* pixel(uint32_t x, uint32_t y) { return pixels.data() + (size_t{y} * width + x) * 4; }
};

// Copies `w x h` pixels at (x, y). The rectangle is clamped to the image.
RgbaImage crop(const RgbaImage& src, uint32_t x, uint32_t y, uint32_t w, uint32_t h);

// Copies `src` into `dst` with its top-left at (x, y), clipping at the edges.
void blit(RgbaImage& dst, const RgbaImage& src, int64_t x, int64_t y);

// Repacks rows of 4-byte pixels (any stride, either byte order) into opaque RGBA.
RgbaImage to_rgba(std::span<const uint8_t> src, uint32_t width, uint32_t height, size_t stride, PixelOrder order);

// Box-filter downscale of 4-byte pixels (byte order is preserved) into a packed `dw x dh` buffer.
void downscale(std::span<const uint8_t> src, size_t sw, size_t sh, size_t stride, size_t dw, size_t dh,
               std::vector<uint8_t>& out);

// Thumbnail at most 320 pixels wide, keeping the aspect ratio.
inline constexpr uint32_t kThumbWidth = 320;
RgbaImage thumbnail(const RgbaImage& src, uint32_t max_width = kThumbWidth);

}  // namespace snapcap
