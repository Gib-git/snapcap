#import "recording_ui.h"

#import <QuartzCore/QuartzCore.h>

#include <array>
#include <format>

#include "mac_util.h"
#import "theme.h"
#import "widgets.h"

using namespace snapcap;

namespace {

std::string format_elapsed(snapcap::Duration d) {
  auto s = std::chrono::duration_cast<std::chrono::seconds>(d).count();
  if (s >= 3600) return std::format("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60);
  return std::format("{}:{:02}", s / 60, s % 60);
}

NSPanel* make_panel(NSRect frame, BOOL protect) {
  NSPanel* p = [[NSPanel alloc] initWithContentRect:frame
                                          styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
                                            backing:NSBackingStoreBuffered
                                              defer:NO];
  p.opaque = NO;
  p.backgroundColor = NSColor.clearColor;
  // Note: setting floatingPanel would reset the level to NSFloatingWindowLevel, below the Dock.
  p.level = NSStatusWindowLevel;
  p.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary |
                         NSWindowCollectionBehaviorIgnoresCycle;
  p.hidesOnDeactivate = NO;
  p.releasedWhenClosed = NO;
  p.becomesKeyOnlyIfNeeded = YES;
  if (protect) p.sharingType = NSWindowSharingNone;
  return p;
}

}  // namespace

// The bar's background: dark pill, draggable.
@interface SNCBarView : NSView
@end

@implementation SNCBarView
- (BOOL)mouseDownCanMoveWindow {
  return YES;
}
- (BOOL)isFlipped {
  return YES;
}
- (void)drawRect:(NSRect)dirty {
  NSRect b = NSInsetRect(self.bounds, 0.5, 0.5);
  NSBezierPath* p = [NSBezierPath bezierPathWithRoundedRect:b xRadius:NSHeight(b) / 2 yRadius:NSHeight(b) / 2];
  [[NSColor colorWithSRGBRed:22 / 255.0 green:23 / 255.0 blue:29 / 255.0 alpha:0.95] setFill];
  [p fill];
  [[NSColor colorWithWhite:1 alpha:0.11] setStroke];
  [p stroke];
}
@end

// A thin coral (amber while paused) frame just outside the recorded region. Click-through.
@interface SNCBorderView : NSView
@property(nonatomic) BOOL paused;
@end

@implementation SNCBorderView
- (void)drawRect:(NSRect)dirty {
  NSColor* color = self.paused ? mac::theme::amber() : mac::theme::coral();
  [color setStroke];
  NSRect b = NSInsetRect(self.bounds, 2, 2);
  NSBezierPath* frame = [NSBezierPath bezierPathWithRoundedRect:b xRadius:3 yRadius:3];
  frame.lineWidth = 2;
  [frame stroke];
  // Corner accents.
  NSBezierPath* corners = [NSBezierPath bezierPath];
  corners.lineWidth = 4;
  corners.lineCapStyle = NSLineCapStyleRound;
  constexpr double len = 14;
  for (auto [x, y, dx, dy] : {std::array{NSMinX(b), NSMinY(b), 1.0, 1.0}, std::array{NSMaxX(b), NSMinY(b), -1.0, 1.0},
                              std::array{NSMinX(b), NSMaxY(b), 1.0, -1.0}, std::array{NSMaxX(b), NSMaxY(b), -1.0, -1.0}}) {
    [corners moveToPoint:NSMakePoint(x + dx * len, y)];
    [corners lineToPoint:NSMakePoint(x, y)];
    [corners lineToPoint:NSMakePoint(x, y + dy * len)];
  }
  [corners stroke];
}
@end

@implementation SNCRecordingUI {
  std::function<void(BarAction)> _action;
  NSPanel* _bar;
  NSPanel* _border;
  SNCBorderView* _borderView;
  CALayer* _dot;
  NSTextField* _time;
  NSTextField* _badge;
  NSTextField* _message;
  SNCPillButton* _pause;
  SNCPillButton* _stop;
  SNCPillButton* _cancel;
  std::optional<CaptureTarget> _placedFor;
  std::optional<BarMode> _mode;
  BOOL _paused;
}

- (instancetype)initWithAction:(std::function<void(BarAction)>)action protectFromCapture:(BOOL)protect {
  if ((self = [super init])) {
    _action = std::move(action);
    _bar = make_panel(NSMakeRect(0, 0, mac::kBarWidth, mac::kBarHeight), protect);
    _bar.movableByWindowBackground = YES;
    _bar.hasShadow = YES;
    _bar.title = @"SnapCap Recording";
    SNCBarView* bg = [[SNCBarView alloc] initWithFrame:NSMakeRect(0, 0, mac::kBarWidth, mac::kBarHeight)];
    bg.wantsLayer = YES;
    _bar.contentView = bg;

    _dot = [CALayer layer];
    _dot.frame = CGRectMake(18, mac::kBarHeight / 2 - 6, 12, 12);
    _dot.cornerRadius = 6;
    [bg.layer addSublayer:_dot];

    _time = [self label:[NSFont monospacedDigitSystemFontOfSize:16 weight:NSFontWeightMedium] color:NSColor.whiteColor];
    _time.frame = NSMakeRect(38, 14, 70, 20);
    _badge = [self label:[NSFont systemFontOfSize:11 weight:NSFontWeightSemibold]
                   color:[NSColor colorWithWhite:1 alpha:0.55]];
    _badge.frame = NSMakeRect(104, 17, 56, 15);
    _message = [self label:[NSFont systemFontOfSize:14.5 weight:NSFontWeightMedium] color:NSColor.whiteColor];
    _message.frame = NSMakeRect(22, 14, 150, 20);
    for (NSView* v in @[ _time, _badge, _message ]) [bg addSubview:v];

    _pause = [self roundButton:@"pause.fill" fill:nil action:@selector(pauseClicked:) tip:@"Pause"];
    _stop = [self roundButton:@"stop.fill" fill:mac::theme::coral() action:@selector(stopClicked:) tip:@"Stop and save"];
    _cancel = [self roundButton:@"xmark" fill:nil action:@selector(cancelClicked:) tip:@"Cancel"];
    [_pause setFrameOrigin:NSMakePoint(mac::kBarWidth - 82, 8)];
    [_stop setFrameOrigin:NSMakePoint(mac::kBarWidth - 42, 8)];
    [_cancel setFrameOrigin:NSMakePoint(mac::kBarWidth - 42, 8)];
    for (NSView* v in @[ _pause, _stop, _cancel ]) [bg addSubview:v];

    _border = make_panel(NSMakeRect(0, 0, 10, 10), protect);
    _border.ignoresMouseEvents = YES;
    _border.title = @"SnapCap Recording Area";
    _borderView = [[SNCBorderView alloc] initWithFrame:NSMakeRect(0, 0, 10, 10)];
    _border.contentView = _borderView;
  }
  return self;
}

- (NSTextField*)label:(NSFont*)font color:(NSColor*)color {
  NSTextField* t = [NSTextField labelWithString:@""];
  t.font = font;
  t.textColor = color;
  t.drawsBackground = NO;
  t.bezeled = NO;
  return t;
}

- (SNCPillButton*)roundButton:(NSString*)symbol fill:(NSColor*)fill action:(SEL)action tip:(NSString*)tip {
  SNCPillButton* b = [SNCPillButton buttonWithTitle:@"" symbol:symbol fill:fill target:self action:action];
  b.toolTip = tip;
  [b setFrameSize:NSMakeSize(32, 32)];
  return b;
}

- (void)pauseClicked:(id)sender {
  _action(BarAction::TogglePause);
}

- (void)stopClicked:(id)sender {
  _action(BarAction::Stop);
}

- (void)cancelClicked:(id)sender {
  _action(BarAction::Cancel);
}

- (void)place:(const CaptureTarget&)target {
  NSScreen* screen = mac::screen_for_display(target.monitor.id);
  RectF mon = screen != nil ? mac::to_top_left(screen.frame) : target.monitor.bounds();
  RectF region = target.desktop_rect();
  PointF at = recording_bar_origin(mon, region, !target.region.has_value(), {mac::kBarWidth, mac::kBarHeight});
  [_bar setFrame:mac::to_cocoa({at.x, at.y, mac::kBarWidth, mac::kBarHeight}) display:NO];
  if (target.region) {
    constexpr double pad = 4;
    RectF outer{region.x - pad, region.y - pad, region.width + 2 * pad, region.height + 2 * pad};
    [_border setFrame:mac::to_cocoa(outer) display:NO];
    [_border orderFrontRegardless];
  } else {
    [_border orderOut:nil];
  }
}

- (void)showFor:(const CaptureTarget&)target state:(const BarState&)state {
  BOOL same = _placedFor && _placedFor->monitor.id == target.monitor.id && _placedFor->region == target.region;
  if (!same) {
    [self place:target];
    _placedFor = target;
  }
  [self applyState:state];
  [_bar orderFrontRegardless];
}

- (void)applyState:(const BarState&)state {
  BOOL counting = state.mode == BarMode::Countdown;
  BOOL recording = state.mode == BarMode::Recording;
  _dot.hidden = !recording;
  _time.hidden = !recording;
  _badge.hidden = !recording;
  _pause.hidden = !recording;
  _stop.hidden = !recording;
  _cancel.hidden = !counting;
  _message.hidden = recording;
  switch (state.mode) {
    case BarMode::Countdown:
      _message.stringValue = [NSString stringWithFormat:@"Starting in %u…", state.countdown];
      _message.alignment = NSTextAlignmentLeft;
      _message.frame = NSMakeRect(22, 14, 150, 20);
      break;
    case BarMode::Finalizing:
      _message.stringValue = @"Saving…";
      _message.alignment = NSTextAlignmentCenter;
      _message.frame = NSMakeRect(0, 14, mac::kBarWidth, 20);
      break;
    case BarMode::Recording: {
      _time.stringValue = mac::ns(format_elapsed(state.elapsed));
      _badge.stringValue = state.paused ? @"Paused" : mac::ns(state.badge);
      _badge.textColor = state.paused ? mac::theme::amber() : [NSColor colorWithWhite:1 alpha:0.55];
      _pause.symbol = state.paused ? @"play.fill" : @"pause.fill";
      _pause.toolTip = state.paused ? @"Resume" : @"Pause";
      _pause.needsDisplay = YES;
      break;
    }
  }
  if (_mode != state.mode || _paused != state.paused) {
    [self setDotPulsing:recording && !state.paused paused:state.paused];
    _borderView.paused = state.paused;
    _borderView.needsDisplay = YES;
  }
  _mode = state.mode;
  _paused = state.paused;
}

- (void)setDotPulsing:(BOOL)pulse paused:(BOOL)paused {
  _dot.backgroundColor = (paused ? mac::theme::amber() : mac::theme::coral()).CGColor;
  [_dot removeAllAnimations];
  if (!pulse) return;
  CABasicAnimation* a = [CABasicAnimation animationWithKeyPath:@"opacity"];
  a.fromValue = @1.0;
  a.toValue = @0.45;
  a.duration = 0.6;
  a.autoreverses = YES;
  a.repeatCount = HUGE_VALF;
  [_dot addAnimation:a forKey:@"pulse"];
}

- (void)hide {
  [_dot removeAllAnimations];
  [_bar orderOut:nil];
  [_border orderOut:nil];
  _placedFor.reset();
  _mode.reset();
}

@end
