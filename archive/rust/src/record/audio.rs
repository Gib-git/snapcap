//! Microphone and system-audio capture, mixing and AAC encoding.
//!
//! Every source is converted to 48 kHz stereo f32 in its callback and buffered.
//! The mixer runs on the recording clock: it emits exactly as many samples as
//! recording time has elapsed (minus a small latency), padding silent or stalled
//! sources with zeros. That keeps audio locked to video even when a loopback
//! device delivers nothing during silence.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use fdk_aac::enc::{AudioObjectType, BitRate, ChannelMode, Encoder, EncoderParams, Transport};

use super::clock::RecClock;
use super::mp4mux::{AAC_FRAME, AUDIO_RATE};
use crate::error::{Context, Result};

pub const AAC_BITRATE: u32 = 160_000;
const LATENCY: Duration = Duration::from_millis(120);
/// A source further ahead than this has drifted; drop its oldest samples.
const MAX_BACKLOG_FRAMES: usize = AUDIO_RATE as usize / 2;

#[derive(Debug, Clone, Default)]
pub struct AudioSpec {
    /// `Some(None)` = default microphone, `Some(Some(name))` = a specific one.
    pub mic: Option<Option<String>>,
    pub system: bool,
}

impl AudioSpec {
    pub fn any(&self) -> bool {
        self.mic.is_some() || self.system
    }
}

type Buffer = Arc<Mutex<VecDeque<f32>>>;

pub struct AudioCapture {
    _streams: Vec<cpal::Stream>,
    buffers: Vec<Buffer>,
    encoder: Encoder,
    produced: u64,
    mix: Vec<f32>,
    pcm: Vec<i16>,
    out: Vec<u8>,
}

/// Lists input devices for the settings screen (names only).
pub fn input_device_names() -> Vec<String> {
    let host = cpal::default_host();
    let Ok(devices) = host.input_devices() else { return Vec::new() };
    devices.filter_map(|d| d.description().ok().map(|desc| desc.name().to_owned())).collect()
}

/// Whether system-audio capture is expected to work on this platform.
pub fn system_audio_supported() -> bool {
    cfg!(any(windows, target_os = "macos")) || linux_monitor_device().is_some()
}

#[cfg(target_os = "linux")]
fn linux_monitor_device() -> Option<cpal::Device> {
    for host_id in cpal::available_hosts() {
        let Ok(host) = cpal::host_from_id(host_id) else { continue };
        let Ok(devices) = host.input_devices() else { continue };
        for d in devices {
            if let Ok(desc) = d.description() {
                if desc.name().to_lowercase().contains("monitor") {
                    return Some(d);
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn linux_monitor_device() -> Option<cpal::Device> {
    None
}

fn find_input(name: &Option<String>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = name {
        if let Ok(mut devices) = host.input_devices() {
            if let Some(d) = devices.find(|d| d.description().map(|x| x.name() == name).unwrap_or(false)) {
                return Some(d);
            }
        }
        log::warn!("microphone '{name}' not found, using default");
    }
    host.default_input_device()
}

fn system_device() -> Option<cpal::Device> {
    if cfg!(target_os = "linux") {
        linux_monitor_device()
    } else {
        // Windows (WASAPI loopback) and macOS 14.2+ (process tap) record an output
        // device when it is opened as an input stream.
        cpal::default_host().default_output_device()
    }
}

impl AudioCapture {
    /// Starts the requested sources. Sources that fail are reported in `warnings`.
    pub fn start(spec: &AudioSpec, clock: Arc<RecClock>, warnings: &mut Vec<String>) -> Result<Self> {
        let mut streams = Vec::new();
        let mut buffers = Vec::new();

        if let Some(name) = &spec.mic {
            match find_input(name).context("no microphone found").and_then(|d| open(&d, false, clock.clone())) {
                Ok((s, b)) => {
                    streams.push(s);
                    buffers.push(b);
                }
                Err(e) => warnings.push(format!("Microphone unavailable: {e}")),
            }
        }
        if spec.system {
            match system_device().context("no system audio device found").and_then(|d| open(&d, true, clock.clone())) {
                Ok((s, b)) => {
                    streams.push(s);
                    buffers.push(b);
                }
                Err(e) => warnings.push(format!("System audio unavailable: {e}")),
            }
        }
        if buffers.is_empty() {
            return Err("no audio source could be opened".into());
        }

        let encoder = Encoder::new(EncoderParams {
            bit_rate: BitRate::Cbr(AAC_BITRATE),
            sample_rate: AUDIO_RATE,
            transport: Transport::Raw,
            channels: ChannelMode::Stereo,
            audio_object_type: AudioObjectType::Mpeg4LowComplexity,
        })
        .map_err(|e| format!("AAC encoder: {e:?}"))?;

        Ok(Self {
            _streams: streams,
            buffers,
            encoder,
            produced: 0,
            mix: vec![0.0; AAC_FRAME as usize * 2],
            pcm: vec![0; AAC_FRAME as usize * 2],
            out: vec![0; 8192],
        })
    }

    /// Mixes and encodes everything due by recording time `now`, passing each AAC frame to `sink`.
    pub fn pump(&mut self, now: Duration, sink: &mut impl FnMut(Vec<u8>)) {
        let due = now.saturating_sub(LATENCY);
        self.produce_until(due, sink);
    }

    /// Flushes all audio up to the final recording time.
    pub fn finish(&mut self, end: Duration, sink: &mut impl FnMut(Vec<u8>)) {
        self.produce_until(end, sink);
    }

    fn produce_until(&mut self, t: Duration, sink: &mut impl FnMut(Vec<u8>)) {
        let target = (t.as_secs_f64() * AUDIO_RATE as f64) as u64;
        let n = AAC_FRAME as usize;
        while self.produced + n as u64 <= target {
            self.mix.iter_mut().for_each(|s| *s = 0.0);
            for buf in &self.buffers {
                let mut b = buf.lock().unwrap();
                let excess = (b.len() / 2).saturating_sub(MAX_BACKLOG_FRAMES);
                if excess > 0 {
                    b.drain(..excess * 2);
                }
                let take = b.len().min(n * 2);
                for (m, s) in self.mix.iter_mut().zip(b.drain(..take)) {
                    *m += s;
                }
            }
            for (p, m) in self.pcm.iter_mut().zip(&self.mix) {
                *p = (m.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            }
            self.encode_frame(sink);
            self.produced += n as u64;
        }
    }

    fn encode_frame(&mut self, sink: &mut impl FnMut(Vec<u8>)) {
        let mut input: &[i16] = &self.pcm;
        while !input.is_empty() {
            match self.encoder.encode(input, &mut self.out) {
                Ok(info) => {
                    if info.output_size > 0 {
                        sink(self.out[..info.output_size].to_vec());
                    }
                    if info.input_consumed == 0 {
                        break;
                    }
                    input = &input[info.input_consumed.min(input.len())..];
                }
                Err(e) => {
                    log::error!("AAC encode failed: {e:?}");
                    break;
                }
            }
        }
    }
}

fn open(device: &cpal::Device, loopback: bool, clock: Arc<RecClock>) -> Result<(cpal::Stream, Buffer)> {
    let supported = if loopback && !cfg!(target_os = "linux") {
        device.default_output_config().context("output config")?
    } else {
        device.default_input_config().context("input config")?
    };
    let config: cpal::StreamConfig = supported.config();
    let channels = config.channels as usize;
    let buffer: Buffer = Arc::new(Mutex::new(VecDeque::with_capacity(AUDIO_RATE as usize * 2)));
    let err = |e: cpal::Error| log::warn!("audio stream error: {e}");

    macro_rules! build {
        ($t:ty) => {{
            let mut conv = Converter::new(config.sample_rate, channels, buffer.clone(), clock);
            device.build_input_stream::<$t, _, _>(
                config.clone(),
                move |data: &[$t], _| conv.push(data.iter().map(|s| cpal::Sample::to_sample::<f32>(*s))),
                err,
                None,
            )
        }};
    }
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::I32 => build!(i32),
        cpal::SampleFormat::U16 => build!(u16),
        cpal::SampleFormat::U8 => build!(u8),
        cpal::SampleFormat::F64 => build!(f64),
        other => return Err(format!("unsupported sample format {other:?}").into()),
    }
    .context("open audio stream")?;
    stream.play().context("start audio stream")?;
    Ok((stream, buffer))
}

/// Per-stream channel mapping and linear resampling to 48 kHz stereo.
struct Converter {
    channels: usize,
    step: f64,
    pos: f64,
    prev: [f32; 2],
    frame: Vec<f32>,
    scratch: Vec<f32>,
    buffer: Buffer,
    clock: Arc<RecClock>,
}

impl Converter {
    fn new(rate: u32, channels: usize, buffer: Buffer, clock: Arc<RecClock>) -> Self {
        Self {
            channels: channels.max(1),
            step: rate as f64 / AUDIO_RATE as f64,
            pos: 0.0,
            prev: [0.0; 2],
            frame: Vec::with_capacity(channels),
            scratch: Vec::with_capacity(4096),
            buffer,
            clock,
        }
    }

    fn push(&mut self, samples: impl Iterator<Item = f32>) {
        if self.clock.is_paused() {
            return;
        }
        self.scratch.clear();
        for s in samples {
            self.frame.push(s);
            if self.frame.len() == self.channels {
                let (l, r) = if self.channels == 1 { (self.frame[0], self.frame[0]) } else { (self.frame[0], self.frame[1]) };
                self.frame.clear();
                self.resample([l, r]);
            }
        }
        let mut b = self.buffer.lock().unwrap();
        b.extend(self.scratch.iter().copied());
    }

    /// Emits output samples that fall between the previous input frame and `cur`.
    fn resample(&mut self, cur: [f32; 2]) {
        if (self.step - 1.0).abs() < 1e-9 {
            self.scratch.extend_from_slice(&cur);
            return;
        }
        while self.pos < 1.0 {
            let t = self.pos as f32;
            self.scratch.push(self.prev[0] + (cur[0] - self.prev[0]) * t);
            self.scratch.push(self.prev[1] + (cur[1] - self.prev[1]) * t);
            self.pos += self.step;
        }
        self.pos -= 1.0;
        self.prev = cur;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn converter(rate: u32, channels: usize) -> (Converter, Buffer) {
        let buf: Buffer = Arc::new(Mutex::new(VecDeque::new()));
        (Converter::new(rate, channels, buf.clone(), Arc::new(RecClock::new())), buf)
    }

    #[test]
    fn resamples_44k1_to_48k() {
        let (mut c, buf) = converter(44_100, 1);
        c.push((0..44_100).map(|_| 0.5f32));
        let frames = buf.lock().unwrap().len() / 2;
        assert!((47_990..=48_010).contains(&frames), "{frames}");
    }

    #[test]
    fn passes_48k_stereo_through() {
        let (mut c, buf) = converter(48_000, 2);
        c.push([0.1, 0.2, 0.3, 0.4].into_iter());
        assert_eq!(buf.lock().unwrap().iter().copied().collect::<Vec<_>>(), vec![0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn mixer_emits_audio_in_step_with_the_clock() {
        let enc = Encoder::new(EncoderParams {
            bit_rate: BitRate::Cbr(AAC_BITRATE),
            sample_rate: AUDIO_RATE,
            transport: Transport::Raw,
            channels: ChannelMode::Stereo,
            audio_object_type: AudioObjectType::Mpeg4LowComplexity,
        })
        .unwrap();
        let buf: Buffer = Arc::new(Mutex::new(VecDeque::new()));
        // One second of a 440 Hz tone.
        buf.lock().unwrap().extend((0..AUDIO_RATE).flat_map(|i| {
            let s = (i as f32 / AUDIO_RATE as f32 * 440.0 * std::f32::consts::TAU).sin() * 0.3;
            [s, s]
        }));
        let mut cap = AudioCapture {
            _streams: Vec::new(),
            buffers: vec![buf],
            encoder: enc,
            produced: 0,
            mix: vec![0.0; 2048],
            pcm: vec![0; 2048],
            out: vec![0; 8192],
        };
        let mut packets = Vec::new();
        cap.finish(Duration::from_secs(2), &mut |p| packets.push(p));
        // 2 s of output (1 s of tone + padding) = 93 full frames; the encoder lags by a couple.
        assert_eq!(cap.produced, 93 * 1024);
        assert!((88..=93).contains(&packets.len()), "{}", packets.len());
        assert!(packets.iter().all(|p| !p.is_empty()));
    }
}
