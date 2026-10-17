use std::cell::Cell;
use std::time::{Duration, Instant};

/// Driver callbacks must continue even during silence and pause. A stopped
/// callback can leave a full ring buffer forever without a CPAL error.
pub(super) struct CallbackWatch {
    seen: Cell<u64>,
    changed: Cell<Instant>,
}

pub(super) const CALLBACK_TIMEOUT: Duration = Duration::from_secs(3);

impl CallbackWatch {
    pub(super) fn new(now: Instant) -> Self {
        Self { seen: Cell::new(0), changed: Cell::new(now) }
    }

    pub(super) fn healthy(&self, callbacks: u64, now: Instant) -> bool {
        if self.seen.replace(callbacks) != callbacks {
            self.changed.set(now);
        }
        now.saturating_duration_since(self.changed.get()) < CALLBACK_TIMEOUT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_driver_that_never_calls_back_times_out() {
        let now = Instant::now();
        let watch = CallbackWatch::new(now);
        assert!(watch.healthy(0, now + Duration::from_secs(2)));
        assert!(!watch.healthy(0, now + CALLBACK_TIMEOUT));
    }

    #[test]
    fn a_driver_that_stops_without_an_error_times_out() {
        let now = Instant::now();
        let watch = CallbackWatch::new(now);
        assert!(watch.healthy(10, now + Duration::from_secs(1)));
        assert!(watch.healthy(10, now + Duration::from_secs(3)));
        assert!(!watch.healthy(10, now + Duration::from_secs(4)));
    }

    #[test]
    fn callbacks_during_pause_or_empty_audio_keep_the_device_healthy() {
        let now = Instant::now();
        let watch = CallbackWatch::new(now);
        for seconds in 1..100 {
            assert!(watch.healthy(seconds, now + Duration::from_secs(seconds)));
        }
    }
}
