use crate::Error;

/// Units used by 2D geometry before its affine transform.
///
/// Both spaces start at the current viewport's top-left, with positive X right
/// and positive Y down. Viewport origin is applied by GPU rasterization. RXUI drawing
/// does not apply window DPI: applications can scale logical coordinates explicitly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DrawSpace {
    /// Physical pixels relative to the viewport, the default.
    #[default]
    Pixels,
    /// Viewport fractions: `(1, 1)` is the bottom-right. Axes scale independently.
    Normalized,
}

/// An affine transform `[xx, yx, xy, yy, tx, ty]` in the draw's selected units.
///
/// Maps `(x, y)` to `(xx*x + xy*y + tx, yx*x + yy*y + ty)`. Geometry, stroke
/// width, and caps are transformed together, before viewport conversion. Scaling
/// normalized coordinates therefore can be anisotropic on nonsquare viewports.
/// Draw calls reject nonfinite transforms and overflowing transformed geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform2D(pub [f32; 6]);
impl Default for Transform2D {
    fn default() -> Self {
        Self::IDENTITY
    }
}
impl From<[f32; 6]> for Transform2D {
    fn from(value: [f32; 6]) -> Self {
        Self(value)
    }
}
impl Transform2D {
    /// Identity transform.
    pub const IDENTITY: Self = Self([1., 0., 0., 1., 0., 0.]);
    /// A translation in the selected drawing units.
    pub const fn translation(x: f32, y: f32) -> Self {
        Self([1., 0., 0., 1., x, y])
    }
    /// Independent scaling around the origin.
    pub const fn scale(x: f32, y: f32) -> Self {
        Self([x, 0., 0., y, 0., 0.])
    }
    /// Clockwise rotation in radians around the origin in the Y-down coordinate system.
    pub fn rotation(radians: f32) -> Self {
        let (s, c) = radians.sin_cos();
        Self([c, s, -s, c, 0., 0.])
    }
    /// Applies this transform, then `next`; translation is affected by `next`.
    pub fn then(self, next: Self) -> Self {
        let [a, b, c, d, x, y] = self.0;
        let [e, f, g, h, u, v] = next.0;
        Self([
            e * a + g * b,
            f * a + h * b,
            e * c + g * d,
            f * c + h * d,
            e * x + g * y + u,
            f * x + h * y + v,
        ])
    }
    /// Maps one point without validation. Drawing validates the resulting geometry.
    pub fn transform_point(self, point: [f32; 2]) -> [f32; 2] {
        let [a, b, c, d, x, y] = self.0;
        [
            a * point[0] + c * point[1] + x,
            b * point[0] + d * point[1] + y,
        ]
    }
    /// Returns the inverse transform, or `None` when this transform is singular
    /// or its inverse is not finite. `t.then(inverse)` maps points back to their origin.
    pub fn invert(self) -> Option<Self> {
        let [a, b, c, d, x, y] = self.0.map(f64::from);
        let determinant = a * d - b * c;
        if !determinant.is_finite() || determinant == 0. {
            return None;
        }
        let inverse = Self(
            [
                d / determinant,
                -b / determinant,
                -c / determinant,
                a / determinant,
                (c * y - d * x) / determinant,
                (b * x - a * y) / determinant,
            ]
            .map(|v| v as f32),
        );
        inverse.valid().then_some(inverse)
    }
    /// Axis-aligned bounds of a transformed rectangle, without validation.
    /// Rotation and shear enlarge the bounds beyond the transformed area.
    pub fn transform_bounds(self, rect: Rect) -> Rect {
        let corners = [
            [rect.x, rect.y],
            [rect.right(), rect.y],
            [rect.x, rect.bottom()],
            [rect.right(), rect.bottom()],
        ]
        .map(|p| self.transform_point(p));
        let (mut min, mut max) = (corners[0], corners[0]);
        for p in &corners[1..] {
            min = [min[0].min(p[0]), min[1].min(p[1])];
            max = [max[0].max(p[0]), max[1].max(p[1])];
        }
        Rect::new(min[0], min[1], max[0] - min[0], max[1] - min[1])
    }
    /// Returns the array accepted by existing texture placement APIs.
    pub const fn to_array(self) -> [f32; 6] {
        self.0
    }
    pub(crate) fn valid(self) -> bool {
        self.0.iter().all(|v| v.is_finite())
    }
}

/// A top-left rectangle in explicitly selected destination units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Left coordinate.
    pub x: f32,
    /// Top coordinate.
    pub y: f32,
    /// Nonnegative width.
    pub width: f32,
    /// Nonnegative height.
    pub height: f32,
}
impl Rect {
    /// Creates a rectangle; drawing validates finite values and nonnegative extents.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
    /// Right edge, `x + width`.
    pub fn right(self) -> f32 {
        self.x + self.width
    }
    /// Bottom edge, `y + height`.
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }
    /// Whether a point lies inside, including the top/left edges and excluding the
    /// bottom/right edges, so adjacent rectangles never both contain a point.
    pub fn contains(self, point: [f32; 2]) -> bool {
        point[0] >= self.x
            && point[0] < self.right()
            && point[1] >= self.y
            && point[1] < self.bottom()
    }
    /// The overlapping area, or `None` when the rectangles share no positive area.
    pub fn intersect(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then(|| Self::new(x, y, right - x, bottom - y))
    }
    /// The smallest rectangle containing both, including any gap between them.
    pub fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self::new(
            x,
            y,
            self.right().max(other.right()) - x,
            self.bottom().max(other.bottom()) - y,
        )
    }
    pub(crate) fn array(self) -> [f32; 4] {
        [self.x, self.y, self.width, self.height]
    }
    pub(crate) fn valid(self) -> bool {
        self.array().iter().all(|v| v.is_finite()) && self.width >= 0. && self.height >= 0.
    }
}

/// Independent circular corner radii, clockwise from the top-left in a Y-down space.
///
/// Radii use the draw's units before transformation. When adjacent radii exceed
/// a side, every radius is scaled by the same factor until they fit, as in CSS;
/// each corner is then limited to half the shorter side, so corners stay circular.
/// A zero corner stays sharp, including in outlines.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRadii {
    /// Top-left radius.
    pub top_left: f32,
    /// Top-right radius.
    pub top_right: f32,
    /// Bottom-right radius.
    pub bottom_right: f32,
    /// Bottom-left radius.
    pub bottom_left: f32,
}
impl CornerRadii {
    /// All corners sharp.
    pub const ZERO: Self = Self::uniform(0.);
    /// Selects each corner, clockwise from the top-left.
    pub const fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self {
        Self {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }
    /// The same radius at every corner.
    pub const fn uniform(radius: f32) -> Self {
        Self::new(radius, radius, radius, radius)
    }
    /// `[top_left, top_right, bottom_right, bottom_left]`.
    pub const fn to_array(self) -> [f32; 4] {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
    }
    pub(crate) fn valid(self) -> bool {
        self.to_array().iter().all(|r| r.is_finite() && *r >= 0.)
    }
    /// Scales all radii uniformly so adjacent corners fit a `width` x `height` box,
    /// then limits each to half the shorter side.
    pub(crate) fn fitted(self, width: f32, height: f32) -> [f32; 4] {
        let [tl, tr, br, bl] = self.to_array();
        let mut factor = 1f32;
        for (side, sum) in [
            (width, tl + tr),
            (width, bl + br),
            (height, tl + bl),
            (height, tr + br),
        ] {
            if sum > side {
                factor = factor.min(side / sum);
            }
        }
        let half = width.min(height) * 0.5;
        [tl, tr, br, bl].map(|r| (r * factor).min(half))
    }
}
impl From<f32> for CornerRadii {
    fn from(radius: f32) -> Self {
        Self::uniform(radius)
    }
}
impl From<[f32; 4]> for CornerRadii {
    fn from([top_left, top_right, bottom_right, bottom_left]: [f32; 4]) -> Self {
        Self::new(top_left, top_right, bottom_right, bottom_left)
    }
}

/// Edge coverage for solid 2D primitives, independent of target MSAA.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeAntialiasing {
    /// Approximate one-pixel analytic edge coverage, including on single-sampled targets.
    #[default]
    Coverage,
    /// Hard shader edges. Target MSAA remains controlled by the application.
    None,
}

/// Primitive geometry for fills and outlines. Paths remain a separate capability.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// Axis-aligned rectangle before transformation.
    Rectangle,
    /// Circular corners with independent radii, scaled together until adjacent
    /// corners fit and limited to half the shorter dimension.
    /// All-zero radii behave as [`Self::Rectangle`], including sharp outline corners.
    RoundedRectangle {
        /// Nonnegative radii in the draw's units, before transformation.
        radii: CornerRadii,
    },
    /// Ellipse inscribed in the rectangle, before transformation.
    Ellipse,
    /// A rounded rectangle convolved with a Gaussian: the soft shape of a box shadow.
    /// Fills only; outlines are rejected. The quad grows by three standard deviations.
    Shadow {
        /// Corner radii of the unblurred shape, fitted like [`Self::RoundedRectangle`].
        radii: CornerRadii,
        /// Nonnegative blur radius in the draw's units, as in CSS: the Gaussian's
        /// standard deviation is half of it. Zero gives the sharp rounded rectangle.
        blur: f32,
    },
}

/// Offset, blur, spread and color of an outer box shadow, as in CSS `box-shadow`.
///
/// Use with [`ShapeDraw::box_shadow`] or [`crate::PaintSession::draw_box_shadow`],
/// before drawing the element that casts it. The whole blurred shape is drawn,
/// including beneath the element; inset shadows are not provided.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShadow {
    /// Displacement of the shadow from its element, in the draw's units.
    pub offset: [f32; 2],
    /// Nonnegative blur radius; the Gaussian's standard deviation is half of it.
    pub blur: f32,
    /// Outset of the shadow shape on every side before blurring; negative shrinks it.
    pub spread: f32,
    /// Linear, straight RGBA.
    pub color: [f32; 4],
}
impl BoxShadow {
    /// A sharp shadow directly beneath the element.
    pub const fn new(color: [f32; 4]) -> Self {
        Self {
            offset: [0., 0.],
            blur: 0.,
            spread: 0.,
            color,
        }
    }
    /// Selects the displacement.
    pub const fn offset(mut self, x: f32, y: f32) -> Self {
        self.offset = [x, y];
        self
    }
    /// Selects the CSS blur radius.
    pub const fn blur(mut self, blur: f32) -> Self {
        self.blur = blur;
        self
    }
    /// Selects the spread distance.
    pub const fn spread(mut self, spread: f32) -> Self {
        self.spread = spread;
        self
    }
}

/// Placement of a shape's stroke relative to its original boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StrokePlacement {
    /// Half the width on either side of the boundary, the default.
    #[default]
    Center,
    /// All of the width inside the boundary; useful for UI borders.
    Inside,
    /// All of the width outside the boundary.
    Outside,
}

/// Shape outline width and placement, independent of color and GPU resources.
///
/// Width uses the draw's units before transformation, just like [`LineDraw::width`].
/// An affine transform scales the outline with its shape; nonuniform scaling does
/// not preserve a uniform screen-space width. Rectangle outlines have sharp
/// corners. Rounded rectangles offset circular corners and clamp a collapsed
/// inner radius to zero. Ellipse outlines offset the original curve by distance,
/// rather than subtracting another ellipse with smaller axes. Distance/coverage
/// calculations use finite-precision shader arithmetic and approximate edge filtering.
/// A width which consumes the interior leaves a solid shape; zero width draws nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    /// Finite, nonnegative outline width in the draw's units.
    pub width: f32,
    /// Placement relative to the original boundary.
    pub placement: StrokePlacement,
}
impl Stroke {
    /// Creates a centered outline; drawing validates the width.
    pub const fn new(width: f32) -> Self {
        Self {
            width,
            placement: StrokePlacement::Center,
        }
    }
    /// Places the full width inside the boundary.
    pub const fn inside(mut self) -> Self {
        self.placement = StrokePlacement::Inside;
        self
    }
    /// Places half the width on either side of the boundary.
    pub const fn centered(mut self) -> Self {
        self.placement = StrokePlacement::Center;
        self
    }
    /// Places the full width outside the boundary.
    pub const fn outside(mut self) -> Self {
        self.placement = StrokePlacement::Outside;
        self
    }
    pub(crate) fn offsets(self) -> (f32, f32) {
        match self.placement {
            StrokePlacement::Inside => (self.width, 0.),
            StrokePlacement::Center => (self.width * 0.5, self.width * 0.5),
            StrokePlacement::Outside => (0., self.width),
        }
    }
    pub(crate) fn valid(self) -> bool {
        self.width.is_finite() && self.width >= 0.
    }
}

/// One filled or outlined solid primitive, independent of GPU resources.
///
/// Color is **linear, straight RGBA**: RGB is finite (HDR values are allowed),
/// alpha is in `0..=1`. Renderers output premultiplied color and use source-over
/// blending. Zero extents or singular transforms produce no area. Drawing order
/// is explicit; batching preserves input order and does not sort by primitive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeDraw {
    /// Original geometry bounds before transformation. Centered/outside strokes
    /// extend beyond this rectangle; inside strokes stay within it.
    pub rect: Rect,
    /// Geometry whose boundary is used for filling or stroking.
    pub shape: Shape,
    /// `None` fills the shape; `Some` draws only its outline.
    pub stroke: Option<Stroke>,
    /// Linear, straight RGBA.
    pub color: [f32; 4],
    /// Geometry units.
    pub space: DrawSpace,
    /// Affine transform in those units.
    pub transform: Transform2D,
    /// Shader edge coverage.
    pub antialiasing: EdgeAntialiasing,
}
impl ShapeDraw {
    /// A filled rectangle in physical pixels with identity transform and edge coverage.
    pub const fn rect(rect: Rect, color: [f32; 4]) -> Self {
        Self {
            rect,
            shape: Shape::Rectangle,
            stroke: None,
            color,
            space: DrawSpace::Pixels,
            transform: Transform2D::IDENTITY,
            antialiasing: EdgeAntialiasing::Coverage,
        }
    }
    /// A filled uniformly rounded rectangle. Drawing validates the radius.
    pub const fn rounded_rect(rect: Rect, radius: f32, color: [f32; 4]) -> Self {
        Self::rounded_rect_corners(rect, CornerRadii::uniform(radius), color)
    }
    /// A filled rounded rectangle with independent corner radii.
    pub const fn rounded_rect_corners(rect: Rect, radii: CornerRadii, color: [f32; 4]) -> Self {
        Self {
            shape: Shape::RoundedRectangle { radii },
            ..Self::rect(rect, color)
        }
    }
    /// The shadow cast by a rounded rectangle element at `rect`.
    ///
    /// The shape is `rect` moved by the offset and grown by the spread; nonzero radii
    /// grow by the spread too (never below zero). A shape shrunk to nothing draws nothing.
    /// Space, transform and edge coverage can be changed afterwards like any shape.
    pub fn box_shadow(rect: Rect, radii: CornerRadii, shadow: BoxShadow) -> Self {
        // Comparisons keep NaN so that validation rejects it.
        let positive = |v: f32| if v < 0. { 0. } else { v };
        let s = shadow.spread;
        let shape = Rect::new(
            rect.x + shadow.offset[0] - s,
            rect.y + shadow.offset[1] - s,
            positive(rect.width + 2. * s),
            positive(rect.height + 2. * s),
        );
        let radii = radii
            .to_array()
            .map(|r| if r > 0. { positive(r + s) } else { r });
        Self {
            shape: Shape::Shadow {
                radii: radii.into(),
                blur: shadow.blur,
            },
            ..Self::rect(shape, shadow.color)
        }
    }
    /// A filled ellipse inscribed in `rect`.
    pub const fn ellipse(rect: Rect, color: [f32; 4]) -> Self {
        Self {
            shape: Shape::Ellipse,
            ..Self::rect(rect, color)
        }
    }
    /// Draws an outline instead of a fill, preserving geometry/color/space/transform.
    /// Zero extents still draw nothing, including for outside strokes.
    pub const fn stroke(mut self, stroke: Stroke) -> Self {
        self.stroke = Some(stroke);
        self
    }
    /// Draws a fill instead of an outline.
    pub const fn filled(mut self) -> Self {
        self.stroke = None;
        self
    }
    /// Selects viewport fractions or pixels for geometry and transform.
    pub const fn space(mut self, space: DrawSpace) -> Self {
        self.space = space;
        self
    }
    /// Selects an affine transform, also accepting the texture API's six-element array.
    pub fn transform(mut self, transform: impl Into<Transform2D>) -> Self {
        self.transform = transform.into();
        self
    }
    /// Selects shader edge coverage; does not change target MSAA.
    pub const fn antialiasing(mut self, value: EdgeAntialiasing) -> Self {
        self.antialiasing = value;
        self
    }
    /// Checks finite input values, nonnegative geometry, and straight alpha.
    /// Rendering also checks transformed bounds against the current viewport.
    pub fn validate(self) -> Result<(), Error> {
        if !self.rect.valid()
            || !color_valid(self.color)
            || !self.transform.valid()
            || self.stroke.is_some_and(|stroke| !stroke.valid())
            || matches!(self.shape, Shape::RoundedRectangle { radii } if !radii.valid())
            || matches!(self.shape, Shape::Shadow { radii, blur }
                if !radii.valid() || !blur.is_finite() || blur < 0. || self.stroke.is_some())
        {
            return Err(Error::InvalidShapeDraw);
        }
        Ok(())
    }
}

/// End caps for an independent line segment; no connected-path joins are implied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineCap {
    /// Ends exactly at the endpoints.
    #[default]
    Butt,
    /// Extends each endpoint by half the width.
    Square,
    /// Semicircular caps centered on the endpoints.
    Round,
}

/// One solid line segment expanded to triangles by the built-in renderer.
///
/// Endpoints and width use the same selected units before transformation. The
/// default is one physical pixel and butt caps. Zero width draws nothing. A
/// zero-length round/square segment is a disk/square; a zero-length butt segment
/// draws nothing. Color and blending follow [`ShapeDraw`]. This type represents
/// independent segments; connected polylines and stroke joins are not supported.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineDraw {
    /// Start point.
    pub start: [f32; 2],
    /// End point.
    pub end: [f32; 2],
    /// Nonnegative width before transformation.
    pub width: f32,
    /// End cap behavior.
    pub cap: LineCap,
    /// Linear, straight RGBA.
    pub color: [f32; 4],
    /// Endpoint and width units.
    pub space: DrawSpace,
    /// Affine transform in those units.
    pub transform: Transform2D,
    /// Shader edge coverage.
    pub antialiasing: EdgeAntialiasing,
}
impl LineDraw {
    /// Creates a one-pixel segment with butt caps and analytic edge coverage.
    pub const fn new(start: [f32; 2], end: [f32; 2], color: [f32; 4]) -> Self {
        Self {
            start,
            end,
            color,
            width: 1.,
            cap: LineCap::Butt,
            space: DrawSpace::Pixels,
            transform: Transform2D::IDENTITY,
            antialiasing: EdgeAntialiasing::Coverage,
        }
    }
    /// Selects width in the draw's units before transformation.
    pub const fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
    /// Selects independent segment caps.
    pub const fn cap(mut self, cap: LineCap) -> Self {
        self.cap = cap;
        self
    }
    /// Selects the units of endpoints, width, and transform.
    pub const fn space(mut self, space: DrawSpace) -> Self {
        self.space = space;
        self
    }
    /// Selects an affine transform, also accepting a six-element array.
    pub fn transform(mut self, transform: impl Into<Transform2D>) -> Self {
        self.transform = transform.into();
        self
    }
    /// Selects shader edge coverage; does not change target MSAA.
    pub const fn antialiasing(mut self, value: EdgeAntialiasing) -> Self {
        self.antialiasing = value;
        self
    }
    /// Checks finite input values, nonnegative geometry, and straight alpha.
    /// Rendering also checks transformed bounds against the current viewport.
    pub fn validate(self) -> Result<(), Error> {
        if !self.start.iter().chain(&self.end).all(|v| v.is_finite())
            || !self.width.is_finite()
            || self.width < 0.
            || !color_valid(self.color)
            || !self.transform.valid()
        {
            return Err(Error::InvalidLineDraw);
        }
        Ok(())
    }
}
pub(crate) fn color_valid(color: [f32; 4]) -> bool {
    color.iter().all(|v| v.is_finite()) && (0. ..=1.).contains(&color[3])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transform_order_and_y_down_rotation() {
        let t = Transform2D::translation(2., 3.).then(Transform2D::scale(2., 4.));
        assert_eq!(t.transform_point([1., 1.]), [6., 16.]);
        let p = Transform2D::rotation(std::f32::consts::FRAC_PI_2).transform_point([1., 0.]);
        assert!(p[0].abs() < 1e-6 && (p[1] - 1.).abs() < 1e-6);
    }
    #[test]
    fn inverse_bounds_and_rectangle_helpers() {
        let t = Transform2D::scale(2., 4.)
            .then(Transform2D::rotation(0.3))
            .then(Transform2D::translation(5., -7.));
        let inverse = t.invert().unwrap();
        let p = t.then(inverse).transform_point([3., 9.]);
        assert!((p[0] - 3.).abs() < 1e-4 && (p[1] - 9.).abs() < 1e-4);
        assert!(Transform2D::scale(0., 1.).invert().is_none());
        assert!(Transform2D([1e-39, 0., 0., 1., 0., 0.]).invert().is_none());
        let bounds = Transform2D::rotation(std::f32::consts::FRAC_PI_2)
            .transform_bounds(Rect::new(0., 0., 4., 2.));
        assert!((bounds.x + 2.).abs() < 1e-5 && (bounds.width - 2.).abs() < 1e-5);
        assert!((bounds.height - 4.).abs() < 1e-5);
        let a = Rect::new(0., 0., 10., 10.);
        let b = Rect::new(5., 8., 10., 10.);
        assert_eq!(a.intersect(b), Some(Rect::new(5., 8., 5., 2.)));
        assert_eq!(a.intersect(Rect::new(10., 0., 5., 5.)), None);
        assert_eq!(a.union(b), Rect::new(0., 0., 15., 18.));
        assert!(a.contains([0., 0.]) && a.contains([9.9, 9.9]));
        assert!(!a.contains([10., 5.]) && !a.contains([5., -0.1]));
        assert_eq!(
            CornerRadii::new(15., 5., 0., 15.).fitted(40., 100.),
            [15., 5., 0., 15.]
        );
        assert_eq!(CornerRadii::uniform(30.).fitted(20., 40.), [10.; 4]);
        assert_eq!(
            CornerRadii::new(30., 10., 0., 0.).fitted(20., 100.),
            [10., 5., 0., 0.]
        );
    }
    #[test]
    fn invalid_geometry_and_colors_are_rejected() {
        let rect = Rect::new(0., 0., 10., 10.);
        assert!(
            ShapeDraw::rounded_rect(rect, -1., [1.; 4])
                .validate()
                .is_err()
        );
        assert!(ShapeDraw::rect(rect, [1., 1., 1., 2.]).validate().is_err());
        assert!(
            ShapeDraw::ellipse(rect, [4., 2., 1., 0.5])
                .validate()
                .is_ok()
        );
        assert!(
            LineDraw::new([f32::NAN, 0.], [0., 0.], [1.; 4])
                .validate()
                .is_err()
        );
        assert!(
            LineDraw::new([0.; 2], [1.; 2], [1.; 4])
                .width(-1.)
                .validate()
                .is_err()
        );
        assert!(
            LineDraw::new([0.; 2], [1.; 2], [1.; 4])
                .transform([f32::INFINITY; 6])
                .validate()
                .is_err()
        );
    }
}
