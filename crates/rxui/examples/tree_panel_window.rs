//! A real controlled hierarchy: 100 folders, each containing 1,000 leaf records.
//! Arrow keys navigate; Right/Left expand/collapse; Shift extends selection;
//! Command/Control-click toggles selection. All rows keep focus on the tree root.
use rxui::prelude::*;
const HEIGHT: f32 = 24.;
struct Tree {
    scroll: ScrollHandle,
    state: TreeState,
}
impl View for Tree {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let change = cx.listener(|s, proposal: &TreeProposal, cx| {
            // Applications may reject a proposal by leaving accepted state unchanged.
            proposal
                .accept(&mut s.state, &s.scroll, HEIGHT, cx)
                .unwrap();
        });
        column()
            .fill_width()
            .fill_height()
            .padding(8.)
            .gap(8.)
            .child(label("Files — arrows navigate, Right/Left expand/collapse"))
            .child(label(format!(
                "{} visible, {} selected",
                self.state.rows().len(),
                self.state.selected().len()
            )))
            .child(
                virtual_tree(
                    &self.state,
                    HEIGHT,
                    &self.scroll,
                    cx,
                    change,
                    |_, node, _| {
                        // Render passive content; the tree supplies indentation/disclosure.
                        label(format!(
                            "{}{}",
                            if self.state.selected().contains(&node.key) {
                                "✓ "
                            } else {
                                ""
                            },
                            node.label
                        ))
                        .font_size(12.5)
                        .text_overflow(rxui::TextOverflow::Ellipsis)
                    },
                )
                .accessibility_label("Files")
                .fill_width()
                .fill_height(),
            )
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let mut nodes = Vec::new();
        for folder in 0..100usize {
            let key = folder * 1001;
            nodes.push(TreeNode {
                key: key.into(),
                parent: None,
                label: format!("Folder {folder:03}"),
                branch: true,
            });
            for child in 1..=1000 {
                nodes.push(TreeNode {
                    key: (key + child).into(),
                    parent: Some(key.into()),
                    label: format!("Record {folder:03}/{child:04} — application-owned data"),
                    branch: false,
                });
            }
        }
        let mut state = TreeState::new(TreeData::new(nodes).unwrap(), TreeSelectionMode::Multiple);
        state.set_expanded((0..100usize).map(|folder| (folder * 1001).into()));
        let root = cx.new(|_| Tree {
            scroll: ScrollHandle::new(),
            state,
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — virtual tree (100,100 nodes)")
                .size(520., 480.),
            root,
        )?;
        Ok(())
    })
}
