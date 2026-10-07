# Native dialogs and desktop services

Status: implemented as optional desktop features. `native-dialogs` adds RFD file
and message dialogs; `desktop-services` adds default-application launch and
file-manager reveal through Opener. Both imply `native`. Clipboard text access is
available with `native` alone. Astrelis remains responsible for graphics and window
hosting; these application services belong to RXUI.

## Requests and completions

File descriptions use the usual builders:

```rust
let options = FileDialog::new()
    .parent(&window)
    .title("Open document")
    .directory(directory)
    .filter("Text documents", ["txt"]);
self.dialog = Some(cx.pick_file(options, |this, selection, cx| {
    this.dialog = None;
    match selection {
        Ok(Some(path)) => { /* schedule reading with cx.spawn_blocking */ }
        Ok(None) => { /* retain the current document */ }
        Err(error) => this.status = error.to_string(),
    }
})?);
```

`pick_file`, `pick_files`, `pick_folder`, `pick_folders` and `save_file` return
`Result<DialogTask, ApplicationError>` immediately. File completions deliver
`DialogResult<Option<PathBuf>>` or `DialogResult<Option<Vec<PathBuf>>>`. File dialogs
only select paths: application jobs own reading, writing, encoding and error policy.
Save As includes the platform's overwrite confirmation, but does not write a file.
RFD reports cancellation and some backend failures as `None`; RXUI cannot distinguish
those cases. Exposed setup/driver failures use `DialogError::Backend`.

`show_message(MessageDialog, completion)` delivers `DialogResult<MessageResponse>`.
The description selects title, body, Info/Warning/Error severity and one of
Ok, OkCancel, YesNo or YesNoCancel. Native captions/localization follow the platform.
Custom button captions are outside this version's scope.

Typed `Context<T>` callbacks receive the owner's current `&mut T`, the result and
a fresh `Context<T>`. AppContext callbacks receive the result and AppContext.
No entity lease spans the native dialog, and no native dialog runs inside a view
evaluation. Construction starts on the UI thread; a sleeping waiter posts the
response through the event-loop proxy. There is no frame polling or busy loop.

## Parent ownership, cancellation and teardown

Every dialog requires a managed parent. A listener defaults to `cx.window()`;
initialization, lifecycle hooks and background completions should use `.parent`.
A queued window can be selected before native creation: the request waits for it.
Wrong-runtime, closed-parent, invalid-option and missing-source requests fail
before reservation. One pending/open dialog is permitted per parent; another
returns `ApplicationError::DialogBusy`. Different parents can have separate requests.

Retain the DialogTask or explicitly detach it. Dropping or cancelling suppresses
delivery and skips an unstarted dialog. Cancellation **does not dismiss an already
open OS dialog**. `is_finished` describes delivery, so it can become true while
the OS dialog remains open. Its parent's reservation remains busy until the native
response arrives.

Owner disposal, originating mount removal, parent closure and app exit suppress
stale callbacks. Successful completion restores the request's source placement;
an explicitly parented request without an originating placement resolves a live
placement in that parent. Other background tasks still have no implicit source.

A decided parent close is marked logically closing immediately, but native
destruction waits for its open dialog to finish. Decided application exit likewise
waits for open native dialogs and suppresses their callbacks. The waiter retains
the native parent independently of cancellation and the task worker pool, keeping
borrowed platform handles valid. Applications should finish their confirmation
workflow before calling `close_window` or `exit`.

With native menus on macOS, standard Edit commands use Cocoa responder actions
while the focused managed window owns an open native dialog. This lets the dialog
manage its native text selection and clipboard. Returning to a different managed
window or finishing the dialog restores RXUI command routing.

## Clipboard and desktop launch

`cx.read_clipboard_text()` and `cx.write_clipboard_text(text)` are synchronous,
fallible native text operations. They use the same persistent clipboard owner as
input Copy/Cut/Paste. Clipboard borrows end before controlled text-change callbacks,
so those callbacks may safely access the public clipboard API.

`cx.open_url(url, completion)`, `open_file(path, completion)` and
`reveal_file(path, completion)` return `Result<Task, ApplicationError>` and run on
the blocking worker pool. Typed completions weakly target current owner state;
they do not keep a disposed entity alive. Completion delivers `DesktopResult`,
including platform launch failure or worker panic. Retain/detach/cancel the Task
using the normal [task contract](async.md). Cancellation cannot undo an OS launch.

URLs require a URI scheme and no control characters; custom schemes are supported.
File paths are passed as data rather than evaluated as shell text. Successful
launch means the launcher accepted the request, not that another app finished
loading its content. Reveal behavior follows the installed file manager.

Linux file dialogs use the XDG portal backend, with Wayland support. Runtime
availability depends on the desktop's portal setup; file results retain RFD's
`None` ambiguity. Native menu attachment is currently macOS/Windows, independently
of the dialog/service features. This version does not add sandbox entitlements,
security-scoped bookmark persistence, document associations or recent-file storage.

## Standalone document example

```sh
cargo run -p rxui --example document_window --features native-menus,native-dialogs,desktop-services --locked
```

[document_window.rs](../crates/rxui/examples/document_window.rs) contains its own
view, window, command/menu setup and lifecycle hooks. It demonstrates Open, Save,
Save As, clipboard access, Reveal File and unsaved Close/Quit confirmation. Choose
Yes to save and continue, No to discard and continue, or Cancel to retain the window.
Cancelling a save picker or failing file I/O also stops the pending close/quit.
The current input control is single-line; the example rejects multiline files
rather than silently changing their contents. It is a workflow example, not a
production document store with atomic replacement and recovery.

## Validation

On macOS, a separate inactive, normal-level probe exercised native Open/Save As
with real file I/O, cancelled unsaved Close, save-before-Quit, clipboard access
inside a text-change callback, and decided parent close during an open picker.
The last case exited cleanly with zero stale callbacks after cancelling the picker.
Automated Paste into native picker fields was inconclusive; those fields were
entered through accessibility for the file workflow checks. Native Edit selector
replacement was observed, but that alone does not establish keyboard Paste behavior.
Windows has compilation/Clippy coverage; its native dialogs were not interactively
tested here. Linux portal behavior likewise still needs native runtime validation.
