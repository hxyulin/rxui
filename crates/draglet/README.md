# draglet

Native drag mechanics for [winit](https://github.com/rust-windowing/winit) applications:
track a primary-button gesture, identify registered destination windows, and
observe release or cancellation. Bring your own UI, renderer and application data.

Draglet is an independently versioned Rust library (`0.1.0`) in the RXUI repository.
It has **no RXUI, Astrelis, GPU, renderer, executor or application-host dependency**.
It does not create your event loop or windows. The examples are small standalone
applications, not a required host framework.

The crate is not yet published on crates.io. Until publication, use a Git or path
dependency. Its version and future releases are independent of RXUI.

## Install

Select the backend your application uses:

```toml
[dependencies]
draglet = { git = "https://github.com/hxyulin/rxui", default-features = false, features = ["wayland"] }
winit = "0.30.13"
```

For a local checkout, replace `git` with `path = "../rxui/crates/draglet"`.
After publication, `version = "0.1"` can replace the Git/path source. Rust 1.98.1
or newer is required. Draglet has no workspace-inherited package fields and can
also be built from a copy of its crate directory alone.

| Feature | API | Use it for |
| --- | --- | --- |
| `appkit` | `draglet::Adapter` | macOS AppKit windows |
| `win32` | `draglet::Adapter` | Windows Win32 windows |
| `x11` | `draglet::x11::X11Adapter` | A standalone X11 server, including Xorg or Xvfb |
| `wayland` | `draglet::wayland::WaylandAdapter` | Compositor-routed drags between registered surfaces on one Wayland connection |

All four features are enabled by default. These switches select draglet's
implementations; winit's platform features are configured independently. For a
Linux application supporting both display systems, enable `x11` and `wayland`,
then choose the adapter matching the event loop's actual display backend.
`draglet::Adapter` does not automatically select a Linux backend.

## Try the standalone demos

From this repository:

```sh
# macOS (use --features win32 on Windows)
cargo run -p draglet --example native_drag --no-default-features --features appkit

# Native Linux X11 session; XWayland is deliberately rejected
cargo run -p draglet --example x11_drag --no-default-features --features x11

# Native Linux Wayland session
cargo run -p draglet --example wayland_drag --no-default-features --features wayland
```

Each demo opens blue and green windows using winit and softbuffer, without RXUI or
a GPU renderer. Arrange them side by side, press in a colored client area, drag
to the other window and release. Watch the terminal for source IDs, target IDs,
logical target positions and release/drop or cancellation events. Try releasing
outside both windows, then start another drag. Close either window to exit.
Window decorations are not client drop targets.

The X11 and AppKit/Win32 examples report native release targets; the application
still decides whether a target accepts its payload. The Wayland example reports
`Dropped` only after the compositor's private drag handshake succeeds.

To run without the RXUI workspace:

```sh
cd crates/draglet
cargo run --example wayland_drag --no-default-features --features wayland
```

Or copy `crates/draglet` elsewhere and run the same command there. The published
package includes the examples and their shared software-rendering helper.
`softbuffer` is a development dependency only.

On Ubuntu, example build prerequisites include `build-essential`, `pkg-config`,
`libxkbcommon-dev` and `libwayland-dev`. X11 protocol tests additionally need
`xvfb` and `xauth`. On Wayland, the runtime must be able to load
`libwayland-client.so.0`; unusual mixed Nix/system environments may need their
library search path configured. No system configuration changes are made by the
library or demos.

## Integrate into your application

Keep each adapter on the window/event-loop thread. Register participating windows
before input and unregister before application-controlled window destruction.
Poll while a gesture is active, including when ordinary winit input is idle. The
examples use an 8 ms `ControlFlow::WaitUntil` wake; choose your own scheduling.
Keep payloads, drag thresholds, acceptance rules, previews and application state
in your UI. Draglet supplies native mechanics, not a widget or payload registry.

### AppKit and Win32

Create an adapter and register your existing windows:

```no_run
# fn setup(window: &std::sync::Arc<winit::window::Window>) -> Result<(), draglet::Error> {
let mut adapter = draglet::Adapter::new()?;
adapter.register(window)?;
# Ok(())
# }
```

Keep that adapter for the lifetime of the registrations. After a real primary
press in a registered window, call `begin_capture(window.id())`. Continue routing
normal input through winit. During the gesture, call `take_signals()` and
`capture_state()`; `target_at_pointer()` reports the live target while held and
the native release snapshot after `Released`. Consume the release target **before**
calling `end_capture(source)`. A `Lost` state cancels the operation and must never
commit a drop. Call `end_capture(source)` after release/cancellation or when your
application cancels the gesture.

Native callbacks only latch observations, never call application UI code. AppKit
observes the native gesture; Win32 uses pointer capture. Do not assume identical
platform capture behavior. `Adapter` is neither `Send` nor `Sync`.
See [`native_drag`](https://github.com/hxyulin/rxui/blob/main/crates/draglet/examples/native_drag.rs) for the complete event-loop lifecycle.

### X11

1. Open `X11Adapter::connect(None)` on a native X11 session, or pass an explicit
   display name for an isolated server. Register existing winit Xlib/XCB windows
   on that same server/screen with `register(&window)`.
2. Call `arm(window.id())` **before** the initiating primary press. This installs
   a passive grab; primary input then arrives on draglet's separate connection.
3. Drain `poll_input()` while armed. Bridge `X11Input` into your normal UI input
   system, including clicks that never become drags. Do not also manufacture a
   second primary event from winit.
4. After a polled `Pressed`, call `acquire_capture(source)`. During the gesture,
   query `target_at_pointer()`. On `Released`, query the target before calling
   `release_capture(source)`; on `Cancelled`, abort and release.
5. Call `unregister(id)` before destroying registered windows.

Targets respect native stacking, foreign-window occlusion and input shapes.
Raw X11 hosts can use `from_connection` and `register_xid`; callers then own XID
lifetimes and scale updates. Keep the protocol connection private from other
capture/event consumers. This backend supports one screen and primary-button
gestures; it does not bridge button chords or wheel events during the grab.

XWayland and XQuartz are rejected: their window trees do not describe native
compositor windows, so they cannot provide the backend's desktop-wide targeting
guarantee. Xvfb is suitable for isolated protocol testing; it does not replace
interactive testing with a desktop window manager.
See [`x11_drag`](https://github.com/hxyulin/rxui/blob/main/crates/draglet/examples/x11_drag.rs) for input bridging and capture ordering.

### Wayland

Construct `WaylandAdapter::new(event_loop.owned_display_handle())` on the event-loop
thread, then `register(&window)` for each participating window. The adapter uses
winit's existing libwayland display connection with its own event queue; it does
not open a second application connection or replace winit's handlers. Construction
performs two discovery round trips; subsequent `pump()` calls are nonblocking.

After a real primary press, `begin_capture(source)` pumps pending events and
consumes a native pointer serial for that registered source. Pump from the event
loop, then drain `take_events()`:

- `TargetChanged` updates the registered destination and its local logical position.
- `Dropped` authorizes the compositor-reported destination; apply application
  acceptance rules before moving your payload.
- `Cancelled` aborts the gesture. Leaving/releasing outside registered surfaces
  must never be interpreted as a successful drop.

After `capture_state()` becomes `Released` or `Lost`, call `end_capture(source)`.
Application cancellation also uses `end_capture`. New gestures require fresh
native presses. Wayland controls the pointer grab; destroying the private source
cancels participation rather than issuing a global pointer-ungrab request.

There are no global pointer queries or unrestricted pointer capture on Wayland.
`capabilities()` reports `pointer_capture: false`; authoritative targeting applies
only to this compositor-routed drag session. `Unavailable` outside registered
surfaces does not identify which foreign application is underneath. The adapter
retains registered windows until unregister/drop to keep borrowed handles alive.
A pointer-capable seat and data-device manager version 3 are required.

This is **same-connection application drag routing**, not an external file,
clipboard or arbitrary OS data-transfer API. Application payloads remain in your
application; the compositor sees only a private handshake MIME type. RXUI's docking
host does not yet automatically integrate this backend.
See [`wayland_drag`](https://github.com/hxyulin/rxui/blob/main/crates/draglet/examples/wayland_drag.rs) for a complete standalone application.

### Coordinates and lifetime rules

`Target::Window { position, .. }` uses **destination-local logical coordinates**.
`Signal::Motion`, `Signal::Released` and `X11Input` use **source-local physical
coordinates**. Convert deliberately when passing events to your UI. `Foreign`,
`NoWindow` and `Unavailable` are not registered drop destinations.

Registration identity must remain live through release handling. Unregister before
removing a window, cancel when a source disappears, and never accept a stale
cached hover as a release target. Native failures are errors or unavailable/lost
states, not permission to fall back to rectangle-based targeting.

## Validation

Current native testing is a starting point, not a cross-platform compatibility
promise:

- **Wayland:** a live KDE Plasma session, with compositor input via a temporary
  virtual pointer: both cross-window directions, same-window drop, cancellation
  over an unrelated window/desktop, and a fresh successful gesture after cancellation.
- **X11:** Ubuntu 24.04 with an isolated Xvfb display: server stacking, foreign
  occlusion, input shapes, decorations, destination scaling, pointer capture and
  source-unmap cancellation. Full desktop/window-manager interaction remains untested.
- **AppKit:** native unit tests; this example has not yet received an interactive
  validation pass.
- **Win32:** cross-compilation checks; native runtime testing remains pending.

Run normal tests with `cargo test -p draglet --all-features`. To include the X11
server tests, use a disposable display, never an active user's desktop:

```sh
xvfb-run -a -s '-screen 0 1280x800x24 -nolisten tcp' sh -c \
  'DRAGLET_X11_TEST_DISPLAY="$DISPLAY" cargo test -p draglet --all-features -- --include-ignored --test-threads=1'
```

The Windows interactive capture test requires a real desktop; run it separately
on Windows. Do not enable all ignored tests indiscriminately on a user's desktop.

## Provenance and license

MIT OR Apache-2.0. AppKit/Win32 implementations and X11 algorithms originated in
`hxyulin/astrelis` revision `4e360d5a3ad5dbe8bff11baf3eaa512a9241db21` under the same
licenses. They are maintained here without an Astrelis dependency. Unsafe native
interop is isolated in the platform modules; X11 uses the safe x11rb protocol API.
