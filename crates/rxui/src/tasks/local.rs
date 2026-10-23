use super::{
    CANCELLED, Delivery, FINISHED, RUNNING, SpawnError, TaskResult, TaskScope, panic_error,
};
use crate::{AppContext, EntityId, Runtime, runtime::RuntimeInner};
use futures_util::{
    FutureExt,
    future::{AbortHandle, Abortable},
};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    rc::{Rc, Weak},
};

/// Owned thread-local future. Its captures may be non-Send on every platform.
pub type LocalFuture = Pin<Box<dyn Future<Output = ()> + 'static>>;
/// Host-provided execution on the runtime's thread, on native and wasm alike.
/// Drive accepted futures independently of RXUI completion polling. Rejected
/// futures must be dropped without starting or retaining them. Do not poll UI
/// completions inline from this method or from the host wake callback.
pub trait LocalTaskExecutor: 'static {
    /// Schedules local work without blocking the UI thread.
    fn spawn_local(&self, future: LocalFuture) -> Result<(), SpawnError>;
}
type Outcome = TaskResult<Box<dyn Any>>;
struct Packet {
    id: u64,
    outcome: Option<Outcome>,
}
pub(super) struct Hub {
    queue: RefCell<VecDeque<Packet>>,
    pub(super) pending: Cell<bool>,
    waking: Cell<bool>,
    wake: Box<dyn Fn()>,
}
impl Hub {
    fn send(&self, packet: Packet) {
        self.queue.borrow_mut().push_back(packet);
        self.request_wake();
    }
    pub(super) fn request_wake(&self) {
        if !self.pending.replace(true) && !self.waking.replace(true) {
            struct Reset<'a>(&'a Cell<bool>);
            impl Drop for Reset<'_> {
                fn drop(&mut self) {
                    self.0.set(false);
                }
            }
            let _reset = Reset(&self.waking);
            (self.wake)();
        }
    }
}
struct Control {
    id: u64,
    status: Cell<u8>,
    abort: AbortHandle,
    hub: Weak<Hub>,
}
impl Control {
    fn cancel(&self) -> bool {
        if self.status.get() != RUNNING {
            return false;
        }
        self.status.set(CANCELLED);
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
/// Thread-local task handle. Drop cancels, including queued success or error.
/// Detaching retains neither owner nor runtime; disposal still cancels the work.
/// Aborted future captures are released when the host next drives its executor.
/// This handle intentionally stays on the runtime thread:
///
/// ```compile_fail
/// fn require_send<T: Send>() {}
/// require_send::<rxui::LocalTask>();
/// ```
#[must_use = "keep the local task handle alive, or call detach explicitly"]
pub struct LocalTask {
    control: Rc<Control>,
    cancel_on_drop: bool,
}
impl LocalTask {
    /// Cancels once and suppresses delivery even after execution finished.
    pub fn cancel(&self) -> bool {
        self.control.cancel()
    }
    /// Whether cancelled or claimed for UI delivery.
    pub fn is_finished(&self) -> bool {
        self.control.status.get() != RUNNING
    }
    /// Keeps work alive until its owner, mount, or runtime ends.
    pub fn detach(mut self) {
        self.cancel_on_drop = false;
    }
}
impl Drop for LocalTask {
    fn drop(&mut self) {
        if self.cancel_on_drop {
            self.control.cancel();
        }
    }
}
impl fmt::Debug for LocalTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalTask")
            .field("finished", &self.is_finished())
            .finish_non_exhaustive()
    }
}
type Completion = dyn FnOnce(Outcome, &mut AppContext<'_>);
struct Record {
    owner: Option<EntityId>,
    mount: Option<Weak<crate::runtime::MountLife>>,
    control: Rc<Control>,
    completion: Box<Completion>,
}
pub(crate) struct LocalTasks {
    executor: Rc<dyn LocalTaskExecutor>,
    pub(super) hub: Rc<Hub>,
    records: HashMap<u64, Record>,
    next: u64,
}
impl LocalTasks {
    pub(super) fn len(&self) -> usize {
        self.records.len()
    }
    pub(crate) fn cancel_disposed(&mut self, owners: &HashSet<EntityId>) -> Vec<Box<Completion>> {
        let ids: Vec<_> = self
            .records
            .iter()
            .filter(|(_, r)| {
                r.owner.is_some_and(|id| owners.contains(&id))
                    || r.mount.as_ref().is_some_and(|m| m.strong_count() == 0)
            })
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.records.remove(&id))
            .map(|r| {
                r.control.cancel();
                r.completion
            })
            .collect()
    }
}
impl Drop for LocalTasks {
    fn drop(&mut self) {
        for record in self.records.values() {
            record.control.cancel();
        }
    }
}
impl Runtime {
    /// Installs a local executor and non-Send completion notification once,
    /// independently of background configuration. The wake callback must schedule
    /// a later host turn, never run poll_tasks inline. This does not drive the
    /// executor. Application does not automatically configure local execution.
    pub fn configure_local_tasks(
        &mut self,
        executor: Rc<dyn LocalTaskExecutor>,
        wake: impl Fn() + 'static,
    ) -> Result<(), SpawnError> {
        let mut slot = self.inner.local_tasks.borrow_mut();
        if slot.is_some() {
            return Err(SpawnError::AlreadyConfigured);
        }
        *slot = Some(LocalTasks {
            executor,
            hub: Rc::new(Hub {
                queue: RefCell::new(VecDeque::new()),
                pending: Cell::new(false),
                waking: Cell::new(false),
                wake: Box::new(wake),
            }),
            records: HashMap::new(),
            next: 1,
        });
        Ok(())
    }
    pub(super) fn take_local_completion(&self) -> Option<Option<Box<Delivery>>> {
        let (record, outcome) = {
            let mut slot = self.inner.local_tasks.borrow_mut();
            let tasks = slot.as_mut()?;
            let packet = tasks.hub.queue.borrow_mut().pop_front()?;
            (tasks.records.remove(&packet.id), packet.outcome)
        };
        Some(match (record, outcome) {
            (Some(record), Some(outcome))
                if record.mount.as_ref().is_none_or(|m| m.strong_count() != 0)
                    && record.control.status.get() == RUNNING =>
            {
                record.control.status.set(FINISHED);
                Some(
                    Box::new(move |cx: &mut AppContext<'_>| (record.completion)(outcome, cx))
                        as Box<Delivery>,
                )
            }
            _ => None,
        })
    }
}
pub(crate) fn start<R: 'static>(
    runtime: &Rc<RuntimeInner>,
    owner: Option<EntityId>,
    completion: impl FnOnce(TaskResult<R>, &mut AppContext<'_>) + 'static,
    future: impl Future<Output = R> + 'static,
) -> Result<LocalTask, SpawnError> {
    start_scoped(runtime, owner, None, completion, future)
}
fn start_scoped<R: 'static>(
    runtime: &Rc<RuntimeInner>,
    owner: Option<EntityId>,
    mount: Option<Weak<crate::runtime::MountLife>>,
    completion: impl FnOnce(TaskResult<R>, &mut AppContext<'_>) + 'static,
    future: impl Future<Output = R> + 'static,
) -> Result<LocalTask, SpawnError> {
    let (executor, hub, control, registration) = {
        let mut slot = runtime.local_tasks.borrow_mut();
        let tasks = slot.as_mut().ok_or(SpawnError::Unavailable)?;
        let id = tasks.next;
        tasks.next = id
            .checked_add(1)
            .expect("RXUI local task identity exhausted");
        let (abort, registration) = AbortHandle::new_pair();
        let hub = Rc::downgrade(&tasks.hub);
        let control = Rc::new(Control {
            id,
            status: Cell::new(RUNNING),
            abort,
            hub: hub.clone(),
        });
        tasks.records.insert(
            id,
            Record {
                owner,
                mount,
                control: control.clone(),
                completion: Box::new(move |result, cx| {
                    completion(
                        result.map(|v| *v.downcast::<R>().expect("typed local task output")),
                        cx,
                    )
                }),
            },
        );
        (tasks.executor.clone(), hub, control, registration)
    };
    struct Scheduling<'a> {
        runtime: &'a Rc<RuntimeInner>,
        control: Rc<Control>,
        accepted: bool,
    }
    impl Drop for Scheduling<'_> {
        fn drop(&mut self) {
            if !self.accepted {
                self.control.cancel();
                let record = self
                    .runtime
                    .local_tasks
                    .borrow_mut()
                    .as_mut()
                    .and_then(|t| t.records.remove(&self.control.id));
                drop(record);
            }
        }
    }
    let mut guard = Scheduling {
        runtime,
        control: control.clone(),
        accepted: false,
    };
    let id = control.id;
    executor.spawn_local(Box::pin(async move {
        if let Ok(outcome) =
            Abortable::new(AssertUnwindSafe(future).catch_unwind(), registration).await
            && let Some(hub) = hub.upgrade()
        {
            hub.send(Packet {
                id,
                outcome: Some(
                    outcome
                        .map(|v| Box::new(v) as Box<dyn Any>)
                        .map_err(panic_error),
                ),
            });
        }
    }))?;
    guard.accepted = true;
    Ok(LocalTask {
        control,
        cancel_on_drop: true,
    })
}
impl TaskScope {
    /// Starts mount-scoped local work. Future and result may be non-Send.
    pub fn spawn_local<R: 'static>(
        &self,
        cx: &mut AppContext<'_>,
        future: impl Future<Output = R> + 'static,
        completion: impl FnOnce(TaskResult<R>, &mut AppContext<'_>) + 'static,
    ) -> Result<LocalTask, SpawnError> {
        if cx.runtime.id != self.runtime {
            return Err(SpawnError::WrongRuntime);
        }
        let mount = self.mount.upgrade().ok_or(SpawnError::Disposed)?;
        if !cx.runtime.mount_live(mount.id) {
            return Err(SpawnError::Disposed);
        }
        start_scoped(
            cx.runtime,
            None,
            Some(self.mount.clone()),
            completion,
            future,
        )
    }
}
