use crate::{RedrawMode, scheduler::Schedule};
use std::time::{Duration, Instant};
#[test]
fn callback_invalidations_survive_submission_and_skip() {
    let now = Instant::now();
    let mut s = Schedule::default();
    assert!(s.request_native(now, true));
    assert!(!s.request_native(now, true));
    s.begin_redraw();
    s.invalidate();
    s.submitted();
    assert!(s.request_native(now, true));
    s.begin_redraw();
    s.mode = RedrawMode::Continuous;
    // prepare Skip never calls submitted, so it does not spin in Continuous.
    assert!(!s.request_native(now, true));
    s.invalidate();
    assert!(s.request_native(now, true));
}
#[test]
fn deadline_is_earliest_one_shot_and_independent_of_immediate_draw() {
    let now = Instant::now();
    let early = now + Duration::from_millis(10);
    let later = now + Duration::from_millis(20);
    let mut s = Schedule::default();
    s.at(later);
    s.at(early);
    s.at(later);
    s.begin_redraw();
    s.submitted();
    assert_eq!(s.wake(true), Some(early));
    s.advance(early);
    assert_eq!(s.wake(true), None);
    assert!(s.request_native(early, true));
    s.at(later);
    s.cancel_deadline();
    assert_eq!(s.wake(true), None);
}
#[test]
fn unavailable_windows_keep_work_without_timed_spinning() {
    let now = Instant::now();
    let mut s = Schedule::default();
    s.at(now);
    s.advance(now);
    assert!(!s.request_native(now, false));
    assert_eq!(s.wake(false), None);
    assert!(s.request_native(now, true));
    s.begin_redraw();
    s.suspended();
    assert_eq!(s.wake(true), None);
    assert!(!s.request_native(now, true));
    s.invalidate();
    assert!(s.request_native(now, true));
    s.begin_redraw();
    s.mode = RedrawMode::Continuous;
    s.submitted();
    assert!(!s.request_native(now, false));
    s.availability_changed();
    assert!(s.request_native(now, true));
}
#[test]
fn retries_are_paced_and_recovery_guard_resets_only_on_submission() {
    let now = Instant::now();
    let delay = Duration::from_millis(16);
    let mut s = Schedule::default();
    s.begin_redraw();
    s.retry(now, delay);
    s.invalidate();
    assert!(!s.request_native(now, true));
    assert_eq!(s.wake(true), Some(now + delay));
    assert_eq!(s.wake(false), None);
    assert!(s.request_native(now + delay, true));
    assert!(s.lost(2));
    s.begin_redraw();
    assert!(s.lost(2));
    assert!(!s.lost(2));
    s.submitted();
    assert!(s.lost(2));
}

#[test]
fn independent_windows_do_not_inherit_continuous_mode_or_deadlines() {
    let now = Instant::now();
    let mut animated = Schedule::default();
    let mut idle = Schedule::default();
    animated.begin_redraw();
    idle.begin_redraw();
    animated.mode(RedrawMode::Continuous);
    animated.submitted();
    idle.submitted();
    assert!(animated.request_native(now, true));
    assert!(!idle.request_native(now, true));
    animated.at(now + Duration::from_millis(10));
    assert!(animated.wake(true).is_some());
    assert_eq!(idle.wake(true), None);
}

#[test]
fn recovery_counter_cannot_wrap_even_at_maximum_limit() {
    let mut s = Schedule::default();
    s.recoveries = u32::MAX - 1;
    assert!(s.lost(u32::MAX));
    assert!(!s.lost(u32::MAX));
}

#[test]
fn persistent_surface_loss_backs_off_but_stays_schedulable() {
    let mut schedule = Schedule::default();
    let now = Instant::now();
    let base = Duration::from_millis(16);
    for attempt in 1..=100 {
        schedule.lost(3);
        let delay = schedule.recovery_delay(base, 3);
        if attempt <= 3 {
            assert_eq!(delay, base);
        }
        if attempt > 9 {
            assert_eq!(delay, Duration::from_secs(1));
        }
        schedule.retry(now, delay);
        schedule.invalidate();
        assert!(!schedule.request_native(now, true));
        assert!(!schedule.request_native(now + delay, false));
        assert!(schedule.request_native(now + delay, true));
        schedule.begin_redraw();
    }
    schedule.submitted();
    assert_eq!(schedule.recovery_delay(base, 3), base);
    schedule.recoveries = u32::MAX;
    assert_eq!(schedule.recovery_delay(Duration::MAX, 3), Duration::MAX);
}
