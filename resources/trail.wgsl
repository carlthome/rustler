// Conga trail / echo-afterimage accumulation shader.
//
// Drawn ADDITIVELY on top of the freshly-rendered crisp scene, sampling the PREVIOUS
// frame's accumulation buffer (ping-pong). It extracts only the bright / additive parts
// of that history — crab glows, beat rings, rope heat, score sparkles — and lays them
// back down faded by `strength`. Dark terrain is masked out, so the beach stays clean
// while the moving conga train leaves a comet of decaying light.
//
// `strength` folds the per-frame feedback decay together with a groove curve on the CPU:
// it is 0 at low groove (normal play stays crisp) and rises toward ~0.86 at max groove,
// so the delirium is earned, not constant.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct TrailUniform {
    strength: f32,
    // crevice AsStd140 pads a 1-float struct to a vec4 boundary (3 padding floats)
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

@group(1) @binding(0)
var t: texture_2d<f32>;

@group(1) @binding(1)
var s: sampler;

@group(3) @binding(0)
var<uniform> tr: TrailUniform;

@vertex
fn vs_main(@location(0) position: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(position.x * 2.0 - 1.0, 1.0 - position.y * 2.0, 0.0, 1.0);
    out.uv = position;
    out.color = vec4<f32>(1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let prev = textureSample(t, s, in.uv).rgb;

    // Keep only bright / additive elements — the conga train's light, not the terrain.
    let luma = dot(prev, vec3<f32>(0.299, 0.587, 0.114));
    let mask = smoothstep(0.45, 0.78, luma);

    // Emit the decayed bright residue. Alpha 0 so the additive blend only adds colour
    // and never touches the opaque scene's alpha channel.
    let trail = prev * mask * tr.strength;
    return vec4<f32>(trail, 0.0);
}
