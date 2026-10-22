// Floating capture notice ("toast") with a thumbnail, shown in the bottom-right corner of
// the display the capture came from, above the Dock. Like the macOS screenshot thumbnail,
// it can be dragged onto any app, folder, browser page or chat to drop the file there.
#pragma once

#import <AppKit/AppKit.h>

#include <filesystem>
#include <functional>

#include "snapcap/host.h"

struct NoticeCallbacks {
  std::function<void(const std::filesystem::path&)> open;
  std::function<void(const std::filesystem::path&)> reveal;
};

@interface SNCNoticePanel : NSPanel
- (instancetype)initWithCallbacks:(NoticeCallbacks)callbacks protectFromCapture:(BOOL)protect;
- (void)showNotice:(const snapcap::Notice&)notice;
- (void)dismiss;
@end

namespace snapcap::mac {
inline constexpr double kNoticeWidth = 340;
inline constexpr double kNoticeHeight = 84;
}
