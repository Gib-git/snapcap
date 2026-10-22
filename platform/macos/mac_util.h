// Shared macOS helpers: coordinate conversion, displays, images, strings.
// SnapCap's core uses top-left-origin desktop points; AppKit uses bottom-left.
#pragma once

#import <AppKit/AppKit.h>

#include <filesystem>
#include <functional>
#include <optional>
#include <string>
#include <vector>

#include "snapcap/geometry.h"
#include "snapcap/image.h"

namespace snapcap::mac {

// ---- strings and paths
NSString* ns(std::string_view s);
std::string str(NSString* s);
NSURL* file_url(const std::filesystem::path& p);

// ---- coordinates
// Height of the primary display, which anchors AppKit's flipped coordinate space.
double primary_height();
RectF to_top_left(NSRect r);
NSRect to_cocoa(const RectF& r);
PointF to_top_left(NSPoint p);

// ---- displays
uint32_t display_id(NSScreen* screen);
NSScreen* screen_for_display(uint32_t display_id);
std::vector<MonitorInfo> monitors();
std::optional<PointF> cursor_position();
// The display minus the menu bar and Dock, in top-left desktop points.
RectF work_area(std::optional<uint32_t> display_id);

// ---- images
CGImageRef create_cgimage(const RgbaImage& img);  // caller releases
RgbaImage from_cgimage(CGImageRef image);
NSImage* nsimage(const RgbaImage& img, double points_per_pixel = 1.0);

// ---- main thread
// Runs `fn` on the main queue (from any thread).
void on_main(std::function<void()> fn);

}  // namespace snapcap::mac
