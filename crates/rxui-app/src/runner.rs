//! High-level application runner that owns windows, routes messages, and
//! drives the Astrelis runtime loop.
//!
//! [`run`] (or [`run_with`]) starts an application implementing [`App`]. The
//! runner opens windows through [`AppCx`], feeds platform events into each
//! window's [`WindowHost`], dispatches typed UI messages into [`App::update`],
//! and schedules redraws — replacing the per-application event-loop
//! boilerplate that direct `astrelis_app::App` implementations require.

use std::{
    any::Any,
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

#[cfg(not(target_arch = "wasm32"))]
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::mpsc::{self, SyncSender, TrySendError},
    thread,
};

#[cfg(not(target_arch = "wasm32"))]
use astrelis_app::RuntimeError;
use astrelis_app::{AppContext, Runtime, TimerId as NativeTimerId};
use astrelis_core::color::Color;
use astrelis_platform::{PlatformError, Window};
use astrelis_text::FontDatabase;
use astrelis_ui_host::{GraphicsContext, HostUpdate, WindowHost, WindowHostOptions};

use crate::error::{DynAppError, Error, Result};
use crate::instrumentation::{
    InstrumentationState, MessageDispatch, MessageMetadata, MessageOrigin, MessageOutcome,
    MessageTrace, QueuedMessage, RuntimeInstrumentationConfig,
};
use crate::subscription::{
    ActiveSubscriptionSnapshot, RawSubscriptionEvent, RawSubscriptionSink, SubscriptionConfig,
    SubscriptionFactory, SubscriptionId, SubscriptionStatus, Subscriptions,
};

pub use astrelis_app::{FixedStep, RuntimeConfig, RuntimePolicy, UpdateInfo};
/// Platform-portable instant; aliases `web_time::Instant` on the web.
pub use astrelis_platform::Instant;
pub use astrelis_platform::{Clipboard, Monitor, WindowAttributes, WindowEvent, WindowId};
pub use astrelis_text::FontDatabaseOptions;
pub use astrelis_ui_core::{Theme, Ui};

/// Maximum passes over messages posted from [`App::update`] before the runner
/// defers the remainder to the next event-loop turn.
const MAX_POSTED_PASSES: usize = 8;

const TASK_PENDING: u8 = 0;
const TASK_COMPLETION_QUEUED: u8 = 1;
const TASK_CANCELLED: u8 = 2;
const TASK_FINISHED: u8 = 3;

/// A runner-local identity used to replace an older pending message with its
/// newest value.
///
/// Namespaces should describe the coalesced event, such as
/// `"chart.viewport"`. Use the instance component to distinguish repeated
/// feature instances.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MessageKey {
    namespace: &'static str,
    instance: u64,
}

impl MessageKey {
    /// Creates a key in a static namespace for one stable feature instance.
    pub const fn new(namespace: &'static str, instance: u64) -> Self {
        Self {
            namespace,
            instance,
        }
    }

    /// Creates a key for an application-wide singleton event.
    pub const fn singleton(namespace: &'static str) -> Self {
        Self::new(namespace, 0)
    }

    /// Returns the descriptive static namespace.
    pub const fn namespace(self) -> &'static str {
        self.namespace
    }

    /// Returns the stable feature-instance component.
    pub const fn instance(self) -> u64 {
        self.instance
    }
}

struct PostedQueue<M> {
    entries: VecDeque<QueuedMessage<M>>,
    keyed: HashMap<MessageKey, usize>,
}

impl<M> Default for PostedQueue<M> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            keyed: HashMap::new(),
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
            if let Some(existing) = &mut entry.trace {
                existing.update_latest(metadata, source, origin);
            }
            entry.message = message;
            entry.source = source;
            instrumentation.coalesced(&mut entry.trace);
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

/// A high-level RXUI application.
///
/// Implementations describe UI in [`build`](Self::build) and react to typed
/// messages in [`update`](Self::update); the runner owns the event loop,
/// window hosting, message routing, and redraw scheduling. Start the
/// application with [`run`] or [`run_with`].
pub trait App: Sized + 'static {
    /// Typed message emitted by UI listeners and dispatched to
    /// [`update`](Self::update).
    type Message: 'static;

    /// Builds the initial UI, typically opening at least one window with
    /// [`AppCx::open_window`].
    fn build(&mut self, cx: &mut AppCx<'_, Self::Message>) -> Result<()>;

    /// Applies one message to application state.
    fn update(&mut self, cx: &mut AppCx<'_, Self::Message>, message: Self::Message) -> Result<()>;

    /// Returns payload-free diagnostic identity for one message.
    ///
    /// This hook is called only while runtime instrumentation is enabled.
    fn message_metadata(_message: &Self::Message) -> MessageMetadata {
        MessageMetadata::unnamed()
    }

    /// Describes application-scoped long-lived event sources desired by state.
    ///
    /// The runner reconciles this set after each completed callback batch.
    fn subscriptions(&self) -> Subscriptions<Self::Message> {
        Subscriptions::none()
    }

    /// Observes a raw window event before the UI handles it.
    ///
    /// Use this for command routers, window-placement tracking, and other
    /// concerns below the widget layer.
    fn window_event(
        &mut self,
        _cx: &mut AppCx<'_, Self::Message>,
        _window: WindowId,
        _event: &WindowEvent,
    ) -> Result<()> {
        Ok(())
    }

    /// Decides whether a user-initiated close request proceeds.
    fn close_requested(
        &mut self,
        _cx: &mut AppCx<'_, Self::Message>,
        _window: WindowId,
    ) -> Result<CloseResponse> {
        Ok(CloseResponse::Close)
    }

    /// Called after a window closed from a granted close request.
    fn window_closed(
        &mut self,
        _cx: &mut AppCx<'_, Self::Message>,
        _window: WindowId,
    ) -> Result<()> {
        Ok(())
    }

    /// Renders one window in response to a platform redraw.
    ///
    /// The default presents the window's UI frame. Override to composite
    /// application scenes through [`AppCx::host`], for example with
    /// `WindowHost::redraw_composited`.
    fn render(&mut self, cx: &mut AppCx<'_, Self::Message>, window: WindowId) -> Result<()> {
        cx.present(window)
    }

    /// Runs one variable-rate update under a continuous
    /// [`RuntimePolicy`]; idle desktop applications never tick.
    fn tick(&mut self, _cx: &mut AppCx<'_, Self::Message>, _info: UpdateInfo) -> Result<()> {
        Ok(())
    }

    /// Called once while the event loop terminates.
    fn exiting(&mut self, _cx: &mut AppCx<'_, Self::Message>) -> Result<()> {
        Ok(())
    }
}

/// Response to a user-initiated window close request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CloseResponse {
    /// Close the window.
    #[default]
    Close,
    /// Keep the window open, for example while confirming unsaved changes.
    Ignore,
}

/// Stable identifier for a timer scheduled through [`AppCx::set_timeout`] or
/// [`AppCx::set_interval`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimerId(u64);

impl TimerId {
    /// Wraps a raw identifier.
    ///
    /// Intended for alternative [`AppBackend`] implementations such as test
    /// harnesses; runner-issued identifiers are unique per application run.
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the raw identifier value.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Stable identifier for one application-scoped background task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(u64);

impl TaskId {
    /// Wraps a raw identifier.
    ///
    /// Intended for alternative [`AppBackend`] implementations such as test
    /// harnesses; runner-issued identifiers are unique per application run.
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the raw identifier value.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Broad category of one active background task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TaskKind {
    /// Completion is owned by an external executor or callback API.
    External,
    /// Work is running through RXUI's bounded native blocking pool.
    Blocking,
}

/// Read-only metadata for one currently active task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveTaskSnapshot {
    id: TaskId,
    name: String,
    kind: TaskKind,
    elapsed: Duration,
}

impl ActiveTaskSnapshot {
    /// Creates task metadata for an alternative backend.
    #[doc(hidden)]
    pub fn new(id: TaskId, name: String, kind: TaskKind, elapsed: Duration) -> Self {
        Self {
            id,
            name,
            kind,
            elapsed,
        }
    }

    /// Returns the task's application-local identifier.
    pub const fn id(&self) -> TaskId {
        self.id
    }

    /// Returns the human-readable diagnostic name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns how the work is being executed.
    pub const fn kind(&self) -> TaskKind {
        self.kind
    }

    /// Returns time elapsed on the application's clock.
    pub const fn elapsed(&self) -> Duration {
        self.elapsed
    }
}

/// Point-in-time read-only state exposed to diagnostics and developer tools.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    active_tasks: Vec<ActiveTaskSnapshot>,
    active_subscriptions: Vec<ActiveSubscriptionSnapshot>,
    message_traces: Vec<MessageTrace>,
    pending_messages: usize,
    coalesced_replacements: u64,
}

impl RuntimeSnapshot {
    /// Creates a snapshot for an alternative backend.
    #[doc(hidden)]
    pub fn new(
        active_tasks: Vec<ActiveTaskSnapshot>,
        active_subscriptions: Vec<ActiveSubscriptionSnapshot>,
    ) -> Self {
        Self {
            active_tasks,
            active_subscriptions,
            message_traces: Vec::new(),
            pending_messages: 0,
            coalesced_replacements: 0,
        }
    }

    /// Creates a complete snapshot for an alternative backend.
    #[doc(hidden)]
    pub fn with_messages(
        active_tasks: Vec<ActiveTaskSnapshot>,
        active_subscriptions: Vec<ActiveSubscriptionSnapshot>,
        message_traces: Vec<MessageTrace>,
        pending_messages: usize,
        coalesced_replacements: u64,
    ) -> Self {
        Self {
            active_tasks,
            active_subscriptions,
            message_traces,
            pending_messages,
            coalesced_replacements,
        }
    }

    /// Returns active tasks in creation order.
    pub fn active_tasks(&self) -> &[ActiveTaskSnapshot] {
        &self.active_tasks
    }

    /// Returns active subscriptions ordered by identity.
    pub fn active_subscriptions(&self) -> &[ActiveSubscriptionSnapshot] {
        &self.active_subscriptions
    }

    /// Returns completed message traces from oldest to newest.
    pub fn message_traces(&self) -> &[MessageTrace] {
        &self.message_traces
    }

    /// Returns messages currently waiting in RXUI's posted-message queue.
    pub const fn pending_messages(&self) -> usize {
        self.pending_messages
    }

    /// Returns cumulative latest-value replacements during this run.
    pub const fn coalesced_replacements(&self) -> u64 {
        self.coalesced_replacements
    }
}

/// Outcome of submitting a result through [`TaskCompletion::complete`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskCompletionStatus {
    /// The result was accepted for event-loop delivery.
    Queued,
    /// The task had already been cancelled or abandoned.
    Cancelled,
}

/// Failure raised while executing a blocking task body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TaskError {
    /// The blocking closure panicked.
    ///
    /// Panic payloads are deliberately not exposed through application
    /// messages because they may contain sensitive data.
    Panicked,
}

impl fmt::Display for TaskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Panicked => formatter.write_str("the blocking task panicked"),
        }
    }
}

impl std::error::Error for TaskError {}

/// Failure to enqueue work on the native blocking-task pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TaskSpawnError {
    /// The configured bounded waiting queue is full.
    QueueFull,
    /// The application could not initialize its worker threads.
    WorkerUnavailable,
}

impl fmt::Display for TaskSpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueFull => formatter.write_str("the blocking-task queue is full"),
            Self::WorkerUnavailable => {
                formatter.write_str("the blocking-task worker pool is unavailable")
            }
        }
    }
}

impl std::error::Error for TaskSpawnError {}

/// Type-erased message factory transported from a worker to an app backend.
#[doc(hidden)]
pub type TaskMessageFactory<M> = Box<dyn FnOnce() -> M + Send + 'static>;
/// Type-erased task submission callback used by custom app backends.
#[doc(hidden)]
pub type TaskSubmit<M> = Box<
    dyn FnOnce(TaskMessageFactory<M>) -> std::result::Result<TaskCompletionStatus, ProxyClosed>
        + Send
        + 'static,
>;
/// Type-erased task abandonment callback used by custom app backends.
#[doc(hidden)]
pub type TaskAbandon = Box<dyn FnOnce() -> std::result::Result<(), ProxyClosed> + Send + 'static>;
type TaskFinish<T> = Box<
    dyn FnOnce(Option<T>) -> std::result::Result<TaskCompletionStatus, ProxyClosed>
        + Send
        + 'static,
>;

/// Backend completion channel used to implement [`AppCx::register_task`].
///
/// This is public only for custom [`AppBackend`] implementations and is not
/// part of RXUI's stable application-facing API.
#[doc(hidden)]
pub struct TaskSink<M: 'static> {
    id: TaskId,
    submit: Option<TaskSubmit<M>>,
    abandon: Option<TaskAbandon>,
}

impl<M: 'static> TaskSink<M> {
    /// Creates a backend task sink.
    #[doc(hidden)]
    pub fn new(id: TaskId, submit: TaskSubmit<M>, abandon: TaskAbandon) -> Self {
        Self {
            id,
            submit: Some(submit),
            abandon: Some(abandon),
        }
    }

    fn id(&self) -> TaskId {
        self.id
    }

    fn complete(
        mut self,
        factory: TaskMessageFactory<M>,
    ) -> std::result::Result<TaskCompletionStatus, ProxyClosed> {
        self.abandon = None;
        self.submit
            .take()
            .expect("a task sink is completed at most once")(factory)
    }

    fn abandon(mut self) -> std::result::Result<(), ProxyClosed> {
        self.submit = None;
        self.abandon
            .take()
            .expect("a task sink is abandoned at most once")()
    }
}

impl<M: 'static> Drop for TaskSink<M> {
    fn drop(&mut self) {
        if let Some(abandon) = self.abandon.take() {
            let _ = abandon();
        }
    }
}

impl<M: 'static> fmt::Debug for TaskSink<M> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TaskSink")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// One-shot bridge from an externally owned executor into the RXUI message
/// queue.
///
/// Dropping an incomplete value abandons the registered task. Completing a
/// cancelled task is harmless and reports [`TaskCompletionStatus::Cancelled`].
pub struct TaskCompletion<T> {
    id: TaskId,
    finish: Option<TaskFinish<T>>,
}

impl<T> TaskCompletion<T> {
    fn new(
        id: TaskId,
        finish: impl FnOnce(Option<T>) -> std::result::Result<TaskCompletionStatus, ProxyClosed>
        + Send
        + 'static,
    ) -> Self {
        Self {
            id,
            finish: Some(Box::new(finish)),
        }
    }

    /// Returns this task's application-local identifier.
    pub const fn id(&self) -> TaskId {
        self.id
    }

    /// Submits the task output for normal event-loop message dispatch.
    pub fn complete(mut self, output: T) -> std::result::Result<TaskCompletionStatus, ProxyClosed> {
        self.finish
            .take()
            .expect("a task completion is consumed at most once")(Some(output))
    }
}

impl<T> Drop for TaskCompletion<T> {
    fn drop(&mut self) {
        if let Some(finish) = self.finish.take() {
            let _ = finish(None);
        }
    }
}

impl<T> fmt::Debug for TaskCompletion<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TaskCompletion")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// The message proxy target application has shut down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProxyClosed;

impl fmt::Display for ProxyClosed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the application event loop has closed")
    }
}

impl std::error::Error for ProxyClosed {}

/// Cloneable, thread-safe handle that posts messages into [`App::update`].
///
/// Obtained from [`AppCx::proxy`]; messages sent from any thread are
/// dispatched on the event-loop thread with no source window.
pub struct MessageProxy<M> {
    post: Arc<dyn Fn(M) -> std::result::Result<(), ProxyClosed> + Send + Sync>,
}

impl<M> Clone for MessageProxy<M> {
    fn clone(&self) -> Self {
        Self {
            post: self.post.clone(),
        }
    }
}

impl<M> fmt::Debug for MessageProxy<M> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MessageProxy")
            .finish_non_exhaustive()
    }
}

impl<M: Send + 'static> MessageProxy<M> {
    /// Wraps a custom posting function.
    ///
    /// Intended for alternative [`AppBackend`] implementations such as test
    /// harnesses; applications obtain proxies from [`AppCx::proxy`].
    pub fn from_fn(
        post: impl Fn(M) -> std::result::Result<(), ProxyClosed> + Send + Sync + 'static,
    ) -> Self {
        Self {
            post: Arc::new(post),
        }
    }

    /// Posts a message for dispatch through [`App::update`].
    pub fn post(&self, message: M) -> std::result::Result<(), ProxyClosed> {
        (self.post)(message)
    }
}

/// Creation options for one application window.
///
/// Unset fields inherit [`WindowAttributes`]/[`WindowHostOptions`] defaults.
/// The [`attributes`](Self::attributes) and
/// [`host_options`](Self::host_options) escape hatches expose every
/// platform-window and renderer option; the dedicated builder fields override
/// values supplied through them.
#[derive(Clone, Debug, Default)]
pub struct WindowConfig {
    title: Option<String>,
    size: Option<(f64, f64)>,
    clear_color: Option<Color>,
    resizable: Option<bool>,
    attributes: Option<WindowAttributes>,
    host_options: Option<WindowHostOptions>,
}

impl WindowConfig {
    /// Creates options with a window title.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: Some(title.into()),
            ..Self::default()
        }
    }

    /// Sets the initial logical client size.
    pub fn size(mut self, width: f64, height: f64) -> Self {
        self.size = Some((width, height));
        self
    }

    /// Sets the color cleared behind the UI.
    pub fn clear_color(mut self, color: Color) -> Self {
        self.clear_color = Some(color);
        self
    }

    /// Sets whether the user may resize the window.
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = Some(resizable);
        self
    }

    /// Replaces the full platform window attributes.
    pub fn attributes(mut self, attributes: WindowAttributes) -> Self {
        self.attributes = Some(attributes);
        self
    }

    /// Replaces the full window-host options, including renderer options.
    pub fn host_options(mut self, options: WindowHostOptions) -> Self {
        self.host_options = Some(options);
        self
    }

    /// Resolves the layered options into concrete window-host options.
    pub fn into_host_options(self) -> WindowHostOptions {
        let mut options = self.host_options.unwrap_or_default();
        if let Some(attributes) = self.attributes {
            options.window = attributes;
        }
        if let Some(title) = self.title {
            options.window.title = title;
        }
        if let Some((width, height)) = self.size {
            options.window.inner_size = Some(astrelis_core::geometry::Size::new(width, height));
        }
        if let Some(resizable) = self.resizable {
            options.window.resizable = resizable;
        }
        if let Some(clear_color) = self.clear_color {
            options.clear_color = clear_color;
        }
        options
    }
}

/// Configuration for application-scoped background work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskConfig {
    blocking_workers: usize,
    blocking_queue_capacity: usize,
}

impl Default for TaskConfig {
    fn default() -> Self {
        let blocking_workers = thread_parallelism().clamp(1, 4);
        Self {
            blocking_workers,
            blocking_queue_capacity: 64,
        }
    }
}

impl TaskConfig {
    /// Sets the number of native blocking-worker threads.
    ///
    /// # Panics
    ///
    /// Panics when `workers` is zero.
    pub fn blocking_workers(mut self, workers: usize) -> Self {
        assert!(workers > 0, "blocking worker count must be non-zero");
        self.blocking_workers = workers;
        self
    }

    /// Sets the maximum number of blocking jobs waiting for a worker.
    ///
    /// # Panics
    ///
    /// Panics when `capacity` is zero.
    pub fn blocking_queue_capacity(mut self, capacity: usize) -> Self {
        assert!(
            capacity > 0,
            "blocking task queue capacity must be non-zero"
        );
        self.blocking_queue_capacity = capacity;
        self
    }
}

fn thread_parallelism() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        thread::available_parallelism().map_or(1, usize::from)
    }
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
}

/// Application-wide configuration for [`run_with`].
pub struct AppConfig {
    /// Scheduling configuration for the Astrelis runtime.
    pub runtime: RuntimeConfig,
    /// Font-discovery options used by every UI built with [`AppCx::new_ui`].
    ///
    /// [`FontDatabase`] is not cloneable, so the runner stores the options
    /// and builds a database per window.
    pub fonts: FontDatabaseOptions,
    /// Theme applied to every UI built with [`AppCx::new_ui`].
    pub theme: Theme,
    /// Pre-configured graphics; `None` creates the default wgpu instance.
    pub graphics: Option<GraphicsContext>,
    /// Terminates the application when its last window closes.
    ///
    /// Defaults to `true`.
    pub exit_on_last_window_close: bool,
    /// Background task and native blocking-pool configuration.
    pub tasks: TaskConfig,
    /// Optional payload-free runtime instrumentation.
    pub instrumentation: RuntimeInstrumentationConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            runtime: RuntimeConfig::default(),
            fonts: FontDatabaseOptions::default(),
            theme: Theme::default(),
            graphics: None,
            exit_on_last_window_close: true,
            tasks: TaskConfig::default(),
            instrumentation: RuntimeInstrumentationConfig::default(),
        }
    }
}

impl AppConfig {
    /// Sets the runtime scheduling configuration.
    pub fn runtime(mut self, runtime: RuntimeConfig) -> Self {
        self.runtime = runtime;
        self
    }

    /// Sets the initial scheduling policy.
    pub fn policy(mut self, policy: RuntimePolicy) -> Self {
        self.runtime.policy = policy;
        self
    }

    /// Sets the font-discovery options for new UI trees.
    pub fn fonts(mut self, fonts: FontDatabaseOptions) -> Self {
        self.fonts = fonts;
        self
    }

    /// Sets the theme for new UI trees.
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Sets background task and native blocking-pool configuration.
    pub fn tasks(mut self, tasks: TaskConfig) -> Self {
        self.tasks = tasks;
        self
    }

    /// Configures payload-free runtime instrumentation and bounded history.
    pub fn instrumentation(mut self, instrumentation: RuntimeInstrumentationConfig) -> Self {
        self.instrumentation = instrumentation;
        self
    }

    /// Supplies an application-configured graphics entry point.
    pub fn graphics(mut self, graphics: GraphicsContext) -> Self {
        self.graphics = Some(graphics);
        self
    }

    /// Sets whether closing the last window terminates the application.
    pub fn exit_on_last_window_close(mut self, exit: bool) -> Self {
        self.exit_on_last_window_close = exit;
        self
    }
}

/// Runner services behind [`AppCx`].
///
/// This trait is an advanced, semver-exempt extension point: it exists so
/// harnesses such as `rxui-testing` can drive an [`App`] against a headless
/// backend. Applications should use [`AppCx`] and never interact with the
/// trait directly.
pub trait AppBackend<M: 'static> {
    /// Builds an empty UI from the application fonts and theme.
    fn new_ui(&mut self) -> Ui<M>;

    /// Opens a window hosting a UI and returns its identifier.
    fn open_window(&mut self, config: WindowConfig, ui: Ui<M>) -> Result<WindowId>;

    /// Closes a window and releases its host.
    fn close_window(&mut self, window: WindowId) -> Result<()>;

    /// Returns every open window in creation order.
    fn windows(&self) -> Vec<WindowId>;

    /// Returns a window's UI tree.
    fn ui_mut(&mut self, window: WindowId) -> Result<&mut Ui<M>>;

    /// Returns a window's platform handle.
    fn window(&self, window: WindowId) -> Result<&Window>;

    /// Returns a window's host for GPU-level access.
    fn host_mut(&mut self, window: WindowId) -> Result<&mut WindowHost<M>>;

    /// Generates and presents one UI frame for a window.
    fn present(&mut self, window: WindowId) -> Result<()>;

    /// Marks one window as needing redraw.
    fn invalidate(&mut self, window: WindowId);

    /// Marks every window as needing redraw.
    fn invalidate_all(&mut self);

    /// Queues a message for dispatch after the current update.
    fn post(&mut self, message: M, source: Option<WindowId>);

    /// Queues or replaces one pending latest-value message.
    fn post_latest(&mut self, key: MessageKey, message: M, source: Option<WindowId>);

    /// Takes every queued posted message and its source window.
    fn take_posted(&mut self) -> Vec<QueuedMessage<M>>;

    /// Wraps one direct message with optional tracing metadata.
    #[doc(hidden)]
    fn instrument_message(
        &mut self,
        message: M,
        source: Option<WindowId>,
        origin: MessageOrigin,
    ) -> QueuedMessage<M>;

    /// Records the beginning of one application update.
    #[doc(hidden)]
    fn message_dispatch_started(&mut self, dispatch: &mut MessageDispatch);

    /// Records the completion of one application update.
    #[doc(hidden)]
    fn message_dispatch_finished(&mut self, dispatch: MessageDispatch, outcome: MessageOutcome);

    /// Returns whether any posted messages remain queued.
    fn has_posted(&self) -> bool;

    /// Returns a thread-safe message posting handle.
    fn proxy(&self) -> MessageProxy<M>
    where
        M: Send;

    /// Registers an application-scoped task completion channel.
    fn register_task(&mut self, name: String) -> TaskSink<M>;

    /// Queues one closure on the native bounded blocking pool.
    #[cfg(not(target_arch = "wasm32"))]
    fn enqueue_blocking(
        &mut self,
        task: TaskId,
        job: Box<dyn FnOnce() + Send + 'static>,
    ) -> std::result::Result<(), TaskSpawnError>;

    /// Cancels a task, returning whether it was still active.
    fn cancel_task(&mut self, task: TaskId) -> bool;

    /// Returns a read-only snapshot of runner-owned runtime state.
    fn runtime_snapshot(&self) -> RuntimeSnapshot;

    /// Reconciles one complete desired subscription set.
    #[doc(hidden)]
    fn reconcile_subscriptions(&mut self, desired: Subscriptions<M>) -> Result<()>;

    /// Cancels every active subscription during orderly shutdown.
    #[doc(hidden)]
    fn cancel_all_subscriptions(&mut self);

    /// Schedules a one-shot message factory after a delay.
    fn set_timeout(&mut self, delay: Duration, factory: Box<dyn FnOnce() -> M>) -> TimerId;

    /// Schedules messages produced by a factory at an interval.
    fn set_interval(&mut self, interval: Duration, factory: Box<dyn FnMut() -> M>) -> TimerId;

    /// Cancels a timer, returning whether it was still scheduled.
    fn cancel_timer(&mut self, timer: TimerId) -> bool;

    /// Changes the runtime scheduling policy.
    fn set_policy(&mut self, policy: RuntimePolicy);

    /// Returns the platform clipboard.
    fn clipboard(&self) -> Clipboard;

    /// Returns the current time on the backend's clock.
    ///
    /// The runtime backend reads the platform clock; test backends may use a
    /// virtual clock so time-dependent behavior stays deterministic.
    fn now(&self) -> Instant;

    /// Returns all currently available monitors.
    fn available_monitors(&self) -> Vec<Monitor>;

    /// Returns the platform's primary monitor when known.
    fn primary_monitor(&self) -> Option<Monitor>;

    /// Requests orderly application termination.
    fn exit(&mut self);
}

/// Operations available to [`App`] callbacks.
///
/// A context borrows the runner for one callback and optionally records the
/// window whose event produced the current message batch, which
/// [`source_ui`](Self::source_ui) and [`source_window`](Self::source_window)
/// expose.
pub struct AppCx<'a, M: 'static> {
    backend: &'a mut dyn AppBackend<M>,
    source: Option<WindowId>,
}

impl<'a, M: 'static> AppCx<'a, M> {
    /// Wraps a backend for one callback.
    ///
    /// Intended for alternative [`AppBackend`] implementations such as test
    /// harnesses; the runner constructs contexts for application callbacks.
    pub fn new(backend: &'a mut dyn AppBackend<M>, source: Option<WindowId>) -> Self {
        Self { backend, source }
    }

    pub(crate) fn reborrow(&mut self) -> AppCx<'_, M> {
        AppCx {
            backend: &mut *self.backend,
            source: self.source,
        }
    }

    /// Builds an empty UI from the application fonts and theme.
    pub fn new_ui(&mut self) -> Ui<M> {
        self.backend.new_ui()
    }

    /// Opens a window hosting a UI and returns its identifier.
    pub fn open_window(&mut self, config: WindowConfig, ui: Ui<M>) -> Result<WindowId> {
        self.backend.open_window(config, ui)
    }

    /// Closes a window programmatically.
    ///
    /// [`App::window_closed`] is not invoked for programmatic closes; the
    /// exit-on-last-window policy still applies once the current callback
    /// completes.
    pub fn close_window(&mut self, window: WindowId) -> Result<()> {
        self.backend.close_window(window)
    }

    /// Returns every open window in creation order.
    pub fn windows(&self) -> Vec<WindowId> {
        self.backend.windows()
    }

    /// Returns a window's UI tree.
    pub fn ui(&mut self, window: WindowId) -> Result<&mut Ui<M>> {
        self.backend.ui_mut(window)
    }

    /// Returns the UI tree the current message originated from.
    ///
    /// Falls back to the sole open window when no source window is recorded
    /// (or the source window has closed); errors when the choice would be
    /// ambiguous or no window is open.
    pub fn source_ui(&mut self) -> Result<&mut Ui<M>> {
        let windows = self.backend.windows();
        let source = self.source.filter(|window| windows.contains(window));
        let window = match (source, windows.as_slice()) {
            (Some(window), _) => window,
            (None, [only]) => *only,
            (None, []) => return Err(Error::msg("no window is open")),
            (None, _) => {
                return Err(Error::msg(
                    "the source window is ambiguous; use `ui(window)` with an explicit window",
                ));
            }
        };
        self.backend.ui_mut(window)
    }

    /// Returns the window the current message originated from, when known.
    pub fn source_window(&self) -> Option<WindowId> {
        self.source
    }

    /// Returns a window's platform handle, for example for native-menu
    /// installation or placement tracking.
    pub fn window(&self, window: WindowId) -> Result<&Window> {
        self.backend.window(window)
    }

    /// Returns a window's host for GPU-level access such as
    /// `WindowHost::redraw_composited` or external-image registration.
    pub fn host(&mut self, window: WindowId) -> Result<&mut WindowHost<M>> {
        self.backend.host_mut(window)
    }

    /// Generates and presents one UI frame for a window.
    pub fn present(&mut self, window: WindowId) -> Result<()> {
        self.backend.present(window)
    }

    /// Marks one window as needing redraw.
    pub fn invalidate(&mut self, window: WindowId) {
        self.backend.invalidate(window);
    }

    /// Marks every window as needing redraw.
    pub fn invalidate_all(&mut self) {
        self.backend.invalidate_all();
    }

    /// Queues a message dispatched through [`App::update`] after the current
    /// callback completes.
    pub fn post(&mut self, message: M) {
        self.backend.post(message, self.source);
    }

    /// Queues a latest-value message after the current callback completes.
    ///
    /// If a pending message has the same key, its payload and source window
    /// are replaced without changing its queue position. Use ordinary
    /// [`post`](Self::post) when every delivery is meaningful.
    pub fn post_latest(&mut self, key: MessageKey, message: M) {
        self.backend.post_latest(key, message, self.source);
    }

    /// Returns a cloneable handle that posts messages from any thread.
    pub fn proxy(&self) -> MessageProxy<M>
    where
        M: Send,
    {
        self.backend.proxy()
    }

    /// Registers a task completed by an externally owned executor.
    ///
    /// The output and mapping closure cross the runtime wakeup bridge, but
    /// the mapper runs on the application event-loop thread. Consequently the
    /// application message type itself does not need to implement [`Send`].
    /// Dropping the returned completion without calling
    /// [`complete`](TaskCompletion::complete) abandons the task.
    pub fn register_task<T: Send + 'static>(
        &mut self,
        map: impl FnOnce(T) -> M + Send + 'static,
    ) -> TaskCompletion<T> {
        self.register_task_named("External task", map)
    }

    /// Registers a diagnostically named task completed by an external executor.
    pub fn register_task_named<T: Send + 'static>(
        &mut self,
        name: impl Into<String>,
        map: impl FnOnce(T) -> M + Send + 'static,
    ) -> TaskCompletion<T> {
        let sink = self.backend.register_task(name.into());
        let id = sink.id();
        TaskCompletion::new(id, move |output| match output {
            Some(output) => sink.complete(Box::new(move || map(output))),
            None => {
                sink.abandon()?;
                Ok(TaskCompletionStatus::Cancelled)
            }
        })
    }

    /// Runs finite blocking work on the application's bounded native worker
    /// pool and maps its result into a message on the event-loop thread.
    ///
    /// A panic in `work` becomes [`TaskError::Panicked`]. Cancelling the
    /// returned [`TaskId`] cannot forcibly interrupt work that has already
    /// started, but always suppresses its eventual message.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn spawn_blocking<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
        map: impl FnOnce(std::result::Result<T, TaskError>) -> M + Send + 'static,
    ) -> std::result::Result<TaskId, TaskSpawnError> {
        self.spawn_blocking_named("Blocking task", work, map)
    }

    /// Runs diagnostically named finite work on the bounded native worker pool.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn spawn_blocking_named<T: Send + 'static>(
        &mut self,
        name: impl Into<String>,
        work: impl FnOnce() -> T + Send + 'static,
        map: impl FnOnce(std::result::Result<T, TaskError>) -> M + Send + 'static,
    ) -> std::result::Result<TaskId, TaskSpawnError> {
        let completion = self.register_task_named(name, map);
        let task = completion.id();
        let job = Box::new(move || {
            let result = catch_unwind(AssertUnwindSafe(work)).map_err(|_| TaskError::Panicked);
            let _ = completion.complete(result);
        });
        self.backend.enqueue_blocking(task, job)?;
        Ok(task)
    }

    /// Cancels a task, returning whether it was still active.
    pub fn cancel_task(&mut self, task: TaskId) -> bool {
        self.backend.cancel_task(task)
    }

    /// Returns a read-only snapshot of runner-owned runtime state.
    pub fn runtime_snapshot(&self) -> RuntimeSnapshot {
        self.backend.runtime_snapshot()
    }

    /// Schedules a message delivered once after a delay.
    pub fn set_timeout(&mut self, delay: Duration, message: M) -> TimerId {
        self.set_timeout_with(delay, move || message)
    }

    /// Schedules a message produced once after a delay.
    ///
    /// The factory runs on the application event-loop thread when the timeout
    /// fires. Cancelling the timer drops the factory without invoking it.
    pub fn set_timeout_with(
        &mut self,
        delay: Duration,
        factory: impl FnOnce() -> M + 'static,
    ) -> TimerId {
        self.backend.set_timeout(delay, Box::new(factory))
    }

    /// Schedules a message delivered repeatedly at an interval.
    ///
    /// Missed intervals are coalesced into one delivery per event-loop turn.
    pub fn set_interval(&mut self, interval: Duration, message: M) -> TimerId
    where
        M: Clone,
    {
        self.set_interval_with(interval, move || message.clone())
    }

    /// Schedules messages produced by a factory repeatedly at an interval.
    ///
    /// The factory runs on the application event-loop thread. Use this form
    /// when each delivery differs or the message type is not [`Clone`]. Missed
    /// intervals are coalesced into one delivery per event-loop turn.
    pub fn set_interval_with(
        &mut self,
        interval: Duration,
        factory: impl FnMut() -> M + 'static,
    ) -> TimerId {
        self.backend.set_interval(interval, Box::new(factory))
    }

    /// Cancels a timer, returning whether it was still scheduled.
    pub fn cancel_timer(&mut self, timer: TimerId) -> bool {
        self.backend.cancel_timer(timer)
    }

    /// Changes the runtime scheduling policy, for example to run
    /// [`App::tick`] continuously.
    pub fn set_policy(&mut self, policy: RuntimePolicy) {
        self.backend.set_policy(policy);
    }

    /// Returns the platform clipboard.
    pub fn clipboard(&self) -> Clipboard {
        self.backend.clipboard()
    }

    /// Returns the current time on the application clock.
    ///
    /// Prefer this over `Instant::now()` in application code: it compiles on
    /// the web (where `std::time::Instant` panics) and stays deterministic
    /// under test harnesses that virtualize time.
    pub fn now(&self) -> Instant {
        self.backend.now()
    }

    /// Returns all currently available monitors.
    pub fn available_monitors(&self) -> Vec<Monitor> {
        self.backend.available_monitors()
    }

    /// Returns the platform's primary monitor when known.
    pub fn primary_monitor(&self) -> Option<Monitor> {
        self.backend.primary_monitor()
    }

    /// Requests orderly application termination.
    pub fn exit(&mut self) {
        self.backend.exit();
    }
}

impl<M: 'static> fmt::Debug for AppCx<'_, M> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppCx")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

/// Result type returned by [`run`], [`run_with`], and `spawn_on_canvas`.
pub type MainResult = std::result::Result<(), RunError>;

/// Terminal failure of an application run.
pub enum RunError {
    /// The platform event loop failed.
    Platform(PlatformError),
    /// An application callback failed.
    App(Error),
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform(error) => write!(formatter, "platform error: {error}"),
            Self::App(error) => write!(formatter, "application error: {error}"),
        }
    }
}

impl fmt::Debug for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform(error) => write!(formatter, "Platform({error:?})"),
            Self::App(error) => write!(formatter, "App({error:?})"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Platform(error) => Some(error),
            // `crate::Error` intentionally does not implement
            // `std::error::Error`; its message is part of `Display`.
            Self::App(_) => None,
        }
    }
}

/// Runs an application with default configuration until it exits.
#[cfg(not(target_arch = "wasm32"))]
pub fn run<A: App>(app: A) -> MainResult {
    run_with(app, AppConfig::default())
}

/// Runs an application with explicit configuration until it exits.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_with<A: App>(app: A, config: AppConfig) -> MainResult {
    let runtime_config = config.runtime;
    let core = RunnerCore::new(app, config);
    match Runtime::finish(astrelis_platform_winit::run_return(Runtime::new(
        core,
        runtime_config,
    ))) {
        Ok(_) => Ok(()),
        Err(RuntimeError::Platform(error)) => Err(RunError::Platform(error)),
        Err(RuntimeError::Application(DynAppError(error))) => Err(RunError::App(error)),
    }
}

/// Starts an application on an existing page canvas.
///
/// This returns after scheduling the browser event loop; application
/// callback errors terminate the loop asynchronously and cannot be observed
/// through the returned result.
#[cfg(target_arch = "wasm32")]
pub fn spawn_on_canvas<A: App>(
    app: A,
    config: AppConfig,
    canvas: web_sys::HtmlCanvasElement,
) -> MainResult {
    let runtime_config = config.runtime;
    let core = RunnerCore::new(app, config);
    astrelis_platform_winit::web::spawn_on_canvas(Runtime::new(core, runtime_config), canvas)
        .map_err(RunError::Platform)
}

struct TaskRecord {
    state: Arc<AtomicU8>,
    name: String,
    kind: TaskKind,
    started_at: Instant,
}

type ServiceDecoder<M> = Rc<RefCell<Box<dyn FnMut(RawSubscriptionEvent) -> M>>>;

enum ActiveSubscriptionSource<M: 'static> {
    Interval {
        timer: NativeTimerId,
        factory: Rc<RefCell<Box<dyn FnMut() -> M>>>,
    },
    Service {
        _guard: Option<Box<dyn Any + Send>>,
        decode: ServiceDecoder<M>,
        status: SubscriptionStatus,
    },
}

struct ActiveSubscription<M: 'static> {
    config: SubscriptionConfig,
    generation: u64,
    source: ActiveSubscriptionSource<M>,
    started_at: Instant,
    starts: u64,
}

#[cfg(not(target_arch = "wasm32"))]
type BlockingJob = Box<dyn FnOnce() + Send + 'static>;

#[cfg(not(target_arch = "wasm32"))]
struct BlockingPool {
    sender: Option<SyncSender<BlockingJob>>,
    workers: Vec<thread::JoinHandle<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl BlockingPool {
    fn new(config: TaskConfig) -> std::result::Result<Self, TaskSpawnError> {
        let (sender, receiver) = mpsc::sync_channel(config.blocking_queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers = Vec::with_capacity(config.blocking_workers);

        for index in 0..config.blocking_workers {
            let receiver = Arc::clone(&receiver);
            let worker = thread::Builder::new()
                .name(format!("rxui-blocking-{index}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let Ok(receiver) = receiver.lock() else {
                                return;
                            };
                            receiver.recv()
                        };
                        let Ok(job) = job else {
                            return;
                        };
                        let _ = catch_unwind(AssertUnwindSafe(job));
                    }
                })
                .map_err(|_| TaskSpawnError::WorkerUnavailable)?;
            workers.push(worker);
        }

        Ok(Self {
            sender: Some(sender),
            workers,
        })
    }

    fn enqueue(&self, job: BlockingJob) -> std::result::Result<(), TaskSpawnError> {
        let Some(sender) = self.sender.as_ref() else {
            return Err(TaskSpawnError::WorkerUnavailable);
        };
        sender.try_send(job).map_err(|error| match error {
            TrySendError::Full(_) => TaskSpawnError::QueueFull,
            TrySendError::Disconnected(_) => TaskSpawnError::WorkerUnavailable,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for BlockingPool {
    fn drop(&mut self) {
        self.sender.take();
        // Never block the UI thread waiting for in-flight application work.
        // Dropping a JoinHandle detaches it; closed-channel workers exit after
        // any already accepted jobs observe cancellation.
        self.workers.clear();
    }
}

/// Runner-owned state shared by every backend call.
struct Shell<M: 'static> {
    graphics: GraphicsContext,
    fonts: FontDatabaseOptions,
    theme: Theme,
    exit_on_last_window_close: bool,
    hosts: Vec<(WindowId, WindowHost<M>)>,
    posted: PostedQueue<M>,
    instrumentation: InstrumentationState,
    timers: HashMap<u64, NativeTimerId>,
    next_timer: u64,
    tasks: HashMap<TaskId, TaskRecord>,
    next_task: u64,
    subscriptions: HashMap<SubscriptionId, ActiveSubscription<M>>,
    next_subscription_generation: u64,
    exit_requested: bool,
    #[cfg(not(target_arch = "wasm32"))]
    task_config: TaskConfig,
    #[cfg(not(target_arch = "wasm32"))]
    blocking_pool: Option<BlockingPool>,
    built: bool,
}

impl<M: 'static> Shell<M> {
    fn new(config: AppConfig) -> Self {
        let instrumentation = InstrumentationState::new(config.instrumentation);
        Self {
            graphics: config.graphics.unwrap_or_default(),
            fonts: config.fonts,
            theme: config.theme,
            exit_on_last_window_close: config.exit_on_last_window_close,
            hosts: Vec::new(),
            posted: PostedQueue::default(),
            instrumentation,
            timers: HashMap::new(),
            next_timer: 1,
            tasks: HashMap::new(),
            next_task: 1,
            subscriptions: HashMap::new(),
            next_subscription_generation: 1,
            exit_requested: false,
            #[cfg(not(target_arch = "wasm32"))]
            task_config: config.tasks,
            #[cfg(not(target_arch = "wasm32"))]
            blocking_pool: None,
            built: false,
        }
    }

    fn host_mut(&mut self, window: WindowId) -> Option<&mut WindowHost<M>> {
        self.hosts
            .iter_mut()
            .find(|(id, _)| *id == window)
            .map(|(_, host)| host)
    }

    fn alloc_timer(&mut self) -> TimerId {
        let id = TimerId(self.next_timer);
        self.next_timer += 1;
        id
    }

    fn register_task(&mut self, name: String, started_at: Instant) -> (TaskId, Arc<AtomicU8>) {
        let id = TaskId(self.next_task);
        self.next_task += 1;
        let state = Arc::new(AtomicU8::new(TASK_PENDING));
        self.tasks.insert(
            id,
            TaskRecord {
                state: Arc::clone(&state),
                name,
                kind: TaskKind::External,
                started_at,
            },
        );
        (id, state)
    }

    fn cancel_task(&mut self, task: TaskId) -> bool {
        let Some(record) = self.tasks.remove(&task) else {
            return false;
        };
        record.state.store(TASK_CANCELLED, Ordering::Release);
        true
    }

    fn abandon_task(&mut self, task: TaskId, state: &Arc<AtomicU8>) {
        let matches = self
            .tasks
            .get(&task)
            .is_some_and(|record| Arc::ptr_eq(&record.state, state));
        if matches {
            self.tasks.remove(&task);
        }
    }

    fn cancel_all_tasks(&mut self) {
        for record in self.tasks.values() {
            record.state.store(TASK_CANCELLED, Ordering::Release);
        }
        self.tasks.clear();
        #[cfg(not(target_arch = "wasm32"))]
        self.blocking_pool.take();
    }

    fn runtime_snapshot(&self, now: Instant) -> RuntimeSnapshot {
        let mut active_tasks = self
            .tasks
            .iter()
            .map(|(&id, task)| ActiveTaskSnapshot {
                id,
                name: task.name.clone(),
                kind: task.kind,
                elapsed: now.saturating_duration_since(task.started_at),
            })
            .collect::<Vec<_>>();
        active_tasks.sort_by_key(|task| task.id);
        let mut active_subscriptions = self
            .subscriptions
            .iter()
            .map(|(&id, subscription)| {
                ActiveSubscriptionSnapshot::new(
                    id,
                    subscription.config.kind(),
                    subscription.config.delivery(),
                    subscription.config.interval(),
                    now.saturating_duration_since(subscription.started_at),
                    subscription.starts,
                    match &subscription.source {
                        ActiveSubscriptionSource::Interval { .. } => SubscriptionStatus::Running,
                        ActiveSubscriptionSource::Service { status, .. } => *status,
                    },
                )
            })
            .collect::<Vec<_>>();
        active_subscriptions.sort_by_key(ActiveSubscriptionSnapshot::id);
        let (message_traces, coalesced_replacements) = self.instrumentation.snapshot();
        RuntimeSnapshot::with_messages(
            active_tasks,
            active_subscriptions,
            message_traces,
            self.posted.entries.len(),
            coalesced_replacements,
        )
    }
}

impl<M: 'static> Drop for Shell<M> {
    fn drop(&mut self) {
        self.cancel_all_tasks();
    }
}

/// Adapter that implements `astrelis_app::App` over a user [`App`].
struct RunnerCore<A: App> {
    user: A,
    shell: Shell<A::Message>,
}

impl<A: App> RunnerCore<A> {
    fn new(user: A, config: AppConfig) -> Self {
        Self {
            user,
            shell: Shell::new(config),
        }
    }

    /// Dispatches a message arriving from outside a window event, such as a
    /// timer or [`MessageProxy`].
    fn dispatch_queued(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        message: A::Message,
        origin: MessageOrigin,
    ) -> std::result::Result<(), DynAppError> {
        if !self.shell.built {
            let mut backend = RuntimeBackend {
                context,
                shell: &mut self.shell,
            };
            let message = backend.instrument_message(message, None, origin);
            backend.shell.posted.post(message);
            return Ok(());
        }
        let exit_on_last = self.shell.exit_on_last_window_close;
        let Self { user, shell } = self;
        let mut backend = RuntimeBackend { context, shell };
        dispatch_external(user, &mut backend, message, origin, exit_on_last).map_err(DynAppError)
    }

    fn complete_task(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        task: TaskId,
        state: Arc<AtomicU8>,
        factory: TaskMessageFactory<A::Message>,
    ) -> std::result::Result<(), DynAppError> {
        let active = self
            .shell
            .tasks
            .get(&task)
            .is_some_and(|record| Arc::ptr_eq(&record.state, &state));
        if !active || state.load(Ordering::Acquire) != TASK_COMPLETION_QUEUED {
            return Ok(());
        }
        self.shell.tasks.remove(&task);
        state.store(TASK_FINISHED, Ordering::Release);
        self.dispatch_queued(context, factory(), MessageOrigin::Task(task))
    }

    fn abandon_task(&mut self, task: TaskId, state: Arc<AtomicU8>) {
        self.shell.abandon_task(task, &state);
    }

    fn fire_subscription(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        id: SubscriptionId,
        generation: u64,
    ) -> std::result::Result<(), DynAppError> {
        let message = {
            let Some(subscription) = self.shell.subscriptions.get(&id) else {
                return Ok(());
            };
            if subscription.generation != generation {
                return Ok(());
            }
            let ActiveSubscriptionSource::Interval { factory, .. } = &subscription.source else {
                return Ok(());
            };
            (factory.borrow_mut())()
        };
        // Astrelis intervals already collapse missed periods into one callback
        // per event-loop turn. `DeliveryPolicy::Every` preserves every callback
        // that does occur; future concurrent sources will use the same policy
        // at their producer queue.
        self.dispatch_queued(context, message, MessageOrigin::Subscription(id))
    }

    fn fire_service_subscription(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        id: SubscriptionId,
        generation: u64,
        pending: Arc<Mutex<Option<RawSubscriptionEvent>>>,
    ) -> std::result::Result<(), DynAppError> {
        let event = pending
            .lock()
            .expect("subscription event slot poisoned")
            .take();
        let Some(event) = event else {
            return Ok(());
        };
        let message = {
            let Some(subscription) = self.shell.subscriptions.get(&id) else {
                return Ok(());
            };
            if subscription.generation != generation {
                return Ok(());
            }
            let ActiveSubscriptionSource::Service { decode, .. } = &subscription.source else {
                return Ok(());
            };
            (decode.borrow_mut())(event)
        };
        self.dispatch_queued(context, message, MessageOrigin::Subscription(id))
    }
}

impl<A: App> astrelis_app::App for RunnerCore<A> {
    type Error = DynAppError;

    fn resumed(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
    ) -> std::result::Result<(), Self::Error> {
        if self.shell.built {
            return Ok(());
        }
        self.shell.built = true;
        let Self { user, shell } = self;
        let mut backend = RuntimeBackend { context, shell };
        user.build(&mut AppCx::new(&mut backend, None))
            .map_err(DynAppError)?;
        flush_posted(user, &mut backend).map_err(DynAppError)?;
        reconcile_subscriptions(user, &mut backend).map_err(DynAppError)?;
        invalidate_dirty(&mut backend);
        Ok(())
    }

    fn window_event(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        window: WindowId,
        event: WindowEvent,
    ) -> std::result::Result<(), Self::Error> {
        let clipboard = context.clipboard();
        let exit_on_last = self.shell.exit_on_last_window_close;
        let Self { user, shell } = self;
        let mut backend = RuntimeBackend { context, shell };
        user.window_event(&mut AppCx::new(&mut backend, Some(window)), window, &event)
            .map_err(DynAppError)?;
        let Some(host) = backend.shell.host_mut(window) else {
            flush_posted(user, &mut backend).map_err(DynAppError)?;
            reconcile_subscriptions(user, &mut backend).map_err(DynAppError)?;
            invalidate_dirty(&mut backend);
            return Ok(());
        };
        let update = host
            .handle_event(&clipboard, &event)
            .map_err(|error| DynAppError(error.into()))?;
        let messages: Vec<_> = host.drain_messages().collect();
        process_host_update(user, &mut backend, window, update, messages, exit_on_last)
            .map_err(DynAppError)
    }

    fn update(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        info: UpdateInfo,
    ) -> std::result::Result<(), Self::Error> {
        if !self.shell.built {
            return Ok(());
        }
        let exit_on_last = self.shell.exit_on_last_window_close;
        let Self { user, shell } = self;
        let mut backend = RuntimeBackend { context, shell };
        let had_windows = !backend.windows().is_empty();
        user.tick(&mut AppCx::new(&mut backend, None), info)
            .map_err(DynAppError)?;
        flush_posted(user, &mut backend).map_err(DynAppError)?;
        reconcile_subscriptions(user, &mut backend).map_err(DynAppError)?;
        invalidate_dirty(&mut backend);
        if exit_on_last && had_windows && backend.windows().is_empty() {
            backend.exit();
        }
        Ok(())
    }

    fn redraw(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        window: WindowId,
    ) -> std::result::Result<(), Self::Error> {
        if self.shell.host_mut(window).is_none() {
            return Ok(());
        }
        let Self { user, shell } = self;
        let mut backend = RuntimeBackend { context, shell };
        user.render(&mut AppCx::new(&mut backend, Some(window)), window)
            .map_err(DynAppError)?;
        let posted = flush_posted(user, &mut backend).map_err(DynAppError)?;
        reconcile_subscriptions(user, &mut backend).map_err(DynAppError)?;
        if posted {
            invalidate_dirty(&mut backend);
        }
        Ok(())
    }

    fn exiting(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
    ) -> std::result::Result<(), Self::Error> {
        let Self { user, shell } = self;
        shell.cancel_all_tasks();
        let mut backend = RuntimeBackend { context, shell };
        backend.cancel_all_subscriptions();
        user.exiting(&mut AppCx::new(&mut backend, None))
            .map_err(DynAppError)
    }
}

/// Production [`AppBackend`] bridging the Astrelis runtime and window hosts.
struct RuntimeBackend<'a, 'ctx, 'platform, A: App> {
    context: &'a mut AppContext<'ctx, 'platform, RunnerCore<A>>,
    shell: &'a mut Shell<A::Message>,
}

impl<A: App> AppBackend<A::Message> for RuntimeBackend<'_, '_, '_, A> {
    fn new_ui(&mut self) -> Ui<A::Message> {
        Ui::new(
            FontDatabase::new(self.shell.fonts),
            self.shell.theme.clone(),
        )
    }

    fn open_window(&mut self, config: WindowConfig, ui: Ui<A::Message>) -> Result<WindowId> {
        let host = WindowHost::open(
            self.context,
            &self.shell.graphics,
            ui,
            config.into_host_options(),
        )?;
        let window = host.id();
        self.shell.hosts.push((window, host));
        Ok(window)
    }

    fn close_window(&mut self, window: WindowId) -> Result<()> {
        let Some(index) = self.shell.hosts.iter().position(|(id, _)| *id == window) else {
            return Err(Error::msg(format!("window {window:?} is not open")));
        };
        self.context.unregister_window(window);
        drop(self.shell.hosts.remove(index));
        Ok(())
    }

    fn windows(&self) -> Vec<WindowId> {
        self.shell.hosts.iter().map(|(id, _)| *id).collect()
    }

    fn ui_mut(&mut self, window: WindowId) -> Result<&mut Ui<A::Message>> {
        self.shell
            .host_mut(window)
            .map(WindowHost::ui_mut)
            .ok_or_else(|| Error::msg(format!("window {window:?} is not open")))
    }

    fn window(&self, window: WindowId) -> Result<&Window> {
        self.shell
            .hosts
            .iter()
            .find(|(id, _)| *id == window)
            .map(|(_, host)| host.window())
            .ok_or_else(|| Error::msg(format!("window {window:?} is not open")))
    }

    fn host_mut(&mut self, window: WindowId) -> Result<&mut WindowHost<A::Message>> {
        self.shell
            .host_mut(window)
            .ok_or_else(|| Error::msg(format!("window {window:?} is not open")))
    }

    fn present(&mut self, window: WindowId) -> Result<()> {
        let host = self
            .shell
            .host_mut(window)
            .ok_or_else(|| Error::msg(format!("window {window:?} is not open")))?;
        host.redraw()?;
        Ok(())
    }

    fn invalidate(&mut self, window: WindowId) {
        self.context.invalidate_window(window);
    }

    fn invalidate_all(&mut self) {
        self.context.invalidate_all();
    }

    fn post(&mut self, message: A::Message, source: Option<WindowId>) {
        let message = self.instrument_message(message, source, MessageOrigin::Posted);
        self.shell.posted.post(message);
    }

    fn post_latest(&mut self, key: MessageKey, message: A::Message, source: Option<WindowId>) {
        let metadata = if self.shell.instrumentation.enabled() {
            A::message_metadata(&message)
        } else {
            MessageMetadata::unnamed()
        };
        let origin = MessageOrigin::Posted;
        let message = match self.shell.posted.replace_latest(
            key,
            message,
            source,
            metadata,
            origin,
            &mut self.shell.instrumentation,
        ) {
            Ok(()) => return,
            Err(message) => message,
        };
        let queue_depth = self.shell.posted.entries.len().saturating_add(1);
        let trace = self.shell.instrumentation.queued(
            metadata,
            source,
            origin,
            self.context.now(),
            queue_depth,
            Some(key),
        );
        self.shell.posted.post_keyed(
            key,
            QueuedMessage {
                message,
                source,
                trace,
            },
        );
    }

    fn take_posted(&mut self) -> Vec<QueuedMessage<A::Message>> {
        self.shell.posted.take()
    }

    fn instrument_message(
        &mut self,
        message: A::Message,
        source: Option<WindowId>,
        origin: MessageOrigin,
    ) -> QueuedMessage<A::Message> {
        let trace = if self.shell.instrumentation.enabled() {
            let metadata = A::message_metadata(&message);
            let queue_depth = self.shell.posted.entries.len().saturating_add(1);
            self.shell.instrumentation.queued(
                metadata,
                source,
                origin,
                self.context.now(),
                queue_depth,
                None,
            )
        } else {
            None
        };
        QueuedMessage {
            message,
            source,
            trace,
        }
    }

    fn message_dispatch_started(&mut self, dispatch: &mut MessageDispatch) {
        self.shell
            .instrumentation
            .started(&mut dispatch.trace, self.context.now());
    }

    fn message_dispatch_finished(&mut self, dispatch: MessageDispatch, outcome: MessageOutcome) {
        self.shell
            .instrumentation
            .finished(dispatch.trace, self.context.now(), outcome);
    }

    fn has_posted(&self) -> bool {
        !self.shell.posted.is_empty()
    }

    fn proxy(&self) -> MessageProxy<A::Message>
    where
        A::Message: Send,
    {
        let proxy = self.context.proxy();
        MessageProxy::from_fn(move |message| {
            proxy
                .run_on_main_thread(move |core: &mut RunnerCore<A>, context| {
                    core.dispatch_queued(context, message, MessageOrigin::Proxy)
                })
                .map_err(|_| ProxyClosed)
        })
    }

    fn register_task(&mut self, name: String) -> TaskSink<A::Message> {
        let (task, state) = self.shell.register_task(name, self.context.now());

        let submit_proxy = self.context.proxy();
        let submit_state = Arc::clone(&state);
        let submit = Box::new(move |factory: TaskMessageFactory<A::Message>| {
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

            let delivery_state = Arc::clone(&submit_state);
            match submit_proxy.run_on_main_thread(move |core, context| {
                core.complete_task(context, task, delivery_state, factory)
            }) {
                Ok(()) => Ok(TaskCompletionStatus::Queued),
                Err(_) => {
                    submit_state.store(TASK_CANCELLED, Ordering::Release);
                    Err(ProxyClosed)
                }
            }
        });

        let abandon_proxy = self.context.proxy();
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
            let cleanup_state = Arc::clone(&abandon_state);
            abandon_proxy
                .run_on_main_thread(move |core, _context| {
                    core.abandon_task(task, cleanup_state);
                    Ok(())
                })
                .map_err(|_| ProxyClosed)
        });

        TaskSink::new(task, submit, abandon)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn enqueue_blocking(
        &mut self,
        task: TaskId,
        job: Box<dyn FnOnce() + Send + 'static>,
    ) -> std::result::Result<(), TaskSpawnError> {
        if let Some(record) = self.shell.tasks.get_mut(&task) {
            record.kind = TaskKind::Blocking;
        }
        if self.shell.blocking_pool.is_none() {
            self.shell.blocking_pool = Some(BlockingPool::new(self.shell.task_config)?);
        }
        let result = self
            .shell
            .blocking_pool
            .as_ref()
            .expect("blocking pool was initialized")
            .enqueue(job);
        if result.is_err() {
            self.shell.cancel_task(task);
        }
        result
    }

    fn cancel_task(&mut self, task: TaskId) -> bool {
        self.shell.cancel_task(task)
    }

    fn runtime_snapshot(&self) -> RuntimeSnapshot {
        self.shell.runtime_snapshot(self.context.now())
    }

    fn reconcile_subscriptions(&mut self, desired: Subscriptions<A::Message>) -> Result<()> {
        if self.shell.exit_requested {
            self.cancel_all_subscriptions();
            return Ok(());
        }
        let desired = desired.into_unique()?;
        let desired_ids = desired
            .iter()
            .map(|subscription| subscription.id())
            .collect::<HashSet<_>>();
        let removed = self
            .shell
            .subscriptions
            .keys()
            .copied()
            .filter(|id| !desired_ids.contains(id))
            .collect::<Vec<_>>();
        for id in removed {
            if let Some(active) = self.shell.subscriptions.remove(&id)
                && let ActiveSubscriptionSource::Interval { timer, .. } = active.source
            {
                self.context.cancel_timer(timer);
            }
        }

        for subscription in desired {
            let (id, config, factory) = subscription.into_parts();
            if let Some(active) = self.shell.subscriptions.get_mut(&id)
                && active.config == config
            {
                match (&mut active.source, factory) {
                    (
                        ActiveSubscriptionSource::Interval {
                            factory: active_factory,
                            ..
                        },
                        SubscriptionFactory::Interval(factory),
                    ) => *active_factory.borrow_mut() = factory,
                    (
                        ActiveSubscriptionSource::Service {
                            decode: active_decode,
                            ..
                        },
                        SubscriptionFactory::Service(factory),
                    ) => {
                        *active_decode.borrow_mut() = factory.into_decoder();
                    }
                    _ => {
                        unreachable!("equivalent subscription configurations have matching sources")
                    }
                }
                continue;
            }
            let starts = if let Some(active) = self.shell.subscriptions.remove(&id) {
                if let ActiveSubscriptionSource::Interval { timer, .. } = active.source {
                    self.context.cancel_timer(timer);
                }
                active.starts.saturating_add(1)
            } else {
                1
            };

            let generation = self.shell.next_subscription_generation;
            self.shell.next_subscription_generation = generation.saturating_add(1);
            let source = match factory {
                SubscriptionFactory::Interval(factory) => {
                    let factory = Rc::new(RefCell::new(factory));
                    let timer = self.context.set_interval(
                        config
                            .interval()
                            .expect("interval configuration has a period"),
                        move |core: &mut RunnerCore<A>, context| {
                            core.fire_subscription(context, id, generation)
                        },
                    );
                    ActiveSubscriptionSource::Interval { timer, factory }
                }
                SubscriptionFactory::Service(factory) => {
                    let pending = Arc::new(Mutex::new(None));
                    let pending_for_sink = Arc::clone(&pending);
                    let proxy = self.context.proxy();
                    let sink = RawSubscriptionSink::new(move |event| {
                        let should_schedule = {
                            let mut pending = pending_for_sink
                                .lock()
                                .expect("subscription event slot poisoned");
                            let should_schedule = pending.is_none();
                            *pending = Some(event);
                            should_schedule
                        };
                        if should_schedule {
                            let pending = Arc::clone(&pending_for_sink);
                            let _ = proxy.run_on_main_thread(move |core, context| {
                                core.fire_service_subscription(context, id, generation, pending)
                            });
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
                    ActiveSubscriptionSource::Service {
                        _guard: guard,
                        decode: Rc::new(RefCell::new(decode)),
                        status,
                    }
                }
            };
            self.shell.subscriptions.insert(
                id,
                ActiveSubscription {
                    config,
                    generation,
                    source,
                    started_at: self.context.now(),
                    starts,
                },
            );
        }
        Ok(())
    }

    fn cancel_all_subscriptions(&mut self) {
        for (_, active) in self.shell.subscriptions.drain() {
            if let ActiveSubscriptionSource::Interval { timer, .. } = active.source {
                self.context.cancel_timer(timer);
            }
        }
    }

    fn set_timeout(
        &mut self,
        delay: Duration,
        factory: Box<dyn FnOnce() -> A::Message>,
    ) -> TimerId {
        let timer = self.shell.alloc_timer();
        let raw = timer.raw();
        let mut slot = Some(factory);
        let native = self
            .context
            .set_timeout(delay, move |core: &mut RunnerCore<A>, context| {
                core.shell.timers.remove(&raw);
                match slot.take() {
                    Some(factory) => {
                        core.dispatch_queued(context, factory(), MessageOrigin::Timeout(timer))
                    }
                    None => Ok(()),
                }
            });
        self.shell.timers.insert(raw, native);
        timer
    }

    fn set_interval(
        &mut self,
        interval: Duration,
        mut factory: Box<dyn FnMut() -> A::Message>,
    ) -> TimerId {
        let timer = self.shell.alloc_timer();
        let native =
            self.context
                .set_interval(interval, move |core: &mut RunnerCore<A>, context| {
                    let message = factory();
                    core.dispatch_queued(context, message, MessageOrigin::Interval(timer))
                });
        self.shell.timers.insert(timer.raw(), native);
        timer
    }

    fn cancel_timer(&mut self, timer: TimerId) -> bool {
        match self.shell.timers.remove(&timer.raw()) {
            Some(native) => self.context.cancel_timer(native),
            None => false,
        }
    }

    fn set_policy(&mut self, policy: RuntimePolicy) {
        self.context.set_policy(policy);
    }

    fn clipboard(&self) -> Clipboard {
        self.context.clipboard()
    }

    fn now(&self) -> Instant {
        self.context.now()
    }

    fn available_monitors(&self) -> Vec<Monitor> {
        self.context.available_monitors()
    }

    fn primary_monitor(&self) -> Option<Monitor> {
        self.context.primary_monitor()
    }

    fn exit(&mut self) {
        self.shell.exit_requested = true;
        self.context.exit();
    }
}

/// Applies a routed host update: close handling, message dispatch, posted
/// re-drain, and redraw invalidation.
fn process_host_update<A: App>(
    user: &mut A,
    backend: &mut dyn AppBackend<A::Message>,
    window: WindowId,
    update: HostUpdate,
    messages: Vec<A::Message>,
    exit_on_last_window_close: bool,
) -> Result<()> {
    let had_windows = !backend.windows().is_empty();
    let mut closed = false;
    if update.close_requested {
        let response =
            user.close_requested(&mut AppCx::new(&mut *backend, Some(window)), window)?;
        if response == CloseResponse::Close {
            backend.close_window(window)?;
            user.window_closed(&mut AppCx::new(&mut *backend, Some(window)), window)?;
            closed = true;
        }
    }
    if !closed {
        for message in messages {
            let message = backend.instrument_message(message, Some(window), MessageOrigin::Ui);
            dispatch_message(user, backend, message)?;
        }
    }
    flush_posted(user, backend)?;
    reconcile_subscriptions(user, backend)?;
    if !closed && update.redraw && backend.windows().contains(&window) {
        backend.invalidate(window);
    }
    invalidate_dirty(backend);
    if exit_on_last_window_close && had_windows && backend.windows().is_empty() {
        backend.exit();
    }
    Ok(())
}

/// Dispatches one message arriving outside a window event.
fn dispatch_external<A: App>(
    user: &mut A,
    backend: &mut dyn AppBackend<A::Message>,
    message: A::Message,
    origin: MessageOrigin,
    exit_on_last_window_close: bool,
) -> Result<()> {
    let had_windows = !backend.windows().is_empty();
    let message = backend.instrument_message(message, None, origin);
    dispatch_message(user, backend, message)?;
    flush_posted(user, backend)?;
    reconcile_subscriptions(user, backend)?;
    invalidate_dirty(backend);
    if exit_on_last_window_close && had_windows && backend.windows().is_empty() {
        backend.exit();
    }
    Ok(())
}

/// Drains messages posted with [`AppCx::post`] through [`App::update`].
///
/// Updates may post further messages, so draining repeats up to
/// [`MAX_POSTED_PASSES`] times; any surplus remains queued for the next
/// event-loop turn rather than looping forever. Returns whether any message
/// was dispatched.
fn flush_posted<A: App>(user: &mut A, backend: &mut dyn AppBackend<A::Message>) -> Result<bool> {
    let mut processed = false;
    for _ in 0..MAX_POSTED_PASSES {
        let batch = backend.take_posted();
        if batch.is_empty() {
            return Ok(processed);
        }
        processed = true;
        for message in batch {
            dispatch_message(user, backend, message)?;
        }
    }
    if backend.has_posted() {
        debug_assert!(
            false,
            "posted-message re-drain exceeded {MAX_POSTED_PASSES} passes; an `update` \
             handler keeps posting new messages on every pass"
        );
    }
    Ok(processed)
}

fn dispatch_message<A: App>(
    user: &mut A,
    backend: &mut dyn AppBackend<A::Message>,
    message: QueuedMessage<A::Message>,
) -> Result<()> {
    let (message, source, mut dispatch) = message.into_parts();
    backend.message_dispatch_started(&mut dispatch);
    let result = user.update(&mut AppCx::new(&mut *backend, source), message);
    let outcome = if result.is_ok() {
        MessageOutcome::Success
    } else {
        MessageOutcome::Error
    };
    backend.message_dispatch_finished(dispatch, outcome);
    result
}

fn reconcile_subscriptions<A: App>(
    user: &A,
    backend: &mut dyn AppBackend<A::Message>,
) -> Result<()> {
    backend.reconcile_subscriptions(user.subscriptions())
}

/// Invalidates every window whose UI reports pending redraw work.
fn invalidate_dirty<M: 'static>(backend: &mut dyn AppBackend<M>) {
    for window in backend.windows() {
        let dirty = backend
            .ui_mut(window)
            .map(|ui| ui.needs_redraw())
            .unwrap_or(false);
        if dirty {
            backend.invalidate(window);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use astrelis_platform::{ClipboardCapabilities, backend as platform_backend};

    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Msg {
        Step(u32),
        Posted(u32),
        Spam,
        LatestSpam,
        CloseSource,
    }

    #[derive(Default)]
    struct TestApp {
        log: Vec<String>,
        close_response: CloseResponse,
    }

    impl App for TestApp {
        type Message = Msg;

        fn build(&mut self, _cx: &mut AppCx<'_, Msg>) -> Result<()> {
            Ok(())
        }

        fn update(&mut self, cx: &mut AppCx<'_, Msg>, message: Msg) -> Result<()> {
            match message {
                Msg::Step(value) => {
                    self.log.push(format!("step:{value}"));
                    if value == 1 {
                        cx.post(Msg::Posted(value));
                    }
                }
                Msg::Posted(value) => self.log.push(format!("posted:{value}")),
                Msg::Spam => cx.post(Msg::Spam),
                Msg::LatestSpam => cx.post_latest(
                    MessageKey::singleton("testing.latest-spam"),
                    Msg::LatestSpam,
                ),
                Msg::CloseSource => {
                    self.log.push("close-source".into());
                    let window = cx.source_window().expect("source window recorded");
                    cx.close_window(window)?;
                }
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

    #[derive(Debug)]
    struct MockClipboard;

    impl platform_backend::Clipboard for MockClipboard {
        fn capabilities(&self) -> ClipboardCapabilities {
            ClipboardCapabilities::default()
        }

        fn read_text(&self) -> std::result::Result<Option<String>, PlatformError> {
            Ok(None)
        }

        fn write_text(&self, _text: String) -> std::result::Result<(), PlatformError> {
            Ok(())
        }
    }

    struct MockBackend<M: 'static> {
        uis: Vec<(WindowId, Ui<M>)>,
        posted: PostedQueue<M>,
        instrumentation: InstrumentationState,
        proxy_sink: Arc<Mutex<Vec<M>>>,
        invalidated: Vec<WindowId>,
        closed: Vec<WindowId>,
        presented: Vec<WindowId>,
        cancelled: Vec<TimerId>,
        task_states: HashMap<TaskId, Arc<AtomicU8>>,
        policy: Option<RuntimePolicy>,
        exited: bool,
        next_window: u64,
        next_timer: u64,
        next_task: u64,
    }

    impl<M: 'static> MockBackend<M> {
        fn new() -> Self {
            Self {
                uis: Vec::new(),
                posted: PostedQueue::default(),
                instrumentation: InstrumentationState::new(RuntimeInstrumentationConfig::default()),
                proxy_sink: Arc::new(Mutex::new(Vec::new())),
                invalidated: Vec::new(),
                closed: Vec::new(),
                presented: Vec::new(),
                cancelled: Vec::new(),
                task_states: HashMap::new(),
                policy: None,
                exited: false,
                next_window: 1,
                next_timer: 1,
                next_task: 1,
            }
        }

        fn open(&mut self) -> WindowId {
            let ui = self.new_ui();
            self.open_window(WindowConfig::default(), ui)
                .expect("mock windows always open")
        }
    }

    impl<M: 'static> AppBackend<M> for MockBackend<M> {
        fn new_ui(&mut self) -> Ui<M> {
            Ui::new(FontDatabase::default(), Theme::default())
        }

        fn open_window(&mut self, _config: WindowConfig, ui: Ui<M>) -> Result<WindowId> {
            let window = WindowId(self.next_window);
            self.next_window += 1;
            self.uis.push((window, ui));
            Ok(window)
        }

        fn close_window(&mut self, window: WindowId) -> Result<()> {
            let Some(index) = self.uis.iter().position(|(id, _)| *id == window) else {
                return Err(Error::msg("window is not open"));
            };
            self.uis.remove(index);
            self.closed.push(window);
            Ok(())
        }

        fn windows(&self) -> Vec<WindowId> {
            self.uis.iter().map(|(id, _)| *id).collect()
        }

        fn ui_mut(&mut self, window: WindowId) -> Result<&mut Ui<M>> {
            self.uis
                .iter_mut()
                .find(|(id, _)| *id == window)
                .map(|(_, ui)| ui)
                .ok_or_else(|| Error::msg("window is not open"))
        }

        fn window(&self, _window: WindowId) -> Result<&Window> {
            Err(Error::msg("the mock backend has no native windows"))
        }

        fn host_mut(&mut self, _window: WindowId) -> Result<&mut WindowHost<M>> {
            Err(Error::msg("the mock backend has no window hosts"))
        }

        fn present(&mut self, window: WindowId) -> Result<()> {
            self.presented.push(window);
            Ok(())
        }

        fn invalidate(&mut self, window: WindowId) {
            self.invalidated.push(window);
        }

        fn invalidate_all(&mut self) {
            let windows = self.windows();
            self.invalidated.extend(windows);
        }

        fn post(&mut self, message: M, source: Option<WindowId>) {
            self.posted.post(QueuedMessage::new(message, source, None));
        }

        fn post_latest(&mut self, key: MessageKey, message: M, source: Option<WindowId>) {
            let message = match self.posted.replace_latest(
                key,
                message,
                source,
                MessageMetadata::unnamed(),
                MessageOrigin::Posted,
                &mut self.instrumentation,
            ) {
                Ok(()) => return,
                Err(message) => message,
            };
            self.posted
                .post_keyed(key, QueuedMessage::new(message, source, None));
        }

        fn take_posted(&mut self) -> Vec<QueuedMessage<M>> {
            self.posted.take()
        }

        fn instrument_message(
            &mut self,
            message: M,
            source: Option<WindowId>,
            _origin: MessageOrigin,
        ) -> QueuedMessage<M> {
            QueuedMessage::new(message, source, None)
        }

        fn message_dispatch_started(&mut self, _dispatch: &mut MessageDispatch) {}

        fn message_dispatch_finished(
            &mut self,
            _dispatch: MessageDispatch,
            _outcome: MessageOutcome,
        ) {
        }

        fn has_posted(&self) -> bool {
            !self.posted.is_empty()
        }

        fn proxy(&self) -> MessageProxy<M>
        where
            M: Send,
        {
            let sink = self.proxy_sink.clone();
            MessageProxy::from_fn(move |message| {
                sink.lock().expect("proxy sink poisoned").push(message);
                Ok(())
            })
        }

        fn register_task(&mut self, _name: String) -> TaskSink<M> {
            let task = TaskId::from_raw(self.next_task);
            self.next_task += 1;
            let state = Arc::new(AtomicU8::new(TASK_PENDING));
            self.task_states.insert(task, Arc::clone(&state));

            let submit_state = Arc::clone(&state);
            let submit = Box::new(move |_factory: TaskMessageFactory<M>| {
                if submit_state
                    .compare_exchange(
                        TASK_PENDING,
                        TASK_COMPLETION_QUEUED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    Ok(TaskCompletionStatus::Queued)
                } else {
                    Ok(TaskCompletionStatus::Cancelled)
                }
            });
            let abandon_state = Arc::clone(&state);
            let abandon = Box::new(move || {
                abandon_state.store(TASK_CANCELLED, Ordering::Release);
                Ok(())
            });
            TaskSink::new(task, submit, abandon)
        }

        #[cfg(not(target_arch = "wasm32"))]
        fn enqueue_blocking(
            &mut self,
            _task: TaskId,
            job: Box<dyn FnOnce() + Send + 'static>,
        ) -> std::result::Result<(), TaskSpawnError> {
            job();
            Ok(())
        }

        fn cancel_task(&mut self, task: TaskId) -> bool {
            let Some(state) = self.task_states.remove(&task) else {
                return false;
            };
            state.store(TASK_CANCELLED, Ordering::Release);
            true
        }

        fn runtime_snapshot(&self) -> RuntimeSnapshot {
            RuntimeSnapshot::default()
        }

        fn reconcile_subscriptions(&mut self, desired: Subscriptions<M>) -> Result<()> {
            desired.into_unique().map(|_| ())
        }

        fn cancel_all_subscriptions(&mut self) {}

        fn set_timeout(&mut self, _delay: Duration, _factory: Box<dyn FnOnce() -> M>) -> TimerId {
            let timer = TimerId::from_raw(self.next_timer);
            self.next_timer += 1;
            timer
        }

        fn set_interval(
            &mut self,
            _interval: Duration,
            _factory: Box<dyn FnMut() -> M>,
        ) -> TimerId {
            let timer = TimerId::from_raw(self.next_timer);
            self.next_timer += 1;
            timer
        }

        fn cancel_timer(&mut self, timer: TimerId) -> bool {
            self.cancelled.push(timer);
            true
        }

        fn set_policy(&mut self, policy: RuntimePolicy) {
            self.policy = Some(policy);
        }

        fn clipboard(&self) -> Clipboard {
            Clipboard::from_backend(Arc::new(MockClipboard))
        }

        fn now(&self) -> Instant {
            Instant::now()
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

    #[test]
    fn dispatches_messages_in_order_then_posted_messages() {
        let mut app = TestApp::default();
        let mut backend = MockBackend::new();
        let window = backend.open();
        process_host_update(
            &mut app,
            &mut backend,
            window,
            HostUpdate::default(),
            vec![Msg::Step(1), Msg::Step(2)],
            true,
        )
        .expect("dispatch succeeds");
        assert_eq!(app.log, ["step:1", "step:2", "posted:1"]);
        assert!(!backend.exited);
    }

    #[test]
    fn keyed_posts_replace_in_place_with_the_latest_source() {
        let first = WindowId(11);
        let second = WindowId(22);
        let viewport = MessageKey::new("chart.viewport", 7);
        let progress = MessageKey::singleton("load.progress");
        assert_eq!(viewport.namespace(), "chart.viewport");
        assert_eq!(viewport.instance(), 7);

        let mut backend = MockBackend::new();
        backend.post_latest(viewport, Msg::Step(1), Some(first));
        backend.post(Msg::Posted(9), None);
        backend.post_latest(progress, Msg::Step(2), Some(first));
        backend.post_latest(viewport, Msg::Step(3), Some(second));

        let messages = backend
            .take_posted()
            .into_iter()
            .map(|message| {
                let (message, source, _) = message.into_parts();
                (message, source)
            })
            .collect::<Vec<_>>();

        assert_eq!(
            messages,
            [
                (Msg::Step(3), Some(second)),
                (Msg::Posted(9), None),
                (Msg::Step(2), Some(first)),
            ]
        );
    }

    #[test]
    fn a_drained_key_starts_a_new_pending_entry() {
        let key = MessageKey::singleton("testing.preview");
        let mut backend = MockBackend::new();
        backend.post_latest(key, Msg::Step(1), None);
        let (message, source, _) = backend.take_posted().pop().unwrap().into_parts();
        assert_eq!((message, source), (Msg::Step(1), None));
        backend.post_latest(key, Msg::Step(2), None);
        let (message, source, _) = backend.take_posted().pop().unwrap().into_parts();
        assert_eq!((message, source), (Msg::Step(2), None));
    }

    #[test]
    #[should_panic(expected = "re-drain")]
    fn posted_message_re_drain_is_bounded() {
        let mut app = TestApp::default();
        let mut backend = MockBackend::new();
        let window = backend.open();
        let _ = process_host_update(
            &mut app,
            &mut backend,
            window,
            HostUpdate::default(),
            vec![Msg::Spam],
            true,
        );
    }

    #[test]
    #[should_panic(expected = "re-drain")]
    fn keyed_post_re_drain_is_bounded() {
        let mut app = TestApp::default();
        let mut backend = MockBackend::new();
        let window = backend.open();
        let _ = process_host_update(
            &mut app,
            &mut backend,
            window,
            HostUpdate::default(),
            vec![Msg::LatestSpam],
            true,
        );
    }

    #[test]
    fn close_flow_runs_hooks_and_exits_after_last_window() {
        let mut app = TestApp::default();
        let mut backend = MockBackend::new();
        let window = backend.open();
        process_host_update(
            &mut app,
            &mut backend,
            window,
            HostUpdate {
                close_requested: true,
                ..Default::default()
            },
            Vec::new(),
            true,
        )
        .expect("close succeeds");
        assert_eq!(app.log, ["close_requested", "window_closed"]);
        assert_eq!(backend.closed, [window]);
        assert!(backend.exited);
    }

    #[test]
    fn closing_one_of_two_windows_does_not_exit() {
        let mut app = TestApp::default();
        let mut backend = MockBackend::new();
        let first = backend.open();
        let second = backend.open();
        process_host_update(
            &mut app,
            &mut backend,
            first,
            HostUpdate {
                close_requested: true,
                ..Default::default()
            },
            Vec::new(),
            true,
        )
        .expect("close succeeds");
        assert_eq!(backend.closed, [first]);
        assert_eq!(backend.windows(), [second]);
        assert!(!backend.exited);
    }

    #[test]
    fn close_request_can_be_ignored() {
        let mut app = TestApp {
            close_response: CloseResponse::Ignore,
            ..Default::default()
        };
        let mut backend = MockBackend::new();
        let window = backend.open();
        process_host_update(
            &mut app,
            &mut backend,
            window,
            HostUpdate {
                close_requested: true,
                ..Default::default()
            },
            Vec::new(),
            true,
        )
        .expect("ignored close succeeds");
        assert_eq!(app.log, ["close_requested"]);
        assert!(backend.closed.is_empty());
        assert_eq!(backend.windows(), [window]);
        assert!(!backend.exited);
    }

    #[test]
    fn exit_policy_can_be_disabled() {
        let mut app = TestApp::default();
        let mut backend = MockBackend::new();
        let window = backend.open();
        process_host_update(
            &mut app,
            &mut backend,
            window,
            HostUpdate {
                close_requested: true,
                ..Default::default()
            },
            Vec::new(),
            false,
        )
        .expect("close succeeds");
        assert_eq!(backend.closed, [window]);
        assert!(!backend.exited);
    }

    #[test]
    fn update_may_close_its_own_source_window() {
        let mut app = TestApp::default();
        let mut backend = MockBackend::new();
        let window = backend.open();
        process_host_update(
            &mut app,
            &mut backend,
            window,
            HostUpdate {
                redraw: true,
                ..Default::default()
            },
            vec![Msg::CloseSource],
            true,
        )
        .expect("reentrant close succeeds");
        assert_eq!(app.log, ["close-source"]);
        assert_eq!(backend.closed, [window]);
        // The closed window must not be invalidated even though the host
        // update requested a redraw.
        assert!(backend.invalidated.is_empty());
        assert!(backend.exited);
    }

    #[test]
    fn source_ui_prefers_the_source_window() {
        let mut backend = MockBackend::<Msg>::new();
        let first = backend.open();
        let _second = backend.open();
        let mut cx = AppCx::new(&mut backend, Some(first));
        assert_eq!(cx.source_window(), Some(first));
        assert!(cx.source_ui().is_ok());
    }

    #[test]
    fn source_ui_falls_back_to_the_sole_window() {
        let mut backend = MockBackend::<Msg>::new();
        let only = backend.open();
        // No source recorded: the sole window is unambiguous.
        assert!(AppCx::new(&mut backend, None).source_ui().is_ok());
        // A stale source falls back to the sole remaining window.
        let stale = WindowId(only.0 + 100);
        assert!(AppCx::new(&mut backend, Some(stale)).source_ui().is_ok());
    }

    #[test]
    fn source_ui_errors_when_ambiguous_or_empty() {
        let mut backend = MockBackend::<Msg>::new();
        assert!(AppCx::new(&mut backend, None).source_ui().is_err());
        backend.open();
        backend.open();
        assert!(AppCx::new(&mut backend, None).source_ui().is_err());
    }

    #[test]
    fn external_dispatch_applies_the_exit_policy() {
        let mut app = TestApp::default();
        let mut backend = MockBackend::new();
        let window = backend.open();
        // `dispatch_external` has no source window, so route the close by id.
        dispatch_external(
            &mut app,
            &mut backend,
            Msg::Step(7),
            MessageOrigin::External,
            true,
        )
        .expect("dispatch succeeds");
        assert_eq!(app.log, ["step:7"]);
        assert!(!backend.exited);
        assert_eq!(backend.windows(), [window]);
    }

    #[test]
    fn window_config_layers_onto_host_options() {
        let options = WindowConfig::new("Test Window")
            .size(640.0, 480.0)
            .resizable(false)
            .clear_color(Color::WHITE)
            .into_host_options();
        assert_eq!(options.window.title, "Test Window");
        assert_eq!(
            options.window.inner_size,
            Some(astrelis_core::geometry::Size::new(640.0, 480.0))
        );
        assert!(!options.window.resizable);
        assert_eq!(options.clear_color, Color::WHITE);
    }

    #[test]
    fn message_proxy_posts_into_the_sink() {
        let backend = MockBackend::<Msg>::new();
        let proxy = AppBackend::proxy(&backend);
        let clone = proxy.clone();
        clone.post(Msg::Step(3)).expect("proxy is open");
        assert_eq!(
            backend.proxy_sink.lock().expect("sink").as_slice(),
            [Msg::Step(3)]
        );
    }

    #[test]
    fn cancelled_registered_task_rejects_completion() {
        let mut backend = MockBackend::<Msg>::new();
        let completion = AppCx::new(&mut backend, None).register_task(Msg::Step);
        let task = completion.id();
        assert!(AppCx::new(&mut backend, None).cancel_task(task));
        assert_eq!(
            completion.complete(3).expect("mock runtime remains open"),
            TaskCompletionStatus::Cancelled
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn blocking_pool_bounds_waiting_work() {
        let pool = BlockingPool::new(
            TaskConfig::default()
                .blocking_workers(1)
                .blocking_queue_capacity(1),
        )
        .expect("worker starts");
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        pool.enqueue(Box::new(move || {
            started_tx.send(()).expect("test remains alive");
            release_rx.recv().expect("test releases worker");
        }))
        .expect("first task starts");
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("first task starts promptly");

        let (queued_tx, queued_rx) = mpsc::channel();
        pool.enqueue(Box::new(move || {
            queued_tx.send(()).expect("test remains alive");
        }))
        .expect("second task enters waiting queue");
        assert!(matches!(
            pool.enqueue(Box::new(|| {})),
            Err(TaskSpawnError::QueueFull)
        ));

        release_tx.send(()).expect("worker releases");
        queued_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("queued task eventually runs");
    }

    #[test]
    #[should_panic(expected = "blocking worker count must be non-zero")]
    fn zero_blocking_workers_are_rejected() {
        let _ = TaskConfig::default().blocking_workers(0);
    }

    #[test]
    #[should_panic(expected = "blocking task queue capacity must be non-zero")]
    fn zero_blocking_queue_capacity_is_rejected() {
        let _ = TaskConfig::default().blocking_queue_capacity(0);
    }
}
