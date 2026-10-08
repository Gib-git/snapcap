//! OpenH264 wrapper producing MP4-ready (length-prefixed) access units.

use openh264::encoder::{
    BitRate, Complexity, Encoder, EncoderConfig, FrameRate, FrameType, IntraFramePeriod, RateControlMode,
    UsageType, VuiConfig,
};
use openh264::{OpenH264API, Timestamp};

use super::yuv::I420;
use crate::error::{Context, Result};

pub struct Packet {
    /// NAL units, each prefixed with a 4-byte big-endian length (AVCC framing).
    pub data: Vec<u8>,
    pub keyframe: bool,
}

/// Target bitrate in bits per second for a given size, frame rate and quality.
pub fn bitrate(width: u32, height: u32, fps: u32, bits_per_pixel: f32) -> u32 {
    ((width * height) as f32 * fps as f32 * bits_per_pixel).clamp(500_000.0, 40_000_000.0) as u32
}

pub struct H264 {
    enc: Encoder,
    pub sps: Option<Vec<u8>>,
    pub pps: Option<Vec<u8>>,
}

impl H264 {
    pub fn new(width: u32, height: u32, fps: u32, bits_per_pixel: f32) -> Result<Self> {
        let bitrate = bitrate(width, height, fps.min(30), bits_per_pixel);
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8) as u16;
        let config = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(bitrate))
            .max_frame_rate(FrameRate::from_hz(fps as f32))
            .skip_frames(false)
            .complexity(Complexity::Medium)
            // Not supported for screen content; set explicitly so OpenH264 doesn't warn.
            .adaptive_quantization(false)
            .background_detection(false)
            .intra_frame_period(IntraFramePeriod::from_num_frames(fps * 2))
            .num_threads(threads)
            // Size-limited slices let OpenH264 encode one frame on several threads (~40% faster).
            .max_slice_len(24_000)
            .vui(VuiConfig::bt709());
        let enc = Encoder::with_api_config(OpenH264API::from_source(), config).context("H.264 encoder init")?;
        Ok(Self { enc, sps: None, pps: None })
    }

    pub fn encode(&mut self, yuv: &I420, ts_ms: u64) -> Result<Option<Packet>> {
        let bs = self
            .enc
            .encode_at(yuv, Timestamp::from_millis(ts_ms))
            .context("H.264 encode")?;
        if matches!(bs.frame_type(), FrameType::Skip | FrameType::Invalid) {
            return Ok(None);
        }
        let keyframe = matches!(bs.frame_type(), FrameType::IDR | FrameType::I);
        let mut data = Vec::new();
        for l in 0..bs.num_layers() {
            let Some(layer) = bs.layer(l) else { continue };
            for n in 0..layer.nal_count() {
                let Some(nal) = layer.nal_unit(n) else { continue };
                let nal = strip_start_code(nal);
                if nal.is_empty() {
                    continue;
                }
                match nal[0] & 0x1f {
                    7 => self.sps = Some(nal.to_vec()),
                    8 => self.pps = Some(nal.to_vec()),
                    _ => {
                        data.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                        data.extend_from_slice(nal);
                    }
                }
            }
        }
        Ok((!data.is_empty()).then_some(Packet { data, keyframe }))
    }

}

fn strip_start_code(nal: &[u8]) -> &[u8] {
    if nal.starts_with(&[0, 0, 0, 1]) {
        &nal[4..]
    } else if nal.starts_with(&[0, 0, 1]) {
        &nal[3..]
    } else {
        nal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_both_start_code_forms() {
        assert_eq!(strip_start_code(&[0, 0, 0, 1, 0x67]), &[0x67]);
        assert_eq!(strip_start_code(&[0, 0, 1, 0x68]), &[0x68]);
        assert_eq!(strip_start_code(&[0x65]), &[0x65]);
    }

    #[test]
    fn first_frame_is_keyframe_with_parameter_sets() {
        let mut enc = H264::new(64, 64, 30, 0.1).unwrap();
        let mut yuv = I420::new(64, 64);
        yuv.data.iter_mut().enumerate().for_each(|(i, b)| *b = (i % 251) as u8);
        let p = enc.encode(&yuv, 0).unwrap().expect("first frame");
        assert!(p.keyframe);
        assert!(enc.sps.is_some() && enc.pps.is_some());
        let len = u32::from_be_bytes(p.data[..4].try_into().unwrap()) as usize;
        assert!(len + 4 <= p.data.len());
    }
}
