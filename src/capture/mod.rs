//! Monitor enumeration and still screenshots.

// Only Windows and X11 need cursor compositing; the tests run everywhere.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub mod cursor;
pub mod live;

use image::RgbaImage;

use crate::error::{Context, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    pub index: usize,
    /// Platform display id (CGDirectDisplayID on macOS).
    pub id: u32,
    pub name: String,
    /// Position and size in the platform's desktop coordinate space:
    /// points on macOS, physical pixels on Windows/Linux.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub primary: bool,
}

impl MonitorInfo {
    /// Captured image pixels per desktop coordinate unit.
    pub fn px_per_unit(&self) -> f32 {
        if cfg!(target_os = "macos") {
            self.scale.max(1.0)
        } else {
            1.0
        }
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x as f64
            && y >= self.y as f64
            && x < self.x as f64 + self.width as f64
            && y < self.y as f64 + self.height as f64
    }
}

/// A rectangle in captured-image pixels of one monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PxRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl PxRect {
    /// Shrinks to even width/height (required by H.264 4:2:0) and clamps to the image.
    pub fn even_within(self, img_w: u32, img_h: u32) -> PxRect {
        let x = self.x.min(img_w.saturating_sub(2));
        let y = self.y.min(img_h.saturating_sub(2));
        let w = self.w.min(img_w - x).max(2) & !1;
        let h = self.h.min(img_h - y).max(2) & !1;
        PxRect { x, y, w, h }
    }
}

/// What a recording captures.
#[derive(Debug, Clone)]
pub struct CaptureTarget {
    pub monitor: MonitorInfo,
    /// Size of the monitor's captured image in pixels.
    pub image_size: (u32, u32),
    /// `None` = the whole monitor.
    pub region: Option<PxRect>,
}

impl CaptureTarget {
    pub fn pixel_rect(&self) -> PxRect {
        let (w, h) = self.image_size;
        self.region.unwrap_or(PxRect { x: 0, y: 0, w, h }).even_within(w, h)
    }

    /// The captured rectangle in desktop coordinates (for placing the border/bar windows).
    pub fn desktop_rect(&self) -> (f32, f32, f32, f32) {
        let r = self.pixel_rect();
        let s = self.monitor.px_per_unit();
        (
            self.monitor.x as f32 + r.x as f32 / s,
            self.monitor.y as f32 + r.y as f32 / s,
            r.w as f32 / s,
            r.h as f32 / s,
        )
    }
}

pub struct Shot {
    pub monitor: MonitorInfo,
    pub image: RgbaImage,
}

pub fn monitors() -> Result<Vec<(xcap::Monitor, MonitorInfo)>> {
    let list = xcap::Monitor::all().context("cannot list displays")?;
    let mut out = Vec::with_capacity(list.len());
    for (index, m) in list.into_iter().enumerate() {
        let info = MonitorInfo {
            index,
            id: m.id().unwrap_or(index as u32),
            name: m.friendly_name().or_else(|_| m.name()).unwrap_or_else(|_| format!("Display {}", index + 1)),
            x: m.x().unwrap_or(0),
            y: m.y().unwrap_or(0),
            width: m.width().unwrap_or(0),
            height: m.height().unwrap_or(0),
            scale: m.scale_factor().unwrap_or(1.0),
            primary: m.is_primary().unwrap_or(index == 0),
        };
        out.push((m, info));
    }
    if out.is_empty() {
        return Err("no displays found".into());
    }
    Ok(out)
}

pub fn monitor_infos() -> Vec<MonitorInfo> {
    monitors().map(|v| v.into_iter().map(|(_, i)| i).collect()).unwrap_or_default()
}

/// Captures every monitor in parallel.
pub fn capture_all() -> Result<Vec<Shot>> {
    let infos = monitor_infos();
    if infos.is_empty() {
        return Err("no displays found".into());
    }
    let results: Vec<Result<Shot>> = std::thread::scope(|s| {
        let handles: Vec<_> = infos
            .into_iter()
            .map(|info| {
                // xcap monitors hold OS handles that can't cross threads on Windows,
                // so each thread looks its monitor up again.
                s.spawn(move || {
                    let m = xcap::Monitor::all().ok().and_then(|v| v.into_iter().nth(info.index)).context("display disappeared")?;
                    let image = m.capture_image().context(format!("cannot capture {}", info.name))?;
                    Ok(Shot { monitor: info, image })
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("capture thread panicked".into()))).collect()
    });
    let shots: Vec<Shot> = results.into_iter().collect::<Result<_>>()?;
    check_not_blank(&shots)?;
    Ok(shots)
}

pub fn capture_monitor(index: usize) -> Result<Shot> {
    let (m, info) = monitors()?.into_iter().nth(index).context("display disappeared")?;
    let image = m.capture_image().context(format!("cannot capture {}", info.name))?;
    let shot = Shot { monitor: info, image };
    check_not_blank(std::slice::from_ref(&shot))?;
    Ok(shot)
}

/// Without Screen Recording permission macOS returns a wallpaper-only or empty image.
/// A capture with zero size is the clearest signal we get.
fn check_not_blank(shots: &[Shot]) -> Result<()> {
    if shots.iter().any(|s| s.image.width() == 0 || s.image.height() == 0) {
        return Err("the screen could not be captured (check screen recording permission)".into());
    }
    Ok(())
}

/// Composes all monitors into one image following their desktop layout.
pub fn stitch(shots: &[Shot]) -> RgbaImage {
    if shots.len() == 1 {
        return shots[0].image.clone();
    }
    // Lay out at the highest pixel density so nothing is downscaled.
    let s = shots
        .iter()
        .map(|sh| sh.image.width() as f32 / sh.monitor.width.max(1) as f32)
        .fold(1.0f32, f32::max);
    let min_x = shots.iter().map(|sh| sh.monitor.x).min().unwrap_or(0);
    let min_y = shots.iter().map(|sh| sh.monitor.y).min().unwrap_or(0);
    let place = |sh: &Shot| {
        (
            ((sh.monitor.x - min_x) as f32 * s).round() as i64,
            ((sh.monitor.y - min_y) as f32 * s).round() as i64,
        )
    };
    let (mut w, mut h) = (0u32, 0u32);
    for sh in shots {
        let (x, y) = place(sh);
        w = w.max(x as u32 + sh.image.width());
        h = h.max(y as u32 + sh.image.height());
    }
    let mut canvas = RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]));
    for sh in shots {
        let (x, y) = place(sh);
        image::imageops::replace(&mut canvas, &sh.image, x, y);
    }
    canvas
}

/// Index of the monitor under the mouse pointer, falling back to the primary one.
pub fn monitor_under_cursor(monitors: &[MonitorInfo]) -> usize {
    if let Some((x, y)) = crate::platform::cursor_position() {
        if let Some(m) = monitors.iter().find(|m| m.contains(x, y)) {
            return m.index;
        }
    }
    monitors.iter().find(|m| m.primary).map(|m| m.index).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(index: usize, x: i32, y: i32, w: u32, h: u32) -> MonitorInfo {
        MonitorInfo { index, id: 0, name: String::new(), x, y, width: w, height: h, scale: 1.0, primary: index == 0 }
    }

    #[test]
    fn even_rect_is_clamped() {
        let r = PxRect { x: 10, y: 10, w: 101, h: 999 }.even_within(100, 50);
        assert_eq!(r, PxRect { x: 10, y: 10, w: 90, h: 40 });
        let r = PxRect { x: 99, y: 0, w: 5, h: 5 }.even_within(100, 50);
        assert_eq!((r.x, r.w), (98, 2));
    }

    #[test]
    fn stitch_places_monitors_side_by_side() {
        let a = Shot { monitor: mon(0, 0, 0, 4, 2), image: RgbaImage::from_pixel(4, 2, image::Rgba([255, 0, 0, 255])) };
        let b = Shot { monitor: mon(1, 4, 0, 2, 2), image: RgbaImage::from_pixel(2, 2, image::Rgba([0, 255, 0, 255])) };
        let img = stitch(&[a, b]);
        assert_eq!(img.dimensions(), (6, 2));
        assert_eq!(img.get_pixel(5, 1).0, [0, 255, 0, 255]);
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn desktop_rect_maps_pixels_to_points() {
        let mut m = mon(0, 100, 50, 1000, 500);
        m.scale = 2.0;
        let t = CaptureTarget { monitor: m, image_size: (2000, 1000), region: Some(PxRect { x: 200, y: 100, w: 400, h: 200 }) };
        let (x, y, w, h) = t.desktop_rect();
        if cfg!(target_os = "macos") {
            assert_eq!((x, y, w, h), (200.0, 100.0, 200.0, 100.0));
        } else {
            assert_eq!((x, y, w, h), (300.0, 150.0, 400.0, 200.0));
        }
    }
}
