//! Complete native animation example. Buttons and menus animate their state paint
//! with the theme's durations, a menu and a dialog fade in and out, the theme switch
//! crossfades, and `cx.animate` drives a drawer width and a spring-loaded meter.
//! Nothing animates, and no frames are drawn, while the window is idle.
use rxui::prelude::*;
use std::time::Duration;

struct Demo {
    anchor: AnchorHandle,
    menu: bool,
    dialog: bool,
    drawer: bool,
    level: f32,
    dark: bool,
}
impl View for Demo {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let drawer = cx.animate(
            "drawer",
            if self.drawer { 220. } else { 56. },
            Transition::Theme,
        );
        let meter = cx.animate(
            "meter",
            self.level,
            Transition::spring(Spring::new(Duration::from_millis(450), 0.55)),
        );
        let reduce = cx.reduce_motion();
        let controls = column()
            .gap(12.)
            .child(label("Implicit state transitions").font_size(18.))
            .child(
                row()
                    .gap(8.)
                    .child(button("Default"))
                    .child(button("Primary").variant(ButtonVariant::Primary))
                    .child(button("Quiet").variant(ButtonVariant::Quiet))
                    .child(button("Snaps").no_transition())
                    .child(
                        button("Springy")
                            .hover_style(
                                PaintStyle::new()
                                    .background(ThemeColor::Accent)
                                    .color(ThemeColor::AccentText)
                                    .radius(18.),
                            )
                            .transition(Transition::spring(Spring::new(
                                Duration::from_millis(500),
                                0.5,
                            ))),
                    ),
            )
            .child(
                row()
                    .gap(8.)
                    .child(
                        button("Actions")
                            .anchor_handle(self.anchor.clone())
                            .on_click(cx.listener(|s, _, _| s.menu = !s.menu)),
                    )
                    .child(button("Open dialog").on_click(cx.listener(|s, _, _| s.dialog = true)))
                    .child(
                        button(if self.dark {
                            "Light theme"
                        } else {
                            "Dark theme"
                        })
                        .on_click(cx.listener(|s, _, cx| {
                            s.dark = !s.dark;
                            let theme = if s.dark {
                                Theme::dark()
                            } else {
                                Theme::light()
                            };
                            let _ = cx.set_theme(theme);
                        })),
                    )
                    .child(
                        button(if reduce {
                            "Reduce motion: on"
                        } else {
                            "Reduce motion: off"
                        })
                        .on_click(cx.listener(|_, _, cx| {
                            let reduce = !cx.reduce_motion();
                            cx.set_reduce_motion(reduce);
                        })),
                    ),
            )
            .child(label("Animated values").font_size(18.))
            .child(
                row()
                    .gap(8.)
                    .child(
                        button(if self.drawer {
                            "Collapse drawer"
                        } else {
                            "Expand drawer"
                        })
                        .on_click(cx.listener(|s, _, _| s.drawer = !s.drawer)),
                    )
                    .child(button("Fill meter").on_click(cx.listener(|s, _, _| {
                        s.level = if s.level < 0.9 { s.level + 0.3 } else { 0.1 };
                    }))),
            )
            .child(
                stack()
                    .width(320.)
                    .height(12.)
                    .radius(6.)
                    .background(ThemeColor::Control)
                    .child(
                        column()
                            .width(320. * meter.clamp(0., 1.))
                            .fill_height()
                            .radius(6.)
                            .background(ThemeColor::Accent),
                    ),
            )
            .child(
                label("Layout never animates implicitly; the drawer and meter use cx.animate.")
                    .color(ThemeColor::TextMuted),
            );
        let mut root = row()
            .fill_width()
            .fill_height()
            .background(ThemeColor::Background)
            .child(
                column()
                    .width(drawer)
                    .fill_height()
                    .padding(12.)
                    .gap(8.)
                    .clip()
                    .background(ThemeColor::Surface)
                    .child(label("Drawer")),
            )
            .child(column().padding(24.).gap(16.).child(controls));
        if self.menu {
            let close = |label: &'static str| {
                button(label)
                    .variant(ButtonVariant::Quiet)
                    .fill_width()
                    .accessibility_role(SemanticRole::MenuItem)
                    .on_click(cx.listener(|s, _, _| s.menu = false))
            };
            root = root.child(
                popover(
                    self.anchor.clone(),
                    menu()
                        .child(close("Rename"))
                        .child(close("Duplicate"))
                        .child(close("Delete")),
                )
                .key("menu")
                .width(180.)
                .accessibility_label("Actions")
                .on_dismiss(cx.listener(|s, _: &DismissEvent, _| s.menu = false)),
            );
        }
        if self.dialog {
            root = root.child(
                modal(
                    column()
                        .gap(12.)
                        .child(label("Discard changes?").font_size(18.))
                        .child(label(
                            "The dialog fades out after closing; focus returns at once.",
                        ))
                        .child(
                            row()
                                .gap(8.)
                                .child(
                                    button("Cancel")
                                        .on_click(cx.listener(|s, _, _| s.dialog = false)),
                                )
                                .child(
                                    button("Discard")
                                        .variant(ButtonVariant::Primary)
                                        .on_click(cx.listener(|s, _, _| s.dialog = false)),
                                ),
                        ),
                )
                .key("dialog")
                .width(360.)
                .padding(20.)
                .accessibility_label("Discard changes")
                .on_dismiss(cx.listener(|s, _: &DismissEvent, _| s.dialog = false)),
            );
        }
        root
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let root = cx.new(|_| Demo {
            anchor: AnchorHandle::new(),
            menu: false,
            dialog: false,
            drawer: true,
            level: 0.4,
            dark: true,
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — animation")
                .size(900., 560.),
            root,
        )?;
        Ok(())
    })
}
