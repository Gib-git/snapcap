#include <doctest/doctest.h>

#include "snapcap/clock.h"

using namespace snapcap;
using namespace std::chrono_literals;

namespace {
// A manually advanced clock source.
struct FakeTime {
  RecClock::TimePoint now{};
  RecClock::Source source() {
    return [this] { return now; };
  }
};
}  // namespace

TEST_CASE("pause freezes recording time") {
  FakeTime t;
  RecClock c(t.source());
  t.now += 20ms;
  CHECK(c.now() == 20ms);
  c.pause();
  CHECK(c.paused());
  CHECK_FALSE(c.live_now().has_value());
  t.now += 40ms;
  CHECK(c.now() == 20ms);
  c.resume();
  CHECK(c.live_now() == Duration{20ms});
  t.now += 5ms;
  CHECK(c.now() == 25ms);
  c.toggle_pause();
  CHECK(c.paused());
  t.now += 1s;
  c.toggle_pause();
  t.now += 5ms;
  CHECK(c.now() == 30ms);
}

TEST_CASE("live_at maps host time onto the recording timeline") {
  FakeTime t;
  t.now += 1s;
  RecClock c(t.source());
  CHECK_FALSE(c.live_at(t.now - 1ms).has_value());
  t.now += 100ms;
  c.pause();
  t.now += 50ms;
  c.resume();
  CHECK(c.live_at(t.now + 10ms) == Duration{110ms});
}

TEST_CASE("pacer limits the rate and snaps to the grid") {
  Pacer p(10);
  int kept = 0;
  for (int i = 0; i < 100; ++i) kept += p.admit(Duration{std::chrono::milliseconds(i * 10)}).has_value();
  CHECK(kept >= 9);
  CHECK(kept <= 11);

  Pacer q(10);
  CHECK(q.admit(0ms) == Duration{0ms});
  CHECK(q.admit(98ms) == Duration{100ms});
  CHECK(q.admit(103ms) == std::nullopt);
  CHECK(q.admit(205ms) == Duration{200ms});
}
