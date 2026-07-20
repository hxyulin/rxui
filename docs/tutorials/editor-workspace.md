# Build and harden an editor workspace

`crates/rxui/examples/reference_editor.rs` combines docking, hierarchy and
table selection, property editing, render views, saved layouts, commands, and
undo/redo. Keep selection and editor data application-owned; call each view's
`sync` method after the model changes.

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
process-specific IDs and rounds logical geometry.

Profile the three critical reference-editor interactions headlessly:

```sh
cargo run --release -p rxui --example reference_editor_perf -- --check
```

The check warms each scenario, measures 500 updates, and enforces an 8 ms
average budget for pane resizing, entity selection, and table resizing.
