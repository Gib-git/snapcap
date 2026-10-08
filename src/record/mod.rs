//! Recording pipeline: capture source -> bounded channel -> encoder thread -> file.

pub mod audio;
pub mod clock;
pub mod frame;
pub mod gif;
pub mod h264;
pub mod mp4mux;
#[cfg(target_os = "macos")]
pub mod vt;
pub mod yuv;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, RecvTimeoutError};

use crate::capture::live::{self, LiveSource, SourceParams};
use crate::capture::CaptureTarget;
use crate::error::Result;
use crate::settings::{Quality, ResolutionCap, VideoFormat};
use audio::{AudioCapture, AudioSpec};
use clock::RecClock;
use frame::Frame;

#[derive(Debug, Clone)]
pub struct RecordConfig {
    pub target: CaptureTarget,
    pub format: VideoFormat,
    pub fps: u32,
    pub quality: Quality,
    pub resolution: ResolutionCap,
    pub gif_max_width: u32,
    pub audio: AudioSpec,
    pub show_cursor: bool,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RecordingResult {
    pub path: PathBuf,
    pub duration: Duration,
    pub warnings: Vec<String>,
}

pub type DoneCallback = Box<dyn FnOnce(Result<RecordingResult>) + Send>;

/// Encoded size for a capture of `w x h` pixels: honours the user's resolution cap,
/// the GIF width, and OpenH264's 3840x2160 (or 2160x3840 portrait) limit. Always even.
pub fn output_size(cfg: &RecordConfig, w: u32, h: u32) -> (u32, u32) {
    match cfg.format {
        VideoFormat::Gif => yuv::fit_even(w, h, None, Some(cfg.gif_max_width)),
        VideoFormat::Mp4 => {
            let (max_w, max_h) = if w >= h { (3840, 2160) } else { (2160, 3840) };
            let cap_h = cfg.resolution.max_height().map_or(max_h, |c| c.min(max_h));
            yuv::fit_even(w, h, Some(cap_h), Some(max_w))
        }
    }
}

pub struct Recorder {
    clock: Arc<RecClock>,
    stop: Arc<AtomicBool>,
    source: Option<Box<dyn LiveSource>>,
}

impl Recorder {
    /// Starts capturing immediately. `done` is called from the worker thread once the file is finalized.
    pub fn start(cfg: RecordConfig, done: DoneCallback) -> Result<Recorder> {
        let clock = Arc::new(RecClock::new());
        let (tx, rx) = bounded::<Frame>(3);
        let fps = match cfg.format {
            VideoFormat::Mp4 => cfg.fps,
            VideoFormat::Gif => cfg.fps.min(30),
        };
        let r = cfg.target.pixel_rect();
        let out_size = output_size(&cfg, r.w, r.h);
        let source = live::start(SourceParams {
            target: cfg.target.clone(),
            output_size: out_size,
            fps,
            show_cursor: cfg.show_cursor,
            clock: clock.clone(),
            tx,
        })?;

        let stop = Arc::new(AtomicBool::new(false));
        let warnings = Arc::new(Mutex::new(Vec::new()));
        let worker = Worker { cfg, clock: clock.clone(), stop: stop.clone(), rx, warnings: warnings.clone(), fps, out_size };
        std::thread::Builder::new()
            .name("snapcap-encoder".into())
            .spawn(move || done(worker.run()))
            .map_err(|e| format!("spawn encoder: {e}"))?;

        Ok(Recorder { clock, stop, source: Some(source) })
    }

    pub fn elapsed(&self) -> Duration {
        self.clock.now()
    }

    pub fn is_paused(&self) -> bool {
        self.clock.is_paused()
    }

    pub fn toggle_pause(&self) {
        if self.clock.is_paused() {
            self.clock.resume();
        } else {
            self.clock.pause();
        }
    }

    /// Stops capture; the worker drains, finalizes and invokes the done callback.
    pub fn stop(mut self) {
        if let Some(mut s) = self.source.take() {
            s.stop();
        }
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if let Some(mut s) = self.source.take() {
            s.stop();
        }
        self.stop.store(true, Ordering::SeqCst);
    }
}

enum Sink {
    Mp4 { video: Video, mux: mp4mux::Mp4Muxer },
    Gif(gif::GifWriter),
}

/// H.264 encoder: VideoToolbox (hardware) on macOS, OpenH264 everywhere else and as fallback.
enum Video {
    Software { enc: Box<h264::H264>, yuv: yuv::I420, scaled: Vec<u8> },
    #[cfg(target_os = "macos")]
    Hardware(vt::VtEncoder),
}

impl Video {
    fn open(w: u32, h: u32, fps: u32, quality: Quality) -> Result<Video> {
        #[cfg(target_os = "macos")]
        match vt::VtEncoder::new(w, h, fps, h264::bitrate(w, h, fps, quality.bits_per_pixel())) {
            Ok(enc) => return Ok(Video::Hardware(enc)),
            Err(e) => log::warn!("hardware encoder unavailable ({e}); using OpenH264"),
        }
        let enc = h264::H264::new(w, h, fps, quality.bits_per_pixel())?;
        Ok(Video::Software { enc: Box::new(enc), yuv: yuv::I420::new(w as usize, h as usize), scaled: Vec::new() })
    }

    /// Encodes one frame and passes every finished packet to the muxer.
    fn encode(&mut self, frame: Frame, mux: &mut mp4mux::Mp4Muxer) -> Result<()> {
        match self {
            Video::Software { enc, yuv, scaled } => {
                let (w, h) = (yuv.width, yuv.height);
                // Exact size (or only an odd pixel to trim): convert in place, otherwise scale first.
                if (w..=w + 1).contains(&(frame.width as usize)) && (h..=h + 1).contains(&(frame.height as usize)) {
                    yuv::rgb_to_i420(&frame.data, frame.stride, frame.order, yuv);
                } else {
                    yuv::downscale(&frame.data, frame.width as usize, frame.height as usize, frame.stride, w, h, scaled);
                    yuv::rgb_to_i420(scaled, w * 4, frame.order, yuv);
                }
                if let Some(pkt) = enc.encode(yuv, frame.ts.as_millis() as u64)? {
                    mux.push_video(frame.ts, pkt, enc.sps.as_deref(), enc.pps.as_deref())?;
                }
            }
            #[cfg(target_os = "macos")]
            Video::Hardware(enc) => {
                let (w, h) = enc.size();
                let (data, stride) = if (w..=w + 1).contains(&(frame.width as usize)) && (h..=h + 1).contains(&(frame.height as usize)) {
                    (frame.data, frame.stride)
                } else {
                    let mut scaled = Vec::new();
                    yuv::downscale(&frame.data, frame.width as usize, frame.height as usize, frame.stride, w, h, &mut scaled);
                    (scaled, w * 4)
                };
                enc.encode(data, stride, frame.ts)?;
                let ready = enc.drain();
                Self::mux_hw(enc, ready, mux)?;
            }
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn mux_hw(enc: &vt::VtEncoder, packets: Vec<(Duration, h264::Packet)>, mux: &mut mp4mux::Mp4Muxer) -> Result<()> {
        let (sps, pps) = enc.parameter_sets();
        for (ts, pkt) in packets {
            mux.push_video(ts, pkt, sps.as_deref(), pps.as_deref())?;
        }
        Ok(())
    }

    /// Emits any frames still inside the encoder.
    fn flush(&mut self, _mux: &mut mp4mux::Mp4Muxer) -> Result<()> {
        #[cfg(target_os = "macos")]
        if let Video::Hardware(enc) = self {
            let rest = enc.flush();
            Self::mux_hw(enc, rest, _mux)?;
        }
        Ok(())
    }
}

struct Worker {
    cfg: RecordConfig,
    clock: Arc<RecClock>,
    stop: Arc<AtomicBool>,
    rx: Receiver<Frame>,
    warnings: Arc<Mutex<Vec<String>>>,
    fps: u32,
    out_size: (u32, u32),
}

impl Worker {
    fn run(self) -> Result<RecordingResult> {
        let result = self.run_inner();
        if result.is_err() {
            let _ = std::fs::remove_file(&self.cfg.path);
        }
        result
    }

    fn run_inner(&self) -> Result<RecordingResult> {
        let mut audio = if self.cfg.format == VideoFormat::Mp4 && self.cfg.audio.any() {
            let mut w = Vec::new();
            let a = AudioCapture::start(&self.cfg.audio, self.clock.clone(), &mut w);
            if let Err(e) = &a {
                w.push(format!("Recording without audio: {e}"));
            }
            self.warnings.lock().unwrap().extend(w);
            a.ok()
        } else {
            None
        };

        let mut sink: Option<Sink> = None;
        let mut frames_in = 0u64;
        let mut pending_audio: Vec<Vec<u8>> = Vec::new();

        loop {
            let stopping = self.stop.load(Ordering::SeqCst);
            let frame = match self.rx.recv_timeout(Duration::from_millis(15)) {
                Ok(f) => Some(f),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => {
                    if stopping {
                        break;
                    }
                    None
                }
            };
            if let Some(frame) = frame {
                frames_in += 1;
                if sink.is_none() {
                    sink = Some(self.open_sink(audio.is_some())?);
                }
                self.encode(sink.as_mut().unwrap(), frame)?;
            }
            if let Some(a) = audio.as_mut() {
                a.pump(self.clock.now(), &mut |p| pending_audio.push(p));
                if let Some(Sink::Mp4 { mux, .. }) = sink.as_mut() {
                    for p in pending_audio.drain(..) {
                        mux.push_audio(p)?;
                    }
                }
            }
            if stopping && self.rx.is_empty() {
                break;
            }
        }

        let end = self.clock.now();
        log::info!(
            "recording finished: {frames_in} frames in {:.1}s ({:.1} fps), {} dropped by capture",
            end.as_secs_f64(),
            frames_in as f64 / end.as_secs_f64().max(0.001),
            live::DROPPED.load(Ordering::Relaxed),
        );
        let sink = sink.ok_or("no frames were captured — check screen recording permission")?;
        let frames = match sink {
            Sink::Mp4 { mut video, mut mux } => {
                video.flush(&mut mux)?;
                drop(video);
                if let Some(a) = audio.as_mut() {
                    a.finish(end, &mut |p| pending_audio.push(p));
                }
                for p in pending_audio.drain(..) {
                    mux.push_audio(p)?;
                }
                drop(audio);
                mux.finish(end, Duration::from_secs_f64(1.0 / self.fps as f64))?
            }
            Sink::Gif(g) => g.finish(end)?,
        };
        log::info!("wrote {frames} frames to {}", self.cfg.path.display());
        Ok(RecordingResult {
            path: self.cfg.path.clone(),
            duration: end,
            warnings: self.warnings.lock().unwrap().clone(),
        })
    }

    fn open_sink(&self, with_audio: bool) -> Result<Sink> {
        let (w, h) = self.out_size;
        Ok(match self.cfg.format {
            VideoFormat::Mp4 => {
                let video = Video::open(w, h, self.fps, self.cfg.quality)?;
                let mux = mp4mux::Mp4Muxer::new(&self.cfg.path, w, h, with_audio.then_some(audio::AAC_BITRATE));
                Sink::Mp4 { video, mux }
            }
            VideoFormat::Gif => Sink::Gif(gif::GifWriter::new(&self.cfg.path, w, h)?),
        })
    }
    fn encode(&self, sink: &mut Sink, frame: Frame) -> Result<()> {
        match sink {
            Sink::Mp4 { video, mux } => video.encode(frame, mux),
            Sink::Gif(g) => g.push(&frame),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frame::PixelOrder;

    fn cfg(format: VideoFormat, resolution: ResolutionCap) -> RecordConfig {
        let monitor = crate::capture::MonitorInfo {
            index: 0, id: 0, name: String::new(), x: 0, y: 0, width: 100, height: 100, scale: 1.0, primary: true,
        };
        RecordConfig {
            target: CaptureTarget { monitor, image_size: (100, 100), region: None },
            format,
            fps: 30,
            quality: Quality::Medium,
            resolution,
            gif_max_width: 960,
            audio: AudioSpec::default(),
            show_cursor: true,
            path: PathBuf::new(),
        }
    }

    #[test]
    fn output_size_respects_encoder_limits() {
        let native = cfg(VideoFormat::Mp4, ResolutionCap::Native);
        assert_eq!(output_size(&native, 5120, 2880), (3840, 2160));
        assert_eq!(output_size(&native, 2160, 3840), (2160, 3840));
        assert_eq!(output_size(&native, 1921, 1081), (1920, 1080));
        assert_eq!(output_size(&cfg(VideoFormat::Mp4, ResolutionCap::P720), 5120, 2880), (1280, 720));
        assert_eq!(output_size(&cfg(VideoFormat::Gif, ResolutionCap::Native), 5120, 2880), (960, 540));
    }

    /// Synthetic end-to-end check of the MP4 path: frames + tone -> file -> parse it back.
    #[test]
    fn mp4_round_trip() {
        let path = std::env::temp_dir().join(format!("snapcap-test-{}.mp4", std::process::id()));
        let (w, h) = (320u32, 180u32);
        let mut enc = h264::H264::new(w, h, 30, 0.1).unwrap();
        let mut mux = mp4mux::Mp4Muxer::new(&path, w, h, Some(audio::AAC_BITRATE));
        let mut yuvbuf = yuv::I420::new(w as usize, h as usize);
        for i in 0..60u32 {
            let data: Vec<u8> = (0..w * h).flat_map(|p| [((p + i * 7) % 255) as u8, (i * 4) as u8, 90, 255]).collect();
            let ts = Duration::from_millis(i as u64 * 1000 / 30);
            yuv::rgb_to_i420(&data, w as usize * 4, PixelOrder::Rgba, &mut yuvbuf);
            if let Some(pkt) = enc.encode(&yuvbuf, ts.as_millis() as u64).unwrap() {
                mux.push_video(ts, pkt, enc.sps.as_deref(), enc.pps.as_deref()).unwrap();
            }
        }
        // ~2 s of silence-ish AAC frames from the real encoder.
        let fdk = fdk_aac::enc::Encoder::new(fdk_aac::enc::EncoderParams {
            bit_rate: fdk_aac::enc::BitRate::Cbr(audio::AAC_BITRATE),
            sample_rate: 48_000,
            transport: fdk_aac::enc::Transport::Raw,
            channels: fdk_aac::enc::ChannelMode::Stereo,
            audio_object_type: fdk_aac::enc::AudioObjectType::Mpeg4LowComplexity,
        })
        .unwrap();
        let pcm = vec![0i16; 2048];
        let mut out = vec![0u8; 8192];
        for _ in 0..94 {
            let info = fdk.encode(&pcm, &mut out).unwrap();
            if info.output_size > 0 {
                mux.push_audio(out[..info.output_size].to_vec()).unwrap();
            }
        }
        let frames = mux.finish(Duration::from_secs(2), Duration::from_millis(33)).unwrap();
        assert_eq!(frames, 60);

        let f = std::fs::File::open(&path).unwrap();
        let size = f.metadata().unwrap().len();
        let reader = mp4::Mp4Reader::read_header(std::io::BufReader::new(f), size).unwrap();
        assert_eq!(reader.tracks().len(), 2);
        let video = reader.tracks().get(&1).unwrap();
        assert_eq!(video.sample_count(), 60);
        assert_eq!((video.width(), video.height()), (320, 180));
        let dur = reader.duration().as_secs_f64();
        assert!((1.9..=2.1).contains(&dur), "duration {dur}");
        if std::env::var_os("SNAPCAP_KEEP_TEST_OUTPUT").is_none() {
            std::fs::remove_file(path).unwrap();
        }
    }

    /// `cargo test --release encode_throughput -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn encode_throughput() {
        for (w, h) in [(1920usize, 1080usize), (2560, 1440), (3840, 2160)] {
            let mut enc = h264::H264::new(w as u32, h as u32, 30, Quality::Medium.bits_per_pixel()).unwrap();
            let mut yuvbuf = yuv::I420::new(w, h);
            // Scrolling text-like pattern: lots of edges, changes every frame.
            let frames: Vec<Vec<u8>> = (0..4)
                .map(|k| (0..w * h).flat_map(|i| {
                    let (x, y) = (i % w, i / w + k * 7);
                    let v = if (x / 3 + y / 9) % 5 == 0 { 30 } else { 230 };
                    [v, v, (x % 255) as u8, 255]
                }).collect())
                .collect();
            let n = 60;
            let t0 = std::time::Instant::now();
            let mut conv = Duration::ZERO;
            for i in 0..n {
                let c = std::time::Instant::now();
                yuv::rgb_to_i420(&frames[i % 4], w * 4, PixelOrder::Bgra, &mut yuvbuf);
                conv += c.elapsed();
                enc.encode(&yuvbuf, i as u64 * 33).unwrap();
            }
            let total = t0.elapsed().as_secs_f64();
            println!("{w}x{h}: {:.1} fps total, colour conversion {:.1} ms/frame", n as f64 / total, conv.as_secs_f64() * 1000.0 / n as f64);
            #[cfg(target_os = "macos")]
            {
                let mut hw = vt::VtEncoder::new(w as u32, h as u32, 30, h264::bitrate(w as u32, h as u32, 30, 0.09)).unwrap();
                let t0 = std::time::Instant::now();
                let mut got = 0;
                for i in 0..n {
                    hw.encode(frames[i % 4].clone(), w * 4, Duration::from_millis(i as u64 * 33)).unwrap();
                    got += hw.drain().len();
                }
                got += hw.flush().len();
                assert_eq!(got, n);
                println!("{w}x{h}: {:.1} fps with VideoToolbox (incl. frame copy)", n as f64 / t0.elapsed().as_secs_f64());
            }
        }
    }
}
