use crate::{Bounds, ElementId, TextSelection};
use std::sync::Arc;

/// Cloneable reference to one semantic element per UI placement. Bind with
/// [`crate::Element::accessibility_handle`]; references resolve only to a unique,
/// visible semantic target in the same placement. Missing or ambiguous bindings
/// publish no relationship. Sharing a handle between windows is safe.
#[derive(Clone, Debug, Default)]
pub struct AccessibilityHandle(std::rc::Rc<()>);
impl AccessibilityHandle {
    /// Creates an unbound reference.
    pub fn new() -> Self {
        Self::default()
    }
    pub(crate) fn identity(&self) -> usize {
        std::rc::Rc::as_ptr(&self.0) as usize
    }
}
/// How assistive technology should announce changes to a live region. Keep the
/// region mounted and change its text/value; delivery depends on the platform
/// adapter and assistive technology. This does not move keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LivePoliteness {
    /// Announce when the user is idle.
    Polite,
    /// Interrupt the current announcement for urgent information.
    Assertive,
}
/// Meaning of an element independent of its visual layout kind.
/// Overriding a role changes metadata; it does not create new interaction behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticRole {
    /// Modal dialog surface.
    Dialog,
    /// Popup menu.
    Menu,
    /// Activatable menu command.
    MenuItem,
    /// Structural container, normally transparent to assistive navigation.
    Container,
    /// Named group of related elements.
    Group,
    /// Collection of form controls.
    Form,
    /// Ordered collection of items.
    List,
    /// One item in a semantic list.
    ListItem,
    /// Section heading; initially published at level one.
    Heading,
    /// Named raster or custom-rendered image.
    Image,
    /// Static text.
    Label,
    /// Activatable button. A toggle button pairs it with `accessibility_checked`
    /// for its pressed state.
    Button,
    /// Single-line text control.
    TextInput,
    /// Multiline text area.
    MultilineTextInput,
    /// Masked password input; its published value is the mask.
    PasswordInput,
    /// Tab group header strip.
    TabList,
    /// Selectable tab header.
    Tab,
    /// Content associated with a tab.
    TabPanel,
    /// Adjustable scroll viewport control.
    Scrollbar,
    /// Adjustable pane separator.
    Splitter,
    /// Static line between groups of content, such as a [`crate::separator`]. It is
    /// neither focusable nor adjustable.
    Separator,
    /// Two- or three-state check box; pair with `accessibility_checked`.
    CheckBox,
    /// One choice in a group; pair with `accessibility_checked`.
    RadioButton,
    /// Group of radio buttons with one choice selected, such as a
    /// [`crate::radio_group`] or [`crate::segmented`] control.
    RadioGroup,
    /// Row or column of controls with one Tab stop, such as a [`crate::toolbar`].
    Toolbar,
    /// On/off toggle; pair with `accessibility_checked`.
    Switch,
    /// Adjustable numeric value; pair with `accessibility_numeric_value`.
    Slider,
    /// Numeric text input with increment and decrement, such as a
    /// [`crate::number_field`]; pair with `accessibility_numeric_value`.
    SpinButton,
    /// Advisory message, such as a neutral or success [`crate::callout`].
    Status,
    /// Important message, such as a warning or danger [`crate::callout`].
    Alert,
    /// Read-only progress; pair with `accessibility_numeric_value`.
    ProgressIndicator,
    /// Collapsed choice with a popup list; pair with `accessibility_expanded` and
    /// `accessibility_value` for the current choice.
    ComboBox,
    /// List of selectable options, such as a combo box popup.
    ListBox,
    /// One option in a list box; pair with `accessibility_selected`.
    ListBoxOption,
    /// Hierarchical list.
    Tree,
    /// One tree node; pair with `accessibility_level` and `accessibility_expanded`.
    TreeItem,
    /// Table of rows and cells.
    Table,
    /// One table row.
    TableRow,
    /// One table cell.
    TableCell,
    /// Header cell labelling a table column.
    ColumnHeader,
}
/// Check state of a check box, radio button, switch or toggle button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checked {
    /// Not checked.
    False,
    /// Checked.
    True,
    /// Partially checked, such as a parent of mixed children.
    Mixed,
}
impl From<bool> for Checked {
    fn from(value: bool) -> Self {
        if value { Self::True } else { Self::False }
    }
}
/// Numeric value of a slider, progress indicator or other adjustable element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumericValue {
    /// Current value.
    pub value: f32,
    /// Smallest value.
    pub min: f32,
    /// Largest value.
    pub max: f32,
    /// Increment and decrement step; assistive technology uses 1 when absent.
    pub step: Option<f32>,
}
#[derive(Clone, Default)]
pub(crate) struct Properties {
    pub handle: Option<AccessibilityHandle>,
    pub labelled_by: Option<AccessibilityHandle>,
    pub active_descendant: Option<AccessibilityHandle>,
    pub controls: Option<AccessibilityHandle>,
    pub live: Option<LivePoliteness>,
    pub label: Option<Arc<str>>,
    pub description: Option<Arc<str>>,
    pub shortcut: Option<Arc<str>>,
    pub value: Option<Arc<str>>,
    pub role: Option<SemanticRole>,
    pub hidden: bool,
    pub disabled: bool,
    pub invalid: bool,
    pub selected: Option<bool>,
    pub multiselectable: bool,
    pub checked: Option<Checked>,
    pub expanded: Option<bool>,
    pub level: Option<usize>,
    pub numeric: Option<NumericValue>,
    pub action: Option<crate::Listener<SemanticAction>>,
    pub set_size: Option<usize>,
    pub position_in_set: Option<usize>,
}
/// Borrowed semantic snapshot for one retained element. It includes offscreen
/// scroll children, but excludes display:none and accessibility-hidden subtrees.
/// Coordinates are logical; native adapters apply the window's scale factor.
#[derive(Debug)]
pub struct SemanticNode<'a> {
    /// Total number of items, including unmounted virtual rows, on a list container.
    pub set_size: Option<usize>,
    /// Zero-based logical index of a mounted list item within the total set.
    pub position_in_set: Option<usize>,
    /// Viewport overlay boundary. Native adapters attach this node to their window
    /// root rather than the logical parent to escape clipping/transforms.
    pub viewport_overlay: bool,
    /// True for a stock modal dialog surface.
    pub modal: bool,
    /// Explicit semantic selection state, including tab headers.
    pub selected: Option<bool>,
    /// Whether this collection accepts multiple selected items.
    pub multiselectable: bool,
    /// Check state of a check box, radio button, switch or toggle button.
    pub checked: Option<Checked>,
    /// Whether a combo box, tree item or other disclosure is expanded.
    pub expanded: Option<bool>,
    /// Hierarchy depth starting at 1, such as a tree item level or heading level.
    pub level: Option<usize>,
    /// Application-supplied numeric value, such as a slider's.
    pub numeric_value: Option<NumericValue>,
    /// Whether an `on_semantic_action` listener accepts numeric changes.
    pub adjustable: bool,
    /// Live semantic label source, such as the active panel's tab header.
    pub labelled_by: Option<ElementId>,
    /// Highlighted enabled semantic target of a composite control.
    pub active_descendant: Option<ElementId>,
    /// Announcement priority for changes to this region.
    pub live: Option<LivePoliteness>,
    /// Live associated content, such as the selected tab's panel.
    pub controls: Option<ElementId>,
    /// Composite navigation orientation.
    pub orientation: Option<crate::Axis>,
    /// Placement-scoped identity, stable across compatible keyed reconciliation.
    pub id: ElementId,
    /// Structural parent, absent for the UI root.
    pub parent: Option<ElementId>,
    /// Structural children; filter through the same snapshot when publishing a tree.
    pub children: &'a [ElementId],
    /// Semantic role, inferred from control kind unless explicitly overridden.
    pub role: SemanticRole,
    /// Accessible name. Text inputs require an application-provided name.
    pub label: Option<&'a str>,
    /// Additional help/description, separate from the name and value.
    pub description: Option<&'a str>,
    /// Keyboard shortcut that performs this element's action, in a human-friendly
    /// form such as [`crate::Shortcut::label`]; separate from the name.
    pub keyboard_shortcut: Option<&'a str>,
    /// Input placeholder hint, separate from the name and the (possibly empty) value.
    pub placeholder: Option<&'a str>,
    /// Full logical border bounds, preserving offscreen geometry.
    pub bounds: Bounds,
    /// Logical content bounds, excluding border/padding.
    pub content_bounds: Bounds,
    /// Effective ancestor clipping.
    pub clip_bounds: Bounds,
    /// Whether this element clips descendants.
    pub clips_children: bool,
    /// Static text or the displayed input value, including transient preedit.
    /// Password inputs expose one mask character per grapheme.
    pub value: Option<&'a str>,
    /// Current text revision for rejecting stale text-position requests.
    pub text_revision: u64,
    /// Committed directional selection, omitted while composing. Offsets index
    /// `value`, so a password input's selection addresses its mask.
    pub selection: Option<TextSelection>,
    /// Whether interaction is disabled.
    pub disabled: bool,
    /// Whether the control's value is invalid, such as a [`crate::field`] with an error.
    pub invalid: bool,
    /// Whether this input accepts no committed changes.
    pub read_only: bool,
    /// Whether Focus is supported by the underlying control.
    pub focusable: bool,
    /// Whether activation is supported by the underlying control.
    pub activatable: bool,
    /// Whether controlled SetValue is supported.
    pub editable: bool,
    /// Retained content offset on container axes.
    pub scroll_offset: [f32; 2],
    /// Maximum reachable offsets; positive axes support scrolling actions.
    pub scroll_range: [f32; 2],
    /// Input-local horizontal text offset.
    pub text_scroll_x: f32,
    /// Current adjustable control value/range/orientation.
    pub range: Option<crate::RangeInfo>,
    /// Number of Link children of rich text; see [`crate::Ui::semantic_links`].
    pub links: usize,
}
/// One link span of a rich text element, published to assistive technology as a
/// Link child of the element's Label node. Links are not elements: address them
/// with the element and index through [`SemanticAction::FocusLink`] and
/// [`SemanticAction::ActivateLink`].
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticLink<'a> {
    /// Rich text element containing the link.
    pub element: ElementId,
    /// Link index, as in [`crate::LinkEvent::index`].
    pub index: usize,
    /// Accessible name: the link's text.
    pub name: &'a str,
    /// Byte range of the link in the element's text.
    pub range: std::ops::Range<usize>,
    /// Application-supplied destination, see [`crate::TextSpan::link_url`].
    pub url: Option<&'a str>,
    /// Application-supplied help, see [`crate::TextSpan::link_description`].
    pub description: Option<&'a str>,
    /// Logical window bounds covering the link's text, or the element's content
    /// box when the measurer supplies no text geometry.
    pub bounds: Bounds,
    /// Whether this link holds focus in the semantic tree.
    pub focused: bool,
    /// Whether the element accepts focus and activation, for example outside an
    /// inert subtree.
    pub available: bool,
}
/// Backend-independent assistive action. Unsupported, disabled, hidden, removed,
/// foreign-placement and stale text targets are ignored with Ok(false).
#[derive(Clone, Debug, PartialEq)]
pub enum SemanticAction {
    /// Focus a control and reveal it through scroll ancestors.
    Focus(ElementId),
    /// Use the same button listener as pointer/keyboard activation, or the element's
    /// `on_semantic_action` listener.
    Activate(ElementId),
    /// Expand a mounted disclosure through its semantic listener.
    Expand(ElementId),
    /// Collapse a mounted disclosure through its semantic listener.
    Collapse(ElementId),
    /// Select a mounted collection item (distinct from text SetSelection).
    Select(ElementId),
    /// Propose a complete controlled input value through its on_change listener.
    SetValue {
        /// Input to update.
        target: ElementId,
        /// Proposed value; the input's user-input normalization rules apply.
        value: String,
    },
    /// Set the directional selection in the published committed text snapshot.
    SetSelection {
        /// Input to select.
        target: ElementId,
        /// Expected revision from SemanticNode, rejecting out-of-date offsets.
        text_revision: u64,
        /// Byte/grapheme endpoints in that snapshot.
        selection: TextSelection,
    },
    /// Set container offsets, clamped to currently reachable ranges.
    Scroll {
        /// Scroll container.
        target: ElementId,
        /// Absolute logical content offsets.
        offset: [f32; 2],
    },
    /// Reveal a retained element without requiring it already be inside the viewport.
    ScrollIntoView(ElementId),
    /// Propose a finite numeric value to a scrollbar/splitter or an element with an
    /// `on_semantic_action` listener.
    SetNumericValue {
        /// Control identity.
        target: ElementId,
        /// Desired logical pixel value.
        value: f32,
    },
    /// Focus one link of a rich text element and reveal it.
    FocusLink {
        /// Rich text element.
        target: ElementId,
        /// Link index within the element.
        link: usize,
    },
    /// Dispatch one link's listener, as a click or Enter would.
    ActivateLink {
        /// Rich text element.
        target: ElementId,
        /// Link index within the element.
        link: usize,
    },
}
impl SemanticAction {
    pub(crate) fn target(&self) -> ElementId {
        match *self {
            Self::Focus(id)
            | Self::Activate(id)
            | Self::Expand(id)
            | Self::Collapse(id)
            | Self::Select(id)
            | Self::ScrollIntoView(id) => id,
            Self::SetValue { target, .. }
            | Self::SetSelection { target, .. }
            | Self::Scroll { target, .. }
            | Self::SetNumericValue { target, .. }
            | Self::FocusLink { target, .. }
            | Self::ActivateLink { target, .. } => target,
        }
    }
}
#[cfg(feature = "accessibility")]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Key {
    pub tree: u64,
    pub viewport: Option<[f32; 2]>,
    pub evaluations: u64,
    pub layouts: u64,
    pub geometry: u64,
    pub focus: Option<ElementId>,
    pub link_focus: Option<usize>,
    pub text_revision: Option<u64>,
    pub selection: Option<TextSelection>,
    pub composing: bool,
}
