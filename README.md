# RXUI

RXUI is being rebuilt as a declarative desktop UI framework over Astrelis, with
builder-based composition, persistent typed state, and context/closure updates.
Taffy is the selected layout engine. The agreed architecture is recorded in
[the design document](docs/next-design.md).

This workspace implements the first milestone: a headless, single-thread state
runtime. It does not yet provide element builders, widgets, layout integration,
painting, or an application/window host. The `rendering`, `layout`, and `native`
features expose the selected dependencies for those later milestones.

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
removed. Element routing and keyed widget lifetimes will build on this contract.

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
```

[The standalone console example](crates/rxui/examples/counter_state.rs) exercises
live listeners, shared-model reads, and dirty evaluation without native/GPU
setup. Use `+`, `-`, `s` to change the step, and `q` to quit. It is a normal
interactive example, with no smoke-test mode or support module.

The benchmark measures state-runtime operations only. It excludes UI description
construction, Taffy layout, shaping, GPU preparation, and presentation. Its CSV
median/p95 values describe averages across timed batches, not individual-event
latency percentiles.

The [initial baseline report](docs/performance/state-runtime.md) includes three
runs, raw results, reproduction instructions, and the measurement boundaries.

## Astrelis dependency and local development

The workspace manifest and committed-source lockfile pin both Astrelis crates to
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
used. Restore the canonical Git-source lockfile before committing; do not commit
the local override. During this initial uncommitted workspace, save a copy of
Cargo.lock before using the patch. After the first commit, `git restore Cargo.lock`
restores the canonical version.

## Rewrite history

The new `main` starts without the old implementation's history. `legacy/main`
preserves the previous tracked source. A separate local export preserves the
previous working documentation and untracked prototype as well. The old remote
`main` has not been replaced. No compatibility layer with the former RXUI API is
part of this rewrite.
