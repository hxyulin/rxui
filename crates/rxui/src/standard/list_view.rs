//! Selectable, virtualized list views with controlled keyed selection.
use super::{callback, in_placement, local, propose, root_builders};
use crate::selection::{Selection, SelectionMode, SelectionRows, TypeAhead};
use crate::{
    AccessibilityHandle, Dispatch, Element, Entity, FocusHandle, Icon, IntoElement, Key, KeyInput,
    KeyboardKey, Listener, Modifiers, PointerButton, PointerInput, ScrollHandle, ScrollState,
    SemanticAction, SemanticRole, ViewContext, column, label,
};
use std::borrow::Cow;
use std::collections::HashSet;
use std::rc::Rc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

/// Accepted, application-owned list selection: the selected keys, the active
/// row's key and the range anchor. [`ListView`] proposes changes as
/// [`ListSelectionEvent`]s; store the proposed selection to accept it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListSelection(Selection);
impl ListSelection {
    /// An empty selection. The first row is active until a row is chosen.
    pub fn new(mode: SelectionMode) -> Self {
        Self(Selection::new(mode))
    }
    /// The selection policy.
    pub fn mode(&self) -> SelectionMode {
        self.0.mode
    }
    /// Selected keys. Keys of rows that are gone stay until the next proposal.
    pub fn selected(&self) -> &HashSet<Key> {
        &self.0.selected
    }
    /// Whether `key` is selected.
    pub fn is_selected(&self, key: &Key) -> bool {
        self.0.selected.contains(key)
    }
    /// The active row's key, which keyboard navigation moves from.
    pub fn active(&self) -> Option<&Key> {
        self.0.active.as_ref()
    }
    /// The key Shift-extension selects from.
    pub fn anchor(&self) -> Option<&Key> {
        self.0.anchor.as_ref()
    }
    /// Replaces the selection, making `active` the active row and the anchor.
    /// Single mode keeps only the first key.
    pub fn set(&mut self, selected: impl IntoIterator<Item = Key>, active: Option<Key>) {
        let take = match self.0.mode {
            SelectionMode::Single => 1,
            SelectionMode::Multiple => usize::MAX,
        };
        self.0.selected = selected.into_iter().take(take).collect();
        self.0.anchor.clone_from(&active);
        self.0.active = active;
    }
}

/// Proposed selection from a [`ListView`]. Store it to accept it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListSelectionEvent {
    /// The proposed selection.
    pub selection: ListSelection,
}

/// One row of a [`ListView`]: a stable key, the label, and optional decoration.
#[must_use = "return the item from the list view's items closure"]
pub struct ListItem<'a> {
    key: Key,
    label: Cow<'a, str>,
    icon: Option<Icon>,
    detail: Option<Cow<'a, str>>,
    trailing: Option<Element>,
}
impl<'a> ListItem<'a> {
    /// Creates a row. Keys must be unique within the list. The label is the
    /// row's accessible name and type-ahead text.
    pub fn new(key: impl Into<Key>, label: impl Into<Cow<'a, str>>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            icon: None,
            detail: None,
            trailing: None,
        }
    }
    /// Shows `icon` before the label.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    /// Muted text after the label.
    pub fn detail(mut self, detail: impl Into<Cow<'a, str>>) -> Self {
        self.detail = Some(detail.into());
        self
    }
    /// Passive content at the row's end, such as a [`crate::badge`].
    pub fn trailing(mut self, content: impl IntoElement) -> Self {
        self.trailing = Some(content.into_element());
        self
    }
}

/// Placement-local scrolling and type-ahead.
pub(crate) struct ListViewState {
    scroll: ScrollHandle,
    typeahead: TypeAhead,
    origin: Instant,
}
impl Default for ListViewState {
    fn default() -> Self {
        Self {
            scroll: ScrollHandle::new(),
            typeahead: TypeAhead::default(),
            origin: Instant::now(),
        }
    }
}

/// Controlled list view builder: a selectable, virtualized list of
/// [`ListItem`] rows at the theme's `row_height`.
///
/// The list is one Tab stop with an active row. Up/Down, Home/End and
/// PageUp/PageDown move the active row and select it; Primary keeps the
/// selection and moves only the active row. In [`SelectionMode::Multiple`],
/// Shift with a move or a click selects the range from the anchor,
/// Primary+Space or Primary-click toggles a row, and Primary+A selects every
/// row. Typing selects the next row whose label starts with the typed text.
/// Selected rows paint `RowSelected` while focus is in the list and
/// `RowSelectedInactive` otherwise.
///
/// Rows have the `ListBoxOption` role in a `ListBox` that publishes the
/// active row as its active descendant. The items closure runs for every item
/// on each description, to collect keys and labels for keyboard input; use
/// [`crate::virtual_list`] directly for very large sets. Give the list a
/// bounded height.
///
/// ```
/// use rxui::prelude::*;
/// struct Files { names: Vec<String>, selection: ListSelection }
/// impl View for Files {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         list_view(cx)
///             .accessibility_label("Files")
///             .height(240.)
///             .selection(&self.selection)
///             .items(self.names.len(), |i| ListItem::new(i, &self.names[i]).icon(Icon::FILE))
///             .on_change(cx.listener(|s, e: &ListSelectionEvent, _| {
///                 s.selection = e.selection.clone();
///             }))
///     }
/// }
/// ```
#[must_use = "attach the list view to a parent or return it from View::view"]
pub struct ListView<'a> {
    root: Element,
    state: Entity<ListViewState>,
    scroll: ScrollHandle,
    scroll_state: Option<ScrollState>,
    mount: Option<crate::MountId>,
    focus: FocusHandle,
    row_height: f32,
    selection: ListSelection,
    count: usize,
    item: Box<dyn Fn(usize) -> ListItem<'a> + 'a>,
    change: Option<Listener<ListSelectionEvent>>,
}

/// Creates a list view whose scrolling and type-ahead belong to this call site
/// in the current placement. List views built in a loop need [`list_view_keyed`].
#[track_caller]
pub fn list_view<'a, T>(cx: &ViewContext<'_, T>) -> ListView<'a> {
    list_view_keyed(cx, ())
}

/// Like [`list_view`], with separate state per key at one call site.
#[track_caller]
pub fn list_view_keyed<'a, T>(cx: &ViewContext<'_, T>, key: impl std::hash::Hash) -> ListView<'a> {
    let state: Entity<ListViewState> = local(cx, key);
    let scroll = state.read(cx).scroll.clone();
    ListView {
        root: column(),
        state,
        scroll_state: scroll.state(cx),
        scroll,
        mount: crate::ReadContext::placement_scope(cx),
        focus: FocusHandle::new(),
        row_height: cx.theme().sizes().row_height,
        selection: ListSelection::new(SelectionMode::Single),
        count: 0,
        item: Box::new(|i| ListItem::new(i, "")),
        change: None,
    }
}

impl<'a> ListView<'a> {
    /// Current controlled selection; update it in response to [`ListSelectionEvent`].
    pub fn selection(mut self, selection: &ListSelection) -> Self {
        self.selection = selection.clone();
        self
    }
    /// Sets `count` rows, describing row `index` with `item`. The closure may
    /// borrow application state.
    pub fn items(mut self, count: usize, item: impl Fn(usize) -> ListItem<'a> + 'a) -> Self {
        self.count = count;
        self.item = Box::new(item);
        self
    }
    /// Receives selection proposals.
    pub fn on_change(mut self, listener: Listener<ListSelectionEvent>) -> Self {
        self.change = Some(listener);
        self
    }
    /// Fixed logical height.
    pub fn height(mut self, height: f32) -> Self {
        self.root = self.root.height(height);
        self
    }
    /// Fills the parent's content height.
    pub fn fill_height(mut self) -> Self {
        self.root = self.root.fill_height();
        self
    }
}
root_builders!(ListView<'_>);

/// Row keys and labels in order, for keyboard and pointer input.
struct Rows {
    keys: Vec<Key>,
    labels: Vec<String>,
}
impl SelectionRows for Rows {
    fn len(&self) -> usize {
        self.keys.len()
    }
    fn key(&self, index: usize) -> &Key {
        &self.keys[index]
    }
    fn index(&self, key: &Key) -> Option<usize> {
        self.keys.iter().position(|k| k == key)
    }
    fn label(&self, index: usize) -> &str {
        &self.labels[index]
    }
}

/// What the input listeners share.
struct Shared {
    rows: Rows,
    selection: ListSelection,
    /// The active row: the selection's, else the first.
    active: Option<usize>,
    state: Entity<ListViewState>,
    scroll: ScrollHandle,
    mount: Option<crate::MountId>,
    row_height: f32,
    change: Option<Listener<ListSelectionEvent>>,
}
impl Shared {
    /// Proposes `next` if it differs and reveals its active row.
    fn propose(
        &self,
        next: Selection,
        cx: &mut crate::AppContext<'_>,
    ) -> Result<Dispatch, crate::AccessError> {
        if next == self.selection.0 {
            return Ok(Dispatch::Unchanged);
        }
        if let Some(index) = next.active.as_ref().and_then(|k| self.rows.index(k)) {
            let _ = in_placement(cx, self.mount, |cx| {
                self.scroll.reveal_row(cx, index, self.row_height)
            });
        }
        propose(
            self.change.as_ref(),
            &ListSelectionEvent {
                selection: ListSelection(next),
            },
            cx,
        )
    }
    /// Moves to row `to` from the active row.
    fn move_to(
        &self,
        to: usize,
        modifiers: Modifiers,
        toggle: bool,
        cx: &mut crate::AppContext<'_>,
    ) -> Result<Dispatch, crate::AccessError> {
        let mut next = self.selection.0.clone();
        next.move_to(
            &self.rows,
            self.active.unwrap_or(to),
            to,
            modifiers,
            toggle,
            true,
        );
        self.propose(next, cx)
    }
}

impl IntoElement for ListView<'_> {
    fn into_element(self) -> Element {
        let mut rows = Rows {
            keys: Vec::with_capacity(self.count),
            labels: Vec::with_capacity(self.count),
        };
        for index in 0..self.count {
            let item = (self.item)(index);
            rows.keys.push(item.key);
            rows.labels.push(item.label.into_owned());
        }
        let len = rows.len();
        let active = self
            .selection
            .active()
            .and_then(|k| rows.index(k))
            .or((len > 0).then_some(0));
        let multiple = self.selection.mode() == SelectionMode::Multiple;
        let row_height = self.row_height;
        let page = self
            .scroll_state
            .map_or(1, |s| (s.viewport[1] / row_height).floor() as usize)
            .max(1);
        let shared = Rc::new(Shared {
            rows,
            selection: self.selection,
            active,
            state: self.state,
            scroll: self.scroll.clone(),
            mount: self.mount,
            row_height,
            change: self.change,
        });
        let active_handle = AccessibilityHandle::new();

        let item = &self.item;
        let mut list = crate::virtual_list::virtual_list_in(
            len,
            row_height,
            &self.scroll,
            self.scroll_state,
            |index| {
                let ListItem {
                    label: text,
                    icon,
                    detail,
                    trailing,
                    ..
                } = item(index);
                let key = shared.rows.keys[index].clone();
                let selected = shared.selection.is_selected(&key);
                let mut row = super::combobox::item_content(
                    icon,
                    label(text.as_ref()),
                    detail.as_deref(),
                    trailing,
                )
                .key(key)
                .list_row(selected)
                .accessibility_role(SemanticRole::ListBoxOption)
                .accessibility_label(text.into_owned())
                .accessibility_selected(selected);
                let semantics = row.semantics.get_or_insert_with(Default::default);
                semantics.set_size = Some(len);
                semantics.position_in_set = Some(index);
                if active == Some(index) {
                    row = row.accessibility_handle(active_handle.clone());
                }
                let (press, focus) = (shared.clone(), self.focus.clone());
                row = row.on_pointer_down(callback(move |event: &PointerInput, cx| {
                    if event.button != Some(PointerButton::Primary) {
                        return Ok(Dispatch::Unchanged);
                    }
                    event.prevent_default();
                    event.stop_propagation();
                    focus.focus_bound(cx);
                    press.move_to(index, event.modifiers, true, cx)
                }));
                let assist = shared.clone();
                row.on_semantic_action(callback(move |event: &SemanticAction, cx| match event {
                    SemanticAction::Activate(_) | SemanticAction::Select(_) => {
                        assist.move_to(index, Modifiers::default(), true, cx)
                    }
                    _ => Ok(Dispatch::Unchanged),
                }))
            },
        )
        .plain()
        .fill_width()
        .fill_height()
        .into_element();
        crate::tree::unfocus_bars(&mut list);

        let keys = shared.clone();
        let on_key = callback(move |input: &KeyInput, cx| {
            let shared = &keys;
            let m = input.event.modifiers;
            let (Some(from), Some(last)) = (shared.active, len.checked_sub(1)) else {
                return Ok(Dispatch::Unchanged);
            };
            if m.alt {
                return Ok(Dispatch::Unchanged);
            }
            let to = match &input.event.key {
                KeyboardKey::ArrowDown => (from + 1).min(last),
                KeyboardKey::ArrowUp => from.saturating_sub(1),
                KeyboardKey::Home => 0,
                KeyboardKey::End => last,
                KeyboardKey::PageDown => (from + page).min(last),
                KeyboardKey::PageUp => from.saturating_sub(page),
                KeyboardKey::Space => {
                    input.prevent_default();
                    return shared.move_to(from, m, true, cx);
                }
                KeyboardKey::Character(c) if m.primary() && c.eq_ignore_ascii_case("a") => {
                    if !multiple {
                        return Ok(Dispatch::Unchanged);
                    }
                    input.prevent_default();
                    let mut next = shared.selection.0.clone();
                    next.select_all(&shared.rows);
                    return shared.propose(next, cx);
                }
                KeyboardKey::Character(text)
                    if !m.control
                        && !m.meta
                        && !text.is_empty()
                        && !text.chars().any(char::is_control) =>
                {
                    input.prevent_default();
                    let found = shared.state.try_update(cx, |s, _| {
                        let time = s.origin.elapsed();
                        s.typeahead.find(&shared.rows, from, text, time)
                    })?;
                    let Some(to) = found else {
                        return Ok(Dispatch::Unchanged);
                    };
                    let mut next = shared.selection.0.clone();
                    next.move_to(&shared.rows, from, to, m, false, false);
                    return shared.propose(next, cx);
                }
                _ => return Ok(Dispatch::Unchanged),
            };
            input.prevent_default();
            shared.state.try_update(cx, |s, _| s.typeahead.clear())?;
            shared.move_to(to, m, false, cx)
        });
        let focus = self.focus.clone();
        let mut root = self
            .root
            .focusable(true)
            .focus_handle(self.focus)
            .accessibility_role(SemanticRole::ListBox)
            .accessibility_active_descendant(active_handle)
            .on_key_down(on_key)
            .on_pointer_down(callback(move |_: &PointerInput, cx| {
                focus.focus_bound(cx);
                Ok(Dispatch::Unchanged)
            }))
            .child(list);
        root.semantics
            .get_or_insert_with(Default::default)
            .multiselectable = multiple;
        root
    }
}
