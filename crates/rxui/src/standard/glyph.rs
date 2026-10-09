//! Vector marks painted with the element's resolved text color, so they follow
//! theme tokens and need no font coverage.
use crate::{CustomElement, Element, custom};

/// A small stroked mark sized by its element's content box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Glyph {
    /// Check box tick.
    Check,
    /// Mixed-state bar.
    Dash,
    /// Downward chevron for a select trigger.
    Chevron,
    /// Twelve spokes; `head` is the brightest one.
    Spinner { head: u8 },
}
pub(crate) const SPOKES: u8 = 12;

impl CustomElement for Glyph {
    type State = ();
    #[cfg(feature = "rendering")]
    fn paint(
        &self,
        _: &(),
        element: &crate::ElementInfo<'_>,
        paint: &mut astrelis::PaintSession<'_, '_>,
    ) -> Result<(), crate::UiError> {
        use astrelis::{LineCap, LineDraw};
        let b = element.content_bounds;
        let at = |x: f32, y: f32| [b.x + x * b.width, b.y + y * b.height];
        let width = (b.width.min(b.height)
            * if matches!(self, Self::Spinner { .. }) {
                0.1
            } else {
                0.16
            })
        .max(1.5);
        let color = element.color;
        let line = |start, end, color| LineDraw {
            width,
            cap: LineCap::Round,
            ..LineDraw::new(start, end, color)
        };
        match *self {
            Self::Check => {
                paint.draw_line(line(at(0.18, 0.52), at(0.42, 0.76), color))?;
                paint.draw_line(line(at(0.42, 0.76), at(0.84, 0.28), color))?;
            }
            Self::Dash => paint.draw_line(line(at(0.22, 0.5), at(0.78, 0.5), color))?,
            Self::Chevron => {
                paint.draw_line(line(at(0.2, 0.36), at(0.5, 0.66), color))?;
                paint.draw_line(line(at(0.5, 0.66), at(0.8, 0.36), color))?;
            }
            Self::Spinner { head } => {
                for spoke in 0..SPOKES {
                    let angle = std::f32::consts::TAU * f32::from(spoke) / f32::from(SPOKES);
                    let (sin, cos) = angle.sin_cos();
                    let point = |r: f32| at(0.5 + 0.5 * r * sin, 0.5 - 0.5 * r * cos);
                    // Spokes behind the head fade out, like a platform activity indicator.
                    let age = (head + SPOKES - spoke) % SPOKES;
                    let alpha = 1. - f32::from(age) / f32::from(SPOKES) * 0.85;
                    let mut faded = color;
                    faded[3] *= alpha;
                    paint.draw_line(line(point(0.45), point(0.9), faded))?;
                }
            }
        }
        Ok(())
    }
}

/// A decorative glyph leaf, hidden from assistive technology.
pub(crate) fn glyph(kind: Glyph, size: f32) -> Element {
    custom(kind).size(size, size).accessibility_hidden(true)
}
