//! Shared entity model for the native and headless editor workbench.

use astrelis_core::geometry::LogicalPoint;
use rxui_core::{
    Axis, Context, Element, Render, button, checkbox, column, label, list, row, split_pane,
    text_field,
};

/// Logical viewport used by the native example and headless suite.
pub const VIEWPORT_WIDTH: f32 = 1_100.0;
/// Logical viewport height used by the native example and headless suite.
pub const VIEWPORT_HEIGHT: f32 = 720.0;

/// One editor document listed in the workbench sidebar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    /// Stable list identity.
    pub id: u64,
    /// Displayed tab/list title.
    pub title: String,
    /// Controlled editor contents.
    pub body: String,
}

/// Entity-owned state shared by the native workbench and its headless tests.
pub struct Workbench {
    /// Persistent sidebar/editor split ratio.
    pub split: f32,
    /// Documents in declaration order.
    pub documents: Vec<Document>,
    /// Selected document identity.
    pub selected: u64,
    /// Workspace name controlled by a text field.
    pub workspace_name: String,
    /// Whether automatic saves are enabled.
    pub autosave: bool,
    /// Whether the settings surface is open.
    pub settings_open: bool,
    /// Sidebar scroll position.
    pub list_offset: LogicalPoint,
    /// Last user-visible status.
    pub status: String,
    next_document: u64,
}

impl Workbench {
    /// Creates the default multi-pane editor workspace.
    pub fn new() -> Self {
        Self {
            split: 0.3,
            documents: vec![
                Document {
                    id: 1,
                    title: "Welcome.md".into(),
                    body: "Welcome to RXUI v2.".into(),
                },
                Document {
                    id: 2,
                    title: "Notes.txt".into(),
                    body: "Entity-owned editor state.".into(),
                },
                Document {
                    id: 3,
                    title: "Roadmap.md".into(),
                    body: "Native host, then scale.".into(),
                },
            ],
            selected: 1,
            workspace_name: "RXUI Workbench".into(),
            autosave: true,
            settings_open: false,
            list_offset: LogicalPoint::ZERO,
            status: "Ready".into(),
            next_document: 4,
        }
    }

    fn selected_document(&self) -> &Document {
        self.documents
            .iter()
            .find(|document| document.id == self.selected)
            .expect("the workbench always retains a selected document")
    }
}

impl Default for Workbench {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for Workbench {
    fn render(&mut self, context: &mut Context<Self>) -> Element {
        let background_enabled = !self.settings_open;
        let toolbar =
            row()
                .gap(8.0)
                .child(
                    button("Save workspace")
                        .enabled(background_enabled)
                        .on_click(context.listener(|this, _, context| {
                            this.status = "Workspace saved".into();
                            context.notify();
                        })),
                )
                .child(button("New document").enabled(background_enabled).on_click(
                    context.listener(|this, _, context| {
                        let id = this.next_document;
                        this.next_document += 1;
                        this.documents.push(Document {
                            id,
                            title: format!("Untitled-{id}.txt"),
                            body: String::new(),
                        });
                        this.selected = id;
                        this.status = "Document created".into();
                        context.notify();
                    }),
                ))
                .child(
                    button("Settings")
                        .enabled(background_enabled)
                        .on_click(context.listener(|this, _, context| {
                            this.settings_open = true;
                            context.notify();
                        })),
                );

        let mut files = list()
            .offset(self.list_offset)
            .on_scroll(context.listener_value(|this, offset, context| {
                this.list_offset = offset;
                context.notify();
            }));
        for document in &self.documents {
            let id = document.id;
            let marker = if id == self.selected { "●" } else { "○" };
            files = files.child(
                button(format!("{marker} {}", document.title))
                    .enabled(background_enabled)
                    .on_click(context.listener(move |this, _, context| {
                        this.selected = id;
                        this.status = format!("Selected document {id}");
                        context.notify();
                    }))
                    .key(id),
            );
        }

        let selected = self.selected_document();
        let selected_id = selected.id;
        let title = selected.title.clone();
        let body = selected.body.clone();
        let sidebar = column().gap(8.0).child(label("Files")).child(files).child(
            button("Delete selected")
                .enabled(background_enabled && self.documents.len() > 1)
                .on_click(context.listener(|this, _, context| {
                    this.documents
                        .retain(|document| document.id != this.selected);
                    this.selected = this.documents[0].id;
                    this.status = "Document deleted".into();
                    context.notify();
                })),
        );
        let mut editor = column()
            .gap(8.0)
            .child(label("Editor"))
            .child(
                text_field("Document title", title).on_input(context.listener_value(
                    move |this, value: String, context| {
                        if let Some(document) = this
                            .documents
                            .iter_mut()
                            .find(|document| document.id == selected_id)
                        {
                            document.title = value;
                            context.notify();
                        }
                    },
                )),
            )
            .child(
                text_field("Document body", body).on_input(context.listener_value(
                    move |this, value: String, context| {
                        if let Some(document) = this
                            .documents
                            .iter_mut()
                            .find(|document| document.id == selected_id)
                        {
                            document.body = value;
                            context.notify();
                        }
                    },
                )),
            )
            .child(label(format!("Status: {}", self.status)));
        if self.status == "Workspace saved" {
            editor = editor.child(button("Dismiss status").on_click(context.listener(
                |this, _, context| {
                    this.status = "Ready".into();
                    context.notify();
                },
            )));
        }
        let workspace = split_pane(Axis::Horizontal, self.split)
            .on_change(context.listener_value(|this, ratio, context| {
                this.split = ratio;
                context.notify();
            }))
            .child(sidebar)
            .child(editor);

        let mut root = column()
            .gap(10.0)
            .child(label(self.workspace_name.clone()))
            .child(toolbar)
            .child(workspace);
        if self.settings_open {
            root =
                root.child(
                    column()
                        .gap(8.0)
                        .child(label("Workspace settings"))
                        .child(
                            text_field("Workspace name", self.workspace_name.clone()).on_input(
                                context.listener_value(|this, value: String, context| {
                                    this.workspace_name = value;
                                    context.notify();
                                }),
                            ),
                        )
                        .child(checkbox("Autosave", self.autosave).on_toggle(
                            context.listener_value(|this, value, context| {
                                this.autosave = value;
                                context.notify();
                            }),
                        ))
                        .child(button("Close settings").on_click(context.listener(
                            |this, _, context| {
                                this.settings_open = false;
                                context.notify();
                            },
                        ))),
                );
        }
        root
    }
}
