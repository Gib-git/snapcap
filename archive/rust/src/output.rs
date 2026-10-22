//! Where captures are written and how they are named.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use crate::error::{Context, Result};
use chrono::{DateTime, Local};

use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureKind {
    Screenshot,
    Recording,
}

/// `<Desktop>/screenshots`. The Desktop comes from the OS (XDG user dirs on Linux, which
/// may be localised); when that isn't configured, `~/Desktop` is used.
pub fn default_dir() -> PathBuf {
    dirs::desktop_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Desktop")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("screenshots")
}

pub fn save_dir(settings: &Settings) -> PathBuf {
    settings.save_dir.clone().unwrap_or_else(default_dir)
}

/// e.g. "Screenshot 2026-10-07 at 18.56.12.png". Dots instead of colons keep it valid on Windows.
pub fn file_name(kind: CaptureKind, ext: &str, when: DateTime<Local>) -> String {
    let prefix = match kind {
        CaptureKind::Screenshot => "Screenshot",
        CaptureKind::Recording => "Recording",
    };
    format!("{prefix} {} .{ext}", when.format("%Y-%m-%d at %H.%M.%S"))
        .replace(" .", ".")
}

/// Creates the folder and returns a path that does not exist yet.
pub fn new_path(dir: &Path, kind: CaptureKind, ext: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).context(format!("cannot create {}", dir.display()))?;
    let name = file_name(kind, ext, Local::now());
    Ok(unique(dir.join(name)))
}

fn unique(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let ext = path.extension().unwrap_or_default().to_string_lossy().into_owned();
    (2..)
        .map(|n| path.with_file_name(format!("{stem} ({n}).{ext}")))
        .find(|p| !p.exists())
        .expect("unbounded iterator")
}

/// Writes RGBA8 pixels as PNG. Fast compression keeps large Retina shots well under 100 ms.
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    let file = File::create(path).context(format!("cannot create {}", path.display()))?;
    let mut enc = png::Encoder::new(BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut writer = enc.write_header().context("png header")?;
    writer.write_image_data(rgba).context("png data")?;
    writer.finish().context("png finish")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn names_are_filesystem_safe() {
        let t = Local.with_ymd_and_hms(2026, 10, 7, 18, 56, 12).unwrap();
        assert_eq!(
            file_name(CaptureKind::Screenshot, "png", t),
            "Screenshot 2026-10-07 at 18.56.12.png"
        );
        assert_eq!(
            file_name(CaptureKind::Recording, "mp4", t),
            "Recording 2026-10-07 at 18.56.12.mp4"
        );
    }

    #[test]
    fn creates_folder_and_avoids_collisions() {
        let dir = std::env::temp_dir().join(format!("snapcap-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = new_path(&dir, CaptureKind::Screenshot, "png").unwrap();
        assert!(dir.is_dir());
        std::fs::write(&a, b"x").unwrap();
        let b = unique(a.clone());
        assert_ne!(a, b);
        assert!(b.to_string_lossy().ends_with("(2).png"));
        write_png(&b, 2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        assert!(std::fs::metadata(&b).unwrap().len() > 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn default_dir_is_named_screenshots() {
        let d = default_dir();
        assert_eq!(d.file_name().unwrap(), "screenshots");
        assert!(d.parent().is_some_and(|p| p.ends_with("Desktop") || dirs::desktop_dir().as_deref() == Some(p)));
    }
}
