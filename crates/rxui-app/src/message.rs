//! Typed mapping between feature-local and application messages.

use std::{fmt, sync::Arc, time::Duration};

use astrelis_platform::Window;
use astrelis_ui_core::{EventContext, Ui};
use astrelis_ui_host::WindowHost;

use crate::Result;
use crate::runner::{
    AppCx, Clipboard, MessageKey, MessageProxy, Monitor, RuntimePolicy, TaskCompletion, TaskId,
    TimerId, WindowConfig, WindowId,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::runner::{TaskError, TaskSpawnError};

/// A cheap, retained mapping from a feature-local message to an application's
/// root message.
///
/// The mapping closure is thread-safe so the same mapper can be used by UI
/// listeners, timers, and [`MessageProxy`] handles. The message types
/// themselves only need to be [`Send`] when a proxy crosses threads.
pub struct MessageMapper<Local: 'static, Root: 'static> {
    map: Arc<dyn Fn(Local) -> Root + Send + Sync + 'static>,
}

impl<Local: 'static, Root: 'static> MessageMapper<Local, Root> {
    /// Creates a mapper from a feature-local message into the root message.
    pub fn new(map: impl Fn(Local) -> Root + Send + Sync + 'static) -> Self {
        Self { map: Arc::new(map) }
    }

    /// Maps one feature-local message into the root message type.
    pub fn map(&self, message: Local) -> Root {
        (self.map)(message)
    }

    /// Maps and emits one message from a retained UI event listener.
    pub fn emit(&self, cx: &mut EventContext<'_, Root>, message: Local) {
        cx.emit(self.map(message));
    }

    /// Derives a mapper for a nested child feature.
    ///
    /// The supplied closure first maps `Child` into `Local`; this mapper then
    /// maps the result into `Root`.
    pub fn map_child<Child: 'static>(
        &self,
        map: impl Fn(Child) -> Local + Send + Sync + 'static,
    ) -> MessageMapper<Child, Root> {
        let parent = self.clone();
        MessageMapper::new(move |message| parent.map(map(message)))
    }

    fn compose<Child: 'static>(
        &self,
        child: MessageMapper<Child, Local>,
    ) -> MessageMapper<Child, Root> {
        let parent = self.clone();
        MessageMapper::new(move |message| parent.map(child.map(message)))
    }
}

impl<Local: 'static, Root: 'static> Clone for MessageMapper<Local, Root> {
    fn clone(&self) -> Self {
        Self {
            map: Arc::clone(&self.map),
        }
    }
}

impl<Local: 'static, Root: 'static> fmt::Debug for MessageMapper<Local, Root> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MessageMapper")
            .finish_non_exhaustive()
    }
}

/// An application context whose message-producing operations accept a
/// feature-local message type.
///
/// UI trees and window hosts keep the application's `Root` message type. Use
/// [`message_mapper`](Self::message_mapper) in widget callbacks and
/// [`root`](Self::root) when deliberately emitting a root-level message.
pub struct MappedAppCx<'a, Local: 'static, Root: 'static> {
    root: AppCx<'a, Root>,
    mapper: MessageMapper<Local, Root>,
}

impl<M: 'static> AppCx<'_, M> {
    /// Reborrows this context so feature-local messages are mapped into `M`.
    pub fn map_messages<Local: 'static>(
        &mut self,
        mapper: MessageMapper<Local, M>,
    ) -> MappedAppCx<'_, Local, M> {
        MappedAppCx {
            root: self.reborrow(),
            mapper,
        }
    }
}

impl<'a, Local: 'static, Root: 'static> MappedAppCx<'a, Local, Root> {
    /// Returns a cheap clone of this context's retained message mapper.
    pub fn message_mapper(&self) -> MessageMapper<Local, Root> {
        self.mapper.clone()
    }

    /// Reborrows this context with an additional child-to-local mapping.
    pub fn map_messages<Child: 'static>(
        &mut self,
        mapper: MessageMapper<Child, Local>,
    ) -> MappedAppCx<'_, Child, Root> {
        let mapper = self.mapper.compose(mapper);
        self.root.map_messages(mapper)
    }

    /// Returns the underlying root-message application context.
    pub fn root(&mut self) -> &mut AppCx<'a, Root> {
        &mut self.root
    }

    /// Builds an empty UI using the application's root message type.
    pub fn new_ui(&mut self) -> Ui<Root> {
        self.root.new_ui()
    }

    /// Opens a window hosting a root-message UI.
    pub fn open_window(&mut self, config: WindowConfig, ui: Ui<Root>) -> Result<WindowId> {
        self.root.open_window(config, ui)
    }

    /// Closes a window programmatically.
    pub fn close_window(&mut self, window: WindowId) -> Result<()> {
        self.root.close_window(window)
    }

    /// Returns every open window in creation order.
    pub fn windows(&self) -> Vec<WindowId> {
        self.root.windows()
    }

    /// Returns a window's root-message UI tree.
    pub fn ui(&mut self, window: WindowId) -> Result<&mut Ui<Root>> {
        self.root.ui(window)
    }

    /// Returns the root-message UI tree that produced the current message.
    pub fn source_ui(&mut self) -> Result<&mut Ui<Root>> {
        self.root.source_ui()
    }

    /// Returns the window that produced the current message, when known.
    pub fn source_window(&self) -> Option<WindowId> {
        self.root.source_window()
    }

    /// Returns a window's platform handle.
    pub fn window(&self, window: WindowId) -> Result<&Window> {
        self.root.window(window)
    }

    /// Returns a window's host for GPU-level access.
    pub fn host(&mut self, window: WindowId) -> Result<&mut WindowHost<Root>> {
        self.root.host(window)
    }

    /// Generates and presents one UI frame for a window.
    pub fn present(&mut self, window: WindowId) -> Result<()> {
        self.root.present(window)
    }

    /// Marks one window as needing redraw.
    pub fn invalidate(&mut self, window: WindowId) {
        self.root.invalidate(window);
    }

    /// Marks every window as needing redraw.
    pub fn invalidate_all(&mut self) {
        self.root.invalidate_all();
    }

    /// Queues a local message after mapping it into the root type.
    pub fn post(&mut self, message: Local) {
        self.root.post(self.mapper.map(message));
    }

    /// Queues or replaces one pending mapped latest-value message.
    pub fn post_latest(&mut self, key: MessageKey, message: Local) {
        self.root.post_latest(key, self.mapper.map(message));
    }

    /// Returns a thread-safe proxy that accepts local messages.
    ///
    /// ```compile_fail
    /// # use std::rc::Rc;
    /// # use rxui_app::{AppCx, MessageMapper};
    /// # enum Root { Local(Rc<()>) }
    /// # fn cannot_proxy(cx: &mut AppCx<'_, Root>) {
    /// let mapper = MessageMapper::new(Root::Local);
    /// let local = cx.map_messages(mapper);
    /// let _ = local.proxy(); // `Rc<()>` is not `Send`.
    /// # }
    /// ```
    pub fn proxy(&self) -> MessageProxy<Local>
    where
        Local: Send,
        Root: Send,
    {
        let root = self.root.proxy();
        let mapper = self.mapper.clone();
        MessageProxy::from_fn(move |message| root.post(mapper.map(message)))
    }

    /// Registers a mapped task completed by an externally owned executor.
    pub fn register_task<T: Send + 'static>(
        &mut self,
        map: impl FnOnce(T) -> Local + Send + 'static,
    ) -> TaskCompletion<T> {
        let mapper = self.mapper.clone();
        self.root
            .register_task(move |output| mapper.map(map(output)))
    }

    /// Runs mapped finite work on the native bounded blocking pool.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn spawn_blocking<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
        map: impl FnOnce(std::result::Result<T, TaskError>) -> Local + Send + 'static,
    ) -> std::result::Result<TaskId, TaskSpawnError> {
        let mapper = self.mapper.clone();
        self.root
            .spawn_blocking(work, move |result| mapper.map(map(result)))
    }

    /// Cancels a task, returning whether it was still active.
    pub fn cancel_task(&mut self, task: TaskId) -> bool {
        self.root.cancel_task(task)
    }

    /// Schedules one mapped local message after a delay.
    pub fn set_timeout(&mut self, delay: Duration, message: Local) -> TimerId {
        self.root.set_timeout(delay, self.mapper.map(message))
    }

    /// Schedules one mapped local message produced after a delay.
    pub fn set_timeout_with(
        &mut self,
        delay: Duration,
        factory: impl FnOnce() -> Local + 'static,
    ) -> TimerId {
        let mapper = self.mapper.clone();
        self.root
            .set_timeout_with(delay, move || mapper.map(factory()))
    }

    /// Schedules one cloneable local message repeatedly at an interval.
    pub fn set_interval(&mut self, interval: Duration, message: Local) -> TimerId
    where
        Local: Clone,
    {
        self.set_interval_with(interval, move || message.clone())
    }

    /// Schedules mapped local messages produced by an interval factory.
    pub fn set_interval_with(
        &mut self,
        interval: Duration,
        mut factory: impl FnMut() -> Local + 'static,
    ) -> TimerId {
        let mapper = self.mapper.clone();
        self.root
            .set_interval_with(interval, move || mapper.map(factory()))
    }

    /// Cancels a timer, returning whether it was still scheduled.
    pub fn cancel_timer(&mut self, timer: TimerId) -> bool {
        self.root.cancel_timer(timer)
    }

    /// Changes the runtime scheduling policy.
    pub fn set_policy(&mut self, policy: RuntimePolicy) {
        self.root.set_policy(policy);
    }

    /// Returns the platform clipboard.
    pub fn clipboard(&self) -> Clipboard {
        self.root.clipboard()
    }

    /// Returns the current time on the application clock.
    pub fn now(&self) -> crate::runner::Instant {
        self.root.now()
    }

    /// Returns all currently available monitors.
    pub fn available_monitors(&self) -> Vec<Monitor> {
        self.root.available_monitors()
    }

    /// Returns the primary monitor when known.
    pub fn primary_monitor(&self) -> Option<Monitor> {
        self.root.primary_monitor()
    }

    /// Requests orderly application termination.
    pub fn exit(&mut self) {
        self.root.exit();
    }
}

impl<Local: 'static, Root: 'static> fmt::Debug for MappedAppCx<'_, Local, Root> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MappedAppCx")
            .field("root", &self.root)
            .field("mapper", &self.mapper)
            .finish()
    }
}
