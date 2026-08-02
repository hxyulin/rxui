//! Lightweight, non-generic element descriptions and fluent builders.

use std::{
    any::{Any, TypeId},
    fmt,
    ops::RangeInclusive,
    rc::Rc,
};

use astrelis_core::geometry::LogicalPoint;
use rxui_tree::{Axis, Invalidation, NodeId, ScrollAxis, UiTree};

use crate::{Entity, EntityCell, Render, RoutedHandler, RoutedValueHandler, Theme};

/// Specification for a retained element outside RXUI's built-in vocabulary.
///
/// There are two deliberately disjoint phases. [`create`](Self::create) and
/// [`update`](Self::update) synchronize ordinary Rust state; neither schedules
/// retained work. [`changed`](Self::changed) describes exactly which engine
/// passes must observe that synchronized state. When a field feeds more than
/// one phase, include every corresponding bit; when in doubt, widen the answer
/// rather than risk a stale frame. [`Invalidation::LAYOUT`] is already widened
/// by the retained tree to composition, paint, accessibility, and hit testing.
///
/// Child descriptions are reconciled separately through RXUI's normal keyed
/// path. Custom elements that expose children remain responsible for laying
/// those retained children out from their [`rxui_tree::Element::layout`]
/// implementation.
pub trait CustomElementSpec: 'static {
    /// Concrete retained element created for this specification type.
    type Element: rxui_tree::Element;

    /// Creates retained state on first mount or after a specification-type change.
    fn create(&self, theme: &Theme) -> Self::Element;

    /// Synchronizes the retained element with the latest lightweight description.
    fn update(&self, element: &mut Self::Element, theme: &Theme);

    /// Reports the retained passes affected relative to `previous`.
    fn changed(&self, previous: &Self) -> Invalidation;

    /// Builds declarative children reconciled below the retained element.
    fn children(&self) -> Vec<Element> {
        Vec::new()
    }
}

pub(crate) trait ErasedCustomSpec {
    fn spec_type_id(&self) -> TypeId;
    fn mount(&self, tree: &mut UiTree, parent: NodeId, position: usize, theme: &Theme) -> NodeId;
    fn update(
        &self,
        previous: &dyn ErasedCustomSpec,
        tree: &mut UiTree,
        node: NodeId,
        theme: &Theme,
    );
    fn children(&self) -> Vec<Element>;
    fn as_any(&self) -> &dyn Any;
}

struct CustomSpecAdapter<S>(S);

impl<S: CustomElementSpec> ErasedCustomSpec for CustomSpecAdapter<S> {
    fn spec_type_id(&self) -> TypeId {
        TypeId::of::<S>()
    }

    fn mount(&self, tree: &mut UiTree, parent: NodeId, position: usize, theme: &Theme) -> NodeId {
        tree.insert_child_at(parent, position, self.0.create(theme))
            .id()
    }

    fn update(
        &self,
        previous: &dyn ErasedCustomSpec,
        tree: &mut UiTree,
        node: NodeId,
        theme: &Theme,
    ) {
        let previous = previous
            .as_any()
            .downcast_ref::<Self>()
            .expect("matching custom specification TypeId must downcast");
        let invalidation = self.0.changed(&previous.0);
        tree.update_element::<S::Element>(node, invalidation, |element| {
            self.0.update(element, theme);
        });
    }

    fn children(&self) -> Vec<Element> {
        self.0.children()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Stable identity supplied to a child in a reorderable sequence.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// Text identity.
    String(String),
    /// Signed integer identity.
    Signed(i64),
    /// Unsigned integer identity.
    Unsigned(u64),
}

impl fmt::Display for Key {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(value) => value.fmt(formatter),
            Self::Signed(value) => value.fmt(formatter),
            Self::Unsigned(value) => value.fmt(formatter),
        }
    }
}

impl From<String> for Key {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for Key {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

macro_rules! key_from_signed {
    ($($ty:ty),* $(,)?) => {$(
        impl From<$ty> for Key {
            fn from(value: $ty) -> Self {
                Self::Signed(value as i64)
            }
        }
    )*};
}

macro_rules! key_from_unsigned {
    ($($ty:ty),* $(,)?) => {$(
        impl From<$ty> for Key {
            fn from(value: $ty) -> Self {
                Self::Unsigned(value as u64)
            }
        }
    )*};
}

key_from_signed!(i8, i16, i32, i64, isize);
key_from_unsigned!(u8, u16, u32, u64, usize);

pub(crate) type RenderFn = fn(Rc<EntityCell>, &mut crate::App) -> Element;

pub(crate) struct EmbeddedEntity {
    pub(crate) cell: Rc<EntityCell>,
    pub(crate) render: RenderFn,
}

pub(crate) enum ElementKind {
    Custom(Box<dyn ErasedCustomSpec>),
    Flex {
        axis: Axis,
        gap: f32,
        children: Vec<Element>,
    },
    Label {
        text: String,
    },
    Button {
        text: String,
        enabled: bool,
        on_click: Option<RoutedHandler>,
    },
    Checkbox {
        label: String,
        checked: bool,
        on_toggle: Option<RoutedValueHandler<bool>>,
    },
    Slider {
        label: String,
        value: f32,
        range: RangeInclusive<f32>,
        step: f32,
        on_change: Option<RoutedValueHandler<f32>>,
    },
    TextField {
        label: String,
        text: String,
        error: Option<String>,
        on_input: Option<RoutedValueHandler<String>>,
        on_commit: Option<RoutedValueHandler<String>>,
    },
    Scroll {
        axis: ScrollAxis,
        offset: LogicalPoint,
        child: Option<Box<Element>>,
        on_scroll: Option<RoutedValueHandler<LogicalPoint>>,
    },
    SplitPane {
        axis: Axis,
        ratio: f32,
        children: Vec<Element>,
        on_change: Option<RoutedValueHandler<f32>>,
    },
    List {
        children: Vec<Element>,
        offset: LogicalPoint,
        on_scroll: Option<RoutedValueHandler<LogicalPoint>>,
    },
    Entity(EmbeddedEntity),
}

/// A lightweight description reconciled into the retained tree.
///
/// The type is deliberately non-generic: entity routing is erased into
/// [`RoutedHandler`] and child entities carry their erased render entry point.
pub struct Element {
    pub(crate) key: Option<Key>,
    pub(crate) kind: ElementKind,
}

impl fmt::Debug for Element {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            ElementKind::Flex { .. } => "Flex",
            ElementKind::Label { .. } => "Label",
            ElementKind::Button { .. } => "Button",
            ElementKind::Checkbox { .. } => "Checkbox",
            ElementKind::Slider { .. } => "Slider",
            ElementKind::TextField { .. } => "TextField",
            ElementKind::Scroll { .. } => "Scroll",
            ElementKind::SplitPane { .. } => "SplitPane",
            ElementKind::List { .. } => "List",
            ElementKind::Entity(_) => "Entity",
            ElementKind::Custom(_) => "Custom",
        };
        formatter
            .debug_struct("Element")
            .field("key", &self.key)
            .field("kind", &kind)
            .finish()
    }
}

impl Element {
    /// Sets the identity used when this element participates in a keyed list.
    pub fn key(mut self, key: impl Into<Key>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Sets spacing between children of a row or column.
    pub fn gap(mut self, gap: f32) -> Self {
        match &mut self.kind {
            ElementKind::Flex { gap: value, .. } => *value = gap.max(0.0),
            _ => panic!("gap is supported only by row and column elements"),
        }
        self
    }

    /// Appends one child to a row or column.
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        match &mut self.kind {
            ElementKind::Flex { children, .. } => children.push(child.into()),
            ElementKind::Scroll { child: current, .. } => *current = Some(Box::new(child.into())),
            ElementKind::SplitPane { children, .. } => {
                assert!(
                    children.len() < 2,
                    "split panes accept exactly two children"
                );
                children.push(child.into());
            }
            ElementKind::List { children, .. } => children.push(child.into()),
            _ => panic!("children are not supported by this element"),
        }
        self
    }

    /// Appends children to a row or column.
    pub fn children(mut self, children: impl IntoIterator<Item = impl Into<Element>>) -> Self {
        match &mut self.kind {
            ElementKind::Flex {
                children: current, ..
            } => current.extend(children.into_iter().map(Into::into)),
            ElementKind::SplitPane {
                children: current, ..
            } => {
                current.extend(children.into_iter().map(Into::into));
                assert!(
                    current.len() <= 2,
                    "split panes accept exactly two children"
                );
            }
            ElementKind::List {
                children: current, ..
            } => {
                current.extend(children.into_iter().map(Into::into));
            }
            _ => panic!("children are not supported by this element"),
        }
        self
    }

    /// Installs the reusable routed activation produced by [`crate::Context::listener`].
    pub fn on_click(mut self, handler: RoutedHandler) -> Self {
        match &mut self.kind {
            ElementKind::Button { on_click, .. } => *on_click = Some(handler),
            _ => panic!("on_click is supported only by button elements"),
        }
        self
    }

    /// Selects whether a button participates in hit testing and focus.
    pub fn enabled(mut self, enabled: bool) -> Self {
        match &mut self.kind {
            ElementKind::Button {
                enabled: current, ..
            } => *current = enabled,
            _ => panic!("enabled is supported only by button elements"),
        }
        self
    }

    /// Installs a controlled checkbox proposal listener.
    pub fn on_toggle(mut self, handler: RoutedValueHandler<bool>) -> Self {
        match &mut self.kind {
            ElementKind::Checkbox { on_toggle, .. } => *on_toggle = Some(handler),
            _ => panic!("on_toggle is supported only by checkbox elements"),
        }
        self
    }

    /// Installs a controlled numeric proposal listener.
    pub fn on_change(mut self, handler: RoutedValueHandler<f32>) -> Self {
        match &mut self.kind {
            ElementKind::Slider { on_change, .. } | ElementKind::SplitPane { on_change, .. } => {
                *on_change = Some(handler);
            }
            _ => panic!("on_change is supported only by sliders and split panes"),
        }
        self
    }

    /// Installs a listener for every text edit proposal.
    pub fn on_input(mut self, handler: RoutedValueHandler<String>) -> Self {
        match &mut self.kind {
            ElementKind::TextField { on_input, .. } => *on_input = Some(handler),
            _ => panic!("on_input is supported only by text fields"),
        }
        self
    }

    /// Installs a listener for text submitted with Enter.
    pub fn on_commit(mut self, handler: RoutedValueHandler<String>) -> Self {
        match &mut self.kind {
            ElementKind::TextField { on_commit, .. } => *on_commit = Some(handler),
            _ => panic!("on_commit is supported only by text fields"),
        }
        self
    }

    /// Surfaces a validation issue as a semantic label adjacent to a text field.
    pub fn error(mut self, error: impl Into<String>) -> Self {
        match &mut self.kind {
            ElementKind::TextField { error: current, .. } => *current = Some(error.into()),
            _ => panic!("error is supported only by text fields"),
        }
        self
    }

    /// Sets the controlled scroll offset.
    pub fn offset(mut self, offset: LogicalPoint) -> Self {
        match &mut self.kind {
            ElementKind::Scroll {
                offset: current, ..
            }
            | ElementKind::List {
                offset: current, ..
            } => *current = offset,
            _ => panic!("offset is supported only by scroll and list elements"),
        }
        self
    }

    /// Installs a controlled scroll-offset proposal listener.
    pub fn on_scroll(mut self, handler: RoutedValueHandler<LogicalPoint>) -> Self {
        match &mut self.kind {
            ElementKind::Scroll { on_scroll, .. } | ElementKind::List { on_scroll, .. } => {
                *on_scroll = Some(handler)
            }
            _ => panic!("on_scroll is supported only by scroll and list elements"),
        }
        self
    }

    /// Sets the keyboard adjustment step for a slider.
    pub fn step(mut self, step: f32) -> Self {
        match &mut self.kind {
            ElementKind::Slider { step: current, .. } => *current = step.max(0.0),
            _ => panic!("step is supported only by slider elements"),
        }
        self
    }
}

impl<T: Render> From<Entity<T>> for Element {
    fn from(entity: Entity<T>) -> Self {
        Self {
            key: None,
            kind: ElementKind::Entity(EmbeddedEntity {
                cell: entity.cell,
                render: crate::render_entity::<T>,
            }),
        }
    }
}

/// Creates a declarative custom retained element.
pub fn custom<S: CustomElementSpec>(spec: S) -> Element {
    Element {
        key: None,
        kind: ElementKind::Custom(Box::new(CustomSpecAdapter(spec))),
    }
}

fn flex(axis: Axis) -> Element {
    Element {
        key: None,
        kind: ElementKind::Flex {
            axis,
            gap: 0.0,
            children: Vec::new(),
        },
    }
}

/// Creates a vertical child container.
pub fn column() -> Element {
    flex(Axis::Vertical)
}

/// Creates a horizontal child container.
pub fn row() -> Element {
    flex(Axis::Horizontal)
}

/// Creates a retained text label description.
pub fn label(text: impl Into<String>) -> Element {
    Element {
        key: None,
        kind: ElementKind::Label { text: text.into() },
    }
}

/// Creates a minimally styled semantic button containing a label.
pub fn button(text: impl Into<String>) -> Element {
    Element {
        key: None,
        kind: ElementKind::Button {
            text: text.into(),
            enabled: true,
            on_click: None,
        },
    }
}

/// Creates a controlled checkbox.
pub fn checkbox(label: impl Into<String>, checked: bool) -> Element {
    Element {
        key: None,
        kind: ElementKind::Checkbox {
            label: label.into(),
            checked,
            on_toggle: None,
        },
    }
}

/// Creates a controlled horizontal slider.
pub fn slider(label: impl Into<String>, value: f32, range: RangeInclusive<f32>) -> Element {
    Element {
        key: None,
        kind: ElementKind::Slider {
            label: label.into(),
            value,
            range,
            step: 1.0,
            on_change: None,
        },
    }
}

/// Creates a controlled single-line text field.
pub fn text_field(label: impl Into<String>, text: impl Into<String>) -> Element {
    Element {
        key: None,
        kind: ElementKind::TextField {
            label: label.into(),
            text: text.into(),
            error: None,
            on_input: None,
            on_commit: None,
        },
    }
}

/// Creates a controlled clipped scroll viewport.
pub fn scroll() -> Element {
    Element {
        key: None,
        kind: ElementKind::Scroll {
            axis: ScrollAxis::Vertical,
            offset: LogicalPoint::ZERO,
            child: None,
            on_scroll: None,
        },
    }
}

/// Creates a controlled two-child split pane.
pub fn split_pane(axis: Axis, ratio: f32) -> Element {
    Element {
        key: None,
        kind: ElementKind::SplitPane {
            axis,
            ratio,
            children: Vec::new(),
            on_change: None,
        },
    }
}

/// Creates a keyed vertical sequence inside a scroll viewport.
///
/// Virtualization is intentionally deferred to Stage 6; this builder uses the
/// normal keyed reconciler and retains every supplied row.
pub fn list() -> Element {
    Element {
        key: None,
        kind: ElementKind::List {
            children: Vec::new(),
            offset: LogicalPoint::ZERO,
            on_scroll: None,
        },
    }
}
