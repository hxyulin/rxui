//! Native example for the optional retained UI inspector.

use std::io;

use astrelis_app::{App, AppContext, Runtime, RuntimeConfig};
use astrelis_core::geometry::Size;
use astrelis_platform::{WindowAttributes, WindowEvent, WindowId};
use astrelis_text::FontDatabase;
use astreon::prelude::*;

#[derive(Clone)]
enum Message {
    Increment,
    Inspector(InspectorAction),
}

struct InspectorExample {
    graphics: GraphicsContext,
    host: Option<WindowHost<Message>>,
    inspector: Option<UiInspector<Message>>,
    value: Option<ElementHandle<Label>>,
    count: usize,
}

impl InspectorExample {
    fn new() -> Self {
        Self {
            graphics: GraphicsContext::new(),
            host: None,
            inspector: None,
            value: None,
            count: 0,
        }
    }
}

impl App for InspectorExample {
    type Error = io::Error;

    fn resumed(&mut self, context: &mut AppContext<'_, '_, Self>) -> Result<(), Self::Error> {
        if self.host.is_some() {
            return Ok(());
        }
        let mut ui = Ui::new(FontDatabase::default(), Theme::dark());
        let root = ui.root();
        let content = ui
            .padding(root, Insets::all(32.0))
            .grow(1.0)
            .column()
            .finish();
        ui.label(
            content,
            "Press F12 (fn-F12 on some Macs), Command-Option-I, or use Inspect.",
        )
        .finish();
        let increment = ui.button(content, "Increment").finish();
        let value = ui.label(content, "Count: 0").finish();
        ui.on_click(increment, |context| context.emit(Message::Increment));
        // Large scrollable list exercising the inspector's virtualized tree.
        let list = ui.add_scroll_view(content).map_err(io::Error::other)?;
        ui.set_layout(
            list,
            LayoutStyle {
                height: Length::Px(160.0),
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )
        .map_err(io::Error::other)?;
        let rows = ui.add_column(list).map_err(io::Error::other)?;
        for index in 0..300 {
            ui.add_label(rows, format!("Row {index}"))
                .map_err(io::Error::other)?;
        }
        let inspector = UiInspector::new(
            &mut ui,
            InspectorOptions {
                allow_editing: true,
                ..InspectorOptions::default()
            },
            Message::Inspector,
        )
        .map_err(io::Error::other)?;
        let host = WindowHost::open(
            context,
            &self.graphics,
            ui,
            WindowHostOptions {
                window: WindowAttributes {
                    title: "Astreon UI inspector".into(),
                    inner_size: Some(Size::new(900.0, 620.0)),
                    ..WindowAttributes::default()
                },
                ..WindowHostOptions::default()
            },
        )
        .map_err(io::Error::other)?;
        self.value = Some(value);
        self.inspector = Some(inspector);
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
        if matches!(event, WindowEvent::Resized(_)) {
            // Bounds shown by the inspector are viewport dependent.
            self.inspector
                .as_mut()
                .expect("inspector exists")
                .sync(host.ui_mut())
                .map_err(io::Error::other)?;
        }
        if update.close_requested {
            context.unregister_window(id);
            self.host = None;
            context.exit();
            return Ok(());
        }
        for message in host.drain_messages().collect::<Vec<_>>() {
            match message {
                Message::Increment => {
                    self.count += 1;
                    host.ui_mut()
                        .set_label_text(
                            self.value.expect("value label exists"),
                            format!("Count: {}", self.count),
                        )
                        .map_err(io::Error::other)?;
                    self.inspector
                        .as_mut()
                        .expect("inspector exists")
                        .sync(host.ui_mut())
                        .map_err(io::Error::other)?;
                }
                Message::Inspector(action) => self
                    .inspector
                    .as_mut()
                    .expect("inspector exists")
                    .apply(host.ui_mut(), action)
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
        InspectorExample::new(),
        RuntimeConfig::default(),
    )))
    .map(|_| ())
}
