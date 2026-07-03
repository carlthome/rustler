struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct FlashlightUniform {
    center_x: f32,
    center_y: f32,
    angle: f32,
    spread: f32,
    range: f32,
    time: f32,
    time_since_catch: f32,
    laser_level: f32,
    screen_width: f32,
    screen_height: f32,
}

@group(1) @binding(0)
var t: texture_2d<f32>;

@group(1) @binding(1)
var s: sampler;

@group(3) @binding(0)
var<uniform> flashlight: FlashlightUniform;

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
    // UV coordinates from vertex shader go from (0,0) at bottom-left to (1,1) at top-right
    // Game coordinates have (0,0) at top-left, (width, height) at bottom-right
    // So we need to flip the Y coordinate
    let x = uv.x * flashlight.screen_width;
    let y = (1.0 - uv.y) * flashlight.screen_height;
    return vec2<f32>(x, y);
}

// Calculate distance from point to flashlight center
fn distance_to_center(screen_pos: vec2<f32>) -> f32 {
    let center = vec2<f32>(flashlight.center_x, flashlight.center_y);
    return length(screen_pos - center);
}

// Calculate angle from flashlight center to point
fn angle_to_point(screen_pos: vec2<f32>) -> f32 {
    let center = vec2<f32>(flashlight.center_x, flashlight.center_y);
    let dir = screen_pos - center;
    return atan2(dir.y, dir.x);
}

// Check if point is within flashlight cone
fn is_in_cone(screen_pos: vec2<f32>) -> bool {
    let angle_to_pos = angle_to_point(screen_pos);
    let angle_diff = abs(angle_to_pos - flashlight.angle);
    
    // Handle angle wrapping around 2π
    let wrapped_diff = min(angle_diff, 2.0 * 3.14159 - angle_diff);
    
    return wrapped_diff <= flashlight.spread * 0.5;
}

// Calculate gradient alpha based on distance and layers
fn calculate_gradient_alpha(screen_pos: vec2<f32>, layer: f32, num_layers: f32) -> f32 {
    let dist = distance_to_center(screen_pos);
    let t_layer = layer / max(num_layers - 1.0, 1.0);
    
    // Scale based on layer
    let min_scale = 0.7;
    let max_scale = 1.4;
    let scale = min_scale + (max_scale - min_scale) * t_layer;
    let scaled_range = flashlight.range * scale;
    
    if (dist > scaled_range) {
        return 0.0;
    }
    
    // Alpha interpolation based on layer
    let min_alpha = 180.0 / 255.0;
    let base_freq = 4.0;
    let max_freq = 18.0;
    let freq = base_freq + (max_freq - base_freq) * min(flashlight.time_since_catch / 12.0, 1.0);
    let flicker_strength = min(flashlight.time_since_catch / 3.0, 2.0);
    let flicker = abs(sin(flashlight.time * freq + (flashlight.center_x + flashlight.center_y) * 0.01));
    let base_alpha = 24.0 / 255.0;
    let max_alpha_val = 90.0 / 255.0;
    let alpha = base_alpha + (max_alpha_val - base_alpha) * flicker * flicker_strength;
    let max_alpha = max(alpha * 0.18, 10.0 / 255.0);
    
    let final_alpha = min_alpha + (max_alpha - min_alpha) * t_layer;
    
    // Distance falloff
    let falloff = 1.0 - (dist / scaled_range);
    
    return final_alpha * falloff;
}

// Calculate layer color
fn calculate_layer_color(layer: f32, num_layers: f32) -> vec3<f32> {
    let t = layer / max(num_layers - 1.0, 1.0);
    let min_color = vec3<f32>(1.0, 1.0, 1.0);
    let max_color = vec3<f32>(1.0, 1.0, 200.0 / 255.0);
    
    return min_color + (max_color - min_color) * t;
}

// Calculate edge vignetting effect
fn calculate_edge_vignetting(screen_pos: vec2<f32>) -> f32 {
    let angle_to_pos = angle_to_point(screen_pos);
    let angle_diff = abs(angle_to_pos - flashlight.angle);
    
    // Handle angle wrapping around 2π
    let wrapped_diff = min(angle_diff, 2.0 * 3.14159 - angle_diff);
    
    // Calculate how close we are to the edge of the cone
    let edge_factor = wrapped_diff / (flashlight.spread * 0.5);
    
    // Apply smooth falloff at the edges
    let vignette_strength = 0.6; // Controls how strong the vignetting is
    let vignette_softness = 0.3; // Controls how soft the transition is
    
    // Create smooth falloff using smoothstep
    let falloff_start = 1.0 - vignette_softness;
    let vignette = 1.0 - smoothstep(falloff_start, 1.0, edge_factor) * vignette_strength;
    
    return vignette;
}

fn hue_to_rgb(h: f32) -> vec3<f32> {
    let r = abs(h * 6.0 - 3.0) - 1.0;
    let g = 2.0 - abs(h * 6.0 - 2.0);
    let b = 2.0 - abs(h * 6.0 - 4.0);
    return clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn laser_beam(uv_angle: f32, beam_angle: f32, level: f32) -> f32 {
    let diff = abs(uv_angle - beam_angle);
    let wrapped = min(diff, 6.28318 - diff);
    return smoothstep(0.025, 0.005, wrapped) * level;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let screen_pos = uv_to_screen(in.uv);

    var final_color = vec3<f32>(0.0);
    var final_alpha = 0.0;

    // Flashlight cone contribution
    if (is_in_cone(screen_pos)) {
        // Calculate number of gradient layers
        let min_layers = 1.0;
        let max_layers = 10.0;
        let t_catch = clamp(flashlight.time_since_catch / 5.0, 0.0, 1.0);
        let num_layers = round(max_layers - (max_layers - min_layers) * t_catch);

        // Calculate edge vignetting
        let vignette = calculate_edge_vignetting(screen_pos);

        // Blend all layers
        for (var i = 0.0; i < num_layers; i += 1.0) {
            let layer_alpha = calculate_gradient_alpha(screen_pos, i, num_layers);
            if (layer_alpha > 0.0) {
                let layer_color = calculate_layer_color(i, num_layers);

                // Apply vignetting to the layer alpha
                let vignetted_alpha = layer_alpha * vignette;

                // Additive blending
                final_color += layer_color * vignetted_alpha;
                final_alpha += vignetted_alpha;
            }
        }
    }

    // Add laser beams if laser level > 0 — these can reach outside the cone
    if (flashlight.laser_level > 0.0) {
        let center = vec2<f32>(flashlight.center_x, flashlight.center_y);
        let to_point = screen_pos - center;
        let dist = length(to_point);
        let uv_angle = atan2(to_point.y, to_point.x);
        let laser_range = flashlight.range * (1.3 + 0.2 * flashlight.laser_level);

        if (dist > 0.0 && dist <= laser_range) {
            // Cycling rainbow hue driven by time
            let hue1 = fract((flashlight.time * 120.0 + flashlight.laser_level * 60.0) / 360.0);

            // Beam 1: along the flashlight direction (always present at laser_level >= 1)
            let beam1_intensity = laser_beam(uv_angle, flashlight.angle, flashlight.laser_level);
            if (beam1_intensity > 0.0) {
                let rgb1 = hue_to_rgb(hue1) * 0.7 + vec3<f32>(0.3);
                final_color += rgb1 * beam1_intensity * 1.5;
                final_alpha += beam1_intensity * 1.5;
            }

            // Beam 2: at 120° offset (laser_level >= 2)
            if (flashlight.laser_level >= 2.0) {
                let hue2 = fract(hue1 + 1.0 / 3.0);
                let beam2_angle = flashlight.angle + 2.09440; // 120° in radians
                let beam2_intensity = laser_beam(uv_angle, beam2_angle, flashlight.laser_level);
                if (beam2_intensity > 0.0) {
                    let rgb2 = hue_to_rgb(hue2) * 0.7 + vec3<f32>(0.3);
                    final_color += rgb2 * beam2_intensity * 1.5;
                    final_alpha += beam2_intensity * 1.5;
                }
            }

            // Beam 3: at 240° offset (laser_level >= 3)
            if (flashlight.laser_level >= 3.0) {
                let hue3 = fract(hue1 + 2.0 / 3.0);
                let beam3_angle = flashlight.angle + 4.18879; // 240° in radians
                let beam3_intensity = laser_beam(uv_angle, beam3_angle, flashlight.laser_level);
                if (beam3_intensity > 0.0) {
                    let rgb3 = hue_to_rgb(hue3) * 0.7 + vec3<f32>(0.3);
                    final_color += rgb3 * beam3_intensity * 1.5;
                    final_alpha += beam3_intensity * 1.5;
                }
            }
        }
    }

    // Discard fully transparent pixels
    if (final_alpha <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    // Clamp final alpha
    final_alpha = clamp(final_alpha, 0.0, 1.0);

    return vec4<f32>(final_color, final_alpha);
}
