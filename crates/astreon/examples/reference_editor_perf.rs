//! Headless performance reproduction for the reference editor's retained UI.
//!
//! Run with `cargo run -p astreon --example reference_editor_perf --locked`.

use std::{collections::BTreeMap, io, time::Instant};

use astrelis_core::geometry::Size;
use astrelis_text::FontDatabase;
use astrelis_ui_core::{LayoutStyle, Length, SemanticAction, SemanticRole, Theme, Ui};
use astreon::editor::docking::{
    DockAction, DockAxis, DockLayout, DockNode, DockStyle, DockTabs, DockWorkspace,
    PanelDescriptor, PanelId,
};
use astreon::prelude::*;

const ITERATIONS: usize = 500;

#[derive(Clone, Debug)]
#[allow(dead_code)]
enum Message {
    Dock(DockAction),
    Tree(TreeAction<u64>),
    Table(TableAction<u64, &'static str>),
    Property(PropertyAction<u8>),
}

fn panel(value: &str) -> PanelId {
    PanelId::new(value).expect("static panel id is valid")
}

fn layout() -> DockLayout {
    DockLayout {
        root: Some(DockNode::Split {
            axis: DockAxis::Horizontal,
            ratio: 0.2,
            first: Box::new(DockNode::Tabs(
                DockTabs::new(vec![panel("hierarchy")]).unwrap(),
            )),
            second: Box::new(DockNode::Split {
                axis: DockAxis::Horizontal,
                ratio: 0.75,
                first: Box::new(DockNode::Split {
                    axis: DockAxis::Vertical,
                    ratio: 0.72,
                    first: Box::new(DockNode::Tabs(DockTabs::new(vec![panel("scene")]).unwrap())),
                    second: Box::new(DockNode::Tabs(
                        DockTabs::new(vec![panel("entities")]).unwrap(),
                    )),
                }),
                second: Box::new(DockNode::Tabs(
                    DockTabs::new(vec![panel("inspector")]).unwrap(),
                )),
            }),
        }),
        floating: Vec::new(),
    }
}

fn main() -> Result<(), io::Error> {
    astrelis_profiling::init();
    let mut ui = Ui::new(FontDatabase::default(), Theme::dark());
    ui.set_viewport(Size::new(1_200.0, 760.0), 1.0);
    let root = ui.root();
    ui.set_layout(
        root,
        LayoutStyle {
            width: Length::Percent(1.0),
            height: Length::Percent(1.0),
            ..Default::default()
        },
    )
    .map_err(io::Error::other)?;

    let dock_host = ui.add_column(root).map_err(io::Error::other)?;
    ui.set_layout(
        dock_host,
        LayoutStyle {
            grow: 1.0,
            ..Default::default()
        },
    )
    .map_err(io::Error::other)?;

    let hierarchy_panel = ui.add_column(root).map_err(io::Error::other)?;
    let mut tree =
        TreeView::new(&mut ui, hierarchy_panel, Message::Tree).map_err(io::Error::other)?;
    ui.set_layout(
        tree.root(),
        LayoutStyle {
            grow: 1.0,
            ..Default::default()
        },
    )
    .map_err(io::Error::other)?;

    let table_panel = ui.add_column(root).map_err(io::Error::other)?;
    let mut table =
        TableView::new(&mut ui, table_panel, Message::Table).map_err(io::Error::other)?;
    ui.set_layout(
        table.root(),
        LayoutStyle {
            grow: 1.0,
            ..Default::default()
        },
    )
    .map_err(io::Error::other)?;

    let inspector_panel = ui.add_column(root).map_err(io::Error::other)?;
    let mut properties =
        PropertyGrid::new(&mut ui, inspector_panel, Message::Property).map_err(io::Error::other)?;
    let scene_panel = ui.add_column(root).map_err(io::Error::other)?;
    ui.add_label(scene_panel, "Interactive 2D scene")
        .map_err(io::Error::other)?;

    let mut workspace = DockWorkspace::new(
        &mut ui,
        dock_host,
        DockStyle {
            divider_size: 4.0,
            divider_visual_size: 1.0,
            ..DockStyle::default()
        },
        Message::Dock,
    )
    .map_err(io::Error::other)?;
    for (id, title, content) in [
        (panel("hierarchy"), "Hierarchy", hierarchy_panel),
        (panel("scene"), "Scene", scene_panel),
        (panel("inspector"), "Inspector", inspector_panel),
        (panel("entities"), "Entities", table_panel),
    ] {
        workspace
            .register_panel(&mut ui, PanelDescriptor::new(id, title), content)
            .map_err(io::Error::other)?;
    }
    let default_layout = layout();
    workspace
        .restore(&mut ui, default_layout.clone(), default_layout)
        .map_err(io::Error::other)?;

    let nodes = vec![
        TreeNode::leaf(1, "Camera"),
        TreeNode::leaf(2, "World")
            .expanded(true)
            .children(vec![TreeNode::leaf(3, "Key Light")]),
    ];
    tree.sync(&mut ui, &nodes, Some(&2))
        .map_err(io::Error::other)?;
    let mut columns = vec![
        TableColumn::new("name", "Name", 180.0),
        TableColumn::new("kind", "Kind", 100.0),
        TableColumn::new("visible", "Visible", 80.0),
    ];
    let rows = vec![
        TableRow {
            id: 1,
            cells: vec!["Camera".into(), "Camera".into(), "Yes".into()],
        },
        TableRow {
            id: 2,
            cells: vec!["World".into(), "Rectangle".into(), "Yes".into()],
        },
        TableRow {
            id: 3,
            cells: vec!["Key Light".into(), "Light".into(), "Yes".into()],
        },
    ];
    table
        .sync(&mut ui, &columns, &rows, None, Some(&2))
        .map_err(io::Error::other)?;
    properties
        .sync(
            &mut ui,
            &[PropertySection {
                id: 0,
                title: "Transform".into(),
                expanded: true,
                fields: vec![
                    PropertyField {
                        id: 1,
                        label: "X".into(),
                        value: PropertyValue::Number {
                            value: 0.0,
                            options: NumericFieldOptions::default(),
                        },
                        validation: ValidationResult::valid(),
                        enabled: true,
                    },
                    PropertyField {
                        id: 2,
                        label: "Y".into(),
                        value: PropertyValue::Number {
                            value: 0.0,
                            options: NumericFieldOptions::default(),
                        },
                        validation: ValidationResult::valid(),
                        enabled: true,
                    },
                ],
            }],
        )
        .map_err(io::Error::other)?;

    ui.display_list().map_err(io::Error::other)?;
    let separator = find_first_separator(&mut ui)?;
    astrelis_profiling::new_frame();
    let started = Instant::now();
    for index in 0..ITERATIONS {
        let ratio = if index % 2 == 0 { 0.35 } else { 0.65 };
        ui.perform_semantic_action(separator, SemanticAction::SetValue(ratio))
            .map_err(io::Error::other)?;
        for message in ui.drain_messages().collect::<Vec<_>>() {
            if let Message::Dock(action) = message {
                workspace.apply(&mut ui, action).map_err(io::Error::other)?;
            }
        }
        ui.display_list().map_err(io::Error::other)?;
    }
    let elapsed = started.elapsed();
    astrelis_profiling::new_frame();

    println!(
        "headless split: {ITERATIONS} iterations, {:.3} ms average, {:.3} ms total",
        elapsed.as_secs_f64() * 1_000.0 / ITERATIONS as f64,
        elapsed.as_secs_f64() * 1_000.0,
    );

    let started = Instant::now();
    for index in 0..ITERATIONS {
        let selected = if index % 2 == 0 { 1 } else { 2 };
        tree.sync(&mut ui, &nodes, Some(&selected))
            .map_err(io::Error::other)?;
        table
            .sync(&mut ui, &columns, &rows, None, Some(&selected))
            .map_err(io::Error::other)?;
        properties
            .sync(
                &mut ui,
                &[PropertySection {
                    id: 0,
                    title: "Transform".into(),
                    expanded: true,
                    fields: vec![PropertyField {
                        id: 1,
                        label: "X".into(),
                        value: PropertyValue::Number {
                            value: selected as f64,
                            options: NumericFieldOptions::default(),
                        },
                        validation: ValidationResult::valid(),
                        enabled: true,
                    }],
                }],
            )
            .map_err(io::Error::other)?;
        ui.display_list().map_err(io::Error::other)?;
    }
    let elapsed = started.elapsed();
    println!(
        "headless selection: {ITERATIONS} iterations, {:.3} ms average, {:.3} ms total",
        elapsed.as_secs_f64() * 1_000.0 / ITERATIONS as f64,
        elapsed.as_secs_f64() * 1_000.0,
    );

    let started = Instant::now();
    for index in 0..ITERATIONS {
        columns[0].width = if index % 2 == 0 { 160.0 } else { 220.0 };
        table
            .sync(&mut ui, &columns, &rows, None, Some(&2))
            .map_err(io::Error::other)?;
        ui.display_list().map_err(io::Error::other)?;
    }
    let elapsed = started.elapsed();
    println!(
        "headless table resize: {ITERATIONS} iterations, {:.3} ms average, {:.3} ms total",
        elapsed.as_secs_f64() * 1_000.0 / ITERATIONS as f64,
        elapsed.as_secs_f64() * 1_000.0,
    );

    astrelis_profiling::new_frame();
    print_profile_summary();
    Ok(())
}

fn find_first_separator(ui: &mut Ui<Message>) -> Result<astrelis_ui_core::ElementId, io::Error> {
    fn visit(node: &astrelis_ui_core::SemanticNode) -> Option<astrelis_ui_core::ElementId> {
        if node.role == SemanticRole::Separator && node.label == "Resize panes" {
            return Some(node.id);
        }
        node.children.iter().find_map(visit)
    }
    let tree = ui.semantic_tree().map_err(io::Error::other)?;
    visit(&tree).ok_or_else(|| io::Error::other("dock separator was not realized"))
}

fn print_profile_summary() {
    let profiler = astrelis_profiling::Profiler::get();
    let timeline = profiler.timeline.read().expect("profiler timeline lock");
    let mut totals = BTreeMap::<String, (usize, u64)>::new();
    for stream in timeline.thread_streams.values() {
        for span in &stream.spans {
            let scope = &timeline.scopes[span.scope.0.get() as usize - 1];
            let name = profiler.strings.get(scope.name).unwrap_or_default();
            let total = totals.entry(name).or_default();
            total.0 += 1;
            total.1 += span.end_ns.saturating_sub(span.start_ns);
        }
    }
    for (name, (count, nanoseconds)) in totals {
        println!(
            "  {name}: count={count}, avg={:.3} ms, total={:.3} ms",
            nanoseconds as f64 / count as f64 / 1_000_000.0,
            nanoseconds as f64 / 1_000_000.0,
        );
    }
}
