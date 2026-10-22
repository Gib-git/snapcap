// Recording clock shared by capture sources, audio and encoders. Time stops while
// paused, so pausing leaves no gap in the output.
#pragma once

#include <chrono>
#include <functional>
#include <mutex>
#include <optional>

namespace snapcap {

using Duration = std::chrono::nanoseconds;

class RecClock {
 public:
  using TimePoint = std::chrono::steady_clock::time_point;
  using Source = std::function<TimePoint()>;

  // `source` exists for tests; production uses the steady clock.
  explicit RecClock(Source source = [] { return std::chrono::steady_clock::now(); });

  // Recording time now (frozen while paused).
  Duration now() const;
  // Recording time now, or nothing while paused (the sample should be dropped).
  std::optional<Duration> live_now() const;
  // Recording time at a host time point, or nothing if that moment is before the start or
  // the clock is paused. Used to map capture timestamps onto the recording timeline.
  std::optional<Duration> live_at(TimePoint at) const;
  bool paused() const;
  void pause();
  void resume();
  void toggle_pause();

 private:
  Duration elapsed_at(TimePoint at) const;  // requires mutex_

  Source source_;
  TimePoint start_;
  mutable std::mutex mutex_;
  std::optional<TimePoint> paused_at_;
  Duration paused_total_{0};
};

// Rate limiter for capture sources. Timestamps are snapped to a fixed frame grid so capture
// jitter never produces near-duplicate timestamps; a frame landing in a used slot is dropped.
class Pacer {
 public:
  explicit Pacer(unsigned fps);
  Duration interval() const { return interval_; }
  // The grid-aligned timestamp for a frame captured at `t`, or nothing to skip it.
  std::optional<Duration> admit(Duration t);

 private:
  Duration interval_;
  std::optional<int64_t> last_slot_;
};

}  // namespace snapcap
