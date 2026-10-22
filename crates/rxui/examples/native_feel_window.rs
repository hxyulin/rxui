//! Standalone native-feel example: system theme following, a custom title bar and cursors.
//! Run: cargo run -p rxui --example native_feel_window --features native
//! Change the system appearance, accent color, contrast or motion settings while it
//! runs; the default System mode follows them. Drag the title bar to move the window
//! and double-click it to maximize. Hover the tiles to preview the cursor set.
use rxui::{AppContext, Element, prelude::*, taffy};
use std::{cell::RefCell, error::Error, rc::Rc};

struct Showcase {
    preferences: SystemPreferences,
    mode: Option<ThemeMode>,
}
impl Showcase {
    fn title_bar(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        // macOS keeps its traffic lights over the transparent title bar; other
        // platforms lose their decorations, so the bar draws its own buttons.
        let mut bar = row()
            .key("title-bar")
            .window_drag_region()
            .fill_width()
            .height(38.)
            .padding_x(12.)
            .gap(8.)
            .background(ThemeColor::Surface)
            .border_bottom(1.)
            .border_color(ThemeColor::Divider)
            .layout(|s| s.align_items = taffy::AlignItems::CENTER);
        if cfg!(target_os = "macos") {
            bar = bar.child(row().width(66.));
        }
        bar = bar
            .child(label("Native feel").font_weight(FontWeight::SEMIBOLD))
            .child(row().flex_grow(1.));
        if !cfg!(target_os = "macos") {
            let native = |cx: &AppContext<'_>| cx.window().and_then(|w| w.native_window());
            bar = bar
                .child(
                    button("—")
                        .variant(ButtonVariant::Quiet)
                        .accessibility_label("Minimize")
                        .on_click(cx.listener(move |_, _, cx| {
                            if let Some(window) = native(cx) {
                                window.set_minimized(true);
                            }
                        })),
                )
                .child(
                    button("□")
                        .variant(ButtonVariant::Quiet)
                        .accessibility_label("Maximize")
                        .on_click(cx.listener(move |_, _, cx| {
                            if let Some(window) = native(cx) {
                                window.set_maximized(!window.is_maximized());
                            }
                        })),
                )
                .child(
                    button("✕")
                        .variant(ButtonVariant::Quiet)
                        .accessibility_label("Close")
                        .on_click(cx.listener(|_, _, cx| {
                            if let Some(window) = cx.window() {
                                cx.request_close(&window).unwrap();
                            }
                        })),
                );
        }
        bar
    }
}
impl View for Showcase {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let p = &self.preferences;
        let flag = |on: bool| if on { "on" } else { "off" };
        let facts = column()
            .gap(6.)
            .child(label(format!(
                "System appearance: {}",
                p.color_scheme
                    .map_or("unknown".into(), |s| format!("{s:?}"))
            )))
            .child(label(format!(
                "Increase contrast: {} · Reduce motion: {} · Reduce transparency: {}",
                flag(p.high_contrast),
                flag(p.reduce_motion),
                flag(p.reduce_transparency)
            )))
            .child(
                row()
                    .gap(8.)
                    .child(label(match p.accent_color {
                        Some(_) => "System accent:",
                        None => "System accent: not reported",
                    }))
                    .child(match p.accent_color {
                        Some(color) => row().size(16., 16.).radius(8.).background(color),
                        None => row(),
                    }),
            );
        let mode_button = |caption: &'static str, mode: ThemeMode| {
            let selected = self.mode == Some(mode);
            button(caption)
                .key(caption)
                .variant(if selected {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Default
                })
                .on_click(cx.listener(move |this, _, cx| {
                    cx.set_theme_mode(mode).unwrap();
                    this.mode = Some(mode);
                }))
        };
        let modes = row()
            .gap(8.)
            .child(mode_button("System", ThemeMode::System))
            .child(mode_button("Light", ThemeMode::Light))
            .child(mode_button("Dark", ThemeMode::Dark));
        let cursors = [
            ("Pointer", Cursor::Pointer),
            ("Text", Cursor::Text),
            ("Move", Cursor::Move),
            ("Grab", Cursor::Grab),
            ("Resize ↕", Cursor::ResizeNorthSouth),
            ("Resize ↔", Cursor::ResizeEastWest),
            ("Resize ⤡", Cursor::ResizeNorthWestSouthEast),
            ("Resize ⤢", Cursor::ResizeNorthEastSouthWest),
            ("Column", Cursor::ResizeColumn),
            ("Row", Cursor::ResizeRow),
            ("Wait", Cursor::Wait),
            ("Progress", Cursor::Progress),
            ("Help", Cursor::Help),
            ("Zoom in", Cursor::ZoomIn),
            ("Zoom out", Cursor::ZoomOut),
            ("Context menu", Cursor::ContextMenu),
            ("Copy", Cursor::Copy),
            ("Alias", Cursor::Alias),
            ("No drop", Cursor::NoDrop),
            ("Not allowed", Cursor::NotAllowed),
            ("Crosshair", Cursor::Crosshair),
            ("Cell", Cursor::Cell),
            ("Hidden", Cursor::Hidden),
        ];
        let tiles = row()
            .gap(8.)
            .fill_width()
            .layout(|s| s.flex_wrap = taffy::FlexWrap::Wrap)
            .children(cursors.into_iter().map(|(name, cursor)| {
                column()
                    .key(name)
                    .cursor(cursor)
                    .width(112.)
                    .padding(10.)
                    .radius(6.)
                    .background(ThemeColor::Control)
                    .border(1., ThemeColor::Border)
                    .child(label(name).color(ThemeColor::TextMuted))
            }));
        // Affirmative last on macOS and GNOME, first on Windows.
        let actions = DialogButtonOrder::platform().arrange([
            (DialogButtonRole::Cancel, button("Cancel").key("cancel")),
            (
                DialogButtonRole::Alternative,
                button("Don't Save").key("discard"),
            ),
            (
                DialogButtonRole::Affirmative,
                button("Save").key("save").variant(ButtonVariant::Primary),
            ),
        ]);
        let body = column()
            .padding(24.)
            .gap(16.)
            .fill_width()
            .child(label("Follows your system").font_size(22.))
            .child(facts)
            .child(modes)
            .child(label("Cursors").font_size(18.))
            .child(tiles)
            .child(label("Dialog button order for this platform").font_size(18.))
            .child(row().gap(8.).children(actions))
            .child(label(
                "Tab moves focus with a visible ring; clicking a button hides it again.",
            ));
        column()
            .fill_width()
            .fill_height()
            .child(self.title_bar(cx))
            .child(scroll_area(body).scrollbar_style(ScrollbarStyle::platform()))
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    let view: Rc<RefCell<Option<rxui::WeakEntity<Showcase>>>> = Rc::default();
    let changes = view.clone();
    Application::new()
        // System mode is the default; the hook only refreshes the displayed facts.
        .system_preferences_changed(move |preferences, cx| {
            if let Some(view) = changes.borrow().as_ref().and_then(|weak| weak.upgrade()) {
                view.update(cx, |this, _| this.preferences = preferences.clone());
            }
        })
        .run(|cx| {
            let root = cx.new(|_| Showcase {
                preferences: SystemPreferences::default(),
                mode: Some(ThemeMode::System),
            });
            let preferences = cx.system_preferences()?;
            let mode = cx.theme_mode()?;
            root.update(cx, |this, _| {
                this.preferences = preferences;
                this.mode = mode;
            });
            *view.borrow_mut() = Some(root.downgrade());
            cx.open_window(
                WindowOptions::new()
                    .title("RXUI native feel")
                    .size(780., 620.)
                    .min_size(520., 360.)
                    .custom_title_bar(),
                root,
            )?;
            Ok(())
        })?;
    Ok(())
}
