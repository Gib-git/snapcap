#include <chrono>
#include <cstring>

#include <doctest/doctest.h>
#include <zlib.h>

#include "snapcap/png.h"

using namespace snapcap;

namespace {

uint32_t be32(const uint8_t* p) {
  return uint32_t{p[0]} << 24 | uint32_t{p[1]} << 16 | uint32_t{p[2]} << 8 | p[3];
}

// Minimal PNG reader for 8-bit RGBA, enough to check the encoder: verifies chunk CRCs,
// inflates IDAT and undoes the filters.
std::optional<RgbaImage> decode(const std::vector<uint8_t>& png) {
  if (png.size() < 8 || std::memcmp(png.data(), "\x89PNG\r\n\x1a\n", 8) != 0) return std::nullopt;
  RgbaImage img;
  std::vector<uint8_t> idat;
  for (size_t pos = 8; pos + 12 <= png.size();) {
    uint32_t len = be32(&png[pos]);
    const uint8_t* type = &png[pos + 4];
    const uint8_t* data = type + 4;
    uint32_t crc = be32(data + len);
    if (crc32(0, type, len + 4) != crc) return std::nullopt;
    if (std::memcmp(type, "IHDR", 4) == 0) {
      img.width = be32(data);
      img.height = be32(data + 4);
      if (data[8] != 8 || data[9] != 6) return std::nullopt;
    } else if (std::memcmp(type, "IDAT", 4) == 0) {
      idat.insert(idat.end(), data, data + len);
    }
    pos += 12 + len;
  }
  size_t row = size_t{img.width} * 4;
  std::vector<uint8_t> raw((row + 1) * img.height);
  uLongf raw_len = raw.size();
  if (uncompress(raw.data(), &raw_len, idat.data(), idat.size()) != Z_OK || raw_len != raw.size()) return std::nullopt;
  img.pixels.resize(row * img.height);
  for (uint32_t y = 0; y < img.height; ++y) {
    const uint8_t* in = &raw[y * (row + 1)];
    uint8_t* out = &img.pixels[y * row];
    const uint8_t* prev = y > 0 ? out - row : nullptr;
    for (size_t i = 0; i < row; ++i) {
      uint8_t left = i >= 4 ? out[i - 4] : 0;
      uint8_t up = prev ? prev[i] : 0;
      switch (in[0]) {
        case 0: out[i] = in[1 + i]; break;
        case 1: out[i] = static_cast<uint8_t>(in[1 + i] + left); break;
        case 2: out[i] = static_cast<uint8_t>(in[1 + i] + up); break;
        default: return std::nullopt;
      }
    }
  }
  return img;
}

}  // namespace

TEST_CASE("PNG round-trips small and multi-piece images") {
  for (auto [w, h] : {std::pair{1u, 1u}, std::pair{7u, 3u}, std::pair{640u, 1500u}}) {
    RgbaImage img(w, h);
    for (size_t i = 0; i < img.pixels.size(); ++i) img.pixels[i] = static_cast<uint8_t>((i * 31) ^ (i >> 7));
    auto png = encode_png(img);
    REQUIRE(png);
    auto back = decode(*png);
    REQUIRE(back.has_value());
    CHECK(back->width == w);
    CHECK(back->height == h);
    CHECK(back->pixels == img.pixels);
  }
}

TEST_CASE("PNG rejects empty images") {
  CHECK_FALSE(encode_png(RgbaImage{}).has_value());
}

// Opt-in benchmark: `snapcap_core_tests -tc="png encode speed*" --no-skip` on a release build.
TEST_CASE("png encode speed (5K screen-like content)" * doctest::skip()) {
  RgbaImage img(5120, 2880, {236, 236, 236, 255});
  for (uint32_t y = 0; y < img.height; ++y) {
    for (uint32_t x = 0; x < img.width; ++x) {
      if ((x / 7 + y / 13) % 11 == 0) img.pixel(x, y)[0] = static_cast<uint8_t>(x * y);  // text-like detail
    }
  }
  auto t0 = std::chrono::steady_clock::now();
  auto png = encode_png(img);
  auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - t0).count();
  REQUIRE(png);
  MESSAGE("5120x2880: " << ms << " ms, " << png->size() / 1024 << " KiB");
}
