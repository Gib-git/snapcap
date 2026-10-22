// Floating recording controls and the border drawn around a recorded region.
// Both are excluded from captures and never take keyboard focus.
#pragma once

#import <AppKit/AppKit.h>

#include <functional>

#include "snapcap/host.h"

@interface SNCRecordingUI : NSObject
- (instancetype)initWithAction:(std::function<void(snapcap::BarAction)>)action protectFromCapture:(BOOL)protect;
// Shows or updates the bar (and the border, for regions). The bar is placed once per
// target, so the user can drag it out of the way.
- (void)showFor:(const snapcap::CaptureTarget&)target state:(const snapcap::BarState&)state;
- (void)hide;
@end

namespace snapcap::mac {
inline constexpr double kBarWidth = 236;
inline constexpr double kBarHeight = 48;
}
