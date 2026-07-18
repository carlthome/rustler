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
    // ggez passes raw pixel-space positions to custom vertex shaders (no MVP applied).
    // We must convert: pixel (0,0)=top-left → NDC (-1,+1); pixel (w,h)=bottom-right → NDC (+1,-1).
    let ndcx = (position.x / pp.screen_width) * 2.0 - 1.0;
    let ndcy = 1.0 - (position.y / pp.screen_height) * 2.0;
    out.position = vec4<f32>(ndcx, ndcy, 0.0, 1.0);
    // UV: (0,0) = top-left of texture, (1,1) = bottom-right — matches ggez image storage.
    out.uv = vec2<f32>(position.x / pp.screen_width, position.y / pp.screen_height);
    out.color = vec4<f32>(1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Debug: output magenta to verify shader is running
    return vec4<f32>(1.0, 0.0, 1.0, 1.0);
}
