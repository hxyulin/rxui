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
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};

use astrelis_core::geometry::LogicalSize;
use astrelis_platform::{
    ClipboardCapabilities, Instant, PlatformError, Window, backend as platform_backend,
};
use astrelis_ui_core::{SemanticAction, SemanticNode, SemanticRole};
use astrelis_ui_testing::{SnapshotBundle, UiHarness, deterministic_font_database};
#[cfg(not(target_arch = "wasm32"))]
use rxui_app::TaskSpawnError;
use rxui_app::{
    ActiveSubscriptionSnapshot, ActiveTaskSnapshot, App, AppBackend, AppCx, Clipboard,
    CloseResponse, Error, InstrumentationState, MessageDispatch, MessageKey, MessageMetadata,
    MessageOrigin, MessageOutcome, MessageProxy, Monitor, ProxyClosed, QueuedMessage,
    RawSubscriptionEvent, RawSubscriptionSink, Result, RuntimeInstrumentationConfig, RuntimePolicy,
    RuntimeSnapshot, SubscriptionConfig, SubscriptionFactory, SubscriptionId, SubscriptionStatus,
    Subscriptions, TaskCompletionStatus, TaskId, TaskKind, TaskMessageFactory, TaskSink, Theme,
    TimerId, Ui, WindowConfig, WindowHost, WindowId,
};

use crate::deterministic_theme;

/// Maximum passes over messages posted from [`App::update`] before the
/// harness defers the remainder to the next operation, mirroring the runner.
const MAX_POSTED_PASSES: usize = 8;

/// Default logical viewport applied to windows opened without an explicit
/// size, matching [`UiHarness`]'s conventional deterministic viewport.
const DEFAULT_VIEWPORT: (f64, f64) = (800.0, 600.0);

const TASK_PENDING: u8 = 0;
const TASK_COMPLETION_QUEUED: u8 = 1;
const TASK_CANCELLED: u8 = 2;
const TASK_FINISHED: u8 = 3;

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
    /// One delayed message factory.
    Timeout(Box<dyn FnOnce() -> M>),
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

enum TaskEvent<M> {
    Complete {
        id: TaskId,
        state: Arc<AtomicU8>,
        factory: TaskMessageFactory<M>,
    },
    Abandon {
        id: TaskId,
        state: Arc<AtomicU8>,
    },
}

enum ExternalEvent<M> {
    Message(M),
    Task(TaskEvent<M>),
    Subscription {
        id: SubscriptionId,
        generation: u64,
        event: RawSubscriptionEvent,
    },
}

type TaskEventQueue<M> = Arc<Mutex<VecDeque<(u64, TaskEvent<M>)>>>;

struct HeadlessTask {
    state: Arc<AtomicU8>,
    name: String,
    kind: TaskKind,
    started_at: Duration,
    #[cfg(not(target_arch = "wasm32"))]
    blocking: Option<Box<dyn FnOnce() + Send + 'static>>,
}

enum HeadlessSubscriptionSource<M> {
    Interval(TimerId),
    Service {
        _guard: Option<Box<dyn std::any::Any + Send>>,
        decode: Box<dyn FnMut(RawSubscriptionEvent) -> M>,
        status: SubscriptionStatus,
    },
}

struct HeadlessSubscription<M> {
    config: SubscriptionConfig,
    generation: u64,
    source: HeadlessSubscriptionSource<M>,
    started_at: Duration,
    starts: u64,
}

type SubscriptionEventQueue =
    Arc<Mutex<VecDeque<(u64, SubscriptionId, u64, RawSubscriptionEvent)>>>;

struct PostedQueue<M> {
    entries: VecDeque<QueuedMessage<M>>,
    keyed: HashMap<MessageKey, usize>,
    replacements: u64,
}

impl<M> Default for PostedQueue<M> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            keyed: HashMap::new(),
            replacements: 0,
        }
    }
}

impl<M> PostedQueue<M> {
    fn post(&mut self, message: QueuedMessage<M>) {
        self.entries.push_back(message);
    }

    fn replace_latest(
        &mut self,
        key: MessageKey,
        message: M,
        source: Option<WindowId>,
        metadata: MessageMetadata,
        origin: MessageOrigin,
        instrumentation: &mut InstrumentationState,
    ) -> std::result::Result<(), M> {
        if let Some(index) = self.keyed.get(&key).copied() {
            let entry = self
                .entries
                .get_mut(index)
                .expect("pending keyed-message index stays valid until the batch is drained");
            entry.replace(message, source, metadata, origin);
            instrumentation.coalesce_message(entry);
            self.replacements = self.replacements.saturating_add(1);
            return Ok(());
        }
        Err(message)
    }

    fn post_keyed(&mut self, key: MessageKey, message: QueuedMessage<M>) {
        let index = self.entries.len();
        self.entries.push_back(message);
        self.keyed.insert(key, index);
    }

    fn take(&mut self) -> Vec<QueuedMessage<M>> {
        self.keyed.clear();
        self.entries.drain(..).collect()
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Headless [`AppBackend`] used by [`AppHarness`].
///
/// Windows are bare [`Ui`] trees with a viewport; there are no platform
/// windows, GPU hosts, or monitors. Timers are recorded against a virtual
/// clock and fired by [`AppHarness::advance`].
struct HeadlessBackend<M: 'static> {
    theme: Theme,
    slots: Vec<(WindowId, Ui<M>)>,
    posted: PostedQueue<M>,
    instrumentation: InstrumentationState,
    message_metadata: fn(&M) -> MessageMetadata,
    proxied: Arc<Mutex<VecDeque<(u64, M)>>>,
    task_events: TaskEventQueue<M>,
    external_sequence: Arc<AtomicU64>,
    tasks: HashMap<TaskId, HeadlessTask>,
    subscriptions: HashMap<SubscriptionId, HeadlessSubscription<M>>,
    subscription_events: SubscriptionEventQueue,
    timers: Vec<TimerEntry<M>>,
    now: Duration,
    epoch: Instant,
    clipboard: Arc<MemoryClipboard>,
    policy: Option<RuntimePolicy>,
    exit_on_last_window_close: bool,
    exited: bool,
    next_window: u64,
    next_timer: u64,
    next_task: u64,
    next_subscription_generation: u64,
}

impl<M: 'static> HeadlessBackend<M> {
    fn new(
        instrumentation: RuntimeInstrumentationConfig,
        message_metadata: fn(&M) -> MessageMetadata,
    ) -> Self {
        Self {
            theme: deterministic_theme(),
            slots: Vec::new(),
            posted: PostedQueue::default(),
            instrumentation: InstrumentationState::new(instrumentation),
            message_metadata,
            proxied: Arc::new(Mutex::new(VecDeque::new())),
            task_events: Arc::new(Mutex::new(VecDeque::new())),
            external_sequence: Arc::new(AtomicU64::new(1)),
            tasks: HashMap::new(),
            subscriptions: HashMap::new(),
            subscription_events: Arc::new(Mutex::new(VecDeque::new())),
            timers: Vec::new(),
            now: Duration::ZERO,
            epoch: Instant::now(),
            clipboard: Arc::new(MemoryClipboard::default()),
            policy: None,
            exit_on_last_window_close: true,
            exited: false,
            next_window: 1,
            next_timer: 1,
            next_task: 1,
            next_subscription_generation: 1,
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

    fn register_task_state(&mut self, name: String) -> (TaskId, Arc<AtomicU8>) {
        let task = TaskId::from_raw(self.next_task);
        self.next_task += 1;
        let state = Arc::new(AtomicU8::new(TASK_PENDING));
        self.tasks.insert(
            task,
            HeadlessTask {
                state: Arc::clone(&state),
                name,
                kind: TaskKind::External,
                started_at: self.now,
                #[cfg(not(target_arch = "wasm32"))]
                blocking: None,
            },
        );
        (task, state)
    }

    fn cancel_all_tasks(&mut self) {
        for task in self.tasks.values() {
            task.state.store(TASK_CANCELLED, Ordering::Release);
        }
        self.tasks.clear();
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

    fn post(&mut self, message: M, source: Option<WindowId>) {
        let message = self.instrument_message(message, source, MessageOrigin::Posted);
        self.posted.post(message);
    }

    fn post_latest(&mut self, key: MessageKey, message: M, source: Option<WindowId>) {
        let metadata = if self.instrumentation.enabled() {
            (self.message_metadata)(&message)
        } else {
            MessageMetadata::unnamed()
        };
        let message = match self.posted.replace_latest(
            key,
            message,
            source,
            metadata,
            MessageOrigin::Posted,
            &mut self.instrumentation,
        ) {
            Ok(()) => return,
            Err(message) => message,
        };
        let trace = self.instrumentation.queued(
            metadata,
            source,
            MessageOrigin::Posted,
            self.epoch + self.now,
            self.posted.entries.len().saturating_add(1),
            Some(key),
        );
        self.posted
            .post_keyed(key, QueuedMessage::new(message, source, trace));
    }

    fn take_posted(&mut self) -> Vec<QueuedMessage<M>> {
        self.posted.take()
    }

    fn instrument_message(
        &mut self,
        message: M,
        source: Option<WindowId>,
        origin: MessageOrigin,
    ) -> QueuedMessage<M> {
        let trace = if self.instrumentation.enabled() {
            self.instrumentation.queued(
                (self.message_metadata)(&message),
                source,
                origin,
                self.epoch + self.now,
                self.posted.entries.len().saturating_add(1),
                None,
            )
        } else {
            None
        };
        QueuedMessage::new(message, source, trace)
    }

    fn message_dispatch_started(&mut self, dispatch: &mut MessageDispatch) {
        self.instrumentation
            .start_message(dispatch, self.epoch + self.now);
    }

    fn message_dispatch_finished(&mut self, dispatch: MessageDispatch, outcome: MessageOutcome) {
        self.instrumentation
            .finish_message(dispatch, self.epoch + self.now, outcome);
    }

    fn has_posted(&self) -> bool {
        !self.posted.is_empty()
    }

    fn proxy(&self) -> MessageProxy<M>
    where
        M: Send,
    {
        let queue = Arc::clone(&self.proxied);
        let sequence = Arc::clone(&self.external_sequence);
        MessageProxy::from_fn(move |message| {
            let order = sequence.fetch_add(1, Ordering::Relaxed);
            queue
                .lock()
                .map_err(|_| ProxyClosed)?
                .push_back((order, message));
            Ok(())
        })
    }

    fn register_task(&mut self, name: String) -> TaskSink<M> {
        let (task, state) = self.register_task_state(name);

        let submit_events = Arc::clone(&self.task_events);
        let submit_sequence = Arc::clone(&self.external_sequence);
        let submit_state = Arc::clone(&state);
        let submit = Box::new(move |factory: TaskMessageFactory<M>| {
            if submit_state
                .compare_exchange(
                    TASK_PENDING,
                    TASK_COMPLETION_QUEUED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                return Ok(TaskCompletionStatus::Cancelled);
            }
            let order = submit_sequence.fetch_add(1, Ordering::Relaxed);
            submit_events.lock().map_err(|_| ProxyClosed)?.push_back((
                order,
                TaskEvent::Complete {
                    id: task,
                    state: Arc::clone(&submit_state),
                    factory,
                },
            ));
            Ok(TaskCompletionStatus::Queued)
        });

        let abandon_events = Arc::clone(&self.task_events);
        let abandon_sequence = Arc::clone(&self.external_sequence);
        let abandon_state = Arc::clone(&state);
        let abandon = Box::new(move || {
            if abandon_state
                .compare_exchange(
                    TASK_PENDING,
                    TASK_CANCELLED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                return Ok(());
            }
            let order = abandon_sequence.fetch_add(1, Ordering::Relaxed);
            abandon_events.lock().map_err(|_| ProxyClosed)?.push_back((
                order,
                TaskEvent::Abandon {
                    id: task,
                    state: Arc::clone(&abandon_state),
                },
            ));
            Ok(())
        });

        TaskSink::new(task, submit, abandon)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn enqueue_blocking(
        &mut self,
        task: TaskId,
        job: Box<dyn FnOnce() + Send + 'static>,
    ) -> std::result::Result<(), TaskSpawnError> {
        let Some(entry) = self.tasks.get_mut(&task) else {
            return Err(TaskSpawnError::WorkerUnavailable);
        };
        entry.kind = TaskKind::Blocking;
        entry.blocking = Some(job);
        Ok(())
    }

    fn cancel_task(&mut self, task: TaskId) -> bool {
        let Some(entry) = self.tasks.remove(&task) else {
            return false;
        };
        entry.state.store(TASK_CANCELLED, Ordering::Release);
        true
    }

    fn runtime_snapshot(&self) -> RuntimeSnapshot {
        let mut tasks = self
            .tasks
            .iter()
            .map(|(&id, task)| {
                ActiveTaskSnapshot::new(
                    id,
                    task.name.clone(),
                    task.kind,
                    self.now.saturating_sub(task.started_at),
                )
            })
            .collect::<Vec<_>>();
        tasks.sort_by_key(ActiveTaskSnapshot::id);
        let mut subscriptions = self
            .subscriptions
            .iter()
            .map(|(&id, subscription)| {
                ActiveSubscriptionSnapshot::new(
                    id,
                    subscription.config.kind(),
                    subscription.config.delivery(),
                    subscription.config.interval(),
                    self.now.saturating_sub(subscription.started_at),
                    subscription.starts,
                    match &subscription.source {
                        HeadlessSubscriptionSource::Interval(_) => SubscriptionStatus::Running,
                        HeadlessSubscriptionSource::Service { status, .. } => *status,
                    },
                )
            })
            .collect::<Vec<_>>();
        subscriptions.sort_by_key(ActiveSubscriptionSnapshot::id);
        let (message_traces, coalesced_replacements) = self.instrumentation.snapshot();
        RuntimeSnapshot::with_messages(
            tasks,
            subscriptions,
            message_traces,
            self.posted.entries.len(),
            coalesced_replacements,
        )
    }

    fn reconcile_subscriptions(&mut self, desired: Subscriptions<M>) -> Result<()> {
        let desired = desired.into_unique()?;
        let desired_ids = desired
            .iter()
            .map(|subscription| subscription.id())
            .collect::<std::collections::HashSet<_>>();
        let removed = self
            .subscriptions
            .keys()
            .copied()
            .filter(|id| !desired_ids.contains(id))
            .collect::<Vec<_>>();
        for id in removed {
            if let Some(active) = self.subscriptions.remove(&id)
                && let HeadlessSubscriptionSource::Interval(timer) = active.source
            {
                self.cancel_timer(timer);
            }
        }

        for subscription in desired {
            let (id, config, factory) = subscription.into_parts();
            if let Some(active) = self.subscriptions.get_mut(&id)
                && active.config == config
            {
                match (&mut active.source, factory) {
                    (
                        HeadlessSubscriptionSource::Interval(timer),
                        SubscriptionFactory::Interval(factory),
                    ) => {
                        let entry = self
                            .timers
                            .iter_mut()
                            .find(|entry| entry.id == *timer)
                            .expect("active subscription timer exists");
                        let TimerKind::Interval {
                            factory: active_factory,
                            ..
                        } = &mut entry.kind
                        else {
                            unreachable!("interval subscription uses an interval timer")
                        };
                        *active_factory = factory;
                    }
                    (
                        HeadlessSubscriptionSource::Service { decode, .. },
                        SubscriptionFactory::Service(factory),
                    ) => *decode = factory.into_decoder(),
                    _ => {
                        unreachable!("equivalent subscription configurations have matching sources")
                    }
                }
                continue;
            }
            let starts = if let Some(active) = self.subscriptions.remove(&id) {
                if let HeadlessSubscriptionSource::Interval(timer) = active.source {
                    self.cancel_timer(timer);
                }
                active.starts.saturating_add(1)
            } else {
                1
            };
            let generation = self.next_subscription_generation;
            self.next_subscription_generation = generation.saturating_add(1);
            let source = match factory {
                SubscriptionFactory::Interval(factory) => HeadlessSubscriptionSource::Interval(
                    self.set_interval(
                        config
                            .interval()
                            .expect("interval configuration has a period"),
                        factory,
                    ),
                ),
                SubscriptionFactory::Service(factory) => {
                    let events = Arc::clone(&self.subscription_events);
                    let sequence = Arc::clone(&self.external_sequence);
                    let sink = RawSubscriptionSink::new(move |event| {
                        let order = sequence.fetch_add(1, Ordering::Relaxed);
                        let mut events = events.lock().expect("subscription event queue poisoned");
                        if let Some(entry) =
                            events
                                .iter_mut()
                                .find(|(_, queued_id, queued_generation, _)| {
                                    *queued_id == id && *queued_generation == generation
                                })
                        {
                            *entry = (order, id, generation, event);
                        } else {
                            events.push_back((order, id, generation, event));
                        }
                    });
                    let (started, decode) = factory.start(sink.clone());
                    let (guard, failure) = started.into_parts();
                    let status = if failure.is_some() {
                        SubscriptionStatus::Failed
                    } else {
                        SubscriptionStatus::Running
                    };
                    if let Some(failure) = failure {
                        sink.emit_raw(failure);
                    }
                    HeadlessSubscriptionSource::Service {
                        _guard: guard,
                        decode,
                        status,
                    }
                }
            };
            self.subscriptions.insert(
                id,
                HeadlessSubscription {
                    config,
                    generation,
                    source,
                    started_at: self.now,
                    starts,
                },
            );
        }
        Ok(())
    }

    fn cancel_all_subscriptions(&mut self) {
        let timers = self
            .subscriptions
            .drain()
            .filter_map(|(_, subscription)| match subscription.source {
                HeadlessSubscriptionSource::Interval(timer) => Some(timer),
                HeadlessSubscriptionSource::Service { .. } => None,
            })
            .collect::<Vec<_>>();
        for timer in timers {
            self.cancel_timer(timer);
        }
    }

    fn set_timeout(&mut self, delay: Duration, factory: Box<dyn FnOnce() -> M>) -> TimerId {
        let timer = self.alloc_timer();
        self.timers.push(TimerEntry {
            id: timer,
            due: self.now + delay,
            kind: TimerKind::Timeout(factory),
        });
        timer
    }

    fn set_interval(&mut self, interval: Duration, factory: Box<dyn FnMut() -> M>) -> TimerId {
        assert!(!interval.is_zero(), "timer interval must be non-zero");
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
        self.cancel_all_tasks();
        self.cancel_all_subscriptions();
    }
}

/// Drains messages posted with [`AppCx::post`] through [`App::update`],
/// mirroring the runner's bounded re-drain: up to [`MAX_POSTED_PASSES`]
/// passes, with any surplus retained for the next harness operation. In
/// debug builds an exhausted re-drain panics, exactly like the runner.
fn flush_posted<A: App>(app: &mut A, backend: &mut HeadlessBackend<A::Message>) -> Result<()> {
    for _ in 0..MAX_POSTED_PASSES {
        let batch = backend.posted.take();
        if batch.is_empty() {
            return Ok(());
        }
        for message in batch {
            dispatch_message(app, backend, message)?;
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
    origin: MessageOrigin,
) -> Result<()> {
    let had_windows = !backend.slots.is_empty();
    for message in messages {
        let message = backend.instrument_message(message, source, origin);
        dispatch_message(app, backend, message)?;
    }
    flush_posted(app, backend)?;
    if !backend.exited {
        let subscriptions = app.subscriptions();
        backend.reconcile_subscriptions(subscriptions)?;
    }
    if backend.exit_on_last_window_close && had_windows && backend.slots.is_empty() {
        backend.exited = true;
        backend.cancel_all_tasks();
        backend.cancel_all_subscriptions();
    }
    Ok(())
}

fn dispatch_message<A: App>(
    app: &mut A,
    backend: &mut HeadlessBackend<A::Message>,
    message: QueuedMessage<A::Message>,
) -> Result<()> {
    let (message, source, mut dispatch) = message.into_parts();
    backend.message_dispatch_started(&mut dispatch);
    let result = app.update(&mut AppCx::new(backend, source), message);
    backend.message_dispatch_finished(
        dispatch,
        if result.is_ok() {
            MessageOutcome::Success
        } else {
            MessageOutcome::Error
        },
    );
    result
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
        Self::new_with_instrumentation(app, RuntimeInstrumentationConfig::default())
    }

    /// Builds the application with opt-in payload-free runtime instrumentation.
    pub fn new_with_instrumentation(
        app: A,
        instrumentation: RuntimeInstrumentationConfig,
    ) -> Result<Self> {
        let backend = HeadlessBackend::new(instrumentation, A::message_metadata);
        let mut harness = Self { app, backend };
        harness
            .app
            .build(&mut AppCx::new(&mut harness.backend, None))?;
        flush_posted(&mut harness.app, &mut harness.backend)?;
        if !harness.backend.exited {
            let subscriptions = harness.app.subscriptions();
            harness.backend.reconcile_subscriptions(subscriptions)?;
        }
        harness.pump_external()?;
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

    /// Returns the number of messages currently waiting in the application
    /// posted-message queue.
    ///
    /// Messages waiting in a [`MessageProxy`] or scheduled timer are not
    /// included.
    pub fn pending_posted_count(&self) -> usize {
        self.backend.posted.entries.len()
    }

    /// Returns the cumulative number of pending messages replaced by keyed
    /// latest-value posts since this harness was created.
    pub fn coalesced_replacement_count(&self) -> u64 {
        self.backend.posted.replacements
    }

    /// Returns a point-in-time snapshot of active tasks and subscriptions.
    pub fn runtime_snapshot(&self) -> RuntimeSnapshot {
        self.backend.runtime_snapshot()
    }

    /// Returns active subscription identities in stable order.
    pub fn active_subscription_ids(&self) -> Vec<SubscriptionId> {
        let mut ids = self
            .backend
            .subscriptions
            .keys()
            .copied()
            .collect::<Vec<_>>();
        ids.sort();
        ids
    }

    /// Returns how many times one subscription identity has started.
    pub fn subscription_start_count(&self, id: SubscriptionId) -> u64 {
        self.backend
            .subscriptions
            .get(&id)
            .map(|subscription| subscription.starts)
            .unwrap_or(0)
    }

    /// Injects one event from an active subscription without advancing time.
    ///
    /// Returns `false` if the identity is not active.
    pub fn emit_subscription(&mut self, id: SubscriptionId) -> Result<bool> {
        let Some(active) = self.backend.subscriptions.get(&id) else {
            return Ok(false);
        };
        let HeadlessSubscriptionSource::Interval(timer) = &active.source else {
            return Ok(false);
        };
        let timer = *timer;
        let generation = active.generation;
        let Some(entry) = self
            .backend
            .timers
            .iter_mut()
            .find(|entry| entry.id == timer)
        else {
            return Ok(false);
        };
        let TimerKind::Interval { factory, .. } = &mut entry.kind else {
            return Ok(false);
        };
        let message = factory();
        let still_active = self
            .backend
            .subscriptions
            .get(&id)
            .is_some_and(|active| active.generation == generation);
        if still_active {
            dispatch_batch(
                &mut self.app,
                &mut self.backend,
                None,
                vec![message],
                MessageOrigin::Subscription(id),
            )?;
        }
        self.pump_external()?;
        Ok(still_active)
    }

    /// Returns active task identifiers in creation order.
    pub fn pending_task_ids(&self) -> Vec<TaskId> {
        let mut tasks: Vec<_> = self.backend.tasks.keys().copied().collect();
        tasks.sort_by_key(|task| task.raw());
        tasks
    }

    /// Completes one active task with an injected application message.
    ///
    /// This bypasses an externally owned executor and lets a test choose task
    /// completion order. Returns `false` when the task is no longer active.
    pub fn complete_task(&mut self, task: TaskId, message: A::Message) -> Result<bool> {
        let Some(entry) = self.backend.tasks.remove(&task) else {
            return Ok(false);
        };
        entry.state.store(TASK_FINISHED, Ordering::Release);
        dispatch_batch(
            &mut self.app,
            &mut self.backend,
            None,
            vec![message],
            MessageOrigin::Task(task),
        )?;
        self.pump_external()?;
        Ok(true)
    }

    /// Runs one recorded [`AppCx::spawn_blocking`] job synchronously.
    ///
    /// No worker thread is created. Returns `false` when the task is not an
    /// active queued blocking job.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn run_blocking_task(&mut self, task: TaskId) -> Result<bool> {
        let Some(job) = self
            .backend
            .tasks
            .get_mut(&task)
            .and_then(|entry| entry.blocking.take())
        else {
            return Ok(false);
        };
        job();
        self.pump_external()?;
        Ok(true)
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
        dispatch_batch(
            &mut self.app,
            &mut self.backend,
            Some(window),
            messages,
            MessageOrigin::Ui,
        )?;
        self.pump_external()
    }

    /// Dispatches one message through [`App::update`] with no source window,
    /// the path a [`MessageProxy`] or external event takes.
    pub fn post(&mut self, message: A::Message) -> Result<()> {
        dispatch_batch(
            &mut self.app,
            &mut self.backend,
            None,
            vec![message],
            MessageOrigin::External,
        )?;
        self.pump_external()
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
        flush_posted(&mut self.app, &mut self.backend)?;
        let subscriptions = self.app.subscriptions();
        self.backend.reconcile_subscriptions(subscriptions)?;
        if self.backend.exit_on_last_window_close && self.backend.slots.is_empty() {
            self.backend.exited = true;
            self.backend.cancel_all_tasks();
            self.backend.cancel_all_subscriptions();
        }
        self.pump_external()
    }

    /// Advances the virtual clock, firing every timer that becomes due.
    ///
    /// Timers fire in deadline order (creation order between equal
    /// deadlines) and their messages dispatch through [`App::update`] with no
    /// source window, like the runner's native timers. A repeating timer fires
    /// at most once per call; missed periods are coalesced and its next
    /// deadline advances past the target time. Timers scheduled by a callback
    /// wait for a subsequent call, including zero-delay timeouts.
    /// `advance(Duration::ZERO)` therefore acts as one deterministic
    /// event-loop turn and also pumps queued [`MessageProxy`] messages.
    pub fn advance(&mut self, duration: Duration) -> Result<()> {
        let target = self.backend.now + duration;
        self.backend.now = target;
        let mut due: Vec<_> = self
            .backend
            .timers
            .iter()
            .filter(|entry| entry.due <= target)
            .map(|entry| (entry.due, entry.id))
            .collect();
        due.sort_by_key(|(deadline, id)| (*deadline, id.raw()));

        for (deadline, timer) in due {
            let Some(index) = self
                .backend
                .timers
                .iter()
                .position(|entry| entry.id == timer && entry.due == deadline)
            else {
                // A preceding callback cancelled this due timer.
                continue;
            };
            let entry = self.backend.timers.remove(index);
            match entry.kind {
                TimerKind::Timeout(factory) => {
                    dispatch_batch(
                        &mut self.app,
                        &mut self.backend,
                        None,
                        vec![factory()],
                        MessageOrigin::Timeout(timer),
                    )?;
                }
                TimerKind::Interval {
                    period,
                    mut factory,
                } => {
                    let message = factory();
                    let mut next_due = entry.due + period;
                    while next_due <= target {
                        next_due += period;
                    }
                    // Reschedule before dispatching so the handler can cancel
                    // the interval through `AppCx::cancel_timer`.
                    self.backend.timers.push(TimerEntry {
                        id: entry.id,
                        due: next_due,
                        kind: TimerKind::Interval { period, factory },
                    });
                    dispatch_batch(
                        &mut self.app,
                        &mut self.backend,
                        None,
                        vec![message],
                        MessageOrigin::Interval(timer),
                    )?;
                }
            }
        }
        self.pump_external()
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

    /// Dispatches proxy messages and task events in submission order, with
    /// the same bounded re-drain as posted messages.
    fn pump_external(&mut self) -> Result<()> {
        for _ in 0..MAX_POSTED_PASSES {
            let proxied: Vec<_> = {
                let mut queue = self.backend.proxied.lock().expect("proxy queue poisoned");
                queue.drain(..).collect()
            };
            let tasks: Vec<_> = {
                let mut queue = self
                    .backend
                    .task_events
                    .lock()
                    .expect("task event queue poisoned");
                queue.drain(..).collect()
            };
            let subscriptions: Vec<_> = {
                let mut queue = self
                    .backend
                    .subscription_events
                    .lock()
                    .expect("subscription event queue poisoned");
                queue.drain(..).collect()
            };
            let mut batch: Vec<_> = proxied
                .into_iter()
                .map(|(order, message)| (order, ExternalEvent::Message(message)))
                .chain(
                    tasks
                        .into_iter()
                        .map(|(order, event)| (order, ExternalEvent::Task(event))),
                )
                .chain(
                    subscriptions
                        .into_iter()
                        .map(|(order, id, generation, event)| {
                            (
                                order,
                                ExternalEvent::Subscription {
                                    id,
                                    generation,
                                    event,
                                },
                            )
                        }),
                )
                .collect();
            if batch.is_empty() {
                return Ok(());
            }
            batch.sort_by_key(|(order, _)| *order);
            for (_, event) in batch {
                match event {
                    ExternalEvent::Message(message) => {
                        dispatch_batch(
                            &mut self.app,
                            &mut self.backend,
                            None,
                            vec![message],
                            MessageOrigin::Proxy,
                        )?;
                    }
                    ExternalEvent::Task(TaskEvent::Complete { id, state, factory }) => {
                        let active = self
                            .backend
                            .tasks
                            .get(&id)
                            .is_some_and(|entry| Arc::ptr_eq(&entry.state, &state));
                        if active && state.load(Ordering::Acquire) == TASK_COMPLETION_QUEUED {
                            self.backend.tasks.remove(&id);
                            state.store(TASK_FINISHED, Ordering::Release);
                            dispatch_batch(
                                &mut self.app,
                                &mut self.backend,
                                None,
                                vec![factory()],
                                MessageOrigin::Task(id),
                            )?;
                        }
                    }
                    ExternalEvent::Task(TaskEvent::Abandon { id, state }) => {
                        let active = self
                            .backend
                            .tasks
                            .get(&id)
                            .is_some_and(|entry| Arc::ptr_eq(&entry.state, &state));
                        if active {
                            self.backend.tasks.remove(&id);
                        }
                    }
                    ExternalEvent::Subscription {
                        id,
                        generation,
                        event,
                    } => {
                        let message =
                            self.backend
                                .subscriptions
                                .get_mut(&id)
                                .and_then(|subscription| {
                                    if subscription.generation != generation {
                                        return None;
                                    }
                                    let HeadlessSubscriptionSource::Service { decode, .. } =
                                        &mut subscription.source
                                    else {
                                        return None;
                                    };
                                    Some(decode(event))
                                });
                        if let Some(message) = message {
                            dispatch_batch(
                                &mut self.app,
                                &mut self.backend,
                                None,
                                vec![message],
                                MessageOrigin::Subscription(id),
                            )?;
                        }
                    }
                }
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
    use rxui_app::RuntimeEvent;
    #[cfg(not(target_arch = "wasm32"))]
    use rxui_app::TaskError;
    use rxui_app::{MessageMapper, Subscription, TaskCompletion};

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
    fn missed_interval_periods_coalesce_per_advance() {
        let mut harness = harness();
        harness.post(Msg::ScheduleInterval).expect("post succeeds");
        harness
            .advance(Duration::from_secs(3))
            .expect("advance succeeds");
        assert_eq!(harness.app().ticks, 1);
        harness
            .advance(Duration::from_secs(1))
            .expect("advance succeeds");
        assert_eq!(harness.app().ticks, 2);
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
        ScheduleTimeoutFactory,
        ScheduleCancelledTimeout,
        Timeout,
        FactoryTimeout,
        ScheduleFixedInterval,
        FixedTick,
        ScheduleFactoryInterval,
        FactoryTick(u32),
        ScheduleNested,
        ScheduleLatestBurst,
        Latest(u32),
        #[cfg(not(target_arch = "wasm32"))]
        ScheduleBlocking,
        #[cfg(not(target_arch = "wasm32"))]
        Blocking(u32),
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
                LocalMsg::ScheduleTimeoutFactory => {
                    cx.set_timeout_with(Duration::from_secs(1), || LocalMsg::FactoryTimeout);
                }
                LocalMsg::ScheduleCancelledTimeout => {
                    let timer = cx.set_timeout(Duration::from_secs(1), LocalMsg::Timeout);
                    let cancelled = cx.cancel_timer(timer);
                    self.log.push((format!("cancelled:{cancelled}"), source));
                }
                LocalMsg::Timeout => self.log.push(("timeout".into(), source)),
                LocalMsg::FactoryTimeout => {
                    self.log.push(("factory-timeout".into(), source));
                }
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
                LocalMsg::ScheduleLatestBurst => {
                    let key = MessageKey::singleton("testing.latest");
                    for value in 1..=1_000 {
                        cx.post_latest(key, LocalMsg::Latest(value));
                    }
                }
                LocalMsg::Latest(value) => {
                    self.log.push((format!("latest:{value}"), source));
                }
                #[cfg(not(target_arch = "wasm32"))]
                LocalMsg::ScheduleBlocking => {
                    cx.spawn_blocking(
                        || 41_u32,
                        |result| LocalMsg::Blocking(result.expect("worker succeeds") + 1),
                    )
                    .expect("blocking task queues");
                }
                #[cfg(not(target_arch = "wasm32"))]
                LocalMsg::Blocking(value) => {
                    self.log.push((format!("blocking:{value}"), source));
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
        assert_eq!(fixed.app().log, [("fixed".into(), None)]);
        fixed
            .advance(Duration::from_secs(1))
            .expect("fixed interval advances again");
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
        assert_eq!(factory.app().log, [("factory:1".into(), None)]);
        factory
            .advance(Duration::from_secs(1))
            .expect("factory interval advances again");
        assert_eq!(
            factory.app().log,
            [("factory:1".into(), None), ("factory:2".into(), None)]
        );
    }

    #[test]
    fn mapped_timeout_factory_runs_at_the_deadline() {
        let mut harness = mapped_harness();
        harness
            .post(RootMsg::Feature(LocalMsg::ScheduleTimeoutFactory))
            .expect("factory timeout schedules");
        harness
            .advance(Duration::from_millis(999))
            .expect("clock advances before deadline");
        assert!(harness.app().log.is_empty());
        harness
            .advance(Duration::from_millis(1))
            .expect("clock reaches deadline");
        assert_eq!(harness.app().log, [("factory-timeout".into(), None)]);
    }

    #[test]
    fn mapped_latest_posts_collapse_a_large_burst() {
        let mut harness = mapped_harness();
        harness
            .post(RootMsg::Feature(LocalMsg::ScheduleLatestBurst))
            .expect("latest burst posts");
        assert_eq!(harness.app().log, [("latest:1000".into(), None)]);
        assert_eq!(harness.pending_posted_count(), 0);
        assert_eq!(harness.coalesced_replacement_count(), 999);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn mapped_blocking_task_delivers_without_a_source_window() {
        let mut harness = mapped_harness();
        harness
            .post(RootMsg::Feature(LocalMsg::ScheduleBlocking))
            .expect("blocking task schedules");
        let task = harness.pending_task_ids()[0];
        assert!(harness.run_blocking_task(task).expect("blocking task runs"));
        assert_eq!(harness.app().log, [("blocking:42".into(), None)]);
        assert!(harness.pending_task_ids().is_empty());
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

    enum FactoryMsg {
        Schedule,
        ScheduleCancelled,
        ScheduleZeroTimeout,
        ScheduleZeroInterval,
        Fired(u32),
    }

    struct FactoryFixture {
        calls: Rc<std::cell::Cell<u32>>,
        delivered: Vec<u32>,
    }

    impl App for FactoryFixture {
        type Message = FactoryMsg;

        fn build(&mut self, _cx: &mut AppCx<'_, FactoryMsg>) -> Result<()> {
            Ok(())
        }

        fn update(&mut self, cx: &mut AppCx<'_, FactoryMsg>, message: FactoryMsg) -> Result<()> {
            match message {
                FactoryMsg::Schedule => {
                    let calls = Rc::clone(&self.calls);
                    cx.set_timeout_with(Duration::from_secs(1), move || {
                        calls.set(calls.get() + 1);
                        FactoryMsg::Fired(7)
                    });
                }
                FactoryMsg::ScheduleCancelled => {
                    let calls = Rc::clone(&self.calls);
                    let timer = cx.set_timeout_with(Duration::from_secs(1), move || {
                        calls.set(calls.get() + 1);
                        FactoryMsg::Fired(8)
                    });
                    assert!(cx.cancel_timer(timer));
                }
                FactoryMsg::ScheduleZeroTimeout => {
                    cx.set_timeout_with(Duration::ZERO, || FactoryMsg::Fired(9));
                }
                FactoryMsg::ScheduleZeroInterval => {
                    cx.set_interval_with(Duration::ZERO, || FactoryMsg::Fired(10));
                }
                FactoryMsg::Fired(value) => self.delivered.push(value),
            }
            Ok(())
        }
    }

    fn factory_harness() -> (AppHarness<FactoryFixture>, Rc<std::cell::Cell<u32>>) {
        let calls = Rc::new(std::cell::Cell::new(0));
        let harness = AppHarness::new(FactoryFixture {
            calls: Rc::clone(&calls),
            delivered: Vec::new(),
        })
        .expect("factory fixture builds");
        (harness, calls)
    }

    #[test]
    fn non_clone_timeout_factory_runs_once_and_cancellation_drops_it() {
        let (mut fired, fired_calls) = factory_harness();
        fired.post(FactoryMsg::Schedule).expect("timeout schedules");
        assert_eq!(fired_calls.get(), 0);
        fired
            .advance(Duration::from_secs(1))
            .expect("timeout fires");
        assert_eq!(fired_calls.get(), 1);
        assert_eq!(fired.app().delivered, [7]);

        let (mut cancelled, cancelled_calls) = factory_harness();
        cancelled
            .post(FactoryMsg::ScheduleCancelled)
            .expect("timeout schedules and cancels");
        cancelled
            .advance(Duration::from_secs(2))
            .expect("clock advances");
        assert_eq!(cancelled_calls.get(), 0);
        assert!(cancelled.app().delivered.is_empty());
    }

    #[test]
    fn callback_scheduled_zero_timeout_waits_for_the_next_turn() {
        let (mut harness, _) = factory_harness();
        harness
            .post(FactoryMsg::ScheduleZeroTimeout)
            .expect("zero timeout schedules");
        assert!(harness.app().delivered.is_empty());
        harness
            .advance(Duration::ZERO)
            .expect("next event-loop turn runs");
        assert_eq!(harness.app().delivered, [9]);
    }

    #[test]
    #[should_panic(expected = "timer interval must be non-zero")]
    fn zero_interval_is_rejected_like_the_runtime() {
        let (mut harness, _) = factory_harness();
        let _ = harness.post(FactoryMsg::ScheduleZeroInterval);
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

    enum TaskMsg {
        #[cfg(not(target_arch = "wasm32"))]
        StartBlocking,
        #[cfg(not(target_arch = "wasm32"))]
        StartPanicking,
        StartExternal,
        StartNamed,
        DropExternal,
        #[cfg(not(target_arch = "wasm32"))]
        Cancel(TaskId),
        #[cfg(not(target_arch = "wasm32"))]
        Exit,
        Delivered(Rc<String>),
    }

    #[derive(Default)]
    struct TaskFixture {
        completions: Vec<TaskCompletion<u32>>,
        log: Vec<String>,
        last_source: Option<WindowId>,
    }

    impl App for TaskFixture {
        type Message = TaskMsg;

        fn build(&mut self, _cx: &mut AppCx<'_, TaskMsg>) -> Result<()> {
            Ok(())
        }

        fn update(&mut self, cx: &mut AppCx<'_, TaskMsg>, message: TaskMsg) -> Result<()> {
            match message {
                #[cfg(not(target_arch = "wasm32"))]
                TaskMsg::StartBlocking => {
                    cx.spawn_blocking(
                        || 7_u32,
                        |result| {
                            TaskMsg::Delivered(Rc::new(
                                result.expect("worker succeeds").to_string(),
                            ))
                        },
                    )
                    .expect("blocking task queues");
                }
                #[cfg(not(target_arch = "wasm32"))]
                TaskMsg::StartPanicking => {
                    cx.spawn_blocking(
                        || -> u32 { panic!("private panic payload") },
                        |result| {
                            let text = match result {
                                Ok(value) => value.to_string(),
                                Err(TaskError::Panicked) => "panicked".into(),
                                Err(_) => "task error".into(),
                            };
                            TaskMsg::Delivered(Rc::new(text))
                        },
                    )
                    .expect("panicking task queues");
                }
                TaskMsg::StartExternal => {
                    self.completions.push(cx.register_task(|value: u32| {
                        TaskMsg::Delivered(Rc::new(value.to_string()))
                    }));
                }
                TaskMsg::StartNamed => {
                    self.completions.push(
                        cx.register_task_named("Load named fixture", |value: u32| {
                            TaskMsg::Delivered(Rc::new(value.to_string()))
                        }),
                    );
                }
                TaskMsg::DropExternal => self.completions.clear(),
                #[cfg(not(target_arch = "wasm32"))]
                TaskMsg::Cancel(task) => {
                    self.log.push(format!("cancelled:{}", cx.cancel_task(task)));
                }
                #[cfg(not(target_arch = "wasm32"))]
                TaskMsg::Exit => cx.exit(),
                TaskMsg::Delivered(value) => {
                    self.last_source = cx.source_window();
                    self.log.push((*value).clone());
                }
            }
            Ok(())
        }
    }

    fn task_harness() -> AppHarness<TaskFixture> {
        AppHarness::new(TaskFixture::default()).expect("task fixture builds")
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn blocking_tasks_create_non_send_messages_on_the_ui_thread() {
        let mut harness = task_harness();
        harness
            .post(TaskMsg::StartBlocking)
            .expect("blocking task schedules");
        let task = harness.pending_task_ids()[0];
        assert!(harness.run_blocking_task(task).expect("task runs"));
        assert_eq!(harness.app().log, ["7"]);
        assert_eq!(harness.app().last_source, None);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn blocking_panics_become_typed_task_errors() {
        let mut harness = task_harness();
        harness
            .post(TaskMsg::StartPanicking)
            .expect("panicking task schedules");
        let task = harness.pending_task_ids()[0];
        assert!(harness.run_blocking_task(task).expect("task runs"));
        assert_eq!(harness.app().log, ["panicked"]);
    }

    #[test]
    fn external_completion_and_abandonment_are_deterministic() {
        let mut harness = task_harness();
        harness
            .post(TaskMsg::StartExternal)
            .expect("external task registers");
        let completion = harness
            .app_mut()
            .completions
            .pop()
            .expect("completion exists");
        assert_eq!(
            completion.complete(9).expect("runtime remains open"),
            TaskCompletionStatus::Queued
        );
        harness.advance(Duration::ZERO).expect("completion pumps");
        assert_eq!(harness.app().log, ["9"]);

        harness
            .post(TaskMsg::StartExternal)
            .expect("second task registers");
        assert_eq!(harness.pending_task_ids().len(), 1);
        harness
            .post(TaskMsg::DropExternal)
            .expect("completion drops");
        assert!(harness.pending_task_ids().is_empty());
    }

    #[test]
    fn runtime_snapshot_reports_named_task_metadata_on_virtual_time() {
        let mut harness = task_harness();
        harness.post(TaskMsg::StartNamed).expect("task registers");
        harness
            .advance(Duration::from_millis(25))
            .expect("virtual clock advances");
        let snapshot = harness.runtime_snapshot();
        let task = &snapshot.active_tasks()[0];
        assert_eq!(task.name(), "Load named fixture");
        assert_eq!(task.kind(), TaskKind::External);
        assert_eq!(task.elapsed(), Duration::from_millis(25));
        harness.post(TaskMsg::DropExternal).expect("task abandons");
        assert!(harness.runtime_snapshot().active_tasks().is_empty());
    }

    #[test]
    fn harness_can_complete_tasks_in_a_chosen_order() {
        let mut harness = task_harness();
        harness
            .post(TaskMsg::StartExternal)
            .expect("first task registers");
        harness
            .post(TaskMsg::StartExternal)
            .expect("second task registers");
        let tasks = harness.pending_task_ids();
        assert!(
            harness
                .complete_task(tasks[1], TaskMsg::Delivered(Rc::new("second".into())))
                .expect("second task completes")
        );
        assert!(
            harness
                .complete_task(tasks[0], TaskMsg::Delivered(Rc::new("first".into())))
                .expect("first task completes")
        );
        assert_eq!(harness.app().log, ["second", "first"]);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn cancellation_and_exit_suppress_task_delivery() {
        let mut harness = task_harness();
        harness
            .post(TaskMsg::StartBlocking)
            .expect("blocking task registers");
        let task = harness.pending_task_ids()[0];
        harness.post(TaskMsg::Cancel(task)).expect("task cancels");
        assert_eq!(harness.app().log, ["cancelled:true"]);
        assert!(!harness.run_blocking_task(task).expect("task is gone"));

        harness
            .post(TaskMsg::StartExternal)
            .expect("external task registers");
        harness.post(TaskMsg::Exit).expect("app exits");
        assert!(harness.exited());
        assert!(harness.pending_task_ids().is_empty());
    }

    #[derive(Clone)]
    enum SubscriptionMsg {
        SetLive(bool),
        SetPayload(u32),
        SetPeriod(Duration),
        Tick(u32),
        Noop,
        Exit,
    }

    struct SubscriptionFixture {
        live: bool,
        payload: u32,
        period: Duration,
        ticks: Vec<u32>,
    }

    impl Default for SubscriptionFixture {
        fn default() -> Self {
            Self {
                live: false,
                payload: 1,
                period: Duration::from_secs(1),
                ticks: Vec::new(),
            }
        }
    }

    impl App for SubscriptionFixture {
        type Message = SubscriptionMsg;

        fn build(&mut self, _cx: &mut AppCx<'_, Self::Message>) -> Result<()> {
            Ok(())
        }

        fn subscriptions(&self) -> Subscriptions<Self::Message> {
            if self.live {
                let payload = self.payload;
                Subscriptions::one(Subscription::interval_with(
                    SubscriptionId::singleton("fixture.live"),
                    self.period,
                    move || SubscriptionMsg::Tick(payload),
                ))
            } else {
                Subscriptions::none()
            }
        }

        fn update(
            &mut self,
            cx: &mut AppCx<'_, Self::Message>,
            message: Self::Message,
        ) -> Result<()> {
            match message {
                SubscriptionMsg::SetLive(live) => self.live = live,
                SubscriptionMsg::SetPayload(payload) => self.payload = payload,
                SubscriptionMsg::SetPeriod(period) => self.period = period,
                SubscriptionMsg::Tick(value) => self.ticks.push(value),
                SubscriptionMsg::Noop => {}
                SubscriptionMsg::Exit => cx.exit(),
            }
            Ok(())
        }
    }

    #[test]
    fn subscriptions_reconcile_once_and_replace_factories_without_restart() {
        let id = SubscriptionId::singleton("fixture.live");
        let mut harness = AppHarness::new(SubscriptionFixture::default()).unwrap();
        assert!(harness.active_subscription_ids().is_empty());

        harness
            .post(SubscriptionMsg::SetLive(true))
            .expect("subscription starts");
        assert_eq!(harness.active_subscription_ids(), [id]);
        assert_eq!(harness.subscription_start_count(id), 1);

        harness
            .post(SubscriptionMsg::SetPayload(7))
            .expect("factory changes");
        harness
            .post(SubscriptionMsg::Noop)
            .expect("unrelated update");
        assert_eq!(harness.subscription_start_count(id), 1);
        assert!(harness.emit_subscription(id).expect("event injects"));
        assert_eq!(harness.app().ticks, [7]);

        harness
            .post(SubscriptionMsg::SetPeriod(Duration::from_secs(2)))
            .expect("configuration changes");
        assert_eq!(harness.subscription_start_count(id), 2);
        assert_eq!(
            harness.runtime_snapshot().active_subscriptions()[0].interval(),
            Some(Duration::from_secs(2))
        );
        harness
            .advance(Duration::from_secs(2))
            .expect("interval fires");
        assert_eq!(harness.app().ticks, [7, 7]);

        harness
            .post(SubscriptionMsg::SetLive(false))
            .expect("subscription cancels");
        assert!(harness.active_subscription_ids().is_empty());
        assert!(
            !harness
                .emit_subscription(id)
                .expect("inactive event rejects")
        );

        let mut exiting = AppHarness::new(SubscriptionFixture::default()).unwrap();
        exiting.post(SubscriptionMsg::SetLive(true)).unwrap();
        exiting.post(SubscriptionMsg::Exit).unwrap();
        assert!(exiting.exited());
        assert!(exiting.active_subscription_ids().is_empty());
    }

    struct DuplicateSubscriptions;

    impl App for DuplicateSubscriptions {
        type Message = ();

        fn build(&mut self, _cx: &mut AppCx<'_, Self::Message>) -> Result<()> {
            Ok(())
        }

        fn subscriptions(&self) -> Subscriptions<Self::Message> {
            let id = SubscriptionId::singleton("duplicate");
            Subscriptions::batch([
                Subscription::interval(id, Duration::from_secs(1), ()),
                Subscription::interval(id, Duration::from_secs(2), ()),
            ])
        }

        fn update(
            &mut self,
            _cx: &mut AppCx<'_, Self::Message>,
            _message: Self::Message,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn duplicate_subscription_ids_fail_initial_reconciliation() {
        assert!(AppHarness::new(DuplicateSubscriptions).is_err());
    }

    #[derive(Clone, Copy)]
    enum TraceMsg {
        Burst,
        Preview(u32),
        Timeout,
    }

    struct TraceApp;

    impl App for TraceApp {
        type Message = TraceMsg;

        fn build(&mut self, cx: &mut AppCx<'_, Self::Message>) -> Result<()> {
            cx.set_timeout(Duration::from_millis(5), TraceMsg::Timeout);
            Ok(())
        }

        fn message_metadata(message: &Self::Message) -> MessageMetadata {
            match message {
                TraceMsg::Burst => MessageMetadata::new("Burst", "test"),
                TraceMsg::Preview(_) => MessageMetadata::new("Preview", "test"),
                TraceMsg::Timeout => MessageMetadata::new("Timeout", "test"),
            }
        }

        fn update(
            &mut self,
            cx: &mut AppCx<'_, Self::Message>,
            message: Self::Message,
        ) -> Result<()> {
            if let TraceMsg::Burst = message {
                for value in 0..3 {
                    cx.post_latest(
                        MessageKey::singleton("trace.preview"),
                        TraceMsg::Preview(value),
                    );
                }
            }
            if let TraceMsg::Preview(value) = message {
                assert_eq!(value, 2);
            }
            Ok(())
        }
    }

    #[test]
    fn instrumentation_reports_order_coalescing_and_timer_origins() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&events);
        let config = RuntimeInstrumentationConfig::default()
            .message_history(2)
            .observer(move |event: &RuntimeEvent| {
                observed.lock().unwrap().push(event.clone());
            });
        let mut harness = AppHarness::new_with_instrumentation(TraceApp, config).unwrap();

        harness.post(TraceMsg::Burst).unwrap();
        let burst_snapshot = harness.runtime_snapshot();
        assert_eq!(burst_snapshot.message_traces().len(), 2);
        let burst = &burst_snapshot.message_traces()[0];
        assert_eq!(burst.identity().metadata().name(), "Burst");
        assert_eq!(burst.identity().origin(), MessageOrigin::External);
        assert_eq!(burst.emitted(), 3);
        let preview = &burst_snapshot.message_traces()[1];
        assert_eq!(preview.replacements(), 2);
        assert_eq!(preview.key(), Some(MessageKey::singleton("trace.preview")));
        assert_eq!(burst_snapshot.coalesced_replacements(), 2);

        harness.advance(Duration::from_millis(5)).unwrap();
        let snapshot = harness.runtime_snapshot();
        assert_eq!(snapshot.message_traces().len(), 2);
        assert!(matches!(
            snapshot.message_traces()[1].identity().origin(),
            MessageOrigin::Timeout(_)
        ));

        let events = events.lock().unwrap();
        let burst_sequence = burst.identity().sequence();
        let kinds = events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::MessageQueued { identity, .. }
                    if identity.sequence() == burst_sequence =>
                {
                    Some("queued")
                }
                RuntimeEvent::MessageDispatchStarted { identity, .. }
                    if identity.sequence() == burst_sequence =>
                {
                    Some("started")
                }
                RuntimeEvent::MessageDispatchFinished(trace)
                    if trace.identity().sequence() == burst_sequence =>
                {
                    Some("finished")
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(kinds, ["queued", "started", "finished"]);
    }

    #[test]
    fn instrumentation_associates_task_and_subscription_origins() {
        let config = || RuntimeInstrumentationConfig::default().message_history(8);

        let mut tasks = AppHarness::new_with_instrumentation(TaskFixture::default(), config())
            .expect("task fixture builds");
        tasks.post(TaskMsg::StartExternal).unwrap();
        let task = tasks.pending_task_ids()[0];
        tasks
            .complete_task(task, TaskMsg::Delivered(Rc::new("done".into())))
            .unwrap();
        assert!(matches!(
            tasks
                .runtime_snapshot()
                .message_traces()
                .last()
                .unwrap()
                .identity()
                .origin(),
            MessageOrigin::Task(id) if id == task
        ));

        let id = SubscriptionId::singleton("fixture.live");
        let mut subscriptions =
            AppHarness::new_with_instrumentation(SubscriptionFixture::default(), config())
                .expect("subscription fixture builds");
        subscriptions.post(SubscriptionMsg::SetLive(true)).unwrap();
        subscriptions.emit_subscription(id).unwrap();
        assert!(matches!(
            subscriptions
                .runtime_snapshot()
                .message_traces()
                .last()
                .unwrap()
                .identity()
                .origin(),
            MessageOrigin::Subscription(origin) if origin == id
        ));
    }

    static METADATA_CALLS: AtomicU64 = AtomicU64::new(0);

    struct DisabledInstrumentationApp;

    impl App for DisabledInstrumentationApp {
        type Message = ();

        fn build(&mut self, _cx: &mut AppCx<'_, Self::Message>) -> Result<()> {
            Ok(())
        }

        fn message_metadata(_message: &Self::Message) -> MessageMetadata {
            METADATA_CALLS.fetch_add(1, Ordering::Relaxed);
            MessageMetadata::named("Unit")
        }

        fn update(
            &mut self,
            _cx: &mut AppCx<'_, Self::Message>,
            _message: Self::Message,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn disabled_instrumentation_skips_metadata_and_history() {
        METADATA_CALLS.store(0, Ordering::Relaxed);
        let mut harness = AppHarness::new(DisabledInstrumentationApp).unwrap();
        harness.post(()).unwrap();
        assert_eq!(METADATA_CALLS.load(Ordering::Relaxed), 0);
        assert!(harness.runtime_snapshot().message_traces().is_empty());
    }

    struct FailingInstrumentationApp;

    impl App for FailingInstrumentationApp {
        type Message = Rc<String>;

        fn build(&mut self, _cx: &mut AppCx<'_, Self::Message>) -> Result<()> {
            Ok(())
        }

        fn message_metadata(_message: &Self::Message) -> MessageMetadata {
            MessageMetadata::new("PrivateFailure", "test")
        }

        fn update(
            &mut self,
            _cx: &mut AppCx<'_, Self::Message>,
            _message: Self::Message,
        ) -> Result<()> {
            Err(Error::msg("sensitive payload must not be retained"))
        }
    }

    #[test]
    fn instrumentation_records_errors_without_requiring_send_or_debug() {
        let mut harness = AppHarness::new_with_instrumentation(
            FailingInstrumentationApp,
            RuntimeInstrumentationConfig::default().message_history(1),
        )
        .unwrap();
        assert!(harness.post(Rc::new("secret".into())).is_err());
        let snapshot = harness.runtime_snapshot();
        assert_eq!(snapshot.message_traces().len(), 1);
        assert_eq!(
            snapshot.message_traces()[0].outcome(),
            MessageOutcome::Error
        );
        assert_eq!(
            snapshot.message_traces()[0].identity().metadata().name(),
            "PrivateFailure"
        );
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
