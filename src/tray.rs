//! System tray / menu bar icon (StatusNotifierItem over D-Bus on Linux).

use std::collections::BTreeMap;
use std::str::FromStr;

use tray_icon::menu::{accelerator::Accelerator, Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::actions::Action;
use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCmd {
    Action(Action),
    OpenFolder,
    Quit,
}

const MENU_ACTIONS: [Action; 6] = [
    Action::ShotScreen,
    Action::ShotRegion,
    Action::ShotAllScreens,
    Action::RecordScreen,
    Action::RecordRegion,
    Action::StopRecording,
];

impl TrayCmd {
    pub fn from_menu_id(id: &str) -> Option<TrayCmd> {
        match id.strip_prefix("snapcap:")? {
            "open-folder" => Some(TrayCmd::OpenFolder),
            "quit" => Some(TrayCmd::Quit),
            other => Action::from_cli_name(other).map(TrayCmd::Action),
        }
    }
}

fn icon(recording: bool) -> Icon {
    let size = 64;
    Icon::from_rgba(crate::icon::tray_icon(size, recording), size, size).expect("valid icon")
}

fn accelerator(shortcut: &str) -> Option<Accelerator> {
    if shortcut.is_empty() {
        return None;
    }
    Accelerator::from_str(&shortcut.replace("Cmd", "Super")).ok()
}

struct Built {
    icon: TrayIcon,
    items: Vec<(Action, MenuItem)>,
}

fn build(shortcuts: &BTreeMap<Action, String>) -> Result<Built> {
    let menu = Menu::new();
    let mut items = Vec::new();
    let item = |a: Action, text: &str| {
        MenuItem::with_id(format!("snapcap:{}", a.cli_name()), text, true, accelerator(shortcuts.get(&a).map(String::as_str).unwrap_or("")))
    };
    for (i, a) in MENU_ACTIONS.into_iter().enumerate() {
        if i == 3 {
            menu.append(&PredefinedMenuItem::separator())?;
        }
        let text = match a {
            Action::ShotScreen => "Capture Screen",
            Action::ShotRegion => "Capture Region",
            Action::ShotAllScreens => "Capture All Screens",
            Action::RecordScreen => "Record Screen",
            Action::RecordRegion => "Record Region",
            Action::StopRecording => "Stop Recording",
            _ => a.label(),
        };
        let mi = item(a, text);
        if a == Action::StopRecording {
            mi.set_enabled(false);
        }
        menu.append(&mi)?;
        items.push((a, mi));
    }
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id("snapcap:open-folder", "Open Screenshots Folder", true, None))?;
    let show = item(Action::ShowWindow, "Open SnapCap…");
    menu.append(&show)?;
    items.push((Action::ShowWindow, show));
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id("snapcap:quit", "Quit SnapCap", true, None))?;

    let builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("SnapCap")
        .with_icon(icon(false));
    // macOS tints template images to match the menu bar.
    #[cfg(target_os = "macos")]
    let builder = builder.with_icon_templated(icon(false));
    let icon = builder.build()?;
    Ok(Built { icon, items })
}

fn set_recording(b: &Built, recording: bool) {
    // macOS tints the idle (template) icon to match the menu bar; the red one stays red.
    #[cfg(target_os = "macos")]
    let _ = if recording { b.icon.set_icon(Some(icon(true))) } else { b.icon.set_icon_templated(Some(icon(false))) };
    #[cfg(not(target_os = "macos"))]
    let _ = b.icon.set_icon(Some(icon(recording)));
    let _ = b.icon.set_tooltip(Some(if recording { "SnapCap — recording" } else { "SnapCap" }));
    for (a, mi) in &b.items {
        match a {
            Action::StopRecording => mi.set_enabled(recording),
            Action::RecordScreen | Action::RecordRegion => mi.set_enabled(!recording),
            _ => {}
        }
    }
}

fn set_shortcuts(b: &Built, shortcuts: &BTreeMap<Action, String>) {
    for (a, mi) in &b.items {
        let _ = mi.set_accelerator(accelerator(shortcuts.get(a).map(String::as_str).unwrap_or("")));
    }
}

pub struct Tray(Built);

impl Tray {
    /// Must be called on the main thread after the event loop started.
    pub fn new(shortcuts: &BTreeMap<Action, String>) -> Result<Tray> {
        build(shortcuts).map(Tray)
    }
    pub fn set_recording(&self, recording: bool) {
        set_recording(&self.0, recording);
    }
    pub fn set_shortcuts(&self, shortcuts: &BTreeMap<Action, String>) {
        set_shortcuts(&self.0, shortcuts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_back_to_commands() {
        assert_eq!(TrayCmd::from_menu_id("snapcap:quit"), Some(TrayCmd::Quit));
        assert_eq!(TrayCmd::from_menu_id("snapcap:open-folder"), Some(TrayCmd::OpenFolder));
        assert_eq!(TrayCmd::from_menu_id("snapcap:shot-region"), Some(TrayCmd::Action(Action::ShotRegion)));
        assert_eq!(TrayCmd::from_menu_id("other"), None);
    }

    #[test]
    fn default_shortcuts_make_valid_accelerators() {
        for (_, s) in crate::settings::default_shortcuts() {
            if !s.is_empty() {
                assert!(accelerator(&s).is_some(), "{s}");
            }
        }
    }
}
