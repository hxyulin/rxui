//! Retained invalidation bookkeeping, relayout propagation, and cached input transforms.

use std::{any::Any, cell::RefCell, rc::Rc};

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalSize},
    math::{Affine2, Vec2},
};
use rxui_tree::{
    Axis, BoxElement, Constraints, Element, Flex, Invalidation, LayoutContext, NodeId,
    SemanticData, SemanticRole, Stack, UiInput, UiTree,
};

#[derive(Clone, Debug, Default, PartialEq)]
struct ProbeLog {
    hit_tests: usize,
    hit_local: Option<LogicalPoint>,
    event_local: Option<LogicalPoint>,
}

/// Instrumented leaf which records the local coordinates it is handed.
struct Probe {
    label: String,
    /// Fixed size, or `None` to fill the incoming maximum constraint.
    size: Option<LogicalSize>,
    transform: Affine2,
    log: Rc<RefCell<ProbeLog>>,
}

impl Probe {
    fn new(label: &str, size: Option<LogicalSize>) -> Self {
        Self {
            label: label.into(),
            size,
            transform: Affine2::IDENTITY,
            log: Rc::new(RefCell::new(ProbeLog::default())),
        }
    }

    fn with_transform(mut self, transform: Affine2) -> Self {
        self.transform = transform;
        self
    }

    fn log(&self) -> Rc<RefCell<ProbeLog>> {
        Rc::clone(&self.log)
    }
}

impl Element for Probe {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(
        &mut self,
        _context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LogicalSize {
        constraints.constrain(self.size.unwrap_or(constraints.max))
    }

    fn accessibility(&self) -> Option<SemanticData> {
        Some(SemanticData {
            role: SemanticRole::Label,
            label: self.label.clone(),
            ..SemanticData::default()
        })
    }

    fn event(&mut self, input: UiInput) -> rxui_tree::EventResult {
        if matches!(input, UiInput::HoverChanged(_)) {
            return rxui_tree::EventResult {
                invalidation: Invalidation::PAINT,
                ..Default::default()
            };
        }
        let handled = matches!(input, UiInput::PointerPressed(_));
        if let UiInput::PointerMoved(point)
        | UiInput::PointerPressed(point)
        | UiInput::PointerReleased(point) = input
        {
            self.log.borrow_mut().event_local = Some(point);
        }
        rxui_tree::EventResult {
            handled,
            ..Default::default()
        }
    }

    fn hit_test(&self, point: LogicalPoint, size: LogicalSize) -> bool {
        let mut log = self.log.borrow_mut();
        log.hit_tests += 1;
        log.hit_local = Some(point);
        point.x >= 0.0 && point.y >= 0.0 && point.x <= size.width && point.y <= size.height
    }

    fn hit_testable(&self) -> bool {
        true
    }

    fn focusable(&self) -> bool {
        true
    }

    fn transform(&self) -> Affine2 {
        self.transform
    }
}

fn label_bounds(ui: &UiTree, label: &str) -> astrelis_core::geometry::LogicalRect {
    ui.semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == label)
        .expect("semantic node exists")
        .bounds
}

#[test]
fn dirty_state_accessors_track_pending_passes_until_update() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    let child = ui.append(
        ui.root(),
        BoxElement::new(LogicalSize::new(40.0, 20.0), Color::WHITE),
    );
    assert!(ui.needs_update());
    assert!(ui.needs_redraw());
    assert!(ui.invalidation().contains(Invalidation::LAYOUT));

    ui.update_passes();
    assert_eq!(ui.invalidation(), Invalidation::empty());
    assert!(!ui.needs_update());
    assert!(!ui.needs_redraw());

    ui.box_mut(child).set_color(Color::BLACK);
    assert_eq!(ui.invalidation(), Invalidation::PAINT);
    assert!(ui.needs_update());
    assert!(ui.needs_redraw());

    ui.update_passes();
    assert!(!ui.needs_update());
}

#[test]
fn accessibility_only_work_needs_an_update_without_a_redraw() {
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
    ui.update_passes();

    let mut semantics = ui.element(child).semantics.clone().unwrap();
    semantics.value = Some("41".into());
    ui.box_mut(child).set_semantics(Some(semantics));
    assert!(ui.needs_update(), "the accessibility pass is still pending");
    assert!(
        !ui.needs_redraw(),
        "accessibility deltas never change rendered output"
    );

    ui.box_mut(child).set_interactive(false);
    assert!(!ui.needs_redraw(), "hit shapes never change pixels either");

    ui.update_passes();
    assert!(!ui.needs_update());
}

#[test]
fn settled_pointer_move_within_one_target_leaves_the_tree_clean() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Horizontal,
            ..Flex::default()
        },
        LogicalSize::new(240.0, 60.0),
    );
    ui.append(
        ui.root(),
        Probe::new("First", Some(LogicalSize::new(100.0, 30.0))),
    );
    ui.append(
        ui.root(),
        Probe::new("Second", Some(LogicalSize::new(100.0, 30.0))),
    );
    ui.update_passes();

    // Entering a control is a real hover transition and must repaint it.
    ui.dispatch(UiInput::PointerMoved(LogicalPoint::new(10.0, 10.0)));
    assert!(ui.needs_redraw());
    ui.update_passes();

    // Moving inside the same control changes nothing observable.
    ui.dispatch(UiInput::PointerMoved(LogicalPoint::new(20.0, 12.0)));
    assert!(
        !ui.needs_update(),
        "a no-op pointer move must not request any pass"
    );
    assert!(!ui.needs_redraw());
    assert_eq!(ui.update_passes().stats.rebuilt_fragments, 0);
}

/// Builds a `Stack` chain of `depth` containers with one instrumented leaf.
fn deep_chain(depth: usize, viewport: LogicalSize) -> (UiTree, rxui_tree::NodeHandle<BoxElement>) {
    let mut ui = UiTree::new(Stack::default(), viewport);
    let mut parent = ui.root();
    for _ in 1..depth {
        parent = ui.append(parent, Stack::default()).id();
    }
    let leaf = ui.append(
        parent,
        BoxElement {
            semantics: Some(SemanticData {
                role: SemanticRole::Label,
                label: "leaf".into(),
                ..SemanticData::default()
            }),
            ..BoxElement::new(LogicalSize::new(40.0, 20.0), Color::WHITE)
        },
    );
    ui.update_passes();
    (ui, leaf)
}

#[test]
fn deep_leaf_invalidation_still_relayouts_from_the_root() {
    let (mut ui, leaf) = deep_chain(4, LogicalSize::new(400.0, 300.0));

    ui.box_mut(leaf).set_size(LogicalSize::new(80.0, 20.0));
    let stats = ui.update_passes().stats;
    assert_eq!(
        stats.layout_elements, 5,
        "every container from the root down to the leaf relayouts"
    );
    assert_eq!(label_bounds(&ui, "leaf").size, LogicalSize::new(80.0, 20.0));
}

#[test]
fn repeated_leaf_invalidation_still_relayouts_from_the_root() {
    let (mut ui, leaf) = deep_chain(4, LogicalSize::new(400.0, 300.0));

    // The second and third mutations hit the root-ward early exit: the parent
    // already carries every propagated bit. The relayout must still happen.
    for width in [80.0, 90.0, 100.0] {
        ui.box_mut(leaf).set_size(LogicalSize::new(width, 20.0));
    }
    assert!(ui.needs_redraw());
    let stats = ui.update_passes().stats;
    assert_eq!(
        stats.layout_elements, 5,
        "coalesced mutations still relayout the whole chain once"
    );
    assert_eq!(
        label_bounds(&ui, "leaf").size,
        LogicalSize::new(100.0, 20.0)
    );

    // A paint-only mutation between two layout mutations must not let the early
    // exit swallow the layout bit on the way to the root.
    ui.box_mut(leaf).set_color(Color::BLACK);
    ui.box_mut(leaf).set_size(LogicalSize::new(60.0, 20.0));
    let stats = ui.update_passes().stats;
    assert_eq!(stats.layout_elements, 5);
    assert_eq!(label_bounds(&ui, "leaf").size, LogicalSize::new(60.0, 20.0));
}

#[test]
fn nested_resize_relayouts_and_recomposes_the_changed_subtree() {
    let mut ui = UiTree::new(Stack::default(), LogicalSize::new(400.0, 300.0));
    let middle = ui.append(ui.root(), Stack::default());
    let probe = Probe::new("fill", None);
    ui.append(middle.id(), probe);
    ui.update_passes();
    assert_eq!(
        label_bounds(&ui, "fill").size,
        LogicalSize::new(400.0, 300.0)
    );

    ui.set_viewport(LogicalSize::new(200.0, 120.0));
    assert!(ui.needs_redraw());
    let stats = ui.update_passes().stats;
    assert_eq!(
        stats.layout_elements, 3,
        "the resize reaches the nested leaf"
    );
    assert_eq!(
        stats.composed_nodes, 3,
        "the resized leaf and its container recompose"
    );
    let bounds = label_bounds(&ui, "fill");
    assert_eq!(bounds.size, LogicalSize::new(200.0, 120.0));
    assert_eq!(bounds.origin, LogicalPoint::ZERO);
    assert!(!ui.needs_update(), "the resize settles in one update");
}

#[test]
fn pointer_dispatch_localizes_through_a_transform_with_one_hit_test() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    let probe = Probe::new("probe", Some(LogicalSize::new(100.0, 50.0)))
        .with_transform(Affine2::from_translation(Vec2::new(10.0, 5.0)));
    let log = probe.log();
    ui.append(ui.root(), probe);
    ui.update_passes();
    log.borrow_mut().hit_tests = 0;

    ui.dispatch(UiInput::PointerMoved(LogicalPoint::new(30.0, 15.0)));

    let recorded = log.borrow().clone();
    assert_eq!(
        recorded.hit_local,
        Some(LogicalPoint::new(20.0, 10.0)),
        "the cached inverse world transform localizes hit tests"
    );
    assert_eq!(
        recorded.event_local,
        Some(LogicalPoint::new(20.0, 10.0)),
        "dispatch localizes with the same cached inverse"
    );
    assert_eq!(
        recorded.hit_tests, 1,
        "hover and dispatch share a single traversal per pointer event"
    );
    // `hit_test` overwrites (never accumulates) this counter, so it records the
    // size of the most recent traversal: the root plus the probe.
    assert_eq!(ui.stats().hit_test_nodes, 2);

    // Wheel input never runs the hover traversal, so it must still hit-test.
    ui.dispatch(UiInput::PointerWheel {
        position: LogicalPoint::new(30.0, 15.0),
        delta: LogicalPoint::new(0.0, 10.0),
    });
    assert_eq!(log.borrow().hit_tests, 2);
}

#[test]
fn pointer_miss_still_clears_focus_and_capture() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    ui.append(
        ui.root(),
        Probe::new("Run", Some(LogicalSize::new(100.0, 30.0))),
    );
    ui.update_passes();

    ui.dispatch(UiInput::PointerPressed(LogicalPoint::new(10.0, 10.0)));
    assert!(ui.focused().is_some());
    ui.dispatch(UiInput::PointerPressed(LogicalPoint::new(380.0, 280.0)));
    assert!(
        ui.focused().is_none(),
        "pressing empty space clears retained focus"
    );

    // A release outside the pressed control is still delivered through capture,
    // and clears it afterwards.
    ui.dispatch(UiInput::PointerPressed(LogicalPoint::new(10.0, 10.0)));
    assert!(
        ui.dispatch(UiInput::PointerReleased(LogicalPoint::new(380.0, 280.0)))
            .is_none()
    );
    assert!(
        ui.dispatch(UiInput::PointerReleased(LogicalPoint::new(380.0, 280.0)))
            .is_none()
    );
}

/// Appends `count` boxes to `parent` and returns them in order.
fn boxes(ui: &mut UiTree, parent: NodeId, count: usize) -> Vec<NodeId> {
    (0..count)
        .map(|index| {
            ui.append(
                parent,
                BoxElement {
                    size: LogicalSize::new(10.0, 4.0),
                    color: Color::WHITE,
                    semantics: Some(SemanticData {
                        role: SemanticRole::Label,
                        label: format!("box {index}"),
                        ..SemanticData::default()
                    }),
                    interactive: true,
                },
            )
            .id()
        })
        .collect()
}

#[test]
fn set_children_with_the_order_it_already_has_requests_nothing() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Vertical,
            ..Flex::default()
        },
        LogicalSize::new(200.0, 200.0),
    );
    let root = ui.root();
    let children = boxes(&mut ui, root, 4);
    ui.update_passes();

    ui.set_children(root, &children);
    assert_eq!(
        ui.invalidation(),
        Invalidation::empty(),
        "republishing the order a parent already has must cost nothing"
    );
    assert_eq!(ui.stats().invalidate_steps, 0);
    let stats = ui.update_passes().stats;
    assert_eq!(stats.layout_elements, 0);
    assert_eq!(stats.rebuilt_fragments, 0);
    assert_eq!(stats.visited_accessibility_nodes, 0);
}

#[test]
fn set_children_that_only_reorders_asks_for_the_parent_and_nothing_below_it() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Vertical,
            ..Flex::default()
        },
        LogicalSize::new(200.0, 200.0),
    );
    let root = ui.root();
    let children = boxes(&mut ui, root, 4);
    ui.update_passes();

    let reordered = vec![children[2], children[0], children[3], children[1]];
    ui.set_children(root, &reordered);

    // A reorder is `TREE | LAYOUT`, and `Invalidation::TREE` is documented to
    // carry `LAYOUT_ALL` with it: a parent whose child order moved has to
    // relayout, and everything downstream of layout follows from that. So this is
    // the whole of what a reorder requests, and the interesting half of "nothing
    // else" is below: no child was individually dirtied.
    assert_eq!(
        ui.invalidation(),
        Invalidation::TREE | Invalidation::LAYOUT_ALL
    );
    let stats = ui.update_passes().stats;
    assert_eq!(
        stats.rebuilt_fragments, 1,
        "only the reordered parent repaints"
    );
    assert_eq!(
        stats.reused_fragments, 4,
        "every moved child keeps its cached fragment"
    );
    assert_eq!(
        ui.semantic_snapshot().len(),
        4,
        "every child is still described exactly once"
    );
}

#[test]
fn set_children_removes_a_child_left_out_of_the_list() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Vertical,
            ..Flex::default()
        },
        LogicalSize::new(200.0, 200.0),
    );
    let root = ui.root();
    let children = boxes(&mut ui, root, 3);
    ui.update_passes();

    // Leaving a child out used to detach it from its parent's list while its slot
    // stayed live, so it went on being reported by the semantic snapshot while no
    // pass could ever reach it again.
    ui.set_children(root, &[children[2], children[0]]);
    let removed = ui.update_passes().accessibility.removed.clone();
    assert!(!ui.contains(children[1]));
    assert!(removed.contains(&children[1]));
    assert_eq!(ui.semantic_snapshot().len(), 2);
}

#[test]
#[should_panic(expected = "duplicate retained child")]
fn set_children_rejects_a_duplicate_or_foreign_identity() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(200.0, 200.0));
    let root = ui.root();
    let children = boxes(&mut ui, root, 2);
    ui.update_passes();

    ui.set_children(root, &[children[0], children[0]]);
}

#[test]
#[should_panic(expected = "non-child identity")]
fn set_children_rejects_a_foreign_identity() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(200.0, 200.0));
    let root = ui.root();
    let children = boxes(&mut ui, root, 2);
    let nested = boxes(&mut ui, children[0], 1);
    ui.update_passes();
    ui.set_children(root, &[nested[0], children[1]]);
}

#[test]
fn insert_and_move_leave_their_siblings_cached() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Vertical,
            ..Flex::default()
        },
        LogicalSize::new(200.0, 200.0),
    );
    let root = ui.root();
    let children = boxes(&mut ui, root, 3);
    ui.update_passes();

    let inserted = ui.insert_child_at(
        root,
        1,
        BoxElement::new(LogicalSize::new(20.0, 6.0), Color::BLACK),
    );
    let stats = ui.update_passes().stats;
    assert_eq!(
        stats.rebuilt_fragments, 2,
        "the inserted node and its parent, not the siblings it shifted"
    );
    assert_eq!(
        ui.semantic_snapshot()
            .into_iter()
            .map(|node| node.bounds.origin.y)
            .collect::<Vec<_>>()
            .len(),
        3,
        "the inserted box describes nothing, so the three described boxes remain"
    );

    ui.move_child(root, children[2], 0);
    let stats = ui.update_passes().stats;
    assert_eq!(stats.rebuilt_fragments, 1, "only the parent repaints");
    assert_eq!(stats.reused_fragments, 4);
    // Clamped, and a move to where the child already is asks for nothing.
    ui.move_child(root, children[2], 0);
    assert_eq!(ui.invalidation(), Invalidation::empty());
    ui.move_child(root, children[2], 99);
    assert!(ui.contains(inserted.id()));
}

/// Freeing a subtree must cost the subtree, not the subtree times the depth of
/// whatever else happens to hold hover or capture.
///
/// The interaction checks used to run inside the recursion, so a pointer resting
/// on a node *outside* the removed subtree paid for a full walk of its own
/// ancestor path once per freed node, purely to answer "no". Closing a
/// ten-thousand-row panel is where that shows up.
#[test]
fn removing_a_large_subtree_is_linear_in_the_subtree() {
    const ROWS: usize = 10_000;

    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Vertical,
            ..Flex::default()
        },
        LogicalSize::new(400.0, 300.0),
    );
    let root = ui.root();
    let elsewhere = ui
        .append(
            root,
            Flex {
                axis: Axis::Vertical,
                ..Flex::default()
            },
        )
        .id();
    let held = boxes(&mut ui, elsewhere, 1)[0];
    let panel = ui
        .append(
            root,
            Flex {
                axis: Axis::Vertical,
                ..Flex::default()
            },
        )
        .id();
    boxes(&mut ui, panel, ROWS);
    ui.update_passes();

    // Hover lands on the box in the *other* subtree, which is what makes every
    // membership question a walk that fails.
    ui.dispatch(UiInput::PointerMoved(LogicalPoint::new(2.0, 2.0)));
    assert_eq!(ui.hit_test(LogicalPoint::new(2.0, 2.0)), Some(held));
    ui.update_passes();

    ui.remove(panel);
    let stats = ui.update_passes().stats;
    assert!(
        stats.invalidate_steps < 16,
        "removal walked {} ancestor links for {ROWS} rows",
        stats.invalidate_steps
    );
    assert!(ui.contains(held));
    assert_eq!(ui.semantic_snapshot().len(), 1);
}

#[test]
fn hit_test_counter_survives_the_pass_that_follows_it() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    ui.append(
        ui.root(),
        Probe::new("probe", Some(LogicalSize::new(100.0, 50.0))),
    );
    ui.update_passes();

    ui.dispatch(UiInput::PointerMoved(LogicalPoint::new(20.0, 10.0)));
    let dispatched = ui.stats().hit_test_nodes;
    assert_eq!(dispatched, 2, "the root plus the probe were traversed");

    // Hit testing happens during dispatch, so a consumer that reads stats after
    // the frame -- which is the only point a host has them -- used to see this
    // counter zeroed by `update_passes`.
    let stats = ui.update_passes().stats;
    assert_eq!(stats.hit_test_nodes, dispatched);
    assert_eq!(ui.stats().hit_test_nodes, dispatched);
}

#[test]
fn hit_testing_rejects_wide_subtrees_before_visiting_their_children() {
    // Keep this arithmetic and target synchronized with `benches/incremental.rs`.
    const PANELS: usize = 10;
    const REGULAR_LEAVES: usize = 99;
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Horizontal,
            ..Flex::default()
        },
        LogicalSize::new(100.0, 500.0),
    );
    let root = ui.root();
    for panel_index in 0..PANELS {
        let panel = ui.append(
            root,
            Flex {
                axis: Axis::Vertical,
                ..Flex::default()
            },
        );
        // 1 root + 10 panels + (9 * 99 + 98) leaves = 1,000 nodes.
        let leaves = if panel_index + 1 == PANELS {
            REGULAR_LEAVES - 1
        } else {
            REGULAR_LEAVES
        };
        for _ in 0..leaves {
            let mut leaf = BoxElement::new(LogicalSize::new(10.0, 5.0), Color::WHITE);
            leaf.interactive = true;
            ui.append(panel.id(), leaf);
        }
    }
    ui.update_passes();

    assert!(ui.hit_test(LogicalPoint::new(5.0, 492.5)).is_some());
    assert_eq!(
        ui.stats().hit_test_nodes,
        12,
        "the root, nine rejected panels, containing panel, and target leaf are visited"
    );
}
