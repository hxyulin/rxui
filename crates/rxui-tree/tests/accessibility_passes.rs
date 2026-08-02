//! Accessibility pass pruning, inherited state, and delta bookkeeping.
//!
//! The pass carries effective enablement and visibility down one recursive walk
//! and prunes subtrees that inherit what they inherited last time and carry no
//! accessibility work. Correctness is covered by the differential sweep in
//! `compose_equivalence.rs`, which compares published semantics against a
//! freshly built tree; what these tests pin is the *cost*, which no correctness
//! assertion can observe. A pass that quietly degraded back into a sweep of
//! every node would keep every other test in the crate green.
//!
//! Every tree here uses [`FontDatabase::empty`], and describes itself through
//! boxes rather than labels, so ten thousand nodes cost no shaping at all.

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalRect, LogicalSize},
};
use astrelis_text::FontDatabase;
use rxui_tree::{
    Axis, BoxElement, Constraints, Element, Flex, LayoutContext, NodeHandle, NodeId, Scroll,
    ScrollAxis, SemanticData, SemanticNode, SemanticRole, Stack, UiTree,
};

struct FocusableBox {
    label: &'static str,
    size: LogicalSize,
}

impl Element for FocusableBox {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn layout(&mut self, _: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        constraints.constrain(self.size)
    }
    fn accessibility(&self) -> Option<SemanticData> {
        Some(SemanticData {
            role: SemanticRole::Button,
            label: self.label.into(),
            ..SemanticData::default()
        })
    }
    fn focusable(&self) -> bool {
        true
    }
}

/// A box that describes itself, so a node's presence in the semantic tree is
/// observable without shaping any text.
fn described(label: &str) -> BoxElement {
    BoxElement {
        size: LogicalSize::new(10.0, 4.0),
        color: Color::WHITE,
        semantics: Some(SemanticData {
            role: SemanticRole::Label,
            label: label.into(),
            ..SemanticData::default()
        }),
        interactive: false,
    }
}

fn column() -> Flex {
    Flex {
        axis: Axis::Vertical,
        ..Flex::default()
    }
}

/// Builds `groups` columns of `rows` described boxes under one root column.
///
/// Returns the tree, the group nodes, and a handle on the box in the middle of
/// the middle group: the deepest node reachable from the root through the
/// longest sibling lists, which is what makes the pruning bound worth stating.
fn grid(groups: usize, rows: usize) -> (UiTree, Vec<NodeId>, NodeHandle<BoxElement>) {
    let mut ui = UiTree::with_fonts(
        column(),
        LogicalSize::new(400.0, 300.0),
        FontDatabase::empty(),
    );
    let root = ui.root();
    let mut group_ids = Vec::new();
    let mut target = None;
    for group in 0..groups {
        let parent = ui.append(root, column());
        group_ids.push(parent.id());
        for row in 0..rows {
            let handle = ui.append(parent.id(), described(&format!("row {group}.{row}")));
            if group == groups / 2 && row == rows / 2 {
                target = Some(handle);
            }
        }
    }
    ui.update_passes();
    (ui, group_ids, target.expect("middle row"))
}

fn snapshot_labels(ui: &UiTree) -> Vec<String> {
    let mut labels = ui
        .semantic_snapshot()
        .into_iter()
        .map(|node| node.data.label)
        .collect::<Vec<_>>();
    labels.sort();
    labels
}

fn node(ui: &UiTree, label: &str) -> SemanticNode {
    ui.semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == label)
        .unwrap_or_else(|| panic!("no semantic node labelled {label}"))
}

#[test]
fn one_edited_label_in_ten_thousand_nodes_visits_its_path_and_its_siblings() {
    const GROUPS: usize = 100;
    const ROWS: usize = 100;

    let (mut ui, _groups, target) = grid(GROUPS, ROWS);
    let total = GROUPS * ROWS + GROUPS + 1;

    // The build is the control: every node is new, so the first pass has to
    // describe all of them and the counter can be trusted to notice a sweep.
    let built = ui.stats();
    assert_eq!(built.visited_accessibility_nodes, total);
    assert_eq!(built.accessibility_skipped_subtrees, 0);

    let mut semantics = ui.element(target).semantics.clone().expect("described");
    semantics.label = "edited".into();
    ui.box_mut(target).set_semantics(Some(semantics));
    let stats = ui.update_passes().stats;

    // Three nodes are described or walked through: the root, the edited box's
    // group, and the box. Everything else is refused at its subtree root, which
    // costs one pruned sibling per unrelated group and per unrelated row.
    assert_eq!(stats.visited_accessibility_nodes, 3);
    assert_eq!(stats.accessibility_skipped_subtrees, GROUPS - 1 + ROWS - 1);
    assert_eq!(stats.accessibility_nodes, 1, "one node reached the delta");
    assert!(
        stats.visited_accessibility_nodes + stats.accessibility_skipped_subtrees < total / 40,
        "the pass must cost the path plus its siblings, not the tree"
    );
    assert_eq!(node(&ui, "edited").data.role, SemanticRole::Label);
}

#[test]
fn a_settled_tree_walks_no_accessibility_nodes() {
    let (mut ui, _groups, _target) = grid(4, 4);
    let stats = ui.update_passes().stats;
    assert_eq!(stats.visited_accessibility_nodes, 0);
    assert_eq!(stats.accessibility_skipped_subtrees, 0);
    assert_eq!(stats.accessibility_nodes, 0);
}

#[test]
fn hiding_a_subtree_withdraws_its_semantics_without_touching_the_rest() {
    const GROUPS: usize = 8;
    const ROWS: usize = 8;

    let (mut ui, groups, _target) = grid(GROUPS, ROWS);
    let before = snapshot_labels(&ui);
    let hidden = groups[1];

    ui.set_visible(hidden, false);
    // One field write and one early-exiting invalidation: the subtree is not
    // walked to record the change, only to publish it.
    assert!(
        ui.stats().invalidate_steps <= 2,
        "hiding must not cost a walk of the subtree"
    );
    let stats = ui.update_passes().stats;

    assert_eq!(
        stats.accessibility_skipped_subtrees,
        GROUPS - 1,
        "the groups that still inherit what they inherited are refused whole"
    );
    assert_eq!(
        stats.visited_accessibility_nodes,
        ROWS + 2,
        "only the root, the hidden group, and its rows are walked"
    );
    let visible = snapshot_labels(&ui);
    assert_eq!(visible.len(), before.len() - ROWS);
    assert!(!visible.iter().any(|label| label.starts_with("row 1.")));

    ui.set_visible(hidden, true);
    ui.update_passes();
    assert_eq!(
        snapshot_labels(&ui),
        before,
        "showing the subtree again must republish exactly what it withdrew"
    );
}

#[test]
fn disabling_an_ancestor_reports_every_descendant_disabled() {
    let (mut ui, groups, _target) = grid(4, 4);
    assert!(node(&ui, "row 2.2").enabled);

    ui.set_enabled(groups[2], false);
    assert!(
        ui.stats().invalidate_steps <= 2,
        "disabling must not cost a walk of the subtree"
    );
    ui.update_passes();
    assert!(!node(&ui, "row 2.2").enabled);
    assert!(
        node(&ui, "row 3.3").enabled,
        "a sibling group is unaffected"
    );

    ui.set_enabled(groups[2], true);
    ui.update_passes();
    assert!(node(&ui, "row 2.2").enabled);
}

#[test]
fn a_moved_node_is_described_at_its_new_bounds() {
    let (mut ui, groups, _target) = grid(3, 3);
    let moved = node(&ui, "row 0.0");

    ui.reparent(moved.id, groups[2], 0);
    ui.update_passes();

    let described = node(&ui, "row 0.0");
    let follower = node(&ui, "row 2.0").bounds;
    assert_eq!(described.parent, Some(groups[2]));
    assert_ne!(described.bounds, moved.bounds);
    assert_eq!(
        described.bounds,
        LogicalRect::from_xywh(follower.origin.x, follower.origin.y - 4.0, 10.0, 4.0),
        "the moved box takes the leading slot of its new group"
    );
}

#[test]
fn focus_moves_report_only_the_two_controls_involved() {
    let mut ui = UiTree::with_fonts(
        column(),
        LogicalSize::new(400.0, 300.0),
        FontDatabase::empty(),
    );
    let root = ui.root();
    let group = ui.append(root, column()).id();
    for index in 0..8 {
        ui.append(group, described(&format!("row {index}")));
    }
    let first = ui.append(
        group,
        FocusableBox {
            label: "first",
            size: LogicalSize::new(40.0, 12.0),
        },
    );
    let second = ui.append(
        group,
        FocusableBox {
            label: "second",
            size: LogicalSize::new(40.0, 12.0),
        },
    );
    ui.update_passes();

    ui.set_focus(Some(first.id()));
    let stats = ui.update_passes().stats;
    assert_eq!(stats.accessibility_nodes, 1);
    assert!(node(&ui, "first").focused);

    ui.set_focus(Some(second.id()));
    let stats = ui.update_passes().stats;
    assert_eq!(
        stats.accessibility_nodes, 2,
        "the blurred and the focused control, and nothing between them"
    );
    assert_eq!(
        stats.visited_accessibility_nodes, 4,
        "the root, the group, and the two buttons"
    );
    assert!(node(&ui, "second").focused);
    assert!(!node(&ui, "first").focused);
}

/// A node reports its parent, and that is all a move between overlaid
/// containers changes.
///
/// [`Stack`] puts every child at its own origin, so moving a box between two
/// stacks that share a position leaves its offset, its size, its world
/// transform, and its bounds exactly as they were. Nothing the two parents'
/// relayout produces can therefore reach the moved node, and the parent it
/// reports to an assistive client would stay stale unless the move asks for the
/// node's own semantics directly.
#[test]
fn a_move_that_changes_no_geometry_still_republishes_the_reported_parent() {
    let mut ui = UiTree::with_fonts(
        Stack::default(),
        LogicalSize::new(200.0, 100.0),
        FontDatabase::empty(),
    );
    let root = ui.root();
    let first = ui.append(root, Stack::default()).id();
    let second = ui.append(root, Stack::default()).id();
    ui.append(first, described("carried"));
    ui.update_passes();
    let before = node(&ui, "carried");
    assert_eq!(before.parent, Some(first));

    ui.reparent(before.id, second, 0);
    ui.update_passes();

    let after = node(&ui, "carried");
    assert_eq!(after.bounds, before.bounds, "no geometry moved");
    assert_eq!(after.parent, Some(second));
}

/// Composed geometry is the one accessibility input that never arrives through
/// `invalidate`, so it has to reach the ancestors some other way.
///
/// Composition writes the bit on the node whose bounds moved, and the
/// accessibility pass prunes on the ancestors' bits. If composition did not
/// report the need back up the descent it had already made, the pass would
/// refuse the root, and a subtree that visibly moved would keep describing
/// itself at the rectangle it used to occupy.
#[test]
fn a_composed_geometry_change_still_reaches_the_accessibility_pass() {
    let mut ui = UiTree::with_fonts(
        column(),
        LogicalSize::new(400.0, 300.0),
        FontDatabase::empty(),
    );
    let root = ui.root();
    let shift = ui.append(root, Scroll::new(ScrollAxis::Both));
    ui.scroll_mut(shift)
        .set_content_extent(Some(LogicalSize::new(500.0, 500.0)));
    ui.append(shift.id(), described("carried"));
    ui.update_passes();
    let before = node(&ui, "carried").bounds;

    ui.scroll_mut(shift)
        .set_offset(LogicalPoint::new(17.0, 23.0));
    ui.update_passes();

    assert_eq!(
        node(&ui, "carried").bounds,
        LogicalRect::from_xywh(
            before.origin.x - 17.0,
            before.origin.y - 23.0,
            before.size.width,
            before.size.height,
        ),
        "a node nothing asked for accessibility work on still reports where it \
         actually is"
    );
}
