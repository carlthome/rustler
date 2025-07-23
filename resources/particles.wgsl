struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct ParticleUniform {
    screen_width: f32,
    screen_height: f32,
    time: f32,
    _padding: f32,
}

@group(1) @binding(0)
var t: texture_2d<f32>;

@group(1) @binding(1)
var s: sampler;

@group(3) @binding(0)
var<uniform> particle_data: ParticleUniform;

@vertex
fn vs_main(@location(0) position: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.uv = position * 0.5 + vec2<f32>(0.5, 0.5);
    out.color = vec4<f32>(1.0);
    return out;
}

// Convert UV coordinates to screen coordinates
fn uv_to_screen(uv: vec2<f32>) -> vec2<f32> {
    let x = uv.x * particle_data.screen_width;
    let y = (1.0 - uv.y) * particle_data.screen_height;
    return vec2<f32>(x, y);
}

// Simple sparkle effect
fn sparkle(pos: vec2<f32>, time: f32) -> f32 {
    let hash_input = pos.x * 12.9898 + pos.y * 78.233;
    let random_val = fract(sin(hash_input) * 43758.5453);
    let sparkle_time = fract(time * 2.0 + random_val * 6.28);
    let intensity = sin(sparkle_time * 6.28) * 0.5 + 0.5;
    return pow(intensity, 4.0) * 0.3;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let screen_pos = uv_to_screen(in.uv);
    
    // Create a subtle sparkle effect across the screen
    let sparkle_intensity = sparkle(screen_pos * 0.05, particle_data.time);
    
    // Base color is transparent
    var color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    
    // Add sparkles
    if (sparkle_intensity > 0.8) {
        color = vec4<f32>(1.0, 1.0, 0.8, sparkle_intensity * 0.5);
    }
    
    return color;
}
