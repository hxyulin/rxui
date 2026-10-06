//! Standalone application-host example. Native setup and task completion are handled by RXUI.
//! Click controls, Tab/Shift-Tab changes focus, Enter/Space activates, wheel scrolls.
use rxui::prelude::*;
use std::{error::Error, time::Duration};

struct Counter {
    count: i32,
    items: Vec<u32>,
    next_item: u32,
    selected: Option<u32>,
    request: u64,
    loading: bool,
    result: Option<u64>,
    error: Option<String>,
    task: Option<Task>,
}
impl Counter {
    fn load(&mut self, _: &ClickEvent, cx: &mut Context<'_, Self>) {
        self.request += 1;
        self.loading = true;
        self.error = None;
        let request = self.request;
        self.task = Some(cx.spawn(
            async move {
                rxui::sleep(Duration::from_millis(750)).await;
                request * 100
            },
            move |this, outcome, _cx| {
                if this.request != request {
                    return;
                }
                this.loading = false;
                this.task = None;
                match outcome {
                    Ok(value) => this.result = Some(value),
                    Err(error) => this.error = Some(error.to_string()),
                }
            },
        ));
    }
}
impl View for Counter {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let status = if self.loading {
            "Loading without blocking the UI…".to_owned()
        } else if let Some(error) = &self.error {
            error.clone()
        } else {
            format!("Background result: {:?}", self.result)
        };
        column()
            .fill_width()
            .padding(24.)
            .gap(12.)
            .child(label("RXUI application and tasks").font_size(28.))
            .child(
                label("Click or use Tab and Enter. Scroll the items below.")
                    .color([0.65, 0.72, 0.84, 1.]),
            )
            .child(label(format!("Counter: {}", self.count)).font_size(22.))
            .child(
                row()
                    .gap(12.)
                    .child(
                        button("Decrease")
                            .key("decrease")
                            .on_click(cx.listener(|this, _, _| this.count -= 1)),
                    )
                    .child(
                        button("Increase")
                            .key("increase")
                            .on_click(cx.listener(|this, _, _| this.count += 1)),
                    )
                    .child(
                        button("Open shared window")
                            .key("open")
                            .on_click(cx.listener(|_, _, cx| {
                                let root = cx.entity().upgrade().unwrap();
                                cx.open_window(
                                    WindowOptions::new()
                                        .title("RXUI — shared Counter")
                                        .size(820., 760.),
                                    root,
                                )
                                .unwrap();
                            })),
                    )
                    .child(
                        button("Close this window")
                            .key("close")
                            .on_click(cx.listener(|_, _, cx| {
                                if let Some(window) = cx.window() {
                                    cx.close_window(&window).unwrap();
                                }
                            })),
                    ),
            )
            .child(
                row()
                    .gap(12.)
                    .child(
                        button("Reverse items")
                            .key("reverse")
                            .on_click(cx.listener(|this, _, _| this.items.reverse())),
                    )
                    .child(
                        button("Add item")
                            .key("add")
                            .on_click(cx.listener(|this, _, _| {
                                this.items.push(this.next_item);
                                this.next_item += 1;
                            })),
                    )
                    .child(
                        button("Remove first")
                            .key("remove")
                            .disabled(self.items.is_empty())
                            .on_click(cx.listener(|this, _, _| {
                                if !this.items.is_empty() {
                                    this.items.remove(0);
                                }
                            })),
                    ),
            )
            .child(
                column()
                    .height(200.)
                    .fill_width()
                    .scroll_y()
                    .key("items-viewport")
                    .gap(6.)
                    .background([0.035, 0.055, 0.08, 1.])
                    .children(self.items.iter().map(|item| {
                        let id = *item;
                        button(format!("Item {id}"))
                            .key(id)
                            .background(if self.selected == Some(id) {
                                [0.08, 0.3, 0.25, 1.]
                            } else {
                                [0.10, 0.16, 0.24, 1.]
                            })
                            .on_click(cx.listener(move |this, _, _| this.selected = Some(id)))
                    })),
            )
            .child(label(
                "Scroll and focus belong to the placement; task/model state is shared.",
            ))
            .child(
                row()
                    .gap(12.)
                    .child(
                        button("Start async job")
                            .key("load")
                            .on_click(cx.listener(Self::load)),
                    )
                    .child(
                        button("Cancel job")
                            .key("cancel")
                            .disabled(!self.loading)
                            .on_click(cx.listener(|this, _, _| {
                                this.request += 1;
                                this.task = None;
                                this.loading = false;
                            })),
                    ),
            )
            .child(label(status))
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    Application::new().run(|cx| {
        let counter = cx.new(|_| Counter {
            count: 0,
            items: (1..=24).collect(),
            next_item: 25,
            selected: None,
            request: 0,
            loading: false,
            result: None,
            error: None,
            task: None,
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — application, scrolling and async tasks")
                .size(820., 760.),
            counter,
        )?;
        Ok(())
    })?;
    Ok(())
}
