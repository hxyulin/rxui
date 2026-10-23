use crate::*;
use std::collections::HashSet;

fn key(s: &str) -> Key {
    s.into()
}
fn add(workspace: &mut DockWorkspace, panels: &[&str]) -> (DockSurfaceId, DockNodeId) {
    let tree = DockTree::from_panels(panels.iter().copied()).unwrap();
    let group = tree.root().id();
    (workspace.create_surface(tree).unwrap(), group)
}
fn tab(group: DockNodeId, index: usize) -> DockDropTarget {
    DockDropTarget::Tab { group, index }
}
fn split(group: DockNodeId, side: DockSide) -> DockDropTarget {
    DockDropTarget::Split {
        group,
        side,
        position: SplitPosition::Fraction(0.4),
    }
}
fn panels(workspace: &DockWorkspace, surface: DockSurfaceId, group: DockNodeId) -> &[Key] {
    workspace
        .surface(surface)
        .unwrap()
        .node(group)
        .unwrap()
        .tabs()
        .unwrap()
        .panels()
}
fn invariant(workspace: &DockWorkspace) {
    fn walk(node: &DockNode, root: bool, nodes: &mut HashSet<DockNodeId>, keys: &mut HashSet<Key>) {
        assert!(nodes.insert(node.id()));
        match node {
            DockNode::Tabs(tabs) => {
                assert!(root || !tabs.panels().is_empty());
                assert_eq!(tabs.selected().is_none(), tabs.panels().is_empty());
                assert!(
                    tabs.selected()
                        .is_none_or(|key| tabs.panels().contains(key))
                );
                for key in tabs.panels() {
                    assert!(keys.insert(key.clone()));
                }
            }
            DockNode::Split(split) => {
                assert!(split.position().valid());
                walk(split.first(), false, nodes, keys);
                walk(split.second(), false, nodes, keys);
            }
        }
    }
    let mut keys = HashSet::new();
    let mut surfaces = HashSet::new();
    for (id, tree) in workspace.surfaces() {
        assert!(surfaces.insert(id));
        walk(tree.root(), true, &mut HashSet::new(), &mut keys);
    }
}
#[test]
fn dock_workspace_creation_uniqueness_and_owned_removal() {
    let mut w = DockWorkspace::new();
    assert!(w.is_empty());
    let (a, group) = add(&mut w, &["a", "b"]);
    let (b, other) = add(&mut w, &["c"]);
    let nested = w
        .split(b, other, DockSide::Right, "d", SplitPosition::default())
        .unwrap();
    let before = w.clone();
    let duplicate = w.surface(b).unwrap().clone();
    assert_eq!(
        w.create_surface(duplicate),
        Err(DockWorkspaceError::DuplicatePanel)
    );
    assert_eq!(
        w.insert(a, group, 0, "d"),
        Err(DockWorkspaceError::DuplicatePanel)
    );
    assert_eq!(
        w.split(a, group, DockSide::Left, "c", SplitPosition::default()),
        Err(DockWorkspaceError::DuplicatePanel)
    );
    assert_eq!(w, before);
    assert_eq!(w.surface_for(&key("d")), Some(b));
    assert_eq!(w.surfaces().map(|(id, _)| id).collect::<Vec<_>>(), [a, b]);
    let removed = w.remove_surface(b).unwrap();
    assert_eq!(removed.group_for(&key("d")), Some(nested));
    assert_eq!(w.surface_for(&key("d")), None);
    assert_eq!(
        w.remove_surface(b),
        Err(DockWorkspaceError::SurfaceNotFound)
    );
    let replacement = w.create_surface(removed).unwrap();
    assert_ne!(replacement, b);
    assert_eq!(w.len(), 2);
    invariant(&w);
}
#[test]
fn dock_workspace_transfer_tab_selection_and_nested_source_collapse() {
    let mut w = DockWorkspace::new();
    let (a, root) = add(&mut w, &["keep", "next"]);
    let output = w
        .split(a, root, DockSide::Right, "output", SplitPosition::default())
        .unwrap();
    let files = w
        .split(
            a,
            output,
            DockSide::Bottom,
            "files",
            SplitPosition::default(),
        )
        .unwrap();
    let (b, dest) = add(&mut w, &["x", "y"]);
    let change = w
        .transfer(a, output, &key("output"), b, tab(dest, 1))
        .unwrap();
    assert_eq!(
        change,
        DockTransfer {
            source_surface: a,
            source_group: output,
            destination_surface: b,
            destination_group: dest,
            changed: true,
            source_empty: false
        }
    );
    assert_eq!(panels(&w, b, dest), [key("x"), key("output"), key("y")]);
    assert_eq!(
        w.surface(b)
            .unwrap()
            .node(dest)
            .unwrap()
            .tabs()
            .unwrap()
            .selected(),
        Some(&key("output"))
    );
    assert!(w.surface(a).unwrap().node(output).is_none());
    assert_eq!(w.surface(a).unwrap().group_for(&key("files")), Some(files));
    assert_eq!(panels(&w, a, root), [key("keep"), key("next")]);
    invariant(&w);
}
#[test]
fn dock_workspace_transfer_split_sides_and_empty_destination() {
    for side in [
        DockSide::Left,
        DockSide::Right,
        DockSide::Top,
        DockSide::Bottom,
    ] {
        let mut w = DockWorkspace::new();
        let (a, source) = add(&mut w, &["a"]);
        let (b, dest) = add(&mut w, &["b"]);
        let moved = w
            .transfer(a, source, &key("a"), b, split(dest, side))
            .unwrap();
        assert!(moved.source_empty);
        assert_eq!(w.len(), 2);
        assert_eq!(w.surface(a).unwrap().root().id(), source);
        let tree = w.surface(b).unwrap();
        let branch = tree.root().split().unwrap();
        assert_eq!(
            branch.axis(),
            if matches!(side, DockSide::Left | DockSide::Right) {
                Axis::Horizontal
            } else {
                Axis::Vertical
            }
        );
        assert_eq!(branch.position(), SplitPosition::Fraction(0.4));
        assert_eq!(
            branch.first().id(),
            if matches!(side, DockSide::Left | DockSide::Top) {
                moved.destination_group
            } else {
                dest
            }
        );
        assert_eq!(tree.group_for(&key("a")), Some(moved.destination_group));
        let back = w
            .transfer(
                b,
                moved.destination_group,
                &key("a"),
                a,
                split(source, side),
            )
            .unwrap();
        assert_eq!(back.destination_group, source);
        assert!(w.surface(a).unwrap().root().tabs().is_some());
        assert_eq!(w.surface(b).unwrap().root().id(), dest);
        invariant(&w);
    }
}
#[test]
fn dock_workspace_transfer_failures_are_atomic() {
    let mut w = DockWorkspace::new();
    let (a, source) = add(&mut w, &["a"]);
    let (b, dest) = add(&mut w, &["b"]);
    let foreign = DockTree::new().root().id();
    let dead_surface = w.create_surface(DockTree::new()).unwrap();
    w.remove_surface(dead_surface).unwrap();
    let branch_group = w
        .split(b, dest, DockSide::Right, "c", SplitPosition::default())
        .unwrap();
    let branch = w.surface(b).unwrap().root().id();
    let before = w.clone();
    for (src_surface, src, panel, dst_surface, target, expected) in [
        (
            a,
            source,
            key("a"),
            b,
            tab(dest, 9),
            DockWorkspaceError::Tree(DockError::InvalidIndex),
        ),
        (
            a,
            source,
            key("a"),
            b,
            tab(foreign, 0),
            DockWorkspaceError::Tree(DockError::NodeNotFound),
        ),
        (
            a,
            source,
            key("a"),
            b,
            tab(branch, 0),
            DockWorkspaceError::Tree(DockError::NotTabGroup),
        ),
        (
            a,
            source,
            key("a"),
            b,
            DockDropTarget::Split {
                group: dest,
                side: DockSide::Top,
                position: SplitPosition::Fraction(f32::NAN),
            },
            DockWorkspaceError::Tree(DockError::InvalidPosition),
        ),
        (
            a,
            source,
            key("missing"),
            b,
            tab(dest, 0),
            DockWorkspaceError::Tree(DockError::PanelNotFound),
        ),
        (
            b,
            branch_group,
            key("b"),
            a,
            tab(source, 0),
            DockWorkspaceError::Tree(DockError::PanelNotFound),
        ),
        (
            a,
            foreign,
            key("a"),
            b,
            tab(dest, 0),
            DockWorkspaceError::Tree(DockError::NodeNotFound),
        ),
        (
            dead_surface,
            source,
            key("a"),
            b,
            tab(dest, 0),
            DockWorkspaceError::SurfaceNotFound,
        ),
        (
            a,
            source,
            key("a"),
            dead_surface,
            tab(dest, 0),
            DockWorkspaceError::SurfaceNotFound,
        ),
    ] {
        assert_eq!(
            w.transfer(src_surface, src, &panel, dst_surface, target),
            Err(expected)
        );
        assert_eq!(w, before);
        invariant(&w);
    }
}
#[test]
fn dock_workspace_same_surface_reorder_split_and_collapse() {
    let mut w = DockWorkspace::new();
    let (a, group) = add(&mut w, &["a", "b", "c"]);
    assert!(
        !w.transfer(a, group, &key("a"), a, tab(group, 0))
            .unwrap()
            .changed
    );
    assert!(
        w.transfer(a, group, &key("c"), a, tab(group, 0))
            .unwrap()
            .changed
    );
    assert_eq!(panels(&w, a, group), [key("c"), key("a"), key("b")]);
    let before = w.clone();
    assert_eq!(
        w.transfer(a, group, &key("a"), a, tab(group, 3)),
        Err(DockWorkspaceError::Tree(DockError::InvalidIndex))
    );
    assert_eq!(w, before);
    let detached = w
        .transfer(a, group, &key("b"), a, split(group, DockSide::Bottom))
        .unwrap();
    let before = w.clone();
    assert_eq!(
        w.transfer(
            a,
            detached.destination_group,
            &key("b"),
            a,
            split(detached.destination_group, DockSide::Left)
        ),
        Err(DockWorkspaceError::Tree(DockError::CannotSplitSolePanel))
    );
    assert_eq!(w, before);
    w.transfer(a, group, &key("a"), a, tab(detached.destination_group, 0))
        .unwrap();
    w.transfer(
        a,
        group,
        &key("c"),
        a,
        split(detached.destination_group, DockSide::Right),
    )
    .unwrap();
    assert!(w.surface(a).unwrap().node(group).is_none());
    invariant(&w);
}
#[test]
fn dock_workspace_detach_reattach_and_stale_source_events() {
    let mut w = DockWorkspace::new();
    let (a, source) = add(&mut w, &["a", "b"]);
    let detached = w.detach(a, source, &key("a")).unwrap();
    assert!(!detached.source_empty);
    assert_eq!(w.len(), 2);
    assert_eq!(
        panels(&w, detached.destination_surface, detached.destination_group),
        [key("a")]
    );
    let before = w.clone();
    assert_eq!(
        w.detach(a, source, &key("a")),
        Err(DockWorkspaceError::Tree(DockError::PanelNotFound))
    );
    for event in [
        DockEvent::Close {
            group: source,
            panel: key("a"),
        },
        DockEvent::Drop {
            source,
            panel: key("a"),
            target: tab(source, 0),
        },
        DockEvent::Select {
            group: source,
            panel: key("a"),
        },
    ] {
        assert_eq!(
            w.apply(a, &event),
            Err(DockWorkspaceError::Tree(DockError::PanelNotFound))
        );
        assert_eq!(w, before);
    }
    w.transfer(
        detached.destination_surface,
        detached.destination_group,
        &key("a"),
        a,
        tab(source, 1),
    )
    .unwrap();
    let empty = w.remove_surface(detached.destination_surface).unwrap();
    assert!(empty.root().tabs().unwrap().panels().is_empty());
    assert_eq!(panels(&w, a, source), [key("b"), key("a")]);
    let only = w.detach(a, source, &key("b")).unwrap();
    let sole = w.detach(a, source, &key("a")).unwrap();
    assert!(sole.source_empty);
    assert_ne!(sole.destination_surface, only.destination_surface);
    invariant(&w);
}
#[test]
fn dock_workspace_checked_events_selection_resize_and_close() {
    let mut w = DockWorkspace::new();
    let (a, group) = add(&mut w, &["a", "b"]);
    assert!(w.select(a, group, &key("b")).unwrap());
    assert!(!w.select(a, group, &key("b")).unwrap());
    w.apply(
        a,
        &DockEvent::Drop {
            source: group,
            panel: key("b"),
            target: split(group, DockSide::Right),
        },
    )
    .unwrap();
    let branch = w.surface(a).unwrap().root().id();
    // Reuse the tree's ordinary event shape, including its gesture phase.
    let resize = ResizeEvent {
        position: SplitPosition::Pixels(180.),
        first_size: 180.,
        available: 500.,
        phase: ResizePhase::End,
    };
    assert!(
        w.apply(
            a,
            &DockEvent::Resize {
                split: branch,
                event: resize
            }
        )
        .unwrap()
    );
    w.apply(
        a,
        &DockEvent::Close {
            group,
            panel: key("a"),
        },
    )
    .unwrap();
    assert!(w.surface(a).unwrap().node(branch).is_none());
    invariant(&w);
}
#[test]
fn dock_workspace_repeated_transfers_preserve_every_panel_and_global_invariants() {
    let mut w = DockWorkspace::new();
    let (a, _) = add(&mut w, &["a", "b", "c"]);
    let (b, _) = add(&mut w, &["d", "e"]);
    let (c, _) = add(&mut w, &[]);
    let keys = [key("a"), key("b"), key("c"), key("d"), key("e")];
    let surfaces = [a, b, c];
    for step in 0..120 {
        let panel = &keys[step % keys.len()];
        let source = w.surface_for(panel).unwrap();
        let group = w.surface(source).unwrap().group_for(panel).unwrap();
        let dest = surfaces[(step / 2) % surfaces.len()];
        fn first_group(node: &DockNode) -> DockNodeId {
            match node {
                DockNode::Tabs(tabs) => tabs.id(),
                DockNode::Split(split) => first_group(split.first()),
            }
        }
        let dest_group = first_group(w.surface(dest).unwrap().root());
        let target = if step % 3 == 0
            && !(source == dest && group == dest_group && panels(&w, source, group).len() == 1)
        {
            split(dest_group, DockSide::Right)
        } else {
            tab(dest_group, 0)
        };
        w.transfer(source, group, panel, dest, target).unwrap();
        invariant(&w);
        for panel in &keys {
            assert!(w.surface_for(panel).is_some());
        }
    }
}

#[test]
fn dock_workspace_cloned_node_ids_remain_scoped_by_surface_and_membership() {
    let mut w = DockWorkspace::new();
    let snapshot = DockTree::new();
    let group = snapshot.root().id();
    let a = w.create_surface(snapshot.clone()).unwrap();
    let b = w.create_surface(snapshot).unwrap();
    assert_ne!(a, b);
    w.insert(a, group, 0, "a").unwrap();
    w.insert(b, group, 0, "b").unwrap();
    let before = w.clone();
    assert_eq!(
        w.transfer(b, group, &key("a"), a, tab(group, 0)),
        Err(DockWorkspaceError::Tree(DockError::PanelNotFound))
    );
    assert_eq!(w, before);
    let moved = w.transfer(a, group, &key("a"), b, tab(group, 1)).unwrap();
    assert_eq!(moved.source_group, moved.destination_group);
    assert!(moved.source_empty);
    assert_eq!(panels(&w, b, group), [key("b"), key("a")]);
    let before = w.clone();
    assert_eq!(
        w.apply(
            a,
            &DockEvent::Close {
                group,
                panel: key("a")
            }
        ),
        Err(DockWorkspaceError::Tree(DockError::PanelNotFound))
    );
    assert_eq!(w, before);
    let back = w
        .transfer(b, group, &key("a"), a, split(group, DockSide::Left))
        .unwrap();
    assert_eq!(back.destination_group, group);
    assert_eq!(panels(&w, a, group), [key("a")]);
    invariant(&w);
}
