# Animation

Status: implemented with the default layout feature. Implicit paint transitions,
overlay enter/exit motion, smooth scrolling and `cx.animate` need a host that
drives the frame clock; the native `Application` does. The primitives (`Easing`,
`Spring`, `Transition`, `Interpolate`, `Animated`) are available in every build.

Motion is a function of the host's frame time, never of a clock RXUI reads. The
same inputs produce the same frames, so headless tests step time explicitly. When
nothing moves, no frame is requested and preparation does no extra work.

## The frame clock

`Runtime::begin_frame(time)` starts a frame at a monotonic host time (the native
host uses the time since the application started). Views read it with
`cx.frame_time()`. The first call also starts the animation clock: until then,
every transition below completes immediately, so a host that never drives frames,
and existing headless code, sees final values.

After `Ui::prepare`, a host checks `Runtime::animation_frame_requested()`. It is
true while a view asked for a frame (`cx.request_animation_frame`, or `cx.animate`
in motion) or a `Ui` placement has retained motion in flight (`Ui::is_animating`).
The native host redraws only the windows involved and goes idle once nothing is
moving. A custom host loops like this:

```rust,ignore
loop {
    runtime.begin_frame(started.elapsed());
    ui.prepare(&mut runtime, viewport, &mut painter)?;
    painter.prepare(&ui, &format, scale)?;
    // compose and present ...
    if !runtime.animation_frame_requested() {
        break; // wait for input
    }
}
```

Only the first `Ui::prepare` after `begin_frame` sees a current frame time. Hosts
also prepare between frames to route input, at the previous frame's time; motion
that starts there waits at its starting value and begins at the next frame, so a
late frame never makes a transition jump to its end.

`TestUi` prepares while `Ui::is_animating` holds, so tests call
`test.runtime().begin_frame(time)` before each `render`.

## Timing and values

`Easing` maps linear progress to eased progress: `Linear`, `EaseIn`, `EaseOut`,
`EaseInOut`, `Standard` (`cubic-bezier(0.2, 0, 0, 1)`, the default) and CSS-style
`CubicBezier(x1, y1, x2, y2)`. Curves are pinned at zero and one; Bézier X control
points are clamped to `0..=1`, Y values may overshoot.

`Spring::new(response, damping_ratio)` describes a damped spring by its undamped
period and damping ratio; `Spring::critically_damped(response)` does not overshoot,
and `Spring::from_physics(stiffness, damping, mass)` takes physical constants. Springs
are solved analytically for the elapsed time, so they are frame-rate independent.
A move ends once it is within 0.1% of its distance and nearly at rest. The ratio is
clamped to at least 0.1 so a spring always settles.

`Transition` selects how a value moves: `None` snaps, `Theme` uses the theme's
duration for the context with `Easing::Standard`, `Transition::tween(duration,
easing)` (or `ease`, `linear`) is fixed, and `Transition::spring(spring)` ends when
settled. A zero duration snaps.

`Interpolate` blends values. It is implemented for `f32`, `[f32; 2]`, `Color`
(channels clamp to `0..=1`, so spring overshoot stays a valid color),
`Option<Color>` (a missing fill fades through transparent), `Bounds`,
`ResolvedShadow` and `ResolvedPaint`. `Interpolate::components` lets a spring carry
its velocity into a new target.

`Animated<T>` is a value that moves toward its latest target. Every method takes the
current frame time:

```rust
use rxui::{Animated, Easing, Transition};
use std::time::Duration;
let ms = Duration::from_millis;
let mut x = Animated::new(0_f32);
x.set(100., Transition::tween(ms(200), Easing::Linear), ms(1000));
assert_eq!(x.value(ms(1100)), 50.);
x.set(0., Transition::tween(ms(200), Easing::Linear), ms(1100)); // continues from 50
assert!(x.is_animating(ms(1200)));
assert_eq!(x.value(ms(1300)), 0.);
```

Retargeting mid-flight starts from the current value, so the motion never jumps.
A spring also keeps its velocity projected onto the new direction.

## Explicit values in views

`cx.animate(key, target, transition)` keeps an `Animated` per placement, call site
and key, and returns the current value:

```rust
use rxui::prelude::*;
struct Drawer { open: bool }
impl View for Drawer {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let width = cx.animate("width", if self.open { 240. } else { 48. }, Transition::Theme);
        column().width(width).fill_height().background(ThemeColor::Surface)
    }
}
```

The first evaluation returns the target. A later evaluation with a different target
moves there and requests an animation frame on every evaluation until the value
settles. The view is then evaluated again only when its state changes. Animated
values can drive anything, including layout, because the view describes them like
any other value. Each moving frame re-evaluates the view, so animate the smallest
component that needs it. The slot is released with the placement. `Transition::Theme`
peeks at the placement theme's `ThemeMotion::state` without subscribing the view to
theme changes.

## Implicit state transitions

Retained elements interpolate their resolved paint when it changes: background,
border and text colors, radii, the focus color and width, selection, caret and
preedit colors, and the box shadow. These changes animate:

- hover, pressed and pointer capture;
- keyboard focus: the focus ring fades in when it becomes visible (including the
  first keyboard input after pointer focus), and disappears immediately on blur;
- disabled changes from a new description;
- explicit paint changes (`.background(...)`, state styles, a selected tab's style);
- theme switches, including palette-only switches.

Buttons, text inputs and elements with `hover_style`, `pressed_style` or
`disabled_style` default to `Transition::Theme`, which uses `ThemeMotion::state`
(120 ms in every preset). The built-in tabs, menus, menu items and dock headers are
buttons, so they animate without configuration. Other elements animate only theme
switches, so a theme change crossfades the whole window. Configure an element with
`.transition(Transition::spring(...))`, or opt out with `.no_transition()` when the
view animates that element's paint itself. Composed button content follows its
button's moving foreground.

Only paint and opacity move. Layout, text shaping and hit testing use the final
values immediately, and `ElementInfo::hovered`, `pressed`, `focused` and `disabled`
report the current state; only `ElementInfo::paint` is in motion. A transition
starts at the first frame after the change, from the paint displayed then. A
transition interrupted by another change continues from the blended paint.
Interpolated paint is clamped, so springs never produce invalid colors. Elements
that were not painted in the previous frame (new, hidden or clipped out of view,
like rows scrolled away) change without a transition.

Set durations per theme through `ThemeMetrics::motion`:

```rust
use rxui::prelude::*;
use std::time::Duration;
let theme = Theme::dark().metrics(|m| {
    m.motion.state = Duration::from_millis(80);
    m.motion.overlay_exit = Duration::ZERO; // close menus and dialogs instantly
});
```

`Theme::compact` keeps the motion metrics. A zero duration disables that kind of
motion.

## Overlay enter and exit

Popovers and modal dialogs fade in over `ThemeMotion::overlay_enter` (150 ms) and out
over `ThemeMotion::overlay_exit` (100 ms). Anchored popovers also slide by
`ThemeMotion::overlay_offset` (4 units) from the side of their anchor, following any
placement flip; modals only fade, so their scrim keeps covering the viewport.
`Overlay::transition(...)` selects a different timing for both directions, and
`Transition::None` snaps.

Opening is immediate for everything but the visual: focus moves into the surface,
semantics publish the dialog or menu, and Escape, outside presses and Tab work from
the first frame.

Closing removes the overlay from the tree as before. Its focus is restored, its
component placements, listeners and local state are released, it leaves the
semantic tree and hit testing, and a modal stops blocking the background, all in
the same preparation. Only the visual lingers: the removed subtree is kept as a
paint-only snapshot with its last displayed paint, which `Ui::elements` reports
(inert, unfocusable, with no interaction state) and the painter fades out. Pointer
input during the exit reaches the content underneath. Reopening an overlay with the
same key and parent while it fades out continues from its current opacity instead
of flashing.

The fade uses group opacity: the overlay is drawn into an isolated layer, as for
`.opacity(...)` (see [compositing](compositing.md)), only while it moves. The slide
offsets that layer's composite by whole physical pixels, so nearest sampling stays
exact; scaling would blur under nearest sampling and is not offered. Custom
painters see the fade in the overlay root's `ElementInfo::opacity`, with
`viewport_overlay` set on both open and exiting roots; the slide is applied only by
`UiPainter`.

## Smooth scrolling

A scroll container's `ScrollBehavior` selects how it moves for discrete mouse-wheel
steps and programmatic commands. `Smooth` eases the offset to its new value over
`ThemeMotion::scroll` (150 ms in every preset) with `Easing::EaseOut`; `Instant`
jumps. Containers default to `ScrollBehavior::platform()`: Smooth on macOS and
Windows, whose system scroll views and browsers animate wheel steps, and Instant on
other platforms, where GTK and Qt scroll views move by whole steps. Set it with
`.scroll_behavior(...)` on an element with `.scroll_y()` and friends, on
`scroll_area`, or on a virtual list. A zero `ThemeMotion::scroll` makes every
container instant.

What animates on a Smooth container:

- `Ui::wheel_with_source` with `WheelSource::Lines`. The native host sends winit's
  line deltas this way, 40 logical units per line. A step taken while a smooth
  scroll moves adds to its target, so quick steps accumulate, and motion past the
  container's end chains to scrolling ancestors as before.
- `scroll_to`, `scroll_by` (relative to the target in flight), `reveal_row`,
  `reveal_span` and `reveal_list_row` on a `ScrollHandle` or `ScrollPlacement`.

What always moves directly:

- Precise deltas: `Ui::wheel`, `Ui::scroll`, and `WheelSource::Precise`, which the
  native host uses for trackpad pixel deltas. They are already smooth, and
  animating them would add latency.
- `jump_to`, scrollbar dragging, track clicks and range keys, assistive Scroll
  actions, focus reveal and caret reveal in text areas and inputs.
- Scroll anchoring corrections in [variable-height lists](virtual-lists.md).

A direct change to a container ends its smooth scroll and keeps the new offset, so
typing in a text area mid-scroll reveals the caret and stays there, and a trackpad
gesture takes over from a wheel animation. An anchoring correction instead moves
the scroll in flight with the content: its start and target shift by the same
amount, so it still ends on the content it was heading to.

Each `Ui::prepare` first writes the offsets at the frame time, then evaluates views,
lays out and publishes scroll metrics. `ScrollHandle::state` reports the offset
being displayed, so a virtual list mounts the rows of each intermediate offset as
the motion proceeds. A command applied during the first preparation of a frame
starts its move at that frame; wheel input between frames starts at the next one,
as other motion does. Moves end at their target or when the container is removed.
While a smooth scroll moves, `Ui::is_animating` holds and the hover target follows
the content under a still pointer; hover listeners update at the next pointer
motion, as after `Ui::scroll`. Each moving frame refreshes geometry and
re-evaluates views that read the container's metrics, the same work as a direct
scroll per frame.

## Reduce motion

`Runtime::set_reduce_motion(true)` makes all of the above complete immediately:
transitions, overlay enter/exit, smooth scrolling and `cx.animate`, including motion
in flight at the next frame. No frames are requested for them. `Runtime::reduce_motion()`,
`AppContext::reduce_motion()` and `cx.reduce_motion()` read the preference, and
`AppContext::set_reduce_motion` lets an application offer its own setting. Views
that call `cx.request_animation_frame()` directly should check
`cx.reduce_motion()` themselves.

The native `Application` host applies `SystemPreferences::reduce_motion` at
startup and whenever that operating system setting changes, before
`system_preferences_changed` runs. A value set with `AppContext::set_reduce_motion`
stays in effect until the operating system setting itself changes again. Custom
hosts call `runtime.set_reduce_motion(value)` themselves; the runtime never queries
the operating system.

## Costs and verification

While idle, `Ui::prepare` compares four interaction targets with the previous
frame and, when overlays exist, their root list; no motion state is allocated and
nothing requests a frame. Starting a transition stores the previous paint for that
element. While anything moves, each frame advances a small table and the painter
blends paint only for elements it actually paints. A theme switch records one entry
per on-screen element whose paint changes. Overlay motion rebuilds the composition
plan each frame and adds one layer pass for the moving overlay, as group opacity
does. Paint transitions never touch text preparation, layout or semantics.

[Animation measurements](performance/animation.md) compare the per-frame cost
before and after this feature. Idle frames cost about 10 ns more for the state check.

Headless tests cover easing and spring math, value retargeting and spring
velocity, hover/pressed/disabled/focus/theme transitions, retargeting without jumps,
settling and stopping frame requests, idle frames without work, reduce motion and
unclocked hosts, `cx.animate` re-evaluation, and overlay enter/exit, including
immediate focus restoration, semantic removal, hit testing through a fading modal
and reopening. Smooth scrolling tests step frame times through programmatic
scrolls, accumulating wheel steps, trackpad deltas that stay direct, reduce motion,
`jump_to`, caret reveal ending a scroll, anchoring during a scroll and virtual list
rows mounting along the way. An opt-in GPU test reads back an overlay's pixels
while it fades in and while its visual lingers after closing:

```sh
cargo test -p rxui --all-features animation
cargo test -p rxui --all-features scroll_motion
cargo test -p rxui --features rendering animation_gpu -- --ignored
cargo run -p rxui --example animation_window --features native --locked
```

## Limitations and follow-ups

- Smooth scrolling uses a fixed duration whatever the distance, so long reveals
  move fast. Scrollbar range keys (arrows, PageUp/PageDown, Home/End) and text
  editing page movement scroll directly; there is no momentum or overscroll bounce.
- Implicit transitions never change layout; animate sizes and positions explicitly
  with `cx.animate`.
- The focus ring fades in but disappears immediately when focus moves.
- Image tints and the native window clear color change immediately; a root view
  that paints its own background crossfades.
- Popover motion is a slide and fade; there is no scale.
- A window resized while an overlay fades out keeps the ghost at its last position.
