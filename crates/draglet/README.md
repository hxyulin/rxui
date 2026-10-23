# draglet

A small native drag utility for winit applications. Independently versioned at
`0.1.0`, with no RXUI, renderer, GPU, executor or application-host dependency.

## Backend features

- `appkit`: macOS event observation, release snapshots and native window targeting.
- `win32`: Windows pointer capture, cancellation and native client targeting.
- `x11`: standalone X11 protocol capture and input-shape/stacking-aware targeting.
- `wayland`: compositor-mediated dragging between registered client surfaces.

All four are enabled by default. Select implementations explicitly with
`default-features = false, features = ["appkit"]`, for example. These features
control draglet's implementations; winit retains its own platform feature selection.

The crate is prepared for publication but has not been published or reserved on
crates.io. It lives in the RXUI repository and has its own release version.

## APIs and platform differences

Use `draglet::Adapter` for AppKit/Win32: register windows before input, call
`begin_capture` after a verified primary press, and poll signals during a gesture.
Unregister before application-controlled window removal. The adapter is neither
`Send` nor `Sync`; native callbacks only latch state, never invoke UI code.

Use `draglet::x11::X11Adapter` on standalone X11. Register and **arm before the
initiating press**. Its separate connection receives grabbed primary input instead
of winit; bridge `X11Input` into the application and poll while armed. Xwayland and
XQuartz are rejected because their window tree cannot establish compositor-wide
foreign-window occlusion. RXUI's host now wraps this standalone implementation.

Use `draglet::wayland::WaylandAdapter` with
`event_loop.owned_display_handle()`, then register the participating windows.
Pump its queue from the event loop. It consumes real pointer serials and uses a
private data-source handshake for compositor target/drop/cancellation events.
`take_events()` reports destination-local logical coordinates. Application data
stays inside the application; this is not an external file-transfer API.

Wayland does not permit global pointer queries or explicit unrestricted pointer
capture. This backend reports those differences through capabilities rather than
approximating targets from window rectangles. It borrows the application's existing
Wayland connection and retains registered windows until unregister/drop.

The two-window `wayland_drag` example uses winit and softbuffer, without RXUI:

```sh
cargo run -p draglet --example wayland_drag --features wayland
```

## Validation status

Native input behavior remains experimental. Unit tests, protocol-state tests and
compile checks do not substitute for desktop input testing. Backend development
and the rename are checkpointed; interactive Windows, X11 and Wayland validation
is unfinished and will resume later. Do not treat the new Wayland backend as
production-validated or infer that RXUI's docking host already integrates it.

The Windows interactive capture test requires a real desktop. X11 integration
tests require an isolated server via `DRAGLET_X11_TEST_DISPLAY`; never point input
injection tests at an active user's desktop.

## Provenance

AppKit/Win32 implementations and X11 algorithms originated in `hxyulin/astrelis`
revision `4e360d5a3ad5dbe8bff11baf3eaa512a9241db21`, under MIT OR Apache-2.0.
They are maintained here with no external Astrelis dependency. Unsafe operations
are confined to the native backend modules.
