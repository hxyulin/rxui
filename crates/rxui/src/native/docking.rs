//! Native primary-drag orchestration. Platform hooks never dispatch RXUI callbacks.
use super::*;
use astrelis_native_drag::{Adapter, CaptureState, Signal, Target};
use astrelis_winit::winit::event::{ElementState, MouseButton};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

fn trace(message: fmt::Arguments<'_>) {
    if std::env::var_os("RXUI_DOCK_TRACE").is_some() {
        use std::io::Write;
        let _ = writeln!(std::io::stderr().lock(), "RXUI dock: {message}");
    }
}

fn native_error(error: impl Error + 'static) -> ApplicationError {
    ApplicationError::Native(Box::new(error))
}

/// A sleeping worker exists only during an acquired gesture. Wakes are independent
/// of surface presentation, animation, caret timers, and accessibility publication.
struct PollWake {
    queued: Arc<AtomicBool>,
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}
impl PollWake {
    fn new(
        proxy: astrelis_winit::winit::event_loop::EventLoopProxy<Wake>,
    ) -> Result<Self, ApplicationError> {
        let (stop, stopped) = mpsc::channel();
        let queued = Arc::new(AtomicBool::new(false));
        let pending = queued.clone();
        let worker = thread::Builder::new()
            .name("rxui-dock-wake".into())
            .spawn(move || {
                while matches!(
                    stopped.recv_timeout(Duration::from_millis(8)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    if !pending.swap(true, Ordering::AcqRel)
                        && proxy.send_event(Wake::Docking).is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(native_error)?;
        Ok(Self {
            queued,
            stop,
            worker: Some(worker),
        })
    }
}
impl Drop for PollWake {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

// The injectable boundary tests orchestration without manufacturing native input.
pub(super) trait Backend {
    type Error: Error + 'static;
    fn register(&mut self, window: &Arc<Window>) -> Result<(), Self::Error>;
    fn unregister(&mut self, id: NativeWindowId) -> Result<(), Self::Error>;
    fn begin_capture(&mut self, id: NativeWindowId) -> Result<(), Self::Error>;
    fn end_capture(&mut self, id: NativeWindowId);
    fn capture_state(&mut self) -> CaptureState;
    fn take_signals(&mut self) -> Vec<Signal>;
    fn target_at_pointer(&mut self) -> Target;
}
impl Backend for Adapter {
    type Error = astrelis_native_drag::Error;
    fn register(&mut self, w: &Arc<Window>) -> Result<(), astrelis_native_drag::Error> {
        self.register(w)
    }
    fn unregister(&mut self, id: NativeWindowId) -> Result<(), astrelis_native_drag::Error> {
        self.unregister(id)
    }
    fn begin_capture(&mut self, id: NativeWindowId) -> Result<(), astrelis_native_drag::Error> {
        self.begin_capture(id)
    }
    fn end_capture(&mut self, id: NativeWindowId) {
        self.end_capture(id);
    }
    fn capture_state(&mut self) -> CaptureState {
        self.capture_state()
    }
    fn take_signals(&mut self) -> Vec<Signal> {
        self.take_signals()
    }
    fn target_at_pointer(&mut self) -> Target {
        self.target_at_pointer()
    }
}

pub(super) struct Docking<B: Backend = Adapter> {
    adapter: B,
    proxy: Option<astrelis_winit::winit::event_loop::EventLoopProxy<Wake>>,
    pub source: Option<NativeWindowId>,
    released_at: Option<Instant>,
    release_position: Option<[f64; 2]>,
    timer: Option<PollWake>,
    preview_window: Option<NativeWindowId>,
    registered: HashSet<NativeWindowId>,
}
impl Docking {
    pub fn new(
        proxy: astrelis_winit::winit::event_loop::EventLoopProxy<Wake>,
    ) -> Result<Self, ApplicationError> {
        Ok(Self {
            adapter: Adapter::new().map_err(native_error)?,
            proxy: Some(proxy),
            source: None,
            released_at: None,
            release_position: None,
            timer: None,
            preview_window: None,
            registered: HashSet::new(),
        })
    }
}
impl<B: Backend> Docking<B> {
    pub fn register(&mut self, window: &Arc<Window>) -> Result<(), ApplicationError> {
        self.adapter.register(window).map_err(native_error)?;
        self.registered.insert(window.id());
        Ok(())
    }
    pub fn unregister(&mut self, id: NativeWindowId) -> Result<(), ApplicationError> {
        if self.registered.remove(&id) {
            self.adapter.unregister(id).map_err(native_error)?;
        }
        Ok(())
    }
    fn begin(&mut self, id: NativeWindowId) -> Result<(), ApplicationError> {
        // Failure to verify a native press leaves ordinary placement-local input
        // intact. Never acquire later from a timer or guess native geometry.
        if let Err(error) = self.adapter.begin_capture(id) {
            trace(format_args!("acquire failed source={id:?}: {error}"));
            return Ok(());
        }
        trace(format_args!("acquired source={id:?}"));
        self.source = Some(id);
        let Some(proxy) = self.proxy.clone() else {
            return Ok(());
        };
        match PollWake::new(proxy) {
            Ok(timer) => self.timer = Some(timer),
            Err(error) => {
                self.finish();
                return Err(error);
            }
        }
        Ok(())
    }
    pub(super) fn acknowledged_wake(&self) {
        if let Some(timer) = &self.timer {
            timer.queued.store(false, Ordering::Release);
        }
    }
    fn finish(&mut self) {
        if let Some(source) = self.source.take() {
            self.adapter.end_capture(source);
        }
        self.timer.take();
        self.released_at = None;
        self.release_position = None;
    }
    fn poll(&mut self) -> Option<Target> {
        let source = self.source?;
        for signal in self.adapter.take_signals() {
            if !matches!(signal, Signal::Motion { .. }) {
                trace(format_args!("native {signal:?}"));
            }
            if let Signal::Released {
                source: id,
                position,
            } = signal
                && source == id
            {
                self.released_at.get_or_insert_with(Instant::now);
                self.release_position = Some(position);
            }
        }
        match self.adapter.capture_state() {
            CaptureState::Held(id) if id == source => Some(self.adapter.target_at_pointer()),
            CaptureState::Released(id) if id == source => {
                let at = self.released_at.get_or_insert_with(Instant::now);
                // Normal source release owns listener routing. A missing winit up
                // cancels instead of leaving a token armed indefinitely.
                (at.elapsed() < Duration::from_secs(1)).then(|| self.adapter.target_at_pointer())
            }
            _ => None,
        }
    }
}
impl<B: Backend> Drop for Docking<B> {
    fn drop(&mut self) {
        self.finish();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum ReleaseRoute {
    Local,
    External(Option<(NativeWindowId, [f32; 2])>),
}
fn release_route(source: NativeWindowId, target: Target) -> ReleaseRoute {
    match target {
        Target::Window { window, position }
            if window == source && position.iter().all(|n| (*n as f32).is_finite()) =>
        {
            ReleaseRoute::Local
        }
        Target::NoWindow => ReleaseRoute::Local, // explicit app-controlled detach
        Target::Window { window, position } if position.iter().all(|n| (*n as f32).is_finite()) => {
            ReleaseRoute::External(Some((window, [position[0] as f32, position[1] as f32])))
        }
        _ => ReleaseRoute::External(None), // foreign, decorations, unknown: no detach
    }
}
fn primary(event: &WindowEvent, state: ElementState) -> bool {
    matches!(event, WindowEvent::MouseInput { state: actual, button: MouseButton::Left, .. } if *actual == state)
}

impl<F> Host<F> {
    fn clear_dock_previews(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
    ) -> Result<(), ApplicationError> {
        let Some(docking) = &mut self.docking else {
            return Ok(());
        };
        let previous = docking.preview_window.take();
        for id in [previous, docking.source].into_iter().flatten() {
            if let Some(window) = self.windows.get_mut(&id)
                && let Some(ui) = &mut window.ui
            {
                ui.native_dock_preview(None, false);
                if cx.window(id).is_some() {
                    cx.request_redraw(id).map_err(native_error)?;
                }
            }
        }
        Ok(())
    }
    pub(super) fn cancel_docking(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
    ) -> Result<(), ApplicationError> {
        self.clear_dock_previews(cx)?;
        let source = self.docking.as_ref().and_then(|d| d.source);
        if let Some(docking) = &mut self.docking {
            docking.finish();
        }
        if let Some(id) = source
            && let Some(window) = self.windows.get_mut(&id)
            && let (Some(ui), Some(painter)) = (&mut window.ui, &mut self.painter)
        {
            let scale = window
                .life
                .native
                .borrow()
                .as_ref()
                .map_or(1., |w| w.scale_factor());
            window
                .input
                .cancel_primary(&mut self.runtime, ui.as_mut(), painter, scale)?;
        }
        Ok(())
    }
    pub(super) fn poll_docking(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
    ) -> Result<(), ApplicationError> {
        let Some(source) = self.docking.as_ref().and_then(|d| d.source) else {
            return Ok(());
        };
        let eligible = self.active
            && !self.commands.exited.get()
            && self.windows.get(&source).is_some_and(|w| {
                !w.life.closing.get() && w.ui.as_ref().is_some_and(|ui| ui.native_dock_source())
            });
        let target = self.docking.as_mut().unwrap().poll();
        if !eligible || target.is_none() {
            trace(format_args!(
                "cancel source={source:?} eligible={eligible} target={target:?}"
            ));
            return self.cancel_docking(cx);
        }
        let target = target.unwrap();
        let destination = match target {
            Target::Window { window, position }
                if window != source && position.iter().all(|n| (*n as f32).is_finite()) =>
            {
                Some((window, position))
            }
            _ => None,
        };
        let previous = self.docking.as_mut().unwrap().preview_window.take();
        if let Some(previous) = previous
            && Some(previous) != destination.map(|d| d.0)
            && let Some(ui) = self.windows.get_mut(&previous).and_then(|w| w.ui.as_mut())
            && ui.native_dock_preview(None, false)
            && cx.window(previous).is_some()
        {
            cx.request_redraw(previous).map_err(native_error)?;
        }
        let outside = !matches!(target, Target::Window { window, .. } if window == source);
        if self
            .windows
            .get_mut(&source)
            .unwrap()
            .ui
            .as_mut()
            .unwrap()
            .native_dock_preview(None, outside)
            && cx.window(source).is_some()
        {
            cx.request_redraw(source).map_err(native_error)?;
        }
        let mut allowed = false;
        if let Some((destination, position)) = destination {
            let session = self.windows[&source].ui.as_ref().unwrap().dock_session();
            if let Some(window) = self
                .windows
                .get_mut(&destination)
                .filter(|w| !w.life.closing.get())
                && let (Some(ui), Some(painter), Some(native)) =
                    (&mut window.ui, &mut self.painter, cx.window(destination))
            {
                let prepared = ui.prepare_input(&mut self.runtime, native.metrics(), painter)?;
                let point = [position[0] as f32, position[1] as f32];
                allowed = session
                    .as_ref()
                    .is_some_and(|session| ui.allows_external_dock(session, point));
                let changed = ui.native_dock_preview(
                    session.map(|session| (session, [position[0] as f32, position[1] as f32])),
                    false,
                );
                self.docking.as_mut().unwrap().preview_window = Some(destination);
                if prepared || changed {
                    cx.request_redraw(destination).map_err(native_error)?;
                }
            }
        }
        self.windows
            .get_mut(&source)
            .unwrap()
            .ui
            .as_mut()
            .unwrap()
            .native_dock_feedback(allowed);
        self.sync_text_platform(cx, source)?;
        Ok(())
    }
    pub(super) fn before_dock_input(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        id: NativeWindowId,
        event: &WindowEvent,
    ) -> Result<Option<ReleaseRoute>, ApplicationError> {
        let Some(source) = self.docking.as_ref().and_then(|d| d.source) else {
            return Ok(None);
        };
        if id != source {
            return Ok(None);
        }
        if primary(event, ElementState::Released) {
            let docking = self.docking.as_mut().unwrap();
            let target = docking.poll();
            if !matches!(docking.adapter.capture_state(), CaptureState::Released(s) if s == source)
                || target.is_none()
            {
                self.cancel_docking(cx)?;
                return Ok(None);
            }
            if let Some(position) = docking.release_position {
                self.windows
                    .get_mut(&id)
                    .unwrap()
                    .input
                    .native_release_position(position);
            }
            let route = release_route(source, target.unwrap());
            trace(format_args!(
                "public release source={source:?} route={route:?}"
            ));
            if matches!(route, ReleaseRoute::External(_))
                && !self
                    .windows
                    .get_mut(&id)
                    .unwrap()
                    .ui
                    .as_mut()
                    .unwrap()
                    .arm_dock_release()
            {
                self.cancel_docking(cx)?;
                return Ok(None);
            }
            return Ok(Some(route));
        }
        if matches!(
            event,
            WindowEvent::Focused(false) | WindowEvent::Destroyed | WindowEvent::CloseRequested
        ) || matches!(event, WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && event.logical_key == Key::Named(NamedKey::Escape))
        {
            self.cancel_docking(cx)?;
        }
        Ok(None)
    }
    pub(super) fn after_dock_input(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        id: NativeWindowId,
        event: &WindowEvent,
        route: Option<ReleaseRoute>,
    ) -> Result<(), ApplicationError> {
        if let Some(route) = route {
            // Capture/pressed bookkeeping and every source up listener completed
            // before the token is available or any destination callback can run.
            let token = self
                .windows
                .get_mut(&id)
                .and_then(|w| w.ui.as_mut())
                .and_then(|ui| ui.take_dock_session());
            trace(format_args!(
                "source release completed token={} route={route:?}",
                token.is_some()
            ));
            self.clear_dock_previews(cx)?;
            self.docking.as_mut().unwrap().finish();
            if let (ReleaseRoute::External(Some((destination, point))), Some(token)) =
                (route, token)
                && let Some(window) = self
                    .windows
                    .get_mut(&destination)
                    .filter(|w| !w.life.closing.get())
                && let (Some(ui), Some(painter), Some(native)) =
                    (&mut window.ui, &mut self.painter, cx.window(destination))
            {
                ui.prepare_input(&mut self.runtime, native.metrics(), painter)?;
                ui.drop_dock_session(&mut self.runtime, token, point)?;
                cx.request_redraw(destination).map_err(native_error)?;
            }
        } else if primary(event, ElementState::Pressed)
            && self.docking.as_ref().is_some_and(|d| d.source.is_none())
            && self
                .windows
                .get(&id)
                .and_then(|w| w.ui.as_ref())
                .is_some_and(|ui| ui.native_dock_source())
        {
            self.docking.as_mut().unwrap().begin(id)?;
        }
        self.poll_docking(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(n: u64) -> NativeWindowId {
        NativeWindowId::from(n)
    }
    struct Fake {
        phase: CaptureState,
        target: Target,
        signals: Vec<Signal>,
        ended: Rc<RefCell<Vec<NativeWindowId>>>,
        fail: bool,
        unregistered: Vec<NativeWindowId>,
    }
    impl Backend for Fake {
        type Error = std::io::Error;
        fn register(&mut self, _: &Arc<Window>) -> Result<(), Self::Error> {
            Ok(())
        }
        fn unregister(&mut self, id: NativeWindowId) -> Result<(), Self::Error> {
            self.unregistered.push(id);
            Ok(())
        }
        fn begin_capture(&mut self, source: NativeWindowId) -> Result<(), Self::Error> {
            if self.fail {
                return Err(std::io::Error::other("not a native press"));
            }
            self.phase = CaptureState::Held(source);
            Ok(())
        }
        fn end_capture(&mut self, source: NativeWindowId) {
            self.ended.borrow_mut().push(source);
            self.phase = CaptureState::Idle;
        }
        fn capture_state(&mut self) -> CaptureState {
            self.phase
        }
        fn take_signals(&mut self) -> Vec<Signal> {
            std::mem::take(&mut self.signals)
        }
        fn target_at_pointer(&mut self) -> Target {
            self.target
        }
    }
    fn fake() -> Docking<Fake> {
        Docking {
            adapter: Fake {
                phase: CaptureState::Idle,
                target: Target::Foreign,
                signals: vec![],
                ended: Rc::default(),
                fail: false,
                unregistered: vec![],
            },
            proxy: None,
            source: None,
            released_at: None,
            release_position: None,
            timer: None,
            preview_window: None,
            registered: HashSet::new(),
        }
    }
    #[test]
    fn close_before_creation_skips_unregister_and_registered_close_is_once() {
        let mut docking = fake();
        docking.unregister(id(1)).unwrap();
        assert!(docking.adapter.unregistered.is_empty());
        docking.registered.insert(id(2));
        docking.unregister(id(2)).unwrap();
        docking.unregister(id(2)).unwrap();
        assert_eq!(docking.adapter.unregistered, vec![id(2)]);
    }
    #[test]
    fn native_release_is_latched_until_normal_ui_release_and_cleanup_is_once() {
        let mut docking = fake();
        let ended = docking.adapter.ended.clone();
        docking.begin(id(1)).unwrap();
        docking.adapter.target = Target::Window {
            window: id(2),
            position: [10., 20.],
        };
        assert_eq!(docking.poll(), Some(docking.adapter.target));
        assert!(docking.released_at.is_none());
        // Release can invalidate a prior hover. It must never commit on a poll.
        docking.adapter.phase = CaptureState::Released(id(1));
        docking.adapter.target = Target::Foreign;
        docking.adapter.signals = vec![
            Signal::Released {
                source: id(9),
                position: [999., 999.],
            },
            Signal::Released {
                source: id(1),
                position: [400., 90.],
            },
        ];
        assert_eq!(docking.poll(), Some(Target::Foreign));
        assert_eq!(docking.release_position, Some([400., 90.]));
        assert!(docking.released_at.is_some());
        assert_eq!(docking.source, Some(id(1)));
        assert!(ended.borrow().is_empty());
        docking.finish();
        assert_eq!(*ended.borrow(), vec![id(1)]);
        assert!(docking.poll().is_none());
        assert!(docking.release_position.is_none());
        drop(docking);
        assert_eq!(*ended.borrow(), vec![id(1)]);
    }
    #[test]
    fn lost_or_unmatched_native_capture_cannot_produce_a_release_target() {
        for phase in [
            CaptureState::Idle,
            CaptureState::Lost(id(1)),
            CaptureState::Held(id(2)),
            CaptureState::Released(id(2)),
        ] {
            let mut docking = fake();
            docking.begin(id(1)).unwrap();
            docking.adapter.phase = phase;
            docking.adapter.target = Target::Window {
                window: id(2),
                position: [2., 3.],
            };
            assert!(docking.poll().is_none());
        }
    }
    #[test]
    fn absent_source_release_expires_and_failed_acquisition_owns_nothing() {
        let mut docking = fake();
        docking.adapter.fail = true;
        docking.begin(id(1)).unwrap();
        assert!(docking.source.is_none());
        assert!(docking.adapter.ended.borrow().is_empty());
        docking.adapter.fail = false;
        docking.begin(id(1)).unwrap();
        docking.adapter.phase = CaptureState::Released(id(1));
        docking.released_at = Some(Instant::now() - Duration::from_secs(2));
        assert!(docking.poll().is_none());
        docking.finish();
        docking.begin(id(2)).unwrap();
        assert_eq!(docking.source, Some(id(2)));
        assert!(docking.released_at.is_none());
    }
    #[test]
    fn release_requires_authoritative_finite_registered_destination() {
        assert_eq!(
            release_route(
                id(1),
                Target::Window {
                    window: id(2),
                    position: [-3., 5.]
                }
            ),
            ReleaseRoute::External(Some((id(2), [-3., 5.])))
        );
        for target in [
            Target::Foreign,
            Target::Unavailable,
            Target::Window {
                window: id(2),
                position: [f64::NAN, 0.],
            },
        ] {
            assert_eq!(release_route(id(1), target), ReleaseRoute::External(None));
        }
        assert_eq!(
            release_route(
                id(1),
                Target::Window {
                    window: id(1),
                    position: [1., 2.]
                }
            ),
            ReleaseRoute::Local
        );
        assert_eq!(release_route(id(1), Target::NoWindow), ReleaseRoute::Local);
    }
}
