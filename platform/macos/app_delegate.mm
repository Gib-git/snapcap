#import "app_delegate.h"

#include <memory>

#include "../posix/ipc.h"
#include "mac_host.h"
#include "snapcap/app_controller.h"
#include "snapcap/log.h"

using namespace snapcap;

namespace {

NSMenu* build_main_menu() {
  NSMenu* bar = [[NSMenu alloc] init];

  NSMenuItem* app_item = [[NSMenuItem alloc] init];
  NSMenu* app_menu = [[NSMenu alloc] initWithTitle:@"SnapCap"];
  [app_menu addItemWithTitle:@"About SnapCap" action:@selector(orderFrontStandardAboutPanel:) keyEquivalent:@""];
  [app_menu addItem:NSMenuItem.separatorItem];
  [app_menu addItemWithTitle:@"Hide SnapCap" action:@selector(hide:) keyEquivalent:@"h"];
  [app_menu addItemWithTitle:@"Quit SnapCap" action:@selector(terminate:) keyEquivalent:@"q"];
  app_item.submenu = app_menu;
  [bar addItem:app_item];

  NSMenuItem* edit_item = [[NSMenuItem alloc] init];
  NSMenu* edit = [[NSMenu alloc] initWithTitle:@"Edit"];
  [edit addItemWithTitle:@"Undo" action:@selector(undo:) keyEquivalent:@"z"];
  [edit addItemWithTitle:@"Redo" action:@selector(redo:) keyEquivalent:@"Z"];
  [edit addItem:NSMenuItem.separatorItem];
  [edit addItemWithTitle:@"Cut" action:@selector(cut:) keyEquivalent:@"x"];
  [edit addItemWithTitle:@"Copy" action:@selector(copy:) keyEquivalent:@"c"];
  [edit addItemWithTitle:@"Paste" action:@selector(paste:) keyEquivalent:@"v"];
  [edit addItemWithTitle:@"Select All" action:@selector(selectAll:) keyEquivalent:@"a"];
  edit_item.submenu = edit;
  [bar addItem:edit_item];

  NSMenuItem* window_item = [[NSMenuItem alloc] init];
  NSMenu* window = [[NSMenu alloc] initWithTitle:@"Window"];
  [window addItemWithTitle:@"Close" action:@selector(performClose:) keyEquivalent:@"w"];
  [window addItemWithTitle:@"Minimize" action:@selector(performMiniaturize:) keyEquivalent:@"m"];
  window_item.submenu = window;
  [bar addItem:window_item];
  NSApp.windowsMenu = window;
  return bar;
}

}  // namespace

@implementation SNCAppDelegate {
  std::optional<Action> _startupAction;
  BOOL _background;
  // Lives until the process exits: background work may still post to it during shutdown.
  mac::MacHost* _host;
  std::shared_ptr<AppController> _controller;
  std::unique_ptr<ipc::Server> _ipc;
}

- (instancetype)initWithStartupAction:(std::optional<Action>)action background:(BOOL)background {
  if ((self = [super init])) {
    _startupAction = action;
    _background = background;
  }
  return self;
}

- (void)applicationDidFinishLaunching:(NSNotification*)notification {
  NSApp.mainMenu = build_main_menu();
  _host = new mac::MacHost();
  _controller = AppController::create(*_host, Settings::load());
  _host->attach(_controller);

  // Later launches (`snapcap --shot region`) forward their action here.
  _ipc = std::make_unique<ipc::Server>();
  std::weak_ptr<AppController> weak = _controller;
  auto started = _ipc->start([weak](Action a) {
    dispatch_async(dispatch_get_main_queue(), ^{
      if (auto c = weak.lock()) c->trigger(a);
    });
  });
  if (!started) log::warn("single-instance listener unavailable: {}", started.error().message);

  [NSNotificationCenter.defaultCenter addObserver:self
                                         selector:@selector(screensChanged:)
                                             name:NSApplicationDidChangeScreenParametersNotification
                                           object:nil];
  _controller->start(_startupAction, _background);
}

- (void)screensChanged:(NSNotification*)notification {
  if (_controller) _controller->on_displays_changed();
}

- (BOOL)applicationShouldHandleReopen:(NSApplication*)sender hasVisibleWindows:(BOOL)visible {
  // Opening SnapCap.app again (Finder, Spotlight, Dock) shows the window.
  if (_controller) _controller->trigger(Action::ShowWindow);
  return NO;
}

- (NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication*)sender {
  if (_host == nullptr || _host->termination_allowed()) return NSTerminateNow;
  // Cmd+Q or logout: let the controller finish a recording first; it terminates when done.
  _controller->quit();
  return _host->termination_allowed() ? NSTerminateNow : NSTerminateCancel;
}

- (void)applicationWillTerminate:(NSNotification*)notification {
  if (_ipc) _ipc->stop();
  if (_controller) _controller->settings().save();
}

- (BOOL)applicationSupportsSecureRestorableState:(NSApplication*)app {
  return YES;
}

@end
