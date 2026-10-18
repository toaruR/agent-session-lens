//! Clock offset correction (architecture-v5 §3.2). Pure arithmetic.

use crate::consts::{CLOCK_SKEW_MS, HB_MEDIAN_N};
use std::collections::VecDeque;

/// Result of correcting one helper timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Corrected {
    pub t_ms: i64,
    /// True when a `clock_skew` diagnostic must be emitted.
    pub clock_skew: bool,
}

#[derive(Debug, Default, Clone)]
pub struct ClockOffset {
    samples: VecDeque<i64>,
}

impl ClockOffset {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a sample (hello / heartbeat): offset = recv_ms - helper_now_ms.
    pub fn record(&mut self, recv_ms: i64, helper_now_ms: i64) {
        self.samples.push_back(recv_ms - helper_now_ms);
        while self.samples.len() > HB_MEDIAN_N {
            self.samples.pop_front();
        }
    }

    /// Median of the retained samples (even count: mean of the two middle, floored).
    pub fn offset_ms(&self) -> Option<i64> {
        if self.samples.is_empty() {
            return None;
        }
        let mut v: Vec<i64> = self.samples.iter().copied().collect();
        v.sort_unstable();
        let n = v.len();
        Some(if n % 2 == 1 {
            v[n / 2]
        } else {
            (v[n / 2 - 1] + v[n / 2]).div_euclid(2)
        })
    }

    /// Correct a helper timestamp, then clamp to `now_ms` (no tolerance).
    pub fn correct(&self, t_helper_ms: i64, now_ms: i64) -> Corrected {
        let offset = self.offset_ms().unwrap_or(0);
        let mut skew = false;
        let mut t = t_helper_ms;
        if offset.unsigned_abs() > CLOCK_SKEW_MS {
            t += offset;
            skew = true;
        }
        if t > now_ms {
            t = now_ms;
            skew = true;
        }
        Corrected { t_ms: t, clock_skew: skew }
    }
}
