#include <doctest/doctest.h>

#include "snapcap/geometry.h"

using namespace snapcap;

namespace {
MonitorInfo mon(size_t index, int32_t x, int32_t y, uint32_t w, uint32_t h) {
  MonitorInfo m;
  m.index = index;
  m.x = x;
  m.y = y;
  m.width = w;
  m.height = h;
  m.primary = index == 0;
  return m;
}
}  // namespace

TEST_CASE("even rect is clamped") {
  CHECK(PxRect{10, 10, 101, 999}.even_within(100, 50) == PxRect{10, 10, 90, 40});
  PxRect r = PxRect{99, 0, 5, 5}.even_within(100, 50);
  CHECK(r.x == 98);
  CHECK(r.w == 2);
}

TEST_CASE("stitch places monitors side by side") {
  std::vector<Shot> shots{{mon(0, 0, 0, 4, 2), RgbaImage(4, 2, {255, 0, 0, 255})},
                          {mon(1, 4, 0, 2, 2), RgbaImage(2, 2, {0, 255, 0, 255})}};
  RgbaImage img = stitch(shots);
  CHECK(img.width == 6);
  CHECK(img.height == 2);
  CHECK(img.pixel(5, 1)[1] == 255);
  CHECK(img.pixel(0, 0)[0] == 255);
}

TEST_CASE("stitch handles negative origins, density and gaps") {
  // A Retina display (2x) left of and above a 1x display.
  MonitorInfo a = mon(0, -100, -50, 100, 50);
  MonitorInfo b = mon(1, 0, 0, 10, 10);
  std::vector<Shot> shots{{a, RgbaImage(200, 100, {1, 1, 1, 255})}, {b, RgbaImage(10, 10, {9, 9, 9, 255})}};
  RgbaImage img = stitch(shots);
  CHECK(img.width == 210);
  CHECK(img.height == 110);
  CHECK(img.pixel(205, 105)[0] == 9);
  CHECK(img.pixel(0, 0)[0] == 1);
  CHECK(img.pixel(0, 105)[0] == 0);  // gap is black
  CHECK(img.pixel(0, 105)[3] == 255);
}

TEST_CASE("desktop rect maps pixels to desktop units") {
  MonitorInfo m = mon(0, 100, 50, 1000, 500);
  m.scale = 2.0;
  CaptureTarget t{m, 2000, 1000, PxRect{200, 100, 400, 200}};
  RectF r = t.desktop_rect();
  if (kDesktopInPoints) {
    CHECK(r == RectF{200, 100, 200, 100});
  } else {
    CHECK(r == RectF{300, 150, 400, 200});
  }
}

TEST_CASE("monitor under cursor falls back to primary") {
  std::vector<MonitorInfo> ms{mon(0, 0, 0, 100, 100), mon(1, 100, 0, 100, 100)};
  CHECK(monitor_under(ms, PointF{150, 10}) == 1);
  CHECK(monitor_under(ms, PointF{5000, 10}) == 0);
  CHECK(monitor_under(ms, std::nullopt) == 0);
  ms[0].primary = false;
  ms[1].primary = true;
  CHECK(monitor_under(ms, std::nullopt) == 1);
}

TEST_CASE("fit_even caps and rounds") {
  CHECK(fit_even(3840, 2160, 1080, std::nullopt) == std::pair{1920u, 1080u});
  CHECK(fit_even(1001, 501, std::nullopt, std::nullopt) == std::pair{1000u, 500u});
  CHECK(fit_even(1920, 1080, std::nullopt, 960) == std::pair{960u, 540u});
}

TEST_CASE("output size respects encoder limits") {
  CHECK(output_size(VideoFormat::Mp4, ResolutionCap::Native, 960, 5120, 2880) == std::pair{3840u, 2160u});
  CHECK(output_size(VideoFormat::Mp4, ResolutionCap::Native, 960, 2160, 3840) == std::pair{2160u, 3840u});
  CHECK(output_size(VideoFormat::Mp4, ResolutionCap::Native, 960, 1921, 1081) == std::pair{1920u, 1080u});
  CHECK(output_size(VideoFormat::Mp4, ResolutionCap::P720, 960, 5120, 2880) == std::pair{1280u, 720u});
  CHECK(output_size(VideoFormat::Gif, ResolutionCap::Native, 960, 5120, 2880) == std::pair{960u, 540u});
}

TEST_CASE("notice sits in the bottom-right corner of the work area") {
  SizeF notice{340, 84};
  // Dock at the bottom: work area excludes the menu bar (top) and the Dock (bottom).
  CHECK(notice_origin(RectF{0, 25, 1512, 870}, notice) == PointF{1512 - 340 - 16, 25 + 870 - 84 - 16});
  // Dock on the left: the work area starts further right, but the corner is the same.
  CHECK(notice_origin(RectF{80, 25, 1432, 957}, notice) == PointF{1512 - 340 - 16, 982 - 84 - 16});
  // A secondary display above and left of the primary (negative coordinates).
  CHECK(notice_origin(RectF{-1920, -1080, 1920, 1050}, notice) == PointF{-340 - 16, -30 - 84 - 16});
  // A work area smaller than the notice keeps it on screen at the top-left.
  CHECK(notice_origin(RectF{10, 10, 100, 50}, notice) == PointF{10, 10});
}

TEST_CASE("recording bar placement") {
  RectF screen{0, 0, 1000, 800};
  SizeF bar{236, 48};
  // Full screen: centred near the bottom.
  CHECK(recording_bar_origin(screen, screen, true, bar) == PointF{382, 800 - 48 - 96});
  // Region with room below: centred under it.
  CHECK(recording_bar_origin(screen, RectF{100, 100, 400, 300}, false, bar) == PointF{182, 412});
  // Region touching the bottom: goes above it.
  CHECK(recording_bar_origin(screen, RectF{100, 300, 400, 500}, false, bar) == PointF{182, 300 - 12 - 48});
  // Region filling the screen: inside its bottom edge. Clamped to the screen horizontally.
  CHECK(recording_bar_origin(screen, RectF{0, 0, 100, 800}, false, bar) == PointF{12, 800 - 48 - 20});
}
