# Contributing

RXUI follows Rust 2024 conventions and uses the stable toolchain. Public
items require documentation and Clippy warnings are treated as errors. Unsafe
code is forbidden throughout the framework.

## Setup

Astrelis is pinned to an exact git revision, so a plain clone builds as-is. To
work against a sibling Astrelis checkout, copy the example Cargo configuration,
which patches every Astrelis crate to `../astrelis`:

```sh
cp .cargo/config.toml.example .cargo/config.toml
```

Keep that patch list in sync with `[workspace.dependencies]` in `Cargo.toml`.

While that patch is active, Cargo rewrites every Astrelis entry in `Cargo.lock`
to a local path source. That form pins nothing, so **never commit `Cargo.lock`
changes made with the patch active**:

```sh
git checkout Cargo.lock
```

CI enforces this - the `boundaries` job fails if `Cargo.lock` does not pin the
revision named in `Cargo.toml`. To update the lockfile deliberately, move
`.cargo/config.toml` aside first so Cargo resolves Astrelis from git.

## Validation

Run the full local validation suite before submitting changes. This mirrors
`.github/workflows/ci.yml`:

```sh
cargo fmt --all --check
cargo test --workspace --all-features --all-targets
cargo clippy --workspace --all-features --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo build -p rxui --all-features --examples
cargo run -p rxui --example native_smoke
```

Every feature also has to compile alone, which `--all-features` cannot show
because it is a single configuration in which nothing is ever absent:

```sh
cargo clippy -p rxui --no-default-features --lib --examples -- -D warnings
cargo clippy -p rxui-widgets --no-default-features --all-targets -- -D warnings
cargo clippy -p rxui-native --no-default-features --all-targets -- -D warnings
```

CI asserts the crate-boundary invariants mechanically: `rxui-core` must not
reach wgpu, winit, arboard, taffy, or `astrelis-ui-core`; `rxui-core` must not
declare a `[features]` table; `rxui` with `--no-default-features` must clear the
same list; and `rxui`, `rxui-widgets`, and `rxui-native` must each keep a
`[features]` table. Optional surfaces belong in `rxui-widgets`, one feature
each, and that only stays enforceable while `rxui-core` has no features of its
own.

Use conventional commit prefixes such as `feat:`, `fix:`, `docs:`, `test:`,
and `refactor:`.

## API stability

During `0.x`, breaking changes are allowed only when documented in release
notes and accompanied by migration guidance in `docs/migrations/` - see
`docs/migrations/0.2-crate-recut.md` for the shape one takes.

One documented exception: `rxui::engine` is semver-exempt while RXUI is on
`0.x`. It is the surface a custom `Element` is written against, and that surface
belongs to Astrelis, which is developed alongside RXUI and not yet published. Public types should live in the
lowest crate that owns their policy. Generic engine capabilities belong in
Astrelis; application and editor conventions belong in RXUI.
