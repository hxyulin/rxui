//! Portable mouse/keyboard input and per-dispatch requests. No window/GPU ownership.
use crate::{Bounds, ElementId, Listener};
use std::cell::Cell;

/// Modifier snapshot in the source window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Shift is held.
    pub shift: bool,
    /// Control is held.
    pub control: bool,
    /// Alt/Option is held.
    pub alt: bool,
    /// Super/Command is held.
    pub meta: bool,
}
impl Modifiers {
    /// Conventional command modifier: Command on macOS, Control elsewhere.
    pub fn primary(self) -> bool {
        if cfg!(target_os = "macos") {
            self.meta
        } else {
            self.control
        }
    }
}
/// Mouse buttons supported by the first general input slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    /// Left/primary button.
    Primary,
    /// Right/secondary button.
    Secondary,
    /// Middle button.
    Middle,
}
/// Small, allocation-free held-button snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerButtons(pub(crate) u8);
impl PointerButtons {
    /// Whether this button is currently held.
    pub fn contains(self, button: PointerButton) -> bool {
        self.0 & (1 << button as u8) != 0
    }
    pub(crate) fn set(&mut self, button: PointerButton, down: bool) {
        if down {
            self.0 |= 1 << button as u8;
        } else {
            self.0 &= !(1 << button as u8);
        }
    }
    pub(crate) fn any(self) -> bool {
        self.0 != 0
    }
}
/// Event position in the root-to-target capture / target-to-root bubble route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventPhase {
    /// Ancestor capture handler, before the target.
    Capture,
    /// Handler on the routed target.
    Target,
    /// Ancestor handler, after the target.
    Bubble,
}
/// Reason a captured mouse gesture ends without a normal release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerCancelReason {
    /// Host/platform cancellation, including loss of native activation.
    Host,
    /// Escape cancelled the gesture.
    Escape,
    /// Captured element was removed, hidden, inert or stopped accepting pointer input.
    TargetUnavailable,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct Requests {
    pub stop: bool,
    pub prevent: bool,
    pub capture: Option<bool>,
    pub focus: bool,
}
/// Mouse listener payload. Coordinates are logical window units. Requests apply
/// after each callback, to current_target, without mutating the UI during a callback.
/// Capture routes motion/up to its owner; hit_target still reports the element under
/// the pointer. One mouse gesture per UI is supported; touch/pen are separate future work.
pub struct PointerInput {
    /// Element receiving this event route (capture owner, if any).
    pub target: ElementId,
    /// Element whose listener is executing.
    pub current_target: ElementId,
    /// Independently hit-tested element under the pointer, respecting z/clip/pointer policy.
    pub hit_target: Option<ElementId>,
    /// Capture/target/bubble stage.
    pub phase: EventPhase,
    /// Current window-local logical position.
    pub position: [f32; 2],
    /// Position relative to current_target's border box.
    pub local_position: [f32; 2],
    /// Current target border bounds.
    pub bounds: Bounds,
    /// Current parent content bounds, for gestures measured against their container.
    pub parent_bounds: Option<Bounds>,
    /// Changed button for down/up, absent for motion/cancellation.
    pub button: Option<PointerButton>,
    /// Held buttons after this native event.
    pub buttons: PointerButtons,
    /// Source-window modifier state.
    pub modifiers: Modifiers,
    /// Position where this captured gesture began.
    pub press_position: Option<[f32; 2]>,
    /// Capture owner's border bounds when capture began.
    pub press_bounds: Option<Bounds>,
    /// Capture owner's parent content bounds when capture began.
    pub press_parent_bounds: Option<Bounds>,
    /// Present for a cancelled gesture.
    pub cancel_reason: Option<PointerCancelReason>,
    pub(crate) requests: Cell<Requests>,
}
impl PointerInput {
    /// Ends the remaining event route. This does not suppress the default action.
    pub fn stop_propagation(&self) {
        self.requests.update(|mut r| {
            r.stop = true;
            r
        });
    }
    /// Suppresses built-in focus, selection or activation for this dispatch.
    pub fn prevent_default(&self) {
        self.requests.update(|mut r| {
            r.prevent = true;
            r
        });
    }
    /// Requests capture by current_target while a button is held. Capturing an
    /// ancestor does not itself suppress a child's default action; prevent it explicitly.
    pub fn capture_pointer(&self) {
        self.requests.update(|mut r| {
            r.capture = Some(true);
            r
        });
    }
    /// Releases explicit capture if current_target owns it.
    pub fn release_pointer(&self) {
        self.requests.update(|mut r| {
            r.capture = Some(false);
            r
        });
    }
    /// Requests focus for current_target; it must be focusable and enabled.
    pub fn focus(&self) {
        self.requests.update(|mut r| {
            r.focus = true;
            r
        });
    }
    /// Window-local displacement from the gesture's press position.
    pub fn drag_delta(&self) -> Option<[f32; 2]> {
        self.press_position
            .map(|p| [self.position[0] - p[0], self.position[1] - p[1]])
    }
}
/// Logical keyboard identity. Characters preserve the native logical key spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyboardKey {
    /// Logical character key, distinct from IME/text insertion.
    Character(String),
    /// Escape.
    Escape,
    /// Enter/Return.
    Enter,
    /// Space.
    Space,
    /// Tab.
    Tab,
    /// Left arrow.
    ArrowLeft,
    /// Right arrow.
    ArrowRight,
    /// Up arrow.
    ArrowUp,
    /// Down arrow.
    ArrowDown,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Other native named/unknown key, for diagnostics; spelling is backend-dependent.
    Other(String),
}
/// Portable host keyboard event, before built-in control/text defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    /// Logical identity.
    pub key: KeyboardKey,
    /// True for key down, false for key up.
    pub pressed: bool,
    /// Native repeat flag.
    pub repeat: bool,
    /// Modifiers.
    pub modifiers: Modifiers,
}
/// Key listener payload, routed through focused element ancestors (or root).
pub struct KeyInput {
    /// Routed focus/root identity.
    pub target: ElementId,
    /// Executing listener identity.
    pub current_target: ElementId,
    /// Routing stage.
    pub phase: EventPhase,
    /// Native logical key event.
    pub event: KeyEvent,
    pub(crate) requests: Cell<Requests>,
}
impl KeyInput {
    /// Ends the remaining route without suppressing defaults.
    pub fn stop_propagation(&self) {
        self.requests.update(|mut r| {
            r.stop = true;
            r
        });
    }
    /// Suppresses native-host control/text defaults for this event.
    pub fn prevent_default(&self) {
        self.requests.update(|mut r| {
            r.prevent = true;
            r
        });
    }
}
/// Host-visible dispatch result. Listener invocation requests redraw/preparation;
/// default_prevented lets a custom host skip its own keyboard/text defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputResult {
    /// A callback ran or retained input state changed.
    pub changed: bool,
    /// Built-in/default handling was suppressed.
    pub default_prevented: bool,
}
/// Portable native cursor selection. A captured element's cursor wins during a gesture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cursor {
    /// Platform arrow.
    #[default]
    Default,
    /// Clickable hand.
    Pointer,
    /// Text insertion cursor.
    Text,
    /// Open dragging hand.
    Grab,
    /// Closed dragging hand.
    Grabbing,
    /// Horizontal resizing.
    ResizeHorizontal,
    /// Vertical resizing.
    ResizeVertical,
    /// Crosshair.
    Crosshair,
    /// Operation not allowed.
    NotAllowed,
}
#[derive(Clone, Default)]
pub(crate) struct InputProperties {
    pub pointer: [Option<Listener<PointerInput>>; 8],
    pub key: [Option<Listener<KeyInput>>; 4],
    pub focusable: Option<bool>,
    pub cursor: Option<Cursor>,
}
impl InputProperties {
    pub fn pointer_target(&self) -> bool {
        self.pointer.iter().any(Option::is_some)
            || self.cursor.is_some()
            || self.focusable == Some(true)
    }
}
