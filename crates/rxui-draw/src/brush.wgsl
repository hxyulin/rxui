struct BrushUniform { axes: vec4<f32>, translation: vec4<f32>, style: vec4<u32> };
struct GradientStop { position: vec4<f32>, color: vec4<f32> };
@group(0) @binding(0) var<uniform> brush: BrushUniform;
@group(0) @binding(1) var<storage, read> stops: array<GradientStop>;

fn brush_color(position: vec2<f32>) -> vec4<f32> {
    if brush.style.x == 0u { return stops[0].color; }
    let q=mat2x2(brush.axes.xy,brush.axes.zw)*position+brush.translation.xy;
    var t=q.x;
    if brush.style.x == 2u {
        // Avoid squaring large coordinates in length(). CPU validation bounds q.
        let scale=max(abs(q.x),abs(q.y));
        t=0.0;
        if scale>0.0 { t=scale*length(q/scale); }
    }
    if brush.style.y == 1u { t=fract(t); }
    else if brush.style.y == 2u { t=1.0-abs(fract(t*0.5)*2.0-1.0); }
    else { t=clamp(t,0.0,1.0); }
    let count=arrayLength(&stops);
    if t<stops[0].position.x { return stops[0].color; }
    if t>=stops[count-1u].position.x { return stops[count-1u].color; }
    // Upper bound selects the last of equal-position stops, with a nonzero
    // denominator on the interval to its right. Exact stops are right-continuous.
    var lo=0u;
    var hi=count;
    loop {
        if lo>=hi { break; }
        let mid=lo+(hi-lo)/2u;
        if stops[mid].position.x<=t { lo=mid+1u; } else { hi=mid; }
    }
    let left=stops[lo-1u];
    let right=stops[lo];
    let u=(t-left.position.x)/(right.position.x-left.position.x);
    return left.color*(1.0-u)+right.color*u;
}
