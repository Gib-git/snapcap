// Mapping between SnapCap's canonical key names and macOS virtual key codes.
#pragma once

#import <AppKit/AppKit.h>

#include <cstdint>
#include <optional>
#include <string>

#include "snapcap/hotkeys.h"

namespace snapcap::mac {

std::optional<uint16_t> key_code(const std::string& canonical_key);
std::optional<std::string> key_name(uint16_t key_code);
uint8_t modifiers_from(NSEventModifierFlags flags);
// Carbon modifier mask (cmdKey | optionKey ...) for RegisterEventHotKey.
uint32_t carbon_modifiers(uint8_t mods);
// Key equivalent string and mask for showing a shortcut on an NSMenuItem.
std::optional<std::pair<NSString*, NSEventModifierFlags>> key_equivalent(const Shortcut& sc);

}  // namespace snapcap::mac
