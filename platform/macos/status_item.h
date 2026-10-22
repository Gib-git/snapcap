// The menu bar icon and its menu (same items as the Rust version's tray).
#pragma once

#import <AppKit/AppKit.h>

#include <functional>
#include <string>

#include "snapcap/settings.h"

// `command` is an action's CLI name, "open-folder" or "quit".
using StatusCommandHandler = std::function<void(const std::string& command)>;

@interface SNCStatusItem : NSObject <NSMenuDelegate>
- (instancetype)initWithHandler:(StatusCommandHandler)handler;
- (void)setRecording:(BOOL)recording;
- (void)setShortcuts:(const snapcap::ShortcutMap&)shortcuts;
@end
