use crate::{
    Callback, CloseResponse, ErrorAction, Handler, PrepareAction, RedrawMode, RunError,
    SurfaceSettings, UnknownWindow, WindowContext, WindowError, scheduler::Schedule,
};
use rxui_draw::{FrameError, GraphicsContext};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::{WindowAttributes, WindowId},
};

/// Native scheduling/recovery policy, validated before run starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunnerOptions {
    /// Exit when the last registered window closes, after at least one existed.
    pub exit_on_last_window: bool,
    /// Delay between retryable acquisitions; must be positive and representable.
    pub retry_delay: Duration,
    /// Consecutive surface losses at the base retry delay, before exponential backoff.
    /// Must be nonzero. Retries continue with a delay capped at max(retry_delay, 1s).
    /// Retains the original field name for source compatibility; no terminal limit.
    pub maximum_consecutive_surface_recoveries: u32,
}
impl Default for RunnerOptions {
    fn default() -> Self {
        Self {
            exit_on_last_window: true,
            retry_delay: Duration::from_millis(16),
            maximum_consecutive_surface_recoveries: 3,
        }
    }
}
impl RunnerOptions {
    /// Checks that retry pacing and recovery limits cannot cause an immediate loop.
    pub fn validate(&self) -> Result<(), RunError> {
        if self.retry_delay.is_zero()
            || Instant::now().checked_add(self.retry_delay).is_none()
            || self.maximum_consecutive_surface_recoveries == 0
        {
            Err(RunError::InvalidOptions)
        } else {
            Ok(())
        }
    }
}

struct ManagedWindow {
    context: WindowContext,
    schedule: Schedule,
    closing: bool,
    was_eligible: bool,
    recovering: bool,
    trace: Option<crate::trace::FrameTrace>,
}
enum Operation {
    Created(WindowId),
    Close(WindowId),
}
struct State {
    windows: HashMap<WindowId, ManagedWindow>,
    operations: VecDeque<Operation>,
    graphics: Option<GraphicsContext>,
    graphics_invalidated: bool,
    active: bool,
    had_window: bool,
    options: RunnerOptions,
}
impl State {
    fn register(&mut self, context: WindowContext) -> Result<WindowId, WindowError> {
        let id = context.window().id();
        if self.windows.contains_key(&id) {
            return Err(WindowError::AlreadyManaged(id));
        }
        self.windows.insert(
            id,
            ManagedWindow {
                was_eligible: context.can_present(),
                recovering: false,
                trace: crate::trace::FrameTrace::from_env(),
                context,
                schedule: Schedule::default(),
                closing: false,
            },
        );
        self.had_window = true;
        self.operations.push_back(Operation::Created(id));
        Ok(id)
    }
    fn get_mut(&mut self, id: WindowId) -> Result<&mut ManagedWindow, UnknownWindow> {
        self.windows
            .get_mut(&id)
            .filter(|w| !w.closing)
            .ok_or(UnknownWindow(id))
    }
}

/// Mutable application operations valid between recording callbacks.
///
/// Window creation and close notifications are deferred until the current
/// callback returns. No operation re-enters the handler. Queued closes immediately
/// disappear from lookups and cannot acquire another frame.
pub struct AppContext<'a, Message: 'static> {
    event_loop: &'a ActiveEventLoop,
    proxy: &'a EventLoopProxy<Message>,
    state: &'a mut State,
}
impl<Message: 'static> AppContext<'_, Message> {
    /// Creates a hidden native window, initializes graphics, then queues its hook.
    /// The first window synchronously waits for one-time GPU initialization on this
    /// desktop convenience path. Later windows share that device. No per-frame GPU
    /// completion wait is introduced. Use async WindowContext::new and adoption for
    /// caller-managed initialization.
    pub fn create_window(
        &mut self,
        mut attributes: WindowAttributes,
        settings: SurfaceSettings,
    ) -> Result<WindowId, WindowError> {
        if !self.state.active || self.event_loop.exiting() {
            return Err(WindowError::Inactive);
        }
        if self.state.graphics_invalidated
            || self
                .state
                .graphics
                .as_ref()
                .is_some_and(GraphicsContext::is_device_lost)
        {
            return Err(WindowError::DeviceUnavailable);
        }
        let visible = attributes.visible;
        attributes.visible = false;
        let window = Arc::new(self.event_loop.create_window(attributes)?);
        let mut context = if let Some(graphics) = &self.state.graphics {
            WindowContext::with_graphics(graphics, window, settings)?
        } else {
            let context = pollster::block_on(WindowContext::with_display(
                window,
                settings,
                self.event_loop.owned_display_handle(),
            ))?;
            self.state.graphics = Some(context.graphics().clone());
            context
        };
        context.defer_show = true;
        context.set_visible(visible);
        self.state.register(context)
    }
    /// Registers a caller-created context, preserving its native visibility/device.
    /// The caller is responsible for native adapters that require pre-show setup.
    pub fn adopt_window(&mut self, mut window: WindowContext) -> Result<WindowId, WindowError> {
        if !self.state.active || self.event_loop.exiting() {
            return Err(WindowError::Inactive);
        }
        if self.state.windows.contains_key(&window.window().id()) {
            return Err(WindowError::AlreadyManaged(window.window().id()));
        }
        window.resume()?;
        let graphics = window.graphics().clone();
        let id = self.state.register(window)?;
        if self.state.graphics.is_none() {
            self.state.graphics = Some(graphics);
        }
        Ok(id)
    }
    /// Looks up a registered window not queued for close.
    pub fn window(&self, id: WindowId) -> Option<&WindowContext> {
        self.state
            .windows
            .get(&id)
            .filter(|w| !w.closing)
            .map(|w| &w.context)
    }
    /// Changes managed settings/visibility/lifecycle between frames.
    pub fn window_mut(&mut self, id: WindowId) -> Option<&mut WindowContext> {
        self.state
            .windows
            .get_mut(&id)
            .filter(|w| !w.closing)
            .map(|w| &mut w.context)
    }
    /// Blocks all managed windows sharing `graphics`, including queued closes.
    /// Their dirty state is retained without acquisition/recovery retries. Call on
    /// the event-loop thread after forwarding wgpu's loss callback through a proxy.
    pub fn invalidate_graphics(&mut self, graphics: &GraphicsContext) {
        if self
            .state
            .graphics
            .as_ref()
            .is_some_and(|g| g.same_device(graphics))
        {
            self.state.graphics_invalidated = true;
        }
        for window in self.state.windows.values_mut() {
            if window.context.graphics().same_device(graphics) {
                window.context.invalidate_device();
            }
        }
    }

    /// Replaces a shared device across its managed windows and the creation default.
    ///
    /// All affected surfaces are dropped first. New surfaces are staged before any
    /// window switches device. On failure every affected window remains blocked on
    /// the old device; the application can retry later or exit. Suspended windows
    /// defer surface creation until resume. Independent adopted devices are untouched.
    /// Rebuild meshes/renderers and other application resources before returning
    /// from the callback; `window_created` is not repeated. No device is requested
    /// and no resource is reconstructed by this method.
    pub fn replace_graphics(
        &mut self,
        old: &GraphicsContext,
        replacement: &GraphicsContext,
    ) -> Result<(), WindowError> {
        self.invalidate_graphics(old);
        if old.same_device(replacement) || replacement.is_device_lost() {
            return Err(WindowError::InvalidReplacementDevice);
        }
        let staged = self
            .state
            .windows
            .iter()
            .filter(|(_, w)| w.context.graphics().same_device(old))
            .map(|(id, w)| Ok((*id, w.context.prepare_graphics(replacement)?)))
            .collect::<Result<Vec<_>, WindowError>>()?;
        for (id, target) in staged {
            let window = self.state.windows.get_mut(&id).expect("staged window");
            window.context.install_graphics(replacement, target);
            window.recovering = false;
            window.schedule.recoveries = 0;
            window.schedule.availability_changed();
        }
        if self
            .state
            .graphics
            .as_ref()
            .is_some_and(|g| g.same_device(old))
        {
            self.state.graphics = Some(replacement.clone());
            self.state.graphics_invalidated = false;
        }
        Ok(())
    }

    /// Invalidates this window, also probing acquisition-level suspension.
    pub fn request_redraw(&mut self, id: WindowId) -> Result<(), UnknownWindow> {
        self.state.get_mut(id)?.schedule.invalidate();
        Ok(())
    }
    /// Schedules one redraw hint; earliest pending deadline wins.
    /// An immediate redraw does not cancel it. This is not a general timer service.
    pub fn request_redraw_at(&mut self, id: WindowId, at: Instant) -> Result<(), UnknownWindow> {
        self.state.get_mut(id)?.schedule.at(at);
        Ok(())
    }
    /// Removes this window's pending one-shot redraw deadline.
    pub fn cancel_redraw_at(&mut self, id: WindowId) -> Result<(), UnknownWindow> {
        self.state.get_mut(id)?.schedule.cancel_deadline();
        Ok(())
    }
    /// Selects per-window on-demand or continuous presentation.
    pub fn set_redraw_mode(&mut self, id: WindowId, mode: RedrawMode) -> Result<(), UnknownWindow> {
        self.state.get_mut(id)?.schedule.mode(mode);
        Ok(())
    }
    /// Queues an already-decided close; bypasses close_requested.
    /// Externally retained `Arc<Window>` handles may keep a native window alive.
    pub fn close_window(&mut self, id: WindowId) -> Result<(), UnknownWindow> {
        self.state.get_mut(id)?.closing = true;
        self.state.operations.push_back(Operation::Close(id));
        Ok(())
    }
    /// Requests orderly loop shutdown, including close and exiting hooks.
    pub fn exit(&mut self) {
        self.event_loop.exit();
    }
    /// Native loop access for platform-specific integrations.
    pub fn event_loop(&self) -> &ActiveEventLoop {
        self.event_loop
    }
    /// Original typed native proxy, compatible with external native adapters.
    pub fn proxy(&self) -> EventLoopProxy<Message> {
        self.proxy.clone()
    }
}

/// Optional native desktop event-loop driver over WindowContext.
///
/// Message is the loop's real user-event type. The handler is borrowed, and no
/// renderer, UI tree, render pass, or executor is supplied by this driver.
/// Default OnDemand windows sleep while idle. Platforms supported by run are
/// macOS, Windows, and Linux; low-level WindowContext is available separately.
pub struct Runner<Message: 'static = ()> {
    event_loop: EventLoop<Message>,
    graphics: Option<GraphicsContext>,
    options: RunnerOptions,
}
impl<Message: 'static> Runner<Message> {
    /// Builds a native loop with this exact user-event type.
    pub fn new() -> Result<Self, winit::error::EventLoopError> {
        EventLoop::<Message>::with_user_event()
            .build()
            .map(Self::from_event_loop)
    }
    /// Preserves caller-selected platform builder settings and native event type.
    pub fn from_event_loop(event_loop: EventLoop<Message>) -> Self {
        Self {
            event_loop,
            graphics: None,
            options: RunnerOptions::default(),
        }
    }
    /// Selects the device for subsequently created managed windows.
    #[must_use]
    pub fn with_graphics(mut self, graphics: GraphicsContext) -> Self {
        self.graphics = Some(graphics);
        self
    }
    /// Selects scheduling/recovery policy; invalid values are rejected by run.
    #[must_use]
    pub fn with_options(mut self, options: RunnerOptions) -> Self {
        self.options = options;
        self
    }
    /// Native typed proxy; usable before running, including by worker threads.
    pub fn proxy(&self) -> EventLoopProxy<Message> {
        self.event_loop.create_proxy()
    }
    /// Borrows the handler until native shutdown and returns the first failure.
    /// A render error abandons its frame. Shutdown errors do not replace an earlier
    /// failure, and a submitted-hook error cannot undo a successful submission.
    pub fn run<H: Handler<Message = Message>>(self, handler: &mut H) -> Result<(), RunError> {
        self.options.validate()?;
        let mut driver = Driver {
            handler,
            proxy: self.event_loop.create_proxy(),
            state: State {
                windows: HashMap::new(),
                operations: VecDeque::new(),
                graphics: self.graphics,
                graphics_invalidated: false,
                active: false,
                had_window: false,
                options: self.options,
            },
            failure: None,
            shut_down: false,
        };
        let result = self.event_loop.run_app(&mut driver);
        if let Some(error) = driver.failure {
            Err(error)
        } else {
            result.map_err(RunError::EventLoop)
        }
    }
}

// Native wgpu can defer loss notification until polling. Drain callbacks once on
// a GPU failure before classifying it; never wait or mask an application failure.
fn reported_device_loss(graphics: &GraphicsContext, error: &RunError) -> bool {
    if matches!(
        error,
        RunError::Acquisition { .. } | RunError::Submission { .. }
    ) {
        let _ = graphics.device().poll(rxui_draw::wgpu::PollType::Poll);
        graphics.is_device_lost()
    } else {
        false
    }
}

struct Driver<'a, H: Handler> {
    handler: &'a mut H,
    proxy: EventLoopProxy<H::Message>,
    state: State,
    failure: Option<RunError>,
    shut_down: bool,
}
impl<H: Handler> Driver<'_, H> {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: RunError) {
        if matches!(
            error,
            RunError::Acquisition {
                source: FrameError::Retry,
                ..
            }
        ) {
            log::debug!("{error}");
        } else {
            log::warn!("{error}");
        }
        let action = self.handler.error(
            &mut AppContext {
                event_loop,
                proxy: &self.proxy,
                state: &mut self.state,
            },
            &error,
        );
        if error.can_continue() && action == ErrorAction::Continue {
            return;
        }
        if self.failure.is_none() {
            self.failure = Some(error);
        }
        event_loop.exit();
    }
    fn call<T>(
        &mut self,
        event_loop: &ActiveEventLoop,
        callback: Callback,
        window: Option<WindowId>,
        f: impl FnOnce(&mut H, &mut AppContext<'_, H::Message>) -> Result<T, H::Error>,
    ) -> Option<T> {
        let mut cx = AppContext {
            event_loop,
            proxy: &self.proxy,
            state: &mut self.state,
        };
        match f(self.handler, &mut cx) {
            Ok(value) => Some(value),
            Err(source) => {
                self.fail(
                    event_loop,
                    RunError::Handler {
                        callback,
                        window,
                        source: source.into(),
                    },
                );
                None
            }
        }
    }
    fn drain(&mut self, event_loop: &ActiveEventLoop) {
        while self.failure.is_none() && !event_loop.exiting() {
            let Some(operation) = self.state.operations.pop_front() else {
                break;
            };
            match operation {
                Operation::Created(id) => {
                    if self.state.windows.get(&id).is_some_and(|w| !w.closing) {
                        self.call(event_loop, Callback::WindowCreated, Some(id), |h, cx| {
                            h.window_created(cx, id)
                        });
                        if self.failure.is_none()
                            && !event_loop.exiting()
                            && let Some(w) = self.state.windows.get_mut(&id).filter(|w| !w.closing)
                        {
                            w.context.finish_creation();
                        }
                    }
                }
                Operation::Close(id) => {
                    if self.state.windows.remove(&id).is_some() {
                        self.call(event_loop, Callback::WindowClosed, Some(id), |h, cx| {
                            h.window_closed(cx, id)
                        });
                    }
                }
            }
        }
        if self.state.options.exit_on_last_window
            && self.state.had_window
            && self.state.windows.is_empty()
        {
            event_loop.exit();
        }
    }
    fn ready(&self, id: WindowId, event_loop: &ActiveEventLoop) -> bool {
        self.failure.is_none()
            && !event_loop.exiting()
            && self.state.active
            && self
                .state
                .windows
                .get(&id)
                .is_some_and(|w| !w.closing && w.context.can_present())
    }
    fn retry_frame(&mut self, id: WindowId) {
        if let Some(w) = self.state.windows.get_mut(&id) {
            w.schedule
                .retry(Instant::now(), self.state.options.retry_delay);
        }
    }
    fn gpu_events(&mut self, event_loop: &ActiveEventLoop) {
        let events: Vec<_> = self
            .state
            .windows
            .iter()
            .flat_map(|(id, w)| {
                // Poll is nonblocking and drives completion leases/callbacks while idle too.
                if let Err(error) = w
                    .context
                    .graphics()
                    .device()
                    .poll(rxui_draw::wgpu::PollType::Poll)
                {
                    log::warn!("device poll failed: {error}");
                }
                w.context
                    .graphics()
                    .take_gpu_events()
                    .into_iter()
                    .map(|e| (*id, e))
            })
            .collect();
        for (window, source) in events {
            self.fail(event_loop, RunError::Graphics { window, source });
            if self.failure.is_some() || event_loop.exiting() {
                break;
            }
        }
        self.drain(event_loop);
    }
    fn redraw(&mut self, event_loop: &ActiveEventLoop, id: WindowId) {
        self.gpu_events(event_loop);
        if self.failure.is_some() || event_loop.exiting() {
            return;
        }
        if let Some(w) = self
            .state
            .windows
            .get_mut(&id)
            .filter(|w| w.recovering && !w.closing)
        {
            if !self.state.active || !w.context.can_recover() {
                return;
            }
            if let Err(source) = w.context.recreate() {
                w.schedule
                    .lost(self.state.options.maximum_consecutive_surface_recoveries);
                let delay = w.schedule.recovery_delay(
                    self.state.options.retry_delay,
                    self.state.options.maximum_consecutive_surface_recoveries,
                );
                w.schedule.retry(Instant::now(), delay);
                self.fail(event_loop, RunError::SurfaceRecovery { window: id, source });
                return;
            }
            w.recovering = false;
            // Successful recreation must not erase the consecutive-loss count.
            w.context.invalidated = false;
            w.context.availability_invalidated = false;
        }
        if !self.ready(id, event_loop) {
            if let Some(w) = self.state.windows.get_mut(&id) {
                w.schedule.unavailable();
            }
            return;
        }
        let trace_start = self.state.windows[&id]
            .trace
            .as_ref()
            .map(|_| Instant::now());
        let action = {
            profiling::scope!("rxui_host::prepare");
            self.call(event_loop, Callback::Prepare, Some(id), |h, cx| {
                h.prepare(cx, id)
            })
        };
        self.drain(event_loop);
        if action != Some(PrepareAction::Render) {
            if action.is_none() {
                self.retry_frame(id);
            }
            return;
        }
        if !self.ready(id, event_loop) {
            if let Some(w) = self.state.windows.get_mut(&id) {
                w.schedule.unavailable();
            }
            return;
        }
        let prepared_at = trace_start.map(|_| Instant::now());
        let mut recorded_at = prepared_at;
        // These disjoint field borrows avoid cloning Arc/window-format metadata.
        let w = self.state.windows.get_mut(&id).expect("ready window");
        let acquired = w.context.info_and_frame();
        let acquired_at = trace_start.map(|_| Instant::now());
        let outcome = match acquired {
            Ok((info, mut frame)) => {
                let rendered = {
                    profiling::scope!("rxui_host::render");
                    self.handler.render(info, &mut frame)
                };
                match rendered {
                    Ok(()) => {
                        recorded_at = trace_start.map(|_| Instant::now());
                        info.window().pre_present_notify();
                        frame.finish().map_err(|source| match source {
                            // The surface was never cleared: a contract violation by
                            // render, not a GPU submission failure. Name the callback.
                            rxui_draw::Error::UninitializedFrame => RunError::Handler {
                                callback: Callback::Render,
                                window: Some(id),
                                source: source.into(),
                            },
                            source => RunError::Submission { window: id, source },
                        })
                    }
                    Err(source) => Err(RunError::Handler {
                        callback: Callback::Render,
                        window: Some(id),
                        source: source.into(),
                    }),
                }
            }
            Err(source) => Err(RunError::Acquisition { window: id, source }),
        };
        // Device loss is not a surface retry: leave policy to the queued user event.
        if outcome.as_ref().err().is_some_and(|error| {
            reported_device_loss(self.state.windows[&id].context.graphics(), error)
        }) {
            let graphics = self.state.windows[&id].context.graphics().clone();
            if self
                .state
                .graphics
                .as_ref()
                .is_some_and(|g| g.same_device(&graphics))
            {
                self.state.graphics_invalidated = true;
            }
            for window in self.state.windows.values_mut() {
                if window.context.graphics().same_device(&graphics) {
                    window.context.invalidate_device();
                    window.schedule.unavailable();
                }
            }
            return;
        }
        match outcome {
            Ok(submission) => {
                if let Some(trace) = self
                    .state
                    .windows
                    .get_mut(&id)
                    .and_then(|w| w.trace.as_mut())
                {
                    trace.submitted(
                        id,
                        trace_start.unwrap(),
                        prepared_at.unwrap(),
                        acquired_at.unwrap(),
                        recorded_at.unwrap(),
                        Instant::now(),
                    );
                }
                profiling::finish_frame!();
                self.state
                    .windows
                    .get_mut(&id)
                    .expect("recorded window")
                    .schedule
                    .submitted();
                if self
                    .call(event_loop, Callback::Submitted, Some(id), |h, cx| {
                        h.submitted(cx, id, submission)
                    })
                    .is_none()
                {
                    self.retry_frame(id);
                }
                self.drain(event_loop);
            }
            Err(RunError::Acquisition { source, .. }) => {
                let w = self.state.windows.get_mut(&id).expect("acquisition window");
                if let Some(trace) = &mut w.trace {
                    trace.retry();
                }
                match source {
                    FrameError::Suspended => w.schedule.suspended(),
                    source => {
                        if matches!(source, FrameError::SurfaceLost) {
                            w.recovering = true;
                            w.schedule
                                .lost(self.state.options.maximum_consecutive_surface_recoveries);
                        }
                        let delay = w.schedule.recovery_delay(
                            self.state.options.retry_delay,
                            self.state.options.maximum_consecutive_surface_recoveries,
                        );
                        w.schedule.retry(Instant::now(), delay);
                        self.fail(event_loop, RunError::Acquisition { window: id, source });
                    }
                }
            }
            Err(error) => {
                self.retry_frame(id);
                self.fail(event_loop, error);
            }
        }
    }
}
impl<H: Handler> ApplicationHandler<H::Message> for Driver<'_, H> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.failure.is_some() || event_loop.exiting() {
            return;
        }
        self.state.active = true;
        let mut failures = Vec::new();
        for (id, w) in &mut self.state.windows {
            w.schedule.availability_changed();
            if let Err(source) = w.context.resume() {
                w.recovering = true;
                w.schedule
                    .lost(self.state.options.maximum_consecutive_surface_recoveries);
                let delay = w.schedule.recovery_delay(
                    self.state.options.retry_delay,
                    self.state.options.maximum_consecutive_surface_recoveries,
                );
                w.schedule.retry(Instant::now(), delay);
                failures.push((*id, source));
            } else {
                w.recovering = false;
            }
        }
        for (window, source) in failures {
            self.fail(event_loop, RunError::SurfaceRecovery { window, source });
            if self.failure.is_some() || event_loop.exiting() {
                return;
            }
        }
        self.call(event_loop, Callback::Resumed, None, |h, cx| h.resumed(cx));
        self.drain(event_loop);
    }
    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        if self.failure.is_some() || event_loop.exiting() {
            return;
        }
        self.state.active = false;
        for w in self.state.windows.values_mut() {
            w.context.suspend();
            w.schedule.availability_changed();
        }
        self.call(event_loop, Callback::Suspended, None, |h, cx| {
            h.suspended(cx)
        });
        self.drain(event_loop);
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.failure.is_some() || event_loop.exiting() {
            return;
        }
        let trace_input = self
            .state
            .windows
            .get(&id)
            .filter(|w| w.trace.is_some())
            .and_then(|w| {
                matches!(
                    &event,
                    WindowEvent::KeyboardInput { .. }
                        | WindowEvent::Ime(_)
                        | WindowEvent::MouseInput { .. }
                        | WindowEvent::MouseWheel { .. }
                        | WindowEvent::CursorMoved { .. }
                )
                .then(|| (Instant::now(), w.schedule.invalidations))
            });
        let redraw = matches!(event, WindowEvent::RedrawRequested);
        let close = matches!(event, WindowEvent::CloseRequested);
        let destroyed = matches!(event, WindowEvent::Destroyed);
        let removed = if destroyed {
            self.state.windows.remove(&id).is_some()
        } else {
            false
        };
        if let Some(w) = self.state.windows.get_mut(&id).filter(|w| !w.closing) {
            if redraw {
                if let Some(trace) = &mut w.trace {
                    trace.received(Instant::now());
                }
                w.schedule.begin_redraw();
            }
            if let Err(source) = w.context.process_event(&event) {
                self.fail(event_loop, RunError::Window { window: id, source });
                return;
            }
        }
        let forwarded = self.call(event_loop, Callback::WindowEvent, Some(id), |h, cx| {
            h.window_event(cx, id, event)
        });
        // A forced destruction notification is delivered even if raw-event handling fails.
        if removed {
            self.call(event_loop, Callback::WindowClosed, Some(id), |h, cx| {
                h.window_closed(cx, id)
            });
        }
        self.drain(event_loop);
        if self.failure.is_some() || event_loop.exiting() {
            return;
        }
        if let Some((start, serial)) = trace_input
            && let Some(w) = self.state.windows.get_mut(&id)
            && w.schedule.invalidations != serial
            && let Some(trace) = &mut w.trace
        {
            trace.input(start, Instant::now());
        }
        if close && self.state.windows.get(&id).is_some_and(|w| !w.closing) {
            let decision = self.call(event_loop, Callback::CloseRequested, Some(id), |h, cx| {
                h.close_requested(cx, id)
            });
            if decision == Some(CloseResponse::Close)
                && self.state.windows.get(&id).is_some_and(|w| !w.closing)
            {
                self.state
                    .windows
                    .get_mut(&id)
                    .expect("close window")
                    .closing = true;
                self.state.operations.push_back(Operation::Close(id));
            }
            self.drain(event_loop);
        }
        if redraw && forwarded.is_some() {
            self.redraw(event_loop, id);
        } else if redraw {
            self.retry_frame(id);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, message: H::Message) {
        if self.failure.is_some() || event_loop.exiting() {
            return;
        }
        self.call(event_loop, Callback::UserEvent, None, |h, cx| {
            h.user_event(cx, message)
        });
        self.drain(event_loop);
    }
    fn device_event(&mut self, event_loop: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        if self.failure.is_some() || event_loop.exiting() {
            return;
        }
        self.call(event_loop, Callback::DeviceEvent, None, |h, cx| {
            h.device_event(cx, id, event)
        });
        self.drain(event_loop);
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.failure.is_some() || event_loop.exiting() {
            return;
        }
        self.drain(event_loop);
        if event_loop.exiting() {
            return;
        }
        self.gpu_events(event_loop);
        if self.failure.is_some() || event_loop.exiting() {
            return;
        }
        let now = Instant::now();
        // Wgpu callbacks may run on worker threads, outside native event delivery.
        let mut wake = (!self.state.windows.is_empty()).then(|| now + Duration::from_millis(100));
        for w in self.state.windows.values_mut().filter(|w| !w.closing) {
            let eligible = self.state.active
                && (w.context.can_present() || (w.recovering && w.context.can_recover()));
            if std::mem::take(&mut w.context.availability_invalidated) || w.was_eligible != eligible
            {
                w.was_eligible = eligible;
                if let Some(trace) = &mut w.trace {
                    trace.availability_changed();
                }
                w.schedule.availability_changed();
            }
            if std::mem::take(&mut w.context.invalidated) {
                w.schedule.invalidate();
            }
            w.schedule.advance(now);
            if w.schedule.request_native(now, eligible) {
                if let Some(trace) = &mut w.trace {
                    trace.requested(now);
                }
                w.context.window().request_redraw();
            }
            if let Some(at) = w.schedule.wake(eligible) {
                wake = Some(wake.map_or(at, |old: Instant| old.min(at)));
            }
        }
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        if self.shut_down {
            return;
        }
        self.shut_down = true;
        self.state.active = false;
        self.state.operations.clear();
        // Remove before notification, including windows queued during a failed hook.
        while let Some(id) = self.state.windows.keys().next().copied() {
            self.state.windows.remove(&id);
            self.call(event_loop, Callback::WindowClosed, Some(id), |h, cx| {
                h.window_closed(cx, id)
            });
        }
        self.call(event_loop, Callback::Exiting, None, |h, cx| h.exiting(cx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a GPU; run with -- --ignored"]
    fn deferred_loss_is_drained_before_classifying_acquisition_but_never_masks_handler_errors() {
        let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        graphics.set_device_lost_callback(move |reason, _| {
            send.send(reason).unwrap();
        });
        graphics.device().destroy();
        let acquisition = RunError::Acquisition {
            window: WindowId::dummy(),
            source: FrameError::Validation,
        };
        assert!(reported_device_loss(&graphics, &acquisition));
        assert_eq!(
            receive.try_recv().unwrap(),
            rxui_draw::wgpu::DeviceLostReason::Destroyed
        );
        let handler = RunError::Handler {
            callback: Callback::Render,
            window: Some(WindowId::dummy()),
            source: std::io::Error::other("application failure during loss").into(),
        };
        assert!(!reported_device_loss(&graphics, &handler));
    }

    #[test]
    fn reject_unpaced_retries_and_unbounded_recovery_configuration() {
        assert!(RunnerOptions::default().validate().is_ok());
        assert!(
            RunnerOptions {
                retry_delay: Duration::ZERO,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            RunnerOptions {
                retry_delay: Duration::MAX,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            RunnerOptions {
                maximum_consecutive_surface_recoveries: 0,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
