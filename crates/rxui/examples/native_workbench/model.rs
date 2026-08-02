//! The workbench component: docking, charts, a graph, and every app surface.
//!
//! This module is deliberately free of windowing, of the event loop, and of any
//! `astrelis-app` dependency, so it can be compiled into a headless integration
//! test as well as into the native binary. `crates/rxui/tests/workbench.rs`
//! includes this exact file with `#[path]` and drives it through `Harness`.

// The wasm build of this example has an empty `main`, so nothing here is
// reachable there.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use rxui::{
    Axis, ButtonStyle, ButtonVariant, Component, ComponentContext, ContainerStyle, FrameStyle,
    IconButtonStyle, Space, StackStyle, Theme, View,
    charts::{ChartAction, ChartPoint, ChartSeries, ChartSeriesKind, ChartSpec, chart},
    color::Color,
    controls::{Choice, radio_group},
    docking::{DockNode, DockPane, dock_workspace},
    forms::form_section,
    geometry::{LogicalPoint, LogicalSize},
    graph::{GraphEdge, GraphNode, NodeGraphAction, NodeGraphSpec, node_graph},
    icons, stack_with,
    surfaces::{
        CommandItem, CommandPaletteNavigation, DialogAction, Toast, ToastLevel, ToolbarItem,
        command_palette, dialog, toasts, toolbar,
    },
};

/// Identity of the split holding the two dock groups.
pub const ROOT_SPLIT: u64 = 10;
/// Identity of the pane showing the chart.
pub const CHART_PANE: u64 = 1;
/// Identity of the pane showing the node graph.
pub const GRAPH_PANE: u64 = 2;

/// Which surface a dock pane renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pane {
    /// Interactive time-series chart.
    Chart,
    /// Interactive node graph.
    Graph,
}

/// Every intent the workbench can report.
#[derive(Clone, Debug)]
pub enum Action {
    /// Activate a dock pane inside its tab group.
    SelectPane(u64),
    /// Replace a split ratio.
    Resize(u64, f32),
    /// A chart interaction.
    Chart(ChartAction),
    /// A node-graph interaction.
    Graph(NodeGraphAction<u64>),
    /// Choose an interaction mode in the settings dialog.
    SetMode(String),
    /// Open the settings dialog.
    OpenDialog,
    /// Dismiss the settings dialog.
    CloseDialog,
    /// Show or hide the command palette.
    TogglePalette,
    /// Move the palette selection backwards.
    PreviousCommand,
    /// Move the palette selection forwards.
    NextCommand,
    /// Replace the palette query.
    Query(String),
    /// Persist the workspace and raise a toast.
    Save,
    /// Dismiss the toast.
    ClearToast,
}

/// The whole workbench application state.
pub struct Workbench {
    /// Persistent dock layout.
    pub layout: DockNode<Pane>,
    /// Graph node the user last selected.
    pub selected_node: Option<u64>,
    /// Chart series and point index the user last selected.
    pub selected_series: Option<(u64, usize)>,
    /// Chosen interaction mode.
    pub mode: String,
    /// Whether the settings dialog is open.
    pub dialog_open: bool,
    /// Whether the command palette is open.
    pub palette_open: bool,
    /// Palette row the user has highlighted.
    pub selected_command: usize,
    /// Current palette query.
    pub query: String,
    /// Pending status toast.
    pub toast: Option<Toast<Action>>,
}

impl Workbench {
    /// Creates the workbench with the chart and the graph docked side by side.
    pub fn new() -> Self {
        Self {
            layout: DockNode::Split {
                id: ROOT_SPLIT,
                axis: Axis::Horizontal,
                ratio: 0.5,
                first: Box::new(DockNode::Tabs {
                    id: 11,
                    active: CHART_PANE,
                    panes: vec![DockPane {
                        id: CHART_PANE,
                        title: "Chart".into(),
                        value: Pane::Chart,
                    }],
                }),
                second: Box::new(DockNode::Tabs {
                    id: 12,
                    active: GRAPH_PANE,
                    panes: vec![DockPane {
                        id: GRAPH_PANE,
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
            selected_command: 0,
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
            Action::TogglePalette => {
                self.palette_open = !self.palette_open;
                self.selected_command = 0;
            }
            Action::PreviousCommand => {
                self.selected_command = (self.selected_command + 1) % 2;
            }
            Action::NextCommand => {
                self.selected_command = (self.selected_command + 1) % 2;
            }
            Action::Query(query) => {
                self.query = query;
                self.selected_command = 0;
            }
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
            ToolbarItem::IconCommand {
                id: "save".into(),
                icon: icons::save(),
                label: "Save".into(),
                action: Action::Save,
                enabled: true,
                style: IconButtonStyle::compact()
                    .button(ButtonStyle::standard().variant(ButtonVariant::Primary))
                    .show_label(true),
            },
            ToolbarItem::Separator,
            ToolbarItem::IconCommand {
                id: "commands".into(),
                icon: icons::search(),
                label: "Commands".into(),
                action: Action::TogglePalette,
                enabled: true,
                style: IconButtonStyle::compact()
                    .button(ButtonStyle::standard().variant(ButtonVariant::Quiet)),
            },
            ToolbarItem::IconCommand {
                id: "settings".into(),
                icon: icons::settings(),
                label: "Settings".into(),
                action: Action::OpenDialog,
                enabled: true,
                style: IconButtonStyle::compact(),
            },
        ];
        let base = rxui::column_with(
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
                    Choice::new("edit", "Edit".to_string(), "Edit"),
                    Choice::new("inspect", "Inspect".to_string(), "Inspect"),
                    Choice::new("present", "Present".to_string(), "Present"),
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
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    action: Action::CloseDialog,
                    variant: ButtonVariant::Quiet,
                },
                DialogAction {
                    id: "save".into(),
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
            self.selected_command,
            Action::Query,
            CommandPaletteNavigation {
                dismiss: Action::TogglePalette,
                previous: Action::PreviousCommand,
                next: Action::NextCommand,
            },
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
