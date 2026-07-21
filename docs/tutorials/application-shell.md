# Build an application shell

Use `crates/rxui/examples/application_shell.rs` as the executable companion.
It runs on the high-level runner from the [first-app tutorial](first-app.md)
and demonstrates one shared `CommandRegistry` driving a responsive toolbar,
keyboard shortcuts, and menus instead of maintaining separate action state.

Keep document changes in `UndoAction` values and call `sync_undo_commands` after
each mutation. Build modal settings with `DialogHost`, return validation through
`ValidationResult`, and reserve `ToastHost` for non-blocking outcomes. Toast
deadlines arrive as ordinary timer messages: after pushing a toast, schedule
`cx.set_timeout(deadline - cx.now(), Message::ExpireToasts)` and expire the
queue when the message comes back through `update`.

## Route events through the runner hooks

Concerns below the widget layer live in the `App` hooks. The `window_event`
hook sees every raw platform event before the UI handles it — the shell uses
it for shortcut routing, window-placement tracking, drag-and-drop, and toolbar
overflow:

```rust
fn window_event(
    &mut self,
    cx: &mut AppCx<'_, Message>,
    window: WindowId,
    event: &WindowEvent,
) -> rxui::Result<()> {
    if let Some(message) = self.router.handle_event(event, &self.commands) {
        cx.post(message);
    }
    self.placement.handle_event(cx.window(window)?, event);
    if let WindowEvent::DroppedFile(path) = event {
        cx.post(Message::FileDropped(path.clone()));
    }
    if let WindowEvent::Resized(size) = event {
        // ... recompute toolbar overflow from the new logical width
    }
    Ok(())
}
```

The close hooks are the persistence boundary. `close_requested` captures the
final window placement (and may return `CloseResponse::Ignore` to keep the
window open, for example while confirming unsaved changes); `window_closed`
and `exiting` both save state, covering user-initiated closes and application
exit:

```rust
fn close_requested(
    &mut self,
    cx: &mut AppCx<'_, Message>,
    window: WindowId,
) -> rxui::Result<CloseResponse> {
    self.placement.capture(cx.window(window)?);
    Ok(CloseResponse::Close)
}

fn window_closed(&mut self, _cx: &mut AppCx<'_, Message>, _window: WindowId) -> rxui::Result<()> {
    self.save_state();
    Ok(())
}

fn exiting(&mut self, _cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
    self.save_state();
    Ok(())
}
```

Persist window placement and application state with `JsonStateStore`; its
versioned, atomic envelope is the migration boundary for stored data. The
shell stores version 2 — a struct wrapping the optional `WindowPlacement`
plus the `RecentDocuments` list — and treats missing or stale-version files
as defaults.

## Desktop services

`DesktopServices` (from `rxui::prelude`, backed by `rxui-services`) provides
native file dialogs, URL and path launching, and pairs with `RecentDocuments`
for recent-file tracking. Dialogs never block the UI thread: each call takes a
one-shot delivery closure that posts the result back into `update` through a
`MessageProxy`:

```rust
Message::OpenFile => {
    // Delivery runs off the UI thread; route the result back into
    // `update` through a message proxy.
    let proxy = cx.proxy();
    self.services
        .pick_file(text_dialog_options("Open File"), move |path| {
            let _ = proxy.post(Message::FileOpened(path));
        });
}
```

`FileDialogOptions` carries the title, filters (`FileFilter`), and suggested
file name; `None` delivered to the closure means the user cancelled.
`save_file`, `pick_files`, and `pick_folder` follow the same shape, and
`open_url`/`open_path`/`reveal_path` report failures synchronously through
`ServiceError`.

Dropped files arrive as `WindowEvent::DroppedFile` in the `window_event` hook
(posted as `Message::FileDropped` above). Opened, saved, and dropped paths all
funnel into `RecentDocuments::touch`, which bumps the file in the
capacity-bounded recents sidebar; the list serializes with the version-2
persisted state so it survives restarts.

Run the shell with:

```sh
cargo run -p rxui --example application_shell
```

Test shell behavior headlessly with `AppHarness`: activate toolbar buttons by
semantic role and label, `advance` the virtual clock to expire toasts, script
dialogs with the `rxui-services` `FakeBackend`, and `request_close` to
exercise the persistence hooks. Native menu installation remains
platform-specific, but the command and menu models are portable.
