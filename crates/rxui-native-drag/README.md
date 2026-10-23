# rxui-native-drag

A small, thread-bound native mouse-drag adapter for winit applications. It has no
RXUI, rendering, GPU, executor or application-host dependency. It can be used
independently of the other packages in this workspace.

It supplies primary-button capture/tracking, capture-loss signals and native
cross-window hit testing. It snapshots the release destination before normal
platform/winit release processing, distinguishes registered client content from
foreign windows and decorations, and tracks registration generations so stale
windows cannot receive a drop. Native callbacks latch observations; they do not
invoke application UI callbacks.

## Platforms and scope

- macOS: AppKit observation and native window/client targeting.
- Windows: Win32 capture, window-message observation and client targeting.
- Other platforms: `Adapter::new()` returns an unsupported-platform error. RXUI's
  host has a separate X11 implementation; this crate does not claim Wayland support.

This is mouse gesture infrastructure for application-managed dragging, such as
moving panels between windows. It does not implement OS file/data drag-and-drop,
clipboard transfer or UI docking policies. Desktop use remains experimental;
retaining the existing implementations does not establish new platform validation.

## Lifecycle

1. Construct `Adapter` on the winit event-loop thread.
2. Register windows before the initiating primary press.
3. After an ordinary winit primary press, call `begin_capture(source)`.
4. Poll `take_signals()` and `capture_state()` while dragging, including while winit
   input is idle. Route ordinary UI input through winit separately.
5. Use `target_at_pointer()` for the authoritative destination. After release this
   returns the release snapshot, checked against current registration liveness.
6. Call `end_capture(source)` when ending the gesture and `unregister` before
   application-controlled window removal.

`Adapter` is neither `Send` nor `Sync`. Keep all operations and destruction on its
creation thread. Unsafe operations are confined to the OS modules; the public
interface is safe Rust.

## Development

```sh
mbx test -p rxui-native-drag
mbx check -p rxui-native-drag --all-targets --target x86_64-pc-windows-gnu
```

The Windows interactive capture test is opt-in and requires an actual desktop.
State/geometry/lifecycle tests run without acquiring real native capture. Set
`RXUI_NATIVE_DRAG_TRACE=1` for macOS drag diagnostics.

Platform implementations were ported from `hxyulin/astrelis` revision
`4e360d5a3ad5dbe8bff11baf3eaa512a9241db21`, under MIT OR Apache-2.0.
They are maintained here as RXUI-owned source, with no external Astrelis dependency.
