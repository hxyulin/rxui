use crate::*;
use std::time::Duration;

/// Measures 8 units per char and 20 per line.
pub(super) struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.chars().count() as f32 * 8., 20.])
    }
}

/// A mounted view with input helpers.
pub(super) struct H<V: View> {
    pub r: Runtime,
    pub e: Entity<V>,
    pub ui: Ui<V>,
}
impl<V: View> H<V> {
    pub fn new(view: V) -> Self {
        let mut r = Runtime::new();
        let e = r.update(|cx| cx.new(|_| view));
        let ui = Ui::new(&mut r, e.clone()).unwrap();
        let mut h = Self { r, e, ui };
        h.prepare();
        h
    }
    /// Prepares until scroll geometry that views read has settled, running
    /// deferred proposals in between.
    pub fn prepare(&mut self) {
        for _ in 0..3 {
            self.ui
                .prepare(&mut self.r, [800., 600.], &mut Measure)
                .unwrap();
            self.r.flush().unwrap();
        }
    }
    pub fn update(&mut self, f: impl FnOnce(&mut V)) {
        self.r.update(|cx| self.e.update(cx, |s, _| f(s)));
        self.prepare();
    }
    pub fn read<R>(&mut self, f: impl FnOnce(&V) -> R) -> R {
        let e = self.e.clone();
        self.r.update(|cx| f(&e.read(cx)))
    }
    pub fn find(&self, key: impl Into<Key>) -> Option<ElementId> {
        let key = key.into();
        self.ui
            .elements()
            .find(|e| e.key == Some(&key) && !e.inert)
            .map(|e| e.id)
    }
    pub fn id(&self, key: impl Into<Key>) -> ElementId {
        let key = key.into();
        self.find(key.clone())
            .unwrap_or_else(|| panic!("no element {key:?}"))
    }
    /// The element with `key` inside `root`'s subtree.
    pub fn within(&self, root: impl Into<Key>, key: impl Into<Key>) -> ElementId {
        let root = self.id(root);
        let key = key.into();
        self.ui
            .elements()
            .find(|e| {
                e.key == Some(&key) && {
                    let mut parent = Some(e.id);
                    while let Some(id) = parent {
                        if id == root {
                            break;
                        }
                        parent = self.ui.element(id).unwrap().parent;
                    }
                    parent.is_some()
                }
            })
            .unwrap_or_else(|| panic!("no element {key:?}"))
            .id
    }
    pub fn center(&self, id: ElementId) -> [f32; 2] {
        let b = self.ui.element(id).unwrap().bounds;
        [b.x + b.width / 2., b.y + b.height / 2.]
    }
    pub fn pointer(&mut self, event: PointerEvent) {
        self.ui.pointer(&mut self.r, event).unwrap();
        self.prepare();
    }
    pub fn click(&mut self, id: ElementId) {
        let at = self.center(id);
        self.pointer(PointerEvent::Moved(at));
        self.pointer(PointerEvent::Pressed(at));
        self.pointer(PointerEvent::Released(at));
    }
    /// Presses `key` and reports whether the default was prevented.
    pub fn key(&mut self, key: KeyboardKey) -> bool {
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
        result.default_prevented
    }
    pub fn type_text(&mut self, text: &str) {
        self.ui
            .text_input(
                &mut self.r,
                TextInputEvent::Insert(text.into()),
                &mut Measure,
            )
            .unwrap();
        self.prepare();
    }
    pub fn select_all(&mut self) {
        self.ui
            .text_input(&mut self.r, TextInputEvent::SelectAll, &mut Measure)
            .unwrap();
        self.prepare();
    }
    pub fn frame(&mut self, millis: u64) {
        self.r.begin_frame(Duration::from_millis(millis));
        self.prepare();
    }
    pub fn paint(&self, id: ElementId) -> ResolvedPaint {
        self.ui.element(id).unwrap().paint
    }
    pub fn text(&self, id: ElementId) -> String {
        self.ui.element(id).unwrap().text.unwrap_or_default().into()
    }
}

struct Forms {
    tab: f64,
    events: Vec<NumberEvent>,
    step: f64,
    precision: Option<usize>,
    disabled: bool,
    read_only: bool,
    font: String,
    error: Option<String>,
    inline: bool,
    language: Key,
    query: String,
}
impl Default for Forms {
    fn default() -> Self {
        Self {
            tab: 4.,
            events: Vec::new(),
            step: 1.,
            precision: None,
            disabled: false,
            read_only: false,
            font: "Mono".into(),
            error: None,
            inline: false,
            language: Key::from("rust"),
            query: String::new(),
        }
    }
}
impl View for Forms {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut number = number_field(cx, self.tab)
            .key("tab")
            .range(1., 16.)
            .step(self.step)
            .disabled(self.disabled)
            .read_only(self.read_only)
            .on_change(cx.listener(|s, e: &NumberEvent, _| {
                s.tab = e.value;
                s.events.push(*e);
            }));
        if let Some(precision) = self.precision {
            number = number.precision(precision);
        }
        let mut font = field(
            "Font family",
            text_input(&self.font)
                .key("font")
                .on_change(cx.listener(|s, e: &TextChangeEvent, _| s.font = e.value.clone())),
        )
        .key("font-field")
        .help("A monospaced family.")
        .error(self.error.as_deref());
        if self.inline {
            font = font.inline(120.);
        }
        column()
            .padding(10.)
            .gap(8.)
            .child(
                field("Tab width", number)
                    .key("tab-field")
                    .help("Spaces per indent level.")
                    .error(self.error.as_deref()),
            )
            .child(font)
            .child(
                field(
                    "Language",
                    select(cx)
                        .key("language")
                        .selected(self.language.clone())
                        .option(choice("rust", "Rust"))
                        .option(choice("go", "Go")),
                )
                .key("language-field")
                .error(self.error.as_deref()),
            )
            .child(
                field(
                    "Go to",
                    combobox(cx)
                        .key("combo")
                        .query(&self.query)
                        .items(2, |i| ComboItem::new(["a.rs", "b.rs"][i])),
                )
                .key("combo-field")
                .error(self.error.as_deref()),
            )
    }
}

/// The control of the field `root`: its input or trigger.
fn control(h: &H<Forms>, root: &str) -> ElementId {
    match root {
        "tab-field" | "combo-field" => h.within(root, "input"),
        "language-field" => h.within(root, "button"),
        _ => h.id("font"),
    }
}

const FIELDS: [&str; 4] = ["tab-field", "font-field", "language-field", "combo-field"];

#[test]
fn fields_label_and_describe_their_control() {
    let mut h = H::new(Forms::default());
    for root in FIELDS {
        let node = h.ui.semantic_node(control(&h, root)).unwrap();
        let label = h.ui.semantic_node(node.labelled_by.unwrap()).unwrap();
        assert_eq!(label.role, SemanticRole::Label);
        assert!(!node.invalid, "{root}");
        let caption = label.value.unwrap().to_string();
        assert_eq!(
            caption,
            match root {
                "tab-field" => "Tab width",
                "font-field" => "Font family",
                "language-field" => "Language",
                _ => "Go to",
            }
        );
    }
    let tab = h.ui.semantic_node(control(&h, "tab-field")).unwrap();
    assert_eq!(tab.description, Some("Spaces per indent level."));
    // Help text is a muted caption below the control.
    let help = h.within("font-field", "help");
    assert_eq!(h.text(help), "A monospaced family.");
    let theme = Theme::dark();
    let c = theme.palette();
    assert_eq!(h.ui.element(help).unwrap().color, c.text_muted);
    let input = h.ui.element(h.id("font")).unwrap().bounds;
    let below = h.ui.element(help).unwrap().bounds;
    assert!(below.y >= input.y + input.height);

    // An error replaces the help text, describes the control and marks it invalid.
    h.update(|s| s.error = Some("Not installed".into()));
    let font = h.ui.semantic_node(h.id("font")).unwrap();
    assert!(font.invalid);
    assert_eq!(font.description, Some("Not installed"));
    assert!(
        h.ui.elements()
            .all(|e| e.key != Some(&Key::from("help")) || e.text.is_none())
    );
    let error = h.within("font-field", "error");
    let text =
        h.ui.elements()
            .find(|e| e.parent == Some(error) && e.text.is_some())
            .unwrap();
    assert_eq!(text.text, Some("Not installed"));
    assert_eq!(text.color, c.danger);
    h.update(|s| s.error = None);
    assert!(!h.ui.semantic_node(h.id("font")).unwrap().invalid);
}

#[test]
fn an_error_paints_each_control_type_with_a_danger_border() {
    let mut h = H::new(Forms::default());
    let theme = Theme::dark();
    let c = theme.palette();
    for root in FIELDS {
        assert_ne!(
            h.paint(control(&h, root)).border_color,
            Some(c.danger),
            "{root}"
        );
    }
    h.update(|s| s.error = Some("Invalid".into()));
    for root in FIELDS {
        let id = control(&h, root);
        assert_eq!(h.paint(id).border_color, Some(c.danger), "{root}");
        assert!(h.ui.semantic_node(id).unwrap().invalid, "{root}");
    }
    // A disabled control keeps the disabled border.
    h.update(|s| s.disabled = true);
    assert_eq!(
        h.paint(control(&h, "tab-field")).border_color,
        Some(c.border_disabled)
    );
}

#[test]
fn inline_fields_put_the_label_beside_the_control() {
    let mut h = H::new(Forms {
        inline: true,
        ..Default::default()
    });
    let font = h.ui.element(h.id("font")).unwrap().bounds;
    let label =
        h.ui.semantic_node(control(&h, "font-field"))
            .unwrap()
            .labelled_by;
    let label = h.ui.element(label.unwrap()).unwrap().bounds;
    assert_eq!(label.width, 120.);
    assert_eq!(font.x, label.x + 120. + 12.);
    let help = h.ui.element(h.within("font-field", "help")).unwrap().bounds;
    assert_eq!(help.x, font.x);
    h.update(|s| s.inline = false);
}

fn input(h: &H<Forms>) -> ElementId {
    h.within("tab", "input")
}

fn focus_number(h: &mut H<Forms>) {
    let id = input(h);
    h.ui.focus(id);
    h.prepare();
}

#[test]
fn number_fields_format_parse_clamp_and_round_to_the_precision() {
    let mut h = H::new(Forms::default());
    assert_eq!(h.text(input(&h)), "4");
    // The default precision follows the step.
    h.update(|s| {
        s.step = 0.25;
        s.tab = 1.5;
    });
    assert_eq!(h.text(input(&h)), "1.50");
    h.update(|s| s.precision = Some(1));
    assert_eq!(h.text(input(&h)), "1.5");
    h.update(|s| {
        s.precision = None;
        s.step = 1.;
    });

    focus_number(&mut h);
    let commit = |h: &mut H<Forms>, text: &str| {
        h.select_all();
        h.type_text(text);
        h.key(KeyboardKey::Enter);
        h.read(|s| s.tab)
    };
    assert_eq!(commit(&mut h, " 7 "), 7.);
    assert_eq!(commit(&mut h, "40"), 16.);
    assert_eq!(commit(&mut h, "-3"), 1.);
    assert_eq!(commit(&mut h, "2.6"), 3.);
    assert_eq!(h.text(input(&h)), "3");
    // Invalid text reverts to the value without a proposal.
    let count = h.read(|s| s.events.len());
    assert_eq!(commit(&mut h, "abc"), 3.);
    assert_eq!(commit(&mut h, "inf"), 3.);
    assert_eq!(h.read(|s| s.events.len()), count);
    assert_eq!(h.text(input(&h)), "3");
    assert!(
        h.read(|s| s.events.clone())
            .iter()
            .all(|e| e.phase == NumberPhase::Commit)
    );
}

#[test]
fn number_field_keys_step_and_page_within_the_range() {
    let mut h = H::new(Forms::default());
    focus_number(&mut h);
    assert!(h.key(KeyboardKey::ArrowUp));
    assert_eq!(h.read(|s| s.tab), 5.);
    assert!(h.key(KeyboardKey::ArrowDown));
    assert!(h.key(KeyboardKey::ArrowDown));
    assert_eq!(h.read(|s| s.tab), 3.);
    assert!(h.key(KeyboardKey::PageUp));
    assert_eq!(h.read(|s| s.tab), 13.);
    assert!(h.key(KeyboardKey::PageUp));
    assert_eq!(h.read(|s| s.tab), 16.);
    let count = h.read(|s| s.events.len());
    // At the end of the range a step proposes nothing.
    assert!(h.key(KeyboardKey::ArrowUp));
    assert_eq!(h.read(|s| s.events.len()), count);
    assert!(h.key(KeyboardKey::PageDown));
    assert!(h.key(KeyboardKey::PageDown));
    assert_eq!(h.read(|s| s.tab), 1.);
    assert!(
        h.read(|s| s.events.clone())
            .iter()
            .all(|e| e.phase == NumberPhase::Keyboard)
    );
    // A step starts from the typed text when it parses.
    h.select_all();
    h.type_text("9");
    assert!(h.key(KeyboardKey::ArrowUp));
    assert_eq!(h.read(|s| s.tab), 10.);
    assert_eq!(h.text(input(&h)), "10");
}

#[test]
fn enter_and_blur_commit_and_escape_reverts_the_draft() {
    let mut h = H::new(Forms::default());
    focus_number(&mut h);
    // Without a draft, Enter and Escape are left to the form.
    assert!(!h.key(KeyboardKey::Enter));
    assert!(!h.key(KeyboardKey::Escape));
    h.select_all();
    h.type_text("12");
    assert_eq!(h.text(input(&h)), "12");
    // Typing proposes nothing until it is committed.
    assert_eq!(h.read(|s| s.tab), 4.);
    assert!(h.key(KeyboardKey::Escape));
    assert_eq!(h.text(input(&h)), "4");
    assert_eq!(h.read(|s| s.tab), 4.);

    h.select_all();
    h.type_text("12");
    assert!(h.key(KeyboardKey::Enter));
    assert_eq!(h.read(|s| s.tab), 12.);

    // Leaving the field commits.
    h.select_all();
    h.type_text("6");
    let font = h.id("font");
    h.ui.focus(font);
    h.prepare();
    assert_eq!(h.read(|s| s.tab), 6.);
    // Leaving with invalid text reverts.
    focus_number(&mut h);
    h.select_all();
    h.type_text("x");
    h.ui.focus(font);
    h.prepare();
    assert_eq!(h.read(|s| s.tab), 6.);
    assert_eq!(h.text(input(&h)), "6");
}

#[test]
fn stepper_buttons_step_without_taking_focus_and_repeat_while_held() {
    let mut h = H::new(Forms::default());
    h.r.set_reduce_motion(true);
    h.frame(1_000);
    focus_number(&mut h);
    let up = h.within("tab", "increment");
    let down = h.within("tab", "decrement");
    h.click(up);
    assert_eq!(h.read(|s| s.tab), 5.);
    assert_eq!(h.ui.focused_element(), Some(input(&h)));
    h.click(down);
    h.click(down);
    assert_eq!(h.read(|s| s.tab), 3.);
    assert!(
        h.read(|s| s.events.clone())
            .iter()
            .all(|e| e.phase == NumberPhase::Step)
    );
    // The stepper is not a Tab stop and is hidden from assistive technology.
    assert!(h.ui.semantic_node(up).is_none());

    // Holding repeats after the delay, then every interval, until release.
    let at = h.center(up);
    h.pointer(PointerEvent::Moved(at));
    h.pointer(PointerEvent::Pressed(at));
    assert_eq!(h.read(|s| s.tab), 4.);
    assert_eq!(h.r.next_wakeup(), Some(Duration::from_millis(1_400)));
    h.frame(1_200);
    assert_eq!(h.read(|s| s.tab), 4.);
    h.frame(1_400);
    assert_eq!(h.read(|s| s.tab), 5.);
    h.frame(1_460);
    assert_eq!(h.read(|s| s.tab), 6.);
    h.pointer(PointerEvent::Released(at));
    assert_eq!(h.r.next_wakeup(), None);
    h.frame(2_000);
    assert_eq!(h.read(|s| s.tab), 6.);
}

#[test]
fn disabled_and_read_only_number_fields_reject_edits_and_steps() {
    for read_only in [false, true] {
        let mut h = H::new(Forms {
            disabled: !read_only,
            read_only,
            ..Default::default()
        });
        let node = h.ui.semantic_node(input(&h)).unwrap();
        assert_eq!(node.disabled, !read_only);
        // A disabled input also reports read-only, as it accepts no edits.
        assert!(node.read_only);
        assert!(!node.adjustable);
        if read_only {
            focus_number(&mut h);
            assert_eq!(h.ui.focused_element(), Some(input(&h)));
            assert!(!h.key(KeyboardKey::ArrowUp));
        }
        let up = h.within("tab", "increment");
        h.click(up);
        assert_eq!(h.read(|s| s.tab), 4.);
        assert!(h.read(|s| s.events.is_empty()));
    }
}

#[test]
fn number_fields_are_spin_buttons_with_a_numeric_value() {
    let mut h = H::new(Forms::default());
    let id = input(&h);
    let node = h.ui.semantic_node(id).unwrap();
    assert_eq!(node.role, SemanticRole::SpinButton);
    assert_eq!(
        node.numeric_value,
        Some(NumericValue {
            value: 4.,
            min: 1.,
            max: 16.,
            step: Some(1.),
        })
    );
    assert!(node.adjustable);
    assert_eq!(node.label, None);
    assert!(node.labelled_by.is_some());
    h.ui.semantic_action(
        &mut h.r,
        SemanticAction::SetNumericValue {
            target: id,
            value: 30.,
        },
        &mut Measure,
    )
    .unwrap();
    h.prepare();
    assert_eq!(
        h.read(|s| s.events.clone()),
        [NumberEvent {
            value: 16.,
            phase: NumberPhase::Accessibility
        }]
    );
}

struct Sections {
    expanded: bool,
    controlled: bool,
    toggles: Vec<bool>,
}
impl View for Sections {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut section = disclosure(
            cx,
            "Advanced",
            column().child(button("Inside").key("inside")).height(60.),
        )
        .key("advanced")
        .on_toggle(cx.listener(|s, e: &ToggleEvent, _| {
            s.toggles.push(e.checked);
            if s.controlled {
                s.expanded = e.checked;
            }
        }));
        if self.controlled {
            section = section.expanded(self.expanded);
        }
        column()
            .padding(10.)
            .child(section)
            .child(button("After").key("after"))
    }
}

#[test]
fn disclosures_toggle_controlled_and_uncontrolled() {
    for controlled in [false, true] {
        let mut h = H::new(Sections {
            expanded: false,
            controlled,
            toggles: Vec::new(),
        });
        let header = h.id("header");
        let node = h.ui.semantic_node(header).unwrap();
        assert_eq!(node.role, SemanticRole::Button);
        assert_eq!(node.expanded, Some(false));
        assert_eq!(node.controls, None);
        assert!(h.find("content").is_none());
        assert!(h.find("inside").is_none());

        h.click(header);
        assert_eq!(h.read(|s| s.toggles.clone()), [true]);
        let node = h.ui.semantic_node(header).unwrap();
        assert_eq!(node.expanded, Some(true));
        let content = h.id("content");
        assert_eq!(node.controls, Some(content));
        let region = h.ui.semantic_node(content).unwrap();
        assert_eq!(region.role, SemanticRole::Group);
        assert_eq!(
            h.ui.semantic_node(region.labelled_by.unwrap())
                .unwrap()
                .value,
            Some("Advanced")
        );
        // Without a frame clock the content shows at its natural height.
        assert_eq!(h.ui.element(content).unwrap().bounds.height, 60.);
        let after = h.ui.element(h.id("after")).unwrap().bounds;
        let header_box = h.ui.element(header).unwrap().bounds;
        assert_eq!(after.y, header_box.y + header_box.height + 60.);

        assert_eq!(h.ui.focused_element(), Some(header));
        h.ui.activate_focused(&mut h.r).unwrap();
        h.prepare();
        assert_eq!(h.read(|s| s.toggles.clone()), [true, false]);
        assert!(h.find("inside").is_none());
    }
    // A controlled disclosure follows the application, not its clicks.
    let mut h = H::new(Sections {
        expanded: true,
        controlled: true,
        toggles: Vec::new(),
    });
    h.update(|s| s.controlled = true);
    assert!(h.find("inside").is_some());
    h.update(|s| s.expanded = false);
    assert!(h.find("inside").is_none());
}

#[test]
fn disclosure_content_animates_its_height_and_is_inert_while_collapsing() {
    let mut h = H::new(Sections {
        expanded: false,
        controlled: true,
        toggles: Vec::new(),
    });
    h.frame(1_000);
    h.update(|s| s.expanded = true);
    let mut heights = Vec::new();
    for t in [1_016, 1_050, 1_100, 1_200, 1_300] {
        h.frame(t);
        heights.push(h.ui.element(h.id("content")).unwrap().bounds.height);
    }
    assert!(heights.windows(2).all(|w| w[0] <= w[1]), "{heights:?}");
    assert!(heights[1] > 0. && heights[1] < 60., "{heights:?}");
    assert_eq!(*heights.last().unwrap(), 60.);
    let inside = h.id("inside");
    assert!(h.ui.semantic_node(inside).is_some());

    // Collapsing hides the content from Tab and assistive technology at once.
    h.update(|s| s.expanded = false);
    // The motion starts on the next fresh frame.
    h.frame(1_350);
    h.frame(1_400);
    let content =
        h.ui.elements()
            .find(|e| e.key == Some(&Key::from("content")))
            .unwrap();
    assert!(content.inert);
    assert!(content.bounds.height > 0. && content.bounds.height < 60.);
    let inside =
        h.ui.elements()
            .find(|e| e.key == Some(&Key::from("inside")))
            .unwrap()
            .id;
    assert!(h.ui.semantic_node(inside).is_none());
    h.ui.focus_next(false);
    h.prepare();
    h.ui.focus_next(false);
    h.prepare();
    assert_ne!(h.ui.focused_element(), Some(inside));
    h.frame(1_600);
    assert!(
        h.ui.elements()
            .all(|e| e.key != Some(&Key::from("content")))
    );

    // Reduced motion shows the content at once.
    h.r.set_reduce_motion(true);
    h.update(|s| s.expanded = true);
    h.frame(1_700);
    assert_eq!(h.ui.element(h.id("content")).unwrap().bounds.height, 60.);
}

#[cfg(feature = "accessibility")]
#[test]
fn accesskit_publishes_spin_buttons_and_invalid_fields() {
    let mut h = H::new(Forms::default());
    h.update(|s| s.error = Some("Invalid".into()));
    let mut cache = AccessKitTree::new();
    let update = cache.update(&h.ui, "Forms", 1.).unwrap().unwrap();
    let (id, spin) = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == accesskit::Role::SpinButton)
        .unwrap();
    assert_eq!(spin.numeric_value(), Some(4.));
    assert_eq!(
        (spin.min_numeric_value(), spin.max_numeric_value()),
        (Some(1.), Some(16.))
    );
    assert_eq!(spin.invalid(), Some(accesskit::Invalid::True));
    assert_eq!(spin.description(), Some("Invalid"));
    assert!(spin.supports_action(accesskit::Action::Increment));
    let request = accesskit::ActionRequest {
        action: accesskit::Action::Increment,
        target_tree: accesskit::TreeId::ROOT,
        target_node: *id,
        data: None,
    };
    assert!(matches!(
        cache.action(request),
        Some(SemanticAction::SetNumericValue { value: 5., .. })
    ));
}
