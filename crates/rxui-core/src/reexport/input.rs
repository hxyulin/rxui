//! Pointer, keyboard, and IME input, plus constructors for synthesizing it.
//!
//! Two reasons this module carries functions and not only re-exports. A
//! [`KeyboardInput`] has eight fields, six of which no caller has an opinion
//! about, so every test that wanted to press a key wrote a fifteen-line struct
//! literal and imported seven types to do it. And `astrelis-ui-next` does not
//! re-export the platform input types it names in [`UiInput`], so no single
//! engine crate was ever enough to build one.
//!
//! The constructors are the whole reason the seven-type import goes away; they
//! are not test-only, because dispatching synthesized input is how an
//! application implements a macro recorder, a tutorial, or a demo mode.

pub use astrelis_platform::{
    CursorIcon, DeviceId, ElementState, GestureDelta, ImeEvent, ImePurpose, Key, KeyCode,
    KeyLocation, KeyboardInput, Modifiers, NamedKey, NativeKey, NativeKeyCode, PhysicalKey,
    PointerButton, ScrollDelta, Touch, TouchForce, TouchPhase,
};
pub use astrelis_ui_next::UiInput;

use astrelis_core::geometry::LogicalPoint;

/// Pointer motion to a window-space point.
pub fn pointer_moved(point: LogicalPoint) -> UiInput {
    UiInput::PointerMoved(point)
}

/// A primary-button press at a window-space point.
pub fn pointer_pressed(point: LogicalPoint) -> UiInput {
    UiInput::PointerPressed(point)
}

/// A primary-button release at a window-space point.
pub fn pointer_released(point: LogicalPoint) -> UiInput {
    UiInput::PointerReleased(point)
}

/// A wheel tick of `delta` logical pixels, routed by `position`.
pub fn pointer_wheel(position: LogicalPoint, delta: LogicalPoint) -> UiInput {
    UiInput::PointerWheel { position, delta }
}

/// A press of `logical_key` with no modifiers and no text.
///
/// The physical key is left unidentified and the location standard: RXUI routes
/// on the logical key, and inventing a scancode would imply a keyboard layout
/// the caller never chose. Use [`with_modifiers`] to add a modifier state.
pub fn key(logical_key: Key) -> UiInput {
    keyboard(logical_key, None)
}

/// A press of a named key such as `NamedKey::Escape`.
pub fn named_key(named: NamedKey) -> UiInput {
    key(Key::Named(named))
}

/// A keystroke that also produces `text`, which is what a text field consumes.
pub fn text(text: &str) -> UiInput {
    keyboard(Key::Character(text.into()), Some(text.to_string()))
}

/// Applies a modifier state to a keyboard event, and leaves anything else alone.
///
/// Separating this from the constructors is what keeps [`Modifiers`] out of the
/// common call: it derives `Default`, so an unmodified press never has to name
/// it, and a chorded one names only the chord.
pub fn with_modifiers(input: UiInput, modifiers: Modifiers) -> UiInput {
    match input {
        UiInput::Keyboard { input, .. } => UiInput::Keyboard { input, modifiers },
        other => other,
    }
}

fn keyboard(logical_key: Key, text: Option<String>) -> UiInput {
    UiInput::Keyboard {
        input: KeyboardInput {
            device_id: DeviceId(1),
            physical_key: PhysicalKey::Unidentified,
            logical_key,
            text,
            location: KeyLocation::Standard,
            state: ElementState::Pressed,
            repeat: false,
            synthetic: false,
        },
        modifiers: Modifiers::default(),
    }
}
