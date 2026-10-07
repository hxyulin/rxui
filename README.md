# RXUI

RXUI is being rebuilt as a declarative desktop UI framework over Astrelis, with
builder-based composition, persistent typed state, and context/closure updates.
Taffy is the selected layout engine. The agreed architecture is recorded in
[the design document](docs/next-design.md).
The [declarative core contract](docs/declarative-core.md) describes the implemented
hosting sequence, identity, resource reuse, input and current limitations.

This workspace implements typed state, declarative element builders, keyed
reconciliation, Taffy flex layout, and basic button input. The default `layout`
feature works headlessly with host-supplied text measurement. `rendering` adds
`UiPainter` over Astrelis. `tasks` adds scoped background futures and blocking
jobs; `native` adds the desktop Application host over astrelis-winit, including
those features and lazy native AccessKit integration. `accessibility` adds AccessKit
translation independently of the native host. `--no-default-features` retains the state-only runtime. Scrolling,
clipping, focus traversal and button activation are implemented. Controlled
single-line editing, selection, clipboard and IME are implemented.
Portable semantics, accessible names/roles, focus, activation, controlled values,
selection and scrolling are implemented. Themes, inherited text styling, control
state paints and grayscale dark/light presets are implemented. Shared RGBA/GPU
images, live framebuffer output, composed buttons and application graphics hooks
are implemented. Optional `image-decoding` adds PNG/JPEG decoding. Virtualization
remains a following milestone.

## Declarative views and a window

```rust
use rxui::prelude::*;

struct Counter { value: i32 }

impl View for Counter {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().padding(24.).gap(12.)
            .child(label(format!("Count: {}", self.value)))
            .child(button("Increase").key("increase")
                .on_click(cx.listener(|this, _, _cx| this.value += 1)))
    }
}

fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let counter = cx.new(|_| Counter { value: 0 });
        cx.open_window(WindowOptions::new().title("Counter"), counter)?;
        Ok(())
    })
}
```

Use `Ui::new(&mut runtime, root)` for one retained placement. Hosts call
`Ui::prepare` with a logical viewport and `TextMeasure`, then inspect the retained
snapshot or prepare/paint it with `UiPainter`. Child `Entity<View>` values can be
passed directly to `.child(...)`; separately placed entities get independent
component mounts. Keyed children preserve compatible node identity across reorder.
Duplicate sibling keys and recursive component placement are diagnosed.

The [single-file window example](crates/rxui/examples/counter_window.rs) uses
Application for native input, Tab/Shift-Tab focus, Enter/Space activation, scrollable
keyed items, resize, and a cancellable background future. It can open a second
window sharing the same model. Application discovers system fonts once by default;
`.font(...)` selects application-provided fonts, with `.system_fonts(true)` allowing
additional discovery explicitly.

```sh
cargo run -p rxui --example counter_window --features native --locked
```

Cloning an entity shares data, including any Task stored in that entity. Each
window creates its own Ui placement and retains independent focus, pointer capture
and scroll offsets. `cx.window()` inside a listener resolves the window where the
event originated, including nested updates. Initialization, ordinary updates and
task completions have no implicit source window.

[The application contract](docs/application.md) documents queued creation, native
handles, close policy and custom hosting. [The task contract](docs/async.md)
documents `cx.spawn`, live-state completions, cancellation, weak owner bindings,
error handling and executor customization. The separate
[custom host example](crates/rxui/examples/counter_custom_host.rs) implements its own
astrelis-winit Handler and completion proxy, keeping direct embedding available.
Both examples are standalone files with no test-only mode or support module.

## Controlled text input

```rust
text_input(self.name.clone()).key("name").fill_width()
    .accessibility_label("Name")
    .on_change(cx.listener(|this, edit: &TextChangeEvent, _| {
        this.name = edit.value.clone();
    }))
```

The application owns the value and can accept, normalize or reject each proposal.
Selection, composition and horizontal scrolling remain local to a placement.
Ordered edits reconcile the application answer without waiting for a frame. The
[text-input contract](docs/text-input.md) documents Unicode geometry, controlled
reconciliation, native IME, clipboard and the current single-line scope.

The [single-file editing example](crates/rxui/examples/text_input_window.rs) includes
shared windows, uppercase normalization, digit-only rejection, read-only state
and external replacement:

```sh
cargo run -p rxui --example text_input_window --features native --locked
```

## Semantics and accessibility

Buttons infer their accessible name from their caption. Name text inputs explicitly
with `.accessibility_label("Name")`; `.accessibility_description(...)` supplies help,
`.accessibility_role(SemanticRole::Form)` describes a container, and
`.accessibility_hidden(true)` excludes decorative subtrees from assistive navigation.
These builders do not change layout or create control behavior.

Application manages one AccessKit adapter per window by default and publishes only
while assistive technology is active. Accessibility actions use the same live
listeners and controlled editing path as pointer/keyboard input. Custom hosts can
read `Ui::semantics()` directly or use `AccessKitTree`. See the
[semantics contract](docs/semantics.md) for actions, identity, hosting and text limits.
The [accessibility performance report](docs/performance/accessibility.md) records
warm-cache costs and the initial-tree boundary.

## Themes and styling

`Application::new().theme(Theme::dark())` chooses the default. `Theme::light()` uses
matching metrics. Elements accept semantic tokens such as `.color(ThemeColor::TextMuted)`
and `.background(ThemeColor::Surface)`, or literal linear RGBA values. `rgb8`/`rgba8`
convert sRGB byte colors for the renderer. Text color/size inherit across component
boundaries; backgrounds, borders and dimensions stay local.

Use `.theme(...)` for a subtree and `WindowOptions::theme(...)` for a window override.
`cx.set_theme`, `cx.set_window_theme` and `cx.use_application_theme` update native
placements live. `Ui::set_theme` supports custom hosts. Explicit element overrides
remain stable, while palette-only switches preserve text measurement/layout caches.
`PaintStyle` supplies local and hover/pressed/disabled patches, with focus painted
independently. Uniform borders and corner radii are supported.

The [styling contract](docs/styling.md) explains inheritance, state precedence,
selection colors, native updates and limitations. Try the standalone gallery:

```sh
cargo run -p rxui --example theme_gallery --features native --locked
```

The [theme measurements](docs/performance/themes.md) record palette/font switch
costs and the retained-cache boundary.

## State and live listeners

```rust
use rxui::Runtime;

let mut runtime = Runtime::new();
let (counter, mount) = runtime.update(|cx| {
    let counter = cx.new(|_| 0_u32);
    let mount = cx.mount(&counter).unwrap();
    (counter, mount)
});

let increment = runtime.evaluate(&mount, |_, cx| {
    cx.listener(|count, _: &(), _cx| *count += 1)
}).unwrap();

runtime.update(|cx| increment.dispatch(&(), cx)).unwrap();
runtime.flush().unwrap();
assert!(runtime.is_dirty(&mount).unwrap());
assert_eq!(runtime.evaluate(&mount, |count, _| *count).unwrap(), 1);
```

An `Entity<T>` owns persistent state; cloning it shares that state. A `Mount<T>`
retains one placement of an entity. Separately mounting the same entity creates
independent mount identities. Listeners bind weakly to their owner and mount,
receive the owner's current `&mut T`, and stop dispatching when that mount is
removed. Ui routes activation through surviving retained button identities;
replacing or removing an element clears its focus/capture. Scroll offsets follow
retained identity; focused buttons are revealed through scroll ancestors.

Updates mutate synchronously and conservatively invalidate dependent mounts.
Reads during `Runtime::evaluate` register dependencies; each normally returned
evaluation replaces its previous read set. Reads in ordinary updates do not
subscribe. `ViewContext` offers reads and listener creation, without state
mutation capability.

Deferred work and change observers run only when the host calls `flush()`. This
allows multiple input events to share an evaluation boundary. Observers see final
state after active mutable accesses have ended; pending source notifications
coalesce. Keep a returned `Subscription` alive to keep its observer registered.
Flush has a callback budget to diagnose possible effect cycles, preserving
remaining work for retry or explicit clearing.

`Entity::read` and `Entity::update` diagnose invalid access with a panic;
`try_read` and `try_update` return `AccessError`. Weak updates and listener
dispatch are fallible. Runtime identity and generation checks reject stale or
foreign handles, and reentering the currently updated entity is rejected.
Update scopes restore access and invalidate during unwind, but do not roll back
application mutations. A panic during evaluation retains the previous dependency
set and leaves the mount dirty. A returned application `Result::Err` is ordinary
callback data, rather than a transaction rollback signal.

## Build and try

Rust 1.98.1 or newer is required.

```sh
cargo test --workspace --locked
cargo run -p rxui --example counter_state --locked
cargo check --workspace --all-features --locked
cargo bench -p rxui --bench state --locked
cargo bench -p rxui --bench elements --locked
```

[The standalone console example](crates/rxui/examples/counter_state.rs) exercises
live listeners, shared-model reads, and dirty evaluation without native/GPU
setup. Use `+`, `-`, `s` to change the step, and `q` to quit. It is a normal
interactive example, with no smoke-test mode or support module.

The state benchmark measures state-runtime operations only. It excludes UI description
construction, Taffy layout, shaping, GPU preparation, and presentation. Its CSV
median/p95 values describe averages across timed batches, not individual-event
latency percentiles.

The [initial baseline report](docs/performance/state-runtime.md) includes three
runs, raw results, reproduction instructions, and the measurement boundaries.
The [elements baseline](docs/performance/elements.md) additionally measures
builder/reconciliation/layout work using deterministic mock text sizing; it does
not measure real shaping or GPU work. The [host and scrolling check](docs/performance/host-scroll.md)
adds retained-scroll geometry costs and compares state dispatch with all features
compiled in.

The GPU cache test is opt-in on a machine with a native adapter:

```sh
cargo test -p rxui --features rendering painting::tests --locked -- --ignored
```

## Astrelis dependency and local development

The workspace manifest and lockfile pin both Astrelis crates to
Git revision
[`1f773d4a13057db8e15adc768c1d59cf979e65ed`](https://github.com/hxyulin/astrelis/commit/1f773d4a13057db8e15adc768c1d59cf979e65ed).
An ordinary clone builds against that Git source.

For edits across the sibling repositories, copy the local patch template to the
ignored configuration file and opt in explicitly:

```sh
cp .cargo/config.toml.example .cargo/local.toml
cargo --config .cargo/local.toml test --workspace --all-features
```

The patch expects `../astrelis` with `crates/astrelis` and
`crates/astrelis-winit`. The override changes Cargo.lock's source entries when
used. Save the canonical Git-source lockfile before using the override and restore that
copy afterward; do not commit the local override. `git restore Cargo.lock` is
appropriate only when the committed lockfile already contains all dependency changes
you intend to keep.

## Rewrite history

The new `main` starts without the old implementation's history. `legacy/main`
preserves the previous tracked source. A separate local export preserves the
previous working documentation and untracked prototype as well. The old remote
`main` has not been replaced. No compatibility layer with the former RXUI API is
part of this rewrite.

## Images and custom rendering

Keep shared `Image` handles in state, then use
`image(source.clone()).width(240.).height(160.).fit(ImageFit::Cover)`. Sources can
be RGBA pixels, optional decoded PNG/JPEG bytes, textures/views or a live framebuffer
color output. `button(row().child(image(icon)).child(label("Save")))` composes content
with normal control behavior; `.variant(ButtonVariant::Primary)` or Quiet selects
themed appearances.

`Application::prepare_graphics` owns allocation, resize and data uploads.
`Application::render_graphics` records application passes before UI painting in the
same frame. The [image contract](docs/images.md) covers layout, alpha, ownership,
clipping, caching and hooks. The [performance report](docs/performance/images.md)
records costs and workload boundaries. Both examples are standalone:

```sh
cargo run -p rxui --example images_window --features native --locked
cargo run -p rxui --example framebuffer_window --features native --locked
```

## Layout and stacked overlays

Rows/columns support flex growth/shrink/basis, min/max constraints, percentage sizes
and axis spacing. `stack()` overlaps children with natural sizing; `.absolute()`
and edge insets anchor overlays. `.z_index(...)` changes sibling paint and pointer
order while Tab/semantics retain description order. `PointerEvents::Block` covers
underlying content; `.inert(true)` disables background interaction/semantics without
removing its layout or painting. See [the layout contract](docs/layout.md).

```sh
cargo run -p rxui --example layout_window --features native --locked
```

## Group opacity

`.opacity(0.5)` fades a complete painted subtree once, preserving overlap between
children. Native windows handle isolated layer passes automatically; custom hosts
use `UiPainter::compose` and retain frame/pass ownership. Opacity changes keep
layout/text measurements; values zero and one need no layer pass. See
[the composition contract](docs/compositing.md) and
[performance measurements](docs/performance/compositing.md).

```sh
cargo run -p rxui --example opacity_window --features native --locked
```
