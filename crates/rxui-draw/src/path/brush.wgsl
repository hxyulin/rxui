struct BrushOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) coverage: f32,
    @location(2) clip: vec4<f32>,
    @location(3) @interpolate(flat) clip_radii: vec4<f32>,
    @location(4) paint_position: vec2<f32>,
};
@vertex fn vertex_brush(
    @location(0) position: vec2<f32>, @location(1) incoming: vec2<f32>,
    @location(2) outgoing: vec2<f32>, @location(3) outer: f32,
    @location(4) axes: vec4<f32>, @location(5) translation_viewport: vec4<f32>,
    @location(6) color: vec4<f32>, @location(7) style: vec4<f32>,
    @location(8) clip_axes: vec4<f32>, @location(9) clip_offset_half: vec4<f32>,
    @location(10) clip_radii: vec4<f32>,
) -> BrushOutput {
    let base=path_vertex(position,incoming,outgoing,outer,axes,translation_viewport,color,style,
        clip_axes,clip_offset_half,clip_radii);
    return BrushOutput(base.position,base.color,base.coverage,base.clip,base.clip_radii,position);
}
fn shade_brush(input: BrushOutput) -> vec4<f32> {
    let tint=shade(Output(input.position,input.color,input.coverage,input.clip,input.clip_radii));
    return brush_color(input.paint_position)*tint;
}
@fragment fn fragment_brush(input: BrushOutput) -> @location(0) vec4<f32> {
    return shade_brush(input);
}
@fragment fn fragment_brush_covered(input: BrushOutput) -> @location(0) vec4<f32> {
    let color=shade_brush(input);
    if color.a<=0.0 { discard; }
    return color;
}
