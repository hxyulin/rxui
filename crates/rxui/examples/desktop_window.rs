//! Standalone popovers, modal dialogs, dock context menus and typed commands.
use rxui::prelude::*;
use std::collections::HashMap;

struct NewPanel;
impl Command for NewPanel {}
struct ClosePanel(Option<(DockNodeId, Key)>);
impl Command for ClosePanel {}
struct RequestRestore;
impl Command for RequestRestore {}
struct ConfirmRestore;
impl Command for ConfirmRestore {}
struct CancelRestore;
impl Command for CancelRestore {}
struct ShowHelp;
impl Command for ShowHelp {}
struct MenuRequest {
    anchor: OverlayAnchor,
    panel: Option<(DockNodeId, Key)>,
}
struct Desktop {
    layout: DockTree,
    notes: HashMap<Key, String>,
    next: u32,
    anchor: AnchorHandle,
    menu: Option<MenuRequest>,
    confirm: bool,
    status: String,
    help: Option<CommandRegistration>,
}
fn first_group(node: &DockNode) -> DockNodeId {
    match node {
        DockNode::Tabs(n) => n.id(),
        DockNode::Split(n) => first_group(n.first()),
    }
}
fn panel_name(key: &Key) -> String {
    match key {
        Key::String(name) if name.as_ref() == "editor" => "Editor".into(),
        Key::String(name) if name.as_ref() == "preview" => "Preview".into(),
        Key::String(name) => name.to_string(),
        Key::Integer(number) => format!("Document {}", number + 1),
    }
}
impl View for Desktop {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let group = first_group(self.layout.root());
        let active = self
            .layout
            .node(group)
            .and_then(DockNode::tabs)
            .and_then(DockTabs::selected)
            .map(|key| (group, key.clone()));
        let new = cx
            .command(NewPanel, |s, _, _| {
                let panel = Key::from(s.next);
                s.next += 1;
                let group = first_group(s.layout.root());
                let index = s.layout.node(group).unwrap().tabs().unwrap().panels().len();
                s.layout.insert(group, index, panel.clone()).unwrap();
                s.layout.select(group, &panel).unwrap();
                s.notes.insert(panel, "New document".into());
                s.menu = None;
                s.status = "Panel created.".into();
            })
            .label("New panel")
            .shortcut(Shortcut::primary("n"));
        let close_target = self.menu.as_ref().and_then(|m| m.panel.clone()).or(active);
        let can_close = close_target
            .as_ref()
            .is_some_and(|(group, key)| self.layout.group_for(key) == Some(*group));
        let close = cx
            .command(ClosePanel(close_target), |s, command, _| {
                if let Some((group, panel)) = &command.0 {
                    match s.layout.apply(&DockEvent::Close {
                        group: *group,
                        panel: panel.clone(),
                    }) {
                        Ok(_) => s.status = "Panel closed; its document data is retained.".into(),
                        Err(error) => s.status = error.to_string(),
                    }
                }
                s.menu = None;
            })
            .label("Close panel")
            .enabled(can_close)
            .shortcut(Shortcut::primary("w"));
        let restore = cx
            .command(RequestRestore, |s, _, _| {
                s.menu = None;
                s.confirm = true;
            })
            .label("Restore layout")
            .shortcut(Shortcut::primary("r").shift());
        let mut tools = row()
            .gap(8.)
            .padding(10.)
            .child(new.button())
            .child(close.button())
            .child(restore.button())
            .child(
                button("Actions…")
                    .anchor_handle(self.anchor.clone())
                    .on_click(cx.listener(|s, _, _| {
                        s.menu = Some(MenuRequest {
                            anchor: s.anchor.clone().into(),
                            panel: None,
                        });
                    })),
            );
        if let Some(request) = &self.menu {
            tools = tools.child(
                popover(
                    request.anchor.clone(),
                    menu()
                        .on_command(close.clone())
                        .child(menu_item(&new))
                        .child(menu_item(&close))
                        .child(menu_item(&restore)),
                )
                .key("actions-menu")
                .width(240.)
                .on_dismiss(cx.listener(|s, _: &DismissEvent, _| s.menu = None)),
            );
        }
        // The popover is logically inside this clipped toolbar, yet paints against
        // the viewport. The toolbar itself keeps ordinary layout/scroll behavior.
        let tools = column().height(64.).fill_width().clip().child(tools);
        let content = dock(&self.layout, |key| {
            let edit_key = key.clone();
            dock_panel(
                panel_name(key),
                column()
                    .padding(16.)
                    .gap(12.)
                    .child(label("Right-click a tab header for commands."))
                    .child(
                        text_input(self.notes.get(key).cloned().unwrap_or_default())
                            .fill_width()
                            .accessibility_label(format!("{} note", panel_name(key)))
                            .on_change(cx.listener(move |s, e: &TextChangeEvent, _| {
                                s.notes.insert(edit_key.clone(), e.value.clone());
                            })),
                    ),
            )
            .closable(true)
        })
        .min_pane_size(140., 120.)
        .on_event(cx.listener(|s, e: &DockEvent, _| {
            if let Err(error) = s.layout.apply(e) {
                s.status = error.to_string();
            }
        }))
        .on_context_menu(cx.listener(|s, e: &DockContextEvent, _| {
            s.menu = Some(MenuRequest {
                anchor: e.position.into(),
                panel: Some((e.group, e.panel.clone())),
            });
        }));
        let mut root=column().fill_width().fill_height().on_command(new).on_command(close).on_command(restore)
            .child(tools).child(content)
            .child(label("Primary+N: new · Primary+W: close · Primary+Shift+R: restore · Primary+Shift+I: app help").padding(8.))
            .child(label(self.status.clone()).padding(8.).color(ThemeColor::TextMuted));
        if self.confirm {
            let confirm = cx
                .command(ConfirmRestore, |s, _, _| {
                    s.layout = DockTree::from_panels(["editor", "preview"]).unwrap();
                    s.confirm = false;
                    s.status = "Layout restored; document edits retained.".into();
                })
                .label("Restore");
            let cancel = cx
                .command(CancelRestore, |s, _, _| s.confirm = false)
                .label("Cancel");
            root = root.child(
                modal(
                    column()
                        .gap(12.)
                        .child(label("Restore the workspace layout?").font_size(22.))
                        .child(label("Document edits remain in application state."))
                        .child(row().gap(8.).child(confirm.button()).child(cancel.button())),
                )
                .key("restore-dialog")
                .width(420.)
                .padding(20.)
                .accessibility_label("Restore workspace layout")
                .on_dismiss(cx.listener(|s, _: &DismissEvent, _| s.confirm = false)),
            );
        }
        root
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let desktop = cx.new(|_| Desktop {
            layout: DockTree::from_panels(["editor", "preview"]).unwrap(),
            notes: HashMap::from([
                (Key::from("editor"), "Editor document".into()),
                (Key::from("preview"), "Preview document".into()),
            ]),
            next: 0,
            anchor: AnchorHandle::new(),
            menu: None,
            confirm: false,
            status: "Ready.".into(),
            help: None,
        });
        let weak = desktop.downgrade();
        let help = cx
            .command(ShowHelp, move |_, cx| {
                let _ = weak.update(cx, |s, _| {
                    s.status =
                        "Application fallback command dispatched from the source window.".into()
                });
            })
            .label("Help")
            .shortcut(Shortcut::primary("i").shift());
        let registration = cx.register_command(&help);
        desktop.update(cx, |s, _| s.help = Some(registration));
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — desktop interactions")
                .size(1100., 760.),
            desktop,
        )?;
        Ok(())
    })
}
