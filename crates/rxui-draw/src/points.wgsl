@group(0) @binding(0) var<storage, read> points: array<vec2<u32>>;
struct Sample { p: vec2<f32>, valid: bool };
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) geometry: vec4<f32>,
    @location(2) @interpolate(flat) style: vec4<f32>,
    @location(3) @interpolate(flat) color: vec4<f32>,
};
fn sample_point(i: u32, range: vec4<u32>, axes: vec4<f32>, placement: vec4<f32>) -> Sample {
    if i >= range.y { return Sample(vec2(0.0), false); }
    let bits=points[(range.x+i)%range.z];
    if all(bits==vec2(0x7fc00001u)) { return Sample(vec2(0.0),false); }
    let p=bitcast<vec2<f32>>(bits);
    return Sample(mat2x2(axes.xy,axes.zw)*p+placement.xy,true);
}
fn unit(v: vec2<f32>) -> vec2<f32> {
    let scale=max(abs(v.x),abs(v.y));
    if scale==0.0 { return vec2(0.0); }
    return normalize(v/scale);
}
fn norm(v: vec2<f32>) -> f32 {
    let scale=max(abs(v.x),abs(v.y));
    if scale==0.0 { return 0.0; }
    return scale*length(v/scale);
}
fn normal(t: vec2<f32>) -> vec2<f32> { return vec2(-t.y,t.x); }
fn output_at(pixel:vec2<f32>,local:vec2<f32>,geometry:vec4<f32>,style:vec4<f32>,color:vec4<f32>,viewport:vec2<f32>) -> Output {
    let q=pixel/viewport;
    return Output(vec4(q.x*2.0-1.0,1.0-q.y*2.0,0.0,1.0),local,geometry,style,color);
}
fn empty_output() -> Output {
    return Output(vec4(0.0,0.0,0.0,1.0),vec2(0.0),vec4(0.0),vec4(0.0),vec4(0.0));
}
struct Join { m:vec2<f32>, d:f32, outer:f32, connected:bool, bevel:bool };
fn join_at(before:vec2<f32>,after:vec2<f32>,valid:bool,kind:u32,limit:f32) -> Join {
    let n0=normal(before);
    let n1=normal(after);
    let m=unit(n0+n1);
    let d=dot(m,n1);
    let turn=before.x*after.y-before.y*after.x;
    let connected=valid && d>0.0001 && dot(before,after)>-0.9999;
    return Join(m,d,-sign(turn),connected,kind!=0u || d<1.0/limit);
}
fn section(j:Join,t:vec2<f32>,side:f32,r:f32,limit:f32) -> vec2<f32> {
    if !j.connected { return normal(t)*side*r; }
    if j.bevel && side==j.outer { return normal(t)*side*r; }
    // Inner intersections are bounded too: short/reversing segments can still
    // overlap, but never produce unbounded spikes. This is a fast stroke, not a union.
    return j.m*side*(r/max(j.d,max(1.0/limit,0.0001)));
}
@vertex fn polyline_vertex(@builtin(vertex_index) vertex:u32,
    @location(0) axes:vec4<f32>,@location(1) placement:vec4<f32>,
    @location(2) color:vec4<f32>,@location(3) style:vec4<f32>,
    @location(4) range:vec4<u32>) -> Output {
    let i=vertex/range.w;
    let v=vertex%range.w;
    let a=sample_point(i,range,axes,placement);
    let b=sample_point(i+1u,range,axes,placement);
    if !a.valid || !b.valid || all(a.p==b.p) { return empty_output(); }
    let t=unit(b.p-a.p);
    let n=normal(t);
    let length=norm(b.p-a.p);
    let radius=style.x*0.5;
    let padding=select(0.0,0.5,style.z>0.0);
    let r=radius+padding;
    let mode=u32(style.w);
    let kind=mode/4u;
    let cap=mode%4u;
    var prev=Sample(vec2(0.0),false);
    if i>0u { prev=sample_point(i-1u,range,axes,placement); }
    let next=sample_point(i+2u,range,axes,placement);
    let incoming=unit(a.p-prev.p);
    let outgoing=unit(next.p-b.p);
    let start=join_at(incoming,t,prev.valid && any(a.p!=prev.p),kind,style.y);
    let end=join_at(t,outgoing,next.valid && any(b.p!=next.p),kind,style.y);
    let base=select(9u,30u,kind==2u);
    if v<6u {
        let sides=array(-1.0,-1.0,1.0,1.0,-1.0,1.0);
        let ends=array(false,true,false,false,true,true);
        let side=sides[v];
        var q=a.p+section(start,t,side,r,style.y);
        if ends[v] { q=b.p+section(end,t,side,r,style.y); }
        if !start.connected && !ends[v] && cap!=2u { q-=t*(padding+select(0.0,radius,cap==1u)); }
        if !end.connected && ends[v] && cap!=2u { q+=t*(padding+select(0.0,radius,cap==1u)); }
        let local=vec2(dot(q-a.p,t),dot(q-a.p,n));
        let flags=select(0u,1u,!start.connected && cap!=2u)+select(0u,2u,!end.connected && cap!=2u)+select(0u,4u,cap==1u);
        return output_at(q,local,vec4(radius,length,f32(flags),0.0),style,color,placement.zw);
    }
    if v<base {
        if !start.connected || !start.bevel || start.outer==0.0 { return empty_output(); }
        let outer=start.outer;
        let m=start.m*outer;
        let inner=section(start,t,-outer,r,style.y);
        let n0=normal(incoming)*outer;
        let n1=n*outer;
        var q=inner;
        if kind==2u {
            let triangle=(v-6u)/3u;
            let corner=(v-6u)%3u;
            let angle=atan2(n0.x*n1.y-n0.y*n1.x,dot(n0,n1));
            let fraction=f32(triangle+select(0u,1u,corner==2u))/8.0;
            let angle_step=angle*fraction;
            let rotated=vec2(n0.x*cos(angle_step)-n0.y*sin(angle_step),n0.x*sin(angle_step)+n0.y*cos(angle_step));
            if corner!=0u { q=rotated*r; }
            return output_at(a.p+q,q,vec4(radius,0.0,0.0,2.0),vec4(m,style.z,0.0),color,placement.zw);
        }
        if v==7u { q=n0*r; }
        if v==8u { q=n1*r; }
        let local=vec2(dot(q,m),0.0);
        return output_at(a.p+q,local,vec4(radius,radius*dot(n0,m),0.0,1.0),style,color,placement.zw);
    }
    // Round cap half-quads share the terminal cross-section with the body.
    let cap_vertex=v-base;
    let terminal=cap_vertex>=6u;
    if cap!=2u || (!terminal && start.connected) || (terminal && end.connected) { return empty_output(); }
    let corners=array(vec2(0.0,-1.0),vec2(1.0,-1.0),vec2(0.0,1.0),vec2(0.0,1.0),vec2(1.0,-1.0),vec2(1.0,1.0));
    let local=corners[cap_vertex%6u]*r;
    let direction=select(-1.0,1.0,terminal);
    let center=select(a.p,b.p,terminal);
    let q=t*local.x*direction+n*local.y;
    return output_at(center+q,q,vec4(radius,0.0,0.0,3.0),style,color,placement.zw);
}
@vertex fn marker_vertex(@builtin(vertex_index) vertex:u32,
    @location(0) axes:vec4<f32>,@location(1) placement:vec4<f32>,
    @location(2) color:vec4<f32>,@location(3) style:vec4<f32>,
    @location(4) range:vec4<u32>) -> Output {
    let s=sample_point(vertex/6u,range,axes,placement);
    if !s.valid { return empty_output(); }
    let corners=array(vec2(-1.0,-1.0),vec2(1.0,-1.0),vec2(-1.0,1.0),vec2(-1.0,1.0),vec2(1.0,-1.0),vec2(1.0,1.0));
    let radius=style.x;
    let local=corners[vertex%6u]*(radius+select(0.0,1.0,style.z>0.0));
    return output_at(s.p+local,local,vec4(radius,0.0,0.0,4.0+style.y),style,color,placement.zw);
}
fn filter_interval(p:f32,lo:f32,hi:f32,footprint:f32) -> f32 {
    return clamp((hi-p)/footprint+0.5,0.0,1.0)-clamp((lo-p)/footprint+0.5,0.0,1.0);
}
fn shade(input:Output) -> vec4<f32> {
    let kind=u32(input.geometry.w);
    let radius=input.geometry.x;
    var distance=norm(input.local)-radius;
    if kind==1u { distance=input.local.x-input.geometry.y; }
    if kind==2u && dot(input.local,input.style.xy)<0.0 { distance=-radius; }
    if kind==5u { distance=max(abs(input.local.x),abs(input.local.y))-radius; }
    if kind==6u { distance=(abs(input.local.x)+abs(input.local.y)-radius)*0.70710678; }
    let footprint=max(fwidth(input.local),vec2(0.000001));
    let edge=max(fwidth(distance),0.000001);
    var coverage=select(0.0,1.0,distance<=0.0);
    if input.style.z>0.0 { coverage=clamp(0.5-distance/edge,0.0,1.0); }
    if kind==0u {
        let flags=u32(input.geometry.z);
        var lo=0.0;
        var hi=0.0;
        let extension=select(0.0,radius,(flags&4u)!=0u);
        if (flags&1u)!=0u { lo=-extension; }
        if (flags&2u)!=0u { hi=input.geometry.y+extension; }
        coverage=select(0.0,1.0,abs(input.local.y)<=radius && ((flags&1u)==0u || input.local.x>=lo) && ((flags&2u)==0u || input.local.x<=hi));
        if input.style.z>0.0 {
            coverage=filter_interval(input.local.y,-radius,radius,footprint.y);
            if (flags&1u)!=0u { coverage*=clamp((input.local.x-lo)/footprint.x+0.5,0.0,1.0); }
            if (flags&2u)!=0u { coverage*=clamp((hi-input.local.x)/footprint.x+0.5,0.0,1.0); }
        }
    }
    let alpha=input.color.a*clamp(coverage,0.0,1.0)*rounded_clip_coverage(input.position.xy);
    return vec4(input.color.rgb*alpha,alpha);
}
@fragment fn fragment_main(input:Output) -> @location(0) vec4<f32> { return shade(input); }
@fragment fn fragment_covered(input:Output) -> @location(0) vec4<f32> {
    let color=shade(input);
    if color.a<=0.0 { discard; }
    return color;
}
