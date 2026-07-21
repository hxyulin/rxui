# Your first RXUI app

Start from `crates/rxui/examples/hello.rs`. An RXUI application implements the
`App` trait from `rxui::prelude` and hands itself to `run`. The runner owns the
event loop, window hosting, message routing, and redraw scheduling; the
application describes its UI once in `build` and reacts to typed messages in
`update`.

1. Define a small cloneable message enum describing user intent.
2. In `build`, create a retained `Ui<Message>` with `cx.new_ui()`, add controls
   under `ui.root()`, attach `on_click`, `on_checked`, or another typed
   listener to emit messages, and open a window with `cx.open_window`.
3. In `update`, match on the message and mutate retained controls through
   `cx.source_ui()`, the UI tree of the window the message came from. The
   runner redraws invalidated windows for you; the desktop runtime sleeps
   while the application is idle.

The complete hello application:

```rust
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

fn main() -> MainResult {
    run(Hello::default())
}
```

Run it with:

```sh
cargo run -p rxui --example hello
```

Every callback returns `rxui::Result<()>`. The opaque `rxui::Error` converts
from any `std::error::Error` through plain `?`, so UI, host, IO, and
persistence errors mix without `map_err` glue; `main` returns `MainResult`,
which surfaces the first callback failure or platform error from `run`.

Beyond `build` and `update`, `App` has optional hooks with sensible defaults:
`window_event` observes raw platform events before the UI handles them,
`close_requested`/`window_closed`/`exiting` bracket window and application
shutdown, `render` overrides presentation for GPU composition, and `tick` runs
under a continuous `RuntimePolicy`. The [application shell
tutorial](application-shell.md) puts them to work.

## Testing your app

Enable the `testing` feature and drive the whole application headlessly with
`AppHarness`: it runs `build` against a deterministic backend, activates
controls by semantic role and label, and routes emitted messages through
`update` exactly like the runner.

```rust
use rxui::testing::{AppHarness, SemanticRole};

#[test]
fn greet_updates_the_status() -> rxui::Result<()> {
    let mut harness = AppHarness::new(Hello::default())?;
    let window = harness.windows()[0];
    harness.activate(window, SemanticRole::Button, "Greet")?;
    let bundle = harness.snapshot_bundle(window)?;
    assert!(bundle.semantics.contains("Welcome to RXUI!"));
    Ok(())
}
```

The harness also advances a virtual clock for timer messages, delivers close
requests, and provides an in-memory clipboard, so tests stay deterministic
without a GPU or event loop.

For full control over the event loop, implementing `astrelis_app::App`
directly and driving `WindowHost` yourself remains supported;
`crates/rxui/examples/native_smoke.rs` demonstrates that path.

Every interactive control should have an accessible label, keyboard behavior,
focus handling, and a deterministic semantic-action test.
