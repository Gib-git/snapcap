// Small custom controls shared by the floating windows.
#pragma once

#import <AppKit/AppKit.h>

// A rounded pill button with its own fill, an optional SF Symbol and white text. It draws
// the same in every window state (overlays and floating panels are never "key").
@interface SNCPillButton : NSControl
+ (instancetype)buttonWithTitle:(NSString*)title
                         symbol:(NSString*)symbol
                           fill:(NSColor*)fill
                         target:(id)target
                         action:(SEL)action;
@property(nonatomic, strong) NSString* title;
@property(nonatomic, strong) NSString* symbol;
@property(nonatomic, strong) NSColor* fill;
@property(nonatomic) CGFloat height;
@end
