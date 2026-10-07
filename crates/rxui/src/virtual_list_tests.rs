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
