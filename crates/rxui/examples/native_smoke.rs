//! Self-terminating real-window smoke test used by the desktop CI matrix.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::{io, time::Duration};

use astrelis_app::{App, AppContext, Runtime, RuntimeConfig};
use astrelis_core::geometry::Size;
use astrelis_platform::{WindowAttributes, WindowEvent, WindowId};
use astrelis_text::FontDatabase;
use astrelis_ui_core::{SemanticAction, SemanticNode, SemanticRole};
use rxui::prelude::*;

#[derive(Clone, Copy)]
enum Message {
    Activate,
}

struct NativeSmoke {
    graphics: GraphicsContext,
    host: Option<WindowHost<Message>>,
    status: Option<ElementHandle<Label>>,
    redraws: usize,
    activated: bool,
}

impl NativeSmoke {
    fn new() -> Self {
        Self {
            graphics: GraphicsContext::new(),
            host: None,
            status: None,
            redraws: 0,
            activated: false,
        }
    }
}

impl App for NativeSmoke {
    type Error = io::Error;

    fn resumed(&mut self, context: &mut AppContext<'_, '_, Self>) -> Result<(), Self::Error> {
        if self.host.is_some() {
            return Ok(());
        }
        let mut ui = Ui::new(FontDatabase::default(), Theme::dark());
        let root = ui.root();
        let content = ui.padding(root, Insets::all(24.0)).column().finish();
        let activate = ui.button(content, "Run native smoke action").finish();
        let status = ui.label(content, "Waiting").finish();
        ui.on_click(activate, |context| context.emit(Message::Activate));
        let host = WindowHost::open(
            context,
            &self.graphics,
            ui,
            WindowHostOptions {
                window: WindowAttributes {
                    title: "RXUI native smoke".into(),
                    inner_size: Some(Size::new(420.0, 180.0)),
                    ..WindowAttributes::default()
                },
                ..WindowHostOptions::default()
            },
        )
        .map_err(io::Error::other)?;
        context.invalidate_window(host.id());
        context.set_timeout(Duration::from_secs(10), |_app, _context| {
            Err(io::Error::other(
                "native smoke did not complete within 10 seconds",
            ))
        });
        self.status = Some(status);
        self.host = Some(host);
        Ok(())
    }

    fn window_event(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
        event: WindowEvent,
    ) -> Result<(), Self::Error> {
        let Some(host) = &mut self.host else {
            return Ok(());
        };
        let update = host
            .handle_event(&context.clipboard(), &event)
            .map_err(io::Error::other)?;
        if update.close_requested {
            context.unregister_window(id);
            self.host = None;
            context.exit();
        } else if update.redraw {
            context.invalidate_window(id);
        }
        Ok(())
    }

    fn redraw(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
    ) -> Result<(), Self::Error> {
        let Some(host) = &mut self.host else {
            return Ok(());
        };
        host.redraw().map_err(io::Error::other)?;
        self.redraws += 1;
        if self.redraws == 1 {
            let tree = host.ui_mut().semantic_tree().map_err(io::Error::other)?;
            let button = find(&tree, SemanticRole::Button, "Run native smoke action")
                .ok_or_else(|| io::Error::other("native button semantics were not produced"))?;
            host.ui_mut()
                .perform_semantic_action(button.id, SemanticAction::Activate)
                .map_err(io::Error::other)?;
            for message in host.drain_messages().collect::<Vec<_>>() {
                match message {
                    Message::Activate => {
                        self.activated = true;
                        host.ui_mut()
                            .set_label_text(self.status.expect("status exists"), "Activated")
                            .map_err(io::Error::other)?;
                    }
                }
            }
            context.invalidate_window(id);
        } else if self.redraws >= 2 {
            if !self.activated {
                return Err(io::Error::other("semantic activation emitted no message"));
            }
            if host.ui().needs_redraw() {
                return Err(io::Error::other("UI remained invalidated after redraw"));
            }
            context.unregister_window(id);
            self.host = None;
            context.exit();
        }
        Ok(())
    }
}

fn find<'a>(node: &'a SemanticNode, role: SemanticRole, label: &str) -> Option<&'a SemanticNode> {
    if node.role == role && node.label == label {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find(child, role, label))
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), astrelis_app::RuntimeError<io::Error>> {
    Runtime::finish(astrelis_platform_winit::run_return(Runtime::new(
        NativeSmoke::new(),
        RuntimeConfig::default(),
    )))
    .map(|_| ())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
