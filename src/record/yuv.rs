//! RGB(A) -> I420 (YUV 4:2:0, BT.709 limited range) conversion and box downscaling.
//! Both run row-parallel across the available cores.

use std::num::NonZeroUsize;

use super::frame::PixelOrder;

/// Planar Y, U, V buffer with even dimensions.
pub struct I420 {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl I420 {
    pub fn new(width: usize, height: usize) -> Self {
        debug_assert!(width.is_multiple_of(2) && height.is_multiple_of(2));
        Self { width, height, data: vec![0; width * height * 3 / 2] }
    }

    pub fn planes(&self) -> (&[u8], &[u8], &[u8]) {
        let y = self.width * self.height;
        let c = y / 4;
        (&self.data[..y], &self.data[y..y + c], &self.data[y + c..])
    }
}

impl openh264::formats::YUVSource for I420 {
    fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }
    fn strides(&self) -> (usize, usize, usize) {
        (self.width, self.width / 2, self.width / 2)
    }
    fn y(&self) -> &[u8] {
        self.planes().0
    }
    fn u(&self) -> &[u8] {
        self.planes().1
    }
    fn v(&self) -> &[u8] {
        self.planes().2
    }
}

fn threads() -> usize {
    std::thread::available_parallelism().map(NonZeroUsize::get).unwrap_or(4).clamp(1, 8)
}

#[inline(always)]
fn luma(r: i32, g: i32, b: i32) -> u8 {
    (((47 * r + 157 * g + 16 * b + 128) >> 8) + 16) as u8
}

/// Converts the top-left `dst.width x dst.height` pixels of `src` into `dst`.
pub fn rgb_to_i420(src: &[u8], stride: usize, order: PixelOrder, dst: &mut I420) {
    let (w, h) = (dst.width, dst.height);
    let (ri, gi, bi) = order.rgb_offsets();
    let (y_plane, uv) = dst.data.split_at_mut(w * h);
    let (u_plane, v_plane) = uv.split_at_mut(w * h / 4);

    let pairs = h / 2;
    let per = pairs.div_ceil(threads()).max(1);

    std::thread::scope(|s| {
        let ys = y_plane.chunks_mut(w * 2 * per);
        let us = u_plane.chunks_mut(w / 2 * per);
        let vs = v_plane.chunks_mut(w / 2 * per);
        for (i, ((yc, uc), vc)) in ys.zip(us).zip(vs).enumerate() {
            s.spawn(move || {
                let first = i * per;
                let count = uc.len() / (w / 2);
                for p in 0..count {
                    let row0 = &src[(first + p) * 2 * stride..];
                    let row1 = &src[((first + p) * 2 + 1) * stride..];
                    let (y0, rest) = yc[p * 2 * w..].split_at_mut(w);
                    let y1 = &mut rest[..w];
                    for cx in 0..w / 2 {
                        let o0 = cx * 8;
                        let o1 = o0 + 4;
                        let px = [
                            (row0[o0 + ri] as i32, row0[o0 + gi] as i32, row0[o0 + bi] as i32),
                            (row0[o1 + ri] as i32, row0[o1 + gi] as i32, row0[o1 + bi] as i32),
                            (row1[o0 + ri] as i32, row1[o0 + gi] as i32, row1[o0 + bi] as i32),
                            (row1[o1 + ri] as i32, row1[o1 + gi] as i32, row1[o1 + bi] as i32),
                        ];
                        y0[cx * 2] = luma(px[0].0, px[0].1, px[0].2);
                        y0[cx * 2 + 1] = luma(px[1].0, px[1].1, px[1].2);
                        y1[cx * 2] = luma(px[2].0, px[2].1, px[2].2);
                        y1[cx * 2 + 1] = luma(px[3].0, px[3].1, px[3].2);
                        let r = (px[0].0 + px[1].0 + px[2].0 + px[3].0 + 2) >> 2;
                        let g = (px[0].1 + px[1].1 + px[2].1 + px[3].1 + 2) >> 2;
                        let b = (px[0].2 + px[1].2 + px[2].2 + px[3].2 + 2) >> 2;
                        uc[p * (w / 2) + cx] = (((-26 * r - 86 * g + 112 * b + 128) >> 8) + 128) as u8;
                        vc[p * (w / 2) + cx] = (((112 * r - 102 * g - 10 * b + 128) >> 8) + 128) as u8;
                    }
                }
            });
        }
    });
}

/// Area-averaging downscale of a 4-byte-per-pixel image into a tightly packed buffer
/// (same channel order). Good quality for text, which matters for screen recordings.
pub fn downscale(src: &[u8], sw: usize, sh: usize, stride: usize, dw: usize, dh: usize, out: &mut Vec<u8>) {
    out.resize(dw * dh * 4, 0);
    // Source span per destination pixel in 16.16 fixed point.
    let fx = ((sw << 16) / dw) as u64;
    let fy = ((sh << 16) / dh) as u64;
    let per = dh.div_ceil(threads()).max(1);

    std::thread::scope(|s| {
        for (i, chunk) in out.chunks_mut(dw * 4 * per).enumerate() {
            s.spawn(move || {
                for (ry, row) in chunk.chunks_mut(dw * 4).enumerate() {
                    let dy = i * per + ry;
                    let y0 = ((dy as u64 * fy) >> 16) as usize;
                    let y1 = ((((dy + 1) as u64 * fy) >> 16) as usize).clamp(y0 + 1, sh);
                    for dx in 0..dw {
                        let x0 = ((dx as u64 * fx) >> 16) as usize;
                        let x1 = ((((dx + 1) as u64 * fx) >> 16) as usize).clamp(x0 + 1, sw);
                        let mut acc = [0u32; 4];
                        for y in y0..y1 {
                            let line = &src[y * stride + x0 * 4..y * stride + x1 * 4];
                            for px in line.as_chunks::<4>().0 {
                                acc[0] += px[0] as u32;
                                acc[1] += px[1] as u32;
                                acc[2] += px[2] as u32;
                                acc[3] += px[3] as u32;
                            }
                        }
                        let n = ((y1 - y0) * (x1 - x0)) as u32;
                        let o = dx * 4;
                        for c in 0..4 {
                            row[o + c] = ((acc[c] + n / 2) / n) as u8;
                        }
                    }
                }
            });
        }
    });
}

/// Output size for a capture of `w x h` under an optional height cap. Always even.
pub fn fit_even(w: u32, h: u32, max_height: Option<u32>, max_width: Option<u32>) -> (u32, u32) {
    let mut scale = 1.0f64;
    if let Some(mh) = max_height {
        if h > mh {
            scale = scale.min(mh as f64 / h as f64);
        }
    }
    if let Some(mw) = max_width {
        if w > mw {
            scale = scale.min(mw as f64 / w as f64);
        }
    }
    let ow = ((w as f64 * scale).round() as u32).max(2) & !1;
    let oh = ((h as f64 * scale).round() as u32).max(2) & !1;
    (ow, oh)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Straightforward floating-point BT.709 reference.
    fn reference(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
        let y = 16.0 + 0.1826 * r + 0.6142 * g + 0.0620 * b;
        let u = 128.0 - 0.1006 * r - 0.3386 * g + 0.4392 * b;
        let v = 128.0 + 0.4392 * r - 0.3989 * g - 0.0403 * b;
        (y, u, v)
    }

    #[test]
    fn matches_reference_within_rounding() {
        let colors = [(0, 0, 0), (255, 255, 255), (255, 0, 0), (0, 255, 0), (0, 0, 255), (12, 200, 99)];
        for (r, g, b) in colors {
            let src: Vec<u8> = std::iter::repeat_n([r, g, b, 255], 16).flatten().collect();
            let mut dst = I420::new(4, 4);
            rgb_to_i420(&src, 16, PixelOrder::Rgba, &mut dst);
            let (ey, eu, ev) = reference(r as f64, g as f64, b as f64);
            let (y, u, v) = dst.planes();
            assert!((y[5] as f64 - ey).abs() <= 1.5, "Y for {r},{g},{b}: {} vs {ey}", y[5]);
            assert!((u[1] as f64 - eu).abs() <= 1.5, "U for {r},{g},{b}: {} vs {eu}", u[1]);
            assert!((v[2] as f64 - ev).abs() <= 1.5, "V for {r},{g},{b}: {} vs {ev}", v[2]);
        }
    }

    #[test]
    fn bgra_order_is_respected() {
        let rgba = [255u8, 0, 0, 255].repeat(4);
        let bgra = [0u8, 0, 255, 255].repeat(4);
        let mut a = I420::new(2, 2);
        let mut b = I420::new(2, 2);
        rgb_to_i420(&rgba, 8, PixelOrder::Rgba, &mut a);
        rgb_to_i420(&bgra, 8, PixelOrder::Bgra, &mut b);
        assert_eq!(a.data, b.data);
    }

    #[test]
    fn large_frame_uses_all_rows() {
        let (w, h) = (64, 38);
        let src = vec![255u8; w * h * 4];
        let mut dst = I420::new(w, h);
        rgb_to_i420(&src, w * 4, PixelOrder::Rgba, &mut dst);
        assert!(dst.planes().0.iter().all(|&y| y == 235));
    }

    #[test]
    fn downscale_averages() {
        // 2x2 -> 1x1: black, white, black, white -> mid grey
        let src = [0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255];
        let mut out = Vec::new();
        downscale(&src, 2, 2, 8, 1, 1, &mut out);
        assert_eq!(out, vec![128, 128, 128, 255]);
    }

    #[test]
    fn fit_even_caps_and_rounds() {
        assert_eq!(fit_even(3840, 2160, Some(1080), None), (1920, 1080));
        assert_eq!(fit_even(1001, 501, None, None), (1000, 500));
        assert_eq!(fit_even(1920, 1080, None, Some(960)), (960, 540));
    }
}
