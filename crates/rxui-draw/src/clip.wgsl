// Rounded clip shared by built-in 2D shading. `axes` and `offset_half.xy` map a
// viewport-normalized position to clip-local units centered on the rectangle;
// `offset_half.zw` is the half size, negative when no clip is active.
fn clip_varying(position: vec2<f32>, axes: vec4<f32>, offset_half: vec4<f32>) -> vec4<f32> {
    let point = vec2(axes.x * position.x + axes.z * position.y,
                     axes.y * position.x + axes.w * position.y) + offset_half.xy;
    return vec4(point, offset_half.zw);
}
// Filtered coverage of the clip. Uses derivatives, so call it in uniform control flow.
// Radii are [top-left, top-right, bottom-right, bottom-left] with Y down.
fn clip_coverage(point_half: vec4<f32>, radii: vec4<f32>) -> f32 {
    let point = point_half.xy;
    let top = select(radii.x, radii.y, point.x > 0.0);
    let bottom = select(radii.w, radii.z, point.x > 0.0);
    let radius = select(top, bottom, point.y > 0.0);
    let q = abs(point) - max(point_half.zw, vec2(0.0)) + vec2(radius);
    let distance = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    let width = max(fwidth(distance), 0.000001);
    return select(clamp(0.5 - distance / width, 0.0, 1.0), 1.0, point_half.z < 0.0);
}
