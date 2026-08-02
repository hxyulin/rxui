//! Incremental retained-core microbenchmarks.

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalSize},
};
use criterion::{Criterion, criterion_group, criterion_main};
use rxui_tree::{Axis, BoxElement, Flex, Label, UiTree};
use std::hint::black_box;

fn ui(count: usize) -> (UiTree, rxui_tree::NodeHandle<Label>) {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Vertical,
            ..Flex::default()
        },
        LogicalSize::new(1280.0, 720.0),
    );
    let root = ui.root();
    let mut target = None;
    for index in 0..count {
        let handle = ui.append(
            root,
            Label::new(format!("Item {index}")).with_color(Color::WHITE),
        );
        if index == count / 2 {
            target = Some(handle);
        }
    }
    ui.update_passes();
    (ui, target.unwrap())
}

fn incremental(criterion: &mut Criterion) {
    let (mut ui, target) = ui(1_000);
    let mut flip = false;
    criterion.bench_function("rxui_tree/label_update_1000", |bencher| {
        bencher.iter(|| {
            flip = !flip;
            ui.label_mut(target)
                .set_text(if flip { "Item flip" } else { "Item flop" });
            black_box(ui.update_passes().stats);
        });
    });

    let mut hit_tree = hit_test_tree();
    let point = LogicalPoint::new(5.0, 492.5);
    criterion.bench_function("rxui_tree/pointer_hit_test_pruned_1000", |bencher| {
        bencher.iter(|| black_box(hit_tree.hit_test(black_box(point))));
    });
}

/// Builds exactly 1,000 nodes: a root, ten spatially disjoint panels, and 989
/// leaves. The target is in the first panel, so reverse paint order visits and
/// rejects all nine later panels by subtree bounds before descending into it.
///
/// Keep this arithmetic and target synchronized with the pruning assertion in
/// `tests/invalidation.rs`.
fn hit_test_tree() -> UiTree {
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
    ui
}

criterion_group!(benches, incremental);
criterion_main!(benches);
