// Public custom-shader interface: group 1 is reserved when opting into clipping.
struct RoundedClipRecord {
    axes: vec4<f32>,
    offset_half: vec4<f32>,
    radii: vec4<f32>,
};
struct RoundedClipStack {
    // viewport origin, count, reserved
    header: vec4<f32>,
    clips: array<RoundedClipRecord, 64>,
};
@group(1) @binding(0) var<uniform> rounded_clips: RoundedClipStack;

// Call in uniform fragment control flow, before any discard.
fn rounded_clip_coverage(fragment_position: vec2<f32>) -> f32 {
    let position = fragment_position - rounded_clips.header.xy;
    var coverage = 1.0;
    for (var i = 0u; i < u32(rounded_clips.header.z); i += 1u) {
        let clip = rounded_clips.clips[i];
        let point = mat2x2(clip.axes.xy, clip.axes.zw) * position + clip.offset_half.xy;
        let top = select(clip.radii.x, clip.radii.y, point.x > 0.0);
        let bottom = select(clip.radii.w, clip.radii.z, point.x > 0.0);
        let radius = select(top, bottom, point.y > 0.0);
        let q = abs(point) - clip.offset_half.zw + vec2(radius);
        let distance = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - radius;
        let width = max(fwidth(distance), 0.000001);
        // Minimum is the intersection of filtered coverages; coincident boundaries
        // remain idempotent rather than becoming darker with every nested scope.
        coverage = min(coverage, clamp(0.5 - distance / width, 0.0, 1.0));
    }
    return coverage;
}
