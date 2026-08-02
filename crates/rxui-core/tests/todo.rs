//! Keyed todo behavior, identity preservation, and stateless composition gate.

use rxui_core::{
    Context, Element, EntityHarness, Render, RoutedHandler, button, column, label, row,
};

#[derive(Clone)]
struct Item {
    id: u64,
    done: bool,
}

struct Todo {
    next_id: u64,
    items: Vec<Item>,
}

fn item_view(id: u64, done: bool, toggle: RoutedHandler, remove: RoutedHandler) -> Element {
    row()
        .child(label(if done {
            format!("✓ Task {id}")
        } else {
            format!("○ Task {id}")
        }))
        .child(button(format!("Toggle {id}")).on_click(toggle))
        .child(button(format!("Remove {id}")).on_click(remove))
        .key(id)
}

impl Render for Todo {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        let controls = row()
            .child(button("Add").on_click(cx.listener(|this, _, cx| {
                this.items.push(Item {
                    id: this.next_id,
                    done: false,
                });
                this.next_id += 1;
                cx.notify();
            })))
            .child(button("Reverse").on_click(cx.listener(|this, _, cx| {
                this.items.reverse();
                cx.notify();
            })));
        let mut items = column();
        for item in &self.items {
            let id = item.id;
            let toggle = cx.listener(move |this: &mut Self, _, cx| {
                let item = this.items.iter_mut().find(|item| item.id == id).unwrap();
                item.done = !item.done;
                cx.notify();
            });
            let remove = cx.listener(move |this: &mut Self, _, cx| {
                this.items.retain(|item| item.id != id);
                cx.notify();
            });
            items = items.child(item_view(id, item.done, toggle, remove));
        }
        column().child(controls).child(items)
    }
}

#[test]
fn add_remove_toggle_and_reverse_preserve_keyed_identity() {
    let mut harness = EntityHarness::new(|cx| {
        cx.new(|_| Todo {
            next_id: 3,
            items: vec![Item { id: 1, done: false }, Item { id: 2, done: false }],
        })
    });

    // The label is produced inside `item_view`, proving a plain function's
    // Element composes by ordinary insertion.
    assert!(harness.try_find("○ Task 1").is_some());
    let one = harness.node_id("○ Task 1");
    let two = harness.node_id("○ Task 2");

    harness.activate("Toggle 1");
    assert_eq!(harness.node_id("✓ Task 1"), one);

    harness.activate("Add");
    let three = harness.node_id("○ Task 3");
    harness.activate("Reverse");
    assert_eq!(harness.node_id("✓ Task 1"), one);
    assert_eq!(harness.node_id("○ Task 2"), two);
    assert_eq!(harness.node_id("○ Task 3"), three);

    harness.activate("Remove 2");
    assert!(harness.try_find("○ Task 2").is_none());
    assert_eq!(harness.node_id("✓ Task 1"), one);
    assert_eq!(harness.node_id("○ Task 3"), three);
}
