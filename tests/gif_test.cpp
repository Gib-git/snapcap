#include <fstream>
#include <random>

#include <doctest/doctest.h>

#include "snapcap/gif.h"
#include "test_util.h"

using namespace snapcap;
using namespace std::chrono_literals;

namespace {

struct Painted {
  std::vector<uint8_t> data;
  VideoFrame frame;
};

template <class F>
Painted paint(uint32_t w, uint32_t h, int ms, F&& f) {
  Painted p;
  for (uint32_t y = 0; y < h; ++y) {
    for (uint32_t x = 0; x < w; ++x) {
      auto c = f(x, y);
      p.data.insert(p.data.end(), c.begin(), c.end());
    }
  }
  p.frame = VideoFrame{p.data, w, h, size_t{w} * 4, PixelOrder::Rgba, Duration{std::chrono::milliseconds(ms)}};
  return p;
}

std::vector<uint8_t> read_all(const std::filesystem::path& p) {
  std::ifstream in(p, std::ios::binary);
  return {std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>()};
}

}  // namespace

TEST_CASE("LZW round-trips, including table resets") {
  std::mt19937 rng(42);
  for (size_t n : {size_t{0}, size_t{1}, size_t{2}, size_t{300}, size_t{100'000}}) {
    std::vector<uint8_t> in(n);
    // Mix runs (long codes) and noise (fills the table quickly, forcing clear codes).
    for (size_t i = 0; i < n; ++i) in[i] = (i / 50) % 3 == 0 ? static_cast<uint8_t>(i / 50) : static_cast<uint8_t>(rng());
    auto enc = lzw_encode(in, 8);
    auto dec = lzw_decode(enc, 8, n);
    REQUIRE(dec.has_value());
    CHECK(*dec == in);
  }
}

TEST_CASE("NeuQuant finds the colours of a small image") {
  std::vector<uint8_t> px;
  for (int i = 0; i < 3000; ++i) {
    const uint8_t c[3][4] = {{250, 10, 10, 255}, {10, 250, 10, 255}, {10, 10, 250, 255}};
    px.insert(px.end(), c[i % 3], c[i % 3] + 4);
  }
  NeuQuant nq(px, 1, 255);
  auto pal = nq.palette_rgb();
  CHECK(pal.size() == 255 * 3);
  auto close = [&](uint8_t i, int r, int g, int b) {
    return std::abs(pal[i * 3u] - r) < 12 && std::abs(pal[i * 3u + 1] - g) < 12 && std::abs(pal[i * 3u + 2] - b) < 12;
  };
  CHECK(close(nq.index_of(250, 10, 10), 250, 10, 10));
  CHECK(close(nq.index_of(10, 250, 10), 10, 250, 10));
  CHECK(close(nq.index_of(10, 10, 250), 10, 10, 250));
}

TEST_CASE("changed_rect bounds") {
  std::vector<uint8_t> a(4 * 4 * 4, 0);
  auto b = a;
  b[(2 * 4 + 1) * 4] = 9;
  b[(3 * 4 + 2) * 4] = 9;
  CHECK(changed_rect(a, b, 4, 4) == std::array<size_t, 4>{1, 2, 2, 2});
  CHECK_FALSE(changed_rect(a, a, 4, 4).has_value());
}

TEST_CASE("delta frames reconstruct and keep time") {
  TempDir dir("gif");
  auto path = dir.path() / "test.gif";
  auto red = [](uint32_t, uint32_t) { return std::array<uint8_t, 4>{255, 0, 0, 255}; };
  // A blue square appears at (4..8, 2..4).
  auto square = [](uint32_t x, uint32_t y) {
    return x >= 4 && x < 8 && y >= 2 && y < 4 ? std::array<uint8_t, 4>{0, 0, 255, 255}
                                              : std::array<uint8_t, 4>{255, 0, 0, 255};
  };
  auto g = GifWriter::create(path, 16, 8);
  REQUIRE(g);
  CHECK((*g)->push(paint(16, 8, 0, red).frame));
  CHECK((*g)->push(paint(16, 8, 104, red).frame));  // duplicate -> merged
  for (int i = 0; i < 7; ++i) CHECK((*g)->push(paint(16, 8, 207 + i * 33, square).frame));
  CHECK((*g)->push(paint(16, 8, 433, red).frame));
  auto frames = (*g)->finish(1000ms);
  REQUIRE(frames);
  CHECK(*frames == 3);
  g->reset();

  auto bytes = read_all(path);
  auto decoded = decode_gif(bytes);
  REQUIRE(decoded);
  // 0.21 s, 0.22 s, then until 1.0 s: exactly 100 cs in total, no drift.
  uint32_t total = 0;
  for (auto d : decoded->delays_cs) total += d;
  CHECK(total == 100);
  CHECK(decoded->rects[1] == std::array<uint16_t, 4>{4, 2, 4, 2});  // only the changed square is stored
  auto px = [&](size_t frame, uint32_t x, uint32_t y) {
    const uint8_t* p = decoded->frames[frame].pixel(x, y);
    return std::array<uint8_t, 3>{p[0], p[1], p[2]};
  };
  CHECK(px(1, 5, 3) == std::array<uint8_t, 3>{0, 0, 255});
  CHECK(px(1, 0, 0) == std::array<uint8_t, 3>{255, 0, 0});
  CHECK(px(2, 5, 3) == std::array<uint8_t, 3>{255, 0, 0});
}

TEST_CASE("rich frames go through NeuQuant and BGRA is converted") {
  TempDir dir("gif-rich");
  auto path = dir.path() / "rich.gif";
  auto g = GifWriter::create(path, 64, 64);
  REQUIRE(g);
  // 4096 distinct colours, delivered as BGRA at twice the output size (downscaled).
  std::vector<uint8_t> data;
  for (uint32_t y = 0; y < 128; ++y) {
    for (uint32_t x = 0; x < 128; ++x) {
      uint8_t r = static_cast<uint8_t>(x * 2), gg = static_cast<uint8_t>(y * 2), b = 128;
      data.insert(data.end(), {b, gg, r, 255});
    }
  }
  VideoFrame f{data, 128, 128, 128 * 4, PixelOrder::Bgra, 0ms};
  CHECK((*g)->push(f));
  REQUIRE((*g)->finish(500ms));
  g->reset();
  auto decoded = decode_gif(read_all(path));
  REQUIRE(decoded);
  const uint8_t* p = decoded->frames[0].pixel(63, 0);
  CHECK(std::abs(p[0] - 253) < 24);  // red ramps along x
  CHECK(std::abs(p[2] - 128) < 24);
}

TEST_CASE("finishing without frames is an error") {
  TempDir dir("gif-empty");
  auto g = GifWriter::create(dir.path() / "empty.gif", 8, 8);
  REQUIRE(g);
  CHECK_FALSE((*g)->finish(1s).has_value());
}
