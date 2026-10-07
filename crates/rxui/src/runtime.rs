use crate::{
    AccessError, AppContext, Context, EffectCycle, Entity, EntityId, MountId, ViewContext,
    entity::EntityCell, id::next_runtime,
};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    rc::{Rc, Weak},
};

type EffectFn = dyn FnOnce(&mut AppContext<'_>);
type ObserverFn = dyn Fn(&mut AppContext<'_>);
enum Effect {
    Deferred(Box<EffectFn>),
    Changed(EntityId),
    Observer(Weak<Observer>),
}
pub(crate) enum Release {
    Entity(EntityId),
    Mount(MountId),
    Subscription(EntityId, u64),
}
struct Slot {
    generation: u64,
    value: Option<Weak<dyn Any>>,
    revision: u64,
}
pub(crate) struct MountRecord {
    life: Weak<MountLife>,
    pub(crate) dependencies: HashSet<EntityId>,
    pub(crate) scratch: HashSet<EntityId>,
    #[cfg(feature = "layout")]
    scroll_reads: Option<Box<crate::scrolling::ScrollReads>>,
    dirty: bool,
    evaluating: bool,
}
pub(crate) struct Bookkeeping {
    slots: Vec<Slot>,
    free: Vec<usize>,
    next_mount: u64,
    next_observer: u64,
    pub(crate) mounts: HashMap<MountId, MountRecord>,
    dependents: HashMap<EntityId, HashSet<MountId>>,
    observers: HashMap<EntityId, Vec<(u64, Weak<Observer>)>>,
    scheduled: HashSet<EntityId>,
    effects: VecDeque<Effect>,
}
pub(crate) struct RuntimeInner {
    pub(crate) id: u64,
    pub(crate) state: RefCell<Bookkeeping>,
    pub(crate) releases: RefCell<Vec<Release>>,
    flushing: Cell<bool>,
    #[cfg(feature = "tasks")]
    pub(crate) tasks: RefCell<Option<crate::tasks::Tasks>>,
    #[cfg(feature = "native")]
    pub(crate) native: RefCell<Option<std::rc::Rc<crate::native::Commands>>>,
}

/// Headless, single-UI-thread state runtime. It owns metadata, not strong entities.
/// Mutations invalidate immediately; effect flushing is explicit so a future
/// host can batch all input before evaluating/presenting. Native/GPU work is absent.
pub struct Runtime {
    pub(crate) inner: Rc<RuntimeInner>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}
impl Runtime {
    /// Creates an independent runtime identity and empty state graph.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RuntimeInner {
                id: next_runtime(),
                state: RefCell::new(Bookkeeping {
                    slots: Vec::new(),
                    free: Vec::new(),
                    next_mount: 1,
                    next_observer: 1,
                    mounts: HashMap::new(),
                    dependents: HashMap::new(),
                    observers: HashMap::new(),
                    scheduled: HashSet::new(),
                    effects: VecDeque::new(),
                }),
                releases: RefCell::new(Vec::new()),
                flushing: Cell::new(false),
                #[cfg(feature = "tasks")]
                tasks: RefCell::new(None),
                #[cfg(feature = "native")]
                native: RefCell::new(None),
            }),
        }
    }
    /// Executes synchronous application work, retaining deferred notifications for
    /// flush. Multiple entity updates coalesce pending source notifications.
    /// The callback result is ordinary application data, with no rollback semantics.
    pub fn update<R>(&mut self, f: impl FnOnce(&mut AppContext<'_>) -> R) -> R {
        self.inner.synchronize();
        let result = f(&mut AppContext {
            runtime: &self.inner,
            dispatch_mount: None,
        });
        self.inner.synchronize();
        result
    }
    /// Evaluates a mounted owner with tracked reads and weak listener binding.
    /// A normally returned description commits its dependency set and clears dirty.
    /// Panic/access failure preserves the prior dependencies and leaves it dirty.
    /// The callback receives read-only state, and no mutable runtime capability.
    pub fn evaluate<T: 'static, R>(
        &mut self,
        mount: &Mount<T>,
        f: impl FnOnce(&T, &mut ViewContext<'_, T>) -> R,
    ) -> Result<R, AccessError> {
        self.evaluate_checked(mount, |state, cx| Ok(f(state, cx)))
    }
    pub(crate) fn evaluate_checked<T: 'static, R, E: From<AccessError>>(
        &mut self,
        mount: &Mount<T>,
        f: impl FnOnce(&T, &mut ViewContext<'_, T>) -> Result<R, E>,
    ) -> Result<R, E> {
        self.inner.synchronize();
        self.inner.validate(mount.owner.id())?;
        let scratch = {
            let mut state = self.inner.state.borrow_mut();
            let record = state
                .mounts
                .get_mut(&mount.id())
                .ok_or(AccessError::Disposed)?;
            if record.evaluating {
                return Err(AccessError::Evaluating.into());
            }
            record.evaluating = true;
            record.dirty = true;
            std::mem::take(&mut record.scratch)
        };
        let evaluation = EvaluationLease {
            runtime: &self.inner,
            mount: mount.id(),
        };
        let mut cx = ViewContext::new(
            &self.inner,
            mount.owner.downgrade(),
            Rc::downgrade(&mount.life),
            mount.id(),
            scratch,
        );
        let value = mount
            .owner
            .cell
            .value
            .try_borrow()
            .map_err(|_| AccessError::Borrowed)?;
        let value_ref = value.as_ref().ok_or(AccessError::Borrowed)?;
        let result = f(value_ref, &mut cx)?;
        cx.commit();
        drop(value);
        drop(cx);
        drop(evaluation);
        self.inner.synchronize();
        Ok(result)
    }
    /// Checks a live mount's dirty flag in O(1), without allocating a collection.
    pub fn is_dirty<T>(&self, mount: &Mount<T>) -> Result<bool, AccessError> {
        if mount.id().runtime != self.inner.id {
            return Err(AccessError::WrongRuntime);
        }
        self.inner.synchronize();
        self.inner
            .state
            .borrow()
            .mounts
            .get(&mount.id())
            .map(|r| r.dirty)
            .ok_or(AccessError::Disposed)
    }
    /// Collects live dirty mount identities for a host's evaluation phase.
    pub fn dirty_mounts(&self) -> Vec<MountId> {
        self.inner.synchronize();
        self.inner
            .state
            .borrow()
            .mounts
            .iter()
            .filter(|(_, r)| r.dirty && r.life.strong_count() != 0)
            .map(|(id, _)| *id)
            .collect()
    }
    /// Monotonic entity update revision, useful for retained preparation caches.
    /// It increments once per update scope, not once per changed field.
    pub fn revision<T>(&self, entity: &Entity<T>) -> Result<u64, AccessError> {
        self.inner.validate(entity.id())?;
        Ok(self.inner.state.borrow().slots[entity.id().slot].revision)
    }
    /// Runs deferred effects/observers after prior update scopes have ended.
    /// A bounded callback budget stops notification cycles. Already performed
    /// mutations remain; remaining queued work can be inspected/retried or cleared.
    pub fn flush(&mut self) -> Result<(), EffectCycle> {
        self.flush_with_limit(1024)
    }
    /// Selects the maximum number of callbacks executed by this flush.
    /// Zero permits cleanup and empty queues but cannot execute pending callbacks.
    pub fn flush_with_limit(&mut self, limit: usize) -> Result<(), EffectCycle> {
        self.inner.synchronize();
        if self.inner.flushing.replace(true) {
            return Err(EffectCycle);
        }
        let flushing = FlushLease(&self.inner.flushing);
        let mut executed = 0;
        loop {
            // Notification expansion is internal; each live observer becomes one
            // queued callback so budget exhaustion never discards later observers.
            let next = self.inner.state.borrow_mut().effects.pop_front();
            let Some(effect) = next else { break };
            match effect {
                Effect::Changed(source) => {
                    let mut state = self.inner.state.borrow_mut();
                    let Bookkeeping {
                        scheduled,
                        observers,
                        effects,
                        ..
                    } = &mut *state;
                    scheduled.remove(&source);
                    if let Some(observers) = observers.get(&source) {
                        for (_, weak) in observers.iter().rev() {
                            effects.push_front(Effect::Observer(weak.clone()));
                        }
                    }
                }
                Effect::Observer(weak) => {
                    if let Some(observer) = weak.upgrade() {
                        if executed == limit {
                            self.inner
                                .state
                                .borrow_mut()
                                .effects
                                .push_front(Effect::Observer(weak));
                            return Err(EffectCycle);
                        }
                        executed += 1;
                        (observer.callback)(&mut AppContext {
                            runtime: &self.inner,
                            dispatch_mount: None,
                        });
                    }
                }
                Effect::Deferred(callback) => {
                    if executed == limit {
                        self.inner
                            .state
                            .borrow_mut()
                            .effects
                            .push_front(Effect::Deferred(callback));
                        return Err(EffectCycle);
                    }
                    executed += 1;
                    callback(&mut AppContext {
                        runtime: &self.inner,
                        dispatch_mount: None,
                    });
                }
            }
            self.inner.synchronize();
        }
        drop(flushing);
        Ok(())
    }
    /// Discards pending effects/notifications while keeping state and dirty mounts.
    /// Useful after a diagnosed observer cycle. It does not undo mutations.
    pub fn clear_effects(&mut self) {
        let effects = {
            let mut state = self.inner.state.borrow_mut();
            state.scheduled.clear();
            std::mem::take(&mut state.effects)
        };
        // User captures may dispose entities/subscriptions; drop outside metadata borrows.
        drop(effects);
        self.inner.synchronize();
    }
    /// Processes queued entity/mount/subscription disposal without invoking callbacks.
    pub fn synchronize(&mut self) {
        self.inner.synchronize();
    }
}

/// Strong identity for one mounted instance. Clones refer to the same mount;
/// separately mounting the same entity creates an independent instance.
pub struct Mount<T: 'static> {
    pub(crate) owner: Entity<T>,
    pub(crate) life: Rc<MountLife>,
}
impl<T> Clone for Mount<T> {
    fn clone(&self) -> Self {
        Self {
            owner: self.owner.clone(),
            life: self.life.clone(),
        }
    }
}
impl<T> fmt::Debug for Mount<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Mount").field(&self.id()).finish()
    }
}
impl<T> Mount<T> {
    /// Placement identity; independent of the owner's identity.
    pub fn id(&self) -> MountId {
        self.life.id
    }
    /// Persistent state retained by this mount.
    pub fn entity(&self) -> &Entity<T> {
        &self.owner
    }
}
pub(crate) struct MountLife {
    pub(crate) id: MountId,
    runtime: Weak<RuntimeInner>,
}
impl Drop for MountLife {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.releases.borrow_mut().push(Release::Mount(self.id));
        }
    }
}
struct Observer {
    source: EntityId,
    serial: u64,
    runtime: Weak<RuntimeInner>,
    callback: Box<ObserverFn>,
}
impl Drop for Observer {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime
                .releases
                .borrow_mut()
                .push(Release::Subscription(self.source, self.serial));
        }
    }
}
/// Scoped change observer. Dropping its last handle removes it; sources/runtimes
/// are not retained. Typed Context::observe also weakly binds the owner.
#[must_use = "dropping the subscription immediately removes its observer"]
pub struct Subscription {
    observer: Rc<Observer>,
}
impl Clone for Subscription {
    fn clone(&self) -> Self {
        Self {
            observer: self.observer.clone(),
        }
    }
}
impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscription")
            .field("source", &self.observer.source)
            .finish_non_exhaustive()
    }
}

impl RuntimeInner {
    pub(crate) fn validate(&self, id: EntityId) -> Result<(), AccessError> {
        if id.runtime != self.id {
            return Err(AccessError::WrongRuntime);
        }
        let state = self.state.borrow();
        let slot = state.slots.get(id.slot).ok_or(AccessError::Disposed)?;
        if slot.generation != id.generation
            || slot.value.as_ref().is_none_or(|v| v.strong_count() == 0)
        {
            return Err(AccessError::Disposed);
        }
        Ok(())
    }
    pub(crate) fn create<T: 'static>(
        self: &Rc<Self>,
        init: impl FnOnce(&mut Context<'_, T>) -> T,
    ) -> Entity<T> {
        self.synchronize();
        let id = {
            let mut state = self.state.borrow_mut();
            let slot = if let Some(index) = state.free.pop() {
                index
            } else {
                let index = state.slots.len();
                state.slots.push(Slot {
                    generation: 1,
                    value: None,
                    revision: 0,
                });
                index
            };
            EntityId {
                runtime: self.id,
                slot,
                generation: state.slots[slot].generation,
            }
        };
        let entity = Entity {
            cell: Rc::new(EntityCell {
                id,
                runtime: Rc::downgrade(self),
                value: RefCell::new(None),
                valid: Cell::new(true),
            }),
        };
        let erased: Rc<dyn Any> = entity.cell.clone();
        self.state.borrow_mut().slots[id.slot].value = Some(Rc::downgrade(&erased));
        drop(erased);
        let mut initialization = InitializationLease {
            runtime: self,
            cell: &entity.cell,
            initialized: false,
        };
        let value = init(&mut Context::new(self, entity.downgrade()));
        *entity.cell.value.borrow_mut() = Some(value);
        initialization.initialized = true;
        drop(initialization);
        entity
    }
    pub(crate) fn mount<T: 'static>(
        self: &Rc<Self>,
        owner: &Entity<T>,
    ) -> Result<Mount<T>, AccessError> {
        self.validate(owner.id())?;
        self.synchronize();
        let id = {
            let mut state = self.state.borrow_mut();
            let serial = state.next_mount;
            state.next_mount = serial
                .checked_add(1)
                .expect("RXUI mount identity space exhausted");
            MountId {
                runtime: self.id,
                serial,
            }
        };
        let life = Rc::new(MountLife {
            id,
            runtime: Rc::downgrade(self),
        });
        self.state.borrow_mut().mounts.insert(
            id,
            MountRecord {
                life: Rc::downgrade(&life),
                dependencies: HashSet::new(),
                scratch: HashSet::new(),
                #[cfg(feature = "layout")]
                scroll_reads: None,
                dirty: true,
                evaluating: false,
            },
        );
        Ok(Mount {
            owner: owner.clone(),
            life,
        })
    }
    #[cfg(feature = "layout")]
    pub(crate) fn invalidate_mount(&self, id: MountId) {
        if let Some(record) = self.state.borrow_mut().mounts.get_mut(&id) {
            record.dirty = true;
        }
    }
    pub(crate) fn mount_live(&self, id: MountId) -> bool {
        id.runtime == self.id
            && self
                .state
                .borrow()
                .mounts
                .get(&id)
                .is_some_and(|r| r.life.strong_count() != 0)
    }
    pub(crate) fn changed(&self, source: EntityId) {
        let mut state = self.state.borrow_mut();
        state.slots[source.slot].revision = state.slots[source.slot].revision.saturating_add(1);
        let Bookkeeping {
            mounts, dependents, ..
        } = &mut *state;
        if let Some(ids) = dependents.get(&source) {
            for id in ids {
                if let Some(record) = mounts.get_mut(id) {
                    record.dirty = true;
                }
            }
        }
        if state.observers.contains_key(&source) && state.scheduled.insert(source) {
            state.effects.push_back(Effect::Changed(source));
        }
    }
    pub(crate) fn defer(&self, f: impl FnOnce(&mut AppContext<'_>) + 'static) {
        self.state
            .borrow_mut()
            .effects
            .push_back(Effect::Deferred(Box::new(f)));
    }
    pub(crate) fn observe<T: 'static>(
        self: &Rc<Self>,
        source: &Entity<T>,
        callback: impl Fn(&Entity<T>, &mut AppContext<'_>) + 'static,
    ) -> Result<Subscription, AccessError> {
        self.validate(source.id())?;
        self.synchronize();
        let serial = {
            let mut state = self.state.borrow_mut();
            let id = state.next_observer;
            state.next_observer = id
                .checked_add(1)
                .expect("RXUI observer identity space exhausted");
            id
        };
        let weak = source.downgrade();
        let observer = Rc::new(Observer {
            source: source.id(),
            serial,
            runtime: Rc::downgrade(self),
            callback: Box::new(move |cx| {
                if let Some(source) = weak.upgrade() {
                    callback(&source, cx);
                }
            }),
        });
        self.state
            .borrow_mut()
            .observers
            .entry(source.id())
            .or_default()
            .push((serial, Rc::downgrade(&observer)));
        Ok(Subscription { observer })
    }
    #[cfg(feature = "layout")]
    pub(crate) fn commit_scroll_reads(&self, mount: MountId, handles: Vec<crate::ScrollHandle>) {
        let old = self
            .state
            .borrow_mut()
            .mounts
            .get_mut(&mount)
            .expect("evaluating mount")
            .scroll_reads
            .take();
        drop(old);
        if !handles.is_empty() {
            let reads = crate::scrolling::ScrollReads::new(mount, handles);
            self.state
                .borrow_mut()
                .mounts
                .get_mut(&mount)
                .expect("evaluating mount")
                .scroll_reads = Some(Box::new(reads));
        }
    }
    pub(crate) fn commit_dependencies(&self, mount: MountId, next: HashSet<EntityId>) {
        let mut state = self.state.borrow_mut();
        let mut old = std::mem::take(
            &mut state
                .mounts
                .get_mut(&mount)
                .expect("evaluating mount")
                .dependencies,
        );
        for id in old.difference(&next) {
            if let Some(ids) = state.dependents.get_mut(id) {
                ids.remove(&mount);
                if ids.is_empty() {
                    state.dependents.remove(id);
                }
            }
        }
        for id in next.difference(&old) {
            state.dependents.entry(*id).or_default().insert(mount);
        }
        old.clear();
        let record = state.mounts.get_mut(&mount).expect("evaluating mount");
        record.dependencies = next;
        record.scratch = old;
        record.dirty = false;
    }
    pub(crate) fn synchronize(&self) {
        let releases = std::mem::take(&mut *self.releases.borrow_mut());
        if releases.is_empty() {
            return;
        }
        let mut state = self.state.borrow_mut();
        #[cfg(feature = "tasks")]
        let mut disposed_owners = Vec::new();
        for release in releases {
            match release {
                Release::Entity(id) => {
                    let Some(slot) = state.slots.get_mut(id.slot) else {
                        continue;
                    };
                    if slot.generation != id.generation
                        || slot.value.as_ref().is_some_and(|v| v.strong_count() != 0)
                    {
                        continue;
                    }
                    slot.value = None;
                    slot.revision = 0;
                    if let Some(generation) = slot.generation.checked_add(1) {
                        slot.generation = generation;
                        state.free.push(id.slot);
                    }
                    if let Some(ids) = state.dependents.remove(&id) {
                        for mount in ids {
                            if let Some(r) = state.mounts.get_mut(&mount) {
                                r.dependencies.remove(&id);
                                r.dirty = true;
                            }
                        }
                    }
                    state.observers.remove(&id);
                    #[cfg(feature = "tasks")]
                    disposed_owners.push(id);
                }
                Release::Mount(id) => {
                    if let Some(record) = state.mounts.remove(&id) {
                        for source in record.dependencies {
                            if let Some(ids) = state.dependents.get_mut(&source) {
                                ids.remove(&id);
                                if ids.is_empty() {
                                    state.dependents.remove(&source);
                                }
                            }
                        }
                    }
                }
                Release::Subscription(source, serial) => {
                    if let Some(observers) = state.observers.get_mut(&source) {
                        observers.retain(|(id, _)| *id != serial);
                        if observers.is_empty() {
                            state.observers.remove(&source);
                        }
                    }
                }
            }
        }
        drop(state);
        #[cfg(feature = "tasks")]
        {
            let callbacks = {
                let mut slot = self.tasks.borrow_mut();
                if let Some(tasks) = slot.as_mut() {
                    disposed_owners
                        .into_iter()
                        .flat_map(|id| tasks.cancel_owner(id))
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                }
            };
            drop(callbacks);
        }
    }
}
struct EvaluationLease<'a> {
    runtime: &'a Rc<RuntimeInner>,
    mount: MountId,
}
impl Drop for EvaluationLease<'_> {
    fn drop(&mut self) {
        if let Some(r) = self.runtime.state.borrow_mut().mounts.get_mut(&self.mount) {
            r.evaluating = false;
        }
    }
}
struct FlushLease<'a>(&'a Cell<bool>);
impl Drop for FlushLease<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

struct InitializationLease<'a, T> {
    runtime: &'a Rc<RuntimeInner>,
    cell: &'a EntityCell<T>,
    initialized: bool,
}
impl<T> Drop for InitializationLease<'_, T> {
    fn drop(&mut self) {
        if !self.initialized {
            self.cell.valid.set(false);
            self.runtime.state.borrow_mut().slots[self.cell.id.slot].value = None;
            self.runtime
                .releases
                .borrow_mut()
                .push(Release::Entity(self.cell.id));
        }
    }
}
