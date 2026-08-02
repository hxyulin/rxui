//! Semantic-snapshot queries shared by this crate's unit tests.
//!
//! Retained identity is what most of these tests are about, and a `NodeId`
//! cannot be constructed by hand - the only way to name one is to find it in a
//! semantic snapshot. Both queries are here rather than duplicated per module
//! because "the focused node" and "the node labeled X" are the same two
//! questions every keyed-identity test asks.

use crate::{Component, ComponentHost};
use astrelis_ui_next::{NodeId, SemanticNode};

/// The single focused node, which every focus test expects to exist.
pub(crate) fn focused(host: &ComponentHost<impl Component>) -> SemanticNode {
    host.ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.focused)
        .expect("a focused node")
}

/// The identity of the node whose accessible label ends with `suffix`.
///
/// Matched on a suffix because several builders prefix a selection marker onto
/// the label they announce.
pub(crate) fn node_labeled(host: &ComponentHost<impl Component>, suffix: &str) -> NodeId {
    host.ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label.ends_with(suffix))
        .map(|node| node.id)
        .unwrap_or_else(|| panic!("no node labeled {suffix}"))
}
