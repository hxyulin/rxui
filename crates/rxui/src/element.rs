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

/// Default control appearance; explicit paint builders/state patches override it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Bordered control with neutral state fills.
    #[default]
    Default,
    /// Accent fill for the primary action.
    Primary,
    /// Text/content without a resting fill/border; hover/pressed still show feedback.
    Quiet,
}
/// Pointer participation of an element and its subtree. Keyboard/semantic focus is separate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerEvents {
    /// Controls receive pointer input; passive content lets lower siblings receive clicks.
    #[default]
    Auto,
    /// Ignore this complete subtree for pointer targeting and wheel scrolling.
    None,
    /// Block lower siblings within this element's clipped border box; children still work.
    Block,
}
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
    Stack,
    Scrollbar(Box<crate::controls::ScrollbarProps>),
    Splitter(Box<crate::controls::SplitterProps>),
    Label(String),
    Image(Box<crate::image::Properties>),
    Button {
        text: Option<String>,
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
    Custom(Rc<dyn crate::custom::AnyCustom>),
}

/// Owned UI description with common properties inline and optional input/semantic
/// bundles allocated only when used. Children and strings belong to the description;
/// reconciliation retains compatible keyed node identity.
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
    pub(crate) text_style: Option<Box<crate::typography::Overrides>>,
    pub(crate) layout_overrides: u8,
    pub(crate) button_variant: ButtonVariant,
    pub(crate) z_index: i32,
    pub(crate) pointer_events: PointerEvents,
    pub(crate) inert: bool,
    pub(crate) opacity: f32,
    pub(crate) clip: bool,
    pub(crate) scroll: Option<ScrollAxes>,
    pub(crate) scroll_handle: Option<crate::ScrollHandle>,
    pub(crate) semantics: Option<Box<crate::semantics::Properties>>,
    pub(crate) input: Option<Box<crate::input::InputProperties>>,
}
impl IntoElement for Element {
    fn into_element(self) -> Element {
        self
    }
}
impl IntoElement for String {
    fn into_element(self) -> Element {
        label(self)
    }
}
impl IntoElement for &str {
    fn into_element(self) -> Element {
        label(self)
    }
}
impl IntoElement for &String {
    fn into_element(self) -> Element {
        label(self.clone())
    }
}
impl<T: View> IntoElement for Entity<T> {
    fn into_element(self) -> Element {
        Element::new(ElementKind::Component(Rc::new(crate::ui::ViewEntity(self))))
    }
}
impl Element {
    pub(crate) fn new(kind: ElementKind) -> Self {
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
            text_style: None,
            layout_overrides: 0,
            button_variant: ButtonVariant::Default,
            z_index: 0,
            pointer_events: PointerEvents::Auto,
            inert: false,
            opacity: 1.,
            clip: false,
            scroll: None,
            scroll_handle: None,
            semantics: None,
            input: None,
        }
    }
    fn control_defaults(&mut self, input: bool) {
        let theme = Theme::default();
        let metrics = theme.sizes();
        let [x, y] = if input {
            [metrics.input_padding_x, metrics.input_padding_y]
        } else {
            [metrics.button_padding_x, metrics.button_padding_y]
        };
        self.style.padding = taffy::geometry::Rect {
            left: length(x),
            right: length(x),
            top: length(y),
            bottom: length(y),
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
    /// Includes/excludes this element in focus navigation. Controls default true;
    /// other elements default false. Focus does not create button/text defaults.
    pub fn focusable(mut self, value: bool) -> Self {
        self.input.get_or_insert_with(Default::default).focusable = Some(value);
        self
    }
    /// Includes/excludes this element in Tab traversal without disabling pointer,
    /// programmatic or assistive focus. Focusable controls default to true.
    pub fn tab_stop(mut self, value: bool) -> Self {
        self.input
            .get_or_insert_with(Default::default)
            .focus
            .get_or_insert_with(Default::default)
            .tab_stop = Some(value);
        self
    }
    /// Binds a placement-local focus reference. Containers restore a remembered
    /// eligible descendant; leaf controls receive focus directly.
    pub fn focus_handle(mut self, handle: crate::FocusHandle) -> Self {
        self.input
            .get_or_insert_with(Default::default)
            .focus
            .get_or_insert_with(Default::default)
            .handle = Some(handle);
        self
    }
    /// Remembers focus within a group and optionally cycles keyboard traversal.
    /// Scope membership does not make the container itself focusable.
    pub fn focus_scope(mut self, scope: crate::FocusScope) -> Self {
        self.input
            .get_or_insert_with(Default::default)
            .focus
            .get_or_insert_with(Default::default)
            .scope = Some(scope);
        self
    }
    /// Exposes selected state for semantic tabs/options, without changing control
    /// behavior or appearance. The stock tabs builder sets this automatically.
    pub fn accessibility_selected(mut self, selected: bool) -> Self {
        self.semantics.get_or_insert_with(Default::default).selected = Some(selected);
        self
    }
    /// Native cursor while this element is targeted or owns capture.
    pub fn cursor(mut self, value: crate::Cursor) -> Self {
        self.input.get_or_insert_with(Default::default).cursor = Some(value);
        self
    }
    /// Registers a pointer down listener in the target/bubble route.
    pub fn on_pointer_down(mut self, listener: Listener<crate::PointerInput>) -> Self {
        self.input.get_or_insert_with(Default::default).pointer[0] = Some(listener);
        self
    }
    /// Registers a pointer down listener in the capture route.
    pub fn on_pointer_down_capture(mut self, listener: Listener<crate::PointerInput>) -> Self {
        self.input.get_or_insert_with(Default::default).pointer[4] = Some(listener);
        self
    }
    /// Registers a pointer move listener in the target/bubble route.
    pub fn on_pointer_move(mut self, listener: Listener<crate::PointerInput>) -> Self {
        self.input.get_or_insert_with(Default::default).pointer[1] = Some(listener);
        self
    }
    /// Registers a pointer move listener in the capture route.
    pub fn on_pointer_move_capture(mut self, listener: Listener<crate::PointerInput>) -> Self {
        self.input.get_or_insert_with(Default::default).pointer[5] = Some(listener);
        self
    }
    /// Registers a pointer up listener in the target/bubble route.
    pub fn on_pointer_up(mut self, listener: Listener<crate::PointerInput>) -> Self {
        self.input.get_or_insert_with(Default::default).pointer[2] = Some(listener);
        self
    }
    /// Registers a pointer up listener in the capture route.
    pub fn on_pointer_up_capture(mut self, listener: Listener<crate::PointerInput>) -> Self {
        self.input.get_or_insert_with(Default::default).pointer[6] = Some(listener);
        self
    }
    /// Registers a pointer cancel listener in the target/bubble route.
    pub fn on_pointer_cancel(mut self, listener: Listener<crate::PointerInput>) -> Self {
        self.input.get_or_insert_with(Default::default).pointer[3] = Some(listener);
        self
    }
    /// Registers a pointer cancel listener in the capture route.
    pub fn on_pointer_cancel_capture(mut self, listener: Listener<crate::PointerInput>) -> Self {
        self.input.get_or_insert_with(Default::default).pointer[7] = Some(listener);
        self
    }
    /// Registers a key down listener in the target/bubble route.
    pub fn on_key_down(mut self, listener: Listener<crate::KeyInput>) -> Self {
        self.input.get_or_insert_with(Default::default).key[0] = Some(listener);
        self
    }
    /// Registers a key down listener in the capture route.
    pub fn on_key_down_capture(mut self, listener: Listener<crate::KeyInput>) -> Self {
        self.input.get_or_insert_with(Default::default).key[2] = Some(listener);
        self
    }
    /// Registers a key up listener in the target/bubble route.
    pub fn on_key_up(mut self, listener: Listener<crate::KeyInput>) -> Self {
        self.input.get_or_insert_with(Default::default).key[1] = Some(listener);
        self
    }
    /// Registers a key up listener in the capture route.
    pub fn on_key_up_capture(mut self, listener: Listener<crate::KeyInput>) -> Self {
        self.input.get_or_insert_with(Default::default).key[3] = Some(listener);
        self
    }
    /// Applies opacity once to this element's complete painted subtree, including its
    /// background, border, text and descendants. Values must be finite in 0..=1.
    /// Partial opacity uses an isolated offscreen layer; 1 is the ordinary direct path,
    /// and 0 skips painting. Layout, hit testing and semantics remain unchanged.
    /// Native Application handles composition automatically. Custom hosts call
    /// UiPainter::compose before painting; use inert/PointerEvents to change input.
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }
    /// Paint this complete subtree after lower-z siblings. Equal values preserve description order.
    /// Every parent scopes its children's order; z does not affect layout, Tab or semantics.
    pub fn z_index(mut self, value: i32) -> Self {
        self.z_index = value;
        self
    }
    /// Pointer participation, independently of keyboard/assistive focus or painting.
    pub fn pointer_events(mut self, value: PointerEvents) -> Self {
        self.pointer_events = value;
        self
    }
    /// Disables pointer, keyboard and assistive interaction for this subtree and excludes
    /// it from semantics, while preserving layout, painting, state and retained identity.
    /// Use on background content while an overlay owns interaction. Focus/capture is cleared;
    /// restoring the subtree does not automatically restore focus.
    pub fn inert(mut self, inert: bool) -> Self {
        self.inert = inert;
        self
    }
    /// Fixed dimensions in logical units.
    pub fn size(self, width: f32, height: f32) -> Self {
        self.width(width).height(height)
    }
    /// Width as a fraction of the containing block (1.0 is 100%).
    pub fn width_percent(mut self, fraction: f32) -> Self {
        self.layout_overrides |= 2;
        self.style.size.width = percent(fraction);
        self
    }
    /// Height as a fraction of the containing block (1.0 is 100%).
    pub fn height_percent(mut self, fraction: f32) -> Self {
        self.layout_overrides |= 4;
        self.style.size.height = percent(fraction);
        self
    }
    /// Minimum width in logical units; zero permits flex shrinking below intrinsic content.
    pub fn min_width(mut self, value: f32) -> Self {
        self.style.min_size.width = length(value);
        self
    }
    /// Minimum height in logical units.
    pub fn min_height(mut self, value: f32) -> Self {
        self.style.min_size.height = length(value);
        self
    }
    /// Maximum width in logical units.
    pub fn max_width(mut self, value: f32) -> Self {
        self.style.max_size.width = length(value);
        self
    }
    /// Maximum height in logical units.
    pub fn max_height(mut self, value: f32) -> Self {
        self.style.max_size.height = length(value);
        self
    }
    /// Share of positive free space along a flex parent's main axis. Default is zero.
    pub fn flex_grow(mut self, value: f32) -> Self {
        self.style.flex_grow = value;
        self
    }
    /// Shrink factor along a flex parent's main axis. RXUI defaults to zero.
    pub fn flex_shrink(mut self, value: f32) -> Self {
        self.style.flex_shrink = value;
        self
    }
    /// Initial main-axis size for flex distribution, in logical units.
    pub fn flex_basis(mut self, value: f32) -> Self {
        self.style.flex_basis = length(value);
        self
    }
    /// Override the parent's cross-axis alignment (vertical in a stack).
    pub fn align_self(mut self, value: AlignSelf) -> Self {
        self.style.align_self = Some(value);
        self
    }
    /// Horizontal child alignment for a stack/grid container.
    pub fn justify_items(mut self, value: AlignItems) -> Self {
        self.style.justify_items = Some(value);
        self
    }
    /// Horizontal alignment of this element in a stack/grid parent.
    pub fn justify_self(mut self, value: AlignSelf) -> Self {
        self.style.justify_self = Some(value);
        self
    }
    /// Horizontal padding; vertical padding remains unchanged.
    pub fn padding_x(mut self, value: f32) -> Self {
        self.layout_overrides |= 1;
        self.style.padding.left = length(value);
        self.style.padding.right = length(value);
        self
    }
    /// Vertical padding; horizontal padding remains unchanged.
    pub fn padding_y(mut self, value: f32) -> Self {
        self.layout_overrides |= 1;
        self.style.padding.top = length(value);
        self.style.padding.bottom = length(value);
        self
    }
    /// Uniform external spacing. Negative margins are supported.
    pub fn margin(mut self, value: f32) -> Self {
        self.style.margin = taffy::geometry::Rect {
            left: length(value),
            right: length(value),
            top: length(value),
            bottom: length(value),
        };
        self
    }
    /// Horizontal external spacing.
    pub fn margin_x(mut self, value: f32) -> Self {
        self.style.margin.left = length(value);
        self.style.margin.right = length(value);
        self
    }
    /// Vertical external spacing.
    pub fn margin_y(mut self, value: f32) -> Self {
        self.style.margin.top = length(value);
        self.style.margin.bottom = length(value);
        self
    }
    /// Removes this element from flow. Insets anchor it in its parent's containing block.
    pub fn absolute(mut self) -> Self {
        self.style.position = Position::Absolute;
        self
    }
    /// Restores flow participation. Insets offset its placement while retaining its flow slot.
    pub fn relative(mut self) -> Self {
        self.style.position = Position::Relative;
        self
    }
    /// Left inset/relative offset in logical units.
    pub fn left(mut self, value: f32) -> Self {
        self.style.inset.left = length(value);
        self
    }
    /// Right inset/relative offset in logical units.
    pub fn right(mut self, value: f32) -> Self {
        self.style.inset.right = length(value);
        self
    }
    /// Top inset/relative offset in logical units.
    pub fn top(mut self, value: f32) -> Self {
        self.style.inset.top = length(value);
        self
    }
    /// Bottom inset/relative offset in logical units.
    pub fn bottom(mut self, value: f32) -> Self {
        self.style.inset.bottom = length(value);
        self
    }
    /// Anchors all four edges equally. Auto-sized absolute children stretch between opposite edges.
    pub fn inset(self, value: f32) -> Self {
        self.left(value).right(value).top(value).bottom(value)
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
    /// Inherited preferred font family. Load named families through the painter's fonts.
    pub fn font_family(mut self, family: impl Into<crate::FontFamily>) -> Self {
        self.text_overrides().family = Some(family.into());
        self
    }
    /// Inherited font weight in `1..=1000`.
    pub fn font_weight(mut self, weight: crate::FontWeight) -> Self {
        self.text_overrides().weight = Some(weight);
        self
    }
    /// Inherited font slope.
    pub fn font_style(mut self, style: crate::FontStyle) -> Self {
        self.text_overrides().style = Some(style);
        self
    }
    /// Inherited line height as a multiple of the font size. The theme default is 1.4.
    pub fn line_height(mut self, multiple: f32) -> Self {
        self.text_overrides().line_height = Some(multiple);
        self
    }
    /// Inherited line alignment within a text leaf's content width. Text only moves
    /// when the leaf is wider than its text, for example with `fill_width`.
    pub fn text_align(mut self, align: crate::TextAlign) -> Self {
        self.text_overrides().align = Some(align);
        self
    }
    fn text_overrides(&mut self) -> &mut crate::typography::Overrides {
        self.text_style.get_or_insert_with(Default::default)
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
    /// Top border width, included in layout. Other sides keep their widths.
    /// Pair with [`Self::border_color`] or [`Self::border`] for a painted color.
    pub fn border_top(mut self, width: f32) -> Self {
        self.layout_overrides |= 8;
        self.style.border.top = length(width);
        self
    }
    /// Right border width, included in layout. Other sides keep their widths.
    pub fn border_right(mut self, width: f32) -> Self {
        self.layout_overrides |= 8;
        self.style.border.right = length(width);
        self
    }
    /// Bottom border width, included in layout. Other sides keep their widths.
    pub fn border_bottom(mut self, width: f32) -> Self {
        self.layout_overrides |= 8;
        self.style.border.bottom = length(width);
        self
    }
    /// Left border width, included in layout. Other sides keep their widths.
    pub fn border_left(mut self, width: f32) -> Self {
        self.layout_overrides |= 8;
        self.style.border.left = length(width);
        self
    }
    /// Border color for every side, literal or theme-bound; widths are unchanged.
    pub fn border_color(mut self, color: impl Into<StyleColor>) -> Self {
        self.paint = self.paint.border_color(color);
        self
    }
    /// Uniform painted corner radius. Rounds descendant painting on a clipping element;
    /// hit testing stays rectangular.
    pub fn radius(mut self, radius: f32) -> Self {
        self.paint = self.paint.radius(radius);
        self
    }
    /// Per-corner painted radii, clockwise from the top-left.
    pub fn corner_radii(
        mut self,
        top_left: f32,
        top_right: f32,
        bottom_right: f32,
        bottom_left: f32,
    ) -> Self {
        self.paint = self
            .paint
            .corner_radii(top_left, top_right, bottom_right, bottom_left);
        self
    }
    /// Blurred box shadow behind the background, following the corner radii.
    pub fn shadow(mut self, shadow: crate::BoxShadow) -> Self {
        self.paint = self.paint.shadow(shadow);
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
    /// With corner radii, descendant painting is also rounded; hit testing stays
    /// rectangular.
    pub fn clip(mut self) -> Self {
        self.clip = true;
        self.style.overflow = taffy::geometry::Point {
            x: taffy::Overflow::Hidden,
            y: taffy::Overflow::Hidden,
        };
        self
    }
    /// Publishes this scroll viewport through a placement-scoped reference.
    /// Use one binding per handle per UI; distinct windows may share the handle.
    pub fn scroll_handle(mut self, handle: crate::ScrollHandle) -> Self {
        self.scroll_handle = Some(handle);
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
    /// Chooses default button paint, independent of dimensions or interaction.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        assert!(
            matches!(self.kind, ElementKind::Button { .. }),
            "variant requires a button"
        );
        self.button_variant = variant;
        self
    }
    /// Image aspect mapping into its content box.
    pub fn fit(mut self, fit: crate::ImageFit) -> Self {
        self.image_properties().fit = fit;
        self
    }
    /// Image alignment (0 start, 0.5 center, 1 end) for contain space or cover crop.
    pub fn image_align(mut self, x: f32, y: f32) -> Self {
        self.image_properties().align = [x, y];
        self
    }
    /// Normalized source crop `[u, v, width, height]`, contained in 0..1.
    pub fn source_region(mut self, uv: [f32; 4]) -> Self {
        self.image_properties().uv = uv;
        self
    }
    /// Image tint, literal or theme-bound; independent of inherited text color.
    pub fn tint(mut self, color: impl Into<StyleColor>) -> Self {
        self.image_properties().tint = color.into();
        self
    }
    /// Image sampling; bindings are cached by source and sampling options.
    pub fn filter(mut self, filter: crate::ImageFilter) -> Self {
        self.image_properties().filter = filter;
        self
    }
    /// Explicit source alpha encoding for application textures/custom shaders.
    pub fn image_alpha(mut self, alpha: crate::ImageAlpha) -> Self {
        self.image_properties().alpha = alpha;
        self
    }
    fn image_properties(&mut self) -> &mut crate::image::Properties {
        if let ElementKind::Image(props) = &mut self.kind {
            props
        } else {
            panic!("image property requires an image element")
        }
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
        if let Some(input) = &self.input {
            if let Some(p) = &input.overlay
                && (!p.gap.is_finite()
                    || p.gap < 0.
                    || !p.margin.is_finite()
                    || p.margin < 0.
                    || matches!(&p.anchor, Some(crate::OverlayAnchor::Point(point)) if point.iter().any(|n| !n.is_finite())))
            {
                return Err(UiError::InvalidOverlay);
            }
            for (i, action) in input.commands.iter().enumerate() {
                if input.commands[..i].iter().any(|a| {
                    a.kind == action.kind
                        || a.shortcuts.iter().any(|s| action.shortcuts.contains(s))
                }) || action
                    .shortcuts
                    .iter()
                    .enumerate()
                    .any(|(i, s)| action.shortcuts[..i].contains(s))
                {
                    return Err(UiError::AmbiguousCommand);
                }
            }
        }
        if self
            .input
            .as_ref()
            .and_then(|p| p.dock.as_ref())
            .is_some_and(|p| !p.valid())
        {
            return Err(UiError::InvalidDockConfiguration);
        }
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
        if matches!(&self.kind, ElementKind::Splitter(p) if !p.valid())
            || matches!(&self.kind, ElementKind::Scrollbar(p) if !p.min_thumb.is_finite() || p.min_thumb <= 0.)
        {
            return Err(UiError::InvalidRangeControl);
        }
        if self.scroll_handle.is_some() && self.scroll.is_none() {
            return Err(UiError::InvalidScrollHandle);
        }
        if !self.opacity.is_finite() || !(0. ..=1.).contains(&self.opacity) {
            return Err(UiError::InvalidOpacity);
        }
        if self.font_size.is_some_and(|v| !v.is_finite() || v <= 0.)
            || self.text_style.as_ref().is_some_and(|t| !t.valid())
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
        let valid_track = |track: &taffy::style::TrackSizingFunction| {
            let valid = |v: CompactLength| v.is_intrinsic() || nonnegative(v);
            valid(track.min.into_raw()) && valid(track.max.into_raw())
        };
        let valid_template = |track: &taffy::style::GridTemplateComponent<String>| match track {
            taffy::style::GridTemplateComponent::Single(track) => valid_track(track),
            taffy::style::GridTemplateComponent::Repeat(repeat) => {
                !repeat.tracks.is_empty() && repeat.tracks.iter().all(valid_track)
            }
        };
        if !s
            .grid_template_rows
            .iter()
            .chain(&s.grid_template_columns)
            .all(valid_template)
            || !s
                .grid_auto_rows
                .iter()
                .chain(&s.grid_auto_columns)
                .all(valid_track)
        {
            return Err(UiError::InvalidStyle);
        }
        if let ElementKind::Image(props) = &self.kind {
            props.validate()?;
        }
        if let Some(tabs) = self.input.as_ref().and_then(|p| p.tabs.as_ref()) {
            tabs.validate()?;
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
                    | ElementKind::Scrollbar(_)
                    | ElementKind::Splitter(_)
                    | ElementKind::Button { text: Some(_), .. }
                    | ElementKind::Image(_)
                    | ElementKind::TextInput { .. }
                    | ElementKind::Component(_)
                    | ElementKind::Custom(_)
            )
        {
            return Err(UiError::LeafChildren);
        }
        if self.scroll.is_some()
            && !matches!(
                self.kind,
                ElementKind::Row | ElementKind::Column | ElementKind::Stack
            )
        {
            return Err(UiError::InvalidStyle);
        }
        if matches!(self.kind, ElementKind::Button { .. }) && self.children.iter().any(has_control)
        {
            return Err(UiError::NestedControl);
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
/// Overlapping content sized by its in-flow children. Each child occupies the same grid cell.
/// Absolute children do not contribute intrinsic size. Later/elevated siblings paint above earlier ones.
/// `align_items` controls vertical placement; `justify_items` controls horizontal placement.
///
/// ```
/// use rxui::prelude::*;
/// let panel = stack().size(320., 200.)
///     .child(column().fill_width().fill_height().background(ThemeColor::Surface))
///     .child(button("Close").absolute().top(8.).right(8.).z_index(1));
/// ```
pub fn stack() -> Element {
    let mut element = Element::new(ElementKind::Stack);
    element.style.display = Display::Grid;
    element.style.align_items = Some(AlignItems::START);
    element.style.justify_items = Some(AlignItems::START);
    element
}
/// Owned text leaf measured by the host's text measurer.
pub fn label(text: impl Into<String>) -> Element {
    Element::new(ElementKind::Label(text.into()))
}
/// Button with a caption or composed content, default padding and themed state paint.
/// Plain strings retain the leaf fast path; composed content cannot contain controls.
/// Use `button(row().child(...))` for composition. Label descendants supply its
/// accessible name; name icon-only buttons with `accessibility_label`. Descendant
/// text inherits the button's current state foreground unless explicitly colored.
/// A caption button is a leaf and rejects additional children during preparation.
///
/// ```
/// use rxui::prelude::*;
/// let pixels = Image::from_rgba8(1, 1, vec![255; 4]).unwrap();
/// let action = button(row().gap(8.)
///     .child(image(pixels).width(16.).height(16.).accessibility_hidden(true))
///     .child(label("Save")))
///     .variant(ButtonVariant::Primary);
/// ```
pub fn button(content: impl IntoElement) -> Element {
    let content = content.into_element();
    let plain = matches!(content.kind, ElementKind::Label(_))
        && content.style == Element::new(ElementKind::Label(String::new())).style
        && content.paint == PaintStyle::default()
        && content.font_size.is_none()
        && content.text_style.is_none()
        && content.key.is_none()
        && content.semantics.is_none()
        && content.theme.is_none()
        && content.states.is_none()
        && content.children.is_empty()
        && !content.clip
        && content.scroll.is_none()
        && content.layout_overrides == 0
        && content.z_index == 0
        && content.pointer_events == PointerEvents::Auto
        && !content.inert
        && content.opacity == 1.
        && content.input.is_none()
        && content.scroll_handle.is_none();
    let (text, children) = if plain {
        let ElementKind::Label(text) = content.kind else {
            unreachable!()
        };
        (Some(text), Vec::new())
    } else {
        (None, vec![content])
    };
    let mut element = Element::new(ElementKind::Button {
        text,
        listener: None,
        disabled: false,
    });
    element.children = children;
    element.control_defaults(false);
    element
}

/// Controlled single-line input. Value changes only when the application accepts an
/// on_change proposal. Selection, IME composition and bounded undo/redo history
/// belong to each placement. External value changes reset that local history.
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

/// Shared raster/GPU image leaf. Use explicit sizing for high-DPI assets and framebuffer outputs.
pub fn image(source: crate::Image) -> Element {
    Element::new(ElementKind::Image(Box::new(crate::image::Properties::new(
        source,
    ))))
}

fn has_control(element: &Element) -> bool {
    matches!(
        element.kind,
        ElementKind::Button { .. }
            | ElementKind::TextInput { .. }
            | ElementKind::Scrollbar(_)
            | ElementKind::Splitter(_)
    ) || element.children.iter().any(has_control)
}
