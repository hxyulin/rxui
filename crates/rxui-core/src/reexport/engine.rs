//! Retained-element authoring vocabulary.
//!
//! # Stability
//!
//! **This module is exempt from RXUI's semantic versioning while RXUI is on
//! `0.x`.** It is the surface a custom [`Element`] is written against, and that
//! surface belongs to the Astrelis engine, which is developed alongside RXUI and
//! not yet published. Everything else `rxui` exports is versioned normally.
//!
//! You need this module only to implement [`crate::RetainedSpec`] over an
//! element of your own. Composing existing views never does.
//!
//! The built-in elements are here because a `RetainedSpec` can target one -
//! that is how RXUI's own `label` and `button` are built - not because an
//! application is expected to name them.

pub use astrelis_text::{TextLayout, TextLayoutRequest, TextStyle, TextWrap};
pub use astrelis_ui_next::{
    Align, Alignment, Axis, BoxElement, Button, ButtonIcon, Checkbox, ClipboardOperation,
    Constraints, Element, ElementMut, EventResult, Flex, Frame, FrameUpdate, ImageAlignment,
    ImageElement, ImageFit, Invalidation, KeyListener, KeyedShapingMemo, Label, LayoutContext,
    NodeHandle, NodeId, PassStats, RenderView, RenderViewContent, Scene, Scroll, ScrollAxis,
    ShapingMemo, Slider, SplitPane, Stack, TextField, UiError, UiRoot,
};
