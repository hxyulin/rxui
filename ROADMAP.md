# RXUI roadmap

## Product boundary

RXUI is an idiomatic Rust, retained-mode, custom-rendered framework for
desktop tools and editors. It does not emulate a native object hierarchy.
Astrelis owns windows, text, painting, composition, and retained tree
mechanics; RXUI owns component authoring, design-system policy, reusable
application surfaces, and testing conventions.

## Release gates

- **0.1.0-rc.1:** complete the component API cutover, validate the native
  examples, and keep reviewed semantic/layout/interaction goldens green.
- **0.1 stable:** finish public API review, publish clean consumer crates, and
  validate Windows, macOS, Linux, and Web builds.
- **1.0 desktop:** native accessibility adapters, mature window/application
  coordination, and a documented compatibility policy.

## Major missing capabilities

- Native accessibility adapters. Semantic trees and actions already exist.
- Multiline and rich/code text editing with large-document virtualization.
- Component-native commands and native menus.
- Component-native file dialogs, persistence, filesystem watching, and recent
  documents.
- A structured application coordinator for multiple windows, subscriptions,
  and keyed background work.
- Localization, RTL layout, high contrast, reduced motion, and animation.

Every interactive feature should ship with keyboard behavior, focus handling,
semantic coverage, deterministic tests, and correct idle invalidation.
