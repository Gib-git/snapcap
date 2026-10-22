// Screen recording on macOS: ScreenCaptureKit crops and scales on the GPU and supplies
// system audio; AVAssetWriter encodes H.264 (hardware) and AAC and writes the MP4. GIFs go
// through the core GIF writer. The microphone comes from AVCaptureSession, and all audio
// is mixed by snapcap::AudioMixer on the recording clock.
#pragma once

#include <memory>
#include <string>
#include <vector>

#include "snapcap/host.h"

namespace snapcap::mac {

Result<std::unique_ptr<Recorder>> start_recorder(const RecordConfig& config, RecordingDone done);

// Names of the available microphones, for the settings screen.
std::vector<std::string> microphone_names();

}  // namespace snapcap::mac
