# Build an application shell

Use `crates/astreon/examples/application_shell.rs` as the executable companion.
It demonstrates one shared `CommandRegistry` driving a responsive toolbar,
keyboard shortcuts, and menus instead of maintaining separate action state.

Keep document changes in `UndoAction` values and call `sync_undo_commands` after
each mutation. Build modal settings with `DialogHost`, return validation through
`ValidationResult`, and reserve `ToastHost` for non-blocking outcomes. Persist
window placement and application state with `JsonStateStore`; its versioned,
atomic envelope is the migration boundary for stored data.

```sh
cargo run -p astreon --example application_shell
```

Test shell behavior headlessly by activating controls through `UiHarness` and
asserting emitted messages. Native menu installation remains platform-specific,
but the command and menu models are portable.
