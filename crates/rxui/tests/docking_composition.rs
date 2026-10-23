//! Dock resolver timing, snapshot ownership, and lazy mounting contracts.
#![cfg(feature = "layout")]
use rxui::{Element, prelude::*};
use std::{cell::RefCell, rc::Rc};

#[test]
fn resolver_is_eager_once_per_panel_in_tree_order_and_snapshot_is_owned() {
    let mut tree = DockTree::from_panels(["a", "b"]).unwrap();
    let main = tree.root().id();
    tree.split(main, DockSide::Left, "c", SplitPosition::Fraction(0.4))
        .unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let builds = Rc::new(RefCell::new(Vec::new()));
    let description = dock(&tree, |key| {
        calls.borrow_mut().push(key.clone());
        let key = key.clone();
        let builds = builds.clone();
        dock_panel_lazy("Title", move || {
            builds.borrow_mut().push(key);
            label("Content")
        })
        .closable(true)
    });
    assert_eq!(
        *calls.borrow(),
        [Key::from("c"), Key::from("a"), Key::from("b")]
    );
    assert!(builds.borrow().is_empty());
    tree.select(main, &"b".into()).unwrap();
    assert!(tree.remove(&"c".into()));
    drop(tree);
    let _ = description
        .content_policy(TabContentPolicy::MountSelected)
        .into_element();
    assert_eq!(*builds.borrow(), [Key::from("c"), Key::from("a")]);
    assert_eq!(calls.borrow().len(), 3);
}

#[test]
fn final_builder_policy_controls_lazy_content_and_skipped_closures_are_dropped() {
    struct DropCount(Rc<RefCell<usize>>);
    impl Drop for DropCount {
        fn drop(&mut self) {
            *self.0.borrow_mut() += 1;
        }
    }
    for (policy, expected) in [
        (TabContentPolicy::KeepMounted, 3),
        (TabContentPolicy::MountSelected, 1),
    ] {
        let tree = DockTree::from_panels(["a", "b", "c"]).unwrap();
        let calls = Rc::new(RefCell::new(0));
        let drops = Rc::new(RefCell::new(0));
        let description = dock(&tree, |_| {
            let calls = calls.clone();
            let owned = DropCount(drops.clone());
            dock_panel_lazy("Title", move || {
                *calls.borrow_mut() += 1;
                drop(owned);
                column().child(label("Content"))
            })
        })
        .content_policy(TabContentPolicy::MountSelected)
        .content_policy(policy);
        assert_eq!(*calls.borrow(), 0);
        assert_eq!(*drops.borrow(), 0);
        let _ = description.into_element();
        assert_eq!(*calls.borrow(), expected);
        assert_eq!(*drops.borrow(), 3);
    }
}

#[test]
fn eager_panels_still_build_inactive_descriptions() {
    let tree = DockTree::from_panels(["a", "b", "c"]).unwrap();
    let mut calls = 0;
    let description = dock(&tree, |_| {
        calls += 1;
        dock_panel("Title", label("Content"))
    });
    assert_eq!(calls, 3);
    let _ = description
        .content_policy(TabContentPolicy::MountSelected)
        .into_element();
}

#[test]
fn empty_dock_does_not_resolve_or_build_content() {
    let _ = dock(&DockTree::new(), |_| panic!("empty dock resolver"))
        .content_policy(TabContentPolicy::MountSelected)
        .into_element();
}

#[test]
fn abandoned_dock_drops_lazy_content_without_building() {
    let tree = DockTree::from_panels(["a"]).unwrap();
    drop(dock(&tree, |_| {
        dock_panel_lazy("Title", || -> Element { panic!("abandoned content") })
    }));
}

#[test]
fn selected_lazy_content_switches_while_all_headers_remain_available() {
    struct Workspace {
        tree: DockTree,
        builds: Rc<RefCell<Vec<Key>>>,
    }
    impl View for Workspace {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            dock(&self.tree, |key| {
                let key = key.clone();
                let builds = self.builds.clone();
                dock_panel_lazy(format!("Header {key:?}"), move || {
                    builds.borrow_mut().push(key.clone());
                    label(format!("Content {key:?}"))
                })
            })
            .content_policy(TabContentPolicy::MountSelected)
        }
    }
    struct Measure;
    impl rxui::TextMeasure for Measure {
        fn measure(
            &mut self,
            _: rxui::ElementId,
            r: rxui::TextRequest<'_>,
        ) -> Result<[f32; 2], rxui::UiError> {
            Ok([r.text.len() as f32 * 8., 20.])
        }
    }
    let mut runtime = Runtime::new();
    let builds = Rc::new(RefCell::new(Vec::new()));
    let root = runtime.update(|cx| {
        cx.new(|_| Workspace {
            tree: DockTree::from_panels(["a", "b"]).unwrap(),
            builds: builds.clone(),
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(*builds.borrow(), [Key::from("a")]);
    assert_eq!(
        ui.elements()
            .filter_map(|e| e.text)
            .filter(|t| t.starts_with("Header "))
            .count(),
        2
    );
    assert_eq!(
        ui.elements()
            .filter_map(|e| e.text)
            .filter(|t| t.starts_with("Content "))
            .count(),
        1
    );
    runtime.update(|cx| {
        root.update(cx, |s, _| {
            s.tree.select(s.tree.root().id(), &"b".into()).unwrap();
        })
    });
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(*builds.borrow(), [Key::from("a"), Key::from("b")]);
    assert_eq!(
        ui.elements()
            .filter_map(|e| e.text)
            .filter(|t| t.starts_with("Header "))
            .count(),
        2
    );
    assert_eq!(
        ui.elements()
            .filter_map(|e| e.text)
            .filter(|t| t.starts_with("Content "))
            .count(),
        1
    );
}
