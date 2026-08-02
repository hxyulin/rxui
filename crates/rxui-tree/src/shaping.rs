//! Per-element memos of shaped text.
//!
//! Shaping - itemization, font fallback, BiDi, kerning - is the most expensive
//! thing a text element does, and layout runs it unconditionally: a parent may
//! measure a child and then lay it out again at a resolved size, and any
//! invalidation anywhere in the subtree re-runs the whole pass. Keying the
//! retained [`TextLayout`] on the exact [`TextLayoutRequest`] that produced it
//! turns those repeats into pointer clones.
//!
//! Every element that draws text needs this, including elements outside this
//! crate, so both memos are public. Nothing here is specific to the builtin
//! set; the only thing they need is a [`LayoutContext`], which is what an
//! element's `layout` already has.

use std::{collections::HashMap, hash::Hash};

use astrelis_text::{TextLayout, TextLayoutRequest};

use crate::LayoutContext;

/// One element's memo of the last text it shaped.
///
/// The key is the whole normalized request, so every input that can change the
/// shaped output - text, style, colors, wrapping, width when it matters -
/// invalidates the memo on its own. There is nothing to invalidate manually,
/// and no element needs to know which of its public fields feed text.
///
/// One memo per shaped string, held by the element that shapes it. That keeps
/// the cache exactly as long as the text it belongs to, needs no eviction, and
/// stays deterministic for golden tests - unlike a shared cache, which would
/// have to clone the string on every lookup because [`TextLayoutRequest`] is
/// full of `f32` and cannot be `Hash`.
/// Cloning an element clones its memo, which is sound because the key travels
/// with it, and cheap because [`TextLayout`] is an `Arc`.
///
/// An element that shapes a fixed, small number of strings should hold one of
/// these per string, as a text field does for its value and its placeholder.
/// Use [`KeyedShapingMemo`] when the count is the length of a
/// collection instead.
#[derive(Clone, Default)]
pub struct ShapingMemo {
    entry: Option<(TextLayoutRequest, TextLayout)>,
}

impl ShapingMemo {
    /// Returns the layout for `request`, shaping it only when the memo does not
    /// already hold the identical request.
    ///
    /// Both the comparison and the stored key use
    /// [`TextLayoutRequest::normalized`], so a request whose only difference
    /// cannot reach the shaped output still hits.
    pub fn shape(
        &mut self,
        context: &mut LayoutContext<'_>,
        request: TextLayoutRequest,
    ) -> TextLayout {
        let request = request.normalized();
        if let Some((shaped, layout)) = &self.entry
            && *shaped == request
        {
            return layout.clone();
        }
        let layout = context.shape_text(request.clone());
        self.entry = Some((request, layout.clone()));
        layout
    }
}

/// Summarized rather than derived: the elements holding a memo already print the
/// layout they paint, and dumping every cached glyph run a second time would
/// bury the rest of their `Debug` output.
impl std::fmt::Debug for ShapingMemo {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("ShapingMemo")
            .field(&self.entry.as_ref().map(|(request, _)| &request.text))
            .finish()
    }
}

/// One element's memo of many shaped strings, one per key.
///
/// This exists because a [`ShapingMemo`] per *element* is the wrong shape for an
/// element whose text count is the length of a collection: a chart with axis
/// labels, a node graph with node titles, any custom list. A single entry there
/// is worse than no memo at all, because each string evicts the previous one and
/// every pass shapes all of them.
///
/// Key by domain identity, never by position. That is what makes an insertion
/// cost one miss rather than one per item after it, and a reorder cost none.
/// Everything else follows from it: [`Self::shape_all`] carries forward only the
/// entries this pass claimed and drops the rest, so the map stays exactly as
/// large as the collection and eviction needs no policy.
///
/// Two items sharing a key cost the second one a re-shape, because the first
/// already claimed the entry. That is slow rather than wrong.
#[derive(Clone)]
pub struct KeyedShapingMemo<Key> {
    entries: HashMap<Key, (TextLayoutRequest, TextLayout)>,
}

/// Hand-written because deriving would demand `Key: Default`, which an identity
/// type does not owe anyone.
impl<Key> Default for KeyedShapingMemo<Key> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

impl<Key: Eq + Hash> KeyedShapingMemo<Key> {
    /// Returns one layout per requested key, shaping only what changed, and
    /// forgets every key not present in `requests`.
    ///
    /// Pass the whole collection in one call. Shaping items individually would
    /// leave this unable to tell a removed key from one this pass happens not to
    /// have reached yet, which is the difference between a map that tracks the
    /// collection and one that only grows.
    pub fn shape_all(
        &mut self,
        context: &mut LayoutContext<'_>,
        requests: impl IntoIterator<Item = (Key, TextLayoutRequest)>,
    ) -> Vec<TextLayout> {
        let mut previous = std::mem::take(&mut self.entries);
        let mut layouts = Vec::new();
        for (key, request) in requests {
            let request = request.normalized();
            let layout = match previous.remove(&key) {
                Some((shaped, layout)) if shaped == request => layout,
                _ => context.shape_text(request.clone()),
            };
            self.entries.insert(key, (request, layout.clone()));
            layouts.push(layout);
        }
        layouts
    }

    /// Number of retained entries, which is the size of the collection the last
    /// [`Self::shape_all`] described.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether anything is retained.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Prints the keys rather than the glyph runs, for the reason [`ShapingMemo`]'s
/// own `Debug` gives.
impl<Key: std::fmt::Debug> std::fmt::Debug for KeyedShapingMemo<Key> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("KeyedShapingMemo")
            .field(&self.entries.keys().collect::<Vec<_>>())
            .finish()
    }
}
