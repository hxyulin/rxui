use crate::*;
use std::{cell::Cell, rc::Rc};
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
    }
}
struct Rows {
    count: usize,
    height: f32,
    first: usize,
    handle: ScrollHandle,
    builds: Rc<Cell<usize>>,
}
impl View for Rows {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        virtual_list(self.count, self.height, &self.handle, cx, |index| {
            self.builds.set(self.builds.get() + 1);
            button(format!("row{}", index + self.first))
                .key(index + self.first)
                .padding(0.)
                .on_click(cx.listener(|s, _, cx| {
                    s.handle.reveal_row(cx, 50_000, 20.).unwrap();
                }))
        })
        .overscan(2)
        .fill_width()
        .fill_height()
    }
}
fn setup(count: usize) -> (Runtime, Entity<Rows>, Ui<Rows>) {
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| {
        cx.new(|_| Rows {
            count,
            height: 20.,
            first: 0,
            handle: ScrollHandle::new(),
            builds: Rc::new(Cell::new(0)),
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    (runtime, root, ui)
}
fn viewport(ui: &Ui<Rows>) -> ElementId {
    ui.semantics().find(|n| n.scroll_range[1] > 0.).unwrap().id
}
fn scroll(runtime: &mut Runtime, ui: &mut Ui<Rows>, y: f32) {
    ui.semantic_action(
        runtime,
        SemanticAction::Scroll {
            target: viewport(ui),
            offset: [0., y],
        },
        &mut Measure,
    )
    .unwrap();
    ui.prepare(runtime, [200., 100.], &mut Measure).unwrap();
}
fn row(ui: &Ui<Rows>, name: &str) -> Option<ElementId> {
    ui.semantics()
        .find(|n| n.role == SemanticRole::Button && n.label == Some(name))
        .map(|n| n.id)
}
#[test]
fn hundred_thousand_rows_have_bounded_builds_nodes_and_correct_extent() {
    let (mut runtime, root, mut ui) = setup(100_000);
    runtime.update(|cx| assert_eq!(root.read(cx).builds.get(), 7));
    assert!(ui.elements().count() < 25);
    assert_eq!(
        ui.semantic_node(viewport(&ui)).unwrap().scroll_range[1],
        1_999_900.
    );
    assert_eq!(
        ui.semantics()
            .find(|n| n.role == SemanticRole::List)
            .unwrap()
            .set_size,
        Some(100_000)
    );
    scroll(&mut runtime, &mut ui, 1_999_900.);
    assert!(row(&ui, "row99999").is_some());
    assert!(row(&ui, "row0").is_none());
    assert!(ui.elements().count() < 25);
    assert_eq!(
        ui.semantics()
            .filter_map(|n| n.position_in_set)
            .collect::<Vec<_>>(),
        (99_993..100_000).collect::<Vec<_>>()
    );
}
#[test]
fn overlapping_rows_keep_identity_and_data_keys_survive_insertions() {
    let (mut runtime, root, mut ui) = setup(1000);
    let id = row(&ui, "row4").unwrap();
    scroll(&mut runtime, &mut ui, 40.);
    assert_eq!(row(&ui, "row4"), Some(id));
    runtime.update(|cx| {
        root.update(cx, |s, _| {
            s.first = 1;
            s.count -= 1;
        })
    });
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    assert_eq!(row(&ui, "row4"), Some(id));
    let bounds = ui.elements().find(|n| n.id == id).unwrap().bounds;
    assert_eq!(bounds.y, 20.);
}
#[test]
fn shrinking_empty_resize_and_fractional_scroll_settle_without_stale_rows() {
    let (mut runtime, root, mut ui) = setup(1000);
    scroll(&mut runtime, &mut ui, 5000.5);
    assert!(row(&ui, "row250").is_some());
    runtime.update(|cx| root.update(cx, |s, _| s.count = 3));
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    assert_eq!(
        ui.semantics()
            .filter(|n| n.role == SemanticRole::Button)
            .count(),
        3
    );
    assert_eq!(
        ui.elements()
            .find(|n| n.id == row(&ui, "row0").unwrap())
            .unwrap()
            .bounds
            .y,
        0.
    );
    runtime.update(|cx| root.update(cx, |s, _| s.count = 0));
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    assert_eq!(
        ui.semantics()
            .filter(|n| n.role == SemanticRole::Button)
            .count(),
        0
    );
    runtime.update(|cx| root.update(cx, |s, _| s.count = 1000));
    ui.prepare(&mut runtime, [200., 0.], &mut Measure).unwrap();
    assert_eq!(
        ui.elements()
            .filter(|n| n.kind == ElementType::Button)
            .count(),
        0
    );
    ui.prepare(&mut runtime, [200., 200.], &mut Measure)
        .unwrap();
    assert_eq!(
        ui.semantics()
            .filter(|n| n.role == SemanticRole::Button)
            .count(),
        12
    );
}
#[test]
fn shared_model_scrolls_independently_and_reveal_row_mounts_requested_data() {
    let (mut runtime, root, mut ui) = setup(100_000);
    let mut second = Ui::new(&mut runtime, root.clone()).unwrap();
    second
        .prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    scroll(&mut runtime, &mut ui, 2000.);
    second
        .prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    assert!(row(&ui, "row100").is_some());
    assert!(row(&second, "row0").is_some());
    let bounds = ui
        .elements()
        .find(|n| n.id == row(&ui, "row100").unwrap())
        .unwrap()
        .bounds;
    ui.pointer(&mut runtime, PointerEvent::Pressed([10., bounds.y + 5.]))
        .unwrap();
    ui.pointer(&mut runtime, PointerEvent::Released([10., bounds.y + 5.]))
        .unwrap();
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    assert!(row(&ui, "row50000").is_some());
    assert!(row(&second, "row0").is_some());
}
#[test]
fn removing_focused_row_releases_focus_and_stale_semantic_targets() {
    let (mut runtime, _, mut ui) = setup(1000);
    let id = row(&ui, "row0").unwrap();
    ui.semantic_action(&mut runtime, SemanticAction::Focus(id), &mut Measure)
        .unwrap();
    assert_eq!(ui.semantic_focus(), Some(id));
    scroll(&mut runtime, &mut ui, 2000.);
    assert_eq!(ui.semantic_focus(), None);
    assert!(
        !ui.semantic_action(&mut runtime, SemanticAction::Focus(id), &mut Measure)
            .unwrap()
    );
}
#[test]
fn invalid_geometry_never_invokes_the_row_builder() {
    for (count, height) in [
        (1, 0.),
        (1, -1.),
        (1, f32::NAN),
        (1, f32::INFINITY),
        (usize::MAX, 20.),
    ] {
        let (mut runtime, root, mut ui) = setup(0);
        runtime.update(|cx| {
            root.update(cx, |s, _| {
                s.count = count;
                s.height = height;
            })
        });
        assert!(matches!(
            ui.prepare(&mut runtime, [200., 100.], &mut Measure),
            Err(UiError::InvalidStyle)
        ));
        runtime.update(|cx| assert_eq!(root.read(cx).builds.get(), 0));
    }
}
#[cfg(feature = "accessibility")]
#[test]
fn accesskit_virtual_set_metadata_and_removed_rows_form_a_valid_bounded_tree() {
    let (mut r, _, mut ui) = setup(100_000);
    let mut cache = AccessKitTree::new();
    let first = cache.update(&ui, "Virtual list", 2.).unwrap().unwrap();
    let list = first
        .nodes
        .iter()
        .find(|(_, n)| n.role() == accesskit::Role::List)
        .unwrap();
    assert_eq!(list.1.size_of_set(), Some(100_000));
    assert_eq!(
        first
            .nodes
            .iter()
            .filter_map(|(_, n)| n.position_in_set())
            .collect::<Vec<_>>(),
        (0..7).collect::<Vec<_>>()
    );
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
    scroll(&mut r, &mut ui, 1_999_900.);
    let delta = cache.update(&ui, "Virtual list", 2.).unwrap().unwrap();
    assert!(delta.nodes.len() < 30);
    assert_eq!(
        delta
            .nodes
            .iter()
            .filter_map(|(_, n)| n.position_in_set())
            .collect::<Vec<_>>(),
        (99_993..100_000).collect::<Vec<_>>()
    );
    consumer.update_and_process_changes(delta, &mut Changes);
}

struct Variable {
    rows: VirtualRows,
    handle: ScrollHandle,
    builds: Rc<Cell<usize>>,
    anchoring: bool,
    reveal: usize,
    revealed: Option<Result<(), ScrollError>>,
}
impl View for Variable {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        variable_list(&self.rows, &self.handle, cx, |index| {
            self.builds.set(self.builds.get() + 1);
            let Some(Key::Integer(key)) = self.rows.key(index) else {
                unreachable!()
            };
            button(format!("item{key}"))
                .padding(0.)
                .on_click(cx.listener(|s, _, cx| {
                    s.revealed = Some(s.handle.reveal_list_row(cx, &s.rows, s.reveal));
                }))
        })
        .anchoring(self.anchoring)
        .fill_width()
        .fill_height()
    }
}
/// Heights 10, 20, 30, 40, 10, 20, ... keyed by index: 100 px per four rows.
fn variable_rows(count: usize) -> VirtualRows {
    (0..count).map(|i| (i, 10. * (i % 4 + 1) as f32)).collect()
}
fn setup_variable(rows: VirtualRows) -> (Runtime, Entity<Variable>, Ui<Variable>) {
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| {
        cx.new(|_| Variable {
            rows,
            handle: ScrollHandle::new(),
            builds: Rc::new(Cell::new(0)),
            anchoring: true,
            reveal: 0,
            revealed: None,
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    (runtime, root, ui)
}
fn variable_viewport(ui: &Ui<Variable>) -> ElementId {
    ui.semantics().find(|n| n.scroll_range[1] > 0.).unwrap().id
}
fn scroll_variable(runtime: &mut Runtime, ui: &mut Ui<Variable>, y: f32) {
    let target = variable_viewport(ui);
    ui.semantic_action(
        runtime,
        SemanticAction::Scroll {
            target,
            offset: [0., y],
        },
        &mut Measure,
    )
    .unwrap();
    ui.prepare(runtime, [200., 100.], &mut Measure).unwrap();
}
fn item_id(ui: &Ui<Variable>, key: usize) -> Option<ElementId> {
    let label = format!("item{key}");
    ui.semantics()
        .find(|n| n.role == SemanticRole::Button && n.label == Some(label.as_str()))
        .map(|n| n.id)
}
fn item(ui: &Ui<Variable>, key: usize) -> Option<Bounds> {
    let id = item_id(ui, key)?;
    ui.elements().find(|n| n.id == id).map(|n| n.bounds)
}
fn offset_y(ui: &Ui<Variable>) -> f32 {
    ui.semantic_node(variable_viewport(ui))
        .unwrap()
        .scroll_offset[1]
}
fn mounted(ui: &Ui<Variable>) -> Vec<usize> {
    ui.semantics().filter_map(|n| n.position_in_set).collect()
}
fn edit(runtime: &mut Runtime, root: &Entity<Variable>, f: impl FnOnce(&mut Variable)) {
    runtime.update(|cx| root.update(cx, |s, _| f(s)));
}
/// Clicks the row under the viewport's top edge, running its reveal listener.
fn click_reveal(runtime: &mut Runtime, ui: &mut Ui<Variable>) {
    ui.pointer(runtime, PointerEvent::Pressed([10., 1.]))
        .unwrap();
    ui.pointer(runtime, PointerEvent::Released([10., 1.]))
        .unwrap();
    ui.prepare(runtime, [200., 100.], &mut Measure).unwrap();
}
#[test]
fn variable_rows_mount_only_the_viewport_at_their_offsets() {
    let (mut runtime, root, mut ui) = setup_variable(variable_rows(100_000));
    assert_eq!(
        ui.semantic_node(variable_viewport(&ui))
            .unwrap()
            .scroll_range[1],
        2_500_000. - 100.
    );
    assert_eq!(mounted(&ui), (0..7).collect::<Vec<_>>());
    runtime.update(|cx| assert_eq!(root.read(cx).builds.get(), 7));
    // Row 40_001 spans 1_000_010..1_000_030.
    scroll_variable(&mut runtime, &mut ui, 1_000_015.);
    assert_eq!(item(&ui, 40_001).unwrap().y, -5.);
    assert_eq!(item(&ui, 40_001).unwrap().height, 20.);
    assert_eq!(item(&ui, 40_002).unwrap().y, 15.);
    assert_eq!(mounted(&ui), (39_999..40_008).collect::<Vec<_>>());
    assert!(ui.elements().count() < 40);
    scroll_variable(&mut runtime, &mut ui, 3_000_000.);
    assert_eq!(item(&ui, 99_999).unwrap().y, 60.);
    assert_eq!(*mounted(&ui).last().unwrap(), 99_999);
}
#[test]
fn single_height_changes_move_only_later_rows() {
    let (mut runtime, root, mut ui) = setup_variable(variable_rows(1000));
    edit(&mut runtime, &root, |s| s.rows.set_height(1, 50.));
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    assert_eq!(item(&ui, 0).unwrap().y, 0.);
    assert_eq!(item(&ui, 1).unwrap().height, 50.);
    assert_eq!(item(&ui, 2).unwrap().y, 60.);
    assert_eq!(offset_y(&ui), 0.);
}
#[test]
fn inserting_removing_and_resizing_above_keep_the_first_visible_row_in_place() {
    let (mut runtime, root, mut ui) = setup_variable(variable_rows(1000));
    // Row 40 starts at 1000; show it 7 px above the viewport top.
    scroll_variable(&mut runtime, &mut ui, 1007.);
    let before = item(&ui, 40).unwrap();
    assert_eq!(before.y, -7.);
    let id = item_id(&ui, 40).unwrap();
    edit(&mut runtime, &root, |s| {
        s.rows.splice(0..0, (5000..5003).map(|k| (k, 15.)));
    });
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    assert_eq!(offset_y(&ui), 1052.);
    assert_eq!(item(&ui, 40).unwrap(), before);
    assert_eq!(item_id(&ui, 40), Some(id));
    // Index 4 is now the original row 1, 20 px tall.
    edit(&mut runtime, &root, |s| {
        s.rows.remove(4);
    });
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    assert_eq!(offset_y(&ui), 1032.);
    assert_eq!(item(&ui, 40).unwrap(), before);
    edit(&mut runtime, &root, |s| s.rows.set_height(0, 70.));
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    assert_eq!(offset_y(&ui), 1087.);
    assert_eq!(item(&ui, 40).unwrap(), before);
    assert_eq!(item_id(&ui, 40), Some(id));
    // Changes to the anchor row's height and below it do not move the viewport.
    let index = runtime.update(|cx| root.read(cx).rows.index_of(&40.into()).unwrap());
    edit(&mut runtime, &root, |s| {
        s.rows.set_height(index, 90.);
        s.rows.insert(index + 1, 6000, 25.);
    });
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    assert_eq!(offset_y(&ui), 1087.);
    assert_eq!(item(&ui, 40).unwrap().y, -7.);
    assert_eq!(item(&ui, 6000).unwrap().y, 83.);
    // Removing the anchor row keeps the next recorded row in place.
    edit(&mut runtime, &root, |s| {
        s.rows.remove(index);
    });
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    assert_eq!(offset_y(&ui), 997.);
    assert_eq!(item(&ui, 6000).unwrap().y, 83.);
}
#[test]
fn anchoring_is_skipped_at_the_top_when_disabled_and_for_queued_commands() {
    let (mut runtime, root, mut ui) = setup_variable(variable_rows(1000));
    edit(&mut runtime, &root, |s| s.rows.insert(0, 5000, 10.));
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    assert_eq!(offset_y(&ui), 0.);
    assert_eq!(item(&ui, 5000).unwrap().y, 0.);

    scroll_variable(&mut runtime, &mut ui, 500.);
    edit(&mut runtime, &root, |s| {
        s.anchoring = false;
        s.rows.insert(0, 5001, 10.);
    });
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    assert_eq!(offset_y(&ui), 500.);
    // Re-enabling records a fresh anchor rather than replaying old changes.
    edit(&mut runtime, &root, |s| s.anchoring = true);
    ui.prepare(&mut runtime, [200., 100.], &mut Measure)
        .unwrap();
    assert_eq!(offset_y(&ui), 500.);

    // A reveal queued with an insertion wins over anchoring.
    edit(&mut runtime, &root, |s| {
        s.rows.insert(0, 5002, 10.);
        s.reveal = 0;
    });
    click_reveal(&mut runtime, &mut ui);
    assert_eq!(offset_y(&ui), 0.);
    assert_eq!(item(&ui, 5002).unwrap().y, 0.);
}
#[test]
fn reveal_list_row_moves_the_minimum_distance() {
    let (mut runtime, root, mut ui) = setup_variable(variable_rows(1000));
    // Row 500 spans 12_500..12_510, below the viewport: its bottom aligns.
    edit(&mut runtime, &root, |s| s.reveal = 500);
    click_reveal(&mut runtime, &mut ui);
    assert_eq!(offset_y(&ui), 12_410.);
    assert_eq!(item(&ui, 500).unwrap().y, 90.);
    // Row 499 spans 12_460..12_500 and is already visible.
    edit(&mut runtime, &root, |s| s.reveal = 499);
    click_reveal(&mut runtime, &mut ui);
    assert_eq!(offset_y(&ui), 12_410.);
    // Row 3 spans 60..100, above the viewport: its top aligns.
    edit(&mut runtime, &root, |s| s.reveal = 3);
    click_reveal(&mut runtime, &mut ui);
    assert_eq!(offset_y(&ui), 60.);
    assert_eq!(item(&ui, 3).unwrap().y, 0.);
    edit(&mut runtime, &root, |s| s.reveal = 1000);
    click_reveal(&mut runtime, &mut ui);
    runtime.update(|cx| {
        assert_eq!(
            root.read(cx).revealed,
            Some(Err(ScrollError::InvalidOffset))
        )
    });
}
#[test]
fn invalid_variable_heights_never_invoke_the_row_builder() {
    for rows in [
        [(0, 10.), (1, 0.)].into_iter().collect::<VirtualRows>(),
        [(0, f32::NAN)].into_iter().collect(),
        [(0, -1.)].into_iter().collect(),
        [(0, 16_777_216.), (1, 1.)].into_iter().collect(),
    ] {
        let (mut runtime, root, mut ui) = setup_variable(VirtualRows::new());
        edit(&mut runtime, &root, |s| s.rows = rows);
        assert!(matches!(
            ui.prepare(&mut runtime, [200., 100.], &mut Measure),
            Err(UiError::InvalidStyle)
        ));
        runtime.update(|cx| assert_eq!(root.read(cx).builds.get(), 0));
    }
}

struct NarrowTree {
    scroll: ScrollHandle,
    selected: usize,
}
impl View for NarrowTree {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().padding(7.).fill_width().fill_height().child(
            virtual_list(100, 24., &self.scroll, cx, |index| {
                button(format!("node {index}: a long undo-history description"))
                    .key(index)
                    .height(24.)
                    .padding_y(0.)
                    .padding_left(48.)
                    .padding_right(12.)
                    .text_overflow(TextOverflow::Ellipsis)
                    .on_click(cx.listener(move |s, _, _| s.selected = index))
            })
            .overscan(0)
            .fill_width()
            .fill_height(),
        )
    }
}
#[test]
fn narrow_tree_rows_keep_slots_clips_hit_areas_and_scroll_ends_on_resize() {
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| {
        cx.new(|_| NarrowTree {
            scroll: ScrollHandle::new(),
            selected: usize::MAX,
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    for size in [[180., 115.], [320., 163.], [100., 91.]] {
        ui.prepare(&mut runtime, size, &mut Measure).unwrap();
        let target = ui.semantics().find(|s| s.scroll_range[1] > 0.).unwrap().id;
        for y in [0., 11.5, 100_000., 0.] {
            ui.semantic_action(
                &mut runtime,
                SemanticAction::Scroll {
                    target,
                    offset: [0., y],
                },
                &mut Measure,
            )
            .unwrap();
            ui.prepare(&mut runtime, size, &mut Measure).unwrap();
            let state = ui.semantic_node(target).unwrap();
            let offset = state.scroll_offset[1];
            let range = state.scroll_range[1];
            assert_eq!(range, 2400. - (size[1] - 14.));
            let first = (offset / 24.).floor() as usize;
            let buttons: Vec<_> = ui
                .elements()
                .filter(|e| e.kind == ElementType::Button)
                .collect();
            for e in &buttons {
                let index = ui
                    .semantic_node(e.parent.unwrap())
                    .unwrap()
                    .position_in_set
                    .unwrap();
                assert_eq!(e.bounds.height, 24.);
                assert_eq!(e.bounds.y, 7. + index as f32 * 24. - offset);
                assert_eq!(e.text_style.overflow, TextOverflow::Ellipsis);
                assert_eq!(e.clip_bounds.y, e.bounds.y.max(7.));
                assert_eq!(
                    e.clip_bounds.y + e.clip_bounds.height,
                    (e.bounds.y + 24.).min(size[1] - 7.)
                );
            }
            if offset == 0. {
                assert_eq!(buttons[0].bounds.y, 7.);
            }
            if offset == range {
                let last = buttons.last().unwrap();
                assert_eq!(
                    ui.semantic_node(last.parent.unwrap())
                        .unwrap()
                        .position_in_set,
                    Some(99)
                );
                assert_eq!(last.bounds.y + last.bounds.height, size[1] - 7.);
            }
            drop(buttons);
            // The leading indentation remains part of the full-row hit target.
            for event in [
                PointerEvent::Pressed([8., 8.]),
                PointerEvent::Released([8., 8.]),
            ] {
                ui.pointer(&mut runtime, event).unwrap();
            }
            runtime.update(|cx| assert_eq!(root.read(cx).selected, first));
            runtime.update(|cx| root.update(cx, |s, _| s.selected = usize::MAX));
            for event in [
                PointerEvent::Pressed([8., 6.]),
                PointerEvent::Released([8., 6.]),
            ] {
                ui.pointer(&mut runtime, event).unwrap();
            }
            runtime.update(|cx| assert_eq!(root.read(cx).selected, usize::MAX));
        }
    }
}
