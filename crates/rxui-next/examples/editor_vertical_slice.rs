//! Headless editor workload for comparing RXUI Next with current RXUI.

use std::time::Instant;

use astrelis_core::geometry::LogicalSize;
use rxui_next::{
    Component, ComponentContext, ComponentHost, PropertyField, TableRow, Theme, TreeRow, column,
    editable_property_grid, render_view, virtual_table_with_widths, virtual_tree,
};

const ITERATIONS: usize = 500;
const WARMUP: usize = 50;

#[derive(Clone)]
enum Action {
    Select(u64),
    Inspector(InspectorAction),
    ResizeTable(f32),
}

#[derive(Clone)]
enum InspectorAction {
    SetProperty(u64, String),
}

struct Editor {
    selected: u64,
    fields: Vec<PropertyField>,
    tree: Vec<TreeRow>,
    table: Vec<TableRow>,
    table_widths: [f32; 3],
}

impl Component for Editor {
    type Action = Action;
    type Effect = ();

    fn update(&mut self, action: Action, _context: &mut ComponentContext<'_, ()>) {
        match action {
            Action::Select(id) => {
                self.selected = id;
                self.fields[0].value = id.to_string();
            }
            Action::Inspector(InspectorAction::SetProperty(id, value)) => {
                if let Some(field) = self.fields.iter_mut().find(|field| field.id == id) {
                    field.value = value;
                }
            }
            Action::ResizeTable(width) => self.table_widths[0] = width,
        }
    }

    fn view(&self, _theme: &Theme) -> rxui_next::AnyView<Action> {
        column(vec![
            virtual_tree(&self.tree, 0..40, Some(self.selected)).keyed("tree"),
            virtual_table_with_widths(&self.table, 0..30, Some(self.selected), &self.table_widths)
                .keyed("table"),
            editable_property_grid(&self.fields, InspectorAction::SetProperty)
                .map_action(Action::Inspector)
                .keyed("properties"),
            render_view("Scene").keyed("scene"),
        ])
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let editor = Editor {
        selected: 0,
        fields: (0..100)
            .map(|id| PropertyField {
                id,
                label: format!("Property {id}"),
                value: id.to_string(),
            })
            .collect(),
        tree: (0..10_000)
            .map(|id| TreeRow {
                id,
                depth: (id % 4) as usize,
                label: format!("Node {id}"),
            })
            .collect(),
        table: (0..10_000)
            .map(|id| TableRow {
                id,
                cells: vec![format!("Entity {id}"), "Mesh".into(), "Visible".into()],
            })
            .collect(),
        table_widths: [180.0, 100.0, 80.0],
    };
    let mut host = ComponentHost::new(editor, LogicalSize::new(1_280.0, 900.0), Theme::dark())?;
    for iteration in 0..WARMUP {
        host.dispatch(Action::Select((iteration % 30) as u64))?;
    }
    let started = Instant::now();
    let mut rebuilt = 0usize;
    let mut laid_out = 0usize;
    let mut shaped = 0usize;
    for iteration in 0..ITERATIONS {
        let stats = host
            .dispatch(Action::Select((iteration % 30) as u64))?
            .stats;
        rebuilt += stats.rebuilt_fragments;
        laid_out += stats.layout_elements;
        shaped += stats.shaped_text;
    }
    let elapsed = started.elapsed();
    println!(
        "rxui-next selection: {ITERATIONS} updates, {:.3} ms average, {:.2} fragments, {:.2} layout elements, {:.2} text shapes/update",
        elapsed.as_secs_f64() * 1_000.0 / ITERATIONS as f64,
        rebuilt as f64 / ITERATIONS as f64,
        laid_out as f64 / ITERATIONS as f64,
        shaped as f64 / ITERATIONS as f64,
    );

    for iteration in 0..WARMUP {
        host.dispatch(Action::ResizeTable(if iteration % 2 == 0 {
            160.0
        } else {
            220.0
        }))?;
    }
    let started = Instant::now();
    rebuilt = 0;
    laid_out = 0;
    shaped = 0;
    for iteration in 0..ITERATIONS {
        let stats = host
            .dispatch(Action::ResizeTable(if iteration % 2 == 0 {
                160.0
            } else {
                220.0
            }))?
            .stats;
        rebuilt += stats.rebuilt_fragments;
        laid_out += stats.layout_elements;
        shaped += stats.shaped_text;
    }
    let elapsed = started.elapsed();
    println!(
        "rxui-next table resize: {ITERATIONS} updates, {:.3} ms average, {:.2} fragments, {:.2} layout elements, {:.2} text shapes/update",
        elapsed.as_secs_f64() * 1_000.0 / ITERATIONS as f64,
        rebuilt as f64 / ITERATIONS as f64,
        laid_out as f64 / ITERATIONS as f64,
        shaped as f64 / ITERATIONS as f64,
    );

    for iteration in 0..WARMUP {
        host.dispatch(Action::Inspector(InspectorAction::SetProperty(
            0,
            iteration.to_string(),
        )))?;
    }
    let started = Instant::now();
    rebuilt = 0;
    laid_out = 0;
    shaped = 0;
    for iteration in 0..ITERATIONS {
        let stats = host
            .dispatch(Action::Inspector(InspectorAction::SetProperty(
                0,
                iteration.to_string(),
            )))?
            .stats;
        rebuilt += stats.rebuilt_fragments;
        laid_out += stats.layout_elements;
        shaped += stats.shaped_text;
    }
    let elapsed = started.elapsed();
    println!(
        "rxui-next property edit: {ITERATIONS} updates, {:.3} ms average, {:.2} fragments, {:.2} layout elements, {:.2} text shapes/update",
        elapsed.as_secs_f64() * 1_000.0 / ITERATIONS as f64,
        rebuilt as f64 / ITERATIONS as f64,
        laid_out as f64 / ITERATIONS as f64,
        shaped as f64 / ITERATIONS as f64,
    );
    Ok(())
}
