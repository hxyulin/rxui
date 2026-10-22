use crate::*;
use std::time::Duration;

struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.chars().count() as f32 * 8., 20.])
    }
}

struct Form {
    agree: Checked,
    wifi: bool,
    disabled: bool,
    size: Option<Key>,
    volume: f32,
    volume_max: f32,
    progress: Option<f32>,
    animate: bool,
    spinner: bool,
    language: Option<Key>,
    language_options: Option<Vec<Choice>>,
    toggles: Vec<ToggleEvent>,
    choices: Vec<Key>,
    slides: Vec<SliderEvent>,
    saves: u32,
}
impl Default for Form {
    fn default() -> Self {
        Self {
            agree: Checked::False,
            wifi: false,
            disabled: false,
            size: Some(Key::from("s")),
            volume: 50.,
            volume_max: 100.,
            progress: Some(0.25),
            animate: false,
            spinner: false,
            language: None,
            language_options: None,
            toggles: Vec::new(),
            choices: Vec::new(),
            slides: Vec::new(),
            saves: 0,
        }
    }
}
impl View for Form {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut size = radio_group()
            .key("size")
            .accessibility_label("Size")
            .option(choice("s", "Small"))
            .option(choice("m", "Medium").disabled(true))
            .option(choice("l", "Large"))
            .on_change(cx.listener(|s, e: &ChoiceEvent, _| {
                s.size = Some(e.key.clone());
                s.choices.push(e.key.clone());
            }));
        if let Some(key) = &self.size {
            size = size.selected(key.clone());
        }
        let mut language = select(cx)
            .key("language")
            .accessibility_label("Language")
            .placeholder("Choose")
            .options(self.language_options.clone().unwrap_or_else(|| {
                vec![
                    choice("apple", "Apple"),
                    choice("banana", "Banana"),
                    choice("blueberry", "Blueberry").disabled(true),
                    choice("cherry", "Cherry"),
                ]
            }))
            .on_change(cx.listener(|s, e: &ChoiceEvent, _| {
                s.language = Some(e.key.clone());
                s.choices.push(e.key.clone());
            }));
        if let Some(key) = &self.language {
            language = language.selected(key.clone());
        }
        let mut progress = progress_bar(self.progress).key("progress");
        if self.animate {
            progress = progress.animate(cx);
        }
        let mut root = column()
            .padding(10.)
            .gap(10.)
            .child(
                checkbox(self.agree, "Agree")
                    .key("agree")
                    .disabled(self.disabled)
                    .on_change(cx.listener(|s, e: &ToggleEvent, _| {
                        s.agree = e.checked.into();
                        s.toggles.push(*e);
                    })),
            )
            .child(
                switch(self.wifi, "Wi-Fi")
                    .key("wifi")
                    .on_change(cx.listener(|s, e: &ToggleEvent, _| s.wifi = e.checked)),
            )
            .child(size)
            .child(
                slider(self.volume)
                    .key("volume")
                    .range(0., self.volume_max)
                    .step(5.)
                    .width(220.)
                    .accessibility_label("Volume")
                    .on_change(cx.listener(|s, e: &SliderEvent, _| {
                        s.volume = e.value;
                        s.slides.push(*e);
                    })),
            )
            .child(progress)
            .child(language)
            .child(
                tooltip(
                    cx,
                    button("Save")
                        .key("save")
                        .on_click(cx.listener(|s, _, _| s.saves += 1)),
                    "Save the document",
                )
                .key("save-tip"),
            );
        if self.spinner {
            root = root.child(spinner(cx).key("spinner"));
        }
        root
    }
}

struct Harness {
    r: Runtime,
    e: Entity<Form>,
    ui: Ui<Form>,
}
impl Harness {
    fn new(form: Form) -> Self {
        let mut r = Runtime::new();
        let e = r.update(|cx| cx.new(|_| form));
        let mut ui = Ui::new(&mut r, e.clone()).unwrap();
        ui.prepare(&mut r, [600., 800.], &mut Measure).unwrap();
        Self { r, e, ui }
    }
    fn prepare(&mut self) {
        self.ui
            .prepare(&mut self.r, [600., 800.], &mut Measure)
            .unwrap();
    }
    fn update(&mut self, f: impl FnOnce(&mut Form)) {
        self.r.update(|cx| self.e.update(cx, |s, _| f(s)));
        self.prepare();
    }
    fn read<R>(&mut self, f: impl FnOnce(&Form) -> R) -> R {
        let e = self.e.clone();
        self.r.update(|cx| f(&e.read(cx)))
    }
    fn find(&self, key: &str) -> Option<ElementId> {
        self.ui
            .elements()
            .find(|e| e.key == Some(&Key::from(key)))
            .map(|e| e.id)
    }
    /// Live element with `key`, excluding the inert copy of a closing overlay that
    /// lingers while it fades out.
    fn shown(&self, key: &str) -> Option<ElementId> {
        self.find(key)
            .filter(|&id| !self.ui.element(id).is_some_and(|e| e.inert))
    }
    fn id(&self, key: &str) -> ElementId {
        self.find(key).unwrap_or_else(|| panic!("no element {key}"))
    }
    /// The element with `key` inside `root`'s subtree.
    fn within(&self, root: &str, key: &str) -> ElementId {
        let root = self.id(root);
        self.ui
            .elements()
            .find(|e| {
                e.key == Some(&Key::from(key)) && {
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
            .unwrap()
            .id
    }
    fn bounds(&self, id: ElementId) -> Bounds {
        self.ui.element(id).unwrap().bounds
    }
    fn center(&self, id: ElementId) -> [f32; 2] {
        let b = self.bounds(id);
        [b.x + b.width / 2., b.y + b.height / 2.]
    }
    fn pointer(&mut self, event: PointerEvent) {
        self.ui.pointer(&mut self.r, event).unwrap();
        self.prepare();
    }
    fn click(&mut self, at: [f32; 2]) {
        self.pointer(PointerEvent::Moved(at));
        self.pointer(PointerEvent::Pressed(at));
        self.pointer(PointerEvent::Released(at));
    }
    fn key(&mut self, key: KeyboardKey) -> InputResult {
        let result = self
            .ui
            .key(
                &mut self.r,
                KeyEvent {
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
    /// A key followed by the native host's Enter/Space activation default.
    fn activation_key(&mut self, key: KeyboardKey) {
        if !self.key(key).default_prevented {
            self.ui.activate_focused(&mut self.r).unwrap();
            self.prepare();
        }
    }
    fn action(&mut self, action: SemanticAction) -> bool {
        let handled = self
            .ui
            .semantic_action(&mut self.r, action, &mut Measure)
            .unwrap();
        self.prepare();
        handled
    }
    fn frame(&mut self, millis: u64) {
        self.r.begin_frame(Duration::from_millis(millis));
        self.prepare();
    }
}

#[test]
fn check_boxes_and_switches_propose_toggles_from_pointer_keyboard_and_assistive_input() {
    let mut h = Harness::new(Form::default());
    let agree = h.id("agree");
    let node = h.ui.semantic_node(agree).unwrap();
    assert_eq!(node.role, SemanticRole::CheckBox);
    assert_eq!(node.label, Some("Agree"));
    assert_eq!(node.checked, Some(Checked::False));
    assert!(node.focusable && node.activatable);

    h.click(h.center(agree));
    assert_eq!(h.read(|s| s.agree), Checked::True);
    assert_eq!(
        h.ui.semantic_node(agree).unwrap().checked,
        Some(Checked::True)
    );
    let indicator = h.within("agree", "indicator");
    assert_eq!(
        h.ui.element(indicator).unwrap().background,
        Some(Theme::dark().palette().accent)
    );

    // Space on the focused box uses the host's ordinary activation default.
    assert_eq!(h.ui.focused_element(), Some(agree));
    h.activation_key(KeyboardKey::Space);
    assert_eq!(h.read(|s| s.agree), Checked::False);

    // Mixed is published as mixed and activates to checked.
    h.update(|s| s.agree = Checked::Mixed);
    assert_eq!(
        h.ui.semantic_node(agree).unwrap().checked,
        Some(Checked::Mixed)
    );
    assert!(h.action(SemanticAction::Activate(agree)));
    assert_eq!(
        h.read(|s| s.toggles.last().copied()),
        Some(ToggleEvent { checked: true })
    );

    // Disabled controls neither change nor stay in Tab traversal.
    h.update(|s| s.disabled = true);
    let before = h.read(|s| s.toggles.len());
    assert!(h.ui.semantic_node(agree).unwrap().disabled);
    h.click(h.center(agree));
    assert!(!h.action(SemanticAction::Activate(agree)));
    assert_eq!(h.read(|s| s.toggles.len()), before);
    h.ui.focus(h.id("wifi"));
    h.ui.focus_next(true);
    assert_ne!(h.ui.focused_element(), Some(agree));

    let wifi = h.id("wifi");
    assert_eq!(h.ui.semantic_node(wifi).unwrap().role, SemanticRole::Switch);
    assert_eq!(
        h.ui.semantic_node(wifi).unwrap().checked,
        Some(Checked::False)
    );
    h.click(h.center(wifi));
    assert!(h.read(|s| s.wifi));
    assert_eq!(
        h.ui.semantic_node(wifi).unwrap().checked,
        Some(Checked::True)
    );
}

#[test]
fn radio_groups_rove_one_tab_stop_and_arrow_keys_skip_disabled_options_and_wrap() {
    let mut h = Harness::new(Form::default());
    let [small, medium, large] = ["s", "m", "l"].map(|k| h.id(k));
    let node = h.ui.semantic_node(large).unwrap();
    assert_eq!(node.role, SemanticRole::RadioButton);
    assert_eq!(node.checked, Some(Checked::False));
    assert_eq!((node.set_size, node.position_in_set), (Some(3), Some(2)));
    assert_eq!(
        h.ui.semantic_node(small).unwrap().checked,
        Some(Checked::True)
    );
    assert!(h.ui.semantic_node(medium).unwrap().disabled);
    assert_eq!(
        h.ui.semantic_node(h.id("size")).unwrap().role,
        SemanticRole::Group
    );

    // Tab from the switch reaches the selected option only.
    h.ui.focus(h.id("wifi"));
    h.ui.focus_next(false);
    assert_eq!(h.ui.focused_element(), Some(small));
    h.ui.focus_next(false);
    assert_eq!(h.ui.focused_element(), Some(h.id("volume")));

    h.ui.focus(small);
    assert!(h.key(KeyboardKey::ArrowDown).default_prevented);
    assert_eq!(h.read(|s| s.size.clone()), Some(Key::from("l")));
    assert_eq!(h.ui.focused_element(), Some(large));
    assert_eq!(
        h.ui.semantic_node(large).unwrap().checked,
        Some(Checked::True)
    );
    h.key(KeyboardKey::ArrowRight);
    assert_eq!(h.read(|s| s.size.clone()), Some(Key::from("s")));
    assert_eq!(h.ui.focused_element(), Some(small));
    h.key(KeyboardKey::ArrowUp);
    assert_eq!(h.ui.focused_element(), Some(large));
    assert_eq!(
        h.read(|s| s.choices.clone()),
        ["l", "s", "l"].map(Key::from).to_vec()
    );

    // With no selection the first enabled option is the Tab stop; a click selects.
    h.update(|s| s.size = None);
    h.ui.focus(h.id("wifi"));
    h.ui.focus_next(false);
    assert_eq!(h.ui.focused_element(), Some(small));
    h.click(h.center(large));
    assert_eq!(h.read(|s| s.size.clone()), Some(Key::from("l")));
    h.click(h.center(medium));
    assert_eq!(h.read(|s| s.size.clone()), Some(Key::from("l")));
}

#[test]
fn sliders_drag_with_capture_step_by_keys_and_accept_assistive_values() {
    let mut h = Harness::new(Form::default());
    let volume = h.id("volume");
    let node = h.ui.semantic_node(volume).unwrap();
    assert_eq!(node.role, SemanticRole::Slider);
    assert_eq!(node.label, Some("Volume"));
    assert!(node.adjustable);
    assert_eq!(
        node.numeric_value,
        Some(NumericValue {
            value: 50.,
            min: 0.,
            max: 100.,
            step: Some(5.)
        })
    );
    let b = h.bounds(volume);
    let y = b.y + b.height / 2.;
    // The rail starts 10 units inside the 220-unit border box and spans 200.
    h.pointer(PointerEvent::Pressed([b.x + 10., y]));
    assert_eq!(h.ui.captured_pointer(), Some(volume));
    assert_eq!(h.ui.focused_element(), Some(volume));
    assert_eq!(h.read(|s| s.volume), 0.);
    h.pointer(PointerEvent::Moved([b.x + 10. + 151., y + 80.]));
    assert_eq!(h.read(|s| s.volume), 75.);
    h.pointer(PointerEvent::Moved([b.x + 500., y]));
    assert_eq!(h.read(|s| s.volume), 100.);
    h.pointer(PointerEvent::Released([b.x + 10. + 151., y]));
    assert_eq!(h.ui.captured_pointer(), None);
    assert_eq!(
        h.read(|s| s.slides.iter().map(|e| e.phase).collect::<Vec<_>>()),
        [
            SliderPhase::Begin,
            SliderPhase::Drag,
            SliderPhase::Drag,
            SliderPhase::End
        ]
    );
    assert_eq!(h.read(|s| s.volume), 75.);
    let thumb = h.within("volume", "thumb");
    let center = h.center(thumb)[0];
    assert!(
        (center - (b.x + 10. + 150.)).abs() < 0.5,
        "thumb at {center}"
    );

    for (key, value) in [
        (KeyboardKey::ArrowRight, 80.),
        (KeyboardKey::ArrowDown, 75.),
        (KeyboardKey::PageUp, 85.),
        (KeyboardKey::End, 100.),
        (KeyboardKey::Home, 0.),
        (KeyboardKey::PageDown, 0.),
    ] {
        assert!(h.key(key.clone()).default_prevented, "{key:?}");
        assert_eq!(h.read(|s| s.volume), value, "{key:?}");
    }
    assert_eq!(
        h.read(|s| s.slides.last().unwrap().phase),
        SliderPhase::Keyboard
    );

    assert!(h.action(SemanticAction::SetNumericValue {
        target: volume,
        value: 42.
    }));
    assert_eq!(h.read(|s| s.volume), 40.);
    assert_eq!(
        h.read(|s| s.slides.last().unwrap().phase),
        SliderPhase::Accessibility
    );

    // Escape cancels the captured drag; the application decides about rollback.
    h.pointer(PointerEvent::Pressed([b.x + 110., y]));
    h.key(KeyboardKey::Escape);
    assert_eq!(h.ui.captured_pointer(), None);
    assert_eq!(
        h.read(|s| s.slides.last().copied()),
        Some(SliderEvent {
            value: 50.,
            phase: SliderPhase::Cancel
        })
    );
    h.pointer(PointerEvent::Released([b.x + 110., y]));

    h.r.update(|cx| h.e.update(cx, |s, _| s.volume_max = -1.));
    assert!(matches!(
        h.ui.prepare(&mut h.r, [600., 800.], &mut Measure),
        Err(UiError::InvalidStyle)
    ));
    h.update(|s| s.volume_max = 100.);
}

#[test]
fn progress_bars_publish_values_and_indeterminate_bars_and_spinners_animate_by_frame() {
    let mut h = Harness::new(Form::default());
    let progress = h.id("progress");
    let node = h.ui.semantic_node(progress).unwrap();
    assert_eq!(node.role, SemanticRole::ProgressIndicator);
    assert_eq!(node.numeric_value.unwrap().value, 0.25);
    let fill = h.within("progress", "fill");
    assert!((h.bounds(fill).width - 198. * 0.25).abs() < 0.01);
    assert!(!h.r.animation_frame_requested());

    h.update(|s| s.progress = None);
    let node = h.ui.semantic_node(progress).unwrap();
    assert_eq!(node.numeric_value, None);
    assert_eq!(node.value, Some("Indeterminate"));
    assert!(!h.r.animation_frame_requested());
    h.update(|s| s.animate = true);
    assert!(h.r.animation_frame_requested());
    let fill = h.within("progress", "fill");
    let first = h.bounds(fill).x;
    h.frame(400);
    assert!(h.r.animation_frame_requested());
    assert!(h.bounds(fill).x > first);

    h.update(|s| {
        s.progress = Some(2.);
        s.spinner = true;
    });
    assert_eq!(
        h.ui.semantic_node(progress)
            .unwrap()
            .numeric_value
            .unwrap()
            .value,
        1.
    );
    let spinner = h.id("spinner");
    let node = h.ui.semantic_node(spinner).unwrap();
    assert_eq!(node.role, SemanticRole::ProgressIndicator);
    assert_eq!(node.label, Some("Loading"));
    assert!(h.r.animation_frame_requested());
    h.update(|s| s.spinner = false);
    h.frame(800);
    assert!(!h.r.animation_frame_requested());
}

#[test]
fn selects_open_a_listbox_keep_focus_and_choose_with_keys_type_ahead_and_pointer() {
    let mut h = Harness::new(Form::default());
    let trigger = h.within("language", "button");
    let node = h.ui.semantic_node(trigger).unwrap();
    assert_eq!(node.role, SemanticRole::ComboBox);
    assert_eq!(node.label, Some("Language"));
    assert_eq!(node.expanded, Some(false));
    assert_eq!(node.value, Some(""));
    assert!(h.find("list").is_none());

    h.click(h.center(trigger));
    assert_eq!(h.ui.semantic_node(trigger).unwrap().expanded, Some(true));
    let list = h.id("list");
    assert!(h.ui.element(list).unwrap().viewport_overlay);
    assert_eq!(h.ui.focused_element(), Some(trigger));
    let apple = h.id("apple");
    assert_eq!(
        h.ui.semantic_node(apple).unwrap().role,
        SemanticRole::ListBoxOption
    );
    let selection = Theme::dark().palette().selection;
    assert_eq!(h.ui.element(apple).unwrap().background, Some(selection));

    // Arrows skip the disabled option and stop at the ends.
    for _ in 0..4 {
        h.key(KeyboardKey::ArrowDown);
    }
    assert_eq!(
        h.ui.element(h.id("cherry")).unwrap().background,
        Some(selection)
    );
    h.key(KeyboardKey::ArrowUp);
    assert!(h.key(KeyboardKey::Enter).default_prevented);
    assert_eq!(h.read(|s| s.language.clone()), Some(Key::from("banana")));
    assert!(h.find("list").is_none());
    assert_eq!(h.ui.focused_element(), Some(trigger));
    let node = h.ui.semantic_node(trigger).unwrap();
    assert_eq!((node.value, node.expanded), (Some("Banana"), Some(false)));

    // Closed type-ahead chooses directly, skipping disabled options.
    h.key(KeyboardKey::Character("b".into()));
    assert_eq!(h.read(|s| s.language.clone()), Some(Key::from("banana")));
    h.key(KeyboardKey::Character("C".into()));
    assert_eq!(h.read(|s| s.language.clone()), Some(Key::from("cherry")));

    // Down opens on the selection; Escape closes without choosing.
    h.key(KeyboardKey::ArrowDown);
    assert_eq!(
        h.ui.element(h.id("cherry")).unwrap().background,
        Some(selection)
    );
    h.key(KeyboardKey::Character("a".into()));
    assert_eq!(
        h.ui.element(h.id("apple")).unwrap().background,
        Some(selection)
    );
    h.key(KeyboardKey::Escape);
    assert!(h.find("list").is_none());
    assert_eq!(h.read(|s| s.language.clone()), Some(Key::from("cherry")));

    // Enter/Space on the closed button open it through ordinary activation.
    h.activation_key(KeyboardKey::Enter);
    assert!(h.find("list").is_some());
    h.key(KeyboardKey::Tab);
    assert!(h.find("list").is_none());

    // Hover highlights and a click chooses; disabled options ignore both.
    h.click(h.center(trigger));
    let blueberry = h.id("blueberry");
    assert!(h.ui.semantic_node(blueberry).is_some());
    h.click(h.center(blueberry));
    assert_eq!(h.read(|s| s.language.clone()), Some(Key::from("cherry")));
    if h.find("list").is_none() {
        h.click(h.center(trigger));
    }
    let apple = h.id("apple");
    h.pointer(PointerEvent::Moved(h.center(apple)));
    assert_eq!(h.ui.element(apple).unwrap().background, Some(selection));
    h.click(h.center(apple));
    assert_eq!(h.read(|s| s.language.clone()), Some(Key::from("apple")));
    assert!(h.find("list").is_none());

    // An outside press closes without choosing or activating what lies beneath.
    h.click(h.center(trigger));
    let toggles = h.read(|s| s.toggles.len());
    h.click(h.center(h.id("agree")));
    assert!(h.find("list").is_none());
    assert_eq!(h.read(|s| s.toggles.len()), toggles);
    assert_eq!(
        h.read(|s| s.choices.clone()),
        ["banana", "cherry", "apple"].map(Key::from).to_vec()
    );
}

#[test]
fn tooltips_appear_after_the_hover_delay_without_focus_or_blocking_input() {
    let mut h = Harness::new(Form::default());
    let save = h.id("save");
    assert_eq!(
        h.ui.semantic_node(save).unwrap().description,
        Some("Save the document")
    );
    h.ui.focus(h.id("agree"));
    let focus = h.ui.focused_element();
    h.pointer(PointerEvent::Moved(h.center(save)));
    assert!(h.shown("tooltip").is_none());
    assert!(h.r.animation_frame_requested());
    h.frame(1_000);
    h.frame(1_000 + TOOLTIP_DELAY.as_millis() as u64 / 2);
    assert!(h.shown("tooltip").is_none());
    h.frame(1_000 + TOOLTIP_DELAY.as_millis() as u64);
    let tip = h.id("tooltip");
    let info = h.ui.element(tip).unwrap();
    assert!(info.viewport_overlay);
    assert_eq!(info.pointer_events, PointerEvents::None);
    assert!(h.ui.semantic_node(tip).is_none());
    assert_eq!(h.ui.focused_element(), focus);
    // Its enter motion and the hover transition settle, then frames stop.
    h.frame(1_000 + TOOLTIP_DELAY.as_millis() as u64 + 1_000);
    assert!(h.shown("tooltip").is_some());
    assert!(!h.r.animation_frame_requested() && !h.ui.is_animating());

    // The press reaches the button and hides the tooltip until the pointer returns.
    h.click(h.center(save));
    assert_eq!(h.read(|s| s.saves), 1);
    assert!(h.shown("tooltip").is_none());
    h.frame(5_000);
    assert!(h.shown("tooltip").is_none());

    h.pointer(PointerEvent::Moved([590., 790.]));
    h.pointer(PointerEvent::Moved(h.center(save)));
    h.frame(6_000);
    h.frame(7_000);
    assert!(h.shown("tooltip").is_some());
    h.key(KeyboardKey::Escape);
    assert!(h.shown("tooltip").is_none());

    h.pointer(PointerEvent::Moved([590., 790.]));
    h.pointer(PointerEvent::Moved(h.center(save)));
    h.frame(8_000);
    h.frame(9_000);
    assert!(h.shown("tooltip").is_some());
    h.pointer(PointerEvent::Moved([590., 790.]));
    assert!(h.shown("tooltip").is_none());
}

struct Wrapped;
impl View for Wrapped {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        row()
            .width(300.)
            .child(tooltip(
                cx,
                button("Grow").key("grow").flex_grow(1.).margin_x(5.),
                "Grows",
            ))
            .child(tooltip(
                cx,
                button("Fixed").key("fixed").disabled(true),
                "Unavailable",
            ))
    }
}
#[test]
fn tooltip_wrappers_keep_the_target_flex_item_layout_and_cover_disabled_targets() {
    let mut r = Runtime::new();
    let e = r.update(|cx| cx.new(|_| Wrapped));
    let mut ui = Ui::new(&mut r, e).unwrap();
    ui.prepare(&mut r, [400., 100.], &mut Measure).unwrap();
    let find = |key: &str| {
        ui.elements()
            .find(|e| e.key == Some(&Key::from(key)))
            .unwrap()
            .bounds
    };
    let (grow, fixed) = (find("grow"), find("fixed"));
    assert_eq!(grow.x, 5.);
    assert!((grow.x + grow.width + 5. - fixed.x).abs() < 0.01);
    assert!((fixed.x + fixed.width - 300.).abs() < 0.01);
    ui.pointer(&mut r, PointerEvent::Moved([fixed.x + 4., fixed.y + 4.]))
        .unwrap();
    for millis in [0, 1_000, 2_000] {
        r.begin_frame(Duration::from_millis(millis));
        ui.prepare(&mut r, [400., 100.], &mut Measure).unwrap();
    }
    let tip = ui
        .elements()
        .find(|e| e.key == Some(&Key::from("tooltip")))
        .unwrap()
        .id;
    let text = ui
        .elements()
        .find(|e| {
            e.parent
                .is_some_and(|p| ui.element(p).unwrap().parent == Some(tip))
        })
        .unwrap();
    assert_eq!(text.text, Some("Unavailable"));
}

#[test]
fn indicator_colors_meet_non_text_contrast_in_every_preset() {
    fn luminance(c: Color) -> f32 {
        c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
    }
    fn contrast(a: Color, b: Color) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        let c = theme.palette();
        // Checked boxes, selected radios, switches and filled tracks. Dark's accent
        // on the Raised popover fill is 2.96:1, so Raised is not part of the target.
        for surface in [c.background, c.surface] {
            assert!(contrast(c.accent, surface) >= 3.);
        }
        // Unchecked indicators, rails and thumbs are outlined like text inputs.
        assert!(contrast(c.input_border, c.input) >= 3.);
        assert!(contrast(c.input_border, c.surface) >= 3.);
        // The fill stays distinct from an outlined rail's interior.
        assert!(contrast(c.accent, c.input) >= 3.);
        // Marks on a checked indicator and the select highlight.
        assert!(contrast(c.accent_text, c.accent) >= 4.5);
        assert!(contrast(c.selection_text, c.selection) >= 7.);
    }
}

#[cfg(feature = "accessibility")]
#[test]
fn standard_controls_publish_accesskit_roles_states_and_actions() {
    use accesskit::{Action, ActionRequest, Role, Toggled, TreeId};
    let mut h = Harness::new(Form {
        agree: Checked::Mixed,
        ..Form::default()
    });
    let mut cache = AccessKitTree::new();
    let update = cache.update(&h.ui, "Form", 1.).unwrap().unwrap();
    let node = |name: &str| {
        update
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some(name))
            .unwrap_or_else(|| panic!("no node {name}"))
            .clone()
    };
    let (agree, check) = node("Agree");
    assert_eq!(check.role(), Role::CheckBox);
    assert_eq!(check.toggled(), Some(Toggled::Mixed));
    assert!(check.supports_action(Action::Click));
    let (_, wifi) = node("Wi-Fi");
    assert_eq!(
        (wifi.role(), wifi.toggled()),
        (Role::Switch, Some(Toggled::False))
    );
    let (_, small) = node("Small");
    assert_eq!(
        (small.role(), small.toggled()),
        (Role::RadioButton, Some(Toggled::True))
    );
    let (volume, slider) = node("Volume");
    assert_eq!(slider.role(), Role::Slider);
    assert_eq!(slider.numeric_value_step(), Some(5.));
    assert!(slider.supports_action(Action::Increment));
    assert!(slider.supports_action(Action::SetValue));
    let (_, combo) = node("Language");
    assert_eq!(
        (combo.role(), combo.is_expanded()),
        (Role::ComboBox, Some(false))
    );
    let request = |target, action| ActionRequest {
        target_tree: TreeId::ROOT,
        target_node: target,
        action,
        data: None,
    };
    for action in [
        cache.action(request(volume, Action::Increment)).unwrap(),
        cache.action(request(agree, Action::Click)).unwrap(),
    ] {
        h.action(action);
    }
    assert_eq!(h.read(|s| (s.volume, s.agree)), (55., Checked::True));
}

#[test]
fn progress_reduced_motion_stops_requests_and_resumes_when_disabled() {
    let mut h = Harness::new(Form {
        progress: None,
        animate: true,
        spinner: true,
        ..Form::default()
    });
    assert!(h.r.animation_frame_requested());
    h.r.set_reduce_motion(true);
    h.frame(400);
    assert!(!h.r.animation_frame_requested());
    let fill = h.within("progress", "fill");
    let first = h.bounds(fill);
    h.frame(800);
    assert_eq!(first, h.bounds(fill));
    assert!(!h.r.animation_frame_requested());
    assert_eq!(
        h.ui.semantic_node(h.id("progress")).unwrap().value,
        Some("Indeterminate")
    );
    assert_eq!(
        h.ui.semantic_node(h.id("spinner")).unwrap().label,
        Some("Loading")
    );
    h.r.update(|cx| cx.set_reduce_motion(false));
    h.prepare();
    h.frame(1200);
    assert!(h.r.animation_frame_requested());
    assert_ne!(first.x, h.bounds(fill).x);
}

#[test]
fn select_reveals_selected_and_keyboard_highlights_and_tracks_keys_through_mutation() {
    let mut h = Harness::new(Form {
        language: Some(Key::from("item-35")),
        language_options: Some(
            (0..40)
                .map(|i| choice(format!("item-{i}"), format!("Item {i}")))
                .collect(),
        ),
        ..Form::default()
    });
    let trigger = h.within("language", "button");
    h.click(h.center(trigger));
    let visible = |h: &Harness, key: &str| {
        let node = h.ui.semantic_node(h.id(key)).unwrap();
        assert!(
            node.bounds.y >= node.clip_bounds.y - 0.01,
            "{key}: {node:?}"
        );
        assert!(
            node.bounds.y + node.bounds.height
                <= node.clip_bounds.y + node.clip_bounds.height + 0.01,
            "{key}: {node:?}"
        );
        assert_eq!(
            h.ui.semantic_node(trigger).unwrap().active_descendant,
            Some(node.id)
        );
    };
    visible(&h, "item-35");
    h.ui.prepare(&mut h.r, [600., 400.], &mut Measure).unwrap();
    visible(&h, "item-35");
    h.prepare();
    visible(&h, "item-35");
    #[cfg(feature = "accessibility")]
    let mut native = AccessKitTree::new();
    #[cfg(feature = "accessibility")]
    let native_trigger = {
        let first = native.update(&h.ui, "Select", 1.).unwrap().unwrap();
        let (id, trigger) = first
            .nodes
            .iter()
            .find(|(_, n)| n.role() == accesskit::Role::ComboBox)
            .unwrap();
        let active = trigger.active_descendant().unwrap();
        assert_eq!(
            first
                .nodes
                .iter()
                .find(|(id, _)| *id == active)
                .unwrap()
                .1
                .label(),
            Some("Item 35")
        );
        let list = trigger.controls()[0];
        let popup = &first.nodes.iter().find(|(id, _)| *id == list).unwrap().1;
        assert_eq!(popup.role(), accesskit::Role::ListBox);
        assert!(popup.children().contains(&active));
        let result = *id;
        let _consumer = accesskit_consumer::Tree::new(first, true);
        result
    };
    h.key(KeyboardKey::End);
    visible(&h, "item-39");
    #[cfg(feature = "accessibility")]
    {
        let delta = native.update(&h.ui, "Select", 1.).unwrap().unwrap();
        let trigger = &delta
            .nodes
            .iter()
            .find(|(id, _)| *id == native_trigger)
            .unwrap()
            .1;
        assert!(trigger.active_descendant().is_some());
    }
    let popup = h.ui.semantic_node(h.id("list")).unwrap().children[0];
    assert!(h.action(SemanticAction::Scroll {
        target: popup,
        offset: [0., 0.]
    }));
    assert_eq!(h.ui.semantic_node(popup).unwrap().scroll_offset[1], 0.);
    h.prepare();
    assert_eq!(h.ui.semantic_node(popup).unwrap().scroll_offset[1], 0.);
    // An unrelated evaluation must preserve the user's scroll position too.
    h.update(|s| s.language = Some(Key::from("item-35")));
    assert_eq!(h.ui.semantic_node(popup).unwrap().scroll_offset[1], 0.);
    h.key(KeyboardKey::Home);
    visible(&h, "item-0");
    for _ in 0..28 {
        h.key(KeyboardKey::ArrowDown);
    }
    visible(&h, "item-28");
    assert_eq!(h.ui.focused_element(), Some(trigger));
    h.update(|s| s.language_options.as_mut().unwrap().reverse());
    assert_eq!(
        h.ui.semantic_node(trigger).unwrap().active_descendant,
        Some(h.id("item-28"))
    );
    visible(&h, "item-28");
    h.update(|s| {
        s.language_options
            .as_mut()
            .unwrap()
            .retain(|o| o.key != Key::from("item-28"))
    });
    visible(&h, "item-35");
    h.update(|s| {
        let option = s
            .language_options
            .as_mut()
            .unwrap()
            .iter_mut()
            .find(|o| o.key == Key::from("item-35"))
            .unwrap();
        option.disabled = true;
    });
    assert!(h.ui.semantic_node(h.id("item-35")).unwrap().disabled);
    visible(&h, "item-39");
    h.update(|s| {
        for option in s.language_options.as_mut().unwrap() {
            option.disabled = true;
        }
    });
    assert_eq!(h.ui.semantic_node(trigger).unwrap().active_descendant, None);
    h.update(|s| s.language_options = Some(vec![]));
    assert_eq!(h.ui.semantic_node(trigger).unwrap().active_descendant, None);
    h.key(KeyboardKey::ArrowUp);
    h.key(KeyboardKey::Enter);
    assert_eq!(h.ui.semantic_node(trigger).unwrap().active_descendant, None);
    #[cfg(feature = "accessibility")]
    {
        let delta = native.update(&h.ui, "Select", 1.).unwrap().unwrap();
        let trigger = &delta
            .nodes
            .iter()
            .find(|(id, _)| *id == native_trigger)
            .unwrap()
            .1;
        assert_eq!(trigger.active_descendant(), None);
        assert!(trigger.controls().is_empty());
    }
}

#[test]
fn select_timed_prefix_search_continues_current_match_and_repeated_letters_cycle() {
    let mut h = Harness::new(Form {
        language_options: Some(vec![
            choice("a", "Apple"),
            choice("ap", "Apricot"),
            choice("b", "Banana"),
            choice("bl", "Blueberry"),
            choice("p", "Pear"),
        ]),
        ..Form::default()
    });
    let trigger = h.within("language", "button");
    h.click(h.center(trigger));
    h.key(KeyboardKey::Character("b".into()));
    h.key(KeyboardKey::Character("l".into()));
    assert_eq!(
        h.ui.semantic_node(trigger).unwrap().active_descendant,
        Some(h.id("bl"))
    );
    h.key(KeyboardKey::Enter);
    assert_eq!(h.read(|s| s.language.clone()), Some(Key::from("bl")));
    h.key(KeyboardKey::Character("a".into()));
    h.key(KeyboardKey::Character("a".into()));
    assert_eq!(h.read(|s| s.language.clone()), Some(Key::from("ap")));
    // The next character must start a fresh prefix after the timeout. With a
    // stale "a" prefix, "p" would keep Apricot; a fresh "p" chooses Pear.
    std::thread::sleep(Duration::from_millis(1050));
    h.key(KeyboardKey::Character("p".into()));
    assert_eq!(h.read(|s| s.language.clone()), Some(Key::from("p")));
}

#[test]
fn tooltip_deadline_wait_is_idle_and_cancels_on_leave_suppression_and_unmount() {
    struct Page {
        show: bool,
    }
    impl View for Page {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let mut root = column();
            if self.show {
                root = root.child(tooltip(cx, button("Hover").key("hover"), "Help"));
            }
            root.children((0..1000).map(|i| label("Row").key(i)))
        }
    }
    let ms = Duration::from_millis;
    let mut r = Runtime::new();
    r.set_reduce_motion(true); // Isolate tooltip waiting from hover paint transitions.
    r.begin_frame(Duration::ZERO);
    let page = r.update(|cx| cx.new(|_| Page { show: true }));
    let mut ui = Ui::new(&mut r, page.clone()).unwrap();
    ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    ui.pointer(&mut r, PointerEvent::Moved([5., 5.])).unwrap();
    ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    assert!(r.animation_frame_requested()); // Need fresh entry time.
    r.begin_frame(ms(16));
    ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    assert_eq!(ui.next_wakeup(&r), Some(ms(16) + TOOLTIP_DELAY));
    assert!(!r.animation_frame_requested());
    let before = ui.stats();
    for frame in 2..=30 {
        assert!(!r.begin_frame(ms(frame * 16)));
        ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    }
    assert_eq!(
        ui.stats().component_evaluations,
        before.component_evaluations
    );
    assert_eq!(ui.stats().reused_nodes, before.reused_nodes);
    assert_eq!(ui.stats().layout_passes, before.layout_passes);
    assert!(r.begin_frame(ms(16) + TOOLTIP_DELAY));
    ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    assert!(ui.elements().any(|e| e.key == Some(&Key::from("tooltip"))));
    assert_eq!(r.next_wakeup(), None);
    assert!(!r.begin_frame(ms(700)));
    // Re-enter, establish another deadline, then leave before it fires.
    for suppress in [false, true] {
        ui.pointer(&mut r, PointerEvent::Moved([790., 590.]))
            .unwrap();
        ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
        ui.pointer(&mut r, PointerEvent::Moved([5., 5.])).unwrap();
        ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
        r.begin_frame(if suppress { ms(900) } else { ms(800) });
        ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
        assert!(r.next_wakeup().is_some());
        let event = if suppress {
            PointerEvent::Pressed([5., 5.])
        } else {
            PointerEvent::Moved([790., 590.])
        };
        ui.pointer(&mut r, event).unwrap();
        ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
        assert_eq!(r.next_wakeup(), None);
    }
    ui.pointer(&mut r, PointerEvent::Released([5., 5.]))
        .unwrap();
    ui.pointer(&mut r, PointerEvent::Moved([790., 590.]))
        .unwrap();
    ui.pointer(&mut r, PointerEvent::Moved([5., 5.])).unwrap();
    ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    r.begin_frame(ms(1000));
    ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    assert!(r.next_wakeup().is_some());
    r.update(|cx| page.update(cx, |p, _| p.show = false));
    ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    assert_eq!(r.next_wakeup(), None);
    drop(ui);
    assert_eq!(r.next_wakeup(), None);
}

#[test]
fn keyboard_focus_tooltips_delay_dismiss_and_rearm_on_refocus() {
    let mut h = Harness::new(Form::default());
    // Keyboard modality plus custom-host focus; notifications dispatch at prepare.
    h.ui.focus_next(false);
    h.ui.focus(h.id("save"));
    h.prepare();
    h.frame(1_000);
    assert!(h.shown("tooltip").is_none());
    assert_eq!(h.r.next_wakeup(), Some(Duration::from_millis(1_600)));
    let evaluations = h.ui.stats().component_evaluations;
    h.frame(1_300);
    assert_eq!(h.ui.stats().component_evaluations, evaluations);
    h.frame(1_600);
    assert!(h.shown("tooltip").is_some());
    assert_eq!(h.r.next_wakeup(), None);
    assert_eq!(h.ui.focused_element(), Some(h.id("save")));
    h.pointer(PointerEvent::Moved(h.center(h.id("save"))));
    h.pointer(PointerEvent::Moved([590., 790.]));
    assert!(h.shown("tooltip").is_some());
    h.key(KeyboardKey::Escape);
    h.frame(3_000);
    assert!(h.shown("tooltip").is_none());
    h.ui.focus(h.id("agree"));
    h.prepare();
    h.ui.focus(h.id("save"));
    h.prepare();
    h.frame(4_000);
    h.frame(4_600);
    assert!(h.shown("tooltip").is_some());
    let saved = h.ui.focused_element();
    h.ui.set_active(false);
    h.prepare();
    assert!(h.shown("tooltip").is_none());
    assert_eq!(h.ui.focused_element(), saved);
    h.ui.set_active(true);
    h.prepare();
    h.frame(5_000);
    h.frame(5_600);
    assert!(h.shown("tooltip").is_some());
    h.ui.focus(h.id("agree"));
    h.prepare();
    assert!(h.shown("tooltip").is_none());
}

#[test]
fn keyboard_tooltip_text_updates_preserve_visible_and_waiting_state() {
    struct Page {
        text: String,
        target: &'static str,
    }
    impl View for Page {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            tooltip(cx, button("Target").key(self.target), self.text.clone())
        }
    }
    let ms = Duration::from_millis;
    let mut r = Runtime::new();
    r.set_reduce_motion(true);
    r.begin_frame(Duration::ZERO);
    let page = r.update(|cx| {
        cx.new(|_| Page {
            text: "Initial".into(),
            target: "first",
        })
    });
    let mut ui = Ui::new(&mut r, page.clone()).unwrap();
    ui.prepare(&mut r, [300., 200.], &mut Measure).unwrap();
    let target = ui
        .elements()
        .find(|e| e.key == Some(&Key::from("first")))
        .unwrap()
        .id;
    ui.focus_next(false);
    ui.focus(target);
    ui.prepare(&mut r, [300., 200.], &mut Measure).unwrap();
    r.begin_frame(ms(16));
    ui.prepare(&mut r, [300., 200.], &mut Measure).unwrap();
    assert_eq!(ui.next_wakeup(&r), Some(ms(616)));

    r.begin_frame(ms(300));
    r.update(|cx| page.update(cx, |p, _| p.text = "While waiting".into()));
    ui.prepare(&mut r, [300., 200.], &mut Measure).unwrap();
    assert_eq!(ui.next_wakeup(&r), Some(ms(616)));
    assert!(!ui.elements().any(|e| e.key == Some(&Key::from("tooltip"))));
    r.begin_frame(ms(616));
    ui.prepare(&mut r, [300., 200.], &mut Measure).unwrap();
    assert!(ui.elements().any(|e| e.key == Some(&Key::from("tooltip"))));
    r.update(|cx| page.update(cx, |p, _| p.text = "While visible".into()));
    ui.prepare(&mut r, [300., 200.], &mut Measure).unwrap();
    assert!(ui.elements().any(|e| e.key == Some(&Key::from("tooltip"))));
    assert!(ui.elements().any(|e| e.text == Some("While visible")));
    assert_eq!(ui.focused_element(), Some(target));
    assert_eq!(ui.next_wakeup(&r), None);

    // Suppression also survives text edits and cancels the deadline.
    ui.key(
        &mut r,
        KeyEvent {
            key: KeyboardKey::Escape,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        },
    )
    .unwrap();
    r.update(|cx| page.update(cx, |p, _| p.text = "Suppressed".into()));
    r.begin_frame(ms(3000));
    ui.prepare(&mut r, [300., 200.], &mut Measure).unwrap();
    assert!(!ui.elements().any(|e| e.key == Some(&Key::from("tooltip"))));
    assert_eq!(ui.next_wakeup(&r), None);

    r.update(|cx| page.update(cx, |p, _| p.target = "second"));
    ui.prepare(&mut r, [300., 200.], &mut Measure).unwrap();
    let target = ui
        .elements()
        .find(|e| e.key == Some(&Key::from("second")))
        .unwrap()
        .id;
    ui.focus(target);
    ui.prepare(&mut r, [300., 200.], &mut Measure).unwrap();
    r.begin_frame(ms(3016));
    ui.prepare(&mut r, [300., 200.], &mut Measure).unwrap();
    assert_eq!(ui.next_wakeup(&r), Some(ms(3616)));
    r.begin_frame(ms(3616));
    ui.prepare(&mut r, [300., 200.], &mut Measure).unwrap();
    assert!(ui.elements().any(|e| e.key == Some(&Key::from("tooltip"))));
    assert_eq!(ui.focused_element(), Some(target));
}
