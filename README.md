# RXUI

A retained-mode Rust UI framework with GPUI-style ergonomics — entities,
closure event handlers, and fluent element builders — over an incremental
dirty-bit engine, built on the [Astrelis](https://github.com/hxyulin/astrelis)
platform/GPU/paint/text stack.

**Status: v2 rewrite in progress.** This branch is a fresh root history; the
design and staged implementation plan live in
[`docs/plans/`](docs/plans/v2-rewrite.md).

Previous architectures are preserved as archive branches:

- `archive/v1-main` — the message/`App` framework (8 crates).
- `archive/v1-ui-next-migration` — the Elm-style `Component` framework over
  `astrelis-ui-next` (5 crates). Salvage source for the v2 reconciler, dirty
  flush, test harness, and instrumentation.

## License

MIT — see [LICENSE-MIT](LICENSE-MIT).
