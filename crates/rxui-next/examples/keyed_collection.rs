//! Dynamic keyed collection whose order changes without losing retained identity.

use astrelis_core::geometry::LogicalSize;
use rxui_next::{
    Component, ComponentContext, ComponentHost, Theme, View, button, column, label, row, views,
};

#[derive(Clone)]
enum Action {
    Reverse,
    Remove(u64),
}

struct RecentDocuments {
    documents: Vec<(u64, String)>,
}

impl Component for RecentDocuments {
    type Action = Action;
    type Effect = ();

    fn update(&mut self, action: Action, _context: &mut ComponentContext<'_, ()>) {
        match action {
            Action::Reverse => self.documents.reverse(),
            Action::Remove(id) => self.documents.retain(|document| document.0 != id),
        }
    }

    fn view(&self, _theme: &Theme) -> View<Action> {
        column((
            label("Recent documents").key("title"),
            button("Reverse", Action::Reverse).key("reverse"),
            column(views(self.documents.iter().map(|(id, name)| {
                row((
                    label(name.clone()).key("name"),
                    button("Remove", Action::Remove(*id)).key("remove"),
                ))
                .key(*id)
            })))
            .key("documents"),
        ))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut host = ComponentHost::new(
        RecentDocuments {
            documents: vec![
                (1, "scene.rx".into()),
                (2, "materials.rx".into()),
                (3, "lighting.rx".into()),
            ],
        },
        LogicalSize::new(480.0, 320.0),
        Theme::dark(),
    )?;

    let before = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .filter(|node| node.data.role == rxui_next::core::SemanticRole::Label)
        .map(|node| (node.data.label, node.id))
        .collect::<std::collections::HashMap<_, _>>();

    host.dispatch(Action::Reverse)?;

    let after = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .filter(|node| node.data.role == rxui_next::core::SemanticRole::Label)
        .map(|node| (node.data.label, node.id))
        .collect::<std::collections::HashMap<_, _>>();

    assert_eq!(before, after);
    Ok(())
}
