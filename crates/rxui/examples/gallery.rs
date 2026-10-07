//! Every built-in component on one page, with live theme and density switching.
use rxui::Element;
use rxui::prelude::*;

const ROWS: usize = 1_000;
const ROW_HEIGHT: f32 = 32.;

struct NewWindow;
impl Command for NewWindow {}
struct OpenFile;
impl Command for OpenFile {}
struct SaveFile;
impl Command for SaveFile {}
struct CloseWindow;
impl Command for CloseWindow {}

#[derive(Clone, Copy, PartialEq)]
enum Palette {
    Dark,
    Light,
    HighContrast,
}
impl Palette {
    const ALL: [(Self, &'static str); 3] = [
        (Self::Dark, "Dark"),
        (Self::Light, "Light"),
        (Self::HighContrast, "High contrast"),
    ];
}

struct Gallery {
    palette: Palette,
    compact: bool,
    name: String,
    filter: String,
    tab: Key,
    tabs_open: Vec<&'static str>,
    menu_anchor: AnchorHandle,
    menu_open: bool,
    list: ScrollHandle,
    selected: usize,
    split: SplitPosition,
    dialog: bool,
    status: String,
}
impl Gallery {
    fn theme(&self) -> Theme {
        let theme = match self.palette {
            Palette::Dark => Theme::dark(),
            Palette::Light => Theme::light(),
            Palette::HighContrast => Theme::high_contrast(),
        };
        if self.compact { theme.compact() } else { theme }
    }
}

/// A muted heading over a group of related controls.
fn section(title: &str, body: impl IntoElement) -> Element {
    column()
        .gap(8.)
        .fill_width()
        .child(label(title).color(ThemeColor::TextMuted))
        .child(body)
}

/// Bordered panel used for content areas inside the sections.
fn panel(body: impl IntoElement) -> Element {
    column()
        .fill_width()
        .fill_height()
        .padding(12.)
        .gap(6.)
        .background(ThemeColor::Surface)
        .border(1., ThemeColor::Divider)
        .radius(6.)
        .child(body)
}

impl View for Gallery {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        // Toolbar: segmented theme and density pickers.
        let mut themes = row().gap(4.);
        for (palette, name) in Palette::ALL {
            themes = themes.child(
                button(name)
                    .variant(if self.palette == palette {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Quiet
                    })
                    .on_click(cx.listener(move |s, _, cx| {
                        s.palette = palette;
                        cx.set_theme(s.theme()).unwrap();
                    })),
            );
        }
        let mut densities = row().gap(4.);
        for (compact, name) in [(false, "Balanced"), (true, "Compact")] {
            densities = densities.child(
                button(name)
                    .variant(if self.compact == compact {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Quiet
                    })
                    .on_click(cx.listener(move |s, _, cx| {
                        s.compact = compact;
                        cx.set_theme(s.theme()).unwrap();
                    })),
            );
        }
        let toolbar = row()
            .fill_width()
            .gap(24.)
            .padding(12.)
            .background(ThemeColor::Surface)
            .border(1., ThemeColor::Divider)
            .child(themes)
            .child(densities);

        // Commands shared by the menu items and keyboard shortcuts.
        let new = cx
            .command(NewWindow, |s, _, _| {
                s.status = "New window".into();
                s.menu_open = false;
            })
            .label("New window")
            .shortcut(Shortcut::primary("n"));
        let open = cx
            .command(OpenFile, |s, _, _| {
                s.status = "Open…".into();
                s.menu_open = false;
            })
            .label("Open…")
            .shortcut(Shortcut::primary("o"));
        let save = cx
            .command(SaveFile, |s, _, _| {
                s.status = "Save".into();
                s.menu_open = false;
            })
            .label("Save")
            .enabled(false)
            .shortcut(Shortcut::primary("s"));
        let close = cx
            .command(CloseWindow, |s, _, _| {
                s.status = "Close window".into();
                s.menu_open = false;
            })
            .label("Close window")
            .shortcut(Shortcut::primary("w"));

        let buttons = section(
            "Buttons",
            row()
                .gap(8.)
                .child(
                    button("Save changes")
                        .variant(ButtonVariant::Primary)
                        .on_click(cx.listener(|s, _, _| s.status = "Saved".into())),
                )
                .child(
                    button("Cancel").on_click(cx.listener(|s, _, _| s.status = "Cancelled".into())),
                )
                .child(button("Reset").variant(ButtonVariant::Quiet))
                .child(button("Export").disabled(true)),
        );

        let inputs = section(
            "Text input",
            column()
                .gap(8.)
                .fill_width()
                .child(
                    text_input(self.name.clone())
                        .fill_width()
                        .accessibility_label("Project name")
                        .on_change(
                            cx.listener(|s, e: &TextChangeEvent, _| s.name = e.value.clone()),
                        ),
                )
                .child(
                    row()
                        .gap(8.)
                        .fill_width()
                        .child(
                            text_input(self.filter.clone())
                                .flex_grow(1.)
                                .min_width(0.)
                                .accessibility_label("Filter files")
                                .on_change(cx.listener(|s, e: &TextChangeEvent, _| {
                                    s.filter = e.value.clone()
                                })),
                        )
                        .child(button("Filter")),
                )
                .child(
                    row()
                        .gap(8.)
                        .fill_width()
                        .child(
                            text_input("0.2.0-dev.0")
                                .flex_grow(1.)
                                .min_width(0.)
                                .read_only(true)
                                .accessibility_label("Version"),
                        )
                        .child(
                            text_input("Disabled")
                                .flex_grow(1.)
                                .min_width(0.)
                                .disabled(true)
                                .accessibility_label("Token"),
                        ),
                ),
        );

        let mut tab_set = tabs()
            .key("tabs")
            .selected(self.tab.clone())
            .height(170.)
            .fill_width()
            .on_select(cx.listener(|s, e: &TabSelectEvent, _| s.tab = e.key.clone()))
            .on_close(cx.listener(|s, e: &TabCloseEvent, _| {
                s.tabs_open.retain(|t| Key::from(*t) != e.key);
                if let Some(next) = &e.next_selection {
                    s.tab = next.clone();
                }
            }));
        for name in &self.tabs_open {
            let body = panel(
                column()
                    .gap(6.)
                    .child(label(format!("{name} panel")))
                    .child(label("Arrow keys move between tabs.").color(ThemeColor::TextMuted)),
            );
            tab_set = tab_set.tab(tab(*name, *name, body).closable(*name == "Logs"));
        }
        tab_set = tab_set.tab(tab("Profiler", "Profiler", label("")).disabled(true));
        let tabs_section = section("Tabs", tab_set);

        let mut menu_row = row().gap(8.).child(
            button("File ▾")
                .anchor_handle(self.menu_anchor.clone())
                .on_click(cx.listener(|s, _, _| s.menu_open = !s.menu_open)),
        );
        if self.menu_open {
            menu_row = menu_row.child(
                popover(
                    self.menu_anchor.clone(),
                    menu()
                        .child(menu_item(&new))
                        .child(menu_item(&open))
                        .child(menu_item(&save))
                        .child(menu_item(&close)),
                )
                .key("file-menu")
                .width(240.)
                .on_dismiss(cx.listener(|s, _: &DismissEvent, _| s.menu_open = false)),
            );
        }
        let menu_section = section("Menu", menu_row);

        let list = section(
            "Virtual list",
            column()
                .fill_width()
                .height(220.)
                .border(1., ThemeColor::Divider)
                .radius(6.)
                .clip()
                .child(
                    virtual_list(ROWS, ROW_HEIGHT, &self.list, cx, |index| {
                        button(format!("file_{index:04}.rs"))
                            .key(index)
                            .fill_width()
                            .radius(0.)
                            .variant(if self.selected == index {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Quiet
                            })
                            .on_click(cx.listener(move |s, _, _| s.selected = index))
                    })
                    .fill_width()
                    .fill_height(),
                ),
        );

        let split = section(
            "Split",
            column().fill_width().height(140.).child(
                split_row(
                    panel(label("Drag the divider")),
                    panel(label("Second pane").color(ThemeColor::TextMuted)),
                )
                .position(self.split)
                .min_first(80.)
                .min_second(80.)
                .on_resize(cx.listener(|s, e: &ResizeEvent, _| s.split = e.position)),
            ),
        );

        let dialog = section(
            "Dialog",
            row()
                .child(button("Discard changes…").on_click(cx.listener(|s, _, _| s.dialog = true))),
        );

        let columns = row()
            .fill_width()
            .gap(32.)
            .child(
                column()
                    .flex_grow(1.)
                    .flex_basis(0.)
                    .min_width(0.)
                    .gap(28.)
                    .child(buttons)
                    .child(inputs)
                    .child(menu_section)
                    .child(dialog),
            )
            .child(
                column()
                    .flex_grow(1.)
                    .flex_basis(0.)
                    .min_width(0.)
                    .gap(28.)
                    .child(tabs_section)
                    .child(list)
                    .child(split),
            );

        let mut root = column()
            .fill_width()
            .fill_height()
            .on_command(new)
            .on_command(open)
            .on_command(save)
            .on_command(close)
            .child(toolbar)
            .child(
                column()
                    .fill_width()
                    .flex_grow(1.)
                    .min_height(0.)
                    .padding(28.)
                    .gap(16.)
                    .scroll_y()
                    .child(label("Component gallery").font_size(22.))
                    .child(label(self.status.clone()).color(ThemeColor::TextMuted))
                    .child(columns),
            );
        if self.dialog {
            root = root.child(
                modal(
                    column()
                        .gap(12.)
                        .child(label("Discard unsaved changes?").font_size(18.))
                        .child(
                            label("theme.rs has edits that have not been saved.")
                                .color(ThemeColor::TextMuted),
                        )
                        .child(
                            row()
                                .gap(8.)
                                .child(
                                    button("Cancel")
                                        .on_click(cx.listener(|s, _, _| s.dialog = false)),
                                )
                                .child(button("Discard").variant(ButtonVariant::Primary).on_click(
                                    cx.listener(|s, _, _| {
                                        s.dialog = false;
                                        s.status = "Changes discarded".into();
                                    }),
                                )),
                        ),
                )
                .key("discard-dialog")
                .width(420.)
                .padding(20.)
                .accessibility_label("Discard unsaved changes")
                .on_dismiss(cx.listener(|s, _: &DismissEvent, _| s.dialog = false)),
            );
        }
        root
    }
}

fn main() -> Result<(), ApplicationError> {
    Application::new().theme(Theme::dark()).run(|cx| {
        let root = cx.new(|_| Gallery {
            palette: Palette::Dark,
            compact: false,
            name: "rxui".into(),
            filter: "theme".into(),
            tab: Key::from("Overview"),
            tabs_open: vec!["Overview", "Layout", "Logs"],
            menu_anchor: AnchorHandle::new(),
            menu_open: false,
            list: ScrollHandle::new(),
            selected: 2,
            split: SplitPosition::default(),
            dialog: false,
            status: "Click anything; Primary+N/O/W run the menu commands.".into(),
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — component gallery")
                .size(1100., 860.),
            root,
        )?;
        Ok(())
    })
}
