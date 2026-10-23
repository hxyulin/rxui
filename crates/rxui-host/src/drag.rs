//! Native drag mechanics, independent of docking policy and rendering.
//!
//! Geometry is not targeting: rectangles cannot establish stacking or foreign
//! occlusion. Winit alone supplies no reliable automatic cross-window routing.
//! Use [`DragTracker`] for observable events, or supply a [`NativeDragHost`] for
//! authoritative targeting and capture. See `docs/native-drag.md`.

use crate::WindowMetrics;
use std::{error::Error, fmt};
use winit::{
    dpi::{LogicalPosition, PhysicalPosition},
    event::{ElementState, MouseButton, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

#[cfg(feature = "x11-drag")]
pub mod x11;

#[cfg(feature = "desktop-drag")]
pub mod desktop;

/// Coordinate convention shared by geometry snapshots on one desktop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopSpace {
    /// macOS top-left desktop coordinates in Cocoa points, normalized per window.
    MacOsPoints,
    /// Windows/X11 desktop physical pixels; local pixels use the same units.
    PhysicalPixels,
}

/// A desktop point tagged with its convention to prevent accidental mixing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DesktopPoint {
    /// Coordinate system, not a monitor's scale factor.
    pub space: DesktopSpace,
    /// Top-left desktop coordinates; negative values are allowed.
    pub position: PhysicalPosition<f64>,
}

/// Best-effort native inner geometry snapshot, with no visibility/z-order claim.
/// Refresh after moves and scale changes and immediately before mapping a drop.
/// Native position/size/scale queries are not an atomic OS transaction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragGeometry {
    origin: DesktopPoint,
    metrics: WindowMetrics,
}

impl DragGeometry {
    /// Normalizes winit's reported inner position. On macOS winit multiplies
    /// the Cocoa origin by *this window's* scale; divide it back before comparing
    /// windows. Integer position rounding in winit can lose subpixel precision.
    pub fn from_winit_parts(
        position: PhysicalPosition<i32>,
        metrics: WindowMetrics,
        space: DesktopSpace,
    ) -> Self {
        let divisor = match space {
            DesktopSpace::MacOsPoints => metrics.scale_factor(),
            DesktopSpace::PhysicalPixels => 1.,
        };
        Self {
            origin: DesktopPoint {
                space,
                position: PhysicalPosition::new(
                    f64::from(position.x) / divisor,
                    f64::from(position.y) / divisor,
                ),
            },
            metrics,
        }
    }

    /// Queries current native geometry. Returns `None` where global positioning
    /// is unavailable (notably Wayland), or metrics are invalid.
    pub fn from_window(window: &Window) -> Option<Self> {
        let metrics = WindowMetrics::from_window(window).ok()?;
        let position = window.inner_position().ok()?;
        let space = if cfg!(target_os = "macos") {
            DesktopSpace::MacOsPoints
        } else {
            DesktopSpace::PhysicalPixels
        };
        Some(Self::from_winit_parts(position, metrics, space))
    }

    /// Normalized desktop inner origin.
    pub fn origin(self) -> DesktopPoint {
        self.origin
    }

    /// Converts an inner-window physical cursor position, including positions
    /// outside the source during capture. Rejects non-finite coordinates.
    pub fn to_desktop(self, local: PhysicalPosition<f64>) -> Option<DesktopPoint> {
        let divisor = match self.origin.space {
            DesktopSpace::MacOsPoints => self.metrics.scale_factor(),
            DesktopSpace::PhysicalPixels => 1.,
        };
        let position = PhysicalPosition::new(
            self.origin.position.x + local.x / divisor,
            self.origin.position.y + local.y / divisor,
        );
        finite(position).then_some(DesktopPoint {
            position,
            ..self.origin
        })
    }

    /// Maps desktop coordinates into destination logical content units without
    /// rounding. The result can be outside the content; it is not a hit test.
    pub fn to_logical(self, point: DesktopPoint) -> Option<LogicalPosition<f64>> {
        if point.space != self.origin.space {
            return None;
        }
        let divisor = match point.space {
            DesktopSpace::MacOsPoints => 1.,
            DesktopSpace::PhysicalPixels => self.metrics.scale_factor(),
        };
        let position = PhysicalPosition::new(
            (point.position.x - self.origin.position.x) / divisor,
            (point.position.y - self.origin.position.y) / divisor,
        );
        finite(position).then_some(LogicalPosition::new(position.x, position.y))
    }
}

fn finite(position: PhysicalPosition<f64>) -> bool {
    position.x.is_finite() && position.y.is_finite()
}

/// Guarantees supplied by a particular native host, not inferred from OS name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeDragCapabilities {
    /// Live native targeting respects stacking, foreign windows, decorations,
    /// input regions, and destination identity. Uncertainty returns Unavailable.
    pub authoritative_targeting: bool,
    /// Unconstrained pointer capture delivers source motion and button release
    /// outside its content area, without locking or confining the pointer.
    pub pointer_capture: bool,
    /// Host reports capture theft/cancellation before a drop can be accepted.
    pub capture_loss_notification: bool,
}

impl NativeDragCapabilities {
    /// Safe winit-only baseline. Implicit button grabs are not an explicit,
    /// observable capture contract; `set_cursor_grab` confines/locks the cursor.
    pub const WINIT_ONLY: Self = Self {
        authoritative_targeting: false,
        pointer_capture: false,
        capture_loss_notification: false,
    };

    /// Whether this host can support a captured, authoritative cross-window drag.
    pub const fn reliable_routing(self) -> bool {
        self.authoritative_targeting && self.pointer_capture && self.capture_loss_notification
    }
}

/// Native destination measured by a host; never an application rectangle guess.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NativeDragTarget {
    /// No authoritative answer (including unsupported or temporarily failed query).
    Unavailable,
    /// Native query found no destination content at the pointer.
    NoWindow,
    /// A foreign/unregistered native window or decoration blocks the destination.
    Foreign,
    /// Topmost eligible registered native content and its local logical point.
    Window {
        /// Destination native identity; host must reject destroyed/stale IDs.
        window: WindowId,
        /// Destination content coordinates using its current scale factor.
        position: LogicalPosition<f64>,
    },
}

/// Native integration boundary, implemented by opt-in platform hosts.
///
/// Implementations must not advertise guarantees they cannot supply. In
/// particular, rectangle iteration, focus, `CursorEntered`, and `Occluded(false)`
/// do not satisfy authoritative targeting. A platform adapter may use safe OS
/// bindings outside this crate. Native handles alone do not establish safety.
pub trait NativeDragHost {
    /// Capture acquisition failure type.
    type Error;
    /// Actual guarantees available on the current display connection.
    fn capabilities(&self) -> NativeDragCapabilities;
    /// Whether this host supports the initiating button. Defaults to all buttons;
    /// a primary-only native backend must reject other buttons before capture.
    fn supports_button(&self, _button: MouseButton) -> bool {
        true
    }
    /// Acquire unconstrained pointer capture after the initiating button press.
    /// On error, clean up any partially acquired resources before returning.
    fn acquire_capture(&mut self, source: WindowId) -> Result<(), Self::Error>;
    /// End the host's capture lease and resources. Called once after successful
    /// acquisition, including guard drop. Must be harmless after capture loss.
    /// When adopting framework-owned native button capture, preserve its lifetime
    /// and any other button/gesture sharing it; end only this drag's observation.
    fn release_capture(&mut self, source: WindowId);
    /// Resolve the *current* pointer using native stacking/occlusion and current
    /// destination geometry. Must return Unavailable after unreported capture
    /// loss or if any required information is uncertain. No cached hover answer.
    fn target_at_pointer(&mut self) -> NativeDragTarget;
}

/// Why an observed drag terminated without a drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragCancelReason {
    /// Escape pressed in the source.
    Escape,
    /// Source lost keyboard focus; conservatively cancel.
    FocusLost,
    /// Source was closed/destroyed (including programmatic removal).
    SourceClosed,
    /// App/event loop suspended.
    Suspended,
    /// Native host reported capture theft or compositor cancellation.
    CaptureLost,
    /// Host can no longer promise the guarantees needed for routing.
    HostUnavailable,
    /// Application chose to cancel.
    Explicit,
}

/// State transitions observable in the winit event stream.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragUpdate {
    /// Physical content position in the source, possibly outside its bounds.
    Moved(PhysicalPosition<f64>),
    /// The initiating button was released. This alone establishes no destination.
    Released,
    /// Terminal cancellation.
    Cancelled(DragCancelReason),
}

/// Single-button event tracker. Does not acquire OS capture or guarantee delivery.
/// Call only after receiving a source button press; forward all source events.
#[derive(Debug)]
pub struct DragTracker {
    source: WindowId,
    button: MouseButton,
    active: bool,
}

impl DragTracker {
    /// Starts tracking an already observed button press.
    pub fn new(source: WindowId, button: MouseButton) -> Self {
        Self {
            source,
            button,
            active: true,
        }
    }
    /// Source identity, also needed when cancelling programmatic window closure.
    pub fn source(&self) -> WindowId {
        self.source
    }
    /// Whether a terminal transition has not yet occurred.
    pub fn is_active(&self) -> bool {
        self.active
    }
    /// Idempotent explicit cancellation for suspension, capture loss, or shutdown.
    pub fn cancel(&mut self, reason: DragCancelReason) -> Option<DragUpdate> {
        if !self.active {
            return None;
        }
        self.active = false;
        Some(DragUpdate::Cancelled(reason))
    }
    /// Observes only source events. Cursor exit does not cancel: a captured drag
    /// must leave its source. Other buttons/windows cannot finish this drag.
    pub fn observe(&mut self, window: WindowId, event: &WindowEvent) -> Option<DragUpdate> {
        if !self.active || window != self.source {
            return None;
        }
        match event {
            WindowEvent::CursorMoved { position, .. } if finite(*position) => {
                Some(DragUpdate::Moved(*position))
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button,
                ..
            } if *button == self.button => {
                self.active = false;
                Some(DragUpdate::Released)
            }
            WindowEvent::Focused(false) => self.cancel(DragCancelReason::FocusLost),
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                self.cancel(DragCancelReason::SourceClosed)
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && event.physical_key == PhysicalKey::Code(KeyCode::Escape) =>
            {
                self.cancel(DragCancelReason::Escape)
            }
            _ => None,
        }
    }
}

/// Failure before a captured drag starts.
#[derive(Debug, PartialEq, Eq)]
pub enum DragStartError<E> {
    /// Host cannot promise reliable capture, cancellation, and native targeting.
    Unsupported,
    /// Host failed to acquire capture; no drag was started.
    Capture(E),
}

impl<E: fmt::Display> fmt::Display for DragStartError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("native host cannot guarantee reliable drag routing"),
            Self::Capture(error) => write!(f, "native pointer capture failed: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for DragStartError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Unsupported => None,
            Self::Capture(error) => Some(error),
        }
    }
}

/// Outcome of an event routed through a captured host.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoutedDragUpdate {
    /// Observable input/lifecycle transition.
    pub update: DragUpdate,
    /// Fresh host query on motion and release; Unavailable on cancellation.
    /// RXUI decides whether an authoritative destination accepts a panel.
    pub target: NativeDragTarget,
}

/// RAII capture lease. Drop, release, and cancellation release the host once.
///
/// Keep on the event-loop thread, forward source events, and explicitly cancel
/// on suspension, programmatic source closure, and host capture-loss messages.
/// The host remains borrowed so another lease cannot acquire it concurrently.
pub struct CapturedDrag<'a, H: NativeDragHost> {
    host: Option<&'a mut H>,
    tracker: DragTracker,
    held: bool,
}

impl<'a, H: NativeDragHost> CapturedDrag<'a, H> {
    /// Acquires capture only if the host advertises every required guarantee.
    pub fn begin(
        host: &'a mut H,
        source: WindowId,
        button: MouseButton,
    ) -> Result<Self, DragStartError<H::Error>> {
        if !host.capabilities().reliable_routing() || !host.supports_button(button) {
            return Err(DragStartError::Unsupported);
        }
        host.acquire_capture(source)
            .map_err(DragStartError::Capture)?;
        Ok(Self {
            host: Some(host),
            tracker: DragTracker::new(source, button),
            held: true,
        })
    }
    /// Processes a source event. Re-query native targeting at release before
    /// relinquishing capture, even when the last hover had a valid destination.
    pub fn observe(&mut self, window: WindowId, event: &WindowEvent) -> Option<RoutedDragUpdate> {
        if !self.host.as_ref()?.capabilities().reliable_routing() {
            return self.cancel(DragCancelReason::HostUnavailable);
        }
        let update = self.tracker.observe(window, event)?;
        let target = match update {
            DragUpdate::Cancelled(_) => NativeDragTarget::Unavailable,
            _ => self.host.as_mut()?.target_at_pointer(),
        };
        if !self.tracker.is_active() {
            self.release();
        }
        Some(RoutedDragUpdate { update, target })
    }
    /// Whether the drag has not yet released or cancelled.
    pub fn is_active(&self) -> bool {
        self.tracker.is_active()
    }
    /// Source identity for routing programmatic removal/capture-loss messages.
    pub fn source(&self) -> WindowId {
        self.tracker.source()
    }
    /// Ends the lease and returns the original host borrow for another drag.
    /// An active drag is cancelled without producing a destination.
    /// This supports event-loop handlers storing the host and lease in Options.
    pub fn into_host(mut self) -> &'a mut H {
        self.tracker.cancel(DragCancelReason::Explicit);
        self.release();
        self.host.take().expect("capture lease owns its host")
    }
    /// Cancels on host capture loss, suspension, source removal, or app policy.
    pub fn cancel(&mut self, reason: DragCancelReason) -> Option<RoutedDragUpdate> {
        let update = self.tracker.cancel(reason)?;
        self.release();
        Some(RoutedDragUpdate {
            update,
            target: NativeDragTarget::Unavailable,
        })
    }
    fn release(&mut self) {
        if self.held {
            self.held = false;
            if let Some(host) = self.host.as_mut() {
                host.release_capture(self.tracker.source());
            }
        }
    }
}

impl<H: NativeDragHost> Drop for CapturedDrag<'_, H> {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests;
