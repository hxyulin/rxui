//! Incremental accessibility data.

use astrelis_core::geometry::LogicalRect;

use crate::NodeId;

/// Small semantic role set used by the research prototype.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SemanticRole {
    /// Structural group.
    #[default]
    Group,
    /// Static text.
    Label,
    /// Activatable button.
    Button,
    /// Editable single-line text field.
    TextField,
    /// Boolean checkbox.
    Checkbox,
    /// Numeric slider.
    Slider,
    /// Raster image.
    Image,
    /// Data visualization.
    Chart,
    /// Node-and-edge graph editor.
    Graph,
    /// Hierarchical collection.
    Tree,
    /// Tabular collection.
    Table,
    /// One collection row.
    Row,
    /// Editable property.
    Field,
    /// Application-rendered viewport.
    RenderView,
}

/// Operation supported by an accessible retained element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticActionKind {
    /// Move keyboard focus to the element.
    Focus,
    /// Activate a button or boolean control.
    Activate,
    /// Replace editable text.
    SetText,
    /// Replace editable selection.
    SetSelection,
    /// Set a numeric value.
    SetValue,
}

/// Accessibility operation requested by a platform adapter.
#[derive(Clone, Debug, PartialEq)]
pub enum SemanticAction {
    /// Move keyboard focus to the element.
    Focus,
    /// Activate a button or boolean control.
    Activate,
    /// Replace editable text.
    SetText(String),
    /// Replace editable selection using UTF-8 byte indices.
    SetSelection {
        /// Selection anchor.
        anchor: usize,
        /// Selection focus.
        focus: usize,
    },
    /// Set a numeric value.
    SetValue(f32),
}

/// Element-local accessible properties.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SemanticData {
    /// Semantic role.
    pub role: SemanticRole,
    /// Accessible name.
    pub label: String,
    /// Optional accessible value.
    pub value: Option<String>,
    /// Whether the node is selected.
    pub selected: Option<bool>,
    /// Whether the node is expanded.
    pub expanded: Option<bool>,
}

/// One complete accessible node in window coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticNode {
    /// Stable retained identity.
    pub id: NodeId,
    /// Parent identity.
    pub parent: Option<NodeId>,
    /// Window-space bounds.
    pub bounds: LogicalRect,
    /// Accessible properties.
    pub data: SemanticData,
    /// Whether the element accepts keyboard focus.
    pub focusable: bool,
    /// Whether the element currently has focus.
    pub focused: bool,
    /// Whether interaction is enabled through the complete ancestor path.
    pub enabled: bool,
    /// Operations accepted by the element.
    pub actions: Vec<SemanticActionKind>,
}

/// Accessibility changes since the preceding update.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccessibilityUpdate {
    /// Inserted or changed nodes.
    pub changed: Vec<SemanticNode>,
    /// Removed retained identities.
    pub removed: Vec<NodeId>,
}
