// Far trees (`trees.rs`, impostor tier): one quad per tree, merged per 256 m chunk, turned
// about the vertical to face the camera (a cylindrical billboard: never seen edge-on, unlike
// crossed cards). Every vertex of a tree carries the tree's base point (POSITION), its corner
// in the picture (UV_0) and its corner offset in metres (UV_1: x across, y up).
//
// Each tree fades in by itself at the LOD1 range's end and out at the view distance, with the
// same 4x4 dither and level mapping as Bevy's `VisibilityRange` (measured from the same LOD
// eye to the same point, the tree's base), so the LOD1 entity fading out and this quad fading
// in cover complementary pixels: no gap, no double draw. WESL (Bevy 0.20).
import bevy_pbr::render::{
    mesh_functions,
    mesh_view_bindings::view,
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct Impostor {
    // Bevy `VisibilityRange` layout: fade in from x to y, fade out from z to w (metres).
    range: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> imp: Impostor;

// Same mapping as `mesh_functions::get_visibility_range_dither_level`.
fn dither_level(d: f32) -> i32 {
    let r = imp.range;
    let offset = select(-16, 0, d >= r.z);
    let bounds = select(r.xy, r.zw, d >= r.z);
    let level = i32(round((d - bounds.x) / max(bounds.y - bounds.x, 1e-3) * 16.0));
    return offset + clamp(level, 0, 16);
}

@vertex
fn vertex(v_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let v = decompress_vertex(v_in, v_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(v_in.instance_index);
    let base = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0)).xyz;
    let level = dither_level(length(view.lod_view_world_position.xyz - base));
    out.uv = v.uv;
    out.uv_b = vec2<f32>(f32(level), 0.0);
    if level <= -16 || level >= 16 {
        // Out of range: all four corners on one point, nothing rasterised.
        out.position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        out.world_position = vec4<f32>(base, 1.0);
        out.world_normal = vec3<f32>(0.0, 1.0, 0.0);
        return out;
    }
    // Face the camera that draws (its own position), turning about Y only.
    let to = view.world_position.xz - base.xz;
    let len = length(to);
    let f = select(vec2<f32>(0.0, 1.0), to / max(len, 1e-4), len > 1e-3);
    let right = vec3<f32>(f.y, 0.0, -f.x);
    let toward = vec3<f32>(f.x, 0.0, f.y);
    let off = v.uv_b;
    let wp = base + right * off.x + vec3<f32>(0.0, off.y, 0.0);
    out.world_position = vec4<f32>(wp, 1.0);
    out.position = position_world_to_clip(wp);
    // Soft foliage normal (as the cards' `soft`): towards the camera, out of the crown, up.
    let side = select(-1.0, 1.0, off.x >= 0.0);
    out.world_normal = normalize(toward * 0.45 + right * side * 0.5 + vec3<f32>(0.0, 0.35, 0.0));
    return out;
}

// Bevy's `visibility_range_dither` (4x4 ordered dither).
const DITHER: vec4<u32> = vec4<u32>(0x0a020800u, 0x060e040cu, 0x09010b03u, 0x050d070fu);

fn dither(frag_coord: vec4<f32>, level: i32) {
    if level == 0 {
        return;
    }
    if level <= -16 || level >= 16 {
        discard;
    }
    let c = vec2<u32>(floor(frag_coord.xy)) % 4u;
    let t = i32((DITHER[c.y] >> (c.x * 8u)) & 0xffu);
    if (level >= 0 && level + t >= 16) || (level < 0 && 1 + level + t <= 0) {
        discard;
    }
}

@fragment
fn fragment(vertex_output: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var in = vertex_output;
    dither(in.position, i32(round(in.uv_b.x)));
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
