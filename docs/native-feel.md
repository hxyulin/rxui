# Native feel: system appearance, cursors and window chrome

Status: implemented with `native`; the portable pieces (`ThemeSet`, `ThemeMode`,
`SystemPreferences`, focus rings, scrollbar styles, dialog order, drag-region
hit testing) are part of `layout` and work headlessly. Detection uses safe
bindings only: winit, AppKit through `objc2-app-kit`, the Windows registry through
`windows-registry`, and the XDG desktop portal through `zbus`. RXUI stays free of
`unsafe` code outside the existing Windows menu backend.

## Following the system appearance

`Application::new()` now follows the system by default. Its theme mode is
`ThemeMode::System`, which chooses between the themes of a `ThemeSet`:

```rust
let app = Application::new()
    // The default, shown for clarity. Light and Dark fix the scheme instead.
    .theme_mode(ThemeMode::System)
    // Optional: customize the themes a mode chooses between.
    .themes(ThemeSet::default().map(Theme::compact));
```

`ThemeSet::resolve(mode, &preferences)` is the whole policy and is public, so
custom hosts and tests use the same rule:

- Light and Dark always return their theme unchanged.
- System uses the reported light/dark appearance (dark when unknown).
- System switches to `ThemeSet::high_contrast` while the user asks for more
  contrast (macOS "Increase contrast", Windows high-contrast themes, the portal
  `contrast` setting).
- Otherwise System applies the system accent color when `follow_accent` is set
  (the default). The accent drives `accent`, `accent_hover`, `accent_pressed`,
  `accent_text`, `focus`, `selection`, `row_selected` and
  `row_selected_inactive`; see [styling](styling.md#following-the-system-accent).
  Accent text stays the theme's (white in the presets) for every macOS accent
  except yellow, which takes black.

An explicit theme wins. `Application::theme(theme)` and `cx.set_theme(theme)`
switch the application to that theme and stop following the system;
`cx.set_theme_mode(mode)` resumes. `WindowOptions::theme` and `cx.set_window_theme`
still override one window, and `.theme(...)` on an element still overrides a
subtree. `cx.theme_mode()` returns `None` while an explicit theme is active, and
`cx.set_theme_set(themes)` replaces the themes at run time.

The native title bar follows too. A window that inherits a System-mode theme
leaves its title bar to the OS; every other window sets the native appearance to
the scheme of its theme (`ColorScheme::of(&theme)`), so an explicitly dark
application gets a dark title bar on a light system. Windows created with an
explicit `native_attributes(...).with_theme(...)` keep that choice.

## System preferences

`SystemPreferences` is a plain snapshot:

| Field | Meaning |
| --- | --- |
| `color_scheme` | `Some(Light/Dark)` when reported |
| `high_contrast` | The user asked for increased contrast |
| `reduce_motion` | The user asked for less motion |
| `reduce_transparency` | The user asked for less transparency or blur |
| `accent_color` | Linear RGBA accent, when exposed |

Read it with `cx.system_preferences()`, for example during initialization, and
react to changes with a hook:

```rust
Application::new().system_preferences_changed(|preferences, cx| {
    // Runs after mode themes have updated, outside entity leases, with no source window.
    settings.update(cx, |s, _| s.reduce_motion = preferences.reduce_motion);
});
```

The hook receives only changed snapshots, after the first detection. Custom hosts
call `SystemPreferences::detect(event_loop)` and `ThemeSet::resolve` themselves.

**Reduce motion.** The host applies `reduce_motion` to the runtime, so built-in
transitions, overlay motion and `cx.animate` values complete immediately while it
is set; see [animation](animation.md#reduce-motion). An application can override it
with `AppContext::set_reduce_motion`. Views that drive their own frames with
`cx.request_animation_frame()` should check `cx.reduce_motion()`.

### When values refresh

| Platform | Light/dark | High contrast | Reduce motion | Reduce transparency | Accent |
| --- | --- | --- | --- | --- | --- |
| macOS | winit, live | AppKit | AppKit | AppKit | `NSColor.controlAccentColor` |
| Windows | winit, live | registry `HighContrast\Flags` | registry `MinAnimate`, best effort | registry `EnableTransparency` | registry `DWM\AccentColor` |
| Linux/BSD | XDG portal, live | portal `contrast`, live | portal `reduced-motion` or GNOME `enable-animations`, live | not reported | portal `accent-color`, live |

On macOS and Windows the host reads every value at startup, when winit reports
`ThemeChanged`, and whenever a managed window becomes active. Accessibility and
accent changes are made in System Settings, so the next activation of an RXUI
window picks them up; there is no polling. A window whose title bar was forced to
an explicit scheme receives no `ThemeChanged`, which the activation refresh covers.
Windows has no registry value that exactly mirrors "Show animations"; RXUI reads
the minimize-animation setting that the same switch turns off, so treat it as a
hint. On Linux and the BSDs a background thread listens to the portal's
`SettingChanged` signal and wakes the event loop through its proxy. Without a
session bus or portal (some minimal X11 sessions), the values keep their
defaults and System mode is dark. The initial portal read waits at most 300 ms.

## Cursors

`Cursor` covers the standard set, mapped to winit cursor icons:

| Cursor | Use |
| --- | --- |
| `Default`, `Pointer`, `Text`, `VerticalText`, `Crosshair`, `Cell` | Ordinary pointing and selection |
| `Grab`, `Grabbing`, `Move` | Dragging |
| `ResizeNorthSouth`, `ResizeEastWest` | Bidirectional edges |
| `ResizeNorthEastSouthWest`, `ResizeNorthWestSouthEast` | Diagonal corners |
| `ResizeColumn`, `ResizeRow` (aliases `ResizeHorizontal`, `ResizeVertical`) | Splitters and table boundaries |
| `Wait`, `Progress` | Busy states |
| `Help`, `ContextMenu`, `ZoomIn`, `ZoomOut` | Hints |
| `Copy`, `Alias`, `NoDrop`, `NotAllowed` | Drag-and-drop feedback |
| `Hidden` | Hides the pointer over the element |

`Hidden` calls winit's `set_cursor_visible(false)`, and the next visible cursor
shows the pointer again. Platforms draw their own variants; where one has no
exact cursor (for example `ContextMenu` on Windows), winit substitutes the closest.

## Platform conventions

**Focus rings.** Focus rings follow CSS `:focus-visible`. A pointer press focuses
a button without a ring; Tab, arrow keys or typing show it; text inputs always
show it. Shortcuts with Control, Command or Alt, and bare modifier keys, do not
change it. `Application::focus_ring(FocusRingPolicy::Always)` or
`Ui::set_focus_ring_policy` keep rings always on. `ElementInfo::focus_visible`
tells custom painters whether to draw the ring; `ElementInfo::focused` still
reports logical focus.

**Primary modifier.** `Shortcut::primary` and `Modifiers::primary` already mean
Command on macOS and Control elsewhere. Standard editing shortcuts, native menu
accelerators and text-editing word/line movement use the same rule, so a shortcut
declared once works on every platform.

**Dialog button order.** Native message dialogs order their own buttons. For
RXUI-drawn dialogs, `DialogButtonOrder::platform().arrange([(role, button), ...])`
puts the affirmative button last on macOS and Linux (GNOME) and first on Windows,
with Cancel next to it on macOS. `DialogButtonOrder::AffirmativeFirst` suits KDE.

**Scrollbars.** `scroll_area(...).scrollbar_style(ScrollbarStyle::Overlay)` draws
8-unit bars over the content's trailing edge on a transparent track without
reserving space, like macOS. `Classic` keeps the 12-unit gutters. Scroll areas
default to `Classic` on every platform so layouts match everywhere;
`ScrollbarStyle::platform()` returns Overlay on macOS. Overlay bars stay visible;
fading them while idle needs the animation work.

**Smooth scrolling.** Scroll containers default to `ScrollBehavior::platform()`:
mouse-wheel steps and programmatic scrolls ease over 150 ms on macOS and Windows,
as their system scroll views and browsers do, and jump on Linux, as GTK and Qt
scroll views do. Trackpad deltas always scroll directly, and reduce motion turns
the animation off. Unlike scrollbar style, behavior does not change layout, so it
follows the platform by default. See [animation](animation.md#smooth-scrolling).

**Interface font.** With system font discovery on (the default), the native host
substitutes the platform interface font for the generic sans-serif family: SF Pro
on macOS (the hidden `.SF NS` face), Segoe UI Variable or Segoe UI on Windows, and
Cantarell, Ubuntu, Noto Sans and similar on Linux, in the order of
`system_ui_font_families()`. Named families and application fonts are untouched.
`Application::system_ui_font(false)` keeps the generic family, and custom hosts
call `UiPainter::set_sans_serif_family`.

## Window chrome

`WindowOptions` exposes the winit window attributes most applications need:

```rust
WindowOptions::new()
    .min_size(520., 360.)          // and .max_size(w, h); validated
    .resizable(true)
    .decorations(true)
    .always_on_top(false)
    .transparent(false)
    .custom_title_bar()            // see below
```

macOS adds `titlebar_transparent`, `fullsize_content_view` and `title_hidden`.
They are accepted, and ignored, on other platforms, so portable code needs no
`cfg`. `native_attributes` still gives full access to winit.

**Custom title bars.** `custom_title_bar()` keeps the macOS traffic lights over a
transparent, untitled title bar with the content underneath (leave about 78
logical units at the leading edge and 28 at the top), and removes decorations on
Windows and Linux, where the application draws its own minimize, maximize and
close buttons. Mark the bar with `.window_drag_region()`:

```rust
row().window_drag_region().height(38.)
    .child(label("My App"))
    .child(button("✕").on_click(cx.listener(|_, _, cx| {
        if let Some(window) = cx.window() { cx.request_close(&window).unwrap(); }
    })))
```

A primary press on the region, outside any interactive descendant, starts a
native window move (`Window::drag_window`), and a double press toggles
maximization. Buttons, inputs and other pointer targets inside the bar keep their
input. The press does not reach the UI, so focus and capture stay unchanged, as
with a native title bar. `Ui::window_drag_region_at(point)` exposes the hit test;
custom hosts get the behavior from `WindowInput`, whose `sync_window` performs the
move within the same event callback. macOS's "double-click a title bar to
minimize" preference is not read; double presses always maximize.

**Transparency.** `.transparent(true)` makes the native window transparent, makes
the default clear color transparent, and asks the surface for a premultiplied,
then postmultiplied, alpha mode. Backends without one (DirectX 12 window surfaces
on Windows, some GL drivers) fall back to an opaque surface, so the window is
then opaque. Paint opaque backgrounds where the content needs them.

## Testing

Headless tests cover theme resolution and accent contrast for the eight macOS
accents, white and black in both schemes (`appearance`), preference delivery through an injected source including
the change hook, explicit-theme precedence and title-bar scheme selection
(`native`), cursor mapping, window option validation, interface-font selection,
dialog ordering (`conventions`), and focus-visible modality, drag-region routing
and overlay scrollbar geometry (`platform_tests`). An ignored test checks that a
platform interface font is installed:

```sh
cargo test -p rxui --all-features
cargo test -p rxui --all-features a_platform_interface_font -- --ignored --nocapture
cargo run -p rxui --example native_feel_window --features native
```

The standalone [native feel example](../crates/rxui/examples/native_feel_window.rs)
shows the detected preferences, mode switching, a custom title bar with a drag
region, the cursor set, platform dialog order and overlay scrollbars on macOS.

## Validation

On macOS the interface-font check found `.SF NS`, and a headless render confirmed
that regular, semibold and bold weights shape from it. The example launched and
ran without errors. Screen capture was unavailable in the sandbox, so the custom
title bar, drag region, transparency and live appearance switching were not
inspected visually. Windows and Linux code paths compile only where those targets
are installed; neither was run here.

## Follow-ups

- Window vibrancy (macOS `NSVisualEffectView`) needs AppKit view manipulation that
  winit does not expose; a safe implementation needs an objc2 view-hierarchy
  binding. Windows Mica/Acrylic is reachable through winit's
  `WindowAttributesExtWindows::with_system_backdrop` via `native_attributes`, but
  wgpu's DirectX 12 window surfaces are opaque, so the backdrop stays hidden under
  the UI until surfaces support composition.
- Overlay scrollbars that fade while idle.
- Reading the macOS double-click title-bar action and the Windows "Show animations"
  value exactly.
- Live notification of macOS accent/contrast changes while the app is active
  (`NSSystemColorsDidChangeNotification`), instead of on activation.
- See the [platform integration roadmap](native-services.md#platform-integration-roadmap)
  for native menus on Linux, tray icons, notifications and single-instance apps.

Window-level hidden cursors and pointer grabs take precedence over hovered cursor
icons. Use `WindowHandle::set_cursor_hidden` or `set_pointer_lock` for a control
mode; direct winit visibility changes are not retained RXUI policy. Focus loss
releases locking. See [pointer locking](application.md#pointer-locking-and-visibility)
for failure handling and platform differences.
