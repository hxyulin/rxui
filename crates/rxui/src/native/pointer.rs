//! Window-owned cursor visibility and native pointer grab state.
use rxui_host::winit::{
    error::ExternalError,
    window::{CursorGrabMode, Window},
};

/// Native grab selected by [`super::WindowHandle::set_pointer_lock`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerLock {
    /// Unbounded relative motion; the cursor stays in place.
    Locked,
    /// Cursor confined to the window, used when locking is unavailable.
    Confined,
}
#[derive(Default)]
pub(super) struct State {
    pub hidden: bool,
    pub lock: Option<PointerLock>,
    pub release_pending: bool,
    pub error: Option<String>,
    visible: Option<bool>,
    hovered_hidden: bool,
}
impl State {
    pub fn set_lock(
        &mut self,
        window: &Window,
        locked: bool,
    ) -> Result<Option<PointerLock>, ExternalError> {
        if locked && !window.has_focus() {
            return Err(ExternalError::Ignored);
        }
        let lock = if locked {
            match window.set_cursor_grab(CursorGrabMode::Locked) {
                Ok(()) => Some(PointerLock::Locked),
                Err(_) => {
                    window.set_cursor_grab(CursorGrabMode::Confined)?;
                    Some(PointerLock::Confined)
                }
            }
        } else {
            window.set_cursor_grab(CursorGrabMode::None)?;
            None
        };
        self.lock = lock;
        self.release_pending = false;
        self.error = None;
        self.visibility(window, self.hovered_hidden);
        Ok(lock)
    }
    pub fn visibility(&mut self, window: &Window, hovered_hidden: bool) {
        self.hovered_hidden = hovered_hidden;
        let visible = !(self.hidden || self.lock.is_some() || hovered_hidden);
        if self.visible != Some(visible) {
            window.set_cursor_visible(visible);
            self.visible = Some(visible);
        }
    }
    pub fn sync(&mut self, window: &Window, hovered_hidden: bool) {
        if std::mem::take(&mut self.release_pending) {
            if let Err(error) = window.set_cursor_grab(CursorGrabMode::None) {
                // Keep failure observable without turning deactivation into a fatal
                // runner error. Logical capture always ends and never re-arms itself.
                self.error = Some(error.to_string());
            }
            self.lock = None;
        }
        self.visibility(window, hovered_hidden);
    }
}
