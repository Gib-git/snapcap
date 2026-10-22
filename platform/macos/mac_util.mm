#include "mac_util.h"

#include <cmath>

namespace snapcap::mac {
namespace {

CGBitmapInfo rgba_info(CGImageAlphaInfo alpha) {
  return static_cast<CGBitmapInfo>(kCGBitmapByteOrderDefault) | static_cast<CGBitmapInfo>(alpha);
}

}  // namespace

NSString* ns(std::string_view s) {
  return [[NSString alloc] initWithBytes:s.data() length:s.size() encoding:NSUTF8StringEncoding] ?: @"";
}

std::string str(NSString* s) {
  if (s == nil) return {};
  const char* utf8 = s.UTF8String;
  return utf8 != nullptr ? std::string(utf8) : std::string();
}

NSURL* file_url(const std::filesystem::path& p) {
  return [NSURL fileURLWithPath:ns(p.string())];
}

double primary_height() {
  NSScreen* primary = NSScreen.screens.firstObject;
  return primary != nil ? primary.frame.size.height : 0.0;
}

RectF to_top_left(NSRect r) {
  return {r.origin.x, primary_height() - (r.origin.y + r.size.height), r.size.width, r.size.height};
}

NSRect to_cocoa(const RectF& r) {
  return NSMakeRect(r.x, primary_height() - (r.y + r.height), r.width, r.height);
}

PointF to_top_left(NSPoint p) {
  return {p.x, primary_height() - p.y};
}

uint32_t display_id(NSScreen* screen) {
  NSNumber* n = screen.deviceDescription[@"NSScreenNumber"];
  return n != nil ? n.unsignedIntValue : 0;
}

NSScreen* screen_for_display(uint32_t id) {
  for (NSScreen* s in NSScreen.screens) {
    if (display_id(s) == id) return s;
  }
  return nil;
}

std::vector<MonitorInfo> monitors() {
  std::vector<MonitorInfo> out;
  const uint32_t main_id = CGMainDisplayID();
  for (NSScreen* s in NSScreen.screens) {
    MonitorInfo m;
    m.index = out.size();
    m.id = display_id(s);
    m.name = str(s.localizedName);
    RectF r = to_top_left(s.frame);
    m.x = static_cast<int32_t>(std::lround(r.x));
    m.y = static_cast<int32_t>(std::lround(r.y));
    m.width = static_cast<uint32_t>(std::lround(r.width));
    m.height = static_cast<uint32_t>(std::lround(r.height));
    m.scale = s.backingScaleFactor;
    m.primary = m.id == main_id;
    out.push_back(std::move(m));
  }
  return out;
}

std::optional<PointF> cursor_position() {
  return to_top_left(NSEvent.mouseLocation);
}

RectF work_area(std::optional<uint32_t> id) {
  NSScreen* s = id ? screen_for_display(*id) : nil;
  if (s == nil) s = NSScreen.screens.firstObject;
  if (s == nil) return {0, 0, 1280, 800};
  return to_top_left(s.visibleFrame);
}

CGImageRef create_cgimage(const RgbaImage& img) {
  if (img.empty()) return nullptr;
  CFDataRef data = CFDataCreate(nullptr, img.pixels.data(), static_cast<CFIndex>(img.pixels.size()));
  CGDataProviderRef provider = CGDataProviderCreateWithCFData(data);
  CGColorSpaceRef cs = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
  CGImageRef image = CGImageCreate(img.width, img.height, 8, 32, img.stride(), cs,
                                   rgba_info(kCGImageAlphaLast), provider, nullptr, true,
                                   kCGRenderingIntentDefault);
  CGColorSpaceRelease(cs);
  CGDataProviderRelease(provider);
  CFRelease(data);
  return image;
}

RgbaImage from_cgimage(CGImageRef image) {
  if (image == nullptr) return {};
  RgbaImage out(static_cast<uint32_t>(CGImageGetWidth(image)), static_cast<uint32_t>(CGImageGetHeight(image)));
  if (out.empty()) return {};
  CGColorSpaceRef cs = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
  // Captures are opaque, so premultiplied and straight alpha are the same here.
  CGContextRef ctx = CGBitmapContextCreate(out.pixels.data(), out.width, out.height, 8, out.stride(), cs,
                                           rgba_info(kCGImageAlphaPremultipliedLast));
  CGColorSpaceRelease(cs);
  if (ctx == nullptr) return {};
  CGContextSetBlendMode(ctx, kCGBlendModeCopy);
  CGContextDrawImage(ctx, CGRectMake(0, 0, out.width, out.height), image);
  CGContextRelease(ctx);
  return out;
}

NSImage* nsimage(const RgbaImage& img, double points_per_pixel) {
  CGImageRef cg = create_cgimage(img);
  if (cg == nullptr) return nil;
  NSImage* image = [[NSImage alloc] initWithCGImage:cg
                                               size:NSMakeSize(img.width * points_per_pixel, img.height * points_per_pixel)];
  CGImageRelease(cg);
  return image;
}

void on_main(std::function<void()> fn) {
  dispatch_async(dispatch_get_main_queue(), ^{
    fn();
  });
}

}  // namespace snapcap::mac
