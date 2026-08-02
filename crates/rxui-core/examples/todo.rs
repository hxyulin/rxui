//! Headless keyed todo list composed with a stateless row view function.

use rxui_core::{
    Context, Element, EntityHarness, Render, RoutedHandler, button, column, label, row,
};

#[derive(Clone)]
struct Item {
    id: u64,
    text: String,
    done: bool,
}

struct Todo {
    next_id: u64,
    items: Vec<Item>,
}

/// A stateless view function: its result is inserted like any other Element.
fn todo_item(item: &Item, toggle: RoutedHandler, remove: RoutedHandler) -> Element {
    row()
        .gap(8.0)
        .child(label(if item.done {
            format!("✓ {}", item.text)
        } else {
            format!("○ {}", item.text)
        }))
        .child(button(format!("Toggle {}", item.id)).on_click(toggle))
        .child(button(format!("Remove {}", item.id)).on_click(remove))
        .key(item.id)
}

impl Render for Todo {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        let view = column()
            .gap(8.0)
            .child(button("Add").on_click(cx.listener(|this, _, cx| {
                let id = this.next_id;
                this.next_id += 1;
                this.items.push(Item {
                    id,
                    text: format!("Task {id}"),
                    done: false,
                });
                cx.notify();
            })))
            .child(button("Reverse").on_click(cx.listener(|this, _, cx| {
                this.items.reverse();
                cx.notify();
            })));

        let mut items = column().gap(4.0);

        for item in &self.items {
            let id = item.id;
            let toggle = cx.listener(move |this: &mut Self, _, cx| {
                if let Some(item) = this.items.iter_mut().find(|item| item.id == id) {
                    item.done = !item.done;
                    cx.notify();
                }
            });
            let remove = cx.listener(move |this: &mut Self, _, cx| {
                this.items.retain(|item| item.id != id);
                cx.notify();
            });
            items = items.child(todo_item(item, toggle, remove));
        }
        view.child(items)
    }
}

fn main() {
    let mut harness = EntityHarness::new(|cx| {
        cx.new(|_| Todo {
            next_id: 3,
            items: vec![
                Item {
                    id: 1,
                    text: "Write entity model".into(),
                    done: false,
                },
                Item {
                    id: 2,
                    text: "Port gate tests".into(),
                    done: false,
                },
            ],
        })
    });
    harness.activate("Toggle 1");
    harness.activate("Add");
    harness.activate("Reverse");
    harness.activate("Remove 2");
    println!("todo after headless actions:\n{}", harness.snapshot());
}
