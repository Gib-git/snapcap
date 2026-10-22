// SnapCap's brand colours (the same values the Rust version used).
#pragma once

#import <AppKit/AppKit.h>

namespace snapcap::mac::theme {

inline NSColor* accent() { return [NSColor colorWithSRGBRed:132 / 255.0 green:118 / 255.0 blue:1.0 alpha:1.0]; }
inline NSColor* coral() { return [NSColor colorWithSRGBRed:1.0 green:90 / 255.0 blue:95 / 255.0 alpha:1.0]; }
inline NSColor* amber() { return [NSColor colorWithSRGBRed:245 / 255.0 green:176 / 255.0 blue:65 / 255.0 alpha:1.0]; }
inline NSColor* pill_background() { return [NSColor colorWithSRGBRed:20 / 255.0 green:21 / 255.0 blue:27 / 255.0 alpha:0.88]; }

}  // namespace snapcap::mac::theme
