//! Reconciliation contracts for the optional retained widget specifications.

#![cfg(any(feature = "charts", feature = "graph"))]

#[cfg(feature = "charts")]
use astrelis_core::color::Color;
use astrelis_core::geometry::{LogicalPoint, LogicalSize};
use rxui_core::{Context, Element, EntityHarness, Render};
use rxui_tree::PassStats;

#[cfg(feature = "charts")]
use rxui_widgets::element::chart::{
    ChartAction, ChartOptions, ChartPoint, ChartSeries, ChartSeriesKind, ChartSpec, chart,
};
#[cfg(feature = "graph")]
use rxui_widgets::element::graph::{
    GraphEdge, GraphNode, GraphViewport, NodeGraphAction, NodeGraphSpec, node_graph,
};

fn assert_no_retained_work(what: &str, stats: PassStats) {
    assert_eq!(
        stats,
        PassStats {
            reused_fragments: 3,
            ..PassStats::default()
        },
        "{what}"
    );
}

fn paint_stats(accessibility: bool) -> PassStats {
    PassStats {
        rebuilt_fragments: 1,
        reused_fragments: 2,
        accessibility_nodes: usize::from(accessibility),
        visited_accessibility_nodes: if accessibility { 3 } else { 0 },
        invalidate_steps: 2,
        ..PassStats::default()
    }
}

fn input_only_stats() -> PassStats {
    PassStats {
        reused_fragments: 3,
        hit_test_nodes: 3,
        ..PassStats::default()
    }
}

#[cfg(feature = "charts")]
fn series(id: u64, kind: ChartSeriesKind, count: usize) -> ChartSeries {
    ChartSeries {
        id,
        name: format!("Series {id}"),
        color: Color::from_hex(0x4c8dff),
        kind,
        points: (0..count)
            .map(|index| ChartPoint {
                x: index as f64,
                y: ((index * 7) % 19) as f64,
            })
            .collect(),
    }
}

#[cfg(feature = "charts")]
#[derive(Clone)]
struct ChartScene {
    series: Vec<ChartSeries>,
    options: ChartOptions,
    selection: Option<(u64, usize)>,
}

#[cfg(feature = "charts")]
impl ChartScene {
    fn new() -> Self {
        Self {
            series: vec![
                series(1, ChartSeriesKind::Line, 24),
                series(2, ChartSeriesKind::Scatter, 8),
            ],
            options: ChartOptions::default(),
            selection: None,
        }
    }
}

#[cfg(feature = "charts")]
impl Render for ChartScene {
    fn render(&mut self, cx: &mut Context<'_, Self>) -> Element {
        chart(
            ChartSpec::new(
                self.series.clone(),
                cx.listener_value(|this, action, _| {
                    this.selection = match action {
                        ChartAction::Select { series, point } => Some((series, point)),
                        ChartAction::Clear => None,
                    };
                }),
            )
            .options(self.options),
        )
    }
}

#[cfg(feature = "charts")]
#[test]
fn refreshing_a_chart_with_an_identical_spec_does_no_retained_work() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| ChartScene::new()));
    harness.refresh();
    assert_no_retained_work("ChartSpec", harness.stats().passes);
}

#[cfg(feature = "charts")]
#[test]
fn a_changed_chart_option_still_reaches_the_retained_element() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| ChartScene::new()));
    harness.mutate(|scene| scene.options.size = LogicalSize::new(320.0, 200.0));
    assert_eq!(
        harness.find("Chart").bounds.size,
        LogicalSize::new(320.0, 200.0)
    );
    assert_eq!(
        harness.stats().passes,
        PassStats {
            layout_elements: 4,
            composed_nodes: 2,
            rebuilt_fragments: 2,
            reused_fragments: 1,
            accessibility_nodes: 1,
            visited_compose_nodes: 3,
            visited_accessibility_nodes: 3,
            invalidate_steps: 2,
            ..PassStats::default()
        }
    );
}

#[cfg(feature = "charts")]
#[test]
fn a_chart_background_change_repaints_without_re_running_layout() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| ChartScene::new()));
    harness.mutate(|scene| scene.options.background = Color::from_hex(0x2a2f3a));
    assert_eq!(harness.stats().passes, paint_stats(false));
}

#[cfg(feature = "charts")]
#[test]
fn a_chart_data_change_repaints_and_announces_the_new_series_count() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| ChartScene::new()));
    harness.mutate(|scene| scene.series.push(series(3, ChartSeriesKind::Line, 6)));
    assert_eq!(
        harness.find("Chart").data.value.as_deref(),
        Some("3 series")
    );
    assert_eq!(harness.stats().passes, paint_stats(true));
}

#[cfg(feature = "charts")]
#[test]
fn chart_hover_repaints_without_layout_or_reshaping() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| ChartScene::new()));
    let bounds = harness.find("Chart").bounds;
    let origin = LogicalPoint::new(
        bounds.origin.x + 12.0,
        bounds.origin.y + bounds.size.height - 12.0,
    );
    harness.hover_at(origin);
    assert_eq!(
        harness.stats().passes,
        PassStats {
            hit_test_nodes: 3,
            ..paint_stats(false)
        }
    );
    harness.hover_at(LogicalPoint::new(origin.x + 1.0, origin.y - 1.0));
    assert_eq!(harness.stats().passes, input_only_stats());
}

#[cfg(feature = "charts")]
#[test]
fn a_chart_release_reports_the_nearest_point_to_the_entity_handler() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| ChartScene::new()));
    let bounds = harness.find("Chart").bounds;
    harness.release_pointer_at(LogicalPoint::new(
        bounds.origin.x + 12.0,
        bounds.origin.y + bounds.size.height - 12.0,
    ));
    assert_eq!(harness.root().read(harness.app()).selection, Some((1, 0)));
    assert_eq!(harness.stats().passes, input_only_stats());
    harness.release_pointer_at(LogicalPoint::new(
        bounds.origin.x + bounds.size.width * 0.5,
        bounds.origin.y + bounds.size.height * 0.5,
    ));
    assert_eq!(harness.root().read(harness.app()).selection, None);
    assert_eq!(harness.stats().passes, input_only_stats());
}

#[cfg(feature = "graph")]
#[derive(Clone)]
struct GraphScene {
    nodes: Vec<GraphNode<u64>>,
    edges: Vec<GraphEdge<u64>>,
    viewport: GraphViewport,
    selected: Option<u64>,
}

#[cfg(feature = "graph")]
impl GraphScene {
    fn new() -> Self {
        Self {
            nodes: vec![
                GraphNode {
                    id: 1,
                    title: "Input".into(),
                    position: LogicalPoint::new(20.0, 20.0),
                    size: LogicalSize::new(120.0, 60.0),
                },
                GraphNode {
                    id: 2,
                    title: "Transform".into(),
                    position: LogicalPoint::new(200.0, 120.0),
                    size: LogicalSize::new(140.0, 60.0),
                },
                GraphNode {
                    id: 3,
                    title: "Output".into(),
                    position: LogicalPoint::new(400.0, 40.0),
                    size: LogicalSize::new(120.0, 60.0),
                },
            ],
            edges: vec![GraphEdge { from: 1, to: 2 }, GraphEdge { from: 2, to: 3 }],
            viewport: GraphViewport::default(),
            selected: None,
        }
    }

    fn wide() -> Self {
        Self {
            nodes: ["Load", "Decode", "Resample", "Mix", "Encode"]
                .into_iter()
                .enumerate()
                .map(|(index, title)| GraphNode {
                    id: index as u64 + 1,
                    title: title.into(),
                    position: LogicalPoint::new(
                        20.0 + index as f32 * 115.0,
                        30.0 + (index % 2) as f32 * 90.0,
                    ),
                    size: LogicalSize::new(100.0, 50.0),
                })
                .collect(),
            edges: Vec::new(),
            viewport: GraphViewport::default(),
            selected: None,
        }
    }
}

#[cfg(feature = "graph")]
impl Render for GraphScene {
    fn render(&mut self, cx: &mut Context<'_, Self>) -> Element {
        let mut spec = NodeGraphSpec::new(
            self.nodes.clone(),
            self.edges.clone(),
            cx.listener_value(|this, action, _| {
                this.selected = match action {
                    NodeGraphAction::Select(id) => Some(id),
                    NodeGraphAction::ClearSelection => None,
                };
            }),
        );
        spec.viewport = self.viewport;
        spec.selected = self.selected;
        node_graph(spec)
    }
}

#[cfg(feature = "graph")]
#[test]
fn refreshing_a_node_graph_with_an_identical_spec_does_no_retained_work() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| GraphScene::new()));
    harness.refresh();
    assert_no_retained_work("NodeGraphSpec", harness.stats().passes);
}

#[cfg(feature = "graph")]
#[test]
fn panning_a_node_graph_reshapes_no_node_titles() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| GraphScene::new()));
    harness.mutate(|scene| scene.viewport.pan = LogicalPoint::new(24.0, 0.0));
    assert_eq!(harness.stats().passes, paint_stats(false));
}

#[cfg(feature = "graph")]
#[test]
fn zooming_a_node_graph_reshapes_no_node_titles() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| GraphScene::new()));
    harness.mutate(|scene| scene.viewport.zoom = 2.0);
    assert_eq!(harness.stats().passes, paint_stats(false));
}

#[cfg(feature = "graph")]
#[test]
fn selecting_a_graph_node_repaints_without_layout_or_a_new_announcement() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| GraphScene::new()));
    let announced = harness.find("Node graph").data.value;
    harness.mutate(|scene| scene.selected = Some(2));
    assert_eq!(harness.find("Node graph").data.value, announced);
    assert_eq!(harness.stats().passes, paint_stats(false));
}

#[cfg(feature = "graph")]
#[test]
fn moving_a_graph_node_repaints_without_reshaping_its_title() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| GraphScene::new()));
    harness.mutate(|scene| scene.nodes[1].position = LogicalPoint::new(240.0, 60.0));
    assert_eq!(harness.stats().passes, paint_stats(false));
}

#[cfg(feature = "graph")]
#[test]
fn changing_only_the_edges_repaints_and_announces_the_new_count() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| GraphScene::new()));
    harness.mutate(|scene| scene.edges.push(GraphEdge { from: 1, to: 3 }));
    assert_eq!(
        harness.find("Node graph").data.value.as_deref(),
        Some("3 nodes, 3 edges")
    );
    assert_eq!(harness.stats().passes, paint_stats(true));
}

#[cfg(feature = "graph")]
#[test]
fn renaming_one_node_of_five_reshapes_only_that_title() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| GraphScene::wide()));
    harness.mutate(|scene| scene.nodes[3].title = "Crossfade".into());
    assert_eq!(
        harness.stats().passes,
        PassStats {
            layout_elements: 4,
            rebuilt_fragments: 1,
            reused_fragments: 2,
            shaped_text: 1,
            visited_compose_nodes: 3,
            visited_accessibility_nodes: 3,
            invalidate_steps: 2,
            ..PassStats::default()
        }
    );
}

#[cfg(feature = "graph")]
#[test]
fn adding_a_graph_node_shapes_only_the_new_title() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| GraphScene::wide()));
    harness.mutate(|scene| {
        scene.nodes.push(GraphNode {
            id: 6,
            title: "Normalize".into(),
            position: LogicalPoint::new(600.0, 30.0),
            size: LogicalSize::new(100.0, 50.0),
        })
    });
    assert_eq!(
        harness.find("Node graph").data.value.as_deref(),
        Some("6 nodes, 0 edges")
    );
    assert_eq!(
        harness.stats().passes,
        PassStats {
            layout_elements: 4,
            rebuilt_fragments: 1,
            reused_fragments: 2,
            accessibility_nodes: 1,
            shaped_text: 1,
            visited_compose_nodes: 3,
            visited_accessibility_nodes: 3,
            invalidate_steps: 2,
            ..PassStats::default()
        }
    );
}

#[cfg(feature = "graph")]
#[test]
fn removing_a_graph_node_reshapes_nothing_and_drops_its_memo_entry() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| GraphScene::wide()));
    let removed = harness.root().read(harness.app()).nodes[1].clone();
    harness.mutate(|scene| {
        scene.nodes.remove(1);
    });
    assert_eq!(
        harness.find("Node graph").data.value.as_deref(),
        Some("4 nodes, 0 edges")
    );
    assert_eq!(
        harness.stats().passes,
        PassStats {
            layout_elements: 4,
            rebuilt_fragments: 1,
            reused_fragments: 2,
            accessibility_nodes: 1,
            visited_compose_nodes: 3,
            visited_accessibility_nodes: 3,
            invalidate_steps: 2,
            ..PassStats::default()
        }
    );
    harness.mutate(|scene| scene.nodes.insert(1, removed));
    assert_eq!(
        harness.stats().passes,
        PassStats {
            layout_elements: 4,
            rebuilt_fragments: 1,
            reused_fragments: 2,
            accessibility_nodes: 1,
            shaped_text: 1,
            visited_compose_nodes: 3,
            visited_accessibility_nodes: 3,
            invalidate_steps: 2,
            ..PassStats::default()
        }
    );
}

#[cfg(feature = "graph")]
#[test]
fn hovering_a_node_graph_repaints_without_reshaping() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| GraphScene::new()));
    let bounds = harness.find("Node graph").bounds;
    let node = LogicalPoint::new(bounds.origin.x + 80.0, bounds.origin.y + 50.0);
    harness.hover_at(node);
    assert_eq!(
        harness.stats().passes,
        PassStats {
            hit_test_nodes: 3,
            ..paint_stats(false)
        }
    );
    harness.hover_at(LogicalPoint::new(node.x + 1.0, node.y + 1.0));
    assert_eq!(harness.stats().passes, input_only_stats());
}

#[cfg(feature = "graph")]
#[test]
fn a_graph_release_selects_the_topmost_node_under_the_pointer() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| GraphScene::new()));
    let bounds = harness.find("Node graph").bounds;
    harness.release_pointer_at(LogicalPoint::new(
        bounds.origin.x + 80.0,
        bounds.origin.y + 50.0,
    ));
    assert_eq!(harness.root().read(harness.app()).selected, Some(1));
    assert_eq!(harness.stats().passes, input_only_stats());
    harness.release_pointer_at(LogicalPoint::new(
        bounds.origin.x + 180.0,
        bounds.origin.y + 100.0,
    ));
    assert_eq!(harness.root().read(harness.app()).selected, None);
    assert_eq!(harness.stats().passes, input_only_stats());
}

#[cfg(feature = "graph")]
#[test]
fn a_node_graph_announces_its_node_and_edge_counts() {
    let harness = EntityHarness::new(|cx| cx.new(|_| GraphScene::new()));
    assert_eq!(
        harness.find("Node graph").data.value.as_deref(),
        Some("3 nodes, 2 edges")
    );
}

// Deferred from the v1 suite: image and render-view are rxui-tree primitives,
// but v2 has no core specs/builders for them; adding test-only wrappers here
// would validate those wrappers rather than a shipped widget API. Icon remains
// deferred because its retained element was never ported to v2.
