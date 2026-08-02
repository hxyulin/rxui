//! The reconciliation contract every [`rxui::view::RetainedSpec`] owes the engine.
//!
//! A specialized element is the one place where RXUI hands a third party direct
//! control over invalidation: `changed()` is the entire answer, and a `true` it
//! did not need costs a full layout, paint, and accessibility pass on that
//! subtree. Each spec below is therefore refreshed against an identical
//! configuration and required to produce *no* retained work at all.

use rxui::{
    Component, ComponentContext, Icon, Theme, View,
    charts::{
        ChartAction, ChartOptions, ChartPoint, ChartSeries, ChartSeriesKind, ChartSpec, chart,
    },
    color::Color,
    engine::PassStats,
    geometry::{LogicalPoint, LogicalSize, Size},
    graph::{GraphEdge, GraphNode, GraphViewport, NodeGraphAction, NodeGraphSpec, node_graph},
    icon,
    icons::{self, IconSpec},
    media::{Image, ImageSpec, RenderViewContent, RenderViewSpec, image, render_view},
    paint::PathVerb,
};
use rxui_test_support::Harness;

/// A viewport large enough that no specialized element is size-constrained.
const VIEWPORT: LogicalSize = LogicalSize::new(800.0, 600.0);

/// Asserts a settled frame did no retained work whatsoever.
///
/// `rebuilt_fragments == 0` is the headline contract, but on its own it is
/// satisfiable by an element that re-laid-out and re-shaped and happened to
/// produce byte-identical paint output. The companion counters make the
/// assertion mean "the engine was never asked", which is what an unchanged
/// spec is supposed to buy.
#[track_caller]
fn assert_no_retained_work(what: &str, stats: PassStats) {
    assert_eq!(
        stats.rebuilt_fragments, 0,
        "{what}: refreshing with an identical spec repainted",
    );
    assert_eq!(
        stats.layout_elements, 0,
        "{what}: refreshing with an identical spec re-laid-out",
    );
    assert_eq!(
        stats.shaped_text, 0,
        "{what}: refreshing with an identical spec re-shaped text",
    );
}

/// The update-model gate's digests, borrowed for the anti-stale guard below.
///
/// Only `semantic_digest` and `fragment_digest` are used here. The module's own
/// `assert_incremental_matches_fresh` wants a `&ComponentHost`, and `Harness`
/// deliberately hands one out to nobody, so the assertion is rebuilt over two
/// harnesses instead of reintroducing the input plumbing the harness exists to
/// remove.
#[allow(dead_code)]
mod support;

/// Asserts an incrementally updated tree equals a freshly mounted equivalent.
///
/// This is the anti-cheat guard for every narrowed `changed()` below. A
/// `PassStats` win is worthless if it was bought by leaving retained state
/// behind, and an invalidation bit a spec fails to report is exactly that: the
/// counters go down and the frame goes stale. Mounting a second host from the
/// live component's own state and comparing both digests is what makes a
/// `layout_elements == 0` assertion mean "nothing needed laying out" rather than
/// "nothing was laid out".
///
/// The fresh scene is cloned from the live one rather than rebuilt by hand,
/// because the property under test is that the *retained tree* converged, not
/// that a test can restate a component's final state.
#[track_caller]
fn assert_matches_freshly_mounted<C: Component + Clone>(what: &str, live: &Harness<C>) {
    let fresh = Harness::new(live.component().clone(), VIEWPORT)
        .expect("the reference scene mounts from the live scene's own state");
    assert_eq!(
        live.with_ui(support::semantic_digest),
        fresh.with_ui(support::semantic_digest),
        "{what}: the incremental semantic tree diverged from a freshly mounted one",
    );
    assert_eq!(
        live.with_ui(support::fragment_digest),
        fresh.with_ui(support::fragment_digest),
        "{what}: the incremental paint output diverged from a freshly mounted one",
    );
}

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

#[derive(Clone)]
struct ChartScene {
    series: Vec<ChartSeries>,
    options: ChartOptions,
    selection: Option<(u64, usize)>,
}

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

impl Component for ChartScene {
    type Action = ChartAction;
    type Effect = ();

    fn update(&mut self, action: ChartAction, _context: &mut ComponentContext<'_, ()>) {
        self.selection = match action {
            ChartAction::Select { series, point } => Some((series, point)),
            ChartAction::Clear => None,
        };
    }

    fn view(&self, _theme: &Theme) -> View<ChartAction> {
        chart(ChartSpec::new(self.series.clone(), |action| action).options(self.options))
    }
}

#[test]
fn refreshing_a_chart_with_an_identical_spec_does_no_retained_work() {
    let mut harness = Harness::new(ChartScene::new(), VIEWPORT).expect("the chart scene mounts");
    harness.refresh();
    assert_no_retained_work("ChartSpec", harness.stats());
}

#[test]
fn a_changed_chart_option_still_reaches_the_retained_element() {
    let mut harness = Harness::new(ChartScene::new(), VIEWPORT).expect("the chart scene mounts");
    harness.mutate(|scene| {
        scene.options = ChartOptions {
            size: LogicalSize::new(320.0, 200.0),
            ..ChartOptions::default()
        };
    });
    // The chart reports its own preferred size from `layout`, so a resized
    // option has to be observable in the accessible bounds - otherwise the
    // no-work assertion above could be passing for the wrong reason.
    assert_eq!(harness.bounds("Chart").size, LogicalSize::new(320.0, 200.0));
    let stats = harness.stats();
    assert!(stats.rebuilt_fragments > 0);
    // The preferred size is the chart's only layout input, so this is the one
    // field of the one spec below that is entitled to re-measure anything.
    assert!(
        stats.layout_elements > 0,
        "a resized chart is the one chart change that re-runs layout",
    );
    assert_matches_freshly_mounted("ChartSpec size", &harness);
}

#[test]
fn a_chart_background_change_repaints_without_re_running_layout() {
    let mut harness = Harness::new(ChartScene::new(), VIEWPORT).expect("the chart scene mounts");
    harness.mutate(|scene| {
        scene.options = ChartOptions {
            background: Color::from_hex(0x2a2f3a),
            ..scene.options
        };
    });
    let stats = harness.stats();
    // A surface fill is read by `paint` and by nothing else. If it re-measured,
    // a theme switch would drag every chart in a dashboard through layout.
    assert_eq!(
        stats.layout_elements, 0,
        "a chart's fill colour is not a layout input",
    );
    assert_eq!(stats.shaped_text, 0);
    assert!(stats.rebuilt_fragments > 0, "the new fill is painted");
    // The stale-frame case this guards: had `changed` reported nothing at all,
    // the counters would look even better and the surface would still be dark.
    assert_matches_freshly_mounted("ChartSpec background", &harness);
}

#[test]
fn a_chart_data_change_repaints_and_announces_the_new_series_count() {
    let mut harness = Harness::new(ChartScene::new(), VIEWPORT).expect("the chart scene mounts");
    harness.mutate(|scene| scene.series.push(series(3, ChartSeriesKind::Line, 6)));
    let stats = harness.stats();
    // New data moves every plotted point, because the domain extents are
    // recomputed from the series - but `ChartElement::layout` computes them
    // nowhere. It constrains `options.size` and stops, and `paint` derives the
    // extents itself on every frame, so data is a repaint and not a re-measure.
    assert_eq!(
        stats.layout_elements, 0,
        "chart extents are a paint-time projection, not a layout product",
    );
    assert!(stats.rebuilt_fragments > 0);
    // Which is also why the count has to be declared as an accessibility change
    // in its own right: no layout pass is going to carry it.
    assert_eq!(
        harness.find("Chart").data.value.as_deref(),
        Some("3 series"),
    );
    assert_matches_freshly_mounted("ChartSpec series", &harness);
}

#[test]
fn chart_hover_repaints_without_layout_or_reshaping() {
    let mut harness = Harness::new(ChartScene::new(), VIEWPORT).expect("the chart scene mounts");
    let bounds = harness.bounds("Chart");
    // The first series runs y = (x * 7) % 19 over x in 0..24, so its x = 0
    // point sits at the domain origin, which projects to the bottom-left of the
    // inset plot box. Hovering there is guaranteed to be within the element's
    // 12 logical unit hover radius of a real point.
    let origin = LogicalPoint::new(bounds.origin.x + 12.0, bounds.origin.y + bounds.size.height);

    harness.hover_at(origin);
    let stats = harness.stats();
    assert_eq!(
        stats.layout_elements, 0,
        "a chart hover must not re-run layout",
    );
    assert_eq!(stats.shaped_text, 0, "a chart hover must not reshape text");
    assert!(
        stats.rebuilt_fragments > 0,
        "the hover marker is painted, so exactly the chart's fragment rebuilds",
    );

    // Moving within the same nearest point changes nothing, and the element
    // returns an empty invalidation, so the engine produces no frame at all.
    harness.hover_at(LogicalPoint::new(origin.x + 1.0, origin.y - 1.0));
    let stats = harness.stats();
    assert_no_retained_work("ChartSpec hover with an unchanged nearest point", stats);
}

#[test]
fn a_chart_release_reports_the_nearest_point_to_the_component() {
    let mut harness = Harness::new(ChartScene::new(), VIEWPORT).expect("the chart scene mounts");
    let bounds = harness.bounds("Chart");
    harness.click_at(LogicalPoint::new(
        bounds.origin.x + 12.0,
        bounds.origin.y + bounds.size.height,
    ));
    // Series 1's first point is the one at the domain origin.
    assert_eq!(harness.component().selection, Some((1, 0)));

    // The centre of the plot area is more than 12 logical units from every
    // point of these two series, which is the element's selection radius.
    harness.click_at(LogicalPoint::new(
        bounds.origin.x + bounds.size.width * 0.5,
        bounds.origin.y + bounds.size.height * 0.5,
    ));
    assert_eq!(harness.component().selection, None);
}

#[derive(Clone)]
struct GraphScene {
    nodes: Vec<GraphNode<u64>>,
    edges: Vec<GraphEdge<u64>>,
    viewport: GraphViewport,
    selected: Option<u64>,
}

impl GraphScene {
    fn new() -> Self {
        Self {
            nodes: Self::nodes(),
            edges: vec![GraphEdge { from: 1, to: 2 }, GraphEdge { from: 2, to: 3 }],
            viewport: GraphViewport::default(),
            selected: None,
        }
    }

    fn nodes() -> Vec<GraphNode<u64>> {
        vec![
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
        ]
    }

    /// Five distinctly titled nodes, for the per-node shaping memo.
    ///
    /// Three is not enough to catch a memo that pairs a title with the wrong
    /// node: at that size a mix-up can still shape the right *number* of
    /// strings, and a count assertion sails through. Five titles of five
    /// different lengths make a mix-up land in the painted glyph runs, where
    /// [`assert_matches_freshly_mounted`] sees it.
    fn wide() -> Self {
        let titles = ["Load", "Decode", "Resample", "Mix", "Encode"];
        Self {
            nodes: titles
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

impl Component for GraphScene {
    type Action = NodeGraphAction<u64>;
    type Effect = ();

    fn update(&mut self, action: NodeGraphAction<u64>, _context: &mut ComponentContext<'_, ()>) {
        self.selected = match action {
            NodeGraphAction::Select(id) => Some(id),
            NodeGraphAction::ClearSelection => None,
        };
    }

    fn view(&self, _theme: &Theme) -> View<NodeGraphAction<u64>> {
        let mut spec = NodeGraphSpec::new(self.nodes.clone(), self.edges.clone(), |action| action);
        spec.viewport = self.viewport;
        spec.selected = self.selected;
        node_graph(spec)
    }
}

#[test]
fn refreshing_a_node_graph_with_an_identical_spec_does_no_retained_work() {
    let mut harness = Harness::new(GraphScene::new(), VIEWPORT).expect("the graph scene mounts");
    harness.refresh();
    assert_no_retained_work("NodeGraphSpec", harness.stats());
}

#[test]
fn panning_a_node_graph_reshapes_no_node_titles() {
    let mut harness = Harness::new(GraphScene::new(), VIEWPORT).expect("the graph scene mounts");
    harness.mutate(|scene| {
        scene.viewport = GraphViewport {
            pan: LogicalPoint::new(24.0, 0.0),
            zoom: 1.0,
        };
    });
    let stats = harness.stats();
    // A pan is a paint-space translation and nothing else: `rect` applies it,
    // `layout` never reads it, the element reports the same size whatever the
    // viewport says, and it has no children to re-place. So the graph is not
    // dragged back through layout, and no title is shaped a second time.
    //
    // Both halves of that are load-bearing. The memo alone brings this to zero
    // shapes while still re-laying-out; the narrowed `changed` alone brings it
    // to zero layouts while any *other* reason to lay out - a retitled node,
    // a resized window - would still shape the whole canvas.
    assert_eq!(stats.shaped_text, 0, "a pan must not reshape any title");
    assert_eq!(stats.layout_elements, 0, "a pan must not re-run layout");
    assert!(
        stats.rebuilt_fragments > 0,
        "the panned canvas is repainted"
    );
    assert_matches_freshly_mounted("NodeGraphSpec pan", &harness);
}

#[test]
fn zooming_a_node_graph_reshapes_no_node_titles() {
    let mut harness = Harness::new(GraphScene::new(), VIEWPORT).expect("the graph scene mounts");
    harness.mutate(|scene| {
        scene.viewport = GraphViewport {
            pan: LogicalPoint::ZERO,
            zoom: 2.0,
        };
    });
    let stats = harness.stats();
    // Zoom scales the node rectangles, which is the case where a paint-only
    // answer looks least plausible - and it holds for the same reason a pan
    // does. The titles are drawn at a fixed 14 logical units and centred in
    // whatever rectangle `paint` computes, so the shaped runs are unaffected.
    assert_eq!(stats.shaped_text, 0);
    assert_eq!(stats.layout_elements, 0);
    assert!(stats.rebuilt_fragments > 0);
    assert_matches_freshly_mounted("NodeGraphSpec zoom", &harness);
}

#[test]
fn selecting_a_graph_node_repaints_without_layout_or_a_new_announcement() {
    let mut harness = Harness::new(GraphScene::new(), VIEWPORT).expect("the graph scene mounts");
    let announced = harness.find("Node graph").data.value;
    harness.dispatch(NodeGraphAction::Select(2));

    let stats = harness.stats();
    // The selection is a fill colour, so it is a repaint. It is deliberately
    // *not* an accessibility change: `NodeGraphElement::accessibility` publishes
    // the node and edge counts and no per-node state, so there is nothing for
    // the semantic delta to carry. If the graph ever starts announcing its
    // selection, this assertion is the one that has to be revisited first.
    assert_eq!(
        stats.layout_elements, 0,
        "a selection highlight must not re-run layout",
    );
    assert_eq!(stats.shaped_text, 0, "a selection must not reshape");
    assert!(stats.rebuilt_fragments > 0, "the new fill is painted");
    assert_eq!(harness.find("Node graph").data.value, announced);
    // And the fill really did change: a spec that reported nothing here would
    // post better counters and paint the node unselected forever.
    assert_matches_freshly_mounted("NodeGraphSpec selection", &harness);
}

#[test]
fn moving_a_graph_node_repaints_without_reshaping_its_title() {
    let mut harness = Harness::new(GraphScene::new(), VIEWPORT).expect("the graph scene mounts");
    harness.mutate(|scene| scene.nodes[1].position = LogicalPoint::new(240.0, 60.0));

    let stats = harness.stats();
    // A drag changes `nodes`, which is the field that also carries the titles -
    // so the narrowing compares the titles pairwise rather than comparing the
    // node list as a whole. A node that only moved paints from a title that is
    // still correct, and asking for a layout pass to learn that would put every
    // frame of a drag through the shaper.
    assert_eq!(
        stats.layout_elements, 0,
        "moving a node changes no measured size and no title",
    );
    assert_eq!(stats.shaped_text, 0);
    assert!(stats.rebuilt_fragments > 0);
    assert_matches_freshly_mounted("NodeGraphSpec node position", &harness);
}

#[test]
fn changing_only_the_edges_repaints_and_announces_the_new_count() {
    let mut harness = Harness::new(GraphScene::new(), VIEWPORT).expect("the graph scene mounts");
    harness.mutate(|scene| scene.edges.push(GraphEdge { from: 1, to: 3 }));

    let stats = harness.stats();
    // An edge is a stroked path between two node centres. Nothing about it is
    // shaped or measured, but the count is announced, which is why the edges
    // carry an accessibility bit that no layout pass is going to supply.
    assert_eq!(stats.layout_elements, 0);
    assert_eq!(stats.shaped_text, 0);
    assert!(stats.rebuilt_fragments > 0);
    assert_eq!(
        harness.find("Node graph").data.value.as_deref(),
        Some("3 nodes, 3 edges"),
    );
    assert_matches_freshly_mounted("NodeGraphSpec edges", &harness);
}

#[test]
fn renaming_one_node_of_five_reshapes_only_that_title() {
    let mut harness = Harness::new(GraphScene::wide(), VIEWPORT).expect("the graph scene mounts");
    harness.mutate(|scene| scene.nodes[3].title = "Crossfade".into());

    let stats = harness.stats();
    // A title is shaped in `layout`, so a rename is genuinely a layout change -
    // and the memo is what keeps it from being a *whole canvas* layout change.
    // Four of the five entries are claimed by an identical request and come back
    // as pointer clones.
    assert_eq!(
        stats.shaped_text, 1,
        "only the retitled node is shaped; the other four are memoized",
    );
    assert!(
        stats.layout_elements > 0,
        "shaping happens in layout, so a rename has to re-run it",
    );
    // The count above is satisfiable by a memo that shaped once and handed the
    // result to the wrong node. This is what says the glyphs landed on the
    // right rectangles.
    assert_matches_freshly_mounted("NodeGraphSpec rename", &harness);
}

#[test]
fn adding_a_graph_node_shapes_only_the_new_title() {
    let mut harness = Harness::new(GraphScene::wide(), VIEWPORT).expect("the graph scene mounts");
    harness.mutate(|scene| {
        scene.nodes.push(GraphNode {
            id: 6,
            title: "Normalize".into(),
            position: LogicalPoint::new(600.0, 30.0),
            size: LogicalSize::new(100.0, 50.0),
        });
    });

    let stats = harness.stats();
    // Keying the memo by node identity rather than by index is what makes this
    // one shape instead of six: an append shifts no other node's key.
    assert_eq!(stats.shaped_text, 1);
    assert_eq!(
        harness.find("Node graph").data.value.as_deref(),
        Some("6 nodes, 0 edges"),
    );
    assert_matches_freshly_mounted("NodeGraphSpec node added", &harness);
}

#[test]
fn removing_a_graph_node_reshapes_nothing_and_drops_its_memo_entry() {
    let mut harness = Harness::new(GraphScene::wide(), VIEWPORT).expect("the graph scene mounts");
    let removed = harness.component().nodes[1].clone();
    harness.mutate(|scene| {
        scene.nodes.remove(1);
    });

    // Every survivor keeps its own entry, so a removal in the middle of the list
    // reshapes nothing at all - the case an index-keyed memo would have turned
    // into three misses.
    assert_eq!(harness.stats().shaped_text, 0);
    assert_eq!(
        harness.find("Node graph").data.value.as_deref(),
        Some("4 nodes, 0 edges"),
    );
    assert_matches_freshly_mounted("NodeGraphSpec node removed", &harness);

    // Putting it back shapes again, which is how a test can see that the entry
    // left with the node. Keeping it would make this free, and that is exactly
    // the trade being refused: the memo is sized by the graph, not by every
    // node the graph has ever held.
    harness.mutate(|scene| scene.nodes.insert(1, removed));
    assert_eq!(harness.stats().shaped_text, 1);
    assert_matches_freshly_mounted("NodeGraphSpec node restored", &harness);
}

#[test]
fn hovering_a_node_graph_repaints_without_reshaping() {
    let mut harness = Harness::new(GraphScene::new(), VIEWPORT).expect("the graph scene mounts");
    let bounds = harness.bounds("Node graph");
    // Node 1 occupies canvas (20, 20) to (140, 80) at the default identity
    // viewport, so its centre is (80, 50) in element space.
    let node = LogicalPoint::new(bounds.origin.x + 80.0, bounds.origin.y + 50.0);

    harness.hover_at(node);
    let stats = harness.stats();
    assert_eq!(stats.layout_elements, 0, "a graph hover must not re-layout");
    assert_eq!(stats.shaped_text, 0, "a graph hover must not reshape");
    assert!(stats.rebuilt_fragments > 0, "the hovered node is repainted");

    harness.hover_at(LogicalPoint::new(node.x + 1.0, node.y + 1.0));
    assert_no_retained_work("NodeGraphSpec hover within the same node", harness.stats());
}

#[test]
fn a_graph_release_selects_the_topmost_node_under_the_pointer() {
    let mut harness = Harness::new(GraphScene::new(), VIEWPORT).expect("the graph scene mounts");
    let bounds = harness.bounds("Node graph");
    harness.click_at(LogicalPoint::new(
        bounds.origin.x + 80.0,
        bounds.origin.y + 50.0,
    ));
    assert_eq!(harness.component().selected, Some(1));

    // Canvas (180, 100) lies in none of the three node rectangles.
    harness.click_at(LogicalPoint::new(
        bounds.origin.x + 180.0,
        bounds.origin.y + 100.0,
    ));
    assert_eq!(harness.component().selected, None);
}

#[test]
fn a_node_graph_announces_its_node_and_edge_counts() {
    let harness = Harness::new(GraphScene::new(), VIEWPORT).expect("the graph scene mounts");
    assert_eq!(
        harness.find("Node graph").data.value.as_deref(),
        Some("3 nodes, 2 edges"),
    );
}

#[derive(Clone)]
struct ImageScene {
    image: Image,
    label: String,
    size: LogicalSize,
    opacity: f32,
}

impl ImageScene {
    /// A 2x2 opaque RGBA8 checkerboard, the smallest image the paint layer takes.
    fn new() -> Self {
        Self {
            image: Image::from_rgba8(
                Size::new(2, 2),
                vec![
                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
                ],
            )
            .expect("a 2x2 RGBA8 buffer is 16 bytes"),
            label: "Preview".into(),
            size: LogicalSize::new(64.0, 64.0),
            opacity: 1.0,
        }
    }
}

impl Component for ImageScene {
    type Action = ();
    type Effect = ();

    fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {}

    fn view(&self, _theme: &Theme) -> View<()> {
        let mut spec = ImageSpec::new(self.image.clone(), self.label.clone()).size(self.size);
        spec.opacity = self.opacity;
        image(spec)
    }
}

#[test]
fn refreshing_an_image_with_an_identical_spec_does_no_retained_work() {
    let mut harness = Harness::new(ImageScene::new(), VIEWPORT).expect("the image scene mounts");
    harness.refresh();
    assert_no_retained_work("ImageSpec", harness.stats());
}

#[test]
fn an_image_spec_compares_sources_by_allocation_identity_not_by_pixels() {
    let mut harness = Harness::new(ImageScene::new(), VIEWPORT).expect("the image scene mounts");
    let pixels = harness.component().image.rgba8().to_vec();
    harness.mutate(|scene| {
        scene.image = Image::from_rgba8(Size::new(2, 2), pixels)
            .expect("the replacement carries the same bytes");
    });
    // `ImageSpec::changed` keys on `Image::cache_id`, which is a per-allocation
    // counter, so a byte-identical replacement is still a change. That is the
    // right trade: comparing pixel buffers per frame would cost more than the
    // repaint it avoids.
    let stats = harness.stats();
    assert!(stats.rebuilt_fragments > 0);
    // A new source does not re-measure: `ImageElement::layout` constrains the
    // spec's requested size and never asks the image how big it is, so a
    // replacement of different pixel dimensions occupies the same box.
    assert_eq!(
        stats.layout_elements, 0,
        "the source image is not a layout input",
    );
    assert_matches_freshly_mounted("ImageSpec source", &harness);
}

#[test]
fn an_image_label_change_re_announces_without_repainting() {
    let mut harness = Harness::new(ImageScene::new(), VIEWPORT).expect("the image scene mounts");
    harness.mutate(|scene| scene.label = "Thumbnail".into());

    let stats = harness.stats();
    // The label is announced and never drawn, so this is the one field of the
    // spec that must produce an accessibility delta and no fragment at all.
    assert_eq!(
        stats.rebuilt_fragments, 0,
        "an image's label is not painted, so nothing repaints",
    );
    assert_eq!(stats.layout_elements, 0);
    assert!(
        harness.try_find("Thumbnail").is_some(),
        "the label reached the semantic tree"
    );
    assert_matches_freshly_mounted("ImageSpec label", &harness);
}

#[test]
fn an_image_opacity_change_repaints_without_re_announcing() {
    let mut harness = Harness::new(ImageScene::new(), VIEWPORT).expect("the image scene mounts");
    harness.mutate(|scene| scene.opacity = 0.4);

    let stats = harness.stats();
    // Draw opacity is the mirror image of the label: painted, never announced.
    assert!(stats.rebuilt_fragments > 0);
    assert_eq!(stats.layout_elements, 0);
    assert_eq!(
        stats.accessibility_nodes, 0,
        "a fade publishes no new accessible property",
    );
    assert_matches_freshly_mounted("ImageSpec opacity", &harness);
}

#[test]
fn an_image_size_change_re_runs_layout() {
    let mut harness = Harness::new(ImageScene::new(), VIEWPORT).expect("the image scene mounts");
    harness.mutate(|scene| scene.size = LogicalSize::new(128.0, 32.0));

    // The requested size is the spec's only layout input, and the accessible
    // bounds are how a test can see that it was honoured.
    assert_eq!(
        harness.bounds("Preview").size,
        LogicalSize::new(128.0, 32.0),
    );
    assert!(harness.stats().layout_elements > 0);
    assert_matches_freshly_mounted("ImageSpec size", &harness);
}

#[derive(Clone)]
struct RenderViewScene {
    label: String,
    size: LogicalSize,
    content: RenderViewContent,
    inputs: usize,
}

impl RenderViewScene {
    fn new() -> Self {
        Self {
            label: "Viewport".into(),
            size: LogicalSize::new(320.0, 240.0),
            content: RenderViewContent::Unavailable,
            inputs: 0,
        }
    }
}

impl Component for RenderViewScene {
    type Action = ();
    type Effect = ();

    fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {
        self.inputs += 1;
    }

    fn view(&self, _theme: &Theme) -> View<()> {
        render_view(RenderViewSpec::new(
            self.label.clone(),
            self.size,
            self.content.clone(),
            |_input| (),
        ))
    }
}

#[test]
fn refreshing_a_render_view_with_an_identical_spec_does_no_retained_work() {
    let mut harness =
        Harness::new(RenderViewScene::new(), VIEWPORT).expect("the render view scene mounts");
    harness.refresh();
    assert_no_retained_work("RenderViewSpec", harness.stats());
}

#[test]
fn a_render_view_reports_a_content_change_and_keeps_routing_input() {
    let mut harness =
        Harness::new(RenderViewScene::new(), VIEWPORT).expect("the render view scene mounts");
    harness.mutate(|scene| scene.content = RenderViewContent::Error("no device".into()));
    let stats = harness.stats();
    assert!(stats.rebuilt_fragments > 0);
    // The content is drawn *and* announced: a viewport that lost its device has
    // to say so rather than only turning red, which is why the content carries
    // an accessibility bit and not just a paint one.
    assert_eq!(
        harness.find("Viewport").data.value.as_deref(),
        Some("no device"),
    );
    // What it is not is a layout input. The viewport keeps the size it asked
    // for whether or not there is anything to show in it.
    assert_eq!(stats.layout_elements, 0);
    assert_matches_freshly_mounted("RenderViewSpec content", &harness);

    let bounds = harness.bounds("Viewport");
    harness.click_at(LogicalPoint::new(
        bounds.origin.x + bounds.size.width * 0.5,
        bounds.origin.y + bounds.size.height * 0.5,
    ));
    // Press and release are two inputs, and `RetainedSpec::update` reinstalled
    // the routing closure during the refresh above, so both must arrive.
    assert_eq!(harness.component().inputs, 2);
}

#[test]
fn a_render_view_label_change_re_announces_without_repainting() {
    let mut harness =
        Harness::new(RenderViewScene::new(), VIEWPORT).expect("the render view scene mounts");
    harness.mutate(|scene| scene.label = "Preview camera".into());

    let stats = harness.stats();
    assert_eq!(
        stats.rebuilt_fragments, 0,
        "a viewport's label is not drawn into it",
    );
    assert_eq!(stats.layout_elements, 0);
    assert!(harness.try_find("Preview camera").is_some());
    assert_matches_freshly_mounted("RenderViewSpec label", &harness);
}

#[test]
fn a_render_view_size_change_re_runs_layout() {
    let mut harness =
        Harness::new(RenderViewScene::new(), VIEWPORT).expect("the render view scene mounts");
    harness.mutate(|scene| scene.size = LogicalSize::new(200.0, 400.0));

    assert_eq!(
        harness.bounds("Viewport").size,
        LogicalSize::new(200.0, 400.0),
    );
    assert!(harness.stats().layout_elements > 0);
    assert_matches_freshly_mounted("RenderViewSpec size", &harness);
}

struct IconScene {
    /// Replaces the built-in glyph, for the two content-identity tests.
    glyph: Option<Icon>,
    size: f32,
    color: Color,
    label: String,
}

impl IconScene {
    fn new() -> Self {
        Self {
            glyph: None,
            size: 24.0,
            color: Color::WHITE,
            label: "Settings".into(),
        }
    }
}

impl Component for IconScene {
    type Action = ();
    type Effect = ();

    fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {}

    fn view(&self, _theme: &Theme) -> View<()> {
        // Left to the default, this builds its glyph with `icons::settings()` on
        // every pass, and every such call allocates a new `Path`. That is the
        // shape a spec has to tolerate: a view is a description, rebuilt from
        // nothing each time, and two descriptions of the same icon must compare
        // equal however many times the path behind them was allocated.
        icon(
            IconSpec::new(self.glyph.clone().unwrap_or_else(icons::settings))
                .size(self.size)
                .color(self.color)
                .label(self.label.clone()),
        )
    }
}

/// Rebuilds an icon's geometry into a separately allocated `Path`.
fn rebuilt_path(icon: &Icon) -> Icon {
    Icon::from_verbs(icon.view_box(), icon.path().verbs().iter().copied())
        .expect("re-recording a valid icon's verbs yields a valid icon")
        .with_fill_rule(icon.fill_rule())
}

/// Rebuilds an icon with exactly one of its verbs displaced by a logical unit.
fn one_verb_moved(icon: &Icon) -> Icon {
    let mut verbs = icon.path().verbs().to_vec();
    let point = verbs
        .iter_mut()
        .find_map(|verb| match verb {
            PathVerb::LineTo(point) => Some(point),
            _ => None,
        })
        .expect("the settings glyph draws line segments");
    point.x += 1.0;
    Icon::from_verbs(icon.view_box(), verbs)
        .expect("displacing one point keeps the icon valid")
        .with_fill_rule(icon.fill_rule())
}

#[test]
fn refreshing_an_icon_with_an_identical_spec_does_no_retained_work() {
    let mut harness = Harness::new(IconScene::new(), VIEWPORT).expect("the icon scene mounts");
    harness.refresh();
    assert_no_retained_work("IconSpec", harness.stats());
}

#[test]
fn an_icon_rebuilt_from_the_same_verbs_is_not_a_change() {
    let mut harness = Harness::new(IconScene::new(), VIEWPORT).expect("the icon scene mounts");
    harness.mutate(|scene| scene.glyph = Some(rebuilt_path(&icons::settings())));
    // Two independently constructed paths recording the same verbs describe the
    // same picture, and `Icon`'s equality says so. Keying on `Path::cache_id`
    // instead - a per-allocation counter - is what made every icon in a tree
    // report itself changed on every single frame.
    assert_no_retained_work("IconSpec with a re-recorded path", harness.stats());
}

#[test]
fn an_icon_differing_in_one_verb_repaints_without_relayout() {
    let mut harness = Harness::new(IconScene::new(), VIEWPORT).expect("the icon scene mounts");
    harness.mutate(|scene| scene.glyph = Some(one_verb_moved(&icons::settings())));
    let stats = harness.stats();
    // The companion to the test above: content equality has to be equality, not
    // a constant `true`. One displaced point is a different picture and must
    // reach the element.
    assert_eq!(stats.rebuilt_fragments, 1);
    // A glyph is fitted into the square `size` asked for, so its verbs never
    // reach `IconElement::layout`. Nothing re-measures.
    assert_eq!(stats.layout_elements, 0);
}

#[test]
fn an_icon_color_change_repaints_without_relayout() {
    let mut harness = Harness::new(IconScene::new(), VIEWPORT).expect("the icon scene mounts");
    harness.mutate(|scene| scene.color = Color::from_hex(0xff5c33));
    let stats = harness.stats();
    assert_eq!(stats.rebuilt_fragments, 1);
    // A monochrome fill is a paint input and nothing else. This is the whole
    // point of narrowing `changed`: recoloring an icon per theme, hover, or
    // enablement state must not drag the icon and its ancestors through layout.
    assert_eq!(stats.layout_elements, 0);
    assert_eq!(stats.accessibility_nodes, 0);
}

#[test]
fn an_icon_size_change_relayouts_and_resizes_the_semantic_node() {
    let mut harness = Harness::new(IconScene::new(), VIEWPORT).expect("the icon scene mounts");
    harness.mutate(|scene| scene.size = 32.0);
    // The size is the one field that reaches `IconElement::layout`, so it has to
    // be observable in the accessible bounds.
    assert_eq!(
        harness.bounds("Settings").size,
        LogicalSize::new(32.0, 32.0)
    );
    let stats = harness.stats();
    assert_eq!(stats.rebuilt_fragments, 1);
    // Two: the icon itself, plus the root component's flex container, which has
    // to re-measure a child that declared a different intrinsic size.
    assert_eq!(stats.layout_elements, 2);
}

#[test]
fn an_icon_size_the_element_cannot_act_on_is_not_a_change() {
    let mut harness = Harness::new(IconScene::new(), VIEWPORT).expect("the icon scene mounts");
    harness.mutate(|scene| scene.size = 0.5);
    harness.mutate(|scene| scene.size = 0.25);
    // An icon is never laid out below one logical unit, so two requests that
    // resolve to the same edge are the same request. Comparing the raw field
    // would relayout on every pass an animation spent under the floor, and would
    // relayout forever for a non-finite edge, which never equals itself.
    assert_no_retained_work("IconSpec below its layout floor", harness.stats());
}

#[test]
fn an_icon_label_change_republishes_semantics_without_repainting() {
    let mut harness = Harness::new(IconScene::new(), VIEWPORT).expect("the icon scene mounts");
    harness.mutate(|scene| scene.label = "Preferences".into());
    assert!(harness.try_find("Preferences").is_some());
    let stats = harness.stats();
    // An icon's label is announced, never drawn: `IconElement::paint` fills the
    // path and stops. Renaming one is a semantic event alone.
    assert_eq!(stats.accessibility_nodes, 1);
    assert_eq!(stats.rebuilt_fragments, 0);
    assert_eq!(stats.layout_elements, 0);
}

#[test]
fn an_icon_keeps_its_retained_identity_across_a_rebuild() {
    let mut harness = Harness::new(IconScene::new(), VIEWPORT).expect("the icon scene mounts");
    let before = harness.find("Settings").id;
    harness.refresh();
    // Reconciliation may not fall back to remove-and-rebuild, which would also
    // discard focus and hover state on any specialized element holding them.
    assert_eq!(harness.find("Settings").id, before);
}
