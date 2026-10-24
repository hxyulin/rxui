use crate::*;

struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.chars().count() as f32 * 8., r.font_size])
    }
}

struct Save;
impl Command for Save {}
struct Open;
impl Command for Open {}
struct Delete;
impl Command for Delete {}

/// Parts in a column and a row, and a menu whose first item has no icon.
struct Parts {
    delete_enabled: bool,
}
impl View for Parts {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let save = cx
            .command(Save, |_, _, _| {})
            .label("Save")
            .shortcut(Shortcut::new(
                KeyboardKey::Character("s".into()),
                Modifiers {
                    meta: true,
                    ..Default::default()
                },
            ));
        let open = cx.command(Open, |_, _, _| {}).label("Open");
        let delete = cx
            .command(Delete, |_, _, _| {})
            .label("Delete")
            .enabled(self.delete_enabled);
        let tones = [
            ("neutral", Tone::Neutral),
            ("accent", Tone::Accent),
            ("danger", Tone::Danger),
            ("warning", Tone::Warning),
            ("success", Tone::Success),
        ];
        column()
            .padding(10.)
            .width(300.)
            .child(label("Above").key("above"))
            .child(separator().key("across"))
            .child(separator().key("inset").horizontal().margin_x(8.))
            .child(separator().key("upright").vertical())
            .child(
                row()
                    .key("row")
                    .height(28.)
                    .child(label("Cut"))
                    .child(separator().key("between"))
                    .child(label("Find")),
            )
            .child(row().children(tones.map(|(key, tone)| badge(key).key(key).tone(tone))))
            .child(
                button(
                    row()
                        .child(label("Problems"))
                        .child(badge("3").key("count")),
                )
                .key("problems"),
            )
            .child(
                button(
                    row()
                        .child(label("Output"))
                        .child(badge("9").accessibility_hidden(true)),
                )
                .key("output"),
            )
            .child(kbd(Shortcut::new(KeyboardKey::Enter, Modifiers::default())).key("cap"))
            .child(
                menu()
                    .key("menu")
                    .width(240.)
                    .child(menu_item(&open).key("open"))
                    .child(menu_item(&save).key("save").icon(Icon::SAVE))
                    .child(menu_separator().key("rule"))
                    .child(
                        menu_item(&delete)
                            .key("delete")
                            .icon(Icon::TRASH)
                            .variant(ButtonVariant::Danger),
                    ),
            )
            .child(
                menu()
                    .key("plain")
                    .width(240.)
                    .child(menu_item(&open).key("plain-open")),
            )
    }
}

struct Harness {
    r: Runtime,
    ui: Ui<Parts>,
    root: Entity<Parts>,
}
impl Harness {
    fn new() -> Self {
        let mut r = Runtime::new();
        let root = r.update(|cx| {
            cx.new(|_| Parts {
                delete_enabled: true,
            })
        });
        let mut ui = Ui::new(&mut r, root.clone()).unwrap();
        ui.prepare(&mut r, [400., 900.], &mut Measure).unwrap();
        Self { r, ui, root }
    }
    fn prepare(&mut self) {
        self.ui
            .prepare(&mut self.r, [400., 900.], &mut Measure)
            .unwrap();
    }
    fn theme(&mut self, theme: Theme) {
        self.ui.set_theme(theme).unwrap();
        self.prepare();
    }
    fn id(&self, key: &str) -> ElementId {
        let key = Key::from(key);
        self.ui.elements().find(|e| e.key == Some(&key)).unwrap().id
    }
    fn info(&self, key: &str) -> ElementInfo<'_> {
        self.ui.element(self.id(key)).unwrap()
    }
    fn bounds(&self, key: &str) -> Bounds {
        self.info(key).bounds
    }
    fn children(&self, key: &str) -> Vec<ElementInfo<'_>> {
        let id = self.id(key);
        self.ui
            .elements()
            .filter(|e| e.parent == Some(id))
            .collect()
    }
}

#[test]
fn separators_follow_their_parent_axis_unless_fixed_and_are_unfocusable_separators() {
    let mut h = Harness::new();
    for (theme, side) in [(Theme::dark(), 16.), (Theme::light().compact(), 14.)] {
        h.theme(theme);
        // In a column: a line across the content width.
        let across = h.bounds("across");
        assert_eq!([across.x, across.width, across.height], [10., 280., 1.]);
        let above = h.bounds("above");
        assert_eq!(across.y, above.y + above.height);
        // A fixed horizontal line stretches between its margins.
        let inset = h.bounds("inset");
        assert_eq!([inset.x, inset.width, inset.height], [18., 264., 1.]);
        // A fixed vertical line in a column is icon-sized and centered.
        let upright = h.bounds("upright");
        assert_eq!([upright.width, upright.height], [1., side]);
        assert_eq!(upright.x, 10. + (280. - 1.) / 2.);
        // In a row: an icon-tall line centered on the row.
        let row = h.bounds("row");
        let between = h.bounds("between");
        assert_eq!([between.width, between.height], [1., side]);
        assert_eq!(between.y - row.y, (28. - side) / 2.);
        assert_eq!(between.x, row.x + 3. * 8.);
        let c = h.ui.theme().palette().clone();
        assert_eq!(h.info("between").background, Some(c.divider));
    }
    let node = h.ui.semantic_node(h.id("between")).unwrap();
    assert_eq!(node.role, SemanticRole::Separator);
    assert!(!node.focusable && !node.activatable);
    // Tab traversal moves between the buttons and menu items only.
    let separators = ["across", "inset", "upright", "between", "rule"].map(|k| h.id(k));
    while h.ui.focus_next(false) {
        let focused = h.ui.focused_element().unwrap();
        assert!(!separators.contains(&focused));
        if focused == h.id("plain-open") {
            break;
        }
    }
}

#[test]
fn badges_paint_each_tone_as_a_caption_pill_in_every_preset() {
    let mut h = Harness::new();
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        h.theme(theme.clone());
        let m = theme.sizes();
        for (key, tone) in [
            ("neutral", Tone::Neutral),
            ("accent", Tone::Accent),
            ("danger", Tone::Danger),
            ("warning", Tone::Warning),
            ("success", Tone::Success),
        ] {
            let info = h.info(key);
            assert_eq!(info.color, theme.color(tone.foreground()), "{key}");
            assert_eq!(
                info.background,
                Some(theme.color(tone.background())),
                "{key}"
            );
            assert_eq!(info.font_size, m.caption_size);
            assert_eq!(info.text_style.weight, FontWeight::MEDIUM);
            // 1 by 7 units of padding around the text.
            let text = key.len() as f32 * 8.;
            assert_eq!(
                [info.bounds.width, info.bounds.height],
                [text + 14., m.caption_size + 2.]
            );
            // The radius covers any height, so painting draws a pill.
            assert!(info.paint.radii.iter().all(|r| *r >= info.bounds.height));
        }
    }
    let theme = Theme::dark();
    assert_eq!(
        theme.color(Tone::Neutral.foreground()),
        theme.palette().text_muted
    );
    assert_eq!(
        theme.color(Tone::Accent.background()),
        theme.palette().accent_subtle
    );
    // Badges are labels: not focusable, and named into a containing button.
    let node = h.ui.semantic_node(h.id("danger")).unwrap();
    assert_eq!((node.role, node.focusable), (SemanticRole::Label, false));
    let problems = h.ui.semantic_node(h.id("problems")).unwrap();
    assert_eq!(problems.label, Some("Problems 3"));
    let output = h.ui.semantic_node(h.id("output")).unwrap();
    assert_eq!(output.label, Some("Output"));
}

#[test]
fn shortcuts_format_with_macos_symbols_or_plus_separated_names() {
    let chord = |key: KeyboardKey, control, alt, shift, meta| {
        Shortcut::new(
            key,
            Modifiers {
                shift,
                control,
                alt,
                meta,
            },
        )
    };
    let c = |s: &str| KeyboardKey::Character(s.into());
    for (shortcut, mac, other) in [
        (
            chord(c("p"), false, false, true, true),
            "⇧⌘P",
            "Shift+Super+P",
        ),
        (
            chord(c("p"), true, false, true, false),
            "⌃⇧P",
            "Ctrl+Shift+P",
        ),
        // Modifiers keep the system order whatever order built them.
        (
            chord(c("k"), true, true, true, true),
            "⌃⌥⇧⌘K",
            "Ctrl+Alt+Shift+Super+K",
        ),
        (chord(c("s"), false, false, false, true), "⌘S", "Super+S"),
        (
            chord(KeyboardKey::Enter, false, false, false, true),
            "⌘↩",
            "Super+Enter",
        ),
        (
            chord(KeyboardKey::Escape, false, false, false, false),
            "⎋",
            "Esc",
        ),
        (
            chord(KeyboardKey::ArrowLeft, true, false, false, false),
            "⌃←",
            "Ctrl+Left",
        ),
        (
            chord(KeyboardKey::ArrowUp, false, true, false, false),
            "⌥↑",
            "Alt+Up",
        ),
        (
            chord(KeyboardKey::ArrowDown, false, false, true, false),
            "⇧↓",
            "Shift+Down",
        ),
        (
            chord(KeyboardKey::ArrowRight, false, false, false, false),
            "→",
            "Right",
        ),
        (
            chord(KeyboardKey::Function(2), false, false, false, false),
            "F2",
            "F2",
        ),
        (
            chord(KeyboardKey::Function(12), true, false, true, false),
            "⌃⇧F12",
            "Ctrl+Shift+F12",
        ),
        (
            chord(KeyboardKey::Delete, false, false, false, true),
            "⌘⌦",
            "Super+Del",
        ),
        (
            chord(c(" "), true, false, false, false),
            "⌃Space",
            "Ctrl+Space",
        ),
        (chord(c("+"), true, false, false, false), "⌃+", "Ctrl++"),
    ] {
        assert_eq!(shortcut.format(true), mac);
        assert_eq!(shortcut.format(false), other);
    }
    // The primary modifier formats as the platform's own.
    let palette = Shortcut::primary("p").shift();
    let expected = if cfg!(target_os = "macos") {
        "⇧⌘P"
    } else {
        "Ctrl+Shift+P"
    };
    assert_eq!(palette.label(), expected);
}

#[test]
fn key_caps_are_muted_caption_text_in_a_bordered_inset() {
    let mut h = Harness::new();
    for theme in [Theme::dark(), Theme::light()] {
        h.theme(theme.clone());
        let (c, m) = (theme.palette(), theme.sizes());
        let cap = h.info("cap");
        let expected = Shortcut::new(KeyboardKey::Enter, Modifiers::default()).label();
        assert_eq!(cap.text, Some(expected.as_str()));
        assert_eq!(cap.font_size, m.caption_size);
        assert_eq!(cap.color, c.text_muted);
        assert_eq!(cap.background, Some(c.background));
        assert_eq!(cap.paint.border_color, Some(c.border));
        assert_eq!(cap.border, [1.; 4]);
        assert_eq!(cap.paint.radii, [m.radius_small; 4]);
    }
}

#[test]
fn menu_items_align_icon_columns_and_trail_their_shortcut() {
    let h = Harness::new();
    // The icon-less first item reserved the icon column once a later item had an
    // icon, so all captions start at the same x.
    let caption = |key: &str| {
        let parts = h.children(key);
        let text = parts.iter().find(|p| p.text.is_some_and(|t| t.len() > 2));
        text.unwrap().bounds
    };
    let open = h.children("open");
    assert_eq!(open.len(), 2);
    assert_eq!(open[0].kind, ElementType::Row);
    assert_eq!([open[0].bounds.width, open[0].bounds.height], [16., 16.]);
    assert_eq!(caption("open").x, caption("save").x);
    assert_eq!(caption("save").x, caption("delete").x);
    let item = h.info("save").content_bounds;
    assert_eq!(caption("save").x, item.x + 16. + 6.);
    // A menu without icons starts captions at the row padding.
    let plain = h.info("plain-open").content_bounds;
    assert_eq!(h.children("plain-open").len(), 1);
    assert_eq!(caption("plain-open").x, plain.x);

    // The shortcut trails at the item's end, and the caption fills between.
    let save = h.children("save");
    assert_eq!(save.len(), 3);
    assert_eq!(save[0].kind, ElementType::Custom);
    assert_eq!(
        save[2].text,
        Some(
            Shortcut::new(
                KeyboardKey::Character("s".into()),
                Modifiers {
                    meta: true,
                    ..Default::default()
                }
            )
            .label()
            .as_str()
        )
    );
    let cap = save[2].bounds;
    assert!((cap.x + cap.width - (item.x + item.width)).abs() < 0.01);
    assert!((save[1].bounds.x + save[1].bounds.width + 6. + 18. - cap.x).abs() < 0.01);
    assert_eq!(save[2].color, h.ui.theme().palette().text_muted);
}

#[test]
fn menu_shortcuts_are_published_apart_from_the_item_name() {
    let h = Harness::new();
    let save = h.ui.semantic_node(h.id("save")).unwrap();
    assert_eq!(save.role, SemanticRole::MenuItem);
    assert_eq!(save.label, Some("Save"));
    let shortcut = h.children("save")[2].text.unwrap().to_owned();
    assert_eq!(save.keyboard_shortcut, Some(shortcut.as_str()));
    // The key cap and the icon are hidden; the caption is the only text.
    let parts = h.children("save");
    assert!(h.ui.semantic_node(parts[0].id).is_none());
    assert!(h.ui.semantic_node(parts[2].id).is_none());
    let open = h.ui.semantic_node(h.id("open")).unwrap();
    assert_eq!((open.label, open.keyboard_shortcut), (Some("Open"), None));
}

#[test]
fn danger_menu_items_paint_danger_text_and_icon_over_row_hover() {
    let mut h = Harness::new();
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        h.theme(theme.clone());
        let c = theme.palette();
        let rest = h.info("delete");
        assert_eq!((rest.color, rest.background), (c.danger, None));
        assert_eq!(rest.paint.border_color, None);
        let parts = h.children("delete");
        assert_eq!(parts[0].color, c.danger);
        assert_eq!(parts[1].color, c.danger);
        let b = rest.bounds;
        h.ui.pointer(&mut h.r, PointerEvent::Moved([b.x + 2., b.y + 2.]))
            .unwrap();
        let hovered = h.info("delete");
        assert_eq!(
            (hovered.color, hovered.background),
            (c.danger, Some(c.row_hover))
        );
        h.ui.pointer(&mut h.r, PointerEvent::Moved([390., 890.]))
            .unwrap();
        // An ordinary item keeps the inherited text color.
        assert_eq!(h.info("save").color, c.text);
    }
    // A disabled danger item disables like any row.
    h.r.update(|cx| h.root.update(cx, |s, _| s.delete_enabled = false));
    h.prepare();
    let disabled = h.info("delete");
    assert_eq!(disabled.color, h.ui.theme().palette().text_disabled);
}

#[test]
fn menu_separators_are_inset_lines_between_items() {
    let h = Harness::new();
    let rule = h.bounds("rule");
    let (save, delete) = (h.bounds("save"), h.bounds("delete"));
    assert_eq!([rule.width, rule.height], [save.width, 1.]);
    // 1-unit menu gaps plus 4 units of margin on each side.
    assert_eq!(rule.y, save.y + save.height + 1. + 4.);
    assert_eq!(delete.y, rule.y + 1. + 4. + 1.);
    let node = h.ui.semantic_node(h.id("rule")).unwrap();
    assert_eq!(node.role, SemanticRole::Separator);
}

#[test]
fn menu_arrow_keys_move_between_items_across_a_separator() {
    let mut h = Harness::new();
    let save = h.id("save");
    assert!(h.ui.focus(save));
    let down = KeyEvent {
        physical_key: Default::default(),
        key: KeyboardKey::ArrowDown,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    };
    h.ui.key(&mut h.r, down).unwrap();
    assert_eq!(h.ui.focused_element(), Some(h.id("delete")));
}

#[test]
fn menu_children_align_whichever_item_brings_the_first_icon() {
    struct Late;
    impl View for Late {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let open = cx.command(Open, |_, _, _| {}).label("Open");
            let save = cx.command(Save, |_, _, _| {}).label("Save");
            menu()
                .children([
                    menu_item(&save).key("a").into_element(),
                    menu_item(&open)
                        .key("b")
                        .icon(Icon::FOLDER_OPEN)
                        .into_element(),
                ])
                .child(label("Note").key("note"))
                .child(menu_item(&save).key("c"))
        }
    }
    let mut r = Runtime::new();
    let root = r.update(|cx| cx.new(|_| Late));
    let mut ui = Ui::new(&mut r, root).unwrap();
    ui.prepare(&mut r, [400., 300.], &mut Measure).unwrap();
    let first_x = |key: &str| {
        let id = ui
            .elements()
            .find(|e| e.key == Some(&Key::from(key)))
            .unwrap()
            .id;
        let parts: Vec<_> = ui.elements().filter(|e| e.parent == Some(id)).collect();
        (parts.len(), parts.last().unwrap().bounds.x)
    };
    assert_eq!(first_x("a"), first_x("b"));
    assert_eq!(first_x("b"), first_x("c"));
    assert_eq!(first_x("a").0, 2);
}

#[cfg(feature = "accessibility")]
#[test]
fn accesskit_publishes_menu_shortcuts_and_separators() {
    let h = Harness::new();
    let mut cache = AccessKitTree::new();
    let update = cache.update(&h.ui, "Parts", 1.).unwrap().unwrap();
    let node = |role, label: &str| {
        update
            .nodes
            .iter()
            .find(|(_, n)| n.role() == role && n.label() == Some(label))
            .map(|(_, n)| n.clone())
            .unwrap()
    };
    let save = node(accesskit::Role::MenuItem, "Save");
    let shortcut = h.children("save")[2].text.unwrap().to_owned();
    assert_eq!(save.keyboard_shortcut(), Some(shortcut.as_str()));
    let open = node(accesskit::Role::MenuItem, "Open");
    assert_eq!(open.keyboard_shortcut(), None);
    let separators = update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == accesskit::Role::Splitter)
        .collect::<Vec<_>>();
    assert_eq!(separators.len(), 5);
    assert!(separators.iter().all(|(_, n)| {
        !n.supports_action(accesskit::Action::Focus) && n.numeric_value().is_none()
    }));
}
