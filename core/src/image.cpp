#include "snapcap/image.h"

#include <algorithm>
#include <cstring>

#include "snapcap/parallel.h"

namespace snapcap {

RgbaImage::RgbaImage(uint32_t w, uint32_t h, std::array<uint8_t, 4> fill) : width(w), height(h) {
  pixels.resize(size_t{w} * h * 4);
  for (size_t i = 0; i < pixels.size(); i += 4) std::memcpy(&pixels[i], fill.data(), 4);
}

RgbaImage crop(const RgbaImage& src, uint32_t x, uint32_t y, uint32_t w, uint32_t h) {
  x = std::min(x, src.width);
  y = std::min(y, src.height);
  w = std::min(w, src.width - x);
  h = std::min(h, src.height - y);
  RgbaImage out(w, h);
  for (uint32_t row = 0; row < h; ++row) {
    std::memcpy(out.pixel(0, row), src.pixel(x, y + row), size_t{w} * 4);
  }
  return out;
}

void blit(RgbaImage& dst, const RgbaImage& src, int64_t x, int64_t y) {
  int64_t x0 = std::max<int64_t>(0, x);
  int64_t y0 = std::max<int64_t>(0, y);
  int64_t x1 = std::min<int64_t>(dst.width, x + src.width);
  int64_t y1 = std::min<int64_t>(dst.height, y + src.height);
  if (x0 >= x1 || y0 >= y1) return;
  auto bytes = static_cast<size_t>(x1 - x0) * 4;
  for (int64_t row = y0; row < y1; ++row) {
    std::memcpy(dst.pixel(static_cast<uint32_t>(x0), static_cast<uint32_t>(row)),
                src.pixel(static_cast<uint32_t>(x0 - x), static_cast<uint32_t>(row - y)), bytes);
  }
}

RgbaImage to_rgba(std::span<const uint8_t> src, uint32_t width, uint32_t height, size_t stride, PixelOrder order) {
  RgbaImage out(width, height);
  const size_t ri = order == PixelOrder::Rgba ? 0 : 2;
  const size_t bi = order == PixelOrder::Rgba ? 2 : 0;
  for (uint32_t row = 0; row < height; ++row) {
    const uint8_t* in = src.data() + size_t{row} * stride;
    uint8_t* o = out.pixel(0, row);
    for (uint32_t col = 0; col < width; ++col, in += 4, o += 4) {
      o[0] = in[ri];
      o[1] = in[1];
      o[2] = in[bi];
      o[3] = 255;
    }
  }
  return out;
}

void downscale(std::span<const uint8_t> src, size_t sw, size_t sh, size_t stride, size_t dw, size_t dh,
               std::vector<uint8_t>& out) {
  out.assign(dw * dh * 4, 0);
  if (dw == 0 || dh == 0 || sw == 0 || sh == 0) return;
  // Source span per destination pixel in 16.16 fixed point.
  const uint64_t fx = (uint64_t{sw} << 16) / dw;
  const uint64_t fy = (uint64_t{sh} << 16) / dh;
  parallel_for(dh, 16, [&](size_t begin, size_t end) {
    for (size_t dy = begin; dy < end; ++dy) {
      size_t y0 = static_cast<size_t>((dy * fy) >> 16);
      size_t y1 = std::clamp(static_cast<size_t>(((dy + 1) * fy) >> 16), y0 + 1, sh);
      uint8_t* row = out.data() + dy * dw * 4;
      for (size_t dx = 0; dx < dw; ++dx) {
        size_t x0 = static_cast<size_t>((dx * fx) >> 16);
        size_t x1 = std::clamp(static_cast<size_t>(((dx + 1) * fx) >> 16), x0 + 1, sw);
        uint32_t acc[4] = {0, 0, 0, 0};
        for (size_t y = y0; y < y1; ++y) {
          const uint8_t* p = src.data() + y * stride + x0 * 4;
          for (size_t x = x0; x < x1; ++x, p += 4) {
            acc[0] += p[0];
            acc[1] += p[1];
            acc[2] += p[2];
            acc[3] += p[3];
          }
        }
        auto n = static_cast<uint32_t>((y1 - y0) * (x1 - x0));
        for (size_t c = 0; c < 4; ++c) row[dx * 4 + c] = static_cast<uint8_t>((acc[c] + n / 2) / n);
      }
    }
  });
}

RgbaImage thumbnail(const RgbaImage& src, uint32_t max_width) {
  if (src.empty()) return {};
  uint32_t tw = std::clamp(max_width, 1u, src.width);
  auto th = static_cast<uint32_t>(std::max<uint64_t>(1, uint64_t{src.height} * tw / src.width));
  RgbaImage out;
  out.width = tw;
  out.height = th;
  downscale(src.pixels, src.width, src.height, src.stride(), tw, th, out.pixels);
  return out;
}

}  // namespace snapcap
