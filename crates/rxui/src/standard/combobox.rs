//! Comboboxes: a controlled query input over a ranked, virtualized results list.
use super::{Matcher, callback, local, propose, root_builders};
use crate::{
    AccessibilityHandle, AnchorHandle, DismissEvent, Dispatch, Element, Entity, FontWeight,
    HoverEvent, Icon, IntoElement, KeyInput, KeyboardKey, Listener, PointerButton, PointerInput,
    ScrollHandle, ScrollState, SemanticRole, TextChangeEvent, TextOverflow, ThemeColor,
    ViewContext, column, label, popover, rich_text, row, span, text_input,
};
use std::any::TypeId;
use std::borrow::Cow;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;
use taffy::prelude::AlignItems;

/// Proposed query text from a [`Combobox`]. Store it to accept it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryEvent {
    /// Proposed query.
    pub text: String,
}

/// An item chosen in a [`Combobox`] with Enter or a click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChooseEvent {
    /// The item's index in [`Combobox::items`], not its position in the results.
    pub index: usize,
}

/// One item of a [`Combobox`]: the label the query matches, and optional
/// decoration around it.
#[must_use = "return the item from the combobox's items closure"]
pub struct ComboItem<'a> {
    label: Cow<'a, str>,
    icon: Option<Icon>,
    detail: Option<Cow<'a, str>>,
    trailing: Option<Element>,
}
impl<'a> ComboItem<'a> {
    /// Creates an item. The label is matched against the query, highlighted
    /// where it matches, and is the item's accessible name. Borrowed labels
    /// are not copied.
    pub fn new(label: impl Into<Cow<'a, str>>) -> Self {
        Self {
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
    /// Muted text after the label, such as a file's directory. It is not matched.
    pub fn detail(mut self, detail: impl Into<Cow<'a, str>>) -> Self {
        self.detail = Some(detail.into());
        self
    }
    /// Passive content at the row's end, such as a [`crate::kbd`] shortcut.
    pub fn trailing(mut self, content: impl IntoElement) -> Self {
        self.trailing = Some(content.into_element());
        self
    }
}

/// Lays out a result or list row's content: an optional icon, the label, muted
/// detail text, and trailing content at the end.
pub(crate) fn item_content(
    icon: Option<Icon>,
    text: Element,
    detail: Option<&str>,
    trailing: Option<Element>,
) -> Element {
    let mut content = row()
        .fill_width()
        .fill_height()
        .gap(6.)
        .align_items(AlignItems::CENTER);
    if let Some(icon) = icon {
        content = content.child(crate::icon(icon).flex_shrink(0.));
    }
    // The detail gives way before the label when the row is narrow.
    content = content.child(
        text.text_overflow(TextOverflow::Ellipsis)
            .flex_shrink(1.)
            .min_width(0.),
    );
    if let Some(detail) = detail {
        content = content.child(
            label(detail)
                .color(ThemeColor::TextMuted)
                .text_overflow(TextOverflow::Ellipsis)
                .flex_shrink(8.)
                .min_width(0.),
        );
    }
    if let Some(trailing) = trailing {
        content = content
            .child(row().flex_grow(1.))
            .child(trailing.flex_shrink(0.));
    }
    content
}

/// The label as spans, with each valid matched range at medium weight in the
/// Focus color. On the highlighted row the ranges keep the text color: Focus
/// falls below 4.5:1 contrast over the row's hover fill in dark themes.
fn highlighted(text: &str, ranges: &[Range<usize>], lit: bool) -> Element {
    if ranges.is_empty() {
        return label(text);
    }
    let mut spans = Vec::new();
    let mut at = 0;
    for range in ranges {
        let valid = at <= range.start
            && range.start < range.end
            && range.end <= text.len()
            && text.is_char_boundary(range.start)
            && text.is_char_boundary(range.end);
        if !valid {
            continue;
        }
        if at < range.start {
            spans.push(span(&text[at..range.start]));
        }
        let matched = span(&text[range.clone()]).weight(FontWeight::MEDIUM);
        spans.push(if lit {
            matched
        } else {
            matched.color(ThemeColor::Focus)
        });
        at = range.end;
    }
    if at < text.len() {
        spans.push(span(&text[at..]));
    }
    rich_text(spans)
}

/// One ranked result.
struct Hit {
    index: usize,
    ranges: Vec<Range<usize>>,
}

/// Results in rank order.
#[derive(Clone)]
enum Hits {
    /// Every item in order: the query is empty.
    All(usize),
    /// Matched items, best first.
    Ranked(Rc<Vec<Hit>>),
}
impl Hits {
    fn len(&self) -> usize {
        match self {
            Self::All(n) => *n,
            Self::Ranked(hits) => hits.len(),
        }
    }
    fn index(&self, position: usize) -> usize {
        match self {
            Self::All(_) => position,
            Self::Ranked(hits) => hits[position].index,
        }
    }
    fn ranges(&self, position: usize) -> &[Range<usize>] {
        match self {
            Self::All(_) => &[],
            Self::Ranked(hits) => &hits[position].ranges,
        }
    }
    fn position(&self, index: usize) -> Option<usize> {
        match self {
            Self::All(n) => (index < *n).then_some(index),
            Self::Ranked(hits) => hits.iter().position(|h| h.index == index),
        }
    }
}

/// Scored results and what they were scored for.
struct Scored {
    query: String,
    labels: u64,
    matcher: TypeId,
    hits: Rc<Vec<Hit>>,
}

/// Placement-local open state, highlight and results cache.
#[derive(Default)]
pub(crate) struct ComboState {
    open: bool,
    /// Highlighted item index, valid while the query is `query`.
    highlight: Option<usize>,
    query: String,
    scroll: ScrollHandle,
    scored: Rc<RefCell<Option<Scored>>>,
}

/// Controlled combobox builder: a text input whose query filters and ranks a
/// list of items, for quick open, command palettes and search fields.
///
/// The list opens in a popover below the input as the user types, or always
/// shows below it in normal flow with [`Self::inline`]. Focus stays in the
/// input. Up/Down move the highlight and wrap, PageUp/PageDown move it by a
/// page, and Primary+Home/End move it to the first and last result; Home/End
/// alone move the caret. Enter chooses the highlighted item, and a click
/// chooses the item under the pointer, which hover highlights. Escape closes
/// an open popover; otherwise it is left to an enclosing dialog.
///
/// The highlight starts on the first result whenever the query changes, and
/// follows its item while the results change for the same query. Results are
/// virtualized at the theme's `row_height`, at most 10 rows tall.
///
/// The [`Matcher`] scores each item's label on the UI thread; results sort by
/// score, ties keeping item order. An empty query shows every item in order
/// without scoring. Results are cached per query, item labels and matcher
/// type, so redescribing the view does not score again. Checking the labels
/// still calls the items closure for every item on each description, so pass
/// borrowed labels.
///
/// ```
/// use rxui::prelude::*;
/// struct QuickOpen { query: String, files: Vec<String>, opened: Option<usize> }
/// impl View for QuickOpen {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         combobox(cx)
///             .accessibility_label("Go to file")
///             .query(&self.query)
///             .on_query(cx.listener(|s, e: &QueryEvent, _| s.query = e.text.clone()))
///             .items(self.files.len(), |i| ComboItem::new(&self.files[i]).icon(Icon::FILE))
///             .matcher(FuzzyMatcher)
///             .on_choose(cx.listener(|s, e: &ChooseEvent, _| s.opened = Some(e.index)))
///     }
/// }
/// ```
#[must_use = "attach the combobox to a parent or return it from View::view"]
pub struct Combobox<'a> {
    root: Element,
    state: Entity<ComboState>,
    open: bool,
    highlight: Option<usize>,
    highlight_query: String,
    scroll: ScrollHandle,
    scroll_state: Option<ScrollState>,
    mount: Option<crate::MountId>,
    scored: Rc<RefCell<Option<Scored>>>,
    row_height: f32,
    query: String,
    on_query: Option<Listener<QueryEvent>>,
    on_choose: Option<Listener<ChooseEvent>>,
    count: usize,
    item: Box<dyn Fn(usize) -> ComboItem<'a> + 'a>,
    matcher: (Rc<dyn Matcher>, TypeId),
    inline: bool,
    placeholder: String,
    empty_text: String,
    list_width: Option<f32>,
}

/// Creates a combobox whose open state and highlight belong to this call site
/// in the current placement. Comboboxes built in a loop need [`combobox_keyed`].
#[track_caller]
pub fn combobox<'a, T>(cx: &ViewContext<'_, T>) -> Combobox<'a> {
    combobox_keyed(cx, ())
}

/// Like [`combobox`], with separate state per key at one call site.
#[track_caller]
pub fn combobox_keyed<'a, T>(cx: &ViewContext<'_, T>, key: impl std::hash::Hash) -> Combobox<'a> {
    let state: Entity<ComboState> = local(cx, key);
    let read = state.read(cx);
    let scroll = read.scroll.clone();
    Combobox {
        root: column().width(320.),
        open: read.open,
        highlight: read.highlight,
        highlight_query: read.query.clone(),
        scroll_state: scroll.state(cx),
        mount: crate::ReadContext::placement_scope(cx),
        scroll,
        scored: read.scored.clone(),
        state: state.clone(),
        row_height: cx.theme().sizes().row_height,
        query: String::new(),
        on_query: None,
        on_choose: None,
        count: 0,
        item: Box::new(|_| ComboItem::new("")),
        matcher: (
            Rc::new(super::FuzzyMatcher),
            TypeId::of::<super::FuzzyMatcher>(),
        ),
        inline: false,
        placeholder: String::new(),
        empty_text: "No results".into(),
        list_width: None,
    }
}

impl<'a> Combobox<'a> {
    /// Current controlled query; update it in response to [`QueryEvent`].
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.query = query.into();
        self
    }
    /// Receives query proposals as the user edits the input.
    pub fn on_query(mut self, listener: Listener<QueryEvent>) -> Self {
        self.on_query = Some(listener);
        self
    }
    /// Sets `count` items, describing item `index` with `item`. The closure may
    /// borrow application state; it runs while the combobox is described.
    pub fn items(mut self, count: usize, item: impl Fn(usize) -> ComboItem<'a> + 'a) -> Self {
        self.count = count;
        self.item = Box::new(item);
        self
    }
    /// How the query filters and ranks items; [`super::FuzzyMatcher`] by default.
    pub fn matcher<M: Matcher>(mut self, matcher: M) -> Self {
        self.matcher = (Rc::new(matcher), TypeId::of::<M>());
        self
    }
    /// Receives the chosen item's index. A popover closes when an item is chosen.
    pub fn on_choose(mut self, listener: Listener<ChooseEvent>) -> Self {
        self.on_choose = Some(listener);
        self
    }
    /// Shows the results below the input in normal flow instead of in a
    /// popover, for palette dialogs. Inline results are always visible.
    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }
    /// Muted text shown in the empty input.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }
    /// Muted text shown when no item matches; "No results" by default.
    pub fn empty_text(mut self, text: impl Into<String>) -> Self {
        self.empty_text = text.into();
        self
    }
    /// Width of the popover. It defaults to a fixed `width` of the combobox,
    /// else 320 logical units; set it when the combobox fills its parent.
    pub fn list_width(mut self, width: f32) -> Self {
        self.list_width = Some(width);
        self
    }

    /// Ranked results for the current query, scored again only when the query,
    /// the item labels or the matcher type changed.
    fn hits(&self) -> Hits {
        if self.query.is_empty() {
            return Hits::All(self.count);
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.count.hash(&mut hasher);
        for index in 0..self.count {
            (self.item)(index).label.hash(&mut hasher);
        }
        let labels = hasher.finish();
        let mut scored = self.scored.borrow_mut();
        if let Some(s) = &*scored
            && s.query == self.query
            && s.labels == labels
            && s.matcher == self.matcher.1
        {
            return Hits::Ranked(s.hits.clone());
        }
        let mut hits: Vec<(i32, Hit)> = (0..self.count)
            .filter_map(|index| {
                let item = (self.item)(index);
                let found = self.matcher.0.score(&self.query, &item.label)?;
                Some((
                    found.score,
                    Hit {
                        index,
                        ranges: found.ranges,
                    },
                ))
            })
            .collect();
        // Stable: equal scores keep item order.
        hits.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        let hits = Rc::new(hits.into_iter().map(|(_, hit)| hit).collect::<Vec<_>>());
        *scored = Some(Scored {
            query: self.query.clone(),
            labels,
            matcher: self.matcher.1,
            hits: hits.clone(),
        });
        Hits::Ranked(hits)
    }
}
root_builders!(Combobox<'_>);

/// Rows of a full list; the list is never taller.
const MAX_ROWS: usize = 10;

/// What the input listeners share.
struct Shared {
    state: Entity<ComboState>,
    hits: Hits,
    query: String,
    scroll: ScrollHandle,
    mount: Option<crate::MountId>,
    row_height: f32,
    choose: Option<Listener<ChooseEvent>>,
}
impl Shared {
    /// Scrolls the list to the top or to reveal `position`. An unmounted list
    /// ignores it.
    fn scroll(&self, cx: &mut crate::AppContext<'_>, position: Option<usize>) {
        let _ = super::in_placement(cx, self.mount, |cx| match position {
            Some(position) => self.scroll.reveal_row(cx, position, self.row_height),
            None => self.scroll.jump_to(cx, [0., 0.]),
        });
    }
    /// Opens or closes the list and highlights the result at `position`.
    fn set(
        &self,
        cx: &mut crate::AppContext<'_>,
        open: bool,
        position: Option<usize>,
    ) -> Result<Dispatch, crate::AccessError> {
        let highlight = position.map(|p| self.hits.index(p));
        let query = &self.query;
        self.state.try_update(cx, |s, cx| {
            if s.open == open && s.highlight == highlight && s.query == *query {
                cx.unchanged();
                return Dispatch::Unchanged;
            }
            s.open = open;
            s.highlight = highlight;
            s.query.clone_from(query);
            Dispatch::Handled
        })
    }
    /// Closes the list and proposes the result at `position`.
    fn choose(
        &self,
        cx: &mut crate::AppContext<'_>,
        position: usize,
    ) -> Result<Dispatch, crate::AccessError> {
        self.set(cx, false, Some(position))?;
        propose(
            self.choose.as_ref(),
            &ChooseEvent {
                index: self.hits.index(position),
            },
            cx,
        )?;
        Ok(Dispatch::Handled)
    }
}

impl IntoElement for Combobox<'_> {
    fn into_element(self) -> Element {
        let hits = self.hits();
        let len = hits.len();
        let visible = self.inline || self.open;
        let highlight = self
            .highlight
            .filter(|_| self.highlight_query == self.query)
            .and_then(|index| hits.position(index))
            .or((len > 0).then_some(0));
        let row_height = self.row_height;
        let page = self
            .scroll_state
            .map_or(MAX_ROWS, |s| (s.viewport[1] / row_height).floor() as usize)
            .max(1);
        let shared = Rc::new(Shared {
            state: self.state.clone(),
            hits: hits.clone(),
            query: self.query.clone(),
            scroll: self.scroll.clone(),
            mount: self.mount,
            row_height,
            choose: self.on_choose.clone(),
        });
        let anchor = AnchorHandle::new();
        let active = AccessibilityHandle::new();
        let list_handle = AccessibilityHandle::new();

        // Naming builders target the root; the input is what users reach.
        let mut root = self.root;
        let naming = root.semantics.take();
        let width = root.style.size.width.into_raw();
        let list_width =
            self.list_width
                .unwrap_or(if width.tag() == taffy::CompactLength::LENGTH_TAG {
                    width.value()
                } else {
                    320.
                });
        let mut input = text_input(self.query.clone())
            .key("input")
            .fill_width()
            .placeholder(self.placeholder)
            .anchor_handle(anchor.clone())
            .accessibility_role(SemanticRole::ComboBox)
            .accessibility_expanded(visible);
        if visible {
            input = input.accessibility_controls(list_handle.clone());
            if highlight.is_some() {
                input = input.accessibility_active_descendant(active.clone());
            }
        }
        let labelled_by = naming.as_ref().and_then(|n| n.labelled_by.clone());
        let name = naming.as_ref().and_then(|n| n.label.clone());
        if let Some(naming) = naming {
            let semantics = input.semantics.get_or_insert_with(Default::default);
            semantics.label = naming.label;
            semantics.description = naming.description;
            semantics.labelled_by = naming.labelled_by;
        }
        {
            let (shared, on_query) = (shared.clone(), self.on_query.clone());
            input = input.on_change(callback(move |event: &TextChangeEvent, cx| {
                if event.value == shared.query {
                    return Ok(Dispatch::Unchanged);
                }
                // Typing reopens the list on the first result of the new query.
                shared.state.try_update(cx, |s, _| {
                    s.open = true;
                    s.highlight = None;
                    s.query.clone_from(&event.value);
                })?;
                shared.scroll(cx, None);
                propose(
                    on_query.as_ref(),
                    &QueryEvent {
                        text: event.value.clone(),
                    },
                    cx,
                )?;
                Ok(Dispatch::Handled)
            }));
        }
        let (key_shared, inline, open) = (shared.clone(), self.inline, self.open);
        root = root.on_key_down(callback(move |input: &KeyInput, cx| {
            let shared = &key_shared;
            let m = input.event.modifiers;
            if m.alt {
                return Ok(Dispatch::Unchanged);
            }
            let last = len.checked_sub(1);
            let next = match &input.event.key {
                KeyboardKey::Escape if open && !inline => {
                    input.prevent_default();
                    return shared.set(cx, false, highlight);
                }
                KeyboardKey::Tab if open && !inline => return shared.set(cx, false, highlight),
                KeyboardKey::Enter if visible => {
                    let Some(position) = highlight else {
                        return Ok(Dispatch::Unchanged);
                    };
                    input.prevent_default();
                    return shared.choose(cx, position);
                }
                // Down or Up opens a closed popover on the current highlight.
                KeyboardKey::ArrowDown | KeyboardKey::ArrowUp if !visible => highlight,
                KeyboardKey::ArrowDown => {
                    highlight.map(|p| if Some(p) == last { 0 } else { p + 1 })
                }
                KeyboardKey::ArrowUp => highlight.and_then(|p| p.checked_sub(1).or(last)),
                KeyboardKey::PageDown if visible => highlight.map(|p| (p + page).min(len - 1)),
                KeyboardKey::PageUp if visible => highlight.map(|p| p.saturating_sub(page)),
                KeyboardKey::Home if visible && m.primary() => highlight.and(Some(0)),
                KeyboardKey::End if visible && m.primary() => last,
                _ => return Ok(Dispatch::Unchanged),
            };
            input.prevent_default();
            let dispatch = shared.set(cx, true, next)?;
            if next.is_some() {
                shared.scroll(cx, next);
            }
            Ok(dispatch)
        }));
        root = root.child(input);
        if !visible {
            return root;
        }

        let rows = len.clamp(1, MAX_ROWS);
        let mut list = column()
            .key("list")
            .fill_width()
            .height(rows as f32 * row_height)
            .accessibility_role(SemanticRole::ListBox)
            .accessibility_handle(list_handle);
        if let Some(labelled_by) = labelled_by {
            list = list.accessibility_labelled_by(labelled_by);
        }
        if let Some(name) = name {
            list = list.accessibility_label(name);
        }
        if len == 0 {
            list = list.child(
                row()
                    .key("empty")
                    .fill_width()
                    .list_row(false)
                    .align_items(AlignItems::CENTER)
                    .child(label(self.empty_text).color(ThemeColor::TextMuted)),
            );
        } else {
            let item = &self.item;
            let results = crate::virtual_list::virtual_list_in(
                len,
                row_height,
                &self.scroll,
                self.scroll_state,
                |position| {
                    let index = hits.index(position);
                    let ComboItem {
                        label: text,
                        icon,
                        detail,
                        trailing,
                    } = item(index);
                    let lit = highlight == Some(position);
                    let mut option = item_content(
                        icon,
                        highlighted(&text, hits.ranges(position), lit),
                        detail.as_deref(),
                        trailing,
                    )
                    .key(index)
                    .list_row(false)
                    .accessibility_role(SemanticRole::ListBoxOption)
                    .accessibility_label(text.into_owned())
                    .accessibility_selected(lit);
                    let semantics = option.semantics.get_or_insert_with(Default::default);
                    semantics.set_size = Some(len);
                    semantics.position_in_set = Some(position);
                    if lit {
                        option = option
                            .background(ThemeColor::RowHover)
                            .accessibility_handle(active.clone());
                    }
                    let hover = shared.clone();
                    option = option.on_hover(callback(move |event: &HoverEvent, cx| {
                        if event.hovered {
                            hover.set(cx, true, Some(position))
                        } else {
                            Ok(Dispatch::Unchanged)
                        }
                    }));
                    // Pressing keeps focus in the input; releasing chooses.
                    option = option.on_pointer_down(callback(|event: &PointerInput, _| {
                        if event.button == Some(PointerButton::Primary) {
                            event.prevent_default();
                        }
                        Ok(Dispatch::Unchanged)
                    }));
                    let press = shared.clone();
                    option.on_pointer_up(callback(move |event: &PointerInput, cx| {
                        if event.button != Some(PointerButton::Primary) {
                            return Ok(Dispatch::Unchanged);
                        }
                        press.choose(cx, position)
                    }))
                },
            )
            .plain()
            // Results that fit need no scrollbar.
            .scrollbars(len > MAX_ROWS)
            .fill_width()
            .fill_height();
            list = list.child(results);
        }
        if self.inline {
            return root.gap(4.).child(list);
        }
        let dismiss = shared.clone();
        root.child(
            popover(anchor, list)
                .key("results")
                .width(list_width)
                .padding(4.)
                .autofocus(false)
                .on_dismiss(callback(move |_: &DismissEvent, cx| {
                    dismiss.set(cx, false, highlight)
                })),
        )
    }
}
