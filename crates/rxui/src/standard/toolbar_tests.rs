use crate::*;

struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.chars().count() as f32 * 8., 16.])
    }
}

/// A toolbar between two buttons, a vertical toolbar, toggle buttons and
/// segmented controls.
struct Tools {
    wrap: bool,
    case: bool,
    layout: Key,
    diff: Key,
    raw_disabled: bool,
    equal: bool,
    unnamed: bool,
    toggles: Vec<bool>,
    choices: Vec<Key>,
}
impl Default for Tools {
    fn default() -> Self {
        Self {
            wrap: false,
            case: true,
            layout: Key::from("list"),
            diff: Key::from("split"),
            raw_disabled: false,
            equal: true,
            unnamed: false,
            toggles: Vec::new(),
            choices: Vec::new(),
        }
    }
}
impl View for Tools {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let tools = toolbar()
            .key("tools")
            .accessibility_label("Tools")
            .child(
                icon_button(Icon::PLUS)
                    .key("new")
                    .accessibility_label("New"),
            )
            .child(button("Search").key("search"))
            .child(separator().key("sep"))
            .child(
                toggle_button(self.wrap, Icon::TEXT_WRAP)
                    .key("wrap")
                    .accessibility_label("Wrap")
                    .on_change(cx.listener(|s, e: &ToggleEvent, _| {
                        s.wrap = e.checked;
                        s.toggles.push(e.checked);
                    })),
            )
            .child(button("Cut").key("cut").disabled(true))
            .child(
                segmented()
                    .key("layout")
                    .selected(self.layout.clone())
                    .accessibility_label("Layout")
                    .option(choice("list", "List").icon(Icon::LIST))
                    .option(choice("grid", "Grid").icon(Icon::LAYOUT_GRID))
                    .on_change(cx.listener(|s, e: &ChoiceEvent, _| s.layout = e.key.clone())),
            )
            .child(separator())
            .child(button("Run").key("run"));
        let mut root = column()
            .padding(10.)
            .gap(10.)
            .child(button("Before").key("before"))
            .child(tools)
            .child(button("After").key("after"))
            .child(
                row().child(
                    toolbar()
                        .vertical()
                        .key("side")
                        .child(icon_button(Icon::FILE).key("files").accessibility_label("Files"))
                        .child(separator().key("side-sep"))
                        .child(icon_button(Icon::SEARCH).key("find").accessibility_label("Find"))
                        .child(icon_button(Icon::BUG).key("debug").accessibility_label("Debug")),
                ),
            )
            .child(
                toggle_button(self.case, "Aa")
                    .key("case")
                    .on_change(cx.listener(|s, e: &ToggleEvent, _| s.case = e.checked)),
            )
            // In a row, so the column does not stretch it.
            .child(row().child(
                segmented()
                    .key("diff")
                    .selected(self.diff.clone())
                    .accessibility_label("Diff view")
                    .equal_width(self.equal)
                    .option(choice("split", "Split"))
                    .option(choice("unified", "Unified").disabled(true))
                    .option(choice("raw", "Raw").disabled(self.raw_disabled))
                    .on_change(cx.listener(|s, e: &ChoiceEvent, _| {
                        s.diff = e.key.clone();
                        s.choices.push(e.key.clone());
                    })),
            ));
        if self.unnamed {
            root = root.child(toggle_button(false, Icon::X));
        }
        root
    }
}

struct Harness {
    r: Runtime,
    e: Entity<Tools>,
    ui: Ui<Tools>,
}
impl Harness {
    fn new() -> Self {
        let mut r = Runtime::new();
        let e = r.update(|cx| cx.new(|_| Tools::default()));
        let mut ui = Ui::new(&mut r, e.clone()).unwrap();
        ui.prepare(&mut r, [600., 600.], &mut Measure).unwrap();
        Self { r, e, ui }
    }
    fn try_prepare(&mut self) -> Result<(), UiError> {
        self.ui.prepare(&mut self.r, [600., 600.], &mut Measure)
    }
    fn prepare(&mut self) {
        self.try_prepare().unwrap();
    }
    fn update(&mut self, f: impl FnOnce(&mut Tools)) {
        self.r.update(|cx| self.e.update(cx, |s, _| f(s)));
    }
    fn read<R>(&mut self, f: impl FnOnce(&Tools) -> R) -> R {
        let e = self.e.clone();
        self.r.update(|cx| f(&e.read(cx)))
    }
    fn id(&self, key: &str) -> ElementId {
        let key = Key::from(key);
        self.ui
            .elements()
            .find(|e| e.key == Some(&key))
            .unwrap_or_else(|| panic!("no element {key:?}"))
            .id
    }
    fn info(&self, key: &str) -> ElementInfo<'_> {
        self.ui.element(self.id(key)).unwrap()
    }
    fn focused(&self) -> Option<&Key> {
        self.ui.element(self.ui.focused_element()?)?.key
    }
    fn key(&mut self, key: KeyboardKey) -> InputResult {
        let result = self
            .ui
            .key(
                &mut self.r,
                KeyEvent {
                    physical_key: Default::default(),
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::default(),
                },
            )
            .unwrap();
        self.prepare();
        result
    }
    fn tab(&mut self, reverse: bool) {
        self.ui.focus_next(reverse);
        self.prepare();
    }
    fn click(&mut self, key: &str) {
        let b = self.info(key).bounds;
        let at = [b.x + b.width / 2., b.y + b.height / 2.];
        for event in [
            PointerEvent::Moved(at),
            PointerEvent::Pressed(at),
            PointerEvent::Released(at),
        ] {
            self.ui.pointer(&mut self.r, event).unwrap();
            self.prepare();
        }
    }
}

#[test]
fn toolbars_are_one_tab_stop_whose_arrow_keys_rove_and_remember_the_last_item() {
    let mut h = Harness::new();
    let focused = |h: &Harness| h.focused().cloned();
    let k = |s: &str| Some(Key::from(s));

    // Tab enters at the first item and leaves with the next Tab.
    h.ui.focus(h.id("before"));
    h.tab(false);
    assert_eq!(focused(&h), k("new"));
    h.tab(false);
    assert_eq!(focused(&h), k("after"));

    // Right skips the separators and the disabled item. The segmented control
    // keeps the key until its last segment, then hands it to the toolbar.
    h.ui.focus(h.id("new"));
    for (key, expected) in [
        (KeyboardKey::ArrowRight, "search"),
        (KeyboardKey::ArrowRight, "wrap"),
        (KeyboardKey::ArrowRight, "list"),
        (KeyboardKey::ArrowRight, "grid"),
        (KeyboardKey::ArrowRight, "run"),
        // Wrapping at both ends.
        (KeyboardKey::ArrowRight, "new"),
        (KeyboardKey::ArrowLeft, "run"),
        (KeyboardKey::Home, "new"),
        (KeyboardKey::End, "run"),
        (KeyboardKey::ArrowLeft, "grid"),
    ] {
        assert!(h.key(key.clone()).default_prevented, "{key:?}");
        assert_eq!(focused(&h), k(expected), "{key:?}");
    }
    assert_eq!(h.read(|s| s.layout.clone()), Key::from("grid"));
    // Up and Down do not rove in a horizontal toolbar.
    h.ui.focus(h.id("search"));
    assert!(!h.key(KeyboardKey::ArrowDown).default_prevented);
    assert_eq!(focused(&h), k("search"));

    // Tab leaves from any item, and re-entry from either side restores it.
    h.tab(false);
    assert_eq!(focused(&h), k("after"));
    h.tab(true);
    assert_eq!(focused(&h), k("search"));
    h.tab(true);
    assert_eq!(focused(&h), k("before"));
    h.tab(false);
    assert_eq!(focused(&h), k("search"));
    // A click is remembered too.
    h.click("run");
    h.ui.focus(h.id("before"));
    h.tab(false);
    assert_eq!(focused(&h), k("run"));

    let node = h.ui.semantic_node(h.id("tools")).unwrap();
    assert_eq!(node.role, SemanticRole::Toolbar);
    assert_eq!(node.label, Some("Tools"));
    assert_eq!(node.orientation, Some(Axis::Horizontal));
    assert!(!node.focusable);
}

#[test]
fn toolbars_center_items_at_control_height_and_vertical_ones_rove_with_up_and_down() {
    let mut h = Harness::new();
    let m = ThemeMetrics::balanced();
    let tools = h.info("tools").bounds;
    assert_eq!(tools.height, m.input_height);
    let new = h.info("new").bounds;
    let search = h.info("search").bounds;
    assert_eq!(search.x, new.x + new.width + 4.);
    for key in ["new", "search", "sep", "wrap", "layout", "run"] {
        let b = h.info(key).bounds;
        let offset = b.y + b.height / 2. - (tools.y + tools.height / 2.);
        assert!(offset.abs() < 0.01, "{key}: {offset}");
    }
    // The vertical toolbar stacks its items and keeps a horizontal line.
    let side = h.info("side").bounds;
    assert_eq!(side.width, m.input_height);
    let sep = h.info("side-sep").bounds;
    assert_eq!([sep.width, sep.height], [m.input_height, 1.]);
    let node = h.ui.semantic_node(h.id("side")).unwrap();
    assert_eq!(node.orientation, Some(Axis::Vertical));

    h.ui.focus(h.id("files"));
    for (key, expected) in [
        (KeyboardKey::ArrowDown, "find"),
        (KeyboardKey::ArrowDown, "debug"),
        (KeyboardKey::ArrowDown, "files"),
        (KeyboardKey::ArrowUp, "debug"),
        (KeyboardKey::Home, "files"),
        (KeyboardKey::End, "debug"),
    ] {
        assert!(h.key(key.clone()).default_prevented, "{key:?}");
        assert_eq!(h.focused(), Some(&Key::from(expected)), "{key:?}");
    }
    assert!(!h.key(KeyboardKey::ArrowLeft).default_prevented);
    assert_eq!(h.focused(), Some(&Key::from("debug")));
}

#[test]
fn toggle_buttons_paint_publish_and_propose_their_pressed_state() {
    let mut h = Harness::new();
    for theme in [Theme::dark(), Theme::light()] {
        h.ui.set_theme(theme.clone()).unwrap();
        h.prepare();
        let c = theme.palette();
        // Pressed: the selected-row fill with ordinary text.
        let case = h.info("case");
        assert_eq!(case.background, Some(c.row_selected));
        assert_eq!(case.color, c.text);
        // Unpressed: a quiet button with no fill and muted content.
        let wrap = h.info("wrap");
        assert_eq!(wrap.background, None);
        assert_eq!(wrap.color, c.text_muted);
    }
    let m = ThemeMetrics::balanced();
    let wrap = h.info("wrap").bounds;
    assert_eq!([wrap.width, wrap.height], [m.input_height; 2]);

    let node = h.ui.semantic_node(h.id("wrap")).unwrap();
    assert_eq!(node.role, SemanticRole::Button);
    assert_eq!(node.label, Some("Wrap"));
    assert_eq!(node.checked, Some(Checked::False));
    let node = h.ui.semantic_node(h.id("case")).unwrap();
    assert_eq!(
        (node.label, node.checked),
        (Some("Aa"), Some(Checked::True))
    );

    // Activation proposes the opposite state; the application stores it.
    h.click("wrap");
    h.click("wrap");
    h.click("case");
    h.ui.pointer(&mut h.r, PointerEvent::Left).unwrap();
    h.prepare();
    assert_eq!(h.read(|s| s.toggles.clone()), [true, false]);
    assert!(!h.read(|s| s.case));
    let node = h.ui.semantic_node(h.id("case")).unwrap();
    assert_eq!(node.checked, Some(Checked::False));
    assert_eq!(h.info("case").background, None);

    // An icon toggle without an accessible name fails preparation.
    h.update(|s| s.unnamed = true);
    let error = h.try_prepare().unwrap_err();
    assert!(matches!(error, UiError::MissingAccessibleName), "{error}");
}

#[test]
fn segmented_controls_raise_the_selected_segment_and_select_with_arrow_keys() {
    let mut h = Harness::new();
    for theme in [Theme::dark(), Theme::light()] {
        h.ui.set_theme(theme.clone()).unwrap();
        h.prepare();
        let c = theme.palette();
        let track = h.info("diff");
        assert_eq!(track.background, Some(c.input));
        assert_eq!(track.paint.border_color, Some(c.input_border));
        let split = h.info("split");
        assert_eq!(split.background, Some(c.control));
        assert_eq!(split.paint.border_color, Some(c.border));
        assert_eq!(split.color, c.text);
        assert_eq!(
            split.paint.shadow,
            StyleShadow::Theme(Elevation::Control).resolve(&theme)
        );
        let raw = h.info("raw");
        assert_eq!(raw.background, None);
        assert_eq!(raw.color, c.text_muted);
    }

    let node = h.ui.semantic_node(h.id("diff")).unwrap();
    assert_eq!(
        (node.role, node.label),
        (SemanticRole::RadioGroup, Some("Diff view"))
    );
    let node = h.ui.semantic_node(h.id("raw")).unwrap();
    assert_eq!(node.role, SemanticRole::RadioButton);
    assert_eq!(
        (node.label, node.checked),
        (Some("Raw"), Some(Checked::False))
    );
    assert_eq!((node.set_size, node.position_in_set), (Some(3), Some(2)));
    assert!(h.ui.semantic_node(h.id("unified")).unwrap().disabled);
    let node = h.ui.semantic_node(h.id("grid")).unwrap();
    assert_eq!(node.label, Some("Grid"));

    // One Tab stop: the selected segment.
    h.ui.focus(h.id("case"));
    h.tab(false);
    assert_eq!(h.focused(), Some(&Key::from("split")));
    h.tab(false);
    assert_ne!(h.focused().cloned(), Some(Key::from("raw")));

    // Arrows skip the disabled segment, select, and stop at the ends.
    h.ui.focus(h.id("split"));
    assert!(h.key(KeyboardKey::ArrowRight).default_prevented);
    assert_eq!(h.focused(), Some(&Key::from("raw")));
    assert_eq!(h.read(|s| s.diff.clone()), Key::from("raw"));
    assert!(!h.key(KeyboardKey::ArrowRight).default_prevented);
    assert_eq!(h.focused(), Some(&Key::from("raw")));
    assert!(h.key(KeyboardKey::ArrowUp).default_prevented);
    assert_eq!(h.focused(), Some(&Key::from("split")));
    assert_eq!(
        h.read(|s| s.choices.clone()),
        [Key::from("raw"), Key::from("split")]
    );
    h.update(|s| s.raw_disabled = true);
    h.prepare();
    assert!(!h.key(KeyboardKey::ArrowRight).default_prevented);
    h.click("unified");
    assert_eq!(h.read(|s| s.diff.clone()), Key::from("split"));
    h.click("raw");
    assert_eq!(h.read(|s| s.diff.clone()), Key::from("split"));
}

#[test]
fn segments_share_the_width_equally_and_shrink_with_compact_density() {
    let mut h = Harness::new();
    for theme in [Theme::dark(), Theme::dark().compact()] {
        h.ui.set_theme(theme.clone()).unwrap();
        h.update(|s| s.equal = true);
        h.prepare();
        let m = theme.sizes();
        let track = h.info("diff").bounds;
        assert_eq!(track.height, m.input_height);
        let segments = ["split", "unified", "raw"].map(|k| h.info(k).bounds);
        // Each segment is as wide as the widest, "Unified".
        let widest = 7. * 8. + 2. * m.button_padding_x + 2.;
        for (i, b) in segments.iter().enumerate() {
            assert_eq!(b.height, m.input_height - 4., "{i}");
            assert_eq!(b.y, track.y + 2.);
            assert!((b.width - widest).abs() < 0.01, "{i}: {}", b.width);
        }
        assert_eq!(segments[0].x, track.x + 2.);
        assert_eq!(track.width, 3. * widest + 4.);
        // Icon segments are square.
        let grid = h.info("grid").bounds;
        assert_eq!([grid.width, grid.height], [m.input_height - 4.; 2]);
        // Without equal widths each segment fits its caption.
        h.update(|s| s.equal = false);
        h.prepare();
        let split = h.info("split").bounds;
        assert_eq!(split.width, 5. * 8. + 2. * m.button_padding_x + 2.);
    }
}
