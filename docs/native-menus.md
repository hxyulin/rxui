# Native menus and desktop requests

RXUI owns the portable menu description, command resolution and Muda adapter.
Astrelis stays independent of UI commands. The host uses astrelis-winit's existing
caller-provided event loop and proxy; no Astrelis changes or pin update are needed.

Enable `native-menus` to add native hosting and Muda. The existing `native`
feature has no menu dependency. macOS uses an application menu bar; Windows uses
separate menu bars for managed windows. Muda's Linux/BSD backend requires a GTK
window, so this winit host returns an unsupported-platform error before starting
an event loop when a menu bar is requested there. Headless and portable builds
remain independent of Muda.

```rust
use rxui::prelude::*;
use rxui::standard_commands::{CloseWindow, Copy, Cut, Paste, Quit, SelectAll};
struct Save;
impl Command for Save {}
let menus = NativeMenuBar::new()
    .menu(NativeMenu::new("My App").role(NativeMenuRole::About)
        .separator().command::<Quit>("Quit"))
    .menu(NativeMenu::new("File").command::<Save>("Save")
        .separator().command::<CloseWindow>("Close Window"))
    .menu(NativeMenu::new("Edit").command::<Cut>("Cut")
        .command::<Copy>("Copy").command::<Paste>("Paste")
        .separator().command::<SelectAll>("Select All"));
let application = Application::new().menu_bar(menus);
```

On macOS, put the application menu first; the system uses the app bundle name
for its displayed title. NativeMenu::submenu supports nested menus. Literal ampersands are escaped rather
than assigning accidental Windows mnemonics. About, Services, Hide, Hide Others
and Show All are system-managed macOS roles and are omitted on Windows. Editing,
Close and Quit use RXUI commands so our controls and veto hooks remain authoritative.

## Live command resolution

A menu entry stores a CommandId and fallback caption, without retaining an entity,
mount, payload or callback. The nearest live action supplies its caption, enabled
state and shortcuts. Disabled actions shadow outer actions. Missing handlers are
disabled; standard native commands additionally have the fallbacks below.

Declare view handlers with cx.command, attach them with Element::on_command, and
reuse the same CommandAction for buttons/popovers. Register global fallbacks with
cx.register_command and retain their registration handles as usual.

Menu selection resolves the identity again after pending tasks/model changes.
Routing walks from logical focus through ancestors, then application registrations.
macOS remembers the last focused managed window while native menus temporarily
change activation. Windows menu identities retain their source window. Closing
windows cannot receive actions. With no source window, application registrations
and the Quit fallback still work.

Modal restrictions apply to menu actions. Background/application actions require
.allow_in_modal(true). The standard Close Window fallback is disabled in a modal;
the global Quit request remains available. Editing targets the permitted input
inside the modal, never a blocked background input.

Ui::query_command::<C>() and query_command_id(id) inspect a prepared snapshot
without invoking callbacks, returning CommandInfo { label, enabled, shortcuts }.
Reconcile model changes before querying. These portable queries cover declared
handlers; standard native fallbacks are supplied by the host. AppContext provides
query_command::<C>() and dispatch_command::<C>() for application registrations
without choosing a window. Explicit CommandAction::invoke continues to target a
particular action instance. Disposed mounts are excluded from presentation and
shortcut routing, including stale view actions retained in global registrations.

## Shortcuts and editing

The first eligible logical chord becomes the native accelerator. It requires
Control, Command or Alt and a representable character/navigation key. Unmodified
keys and unsupported keys stay in normal RXUI input. A chord shadowed by another
focused command is not installed; duplicate menu entries install it once.
Windows consumes translated accelerator messages before RXUI can also receive
KeyboardInput. Native accelerators use OS menu dispatch before raw RXUI key
listeners; express interception through command scopes or availability. Native
accelerator repetition follows the platform menu system; CommandAction::repeat
controls the ordinary RXUI key route.

Standard exact primary shortcuts are C, X, V, A, W and Q. Declared commands take
precedence over host fallbacks. Copy requires a selection, Cut an editable
selection, Paste an editable input, and Select All a selectable input. Read-only
inputs support Copy and Select All. Clipboard failures are reported without
terminating the app. Cut removes text only after a successful clipboard write.
Paste uses controlled editing proposals, preserving acceptance/normalization/rejection.

## Lifecycle requests

cx.request_close(&window) uses Application::close_requested; KeepOpen leaves the
window live. cx.request_quit() uses Application::quit_requested; KeepOpen leaves
the app running. Both default to acceptance. Accepted Quit exits once without
issuing separate window close requests. Explicit close_window and exit are
already-decided operations and bypass the hooks. Operations queued by a request
hook are applied before the event loop goes idle, including opening a confirmation
window or deciding to exit inside a hook. Repeatedly requeuing close/quit from the
same veto hooks is bounded and reported as an error.

For async saving, return KeepOpen, start a task, then call close_window or exit
after saving succeeds. RunnerOptions still controls exiting after the last window
closes. Set exit_on_last_window: false for a macOS app whose menu remains available
without document windows.

The single-file example [native_menu_window.rs](../crates/rxui/examples/native_menu_window.rs)
shows independent documents, live Save captions and availability, a nested Save
All menu, editing and Close/Quit vetoes:

```sh
cargo run -p rxui --example native_menu_window --features native-menus
```

## Integration boundaries

The host refreshes menu state after input/model changes and reconciles dirty CPU
snapshots as needed. Native setters run only when presentation changes. It adds
no polling timer, extra redraw loop or GPU resources.

Muda's event handler is process-wide and installed once. RXUI forwards through a
replaceable event-loop proxy and releases its proxy/attachments when the host
drops. Do not independently install another Muda event handler with this adapter.

Three Win32 operations need unsafe APIs: attachment, detachment and accelerator
translation. They are isolated in native/menu_backend/windows.rs, with owned
window/menu lifetimes and borrowed MSG lifetimes documented. The package denies
unsafe elsewhere; the workspace default remains forbid for other crates. The
menu model and public API contain no unsafe code.

RXUI popovers continue to provide context menus. Checked/radio items, dynamic menu
structure/replacement and a GTK host remain separate extensions. Live captions,
availability and shortcuts do not require rebuilding menu structure.
