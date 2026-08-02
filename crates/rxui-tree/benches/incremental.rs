//! Incremental retained-core microbenchmarks.

use astrelis_core::{color::Color, geometry::LogicalSize};
use criterion::{Criterion, criterion_group, criterion_main};
use rxui_tree::{Axis, Flex, Label, UiTree};
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
}

criterion_group!(benches, incremental);
criterion_main!(benches);
