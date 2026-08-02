//! Lightweight, non-generic element descriptions and fluent builders.

use std::{fmt, rc::Rc};

use rxui_tree::Axis;

use crate::{Entity, EntityCell, Render, RoutedHandler};

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
        on_click: Option<RoutedHandler>,
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
            ElementKind::Entity(_) => "Entity",
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
            _ => panic!("children are supported only by row and column elements"),
        }
        self
    }

    /// Appends children to a row or column.
    pub fn children(mut self, children: impl IntoIterator<Item = impl Into<Element>>) -> Self {
        match &mut self.kind {
            ElementKind::Flex {
                children: current, ..
            } => current.extend(children.into_iter().map(Into::into)),
            _ => panic!("children are supported only by row and column elements"),
        }
        self
    }

    /// Installs the one-shot routed activation produced by [`crate::Context::listener`].
    pub fn on_click(mut self, handler: RoutedHandler) -> Self {
        match &mut self.kind {
            ElementKind::Button { on_click, .. } => *on_click = Some(handler),
            _ => panic!("on_click is supported only by button elements"),
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
            on_click: None,
        },
    }
}
