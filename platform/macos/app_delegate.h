#pragma once

#import <AppKit/AppKit.h>

#include <optional>

#include "snapcap/actions.h"

@interface SNCAppDelegate : NSObject <NSApplicationDelegate>
- (instancetype)initWithStartupAction:(std::optional<snapcap::Action>)action background:(BOOL)background;
@end
