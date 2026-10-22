//! Persistent user settings, stored as JSON in the platform config directory.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::actions::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoFormat {
    Mp4,
    Gif,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Low,
    Medium,
    High,
}

impl Quality {
    /// Bits per pixel per frame used to derive the H.264 target bitrate.
    /// Screen content compresses well, so these are deliberately modest.
    pub fn bits_per_pixel(self) -> f32 {
        match self {
            Quality::Low => 0.05,
            Quality::Medium => 0.09,
            Quality::High => 0.16,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Quality::Low => "Small file",
            Quality::Medium => "Balanced",
            Quality::High => "High quality",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionCap {
    Native,
    P1440,
    P1080,
    P720,
}

impl ResolutionCap {
    pub fn max_height(self) -> Option<u32> {
        match self {
            ResolutionCap::Native => None,
            ResolutionCap::P1440 => Some(1440),
            ResolutionCap::P1080 => Some(1080),
            ResolutionCap::P720 => Some(720),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ResolutionCap::Native => "Native",
            ResolutionCap::P1440 => "1440p",
            ResolutionCap::P1080 => "1080p",
            ResolutionCap::P720 => "720p",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemePref {
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// `None` means the default `~/Desktop/screenshots`.
    pub save_dir: Option<PathBuf>,
    pub video_format: VideoFormat,
    pub fps: u32,
    pub quality: Quality,
    pub resolution: ResolutionCap,
    pub gif_fps: u32,
    pub gif_max_width: u32,
    pub record_mic: bool,
    /// Microphone device name; `None` = system default input.
    pub mic_device: Option<String>,
    pub record_system_audio: bool,
    pub countdown_secs: u32,
    pub show_cursor: bool,
    pub copy_to_clipboard: bool,
    pub open_after_capture: bool,
    pub theme: ThemePref,
    /// Action -> shortcut string such as "Alt+Shift+2". Empty string = disabled.
    pub shortcuts: BTreeMap<Action, String>,
    /// Whether the window was closed to tray before (used to show a one-time hint).
    pub tray_hint_shown: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            save_dir: None,
            video_format: VideoFormat::Mp4,
            fps: 30,
            quality: Quality::Medium,
            // macOS encodes in hardware, so full resolution (up to 4K) is cheap there. Elsewhere
            // 1440p keeps software encoding smooth on 4K/5K displays; Native is one click away.
            resolution: if cfg!(target_os = "macos") { ResolutionCap::Native } else { ResolutionCap::P1440 },
            gif_fps: 15,
            gif_max_width: 960,
            record_mic: false,
            mic_device: None,
            record_system_audio: false,
            countdown_secs: 3,
            show_cursor: true,
            copy_to_clipboard: true,
            open_after_capture: false,
            theme: ThemePref::System,
            shortcuts: default_shortcuts(),
            tray_hint_shown: false,
        }
    }
}

pub fn default_shortcuts() -> BTreeMap<Action, String> {
    Action::ALL
        .into_iter()
        .map(|a| (a, a.default_shortcut().to_owned()))
        .collect()
}

impl Settings {
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("SnapCap").join("settings.json"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        let mut settings: Settings = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("ignoring malformed settings {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        };
        // Actions added in newer versions get their default shortcut.
        for a in Action::ALL {
            settings
                .shortcuts
                .entry(a)
                .or_insert_with(|| a.default_shortcut().to_owned());
        }
        settings.fps = settings.fps.clamp(5, 60);
        settings.gif_fps = settings.gif_fps.clamp(5, 30);
        settings
    }

    pub fn save(&self) {
        let Some(path) = Self::config_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                // Write-then-rename so a crash never leaves a truncated file behind.
                let tmp = path.with_extension("json.tmp");
                if std::fs::write(&tmp, text).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
            Err(e) => log::error!("could not serialize settings: {e}"),
        }
    }

    pub fn shortcut(&self, action: Action) -> &str {
        self.shortcuts.get(&action).map(String::as_str).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let mut s = Settings { fps: 60, ..Settings::default() };
        s.shortcuts.insert(Action::ShotRegion, "Ctrl+Shift+S".into());
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let back: Settings = serde_json::from_str(r#"{"fps": 15}"#).unwrap();
        assert_eq!(back.fps, 15);
        assert_eq!(back.video_format, VideoFormat::Mp4);
        assert_eq!(back.shortcut(Action::ShotRegion), "Alt+Shift+2");
    }
}
