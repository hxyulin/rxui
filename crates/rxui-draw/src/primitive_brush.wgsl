struct BrushOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) geometry: vec4<f32>,
    @location(2) @interpolate(flat) style: vec2<f32>,
    @location(3) @interpolate(flat) color: vec4<f32>,
    @location(4) @interpolate(flat) radii: vec4<f32>,
    @location(5) clip: vec4<f32>,
    @location(6) @interpolate(flat) clip_radii: vec4<f32>,
    @location(7) paint_position: vec2<f32>,
};
@vertex fn vertex_brush(@builtin(vertex_index) index: u32,
    @location(0) origin_axis_x: vec4<f32>, @location(1) axis_y_min: vec4<f32>,
    @location(2) span_style: vec4<f32>, @location(3) geometry: vec4<f32>,
    @location(4) color: vec4<f32>, @location(5) radii: vec4<f32>,
    @location(6) paint_origin_x: vec4<f32>, @location(7) paint_axis_y: vec4<f32>,
    @location(8) clip_axes: vec4<f32>, @location(9) clip_offset_half: vec4<f32>,
    @location(10) clip_radii: vec4<f32>) -> BrushOutput {
    let base=primitive_vertex(index,origin_axis_x,axis_y_min,span_style,geometry,color,radii,
        clip_axes,clip_offset_half,clip_radii);
    let position=paint_origin_x.xy+base.local.x*paint_origin_x.zw+base.local.y*paint_axis_y.xy;
    return BrushOutput(base.position,base.local,base.geometry,base.style,base.color,base.radii,
        base.clip,base.clip_radii,position);
}
fn shade_brush(input: BrushOutput) -> vec4<f32> {
    let tint=shade_fragment_main(Output(input.position,input.local,input.geometry,input.style,
        input.color,input.radii,input.clip,input.clip_radii));
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
