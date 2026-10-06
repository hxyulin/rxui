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
}

/// Application mutation capability, valid during Runtime::update or effect flush.
/// It creates entities/mounts and queues effects without owning a native loop.
pub struct AppContext<'a> {
    pub(crate) runtime: &'a Rc<RuntimeInner>,
}
impl sealed::Sealed for AppContext<'_> {}
impl ReadContext for AppContext<'_> {
    fn validate(&self, entity: EntityId) -> Result<(), AccessError> {
        self.runtime.validate(entity)
    }
    fn track(&self, _entity: EntityId) {}
}
impl AppContext<'_> {
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
    pub(crate) fn new(runtime: &'a Rc<RuntimeInner>, owner: WeakEntity<T>) -> Self {
        Self {
            app: AppContext { runtime },
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
    committed: bool,
}
impl<T> sealed::Sealed for ViewContext<'_, T> {}
impl<T> ReadContext for ViewContext<'_, T> {
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
