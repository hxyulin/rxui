# rxui

A declarative desktop UI framework for Rust, with an embeddable wgpu renderer.

You describe each view with builders, from persistent, typed application state.
RXUI reconciles the result against the previous frame by key, lays it out with
Taffy, routes input, publishes accessibility trees through AccessKit, and paints
with wgpu. You can supply your own device, queue, encoder and target views, or
use the optional desktop host.

```rust,no_run
use rxui::prelude::*;

struct Counter { value: i32 }

impl View for Counter {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().padding(24.).gap(12.)
            .child(label(format!("Count: {}", self.value)))
            .child(button("Increase").key("increase")
                .on_click(cx.listener(|this, _, _cx| this.value += 1)))
    }
}

fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let counter = cx.new(|_| Counter { value: 0 });
        cx.open_window(WindowOptions::new().title("Counter"), counter)?;
        Ok(())
    })
}
```

The example needs the `native` feature. See the
[repository README](https://github.com/hxyulin/rxui/blob/main/README.md) for features and screenshots, the
[guide](https://github.com/hxyulin/rxui/blob/main/docs/guide.md), and the
[examples](https://github.com/hxyulin/rxui/blob/main/crates/rxui/examples).

The built-in icons are a subset of [Lucide](https://lucide.dev), licensed under
ISC, with icons derived from Feather under MIT. The full notice is in
[`src/icons/LICENSE-lucide.txt`](https://github.com/hxyulin/rxui/blob/main/crates/rxui/src/icons/LICENSE-lucide.txt).
