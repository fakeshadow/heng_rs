use std::time::Duration;

const fn day(count: u64) -> Duration {
    Duration::from_secs(24 * 60 * 60 * count)
}
const fn hour(count: u64) -> Duration {
    Duration::from_secs(60 * 60 * count)
}
const fn min(count: u64) -> Duration {
    Duration::from_secs(60 * count)
}

pub struct Time {
    dur: Duration,
}

impl Default for Time {
    fn default() -> Self {
        Time {
            dur: Default::default(),
        }
    }
}

impl Time {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn every(mut self, dur: Duration) -> Self {
        self.dur += dur;
        self
    }

    pub fn plus(mut self, dur: Duration) -> Self {
        self.dur += dur;
        self
    }
}

impl From<Time> for Duration {
    fn from(t: Time) -> Duration {
        t.dur
    }
}

pub trait ToDuration: Sized {
    fn into_u64(self) -> u64;

    fn d(self) -> Duration {
        day(self.into_u64())
    }

    fn h(self) -> Duration {
        hour(self.into_u64())
    }

    fn m(self) -> Duration {
        min(self.into_u64())
    }

    fn s(self) -> Duration {
        Duration::from_secs(self.into_u64())
    }

    fn millis(self) -> Duration {
        Duration::from_millis(self.into_u64())
    }

    fn micros(self) -> Duration {
        Duration::from_micros(self.into_u64())
    }

    fn nanos(self) -> Duration {
        Duration::from_nanos(self.into_u64())
    }
}

impl ToDuration for u16 {
    fn into_u64(self) -> u64 {
        self as u64
    }
}

#[cfg(test)]
mod test_time {
    use std::time::Duration;

    use crate::time::{Time, ToDuration};

    #[test]
    fn test_duration() {
        assert_eq!(1.d(), Duration::from_secs(24 * 60 * 60 * 1));
        assert_eq!(2.h(), Duration::from_secs(60 * 60 * 2));
        assert_eq!(3.m(), Duration::from_secs(60 * 3));
        assert_eq!(4.millis(), Duration::from_millis(4));
        assert_eq!(5.micros(), Duration::from_micros(5));
        assert_eq!(6.nanos(), Duration::from_nanos(6));
    }

    #[test]
    fn test_time() {
        let time: Duration = Time::new()
            .every(1.d())
            .plus(2.m())
            .plus(3.h())
            .plus(4.s())
            .into();

        assert_eq!(time, 1.d() + 2.m() + 3.h() + 4.s());
    }
}
