use crate::*;

#[derive(Default)]
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([request.text.len() as f32 * 8., 20.])
    }
}
struct Page {
    value: String,
    label: String,
    clicks: u32,
    readonly: bool,
    disabled: bool,
    hidden: bool,
    items: Vec<u32>,
}
impl View for Page {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .width(300.)
            .gap(8.)
            .accessibility_role(SemanticRole::Form)
            .accessibility_label("Test form")
            .child(
                label("Heading")
                    .key("heading")
                    .accessibility_role(SemanticRole::Heading),
            )
            .child(
                text_input(self.value.clone())
                    .key("field")
                    .accessibility_label(self.label.clone())
                    .accessibility_description("Name to use")
                    .read_only(self.readonly)
                    .disabled(self.disabled)
                    .on_change(cx.listener(|this, event: &TextChangeEvent, _| {
                        this.value = event.value.to_uppercase()
                    })),
            )
            .child(
                column()
                    .key("list")
                    .height(40.)
                    .scroll_y()
                    .accessibility_role(SemanticRole::List)
                    .children(self.items.iter().map(|id| {
                        button(format!("Item {id}"))
                            .key(*id)
                            .height(30.)
                            .disabled(self.disabled)
                            .on_click(cx.listener(|this, _, _| this.clicks += 1))
                    })),
            )
            .child(
                column()
                    .key("hidden")
                    .accessibility_hidden(self.hidden)
                    .child(
                        button("Decorative action")
                            .key("decorative")
                            .on_click(cx.listener(|this, _, _| this.clicks += 10)),
                    ),
            )
    }
}
fn setup() -> (Runtime, Entity<Page>, Ui<Page>, Measure) {
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| {
        cx.new(|_| Page {
            value: "e\u{301}👩‍👩‍👧‍👦".into(),
            label: "Name".into(),
            clicks: 0,
            readonly: false,
            disabled: false,
            hidden: true,
            items: vec![1, 2, 3],
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    let mut measure = Measure;
    ui.prepare(&mut runtime, [400., 400.], &mut measure)
        .unwrap();
    (runtime, root, ui, measure)
}
fn keyed(ui: &Ui<Page>, key: impl Into<Key>) -> ElementId {
    let key = key.into();
    ui.elements().find(|e| e.key == Some(&key)).unwrap().id
}
#[test]
fn snapshots_infer_names_roles_values_and_exclude_hidden_subtrees_without_hiding_paint() {
    let (_, _, ui, _) = setup();
    let field = ui.semantic_node(keyed(&ui, "field")).unwrap();
    assert_eq!(field.role, SemanticRole::TextInput);
    assert_eq!(field.label, Some("Name"));
    assert_eq!(field.description, Some("Name to use"));
    assert_eq!(field.value, Some("e\u{301}👩‍👩‍👧‍👦"));
    assert!(field.editable && field.focusable);
    assert_eq!(
        ui.semantic_node(keyed(&ui, 1)).unwrap().label,
        Some("Item 1")
    );
    assert_eq!(
        ui.semantic_node(keyed(&ui, "heading")).unwrap().role,
        SemanticRole::Heading
    );
    assert!(ui.semantic_node(keyed(&ui, "decorative")).is_none());
    assert!(ui.element(keyed(&ui, "decorative")).is_some());
    let offscreen = ui.semantic_node(keyed(&ui, 3)).unwrap();
    assert_eq!(
        offscreen.bounds.intersection(offscreen.clip_bounds).height,
        0.
    );
    assert!(offscreen.focusable);
}
#[test]
fn assistive_focus_reveals_offscreen_controls_and_activation_uses_existing_listener() {
    let (mut runtime, root, mut ui, mut measure) = setup();
    let item = keyed(&ui, 3);
    let stats = ui.stats();
    assert!(
        ui.semantic_action(&mut runtime, SemanticAction::Focus(item), &mut measure)
            .unwrap()
    );
    assert_eq!(ui.focused_element(), Some(item));
    assert!(
        ui.semantic_node(item)
            .unwrap()
            .bounds
            .intersection(ui.semantic_node(item).unwrap().clip_bounds)
            .height
            > 0.
    );
    assert_eq!(ui.stats(), stats);
    assert!(
        ui.semantic_action(&mut runtime, SemanticAction::Activate(item), &mut measure)
            .unwrap()
    );
    assert_eq!(runtime.update(|cx| root.read(cx).clicks), 1);
    assert!(ui.activate_focused(&mut runtime).unwrap());
    assert_eq!(runtime.update(|cx| root.read(cx).clicks), 2);
}
#[test]
fn semantic_values_use_controlled_normalization_and_selection_rejects_stale_utf8_offsets() {
    let (mut runtime, root, mut ui, mut measure) = setup();
    let field = keyed(&ui, "field");
    let revision = ui.semantic_node(field).unwrap().text_revision;
    let selection = TextSelection {
        anchor: TextPosition::new(0),
        focus: TextPosition::new(3),
    };
    assert!(
        ui.semantic_action(
            &mut runtime,
            SemanticAction::SetSelection {
                target: field,
                text_revision: revision,
                selection
            },
            &mut measure
        )
        .unwrap()
    );
    assert_eq!(ui.selected_text(), Some("e\u{301}"));
    assert!(
        !ui.semantic_action(
            &mut runtime,
            SemanticAction::SetSelection {
                target: field,
                text_revision: revision,
                selection: TextSelection::caret(1)
            },
            &mut measure
        )
        .unwrap()
    );
    ui.set_active(false);
    assert!(
        ui.semantic_action(
            &mut runtime,
            SemanticAction::SetValue {
                target: field,
                value: "next".into()
            },
            &mut measure
        )
        .unwrap()
    );
    assert_eq!(runtime.update(|cx| root.read(cx).value.clone()), "NEXT");
    assert!(
        !ui.semantic_action(
            &mut runtime,
            SemanticAction::SetSelection {
                target: field,
                text_revision: revision,
                selection
            },
            &mut measure
        )
        .unwrap()
    );
}
#[test]
fn hidden_disabled_readonly_foreign_and_removed_actions_are_ignored() {
    let (mut runtime, root, mut ui, mut measure) = setup();
    let field = keyed(&ui, "field");
    let hidden = keyed(&ui, "decorative");
    let item = keyed(&ui, 1);
    assert!(
        !ui.semantic_action(&mut runtime, SemanticAction::Activate(hidden), &mut measure)
            .unwrap()
    );
    let mut other = Ui::new(&mut runtime, root.clone()).unwrap();
    other
        .prepare(&mut runtime, [400., 400.], &mut measure)
        .unwrap();
    assert!(
        !ui.semantic_action(
            &mut runtime,
            SemanticAction::Focus(keyed(&other, 1)),
            &mut measure
        )
        .unwrap()
    );
    runtime.update(|cx| root.update(cx, |this, _| this.readonly = true));
    assert!(
        !ui.semantic_action(
            &mut runtime,
            SemanticAction::SetValue {
                target: field,
                value: "x".into()
            },
            &mut measure
        )
        .unwrap()
    );
    ui.prepare(&mut runtime, [400., 400.], &mut measure)
        .unwrap();
    assert!(ui.semantic_node(field).unwrap().read_only);
    runtime.update(|cx| root.update(cx, |this, _| this.disabled = true));
    assert!(
        !ui.semantic_action(&mut runtime, SemanticAction::Activate(item), &mut measure)
            .unwrap()
    );
    runtime.update(|cx| {
        root.update(cx, |this, _| {
            this.items.remove(0);
        })
    });
    assert!(
        !ui.semantic_action(&mut runtime, SemanticAction::Activate(item), &mut measure)
            .unwrap()
    );
    assert_eq!(runtime.update(|cx| root.read(cx).clicks), 0);
}
#[test]
fn semantic_scroll_clamps_offsets_and_preserves_text_layout_counters() {
    let (mut runtime, _, mut ui, mut measure) = setup();
    let list = keyed(&ui, "list");
    let stats = ui.stats();
    assert!(
        ui.semantic_action(
            &mut runtime,
            SemanticAction::Scroll {
                target: list,
                offset: [0., 999.]
            },
            &mut measure
        )
        .unwrap()
    );
    let snapshot = ui.semantic_node(list).unwrap();
    assert_eq!(snapshot.scroll_offset, snapshot.scroll_range);
    assert_eq!(ui.stats(), stats);
    assert!(
        !ui.semantic_action(
            &mut runtime,
            SemanticAction::Scroll {
                target: list,
                offset: [0., f32::NAN]
            },
            &mut measure
        )
        .unwrap()
    );
}

#[cfg(feature = "accessibility")]
mod accesskit_tests {
    use super::*;
    use accesskit::{Action, ActionData, ActionRequest, NodeId, TreeId};
    struct Changes;
    impl accesskit_consumer::TreeChangeHandler for Changes {
        fn node_added(&mut self, _: &accesskit_consumer::NodeRef) {}
        fn node_updated(
            &mut self,
            _: &accesskit_consumer::NodeRef,
            _: &accesskit_consumer::NodeRef,
        ) {
        }
        fn focus_moved(
            &mut self,
            _: Option<&accesskit_consumer::NodeRef>,
            _: Option<&accesskit_consumer::NodeRef>,
        ) {
        }
        fn node_removed(&mut self, _: &accesskit_consumer::NodeRef) {}
    }
    fn request(target: NodeId, action: Action, data: Option<ActionData>) -> ActionRequest {
        ActionRequest {
            target_tree: TreeId::ROOT,
            target_node: target,
            action,
            data,
        }
    }
    fn named(update: &accesskit::TreeUpdate, name: &str) -> NodeId {
        update
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some(name))
            .unwrap()
            .0
    }
    #[test]
    fn consumer_accepts_initial_delta_reorder_removal_and_reactivation_trees() {
        let (mut runtime, root, mut ui, mut measure) = setup();
        let mut cache = AccessKitTree::new();
        let first = cache.update(&ui, "Test", 2.).unwrap().unwrap();
        let button = named(&first, "Item 1");
        let field = named(&first, "Name");
        let field_bounds = ui.semantic_node(keyed(&ui, "field")).unwrap().bounds;
        assert_eq!(
            first
                .nodes
                .iter()
                .find(|(id, _)| *id == field)
                .unwrap()
                .1
                .bounds()
                .unwrap()
                .x0,
            field_bounds.x as f64 * 2.
        );
        let mut consumer = accesskit_consumer::Tree::new(first, true);
        assert_eq!(
            consumer
                .state()
                .node_by_tree_local_id(field, TreeId::ROOT)
                .unwrap()
                .value()
                .as_deref(),
            Some("e\u{301}👩‍👩‍👧‍👦")
        );
        runtime.update(|cx| root.update(cx, |this, _| this.items.reverse()));
        ui.prepare(&mut runtime, [400., 400.], &mut measure)
            .unwrap();
        let delta = cache.update(&ui, "Test", 2.).unwrap().unwrap();
        assert!(delta.tree.is_none());
        assert!(cache.action(request(button, Action::Click, None)).is_some());
        consumer.update_and_process_changes(delta, &mut Changes);
        runtime.update(|cx| root.update(cx, |this, _| this.items.retain(|id| *id != 1)));
        ui.prepare(&mut runtime, [400., 400.], &mut measure)
            .unwrap();
        let delta = cache.update(&ui, "Test", 2.).unwrap().unwrap();
        consumer.update_and_process_changes(delta, &mut Changes);
        assert!(
            consumer
                .state()
                .node_by_tree_local_id(button, TreeId::ROOT)
                .is_none()
        );
        assert!(cache.action(request(button, Action::Click, None)).is_none());
        cache.reset();
        let full = cache.update(&ui, "Test", 2.).unwrap().unwrap();
        assert!(full.tree.is_some());
        accesskit_consumer::Tree::new(full, true);
    }
    #[test]
    fn hover_capture_and_blink_do_no_publication_work_while_selection_sends_only_parent() {
        let (mut runtime, _, mut ui, mut measure) = setup();
        let mut cache = AccessKitTree::new();
        let first = cache.update(&ui, "Test", 1.).unwrap().unwrap();
        let field = named(&first, "Name");
        let stats = cache.stats();
        for _ in 0..10 {
            ui.set_caret_visible(false);
            ui.pointer(&mut runtime, PointerEvent::Moved([10., 10.]))
                .unwrap();
            assert!(cache.update(&ui, "Test", 1.).unwrap().is_none());
        }
        assert_eq!(cache.stats(), stats);
        let target = keyed(&ui, "field");
        ui.semantic_action(&mut runtime, SemanticAction::Focus(target), &mut measure)
            .unwrap();
        let focused = cache.update(&ui, "Test", 1.).unwrap().unwrap();
        assert_eq!(focused.focus, field);
        ui.text_input(&mut runtime, TextInputEvent::SelectAll, &mut measure)
            .unwrap();
        let delta = cache.update(&ui, "Test", 1.).unwrap().unwrap();
        assert_eq!(delta.nodes.len(), 1);
        assert_eq!(delta.nodes[0].0, field);
        assert_eq!(cache.stats().prepared_text_runs, stats.prepared_text_runs);
    }
    #[test]
    fn native_grapheme_selection_round_trips_and_old_runs_cannot_select_new_values() {
        let (mut runtime, root, mut ui, mut measure) = setup();
        let mut cache = AccessKitTree::new();
        let first = cache.update(&ui, "Test", 1.).unwrap().unwrap();
        let field = named(&first, "Name");
        let node = &first.nodes.iter().find(|(id, _)| *id == field).unwrap().1;
        let run = node.children()[0];
        let selection = accesskit::TextSelection {
            anchor: accesskit::TextPosition {
                node: run,
                character_index: 0,
            },
            focus: accesskit::TextPosition {
                node: run,
                character_index: 1,
            },
        };
        let action = cache
            .action(request(
                field,
                Action::SetTextSelection,
                Some(ActionData::SetTextSelection(selection)),
            ))
            .unwrap();
        assert!(
            ui.semantic_action(&mut runtime, action.clone(), &mut measure)
                .unwrap()
        );
        assert_eq!(ui.selected_text(), Some("e\u{301}"));
        runtime.update(|cx| root.update(cx, |this, _| this.value = "new".into()));
        assert!(
            !ui.semantic_action(&mut runtime, action, &mut measure)
                .unwrap()
        );
        ui.prepare(&mut runtime, [400., 400.], &mut measure)
            .unwrap();
        cache.update(&ui, "Test", 1.).unwrap();
        assert!(
            cache
                .action(request(
                    field,
                    Action::SetTextSelection,
                    Some(ActionData::SetTextSelection(selection))
                ))
                .is_none()
        );
    }
    #[test]
    fn composition_suppresses_native_selection_and_empty_values_have_valid_text_ranges() {
        let (mut runtime, root, mut ui, mut measure) = setup();
        let mut cache = AccessKitTree::new();
        let first = cache.update(&ui, "Test", 1.).unwrap().unwrap();
        let field = named(&first, "Name");
        ui.semantic_action(
            &mut runtime,
            SemanticAction::Focus(keyed(&ui, "field")),
            &mut measure,
        )
        .unwrap();
        ui.text_input(
            &mut runtime,
            TextInputEvent::Preedit {
                text: "你".into(),
                cursor: Some((3, 3)),
            },
            &mut measure,
        )
        .unwrap();
        ui.prepare(&mut runtime, [400., 400.], &mut measure)
            .unwrap();
        let delta = cache.update(&ui, "Test", 1.).unwrap().unwrap();
        let node = &delta.nodes.iter().find(|(id, _)| *id == field).unwrap().1;
        assert!(!node.supports_action(Action::SetTextSelection));
        runtime.update(|cx| root.update(cx, |this, _| this.value.clear()));
        ui.prepare(&mut runtime, [400., 400.], &mut measure)
            .unwrap();
        cache.reset();
        let full = cache.update(&ui, "Test", 1.).unwrap().unwrap();
        let consumer = accesskit_consumer::Tree::new(full, true);
        assert_eq!(
            consumer
                .state()
                .node_by_tree_local_id(field, TreeId::ROOT)
                .unwrap()
                .value()
                .as_deref(),
            Some("")
        );
    }
    #[test]
    fn native_scroll_units_are_converted_from_physical_to_logical_coordinates() {
        let (mut runtime, _, mut ui, mut measure) = setup();
        let mut cache = AccessKitTree::new();
        let first = cache.update(&ui, "Test", 2.).unwrap().unwrap();
        let scroll = first
            .nodes
            .iter()
            .find(|(_, n)| n.supports_action(Action::SetScrollOffset))
            .unwrap()
            .0;
        assert!(
            cache
                .action(request(
                    scroll,
                    Action::ScrollDown,
                    Some(ActionData::Value("invalid".into())),
                ))
                .is_none()
        );
        let action = cache
            .action(request(
                scroll,
                Action::SetScrollOffset,
                Some(ActionData::SetScrollOffset(accesskit::Point::new(0., 40.))),
            ))
            .unwrap();
        ui.semantic_action(&mut runtime, action, &mut measure)
            .unwrap();
        assert_eq!(
            ui.semantic_node(keyed(&ui, "list")).unwrap().scroll_offset,
            [0., 20.]
        );
    }
    #[test]
    fn oversized_graphemes_publish_values_without_invalid_text_selection_units() {
        let (mut runtime, root, mut ui, mut measure) = setup();
        let value = format!("a{}", "\u{301}".repeat(200));
        runtime.update(|cx| root.update(cx, |this, _| this.value = value.clone()));
        ui.prepare(&mut runtime, [400., 400.], &mut measure)
            .unwrap();
        let mut cache = AccessKitTree::new();
        let full = cache.update(&ui, "Test", 1.).unwrap().unwrap();
        let field = named(&full, "Name");
        let node = &full.nodes.iter().find(|(id, _)| *id == field).unwrap().1;
        assert!(node.children().is_empty());
        assert!(!node.supports_action(Action::SetTextSelection));
        assert!(node.supports_action(Action::SetValue));
        let consumer = accesskit_consumer::Tree::new(full, true);
        assert_eq!(
            consumer
                .state()
                .node_by_tree_local_id(field, TreeId::ROOT)
                .unwrap()
                .value()
                .as_deref(),
            Some(value.as_str())
        );
        runtime.update(|cx| root.update(cx, |this, _| this.value = "short".into()));
        ui.prepare(&mut runtime, [400., 400.], &mut measure)
            .unwrap();
        let delta = cache.update(&ui, "Test", 1.).unwrap().unwrap();
        let node = &delta.nodes.iter().find(|(id, _)| *id == field).unwrap().1;
        assert!(node.supports_action(Action::SetTextSelection));
    }
    #[test]
    fn different_placements_require_separate_translation_caches() {
        let (mut runtime, root, ui, mut measure) = setup();
        let mut other = Ui::new(&mut runtime, root).unwrap();
        other
            .prepare(&mut runtime, [400., 400.], &mut measure)
            .unwrap();
        let mut cache = AccessKitTree::new();
        cache.update(&ui, "Test", 1.).unwrap();
        assert!(matches!(
            cache.update(&other, "Test", 1.),
            Err(UiError::InvalidGeometry)
        ));
    }
}
