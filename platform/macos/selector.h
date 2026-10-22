// Region selection overlay: a borderless window per display showing that display's frozen
// capture. The selection logic lives in snapcap::SelectionModel; this only draws and
// forwards input.
#pragma once

#import <AppKit/AppKit.h>

#include <functional>
#include <memory>
#include <optional>
#include <vector>

#include "snapcap/host.h"

@interface SNCSelector : NSObject
- (instancetype)initWithPurpose:(snapcap::SelectionPurpose)purpose
                          shots:(std::shared_ptr<const std::vector<snapcap::Shot>>)shots
                          focus:(size_t)focus
                           done:(std::function<void(std::optional<snapcap::Selection>)>)done;
- (void)show;
- (void)close;
@end
