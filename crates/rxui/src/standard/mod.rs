//! Standard form controls: check boxes, switches, radio groups, sliders, progress
//! indicators, selects and tooltips.
//!
//! Every control is controlled. The application owns the value and receives typed
//! proposals through a [`Listener`], exactly like `text_input` or `tabs`. Controls are
//! composed from the existing primitives: buttons supply focus, Tab stops,
//! Enter/Space activation, disabled state and hover/pressed paint; popovers supply
//! placement and dismissal. Transient interaction state that is not application data
//! (whether a select's list is open, a pending tooltip delay) belongs to the
//! placement through [`ViewContext::use_keyed_state`], so the stateful builders take
//! the view context.
//!
//! ```
//! use rxui::prelude::*;
//! struct Settings { wrap: bool, size: Key, volume: f32 }
//! impl View for Settings {
//!     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
//!         column().gap(8.)
//!             .child(checkbox(self.wrap, "Wrap lines")
//!                 .on_change(cx.listener(|s, e: &ToggleEvent, _| s.wrap = e.checked)))
//!             .child(radio_group().selected(self.size.clone())
//!                 .accessibility_label("Size")
//!                 .option(choice("small", "Small"))
//!                 .option(choice("large", "Large"))
//!                 .on_change(cx.listener(|s, e: &ChoiceEvent, _| s.size = e.key.clone())))
//!             .child(slider(self.volume).range(0., 100.).step(1.)
//!                 .accessibility_label("Volume")
//!                 .on_change(cx.listener(|s, e: &SliderEvent, _| s.volume = e.value)))
//!     }
//! }
//! ```
use crate::{AccessError, AppContext, Dispatch, Element, Key, Listener, ThemeColor, ViewContext};

mod glyph;
mod progress;
mod radio;
mod select;
mod slider;
mod toggle;
mod tooltip;

pub use progress::{ProgressBar, progress_bar, spinner};
pub use radio::{RadioGroup, radio_group};
pub use select::{Select, select, select_keyed};
pub use slider::{Slider, SliderEvent, SliderPhase, slider};
pub use toggle::{Toggle, checkbox, switch};
pub use tooltip::{TOOLTIP_DELAY, tooltip};

#[cfg(all(test, feature = "rendering"))]
mod gpu_tests;
#[cfg(test)]
mod tests;

/// Proposed check state from a check box or switch. A mixed check box proposes
/// `true`, matching the platform convention; store the value to accept it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToggleEvent {
    /// Proposed state.
    pub checked: bool,
}

/// Proposed selection from a radio group or select. Store the key to accept it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceEvent {
    /// Key of the chosen option.
    pub key: Key,
}

/// One option of a [`RadioGroup`] or [`Select`]: a stable application key and the
/// caption shown to users and assistive technology.
#[must_use = "add the choice to a radio group or select"]
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub(crate) key: Key,
    pub(crate) label: String,
    pub(crate) disabled: bool,
}
/// Creates an enabled option. Keys must be unique within one group or select;
/// duplicates fail preparation with [`crate::UiError::DuplicateKey`].
pub fn choice(key: impl Into<Key>, label: impl Into<String>) -> Choice {
    Choice {
        key: key.into(),
        label: label.into(),
        disabled: false,
    }
}
impl Choice {
    /// Disabled options are shown but cannot be chosen, focused or reached with
    /// arrow keys or type-ahead.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// Internal listener that runs outside any component scope.
pub(crate) fn callback<E: 'static>(
    f: impl Fn(&E, &mut AppContext<'_>) -> Result<Dispatch, AccessError> + 'static,
) -> Listener<E> {
    Listener::from_callback(f)
}

/// Dispatches an optional application listener, reporting Unchanged without one.
pub(crate) fn propose<E: 'static>(
    listener: Option<&Listener<E>>,
    event: &E,
    cx: &mut AppContext<'_>,
) -> Result<Dispatch, AccessError> {
    listener.map_or(Ok(Dispatch::Unchanged), |l| l.dispatch(event, cx))
}

/// Indicator fill, border and mark colors for a resting or checked control.
pub(crate) fn indicator_colors(on: bool, disabled: bool) -> [ThemeColor; 3] {
    match (on, disabled) {
        (true, false) => [
            ThemeColor::Accent,
            ThemeColor::Accent,
            ThemeColor::AccentText,
        ],
        (false, false) => [
            ThemeColor::Input,
            ThemeColor::InputBorder,
            ThemeColor::TextMuted,
        ],
        (true, true) => [
            ThemeColor::TextDisabled,
            ThemeColor::TextDisabled,
            ThemeColor::ControlDisabled,
        ],
        (false, true) => [
            ThemeColor::ControlDisabled,
            ThemeColor::BorderDisabled,
            ThemeColor::TextDisabled,
        ],
    }
}

/// Forwards identity, sizing and naming builders to a control's root element.
macro_rules! root_builders {
    ($ty:ty) => {
        impl $ty {
            /// Stable identity within the parent's sibling scope.
            pub fn key(mut self, key: impl Into<$crate::Key>) -> Self {
                self.root.key = Some(key.into());
                self
            }
            /// Accessible name. Captions supply one automatically.
            pub fn accessibility_label(mut self, label: impl Into<std::sync::Arc<str>>) -> Self {
                self.root = self.root.accessibility_label(label);
                self
            }
            /// Supplemental accessibility help, such as a tooltip's text.
            pub fn accessibility_description(
                mut self,
                description: impl Into<std::sync::Arc<str>>,
            ) -> Self {
                self.root = self.root.accessibility_description(description);
                self
            }
            /// Fixed logical width.
            pub fn width(mut self, width: f32) -> Self {
                self.root = self.root.width(width);
                self
            }
            /// Fills the parent's content width.
            pub fn fill_width(mut self) -> Self {
                self.root = self.root.fill_width();
                self
            }
            /// Share of free space along a flex parent's main axis.
            pub fn flex_grow(mut self, factor: f32) -> Self {
                self.root = self.root.flex_grow(factor);
                self
            }
            /// Minimum logical width.
            pub fn min_width(mut self, width: f32) -> Self {
                self.root = self.root.min_width(width);
                self
            }
            /// Full Taffy customization of the root element.
            pub fn layout(mut self, configure: impl FnOnce(&mut $crate::taffy::Style)) -> Self {
                self.root = self.root.layout(configure);
                self
            }
        }
    };
}
pub(crate) use root_builders;

/// Placement-local state slot shared by the stateful builders.
#[track_caller]
pub(crate) fn local<T, S: Default + 'static>(
    cx: &ViewContext<'_, T>,
    key: impl std::hash::Hash,
) -> crate::Entity<S> {
    cx.use_keyed_state(key, |_| S::default())
}

/// Builds a caption label, omitting it when empty. Inside a button it inherits
/// the button's normal, hover, pressed or disabled foreground.
pub(crate) fn caption(text: &str) -> Option<Element> {
    (!text.is_empty()).then(|| crate::label(text))
}
