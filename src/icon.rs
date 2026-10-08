//! Icons drawn procedurally with anti-aliased signed distance functions, so the
//! app ships no image assets and every icon is crisp at any size.

const VIOLET: [f32; 3] = [0.43, 0.36, 0.99];
const BLUE: [f32; 3] = [0.20, 0.55, 0.98];
const CORAL: [f32; 3] = [1.0, 0.33, 0.37];

fn rounded_box(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hw - r);
    let qy = (py - cy).abs() - (hh - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

fn capsule(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, r: f32) -> f32 {
    let (pax, pay, bax, bay) = (px - ax, py - ay, bx - ax, by - ay);
    let h = ((pax * bax + pay * bay) / (bax * bax + bay * bay)).clamp(0.0, 1.0);
    ((pax - bax * h).powi(2) + (pay - bay * h).powi(2)).sqrt() - r
}

/// Distance to four viewfinder corner brackets inside the unit square.
fn brackets(x: f32, y: f32, inset: f32, len: f32, r: f32) -> f32 {
    let (a, b) = (inset, 1.0 - inset);
    let mut d = f32::MAX;
    for (cx, cy, sx, sy) in [(a, a, 1.0, 1.0), (b, a, -1.0, 1.0), (a, b, 1.0, -1.0), (b, b, -1.0, -1.0)] {
        d = d.min(capsule(x, y, cx, cy, cx + sx * len, cy, r));
        d = d.min(capsule(x, y, cx, cy, cx, cy + sy * len, r));
    }
    d
}

fn coverage(d: f32, px: f32) -> f32 {
    (0.5 - d / px).clamp(0.0, 1.0)
}

fn over(dst: &mut [f32; 4], rgb: [f32; 3], a: f32) {
    let out_a = a + dst[3] * (1.0 - a);
    if out_a <= 0.0 {
        return;
    }
    for i in 0..3 {
        dst[i] = (rgb[i] * a + dst[i] * dst[3] * (1.0 - a)) / out_a;
    }
    dst[3] = out_a;
}

fn render(size: u32, f: impl Fn(f32, f32, f32, &mut [f32; 4])) -> Vec<u8> {
    let px = 1.0 / size as f32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for j in 0..size {
        for i in 0..size {
            let (x, y) = ((i as f32 + 0.5) * px, (j as f32 + 0.5) * px);
            let mut c = [0.0f32; 4];
            f(x, y, px, &mut c);
            out.extend_from_slice(&[
                (c[0] * 255.0).round() as u8,
                (c[1] * 255.0).round() as u8,
                (c[2] * 255.0).round() as u8,
                (c[3] * 255.0).round() as u8,
            ]);
        }
    }
    out
}

/// Full-colour application icon: gradient squircle, white viewfinder, coral record dot.
pub fn app_icon(size: u32) -> Vec<u8> {
    render(size, |x, y, px, c| {
        let body = coverage(rounded_box(x, y, 0.5, 0.5, 0.42, 0.42, 0.19), px);
        if body > 0.0 {
            let t = ((x + y) * 0.5).clamp(0.0, 1.0);
            let g = [0, 1, 2].map(|k| VIOLET[k] * (1.0 - t) + BLUE[k] * t);
            over(c, g, body);
        }
        let frame = coverage(brackets(x, y, 0.25, 0.15, 0.034), px) * body;
        over(c, [1.0, 1.0, 1.0], frame);
        let r = (x - 0.5).hypot(y - 0.5);
        over(c, [1.0, 1.0, 1.0], coverage((r - 0.125).abs() - 0.024, px) * body);
        let dot = coverage(r - 0.085, px);
        over(c, CORAL, dot * body);
    })
}

/// Tray icon. Idle: monochrome viewfinder (used as a macOS template image).
/// Recording: a coral dot with a white stop square.
pub fn tray_icon(size: u32, recording: bool) -> Vec<u8> {
    if recording {
        return render(size, |x, y, px, c| {
            over(c, CORAL, coverage((x - 0.5).hypot(y - 0.5) - 0.44, px));
            over(c, [1.0; 3], coverage(rounded_box(x, y, 0.5, 0.5, 0.15, 0.15, 0.04), px));
        });
    }
    let mono = if cfg!(target_os = "macos") { [0.0; 3] } else { [1.0; 3] };
    render(size, move |x, y, px, c| {
        over(c, mono, coverage(brackets(x, y, 0.12, 0.24, 0.055), px));
        over(c, mono, coverage((x - 0.5).hypot(y - 0.5) - 0.16, px));
    })
}

/// Encodes RGBA as PNG bytes.
pub fn png_bytes(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, size, size);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("png header");
        w.write_image_data(rgba).expect("png data");
    }
    out
}

/// Builds a Windows .ico holding PNG-compressed images (supported since Vista).
pub fn ico_bytes(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| png_bytes(s, &app_icon(s))).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, img) in sizes.iter().zip(&images) {
        let dim = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0, 1, 0, 32, 0]);
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        out.extend_from_slice(&img);
    }
    out
}

/// `snapcap --export-icons <dir>`: writes the icon files the packaging scripts use.
pub fn export(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for s in [16u32, 32, 64, 128, 256, 512, 1024] {
        std::fs::write(dir.join(format!("icon_{s}.png")), png_bytes(s, &app_icon(s)))?;
    }
    std::fs::write(dir.join("icon.png"), png_bytes(512, &app_icon(512)))?;
    std::fs::write(dir.join("icon.ico"), ico_bytes(&[16, 24, 32, 48, 64, 128, 256]))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_have_expected_shape() {
        let icon = app_icon(64);
        assert_eq!(icon.len(), 64 * 64 * 4);
        // Corners are transparent, centre is opaque coral-ish.
        assert_eq!(icon[3], 0);
        let mid = (32 * 64 + 32) * 4;
        assert_eq!(icon[mid + 3], 255);
        assert!(icon[mid] > 200 && icon[mid + 2] < 150);
        let ico = ico_bytes(&[16, 32]);
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 2, 0]);
    }
}
