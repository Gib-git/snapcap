#import "main_window.h"

#import <QuickLookThumbnailing/QuickLookThumbnailing.h>
#import <objc/runtime.h>

#include <array>
#include <format>
#include <map>

#include "keymap.h"
#include "mac_util.h"
#include "snapcap/hotkeys.h"
#include "snapcap/icon.h"
#include "snapcap/output.h"
#include "snapcap/paths.h"
#include "snapcap/recent.h"
#import "theme.h"

using namespace snapcap;

// ============================================================================ small helpers

// Calls a block when a control fires. Kept alive by the control it is attached to.
@interface SNCBlockTarget : NSObject
@property(nonatomic, copy) void (^block)(id sender);
- (void)fire:(id)sender;
@end

@implementation SNCBlockTarget
- (void)fire:(id)sender {
  if (self.block) self.block(sender);
}
@end

namespace {

const void* kTargetKey = &kTargetKey;

void on_action(NSControl* control, void (^block)(id sender)) {
  SNCBlockTarget* t = [[SNCBlockTarget alloc] init];
  t.block = block;
  objc_setAssociatedObject(control, kTargetKey, t, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  control.target = t;
  control.action = @selector(fire:);
}

NSTextField* label(NSString* text, CGFloat size = 13, NSFontWeight weight = NSFontWeightRegular,
                   NSColor* color = NSColor.labelColor) {
  NSTextField* t = [NSTextField labelWithString:text ?: @""];
  t.font = [NSFont systemFontOfSize:size weight:weight];
  t.textColor = color;
  t.lineBreakMode = NSLineBreakByTruncatingMiddle;
  [t setContentCompressionResistancePriority:NSLayoutPriorityDefaultLow
                              forOrientation:NSLayoutConstraintOrientationHorizontal];
  return t;
}

NSTextField* wrapping_label(NSString* text, CGFloat size, NSColor* color) {
  NSTextField* t = [NSTextField wrappingLabelWithString:text];
  t.font = [NSFont systemFontOfSize:size];
  t.textColor = color;
  return t;
}

NSTextField* section(NSString* text) {
  NSTextField* t = label(text.uppercaseString, 11, NSFontWeightSemibold, NSColor.secondaryLabelColor);
  return t;
}

NSStackView* vstack(NSArray<NSView*>* views, CGFloat spacing) {
  NSStackView* s = [NSStackView stackViewWithViews:views];
  s.orientation = NSUserInterfaceLayoutOrientationVertical;
  s.alignment = NSLayoutAttributeLeading;
  s.spacing = spacing;
  return s;
}

NSStackView* hstack(NSArray<NSView*>* views, CGFloat spacing) {
  NSStackView* s = [NSStackView stackViewWithViews:views];
  s.orientation = NSUserInterfaceLayoutOrientationHorizontal;
  s.alignment = NSLayoutAttributeCenterY;
  s.spacing = spacing;
  return s;
}

NSView* spacer() {
  NSView* v = [[NSView alloc] init];
  [v setContentHuggingPriority:1 forOrientation:NSLayoutConstraintOrientationHorizontal];
  return v;
}

// A label on the left, controls on the right.
NSStackView* row(NSView* left, NSArray<NSView*>* right) {
  NSMutableArray* views = [NSMutableArray arrayWithObjects:left, spacer(), nil];
  [views addObjectsFromArray:right];
  NSStackView* s = hstack(views, 8);
  return s;
}

NSImage* symbol(NSString* name, CGFloat size, NSColor* color = nil) {
  NSImage* img = [NSImage imageWithSystemSymbolName:name accessibilityDescription:nil];
  NSImageSymbolConfiguration* cfg = [NSImageSymbolConfiguration configurationWithPointSize:size weight:NSFontWeightRegular];
  if (color != nil) {
    cfg = [cfg configurationByApplyingConfiguration:[NSImageSymbolConfiguration configurationWithHierarchicalColor:color]];
  }
  return [img imageWithSymbolConfiguration:cfg];
}

NSButton* icon_button(NSString* sym, NSString* tip, void (^block)(id)) {
  NSButton* b = [NSButton buttonWithImage:symbol(sym, 15) target:nil action:nil];
  b.bezelStyle = NSBezelStyleAccessoryBarAction;
  b.bordered = NO;
  b.toolTip = tip;
  on_action(b, block);
  return b;
}

NSButton* link_button(NSString* title, void (^block)(id)) {
  NSButton* b = [NSButton buttonWithTitle:title target:nil action:nil];
  b.bordered = NO;
  b.attributedTitle = [[NSAttributedString alloc]
      initWithString:title
          attributes:@{
            NSForegroundColorAttributeName : NSColor.linkColor,
            NSFontAttributeName : [NSFont systemFontOfSize:12],
          }];
  on_action(b, block);
  return b;
}

NSSwitch* toggle(bool on, void (^block)(bool)) {
  NSSwitch* s = [[NSSwitch alloc] init];
  s.controlSize = NSControlSizeSmall;
  s.state = on ? NSControlStateValueOn : NSControlStateValueOff;
  on_action(s, ^(NSSwitch* sender) {
    block(sender.state == NSControlStateValueOn);
  });
  return s;
}

template <class T, size_t N>
NSSegmentedControl* segmented(const std::array<std::pair<T, const char*>, N>& options, T value, void (^block)(T)) {
  NSMutableArray* labels = [NSMutableArray array];
  NSInteger selected = -1;
  for (size_t i = 0; i < N; ++i) {
    [labels addObject:@(options[i].second)];
    if (options[i].first == value) selected = static_cast<NSInteger>(i);
  }
  NSSegmentedControl* c = [NSSegmentedControl segmentedControlWithLabels:labels
                                                            trackingMode:NSSegmentSwitchTrackingSelectOne
                                                                  target:nil
                                                                  action:nil];
  c.selectedSegment = selected;
  c.controlSize = NSControlSizeSmall;
  auto opts = options;
  on_action(c, ^(NSSegmentedControl* sender) {
    auto i = sender.selectedSegment;
    if (i >= 0 && static_cast<size_t>(i) < N) block(opts[static_cast<size_t>(i)].first);
  });
  return c;
}

std::string shorten_home(const std::filesystem::path& p) {
  std::string s = p.string();
  if (auto home = paths::home_dir()) {
    std::string h = home->string();
    if (s.starts_with(h)) return "~" + s.substr(h.size());
  }
  return s;
}

}  // namespace

// ============================================================================ grouped box

// A rounded card holding rows, like a System Settings group.
@interface SNCGroupView : NSView
+ (instancetype)groupWithRows:(NSArray<NSView*>*)rows;
@end

@implementation SNCGroupView
+ (instancetype)groupWithRows:(NSArray<NSView*>*)rows {
  SNCGroupView* g = [[SNCGroupView alloc] init];
  NSStackView* stack = vstack(rows, 10);
  stack.alignment = NSLayoutAttributeLeading;
  stack.translatesAutoresizingMaskIntoConstraints = NO;
  [g addSubview:stack];
  [NSLayoutConstraint activateConstraints:@[
    [stack.leadingAnchor constraintEqualToAnchor:g.leadingAnchor constant:14],
    [stack.trailingAnchor constraintEqualToAnchor:g.trailingAnchor constant:-14],
    [stack.topAnchor constraintEqualToAnchor:g.topAnchor constant:12],
    [stack.bottomAnchor constraintEqualToAnchor:g.bottomAnchor constant:-12],
  ]];
  for (NSView* r in rows) [r.widthAnchor constraintEqualToAnchor:stack.widthAnchor].active = YES;
  return g;
}
- (BOOL)isFlipped {
  return YES;
}
- (void)drawRect:(NSRect)dirty {
  NSBezierPath* p = [NSBezierPath bezierPathWithRoundedRect:NSInsetRect(self.bounds, 0.5, 0.5) xRadius:12 yRadius:12];
  [NSColor.controlBackgroundColor setFill];
  [p fill];
  [[NSColor.separatorColor colorWithAlphaComponent:0.7] setStroke];
  [p stroke];
}
@end

// ============================================================================ action card

@interface SNCActionCard : NSControl
@property(nonatomic, strong) NSString* symbolName;
@property(nonatomic, strong) NSString* title;
@property(nonatomic, strong) NSString* shortcut;
@property(nonatomic, strong) NSColor* accent;
@end

@implementation SNCActionCard {
  BOOL _hover;
  BOOL _pressed;
}

- (instancetype)initWithFrame:(NSRect)frame {
  if ((self = [super initWithFrame:frame])) {
    [self addTrackingArea:[[NSTrackingArea alloc] initWithRect:NSZeroRect
                                                       options:NSTrackingMouseEnteredAndExited |
                                                               NSTrackingActiveInKeyWindow | NSTrackingInVisibleRect
                                                         owner:self
                                                      userInfo:nil]];
  }
  return self;
}
- (BOOL)isFlipped {
  return YES;
}
- (NSSize)intrinsicContentSize {
  return NSMakeSize(NSViewNoIntrinsicMetric, 112);
}
- (void)mouseEntered:(NSEvent*)e {
  _hover = YES;
  self.needsDisplay = YES;
}
- (void)mouseExited:(NSEvent*)e {
  _hover = NO;
  self.needsDisplay = YES;
}
- (void)mouseDown:(NSEvent*)e {
  if (!self.enabled) return;
  _pressed = YES;
  self.needsDisplay = YES;
}
- (void)mouseUp:(NSEvent*)e {
  BOOL inside = NSPointInRect([self convertPoint:e.locationInWindow fromView:nil], self.bounds);
  BOOL fire = _pressed && inside && self.enabled;
  _pressed = NO;
  self.needsDisplay = YES;
  if (fire) [self sendAction:self.action to:self.target];
}
- (void)resetCursorRects {
  if (self.enabled) [self addCursorRect:self.bounds cursor:NSCursor.pointingHandCursor];
}
- (void)drawRect:(NSRect)dirty {
  NSRect b = NSInsetRect(self.bounds, 1, 1);
  NSBezierPath* card = [NSBezierPath bezierPathWithRoundedRect:b xRadius:14 yRadius:14];
  [NSColor.controlBackgroundColor setFill];
  [card fill];
  if (_pressed) {
    [[self.accent colorWithAlphaComponent:0.08] setFill];
    [card fill];
  }
  [(_hover && self.enabled ? self.accent : [NSColor.separatorColor colorWithAlphaComponent:0.7]) setStroke];
  card.lineWidth = _hover && self.enabled ? 1.5 : 1;
  [card stroke];
  CGFloat alpha = self.enabled ? 1.0 : 0.4;

  NSRect tile = NSMakeRect(NSMinX(b) + 14, NSMinY(b) + 14, 36, 36);
  [[self.accent colorWithAlphaComponent:0.16 * alpha] setFill];
  [[NSBezierPath bezierPathWithRoundedRect:tile xRadius:10 yRadius:10] fill];
  NSImage* icon = symbol(self.symbolName, 17, [self.accent colorWithAlphaComponent:alpha]);
  NSSize is = icon.size;
  [icon drawInRect:NSMakeRect(NSMidX(tile) - is.width / 2, NSMidY(tile) - is.height / 2, is.width, is.height)
          fromRect:NSZeroRect
         operation:NSCompositingOperationSourceOver
          fraction:1
    respectFlipped:YES
             hints:nil];
  [self.title drawAtPoint:NSMakePoint(NSMinX(b) + 14, NSMaxY(b) - 46)
           withAttributes:@{
             NSFontAttributeName : [NSFont systemFontOfSize:14 weight:NSFontWeightSemibold],
             NSForegroundColorAttributeName : [NSColor.labelColor colorWithAlphaComponent:alpha],
           }];
  if (self.shortcut.length > 0) {
    [self.shortcut drawAtPoint:NSMakePoint(NSMinX(b) + 14, NSMaxY(b) - 26)
                withAttributes:@{
                  NSFontAttributeName : [NSFont systemFontOfSize:12],
                  NSForegroundColorAttributeName : NSColor.secondaryLabelColor,
                }];
  }
}
@end

// ============================================================================ recent capture tile

@interface SNCRecentTile : NSView <NSDraggingSource>
@property(nonatomic) std::filesystem::path path;
@property(nonatomic, strong) NSImage* thumbnail;
@property(nonatomic, copy) void (^onOpen)(void);
@property(nonatomic, copy) NSMenu* (^makeMenu)(void);
@end

@implementation SNCRecentTile {
  BOOL _hover;
  BOOL _down;
  NSPoint _downAt;
}
- (instancetype)initWithFrame:(NSRect)frame {
  if ((self = [super initWithFrame:frame])) {
    [self addTrackingArea:[[NSTrackingArea alloc] initWithRect:NSZeroRect
                                                       options:NSTrackingMouseEnteredAndExited |
                                                               NSTrackingActiveInKeyWindow | NSTrackingInVisibleRect
                                                         owner:self
                                                      userInfo:nil]];
  }
  return self;
}
- (BOOL)isFlipped {
  return YES;
}
- (void)mouseEntered:(NSEvent*)e {
  _hover = YES;
  self.needsDisplay = YES;
}
- (void)mouseExited:(NSEvent*)e {
  _hover = NO;
  self.needsDisplay = YES;
}
- (void)resetCursorRects {
  [self addCursorRect:self.bounds cursor:NSCursor.pointingHandCursor];
}
- (void)mouseDown:(NSEvent*)e {
  _down = YES;
  _downAt = e.locationInWindow;
}
- (void)mouseDragged:(NSEvent*)e {
  if (!_down) return;
  NSPoint p = e.locationInWindow;
  if (std::hypot(p.x - _downAt.x, p.y - _downAt.y) < 3) return;
  _down = NO;
  std::error_code ec;
  if (!std::filesystem::exists(self.path, ec)) return;
  // Drop it onto another app or folder, as from Finder.
  NSURL* url = mac::file_url(self.path);
  NSDraggingItem* item = [[NSDraggingItem alloc] initWithPasteboardWriter:url];
  NSImage* image = self.thumbnail ?: [NSWorkspace.sharedWorkspace iconForFile:url.path];
  [item setDraggingFrame:self.bounds contents:image];
  [self beginDraggingSessionWithItems:@[ item ] event:e source:self];
}
- (void)mouseUp:(NSEvent*)e {
  if (_down && e.clickCount == 1 && self.onOpen) self.onOpen();
  _down = NO;
}
- (NSMenu*)menuForEvent:(NSEvent*)e {
  return self.makeMenu ? self.makeMenu() : nil;
}
- (NSDragOperation)draggingSession:(NSDraggingSession*)s sourceOperationMaskForDraggingContext:(NSDraggingContext)c {
  return NSDragOperationCopy;
}
- (void)drawRect:(NSRect)dirty {
  NSRect b = NSInsetRect(self.bounds, 0.5, 0.5);
  NSBezierPath* shape = [NSBezierPath bezierPathWithRoundedRect:b xRadius:10 yRadius:10];
  [NSColor.controlBackgroundColor setFill];
  [shape fill];
  auto kind = media_kind(self.path).value_or(MediaKind::Png);
  if (self.thumbnail != nil) {
    // Cover-fit crop.
    NSSize ts = self.thumbnail.size;
    double scale = std::max(NSWidth(b) / ts.width, NSHeight(b) / ts.height);
    NSRect r = NSMakeRect(NSMidX(b) - ts.width * scale / 2, NSMidY(b) - ts.height * scale / 2, ts.width * scale,
                          ts.height * scale);
    [NSGraphicsContext saveGraphicsState];
    [shape addClip];
    [self.thumbnail drawInRect:r fromRect:NSZeroRect operation:NSCompositingOperationSourceOver fraction:1 respectFlipped:YES hints:nil];
    [NSGraphicsContext restoreGraphicsState];
  } else {
    NSImage* icon = symbol(kind == MediaKind::Mp4 ? @"film" : @"photo", 22, NSColor.tertiaryLabelColor);
    NSSize is = icon.size;
    [icon drawInRect:NSMakeRect(NSMidX(b) - is.width / 2, NSMidY(b) - is.height / 2, is.width, is.height)
            fromRect:NSZeroRect operation:NSCompositingOperationSourceOver fraction:1 respectFlipped:YES hints:nil];
  }
  NSRect badge = NSMakeRect(NSMinX(b) + 6, NSMinY(b) + 6, 34, 17);
  [[(kind == MediaKind::Png ? mac::theme::accent() : mac::theme::coral()) colorWithAlphaComponent:0.92] setFill];
  [[NSBezierPath bezierPathWithRoundedRect:badge xRadius:5 yRadius:5] fill];
  NSString* text = mac::ns(label(kind));
  NSDictionary* attrs = @{
    NSFontAttributeName : [NSFont systemFontOfSize:10 weight:NSFontWeightSemibold],
    NSForegroundColorAttributeName : NSColor.whiteColor,
  };
  NSSize tsz = [text sizeWithAttributes:attrs];
  [text drawAtPoint:NSMakePoint(NSMidX(badge) - tsz.width / 2, NSMidY(badge) - tsz.height / 2) withAttributes:attrs];
  [(_hover ? mac::theme::accent() : [NSColor.separatorColor colorWithAlphaComponent:0.7]) setStroke];
  shape.lineWidth = _hover ? 2 : 1;
  [shape stroke];
}
@end

// Lays out tiles in a fixed number of columns, with a 0.66 aspect ratio.
@interface SNCTileGrid : NSView
@property(nonatomic) NSUInteger columns;
@end

@implementation SNCTileGrid
- (BOOL)isFlipped {
  return YES;
}
- (CGFloat)tileWidth {
  return std::floor((NSWidth(self.bounds) - 10 * (self.columns - 1)) / self.columns);
}
- (NSSize)intrinsicContentSize {
  NSUInteger rows = (self.subviews.count + self.columns - 1) / self.columns;
  CGFloat h = std::round([self tileWidth] * 0.66);
  return NSMakeSize(NSViewNoIntrinsicMetric, rows == 0 ? 0 : rows * h + (rows - 1) * 10);
}
- (void)layout {
  [super layout];
  CGFloat w = [self tileWidth], h = std::round(w * 0.66);
  NSUInteger i = 0;
  for (NSView* v in self.subviews) {
    NSUInteger c = i % self.columns, r = i / self.columns;
    v.frame = NSMakeRect(c * (w + 10), r * (h + 10), w, h);
    ++i;
  }
  [self invalidateIntrinsicContentSize];
}
- (void)setFrameSize:(NSSize)size {
  BOOL changed = size.width != self.frame.size.width;
  [super setFrameSize:size];
  if (changed) [self invalidateIntrinsicContentSize];
}
@end

// ============================================================================ shortcut recorder

@interface SNCShortcutField : NSView
@property(nonatomic) Action action;
@property(nonatomic) BOOL capturing;
@property(nonatomic, strong) NSString* text;
@property(nonatomic, copy) void (^onClick)(void);
@property(nonatomic, copy) void (^onKey)(NSEvent* event);
@property(nonatomic, copy) void (^onResign)(void);
@end

@implementation SNCShortcutField
- (BOOL)isFlipped {
  return YES;
}
- (BOOL)acceptsFirstResponder {
  return YES;
}
- (NSSize)intrinsicContentSize {
  NSString* t = [self displayText];
  CGFloat w = [t sizeWithAttributes:@{NSFontAttributeName : [NSFont systemFontOfSize:13]}].width + 24;
  return NSMakeSize(std::max<CGFloat>(110, w), 26);
}
- (NSString*)displayText {
  return self.capturing ? @"Press keys…  (Esc to cancel)" : self.text;
}
- (void)mouseDown:(NSEvent*)e {
  if (self.onClick) self.onClick();
}
- (void)keyDown:(NSEvent*)e {
  if (self.capturing && self.onKey) {
    self.onKey(e);
  } else {
    [super keyDown:e];
  }
}
// Shortcuts with Cmd arrive here before keyDown.
- (BOOL)performKeyEquivalent:(NSEvent*)e {
  if (self.capturing && self.window.firstResponder == self && self.onKey) {
    self.onKey(e);
    return YES;
  }
  return [super performKeyEquivalent:e];
}
- (BOOL)resignFirstResponder {
  if (self.capturing && self.onResign) {
    auto block = self.onResign;
    dispatch_async(dispatch_get_main_queue(), ^{
      block();
    });
  }
  return YES;
}
- (void)resetCursorRects {
  [self addCursorRect:self.bounds cursor:NSCursor.pointingHandCursor];
}
- (void)drawRect:(NSRect)dirty {
  NSRect b = NSInsetRect(self.bounds, 0.5, 0.5);
  NSBezierPath* p = [NSBezierPath bezierPathWithRoundedRect:b xRadius:7 yRadius:7];
  [(self.capturing ? [mac::theme::accent() colorWithAlphaComponent:0.15] : NSColor.controlColor) setFill];
  [p fill];
  [(self.capturing ? mac::theme::accent() : NSColor.separatorColor) setStroke];
  p.lineWidth = self.capturing ? 1.5 : 1;
  [p stroke];
  NSString* t = [self displayText];
  NSDictionary* attrs = @{
    NSFontAttributeName : [NSFont systemFontOfSize:13],
    NSForegroundColorAttributeName : [self.text isEqualToString:@"Not set"] && !self.capturing
                                         ? NSColor.secondaryLabelColor
                                         : NSColor.labelColor,
  };
  NSSize ts = [t sizeWithAttributes:attrs];
  [t drawAtPoint:NSMakePoint(NSMidX(b) - ts.width / 2, NSMidY(b) - ts.height / 2) withAttributes:attrs];
}
@end

// Flipped container so scrolled content starts at the top.
@interface SNCFlippedView : NSView
@end
@implementation SNCFlippedView
- (BOOL)isFlipped {
  return YES;
}
@end

// ============================================================================ the window

@implementation SNCMainWindow {
  std::weak_ptr<AppController> _controller;
  MainWindowServices _services;
  BOOL _settingsPage;
  NSView* _header;
  NSScrollView* _scroll;
  NSStackView* _page;
  NSView* _footer;
  SNCTileGrid* _grid;
  std::map<std::string, NSImage*> _thumbs;
  NSTimer* _permissionTimer;
  SNCShortcutField* _activeShortcutField;
}

- (instancetype)initWithController:(std::weak_ptr<AppController>)controller services:(MainWindowServices)services {
  if ((self = [super initWithContentRect:NSMakeRect(0, 0, 600, 700)
                               styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable |
                                         NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable
                                 backing:NSBackingStoreBuffered
                                   defer:NO])) {
    _controller = std::move(controller);
    _services = std::move(services);
    self.title = @"SnapCap";
    self.releasedWhenClosed = NO;
    self.delegate = self;
    self.contentMinSize = NSMakeSize(520, 560);
    self.frameAutosaveName = @"SnapCapMain";
    if (![self setFrameUsingName:@"SnapCapMain"]) [self center];

    NSView* root = [[NSView alloc] init];
    self.contentView = root;
    _scroll = [[NSScrollView alloc] init];
    _scroll.hasVerticalScroller = YES;
    _scroll.drawsBackground = NO;
    _scroll.autohidesScrollers = YES;
    SNCFlippedView* doc = [[SNCFlippedView alloc] init];
    _scroll.documentView = doc;
    _page = vstack(@[], 8);
    _page.edgeInsets = NSEdgeInsetsMake(4, 20, 16, 20);
    _page.translatesAutoresizingMaskIntoConstraints = NO;
    [doc addSubview:_page];
    doc.translatesAutoresizingMaskIntoConstraints = NO;
    [NSLayoutConstraint activateConstraints:@[
      [doc.leadingAnchor constraintEqualToAnchor:_scroll.contentView.leadingAnchor],
      [doc.trailingAnchor constraintEqualToAnchor:_scroll.contentView.trailingAnchor],
      [doc.topAnchor constraintEqualToAnchor:_scroll.contentView.topAnchor],
      [_page.leadingAnchor constraintEqualToAnchor:doc.leadingAnchor],
      [_page.trailingAnchor constraintEqualToAnchor:doc.trailingAnchor],
      [_page.topAnchor constraintEqualToAnchor:doc.topAnchor],
      [_page.bottomAnchor constraintEqualToAnchor:doc.bottomAnchor],
    ]];
    _scroll.translatesAutoresizingMaskIntoConstraints = NO;
    [root addSubview:_scroll];
    [NSLayoutConstraint activateConstraints:@[
      [_scroll.leadingAnchor constraintEqualToAnchor:root.leadingAnchor],
      [_scroll.trailingAnchor constraintEqualToAnchor:root.trailingAnchor],
    ]];
    [self reload];
  }
  return self;
}

- (void)windowWillClose:(NSNotification*)n {
  [_permissionTimer invalidate];
  _permissionTimer = nil;
}

- (BOOL)windowShouldClose:(NSWindow*)sender {
  // The controller decides: hide to the menu bar, or quit when there is no menu bar item.
  if (auto c = _controller.lock()) c->on_main_window_closed();
  return NO;
}

- (void)windowDidBecomeKey:(NSNotification*)n {
  if (auto c = _controller.lock()) {
    if (!c->has_permission()) c->refresh_permission();
  }
}

// ---------------------------------------------------------------- page building

- (void)update:(void (^)(Settings& s))change {
  auto c = _controller.lock();
  if (!c) return;
  Settings s = c->settings();
  change(s);
  c->update_settings(s);
}

- (void)reload {
  auto c = _controller.lock();
  if (!c) return;
  NSPoint scrolled = _scroll.contentView.bounds.origin;
  [_header removeFromSuperview];
  [_footer removeFromSuperview];
  _header = [self buildHeader:*c];
  _footer = [self buildFooter:*c];
  NSView* root = self.contentView;
  for (NSView* v in @[ _header, _footer ]) {
    v.translatesAutoresizingMaskIntoConstraints = NO;
    [root addSubview:v];
  }
  // These constraints go away with the old header and footer views.
  [NSLayoutConstraint activateConstraints:@[
    [_header.topAnchor constraintEqualToAnchor:root.topAnchor constant:12],
    [_header.leadingAnchor constraintEqualToAnchor:root.leadingAnchor constant:20],
    [_header.trailingAnchor constraintEqualToAnchor:root.trailingAnchor constant:-14],
    [_scroll.topAnchor constraintEqualToAnchor:_header.bottomAnchor constant:8],
    [_footer.topAnchor constraintEqualToAnchor:_scroll.bottomAnchor constant:6],
    [_footer.leadingAnchor constraintEqualToAnchor:root.leadingAnchor constant:20],
    [_footer.trailingAnchor constraintEqualToAnchor:root.trailingAnchor constant:-20],
    [_footer.bottomAnchor constraintEqualToAnchor:root.bottomAnchor constant:-10],
  ]];

  for (NSView* v in [_page.arrangedSubviews copy]) [v removeFromSuperview];
  _grid = nil;
  _activeShortcutField = nil;
  NSArray<NSView*>* views = _settingsPage ? [self settingsViews:*c] : [self homeViews:*c];
  for (NSView* v in views) {
    [_page addArrangedSubview:v];
    [v.widthAnchor constraintEqualToAnchor:_page.widthAnchor constant:-40].active = YES;
  }
  [_page layoutSubtreeIfNeeded];
  [_scroll.contentView scrollToPoint:scrolled];
  [_scroll reflectScrolledClipView:_scroll.contentView];
  if (_activeShortcutField != nil) [self makeFirstResponder:_activeShortcutField];

  // While permission is missing, notice when the user grants it.
  if (!c->has_permission() && _permissionTimer == nil) {
    __weak SNCMainWindow* weak = self;
  std::weak_ptr<AppController> ctlw = _controller;
    _permissionTimer = [NSTimer scheduledTimerWithTimeInterval:2
                                                       repeats:YES
                                                         block:^(NSTimer* t) {
                                                           SNCMainWindow* w = weak;
                                                           if (w == nil || !w.visible) return;
                                                           if (auto ctl = w->_controller.lock()) ctl->refresh_permission();
                                                         }];
  } else if (c->has_permission()) {
    [_permissionTimer invalidate];
    _permissionTimer = nil;
  }
}

- (NSView*)buildHeader:(AppController&)c {
  __weak SNCMainWindow* weak = self;
  std::weak_ptr<AppController> ctlw = _controller;
  NSMutableArray<NSView*>* left = [NSMutableArray array];
  if (_settingsPage) {
    [left addObject:icon_button(@"chevron.left", @"Back", ^(id) {
            SNCMainWindow* w = weak;
            w->_settingsPage = NO;
            if (auto ctl = w->_controller.lock()) ctl->end_shortcut_capture();
            [w reload];
          })];
    [left addObject:label(@"Settings", 19, NSFontWeightBold)];
  } else {
    NSImageView* logo = [NSImageView imageViewWithImage:mac::nsimage(app_icon(64), 0.5)];
    [left addObject:logo];
    NSStackView* titles = vstack(@[ label(@"SnapCap", 18, NSFontWeightBold),
                                    label(@"Screenshots & screen recording", 11.5, NSFontWeightRegular,
                                          NSColor.secondaryLabelColor) ],
                                 0);
    [left addObject:titles];
  }
  NSMutableArray<NSView*>* right = [NSMutableArray array];
  [right addObject:icon_button(@"folder", @"Open screenshots folder", ^(id) {
           if (auto ctl = ctlw.lock()) ctl->open_save_folder();
         })];
  if (!_settingsPage) {
    [right addObject:icon_button(@"gearshape", @"Settings", ^(id) {
             SNCMainWindow* w = weak;
             w->_settingsPage = YES;
             [w reload];
           })];
  }
  return row(hstack(left, 10), right);
}

- (NSView*)buildFooter:(AppController&)c {
  std::string_view mode = c.mode_name();
  if (mode == "recording" || mode == "finalizing" || mode == "countdown") {
    NSColor* color = mode == "recording" ? mac::theme::coral() : mac::theme::amber();
    NSString* text = mode == "recording" ? @"Recording…" : mode == "finalizing" ? @"Saving recording…" : @"Starting…";
    return hstack(@[ [NSImageView imageViewWithImage:symbol(@"circle.fill", 8, color)],
                     label(text, 11.5, NSFontWeightRegular, NSColor.secondaryLabelColor) ],
                  6);
  }
  std::filesystem::path dir = save_dir(c.settings());
  std::weak_ptr<AppController> ctlw = _controller;
  NSButton* link = link_button(mac::ns(shorten_home(dir)), ^(id) {
    if (auto ctl = ctlw.lock()) ctl->open_save_folder();
  });
  link.toolTip = [NSString stringWithFormat:@"%@\nClick to open", mac::ns(dir.string())];
  [link setContentCompressionResistancePriority:NSLayoutPriorityDefaultLow
                                 forOrientation:NSLayoutConstraintOrientationHorizontal];
  return hstack(@[ label(@"Saving to", 11.5, NSFontWeightRegular, NSColor.secondaryLabelColor), link ], 4);
}

// ---------------------------------------------------------------- home

- (NSArray<NSView*>*)homeViews:(AppController&)c {
  NSMutableArray<NSView*>* views = [NSMutableArray array];
  __weak SNCMainWindow* weak = self;
  std::weak_ptr<AppController> ctlw = _controller;
  const Settings& s = c.settings();
  if (!c.has_permission()) [views addObject:[self permissionBanner]];
  BOOL busy = !c.idle();

  auto card = [&](Action a, NSString* sym, NSString* title, NSColor* accent) {
    SNCActionCard* v = [[SNCActionCard alloc] init];
    v.symbolName = sym;
    v.title = title;
    std::string text = std::string(s.shortcut(a));
    v.shortcut = text.empty() ? @"" : mac::ns(display_shortcut(text));
    v.accent = accent;
    v.enabled = !busy;
    on_action(v, ^(id) {
      if (auto ctl = ctlw.lock()) ctl->trigger(a);
    });
    return v;
  };
  auto cards = [](NSArray<NSView*>* list) {
    NSStackView* r = hstack(list, 10);
    r.distribution = NSStackViewDistributionFillEqually;
    return r;
  };

  [views addObject:section(@"Screenshot")];
  [views addObject:cards(@[ card(Action::ShotScreen, @"display", @"Screen", mac::theme::accent()),
                            card(Action::ShotRegion, @"rectangle.dashed", @"Region", mac::theme::accent()),
                            card(Action::ShotAllScreens, @"rectangle.split.2x2", @"All screens", mac::theme::accent()) ])];
  [views addObject:section(@"Record")];
  [views addObject:cards(@[ card(Action::RecordScreen, @"record.circle", @"Record screen", mac::theme::coral()),
                            card(Action::RecordRegion, @"rectangle.dashed.badge.record", @"Record region",
                                 mac::theme::coral()) ])];
  if (c.recording()) {
    NSButton* stop = [NSButton buttonWithTitle:@"Stop recording" target:nil action:nil];
    stop.bezelColor = mac::theme::coral();
    stop.keyEquivalent = @"";
    on_action(stop, ^(id) {
      if (auto ctl = ctlw.lock()) ctl->trigger(Action::StopRecording);
    });
    [views addObject:stop];
  }

  // Quick recording options.
  BOOL mp4 = s.video_format == VideoFormat::Mp4;
  constexpr std::array<std::pair<VideoFormat, const char*>, 2> kFormats{{{VideoFormat::Mp4, "MP4"}, {VideoFormat::Gif, "GIF"}}};
  NSSegmentedControl* format = segmented(kFormats, s.video_format, ^(VideoFormat f) {
    [weak update:^(Settings& st) { st.video_format = f; }];
  });
  NSSwitch* mic = toggle(s.record_mic, ^(bool on) {
    [weak update:^(Settings& st) { st.record_mic = on; }];
  });
  NSSwitch* sys = toggle(s.record_system_audio, ^(bool on) {
    [weak update:^(Settings& st) { st.record_system_audio = on; }];
  });
  mic.enabled = sys.enabled = mp4;
  NSMutableArray* quick = [NSMutableArray arrayWithObjects:format, spacer(),
                                                           [NSImageView imageViewWithImage:symbol(@"mic", 13, NSColor.secondaryLabelColor)],
                                                           label(@"Mic", 12, NSFontWeightRegular, NSColor.secondaryLabelColor), mic,
                                                           [NSImageView imageViewWithImage:symbol(@"speaker.wave.2", 13, NSColor.secondaryLabelColor)],
                                                           label(@"System audio", 12, NSFontWeightRegular, NSColor.secondaryLabelColor),
                                                           sys, nil];
  if (!mp4) [quick insertObject:label(@"GIFs have no audio", 11, NSFontWeightRegular, NSColor.secondaryLabelColor) atIndex:1];
  [views addObject:[SNCGroupView groupWithRows:@[ hstack(quick, 8) ]]];

  // Recent captures.
  NSButton* all = link_button(@"Show all", ^(id) {
    if (auto ctl = ctlw.lock()) ctl->open_save_folder();
  });
  all.hidden = c.recent().empty();
  [views addObject:row(section(@"Recent"), @[ all ])];
  [views addObject:[self recentView:c.recent()]];
  return views;
}

- (NSView*)permissionBanner {
  NSImageView* icon = [NSImageView imageViewWithImage:symbol(@"exclamationmark.shield", 22, mac::theme::amber())];
  NSButton* open = [NSButton buttonWithTitle:@"Open System Settings" target:nil action:nil];
  open.bezelColor = mac::theme::accent();
  on_action(open, ^(id) {
    CGRequestScreenCaptureAccess();
    [NSWorkspace.sharedWorkspace
        openURL:[NSURL URLWithString:@"x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"]];
  });
  NSStackView* text = vstack(@[ label(@"Screen Recording permission needed", 13, NSFontWeightSemibold),
                                wrapping_label(@"Enable SnapCap under Privacy & Security → Screen & System Audio "
                                               @"Recording. SnapCap notices as soon as it's allowed.",
                                               11.5, NSColor.secondaryLabelColor),
                                open ],
                             6);
  NSStackView* banner = hstack(@[ icon, text ], 12);
  banner.alignment = NSLayoutAttributeTop;
  banner.edgeInsets = NSEdgeInsetsMake(12, 14, 12, 14);
  banner.wantsLayer = YES;
  banner.layer.cornerRadius = 12;
  banner.layer.backgroundColor = [mac::theme::amber() colorWithAlphaComponent:0.12].CGColor;
  banner.layer.borderColor = [mac::theme::amber() colorWithAlphaComponent:0.6].CGColor;
  banner.layer.borderWidth = 1;
  return banner;
}

- (NSView*)recentView:(const std::vector<std::filesystem::path>&)files {
  if (files.empty()) {
    NSStackView* empty = vstack(@[ [NSImageView imageViewWithImage:symbol(@"photo", 26, NSColor.tertiaryLabelColor)],
                                   label(@"Your captures will appear here", 13, NSFontWeightRegular,
                                         NSColor.secondaryLabelColor) ],
                                6);
    empty.alignment = NSLayoutAttributeCenterX;
    empty.edgeInsets = NSEdgeInsetsMake(14, 0, 14, 0);
    return [SNCGroupView groupWithRows:@[ empty ]];
  }
  _grid = [[SNCTileGrid alloc] init];
  _grid.columns = 4;
  __weak SNCMainWindow* weak = self;
  std::weak_ptr<AppController> ctlw = _controller;
  for (const auto& path : files) {
    SNCRecentTile* tile = [[SNCRecentTile alloc] init];
    tile.path = path;
    tile.toolTip = mac::ns(path.filename().string());
    tile.thumbnail = [self thumbnailFor:path tile:tile];
    std::filesystem::path p = path;
    tile.onOpen = ^{
      SNCMainWindow* w = weak;
      if (w != nil) w->_services.open(p);
    };
    tile.makeMenu = ^NSMenu* {
      return [weak menuFor:p];
    };
    [_grid addSubview:tile];
  }
  return _grid;
}

- (NSMenu*)menuFor:(const std::filesystem::path&)path {
  NSMenu* menu = [[NSMenu alloc] init];
  std::filesystem::path p = path;
  std::weak_ptr<AppController> ctlw = _controller;
  auto add = [&](NSString* title, NSString* sym, void (^block)(void)) {
    NSMenuItem* item = [[NSMenuItem alloc] initWithTitle:title action:@selector(fire:) keyEquivalent:@""];
    item.image = [NSImage imageWithSystemSymbolName:sym accessibilityDescription:nil];
    SNCBlockTarget* t = [[SNCBlockTarget alloc] init];
    t.block = ^(id) {
      block();
    };
    item.target = t;
    item.representedObject = t;  // keeps the target alive with the item
    [menu addItem:item];
  };
  auto open_fn = _services.open;
  auto reveal_fn = _services.reveal;
  add(@"Open", @"photo", ^{
    open_fn(p);
  });
  add(@"Show in Folder", @"folder", ^{
    reveal_fn(p);
  });
  add(@"Copy Path", @"doc.on.doc", ^{
    [NSPasteboard.generalPasteboard clearContents];
    [NSPasteboard.generalPasteboard setString:mac::ns(p.string()) forType:NSPasteboardTypeString];
  });
  [menu addItem:NSMenuItem.separatorItem];
  add(@"Move to Trash", @"trash", ^{
    [NSFileManager.defaultManager trashItemAtURL:mac::file_url(p) resultingItemURL:nil error:nil];
    if (auto ctl = ctlw.lock()) ctl->refresh_recent();
  });
  return menu;
}

- (NSImage*)thumbnailFor:(const std::filesystem::path&)path tile:(SNCRecentTile*)tile {
  std::error_code ec;
  auto stamp = std::filesystem::last_write_time(path, ec).time_since_epoch().count();
  std::string key = std::format("{}|{}", path.string(), stamp);
  if (auto it = _thumbs.find(key); it != _thumbs.end()) return it->second;
  QLThumbnailGenerationRequest* req =
      [[QLThumbnailGenerationRequest alloc] initWithFileAtURL:mac::file_url(path)
                                                         size:NSMakeSize(320, 212)
                                                        scale:self.backingScaleFactor ?: 2
                                          representationTypes:QLThumbnailGenerationRequestRepresentationTypeThumbnail];
  __weak SNCMainWindow* weak = self;
  std::weak_ptr<AppController> ctlw = _controller;
  __weak SNCRecentTile* weak_tile = tile;
  [QLThumbnailGenerator.sharedGenerator
      generateBestRepresentationForRequest:req
                         completionHandler:^(QLThumbnailRepresentation* rep, NSError*) {
                           NSImage* image = rep.NSImage;
                           if (image == nil) return;
                           dispatch_async(dispatch_get_main_queue(), ^{
                             SNCMainWindow* w = weak;
                             if (w == nil) return;
                             w->_thumbs[key] = image;
                             SNCRecentTile* t = weak_tile;
                             t.thumbnail = image;
                             t.needsDisplay = YES;
                           });
                         }];
  return nil;
}

- (void)setRecent:(const std::vector<std::filesystem::path>&)files {
  if (!_settingsPage && self.visible) [self reload];
}

// ---------------------------------------------------------------- settings

- (NSArray<NSView*>*)settingsViews:(AppController&)c {
  NSMutableArray<NSView*>* views = [NSMutableArray array];
  __weak SNCMainWindow* weak = self;
  std::weak_ptr<AppController> ctlw = _controller;
  const Settings& s = c.settings();

  // General
  std::filesystem::path dir = save_dir(s);
  NSButton* change = [NSButton buttonWithTitle:@"Change…" target:nil action:nil];
  on_action(change, ^(id) {
    [weak chooseFolder];
  });
  NSMutableArray* folder_buttons = [NSMutableArray arrayWithObject:change];
  if (s.save_dir) {
    NSButton* reset = [NSButton buttonWithTitle:@"Reset" target:nil action:nil];
    reset.toolTip = @"Use ~/Desktop/screenshots";
    on_action(reset, ^(id) {
      [weak update:^(Settings& st) { st.save_dir.reset(); }];
    });
    [folder_buttons insertObject:reset atIndex:0];
  }
  NSTextField* path = label(mac::ns(shorten_home(dir)), 11.5, NSFontWeightRegular, NSColor.secondaryLabelColor);
  path.toolTip = mac::ns(dir.string());
  constexpr std::array<std::pair<ThemePref, const char*>, 3> kThemes{
      {{ThemePref::System, "System"}, {ThemePref::Light, "Light"}, {ThemePref::Dark, "Dark"}}};
  [views addObject:section(@"General")];
  [views addObject:[SNCGroupView groupWithRows:@[
           row(label(@"Save captures to"), folder_buttons),
           path,
           row(label(@"Copy screenshots to clipboard"), @[ toggle(s.copy_to_clipboard, ^(bool on) {
                 [weak update:^(Settings& st) { st.copy_to_clipboard = on; }];
               }) ]),
           row(label(@"Open file after capture"), @[ toggle(s.open_after_capture, ^(bool on) {
                 [weak update:^(Settings& st) { st.open_after_capture = on; }];
               }) ]),
           row(label(@"Appearance"), @[ segmented(kThemes, s.theme, ^(ThemePref t) {
                 [weak update:^(Settings& st) { st.theme = t; }];
               }) ]),
         ]]];

  // Recording
  constexpr std::array<std::pair<VideoFormat, const char*>, 2> kFormats{{{VideoFormat::Mp4, "MP4"}, {VideoFormat::Gif, "GIF"}}};
  NSMutableArray* rec = [NSMutableArray array];
  [rec addObject:row(label(@"Format"), @[ segmented(kFormats, s.video_format, ^(VideoFormat f) {
         [weak update:^(Settings& st) { st.video_format = f; }];
       }) ])];
  if (s.video_format == VideoFormat::Mp4) {
    constexpr std::array<std::pair<uint32_t, const char*>, 4> kFps{{{15, "15"}, {24, "24"}, {30, "30"}, {60, "60"}}};
    constexpr std::array<std::pair<Quality, const char*>, 3> kQuality{
        {{Quality::Low, "Small file"}, {Quality::Medium, "Balanced"}, {Quality::High, "High quality"}}};
    constexpr std::array<std::pair<ResolutionCap, const char*>, 4> kRes{{{ResolutionCap::Native, "Native"},
                                                                         {ResolutionCap::P1440, "1440p"},
                                                                         {ResolutionCap::P1080, "1080p"},
                                                                         {ResolutionCap::P720, "720p"}}};
    [rec addObject:row(label(@"Frame rate"), @[ segmented(kFps, s.fps, ^(uint32_t v) {
           [weak update:^(Settings& st) { st.fps = v; }];
         }) ])];
    [rec addObject:row(label(@"Quality"), @[ segmented(kQuality, s.quality, ^(Quality q) {
           [weak update:^(Settings& st) { st.quality = q; }];
         }) ])];
    [rec addObject:row(label(@"Maximum resolution"), @[ segmented(kRes, s.resolution, ^(ResolutionCap r) {
           [weak update:^(Settings& st) { st.resolution = r; }];
         }) ])];
  } else {
    constexpr std::array<std::pair<uint32_t, const char*>, 4> kGifFps{{{10, "10"}, {15, "15"}, {20, "20"}, {25, "25"}}};
    constexpr std::array<std::pair<uint32_t, const char*>, 4> kGifWidth{
        {{480, "480"}, {640, "640"}, {960, "960"}, {1280, "1280"}}};
    [rec addObject:row(label(@"GIF frame rate"), @[ segmented(kGifFps, s.gif_fps, ^(uint32_t v) {
           [weak update:^(Settings& st) { st.gif_fps = v; }];
         }) ])];
    [rec addObject:row(label(@"GIF width"), @[ segmented(kGifWidth, s.gif_max_width, ^(uint32_t v) {
           [weak update:^(Settings& st) { st.gif_max_width = v; }];
         }) ])];
  }
  constexpr std::array<std::pair<uint32_t, const char*>, 4> kCountdown{{{0, "Off"}, {3, "3 s"}, {5, "5 s"}, {10, "10 s"}}};
  [rec addObject:row(label(@"Countdown"), @[ segmented(kCountdown, s.countdown_secs, ^(uint32_t v) {
         [weak update:^(Settings& st) { st.countdown_secs = v; }];
       }) ])];
  [rec addObject:row(label(@"Show mouse cursor"), @[ toggle(s.show_cursor, ^(bool on) {
         [weak update:^(Settings& st) { st.show_cursor = on; }];
       }) ])];
  [views addObject:section(@"Recording")];
  [views addObject:[SNCGroupView groupWithRows:rec]];

  // Audio
  NSMutableArray* audio = [NSMutableArray array];
  NSStackView* mic_label = vstack(@[ label(@"Record microphone"),
                                     label(@"MP4 recordings only", 11, NSFontWeightRegular, NSColor.secondaryLabelColor) ],
                                  1);
  [audio addObject:row(mic_label, @[ toggle(s.record_mic, ^(bool on) {
           [weak update:^(Settings& st) { st.record_mic = on; }];
         }) ])];
  if (s.record_mic) {
    NSPopUpButton* pick = [[NSPopUpButton alloc] initWithFrame:NSZeroRect pullsDown:NO];
    [pick addItemWithTitle:@"System default"];
    for (const auto& name : _services.microphones()) [pick addItemWithTitle:mac::ns(name)];
    if (s.mic_device) {
      NSString* current = mac::ns(*s.mic_device);
      if ([pick itemWithTitle:current] == nil) [pick addItemWithTitle:current];
      [pick selectItemWithTitle:current];
    }
    on_action(pick, ^(NSPopUpButton* sender) {
      NSInteger i = sender.indexOfSelectedItem;
      std::string name = mac::str(sender.titleOfSelectedItem);
      [weak update:^(Settings& st) {
        if (i <= 0) {
          st.mic_device.reset();
        } else {
          st.mic_device = name;
        }
      }];
    });
    [audio addObject:row(label(@"Microphone", 13, NSFontWeightRegular, NSColor.secondaryLabelColor), @[ pick ])];
  }
  NSStackView* sys_label = vstack(@[ label(@"Record system audio"),
                                     label(@"Everything you hear on this Mac", 11, NSFontWeightRegular,
                                           NSColor.secondaryLabelColor) ],
                                  1);
  [audio addObject:row(sys_label, @[ toggle(s.record_system_audio, ^(bool on) {
           [weak update:^(Settings& st) { st.record_system_audio = on; }];
         }) ])];
  [views addObject:section(@"Audio")];
  [views addObject:[SNCGroupView groupWithRows:audio]];

  // Keyboard shortcuts
  NSButton* reset = link_button(@"Reset to defaults", ^(id) {
    if (auto ctl = ctlw.lock()) ctl->end_shortcut_capture();
    [weak update:^(Settings& st) { st.shortcuts = default_shortcuts(); }];
  });
  [views addObject:row(section(@"Keyboard shortcuts"), @[ reset ])];
  [views addObject:[SNCGroupView groupWithRows:[self shortcutRows:c]]];

  // Command line
  NSMutableArray* cli = [NSMutableArray array];
  [cli addObject:wrapping_label(@"Script SnapCap, or bind these commands in other tools:", 11.5,
                                NSColor.secondaryLabelColor)];
  NSString* exe = NSBundle.mainBundle.executablePath ?: @"snapcap";
  for (NSString* args in @[ @"--shot screen", @"--shot region", @"--record region", @"--record stop" ]) {
    NSString* cmd = [NSString stringWithFormat:@"\"%@\" %@", exe, args];
    NSTextField* t = label(cmd, 11.5, NSFontWeightRegular);
    t.font = [NSFont monospacedSystemFontOfSize:11 weight:NSFontWeightRegular];
    t.selectable = YES;
    [cli addObject:row(t, @[ icon_button(@"doc.on.doc", @"Copy", ^(id) {
           [NSPasteboard.generalPasteboard clearContents];
           [NSPasteboard.generalPasteboard setString:cmd forType:NSPasteboardTypeString];
         }) ])];
  }
  [views addObject:section(@"Command line")];
  [views addObject:[SNCGroupView groupWithRows:cli]];

  NSTextField* about = label([NSString stringWithFormat:@"SnapCap %s · Everything stays on this computer", SNAPCAP_VERSION],
                             11, NSFontWeightRegular, NSColor.secondaryLabelColor);
  about.alignment = NSTextAlignmentCenter;
  [views addObject:about];
  return views;
}

- (NSArray<NSView*>*)shortcutRows:(AppController&)c {
  NSMutableArray* rows = [NSMutableArray array];
  __weak SNCMainWindow* weak = self;
  std::weak_ptr<AppController> ctlw = _controller;
  const Settings& s = c.settings();
  auto dups = duplicate_shortcuts(s.shortcuts);
  for (Action a : kAllActions) {
    std::string text(s.shortcut(a));
    BOOL capturing = c.capturing_shortcut() == a;
    SNCShortcutField* field = [[SNCShortcutField alloc] init];
    field.action = a;
    field.capturing = capturing;
    field.text = mac::ns(display_shortcut(text));
    field.toolTip = capturing ? @"Press the new shortcut, Backspace to remove it, Esc to cancel" : @"Click to change";
    field.onClick = ^{
      if (auto ctl = ctlw.lock()) {
        if (ctl->capturing_shortcut() == a) {
          ctl->end_shortcut_capture();
        } else {
          ctl->end_shortcut_capture();
          ctl->begin_shortcut_capture(a);
        }
      }
      [weak reload];
    };
    field.onKey = ^(NSEvent* e) {
      [weak shortcutKey:e forAction:a];
    };
    field.onResign = ^{
      SNCMainWindow* w = weak;
      if (w == nil) return;
      // Clicking elsewhere cancels; a rebuild that re-focuses the field does not.
      if (w.firstResponder != w->_activeShortcutField) {
        if (auto ctl = w->_controller.lock(); ctl && ctl->capturing_shortcut() == a) {
          ctl->end_shortcut_capture();
          [w reload];
        }
      }
    };
    if (capturing) _activeShortcutField = field;
    [field setContentHuggingPriority:NSLayoutPriorityRequired forOrientation:NSLayoutConstraintOrientationHorizontal];

    NSMutableArray* right = [NSMutableArray arrayWithObject:field];
    NSButton* remove = icon_button(@"xmark", @"Remove shortcut", ^(id) {
      [weak update:^(Settings& st) { st.shortcuts[a] = ""; }];
    });
    remove.hidden = text.empty() || capturing;
    [right addObject:remove];
    NSStackView* line = row(label(mac::ns(label(a))), right);
    line.detachesHiddenViews = NO;  // keep the shortcut fields in one column
    [rows addObject:line];

    std::optional<std::pair<NSColor*, std::string>> problem;
    if (auto d = dups.find(a); d != dups.end()) {
      problem = {NSColor.systemRedColor, std::format("Same as “{}”", label(d->second))};
    } else if (auto e = c.hotkey_errors().find(a); e != c.hotkey_errors().end()) {
      problem = {NSColor.systemRedColor, e->second};
    } else if (auto w = system_conflict(text)) {
      problem = {mac::theme::amber(), std::format("{} — it may not trigger SnapCap", *w)};
    }
    if (problem) {
      NSTextField* t = label(mac::ns("⚠︎  " + problem->second), 11, NSFontWeightRegular, problem->first);
      [rows addObject:t];
    }
  }
  return rows;
}

- (void)shortcutKey:(NSEvent*)e forAction:(Action)a {
  auto c = _controller.lock();
  if (!c) return;
  uint8_t mods = mac::modifiers_from(e.modifierFlags);
  if (mods == 0 && e.keyCode == 53) {  // Escape: cancel
    c->end_shortcut_capture();
    return [self reload];
  }
  if (mods == 0 && (e.keyCode == 51 || e.keyCode == 117)) {  // Backspace / Delete: remove
    [self update:^(Settings& st) { st.shortcuts[a] = ""; }];
    c->end_shortcut_capture();
    return [self reload];
  }
  auto name = mac::key_name(e.keyCode);
  if (!name) return NSBeep();
  auto text = shortcut_from_key_press(mods, *name);
  if (!text) return;  // e.g. a plain letter: wait for a combination
  std::string value = *text;
  [self update:^(Settings& st) { st.shortcuts[a] = value; }];
  c->end_shortcut_capture();
  [self reload];
}

- (void)chooseFolder {
  auto c = _controller.lock();
  if (!c) return;
  NSOpenPanel* panel = [NSOpenPanel openPanel];
  panel.canChooseDirectories = YES;
  panel.canChooseFiles = NO;
  panel.canCreateDirectories = YES;
  panel.prompt = @"Choose";
  panel.directoryURL = mac::file_url(save_dir(c->settings()));
  __weak SNCMainWindow* weak = self;
  std::weak_ptr<AppController> ctlw = _controller;
  [panel beginSheetModalForWindow:self
                completionHandler:^(NSModalResponse r) {
                  if (r != NSModalResponseOK || panel.URL == nil) return;
                  std::string chosen = mac::str(panel.URL.path);
                  [weak update:^(Settings& st) { st.save_dir = std::filesystem::path(chosen); }];
                }];
}

@end
