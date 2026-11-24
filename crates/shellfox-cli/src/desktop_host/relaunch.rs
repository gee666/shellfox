//! How often the host brings a crashed UI back. A GPU driver reset or a
//! crash mid-work must not leave the user without a window, but a UI that
//! crashes at every start must not loop forever.
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const LIMIT: usize = 3;
const WINDOW: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
pub(super) struct Relaunches(VecDeque<Instant>);
impl Relaunches {
    /// The wait before the next relaunch, or None once the limit is reached.
    /// The wait grows so a resetting GPU driver has time to come back.
    pub fn next(&mut self, now: Instant) -> Option<Duration> {
        while self
            .0
            .front()
            .is_some_and(|at| now.duration_since(*at) >= WINDOW)
        {
            self.0.pop_front();
        }
        if self.0.len() >= LIMIT {
            return None;
        }
        let wait = [1, 3, 10][self.0.len()];
        self.0.push_back(now);
        Some(Duration::from_secs(wait))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn three_relaunches_per_five_minutes_with_growing_waits() {
        let start = Instant::now();
        let mut relaunches = Relaunches::default();
        assert_eq!(relaunches.next(start), Some(Duration::from_secs(1)));
        assert_eq!(relaunches.next(start), Some(Duration::from_secs(3)));
        assert_eq!(relaunches.next(start), Some(Duration::from_secs(10)));
        assert_eq!(relaunches.next(start + Duration::from_secs(60)), None);
        let later = start + WINDOW;
        assert_eq!(relaunches.next(later), Some(Duration::from_secs(1)));
    }
}
