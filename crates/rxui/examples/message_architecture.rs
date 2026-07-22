//! Feature-local and hierarchical message architecture in one small app.

#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use std::time::Duration;

use rxui::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CounterId(u8);

#[derive(Debug)]
enum Message {
    Workspace(WorkspaceMessage),
}

#[derive(Debug)]
enum WorkspaceMessage {
    Counter(CounterId, CounterMessage),
}

#[derive(Debug)]
enum CounterMessage {
    Decrement,
    Increment,
    ResetLater,
    CancelReset,
    ResetNow,
    QueuePreviewBurst,
    PreviewValue(i32),
}

struct CounterFeature {
    id: CounterId,
    name: &'static str,
    value: i32,
    value_label: Option<ElementHandle<Label>>,
    reset_status: Option<ElementHandle<Label>>,
    preview_status: Option<ElementHandle<Label>>,
    reset_timer: Option<TimerId>,
    preview_message: &'static str,
}

impl CounterFeature {
    fn new(id: CounterId, name: &'static str) -> Self {
        Self {
            id,
            name,
            value: 0,
            value_label: None,
            reset_status: None,
            preview_status: None,
            reset_timer: None,
            preview_message: "No preview burst queued",
        }
    }

    fn build<T>(
        &mut self,
        ui: &mut Ui<Message>,
        parent: ElementHandle<T>,
        messages: MessageMapper<CounterMessage, Message>,
    ) {
        let card = ui
            .padding(parent, Insets::all(20.0))
            .grow(1.0)
            .min_width(px(260.0))
            .column()
            .flex(FlexStyle {
                row_gap: 12.0,
                ..Default::default()
            })
            .finish();
        ui.label(card, self.name)
            .style(WidgetStyle {
                font_size: Some(20.0),
                font_weight: Some(650.0),
                ..Default::default()
            })
            .finish();
        self.value_label = Some(
            ui.label(card, "0")
                .style(WidgetStyle {
                    font_size: Some(36.0),
                    font_weight: Some(700.0),
                    ..Default::default()
                })
                .finish(),
        );

        let adjustments = ui
            .row(card)
            .flex(FlexStyle {
                column_gap: 8.0,
                ..Default::default()
            })
            .finish();
        let decrement = ui.button(adjustments, "−1").finish();
        let decrement_messages = messages.clone();
        ui.on_click(decrement, move |event| {
            decrement_messages.emit(event, CounterMessage::Decrement);
        });
        let increment = ui.button(adjustments, "+1").finish();
        let increment_messages = messages.clone();
        ui.on_click(increment, move |event| {
            increment_messages.emit(event, CounterMessage::Increment);
        });

        let reset_actions = ui
            .row(card)
            .flex(FlexStyle {
                column_gap: 8.0,
                ..Default::default()
            })
            .finish();
        let reset = ui.button(reset_actions, "Reset in 2s").finish();
        let reset_messages = messages.clone();
        ui.on_click(reset, move |event| {
            reset_messages.emit(event, CounterMessage::ResetLater);
        });
        let cancel = ui.button(reset_actions, "Cancel reset").finish();
        let cancel_messages = messages.clone();
        ui.on_click(cancel, move |event| {
            cancel_messages.emit(event, CounterMessage::CancelReset);
        });
        self.reset_status = Some(ui.label(card, "No reset scheduled").finish());

        let preview = ui.button(card, "Queue 1,000 previews").finish();
        ui.on_click(preview, move |event| {
            messages.emit(event, CounterMessage::QueuePreviewBurst);
        });
        self.preview_status = Some(ui.label(card, self.preview_message).finish());
    }

    fn update(
        &mut self,
        cx: &mut MappedAppCx<'_, CounterMessage, Message>,
        message: CounterMessage,
    ) -> rxui::Result<()> {
        match message {
            CounterMessage::Decrement => self.value -= 1,
            CounterMessage::Increment => self.value += 1,
            CounterMessage::ResetLater => {
                if let Some(timer) = self.reset_timer.take() {
                    cx.cancel_timer(timer);
                }
                self.reset_timer =
                    Some(cx.set_timeout(Duration::from_secs(2), CounterMessage::ResetNow));
            }
            CounterMessage::CancelReset => {
                if let Some(timer) = self.reset_timer.take() {
                    cx.cancel_timer(timer);
                }
            }
            CounterMessage::ResetNow => {
                self.reset_timer = None;
                self.value = 0;
            }
            CounterMessage::QueuePreviewBurst => {
                let key = MessageKey::new("counter.preview", u64::from(self.id.0));
                for value in 1..=1_000 {
                    cx.post_latest(key, CounterMessage::PreviewValue(value));
                }
                self.preview_message = "Queued 1,000 preview values";
            }
            CounterMessage::PreviewValue(value) => {
                self.value = value;
                self.preview_message = "Delivered final preview (999 replaced)";
            }
        }
        self.sync(cx)
    }

    fn sync(&self, cx: &mut MappedAppCx<'_, CounterMessage, Message>) -> rxui::Result<()> {
        let value = self.value_label.expect("value label exists");
        let status = self.reset_status.expect("reset status exists");
        let preview_status = self.preview_status.expect("preview status exists");
        let reset_status = if self.reset_timer.is_some() {
            "Reset scheduled"
        } else {
            "No reset scheduled"
        };
        let ui = cx.source_ui()?;
        ui.set_label_text(value, self.value.to_string())?;
        ui.set_label_text(status, reset_status)?;
        ui.set_label_text(preview_status, self.preview_message)?;
        Ok(())
    }
}

struct MessageArchitecture {
    counters: [CounterFeature; 2],
}

impl Default for MessageArchitecture {
    fn default() -> Self {
        Self {
            counters: [
                CounterFeature::new(CounterId(1), "Counter A"),
                CounterFeature::new(CounterId(2), "Counter B"),
            ],
        }
    }
}

impl App for MessageArchitecture {
    type Message = Message;

    fn build(&mut self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        let workspace_messages = MessageMapper::new(Message::Workspace);
        let mut ui = cx.new_ui();
        let root = ui.root();
        let content = ui
            .padding(root, Insets::all(28.0))
            .grow(1.0)
            .column()
            .flex(FlexStyle {
                row_gap: 16.0,
                ..Default::default()
            })
            .finish();
        ui.label(content, "Hierarchical Message Mapping")
            .style(WidgetStyle {
                font_size: Some(25.0),
                font_weight: Some(700.0),
                ..Default::default()
            })
            .finish();
        ui.label(
            content,
            "Each card owns CounterMessage; the application only dispatches Message.",
        )
        .finish();
        let cards = ui
            .row(content)
            .grow(1.0)
            .flex(FlexStyle {
                column_gap: 16.0,
                wrap: FlexWrap::Wrap,
                ..Default::default()
            })
            .finish();

        for counter in &mut self.counters {
            let id = counter.id;
            let messages =
                workspace_messages.map_child(move |message| WorkspaceMessage::Counter(id, message));
            counter.build(&mut ui, cards, messages);
        }

        cx.open_window(
            WindowConfig::new("RXUI message architecture").size(760.0, 520.0),
            ui,
        )?;
        Ok(())
    }

    fn update(&mut self, cx: &mut AppCx<'_, Message>, message: Message) -> rxui::Result<()> {
        let Message::Workspace(WorkspaceMessage::Counter(id, message)) = message;
        let counter = self
            .counters
            .iter_mut()
            .find(|counter| counter.id == id)
            .expect("counter IDs originate from the retained feature instances");
        let mapper = MessageMapper::new(move |message| {
            Message::Workspace(WorkspaceMessage::Counter(id, message))
        });
        let mut feature_cx = cx.map_messages(mapper);
        counter.update(&mut feature_cx, message)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> MainResult {
    run(MessageArchitecture::default())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
