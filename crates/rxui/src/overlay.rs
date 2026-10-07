//! Controlled viewport overlays with logical component ownership.
//!
//! ```
//! use rxui::prelude::*;
//! struct Page { anchor: AnchorHandle, open: bool }
//! impl View for Page {
//!     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
//!         let mut root = column().child(button("Open")
//!             .anchor_handle(self.anchor.clone())
//!             .on_click(cx.listener(|s, _, _| s.open = true)));
//!         if self.open {
//!             root = root.child(popover(self.anchor.clone(), label("Content"))
//!                 .key("popover").width(240.)
//!                 .on_dismiss(cx.listener(|s, _: &DismissEvent, _| s.open = false)));
//!         }
//!         root
//!     }
//! }
//! ```
use crate::*;
use std::{fmt, rc::Rc};

/// Cloneable geometry reference bound with Element::anchor_handle. Bind once per
/// Ui; sharing a handle across windows preserves independent anchor geometry.
#[derive(Clone, Default)]
pub struct AnchorHandle(Rc<()>);
impl fmt::Debug for AnchorHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnchorHandle").finish_non_exhaustive()
    }
}
impl AnchorHandle {
    /// Creates an unbound anchor.
    pub fn new() -> Self {
        Self::default()
    }
    pub(crate) fn id(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
}
/// Source of popover placement, in logical window coordinates.
#[derive(Clone, Debug)]
pub enum OverlayAnchor {
    /// Follow an element, including scrolling and resizing.
    Element(AnchorHandle),
    /// Fixed logical pointer/context-menu position.
    Point([f32; 2]),
}
impl From<AnchorHandle> for OverlayAnchor {
    fn from(h: AnchorHandle) -> Self {
        Self::Element(h)
    }
}
impl From<[f32; 2]> for OverlayAnchor {
    fn from(p: [f32; 2]) -> Self {
        Self::Point(p)
    }
}
/// Preferred popover alignment. Placement flips along its main axis when the
/// opposite side fits better, then clamps to the configured viewport margin.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PopoverPlacement {
    /// Below, aligned to the anchor's left edge.
    #[default]
    BottomStart,
    /// Below, aligned to the anchor's right edge.
    BottomEnd,
    /// Above, aligned to the left edge.
    TopStart,
    /// Above, aligned to the right edge.
    TopEnd,
    /// Right, aligned to the top edge.
    RightStart,
    /// Left, aligned to the top edge.
    LeftStart,
}
/// Why an overlay requests closing. The application owns open state and may reject
/// dismissal. Outside presses are consumed rather than activating content behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DismissReason {
    /// Unprevented Escape.
    Escape,
    /// Pointer press outside the overlay surface.
    OutsidePointer,
    /// Element anchor disappeared, became hidden/inert, or was completely clipped.
    AnchorUnavailable,
}
/// Controlled close proposal; no overlay is removed automatically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DismissEvent {
    /// Cause of this proposal.
    pub reason: DismissReason,
}
#[derive(Clone)]
pub(crate) struct Properties {
    pub anchor: Option<OverlayAnchor>,
    pub placement: PopoverPlacement,
    pub gap: f32,
    pub margin: f32,
    pub modal: bool,
    pub autofocus: bool,
    pub dismiss: Option<Listener<DismissEvent>>,
}
/// Owned overlay builder. Attach conditionally while open, with a stable key.
/// Surface content retains its logical parent for state/theme/event routing while
/// layout and paint are placed in the window's viewport overlay layer.
#[must_use]
pub struct Overlay {
    surface: Element,
    props: Properties,
}
/// Creates a nonmodal anchored popover. Element anchors are window-local; point
/// anchors use logical coordinates. Defaults to BottomStart with 4-unit gap/8-unit margin.
pub fn popover(anchor: impl Into<OverlayAnchor>, content: impl IntoElement) -> Overlay {
    Overlay::new(Some(anchor.into()), content, false)
}
/// Creates a centered modal dialog. Background pointer/keyboard/assistive actions
/// are unavailable while mounted; focus is contained and restored on removal.
pub fn modal(content: impl IntoElement) -> Overlay {
    Overlay::new(None, content, true)
}
impl Overlay {
    fn new(anchor: Option<OverlayAnchor>, content: impl IntoElement, modal: bool) -> Self {
        Self {
            surface: column()
                .padding(8.)
                .background(ThemeColor::Raised)
                .border(1., ThemeColor::Border)
                .radius(6.)
                .clip()
                .scroll_y()
                .focusable(true)
                .tab_stop(false)
                .focus_scope(FocusScope::Cycle)
                .accessibility_role(if modal {
                    SemanticRole::Dialog
                } else {
                    SemanticRole::Group
                })
                .child(content),
            props: Properties {
                anchor,
                placement: Default::default(),
                gap: 4.,
                margin: 8.,
                modal,
                autofocus: true,
                dismiss: None,
            },
        }
    }
    /// Receives controlled dismissal proposals. Without a listener outside presses
    /// and Escape still stay within the overlay but do not remove it.
    pub fn on_dismiss(mut self, listener: Listener<DismissEvent>) -> Self {
        self.props.dismiss = Some(listener);
        self
    }
    /// Preferred alignment relative to an anchor.
    pub fn placement(mut self, placement: PopoverPlacement) -> Self {
        self.props.placement = placement;
        self
    }
    /// Distance from the anchor along the placement axis; finite and nonnegative.
    pub fn gap(mut self, gap: f32) -> Self {
        self.props.gap = gap;
        self
    }
    /// Minimum viewport edge margin; finite and nonnegative.
    pub fn viewport_margin(mut self, margin: f32) -> Self {
        self.props.margin = margin;
        self
    }
    /// Whether opening moves focus into the surface (default true). Modal dialogs
    /// always acquire focus even when this is false.
    pub fn autofocus(mut self, value: bool) -> Self {
        self.props.autofocus = value;
        self
    }
    /// Stable overlay identity in its logical sibling scope.
    pub fn key(mut self, key: impl Into<Key>) -> Self {
        self.surface.key = Some(key.into());
        self
    }
    /// Requested surface width, constrained to the viewport during layout.
    pub fn width(mut self, width: f32) -> Self {
        self.surface = self.surface.width(width);
        self
    }
    /// Requested surface height, constrained to the viewport during layout.
    pub fn height(mut self, height: f32) -> Self {
        self.surface = self.surface.height(height);
        self
    }
    /// Surface padding.
    pub fn padding(mut self, value: f32) -> Self {
        self.surface = self.surface.padding(value);
        self
    }
    /// Accessible surface name, required for meaningful dialogs.
    pub fn accessibility_label(mut self, label: impl Into<String>) -> Self {
        self.surface = self.surface.accessibility_label(label.into());
        self
    }
}
impl IntoElement for Overlay {
    fn into_element(mut self) -> Element {
        let key = self.surface.key.take();
        let mut root = stack()
            .fill_width()
            .fill_height()
            .pointer_events(PointerEvents::Block);
        if self.props.modal {
            root = root.background([0., 0., 0., 0.4]);
        }
        root.key = key;
        root.input.get_or_insert_with(Default::default).overlay = Some(Box::new(self.props));
        root.child(self.surface)
    }
}
impl Element {
    /// Binds geometry used by an anchored popover. Does not make the element
    /// focusable or change its input/paint behavior.
    pub fn anchor_handle(mut self, handle: AnchorHandle) -> Self {
        self.input.get_or_insert_with(Default::default).anchor = Some(handle);
        self
    }
}
/// Builds a menu container. Up/Down/Home/End navigate eligible MenuItem descendants;
/// Enter/Space use ordinary activation. Put it inside a popover for dismissal/focus.
pub fn menu() -> Element {
    let mut element = column().gap(2.).accessibility_role(SemanticRole::Menu);
    element.input.get_or_insert_with(Default::default).menu = true;
    element
}
/// Builds an item for a specific typed command, sharing caption/enabled state and
/// callback with command buttons. Close the controlled menu in the command callback.
pub fn menu_item<C: Command>(action: &CommandAction<C>) -> Element {
    action
        .button()
        .accessibility_role(SemanticRole::MenuItem)
        .variant(ButtonVariant::Quiet)
        .fill_width()
}
