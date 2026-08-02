//! Contract tests for the erased custom-element escape hatch.

use rxui_core::{Context, CustomElementSpec, Element, EntityHarness, Render, Theme, custom, label};
use rxui_tree::{Flex, Frame, Invalidation, PassStats};

#[derive(Clone)]
struct FlexSpec {
    revision: u32,
    invalidation: Invalidation,
    children: Vec<(u64, String)>,
}

impl CustomElementSpec for FlexSpec {
    type Element = Flex;

    fn create(&self, _theme: &Theme) -> Self::Element {
        Flex::default()
    }

    fn update(&self, _element: &mut Self::Element, _theme: &Theme) {}

    fn changed(&self, previous: &Self) -> Invalidation {
        if self.revision == previous.revision {
            Invalidation::empty()
        } else {
            self.invalidation
        }
    }

    fn children(&self) -> Vec<Element> {
        self.children
            .iter()
            .map(|(key, text)| label(text).key(*key))
            .collect()
    }
}

#[derive(Clone)]
struct FrameSpec;

impl CustomElementSpec for FrameSpec {
    type Element = Frame;

    fn create(&self, _theme: &Theme) -> Self::Element {
        Frame::default()
    }
    fn update(&self, _element: &mut Self::Element, _theme: &Theme) {}
    fn changed(&self, _previous: &Self) -> Invalidation {
        Invalidation::empty()
    }
}

#[derive(Clone)]
struct Scene {
    revision: u32,
    invalidation: Invalidation,
    alternate: bool,
    children: Vec<(u64, String)>,
}

impl Render for Scene {
    fn render(&mut self, _cx: &mut Context<'_, Self>) -> Element {
        if self.alternate {
            custom(FrameSpec)
        } else {
            custom(FlexSpec {
                revision: self.revision,
                invalidation: self.invalidation,
                children: self.children.clone(),
            })
        }
    }
}

fn scene() -> Scene {
    Scene {
        revision: 0,
        invalidation: Invalidation::empty(),
        alternate: false,
        children: Vec::new(),
    }
}

#[test]
fn identical_custom_spec_performs_zero_retained_work() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| scene()));
    harness.refresh();
    assert_eq!(
        harness.stats().passes,
        PassStats {
            reused_fragments: 3,
            ..PassStats::default()
        }
    );
}

#[test]
fn changed_routes_exactly_the_reported_invalidation() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| scene()));
    harness.mutate(|scene| {
        scene.revision += 1;
        scene.invalidation = Invalidation::PAINT;
    });
    assert_eq!(
        harness.stats().passes,
        PassStats {
            rebuilt_fragments: 1,
            reused_fragments: 2,
            invalidate_steps: 2,
            ..PassStats::default()
        }
    );
}

#[test]
fn a_custom_spec_type_change_remounts_the_retained_element() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| scene()));
    let before = harness
        .app()
        .tree()
        .children(harness.app().tree().children(harness.app().tree().root())[0])[0];
    harness.mutate(|scene| scene.alternate = true);
    let after = harness
        .app()
        .tree()
        .children(harness.app().tree().children(harness.app().tree().root())[0])[0];
    assert_ne!(after, before);
    assert_eq!(
        harness.stats().passes,
        PassStats {
            layout_elements: 4,
            composed_nodes: 1,
            rebuilt_fragments: 2,
            reused_fragments: 1,
            visited_compose_nodes: 3,
            visited_accessibility_nodes: 3,
            invalidate_steps: 1,
            ..PassStats::default()
        }
    );
}

#[test]
fn custom_children_reconcile_through_the_keyed_path() {
    let mut initial = scene();
    initial.children = vec![(1, "One".into()), (2, "Two".into())];
    let mut harness = EntityHarness::new(|cx| cx.new(|_| initial));
    let one = harness.node_id("One");
    let two = harness.node_id("Two");
    harness.mutate(|scene| scene.children.swap(0, 1));
    assert_eq!(harness.node_id("One"), one);
    assert_eq!(harness.node_id("Two"), two);
    assert_eq!(
        harness.stats().passes,
        PassStats {
            layout_elements: 8,
            rebuilt_fragments: 1,
            reused_fragments: 4,
            shaped_text: 4,
            visited_compose_nodes: 3,
            compose_skipped_subtrees: 2,
            visited_accessibility_nodes: 3,
            accessibility_skipped_subtrees: 2,
            invalidate_steps: 2,
            ..PassStats::default()
        }
    );
}
