//! Recording clock shared by capture sources, audio and encoders.
//! Time stops while paused, so pausing leaves no gap in the output.

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RecClock {
    start: Instant,
    state: Mutex<State>,
}

struct State {
    paused_at: Option<Instant>,
    paused_total: Duration,
}

impl Default for RecClock {
    fn default() -> Self {
        Self::new()
    }
}

impl RecClock {
    pub fn new() -> Self {
        Self { start: Instant::now(), state: Mutex::new(State { paused_at: None, paused_total: Duration::ZERO }) }
    }

    fn elapsed_at(&self, s: &State, at: Instant) -> Duration {
        let end = s.paused_at.unwrap_or(at);
        end.saturating_duration_since(self.start).saturating_sub(s.paused_total)
    }

    /// Recording time now (frozen while paused).
    pub fn now(&self) -> Duration {
        let s = self.state.lock().unwrap();
        self.elapsed_at(&s, Instant::now())
    }

    /// Recording time now, or `None` while paused (the sample should be dropped).
    pub fn live_now(&self) -> Option<Duration> {
        let s = self.state.lock().unwrap();
        if s.paused_at.is_some() {
            None
        } else {
            Some(self.elapsed_at(&s, Instant::now()))
        }
    }

    pub fn is_paused(&self) -> bool {
        self.state.lock().unwrap().paused_at.is_some()
    }

    pub fn pause(&self) {
        let mut s = self.state.lock().unwrap();
        if s.paused_at.is_none() {
            s.paused_at = Some(Instant::now());
        }
    }

    pub fn resume(&self) {
        let mut s = self.state.lock().unwrap();
        if let Some(at) = s.paused_at.take() {
            s.paused_total += at.elapsed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_freezes_time() {
        let c = RecClock::new();
        std::thread::sleep(Duration::from_millis(20));
        c.pause();
        let a = c.now();
        assert!(c.live_now().is_none());
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(c.now(), a);
        c.resume();
        let b = c.live_now().unwrap();
        assert!(b >= a && b < a + Duration::from_millis(20), "{a:?} {b:?}");
    }
}
