//! Controlled tab selection, close proposals and panel mounting policy.
use crate::{
    Axis, ButtonVariant, ClickEvent, Element, FocusScope, IntoElement, Key, Listener, SemanticRole,
    ThemeColor, UiError, button, column, row, stack,
};
use taffy::prelude::{AlignItems, JustifyItems, TaffyAuto, fr, length, minmax};

/// Selection proposal; the application updates its selected key in a listener.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabSelectEvent {
    /// Stable application-owned tab key.
    pub key: Key,
}
/// Close proposal. The framework never removes application tabs itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabCloseEvent {
    /// Tab whose close action was requested.
    pub key: Key,
    /// Suggested selection after removal: unchanged for background tabs,
    /// otherwise the following enabled tab, then the preceding enabled tab.
    pub next_selection: Option<Key>,
}
/// Lifetime of inactive panel placements. Entity lifetime remains application-owned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabContentPolicy {
    /// Retain keyed mounts, scroll, editing and remembered focus while inactive.
    /// Hidden panels do no painting/input/semantics but can still evaluate updates.
    #[default]
    KeepMounted,
    /// Only mount the selected panel. Switching disposes its placement/task listeners
    /// and widget state; strong application entities can retain their model data.
    MountSelected,
}
/// Whether arrow-key focus movement also proposes selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabActivation {
    /// Arrows/Home/End move focus and propose selection immediately.
    #[default]
    Automatic,
    /// Arrows/Home/End move focus; Enter/Space activate through the host's defaults.
    Manual,
}
/// One owned tab description, independent of its panel's component/entity lifetime.
#[must_use = "attach the tab to Tabs"]
pub struct Tab {
    key: Key,
    title: String,
    content: Element,
    disabled: bool,
    closable: bool,
}
/// Creates a tab with a stable key, accessible title and arbitrary panel content.
pub fn tab(key: impl Into<Key>, title: impl Into<String>, content: impl IntoElement) -> Tab {
    Tab {
        key: key.into(),
        title: title.into(),
        content: content.into_element(),
        disabled: false,
        closable: false,
    }
}
impl Tab {
    /// Prevents selection/focus of this header. Disabled tabs cannot be selected.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Displays a close button when Tabs has an on_close listener; Delete on the
    /// header proposes the same action. The application owns removal and selection.
    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }
}
/// Controlled tabs builder. No selected key means no active panel. Keys must be
/// unique, and an explicit selection must identify an enabled tab. Invalid
/// descriptions are diagnosed during Ui preparation, without constructor panics.
///
/// ```
/// use rxui::prelude::*;
/// struct Page { selected: Key }
/// impl View for Page {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         tabs().selected(self.selected.clone())
///             .tab(tab("editor", "Editor", text_input("Document")))
///             .tab(tab("output", "Output", label("Build output")))
///             .on_select(cx.listener(|s, e: &TabSelectEvent, _| s.selected = e.key.clone()))
///     }
/// }
/// ```
#[must_use = "attach the tabs to a parent or return them from View::view"]
pub struct Tabs {
    root: Element,
    selected: Option<Key>,
    entries: Vec<Tab>,
    select: Option<Listener<TabSelectEvent>>,
    close: Option<Listener<TabCloseEvent>>,
    policy: TabContentPolicy,
    activation: TabActivation,
    axis: Axis,
}
/// Creates an empty tab group with retained inactive panels and automatic activation.
pub fn tabs() -> Tabs {
    Tabs {
        root: column()
            .fill_width()
            .flex_grow(1.)
            .flex_basis(0.)
            .min_width(0.)
            .min_height(0.),
        selected: None,
        entries: Vec::new(),
        select: None,
        close: None,
        policy: TabContentPolicy::default(),
        activation: TabActivation::default(),
        axis: Axis::Horizontal,
    }
}
impl Tabs {
    /// Current controlled selection; update it in response to TabSelectEvent.
    pub fn selected(mut self, key: impl Into<Key>) -> Self {
        self.selected = Some(key.into());
        self
    }
    /// Appends a tab in header order. Compatible stable keys preserve panel placement.
    pub fn tab(mut self, tab: Tab) -> Self {
        self.entries.push(tab);
        self
    }
    /// Appends dynamic descriptions in application order.
    pub fn tabs(mut self, tabs: impl IntoIterator<Item = Tab>) -> Self {
        self.entries.extend(tabs);
        self
    }
    /// Binds selection proposals to current owner state through cx.listener.
    pub fn on_select(mut self, listener: Listener<TabSelectEvent>) -> Self {
        self.select = Some(listener);
        self
    }
    /// Binds close proposals; the callback must update the collection/selection.
    pub fn on_close(mut self, listener: Listener<TabCloseEvent>) -> Self {
        self.close = Some(listener);
        self
    }
    /// Chooses whether inactive placements survive selection changes.
    pub fn content_policy(mut self, policy: TabContentPolicy) -> Self {
        self.policy = policy;
        self
    }
    /// Chooses automatic or Enter/Space activation for keyboard navigation.
    pub fn activation(mut self, activation: TabActivation) -> Self {
        self.activation = activation;
        self
    }
    /// Horizontal header above content or vertical header beside content.
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }
    /// Stable sibling identity for this entire group.
    pub fn key(mut self, key: impl Into<Key>) -> Self {
        self.root = self.root.key(key);
        self
    }
    /// Fixed logical dimensions for the group.
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.root = self
            .root
            .size(width, height)
            .flex_grow(0.)
            .layout(|style| style.flex_basis = crate::taffy::Dimension::AUTO);
        self
    }
    /// Configures the root width constraint.
    pub fn width(mut self, width: f32) -> Self {
        self.root = self.root.width(width);
        self
    }
    /// Configures the root height constraint.
    pub fn height(mut self, height: f32) -> Self {
        self.root = self.root.height(height);
        self
    }
    /// Fills the parent's width constraint.
    pub fn fill_width(mut self) -> Self {
        self.root = self.root.fill_width();
        self
    }
    /// Fills the parent's height constraint.
    pub fn fill_height(mut self) -> Self {
        self.root = self.root.fill_height();
        self
    }
    /// Configures root flex growth.
    pub fn flex_grow(mut self, factor: f32) -> Self {
        self.root = self.root.flex_grow(factor);
        self
    }
    /// Configures root minimum width.
    pub fn min_width(mut self, width: f32) -> Self {
        self.root = self.root.min_width(width);
        self
    }
    /// Configures root minimum height.
    pub fn min_height(mut self, height: f32) -> Self {
        self.root = self.root.min_height(height);
        self
    }
    /// Full Taffy customization on the group's root.
    pub fn layout(mut self, configure: impl FnOnce(&mut crate::taffy::Style)) -> Self {
        self.root = self.root.layout(configure);
        self
    }
}
#[derive(Clone)]
pub(crate) enum Properties {
    Root {
        selected: Option<Key>,
        keys: Vec<(Key, bool)>,
    },
    List {
        axis: Axis,
        activation: TabActivation,
    },
    Header {
        key: Key,
        select: Option<Listener<TabSelectEvent>>,
        close: Option<Listener<TabCloseEvent>>,
        close_event: TabCloseEvent,
    },
    Close {
        key: Key,
    },
    Panel {
        key: Key,
    },
}
impl Properties {
    pub(crate) fn validate(&self) -> Result<(), UiError> {
        if let Self::Root { selected, keys } = self {
            let mut seen = std::collections::HashSet::new();
            if keys.iter().any(|(key, _)| !seen.insert(key)) {
                return Err(UiError::DuplicateTabKey);
            }
            if selected.as_ref().is_some_and(|key| {
                !keys
                    .iter()
                    .any(|(other, disabled)| other == key && !disabled)
            }) {
                return Err(UiError::InvalidTabSelection);
            }
        }
        Ok(())
    }
}
fn metadata(mut element: Element, properties: Properties) -> Element {
    element.input.get_or_insert_with(Default::default).tabs = Some(Box::new(properties));
    element
}
impl IntoElement for Tabs {
    fn into_element(self) -> Element {
        let keys: Vec<_> = self
            .entries
            .iter()
            .map(|tab| (tab.key.clone(), tab.disabled))
            .collect();
        let first = self
            .entries
            .iter()
            .find(|tab| !tab.disabled)
            .map(|tab| tab.key.clone());
        let mut headers = if self.axis == Axis::Horizontal {
            row()
        } else {
            column()
        };
        headers = headers
            .key("headers")
            .gap(2.)
            .padding(4.)
            .flex_shrink(0.)
            .background(ThemeColor::Surface)
            .accessibility_role(SemanticRole::TabList);
        headers = if self.axis == Axis::Horizontal {
            headers.min_width(0.).scroll_x()
        } else {
            headers.min_height(0.).scroll_y()
        };
        headers = metadata(
            headers,
            Properties::List {
                axis: self.axis,
                activation: self.activation,
            },
        );
        let mut panels = stack()
            .key("panels")
            .fill_width()
            .fill_height()
            .flex_grow(1.)
            .flex_basis(0.)
            .min_width(0.)
            .min_height(0.)
            .layout(|s| {
                s.grid_template_rows = vec![minmax(length(0.), fr(1.))];
                s.grid_template_columns = vec![minmax(length(0.), fr(1.))];
                s.align_items = Some(AlignItems::STRETCH);
                s.justify_items = Some(JustifyItems::STRETCH);
            });
        for (index, entry) in self.entries.into_iter().enumerate() {
            let selected = self.selected.as_ref() == Some(&entry.key);
            let next_selection = if !selected {
                self.selected.clone()
            } else {
                keys.iter()
                    .skip(index + 1)
                    .find(|(_, disabled)| !disabled)
                    .or_else(|| keys[..index].iter().rev().find(|(_, disabled)| !disabled))
                    .map(|(key, _)| key.clone())
            };
            let close_event = TabCloseEvent {
                key: entry.key.clone(),
                next_selection,
            };
            let close = entry.closable.then(|| self.close.clone()).flatten();
            let mut header = button(entry.title.clone())
                .key("tab")
                .disabled(entry.disabled)
                .variant(if selected {
                    ButtonVariant::Default
                } else {
                    ButtonVariant::Quiet
                })
                .tab_stop(
                    selected || (self.selected.is_none() && first.as_ref() == Some(&entry.key)),
                )
                .accessibility_role(SemanticRole::Tab)
                .accessibility_selected(selected);
            if let Some(listener) = &self.select {
                let key = entry.key.clone();
                header = header.on_click(
                    listener.map_event(move |_: &ClickEvent| TabSelectEvent { key: key.clone() }),
                );
            }
            header = metadata(
                header,
                Properties::Header {
                    key: entry.key.clone(),
                    select: self.select.clone(),
                    close: close.clone(),
                    close_event: close_event.clone(),
                },
            );
            let mut group = row().key(entry.key.clone()).flex_shrink(0.).child(header);
            if let Some(listener) = close {
                group = group.child(metadata(
                    button("×")
                        .key("close")
                        .variant(ButtonVariant::Quiet)
                        .tab_stop(false)
                        .accessibility_label(format!("Close {}", entry.title))
                        .on_click(listener.map_event(move |_: &ClickEvent| close_event.clone())),
                    Properties::Close {
                        key: entry.key.clone(),
                    },
                ));
            }
            headers = headers.child(group);
            if selected || self.policy == TabContentPolicy::KeepMounted {
                let mut panel = stack()
                    .layout(|s| {
                        s.grid_template_rows = vec![minmax(length(0.), fr(1.))];
                        s.grid_template_columns = vec![minmax(length(0.), fr(1.))];
                        s.align_items = Some(AlignItems::STRETCH);
                        s.justify_items = Some(JustifyItems::STRETCH);
                    })
                    .key(entry.key.clone())
                    .fill_width()
                    .fill_height()
                    .min_width(0.)
                    .min_height(0.)
                    .focusable(true)
                    .focus_scope(FocusScope::Group)
                    .accessibility_role(SemanticRole::TabPanel)
                    .accessibility_label(entry.title)
                    .child(entry.content);
                if !selected {
                    panel = panel.layout(|s| s.display = crate::taffy::Display::None);
                }
                panels = panels.child(metadata(panel, Properties::Panel { key: entry.key }));
            }
        }
        let root = self.root.layout(|s| {
            s.flex_direction = if self.axis == Axis::Horizontal {
                crate::taffy::FlexDirection::Column
            } else {
                crate::taffy::FlexDirection::Row
            }
        });
        metadata(
            root.child(headers).child(panels),
            Properties::Root {
                selected: self.selected,
                keys,
            },
        )
    }
}
