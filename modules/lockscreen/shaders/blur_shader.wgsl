const SLOT_BITS: u32 = 12u;
const SLOT_MASK: u32 = (1u << SLOT_BITS) - 1u;
const GEN_BITS: u32 = 32u - SLOT_BITS;
const GEN_MASK: u32 = (1u << GEN_BITS) - 1u;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) style: vec4<u32>,
    @location(3) tex: vec4<u32>,
    @location(10) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv_tex: vec2<f32>,
    @location(2) @interpolate(flat) slot_plus_one: u32,
    @location(3) @interpolate(flat) gen: u32,
    @location(5) rect_size_px: vec2<f32>,
    @location(6) @interpolate(flat) blur_strength: u32,
}

struct Globals {
    window_size: vec2<f32>,
    mouse_pos: vec2<f32>,
    mouse_buttons: u32,
    time: f32,
    delta_time: f32,
    frame: u32,
    scale: f32,
}

var<push_constant> globals: Globals;

@group(0) @binding(0) var tex_arr: binding_array<texture_2d<f32>>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<storage, read> gens: array<u32>;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    let uv = vec2<f32>(in.uv.x, 1.0 - in.uv.y);
    let world_pos = in.position + (uv * in.size);

    let ndc = vec2<f32>(
        (world_pos.x / globals.window_size.x) * 2.0 - 1.0,
        1.0 - (world_pos.y / globals.window_size.y) * 2.0
    );

    let packed = in.tex.x;
    let scale = unpack2x16unorm(in.tex.y);
    let offs = unpack2x16unorm(in.tex.z);

    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.slot_plus_one = packed & SLOT_MASK;
    out.gen = packed >> SLOT_BITS;
    out.color = unpack4x8unorm(in.style.x);
    out.uv_tex = (uv * scale) + offs;
    out.rect_size_px = in.size * globals.scale;
    out.blur_strength = in.style.y;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let idx = in.slot_plus_one - 1u;

    // Safety check matching default pipeline
    if in.slot_plus_one == 0u || (gens[idx] & GEN_MASK) != in.gen {
        return vec4<f32>(0.0);
    }

    let strength = f32(in.blur_strength);
    let base_color = textureSample(tex_arr[idx], samp, in.uv_tex);

    // Fallback to unblurred if strength is effectively 0
    if strength <= 0.0 {
        return base_color * in.color;
    }

    // Step size in UV space per physical pixel
    let texel_size = 1.0 / in.rect_size_px;

    var color_accumulator = vec4<f32>(0.0);
    var total_weight = 0.0;

    // 5x5 Kernel Blur Loop
    for (var x: f32 = -2.0; x <= 2.0; x += 1.0) {
        for (var y: f32 = -2.0; y <= 2.0; y += 1.0) {
            // Scale offset by user-defined strength
            let offset = vec2<f32>(x, y) * (strength / 2.0) * texel_size;

            // Simple Gaussian weight approximation based on distance squared: $1 / (1 + d^2)$
            let weight = 1.0 / (1.0 + dot(vec2<f32>(x, y), vec2<f32>(x, y)));

            let sample_uv = in.uv_tex + offset;
            color_accumulator += textureSample(tex_arr[idx], samp, sample_uv) * weight;
            total_weight += weight;
        }
    }

    let blurred = color_accumulator / total_weight;
    let final_color = blurred * in.color;

    // Return with alpha premultiplied matching your UI blend state
    return vec4<f32>(final_color.rgb * final_color.a, final_color.a);
}
