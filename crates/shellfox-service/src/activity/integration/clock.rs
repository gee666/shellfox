use std::time::{Duration, Instant};

/// One service-owned monotonic anchor, shared by input, output, and quiet timers.
/// The caller chooses its u64 origin; this type never reads or changes wall time.
pub struct ActivityClock {
    instant: Instant,
    logical_ms: u64,
}
impl ActivityClock {
    pub fn new(instant: Instant, logical_ms: u64) -> Self {
        Self {
            instant,
            logical_ms,
        }
    }
    pub fn at(&self, instant: Instant) -> Option<u64> {
        if instant >= self.instant {
            self.logical_ms
                .checked_add(u64::try_from(instant.duration_since(self.instant).as_millis()).ok()?)
        } else {
            self.logical_ms
                .checked_sub(u64::try_from(self.instant.duration_since(instant).as_millis()).ok()?)
        }
    }
    pub fn now_ms(&self) -> Option<u64> {
        self.at(Instant::now())
    }
    pub fn instant_at(&self, logical_ms: u64) -> Option<Instant> {
        if logical_ms >= self.logical_ms {
            self.instant
                .checked_add(Duration::from_millis(logical_ms - self.logical_ms))
        } else {
            self.instant
                .checked_sub(Duration::from_millis(self.logical_ms - logical_ms))
        }
    }
}
/// Explicit UTC conversion at the persistence boundary. A service may anchor its
/// logical origin at UTC milliseconds so preexisting SessionAging values align.
#[derive(Clone, Copy)]
pub struct ActivityUtcAnchor {
    pub logical_ms: u64,
    pub unix_ms: u64,
}
impl ActivityUtcAnchor {
    pub fn to_unix_ms(self, logical_ms: u64) -> Option<u64> {
        let unix = if logical_ms >= self.logical_ms {
            self.unix_ms.checked_add(logical_ms - self.logical_ms)
        } else {
            self.unix_ms.checked_sub(self.logical_ms - logical_ms)
        }?;
        (unix <= 8_640_000_000_000_000).then_some(unix)
    }
    pub fn from_unix_ms(self, unix_ms: u64) -> Option<u64> {
        if unix_ms > 8_640_000_000_000_000 {
            return None;
        }
        if unix_ms >= self.unix_ms {
            self.logical_ms.checked_add(unix_ms - self.unix_ms)
        } else {
            self.logical_ms.checked_sub(self.unix_ms - unix_ms)
        }
    }
}
