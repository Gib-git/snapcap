#include "snapcap/geometry.h"

#include <algorithm>
#include <cmath>

#include "snapcap/os.h"

namespace snapcap {

double MonitorInfo::px_per_unit() const {
  return kDesktopInPoints ? std::max(scale, 1.0) : 1.0;
}

PxRect PxRect::even_within(uint32_t img_w, uint32_t img_h) const {
  uint32_t nx = std::min(x, img_w >= 2 ? img_w - 2 : 0u);
  uint32_t ny = std::min(y, img_h >= 2 ? img_h - 2 : 0u);
  uint32_t nw = std::max(std::min(w, img_w - nx), 2u) & ~1u;
  uint32_t nh = std::max(std::min(h, img_h - ny), 2u) & ~1u;
  return {nx, ny, nw, nh};
}

PxRect CaptureTarget::pixel_rect() const {
  return region.value_or(PxRect{0, 0, image_width, image_height}).even_within(image_width, image_height);
}

RectF CaptureTarget::desktop_rect() const {
  PxRect r = pixel_rect();
  double s = monitor.px_per_unit();
  return {monitor.x + r.x / s, monitor.y + r.y / s, r.w / s, r.h / s};
}

RgbaImage stitch(std::span<const Shot> shots) {
  if (shots.empty()) return {};
  if (shots.size() == 1) return shots[0].image;
  double s = 1.0;
  for (const Shot& sh : shots) s = std::max(s, double(sh.image.width) / std::max(sh.monitor.width, 1u));
  int32_t min_x = shots[0].monitor.x, min_y = shots[0].monitor.y;
  for (const Shot& sh : shots) {
    min_x = std::min(min_x, sh.monitor.x);
    min_y = std::min(min_y, sh.monitor.y);
  }
  auto place = [&](const Shot& sh) {
    return std::pair{static_cast<int64_t>(std::lround((sh.monitor.x - min_x) * s)),
                     static_cast<int64_t>(std::lround((sh.monitor.y - min_y) * s))};
  };
  int64_t w = 0, h = 0;
  for (const Shot& sh : shots) {
    auto [x, y] = place(sh);
    w = std::max(w, x + sh.image.width);
    h = std::max(h, y + sh.image.height);
  }
  RgbaImage canvas(static_cast<uint32_t>(w), static_cast<uint32_t>(h), {0, 0, 0, 255});
  for (const Shot& sh : shots) {
    auto [x, y] = place(sh);
    blit(canvas, sh.image, x, y);
  }
  return canvas;
}

size_t monitor_under(std::span<const MonitorInfo> monitors, std::optional<PointF> cursor) {
  if (cursor) {
    for (size_t i = 0; i < monitors.size(); ++i) {
      if (monitors[i].contains(*cursor)) return i;
    }
  }
  for (size_t i = 0; i < monitors.size(); ++i) {
    if (monitors[i].primary) return i;
  }
  return 0;
}

std::pair<uint32_t, uint32_t> fit_even(uint32_t w, uint32_t h, std::optional<uint32_t> max_h,
                                       std::optional<uint32_t> max_w) {
  double scale = 1.0;
  if (max_h && h > *max_h) scale = std::min(scale, double(*max_h) / h);
  if (max_w && w > *max_w) scale = std::min(scale, double(*max_w) / w);
  auto even = [](double v) { return std::max(static_cast<uint32_t>(std::lround(v)), 2u) & ~1u; };
  return {even(w * scale), even(h * scale)};
}

std::pair<uint32_t, uint32_t> output_size(VideoFormat format, ResolutionCap resolution, uint32_t gif_max_width,
                                          uint32_t w, uint32_t h) {
  if (format == VideoFormat::Gif) return fit_even(w, h, std::nullopt, gif_max_width);
  auto [limit_w, limit_h] = w >= h ? std::pair{3840u, 2160u} : std::pair{2160u, 3840u};
  uint32_t cap_h = std::min(max_height(resolution).value_or(limit_h), limit_h);
  return fit_even(w, h, cap_h, limit_w);
}

PointF notice_origin(const RectF& work_area, SizeF size, double margin) {
  double x = work_area.right() - size.width - margin;
  double y = work_area.bottom() - size.height - margin;
  // Never spill past the top-left of a tiny work area.
  return {std::max(x, work_area.x), std::max(y, work_area.y)};
}

PointF recording_bar_origin(const RectF& mon, const RectF& r, bool full_screen, SizeF bar) {
  constexpr double kMargin = 12.0;
  if (full_screen) return {mon.x + (mon.width - bar.width) / 2, mon.bottom() - bar.height - 96.0};
  double y = r.bottom() + kMargin;
  if (y + bar.height > mon.bottom() - kMargin) {
    y = r.y - kMargin - bar.height;
    if (y < mon.y + 30.0) y = r.bottom() - bar.height - 20.0;
  }
  double lo = mon.x + kMargin, hi = mon.right() - bar.width - kMargin;
  double x = std::clamp(r.x + r.width / 2 - bar.width / 2, lo, std::max(lo, hi));
  return {x, y};
}

}  // namespace snapcap
