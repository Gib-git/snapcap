#include <doctest/doctest.h>

#include "snapcap/hotkeys.h"

using namespace snapcap;

TEST_CASE("defaults parse and do not collide") {
  auto defaults = default_shortcuts();
  for (const auto& [action, text] : defaults) {
    if (!text.empty()) CHECK_MESSAGE(parse_shortcut(text).has_value(), text);
  }
  CHECK(duplicate_shortcuts(defaults).empty());
}

TEST_CASE("parsing is case-insensitive and canonical") {
  auto a = parse_shortcut("shift+alt+2");
  auto b = parse_shortcut("Alt+Shift+Digit2");
  REQUIRE(a);
  REQUIRE(b);
  CHECK(*a == *b);
  CHECK(a->to_string() == "Alt+Shift+2");
  CHECK(parse_shortcut("Cmd+KeyK")->key == "K");
  CHECK(parse_shortcut("Option+F12")->mods == kAlt);
  CHECK(parse_shortcut("Ctrl+Minus")->key == "-");
  CHECK_FALSE(parse_shortcut(""));
  CHECK_FALSE(parse_shortcut("Ctrl+Shift"));
  CHECK_FALSE(parse_shortcut("Ctrl+A+B"));
  CHECK_FALSE(parse_shortcut("Ctrl+Banana"));
  CHECK_FALSE(parse_shortcut("Ctrl++"));
}

TEST_CASE("detects duplicates") {
  ShortcutMap m{{Action::ShotScreen, "Alt+Shift+1"}, {Action::ShotRegion, "shift+alt+1"},
                {Action::RecordScreen, "Alt+Shift+3"}, {Action::ShowWindow, ""}};
  auto d = duplicate_shortcuts(m);
  CHECK(d.at(Action::ShotScreen) == Action::ShotRegion);
  CHECK(d.at(Action::ShotRegion) == Action::ShotScreen);
  CHECK_FALSE(d.contains(Action::RecordScreen));
}

TEST_CASE("system screenshot shortcuts are flagged per OS") {
  CHECK(system_conflict("Cmd+Shift+4", Os::MacOS).has_value());
  CHECK(system_conflict("shift+cmd+5", Os::MacOS).has_value());
  CHECK_FALSE(system_conflict("Alt+Shift+4", Os::MacOS).has_value());
  CHECK(system_conflict("Win+Shift+S", Os::Windows).has_value());
  CHECK(system_conflict("PrintScreen", Os::Windows).has_value());
  CHECK(system_conflict("Shift+PrintScreen", Os::Linux).has_value());
  CHECK_FALSE(system_conflict("Ctrl+PrintScreen", Os::Linux).has_value());
}

TEST_CASE("display is platform flavoured") {
  CHECK(display_shortcut("Alt+Shift+2", Os::MacOS) == "⌥⇧2");
  CHECK(display_shortcut("Ctrl+Cmd+Up", Os::MacOS) == "⌃⌘↑");
  CHECK(display_shortcut("Alt+Shift+2", Os::Windows) == "Alt+Shift+2");
  CHECK(display_shortcut("", Os::MacOS) == "Not set");
}

TEST_CASE("key presses become shortcuts") {
  CHECK(shortcut_from_key_press(kAlt | kShift, "2") == "Alt+Shift+2");
  CHECK(shortcut_from_key_press(0, "a") == std::nullopt);
  CHECK(shortcut_from_key_press(kShift, "a") == std::nullopt);
  CHECK(shortcut_from_key_press(0, "F9") == "F9");
  CHECK(shortcut_from_key_press(0, "PrintScreen") == "PrintScreen");
  auto minus = shortcut_from_key_press(kCtrl, "-");
  REQUIRE(minus);
  CHECK(parse_shortcut(*minus).has_value());
}
