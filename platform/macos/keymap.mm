#include "keymap.h"

#include <Carbon/Carbon.h>

#include <array>
#include <format>

namespace snapcap::mac {
namespace {

struct Key {
  const char* name;
  uint16_t code;
  unichar equivalent;  // for NSMenuItem key equivalents
};

constexpr std::array<Key, 80> kKeys{{
    {"A", kVK_ANSI_A, 'a'}, {"B", kVK_ANSI_B, 'b'}, {"C", kVK_ANSI_C, 'c'}, {"D", kVK_ANSI_D, 'd'},
    {"E", kVK_ANSI_E, 'e'}, {"F", kVK_ANSI_F, 'f'}, {"G", kVK_ANSI_G, 'g'}, {"H", kVK_ANSI_H, 'h'},
    {"I", kVK_ANSI_I, 'i'}, {"J", kVK_ANSI_J, 'j'}, {"K", kVK_ANSI_K, 'k'}, {"L", kVK_ANSI_L, 'l'},
    {"M", kVK_ANSI_M, 'm'}, {"N", kVK_ANSI_N, 'n'}, {"O", kVK_ANSI_O, 'o'}, {"P", kVK_ANSI_P, 'p'},
    {"Q", kVK_ANSI_Q, 'q'}, {"R", kVK_ANSI_R, 'r'}, {"S", kVK_ANSI_S, 's'}, {"T", kVK_ANSI_T, 't'},
    {"U", kVK_ANSI_U, 'u'}, {"V", kVK_ANSI_V, 'v'}, {"W", kVK_ANSI_W, 'w'}, {"X", kVK_ANSI_X, 'x'},
    {"Y", kVK_ANSI_Y, 'y'}, {"Z", kVK_ANSI_Z, 'z'}, {"0", kVK_ANSI_0, '0'}, {"1", kVK_ANSI_1, '1'},
    {"2", kVK_ANSI_2, '2'}, {"3", kVK_ANSI_3, '3'}, {"4", kVK_ANSI_4, '4'}, {"5", kVK_ANSI_5, '5'},
    {"6", kVK_ANSI_6, '6'}, {"7", kVK_ANSI_7, '7'}, {"8", kVK_ANSI_8, '8'}, {"9", kVK_ANSI_9, '9'},
    {"-", kVK_ANSI_Minus, '-'}, {"=", kVK_ANSI_Equal, '='}, {",", kVK_ANSI_Comma, ','},
    {".", kVK_ANSI_Period, '.'}, {"/", kVK_ANSI_Slash, '/'}, {"\\", kVK_ANSI_Backslash, '\\'},
    {";", kVK_ANSI_Semicolon, ';'}, {"'", kVK_ANSI_Quote, '\''}, {"`", kVK_ANSI_Grave, '`'},
    {"[", kVK_ANSI_LeftBracket, '['}, {"]", kVK_ANSI_RightBracket, ']'},
    {"Space", kVK_Space, ' '}, {"Enter", kVK_Return, '\r'}, {"Tab", kVK_Tab, '\t'},
    {"Delete", kVK_ForwardDelete, NSDeleteFunctionKey}, {"Insert", kVK_Help, NSHelpFunctionKey},
    {"Home", kVK_Home, NSHomeFunctionKey}, {"End", kVK_End, NSEndFunctionKey},
    {"PageUp", kVK_PageUp, NSPageUpFunctionKey}, {"PageDown", kVK_PageDown, NSPageDownFunctionKey},
    {"Up", kVK_UpArrow, NSUpArrowFunctionKey}, {"Down", kVK_DownArrow, NSDownArrowFunctionKey},
    {"Left", kVK_LeftArrow, NSLeftArrowFunctionKey}, {"Right", kVK_RightArrow, NSRightArrowFunctionKey},
    {"F1", kVK_F1, NSF1FunctionKey}, {"F2", kVK_F2, NSF2FunctionKey}, {"F3", kVK_F3, NSF3FunctionKey},
    {"F4", kVK_F4, NSF4FunctionKey}, {"F5", kVK_F5, NSF5FunctionKey}, {"F6", kVK_F6, NSF6FunctionKey},
    {"F7", kVK_F7, NSF7FunctionKey}, {"F8", kVK_F8, NSF8FunctionKey}, {"F9", kVK_F9, NSF9FunctionKey},
    {"F10", kVK_F10, NSF10FunctionKey}, {"F11", kVK_F11, NSF11FunctionKey}, {"F12", kVK_F12, NSF12FunctionKey},
    {"F13", kVK_F13, NSF13FunctionKey}, {"F14", kVK_F14, NSF14FunctionKey}, {"F15", kVK_F15, NSF15FunctionKey},
    {"F16", kVK_F16, NSF16FunctionKey}, {"F17", kVK_F17, NSF17FunctionKey}, {"F18", kVK_F18, NSF18FunctionKey},
    {"F19", kVK_F19, NSF19FunctionKey}, {"F20", kVK_F20, NSF20FunctionKey},
}};

}  // namespace

std::optional<uint16_t> key_code(const std::string& name) {
  for (const Key& k : kKeys) {
    if (name == k.name) return k.code;
  }
  return std::nullopt;
}

std::optional<std::string> key_name(uint16_t code) {
  for (const Key& k : kKeys) {
    if (code == k.code) return std::string(k.name);
  }
  return std::nullopt;
}

uint8_t modifiers_from(NSEventModifierFlags flags) {
  uint8_t m = 0;
  if (flags & NSEventModifierFlagControl) m |= kCtrl;
  if (flags & NSEventModifierFlagOption) m |= kAlt;
  if (flags & NSEventModifierFlagShift) m |= kShift;
  if (flags & NSEventModifierFlagCommand) m |= kSuper;
  return m;
}

uint32_t carbon_modifiers(uint8_t mods) {
  uint32_t m = 0;
  if (mods & kCtrl) m |= controlKey;
  if (mods & kAlt) m |= optionKey;
  if (mods & kShift) m |= shiftKey;
  if (mods & kSuper) m |= cmdKey;
  return m;
}

std::optional<std::pair<NSString*, NSEventModifierFlags>> key_equivalent(const Shortcut& sc) {
  for (const Key& k : kKeys) {
    if (sc.key != k.name) continue;
    NSEventModifierFlags flags = 0;
    if (sc.mods & kCtrl) flags |= NSEventModifierFlagControl;
    if (sc.mods & kAlt) flags |= NSEventModifierFlagOption;
    if (sc.mods & kShift) flags |= NSEventModifierFlagShift;
    if (sc.mods & kSuper) flags |= NSEventModifierFlagCommand;
    unichar c = k.equivalent;
    return std::pair{[NSString stringWithCharacters:&c length:1], flags};
  }
  return std::nullopt;
}

}  // namespace snapcap::mac
