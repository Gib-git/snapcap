// The main window: capture actions, quick recording options, recent captures, settings.
#pragma once

#import <AppKit/AppKit.h>

#include <filesystem>
#include <functional>
#include <memory>
#include <vector>

#include "snapcap/app_controller.h"

struct MainWindowServices {
  std::function<void(const std::filesystem::path&)> open;
  std::function<void(const std::filesystem::path&)> reveal;
  std::function<std::vector<std::string>()> microphones;
};

@interface SNCMainWindow : NSWindow <NSWindowDelegate>
- (instancetype)initWithController:(std::weak_ptr<snapcap::AppController>)controller
                          services:(MainWindowServices)services;
// Rebuilds the visible page from the controller's state.
- (void)reload;
- (void)setRecent:(const std::vector<std::filesystem::path>&)files;
@end
