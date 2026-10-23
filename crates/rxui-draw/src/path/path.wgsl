struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) coverage: f32,
    @location(2) clip: vec4<f32>,
    @location(3) @interpolate(flat) clip_radii: vec4<f32>,
};

fn safe_unit(v: vec2<f32>) -> vec2<f32> {
    let scale = max(abs(v.x), abs(v.y));
    if scale == 0.0 { return vec2(0.0); }
    return normalize(v / scale);
}


fn path_vertex(
    position: vec2<f32>,
    incoming: vec2<f32>,
    outgoing: vec2<f32>,
    outer: f32,
    axes: vec4<f32>,
    translation_viewport: vec4<f32>,
    color: vec4<f32>,
    style: vec4<f32>,
    clip_axes: vec4<f32>,
    clip_offset_half: vec4<f32>,
    clip_radii: vec4<f32>,
) -> Output {
    let matrix = mat2x2(axes.xy, axes.zw);
    var pixel = matrix * position + translation_viewport.xy;
    if style.x > 0.0 {
        let a = safe_unit(matrix * incoming);
        let b = safe_unit(matrix * outgoing);
        let n1 = vec2(a.y, -a.x) * style.y;
        let n2 = vec2(b.y, -b.x) * style.y;
        let miter = safe_unit(n1 + n2);
        let projection = max(dot(miter, n2), 0.125);
        // Center the coverage band on the geometric boundary. Interior boundary
        // vertices move half a pixel inward; outer vertices move half outward.
        pixel += miter / projection * (outer - 0.5);
    }
    let normalized = pixel / translation_viewport.zw;
    var output: Output;
    output.position = vec4(normalized.x * 2.0 - 1.0, 1.0 - normalized.y * 2.0, 0.0, 1.0);
    output.color = color;
    output.coverage = 1.0 - outer * style.x;
    output.clip = clip_varying(normalized, clip_axes, clip_offset_half);
    output.clip_radii = clip_radii;
    return output;
}

fn shade(input: Output) -> vec4<f32> {
    let clip = rounded_clip_coverage(input.position.xy);
    let alpha = input.color.a * clamp(input.coverage, 0.0, 1.0) * clip;
    return vec4(input.color.rgb * alpha, alpha);
}
@fragment
fn fragment_main(input: Output) -> @location(0) vec4<f32> {
    return shade(input);
}
@fragment
fn fragment_covered(input: Output) -> @location(0) vec4<f32> {
    let color = shade(input);
    if color.a == 0.0 { discard; }
    return color;
}

@vertex
fn vertex_main(
    @location(0) position: vec2<f32>,
    @location(1) incoming: vec2<f32>,
    @location(2) outgoing: vec2<f32>,
    @location(3) outer: f32,
    @location(4) axes: vec4<f32>,
    @location(5) translation_viewport: vec4<f32>,
    @location(6) color: vec4<f32>,
    @location(7) style: vec4<f32>,
    @location(8) clip_axes: vec4<f32>,
    @location(9) clip_offset_half: vec4<f32>,
    @location(10) clip_radii: vec4<f32>,
) -> Output {
    return path_vertex(position,incoming,outgoing,outer,axes,translation_viewport,color,style,
        clip_axes,clip_offset_half,clip_radii);
}
