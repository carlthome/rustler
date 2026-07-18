struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct PostProcessUniform {
    groove: f32,
    time: f32,
    screen_width: f32,
    screen_height: f32,
}

@group(1) @binding(0)
var t: texture_2d<f32>;

@group(1) @binding(1)
var s: sampler;

@group(3) @binding(0)
var<uniform> pp: PostProcessUniform;

@vertex
fn vs_main(@location(0) position: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    // ggez generates position in logical image space (0-1280, 0-960).
    // Scale to drawable space, then to NDC for proper full-screen coverage.
    let logical_pos = position / vec2<f32>(1280.0, 960.0); // 0..1
    let screen_pos = logical_pos * vec2<f32>(pp.screen_width, pp.screen_height); // 0..drawable
    let ndc = screen_pos / vec2<f32>(pp.screen_width, pp.screen_height) * 2.0 - vec2<f32>(1.0);
    out.position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    out.uv = logical_pos;
    out.color = vec4<f32>(1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Debug: output magenta to verify shader is running
    return vec4<f32>(1.0, 0.0, 1.0, 1.0);
}
