//! Property-aware retained mutation used by framework reconciliation.
//!
//! Every setter here covers one property, compares against the value the
//! element already holds, and declares the narrowest invalidation that property
//! can cause. The comparison has to happen on this side of the boundary: after
//! an in-place mutation only the retained element knows what the value used to
//! be, so a consumer that cannot read a property back cannot avoid a redundant
//! write. Properties that had no reader gained one.
//!
//! A setter that changes what an element measures declares
//! [`Invalidation::LAYOUT`] alone. [`UiTree`] expands that to the passes layout
//! feeds, so [`UiTree::invalidation`] reads [`Invalidation::LAYOUT_ALL`] after
//! one of them: the declaration is minimal, the expansion is the engine's.
//!
//! Text colour is one of those layout properties rather than a paint property.
//! The shaper bakes the brush into every glyph run, so a recoloured string has
//! to be shaped again, and shaping happens during layout. Making a recolour
//! paint-only needs a paint-side text brush, which does not exist yet.

use std::{any::Any, ops::RangeInclusive};

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalSize},
};
use astrelis_paint::{Image, ImageSampling};

use crate::{
    ActionBox, Align, Alignment, Axis, BoxElement, Button, ButtonIcon, Checkbox, Element, Flex,
    Frame, ImageAlignment, ImageElement, ImageFit, Invalidation, KeyListener, Label, NodeHandle,
    RenderView, RenderViewContent, Scroll, ScrollAxis, SemanticData, Slider, SplitPane, Stack,
    TextField, UiInput, UiTree,
};

const CONTROLLED: Invalidation =
    Invalidation::from_bits_retain(Invalidation::PAINT.bits() | Invalidation::ACCESSIBILITY.bits());

/// Work a change in composed position causes: everything layout would have
/// triggered, minus layout itself.
const MOVED: Invalidation = Invalidation::from_bits_retain(
    Invalidation::COMPOSE.bits()
        | Invalidation::PAINT.bits()
        | Invalidation::ACCESSIBILITY.bits()
        | Invalidation::HIT_TEST.bits(),
);

/// Typed property-aware access to one retained element.
pub struct NodeMut<'a, E: Element> {
    ui: &'a mut UiTree,
    handle: NodeHandle<E>,
}

impl UiTree {
    /// Begins property-aware mutation of a typed retained element.
    pub fn edit<E: Element>(&mut self, handle: NodeHandle<E>) -> NodeMut<'_, E> {
        NodeMut { ui: self, handle }
    }

    /// Begins property-aware mutation of a retained label.
    pub fn label_mut(&mut self, handle: NodeHandle<Label>) -> NodeMut<'_, Label> {
        self.edit(handle)
    }

    /// Begins property-aware mutation of a retained box.
    pub fn box_mut(&mut self, handle: NodeHandle<BoxElement>) -> NodeMut<'_, BoxElement> {
        self.edit(handle)
    }

    /// Begins property-aware mutation of a retained flex container.
    pub fn flex_mut(&mut self, handle: NodeHandle<Flex>) -> NodeMut<'_, Flex> {
        self.edit(handle)
    }

    /// Begins property-aware mutation of a retained frame.
    pub fn frame_mut(&mut self, handle: NodeHandle<Frame>) -> NodeMut<'_, Frame> {
        self.edit(handle)
    }

    /// Begins property-aware mutation of a retained stack.
    pub fn stack_mut(&mut self, handle: NodeHandle<Stack>) -> NodeMut<'_, Stack> {
        self.edit(handle)
    }

    /// Begins property-aware mutation of a retained scroll container.
    pub fn scroll_mut(&mut self, handle: NodeHandle<Scroll>) -> NodeMut<'_, Scroll> {
        self.edit(handle)
    }

    /// Begins property-aware mutation of an alignment container.
    pub fn align_mut(&mut self, handle: NodeHandle<Align>) -> NodeMut<'_, Align> {
        self.edit(handle)
    }

    /// Begins property-aware mutation of a retained image.
    pub fn image_mut(&mut self, handle: NodeHandle<ImageElement>) -> NodeMut<'_, ImageElement> {
        self.edit(handle)
    }

    /// Begins property-aware mutation of an application render viewport.
    pub fn render_view_mut(&mut self, handle: NodeHandle<RenderView>) -> NodeMut<'_, RenderView> {
        self.edit(handle)
    }

    /// Begins property-aware mutation of a retained button.
    pub fn button_mut(&mut self, handle: NodeHandle<Button>) -> NodeMut<'_, Button> {
        self.edit(handle)
    }
    /// Begins property-aware mutation of a retained checkbox.
    pub fn checkbox_mut(&mut self, handle: NodeHandle<Checkbox>) -> NodeMut<'_, Checkbox> {
        self.edit(handle)
    }
    /// Begins property-aware mutation of a retained slider.
    pub fn slider_mut(&mut self, handle: NodeHandle<Slider>) -> NodeMut<'_, Slider> {
        self.edit(handle)
    }
    /// Begins property-aware mutation of a retained text field.
    pub fn text_field_mut(&mut self, handle: NodeHandle<TextField>) -> NodeMut<'_, TextField> {
        self.edit(handle)
    }
    /// Begins property-aware mutation of a retained split pane.
    pub fn split_pane_mut(&mut self, handle: NodeHandle<SplitPane>) -> NodeMut<'_, SplitPane> {
        self.edit(handle)
    }
    /// Begins property-aware mutation of a retained key listener.
    pub fn key_listener_mut(
        &mut self,
        handle: NodeHandle<KeyListener>,
    ) -> NodeMut<'_, KeyListener> {
        self.edit(handle)
    }
}

fn same_icon(left: Option<&ButtonIcon>, right: Option<&ButtonIcon>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.size == right.size
                && left.view_box == right.view_box
                && left.fill_rule == right.fill_rule
                && left.path.verbs() == right.path.verbs()
        }
        _ => false,
    }
}

fn ordered(range: RangeInclusive<f32>) -> RangeInclusive<f32> {
    let start = (*range.start()).min(*range.end());
    let end = (*range.start()).max(*range.end());
    start..=end
}

impl<E: Element> NodeMut<'_, E> {
    /// Writes `value` and requests `invalidation` only when it differs from what
    /// the element holds, reporting whether anything changed.
    fn guarded<T: PartialEq>(
        &mut self,
        value: T,
        invalidation: Invalidation,
        read: impl FnOnce(&E) -> &T,
        write: impl FnOnce(&mut E, T),
    ) -> bool {
        if read(self.ui.element(self.handle)) == &value {
            return false;
        }
        self.ui
            .update(self.handle, invalidation, |element| write(element, value));
        true
    }

    /// Reports that interior-mutable transform or clipping state changed.
    ///
    /// This is the typed escape hatch for custom elements whose composed state
    /// lives behind `Cell`-like storage. It deliberately fixes the invalidation
    /// to composition and its downstream consumers; it does not expose the raw
    /// arbitrary-flag update API.
    pub fn mark_composition_changed(&mut self) {
        self.ui.update(self.handle, MOVED, |_| {});
    }
}

impl NodeMut<'_, Button> {
    /// Replaces the shaped and accessible label. `LAYOUT`.
    pub fn set_label(&mut self, label: impl Into<String>) -> bool {
        self.guarded(
            label.into(),
            Invalidation::LAYOUT,
            |button| &button.label,
            |button, label| button.label = label,
        )
    }
    /// Replaces the preferred size. `LAYOUT`.
    pub fn set_size(&mut self, size: LogicalSize) -> bool {
        self.guarded(
            size,
            Invalidation::LAYOUT,
            |button| &button.size,
            |button, size| button.size = size,
        )
    }
    /// Replaces the resting background. `PAINT`.
    pub fn set_color(&mut self, color: Color) -> bool {
        self.guarded(
            color,
            Invalidation::PAINT,
            |button| &button.color,
            |button, color| button.color = color,
        )
    }
    /// Replaces the pressed background. `PAINT`.
    pub fn set_pressed_color(&mut self, color: Color) -> bool {
        self.guarded(
            color,
            Invalidation::PAINT,
            |button| &button.pressed_color,
            |button, color| button.pressed_color = color,
        )
    }
    /// Replaces the glyph and icon colour. `LAYOUT`.
    pub fn set_text_color(&mut self, color: Color) -> bool {
        self.guarded(
            color,
            Invalidation::LAYOUT,
            |button| &button.text_color,
            |button, color| button.text_color = color,
        )
    }
    /// Replaces the glyph size. `LAYOUT`.
    pub fn set_font_size(&mut self, font_size: f32) -> bool {
        self.guarded(
            font_size,
            Invalidation::LAYOUT,
            |button| &button.font_size,
            |button, font_size| button.font_size = font_size,
        )
    }
    /// Selects whether the accessible label is also painted. `LAYOUT`.
    pub fn set_label_visible(&mut self, visible: bool) -> bool {
        self.guarded(
            visible,
            Invalidation::LAYOUT,
            |button| &button.show_label,
            |button, visible| button.set_label_visible(visible),
        )
    }
    /// Replaces the optional leading vector content. `LAYOUT`.
    pub fn set_icon(&mut self, icon: Option<ButtonIcon>) -> bool {
        if same_icon(self.ui.element(self.handle).icon.as_ref(), icon.as_ref()) {
            return false;
        }
        self.ui.update(self.handle, Invalidation::LAYOUT, |button| {
            button.set_icon(icon)
        });
        true
    }
    /// Replaces the erased activation factory. No invalidation.
    pub fn set_action(&mut self, action: impl Fn() -> Box<dyn Any> + 'static) {
        self.ui
            .update(self.handle, Invalidation::empty(), |button| {
                button.set_action_factory(action)
            });
    }
    /// Replaces every resolved button property at once.
    pub fn set_button(
        &mut self,
        label: String,
        size: LogicalSize,
        color: Color,
        pressed_color: Color,
    ) {
        self.set_label(label);
        self.set_size(size);
        self.set_color(color);
        self.set_pressed_color(pressed_color);
    }
}

impl NodeMut<'_, TextField> {
    /// Replaces the accessible name and placeholder. `LAYOUT`.
    pub fn set_label(&mut self, label: impl Into<String>) -> bool {
        self.guarded(
            label.into(),
            Invalidation::LAYOUT,
            |field| &field.label,
            |field, label| field.label = label,
        )
    }
    /// Replaces the controlled value. `LAYOUT`.
    pub fn set_text(&mut self, text: impl Into<String>) -> bool {
        self.guarded(
            text.into(),
            Invalidation::LAYOUT,
            |field| &field.text,
            |field, text| field.set_text(text),
        )
    }
    /// Replaces the preferred control width. `LAYOUT`.
    pub fn set_width(&mut self, width: f32) -> bool {
        self.guarded(
            width,
            Invalidation::LAYOUT,
            |field| &field.width,
            |field, width| field.width = width,
        )
    }
    /// Replaces the glyph size. `LAYOUT`.
    pub fn set_font_size(&mut self, font_size: f32) -> bool {
        self.guarded(
            font_size,
            Invalidation::LAYOUT,
            |field| &field.font_size,
            |field, font_size| field.font_size = font_size,
        )
    }
    /// Replaces the glyph colour. `LAYOUT`.
    pub fn set_text_color(&mut self, color: Color) -> bool {
        self.guarded(
            color,
            Invalidation::LAYOUT,
            |field| &field.text_color,
            |field, color| field.text_color = color,
        )
    }
    /// Replaces the background fill. `PAINT`.
    pub fn set_background(&mut self, color: Color) -> bool {
        self.guarded(
            color,
            Invalidation::PAINT,
            |field| &field.background,
            |field, color| field.background = color,
        )
    }
    /// Replaces the selection highlight. `PAINT`.
    pub fn set_selection_color(&mut self, color: Color) -> bool {
        self.guarded(
            color,
            Invalidation::PAINT,
            |field| &field.selection_color,
            |field, color| field.selection_color = color,
        )
    }
    /// Replaces the caret colour. `PAINT`.
    pub fn set_caret_color(&mut self, color: Color) -> bool {
        self.guarded(
            color,
            Invalidation::PAINT,
            |field| &field.caret_color,
            |field, color| field.caret_color = color,
        )
    }
    /// Replaces the erased change-action factory. No invalidation.
    pub fn set_change_action(&mut self, changed: impl Fn(String) -> Box<dyn Any> + 'static) {
        self.ui.update(self.handle, Invalidation::empty(), |field| {
            field.set_changed_factory(changed)
        });
    }
    /// Replaces the erased submit-action factory. No invalidation.
    pub fn set_submit_action(&mut self, submitted: impl Fn(String) -> Box<dyn Any> + 'static) {
        self.ui.update(self.handle, Invalidation::empty(), |field| {
            field.set_submitted_factory(submitted)
        });
    }
    /// Replaces every resolved field property at once.
    pub fn set_field(
        &mut self,
        label: String,
        value: String,
        text_color: Color,
        background: Color,
        changed: impl Fn(String) -> Box<dyn Any> + 'static,
    ) {
        self.set_label(label);
        self.set_text(value);
        self.set_text_color(text_color);
        self.set_background(background);
        self.set_change_action(changed);
    }
}

impl NodeMut<'_, Checkbox> {
    /// Replaces the shaped and accessible label. `LAYOUT`.
    pub fn set_label(&mut self, label: impl Into<String>) -> bool {
        self.guarded(
            label.into(),
            Invalidation::LAYOUT,
            |checkbox| &checkbox.label,
            |checkbox, label| checkbox.label = label,
        )
    }
    /// Replaces the controlled checked state. `PAINT | ACCESSIBILITY`.
    pub fn set_checked(&mut self, checked: bool) -> bool {
        self.guarded(
            checked,
            CONTROLLED,
            |checkbox| &checkbox.checked,
            |checkbox, checked| checkbox.checked = checked,
        )
    }
    /// Replaces the preferred size. `LAYOUT`.
    pub fn set_size(&mut self, size: LogicalSize) -> bool {
        self.guarded(
            size,
            Invalidation::LAYOUT,
            |checkbox| &checkbox.size,
            |checkbox, size| checkbox.size = size,
        )
    }
    /// Replaces the label colour. `LAYOUT`.
    pub fn set_text_color(&mut self, color: Color) -> bool {
        self.guarded(
            color,
            Invalidation::LAYOUT,
            |checkbox| &checkbox.text_color,
            |checkbox, color| checkbox.text_color = color,
        )
    }
    /// Replaces the unchecked outline colour. `PAINT`.
    pub fn set_outline_color(&mut self, color: Color) -> bool {
        self.guarded(
            color,
            Invalidation::PAINT,
            |checkbox| &checkbox.outline_color,
            |checkbox, color| checkbox.outline_color = color,
        )
    }
    /// Replaces the checked fill colour. `PAINT`.
    pub fn set_accent_color(&mut self, color: Color) -> bool {
        self.guarded(
            color,
            Invalidation::PAINT,
            |checkbox| &checkbox.accent_color,
            |checkbox, color| checkbox.accent_color = color,
        )
    }
    /// Replaces the erased change-action factory. No invalidation.
    pub fn set_change_action(&mut self, changed: impl Fn(bool) -> Box<dyn Any> + 'static) {
        self.ui
            .update(self.handle, Invalidation::empty(), |checkbox| {
                checkbox.set_changed(changed)
            });
    }
    /// Replaces every resolved checkbox property at once.
    pub fn set_checkbox(
        &mut self,
        label: String,
        checked: bool,
        text_color: Color,
        outline_color: Color,
        accent_color: Color,
    ) {
        self.set_label(label);
        self.set_checked(checked);
        self.set_text_color(text_color);
        self.set_outline_color(outline_color);
        self.set_accent_color(accent_color);
    }
}

impl NodeMut<'_, Slider> {
    /// Replaces the accessible label. `ACCESSIBILITY`.
    pub fn set_label(&mut self, label: impl Into<String>) -> bool {
        self.guarded(
            label.into(),
            Invalidation::ACCESSIBILITY,
            |slider| &slider.label,
            |slider, label| slider.label = label,
        )
    }
    /// Replaces the controlled value. `PAINT | ACCESSIBILITY`.
    pub fn set_value(&mut self, value: f32) -> bool {
        let slider = self.ui.element(self.handle);
        let value = value.clamp(*slider.range.start(), *slider.range.end());
        self.guarded(
            value,
            CONTROLLED,
            |slider| &slider.value,
            |slider, value| slider.value = value,
        )
    }
    /// Replaces the accepted range. `PAINT | ACCESSIBILITY`.
    pub fn set_range(&mut self, range: RangeInclusive<f32>) -> bool {
        let range = ordered(range);
        let slider = self.ui.element(self.handle);
        let value = slider.value.clamp(*range.start(), *range.end());
        if slider.range == range && slider.value == value {
            return false;
        }
        self.ui.update(self.handle, CONTROLLED, |slider| {
            slider.range = range;
            slider.value = value;
        });
        true
    }
    /// Replaces the keyboard adjustment step. No invalidation.
    pub fn set_step(&mut self, step: f32) -> bool {
        self.guarded(
            step.max(0.0),
            Invalidation::empty(),
            |slider| &slider.step,
            |slider, step| slider.step = step,
        )
    }
    /// Replaces the preferred size. `LAYOUT`.
    pub fn set_size(&mut self, size: LogicalSize) -> bool {
        self.guarded(
            size,
            Invalidation::LAYOUT,
            |slider| &slider.size,
            |slider, size| slider.size = size,
        )
    }
    /// Replaces the unfilled track colour. `PAINT`.
    pub fn set_track_color(&mut self, color: Color) -> bool {
        self.guarded(
            color,
            Invalidation::PAINT,
            |slider| &slider.track_color,
            |slider, color| slider.track_color = color,
        )
    }
    /// Replaces the filled track and thumb colour. `PAINT`.
    pub fn set_accent_color(&mut self, color: Color) -> bool {
        self.guarded(
            color,
            Invalidation::PAINT,
            |slider| &slider.accent_color,
            |slider, color| slider.accent_color = color,
        )
    }
    /// Replaces the erased change-action factory. No invalidation.
    pub fn set_change_action(&mut self, changed: impl Fn(f32) -> Box<dyn Any> + 'static) {
        self.ui
            .update(self.handle, Invalidation::empty(), |slider| {
                slider.set_changed(changed)
            });
    }
    /// Replaces every resolved slider property at once.
    pub fn set_slider(
        &mut self,
        label: String,
        value: f32,
        range: RangeInclusive<f32>,
        step: f32,
        track_color: Color,
        accent_color: Color,
    ) {
        self.set_label(label);
        self.set_range(range);
        self.set_value(value);
        self.set_step(step);
        self.set_track_color(track_color);
        self.set_accent_color(accent_color);
    }
}

impl NodeMut<'_, SplitPane> {
    /// Replaces the split direction. `LAYOUT`.
    pub fn set_axis(&mut self, axis: Axis) -> bool {
        self.guarded(
            axis,
            Invalidation::LAYOUT,
            |pane| &pane.axis,
            |pane, axis| pane.axis = axis,
        )
    }

    /// Replaces the controlled split ratio. `LAYOUT`.
    pub fn set_ratio(&mut self, ratio: f32) -> bool {
        self.guarded(
            ratio.clamp(0.05, 0.95),
            Invalidation::LAYOUT,
            |pane| &pane.ratio,
            |pane, ratio| pane.ratio = ratio,
        )
    }

    /// Replaces the erased resize-action factory. No invalidation.
    pub fn set_change_action(&mut self, changed: impl Fn(f32) -> Box<dyn Any> + 'static) {
        self.ui.update(self.handle, Invalidation::empty(), |pane| {
            pane.set_changed(changed);
        });
    }
}

impl NodeMut<'_, Align> {
    /// Replaces child placement. `LAYOUT`.
    pub fn set_alignment(&mut self, alignment: Alignment) -> bool {
        self.guarded(
            alignment,
            Invalidation::LAYOUT,
            |align| &align.alignment,
            |align, alignment| align.alignment = alignment,
        )
    }

    /// Replaces the minimum distance from each boundary. `LAYOUT`.
    pub fn set_padding(&mut self, padding: f32) -> bool {
        self.guarded(
            padding,
            Invalidation::LAYOUT,
            |align| &align.padding,
            |align, padding| align.padding = padding,
        )
    }

    /// Replaces all alignment-container properties. See
    /// [`NodeMut::set_content`] for why the bundles remain.
    pub fn set_align(&mut self, alignment: Alignment, padding: f32) {
        self.set_alignment(alignment);
        self.set_padding(padding);
    }
}

impl NodeMut<'_, ImageElement> {
    /// Replaces the immutable image source. `PAINT | ACCESSIBILITY`.
    pub fn set_image(&mut self, image: Image) -> bool {
        if self.ui.element(self.handle).image.cache_id() == image.cache_id() {
            return false;
        }
        self.ui.update(
            self.handle,
            Invalidation::PAINT | Invalidation::ACCESSIBILITY,
            |element| {
                element.image = image;
            },
        );
        true
    }

    /// Replaces the accessible label. `ACCESSIBILITY`.
    pub fn set_label(&mut self, label: impl Into<String>) -> bool {
        self.guarded(
            label.into(),
            Invalidation::ACCESSIBILITY,
            |element| &element.label,
            |element, label| element.label = label,
        )
    }

    /// Replaces the preferred logical size. `LAYOUT`.
    pub fn set_size(&mut self, size: LogicalSize) -> bool {
        self.guarded(
            size,
            Invalidation::LAYOUT,
            |element| &element.size,
            |element, size| element.size = size,
        )
    }

    /// Replaces the fitting policy. `PAINT`.
    pub fn set_fit(&mut self, fit: ImageFit) -> bool {
        self.guarded(
            fit,
            Invalidation::PAINT,
            |element| &element.fit,
            |element, fit| element.fit = fit,
        )
    }

    /// Replaces normalized placement within the fitted bounds. `PAINT`.
    pub fn set_alignment(&mut self, alignment: ImageAlignment) -> bool {
        self.guarded(
            alignment,
            Invalidation::PAINT,
            |element| &element.alignment,
            |element, alignment| element.alignment = alignment,
        )
    }

    /// Replaces the sampling policy. `PAINT`.
    pub fn set_sampling(&mut self, sampling: ImageSampling) -> bool {
        self.guarded(
            sampling,
            Invalidation::PAINT,
            |element| &element.sampling,
            |element, sampling| element.sampling = sampling,
        )
    }

    /// Replaces draw opacity. `PAINT`.
    pub fn set_opacity(&mut self, opacity: f32) -> bool {
        self.guarded(
            opacity.clamp(0.0, 1.0),
            Invalidation::PAINT,
            |element| &element.opacity,
            |element, opacity| element.opacity = opacity,
        )
    }
}

impl NodeMut<'_, RenderView> {
    /// Replaces the accessible label. `ACCESSIBILITY`.
    pub fn set_label(&mut self, label: impl Into<String>) -> bool {
        self.guarded(
            label.into(),
            Invalidation::ACCESSIBILITY,
            |element| &element.label,
            |element, label| element.label = label,
        )
    }

    /// Replaces the preferred viewport size. `LAYOUT`.
    pub fn set_size(&mut self, size: LogicalSize) -> bool {
        self.guarded(
            size,
            Invalidation::LAYOUT,
            |element| &element.size,
            |element, size| element.size = size,
        )
    }

    /// Replaces rendered content. `PAINT | ACCESSIBILITY`.
    pub fn set_content(&mut self, content: RenderViewContent) -> bool {
        self.guarded(
            content,
            Invalidation::PAINT | Invalidation::ACCESSIBILITY,
            |element| &element.content,
            |element, content| element.content = content,
        )
    }

    /// Replaces typed-erased input routing. `HIT_TEST | ACCESSIBILITY`.
    pub fn set_input(&mut self, input: impl Fn(UiInput) -> Box<dyn Any> + 'static) {
        self.ui.update(
            self.handle,
            Invalidation::HIT_TEST | Invalidation::ACCESSIBILITY,
            |element| {
                element.set_input(input);
            },
        );
    }
}

impl NodeMut<'_, Label> {
    /// Replaces the shaped and accessible text. `LAYOUT`.
    pub fn set_text(&mut self, text: impl Into<String>) -> bool {
        self.guarded(
            text.into(),
            Invalidation::LAYOUT,
            |label| &label.text,
            |label, text| label.text = text,
        )
    }

    /// Replaces the nominal font size. `LAYOUT`.
    pub fn set_font_size(&mut self, font_size: f32) -> bool {
        self.guarded(
            font_size,
            Invalidation::LAYOUT,
            |label| &label.font_size,
            |label, font_size| label.font_size = font_size,
        )
    }

    /// Replaces the glyph colour. `LAYOUT`, because the brush is shaped in.
    pub fn set_color(&mut self, color: Option<Color>) -> bool {
        self.guarded(
            color,
            Invalidation::LAYOUT,
            |label| &label.color,
            |label, color| label.color = color,
        )
    }

    /// Replaces the wrapping width and reserved extent. `LAYOUT`.
    pub fn set_width(&mut self, width: Option<f32>) -> bool {
        let width = width.map(|width| width.max(0.0));
        if self.ui.element(self.handle).width() == width {
            return false;
        }
        self.ui.update(self.handle, Invalidation::LAYOUT, |label| {
            label.set_width(width);
        });
        true
    }

    /// Replaces every resolved text property at once.
    ///
    /// Kept for callers that push a whole description each frame. It delegates,
    /// so it inherits the per-property guards and declares only the union of
    /// what genuinely changed.
    pub fn set_content(&mut self, text: String, font_size: f32, color: Color, width: Option<f32>) {
        self.set_text(text);
        self.set_font_size(font_size);
        self.set_color(Some(color));
        self.set_width(width);
    }
}

impl NodeMut<'_, BoxElement> {
    /// Replaces the preferred size. `LAYOUT`.
    pub fn set_size(&mut self, size: LogicalSize) -> bool {
        self.guarded(
            size,
            Invalidation::LAYOUT,
            |element| &element.size,
            |element, size| element.size = size,
        )
    }

    /// Replaces the fill colour. `PAINT`.
    pub fn set_color(&mut self, color: Color) -> bool {
        self.guarded(
            color,
            Invalidation::PAINT,
            |element| &element.color,
            |element, color| element.color = color,
        )
    }

    /// Replaces the accessible properties. `ACCESSIBILITY`.
    pub fn set_semantics(&mut self, semantics: Option<SemanticData>) -> bool {
        self.guarded(
            semantics,
            Invalidation::ACCESSIBILITY,
            |element| &element.semantics,
            |element, semantics| element.semantics = semantics,
        )
    }

    /// Selects whether pointer input targets this box. `HIT_TEST`.
    pub fn set_interactive(&mut self, interactive: bool) -> bool {
        self.guarded(
            interactive,
            Invalidation::HIT_TEST,
            |element| &element.interactive,
            |element, interactive| element.interactive = interactive,
        )
    }

    /// Replaces every resolved box property at once. See
    /// [`NodeMut::set_content`] for why the bundles remain.
    pub fn set_box(
        &mut self,
        size: LogicalSize,
        color: Color,
        semantics: Option<SemanticData>,
        interactive: bool,
    ) {
        self.set_size(size);
        self.set_color(color);
        self.set_semantics(semantics);
        self.set_interactive(interactive);
    }
}

impl<A: Clone + 'static> NodeMut<'_, ActionBox<A>> {
    /// Replaces the visual and semantic surface through equality-guarded
    /// property setters.
    pub fn set_surface(&mut self, surface: BoxElement) {
        let current = &self.ui.element(self.handle).surface;
        let size_changed = current.size != surface.size;
        let color_changed = current.color != surface.color;
        let semantics_changed = current.semantics != surface.semantics;
        let interactive_changed = current.interactive != surface.interactive;
        if size_changed || color_changed || semantics_changed || interactive_changed {
            let mut invalidation = Invalidation::empty();
            if size_changed {
                invalidation |= Invalidation::LAYOUT;
            }
            if color_changed {
                invalidation |= Invalidation::PAINT;
            }
            if semantics_changed {
                invalidation |= Invalidation::ACCESSIBILITY;
            }
            if interactive_changed {
                invalidation |= Invalidation::HIT_TEST;
            }
            self.ui.update(self.handle, invalidation, |box_element| {
                box_element.surface = surface;
            });
        }
    }

    /// Replaces routing data without requesting a retained pass.
    pub fn set_action(&mut self, action: Option<A>) {
        self.ui
            .update(self.handle, Invalidation::empty(), |box_element| {
                box_element.action = action;
            });
    }
}

impl NodeMut<'_, Flex> {
    /// Replaces the main axis. `LAYOUT`.
    pub fn set_axis(&mut self, axis: Axis) -> bool {
        self.guarded(
            axis,
            Invalidation::LAYOUT,
            |flex| &flex.axis,
            |flex, axis| flex.axis = axis,
        )
    }

    /// Replaces the gap between adjacent children. `LAYOUT`.
    pub fn set_gap(&mut self, gap: f32) -> bool {
        self.guarded(
            gap,
            Invalidation::LAYOUT,
            |flex| &flex.gap,
            |flex, gap| flex.gap = gap,
        )
    }

    /// Replaces the container insets. `LAYOUT`.
    pub fn set_padding(&mut self, padding: f32) -> bool {
        self.guarded(
            padding,
            Invalidation::LAYOUT,
            |flex| &flex.padding,
            |flex, padding| flex.padding = padding,
        )
    }

    /// Replaces the optional background fill. `PAINT`.
    pub fn set_background(&mut self, background: Option<Color>) -> bool {
        self.guarded(
            background,
            Invalidation::PAINT,
            |flex| &flex.background,
            |flex, background| flex.background = background,
        )
    }

    /// Replaces every resolved flex property at once. See
    /// [`NodeMut::set_content`] for why the bundles remain.
    pub fn set_flex(&mut self, axis: Axis, gap: f32, padding: f32, background: Option<Color>) {
        self.set_axis(axis);
        self.set_gap(gap);
        self.set_padding(padding);
        self.set_background(background);
    }
}

impl NodeMut<'_, Frame> {
    /// Replaces the optional preferred width. `LAYOUT`.
    pub fn set_width(&mut self, width: Option<f32>) -> bool {
        self.guarded(
            width.map(|width| width.max(0.0)),
            Invalidation::LAYOUT,
            |frame| &frame.width,
            |frame, width| frame.width = width,
        )
    }

    /// Replaces the optional preferred height. `LAYOUT`.
    pub fn set_height(&mut self, height: Option<f32>) -> bool {
        self.guarded(
            height.map(|height| height.max(0.0)),
            Invalidation::LAYOUT,
            |frame| &frame.height,
            |frame, height| frame.height = height,
        )
    }

    /// Replaces the minimum accepted size. `LAYOUT`.
    pub fn set_min(&mut self, min: LogicalSize) -> bool {
        self.guarded(
            LogicalSize::new(min.width.max(0.0), min.height.max(0.0)),
            Invalidation::LAYOUT,
            |frame| &frame.min,
            |frame, min| frame.min = min,
        )
    }

    /// Replaces the optional maximum size. `LAYOUT`.
    ///
    /// Not reconciled against the minimum, unlike the bundle this replaced:
    /// `Frame::layout` already resolves a minimum above the maximum in favour of
    /// the maximum, and normalizing here made the stored value depend on which of
    /// the two happened to be written first.
    pub fn set_max(&mut self, max: Option<LogicalSize>) -> bool {
        self.guarded(
            max.map(|max| LogicalSize::new(max.width.max(0.0), max.height.max(0.0))),
            Invalidation::LAYOUT,
            |frame| &frame.max,
            |frame, max| frame.max = max,
        )
    }

    /// Replaces the relative flex growth. `LAYOUT`.
    pub fn set_grow(&mut self, grow: f32) -> bool {
        self.guarded(
            grow.max(0.0),
            Invalidation::LAYOUT,
            |frame| &frame.grow,
            |frame, grow| frame.grow = grow,
        )
    }

    /// Replaces every resolved frame property at once. See
    /// [`NodeMut::set_content`] for why the bundles remain.
    pub fn set_frame(
        &mut self,
        width: Option<f32>,
        height: Option<f32>,
        min: LogicalSize,
        max: Option<LogicalSize>,
        grow: f32,
    ) {
        self.set_width(width);
        self.set_height(height);
        self.set_min(min);
        self.set_max(max);
        self.set_grow(grow);
    }
}

impl NodeMut<'_, Stack> {
    /// Replaces the overlay insets. `LAYOUT`.
    pub fn set_padding(&mut self, padding: f32) -> bool {
        self.guarded(
            padding,
            Invalidation::LAYOUT,
            |stack| &stack.padding,
            |stack, padding| stack.padding = padding,
        )
    }

    /// Replaces the optional background fill. `PAINT`.
    pub fn set_background(&mut self, background: Option<Color>) -> bool {
        self.guarded(
            background,
            Invalidation::PAINT,
            |stack| &stack.background,
            |stack, background| stack.background = background,
        )
    }

    /// Replaces every resolved overlay property at once. See
    /// [`NodeMut::set_content`] for why the bundles remain.
    pub fn set_stack(&mut self, padding: f32, background: Option<Color>) {
        self.set_padding(padding);
        self.set_background(background);
    }
}

impl NodeMut<'_, Scroll> {
    /// Replaces the enabled scroll axes. `LAYOUT`, because the axes decide what
    /// the content is measured against.
    pub fn set_axis(&mut self, axis: ScrollAxis) -> bool {
        self.guarded(
            axis,
            Invalidation::LAYOUT,
            |scroll| &scroll.axis,
            |scroll, axis| scroll.axis = axis,
        )
    }

    /// Moves the content within the viewport, clamped to the resolved extent.
    /// `COMPOSE | PAINT | ACCESSIBILITY | HIT_TEST`.
    ///
    /// The offset reaches the children as their layout offset, and this assigns
    /// it to them directly, so the content moves without a layout pass at all.
    /// [`Scroll`]'s own wheel handling cannot do the same: an element has no
    /// route to its children outside its own `layout`.
    pub fn set_offset(&mut self, offset: LogicalPoint) -> bool {
        let scroll = self.ui.element(self.handle);
        let offset = scroll.clamped(offset);
        if scroll.offset == offset {
            return false;
        }
        self.ui.update(self.handle, MOVED, |scroll| {
            scroll.offset = offset;
        });
        let origin = self.ui.element(self.handle).content_origin();
        let parent = self.handle.id();
        for child in self.ui.children_ids(parent) {
            self.ui.place_child(parent, child, origin);
        }
        true
    }

    /// Declares or clears the known content extent. `LAYOUT`.
    pub fn set_content_extent(&mut self, extent: Option<LogicalSize>) -> bool {
        self.guarded(
            extent,
            Invalidation::LAYOUT,
            |scroll| &scroll.content_extent,
            |scroll, extent| scroll.content_extent = extent,
        )
    }

    /// Replaces the erased scroll-action factory. No invalidation.
    pub fn set_scrolled_factory(
        &mut self,
        scrolled: impl Fn(LogicalPoint) -> Box<dyn Any> + 'static,
    ) {
        self.ui
            .update(self.handle, Invalidation::empty(), |scroll| {
                scroll.set_scrolled_factory(scrolled);
            })
    }

    /// Replaces the controlled axes and offset at once.
    ///
    /// Deliberately not delegating, unlike the other bundles: an offset supplied
    /// alongside an axis change has to be clamped against the extent the new axis
    /// produces, which only the next layout pass knows, whereas
    /// [`NodeMut::set_offset`] clamps against the extent the last one
    /// resolved.
    pub fn set_scroll(&mut self, axis: ScrollAxis, offset: LogicalPoint) {
        let scroll = self.ui.element(self.handle);
        if scroll.axis == axis && scroll.offset == offset {
            return;
        }
        self.ui.update(self.handle, Invalidation::LAYOUT, |scroll| {
            scroll.axis = axis;
            scroll.offset = offset;
        })
    }
}
