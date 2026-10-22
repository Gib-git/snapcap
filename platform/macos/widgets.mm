#import "widgets.h"

#include <algorithm>
#include <cmath>

@implementation SNCPillButton {
  BOOL _pressed;
  BOOL _hovered;
}

+ (instancetype)buttonWithTitle:(NSString*)title
                         symbol:(NSString*)symbol
                           fill:(NSColor*)fill
                         target:(id)target
                         action:(SEL)action {
  SNCPillButton* b = [[SNCPillButton alloc] initWithFrame:NSZeroRect];
  b.title = title ?: @"";
  b.symbol = symbol;
  b.fill = fill;
  b.height = 32;
  b.target = target;
  b.action = action;
  b.toolTip = title.length > 0 ? nil : symbol;
  [b sizeToFit];
  [b addTrackingArea:[[NSTrackingArea alloc] initWithRect:NSZeroRect
                                                  options:NSTrackingMouseEnteredAndExited | NSTrackingActiveAlways |
                                                          NSTrackingInVisibleRect
                                                    owner:b
                                                 userInfo:nil]];
  return b;
}

- (NSDictionary*)attributes {
  return @{
    NSFontAttributeName : [NSFont systemFontOfSize:13 weight:NSFontWeightSemibold],
    NSForegroundColorAttributeName : NSColor.whiteColor,
  };
}

- (NSImage*)icon {
  if (self.symbol == nil) return nil;
  NSImage* img = [NSImage imageWithSystemSymbolName:self.symbol accessibilityDescription:self.title];
  NSImageSymbolConfiguration* cfg = [NSImageSymbolConfiguration configurationWithPointSize:14
                                                                                    weight:NSFontWeightSemibold];
  cfg = [cfg configurationByApplyingConfiguration:[NSImageSymbolConfiguration
                                                      configurationWithHierarchicalColor:NSColor.whiteColor]];
  return [img imageWithSymbolConfiguration:cfg];
}

- (NSSize)intrinsicContentSize {
  NSSize text = self.title.length > 0 ? [self.title sizeWithAttributes:[self attributes]] : NSZeroSize;
  NSImage* icon = [self icon];
  double w = 28 + text.width + (icon != nil ? icon.size.width : 0) + (icon != nil && text.width > 0 ? 6 : 0);
  if (self.title.length == 0) w = std::max(w, 40.0);
  return NSMakeSize(std::ceil(w), self.height);
}

- (void)sizeToFit {
  [self setFrameSize:[self intrinsicContentSize]];
}

- (BOOL)isFlipped {
  return YES;
}

- (BOOL)acceptsFirstMouse:(NSEvent*)event {
  return YES;
}

- (void)resetCursorRects {
  [self addCursorRect:self.bounds cursor:NSCursor.pointingHandCursor];
}

- (void)mouseEntered:(NSEvent*)event {
  _hovered = YES;
  self.needsDisplay = YES;
}

- (void)mouseExited:(NSEvent*)event {
  _hovered = NO;
  self.needsDisplay = YES;
}

- (BOOL)inside:(NSEvent*)event {
  return NSPointInRect([self convertPoint:event.locationInWindow fromView:nil], self.bounds);
}

- (void)mouseDown:(NSEvent*)event {
  _pressed = YES;
  self.needsDisplay = YES;
}

- (void)mouseDragged:(NSEvent*)event {
  BOOL inside = [self inside:event];
  if (inside != _pressed) {
    _pressed = inside;
    self.needsDisplay = YES;
  }
}

- (void)mouseUp:(NSEvent*)event {
  // Fires only when released over the button, like a standard button.
  BOOL fire = _pressed && [self inside:event];
  _pressed = NO;
  self.needsDisplay = YES;
  if (fire) [self sendAction:self.action to:self.target];
}

- (void)drawRect:(NSRect)dirty {
  NSRect b = self.bounds;
  NSColor* fill = self.fill ?: [NSColor colorWithWhite:1 alpha:0.12];
  if (_pressed) {
    fill = [fill blendedColorWithFraction:0.25 ofColor:NSColor.blackColor];
  } else if (_hovered) {
    fill = [fill blendedColorWithFraction:0.12 ofColor:NSColor.whiteColor];
  }
  [fill setFill];
  [[NSBezierPath bezierPathWithRoundedRect:b xRadius:NSHeight(b) / 2 yRadius:NSHeight(b) / 2] fill];

  NSImage* icon = [self icon];
  NSSize text = self.title.length > 0 ? [self.title sizeWithAttributes:[self attributes]] : NSZeroSize;
  double total = text.width + (icon != nil ? icon.size.width : 0) + (icon != nil && text.width > 0 ? 6 : 0);
  double x = NSMidX(b) - total / 2;
  if (icon != nil) {
    NSSize is = icon.size;
    [icon drawInRect:NSMakeRect(x, NSMidY(b) - is.height / 2, is.width, is.height)
            fromRect:NSZeroRect
           operation:NSCompositingOperationSourceOver
            fraction:1
      respectFlipped:YES
               hints:nil];
    x += is.width + 6;
  }
  if (text.width > 0) [self.title drawAtPoint:NSMakePoint(x, NSMidY(b) - text.height / 2) withAttributes:[self attributes]];
}

@end
