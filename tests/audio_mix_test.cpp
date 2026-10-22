#include <cmath>
#include <numbers>
#include <vector>

#include <doctest/doctest.h>

#include "snapcap/audio_mix.h"

using namespace snapcap;
using namespace std::chrono_literals;

TEST_CASE("resamples 44.1 kHz mono to 48 kHz stereo") {
  auto q = std::make_shared<SampleQueue>();
  AudioConverter c(44'100, 1, q, nullptr);
  std::vector<float> second(44'100, 0.5f);
  c.push_interleaved(second);
  CHECK(q->frames() >= 47'990);
  CHECK(q->frames() <= 48'010);
}

TEST_CASE("passes 48 kHz stereo through unchanged") {
  auto q = std::make_shared<SampleQueue>();
  AudioConverter c(48'000, 2, q, nullptr);
  std::vector<float> in{0.1f, 0.2f, 0.3f, 0.4f};
  c.push_interleaved(in);
  std::vector<float> out(4, 0.0f);
  CHECK(q->mix_into(out, 2) == 2);
  CHECK(out == in);
}

TEST_CASE("planar input and paused clock") {
  auto q = std::make_shared<SampleQueue>();
  auto clock = std::make_shared<RecClock>();
  AudioConverter c(48'000, 2, q, clock);
  std::vector<float> l{1, 2}, r{3, 4};
  std::vector<std::span<const float>> planes{l, r};
  c.push_planar(planes);
  CHECK(q->frames() == 2);
  clock->pause();
  c.push_planar(planes);
  CHECK(q->frames() == 2);
}

TEST_CASE("mixer emits audio in step with the clock and pads silence") {
  auto tone = std::make_shared<SampleQueue>();
  std::vector<float> samples;
  for (uint32_t i = 0; i < kAudioRate; ++i) {
    auto s = static_cast<float>(std::sin(i * 440.0 * 2 * std::numbers::pi / kAudioRate) * 0.3);
    samples.push_back(s);
    samples.push_back(s);
  }
  tone->append(samples);
  auto silent = std::make_shared<SampleQueue>();

  AudioMixer mixer;
  mixer.add_source(tone);
  mixer.add_source(silent);
  size_t chunks = 0;
  uint64_t expected_first = 0;
  bool ordered = true;
  auto sink = [&](std::span<const float> pcm, uint64_t first) {
    ordered = ordered && first == expected_first && pcm.size() == kMixFrames * 2;
    expected_first += kMixFrames;
    ++chunks;
  };
  mixer.pump(1s, sink);  // latency holds back the last 120 ms
  CHECK(mixer.produced_frames() == ((kAudioRate - kAudioRate * 120 / 1000) / kMixFrames) * kMixFrames);
  mixer.finish(2s, sink);
  CHECK(mixer.produced_frames() == 93 * kMixFrames);
  CHECK(chunks == 93);
  CHECK(ordered);
}

TEST_CASE("a source that runs ahead is trimmed") {
  auto q = std::make_shared<SampleQueue>();
  q->append(std::vector<float>(kAudioRate * 2 * 2, 0.1f));  // 2 s backlog
  AudioMixer mixer;
  mixer.add_source(q);
  mixer.finish(Duration{std::chrono::milliseconds(22)}, [](auto, auto) {});
  CHECK(q->frames() <= kAudioRate / 2);
}
