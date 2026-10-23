struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) geometry: vec4<f32>,
    @location(2) @interpolate(flat) style: vec2<f32>,
    @location(3) @interpolate(flat) color: vec4<f32>,
    @location(4) @interpolate(flat) radii: vec4<f32>,
    @location(5) clip: vec4<f32>,
    @location(6) @interpolate(flat) clip_radii: vec4<f32>,
};
 fn primitive_vertex(index: u32,
    origin_axis_x: vec4<f32>, axis_y_min: vec4<f32>,
    span_style: vec4<f32>, geometry: vec4<f32>,
    color: vec4<f32>, radii: vec4<f32>,
    clip_axes: vec4<f32>, clip_offset_half: vec4<f32>, clip_radii: vec4<f32>) -> Output {
    let corners = array(vec2(0.0, 0.0), vec2(0.0, 1.0), vec2(1.0, 0.0),
                        vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(1.0, 1.0));
    let local = axis_y_min.zw + corners[index] * span_style.xy;
    let position = origin_axis_x.xy + local.x * origin_axis_x.zw + local.y * axis_y_min.xy;
    var output: Output;
    output.position = vec4(position * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
    output.local = local;
    output.geometry = geometry;
    output.style = span_style.zw;
    output.color = color;
    output.radii = radii;
    output.clip = clip_varying(position, clip_axes, clip_offset_half);
    output.clip_radii = clip_radii;
    return output;
}
fn box_distance(p: vec2<f32>, half_size: vec2<f32>) -> f32 {
    let q = abs(p) - half_size;
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0);
}
// Radii are [top-left, top-right, bottom-right, bottom-left] with Y down.
fn rounded_box_distance(p: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let top = select(radii.x, radii.y, p.x > 0.0);
    let bottom = select(radii.w, radii.z, p.x > 0.0);
    let radius = select(top, bottom, p.y > 0.0);
    return box_distance(p, half_size - vec2(radius)) - radius;
}
// Analytic box shadow after Evan Wallace's "Fast Rounded Rectangle Shadows":
// the horizontal Gaussian integral is exact per row (via erf), and the vertical
// integral uses four midpoint rows within three standard deviations. Each row's
// extent follows the corner radii on its own side.
fn erf_approx(x: vec2<f32>) -> vec2<f32> {
    let s = sign(x);
    let a = abs(x);
    var r = 1.0 + (0.278393 + (0.230389 + 0.000972 * a + 0.078108 * a * a) * a) * a;
    r = r * r;
    return s - s / (r * r);
}
fn shadow_row(x: f32, y: f32, sigma: f32, half_size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let left_radius = select(radii.x, radii.w, y > 0.0);
    let right_radius = select(radii.y, radii.z, y > 0.0);
    let d = min(half_size.y - vec2(left_radius, right_radius) - abs(y), vec2(0.0));
    let left = half_size.x - left_radius + sqrt(max(0.0, left_radius * left_radius - d.x * d.x));
    let right = half_size.x - right_radius + sqrt(max(0.0, right_radius * right_radius - d.y * d.y));
    let integral = 0.5 + 0.5 * erf_approx((vec2(x) + vec2(left, -right)) * (0.70710678 / sigma));
    return integral.x - integral.y;
}
fn shadow_coverage(p: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>, sigma: f32) -> f32 {
    let start = clamp(-3.0 * sigma, p.y - half_size.y, p.y + half_size.y);
    let end = clamp(3.0 * sigma, p.y - half_size.y, p.y + half_size.y);
    let step = (end - start) * 0.25;
    var offset = start + step * 0.5;
    var value = 0.0;
    for (var i = 0; i < 4; i += 1) {
        let weight = exp(-offset * offset / (2.0 * sigma * sigma)) / (2.50662827 * sigma);
        value += shadow_row(p.x, p.y - offset, sigma, half_size, radii) * weight * step;
        offset += step;
    }
    return clamp(value, 0.0, 1.0);
}
// Closest-point distance to an ellipse, normalized by its major radius.
// Solve the monotone Lagrange-multiplier equation with a bounded bisection.
// See https://www.geometrictools.com/Documentation/DistancePointEllipseEllipsoid.pdf
// Axis/circle cases avoid the singular root. Used only for ellipse outlines:
// an implicit ellipse equation is not a distance and gives uneven stroke widths.
fn ellipse_distance(point: vec2<f32>, half_size: vec2<f32>) -> f32 {
    if half_size.x == half_size.y { return length(point) - half_size.x; }
    let swap = half_size.y > half_size.x;
    let axes = select(half_size, half_size.yx, swap);
    let p = abs(select(point, point.yx, swap)) / axes.x;
    let b = axes.y / axes.x;
    // A radius ratio below float representability approaches a segment.
    if b == 0.0 {
        return length(vec2(max(p.x - 1.0, 0.0), p.y)) * axes.x;
    }
    let b2 = b * b;
    var closest = vec2(0.0, b);
    if p.y == 0.0 {
        let denominator = 1.0 - b2;
        if p.x < denominator {
            let x = p.x / denominator;
            closest = vec2(x, b * sqrt(max(1.0 - x * x, 0.0)));
        } else {
            closest = vec2(1.0, 0.0);
        }
    } else if p.x > 0.0 {
        let inside = length(vec2(p.x, p.y / b)) < 1.0;
        // u = multiplier + b^2 avoids subtracting nearly equal numbers
        // near the minor-axis singularity. b*p.y is a valid lower bound.
        var lo = b * p.y;
        var hi = select(length(vec2(p.x, b * p.y)) + b2, b2, inside);
        // Near the major axis the root approaches zero. A tight upper bound
        // retains relative precision despite interpolated coordinates being
        // only approximately zero at an axis pixel.
        if inside && p.x < 1.0 - b2 {
            let major_ratio = p.x / (1.0 - b2);
            hi = min(hi, b * p.y / sqrt(max(1.0 - major_ratio * major_ratio, 0.0000001)));
        }
        for (var i = 0u; i < 24u; i += 1u) {
            let u = (lo + hi) * 0.5;
            let ratios = vec2(p.x / (1.0 - b2 + u), b * p.y / u);
            if dot(ratios, ratios) > 1.0 { lo = u; } else { hi = u; }
        }
        let u = (lo + hi) * 0.5;
        closest = vec2(p.x / (1.0 - b2 + u), b2 * p.y / u);
    }
    let distance = length(p - closest) * axes.x;
    return select(distance, -distance, length(vec2(p.x, p.y / b)) <= 1.0);
}
fn filtered_box(p: vec2<f32>, bounds: vec2<f32>, footprint: vec2<f32>) -> f32 {
    let positive = clamp((bounds - p) / footprint + vec2(0.5), vec2(0.0), vec2(1.0));
    let negative = clamp((-bounds - p) / footprint + vec2(0.5), vec2(0.0), vec2(1.0));
    let coverage = positive - negative;
    return coverage.x * coverage.y;
}
fn shade_fragment_main(input: Output) -> vec4<f32> {
    let half_size = input.geometry.xy;
    let kind = u32(input.style.x);
    let radius = input.style.y;
    var distance = box_distance(input.local, half_size);
    var inner_distance = 1.0;
    var bounds = half_size;
    var inner_bounds = vec2(0.0);
    var has_inner = false;
    if kind == 1u || kind == 9u {
        distance = rounded_box_distance(input.local, half_size, input.radii);
    } else if kind == 2u {
        distance = length(input.local / max(half_size, vec2(0.000001))) - 1.0;
    } else if kind == 4u {
        distance = box_distance(input.local, vec2(half_size.x + half_size.y, half_size.y));
    } else if kind == 5u {
        distance = length(vec2(max(abs(input.local.x) - half_size.x, 0.0), input.local.y)) - half_size.y;
    } else if kind == 6u || kind == 7u {
        let width = input.geometry.z;
        inner_bounds = max(half_size - vec2(width), vec2(0.0));
        has_inner = all(inner_bounds > vec2(0.0));
        inner_distance = box_distance(input.local, inner_bounds);
        if kind == 7u {
            distance = rounded_box_distance(input.local, half_size, input.radii);
            let inner_radii = min(max(input.radii - vec4(width), vec4(0.0)), vec4(min(inner_bounds.x, inner_bounds.y)));
            inner_distance = rounded_box_distance(input.local, inner_bounds, inner_radii);
        }
    } else if kind == 8u {
        let base_distance = ellipse_distance(input.local, half_size);
        let inset = input.geometry.z - radius;
        distance = base_distance - radius;
        inner_distance = base_distance + inset;
        bounds += vec2(radius);
        inner_bounds = max(half_size - vec2(inset), vec2(0.0));
        has_inner = all(inner_bounds > vec2(0.0));
    }
    // Derivatives run outside primitive-dependent control flow. Geometry's final
    // component selects analytic coverage independently of attachment MSAA.
    let edge_width = max(fwidth(distance), 0.000001);
    let inner_edge_width = max(fwidth(inner_distance), 0.000001);
    // Bound coverage by the filtered local box. Opposing fringes overlap for
    // subpixel geometry; a single distance threshold otherwise makes thin lines
    // too opaque. The box filter is approximate under rotation/shear, like fwidth.
    let footprint = max(fwidth(input.local), vec2(0.000001));
    if kind == 4u || kind == 5u { bounds.x += half_size.y; }
    let analytic = min(clamp(0.5 - distance / edge_width, 0.0, 1.0), filtered_box(input.local, bounds, footprint));
    let inner_analytic = min(clamp(0.5 - inner_distance / inner_edge_width, 0.0, 1.0), filtered_box(input.local, inner_bounds, footprint));
    let inner_coverage = select(0.0, inner_analytic, has_inner);
    let outlined = kind >= 6u && kind <= 8u;
    let hard = select(0.0, 1.0, distance <= 0.0 && !(outlined && has_inner && inner_distance <= 0.0));
    let filtered = max(analytic - select(0.0, inner_coverage, outlined), 0.0);
    var coverage = select(hard, filtered, input.geometry.w > 0.0);
    if kind == 9u && radius > 0.0 {
        coverage = shadow_coverage(input.local, half_size, input.radii, radius);
    }
    let alpha = input.color.a * coverage * rounded_clip_coverage(input.position.xy);
    return vec4(input.color.rgb * alpha, alpha);
}

@fragment fn fragment_main(input: Output) -> @location(0) vec4<f32> {
    return shade_fragment_main(input);
}
@fragment fn fragment_covered(input: Output) -> @location(0) vec4<f32> {
    let color = shade_fragment_main(input);
    if color.a <= 0.0 { discard; }
    return color;
}

@vertex fn vertex_main(@builtin(vertex_index) index: u32,
    @location(0) origin_axis_x: vec4<f32>, @location(1) axis_y_min: vec4<f32>,
    @location(2) span_style: vec4<f32>, @location(3) geometry: vec4<f32>,
    @location(4) color: vec4<f32>, @location(5) radii: vec4<f32>,
    @location(8) clip_axes: vec4<f32>, @location(9) clip_offset_half: vec4<f32>,
    @location(10) clip_radii: vec4<f32>) -> Output {
    return primitive_vertex(index,origin_axis_x,axis_y_min,span_style,geometry,color,radii,
        clip_axes,clip_offset_half,clip_radii);
}
