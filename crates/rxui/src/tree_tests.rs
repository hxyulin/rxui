use crate::*;
use std::{cell::Cell, rc::Rc, time::Duration};
fn node(key: usize, parent: Option<usize>, branch: bool, name: &str) -> TreeNode {
    TreeNode {
        key: key.into(),
        parent: parent.map(Into::into),
        branch,
        label: name.into(),
    }
}
fn data() -> TreeData {
    TreeData::new(vec![
        node(0, None, true, "Alpha"),
        node(1, Some(0), false, "Apple"),
        node(2, Some(0), true, "Berry"),
        node(3, Some(2), false, "Blueberry"),
        node(4, None, false, "Beta"),
    ])
    .unwrap()
}
fn state() -> TreeState {
    let mut s = TreeState::new(data(), TreeSelectionMode::Multiple);
    s.set_expanded([0.into(), 2.into()]);
    s
}
fn apply(s: &mut TreeState, command: TreeCommand, modifiers: Modifiers) {
    let p = s.propose(command, modifiers, 2, Duration::ZERO).unwrap();
    assert!(s.apply(&p));
}
fn select(s: &mut TreeState, key: usize, modifiers: Modifiers) {
    apply(s, TreeCommand::Select(key.into()), modifiers);
}
fn primary() -> Modifiers {
    Modifiers {
        control: !cfg!(target_os = "macos"),
        meta: cfg!(target_os = "macos"),
        ..Modifiers::default()
    }
}
#[test]
fn validates_all_hidden_domain_keys_and_parent_shape() {
    assert_eq!(
        TreeData::new(vec![
            node(0, None, true, "a"),
            node(1, Some(0), false, "b"),
            node(1, None, false, "duplicate")
        ])
        .err(),
        Some(TreeDataError::DuplicateKey(1.into()))
    );
    assert!(TreeData::new(vec![node(1, Some(1), true, "cycle")]).is_err());
    assert!(
        TreeData::new(vec![
            node(0, None, false, "leaf"),
            node(1, Some(0), false, "bad")
        ])
        .is_err()
    );
}
#[test]
fn modifier_selection_activity_ranges_and_single_mode() {
    let mut s = state();
    select(&mut s, 1, Modifiers::default());
    select(&mut s, 4, primary());
    assert_eq!(
        s.selected(),
        &std::collections::HashSet::from([1.into(), 4.into()])
    );
    select(&mut s, 4, primary());
    assert_eq!(s.selected().len(), 1);
    select(&mut s, 1, Modifiers::default());
    apply(&mut s, TreeCommand::Key(KeyboardKey::End), primary());
    assert_eq!(s.active(), Some(&4.into()));
    assert_eq!(s.selected().len(), 1);
    select(
        &mut s,
        3,
        Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(
        s.selected(),
        &std::collections::HashSet::from([1.into(), 2.into(), 3.into()])
    );
    let mut single = TreeState::new(data(), TreeSelectionMode::Single);
    select(&mut single, 0, Modifiers::default());
    select(
        &mut single,
        4,
        Modifiers {
            shift: true,
            ..primary()
        },
    );
    assert_eq!(
        single.selected(),
        &std::collections::HashSet::from([4.into()])
    );
}
#[test]
fn collapsed_keys_survive_deleted_keys_are_pruned_and_ranges_are_deterministic() {
    let mut s = state();
    select(&mut s, 3, Modifiers::default());
    apply(
        &mut s,
        TreeCommand::Expand(0.into(), false),
        Modifiers::default(),
    );
    assert_eq!(s.active(), Some(&0.into()));
    assert!(s.selected().contains(&3.into()));
    assert_eq!(s.anchor(), Some(&3.into()));
    select(
        &mut s,
        4,
        Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(
        s.selected(),
        &std::collections::HashSet::from([0.into(), 4.into()])
    );
    assert_eq!(s.anchor(), Some(&3.into()));
    s.set_data(TreeData::new(vec![node(4, None, false, "Beta")]).unwrap());
    assert_eq!(s.anchor(), None);
    assert_eq!(s.active(), Some(&4.into()));
    assert_eq!(s.selected(), &std::collections::HashSet::from([4.into()]));
    s.set_data(TreeData::new(vec![]).unwrap());
    assert!(s.active().is_none());
    assert!(s.selected().is_empty());
}
#[test]
fn rejected_and_stale_proposals_cannot_mutate_reordered_or_newer_state() {
    let mut s = state();
    let rejected = s
        .propose(
            TreeCommand::Select(3.into()),
            Modifiers::default(),
            2,
            Duration::ZERO,
        )
        .unwrap();
    assert!(s.selected().is_empty());
    assert_eq!(s.active(), Some(&0.into()));
    select(&mut s, 4, Modifiers::default());
    assert!(!s.apply(&rejected));
    let stale = s
        .propose(
            TreeCommand::Expand(0.into(), false),
            Modifiers::default(),
            2,
            Duration::ZERO,
        )
        .unwrap();
    let stale_range = s
        .propose(
            TreeCommand::Key(KeyboardKey::Home),
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
            2,
            Duration::ZERO,
        )
        .unwrap();
    s.set_data(
        TreeData::new(vec![
            node(4, None, false, "Beta"),
            node(0, None, true, "Alpha"),
        ])
        .unwrap(),
    );
    assert!(!s.apply(&stale));
    assert!(!s.apply(&stale_range));
    assert!(s.expanded(&0.into()));
}
#[test]
fn hierarchy_navigation_pages_and_browser_safe_typeahead() {
    let mut s = TreeState::new(data(), TreeSelectionMode::Multiple);
    apply(
        &mut s,
        TreeCommand::Key(KeyboardKey::ArrowRight),
        Modifiers::default(),
    );
    assert!(s.expanded(&0.into()));
    apply(
        &mut s,
        TreeCommand::Key(KeyboardKey::ArrowRight),
        Modifiers::default(),
    );
    assert_eq!(s.active(), Some(&1.into()));
    apply(
        &mut s,
        TreeCommand::Key(KeyboardKey::ArrowLeft),
        Modifiers::default(),
    );
    assert_eq!(s.active(), Some(&0.into()));
    apply(
        &mut s,
        TreeCommand::Key(KeyboardKey::PageDown),
        Modifiers::default(),
    );
    assert_eq!(s.active(), Some(&2.into()));
    apply(
        &mut s,
        TreeCommand::Key(KeyboardKey::ArrowRight),
        Modifiers::default(),
    );
    assert!(s.expanded(&2.into()));
    apply(
        &mut s,
        TreeCommand::Key(KeyboardKey::PageUp),
        Modifiers::default(),
    );
    assert_eq!(s.active(), Some(&0.into()));
    for (text, ms, expected) in [
        ("b", 1, 2),
        ("b", 20, 3),
        ("e", 30, 4),
        ("a", 900, 0),
        ("p", 950, 1),
    ] {
        let p = s
            .propose(
                TreeCommand::Key(KeyboardKey::Character(text.into())),
                Modifiers::default(),
                2,
                Duration::from_millis(ms),
            )
            .unwrap();
        assert!(s.apply(&p));
        assert_eq!(s.active(), Some(&expected.into()));
    }
}
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
    }
}
struct Demo {
    state: TreeState,
    scroll: ScrollHandle,
    builds: Rc<Cell<usize>>,
    reject: bool,
}
impl View for Demo {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let change = cx.listener(|s, p: &TreeProposal, cx| {
            if !s.reject {
                p.accept(&mut s.state, &s.scroll, 20., cx).unwrap();
            }
        });
        virtual_tree(&self.state, 20., &self.scroll, cx, change, |_, n, r| {
            self.builds.set(self.builds.get() + 1);
            label(n.label.clone()).padding_left((r.level - 1) as f32 * 12.)
        })
        .fill_width()
        .fill_height()
    }
}
fn setup(count: usize) -> (Runtime, Entity<Demo>, Ui<Demo>) {
    let mut r = Runtime::new();
    let root = r.update(|cx| {
        cx.new(|_| {
            let nodes = (0..count)
                .map(|i| node(i, (i > 0).then_some(0), i == 0, &format!("Node {i}")))
                .collect();
            let mut state =
                TreeState::new(TreeData::new(nodes).unwrap(), TreeSelectionMode::Multiple);
            state.set_expanded([0.into()]);
            Demo {
                state,
                scroll: ScrollHandle::new(),
                builds: Rc::new(Cell::new(0)),
                reject: false,
            }
        })
    });
    let mut ui = Ui::new(&mut r, root.clone()).unwrap();
    ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
    (r, root, ui)
}
fn tree(ui: &Ui<Demo>) -> SemanticNode<'_> {
    ui.semantics()
        .find(|n| n.role == SemanticRole::Tree)
        .unwrap()
}
fn item(ui: &Ui<Demo>, name: &str) -> ElementId {
    ui.semantics()
        .find(|n| n.role == SemanticRole::TreeItem && n.label == Some(name))
        .unwrap()
        .id
}
fn key(r: &mut Runtime, ui: &mut Ui<Demo>, key: KeyboardKey) {
    ui.key(
        r,
        KeyEvent {
            key,
            physical_key: PhysicalKey::Unidentified,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        },
    )
    .unwrap();
    ui.prepare(r, [200., 100.], &mut Measure).unwrap();
}
fn scroll(r: &mut Runtime, ui: &mut Ui<Demo>, y: f32) {
    let target = ui.semantics().find(|n| n.scroll_range[1] > 0.).unwrap().id;
    ui.semantic_action(
        r,
        SemanticAction::Scroll {
            target,
            offset: [0., y],
        },
        &mut Measure,
    )
    .unwrap();
    ui.prepare(r, [200., 100.], &mut Measure).unwrap();
}
#[test]
fn hundred_thousand_tree_rows_bound_rendering_and_keep_projection_on_scroll() {
    let (mut r, root, mut ui) = setup(100_000);
    r.update(|cx| assert_eq!(root.read(cx).builds.get(), 7));
    assert!(ui.elements().count() < 60);
    assert!(
        !ui.semantics()
            .any(|n| matches!(n.role, SemanticRole::List | SemanticRole::ListItem))
    );
    let projection = r.update(|cx| root.read(cx).state.rows().as_ptr());
    scroll(&mut r, &mut ui, 1_999_900.);
    assert!(ui.elements().count() < 60);
    assert_eq!(
        ui.semantic_node(item(&ui, "Node 99999"))
            .unwrap()
            .position_in_set,
        Some(99_998)
    );
    r.update(|cx| assert_eq!(root.read(cx).state.rows().as_ptr(), projection));
    assert!(tree(&ui).active_descendant.is_none());
}
#[test]
fn root_focus_offscreen_keyboard_reveal_independent_placements_and_rejected_events() {
    let (mut r, root, mut ui) = setup(100_000);
    let mut second = Ui::new(&mut r, root.clone()).unwrap();
    second.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
    let root_id = tree(&ui).id;
    ui.semantic_action(&mut r, SemanticAction::Focus(root_id), &mut Measure)
        .unwrap();
    key(&mut r, &mut ui, KeyboardKey::End);
    assert_eq!(ui.focused_element(), Some(root_id));
    assert_eq!(tree(&ui).active_descendant, Some(item(&ui, "Node 99999")));
    second.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
    assert!(second.semantics().any(|n| n.label == Some("Node 0")));
    assert!(tree(&second).active_descendant.is_none());
    r.update(|cx| root.update(cx, |s, _| s.reject = true));
    ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
    key(&mut r, &mut ui, KeyboardKey::Home);
    assert_eq!(tree(&ui).active_descendant, Some(item(&ui, "Node 99999")));
    let old = item(&ui, "Node 99999");
    scroll(&mut r, &mut ui, 0.);
    assert!(
        !ui.semantic_action(&mut r, SemanticAction::Select(old), &mut Measure)
            .unwrap()
    );
}
#[test]
fn mounted_semantic_expand_collapse_and_select_are_controlled() {
    let (mut r, root, mut ui) = setup(12);
    let id = item(&ui, "Node 0");
    assert!(
        ui.semantic_action(&mut r, SemanticAction::Collapse(id), &mut Measure)
            .unwrap()
    );
    ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
    assert_eq!(
        ui.semantics()
            .filter(|n| n.role == SemanticRole::TreeItem)
            .count(),
        1
    );
    assert!(
        ui.semantic_action(&mut r, SemanticAction::Expand(id), &mut Measure)
            .unwrap()
    );
    ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
    let child = item(&ui, "Node 2");
    assert!(
        ui.semantic_action(&mut r, SemanticAction::Select(child), &mut Measure)
            .unwrap()
    );
    ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
    r.update(|cx| assert!(root.read(cx).state.selected().contains(&2.into())));
}
#[cfg(feature = "accessibility")]
#[test]
fn accesskit_tree_has_valid_bounded_updates_and_collection_actions() {
    let (mut r, _, mut ui) = setup(100_000);
    let mut cache = AccessKitTree::new();
    let first = cache.update(&ui, "Tree", 1.).unwrap().unwrap();
    let parent = first
        .nodes
        .iter()
        .find(|(_, n)| n.role() == accesskit::Role::TreeItem && n.is_expanded() == Some(true))
        .unwrap();
    assert!(parent.1.supports_action(accesskit::Action::Collapse));
    let request = accesskit::ActionRequest {
        action: accesskit::Action::Collapse,
        target_tree: accesskit::TreeId::ROOT,
        target_node: parent.0,
        data: None,
    };
    assert!(matches!(
        cache.action(request),
        Some(SemanticAction::Collapse(_))
    ));
    assert!(
        first
            .nodes
            .iter()
            .find(|(_, n)| n.role() == accesskit::Role::Tree)
            .unwrap()
            .1
            .is_multiselectable()
    );
    let leaf = first
        .nodes
        .iter()
        .find(|(_, n)| n.role() == accesskit::Role::TreeItem && n.is_expanded().is_none())
        .unwrap();
    assert!(!leaf.1.supports_action(accesskit::Action::Expand));
    assert!(matches!(
        cache.action(accesskit::ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: leaf.0,
            data: None
        }),
        Some(SemanticAction::Select(_))
    ));
    let mut consumer = accesskit_consumer::Tree::new(first, true);
    struct Changes;
    impl accesskit_consumer::TreeChangeHandler for Changes {
        fn node_added(&mut self, _: &accesskit_consumer::NodeRef) {}
        fn node_updated(
            &mut self,
            _: &accesskit_consumer::NodeRef,
            _: &accesskit_consumer::NodeRef,
        ) {
        }
        fn node_removed(&mut self, _: &accesskit_consumer::NodeRef) {}
        fn focus_moved(
            &mut self,
            _: Option<&accesskit_consumer::NodeRef>,
            _: Option<&accesskit_consumer::NodeRef>,
        ) {
        }
    }
    let root_id = tree(&ui).id;
    ui.semantic_action(&mut r, SemanticAction::Focus(root_id), &mut Measure)
        .unwrap();
    key(&mut r, &mut ui, KeyboardKey::End);
    let update = cache.update(&ui, "Tree", 1.).unwrap().unwrap();
    assert!(update.nodes.len() < 60);
    consumer.update_and_process_changes(update, &mut Changes);
    scroll(&mut r, &mut ui, 0.);
    consumer.update_and_process_changes(
        cache.update(&ui, "Tree", 1.).unwrap().unwrap(),
        &mut Changes,
    );
}
#[test]
fn shared_domain_independent_states_and_controlled_selection_normalization() {
    let shared = data();
    let mut a = TreeState::new(shared.clone(), TreeSelectionMode::Multiple);
    let b = TreeState::new(shared, TreeSelectionMode::Multiple);
    a.set_expanded([0.into(), 2.into(), 4.into(), 999.into()]);
    a.set_selection([3.into(), 999.into()], Some(3.into()), Some(3.into()));
    assert_eq!(a.active(), Some(&3.into()));
    assert!(!a.expanded(&4.into()));
    assert!(!a.expanded(&999.into()));
    assert_eq!(b.rows().len(), 2);
    assert!(b.selected().is_empty());
    a.set_expanded([]);
    assert_eq!(a.active(), Some(&0.into()));
    assert_eq!(a.anchor(), Some(&3.into()));
    assert!(a.selected().contains(&3.into()));
    let empty_branch = TreeData::new(vec![node(0, None, true, "empty")]).unwrap();
    a.set_data(empty_branch);
    apply(
        &mut a,
        TreeCommand::Key(KeyboardKey::ArrowRight),
        Modifiers::default(),
    );
    assert!(a.expanded(&0.into()));
    assert_eq!(a.rows().len(), 1);
}
#[test]
fn extending_typeahead_keeps_current_match_and_shift_without_anchor_is_stable() {
    let mut s = state();
    for (text, key) in [("b", 2), ("e", 2)] {
        let p = s
            .propose(
                TreeCommand::Key(KeyboardKey::Character(text.into())),
                Modifiers::default(),
                2,
                Duration::ZERO,
            )
            .unwrap();
        assert!(s.apply(&p));
        assert_eq!(s.active(), Some(&key.into()));
    }
    s.set_selection([], Some(1.into()), None);
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    apply(&mut s, TreeCommand::Key(KeyboardKey::ArrowDown), shift);
    apply(&mut s, TreeCommand::Key(KeyboardKey::ArrowDown), shift);
    assert_eq!(s.anchor(), Some(&1.into()));
    assert_eq!(
        s.selected(),
        &std::collections::HashSet::from([1.into(), 2.into(), 3.into()])
    );
}
#[test]
fn passive_row_pointer_focus_and_semantic_capability_guards() {
    let (mut r, root, mut ui) = setup(12);
    let root_id = tree(&ui).id;
    assert!(
        ui.semantics()
            .filter(|n| n.role == SemanticRole::TreeItem)
            .all(|n| !n.focusable)
    );
    let leaf = item(&ui, "Node 2");
    assert!(
        !ui.semantic_action(&mut r, SemanticAction::Expand(leaf), &mut Measure)
            .unwrap()
    );
    assert!(
        !ui.semantic_action(&mut r, SemanticAction::Collapse(leaf), &mut Measure)
            .unwrap()
    );
    assert!(
        !ui.semantic_action(&mut r, SemanticAction::Select(root_id), &mut Measure)
            .unwrap()
    );
    let bounds = ui.elements().find(|n| n.id == leaf).unwrap().bounds;
    ui.pointer(
        &mut r,
        PointerEvent::Pressed([bounds.x + 5., bounds.y + 5.]),
    )
    .unwrap();
    ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
    assert_eq!(ui.focused_element(), Some(root_id));
    r.update(|cx| assert!(root.read(cx).state.selected().contains(&2.into())));
    // The stock bar still scrolls with the pointer, without becoming a Tab stop
    // or stealing persistent tree focus after preparation.
    let bar = ui
        .semantics()
        .find(|n| n.role == SemanticRole::Scrollbar)
        .unwrap();
    assert!(!bar.focusable);
    let position = [
        bar.bounds.x + bar.bounds.width / 2.,
        bar.bounds.y + bar.bounds.height - 5.,
    ];
    ui.pointer(
        &mut r,
        PointerEvent::Down {
            position,
            button: PointerButton::Primary,
            modifiers: Modifiers::default(),
        },
    )
    .unwrap();
    ui.pointer(
        &mut r,
        PointerEvent::Up {
            position,
            button: PointerButton::Primary,
            modifiers: Modifiers::default(),
        },
    )
    .unwrap();
    ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
    assert_eq!(ui.focused_element(), Some(root_id));
    assert!(
        ui.semantics()
            .find(|n| n.scroll_range[1] > 0.)
            .unwrap()
            .scroll_offset[1]
            > 0.
    );
}
fn press(r: &mut Runtime, ui: &mut Ui<Demo>, name: &str, x: f32, modifiers: Modifiers) {
    let bounds = ui
        .elements()
        .find(|n| n.id == item(ui, name))
        .unwrap()
        .bounds;
    let position = [bounds.x + x, bounds.y + 5.];
    ui.pointer(
        r,
        PointerEvent::Down {
            position,
            button: PointerButton::Primary,
            modifiers,
        },
    )
    .unwrap();
    ui.pointer(
        r,
        PointerEvent::Up {
            position,
            button: PointerButton::Primary,
            modifiers,
        },
    )
    .unwrap();
    ui.prepare(r, [200., 100.], &mut Measure).unwrap();
}
#[test]
fn stock_disclosure_pointer_commands_preserve_selection_modifiers_and_root_focus() {
    let (mut r, root, mut ui) = setup(12);
    let root_id = tree(&ui).id;
    // The disclosure follows the row's leading padding.
    let disclosure = ThemeMetrics::balanced().row_padding_x + 5.;
    // The label region selects; the branch stays expanded.
    press(&mut r, &mut ui, "Node 0", 40., Modifiers::default());
    r.update(|cx| {
        let s = &root.read(cx).state;
        assert!(s.expanded(&0.into()));
        assert!(s.selected().contains(&0.into()));
    });
    press(&mut r, &mut ui, "Node 2", 40., Modifiers::default());
    press(&mut r, &mut ui, "Node 3", 40., primary());
    press(
        &mut r,
        &mut ui,
        "Node 4",
        40.,
        Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    );
    r.update(|cx| {
        assert_eq!(
            root.read(cx).state.selected(),
            &std::collections::HashSet::from([3.into(), 4.into()])
        )
    });
    // Modifiers on the disclosure do not turn expansion into selection.
    press(
        &mut r,
        &mut ui,
        "Node 0",
        disclosure,
        Modifiers {
            shift: true,
            ..primary()
        },
    );
    assert_eq!(ui.focused_element(), Some(root_id));
    r.update(|cx| {
        let s = &root.read(cx).state;
        assert!(!s.expanded(&0.into()));
        assert_eq!(
            s.selected(),
            &std::collections::HashSet::from([3.into(), 4.into()])
        );
    });
    press(&mut r, &mut ui, "Node 0", disclosure, Modifiers::default());
    assert_eq!(ui.focused_element(), Some(root_id));
    r.update(|cx| assert!(root.read(cx).state.expanded(&0.into())));
    // Reject a disclosure with unrelated offscreen activity. No reveal is queued.
    key(&mut r, &mut ui, KeyboardKey::End);
    scroll(&mut r, &mut ui, 0.);
    r.update(|cx| root.update(cx, |s, _| s.reject = true));
    ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
    press(&mut r, &mut ui, "Node 0", disclosure, Modifiers::default());
    r.update(|cx| {
        let s = &root.read(cx).state;
        assert!(s.expanded(&0.into()));
        assert_eq!(s.active(), Some(&11.into()));
        assert_eq!(s.selected(), &std::collections::HashSet::from([11.into()]));
    });
    assert_eq!(
        ui.semantics()
            .find(|n| n.scroll_range[1] > 0.)
            .unwrap()
            .scroll_offset[1],
        0.
    );
    assert_eq!(ui.focused_element(), Some(root_id));
}
#[test]
fn invalid_extended_typeahead_falls_back_to_newest_character() {
    let mut s = state();
    for (text, expected) in [("b", 2), ("z", 2), ("a", 0)] {
        let p = s
            .propose(
                TreeCommand::Key(KeyboardKey::Character(text.into())),
                Modifiers::default(),
                2,
                Duration::ZERO,
            )
            .unwrap();
        assert!(s.apply(&p));
        assert_eq!(s.active(), Some(&expected.into()));
    }
}
#[test]
fn widget_typeahead_expires_while_frame_clock_is_idle() {
    let (mut r, root, mut ui) = setup(3);
    r.update(|cx| {
        root.update(cx, |s, _| {
            s.state = TreeState::new(
                TreeData::new(vec![
                    node(0, None, false, "Alpha"),
                    node(1, None, false, "Abacus"),
                    node(2, None, false, "Beta"),
                ])
                .unwrap(),
                TreeSelectionMode::Single,
            );
        })
    });
    ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
    let root_id = tree(&ui).id;
    ui.semantic_action(&mut r, SemanticAction::Focus(root_id), &mut Measure)
        .unwrap();
    key(&mut r, &mut ui, KeyboardKey::Character("a".into()));
    r.update(|cx| assert_eq!(root.read(cx).state.active(), Some(&1.into())));
    // Preparation recreates descriptions; the state's clock origin must survive.
    // No begin_frame occurs, including during this idle interval.
    std::thread::sleep(Duration::from_millis(850));
    key(&mut r, &mut ui, KeyboardKey::Character("b".into()));
    r.update(|cx| {
        assert_eq!(cx.runtime.frames.borrow().1, Duration::ZERO);
        assert_eq!(root.read(cx).state.active(), Some(&2.into()));
    });
}

#[test]
fn uppercase_typeahead_selects_its_match_without_extending_a_range() {
    for shift in [false, true] {
        let mut s = state();
        select(&mut s, 0, Modifiers::default());
        apply(
            &mut s,
            TreeCommand::Key(KeyboardKey::Character(if shift { "B" } else { "b" }.into())),
            Modifiers {
                shift,
                ..Modifiers::default()
            },
        );
        assert_eq!(s.active(), Some(&2.into()));
        assert_eq!(s.selected(), &std::collections::HashSet::from([2.into()]));
        assert_eq!(s.anchor(), Some(&2.into()));
    }
}

#[test]
fn unsupported_modified_tree_keys_leave_application_shortcuts_unprevented() {
    let (mut r, model, mut ui) = setup(20);
    ui.focus(tree(&ui).id);
    let unsupported = [
        Modifiers {
            alt: true,
            ..Modifiers::default()
        },
        Modifiers {
            control: cfg!(target_os = "macos"),
            meta: !cfg!(target_os = "macos"),
            ..Modifiers::default()
        },
    ];
    for modifiers in unsupported {
        for key in [
            KeyboardKey::ArrowLeft,
            KeyboardKey::ArrowDown,
            KeyboardKey::End,
            KeyboardKey::Space,
        ] {
            let result = ui
                .key(
                    &mut r,
                    KeyEvent {
                        key,
                        physical_key: PhysicalKey::Unidentified,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    },
                )
                .unwrap();
            assert!(!result.default_prevented);
            ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
            r.update(|cx| {
                let s = model.read(cx);
                assert_eq!(s.state.active(), Some(&0.into()));
                assert!(s.state.expanded(&0.into()));
                assert!(s.state.selected().is_empty());
            });
        }
    }
    let result = ui
        .key(
            &mut r,
            KeyEvent {
                key: KeyboardKey::ArrowDown,
                physical_key: PhysicalKey::Unidentified,
                pressed: true,
                repeat: false,
                modifiers: Modifiers {
                    shift: true,
                    ..Modifiers::default()
                },
            },
        )
        .unwrap();
    assert!(result.default_prevented);
    r.update(|cx| {
        assert_eq!(
            model.read(cx).state.selected(),
            &std::collections::HashSet::from([0.into(), 1.into()])
        )
    });
}
