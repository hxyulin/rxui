# Astrelis follow-ups for production native hosts

RXUI is pinned to Astrelis `4b0d3c54a5c7b00c08bef3a3dea484bc27a909be`.
The dependency work from the input/hosting thread is complete and adopted:

- GPU diagnostics and native backend controls (`fa18e46`): device-loss and
  uncaptured-error callbacks, bounded `GpuEvent` queue, runner delivery through
  RXUI's `Application::error`, and logging through the `log` facade. RXUI does not
  drain the runner's queue a second time.
- Native GLES initialization (`fa18e46`): RXUI forwards `gles` to Astrelis and
  astrelis-winit, replacing the temporary direct-wgpu feature bridge. Runner-created
  instances carry winit's owned display handle and read supported `WGPU_*` settings.
- Surface latency (`fa18e46`): `WindowOptions::surface(SurfaceSettings)` exposes
  `desired_maximum_frame_latency` next to present mode, retaining it across surface
  recreation. The default is 2 queued frames.
- Failed-frame policy and surface recovery (`efcb186`): RXUI continues recoverable
  frame errors, exposes an application policy/observer, and uses runner retry pacing
  and capped surface-loss backoff. Device loss and out-of-memory still exit cleanly.
- Leased text atlas pages (`4b0d3c5`): partial preparation succeeds; RXUI retries
  incomplete retained text, placeholders and chunks. Native retries request a redraw
  after 100 ms while atlas pressure remains.

See [application hosting](application.md#runtime-errors-and-gpu-diagnostics) for
policy, backend selection, surface latency and custom-host retry contracts.
Astrelis documents the underlying contracts in `docs/winit.md` and `docs/text.md`.

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
