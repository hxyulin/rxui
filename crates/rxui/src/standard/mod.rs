//! Standard form controls: check boxes, switches, radio groups, sliders, progress
//! indicators, selects and tooltips, and the small parts they share: separators,
//! badges and key caps.
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
mod small;
mod toggle;
mod tooltip;

pub use progress::{ProgressBar, progress_bar, spinner};
pub use radio::{RadioGroup, radio_group};
pub use select::{Select, select, select_keyed};
pub use slider::{Slider, SliderEvent, SliderPhase, slider};
pub use small::{Badge, Separator, Tone, badge, kbd, separator};
pub use toggle::{Toggle, checkbox, switch};
pub use tooltip::{TOOLTIP_DELAY, tooltip};

#[cfg(all(test, feature = "rendering"))]
mod gpu_tests;
#[cfg(test)]
mod small_tests;
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
/// Unchecked outlines use TextMuted: input borders are soft and stay below the
/// 3:1 non-text contrast an indicator's boundary needs.
pub(crate) fn indicator_colors(on: bool, disabled: bool) -> [ThemeColor; 3] {
    match (on, disabled) {
        (true, false) => [
            ThemeColor::Accent,
            ThemeColor::Accent,
            ThemeColor::AccentText,
        ],
        (false, false) => [
            ThemeColor::Input,
            ThemeColor::TextMuted,
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

/// A part of a standard control sized from [`crate::ThemeMetrics`] while its
/// theme resolves, so density switches resize it the way they resize buttons.
/// Check boxes and radios follow `indicator_size`; switches, slider thumbs and
/// rails and progress bars follow the switch metrics; icons and separators in a
/// row follow `icon_size`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    /// Check box square.
    CheckBox,
    /// Tick or dash filling a check box's content box.
    Mark,
    /// Radio circle.
    Radio,
    /// Dot of a selected radio.
    RadioDot,
    /// Switch track.
    Switch,
    /// Switch knob inside the track's padding.
    Knob,
    /// Slider content, as tall as the thumb.
    SliderBody,
    /// Slider rail, centered on the thumb.
    Rail,
    /// Slider thumb, centered on its inset.
    Thumb,
    /// Progress bar track.
    Progress,
    /// An icon, `icon_size` square.
    Icon,
    /// A square icon button, `input_height` on each side.
    IconButton,
    /// A separator, `icon_size` tall when it runs across a row.
    Separator,
    /// A key cap, rounded with `radius_small`.
    KeyCap,
}
impl Part {
    /// Switch knob side: five eighths of the track height.
    fn knob(m: &crate::ThemeMetrics) -> f32 {
        (m.switch_height * 5. / 8.).round()
    }
    /// Slider rail thickness: three eighths of the thumb.
    fn rail(m: &crate::ThemeMetrics) -> f32 {
        (m.switch_height * 3. / 8.).round()
    }
    /// Progress bar thickness: half the switch height.
    fn progress(m: &crate::ThemeMetrics) -> f32 {
        (m.switch_height / 2.).floor()
    }
    /// Uniform corner radius; rounded parts are fully round.
    pub(crate) fn radius(self, m: &crate::ThemeMetrics) -> f32 {
        let s = m.indicator_size;
        match self {
            Self::CheckBox => s * 3. / 14.,
            Self::Mark | Self::SliderBody | Self::Icon | Self::Separator => 0.,
            Self::KeyCap => m.radius_small,
            Self::IconButton => m.radius,
            Self::Radio => s / 2.,
            Self::RadioDot => s * 3. / 14.,
            Self::Switch | Self::Thumb => m.switch_height / 2.,
            Self::Knob => Self::knob(m) / 2.,
            Self::Rail => Self::rail(m) / 2.,
            Self::Progress => Self::progress(m) / 2.,
        }
    }
    /// Writes the part's sizes into its layout, keeping a width, height or
    /// padding the element sets explicitly (`overrides` bits 2, 4 and 1).
    pub(crate) fn layout(self, m: &crate::ThemeMetrics, style: &mut taffy::Style, overrides: u8) {
        use taffy::prelude::{auto, length};
        let (s, h) = (m.indicator_size, m.switch_height);
        let mark = (s - 2.).max(0.);
        let [width, height] = match self {
            Self::CheckBox | Self::Radio => [Some(s); 2],
            Self::Mark => [Some(mark); 2],
            Self::RadioDot => [Some(s * 3. / 7.); 2],
            Self::Switch => [Some(m.switch_width), Some(h)],
            Self::Knob => [Some(Self::knob(m)); 2],
            Self::Thumb => [Some(h); 2],
            Self::SliderBody => [None, Some(h)],
            Self::Rail => [None, Some(Self::rail(m))],
            Self::Progress => [None, Some(Self::progress(m))],
            Self::Icon => [Some(m.icon_size); 2],
            Self::IconButton => [Some(m.input_height); 2],
            Self::Separator => [None, Some(m.icon_size)],
            Self::KeyCap => [None; 2],
        };
        if let Some(width) = width
            && overrides & 2 == 0
        {
            style.size.width = length(width);
        }
        if let Some(height) = height
            && overrides & 4 == 0
        {
            style.size.height = length(height);
        }
        match self {
            // The knob sits inside the one-unit border and an even padding.
            Self::Switch if overrides & 1 == 0 => {
                let pad = length(((h - 2. - Self::knob(m)) / 2.).max(0.));
                style.padding = taffy::Rect {
                    left: pad,
                    right: pad,
                    top: pad,
                    bottom: pad,
                };
            }
            Self::Rail => {
                style.inset.top = length((h - Self::rail(m)) / 2.);
                style.inset.bottom = auto();
            }
            Self::Thumb => style.margin.left = length(-h / 2.),
            _ => {}
        }
    }
}

/// Marks `element` as a theme-sized [`Part`].
pub(crate) fn part(mut element: Element, part: Part) -> Element {
    element.surface = crate::element::Surface::Part(part);
    element
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
            /// Uses another semantic element as this control's accessible name.
            pub fn accessibility_labelled_by(mut self, label: crate::AccessibilityHandle) -> Self {
                self.root = self.root.accessibility_labelled_by(label);
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

/// A quiet button whose caption keeps the ordinary text color, for controls
/// whose indicator rather than their caption shows the state.
pub(crate) fn quiet_root(button: Element) -> Element {
    button
        .variant(crate::ButtonVariant::Quiet)
        .color(ThemeColor::Text)
        .disabled_style(crate::PaintStyle::new().color(ThemeColor::TextDisabled))
}

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
