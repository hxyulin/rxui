//! Every built-in component on one page, with live theme and density switching.
use rxui::Element;
use rxui::prelude::*;
use rxui::taffy::{AlignItems, FlexWrap};

const ROWS: usize = 1_000;

/// About 2,000 workspace-like files for the quick-open combobox.
fn workspace_files() -> Vec<(String, String)> {
    let dirs = [
        "crates/rxui/src",
        "crates/rxui/src/standard",
        "crates/rxui/examples",
        "crates/rxui-draw/src",
        "crates/rxui-host/src",
        "docs",
        "tests/fixtures",
        "tools/icons",
    ];
    let stems = [
        "theme",
        "layout",
        "editor",
        "buffer",
        "cursor",
        "selection",
        "tree",
        "list_view",
        "combobox",
        "matcher",
        "scroll",
        "focus",
        "overlay",
        "tabs",
        "dock",
        "commands",
        "icons",
        "text",
        "input",
        "paint",
        "shaper",
        "glyph",
        "image",
        "window",
        "menu",
    ];
    let suffixes = ["", "_tests", "_view", "_state", "_dispatch"];
    let mut files = Vec::new();
    for dir in dirs {
        let ext = if dir == "docs" { "md" } else { "rs" };
        for stem in stems {
            for suffix in suffixes {
                for n in ["", "_2"] {
                    files.push((format!("{stem}{suffix}{n}.{ext}"), dir.to_string()));
                }
            }
        }
    }
    files
}

struct NewWindow;
impl Command for NewWindow {}
struct OpenFile;
impl Command for OpenFile {}
struct SaveFile;
impl Command for SaveFile {}
struct CloseWindow;
impl Command for CloseWindow {}
struct DeleteFile;
impl Command for DeleteFile {}

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
    fn from_key(key: &Key) -> Self {
        Self::ALL
            .into_iter()
            .find(|(_, name)| Key::from(*name) == *key)
            .map_or(Self::Dark, |(palette, _)| palette)
    }
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
    query: String,
    files: Vec<(String, String)>,
    rows: ListSelection,
    split: SplitPosition,
    dialog: bool,
    status: String,
    email: bool,
    push: bool,
    wrap: bool,
    sync: bool,
    endings: Key,
    zoom: f32,
    language: Key,
    layout: Key,
    diff: Key,
    tab_width: f64,
    font: String,
    indent: Key,
    trim: bool,
    final_newline: bool,
    callout: bool,
    toasts: Vec<Toast>,
    next_toast: u64,
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
        .gap(8.)
        .background(ThemeColor::Surface)
        .border(1., ThemeColor::Divider)
        .radius(6.)
        .child(body)
}

impl View for Gallery {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        // Window toolbar: theme and density pickers.
        let palette = Palette::ALL
            .into_iter()
            .find(|(palette, _)| *palette == self.palette)
            .map_or("Dark", |(_, name)| name);
        let themes = segmented()
            .equal_width(false)
            .selected(palette)
            .accessibility_label("Theme")
            .options(Palette::ALL.map(|(_, name)| choice(name, name)))
            .on_change(cx.listener(|s, e: &ChoiceEvent, cx| {
                s.palette = Palette::from_key(&e.key);
                cx.set_theme(s.theme()).unwrap();
            }));
        let densities = segmented()
            .selected(if self.compact { "compact" } else { "balanced" })
            .accessibility_label("Density")
            .option(choice("balanced", "Balanced"))
            .option(choice("compact", "Compact"))
            .on_change(cx.listener(|s, e: &ChoiceEvent, cx| {
                s.compact = e.key == Key::from("compact");
                cx.set_theme(s.theme()).unwrap();
            }));
        let window_toolbar = toolbar()
            .accessibility_label("Appearance")
            .child(themes)
            .child(separator().margin_x(8.))
            .child(densities);
        let top = row()
            .fill_width()
            .padding(12.)
            .background(ThemeColor::Surface)
            .border_bottom(1.)
            .border_color(ThemeColor::Divider)
            .child(window_toolbar);

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
        let delete = cx
            .command(DeleteFile, |s, _, _| {
                s.status = "Delete file".into();
                s.menu_open = false;
            })
            .label("Delete file");

        // Icon buttons need an accessible name; their tooltip shows it too.
        let mut icon_buttons = row().gap(4.).align_items(AlignItems::CENTER);
        for (icon, name) in [
            (Icon::SEARCH, "Search"),
            (Icon::REFRESH_CW, "Refresh"),
            (Icon::SETTINGS, "Settings"),
            (Icon::ELLIPSIS, "More actions"),
        ] {
            icon_buttons = icon_buttons.child(tooltip(
                cx,
                icon_button(icon)
                    .key(name)
                    .accessibility_label(name)
                    .on_click(cx.listener(move |s, _, _| s.status = name.into())),
                name,
            ));
        }
        let buttons = section(
            "Buttons",
            column()
                .gap(8.)
                .child(
                    row()
                        .gap(8.)
                        .child(
                            button("Save changes")
                                .leading_icon(Icon::SAVE)
                                .variant(ButtonVariant::Primary)
                                .on_click(cx.listener(|s, _, _| s.status = "Saved".into())),
                        )
                        .child(
                            button("Cancel")
                                .on_click(cx.listener(|s, _, _| s.status = "Cancelled".into())),
                        )
                        .child(button("Reset").variant(ButtonVariant::Quiet))
                        .child(
                            button("Delete")
                                .leading_icon(Icon::TRASH)
                                .variant(ButtonVariant::Danger),
                        )
                        .child(button("Export").leading_icon(Icon::DOWNLOAD).disabled(true)),
                )
                .child(icon_buttons),
        );

        // One Tab stop: arrow keys move between the items, skipping separators.
        let tools = section(
            "Toolbar",
            column()
                .gap(12.)
                .child(
                    toolbar()
                        .accessibility_label("Editor")
                        .child(
                            icon_button(Icon::PLUS)
                                .accessibility_label("New file")
                                .on_click(cx.listener(|s, _, _| s.status = "New file".into())),
                        )
                        .child(
                            icon_button(Icon::SEARCH)
                                .accessibility_label("Search")
                                .on_click(cx.listener(|s, _, _| s.status = "Search".into())),
                        )
                        .child(separator())
                        .child(
                            toggle_button(self.wrap, Icon::TEXT_WRAP)
                                .accessibility_label("Wrap lines")
                                .on_change(cx.listener(|s, e: &ToggleEvent, _| s.wrap = e.checked)),
                        )
                        .child(
                            segmented()
                                .selected(self.layout.clone())
                                .accessibility_label("Layout")
                                .option(choice("list", "List").icon(Icon::LIST))
                                .option(choice("grid", "Grid").icon(Icon::LAYOUT_GRID))
                                .on_change(
                                    cx.listener(|s, e: &ChoiceEvent, _| s.layout = e.key.clone()),
                                ),
                        )
                        .child(separator())
                        .child(
                            button("Run")
                                .leading_icon(Icon::PLAY)
                                .variant(ButtonVariant::Primary)
                                .on_click(cx.listener(|s, _, _| s.status = "Running".into())),
                        ),
                )
                .child(
                    row().child(
                        segmented()
                            .selected(self.diff.clone())
                            .accessibility_label("Diff view")
                            .option(choice("split", "Split"))
                            .option(choice("unified", "Unified"))
                            .option(choice("raw", "Raw"))
                            .on_change(cx.listener(|s, e: &ChoiceEvent, _| s.diff = e.key.clone())),
                    ),
                ),
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
                        .child(button("Filter").leading_icon(Icon::FUNNEL)),
                )
                .child(
                    row()
                        .gap(8.)
                        .fill_width()
                        .child(
                            text_input(env!("CARGO_PKG_VERSION"))
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
                    .gap(8.)
                    .child(label(format!("{name} panel")))
                    .child(label("Arrow keys move between tabs.").color(ThemeColor::TextMuted)),
            );
            tab_set = tab_set.tab(tab(*name, *name, body).closable(*name == "Logs"));
        }
        tab_set = tab_set.tab(tab("Profiler", "Profiler", label("")).disabled(true));
        let tabs_section = section("Tabs", tab_set);

        let mut menu_row = row().gap(8.).child(
            button("File")
                .trailing_icon(Icon::CHEVRON_DOWN)
                .anchor_handle(self.menu_anchor.clone())
                .on_click(cx.listener(|s, _, _| s.menu_open = !s.menu_open)),
        );
        if self.menu_open {
            menu_row = menu_row.child(
                popover(
                    self.menu_anchor.clone(),
                    menu()
                        .child(menu_item(&new).icon(Icon::PLUS))
                        .child(menu_item(&open).icon(Icon::FOLDER_OPEN))
                        .child(menu_item(&save).icon(Icon::SAVE))
                        .child(menu_separator())
                        .child(menu_item(&close))
                        .child(
                            menu_item(&delete)
                                .icon(Icon::TRASH)
                                .variant(ButtonVariant::Danger),
                        ),
                )
                .key("file-menu")
                .width(240.)
                .on_dismiss(cx.listener(|s, _: &DismissEvent, _| s.menu_open = false)),
            );
        }
        let menu_section = section("Menu", menu_row);

        let quick_open = section(
            "Combobox",
            combobox(cx)
                .fill_width()
                .list_width(480.)
                .accessibility_label("Go to file")
                .placeholder("Go to file")
                .query(&self.query)
                .on_query(cx.listener(|s, e: &QueryEvent, _| s.query = e.text.clone()))
                .items(self.files.len(), |i| {
                    let (name, dir) = &self.files[i];
                    ComboItem::new(name).icon(Icon::FILE).detail(dir)
                })
                .matcher(FuzzyMatcher)
                .on_choose(cx.listener(|s, e: &ChooseEvent, _| {
                    let (name, dir) = &s.files[e.index];
                    s.status = format!("Opened {dir}/{name}");
                    s.query.clear();
                })),
        );

        let list = section(
            "List view",
            column()
                .fill_width()
                .height(220.)
                .border(1., ThemeColor::Divider)
                .radius(6.)
                .clip()
                .child(
                    list_view(cx)
                        .fill_width()
                        .fill_height()
                        .accessibility_label("Files")
                        .selection(&self.rows)
                        .items(ROWS, |i| {
                            ListItem::new(i, format!("file_{i:04}.rs")).icon(Icon::FILE)
                        })
                        .on_change(cx.listener(|s, e: &ListSelectionEvent, _| {
                            s.rows = e.selection.clone();
                        })),
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

        // Every built-in icon, named by its tooltip.
        let mut grid = row()
            .gap(4.)
            .fill_width()
            .layout(|s| s.flex_wrap = FlexWrap::Wrap);
        for icon in Icon::built_in() {
            let name = icon.name().unwrap_or_default();
            grid = grid.child(tooltip(
                cx,
                column()
                    .key(name)
                    .size(28., 28.)
                    .align_items(AlignItems::CENTER)
                    .justify_content(rxui::taffy::JustifyContent::CENTER)
                    .child(rxui::icon(icon).accessibility_label(name)),
                name,
            ));
        }
        let icons = section("Icons", grid);

        // Form controls: every value lives in Gallery and changes through a listener.
        let notifications = match (self.email, self.push) {
            (true, true) => Checked::True,
            (false, false) => Checked::False,
            _ => Checked::Mixed,
        };
        let mut status_row = row()
            .gap(12.)
            .align_items(AlignItems::CENTER)
            .child(if self.sync {
                progress_bar(None)
                    .width(160.)
                    .accessibility_label("Sync progress")
                    .animate(cx)
            } else {
                progress_bar(self.zoom / 200.)
                    .width(160.)
                    .accessibility_label("Zoom level")
            });
        if self.sync {
            status_row = status_row.child(spinner(cx).accessibility_label("Syncing"));
        }
        let controls = section(
            "Form controls",
            column()
                .gap(12.)
                .fill_width()
                .child(
                    checkbox(notifications, "All notifications").on_change(cx.listener(
                        |s, e: &ToggleEvent, _| {
                            s.email = e.checked;
                            s.push = e.checked;
                        },
                    )),
                )
                .child(
                    column()
                        .padding_x(24.)
                        .child(
                            checkbox(self.email, "Email").on_change(
                                cx.listener(|s, e: &ToggleEvent, _| s.email = e.checked),
                            ),
                        )
                        .child(
                            checkbox(self.push, "Push")
                                .on_change(cx.listener(|s, e: &ToggleEvent, _| s.push = e.checked)),
                        ),
                )
                .child(
                    row()
                        .gap(16.)
                        .child(
                            switch(self.wrap, "Wrap lines")
                                .on_change(cx.listener(|s, e: &ToggleEvent, _| s.wrap = e.checked)),
                        )
                        .child(
                            switch(self.sync, "Background sync")
                                .on_change(cx.listener(|s, e: &ToggleEvent, _| s.sync = e.checked)),
                        ),
                )
                .child(
                    radio_group()
                        .axis(Axis::Horizontal)
                        .selected(self.endings.clone())
                        .accessibility_label("Line endings")
                        .option(choice("lf", "LF"))
                        .option(choice("crlf", "CRLF"))
                        .option(choice("cr", "CR").disabled(true))
                        .on_change(cx.listener(|s, e: &ChoiceEvent, _| s.endings = e.key.clone())),
                )
                .child(
                    row()
                        .gap(12.)
                        .align_items(AlignItems::CENTER)
                        .child(
                            slider(self.zoom)
                                .range(50., 200.)
                                .step(10.)
                                .accessibility_label("Zoom")
                                .on_change(cx.listener(|s, e: &SliderEvent, _| s.zoom = e.value)),
                        )
                        .child(label(format!("{}%", self.zoom)).color(ThemeColor::TextMuted)),
                )
                .child(status_row)
                .child(
                    row()
                        .gap(8.)
                        .align_items(AlignItems::CENTER)
                        .child(
                            select(cx)
                                .selected(self.language.clone())
                                .accessibility_label("Language")
                                .options([
                                    choice("rust", "Rust"),
                                    choice("toml", "TOML"),
                                    choice("md", "Markdown"),
                                    choice("cpp", "C++").disabled(true),
                                    choice("py", "Python"),
                                ])
                                .on_change(
                                    cx.listener(|s, e: &ChoiceEvent, _| s.language = e.key.clone()),
                                ),
                        )
                        .child(tooltip(
                            cx,
                            button("Format")
                                .on_click(cx.listener(|s, _, _| s.status = "Formatted".into())),
                            "Reformat the document with the selected language's rules",
                        )),
                ),
        );

        // Badges in every tone, key caps for shortcuts, and separators that
        // follow their parent's direction.
        let quiet = |icon: Icon, name: &'static str| {
            icon_button(icon)
                .accessibility_label(name)
                .on_click(cx.listener(move |s, _, _| s.status = name.into()))
        };
        let parts = section(
            "Badges, key caps and separators",
            column()
                .gap(12.)
                .fill_width()
                .child(
                    row()
                        .gap(6.)
                        .align_items(AlignItems::CENTER)
                        .child(badge("Draft"))
                        .child(badge("Beta").tone(Tone::Accent))
                        .child(badge("3 errors").tone(Tone::Danger))
                        .child(badge("2 warnings").tone(Tone::Warning))
                        .child(badge("Passing").tone(Tone::Success)),
                )
                .child(
                    row()
                        .gap(8.)
                        .align_items(AlignItems::CENTER)
                        .child(label("Command palette").color(ThemeColor::TextMuted))
                        .child(kbd(Shortcut::primary("p").shift()))
                        .child(separator())
                        .child(label("Rename").color(ThemeColor::TextMuted))
                        .child(kbd(Shortcut::new(
                            KeyboardKey::Function(2),
                            Modifiers::default(),
                        )))
                        .child(separator())
                        .child(label("Back").color(ThemeColor::TextMuted))
                        .child(kbd(Shortcut::new(
                            KeyboardKey::ArrowLeft,
                            Modifiers {
                                alt: true,
                                ..Default::default()
                            },
                        ))),
                )
                .child(separator())
                .child(
                    row()
                        .gap(2.)
                        .align_items(AlignItems::CENTER)
                        .child(quiet(Icon::COPY, "Copy"))
                        .child(quiet(Icon::CLIPBOARD, "Paste"))
                        .child(separator().margin_x(4.))
                        .child(quiet(Icon::SEARCH, "Find"))
                        .child(quiet(Icon::TEXT_WRAP, "Wrap")),
                ),
        );

        // Labelled fields with help and error text, and a disclosure.
        let font_error = (!self.font.contains("Mono")).then_some("Not a monospaced family.");
        let forms = section(
            "Forms",
            column()
                .gap(12.)
                .fill_width()
                .child(
                    field(
                        "Tab width",
                        number_field(cx, self.tab_width)
                            .range(1., 16.)
                            .on_change(cx.listener(|s, e: &NumberEvent, _| s.tab_width = e.value)),
                    )
                    .help("Spaces per indent level."),
                )
                .child(
                    field(
                        "Font family",
                        text_input(&self.font).width(240.).on_change(
                            cx.listener(|s, e: &TextChangeEvent, _| s.font = e.value.clone()),
                        ),
                    )
                    .error(font_error),
                )
                .child(field(
                    "Indentation",
                    select(cx)
                        .selected(self.indent.clone())
                        .option(choice("spaces", "Spaces"))
                        .option(choice("tabs", "Tabs"))
                        .on_change(cx.listener(|s, e: &ChoiceEvent, _| s.indent = e.key.clone())),
                ))
                .child(disclosure(
                    cx,
                    "Advanced",
                    column()
                        .gap(8.)
                        .padding_x(24.)
                        .padding_y(4.)
                        .child(
                            checkbox(self.trim, "Trim trailing whitespace")
                                .on_change(cx.listener(|s, e: &ToggleEvent, _| s.trim = e.checked)),
                        )
                        .child(
                            checkbox(self.final_newline, "Insert final newline").on_change(
                                cx.listener(|s, e: &ToggleEvent, _| s.final_newline = e.checked),
                            ),
                        ),
                )),
        );

        // Callouts in each tone, and buttons that push toasts.
        let mut callouts = column()
            .gap(8.)
            .fill_width()
            .child(callout(Tone::Neutral, "Indexing 1,204 files"))
            .child(
                callout(Tone::Accent, "A new version is available")
                    .description("0.2.0 adds docking presets."),
            )
            .child(
                callout(Tone::Warning, "rust-analyzer is out of date")
                    .description("Some features may not work until it is updated.")
                    .action(
                        button("Update")
                            .on_click(cx.listener(|s, _, _| s.status = "Updating".into())),
                    ),
            )
            .child(callout(Tone::Success, "All tests passed"));
        if self.callout {
            callouts = callouts.child(
                callout(Tone::Danger, "Build failed")
                    .description("error[E0308]: mismatched types in src/main.rs.")
                    .on_dismiss(cx.listener(|s, _: &ClickEvent, _| s.callout = false)),
            );
        }
        let mut toast_buttons = row().gap(8.);
        for (tone, name, title) in [
            (Tone::Neutral, "Info", "Copied to clipboard"),
            (Tone::Success, "Success", "Saved 3 files"),
            (Tone::Warning, "Warning", "Low disk space"),
            (Tone::Danger, "Error", "Push rejected"),
        ] {
            toast_buttons =
                toast_buttons.child(button(name).on_click(cx.listener(move |s, _, _| {
                    s.next_toast += 1;
                    let mut toast = Toast::new(s.next_toast, title).tone(tone);
                    if tone == Tone::Danger {
                        toast = toast
                            .description("The remote contains work you do not have.")
                            .action("Pull");
                    }
                    s.toasts.push(toast);
                })));
        }
        let feedback = section("Feedback", callouts.child(toast_buttons));

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
                    .gap(24.)
                    .child(buttons)
                    .child(tools)
                    .child(inputs)
                    .child(controls)
                    .child(parts)
                    .child(menu_section)
                    .child(dialog),
            )
            .child(
                column()
                    .flex_grow(1.)
                    .flex_basis(0.)
                    .min_width(0.)
                    .gap(24.)
                    .child(quick_open)
                    .child(tabs_section)
                    .child(forms)
                    .child(feedback)
                    .child(icons)
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
            .on_command(delete)
            .child(top)
            .child(
                column()
                    .fill_width()
                    .flex_grow(1.)
                    .min_height(0.)
                    .padding(24.)
                    .gap(16.)
                    .scroll_y()
                    .child(label("Component gallery").text_role(TextRole::Heading))
                    .child(label(self.status.clone()).color(ThemeColor::TextMuted))
                    .child(columns),
            )
            .child(
                toasts(cx, &self.toasts)
                    .on_dismiss(cx.listener(|s, e: &ToastDismiss, _| {
                        s.toasts.retain(|t| t.id != e.id);
                    }))
                    .on_action(cx.listener(|s, _: &ToastAction, _| s.status = "Pulling".into())),
            );
        if self.dialog {
            root = root.child(
                modal(
                    column()
                        .gap(12.)
                        .child(label("Discard unsaved changes?").text_role(TextRole::Subheading))
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
                                .child(button("Discard").variant(ButtonVariant::Danger).on_click(
                                    cx.listener(|s, _, _| {
                                        s.dialog = false;
                                        s.status = "Changes discarded".into();
                                    }),
                                )),
                        ),
                )
                .key("discard-dialog")
                .width(420.)
                .padding(16.)
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
            query: String::new(),
            files: workspace_files(),
            rows: ListSelection::new(SelectionMode::Multiple),
            split: SplitPosition::default(),
            dialog: false,
            status: "Click anything; Primary+N/O/W run the menu commands.".into(),
            email: true,
            push: false,
            wrap: true,
            sync: false,
            endings: Key::from("lf"),
            zoom: 100.,
            language: Key::from("rust"),
            layout: Key::from("list"),
            diff: Key::from("split"),
            tab_width: 4.,
            font: "Comic Code".into(),
            indent: Key::from("spaces"),
            trim: true,
            final_newline: false,
            callout: true,
            toasts: Vec::new(),
            next_toast: 0,
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
