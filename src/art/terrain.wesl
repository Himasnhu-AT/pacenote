// Pacenote terrain: the engine's splat map (4 layers, RGBA weights over the terrain) with full
// PBR layers: colour, normal (BC5, RG) and ORM (R = AO, G = roughness) per layer, sampled with
// world-XZ UVs. Each layer is sampled at two scales/rotations blended by low-frequency noise
// (breaks visible tiling), and transitions are sharpened by the layers' AO as a height cue.
// WESL (Bevy 0.20).
import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};
@if(VISIBILITY_RANGE_DITHER)
import bevy_pbr::render::pbr_functions::visibility_range_dither;

struct Params {
    // rgb: linear tint (1 = texture as is), a: roughness multiplier
    tint: array<vec4<f32>, 4>,
    // x: 1 / tiling metres, y: normal strength, z: AO strength, w: unused
    layer: array<vec4<f32>, 4>,
    // xy: Bevy-space XZ of the terrain's min corner, zw: 1 / size
    extent: vec4<f32>,
    // x: macro variation strength, y: macro frequency (1/m), z: detail blend (0..1), w: unused
    misc: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> p: Params;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var splat_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var splat_smp: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var c0: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var c1: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var c2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var c3: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var n0: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var n1: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(109) var n2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(110) var n3: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(111) var o0: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(112) var o1: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(113) var o2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(114) var o3: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(115) var smp: sampler;
// Light under the forest canopy (R: 1 open .. 0 dense), over the same extent as the splat.
@group(constants::MATERIAL_BIND_GROUP) @binding(116) var canopy_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(117) var canopy_smp: sampler;

fn hash21(q0: vec2<f32>) -> f32 {
    let q = fract(q0 * vec2<f32>(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

fn value_noise(x: vec2<f32>) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

struct Sample {
    color: vec3<f32>,
    // tangent-space normal xy
    n: vec2<f32>,
    ao: f32,
    rough: f32,
};

// Second UV set: rotated ~37°, scaled 0.41, offset.
fn uv2(uv: vec2<f32>) -> vec2<f32> {
    let c = 0.7986;
    let s = 0.6018;
    return vec2<f32>(c * uv.x - s * uv.y, s * uv.x + c * uv.y) * 0.41 + vec2<f32>(0.37, 0.71);
}

fn layer(i: u32, ct: texture_2d<f32>, nt: texture_2d<f32>, ot: texture_2d<f32>, wp: vec2<f32>, k: f32) -> Sample {
    let uv = wp * p.layer[i].x;
    let ub = uv2(uv);
    let ca = textureSample(ct, smp, uv).rgb;
    let cb = textureSample(ct, smp, ub).rgb;
    let na = textureSample(nt, smp, uv).rg * 2.0 - 1.0;
    let nb = textureSample(nt, smp, ub).rg * 2.0 - 1.0;
    let oa = textureSample(ot, smp, uv).rg;
    let ob = textureSample(ot, smp, ub).rg;
    var s: Sample;
    s.color = mix(ca, cb, k) * p.tint[i].rgb;
    s.n = mix(na, nb, k) * p.layer[i].y;
    let o = mix(oa, ob, k);
    s.ao = mix(1.0, o.r, p.layer[i].z);
    s.rough = o.g * p.tint[i].a;
    return s;
}

@fragment
fn fragment(vertex_output: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var in = vertex_output;
@if(VISIBILITY_RANGE_DITHER)
    visibility_range_dither(in.position, in.visibility_range_dither);
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let wp = in.world_position.xz;
    let suv = (wp - p.extent.xy) * p.extent.zw;
    var w = textureSample(splat_tex, splat_smp, suv);

    // Anti-tiling blend factor: low-frequency noise, kept away from 0/1 extremes.
    let k = smoothstep(0.25, 0.75, value_noise(wp * 0.045)) * p.misc.z;

    let s0 = layer(0u, c0, n0, o0, wp, k);
    let s1 = layer(1u, c1, n1, o1, wp, k);
    let s2 = layer(2u, c2, n2, o2, wp, k);
    let s3 = layer(3u, c3, n3, o3, wp, k);

    // Height-ish blend: crevices (low AO) of a layer give way to its neighbours first.
    let h = vec4<f32>(s0.ao, s1.ao, s2.ao, s3.ao);
    w = w * (0.35 + h);
    w = pow(w, vec4<f32>(2.2));
    w = w / max(w.r + w.g + w.b + w.a, 1e-4);

    var col = s0.color * w.r + s1.color * w.g + s2.color * w.b + s3.color * w.a;
    let tn = s0.n * w.r + s1.n * w.g + s2.n * w.b + s3.n * w.a;
    let ao = s0.ao * w.r + s1.ao * w.g + s2.ao * w.b + s3.ao * w.a;
    let rough = s0.rough * w.r + s1.rough * w.g + s2.rough * w.b + s3.rough * w.a;

    let freq = p.misc.y;
    let mv = value_noise(wp * freq) * 0.6 + value_noise(wp * freq * 4.7) * 0.4;
    col = col * (1.0 + p.misc.x * (mv - 0.5));

    // Forest floor in shade beyond the shadow distance (near trees cast real shadows).
    let light = textureSample(canopy_tex, canopy_smp, suv).r;
    let far = smoothstep(110.0, 170.0, distance(view.world_position.xyz, in.world_position.xyz));
    col = col * mix(1.0, 0.3, (1.0 - light) * far);

    // World-aligned tangent frame (UVs are world XZ; green = -Z).
    let N = normalize(in.world_normal);
    let T = normalize(vec3<f32>(1.0, 0.0, 0.0) - N * N.x);
    let B = cross(N, T);
    let tz = sqrt(max(1.0 - dot(tn, tn), 0.0));
    let nm = normalize(T * tn.x + B * tn.y + N * tz);

    pbr_input.material.base_color = vec4<f32>(col, 1.0);
    pbr_input.material.perceptual_roughness = clamp(rough, 0.05, 1.0);
    pbr_input.N = nm;
    pbr_input.diffuse_occlusion = pbr_input.diffuse_occlusion * ao;
    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
