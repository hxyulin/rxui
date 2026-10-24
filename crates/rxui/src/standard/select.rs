//! Controlled selects: a combo box button with a list box popover.
use super::glyph::{Glyph, glyph};
use super::{Choice, ChoiceEvent, callback, local, propose, root_builders};
use crate::{
    AnchorHandle, ClickEvent, DismissEvent, Dispatch, Element, Entity, HoverEvent, IntoElement,
    Key, KeyInput, KeyboardKey, Listener, PointerButton, PointerInput, SemanticRole, ThemeColor,
    ViewContext, button, column, label, popover, row,
};
use std::rc::Rc;
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
use taffy::prelude::AlignItems;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

/// Placement-local open state and keyboard highlight.
#[derive(Default)]
pub(crate) struct SelectState {
    open: bool,
    highlight: Option<Key>,
    prefix: String,
    typed_at: Option<Instant>,
}

/// Controlled select builder: a ComboBox button showing the current option that
/// opens a ListBox popover below it. Focus stays on the button while the list is
/// open, as in the platform combo box pattern; Up/Down, Home/End and type-ahead
/// move the highlight, Enter or Space chooses it and Escape, Tab or an outside
/// press closes the list. While closed, Up/Down open the list and a letter chooses
/// the next option starting with it. Typing within one second extends the prefix;
/// repeated letters cycle matches. Highlights follow option keys through changes
/// and scroll into view. A selected key that names no option shows
/// the placeholder.
///
/// ```
/// use rxui::prelude::*;
/// struct Editor { language: Key }
/// impl View for Editor {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         select(cx).key("language").selected(self.language.clone())
///             .accessibility_label("Language")
///             .option(choice("rust", "Rust"))
///             .option(choice("toml", "TOML"))
///             .option(choice("md", "Markdown"))
///             .on_change(cx.listener(|s, e: &ChoiceEvent, _| s.language = e.key.clone()))
///     }
/// }
/// ```
#[must_use = "attach the select to a parent or return it from View::view"]
pub struct Select {
    root: Element,
    state: Entity<SelectState>,
    open: bool,
    highlight: Option<Key>,
    selected: Option<Key>,
    options: Vec<Choice>,
    change: Option<Listener<ChoiceEvent>>,
    placeholder: String,
    disabled: bool,
    list_width: Option<f32>,
}

/// Creates a select whose open state belongs to this call site in the current
/// placement. Selects built in a loop need [`select_keyed`].
#[track_caller]
pub fn select<T>(cx: &ViewContext<'_, T>) -> Select {
    select_keyed(cx, ())
}

/// Like [`select`], with a separate open state per key at one call site.
#[track_caller]
pub fn select_keyed<T>(cx: &ViewContext<'_, T>, key: impl std::hash::Hash) -> Select {
    let state: Entity<SelectState> = local(cx, key);
    let (open, highlight) = {
        let read = state.read(cx);
        (read.open, read.highlight.clone())
    };
    Select {
        root: column().width(200.),
        state,
        open,
        highlight,
        selected: None,
        options: Vec::new(),
        change: None,
        placeholder: String::new(),
        disabled: false,
        list_width: None,
    }
}

impl Select {
    /// Current controlled selection; update it in response to [`ChoiceEvent`].
    pub fn selected(mut self, key: impl Into<Key>) -> Self {
        self.selected = Some(key.into());
        self
    }
    /// Appends an option in list order.
    pub fn option(mut self, option: Choice) -> Self {
        self.options.push(option);
        self
    }
    /// Appends options in list order.
    pub fn options(mut self, options: impl IntoIterator<Item = Choice>) -> Self {
        self.options.extend(options);
        self
    }
    /// Receives selection proposals. Choosing the current option proposes it again.
    pub fn on_change(mut self, listener: Listener<ChoiceEvent>) -> Self {
        self.change = Some(listener);
        self
    }
    /// Muted text shown when no option is selected.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }
    /// Disables the button and hides an open list.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Width of the open list. It defaults to a fixed `width` of the select, else
    /// 200 logical units; set it when the select fills or grows into its parent.
    pub fn list_width(mut self, width: f32) -> Self {
        self.list_width = Some(width);
        self
    }
}
root_builders!(Select);

/// Shared, cheaply cloned option data for the input listeners.
struct Options {
    labels: Vec<String>,
    keys: Vec<Key>,
    enabled: Vec<bool>,
    selected: Option<usize>,
}
impl Options {
    fn first(&self) -> Option<usize> {
        self.enabled.iter().position(|e| *e)
    }
    fn last(&self) -> Option<usize> {
        self.enabled.iter().rposition(|e| *e)
    }
    /// Highlight shown when the list opens: the selection, else the first option.
    fn initial(&self) -> Option<usize> {
        self.selected
            .filter(|i| self.enabled.get(*i) == Some(&true))
            .or_else(|| self.first())
    }
    /// Nearest enabled option after (or before) `from`, without wrapping.
    fn step(&self, from: Option<usize>, forward: bool) -> Option<usize> {
        let Some(from) = from else {
            return self.initial();
        };
        if forward {
            (from + 1..self.enabled.len()).find(|i| self.enabled[*i])
        } else {
            (0..from).rev().find(|i| self.enabled[*i])
        }
        .or(Some(from))
    }
    /// Next enabled option after `from` whose caption starts with `text`, wrapping.
    fn type_ahead(&self, from: Option<usize>, text: &str) -> Option<usize> {
        let text = text.to_lowercase();
        let n = self.labels.len();
        let start = from.map_or(0, |i| i + 1);
        (0..n)
            .map(|offset| (start + offset) % n)
            .find(|i| self.enabled[*i] && self.labels[*i].to_lowercase().starts_with(&text))
    }
}

fn set(
    state: &Entity<SelectState>,
    cx: &mut crate::AppContext<'_>,
    open: bool,
    highlight: Option<Key>,
) -> Result<Dispatch, crate::AccessError> {
    state.try_update(cx, |s, cx| {
        if s.open == open && s.highlight == highlight {
            cx.unchanged();
            return Dispatch::Unchanged;
        }
        if s.open != open {
            s.prefix.clear();
            s.typed_at = None;
        }
        s.open = open;
        s.highlight = highlight;
        Dispatch::Handled
    })
}

/// Accumulate a prefix for one second; repeated single letters cycle. If the
/// prefix has no match, start a new search with the latest input.
fn type_ahead(
    state: &Entity<SelectState>,
    options: &Options,
    from: Option<usize>,
    text: &str,
    cx: &mut crate::AppContext<'_>,
) -> Result<Option<usize>, crate::AccessError> {
    state.try_update(cx, |s, _| {
        let now = Instant::now();
        if s.typed_at
            .is_none_or(|last| now.duration_since(last) > Duration::from_secs(1))
        {
            s.prefix.clear();
        }
        let text = text.to_lowercase();
        let cycle = s.prefix == text;
        if !cycle {
            s.prefix.push_str(&text);
        }
        s.typed_at = Some(now);
        // Continuing a prefix includes the current match; single letters cycle.
        let start = if !cycle && s.prefix != text {
            from.map(|i| {
                if i == 0 {
                    options.keys.len() - 1
                } else {
                    i - 1
                }
            })
        } else {
            from
        };
        let found = options.type_ahead(start, &s.prefix);
        if found.is_some() {
            found
        } else {
            s.prefix = text;
            options.type_ahead(from, &s.prefix)
        }
    })
}

/// Closes the list and proposes option `index`.
fn choose(
    state: &Entity<SelectState>,
    options: &Options,
    change: Option<&Listener<ChoiceEvent>>,
    index: usize,
    cx: &mut crate::AppContext<'_>,
) -> Result<Dispatch, crate::AccessError> {
    set(state, cx, false, None)?;
    propose(
        change,
        &ChoiceEvent {
            key: options.keys[index].clone(),
        },
        cx,
    )?;
    Ok(Dispatch::Handled)
}

impl IntoElement for Select {
    fn into_element(self) -> Element {
        let selected = self
            .options
            .iter()
            .position(|o| Some(&o.key) == self.selected.as_ref());
        let options = Rc::new(Options {
            labels: self.options.iter().map(|o| o.label.clone()).collect(),
            keys: self.options.iter().map(|o| o.key.clone()).collect(),
            enabled: self.options.iter().map(|o| !o.disabled).collect(),
            selected,
        });
        let open = self.open && !self.disabled;
        let highlight = self
            .highlight
            .as_ref()
            .and_then(|key| options.keys.iter().position(|k| k == key))
            .filter(|i| options.enabled[*i])
            .or_else(|| options.initial());
        let active = crate::AccessibilityHandle::new();
        let list_handle = crate::AccessibilityHandle::new();
        let width = self.root.style.size.width.into_raw();
        let list_width =
            self.list_width
                .unwrap_or(if width.tag() == taffy::CompactLength::LENGTH_TAG {
                    width.value()
                } else {
                    200.
                });
        let anchor = AnchorHandle::new();
        let current = selected.map(|i| self.options[i].label.clone());
        let shown = current.clone().unwrap_or_else(|| self.placeholder.clone());
        let mut text = label(shown).flex_grow(1.).flex_shrink(1.).min_width(0.);
        if current.is_none() {
            text = text.color(ThemeColor::TextMuted);
        }
        let state = self.state;
        // Naming builders target the root; the combo box button is what users reach.
        let mut root = self.root;
        let naming = root.semantics.take();
        let mut trigger = button(
            row()
                .gap(8.)
                .fill_width()
                .align_items(AlignItems::CENTER)
                .child(text)
                .child(glyph(Glyph::Chevron, 12.).color(ThemeColor::TextMuted)),
        )
        .key("button")
        .fill_width()
        .anchor_handle(anchor.clone())
        .disabled(self.disabled)
        .accessibility_role(SemanticRole::ComboBox)
        .accessibility_expanded(open)
        .accessibility_value(current.unwrap_or_default());
        if open && highlight.is_some() {
            trigger = trigger.accessibility_active_descendant(active.clone());
        }
        if open {
            trigger = trigger.accessibility_controls(list_handle.clone());
        }
        let labelled_by = naming.as_ref().and_then(|n| n.labelled_by.clone());
        let name = naming.as_ref().and_then(|n| n.label.clone());
        if let Some(naming) = naming {
            let semantics = trigger.semantics.get_or_insert_with(Default::default);
            semantics.label = naming.label;
            semantics.description = naming.description;
            semantics.labelled_by = naming.labelled_by;
        }
        {
            let (state, options) = (state.clone(), options.clone());
            trigger = trigger.on_click(callback(move |_: &ClickEvent, cx| {
                set(
                    &state,
                    cx,
                    !open,
                    options.initial().map(|i| options.keys[i].clone()),
                )
            }));
        }
        let (key_state, key_options, key_change) =
            (state.clone(), options.clone(), self.change.clone());
        let disabled = self.disabled;
        root = root.on_key_down(callback(move |input: &KeyInput, cx| {
            let (state, options, change) = (&key_state, &key_options, key_change.as_ref());
            if disabled {
                return Ok(Dispatch::Unchanged);
            }
            let m = input.event.modifiers;
            if m.control || m.meta {
                return Ok(Dispatch::Unchanged);
            }
            let typed = match &input.event.key {
                KeyboardKey::Character(c) if !m.alt && !c.trim().is_empty() => Some(c.as_str()),
                _ => None,
            };
            if !open {
                return match (&input.event.key, typed) {
                    (KeyboardKey::ArrowDown | KeyboardKey::ArrowUp, _) => {
                        input.prevent_default();
                        set(
                            state,
                            cx,
                            true,
                            options.initial().map(|i| options.keys[i].clone()),
                        )
                    }
                    (_, Some(text)) => {
                        input.prevent_default();
                        match type_ahead(state, options, options.selected, text, cx)? {
                            Some(next) if Some(next) != options.selected => propose(
                                change,
                                &ChoiceEvent {
                                    key: options.keys[next].clone(),
                                },
                                cx,
                            ),
                            _ => Ok(Dispatch::Unchanged),
                        }
                    }
                    _ => Ok(Dispatch::Unchanged),
                };
            }
            let next = match (&input.event.key, typed) {
                (KeyboardKey::ArrowDown, _) => options.step(highlight, true),
                (KeyboardKey::ArrowUp, _) => options.step(highlight, false),
                (KeyboardKey::Home | KeyboardKey::PageUp, _) => options.first(),
                (KeyboardKey::End | KeyboardKey::PageDown, _) => options.last(),
                (KeyboardKey::Enter | KeyboardKey::Space, _) => {
                    input.prevent_default();
                    return match highlight.filter(|i| options.enabled.get(*i) == Some(&true)) {
                        Some(index) => choose(state, options, change, index, cx),
                        None => set(state, cx, false, None),
                    };
                }
                (KeyboardKey::Tab, _) => return set(state, cx, false, None),
                (_, Some(text)) => type_ahead(state, options, highlight, text, cx)?.or(highlight),
                _ => return Ok(Dispatch::Unchanged),
            };
            input.prevent_default();
            set(state, cx, true, next.map(|i| options.keys[i].clone()))
        }));
        root = root.child(trigger);
        if open {
            let mut list = column()
                .fill_width()
                .accessibility_role(SemanticRole::ListBox)
                .accessibility_handle(list_handle);
            if let Some(labelled_by) = labelled_by {
                list = list.accessibility_labelled_by(labelled_by);
            }
            if let Some(name) = name {
                list = list.accessibility_label(name);
            }
            for (index, option) in self.options.iter().enumerate() {
                let lit = highlight == Some(index) && !option.disabled;
                let mark = if selected == Some(index) {
                    glyph(Glyph::Check, 12.)
                } else {
                    row().size(12., 12.)
                };
                let mut item = row()
                    .key(option.key.clone())
                    .fill_width()
                    .flex_shrink(0.)
                    .gap(6.)
                    .padding_x(8.)
                    .padding_y(6.)
                    .radius(4.)
                    .align_items(AlignItems::CENTER)
                    .accessibility_role(SemanticRole::ListBoxOption)
                    .accessibility_selected(selected == Some(index))
                    .accessibility_label(option.label.clone())
                    .child(mark)
                    .child(label(option.label.clone()));
                let semantics = item.semantics.get_or_insert_with(Default::default);
                semantics.set_size = Some(self.options.len());
                semantics.position_in_set = Some(index);
                if lit {
                    item = item.accessibility_handle(active.clone());
                }
                if option.disabled {
                    item = item.accessibility_disabled(true);
                    item = item.color(ThemeColor::TextDisabled);
                } else {
                    // Rows highlight like menu items; the check marks the selection.
                    if lit {
                        item = item.background(ThemeColor::RowHover);
                    }
                    let hover_state = state.clone();
                    let hover_key = option.key.clone();
                    item = item.on_hover(callback(move |event: &HoverEvent, cx| {
                        if event.hovered {
                            set(&hover_state, cx, true, Some(hover_key.clone()))
                        } else {
                            Ok(Dispatch::Unchanged)
                        }
                    }));
                    let (state, options, change) =
                        (state.clone(), options.clone(), self.change.clone());
                    item = item.on_pointer_up(callback(move |event: &PointerInput, cx| {
                        if event.button != Some(PointerButton::Primary) {
                            return Ok(Dispatch::Unchanged);
                        }
                        choose(&state, &options, change.as_ref(), index, cx)
                    }));
                }
                list = list.child(item);
            }
            let dismiss = state.clone();
            root = root.child(
                popover(anchor, list)
                    .key("list")
                    .width(list_width)
                    .padding(4.)
                    .autofocus(false)
                    .on_dismiss(callback(move |_: &DismissEvent, cx| {
                        set(&dismiss, cx, false, None)
                    })),
            );
        }
        root
    }
}
