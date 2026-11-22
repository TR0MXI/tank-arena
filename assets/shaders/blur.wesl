import bevy_ui_render::ui_vertex_output::UiVertexOutput;

@group(1) @binding(0) var home: texture_2d<f32>;
@group(1) @binding(1) var linear_sampler: sampler;

@group(1) @binding(2) var<uniform> reveal: f32;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    // 二项式滤波核按窗口逻辑单位取样，而非按 Retina 屏幕的物理像素取样。
    let step = vec2<f32>(1.45 / 1080.0, 1.45 / 720.0) * reveal;
    var color = vec3<f32>(0.0);
    let weights = array<f32, 5>(1.0, 4.0, 6.0, 4.0, 1.0);
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            let weight = weights[x + 2] * weights[y + 2] / 256.0;
            color += weight * textureSample(home, linear_sampler, in.uv + vec2<f32>(f32(x), f32(y)) * step).rgb;
        }
    }
    return vec4<f32>(mix(color, vec3<f32>(0.012, 0.026, 0.024), 0.30 * reveal), 1.0);
}
