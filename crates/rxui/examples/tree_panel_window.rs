//! A real controlled hierarchy: 100 folders, each containing 1,000 leaf records.
//! Arrow keys navigate; Right/Left expand/collapse; Shift extends selection;
//! Command/Control-click toggles selection. All rows keep focus on the tree root.
use rxui::prelude::*;
struct Tree {
    scroll: ScrollHandle,
    state: TreeState,
}
impl View for Tree {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        // Slots match the theme's `list_row` height, following density switches.
        let height = cx.theme().sizes().row_height;
        let change = cx.listener(move |s, proposal: &TreeProposal, cx| {
            // Applications may reject a proposal by leaving accepted state unchanged.
            proposal
                .accept(&mut s.state, &s.scroll, height, cx)
                .unwrap();
        });
        column()
            .fill_width()
            .fill_height()
            .padding(16.)
            .gap(8.)
            .child(label("Files").text_role(TextRole::Heading))
            .child(
                label(format!(
                    "{} visible, {} selected · arrows navigate, Right/Left expand/collapse",
                    self.state.rows().len(),
                    self.state.selected().len()
                ))
                .color(ThemeColor::TextMuted),
            )
            .child(
                virtual_tree(
                    &self.state,
                    height,
                    &self.scroll,
                    cx,
                    change,
                    |_, node, _| {
                        // Render passive content; the tree supplies indentation,
                        // disclosure and the theme's row selection.
                        label(node.label.clone()).text_overflow(rxui::TextOverflow::Ellipsis)
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
        let mut state = TreeState::new(TreeData::new(nodes).unwrap(), SelectionMode::Multiple);
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
