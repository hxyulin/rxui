//! Incremental retained runtime behavior.
//!
//! Stage 4: dropped pointer_capture_finishes_a_press_released_outside,
//! hover_transitions_repaint_only_the_entered_and_exited_controls,
//! hovered_and_captured_elements_select_native_cursors,
//! keyboard_events_bubble_to_overlay_boundaries,
//! text_fields_release_command_navigation_keys_to_their_owner,
//! split_pane_routes_drag_outside_the_divider_and_reflows_children,
//! clicking_a_splitter_does_not_move_it,
//! a_press_bubbling_through_a_split_pane_neither_resizes_it_nor_steals_the_release,
//! shaped_text_field_routes_focus_editing_and_incremental_repaint,
//! semantic_actions_focus_and_activate_control_values,
//! tab_focus_traversal_and_keyboard_activation_follow_tree_order, and
//! icon_only_button_keeps_accessible_label_and_compact_geometry.

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalSize},
};
use rxui_tree::{
    Align, Alignment, Axis, BoxElement, Constraints, Element, EventResult, Flex, Frame, Label,
    LayoutContext, Scroll, ScrollAxis, SemanticData, SemanticRole, Stack, UiInput, UiTree,
};

#[derive(Clone, Debug, PartialEq)]
enum Action {
    Activate,
}

struct TestControl {
    label: &'static str,
    size: LogicalSize,
}

impl Element for TestControl {
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
    fn event(&mut self, input: UiInput) -> EventResult {
        match input {
            UiInput::PointerPressed(_) => EventResult {
                handled: true,
                ..EventResult::default()
            },
            UiInput::PointerReleased(_) | UiInput::Paste(_) => {
                EventResult::action(Action::Activate)
            }
            _ => EventResult::default(),
        }
    }
    fn hit_testable(&self) -> bool {
        true
    }
    fn focusable(&self) -> bool {
        true
    }
}

#[test]
fn paint_only_update_rebuilds_one_fragment_and_skips_layout() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    let child = ui.append(
        ui.root(),
        BoxElement::new(LogicalSize::new(40.0, 20.0), Color::WHITE),
    );
    ui.update_passes();

    ui.box_mut(child).set_color(Color::BLACK);
    let update = ui.update_passes();
    assert_eq!(update.stats.layout_elements, 0);
    assert_eq!(update.stats.rebuilt_fragments, 1);
    assert!(update.scene.rebuilt(child.id()));
}

#[test]
fn unchanged_update_does_no_retained_work() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    ui.append(ui.root(), Label::new("unchanged"));
    ui.update_passes();
    let update = ui.update_passes();
    assert_eq!(update.stats.layout_elements, 0);
    assert_eq!(update.stats.rebuilt_fragments, 0);
    assert_eq!(update.accessibility.changed.len(), 0);
}

#[test]
fn keyed_reorder_primitive_preserves_identity_and_cached_fragments() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Horizontal,
            ..Flex::default()
        },
        LogicalSize::new(400.0, 300.0),
    );
    let root = ui.root();
    let a = ui.append(root, Label::new("A"));
    let b = ui.append(root, Label::new("B"));
    ui.update_passes();
    let x = |ui: &UiTree, label: &str| {
        ui.semantic_snapshot()
            .into_iter()
            .find(|node| node.data.label == label)
            .unwrap()
            .bounds
            .origin
            .x
    };
    assert!(x(&ui, "A") < x(&ui, "B"));

    ui.set_children(root, &[b.id(), a.id()]);
    let stats = ui.update_passes().stats;
    assert!(ui.contains(a.id()) && ui.contains(b.id()));
    assert_eq!(
        stats.rebuilt_fragments, 1,
        "only the changed parent repaints"
    );
    // Identity survives *and* the order is the one that was asked for. Asserting
    // only identity is what an order-blind comparison does, and it passes just as
    // well against a reorder that silently did nothing, which is the whole
    // failure mode worth guarding here: the permutation is in place, so the
    // children keep their nodes, their fragments, and their measured sizes, and
    // the only observable that moved is where the parent puts them.
    assert!(
        x(&ui, "B") < x(&ui, "A"),
        "B was asked to come first: A={} B={}",
        x(&ui, "A"),
        x(&ui, "B"),
    );
}

#[test]
fn stack_overlays_children_and_targets_the_topmost_control() {
    let mut ui = UiTree::new(Stack::default(), LogicalSize::new(200.0, 100.0));
    let first = ui.append(
        ui.root(),
        TestControl {
            label: "First",
            size: LogicalSize::new(100.0, 30.0),
        },
    );
    let second = ui.append(
        ui.root(),
        TestControl {
            label: "Second",
            size: LogicalSize::new(100.0, 30.0),
        },
    );
    ui.update_passes();

    assert_eq!(ui.hit_test(LogicalPoint::new(5.0, 5.0)), Some(second.id()));
    assert!(ui.contains(first.id()));
}

#[test]
fn wheel_input_bubbles_to_a_clipped_scroll_ancestor() {
    let mut ui = UiTree::new(
        Scroll::new(ScrollAxis::Vertical),
        LogicalSize::new(200.0, 50.0),
    );
    let content = ui.append(
        ui.root(),
        Flex {
            axis: Axis::Vertical,
            gap: 4.0,
            ..Flex::default()
        },
    );
    for index in 0..8 {
        ui.append(content.id(), Label::new(format!("Row {index}")));
    }
    ui.update_passes();
    let before = ui
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Row 0")
        .unwrap()
        .bounds
        .origin
        .y;

    ui.dispatch(UiInput::PointerWheel {
        position: LogicalPoint::new(10.0, 10.0),
        delta: LogicalPoint::new(0.0, 30.0),
    });
    ui.update_passes();
    let after = ui
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Row 0")
        .unwrap()
        .bounds
        .origin
        .y;
    assert_eq!(after, before - 30.0);
}

#[test]
fn hit_testing_prunes_subtrees_and_dispatches_typed_actions() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    let button = ui.append(
        ui.root(),
        TestControl {
            label: "Run",
            size: LogicalSize::new(100.0, 30.0),
        },
    );
    ui.update_passes();
    let point = LogicalPoint::new(10.0, 10.0);
    assert_eq!(ui.hit_test(point), Some(button.id()));
    ui.dispatch(UiInput::PointerPressed(point));
    let action = ui.dispatch(UiInput::PointerReleased(point)).unwrap();
    assert_eq!(*action.downcast::<Action>().unwrap(), Action::Activate);
}

#[test]
fn flex_growth_allocates_exact_bounded_shares() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Horizontal,
            ..Flex::default()
        },
        LogicalSize::new(300.0, 100.0),
    );
    let first = ui.append(
        ui.root(),
        Frame {
            grow: 1.0,
            ..Frame::default()
        },
    );
    ui.append(first.id(), Label::new("First"));
    let second = ui.append(
        ui.root(),
        Frame {
            grow: 2.0,
            ..Frame::default()
        },
    );
    ui.append(second.id(), Label::new("Second"));
    ui.update_passes();

    let semantics = ui.semantic_snapshot();
    let first = semantics
        .iter()
        .find(|node| node.data.label == "First")
        .unwrap();
    let second = semantics
        .iter()
        .find(|node| node.data.label == "Second")
        .unwrap();
    assert_eq!(first.bounds.size.width, 100.0);
    assert_eq!(second.bounds.origin.x, 100.0);
    assert_eq!(second.bounds.size.width, 200.0);
}

#[test]
fn alignment_centers_intrinsic_content_in_the_viewport() {
    let mut ui = UiTree::new(
        Align {
            alignment: Alignment::Center,
            padding: 0.0,
        },
        LogicalSize::new(200.0, 100.0),
    );
    ui.append(ui.root(), Label::new("Centered"));
    ui.update_passes();
    let label = ui
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Centered")
        .unwrap();
    assert!((label.bounds.origin.x + label.bounds.size.width * 0.5 - 100.0).abs() < 0.01);
    assert!((label.bounds.origin.y + label.bounds.size.height * 0.5 - 50.0).abs() < 0.01);
}

#[test]
fn accessibility_reports_deltas_and_removals() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    let child = ui.append(
        ui.root(),
        BoxElement {
            size: LogicalSize::new(40.0, 20.0),
            color: Color::WHITE,
            semantics: Some(SemanticData {
                role: SemanticRole::Field,
                label: "Width".into(),
                value: Some("40".into()),
                ..SemanticData::default()
            }),
            interactive: true,
        },
    );
    let first = ui.update_passes();
    assert!(
        first
            .accessibility
            .changed
            .iter()
            .any(|node| node.id == child.id())
    );
    let mut semantics = ui.element(child).semantics.clone().unwrap();
    semantics.value = Some("41".into());
    ui.box_mut(child).set_semantics(Some(semantics));
    let changed = ui.update_passes();
    assert_eq!(changed.accessibility.changed.len(), 1);
    ui.remove(child.id());
    let removed = ui.update_passes();
    assert!(removed.accessibility.removed.contains(&child.id()));
}

/// Moving a subtree between parents keeps everything the subtree had built up.
///
/// The point of a retained tree is that a structural move is not a rebuild: the
/// moved nodes keep their identities, their cached paint fragments, their
/// published semantics, and the constraints they were last measured against, and
/// only the geometry that actually moved is recomputed. Focus is stored as an
/// identity rather than a path, so it survives for free, which is worth pinning
/// rather than assuming.
#[test]
fn reparent_preserves_fragments_semantics_and_focus() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Horizontal,
            ..Flex::default()
        },
        LogicalSize::new(400.0, 200.0),
    );
    let root = ui.root();
    let left = ui
        .append(
            root,
            Flex {
                axis: Axis::Vertical,
                ..Flex::default()
            },
        )
        .id();
    let right = ui
        .append(
            root,
            Flex {
                axis: Axis::Vertical,
                ..Flex::default()
            },
        )
        .id();
    // The moved subtree is a container with a focusable control inside it, so
    // the move crosses a level rather than only changing one node's parent.
    let moved = ui
        .append(
            left,
            Flex {
                axis: Axis::Vertical,
                ..Flex::default()
            },
        )
        .id();
    let button = ui.append(
        moved,
        TestControl {
            label: "Moved",
            size: LogicalSize::new(80.0, 24.0),
        },
    );
    ui.append(right, Label::new("Stationary"));
    ui.update_passes();

    ui.set_focus(Some(button.id()));
    ui.update_passes();
    let described = |ui: &UiTree| {
        ui.semantic_snapshot()
            .into_iter()
            .find(|node| node.data.label == "Moved")
            .expect("the moved button describes itself")
    };
    let before = described(&ui);
    assert!(before.focused);
    assert_eq!(before.parent, Some(moved));

    // Index 1 puts it after the stationary label, so the move is observable in
    // the bounds it is described at.
    ui.reparent(moved, right, 1);
    let stats = ui.update_passes().stats;

    assert_eq!(ui.focused(), Some(button.id()), "focus is an identity");
    assert_eq!(
        stats.rebuilt_fragments, 2,
        "only the two parents, whose child lists changed, repaint"
    );
    assert!(
        !ui.scene().rebuilt(button.id()),
        "nothing repainted the moved button"
    );
    assert_eq!(stats.shaped_text, 0, "no text was reshaped by the move");

    let after = described(&ui);
    assert!(after.focused, "the moved control is still the focused one");
    assert_eq!(after.parent, Some(moved), "its own parent did not change");
    assert_eq!(after.bounds.size, before.bounds.size);
    assert!(
        after.bounds.origin.y > before.bounds.origin.y,
        "it is described where it moved to"
    );
    assert_eq!(
        *ui.dispatch(UiInput::Paste(String::new()))
            .unwrap()
            .downcast::<Action>()
            .unwrap(),
        Action::Activate,
        "keyboard routing still reaches it through focus"
    );
}

/// Reparenting must leave the tree in the state a fresh build would reach.
///
/// Composition prunes any subtree whose parent produced the same world transform
/// and clip, so a structural move that failed to invalidate the moved subtree
/// would keep composing it under its old ancestor's transform and clip. The
/// stale values are self-consistent, and therefore invisible to a test that only
/// inspects the tree that was mutated. `compose_equivalence.rs` makes this
/// argument in general over random trees and random mutations; this is the same
/// comparison for one topology operation that sweep does not yet perform.
#[test]
fn a_reparented_subtree_composes_like_a_freshly_built_one() {
    fn described(label: &str, size: LogicalSize) -> BoxElement {
        BoxElement {
            size,
            color: Color::WHITE,
            semantics: Some(SemanticData {
                role: SemanticRole::Label,
                label: label.into(),
                ..SemanticData::default()
            }),
            interactive: true,
        }
    }

    fn column() -> Flex {
        Flex {
            axis: Axis::Vertical,
            gap: 3.0,
            padding: 2.0,
            ..Flex::default()
        }
    }

    let viewport = LogicalSize::new(200.0, 160.0);
    // A clipping `Scroll` on one side, so the moved subtree inherits a different
    // clip as well as a different transform.
    let build = |moved_into_right: bool| {
        let mut ui = UiTree::new(column(), viewport);
        let root = ui.root();
        let left = ui.append(root, Scroll::new(ScrollAxis::Vertical));
        let right = ui.append(root, column());
        let host = if moved_into_right {
            right.id()
        } else {
            left.id()
        };
        let carried = ui.append(host, column()).id();
        ui.append(
            carried,
            described("carried one", LogicalSize::new(40.0, 18.0)),
        );
        ui.append(
            carried,
            described("carried two", LogicalSize::new(30.0, 12.0)),
        );
        // Appended last, and the move lands ahead of it, so both trees end with
        // the same child order.
        ui.append(
            right.id(),
            described("resident", LogicalSize::new(50.0, 20.0)),
        );
        ui.update_passes();
        (ui, carried, right.id())
    };

    let (mut live, carried, right) = build(false);
    live.reparent(carried, right, 0);
    live.update_passes();
    let (mut fresh, _, _) = build(true);

    let live_fragments = live.scene().fragments().to_vec();
    let fresh_fragments = fresh.scene().fragments().to_vec();
    assert_eq!(live_fragments.len(), fresh_fragments.len());
    for (index, (live_instance, fresh_instance)) in
        live_fragments.iter().zip(&fresh_fragments).enumerate()
    {
        assert_eq!(
            (live_instance.transform, live_instance.clip),
            (fresh_instance.transform, fresh_instance.clip),
            "fragment {index} composed differently after the move"
        );
    }

    let described_rows = |ui: &UiTree| {
        let mut rows = ui
            .semantic_snapshot()
            .into_iter()
            .map(|node| (node.data.label, node.bounds, node.enabled))
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| left.0.cmp(&right.0));
        rows
    };
    assert_eq!(described_rows(&live), described_rows(&fresh));

    // Hit testing reads `subtree_bounds` and the cached inverse transform, which
    // no other assertion here can observe.
    let label_at = |ui: &mut UiTree, point| {
        ui.hit_test(point).and_then(|id| {
            ui.semantic_snapshot()
                .into_iter()
                .find(|node| node.id == id)
                .map(|node| node.data.label)
        })
    };
    for row in 0..8 {
        for column in 0..8 {
            let point = LogicalPoint::new(
                (column as f32 + 0.5) * viewport.width / 8.0,
                (row as f32 + 0.5) * viewport.height / 8.0,
            );
            assert_eq!(
                label_at(&mut live, point),
                label_at(&mut fresh, point),
                "hit_test({}, {}) diverged from a fresh build",
                point.x,
                point.y,
            );
        }
    }
}

#[test]
#[should_panic(expected = "reparent would make a node its own ancestor")]
fn reparent_refuses_to_make_a_node_its_own_ancestor() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(200.0, 100.0));
    let root = ui.root();
    let outer = ui.append(root, Flex::default()).id();
    let inner = ui.append(outer, Flex::default()).id();
    ui.update_passes();

    ui.reparent(outer, inner, 0);
}

#[test]
#[should_panic(expected = "reparent would make a node its own ancestor")]
fn reparent_refuses_to_make_a_node_its_own_parent() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(200.0, 100.0));
    let outer = ui.append(ui.root(), Flex::default()).id();
    ui.reparent(outer, outer, 0);
}

#[test]
#[should_panic(expected = "retained root cannot be reparented")]
fn reparent_refuses_to_move_the_root() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(200.0, 100.0));
    let root = ui.root();
    let inner = ui.append(root, Flex::default()).id();
    ui.reparent(root, inner, 0);
}

#[test]
fn node_identity_bits_survive_slot_reuse() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(80.0, 40.0));
    let root = ui.root();
    let first = ui.append(root, Flex::default()).id();
    ui.remove(first);
    let second = ui.append(root, Flex::default()).id();
    assert_ne!(
        first.to_bits(),
        second.to_bits(),
        "a recycled slot must not collide with its earlier occupant",
    );
    assert_eq!(root.to_bits(), ui.root().to_bits());
}
