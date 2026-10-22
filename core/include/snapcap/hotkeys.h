// Keyboard shortcuts: parsing, canonical form, display, and conflict detection.
// Registering them with the OS is platform code.
#pragma once

#include <cstdint>
#include <map>
#include <optional>
#include <string>
#include <string_view>

#include "snapcap/actions.h"
#include "snapcap/error.h"
#include "snapcap/os.h"
#include "snapcap/settings.h"

namespace snapcap {

enum Modifier : uint8_t {
  kCtrl = 1 << 0,
  kAlt = 1 << 1,  // Option on macOS
  kShift = 1 << 2,
  kSuper = 1 << 3,  // Cmd on macOS, Win on Windows
};

struct Shortcut {
  uint8_t mods = 0;
  // Canonical key name: "A".."Z", "0".."9", "F1".."F24", "Space", "Enter", "Tab", "Delete",
  // "Insert", "Home", "End", "PageUp", "PageDown", "Up", "Down", "Left", "Right",
  // "PrintScreen", or a punctuation character such as "-" or "/".
  std::string key;

  bool operator==(const Shortcut&) const = default;
  auto operator<=>(const Shortcut&) const = default;

  // Canonical text, e.g. "Ctrl+Alt+Shift+Cmd+K" (modifiers always in this order).
  std::string to_string() const;
};

// Parses "Alt+Shift+2", "shift+alt+2", "Cmd+KeyA", "Super+Digit3", ... Case-insensitive.
Result<Shortcut> parse_shortcut(std::string_view text);

// Actions whose shortcut duplicates another action's, mapping each to the other.
std::map<Action, Action> duplicate_shortcuts(const ShortcutMap& shortcuts);

// The OS's own screenshot shortcuts, which SnapCap should not take over.
std::optional<std::string_view> system_conflict(std::string_view text, Os os = kOs);

// Human-friendly rendering: "⌥⇧2" on macOS, "Alt+Shift+2" elsewhere, "Not set" when empty.
std::string display_shortcut(std::string_view text, Os os = kOs);

// Builds the canonical shortcut text from a key press in a shortcut recorder. Requires a
// modifier other than Shift, except for function keys and Print Screen.
std::optional<std::string> shortcut_from_key_press(uint8_t mods, std::string_view key_name);

}  // namespace snapcap
