use crate::{
    Entity, Listener, PaintStyle, StyleColor, TextChangeEvent, TextSubmitEvent, Theme, ViewContext,
    ui::UiError,
};
use std::{collections::HashSet, rc::Rc, sync::Arc};
use taffy::{Style, geometry::Size, prelude::*, style::CompactLength};

/// Persistent component that describes its UI from read-only current state.
/// Returned descriptions own their data; borrowed state cannot escape evaluation.
pub trait View: Sized + 'static {
    /// Builds the description for one mounted instance.
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement;
}

/// Converts builders and stateful entities into owned element descriptions.
pub trait IntoElement: Sized {
    /// Converts this value into one description.
    fn into_element(self) -> Element;
    /// Assigns an identity within the parent's sibling scope.
    fn key(self, key: impl Into<Key>) -> Element {
        let mut element = self.into_element();
        element.key = Some(key.into());
        element
    }
}

/// Identity scoped to siblings. Integer and string keys occupy distinct domains.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// Owned string key.
    String(Arc<str>),
    /// Integer key, preserving all supported integer values.
    Integer(i128),
}
impl From<&str> for Key {
    fn from(value: &str) -> Self {
        Self::String(value.into())
    }
}
impl From<String> for Key {
    fn from(value: String) -> Self {
        Self::String(value.into())
    }
}
macro_rules! integer_keys {
    ($($ty:ty),*) => {$ (
        impl From<$ty> for Key {
            fn from(value: $ty) -> Self { Self::Integer(value as i128) }
        }
    )*};
}
integer_keys!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize, i128);

/// Semantic button activation, shared by pointer and keyboard dispatch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClickEvent;

/// Linear RGBA color used by element painting.
pub type Color = [f32; 4];
/// Axes with retained scroll offsets; overflow is clipped to the container content box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAxes {
    /// Vertical scrolling only.
    Vertical,
    /// Horizontal scrolling only.
    Horizontal,
    /// Both axes.
    Both,
}
impl ScrollAxes {
    pub(crate) fn allowed(self) -> [bool; 2] {
        match self {
            Self::Vertical => [false, true],
            Self::Horizontal => [true, false],
            Self::Both => [true, true],
        }
    }
}

#[derive(Clone)]
pub(crate) enum ElementKind {
    Row,
    Column,
    Label(String),
    Button {
        text: String,
        listener: Option<Listener<ClickEvent>>,
        disabled: bool,
    },
    TextInput {
        value: String,
        change: Option<Listener<TextChangeEvent>>,
        submit: Option<Listener<TextSubmitEvent>>,
        disabled: bool,
        read_only: bool,
    },
    Component(Rc<dyn crate::ui::Component>),
}

/// Owned UI description. Properties are stored directly, rather than individually boxed.
/// Children and strings belong to the description; reconciliation retains node identity.
#[must_use = "attach the element to a parent or return it from View::view"]
#[derive(Clone)]
pub struct Element {
    pub(crate) key: Option<Key>,
    pub(crate) kind: ElementKind,
    pub(crate) children: Vec<Element>,
    pub(crate) style: Style,
    pub(crate) paint: PaintStyle,
    pub(crate) states: Option<Box<crate::theme::StateStyles>>,
    pub(crate) theme: Option<Theme>,
    pub(crate) font_size: Option<f32>,
    pub(crate) layout_overrides: u8,
    pub(crate) clip: bool,
    pub(crate) scroll: Option<ScrollAxes>,
    pub(crate) semantics: Option<Box<crate::semantics::Properties>>,
}
impl IntoElement for Element {
    fn into_element(self) -> Element {
        self
    }
}
impl<T: View> IntoElement for Entity<T> {
    fn into_element(self) -> Element {
        Element::new(ElementKind::Component(Rc::new(crate::ui::ViewEntity(self))))
    }
}
impl Element {
    fn new(kind: ElementKind) -> Self {
        Self {
            key: None,
            kind,
            children: Vec::new(),
            style: Style {
                flex_shrink: 0.,
                flex_direction: FlexDirection::Column,
                ..Style::default()
            },
            paint: PaintStyle::new(),
            states: None,
            theme: None,
            font_size: None,
            layout_overrides: 0,
            clip: false,
            scroll: None,
            semantics: None,
        }
    }
    fn control_defaults(&mut self, input: bool) {
        let theme = Theme::default();
        let metrics = theme.sizes();
        let padding = if input {
            metrics.input_padding
        } else {
            metrics.button_padding
        };
        self.style.padding = taffy::geometry::Rect {
            left: length(padding),
            right: length(padding),
            top: length(padding),
            bottom: length(padding),
        };
        let border = metrics.border_width;
        self.style.border = taffy::geometry::Rect {
            left: length(border),
            right: length(border),
            top: length(border),
            bottom: length(border),
        };
        if input {
            self.style.size.width = length(metrics.input_width);
            self.style.size.height = length(metrics.input_height);
        }
    }
    /// Appends a child. Leaf elements reject children during preparation.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_element());
        self
    }
    /// Appends children in description order; dynamic collections should use stable keys.
    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.children
            .extend(children.into_iter().map(IntoElement::into_element));
        self
    }
    /// Uniform padding in logical units.
    pub fn padding(mut self, value: f32) -> Self {
        self.layout_overrides |= 1;
        self.style.padding = taffy::geometry::Rect {
            left: length(value),
            right: length(value),
            top: length(value),
            bottom: length(value),
        };
        self
    }
    /// Gap between children in logical units.
    pub fn gap(mut self, value: f32) -> Self {
        self.style.gap = Size {
            width: length(value),
            height: length(value),
        };
        self
    }
    /// Fixed width in logical units.
    pub fn width(mut self, value: f32) -> Self {
        self.layout_overrides |= 2;
        self.style.size.width = length(value);
        self
    }
    /// Fixed height in logical units.
    pub fn height(mut self, value: f32) -> Self {
        self.layout_overrides |= 4;
        self.style.size.height = length(value);
        self
    }
    /// Fills the parent's content width.
    pub fn fill_width(mut self) -> Self {
        self.layout_overrides |= 2;
        self.style.size.width = percent(1.);
        self
    }
    /// Fills the parent's content height.
    pub fn fill_height(mut self) -> Self {
        self.layout_overrides |= 4;
        self.style.size.height = percent(1.);
        self
    }
    /// Cross-axis child alignment.
    pub fn align_items(mut self, value: AlignItems) -> Self {
        self.style.align_items = Some(value);
        self
    }
    /// Main-axis child distribution.
    pub fn justify_content(mut self, value: JustifyContent) -> Self {
        self.style.justify_content = Some(value);
        self
    }
    /// Local background fill, literal linear RGBA or a semantic theme token.
    pub fn background(mut self, value: impl Into<StyleColor>) -> Self {
        self.paint = self.paint.background(value);
        self
    }
    /// Inherited text color. Tokens resolve in each descendant's nearest theme.
    /// Changing color does not invalidate text measurement.
    pub fn color(mut self, value: impl Into<StyleColor>) -> Self {
        self.paint = self.paint.color(value);
        self
    }
    /// Inherited text size in logical units. Changing effective size invalidates measurement.
    pub fn font_size(mut self, value: f32) -> Self {
        self.font_size = Some(value);
        self
    }
    /// Overrides the theme for this complete placement subtree, including components.
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
    /// Installs a paint patch. Fields provided by the patch overwrite earlier builders;
    /// omitted fields preserve them. Text color inherits; other paint properties stay local.
    pub fn paint_style(mut self, style: PaintStyle) -> Self {
        self.paint.merge(style);
        self
    }
    /// Removes the default/local fill, including a control's theme fill.
    pub fn no_background(mut self) -> Self {
        self.paint = self.paint.no_background();
        self
    }
    /// Uniform border included in layout. Color is literal or theme-bound.
    pub fn border(mut self, width: f32, color: impl Into<StyleColor>) -> Self {
        self.layout_overrides |= 8;
        self.style.border = taffy::geometry::Rect {
            left: length(width),
            right: length(width),
            top: length(width),
            bottom: length(width),
        };
        self.paint = self.paint.border_color(color);
        self
    }
    /// Uniform painted corner radius, without rounded clipping or hit testing.
    pub fn radius(mut self, radius: f32) -> Self {
        self.paint = self.paint.radius(radius);
        self
    }
    /// Paint-only hovered-control patch, applied after ordinary explicit overrides.
    pub fn hover_style(mut self, style: PaintStyle) -> Self {
        self.states
            .get_or_insert_with(Default::default)
            .hover
            .merge(style);
        self
    }
    /// Paint-only pressed-control patch. Pressed takes precedence over hover.
    pub fn pressed_style(mut self, style: PaintStyle) -> Self {
        self.states
            .get_or_insert_with(Default::default)
            .pressed
            .merge(style);
        self
    }
    /// Paint-only disabled-control patch. Disabled takes precedence over pointer states.
    pub fn disabled_style(mut self, style: PaintStyle) -> Self {
        self.states
            .get_or_insert_with(Default::default)
            .disabled
            .merge(style);
        self
    }
    /// Clips descendant painting and hit testing to this element's content box.
    pub fn clip(mut self) -> Self {
        self.clip = true;
        self.style.overflow = taffy::geometry::Point {
            x: taffy::Overflow::Hidden,
            y: taffy::Overflow::Hidden,
        };
        self
    }
    /// Retained vertical scrolling. Give the container a bounded height.
    pub fn scroll_y(self) -> Self {
        self.scrolling(ScrollAxes::Vertical)
    }
    /// Retained horizontal scrolling. Give the container a bounded width.
    pub fn scroll_x(self) -> Self {
        self.scrolling(ScrollAxes::Horizontal)
    }
    /// Retained scrolling on both axes, within a bounded container.
    pub fn scroll(self) -> Self {
        self.scrolling(ScrollAxes::Both)
    }
    fn scrolling(mut self, axes: ScrollAxes) -> Self {
        self.clip = true;
        self.scroll = Some(axes);
        let [x, y] = axes.allowed();
        self.style.overflow = taffy::geometry::Point {
            x: if x {
                taffy::Overflow::Scroll
            } else {
                taffy::Overflow::Hidden
            },
            y: if y {
                taffy::Overflow::Scroll
            } else {
                taffy::Overflow::Hidden
            },
        };
        self
    }
    /// Full Taffy flex style customization, before validation and reconciliation.
    /// Changed box fields become explicit theme overrides. To freeze a field equal
    /// to its existing default, use its dedicated builder (padding/width/height/border).
    /// Use clip/scroll builders for retained clipping/interaction. Taffy overflow
    /// settings alone do not enable those behaviors or text alignment.
    pub fn layout(mut self, f: impl FnOnce(&mut Style)) -> Self {
        let before = self.style.clone();
        f(&mut self.style);
        if before.padding != self.style.padding {
            self.layout_overrides |= 1;
        }
        if before.size.width != self.style.size.width {
            self.layout_overrides |= 2;
        }
        if before.size.height != self.style.size.height {
            self.layout_overrides |= 4;
        }
        if before.border != self.style.border {
            self.layout_overrides |= 8;
        }
        self
    }
    /// Installs semantic activation on a button; other element kinds reject it.
    pub fn on_click(mut self, listener: Listener<ClickEvent>) -> Self {
        if let ElementKind::Button {
            listener: target, ..
        } = &mut self.kind
        {
            *target = Some(listener);
        } else {
            panic!("on_click requires a button element");
        }
        self
    }
    /// Disables activation and removes a button from sequential focus traversal.
    pub fn disabled(mut self, disabled: bool) -> Self {
        if let ElementKind::Button {
            disabled: target, ..
        }
        | ElementKind::TextInput {
            disabled: target, ..
        } = &mut self.kind
        {
            *target = disabled;
        } else {
            panic!("disabled requires a button or text input");
        }
        self
    }
    /// Installs controlled text proposals; accept by updating the value supplied to text_input.
    pub fn on_change(mut self, listener: Listener<TextChangeEvent>) -> Self {
        if let ElementKind::TextInput { change, .. } = &mut self.kind {
            *change = Some(listener);
        } else {
            panic!("on_change requires a text input");
        }
        self
    }
    /// Installs single-line Enter submission outside active IME composition.
    pub fn on_submit(mut self, listener: Listener<TextSubmitEvent>) -> Self {
        if let ElementKind::TextInput { submit, .. } = &mut self.kind {
            *submit = Some(listener);
        } else {
            panic!("on_submit requires a text input");
        }
        self
    }
    /// Keeps selection/copy available while preventing committed edits and IME.
    pub fn read_only(mut self, value: bool) -> Self {
        if let ElementKind::TextInput { read_only, .. } = &mut self.kind {
            *read_only = value;
        } else {
            panic!("read_only requires a text input");
        }
        self
    }
    /// Accessible name, separate from displayed text/value. Buttons default to
    /// their caption; text inputs should supply a meaningful name explicitly.
    pub fn accessibility_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.semantics.get_or_insert_with(Default::default).label = Some(label.into());
        self
    }
    /// Supplemental accessibility help, independent of the control's value.
    pub fn accessibility_description(mut self, description: impl Into<Arc<str>>) -> Self {
        self.semantics
            .get_or_insert_with(Default::default)
            .description = Some(description.into());
        self
    }
    /// Override inferred semantics, for example a heading, form or list container.
    /// This does not add focus or interaction to an otherwise passive element.
    pub fn accessibility_role(mut self, role: crate::SemanticRole) -> Self {
        self.semantics.get_or_insert_with(Default::default).role = Some(role);
        self
    }
    /// Excludes this complete subtree from semantics without changing its painting
    /// or ordinary input behavior. Assistive actions cannot target hidden descendants.
    pub fn accessibility_hidden(mut self, hidden: bool) -> Self {
        self.semantics.get_or_insert_with(Default::default).hidden = hidden;
        self
    }
    pub(crate) fn validate(&self) -> Result<(), UiError> {
        let finite = |v: CompactLength| v.is_auto() || v.value().is_finite();
        let nonnegative = |v: CompactLength| finite(v) && (v.is_auto() || v.value() >= 0.);
        let s = &self.style;
        let sizes = [
            s.size.width.into_raw(),
            s.size.height.into_raw(),
            s.min_size.width.into_raw(),
            s.min_size.height.into_raw(),
            s.max_size.width.into_raw(),
            s.max_size.height.into_raw(),
        ];
        if self.font_size.is_some_and(|v| !v.is_finite() || v <= 0.)
            || sizes.iter().any(|v| !nonnegative(*v))
            || !nonnegative(s.flex_basis.into_raw())
            || [s.flex_grow, s.flex_shrink, s.scrollbar_width]
                .iter()
                .any(|v| !v.is_finite() || *v < 0.)
            || s.aspect_ratio.is_some_and(|v| !v.is_finite() || v <= 0.)
            || [s.padding, s.border].iter().any(|r| {
                [r.left, r.right, r.top, r.bottom]
                    .iter()
                    .any(|v| !nonnegative(v.into_raw()))
            })
            || [s.margin, s.inset].iter().any(|r| {
                [r.left, r.right, r.top, r.bottom]
                    .iter()
                    .any(|v| !finite(v.into_raw()))
            })
            || [s.gap.width, s.gap.height]
                .iter()
                .any(|v| !nonnegative(v.into_raw()))
        {
            return Err(UiError::InvalidStyle);
        }
        self.paint.validate()?;
        if let Some(states) = &self.states {
            states.hover.validate()?;
            states.pressed.validate()?;
            states.disabled.validate()?;
        }
        if let Some(theme) = &self.theme {
            theme.validate()?;
        }
        if let ElementKind::TextInput { value, .. } = &self.kind
            && value.chars().any(char::is_control)
        {
            return Err(UiError::InvalidTextValue);
        }
        if !self.children.is_empty()
            && matches!(
                self.kind,
                ElementKind::Label(_)
                    | ElementKind::Button { .. }
                    | ElementKind::TextInput { .. }
                    | ElementKind::Component(_)
            )
        {
            return Err(UiError::LeafChildren);
        }
        if self.scroll.is_some() && !matches!(self.kind, ElementKind::Row | ElementKind::Column) {
            return Err(UiError::InvalidStyle);
        }
        let mut keys = HashSet::new();
        for child in &self.children {
            if let Some(key) = &child.key
                && !keys.insert(key)
            {
                return Err(UiError::DuplicateKey(key.clone()));
            }
            child.validate()?;
        }
        Ok(())
    }
}

/// Flex row, with children in description order.
pub fn row() -> Element {
    let mut element = Element::new(ElementKind::Row);
    element.style.flex_direction = FlexDirection::Row;
    element
}
/// Flex column, with children in description order.
pub fn column() -> Element {
    Element::new(ElementKind::Column)
}
/// Owned text leaf measured by the host's text measurer.
pub fn label(text: impl Into<String>) -> Element {
    Element::new(ElementKind::Label(text.into()))
}
/// Button leaf with default padding/background, activated via on_click.
pub fn button(text: impl Into<String>) -> Element {
    let mut element = Element::new(ElementKind::Button {
        text: text.into(),
        listener: None,
        disabled: false,
    });
    element.control_defaults(false);
    element
}

/// Controlled single-line input. Value changes only when the application accepts an
/// on_change proposal. Selection and IME composition belong to each placement.
/// Application values must contain no control characters. User line breaks/tabs
/// normalize to spaces. Missing on_change behaves as a selectable read-only value.
///
/// ```
/// use rxui::prelude::*;
/// struct Form { value: String }
/// impl View for Form {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         text_input(self.value.clone()).key("value")
///             .on_change(cx.listener(|this, edit: &TextChangeEvent, _| {
///                 this.value = edit.value.clone();
///             }))
///     }
/// }
/// ```
pub fn text_input(value: impl Into<String>) -> Element {
    let mut element = Element::new(ElementKind::TextInput {
        value: value.into(),
        change: None,
        submit: None,
        disabled: false,
        read_only: false,
    });
    element.control_defaults(true);
    element
}
