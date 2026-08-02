//! Layout pass counting and geometry stability for the container elements.

use astrelis_core::{
    color::Color,
    geometry::{LogicalRect, LogicalSize},
};
use rxui_tree::{
    Align, Alignment, Axis, BoxElement, Flex, Frame, Label, NodeId, SemanticData, SemanticRole,
    UiTree,
};

/// A zero-cost geometry probe: a box reports its accessible bounds, and a
/// [`Frame`] always lays its children out with tight constraints, so the probe
/// mirrors its parent's resolved rectangle.
fn probe(label: &str, size: LogicalSize) -> BoxElement {
    BoxElement {
        size,
        color: Color::WHITE,
        semantics: Some(SemanticData {
            role: SemanticRole::Group,
            label: label.into(),
            ..SemanticData::default()
        }),
        interactive: false,
    }
}

fn bounds(ui: &UiTree, label: &str) -> LogicalRect {
    ui.semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == label)
        .unwrap_or_else(|| panic!("missing semantic node for {label}"))
        .bounds
}

fn grow_frame(ui: &mut UiTree, grow: f32) -> NodeId {
    let root = ui.root();
    ui.append(
        root,
        Frame {
            grow,
            ..Frame::default()
        },
    )
    .id()
}

#[test]
fn flex_lays_out_each_grow_child_once_per_pass() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Horizontal,
            ..Flex::default()
        },
        LogicalSize::new(300.0, 100.0),
    );
    for _ in 0..3 {
        grow_frame(&mut ui, 1.0);
    }
    let stats = ui.update_passes().stats;
    // One root flex plus one layout per growing child: the flex no longer
    // measures growing children against a degenerate main-axis constraint only
    // to overwrite the result while resolving growth.
    assert_eq!(stats.layout_elements, 4);
}

#[test]
fn nested_flex_frame_label_lays_out_and_shapes_each_element_once() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Horizontal,
            ..Flex::default()
        },
        LogicalSize::new(300.0, 100.0),
    );
    for index in 0..3 {
        let frame = grow_frame(&mut ui, 1.0);
        ui.append(frame, Label::new(format!("Row {index}")));
    }
    let stats = ui.update_passes().stats;
    // Root flex + 3 frames + 3 labels, each laid out exactly once. The frames
    // receive a tight constraint from growth resolution, so both axes are pinned
    // and their measuring pass is skipped.
    assert_eq!(stats.layout_elements, 7);
    // One shaping call per label instead of one per redundant label layout.
    assert_eq!(stats.shaped_text, 3);

    // Growth still resolves to equal thirds of the viewport.
    for (index, origin) in [0.0, 100.0, 200.0].into_iter().enumerate() {
        let label = bounds(&ui, &format!("Row {index}"));
        assert_eq!(label.origin.x, origin);
        assert_eq!(label.size.width, 100.0);
        assert_eq!(label.size.height, 100.0);
    }
}

#[test]
fn mixed_fixed_and_grow_children_keep_exact_geometry() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Horizontal,
            gap: 10.0,
            padding: 5.0,
            ..Flex::default()
        },
        LogicalSize::new(300.0, 100.0),
    );
    let fixed = ui
        .append(
            ui.root(),
            Frame {
                width: Some(50.0),
                height: Some(20.0),
                ..Frame::default()
            },
        )
        .id();
    ui.append(fixed, probe("fixed", LogicalSize::new(10.0, 10.0)));
    let grow = grow_frame(&mut ui, 1.0);
    ui.append(grow, probe("grow", LogicalSize::new(10.0, 10.0)));

    let stats = ui.update_passes().stats;
    // Root flex + 2 frames + 2 probes; every frame here is pinned on both axes.
    assert_eq!(stats.layout_elements, 5);

    // Inner space is 290x90, the fixed child consumes 50 and the gap 10, so the
    // growing child receives the remaining 230 and stretches across the cross axis.
    assert_eq!(
        bounds(&ui, "fixed"),
        LogicalRect::from_xywh(5.0, 5.0, 50.0, 20.0)
    );
    assert_eq!(
        bounds(&ui, "grow"),
        LogicalRect::from_xywh(65.0, 5.0, 230.0, 90.0)
    );
}

#[test]
fn frame_resolves_every_pinned_axis_combination() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Vertical,
            ..Flex::default()
        },
        LogicalSize::new(400.0, 400.0),
    );
    for (label, width, height) in [
        ("both", Some(80.0), Some(40.0)),
        ("width", Some(80.0), None),
        ("height", None, Some(40.0)),
        ("neither", None, None),
    ] {
        let frame = ui
            .append(
                ui.root(),
                Frame {
                    width,
                    height,
                    ..Frame::default()
                },
            )
            .id();
        ui.append(frame, probe(label, LogicalSize::new(60.0, 20.0)));
    }
    let stats = ui.update_passes().stats;
    // Root flex + 4 frames + 4 probes = 9 mandatory layouts. The three frames
    // with an unpinned axis still measure their child, adding one layout each;
    // only the fully explicit frame skips measuring.
    assert_eq!(stats.layout_elements, 12);

    // Explicit extents win, and every unpinned axis still comes from content.
    assert_eq!(
        bounds(&ui, "both"),
        LogicalRect::from_xywh(0.0, 0.0, 80.0, 40.0)
    );
    assert_eq!(
        bounds(&ui, "width"),
        LogicalRect::from_xywh(0.0, 40.0, 80.0, 20.0)
    );
    assert_eq!(
        bounds(&ui, "height"),
        LogicalRect::from_xywh(0.0, 60.0, 60.0, 40.0)
    );
    assert_eq!(
        bounds(&ui, "neither"),
        LogicalRect::from_xywh(0.0, 100.0, 60.0, 20.0)
    );
}

#[test]
fn frame_min_and_max_still_clamp_measured_content() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Vertical,
            ..Flex::default()
        },
        LogicalSize::new(400.0, 400.0),
    );
    let frame = ui
        .append(
            ui.root(),
            Frame {
                min: LogicalSize::new(30.0, 30.0),
                max: Some(LogicalSize::new(50.0, 50.0)),
                ..Frame::default()
            },
        )
        .id();
    ui.append(frame, probe("clamped", LogicalSize::new(60.0, 20.0)));
    ui.update_passes();

    // The child measures 50x20 against the frame maximum, then the frame clamps
    // that content into [30, 50] on both axes.
    assert_eq!(
        bounds(&ui, "clamped"),
        LogicalRect::from_xywh(0.0, 0.0, 50.0, 30.0)
    );
}

#[test]
fn flex_without_children_lays_out_only_itself() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(100.0, 100.0));
    let stats = ui.update_passes().stats;
    assert_eq!(stats.layout_elements, 1);
}

#[test]
fn flex_with_one_grow_child_fills_the_viewport_once() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Vertical,
            ..Flex::default()
        },
        LogicalSize::new(100.0, 80.0),
    );
    let frame = grow_frame(&mut ui, 1.0);
    ui.append(frame, probe("single", LogicalSize::new(10.0, 10.0)));

    let stats = ui.update_passes().stats;
    // Root flex + frame + probe.
    assert_eq!(stats.layout_elements, 3);
    assert_eq!(
        bounds(&ui, "single"),
        LogicalRect::from_xywh(0.0, 0.0, 100.0, 80.0)
    );
}

#[test]
fn grow_child_with_no_remaining_space_collapses_on_the_main_axis() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Horizontal,
            ..Flex::default()
        },
        LogicalSize::new(100.0, 50.0),
    );
    let blocker = ui
        .append(
            ui.root(),
            Frame {
                width: Some(100.0),
                height: Some(50.0),
                ..Frame::default()
            },
        )
        .id();
    ui.append(blocker, probe("blocker", LogicalSize::new(10.0, 10.0)));
    let grow = grow_frame(&mut ui, 1.0);
    ui.append(grow, probe("saturated", LogicalSize::new(10.0, 10.0)));

    let stats = ui.update_passes().stats;
    // Root flex + 2 frames + 2 probes; the collapsed frame is still laid out and
    // placed exactly once.
    assert_eq!(stats.layout_elements, 5);
    assert_eq!(
        bounds(&ui, "blocker"),
        LogicalRect::from_xywh(0.0, 0.0, 100.0, 50.0)
    );
    assert_eq!(
        bounds(&ui, "saturated"),
        LogicalRect::from_xywh(100.0, 0.0, 0.0, 50.0)
    );
}

#[test]
fn unequal_growth_shares_remain_proportional() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Horizontal,
            ..Flex::default()
        },
        LogicalSize::new(300.0, 100.0),
    );
    let first = grow_frame(&mut ui, 1.0);
    ui.append(first, probe("first", LogicalSize::new(10.0, 10.0)));
    let second = grow_frame(&mut ui, 2.0);
    ui.append(second, probe("second", LogicalSize::new(10.0, 10.0)));

    let stats = ui.update_passes().stats;
    assert_eq!(stats.layout_elements, 5);
    assert_eq!(
        bounds(&ui, "first"),
        LogicalRect::from_xywh(0.0, 0.0, 100.0, 100.0)
    );
    assert_eq!(
        bounds(&ui, "second"),
        LogicalRect::from_xywh(100.0, 0.0, 200.0, 100.0)
    );
}

/// A child a container stops measuring loses its geometry, and gets it back.
///
/// `Align` documents one intrinsic child and lays out only the first, so the
/// second child of an `Align` is never measured. Nothing but the pass itself can
/// notice that: the child keeps whatever `size` and `offset` it was last given,
/// which is self-consistent and therefore invisible to any check that inspects
/// only the live tree. The recovery half is the subtle one - discarding must
/// also drop `last_constraints`, or `layout_node`'s cached-size early return
/// hands back the zero once the container starts measuring the child again.
#[test]
fn a_child_a_container_stops_measuring_is_zeroed_and_restored() {
    let mut ui = UiTree::new(
        Align {
            alignment: Alignment::TopLeading,
            padding: 0.0,
        },
        LogicalSize::new(200.0, 160.0),
    );
    let root = ui.root();
    let first = ui.append(root, probe("first", LogicalSize::new(30.0, 20.0)));
    let first = first.id();
    let second = ui.append(root, probe("second", LogicalSize::new(50.0, 40.0)));
    let second = second.id();
    ui.update_passes();
    assert_eq!(
        bounds(&ui, "first"),
        LogicalRect::from_xywh(0.0, 0.0, 30.0, 20.0),
    );
    assert_eq!(
        bounds(&ui, "second"),
        LogicalRect::default(),
        "Align measures only its first child, so the second starts at zero",
    );

    ui.set_children(root, &[second, first]);
    ui.update_passes();
    assert_eq!(
        bounds(&ui, "second"),
        LogicalRect::from_xywh(0.0, 0.0, 50.0, 40.0),
    );
    assert_eq!(
        bounds(&ui, "first"),
        LogicalRect::default(),
        "the child that moved out of slot zero must not keep its old rectangle",
    );

    ui.set_children(root, &[first, second]);
    ui.update_passes();
    assert_eq!(
        bounds(&ui, "first"),
        LogicalRect::from_xywh(0.0, 0.0, 30.0, 20.0),
        "measuring the child again must recompute it, not reuse the discard",
    );
    assert_eq!(bounds(&ui, "second"), LogicalRect::default());
}
