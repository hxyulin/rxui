use crate::{AccessError, AppContext, Context, WeakEntity, runtime::MountLife};
use std::{
    fmt,
    rc::{Rc, Weak},
};

/// Result of dispatching a component listener.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dispatch {
    /// The callback updated its current owner successfully.
    Handled,
    /// The callback ran and called [`crate::Context::unchanged`]: its owner was
    /// not invalidated, and the event needs no redraw on its account.
    Unchanged,
    /// The mounted target or owner no longer exists; no callback ran.
    TargetGone,
}
type Callback<E> = dyn Fn(&E, &mut AppContext<'_>) -> Result<Dispatch, AccessError>;

/// Typed event handler with weak component and mount ownership.
/// Cloning a listener shares its callback without retaining the component/mount.
pub struct Listener<E: 'static> {
    callback: Rc<Callback<E>>,
}
impl<E> Clone for Listener<E> {
    fn clone(&self) -> Self {
        Self {
            callback: self.callback.clone(),
        }
    }
}
impl<E> fmt::Debug for Listener<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Listener").finish_non_exhaustive()
    }
}
impl<E> Listener<E> {
    #[cfg(feature = "layout")]
    pub(crate) fn from_callback(
        callback: impl Fn(&E, &mut AppContext<'_>) -> Result<Dispatch, AccessError> + 'static,
    ) -> Self {
        Self {
            callback: Rc::new(callback),
        }
    }
    pub(crate) fn bound<T: 'static>(
        owner: WeakEntity<T>,
        mount: Weak<MountLife>,
        callback: impl Fn(&mut T, &E, &mut Context<'_, T>) + 'static,
    ) -> Self {
        Self {
            callback: Rc::new(move |event, cx| {
                if owner.id().runtime != cx.runtime.id {
                    return Err(AccessError::WrongRuntime);
                }
                let Some(mount) = mount.upgrade() else {
                    return Ok(Dispatch::TargetGone);
                };
                if !cx.runtime.mount_live(mount.id) {
                    return Ok(Dispatch::TargetGone);
                }
                let Some(entity) = owner.upgrade() else {
                    return Ok(Dispatch::TargetGone);
                };
                let changed = entity.try_update(cx, |state, cx| {
                    cx.bind_dispatch_mount(Some(mount.id));
                    callback(state, event, cx);
                    cx.changed
                })?;
                Ok(if changed {
                    Dispatch::Handled
                } else {
                    Dispatch::Unchanged
                })
            }),
        }
    }
    #[cfg(feature = "layout")]
    pub(crate) fn map_event<F: 'static>(&self, map: impl Fn(&F) -> E + 'static) -> Listener<F> {
        let listener = self.clone();
        Listener {
            callback: Rc::new(move |event, cx| listener.dispatch(&map(event), cx)),
        }
    }
    /// Executes in an application mutation scope. Deferred effects are not flushed
    /// here. Wrong-runtime/reentrant accesses are errors, not silent no-ops.
    pub fn dispatch(&self, event: &E, cx: &mut AppContext<'_>) -> Result<Dispatch, AccessError> {
        (self.callback)(event, cx)
    }
}
