//! Native multi-viewport docking.
//!
//! Drag the Inspector tab into empty workspace space to detach it into a
//! native window. Closing that secondary window docks the panel back into the
//! primary viewport.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::collections::BTreeMap;

use astrelis_core::geometry::Size;
use astrelis_platform::WindowAttributes;
use rxui::{
    editor::{
        DockAction, DockFloatingMode, DockLayout, DockNode, DockPlacement, DockSide, DockStyle,
        DockTabs, DockViewport, DockViewportId, DockWorkspace, MultiViewportDockLayout,
        PanelDescriptor, PanelId, PreferredPlacement,
    },
    prelude::*,
};

fn panel(value: &str) -> PanelId {
    PanelId::new(value).expect("static panel identity")
}

fn viewport(value: &str) -> DockViewportId {
    DockViewportId::new(value).expect("static viewport identity")
}

fn initial_layout() -> DockLayout {
    DockLayout {
        root: Some(DockNode::Split {
            axis: rxui::editor::DockAxis::Horizontal,
            ratio: 0.68,
            first: Box::new(DockNode::Tabs(
                DockTabs::new(vec![panel("scene")]).expect("scene tab"),
            )),
            second: Box::new(DockNode::Tabs(
                DockTabs::new(vec![panel("inspector")]).expect("inspector tab"),
            )),
        }),
        floating: Vec::new(),
    }
}

#[derive(Clone, Debug)]
enum Message {
    Dock(DockAction),
}

struct HostedViewport {
    logical: DockViewportId,
    workspace: DockWorkspace<Message>,
}

struct MultiViewportExample {
    layout: MultiViewportDockLayout,
    windows: BTreeMap<WindowId, HostedViewport>,
    next_viewport: u64,
}

impl MultiViewportExample {
    fn new() -> Self {
        let primary = DockViewport::new(viewport("main"), "RXUI docking", initial_layout());
        Self {
            layout: MultiViewportDockLayout::new(primary),
            windows: BTreeMap::new(),
            next_viewport: 1,
        }
    }

    fn build_viewport_ui(
        cx: &mut AppCx<'_, Message>,
        viewport: &DockViewport,
    ) -> rxui::Result<(Ui<Message>, DockWorkspace<Message>)> {
        let mut ui = cx.new_ui();
        let root = ui.root();
        ui.set_layout(
            root,
            LayoutStyle {
                width: Length::Percent(1.0),
                height: Length::Percent(1.0),
                ..Default::default()
            },
        )?;
        let dock_host = ui.add_column(root)?;
        ui.set_layout(
            dock_host,
            LayoutStyle {
                grow: 1.0,
                width: Length::Percent(1.0),
                height: Length::Percent(1.0),
                ..Default::default()
            },
        )?;

        let mut workspace = DockWorkspace::new(
            &mut ui,
            dock_host,
            DockStyle {
                floating_mode: DockFloatingMode::NativeViewport,
                ..DockStyle::default()
            },
            Message::Dock,
        )?;
        for panel_id in viewport.layout.panels() {
            let content = ui.add_column(root)?;
            match panel_id.as_str() {
                "scene" => {
                    ui.add_label(content, "Scene viewport")?;
                    ui.add_label(
                        content,
                        "Drag the Inspector tab into empty workspace space to detach it.",
                    )?;
                }
                "inspector" => {
                    ui.add_label(content, "Inspector")?;
                    ui.add_label(content, "Closing this window docks the panel back.")?;
                }
                _ => {
                    ui.add_label(content, panel_id.as_str())?;
                }
            }
            let title = match panel_id.as_str() {
                "scene" => "Scene",
                "inspector" => "Inspector",
                value => value,
            };
            workspace.register_panel(
                &mut ui,
                PanelDescriptor::new(panel_id.clone(), title).preferred(
                    PreferredPlacement::Split {
                        anchor: panel("scene"),
                        side: DockSide::Right,
                    },
                ),
                content,
            )?;
        }
        workspace.restore(&mut ui, viewport.layout.clone(), viewport.layout.clone())?;
        Ok((ui, workspace))
    }

    fn open_viewport(
        &mut self,
        cx: &mut AppCx<'_, Message>,
        logical: DockViewportId,
        suggested_size: Option<Size<astrelis_core::geometry::Logical, f64>>,
    ) -> rxui::Result<WindowId> {
        let viewport = self
            .layout
            .viewport(&logical)
            .cloned()
            .ok_or_else(|| rxui::Error::msg(format!("missing viewport {logical}")))?;
        let (ui, workspace) = Self::build_viewport_ui(cx, &viewport)?;
        let window = cx.open_window(
            WindowConfig::default().attributes(WindowAttributes {
                title: viewport.title.clone(),
                inner_size: suggested_size.or(Some(Size::new(900.0, 600.0))),
                ..WindowAttributes::default()
            }),
            ui,
        )?;
        self.windows
            .insert(window, HostedViewport { logical, workspace });
        Ok(window)
    }
}

impl App for MultiViewportExample {
    type Message = Message;

    fn build(&mut self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        self.open_viewport(cx, viewport("main"), None)?;
        Ok(())
    }

    fn update(&mut self, cx: &mut AppCx<'_, Message>, message: Message) -> rxui::Result<()> {
        let window = cx
            .source_window()
            .ok_or_else(|| rxui::Error::msg("docking message has no source window"))?;
        let Message::Dock(action) = message;
        let (logical, current_layout, native_request) = {
            let hosted = self
                .windows
                .get_mut(&window)
                .ok_or_else(|| rxui::Error::msg("docking source window is no longer open"))?;
            let outcome = hosted.workspace.apply(cx.ui(window)?, action)?;
            (
                hosted.logical.clone(),
                hosted.workspace.layout().clone(),
                outcome.native_viewport,
            )
        };

        if let Some(request) = native_request {
            let detached = DockViewportId::new(format!("detached-{}", self.next_viewport))?;
            self.next_viewport += 1;
            self.layout.detach_panel(
                request.panel.clone(),
                detached.clone(),
                request.panel.to_string(),
            )?;
            self.layout
                .viewport_mut(&logical)
                .expect("source viewport exists")
                .layout = current_layout;
            self.open_viewport(
                cx,
                detached,
                Some(Size::new(
                    f64::from(request.width.max(240)),
                    f64::from(request.height.max(180)),
                )),
            )?;
        } else {
            self.layout
                .viewport_mut(&logical)
                .expect("source viewport exists")
                .layout = current_layout;
        }
        Ok(())
    }

    fn close_requested(
        &mut self,
        cx: &mut AppCx<'_, Message>,
        window: WindowId,
    ) -> rxui::Result<CloseResponse> {
        let Some(hosted) = self.windows.get(&window) else {
            return Ok(CloseResponse::Close);
        };
        if &hosted.logical == self.layout.primary() {
            for other in cx
                .windows()
                .into_iter()
                .filter(|candidate| *candidate != window)
            {
                cx.close_window(other)?;
                self.windows.remove(&other);
            }
        }
        Ok(CloseResponse::Close)
    }

    fn window_closed(&mut self, cx: &mut AppCx<'_, Message>, window: WindowId) -> rxui::Result<()> {
        let Some(closed) = self.windows.remove(&window) else {
            return Ok(());
        };
        if &closed.logical == self.layout.primary() {
            return Ok(());
        }
        let Some(panel_id) = self
            .layout
            .viewport(&closed.logical)
            .and_then(|viewport| viewport.layout.panels().first().cloned())
            .cloned()
        else {
            return Ok(());
        };
        let main = self.layout.primary().clone();
        self.layout.place_panel(
            panel_id.clone(),
            &main,
            DockPlacement::Tab {
                anchor: panel("scene"),
                index: usize::MAX,
            },
        )?;
        self.layout.remove_empty_viewport(&closed.logical)?;
        let main_window = self
            .windows
            .iter_mut()
            .find(|(_, hosted)| hosted.logical == main)
            .map(|(window, hosted)| (*window, hosted))
            .ok_or_else(|| rxui::Error::msg("primary viewport is not open"))?;
        main_window
            .1
            .workspace
            .open(cx.ui(main_window.0)?, &panel_id)?;
        self.layout
            .viewport_mut(&main)
            .expect("primary viewport exists")
            .layout = main_window.1.workspace.layout().clone();
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> MainResult {
    run(MultiViewportExample::new())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
