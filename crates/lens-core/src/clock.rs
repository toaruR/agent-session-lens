//! Injectable time source. All time-dependent logic goes through `Clock`.

use std::sync::Mutex;
use std::time::{Duration, SystemTime};

pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Manually advanced clock for tests.
#[derive(Debug)]
pub struct FakeClock {
    now: Mutex<SystemTime>,
}

impl FakeClock {
    pub fn new(start: SystemTime) -> Self {
        Self { now: Mutex::new(start) }
    }

    pub fn advance(&self, d: Duration) {
        let mut g = self.now.lock().unwrap();
        *g += d;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        *self.now.lock().unwrap()
    }
}
