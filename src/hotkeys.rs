//! Global keyboard shortcuts: parsing, display, conflict detection and live re-registration.

use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

use global_hotkey::hotkey::HotKey;
use eframe::egui;
use global_hotkey::GlobalHotKeyManager;

use crate::actions::Action;

pub struct Hotkeys {
    manager: Option<GlobalHotKeyManager>,
    registered: HashMap<u32, (Action, HotKey)>,
    /// Per-action problem shown next to the shortcut in Settings.
    pub errors: BTreeMap<Action, String>,
    pub unavailable_reason: Option<String>,
}

impl Hotkeys {
    /// Must be called on the main thread (macOS requirement).
    pub fn new() -> Self {
        let (manager, unavailable_reason) = if crate::platform::is_wayland() {
            (None, Some("Wayland does not let apps register global shortcuts.".to_owned()))
        } else {
            match GlobalHotKeyManager::new() {
                Ok(m) => (Some(m), None),
                Err(e) => (None, Some(format!("Global shortcuts are unavailable: {e}"))),
            }
        };
        Self { manager, registered: HashMap::new(), errors: BTreeMap::new(), unavailable_reason }
    }

    pub fn available(&self) -> bool {
        self.manager.is_some()
    }

    pub fn action_for(&self, id: u32) -> Option<Action> {
        self.registered.get(&id).map(|(a, _)| *a)
    }

    /// Unregisters everything (e.g. while the user is typing a new shortcut).
    pub fn suspend(&mut self) {
        if let Some(m) = &self.manager {
            for (_, (_, hk)) in self.registered.drain() {
                let _ = m.unregister(hk);
            }
        }
        self.registered.clear();
    }

    /// Replaces all registrations with `shortcuts`, recording any problems in `errors`.
    pub fn apply(&mut self, shortcuts: &BTreeMap<Action, String>) {
        self.suspend();
        self.errors.clear();
        let dups = duplicates(shortcuts);
        let Some(m) = &self.manager else { return };
        for (&action, text) in shortcuts {
            if text.trim().is_empty() {
                continue;
            }
            if let Some(other) = dups.get(&action) {
                self.errors.insert(action, format!("Also used by “{}”", other.label()));
                continue;
            }
            match parse(text) {
                Ok(hk) => match m.register(hk) {
                    Ok(()) => {
                        self.registered.insert(hk.id(), (action, hk));
                    }
                    Err(e) => {
                        self.errors.insert(action, format!("Unavailable — taken by another app? ({e})"));
                    }
                },
                Err(e) => {
                    self.errors.insert(action, e);
                }
            }
        }
    }
}

pub fn parse(text: &str) -> Result<HotKey, String> {
    let normalized = text.replace("Cmd", "Super").replace("Win", "Super");
    HotKey::from_str(&normalized).map_err(|e| format!("Invalid shortcut: {e}"))
}

/// Actions whose shortcut duplicates another action's (maps each to the other).
pub fn duplicates(shortcuts: &BTreeMap<Action, String>) -> BTreeMap<Action, Action> {
    let mut seen: HashMap<u32, Action> = HashMap::new();
    let mut out = BTreeMap::new();
    for (&action, text) in shortcuts {
        let Ok(hk) = parse(text) else { continue };
        if let Some(&first) = seen.get(&hk.id()) {
            out.insert(action, first);
            out.insert(first, action);
        } else {
            seen.insert(hk.id(), action);
        }
    }
    out
}

/// Shortcuts the operating system already uses for its own screenshot tools.
pub fn system_conflict(text: &str) -> Option<&'static str> {
    let t = text.to_ascii_lowercase();
    if cfg!(target_os = "macos") && matches!(t.as_str(), "cmd+shift+3" | "cmd+shift+4" | "cmd+shift+5" | "cmd+shift+6") {
        return Some("Used by macOS screenshots");
    }
    if cfg!(windows) && (t == "win+shift+s" || t == "printscreen") {
        return Some("Used by Windows Snipping Tool");
    }
    if cfg!(target_os = "linux") && (t == "printscreen" || t == "shift+printscreen" || t == "alt+printscreen") {
        return Some("Used by the desktop's screenshot tool");
    }
    None
}

/// Human-friendly rendering: "⌥⇧2" on macOS, "Alt+Shift+2" elsewhere.
pub fn display(text: &str) -> String {
    if text.is_empty() {
        return "Not set".into();
    }
    if !cfg!(target_os = "macos") {
        return text.to_owned();
    }
    let mut mods = String::new();
    let mut key = "";
    for part in text.split('+') {
        match part {
            "Ctrl" => mods.push('⌃'),
            "Alt" => mods.push('⌥'),
            "Shift" => mods.push('⇧'),
            "Cmd" | "Super" => mods.push('⌘'),
            k => key = k,
        }
    }
    let key = match key {
        "Space" => "Space",
        "Enter" => "↩",
        "Up" => "↑",
        "Down" => "↓",
        "Left" => "←",
        "Right" => "→",
        "Delete" => "⌦",
        "Tab" => "⇥",
        k => k,
    };
    format!("{mods}{key}")
}

/// Builds the canonical shortcut string from a key press captured by egui.
/// Requires at least one modifier, except for function keys and Print Screen.
pub fn from_egui(mods: egui::Modifiers, key: egui::Key) -> Option<String> {
    use egui::Key;
    let name: String = match key {
        Key::Space => "Space".into(),
        Key::Enter => "Enter".into(),
        Key::Tab => "Tab".into(),
        Key::Delete => "Delete".into(),
        Key::Insert => "Insert".into(),
        Key::Home => "Home".into(),
        Key::End => "End".into(),
        Key::PageUp => "PageUp".into(),
        Key::PageDown => "PageDown".into(),
        Key::ArrowUp => "Up".into(),
        Key::ArrowDown => "Down".into(),
        Key::ArrowLeft => "Left".into(),
        Key::ArrowRight => "Right".into(),
        Key::Minus => "-".into(),
        Key::Equals => "=".into(),
        Key::Comma => ",".into(),
        Key::Period => ".".into(),
        Key::Slash => "/".into(),
        Key::Backslash => "\\".into(),
        Key::Semicolon => ";".into(),
        Key::Quote => "'".into(),
        Key::Backtick => "`".into(),
        Key::OpenBracket => "[".into(),
        Key::CloseBracket => "]".into(),
        k => {
            let n = k.name();
            let ok = (n.len() == 1 && n.chars().all(|c| c.is_ascii_alphanumeric()))
                || (n.starts_with('F') && n[1..].parse::<u8>().is_ok());
            if !ok {
                return None;
            }
            n.to_ascii_uppercase()
        }
    };
    let mut parts: Vec<&str> = Vec::new();
    if mods.ctrl {
        parts.push("Ctrl");
    }
    if mods.alt {
        parts.push("Alt");
    }
    if mods.shift {
        parts.push("Shift");
    }
    if cfg!(target_os = "macos") && mods.mac_cmd {
        parts.push("Cmd");
    }
    let is_fn = name.starts_with('F') && name.len() > 1;
    let has_non_shift_mod = parts.iter().any(|p| *p != "Shift");
    if !has_non_shift_mod && !is_fn {
        return None;
    }
    parts.push(&name);
    Some(parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse_and_do_not_collide() {
        let defaults = crate::settings::default_shortcuts();
        for (a, s) in &defaults {
            if !s.is_empty() {
                assert!(parse(s).is_ok(), "{a:?}: {s}");
            }
        }
        assert!(duplicates(&defaults).is_empty());
    }

    #[test]
    fn detects_duplicates() {
        let mut m = BTreeMap::new();
        m.insert(Action::ShotScreen, "Alt+Shift+1".to_owned());
        m.insert(Action::ShotRegion, "shift+alt+1".to_owned());
        m.insert(Action::RecordScreen, "Alt+Shift+3".to_owned());
        let d = duplicates(&m);
        assert_eq!(d.get(&Action::ShotScreen), Some(&Action::ShotRegion));
        assert_eq!(d.get(&Action::ShotRegion), Some(&Action::ShotScreen));
        assert!(!d.contains_key(&Action::RecordScreen));
    }

    #[test]
    fn egui_round_trip() {
        let mods = egui::Modifiers { alt: true, shift: true, ..Default::default() };
        let s = from_egui(mods, egui::Key::Num2).unwrap();
        assert_eq!(s, "Alt+Shift+2");
        assert!(parse(&s).is_ok());
        assert_eq!(from_egui(egui::Modifiers::default(), egui::Key::A), None);
        assert_eq!(from_egui(egui::Modifiers::default(), egui::Key::F9).as_deref(), Some("F9"));
        let ctrl = egui::Modifiers { ctrl: true, ..Default::default() };
        assert!(parse(&from_egui(ctrl, egui::Key::Minus).unwrap()).is_ok());
    }

    #[test]
    fn display_is_platform_flavoured() {
        let d = display("Alt+Shift+2");
        if cfg!(target_os = "macos") {
            assert_eq!(d, "⌥⇧2");
        } else {
            assert_eq!(d, "Alt+Shift+2");
        }
        assert_eq!(display(""), "Not set");
    }
}
