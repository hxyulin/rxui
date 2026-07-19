//! Minimal native Astreon application.

use std::io;

use astrelis_app::{App, AppContext, Runtime, RuntimeConfig};
use astrelis_core::geometry::Size;
use astrelis_platform::{WindowAttributes, WindowEvent, WindowId};
use astrelis_text::FontDatabase;
use astreon::prelude::*;

#[derive(Clone, Copy)]
enum Message {
    Greet,
}

struct Hello {
    graphics: GraphicsContext,
    host: Option<WindowHost<Message>>,
    status: Option<ElementHandle<Label>>,
}

impl Hello {
    fn new() -> Self {
        Self {
            graphics: GraphicsContext::new(),
            host: None,
            status: None,
        }
    }
}

impl App for Hello {
    type Error = io::Error;

    fn resumed(&mut self, context: &mut AppContext<'_, '_, Self>) -> Result<(), Self::Error> {
        if self.host.is_some() {
            return Ok(());
        }

        let mut ui = Ui::new(FontDatabase::default(), Theme::default());
        let root = ui.root();
        let content = ui
            .padding(root, Insets::all(28.0))
            .grow(1.0)
            .column()
            .finish();
        ui.label(content, "Hello from Astreon").finish();
        let greet = ui.button(content, "Greet").finish();
        let status = ui.label(content, "Ready").finish();
        ui.on_click(greet, |event| event.emit(Message::Greet));

        let host = WindowHost::open(
            context,
            &self.graphics,
            ui,
            WindowHostOptions {
                window: WindowAttributes {
                    title: "Astreon hello".into(),
                    inner_size: Some(Size::new(520.0, 280.0)),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .map_err(io::Error::other)?;
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
            return Ok(());
        }

        for message in host.drain_messages().collect::<Vec<_>>() {
            match message {
                Message::Greet => host
                    .ui_mut()
                    .set_label_text(
                        self.status.expect("status is created"),
                        "Welcome to Astreon!",
                    )
                    .map_err(io::Error::other)?,
            }
        }
        if update.redraw || host.ui().needs_redraw() {
            context.invalidate_window(id);
        }
        Ok(())
    }

    fn redraw(
        &mut self,
        _context: &mut AppContext<'_, '_, Self>,
        _window: WindowId,
    ) -> Result<(), Self::Error> {
        if let Some(host) = &mut self.host {
            host.redraw().map_err(io::Error::other)?;
        }
        Ok(())
    }
}

fn main() -> Result<(), astrelis_app::RuntimeError<io::Error>> {
    Runtime::finish(astrelis_platform_winit::run_return(Runtime::new(
        Hello::new(),
        RuntimeConfig::default(),
    )))
    .map(|_| ())
}
