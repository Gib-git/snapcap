// Every user-triggerable command. Hotkeys, the tray menu, the CLI and the UI
// buttons all funnel through this one enum.
#pragma once

#include <array>
#include <optional>
#include <span>
#include <string>
#include <string_view>

namespace snapcap {

enum class Action {
  ShotScreen,
  ShotAllScreens,
  ShotRegion,
  RecordScreen,
  RecordRegion,
  StopRecording,
  TogglePause,
  ShowWindow,
};

inline constexpr std::array<Action, 8> kAllActions = {
    Action::ShotScreen,   Action::ShotAllScreens, Action::ShotRegion,  Action::RecordScreen,
    Action::RecordRegion, Action::StopRecording,  Action::TogglePause, Action::ShowWindow,
};

std::string_view label(Action a);

// Default shortcut. Alt/Option+Shift+digit avoids the macOS (Cmd+Shift+3/4/5)
// and Windows (Win+Shift+S) system screenshot shortcuts.
std::string_view default_shortcut(Action a);

// Name used on the command line (`snapcap --shot region`) and over IPC.
std::string_view cli_name(Action a);
std::optional<Action> from_cli_name(std::string_view name);

// Key used in settings.json (matches the Rust version's serde names, e.g. "ShotRegion").
std::string_view settings_key(Action a);
std::optional<Action> from_settings_key(std::string_view key);

// Parses `--shot screen|all|region`, `--record screen|region|stop`, `--pause`, `--show`
// as well as the bare IPC names.
std::optional<Action> action_from_args(std::span<const std::string> args);

}  // namespace snapcap
