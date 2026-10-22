//! Live frame sources used while recording.
//!
//! * macOS: ScreenCaptureKit. It crops to the region in hardware, includes the cursor,
//!   and excludes SnapCap's own windows (recording bar, border).
//! * Windows / Linux Wayland: xcap's streaming recorder (DXGI duplication / PipeWire),
//!   with a paced polling fallback.
//! * Linux X11: paced polling of the region.

use std::sync::atomic::Ordering;
#[cfg(not(target_os = "macos"))]
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crossbeam_channel::Sender;

use super::CaptureTarget;
use crate::error::Result;
use crate::record::clock::RecClock;
use crate::record::frame::Frame;

/// Frames a source had to discard because the encoder was busy (diagnostics).
pub static DROPPED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn send(tx: &Sender<Frame>, frame: Frame) {
    if tx.try_send(frame).is_err() {
        DROPPED.fetch_add(1, Ordering::Relaxed);
    }
}

/// A running capture. Dropping it stops the capture.
pub trait LiveSource {
    fn stop(&mut self);
}

pub struct SourceParams {
    pub target: CaptureTarget,
    /// Size the encoder wants; sources that can scale in hardware deliver this directly.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub output_size: (u32, u32),
    pub fps: u32,
    pub show_cursor: bool,
    pub clock: Arc<RecClock>,
    pub tx: Sender<Frame>,
}

pub fn start(p: SourceParams) -> Result<Box<dyn LiveSource>> {
    DROPPED.store(0, Ordering::Relaxed);
    #[cfg(target_os = "macos")]
    {
        mac::start(p)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let use_stream = cfg!(windows) || std::env::var_os("WAYLAND_DISPLAY").is_some();
        if use_stream {
            match xcap_stream::start(&p) {
                Ok(s) => return Ok(s),
                Err(e) => log::warn!("streaming capture unavailable ({e}); falling back to polling"),
            }
        }
        polling::start(p)
    }
}

/// Background thread with a stop flag, shared by the xcap-based sources.
#[cfg(not(target_os = "macos"))]
struct ThreadSource {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    on_stop: Option<Box<dyn FnOnce()>>,
}

#[cfg(not(target_os = "macos"))]
impl LiveSource for ThreadSource {
    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(f) = self.on_stop.take() {
            f();
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl Drop for ThreadSource {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(not(target_os = "macos"))]
mod polling {
    use super::*;
    use crate::error::Context;
    use crate::record::frame::{Pacer, PixelOrder};
    use std::time::Instant;

    pub fn start(p: SourceParams) -> Result<Box<dyn LiveSource>> {
        let index = p.target.monitor.index;
        let r = p.target.pixel_rect();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<()>>();
        let handle = std::thread::Builder::new()
            .name("snapcap-poll".into())
            .spawn(move || {
                // Monitor handles can't move between threads on Windows; look it up here.
                let monitor = match xcap::Monitor::all().ok().and_then(|v| v.into_iter().nth(index)) {
                    Some(m) => m,
                    None => {
                        let _ = ready_tx.send(Err("display disappeared".into()));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                let mut cursor = p.show_cursor.then(crate::capture::cursor::CursorReader::new);
                let origin = (p.target.monitor.x + r.x as i32, p.target.monitor.y + r.y as i32);
                let mut pacer = Pacer::new(p.fps);
                let interval = pacer.interval();
                let mut next = Instant::now();
                while !stop2.load(Ordering::Relaxed) {
                    next += interval;
                    if let Some(ts) = p.clock.live_now().and_then(|t| pacer.admit(t)) {
                        match monitor.capture_region(r.x, r.y, r.w, r.h) {
                            Ok(img) => {
                                let (w, h) = img.dimensions();
                                let mut data = img.into_raw();
                                if let Some(c) = cursor.as_mut().and_then(|c| c.read()) {
                                    crate::capture::cursor::composite(&mut data, w as usize * 4, w, h, origin, &c);
                                }
                                let frame = Frame {
                                    data,
                                    width: w,
                                    height: h,
                                    stride: w as usize * 4,
                                    order: PixelOrder::Rgba,
                                    ts,
                                };
                                send(&p.tx, frame);
                            }
                            Err(e) => log::warn!("capture failed: {e}"),
                        }
                    }
                    let now = Instant::now();
                    if next > now {
                        std::thread::sleep(next - now);
                    } else {
                        next = now; // fell behind; don't try to catch up with a burst
                    }
                }
            })
            .context("spawn capture thread")?;
        ready_rx.recv().map_err(|e| e.to_string())??;
        Ok(Box::new(ThreadSource { stop, handle: Some(handle), on_stop: None }))
    }
}

#[cfg(not(target_os = "macos"))]
mod xcap_stream {
    use super::*;
    use std::time::Duration;
    use crate::error::Context;
    use crate::record::frame::{Pacer, PixelOrder};

    pub fn start(p: &SourceParams) -> Result<Box<dyn LiveSource>> {
        let (monitor, _) = crate::capture::monitors()?
            .into_iter()
            .find(|(_, i)| i.index == p.target.monitor.index)
            .context("display disappeared")?;
        let (recorder, rx) = monitor.video_recorder().context("video recorder")?;
        recorder.start().context("start video recorder")?;
        let r = p.target.pixel_rect();
        let (clock, tx, fps, show_cursor) = (p.clock.clone(), p.tx.clone(), p.fps, p.show_cursor);
        let origin = (p.target.monitor.x + r.x as i32, p.target.monitor.y + r.y as i32);
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let handle = std::thread::Builder::new()
            .name("snapcap-stream".into())
            .spawn(move || {
                let mut pacer = Pacer::new(fps);
                let mut cursor = show_cursor.then(crate::capture::cursor::CursorReader::new);
                while !stop2.load(Ordering::Relaxed) {
                    let Ok(f) = rx.recv_timeout(Duration::from_millis(100)) else { continue };
                    if f.width < r.x + r.w || f.height < r.y + r.h {
                        continue;
                    }
                    let Some(ts) = clock.live_now().and_then(|t| pacer.admit(t)) else { continue };
                    let mut frame = Frame::cropped(&f.raw, f.width as usize * 4, r.x, r.y, r.w, r.h, PixelOrder::Rgba, ts);
                    if let Some(c) = cursor.as_mut().and_then(|c| c.read()) {
                        crate::capture::cursor::composite(&mut frame.data, frame.stride, r.w, r.h, origin, &c);
                    }
                    send(&tx, frame);
                }
            })
            .context("spawn capture thread")?;
        let rec = recorder.clone();
        Ok(Box::new(ThreadSource {
            stop,
            handle: Some(handle),
            on_stop: Some(Box::new(move || {
                let _ = rec.stop();
            })),
        }))
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use crate::error::Context;
    use crate::record::frame::{Pacer, PixelOrder};
    use screencapturekit::cm::{CMSampleBufferSCExt, SCFrameStatus};
    use screencapturekit::cv::CVPixelBufferLockFlags;
    use screencapturekit::prelude::*;
    use std::sync::Mutex;

    struct ScSource {
        stream: Option<SCStream>,
    }

    impl LiveSource for ScSource {
        fn stop(&mut self) {
            if let Some(s) = self.stream.take() {
                if let Err(e) = s.stop_capture() {
                    log::warn!("stop capture: {e}");
                }
            }
        }
    }

    impl Drop for ScSource {
        fn drop(&mut self) {
            self.stop();
        }
    }

    pub fn start(p: SourceParams) -> Result<Box<dyn LiveSource>> {
        let content = SCShareableContent::get().context("screen recording permission is required")?;
        let display = content
            .displays()
            .into_iter()
            .find(|d| d.display_id() == p.target.monitor.id)
            .context("display disappeared")?;

        // Keep our own UI (recording bar, region border) out of the video.
        let pid = std::process::id() as i32;
        let me: Vec<SCRunningApplication> = if crate::ui::protect_from_capture() {
            content.applications().into_iter().filter(|a| a.process_id() == pid).collect()
        } else {
            Vec::new()
        };
        let refs: Vec<&SCRunningApplication> = me.iter().collect();
        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_excluding_applications(&refs, &[])
            .build()
            .context("content filter")?;

        let r = p.target.pixel_rect();
        let s = p.target.monitor.px_per_unit() as f64;
        // ScreenCaptureKit scales on the GPU, so frames arrive at the encoder's size.
        let (ow, oh) = p.output_size;
        let mut config = SCStreamConfiguration::new()
            .with_width(ow)
            .with_height(oh)
            .with_pixel_format(PixelFormat::BGRA)
            .with_shows_cursor(p.show_cursor)
            .with_fps(p.fps)
            .with_queue_depth(5);
        if p.target.region.is_some() {
            config = config.with_source_rect(CGRect::new(r.x as f64 / s, r.y as f64 / s, r.w as f64 / s, r.h as f64 / s));
        }

        let pacer = Mutex::new(Pacer::new(p.fps));
        let (clock, tx) = (p.clock.clone(), p.tx.clone());
        let handler = move |sample: CMSampleBuffer, kind: SCStreamOutputType| {
            if !matches!(kind, SCStreamOutputType::Screen) {
                return;
            }
            if let Some(status) = sample.frame_status() {
                if status != SCFrameStatus::Complete {
                    return; // idle/blank frames carry no new pixels
                }
            }
            let Some(ts) = clock.live_now().and_then(|t| pacer.lock().unwrap().admit(t)) else { return };
            let Some(pb) = sample.pixel_buffer() else { return };
            let Ok(guard) = pb.lock(CVPixelBufferLockFlags::READ_ONLY) else { return };
            let (w, h, stride) = (guard.width(), guard.height(), guard.bytes_per_row());
            let Some(bytes) = (unsafe { guard.as_slice() }) else { return };
            let len = (stride * h).min(bytes.len());
            let frame = Frame {
                data: bytes[..len].to_vec(),
                width: w as u32,
                height: h as u32,
                stride,
                order: PixelOrder::Bgra,
                ts,
            };
            send(&tx, frame);
        };

        let mut stream = SCStream::new(&filter, &config).context("create capture stream")?;
        stream.add_output_handler(handler, SCStreamOutputType::Screen).context("capture handler")?;
        stream.start_capture().context("start capture (is screen recording allowed?)")?;
        Ok(Box::new(ScSource { stream: Some(stream) }))
    }
}
