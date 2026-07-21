# Build and harden an editor workspace

`crates/rxui/examples/reference_editor.rs` combines docking, hierarchy and
table selection, property editing, render views, saved layouts, commands, and
undo/redo. Keep selection and editor data application-owned; call each view's
`sync` method after the model changes.

The editor runs on the high-level runner: `build` assembles the docked
workspace and opens the window, `update` applies each typed message and
re-syncs only the affected views, and the `window_event` hook routes command
shortcuts, feeds the window-placement tracker, and resizes the toolbar —
the runner owns host event dispatch, so the hook covers only application-side
routing. `close_requested` captures placement and persists the workspace
before the window closes.

The GPU scene texture goes through `cx.host(window)`, the window's
`WindowHost`: create the texture with `host.device()`, register it against an
`ExternalImage` shown by the `RenderView`, and upload pixels with
`host.queue().write_texture(...)` whenever the scene changes.

Frequent layout changes (pane drags) debounce their saves through a timer
message: the first change schedules
`cx.set_timeout(Duration::from_millis(250), Message::FlushSave)`, and the
state store writes once when the message arrives; `close_requested` cancels
the pending timer and saves immediately.

Enable the optional inspector while developing:

```toml
rxui = { version = "=0.1.0-rc.1", features = ["editor", "devtools", "testing"] }
```

Mount `UiInspector::new`, map `InspectorAction` into the application message
enum, and route those messages through `UiInspector::apply`. Press F12 (or
fn-F12 when macOS uses the key for media controls), Command-Option-I, or the
launcher to open it. Choose **Pick**, then click an application element. Escape
cancels picking or closes the modeless panel.

For regression tests, construct the same controlled view using
`deterministic_font_database()` and `deterministic_theme()`, wrap it in
`UiHarness`, and compare `semantic_snapshot`, `inspection_snapshot`, or
`display_list_snapshot` with checked-in text. The output deliberately omits
process-specific IDs and rounds logical geometry. `AppHarness` covers the
message loop end to end when a test needs `update`, timers, or the close flow
rather than a single view.

While iterating on responsiveness, set `RXUI_PERF=1` to print per-message
timing (count, average, and maximum per label) from the running example.
Profile the three critical reference-editor interactions headlessly:

```sh
cargo run --release -p rxui --example reference_editor_perf -- --check
```

The check warms each scenario, measures 500 updates, and enforces an 8 ms
average budget for pane resizing, entity selection, and table resizing.
