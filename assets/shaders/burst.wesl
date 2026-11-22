import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> tint_seed: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> phase: f32;

// 内部使用预乘透明度合成，输出给 Material2d 时还原为非预乘颜色。
fn over(bg: vec4<f32>, color: vec3<f32>, alpha: f32) -> vec4<f32> {
    return bg * (1.0 - alpha) + vec4<f32>(color * alpha, alpha);
}
fn noise(i: f32) -> f32 { return fract(sin(i * 73.19 + tint_seed.w * 0.91) * 4375.71); }
fn direction(a: f32) -> vec2<f32> { return vec2<f32>(cos(a), sin(a)); }
fn axes(q: vec2<f32>, d: vec2<f32>) -> vec2<f32> { return vec2<f32>(dot(q,d), dot(q,vec2<f32>(-d.y,d.x))); }

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let q = 2.0 * in.uv - 1.0;
    let t = phase;
    let aa = max(fwidth(q.x), 0.0001);
    let r = length(q);
    let gold = vec3<f32>(1.0, 0.48, 0.085);
    let ivory = vec3<f32>(1.0, 0.90, 0.63);
    var col = vec4<f32>(0.0);
    // 在碎片轮廓后方绘制逐渐扩散的柔和烟尘，不做全屏闪光。
    for (var i = 0; i < 5; i++) {
        let k = f32(i);
        let d = direction(1.257 * k + noise(k));
        let center = d * (0.07 + t * (0.15 + 0.11 * noise(k+2.0))) - vec2<f32>(0.0, 0.14*t*t);
        let size = 0.13 + t * 0.19;
        let mask = 1.0 - smoothstep(0.27*size, size, length(q-center));
        col = over(col, vec3<f32>(0.073,0.084,0.076), mask * 0.68 * (1.0-t));
    }
    let spread = 1.0 - (1.0-t)*(1.0-t);
    let ring = abs(r - (0.10 + 0.57*spread));
    col = over(col, gold, (1.0-smoothstep(0.003,0.017+aa,ring)) * 0.75 * (1.0-smoothstep(0.02,0.53,t)));
    // 橙色火花先减速熄灭，金属碎片随后才落下。
    for (var i = 0; i < 12; i++) {
        let k = f32(i);
        let d = direction(0.524 * k + 0.45*noise(k+13.0) + 0.41*tint_seed.w);
        let u = min(t / (0.37 + 0.33*noise(k+7.0)), 1.0);
        let head = 0.10 + (0.38 + 0.42*noise(k+3.0)) * (1.0-(1.0-u)*(1.0-u));
        let p = axes(q,d);
        let delta = vec2<f32>(p.x-clamp(p.x,head-0.14*(1.0-u),head), p.y);
        let mask = 1.0-smoothstep(0.009*(1.0-u),0.010+aa,length(delta));
        col = over(col, mix(ivory,gold,u), mask*(1.0-u*u));
    }
    // 翻滚的金属碎片带有倒角，涂装沿用被击中坦克的队色。
    for (var i = 0; i < 7; i++) {
        let k = f32(i);
        let d = direction(0.898*k + noise(k+5.0) + tint_seed.w);
        let center = d*(0.04 + (0.36+0.30*noise(k))*spread) + vec2<f32>(0.0,0.12*t*t);
        let p = abs(axes(q-center,direction(k+spread*(2.0+5.0*noise(k+1.0)))));
        let size = (1.0-0.55*t) * (0.8+0.4*noise(k+9.0));
        let sdf = max(max(p.x-0.068*size, p.y-0.029*size), (p.x+p.y-0.084*size)*0.707);
        let mask = 1.0-smoothstep(-aa,aa,sdf);
        let paint = mix(tint_seed.rgb, vec3<f32>(0.035,0.053,0.05), 0.22*noise(k));
        col = over(col, mix(paint,ivory,0.55*(1.0-smoothstep(0.0,0.27,t))), mask*(1.0-smoothstep(0.53,1.0,t)));
    }
    let flash = 1.0-smoothstep(0.0,0.30,t);
    let core = 1.0 - smoothstep(0.06, 0.27, r);
    col = over(col, mix(gold,ivory,core), core*flash);
    return vec4<f32>(col.rgb/max(col.a,0.00001),col.a);
}
