use crate::{
    AccessError, AppContext, Context, EntityId, ReadContext,
    runtime::{Release, RuntimeInner},
};
use std::{
    cell::{Cell, Ref, RefCell},
    fmt,
    ops::Deref,
    rc::{Rc, Weak},
};

pub(crate) struct EntityCell<T> {
    pub(crate) id: EntityId,
    pub(crate) runtime: Weak<RuntimeInner>,
    pub(crate) value: RefCell<Option<T>>,
    pub(crate) valid: Cell<bool>,
}
impl<T> Drop for EntityCell<T> {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.releases.borrow_mut().push(Release::Entity(self.id));
        }
    }
}

/// Strong UI-thread handle to one typed, persistent value.
/// Clones share state. A runtime stores only weak references to entity values.
/// Values may be models or, in later milestones, stateful views.
pub struct Entity<T: 'static> {
    pub(crate) cell: Rc<EntityCell<T>>,
}
impl<T> Clone for Entity<T> {
    fn clone(&self) -> Self {
        Self {
            cell: self.cell.clone(),
        }
    }
}
impl<T> fmt::Debug for Entity<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Entity").field(&self.id()).finish()
    }
}

/// Weak entity identity, without retaining its value or runtime.
pub struct WeakEntity<T: 'static> {
    id: EntityId,
    cell: Weak<EntityCell<T>>,
}
impl<T> Clone for WeakEntity<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            cell: self.cell.clone(),
        }
    }
}
impl<T> fmt::Debug for WeakEntity<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("WeakEntity").field(&self.id).finish()
    }
}

/// Immutable access scoped to both an entity and a valid read context.
pub struct Read<'a, T> {
    value: Ref<'a, T>,
}
impl<T> Deref for Read<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: 'static> Entity<T> {
    /// Runtime-scoped generation identity; does not borrow the value.
    pub fn id(&self) -> EntityId {
        self.cell.id
    }
    /// Creates a handle that does not keep this value alive.
    pub fn downgrade(&self) -> WeakEntity<T> {
        WeakEntity {
            id: self.id(),
            cell: Rc::downgrade(&self.cell),
        }
    }
    /// Reads a value, tracking a dependency only in a ViewContext.
    /// Panics on programmer misuse; try_read provides the fallible primitive.
    pub fn read<'a>(&'a self, cx: &'a impl ReadContext) -> Read<'a, T> {
        self.try_read(cx)
            .unwrap_or_else(|e| panic!("cannot read {:?}: {e}", self.id()))
    }
    /// Reads without panicking on cross-runtime or incompatible access.
    pub fn try_read<'a>(&'a self, cx: &'a impl ReadContext) -> Result<Read<'a, T>, AccessError> {
        cx.validate(self.id())?;
        let value = self
            .cell
            .value
            .try_borrow()
            .map_err(|_| AccessError::Borrowed)?;
        if value.is_none() {
            return Err(AccessError::Borrowed);
        }
        cx.track(self.id());
        Ok(Read {
            value: Ref::map(value, |v| v.as_ref().expect("checked entity value")),
        })
    }
    /// Mutates synchronously, restoring the value and invalidating dependents even
    /// during unwind. The closure's return value is returned unchanged; it is not
    /// a rollback transaction. Observers/effects run only at a later flush.
    /// Panics on programmer misuse; use try_update for fallible access.
    pub fn update<R>(
        &self,
        cx: &mut AppContext<'_>,
        f: impl FnOnce(&mut T, &mut Context<'_, T>) -> R,
    ) -> R {
        self.try_update(cx, f)
            .unwrap_or_else(|e| panic!("cannot update {:?}: {e}", self.id()))
    }
    /// Fallible access to the same update scope used by update and listeners.
    pub fn try_update<R>(
        &self,
        cx: &mut AppContext<'_>,
        f: impl FnOnce(&mut T, &mut Context<'_, T>) -> R,
    ) -> Result<R, AccessError> {
        cx.validate(self.id())?;
        let value = self
            .cell
            .value
            .try_borrow_mut()
            .map_err(|_| AccessError::Borrowed)?
            .take()
            .ok_or(AccessError::Borrowed)?;
        let mut lease = UpdateLease {
            cell: &self.cell,
            value: Some(value),
            runtime: cx.runtime,
        };
        let mut context = Context::new(cx.runtime, self.downgrade());
        context.bind_dispatch_mount(cx.dispatch_mount);
        Ok(f(lease.value.as_mut().expect("leased value"), &mut context))
    }
}
impl<T: 'static> WeakEntity<T> {
    /// Original runtime/generation identity, also after disposal.
    pub fn id(&self) -> EntityId {
        self.id
    }
    /// Upgrades only while the value and its runtime remain alive.
    pub fn upgrade(&self) -> Option<Entity<T>> {
        let cell = self.cell.upgrade()?;
        if !cell.valid.get() {
            return None;
        }
        cell.runtime.upgrade()?;
        Some(Entity { cell })
    }
    /// Updates a live weak target or returns Disposed. No resurrection occurs.
    pub fn update<R>(
        &self,
        cx: &mut AppContext<'_>,
        f: impl FnOnce(&mut T, &mut Context<'_, T>) -> R,
    ) -> Result<R, AccessError> {
        if self.id.runtime != cx.runtime.id {
            return Err(AccessError::WrongRuntime);
        }
        self.upgrade()
            .ok_or(AccessError::Disposed)?
            .try_update(cx, f)
    }
}

struct UpdateLease<'a, T> {
    cell: &'a EntityCell<T>,
    value: Option<T>,
    runtime: &'a Rc<RuntimeInner>,
}
impl<T> Drop for UpdateLease<'_, T> {
    fn drop(&mut self) {
        *self.cell.value.borrow_mut() = self.value.take();
        self.runtime.changed(self.cell.id);
    }
}
