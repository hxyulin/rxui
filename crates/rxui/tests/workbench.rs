//! The `native_workbench` example, driven headlessly.
//!
//! The example is the only consumer of a broad slice of RXUI's public surface -
//! docking, charts, the node graph, the toolbar, the command palette, the modal
//! dialog, and toasts - and until now nothing compiled it except a GPU build.
//!
//! The component under test is the example's own `model.rs`, included here with
//! `#[path]`. An example is a separate crate root, so its modules cannot be
//! imported; a second compilation of the same file is the only way to share it
//! without moving the component into a library. That costs one extra build of
//! the module and forbids it from naming anything outside `rxui` and
//! `astrelis-core`, both of which are cheap next to letting 23 public items go
//! untested on every platform without a GPU.

use astrelis_core::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use astrelis_platform::{CursorIcon, NamedKey};
use rxui::core::{SemanticNode, SemanticRole};
use rxui_test_support::Harness;

#[path = "../examples/native_workbench/model.rs"]
mod model;

use model::{Action, CHART_PANE, GRAPH_PANE, ROOT_SPLIT, Workbench};

/// The logical size the native example asks for, so the headless geometry
/// matches what a developer sees when running the binary.
const VIEWPORT: LogicalSize = LogicalSize::new(1100.0, 720.0);

fn mount() -> Harness<Workbench> {
    Harness::new(Workbench::new(), VIEWPORT).expect("the workbench mounts")
}

fn centre(bounds: LogicalRect) -> LogicalPoint {
    LogicalPoint::new(
        bounds.origin.x + bounds.size.width * 0.5,
        bounds.origin.y + bounds.size.height * 0.5,
    )
}

/// Locates the single node with `role`.
///
/// The chart's tab button and the chart surface itself both publish the label
/// "Chart", so the specialized surfaces have to be addressed by role.
#[track_caller]
fn by_role(harness: &Harness<Workbench>, role: SemanticRole) -> SemanticNode {
    let mut matches = harness
        .semantics()
        .into_iter()
        .filter(|node| node.data.role == role);
    let found = matches
        .next()
        .unwrap_or_else(|| panic!("no semantic node with role {role:?}"));
    assert!(
        matches.next().is_none(),
        "more than one node with role {role:?}",
    );
    found
}

/// Locates the enabled node named `label`.
///
/// "Save" names both a toolbar command and a dialog action. Whichever surface
/// currently owns the interaction is the enabled one, because a dialog disables
/// its background.
#[track_caller]
fn enabled(harness: &Harness<Workbench>, label: &str) -> SemanticNode {
    let mut matches = harness
        .semantics()
        .into_iter()
        .filter(|node| node.data.label == label && node.enabled);
    let found = matches
        .next()
        .unwrap_or_else(|| panic!("no enabled semantic node labeled {label:?}"));
    assert!(
        matches.next().is_none(),
        "more than one enabled node labeled {label:?}",
    );
    found
}

#[test]
fn the_workbench_mounts_its_toolbar_both_docked_panes_and_no_overlays() {
    let harness = mount();
    for command in ["Save", "Commands", "Settings"] {
        assert!(
            harness.try_find(command).is_some(),
            "the toolbar publishes {command}",
        );
    }
    // Both dock groups are realized, not just the active one: the split shows
    // two tab groups side by side.
    assert_eq!(
        by_role(&harness, SemanticRole::Chart).data.value.as_deref(),
        Some("2 series"),
    );
    assert_eq!(
        by_role(&harness, SemanticRole::Graph).data.value.as_deref(),
        Some("3 nodes, 2 edges"),
    );

    // Neither overlay is open on mount, so neither publishes anything.
    assert!(harness.try_find("Workspace settings").is_none());
    assert!(harness.try_find("Command search").is_none());
    assert!(harness.try_find("Workspace saved").is_none());
}

#[test]
fn the_two_dock_panes_sit_side_by_side_at_the_declared_ratio() {
    let harness = mount();
    let chart = by_role(&harness, SemanticRole::Chart).bounds;
    let graph = by_role(&harness, SemanticRole::Graph).bounds;
    assert!(
        chart.origin.x + chart.size.width <= graph.origin.x,
        "a horizontal split must not overlap its panes",
    );
    // The root split is declared at 0.5 in a 1100 wide viewport, so the divider
    // sits near the midpoint and neither pane may claim the whole width.
    assert!(chart.origin.x < VIEWPORT.width * 0.5);
    assert!(graph.origin.x > VIEWPORT.width * 0.4);
}

#[test]
fn a_toolbar_command_opens_the_settings_dialog_and_disables_the_workbench() {
    let mut harness = mount();
    harness.activate("Settings");

    assert!(harness.component().dialog_open);
    assert!(harness.try_find("Workspace settings").is_some());
    assert!(harness.try_find("Interaction mode").is_some());
    // The dialog disables its background, so every toolbar command goes dead.
    for command in ["Save", "Commands", "Settings"] {
        assert!(
            !harness.find(command).enabled,
            "{command} must be disabled behind the modal",
        );
    }
    // The modal claims focus on open, and the first focusable inside it is the
    // radio group in the dialog's *content*, which is declared before the
    // action row.
    assert!(harness.find("● Edit").focused);
}

#[test]
fn choosing_an_interaction_mode_moves_the_dialog_radio_marker() {
    let mut harness = mount();
    harness.activate("Settings");
    assert!(harness.try_find("● Edit").is_some());

    harness.activate("○ Inspect");
    assert_eq!(harness.component().mode, "Inspect");
    assert!(harness.try_find("● Inspect").is_some());
    assert!(harness.try_find("○ Edit").is_some());
}

#[test]
fn escape_dismisses_the_settings_dialog_and_re_enables_the_workbench() {
    let mut harness = mount();
    harness.activate("Settings");
    harness.press(NamedKey::Escape);

    assert!(!harness.component().dialog_open);
    assert!(harness.try_find("Workspace settings").is_none());
    assert!(harness.find("Settings").enabled);
}

#[test]
fn saving_from_the_dialog_closes_it_and_raises_a_dismissible_toast() {
    let mut harness = mount();
    harness.activate("Settings");
    // The toolbar's Save is disabled behind the modal, so the enabled one is
    // the dialog's confirming action.
    let save = enabled(&harness, "Save");
    harness.click_at(centre(save.bounds));

    assert!(!harness.component().dialog_open);
    assert!(harness.try_find("Workspace saved").is_some());

    harness.activate("Dismiss");
    assert!(harness.component().toast.is_none());
    assert!(harness.try_find("Workspace saved").is_none());
}

#[test]
fn the_command_palette_opens_from_the_toolbar_and_lists_its_commands() {
    let mut harness = mount();
    harness.activate("Commands");

    assert!(harness.component().palette_open);
    assert!(harness.try_find("Command search").is_some());
    // The palette marks its selected row with "› " and pads the rest with two
    // spaces, so the selection is readable straight off the accessible name.
    assert!(harness.try_find("› Save workspace").is_some());
    assert!(harness.try_find("  Open settings").is_some());
}

#[test]
fn arrow_keys_move_the_palette_selection_and_enter_invokes_it() {
    let mut harness = mount();
    harness.activate("Commands");
    harness.press(NamedKey::Other("ArrowDown".into()));

    assert_eq!(harness.component().selected_command, 1);
    assert!(harness.try_find("› Open settings").is_some());

    harness.press(NamedKey::Enter);
    // Row 1 is "Open settings", so submitting it opens the settings dialog and
    // closes the palette.
    assert!(harness.component().dialog_open);
    assert!(!harness.component().palette_open);
}

#[test]
fn typing_in_the_palette_narrows_it_to_the_matching_command() {
    let mut harness = mount();
    harness.activate("Commands");
    let search = harness.bounds("Command search");
    harness.click_at(centre(search));
    harness.type_text("settings");

    assert_eq!(harness.component().query, "settings");
    assert!(harness.try_find("› Open settings").is_some());
    assert!(harness.try_find("  Save workspace").is_none());
}

#[test]
fn escape_dismisses_the_command_palette() {
    let mut harness = mount();
    harness.activate("Commands");
    harness.press(NamedKey::Escape);

    assert!(!harness.component().palette_open);
    assert!(harness.try_find("Command search").is_none());
    assert!(harness.find("Commands").enabled);
}

#[test]
fn dragging_the_root_splitter_widens_the_chart_pane() {
    let mut harness = mount();
    let before = by_role(&harness, SemanticRole::Chart).bounds.size.width;
    let graph_tab = harness.bounds("Graph");
    // The divider is the gap immediately left of the second group's tab strip;
    // `SplitPane` makes it 6 logical units wide, so its centre is 3 units in.
    let divider = LogicalPoint::new(graph_tab.origin.x - 3.0, VIEWPORT.height * 0.5);

    harness.hover_at(divider);
    assert_eq!(
        harness.cursor_icon(),
        CursorIcon::EwResize,
        "the divider has to advertise itself before it can be dragged",
    );

    harness.press_pointer_at(divider);
    harness.hover_at(LogicalPoint::new(
        VIEWPORT.width * 0.75,
        VIEWPORT.height * 0.5,
    ));
    harness.release_pointer_at(LogicalPoint::new(
        VIEWPORT.width * 0.75,
        VIEWPORT.height * 0.5,
    ));

    let after = by_role(&harness, SemanticRole::Chart).bounds.size.width;
    assert!(
        after > before,
        "dragging the divider right widened the chart pane from {before} to {after}",
    );
}

#[test]
fn resizing_the_root_split_reshapes_no_graph_titles() {
    let mut harness = mount();
    harness.dispatch(Action::Resize(ROOT_SPLIT, 700.0));
    // The graph's own configuration did not move here; its *parent* did, so the
    // pane is re-measured from the outside and `NodeGraphElement::layout` runs
    // whatever the spec reported. Nothing a spec can say about invalidation
    // helps with that: only the per-node shaping memo keeps a splitter drag -
    // one frame per pointer move - off the shaper.
    assert_eq!(harness.stats().shaped_text, 0);
    // And the drag really did re-measure, so the zero above is not a frame that
    // never happened.
    assert!(harness.stats().layout_elements > 0);
}

#[test]
fn a_resize_action_clamps_the_root_split_into_a_usable_range() {
    let mut harness = mount();
    harness.dispatch(Action::Resize(ROOT_SPLIT, 12.0));
    let chart = by_role(&harness, SemanticRole::Chart).bounds;
    let graph = by_role(&harness, SemanticRole::Graph).bounds;
    // `DockNode::set_ratio` clamps to 0.95, so the second pane keeps roughly 5%
    // of the width instead of collapsing to nothing.
    assert!(graph.size.width > 0.0, "the trailing pane stays visible");
    assert!(chart.size.width > graph.size.width);
}

/// The bottom-left corner of the chart's inset plot box.
///
/// Both series' first point is the domain origin `(0, 0)`, and the chart maps
/// the domain origin to the bottom-left of a box inset by 12 logical units, or
/// by 10% of the smaller side when the surface is small.
fn chart_domain_origin(bounds: LogicalRect) -> LogicalPoint {
    let inset = 12.0f32
        .min(bounds.size.width * 0.1)
        .min(bounds.size.height * 0.1);
    LogicalPoint::new(
        bounds.origin.x + inset,
        bounds.origin.y + bounds.size.height - inset,
    )
}

#[test]
fn dragging_from_inside_a_docked_pane_moves_neither_the_divider_nor_the_selection() {
    let mut harness = mount();
    let bounds = by_role(&harness, SemanticRole::Chart).bounds;
    let before = bounds.size.width;
    let inside = LogicalPoint::new(bounds.origin.x + 12.0, bounds.origin.y + 300.0);

    harness.press_pointer_at(inside);
    harness.hover_at(LogicalPoint::new(inside.x + 200.0, inside.y));
    harness.release_pointer_at(LogicalPoint::new(inside.x + 200.0, inside.y));

    // A press 12 units inside the chart, nowhere near the divider at x = 547,
    // used to grab the divider and drag it the full 200 units, because a press a
    // child declines bubbles to the split and the split accepted every one.
    let after = by_role(&harness, SemanticRole::Chart).bounds.size.width;
    assert_eq!(after, before);
    // Nor does the chart mistake a drag that ended 200 units away for a click on
    // where it started: the release is what selects, and it landed elsewhere.
    assert_eq!(harness.component().selected_series, None);
}

#[test]
fn the_chart_reports_the_nearest_point_and_empty_space_clears_it() {
    let mut harness = mount();
    let bounds = by_role(&harness, SemanticRole::Chart).bounds;
    // A full click, which is what the divider fix bought: the chart handles the
    // release and not the press, so before it the enclosing split captured the
    // gesture on press and the release never arrived here at all.
    harness.click_at(chart_domain_origin(bounds));
    // Series 1 is declared first, so it wins the tie against series 2's own
    // point at the same coordinate.
    assert_eq!(harness.component().selected_series, Some((1, 0)));

    // Just inside the top-left corner: the highest-valued point sits far to the
    // right and the leftmost points sit far below, so nothing is within the
    // element's 12 unit selection radius.
    harness.click_at(LogicalPoint::new(
        bounds.origin.x + 14.0,
        bounds.origin.y + 14.0,
    ));
    assert_eq!(harness.component().selected_series, None);
}

#[test]
fn the_node_graph_reports_the_node_under_the_pointer_and_empty_canvas_clears_it() {
    let mut harness = mount();
    let bounds = by_role(&harness, SemanticRole::Graph).bounds;
    // Node 1 covers canvas (30, 80) to (150, 140) at the default identity
    // viewport, so (90, 110) is its centre.
    harness.click_at(LogicalPoint::new(
        bounds.origin.x + 90.0,
        bounds.origin.y + 110.0,
    ));
    assert_eq!(harness.component().selected_node, Some(1));

    // Canvas (200, 40) falls between all three node rectangles.
    harness.click_at(LogicalPoint::new(
        bounds.origin.x + 200.0,
        bounds.origin.y + 40.0,
    ));
    assert_eq!(harness.component().selected_node, None);
}

#[test]
fn selecting_a_graph_node_reshapes_no_node_title() {
    let mut harness = mount();
    let bounds = by_role(&harness, SemanticRole::Graph).bounds;
    harness.release_pointer_at(LogicalPoint::new(
        bounds.origin.x + 90.0,
        bounds.origin.y + 110.0,
    ));
    assert_eq!(harness.component().selected_node, Some(1));
    // `NodeGraphSpec::changed` reports the new selection as `Invalidation::ALL`,
    // so the graph is still dragged back through layout for what is only a fill
    // colour - but the titles it finds there are the ones it already shaped.
    assert_eq!(harness.stats().shaped_text, 0);
}

#[test]
fn selecting_a_dock_tab_keeps_its_pane_retained() {
    let mut harness = mount();
    let graph = by_role(&harness, SemanticRole::Graph).id;
    harness.dispatch(Action::SelectPane(GRAPH_PANE));
    // Re-selecting the already active pane of a single-pane group is a no-op for
    // the layout, and must therefore not tear the graph surface down: a rebuilt
    // node graph would lose its hover and drag state.
    assert_eq!(by_role(&harness, SemanticRole::Graph).id, graph);

    harness.dispatch(Action::SelectPane(CHART_PANE));
    assert_eq!(by_role(&harness, SemanticRole::Graph).id, graph);
}

#[test]
fn refreshing_the_whole_workbench_reshapes_no_text() {
    let mut harness = mount();
    harness.refresh();
    // The workbench is the largest tree in the repo's tests; if a whole-tree
    // rebuild of it reshaped text, every keystroke in a real editor would too.
    assert_eq!(harness.stats().shaped_text, 0);
}
