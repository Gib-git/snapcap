#include "snapcap/audio_mix.h"

#include <algorithm>
#include <cmath>

namespace snapcap {
namespace {

constexpr Duration kLatency = std::chrono::milliseconds(120);
// A source further ahead than this has drifted; its oldest samples are dropped.
constexpr size_t kMaxBacklogFrames = kAudioRate / 2;

}  // namespace

void SampleQueue::append(std::span<const float> interleaved) {
  std::lock_guard lock(mutex_);
  samples_.insert(samples_.end(), interleaved.begin(), interleaved.end());
}

size_t SampleQueue::mix_into(std::span<float> out, size_t frames) {
  std::lock_guard lock(mutex_);
  size_t take = std::min({samples_.size() / 2, frames, out.size() / 2});
  for (size_t i = 0; i < take * 2; ++i) out[i] += samples_[i];
  samples_.erase(samples_.begin(), samples_.begin() + static_cast<std::ptrdiff_t>(take * 2));
  return take;
}

void SampleQueue::trim(size_t max_frames) {
  std::lock_guard lock(mutex_);
  size_t frames = samples_.size() / 2;
  if (frames > max_frames) {
    samples_.erase(samples_.begin(), samples_.begin() + static_cast<std::ptrdiff_t>((frames - max_frames) * 2));
  }
}

size_t SampleQueue::frames() const {
  std::lock_guard lock(mutex_);
  return samples_.size() / 2;
}

AudioConverter::AudioConverter(uint32_t sample_rate, uint32_t channels, std::shared_ptr<SampleQueue> queue,
                               std::shared_ptr<const RecClock> clock)
    : channels_(std::max(1u, channels)),
      step_(double(sample_rate) / kAudioRate),
      queue_(std::move(queue)),
      clock_(std::move(clock)) {
  scratch_.reserve(8192);
}

void AudioConverter::emit(float l, float r) {
  if (std::abs(step_ - 1.0) < 1e-9) {
    scratch_.push_back(l);
    scratch_.push_back(r);
    return;
  }
  // Output samples that fall between the previous input frame and this one.
  while (pos_ < 1.0) {
    auto t = static_cast<float>(pos_);
    scratch_.push_back(prev_[0] + (l - prev_[0]) * t);
    scratch_.push_back(prev_[1] + (r - prev_[1]) * t);
    pos_ += step_;
  }
  pos_ -= 1.0;
  prev_[0] = l;
  prev_[1] = r;
}

void AudioConverter::push_interleaved(std::span<const float> samples) {
  if (clock_ && clock_->paused()) return;
  scratch_.clear();
  for (size_t i = 0; i + channels_ <= samples.size(); i += channels_) {
    float l = samples[i];
    float r = channels_ == 1 ? l : samples[i + 1];
    emit(l, r);
  }
  queue_->append(scratch_);
}

void AudioConverter::push_planar(std::span<const std::span<const float>> channels) {
  if (clock_ && clock_->paused()) return;
  if (channels.empty()) return;
  scratch_.clear();
  size_t frames = channels[0].size();
  for (const auto& c : channels) frames = std::min(frames, c.size());
  for (size_t i = 0; i < frames; ++i) {
    float l = channels[0][i];
    float r = channels.size() == 1 ? l : channels[1][i];
    emit(l, r);
  }
  queue_->append(scratch_);
}

void AudioMixer::add_source(std::shared_ptr<SampleQueue> queue) {
  sources_.push_back(std::move(queue));
}

void AudioMixer::pump(Duration now, const ChunkSink& sink) {
  produce_until(std::max(now - kLatency, Duration{0}), sink);
}

void AudioMixer::finish(Duration end, const ChunkSink& sink) {
  produce_until(end, sink);
}

void AudioMixer::produce_until(Duration t, const ChunkSink& sink) {
  auto target = static_cast<uint64_t>(std::chrono::duration<double>(t).count() * kAudioRate);
  while (produced_ + kMixFrames <= target) {
    std::ranges::fill(mix_, 0.0f);
    for (auto& src : sources_) {
      src->trim(kMaxBacklogFrames);
      src->mix_into(mix_, kMixFrames);
    }
    for (float& s : mix_) s = std::clamp(s, -1.0f, 1.0f);
    sink(mix_, produced_);
    produced_ += kMixFrames;
  }
}

}  // namespace snapcap
