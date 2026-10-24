# Production native-host follow-ups

RXUI owns its drawing and native lifecycle code in `rxui-draw` and `rxui-host`.
There is no Astrelis Git dependency or revision pin. The initial port's provenance
is recorded in [PORTING.md](../crates/rxui-draw/PORTING.md).

## Implemented contracts

- GPU diagnostics: device-loss and uncaptured-error callbacks feed a bounded
  `GpuEvent` queue, delivered through `Application::error` and the `log` facade.
- Native backend controls: `gles` enables the wgpu GLES backend in the drawing and
  host crates. Runner-created instances carry winit's owned display handle and
  read supported `WGPU_*` settings.
- Surface latency: `WindowOptions::surface(SurfaceSettings)` exposes
  `desired_maximum_frame_latency` alongside present mode, retaining it across
  surface recreation. The default is 2 queued frames.
- Failed-frame policy: recoverable errors use runner retry pacing and capped
  surface-loss backoff; device loss and out-of-memory exit cleanly.
- Leased text atlas pages: partial preparation succeeds, and incomplete retained
  text retries after 100 ms while atlas pressure remains.

See [application hosting](application.md#runtime-errors-and-gpu-diagnostics) for
policy, backend selection, surface latency and custom-host retry contracts.
See [wgpu embedding](wgpu-embedding.md) for application-owned GPU integration.

## Remaining production work

- Automatic device/resource rebuild after device loss is not provided. The error
  hook receives the fatal diagnostic before clean shutdown. Restart with a fresh
  GraphicsContext and rebuild surfaces, UiPainter/text atlases, prepared text/draw
  geometry and all custom GPU resources. CPU models, layouts and font data can be
  retained. Recreating an event loop in-process depends on the platform.
- Verify GLES at runtime on offline Linux with the target NVIDIA/Intel drivers,
  including EGL display selection and presentation on both X11 and Wayland.
  A macOS build cannot verify those paths. Custom hosts must supply an appropriate
  owned display handle; adopting a caller-created GraphicsContext does not repair
  its instance configuration.
- Exercise real device loss, out-of-memory, repeated surface loss/recreation,
  compositor transitions and atlas lease pressure on the competition hardware.
  Verify fullscreen video/HUD behavior and the application's 75 Hz input service
  during failed frames, minimization, focus changes and recovery. Automated policy
  tests do not establish hardware/driver behavior or match-duration reliability.

Text outlines, tabular digits and element rotation remain outside this work.
