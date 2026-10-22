#include "snapcap/clock.h"

#include <algorithm>
#include <cmath>

namespace snapcap {

RecClock::RecClock(Source source) : source_(std::move(source)), start_(source_()) {}

Duration RecClock::elapsed_at(TimePoint at) const {
  TimePoint end = paused_at_.value_or(at);
  Duration d = std::chrono::duration_cast<Duration>(end - start_) - paused_total_;
  return std::max(d, Duration{0});
}

Duration RecClock::now() const {
  TimePoint t = source_();
  std::lock_guard lock(mutex_);
  return elapsed_at(t);
}

std::optional<Duration> RecClock::live_now() const {
  TimePoint t = source_();
  std::lock_guard lock(mutex_);
  if (paused_at_) return std::nullopt;
  return elapsed_at(t);
}

std::optional<Duration> RecClock::live_at(TimePoint at) const {
  std::lock_guard lock(mutex_);
  if (paused_at_ || at < start_) return std::nullopt;
  return elapsed_at(at);
}

bool RecClock::paused() const {
  std::lock_guard lock(mutex_);
  return paused_at_.has_value();
}

void RecClock::pause() {
  TimePoint t = source_();
  std::lock_guard lock(mutex_);
  if (!paused_at_) paused_at_ = t;
}

void RecClock::resume() {
  TimePoint t = source_();
  std::lock_guard lock(mutex_);
  if (paused_at_) {
    paused_total_ += std::chrono::duration_cast<Duration>(t - *paused_at_);
    paused_at_.reset();
  }
}

void RecClock::toggle_pause() {
  TimePoint t = source_();
  std::lock_guard lock(mutex_);
  if (paused_at_) {
    paused_total_ += std::chrono::duration_cast<Duration>(t - *paused_at_);
    paused_at_.reset();
  } else {
    paused_at_ = t;
  }
}

Pacer::Pacer(unsigned fps) : interval_(Duration{1'000'000'000 / std::max(1u, fps)}) {}

std::optional<Duration> Pacer::admit(Duration t) {
  auto slot = static_cast<int64_t>(std::llround(double(t.count()) / double(interval_.count())));
  if (last_slot_ && slot <= *last_slot_) return std::nullopt;
  last_slot_ = slot;
  return interval_ * slot;
}

}  // namespace snapcap
