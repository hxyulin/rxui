//! AppKit primary-drag tracking and native input-window hit testing.
use super::*;
use block2::RcBlock;
use objc2::{
    MainThreadMarker,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
};
use objc2_app_kit::{
    NSApplication, NSApplicationDidResignActiveNotification, NSEvent, NSEventMask, NSEventType,
    NSView, NSWindow, NSWindowDidResignKeyNotification, NSWindowWillCloseNotification,
};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSNotificationName, NSObjectProtocol, NSPoint,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::{
        Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

// Global button state can lead the delivered event stream. Give an already
// acquired gesture time to receive its native up; this is a cancellation bound,
// never evidence that a release occurred. Polls must not move the deadline.
const NATIVE_UP_GRACE: Duration = Duration::from_millis(500);

fn trace(args: fmt::Arguments<'_>) {
    if std::env::var_os("RXUI_NATIVE_DRAG_TRACE").is_some()
        || std::env::var_os("RXUI_DOCK_TRACE").is_some()
    {
        use std::io::Write;
        let _ = writeln!(std::io::stderr().lock(), "RXUI native drag: {args}");
    }
}

struct Observer(Retained<ProtocolObject<dyn NSObjectProtocol>>);
impl Drop for Observer {
    fn drop(&mut self) {
        // SAFETY: this is exactly the opaque observer token returned by our
        // notification-center registration; Adapter and registrations are !Send.
        unsafe {
            NSNotificationCenter::defaultCenter().removeObserver((*self.0).as_ref());
        }
    }
}
fn observer(
    name: &NSNotificationName,
    object: Option<&AnyObject>,
    flag: Arc<AtomicBool>,
) -> Observer {
    // Only an atomic flag crosses a notification's posting thread. No AppKit
    // object, Rc or UI callback is touched by this possibly-any-thread block.
    let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
        flag.store(true, Ordering::Release);
    });
    // SAFETY: named notification/object are the documented AppKit sender types;
    // no operation queue is used, and the block's sole capture is Send+Sync.
    Observer(unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(name),
            object,
            None,
            &block,
        )
    })
}
struct Registration {
    owner: Weak<Window>,
    native: Retained<NSWindow>,
    view: Retained<NSView>,
    lost: Arc<AtomicBool>,
    _observers: Vec<Observer>,
}
#[derive(Default)]
struct State {
    windows: HashMap<WindowId, Registration>,
    phase: Option<CaptureState>,
    last_down: Option<WindowId>,
    release_target: Option<Target>,
    pending_up_deadline: Option<Instant>,
    signals: Vec<Signal>,
}
impl State {
    fn phase(&self) -> CaptureState {
        self.phase.unwrap_or(CaptureState::Idle)
    }
    fn lost(&mut self, reason: &'static str) {
        // Uncertainty also retires a native down not yet adopted by a lease.
        self.last_down = None;
        self.pending_up_deadline = None;
        if let CaptureState::Held(source) | CaptureState::Released(source) = self.phase() {
            trace(format_args!("lost source={source:?} reason={reason}"));
            self.phase = Some(CaptureState::Lost(source));
            self.signals.retain(
                |signal| !matches!(signal, Signal::Released { source: id, .. } if *id == source),
            );
            self.release_target = None;
            self.signals.push(Signal::Lost { source });
        }
    }
    fn consume_press(&mut self, source: WindowId) -> bool {
        if self.last_down != Some(source) {
            return false;
        }
        self.last_down = None;
        true
    }
    fn validate_capture(&mut self, live: bool, primary_held: bool, now: Instant) {
        if !matches!(
            self.phase(),
            CaptureState::Held(_) | CaptureState::Released(_)
        ) {
            return;
        }
        if !live {
            self.lost("native-liveness");
            return;
        }
        if !matches!(self.phase(), CaptureState::Held(_)) {
            return;
        }
        if let Some(deadline) = self.pending_up_deadline {
            // A later global Held sample is not a fresh native down. It must
            // neither rearm this press nor extend/cancel its pending-up deadline.
            if now >= deadline {
                self.lost("pending-native-up-timeout");
            }
        } else if !primary_held {
            self.pending_up_deadline = Some(now + NATIVE_UP_GRACE);
            trace(format_args!(
                "pending native up source={:?} grace_ms={}",
                self.phase(),
                NATIVE_UP_GRACE.as_millis()
            ));
        }
    }
    fn observed_up(&mut self, source: WindowId, target: Target, position: [f64; 2], now: Instant) {
        let CaptureState::Held(expected) = self.phase() else {
            return;
        };
        if expected != source {
            self.lost("native-up-source-mismatch");
            return;
        }
        if self
            .pending_up_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.lost("pending-native-up-timeout");
            return;
        }
        // Only the actual native-up path can produce Released. A mask sample,
        // timeout or later button state can never complete this gesture.
        self.pending_up_deadline = None;
        self.last_down = None;
        self.release_target = Some(target);
        self.phase = Some(CaptureState::Released(source));
        self.signals.push(Signal::Released { source, position });
        trace(format_args!(
            "released source={source:?} reason=observed-native-up target={target:?}"
        ));
    }
    fn native_down(&mut self, source: Option<WindowId>) {
        self.lost("new-native-down");
        self.last_down = source;
    }
    fn end_capture(&mut self, id: WindowId) {
        if matches!(self.phase(), CaptureState::Held(s) | CaptureState::Released(s) | CaptureState::Lost(s) if s == id)
        {
            self.phase = None;
            self.release_target = None;
            self.pending_up_deadline = None;
            self.signals.retain(|event| match event {
                Signal::Motion { source, .. }
                | Signal::Released { source, .. }
                | Signal::Lost { source } => *source != id,
            });
        }
    }
    fn source_live(&self, id: WindowId) -> bool {
        self.windows
            .get(&id)
            .is_some_and(|r| r.owner.upgrade().is_some() && r.native.isVisible())
    }
    fn refresh(&mut self, app: &NSApplication, app_lost: bool) {
        if let CaptureState::Held(source) | CaptureState::Released(source) = self.phase() {
            let native_lost = self
                .windows
                .get(&source)
                .is_none_or(|r| r.lost.load(Ordering::Acquire));
            let active = app.isActive();
            let source_live = self.source_live(source);
            let live = !app_lost && active && source_live && !native_lost;
            if !live {
                trace(format_args!(
                    "liveness source={source:?} app_lost={app_lost} active={active} source_live={source_live} source_notified={native_lost}"
                ));
            }
            self.validate_capture(
                live,
                NSEvent::pressedMouseButtons() & 1 != 0,
                Instant::now(),
            );
        }
    }
    fn point_in_view(r: &Registration, screen: NSPoint) -> [f64; 2] {
        let point = r
            .view
            .convertPoint_fromView(r.native.convertPointFromScreen(screen), None);
        let bounds = r.view.bounds();
        [
            point.x - bounds.origin.x,
            if r.view.isFlipped() {
                point.y - bounds.origin.y
            } else {
                bounds.origin.y + bounds.size.height - point.y
            },
        ]
    }
    fn target(&self, screen: NSPoint, mtm: MainThreadMarker) -> Target {
        if !screen.x.is_finite() || !screen.y.is_finite() {
            return Target::Unavailable;
        }
        let number = NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(screen, 0, mtm);
        if number == 0 {
            return Target::NoWindow;
        }
        for (&id, r) in &self.windows {
            if r.native.windowNumber() != number {
                continue;
            }
            if r.owner.upgrade().is_none() || !r.native.isVisible() {
                return Target::Unavailable;
            }
            let point = Self::point_in_view(r, screen);
            let bounds = r.view.bounds();
            if !point.iter().all(|v| v.is_finite()) {
                return Target::Unavailable;
            }
            if point[0] < 0.
                || point[1] < 0.
                || point[0] >= bounds.size.width
                || point[1] >= bounds.size.height
            {
                return Target::Foreign;
            }
            return Target::Window {
                window: id,
                position: point,
            };
        }
        Target::Foreign
    }
    fn physical(&self, id: WindowId, screen: NSPoint) -> Option<[f64; 2]> {
        let r = self.windows.get(&id)?;
        let owner = r.owner.upgrade()?;
        let point = Self::point_in_view(r, screen);
        let scale = owner.scale_factor();
        let p = [point[0] * scale, point[1] * scale];
        p.iter().all(|v| v.is_finite()).then_some(p)
    }
    fn event(&mut self, event: &NSEvent, mtm: MainThreadMarker, app_lost: bool) {
        let app = NSApplication::sharedApplication(mtm);
        let kind = event.r#type();
        if kind == NSEventType::LeftMouseDown {
            // A new native down cannot belong to the still-held old gesture.
            let source = event.window(mtm).and_then(|window| {
                self.windows
                    .iter()
                    .find(|(_, r)| r.native.windowNumber() == window.windowNumber())
                    .map(|(&id, _)| id)
            });
            self.native_down(source);
            if let Some(source) = self.last_down
                && let Some(registration) = self.windows.get(&source)
            {
                registration.lost.store(false, Ordering::Release);
            }
            return;
        }
        let CaptureState::Held(source) = self.phase() else {
            if kind == NSEventType::LeftMouseUp {
                self.last_down = None;
            }
            return;
        };
        // Process the observed native up without consulting current button state:
        // global state is independent of this queued event. Explicit loss still
        // cancels immediately, and an established pending-up deadline is bounded.
        if kind == NSEventType::LeftMouseUp {
            // Bound waiting for the up's observation, not later hit-test work.
            let observed_at = Instant::now();
            let source_matches = event.window(mtm).is_some_and(|w| {
                self.windows
                    .get(&source)
                    .is_some_and(|r| r.native.windowNumber() == w.windowNumber())
            });
            if !source_matches
                || !app.isActive()
                || !self.source_live(source)
                || app_lost
                || self
                    .windows
                    .get(&source)
                    .is_none_or(|r| r.lost.load(Ordering::Acquire))
            {
                self.lost("invalid-native-up-liveness-or-source");
                self.last_down = None;
                return;
            }
            let screen = event
                .window(mtm)
                .expect("validated release window")
                .convertPointToScreen(event.locationInWindow());
            let target = self.target(screen, mtm);
            if let Some(position) = self.physical(source, screen) {
                self.observed_up(source, target, position, observed_at);
            } else {
                self.lost("invalid-source-coordinates");
            }
            self.last_down = None;
            return;
        }
        self.refresh(&app, app_lost);
        if self.phase() != CaptureState::Held(source) {
            return;
        }
        if kind == NSEventType::LeftMouseDragged || kind == NSEventType::MouseMoved {
            if let Some(position) = self.physical(source, NSEvent::mouseLocation()) {
                if let Some(Signal::Motion {
                    source: old,
                    position: p,
                }) = self.signals.last_mut()
                    && *old == source
                {
                    *p = position;
                } else {
                    self.signals.push(Signal::Motion { source, position });
                }
            } else {
                self.lost("invalid-source-coordinates");
            }
        }
        if kind == NSEventType::KeyDown && event.keyCode() == 53 {
            self.lost("escape");
        }
    }
}

pub(crate) struct Adapter {
    state: Rc<RefCell<State>>,
    monitor: Retained<AnyObject>,
    app_lost: Arc<AtomicBool>,
    _app_observer: Observer,
    callback_failed: Arc<AtomicBool>,
    mtm: MainThreadMarker,
}
impl Adapter {
    pub(crate) fn new() -> Result<Self, Error> {
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            Error("AppKit drag adapter must be created on the main thread".into())
        })?;
        let app = NSApplication::sharedApplication(mtm);
        let state = Rc::new(RefCell::new(State::default()));
        let app_lost = Arc::new(AtomicBool::new(false));
        let callback_failed = Arc::new(AtomicBool::new(false));
        // SAFETY: this documented static notification name is initialized by AppKit.
        let app_observer = observer(
            unsafe { NSApplicationDidResignActiveNotification },
            Some(&app),
            app_lost.clone(),
        );
        let events = state.clone();
        let lost = app_lost.clone();
        let failed = callback_failed.clone();
        let block = RcBlock::new(move |pointer: NonNull<NSEvent>| -> *mut NSEvent {
            // Never unwind through the Objective-C block boundary or call UI code.
            let result = catch_unwind(AssertUnwindSafe(|| {
                if let Ok(mut state) = events.try_borrow_mut() {
                    // SAFETY: AppKit supplies a live NSEvent for this monitor call.
                    // Local monitors run on the main thread, matching mtm/state.
                    let event = unsafe { pointer.as_ref() };
                    if event.r#type() == NSEventType::LeftMouseDown {
                        lost.store(false, Ordering::Release);
                    }
                    state.event(event, mtm, lost.load(Ordering::Acquire));
                } else {
                    failed.store(true, Ordering::Release);
                }
            }));
            if result.is_err() {
                failed.store(true, Ordering::Release);
            }
            // Preserve winit's normal input delivery; never consume this event.
            pointer.as_ptr()
        });
        // SAFETY: the block returns exactly AppKit's still-live event pointer;
        // state captures live until removeMonitor in Drop on the main thread.
        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::Any, &block)
        }
        .ok_or_else(|| Error("AppKit refused the local event monitor".into()))?;
        Ok(Self {
            state,
            monitor,
            app_lost,
            _app_observer: app_observer,
            callback_failed,
            mtm,
        })
    }
    fn refresh(&mut self) {
        let app = NSApplication::sharedApplication(self.mtm);
        let mut state = self.state.borrow_mut();
        if self.callback_failed.swap(false, Ordering::AcqRel) {
            state.lost("monitor-failure");
        }
        state.refresh(&app, self.app_lost.load(Ordering::Acquire));
    }
    pub(crate) fn register(&mut self, window: &Arc<Window>) -> Result<(), Error> {
        let raw = match window
            .window_handle()
            .map_err(|e| Error(e.to_string()))?
            .as_raw()
        {
            RawWindowHandle::AppKit(h) => h.ns_view.as_ptr(),
            _ => return Err(Error("window is not an AppKit client".into())),
        };
        let app = NSApplication::sharedApplication(self.mtm);
        let native = app.windows().iter().find(|w| {
            w.contentView()
                .is_some_and(|v| Retained::as_ptr(&v).cast_mut().cast::<std::ffi::c_void>() == raw)
        });
        let native =
            native.ok_or_else(|| Error("winit content view has no live AppKit window".into()))?;
        let view = native
            .contentView()
            .ok_or_else(|| Error("native window has no content view".into()))?;
        let lost = Arc::new(AtomicBool::new(false));
        // SAFETY: these are documented initialized AppKit notification names;
        // object is exactly the registered NSWindow sender, retained below.
        let observers = unsafe {
            vec![
                observer(
                    NSWindowDidResignKeyNotification,
                    Some(&native),
                    lost.clone(),
                ),
                observer(NSWindowWillCloseNotification, Some(&native), lost.clone()),
            ]
        };
        let mut state = self.state.borrow_mut();
        if state.windows.contains_key(&window.id()) {
            return Err(Error("duplicate native registration".into()));
        }
        state.windows.insert(
            window.id(),
            Registration {
                owner: Arc::downgrade(window),
                native,
                view,
                lost,
                _observers: observers,
            },
        );
        Ok(())
    }
    pub(crate) fn unregister(&mut self, id: WindowId) -> Result<(), Error> {
        self.end_capture(id);
        self.state
            .borrow_mut()
            .windows
            .remove(&id)
            .ok_or_else(|| Error("unregistered window".into()))?;
        Ok(())
    }
    pub(crate) fn begin_capture(&mut self, id: WindowId) -> Result<(), Error> {
        self.refresh();
        let app = NSApplication::sharedApplication(self.mtm);
        let mut state = self.state.borrow_mut();
        if matches!(state.phase(), CaptureState::Held(_)) {
            return Err(Error("a native gesture is already active".into()));
        }
        if state.last_down != Some(id)
            || NSEvent::pressedMouseButtons() & 1 == 0
            || !app.isActive()
            || !state.source_live(id)
            || self.app_lost.load(Ordering::Acquire)
            || state
                .windows
                .get(&id)
                .is_none_or(|r| r.lost.load(Ordering::Acquire))
        {
            return Err(Error(
                "begin_capture requires a live primary AppKit press in an active registered source"
                    .into(),
            ));
        }
        state.signals.clear();
        assert!(state.consume_press(id)); // Acquisition consumes the native down.
        state.release_target = None;
        state.pending_up_deadline = None;
        state.phase = Some(CaptureState::Held(id));
        Ok(())
    }
    pub(crate) fn end_capture(&mut self, id: WindowId) {
        // AppKit owns the mouse-down tracking sequence. Only end our lease;
        // preserve a newer native down's eligibility and never synthesize up.
        self.state.borrow_mut().end_capture(id);
    }

    pub(crate) fn capture_state(&mut self) -> CaptureState {
        self.refresh();
        self.state.borrow().phase()
    }
    pub(crate) fn take_signals(&mut self) -> Vec<Signal> {
        self.refresh();
        std::mem::take(&mut self.state.borrow_mut().signals)
    }
    pub(crate) fn target_at_pointer(&mut self) -> Target {
        self.refresh();
        let state = self.state.borrow();
        match state.phase() {
            CaptureState::Lost(_) => Target::Unavailable,
            CaptureState::Released(_) => match state.release_target {
                Some(Target::Window { window, .. }) if !state.source_live(window) => {
                    Target::Unavailable
                }
                Some(target) => target,
                None => Target::Unavailable,
            },
            _ => state.target(NSEvent::mouseLocation(), self.mtm),
        }
    }
}
impl Drop for Adapter {
    fn drop(&mut self) {
        // SAFETY: exact monitor token from our installation, removed once on the
        // creating main thread before any captured Rc/AppKit state is released.
        unsafe {
            NSEvent::removeMonitor(&self.monitor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> WindowId {
        WindowId::from(7_u64)
    }
    #[test]
    fn native_press_can_be_consumed_only_once() {
        let id = source();
        let mut state = State {
            last_down: Some(id),
            ..State::default()
        };
        assert!(state.consume_press(id));
        assert!(!state.consume_press(id));
    }
    #[test]
    fn cancellation_invalidates_held_press_eligibility() {
        let id = source();
        let mut state = State {
            phase: Some(CaptureState::Held(id)),
            last_down: Some(id),
            ..State::default()
        };
        state.lost("test-explicit-loss");
        state.phase = None; // End the lost lease.
        assert!(!state.consume_press(id)); // Holding the button cannot rearm it.
        state.last_down = Some(id); // Only another native down permits a new lease.
        assert!(state.consume_press(id));
    }
    #[test]
    fn source_loss_replaces_pending_release_with_cancellation() {
        let id = source();
        let mut state = State {
            phase: Some(CaptureState::Released(id)),
            release_target: Some(Target::Foreign),
            signals: vec![Signal::Released {
                source: id,
                position: [1., 2.],
            }],
            ..State::default()
        };
        state.validate_capture(false, false, Instant::now());
        assert_eq!(state.phase(), CaptureState::Lost(id));
        assert_eq!(state.release_target, None);
        assert_eq!(state.signals, vec![Signal::Lost { source: id }]);
        state.validate_capture(false, false, Instant::now());
        assert_eq!(state.signals.len(), 1);
    }
    #[test]
    fn native_up_survives_expected_button_up_mask() {
        let id = source();
        let mut state = State {
            phase: Some(CaptureState::Released(id)),
            ..State::default()
        };
        state.validate_capture(true, false, Instant::now());
        assert_eq!(state.phase(), CaptureState::Released(id));
        state.phase = Some(CaptureState::Held(id));
        state.validate_capture(true, false, Instant::now());
        assert_eq!(state.phase(), CaptureState::Held(id));
        assert!(state.pending_up_deadline.is_some());
    }
    fn held() -> State {
        State {
            phase: Some(CaptureState::Held(source())),
            ..State::default()
        }
    }
    #[test]
    fn queued_up_survives_polls_and_older_drag_after_global_release() {
        let now = Instant::now();
        let mut state = held();
        // Wake/global mask sees physical up ahead of AppKit's queued drag/up.
        state.validate_capture(true, false, now);
        let deadline = state.pending_up_deadline.unwrap();
        // The older queued drag takes the same refresh path as every poll.
        for ms in [8, 16, 60, 120] {
            state.validate_capture(true, false, now + Duration::from_millis(ms));
            assert_eq!(state.phase(), CaptureState::Held(source()));
            assert_eq!(state.pending_up_deadline, Some(deadline));
            assert!(state.signals.is_empty()); // The mask never authorizes Up.
        }
        state.observed_up(
            source(),
            Target::Foreign,
            [-20., 35.],
            now + Duration::from_millis(128),
        );
        assert_eq!(state.phase(), CaptureState::Released(source()));
        assert_eq!(state.release_target, Some(Target::Foreign));
        assert_eq!(state.pending_up_deadline, None);
        assert_eq!(
            state.signals,
            [Signal::Released {
                source: source(),
                position: [-20., 35.]
            }]
        );
        // Subsequent mask-up polls and duplicate native Ups cannot change it.
        state.validate_capture(true, false, now + NATIVE_UP_GRACE * 2);
        state.observed_up(
            source(),
            Target::NoWindow,
            [0., 0.],
            now + NATIVE_UP_GRACE * 2,
        );
        assert_eq!(state.release_target, Some(Target::Foreign));
        assert_eq!(state.signals.len(), 1);
    }
    #[test]
    fn pending_up_deadline_is_immutable_even_if_global_mask_returns_held() {
        let now = Instant::now();
        let mut state = held();
        state.validate_capture(true, false, now);
        let deadline = now + NATIVE_UP_GRACE;
        for (ms, mask) in [(8, false), (100, true), (240, false), (499, true)] {
            state.validate_capture(true, mask, now + Duration::from_millis(ms));
            assert_eq!(state.pending_up_deadline, Some(deadline));
            assert_eq!(state.phase(), CaptureState::Held(source()));
        }
        state.validate_capture(true, true, deadline);
        assert_eq!(state.phase(), CaptureState::Lost(source()));
        assert_eq!(state.signals, [Signal::Lost { source: source() }]);
    }
    #[test]
    fn missing_native_up_expires_once_and_cannot_revive() {
        let now = Instant::now();
        let mut state = held();
        state.validate_capture(true, false, now);
        state.validate_capture(true, false, now + NATIVE_UP_GRACE);
        state.validate_capture(true, true, now + NATIVE_UP_GRACE * 2);
        state.validate_capture(false, false, now + NATIVE_UP_GRACE * 3);
        state.observed_up(
            source(),
            Target::Foreign,
            [1., 2.],
            now + NATIVE_UP_GRACE * 3,
        );
        assert_eq!(state.phase(), CaptureState::Lost(source()));
        assert_eq!(state.signals, [Signal::Lost { source: source() }]);
        assert_eq!(state.release_target, None);
        assert_eq!(state.pending_up_deadline, None);
        assert!(!state.consume_press(source()));
    }
    #[test]
    fn observed_up_after_deadline_without_intervening_poll_is_rejected() {
        let now = Instant::now();
        let mut state = held();
        state.validate_capture(true, false, now);
        state.observed_up(source(), Target::NoWindow, [1., 2.], now + NATIVE_UP_GRACE);
        assert_eq!(state.phase(), CaptureState::Lost(source()));
        assert_eq!(state.signals, [Signal::Lost { source: source() }]);
    }
    #[test]
    fn explicit_liveness_loss_during_grace_is_immediate() {
        let now = Instant::now();
        let mut state = held();
        state.validate_capture(true, false, now);
        state.validate_capture(false, true, now + Duration::from_millis(1));
        assert_eq!(state.phase(), CaptureState::Lost(source()));
        state.observed_up(
            source(),
            Target::Foreign,
            [1., 2.],
            now + Duration::from_millis(2),
        );
        assert_eq!(state.signals, [Signal::Lost { source: source() }]);
        assert_eq!(state.release_target, None);
    }
    #[test]
    fn mismatched_native_up_during_grace_cannot_release_source() {
        let now = Instant::now();
        let mut state = held();
        state.validate_capture(true, false, now);
        state.observed_up(WindowId::from(8_u64), Target::Foreign, [1., 2.], now);
        assert_eq!(state.phase(), CaptureState::Lost(source()));
        assert_eq!(state.signals, [Signal::Lost { source: source() }]);
    }
    #[test]
    fn fresh_native_down_before_old_cleanup_retires_deadline_preserves_eligibility() {
        let now = Instant::now();
        for next in [source(), WindowId::from(8_u64)] {
            let mut state = held();
            state.validate_capture(true, false, now);
            state.native_down(Some(next));
            assert_eq!(state.phase(), CaptureState::Lost(source()));
            assert_eq!(state.pending_up_deadline, None);
            state.end_capture(source());
            assert_eq!(state.phase(), CaptureState::Idle);
            assert!(state.consume_press(next));
            assert!(!state.consume_press(next));
            assert!(state.signals.is_empty());
        }
    }
    #[test]
    fn removing_destination_cannot_clear_source_pending_deadline() {
        let now = Instant::now();
        let mut state = held();
        state.validate_capture(true, false, now);
        state.end_capture(WindowId::from(8_u64));
        assert_eq!(state.pending_up_deadline, Some(now + NATIVE_UP_GRACE));
        state.validate_capture(true, false, now + NATIVE_UP_GRACE);
        assert_eq!(state.phase(), CaptureState::Lost(source()));
    }
    #[test]
    fn held_button_without_up_sample_does_not_start_a_deadline() {
        let now = Instant::now();
        let mut state = held();
        state.validate_capture(true, true, now);
        state.validate_capture(true, true, now + Duration::from_secs(60));
        assert_eq!(state.phase(), CaptureState::Held(source()));
        assert_eq!(state.pending_up_deadline, None);
        assert!(state.signals.is_empty());
    }
    #[test]
    fn adapter_rejects_non_main_thread() {
        assert!(
            std::thread::spawn(|| Adapter::new().is_err())
                .join()
                .unwrap()
        );
    }
}
