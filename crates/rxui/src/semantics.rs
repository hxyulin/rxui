use crate::{Bounds, ElementId, TextSelection};
use std::sync::Arc;

/// Meaning of an element independent of its visual layout kind.
/// Overriding a role changes metadata; it does not create new interaction behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticRole {
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
    /// Activatable button.
    Button,
    /// Single-line text control.
    TextInput,
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
}
#[derive(Clone, Default)]
pub(crate) struct Properties {
    pub label: Option<Arc<str>>,
    pub description: Option<Arc<str>>,
    pub role: Option<SemanticRole>,
    pub hidden: bool,
    pub selected: Option<bool>,
}
/// Borrowed semantic snapshot for one retained element. It includes offscreen
/// scroll children, but excludes display:none and accessibility-hidden subtrees.
/// Coordinates are logical; native adapters apply the window's scale factor.
#[derive(Debug)]
pub struct SemanticNode<'a> {
    /// Explicit semantic selection state, including tab headers.
    pub selected: Option<bool>,
    /// Live semantic label source, such as the active panel's tab header.
    pub labelled_by: Option<ElementId>,
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
    /// Full logical border bounds, preserving offscreen geometry.
    pub bounds: Bounds,
    /// Logical content bounds, excluding border/padding.
    pub content_bounds: Bounds,
    /// Effective ancestor clipping.
    pub clip_bounds: Bounds,
    /// Whether this element clips descendants.
    pub clips_children: bool,
    /// Static text or the displayed input value, including transient preedit.
    pub value: Option<&'a str>,
    /// Current text revision for rejecting stale text-position requests.
    pub text_revision: u64,
    /// Committed directional selection, omitted while composing.
    pub selection: Option<TextSelection>,
    /// Whether interaction is disabled.
    pub disabled: bool,
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
}
/// Backend-independent assistive action. Unsupported, disabled, hidden, removed,
/// foreign-placement and stale text targets are ignored with Ok(false).
#[derive(Clone, Debug, PartialEq)]
pub enum SemanticAction {
    /// Focus a control and reveal it through scroll ancestors.
    Focus(ElementId),
    /// Use the same button listener as pointer/keyboard activation.
    Activate(ElementId),
    /// Propose a complete controlled input value through its on_change listener.
    SetValue {
        /// Input to update.
        target: ElementId,
        /// Proposed single-line value; user-input normalization rules apply.
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
    /// Propose a finite numeric value to a scrollbar/splitter.
    SetNumericValue {
        /// Control identity.
        target: ElementId,
        /// Desired logical pixel value.
        value: f32,
    },
}
impl SemanticAction {
    pub(crate) fn target(&self) -> ElementId {
        match *self {
            Self::Focus(id) | Self::Activate(id) | Self::ScrollIntoView(id) => id,
            Self::SetValue { target, .. }
            | Self::SetSelection { target, .. }
            | Self::Scroll { target, .. }
            | Self::SetNumericValue { target, .. } => target,
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
    pub text_revision: Option<u64>,
    pub selection: Option<TextSelection>,
    pub composing: bool,
}
