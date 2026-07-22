//! Native/browser visual showcase for RXUI charts, images, services, and node graphs.

#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use std::{path::PathBuf, sync::Arc, time::Duration};

use astrelis_core::geometry::Size;
use astrelis_paint::Image;
use rxui::prelude::*;

#[derive(Clone, Debug)]
enum Message {
    Graph(NodeGraphAction<u64>),
    Chart(ChartAction),
    ToggleLive,
    LiveTick,
    FollowLatest,
    Import,
    Export,
    FilePicked(Result<Option<SelectedFile>, ServiceError>),
    Saved(Result<Option<SavedFile>, ServiceError>),
    Reload(PathBuf),
    Reloaded(Result<Option<SelectedFile>, ServiceError>),
}

struct WorkflowStudio {
    services: DesktopServices,
    graph: NodeGraphDocument<u64>,
    graph_selection: NodeGraphSelection<u64>,
    graph_view: Option<ElementHandle<NodeGraphView<u64, Message>>>,
    chart_view: Option<ElementHandle<ChartView<Message>>>,
    image_view: Option<ElementHandle<ImageView>>,
    status: Option<ElementHandle<Label>>,
    watcher: Option<FileWatcher>,
    live_timer: Option<TimerId>,
    reload_task: Option<TaskId>,
    next_sample: u64,
    next_edge: u64,
}

impl WorkflowStudio {
    fn new() -> Self {
        Self {
            services: DesktopServices::native(),
            graph: sample_graph(),
            graph_selection: NodeGraphSelection::default(),
            graph_view: None,
            chart_view: None,
            image_view: None,
            status: None,
            watcher: None,
            live_timer: None,
            reload_task: None,
            next_sample: 1_009,
            next_edge: 10_000,
        }
    }

    fn set_status(&self, cx: &mut AppCx<'_, Message>, text: impl Into<String>) -> rxui::Result<()> {
        cx.source_ui()?
            .set_label_text(self.status.expect("status exists"), text)?;
        Ok(())
    }

    fn sync_graph(&self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        let mut result = Ok(());
        cx.source_ui()?
            .update_widget(self.graph_view.expect("graph exists"), |view| {
                result = view.sync(self.graph.clone(), self.graph_selection.clone())
            })?;
        result.map_err(rxui::Error::from)
    }

    fn import_file(
        &mut self,
        cx: &mut AppCx<'_, Message>,
        selected: SelectedFile,
    ) -> rxui::Result<()> {
        if selected.name.to_ascii_lowercase().ends_with(".json") {
            let graph = serde_json::from_slice::<NodeGraphDocument<u64>>(&selected.bytes)?;
            graph.validate()?;
            self.graph = graph;
            self.graph_selection = NodeGraphSelection::default();
            self.sync_graph(cx)?;
            self.set_status(cx, format!("Imported graph: {}", selected.name))?;
        } else {
            let image = decode_image(&selected.bytes)?;
            cx.source_ui()?
                .update_widget(self.image_view.expect("image exists"), |view| {
                    view.set_image(image)
                })?;
            self.set_status(cx, format!("Imported image: {}", selected.name))?;
        }

        if let Some(path) = selected.path {
            let proxy = cx.proxy();
            self.watcher = self
                .services
                .watch(FileWatchOptions::new(path.clone()), move |event| {
                    if event.is_ok() {
                        let _ = proxy.post(Message::Reload(path.clone()));
                    }
                })
                .ok();
        }
        Ok(())
    }

    fn apply_graph_action(
        &mut self,
        cx: &mut AppCx<'_, Message>,
        action: NodeGraphAction<u64>,
    ) -> rxui::Result<()> {
        match action {
            NodeGraphAction::SetSelection(selection) => self.graph_selection = selection,
            NodeGraphAction::SetNodePositions { positions, .. } => {
                for (id, position) in positions {
                    if let Some(node) = self.graph.nodes.iter_mut().find(|node| node.id == id) {
                        node.position = position;
                    }
                }
            }
            NodeGraphAction::Connect { from, to } => {
                self.graph.edges.push(GraphEdge {
                    id: self.next_edge,
                    from,
                    to,
                });
                self.next_edge += 1;
            }
            NodeGraphAction::DeleteSelection(selection) => {
                self.graph
                    .nodes
                    .retain(|node| !selection.nodes.contains(&node.id));
                self.graph.edges.retain(|edge| {
                    !selection.edges.contains(&edge.id)
                        && !selection.nodes.contains(&edge.from.node)
                        && !selection.nodes.contains(&edge.to.node)
                });
                self.graph_selection = NodeGraphSelection::default();
            }
            NodeGraphAction::SetViewport { viewport, .. } => self.graph.viewport = viewport,
            NodeGraphAction::FrameAll => {
                let min_x = self
                    .graph
                    .nodes
                    .iter()
                    .map(|node| node.position.x)
                    .reduce(f32::min)
                    .unwrap_or(0.0);
                let min_y = self
                    .graph
                    .nodes
                    .iter()
                    .map(|node| node.position.y)
                    .reduce(f32::min)
                    .unwrap_or(0.0);
                self.graph.viewport = GraphViewport {
                    pan: GraphPoint::new(48.0 - min_x, 48.0 - min_y),
                    zoom: 1.0,
                };
            }
        }
        self.sync_graph(cx)
    }
}

impl App for WorkflowStudio {
    type Message = Message;

    fn build(&mut self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        let mut ui = Ui::new(
            astrelis_ui_core::deterministic_font_database(),
            workflow_theme(),
        );
        let root = ui.root();
        ui.set_layout(
            root,
            LayoutStyle {
                width: Length::Percent(1.0),
                height: Length::Percent(1.0),
                ..Default::default()
            },
        )?;
        let padding = ui.add_padding(root, Insets::all(12.0))?;
        ui.set_layout(
            padding,
            LayoutStyle {
                grow: 1.0,
                ..Default::default()
            },
        )?;
        let content = ui.add_column(padding)?;
        ui.set_layout(
            content,
            LayoutStyle {
                grow: 1.0,
                ..Default::default()
            },
        )?;
        let title = ui.add_label(content, "Workflow Studio")?;
        ui.set_widget_style(
            title,
            WidgetStyle {
                font_size: Some(22.0),
                ..Default::default()
            },
        )?;
        let toolbar = ui.add_row(content)?;
        let import = ui.button(toolbar, "Import graph/image…").finish();
        ui.on_click(import, |event| event.emit(Message::Import));
        let export = ui.button(toolbar, "Export graph…").finish();
        ui.on_click(export, |event| event.emit(Message::Export));
        let live = ui.button(toolbar, "Toggle live data").finish();
        ui.on_click(live, |event| event.emit(Message::ToggleLive));
        let follow = ui.button(toolbar, "Follow latest").finish();
        ui.on_click(follow, |event| event.emit(Message::FollowLatest));
        self.status = Some(ui.add_label(toolbar, "Ready — drag nodes, ports, and the chart")?);

        let workspace = ui.add_row(content)?;
        ui.set_layout(
            workspace,
            LayoutStyle {
                grow: 1.0,
                min_height: Length::Px(0.0),
                ..Default::default()
            },
        )?;
        let graph = ui.add_widget(
            workspace,
            NodeGraphView::new(
                self.graph.clone(),
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
        self.graph_view = Some(graph);

        let side = ui.add_column(workspace)?;
        ui.set_layout(
            side,
            LayoutStyle {
                width: Length::Px(390.0),
                shrink: 0.0,
                ..Default::default()
            },
        )?;
        let chart = ui.add_widget(
            side,
            ChartView::new(
                sample_series(),
                ChartOptions {
                    title: "Pipeline telemetry".into(),
                    x_axis: AxisOptions {
                        label: "Sample".into(),
                        ..Default::default()
                    },
                    y_axis: AxisOptions {
                        label: "Value".into(),
                        range: Some((0.0, 100.0)),
                        ..Default::default()
                    },
                    interaction: ChartInteractionOptions {
                        pan: ChartAxes::Horizontal,
                        zoom: ChartAxes::Horizontal,
                        bounds: Some(ChartViewport::new(0.0, 1_000_000.0, 0.0, 100.0)?),
                        follow_latest_x: Some(240.0),
                    },
                    ..Default::default()
                },
                Message::Chart,
            )?,
        )?;
        ui.set_layout(
            chart,
            LayoutStyle {
                width: Length::Percent(1.0),
                height: Length::Px(265.0),
                ..Default::default()
            },
        )?;
        self.chart_view = Some(chart);

        let image = ui.add_widget(
            side,
            ImageView::new(checkerboard(), "Imported asset preview").fit(ImageFit::Cover),
        )?;
        ui.set_layout(
            image,
            LayoutStyle {
                grow: 1.0,
                width: Length::Percent(1.0),
                min_height: Length::Px(160.0),
                ..Default::default()
            },
        )?;
        self.image_view = Some(image);

        cx.open_window(
            WindowConfig::new("RXUI Workflow Studio").size(1280.0, 760.0),
            ui,
        )?;
        Ok(())
    }

    fn update(&mut self, cx: &mut AppCx<'_, Message>, message: Message) -> rxui::Result<()> {
        match message {
            Message::Graph(action) => self.apply_graph_action(cx, action)?,
            Message::Chart(action) => {
                let handle = self.chart_view.expect("chart exists");
                match action {
                    ChartAction::Select(selection) => cx
                        .source_ui()?
                        .update_widget(handle, |view| view.set_selection(selection))?,
                    ChartAction::SetViewport(viewport) => {
                        let mut result = Ok(());
                        cx.source_ui()?
                            .update_widget(handle, |view| result = view.set_viewport(viewport))?;
                        result?;
                    }
                    ChartAction::ResetViewport => {
                        let viewport = ChartViewport::fit(&sample_series());
                        let mut result = Ok(());
                        cx.source_ui()?
                            .update_widget(handle, |view| result = view.set_viewport(viewport))?;
                        result?;
                    }
                }
            }
            Message::ToggleLive => {
                if let Some(timer) = self.live_timer.take() {
                    cx.cancel_timer(timer);
                    self.set_status(cx, "Live updates paused")?;
                } else {
                    self.live_timer =
                        Some(cx.set_interval(Duration::from_millis(120), Message::LiveTick));
                    self.set_status(
                        cx,
                        "Live updates running — manual X navigation pauses follow",
                    )?;
                }
            }
            Message::LiveTick => {
                let x = self.next_sample as f64;
                self.next_sample += 1;
                let y = (x * 0.025).sin() * 32.0 + 52.0;
                let sample = 45.0 + ((self.next_sample * 37 % 29) as f64);
                let handle = self.chart_view.expect("chart exists");
                let mut result = Ok(());
                cx.source_ui()?.update_widget(handle, |view| {
                    result = view.append_point("filtered", ChartPoint::new(x, y));
                    if result.is_ok() {
                        result = view.append_point("samples", ChartPoint::new(x, sample));
                    }
                })?;
                result?;
            }
            Message::FollowLatest => {
                let handle = self.chart_view.expect("chart exists");
                cx.source_ui()?.update_widget(handle, |view| {
                    let _ = view.resume_follow_latest();
                })?;
                self.set_status(cx, "Following latest chart samples")?;
            }
            Message::Import => {
                let proxy = cx.proxy();
                self.services.pick_file_contents(
                    FileDialogOptions::new()
                        .title("Import graph or image")
                        .filter("Workflow or image", &["json", "png", "jpg", "jpeg", "webp"]),
                    move |result| {
                        let _ = proxy.post(Message::FilePicked(result));
                    },
                );
            }
            Message::Export => {
                let bytes: Arc<[u8]> = serde_json::to_vec_pretty(&self.graph)?.into();
                let proxy = cx.proxy();
                self.services.save_bytes(
                    FileDialogOptions::new()
                        .title("Export graph")
                        .file_name("workflow.json")
                        .filter("JSON", &["json"]),
                    bytes,
                    move |result| {
                        let _ = proxy.post(Message::Saved(result));
                    },
                );
            }
            Message::FilePicked(Ok(Some(selected))) => self.import_file(cx, selected)?,
            Message::FilePicked(Ok(None)) => self.set_status(cx, "Import cancelled")?,
            Message::FilePicked(Err(error)) => {
                self.set_status(cx, format!("Import failed: {error}"))?
            }
            Message::Saved(Ok(Some(saved))) => {
                self.set_status(cx, format!("Saved {}", saved.name))?
            }
            Message::Saved(Ok(None)) => self.set_status(cx, "Export cancelled")?,
            Message::Saved(Err(error)) => self.set_status(cx, format!("Export failed: {error}"))?,
            Message::Reload(path) => {
                if let Some(task) = self.reload_task.take() {
                    cx.cancel_task(task);
                }
                self.set_status(cx, format!("Reloading {}…", path.display()))?;

                #[cfg(not(target_arch = "wasm32"))]
                {
                    let read_path = path.clone();
                    match cx.spawn_blocking(
                        move || std::fs::read(read_path),
                        move |outcome| {
                            let result = match outcome {
                                Ok(Ok(bytes)) => Ok(Some(SelectedFile {
                                    name: path.file_name().map_or_else(
                                        || path.display().to_string(),
                                        |name| name.to_string_lossy().into_owned(),
                                    ),
                                    bytes: bytes.into(),
                                    path: Some(path),
                                })),
                                Ok(Err(error)) => Err(ServiceError::Backend(error.to_string())),
                                Err(error) => Err(ServiceError::Backend(error.to_string())),
                            };
                            Message::Reloaded(result)
                        },
                    ) {
                        Ok(task) => self.reload_task = Some(task),
                        Err(error) => {
                            self.set_status(cx, format!("Could not queue reload: {error}"))?;
                        }
                    }
                }

                #[cfg(target_arch = "wasm32")]
                self.set_status(cx, "Native file reload is unavailable in the browser")?;
            }
            Message::Reloaded(result) => {
                self.reload_task = None;
                match result {
                    Ok(Some(selected)) => self.import_file(cx, selected)?,
                    Ok(None) => self.set_status(cx, "Reload cancelled")?,
                    Err(error) => self.set_status(cx, format!("Reload failed: {error}"))?,
                }
            }
        }
        Ok(())
    }
}

fn sample_graph() -> NodeGraphDocument<u64> {
    let output = |id, label: &str| GraphPort {
        id,
        label: label.into(),
        direction: GraphPortDirection::Output,
    };
    let input = |id, label: &str| GraphPort {
        id,
        label: label.into(),
        direction: GraphPortDirection::Input,
    };
    NodeGraphDocument {
        format_version: 1,
        viewport: GraphViewport::default(),
        nodes: vec![
            GraphNode {
                id: 1,
                title: "Sensor input".into(),
                position: GraphPoint::new(32.0, 80.0),
                size: GraphSize::new(170.0, 112.0),
                ports: vec![output(101, "samples")],
            },
            GraphNode {
                id: 2,
                title: "Low-pass filter".into(),
                position: GraphPoint::new(278.0, 48.0),
                size: GraphSize::new(190.0, 144.0),
                ports: vec![input(201, "input"), output(202, "filtered")],
            },
            GraphNode {
                id: 3,
                title: "Telemetry chart".into(),
                position: GraphPoint::new(548.0, 128.0),
                size: GraphSize::new(180.0, 112.0),
                ports: vec![input(301, "series")],
            },
        ],
        edges: vec![
            GraphEdge {
                id: 1001,
                from: GraphEndpoint { node: 1, port: 101 },
                to: GraphEndpoint { node: 2, port: 201 },
            },
            GraphEdge {
                id: 1002,
                from: GraphEndpoint { node: 2, port: 202 },
                to: GraphEndpoint { node: 3, port: 301 },
            },
        ],
    }
}

fn sample_series() -> Vec<ChartSeries> {
    let line = (0..1_000)
        .map(|x| ChartPoint::new(x as f64, (x as f64 * 0.025).sin() * 32.0 + 52.0))
        .collect();
    let scatter = (0..64)
        .map(|x| ChartPoint::new((x * 16) as f64, 45.0 + ((x * 37 % 29) as f64)))
        .collect();
    let bars = (0..12)
        .map(|x| ChartPoint::new((x * 85 + 40) as f64, (x * 13 % 42 + 12) as f64))
        .collect();
    vec![
        ChartSeries::new("filtered", "Filtered signal", ChartSeriesKind::Line, line),
        ChartSeries::new("samples", "Samples", ChartSeriesKind::Scatter, scatter),
        ChartSeries::new("load", "Load", ChartSeriesKind::Bar { width: 24.0 }, bars),
    ]
}

fn checkerboard() -> Image {
    let size = 96_u32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let light = ((x / 12) + (y / 12)) % 2 == 0;
            rgba.extend(if light {
                [76, 141, 255, 255]
            } else {
                [27, 27, 31, 255]
            });
        }
    }
    Image::from_rgba8(Size::new(size, size), rgba).expect("checkerboard is valid")
}

fn workflow_theme() -> Theme {
    Theme {
        font_families: vec![astrelis_text::FontFamily::Named("Noto Sans".into())],
        ..Theme::dark()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> MainResult {
    run_with(
        WorkflowStudio::new(),
        AppConfig::default().theme(workflow_theme()),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
/// Starts Workflow Studio in the page's `#rxui-canvas` element.
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("browser document is unavailable"))?;
    let canvas = document
        .get_element_by_id("rxui-canvas")
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("#rxui-canvas was not found"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| wasm_bindgen::JsValue::from_str("#rxui-canvas is not a canvas"))?;
    rxui::app::spawn_on_canvas(
        WorkflowStudio::new(),
        AppConfig::default().theme(workflow_theme()),
        canvas,
    )
    .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
}
