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
    /// Consecutive primary presses at one spot, as counted by the host: 2 for a
    /// double-click press, 3 for a triple-click. 1 for every other event, and for
    /// hosts that route through `Ui::pointer` without click tracking.
    pub click_count: u8,
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
/// Hover listener payload. Enter and leave are delivered to each element with a
/// hover listener when the pointer's hit target moves into or out of its subtree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoverEvent {
    /// Element whose hover listener runs.
    pub target: ElementId,
    /// True when the pointer entered the subtree, false when it left.
    pub hovered: bool,
}
/// Notification that keyboard focus entered or left an element's subtree, or
/// changed input modality while remaining there. It cannot prevent focus changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusEvent {
    /// Element whose listener runs (an ancestor may observe descendant focus).
    pub target: ElementId,
    /// Whether the subtree contains focus in the active window after the transition.
    pub focused: bool,
    /// Whether keyboard navigation is the current input modality.
    pub keyboard: bool,
}
/// Mouse wheel or trackpad scroll payload, routed from the element under the pointer
/// to the root before default container scrolling.
pub struct WheelInput {
    /// Topmost element under the pointer.
    pub target: ElementId,
    /// Element whose listener is executing.
    pub current_target: ElementId,
    /// Target or bubble stage; wheel input has no capture stage.
    pub phase: EventPhase,
    /// Window-local logical position.
    pub position: [f32; 2],
    /// Logical content motion; positive values move toward later content.
    pub delta: [f32; 2],
    /// Whether `delta` is precise motion or converted wheel steps.
    pub source: WheelSource,
    /// Source-window modifier state.
    pub modifiers: Modifiers,
    pub(crate) requests: Cell<Requests>,
}
/// How a wheel event's delta was produced. Default container scrolling applies
/// precise motion directly and may animate wheel steps; see `ScrollBehavior`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WheelSource {
    /// Pixel deltas from a trackpad or other precise device, already smooth.
    #[default]
    Precise,
    /// Discrete mouse-wheel steps converted to logical pixels by the host.
    Lines,
}
impl WheelInput {
    /// Ends the remaining route without suppressing default scrolling.
    pub fn stop_propagation(&self) {
        self.requests.update(|mut r| {
            r.stop = true;
            r
        });
    }
    /// Suppresses default container scrolling for this event.
    pub fn prevent_default(&self) {
        self.requests.update(|mut r| {
            r.prevent = true;
            r
        });
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
    /// Suppresses native-host control/text defaults for this event: activation, Tab
    /// focus traversal, built-in editing, and the text a key press would insert
    /// into a text input client.
    pub fn prevent_default(&self) {
        self.requests.update(|mut r| {
            r.prevent = true;
            r
        });
    }
}
/// Where committed text for a [text input client](crate::Element::text_input_client)
/// came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextInsertSource {
    /// Text produced by a key press after keyboard layout and dead-key processing.
    Keyboard,
    /// Text committed by an input method, replacing the client's preedit.
    Ime,
    /// Clipboard text from the standard Paste command's host default.
    Paste,
}
/// Committed text routed to the focused text input client and its ancestors.
///
/// Keyboard text arrives only after the key that produced it was routed to key
/// listeners without `prevent_default`. The text is passed through as the platform
/// produced it, so it may contain line breaks or other characters; the client
/// normalizes it.
pub struct TextInsertInput {
    /// Focused text input client.
    pub target: ElementId,
    /// Element whose listener is executing.
    pub current_target: ElementId,
    /// Target or bubble stage; text has no capture stage.
    pub phase: EventPhase,
    /// Committed text. Empty only for an IME commit that ends composition without
    /// text, which still replaces the preedit.
    pub text: String,
    /// Keyboard, IME commit or paste.
    pub source: TextInsertSource,
    pub(crate) requests: Cell<Requests>,
}
impl TextInsertInput {
    /// Ends the remaining route.
    pub fn stop_propagation(&self) {
        self.requests.update(|mut r| {
            r.stop = true;
            r
        });
    }
}
/// Why a text input client's IME composition ended without a commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImeCancelReason {
    /// The host cancelled composition: Escape, or the platform disabled the IME.
    Host,
    /// Focus moved to another element or left the placement.
    FocusChanged,
    /// The window lost native activation.
    Deactivated,
    /// A primary pointer press on the composing client, which normally moves its caret.
    Pointer,
    /// The client stopped accepting text: `text_input_client(false)`, hidden, inert,
    /// outside an open modal, or no longer focusable.
    Unavailable,
    /// The client was removed from the placement.
    Removed,
}
/// IME composition change for a text input client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeEvent {
    /// The composing (preedit) text changed. Display it at the client's caret, in
    /// place of its selection, without changing the document. Empty text clears the
    /// display while composition continues, as before a commit.
    Preedit {
        /// Composing text.
        text: String,
        /// UTF-8 byte range of the IME's selection or caret within `text`; `None`
        /// hides the composition caret.
        cursor: Option<(usize, usize)>,
    },
    /// Composition ended without committing; discard the preedit display. A commit
    /// ends composition through [`TextInsertInput`] instead, with
    /// [`TextInsertSource::Ime`], and sends no cancellation.
    Cancel(ImeCancelReason),
}
/// IME listener payload, routed to the composing text input client and its ancestors.
pub struct ImeInput {
    /// Composing text input client.
    pub target: ElementId,
    /// Element whose listener is executing.
    pub current_target: ElementId,
    /// Target or bubble stage; IME input has no capture stage.
    pub phase: EventPhase,
    /// Preedit change or cancellation.
    pub event: ImeEvent,
    pub(crate) requests: Cell<Requests>,
}
impl ImeInput {
    /// Ends the remaining route.
    pub fn stop_propagation(&self) {
        self.requests.update(|mut r| {
            r.stop = true;
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
    /// Horizontal resizing; the native host shows a column-resize cursor.
    ResizeHorizontal,
    /// Vertical resizing; the native host shows a row-resize cursor.
    ResizeVertical,
    /// Crosshair.
    Crosshair,
    /// Operation not allowed.
    NotAllowed,
    /// Bidirectional north-south resize, such as a horizontal edge.
    ResizeNorthSouth,
    /// Bidirectional east-west resize, such as a vertical edge.
    ResizeEastWest,
    /// Diagonal resize between the north-east and south-west corners.
    ResizeNorthEastSouthWest,
    /// Diagonal resize between the north-west and south-east corners.
    ResizeNorthWestSouthEast,
    /// Resizing a column boundary; the default for horizontal splitters.
    ResizeColumn,
    /// Resizing a row boundary; the default for vertical splitters.
    ResizeRow,
    /// Something can be moved in any direction.
    Move,
    /// The application is busy and does not accept input.
    Wait,
    /// Work is running in the background; input is still accepted.
    Progress,
    /// Help is available for the hovered element.
    Help,
    /// Zoom in.
    ZoomIn,
    /// Zoom out.
    ZoomOut,
    /// A context menu is available.
    ContextMenu,
    /// Dropping here copies.
    Copy,
    /// Dropping here creates a link or alias.
    Alias,
    /// The dragged item cannot be dropped here.
    NoDrop,
    /// Vertical text insertion.
    VerticalText,
    /// Selecting table cells.
    Cell,
    /// Hides the pointer while it is over the element, for example in a video
    /// view. The native host shows it again when another cursor applies.
    Hidden,
}
#[derive(Clone, Default)]
pub(crate) struct InputProperties {
    pub anchor: Option<crate::AnchorHandle>,
    pub measured_row: Option<crate::virtual_rows::MeasuredRow>,
    pub overlay: Option<Box<crate::overlay::Properties>>,
    pub menu: bool,
    pub commands: Vec<crate::commands::Action>,
    pub dock: Option<Box<crate::dock_view::Metadata>>,
    pub focus: Option<Box<crate::ui::focus::Properties>>,
    pub tabs: Option<Box<crate::tabs::Properties>>,
    pub pointer: [Option<Listener<PointerInput>>; 8],
    pub key: [Option<Listener<KeyInput>>; 4],
    pub hover: Option<Listener<HoverEvent>>,
    pub focus_change: Option<Listener<FocusEvent>>,
    pub wheel: Option<Listener<WheelInput>>,
    /// Text input client opt-in, listeners and IME area; see `Element::text_input_client`.
    pub text: Option<Box<TextClient>>,
    pub focusable: Option<bool>,
    pub cursor: Option<Cursor>,
    pub window_drag_region: bool,
    /// A label whose text can be selected and copied; see `Element::selectable`.
    pub selectable: bool,
}
impl InputProperties {
    pub fn pointer_target(&self) -> bool {
        self.pointer.iter().any(Option::is_some)
            || self.hover.is_some()
            || self.cursor.is_some()
            || self.window_drag_region
            || self.focusable == Some(true)
    }
}
/// Text input client configuration, allocated only on elements that use it.
#[derive(Clone, Default)]
pub(crate) struct TextClient {
    pub enabled: bool,
    pub insert: Option<Listener<TextInsertInput>>,
    pub ime: Option<Listener<ImeInput>>,
    /// IME candidate anchor relative to the element's border box.
    pub ime_area: Option<crate::Bounds>,
    /// Deferred candidate geometry, read after custom preparation by native hosts.
    pub ime_area_provider: Option<std::rc::Rc<dyn Fn() -> Option<crate::Bounds>>>,
}
