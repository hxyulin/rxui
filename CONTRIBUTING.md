# Contributing to RXUI

RXUI is a declarative desktop UI framework with an RXUI-owned wgpu renderer. Read the
[guide](docs/guide.md) and the [design document](docs/next-design.md) first, then
the contract for the area you are changing. Small fixes, new examples and clear
bug reports are all useful contributions.

## Set up

- Rust stable with `rustfmt` and `clippy`. `rust-toolchain.toml` selects these.
  The minimum supported version is the `rust-version` in [Cargo.toml](Cargo.toml),
  currently 1.98.1, and CI checks it.
- [prek](https://prek.j178.dev) for Git hooks. It reads [prek.toml](prek.toml).
  [typos](https://github.com/crate-ci/typos) runs through prek, so you don't
  need to install it separately.
- A GPU is needed only to run the windowed examples and the opt-in GPU tests.
  The default test suite runs headless.

```sh
git clone https://github.com/hxyulin/rxui.git
cd rxui
prek install
cargo test --workspace --all-features
cargo run -p rxui --example gallery --features native
```

`prek install` sets up the hooks for both commit and push. On commit, they fix
whitespace and line endings, check TOML, YAML and JSON syntax, check spelling,
and run `cargo fmt`. On push, they run clippy with warnings denied.
`prek run --all-files` runs the commit hooks over the whole tree.

## Drawing and hosting

`crates/rxui-draw` owns GPU drawing, shaders, shaping and resource preparation.
`crates/rxui-host` supplies the optional native lifecycle integration. The renderer
accepts application-owned wgpu handles; see [embedding](docs/wgpu-embedding.md).
Drawing algorithms were ported from Astrelis; provenance is recorded in
`crates/rxui-draw/PORTING.md`. Changes here do not require a sibling checkout.
The optional native drag service is the independently versioned local `draglet`
workspace package. It depends on winit and platform bindings, without a renderer
or RXUI dependency. It has its own `0.1.0` version and `appkit`, `win32`, `x11`,
and `wayland` backend features. No Astrelis checkout or Git dependency is required.

## Find the right source

| Location | Purpose |
| --- | --- |
| `crates/rxui/src/` | State runtime, elements, layout, input, painting, native host and their tests |
| `crates/rxui/examples/` | Standalone, copyable programs, one per file |
| `crates/rxui/benches/` | Benchmarks for state, elements, layout, interaction, images and compositing |
| `crates/rxui/tests/fonts/` | The licensed test font |
| `docs/` | The guide, the design document and per-area contracts |
| `docs/performance/` | Benchmark reports and the raw runs behind them |
| `docs/images/` | Screenshots that the documentation references |

## Validate a change

Run these before opening a pull request. CI runs the same checks:

```sh
prek run --all-files                                                  # formatting, typos, file checks
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --workspace --no-default-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
```

Public items need documentation (`missing_docs` is on). `rxui-draw` and `rxui-host`
forbid unsafe code. RXUI denies it except in the Windows menu backend; draglet
isolates native interop in its AppKit, Win32 and Wayland modules.
When you fix a bug, add a test that fails without the fix.

GPU cache tests are opt-in because hosted CI has no GPU. The manually dispatched
`gpu.yml` workflow requires an isolated self-hosted runner labelled `rxui-gpu`,
a native adapter with working graphics drivers, and any required desktop session.
No such runner is provisioned by this repository; dispatch only trusted revisions. If you change rendering, run
them on a machine with a native adapter:

```sh
cargo test -p rxui --features rendering painting::tests -- --ignored
```

CI tests all workspace features on Linux, macOS and Windows, checks all targets,
and tests isolated RXUI layout, accessibility, tasks, image-decoding and rendering
feature subsets on Linux. Feature jobs select only `-p rxui` so they do not repeat
unrelated workspace packages. The Linux workspace job also runs draglet and host
X11 protocol tests on an isolated Xvfb display and verifies draglet's standalone
package. CI omits test debug information to keep test artifacts small. Windows
also uses bundled LLD and two build jobs to bound linking memory without dropping
test coverage. The GPU workflow runs
`rxui`, `rxui-draw` and the host's GPU device-loss test; it never runs ignored
native drag tests.
Windowed/native interaction still needs a
manual check on the affected platform; report those checks in the pull request.

## Performance changes

Benchmarks live in `crates/rxui/benches/`, for example:

```sh
cargo bench -p rxui --bench elements --locked
```

When a change touches a measured path, rerun its benchmark and update the
matching report under `docs/performance/`. Commit the raw CSV files with the
reproduction notes, as the existing reports do. Name the machine, compare against
the previous result, and keep the measurement boundaries unchanged unless the
change is about the measurement itself.

## Examples and documentation

An RXUI example is one file that someone can copy into their own project, with no
shared support module and no smoke-test mode. Declare its `required-features`
in `crates/rxui/Cargo.toml`. Draglet demos share a small software-rendering helper
in their own crate; no example rendering code is part of the draglet library.

Update the contract in `docs/` when the behavior it describes changes. The
README is the entry point. Put detailed material in [docs/guide.md](docs/guide.md)
or the per-area documents. When a screenshot in `docs/images/` no longer matches,
replace it.

## Issues and pull requests

Use the issue forms for bugs and feature requests. For a bug, include the
smallest reproducing view, the expected and actual result, a screenshot for
anything visible, your OS, and the enabled features.

Keep pull requests focused. Describe the problem and the resulting behavior, list
the checks you ran, and attach before and after screenshots for visible changes.
Explain any tradeoff a reviewer has to weigh. The pull request template gives
the structure; delete sections that don't apply.

## Licensing

RXUI is dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
Unless you explicitly state otherwise, any contribution you intentionally submit
for inclusion, as defined in the Apache-2.0 license, is dual licensed under both,
without any additional terms or conditions. Third-party material keeps its own
license; the OFL test font is an example.

GPU regression tests are opt-in, keeping ordinary tests usable without an adapter:

```sh
cargo test -p rxui-draw --lib -- --ignored --skip capture_coverage_comparison
cargo test -p rxui --features rendering --test wgpu_embedding -- --ignored
cargo test -p rxui --all-features --lib -- --ignored
```
