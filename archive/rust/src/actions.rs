//! Every user-triggerable command. Hotkeys, the tray menu, the CLI and the UI
//! buttons all funnel through this one enum.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Action {
    ShotScreen,
    ShotAllScreens,
    ShotRegion,
    RecordScreen,
    RecordRegion,
    StopRecording,
    TogglePause,
    ShowWindow,
}

impl Action {
    pub const ALL: [Action; 8] = [
        Action::ShotScreen,
        Action::ShotAllScreens,
        Action::ShotRegion,
        Action::RecordScreen,
        Action::RecordRegion,
        Action::StopRecording,
        Action::TogglePause,
        Action::ShowWindow,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::ShotScreen => "Screenshot current screen",
            Action::ShotAllScreens => "Screenshot all screens",
            Action::ShotRegion => "Screenshot region",
            Action::RecordScreen => "Record current screen",
            Action::RecordRegion => "Record region",
            Action::StopRecording => "Stop recording",
            Action::TogglePause => "Pause / resume recording",
            Action::ShowWindow => "Show SnapCap window",
        }
    }

    /// Default shortcut. Alt/Option+Shift+digit avoids the macOS (Cmd+Shift+3/4/5)
    /// and Windows (Win+Shift+S) system screenshot shortcuts.
    pub fn default_shortcut(self) -> &'static str {
        match self {
            Action::ShotScreen => "Alt+Shift+1",
            Action::ShotRegion => "Alt+Shift+2",
            Action::RecordScreen => "Alt+Shift+3",
            Action::RecordRegion => "Alt+Shift+4",
            Action::StopRecording => "Alt+Shift+0",
            Action::TogglePause => "Alt+Shift+9",
            Action::ShotAllScreens | Action::ShowWindow => "",
        }
    }

    /// Name used on the command line (`snapcap --shot region`) and over IPC.
    pub fn cli_name(self) -> &'static str {
        match self {
            Action::ShotScreen => "shot-screen",
            Action::ShotAllScreens => "shot-all",
            Action::ShotRegion => "shot-region",
            Action::RecordScreen => "record-screen",
            Action::RecordRegion => "record-region",
            Action::StopRecording => "stop",
            Action::TogglePause => "pause",
            Action::ShowWindow => "show",
        }
    }

    pub fn from_cli_name(name: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.cli_name() == name)
    }

    /// Parses the CLI forms `--shot screen|all|region`, `--record screen|region|stop`,
    /// `--pause`, `--show` as well as the bare IPC names.
    pub fn from_args(args: &[String]) -> Option<Action> {
        let mut it = args.iter().map(String::as_str);
        while let Some(arg) = it.next() {
            let action = match arg {
                "--shot" | "-s" => match it.next()? {
                    "screen" => Action::ShotScreen,
                    "all" => Action::ShotAllScreens,
                    "region" => Action::ShotRegion,
                    _ => return None,
                },
                "--record" | "-r" => match it.next()? {
                    "screen" => Action::RecordScreen,
                    "region" => Action::RecordRegion,
                    "stop" => Action::StopRecording,
                    _ => return None,
                },
                "--stop" => Action::StopRecording,
                "--pause" => Action::TogglePause,
                "--show" => Action::ShowWindow,
                other => match Action::from_cli_name(other.trim_start_matches("--")) {
                    Some(a) => a,
                    None => continue,
                },
            };
            return Some(action);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parses_cli_forms() {
        assert_eq!(Action::from_args(&args("--shot region")), Some(Action::ShotRegion));
        assert_eq!(Action::from_args(&args("--record stop")), Some(Action::StopRecording));
        assert_eq!(Action::from_args(&args("--show")), Some(Action::ShowWindow));
        assert_eq!(Action::from_args(&args("shot-all")), Some(Action::ShotAllScreens));
        assert_eq!(Action::from_args(&args("--shot nope")), None);
        assert_eq!(Action::from_args(&args("")), None);
    }

    #[test]
    fn cli_names_round_trip() {
        for a in Action::ALL {
            assert_eq!(Action::from_cli_name(a.cli_name()), Some(a));
        }
    }
}
