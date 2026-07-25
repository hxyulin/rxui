//! Retained-tree inspection capture.
//!
//! `InspectionSnapshot` is the data half of RXUI's devtools: a flattened,
//! `NodeId`-addressed view of the retained tree plus the counters from the last
//! pass. Anything it drops is invisible to the inspector, so these tests pin
//! what it must carry and how faithfully it must track the live tree.

use astrelis_core::geometry::LogicalSize;
use rxui::core::SemanticRole;
use rxui::{
    ButtonVariant, Component, ComponentContext, DialogAction, InspectionSnapshot, Theme, View,
    column, dialog, label, text_field,
};
use rxui_test_support::Harness;

const VIEWPORT: LogicalSize = LogicalSize::new(480.0, 320.0);

#[derive(Clone, Debug)]
enum Edit {
    Rename(String),
    Dismiss,
    Confirm,
}

struct Inspected {
    name: String,
    modal: bool,
}

impl Inspected {
    fn new(modal: bool) -> Self {
        Self {
            name: "Astrelis".into(),
            modal,
        }
    }
}

impl Component for Inspected {
    type Action = Edit;
    type Effect = ();

    fn update(&mut self, action: Edit, _context: &mut ComponentContext<'_, ()>) {
        match action {
            Edit::Rename(name) => self.name = name,
            Edit::Dismiss | Edit::Confirm => self.modal = false,
        }
    }

    fn view(&self, _theme: &Theme) -> View<Edit> {
        dialog(
            self.modal,
            "Rename",
            column((
                label("Project"),
                text_field("Name", self.name.clone(), Edit::Rename),
            )),
            label("Pick a new name."),
            Edit::Dismiss,
            &[DialogAction {
                id: "confirm".into(),
                label: "Confirm".into(),
                action: Edit::Confirm,
                variant: ButtonVariant::Primary,
            }],
        )
    }
}

fn capture(harness: &Harness<Inspected>) -> InspectionSnapshot {
    harness.with_ui(InspectionSnapshot::capture)
}

#[test]
fn a_capture_carries_one_row_per_published_semantic_node() {
    let harness = Harness::new(Inspected::new(false), VIEWPORT).expect("the tree mounts");
    let snapshot = capture(&harness);
    assert_eq!(snapshot.nodes.len(), harness.semantics().len());
    // Retained order, not sorted order: the inspector's row list has to read
    // like the tree it inspects.
    let captured = snapshot
        .nodes
        .iter()
        .map(|node| node.id)
        .collect::<Vec<_>>();
    let published = harness
        .semantics()
        .iter()
        .map(|node| node.id)
        .collect::<Vec<_>>();
    assert_eq!(captured, published);
}

#[test]
fn a_capture_preserves_every_field_an_inspector_shows() {
    let harness = Harness::new(Inspected::new(false), VIEWPORT).expect("the tree mounts");
    let snapshot = capture(&harness);
    let field = harness.find("Name");
    let row = snapshot
        .nodes
        .iter()
        .find(|node| node.id == field.id)
        .expect("the captured tree contains the text field");
    assert_eq!(row.role, SemanticRole::TextField);
    assert_eq!(row.label, "Name");
    assert_eq!(row.value.as_deref(), Some("Astrelis"));
    assert_eq!(row.bounds, field.bounds);
    assert_eq!(row.enabled, field.enabled);
    assert_eq!(row.focused, field.focused);
}

#[test]
fn a_capture_reports_the_counters_from_the_pass_that_produced_the_tree() {
    let mut harness = Harness::new(Inspected::new(false), VIEWPORT).expect("the tree mounts");
    harness.refresh();
    // A refresh over an unchanged tree settles with no work, and the capture has
    // to say so rather than replaying the mount's counters. `stats` is the only
    // part of the snapshot that is not derived from the semantic tree, so a
    // stale read here would be invisible everywhere else.
    let snapshot = capture(&harness);
    assert_eq!(snapshot.stats, harness.stats());
    assert_eq!(snapshot.stats.layout_elements, 0);
    assert_eq!(snapshot.stats.rebuilt_fragments, 0);
}

#[test]
fn a_capture_tracks_focus_and_enablement_as_they_change() {
    let mut harness = Harness::new(Inspected::new(false), VIEWPORT).expect("the tree mounts");
    let field = harness.find("Name").id;
    assert!(
        !capture(&harness).nodes.iter().any(|node| node.focused),
        "nothing is focused before the tree is driven",
    );

    harness.focus_first();
    let focused = capture(&harness)
        .nodes
        .into_iter()
        .filter(|node| node.focused)
        .map(|node| node.id)
        .collect::<Vec<_>>();
    assert_eq!(focused, vec![field], "the text field is the only focusable");

    // Opening the modal disables the whole background subtree, which the
    // capture must report as *effective* enablement, not as the flag the view
    // set on any one node.
    harness.dispatch(Edit::Confirm);
    harness.mutate(|state| state.modal = true);
    let row = capture(&harness)
        .nodes
        .into_iter()
        .find(|node| node.id == field)
        .expect("the disabled field is still published");
    assert!(!row.enabled);
}

#[test]
fn two_captures_of_an_unchanged_tree_are_equal() {
    let harness = Harness::new(Inspected::new(true), VIEWPORT).expect("the tree mounts");
    // `InspectionSnapshot` derives `PartialEq` so devtools can diff frames.
    // That is only meaningful if capture is deterministic, which is why nothing
    // in it may be ordered by a hash map.
    assert_eq!(capture(&harness), capture(&harness));
}

#[test]
fn a_capture_of_a_hidden_subtree_omits_it() {
    let mut harness = Harness::new(Inspected::new(true), VIEWPORT).expect("the tree mounts");
    assert!(
        capture(&harness)
            .nodes
            .iter()
            .any(|node| node.label == "Confirm"),
    );

    harness.dispatch(Edit::Dismiss);
    // The dialog is hidden with `.visible(false)`, which keeps the nodes mounted
    // but drops them from semantics. An inspector that showed them would be
    // describing a tree no user can reach.
    assert!(
        !capture(&harness)
            .nodes
            .iter()
            .any(|node| node.label == "Confirm"),
    );
}

#[test]
fn an_inspector_row_exists_for_every_captured_node() {
    let harness = Harness::new(Inspected::new(false), VIEWPORT).expect("the tree mounts");
    let snapshot = capture(&harness);
    let inspector = InspectorPane { snapshot };
    let expected = inspector.snapshot.nodes.len();
    let harness =
        Harness::new(inspector, LogicalSize::new(640.0, 640.0)).expect("the inspector pane mounts");
    let rows = harness
        .semantics()
        .iter()
        .filter(|node| node.data.role == SemanticRole::Button)
        .count();
    // One button per captured node, and none dropped: `inspection_view` is the
    // only place the captured ids become clickable, so a row it skips is a node
    // the inspector cannot select.
    assert_eq!(rows, expected);
}

struct InspectorPane {
    snapshot: InspectionSnapshot,
}

impl Component for InspectorPane {
    type Action = rxui::core::NodeId;
    type Effect = ();

    fn update(&mut self, _action: Self::Action, _context: &mut ComponentContext<'_, ()>) {}

    fn view(&self, _theme: &Theme) -> View<Self::Action> {
        rxui::inspection_view(&self.snapshot, None, |id| id)
    }
}
