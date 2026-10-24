//! Controlled segmented controls: a radio group drawn as one inset track.
use super::{Choice, ChoiceEvent, Part, callback, part, propose, root_builders};
use crate::{
    ButtonVariant, ClickEvent, Dispatch, Element, FocusHandle, IntoElement, Key, KeyInput,
    KeyboardKey, Listener, Modifiers, SemanticRole, ThemeColor, button, row,
};
use std::rc::Rc;
use taffy::prelude::{AlignItems, JustifyContent, fr};

/// Controlled segmented control builder; see [`segmented`].
#[must_use = "attach the segmented control to a parent or return it from View::view"]
pub struct Segmented {
    root: Element,
    selected: Option<Key>,
    options: Vec<Choice>,
    change: Option<Listener<ChoiceEvent>>,
    disabled: bool,
    equal_width: bool,
}

/// Creates an empty segmented control with no selection: a row of mutually
/// exclusive options joined into one control, such as a view mode switch.
///
/// The options sit in an inset track with the [`ThemeColor::Input`] fill, a
/// 1-unit [`ThemeColor::InputBorder`] outline and 1 unit of padding, as tall as
/// a text input. The selected segment is raised like a default button, with
/// the [`ThemeColor::Control`] fill, a [`ThemeColor::Border`] outline, the
/// control shadow and [`ThemeColor::Text`] content. The others are quiet:
/// [`ThemeColor::TextMuted`] content and a [`ThemeColor::RowHover`] fill while
/// hovered. Segments share the track's width equally unless
/// [`Segmented::equal_width`] turns that off. An option with
/// [`Choice::icon`] shows only its icon in a square segment, and its caption
/// names it.
///
/// It behaves like a [`crate::radio_group`]: a [`SemanticRole::RadioGroup`] of
/// radio buttons and one Tab stop, the selected enabled option or else the
/// first enabled one. Arrow keys move focus to the previous or next enabled
/// option and propose it; at either end they do nothing, so a segmented
/// control in a [`crate::toolbar`] passes them on to the toolbar. Clicks,
/// Space, Enter and assistive activation propose the activated option.
/// Disabled options are skipped.
///
/// ```
/// use rxui::{Icon, prelude::*};
/// struct Diff { view: Key, layout: Key }
/// impl View for Diff {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         row().gap(8.)
///             .child(segmented().selected(self.view.clone()).accessibility_label("Diff view")
///                 .option(choice("split", "Split"))
///                 .option(choice("unified", "Unified"))
///                 .on_change(cx.listener(|s, e: &ChoiceEvent, _| s.view = e.key.clone())))
///             .child(segmented().selected(self.layout.clone()).accessibility_label("Layout")
///                 .option(choice("list", "List").icon(Icon::LIST))
///                 .option(choice("grid", "Grid").icon(Icon::LAYOUT_GRID))
///                 .on_change(cx.listener(|s, e: &ChoiceEvent, _| s.layout = e.key.clone())))
///     }
/// }
/// ```
pub fn segmented() -> Segmented {
    let root = part(row(), Part::Track)
        .background(ThemeColor::Input)
        .border(1., ThemeColor::InputBorder)
        .align_items(AlignItems::CENTER)
        .accessibility_role(SemanticRole::RadioGroup);
    Segmented {
        root,
        selected: None,
        options: Vec::new(),
        change: None,
        disabled: false,
        equal_width: true,
    }
}

impl Segmented {
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
    /// Disables every option.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Whether segments share the width equally, as wide as the widest one
    /// (default), or each fit their own content.
    pub fn equal_width(mut self, equal: bool) -> Self {
        self.equal_width = equal;
        self
    }
}
root_builders!(Segmented);

/// Next enabled index from `from` in direction `forward`, without wrapping.
fn step(enabled: &[bool], from: usize, forward: bool) -> Option<usize> {
    if forward {
        (from + 1..enabled.len()).find(|i| enabled[*i])
    } else {
        (0..from).rev().find(|i| enabled[*i])
    }
}

impl IntoElement for Segmented {
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
        if self.equal_width {
            // Equal fractions size every column to the widest segment, or share a
            // stretched track's width.
            root.style.display = taffy::Display::Grid;
            root.style.grid_template_columns = (0..count).map(|_| fr(1.)).collect();
        }
        for (index, option) in self.options.into_iter().enumerate() {
            let checked = selected == Some(index);
            let content = match &option.icon {
                Some(icon) => crate::icon(icon.clone()),
                None => crate::label(option.label.clone()),
            };
            let shape = if option.icon.is_some() {
                Part::IconSegment
            } else {
                Part::Segment
            };
            let mut segment = part(button(content.key("content")), shape)
                .variant(if checked {
                    ButtonVariant::Default
                } else {
                    ButtonVariant::Quiet
                })
                .key(option.key.clone())
                .align_items(AlignItems::CENTER)
                .justify_content(JustifyContent::CENTER)
                .disabled(!enabled[index])
                .tab_stop(stop == Some(index))
                .focus_handle(handles[index].clone())
                .accessibility_role(SemanticRole::RadioButton)
                .accessibility_checked(checked);
            if option.icon.is_some() || option.label.is_empty() {
                let name = if option.label.is_empty() {
                    option.key_label()
                } else {
                    option.label.clone()
                };
                segment = segment.accessibility_label(name);
            }
            let semantics = segment.semantics.get_or_insert_with(Default::default);
            semantics.set_size = Some(count);
            semantics.position_in_set = Some(index);
            if let Some(listener) = &self.change {
                let key = option.key.clone();
                segment = segment.on_click(
                    listener.map_event(move |_: &ClickEvent| ChoiceEvent { key: key.clone() }),
                );
            }
            let (enabled, keys, handles, change) = (
                enabled.clone(),
                keys.clone(),
                handles.clone(),
                self.change.clone(),
            );
            segment = segment.on_key_down(callback(move |input: &KeyInput, cx| {
                let forward = match input.event.key {
                    KeyboardKey::ArrowDown | KeyboardKey::ArrowRight => true,
                    KeyboardKey::ArrowUp | KeyboardKey::ArrowLeft => false,
                    _ => return Ok(Dispatch::Unchanged),
                };
                if input.event.modifiers != Modifiers::default() {
                    return Ok(Dispatch::Unchanged);
                }
                // At an end the key is left to a surrounding toolbar.
                let Some(next) = step(&enabled, index, forward) else {
                    return Ok(Dispatch::Unchanged);
                };
                input.prevent_default();
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
            root = root.child(segment);
        }
        root
    }
}
