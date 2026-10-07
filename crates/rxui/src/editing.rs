//! Controlled single-line editing and placement-local history.
//!
//! Adjacent single-grapheme non-whitespace typing and ordinary repeated deletion
//! coalesce until navigation, pointer selection, focus/activation change, composition
//! or a different edit kind. Paste and IME commits are separate transactions. History
//! keeps at most 128 snapshots and 1 MiB of UTF-8 across undo/redo. Oversized snapshots
//! are discarded without rejecting the edit. There is no clock-based grouping.
//!
//! Undo/redo propose values through on_change. Rejection keeps history and selection;
//! normalization of a replay accepts the actual answer and resets the incompatible
//! chain. External controlled changes reset history in that placement, including
//! edits in another shared window. This is field history, not shared document history.
//! Applications can override standard Undo/Redo commands for document-level editing.
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

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
    /// Proposed complete single-line value.
    pub value: String,
    /// Proposed selection in that value.
    pub selection: TextSelection,
}
/// Single-line input submission, normally from Enter outside IME composition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSubmitEvent {
    /// Current application-controlled value.
    pub value: String,
}
/// Selection navigation supplied by a host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextMovement {
    /// Previous visually distinct caret, with a logical fallback in headless hosts.
    Left,
    /// Next visually distinct caret, with a logical fallback in headless hosts.
    Right,
    /// Previous Unicode word start in logical order.
    WordLeft,
    /// Next Unicode word end in logical order.
    WordRight,
    /// Start of the single-line value.
    Start,
    /// End of the single-line value.
    End,
}
/// Backend-independent input. Editing is synchronous; rendering may follow later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputEvent {
    /// Replace selection with committed keyboard text. Line breaks/tabs become spaces.
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
    /// Horizontal content offset keeping the active caret visible.
    pub scroll_x: f32,
    /// Whether the value may be edited; read-only inputs still allow selection/copy.
    pub read_only: bool,
    /// Whether painting should show the active caret.
    pub caret_visible: bool,
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
pub(crate) struct Snapshot {
    pub value: String,
    pub selection: TextSelection,
}
/// At most 128 stored snapshots and 1 MiB of UTF-8 across both stacks per placement.
#[derive(Default)]
struct History {
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
            self.bytes -= removed.value.len();
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
        let snapshot = || Snapshot {
            value: old.to_owned(),
            selection: before,
        };
        match action {
            EditAction::Edit(kind) => {
                self.bytes -= self.redo.iter().map(|s| s.value.len()).sum::<usize>();
                self.redo.clear();
                let merge = kind != EditKind::Separate
                    && self.group == Some((kind, before))
                    && !self.undo.is_empty();
                if !merge {
                    self.bytes += old.len();
                    self.undo.push_back(snapshot());
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
                    self.bytes -= removed.value.len();
                    self.bytes += old.len();
                    destination.push_back(snapshot());
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
    pub selection: TextSelection,
    pub display: String,
    pub composition: Option<Composition>,
    pub pending: Option<Pending>,
    pub scroll_x: f32,
    pub ime_reset_revision: u64,
    history: History,
    pub drag_selection: Option<TextSelection>,
    pub drag_line: bool,
}
impl Editor {
    pub fn new(value: &str) -> Self {
        Self {
            selection: TextSelection::caret(value.len()),
            display: value.into(),
            composition: None,
            pending: None,
            scroll_x: 0.,
            ime_reset_revision: 0,
            history: History::default(),
            drag_selection: None,
            drag_line: false,
        }
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
    pub fn replay(&self, redo: bool) -> Option<&Snapshot> {
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
        if let Some(c) = &self.composition {
            let range = c.replacement.range();
            self.display.push_str(&value[..range.start]);
            self.display.push_str(&c.text);
            self.display.push_str(&value[range.end..]);
        } else {
            self.display.push_str(value);
        }
    }
    pub fn display_position(&self) -> Option<TextPosition> {
        match &self.composition {
            Some(c) => c.cursor.map(|(_, end)| {
                TextPosition::new(boundary(&self.display, c.replacement.range().start + end))
            }),
            None => Some(self.selection.focus),
        }
    }
    pub fn ime_position(&self) -> TextPosition {
        self.display_position().unwrap_or_else(|| {
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
            read_only,
            caret_visible,
        }
    }
}
pub(crate) fn word_left(value: &str, offset: usize) -> usize {
    boundary(
        value,
        value
            .unicode_word_indices()
            .map(|(i, _)| i)
            .take_while(|i| *i < offset)
            .last()
            .unwrap_or(0),
    )
}
pub(crate) fn word_right(value: &str, offset: usize) -> usize {
    after_boundary(
        value,
        value
            .unicode_word_indices()
            .map(|(i, w)| i + w.len())
            .find(|i| *i > offset)
            .unwrap_or(value.len()),
    )
}
pub(crate) fn word_selection(value: &str, position: TextPosition) -> TextSelection {
    let offset =
        if position.affinity == TextAffinity::Upstream || position.byte_offset == value.len() {
            previous(value, position.byte_offset)
        } else {
            position.byte_offset
        };
    let range = value
        .split_word_bound_indices()
        .find(|(i, w)| *i <= offset && offset < i + w.len())
        .map_or(0..0, |(i, w)| {
            boundary(value, i)..after_boundary(value, i + w.len())
        });
    TextSelection {
        anchor: TextPosition::new(range.start),
        focus: TextPosition::new(range.end),
    }
}
pub(crate) fn boundary(value: &str, offset: usize) -> usize {
    if offset >= value.len() {
        return value.len();
    }
    value
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .take_while(|i| *i <= offset)
        .last()
        .unwrap_or(0)
}
pub(crate) fn after_boundary(value: &str, offset: usize) -> usize {
    let floor = boundary(value, offset);
    if floor == offset {
        floor
    } else {
        next(value, floor)
    }
}
pub(crate) fn previous(value: &str, offset: usize) -> usize {
    value
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .take_while(|i| *i < offset)
        .last()
        .unwrap_or(0)
}
pub(crate) fn next(value: &str, offset: usize) -> usize {
    value
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .find(|i| *i > offset)
        .unwrap_or(value.len())
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
