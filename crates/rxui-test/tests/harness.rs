//! Label-addressed retained-tree harness behavior.

use astrelis_core::geometry::{LogicalPoint, LogicalSize};
use astrelis_platform::{CursorIcon, Key, Modifiers, NamedKey};
use rxui_test::{
    Harness, MemoryClipboard,
    probe::{Probe, ProbeEvent, ProbeLog},
};
use rxui_tree::{Axis, Flex, PassStats, UiTree};

fn probes(specs: &[(&str, CursorIcon)]) -> (Harness, Vec<ProbeLog>) {
    let mut tree = UiTree::new(
        Flex {
            axis: Axis::Vertical,
            ..Flex::default()
        },
        LogicalSize::new(240.0, 120.0),
    );
    let root = tree.root();
    let mut logs = Vec::new();
    for (label, cursor) in specs {
        let (probe, log) = Probe::new(*label, LogicalSize::new(100.0, 30.0));
        tree.append(root, probe.with_cursor(*cursor));
        logs.push(log);
    }
    (Harness::new(tree), logs)
}

#[test]
fn click_addresses_the_semantic_centre_and_only_the_requested_control() {
    let (mut harness, logs) = probes(&[
        ("Save", CursorIcon::Pointer),
        ("Cancel", CursorIcon::Default),
    ]);

    harness.click("Save");

    assert_eq!(
        logs[0].events(),
        vec![
            ProbeEvent::HoverChanged(true),
            ProbeEvent::FocusChanged(true),
            ProbeEvent::PointerPressed(LogicalPoint::new(50.0, 15.0)),
            ProbeEvent::PointerReleased(LogicalPoint::new(50.0, 15.0)),
            ProbeEvent::Activated,
        ]
    );
    assert!(logs[1].events().is_empty());
}

#[test]
fn semantic_activation_does_not_depend_on_pointer_geometry() {
    let (mut harness, logs) = probes(&[("Save", CursorIcon::Pointer)]);

    harness.activate("Save");

    assert_eq!(logs[0].events(), vec![ProbeEvent::Activated]);
    assert_eq!(harness.focused(), None);
}

#[test]
fn hover_transitions_and_pointer_left_select_the_native_cursor() {
    let (mut harness, logs) = probes(&[
        ("Save", CursorIcon::Pointer),
        ("Cancel", CursorIcon::Crosshair),
    ]);

    harness.hover("Save");
    assert_eq!(harness.cursor_icon(), CursorIcon::Pointer);
    harness.hover("Cancel");
    assert_eq!(harness.cursor_icon(), CursorIcon::Crosshair);
    harness.pointer_left();
    assert_eq!(harness.cursor_icon(), CursorIcon::Default);

    assert_eq!(
        logs[0].events(),
        vec![
            ProbeEvent::HoverChanged(true),
            ProbeEvent::PointerMoved(LogicalPoint::new(50.0, 15.0)),
            ProbeEvent::HoverChanged(false),
        ]
    );
    assert_eq!(
        logs[1].events(),
        vec![
            ProbeEvent::HoverChanged(true),
            ProbeEvent::PointerMoved(LogicalPoint::new(50.0, 15.0)),
            ProbeEvent::HoverChanged(false),
        ]
    );
}

#[test]
fn pointer_capture_routes_drag_and_release_after_leaving_the_control() {
    let (mut harness, logs) = probes(&[("Drag", CursorIcon::Move)]);
    let centre = LogicalPoint::new(50.0, 15.0);
    let outside = LogicalPoint::new(220.0, 100.0);

    harness.press_pointer_at(centre);
    harness.hover_at(outside);
    assert_eq!(harness.cursor_icon(), CursorIcon::Move);
    harness.release_pointer_at(outside);
    assert_eq!(harness.cursor_icon(), CursorIcon::Default);

    assert!(
        logs[0]
            .events()
            .contains(&ProbeEvent::PointerMoved(outside))
    );
    assert!(
        logs[0]
            .events()
            .contains(&ProbeEvent::PointerReleased(outside))
    );
}

#[test]
fn focus_traversal_keyboard_activation_and_snapshot_follow_tree_order() {
    let (mut harness, logs) = probes(&[
        ("First", CursorIcon::Default),
        ("Second", CursorIcon::Default),
        ("Third", CursorIcon::Default),
    ]);

    harness.focus_first();
    assert_eq!(harness.focused(), Some(harness.find("First").id));
    harness.press(NamedKey::Space);
    assert_eq!(logs[0].activations(), 1);

    harness.press(NamedKey::Tab);
    assert_eq!(harness.focused(), Some(harness.find("Second").id));
    harness.press_with_modifiers(
        NamedKey::Tab,
        Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(harness.focused(), Some(harness.find("First").id));
    harness.press(NamedKey::Tab);
    assert_eq!(harness.focused(), Some(harness.find("Second").id));
    harness.press(NamedKey::Enter);
    assert_eq!(logs[1].activations(), 1);
    assert_eq!(logs[2].activations(), 0);

    assert!(harness.snapshot().contains(
        "Button label=\"Second\" value=None bounds=(0.00,30.00) 100.00x30.00 enabled=true focused=true"
    ));
}

#[test]
fn typed_text_reaches_the_focused_element() {
    let (mut harness, logs) = probes(&[("Field", CursorIcon::Text)]);
    harness.focus_first();
    logs[0].clear();

    harness.type_text("ab");

    assert_eq!(
        logs[0].events(),
        vec![
            ProbeEvent::Key(Key::Character("a".into())),
            ProbeEvent::Key(Key::Character("b".into())),
        ]
    );
}

#[test]
fn clipboard_operations_are_drained_applied_and_counted() {
    let mut tree = UiTree::new(Flex::default(), LogicalSize::new(100.0, 30.0));
    let root = tree.root();
    let copy = NamedKey::Other("Copy".into());
    let (probe, _) = Probe::new("Copy", LogicalSize::new(100.0, 30.0));
    tree.append(
        root,
        probe.with_clipboard_write(copy.clone(), "copied text"),
    );
    let mut harness = Harness::new(tree);
    harness.focus_first();
    harness.press(copy);

    let mut clipboard = MemoryClipboard::default();
    assert_eq!(harness.run_pending_services(&mut clipboard), 1);
    assert_eq!(clipboard.text(), Some("copied text"));
    assert_eq!(harness.run_pending_services(&mut clipboard), 0);
}

#[test]
fn settled_pointer_move_reports_exactly_hit_test_work() {
    let (mut harness, _) = probes(&[("Target", CursorIcon::Pointer)]);
    harness.hover_at(LogicalPoint::new(10.0, 10.0));

    harness.hover_at(LogicalPoint::new(20.0, 12.0));

    assert_eq!(
        harness.stats(),
        PassStats {
            hit_test_nodes: 2,
            ..PassStats::default()
        }
    );
}

#[test]
fn non_pointer_input_does_not_reuse_pointer_hit_test_stats() {
    let (mut harness, _) = probes(&[("Target", CursorIcon::Pointer)]);
    harness.hover("Target");
    assert_eq!(harness.stats().hit_test_nodes, 2);

    harness.press(NamedKey::Escape);

    assert_eq!(harness.stats(), PassStats::default());
}
