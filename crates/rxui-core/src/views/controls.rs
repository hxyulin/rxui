//! Controlled leaf controls: buttons, fields, checkboxes, sliders.

use std::{ops::RangeInclusive, sync::Arc};

use astrelis_core::{color::Color, geometry::LogicalSize};
use astrelis_ui_next::{
    Button, ButtonIcon, Checkbox, Invalidation, NodeHandle, Slider, TextField, UiError,
};

use crate::{
    ButtonStyle, ButtonVariant, ColorRole, Icon, IconButtonStyle, View,
    view::{ActionCell, AnyView, MapCell, Mounted, ViewContext, ViewNode, leaf_mounted_state},
};
/// Creates an activatable typed-action button.
pub fn button<Action: Clone + 'static>(text: impl Into<String>, action: Action) -> AnyView<Action> {
    button_with(text, action, ButtonStyle::standard())
}

/// Creates a button with typed presentation options.
pub fn button_with<Action: Clone + 'static>(
    text: impl Into<String>,
    action: Action,
    style: ButtonStyle,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(ButtonView {
            text: text.into(),
            action,
            variant: style.variant,
            size: style.size,
            icon: None,
            icon_size: 0.0,
            show_label: true,
        }),
    }
}

pub(crate) fn icon_button_view<Action: Clone + 'static>(
    icon: Icon,
    label: String,
    action: Action,
    style: IconButtonStyle,
) -> View<Action> {
    AnyView {
        key: None,
        inner: Box::new(ButtonView {
            text: label,
            action,
            variant: style.button.variant,
            size: style.button.size,
            icon: Some(icon),
            icon_size: style.icon_size,
            show_label: style.show_label,
        }),
    }
}

/// Creates a controlled editable text field.
pub fn text_field<Action: 'static>(
    label: impl Into<String>,
    value: impl Into<String>,
    on_changed: impl Fn(String) -> Action + 'static,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(TextFieldView {
            label: label.into(),
            value: value.into(),
            on_changed: Arc::new(on_changed),
        }),
    }
}

/// Creates a controlled boolean checkbox.
pub fn checkbox<Action: 'static>(
    label: impl Into<String>,
    checked: bool,
    on_changed: impl Fn(bool) -> Action + 'static,
) -> View<Action> {
    AnyView {
        key: None,
        inner: Box::new(CheckboxView {
            label: label.into(),
            checked,
            on_changed: Arc::new(on_changed),
        }),
    }
}

/// Creates a controlled horizontal numeric slider.
pub fn slider<Action: 'static>(
    label: impl Into<String>,
    value: f32,
    range: RangeInclusive<f32>,
    on_changed: impl Fn(f32) -> Action + 'static,
) -> View<Action> {
    slider_with_step(label, value, range, 1.0, on_changed)
}

/// Creates a controlled slider with an explicit keyboard step.
pub fn slider_with_step<Action: 'static>(
    label: impl Into<String>,
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    on_changed: impl Fn(f32) -> Action + 'static,
) -> View<Action> {
    AnyView {
        key: None,
        inner: Box::new(SliderView {
            label: label.into(),
            value,
            range,
            step: step.max(0.0),
            on_changed: Arc::new(on_changed),
        }),
    }
}
struct ButtonView<Action> {
    text: String,
    action: Action,
    variant: ButtonVariant,
    size: LogicalSize,
    icon: Option<Icon>,
    icon_size: f32,
    show_label: bool,
}

struct ButtonState<Action: Clone + 'static> {
    handle: NodeHandle<Button>,
    text: String,
    variant: ButtonVariant,
    size: LogicalSize,
    colors: (Color, Color),
    icon: Option<(Icon, f32)>,
    show_label: bool,
    action: ActionCell<Action>,
}

leaf_mounted_state!(ButtonState<Action> where Action: Clone);

/// Pairs a button's glyph with the edge the retained control will resolve.
///
/// The `Icon` is compared by its verbs rather than by `Path::cache_id`, which is
/// a per-allocation counter: keying on the counter made every `icon_button`
/// re-run `LAYOUT_ALL` on every pass, because the icon constructors allocate a
/// fresh `Path` each time `view()` calls them.
fn button_icon_state(icon: Option<&Icon>, icon_size: f32) -> Option<(Icon, f32)> {
    icon.map(|icon| (icon.clone(), resolved_icon_size(icon_size)))
}

/// Mirrors the glyph edge `Button::layout` derives from a requested size.
///
/// Resolving it here rather than at the comparison site is what lets two
/// requests the control cannot distinguish - a negative edge and a NaN one both
/// land on the same fallback - compare equal instead of forcing a relayout.
fn resolved_icon_size(icon_size: f32) -> f32 {
    if icon_size.is_finite() {
        icon_size.max(1.0)
    } else {
        16.0
    }
}

/// Projects the part of a button's icon state that `Button::layout` measures.
fn button_icon_edge(icon: Option<&(Icon, f32)>) -> Option<f32> {
    icon.map(|(_, edge)| *edge)
}

/// Rebuilds the retained glyph a button paints.
fn button_icon(icon: Option<&(Icon, f32)>) -> Option<ButtonIcon> {
    icon.map(|(icon, edge)| {
        ButtonIcon::new(icon.path.clone(), icon.view_box, *edge).with_fill_rule(icon.fill_rule)
    })
}

impl<Action: Clone + 'static> ViewNode<Action> for ButtonView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let colors = context.theme().button(self.variant);
        let action = ActionCell::new(self.action, context.emitter());
        let emit = action.emitter();
        let icon_state = button_icon_state(self.icon.as_ref(), self.icon_size);
        let mut button = Button::with_action_factory(
            self.text.clone(),
            self.size,
            colors.0,
            colors.1,
            move || emit(),
        )
        .with_label_visible(self.show_label);
        if let Some(icon) = button_icon(icon_state.as_ref()) {
            button = button.with_icon(icon);
        }
        let handle = context.append(button)?;
        Ok(Mounted::new(
            handle.id(),
            ButtonState {
                handle,
                text: self.text,
                variant: self.variant,
                size: self.size,
                colors,
                icon: icon_state,
                show_label: self.show_label,
                action,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let colors = context.theme().button(self.variant);
        let emitter = context.emitter();
        let icon_state = button_icon_state(self.icon.as_ref(), self.icon_size);
        let state = mounted.state_mut::<ButtonState<Action>>()?;
        // The activation action is written into the cell the retained element
        // already reads through, so a changed action no longer reinstalls a
        // boxed closure.
        state.action.update(self.action, &emitter);
        // Label, size, and whether there is a glyph at all feed `Button::layout`,
        // as does the glyph's edge. Its verbs, view box, and winding rule do not:
        // the control measures the square it reserved and scales the path into it,
        // so a different picture at the same edge is a repaint. The two fills are
        // likewise paint-only.
        let icon_changed = state.icon != icon_state;
        let mut invalidation = Invalidation::empty();
        if state.size != self.size
            || state.text != self.text
            || state.show_label != self.show_label
            || button_icon_edge(state.icon.as_ref()) != button_icon_edge(icon_state.as_ref())
        {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if state.colors != colors || icon_changed {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() {
            let text = self.text.clone();
            let size = self.size;
            let show_label = self.show_label;
            let icon = icon_changed.then(|| button_icon(icon_state.as_ref()));
            context.ui().update(state.handle, invalidation, |button| {
                button.label = text;
                button.size = size;
                button.color = colors.0;
                button.pressed_color = colors.1;
                button.show_label = show_label;
                if let Some(icon) = icon {
                    button.icon = icon;
                }
            })?;
        }
        state.text = self.text;
        state.variant = self.variant;
        state.size = self.size;
        state.colors = colors;
        state.icon = icon_state;
        state.show_label = self.show_label;
        Ok(())
    }
}

struct TextFieldView<Action: 'static> {
    label: String,
    value: String,
    on_changed: Arc<dyn Fn(String) -> Action>,
}

struct TextFieldState<Action: 'static> {
    handle: NodeHandle<TextField>,
    label: String,
    // The declared value is deliberately *not* cached here. Editing mutates the
    // element itself, so what it holds can differ from what this view last
    // declared while the declaration stands still, and only the live text is
    // worth comparing against - see `rebuild`.
    colors: (Color, Color),
    changed: MapCell<String, Action>,
}

leaf_mounted_state!(TextFieldState<Action>);

impl<Action: 'static> ViewNode<Action> for TextFieldView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let theme = context.theme();
        let colors = (
            theme.color(ColorRole::Text),
            theme.color(ColorRole::Surface),
        );
        let changed = MapCell::new(self.on_changed, context.emitter());
        let emit = changed.emitter();
        let mut field = TextField::new(self.label.clone(), self.value.clone())
            .on_changed_factory(move |value| emit(value));
        field.text_color = colors.0;
        field.background = colors.1;
        let handle = context.append(field)?;
        Ok(Mounted::new(
            handle.id(),
            TextFieldState {
                handle,
                label: self.label,
                colors,
                changed,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let theme = context.theme();
        let colors = (
            theme.color(ColorRole::Text),
            theme.color(ColorRole::Surface),
        );
        let emitter = context.emitter();
        let state = mounted.state_mut::<TextFieldState<Action>>()?;
        state.changed.update(self.on_changed, &emitter);
        // The value is compared against the element's *live* text, not against
        // the value this view last declared. The two diverge whenever an edit is
        // refused: the element rewrote its own text as the user typed, the
        // component reduced the change and kept its old value, and a guard that
        // trusts the declaration then sees 12 against 12, writes nothing, and
        // leaves text the controller never accepted on screen for good. Reading
        // the element costs one tree lookup and is what makes refusing an edit
        // mean anything at all.
        let stale_value = context.ui().element(state.handle)?.text != self.value;
        // Value, placeholder label, and glyph color all reach the shaper, which
        // runs in layout. Only the field's background is paint-only.
        let mut invalidation = Invalidation::empty();
        if stale_value || state.label != self.label || state.colors.0 != colors.0 {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if state.colors.1 != colors.1 {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() {
            let label = self.label.clone();
            let value = self.value;
            context.ui().update(state.handle, invalidation, |field| {
                field.label = label;
                // `set_text` clamps the retained caret and anchor into the new
                // value, which is what keeps a reverted edit from leaving a
                // selection pointing past the end of the text.
                field.set_text(value);
                field.text_color = colors.0;
                field.background = colors.1;
            })?;
        }
        state.label = self.label;
        state.colors = colors;
        Ok(())
    }
}

struct CheckboxView<Action: 'static> {
    label: String,
    checked: bool,
    on_changed: Arc<dyn Fn(bool) -> Action>,
}

struct CheckboxState<Action: 'static> {
    handle: NodeHandle<Checkbox>,
    label: String,
    // Not cached, for the same reason as `TextFieldState`: `Checkbox::toggle`
    // flips its own field before emitting, so the element and this view's last
    // declaration diverge the moment a controller refuses the change.
    colors: (Color, Color, Color),
    changed: MapCell<bool, Action>,
}

leaf_mounted_state!(CheckboxState<Action>);

impl<Action: 'static> ViewNode<Action> for CheckboxView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let theme = context.theme();
        let colors = (
            theme.color(ColorRole::Text),
            theme.color(ColorRole::Muted),
            theme.color(ColorRole::Accent),
        );
        let changed = MapCell::new(self.on_changed, context.emitter());
        let emit = changed.emitter();
        let mut checkbox = Checkbox::new(self.label.clone(), self.checked, move |checked| {
            emit(checked)
        });
        checkbox.text_color = colors.0;
        checkbox.outline_color = colors.1;
        checkbox.accent_color = colors.2;
        let handle = context.append(checkbox)?;
        Ok(Mounted::new(
            handle.id(),
            CheckboxState {
                handle,
                label: self.label,
                colors,
                changed,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let theme = context.theme();
        let colors = (
            theme.color(ColorRole::Text),
            theme.color(ColorRole::Muted),
            theme.color(ColorRole::Accent),
        );
        let emitter = context.emitter();
        let state = mounted.state_mut::<CheckboxState<Action>>()?;
        state.changed.update(self.on_changed, &emitter);
        // Compared against the element's live state rather than the declaration,
        // which is what makes refusing a toggle mean anything: the checkbox flips
        // itself as the pointer goes down and emits afterwards, so a controller
        // that keeps its old value leaves the two disagreeing while the
        // declaration stands still. Trusting the declaration then writes nothing
        // and the box stays ticked for good.
        let stale_checked = context.ui().element(state.handle)?.checked != self.checked;
        // The label and its glyph color reach the shaper, which runs in layout.
        // The checked state changes the indicator fill and the accessible value;
        // the outline and accent fills are paint-only.
        let mut invalidation = Invalidation::empty();
        if state.label != self.label || state.colors.0 != colors.0 {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if stale_checked {
            invalidation |= Invalidation::PAINT | Invalidation::ACCESSIBILITY;
        }
        if state.colors.1 != colors.1 || state.colors.2 != colors.2 {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() {
            let label = self.label.clone();
            let checked = self.checked;
            context
                .ui()
                .update(state.handle, invalidation, |checkbox| {
                    checkbox.label = label;
                    checkbox.checked = checked;
                    checkbox.text_color = colors.0;
                    checkbox.outline_color = colors.1;
                    checkbox.accent_color = colors.2;
                })?;
        }
        state.label = self.label;
        state.colors = colors;
        Ok(())
    }
}

struct SliderView<Action: 'static> {
    label: String,
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    on_changed: Arc<dyn Fn(f32) -> Action>,
}

struct SliderState<Action: 'static> {
    handle: NodeHandle<Slider>,
    label: String,
    // Not cached, for the same reason as `CheckboxState`: the slider writes its
    // own value on a drag, a keyboard step, and a semantic action, all before the
    // controller has said whether it accepts any of them.
    range: RangeInclusive<f32>,
    step: f32,
    colors: (Color, Color),
    changed: MapCell<f32, Action>,
}

leaf_mounted_state!(SliderState<Action>);

impl<Action: 'static> ViewNode<Action> for SliderView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let theme = context.theme();
        let colors = (
            theme.color(ColorRole::Muted),
            theme.color(ColorRole::Accent),
        );
        let changed = MapCell::new(self.on_changed, context.emitter());
        let emit = changed.emitter();
        let mut slider = Slider::new(
            self.label.clone(),
            self.value,
            self.range.clone(),
            move |value| emit(value),
        );
        slider.step = self.step;
        slider.track_color = colors.0;
        slider.accent_color = colors.1;
        let handle = context.append(slider)?;
        Ok(Mounted::new(
            handle.id(),
            SliderState {
                handle,
                label: self.label,
                range: self.range,
                step: self.step,
                colors,
                changed,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let theme = context.theme();
        let colors = (
            theme.color(ColorRole::Muted),
            theme.color(ColorRole::Accent),
        );
        let emitter = context.emitter();
        let state = mounted.state_mut::<SliderState<Action>>()?;
        state.changed.update(self.on_changed, &emitter);
        // Live value, not the declaration - see `CheckboxView::rebuild`. A drag
        // is the case that matters most here, because it emits a value per
        // pointer move and a controller that clamps or quantizes disagrees with
        // the element on nearly every one of them.
        let stale_value = context.ui().element(state.handle)?.value != self.value;
        // A slider's size is fixed and its label is never painted, so nothing
        // here relayouts. The label is accessibility-only; value and range move
        // the thumb and the reported value; the step is neither painted nor
        // exposed, so changing it invalidates nothing.
        let mut invalidation = Invalidation::empty();
        if state.label != self.label {
            invalidation |= Invalidation::ACCESSIBILITY;
        }
        if stale_value || state.range != self.range {
            invalidation |= Invalidation::PAINT | Invalidation::ACCESSIBILITY;
        }
        if state.colors != colors {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() || state.step != self.step {
            let label = self.label.clone();
            let range = self.range.clone();
            let value = self.value;
            let step = self.step;
            context.ui().update(state.handle, invalidation, |slider| {
                // Normalization and clamping mirror `Slider::new`, which is what
                // the retained element guarantees about these fields.
                let start = (*range.start()).min(*range.end());
                let end = (*range.start()).max(*range.end());
                slider.label = label;
                slider.range = start..=end;
                slider.value = value.clamp(start, end);
                slider.step = step.max(0.0);
                slider.track_color = colors.0;
                slider.accent_color = colors.1;
            })?;
        }
        state.label = self.label;
        state.range = self.range;
        state.step = self.step;
        state.colors = colors;
        Ok(())
    }
}
