@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) kind: f32,
    @location(3) clip: vec4<f32>,
    @location(4) @interpolate(flat) clip_radii: vec4<f32>,
};
@vertex fn vertex_main(@builtin(vertex_index) index: u32,
    @location(0) rect: vec4<f32>, @location(1) uv: vec4<f32>, @location(2) kind: vec4<u32>,
    @location(3) origin_axis_x: vec4<f32>, @location(4) axis_y: vec4<f32>, @location(5) color: vec4<f32>,
    @location(6) clip_axes: vec4<f32>, @location(7) clip_offset_half: vec4<f32>,
    @location(8) clip_radii: vec4<f32>) -> Output {
    let corners = array(vec2(0.0, 0.0), vec2(0.0, 1.0), vec2(1.0, 0.0),
                        vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(1.0, 1.0));
    let corner = corners[index];
    let local = rect.xy + corner * rect.zw;
    let destination = origin_axis_x.xy + local.x * origin_axis_x.zw + local.y * axis_y.xy;
    var output: Output;
    output.position = vec4(destination * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
    output.uv = uv.xy + corner * uv.zw;
    // Span colors replace the draw RGB unless the draw disables them (axis_y.z).
    // The draw alpha already includes opacity and multiplies every glyph.
    let span = vec4(unpack_half2(kind.y), unpack_half2(kind.z));
    output.color = select(color, vec4(span.rgb, span.a * color.a), kind.w != 0u && axis_y.z == 0.0);
    output.kind = f32(kind.x);
    output.clip = clip_varying(destination, clip_axes, clip_offset_half);
    output.clip_radii = clip_radii;
    return output;
}
// Bit-exact `unpack2x16float` without Naga's SHADER_FLOAT16_IN_FLOAT32
// requirement, which wgpu withholds on adapters without shaderFloat16 (lavapipe).
fn unpack_half(bits: u32) -> f32 {
    let sign = (bits & 0x8000u) << 16u;
    let exponent = (bits >> 10u) & 0x1fu;
    let mantissa = bits & 0x3ffu;
    if exponent == 0u {
        let magnitude = f32(mantissa) * exp2(-24.0);
        return select(magnitude, -magnitude, sign != 0u);
    }
    let biased = select(exponent + 112u, 255u, exponent == 31u);
    return bitcast<f32>(sign | (biased << 23u) | (mantissa << 13u));
}
fn unpack_half2(bits: u32) -> vec2<f32> {
    return vec2(unpack_half(bits & 0xffffu), unpack_half(bits >> 16u));
}
// Filtered coverage of the unit square in a decoration quad's UV space.
fn solid_coverage(uv: vec2<f32>) -> f32 {
    let footprint = max(fwidth(uv), vec2(0.000001));
    let inside = clamp((vec2(1.0) - uv) / footprint + 0.5, vec2(0.0), vec2(1.0))
        - clamp(-uv / footprint + 0.5, vec2(0.0), vec2(1.0));
    return inside.x * inside.y;
}
fn shade_fragment_main(input: Output) -> vec4<f32> {
    let texel = textureSample(atlas, atlas_sampler, input.uv);
    let clip = rounded_clip_coverage(input.position.xy);
    let solid = solid_coverage(input.uv);
    if input.kind == 0.0 || input.kind == 3.0 {
        let coverage = select(texel.r, solid, input.kind == 3.0);
        let alpha = coverage * input.color.a * clip;
        return vec4(input.color.rgb * alpha, alpha);
    }
    // Color atlas stores linear, premultiplied RGBA. Draw RGB colors masks only.
    return texel * input.color.a * clip;
}

fn shade_fragment_mtsdf(input: Output) -> vec4<f32> {
    let texel = textureSample(atlas, atlas_sampler, input.uv);
    let distance = max(min(texel.r, texel.g), min(max(texel.r, texel.g), texel.b)) - 0.5;
    // The gradient of reconstructed distance follows the screen-space edge normal,
    // accounting for rotation, reflection, and anisotropic affine scaling.
    let distance_width = max(length(vec2(dpdx(distance), dpdy(distance))), 0.000001);
    let coverage = clamp(distance / distance_width + 0.5, 0.0, 1.0);
    let alpha = coverage * input.color.a * rounded_clip_coverage(input.position.xy);
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

@fragment fn fragment_mtsdf(input: Output) -> @location(0) vec4<f32> {
    return shade_fragment_mtsdf(input);
}
@fragment fn fragment_mtsdf_covered(input: Output) -> @location(0) vec4<f32> {
    let color = shade_fragment_mtsdf(input);
    if color.a <= 0.0 { discard; }
    return color;
}
