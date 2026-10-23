//! Retained vector paths and explicit fill/stroke preparation.

use std::sync::Arc;

use crate::{DrawSpace, EdgeAntialiasing, Error, LineCap, Transform2D};
use lyon_tessellation::path::{Path as LyonPath, math::point};

mod geometry;
mod renderer;
pub use renderer::{PathDrawSession, PathRenderer, PreparedPath};

/// Immutable CPU path containing straight, quadratic, and cubic segments.
///
/// Multiple contours and self-intersections are allowed. Coordinates have no
/// implicit DPI or viewport conversion. Filling implicitly closes open contours;
/// stroking keeps them open unless explicitly closed. Clones share CPU storage.
/// A path is independent of devices, paint color, transforms, and stroke width.
/// Prepare it explicitly with [`PathRenderer::prepare_path`] before drawing.
///
/// ```
/// use rxui_draw::Path;
/// let mut builder = Path::builder();
/// builder.move_to([0., 0.]);
/// builder.line_to([40., 0.]);
/// builder.quadratic_to([60., 20.], [40., 40.]);
/// builder.cubic_to([20., 50.], [0., 30.], [0., 0.]);
/// builder.close();
/// let path = builder.build()?;
/// assert!(!path.is_empty());
/// # Ok::<(), rxui_draw::Error>(())
/// ```
#[derive(Clone, Debug)]
pub struct Path {
    inner: Arc<LyonPath>,
    empty: bool,
}
impl Path {
    /// Starts an empty builder. Commands are validated together by `build`.
    pub fn builder() -> PathBuilder {
        PathBuilder::default()
    }
    /// Whether this path contains no line or curve segments.
    /// A nonempty path can still produce empty fill/stroke geometry.
    pub fn is_empty(&self) -> bool {
        self.empty
    }
}

#[derive(Clone, Copy, Debug)]
enum Command {
    Move([f32; 2]),
    Line([f32; 2]),
    Quadratic([f32; 2], [f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
    Close,
}

/// Mutable CPU path construction, with validation when [`Self::build`] consumes it.
///
/// `move_to` begins a contour and ends any previous open contour. Segments and
/// `close` require an open contour; after closing, begin another with `move_to`.
/// Invalid order or nonfinite coordinates return [`Error::InvalidPath`] instead
/// of reaching the tessellator. An empty builder creates a valid empty path.
#[derive(Debug, Default)]
pub struct PathBuilder {
    commands: Vec<Command>,
}
impl PathBuilder {
    /// Starts a contour at `position`, ending an earlier contour without closing it.
    pub fn move_to(&mut self, position: [f32; 2]) -> &mut Self {
        self.commands.push(Command::Move(position));
        self
    }
    /// Adds a straight segment from the current point to `position`.
    pub fn line_to(&mut self, position: [f32; 2]) -> &mut Self {
        self.commands.push(Command::Line(position));
        self
    }
    /// Adds a quadratic Bézier segment with one control point.
    pub fn quadratic_to(&mut self, control: [f32; 2], position: [f32; 2]) -> &mut Self {
        self.commands.push(Command::Quadratic(control, position));
        self
    }
    /// Adds a cubic Bézier segment with two control points.
    pub fn cubic_to(
        &mut self,
        control1: [f32; 2],
        control2: [f32; 2],
        position: [f32; 2],
    ) -> &mut Self {
        self.commands
            .push(Command::Cubic(control1, control2, position));
        self
    }
    /// Closes the active contour with a straight segment to its first point.
    pub fn close(&mut self) -> &mut Self {
        self.commands.push(Command::Close);
        self
    }
    /// Validates command order/coordinates and consumes this builder into a path.
    /// The last open contour remains open. No tessellation or GPU work happens here.
    pub fn build(self) -> Result<Path, Error> {
        let mut builder = LyonPath::builder();
        let mut active = false;
        let mut empty = true;
        for (command, value) in self.commands.into_iter().enumerate() {
            let valid = |p: [f32; 2]| p.into_iter().all(f32::is_finite);
            match value {
                Command::Move(p) if valid(p) => {
                    if active {
                        builder.end(false);
                    }
                    builder.begin(point(p[0], p[1]));
                    active = true;
                }
                Command::Line(p) if active && valid(p) => {
                    builder.line_to(point(p[0], p[1]));
                    empty = false;
                }
                Command::Quadratic(c, p) if active && valid(c) && valid(p) => {
                    builder.quadratic_bezier_to(point(c[0], c[1]), point(p[0], p[1]));
                    empty = false;
                }
                Command::Cubic(c1, c2, p) if active && valid(c1) && valid(c2) && valid(p) => {
                    builder.cubic_bezier_to(
                        point(c1[0], c1[1]),
                        point(c2[0], c2[1]),
                        point(p[0], p[1]),
                    );
                    empty = false;
                }
                Command::Close if active => {
                    builder.end(true);
                    active = false;
                }
                _ => return Err(Error::InvalidPath { command }),
            }
        }
        if active {
            builder.end(false);
        }
        Ok(Path {
            inner: Arc::new(builder.build()),
            empty,
        })
    }
}

/// Determines which regions enclosed by path contours are filled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillRule {
    /// Fill regions with a nonzero winding count. Opposite-winding contours cut holes.
    #[default]
    NonZero,
    /// Fill regions crossed an odd number of times, regardless of contour orientation.
    EvenOdd,
}

/// Connection between adjacent segments of a stroked path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineJoin {
    /// Extend offset edges to their intersection; bevel when the miter limit is exceeded.
    #[default]
    Miter,
    /// Connect offset edges with a straight bevel.
    Bevel,
    /// Connect offset edges with a circular arc.
    Round,
}

/// Centered path stroke prepared in the path's own coordinate units.
///
/// Changing width, caps, joins, or the miter limit requires preparing new geometry.
/// Color/placement remain per draw. An affine draw transform also scales the
/// stroke; nonuniform scaling does not preserve a uniform screen-space width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathStroke {
    /// Finite, nonnegative stroke width. Zero prepares empty geometry.
    pub width: f32,
    /// Cap at each open contour's start. Closed contours have no caps.
    pub start_cap: LineCap,
    /// Cap at each open contour's end.
    pub end_cap: LineCap,
    /// Connection at each intermediate segment.
    pub join: LineJoin,
    /// Maximum miter length relative to half-width; finite and at least one.
    pub miter_limit: f32,
}
impl PathStroke {
    /// Creates a centered stroke with butt caps, miter joins, and miter limit four.
    pub const fn new(width: f32) -> Self {
        Self {
            width,
            start_cap: LineCap::Butt,
            end_cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.,
        }
    }
    /// Selects the same cap at both ends of every open contour.
    pub const fn cap(mut self, cap: LineCap) -> Self {
        self.start_cap = cap;
        self.end_cap = cap;
        self
    }
    /// Selects connections between adjacent segments.
    pub const fn join(mut self, join: LineJoin) -> Self {
        self.join = join;
        self
    }
    /// Selects the miter limit. Preparation validates the value.
    pub const fn miter_limit(mut self, limit: f32) -> Self {
        self.miter_limit = limit;
        self
    }
}

/// Geometry preparation settings, independent of drawing and attachment formats.
///
/// The default fills with nonzero winding. Selecting a stroke prepares the stroke
/// instead of the fill; prepare twice to retain both. Tolerance controls curve and
/// round-join approximation in path units. It is not a raster-density setting:
/// under a draw transform the approximation error scales with the geometry.
/// For large zooms, prepare with a smaller tolerance explicitly. Smaller tolerances
/// increase preparation time and retained geometry size. Preparation is synchronous.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathOptions {
    /// Fill rule, used only when `stroke` is None.
    pub fill_rule: FillRule,
    /// Optional centered stroke; None selects a fill.
    pub stroke: Option<PathStroke>,
    /// Finite, strictly positive maximum curve approximation error; default 0.1.
    pub tolerance: f32,
}
impl Default for PathOptions {
    fn default() -> Self {
        Self::new()
    }
}
impl PathOptions {
    /// Selects a nonzero fill with tolerance 0.1 in path units.
    pub const fn new() -> Self {
        Self {
            fill_rule: FillRule::NonZero,
            stroke: None,
            tolerance: 0.1,
        }
    }
    /// Selects a fill rule and returns to fill preparation.
    pub const fn fill_rule(mut self, rule: FillRule) -> Self {
        self.fill_rule = rule;
        self.stroke = None;
        self
    }
    /// Selects stroke preparation instead of filling.
    pub const fn stroke(mut self, stroke: PathStroke) -> Self {
        self.stroke = Some(stroke);
        self
    }
    /// Selects the curve approximation error in path units.
    pub const fn tolerance(mut self, tolerance: f32) -> Self {
        self.tolerance = tolerance;
        self
    }
    fn validate(self) -> Result<(), Error> {
        if !self.tolerance.is_finite()
            || self.tolerance <= 0.
            || self.stroke.is_some_and(|s| {
                !s.width.is_finite()
                    || s.width < 0.
                    || !s.miter_limit.is_finite()
                    || s.miter_limit < 1.
            })
        {
            return Err(Error::InvalidPathOptions);
        }
        Ok(())
    }
}

/// Per-draw paint and placement of retained path geometry.
///
/// Coordinates/transform follow [`crate::ShapeDraw`]. Color is linear straight
/// RGBA; the renderer outputs premultiplied color. Coverage uses an approximate
/// one-pixel band centered on the boundary, computed after transformation,
/// including on single-sampled targets. It does not improve curve tessellation.
/// Very narrow features and intersecting fringes can have approximate coverage;
/// select None and target MSAA when geometric sample coverage is preferred.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathDraw {
    /// Linear straight RGBA, with finite RGB and alpha in zero to one.
    pub color: [f32; 4],
    /// Affine transform in the selected units.
    pub transform: Transform2D,
    /// Units of the path and transform before viewport conversion.
    pub space: DrawSpace,
    /// Edge coverage. Does not change target MSAA.
    pub antialiasing: EdgeAntialiasing,
}
impl Default for PathDraw {
    fn default() -> Self {
        Self::new([1.; 4])
    }
}
impl PathDraw {
    /// Creates a pixel-space draw with identity transform and edge coverage.
    pub const fn new(color: [f32; 4]) -> Self {
        Self {
            color,
            transform: Transform2D::IDENTITY,
            space: DrawSpace::Pixels,
            antialiasing: EdgeAntialiasing::Coverage,
        }
    }
    /// Selects color without changing retained geometry.
    pub const fn color(mut self, color: [f32; 4]) -> Self {
        self.color = color;
        self
    }
    /// Selects an affine transform, also accepting a six-element array.
    pub fn transform(mut self, transform: impl Into<Transform2D>) -> Self {
        self.transform = transform.into();
        self
    }
    /// Selects units for both retained coordinates and the draw transform.
    pub const fn space(mut self, space: DrawSpace) -> Self {
        self.space = space;
        self
    }
    /// Selects edge coverage without changing geometry or target sample count.
    pub const fn antialiasing(mut self, antialiasing: EdgeAntialiasing) -> Self {
        self.antialiasing = antialiasing;
        self
    }
}

#[cfg(test)]
mod tests;
