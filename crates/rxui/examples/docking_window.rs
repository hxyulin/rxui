//! Standalone controlled docking tree. Panel models outlive layout placements.
use rxui::prelude::*;
use std::collections::HashMap;

struct Document {
    title: String,
    note: String,
    scroll: ScrollHandle,
}
impl View for Document {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let rows = column()
            .gap(4.)
            .children((0..60).map(|i| label(format!("{} — row {i:02}", self.title)).key(i)));
        let note = text_input(self.note.clone())
            .fill_width()
            .accessibility_label(format!("{} note", self.title))
            .on_change(cx.listener(|s, e: &TextChangeEvent, _| s.note = e.value.clone()));
        column()
            .fill_width()
            .fill_height()
            .min_width(0.)
            .min_height(0.)
            .padding(12.)
            .gap(10.)
            .child(label(self.title.clone()).font_size(22.))
            .child(note)
            .child(scroll_area(rows).handle(self.scroll.clone()))
    }
}
struct Workspace {
    layout: DockTree,
    documents: HashMap<Key, Entity<Document>>,
    order: Vec<Key>,
    next: u32,
    policy: TabContentPolicy,
    status: String,
}
fn first_group(node: &DockNode) -> DockNodeId {
    match node {
        DockNode::Tabs(n) => n.id(),
        DockNode::Split(n) => first_group(n.first()),
    }
}
impl Workspace {
    fn reset(&mut self) {
        self.layout = DockTree::from_panels(self.order.clone()).expect("unique document keys");
        let main = self.layout.root().id();
        self.layout
            .dock_panel(
                &Key::from("output"),
                main,
                DockSide::Bottom,
                SplitPosition::Fraction(0.7),
            )
            .unwrap();
        self.layout
            .dock_panel(
                &Key::from("files"),
                main,
                DockSide::Left,
                SplitPosition::Pixels(240.),
            )
            .unwrap();
        self.status = "Layout restored; document data retained.".into();
    }
    fn add(&mut self, cx: &mut Context<'_, Self>) {
        let id = self.next;
        self.next += 1;
        let key = Key::from(id);
        self.documents.insert(
            key.clone(),
            cx.new(|_| Document {
                title: format!("Document {id}"),
                note: "New document".into(),
                scroll: ScrollHandle::new(),
            }),
        );
        self.order.push(key.clone());
        let target = self
            .layout
            .group_for(&Key::from("editor"))
            .unwrap_or_else(|| first_group(self.layout.root()));
        let index = self
            .layout
            .node(target)
            .unwrap()
            .tabs()
            .unwrap()
            .panels()
            .len();
        self.layout.insert(target, index, key.clone()).unwrap();
        self.layout.select(target, &key).unwrap();
        self.status = "Added a document tab.".into();
    }
}
impl View for Workspace {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let content = dock(&self.layout, |key| {
            let document = self.documents[key].clone();
            let title = document.read(cx).title.clone();
            dock_panel(title, document).closable(true)
        })
        .key("workspace")
        .min_pane_size(150., 140.)
        .content_policy(self.policy)
        .on_event(cx.listener(|s, e: &DockEvent, _| match s.layout.apply(e) {
            Ok(_) => {
                if matches!(e, DockEvent::Close { .. }) {
                    s.status =
                        "Panel closed. Restore layout reopens it; its document model is retained."
                            .into();
                }
            }
            Err(error) => s.status = error.to_string(),
        }));
        let tools=row().gap(8.).padding(10.).min_width(0.).scroll_x()
            .child(button("New tab").on_click(cx.listener(|s,_,cx|s.add(cx))))
            .child(button("Reorder active").on_click(cx.listener(|s,_,_| {
                let group=s.layout.group_for(&Key::from("editor")).unwrap_or_else(||first_group(s.layout.root()));
                let tabs=s.layout.node(group).unwrap().tabs().unwrap();
                if let Some(selected)=tabs.selected().cloned() {
                    let index=if tabs.panels().first()==Some(&selected) {tabs.panels().len()-1} else {0};
                    s.layout.move_panel(&selected,group,index).unwrap();
                    s.status="Reordered within the group; retained widget placement survives.".into();
                }
            })))
            .child(button("Move preview").on_click(cx.listener(|s,_,_| {
                let preview=Key::from("preview");
                if let (Some(source),Some(editor),Some(output))=(s.layout.group_for(&preview),s.layout.group_for(&Key::from("editor")),s.layout.group_for(&Key::from("output"))) {
                    let target=if source==editor {output} else {editor};
                    s.layout.move_panel(&preview,target,0).unwrap();
                    s.status="Moved preview between groups. Document data survives; widget placement remounts.".into();
                } else {s.status="Restore the layout to reopen editor, preview and output.".into();}
            })))
            .child(button("Split preview right").on_click(cx.listener(|s,_,_| {
                if let Some(target)=s.layout.group_for(&Key::from("editor")) {
                    match s.layout.dock_panel(&Key::from("preview"),target,DockSide::Right,SplitPosition::Fraction(0.5)) {
                        Ok(_)=>s.status="Created a new group to the right of editor.".into(),
                        Err(error)=>s.status=error.to_string(),
                    }
                }
            })))
            .child(button("Restore layout").on_click(cx.listener(|s,_,_|s.reset())))
            .child(button(if self.policy==TabContentPolicy::KeepMounted {"Retain panels"} else {"Mount selected"})
                .on_click(cx.listener(|s,_,_|s.policy=if s.policy==TabContentPolicy::KeepMounted {TabContentPolicy::MountSelected} else {TabContentPolicy::KeepMounted})))
            .child(button("Shared window").on_click(cx.listener(|_,_,cx| {
                let workspace=cx.entity().upgrade().expect("live workspace");
                cx.open_window(WindowOptions::new().title("RXUI — shared dock").size(1000.,740.),workspace).unwrap();
            })));
        column()
            .fill_width()
            .fill_height()
            .min_width(0.)
            .min_height(0.)
            .child(tools)
            .child(content)
            .child(
                label(self.status.clone())
                    .padding(10.)
                    .color(ThemeColor::TextMuted),
            )
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let mut documents = HashMap::new();
        let mut order = Vec::new();
        for (key, title) in [
            ("editor", "Editor"),
            ("preview", "Preview"),
            ("output", "Output"),
            ("files", "Files"),
        ] {
            let key = Key::from(key);
            order.push(key.clone());
            documents.insert(
                key,
                cx.new(|_| Document {
                    title: title.into(),
                    note: format!("{title} data survives layout changes"),
                    scroll: ScrollHandle::new(),
                }),
            );
        }
        let mut state = Workspace {
            layout: DockTree::new(),
            documents,
            order,
            next: 0,
            policy: TabContentPolicy::KeepMounted,
            status: String::new(),
        };
        state.reset();
        let workspace = cx.new(|_| state);
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — docking tree")
                .size(1200., 800.),
            workspace,
        )?;
        Ok(())
    })
}
