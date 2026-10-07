use crate::*;
#[derive(Default)]
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([request.text.len() as f32 * 8., 20.])
    }
}
fn keyed<T: View>(ui: &Ui<T>, key: &str) -> ElementId {
    ui.elements()
        .find(|e| e.key == Some(&Key::from(key)))
        .unwrap()
        .id
}
fn prepare<T: View>(r: &mut Runtime, ui: &mut Ui<T>) {
    ui.prepare(r, [640., 400.], &mut Measure).unwrap();
}
fn activate<T: View>(r: &mut Runtime, ui: &mut Ui<T>, key: &str) {
    let id = keyed(ui, key);
    assert!(
        ui.semantic_action(r, SemanticAction::Activate(id), &mut Measure)
            .unwrap()
    );
    prepare(r, ui);
}
fn key<T: View>(r: &mut Runtime, ui: &mut Ui<T>, key: KeyboardKey) -> InputResult {
    ui.key(
        r,
        KeyEvent {
            key,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        },
    )
    .unwrap()
}
struct FocusPage {
    handle: FocusHandle,
    saved: Option<FocusPlacement>,
    show: bool,
    disabled: bool,
    duplicate: bool,
}
impl View for FocusPage {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut group = column()
            .key("scope")
            .focus_scope(FocusScope::Cycle)
            .focus_handle(self.handle.clone())
            .child(text_input("First").key("first"))
            .child(text_input("Second").key("second").disabled(self.disabled));
        if !self.show {
            group = group.layout(|s| s.display = taffy::Display::None);
        }
        let mut root = column()
            .child(button("Outside").key("outside"))
            .child(
                button("Restore")
                    .key("restore")
                    .on_click(cx.listener(|s, _, cx| {
                        s.saved = Some(s.handle.placement(cx).unwrap());
                        s.handle.focus(cx).unwrap();
                    })),
            )
            .child(group);
        if self.duplicate {
            root = root.child(column().focus_handle(self.handle.clone()));
        }
        root
    }
}
#[test]
fn scope_cycles_restores_and_resolves_handles_in_only_the_source_window() {
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| FocusPage {
            handle: FocusHandle::new(),
            saved: None,
            show: true,
            disabled: false,
            duplicate: false,
        })
    });
    let mut a = Ui::new(&mut r, e.clone()).unwrap();
    let mut b = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut a);
    prepare(&mut r, &mut b);
    assert!(a.focus(keyed(&a, "first")));
    assert!(a.focus_next(false));
    assert_eq!(a.focused_element(), Some(keyed(&a, "second")));
    assert!(a.focus_next(false));
    assert_eq!(a.focused_element(), Some(keyed(&a, "first")));
    assert!(a.focus_next(true));
    assert_eq!(a.focused_element(), Some(keyed(&a, "second")));
    a.focus(keyed(&a, "outside"));
    activate(&mut r, &mut a, "restore");
    assert_eq!(a.focused_element(), Some(keyed(&a, "second")));
    assert_eq!(b.focused_element(), None);
    let saved = r.update(|cx| e.read(cx).saved.clone().unwrap());
    let foreign = r.update(|cx| e.read(cx).handle.clone());
    assert_eq!(
        r.update(|cx| foreign.focus(cx)),
        Err(FocusError::NoPlacement)
    );
    r.update(|cx| e.update(cx, |s, _| s.disabled = true));
    prepare(&mut r, &mut a);
    r.update(|cx| saved.focus(cx)).unwrap();
    prepare(&mut r, &mut a);
    assert_eq!(a.focused_element(), Some(keyed(&a, "first")));
    let mut other = Runtime::new();
    assert_eq!(
        other.update(|cx| saved.focus(cx)),
        Err(FocusError::WrongRuntime)
    );
    r.update(|cx| e.update(cx, |s, _| s.show = false));
    prepare(&mut r, &mut a);
    assert_eq!(a.focused_element(), None);
    r.update(|cx| saved.focus(cx)).unwrap();
    prepare(&mut r, &mut a);
    assert_eq!(a.focused_element(), None);
    drop(a);
    assert_eq!(r.update(|cx| saved.focus(cx)), Err(FocusError::Disposed));
    prepare(&mut r, &mut b);
}
#[test]
fn duplicate_focus_bindings_fail_and_retry_without_panicking() {
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| FocusPage {
            handle: FocusHandle::new(),
            saved: None,
            show: true,
            disabled: false,
            duplicate: true,
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    assert!(matches!(
        ui.prepare(&mut r, [640., 400.], &mut Measure),
        Err(UiError::DuplicateFocusHandle)
    ));
    r.update(|cx| e.update(cx, |s, _| s.duplicate = false));
    prepare(&mut r, &mut ui);
    activate(&mut r, &mut ui, "restore");
    assert_eq!(ui.focused_element(), Some(keyed(&ui, "first")));
}
struct Document {
    name: String,
    handle: ScrollHandle,
}
impl View for Document {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .fill_height()
            .child(
                text_input(self.name.clone())
                    .key("field")
                    .accessibility_label(self.name.clone())
                    .on_change(cx.listener(|s, e: &TextChangeEvent, _| s.name = e.value.clone())),
            )
            .child(
                scroll_area(
                    column().children((0..40).map(|i| label(format!("Line {i}")).height(24.))),
                )
                .handle(self.handle.clone())
                .size(400., 100.),
            )
    }
}
struct Page {
    selected: Option<Key>,
    entries: Vec<u32>,
    docs: Vec<Entity<Document>>,
    policy: TabContentPolicy,
    activation: TabActivation,
    disabled: bool,
    close: Option<TabCloseEvent>,
}
impl View for Page {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut group = tabs()
            .key("tabs")
            .content_policy(self.policy)
            .activation(self.activation)
            .tabs(self.entries.iter().map(|i| {
                tab(*i, format!("Doc {i}"), self.docs[*i as usize].clone())
                    .closable(true)
                    .disabled(self.disabled && *i == 1)
            }))
            .on_select(cx.listener(|s, e: &TabSelectEvent, _| s.selected = Some(e.key.clone())))
            .on_close(cx.listener(|s, e: &TabCloseEvent, _| {
                s.entries.retain(|i| Key::from(*i) != e.key);
                s.selected = e.next_selection.clone();
                s.close = Some(e.clone());
            }));
        if let Some(selected) = &self.selected {
            group = group.selected(selected.clone());
        }
        column()
            .fill_width()
            .fill_height()
            .child(button("Outside").key("outside"))
            .child(group)
    }
}

fn setup(policy: TabContentPolicy) -> (Runtime, Entity<Page>, Ui<Page>) {
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        let docs = (0..3)
            .map(|i| {
                cx.new(|_| Document {
                    name: format!("Field {i}"),
                    handle: ScrollHandle::new(),
                })
            })
            .collect();
        cx.new(|_| Page {
            selected: Some(0.into()),
            entries: vec![0, 1, 2],
            docs,
            policy,
            activation: TabActivation::Automatic,
            disabled: false,
            close: None,
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut ui);
    (r, e, ui)
}
fn header(ui: &Ui<Page>, name: &str) -> ElementId {
    ui.semantics()
        .find(|s| s.role == SemanticRole::Tab && s.label == Some(name))
        .unwrap()
        .id
}
#[test]
fn tabs_retain_mounts_scroll_and_editor_selection_and_restore_panel_focus() {
    let (mut r, e, mut ui) = setup(TabContentPolicy::KeepMounted);
    assert_eq!(ui.mount_ids().count(), 4);
    let field = keyed(&ui, "field");
    assert!(ui.focus(field));
    let revision = ui.element(field).unwrap().text_revision;
    ui.semantic_action(
        &mut r,
        SemanticAction::SetSelection {
            target: field,
            text_revision: revision,
            selection: TextSelection {
                anchor: TextPosition::new(2),
                focus: TextPosition::new(4),
            },
        },
        &mut Measure,
    )
    .unwrap();
    let viewport = ui.elements().find(|n| n.scroll_range[1] > 0.).unwrap();
    let point = [viewport.bounds.x + 2., viewport.bounds.y + 2.];
    let scroll_id = viewport.id;
    assert!(ui.scroll(point, [0., 80.]).unwrap());
    r.update(|cx| e.update(cx, |s, _| s.selected = Some(1.into())));
    prepare(&mut r, &mut ui);
    assert_eq!(ui.focused_element(), Some(keyed(&ui, "field")));
    assert_ne!(keyed(&ui, "field"), field);
    r.update(|cx| e.update(cx, |s, _| s.selected = Some(0.into())));
    prepare(&mut r, &mut ui);
    assert_eq!(keyed(&ui, "field"), field);
    assert_eq!(ui.focused_element(), Some(field));
    assert_eq!(
        ui.element(field)
            .unwrap()
            .editing
            .unwrap()
            .selection
            .range(),
        2..4
    );
    assert_eq!(ui.element(scroll_id).unwrap().scroll_offset[1], 80.);
    assert_eq!(ui.mount_ids().count(), 4);
    // Tab enters the remembered panel target rather than an arbitrary first control.
    ui.focus(header(&ui, "Doc 0"));
    assert!(ui.focus_next(false));
    assert_eq!(ui.focused_element(), Some(field));
}
#[test]
fn tab_keys_roving_stop_manual_activation_close_and_disabled_headers() {
    let (mut r, e, mut ui) = setup(TabContentPolicy::KeepMounted);
    ui.focus(header(&ui, "Doc 0"));
    assert!(key(&mut r, &mut ui, KeyboardKey::ArrowRight).default_prevented);
    prepare(&mut r, &mut ui);
    assert_eq!(r.update(|cx| e.read(cx).selected.clone()), Some(1.into()));
    assert_eq!(ui.focused_element(), Some(header(&ui, "Doc 1")));
    assert!(ui.focus_next(false));
    assert_eq!(ui.focused_element(), Some(keyed(&ui, "field")));
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.activation = TabActivation::Manual;
            s.selected = Some(0.into());
            s.disabled = true;
        })
    });
    prepare(&mut r, &mut ui);
    ui.focus(header(&ui, "Doc 0"));
    key(&mut r, &mut ui, KeyboardKey::ArrowRight);
    prepare(&mut r, &mut ui);
    assert_eq!(ui.focused_element(), Some(header(&ui, "Doc 2")));
    assert_eq!(r.update(|cx| e.read(cx).selected.clone()), Some(0.into()));
    assert!(ui.activate_focused(&mut r).unwrap());
    prepare(&mut r, &mut ui);
    assert_eq!(r.update(|cx| e.read(cx).selected.clone()), Some(2.into()));
    assert!(key(&mut r, &mut ui, KeyboardKey::Delete).default_prevented);
    prepare(&mut r, &mut ui);
    assert_eq!(r.update(|cx| e.read(cx).selected.clone()), Some(0.into()));
    assert_eq!(ui.focused_element(), Some(header(&ui, "Doc 0")));
    assert_eq!(
        r.update(|cx| e.read(cx).close.clone().unwrap().next_selection),
        Some(0.into())
    );
}
#[test]
fn mount_selected_disposes_widget_placement_but_retains_application_entity() {
    let (mut r, e, mut ui) = setup(TabContentPolicy::MountSelected);
    assert_eq!(ui.mount_ids().count(), 2);
    let old = keyed(&ui, "field");
    ui.focus(old);
    let doc = r.update(|cx| e.read(cx).docs[0].clone());
    r.update(|cx| e.update(cx, |s, _| s.selected = Some(1.into())));
    prepare(&mut r, &mut ui);
    assert!(!ui.contains_element(old));
    assert_eq!(ui.mount_ids().count(), 2);
    r.update(|cx| doc.update(cx, |s, _| s.name = "Saved model".into()));
    r.update(|cx| e.update(cx, |s, _| s.selected = Some(0.into())));
    prepare(&mut r, &mut ui);
    assert_ne!(keyed(&ui, "field"), old);
    assert_eq!(
        ui.element(keyed(&ui, "field")).unwrap().text,
        Some("Saved model")
    );
}
#[cfg(feature = "accessibility")]
#[test]
fn tab_accessibility_links_and_selection_update_on_controlled_switch() {
    let (mut r, e, mut ui) = setup(TabContentPolicy::KeepMounted);
    let mut cache = AccessKitTree::new();
    let first = cache.update(&ui, "Tabs", 2.).unwrap().unwrap();
    let panel = first
        .nodes
        .iter()
        .find(|(_, n)| n.role() == accesskit::Role::TabPanel)
        .unwrap();
    let header = first
        .nodes
        .iter()
        .find(|(_, n)| n.role() == accesskit::Role::Tab && n.is_selected() == Some(true))
        .unwrap();
    assert_eq!(header.1.controls(), &[panel.0]);
    assert_eq!(panel.1.labelled_by(), &[header.0]);
    let mut consumer = accesskit_consumer::Tree::new(first, true);
    r.update(|cx| e.update(cx, |s, _| s.selected = Some(1.into())));
    prepare(&mut r, &mut ui);
    consumer.update_and_process_changes(
        cache.update(&ui, "Tabs", 2.).unwrap().unwrap(),
        &mut Changes,
    );
    let node = ui
        .semantics()
        .find(|n| n.role == SemanticRole::TabPanel)
        .unwrap();
    assert_eq!(node.label, Some("Doc 1"));
}

#[cfg(feature = "accessibility")]
struct Changes;
#[cfg(feature = "accessibility")]
impl accesskit_consumer::TreeChangeHandler for Changes {
    fn node_added(&mut self, _: &accesskit_consumer::NodeRef) {}
    fn node_updated(&mut self, _: &accesskit_consumer::NodeRef, _: &accesskit_consumer::NodeRef) {}
    fn focus_moved(
        &mut self,
        _: Option<&accesskit_consumer::NodeRef>,
        _: Option<&accesskit_consumer::NodeRef>,
    ) {
    }
    fn node_removed(&mut self, _: &accesskit_consumer::NodeRef) {}
}

#[test]
fn tab_validation_retry_reordering_and_last_close_preserve_a_valid_focus_target() {
    let (mut r, e, mut ui) = setup(TabContentPolicy::KeepMounted);
    r.update(|cx| e.update(cx, |s, _| s.entries.push(0)));
    assert!(matches!(
        ui.prepare(&mut r, [640., 400.], &mut Measure),
        Err(UiError::DuplicateTabKey)
    ));
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.entries = vec![2, 1, 0];
            s.selected = Some(99.into());
        })
    });
    assert!(matches!(
        ui.prepare(&mut r, [640., 400.], &mut Measure),
        Err(UiError::InvalidTabSelection)
    ));
    r.update(|cx| e.update(cx, |s, _| s.selected = Some(0.into())));
    prepare(&mut r, &mut ui);
    let field = keyed(&ui, "field");
    ui.focus(field);
    r.update(|cx| e.update(cx, |s, _| s.entries.reverse()));
    prepare(&mut r, &mut ui);
    assert_eq!(keyed(&ui, "field"), field);
    assert_eq!(ui.focused_element(), Some(field));
    for name in ["Doc 0", "Doc 1", "Doc 2"] {
        ui.focus(header(&ui, name));
        assert!(key(&mut r, &mut ui, KeyboardKey::Delete).default_prevented);
        prepare(&mut r, &mut ui);
    }
    assert!(r.update(|cx| e.read(cx).entries.is_empty()));
    assert_eq!(r.update(|cx| e.read(cx).selected.clone()), None);
    assert_eq!(ui.focused_element(), Some(keyed(&ui, "outside")));
    assert_eq!(ui.mount_ids().count(), 1);
}
#[test]
fn tab_stop_exclusion_keeps_pointer_programmatic_and_assistive_activation_available() {
    struct Buttons {
        clicks: u32,
    }
    impl View for Buttons {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            row()
                .child(button("First").key("first"))
                .child(
                    button("Skip")
                        .key("skip")
                        .tab_stop(false)
                        .on_click(cx.listener(|s, _, _| s.clicks += 1)),
                )
                .child(button("Last").key("last"))
        }
    }
    let mut r = Runtime::new();
    let e = r.update(|cx| cx.new(|_| Buttons { clicks: 0 }));
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut ui);
    ui.focus(keyed(&ui, "first"));
    ui.focus_next(false);
    assert_eq!(ui.focused_element(), Some(keyed(&ui, "last")));
    let skip = keyed(&ui, "skip");
    ui.focus(skip);
    assert!(ui.activate_focused(&mut r).unwrap());
    prepare(&mut r, &mut ui);
    let b = ui.element(skip).unwrap().bounds;
    let point = [b.x + 2., b.y + 2.];
    ui.pointer(&mut r, PointerEvent::Pressed(point)).unwrap();
    ui.pointer(&mut r, PointerEvent::Released(point)).unwrap();
    prepare(&mut r, &mut ui);
    assert_eq!(r.update(|cx| e.read(cx).clicks), 2);
    assert!(
        ui.semantic_action(&mut r, SemanticAction::Activate(skip), &mut Measure)
            .unwrap()
    );
    assert_eq!(r.update(|cx| e.read(cx).clicks), 3);
}
#[test]
fn tab_navigation_does_not_restore_a_non_tab_stop_but_explicit_focus_can() {
    struct Page {
        focus: FocusHandle,
    }
    impl View for Page {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column()
                .child(button("Outside").key("outside"))
                .child(
                    column()
                        .focus_scope(FocusScope::Group)
                        .focus_handle(self.focus.clone())
                        .child(button("First").key("first"))
                        .child(button("Skip").key("skip").tab_stop(false)),
                )
                .child(
                    button("Restore")
                        .key("restore")
                        .on_click(cx.listener(|s, _, cx| s.focus.focus(cx).unwrap())),
                )
                .child(tabs().selected(0).tab(tab(
                    0,
                    "Only non-tab-stop content",
                    button("Explicit only").tab_stop(false),
                )))
        }
    }
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| Page {
            focus: FocusHandle::new(),
        })
    });
    let mut ui = Ui::new(&mut r, e).unwrap();
    prepare(&mut r, &mut ui);
    let skip = keyed(&ui, "skip");
    ui.focus(skip);
    ui.focus(keyed(&ui, "outside"));
    ui.focus_next(false);
    assert_eq!(ui.focused_element(), Some(keyed(&ui, "first")));
    ui.focus(skip);
    activate(&mut r, &mut ui, "restore");
    assert_eq!(ui.focused_element(), Some(skip));
    let header = ui
        .semantics()
        .find(|n| n.role == SemanticRole::Tab)
        .unwrap()
        .id;
    ui.focus(header);
    ui.focus_next(false);
    assert_eq!(
        ui.semantics()
            .find(|n| Some(n.id) == ui.focused_element())
            .unwrap()
            .role,
        SemanticRole::TabPanel
    );
}

#[test]
fn vertical_tabs_respect_key_prevention_and_do_not_claim_horizontal_arrows() {
    struct Vertical {
        selected: Key,
        prevent: bool,
    }
    impl View for Vertical {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let mut element = tabs()
                .axis(Axis::Vertical)
                .selected(self.selected.clone())
                .tab(tab(0, "First", label("First content")))
                .tab(tab(1, "Second", label("Second content")))
                .on_select(cx.listener(|s, e: &TabSelectEvent, _| s.selected = e.key.clone()))
                .into_element();
            if self.prevent {
                element = element
                    .on_key_down_capture(cx.listener(|_, e: &KeyInput, _| e.prevent_default()));
            }
            element
        }
    }
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| Vertical {
            selected: 0.into(),
            prevent: true,
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut ui);
    let first = ui
        .semantics()
        .find(|n| n.role == SemanticRole::Tab && n.label == Some("First"))
        .unwrap()
        .id;
    ui.focus(first);
    assert!(key(&mut r, &mut ui, KeyboardKey::ArrowDown).default_prevented);
    prepare(&mut r, &mut ui);
    assert_eq!(r.update(|cx| e.read(cx).selected.clone()), 0.into());
    r.update(|cx| e.update(cx, |s, _| s.prevent = false));
    prepare(&mut r, &mut ui);
    assert!(!key(&mut r, &mut ui, KeyboardKey::ArrowRight).default_prevented);
    assert!(key(&mut r, &mut ui, KeyboardKey::ArrowDown).default_prevented);
    prepare(&mut r, &mut ui);
    assert_eq!(r.update(|cx| e.read(cx).selected.clone()), 1.into());
    ui.focus_next(false);
    assert_eq!(
        ui.semantics()
            .find(|n| Some(n.id) == ui.focused_element())
            .unwrap()
            .role,
        SemanticRole::TabPanel
    );
}

#[test]
fn entity_panels_constrain_filling_scroll_areas_and_long_headers_reveal_on_navigation() {
    struct LongDocument;
    impl View for LongDocument {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column()
                .fill_width()
                .fill_height()
                .min_height(0.)
                .child(label("Top").height(24.))
                .child(scroll_area(column().children(
                    (0..100).map(|i| label(format!("Row {i}")).height(24.)),
                )))
        }
    }
    struct Many {
        doc: Entity<LongDocument>,
        selected: Key,
    }
    impl View for Many {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            tabs()
                .size(300., 200.)
                .selected(self.selected.clone())
                .tabs((0..12).map(|i| tab(i, format!("Document {i}"), self.doc.clone())))
                .on_select(cx.listener(|s, e: &TabSelectEvent, _| s.selected = e.key.clone()))
        }
    }
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        let doc = cx.new(|_| LongDocument);
        cx.new(|_| Many {
            doc,
            selected: 0.into(),
        })
    });
    let mut ui = Ui::new(&mut r, e).unwrap();
    prepare(&mut r, &mut ui);
    let viewport = ui.elements().find(|n| n.scroll_range[1] > 0.).unwrap();
    assert!(
        viewport.bounds.height > 0. && viewport.bounds.height < 200.,
        "panel viewport: {:?}",
        viewport.bounds
    );
    let header = ui
        .semantics()
        .find(|n| n.role == SemanticRole::Tab && n.label == Some("Document 0"))
        .unwrap()
        .id;
    ui.focus(header);
    assert!(key(&mut r, &mut ui, KeyboardKey::End).default_prevented);
    prepare(&mut r, &mut ui);
    let strip = ui
        .semantics()
        .find(|n| n.role == SemanticRole::TabList)
        .unwrap();
    assert!(
        strip.scroll_offset[0] > 0.,
        "overflowing headers should reveal the focused last tab"
    );
    let focused = ui.element(ui.focused_element().unwrap()).unwrap();
    assert!(focused.bounds.intersection(focused.clip_bounds).width > 0.);
}
