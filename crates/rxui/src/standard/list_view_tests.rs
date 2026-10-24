use crate::*;

struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.chars().count() as f32 * 8., 16.])
    }
}

const NAMES: [&str; 8] = [
    "Cargo.toml",
    "README.md",
    "build.rs",
    "benches",
    "docs",
    "examples",
    "src",
    "tests",
];

struct Files {
    names: Vec<String>,
    selection: ListSelection,
    proposals: usize,
    /// Visible rows.
    rows: f32,
}
impl View for Files {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .padding(10.)
            .gap(10.)
            .child(button("Before").key("before"))
            .child(
                list_view(cx)
                    .key("files")
                    .accessibility_label("Files")
                    .height(self.rows * ThemeMetrics::balanced().row_height)
                    .selection(&self.selection)
                    .items(self.names.len(), |i| {
                        ListItem::new(self.names[i].as_str(), &self.names[i]).icon(Icon::FILE)
                    })
                    .on_change(cx.listener(|s, e: &ListSelectionEvent, _| {
                        s.selection = e.selection.clone();
                        s.proposals += 1;
                    })),
            )
    }
}

struct Harness {
    r: Runtime,
    e: Entity<Files>,
    ui: Ui<Files>,
}
impl Harness {
    fn new(mode: SelectionMode) -> Self {
        Self::with_rows(mode, 8.)
    }
    fn with_rows(mode: SelectionMode, rows: f32) -> Self {
        let mut r = Runtime::new();
        let e = r.update(|cx| {
            cx.new(|_| Files {
                names: NAMES.iter().map(|n| n.to_string()).collect(),
                selection: ListSelection::new(mode),
                proposals: 0,
                rows,
            })
        });
        let ui = Ui::new(&mut r, e.clone()).unwrap();
        let mut h = Self { r, e, ui };
        h.prepare();
        h.ui.focus(h.id("files"));
        h.prepare();
        h
    }
    fn prepare(&mut self) {
        // The list measures its viewport before describing rows.
        for _ in 0..3 {
            self.ui
                .prepare(&mut self.r, [400., 400.], &mut Measure)
                .unwrap();
        }
    }
    fn read<R>(&mut self, f: impl FnOnce(&Files) -> R) -> R {
        let e = self.e.clone();
        self.r.update(|cx| f(&e.read(cx)))
    }
    fn find(&self, key: &str) -> Option<ElementId> {
        let key = Key::from(key);
        self.ui
            .elements()
            .find(|e| e.key == Some(&key))
            .map(|e| e.id)
    }
    fn id(&self, key: &str) -> ElementId {
        self.find(key)
            .unwrap_or_else(|| panic!("no element {key:?}"))
    }
    /// The option row inside the virtual list's slot for `key`.
    fn row(&self, key: &str) -> ElementId {
        let slot = self.id(key);
        self.ui
            .elements()
            .find(|e| e.parent == Some(slot))
            .unwrap()
            .id
    }
    fn key_with(&mut self, key: KeyboardKey, modifiers: Modifiers) -> bool {
        let event = KeyEvent {
            physical_key: Default::default(),
            key,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let prevented = self.ui.key(&mut self.r, event).unwrap().default_prevented;
        self.prepare();
        prevented
    }
    fn key(&mut self, key: KeyboardKey) -> bool {
        self.key_with(key, Modifiers::default())
    }
    fn click(&mut self, key: &str, modifiers: Modifiers) {
        let b = self.ui.element(self.row(key)).unwrap().bounds;
        let position = [b.x + 4., b.y + b.height / 2.];
        let button = PointerButton::Primary;
        for event in [
            PointerEvent::Down {
                position,
                button,
                modifiers,
            },
            PointerEvent::Up {
                position,
                button,
                modifiers,
            },
        ] {
            self.ui.pointer(&mut self.r, event).unwrap();
            self.prepare();
        }
    }
    fn selected(&mut self) -> Vec<String> {
        let mut keys: Vec<String> = self.read(|s| {
            s.selection
                .selected()
                .iter()
                .map(|k| match k {
                    Key::String(s) => s.to_string(),
                    Key::Integer(i) => i.to_string(),
                })
                .collect()
        });
        keys.sort_by_key(|k| NAMES.iter().position(|n| n == k));
        keys
    }
    fn active(&mut self) -> Option<Key> {
        self.read(|s| s.selection.active().cloned())
    }
}

fn primary() -> Modifiers {
    Modifiers {
        control: !cfg!(target_os = "macos"),
        meta: cfg!(target_os = "macos"),
        ..Modifiers::default()
    }
}
fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::default()
    }
}

#[test]
fn single_selection_follows_arrows_pages_and_ends() {
    let mut h = Harness::with_rows(SelectionMode::Single, 4.);
    assert!(h.key(KeyboardKey::ArrowDown));
    assert_eq!(h.selected(), ["README.md"]);
    assert!(h.key(KeyboardKey::ArrowUp));
    assert_eq!(h.selected(), ["Cargo.toml"]);
    // Up at the top changes nothing and proposes nothing.
    let proposals = h.read(|s| s.proposals);
    assert!(h.key(KeyboardKey::ArrowUp));
    assert_eq!(h.read(|s| s.proposals), proposals);
    // A page is the 4 visible rows.
    assert!(h.key(KeyboardKey::PageDown));
    assert_eq!(h.selected(), ["docs"]);
    assert!(h.key(KeyboardKey::End));
    assert_eq!(h.selected(), ["tests"]);
    // The active row scrolled into view.
    assert!(h.find("tests").is_some());
    assert!(h.find("Cargo.toml").is_none());
    assert!(h.key(KeyboardKey::Home));
    assert_eq!(h.selected(), ["Cargo.toml"]);
    // Primary moves only the active row; Space selects it.
    assert!(h.key_with(KeyboardKey::ArrowDown, primary()));
    assert_eq!(h.selected(), ["Cargo.toml"]);
    assert_eq!(h.active(), Some(Key::from("README.md")));
    assert!(h.key(KeyboardKey::Space));
    assert_eq!(h.selected(), ["README.md"]);
    // Shift does not extend in single mode.
    assert!(h.key_with(KeyboardKey::ArrowDown, shift()));
    assert_eq!(h.selected(), ["build.rs"]);
    // Clicks select, and Primary-click does not deselect in single mode.
    h.click("benches", Modifiers::default());
    assert_eq!(h.selected(), ["benches"]);
    h.click("Cargo.toml", primary());
    assert_eq!(h.selected(), ["Cargo.toml"]);
    h.click("Cargo.toml", primary());
    assert_eq!(h.selected(), ["Cargo.toml"]);
}

#[test]
fn multiple_selection_extends_with_shift_and_toggles_with_primary() {
    let mut h = Harness::new(SelectionMode::Multiple);
    h.click("README.md", Modifiers::default());
    assert!(h.key_with(KeyboardKey::ArrowDown, shift()));
    assert!(h.key_with(KeyboardKey::ArrowDown, shift()));
    assert_eq!(h.selected(), ["README.md", "build.rs", "benches"]);
    // Shift-click selects from the anchor.
    h.click("docs", shift());
    assert_eq!(h.selected(), ["README.md", "build.rs", "benches", "docs"]);
    // Primary-click toggles and moves the anchor.
    h.click("build.rs", primary());
    assert_eq!(h.selected(), ["README.md", "benches", "docs"]);
    h.click("src", primary());
    assert_eq!(h.selected(), ["README.md", "benches", "docs", "src"]);
    // Primary+Shift adds a range from the new anchor.
    h.click(
        "tests",
        Modifiers {
            shift: true,
            ..primary()
        },
    );
    assert_eq!(
        h.selected(),
        ["README.md", "benches", "docs", "src", "tests"]
    );
    // Primary+Space toggles the active row; Primary moves it without selecting.
    assert!(h.key_with(KeyboardKey::ArrowUp, primary()));
    assert!(h.key_with(KeyboardKey::ArrowUp, primary()));
    assert_eq!(h.active(), Some(Key::from("examples")));
    assert!(h.key_with(KeyboardKey::Space, primary()));
    assert_eq!(
        h.selected(),
        ["README.md", "benches", "docs", "examples", "src", "tests"]
    );
    // Primary+A selects everything; a plain move selects one row again.
    assert!(h.key_with(KeyboardKey::Character("a".into()), primary()));
    assert_eq!(h.selected(), NAMES);
    assert!(h.key(KeyboardKey::ArrowDown));
    assert_eq!(h.selected(), ["src"]);
}

#[test]
fn typing_selects_the_next_row_starting_with_the_typed_text() {
    let mut h = Harness::new(SelectionMode::Multiple);
    assert!(h.key(KeyboardKey::Character("b".into())));
    assert_eq!(h.selected(), ["build.rs"]);
    // A repeated letter cycles; a longer prefix keeps the current match.
    assert!(h.key(KeyboardKey::Character("b".into())));
    assert_eq!(h.selected(), ["benches"]);
    assert!(h.key(KeyboardKey::Character("b".into())));
    assert_eq!(h.selected(), ["build.rs"]);
    assert!(h.key(KeyboardKey::Character("e".into())));
    assert_eq!(h.selected(), ["benches"]);
    // Type-ahead does not extend a range, even with Shift.
    h.click("Cargo.toml", Modifiers::default());
    assert!(h.key_with(KeyboardKey::Character("S".into()), shift()));
    assert_eq!(h.selected(), ["src"]);
    // Primary+A is select-all, not type-ahead; other shortcuts pass through.
    assert!(!h.key_with(KeyboardKey::Character("x".into()), primary()));
    assert_eq!(h.selected(), ["src"]);
}

#[test]
fn selection_paints_inactive_without_focus_and_publishes_listbox_semantics() {
    let mut h = Harness::new(SelectionMode::Multiple);
    h.click("README.md", Modifiers::default());
    h.click("docs", primary());
    let theme = Theme::dark();
    let c = theme.palette();
    let background = |h: &Harness, key: &str| h.ui.element(h.row(key)).unwrap().background;
    assert_eq!(h.ui.focused_element(), Some(h.id("files")));
    assert_eq!(background(&h, "README.md"), Some(c.row_selected));
    assert_eq!(background(&h, "build.rs"), None);
    h.ui.focus(h.id("before"));
    h.prepare();
    assert_eq!(background(&h, "README.md"), Some(c.row_selected_inactive));

    let list = h.ui.semantic_node(h.id("files")).unwrap();
    assert_eq!(list.role, SemanticRole::ListBox);
    assert_eq!(list.label, Some("Files"));
    assert!(list.multiselectable);
    assert!(list.focusable);
    assert_eq!(list.active_descendant, Some(h.row("docs")));
    let option = h.ui.semantic_node(h.row("README.md")).unwrap();
    assert_eq!(option.role, SemanticRole::ListBoxOption);
    assert_eq!(option.label, Some("README.md"));
    assert_eq!(option.selected, Some(true));
    assert_eq!(
        (option.position_in_set, option.set_size),
        (Some(1), Some(8))
    );
    assert!(!option.focusable);
    // One Tab stop: Tab from the button enters the list, and the next leaves it.
    h.ui.focus_next(false);
    h.prepare();
    assert_eq!(h.ui.focused_element(), Some(h.id("files")));
    h.ui.focus_next(false);
    h.prepare();
    assert_eq!(h.ui.focused_element(), Some(h.id("before")));
    let single = Harness::new(SelectionMode::Single);
    let list = single.ui.semantic_node(single.id("files")).unwrap();
    assert!(!list.multiselectable);
}

#[test]
fn assistive_selection_is_controlled() {
    let mut h = Harness::new(SelectionMode::Single);
    let row = h.row("docs");
    assert!(
        h.ui.semantic_action(&mut h.r, SemanticAction::Select(row), &mut Measure)
            .unwrap()
    );
    h.prepare();
    assert_eq!(h.selected(), ["docs"]);
    assert_eq!(h.active(), Some(Key::from("docs")));
}
