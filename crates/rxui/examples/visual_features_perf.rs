//! Headless performance budget for chart decimation and node-graph updates.
//!
//! Run `cargo run --release -p rxui --example visual_features_perf -- --check`
//! to enforce the 16 ms average interaction budget.

use std::time::Instant;

use astrelis_core::geometry::Size;
use astrelis_text::FontDatabase;
use rxui::prelude::*;

const ITERATIONS: usize = 100;
const BUDGET_MS: f64 = 16.0;

#[derive(Clone, Debug)]
#[allow(dead_code)]
enum Message {
    Chart(ChartAction),
    Graph(NodeGraphAction<u64>),
}

fn main() -> rxui::Result<()> {
    let check = std::env::args().any(|argument| argument == "--check");
    let mut ui = Ui::new(FontDatabase::default(), Theme::dark());
    ui.set_viewport(Size::new(1_280.0, 720.0), 1.0);
    let root = ui.root();
    let row = ui.add_row(root)?;
    ui.set_layout(
        row,
        LayoutStyle {
            width: Length::Percent(1.0),
            height: Length::Percent(1.0),
            ..Default::default()
        },
    )?;

    let points = (0..100_000)
        .map(|index| {
            ChartPoint::new(
                index as f64,
                (index as f64 * 0.001).sin() * 40.0 + (index % 97) as f64 * 0.02,
            )
        })
        .collect();
    let series = vec![ChartSeries::new(
        "signal",
        "Signal",
        ChartSeriesKind::Line,
        points,
    )];
    let chart = ui.add_widget(
        row,
        ChartView::new(series.clone(), ChartOptions::default(), Message::Chart)?,
    )?;
    ui.set_layout(
        chart,
        LayoutStyle {
            grow: 1.0,
            min_width: Length::Px(0.0),
            height: Length::Percent(1.0),
            ..Default::default()
        },
    )?;

    let mut document = large_graph();
    let graph = ui.add_widget(
        row,
        NodeGraphView::new(
            document.clone(),
            NodeGraphOptions::default(),
            Message::Graph,
        )?,
    )?;
    ui.set_layout(
        graph,
        LayoutStyle {
            grow: 1.0,
            min_width: Length::Px(0.0),
            height: Length::Percent(1.0),
            ..Default::default()
        },
    )?;
    ui.display_list()?;

    let fitted = ChartViewport::fit(&series);
    let started = Instant::now();
    for iteration in 0..ITERATIONS {
        let span = fitted.x_max - fitted.x_min;
        let shift = span * iteration as f64 * 0.0001;
        let viewport = ChartViewport::new(
            fitted.x_min + shift,
            fitted.x_max + shift,
            fitted.y_min,
            fitted.y_max,
        )?;
        let mut chart_result = Ok(());
        ui.update_widget(chart, |view| chart_result = view.set_viewport(viewport))?;
        chart_result?;

        let node_index = iteration % document.nodes.len();
        document.nodes[node_index].position.x += 1.0;
        let mut graph_result = Ok(());
        ui.update_widget(graph, |view| {
            graph_result = view.sync(document.clone(), NodeGraphSelection::default())
        })?;
        graph_result?;
        ui.display_list()?;
    }
    let elapsed = started.elapsed();
    let average_ms = elapsed.as_secs_f64() * 1_000.0 / ITERATIONS as f64;
    println!(
        "visual features: {ITERATIONS} interactions, {average_ms:.3} ms average, {:.3} ms total",
        elapsed.as_secs_f64() * 1_000.0
    );
    if check && average_ms > BUDGET_MS {
        return Err(rxui::Error::msg(format!(
            "visual features averaged {average_ms:.3} ms, exceeding the {BUDGET_MS:.3} ms budget"
        )));
    }
    Ok(())
}

fn large_graph() -> NodeGraphDocument<u64> {
    let nodes = (0..250_u64)
        .map(|id| GraphNode {
            id,
            title: format!("Node {id}"),
            position: GraphPoint::new((id % 20) as f32 * 180.0, (id / 20) as f32 * 120.0),
            size: GraphSize::new(140.0, 88.0),
            ports: vec![
                GraphPort {
                    id: 1,
                    label: "input".into(),
                    direction: GraphPortDirection::Input,
                },
                GraphPort {
                    id: 2,
                    label: "output".into(),
                    direction: GraphPortDirection::Output,
                },
            ],
        })
        .collect::<Vec<_>>();
    let edges = (0..500_u64)
        .map(|id| {
            let from = id % 250;
            let to = (from + 1) % 250;
            GraphEdge {
                id: 10_000 + id,
                from: GraphEndpoint {
                    node: from,
                    port: 2,
                },
                to: GraphEndpoint { node: to, port: 1 },
            }
        })
        .collect();
    NodeGraphDocument {
        format_version: 1,
        nodes,
        edges,
        viewport: GraphViewport::default(),
    }
}
