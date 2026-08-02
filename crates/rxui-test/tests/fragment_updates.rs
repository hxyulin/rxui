//! Direct scene-emission and exact incremental-fragment profiles.

use astrelis_core::geometry::LogicalSize;
use astrelis_text::FontDatabase;
use rxui_tree::{Axis, Flex, Label, PassStats, UiTree};

fn fragment_texts(ui: &UiTree) -> Vec<Vec<&str>> {
    ui.scene()
        .fragments()
        .iter()
        .map(|fragment| {
            fragment
                .list
                .texts()
                .iter()
                .map(|layout| layout.text())
                .collect()
        })
        .collect()
}

#[test]
fn changing_one_label_rebuilds_exactly_its_scene_fragment() {
    let mut ui = UiTree::with_fonts(
        Flex {
            axis: Axis::Horizontal,
            gap: 4.0,
            ..Flex::default()
        },
        LogicalSize::new(200.0, 40.0),
        FontDatabase::empty(),
    );
    let root = ui.root();
    let changed = ui.append(root, Label::new("Before"));
    let stable = ui.append(root, Label::new("Stable"));

    ui.update_passes();
    assert_eq!(
        fragment_texts(&ui),
        vec![vec![], vec!["Before"], vec!["Stable"]]
    );

    assert!(ui.label_mut(changed).set_text("After"));
    let stats = ui.update_passes().stats;

    assert_eq!(stats.rebuilt_fragments, 1);
    assert_eq!(
        stats,
        PassStats {
            layout_elements: 2,
            composed_nodes: 0,
            rebuilt_fragments: 1,
            reused_fragments: 2,
            hit_test_nodes: 0,
            accessibility_nodes: 1,
            shaped_text: 1,
            visited_compose_nodes: 2,
            compose_skipped_subtrees: 1,
            visited_accessibility_nodes: 2,
            accessibility_skipped_subtrees: 1,
            invalidate_steps: 1,
        }
    );
    assert_eq!(ui.scene().fragments().len(), 3);
    assert!(ui.scene().rebuilt(changed.id()));
    assert!(!ui.scene().rebuilt(stable.id()));
    assert!(!ui.scene().rebuilt(root));
    assert_eq!(
        fragment_texts(&ui),
        vec![vec![], vec!["After"], vec!["Stable"]]
    );
}
