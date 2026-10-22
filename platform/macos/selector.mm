#import "selector.h"

#include <Carbon/Carbon.h>

#include <cmath>
#include <format>

#include "mac_util.h"
#include "snapcap/selection.h"
#import "theme.h"
#import "widgets.h"

using namespace snapcap;
using Kind = SelectionModel::DragKind;

namespace {

constexpr double kHandle = 9;
constexpr double kLoupeSize = 116;
constexpr double kLoupeSpan = 15;  // image pixels shown across the magnifier
constexpr double kToolbarHeight = 46;

// A CGImage that reads the shot's pixels in place (no 60 MB copy per 5K display).
CGImageRef shared_cgimage(std::shared_ptr<const std::vector<Shot>> shots, size_t i) {
  const RgbaImage& img = (*shots)[i].image;
  auto* keep = new std::shared_ptr<const std::vector<Shot>>(std::move(shots));
  CGDataProviderRef provider = CGDataProviderCreateWithData(
      keep, img.pixels.data(), img.pixels.size(),
      [](void* info, const void*, size_t) { delete static_cast<std::shared_ptr<const std::vector<Shot>>*>(info); });
  CGColorSpaceRef cs = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
  CGImageRef image = CGImageCreate(img.width, img.height, 8, 32, img.stride(), cs,
                                   static_cast<CGBitmapInfo>(kCGImageAlphaNoneSkipLast), provider, nullptr, false,
                                   kCGRenderingIntentDefault);
  CGColorSpaceRelease(cs);
  CGDataProviderRelease(provider);
  return image;
}

void draw_pill(NSString* text, NSPoint anchor, double size, int align /* -1 left, 0 centre */, BOOL above) {
  NSDictionary* attrs = @{
    NSFontAttributeName : [NSFont systemFontOfSize:size weight:NSFontWeightMedium],
    NSForegroundColorAttributeName : NSColor.whiteColor,
  };
  NSSize ts = [text sizeWithAttributes:attrs];
  NSSize box = NSMakeSize(ts.width + 20, ts.height + 10);
  double x = align < 0 ? anchor.x : anchor.x - box.width / 2;
  double y = above ? anchor.y - box.height : anchor.y - box.height / 2;
  NSRect r = NSMakeRect(x, y, box.width, box.height);
  [snapcap::mac::theme::pill_background() setFill];
  [[NSBezierPath bezierPathWithRoundedRect:r xRadius:box.height / 2 yRadius:box.height / 2] fill];
  [text drawAtPoint:NSMakePoint(r.origin.x + 10, r.origin.y + 5) withAttributes:attrs];
}

NSCursor* resize_cursor(const Edges& e) {
  bool horizontal = e.left || e.right, vertical = e.top || e.bottom;
  if (horizontal && vertical) {
    if (@available(macOS 15.0, *)) {
      NSCursorFrameResizePosition pos = e.top ? (e.left ? NSCursorFrameResizePositionTopLeft
                                                        : NSCursorFrameResizePositionTopRight)
                                              : (e.left ? NSCursorFrameResizePositionBottomLeft
                                                        : NSCursorFrameResizePositionBottomRight);
      return [NSCursor frameResizeCursorFromPosition:pos inDirections:NSCursorFrameResizeDirectionsAll];
    }
    return NSCursor.crosshairCursor;
  }
  return horizontal ? NSCursor.resizeLeftRightCursor : NSCursor.resizeUpDownCursor;
}

}  // namespace

@class SNCOverlayView;

@interface SNCSelector ()
@property(nonatomic, readonly) SelectionModel* model;
- (void)handle:(const SelectionModel::Outcome&)outcome;
- (void)redrawAll;
@end

@interface SNCOverlayWindow : NSWindow
@end

@implementation SNCOverlayWindow
- (BOOL)canBecomeKeyWindow {
  return YES;
}
- (BOOL)canBecomeMainWindow {
  return YES;
}
@end

@interface SNCOverlayView : NSView
- (instancetype)initWithFrame:(NSRect)frame
                     selector:(SNCSelector*)selector
                        index:(size_t)index
                        image:(CGImageRef)image
                       pixels:(NSSize)pixels;
- (void)refreshToolbar;
- (void)detach;
@end

@implementation SNCOverlayView {
  __weak SNCSelector* _selector;
  size_t _index;
  NSImage* _image;
  CGImageRef _cgimage;
  NSSize _pixels;
  std::optional<NSPoint> _hover;  // view points
  NSView* _toolbar;
}

- (instancetype)initWithFrame:(NSRect)frame
                     selector:(SNCSelector*)selector
                        index:(size_t)index
                        image:(CGImageRef)image
                       pixels:(NSSize)pixels {
  if ((self = [super initWithFrame:frame])) {
    _selector = selector;
    _index = index;
    _cgimage = CGImageRetain(image);
    _image = [[NSImage alloc] initWithCGImage:image size:frame.size];
    _pixels = pixels;
    [self addTrackingArea:[[NSTrackingArea alloc]
                              initWithRect:NSZeroRect
                                   options:NSTrackingMouseMoved | NSTrackingMouseEnteredAndExited |
                                           NSTrackingActiveAlways | NSTrackingInVisibleRect | NSTrackingCursorUpdate
                                     owner:self
                                  userInfo:nil]];
  }
  return self;
}

- (void)dealloc {
  CGImageRelease(_cgimage);
}

- (BOOL)isFlipped {
  return YES;
}

- (BOOL)acceptsFirstResponder {
  return YES;
}

- (BOOL)acceptsFirstMouse:(NSEvent*)event {
  return YES;
}

// ---- coordinates: view points <-> image pixels
- (double)scale {
  return _pixels.width / std::max(1.0, self.bounds.size.width);
}

- (PointF)toPx:(NSPoint)p {
  double s = [self scale];
  return {std::clamp(p.x * s, 0.0, _pixels.width), std::clamp(p.y * s, 0.0, _pixels.height)};
}

- (NSRect)toPt:(const RectF&)r {
  double s = [self scale];
  return NSMakeRect(r.x / s, r.y / s, r.width / s, r.height / s);
}

// The model, or null once the selector is gone: a view can outlive it and still receive a
// queued mouse, key or cursor event.
- (SelectionModel*)model {
  SNCSelector* selector = _selector;
  return selector != nil ? selector.model : nullptr;
}

- (NSPoint)local:(NSEvent*)event {
  return [self convertPoint:event.locationInWindow fromView:nil];
}

- (std::optional<NSRect>)selectionHere {
  SelectionModel* m = [self model];
  if (m == nullptr) return std::nullopt;
  auto sel = m->selection();
  if (!sel || sel->first != _index) return std::nullopt;
  return [self toPt:sel->second];
}

// ---- input
- (void)mouseDown:(NSEvent*)event {
  SelectionModel* m = [self model];
  if (m == nullptr) return;
  if (event.clickCount == 2) {
    [_selector handle:m->double_click(_index)];
    return;
  }
  m->press(_index, [self toPx:[self local:event]]);
  [_selector redrawAll];
}

- (void)mouseDragged:(NSEvent*)event {
  NSPoint p = [self local:event];
  _hover = p;
  SelectionModel* m = [self model];
  if (m == nullptr) return;
  m->drag(_index, [self toPx:p]);
  [self updateCursor];
  [_selector redrawAll];
}

- (void)mouseUp:(NSEvent*)event {
  SelectionModel* m = [self model];
  if (m == nullptr) return;
  auto outcome = m->release(_index, [self toPx:[self local:event]]);
  [_selector redrawAll];
  [_selector handle:outcome];
}

- (void)mouseMoved:(NSEvent*)event {
  _hover = [self local:event];
  [self updateCursor];
  self.needsDisplay = YES;
}

- (void)mouseEntered:(NSEvent*)event {
  [self.window makeKeyWindow];
  _hover = [self local:event];
  self.needsDisplay = YES;
}

- (void)mouseExited:(NSEvent*)event {
  _hover.reset();
  self.needsDisplay = YES;
}

- (void)cursorUpdate:(NSEvent*)event {
  [self updateCursor];
}

- (void)updateCursor {
  SelectionModel* m = [self model];
  if (m == nullptr) return;
  if (m->dragging() == Kind::Move) return [NSCursor.closedHandCursor set];
  if (m->dragging() != Kind::None || !_hover) return [NSCursor.crosshairCursor set];
  Edges e;
  switch (m->hit(_index, [self toPx:*_hover], &e)) {
    case Kind::Resize: return [resize_cursor(e) set];
    case Kind::Move: return [NSCursor.openHandCursor set];
    default: return [NSCursor.crosshairCursor set];
  }
}

- (void)keyDown:(NSEvent*)event {
  SelectionModel* m = [self model];
  if (m == nullptr) return;
  switch (event.keyCode) {
    case kVK_Escape: return [_selector handle:m->key_escape()];
    case kVK_Return:
    case kVK_ANSI_KeypadEnter: return [_selector handle:m->key_enter(_index)];
    default: return;
  }
}

// ---- record toolbar
// Stops receiving pointer and cursor events once the overlay closes.
- (void)detach {
  for (NSTrackingArea* t in [self.trackingAreas copy]) [self removeTrackingArea:t];
  _hover.reset();
}

- (void)refreshToolbar {
  SelectionModel* m = [self model];
  if (m == nullptr) return;
  auto sel = [self selectionHere];
  BOOL show = m->purpose() == SelectionPurpose::Record && sel && m->dragging() == Kind::None;
  if (!show) {
    _toolbar.hidden = YES;
    return;
  }
  if (_toolbar == nil) [self buildToolbar];
  NSRect r = *sel;
  NSRect b = self.bounds;
  double width = _toolbar.frame.size.width;
  double x = std::clamp(NSMidX(r) - width / 2, NSMinX(b) + 12, NSMaxX(b) - width - 12);
  double y = NSMaxY(r) + 12;
  if (y + kToolbarHeight > NSMaxY(b) - 12) {
    y = NSMinY(r) - 12 - kToolbarHeight;
    if (y < NSMinY(b) + 12) y = NSMaxY(r) - kToolbarHeight - 16;
  }
  [_toolbar setFrameOrigin:NSMakePoint(x, y)];
  _toolbar.hidden = NO;
}

- (void)buildToolbar {
  SNCPillButton* record = [SNCPillButton buttonWithTitle:@"Start recording"
                                                  symbol:@"record.circle"
                                                    fill:mac::theme::coral()
                                                  target:self
                                                  action:@selector(startRecording:)];
  SNCPillButton* full = [SNCPillButton buttonWithTitle:@""
                                                symbol:@"arrow.up.left.and.arrow.down.right"
                                                  fill:nil
                                                target:self
                                                action:@selector(wholeScreen:)];
  full.toolTip = @"Whole screen";
  SNCPillButton* cancel = [SNCPillButton buttonWithTitle:@"Cancel" symbol:nil fill:nil target:self action:@selector(cancel:)];

  NSView* bar = [[NSView alloc] init];
  bar.wantsLayer = YES;
  bar.layer.backgroundColor = [NSColor colorWithSRGBRed:24 / 255.0 green:25 / 255.0 blue:31 / 255.0 alpha:0.96].CGColor;
  bar.layer.borderColor = [NSColor colorWithWhite:1 alpha:0.1].CGColor;
  bar.layer.borderWidth = 1;
  bar.layer.cornerRadius = kToolbarHeight / 2;
  double x = 8;
  for (SNCPillButton* b in @[ record, full, cancel ]) {
    [b setFrameOrigin:NSMakePoint(x, (kToolbarHeight - b.frame.size.height) / 2)];
    [bar addSubview:b];
    x += b.frame.size.width + 6;
  }
  bar.frame = NSMakeRect(0, 0, x + 2, kToolbarHeight);
  _toolbar = bar;
  [self addSubview:bar];
}

- (void)startRecording:(id)sender {
  if (SelectionModel* m = [self model]) [_selector handle:m->confirm_current()];
}

- (void)wholeScreen:(id)sender {
  SelectionModel* m = [self model];
  if (m == nullptr) return;
  m->select_full(_index);
  [_selector redrawAll];
}

- (void)cancel:(id)sender {
  if (SelectionModel* m = [self model]) [_selector handle:m->key_escape()];
}

// ---- drawing
- (void)drawRect:(NSRect)dirty {
  NSRect b = self.bounds;
  [_image drawInRect:b fromRect:NSZeroRect operation:NSCompositingOperationCopy fraction:1.0 respectFlipped:YES hints:nil];

  SelectionModel* m = [self model];
  if (m == nullptr) return;
  auto sel = [self selectionHere];
  NSColor* dim = [NSColor colorWithWhite:0 alpha:0.45];
  if (sel) {
    NSRect r = *sel;
    [dim setFill];
    NSRectFillUsingOperation(NSMakeRect(0, 0, NSWidth(b), NSMinY(r)), NSCompositingOperationSourceOver);
    NSRectFillUsingOperation(NSMakeRect(0, NSMaxY(r), NSWidth(b), NSHeight(b) - NSMaxY(r)),
                             NSCompositingOperationSourceOver);
    NSRectFillUsingOperation(NSMakeRect(0, NSMinY(r), NSMinX(r), NSHeight(r)), NSCompositingOperationSourceOver);
    NSRectFillUsingOperation(NSMakeRect(NSMaxX(r), NSMinY(r), NSWidth(b) - NSMaxX(r), NSHeight(r)),
                             NSCompositingOperationSourceOver);
    NSBezierPath* border = [NSBezierPath bezierPathWithRect:NSInsetRect(r, -0.75, -0.75)];
    border.lineWidth = 1.5;
    [mac::theme::accent() setStroke];
    [border stroke];
    if (m->purpose() == SelectionPurpose::Record && m->dragging() == Kind::None) {
      for (NSPoint c : {NSMakePoint(NSMinX(r), NSMinY(r)), NSMakePoint(NSMidX(r), NSMinY(r)),
                        NSMakePoint(NSMaxX(r), NSMinY(r)), NSMakePoint(NSMinX(r), NSMidY(r)),
                        NSMakePoint(NSMaxX(r), NSMidY(r)), NSMakePoint(NSMinX(r), NSMaxY(r)),
                        NSMakePoint(NSMidX(r), NSMaxY(r)), NSMakePoint(NSMaxX(r), NSMaxY(r))}) {
        NSBezierPath* h = [NSBezierPath
            bezierPathWithRoundedRect:NSMakeRect(c.x - kHandle / 2, c.y - kHandle / 2, kHandle, kHandle)
                              xRadius:2
                              yRadius:2];
        [NSColor.whiteColor setFill];
        [h fill];
        [mac::theme::accent() setStroke];
        h.lineWidth = 1;
        [h stroke];
      }
    }
    RectF px = m->selection()->second;
    NSString* size = [NSString stringWithFormat:@"%ld × %ld", std::lround(px.width), std::lround(px.height)];
    BOOL room_above = NSMinY(r) > 34;
    draw_pill(size, room_above ? NSMakePoint(NSMinX(r), NSMinY(r) - 8) : NSMakePoint(NSMinX(r) + 8, NSMinY(r) + 30),
              12.5, -1, YES);
  } else {
    [[NSColor colorWithWhite:0 alpha:0.34] setFill];
    NSRectFillUsingOperation(b, NSCompositingOperationSourceOver);
  }

  // Crosshair guides and the magnifier while choosing.
  BOOL choosing = m->dragging() == Kind::Create || (m->dragging() == Kind::None && !sel);
  if (_hover && choosing) {
    NSPoint hp = *_hover;
    [[NSColor colorWithWhite:1 alpha:0.28] setStroke];
    NSBezierPath* guides = [NSBezierPath bezierPath];
    [guides moveToPoint:NSMakePoint(0, hp.y + 0.5)];
    [guides lineToPoint:NSMakePoint(NSWidth(b), hp.y + 0.5)];
    [guides moveToPoint:NSMakePoint(hp.x + 0.5, 0)];
    [guides lineToPoint:NSMakePoint(hp.x + 0.5, NSHeight(b))];
    guides.lineWidth = 1;
    [guides stroke];
    [self drawLoupeAt:hp];
  }

  if (m->dragging() == Kind::None && (!sel || m->purpose() == SelectionPurpose::Screenshot)) {
    NSString* hint = m->purpose() == SelectionPurpose::Screenshot
                         ? @"Drag to capture an area   ·   Click to capture this screen   ·   Esc to cancel"
                         : @"Drag to choose the recording area   ·   Click for the whole screen   ·   Esc to cancel";
    draw_pill(hint, NSMakePoint(NSMidX(b), 44), 13.5, 0, NO);
  }
}

- (void)drawLoupeAt:(NSPoint)at {
  NSRect b = self.bounds;
  PointF px = [self toPx:at];
  NSPoint pos = NSMakePoint(at.x + 22, at.y + 22);
  if (pos.x + kLoupeSize > NSMaxX(b) - 8) pos.x = at.x - 22 - kLoupeSize;
  if (pos.y + kLoupeSize + 28 > NSMaxY(b) - 8) pos.y = at.y - 22 - kLoupeSize - 28;
  NSRect r = NSMakeRect(pos.x, pos.y, kLoupeSize, kLoupeSize);

  [[NSColor colorWithWhite:0 alpha:0.8] setFill];
  [[NSBezierPath bezierPathWithRoundedRect:NSInsetRect(r, -3, -3) xRadius:8 yRadius:8] fill];
  // Source rectangle in image pixels (CGImage space is top-down, matching this flipped view).
  double cx = std::floor(px.x) + 0.5, cy = std::floor(px.y) + 0.5;
  CGRect src = CGRectMake(cx - kLoupeSpan / 2, cy - kLoupeSpan / 2, kLoupeSpan, kLoupeSpan);
  CGImageRef crop = CGImageCreateWithImageInRect(_cgimage, CGRectIntegral(src));
  if (crop != nullptr) {
    NSGraphicsContext* ctx = NSGraphicsContext.currentContext;
    [ctx saveGraphicsState];
    [[NSBezierPath bezierPathWithRoundedRect:r xRadius:4 yRadius:4] addClip];
    ctx.imageInterpolation = NSImageInterpolationNone;
    NSImage* zoom = [[NSImage alloc] initWithCGImage:crop size:NSZeroSize];
    [zoom drawInRect:r fromRect:NSZeroRect operation:NSCompositingOperationCopy fraction:1 respectFlipped:YES hints:nil];
    [ctx restoreGraphicsState];
    CGImageRelease(crop);
  }
  double cell = kLoupeSize / kLoupeSpan;
  [NSColor.whiteColor setStroke];
  NSFrameRectWithWidth(NSMakeRect(NSMidX(r) - cell / 2, NSMidY(r) - cell / 2, cell, cell), 1);
  NSBezierPath* frame = [NSBezierPath bezierPathWithRoundedRect:r xRadius:4 yRadius:4];
  frame.lineWidth = 1.5;
  [[NSColor colorWithWhite:1 alpha:0.8] setStroke];
  [frame stroke];
  draw_pill([NSString stringWithFormat:@"%ld, %ld", static_cast<long>(px.x), static_cast<long>(px.y)],
            NSMakePoint(NSMidX(r), NSMaxY(r) + 16), 11.5, 0, NO);
}

@end

@implementation SNCSelector {
  std::unique_ptr<SelectionModel> _model;
  std::shared_ptr<const std::vector<Shot>> _shots;
  std::function<void(std::optional<Selection>)> _done;
  NSMutableArray<NSWindow*>* _windows;
  NSMutableArray<SNCOverlayView*>* _views;
  size_t _focus;
  BOOL _finished;
}

- (instancetype)initWithPurpose:(SelectionPurpose)purpose
                          shots:(std::shared_ptr<const std::vector<Shot>>)shots
                          focus:(size_t)focus
                           done:(std::function<void(std::optional<Selection>)>)done {
  if ((self = [super init])) {
    std::vector<SelectionModel::Display> displays;
    for (const Shot& s : *shots) {
      displays.push_back({s.image.width, s.image.height, double(s.image.width) / std::max(1u, s.monitor.width)});
    }
    _model = std::make_unique<SelectionModel>(purpose, std::move(displays));
    _shots = std::move(shots);
    _done = std::move(done);
    _focus = focus;
    _windows = [NSMutableArray array];
    _views = [NSMutableArray array];
  }
  return self;
}

- (SelectionModel*)model {
  return _model.get();
}

- (void)show {
  for (size_t i = 0; i < _shots->size(); ++i) {
    const Shot& s = (*_shots)[i];
    NSScreen* screen = mac::screen_for_display(s.monitor.id);
    NSRect frame = screen != nil ? screen.frame : mac::to_cocoa(s.monitor.bounds());
    SNCOverlayWindow* w = [[SNCOverlayWindow alloc] initWithContentRect:frame
                                                              styleMask:NSWindowStyleMaskBorderless
                                                                backing:NSBackingStoreBuffered
                                                                  defer:NO];
    w.releasedWhenClosed = NO;
    // Above the menu bar and the Dock, on whichever Space is active.
    w.level = NSScreenSaverWindowLevel;
    w.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary |
                           NSWindowCollectionBehaviorStationary | NSWindowCollectionBehaviorIgnoresCycle;
    w.opaque = YES;
    w.hasShadow = NO;
    w.animationBehavior = NSWindowAnimationBehaviorNone;
    w.acceptsMouseMovedEvents = YES;
    w.title = [NSString stringWithFormat:@"SnapCap Selection %zu", i + 1];
    CGImageRef image = shared_cgimage(_shots, i);
    SNCOverlayView* v = [[SNCOverlayView alloc] initWithFrame:NSMakeRect(0, 0, frame.size.width, frame.size.height)
                                                     selector:self
                                                        index:i
                                                        image:image
                                                       pixels:NSMakeSize(s.image.width, s.image.height)];
    CGImageRelease(image);
    w.contentView = v;
    [w setFrame:frame display:NO];
    [_windows addObject:w];
    [_views addObject:v];
  }
  [NSApp activateIgnoringOtherApps:YES];
  for (NSUInteger i = 0; i < _windows.count; ++i) {
    if (i != _focus) [_windows[i] orderFrontRegardless];
  }
  if (_focus < _windows.count) {
    [_windows[_focus] makeKeyAndOrderFront:nil];
    [_windows[_focus] makeFirstResponder:_views[_focus]];
  }
  [NSCursor.crosshairCursor set];
}

- (void)redrawAll {
  for (SNCOverlayView* v in _views) {
    [v refreshToolbar];
    v.needsDisplay = YES;
  }
}

- (void)handle:(const SelectionModel::Outcome&)outcome {
  if (_finished || (!outcome.cancel && !outcome.confirm)) return;
  _finished = YES;
  auto done = std::move(_done);
  // The controller hides the overlay (close) in response.
  done(outcome.cancel ? std::nullopt : outcome.confirm);
}

- (void)close {
  // The windows are released with the selector, which the host keeps alive until the
  // current event (possibly one of our own mouse handlers) has finished.
  _finished = YES;
  for (SNCOverlayView* v in _views) [v detach];
  for (NSWindow* w in _windows) [w orderOut:nil];
  [NSCursor.arrowCursor set];
}

@end
