//! Opt-in AppKit/Win32 primary-mouse adapter. See `docs/native-drag-desktop.md`.
//!
//! Register windows before input, acquire after a normal winit primary press,
//! and poll native observations while a lease is active. Ordinary UI input still
//! goes through winit. Native hooks latch release targeting before winit releases
//! capture, and supply cancellation that winit does not publicly expose.

use super::*;
use draglet::{Adapter, CaptureState, Signal, Target};
use std::{cell::RefCell, sync::Arc};

pub use draglet::Error;

/// Thread-bound native host for registered AppKit or Win32 windows.
/// Construction fails on other platforms. This feature is experimental: see the
/// platform validation matrix before treating it as a supported deployment.
pub struct DesktopDragHost {
    native: RefCell<Adapter>,
}

impl DesktopDragHost {
    /// Installs native observation on the winit event-loop thread.
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            native: RefCell::new(Adapter::new()?),
        })
    }
    /// Registers a window before its initiating native primary press.
    pub fn register(&mut self, window: &Arc<Window>) -> Result<(), Error> {
        self.native.get_mut().register(window)
    }
    /// Removes a registration before application-controlled window destruction.
    /// Cancel an active lease first, then return its host with `into_host`.
    pub fn unregister(&mut self, window: WindowId) -> Result<(), Error> {
        self.native.get_mut().unregister(window)
    }
}

impl NativeDragHost for DesktopDragHost {
    type Error = Error;
    fn capabilities(&self) -> NativeDragCapabilities {
        let Ok(mut native) = self.native.try_borrow_mut() else {
            return NativeDragCapabilities::WINIT_ONLY;
        };
        if matches!(native.capture_state(), CaptureState::Lost(_)) {
            return NativeDragCapabilities::WINIT_ONLY;
        }
        NativeDragCapabilities {
            authoritative_targeting: true,
            pointer_capture: true,
            capture_loss_notification: true,
        }
    }
    fn supports_button(&self, button: MouseButton) -> bool {
        button == MouseButton::Left
    }
    fn acquire_capture(&mut self, source: WindowId) -> Result<(), Self::Error> {
        self.native.get_mut().begin_capture(source)
    }
    fn release_capture(&mut self, source: WindowId) {
        self.native.get_mut().end_capture(source);
    }
    fn target_at_pointer(&mut self) -> NativeDragTarget {
        match self.native.get_mut().target_at_pointer() {
            Target::Unavailable => NativeDragTarget::Unavailable,
            Target::NoWindow => NativeDragTarget::NoWindow,
            Target::Foreign => NativeDragTarget::Foreign,
            Target::Window { window, position } => NativeDragTarget::Window {
                window,
                position: LogicalPosition::new(position[0], position[1]),
            },
        }
    }
}

impl CapturedDrag<'_, DesktopDragHost> {
    /// Drains native observations for this lease. Call from `about_to_wait` and
    /// source input handling; use a short `WaitUntil` while dragging so native
    /// cancellation is processed even when no winit window event is produced.
    ///
    /// Route the resulting updates once. Calling `observe` for the same motion
    /// as well produces duplicate moves; a terminal update remains idempotent.
    pub fn poll_native(&mut self) -> Vec<RoutedDragUpdate> {
        let Some(host) = self.host.as_mut() else {
            return Vec::new();
        };
        let signals = host.native.get_mut().take_signals();
        let mut updates = Vec::new();
        for signal in signals {
            if !self.is_active() {
                break;
            }
            match signal {
                Signal::Motion { source, position } if source == self.source() => {
                    if let Some(update) = self.observe(
                        source,
                        &WindowEvent::CursorMoved {
                            device_id: winit::event::DeviceId::dummy(),
                            position: PhysicalPosition::new(position[0], position[1]),
                        },
                    ) {
                        updates.push(update);
                    }
                }
                Signal::Released { source, .. } if source == self.source() => {
                    if let Some(update) = self.observe(
                        source,
                        &WindowEvent::MouseInput {
                            device_id: winit::event::DeviceId::dummy(),
                            state: ElementState::Released,
                            button: MouseButton::Left,
                        },
                    ) {
                        updates.push(update);
                    }
                }
                Signal::Lost { source } if source == self.source() => {
                    if let Some(update) = self.cancel(DragCancelReason::CaptureLost) {
                        updates.push(update);
                    }
                }
                _ => {}
            }
        }
        updates
    }
}
