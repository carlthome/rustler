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
    title_card_t: f32,
    // crevice AsStd140 pads a 5-float struct to vec4 boundary (3 padding floats)
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
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
    out.position = vec4<f32>(position.x * 2.0 - 1.0, 1.0 - position.y * 2.0, 0.0, 1.0);
    out.uv = position;
    out.color = vec4<f32>(1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;

    // Chromatic aberration — scales with groove (invisible at 0, up to ~6px split at max groove)
    let ca = pp.groove * 0.006;
    let r = textureSample(t, s, uv + vec2<f32>(ca, 0.0)).r;
    let g = textureSample(t, s, uv).g;
    let b_ch = textureSample(t, s, uv - vec2<f32>(ca, 0.0)).b;
    var color = vec3<f32>(r, g, b_ch);

    // CRT scanlines — subtle horizontal darkening at every screen pixel row
    let line = sin(uv.y * pp.screen_height * 3.14159);
    color = color * (0.94 + 0.06 * line);

    // Vignette — darken edges
    let vig_uv = uv * 2.0 - vec2<f32>(1.0);
    let vignette = clamp(1.0 - dot(vig_uv * 0.6, vig_uv * 0.6), 0.0, 1.0);
    color = color * vignette;

    // Haze glow — soft bloom at high groove
    if (pp.groove > 0.5) {
        let px_x = 1.0 / pp.screen_width;
        let px_y = 1.0 / pp.screen_height;
        let c0 = textureSample(t, s, uv).rgb;
        let c1 = textureSample(t, s, uv + vec2<f32>(px_x, 0.0)).rgb;
        let c2 = textureSample(t, s, uv - vec2<f32>(px_x, 0.0)).rgb;
        let blurred = (c0 + c1 + c2) / 3.0;
        let blend = (pp.groove - 0.5) * 0.25;
        color = mix(color, blurred, blend);
    }

    // Title card effect — desaturate + darken the world while the level card is showing.
    // t=0: no effect. t=1: full greyscale + slight darkening.
    if (pp.title_card_t > 0.0) {
        let luma = dot(color, vec3<f32>(0.299, 0.587, 0.114));
        let grey = vec3<f32>(luma);
        // Desaturate toward grey
        color = mix(color, grey, pp.title_card_t * 0.85);
        // Slight darkening so the white title card text pops
        color = color * (1.0 - pp.title_card_t * 0.25);
        // Subtle blue-grey tint like Control's aesthetic
        let tint = vec3<f32>(0.88, 0.92, 1.0);
        color = mix(color, color * tint, pp.title_card_t * 0.4);
    }

    return vec4<f32>(color, 1.0);
}
