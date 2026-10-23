//! Application-owned regions and independent native windows sharing document models.
//! Detach with a button or release outside all dock regions. Reattach with a button
//! or close the detached window. Enable `native-docking` for experimental native
//! cross-window dragging; `--two-windows` starts with a separate destination.
use rxui::{host::CloseResponse, prelude::*};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

struct Document(String);
impl View for Document {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        text_input(self.0.clone())
            .fill_width()
            .on_change(cx.listener(|s, e: &TextChangeEvent, _| s.0 = e.value.clone()))
    }
}
struct Workspace {
    layout: DockWorkspace,
    documents: HashMap<Key, Entity<Document>>,
    windows: HashMap<DockSurfaceId, WindowHandle>,
    main_window: Option<WindowHandle>,
    home: DockSurfaceId,
    status: String,
}
fn first_group(node: &DockNode) -> DockNodeId {
    match node {
        DockNode::Tabs(n) => n.id(),
        DockNode::Split(n) => first_group(n.first()),
    }
}
fn panels(node: &DockNode, out: &mut Vec<(DockNodeId, Key)>) {
    match node {
        DockNode::Tabs(n) => out.extend(n.panels().iter().cloned().map(|key| (n.id(), key))),
        DockNode::Split(n) => {
            panels(n.first(), out);
            panels(n.second(), out);
        }
    }
}
impl Workspace {
    // Closing a detached window returns all remaining panels to the main region.
    // Stage the whole batch so failure never leaves a partially reattached window.
    fn reattach(&mut self, surface: DockSurfaceId) -> Result<(), DockWorkspaceError> {
        let Some(tree) = self.layout.surface(surface) else {
            return Ok(());
        };
        let mut entries = Vec::new();
        panels(tree.root(), &mut entries);
        let mut staged = self.layout.clone();
        let group = first_group(staged.surface(self.home).unwrap().root());
        for (source, panel) in entries {
            let index = staged
                .surface(self.home)
                .unwrap()
                .node(group)
                .unwrap()
                .tabs()
                .unwrap()
                .panels()
                .len();
            staged.transfer(
                surface,
                source,
                &panel,
                self.home,
                DockDropTarget::Tab { group, index },
            )?;
        }
        staged.remove_surface(surface)?;
        self.layout = staged;
        self.windows.remove(&surface);
        self.status = "Panels returned to the main region; document text retained.".into();
        Ok(())
    }
}
struct WindowView {
    workspace: Entity<Workspace>,
    surfaces: Vec<DockSurfaceId>,
    detached: bool,
}
impl WindowView {
    fn detach(
        &mut self,
        surface: DockSurfaceId,
        group: DockNodeId,
        panel: &Key,
        cx: &mut Context<'_, Self>,
    ) {
        let workspace = self.workspace.clone();
        // Queue the window before publishing the staged layout. Queueing failure
        // leaves the original layout intact. Native creation failures remain host errors.
        let result = workspace.update(cx, |s, cx| -> Result<(), String> {
            let mut staged = s.layout.clone();
            let change = staged
                .detach(surface, group, panel)
                .map_err(|e| e.to_string())?;
            let root = cx.new(|_| WindowView {
                workspace: workspace.clone(),
                surfaces: vec![change.destination_surface],
                detached: true,
            });
            let window = cx
                .open_window(
                    WindowOptions::new()
                        .title("RXUI — detached surface")
                        .size(650., 480.),
                    root,
                )
                .map_err(|e| e.to_string())?;
            s.layout = staged;
            s.windows.insert(change.destination_surface, window);
            s.status = "Detached. Edit the document, then reattach or close its window.".into();
            Ok(())
        });
        if let Err(error) = result {
            workspace.update(cx, |s, _| s.status = error);
        }
    }
}
impl View for WindowView {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let mut regions = row()
            .fill_width()
            .fill_height()
            .min_width(0.)
            .min_height(0.)
            .gap(8.);
        for &surface in &self.surfaces {
            let Some(tree) = workspace.layout.surface(surface) else {
                continue;
            };
            let group = first_group(tree.root());
            let selected = tree
                .node(group)
                .unwrap()
                .tabs()
                .unwrap()
                .selected()
                .cloned();
            let content = dock(tree, |key| {
                let document = workspace.documents[key].clone();
                dock_panel_lazy(format!("{key:?}"), move || document).closable(true)
            })
            .surface(surface)
            .transfer_scope("example-workspace")
            .min_pane_size(120., 100.)
            .on_event(cx.listener(move |s, e: &DockEvent, cx| {
                s.workspace.update(cx, |w, _| {
                    if let Err(error) = w.layout.apply(surface, e) {
                        w.status = error.to_string();
                    }
                });
            }))
            .on_transfer(cx.listener(|s, e: &DockTransferEvent, cx| {
                s.workspace.update(cx, |w, cx| {
                    match w.layout.transfer(
                        e.source_surface,
                        e.source_group,
                        &e.panel,
                        e.target_surface,
                        e.target,
                    ) {
                        Ok(change) => {
                            eprintln!("docking transfer accepted: panel={:?}, source={:?}, target={:?}, source_empty={}", e.panel, e.source_surface, e.target_surface, change.source_empty);
                            w.status =
                                "Panel transferred; application accepted the checked proposal."
                                    .into();
                            if change.source_empty
                                && let Some(window) = w.windows.remove(&change.source_surface)
                            {
                                w.layout.remove_surface(change.source_surface).unwrap();
                                if let Err(error) = cx.close_window(&window) {
                                    w.status = error.to_string();
                                }
                            }
                        }
                        Err(error) => w.status = error.to_string(),
                    }
                });
            }))
            .on_detach(cx.listener(|s, e: &DockDetachEvent, cx| {
                s.detach(e.source_surface, e.source_group, &e.panel, cx);
            }));
            let tools = if self.detached {
                row().child(
                    button("Reattach all").on_click(cx.listener(move |s, _, cx| {
                        let window = s.workspace.read(cx).windows.get(&surface).cloned();
                        let result = s.workspace.update(cx, |w, _| w.reattach(surface));
                        match result {
                            Ok(()) => {
                                if let Some(window) = window {
                                    cx.close_window(&window).unwrap();
                                }
                            }
                            Err(error) => {
                                s.workspace.update(cx, |w, _| w.status = error.to_string())
                            }
                        }
                    })),
                )
            } else {
                row().child(
                    button("Detach active").on_click(cx.listener(move |s, _, cx| {
                        if let Some(panel) = &selected {
                            s.detach(surface, group, panel, cx);
                        }
                    })),
                )
            };
            regions = regions.child(
                column()
                    .fill_width()
                    .fill_height()
                    .flex_grow(1.)
                    .min_width(0.)
                    .min_height(0.)
                    .child(tools)
                    .child(content),
            );
        }
        column().fill_width().fill_height().padding(10.).gap(10.)
            .child(label("Drag tabs between regions or windows with native-docking enabled. Close a detached window to reattach."))
            .child(label("Document text follows the panel. Caret, scroll and focus are placement-local."))
            .child(regions)
            .child(label(workspace.status.clone()))
    }
}
fn main() -> Result<(), ApplicationError> {
    let state: Rc<RefCell<Option<Entity<Workspace>>>> = Rc::new(RefCell::new(None));
    let close_state = state.clone();
    let two_windows = std::env::args().any(|arg| arg == "--two-windows" || arg == "--smoke");
    let smoke = std::env::args().any(|arg| arg == "--smoke");
    let app = Application::new();
    #[cfg(all(feature = "native-docking", not(target_arch = "wasm32")))]
    let app = app.multi_viewport_docking(true);
    app.window_created(move |window, _| {
        if two_windows && let Some(native) = window.native_window() {
            let x = if native.title().contains("destination") {
                760.
            } else {
                40.
            };
            native.set_outer_position(rxui::host::winit::dpi::LogicalPosition::new(x, 100.));
        }
        eprintln!("docking window created: {:?}", window.id());
    })
    .close_requested(move |window, cx| {
        if let Some(workspace) = close_state.borrow().as_ref() {
            let keep_main = {
                let state = workspace.read(cx);
                state
                    .main_window
                    .as_ref()
                    .is_some_and(|main| main.id() == window.id())
                    && !state.windows.is_empty()
            };
            if keep_main {
                workspace.update(cx, |w, _| {
                    w.status =
                        "Reattach or close the detached windows before closing the main window."
                            .into();
                });
                return CloseResponse::KeepOpen;
            }
            let surface = workspace
                .read(cx)
                .windows
                .iter()
                .find_map(|(surface, handle)| (handle.id() == window.id()).then_some(*surface));
            if let Some(surface) = surface {
                let result = workspace.update(cx, |w, _| w.reattach(surface));
                if let Err(error) = result {
                    workspace.update(cx, |w, _| w.status = error.to_string());
                    return CloseResponse::KeepOpen;
                }
            }
        }
        CloseResponse::Close
    })
    .run(move |cx| {
        let mut layout = DockWorkspace::new();
        let home = layout
            .create_surface(DockTree::from_panels(["editor", "preview"]).unwrap())
            .unwrap();
        let side = layout
            .create_surface(DockTree::from_panels(["output"]).unwrap())
            .unwrap();
        let documents = ["editor", "preview", "output"]
            .into_iter()
            .map(|name| {
                (
                    Key::from(name),
                    cx.new(|_| Document(format!("Edit {name}; this text follows its entity."))),
                )
            })
            .collect();
        let workspace = cx.new(|_| Workspace {
            layout,
            documents,
            windows: HashMap::new(),
            main_window: None,
            home,
            status: String::new(),
        });
        *state.borrow_mut() = Some(workspace.clone());
        let root = cx.new(|_| WindowView {
            workspace: workspace.clone(),
            surfaces: if two_windows {
                vec![home]
            } else {
                vec![home, side]
            },
            detached: false,
        });
        let window = cx.open_window(
            WindowOptions::new()
                .title("RXUI — docking surfaces")
                .size(if two_windows { 680. } else { 1100. }, 700.),
            root,
        )?;
        workspace.update(cx, |w, _| w.main_window = Some(window));
        if two_windows {
            let root = cx.new(|_| WindowView {
                workspace: workspace.clone(),
                surfaces: vec![side],
                detached: true,
            });
            let window = cx.open_window(
                WindowOptions::new()
                    .title("RXUI — docking destination")
                    .size(650., 480.),
                root,
            )?;
            workspace.update(cx, |w, _| {
                w.windows.insert(side, window);
            });
        }
        if smoke {
            cx.spawn(
                async {
                    async_io::Timer::after(std::time::Duration::from_secs(3)).await;
                },
                |_, cx| {
                    eprintln!("docking smoke: native creation completed; gestures not tested");
                    cx.exit().unwrap();
                },
            )
            .detach();
        }
        Ok(())
    })
}
