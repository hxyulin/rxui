//! Entity update-isolation ratchet with deterministic, exact work counters.
//!
//! Notifications name an entity; they do not imply a root rebuild. These tests
//! translate the v1 gate's invariants to that identity boundary: unchanged
//! output mutates no retained node, a child notification never renders its
//! parent, and view-layer cost is independent of untouched sibling count.

use std::{cell::Cell, rc::Rc};

use astrelis_core::geometry::LogicalSize;
use astrelis_text::FontDatabase;
use rxui_core::{
    App, Context, Element, Entity, EntityHarness, Render, Theme, ViewStats, button, checkbox,
    column, label, slider, text_field,
};
use rxui_tree::PassStats;

struct Static;

impl Render for Static {
    fn render(&mut self, _: &mut Context<Self>) -> Element {
        column().child(label("one")).child(label("two"))
    }
}

#[test]
fn unchanged_rerender_has_zero_retained_mutations() {
    let mut app = App::new(LogicalSize::new(320.0, 240.0), FontDatabase::empty());
    let root = app.new_entity(|_| Static);
    app.mount(&root);
    root.update(&mut app, |_, cx| cx.notify());
    let stats = app.flush();
    assert_eq!(
        stats.passes,
        PassStats {
            reused_fragments: 5,
            ..PassStats::default()
        }
    );
    assert_eq!(
        stats.views,
        ViewStats {
            component_views: 1,
            nodes_built: 0,
            nodes_rebuilt: 3,
            containers_reconciled: 1,
            set_children_calls: 0,
            memo_hits: 0,
            memo_misses: 0,
            rows_realized: 0,
            rows_recycled: 0,
        }
    );
}

struct StaticForm;

impl Render for StaticForm {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        column()
            .child(text_field("Name", "Ada").on_input(cx.listener_value(|_, _: String, _| {})))
            .child(checkbox("Enabled", true).on_toggle(cx.listener_value(|_, _, _| {})))
            .child(slider("Gain", 5.0, 0.0..=10.0).on_change(cx.listener_value(|_, _, _| {})))
            .child(button("Save").on_click(cx.listener(|_, _, _| {})))
    }
}

#[test]
fn unchanged_form_rerender_has_zero_retained_mutations() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| StaticForm));
    harness.refresh();
    assert_eq!(harness.stats().passes.layout_elements, 0);
    assert_eq!(harness.stats().passes.rebuilt_fragments, 0);
    assert_eq!(harness.stats().passes.accessibility_nodes, 0);
    assert_eq!(harness.stats().views.nodes_built, 0);
    assert_eq!(harness.stats().views.set_children_calls, 0);
}

struct Child {
    value: usize,
    renders: Rc<Cell<usize>>,
}

impl Render for Child {
    fn render(&mut self, _: &mut Context<Self>) -> Element {
        self.renders.set(self.renders.get() + 1);
        label(format!("child {}", self.value))
    }
}

struct Parent {
    children: Vec<Entity<Child>>,
    renders: Rc<Cell<usize>>,
}

impl Render for Parent {
    fn render(&mut self, _: &mut Context<Self>) -> Element {
        self.renders.set(self.renders.get() + 1);
        column().children(self.children.iter().cloned().map(Element::from))
    }
}

fn isolated_child_update(siblings: usize) -> (rxui_core::FlushStats, usize, usize) {
    let mut app = App::new(LogicalSize::new(800.0, 600.0), FontDatabase::empty());
    let parent_renders = Rc::new(Cell::new(0));
    let child_renders = Rc::new(Cell::new(0));
    let root = app.new_entity(|cx| Parent {
        children: (0..siblings)
            .map(|value| {
                let renders = child_renders.clone();
                cx.new(move |_| Child { value, renders })
            })
            .collect(),
        renders: parent_renders.clone(),
    });
    app.mount(&root);
    parent_renders.set(0);
    child_renders.set(0);
    let child = root.read(&app).children[0].clone();
    child.update(&mut app, |child, cx| {
        child.value += 1;
        cx.notify();
    });
    let stats = app.flush();
    (stats, parent_renders.get(), child_renders.get())
}

#[test]
fn notifying_child_does_not_render_parent_and_cost_is_sibling_independent() {
    let (four, four_parent, four_children) = isolated_child_update(4);
    let (sixty_four, many_parent, many_children) = isolated_child_update(64);
    assert_eq!((four_parent, four_children), (0, 1));
    assert_eq!((many_parent, many_children), (0, 1));
    assert_eq!(four.views, sixty_four.views);
    assert_eq!(
        four.views,
        ViewStats {
            component_views: 1,
            nodes_built: 0,
            nodes_rebuilt: 1,
            containers_reconciled: 0,
            set_children_calls: 0,
            memo_hits: 0,
            memo_misses: 0,
            rows_realized: 0,
            rows_recycled: 0,
        }
    );
    assert_eq!(
        four.passes,
        PassStats {
            layout_elements: 20,
            rebuilt_fragments: 1,
            reused_fragments: 10,
            accessibility_nodes: 1,
            shaped_text: 8,
            visited_compose_nodes: 5,
            compose_skipped_subtrees: 3,
            visited_accessibility_nodes: 5,
            accessibility_skipped_subtrees: 3,
            invalidate_steps: 4,
            ..PassStats::default()
        }
    );
    assert_eq!(
        sixty_four.passes,
        PassStats {
            layout_elements: 260,
            rebuilt_fragments: 1,
            reused_fragments: 130,
            accessibility_nodes: 1,
            shaped_text: 128,
            visited_compose_nodes: 5,
            compose_skipped_subtrees: 63,
            visited_accessibility_nodes: 5,
            accessibility_skipped_subtrees: 63,
            invalidate_steps: 4,
            ..PassStats::default()
        }
    );
}

#[test]
fn theme_revision_invalidates_every_mounted_entity() {
    let mut app = App::new(LogicalSize::new(320.0, 240.0), FontDatabase::empty());
    let parent_renders = Rc::new(Cell::new(0));
    let child_renders = Rc::new(Cell::new(0));
    let root = app.new_entity(|cx| {
        let renders = child_renders.clone();
        Parent {
            children: vec![cx.new(move |_| Child { value: 0, renders })],
            renders: parent_renders.clone(),
        }
    });
    app.mount(&root);
    parent_renders.set(0);
    child_renders.set(0);
    let mut theme = Theme::dark();
    theme.revision += 1;
    app.set_theme(theme);
    let stats = app.flush();
    assert_eq!((parent_renders.get(), child_renders.get()), (1, 1));
    assert_eq!(
        stats,
        rxui_core::FlushStats {
            passes: PassStats {
                reused_fragments: 5,
                ..PassStats::default()
            },
            views: ViewStats {
                component_views: 2,
                nodes_built: 0,
                nodes_rebuilt: 2,
                containers_reconciled: 1,
                set_children_calls: 0,
                memo_hits: 0,
                memo_misses: 0,
                rows_realized: 0,
                rows_recycled: 0,
            },
        }
    );
}
