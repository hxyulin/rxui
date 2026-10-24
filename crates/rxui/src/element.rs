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
    /// Bordered control with neutral state fills, medium-weight text and the
    /// theme's [`crate::Elevation::Control`] shadow.
    #[default]
    Default,
    /// Accent fill without a visible border, for the primary action.
    Primary,
    /// Muted text without a resting fill or border. Hover and press brighten the
    /// text and show a row-like fill.
    Quiet,
    /// Destructive action: danger-colored text and border on no fill, with a
    /// tinted fill while hovered or pressed.
    Danger,
}
/// Theme-resolved default paint of a non-control element; see
/// [`Element::raised`] and [`Element::list_row`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Surface {
    #[default]
    Plain,
    Raised(crate::Elevation),
    Row {
        selected: bool,
    },
    /// A standard control part sized and rounded from the theme's metrics.
    Part(crate::standard::Part),
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
/// Axes with retained scroll offsets. Like a CSS scroll container, overflow is clipped
/// to the padding box and the end padding is part of the scrollable extent.
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
    /// Label text with styled spans; see [`rich_text`].
    RichText(Box<crate::typography::RichText>),
    Image(Box<crate::image::Properties>),
    Button {
        text: Option<String>,
        listener: Option<Listener<ClickEvent>>,
        disabled: bool,
    },
    TextInput {
        value: String,
        change: Option<Listener<TextChangeEvent>>,
        disabled: bool,
        read_only: bool,
        /// Submission, placeholder, masking and multiline options; absent for plain
        /// inputs, keeping this variant no larger than the others.
        options: Option<Box<crate::editing::InputOptions>>,
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
    pub(crate) surface: Surface,
    pub(crate) z_index: i32,
    pub(crate) pointer_events: PointerEvents,
    pub(crate) inert: bool,
    pub(crate) opacity: f32,
    pub(crate) clip: bool,
    pub(crate) scroll: Option<ScrollAxes>,
    pub(crate) scroll_behavior: crate::ScrollBehavior,
    pub(crate) scroll_handle: Option<crate::ScrollHandle>,
    pub(crate) text_editing: Option<Box<crate::ui::editing_transfer::Properties>>,
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
                // Absolute children anchor to their parent by default in RXUI.
                position: Position::Relative,
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
            surface: Surface::Plain,
            z_index: 0,
            pointer_events: PointerEvents::Auto,
            inert: false,
            opacity: 1.,
            clip: false,
            scroll: None,
            scroll_behavior: crate::ScrollBehavior::platform(),
            scroll_handle: None,
            text_editing: None,
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
    /// Associates a built-in input with an application-owned document identity.
    /// Changing the document key resets its editing session, even for equal bytes;
    /// version changes alone preserve valid selection/history.
    pub fn text_editing_identity(mut self, identity: crate::TextEditingIdentity) -> Self {
        self.text_editing
            .get_or_insert_with(Default::default)
            .identity = Some(identity);
        self
    }
    /// Binds a weak queued capture handle. Multiple live bindings are ambiguous.
    pub fn text_editing_handle(mut self, handle: crate::TextEditingHandle) -> Self {
        self.text_editing
            .get_or_insert_with(Default::default)
            .handle = Some(handle);
        self
    }
    /// Restores a queued committed snapshot once it becomes ready. Pending captures
    /// survive destination-first preparation; inspect failures through the ticket.
    pub fn restore_text_editing(mut self, capture: crate::TextEditingCapture) -> Self {
        self.text_editing
            .get_or_insert_with(Default::default)
            .restore = Some(capture);
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
    /// Marks a custom title bar or other window-moving area. A primary press on it,
    /// outside any interactive descendant, starts a native window move in the
    /// native host, and a double press toggles maximization. Hosts query
    /// `Ui::window_drag_region_at`; headless placements only record the flag.
    pub fn window_drag_region(mut self) -> Self {
        self.input
            .get_or_insert_with(Default::default)
            .window_drag_region = true;
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
    /// Registers an enter/leave listener. It runs when the pointer's hit target moves
    /// into or out of this element's subtree, and with a leave when the pointer leaves
    /// the window or the element stops taking input. The listener also makes this
    /// element a pointer target, like the pointer listeners.
    pub fn on_hover(mut self, listener: Listener<crate::HoverEvent>) -> Self {
        self.input.get_or_insert_with(Default::default).hover = Some(listener);
        self
    }
    /// Observes focus/blur within this subtree, without making it focusable.
    /// Leaves run innermost first, enters outermost first; moving between two
    /// descendants does not blur their common ancestors. Modality changes also
    /// notify focused subtrees. Notifications are queued after focus commits and
    /// dispatched by input processing or the next preparation, outside model
    /// leases. Focus commands queued by callbacks apply in a later preparation
    /// pass (the normal four-pass stability limit applies). Removal sends blur
    /// to retained listener bindings; a disposed component listener is skipped.
    /// Native deactivation blurs and reactivation focuses the remembered target.
    /// Dropping the entire Ui does not dispatch callbacks.
    pub fn on_focus_change(mut self, listener: Listener<crate::FocusEvent>) -> Self {
        self.input.get_or_insert_with(Default::default).focus_change = Some(listener);
        self
    }
    /// Registers a wheel listener in the target/bubble route. It runs before default
    /// container scrolling, which `WheelInput::prevent_default` suppresses.
    pub fn on_wheel(mut self, listener: Listener<crate::WheelInput>) -> Self {
        self.input.get_or_insert_with(Default::default).wheel = Some(listener);
        self
    }
    /// Routes keys normally but suppresses keyboard defaults while focus is in
    /// this subtree (or this is the root with no focus). This includes shortcuts,
    /// control activation, focus traversal, Escape cancellation and text/IME input.
    /// Use key capture listeners to collect input independently of the focused child.
    pub fn raw_keyboard(mut self, raw: bool) -> Self {
        self.input.get_or_insert_with(Default::default).raw_keyboard = raw;
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
    fn text_client(&mut self) -> &mut crate::input::TextClient {
        self.input
            .get_or_insert_with(Default::default)
            .text
            .get_or_insert_with(Default::default)
    }
    /// Makes this element a text input client: while it is focused, committed text
    /// (typed characters after layout and dead-key processing, IME commits and the
    /// default Paste) reaches [`Self::on_text_insert`] listeners, IME composition
    /// reaches [`Self::on_ime`] listeners, and the native host enables the platform
    /// IME with its candidate window at [`Self::ime_cursor_area`].
    ///
    /// Either value also makes the element focusable unless `.focusable(...)` was
    /// already called, so a view can toggle the flag, for example for a read-only
    /// editor, without losing focus. Key events still route first: a key-down
    /// listener that calls `prevent_default` suppresses the text that key would
    /// insert, and on Tab suppresses focus traversal. Text inputs, text areas and
    /// selectable labels ignore this flag. See `docs/custom-elements.md`.
    pub fn text_input_client(mut self, value: bool) -> Self {
        self.text_client().enabled = value;
        let input = self.input.as_mut().expect("text client properties");
        input.focusable.get_or_insert(true);
        self
    }
    /// Registers a committed-text listener in the target/bubble route from the
    /// focused text input client. It runs once per typed character sequence, IME
    /// commit or paste; see [`crate::TextInsertInput`].
    pub fn on_text_insert(mut self, listener: Listener<crate::TextInsertInput>) -> Self {
        self.text_client().insert = Some(listener);
        self
    }
    /// Registers an IME composition listener in the target/bubble route from the
    /// composing text input client: preedit updates and cancellations, see
    /// [`crate::ImeEvent`].
    pub fn on_ime(mut self, listener: Listener<crate::ImeInput>) -> Self {
        self.text_client().ime = Some(listener);
        self
    }
    /// The text input client's caret rectangle, in logical units relative to this
    /// element's border box, where the platform IME places its candidate window.
    /// Update it from the view as the caret moves. Without it, the content box is
    /// used. Values must be finite with non-negative size.
    pub fn ime_cursor_area(mut self, area: crate::Bounds) -> Self {
        let client = self.text_client();
        client.ime_area = Some(area);
        client.ime_area_provider = None;
        self
    }
    /// Reads a text input client's measured caret rectangle when the host queries
    /// IME geometry, including after custom preparation in the current frame.
    /// This avoids rebuilding a view or requesting another frame solely to publish
    /// a caret measured by [`crate::CustomElement::prepare`]. The callback reads
    /// application-owned geometry; it must not mutate the UI or request a frame.
    ///
    /// Rectangles use logical units relative to this element's border box, like
    /// [`Self::ime_cursor_area`]. `None` uses the content box. Values must be finite
    /// with non-negative size. The last area setter wins. Hosts may query before
    /// custom preparation too, so retain the latest available rectangle.
    pub fn ime_cursor_area_with(
        mut self,
        provider: impl Fn() -> Option<crate::Bounds> + 'static,
    ) -> Self {
        let client = self.text_client();
        client.ime_area = None;
        client.ime_area_provider = Some(std::rc::Rc::new(provider));
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
    /// How this element's resolved paint moves to a new appearance: hover, pressed,
    /// focus, disabled, explicit paint and theme changes interpolate colors, radii,
    /// focus width and shadow instead of snapping. Layout and text never animate.
    /// Buttons, text inputs and elements with state styles default to
    /// [`crate::Transition::Theme`]; other elements animate only theme changes unless
    /// set explicitly. On a popover or modal it also selects the enter/exit motion.
    /// Requires a host that drives `Runtime::begin_frame`; see `docs/animation.md`.
    pub fn transition(mut self, transition: crate::Transition) -> Self {
        self.states.get_or_insert_with(Default::default).transition = Some(transition);
        self
    }
    /// Snaps paint changes immediately, as `.transition(Transition::None)`. Use it
    /// when the view animates this element's paint itself.
    pub fn no_transition(self) -> Self {
        self.transition(crate::Transition::None)
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
        self.style.justify_items = value;
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
    /// Left padding; all other sides remain unchanged.
    pub fn padding_left(mut self, value: f32) -> Self {
        self.layout_overrides |= 1;
        self.style.padding.left = length(value);
        self
    }
    /// Right padding; all other sides remain unchanged.
    pub fn padding_right(mut self, value: f32) -> Self {
        self.layout_overrides |= 1;
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
        self.style.align_items = value;
        self
    }
    /// Main-axis child distribution.
    pub fn justify_content(mut self, value: JustifyContent) -> Self {
        self.style.justify_content = value;
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
    /// Inherited size and weight of a step on the theme's type scale, resolved in
    /// this element's theme and again on theme switches. An explicit
    /// [`Self::font_size`] or [`Self::font_weight`] on the same element wins.
    ///
    /// ```
    /// use rxui::prelude::*;
    /// let header = column()
    ///     .child(label("Settings").text_role(TextRole::Title))
    ///     .child(label("Changes apply immediately").text_role(TextRole::Caption));
    /// ```
    pub fn text_role(mut self, role: crate::TextRole) -> Self {
        self.text_overrides().role = Some(role);
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
    /// Inherited text overflow policy for labels and button captions. Ellipsis
    /// uses a single line at the content width; set `min_width(0.)` on flex items
    /// that must shrink below their intrinsic text width. Inputs and selectable labels
    /// keep their editing behavior.
    pub fn text_overflow(mut self, overflow: crate::TextOverflow) -> Self {
        self.text_overrides().overflow = Some(overflow);
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
    /// Blurred box shadow behind the background, following the corner radii. Accepts
    /// a literal [`crate::BoxShadow`] or a theme [`crate::Elevation`].
    pub fn shadow(mut self, shadow: impl Into<crate::StyleShadow>) -> Self {
        self.paint = self.paint.shadow(shadow);
        self
    }
    /// The nearest theme's shadow for `level`, re-resolved on theme switches. Flat
    /// levels, such as every level of the high-contrast preset, paint no shadow.
    pub fn elevation(mut self, level: crate::Elevation) -> Self {
        self.paint = self.paint.elevation(level);
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
    /// How this scroll container moves for discrete wheel steps and programmatic
    /// commands; [`crate::ScrollBehavior::platform`] by default.
    pub fn scroll_behavior(mut self, behavior: crate::ScrollBehavior) -> Self {
        self.scroll_behavior = behavior;
        self
    }
    /// Retained vertical scrolling. Give the container a bounded height. Content
    /// scrolls under the padding, which stays reachable at both ends.
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
    /// Paints a floating surface at `level`: the theme's raised fill, a 1-unit
    /// divider-colored border, the large corner radius and the level's shadow, all
    /// resolved in the nearest theme and again on theme switches. Explicit paint
    /// builders override each part. Popovers, menus and dialogs use it.
    ///
    /// ```
    /// use rxui::prelude::*;
    /// let card = column().padding(12.).raised(Elevation::Popover).child(label("Details"));
    /// ```
    pub fn raised(mut self, level: crate::Elevation) -> Self {
        if self.layout_overrides & 8 == 0 {
            self = self.layout(|s| {
                s.border = taffy::geometry::Rect {
                    left: length(1.),
                    right: length(1.),
                    top: length(1.),
                    bottom: length(1.),
                }
            });
        }
        self.surface = Surface::Raised(level);
        self
    }
    /// Paints this element as a list, tree or menu row: no box at rest,
    /// [`crate::ThemeColor::RowHover`] while hovered, pressed or showing keyboard
    /// focus, and when `selected`, [`crate::ThemeColor::RowSelected`] while focus is
    /// within the row's list, otherwise [`crate::ThemeColor::RowSelectedInactive`].
    /// The list is the nearest focusable ancestor, or else the nearest scrolling
    /// one. Text keeps its color, the focus ring is drawn inside the row, and a
    /// button row drops its variant's fill and border. Hover needs a pointer
    /// target, such as a button or an element with a pointer listener.
    ///
    /// Without an explicit height or minimum height, the row is at least
    /// [`crate::ThemeMetrics::row_height`] tall; without explicit padding it has
    /// `row_padding_x` on each side and none vertically.
    ///
    /// ```
    /// use rxui::prelude::*;
    /// struct Files { names: Vec<String>, selected: usize }
    /// impl View for Files {
    ///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
    ///         let mut list = column();
    ///         for (index, name) in self.names.iter().enumerate() {
    ///             list = list.child(button(name.clone()).key(index)
    ///                 .list_row(index == self.selected)
    ///                 .on_click(cx.listener(move |this, _, _| this.selected = index)));
    ///         }
    ///         list
    ///     }
    /// }
    /// ```
    pub fn list_row(mut self, selected: bool) -> Self {
        self.surface = Surface::Row { selected };
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
        self.input_options("on_submit").on_submit = Some(listener);
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
    fn input_options(&mut self, method: &str) -> &mut crate::editing::InputOptions {
        if let ElementKind::TextInput { options, .. } = &mut self.kind {
            options.get_or_insert_with(Default::default)
        } else {
            panic!("{method} requires a text input")
        }
    }
    /// Hint shown in the theme's muted text color while the value is empty and no
    /// IME composition is active. Assistive technology receives it as the
    /// placeholder, separately from the accessible name and the empty value.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        let text = text.into();
        self.input_options("placeholder").placeholder = (!text.is_empty()).then_some(text);
        self
    }
    /// Masks the value with one bullet per grapheme. Copy and cut are unavailable,
    /// word movement/deletion and double-click act on the whole value, platform IME
    /// composition is disabled, and assistive technology sees a password field whose
    /// value is the mask. The value itself is still the application's plain text.
    /// Text areas cannot be masked.
    pub fn password(mut self, value: bool) -> Self {
        let options = self.input_options("password");
        assert!(
            !options.multiline,
            "password masking requires a single-line input"
        );
        options.password = value;
        self
    }
    /// Which Enter chord submits a text area; see [`crate::SubmitShortcut`].
    /// Single-line inputs always submit on Enter.
    pub fn submit_shortcut(mut self, shortcut: crate::SubmitShortcut) -> Self {
        self.input_options("submit_shortcut").shortcut = shortcut;
        self
    }
    /// Lets pointer drags, double/triple clicks and Select All select a label's text
    /// and the standard Copy command copy it. A selectable label receives pointer
    /// focus but stays out of Tab traversal unless `.tab_stop(true)` follows this
    /// call. Its selection collapses when focus moves elsewhere, and hovering shows
    /// the text cursor. Labels otherwise keep their alignment and wrapping, show no
    /// caret and accept no edits. Rich text with [links](crate::link) keeps its links
    /// in Tab traversal and shows the pointer cursor over them.
    pub fn selectable(mut self, value: bool) -> Self {
        assert!(
            matches!(self.kind, ElementKind::Label(_) | ElementKind::RichText(_)),
            "selectable requires a label"
        );
        let links = matches!(&self.kind, ElementKind::RichText(rich) if rich.has_links());
        let input = self.input.get_or_insert_with(Default::default);
        input.selectable = value;
        // Links make the element focusable on their own.
        input.focusable = (value || !links).then_some(value);
        input.cursor = value.then_some(crate::Cursor::Text);
        if value && !links {
            self = self.tab_stop(false);
        }
        self
    }
    /// Accessible name, separate from displayed text/value. Buttons default to
    /// their caption; text inputs should supply a meaningful name explicitly.
    pub fn accessibility_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.semantics.get_or_insert_with(Default::default).label = Some(label.into());
        self
    }
    /// Binds a reference for accessible labels or composite highlights. Bind a
    /// handle to at most one element in each UI placement.
    pub fn accessibility_handle(mut self, handle: crate::AccessibilityHandle) -> Self {
        self.semantics.get_or_insert_with(Default::default).handle = Some(handle);
        self
    }
    /// Names this control through another semantic element. Missing, hidden,
    /// removed or ambiguous targets omit the relationship.
    pub fn accessibility_labelled_by(mut self, label: crate::AccessibilityHandle) -> Self {
        self.semantics
            .get_or_insert_with(Default::default)
            .labelled_by = Some(label);
        self
    }
    /// Exposes a composite control's highlighted option while focus stays on the
    /// control. Disabled targets are omitted. A changed target is revealed through
    /// scroll ancestors after layout; unchanged highlights preserve manual scrolling.
    pub fn accessibility_active_descendant(mut self, target: crate::AccessibilityHandle) -> Self {
        self.semantics
            .get_or_insert_with(Default::default)
            .active_descendant = Some(target);
        self
    }
    /// Associates this control with content it operates, such as a combo box's
    /// popup list. Missing, hidden or ambiguous targets omit the relationship.
    pub fn accessibility_controls(mut self, target: crate::AccessibilityHandle) -> Self {
        self.semantics.get_or_insert_with(Default::default).controls = Some(target);
        self
    }
    /// Publishes disabled state for a custom semantic widget. This does not
    /// suppress ordinary input listeners; the widget must enforce its own policy.
    pub fn accessibility_disabled(mut self, disabled: bool) -> Self {
        self.semantics.get_or_insert_with(Default::default).disabled = disabled;
        self
    }
    /// Announces changes to this region's text/value without moving focus. Mount
    /// the region before updating its content. Announcements are atomic (the full
    /// region) and depend on the native adapter and assistive technology.
    pub fn accessibility_live(mut self, priority: crate::LivePoliteness) -> Self {
        self.semantics.get_or_insert_with(Default::default).live = Some(priority);
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
    /// Accessible value text, such as a combo box's current choice or a slider's
    /// formatted value. Labels and text inputs otherwise expose their text.
    pub fn accessibility_value(mut self, value: impl Into<Arc<str>>) -> Self {
        self.semantics.get_or_insert_with(Default::default).value = Some(value.into());
        self
    }
    /// Check state for a check box, radio button or switch role.
    pub fn accessibility_checked(mut self, checked: impl Into<crate::Checked>) -> Self {
        self.semantics.get_or_insert_with(Default::default).checked = Some(checked.into());
        self
    }
    /// Expanded state for a combo box, tree item or other disclosure.
    pub fn accessibility_expanded(mut self, expanded: bool) -> Self {
        self.semantics.get_or_insert_with(Default::default).expanded = Some(expanded);
        self
    }
    /// Hierarchy level starting at 1, such as a tree item's depth or a heading level.
    pub fn accessibility_level(mut self, level: usize) -> Self {
        self.semantics.get_or_insert_with(Default::default).level = Some(level);
        self
    }
    /// Numeric value and range for a slider, progress indicator or similar role.
    /// Values must be finite with `min <= max` and a positive step.
    pub fn accessibility_numeric_value(mut self, value: crate::NumericValue) -> Self {
        self.semantics.get_or_insert_with(Default::default).numeric = Some(value);
        self
    }
    /// Handles assistive activation and numeric changes for an element without
    /// built-in behavior, such as a custom check box or slider. The listener receives
    /// `SemanticAction::Activate` for an assistive click and
    /// `SemanticAction::SetNumericValue` for a new value, including increments by the
    /// numeric step. It replaces the built-in handling of those two actions here.
    /// Mounted collection items also receive `Select`, and disclosures receive
    /// `Expand`/`Collapse` when their selected/expanded properties support them.
    /// Pointer and keyboard input still use the ordinary listeners.
    pub fn on_semantic_action(mut self, listener: Listener<crate::SemanticAction>) -> Self {
        self.semantics.get_or_insert_with(Default::default).action = Some(listener);
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
        if let Some(n) = self.semantics.as_ref().and_then(|p| p.numeric)
            && (![n.value, n.min, n.max].iter().all(|v| v.is_finite())
                || n.min > n.max
                || n.step.is_some_and(|s| !s.is_finite() || s <= 0.))
        {
            return Err(UiError::InvalidStyle);
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
        if let ElementKind::TextInput { value, options, .. } = &self.kind
            && crate::editing::has_control(value, options.as_ref().is_some_and(|o| o.multiline))
        {
            return Err(UiError::InvalidTextValue);
        }
        if let ElementKind::RichText(rich) = &self.kind
            && !rich.spans.iter().all(|(_, style)| style.valid())
        {
            return Err(UiError::InvalidStyle);
        }
        if !self.children.is_empty()
            && matches!(
                self.kind,
                ElementKind::Label(_)
                    | ElementKind::RichText(_)
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
    element.style.align_items = AlignItems::START;
    element.style.justify_items = AlignItems::START;
    element
}
/// Owned text leaf measured by the host's text measurer.
pub fn label(text: impl Into<String>) -> Element {
    Element::new(ElementKind::Label(text.into()))
}
/// A label whose text is the concatenation of styled spans: weights, slopes,
/// families, sizes, colors, underlines and strikethroughs within one wrapped
/// paragraph. Unset span attributes inherit the element's text style and color,
/// and theme-bound span colors follow theme switches. It measures, wraps, aligns,
/// selects (with `.selectable(true)`) and publishes its plain text exactly like
/// [`label`]; assistive technology receives the text without attributes.
///
/// Spans made with [`crate::link`] or [`crate::TextSpan::link`] are links: Tab
/// stops activated by click, Enter or assistive Click, published as Link nodes.
///
/// ```
/// use rxui::prelude::*;
/// let status = rich_text([
///     span("Build "),
///     span("failed").bold().color(ThemeColor::Accent),
///     span(" in "),
///     span("src/main.rs").monospace().underline(),
/// ]);
/// ```
pub fn rich_text(spans: impl IntoIterator<Item = impl Into<crate::TextSpan>>) -> Element {
    Element::new(ElementKind::RichText(Box::new(
        crate::typography::RichText::new(spans.into_iter().map(Into::into)),
    )))
}
/// Button with a caption or composed content, default padding and themed state paint.
/// Plain strings retain the leaf fast path; composed content cannot contain controls.
/// Use `button(row().child(...))` for composition. Label descendants supply its
/// accessible name; name icon-only buttons with `accessibility_label`. Descendant
/// text inherits the button's current state foreground unless explicitly colored.
/// A caption button is a leaf and rejects additional children during preparation.
/// Its caption stays vertically centered when the button is taller than the text,
/// for example with a fixed `height`; composed content follows flex alignment.
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
        && content.surface == Surface::Plain
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
        disabled: false,
        read_only: false,
        options: None,
    });
    element.control_defaults(true);
    element
}

/// Controlled multiline text area with the same proposal, selection, history and
/// IME contract as [`text_input`]. Values may contain `\n` line breaks and tabs but
/// no other control characters; typed or pasted CRLF/CR line breaks become `\n`.
/// Text wraps at the content width and the area grows with its content from a
/// minimum of four lines, so place long documents in a scrolling container: caret
/// movement and edits reveal the caret through scrolling ancestors. An explicit
/// height clips overflowing text without scrolling it.
///
/// Enter inserts a line break and the [`crate::SubmitShortcut`] (Primary+Enter by
/// default) submits. Up/Down keep the horizontal caret position, Home/End (Command+
/// Left/Right on macOS) move within the visual line, and Page Up/Down move by the
/// visible height. Tab still moves focus.
///
/// ```
/// use rxui::prelude::*;
/// struct Notes { text: String }
/// impl View for Notes {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         column().scroll_y().height(240.).child(
///             text_area(self.text.clone()).key("notes").fill_width()
///                 .placeholder("Write something")
///                 .on_change(cx.listener(|this, edit: &TextChangeEvent, _| {
///                     this.text = edit.value.clone();
///                 })))
///     }
/// }
/// ```
pub fn text_area(value: impl Into<String>) -> Element {
    let mut element = text_input(value);
    let theme = Theme::default();
    let metrics = theme.sizes();
    if let ElementKind::TextInput { options, .. } = &mut element.kind {
        options.get_or_insert_with(Default::default).multiline = true;
    }
    element.style.size.height = Dimension::auto();
    // Four lines of the default font plus padding and border; an explicit minimum
    // height overrides it.
    element.style.min_size.height = length(
        4. * metrics.font_size * metrics.line_height
            + 2. * (metrics.input_padding_y + metrics.border_width),
    );
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
