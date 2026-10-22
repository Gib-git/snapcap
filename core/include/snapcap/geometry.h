// Displays, capture rectangles and window placement. All desktop coordinates use a
// top-left origin with y growing downwards; platforms convert at their boundary.
#pragma once

#include <cstdint>
#include <optional>
#include <span>
#include <string>
#include <vector>

#include "snapcap/image.h"
#include "snapcap/settings.h"

namespace snapcap {

struct PointF {
  double x = 0;
  double y = 0;
  bool operator==(const PointF&) const = default;
};

struct SizeF {
  double width = 0;
  double height = 0;
  bool operator==(const SizeF&) const = default;
};

struct RectF {
  double x = 0;
  double y = 0;
  double width = 0;
  double height = 0;
  bool operator==(const RectF&) const = default;
  double right() const { return x + width; }
  double bottom() const { return y + height; }
  bool contains(PointF p) const { return p.x >= x && p.y >= y && p.x < right() && p.y < bottom(); }
};

struct MonitorInfo {
  size_t index = 0;
  // Platform display id (CGDirectDisplayID on macOS).
  uint32_t id = 0;
  std::string name;
  // Position and size in desktop coordinates: points on macOS, physical pixels elsewhere.
  int32_t x = 0;
  int32_t y = 0;
  uint32_t width = 0;
  uint32_t height = 0;
  double scale = 1.0;
  bool primary = false;

  bool operator==(const MonitorInfo&) const = default;

  // Captured image pixels per desktop coordinate unit.
  double px_per_unit() const;
  RectF bounds() const { return {double(x), double(y), double(width), double(height)}; }
  bool contains(PointF p) const { return bounds().contains(p); }
};

// A rectangle in captured-image pixels of one monitor.
struct PxRect {
  uint32_t x = 0;
  uint32_t y = 0;
  uint32_t w = 0;
  uint32_t h = 0;
  bool operator==(const PxRect&) const = default;

  // Shrinks to even width/height (required by H.264 4:2:0) and clamps to the image.
  PxRect even_within(uint32_t img_w, uint32_t img_h) const;
};

// What a recording captures.
struct CaptureTarget {
  MonitorInfo monitor;
  // Size of the monitor's captured image in pixels.
  uint32_t image_width = 0;
  uint32_t image_height = 0;
  // Unset = the whole monitor.
  std::optional<PxRect> region;

  PxRect pixel_rect() const;
  // The captured rectangle in desktop coordinates (for placing the border and bar windows).
  RectF desktop_rect() const;
};

struct Shot {
  MonitorInfo monitor;
  RgbaImage image;
};

// Composes all monitors into one image following their desktop layout, at the highest
// pixel density so nothing is downscaled. Gaps are black.
RgbaImage stitch(std::span<const Shot> shots);

// Index (into `monitors`) of the display containing `cursor`, falling back to the primary one.
size_t monitor_under(std::span<const MonitorInfo> monitors, std::optional<PointF> cursor);

// Output size for a capture of `w x h` under optional caps. Always even.
std::pair<uint32_t, uint32_t> fit_even(uint32_t w, uint32_t h, std::optional<uint32_t> max_height,
                                       std::optional<uint32_t> max_width);

// Encoded size for a capture of `w x h` pixels: honours the resolution cap, the GIF width,
// and the H.264 limit of 3840x2160 (2160x3840 portrait).
std::pair<uint32_t, uint32_t> output_size(VideoFormat format, ResolutionCap resolution, uint32_t gif_max_width,
                                          uint32_t w, uint32_t h);

// Top-left position for a window of `size` in the bottom-right corner of `work_area`
// (the display minus the Dock, menu bar or taskbar), `margin` away from both edges.
PointF notice_origin(const RectF& work_area, SizeF size, double margin = 16.0);

// Where the recording bar goes: centred below the region, or above it when there is no
// room, or inside its bottom edge as a last resort. Full-screen recordings put it near
// the bottom of the display.
PointF recording_bar_origin(const RectF& monitor, const RectF& region, bool full_screen, SizeF bar);

}  // namespace snapcap
