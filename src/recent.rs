//! Recent captures shown in the main window, with thumbnails generated off the UI thread.

use std::path::{Path, PathBuf};

use eframe::egui::ColorImage;
use image::RgbaImage;

pub const MAX_RECENT: usize = 12;
const THUMB_WIDTH: u32 = 320;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Png,
    Mp4,
    Gif,
}

impl MediaKind {
    pub fn from_path(p: &Path) -> Option<MediaKind> {
        match p.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "png" => Some(MediaKind::Png),
            "mp4" => Some(MediaKind::Mp4),
            "gif" => Some(MediaKind::Gif),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MediaKind::Png => "PNG",
            MediaKind::Mp4 => "MP4",
            MediaKind::Gif => "GIF",
        }
    }
}

/// Newest SnapCap-style media files in `dir`.
pub fn scan(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            MediaKind::from_path(&path)?;
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.into_iter().take(MAX_RECENT).map(|(_, p)| p).collect()
}

pub fn thumbnail_of(img: &RgbaImage) -> ColorImage {
    let (w, h) = img.dimensions();
    let tw = THUMB_WIDTH.min(w).max(1);
    let th = ((h as u64 * tw as u64) / w.max(1) as u64).max(1) as u32;
    let small = image::imageops::thumbnail(img, tw, th);
    ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], small.as_raw())
}

/// Decodes a PNG row by row, box-averaging straight into a thumbnail, so a 5K
/// screenshot never needs a full-size buffer (keeps the idle footprint small).
fn png_thumbnail(path: &Path) -> Option<ColorImage> {
    let file = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let mut dec = png::Decoder::new(file);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let (w, h) = (reader.info().width as usize, reader.info().height as usize);
    if reader.info().interlaced || w == 0 || h == 0 {
        // Rare for screenshots; fall back to a full decode.
        return Some(thumbnail_of(&image::open(path).ok()?.to_rgba8()));
    }
    let channels = reader.output_color_type().0.samples();
    let tw = (THUMB_WIDTH as usize).min(w).max(1);
    let th = (h * tw / w).max(1);
    let mut out = vec![0u8; tw * th * 4];
    let mut acc = vec![0u32; tw * 4];
    let mut rows_in = 0u32;
    let mut cur_ty = 0usize;
    let flush = |acc: &mut [u32], rows: u32, ty: usize, out: &mut [u8]| {
        for tx in 0..tw {
            let x0 = tx * w / tw;
            let x1 = ((tx + 1) * w / tw).max(x0 + 1);
            let n = rows * (x1 - x0) as u32;
            for c in 0..4 {
                out[(ty * tw + tx) * 4 + c] = (acc[tx * 4 + c] / n.max(1)) as u8;
            }
        }
        acc.iter_mut().for_each(|v| *v = 0);
    };
    for y in 0..h {
        let row = reader.next_row().ok()??;
        let data = row.data();
        let ty = (y * th / h).min(th - 1);
        if ty != cur_ty {
            flush(&mut acc, rows_in, cur_ty, &mut out);
            rows_in = 0;
            cur_ty = ty;
        }
        for tx in 0..tw {
            let x0 = tx * w / tw;
            let x1 = ((tx + 1) * w / tw).max(x0 + 1);
            for x in x0..x1 {
                let px = &data[x * channels..x * channels + channels];
                let (r, g, b, a) = match channels {
                    1 => (px[0], px[0], px[0], 255),
                    2 => (px[0], px[0], px[0], px[1]),
                    3 => (px[0], px[1], px[2], 255),
                    _ => (px[0], px[1], px[2], px[3]),
                };
                let o = tx * 4;
                acc[o] += r as u32;
                acc[o + 1] += g as u32;
                acc[o + 2] += b as u32;
                acc[o + 3] += a as u32;
            }
        }
        rows_in += 1;
    }
    flush(&mut acc, rows_in, cur_ty, &mut out);
    Some(ColorImage::from_rgba_unmultiplied([tw, th], &out))
}

/// Builds a thumbnail for PNG (decoded) and GIF (first frame). Videos get an icon tile instead.
pub fn load_thumbnail(path: &Path) -> Option<ColorImage> {
    match MediaKind::from_path(path)? {
        MediaKind::Png => png_thumbnail(path),
        MediaKind::Gif => {
            let mut opts = gif::DecodeOptions::new();
            opts.set_color_output(gif::ColorOutput::RGBA);
            let mut dec = opts.read_info(std::fs::File::open(path).ok()?).ok()?;
            let (w, h) = (dec.width() as u32, dec.height() as u32);
            let frame = dec.read_next_frame().ok()??;
            let mut canvas = RgbaImage::new(w, h);
            let sub = RgbaImage::from_raw(frame.width as u32, frame.height as u32, frame.buffer.to_vec())?;
            image::imageops::replace(&mut canvas, &sub, frame.left as i64, frame.top as i64);
            Some(thumbnail_of(&canvas))
        }
        MediaKind::Mp4 => None,
    }
}

/// Opens the folder containing `path` and selects the file where the platform supports it.
pub fn reveal(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(dir) = path.parent() {
            let _ = open::that_detached(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_finds_media_newest_first() {
        let dir = std::env::temp_dir().join(format!("snapcap-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.png");
        crate::output::write_png(&a, 2, 2, &[255; 16]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("b.mp4"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let found = scan(&dir);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].file_name().unwrap(), "b.mp4");
        assert!(load_thumbnail(&a).is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn streaming_png_thumbnail_matches_content() {
        let path = std::env::temp_dir().join(format!("snapcap-thumb-{}.png", std::process::id()));
        // 640x400: left half red, right half blue.
        let (w, h) = (640u32, 400u32);
        let px: Vec<u8> = (0..w * h).flat_map(|i| if i % w < w / 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }).collect();
        crate::output::write_png(&path, w, h, &px).unwrap();
        let t = load_thumbnail(&path).unwrap();
        assert_eq!(t.size, [320, 200]);
        assert_eq!(t.pixels[10 * 320 + 5], eframe::egui::Color32::from_rgb(255, 0, 0));
        assert_eq!(t.pixels[199 * 320 + 319], eframe::egui::Color32::from_rgb(0, 0, 255));
        std::fs::remove_file(path).unwrap();
    }
}
