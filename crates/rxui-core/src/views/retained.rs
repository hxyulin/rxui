//! The escape hatch: a specialized retained element behind a view boundary.

use std::{any::Any, ops::ControlFlow};

use astrelis_ui_next::{Element, Invalidation, NodeHandle, UiError};

use crate::{
    Theme, View,
    view::{ActionEmitter, AnyView, Mounted, MountedChildren, MountedState, ViewContext, ViewNode},
};

/// Configuration contract for a specialized retained element.
///
/// This is the escape hatch for charts, render viewports, docking surfaces,
/// and other workloads whose interaction state should remain imperative.
///
/// # Two disjoint phases
///
/// A pass over a spec does two things that never overlap: it writes the element,
/// then it reconciles children. The element write happens *inside* the engine's
/// `update`, which hands out `&mut Element` and nothing else - which is why
/// [`create`](Self::create) and [`update`](Self::update) receive an
/// [`ActionEmitter`] and a [`Theme`] rather than a context. Children are
/// reconciled afterwards, with the whole tree available, from
/// [`children`](Self::children).
pub trait RetainedSpec<Action: 'static>: Clone + 'static {
    /// Concrete retained element owned by the incremental tree.
    type Element: Element;

    /// Creates newly mounted retained state.
    fn create(&self, emitter: &ActionEmitter<Action>, theme: &Theme) -> Self::Element;

    /// Applies changed configuration without discarding interaction state.
    ///
    /// Called on every pass, including one where [`changed`](Self::changed)
    /// reported nothing: the element still has to be pointed at this frame's
    /// callbacks. Only the invalidation is conditional.
    fn update(&self, element: &mut Self::Element, emitter: &ActionEmitter<Action>, theme: &Theme);

    /// Reports which retained passes this configuration change requires.
    ///
    /// An empty [`Invalidation`] means the element is visually and semantically
    /// identical to `previous` and no pass has to run over it. Reporting more
    /// than changed is merely slow; reporting less leaves a stale frame, so
    /// widen when in doubt - [`Invalidation::ALL`] is always correct.
    fn changed(&self, previous: &Self) -> Invalidation;

    /// Produces child views hosted inside this element.
    ///
    /// Defaults to none, which is what a leaf wants. A container element returns
    /// its children here and gets full keyed reconciliation, nested components
    /// included, without implementing [`ViewNode`] itself.
    fn children(&self) -> Vec<AnyView<Action>> {
        Vec::new()
    }
}

/// Mounts a specialized retained element behind the reconciled view boundary.
pub fn retained<Action: 'static, Spec: RetainedSpec<Action>>(spec: Spec) -> View<Action> {
    AnyView {
        key: None,
        inner: Box::new(RetainedView { spec }),
    }
}

struct RetainedView<Spec> {
    spec: Spec,
}

struct RetainedState<Spec: RetainedSpec<Action>, Action: 'static> {
    handle: NodeHandle<Spec::Element>,
    spec: Spec,
    children: MountedChildren<Action>,
}

impl<Spec: RetainedSpec<Action>, Action: 'static> MountedState<Action>
    for RetainedState<Spec, Action>
{
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn visit_children(&mut self, visit: &mut dyn FnMut(&mut Mounted<Action>) -> ControlFlow<()>) {
        self.children.visit(visit);
    }
}

impl<Spec: RetainedSpec<Action>, Action: 'static> ViewNode<Action> for RetainedView<Spec> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let spec = self.spec;
        let emitter = context.emitter();
        let element = spec.create(&emitter, context.theme());
        let handle = context.append(element)?;
        let mut children = MountedChildren::new();
        let child_views = spec.children();
        if !child_views.is_empty() {
            children.build(child_views, &mut context.child(handle.id()))?;
        }
        Ok(Mounted::new(
            handle.id(),
            RetainedState::<Spec, Action> {
                handle,
                spec,
                children,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let spec = self.spec;
        let emitter = context.emitter();
        let theme = context.theme();
        let state = mounted.state_mut::<RetainedState<Spec, Action>>()?;
        let handle = state.handle;
        let invalidation = spec.changed(&state.spec);
        let child_views = spec.children();
        // The element write and the child reconciliation are deliberately
        // sequential: `update` holds `&mut Element`, so nothing that needs the
        // tree can run inside it.
        context.ui().update(handle, invalidation, |element| {
            spec.update(element, &emitter, theme);
        })?;
        state.spec = spec;
        if !(child_views.is_empty() && state.children.is_empty()) {
            let mut child_context = context.child(handle.id());
            state.children.reconcile(child_views, &mut child_context)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use astrelis_core::geometry::LogicalSize;
    use astrelis_ui_next::{Flex, NodeId, SemanticRole, UiRoot};

    use super::*;
    use crate::{DirtyHandle, ViewHost, diagnostics::ViewStats, label};

    /// A specialized element that hosts child views.
    ///
    /// This is the shape a leaf-only `RetainedSpec` could not express, and no
    /// other in-tree spec has children, so without this the retained-container
    /// path would compile and never run.
    #[derive(Clone)]
    struct Panel {
        rows: Vec<&'static str>,
    }

    impl RetainedSpec<()> for Panel {
        type Element = Flex;

        fn create(&self, _emitter: &ActionEmitter<()>, _theme: &Theme) -> Flex {
            Flex::default()
        }

        fn update(&self, _element: &mut Flex, _emitter: &ActionEmitter<()>, _theme: &Theme) {}

        fn changed(&self, _previous: &Self) -> Invalidation {
            Invalidation::empty()
        }

        fn children(&self) -> Vec<AnyView<()>> {
            self.rows
                .iter()
                .map(|row| label(*row).keyed(*row))
                .collect()
        }
    }

    fn panel(rows: &[&'static str]) -> View<()> {
        retained(Panel {
            rows: rows.to_vec(),
        })
    }

    /// Every mounted row, paired with the retained node carrying it.
    fn rows(ui: &UiRoot) -> Vec<(String, NodeId)> {
        ui.semantic_snapshot()
            .into_iter()
            .filter(|node| node.data.role == SemanticRole::Label)
            .map(|node| (node.data.label, node.id))
            .collect()
    }

    fn mount() -> (ViewHost<()>, UiRoot, Theme, DirtyHandle) {
        let theme = Theme::dark();
        let dirty = DirtyHandle::default();
        let mut ui = UiRoot::new(Flex::default(), LogicalSize::new(400.0, 300.0));
        let host = ViewHost::mount(&mut ui, &theme, panel(&["a", "b", "c"]), &dirty)
            .expect("the panel mounts");
        ui.update_passes().expect("first pass");
        (host, ui, theme, dirty)
    }

    #[test]
    fn a_retained_spec_mounts_the_children_it_reports() {
        let (_host, ui, _theme, _dirty) = mount();
        assert_eq!(
            rows(&ui)
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "c"],
        );
    }

    #[test]
    fn a_retained_spec_reconciles_its_children_by_key() {
        let (mut host, mut ui, theme, dirty) = mount();
        let before = rows(&ui);

        let ((), stats) = ViewStats::measure(|| {
            host.rebuild(&mut ui, &theme, panel(&["c", "d", "a"]), &dirty, false)
                .expect("the panel reconciles");
        });
        ui.update_passes().expect("second pass");
        let after = rows(&ui);

        // 1 = the spec's own child list, reconciled exactly once. A leaf spec
        // reports none and this stays 0, which is what keeps the escape hatch
        // free for the four in-tree specs that have no children.
        assert_eq!(stats.containers_reconciled, 1);
        // 1 = the new row. The two survivors are reconciled, not remounted.
        assert_eq!(stats.nodes_built, 1);
        // 1 = the framework published the reordered child list. Visual order
        // does not follow yet, because `UiRoot::set_children` appends and
        // removes rather than permuting in place; a builtin `column` reorders
        // exactly the same way today.
        assert_eq!(stats.set_children_calls, 1);

        for (text, node) in &before {
            match after.iter().find(|(label, _)| label == text) {
                Some((_, moved)) => {
                    assert_eq!(node, moved, "row `{text}` must keep its retained identity");
                }
                // `b` left the list, so its retained node must be gone with it.
                None => assert!(!ui.contains(*node), "row `{text}` must be removed"),
            }
        }
        let mut labels = after
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>();
        labels.sort_unstable();
        assert_eq!(labels, ["a", "c", "d"]);
    }
}
