//! RXUI lifecycle bridge for draglet's standalone X11 backend.
use super::*;
pub use draglet::x11::{X11Adapter, X11DragError, X11Input};
use std::sync::Arc;
use x11rb::{connection::Connection, rust_connection::RustConnection};

/// X11 adapter integrated with RXUI's captured-drag lifecycle.
pub struct X11DragHost {
    inner: X11Adapter,
    #[cfg(test)]
    connection: Arc<RustConnection>,
}
impl std::ops::Deref for X11DragHost {
    type Target = X11Adapter;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl std::ops::DerefMut for X11DragHost {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
impl X11DragHost {
    /// Returns the target in RXUI coordinate types.
    pub fn target_at_pointer(&mut self) -> NativeDragTarget {
        NativeDragHost::target_at_pointer(self)
    }
    /// Opens a dedicated native X11 connection.
    pub fn connect(display: Option<&str>) -> Result<Self, X11DragError> {
        let (connection, screen) = x11rb::connect(display).map_err(X11DragError::from_display)?;
        let root = connection.setup().roots[screen].root;
        Self::from_connection(Arc::new(connection), root)
    }
    /// Uses a dedicated application-provided connection and screen root.
    pub fn from_connection(
        connection: Arc<RustConnection>,
        root: u32,
    ) -> Result<Self, X11DragError> {
        Ok(Self {
            inner: X11Adapter::from_connection(connection.clone(), root)?,
            #[cfg(test)]
            connection,
        })
    }
}
impl NativeDragHost for X11DragHost {
    type Error = X11DragError;
    fn supports_button(&self, button: MouseButton) -> bool {
        self.inner.supports_button(button)
    }
    fn capabilities(&self) -> NativeDragCapabilities {
        let c = self.inner.capabilities();
        NativeDragCapabilities {
            authoritative_targeting: c.authoritative_targeting,
            pointer_capture: c.pointer_capture,
            capture_loss_notification: c.capture_loss_notification,
        }
    }
    fn acquire_capture(&mut self, source: WindowId) -> Result<(), Self::Error> {
        self.inner.acquire_capture(source)
    }
    fn release_capture(&mut self, source: WindowId) {
        self.inner.release_capture(source);
    }
    fn target_at_pointer(&mut self) -> NativeDragTarget {
        match self.inner.target_at_pointer() {
            draglet::Target::Unavailable => NativeDragTarget::Unavailable,
            draglet::Target::NoWindow => NativeDragTarget::NoWindow,
            draglet::Target::Foreign => NativeDragTarget::Foreign,
            draglet::Target::Window { window, position } => NativeDragTarget::Window {
                window,
                position: LogicalPosition::new(position[0], position[1]),
            },
        }
    }
}
impl CapturedDrag<'_, X11DragHost> {
    /// Pumps the host's independent X connection during capture. Use an event
    /// loop wake/deadline while active; native winit motion is not the event source.
    /// A connection failure cancels the lease before returning an error.
    pub fn poll_x11(&mut self) -> Result<Option<RoutedDragUpdate>, X11DragError> {
        if !self.is_active() {
            return Ok(None);
        }
        let event = match self.host.as_mut().expect("lease host").poll_input() {
            Ok(e) => e,
            Err(e) => {
                self.cancel(DragCancelReason::CaptureLost);
                return Err(e);
            }
        };
        let Some(event) = event else {
            return Ok(None);
        };
        let (window, event) = match event {
            X11Input::Moved { window, position } => (
                window,
                WindowEvent::CursorMoved {
                    device_id: winit::event::DeviceId::dummy(),
                    position,
                },
            ),
            X11Input::Released { window, .. } => (
                window,
                WindowEvent::MouseInput {
                    device_id: winit::event::DeviceId::dummy(),
                    state: ElementState::Released,
                    button: MouseButton::Left,
                },
            ),
            X11Input::Cancelled { window } if window == self.source() => {
                return Ok(self.cancel(DragCancelReason::CaptureLost));
            }
            _ => return Ok(None),
        };
        Ok(self.observe(window, &event))
    }
}
#[cfg(test)]
mod tests;
