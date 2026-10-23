use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
    StrokeVertex, VertexBuffers, math::point,
};

use super::{FillRule, LineJoin, Path, PathOptions};
use crate::{Error, LineCap, Rect};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct Vertex {
    pub position: [f32; 2],
    pub incoming: [f32; 2],
    pub outgoing: [f32; 2],
    pub outer: f32,
    pub padding: f32,
}

#[derive(Default)]
pub(super) struct Tessellators {
    fill: FillTessellator,
    stroke: StrokeTessellator,
    scratch: Scratch,
}

// Working storage reused across preparations; only capacity survives a call.
#[derive(Default)]
struct Scratch {
    triangles: VertexBuffers<[f32; 2], u32>,
    positions: Vec<[f32; 2]>,
    remap: Vec<u32>,
    ids: HashMap<[u32; 2], u32>,
    edges: Vec<((u32, u32), i32)>,
    outgoing: Vec<Vec<u32>>,
    incoming: Vec<Vec<u32>>,
    boundary: Vec<(u32, u32)>,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}
impl std::fmt::Debug for Tessellators {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tessellators").finish_non_exhaustive()
    }
}
pub(super) struct Geometry {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub interior_indices: u32,
    pub bounds: Option<Rect>,
    pub coordinate_extent: [f64; 2],
}
fn failed(message: impl ToString) -> Error {
    Error::PathTessellation {
        message: message.to_string(),
    }
}
fn fill_options(options: PathOptions) -> FillOptions {
    FillOptions::default()
        .with_tolerance(options.tolerance)
        .with_fill_rule(match options.fill_rule {
            FillRule::NonZero => lyon_tessellation::FillRule::NonZero,
            FillRule::EvenOdd => lyon_tessellation::FillRule::EvenOdd,
        })
}
fn cap(cap: LineCap) -> lyon_tessellation::LineCap {
    match cap {
        LineCap::Butt => lyon_tessellation::LineCap::Butt,
        LineCap::Square => lyon_tessellation::LineCap::Square,
        LineCap::Round => lyon_tessellation::LineCap::Round,
    }
}

impl Tessellators {
    pub fn prepare(&mut self, path: &Path, options: PathOptions) -> Result<Geometry, Error> {
        options.validate()?;
        let scratch = &mut self.scratch;
        scratch.triangles.vertices.clear();
        scratch.triangles.indices.clear();
        if path.is_empty() || options.stroke.is_some_and(|stroke| stroke.width == 0.) {
            return fringe(scratch);
        }
        if let Some(stroke) = options.stroke {
            let mut settings = StrokeOptions::default()
                .with_tolerance(options.tolerance)
                .with_line_width(stroke.width)
                .with_start_cap(cap(stroke.start_cap))
                .with_end_cap(cap(stroke.end_cap))
                .with_line_join(match stroke.join {
                    LineJoin::Miter => lyon_tessellation::LineJoin::Miter,
                    LineJoin::Bevel => lyon_tessellation::LineJoin::Bevel,
                    LineJoin::Round => lyon_tessellation::LineJoin::Round,
                });
            // Lyon 1.0.22 tests |normal|^2 against 4 * limit^2, measuring
            // center-to-tip extension relative to the full stroke width. Our
            // public SVG-style limit measures it relative to half-width. Set the
            // public field directly: Lyon's convenience setter rejects values
            // below one, but its tessellator accepts the translated >= 0.5 range.
            // The limit-one/right-angle pixel test guards this version-specific
            // conversion when Cargo.lock moves to another Lyon release.
            settings.miter_limit = stroke.miter_limit * 0.5;
            self.stroke
                .tessellate_path(
                    path.inner.as_ref(),
                    &settings,
                    &mut BuffersBuilder::new(&mut scratch.triangles, |v: StrokeVertex<'_, '_>| {
                        v.position().to_array()
                    }),
                )
                .map_err(failed)?;
            // Stroke tessellation can overlap at crossings and tight joins. Fill
            // the union of its consistently oriented triangles before shading so
            // translucent strokes blend once, not once per overlapping segment.
            let union = stroke_outline(scratch)?;
            scratch.triangles.vertices.clear();
            scratch.triangles.indices.clear();
            self.fill
                .tessellate_path(
                    &union,
                    &fill_options(PathOptions {
                        fill_rule: FillRule::NonZero,
                        ..options
                    }),
                    &mut BuffersBuilder::new(&mut scratch.triangles, |v: FillVertex<'_>| {
                        v.position().to_array()
                    }),
                )
                .map_err(failed)?;
        } else {
            self.fill
                .tessellate_path(
                    path.inner.as_ref(),
                    &fill_options(options),
                    &mut BuffersBuilder::new(&mut scratch.triangles, |v: FillVertex<'_>| {
                        v.position().to_array()
                    }),
                )
                .map_err(failed)?;
        }
        fringe(scratch)
    }
    /// Returns a prepared geometry's storage for the next preparation.
    pub fn recycle(&mut self, geometry: Geometry) {
        let scratch = &mut self.scratch;
        if geometry.vertices.capacity() > scratch.vertices.capacity() {
            scratch.vertices = geometry.vertices;
        }
        if geometry.indices.capacity() > scratch.indices.capacity() {
            scratch.indices = geometry.indices;
        }
    }
}

// Sums signed counts per edge in key order, dropping the input order.
fn sum_edges(edges: &mut Vec<((u32, u32), i32)>) {
    edges.sort_unstable_by_key(|(edge, _)| *edge);
    let mut write = 0;
    for read in 0..edges.len() {
        if write > 0 && edges[write - 1].0 == edges[read].0 {
            edges[write - 1].1 += edges[read].1;
        } else {
            edges[write] = edges[read];
            write += 1;
        }
    }
    edges.truncate(write);
}

// Clears the first `count` adjacency lists, keeping their capacity.
fn reset_lists(lists: &mut Vec<Vec<u32>>, count: usize) {
    if lists.len() < count {
        lists.resize_with(count, Vec::new);
    }
    for list in &mut lists[..count] {
        list.clear();
    }
}

fn stroke_outline(scratch: &mut Scratch) -> Result<lyon_tessellation::path::Path, Error> {
    weld_positions(scratch)?;
    let Scratch {
        triangles,
        positions,
        remap,
        edges,
        outgoing,
        ..
    } = scratch;
    edges.clear();
    for triangle in triangles.indices.as_chunks::<3>().0 {
        let [a, mut b, mut c] = triangle.map(|i| remap[i as usize]);
        let area = cross(
            positions[a as usize],
            positions[b as usize],
            positions[c as usize],
        );
        if !area.is_finite() {
            return Err(failed("nonfinite stroke geometry"));
        }
        if area == 0. {
            continue;
        }
        if area < 0. {
            std::mem::swap(&mut b, &mut c);
        }
        for (a, b) in [(a, b), (b, c), (c, a)] {
            edges.push(if a < b { ((a, b), 1) } else { ((b, a), -1) });
        }
    }
    sum_edges(edges);
    // Removing shared edges preserves the winding sum of all positive triangles,
    // but avoids submitting every internal diagonal as a fill intersection. The
    // remaining directed graph is balanced, so it decomposes into closed walks.
    // Walk orientation/overlap is left intact; the fill tessellator computes union.
    reset_lists(outgoing, positions.len());
    for &((a, b), count) in edges.iter() {
        let (a, b) = if count > 0 { (a, b) } else { (b, a) };
        outgoing[a as usize].extend(std::iter::repeat_n(b, count.unsigned_abs() as usize));
    }
    let mut path = lyon_tessellation::path::Path::builder();
    // Walks start at the lowest vertex that still has an unused edge.
    for start in 0..positions.len() as u32 {
        while let Some(mut current) = outgoing[start as usize].pop() {
            let p = positions[start as usize];
            path.begin(point(p[0], p[1]));
            while current != start {
                let p = positions[current as usize];
                path.line_to(point(p[0], p[1]));
                current = outgoing[current as usize]
                    .pop()
                    .ok_or_else(|| failed("open stroke outline"))?;
            }
            path.end(true);
        }
    }
    Ok(path.build())
}

// Welds exactly equal triangle positions into `positions`, mapping each input
// vertex through `remap`.
fn weld_positions(scratch: &mut Scratch) -> Result<(), Error> {
    let Scratch {
        triangles,
        positions,
        remap,
        ids,
        ..
    } = scratch;
    positions.clear();
    remap.clear();
    ids.clear();
    for &position in &triangles.vertices {
        if !position.into_iter().all(f32::is_finite) {
            return Err(failed("nonfinite tessellation geometry"));
        }
        let key = position.map(|v| if v == 0. { 0 } else { v.to_bits() });
        let next = u32::try_from(positions.len()).map_err(|_| Error::PathTooLarge)?;
        let index = *ids.entry(key).or_insert_with(|| {
            positions.push(position);
            next
        });
        remap.push(index);
    }
    Ok(())
}
fn cross(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f64 {
    (f64::from(b[0]) - f64::from(a[0])) * (f64::from(c[1]) - f64::from(a[1]))
        - (f64::from(b[1]) - f64::from(a[1])) * (f64::from(c[0]) - f64::from(a[0]))
}
fn direction(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    let x = f64::from(b[0]) - f64::from(a[0]);
    let y = f64::from(b[1]) - f64::from(a[1]);
    let length = x.hypot(y);
    [(x / length) as f32, (y / length) as f32]
}

fn fringe(scratch: &mut Scratch) -> Result<Geometry, Error> {
    // Exact welding removes duplicated tessellator vertices, including -0.0.
    weld_positions(scratch)?;
    let Scratch {
        triangles,
        positions,
        remap,
        edges,
        outgoing,
        incoming,
        boundary,
        vertices,
        indices,
        ..
    } = scratch;
    vertices.clear();
    vertices.extend(positions.iter().map(|&position| Vertex {
        position,
        incoming: [0.; 2],
        outgoing: [0.; 2],
        outer: 0.,
        padding: 0.,
    }));
    indices.clear();
    edges.clear();
    for triangle in triangles.indices.as_chunks::<3>().0 {
        let [a, mut b, mut c] = [
            remap[triangle[0] as usize],
            remap[triangle[1] as usize],
            remap[triangle[2] as usize],
        ];
        let area = cross(
            vertices[a as usize].position,
            vertices[b as usize].position,
            vertices[c as usize].position,
        );
        if area == 0. {
            continue;
        }
        if area < 0. {
            std::mem::swap(&mut b, &mut c);
        }
        indices.extend([a, b, c]);
        for (a, b) in [(a, b), (b, c), (c, a)] {
            edges.push(if a < b { ((a, b), 1) } else { ((b, a), -1) });
        }
    }
    sum_edges(edges);
    let interior_indices = u32::try_from(indices.len()).map_err(|_| Error::PathTooLarge)?;
    if indices.is_empty() {
        return Ok(Geometry {
            vertices: Vec::new(),
            indices: Vec::new(),
            interior_indices: 0,
            bounds: None,
            coordinate_extent: [0.; 2],
        });
    }
    let mut minimum = [f32::INFINITY; 2];
    let mut maximum = [f32::NEG_INFINITY; 2];
    for &index in indices.iter() {
        let p = vertices[index as usize].position;
        for axis in 0..2 {
            minimum[axis] = minimum[axis].min(p[axis]);
            maximum[axis] = maximum[axis].max(p[axis]);
        }
    }
    let bounds = Rect::new(
        minimum[0],
        minimum[1],
        maximum[0] - minimum[0],
        maximum[1] - minimum[1],
    );
    if !bounds.valid() {
        return Err(failed("path bounds overflow"));
    }
    boundary.clear();
    reset_lists(outgoing, vertices.len());
    reset_lists(incoming, vertices.len());
    for &((a, b), count) in edges.iter() {
        if count == 0 {
            continue;
        }
        if count.abs() != 1 {
            return Err(failed("overlapping tessellation boundary"));
        }
        let (a, b) = if count > 0 { (a, b) } else { (b, a) };
        boundary.push((a, b));
        outgoing[a as usize].push(b);
        incoming[b as usize].push(a);
    }
    // Pair edges around touching contours by following the interior on the left.
    let turn = |incoming: [f32; 2], outgoing: [f32; 2]| {
        (f64::from(-incoming[1]).atan2(f64::from(-incoming[0]))
            - f64::from(outgoing[1]).atan2(f64::from(outgoing[0])))
        .rem_euclid(std::f64::consts::TAU)
    };
    for &(a, b) in boundary.iter() {
        let p = vertices[a as usize].position;
        let q = vertices[b as usize].position;
        let edge = direction(p, q);
        let previous = incoming[a as usize]
            .iter()
            .map(|&i| direction(vertices[i as usize].position, p))
            .min_by(|x, y| turn(*x, edge).total_cmp(&turn(*y, edge)))
            .ok_or_else(|| failed("open tessellation boundary"))?;
        let next = outgoing[b as usize]
            .iter()
            .map(|&i| direction(q, vertices[i as usize].position))
            .min_by(|x, y| turn(edge, *x).total_cmp(&turn(edge, *y)))
            .ok_or_else(|| failed("open tessellation boundary"))?;
        vertices[a as usize].incoming = previous;
        vertices[a as usize].outgoing = edge;
        vertices[b as usize].incoming = edge;
        vertices[b as usize].outgoing = next;
        let outer_a = u32::try_from(vertices.len()).map_err(|_| Error::PathTooLarge)?;
        vertices.push(Vertex {
            position: p,
            incoming: previous,
            outgoing: edge,
            outer: 1.,
            padding: 0.,
        });
        let outer_b = u32::try_from(vertices.len()).map_err(|_| Error::PathTooLarge)?;
        vertices.push(Vertex {
            position: q,
            incoming: edge,
            outgoing: next,
            outer: 1.,
            padding: 0.,
        });
        indices.extend([a, outer_a, b, b, outer_a, outer_b]);
    }
    if indices.len() > u32::MAX as usize {
        return Err(Error::PathTooLarge);
    }
    Ok(Geometry {
        vertices: std::mem::take(vertices),
        indices: std::mem::take(indices),
        interior_indices,
        bounds: Some(bounds),
        coordinate_extent: [
            f64::from(minimum[0]).abs().max(f64::from(maximum[0]).abs()),
            f64::from(minimum[1]).abs().max(f64::from(maximum[1]).abs()),
        ],
    })
}
