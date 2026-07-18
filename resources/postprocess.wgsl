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
    // ggez generates vertices based on image size (1280x960), but we need to scale to drawable size.
    // Position is in image space; scale to normalized device coordinates.
    let ndc_x = (position.x / 1280.0) * 2.0 - 1.0;
    let ndc_y = 1.0 - (position.y / 960.0) * 2.0;
    out.position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    // UV: directly from normalized position
    out.uv = position / vec2<f32>(1280.0, 960.0);
    out.color = vec4<f32>(1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Debug: output magenta to verify shader is running
    return vec4<f32>(1.0, 0.0, 1.0, 1.0);
}
