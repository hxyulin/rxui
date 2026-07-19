# Contributing

Astreon follows Rust 2024 conventions and uses the stable toolchain. Public
items require documentation, unsafe code is forbidden, and Clippy warnings are
treated as errors.

Run the full local validation suite before submitting changes:

```sh
cargo fmt --all --check
cargo test --workspace --all-features --all-targets
cargo clippy --workspace --all-features --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
```

Use conventional commit prefixes such as `feat:`, `fix:`, `docs:`, `test:`,
and `refactor:`.

## API stability

During `0.x`, breaking changes are allowed only when documented in release
notes and accompanied by migration guidance. Public types should live in the
lowest crate that owns their policy. Generic engine capabilities belong in
Astrelis; application and editor conventions belong in Astreon.
