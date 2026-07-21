//! Native example for the optional retained UI inspector.
//!
//! Runs on the high-level [`rxui::app`] runner: the `window_event` hook
//! re-syncs the inspector on resize (its reported bounds are viewport
//! dependent) and [`InspectorAction`] messages flow through [`App::update`].

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use rxui::prelude::*;

#[derive(Clone)]
enum Message {
    Increment,
    Inspector(InspectorAction),
}

struct InspectorExample {
    inspector: Option<UiInspector<Message>>,
    value: Option<ElementHandle<Label>>,
    count: usize,
}

impl InspectorExample {
    fn new() -> Self {
        Self {
            inspector: None,
            value: None,
            count: 0,
        }
    }
}

impl App for InspectorExample {
    type Message = Message;

    fn build(&mut self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        let mut ui = cx.new_ui();
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
        let list = ui.add_scroll_view(content)?;
        ui.set_layout(
            list,
            LayoutStyle {
                height: Length::Px(160.0),
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;
        let rows = ui.add_column(list)?;
        for index in 0..300 {
            ui.add_label(rows, format!("Row {index}"))?;
        }
        let inspector = UiInspector::new(
            &mut ui,
            InspectorOptions {
                allow_editing: true,
                ..InspectorOptions::default()
            },
            Message::Inspector,
        )?;
        self.value = Some(value);
        self.inspector = Some(inspector);
        cx.open_window(
            WindowConfig::new("RXUI UI inspector").size(900.0, 620.0),
            ui,
        )?;
        Ok(())
    }

    fn update(&mut self, cx: &mut AppCx<'_, Message>, message: Message) -> rxui::Result<()> {
        let ui = cx.source_ui()?;
        match message {
            Message::Increment => {
                self.count += 1;
                ui.set_label_text(
                    self.value.expect("value label exists"),
                    format!("Count: {}", self.count),
                )?;
                self.inspector
                    .as_mut()
                    .expect("inspector exists")
                    .sync(ui)?;
            }
            Message::Inspector(action) => self
                .inspector
                .as_mut()
                .expect("inspector exists")
                .apply(ui, action)?,
        }
        Ok(())
    }

    fn window_event(
        &mut self,
        cx: &mut AppCx<'_, Message>,
        window: WindowId,
        event: &WindowEvent,
    ) -> rxui::Result<()> {
        if matches!(event, WindowEvent::Resized(_)) {
            // Bounds shown by the inspector are viewport dependent.
            self.inspector
                .as_mut()
                .expect("inspector exists")
                .sync(cx.ui(window)?)?;
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> MainResult {
    run_with(
        InspectorExample::new(),
        AppConfig::default().theme(Theme::dark()),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {}
