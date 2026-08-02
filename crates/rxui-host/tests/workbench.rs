//! The native workbench model driven through the entity harness.

use astrelis_core::geometry::{LogicalPoint, LogicalSize};
use rxui_core::{EntityHarness, ViewStats};
use rxui_host::workbench::{VIEWPORT_HEIGHT, VIEWPORT_WIDTH, Workbench};
use rxui_tree::{PassStats, SemanticAction};

fn mount() -> EntityHarness<Workbench> {
    EntityHarness::with_viewport(
        LogicalSize::new(VIEWPORT_WIDTH, VIEWPORT_HEIGHT),
        |context| context.new(|_| Workbench::new()),
    )
}

#[test]
fn workbench_mounts_toolbar_split_list_fields_and_no_settings_surface() {
    let harness = mount();
    for label in [
        "Save workspace",
        "New document",
        "Settings",
        "Files",
        "Editor",
        "Document title",
        "Document body",
    ] {
        assert!(harness.try_find(label).is_some(), "missing {label}");
    }
    assert!(harness.try_find("Workspace settings").is_none());
    assert_eq!(harness.root().read(harness.app()).documents.len(), 3);
}

#[test]
fn toolbar_commands_are_buttons_in_declaration_order() {
    let harness = mount();
    let save = harness.find("Save workspace");
    let new = harness.find("New document");
    let settings = harness.find("Settings");
    assert!(save.bounds.origin.x < new.bounds.origin.x);
    assert!(new.bounds.origin.x < settings.bounds.origin.x);
}

#[test]
fn split_panes_are_side_by_side_at_the_model_ratio() {
    let harness = mount();
    let files = harness.find("Files").bounds;
    let editor = harness.find("Editor").bounds;
    assert!(files.origin.x < editor.origin.x);
    assert!(editor.origin.x > VIEWPORT_WIDTH * 0.2);
    assert!(editor.origin.x < VIEWPORT_WIDTH * 0.5);
}

#[test]
fn selecting_a_list_row_updates_the_shared_editor_model() {
    let mut harness = mount();
    harness.activate("○ Notes.txt");
    let state = harness.root().read(harness.app());
    assert_eq!(state.selected, 2);
    assert_eq!(state.status, "Selected document 2");
    drop(state);
    assert!(harness.try_find("● Notes.txt").is_some());
}

#[test]
fn text_fields_edit_the_selected_document_and_workspace() {
    let mut harness = mount();
    harness.semantic_action(
        "Document body",
        SemanticAction::SetText("Edited through EntityHarness".into()),
    );
    assert_eq!(
        harness.root().read(harness.app()).documents[0].body,
        "Edited through EntityHarness"
    );

    harness.activate("Settings");
    harness.semantic_action(
        "Workspace name",
        SemanticAction::SetText("Renamed workspace".into()),
    );
    assert_eq!(
        harness.root().read(harness.app()).workspace_name,
        "Renamed workspace"
    );
}

#[test]
fn settings_surface_disables_toolbar_and_close_reenables_it() {
    let mut harness = mount();
    harness.activate("Settings");
    assert!(harness.try_find("Workspace settings").is_some());
    for label in ["Save workspace", "New document", "Settings"] {
        assert!(!harness.find(label).enabled, "{label} stayed enabled");
    }

    harness.activate("Close settings");
    assert!(harness.try_find("Workspace settings").is_none());
    assert!(harness.find("Settings").enabled);
}

#[test]
fn saving_raises_a_dismissible_status_surface() {
    let mut harness = mount();
    harness.activate("Save workspace");
    assert!(harness.try_find("Status: Workspace saved").is_some());
    harness.activate("Dismiss status");
    assert!(harness.try_find("Status: Workspace saved").is_none());
    assert!(harness.try_find("Status: Ready").is_some());
}

#[test]
fn deleting_a_document_keeps_other_keyed_rows_retained() {
    let mut harness = mount();
    let survivor = harness.node_id("○ Notes.txt");
    harness.activate("Delete selected");
    assert_eq!(harness.node_id("● Notes.txt"), survivor);
    assert!(harness.try_find("● Welcome.md").is_none());
}

#[test]
fn splitter_input_changes_the_persistent_ratio() {
    let mut harness = mount();
    let editor = harness.find("Editor").bounds;
    let divider = LogicalPoint::new(editor.origin.x - 3.0, VIEWPORT_HEIGHT * 0.5);
    harness.press_pointer_at(divider);
    harness.hover_at(LogicalPoint::new(
        VIEWPORT_WIDTH * 0.6,
        VIEWPORT_HEIGHT * 0.5,
    ));
    harness.release_pointer_at(LogicalPoint::new(
        VIEWPORT_WIDTH * 0.6,
        VIEWPORT_HEIGHT * 0.5,
    ));
    assert!(harness.root().read(harness.app()).split > 0.5);
}

#[test]
fn refreshing_the_workbench_rebuilds_views_but_no_retained_passes() {
    let mut harness = mount();
    harness.refresh();
    let stats = harness.stats();
    assert_eq!(
        stats.passes,
        PassStats {
            reused_fragments: 26,
            ..PassStats::default()
        }
    );
    assert_eq!(
        stats.views,
        ViewStats {
            component_views: 1,
            nodes_rebuilt: 19,
            containers_reconciled: 6,
            ..ViewStats::default()
        }
    );
}
