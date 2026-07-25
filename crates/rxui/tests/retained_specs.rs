//! The reconciliation contract every [`rxui::RetainedSpec`] owes the engine.
//!
//! A specialized element is the one place where RXUI hands a third party direct
//! control over invalidation: `changed()` is the entire answer, and a `true` it
//! did not need costs a full layout, paint, and accessibility pass on that
//! subtree. Each spec below is therefore refreshed against an identical
//! configuration and required to produce *no* retained work at all.

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalSize, Size},
};
use astrelis_paint::{Image, PathVerb};
use rxui::core::PassStats;
use rxui::{
    ChartAction, ChartOptions, ChartPoint, ChartSeries, ChartSeriesKind, ChartSpec, Component,
    ComponentContext, GraphEdge, GraphNode, GraphViewport, Icon, IconSpec, ImageSpec,
    NodeGraphAction, NodeGraphSpec, RenderViewContent, RenderViewSpec, Theme, View, chart, icon,
    icons, image, node_graph, render_surface,
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

struct ChartScene {
    options: ChartOptions,
    selection: Option<(u64, usize)>,
}

impl ChartScene {
    fn new() -> Self {
        Self {
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
        chart(
            ChartSpec::new(
                vec![
                    series(1, ChartSeriesKind::Line, 24),
                    series(2, ChartSeriesKind::Scatter, 8),
                ],
                |action| action,
            )
            .options(self.options),
        )
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
    assert!(harness.stats().rebuilt_fragments > 0);
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

struct GraphScene {
    viewport: GraphViewport,
    selected: Option<u64>,
}

impl GraphScene {
    const NODES: usize = 3;

    fn new() -> Self {
        Self {
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
        let mut spec = NodeGraphSpec::new(
            Self::nodes(),
            vec![GraphEdge { from: 1, to: 2 }, GraphEdge { from: 2, to: 3 }],
            |action| action,
        );
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
#[ignore = "NodeGraphElement::layout reshapes every node title on every layout pass; \
            rxui-widgets/src/node_graph.rs:121-134 has no shaping memo and the engine's \
            ShapingMemo is pub(crate), so a pure pan costs one shape per node"]
fn panning_a_node_graph_reshapes_no_node_titles() {
    let mut harness = Harness::new(GraphScene::new(), VIEWPORT).expect("the graph scene mounts");
    harness.mutate(|scene| {
        scene.viewport = GraphViewport {
            pan: LogicalPoint::new(24.0, 0.0),
            zoom: 1.0,
        };
    });
    assert_eq!(harness.stats().shaped_text, 0);
}

#[test]
fn panning_a_node_graph_currently_reshapes_every_node_title() {
    let mut harness = Harness::new(GraphScene::new(), VIEWPORT).expect("the graph scene mounts");
    harness.mutate(|scene| {
        scene.viewport = GraphViewport {
            pan: LogicalPoint::new(24.0, 0.0),
            zoom: 1.0,
        };
    });
    // Documents the bug `panning_a_node_graph_reshapes_no_node_titles` states:
    // a pan is a paint-space translation, but `RetainedSpec::changed` can only
    // answer `Invalidation::ALL`, and the element re-shapes unconditionally in
    // `layout`, so the whole title set is shaped again. One shape per node.
    assert_eq!(harness.stats().shaped_text, GraphScene::NODES);
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

struct ImageScene {
    image: Image,
    label: String,
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
        }
    }
}

impl Component for ImageScene {
    type Action = ();
    type Effect = ();

    fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {}

    fn view(&self, _theme: &Theme) -> View<()> {
        image(
            ImageSpec::new(self.image.clone(), self.label.clone())
                .size(LogicalSize::new(64.0, 64.0)),
        )
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
    assert!(harness.stats().rebuilt_fragments > 0);
}

struct RenderViewScene {
    content: RenderViewContent,
    inputs: usize,
}

impl RenderViewScene {
    const fn new() -> Self {
        Self {
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
        render_surface(RenderViewSpec::new(
            "Viewport",
            LogicalSize::new(320.0, 240.0),
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
    assert!(harness.stats().rebuilt_fragments > 0);

    let bounds = harness.bounds("Viewport");
    harness.click_at(LogicalPoint::new(
        bounds.origin.x + bounds.size.width * 0.5,
        bounds.origin.y + bounds.size.height * 0.5,
    ));
    // Press and release are two inputs, and `RetainedSpec::update` reinstalled
    // the routing closure during the refresh above, so both must arrive.
    assert_eq!(harness.component().inputs, 2);
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
