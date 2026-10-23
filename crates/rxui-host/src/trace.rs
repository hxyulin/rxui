//! Opt-in CPU timings. Submission is not a GPU completion/presentation timestamp.
use std::{fmt, time::Instant};
use winit::window::WindowId;
#[derive(Default)]
pub(crate) struct FrameTrace {
    first_input: Option<Instant>,
    last_input: Option<Instant>,
    inputs: u32,
    event_cpu_ms: f64,
    queued: Option<Instant>,
    received: Option<Instant>,
    frame: u64,
    retries: u32,
    availability_changes: u32,
}
#[derive(Debug, PartialEq)]
struct Report {
    acquisition_retries: u32,
    availability_changes: u32,
    frame: u64,
    inputs: u32,
    input_to_prepare_ms: Option<f64>,
    last_input_to_prepare_ms: Option<f64>,
    event_cpu_ms: f64,
    request_to_redraw_ms: Option<f64>,
    input_to_request_ms: Option<f64>,
    redraw_to_prepare_ms: Option<f64>,
    prepare_ms: f64,
    acquire_ms: f64,
    record_ms: f64,
    submit_ms: f64,
    input_to_submit_ms: Option<f64>,
    last_input_to_submit_ms: Option<f64>,
}
fn ms(a: Instant, b: Instant) -> f64 {
    b.saturating_duration_since(a).as_secs_f64() * 1000.
}
impl FrameTrace {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("ASTRELIS_PROFILE").map(|_| Self::default())
    }
    pub fn input(&mut self, start: Instant, end: Instant) {
        self.first_input.get_or_insert(start);
        self.last_input = Some(start);
        self.inputs = self.inputs.saturating_add(1);
        self.event_cpu_ms += ms(start, end);
    }
    pub fn requested(&mut self, now: Instant) {
        self.queued = Some(now);
    }
    pub fn retry(&mut self) {
        self.retries = self.retries.saturating_add(1);
    }
    pub fn availability_changed(&mut self) {
        self.availability_changes = self.availability_changes.saturating_add(1);
    }
    pub fn received(&mut self, now: Instant) {
        self.received = Some(now);
    }
    fn report(
        &mut self,
        start: Instant,
        prepared: Instant,
        acquired: Instant,
        recorded: Instant,
        submitted: Instant,
    ) -> Report {
        self.frame += 1;
        let report = Report {
            acquisition_retries: self.retries,
            availability_changes: self.availability_changes,
            frame: self.frame,
            inputs: self.inputs,
            input_to_prepare_ms: self.first_input.map(|at| ms(at, start)),
            last_input_to_prepare_ms: self.last_input.map(|at| ms(at, start)),
            event_cpu_ms: self.event_cpu_ms,
            input_to_request_ms: self.first_input.zip(self.queued).map(|(a, b)| ms(a, b)),
            request_to_redraw_ms: self.queued.zip(self.received).map(|(a, b)| ms(a, b)),
            redraw_to_prepare_ms: self.received.map(|at| ms(at, start)),
            prepare_ms: ms(start, prepared),
            acquire_ms: ms(prepared, acquired),
            record_ms: ms(acquired, recorded),
            submit_ms: ms(recorded, submitted),
            input_to_submit_ms: self.first_input.map(|at| ms(at, submitted)),
            last_input_to_submit_ms: self.last_input.map(|at| ms(at, submitted)),
        };
        self.retries = 0;
        self.availability_changes = 0;
        self.first_input = None;
        self.last_input = None;
        self.inputs = 0;
        self.event_cpu_ms = 0.;
        self.queued = None;
        self.received = None;
        report
    }
    pub fn submitted(
        &mut self,
        id: WindowId,
        start: Instant,
        prepared: Instant,
        acquired: Instant,
        recorded: Instant,
        submitted: Instant,
    ) {
        let report = self.report(start, prepared, acquired, recorded, submitted);
        eprintln!("RXUI drawing frame: window={id:?}, {report}");
    }
}
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "frame={}, inputs={}, acquisition_retries={}, availability_changes={}, input_to_prepare_ms={:?}, last_input_to_prepare_ms={:?}, event_cpu_ms={:.6}, request_to_redraw_ms={:?}, input_to_request_ms={:?}, redraw_to_prepare_ms={:?}, prepare_ms={:.6}, acquire_ms={:.6}, record_ms={:.6}, submit_ms={:.6}, input_to_submit_ms={:?}, last_input_to_submit_ms={:?}",
            self.frame,
            self.inputs,
            self.acquisition_retries,
            self.availability_changes,
            self.input_to_prepare_ms,
            self.last_input_to_prepare_ms,
            self.event_cpu_ms,
            self.request_to_redraw_ms,
            self.input_to_request_ms,
            self.redraw_to_prepare_ms,
            self.prepare_ms,
            self.acquire_ms,
            self.record_ms,
            self.submit_ms,
            self.input_to_submit_ms,
            self.last_input_to_submit_ms
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn phases_separate_coalesced_input_and_reset_after_submission() {
        let start = Instant::now();
        let t = |n| start + Duration::from_millis(n);
        let mut trace = FrameTrace::default();
        trace.input(t(1), t(2));
        trace.input(t(3), t(4));
        trace.requested(t(4));
        trace.received(t(8));
        let report = trace.report(t(10), t(11), t(14), t(15), t(16));
        assert_eq!(report.inputs, 2);
        assert_eq!(report.input_to_prepare_ms, Some(9.));
        assert_eq!(report.last_input_to_prepare_ms, Some(7.));
        assert_eq!(report.event_cpu_ms, 2.);
        assert_eq!(report.request_to_redraw_ms, Some(4.));
        assert_eq!(report.redraw_to_prepare_ms, Some(2.));
        assert_eq!(report.acquire_ms, 3.);
        assert_eq!(report.input_to_submit_ms, Some(15.));
        assert_eq!(report.last_input_to_submit_ms, Some(13.));
        let idle = trace.report(t(20), t(21), t(22), t(23), t(24));
        assert_eq!(idle.inputs, 0);
        assert_eq!(idle.input_to_prepare_ms, None);
        assert_eq!(idle.last_input_to_submit_ms, None);
        assert_eq!(idle.request_to_redraw_ms, None);
        assert_eq!(idle.frame, 2);
    }
}
