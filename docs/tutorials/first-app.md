# Your first Astreon app

Start from `crates/astreon/examples/hello.rs`. An Astreon application owns an
Astrelis `App`, creates one retained `Ui<Message>` in `resumed`, and connects it
to a native window through `WindowHost`.

1. Define a small cloneable message enum describing user intent.
2. Build controls under `ui.root()` and use `on_click`, `on_checked`, or another
   typed listener to emit those messages.
3. In `window_event`, pass the event to `WindowHost::handle_event`, drain
   messages, mutate retained controls, and invalidate the window when requested.
4. In `redraw`, call `WindowHost::redraw`. Do not run a continuous frame loop;
   the desktop runtime sleeps after invalidation has cleared.

Run the complete example with:

```sh
cargo run -p astreon --example hello
```

Every interactive control should have an accessible label, keyboard behavior,
focus handling, and a deterministic semantic-action test.
