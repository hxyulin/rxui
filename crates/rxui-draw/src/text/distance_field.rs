//! Outline conversion and bounded CPU MTSDF generation, performed only during preparation.

use super::TextRenderError;
use bymsdfgen_core::{
    Bitmap,
    coloring::edge_coloring_simple,
    generator::{
        DistanceMapping, MsdfGeneratorConfig, Projection, SdfTransformation, generate_mtsdf,
    },
    geometry::{Contour, EdgeSegment as Segment, Shape},
    math::{Range, Vector2 as Point},
    raster::{FillRule, distance_sign_correction_multi},
};
use swash::{
    scale::{image::Image, outline::Outline},
    zeno::{Command, PathData, Placement},
};

pub(super) fn generate(
    outline: &Outline,
    pixels_per_em: u32,
    range_em: f32,
    page_size: u32,
) -> Result<Option<Image>, TextRenderError> {
    if outline.points().is_empty() {
        return Ok(None);
    }
    if outline
        .points()
        .iter()
        .any(|p| !p.x.is_finite() || !p.y.is_finite())
    {
        return Err(TextRenderError::InvalidRaster);
    }
    let bounds = outline.bounds();
    let range = f64::from(range_em) * f64::from(pixels_per_em);
    let padding = (range * 0.5).ceil() + 1.;
    // Swash outlines are Y-up. Generate in that space, then flip image rows for
    // the Y-down quad. Control-point bounds are conservative for curved outlines.
    let left = f64::from(bounds.min.x).floor() - padding;
    let bottom = f64::from(bounds.min.y).floor() - padding;
    let right = f64::from(bounds.max.x).ceil() + padding;
    let top = f64::from(bounds.max.y).ceil() + padding;
    let width = right - left;
    let height = top - bottom;
    if [left, bottom, right, top, width, height]
        .iter()
        .any(|v| !v.is_finite())
        || [left, top]
            .iter()
            .any(|&v| v < f64::from(i32::MIN) || v > f64::from(i32::MAX))
    {
        return Err(TextRenderError::InvalidRaster);
    }
    if width + 2. > f64::from(page_size) || height + 2. > f64::from(page_size) {
        return Err(TextRenderError::GlyphTooLarge);
    }
    let point = |p: swash::zeno::Point| Point::new(f64::from(p.x) - left, f64::from(p.y) - bottom);
    let mut shape = Shape::default();
    let mut contour = Contour::default();
    let mut current = Point::ZERO;
    let mut start = current;
    for command in outline.path().commands() {
        match command {
            Command::MoveTo(p) => {
                close_contour(&mut shape, &mut contour, current, start);
                current = point(p);
                start = current;
            }
            Command::LineTo(p) => {
                let next = point(p);
                if next != current {
                    contour.add_edge(Segment::line(current, next));
                }
                current = next;
            }
            Command::QuadTo(c, p) => {
                let next = point(p);
                if point(c) != current || next != current {
                    contour.add_edge(Segment::quadratic(current, point(c), next));
                }
                current = next;
            }
            Command::CurveTo(c1, c2, p) => {
                let next = point(p);
                if point(c1) != current || point(c2) != current || next != current {
                    contour.add_edge(Segment::cubic(current, point(c1), point(c2), next));
                }
                current = next;
            }
            Command::Close => {
                close_contour(&mut shape, &mut contour, current, start);
                current = start;
            }
        }
    }
    close_contour(&mut shape, &mut contour, current, start);
    if shape.contours.is_empty() {
        return Ok(None);
    }
    // Preserve nonzero contour topology, normalize convergent corners, and use
    // deterministic edge colors. Coordinates are already in generation texels.
    shape.normalize();
    edge_coloring_simple(&mut shape, 3., 0);
    let projection = Projection::default();
    let transformation = SdfTransformation::new(
        projection,
        DistanceMapping::from_range(Range::symmetric(range)),
    );
    let mut bitmap = Bitmap::<f32, 4>::new(width as usize, height as usize);
    generate_mtsdf(
        &mut bitmap,
        &shape,
        &transformation,
        &MsdfGeneratorConfig::default(),
    );
    distance_sign_correction_multi(&mut bitmap, &shape, &projection, 0.5, FillRule::NonZero);
    let mut data = Vec::with_capacity(width as usize * height as usize * 4);
    for y in (0..bitmap.height).rev() {
        for x in 0..bitmap.width {
            for &channel in bitmap.pixel(x, y) {
                // A channel with no contributing edge can use an infinite distance
                // sentinel; saturate it like any value outside the encoded range.
                if channel.is_nan() {
                    return Err(TextRenderError::InvalidRaster);
                }
                data.push((channel.clamp(0., 1.) * 255.).round() as u8);
            }
        }
    }
    Ok(Some(Image {
        placement: Placement {
            left: left as i32,
            top: top as i32,
            width: width as u32,
            height: height as u32,
        },
        data,
        // This is internal field data. The caller explicitly selects the atlas kind,
        // bypassing coverage/color normalization and treating alpha as distance.
        ..Default::default()
    }))
}

fn close_contour(shape: &mut Shape, contour: &mut Contour, current: Point, start: Point) {
    if contour.segments.is_empty() {
        return;
    }
    if current != start {
        contour.add_edge(Segment::line(current, start));
    }
    shape.contours.push(std::mem::take(contour));
}
