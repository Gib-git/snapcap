#include "snapcap/actions.h"

namespace snapcap {

std::string_view label(Action a) {
  switch (a) {
    case Action::ShotScreen: return "Screenshot current screen";
    case Action::ShotAllScreens: return "Screenshot all screens";
    case Action::ShotRegion: return "Screenshot region";
    case Action::RecordScreen: return "Record current screen";
    case Action::RecordRegion: return "Record region";
    case Action::StopRecording: return "Stop recording";
    case Action::TogglePause: return "Pause / resume recording";
    case Action::ShowWindow: return "Show SnapCap window";
  }
  return "";
}

std::string_view default_shortcut(Action a) {
  switch (a) {
    case Action::ShotScreen: return "Alt+Shift+1";
    case Action::ShotRegion: return "Alt+Shift+2";
    case Action::RecordScreen: return "Alt+Shift+3";
    case Action::RecordRegion: return "Alt+Shift+4";
    case Action::StopRecording: return "Alt+Shift+0";
    case Action::TogglePause: return "Alt+Shift+9";
    case Action::ShotAllScreens:
    case Action::ShowWindow: return "";
  }
  return "";
}

std::string_view cli_name(Action a) {
  switch (a) {
    case Action::ShotScreen: return "shot-screen";
    case Action::ShotAllScreens: return "shot-all";
    case Action::ShotRegion: return "shot-region";
    case Action::RecordScreen: return "record-screen";
    case Action::RecordRegion: return "record-region";
    case Action::StopRecording: return "stop";
    case Action::TogglePause: return "pause";
    case Action::ShowWindow: return "show";
  }
  return "";
}

std::optional<Action> from_cli_name(std::string_view name) {
  for (Action a : kAllActions) {
    if (cli_name(a) == name) return a;
  }
  return std::nullopt;
}

std::string_view settings_key(Action a) {
  switch (a) {
    case Action::ShotScreen: return "ShotScreen";
    case Action::ShotAllScreens: return "ShotAllScreens";
    case Action::ShotRegion: return "ShotRegion";
    case Action::RecordScreen: return "RecordScreen";
    case Action::RecordRegion: return "RecordRegion";
    case Action::StopRecording: return "StopRecording";
    case Action::TogglePause: return "TogglePause";
    case Action::ShowWindow: return "ShowWindow";
  }
  return "";
}

std::optional<Action> from_settings_key(std::string_view key) {
  for (Action a : kAllActions) {
    if (settings_key(a) == key) return a;
  }
  return std::nullopt;
}

std::optional<Action> action_from_args(std::span<const std::string> args) {
  for (size_t i = 0; i < args.size(); ++i) {
    std::string_view arg = args[i];
    auto next = [&]() -> std::optional<std::string_view> {
      if (i + 1 >= args.size()) return std::nullopt;
      return std::string_view(args[++i]);
    };
    if (arg == "--shot" || arg == "-s") {
      auto v = next();
      if (v == "screen") return Action::ShotScreen;
      if (v == "all") return Action::ShotAllScreens;
      if (v == "region") return Action::ShotRegion;
      return std::nullopt;
    }
    if (arg == "--record" || arg == "-r") {
      auto v = next();
      if (v == "screen") return Action::RecordScreen;
      if (v == "region") return Action::RecordRegion;
      if (v == "stop") return Action::StopRecording;
      return std::nullopt;
    }
    if (arg == "--stop") return Action::StopRecording;
    if (arg == "--pause") return Action::TogglePause;
    if (arg == "--show") return Action::ShowWindow;
    while (arg.starts_with('-')) arg.remove_prefix(1);
    if (auto a = from_cli_name(arg)) return a;
  }
  return std::nullopt;
}

}  // namespace snapcap
