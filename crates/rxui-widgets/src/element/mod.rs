//! Widgets that implement their own retained [`astrelis_ui_next::Element`].
//!
//! These are the only two surfaces in RXUI that paint themselves rather than
//! composing existing view kinds, which is why they are filed by *technique*
//! here and the docking workspace - which has no custom element - is filed by
//! *optionality* in [`crate::compose`].

#[cfg(feature = "charts")]
pub mod chart;
#[cfg(feature = "graph")]
pub mod graph;

#[cfg(any(feature = "charts", feature = "graph"))]
pub(crate) mod canvas {
    //! What the two self-painting surfaces here hold in common.
    //!
    //! Almost nothing about their drawing is shared - one projects a domain and
    //! the other applies a viewport - but both are opaque canvases that fill
    //! before they draw, and both answer the pointer from element state instead
    //! of from a controlled spec field. Those two decisions are the ones that
    //! have to agree between them, so they live here and not in either file.

    use astrelis_core::{
        color::Color,
        geometry::{LogicalRect, LogicalSize},
    };
    use astrelis_paint::{Brush, PaintError, Painter};
    use astrelis_ui_next::Invalidation;

    /// The fill a canvas sits on when nothing else is asked for.
    ///
    /// Named once because a chart docked beside a graph is meant to read as one
    /// dark surface with two pictures on it, and because neither element takes
    /// its fill from the theme yet - when one of them does, this is the single
    /// place that has to learn about it.
    pub(crate) fn surface_fill() -> Color {
        Color::from_hex(0x16181d)
    }

    /// What the pointer is over, as a canvas tracks it between frames.
    ///
    /// Hover is element state on both canvases rather than a spec field,
    /// because routing a pointer position through the component reducer would
    /// rebuild the view on every mouse move to move a highlight that only
    /// `paint` ever reads.
    pub(crate) struct Hover<T> {
        current: Option<T>,
    }

    impl<T> Default for Hover<T> {
        /// Nothing is hovered at birth, whatever the target type is.
        ///
        /// Written out rather than derived because the derive would demand
        /// `T: Default` of ids that have no sensible default.
        fn default() -> Self {
            Self { current: None }
        }
    }

    impl<T: PartialEq> Hover<T> {
        /// Records the target under the pointer and reports the frame it owes.
        ///
        /// A pointer that moves within the target it was already over is the
        /// overwhelmingly common case, and it has to cost nothing: the empty
        /// invalidation is what keeps the engine from producing a frame
        /// identical to the one on screen.
        pub(crate) fn set(&mut self, target: Option<T>) -> Invalidation {
            if self.current == target {
                return Invalidation::empty();
            }
            self.current = target;
            Invalidation::PAINT
        }

        /// The hovered target, for the paint that has to highlight it.
        pub(crate) fn current(&self) -> Option<&T> {
            self.current.as_ref()
        }
    }

    /// Draws canvas content over an opaque fill, clipped to the element.
    ///
    /// Neither canvas keeps its own drawing inside the size it was handed: the
    /// chart projects into an inset box derived from live data, and the graph
    /// applies a pan and zoom the application controls. The clip is the only
    /// thing that stops either from painting over its neighbours, and the fill
    /// beneath is what makes the result a surface rather than a picture drawn
    /// onto whatever the parent left there.
    pub(crate) fn draw(
        painter: &mut Painter,
        size: LogicalSize,
        background: Color,
        content: impl FnOnce(&mut Painter) -> Result<(), PaintError>,
    ) -> Result<(), PaintError> {
        painter.with_save(|painter| {
            let bounds = LogicalRect::from_xywh(0.0, 0.0, size.width, size.height);
            painter.clip_rect(bounds)?;
            painter.fill_rect(bounds, Brush::Solid(background))?;
            content(painter)
        })
    }
}
