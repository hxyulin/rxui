//! Shaped text.

use std::sync::Arc;

use astrelis_core::color::Color;
use astrelis_ui_next::{Invalidation, Label, NodeHandle, UiError};

use crate::{
    ColorRole, LabelStyle,
    view::{AnyView, Mounted, ViewContext, ViewNode, leaf_mounted_state},
};
/// Creates a text view.
///
/// Text is carried as `Arc<str>` so that reconciling an unchanged label clones a
/// pointer instead of the string.
pub fn label<Action: 'static>(text: impl Into<Arc<str>>) -> AnyView<Action> {
    label_with_style(text, LabelStyle::default())
}

/// Creates a shaped text view with an optional preferred width.
pub fn label_with_width<Action: 'static>(
    text: impl Into<Arc<str>>,
    width: impl Into<Option<f32>>,
) -> AnyView<Action> {
    label_with_style(text, LabelStyle::default().width(width.into()))
}
/// Creates a text view with typed presentation.
pub fn label_with_style<Action: 'static>(
    text: impl Into<Arc<str>>,
    style: LabelStyle,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(LabelView {
            text: text.into(),
            font_size: style.font_size.max(1.0),
            role: style.role,
            width: style.width,
        }),
    }
}
struct LabelView {
    text: Arc<str>,
    font_size: f32,
    role: ColorRole,
    width: Option<f32>,
}

struct LabelState {
    handle: NodeHandle<Label>,
    text: Arc<str>,
    font_size: f32,
    color: Color,
    width: Option<f32>,
}

leaf_mounted_state!(LabelState);

impl<Action: 'static> ViewNode<Action> for LabelView {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let color = context.theme().color(self.role);
        let mut label = Label::new(&*self.text)
            .with_font_size(self.font_size)
            .with_color(color);
        label.set_width(self.width);
        let handle = context.append(label)?;
        Ok(Mounted::new(
            handle.id(),
            LabelState {
                handle,
                text: self.text,
                font_size: self.font_size,
                color,
                width: self.width,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let color = context.theme().color(self.role);
        let state = mounted.state_mut::<LabelState>()?;
        if state.text == self.text
            && state.font_size == self.font_size
            && state.width == self.width
            && state.color == color
        {
            return Ok(());
        }
        // `LAYOUT_ALL` is exact for a label rather than a fallback: the shaper
        // bakes the brush into every glyph run, so even a color-only change has
        // to re-shape, and re-shaping happens in layout. The one thing that is
        // *not* exact is the comparison - `Label::preferred_width` is private,
        // so the retained width cannot be read back and is mirrored here
        // instead.
        //
        // The engine's own per-element shaping memo keeps a redundant re-shape
        // cheap; this cannot become narrower until paint takes a text brush.
        let text = String::from(&*self.text);
        let font_size = self.font_size;
        let width = self.width;
        context
            .ui()
            .update(state.handle, Invalidation::LAYOUT_ALL, |label| {
                label.text = text;
                label.font_size = font_size;
                label.color = Some(color);
                label.set_width(width);
            })?;
        state.text = self.text;
        state.font_size = font_size;
        state.color = color;
        state.width = width;
        Ok(())
    }
}
