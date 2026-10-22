#include "snapcap/icon.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <format>
#include <fstream>
#include <limits>
#include <system_error>

#include "snapcap/png.h"

namespace snapcap {
namespace {

using Rgb = std::array<float, 3>;
using Rgba = std::array<float, 4>;

constexpr Rgb kViolet{0.43f, 0.36f, 0.99f};
constexpr Rgb kBlue{0.20f, 0.55f, 0.98f};
constexpr Rgb kCoral{1.0f, 0.33f, 0.37f};
constexpr Rgb kWhite{1.0f, 1.0f, 1.0f};
constexpr Rgb kBlack{0.0f, 0.0f, 0.0f};

float sq(float v) { return v * v; }

float rounded_box(float px, float py, float cx, float cy, float hw, float hh, float r) {
  float qx = std::abs(px - cx) - (hw - r);
  float qy = std::abs(py - cy) - (hh - r);
  float outside = std::sqrt(sq(std::max(qx, 0.0f)) + sq(std::max(qy, 0.0f)));
  return outside + std::min(std::max(qx, qy), 0.0f) - r;
}

float capsule(float px, float py, float ax, float ay, float bx, float by, float r) {
  float pax = px - ax, pay = py - ay, bax = bx - ax, bay = by - ay;
  float h = std::clamp((pax * bax + pay * bay) / (bax * bax + bay * bay), 0.0f, 1.0f);
  return std::sqrt(sq(pax - bax * h) + sq(pay - bay * h)) - r;
}

// Distance to four viewfinder corner brackets inside the unit square.
float brackets(float x, float y, float inset, float len, float r) {
  float a = inset, b = 1.0f - inset;
  float d = std::numeric_limits<float>::max();
  const std::array<std::array<float, 4>, 4> corners{{{a, a, 1, 1}, {b, a, -1, 1}, {a, b, 1, -1}, {b, b, -1, -1}}};
  for (const auto& [cx, cy, sx, sy] : corners) {
    d = std::min(d, capsule(x, y, cx, cy, cx + sx * len, cy, r));
    d = std::min(d, capsule(x, y, cx, cy, cx, cy + sy * len, r));
  }
  return d;
}

float coverage(float d, float px) { return std::clamp(0.5f - d / px, 0.0f, 1.0f); }

void over(Rgba& dst, const Rgb& rgb, float a) {
  float out_a = a + dst[3] * (1.0f - a);
  if (out_a <= 0.0f) return;
  for (size_t i = 0; i < 3; ++i) dst[i] = (rgb[i] * a + dst[i] * dst[3] * (1.0f - a)) / out_a;
  dst[3] = out_a;
}

template <class F>
RgbaImage render(uint32_t size, F&& f) {
  RgbaImage img(size, size);
  float px = 1.0f / float(size);
  for (uint32_t j = 0; j < size; ++j) {
    for (uint32_t i = 0; i < size; ++i) {
      Rgba c{0, 0, 0, 0};
      f((float(i) + 0.5f) * px, (float(j) + 0.5f) * px, px, c);
      uint8_t* o = img.pixel(i, j);
      for (size_t k = 0; k < 4; ++k) o[k] = static_cast<uint8_t>(std::lround(c[k] * 255.0f));
    }
  }
  return img;
}

void put_le16(std::vector<uint8_t>& out, uint32_t v) {
  out.push_back(static_cast<uint8_t>(v));
  out.push_back(static_cast<uint8_t>(v >> 8));
}

void put_le32(std::vector<uint8_t>& out, uint32_t v) {
  put_le16(out, v & 0xFFFF);
  put_le16(out, v >> 16);
}

Result<void> write_file(const std::filesystem::path& path, std::span<const uint8_t> bytes) {
  std::ofstream out(path, std::ios::binary | std::ios::trunc);
  out.write(reinterpret_cast<const char*>(bytes.data()), static_cast<std::streamsize>(bytes.size()));
  if (!out) return fail(std::format("cannot write {}", path.string()));
  return {};
}

}  // namespace

RgbaImage app_icon(uint32_t size) {
  return render(size, [](float x, float y, float px, Rgba& c) {
    float body = coverage(rounded_box(x, y, 0.5f, 0.5f, 0.42f, 0.42f, 0.19f), px);
    if (body > 0.0f) {
      float t = std::clamp((x + y) * 0.5f, 0.0f, 1.0f);
      Rgb g{};
      for (size_t k = 0; k < 3; ++k) g[k] = kViolet[k] * (1.0f - t) + kBlue[k] * t;
      over(c, g, body);
    }
    over(c, kWhite, coverage(brackets(x, y, 0.25f, 0.15f, 0.034f), px) * body);
    float r = std::hypot(x - 0.5f, y - 0.5f);
    over(c, kWhite, coverage(std::abs(r - 0.125f) - 0.024f, px) * body);
    over(c, kCoral, coverage(r - 0.085f, px) * body);
  });
}

RgbaImage tray_icon(uint32_t size, bool recording, bool black) {
  if (recording) {
    return render(size, [](float x, float y, float px, Rgba& c) {
      over(c, kCoral, coverage(std::hypot(x - 0.5f, y - 0.5f) - 0.44f, px));
      over(c, kWhite, coverage(rounded_box(x, y, 0.5f, 0.5f, 0.15f, 0.15f, 0.04f), px));
    });
  }
  const Rgb mono = black ? kBlack : kWhite;
  return render(size, [mono](float x, float y, float px, Rgba& c) {
    over(c, mono, coverage(brackets(x, y, 0.12f, 0.24f, 0.055f), px));
    over(c, mono, coverage(std::hypot(x - 0.5f, y - 0.5f) - 0.16f, px));
  });
}

std::vector<uint8_t> ico_bytes(std::span<const uint32_t> sizes) {
  std::vector<std::vector<uint8_t>> images;
  for (uint32_t s : sizes) images.push_back(encode_png(app_icon(s), 9).value_or(std::vector<uint8_t>{}));
  std::vector<uint8_t> out{0, 0, 1, 0};
  put_le16(out, static_cast<uint32_t>(sizes.size()));
  auto offset = static_cast<uint32_t>(6 + 16 * sizes.size());
  for (size_t i = 0; i < sizes.size(); ++i) {
    auto dim = static_cast<uint8_t>(sizes[i] >= 256 ? 0 : sizes[i]);
    out.insert(out.end(), {dim, dim, 0, 0, 1, 0, 32, 0});
    put_le32(out, static_cast<uint32_t>(images[i].size()));
    put_le32(out, offset);
    offset += static_cast<uint32_t>(images[i].size());
  }
  for (const auto& img : images) out.insert(out.end(), img.begin(), img.end());
  return out;
}

Result<void> export_icons(const std::filesystem::path& dir) {
  std::error_code ec;
  std::filesystem::create_directories(dir, ec);
  for (uint32_t s : {16u, 32u, 64u, 128u, 256u, 512u, 1024u}) {
    if (auto r = write_png(dir / std::format("icon_{}.png", s), app_icon(s), 9); !r) return r;
  }
  if (auto r = write_png(dir / "icon.png", app_icon(512), 9); !r) return r;
  constexpr std::array<uint32_t, 7> kIcoSizes{16, 24, 32, 48, 64, 128, 256};
  return write_file(dir / "icon.ico", ico_bytes(kIcoSizes));
}

}  // namespace snapcap
