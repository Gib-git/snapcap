//! Animated GIF writer tuned for screen recordings:
//! * each frame only stores the rectangle that changed, with unchanged pixels
//!   transparent, which keeps files small when only part of the screen moves;
//! * every frame gets its own NeuQuant palette (255 colours + transparency);
//! * identical consecutive frames are merged, and delays carry their rounding
//!   error forward so the total duration matches the recording.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::time::Duration;

use color_quant::NeuQuant;
use gif::{DisposalMethod, Encoder, Frame as GifFrame, Repeat};

use super::frame::Frame;
use super::yuv::downscale;
use crate::error::{Context, Result};

const TRANSPARENT: u8 = 255;

pub struct GifWriter {
    enc: Encoder<BufWriter<File>>,
    width: u16,
    height: u16,
    /// Last frame written (scaled RGBA), used to find what changed.
    shown: Option<Vec<u8>>,
    /// Frame waiting for its duration: (start time, scaled RGBA).
    pending: Option<(Duration, Vec<u8>)>,
    /// Centiseconds of animation written so far.
    written_cs: u64,
    scratch: Vec<u8>,
    frames: u64,
}

impl GifWriter {
    pub fn new(path: &Path, width: u32, height: u32) -> Result<Self> {
        let file = File::create(path).context(format!("cannot create {}", path.display()))?;
        let mut enc = Encoder::new(BufWriter::new(file), width as u16, height as u16, &[]).context("gif header")?;
        enc.set_repeat(Repeat::Infinite).context("gif loop")?;
        Ok(Self {
            enc,
            width: width as u16,
            height: height as u16,
            shown: None,
            pending: None,
            written_cs: 0,
            scratch: Vec::new(),
            frames: 0,
        })
    }

    pub fn push(&mut self, frame: &Frame) -> Result<()> {
        let rgba = frame.to_rgba();
        let (w, h) = (self.width as usize, self.height as usize);
        let scaled = if frame.width as usize == w && frame.height as usize == h {
            rgba
        } else {
            downscale(&rgba, frame.width as usize, frame.height as usize, frame.width as usize * 4, w, h, &mut self.scratch);
            std::mem::take(&mut self.scratch)
        };
        match self.pending.take() {
            None => {
                self.written_cs = cs(frame.ts);
                self.pending = Some((frame.ts, scaled));
            }
            // Identical content: just let the previous frame last longer.
            Some(p) if p.1 == scaled => self.pending = Some(p),
            Some((_, prev)) => {
                self.write(prev, frame.ts)?;
                self.pending = Some((frame.ts, scaled));
            }
        }
        Ok(())
    }

    /// Writes `rgba`, lasting until `end`.
    fn write(&mut self, rgba: Vec<u8>, end: Duration) -> Result<()> {
        let (w, h) = (self.width as usize, self.height as usize);
        let delay = cs(end).saturating_sub(self.written_cs).max(2);
        self.written_cs += delay;

        let rect = match &self.shown {
            Some(prev) => changed_rect(prev, &rgba, w, h).unwrap_or((0, 0, 1, 1)),
            None => (0, 0, w, h),
        };
        let (x, y, rw, rh) = rect;

        // Gather the sub-rectangle, marking pixels that did not change.
        let mut sub = Vec::with_capacity(rw * rh * 4);
        let mut keep = Vec::with_capacity(rw * rh);
        for row in y..y + rh {
            for col in x..x + rw {
                let o = (row * w + col) * 4;
                let px = &rgba[o..o + 4];
                let same = self.shown.as_ref().is_some_and(|p| &p[o..o + 4] == px);
                keep.push(same);
                sub.extend_from_slice(px);
            }
        }
        let (palette, indices) = quantize(&sub, &keep);
        let mut f = GifFrame::from_palette_pixels(rw as u16, rh as u16, indices, palette, Some(TRANSPARENT));
        f.left = x as u16;
        f.top = y as u16;
        f.delay = delay.min(u16::MAX as u64) as u16;
        f.dispose = DisposalMethod::Keep;
        self.enc.write_frame(&f).context("gif frame")?;
        self.shown = Some(rgba);
        self.frames += 1;
        Ok(())
    }

    pub fn finish(mut self, end: Duration) -> Result<u64> {
        if let Some((start, prev)) = self.pending.take() {
            self.write(prev, end.max(start + Duration::from_millis(100)))?;
        }
        if self.frames == 0 {
            return Err("no frames were captured".into());
        }
        let mut inner = self.enc.into_inner().context("gif trailer")?;
        std::io::Write::flush(&mut inner).context("gif flush")?;
        Ok(self.frames)
    }
}

/// Maps changed pixels to palette indices (unchanged ones to `TRANSPARENT`).
/// Flat UI regions with at most 255 colours get an exact palette; anything richer
/// goes through NeuQuant, sampling every pixel for small regions.
fn quantize(px: &[u8], keep: &[bool]) -> (Vec<u8>, Vec<u8>) {
    let mut exact: std::collections::HashMap<[u8; 3], u8> = std::collections::HashMap::new();
    let mut palette = Vec::with_capacity(256 * 3);
    let mut fits = true;
    for (p, k) in px.as_chunks::<4>().0.iter().zip(keep) {
        if *k {
            continue;
        }
        let c = [p[0], p[1], p[2]];
        if !exact.contains_key(&c) {
            if exact.len() == 255 {
                fits = false;
                break;
            }
            exact.insert(c, exact.len() as u8);
            palette.extend_from_slice(&c);
        }
    }
    let (mut palette, indices) = if fits {
        let idx = px
            .as_chunks::<4>().0.iter()
            .zip(keep)
            .map(|(p, k)| if *k { TRANSPARENT } else { exact[&[p[0], p[1], p[2]]] })
            .collect();
        (palette, idx)
    } else {
        let changed: Vec<u8> = px.as_chunks::<4>().0.iter().zip(keep).filter(|(_, k)| !**k).flat_map(|(p, _)| p.iter().copied()).collect();
        let samplefac = if changed.len() / 4 < 50_000 { 1 } else { 10 };
        let nq = NeuQuant::new(samplefac, 255, &changed);
        let idx = px
            .as_chunks::<4>().0.iter()
            .zip(keep)
            .map(|(p, k)| if *k { TRANSPARENT } else { nq.index_of(p) as u8 })
            .collect();
        (nq.color_map_rgb(), idx)
    };
    palette.resize(256 * 3, 0);
    (palette, indices)
}

fn cs(d: Duration) -> u64 {
    (d.as_millis() as u64 + 5) / 10
}

/// Bounding box `(x, y, w, h)` of pixels that differ, or `None` if identical.
fn changed_rect(a: &[u8], b: &[u8], w: usize, h: usize) -> Option<(usize, usize, usize, usize)> {
    let row_bytes = w * 4;
    let rows: Vec<usize> = (0..h).filter(|&r| a[r * row_bytes..(r + 1) * row_bytes] != b[r * row_bytes..(r + 1) * row_bytes]).collect();
    let (&top, &bottom) = (rows.first()?, rows.last()?);
    let (mut left, mut right) = (w, 0);
    for &r in &rows {
        let ra = &a[r * row_bytes..(r + 1) * row_bytes];
        let rb = &b[r * row_bytes..(r + 1) * row_bytes];
        if let Some(l) = (0..w).find(|&c| ra[c * 4..c * 4 + 4] != rb[c * 4..c * 4 + 4]) {
            left = left.min(l);
        }
        if let Some(rt) = (0..w).rev().find(|&c| ra[c * 4..c * 4 + 4] != rb[c * 4..c * 4 + 4]) {
            right = right.max(rt);
        }
    }
    Some((left, top, right - left + 1, bottom - top + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::frame::PixelOrder;

    fn frame(w: u32, h: u32, ms: u64, paint: impl Fn(u32, u32) -> [u8; 4]) -> Frame {
        Frame {
            data: (0..w * h).flat_map(|i| paint(i % w, i / w)).collect(),
            width: w,
            height: h,
            stride: w as usize * 4,
            order: PixelOrder::Rgba,
            ts: Duration::from_millis(ms),
        }
    }

    type Decoded = (Vec<u16>, Vec<Vec<u8>>, Vec<(u16, u16, u16, u16)>);

    fn decode(path: &Path) -> Decoded {
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::RGBA);
        let mut dec = opts.read_info(File::open(path).unwrap()).unwrap();
        let (w, h) = (dec.width() as usize, dec.height() as usize);
        let mut canvas = vec![0u8; w * h * 4];
        let (mut delays, mut images, mut rects) = (Vec::new(), Vec::new(), Vec::new());
        while let Some(f) = dec.read_next_frame().unwrap() {
            for row in 0..f.height as usize {
                for col in 0..f.width as usize {
                    let s = (row * f.width as usize + col) * 4;
                    if f.buffer[s + 3] == 0 {
                        continue; // transparent: keep what was there
                    }
                    let d = ((row + f.top as usize) * w + col + f.left as usize) * 4;
                    canvas[d..d + 4].copy_from_slice(&f.buffer[s..s + 4]);
                }
            }
            delays.push(f.delay);
            rects.push((f.left, f.top, f.width, f.height));
            images.push(canvas.clone());
        }
        (delays, images, rects)
    }

    #[test]
    fn delta_frames_reconstruct_and_keep_time() {
        let path = std::env::temp_dir().join(format!("snapcap-test-{}.gif", std::process::id()));
        let red = |_, _| [255, 0, 0, 255];
        let mut g = GifWriter::new(&path, 16, 8).unwrap();
        g.push(&frame(16, 8, 0, red)).unwrap();
        g.push(&frame(16, 8, 104, red)).unwrap(); // duplicate -> merged
        // A blue square appears at (4..8, 2..4).
        let square = |x, y| if (4..8).contains(&x) && (2..4).contains(&y) { [0, 0, 255, 255] } else { [255, 0, 0, 255] };
        for i in 0..7 {
            g.push(&frame(16, 8, 207 + i * 33, square)).unwrap(); // only first is new
        }
        g.push(&frame(16, 8, 433, red)).unwrap();
        assert_eq!(g.finish(Duration::from_millis(1000)).unwrap(), 3);

        let (delays, images, rects) = decode(&path);
        // 0.21 s, 0.22 s, then until 1.0 s — total exactly 100 cs, no drift.
        assert_eq!(delays.iter().map(|d| *d as u32).sum::<u32>(), 100);
        assert_eq!(rects[1], (4, 2, 4, 2), "only the changed square is stored");
        let px = |img: &Vec<u8>, x: usize, y: usize| img[(y * 16 + x) * 4..(y * 16 + x) * 4 + 3].to_vec();
        assert_eq!(px(&images[1], 5, 3), vec![0, 0, 255]);
        assert_eq!(px(&images[1], 0, 0), vec![255, 0, 0]);
        assert_eq!(px(&images[2], 5, 3), vec![255, 0, 0]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn changed_rect_bounds() {
        let a = vec![0u8; 4 * 4 * 4];
        let mut b = a.clone();
        b[(2 * 4 + 1) * 4] = 9;
        b[(3 * 4 + 2) * 4] = 9;
        assert_eq!(changed_rect(&a, &b, 4, 4), Some((1, 2, 2, 2)));
        assert_eq!(changed_rect(&a, &a, 4, 4), None);
    }
}
