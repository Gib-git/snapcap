#include <doctest/doctest.h>

#include "snapcap/image.h"

using namespace snapcap;

TEST_CASE("downscale averages") {
  // 2x2 -> 1x1: black, white, black, white -> mid grey
  std::vector<uint8_t> src{0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255};
  std::vector<uint8_t> out;
  downscale(src, 2, 2, 8, 1, 1, out);
  CHECK(out == std::vector<uint8_t>{128, 128, 128, 255});
}

TEST_CASE("downscale of a large image uses every row") {
  RgbaImage img(1000, 700, {10, 20, 30, 255});
  RgbaImage t = thumbnail(img);
  CHECK(t.width == 320);
  CHECK(t.height == 224);
  for (uint32_t y = 0; y < t.height; ++y) CHECK(t.pixel(319, y)[2] == 30);
}

TEST_CASE("crop copies the right pixels and clamps") {
  RgbaImage img(3, 2);
  for (uint32_t i = 0; i < 6; ++i) img.pixels[i * 4] = static_cast<uint8_t>(i);
  RgbaImage c = crop(img, 1, 1, 2, 1);
  CHECK(c.width == 2);
  CHECK(c.pixel(0, 0)[0] == 4);
  CHECK(c.pixel(1, 0)[0] == 5);
  CHECK(crop(img, 2, 1, 50, 50).width == 1);
  CHECK(crop(img, 9, 9, 1, 1).empty());
}

TEST_CASE("to_rgba honours stride and byte order") {
  // 1x2 BGRA with 8 bytes of row padding.
  std::vector<uint8_t> src{3, 2, 1, 0, 9, 9, 9, 9, 6, 5, 4, 0, 9, 9, 9, 9};
  RgbaImage img = to_rgba(src, 1, 2, 8, PixelOrder::Bgra);
  CHECK(img.pixels == std::vector<uint8_t>{1, 2, 3, 255, 4, 5, 6, 255});
}

TEST_CASE("blit clips at the edges") {
  RgbaImage dst(4, 4);
  RgbaImage src(3, 3, {7, 7, 7, 7});
  blit(dst, src, -1, 2);
  CHECK(dst.pixel(0, 2)[0] == 7);
  CHECK(dst.pixel(1, 3)[0] == 7);
  CHECK(dst.pixel(2, 3)[0] == 0);
  CHECK(dst.pixel(0, 1)[0] == 0);
}
