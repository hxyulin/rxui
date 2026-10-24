use crate::*;

struct Static(Element);
impl View for Static {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        self.0.clone()
    }
}
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([
            request.text.chars().count() as f32 * request.font_size / 2.,
            request.font_size,
        ])
    }
}
fn mount(element: Element) -> (Runtime, Entity<Static>, Ui<Static>) {
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| cx.new(|_| Static(element)));
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    (runtime, root, ui)
}
fn id(ui: &Ui<Static>, key: &str) -> ElementId {
    ui.elements()
        .find(|e| e.key == Some(&Key::from(key)))
        .unwrap()
        .id
}
fn paint(ui: &Ui<Static>, key: &str) -> ResolvedPaint {
    ui.element(id(ui, key)).unwrap().paint
}
fn hover(ui: &mut Ui<Static>, runtime: &mut Runtime, key: &str) {
    let b = ui.element(id(ui, key)).unwrap().bounds;
    ui.pointer(runtime, PointerEvent::Moved([b.x + 2., b.y + 2.]))
        .unwrap();
}
fn switch(ui: &mut Ui<Static>, runtime: &mut Runtime, theme: &Theme) {
    ui.set_theme(theme.clone()).unwrap();
    ui.prepare(runtime, [800., 600.], &mut Measure).unwrap();
}
fn shadow(theme: &Theme, level: Elevation) -> Option<ResolvedShadow> {
    theme.shadow(level).map(|s| ResolvedShadow {
        offset: s.offset,
        blur: s.blur,
        spread: s.spread,
        color: match s.color {
            StyleColor::Literal(c) => c,
            StyleColor::Theme(t) => theme.color(t),
        },
    })
}
fn presets() -> [Theme; 3] {
    [Theme::dark(), Theme::light(), Theme::high_contrast()]
}

#[test]
fn danger_buttons_paint_danger_text_and_border_tint_when_active_and_disable_like_controls() {
    let (mut runtime, root, mut ui) = mount(
        column().padding(10.).child(
            button("Delete")
                .key("danger")
                .variant(ButtonVariant::Danger),
        ),
    );
    for theme in presets() {
        switch(&mut ui, &mut runtime, &theme);
        let c = theme.palette();
        let rest = paint(&ui, "danger");
        assert_eq!(rest.background, None);
        assert_eq!(rest.color, c.danger);
        assert_eq!(rest.border_color, Some(c.danger));
        hover(&mut ui, &mut runtime, "danger");
        let hovered = paint(&ui, "danger");
        assert_eq!(hovered.background, Some(c.danger_subtle));
        assert_eq!(
            (hovered.color, hovered.border_color),
            (c.danger, Some(c.danger))
        );
        let b = ui.element(id(&ui, "danger")).unwrap().bounds;
        ui.pointer(&mut runtime, PointerEvent::Pressed([b.x + 2., b.y + 2.]))
            .unwrap();
        assert_eq!(paint(&ui, "danger").background, Some(c.danger_subtle));
        ui.pointer(&mut runtime, PointerEvent::Released([b.x + 2., b.y + 2.]))
            .unwrap();
        ui.pointer(&mut runtime, PointerEvent::Moved([700., 500.]))
            .unwrap();
    }
    runtime.update(|cx| {
        root.update(cx, |s, _| {
            s.0 = column().padding(10.).child(
                button("Delete")
                    .key("danger")
                    .variant(ButtonVariant::Danger)
                    .disabled(true),
            )
        })
    });
    let theme = Theme::dark();
    switch(&mut ui, &mut runtime, &theme);
    let c = theme.palette();
    let disabled = paint(&ui, "danger");
    assert_eq!(
        (disabled.background, disabled.color, disabled.border_color),
        (
            Some(c.control_disabled),
            c.text_disabled,
            Some(c.border_disabled)
        )
    );
}

#[test]
fn inputs_and_button_variants_resolve_distinct_default_paints() {
    let (mut runtime, _, mut ui) = mount(
        column()
            .padding(10.)
            .gap(10.)
            .child(button("Save").key("button"))
            .child(text_input("").key("input"))
            .child(button("Go").key("primary").variant(ButtonVariant::Primary))
            .child(button("More").key("quiet").variant(ButtonVariant::Quiet)),
    );
    for theme in presets() {
        switch(&mut ui, &mut runtime, &theme);
        let c = theme.palette();
        let button = paint(&ui, "button");
        let input = paint(&ui, "input");
        assert_eq!(
            (button.background, button.border_color),
            (Some(c.control), Some(c.border))
        );
        assert_eq!(
            (input.background, input.border_color),
            (Some(c.input), Some(c.input_border))
        );
        // Light buttons rest on a control shadow; inputs are never raised.
        assert_eq!(button.shadow, shadow(&theme, Elevation::Control));
        assert_eq!(input.shadow, None);
        // Primary buttons have no visible edge; quiet ones mute their caption.
        let primary = paint(&ui, "primary");
        assert_eq!(
            (primary.background, primary.border_color),
            (Some(c.accent), None)
        );
        assert_eq!(paint(&ui, "quiet").color, c.text_muted);
        // Hover fills a button but leaves an input's fill alone.
        hover(&mut ui, &mut runtime, "button");
        assert_eq!(paint(&ui, "button").background, Some(c.control_hover));
        hover(&mut ui, &mut runtime, "input");
        assert_eq!(paint(&ui, "input").background, Some(c.input));
        hover(&mut ui, &mut runtime, "quiet");
        let quiet = paint(&ui, "quiet");
        assert_eq!((quiet.background, quiet.color), (Some(c.row_hover), c.text));
        // A focused input outlines itself in the focus color.
        assert!(ui.focus(id(&ui, "input")));
        assert_eq!(paint(&ui, "input").border_color, Some(c.focus));
        assert!(ui.focus(id(&ui, "button")));
        assert_eq!(paint(&ui, "input").border_color, Some(c.input_border));
        ui.pointer(&mut runtime, PointerEvent::Moved([700., 500.]))
            .unwrap();
    }
    // Labelled buttons default to medium weight; an explicit weight wins.
    let weight = |ui: &Ui<Static>, key: &str| ui.element(id(ui, key)).unwrap().text_style.weight;
    assert_eq!(weight(&ui, "button"), FontWeight::MEDIUM);
    assert_eq!(weight(&ui, "quiet"), FontWeight::NORMAL);
    assert_eq!(weight(&ui, "input"), FontWeight::NORMAL);
}

#[test]
fn popovers_and_dialogs_resolve_raised_surfaces_and_follow_theme_switches() {
    let (mut runtime, _, mut ui) = mount(
        column()
            .child(column().key("card").raised(Elevation::Popover))
            .child(popover([10., 10.], label("Menu")).key("popover"))
            .child(
                column()
                    .key("custom")
                    .raised(Elevation::Modal)
                    .radius(2.)
                    .background(ThemeColor::Surface),
            ),
    );
    let surfaces = |ui: &Ui<Static>, level: SemanticRole| {
        ui.elements()
            .filter(|e| ui.semantic_node(e.id).is_some_and(|n| n.role == level))
            .map(|e| e.paint)
            .collect::<Vec<_>>()
    };
    for theme in presets() {
        switch(&mut ui, &mut runtime, &theme);
        let c = theme.palette();
        let m = theme.sizes();
        let card = paint(&ui, "card");
        assert_eq!(card.background, Some(c.raised));
        assert_eq!(card.border_color, Some(c.divider));
        assert_eq!(card.radii, [m.radius_large; 4]);
        assert_eq!(card.shadow, shadow(&theme, Elevation::Popover));
        assert_eq!(ui.element(id(&ui, "card")).unwrap().border, [1.; 4]);
        // The popover overlay's surface is the same raised surface, without a
        // ring of its own: it only scopes focus for its items.
        let popover = surfaces(&ui, SemanticRole::Group);
        let ringless = ResolvedPaint {
            focus_width: 0.,
            ..card
        };
        assert!(popover.contains(&ringless), "{popover:?}");
        // Explicit builders win over each part; the level still resolves.
        let custom = paint(&ui, "custom");
        assert_eq!(custom.radii, [2.; 4]);
        assert_eq!(custom.background, Some(c.surface));
        assert_eq!(custom.border_color, Some(c.divider));
        assert_eq!(custom.shadow, shadow(&theme, Elevation::Modal));
    }
    // Dark and light popovers cast a shadow; high contrast stays flat.
    assert!(Theme::dark().shadow(Elevation::Popover).is_some());
    assert!(Theme::light().shadow(Elevation::Popover).is_some());
    assert_eq!(paint(&ui, "card").shadow, None);
}

#[test]
fn modal_dialogs_use_the_modal_elevation() {
    let (_, _, ui) = mount(
        column().child(
            modal(label("Discard?"))
                .key("dialog")
                .accessibility_label("Discard"),
        ),
    );
    let theme = Theme::dark();
    let dialog = ui
        .elements()
        .find(|e| {
            ui.semantic_node(e.id)
                .is_some_and(|n| n.role == SemanticRole::Dialog)
        })
        .unwrap();
    assert_eq!(dialog.paint.shadow, shadow(&theme, Elevation::Modal));
    assert_eq!(dialog.paint.radii, [theme.sizes().radius_large; 4]);
}

#[test]
fn list_rows_use_row_tokens_and_dim_selection_while_focus_is_outside_their_list() {
    let rows = column()
        .key("list")
        .focusable(true)
        .padding(4.)
        .child(button("One").key("selected").list_row(true))
        .child(button("Two").key("plain").list_row(false))
        .child(
            column()
                .key("passive")
                .height(20.)
                .list_row(false)
                .on_hover(Listener::from_callback(|_: &HoverEvent, _| {
                    Ok(Dispatch::Unchanged)
                })),
        );
    let (mut runtime, _, mut ui) = mount(
        column()
            .padding(10.)
            .child(rows)
            .child(button("Elsewhere").key("outside")),
    );
    for theme in presets() {
        switch(&mut ui, &mut runtime, &theme);
        let c = theme.palette();
        // Nothing focused: the selection is inactive and other rows are bare.
        let selected = paint(&ui, "selected");
        assert_eq!(selected.background, Some(c.row_selected_inactive));
        assert_eq!(selected.border_color, None);
        assert_eq!(selected.color, c.text);
        assert_eq!(paint(&ui, "plain").background, None);
        assert!(ui.focus(id(&ui, "list")));
        assert_eq!(paint(&ui, "selected").background, Some(c.row_selected));
        // Focus on a row inside the list keeps the selection active.
        assert!(ui.focus(id(&ui, "plain")));
        assert_eq!(paint(&ui, "selected").background, Some(c.row_selected));
        assert!(ui.focus(id(&ui, "outside")));
        assert_eq!(
            paint(&ui, "selected").background,
            Some(c.row_selected_inactive)
        );
        hover(&mut ui, &mut runtime, "plain");
        assert_eq!(paint(&ui, "plain").background, Some(c.row_hover));
        hover(&mut ui, &mut runtime, "passive");
        assert_eq!(paint(&ui, "passive").background, Some(c.row_hover));
        ui.pointer(&mut runtime, PointerEvent::Moved([700., 500.]))
            .unwrap();
        assert_eq!(paint(&ui, "plain").background, None);
    }
    // Keyboard focus highlights an unselected row like hover, as in menus.
    ui.set_focus_ring_policy(FocusRingPolicy::Always);
    assert!(ui.focus(id(&ui, "plain")));
    assert_eq!(
        paint(&ui, "plain").background,
        Some(Theme::high_contrast().palette().row_hover)
    );
}

#[test]
fn list_rows_take_dense_metric_sizes_unless_the_app_sets_its_own() {
    let (mut runtime, _, mut ui) = mount(
        column()
            .child(button("Row").key("row").list_row(false))
            .child(
                row()
                    .key("container")
                    .list_row(true)
                    .child(label("Passive")),
            )
            .child(
                column()
                    .key("tall")
                    .list_row(false)
                    .child(column().height(40.)),
            )
            .child(button("Fixed").key("fixed").height(30.).list_row(false))
            .child(
                button("Minimum")
                    .key("minimum")
                    .min_height(32.)
                    .list_row(false),
            )
            .child(button("Padded").key("padded").padding(10.).list_row(false)),
    );
    let element = |ui: &Ui<Static>, key: &str| {
        let e = ui.element(id(ui, key)).unwrap();
        (e.bounds, e.content_bounds)
    };
    for theme in [Theme::dark(), Theme::dark().compact()] {
        switch(&mut ui, &mut runtime, &theme);
        let m = theme.sizes();
        // Rows drop the button padding for their own, with none vertically.
        for key in ["row", "container"] {
            let (bounds, content) = element(&ui, key);
            assert_eq!(bounds.height, m.row_height, "{key}");
            let inset = if key == "row" { m.border_width } else { 0. };
            assert_eq!(content.x - bounds.x, m.row_padding_x + inset, "{key}");
            assert_eq!(content.y - bounds.y, inset, "{key}");
        }
        // The height is a minimum that taller content exceeds.
        assert_eq!(element(&ui, "tall").0.height, 40.);
        // Explicit sizes and padding win.
        assert_eq!(element(&ui, "fixed").0.height, 30.);
        assert_eq!(element(&ui, "minimum").0.height, 32.);
        let (bounds, content) = element(&ui, "padded");
        assert_eq!(content.x - bounds.x, 11.);
        assert_eq!(content.y - bounds.y, 11.);
    }
    assert_eq!(ThemeMetrics::balanced().row_height, 24.);
    assert_eq!(ThemeMetrics::compact().row_height, 22.);
}

#[test]
fn focus_rings_surround_boxes_outside_and_stay_inside_rows() {
    let (mut runtime, _, mut ui) = mount(
        column()
            .padding(20.)
            .gap(20.)
            .child(button("Ring").key("button"))
            .child(button("Row").key("row").list_row(false))
            .child(
                button("Custom")
                    .key("custom")
                    .paint_style(PaintStyle::new().focus_width(1.).focus_offset(0.)),
            ),
    );
    let extent = |ui: &Ui<Static>, key: &str| {
        let e = ui.element(id(ui, key)).unwrap();
        (e.bounds, e.paint.focus_extent(e.bounds).unwrap())
    };
    let grown = |b: Bounds, by: f32| Bounds {
        x: b.x - by,
        y: b.y - by,
        width: b.width + 2. * by,
        height: b.height + 2. * by,
    };
    // A 2-unit ring after a 1-unit gap.
    let (bounds, ring) = extent(&ui, "button");
    assert_eq!(ring, grown(bounds, 3.));
    let (bounds, ring) = extent(&ui, "row");
    assert_eq!(paint(&ui, "row").focus_offset, -2.);
    assert_eq!(ring, bounds);
    let (bounds, ring) = extent(&ui, "custom");
    assert_eq!(ring, grown(bounds, 1.));
    // High contrast keeps its thicker ring, still outside.
    switch(&mut ui, &mut runtime, &Theme::high_contrast());
    let (bounds, ring) = extent(&ui, "button");
    assert_eq!(ring, grown(bounds, 4.));
    let (bounds, ring) = extent(&ui, "row");
    assert_eq!((paint(&ui, "row").focus_offset, ring), (-3., bounds));
    // Zero-width rings cover nothing.
    let none = ResolvedPaint {
        focus_width: 0.,
        ..paint(&ui, "button")
    };
    assert_eq!(none.focus_extent(bounds), None);
}
