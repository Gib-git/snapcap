//! Mouse cursor compositing for capture backends that leave the cursor out
//! (Windows desktop duplication and X11). macOS gets it from ScreenCaptureKit.

/// Cursor bitmap in premultiplied RGBA, positioned in desktop coordinates.
#[derive(Clone)]
pub struct CursorImage {
    /// Top-left of the bitmap on the desktop (hotspot already subtracted).
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Draws `cursor` onto an RGBA frame whose top-left is at `origin` on the desktop.
pub fn composite(frame: &mut [u8], stride: usize, fw: u32, fh: u32, origin: (i32, i32), cursor: &CursorImage) {
    let dx = cursor.x - origin.0;
    let dy = cursor.y - origin.1;
    for cy in 0..cursor.height as i32 {
        let y = dy + cy;
        if y < 0 || y >= fh as i32 {
            continue;
        }
        for cx in 0..cursor.width as i32 {
            let x = dx + cx;
            if x < 0 || x >= fw as i32 {
                continue;
            }
            let s = ((cy as u32 * cursor.width + cx as u32) * 4) as usize;
            let a = cursor.rgba[s + 3] as u32;
            if a == 0 {
                continue;
            }
            let d = y as usize * stride + x as usize * 4;
            for c in 0..3 {
                // Premultiplied "over": out = src + dst * (1 - a)
                let v = cursor.rgba[s + c] as u32 + frame[d + c] as u32 * (255 - a) / 255;
                frame[d + c] = v.min(255) as u8;
            }
        }
    }
}

/// Reads the current cursor, if it is visible. Cheap enough to call every frame.
#[cfg(windows)]
pub struct CursorReader {
    cache: Option<(usize, CursorImage, (i32, i32))>,
}

#[cfg(windows)]
impl CursorReader {
    pub fn new() -> Self {
        Self { cache: None }
    }

    pub fn read(&mut self) -> Option<CursorImage> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorInfo, CURSORINFO, CURSOR_SHOWING};
        let mut info: CURSORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
        if unsafe { GetCursorInfo(&mut info) } == 0 || info.flags & CURSOR_SHOWING == 0 || info.hCursor.is_null() {
            return None;
        }
        let handle = info.hCursor as usize;
        if self.cache.as_ref().map(|c| c.0) != Some(handle) {
            let (img, hot) = win::bitmap(info.hCursor)?;
            self.cache = Some((handle, img, hot));
        }
        let (_, img, (hx, hy)) = self.cache.as_ref()?;
        let mut img = img.clone();
        img.x = info.ptScreenPos.x - hx;
        img.y = info.ptScreenPos.y - hy;
        Some(img)
    }
}

#[cfg(windows)]
mod win {
    use super::CursorImage;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS, HBITMAP,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetIconInfo, HCURSOR, ICONINFO};

    /// Reads a bitmap as top-down 32-bit BGRA.
    unsafe fn pixels(hbm: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
        unsafe {
            let mut bm: BITMAP = std::mem::zeroed();
            if GetObjectW(hbm, std::mem::size_of::<BITMAP>() as i32, &mut bm as *mut _ as *mut _) == 0 {
                return None;
            }
            let (w, h) = (bm.bmWidth as u32, bm.bmHeight as u32);
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };
            let mut buf = vec![0u8; (w * h * 4) as usize];
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            let lines = GetDIBits(dc, hbm, 0, h, buf.as_mut_ptr() as *mut _, &mut bi, DIB_RGB_COLORS);
            DeleteDC(dc);
            (lines != 0).then_some((w, h, buf))
        }
    }

    /// Converts a cursor handle to premultiplied RGBA plus its hotspot.
    pub fn bitmap(cursor: HCURSOR) -> Option<(CursorImage, (i32, i32))> {
        unsafe {
            let mut ii: ICONINFO = std::mem::zeroed();
            if GetIconInfo(cursor, &mut ii) == 0 {
                return None;
            }
            let mask = pixels(ii.hbmMask);
            let color = if ii.hbmColor.is_null() { None } else { pixels(ii.hbmColor) };
            if !ii.hbmMask.is_null() {
                DeleteObject(ii.hbmMask);
            }
            if !ii.hbmColor.is_null() {
                DeleteObject(ii.hbmColor);
            }
            let hot = (ii.xHotspot as i32, ii.yHotspot as i32);
            let (w, h, rgba) = match (color, mask) {
                (Some((w, h, bgra)), mask) => {
                    let has_alpha = bgra.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
                    let mut out = Vec::with_capacity(bgra.len());
                    for (i, p) in bgra.as_chunks::<4>().0.iter().enumerate() {
                        let a = if has_alpha {
                            p[3]
                        } else {
                            // No alpha channel: the AND mask says which pixels are transparent.
                            match &mask {
                                Some((_, _, m)) if m.get(i * 4).is_some_and(|v| *v != 0) => 0,
                                _ => 255,
                            }
                        };
                        let pm = |v: u8| if has_alpha { v } else { (v as u32 * a as u32 / 255) as u8 };
                        out.extend_from_slice(&[pm(p[2]), pm(p[1]), pm(p[0]), a]);
                    }
                    (w, h, out)
                }
                // Monochrome cursor: the mask holds the AND bitmap on top of the XOR bitmap.
                (None, Some((w, h2, m))) => {
                    let h = h2 / 2;
                    let mut out = Vec::with_capacity((w * h * 4) as usize);
                    for i in 0..(w * h) as usize {
                        let and = m[i * 4] != 0;
                        let xor = m[(i + (w * h) as usize) * 4] != 0;
                        let px = match (and, xor) {
                            (false, false) => [0, 0, 0, 255],
                            (false, true) => [255, 255, 255, 255],
                            (true, false) => [0, 0, 0, 0],
                            // "Invert screen" pixels: approximate with black so they stay visible.
                            (true, true) => [0, 0, 0, 255],
                        };
                        out.extend_from_slice(&px);
                    }
                    (w, h, out)
                }
                _ => return None,
            };
            Some((CursorImage { x: 0, y: 0, width: w, height: h, rgba }, hot))
        }
    }
}

#[cfg(target_os = "linux")]
pub struct CursorReader {
    conn: Option<x11rb::rust_connection::RustConnection>,
}

#[cfg(target_os = "linux")]
impl CursorReader {
    /// Connects to X11 (XFixes). On Wayland the compositor decides about the cursor.
    pub fn new() -> Self {
        use x11rb::protocol::xfixes::ConnectionExt;
        let conn = x11rb::connect(None).ok().and_then(|(c, _)| {
            c.xfixes_query_version(4, 0).ok()?.reply().ok()?;
            Some(c)
        });
        Self { conn }
    }

    pub fn read(&mut self) -> Option<CursorImage> {
        use x11rb::protocol::xfixes::ConnectionExt;
        let c = self.conn.as_ref()?;
        let r = c.xfixes_get_cursor_image().ok()?.reply().ok()?;
        // ARGB, premultiplied, one u32 per pixel.
        let rgba = r
            .cursor_image
            .iter()
            .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8])
            .collect();
        Some(CursorImage {
            x: r.x as i32 - r.xhot as i32,
            y: r.y as i32 - r.yhot as i32,
            width: r.width as u32,
            height: r.height as u32,
            rgba,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composites_premultiplied_and_clips() {
        // 4x4 white frame, 2x2 cursor: one opaque black, one half-transparent black.
        let mut frame = vec![255u8; 4 * 4 * 4];
        let cursor = CursorImage { x: 13, y: 21, width: 2, height: 2, rgba: vec![0, 0, 0, 255, 0, 0, 0, 128, 0, 0, 0, 0, 0, 0, 0, 0] };
        composite(&mut frame, 16, 4, 4, (10, 20), &cursor);
        let px = |f: &[u8], x: usize, y: usize| f[y * 16 + x * 4];
        assert_eq!(px(&frame, 3, 1), 0); // opaque cursor pixel at (13-10, 21-20)
        assert_eq!(px(&frame, 0, 0), 255); // untouched
        // The half-transparent pixel would land at x=4, outside the frame: clipped, no panic.
        let half = CursorImage { x: 10, y: 20, width: 1, height: 1, rgba: vec![0, 0, 0, 128] };
        composite(&mut frame, 16, 4, 4, (10, 20), &half);
        assert_eq!(px(&frame, 0, 0), 127);
    }
}
