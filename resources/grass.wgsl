struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct ResolutionUniform {
    width: f32,
    height: f32,
    time: f32,
    // Beat phase in [0,1): 0.0 the instant a beat lands, climbing toward 1.0 before the next.
    beat: f32,
}

@group(1) @binding(0)
var t: texture_2d<f32>;

@group(1) @binding(1)
var s: sampler;

@group(3) @binding(0)
var<uniform> resolution: ResolutionUniform;

@vertex
fn vs_main(@location(0) position: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.uv = position * 0.5 + vec2<f32>(0.5, 0.5); // Remap NDC [-1,1] to [0,1], origin at bottom left
    out.color = vec4<f32>(1.0);
    return out;
}

// Simple hash function for randomness
fn hash(n: f32) -> f32 {
    return fract(sin(n) * 43758.5453);
}

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

// Smooth value noise on a 2D grid.
fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Fractal Brownian motion — layered noise for organic ground mottling.
fn fbm(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var amp = 0.5;
    var freq = 1.0;
    for (var i = 0; i < 4; i += 1) {
        v += amp * value_noise(p * freq);
        freq *= 2.0;
        amp *= 0.5;
    }
    return v;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Aspect-correct field space so noise cells and tufts stay round rather than
    // stretched with the window's aspect ratio.
    let aspect = resolution.width / max(resolution.height, 1.0);
    let uv = vec2<f32>(in.uv.x * aspect, in.uv.y);
    let time = resolution.time;

    // A slow wind vector that drifts the whole field so nothing sits perfectly still.
    let wind = vec2<f32>(sin(time * 0.35), cos(time * 0.27)) * 0.06;

    // --- Base ground: organic mottling from layered noise -------------------
    // Two noise octaves at different scales give large soft patches plus finer
    // grain, so the ground reads as a living field instead of a flat fill.
    let patches = fbm(uv * 4.0 + wind);
    let grain = fbm(uv * 18.0 - wind * 2.0);
    let mottle = mix(patches, grain, 0.35);

    // Blend between a darker, cooler shade and a brighter, warmer one by the noise.
    let ground_dark = vec3<f32>(0.13, 0.42, 0.20);
    let ground_light = vec3<f32>(0.28, 0.66, 0.32);
    var color = mix(ground_dark, ground_light, smoothstep(0.25, 0.75, mottle));

    // Scattered dirt/dry patches where the noise dips low.
    let dry = 1.0 - smoothstep(0.12, 0.32, patches);
    color = mix(color, vec3<f32>(0.34, 0.44, 0.22), dry * 0.35);

    // --- Wind light-sweep: a soft diagonal band of brightness that glides across
    // the field, like sunlight rippling over grass on a breezy day.
    let sweep_dir = normalize(vec2<f32>(0.8, 0.6));
    let sweep = sin(dot(uv, sweep_dir) * 3.5 - time * 0.9);
    color += vec3<f32>(0.05, 0.07, 0.03) * smoothstep(0.6, 1.0, sweep);

    // --- Grass tufts scattered across the whole field -----------------------
    // Cellular scatter: each grid cell may hold one small tuft that sways with the
    // wind. Single-cell lookup keeps this cheap regardless of screen size.
    let cell_scale = 26.0;
    let g = uv * cell_scale;
    let cell = floor(g);
    let fpart = fract(g);
    let present = step(0.52, hash2(cell + vec2<f32>(9.1, 4.3)));
    // Random tuft anchor within the cell, plus a per-cell wind sway phase.
    var anchor = vec2<f32>(hash2(cell), hash2(cell + vec2<f32>(3.7, 1.2)));
    let sway = sin(time * 1.6 + hash2(cell) * 6.28) * 0.08;
    anchor.x += sway;
    let td = distance(fpart, anchor);
    // Small bright blade cluster with a soft edge.
    let tuft = (1.0 - smoothstep(0.02, 0.11, td)) * present;
    let tuft_color = vec3<f32>(
        0.30 + hash2(cell + vec2<f32>(4.0, 0.0)) * 0.18,
        0.72 + hash2(cell + vec2<f32>(5.0, 0.0)) * 0.20,
        0.22 + hash2(cell + vec2<f32>(6.0, 0.0)) * 0.14
    );
    color = mix(color, tuft_color, tuft * 0.85);

    // Tiny highlight speck at each tuft tip for a bit of sparkle in the field.
    let tip = (1.0 - smoothstep(0.0, 0.035, td)) * present;
    color += vec3<f32>(0.12, 0.16, 0.08) * tip;

    // --- Beat ripple: a ring of light rides outward from screen center on every downbeat,
    // so the whole ground pulses in time with the music. `beat` is 0 the instant a beat lands
    // and climbs toward 1 before the next, so the ring's radius tracks the phase directly.
    let center = vec2<f32>(0.5 * aspect, 0.5);
    let r = distance(uv, center);
    // Ring radius sweeps out with the beat phase; a smooth band chases it.
    let ring_r = resolution.beat * 1.3;
    let ring = 1.0 - smoothstep(0.0, 0.16, abs(r - ring_r));
    // Fade the ring as it expands so it dissolves at the edges rather than snapping off.
    let ring_fade = 1.0 - smoothstep(0.5, 1.3, ring_r);
    // Warm gold pulse, brightest at the leading edge and on the tufts it washes over.
    let pulse = ring * ring_fade;
    color += vec3<f32>(0.16, 0.13, 0.05) * pulse;
    color += tuft_color * tuft * pulse * 0.9;

    // A gentle full-field breath right on the beat (strongest at beat==0), so even far from
    // the ring the ground lifts a touch with each hit.
    let breath = pow(1.0 - resolution.beat, 3.0);
    color += vec3<f32>(0.03, 0.045, 0.02) * breath;

    return vec4<f32>(color, 1.0);
}
