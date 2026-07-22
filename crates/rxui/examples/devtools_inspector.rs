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
    StartTask,
    CompleteTask,
    CancelTask,
    TaskFinished,
    Inspector(InspectorAction),
}

struct InspectorExample {
    inspector: Option<UiInspector<Message>>,
    value: Option<ElementHandle<Label>>,
    window: Option<WindowId>,
    demo_task: Option<TaskCompletion<()>>,
    count: usize,
}

impl InspectorExample {
    fn new() -> Self {
        Self {
            inspector: None,
            value: None,
            window: None,
            demo_task: None,
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
        let tasks = ui.row(content).finish();
        ui.set_flex(tasks, 8.0, Alignment::Center)?;
        let start_task = ui.button(tasks, "Start named task").finish();
        let complete_task = ui.button(tasks, "Complete task").finish();
        let cancel_task = ui.button(tasks, "Cancel task").finish();
        let value = ui.label(content, "Count: 0").finish();
        ui.on_click(increment, |context| context.emit(Message::Increment));
        ui.on_click(start_task, |context| context.emit(Message::StartTask));
        ui.on_click(complete_task, |context| context.emit(Message::CompleteTask));
        ui.on_click(cancel_task, |context| context.emit(Message::CancelTask));
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
        let window = cx.open_window(
            WindowConfig::new("RXUI UI inspector").size(900.0, 620.0),
            ui,
        )?;
        self.window = Some(window);
        Ok(())
    }

    fn update(&mut self, cx: &mut AppCx<'_, Message>, message: Message) -> rxui::Result<()> {
        match message {
            Message::Increment => {
                self.count += 1;
                let ui = cx.source_ui()?;
                ui.set_label_text(
                    self.value.expect("value label exists"),
                    format!("Count: {}", self.count),
                )?;
                self.inspector
                    .as_mut()
                    .expect("inspector exists")
                    .sync(ui)?;
            }
            Message::StartTask => {
                if self.demo_task.is_none() {
                    self.demo_task =
                        Some(cx.register_task_named("Inspector demo load", |()| {
                            Message::TaskFinished
                        }));
                }
            }
            Message::CompleteTask => {
                if let Some(task) = self.demo_task.take() {
                    let _ = task.complete(());
                }
            }
            Message::CancelTask => {
                if let Some(task) = self.demo_task.take() {
                    cx.cancel_task(task.id());
                }
            }
            Message::TaskFinished => {}
            Message::Inspector(action) => {
                let window = self.window.expect("window exists");
                self.inspector
                    .as_mut()
                    .expect("inspector exists")
                    .apply(cx.ui(window)?, action)?;
            }
        }
        let snapshot = cx.runtime_snapshot();
        let window = self.window.expect("window exists");
        self.inspector
            .as_mut()
            .expect("inspector exists")
            .sync_runtime(cx.ui(window)?, &snapshot)?;
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
