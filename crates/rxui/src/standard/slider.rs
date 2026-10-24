//! Controlled horizontal sliders.
use super::{Part, callback, part, propose, root_builders};
use crate::{
    ButtonVariant, Dispatch, Element, IntoElement, KeyInput, KeyboardKey, Listener, NumericValue,
    PaintStyle, PointerButton, PointerInput, SemanticAction, SemanticRole, ThemeColor, button, row,
    stack,
};

/// Stage of a slider proposal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliderPhase {
    /// Primary press on the slider; the value jumps to the pointer.
    Begin,
    /// Captured pointer motion.
    Drag,
    /// Release ended the drag.
    End,
    /// The gesture was cancelled by Escape, the host or removal. The proposal
    /// repeats the current value; store the value seen at Begin to roll back.
    Cancel,
    /// Arrow, Page or Home/End key.
    Keyboard,
    /// Assistive SetValue, Increment or Decrement.
    Accessibility,
}

/// Controlled value proposal. The value is clamped to the range and snapped to the
/// step; store it in `on_change` to accept it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderEvent {
    /// Proposed value.
    pub value: f32,
    /// Gesture or action stage.
    pub phase: SliderPhase,
}

/// Horizontal space between the border box and the ends of the rail.
const PAD: f32 = 2.;
/// Inset of the thumb's center travel inside the content box: half the balanced
/// thumb. It does not change with density, so pointer positions map to values
/// without the theme; a smaller compact thumb stops a little short of the ends.
const TRAVEL: f32 = 8.;
const INSET: f32 = PAD + TRAVEL;

/// Controlled slider builder. It is a focusable button with the Slider role and
/// a numeric value: dragging captures the pointer, arrow keys move by the step,
/// PageUp/PageDown by the page step and Home/End to the ends. Assistive
/// SetValue/Increment/Decrement use the same listener. The range must be finite
/// with `min <= max` and a positive step; invalid ranges fail preparation with
/// [`crate::UiError::InvalidStyle`].
///
/// ```
/// use rxui::prelude::*;
/// struct Mixer { gain: f32 }
/// impl View for Mixer {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         slider(self.gain).key("gain").range(-12., 12.).step(0.5)
///             .accessibility_label("Gain")
///             .on_change(cx.listener(|s, e: &SliderEvent, _| s.gain = e.value))
///     }
/// }
/// ```
#[must_use = "attach the slider to a parent or return it from View::view"]
pub struct Slider {
    root: Element,
    value: f32,
    min: f32,
    max: f32,
    step: Option<f32>,
    page: Option<f32>,
    disabled: bool,
    change: Option<Listener<SliderEvent>>,
}

/// Creates a 0..=1 slider without snapping, 200 logical units wide.
pub fn slider(value: f32) -> Slider {
    Slider {
        root: button(row())
            .variant(ButtonVariant::Quiet)
            .padding_x(PAD)
            .padding_y(4.)
            .border(0., ThemeColor::Border)
            .width(200.)
            .hover_style(PaintStyle::new().no_background())
            .pressed_style(PaintStyle::new().no_background()),
        value,
        min: 0.,
        max: 1.,
        step: None,
        page: None,
        disabled: false,
        change: None,
    }
}

impl Slider {
    /// Inclusive value range.
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = min;
        self.max = max;
        self
    }
    /// Snapping increment, also used by arrow keys and assistive increments. Without
    /// a step, values are continuous and keys move by one hundredth of the range.
    pub fn step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self
    }
    /// PageUp/PageDown increment; defaults to a tenth of the range, at least one step.
    pub fn page_step(mut self, page: f32) -> Self {
        self.page = Some(page);
        self
    }
    /// Receives value proposals. Without a listener the slider is read-only but
    /// still focusable.
    pub fn on_change(mut self, listener: Listener<SliderEvent>) -> Self {
        self.change = Some(listener);
        self
    }
    /// Disables input and removes the slider from Tab traversal.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}
root_builders!(Slider);

/// Validated range and the proposal arithmetic shared by every input path.
#[derive(Clone, Copy)]
struct Range {
    min: f32,
    max: f32,
    step: Option<f32>,
    page: f32,
}
impl Range {
    fn constrain(self, value: f32) -> f32 {
        let value = value.clamp(self.min, self.max);
        match self.step {
            Some(step) => {
                (self.min + ((value - self.min) / step).round() * step).clamp(self.min, self.max)
            }
            None => value,
        }
    }
    fn increment(self) -> f32 {
        self.step.unwrap_or((self.max - self.min) / 100.)
    }
    fn fraction(self, value: f32) -> f32 {
        if self.max > self.min {
            (value - self.min) / (self.max - self.min)
        } else {
            0.
        }
    }
    fn at_pointer(self, event: &PointerInput) -> f32 {
        let track = (event.bounds.width - 2. * INSET).max(1.);
        let t = ((event.position[0] - event.bounds.x - INSET) / track).clamp(0., 1.);
        self.constrain(self.min + t * (self.max - self.min))
    }
}

impl IntoElement for Slider {
    fn into_element(self) -> Element {
        let valid = [self.value, self.min, self.max]
            .iter()
            .all(|v| v.is_finite())
            && self.min <= self.max
            && self.step.is_none_or(|s| s.is_finite() && s > 0.)
            && self.page.is_none_or(|p| p.is_finite() && p > 0.);
        let range = Range {
            min: self.min,
            max: self.max,
            step: self.step,
            page: 0.,
        };
        let range = Range {
            page: self
                .page
                .unwrap_or_else(|| ((self.max - self.min) / 10.).max(range.increment())),
            ..range
        };
        let value = if valid {
            self.value.clamp(self.min, self.max)
        } else {
            self.value
        };
        let fraction = if valid { range.fraction(value) } else { 0. };
        // Outlined rail and thumb keep the filled part distinct in every preset,
        // including high contrast where the accent and border are both light. The
        // outline is TextMuted, since input borders stay below 3:1.
        let (surface, edge, fill) = if self.disabled {
            (
                ThemeColor::ControlDisabled,
                ThemeColor::BorderDisabled,
                ThemeColor::TextDisabled,
            )
        } else {
            (ThemeColor::Input, ThemeColor::TextMuted, ThemeColor::Accent)
        };
        let content = part(stack(), Part::SliderBody).fill_width().child(
            stack()
                .key("rail")
                .absolute()
                .left(TRAVEL)
                .right(TRAVEL)
                .top(0.)
                .bottom(0.)
                .child(
                    part(row(), Part::Rail)
                        .key("track")
                        .absolute()
                        .left(0.)
                        .right(0.)
                        .background(surface)
                        .border(1., edge)
                        .clip()
                        .child(
                            row()
                                .key("fill")
                                .fill_height()
                                .width_percent(fraction)
                                .background(fill),
                        ),
                )
                .child(
                    part(row(), Part::Thumb)
                        .key("thumb")
                        .absolute()
                        .top(0.)
                        .background(surface)
                        .border(1., edge)
                        .layout(|s| {
                            s.inset.left = taffy::LengthPercentageAuto::percent(fraction);
                        }),
                ),
        );
        let mut root = self.root;
        root.children = vec![content.accessibility_hidden(true)];
        root = root
            .disabled(self.disabled)
            .accessibility_role(SemanticRole::Slider)
            .accessibility_numeric_value(NumericValue {
                value,
                min: self.min,
                max: self.max,
                // An invalid step is published as given so preparation reports it.
                step: if valid {
                    Some(range.increment()).filter(|s| *s > 0.)
                } else {
                    self.step
                },
            });
        let Some(change) = self.change else {
            return root;
        };
        fn send(
            change: &Listener<SliderEvent>,
            value: f32,
            phase: SliderPhase,
            cx: &mut crate::AppContext<'_>,
        ) -> Result<Dispatch, crate::AccessError> {
            change.dispatch(&SliderEvent { value, phase }, cx)
        }
        let listener = change.clone();
        root = root.on_pointer_down(callback(move |event: &PointerInput, cx| {
            if event.button != Some(PointerButton::Primary) {
                return Ok(Dispatch::Unchanged);
            }
            event.capture_pointer();
            event.focus();
            event.prevent_default();
            send(&listener, range.at_pointer(event), SliderPhase::Begin, cx)?;
            Ok(Dispatch::Handled)
        }));
        let listener = change.clone();
        root = root.on_pointer_move(callback(move |event: &PointerInput, cx| {
            if event.press_position.is_none() {
                return Ok(Dispatch::Unchanged);
            }
            let next = range.at_pointer(event);
            if next == value {
                return Ok(Dispatch::Unchanged);
            }
            send(&listener, next, SliderPhase::Drag, cx)
        }));
        let listener = change.clone();
        root = root.on_pointer_up(callback(move |event: &PointerInput, cx| {
            if event.press_position.is_none() || event.button != Some(PointerButton::Primary) {
                return Ok(Dispatch::Unchanged);
            }
            send(&listener, range.at_pointer(event), SliderPhase::End, cx)
        }));
        let listener = change.clone();
        root = root.on_pointer_cancel(callback(move |_: &PointerInput, cx| {
            send(&listener, value, SliderPhase::Cancel, cx)
        }));
        let listener = change.clone();
        root = root.on_key_down(callback(move |input: &KeyInput, cx| {
            let m = input.event.modifiers;
            if m.control || m.alt || m.meta {
                return Ok(Dispatch::Unchanged);
            }
            let next = match input.event.key {
                KeyboardKey::ArrowRight | KeyboardKey::ArrowUp => value + range.increment(),
                KeyboardKey::ArrowLeft | KeyboardKey::ArrowDown => value - range.increment(),
                KeyboardKey::PageUp => value + range.page,
                KeyboardKey::PageDown => value - range.page,
                KeyboardKey::Home => range.min,
                KeyboardKey::End => range.max,
                _ => return Ok(Dispatch::Unchanged),
            };
            input.prevent_default();
            let next = range.constrain(next);
            if next == value {
                return Ok(Dispatch::Unchanged);
            }
            send(&listener, next, SliderPhase::Keyboard, cx)
        }));
        root.on_semantic_action(callback(move |action: &SemanticAction, cx| {
            let SemanticAction::SetNumericValue { value: next, .. } = *action else {
                return Ok(Dispatch::Unchanged);
            };
            if !next.is_finite() {
                return Ok(Dispatch::Unchanged);
            }
            propose(
                Some(&change),
                &SliderEvent {
                    value: range.constrain(next),
                    phase: SliderPhase::Accessibility,
                },
                cx,
            )
        }))
    }
}
