#import "notice_panel.h"

#import <QuickLookThumbnailing/QuickLookThumbnailing.h>

#include <algorithm>
#include <optional>

#include "mac_util.h"

using namespace snapcap;

namespace {

constexpr NSTimeInterval kTick = 0.25;
// Largest side of the image that follows the pointer while dragging, in points.
constexpr double kDragImageMax = 220;
constexpr double kCardRadius = 14;

}  // namespace

// The card's content. Handles clicks, hover and dragging the file out.
@interface SNCNoticeView : NSView <NSDraggingSource>
@property(nonatomic, weak) SNCNoticePanel* panel;
@property(nonatomic) BOOL hovering;
@property(nonatomic) BOOL dragging;
@property(nonatomic) BOOL revealHovered;
@end

@interface SNCNoticePanel ()
- (const Notice&)notice;
@property(nonatomic, strong) NSImage* thumbnail;
- (void)extendBy:(NSTimeInterval)seconds;
- (void)viewClicked;
- (void)revealClicked;
- (void)dragEndedWithDrop:(BOOL)dropped;
@end

@implementation SNCNoticeView {
  NSTrackingArea* _tracking;
  NSPoint _downAt;
  BOOL _mouseDown;
}

- (BOOL)isFlipped {
  return YES;
}

- (BOOL)acceptsFirstMouse:(NSEvent*)event {
  return YES;
}

- (void)updateTrackingAreas {
  [super updateTrackingAreas];
  if (_tracking) [self removeTrackingArea:_tracking];
  _tracking = [[NSTrackingArea alloc]
      initWithRect:self.bounds
           options:NSTrackingMouseEnteredAndExited | NSTrackingMouseMoved | NSTrackingActiveAlways |
                   NSTrackingInVisibleRect
             owner:self
          userInfo:nil];
  [self addTrackingArea:_tracking];
}

- (NSRect)cardRect {
  return NSInsetRect(self.bounds, 2, 2);
}

- (NSRect)thumbRect {
  NSRect card = [self cardRect];
  return NSMakeRect(NSMinX(card) + 12, NSMinY(card) + 12, 88, NSHeight(card) - 24);
}

- (NSRect)revealRect {
  NSRect card = [self cardRect];
  return NSMakeRect(NSMaxX(card) - 112, NSMaxY(card) - 30, 100, 22);
}

- (void)mouseEntered:(NSEvent*)event {
  self.hovering = YES;
  [self.panel extendBy:2];
  self.needsDisplay = YES;
}

- (void)mouseExited:(NSEvent*)event {
  self.hovering = NO;
  self.revealHovered = NO;
  self.needsDisplay = YES;
}

- (void)mouseMoved:(NSEvent*)event {
  // Keep it on screen while the user is looking at it.
  SNCNoticePanel* panel = self.panel;
  if (panel == nil) return;  // a queued event after the panel is gone
  [panel extendBy:2];
  BOOL over = [panel notice].path && NSPointInRect([self convertPoint:event.locationInWindow fromView:nil],
                                                        [self revealRect]);
  if (over != self.revealHovered) {
    self.revealHovered = over;
    self.needsDisplay = YES;
  }
}

- (void)resetCursorRects {
  [self addCursorRect:self.bounds cursor:NSCursor.pointingHandCursor];
}

- (void)mouseDown:(NSEvent*)event {
  _mouseDown = YES;
  _downAt = event.locationInWindow;
}

- (void)mouseDragged:(NSEvent*)event {
  if (!_mouseDown || self.dragging) return;
  SNCNoticePanel* panel = self.panel;
  if (panel == nil) return;
  const auto& path = [panel notice].path;
  if (!path) return;
  NSPoint p = event.locationInWindow;
  if (std::hypot(p.x - _downAt.x, p.y - _downAt.y) < 3) return;
  std::error_code ec;
  if (!std::filesystem::exists(*path, ec)) return;
  _mouseDown = NO;

  NSURL* url = mac::file_url(*path);
  NSDraggingItem* item = [[NSDraggingItem alloc] initWithPasteboardWriter:url];
  // The capture itself for images; the Finder icon for videos.
  NSImage* image = [[NSImage alloc] initWithContentsOfURL:url];
  NSSize size;
  if (image != nil && image.size.width > 0 && image.size.height > 0) {
    double fit = std::min({kDragImageMax / image.size.width, kDragImageMax / image.size.height, 1.0});
    size = NSMakeSize(image.size.width * fit, image.size.height * fit);
  } else {
    image = [NSWorkspace.sharedWorkspace iconForFile:url.path];
    size = NSMakeSize(96, 96);
  }
  NSPoint at = [self convertPoint:p fromView:nil];
  [item setDraggingFrame:NSMakeRect(at.x - size.width / 2, at.y - size.height / 2, size.width, size.height)
                contents:image];
  self.dragging = YES;
  [self beginDraggingSessionWithItems:@[ item ] event:event source:self];
}

- (void)mouseUp:(NSEvent*)event {
  if (!_mouseDown) return;
  _mouseDown = NO;
  NSPoint p = [self convertPoint:event.locationInWindow fromView:nil];
  SNCNoticePanel* panel = self.panel;
  if (panel == nil) return;
  if ([panel notice].path && self.hovering && NSPointInRect(p, [self revealRect])) {
    [panel revealClicked];
  } else {
    [panel viewClicked];
  }
}

- (NSDragOperation)draggingSession:(NSDraggingSession*)session
    sourceOperationMaskForDraggingContext:(NSDraggingContext)context {
  // Copy only: Finder would otherwise move the file out of the captures folder.
  return NSDragOperationCopy;
}

- (void)draggingSession:(NSDraggingSession*)session
           endedAtPoint:(NSPoint)screenPoint
              operation:(NSDragOperation)operation {
  self.dragging = NO;
  [self.panel dragEndedWithDrop:operation != NSDragOperationNone];
}

- (void)drawRect:(NSRect)dirty {
  SNCNoticePanel* panel = self.panel;
  if (panel == nil) return;
  const Notice& n = [panel notice];
  NSRect card = [self cardRect];
  NSBezierPath* shape = [NSBezierPath bezierPathWithRoundedRect:card xRadius:kCardRadius yRadius:kCardRadius];
  [NSColor.windowBackgroundColor setFill];
  [shape fill];
  [[NSColor.separatorColor colorWithAlphaComponent:0.6] setStroke];
  shape.lineWidth = 1;
  [shape stroke];

  BOOL error = n.kind == NoticeKind::Error;
  NSColor* accent = error ? NSColor.systemRedColor : NSColor.controlAccentColor;
  NSRect thumb = [self thumbRect];
  if (panel.thumbnail != nil) {
    NSBezierPath* clip = [NSBezierPath bezierPathWithRoundedRect:thumb xRadius:6 yRadius:6];
    [NSColor.underPageBackgroundColor setFill];
    [clip fill];
    NSSize ts = panel.thumbnail.size;
    double fit = std::min(NSWidth(thumb) / ts.width, NSHeight(thumb) / ts.height);
    NSRect r = NSMakeRect(NSMidX(thumb) - ts.width * fit / 2, NSMidY(thumb) - ts.height * fit / 2, ts.width * fit,
                          ts.height * fit);
    [NSGraphicsContext saveGraphicsState];
    [clip addClip];
    [panel.thumbnail drawInRect:r fromRect:NSZeroRect operation:NSCompositingOperationSourceOver fraction:1.0
                     respectFlipped:YES
                              hints:@{NSImageHintInterpolation : @(NSImageInterpolationHigh)}];
    [NSGraphicsContext restoreGraphicsState];
  } else {
    [[accent colorWithAlphaComponent:0.14] setFill];
    [[NSBezierPath bezierPathWithRoundedRect:thumb xRadius:8 yRadius:8] fill];
    NSString* symbol = error ? @"exclamationmark.triangle" : (n.is_video ? @"film" : @"photo");
    NSImage* icon = [NSImage imageWithSystemSymbolName:symbol accessibilityDescription:nil];
    NSImageSymbolConfiguration* cfg = [NSImageSymbolConfiguration configurationWithPointSize:24
                                                                                     weight:NSFontWeightRegular];
    cfg = [cfg configurationByApplyingConfiguration:[NSImageSymbolConfiguration configurationWithHierarchicalColor:accent]];
    icon = [icon imageWithSymbolConfiguration:cfg];
    NSSize is = icon.size;
    [icon drawInRect:NSMakeRect(NSMidX(thumb) - is.width / 2, NSMidY(thumb) - is.height / 2, is.width, is.height)
            fromRect:NSZeroRect
           operation:NSCompositingOperationSourceOver
            fraction:1.0
      respectFlipped:YES
               hints:nil];
  }

  double tx = NSMaxX(thumb) + 12;
  double max_w = NSMaxX(card) - tx - 12;
  NSDictionary* title_attrs = @{
    NSFontAttributeName : [NSFont systemFontOfSize:14 weight:NSFontWeightSemibold],
    NSForegroundColorAttributeName : NSColor.labelColor,
  };
  [mac::ns(n.title) drawInRect:NSMakeRect(tx, NSMinY(card) + 13, max_w, 20) withAttributes:title_attrs];

  NSMutableParagraphStyle* para = [[NSMutableParagraphStyle alloc] init];
  para.lineBreakMode = NSLineBreakByTruncatingMiddle;
  NSDictionary* detail_attrs = @{
    NSFontAttributeName : [NSFont systemFontOfSize:11.5],
    NSForegroundColorAttributeName : NSColor.secondaryLabelColor,
    NSParagraphStyleAttributeName : para,
  };
  NSArray<NSString*>* lines = [mac::ns(n.detail) componentsSeparatedByString:@"\n"];
  double y = NSMinY(card) + 35;
  if (lines.count == 1) {
    // A single message (usually an error) wraps over two lines.
    NSMutableParagraphStyle* wrap = [[NSMutableParagraphStyle alloc] init];
    wrap.lineBreakMode = NSLineBreakByWordWrapping;
    NSMutableDictionary* attrs = [detail_attrs mutableCopy];
    attrs[NSParagraphStyleAttributeName] = wrap;
    [lines[0] drawWithRect:NSMakeRect(tx, y, max_w, 30)
                   options:NSStringDrawingUsesLineFragmentOrigin | NSStringDrawingTruncatesLastVisibleLine
                attributes:attrs
                   context:nil];
  } else {
    // One line each: file name (middle-truncated), "Copied to clipboard", warnings...
    for (NSString* line in lines) {
      if (y + 15 > NSMaxY(card) - 6) break;
      [line drawInRect:NSMakeRect(tx, y, max_w, 15) withAttributes:detail_attrs];
      y += 15;
    }
  }

  if (n.path && self.hovering) {
    NSRect reveal = [self revealRect];
    NSMutableParagraphStyle* right = [[NSMutableParagraphStyle alloc] init];
    right.alignment = NSTextAlignmentRight;
    NSDictionary* attrs = @{
      NSFontAttributeName : [NSFont systemFontOfSize:11.5 weight:NSFontWeightMedium],
      NSForegroundColorAttributeName : self.revealHovered ? NSColor.controlAccentColor : NSColor.secondaryLabelColor,
      NSParagraphStyleAttributeName : right,
    };
    // A soft backing so the link never collides with long detail text.
    NSSize ts = [@"Show in folder" sizeWithAttributes:attrs];
    NSRect pill = NSMakeRect(NSMaxX(reveal) - ts.width - 8, NSMinY(reveal) + 1, ts.width + 10, NSHeight(reveal) - 2);
    [NSColor.windowBackgroundColor setFill];
    [[NSBezierPath bezierPathWithRoundedRect:pill xRadius:5 yRadius:5] fill];
    [@"Show in folder" drawInRect:NSMakeRect(NSMinX(reveal), NSMinY(reveal) + 3, NSWidth(reveal), 16)
                   withAttributes:attrs];
  }
}

@end

@implementation SNCNoticePanel {
  NoticeCallbacks _callbacks;
  Notice _notice;
  SNCNoticeView* _view;
  NSTimer* _timer;
  NSDate* _until;
  uint64_t _generation;
}

- (instancetype)initWithCallbacks:(NoticeCallbacks)callbacks protectFromCapture:(BOOL)protect {
  NSRect frame = NSMakeRect(0, 0, mac::kNoticeWidth, mac::kNoticeHeight);
  if ((self = [super initWithContentRect:frame
                               styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
                                 backing:NSBackingStoreBuffered
                                   defer:NO])) {
    _callbacks = std::move(callbacks);
    self.opaque = NO;
    self.backgroundColor = NSColor.clearColor;
    self.hasShadow = YES;
    // Note: setting floatingPanel would reset the level to NSFloatingWindowLevel, below the Dock.
    self.level = NSStatusWindowLevel;
    self.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces |
                              NSWindowCollectionBehaviorFullScreenAuxiliary | NSWindowCollectionBehaviorStationary |
                              NSWindowCollectionBehaviorIgnoresCycle;
    self.hidesOnDeactivate = NO;
    self.releasedWhenClosed = NO;
    self.becomesKeyOnlyIfNeeded = YES;
    self.movableByWindowBackground = NO;
    if (protect) self.sharingType = NSWindowSharingNone;
    self.title = @"SnapCap Notice";
    _view = [[SNCNoticeView alloc] initWithFrame:frame];
    _view.panel = self;
    self.contentView = _view;
  }
  return self;
}

- (BOOL)canBecomeKeyWindow {
  return NO;
}

- (BOOL)canBecomeMainWindow {
  return NO;
}

- (const Notice&)notice {
  return _notice;
}

- (void)showNotice:(const Notice&)notice {
  _notice = notice;
  ++_generation;
  self.thumbnail = notice.thumbnail ? mac::nsimage(*notice.thumbnail) : nil;
  if (!self.thumbnail && notice.path) [self loadThumbnailFor:*notice.path generation:_generation];

  // Bottom-right corner of the capture's display, above the Dock.
  RectF work = mac::work_area(notice.monitor_id);
  PointF origin = notice_origin(work, {mac::kNoticeWidth, mac::kNoticeHeight});
  [self setFrame:mac::to_cocoa({origin.x, origin.y, mac::kNoticeWidth, mac::kNoticeHeight}) display:NO];

  _until = [NSDate dateWithTimeIntervalSinceNow:std::chrono::duration<double>(notice.lifetime).count()];
  _view.hovering = NO;
  _view.revealHovered = NO;
  _view.needsDisplay = YES;
  [self orderFrontRegardless];
  [self invalidateCursorRectsForView:_view];
  if (_timer == nil) {
    _timer = [NSTimer timerWithTimeInterval:kTick target:self selector:@selector(tick) userInfo:nil repeats:YES];
    // Common modes, so the notice still expires while a menu is open.
    [NSRunLoop.mainRunLoop addTimer:_timer forMode:NSRunLoopCommonModes];
  }
}

- (void)loadThumbnailFor:(const std::filesystem::path&)path generation:(uint64_t)generation {
  QLThumbnailGenerationRequest* req =
      [[QLThumbnailGenerationRequest alloc] initWithFileAtURL:mac::file_url(path)
                                                         size:NSMakeSize(88, 60)
                                                        scale:NSScreen.mainScreen.backingScaleFactor ?: 2
                                          representationTypes:QLThumbnailGenerationRequestRepresentationTypeThumbnail];
  __weak SNCNoticePanel* weak = self;
  [QLThumbnailGenerator.sharedGenerator
      generateBestRepresentationForRequest:req
                         completionHandler:^(QLThumbnailRepresentation* rep, NSError*) {
                           if (rep == nil) return;
                           NSImage* image = rep.NSImage;
                           dispatch_async(dispatch_get_main_queue(), ^{
                             SNCNoticePanel* panel = weak;
                             // Ignore a thumbnail for a notice that has since been replaced.
                             if (panel == nil || panel->_generation != generation) return;
                             panel.thumbnail = image;
                             panel->_view.needsDisplay = YES;
                           });
                         }];
}

- (void)extendBy:(NSTimeInterval)seconds {
  NSDate* at_least = [NSDate dateWithTimeIntervalSinceNow:seconds];
  if (_until == nil || [_until compare:at_least] == NSOrderedAscending) _until = at_least;
}

- (void)tick {
  if (!self.visible) return;
  // Closing the window mid-drag would end the drag.
  if (_view.dragging) return [self extendBy:1];
  if (_until != nil && _until.timeIntervalSinceNow <= 0) [self dismiss];
}

- (void)dismiss {
  [_timer invalidate];
  _timer = nil;
  ++_generation;
  _view.dragging = NO;
  [self orderOut:nil];
}

- (void)viewClicked {
  if (_notice.path) _callbacks.open(*_notice.path);
  [self dismiss];
}

- (void)revealClicked {
  if (_notice.path) _callbacks.reveal(*_notice.path);
  [self dismiss];
}

- (void)dragEndedWithDrop:(BOOL)dropped {
  // When the drop lands the notice goes away, like the macOS screenshot thumbnail.
  if (dropped) return [self dismiss];
  _until = [NSDate dateWithTimeIntervalSinceNow:3];
}

@end
