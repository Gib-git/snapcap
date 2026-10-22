#include "capture.h"

#import <CoreImage/CoreImage.h>
#import <CoreMedia/CoreMedia.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>

#include <cmath>
#include <format>
#include <memory>
#include <mutex>
#include <optional>
#include <unistd.h>

#include "mac_util.h"
#include "snapcap/log.h"

using namespace snapcap;

// macOS 13: no SCScreenshotManager, so take the first complete frame of a short stream.
API_AVAILABLE(macos(12.3))
@interface SNCOneFrame : NSObject <SCStreamOutput, SCStreamDelegate>
- (void)captureWithFilter:(SCContentFilter*)filter
            configuration:(SCStreamConfiguration*)config
               completion:(void (^)(CGImageRef image, NSString* error))completion;
@end

@implementation SNCOneFrame {
  SCStream* _stream;
  void (^_completion)(CGImageRef, NSString*);
  dispatch_queue_t _queue;
  BOOL _done;
}

- (void)captureWithFilter:(SCContentFilter*)filter
            configuration:(SCStreamConfiguration*)config
               completion:(void (^)(CGImageRef, NSString*))completion {
  _completion = completion;
  _queue = dispatch_queue_create("dev.snapcap.oneframe", DISPATCH_QUEUE_SERIAL);
  _stream = [[SCStream alloc] initWithFilter:filter configuration:config delegate:self];
  NSError* error = nil;
  if (![_stream addStreamOutput:self type:SCStreamOutputTypeScreen sampleHandlerQueue:_queue error:&error]) {
    [self finish:nullptr error:error.localizedDescription];
    return;
  }
  [_stream startCaptureWithCompletionHandler:^(NSError* err) {
    if (err != nil) dispatch_async(self->_queue, ^{ [self finish:nullptr error:err.localizedDescription]; });
  }];
  dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 3 * NSEC_PER_SEC), _queue, ^{
    [self finish:nullptr error:@"timed out"];
  });
}

- (void)stream:(SCStream*)stream didOutputSampleBuffer:(CMSampleBufferRef)buffer ofType:(SCStreamOutputType)type {
  if (type != SCStreamOutputTypeScreen || _done) return;
  CFArrayRef attachments = CMSampleBufferGetSampleAttachmentsArray(buffer, false);
  if (attachments == nullptr || CFArrayGetCount(attachments) == 0) return;
  NSDictionary* info = (__bridge NSDictionary*)CFArrayGetValueAtIndex(attachments, 0);
  NSNumber* status = info[SCStreamFrameInfoStatus];
  if (status == nil || status.integerValue != SCFrameStatusComplete) return;
  CVPixelBufferRef pixels = CMSampleBufferGetImageBuffer(buffer);
  if (pixels == nullptr) return;
  CIImage* ci = [CIImage imageWithCVPixelBuffer:pixels];
  CIContext* ctx = [CIContext contextWithOptions:nil];
  CGImageRef image = [ctx createCGImage:ci fromRect:ci.extent];
  [self finish:image error:nil];
  if (image != nullptr) CGImageRelease(image);
}

- (void)stream:(SCStream*)stream didStopWithError:(NSError*)error {
  dispatch_async(_queue, ^{ [self finish:nullptr error:error.localizedDescription]; });
}

// Runs on _queue.
- (void)finish:(CGImageRef)image error:(NSString*)error {
  if (_done) return;
  _done = YES;
  _completion(image, error ?: (image ? nil : @"no frame"));
  _completion = nil;
  [_stream stopCaptureWithCompletionHandler:^(NSError*) {}];
}

@end

namespace snapcap::mac {
namespace {

struct Batch {
  std::mutex mutex;
  std::vector<std::optional<Shot>> shots;
  std::optional<std::string> error;
  std::function<void(Result<std::vector<Shot>>)> done;
};

void capture_one(SCContentFilter* filter, SCStreamConfiguration* config, std::function<void(CGImageRef, NSString*)> cb) {
  if (@available(macOS 14.0, *)) {
    [SCScreenshotManager captureImageWithFilter:filter
                                  configuration:config
                              completionHandler:^(CGImageRef image, NSError* error) {
                                cb(image, error.localizedDescription);
                              }];
  } else {
    SNCOneFrame* grabber = [[SNCOneFrame alloc] init];
    // The block keeps the grabber alive until it reports.
    [grabber captureWithFilter:filter
                 configuration:config
                    completion:^(CGImageRef image, NSString* error) {
                      cb(image, error);
                      (void)grabber;
                    }];
  }
}

}  // namespace

void capture_displays(std::vector<MonitorInfo> which, std::function<void(Result<std::vector<Shot>>)> done) {
  auto batch = std::make_shared<Batch>();
  batch->shots.resize(which.size());
  batch->done = std::move(done);
  auto finish = [batch] {
    on_main([batch] {
      std::vector<Shot> out;
      for (auto& s : batch->shots) {
        if (s) out.push_back(std::move(*s));
      }
      // Some displays (e.g. a virtual one) may be uncapturable; that's fine if others worked.
      if (out.empty() || (batch->error && batch->shots.size() == 1)) {
        return batch->done(fail(batch->error.value_or("Nothing could be captured")));
      }
      if (batch->error) log::warn("{}", *batch->error);
      batch->done(std::move(out));
    });
  };

  [SCShareableContent getShareableContentExcludingDesktopWindows:NO
                                             onScreenWindowsOnly:YES
                                               completionHandler:^(SCShareableContent* content, NSError* error) {
    if (content == nil) {
      batch->error = std::format("The screen could not be captured ({}). Check Screen Recording permission.",
                                 str(error.localizedDescription));
      return finish();
    }
    if (content.displays.count == 0) {
      batch->error = "There is no display to capture. Is the screen locked or asleep?";
      return finish();
    }
    NSMutableArray<SCRunningApplication*>* own = [NSMutableArray array];
    for (SCRunningApplication* app in content.applications) {
      if (app.processID == getpid()) [own addObject:app];
    }
    dispatch_group_t group = dispatch_group_create();
    for (size_t i = 0; i < which.size(); ++i) {
      const MonitorInfo m = which[i];
      SCDisplay* display = nil;
      for (SCDisplay* d in content.displays) {
        if (d.displayID == m.id) display = d;
      }
      if (display == nil) {
        std::lock_guard lock(batch->mutex);
        batch->error = std::format("“{}” can't be captured", m.name);
        continue;
      }
      SCContentFilter* filter = [[SCContentFilter alloc] initWithDisplay:display
                                                   excludingApplications:own
                                                        exceptingWindows:@[]];
      SCStreamConfiguration* config = [[SCStreamConfiguration alloc] init];
      config.width = static_cast<size_t>(std::lround(display.width * m.scale));
      config.height = static_cast<size_t>(std::lround(display.height * m.scale));
      config.showsCursor = NO;
      config.pixelFormat = kCVPixelFormatType_32BGRA;
      config.colorSpaceName = kCGColorSpaceSRGB;
      if (@available(macOS 14.0, *)) config.captureResolution = SCCaptureResolutionBest;
      dispatch_group_enter(group);
      capture_one(filter, config, [batch, group, i, m](CGImageRef image, NSString* err) {
        RgbaImage pixels = from_cgimage(image);
        {
          std::lock_guard lock(batch->mutex);
          if (pixels.empty()) {
            batch->error = std::format("Could not capture “{}”: {}", m.name, err ? str(err) : "empty image");
          } else {
            batch->shots[i] = Shot{m, std::move(pixels)};
          }
        }
        dispatch_group_leave(group);
      });
    }
    dispatch_group_notify(group, dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
      finish();
    });
  }];
}

}  // namespace snapcap::mac
