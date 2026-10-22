//! Hardware H.264 encoding on macOS through VideoToolbox.
//!
//! Frames (BGRA, straight from ScreenCaptureKit) are wrapped without copying and
//! encoded asynchronously; finished packets are collected from VideoToolbox's
//! callback and drained in presentation order (B-frames are disabled).

#![allow(non_upper_case_globals)]

use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::h264::Packet;
use crate::error::Result;

type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFArrayRef = *const c_void;
type OSStatus = i32;

#[repr(C)]
#[derive(Clone, Copy)]
struct CMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

const INVALID_TIME: CMTime = CMTime { value: 0, timescale: 0, flags: 0, epoch: 0 };
const TIMESCALE: i32 = 1_000_000;
const CODEC_H264: u32 = u32::from_be_bytes(*b"avc1");
const PIXEL_BGRA: u32 = u32::from_be_bytes(*b"BGRA");
const NUMBER_SINT32: isize = 3;
const NUMBER_FLOAT64: isize = 13;

type OutputCallback = unsafe extern "C" fn(*mut c_void, *mut c_void, OSStatus, u32, *const c_void);
type ReleaseBytes = unsafe extern "C" fn(*mut c_void, *const c_void);

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    static kCFBooleanFalse: CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
    fn CFNumberCreate(alloc: CFTypeRef, kind: isize, value: *const c_void) -> CFTypeRef;
    fn CFArrayGetCount(array: CFArrayRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, idx: isize) -> CFTypeRef;
    fn CFDictionaryContainsKey(dict: CFDictionaryRef, key: CFTypeRef) -> u8;
}

#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    static kCVImageBufferColorPrimaries_ITU_R_709_2: CFStringRef;
    static kCVImageBufferTransferFunction_ITU_R_709_2: CFStringRef;
    static kCVImageBufferYCbCrMatrix_ITU_R_709_2: CFStringRef;
    #[allow(clippy::too_many_arguments)]
    fn CVPixelBufferCreateWithBytes(
        alloc: CFTypeRef,
        width: usize,
        height: usize,
        format: u32,
        base: *mut c_void,
        bytes_per_row: usize,
        release: Option<ReleaseBytes>,
        release_ref: *mut c_void,
        attrs: CFDictionaryRef,
        out: *mut CFTypeRef,
    ) -> i32;
}

#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {
    static kCMSampleAttachmentKey_NotSync: CFStringRef;
    fn CMSampleBufferGetDataBuffer(sbuf: *const c_void) -> CFTypeRef;
    fn CMSampleBufferGetPresentationTimeStamp(sbuf: *const c_void) -> CMTime;
    fn CMSampleBufferGetFormatDescription(sbuf: *const c_void) -> CFTypeRef;
    fn CMSampleBufferGetSampleAttachmentsArray(sbuf: *const c_void, create: u8) -> CFArrayRef;
    fn CMBlockBufferGetDataLength(bb: CFTypeRef) -> usize;
    fn CMBlockBufferCopyDataBytes(bb: CFTypeRef, offset: usize, len: usize, dest: *mut c_void) -> OSStatus;
    fn CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
        desc: CFTypeRef,
        index: usize,
        ptr: *mut *const u8,
        size: *mut usize,
        count: *mut usize,
        nal_header_len: *mut i32,
    ) -> OSStatus;
}

#[link(name = "VideoToolbox", kind = "framework")]
unsafe extern "C" {
    static kVTCompressionPropertyKey_RealTime: CFStringRef;
    static kVTCompressionPropertyKey_ProfileLevel: CFStringRef;
    static kVTCompressionPropertyKey_AllowFrameReordering: CFStringRef;
    static kVTCompressionPropertyKey_AverageBitRate: CFStringRef;
    static kVTCompressionPropertyKey_MaxKeyFrameInterval: CFStringRef;
    static kVTCompressionPropertyKey_ExpectedFrameRate: CFStringRef;
    static kVTCompressionPropertyKey_ColorPrimaries: CFStringRef;
    static kVTCompressionPropertyKey_TransferFunction: CFStringRef;
    static kVTCompressionPropertyKey_YCbCrMatrix: CFStringRef;
    static kVTProfileLevel_H264_High_AutoLevel: CFStringRef;
    #[allow(clippy::too_many_arguments)]
    fn VTCompressionSessionCreate(
        alloc: CFTypeRef,
        width: i32,
        height: i32,
        codec: u32,
        encoder_spec: CFDictionaryRef,
        source_attrs: CFDictionaryRef,
        compressed_alloc: CFTypeRef,
        callback: Option<OutputCallback>,
        refcon: *mut c_void,
        out: *mut CFTypeRef,
    ) -> OSStatus;
    fn VTSessionSetProperty(session: CFTypeRef, key: CFStringRef, value: CFTypeRef) -> OSStatus;
    fn VTCompressionSessionPrepareToEncodeFrames(session: CFTypeRef) -> OSStatus;
    fn VTCompressionSessionEncodeFrame(
        session: CFTypeRef,
        image: CFTypeRef,
        pts: CMTime,
        duration: CMTime,
        props: CFDictionaryRef,
        frame_ref: *mut c_void,
        flags_out: *mut u32,
    ) -> OSStatus;
    fn VTCompressionSessionCompleteFrames(session: CFTypeRef, until: CMTime) -> OSStatus;
    fn VTCompressionSessionInvalidate(session: CFTypeRef);
}

/// Encoded output shared with the VideoToolbox callback thread.
#[derive(Default)]
struct Shared {
    packets: Vec<(Duration, Packet)>,
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    error: Option<OSStatus>,
}

pub struct VtEncoder {
    session: CFTypeRef,
    shared: *const Mutex<Shared>,
    width: usize,
    height: usize,
}

// The session is only used from the encoder thread; VideoToolbox handles its own threads.
unsafe impl Send for VtEncoder {}

fn check(status: OSStatus, what: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!("VideoToolbox {what} failed ({status})").into())
    }
}

impl VtEncoder {
    pub fn new(width: u32, height: u32, fps: u32, bitrate: u32) -> Result<Self> {
        let shared = Arc::into_raw(Arc::new(Mutex::new(Shared::default())));
        let mut session: CFTypeRef = std::ptr::null();
        let status = unsafe {
            VTCompressionSessionCreate(
                std::ptr::null(),
                width as i32,
                height as i32,
                CODEC_H264,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                Some(on_output),
                shared as *mut c_void,
                &mut session,
            )
        };
        if status != 0 || session.is_null() {
            unsafe { drop(Arc::from_raw(shared)) };
            return Err(format!("VideoToolbox session create failed ({status})").into());
        }
        let enc = VtEncoder { session, shared, width: width as usize, height: height as usize };
        unsafe {
            let set = |key: CFStringRef, value: CFTypeRef| VTSessionSetProperty(session, key, value);
            let num_i32 = |v: i32| CFNumberCreate(std::ptr::null(), NUMBER_SINT32, &v as *const i32 as *const c_void);
            let num_f64 = |v: f64| CFNumberCreate(std::ptr::null(), NUMBER_FLOAT64, &v as *const f64 as *const c_void);
            check(set(kVTCompressionPropertyKey_RealTime, kCFBooleanTrue), "real-time")?;
            check(set(kVTCompressionPropertyKey_AllowFrameReordering, kCFBooleanFalse), "frame reordering")?;
            // Optional tuning: older encoders may reject some of these.
            let _ = set(kVTCompressionPropertyKey_ProfileLevel, kVTProfileLevel_H264_High_AutoLevel);
            for (key, value) in [
                (kVTCompressionPropertyKey_AverageBitRate, num_i32(bitrate as i32)),
                (kVTCompressionPropertyKey_MaxKeyFrameInterval, num_i32((fps * 2) as i32)),
                (kVTCompressionPropertyKey_ExpectedFrameRate, num_f64(fps as f64)),
            ] {
                let _ = set(key, value);
                CFRelease(value);
            }
            let _ = set(kVTCompressionPropertyKey_ColorPrimaries, kCVImageBufferColorPrimaries_ITU_R_709_2);
            let _ = set(kVTCompressionPropertyKey_TransferFunction, kCVImageBufferTransferFunction_ITU_R_709_2);
            let _ = set(kVTCompressionPropertyKey_YCbCrMatrix, kCVImageBufferYCbCrMatrix_ITU_R_709_2);
            check(VTCompressionSessionPrepareToEncodeFrames(session), "prepare")?;
        }
        Ok(enc)
    }

    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn shared(&self) -> &Mutex<Shared> {
        unsafe { &*self.shared }
    }

    /// Submits a tightly packed or strided BGRA frame. Ownership of `data` passes to VideoToolbox.
    pub fn encode(&mut self, data: Vec<u8>, stride: usize, ts: Duration) -> Result<()> {
        debug_assert!(data.len() >= stride * self.height);
        let boxed = Box::into_raw(Box::new(data));
        let base = unsafe { (*boxed).as_mut_ptr() } as *mut c_void;
        let mut pixbuf: CFTypeRef = std::ptr::null();
        let status = unsafe {
            CVPixelBufferCreateWithBytes(
                std::ptr::null(),
                self.width,
                self.height,
                PIXEL_BGRA,
                base,
                stride,
                Some(release_bytes),
                boxed as *mut c_void,
                std::ptr::null(),
                &mut pixbuf,
            )
        };
        if status != 0 || pixbuf.is_null() {
            unsafe { drop(Box::from_raw(boxed)) };
            return Err(format!("CVPixelBufferCreateWithBytes failed ({status})").into());
        }
        let pts = CMTime { value: ts.as_micros() as i64, timescale: TIMESCALE, flags: 1, epoch: 0 };
        let status = unsafe {
            let s = VTCompressionSessionEncodeFrame(self.session, pixbuf, pts, INVALID_TIME, std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut());
            CFRelease(pixbuf); // VideoToolbox retains it while encoding
            s
        };
        check(status, "encode")?;
        match self.shared().lock().unwrap().error.take() {
            Some(e) => check(e, "encode callback"),
            None => Ok(()),
        }
    }

    /// Takes packets finished so far, in presentation order.
    pub fn drain(&mut self) -> Vec<(Duration, Packet)> {
        std::mem::take(&mut self.shared().lock().unwrap().packets)
    }

    pub fn parameter_sets(&self) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        let s = self.shared().lock().unwrap();
        (s.sps.clone(), s.pps.clone())
    }

    /// Waits for every submitted frame to be encoded.
    pub fn flush(&mut self) -> Vec<(Duration, Packet)> {
        unsafe { VTCompressionSessionCompleteFrames(self.session, INVALID_TIME) };
        self.drain()
    }
}

impl Drop for VtEncoder {
    fn drop(&mut self) {
        unsafe {
            VTCompressionSessionCompleteFrames(self.session, INVALID_TIME);
            VTCompressionSessionInvalidate(self.session);
            CFRelease(self.session);
            drop(Arc::from_raw(self.shared));
        }
    }
}

unsafe extern "C" fn release_bytes(refcon: *mut c_void, _base: *const c_void) {
    unsafe { drop(Box::from_raw(refcon as *mut Vec<u8>)) };
}

unsafe extern "C" fn on_output(refcon: *mut c_void, _frame: *mut c_void, status: OSStatus, _flags: u32, sbuf: *const c_void) {
    let shared = unsafe { &*(refcon as *const Mutex<Shared>) };
    let mut s = shared.lock().unwrap();
    if status != 0 {
        s.error = Some(status);
        return;
    }
    if sbuf.is_null() {
        return; // frame dropped by the encoder
    }
    unsafe {
        let block = CMSampleBufferGetDataBuffer(sbuf);
        if block.is_null() {
            return;
        }
        let len = CMBlockBufferGetDataLength(block);
        let mut data = vec![0u8; len];
        if CMBlockBufferCopyDataBytes(block, 0, len, data.as_mut_ptr() as *mut c_void) != 0 {
            return;
        }
        let attachments = CMSampleBufferGetSampleAttachmentsArray(sbuf, 0);
        let keyframe = attachments.is_null()
            || CFArrayGetCount(attachments) == 0
            || CFDictionaryContainsKey(CFArrayGetValueAtIndex(attachments, 0), kCMSampleAttachmentKey_NotSync) == 0;
        if keyframe && s.sps.is_none() {
            let desc = CMSampleBufferGetFormatDescription(sbuf);
            let param = |i: usize| {
                let (mut ptr, mut size, mut count, mut hdr) = (std::ptr::null(), 0usize, 0usize, 0i32);
                let st = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(desc, i, &mut ptr, &mut size, &mut count, &mut hdr);
                (st == 0 && !ptr.is_null()).then(|| std::slice::from_raw_parts(ptr, size).to_vec())
            };
            s.sps = param(0);
            s.pps = param(1);
        }
        let pts = CMSampleBufferGetPresentationTimeStamp(sbuf);
        let ts = if pts.timescale > 0 {
            Duration::from_micros((pts.value.max(0) as i128 * 1_000_000 / pts.timescale as i128) as u64)
        } else {
            Duration::ZERO
        };
        // VideoToolbox already emits AVCC framing (4-byte big-endian lengths).
        s.packets.push((ts, Packet { data, keyframe }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_bgra_frames_to_avcc() {
        let (w, h) = (640usize, 360usize);
        let mut enc = VtEncoder::new(w as u32, h as u32, 30, 2_000_000).expect("VideoToolbox available");
        let mut packets = Vec::new();
        for i in 0..30u32 {
            let frame: Vec<u8> = (0..w * h).flat_map(|p| [(p as u32 + i * 9) as u8, (i * 5) as u8, 120, 255]).collect();
            enc.encode(frame, w * 4, Duration::from_millis(i as u64 * 33)).unwrap();
            packets.extend(enc.drain());
        }
        packets.extend(enc.flush());
        assert_eq!(packets.len(), 30);
        assert!(packets[0].1.keyframe);
        let (sps, pps) = enc.parameter_sets();
        assert_eq!(sps.unwrap()[0] & 0x1f, 7);
        assert_eq!(pps.unwrap()[0] & 0x1f, 8);
        // Timestamps come back in order and AVCC lengths cover each packet exactly.
        assert!(packets.windows(2).all(|p| p[0].0 < p[1].0));
        for (_, p) in &packets {
            let mut o = 0;
            while o < p.data.len() {
                o += 4 + u32::from_be_bytes(p.data[o..o + 4].try_into().unwrap()) as usize;
            }
            assert_eq!(o, p.data.len());
        }
    }
}
