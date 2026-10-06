use crate::{AppContext, EntityId, Runtime, runtime::RuntimeInner};
use futures_util::{
    FutureExt,
    future::{AbortHandle, Abortable},
};
use std::{
    any::Any,
    collections::HashMap,
    error::Error,
    fmt,
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc,
    },
    time::Duration,
};

/// Owned, Send future submitted to an application's executor.
pub type BackgroundFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
/// Owned blocking work, executed outside the UI and async polling threads.
pub type BlockingJob = Box<dyn FnOnce() + Send + 'static>;

/// Host-provided execution capability. Rejected jobs must not be started.
/// Futures may require their service runtime's reactor; installing a custom
/// executor lets applications supply it without putting a network runtime in RXUI.
pub trait TaskExecutor: Send + Sync + 'static {
    /// Schedules a Send future without blocking the UI thread.
    fn spawn(&self, future: BackgroundFuture) -> Result<(), SpawnError>;
    /// Schedules blocking work on a separate executor/pool.
    fn spawn_blocking(&self, _job: BlockingJob) -> Result<(), SpawnError> {
        Err(SpawnError::BlockingUnavailable)
    }
}
/// Task setup or scheduling failure, before an accepted job starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpawnError {
    /// Runtime has no executor/wakeup adapter.
    Unavailable,
    /// Runtime execution was configured more than once.
    AlreadyConfigured,
    /// Installed adapter does not support blocking work.
    BlockingUnavailable,
    /// Executor rejected this work without starting it.
    Rejected(String),
}
impl fmt::Display for SpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("RXUI task executor is not configured"),
            Self::AlreadyConfigured => f.write_str("RXUI task executor is already configured"),
            Self::BlockingUnavailable => f.write_str("RXUI executor has no blocking-work adapter"),
            Self::Rejected(reason) => write!(f, "RXUI executor rejected work: {reason}"),
        }
    }
}
impl Error for SpawnError {}
/// Captured panic from task polling or a blocking job. Domain errors remain part
/// of the future's ordinary output, for example TaskResult<Result<Data, ApiError>>.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskError {
    /// Task panicked; unwinding stayed outside the UI update callback.
    Panicked(String),
}
impl fmt::Display for TaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Panicked(message) => write!(f, "RXUI task panicked: {message}"),
        }
    }
}
impl Error for TaskError {}
/// Completion of execution, separate from errors in the task's output type.
pub type TaskResult<T> = Result<T, TaskError>;

const RUNNING: u8 = 0;
const CANCELLED: u8 = 1;
const FINISHED: u8 = 2;
type Outcome = TaskResult<Box<dyn Any + Send>>;
struct Packet {
    id: u64,
    outcome: Option<Outcome>,
}
struct Hub {
    sender: mpsc::Sender<Packet>,
    pending: AtomicBool,
    wake: Box<dyn Fn() + Send + Sync>,
}
impl Hub {
    fn send(&self, packet: Packet) {
        if self.sender.send(packet).is_ok() {
            self.request_wake();
        }
    }
    fn request_wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            (self.wake)();
        }
    }
}
struct Control {
    id: u64,
    status: AtomicU8,
    abort: AbortHandle,
    hub: Weak<Hub>,
}
impl Control {
    fn cancel(&self) -> bool {
        if self
            .status
            .compare_exchange(RUNNING, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        self.abort.abort();
        if let Some(hub) = self.hub.upgrade() {
            hub.send(Packet {
                id: self.id,
                outcome: None,
            });
        }
        true
    }
}
/// One scoped background job. Dropping it cancels delivery and aborts pollable
/// work; already-running blocking work can only cooperate or discard its result.
/// A detached typed task still ends with its owner/runtime. It retains no entity.
#[must_use = "keep the task handle alive, or call detach explicitly"]
pub struct Task {
    control: Arc<Control>,
    cancel_on_drop: bool,
}
impl Task {
    /// Cancels once, including a result queued but not yet delivered.
    pub fn cancel(&self) -> bool {
        self.control.cancel()
    }
    /// Whether this task was cancelled or its completion was claimed on the UI thread.
    pub fn is_finished(&self) -> bool {
        self.control.status.load(Ordering::Acquire) != RUNNING
    }
    /// Releases handle ownership while keeping the owner/runtime-scoped job alive.
    pub fn detach(mut self) {
        self.cancel_on_drop = false;
    }
}
impl Drop for Task {
    fn drop(&mut self) {
        if self.cancel_on_drop {
            self.control.cancel();
        }
    }
}
impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Task")
            .field("finished", &self.is_finished())
            .finish_non_exhaustive()
    }
}
type Completion = dyn FnOnce(Outcome, &mut AppContext<'_>);
struct Record {
    owner: Option<EntityId>,
    control: Arc<Control>,
    completion: Box<Completion>,
}
pub(crate) struct Tasks {
    executor: Arc<dyn TaskExecutor>,
    hub: Arc<Hub>,
    receiver: mpsc::Receiver<Packet>,
    records: HashMap<u64, Record>,
    next: u64,
}
impl Tasks {
    fn new(executor: Arc<dyn TaskExecutor>, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            executor,
            hub: Arc::new(Hub {
                sender,
                pending: AtomicBool::new(false),
                wake: Box::new(wake),
            }),
            receiver,
            records: HashMap::new(),
            next: 1,
        }
    }
    pub(crate) fn cancel_owner(&mut self, owner: EntityId) -> Vec<Box<Completion>> {
        let ids: Vec<_> = self
            .records
            .iter()
            .filter(|(_, record)| record.owner == Some(owner))
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.records.remove(&id))
            .map(|record| {
                record.control.cancel();
                record.completion
            })
            .collect()
    }
}
impl Drop for Tasks {
    fn drop(&mut self) {
        for record in self.records.values() {
            record.control.cancel();
        }
    }
}
impl Runtime {
    /// Installs execution and a completion wakeup once. Creates no worker threads
    /// itself; native Application supplies defaults, headless/custom hosts choose adapters.
    pub fn configure_tasks(
        &mut self,
        executor: Arc<dyn TaskExecutor>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<(), SpawnError> {
        let mut slot = self.inner.tasks.borrow_mut();
        if slot.is_some() {
            return Err(SpawnError::AlreadyConfigured);
        }
        *slot = Some(Tasks::new(executor, wake));
        Ok(())
    }
    /// Delivers ready task results in fresh UI updates, with no active entity lease.
    /// Up to 1,024 queue records are consumed; the adapter is woken again if that
    /// budget is reached. A callback panic does not discard later queued results.
    pub fn poll_tasks(&mut self) -> usize {
        self.inner.synchronize();
        if let Some(tasks) = self.inner.tasks.borrow().as_ref() {
            tasks.hub.pending.store(false, Ordering::Release);
        }
        let mut delivered = 0;
        for _ in 0..1024 {
            let next = {
                let mut slot = self.inner.tasks.borrow_mut();
                let Some(tasks) = slot.as_mut() else {
                    return delivered;
                };
                let Ok(packet) = tasks.receiver.try_recv() else {
                    return delivered;
                };
                (tasks.records.remove(&packet.id), packet.outcome)
            };
            if let (Some(record), Some(outcome)) = next
                && record
                    .control
                    .status
                    .compare_exchange(RUNNING, FINISHED, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                delivered += 1;
                (record.completion)(
                    outcome,
                    &mut AppContext {
                        runtime: &self.inner,
                        dispatch_mount: None,
                    },
                );
            }
            self.inner.synchronize();
        }
        if let Some(tasks) = self.inner.tasks.borrow().as_ref() {
            tasks.hub.request_wake();
        }
        delivered
    }
    /// Number of registered tasks, including results awaiting delivery.
    pub fn pending_tasks(&self) -> usize {
        self.inner
            .tasks
            .borrow()
            .as_ref()
            .map_or(0, |tasks| tasks.records.len())
    }
}

fn panic_error(payload: Box<dyn Any + Send>) -> TaskError {
    TaskError::Panicked(
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).into()))
            .unwrap_or_else(|| "non-string panic payload".into()),
    )
}
pub(crate) fn start<R: Send + 'static>(
    runtime: &std::rc::Rc<RuntimeInner>,
    owner: Option<EntityId>,
    completion: impl FnOnce(TaskResult<R>, &mut AppContext<'_>) + 'static,
    work: Work<R>,
) -> Result<Task, SpawnError> {
    let (executor, hub, control, registration) = {
        let mut slot = runtime.tasks.borrow_mut();
        let tasks = slot.as_mut().ok_or(SpawnError::Unavailable)?;
        let id = tasks.next;
        tasks.next = tasks
            .next
            .checked_add(1)
            .expect("RXUI task identity exhausted");
        let (abort, registration) = AbortHandle::new_pair();
        let control = Arc::new(Control {
            id,
            status: AtomicU8::new(RUNNING),
            abort,
            hub: Arc::downgrade(&tasks.hub),
        });
        tasks.records.insert(
            id,
            Record {
                owner,
                control: control.clone(),
                completion: Box::new(move |outcome, cx| {
                    completion(
                        outcome.map(|value| *value.downcast::<R>().expect("typed task output")),
                        cx,
                    );
                }),
            },
        );
        (
            tasks.executor.clone(),
            tasks.hub.clone(),
            control,
            registration,
        )
    };
    let id = control.id;
    let mut scheduling = Scheduling {
        runtime,
        control: control.clone(),
        accepted: false,
    };
    let scheduled = match work {
        Work::Future(future) => executor.spawn(Box::pin(async move {
            if let Ok(outcome) =
                Abortable::new(AssertUnwindSafe(future).catch_unwind(), registration).await
            {
                hub.send(Packet {
                    id,
                    outcome: Some(
                        outcome
                            .map(|value| Box::new(value) as Box<dyn Any + Send>)
                            .map_err(panic_error),
                    ),
                });
            }
        })),
        Work::Blocking(job) => {
            let job_control = control.clone();
            executor.spawn_blocking(Box::new(move || {
                if job_control.status.load(Ordering::Acquire) != RUNNING {
                    return;
                }
                let outcome = std::panic::catch_unwind(AssertUnwindSafe(job))
                    .map(|value| Box::new(value) as Box<dyn Any + Send>)
                    .map_err(panic_error);
                hub.send(Packet {
                    id,
                    outcome: Some(outcome),
                });
            }))
        }
    };
    scheduled?;
    scheduling.accepted = true;
    Ok(Task {
        control,
        cancel_on_drop: true,
    })
}
pub(crate) enum Work<R> {
    Future(Pin<Box<dyn Future<Output = R> + Send>>),
    Blocking(Box<dyn FnOnce() -> R + Send>),
}
struct Scheduling<'a> {
    runtime: &'a std::rc::Rc<RuntimeInner>,
    control: Arc<Control>,
    accepted: bool,
}
impl Drop for Scheduling<'_> {
    fn drop(&mut self) {
        if !self.accepted {
            self.control.cancel();
            let record = self
                .runtime
                .tasks
                .borrow_mut()
                .as_mut()
                .and_then(|tasks| tasks.records.remove(&self.control.id));
            drop(record);
        }
    }
}

/// Asynchronous desktop delay driven by the timer reactor, without UI polling.
#[cfg(not(target_arch = "wasm32"))]
pub async fn sleep(duration: Duration) {
    async_io::Timer::after(duration).await;
}

/// Fixed-size desktop worker pools: async polling and blocking work use separate
/// threads. Shutdown signals workers without waiting on arbitrary user work.
#[cfg(not(target_arch = "wasm32"))]
pub struct ThreadPoolExecutor {
    executor: Arc<async_executor::Executor<'static>>,
    shutdown: async_channel::Sender<()>,
    blocking: async_channel::Sender<BlockingJob>,
    stopping: Arc<AtomicBool>,
    _workers: Vec<std::thread::JoinHandle<()>>,
}
#[cfg(not(target_arch = "wasm32"))]
impl ThreadPoolExecutor {
    /// Creates a fixed number of async and blocking workers; both must be nonzero.
    pub fn new(async_workers: usize, blocking_workers: usize) -> Result<Self, std::io::Error> {
        if async_workers == 0 || blocking_workers == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "worker counts must be positive",
            ));
        }
        let executor = Arc::new(async_executor::Executor::new());
        let (shutdown, stop) = async_channel::unbounded::<()>();
        let (blocking, jobs) = async_channel::unbounded::<BlockingJob>();
        let stopping = Arc::new(AtomicBool::new(false));
        let mut pool = Self {
            executor: executor.clone(),
            shutdown,
            blocking,
            stopping: stopping.clone(),
            _workers: Vec::new(),
        };
        for i in 0..async_workers {
            let executor = executor.clone();
            let stop = stop.clone();
            pool._workers.push(
                std::thread::Builder::new()
                    .name(format!("rxui-async-{i}"))
                    .spawn(move || {
                        let _ = futures_lite::future::block_on(executor.run(stop.recv()));
                    })?,
            );
        }
        for i in 0..blocking_workers {
            let jobs = jobs.clone();
            let stopping = stopping.clone();
            pool._workers.push(
                std::thread::Builder::new()
                    .name(format!("rxui-blocking-{i}"))
                    .spawn(move || {
                        while let Ok(job) = futures_lite::future::block_on(jobs.recv()) {
                            if stopping.load(Ordering::Acquire) {
                                break;
                            }
                            job();
                        }
                    })?,
            );
        }
        Ok(pool)
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl TaskExecutor for ThreadPoolExecutor {
    fn spawn(&self, future: BackgroundFuture) -> Result<(), SpawnError> {
        self.executor.spawn(future).detach();
        Ok(())
    }
    fn spawn_blocking(&self, job: BlockingJob) -> Result<(), SpawnError> {
        self.blocking
            .try_send(job)
            .map_err(|_| SpawnError::Rejected("blocking pool is closed".into()))
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Drop for ThreadPoolExecutor {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        self.shutdown.close();
        self.blocking.close();
    }
}
