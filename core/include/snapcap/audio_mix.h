// Audio mixing for recordings.
//
// Every source is converted to 48 kHz stereo float in its capture callback and queued.
// The mixer runs on the recording clock: it emits exactly as many samples as recording
// time has elapsed (minus a small latency), padding silent or stalled sources with zeros.
// That keeps audio locked to video even when a loopback device delivers nothing during
// silence.
#pragma once

#include <cstddef>
#include <cstdint>
#include <deque>
#include <functional>
#include <memory>
#include <mutex>
#include <span>
#include <vector>

#include "snapcap/clock.h"

namespace snapcap {

inline constexpr uint32_t kAudioRate = 48'000;
// Frames per mixed chunk (one AAC frame).
inline constexpr size_t kMixFrames = 1024;

// Thread-safe queue of interleaved stereo samples.
class SampleQueue {
 public:
  void append(std::span<const float> interleaved);
  // Moves up to `frames` stereo frames into `out` (added to what is there); returns frames taken.
  size_t mix_into(std::span<float> out, size_t frames);
  // Drops the oldest frames beyond `max_frames`, so a source that ran ahead cannot drift.
  void trim(size_t max_frames);
  size_t frames() const;

 private:
  mutable std::mutex mutex_;
  std::deque<float> samples_;
};

// Per-source channel mapping and linear resampling to 48 kHz stereo.
class AudioConverter {
 public:
  // `clock` may be null; when set, samples arriving while it is paused are discarded.
  AudioConverter(uint32_t sample_rate, uint32_t channels, std::shared_ptr<SampleQueue> queue,
                 std::shared_ptr<const RecClock> clock);
  // Interleaved samples, `frames * channels` long.
  void push_interleaved(std::span<const float> samples);
  // Non-interleaved: one span per channel, each `frames` long.
  void push_planar(std::span<const std::span<const float>> channels);

 private:
  void emit(float l, float r);

  uint32_t channels_;
  double step_;
  double pos_ = 0.0;
  float prev_[2] = {0.0f, 0.0f};
  std::vector<float> scratch_;
  std::shared_ptr<SampleQueue> queue_;
  std::shared_ptr<const RecClock> clock_;
};

class AudioMixer {
 public:
  using ChunkSink = std::function<void(std::span<const float> interleaved_stereo, uint64_t first_frame)>;

  void add_source(std::shared_ptr<SampleQueue> queue);
  // Mixes everything due by recording time `now` (minus the latency allowance).
  void pump(Duration now, const ChunkSink& sink);
  // Mixes everything up to the final recording time.
  void finish(Duration end, const ChunkSink& sink);
  uint64_t produced_frames() const { return produced_; }

 private:
  void produce_until(Duration t, const ChunkSink& sink);

  std::vector<std::shared_ptr<SampleQueue>> sources_;
  std::vector<float> mix_ = std::vector<float>(kMixFrames * 2);
  uint64_t produced_ = 0;
};

}  // namespace snapcap
