use crate::RedrawMode;
use std::time::{Duration, Instant};

/// Pure per-window scheduling state. Native requests are emitted by the driver.
pub(crate) struct Schedule {
    pub(crate) mode: RedrawMode,
    dirty: bool,
    pub(super) invalidations: u64,
    queued: bool,
    deadline: Option<Instant>,
    retry: Option<Instant>,
    blocked: bool,
    pub(super) recoveries: u32,
}
impl Default for Schedule {
    fn default() -> Self {
        Self {
            mode: RedrawMode::OnDemand,
            dirty: true,
            invalidations: 0,
            queued: false,
            deadline: None,
            retry: None,
            blocked: false,
            recoveries: 0,
        }
    }
}
impl Schedule {
    pub(crate) fn invalidate(&mut self) {
        self.invalidations = self.invalidations.wrapping_add(1);
        self.dirty = true;
        self.blocked = false;
    }
    pub(crate) fn availability_changed(&mut self) {
        self.invalidate();
        self.queued = false;
        self.retry = None;
    }
    pub(crate) fn at(&mut self, at: Instant) {
        self.deadline = Some(self.deadline.map_or(at, |old| old.min(at)));
    }
    pub(crate) fn cancel_deadline(&mut self) {
        self.deadline = None;
    }
    pub(crate) fn mode(&mut self, mode: RedrawMode) {
        if self.mode != mode {
            self.mode = mode;
            self.invalidate();
        }
    }
    // Consume before callbacks; any callback invalidation belongs to the next draw.
    pub(crate) fn begin_redraw(&mut self) {
        self.queued = false;
        self.dirty = false;
        self.retry = None;
        self.blocked = false;
    }
    pub(crate) fn unavailable(&mut self) {
        self.dirty = true;
        self.queued = false;
    }
    pub(crate) fn suspended(&mut self) {
        self.unavailable();
        self.blocked = true;
        self.retry = None;
    }
    pub(crate) fn retry(&mut self, now: Instant, delay: Duration) {
        self.unavailable();
        self.retry = now.checked_add(delay);
    }
    pub(crate) fn lost(&mut self, limit: u32) -> bool {
        let Some(next) = self.recoveries.checked_add(1) else {
            return false;
        };
        self.recoveries = next;
        self.recoveries <= limit
    }
    pub(crate) fn recovery_delay(&self, delay: Duration, fast_attempts: u32) -> Duration {
        let shift = self.recoveries.saturating_sub(fast_attempts).min(16);
        delay
            .saturating_mul(1 << shift)
            .min(delay.max(Duration::from_secs(1)))
    }
    pub(crate) fn submitted(&mut self) {
        self.recoveries = 0;
        if self.mode == RedrawMode::Continuous {
            self.dirty = true;
        }
    }
    // Due deadlines are consumed even while unavailable. Never spin on old timers.
    pub(crate) fn advance(&mut self, now: Instant) {
        if self.deadline.is_some_and(|at| at <= now) {
            self.deadline = None;
            self.invalidate();
        }
    }
    pub(crate) fn request_native(&mut self, now: Instant, eligible: bool) -> bool {
        if eligible
            && self.dirty
            && !self.queued
            && !self.blocked
            && self.retry.is_none_or(|at| at <= now)
        {
            self.retry = None;
            self.queued = true;
            true
        } else {
            false
        }
    }
    pub(crate) fn wake(&self, eligible: bool) -> Option<Instant> {
        if !eligible {
            return None;
        }
        let retry = if self.dirty && !self.blocked && !self.queued {
            self.retry
        } else {
            None
        };
        match (self.deadline, retry) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}
