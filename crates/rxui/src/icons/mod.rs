//! Vector icons: a curated Lucide subset and application icons from SVG path data,
//! painted in the element's text color.
//!
//! The built-in icons come from [Lucide](https://lucide.dev), ISC licensed (some
//! derived from Feather, MIT licensed); the license text is in
//! `crates/rxui/src/icons/LICENSE-lucide.txt` next to the data.
use std::sync::{Arc, OnceLock};

use crate::{Element, standard::Part};

#[rustfmt::skip]
mod lucide;
mod svg_path;
#[cfg(test)]
mod tests;

use lucide::{Builtin, DATA};
pub use svg_path::IconError;
use svg_path::Segment;

/// Gap between an icon and its caption in [`Element::leading_icon`] rows.
pub(crate) const ICON_GAP: f32 = 6.;

/// How an icon's geometry is painted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum IconPaint {
    /// A centered stroke one twelfth of the view box wide (2 units in a 24-unit
    /// box), with round caps and joins, as Lucide draws.
    #[default]
    Stroke,
    /// A nonzero-winding fill of the closed contours.
    Fill,
}

/// A vector icon: shared path geometry in a view box and a paint mode.
///
/// Cloning is cheap. Icons compare by value, so a view rebuilding the same icon
/// does not invalidate layout or painting: built-in icons compare by identity and
/// icons from SVG path data by their parsed geometry. An icon from a prebuilt
/// drawing path equals only its clones.
///
/// Built-in icons are associated constants, such as [`Icon::SEARCH`], parsed once
/// on first use. They are stroke icons in a 24×24 view box from Lucide 1.61.0;
/// [`Icon::built_in`] lists them.
///
/// ```
/// # #[cfg(feature = "layout")] {
/// use rxui::{Icon, icon, icon_button, prelude::*};
/// let decorative = icon(Icon::SEARCH);
/// let meaningful = icon(Icon::CIRCLE_ALERT).accessibility_label("Warning");
/// let plus = Icon::from_svg_path([24., 24.], "M5 12h14M12 5v14").unwrap();
/// let action = icon_button(plus).accessibility_label("New file");
/// # }
/// ```
#[derive(Clone, PartialEq)]
pub struct Icon {
    source: Source,
    paint: IconPaint,
}
#[derive(Clone)]
enum Source {
    Builtin(Builtin),
    Custom(Arc<Geometry>),
}
impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Builtin(a), Self::Builtin(b)) => a == b,
            (Self::Custom(a), Self::Custom(b)) => Arc::ptr_eq(a, b) || a == b,
            _ => false,
        }
    }
}

/// Parsed icon geometry in view box units.
#[derive(Debug)]
struct Geometry {
    view_box: [f32; 2],
    shape: Shape,
}
#[derive(Debug)]
enum Shape {
    Segments(Vec<Segment>),
    #[cfg(feature = "rendering")]
    Path(rxui_draw::Path),
}
impl PartialEq for Geometry {
    fn eq(&self, other: &Self) -> bool {
        self.view_box == other.view_box
            && match (&self.shape, &other.shape) {
                (Shape::Segments(a), Shape::Segments(b)) => a == b,
                // Prebuilt paths are opaque; only clones of one icon are equal.
                #[cfg(feature = "rendering")]
                _ => false,
            }
    }
}

impl std::fmt::Debug for Icon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("Icon");
        match &self.source {
            Source::Builtin(b) => s.field("name", &DATA[*b as usize].0),
            Source::Custom(g) => s.field("view_box", &g.view_box),
        };
        s.field("paint", &self.paint).finish()
    }
}

impl Icon {
    const fn builtin(builtin: Builtin) -> Self {
        Self {
            source: Source::Builtin(builtin),
            paint: IconPaint::Stroke,
        }
    }
    /// An icon from SVG path data (a `<path>` element's `d` attribute) in a view
    /// box of `[width, height]` units with its origin at the top left. It strokes
    /// by default; see [`Self::fill`].
    ///
    /// Supports the `M L H V C S Q T A Z` commands in absolute and relative forms,
    /// implicitly repeated arguments and compact numbers such as `1.5.5` or `-1-2`.
    /// Arcs become cubic Béziers. Full SVG documents are out of scope: convert
    /// other shapes to path data first.
    ///
    /// # Errors
    ///
    /// Malformed path data, or a view box that is not finite and positive.
    pub fn from_svg_path(view_box: [f32; 2], d: &str) -> Result<Self, IconError> {
        valid_view_box(view_box)?;
        Ok(Self::custom(view_box, Shape::Segments(svg_path::parse(d)?)))
    }
    /// An icon from a prebuilt drawing path in a view box of `[width, height]`
    /// units. It equals only its own clones.
    ///
    /// # Errors
    ///
    /// A view box that is not finite and positive.
    #[cfg(feature = "rendering")]
    pub fn from_path(
        view_box: [f32; 2],
        path: rxui_draw::Path,
        paint: IconPaint,
    ) -> Result<Self, IconError> {
        valid_view_box(view_box)?;
        Ok(Self {
            paint,
            ..Self::custom(view_box, Shape::Path(path))
        })
    }
    fn custom(view_box: [f32; 2], shape: Shape) -> Self {
        Self {
            source: Source::Custom(Arc::new(Geometry { view_box, shape })),
            paint: IconPaint::Stroke,
        }
    }
    /// Strokes the outline, the default.
    pub fn stroke(mut self) -> Self {
        self.paint = IconPaint::Stroke;
        self
    }
    /// Fills the closed contours instead of stroking them.
    pub fn fill(mut self) -> Self {
        self.paint = IconPaint::Fill;
        self
    }
    /// The paint mode.
    pub fn paint(&self) -> IconPaint {
        self.paint
    }
    /// View box width and height.
    pub fn view_box(&self) -> [f32; 2] {
        self.geometry().view_box
    }
    /// Lucide name of a built-in icon, such as `"chevron-down"`.
    pub fn name(&self) -> Option<&'static str> {
        match self.source {
            Source::Builtin(b) => Some(DATA[b as usize].0),
            Source::Custom(_) => None,
        }
    }
    /// Every built-in icon, in documentation order.
    pub fn built_in() -> impl ExactSizeIterator<Item = Self> {
        Builtin::ALL.into_iter().map(Self::builtin)
    }
    fn geometry(&self) -> &Geometry {
        match &self.source {
            Source::Builtin(b) => builtin_geometry(*b),
            Source::Custom(g) => g,
        }
    }
    /// Extent of the drawn segments as `[min_x, min_y, max_x, max_y]` in view box
    /// units, excluding stroke width; None for a prebuilt path or empty data.
    #[cfg(test)]
    pub(crate) fn bounds(&self) -> Option<[f32; 4]> {
        match &self.geometry().shape {
            Shape::Segments(s) => svg_path::bounds(s),
            #[cfg(feature = "rendering")]
            Shape::Path(_) => None,
        }
    }
    /// Whether two icons share geometry storage, for testing cheap clones.
    #[cfg(test)]
    pub(crate) fn shares_geometry(&self, other: &Self) -> bool {
        std::ptr::eq(self.geometry(), other.geometry())
    }
}

fn valid_view_box(view_box: [f32; 2]) -> Result<(), IconError> {
    if view_box.iter().all(|v| v.is_finite() && *v > 0.) {
        Ok(())
    } else {
        Err(IconError::new(0, "view box must be finite and positive"))
    }
}

/// Built-in geometry, each icon parsed on first use.
fn builtin_geometry(builtin: Builtin) -> &'static Geometry {
    static CACHE: [OnceLock<Geometry>; DATA.len()] = [const { OnceLock::new() }; DATA.len()];
    CACHE[builtin as usize].get_or_init(|| {
        let (name, shapes) = DATA[builtin as usize];
        let mut segments = Vec::new();
        for d in shapes {
            let parsed = svg_path::parse(d)
                .unwrap_or_else(|e| panic!("built-in icon {name} is valid SVG path data: {e}"));
            segments.extend(parsed);
        }
        Geometry {
            view_box: [24., 24.],
            shape: Shape::Segments(segments),
        }
    })
}

/// The custom element painting an icon; see [`icon`].
#[derive(PartialEq)]
pub(crate) struct IconMark(Icon);

/// Tessellated geometry retained per placement, rebuilt when the icon or the
/// approximation tolerance its painted scale needs changes.
#[cfg(feature = "rendering")]
#[derive(Default)]
pub(crate) struct Prepared {
    key: Option<(Icon, f32)>,
    path: Option<rxui_draw::PreparedPath>,
}

impl IconMark {
    /// Uniform view box scale and offset that center the icon in `b`.
    #[cfg(feature = "rendering")]
    fn placement(&self, b: crate::Bounds) -> (f32, [f32; 2]) {
        let [w, h] = self.0.view_box();
        let scale = (b.width / w).min(b.height / h).max(0.);
        let offset = [
            b.x + (b.width - w * scale) / 2.,
            b.y + (b.height - h * scale) / 2.,
        ];
        (scale, offset)
    }
}

impl crate::CustomElement for IconMark {
    #[cfg(feature = "rendering")]
    type State = Prepared;
    #[cfg(not(feature = "rendering"))]
    type State = ();
    fn measure_eq(&self, _: &Self) -> bool {
        // Icons use the empty intrinsic measurement; their theme part sizes them.
        true
    }
    #[cfg(feature = "rendering")]
    fn prepare(
        &self,
        state: &mut Prepared,
        element: &crate::ElementInfo<'_>,
        cx: &mut crate::CustomPrepare<'_>,
    ) -> Result<(), crate::UiError> {
        use rxui_draw::{LineCap, LineJoin, PathOptions, PathStroke};
        let (scale, _) = self.placement(element.content_bounds);
        // Curve error of about a fifth of a physical pixel, in power-of-two
        // steps so small size changes reuse the geometry.
        let pixels = (scale * cx.raster_scale).max(1e-3);
        let tolerance = (0.2 / pixels).clamp(1. / 256., 0.25).log2().floor().exp2();
        if state
            .key
            .as_ref()
            .is_some_and(|(icon, t)| *icon == self.0 && *t == tolerance)
        {
            return Ok(());
        }
        let geometry = self.0.geometry();
        let built;
        let path = match &geometry.shape {
            Shape::Segments(segments) => {
                built = to_path(segments)?;
                &built
            }
            Shape::Path(path) => path,
        };
        let options = PathOptions::new().tolerance(tolerance);
        let options = match self.0.paint {
            IconPaint::Fill => options,
            IconPaint::Stroke => {
                let [w, h] = geometry.view_box;
                options.stroke(
                    PathStroke::new(w.min(h) / 12.)
                        .cap(LineCap::Round)
                        .join(LineJoin::Round),
                )
            }
        };
        state.path = Some(cx.painter.prepare_path(path, options)?);
        state.key = Some((self.0.clone(), tolerance));
        Ok(())
    }
    #[cfg(feature = "rendering")]
    fn paint(
        &self,
        state: &Prepared,
        element: &crate::ElementInfo<'_>,
        paint: &mut rxui_draw::PaintSession<'_, '_>,
    ) -> Result<(), crate::UiError> {
        let Some(path) = state.path.as_ref().filter(|p| !p.is_empty()) else {
            return Ok(());
        };
        let (scale, [x, y]) = self.placement(element.content_bounds);
        if scale <= 0. {
            return Ok(());
        }
        let transform = rxui_draw::Transform2D::scale(scale, scale)
            .then(rxui_draw::Transform2D::translation(x, y));
        paint.draw_path(
            path,
            rxui_draw::PathDraw::new(element.color).transform(transform),
        )?;
        Ok(())
    }
}

/// Builds a drawing path from parsed segments, which always open contours with a move.
#[cfg(feature = "rendering")]
fn to_path(segments: &[Segment]) -> Result<rxui_draw::Path, rxui_draw::Error> {
    let mut builder = rxui_draw::Path::builder();
    for segment in segments {
        match *segment {
            Segment::Move(p) => builder.move_to(p),
            Segment::Line(p) => builder.line_to(p),
            Segment::Quad(c, p) => builder.quadratic_to(c, p),
            Segment::Cubic(c1, c2, p) => builder.cubic_to(c1, c2, p),
            Segment::Close => builder.close(),
        };
    }
    builder.build()
}

/// An icon leaf painted in its resolved text color, so it follows hover, pressed
/// and disabled states and theme switches like text does.
///
/// It is [`crate::ThemeMetrics::icon_size`] square (16 balanced, 14 compact),
/// resolved from the nearest theme so density switches resize it; an explicit
/// [`Element::size`], width or height wins. The icon scales uniformly to fit and
/// centers in its content box, and its stroke width scales with it.
///
/// Icons are decorative by default and hidden from assistive technology. An
/// [`Element::accessibility_label`] makes the icon an
/// [`crate::SemanticRole::Image`] with that name.
///
/// ```
/// use rxui::{Icon, icon, prelude::*};
/// let status = row().gap(6.)
///     .child(icon(Icon::CIRCLE_CHECK).color(ThemeColor::Success).accessibility_label("Passed"))
///     .child(label("All tests passed"));
/// ```
pub fn icon(icon: Icon) -> Element {
    let mut element = crate::custom(IconMark(icon))
        .accessibility_role(crate::SemanticRole::Image)
        .accessibility_hidden(true);
    element.surface = crate::element::Surface::Part(Part::Icon);
    element
}

/// An empty box the size of an icon, reserving an icon's column in aligned rows.
pub(crate) fn icon_space() -> Element {
    crate::standard::part(crate::row(), Part::Icon)
}

/// A square, quiet button showing only an icon: as tall and wide as a text input
/// ([`crate::ThemeMetrics::input_height`]), with the icon centered.
///
/// It must have an accessible name: preparation fails with
/// [`crate::UiError::MissingAccessibleName`] without an
/// [`Element::accessibility_label`] or [`Element::accessibility_labelled_by`].
///
/// ```
/// use rxui::{Icon, icon_button, prelude::*};
/// let search = icon_button(Icon::SEARCH).accessibility_label("Search");
/// ```
pub fn icon_button(icon: Icon) -> Element {
    crate::standard::part(
        crate::button(self::icon(icon))
            .variant(crate::ButtonVariant::Quiet)
            .padding(0.)
            .align_items(taffy::AlignItems::CENTER)
            .justify_content(taffy::JustifyContent::CENTER),
        Part::IconButton,
    )
}

impl Element {
    /// Adds `icon` before this element's caption, laying the content out as a
    /// centered row with a 6-unit gap. On a button the caption becomes a label
    /// child that keeps naming it; a label becomes a row of icon and label.
    ///
    /// ```
    /// use rxui::{Icon, prelude::*};
    /// let run = button("Run").leading_icon(Icon::PLAY).variant(ButtonVariant::Primary);
    /// let more = button("More").trailing_icon(Icon::CHEVRON_DOWN);
    /// ```
    pub fn leading_icon(self, icon: Icon) -> Self {
        self.with_icon(icon, true)
    }
    /// Adds `icon` after this element's caption; see [`Self::leading_icon`].
    pub fn trailing_icon(self, icon: Icon) -> Self {
        self.with_icon(icon, false)
    }
    fn with_icon(mut self, icon: Icon, leading: bool) -> Self {
        use crate::element::ElementKind;
        let mark = self::icon(icon);
        if matches!(self.kind, ElementKind::Label(_) | ElementKind::RichText(_)) {
            let key = self.key.take();
            let row = crate::row()
                .gap(ICON_GAP)
                .align_items(taffy::AlignItems::CENTER);
            let row = if leading {
                row.child(mark).child(self)
            } else {
                row.child(self).child(mark)
            };
            return Self { key, ..row };
        }
        if let ElementKind::Button { text, .. } = &mut self.kind
            && let Some(text) = text.take()
        {
            self.children.push(crate::label(text));
        }
        if leading {
            self.children.insert(0, mark);
        } else {
            self.children.push(mark);
        }
        self.style.flex_direction = taffy::FlexDirection::Row;
        self.align_items(taffy::AlignItems::CENTER)
            .justify_content(taffy::JustifyContent::CENTER)
            .gap(ICON_GAP)
    }
    /// Whether this element is an [`icon`] leaf.
    pub(crate) fn is_icon(&self) -> bool {
        self.surface == crate::element::Surface::Part(Part::Icon)
            && matches!(self.kind, crate::element::ElementKind::Custom(_))
    }
}
