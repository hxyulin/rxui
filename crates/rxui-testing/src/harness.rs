//! Deterministic, headless harness for complete [`rxui_app::App`]
//! applications.
//!
//! [`AppHarness`] drives an [`App`] the way [`rxui_app::run`] does — building
//! it against an [`AppBackend`], routing UI-emitted messages through
//! [`App::update`] with the source window recorded, honoring the close-request
//! and exit-on-last-window policies — but without a platform event loop, GPU,
//! or wall clock. Windows are bare [`Ui`] trees with a viewport, timers fire
//! from a virtual clock advanced by [`AppHarness::advance`], and the clipboard
//! is an in-memory buffer, so tests are deterministic and run headlessly.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use astrelis_core::geometry::LogicalSize;
use astrelis_platform::{
    ClipboardCapabilities, Instant, PlatformError, Window, backend as platform_backend,
};
use astrelis_ui_core::{SemanticAction, SemanticNode, SemanticRole};
use astrelis_ui_testing::{SnapshotBundle, UiHarness, deterministic_font_database};
use rxui_app::{
    App, AppBackend, AppCx, Clipboard, CloseResponse, Error, MessageProxy, Monitor, ProxyClosed,
    Result, RuntimePolicy, Theme, TimerId, Ui, WindowConfig, WindowHost, WindowId,
};

use crate::deterministic_theme;

/// Maximum passes over messages posted from [`App::update`] before the
/// harness defers the remainder to the next operation, mirroring the runner.
const MAX_POSTED_PASSES: usize = 8;

/// Maximum timer deliveries in one [`AppHarness::advance`] call before the
/// harness reports a scheduling livelock.
const MAX_TIMER_FIRINGS: usize = 10_000;

/// Default logical viewport applied to windows opened without an explicit
/// size, matching [`UiHarness`]'s conventional deterministic viewport.
const DEFAULT_VIEWPORT: (f64, f64) = (800.0, 600.0);

/// In-memory clipboard backing [`AppCx::clipboard`] in the harness.
#[derive(Debug, Default)]
struct MemoryClipboard {
    text: Mutex<Option<String>>,
}

impl platform_backend::Clipboard for MemoryClipboard {
    fn capabilities(&self) -> ClipboardCapabilities {
        ClipboardCapabilities {
            read_text: true,
            write_text: true,
        }
    }

    fn read_text(&self) -> std::result::Result<Option<String>, PlatformError> {
        Ok(self.text.lock().expect("clipboard lock poisoned").clone())
    }

    fn write_text(&self, text: String) -> std::result::Result<(), PlatformError> {
        *self.text.lock().expect("clipboard lock poisoned") = Some(text);
        Ok(())
    }
}

/// Payload of one scheduled virtual timer.
enum TimerKind<M> {
    /// One message delivered once.
    Timeout(M),
    /// A factory invoked on every elapsed period.
    Interval {
        period: Duration,
        factory: Box<dyn FnMut() -> M>,
    },
}

/// One timer scheduled against the harness's virtual clock.
struct TimerEntry<M> {
    id: TimerId,
    due: Duration,
    kind: TimerKind<M>,
}

/// Headless [`AppBackend`] used by [`AppHarness`].
///
/// Windows are bare [`Ui`] trees with a viewport; there are no platform
/// windows, GPU hosts, or monitors. Timers are recorded against a virtual
/// clock and fired by [`AppHarness::advance`].
struct HeadlessBackend<M: 'static> {
    theme: Theme,
    slots: Vec<(WindowId, Ui<M>)>,
    posted: VecDeque<M>,
    proxied: Arc<Mutex<VecDeque<M>>>,
    timers: Vec<TimerEntry<M>>,
    now: Duration,
    epoch: Instant,
    clipboard: Arc<MemoryClipboard>,
    policy: Option<RuntimePolicy>,
    exit_on_last_window_close: bool,
    exited: bool,
    next_window: u64,
    next_timer: u64,
}

impl<M: 'static> HeadlessBackend<M> {
    fn new() -> Self {
        Self {
            theme: deterministic_theme(),
            slots: Vec::new(),
            posted: VecDeque::new(),
            proxied: Arc::new(Mutex::new(VecDeque::new())),
            timers: Vec::new(),
            now: Duration::ZERO,
            epoch: Instant::now(),
            clipboard: Arc::new(MemoryClipboard::default()),
            policy: None,
            exit_on_last_window_close: true,
            exited: false,
            next_window: 1,
            next_timer: 1,
        }
    }

    fn ui_slot(&mut self, window: WindowId) -> Result<&mut Ui<M>> {
        self.slots
            .iter_mut()
            .find(|(id, _)| *id == window)
            .map(|(_, ui)| ui)
            .ok_or_else(|| Error::msg(format!("window {window:?} is not open")))
    }

    fn alloc_timer(&mut self) -> TimerId {
        let timer = TimerId::from_raw(self.next_timer);
        self.next_timer += 1;
        timer
    }

    /// Returns the index of the next timer due at or before `target`,
    /// ordering equal deadlines by creation.
    fn next_due(&self, target: Duration) -> Option<usize> {
        self.timers
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.due <= target)
            .min_by_key(|(_, entry)| (entry.due, entry.id.raw()))
            .map(|(index, _)| index)
    }
}

impl<M: 'static> AppBackend<M> for HeadlessBackend<M> {
    fn new_ui(&mut self) -> Ui<M> {
        Ui::new(deterministic_font_database(), self.theme.clone())
    }

    fn open_window(&mut self, config: WindowConfig, mut ui: Ui<M>) -> Result<WindowId> {
        let options = config.into_host_options();
        let (width, height) = options
            .window
            .inner_size
            .map_or(DEFAULT_VIEWPORT, |size| (size.width, size.height));
        ui.set_viewport(LogicalSize::new(width as f32, height as f32), 1.0);
        let window = WindowId(self.next_window);
        self.next_window += 1;
        self.slots.push((window, ui));
        Ok(window)
    }

    fn close_window(&mut self, window: WindowId) -> Result<()> {
        let Some(index) = self.slots.iter().position(|(id, _)| *id == window) else {
            return Err(Error::msg(format!("window {window:?} is not open")));
        };
        drop(self.slots.remove(index));
        Ok(())
    }

    fn windows(&self) -> Vec<WindowId> {
        self.slots.iter().map(|(id, _)| *id).collect()
    }

    fn ui_mut(&mut self, window: WindowId) -> Result<&mut Ui<M>> {
        self.ui_slot(window)
    }

    fn window(&self, _window: WindowId) -> Result<&Window> {
        Err(Error::msg("the headless harness has no platform windows"))
    }

    fn host_mut(&mut self, _window: WindowId) -> Result<&mut WindowHost<M>> {
        Err(Error::msg("the headless harness has no GPU window hosts"))
    }

    fn present(&mut self, window: WindowId) -> Result<()> {
        // Generating the display list performs layout and paint, consuming
        // the pending redraw work a real present would.
        let _ = self.ui_slot(window)?.display_list()?;
        Ok(())
    }

    fn invalidate(&mut self, _window: WindowId) {
        // Headless windows have no redraw scheduler.
    }

    fn invalidate_all(&mut self) {}

    fn post(&mut self, message: M) {
        self.posted.push_back(message);
    }

    fn take_posted(&mut self) -> Vec<M> {
        self.posted.drain(..).collect()
    }

    fn proxy(&self) -> MessageProxy<M>
    where
        M: Send,
    {
        let queue = Arc::clone(&self.proxied);
        MessageProxy::from_fn(move |message| {
            queue.lock().map_err(|_| ProxyClosed)?.push_back(message);
            Ok(())
        })
    }

    fn set_timeout(&mut self, delay: Duration, message: M) -> TimerId {
        let timer = self.alloc_timer();
        self.timers.push(TimerEntry {
            id: timer,
            due: self.now + delay,
            kind: TimerKind::Timeout(message),
        });
        timer
    }

    fn set_interval(&mut self, interval: Duration, factory: Box<dyn FnMut() -> M>) -> TimerId {
        let timer = self.alloc_timer();
        self.timers.push(TimerEntry {
            id: timer,
            due: self.now + interval,
            kind: TimerKind::Interval {
                period: interval,
                factory,
            },
        });
        timer
    }

    fn cancel_timer(&mut self, timer: TimerId) -> bool {
        let before = self.timers.len();
        self.timers.retain(|entry| entry.id != timer);
        self.timers.len() != before
    }

    fn set_policy(&mut self, policy: RuntimePolicy) {
        self.policy = Some(policy);
    }

    fn clipboard(&self) -> Clipboard {
        Clipboard::from_backend(self.clipboard.clone())
    }

    fn now(&self) -> Instant {
        self.epoch + self.now
    }

    fn available_monitors(&self) -> Vec<Monitor> {
        Vec::new()
    }

    fn primary_monitor(&self) -> Option<Monitor> {
        None
    }

    fn exit(&mut self) {
        self.exited = true;
    }
}

/// Drains messages posted with [`AppCx::post`] through [`App::update`],
/// mirroring the runner's bounded re-drain: up to [`MAX_POSTED_PASSES`]
/// passes, with any surplus requeued for the next harness operation. In
/// debug builds an exhausted re-drain panics, exactly like the runner.
fn flush_posted<A: App>(
    app: &mut A,
    backend: &mut HeadlessBackend<A::Message>,
    source: Option<WindowId>,
) -> Result<()> {
    for _ in 0..MAX_POSTED_PASSES {
        let batch: Vec<_> = backend.posted.drain(..).collect();
        if batch.is_empty() {
            return Ok(());
        }
        for message in batch {
            app.update(&mut AppCx::new(backend, source), message)?;
        }
    }
    debug_assert!(
        backend.posted.is_empty(),
        "posted-message re-drain exceeded {MAX_POSTED_PASSES} passes; an `update` handler \
         keeps posting new messages on every pass"
    );
    Ok(())
}

/// Dispatches a batch of messages through [`App::update`] with a recorded
/// source window, then flushes posted messages and applies the
/// exit-on-last-window-close policy, mirroring the runner.
fn dispatch_batch<A: App>(
    app: &mut A,
    backend: &mut HeadlessBackend<A::Message>,
    source: Option<WindowId>,
    messages: Vec<A::Message>,
) -> Result<()> {
    let had_windows = !backend.slots.is_empty();
    for message in messages {
        app.update(&mut AppCx::new(backend, source), message)?;
    }
    flush_posted(app, backend, source)?;
    if backend.exit_on_last_window_close && had_windows && backend.slots.is_empty() {
        backend.exited = true;
    }
    Ok(())
}

/// Deterministic, headless harness around one [`App`] implementation.
///
/// The harness runs [`App::build`] on construction with the crate's
/// deterministic fonts and theme, then lets tests drive the application
/// through the same paths the runner uses:
///
/// - [`activate`](Self::activate) / [`perform`](Self::perform) trigger
///   semantic actions and route emitted messages through [`App::update`] with
///   the source window recorded, so [`AppCx::source_window`] and
///   [`AppCx::source_ui`] resolve as in production.
/// - [`post`](Self::post) dispatches a message with no source window, like a
///   [`MessageProxy`] or timer delivery.
/// - [`request_close`](Self::request_close) exercises
///   [`App::close_requested`], [`App::window_closed`], and the
///   exit-on-last-window-close policy.
/// - [`advance`](Self::advance) moves a virtual clock and fires timers
///   scheduled through [`AppCx::set_timeout`] and [`AppCx::set_interval`].
///
/// Messages sent through [`AppCx::proxy`] handles are queued and dispatched
/// at the end of the next harness operation ([`advance`](Self::advance) with
/// [`Duration::ZERO`] pumps them without moving the clock).
///
/// # Example
///
/// ```no_run
/// use rxui_testing::AppHarness;
/// # struct MyApp;
/// # #[derive(Clone)] enum Msg { Refresh }
/// # impl rxui_app::App for MyApp {
/// #     type Message = Msg;
/// #     fn build(&mut self, _cx: &mut rxui_app::AppCx<'_, Msg>) -> rxui_app::Result<()> { Ok(()) }
/// #     fn update(&mut self, _cx: &mut rxui_app::AppCx<'_, Msg>, _message: Msg) -> rxui_app::Result<()> { Ok(()) }
/// # }
///
/// let mut harness = AppHarness::new(MyApp).unwrap();
/// let window = harness.windows()[0];
/// harness
///     .activate(window, rxui_testing::SemanticRole::Button, "Refresh")
///     .unwrap();
/// assert!(!harness.exited());
/// ```
pub struct AppHarness<A: App> {
    app: A,
    backend: HeadlessBackend<A::Message>,
}

impl<A: App> AppHarness<A> {
    /// Builds the application against a headless backend with deterministic
    /// fonts ([`deterministic_font_database`]) and theme
    /// ([`deterministic_theme`]), running [`App::build`] and flushing any
    /// messages it posts.
    pub fn new(app: A) -> Result<Self> {
        let backend = HeadlessBackend::new();
        let mut harness = Self { app, backend };
        harness
            .app
            .build(&mut AppCx::new(&mut harness.backend, None))?;
        flush_posted(&mut harness.app, &mut harness.backend, None)?;
        harness.pump_proxied()?;
        Ok(harness)
    }

    /// Returns the application under test.
    pub fn app(&self) -> &A {
        &self.app
    }

    /// Returns the application under test for direct state manipulation.
    pub fn app_mut(&mut self) -> &mut A {
        &mut self.app
    }

    /// Returns every open window in creation order.
    pub fn windows(&self) -> Vec<WindowId> {
        self.backend.windows()
    }

    /// Returns whether the application has requested termination, either via
    /// [`AppCx::exit`] or the exit-on-last-window-close policy.
    pub fn exited(&self) -> bool {
        self.backend.exited
    }

    /// Returns a window's retained UI tree for direct inspection or setup.
    ///
    /// # Panics
    ///
    /// Panics when the window is not open.
    pub fn ui(&mut self, window: WindowId) -> &mut Ui<A::Message> {
        match self.backend.ui_slot(window) {
            Ok(ui) => ui,
            Err(error) => panic!("{error}"),
        }
    }

    /// Activates the first semantic node matching `role` and `label` in a
    /// window, then routes every emitted message through [`App::update`] with
    /// that window recorded as the source.
    pub fn activate(&mut self, window: WindowId, role: SemanticRole, label: &str) -> Result<()> {
        self.perform(window, role, label, SemanticAction::Activate)
    }

    /// Performs a semantic action on the first node matching `role` and
    /// `label` in a window, then routes every emitted message through
    /// [`App::update`] with that window recorded as the source.
    pub fn perform(
        &mut self,
        window: WindowId,
        role: SemanticRole,
        label: &str,
        action: SemanticAction,
    ) -> Result<()> {
        let ui = self.backend.ui_slot(window)?;
        let root = ui.semantic_tree()?;
        let node = find_node(&root, role, label).ok_or_else(|| {
            Error::msg(format!(
                "no {role:?} labeled {label:?} in window {window:?}"
            ))
        })?;
        let id = node.id;
        ui.perform_semantic_action(id, action)?;
        let messages: Vec<_> = ui.drain_messages().collect();
        dispatch_batch(&mut self.app, &mut self.backend, Some(window), messages)?;
        self.pump_proxied()
    }

    /// Dispatches one message through [`App::update`] with no source window,
    /// the path a [`MessageProxy`] or external event takes.
    pub fn post(&mut self, message: A::Message) -> Result<()> {
        dispatch_batch(&mut self.app, &mut self.backend, None, vec![message])?;
        self.pump_proxied()
    }

    /// Delivers a user-initiated close request to a window.
    ///
    /// [`App::close_requested`] decides the outcome: on
    /// [`CloseResponse::Close`] the window is removed, [`App::window_closed`]
    /// runs, and closing the last window marks the harness
    /// [`exited`](Self::exited); on [`CloseResponse::Ignore`] the window
    /// stays open.
    pub fn request_close(&mut self, window: WindowId) -> Result<()> {
        // Validate the window before invoking any application hook.
        self.backend.ui_slot(window)?;
        let response = self
            .app
            .close_requested(&mut AppCx::new(&mut self.backend, Some(window)), window)?;
        if response == CloseResponse::Close {
            AppBackend::close_window(&mut self.backend, window)?;
            self.app
                .window_closed(&mut AppCx::new(&mut self.backend, Some(window)), window)?;
        }
        flush_posted(&mut self.app, &mut self.backend, Some(window))?;
        if self.backend.exit_on_last_window_close && self.backend.slots.is_empty() {
            self.backend.exited = true;
        }
        self.pump_proxied()
    }

    /// Advances the virtual clock, firing every timer that becomes due.
    ///
    /// Timers fire in deadline order (creation order between equal
    /// deadlines) and their messages dispatch through [`App::update`] with no
    /// source window, like the runner's native timers. Intervals refire every
    /// elapsed period within the advance; a zero-period interval fires once
    /// per call. `advance(Duration::ZERO)` fires nothing but still pumps
    /// queued [`MessageProxy`] messages.
    pub fn advance(&mut self, duration: Duration) -> Result<()> {
        let target = self.backend.now + duration;
        let mut fired = 0usize;
        while let Some(index) = self.backend.next_due(target) {
            fired += 1;
            if fired > MAX_TIMER_FIRINGS {
                return Err(Error::msg(format!(
                    "more than {MAX_TIMER_FIRINGS} timer deliveries in one `advance`; a \
                     handler is rescheduling zero-delay timers on every delivery"
                )));
            }
            let entry = self.backend.timers.remove(index);
            self.backend.now = entry.due;
            match entry.kind {
                TimerKind::Timeout(message) => {
                    dispatch_batch(&mut self.app, &mut self.backend, None, vec![message])?;
                }
                TimerKind::Interval {
                    period,
                    mut factory,
                } => {
                    let message = factory();
                    let due = if period.is_zero() {
                        // Never due again within this advance.
                        target + Duration::from_nanos(1)
                    } else {
                        entry.due + period
                    };
                    // Reschedule before dispatching so the handler can cancel
                    // the interval through `AppCx::cancel_timer`.
                    self.backend.timers.push(TimerEntry {
                        id: entry.id,
                        due,
                        kind: TimerKind::Interval { period, factory },
                    });
                    dispatch_batch(&mut self.app, &mut self.backend, None, vec![message])?;
                }
            }
        }
        self.backend.now = target;
        self.pump_proxied()
    }

    /// Captures a window's semantic, inspection, and display-list snapshots
    /// through [`UiHarness`].
    pub fn snapshot_bundle(&mut self, window: WindowId) -> Result<SnapshotBundle> {
        let ui = self.backend.ui_slot(window)?;
        let placeholder = Ui::new(astrelis_text::FontDatabase::default(), Theme::default());
        let mut harness = UiHarness::new(placeholder);
        // Swap the real tree in and out so the window keeps its own viewport
        // and layout caches; `UiHarness` cannot return ownership.
        std::mem::swap(harness.ui_mut(), ui);
        let bundle = harness.snapshot_bundle();
        std::mem::swap(harness.ui_mut(), ui);
        Ok(bundle?)
    }

    /// Returns the text currently on the harness's in-memory clipboard,
    /// written either by the application through [`AppCx::clipboard`] or by
    /// [`set_clipboard_text`](Self::set_clipboard_text).
    pub fn clipboard_text(&self) -> Option<String> {
        self.backend
            .clipboard
            .text
            .lock()
            .expect("clipboard lock poisoned")
            .clone()
    }

    /// Seeds the in-memory clipboard, for example before testing a paste
    /// path.
    pub fn set_clipboard_text(&mut self, text: impl Into<String>) {
        *self
            .backend
            .clipboard
            .text
            .lock()
            .expect("clipboard lock poisoned") = Some(text.into());
    }

    /// Dispatches messages queued through [`AppCx::proxy`] handles, with the
    /// same bounded re-drain as posted messages.
    fn pump_proxied(&mut self) -> Result<()> {
        for _ in 0..MAX_POSTED_PASSES {
            let batch: Vec<_> = {
                let mut queue = self.backend.proxied.lock().expect("proxy queue poisoned");
                queue.drain(..).collect()
            };
            if batch.is_empty() {
                return Ok(());
            }
            for message in batch {
                dispatch_batch(&mut self.app, &mut self.backend, None, vec![message])?;
            }
        }
        Ok(())
    }
}

/// Finds the first semantic node with the requested role and label.
fn find_node<'a>(
    node: &'a SemanticNode,
    role: SemanticRole,
    label: &str,
) -> Option<&'a SemanticNode> {
    if node.role == role && node.label == label {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_node(child, role, label))
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, rc::Rc, thread};

    use astrelis_ui_core::{ElementHandle, EventFilter, Label};
    use rxui_app::MessageMapper;

    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Msg {
        Clicked,
        Tick,
        Note(&'static str),
        ScheduleTimeout,
        ScheduleInterval,
        CopyToClipboard,
        GrabProxy,
    }

    #[derive(Default)]
    struct Fixture {
        extra_window: bool,
        close_response: CloseResponse,
        labels: HashMap<WindowId, ElementHandle<Label>>,
        clicks: u32,
        ticks: u32,
        log: Vec<String>,
        last_source: Option<WindowId>,
        source_ui_ok: bool,
        proxy: Option<MessageProxy<Msg>>,
    }

    impl App for Fixture {
        type Message = Msg;

        fn build(&mut self, cx: &mut AppCx<'_, Msg>) -> Result<()> {
            let count = if self.extra_window { 2 } else { 1 };
            for index in 0..count {
                let mut ui = cx.new_ui();
                let root = ui.root();
                let label = ui.add_label(root, "clicks: 0")?;
                let button = ui.add_button(root, "Increment")?;
                ui.listen(button, None, EventFilter::Activate, |context, _| {
                    context.emit(Msg::Clicked)
                })?;
                let window = cx.open_window(
                    WindowConfig::new(format!("Fixture {index}")).size(320.0, 240.0),
                    ui,
                )?;
                self.labels.insert(window, label);
            }
            Ok(())
        }

        fn update(&mut self, cx: &mut AppCx<'_, Msg>, message: Msg) -> Result<()> {
            match message {
                Msg::Clicked => {
                    self.clicks += 1;
                    self.last_source = cx.source_window();
                    self.source_ui_ok = cx.source_ui().is_ok();
                    if let Some(window) = self.last_source {
                        let label = self.labels[&window];
                        cx.ui(window)?
                            .set_label_text(label, format!("clicks: {}", self.clicks))?;
                    }
                }
                Msg::Tick => self.ticks += 1,
                Msg::Note(note) => self.log.push(note.into()),
                Msg::ScheduleTimeout => {
                    cx.set_timeout(Duration::from_secs(1), Msg::Tick);
                }
                Msg::ScheduleInterval => {
                    cx.set_interval(Duration::from_secs(1), Msg::Tick);
                }
                Msg::CopyToClipboard => cx.clipboard().write_text("from fixture")?,
                Msg::GrabProxy => self.proxy = Some(cx.proxy()),
            }
            Ok(())
        }

        fn close_requested(
            &mut self,
            _cx: &mut AppCx<'_, Msg>,
            _window: WindowId,
        ) -> Result<CloseResponse> {
            self.log.push("close_requested".into());
            Ok(self.close_response)
        }

        fn window_closed(&mut self, _cx: &mut AppCx<'_, Msg>, _window: WindowId) -> Result<()> {
            self.log.push("window_closed".into());
            Ok(())
        }
    }

    fn harness() -> AppHarness<Fixture> {
        AppHarness::new(Fixture::default()).expect("fixture builds")
    }

    #[test]
    fn build_creates_a_window() {
        let harness = harness();
        assert_eq!(harness.windows().len(), 1);
        assert!(!harness.exited());
    }

    #[test]
    fn activation_routes_through_update_and_mutates_the_label() {
        let mut harness = harness();
        let window = harness.windows()[0];
        harness
            .activate(window, SemanticRole::Button, "Increment")
            .expect("button activates");
        assert_eq!(harness.app().clicks, 1);
        let bundle = harness.snapshot_bundle(window).expect("snapshot succeeds");
        assert!(bundle.semantics.contains("label=\"clicks: 1\""));
        assert!(!bundle.semantics.contains("label=\"clicks: 0\""));
    }

    #[test]
    fn snapshots_are_deterministic_and_keep_the_window_viewport() {
        let mut harness = harness();
        let window = harness.windows()[0];
        let first = harness.snapshot_bundle(window).expect("snapshot succeeds");
        let second = harness.snapshot_bundle(window).expect("snapshot succeeds");
        assert_eq!(first, second);
        // The window opened at 320x240, not the `UiHarness` default 800x600.
        assert!(first.inspection.contains("viewport=320.000x240.000"));
    }

    #[test]
    fn activation_records_the_source_window_during_update() {
        let mut harness = AppHarness::new(Fixture {
            extra_window: true,
            ..Fixture::default()
        })
        .expect("fixture builds");
        let windows = harness.windows();
        assert_eq!(windows.len(), 2);
        let second = windows[1];
        harness
            .activate(second, SemanticRole::Button, "Increment")
            .expect("button activates");
        assert_eq!(harness.app().last_source, Some(second));
        // With two windows open, `source_ui` only resolves because the
        // source window was recorded.
        assert!(harness.app().source_ui_ok);
        let bundle = harness.snapshot_bundle(second).expect("snapshot succeeds");
        assert!(bundle.semantics.contains("label=\"clicks: 1\""));
    }

    #[test]
    fn activating_a_missing_control_errors() {
        let mut harness = harness();
        let window = harness.windows()[0];
        assert!(
            harness
                .activate(window, SemanticRole::Button, "Nonexistent")
                .is_err()
        );
    }

    #[test]
    fn close_request_runs_hooks_and_exits_under_the_default_policy() {
        let mut harness = harness();
        let window = harness.windows()[0];
        harness.request_close(window).expect("close succeeds");
        assert_eq!(harness.app().log, ["close_requested", "window_closed"]);
        assert!(harness.windows().is_empty());
        assert!(harness.exited());
    }

    #[test]
    fn ignored_close_request_keeps_the_window() {
        let mut harness = AppHarness::new(Fixture {
            close_response: CloseResponse::Ignore,
            ..Fixture::default()
        })
        .expect("fixture builds");
        let window = harness.windows()[0];
        harness
            .request_close(window)
            .expect("ignored close succeeds");
        assert_eq!(harness.app().log, ["close_requested"]);
        assert_eq!(harness.windows(), [window]);
        assert!(!harness.exited());
    }

    #[test]
    fn timeout_fires_once_via_advance() {
        let mut harness = harness();
        harness.post(Msg::ScheduleTimeout).expect("post succeeds");
        harness
            .advance(Duration::from_millis(500))
            .expect("advance succeeds");
        assert_eq!(harness.app().ticks, 0);
        harness
            .advance(Duration::from_millis(500))
            .expect("advance succeeds");
        assert_eq!(harness.app().ticks, 1);
        harness
            .advance(Duration::from_secs(5))
            .expect("advance succeeds");
        assert_eq!(harness.app().ticks, 1);
    }

    #[test]
    fn interval_refires_every_elapsed_period() {
        let mut harness = harness();
        harness.post(Msg::ScheduleInterval).expect("post succeeds");
        harness
            .advance(Duration::from_secs(3))
            .expect("advance succeeds");
        assert_eq!(harness.app().ticks, 3);
        harness
            .advance(Duration::from_secs(1))
            .expect("advance succeeds");
        assert_eq!(harness.app().ticks, 4);
    }

    #[test]
    fn post_dispatches_without_a_source_window() {
        let mut harness = harness();
        harness.post(Msg::Note("external")).expect("post succeeds");
        assert_eq!(harness.app().log, ["external"]);
        harness.post(Msg::Clicked).expect("post succeeds");
        // No source window is recorded on the external path.
        assert_eq!(harness.app().last_source, None);
    }

    #[test]
    fn proxy_messages_are_pumped_into_update() {
        let mut harness = harness();
        harness.post(Msg::GrabProxy).expect("post succeeds");
        let proxy = harness.app().proxy.clone().expect("proxy captured");
        proxy.post(Msg::Note("via proxy")).expect("proxy is open");
        harness.advance(Duration::ZERO).expect("pump succeeds");
        assert_eq!(harness.app().log, ["via proxy"]);
    }

    #[test]
    fn clipboard_round_trips_through_app_cx() {
        let mut harness = harness();
        harness.post(Msg::CopyToClipboard).expect("post succeeds");
        assert_eq!(harness.clipboard_text().as_deref(), Some("from fixture"));
        harness.set_clipboard_text("seeded");
        assert_eq!(harness.clipboard_text().as_deref(), Some("seeded"));
    }

    #[derive(Clone, Debug)]
    enum LocalMsg {
        Activated,
        Posted,
        ScheduleTimeout,
        ScheduleCancelledTimeout,
        Timeout,
        ScheduleFixedInterval,
        FixedTick,
        ScheduleFactoryInterval,
        FactoryTick(u32),
        ScheduleNested,
        Child(ChildMsg),
        GrabProxy,
        Proxied,
    }

    #[derive(Clone, Debug)]
    enum ChildMsg {
        Ping,
    }

    #[derive(Debug)]
    enum RootMsg {
        Feature(LocalMsg),
    }

    #[derive(Default)]
    struct MappedFixture {
        log: Vec<(String, Option<WindowId>)>,
        proxy: Option<MessageProxy<LocalMsg>>,
    }

    impl App for MappedFixture {
        type Message = RootMsg;

        fn build(&mut self, cx: &mut AppCx<'_, RootMsg>) -> Result<()> {
            let mapper = MessageMapper::new(RootMsg::Feature);
            let mut ui = cx.new_ui();
            let button = ui.add_button(ui.root(), "Mapped action")?;
            ui.listen(button, None, EventFilter::Activate, move |event, _| {
                mapper.emit(event, LocalMsg::Activated);
            })?;
            cx.open_window(WindowConfig::new("Mapped fixture"), ui)?;
            Ok(())
        }

        fn update(&mut self, cx: &mut AppCx<'_, RootMsg>, message: RootMsg) -> Result<()> {
            let RootMsg::Feature(message) = message;
            let mut cx = cx.map_messages(MessageMapper::new(RootMsg::Feature));
            let source = cx.source_window();
            match message {
                LocalMsg::Activated => {
                    self.log.push(("activated".into(), source));
                    cx.post(LocalMsg::Posted);
                }
                LocalMsg::Posted => self.log.push(("posted".into(), source)),
                LocalMsg::ScheduleTimeout => {
                    cx.set_timeout(Duration::from_secs(1), LocalMsg::Timeout);
                }
                LocalMsg::ScheduleCancelledTimeout => {
                    let timer = cx.set_timeout(Duration::from_secs(1), LocalMsg::Timeout);
                    let cancelled = cx.cancel_timer(timer);
                    self.log.push((format!("cancelled:{cancelled}"), source));
                }
                LocalMsg::Timeout => self.log.push(("timeout".into(), source)),
                LocalMsg::ScheduleFixedInterval => {
                    cx.set_interval(Duration::from_secs(1), LocalMsg::FixedTick);
                }
                LocalMsg::FixedTick => self.log.push(("fixed".into(), source)),
                LocalMsg::ScheduleFactoryInterval => {
                    let mut tick = 0;
                    cx.set_interval_with(Duration::from_secs(1), move || {
                        tick += 1;
                        LocalMsg::FactoryTick(tick)
                    });
                }
                LocalMsg::FactoryTick(tick) => {
                    self.log.push((format!("factory:{tick}"), source));
                }
                LocalMsg::ScheduleNested => {
                    let mut child = cx.map_messages(MessageMapper::new(LocalMsg::Child));
                    child.post(ChildMsg::Ping);
                }
                LocalMsg::Child(ChildMsg::Ping) => self.log.push(("child".into(), source)),
                LocalMsg::GrabProxy => self.proxy = Some(cx.proxy()),
                LocalMsg::Proxied => self.log.push(("proxied".into(), source)),
            }
            Ok(())
        }
    }

    fn mapped_harness() -> AppHarness<MappedFixture> {
        AppHarness::new(MappedFixture::default()).expect("mapped fixture builds")
    }

    #[test]
    fn mapped_widget_emission_and_post_preserve_the_source_window() {
        let mut harness = mapped_harness();
        let window = harness.windows()[0];
        harness
            .activate(window, SemanticRole::Button, "Mapped action")
            .expect("mapped action activates");
        assert_eq!(
            harness.app().log,
            [
                ("activated".into(), Some(window)),
                ("posted".into(), Some(window))
            ]
        );
    }

    #[test]
    fn nested_mapped_contexts_apply_each_mapping_once() {
        let mut harness = mapped_harness();
        harness
            .post(RootMsg::Feature(LocalMsg::ScheduleNested))
            .expect("nested message posts");
        assert_eq!(harness.app().log, [("child".into(), None)]);
    }

    #[test]
    fn mapped_timeout_and_intervals_deliver_without_a_source_window() {
        let mut timeout = mapped_harness();
        timeout
            .post(RootMsg::Feature(LocalMsg::ScheduleTimeout))
            .expect("timeout schedules");
        timeout
            .advance(Duration::from_secs(1))
            .expect("timeout advances");
        assert_eq!(timeout.app().log, [("timeout".into(), None)]);

        let mut fixed = mapped_harness();
        fixed
            .post(RootMsg::Feature(LocalMsg::ScheduleFixedInterval))
            .expect("fixed interval schedules");
        fixed
            .advance(Duration::from_secs(2))
            .expect("fixed interval advances");
        assert_eq!(
            fixed.app().log,
            [("fixed".into(), None), ("fixed".into(), None)]
        );

        let mut factory = mapped_harness();
        factory
            .post(RootMsg::Feature(LocalMsg::ScheduleFactoryInterval))
            .expect("factory interval schedules");
        factory
            .advance(Duration::from_secs(2))
            .expect("factory interval advances");
        assert_eq!(
            factory.app().log,
            [("factory:1".into(), None), ("factory:2".into(), None)]
        );
    }

    #[test]
    fn mapped_timer_can_be_cancelled() {
        let mut harness = mapped_harness();
        harness
            .post(RootMsg::Feature(LocalMsg::ScheduleCancelledTimeout))
            .expect("cancelled timeout schedules");
        harness
            .advance(Duration::from_secs(2))
            .expect("clock advances beyond cancelled timeout");
        assert_eq!(harness.app().log, [("cancelled:true".into(), None)]);
    }

    #[test]
    fn mapped_proxy_posts_from_another_thread() {
        let mut harness = mapped_harness();
        harness
            .post(RootMsg::Feature(LocalMsg::GrabProxy))
            .expect("proxy is captured");
        let proxy = harness.app().proxy.clone().expect("proxy exists");
        thread::spawn(move || proxy.post(LocalMsg::Proxied).expect("proxy stays open"))
            .join()
            .expect("proxy thread joins");
        harness.advance(Duration::ZERO).expect("proxy pumps");
        assert_eq!(harness.app().log, [("proxied".into(), None)]);
    }

    #[test]
    fn synchronous_mapper_accepts_non_send_non_clone_messages() {
        struct Local(Rc<String>);
        struct Root(Rc<String>);

        let mapper = MessageMapper::new(|Local(value)| Root(value));
        let mapper_clone = mapper.clone();
        let mut ui = Ui::new(deterministic_font_database(), deterministic_theme());
        let button = ui.add_button(ui.root(), "Local action").expect("button");
        ui.listen(button, None, EventFilter::Activate, move |event, _| {
            mapper_clone.emit(event, Local(Rc::new("local".into())));
        })
        .expect("listener");
        let mut harness = UiHarness::new(ui);
        harness
            .activate(SemanticRole::Button, "Local action")
            .expect("local action activates");
        let messages = harness.drain_messages().collect::<Vec<_>>();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].0.as_str(), "local");
    }
}
