//! Native editor-style workbench exercising the migrated UI catalog.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalSize, Size},
};
use astrelis_platform::WindowAttributes;
use rxui_next::{
    ButtonVariant, ChartAction, ChartPoint, ChartSeries, ChartSeriesKind, ChartSpec, Choice,
    CommandItem, Component, ComponentContext, ContainerStyle, DialogAction, DockAxis, DockNode,
    DockPane, FrameStyle, GraphEdge, GraphNode, NodeGraphAction, NodeGraphSpec, Space, StackStyle,
    Theme, Toast, ToastLevel, ToolbarItem, View, WindowHostOptions, chart, command_palette, dialog,
    dock_workspace, form_section, node_graph, radio_group, run_component, stack_with, toasts,
    toolbar,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Pane {
    Chart,
    Graph,
}

#[derive(Clone)]
enum Action {
    SelectPane(u64),
    Resize(u64, f32),
    Chart(ChartAction),
    Graph(NodeGraphAction<u64>),
    SetMode(String),
    OpenDialog,
    CloseDialog,
    TogglePalette,
    Query(String),
    Save,
    ClearToast,
}

struct Workbench {
    layout: DockNode<Pane>,
    selected_node: Option<u64>,
    selected_series: Option<(u64, usize)>,
    mode: String,
    dialog_open: bool,
    palette_open: bool,
    query: String,
    toast: Option<Toast<Action>>,
}

impl Workbench {
    fn new() -> Self {
        Self {
            layout: DockNode::Split {
                id: 10,
                axis: DockAxis::Horizontal,
                ratio: 0.5,
                first: Box::new(DockNode::Tabs {
                    id: 11,
                    active: 1,
                    panes: vec![DockPane {
                        id: 1,
                        title: "Chart".into(),
                        value: Pane::Chart,
                    }],
                }),
                second: Box::new(DockNode::Tabs {
                    id: 12,
                    active: 2,
                    panes: vec![DockPane {
                        id: 2,
                        title: "Graph".into(),
                        value: Pane::Graph,
                    }],
                }),
            },
            selected_node: None,
            selected_series: None,
            mode: "Edit".into(),
            dialog_open: false,
            palette_open: false,
            query: String::new(),
            toast: None,
        }
    }
}

impl Component for Workbench {
    type Action = Action;
    type Effect = ();

    fn update(&mut self, action: Self::Action, _context: &mut ComponentContext<'_, ()>) {
        match action {
            Action::SelectPane(pane) => {
                self.layout.select(pane);
            }
            Action::Resize(split, ratio) => {
                self.layout.set_ratio(split, ratio);
            }
            Action::Chart(ChartAction::Select { series, point }) => {
                self.selected_series = Some((series, point));
            }
            Action::Chart(ChartAction::Clear) => self.selected_series = None,
            Action::Graph(NodeGraphAction::Select(node)) => self.selected_node = Some(node),
            Action::Graph(NodeGraphAction::ClearSelection) => self.selected_node = None,
            Action::SetMode(mode) => self.mode = mode,
            Action::OpenDialog => {
                self.dialog_open = true;
                self.palette_open = false;
            }
            Action::CloseDialog => self.dialog_open = false,
            Action::TogglePalette => self.palette_open = !self.palette_open,
            Action::Query(query) => self.query = query,
            Action::Save => {
                self.dialog_open = false;
                self.palette_open = false;
                self.toast = Some(Toast {
                    id: 1,
                    message: "Workspace saved".into(),
                    level: ToastLevel::Success,
                    action: Some(("Dismiss".into(), Action::ClearToast)),
                });
            }
            Action::ClearToast => self.toast = None,
        }
    }

    fn view(&self, _theme: &Theme) -> View<Self::Action> {
        let series = vec![
            ChartSeries {
                id: 1,
                name: "Requests".into(),
                color: Color::from_hex(0x4c8dff),
                kind: ChartSeriesKind::Line,
                points: (0..32)
                    .map(|x| ChartPoint {
                        x: f64::from(x),
                        y: f64::from((x * 7) % 19),
                    })
                    .collect(),
            },
            ChartSeries {
                id: 2,
                name: "Errors".into(),
                color: Color::from_hex(0xe05b65),
                kind: ChartSeriesKind::Scatter,
                points: (0..12)
                    .map(|x| ChartPoint {
                        x: f64::from(x * 2),
                        y: f64::from((x * 5) % 11),
                    })
                    .collect(),
            },
        ];
        let graph_nodes = vec![
            GraphNode {
                id: 1,
                title: "Input".into(),
                position: LogicalPoint::new(30.0, 80.0),
                size: LogicalSize::new(120.0, 60.0),
            },
            GraphNode {
                id: 2,
                title: "Transform".into(),
                position: LogicalPoint::new(220.0, 130.0),
                size: LogicalSize::new(140.0, 60.0),
            },
            GraphNode {
                id: 3,
                title: "Output".into(),
                position: LogicalPoint::new(430.0, 70.0),
                size: LogicalSize::new(120.0, 60.0),
            },
        ];
        let graph_edges = vec![GraphEdge { from: 1, to: 2 }, GraphEdge { from: 2, to: 3 }];
        let selected_node = self.selected_node;
        let render = move |pane: &DockPane<Pane>| match pane.value {
            Pane::Chart => chart(ChartSpec::new(series.clone(), Action::Chart)),
            Pane::Graph => {
                let mut spec =
                    NodeGraphSpec::new(graph_nodes.clone(), graph_edges.clone(), Action::Graph);
                spec.selected = selected_node;
                node_graph(spec)
            }
        };
        let workspace = dock_workspace(&self.layout, render, Action::SelectPane, Action::Resize);
        let commands = vec![
            ToolbarItem::Command {
                label: "Save".into(),
                action: Action::Save,
                enabled: true,
                variant: ButtonVariant::Primary,
            },
            ToolbarItem::Separator,
            ToolbarItem::Command {
                label: "Commands".into(),
                action: Action::TogglePalette,
                enabled: true,
                variant: ButtonVariant::Quiet,
            },
            ToolbarItem::Command {
                label: "Settings".into(),
                action: Action::OpenDialog,
                enabled: true,
                variant: ButtonVariant::Standard,
            },
        ];
        let base = rxui_next::column_with(
            ContainerStyle::new().gap(Space::Xs),
            (
                toolbar(&commands).key("toolbar"),
                workspace
                    .frame(FrameStyle::new().grow(1.0))
                    .key("workspace"),
            ),
        );
        let settings = form_section(
            "Interaction mode",
            radio_group(
                &[
                    Choice::new("Edit".to_string(), "Edit"),
                    Choice::new("Inspect".to_string(), "Inspect"),
                    Choice::new("Present".to_string(), "Present"),
                ],
                Some(&self.mode),
                Action::SetMode,
            ),
        );
        let main = dialog(
            self.dialog_open,
            "Workspace settings",
            base,
            settings,
            Action::CloseDialog,
            &[
                DialogAction {
                    label: "Cancel".into(),
                    action: Action::CloseDialog,
                    variant: ButtonVariant::Quiet,
                },
                DialogAction {
                    label: "Save".into(),
                    action: Action::Save,
                    variant: ButtonVariant::Primary,
                },
            ],
        );
        let palette_items = vec![
            CommandItem {
                id: "save".into(),
                label: "Save workspace".into(),
                description: Some("Persist the current workspace".into()),
                action: Action::Save,
                enabled: true,
            },
            CommandItem {
                id: "settings".into(),
                label: "Open settings".into(),
                description: None,
                action: Action::OpenDialog,
                enabled: true,
            },
        ];
        let palette = command_palette(
            self.palette_open,
            &self.query,
            &palette_items,
            0,
            Action::Query,
            Action::TogglePalette,
        );
        let toast_items = self.toast.iter().cloned().collect::<Vec<_>>();
        stack_with(
            StackStyle::new(),
            (
                main.enabled(!self.palette_open),
                palette,
                toasts(&toast_items),
            ),
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), astrelis_app::RuntimeError<std::io::Error>> {
    run_component(
        Workbench::new(),
        Theme::dark(),
        WindowHostOptions {
            window: WindowAttributes {
                title: "RXUI Next workbench".into(),
                inner_size: Some(Size::new(1100.0, 720.0)),
                ..WindowAttributes::default()
            },
            ..WindowHostOptions::default()
        },
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {}
