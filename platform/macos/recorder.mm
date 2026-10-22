#include "recorder.h"

#import <AVFoundation/AVFoundation.h>
#import <CoreMedia/CoreMedia.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>

#include <unistd.h>

#include <atomic>
#include <cmath>
#include <cstring>
#include <format>
#include <optional>

#include "mac_util.h"
#include "snapcap/audio_mix.h"
#include "snapcap/clock.h"
#include "snapcap/gif.h"
#include "snapcap/log.h"

using namespace snapcap;
// MacTypes.h also defines `Duration`.
using RecTime = snapcap::Duration;

namespace {

constexpr uint32_t kAacBitrate = 160'000;
constexpr int64_t kNanos = 1'000'000'000;

CMTime to_cmtime(RecTime d) {
  return CMTimeMake(d.count(), static_cast<int32_t>(kNanos));
}

// Pushes any common LPCM layout (float or int16, interleaved or planar) into a converter.
bool push_audio(CMSampleBufferRef buffer, std::unique_ptr<AudioConverter>& converter,
                const std::shared_ptr<SampleQueue>& queue, const std::shared_ptr<const RecClock>& clock) {
  CMFormatDescriptionRef fmt = CMSampleBufferGetFormatDescription(buffer);
  const AudioStreamBasicDescription* asbd = fmt ? CMAudioFormatDescriptionGetStreamBasicDescription(fmt) : nullptr;
  if (asbd == nullptr || asbd->mFormatID != kAudioFormatLinearPCM) return false;
  const bool is_float = (asbd->mFormatFlags & kAudioFormatFlagIsFloat) != 0;
  const bool planar = (asbd->mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0;
  if (!(is_float && asbd->mBitsPerChannel == 32) && !(!is_float && asbd->mBitsPerChannel == 16)) return false;
  const uint32_t channels = asbd->mChannelsPerFrame;
  if (!converter) {
    converter = std::make_unique<AudioConverter>(static_cast<uint32_t>(asbd->mSampleRate), channels, queue, clock);
  }

  size_t list_size = 0;
  CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(buffer, &list_size, nullptr, 0, nullptr, nullptr, 0, nullptr);
  std::vector<uint8_t> storage(list_size);
  auto* list = reinterpret_cast<AudioBufferList*>(storage.data());
  CMBlockBufferRef block = nullptr;
  if (CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
          buffer, nullptr, list, list_size, nullptr, nullptr,
          kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment, &block) != noErr) {
    return false;
  }
  auto as_float = [&](const AudioBuffer& b) {
    size_t n = b.mDataByteSize / (is_float ? 4 : 2);
    std::vector<float> out(n);
    if (is_float) {
      std::memcpy(out.data(), b.mData, n * 4);
    } else {
      const auto* s = static_cast<const int16_t*>(b.mData);
      for (size_t i = 0; i < n; ++i) out[i] = s[i] / 32768.0f;
    }
    return out;
  };
  if (planar) {
    std::vector<std::vector<float>> planes;
    for (UInt32 i = 0; i < list->mNumberBuffers; ++i) planes.push_back(as_float(list->mBuffers[i]));
    std::vector<std::span<const float>> spans(planes.begin(), planes.end());
    converter->push_planar(spans);
  } else if (list->mNumberBuffers > 0) {
    converter->push_interleaved(as_float(list->mBuffers[0]));
  }
  if (block != nullptr) CFRelease(block);
  return true;
}

}  // namespace

// ---------------------------------------------------------------------------------------------

@interface SNCRecordingSession : NSObject <SCStreamOutput, SCStreamDelegate, AVCaptureAudioDataOutputSampleBufferDelegate>
- (instancetype)initWithConfig:(const RecordConfig&)config done:(RecordingDone)done;
- (void)start;
- (void)stop;
@property(nonatomic, readonly) std::shared_ptr<RecClock> clock;
@end

@implementation SNCRecordingSession {
  RecordConfig _cfg;
  RecordingDone _done;  // called once, on the main thread
  std::shared_ptr<RecClock> _clock;
  dispatch_queue_t _queue;  // everything below is only touched on this queue
  dispatch_queue_t _micQueue;
  SCStream* _stream;
  AVAssetWriter* _writer;
  AVAssetWriterInput* _video;
  AVAssetWriterInputPixelBufferAdaptor* _adaptor;
  AVAssetWriterInput* _audio;
  std::unique_ptr<GifWriter> _gif;
  std::optional<Pacer> _pacer;
  AudioMixer _mixer;
  std::shared_ptr<SampleQueue> _systemQueue;
  std::shared_ptr<SampleQueue> _micQueueSamples;
  std::unique_ptr<AudioConverter> _systemConverter;
  std::unique_ptr<AudioConverter> _micConverter;  // touched on _micQueue only
  AVCaptureSession* _micSession;
  dispatch_source_t _audioTimer;
  std::optional<RecTime> _sessionStart;  // first video frame
  RecTime _lastFrame;
  uint64_t _frames;
  uint64_t _dropped;
  std::vector<std::string> _warnings;
  std::optional<std::string> _failure;
  BOOL _finalizing;
  std::atomic<bool> _stopRequested;
}

- (instancetype)initWithConfig:(const RecordConfig&)config done:(RecordingDone)done {
  if ((self = [super init])) {
    _cfg = config;
    _done = std::move(done);
    _clock = std::make_shared<RecClock>();
    _queue = dispatch_queue_create("dev.snapcap.recorder", DISPATCH_QUEUE_SERIAL);
    _micQueue = dispatch_queue_create("dev.snapcap.microphone", DISPATCH_QUEUE_SERIAL);
    _pacer.emplace(config.fps);
    _lastFrame = RecTime{0};
  }
  return self;
}

- (std::shared_ptr<RecClock>)clock {
  return _clock;
}

// ---- setup

- (void)start {
  dispatch_async(_queue, ^{
    [self setUp];
  });
}

// On _queue.
- (void)setUp {
  const BOOL mp4 = _cfg.format == VideoFormat::Mp4;
  if (mp4) {
    NSError* error = nil;
    [NSFileManager.defaultManager removeItemAtURL:mac::file_url(_cfg.path) error:nil];
    _writer = [AVAssetWriter assetWriterWithURL:mac::file_url(_cfg.path) fileType:AVFileTypeMPEG4 error:&error];
    if (_writer == nil) return [self failNow:std::format("Cannot create the video file: {}", mac::str(error.localizedDescription))];
    NSDictionary* compression = @{
      AVVideoAverageBitRateKey : @(video_bitrate(_cfg.out_width, _cfg.out_height, _cfg.fps, _cfg.quality)),
      AVVideoExpectedSourceFrameRateKey : @(_cfg.fps),
      AVVideoMaxKeyFrameIntervalKey : @(_cfg.fps * 2),
      AVVideoProfileLevelKey : AVVideoProfileLevelH264HighAutoLevel,
      AVVideoAllowFrameReorderingKey : @NO,
    };
    NSDictionary* settings = @{
      AVVideoCodecKey : AVVideoCodecTypeH264,
      AVVideoWidthKey : @(_cfg.out_width),
      AVVideoHeightKey : @(_cfg.out_height),
      AVVideoCompressionPropertiesKey : compression,
      AVVideoColorPropertiesKey : @{
        AVVideoColorPrimariesKey : AVVideoColorPrimaries_ITU_R_709_2,
        AVVideoTransferFunctionKey : AVVideoTransferFunction_ITU_R_709_2,
        AVVideoYCbCrMatrixKey : AVVideoYCbCrMatrix_ITU_R_709_2,
      },
    };
    _video = [AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeVideo outputSettings:settings];
    _video.expectsMediaDataInRealTime = YES;
    _adaptor = [AVAssetWriterInputPixelBufferAdaptor assetWriterInputPixelBufferAdaptorWithAssetWriterInput:_video
                                                                                sourcePixelBufferAttributes:nil];
    if (![_writer canAddInput:_video]) return [self failNow:"The video encoder is unavailable"];
    [_writer addInput:_video];
    if (_cfg.audio.any()) {
      AudioChannelLayout layout{};
      layout.mChannelLayoutTag = kAudioChannelLayoutTag_Stereo;
      _audio = [AVAssetWriterInput
          assetWriterInputWithMediaType:AVMediaTypeAudio
                         outputSettings:@{
                           AVFormatIDKey : @(kAudioFormatMPEG4AAC),
                           AVSampleRateKey : @(kAudioRate),
                           AVNumberOfChannelsKey : @2,
                           AVEncoderBitRateKey : @(kAacBitrate),
                           AVChannelLayoutKey : [NSData dataWithBytes:&layout length:sizeof(layout)],
                         }];
      _audio.expectsMediaDataInRealTime = YES;
      if ([_writer canAddInput:_audio]) {
        [_writer addInput:_audio];
      } else {
        _audio = nil;
        _warnings.push_back("Recording without audio: the AAC encoder is unavailable");
      }
    }
    if (![_writer startWriting]) {
      return [self failNow:std::format("Cannot start writing: {}", mac::str(_writer.error.localizedDescription))];
    }
  } else {
    auto gif = GifWriter::create(_cfg.path, _cfg.out_width, _cfg.out_height);
    if (!gif) return [self failNow:gif.error().message];
    _gif = std::move(*gif);
  }

  if (mp4 && _audio != nil && _cfg.audio.mic) [self startMicrophone];

  [SCShareableContent getShareableContentExcludingDesktopWindows:NO
                                             onScreenWindowsOnly:YES
                                               completionHandler:^(SCShareableContent* content, NSError* error) {
                                                 dispatch_async(self->_queue, ^{
                                                   [self startStreamWith:content error:error];
                                                 });
                                               }];
}

- (void)startStreamWith:(SCShareableContent*)content error:(NSError*)error {
  if (_stopRequested || _finalizing) return [self finishRecording];
  if (content == nil) {
    return [self failNow:std::format("Screen recording permission is required ({})", mac::str(error.localizedDescription))];
  }
  SCDisplay* display = nil;
  for (SCDisplay* d in content.displays) {
    if (d.displayID == _cfg.target.monitor.id) display = d;
  }
  if (display == nil) return [self failNow:"The display to record is no longer available"];
  NSMutableArray<SCRunningApplication*>* own = [NSMutableArray array];
  for (SCRunningApplication* app in content.applications) {
    if (app.processID == getpid()) [own addObject:app];
  }
  // SnapCap's own windows (recording bar, border, notices) never appear in recordings.
  SCContentFilter* filter = [[SCContentFilter alloc] initWithDisplay:display excludingApplications:own exceptingWindows:@[]];

  SCStreamConfiguration* config = [[SCStreamConfiguration alloc] init];
  double scale = _cfg.target.monitor.px_per_unit();
  if (_cfg.target.region) {
    PxRect r = _cfg.target.pixel_rect();
    config.sourceRect = CGRectMake(r.x / scale, r.y / scale, r.w / scale, r.h / scale);
  }
  config.width = _cfg.out_width;
  config.height = _cfg.out_height;
  config.minimumFrameInterval = CMTimeMake(1, static_cast<int32_t>(std::max(1u, _cfg.fps)));
  config.queueDepth = 6;
  config.showsCursor = _cfg.show_cursor;
  if (_cfg.format == VideoFormat::Mp4) {
    config.pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
    config.colorMatrix = kCGDisplayStreamYCbCrMatrix_ITU_R_709_2;
  } else {
    config.pixelFormat = kCVPixelFormatType_32BGRA;
  }
  config.colorSpaceName = kCGColorSpaceSRGB;
  if (_audio != nil && _cfg.audio.system) {
    config.capturesAudio = YES;
    config.excludesCurrentProcessAudio = YES;
    config.sampleRate = kAudioRate;
    config.channelCount = 2;
  }

  _stream = [[SCStream alloc] initWithFilter:filter configuration:config delegate:self];
  NSError* add_error = nil;
  if (![_stream addStreamOutput:self type:SCStreamOutputTypeScreen sampleHandlerQueue:_queue error:&add_error]) {
    return [self failNow:std::format("Cannot capture the screen: {}", mac::str(add_error.localizedDescription))];
  }
  if (config.capturesAudio) {
    _systemQueue = std::make_shared<SampleQueue>();
    _mixer.add_source(_systemQueue);
    if (![_stream addStreamOutput:self type:SCStreamOutputTypeAudio sampleHandlerQueue:_queue error:&add_error]) {
      _warnings.push_back("System audio unavailable");
    }
  }
  [_stream startCaptureWithCompletionHandler:^(NSError* start_error) {
    if (start_error == nil) return;
    dispatch_async(self->_queue, ^{
      self->_failure = std::format("Recording could not start: {}", mac::str(start_error.localizedDescription));
      [self finishRecording];
    });
  }];
  [self startAudioTimer];
}

- (void)startMicrophone {
  AVAuthorizationStatus status = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
  if (status == AVAuthorizationStatusNotDetermined) {
    // Asks once; the microphone joins the recording as soon as access is granted.
    [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio
                             completionHandler:^(BOOL granted) {
                               dispatch_async(self->_queue, ^{
                                 if (granted && !self->_finalizing) [self openMicrophone];
                                 if (!granted) self->_warnings.push_back("Microphone unavailable: access was not allowed");
                               });
                             }];
    return;
  }
  if (status != AVAuthorizationStatusAuthorized) {
    _warnings.push_back("Microphone unavailable: allow SnapCap in System Settings → Privacy & Security → Microphone");
    return;
  }
  [self openMicrophone];
}

// Runs on _queue (or during start, before capture begins).
- (void)openMicrophone {
  AVCaptureDevice* device = nil;
  if (_cfg.audio.mic_device) {
    NSString* wanted = mac::ns(*_cfg.audio.mic_device);
    for (AVCaptureDevice* d in [self.class microphones]) {
      if ([d.localizedName isEqualToString:wanted]) device = d;
    }
    if (device == nil) log::warn("microphone '{}' not found, using the default", *_cfg.audio.mic_device);
  }
  if (device == nil) device = [AVCaptureDevice defaultDeviceWithMediaType:AVMediaTypeAudio];
  if (device == nil) {
    _warnings.push_back("Microphone unavailable: no microphone found");
    return;
  }
  NSError* error = nil;
  AVCaptureDeviceInput* input = [AVCaptureDeviceInput deviceInputWithDevice:device error:&error];
  AVCaptureSession* session = [[AVCaptureSession alloc] init];
  AVCaptureAudioDataOutput* output = [[AVCaptureAudioDataOutput alloc] init];
  if (input == nil || ![session canAddInput:input] || ![session canAddOutput:output]) {
    _warnings.push_back(std::format("Microphone unavailable: {}", error ? mac::str(error.localizedDescription) : "cannot open it"));
    return;
  }
  [session addInput:input];
  [output setSampleBufferDelegate:self queue:_micQueue];
  [session addOutput:output];
  _micQueueSamples = std::make_shared<SampleQueue>();
  _mixer.add_source(_micQueueSamples);
  _micSession = session;
  [session startRunning];
}

+ (NSArray<AVCaptureDevice*>*)microphones {
  NSArray<AVCaptureDeviceType>* types;
  if (@available(macOS 14.0, *)) {
    types = @[ AVCaptureDeviceTypeMicrophone, AVCaptureDeviceTypeExternal ];
  } else {
    types = @[ AVCaptureDeviceTypeBuiltInMicrophone, AVCaptureDeviceTypeExternalUnknown ];
  }
  return [AVCaptureDeviceDiscoverySession discoverySessionWithDeviceTypes:types
                                                                mediaType:AVMediaTypeAudio
                                                                 position:AVCaptureDevicePositionUnspecified]
      .devices;
}

- (void)startAudioTimer {
  if (_audio == nil) return;
  _audioTimer = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0, _queue);
  dispatch_source_set_timer(_audioTimer, dispatch_time(DISPATCH_TIME_NOW, 0), 40 * NSEC_PER_MSEC, 10 * NSEC_PER_MSEC);
  __weak SNCRecordingSession* weak = self;
  dispatch_source_set_event_handler(_audioTimer, ^{
    SNCRecordingSession* s = weak;
    if (s != nil) [s pumpAudio:NO];
  });
  dispatch_resume(_audioTimer);
}

// ---- samples (on _queue)

- (void)stream:(SCStream*)stream didOutputSampleBuffer:(CMSampleBufferRef)buffer ofType:(SCStreamOutputType)type {
  if (_finalizing || !CMSampleBufferIsValid(buffer)) return;
  if (type == SCStreamOutputTypeAudio) {
    push_audio(buffer, _systemConverter, _systemQueue, _clock);
    return;
  }
  if (type != SCStreamOutputTypeScreen) return;
  CFArrayRef attachments = CMSampleBufferGetSampleAttachmentsArray(buffer, false);
  if (attachments == nullptr || CFArrayGetCount(attachments) == 0) return;
  NSDictionary* info = (__bridge NSDictionary*)CFArrayGetValueAtIndex(attachments, 0);
  NSNumber* status = info[SCStreamFrameInfoStatus];
  if (status == nil || status.integerValue != SCFrameStatusComplete) return;  // idle: the screen didn't change
  CVPixelBufferRef pixels = CMSampleBufferGetImageBuffer(buffer);
  if (pixels == nullptr) return;

  // Recording time of this frame: now on the recording clock, minus how long ago it was captured.
  auto live = _clock->live_now();
  if (!live) return;  // paused
  double age = CMTimeGetSeconds(CMTimeSubtract(CMClockGetTime(CMClockGetHostTimeClock()),
                                               CMSampleBufferGetPresentationTimeStamp(buffer)));
  age = std::clamp(std::isfinite(age) ? age : 0.0, 0.0, 0.5);
  RecTime t = std::max(RecTime{0}, *live - std::chrono::duration_cast<RecTime>(std::chrono::duration<double>(age)));
  auto ts = _pacer->admit(t);
  if (!ts) return;

  if (_gif) {
    CVPixelBufferLockBaseAddress(pixels, kCVPixelBufferLock_ReadOnly);
    const auto* base = static_cast<const uint8_t*>(CVPixelBufferGetBaseAddress(pixels));
    size_t stride = CVPixelBufferGetBytesPerRow(pixels);
    auto w = static_cast<uint32_t>(CVPixelBufferGetWidth(pixels));
    auto h = static_cast<uint32_t>(CVPixelBufferGetHeight(pixels));
    VideoFrame frame{{base, stride * h}, w, h, stride, PixelOrder::Bgra, *ts};
    auto r = _gif->push(frame);
    CVPixelBufferUnlockBaseAddress(pixels, kCVPixelBufferLock_ReadOnly);
    if (!r) {
      _failure = r.error().message;
      return [self finishRecording];
    }
  } else {
    if (!_sessionStart) {
      _sessionStart = *ts;
      [_writer startSessionAtSourceTime:to_cmtime(*ts)];
    }
    if (!_video.readyForMoreMediaData) {
      ++_dropped;
      return;
    }
    if (![_adaptor appendPixelBuffer:pixels withPresentationTime:to_cmtime(*ts)]) {
      _failure = std::format("Video encoding failed: {}", mac::str(_writer.error.localizedDescription));
      return [self finishRecording];
    }
  }
  _lastFrame = *ts;
  ++_frames;
}

- (void)captureOutput:(AVCaptureOutput*)output
    didOutputSampleBuffer:(CMSampleBufferRef)buffer
           fromConnection:(AVCaptureConnection*)connection {
  // On _micQueue. The sample queue is thread-safe; the converter is only used here.
  if (_micQueueSamples) push_audio(buffer, _micConverter, _micQueueSamples, _clock);
}

- (void)pumpAudio:(BOOL)final {
  if (_audio == nil) return;
  auto sink = [&](std::span<const float> pcm, uint64_t first_frame) {
    RecTime at{static_cast<int64_t>(first_frame) * kNanos / kAudioRate};
    // Audio before the first video frame is outside the file's timeline.
    if (!_sessionStart || at + RecTime{static_cast<int64_t>(kMixFrames) * kNanos / kAudioRate} <= *_sessionStart) return;
    if (!_audio.readyForMoreMediaData) return;
    CMSampleBufferRef sample = [self audioSample:pcm at:at];
    if (sample != nullptr) {
      [_audio appendSampleBuffer:sample];
      CFRelease(sample);
    }
  };
  if (final) {
    _mixer.finish(_clock->now(), sink);
  } else {
    _mixer.pump(_clock->now(), sink);
  }
}

- (CMSampleBufferRef)audioSample:(std::span<const float>)pcm at:(RecTime)at {
  AudioStreamBasicDescription asbd{};
  asbd.mSampleRate = kAudioRate;
  asbd.mFormatID = kAudioFormatLinearPCM;
  asbd.mFormatFlags = kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked;
  asbd.mBytesPerPacket = 8;
  asbd.mFramesPerPacket = 1;
  asbd.mBytesPerFrame = 8;
  asbd.mChannelsPerFrame = 2;
  asbd.mBitsPerChannel = 32;
  CMAudioFormatDescriptionRef fmt = nullptr;
  if (CMAudioFormatDescriptionCreate(nullptr, &asbd, 0, nullptr, 0, nullptr, nullptr, &fmt) != noErr) return nullptr;
  size_t bytes = pcm.size() * sizeof(float);
  CMBlockBufferRef block = nullptr;
  OSStatus st = CMBlockBufferCreateWithMemoryBlock(nullptr, nullptr, bytes, nullptr, nullptr, 0, bytes, 0, &block);
  if (st == noErr) st = CMBlockBufferReplaceDataBytes(pcm.data(), block, 0, bytes);
  CMSampleBufferRef sample = nullptr;
  if (st == noErr) {
    st = CMAudioSampleBufferCreateReadyWithPacketDescriptions(nullptr, block, fmt, static_cast<CMItemCount>(pcm.size() / 2),
                                                              to_cmtime(at), nullptr, &sample);
  }
  if (block != nullptr) CFRelease(block);
  CFRelease(fmt);
  return st == noErr ? sample : nullptr;
}

- (void)stream:(SCStream*)stream didStopWithError:(NSError*)error {
  dispatch_async(_queue, ^{
    if (self->_finalizing) return;
    self->_warnings.push_back(std::format("Recording stopped early: {}", mac::str(error.localizedDescription)));
    [self finishRecording];
  });
}

// ---- stopping

- (void)stop {
  if (_stopRequested.exchange(true)) return;
  dispatch_async(_queue, ^{
    if (self->_finalizing) return;
    SCStream* stream = self->_stream;
    if (stream == nil) return [self finishRecording];
    [stream stopCaptureWithCompletionHandler:^(NSError*) {
      dispatch_async(self->_queue, ^{
        [self finishRecording];
      });
    }];
    // If ScreenCaptureKit never answers, finish anyway.
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 3 * NSEC_PER_SEC), self->_queue, ^{
      [self finishRecording];
    });
  });
}

- (void)failNow:(std::string)message {
  _failure = std::move(message);
  [self finishRecording];
}

// On _queue. Idempotent.
- (void)finishRecording {
  if (_finalizing) return;
  _finalizing = YES;
  if (_audioTimer != nil) {
    dispatch_source_cancel(_audioTimer);
    _audioTimer = nil;
  }
  [_micSession stopRunning];
  if (_stream != nil && !_stopRequested) [_stream stopCaptureWithCompletionHandler:^(NSError*){}];
  const RecTime end = std::max(_clock->now(), _lastFrame + _pacer->interval());
  log::info("recording finished: {} frames in {:.1f}s, {} dropped by the encoder", _frames,
            std::chrono::duration<double>(end).count(), _dropped);

  if (_failure || _frames == 0) {
    std::string message = _failure.value_or("No frames were captured. Check Screen Recording permission.");
    if (_writer != nil) [_writer cancelWriting];
    _gif.reset();
    [NSFileManager.defaultManager removeItemAtURL:mac::file_url(_cfg.path) error:nil];
    return [self report:fail(message)];
  }
  if (_gif) {
    auto frames = _gif->finish(end);
    _gif.reset();
    if (!frames) return [self report:fail(frames.error().message)];
    return [self report:RecordingResult{_cfg.path, end, _warnings}];
  }
  [self pumpAudio:YES];
  [_video markAsFinished];
  [_audio markAsFinished];
  // The last frame lasts until the moment recording stopped.
  [_writer endSessionAtSourceTime:to_cmtime(end)];
  RecTime duration = end - _sessionStart.value_or(RecTime{0});
  [_writer finishWritingWithCompletionHandler:^{
    dispatch_async(self->_queue, ^{
      if (self->_writer.status == AVAssetWriterStatusCompleted) {
        [self report:RecordingResult{self->_cfg.path, duration, self->_warnings}];
      } else {
        [NSFileManager.defaultManager removeItemAtURL:mac::file_url(self->_cfg.path) error:nil];
        [self report:fail(std::format("Could not write the video: {}",
                                      mac::str(self->_writer.error.localizedDescription)))];
      }
    });
  }];
}

- (void)report:(Result<RecordingResult>)result {
  RecordingDone done = std::move(_done);
  _done = nullptr;
  _stream = nil;
  if (!done) return;
  dispatch_async(dispatch_get_main_queue(), ^{
    done(result);
  });
}

@end

// ---------------------------------------------------------------------------------------------

namespace snapcap::mac {
namespace {

class MacRecorder final : public Recorder {
 public:
  explicit MacRecorder(SNCRecordingSession* session) : session_(session), clock_(session.clock) {}
  // Destroying the recorder stops it; the session finishes the file on its own queue.
  ~MacRecorder() override { [session_ stop]; }
  void stop() override { [session_ stop]; }
  void toggle_pause() override { clock_->toggle_pause(); }
  bool paused() const override { return clock_->paused(); }
  RecTime elapsed() const override { return clock_->now(); }

 private:
  SNCRecordingSession* session_;
  std::shared_ptr<RecClock> clock_;
};

}  // namespace

Result<std::unique_ptr<Recorder>> start_recorder(const RecordConfig& config, RecordingDone done) {
  if (config.out_width < 2 || config.out_height < 2) return fail("The recording area is too small");
  SNCRecordingSession* session = [[SNCRecordingSession alloc] initWithConfig:config done:std::move(done)];
  [session start];
  return std::unique_ptr<Recorder>(new MacRecorder(session));
}

std::vector<std::string> microphone_names() {
  std::vector<std::string> out;
  for (AVCaptureDevice* d in [SNCRecordingSession microphones]) out.push_back(str(d.localizedName));
  return out;
}

}  // namespace snapcap::mac
