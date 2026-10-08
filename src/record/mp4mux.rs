//! MP4 writer for one H.264 video track plus an optional AAC audio track.
//!
//! Video is variable frame rate: each sample's duration is the gap to the next frame,
//! so dropped or skipped frames never desynchronise audio and video.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bytes::Bytes;
use mp4::{
    AacConfig, AudioObjectType, AvcConfig, ChannelConfig, MediaConfig, Mp4Config, Mp4Sample, Mp4Writer,
    SampleFreqIndex, TrackConfig, TrackType,
};

use super::h264::Packet;
use crate::error::{Context, Result};

pub const VIDEO_TIMESCALE: u32 = 90_000;
pub const AUDIO_RATE: u32 = 48_000;
pub const AAC_FRAME: u32 = 1024;

const VIDEO_TRACK: u32 = 1;
const AUDIO_TRACK: u32 = 2;

pub struct Mp4Muxer {
    path: PathBuf,
    width: u16,
    height: u16,
    audio_bitrate: Option<u32>,
    writer: Option<Mp4Writer<BufWriter<File>>>,
    /// Previous video sample, written once the next one's timestamp is known.
    pending: Option<(u64, Packet)>,
    /// AAC frames that arrived before the first keyframe opened the file.
    audio_backlog: Vec<Vec<u8>>,
    audio_frames: u64,
    video_frames: u64,
}

fn to_ticks(d: Duration) -> u64 {
    (d.as_nanos() * VIDEO_TIMESCALE as u128 / 1_000_000_000) as u64
}

impl Mp4Muxer {
    pub fn new(path: &Path, width: u32, height: u32, audio_bitrate: Option<u32>) -> Self {
        Self {
            path: path.to_owned(),
            width: width as u16,
            height: height as u16,
            audio_bitrate,
            writer: None,
            pending: None,
            audio_backlog: Vec::new(),
            audio_frames: 0,
            video_frames: 0,
        }
    }

    fn open(&mut self, sps: &[u8], pps: &[u8]) -> Result<()> {
        let file = File::create(&self.path).context(format!("cannot create {}", self.path.display()))?;
        let config = Mp4Config {
            major_brand: "isom".parse().unwrap(),
            minor_version: 512,
            compatible_brands: ["isom", "iso2", "avc1", "mp41"].iter().map(|b| b.parse().unwrap()).collect(),
            timescale: 1000,
        };
        let mut w = Mp4Writer::write_start(BufWriter::with_capacity(1 << 20, file), &config).context("mp4 start")?;
        w.add_track(&TrackConfig {
            track_type: TrackType::Video,
            timescale: VIDEO_TIMESCALE,
            language: "und".into(),
            media_conf: MediaConfig::AvcConfig(AvcConfig {
                width: self.width,
                height: self.height,
                seq_param_set: sps.to_vec(),
                pic_param_set: pps.to_vec(),
            }),
        })
        .context("mp4 video track")?;
        if let Some(bitrate) = self.audio_bitrate {
            w.add_track(&TrackConfig {
                track_type: TrackType::Audio,
                timescale: AUDIO_RATE,
                language: "und".into(),
                media_conf: MediaConfig::AacConfig(AacConfig {
                    bitrate,
                    profile: AudioObjectType::AacLowComplexity,
                    freq_index: SampleFreqIndex::Freq48000,
                    chan_conf: ChannelConfig::Stereo,
                }),
            })
            .context("mp4 audio track")?;
        }
        self.writer = Some(w);
        for frame in std::mem::take(&mut self.audio_backlog) {
            self.write_audio(frame)?;
        }
        Ok(())
    }

    /// Adds an encoded frame. Frames before the first keyframe are dropped (undecodable).
    pub fn push_video(&mut self, ts: Duration, pkt: Packet, sps: Option<&[u8]>, pps: Option<&[u8]>) -> Result<()> {
        if self.writer.is_none() {
            match (pkt.keyframe, sps, pps) {
                (true, Some(sps), Some(pps)) => self.open(sps, pps)?,
                _ => return Ok(()),
            }
        }
        let ticks = to_ticks(ts);
        if let Some((prev_ticks, prev)) = self.pending.take() {
            // Guarantee strictly increasing timestamps even if the clock stalls.
            let ticks = ticks.max(prev_ticks + 1);
            self.write_video(prev_ticks, (ticks - prev_ticks) as u32, prev)?;
            self.pending = Some((ticks, pkt));
        } else {
            self.pending = Some((ticks, pkt));
        }
        Ok(())
    }

    fn write_video(&mut self, start: u64, duration: u32, pkt: Packet) -> Result<()> {
        let w = self.writer.as_mut().expect("opened");
        w.write_sample(
            VIDEO_TRACK,
            &Mp4Sample {
                start_time: start,
                duration,
                rendering_offset: 0,
                is_sync: pkt.keyframe,
                bytes: Bytes::from(pkt.data),
            },
        )
        .context("mp4 write video")?;
        self.video_frames += 1;
        Ok(())
    }

    /// Adds one raw AAC frame (1024 samples per channel).
    pub fn push_audio(&mut self, frame: Vec<u8>) -> Result<()> {
        if self.audio_bitrate.is_none() {
            return Ok(());
        }
        if self.writer.is_none() {
            self.audio_backlog.push(frame);
            return Ok(());
        }
        self.write_audio(frame)
    }

    fn write_audio(&mut self, frame: Vec<u8>) -> Result<()> {
        let start = self.audio_frames * AAC_FRAME as u64;
        let w = self.writer.as_mut().expect("opened");
        w.write_sample(
            AUDIO_TRACK,
            &Mp4Sample {
                start_time: start,
                duration: AAC_FRAME,
                rendering_offset: 0,
                is_sync: true,
                bytes: Bytes::from(frame),
            },
        )
        .context("mp4 write audio")?;
        self.audio_frames += 1;
        Ok(())
    }

    /// Writes the last frame (lasting until `end`) and the index. Returns the frame count.
    pub fn finish(mut self, end: Duration, min_frame: Duration) -> Result<u64> {
        if let Some((ticks, pkt)) = self.pending.take() {
            let end_ticks = to_ticks(end).max(ticks + to_ticks(min_frame).max(1));
            self.write_video(ticks, (end_ticks - ticks) as u32, pkt)?;
        }
        let Some(mut w) = self.writer.take() else {
            return Err("no video frames were captured".into());
        };
        w.write_end().context("mp4 finalize")?;
        let mut inner = w.into_writer();
        std::io::Write::flush(&mut inner).context("mp4 flush")?;
        Ok(self.video_frames)
    }
}
