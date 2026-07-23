# RXUI roadmap

## Product boundary

RXUI is an idiomatic Rust, retained-mode, custom-rendered framework for
desktop tools and editors. It does not emulate Qt's object model or wrap native
OS controls. Mobile, game-first HUD tooling, and a declarative reconciler are
outside the first stable release. Modern WebGPU browsers remain supported.

An unpublished component/reconciler experiment lives in `rxui-next`. It is
research for a later breaking release and does not change the 0.1 stable scope;
see `docs/rfcs/ui-next.md`.

Astrelis owns retained tree mechanics and engine primitives. General-purpose
missing hooks belong in Astrelis; application and editor policy stays in RXUI.

## Release gates

- **0.1.0-rc.1 — First public preview:** application hosting, commands,
  design-system widgets, native menus, shell conventions, persistence,
  docking, editor views, devtools, testing helpers, native/browser examples,
  and performance budgets.
- **0.1 stable:** public API review, documented SemVer policy, clean consumer
  builds from crates.io, and validated Windows/macOS/Linux release examples.
- **1.0 stable desktop:** native screen-reader adapters backed by the semantic
  tree, mature application/window conventions, and a supported compatibility
  policy.

## Major missing capabilities

- Native accessibility adapters; semantic trees and actions already exist,
  but operating-system assistive technologies cannot consume them yet.
- Multiline and rich/code text editing with line navigation, large-document
  virtualization, syntax spans, and undo integration.
- Browser folder selection; portable file-content opening and byte downloads
  are available, but browsers cannot expose native `PathBuf` values or native
  filesystem watchers.
- Later hardening for localization and RTL layout, high contrast, reduced
  motion, and animation/transitions.

Every interactive feature must ship with keyboard behavior, focus handling,
semantic coverage, deterministic tests, and correct idle invalidation.

## Post-RC message runtime follow-ups

The typed message architecture, queue control, timers, cancellable tasks,
declarative interval/filesystem subscriptions, and bounded runtime diagnostics
are complete for the first release candidate. Later releases may add:

- UI layout, semantic, and paint invalidation attribution to message traces,
  after defining a clean diagnostics boundary between RXUI and Astrelis;
- declarative receiver or stream subscriptions with explicit ownership and
  bounded delivery semantics;
- keyed task start policies such as keep-existing and replace-existing;
- opt-in message replay with explicit serialization, versioning, and treatment
  of external nondeterminism;
- latest-value coalescing for direct UI emissions and cross-thread proxies,
  where call sites can state the policy explicitly.

## Added for 0.1.0-rc.1

- Debounced native filesystem watching and portable browser file-content
  open/save services.
- PNG/JPEG/WebP decoding and retained image presentation.
- Line, bar, and scatter charts with axes, compact legends, axis-specific
  pan/zoom, viewport clamping, live latest-X following, keyboard selection,
  and deterministic decimation.
- Versioned serializable node graphs with ports, routed edges, box selection,
  keyboard editing, connection gestures, and pan/zoom.
