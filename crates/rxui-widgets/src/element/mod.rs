//! Widgets that implement their own retained [`rxui_tree::Element`].
//!
//! These are the only two surfaces in RXUI that paint themselves rather than
//! composing existing element kinds, which is why they are filed by technique
//! here and docking—whose view has no custom retained element—is filed under
//! [`crate::compose`].

#[cfg(feature = "charts")]
pub mod chart;
#[cfg(feature = "graph")]
pub mod graph;

#[cfg(any(feature = "charts", feature = "graph"))]
pub(crate) mod canvas {
    //! What the two self-painting surfaces here hold in common.
    //!
    //! Almost nothing about their drawing is shared—one projects a domain and
    //! the other applies a viewport—but both are opaque canvases that fill
    //! before drawing, and both answer the pointer from retained element state.

    use astrelis_core::{
        color::Color,
        geometry::{LogicalRect, LogicalSize},
    };
    use astrelis_paint::{Brush, PaintError, Painter};
    use rxui_tree::Invalidation;

    /// The fill a canvas sits on when nothing else is requested.
    pub(crate) fn surface_fill() -> Color {
        Color::from_hex(0x16181d)
    }

    /// Pointer target retained between frames rather than routed through a model.
    pub(crate) struct Hover<T> {
        current: Option<T>,
    }

    impl<T> Default for Hover<T> {
        fn default() -> Self {
            Self { current: None }
        }
    }

    impl<T: PartialEq> Hover<T> {
        /// Records a target and requests only the paint it can affect.
        pub(crate) fn set(&mut self, target: Option<T>) -> Invalidation {
            if self.current == target {
                return Invalidation::empty();
            }
            self.current = target;
            Invalidation::PAINT
        }

        /// Returns the current pointer target.
        pub(crate) fn current(&self) -> Option<&T> {
            self.current.as_ref()
        }
    }

    /// Draws content over an opaque fill, clipped to the element bounds.
    ///
    /// Projection and pan/zoom may place geometry outside the element; this
    /// shared clip is what keeps either canvas from painting over neighbours.
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
