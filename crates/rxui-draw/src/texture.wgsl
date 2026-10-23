@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
    @location(2) clip: vec4<f32>,
    @location(3) @interpolate(flat) clip_radii: vec4<f32>,
};
@vertex fn vertex_main(@builtin(vertex_index) index: u32,
    @location(0) origin_axis_x: vec4<f32>, @location(1) axis_y: vec4<f32>,
    @location(2) source: vec4<f32>, @location(3) tint: vec4<f32>,
    @location(7) clip_axes: vec4<f32>, @location(8) clip_offset_half: vec4<f32>,
    @location(9) clip_radii: vec4<f32>) -> Output {
    let corners = array(vec2(0.0, 0.0), vec2(0.0, 1.0), vec2(1.0, 0.0),
                        vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(1.0, 1.0));
    let corner = corners[index];
    let destination = origin_axis_x.xy + corner.x * origin_axis_x.zw + corner.y * axis_y.xy;
    var output: Output;
    output.position = vec4(destination * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
    output.uv = source.xy + corner * source.zw;
    output.tint = tint;
    output.clip = clip_varying(destination, clip_axes, clip_offset_half);
    output.clip_radii = clip_radii;
    return output;
}
// Retained instance positions stay immutable; the extra transform is per draw.
@vertex fn vertex_transformed(@builtin(vertex_index) index: u32,
    @location(0) origin_axis_x: vec4<f32>, @location(1) axis_y: vec4<f32>,
    @location(2) source: vec4<f32>, @location(3) tint: vec4<f32>,
    @location(4) pixels: vec4<f32>, @location(5) normalized: vec4<f32>,
    @location(6) translations: vec4<f32>,
    @location(7) clip_axes: vec4<f32>, @location(8) clip_offset_half: vec4<f32>,
    @location(9) clip_radii: vec4<f32>) -> Output {
    let corners = array(vec2(0.0, 0.0), vec2(0.0, 1.0), vec2(1.0, 0.0),
                        vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(1.0, 1.0));
    let corner = corners[index];
    let p = origin_axis_x.xy + corner.x * origin_axis_x.zw + corner.y * axis_y.xy;
    let m = select(pixels, normalized, axis_y.z != 0.0);
    let offset = select(translations.xy, translations.zw, axis_y.z != 0.0);
    let destination = vec2(m.x*p.x+m.z*p.y,m.y*p.x+m.w*p.y)+offset;
    var output: Output;
    output.position = vec4(destination * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
    output.uv = source.xy + corner * source.zw;
    output.tint = tint;
    output.clip = clip_varying(destination, clip_axes, clip_offset_half);
    output.clip_radii = clip_radii;
    return output;
}
fn tinted(color: vec4<f32>, input: Output) -> vec4<f32> {
    let tint = input.tint;
    let result = vec4(color.rgb * tint.rgb * tint.a, color.a * tint.a);
    return result * rounded_clip_coverage(input.position.xy);
}
fn shade_fragment_straight(input: Output) -> vec4<f32> {
    let color = textureSample(image, image_sampler, input.uv);
    return tinted(vec4(color.rgb * color.a, color.a), input);
}
// Linear filtering must interpolate premultiplied texels. Filtering straight RGB first
// blends the colour of transparent texels into edges as dark fringes, so gather the
// bilinear footprint of the first level, premultiply each texel, then weight it.
// Where the footprint's alpha is uniform (opaque or empty areas, most of a typical
// image) the two orders agree, so one filtered sample replaces the colour gathers.
fn shade_fragment_straight_linear(input: Output) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(image));
    let texel = input.uv * size - 0.5;
    let base = floor(texel);
    let f = texel - base;
    // Gathering at the footprint centre keeps the footprint consistent with `base`.
    let centre = (base + 1.0) / size;
    let a = textureGather(3, image, image_sampler, centre);
    let sampled = textureSampleLevel(image, image_sampler, input.uv, 0.0);
    var color = vec4(sampled.rgb * a.x, a.x);
    if any(a != a.xxxx) {
        let r = textureGather(0, image, image_sampler, centre);
        let g = textureGather(1, image, image_sampler, centre);
        let b = textureGather(2, image, image_sampler, centre);
        // Gathered components are ordered (u0, v1), (u1, v1), (u1, v0), (u0, v0).
        let weights = vec4((1.0 - f.x) * f.y, f.x * f.y, f.x * (1.0 - f.y), (1.0 - f.x) * (1.0 - f.y));
        let coverage = weights * a;
        color = vec4(dot(r, coverage), dot(g, coverage), dot(b, coverage), dot(a, weights));
    }
    return tinted(color, input);
}
fn shade_fragment_premultiplied(input: Output) -> vec4<f32> {
    return tinted(textureSample(image, image_sampler, input.uv), input);
}

@fragment fn fragment_straight(input: Output) -> @location(0) vec4<f32> {
    return shade_fragment_straight(input);
}
@fragment fn fragment_straight_covered(input: Output) -> @location(0) vec4<f32> {
    let color = shade_fragment_straight(input);
    if color.a <= 0.0 { discard; }
    return color;
}

@fragment fn fragment_straight_linear(input: Output) -> @location(0) vec4<f32> {
    return shade_fragment_straight_linear(input);
}
@fragment fn fragment_straight_linear_covered(input: Output) -> @location(0) vec4<f32> {
    let color = shade_fragment_straight_linear(input);
    if color.a <= 0.0 { discard; }
    return color;
}

@fragment fn fragment_premultiplied(input: Output) -> @location(0) vec4<f32> {
    return shade_fragment_premultiplied(input);
}
@fragment fn fragment_premultiplied_covered(input: Output) -> @location(0) vec4<f32> {
    let color = shade_fragment_premultiplied(input);
    if color.a <= 0.0 { discard; }
    return color;
}
