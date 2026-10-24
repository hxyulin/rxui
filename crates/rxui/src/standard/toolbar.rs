//! Toolbars and toggle buttons.
use super::{Part, ToggleEvent, part, root_builders};
use crate::{
    Axis, ButtonVariant, ClickEvent, Element, Icon, IntoElement, Listener, PaintStyle,
    SemanticRole, ThemeColor, button, row,
};
use taffy::prelude::AlignItems;

/// A row of controls with one Tab stop; see [`toolbar`].
#[must_use = "attach the toolbar to a parent or return it from View::view"]
pub struct Toolbar {
    root: Element,
}
root_builders!(Toolbar);

/// A row of buttons, icon buttons, toggle buttons, selects, segmented controls
/// and separators with the [`SemanticRole::Toolbar`] role.
///
/// The toolbar is one Tab stop. Tab enters it at the item focused last, or at
/// its first item (its last with Shift-Tab), and the next Tab leaves it.
/// Left and Right move focus to the previous or next item, wrapping, and Home
/// and End move it to the first and last item. Separators and disabled items
/// are skipped. A control that uses an arrow key itself, such as a segmented
/// control between its ends, keeps it.
///
/// Items are vertically centered with a 4-unit gap, and the toolbar is at
/// least [`crate::ThemeMetrics::input_height`] tall. Icon-only items need an
/// accessible name.
///
/// ```
/// use rxui::{Icon, icon_button, prelude::*};
/// struct Editor { wrap: bool }
/// impl View for Editor {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         toolbar().accessibility_label("Editor")
///             .child(icon_button(Icon::PLUS).accessibility_label("New"))
///             .child(separator())
///             .child(toggle_button(self.wrap, Icon::TEXT_WRAP).accessibility_label("Wrap")
///                 .on_change(cx.listener(|s, e: &ToggleEvent, _| s.wrap = e.checked)))
///     }
/// }
/// ```
pub fn toolbar() -> Toolbar {
    let root = part(row(), Part::Toolbar { vertical: false })
        .gap(4.)
        .align_items(AlignItems::CENTER)
        .accessibility_role(SemanticRole::Toolbar)
        .roving_focus(Axis::Horizontal);
    Toolbar { root }
}
impl Toolbar {
    /// Appends an item in navigation order.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.root = self.root.child(child);
        self
    }
    /// Appends items in navigation order.
    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.root = self.root.children(children);
        self
    }
    /// Stacks the items in a column, horizontally centered, at least
    /// `input_height` wide. Up and Down move focus instead of Left and Right.
    pub fn vertical(mut self) -> Self {
        self.root.style.flex_direction = taffy::FlexDirection::Column;
        self.root = part(self.root, Part::Toolbar { vertical: true }).roving_focus(Axis::Vertical);
        self
    }
}
impl IntoElement for Toolbar {
    fn into_element(self) -> Element {
        self.root
    }
}

/// What a [`toggle_button`] shows: a caption, an icon, or any element.
pub enum ToggleContent {
    /// A caption, which also names the button.
    Text(String),
    /// An icon alone, centered in a square button. The button needs an
    /// accessible name.
    Icon(Icon),
    /// Composed content, such as a label with a leading icon.
    Element(Box<Element>),
}
impl From<&str> for ToggleContent {
    fn from(text: &str) -> Self {
        Self::Text(text.into())
    }
}
impl From<String> for ToggleContent {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}
impl From<Icon> for ToggleContent {
    fn from(icon: Icon) -> Self {
        Self::Icon(icon)
    }
}
impl From<Element> for ToggleContent {
    fn from(element: Element) -> Self {
        Self::Element(Box::new(element))
    }
}

/// Controlled toggle button builder; see [`toggle_button`].
#[must_use = "attach the toggle button to a parent or return it from View::view"]
pub struct ToggleButton {
    root: Element,
    pressed: bool,
    disabled: bool,
    change: Option<Listener<ToggleEvent>>,
}
root_builders!(ToggleButton);

/// A quiet button that stays pressed while `pressed` is true, such as a
/// formatting or view option in a toolbar.
///
/// Unpressed, it looks like a [`ButtonVariant::Quiet`] button. Pressed, it has
/// the [`ThemeColor::RowSelected`] fill with [`ThemeColor::Text`] content.
/// It is a [`SemanticRole::Button`] whose check state publishes the pressed
/// state. Clicks, Enter, Space and assistive activation propose the opposite
/// state through [`ToggleButton::on_change`].
///
/// An icon toggle is an [`crate::icon_button`]: square and
/// [`crate::ThemeMetrics::input_height`] on each side. Like an icon button, it
/// must have an accessible name; preparation fails with
/// [`crate::UiError::MissingAccessibleName`] without one.
///
/// ```
/// use rxui::{Icon, prelude::*};
/// struct Find { case: bool, words: bool }
/// impl View for Find {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         row().gap(4.)
///             .child(toggle_button(self.case, "Aa").accessibility_label("Match case")
///                 .on_change(cx.listener(|s, e: &ToggleEvent, _| s.case = e.checked)))
///             .child(toggle_button(self.words, Icon::TEXT_WRAP).accessibility_label("Wrap")
///                 .on_change(cx.listener(|s, e: &ToggleEvent, _| s.words = e.checked)))
///     }
/// }
/// ```
pub fn toggle_button(pressed: bool, content: impl Into<ToggleContent>) -> ToggleButton {
    let root = match content.into() {
        ToggleContent::Text(text) => button(text).variant(ButtonVariant::Quiet),
        ToggleContent::Icon(icon) => crate::icon_button(icon),
        ToggleContent::Element(element) => button(*element).variant(ButtonVariant::Quiet),
    };
    ToggleButton {
        root,
        pressed,
        disabled: false,
        change: None,
    }
}
impl ToggleButton {
    /// Receives the proposed next state. Without a listener the button still
    /// focuses and activates but cannot change.
    pub fn on_change(mut self, listener: Listener<ToggleEvent>) -> Self {
        self.change = Some(listener);
        self
    }
    /// Disables activation and removes the button from Tab traversal. A
    /// pressed button keeps its fill.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}
impl IntoElement for ToggleButton {
    fn into_element(self) -> Element {
        let mut root = self
            .root
            .disabled(self.disabled)
            .accessibility_checked(self.pressed);
        if self.pressed {
            root = root
                .background(ThemeColor::RowSelected)
                .color(ThemeColor::Text)
                .disabled_style(PaintStyle::new().color(ThemeColor::TextDisabled));
        }
        if let Some(listener) = self.change {
            let next = ToggleEvent {
                checked: !self.pressed,
            };
            root = root.on_click(listener.map_event(move |_: &ClickEvent| next));
        }
        root
    }
}
