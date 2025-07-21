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

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let screen_pos = uv_to_screen(in.uv);
    
    // Check if we're in the flashlight cone
    if (!is_in_cone(screen_pos)) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    
    // Calculate number of gradient layers
    let min_layers = 1.0;
    let max_layers = 10.0;
    let t_catch = clamp(flashlight.time_since_catch / 5.0, 0.0, 1.0);
    let num_layers = round(max_layers - (max_layers - min_layers) * t_catch);
    
    var final_color = vec3<f32>(0.0);
    var final_alpha = 0.0;
    
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
    
    // Add laser beams if laser level > 0
    if (flashlight.laser_level > 0.0) {
        let num_lasers = 2.0 + flashlight.laser_level * 2.0;
        let center = vec2<f32>(flashlight.center_x, flashlight.center_y);
        
        for (var i = 0.0; i < num_lasers; i += 1.0) {
            let t = i / num_lasers;
            // Reduce the spread multiplier to keep lasers more inward from the edges
            let laser_spread_factor = 0.75; // Use 75% of the cone spread instead of 100%
            let laser_angle = flashlight.angle - flashlight.spread * laser_spread_factor * 0.5 + flashlight.spread * laser_spread_factor * t;
            
            // Calculate distance to laser line
            let laser_dir = vec2<f32>(cos(laser_angle), sin(laser_angle));
            let to_point = screen_pos - center;
            let projection = dot(to_point, laser_dir);
            
            if (projection > 0.0 && projection <= flashlight.range * (1.2 + 0.2 * flashlight.laser_level)) {
                let perp_dist = length(to_point - laser_dir * projection);
                let laser_width = 6.0 + 2.0 * flashlight.laser_level;
                
                if (perp_dist <= laser_width * 0.5) {
                    // Use modulo to select color without dynamic indexing
                    var laser_color = vec3<f32>(1.0, 0.0, 1.0); // Default magenta
                    let color_selector = i32(i) % 5;
                    if (color_selector == 0) {
                        laser_color = vec3<f32>(1.0, 0.0, 1.0); // Magenta
                    } else if (color_selector == 1) {
                        laser_color = vec3<f32>(0.0, 1.0, 1.0); // Cyan
                    } else if (color_selector == 2) {
                        laser_color = vec3<f32>(1.0, 1.0, 0.0); // Yellow
                    } else if (color_selector == 3) {
                        laser_color = vec3<f32>(0.0, 1.0, 0.0); // Green
                    } else {
                        laser_color = vec3<f32>(1.0, 0.0, 0.0); // Red
                    }
                    
                    let laser_falloff = 1.0 - (perp_dist / (laser_width * 0.5));
                    
                    // Create pulsating rave effect for lasers
                    let pulse_freq = 3.0 + i * 1.5; // Different frequency for each laser
                    let pulse_phase = i * 0.8; // Phase offset for each laser
                    let pulse_base = sin(flashlight.time * pulse_freq + pulse_phase);
                    let pulse_secondary = sin(flashlight.time * pulse_freq * 2.3 + pulse_phase * 1.7);
                    
                    // Combine multiple sine waves for complex pulsing
                    let pulse_intensity = 0.7 + 0.3 * (pulse_base * 0.6 + pulse_secondary * 0.4);
                    
                    // Add quick strobe effect occasionally
                    let strobe_freq = 8.0 + i * 2.0;
                    let strobe = step(0.85, sin(flashlight.time * strobe_freq + pulse_phase));
                    let strobe_intensity = 1.0 + strobe * 0.8;
                    
                    // Combine all effects
                    let total_intensity = pulse_intensity * strobe_intensity;
                    
                    // Apply vignetting and pulsing effects
                    let vignetted_laser_falloff = laser_falloff * vignette * total_intensity;
                    
                    final_color += laser_color * vignetted_laser_falloff * 0.8;
                    final_alpha += vignetted_laser_falloff * 0.8;
                }
            }
        }
    }
    
    // Clamp final alpha
    final_alpha = clamp(final_alpha, 0.0, 1.0);
    
    return vec4<f32>(final_color, final_alpha);
}
