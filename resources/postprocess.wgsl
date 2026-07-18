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
    // ggez emits the quad in image-pixel space: 0..image_w, 0..image_h.
    // Normalize by image dimensions to get 0..1 UV coordinates.
    let uv = position / vec2<f32>(pp.screen_width, pp.screen_height);
    out.uv = uv;
    // Map 0..1 -> NDC -1..1, flipping Y because wgpu clip-space Y is up
    // while texture/UV Y is down.
    out.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.color = vec4<f32>(1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    // Debug: just sample and return the texture directly
    let sampled = textureSample(t, s, uv);
    return sampled;
}
