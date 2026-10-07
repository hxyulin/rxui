use crate::{
    AccessError, Entity, EntityId, Listener, Mount, MountId, Subscription, WeakEntity,
    runtime::{MountLife, RuntimeInner},
};
use std::{
    cell::RefCell,
    collections::HashSet,
    ops::{Deref, DerefMut},
    rc::{Rc, Weak},
};

mod sealed {
    pub trait Sealed {}
}

/// Read capability supplied by RXUI contexts. External implementations are sealed.
/// View reads register dependencies; application/update reads do not.
pub trait ReadContext: sealed::Sealed {
    /// Internal handle validation used by Entity access.
    #[doc(hidden)]
    fn validate(&self, entity: EntityId) -> Result<(), AccessError>;
    /// Internal dependency recording used by Entity reads.
    #[doc(hidden)]
    fn track(&self, entity: EntityId);
    /// Internal placement resolution for layout references.
    #[doc(hidden)]
    fn placement_scope(&self) -> Option<MountId> {
        None
    }
    /// Internal metric-read tracking, distinct from ordinary event reads.
    #[doc(hidden)]
    #[cfg(feature = "layout")]
    fn track_scroll(&self, _handle: &crate::ScrollHandle) {}
}

/// Application mutation capability, valid during Runtime::update or effect flush.
/// It creates entities/mounts and queues effects without owning a native loop.
pub struct AppContext<'a> {
    pub(crate) runtime: &'a Rc<RuntimeInner>,
    pub(crate) dispatch_mount: Option<MountId>,
}
impl sealed::Sealed for AppContext<'_> {}
impl ReadContext for AppContext<'_> {
    fn placement_scope(&self) -> Option<MountId> {
        self.dispatch_mount
    }
    fn validate(&self, entity: EntityId) -> Result<(), AccessError> {
        self.runtime.validate(entity)
    }
    fn track(&self, _entity: EntityId) {}
}
impl AppContext<'_> {
    /// Launches an application-scoped Send future. Retain its Task or detach explicitly.
    /// Panics if execution is not configured/rejected; try_spawn provides fallible setup.
    #[cfg(feature = "tasks")]
    pub fn spawn<R: Send + 'static>(
        &mut self,
        future: impl std::future::Future<Output = R> + Send + 'static,
        completion: impl FnOnce(crate::TaskResult<R>, &mut AppContext<'_>) + 'static,
    ) -> crate::Task {
        self.try_spawn(future, completion)
            .unwrap_or_else(|e| panic!("cannot spawn RXUI task: {e}"))
    }
    /// Fallible application task setup. Completion executes only at poll_tasks.
    #[cfg(feature = "tasks")]
    pub fn try_spawn<R: Send + 'static>(
        &mut self,
        future: impl std::future::Future<Output = R> + Send + 'static,
        completion: impl FnOnce(crate::TaskResult<R>, &mut AppContext<'_>) + 'static,
    ) -> Result<crate::Task, crate::SpawnError> {
        crate::tasks::start(
            self.runtime,
            None,
            completion,
            crate::tasks::Work::Future(Box::pin(future)),
        )
    }
    /// Launches application-scoped blocking work on the executor's separate pool.
    #[cfg(feature = "tasks")]
    pub fn spawn_blocking<R: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> R + Send + 'static,
        completion: impl FnOnce(crate::TaskResult<R>, &mut AppContext<'_>) + 'static,
    ) -> crate::Task {
        self.try_spawn_blocking(work, completion)
            .unwrap_or_else(|e| panic!("cannot spawn RXUI blocking task: {e}"))
    }
    /// Fallible blocking-job setup.
    #[cfg(feature = "tasks")]
    pub fn try_spawn_blocking<R: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> R + Send + 'static,
        completion: impl FnOnce(crate::TaskResult<R>, &mut AppContext<'_>) + 'static,
    ) -> Result<crate::Task, crate::SpawnError> {
        crate::tasks::start(
            self.runtime,
            None,
            completion,
            crate::tasks::Work::Blocking(Box::new(work)),
        )
    }
    /// Creates one persistent value. The typed context identifies the value before
    /// initialization completes, but attempts to read/update it then are Borrowed.
    #[expect(
        clippy::new_ret_no_self,
        reason = "cx.new is the agreed typed entity factory API"
    )]
    pub fn new<T: 'static>(&mut self, init: impl FnOnce(&mut Context<'_, T>) -> T) -> Entity<T> {
        self.runtime.create(init)
    }
    /// Creates an independent mount for this value, initially dirty.
    /// Multiple mounts can share one model while retaining independent identity.
    pub fn mount<T: 'static>(&mut self, owner: &Entity<T>) -> Result<Mount<T>, AccessError> {
        self.runtime.mount(owner)
    }
    /// Queues work for the next flush, after active state borrows have ended.
    pub fn defer(&mut self, f: impl FnOnce(&mut AppContext<'_>) + 'static) {
        self.runtime.defer(f);
    }
    /// Observes coalesced changes to a source. Keep the returned subscription alive.
    /// It retains neither the source nor runtime. Callbacks execute only at flush.
    pub fn observe<T: 'static>(
        &mut self,
        source: &Entity<T>,
        callback: impl Fn(&Entity<T>, &mut AppContext<'_>) + 'static,
    ) -> Result<Subscription, AccessError> {
        self.runtime.observe(source, callback)
    }
}

/// Typed update/initialization context for the supplied mutable state reference.
/// Dereferences to AppContext for operations on other entities.
pub struct Context<'a, T: 'static> {
    app: AppContext<'a>,
    owner: WeakEntity<T>,
}
impl<'a, T> Context<'a, T> {
    pub(crate) fn bind_dispatch_mount(&mut self, mount: Option<MountId>) {
        self.app.dispatch_mount = mount;
    }
    /// Launches an owner-scoped future with a weak completion binding to current T.
    /// Keep the Task (usually in T) or detach it explicitly. Updates never cross awaits.
    /// Owned snapshots can cross an await; borrowed component state cannot:
    ///
    /// ```compile_fail
    /// use rxui::Runtime;
    /// let mut runtime = Runtime::new();
    /// let model = runtime.update(|cx| cx.new(|_| String::from("hello")));
    /// runtime.update(|cx| model.update(cx, |state, cx| {
    ///     cx.spawn(async { state.len() }, |_, _, _| {}).detach();
    /// }));
    /// ```
    #[cfg(feature = "tasks")]
    pub fn spawn<R: Send + 'static>(
        &mut self,
        future: impl std::future::Future<Output = R> + Send + 'static,
        completion: impl FnOnce(&mut T, crate::TaskResult<R>, &mut Context<'_, T>) + 'static,
    ) -> crate::Task {
        self.try_spawn(future, completion)
            .unwrap_or_else(|e| panic!("cannot spawn RXUI task: {e}"))
    }
    /// Fallible owner-scoped future setup.
    #[cfg(feature = "tasks")]
    pub fn try_spawn<R: Send + 'static>(
        &mut self,
        future: impl std::future::Future<Output = R> + Send + 'static,
        completion: impl FnOnce(&mut T, crate::TaskResult<R>, &mut Context<'_, T>) + 'static,
    ) -> Result<crate::Task, crate::SpawnError> {
        let owner = self.owner.clone();
        crate::tasks::start(
            self.app.runtime,
            Some(owner.id()),
            move |outcome, cx| {
                if let Some(entity) = owner.upgrade() {
                    entity.update(cx, |state, cx| completion(state, outcome, cx));
                }
            },
            crate::tasks::Work::Future(Box::pin(future)),
        )
    }
    /// Launches blocking work with a weak completion binding to current T.
    #[cfg(feature = "tasks")]
    pub fn spawn_blocking<R: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> R + Send + 'static,
        completion: impl FnOnce(&mut T, crate::TaskResult<R>, &mut Context<'_, T>) + 'static,
    ) -> crate::Task {
        self.try_spawn_blocking(work, completion)
            .unwrap_or_else(|e| panic!("cannot spawn RXUI blocking task: {e}"))
    }
    /// Fallible owner-scoped blocking-job setup.
    #[cfg(feature = "tasks")]
    pub fn try_spawn_blocking<R: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> R + Send + 'static,
        completion: impl FnOnce(&mut T, crate::TaskResult<R>, &mut Context<'_, T>) + 'static,
    ) -> Result<crate::Task, crate::SpawnError> {
        let owner = self.owner.clone();
        crate::tasks::start(
            self.app.runtime,
            Some(owner.id()),
            move |outcome, cx| {
                if let Some(entity) = owner.upgrade() {
                    entity.update(cx, |state, cx| completion(state, outcome, cx));
                }
            },
            crate::tasks::Work::Blocking(Box::new(work)),
        )
    }
    pub(crate) fn new(runtime: &'a Rc<RuntimeInner>, owner: WeakEntity<T>) -> Self {
        Self {
            app: AppContext {
                runtime,
                dispatch_mount: None,
            },
            owner,
        }
    }
    /// Weak current-owner identity; use the supplied state reference for access
    /// during this scope rather than reentering the owner through this handle.
    pub fn entity(&self) -> WeakEntity<T> {
        self.owner.clone()
    }
    /// Observes another value through a weakly bound owner update callback.
    /// Store the subscription in the owner for automatic disposal with its state.
    pub fn observe<U: 'static>(
        &mut self,
        source: &Entity<U>,
        callback: impl Fn(&mut T, &Entity<U>, &mut Context<'_, T>) + 'static,
    ) -> Result<Subscription, AccessError> {
        let owner = self.owner.clone();
        self.app.observe(source, move |source, cx| {
            if let Some(entity) = owner.upgrade() {
                entity.update(cx, |state, cx| callback(state, source, cx));
            }
        })
    }
}
impl<'a, T> Deref for Context<'a, T> {
    type Target = AppContext<'a>;
    fn deref(&self) -> &Self::Target {
        &self.app
    }
}
impl<T> DerefMut for Context<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.app
    }
}
impl<T> sealed::Sealed for Context<'_, T> {}
impl<T> ReadContext for Context<'_, T> {
    fn placement_scope(&self) -> Option<MountId> {
        self.app.dispatch_mount
    }
    fn validate(&self, entity: EntityId) -> Result<(), AccessError> {
        self.app.validate(entity)
    }
    fn track(&self, _entity: EntityId) {}
}

/// Read-only evaluation capability for one mounted owner.
/// It binds weak listeners and records entity dependencies. It deliberately has
/// no application-mutation Deref, so evaluation cannot update state through it.
pub struct ViewContext<'a, T: 'static> {
    pub(crate) runtime: &'a Rc<RuntimeInner>,
    owner: WeakEntity<T>,
    mount: Weak<MountLife>,
    id: MountId,
    pub(crate) dependencies: RefCell<HashSet<EntityId>>,
    #[cfg(feature = "layout")]
    scroll_dependencies: RefCell<Vec<crate::ScrollHandle>>,
    committed: bool,
}
impl<T> sealed::Sealed for ViewContext<'_, T> {}
impl<T> ReadContext for ViewContext<'_, T> {
    fn placement_scope(&self) -> Option<MountId> {
        Some(self.id)
    }
    #[cfg(feature = "layout")]
    fn track_scroll(&self, handle: &crate::ScrollHandle) {
        let mut reads = self.scroll_dependencies.borrow_mut();
        if !reads.iter().any(|h| h.id() == handle.id()) {
            reads.push(handle.clone());
        }
    }
    fn validate(&self, entity: EntityId) -> Result<(), AccessError> {
        self.runtime.validate(entity)
    }
    fn track(&self, entity: EntityId) {
        self.dependencies.borrow_mut().insert(entity);
    }
}
impl<'a, T> ViewContext<'a, T> {
    pub(crate) fn new(
        runtime: &'a Rc<RuntimeInner>,
        owner: WeakEntity<T>,
        mount: Weak<MountLife>,
        id: MountId,
        mut dependencies: HashSet<EntityId>,
    ) -> Self {
        dependencies.clear();
        dependencies.insert(owner.id());
        Self {
            runtime,
            owner,
            mount,
            id,
            dependencies: RefCell::new(dependencies),
            #[cfg(feature = "layout")]
            scroll_dependencies: RefCell::new(Vec::new()),
            committed: false,
        }
    }
    /// Current placement identity, distinct from the owner's EntityId.
    pub fn mount_id(&self) -> MountId {
        self.id
    }
    /// Binds a reusable event callback to the live owner and this mount.
    /// Events access current state through &mut T and automatically invalidate it.
    /// This first milestone checks mount lifetime; element routing is later work.
    pub fn listener<E: 'static>(
        &self,
        callback: impl Fn(&mut T, &E, &mut Context<'_, T>) + 'static,
    ) -> Listener<E> {
        Listener::bound(self.owner.clone(), self.mount.clone(), callback)
    }
    pub(crate) fn commit(&mut self) {
        let next = std::mem::take(self.dependencies.get_mut());
        self.runtime.commit_dependencies(self.id, next);
        #[cfg(feature = "layout")]
        self.runtime
            .commit_scroll_reads(self.id, std::mem::take(self.scroll_dependencies.get_mut()));
        self.committed = true;
    }
}
impl<T> Drop for ViewContext<'_, T> {
    fn drop(&mut self) {
        if !self.committed {
            let mut scratch = std::mem::take(self.dependencies.get_mut());
            scratch.clear();
            if let Some(record) = self.runtime.state.borrow_mut().mounts.get_mut(&self.id) {
                record.scratch = scratch;
            }
        }
    }
}
