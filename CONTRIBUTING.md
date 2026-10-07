# Contributing to RXUI

RXUI is a declarative desktop UI framework built on Astrelis. Read the
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

## Working with a local Astrelis

The workspace pins Astrelis to a Git revision in [Cargo.toml](Cargo.toml) and the
lockfile, so an ordinary clone builds against it. To change both repositories
together, check Astrelis out next to RXUI as `../astrelis` and opt in to the
local patch:

```sh
cp .cargo/config.toml.example .cargo/local.toml
cargo --config .cargo/local.toml test --workspace --all-features
```

The patch rewrites the Astrelis source entries in `Cargo.lock`. Don't commit
`.cargo/local.toml` or a lockfile that points at local paths. Land the Astrelis
change first, then bump the pinned revision here in its own commit.

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

Public items need documentation (`missing_docs` is on). `unsafe` code is denied.
The only exception is the Windows menu backend, which needs it for Win32 calls.
When you fix a bug, add a test that fails without the fix.

GPU cache tests are opt-in because CI has no GPU. If you change rendering, run
them on a machine with a native adapter:

```sh
cargo test -p rxui --features rendering painting::tests -- --ignored
```

CI runs on Linux only. If you change platform-specific code, such as native
menus, dialogs or the Windows menu backend, build and run it on the affected
platform and say so in the pull request.

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

An example is one file that someone can copy into their own project, with no
shared support module and no smoke-test mode. Declare its `required-features`
in `crates/rxui/Cargo.toml`.

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
