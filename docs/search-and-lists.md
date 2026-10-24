# Search and lists

`combobox` filters and ranks a list of items as the user types, for quick open,
command palettes and search fields. `list_view` is a selectable, virtualized
list. Both are controlled: the application owns the query, the items and the
selection, and accepts typed proposals. Both build their rows with
[`list_row`](virtual-lists.md) at the theme's `row_height`, so 50,000 items
mount only the visible rows.

## Combobox

```rust
combobox(cx)
    .accessibility_label("Go to file")
    .query(&self.query)
    .on_query(cx.listener(|s, e: &QueryEvent, _| s.query = e.text.clone()))
    .items(self.files.len(), |i| ComboItem::new(&self.files[i].name)
        .icon(Icon::FILE).detail(&self.files[i].dir))
    .matcher(FuzzyMatcher)
    .on_choose(cx.listener(|s, e: &ChooseEvent, _| s.open(e.index)))
```

A combobox is a text input with a results list. By default the list opens in a
popover below the input while the user types. The popover is as wide as a fixed
`.width(...)`, else 320 units; set `.list_width(...)` when the combobox fills its
parent. `.inline()` shows the list below the input in normal flow and keeps it
visible, for palette dialogs. Focus stays in the input in both modes.

| Key | Effect |
| --- | --- |
| Typing | Proposes the query through `QueryEvent`, reopens the popover and highlights the first result. |
| Up, Down | Move the highlight and wrap. In a closed popover they open it. |
| PageUp, PageDown | Move the highlight by the visible rows, stopping at the ends. |
| Primary+Home, Primary+End | Highlight the first and last result. Home and End alone move the caret. |
| Enter | Chooses the highlighted result. In a closed popover Enter belongs to the input. |
| Escape | Closes an open popover. Otherwise the combobox leaves it unhandled, so an enclosing dialog closes. |
| Tab | Closes an open popover and moves focus. |

Hovering a result highlights it, and a click chooses it without taking focus
from the input. `ChooseEvent::index` is the item's index in `.items(...)`, not its
position in the results. Choosing closes the popover.

The highlight is placement-local state. It starts on the first result whenever
the query changes, including changes the application makes. While the query
stays the same, it follows its item index through changed results, and moves to
the first result when that index no longer matches. Keyboard moves scroll the
highlight into view.

Rows show an optional icon, the label, the `.detail(...)` text in `TextMuted`,
and `.trailing(...)` content, such as a [`kbd`](controls.md#key-caps) shortcut,
at the row's end. Matched characters are drawn at medium weight in `Focus`, which
is readable as text at 4.5:1. On the highlighted row they keep the text color,
because `Focus` drops below 4.5:1 over the row's hover fill in dark themes. The
list is at most 10 rows tall. With no results it shows one muted row, "No
results" by default, set with `.empty_text(...)`.

The input has the ComboBox role with expanded state, `controls` pointing at the
ListBox, and the highlighted ListBoxOption as its `active_descendant`, as in
[selects](controls.md#selects). Options publish selected state, position and set
size. `.accessibility_label(...)` names the input and the list.

Open state, highlight and the results cache belong to the call site in the
current placement. Comboboxes built in a loop need `combobox_keyed(cx, key)`.

### Matchers

`Matcher` is a trait:

```rust
pub trait Matcher: 'static {
    fn score(&self, query: &str, candidate: &str) -> Option<Match>;
}
pub struct Match {
    pub score: i32,
    pub ranges: Vec<Range<usize>>,
}
```

`ranges` are UTF-8 byte ranges into the candidate. Highlighting skips a range
that does not start and end on a char boundary. Results sort by score, highest
first, and equal scores keep the items' order. An empty query shows every item
in order without calling the matcher.

| Matcher | Behavior |
| --- | --- |
| `FuzzyMatcher` (default) | Case-insensitive subsequence. |
| `SubstringMatcher` | Case-insensitive substring. Earlier matches, word starts and shorter candidates rank first. |
| `PassthroughMatcher` | Keeps every item in order, highlighting nothing. Use it when the application filters the items itself. |

`FuzzyMatcher` scores the best alignment of the query in the candidate. Each
matched character scores 16. It scores 8 more at a word start: the candidate's
start, or after a character that is not a letter, digit or `.`, such as `_`,
`-`, `/`, `:` or a space. A dot starts a file extension, so a match in the stem
of `theme.md` scores the same as in `theme.rs`. A lowercase-to-uppercase step
scores 7. A match right after
the previous one scores 4 and keeps the word-start bonus of the run's first
match. Matching the first query character at the candidate's start scores 8, and
an exact-case match scores 1. A gap between matches costs 5 plus 1 per extra
character, and each character of the candidate costs 1. For the query `thm`,
`theme.rs` ranks above `themes/mod.rs`, which ranks above `path/to/the_main.rs`.

Matching runs on the UI thread. The combobox caches the ranked results per
query, item labels and matcher type, so redescribing the view for unrelated
state does not score again. Checking the labels still calls the items closure
for every item on each description; `ComboItem::new` borrows a `&str` label
without copying it. On an Apple M-series machine, scoring 50,000 paths takes 1
to 9 ms with `FuzzyMatcher` and about 1.5 ms with `SubstringMatcher`, and a
cached redescription of a 50,000-item combobox takes about 0.5 ms:

```sh
cargo bench -p rxui --bench combobox --locked
```

Applications with larger sets, or slower matchers, filter themselves, off the UI
thread if needed, and pass the results with `PassthroughMatcher`.

The [`command_palette`](../crates/rxui/examples/command_palette.rs) example opens
an inline combobox over the application's commands in a modal with
Primary+Shift+P.

## List views

```rust
list_view(cx)
    .accessibility_label("Files")
    .height(240.)
    .selection(&self.selection)
    .items(self.names.len(), |i| ListItem::new(i, &self.names[i]).icon(Icon::FILE))
    .on_change(cx.listener(|s, e: &ListSelectionEvent, _| s.selection = e.selection.clone()))
```

`ListSelection::new(SelectionMode::Single | SelectionMode::Multiple)` holds the
selected keys, the active row's key and the range anchor. The list proposes a
whole new `ListSelection` in `ListSelectionEvent`; store it to accept it. A
proposal that changes nothing is not sent. `ListSelection::set` replaces the
selection from the application. Item keys must be unique.

The list is one Tab stop. It keeps focus on its root and publishes the active
row as the ListBox's active descendant.

| Input | Effect |
| --- | --- |
| Up, Down, Home, End, PageUp, PageDown | Move the active row and select only it. |
| Primary with a move | Moves the active row and keeps the selection. |
| Space | Selects only the active row. Primary+Space toggles it in multiple mode. |
| Shift with a move or click | In multiple mode, selects the range from the anchor. Primary+Shift adds the range to the selection. |
| Primary+A | Selects every row in multiple mode. |
| Click | Selects the row and focuses the list. Primary-click toggles it in multiple mode. |
| Typing | Selects the next row whose label starts with the typed text. Input within 700 ms extends the prefix, and a repeated letter cycles. |

Selected rows paint `RowSelected` while focus is in the list and
`RowSelectedInactive` otherwise. Rows publish ListBoxOption with selected state,
position and set size, and the ListBox publishes `multiselectable` in multiple
mode. Assistive Select and Activate actions select a row like a click. The list
reveals the proposed active row; a rejected proposal leaves the scroll position
where it moved.

The items closure runs for every item on each description, to collect the keys
and labels that keyboard input needs. Use [`virtual_list`](virtual-lists.md)
directly for very large sets.

[`virtual_tree`](virtual-trees.md) shares the selection model: `SelectionMode`,
the anchor, Shift ranges, Primary toggles and type-ahead behave the same in both.

## Verification

Matcher tests cover ranking on realistic paths, word-start and camelCase bonuses,
the best alignment over the leftmost one, case handling, byte ranges on Unicode
char boundaries, empty queries and misses. Combobox tests cover semantics,
ranked results and choosing by item index, wrapping, page and Primary+Home/End
movement with reveal, Escape in both modes, highlight reset and preservation,
hover and click without moving focus, the empty state, inline mode, matched-text
spans, and 50,000 items mounting only visible rows and scoring once per query.
List view tests cover single and multiple selection, Shift ranges, Primary
toggles, select-all, type-ahead, focused and unfocused selection paint,
semantics, one Tab stop and assistive selection.
