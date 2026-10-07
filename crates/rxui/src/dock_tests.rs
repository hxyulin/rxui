use crate::*;

fn key(s: &str) -> Key {
    s.into()
}
fn invariant(tree: &DockTree) {
    fn walk(
        n: &DockNode,
        root: bool,
        nodes: &mut std::collections::HashSet<DockNodeId>,
        keys: &mut std::collections::HashSet<Key>,
    ) {
        assert!(nodes.insert(n.id()));
        match n {
            DockNode::Tabs(n) => {
                assert!(root || !n.panels().is_empty());
                assert_eq!(n.selected().is_none(), n.panels().is_empty());
                assert!(n.selected().is_none_or(|k| n.panels().contains(k)));
                for k in n.panels() {
                    assert!(keys.insert(k.clone()));
                }
            }
            DockNode::Split(n) => {
                assert!(n.position().valid());
                walk(n.first(), false, nodes, keys);
                walk(n.second(), false, nodes, keys);
            }
        }
    }
    walk(
        tree.root(),
        true,
        &mut Default::default(),
        &mut Default::default(),
    );
}
#[test]
fn dock_topology_sides_nested_collapse_and_surviving_identity() {
    for (side, axis, before) in [
        (DockSide::Left, Axis::Horizontal, true),
        (DockSide::Right, Axis::Horizontal, false),
        (DockSide::Top, Axis::Vertical, true),
        (DockSide::Bottom, Axis::Vertical, false),
    ] {
        let mut t = DockTree::from_panels(["editor", "preview"]).unwrap();
        let group = t.root().id();
        let new = t
            .split(group, side, "output", SplitPosition::Pixels(220.))
            .unwrap();
        let split = t.root().split().unwrap();
        assert_eq!(split.axis(), axis);
        assert_eq!(split.position(), SplitPosition::Pixels(220.));
        assert_eq!(split.first().id(), if before { new } else { group });
        assert_eq!(split.second().id(), if before { group } else { new });
        let removed = split.id();
        let files = t
            .split(new, DockSide::Right, "files", SplitPosition::Fraction(0.4))
            .unwrap();
        assert!(t.remove(&key("output")));
        assert_eq!(t.group_for(&key("files")), Some(files));
        assert!(t.remove(&key("files")));
        assert_eq!(t.root().id(), group);
        assert_eq!(t.node(removed), None);
        assert!(t.remove(&key("editor")));
        assert_eq!(t.root().tabs().unwrap().selected(), Some(&key("preview")));
        assert!(t.remove(&key("preview")));
        assert_eq!(t.root().id(), group);
        assert!(!t.remove(&key("preview")));
        invariant(&t);
        t.split(group, side, "again", SplitPosition::default())
            .unwrap();
        assert!(t.root().tabs().is_some());
        invariant(&t);
    }
}
#[test]
fn dock_reorder_moves_and_split_off_preserve_selection_contracts() {
    let mut t = DockTree::from_panels(["a", "b", "c"]).unwrap();
    let main = t.root().id();
    t.move_panel(&key("c"), main, 0).unwrap();
    assert_eq!(
        t.root().tabs().unwrap().panels(),
        &[key("c"), key("a"), key("b")]
    );
    assert_eq!(t.root().tabs().unwrap().selected(), Some(&key("c")));
    assert!(!t.move_panel(&key("c"), main, 0).unwrap());
    let new = t
        .dock_panel(
            &key("c"),
            main,
            DockSide::Bottom,
            SplitPosition::Fraction(0.6),
        )
        .unwrap();
    assert_eq!(
        t.node(main).unwrap().tabs().unwrap().selected(),
        Some(&key("a"))
    );
    t.move_panel(&key("a"), new, 1).unwrap();
    assert_eq!(
        t.node(main).unwrap().tabs().unwrap().selected(),
        Some(&key("b"))
    );
    t.move_panel(&key("b"), new, 0).unwrap();
    assert_eq!(t.root().id(), new);
    assert_eq!(t.node(main), None);
    invariant(&t);
    let other = t
        .dock_panel(&key("a"), new, DockSide::Left, SplitPosition::Fraction(0.3))
        .unwrap();
    let replacement = t
        .dock_panel(&key("a"), new, DockSide::Top, SplitPosition::Fraction(0.2))
        .unwrap();
    assert_eq!(t.node(other), None);
    assert_ne!(other, replacement);
    invariant(&t);
}
#[test]
fn dock_failed_edits_and_stale_proposals_are_atomic() {
    assert_eq!(
        DockTree::from_panels(["a", "a"]).unwrap_err(),
        DockError::DuplicatePanel
    );
    let mut t = DockTree::from_panels(["a", "b"]).unwrap();
    let main = t.root().id();
    let other = t
        .split(main, DockSide::Right, "c", SplitPosition::default())
        .unwrap();
    let split = t.root().id();
    let foreign = DockTree::new().root().id();
    let before = t.clone();
    assert_eq!(t.insert(main, 3, "d"), Err(DockError::InvalidIndex));
    assert_eq!(t.insert(main, 0, "c"), Err(DockError::DuplicatePanel));
    assert_eq!(t.select(main, &key("c")), Err(DockError::PanelNotFound));
    assert_eq!(t.select(foreign, &key("a")), Err(DockError::NodeNotFound));
    assert_eq!(
        t.move_panel(&key("a"), split, 0),
        Err(DockError::NotTabGroup)
    );
    assert_eq!(
        t.move_panel(&key("a"), main, 2),
        Err(DockError::InvalidIndex)
    );
    assert_eq!(
        t.resize(main, SplitPosition::default()),
        Err(DockError::NotSplit)
    );
    for value in [
        SplitPosition::Fraction(f32::NAN),
        SplitPosition::Fraction(1.1),
        SplitPosition::Pixels(-1.),
    ] {
        assert_eq!(t.resize(split, value), Err(DockError::InvalidPosition));
        assert_eq!(
            t.split(main, DockSide::Right, "d", value),
            Err(DockError::InvalidPosition)
        );
    }
    assert_eq!(
        t.dock_panel(&key("c"), other, DockSide::Left, SplitPosition::default()),
        Err(DockError::CannotSplitSolePanel)
    );
    assert_eq!(t, before);
    let stale = DockEvent::Close {
        group: main,
        panel: key("a"),
    };
    t.move_panel(&key("a"), other, 0).unwrap();
    let before = t.clone();
    assert_eq!(t.apply(&stale), Err(DockError::PanelNotFound));
    assert_eq!(t, before);
    invariant(&t);
}
#[test]
fn dock_many_edits_keep_global_uniqueness_and_collapsed_branches() {
    let mut t = DockTree::from_panels([0_u32]).unwrap();
    for i in 1_u32..64 {
        let target = t.group_for(&Key::from(i - 1)).unwrap();
        t.split(
            target,
            if i % 2 == 0 {
                DockSide::Right
            } else {
                DockSide::Bottom
            },
            i,
            SplitPosition::default(),
        )
        .unwrap();
        invariant(&t);
    }
    for i in 0_u32..63 {
        let target = t.group_for(&Key::from(63_u32)).unwrap();
        t.move_panel(&Key::from(i), target, 0).unwrap();
        invariant(&t);
    }
    assert!(t.root().tabs().is_some());
    for i in 0_u32..64 {
        assert!(t.remove(&Key::from(i)));
        invariant(&t);
    }
}

struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
    }
}
struct Document {
    name: String,
    scroll: ScrollHandle,
}
impl View for Document {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .fill_width()
            .fill_height()
            .min_height(0.)
            .child(
                text_input(self.name.clone())
                    .key(self.name.clone())
                    .accessibility_label(self.name.clone())
                    .on_change(cx.listener(|s, e: &TextChangeEvent, _| s.name = e.value.clone())),
            )
            .child(
                scroll_area(
                    column().children((0..80).map(|i| label(format!("Row {i}")).height(24.))),
                )
                .handle(self.scroll.clone()),
            )
    }
}
struct Workspace {
    tree: DockTree,
    docs: std::collections::HashMap<Key, Entity<Document>>,
    accept: bool,
    events: Vec<DockEvent>,
    policy: TabContentPolicy,
    minimum: [f32; 2],
    divider: f32,
}
impl View for Workspace {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        dock(&self.tree, |key| {
            dock_panel(format!("{key:?}"), self.docs[key].clone()).closable(true)
        })
        .key("dock")
        .content_policy(self.policy)
        .min_pane_size(self.minimum[0], self.minimum[1])
        .divider_size(self.divider)
        .on_event(cx.listener(|s, e: &DockEvent, _| {
            s.events.push(e.clone());
            if s.accept {
                let _ = s.tree.apply(e);
            }
        }))
    }
}
fn setup() -> (
    Runtime,
    Entity<Workspace>,
    Ui<Workspace>,
    DockNodeId,
    DockNodeId,
) {
    let mut r = Runtime::new();
    let mut tree = DockTree::from_panels(["editor", "preview"]).unwrap();
    let main = tree.root().id();
    let output = tree
        .split(
            main,
            DockSide::Bottom,
            "output",
            SplitPosition::Fraction(0.6),
        )
        .unwrap();
    tree.split(
        output,
        DockSide::Right,
        "files",
        SplitPosition::Fraction(0.5),
    )
    .unwrap();
    let docs = r.update(|cx| {
        ["editor", "preview", "output", "files"]
            .into_iter()
            .map(|name| {
                (
                    key(name),
                    cx.new(|_| Document {
                        name: name.into(),
                        scroll: ScrollHandle::new(),
                    }),
                )
            })
            .collect()
    });
    let e = r.update(|cx| {
        cx.new(|_| Workspace {
            tree,
            docs,
            accept: true,
            events: Vec::new(),
            policy: TabContentPolicy::KeepMounted,
            minimum: [120., 100.],
            divider: 8.,
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut ui);
    (r, e, ui, main, output)
}
fn prepare(r: &mut Runtime, ui: &mut Ui<Workspace>) {
    ui.prepare(r, [800., 600.], &mut Measure).unwrap();
}
fn field(ui: &Ui<Workspace>, name: &str) -> ElementId {
    ui.elements()
        .find(|e| e.kind == ElementType::TextInput && e.key == Some(&key(name)))
        .unwrap()
        .id
}
fn header(ui: &Ui<Workspace>, name: &str) -> ElementId {
    ui.semantics()
        .find(|n| {
            n.role == SemanticRole::Tab && n.label == Some(format!("{:?}", key(name)).as_str())
        })
        .unwrap()
        .id
}
#[test]
fn dock_nested_layout_resize_and_semantics_keep_placement_identity() {
    let (mut r, e, mut ui, _, _) = setup();
    let dividers: Vec<_> = ui
        .elements()
        .filter(|n| n.kind == ElementType::Splitter)
        .map(|n| (n.id, n.bounds))
        .collect();
    assert_eq!(dividers.len(), 2);
    assert_eq!(
        ui.semantics()
            .filter(|n| n.role == SemanticRole::TabPanel)
            .count(),
        3
    );
    for name in ["editor", "output", "files"] {
        let id = field(&ui, name);
        let n = ui.element(id).unwrap();
        assert!(
            n.bounds.width > 100. && n.bounds.height > 0. && n.bounds.x + n.bounds.width <= 800.1
        );
        let doc = r.update(|cx| e.read(cx).docs[&key(name)].clone());
        assert!(
            ui.elements()
                .filter(|n| n.scroll_range[1] > 0.)
                .all(|n| n.bounds.height > 0. && n.bounds.height < 600.)
        );
        assert!(r.update(|cx| doc.read(cx).name == name));
    }
    let (id, b) = dividers[0];
    let before = field(&ui, "editor");
    ui.semantic_action(
        &mut r,
        SemanticAction::SetNumericValue {
            target: id,
            value: 250.,
        },
        &mut Measure,
    )
    .unwrap();
    prepare(&mut r, &mut ui);
    assert_eq!(field(&ui, "editor"), before);
    assert_eq!(ui.element(id).unwrap().id, id);
    assert_ne!(ui.element(id).unwrap().bounds, b);
    assert!(r.update(|cx| matches!(e.read(cx).events.last(),Some(DockEvent::Resize {event,..}) if event.phase==ResizePhase::Accessibility)));
    ui.prepare(&mut r, [80., 70.], &mut Measure).unwrap();
    assert!(ui.elements().all(|n| n.bounds.width.is_finite()
        && n.bounds.height.is_finite()
        && n.bounds.width >= 0.
        && n.bounds.height >= 0.));
    assert!(ui.semantics().filter_map(|n| n.range).all(|n| n.read_only));
}
#[test]
fn dock_selection_reorder_and_close_are_controlled_and_same_group_state_is_retained() {
    let (mut r, e, mut ui, main, _) = setup();
    let editor = field(&ui, "editor");
    ui.focus(editor);
    ui.text_input(
        &mut r,
        TextInputEvent::Move {
            movement: TextMovement::End,
            extend: false,
        },
        &mut Measure,
    )
    .unwrap();
    let selection = ui.element(editor).unwrap().editing.unwrap().selection;
    r.update(|cx| e.update(cx, |s, _| s.accept = false));
    prepare(&mut r, &mut ui);
    ui.semantic_action(
        &mut r,
        SemanticAction::Activate(header(&ui, "preview")),
        &mut Measure,
    )
    .unwrap();
    prepare(&mut r, &mut ui);
    assert_eq!(
        r.update(|cx| e
            .read(cx)
            .tree
            .node(main)
            .unwrap()
            .tabs()
            .unwrap()
            .selected()
            .cloned()),
        Some(key("editor"))
    );
    r.update(|cx| e.update(cx, |s, _| s.accept = true));
    prepare(&mut r, &mut ui);
    ui.semantic_action(
        &mut r,
        SemanticAction::Activate(header(&ui, "preview")),
        &mut Measure,
    )
    .unwrap();
    prepare(&mut r, &mut ui);
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.tree.move_panel(&key("editor"), main, 1).unwrap();
        })
    });
    prepare(&mut r, &mut ui);
    assert_eq!(field(&ui, "editor"), editor);
    assert_eq!(
        ui.element(editor).unwrap().editing.unwrap().selection,
        selection
    );
    let close = ui
        .semantics()
        .find(|n| n.label == Some("Close String(\"editor\")"))
        .unwrap()
        .id;
    ui.semantic_action(&mut r, SemanticAction::Activate(close), &mut Measure)
        .unwrap();
    prepare(&mut r, &mut ui);
    assert!(ui.element(editor).is_none());
    assert!(r.update(|cx| e.read(cx).docs.contains_key(&key("editor"))));
}
#[test]
fn dock_topology_reparents_entities_and_cancels_removed_split_capture() {
    let (mut r, e, mut ui, main, output) = setup();
    let old = field(&ui, "editor");
    let divider = ui
        .elements()
        .find(|n| n.kind == ElementType::Splitter)
        .unwrap();
    let (id, b) = (divider.id, divider.bounds);
    ui.pointer(&mut r, PointerEvent::Pressed([b.x + 2., b.y + 2.]))
        .unwrap();
    assert_eq!(ui.captured_pointer(), Some(id));
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.tree.remove(&key("output"));
            s.tree.remove(&key("files"));
        })
    });
    prepare(&mut r, &mut ui);
    assert_eq!(ui.captured_pointer(), None);
    assert!(ui.element(id).is_none());
    assert!(ui.element(old).is_none());
    let new = field(&ui, "editor");
    assert_ne!(new, old);
    assert_eq!(r.update(|cx| e.read(cx).tree.root().id()), main);
    assert!(r.update(|cx| e.read(cx).tree.node(output).is_none()));
    let before = ui.mount_ids().count();
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.policy = TabContentPolicy::MountSelected;
            s.tree.select(main, &key("preview")).unwrap();
        })
    });
    prepare(&mut r, &mut ui);
    assert!(ui.element(new).is_none());
    assert!(ui.mount_ids().count() < before);
}
#[test]
fn dock_invalid_configuration_fails_before_mounting_and_can_retry() {
    let (mut r, e, mut ui, _, _) = setup();
    for (minimum, divider) in [
        ([f32::NAN, 100.], 8.),
        ([100., -1.], 8.),
        ([100., 100.], 0.),
    ] {
        r.update(|cx| {
            e.update(cx, |s, _| {
                s.minimum = minimum;
                s.divider = divider;
            })
        });
        assert!(matches!(
            ui.prepare(&mut r, [800., 600.], &mut Measure),
            Err(UiError::InvalidDockConfiguration)
        ));
    }
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.minimum = [0., 0.];
            s.divider = 4.;
        })
    });
    prepare(&mut r, &mut ui);
    assert!(ui.is_prepared());
}
