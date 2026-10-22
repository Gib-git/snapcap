#include "snapcap/hotkeys.h"

#include <algorithm>
#include <array>
#include <cctype>
#include <format>
#include <utility>

namespace snapcap {
namespace {

std::string lower(std::string_view s) {
  std::string out(s);
  std::ranges::transform(out, out.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
  return out;
}

std::string trim(std::string_view s) {
  while (!s.empty() && std::isspace(static_cast<unsigned char>(s.front()))) s.remove_prefix(1);
  while (!s.empty() && std::isspace(static_cast<unsigned char>(s.back()))) s.remove_suffix(1);
  return std::string(s);
}

std::optional<uint8_t> modifier(std::string_view token) {
  std::string t = lower(token);
  if (t == "ctrl" || t == "control") return kCtrl;
  if (t == "alt" || t == "option" || t == "opt") return kAlt;
  if (t == "shift") return kShift;
  if (t == "cmd" || t == "command" || t == "super" || t == "win" || t == "meta") return kSuper;
  return std::nullopt;
}

constexpr std::array<std::pair<const char*, const char*>, 32> kNamedKeys{{
    {"space", "Space"},       {"enter", "Enter"},
    {"return", "Enter"},      {"tab", "Tab"},
    {"delete", "Delete"},     {"del", "Delete"},
    {"insert", "Insert"},     {"home", "Home"},
    {"end", "End"},           {"pageup", "PageUp"},
    {"pagedown", "PageDown"}, {"up", "Up"},
    {"arrowup", "Up"},        {"down", "Down"},
    {"arrowdown", "Down"},    {"left", "Left"},
    {"arrowleft", "Left"},    {"right", "Right"},
    {"arrowright", "Right"},  {"printscreen", "PrintScreen"},
    {"minus", "-"},           {"equal", "="},
    {"comma", ","},           {"period", "."},
    {"slash", "/"},           {"backslash", "\\"},
    {"semicolon", ";"},       {"quote", "'"},
    {"backquote", "`"},       {"bracketleft", "["},
    {"bracketright", "]"},    {"backtick", "`"},
}};

constexpr std::string_view kPunctuation = "-=,./\\;'`[]";

std::optional<std::string> canonical_key(std::string_view token) {
  std::string t = lower(token);
  if (t.size() == 1) {
    auto c = static_cast<unsigned char>(t[0]);
    if (std::isalnum(c)) return std::string(1, static_cast<char>(std::toupper(c)));
    if (kPunctuation.find(t[0]) != std::string_view::npos) return t;
    return std::nullopt;
  }
  if (t.size() == 4 && t.starts_with("key") && std::isalpha(static_cast<unsigned char>(t[3]))) {
    return std::string(1, static_cast<char>(std::toupper(static_cast<unsigned char>(t[3]))));
  }
  if (t.size() == 6 && t.starts_with("digit") && std::isdigit(static_cast<unsigned char>(t[5]))) {
    return std::string(1, t[5]);
  }
  if (t.size() >= 2 && t.size() <= 3 && t[0] == 'f' &&
      std::all_of(t.begin() + 1, t.end(), [](char c) { return std::isdigit(static_cast<unsigned char>(c)); })) {
    int n = std::stoi(t.substr(1));
    if (n >= 1 && n <= 24) return std::format("F{}", n);
    return std::nullopt;
  }
  for (const auto& [name, canon] : kNamedKeys) {
    if (t == name) return std::string(canon);
  }
  return std::nullopt;
}

bool is_function_key(std::string_view key) {
  return key.size() >= 2 && key[0] == 'F' && std::isdigit(static_cast<unsigned char>(key[1]));
}

}  // namespace

std::string Shortcut::to_string() const {
  std::string out;
  auto add = [&](const char* s) {
    if (!out.empty()) out += '+';
    out += s;
  };
  if (mods & kCtrl) add("Ctrl");
  if (mods & kAlt) add("Alt");
  if (mods & kShift) add("Shift");
  if (mods & kSuper) add(kOs == Os::MacOS ? "Cmd" : "Super");
  add(key.c_str());
  return out;
}

Result<Shortcut> parse_shortcut(std::string_view text) {
  Shortcut sc;
  std::string_view rest = text;
  bool any = false;
  while (true) {
    // "+" itself is not a supported key, so splitting on '+' is unambiguous.
    size_t plus = rest.find('+');
    std::string token = trim(rest.substr(0, plus));
    if (token.empty()) return fail(std::format("Invalid shortcut “{}”", text));
    any = true;
    if (auto m = modifier(token)) {
      sc.mods = static_cast<uint8_t>(sc.mods | *m);
    } else if (!sc.key.empty()) {
      return fail(std::format("Invalid shortcut “{}”: more than one key", text));
    } else if (auto k = canonical_key(token)) {
      sc.key = *k;
    } else {
      return fail(std::format("Invalid shortcut “{}”: unknown key “{}”", text, token));
    }
    if (plus == std::string_view::npos) break;
    rest = rest.substr(plus + 1);
  }
  if (!any || sc.key.empty()) return fail(std::format("Invalid shortcut “{}”: no key", text));
  return sc;
}

std::map<Action, Action> duplicate_shortcuts(const ShortcutMap& shortcuts) {
  std::map<Shortcut, Action> seen;
  std::map<Action, Action> out;
  for (const auto& [action, text] : shortcuts) {
    auto sc = parse_shortcut(text);
    if (!sc) continue;
    auto [it, inserted] = seen.emplace(*sc, action);
    if (!inserted) {
      out[action] = it->second;
      out[it->second] = action;
    }
  }
  return out;
}

std::optional<std::string_view> system_conflict(std::string_view text, Os os) {
  auto sc = parse_shortcut(text);
  if (!sc) return std::nullopt;
  if (os == Os::MacOS && sc->mods == (kSuper | kShift) &&
      (sc->key == "3" || sc->key == "4" || sc->key == "5" || sc->key == "6")) {
    return "Used by macOS screenshots";
  }
  if (os == Os::Windows &&
      ((sc->mods == (kSuper | kShift) && sc->key == "S") || (sc->mods == 0 && sc->key == "PrintScreen"))) {
    return "Used by Windows Snipping Tool";
  }
  if (os == Os::Linux && sc->key == "PrintScreen" && (sc->mods == 0 || sc->mods == kShift || sc->mods == kAlt)) {
    return "Used by the desktop's screenshot tool";
  }
  return std::nullopt;
}

std::string display_shortcut(std::string_view text, Os os) {
  if (trim(text).empty()) return "Not set";
  if (os != Os::MacOS) return std::string(text);
  auto sc = parse_shortcut(text);
  if (!sc) return std::string(text);
  std::string out;
  if (sc->mods & kCtrl) out += "⌃";
  if (sc->mods & kAlt) out += "⌥";
  if (sc->mods & kShift) out += "⇧";
  if (sc->mods & kSuper) out += "⌘";
  static constexpr std::array<std::pair<const char*, const char*>, 7> kSymbols{{
      {"Enter", "↩"}, {"Up", "↑"}, {"Down", "↓"}, {"Left", "←"}, {"Right", "→"}, {"Delete", "⌦"}, {"Tab", "⇥"},
  }};
  for (const auto& [name, symbol] : kSymbols) {
    if (sc->key == name) return out + symbol;
  }
  return out + sc->key;
}

std::optional<std::string> shortcut_from_key_press(uint8_t mods, std::string_view key_name) {
  auto key = canonical_key(key_name);
  if (!key) return std::nullopt;
  bool has_non_shift = (mods & (kCtrl | kAlt | kSuper)) != 0;
  if (!has_non_shift && !is_function_key(*key) && *key != "PrintScreen") return std::nullopt;
  return Shortcut{mods, *key}.to_string();
}

}  // namespace snapcap
