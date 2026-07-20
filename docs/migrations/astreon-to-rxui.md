# Migrating from Astreon to RXUI

RXUI `0.1.0-rc.1` is the first public release of the framework previously
developed under the Astreon name. The retained UI architecture and application
model are unchanged; package names, import roots, branding identifiers, and
example paths have changed.

Update dependency names together so one version of the shared types is used:

```toml
[dependencies]
rxui = { version = "=0.1.0-rc.1", features = ["editor"] }
```

Replace Rust paths mechanically:

- `astreon` → `rxui`;
- `astreon_app` → `rxui_app`;
- `astreon_widgets` → `rxui_widgets`;
- apply the same prefix change to editor, devtools, native-menu, and testing
  crates.

Applications that intentionally persisted data under an Astreon-specific
organization or application identifier should either retain that identifier
or migrate the old directory explicitly. RXUI's bundled examples use new RXUI
identifiers and do not migrate example-only state.

The browser example canvas changed from `#astreon-canvas` to `#rxui-canvas`.
Custom host pages must update the element ID they provide.

The prerelease depends on Astrelis `=0.3.0-rc.1`, which is itself a breaking
rewrite of the old Astrelis 0.2 architecture. Direct Astrelis consumers should
also follow Astrelis's 0.3 migration guide.
