# Native Application hosting

Status: implemented for desktop with the optional `native` feature. The host
composes Runtime, Ui, UiPainter and astrelis-winit; those lower-level APIs remain
usable independently. There is one shared graphics device and one UiPainter per
Application, with retained resources for every managed UI placement.

## Creation and ownership

`Application::new().run(initialize)` invokes its initializer once on the first
native resume. `cx.open_window(WindowOptions, Entity<View>)` validates options and
runtime identity immediately, then queues native creation outside the entity update
scope. Its returned WindowHandle has a stable RXUI WindowId before the winit
window exists. Platform/device creation failures propagate from Application::run.

WindowOptions configures title, logical size, clear color and SurfaceSettings.
`native_attributes(...)` exposes winit configuration, while `.graphics(...)` and
`.runner_options(...)` customize the device and lifecycle policy. `.font(...)`
selects application fonts instead of default system discovery; `.system_fonts(true)`
explicitly enables discovery alongside those fonts.

Dropping a WindowHandle does not close its window. `cx.close_window(&handle)`
queues an explicit close and marks the handle closing immediately; repeated close
requests while closing are idempotent. A queued close before creation cancels the
creation. `is_closed()` reports closing/removed state; `native_window()` returns
an Arc only after creation and before closing. RXUI clears its native Arc on removal.
An explicitly cloned native Arc follows winit ownership rules.

Each managed window owns a distinct Ui placement. Sharing an Entity<View> shares
its fields and any Task stored there, while focus, capture, hover and scroll belong
to the individual placement. Removing one window releases that placement and its
resources; shared models/jobs remain alive while another strong owner exists.
When application view state also needs to differ per window, create a separate
view entity for each window and give both a cloned handle to one domain model.

## Theme selection

Application::theme selects the default; windows inherit it unless WindowOptions::theme
supplies an explicit override. AppContext::set_theme updates inheriting windows,
set_window_theme chooses an explicit window value, and use_application_theme resumes
inheritance. WindowHandle::theme reports the latest selection, including pending
creation. Updates are queued outside active entity borrows. The default clear color
follows the effective palette; an explicit WindowOptions::background stays fixed.
See [the styling contract](styling.md) for subtree scopes, overrides and cache behavior.

## Event source and lifecycle hooks

`cx.window()` resolves the window whose mounted listener received the event.
Nested updates inherit that source. Shared entity identity never chooses a window.
Initialization, direct runtime updates and task completions have no implicit source.
Capture an explicit WindowHandle for background window operations and account for
its possible closure.

`.window_created(...)` runs after native creation and before showing the window;
its handle has native_window available for platform adapter setup. The built-in
AccessKit adapter is already installed at that point. Use `.accessibility(false)`
when a custom hook will own accessibility integration.
`.close_requested(...)` can return CloseResponse::KeepOpen for an OS close request,
for example while asynchronous saving completes. Once saving succeeds, use an
explicit `cx.close_window(...)`, which bypasses that veto hook. This separates an
OS request from the application's decided close operation.

`.exiting(...)` provides final synchronous cleanup with a valid AppContext. Finish
required async shutdown work before accepting close or calling `cx.exit()`; the
host does not block indefinitely on pending jobs. Exit stops accepting window
operations, and runtime disposal cancels outstanding task delivery. A custom
RunnerOptions policy can select whether closing the last window exits.

## Scheduling and embedding

The default is on-demand rendering. Input/task updates invalidate dependent
placements and request redraw through astrelis-winit. Completion uses event-loop
proxy wakeups rather than periodic polling. The host drains task results and
flushes deferred effects after update scopes end, then processes window commands.
Dirty UI preparation happens before surface acquisition.

Pointer input, wheel scrolling, Tab/Shift-Tab focus, button activation and controlled
text editing are routed in logical coordinates. The host wires native keyboard text,
clipboard, IME enable/reset/candidate geometry and on-demand caret blink deadlines.
Resize/DPI changes invalidate geometry and select
the appropriate text raster density. Model/task progression continues while
presentation is suspended; queued new windows wait for resume. Surface recovery,
retry pacing and presentation remain Astrelis responsibilities.

The host opens its own clear-color UI pass. Applications requiring custom 2D/3D
passes or complete input/platform control can use the explicit
[custom host example](../crates/rxui/examples/counter_custom_host.rs): route events,
call Ui::prepare, prepare UiPainter resources, then use UiPainter::compose to
record any opacity layers and paint into a caller-owned pass. Native hosting does
this automatically after application graphics hooks. See [group opacity](compositing.md).
UiPainter preserves the caller's scissor and supports multiple placements; call
`forget(&ui)` when removing a placement from a shared painter.

The native host installs one AccessKit adapter per window before showing it,
forwards native window events to that adapter, and handles its asynchronous
activation/action events through the same event-loop proxy used for tasks. While
active, CPU layout and semantic publication progress independently of surface
acquisition. Focus, button activation, text values/selections and scrolling route
through the existing retained controls and source-window listeners. Closing a
window drops its adapter before releasing the native window. Publication caches
are per placement and released on deactivation. The [semantics contract](semantics.md)
describes custom integration and the initial text geometry limits.

The [single-line text-input contract](text-input.md) describes implemented editing,
selection and IME behavior. Routed listeners, scrollbars and controlled splits are
covered by [the interaction contract](interaction.md). Multiline/undo editing and
virtualization remain independent
UI features rather than obligations of the native surface runner.

## Application-owned GPU work

`.prepare_graphics(...)` creates/resizes/uploads application resources before
acquisition and UI GPU preparation. It receives compatible graphics, window
metrics/format, an explicit WindowHandle and mutable AppContext. Updates are flushed
before preparing the UI; it may run on acquisition retries without a presentation.
`.render_graphics(...)` records before UI painting into the host's Frame, so offscreen
output and its image placement can share one submission. The recording hook receives
no mutable model context and leaves finish/presentation to the host. Custom hosts
can continue to own the entire sequence. See [images and graphics output](images.md)
and the standalone [framebuffer chart](../crates/rxui/examples/framebuffer_window.rs).
