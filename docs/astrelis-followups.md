# Astrelis follow-ups for production native hosts

RXUI remains pinned to Astrelis `5e5ea9df12f3539661b8d487a83b907e63641bfa`.
No Astrelis source or workspace revision is changed by the input/hosting work.
References below use paths and lines in that pinned checkout, readable at
`~/.cargo/git/checkouts/astrelis-7a2542a700c0f8e3/5e5ea9d`.

## GLES initialization and backend selection

RXUI's optional `gles` feature adds `wgpu = "30.0.1"` with default features disabled
and `features = ["gles"]`. Cargo feature unification enables GLES in the same wgpu
used by Astrelis. It does not itself change graphics initialization or guarantee
Linux GL presentation at this pin. Compile with:

```sh
cargo check -p rxui --features native,gles
```

Astrelis `crates/astrelis/src/context.rs:94` and `:113` create
`wgpu::Instance::default()` for surface and headless paths respectively. In wgpu
30.0.1, `src/api/instance.rs` implements that default with
`InstanceDescriptor::new_without_display_handle()`. The registry source
`wgpu-types-30.0.1/src/instance.rs:51` explicitly requires a display handle for
GLES presentation, especially Wayland, and recommends winit's
`EventLoop::owned_display_handle()`.

`wgpu-hal-30.0.1/src/gles/egl.rs:774` chooses the EGL Wayland/X11/XCB platform from
the instance descriptor's display handle. At `:850`, the missing-display path can
select a Mesa surfaceless display, which is unsuitable as a general windowed GL
fallback. Passing the window to `create_surface` afterward does not repair the
instance's EGL display selection. This matters on X11 as well as Wayland.

Required dependency work: carry an owned display handle from the runner into
surface-aware context creation, use
`InstanceDescriptor::new_with_display_handle(...).with_env()`, and expose a native
Astrelis `gles` feature. `.with_env()` is the explicit opt-in for `WGPU_BACKEND` and
other supported `WGPU_*` settings; `Instance::default()` does not read them.
Keep the headless initialization path distinct. The relevant native call chain is
`crates/astrelis-winit/src/runner.rs:126`,
`crates/astrelis-winit/src/window.rs:52` and
`crates/astrelis/src/context.rs:90`.

This work is covered by the parallel Astrelis thread at `~/dev/projects/astrelis`,
branch `fix/runtime-resilience`. RXUI deliberately does not duplicate it. Once that
work lands and a separately authorized re-pin is made, replace the temporary
direct-wgpu feature bridge with forwarding to `astrelis/gles`. Applications that
already own suitable wgpu handles can use `GraphicsContext::from_wgpu`
(`crates/astrelis/src/context.rs:245`) and `Application::graphics`; RXUI does not
create an additional instance or silently override that application's settings.

Runtime GLES on Linux, EGL driver selection on NVIDIA/Intel, and X11/Wayland
presentation are not verified by a macOS compilation check.

## Runtime resilience and latency

These are outside the RXUI input changes and are covered by the same parallel
Astrelis thread. The pinned behavior still applies until a later re-pin:

- `crates/astrelis-winit/src/runner.rs:290`: `Runner::fail` stores a fatal error and
  exits the loop. Acquisition failures at `:475` and callback failures need a
  recoverable/fatal policy, useful logging and surface/device-loss recovery.
- `crates/astrelis/src/context.rs:230`: device creation installs no explicit
  device-lost or uncaptured-error callbacks. Recovery and error reporting belong
  with the graphics context and runner lifecycle.
- `crates/astrelis/src/text/renderer.rs:1575`: atlas budget exhaustion can return
  `TextRenderError::AtlasFull`. Budget trimming already exists at `:810`; eviction,
  retry and runner policy must cooperate so transient exhaustion does not quit an
  application.
- `crates/astrelis/src/target.rs:158` takes wgpu's default surface configuration;
  `crates/astrelis-winit/src/policy.rs:85` exposes no
  `desired_maximum_frame_latency` setting. Expose and validate it through the
  existing surface options/settings rather than changing it from RXUI.

Text outlines, tabular digits and element rotation remain outside this thread.
