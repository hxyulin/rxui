//! Number fields: a text input for a number with a stepper.
use super::{Part, callback, local, part, propose, root_builders};
use crate::{
    ButtonVariant, Dispatch, Element, Entity, FocusEvent, Icon, IntoElement, KeyInput, KeyboardKey,
    Listener, NumericValue, PaintStyle, PointerButton, PointerInput, SemanticAction, SemanticRole,
    TextChangeEvent, ThemeColor, ViewContext, button, column, row, text_input,
};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;
use taffy::prelude::{AlignItems, JustifyContent};

/// Width of the stepper inside a number field's border, excluding its divider.
pub(crate) const STEPPER_WIDTH: f32 = 20.;
/// Time a stepper button is held before it repeats.
const REPEAT_DELAY: Duration = Duration::from_millis(400);
/// Time between repeated steps while a stepper button is held.
const REPEAT_INTERVAL: Duration = Duration::from_millis(60);

/// How a [`NumberEvent`] was proposed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberPhase {
    /// Up, Down, PageUp or PageDown.
    Keyboard,
    /// A stepper button press, or a repeat while it is held.
    Step,
    /// Typed text committed with Enter or by leaving the field.
    Commit,
    /// Assistive SetValue, Increment or Decrement.
    Accessibility,
}

/// Controlled value proposal from a [`NumberField`]. The value is within the
/// range and rounded to the precision; store it in `on_change` to accept it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberEvent {
    /// Proposed value.
    pub value: f64,
    /// What proposed it.
    pub phase: NumberPhase,
}

/// A held stepper button.
#[derive(Clone)]
struct Held {
    up: bool,
    /// Frame time of the next repeat, set by the first evaluation after the press.
    next: Rc<Cell<Option<Duration>>>,
}

/// Placement-local draft text and stepper state.
#[derive(Default)]
pub(crate) struct NumberState {
    /// Text the user is editing, until it is committed or reverted.
    draft: Option<String>,
    held: Option<Held>,
}

/// Controlled number field builder: a text input showing the formatted value,
/// with a stepper of two small buttons at its end; see [`number_field`].
#[must_use = "attach the number field to a parent or return it from View::view"]
pub struct NumberField {
    root: Element,
    state: Entity<NumberState>,
    draft: Option<String>,
    held: Option<Held>,
    now: Duration,
    /// A held stepper button is due to repeat.
    repeat: bool,
    clock: crate::context::FrameClock,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    precision: Option<usize>,
    disabled: bool,
    read_only: bool,
    change: Option<Listener<NumberEvent>>,
}

/// Creates a number field showing `value`, 120 logical units wide, with a step
/// of 1 and no range limit. Its draft text and stepper state belong to this
/// call site in the current placement; number fields built in a loop need
/// [`number_field_keyed`].
///
/// Up and Down step by `step`, and PageUp and PageDown by ten steps, from the
/// typed text when it parses and from the value otherwise. Typing edits a local
/// draft without proposing anything. Enter, or focus leaving the field, commits
/// the draft: text that parses as a number proposes it clamped to the range and
/// rounded to the precision, and other text reverts to the value. Escape reverts
/// the draft; without one, Enter and Escape are left to an enclosing form or
/// dialog. Pressing a stepper button steps once, then repeats every 60 ms after
/// it is held for 400 ms. The stepper buttons keep focus where it is and are not
/// Tab stops.
///
/// The input has the SpinButton role with the numeric value, range and step,
/// and accepts assistive SetValue, Increment and Decrement. Assistive values
/// are `f32`, so they lose precision beyond about seven significant digits.
///
/// ```
/// use rxui::prelude::*;
/// struct Editor { tab: f64 }
/// impl View for Editor {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         number_field(cx, self.tab).range(1., 16.)
///             .accessibility_label("Tab width")
///             .on_change(cx.listener(|s, e: &NumberEvent, _| s.tab = e.value))
///     }
/// }
/// ```
#[track_caller]
pub fn number_field<T>(cx: &ViewContext<'_, T>, value: f64) -> NumberField {
    number_field_keyed(cx, (), value)
}

/// Like [`number_field`], with separate state per key at one call site.
#[track_caller]
pub fn number_field_keyed<T>(
    cx: &ViewContext<'_, T>,
    key: impl std::hash::Hash,
    value: f64,
) -> NumberField {
    let state: Entity<NumberState> = local(cx, key);
    let read = state.read(cx);
    NumberField {
        root: row().width(120.),
        draft: read.draft.clone(),
        held: read.held.clone(),
        now: cx.frame_time(),
        repeat: false,
        clock: cx.frame_clock(),
        state: state.clone(),
        value,
        min: f64::NEG_INFINITY,
        max: f64::INFINITY,
        step: 1.,
        precision: None,
        disabled: false,
        read_only: false,
        change: None,
    }
    .schedule(cx)
}

impl NumberField {
    /// Inclusive value range.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = min;
        self.max = max;
        self
    }
    /// Increment of the stepper and of Up and Down; 1 by default. It must be
    /// positive and finite, or the field ignores it.
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }
    /// Decimal places shown and kept in proposed values. It defaults to the
    /// decimal places of the step: 0 for a step of 1, 2 for 0.25.
    pub fn precision(mut self, decimals: usize) -> Self {
        self.precision = Some(decimals);
        self
    }
    /// Receives value proposals. Without a listener the field is read-only.
    pub fn on_change(mut self, listener: Listener<NumberEvent>) -> Self {
        self.change = Some(listener);
        self
    }
    /// Disables input and removes the field from Tab traversal.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Keeps the value selectable and focusable but rejects edits and steps.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Notes whether a held stepper button is due to repeat, and asks to be
    /// evaluated again at the next repeat.
    fn schedule<T>(mut self, cx: &ViewContext<'_, T>) -> Self {
        let Some(held) = &self.held else {
            return self;
        };
        let next = match held.next.get() {
            None => self.now + REPEAT_DELAY,
            Some(at) if self.now >= at => {
                self.repeat = true;
                self.now + REPEAT_INTERVAL
            }
            Some(at) => at,
        };
        held.next.set(Some(next));
        cx.request_wakeup_at(next);
        self
    }
    fn rule(&self) -> Rule {
        let step = if self.step.is_finite() && self.step > 0. {
            self.step
        } else {
            1.
        };
        Rule {
            min: self.min,
            max: self.max.max(self.min),
            step,
            precision: self.precision.unwrap_or_else(|| decimals(step)).min(15),
        }
    }
}
root_builders!(NumberField);

/// Decimal places needed to show `step` exactly, up to 6.
fn decimals(step: f64) -> usize {
    (0..6)
        .find(|d| {
            let scaled = step * 10f64.powi(*d as i32);
            (scaled - scaled.round()).abs() < 1e-9 * scaled.abs().max(1.)
        })
        .unwrap_or(6)
}

/// Range, step and precision: the arithmetic every input path shares.
#[derive(Clone, Copy)]
pub(crate) struct Rule {
    min: f64,
    max: f64,
    step: f64,
    precision: usize,
}
impl Rule {
    /// `value` rounded to the precision and clamped to the range.
    pub(crate) fn constrain(self, value: f64) -> f64 {
        let scale = 10f64.powi(self.precision as i32);
        let rounded = (value * scale).round() / scale;
        let value = if rounded.is_finite() { rounded } else { value };
        value.clamp(self.min, self.max)
    }
    /// `value` with the precision's decimal places, without a negative zero.
    pub(crate) fn format(self, value: f64) -> String {
        let text = format!("{:.*}", self.precision, value);
        match text.strip_prefix('-') {
            Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.into(),
            _ => text,
        }
    }
    /// The typed text as a constrained value, or `None` when it is not a finite
    /// number.
    pub(crate) fn parse(self, text: &str) -> Option<f64> {
        let value: f64 = text.trim().parse().ok()?;
        value.is_finite().then(|| self.constrain(value))
    }
}

/// What the field's listeners share.
struct Shared {
    state: Entity<NumberState>,
    rule: Rule,
    value: f64,
    change: Option<Listener<NumberEvent>>,
}
impl Shared {
    /// Takes the draft, if any.
    fn take_draft(
        &self,
        cx: &mut crate::AppContext<'_>,
    ) -> Result<Option<String>, crate::AccessError> {
        self.state.try_update(cx, |s, cx| {
            if s.draft.is_none() {
                cx.unchanged();
            }
            s.draft.take()
        })
    }
    /// Proposes `value` unless it is the current value.
    fn propose(
        &self,
        cx: &mut crate::AppContext<'_>,
        value: f64,
        phase: NumberPhase,
    ) -> Result<Dispatch, crate::AccessError> {
        if value == self.value {
            return Ok(Dispatch::Unchanged);
        }
        propose(self.change.as_ref(), &NumberEvent { value, phase }, cx)
    }
    /// Moves `steps` steps from the draft, when it parses, or from the value.
    fn step(
        &self,
        cx: &mut crate::AppContext<'_>,
        steps: f64,
        phase: NumberPhase,
    ) -> Result<Dispatch, crate::AccessError> {
        let draft = self.take_draft(cx)?;
        let base = draft
            .as_deref()
            .and_then(|d| self.rule.parse(d))
            .unwrap_or(self.value);
        let next = self.rule.constrain(base + steps * self.rule.step);
        self.propose(cx, next, phase)?;
        Ok(Dispatch::Handled)
    }
    /// Commits the draft, or reverts it when it does not parse.
    fn commit(&self, cx: &mut crate::AppContext<'_>) -> Result<Dispatch, crate::AccessError> {
        let Some(draft) = self.take_draft(cx)? else {
            return Ok(Dispatch::Unchanged);
        };
        if let Some(value) = self.rule.parse(&draft) {
            self.propose(cx, value, NumberPhase::Commit)?;
        }
        Ok(Dispatch::Handled)
    }
}

/// One half of the stepper. A press steps without moving focus and holds the
/// button for repeats until release.
fn stepper_button(shared: &Rc<Shared>, up: bool, enabled: bool) -> Element {
    let glyph = crate::icon(if up {
        Icon::CHEVRON_UP
    } else {
        Icon::CHEVRON_DOWN
    })
    .size(10., 10.);
    let mut button = button(glyph)
        .key(if up { "increment" } else { "decrement" })
        .variant(ButtonVariant::Quiet)
        .padding(0.)
        .border(0., ThemeColor::Border)
        .width(STEPPER_WIDTH)
        .flex_grow(1.)
        .flex_basis(0.)
        .align_items(AlignItems::CENTER)
        .justify_content(JustifyContent::CENTER)
        .focusable(false)
        .tab_stop(false)
        .disabled(!enabled)
        // A fill would cross the field's rounded border; the glyph brightens instead.
        .hover_style(PaintStyle::new().no_background().color(ThemeColor::Text))
        .pressed_style(PaintStyle::new().no_background().color(ThemeColor::Text));
    if !enabled {
        return button;
    }
    let press = shared.clone();
    button = button.on_pointer_down(callback(move |event: &PointerInput, cx| {
        if event.button != Some(PointerButton::Primary) {
            return Ok(Dispatch::Unchanged);
        }
        event.prevent_default();
        event.capture_pointer();
        press.state.try_update(cx, |s, _| {
            s.held = Some(Held {
                up,
                next: Rc::default(),
            });
        })?;
        press.step(cx, if up { 1. } else { -1. }, NumberPhase::Step)
    }));
    let release = |shared: Rc<Shared>| {
        callback(move |_: &PointerInput, cx| {
            shared.state.try_update(cx, |s, cx| {
                if s.held.take().is_none() {
                    cx.unchanged();
                    return Dispatch::Unchanged;
                }
                Dispatch::Handled
            })
        })
    };
    button
        .on_pointer_up(release(shared.clone()))
        .on_pointer_cancel(release(shared.clone()))
}

impl IntoElement for NumberField {
    fn into_element(self) -> Element {
        let rule = self.rule();
        let editable = !self.disabled && !self.read_only && self.change.is_some();
        let text = self
            .draft
            .clone()
            .unwrap_or_else(|| rule.format(self.value));
        let shared = Rc::new(Shared {
            state: self.state.clone(),
            rule,
            value: self.value,
            change: self.change.clone(),
        });
        // Assistive values are f32; keep unbounded ends finite.
        let narrow = |v: f64| v.clamp(f64::from(f32::MIN), f64::from(f32::MAX)) as f32;
        let mut input = part(text_input(text), Part::NumberInput)
            .key("input")
            .fill_width()
            .disabled(self.disabled)
            .read_only(!editable && !self.disabled)
            .accessibility_role(SemanticRole::SpinButton)
            .accessibility_numeric_value(NumericValue {
                value: narrow(self.value),
                min: narrow(rule.min),
                max: narrow(rule.max),
                step: Some(narrow(rule.step)).filter(|s| *s > 0.),
            });
        // Naming builders target the root; the input is what users reach.
        let mut root = self.root;
        if let Some(naming) = root.semantics.take() {
            let semantics = input.semantics.get_or_insert_with(Default::default);
            semantics.label = naming.label;
            semantics.description = naming.description;
            semantics.labelled_by = naming.labelled_by;
        }
        if editable {
            let edit = shared.clone();
            input = input.on_change(callback(move |event: &TextChangeEvent, cx| {
                let value = event.value.clone();
                edit.state.try_update(cx, |s, _| s.draft = Some(value))?;
                Ok(Dispatch::Handled)
            }));
            let blur = shared.clone();
            input = input.on_focus_change(callback(move |event: &FocusEvent, cx| {
                if event.focused {
                    return Ok(Dispatch::Unchanged);
                }
                blur.commit(cx)
            }));
            let assist = shared.clone();
            input = input.on_semantic_action(callback(move |action: &SemanticAction, cx| {
                let SemanticAction::SetNumericValue { value, .. } = *action else {
                    return Ok(Dispatch::Unchanged);
                };
                if !value.is_finite() {
                    return Ok(Dispatch::Unchanged);
                }
                assist.take_draft(cx)?;
                let value = assist.rule.constrain(f64::from(value));
                assist.propose(cx, value, NumberPhase::Accessibility)
            }));
            let keys = shared.clone();
            root = root.on_key_down(callback(move |input: &KeyInput, cx| {
                let m = input.event.modifiers;
                if m.alt || m.control || m.meta {
                    return Ok(Dispatch::Unchanged);
                }
                let steps = match input.event.key {
                    KeyboardKey::ArrowUp => 1.,
                    KeyboardKey::ArrowDown => -1.,
                    KeyboardKey::PageUp => 10.,
                    KeyboardKey::PageDown => -10.,
                    KeyboardKey::Enter => {
                        let dispatch = keys.commit(cx)?;
                        if dispatch == Dispatch::Handled {
                            input.prevent_default();
                        }
                        return Ok(dispatch);
                    }
                    KeyboardKey::Escape => {
                        if keys.take_draft(cx)?.is_none() {
                            return Ok(Dispatch::Unchanged);
                        }
                        input.prevent_default();
                        return Ok(Dispatch::Handled);
                    }
                    _ => return Ok(Dispatch::Unchanged),
                };
                input.prevent_default();
                keys.step(cx, steps, NumberPhase::Keyboard)
            }));
        }
        if let Some(held) = self.held.filter(|_| self.repeat && editable) {
            // Proposals cannot run while the view is evaluated, so the repeat
            // runs at the next flush, unless the button is released first.
            let shared = shared.clone();
            self.clock.defer(move |cx| {
                let current = shared.state.read(cx).held.as_ref().map(|h| h.next.clone());
                if current.is_some_and(|c| Rc::ptr_eq(&c, &held.next)) {
                    let steps = if held.up { 1. } else { -1. };
                    let _ = shared.step(cx, steps, NumberPhase::Step);
                }
            });
        }
        let stepper = row()
            .key("stepper")
            .absolute()
            .top(1.)
            .bottom(1.)
            .right(1.)
            .accessibility_hidden(true)
            .child(
                row()
                    .key("divider")
                    .width(1.)
                    .fill_height()
                    .background(ThemeColor::Divider),
            )
            .child(
                column()
                    .fill_height()
                    .child(stepper_button(&shared, true, editable))
                    .child(stepper_button(&shared, false, editable)),
            );
        root.child(input).child(stepper)
    }
}
