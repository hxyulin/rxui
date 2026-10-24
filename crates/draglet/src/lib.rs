#![doc = include_str!("../README.md")]

use std::{error::Error as StdError, fmt, marker::PhantomData, rc::Rc, sync::Arc};
use winit::window::{Window, WindowId};

#[cfg(all(target_os = "macos", feature = "appkit"))]
#[allow(unsafe_code)]
mod macos;
#[cfg(all(target_os = "macos", feature = "appkit"))]
use macos as platform;
#[cfg(all(target_os = "windows", feature = "win32"))]
#[allow(unsafe_code)]
mod windows;
#[cfg(all(target_os = "windows", feature = "win32"))]
use windows as platform;
#[cfg(not(any(
    all(target_os = "macos", feature = "appkit"),
    all(target_os = "windows", feature = "win32")
)))]
mod unsupported;
#[cfg(not(any(
    all(target_os = "macos", feature = "appkit"),
    all(target_os = "windows", feature = "win32")
)))]
use unsupported as platform;

/// Platform initialization, registration, or capture failure.
#[derive(Debug, Clone)]
pub struct Error(pub(crate) String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl StdError for Error {}

/// Native input destination, including foreign-window occlusion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// Native query or identity cannot be established.
    Unavailable,
    /// No native window is hit.
    NoWindow,
    /// A foreign/unregistered window or decoration is hit.
    Foreign,
    /// Registered client content, in destination logical coordinates.
    Window {
        /// Registered native identity.
        window: WindowId,
        /// Logical position within its content.
        position: [f64; 2],
    },
}
/// Lifecycle of the adapter's current primary-mouse gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureState {
    /// No gesture owned by the adapter.
    Idle,
    /// Native primary gesture is being tracked.
    Held(WindowId),
    /// A native button-up was observed and its destination sampled.
    Released(WindowId),
    /// Native cancellation/loss was observed; a drop cannot be accepted.
    Lost(WindowId),
}
/// Native input observations, latched before platform/winit defaults run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Signal {
    /// Source physical content coordinates, including outside its bounds.
    Motion {
        /// Source native identity.
        source: WindowId,
        /// Source physical position.
        position: [f64; 2],
    },
    /// Primary native release; target is queried at the native release boundary.
    Released {
        /// Source native identity.
        source: WindowId,
        /// Source physical position.
        position: [f64; 2],
    },
    /// The native gesture was cancelled or lost capture.
    Lost {
        /// Source native identity.
        source: WindowId,
    },
}
/// OS adapter kept on its creation/event-loop thread. It is neither Send nor Sync.
///
/// The native callbacks and registrations must never migrate to another thread:
///
/// ```compile_fail
/// fn requires_send<T: Send>() {}
/// requires_send::<draglet::Adapter>();
/// ```
///
/// ```compile_fail
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<draglet::Adapter>();
/// ```
///
/// Register on the window thread before input; begin only after the primary press.
/// Poll signals during an active gesture, even when ordinary input is idle. Route
/// UI input separately through winit; these signals supply drag mechanics only.
pub struct Adapter {
    inner: platform::Adapter,
    _thread: PhantomData<Rc<()>>,
}
impl Adapter {
    /// Installs the current platform hooks on the event-loop thread.
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            inner: platform::Adapter::new()?,
            _thread: PhantomData,
        })
    }
    /// Registers the winit window and installs native observation before input.
    pub fn register(&mut self, window: &Arc<Window>) -> Result<(), Error> {
        self.inner.register(window)
    }
    /// Cancels/unregisters before application-controlled source/window removal.
    pub fn unregister(&mut self, id: WindowId) -> Result<(), Error> {
        self.inner.unregister(id)
    }
    /// Begins tracking a verified native primary press in a registered source.
    pub fn begin_capture(&mut self, id: WindowId) -> Result<(), Error> {
        self.inner.begin_capture(id)
    }
    /// Ends this lease, preserving capture belonging to a newer/foreign gesture.
    pub fn end_capture(&mut self, id: WindowId) {
        self.inner.end_capture(id);
    }
    /// Refreshes native liveness/cancellation state without recording GPU work.
    pub fn capture_state(&mut self) -> CaptureState {
        self.inner.capture_state()
    }
    /// Takes pending observations. Call on the event-loop thread.
    pub fn take_signals(&mut self) -> Vec<Signal> {
        self.inner.take_signals()
    }
    /// Native live hit test; after release returns that native release's snapshot,
    /// validated against current registration liveness (never a cached hover).
    pub fn target_at_pointer(&mut self) -> Target {
        self.inner.target_at_pointer()
    }
}

#[cfg(all(test, not(any(target_os = "macos", target_os = "windows"))))]
mod tests {
    #[test]
    fn unsupported_platform_fails_before_installing_hooks() {
        let error = super::Adapter::new()
            .err()
            .expect("unsupported adapter must fail");
        assert!(error.to_string().contains("disabled or unavailable"));
    }
}

/// Compositor-driven same-client Wayland dragging on the application's connection.
#[cfg(all(feature = "wayland", unix))]
#[allow(unsafe_code)]
pub mod wayland;
/// Safe X11 protocol backend. Must be armed before the initiating press.
#[cfg(feature = "x11")]
pub mod x11;

/// Backend capabilities; unavailable guarantees must never be inferred from OS name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// Can identify registered targets authoritatively during its drag session.
    pub authoritative_targeting: bool,
    /// Can explicitly capture the pointer without confinement.
    pub pointer_capture: bool,
    /// Can report capture cancellation/loss.
    pub capture_loss_notification: bool,
}
