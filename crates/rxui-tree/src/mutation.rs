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

use std::any::Any;

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalSize},
};

use crate::{
    Axis, BoxElement, Element, Flex, Frame, Invalidation, Label, NodeHandle, Scroll, ScrollAxis,
    SemanticData, Stack, UiTree,
};

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
