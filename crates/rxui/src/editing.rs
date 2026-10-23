//! Controlled single-line and multiline editing and placement-local history.
//!
//! Adjacent single-grapheme non-whitespace typing and ordinary repeated deletion
//! coalesce until navigation, pointer selection, focus/activation change, composition
//! or a different edit kind. Paste and IME commits are separate transactions. History
//! records each step as the changed byte range and the text that restores it, so a
//! large document's history costs only its edits. It keeps at most 128 steps and
//! 1 MiB of restoring UTF-8 across undo/redo. Oversized steps are discarded without
//! rejecting the edit. There is no clock-based grouping.
//!
//! Undo/redo propose values through on_change. Rejection keeps history and selection;
//! normalization of a replay accepts the actual answer and resets the incompatible
//! chain. External controlled changes reset history in that placement, including
//! edits in another shared window. This is field history, not shared document history.
//! Applications can override standard Undo/Redo commands for document-level editing.
use std::ops::Range;
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

/// Logical side of a boundary with two visual positions in bidirectional text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAffinity {
    /// Attach to the preceding logical grapheme.
    Upstream,
    /// Attach to the following logical grapheme.
    #[default]
    Downstream,
}
/// Whole-value UTF-8 byte position at an extended-grapheme boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextPosition {
    /// Byte offset in the text snapshot.
    pub byte_offset: usize,
    /// Visual attachment at a bidirectional boundary.
    pub affinity: TextAffinity,
}
impl TextPosition {
    /// Creates a downstream position.
    pub const fn new(byte_offset: usize) -> Self {
        Self {
            byte_offset,
            affinity: TextAffinity::Downstream,
        }
    }
}
/// Directional selection retained by one text input placement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextSelection {
    /// Fixed end during an extended selection.
    pub anchor: TextPosition,
    /// Moving end and active caret.
    pub focus: TextPosition,
}
impl TextSelection {
    /// Collapsed selection at a byte offset.
    pub const fn caret(byte_offset: usize) -> Self {
        Self {
            anchor: TextPosition::new(byte_offset),
            focus: TextPosition::new(byte_offset),
        }
    }
    /// Ordered byte range, independent of selection direction.
    pub fn range(self) -> Range<usize> {
        self.anchor.byte_offset.min(self.focus.byte_offset)
            ..self.anchor.byte_offset.max(self.focus.byte_offset)
    }
    /// Whether the selection contains no bytes.
    pub fn is_collapsed(self) -> bool {
        self.anchor.byte_offset == self.focus.byte_offset
    }
}
/// Proposed committed value. The application accepts it by updating the controlled
/// property; it may also normalize or reject it. Preedit does not emit this event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextChangeEvent {
    /// Proposed complete value; only a text area's value contains line breaks/tabs.
    pub value: String,
    /// Proposed selection in that value.
    pub selection: TextSelection,
}
/// Input submission, normally from Enter outside IME composition. A text area
/// submits only through its [`SubmitShortcut`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSubmitEvent {
    /// Current application-controlled value.
    pub value: String,
}
/// Which Enter chord submits a text area; the others insert a line break.
/// Single-line inputs always submit on Enter. The primary modifier is Command on
/// macOS and Control elsewhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SubmitShortcut {
    /// Primary+Enter submits; Enter and Shift+Enter insert a line break (default).
    #[default]
    PrimaryEnter,
    /// Enter submits; Shift+Enter inserts a line break, as in chat composers.
    Enter,
    /// Enter always inserts a line break; on_submit is never dispatched by keys.
    Never,
}
/// Selection navigation supplied by a host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextMovement {
    /// Previous visually distinct caret, with a logical fallback in headless hosts.
    Left,
    /// Next visually distinct caret, with a logical fallback in headless hosts.
    Right,
    /// Previous Unicode word start in logical order. Password inputs move to the start.
    WordLeft,
    /// Next Unicode word end in logical order. Password inputs move to the end.
    WordRight,
    /// Start of the value.
    Start,
    /// End of the value.
    End,
    /// Start of the current visual line; the value start in single-line inputs.
    LineStart,
    /// End of the current visual line; the value end in single-line inputs.
    LineEnd,
    /// Previous visual line, keeping the horizontal caret position across repeated
    /// vertical moves. The first line moves to the start; single-line inputs too.
    Up,
    /// Next visual line, keeping the horizontal position. The last line moves to the
    /// end; single-line inputs move to the end.
    Down,
    /// Up by the visible height of the input, less one line.
    PageUp,
    /// Down by the visible height of the input, less one line.
    PageDown,
}
/// Backend-independent input. Editing is synchronous; rendering may follow later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputEvent {
    /// Replace selection with committed keyboard text. Single-line inputs turn line
    /// breaks/tabs into spaces; text areas keep them, normalizing CRLF/CR to LF.
    /// Clipboard hosts should use Paste to preserve a separate undo transaction.
    Insert(String),
    /// Inserts normalized clipboard text as one separate undo transaction.
    Paste(String),
    /// Delete selection or the preceding extended grapheme.
    Backspace,
    /// Delete selection or the following extended grapheme.
    Delete,
    /// Delete selection or back to the preceding Unicode word start.
    BackspaceWord,
    /// Delete selection or through the following Unicode word end.
    DeleteWord,
    /// Delete selection or back to the start of the single line.
    BackspaceToStart,
    /// Delete selection or through the end of the single line.
    DeleteToEnd,
    /// The Enter key with its modifiers. Single-line inputs submit; text areas insert
    /// a line break unless the chord matches their [`SubmitShortcut`]. Repeated keys
    /// insert line breaks but never submit. Hosts without modifier information can
    /// send [`Self::Submit`] or `Insert("\n")` instead.
    Enter {
        /// Modifiers held with Enter.
        modifiers: crate::Modifiers,
        /// Whether this is a key-repeat event.
        repeat: bool,
    },
    /// Propose the previous locally accepted value and restore its selection.
    /// External controlled changes reset this placement's bounded history.
    Undo,
    /// Replay a locally undone edit through the same controlled change listener.
    Redo,
    /// Move the caret, optionally keeping the anchor for selection extension.
    Move {
        /// Navigation direction/unit.
        movement: TextMovement,
        /// Preserve anchor rather than collapsing selection.
        extend: bool,
    },
    /// Select the complete controlled value.
    SelectAll,
    /// Transient IME text, with optional UTF-8 byte cursor endpoints inside preedit.
    Preedit {
        /// Composing text; empty text clears its display while retaining the replacement scope.
        text: String,
        /// Platform-provided byte endpoints; None means no visible composition caret.
        cursor: Option<(usize, usize)>,
    },
    /// Replace the original composition selection once with committed IME text.
    Commit(String),
    /// Discard transient composition without changing application data.
    CancelComposition,
    /// Dispatch on_submit without changing the value.
    Submit,
}
/// Read-only retained editing state for painting and host integration.
pub struct TextInputInfo {
    /// Whether this placement has an undo proposal available and permits editing.
    pub can_undo: bool,
    /// Whether this placement has a redo proposal available and permits editing.
    pub can_redo: bool,
    /// Selection in the application-controlled value, independent of preedit display.
    pub selection: TextSelection,
    /// Composition range in displayed text, when composing.
    pub preedit_range: Option<Range<usize>>,
    /// Displayed composition selection/caret; absent when the IME hides it.
    pub preedit_cursor: Option<Range<usize>>,
    /// Selection in the displayed text: equal to `selection` except in password
    /// inputs, whose display replaces each grapheme with one mask character.
    pub display_selection: TextSelection,
    /// Horizontal content offset keeping the active caret visible; zero in text areas.
    pub scroll_x: f32,
    /// Muted theme color for the placeholder, when one is shown.
    pub placeholder_color: crate::Color,
    /// Whether painting should treat the displayed text as masked password text.
    pub masked: bool,
    /// Whether the value may be edited; read-only inputs still allow selection/copy.
    pub read_only: bool,
    /// Whether painting should show the active caret.
    pub caret_visible: bool,
}
/// Per-input options beyond the common controlled-input fields, allocated only when
/// submission, a placeholder, masking or multiline editing is requested.
#[derive(Clone, Default)]
pub(crate) struct InputOptions {
    pub on_submit: Option<crate::Listener<TextSubmitEvent>>,
    pub placeholder: Option<String>,
    pub password: bool,
    pub multiline: bool,
    pub shortcut: SubmitShortcut,
}
pub(crate) struct Composition {
    pub text: String,
    pub cursor: Option<(usize, usize)>,
    pub replacement: TextSelection,
}
pub(crate) struct Pending {
    pub proposed: String,
    pub previous: TextSelection,
    pub action: EditAction,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditKind {
    Typing,
    Backspace,
    Delete,
    Separate,
}
#[derive(Clone, Copy)]
pub(crate) enum EditAction {
    Edit(EditKind),
    Undo,
    Redo,
}
pub(crate) struct Proposal<'a> {
    pub range: Range<usize>,
    pub insert: &'a str,
    pub action: EditAction,
    pub selection: Option<TextSelection>,
}
impl<'a> Proposal<'a> {
    pub fn new(range: Range<usize>, insert: &'a str, kind: EditKind) -> Self {
        Self {
            range,
            insert,
            action: EditAction::Edit(kind),
            selection: None,
        }
    }
}
/// One reversible history step: replacing `range` of the current value with `text`
/// restores the recorded value and its selection.
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub range: Range<usize>,
    pub text: String,
    pub selection: TextSelection,
}
/// Byte lengths of the longest common prefix and suffix of two values, aligned to
/// UTF-8 character boundaries and never overlapping in either value.
pub(crate) fn common_ends(old: &str, new: &str) -> (usize, usize) {
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let n = a.len().min(b.len());
    let mut prefix = 0;
    // Slice comparison uses memcmp; the byte loop only finishes the last chunk.
    while prefix + 64 <= n && a[prefix..prefix + 64] == b[prefix..prefix + 64] {
        prefix += 64;
    }
    while prefix < n && a[prefix] == b[prefix] {
        prefix += 1;
    }
    while !old.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let limit = n - prefix;
    let mut suffix = 0;
    while suffix + 64 <= limit
        && a[a.len() - suffix - 64..a.len() - suffix] == b[b.len() - suffix - 64..b.len() - suffix]
    {
        suffix += 64;
    }
    while suffix < limit && a[a.len() - suffix - 1] == b[b.len() - suffix - 1] {
        suffix += 1;
    }
    while !old.is_char_boundary(old.len() - suffix) {
        suffix -= 1;
    }
    (prefix, suffix)
}
/// The step that turns `new` back into `old`, restoring `selection`.
fn reverse_step(old: &str, new: &str, selection: TextSelection) -> Snapshot {
    let (prefix, suffix) = common_ends(old, new);
    Snapshot {
        range: prefix..new.len() - suffix,
        text: old[prefix..old.len() - suffix].to_owned(),
        selection,
    }
}
/// At most 128 stored steps and 1 MiB of restoring UTF-8 across both stacks per placement.
#[derive(Clone, Default)]
pub(crate) struct History {
    undo: std::collections::VecDeque<Snapshot>,
    redo: std::collections::VecDeque<Snapshot>,
    bytes: usize,
    group: Option<(EditKind, TextSelection)>,
}
impl History {
    fn clear(&mut self) {
        *self = Self::default();
    }
    fn trim(&mut self) {
        while self.bytes > 1_048_576 || self.undo.len() + self.redo.len() > 128 {
            let removed = self
                .undo
                .pop_front()
                .or_else(|| self.redo.pop_front())
                .unwrap();
            self.bytes -= removed.text.len();
        }
    }
    fn accept(
        &mut self,
        old: &str,
        new: &str,
        before: TextSelection,
        after: TextSelection,
        action: EditAction,
        proposed: &str,
    ) {
        match action {
            EditAction::Edit(kind) => {
                self.bytes -= self.redo.iter().map(|s| s.text.len()).sum::<usize>();
                self.redo.clear();
                let merge = kind != EditKind::Separate
                    && self.group == Some((kind, before))
                    && !self.undo.is_empty();
                let step = reverse_step(old, new, before);
                if merge {
                    // Compose the top step (old -> group start) with this edit
                    // (new -> old) over the union of both changed regions of `old`.
                    // Outside that union, old and new differ only by a shift.
                    let top = self.undo.back_mut().unwrap();
                    let changed = step.range.start..old.len() - (new.len() - step.range.end);
                    let low = top.range.start.min(changed.start);
                    let high = top.range.end.max(changed.end);
                    let mut text =
                        String::with_capacity(high - low - top.range.len() + top.text.len());
                    text.push_str(&old[low..top.range.start]);
                    text.push_str(&top.text);
                    text.push_str(&old[top.range.end..high]);
                    self.bytes = self.bytes - top.text.len() + text.len();
                    top.range = low..high + new.len() - old.len();
                    top.text = text;
                } else {
                    self.bytes += step.text.len();
                    self.undo.push_back(step);
                }
                self.trim();
                self.group =
                    (kind != EditKind::Separate && !self.undo.is_empty()).then_some((kind, after));
            }
            EditAction::Undo | EditAction::Redo if new == proposed => {
                let (source, destination) = match action {
                    EditAction::Undo => (&mut self.undo, &mut self.redo),
                    _ => (&mut self.redo, &mut self.undo),
                };
                if let Some(removed) = source.pop_back() {
                    self.bytes -= removed.text.len();
                    let step = reverse_step(old, new, before);
                    self.bytes += step.text.len();
                    destination.push_back(step);
                }
                self.trim();
                self.group = None;
            }
            // Application normalization changes the replay chain. Keep its actual
            // answer, but do not leave stale history describing a different value.
            _ => self.clear(),
        }
    }
}
pub(crate) struct Editor {
    pub life: std::rc::Rc<()>,
    pub selection: TextSelection,
    pub display: String,
    pub composition: Option<Composition>,
    pub pending: Option<Pending>,
    pub scroll_x: f32,
    pub ime_reset_revision: u64,
    pub(super) history: History,
    pub drag_selection: Option<TextSelection>,
    pub drag_line: bool,
    /// Password inputs display one mask character per grapheme.
    pub masked: bool,
    /// Preferred horizontal caret position, valid while the selection is still the
    /// one a vertical move produced.
    pub goal: Option<(f32, TextSelection)>,
    /// The caret moved or the text changed: reveal the caret through scrolling
    /// ancestors once layout is current.
    pub reveal: bool,
}
/// The mask character shown for each grapheme of a password input.
pub(crate) const MASK: char = '\u{2022}';
impl Editor {
    pub fn new(value: &str, masked: bool) -> Self {
        let mut editor = Self {
            life: Default::default(),
            selection: TextSelection::caret(value.len()),
            display: String::new(),
            composition: None,
            pending: None,
            scroll_x: 0.,
            ime_reset_revision: 0,
            history: History::default(),
            drag_selection: None,
            drag_line: false,
            masked,
            goal: None,
            reveal: false,
        };
        editor.rebuild(value);
        editor
    }
    pub fn reconcile(&mut self, old: &str, new: &str) {
        if self.pending.is_none() && old == new {
            return;
        }
        let pending = self.pending.take();
        if let Some(pending) = &pending {
            if new == old && new != pending.proposed {
                self.selection = pending.previous;
                self.break_group();
            }
        } else if old != new {
            self.history.clear();
        }
        if old != new {
            self.cancel_composition();
        }
        self.selection.anchor.byte_offset = boundary(new, self.selection.anchor.byte_offset);
        self.selection.focus.byte_offset = boundary(new, self.selection.focus.byte_offset);
        if old != new
            && let Some(pending) = pending
        {
            self.history.accept(
                old,
                new,
                pending.previous,
                self.selection,
                pending.action,
                &pending.proposed,
            );
        }
        self.rebuild(new);
    }
    pub fn break_group(&mut self) {
        self.history.group = None;
    }
    /// The next replay step, provided it still addresses `value`.
    pub fn replay(&mut self, value: &str, redo: bool) -> Option<&Snapshot> {
        let stack = if redo {
            &self.history.redo
        } else {
            &self.history.undo
        };
        let range = stack.back()?.range.clone();
        if range.end > value.len()
            || !value.is_char_boundary(range.start)
            || !value.is_char_boundary(range.end)
        {
            // History always describes the current value; never apply a step to
            // text it was not recorded against.
            self.history.clear();
            return None;
        }
        if redo {
            self.history.redo.back()
        } else {
            self.history.undo.back()
        }
    }
    pub fn cancel_composition(&mut self) -> bool {
        if self.composition.take().is_none() {
            return false;
        }
        self.ime_reset_revision = self
            .ime_reset_revision
            .checked_add(1)
            .expect("RXUI IME reset revision exhausted");
        true
    }
    pub fn rebuild(&mut self, value: &str) {
        self.display.clear();
        if self.masked {
            // Masked inputs accept no composition; see Ui::text_input.
            self.display.extend(value.graphemes(true).map(|_| MASK));
        } else if let Some(c) = &self.composition {
            let range = c.replacement.range();
            self.display.push_str(&value[..range.start]);
            self.display.push_str(&c.text);
            self.display.push_str(&value[range.end..]);
        } else {
            self.display.push_str(value);
        }
    }
    /// Maps a committed-value position into the displayed text.
    pub fn to_display(&self, value: &str, position: TextPosition) -> TextPosition {
        if !self.masked {
            return position;
        }
        TextPosition {
            byte_offset: value[..position.byte_offset.min(value.len())]
                .graphemes(true)
                .count()
                * MASK.len_utf8(),
            ..position
        }
    }
    /// Maps a displayed-text position back into the committed value.
    pub fn to_value(&self, value: &str, position: TextPosition) -> TextPosition {
        if !self.masked {
            return position;
        }
        TextPosition {
            byte_offset: value
                .grapheme_indices(true)
                .nth(position.byte_offset / MASK.len_utf8())
                .map_or(value.len(), |(i, _)| i),
            ..position
        }
    }
    pub fn display_position(&self, value: &str) -> Option<TextPosition> {
        match &self.composition {
            Some(c) => c.cursor.map(|(_, end)| {
                TextPosition::new(boundary(&self.display, c.replacement.range().start + end))
            }),
            None => Some(self.to_display(value, self.selection.focus)),
        }
    }
    pub fn ime_position(&self, value: &str) -> TextPosition {
        self.display_position(value).unwrap_or_else(|| {
            TextPosition::new(boundary(
                &self.display,
                self.composition
                    .as_ref()
                    .map_or(self.selection.focus.byte_offset, |c| {
                        c.replacement.range().start
                    }),
            ))
        })
    }
    pub fn info(&self, read_only: bool, caret_visible: bool) -> TextInputInfo {
        let start = self
            .composition
            .as_ref()
            .map(|c| c.replacement.range().start);
        TextInputInfo {
            can_undo: !read_only && self.composition.is_none() && !self.history.undo.is_empty(),
            can_redo: !read_only && self.composition.is_none() && !self.history.redo.is_empty(),
            selection: self.selection,
            display_selection: self.selection,
            preedit_range: self.composition.as_ref().map(|c| {
                boundary(&self.display, start.unwrap())
                    ..after_boundary(&self.display, start.unwrap() + c.text.len())
            }),
            preedit_cursor: self.composition.as_ref().and_then(|c| {
                c.cursor.map(|(a, b)| {
                    boundary(&self.display, start.unwrap() + a)
                        ..after_boundary(&self.display, start.unwrap() + b)
                })
            }),
            scroll_x: self.scroll_x,
            placeholder_color: [0.; 4],
            masked: self.masked,
            read_only,
            caret_visible,
        }
    }
}
/// Start of the paragraph (logical line) containing `offset`.
pub(crate) fn paragraph_start(value: &str, offset: usize) -> usize {
    value[..offset].rfind('\n').map_or(0, |i| i + 1)
}
/// End of the paragraph containing `offset`, before its line break.
pub(crate) fn paragraph_end(value: &str, offset: usize) -> usize {
    value[offset..]
        .find('\n')
        .map_or(value.len(), |i| offset + i)
}
/// Words never span a line break, so word navigation segments one paragraph at a
/// time rather than the whole value.
pub(crate) fn word_left(value: &str, mut offset: usize) -> usize {
    loop {
        let start = paragraph_start(value, offset);
        let end = paragraph_end(value, offset);
        if let Some(i) = value[start..end]
            .unicode_word_indices()
            .map(|(i, _)| start + i)
            .take_while(|i| *i < offset)
            .last()
        {
            return boundary(value, i);
        }
        if start == 0 {
            return 0;
        }
        offset = start - 1;
    }
}
pub(crate) fn word_right(value: &str, mut offset: usize) -> usize {
    loop {
        let start = paragraph_start(value, offset);
        let end = paragraph_end(value, offset);
        if let Some(i) = value[start..end]
            .unicode_word_indices()
            .map(|(i, w)| start + i + w.len())
            .find(|i| *i > offset)
        {
            return after_boundary(value, i);
        }
        if end == value.len() {
            return value.len();
        }
        offset = end + 1;
    }
}
pub(crate) fn word_selection(value: &str, position: TextPosition) -> TextSelection {
    let offset =
        if position.affinity == TextAffinity::Upstream || position.byte_offset == value.len() {
            previous(value, position.byte_offset)
        } else {
            position.byte_offset
        };
    let start = paragraph_start(value, offset);
    let end = paragraph_end(value, offset);
    let range = value[start..end]
        .split_word_bound_indices()
        .map(|(i, w)| (start + i, w))
        .find(|(i, w)| *i <= offset && offset < i + w.len())
        .map_or(offset..offset, |(i, w)| {
            boundary(value, i)..after_boundary(value, i + w.len())
        });
    TextSelection {
        anchor: TextPosition::new(range.start),
        focus: TextPosition::new(range.end),
    }
}
/// The paragraph containing `offset`, including its line break, for triple-click.
pub(crate) fn paragraph_selection(value: &str, offset: usize) -> TextSelection {
    let offset = offset.min(value.len());
    let end = paragraph_end(value, offset);
    TextSelection {
        anchor: TextPosition::new(paragraph_start(value, offset)),
        focus: TextPosition::new(if end < value.len() { end + 1 } else { end }),
    }
}
/// Largest extended-grapheme boundary not after `offset`. Segmentation is local to
/// the offset, so the cost does not grow with the length of the value.
pub(crate) fn boundary(value: &str, offset: usize) -> usize {
    if offset >= value.len() {
        return value.len();
    }
    let mut offset = offset;
    while !value.is_char_boundary(offset) {
        offset -= 1;
    }
    let mut cursor = GraphemeCursor::new(offset, value.len(), true);
    if cursor.is_boundary(value, 0).unwrap_or(true) {
        return offset;
    }
    cursor.prev_boundary(value, 0).ok().flatten().unwrap_or(0)
}
pub(crate) fn after_boundary(value: &str, offset: usize) -> usize {
    let floor = boundary(value, offset);
    if floor == offset {
        floor
    } else {
        next(value, floor)
    }
}
/// Largest grapheme boundary before `offset`, or zero.
pub(crate) fn previous(value: &str, offset: usize) -> usize {
    let offset = offset.min(value.len());
    let floor = boundary(value, offset);
    if floor < offset {
        return floor;
    }
    let mut cursor = GraphemeCursor::new(floor, value.len(), true);
    cursor.prev_boundary(value, 0).ok().flatten().unwrap_or(0)
}
/// Smallest grapheme boundary after `offset`, or the value length.
pub(crate) fn next(value: &str, offset: usize) -> usize {
    if offset >= value.len() {
        return value.len();
    }
    let mut cursor = GraphemeCursor::new(boundary(value, offset), value.len(), true);
    cursor
        .next_boundary(value, 0)
        .ok()
        .flatten()
        .unwrap_or(value.len())
}
/// Whether a controlled value contains control characters other than the line
/// breaks and tabs a text area allows. Descriptions are validated on every
/// evaluation, so this scans bytes in fixed blocks that compile to vector compares
/// and decodes only blocks that may hold a C0, DEL or C1 control.
pub(crate) fn has_control(value: &str, multiline: bool) -> bool {
    let suspect = |b: u8| {
        (b < 0x20 && !(multiline && (b == b'\n' || b == b'\t'))) | (b == 0x7f) | (b == 0xc2)
    };
    let mut offset = 0;
    for block in value.as_bytes().chunks(64) {
        if block.iter().fold(false, |any, &b| any | suspect(b)) {
            // C1 controls are U+0080..=U+009F, encoded 0xC2 0x80..=0x9F, which may
            // straddle the block end; decode from the previous char boundary.
            let mut start = offset;
            while !value.is_char_boundary(start) {
                start -= 1;
            }
            let end = (offset + block.len() + 1).min(value.len());
            let mut end = end;
            while !value.is_char_boundary(end) {
                end += 1;
            }
            if value[start..end]
                .chars()
                .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\t')))
            {
                return true;
            }
        }
        offset += block.len();
    }
    false
}
pub(crate) fn single_line(value: &str) -> String {
    value
        .graphemes(true)
        .filter_map(|g| {
            if matches!(g, "\r\n" | "\n\r" | "\n" | "\r" | "\t") {
                Some(" ")
            } else if g.chars().any(char::is_control) {
                None
            } else {
                Some(g)
            }
        })
        .collect()
}
/// Text-area normalization: CRLF and CR become LF, tabs remain, and other control
/// characters are discarded.
pub(crate) fn multi_line(value: &str) -> String {
    value
        .graphemes(true)
        .filter_map(|g| {
            if matches!(g, "\r\n" | "\n" | "\r") {
                Some("\n")
            } else if g == "\t" || !g.chars().any(char::is_control) {
                Some(g)
            } else {
                None
            }
        })
        .collect()
}
/// Logical-line vertical movement for hosts without text geometry: the target
/// paragraph keeps the grapheme column, clamped to its length. Moving past the
/// first or last paragraph reaches the start or end of the value.
pub(crate) fn logical_vertical(value: &str, offset: usize, lines: isize, column: usize) -> usize {
    let mut start = paragraph_start(value, offset);
    for _ in 0..lines.unsigned_abs() {
        if lines < 0 {
            if start == 0 {
                return 0;
            }
            start = paragraph_start(value, start - 1);
        } else {
            let end = paragraph_end(value, start);
            if end == value.len() {
                return value.len();
            }
            start = end + 1;
        }
    }
    let end = paragraph_end(value, start);
    value[start..end]
        .grapheme_indices(true)
        .nth(column)
        .map_or(end, |(i, _)| start + i)
}
/// Grapheme column of `offset` within its paragraph.
pub(crate) fn logical_column(value: &str, offset: usize) -> usize {
    value[paragraph_start(value, offset)..offset]
        .graphemes(true)
        .count()
}
