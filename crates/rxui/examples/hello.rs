//! Minimal native RXUI application.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use rxui::prelude::*;

#[derive(Clone, Copy)]
enum Message {
    Greet,
}

#[derive(Default)]
struct Hello {
    status: Option<ElementHandle<Label>>,
}

impl App for Hello {
    type Message = Message;

    fn build(&mut self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        let mut ui = cx.new_ui();
        let root = ui.root();
        let content = ui
            .padding(root, Insets::all(28.0))
            .grow(1.0)
            .column()
            .finish();
        ui.label(content, "Hello from RXUI").finish();
        let greet = ui.button(content, "Greet").finish();
        self.status = Some(ui.label(content, "Ready").finish());
        ui.on_click(greet, |event| event.emit(Message::Greet));
        cx.open_window(WindowConfig::new("RXUI hello").size(520.0, 280.0), ui)?;
        Ok(())
    }

    fn update(&mut self, cx: &mut AppCx<'_, Message>, message: Message) -> rxui::Result<()> {
        match message {
            Message::Greet => {
                let status = self.status.expect("status label is created in `build`");
                cx.source_ui()?.set_label_text(status, "Welcome to RXUI!")?;
            }
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> MainResult {
    run(Hello::default())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
