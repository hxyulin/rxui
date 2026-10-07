# Window geometry and native file drops

These APIs are part of `native`. They need no new dependencies or Astrelis changes.
The application owns persistence, window keys and document/import decisions.

## Persistable window geometry

`WindowGeometry` describes the normal window's logical content size, optional
physical desktop frame position and whether it should reopen maximized:

```rust
let saved = WindowGeometry {
    inner_size: [900., 600.],
    outer_position: Some([120, 80]),
    maximized: false,
};
let options = WindowOptions::new().title("Workspace").geometry(saved);
```

`WindowOptions::geometry` applies the size, position and maximized attributes and
enables monitor-aware restoration at native creation. Later `.size` or
`.native_attributes` calls override their corresponding attributes. Nonfinite or
nonpositive sizes and nonfinite logical positions fail before window reservation.
Physical custom attributes wait for the native window's actual scale before being
reported as logical geometry; no guessed scale leaks into the pending snapshot.

Restoration chooses the monitor nearest the saved frame origin, or a host-preferred
monitor when no origin is provided. Oversized content is capped to the monitor's
logical bounds with space reserved for decorations. Origins are clamped to keep
the estimated frame on that monitor. Missing monitor information defers placement
to the OS. This handles removed displays, negative desktop coordinates and changed
DPI, but does not identify monitors by persistent hardware identity.

Winit exposes full display bounds rather than the usable work area, so this cannot
promise exact clearance from every dock/taskbar. Native minimum/maximum constraints,
platform decorations and the compositor can adjust the final size/position.
Saved outer-frame placement is applied after native creation and before showing
and maximizing. This avoids macOS's content-area interpretation of creation
position attributes, which would otherwise shift the saved origin on each launch.
Position queries/setters are unavailable on Wayland; a live snapshot reports `None`
there. Fullscreen selection remains an explicit winit/native-attributes decision.

`window.geometry()` returns the most recent persistable snapshot, including after
close. Pending logical requests are available before native creation; requests that
need actual DPI remain unavailable until creation. It is a snapshot, not a view
dependency subscription.
If custom native attributes create an already maximized window without specifying
any normal size, the snapshot remains unavailable until normal bounds are observed;
RXUI does not invent a restore size.

```rust
Application::new().window_geometry_changed(move |window, geometry, cx| {
    // Update the model associated with window.id(), or a stable application key.
    // Retain the latest value; debounce/background any ongoing disk writes.
});
```

The hook receives an initial native snapshot and subsequent changed snapshots.
It runs outside active entity leases in a fresh AppContext update, with the window
explicitly supplied and no implicit `cx.window()` source. Native move/resize/DPI/focus
events queue one coalesced proxy wake per window. Observation waits until the
native operation settles: on macOS, zoom can report enlarged bounds before the
maximized flag changes. No timer polls geometry. Unchanged observations
produce no callback. Native minimization, zero-size surfaces and fullscreen bounds
do not overwrite normal bounds. Maximization changes the flag while retaining the
normal size/position. Closing windows stop delivery but keep their last snapshot.

RXUI does not pick a preferences directory, serialization format or global window
identifier. Persist by a stable application key such as `workspace` or `inspector`;
runtime WindowId values are not stable across launches. An application can write
periodically with background tasks, before completing an asynchronous shutdown,
or after `Application::run` returns. Disk writes are never part of the host's
geometry observer.

## Native file notifications

```rust
Application::new().file_drop(move |window, event, cx| {
    if let FileDropEvent::Dropped(path) = event {
        // Locate this window's view and update it through entity.update(cx, ...).
        // Schedule reads/decoding with spawn_blocking inside that update.
    }
});
```

`FileDropEvent` has `Hovered(PathBuf)`, `Cancelled` and `Dropped(PathBuf)` variants.
Multiple files produce separate hover/drop deliveries in native event order; one
cancellation clears the complete hover preview. A drop may have no prior hover.
Winit provides neither a reliable position nor a batch-end notification with these
events, so RXUI exposes a window-level hook and does not infer element targets from
the last mouse event. The hook is a notification, not native drag acceptance or
copy/move-effect negotiation. Internal docking/pointer dragging remains separate.

The native managed window is supplied explicitly. Callbacks run outside entity
leases; closing windows and an exiting app suppress delivery. Files are not opened,
read or inspected by RXUI. Paths can identify directories, unavailable files or
items the app does not support; application validation and worker jobs decide what
to accept. Keep UI callbacks short. This is path delivery, not a URI/document-object,
sandbox bookmark or arbitrary native drag-payload abstraction.

Support follows the pinned winit 0.30 backend. macOS, Windows and X11 implement
file notifications; the Wayland backend currently does not. Windows custom native
attributes can explicitly disable winit's drag-and-drop support. Applications still
need an Open/picker path where native drops are unavailable.

## Standalone example

```sh
cargo run -p rxui --example window_state --features native --locked -- /path/to/window-state.txt
```

[window_state.rs](../crates/rxui/examples/window_state.rs) contains its own view,
window and hooks. It retains the latest geometry in memory and writes it after
orderly event-loop exit, then restores it on the next launch. Without a supplied
path it uses a temporary file, which the OS may remove. Malformed state falls back
to defaults. The simple versioned text format illustrates application storage;
production preferences may need atomic replacement, version migration and recovery.
The example also displays hovering state and accepted paths without reading them.

## Validation

A separate inactive, normal-level macOS probe exercised offscreen restoration,
native programmatic resize/move, zoom/unzoom, saving and repeated reopening in
normal and maximized states. Its 780 × 480 logical normal bounds and physical
outer origin `[120, 140]` survived reopening exactly. Native testing caught and
fixed zoom notification ordering and the content/outer-origin mismatch described
above. The example itself has no smoke/test mode or probe controls.

Synthetic native-event tests cover hover/cancel/drop mapping, per-file order,
unmodified Unicode paths, distinct managed window identity, fresh model updates,
and closing/exiting delivery suppression without requiring layout or GPU work.
The automated Finder drag did not produce an OS drop in the probe, so real
file-manager drag behavior remains unverified here. Windows has compilation and
Clippy coverage; native Windows/X11 runtime checks remain outstanding.
