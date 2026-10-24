//! Controlled radio groups with roving focus and arrow-key selection.
use super::{
    Choice, ChoiceEvent, Part, callback, caption, indicator_colors, part, propose, quiet_root,
    root_builders,
};
use crate::{
    Axis, ClickEvent, Dispatch, Element, FocusHandle, IntoElement, Key, KeyInput, KeyboardKey,
    Listener, Modifiers, SemanticRole, button, column, row, stack,
};
use std::rc::Rc;
use taffy::prelude::AlignItems;

/// Controlled radio group builder. The group is one Tab stop: the selected enabled
/// option, or the first enabled option without a selection. Arrow keys move focus
/// to the next or previous enabled option, wrapping, and propose it immediately;
/// Space, Enter, clicks and assistive activation propose the activated option.
/// A selected key that names no option leaves every option unchecked.
///
/// ```
/// use rxui::prelude::*;
/// struct Export { format: Key }
/// impl View for Export {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         radio_group().key("format").selected(self.format.clone())
///             .accessibility_label("Format")
///             .option(choice("png", "PNG"))
///             .option(choice("jpeg", "JPEG"))
///             .option(choice("webp", "WebP").disabled(true))
///             .on_change(cx.listener(|s, e: &ChoiceEvent, _| s.format = e.key.clone()))
///     }
/// }
/// ```
#[must_use = "attach the group to a parent or return it from View::view"]
pub struct RadioGroup {
    root: Element,
    selected: Option<Key>,
    options: Vec<Choice>,
    change: Option<Listener<ChoiceEvent>>,
    axis: Axis,
    disabled: bool,
}

/// Creates an empty vertical radio group with no selection.
pub fn radio_group() -> RadioGroup {
    RadioGroup {
        root: column()
            .gap(2.)
            .accessibility_role(SemanticRole::RadioGroup),
        selected: None,
        options: Vec::new(),
        change: None,
        axis: Axis::Vertical,
        disabled: false,
    }
}

impl RadioGroup {
    /// Current controlled selection; update it in response to [`ChoiceEvent`].
    pub fn selected(mut self, key: impl Into<Key>) -> Self {
        self.selected = Some(key.into());
        self
    }
    /// Appends an option in navigation order.
    pub fn option(mut self, option: Choice) -> Self {
        self.options.push(option);
        self
    }
    /// Appends options in navigation order.
    pub fn options(mut self, options: impl IntoIterator<Item = Choice>) -> Self {
        self.options.extend(options);
        self
    }
    /// Receives selection proposals.
    pub fn on_change(mut self, listener: Listener<ChoiceEvent>) -> Self {
        self.change = Some(listener);
        self
    }
    /// Vertical (default) stacks options; horizontal places them in a row. Both
    /// orientations accept all four arrow keys.
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }
    /// Disables every option.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}
root_builders!(RadioGroup);

/// Next enabled index from `from` in direction `step`, wrapping.
fn step(enabled: &[bool], from: usize, forward: bool) -> Option<usize> {
    let n = enabled.len();
    (1..=n)
        .map(|offset| {
            if forward {
                (from + offset) % n
            } else {
                (from + n - offset % n) % n
            }
        })
        .find(|i| enabled[*i])
}

impl IntoElement for RadioGroup {
    fn into_element(self) -> Element {
        let count = self.options.len();
        let enabled: Rc<[bool]> = self
            .options
            .iter()
            .map(|o| !o.disabled && !self.disabled)
            .collect();
        let keys: Rc<[Key]> = self.options.iter().map(|o| o.key.clone()).collect();
        let handles: Rc<[FocusHandle]> = (0..count).map(|_| FocusHandle::new()).collect();
        let selected = self
            .options
            .iter()
            .position(|o| Some(&o.key) == self.selected.as_ref());
        let stop = selected
            .filter(|i| enabled[*i])
            .or_else(|| enabled.iter().position(|e| *e));
        let mut root = self.root;
        if self.axis == Axis::Horizontal {
            root.style.flex_direction = taffy::FlexDirection::Row;
            root = root.gap(12.);
        }
        for (index, option) in self.options.into_iter().enumerate() {
            let checked = selected == Some(index);
            let disabled = !enabled[index];
            let [fill, border, mark] = indicator_colors(checked, disabled);
            let mut circle = part(stack(), Part::Radio)
                .background(fill)
                .border(1., border)
                .align_items(AlignItems::CENTER)
                .justify_items(AlignItems::CENTER);
            if checked {
                circle = circle.child(part(row(), Part::RadioDot).key("dot").background(mark));
            }
            let mut content = row()
                .gap(8.)
                .align_items(AlignItems::CENTER)
                .child(circle.key("indicator").accessibility_hidden(true));
            if let Some(text) = caption(&option.label) {
                content = content.child(text.key("caption"));
            }
            let mut radio = quiet_root(button(content))
                .key(option.key.clone())
                .padding_x(4.)
                .disabled(disabled)
                .tab_stop(stop == Some(index))
                .focus_handle(handles[index].clone())
                .accessibility_role(SemanticRole::RadioButton)
                .accessibility_checked(checked);
            if option.label.is_empty() {
                radio = radio.accessibility_label(option.key_label());
            }
            let semantics = radio.semantics.get_or_insert_with(Default::default);
            semantics.set_size = Some(count);
            semantics.position_in_set = Some(index);
            if let Some(listener) = &self.change {
                let key = option.key.clone();
                radio = radio.on_click(
                    listener.map_event(move |_: &ClickEvent| ChoiceEvent { key: key.clone() }),
                );
            }
            let (enabled, keys, handles, change) = (
                enabled.clone(),
                keys.clone(),
                handles.clone(),
                self.change.clone(),
            );
            radio = radio.on_key_down(callback(move |input: &KeyInput, cx| {
                let forward = match input.event.key {
                    KeyboardKey::ArrowDown | KeyboardKey::ArrowRight => true,
                    KeyboardKey::ArrowUp | KeyboardKey::ArrowLeft => false,
                    _ => return Ok(Dispatch::Unchanged),
                };
                if input.event.modifiers != Modifiers::default() {
                    return Ok(Dispatch::Unchanged);
                }
                input.prevent_default();
                let Some(next) = step(&enabled, index, forward).filter(|n| *n != index) else {
                    return Ok(Dispatch::Unchanged);
                };
                handles[next].focus_bound(cx);
                propose(
                    change.as_ref(),
                    &ChoiceEvent {
                        key: keys[next].clone(),
                    },
                    cx,
                )?;
                Ok(Dispatch::Handled)
            }));
            root = root.child(radio);
        }
        root
    }
}

impl Choice {
    /// Fallback accessible name for an option without a caption.
    pub(crate) fn key_label(&self) -> String {
        match &self.key {
            Key::String(s) => s.to_string(),
            Key::Integer(i) => i.to_string(),
        }
    }
}
