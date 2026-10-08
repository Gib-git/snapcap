//! A captured video frame as it travels from a capture source to an encoder.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelOrder {
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Rgba,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Bgra,
}

impl PixelOrder {
    /// Byte offsets of (R, G, B) inside a 4-byte pixel.
    #[inline]
    pub fn rgb_offsets(self) -> (usize, usize, usize) {
        match self {
            PixelOrder::Rgba => (0, 1, 2),
            PixelOrder::Bgra => (2, 1, 0),
        }
    }
}

pub struct Frame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Bytes per row; may be larger than `width * 4`.
    pub stride: usize,
    pub order: PixelOrder,
    /// Recording-relative presentation time (pauses already removed).
    pub ts: Duration,
}

impl Frame {
    /// Copies a sub-rectangle out of a larger packed 4-byte-per-pixel image.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    #[allow(clippy::too_many_arguments)]
    pub fn cropped(
        src: &[u8],
        src_stride: usize,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        order: PixelOrder,
        ts: Duration,
    ) -> Frame {
        let row = w as usize * 4;
        let mut data = Vec::with_capacity(row * h as usize);
        for r in 0..h as usize {
            let start = (y as usize + r) * src_stride + x as usize * 4;
            data.extend_from_slice(&src[start..start + row]);
        }
        Frame { data, width: w, height: h, stride: row, order, ts }
    }

    /// Returns tightly packed RGBA bytes (used for GIF encoding).
    pub fn to_rgba(&self) -> Vec<u8> {
        let (ri, gi, bi) = self.order.rgb_offsets();
        let w = self.width as usize;
        let mut out = Vec::with_capacity(w * self.height as usize * 4);
        for r in 0..self.height as usize {
            let row = &self.data[r * self.stride..r * self.stride + w * 4];
            for px in row.as_chunks::<4>().0 {
                out.extend_from_slice(&[px[ri], px[gi], px[bi], 255]);
            }
        }
        out
    }
}

/// Rate limiter for capture sources. Timestamps are snapped to a fixed frame grid so
/// capture jitter never produces near-duplicate timestamps; a frame that would land in
/// an already used slot is dropped.
pub struct Pacer {
    interval: Duration,
    last_slot: Option<u64>,
}

impl Pacer {
    pub fn new(fps: u32) -> Self {
        Self { interval: Duration::from_secs_f64(1.0 / fps.max(1) as f64), last_slot: None }
    }

    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Returns the grid-aligned timestamp to use for a frame captured at `t`,
    /// or `None` if the frame should be skipped.
    pub fn admit(&mut self, t: Duration) -> Option<Duration> {
        let slot = (t.as_secs_f64() / self.interval.as_secs_f64()).round() as u64;
        if self.last_slot.is_some_and(|last| slot <= last) {
            return None;
        }
        self.last_slot = Some(slot);
        Some(self.interval.mul_f64(slot as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_copies_the_right_pixels() {
        // 3x2 image, pixel value = index
        let src: Vec<u8> = (0..6u8).flat_map(|i| [i, i, i, 255]).collect();
        let f = Frame::cropped(&src, 12, 1, 1, 2, 1, PixelOrder::Rgba, Duration::ZERO);
        assert_eq!(f.data, vec![4, 4, 4, 255, 5, 5, 5, 255]);
        assert_eq!(f.stride, 8);
    }

    #[test]
    fn pacer_limits_rate_and_snaps_to_grid() {
        let mut p = Pacer::new(10);
        let kept: Vec<Duration> = (0..100).filter_map(|i| p.admit(Duration::from_millis(i * 10))).collect();
        assert!((9..=11).contains(&kept.len()), "kept {}", kept.len());
        // Jittery input: 0, 98, 103, 205 ms -> slots 0, 1, (1 again: dropped), 2.
        let mut p = Pacer::new(10);
        let out: Vec<Option<Duration>> = [0, 98, 103, 205].iter().map(|ms| p.admit(Duration::from_millis(*ms))).collect();
        assert_eq!(out, vec![Some(Duration::ZERO), Some(Duration::from_millis(100)), None, Some(Duration::from_millis(200))]);
    }
}
