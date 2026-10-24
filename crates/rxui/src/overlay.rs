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
use crate::element::ElementKind;
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
    /// A passive corner overlay, such as a toast: it stacks in its viewport
    /// corner, never takes focus, and leaves outside presses and Escape alone.
    pub corner: Option<crate::ToastPosition>,
}
/// Owned overlay builder. Attach conditionally while open, with a stable key.
/// Surface content retains its logical parent for state/theme/event routing while
/// layout and paint are placed in the window's viewport overlay layer.
#[must_use]
pub struct Overlay {
    surface: Element,
    props: Properties,
    transition: Option<crate::Transition>,
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
                .raised(if modal {
                    Elevation::Modal
                } else {
                    Elevation::Popover
                })
                .clip()
                .scroll_y()
                // The surface only holds focus for its scope; its items show focus.
                .paint_style(PaintStyle::new().focus_width(0.))
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
                corner: None,
            },
            transition: None,
        }
    }
    /// A passive overlay showing `surface` in a viewport corner. Corner overlays
    /// of one position stack in description order, the last nearest the corner.
    pub(crate) fn corner(position: crate::ToastPosition, surface: Element) -> Self {
        Self {
            surface,
            props: Properties {
                anchor: None,
                placement: Default::default(),
                gap: 8.,
                margin: 16.,
                modal: false,
                autofocus: false,
                dismiss: None,
                corner: Some(position),
            },
            transition: None,
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
    /// Enter/exit motion. The default, [`crate::Transition::Theme`], fades in over
    /// `ThemeMotion::overlay_enter` and out over `ThemeMotion::overlay_exit`, sliding
    /// popovers by `ThemeMotion::overlay_offset`; `Transition::None` snaps. A closing
    /// overlay releases focus, input and semantics immediately and only its visual
    /// lingers while it fades out.
    pub fn transition(mut self, transition: crate::Transition) -> Self {
        self.transition = Some(transition);
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
        let mut root = stack().fill_width().fill_height();
        // A corner overlay lets the window behind it receive input.
        if self.props.corner.is_none() {
            root = root.pointer_events(PointerEvents::Block);
        }
        if self.props.modal {
            root = root.background([0., 0., 0., 0.4]);
        }
        root.key = key;
        if let Some(transition) = self.transition {
            root = root.transition(transition);
        }
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
///
/// When any [`menu_item`] child has an icon, every item child reserves the icon
/// column, so captions align. Only direct children take part.
pub fn menu() -> Element {
    let mut element = column().gap(1.).accessibility_role(SemanticRole::Menu);
    element.input.get_or_insert_with(Default::default).menu = true;
    element
}
/// A horizontal [`separator`] between groups of menu items, with 4 units of
/// space above and below.
///
/// ```
/// use rxui::prelude::*;
/// # struct Cut; impl Command for Cut {}
/// # struct Paste; impl Command for Paste {}
/// # fn items(cut: &CommandAction<Cut>, paste: &CommandAction<Paste>) -> rxui::Element {
/// menu().child(menu_item(cut)).child(menu_separator()).child(menu_item(paste))
/// # }
/// ```
pub fn menu_separator() -> crate::Separator {
    separator().horizontal().margin_y(4.)
}
/// Builds an item for a specific typed command, sharing caption/enabled state and
/// callback with command buttons. Close the controlled menu in the command callback.
///
/// The item is a menu row: an optional icon column ([`MenuItem::icon`]), the
/// caption filling the remaining width, and the command's first shortcut as a
/// trailing [`kbd`]. The shortcut is published as the item's keyboard shortcut
/// rather than as part of its name.
///
/// ```
/// use rxui::prelude::*;
/// struct Save;
/// impl Command for Save {}
/// struct Editor { menu: bool }
/// impl View for Editor {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         let save = cx.command(Save, |s, _, _| s.menu = false)
///             .label("Save").shortcut(Shortcut::primary("s"));
///         menu().child(menu_item(&save).icon(Icon::SAVE))
///     }
/// }
/// ```
pub fn menu_item<C: Command>(action: &CommandAction<C>) -> MenuItem {
    MenuItem {
        root: action
            .button()
            .accessibility_role(SemanticRole::MenuItem)
            .list_row(false)
            .fill_width(),
        icon: None,
        shortcut: action.action.shortcuts.first().cloned(),
        enabled: action.is_enabled(),
    }
}
/// One command row of a [`menu`]; see [`menu_item`].
#[must_use = "add the item to a menu"]
pub struct MenuItem {
    root: Element,
    icon: Option<crate::Icon>,
    shortcut: Option<Shortcut>,
    enabled: bool,
}
crate::standard::root_builders!(MenuItem);
impl MenuItem {
    /// Shows `icon` before the caption, in the caption's color.
    pub fn icon(mut self, icon: crate::Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    /// [`ButtonVariant::Danger`] paints the caption and icon in
    /// [`ThemeColor::Danger`], keeping the row's hover fill, for destructive
    /// commands. Other variants paint an ordinary menu row.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.root = self.root.variant(variant);
        self
    }
}
impl IntoElement for MenuItem {
    fn into_element(self) -> Element {
        let mut root = self.root;
        if let ElementKind::Button { text, .. } = &mut root.kind
            && let Some(text) = text.take()
        {
            root.children.push(label(text).flex_grow(1.));
        }
        if let Some(icon) = self.icon {
            root.children.insert(0, crate::icon(icon));
        }
        if let Some(shortcut) = self.shortcut {
            root = root.accessibility_shortcut(shortcut.label());
            let mut cap = kbd(shortcut)
                .accessibility_hidden(true)
                // With the 6-unit gap, 24 units separate caption and shortcut.
                .layout(|s| s.margin.left = taffy::prelude::length(18.));
            if !self.enabled {
                cap = cap.color(ThemeColor::TextDisabled);
            }
            root.children.push(cap);
        }
        root.style.flex_direction = taffy::FlexDirection::Row;
        root.input.get_or_insert_with(Default::default).menu_item = true;
        root.align_items(taffy::AlignItems::CENTER)
            .gap(crate::icons::ICON_GAP)
    }
}
impl Element {
    /// Appends a child of a [`menu`]. The first item with an icon makes the
    /// earlier items reserve the icon column, and later items without one
    /// reserve it as they are added.
    pub(crate) fn push_menu_child(&mut self, mut child: Element) {
        let input = self.input.as_mut().expect("menu input properties");
        match icon_column(&child) {
            Some(true) if !input.menu_icons => {
                input.menu_icons = true;
                for item in &mut self.children {
                    if icon_column(item) == Some(false) {
                        item.children.insert(0, crate::icons::icon_space());
                    }
                }
            }
            Some(false) if input.menu_icons => {
                child.children.insert(0, crate::icons::icon_space());
            }
            _ => {}
        }
        self.children.push(child);
    }
}
/// Whether a [`menu_item`] has an icon column; None for other elements.
fn icon_column(element: &Element) -> Option<bool> {
    element
        .input
        .as_ref()
        .is_some_and(|i| i.menu_item)
        .then(|| {
            element.children.first().is_some_and(|c| {
                c.surface == crate::element::Surface::Part(crate::standard::Part::Icon)
            })
        })
}
