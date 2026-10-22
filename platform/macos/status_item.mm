#import "status_item.h"

#include <array>
#include <map>

#include "keymap.h"
#include "mac_util.h"
#include "snapcap/icon.h"

using namespace snapcap;

namespace {

constexpr std::array<std::pair<Action, const char*>, 6> kMenuActions{{
    {Action::ShotScreen, "Capture Screen"},
    {Action::ShotRegion, "Capture Region"},
    {Action::ShotAllScreens, "Capture All Screens"},
    {Action::RecordScreen, "Record Screen"},
    {Action::RecordRegion, "Record Region"},
    {Action::StopRecording, "Stop Recording"},
}};

NSImage* tray_image(bool recording) {
  // 18 pt, drawn at 2x for Retina menu bars.
  NSImage* image = mac::nsimage(tray_icon(36, recording, /*black=*/true), 0.5);
  // A template image is tinted by macOS to match the menu bar; the red one stays red.
  [image setTemplate:!recording];
  return image;
}

}  // namespace

@implementation SNCStatusItem {
  StatusCommandHandler _handler;
  NSStatusItem* _item;
  std::map<Action, NSMenuItem*> _items;
  BOOL _recording;
}

- (instancetype)initWithHandler:(StatusCommandHandler)handler {
  if ((self = [super init])) {
    _handler = std::move(handler);
    _item = [NSStatusBar.systemStatusBar statusItemWithLength:NSSquareStatusItemLength];
    _item.button.image = tray_image(false);
    _item.button.toolTip = @"SnapCap";
    NSMenu* menu = [[NSMenu alloc] initWithTitle:@"SnapCap"];
    menu.autoenablesItems = NO;
    for (size_t i = 0; i < kMenuActions.size(); ++i) {
      if (i == 3) [menu addItem:NSMenuItem.separatorItem];
      auto [action, title] = kMenuActions[i];
      _items[action] = [self addItemTo:menu title:@(title) command:std::string(cli_name(action))];
    }
    _items[Action::StopRecording].enabled = NO;
    [menu addItem:NSMenuItem.separatorItem];
    [self addItemTo:menu title:@"Open Screenshots Folder" command:"open-folder"];
    _items[Action::ShowWindow] = [self addItemTo:menu title:@"Open SnapCap…" command:"show"];
    [menu addItem:NSMenuItem.separatorItem];
    [self addItemTo:menu title:@"Quit SnapCap" command:"quit"];
    _item.menu = menu;
  }
  return self;
}

- (NSMenuItem*)addItemTo:(NSMenu*)menu title:(NSString*)title command:(const std::string&)command {
  NSMenuItem* item = [[NSMenuItem alloc] initWithTitle:title action:@selector(menuAction:) keyEquivalent:@""];
  item.target = self;
  item.representedObject = mac::ns(command);
  [menu addItem:item];
  return item;
}

- (void)menuAction:(NSMenuItem*)sender {
  _handler(mac::str(sender.representedObject));
}

- (void)setRecording:(BOOL)recording {
  _recording = recording;
  _item.button.image = tray_image(recording);
  _item.button.toolTip = recording ? @"SnapCap — recording" : @"SnapCap";
  _items[Action::StopRecording].enabled = recording;
  _items[Action::RecordScreen].enabled = !recording;
  _items[Action::RecordRegion].enabled = !recording;
}

- (void)setShortcuts:(const ShortcutMap&)shortcuts {
  for (auto& [action, item] : _items) {
    item.keyEquivalent = @"";
    item.keyEquivalentModifierMask = 0;
    auto it = shortcuts.find(action);
    if (it == shortcuts.end()) continue;
    auto sc = parse_shortcut(it->second);
    if (!sc) continue;
    if (auto eq = mac::key_equivalent(*sc)) {
      item.keyEquivalent = eq->first;
      item.keyEquivalentModifierMask = eq->second;
    }
  }
}

@end
