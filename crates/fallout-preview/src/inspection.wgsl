// Source alpha comparison precedes blending, including when both are enabled.
// This shader deliberately leaves retail lighting and controller effects open.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::main_pass_post_lighting_processing,
    forward_io::{VertexOutput, FragmentOutput},
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> source_alpha: vec4<u32>;

fn passes_alpha(value: f32, threshold: f32, function: u32) -> bool {
    switch function {
        case 0u: { return true; }
        case 1u: { return value < threshold; }
        case 2u: { return value == threshold; }
        case 3u: { return value <= threshold; }
        case 4u: { return value > threshold; }
        case 5u: { return value != threshold; }
        case 6u: { return value >= threshold; }
        default: { return false; }
    }
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let input = pbr_input_from_standard_material(in, is_front);
    let color = input.material.base_color;
    if ((source_alpha.x & 0x200u) != 0u) {
        let threshold = f32(source_alpha.y) / 255.0;
        if (!passes_alpha(color.a, threshold, (source_alpha.x >> 10u) & 7u)) {
            discard;
        }
    }
    var result: FragmentOutput;
    result.color = main_pass_post_lighting_processing(input, color);
    return result;
}
