//! SnapCap — cross-platform screenshots and screen recording.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod actions;
mod app;
mod capture;
mod error;
mod hotkeys;
mod icon;
mod ipc;
mod output;
mod platform;
mod recent;
mod record;
mod settings;
mod tray;
mod ui;

use eframe::egui;

use actions::Action;

const USAGE: &str = "SnapCap — screenshots and screen recording

Usage: snapcap [OPTION]

  --shot screen|all|region     Take a screenshot
  --record screen|region|stop  Start or stop a recording
  --pause                      Pause or resume the current recording
  --show                       Open the SnapCap window
  --background                 Start without opening the window
  --export-icons <dir>         Write the app icons (used by packaging scripts)
  --help, --version

If SnapCap is already running, the action is sent to that instance.";

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("snapcap=info,warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("--help" | "-h") => {
            println!("{USAGE}");
            return Ok(());
        }
        Some("--version" | "-V") => {
            println!("snapcap {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--export-icons") => {
            let dir = args.get(1).map(String::as_str).unwrap_or("assets");
            if let Err(e) = icon::export(std::path::Path::new(dir)) {
                eprintln!("could not export icons: {e}");
                std::process::exit(1);
            }
            return Ok(());
        }
        _ => {}
    }

    let action = Action::from_args(&args);
    let background = args.iter().any(|a| a == "--background");
    if !args.is_empty() && action.is_none() && !background {
        eprintln!("unrecognised arguments: {}\n\n{USAGE}", args.join(" "));
        std::process::exit(2);
    }

    // Single instance: hand the request to the running copy.
    if ipc::send_to_running(action.unwrap_or(Action::ShowWindow)) {
        return Ok(());
    }

    let settings = settings::Settings::load();
    let options = eframe::NativeOptions {
        // The root viewport is an invisible 1×1 controller window parked off-screen;
        // see `app.rs` for why. Real windows are child viewports.
        viewport: egui::ViewportBuilder::default()
            .with_title("SnapCap")
            .with_app_id("snapcap")
            .with_inner_size([1.0, 1.0])
            .with_position([-32000.0, -32000.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_taskbar(false)
            .with_active(false)
            .with_mouse_passthrough(true),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "SnapCap",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, settings, action, background)))),
    )
}
