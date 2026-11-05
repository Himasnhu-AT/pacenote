// Pacenote clouds (`sky.rs`): a dome centred on the camera, drawn after Bevy's atmosphere in the
// transparent pass (depth-tested, not written), so only sky pixels shade. Each pixel ray-marches
// the cloud shell (base..top altitude over a curved earth) in a few jittered steps (TAA resolves
// the noise); Low draws one sample of a flat layer. Output is premultiplied radiance × exposure.
// Stars are added where the clouds let them through. WESL (Bevy 0.20).
import bevy_pbr::render::{
    mesh_view_bindings::{view, globals},
    view_transformations::position_world_to_clip,
};

struct Clouds {
    // xyz: towards the light (sun or moon); w: 1 = moon
    sun_dir: vec4<f32>,
    // rgb: light illuminance on the clouds; w: daylight 0..1
    sun_rad: vec4<f32>,
    // rgb: sky radiance from above / ground bounce from below
    amb_top: vec4<f32>,
    amb_bot: vec4<f32>,
    // rgb: horizon haze radiance; w: distance (m) at which clouds are half haze
    haze: vec4<f32>,
    // coverage, extinction (1/m), base and top altitude (m)
    shape: vec4<f32>,
    // xy: wind offset (m), z: detail offset, w: steps (1 = flat layer)
    wind: vec4<f32>,
    // x: star radiance, y: sky rotation (rad), z: time (s), w: light taps
    stars: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> c: Clouds;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var weather_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var weather_smp: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var detail_tex: texture_3d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var detail_smp: sampler;

const DOME_R: f32 = 40000.0;
const EARTH_R: f32 = 6360000.0;
const PI: f32 = 3.14159265;
// Longest stretch of the shell one pixel marches (m): long, grazing paths near the horizon
// would otherwise spread the few steps over tens of kilometres.
const MAX_PATH: f32 = 14000.0;
// Map scales (m per texture tile): macro coverage, shape, detail volume.
const MACRO_TILE: f32 = 60000.0;
const SHAPE_TILE: f32 = 14000.0;
const DETAIL_TILE: f32 = 1700.0;

struct VOut {
    @builtin(position) position: vec4<f32>,
    @location(0) dir: vec3<f32>,
};

@vertex
fn vertex(@location(0) position: vec3<f32>) -> VOut {
    var out: VOut;
    let wp = view.world_position + position * DOME_R;
    out.position = position_world_to_clip(wp);
    out.dir = position;
    return out;
}

// Distance along `d` from a point at radius `r0` (direction-cosine `mu` with the up vector) to
// the sphere of radius `r` around the earth's centre (the far hit: we are inside it).
fn shell_hit(r0: f32, mu: f32, r: f32) -> f32 {
    let b = r0 * mu;
    let disc = b * b - r0 * r0 + r * r;
    return -b + sqrt(max(disc, 0.0));
}

fn remap(v: f32, lo: f32, hi: f32) -> f32 {
    return clamp((v - lo) / max(hi - lo, 1e-4), 0.0, 1.0);
}

// Cloud density (0..1) at world point `p` (altitude = p.y); `detail` erodes the edges.
fn density(p: vec3<f32>, detail: bool) -> f32 {
    let h = remap(p.y, c.shape.z, c.shape.w);
    let q = p.xz + c.wind.xy;
    let m = textureSampleLevel(weather_tex, weather_smp, q / MACRO_TILE, 0.0);
    let s = textureSampleLevel(weather_tex, weather_smp, q / SHAPE_TILE + vec2<f32>(0.31, 0.17), 0.0);
    // Local coverage: the global value moved by the macro map (clear and cloudy regions).
    let cov = clamp(c.shape.x + (m.r - 0.5) * 0.7, 0.0, 1.0);
    // Height profile: flat-ish bases, rounded tops; denser cells grow taller.
    let tall = mix(0.35, 1.0, s.b);
    let profile = remap(h, 0.0, 0.08) * remap(h, tall, tall * 0.55);
    // Puffy cells (B) round the Perlin-Worley lumps (G) into cumulus.
    let base_shape = s.g * 0.75 + s.b * 0.25;
    var d = remap(base_shape, 1.0 - cov * 1.25, 1.05 - cov * 0.4) * profile;
    if detail && d > 0.0 {
        let dp = (p + vec3<f32>(c.wind.z, 0.0, c.wind.y * 0.6)) / DETAIL_TILE;
        let n = textureSampleLevel(detail_tex, detail_smp, dp, 0.0).r;
        // Erode more at the bottom (wisps) than at the tops (cauliflower).
        d = remap(d, n * mix(0.45, 0.25, h), 1.0);
    }
    return d;
}

fn hg(cos_t: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * PI * pow(max(1.0 + g2 - 2.0 * g * cos_t, 1e-4), 1.5));
}

// Interleaved gradient noise, moved each frame (TAA averages the march jitter).
fn ign(px: vec2<f32>) -> f32 {
    let f = f32(globals.frame_count % 64u) * 5.588238;
    return fract(52.9829189 * fract(0.06711056 * (px.x + f) + 0.00583715 * (px.y + f)));
}

fn hash2(p: vec2<f32>) -> vec2<f32> {
    var q = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(q) * 43758.5453);
}

// A star field on a rotating sky: cube-face cells, about one star in twelve cells.
fn stars(d: vec3<f32>, px_angle: f32) -> vec3<f32> {
    // Rotate about the celestial pole (46° up, due north = −Z).
    let pole = normalize(vec3<f32>(0.0, 0.719, -0.695));
    let a = c.stars.y;
    let v = d * cos(a) + cross(pole, d) * sin(a) + pole * dot(pole, d) * (1.0 - cos(a));
    let av = abs(v);
    var uv: vec2<f32>;
    var face: f32;
    if av.x >= av.y && av.x >= av.z {
        uv = v.yz / av.x;
        face = select(0.0, 1.0, v.x > 0.0);
    } else if av.y >= av.z {
        uv = v.xz / av.y;
        face = select(2.0, 3.0, v.y > 0.0);
    } else {
        uv = v.xy / av.z;
        face = select(4.0, 5.0, v.z > 0.0);
    }
    let cells = 180.0;
    let g = (uv * 0.5 + 0.5) * cells;
    let cell = floor(g);
    let r = hash2(cell + face * 271.0);
    if r.x > 0.055 {
        return vec3<f32>(0.0);
    }
    let centre = cell + 0.2 + 0.6 * hash2(cell + face * 97.0 + 13.0);
    // Angular distance in pixels (a cell is ≈ 2/cells rad on the face).
    let dist_px = length(g - centre) * (2.0 / cells) / max(px_angle, 1e-5);
    let core = exp(-dist_px * dist_px * 1.6);
    // Magnitudes: many faint, few bright; slight colour spread; a little twinkle.
    let mag = pow(r.y, 6.0) * 6.0 + 0.25;
    let tw = 0.85 + 0.15 * sin(c.stars.z * (3.0 + r.y * 7.0) + r.x * 40.0);
    let tint = mix(vec3<f32>(1.0, 0.85, 0.7), vec3<f32>(0.75, 0.85, 1.0), fract(r.y * 13.7));
    return tint * core * mag * tw;
}

@fragment
fn fragment(in: VOut) -> @location(0) vec4<f32> {
    let d = normalize(in.dir);
    let cam = view.world_position;
    // Pixel footprint (rad), for star size.
    let px_angle = length(fwidth(d));

    var radiance = vec3<f32>(0.0);
    var trans = 1.0;

    let r0 = EARTH_R + cam.y;
    let mu = d.y;
    let base = c.shape.z;
    let top = c.shape.w;
    // Below the horizon there is no cloud above (and the ground draws there anyway).
    if mu > -0.02 && cam.y < base {
        let t0 = shell_hit(r0, mu, EARTH_R + base);
        let t1 = min(shell_hit(r0, mu, EARTH_R + top), t0 + MAX_PATH);
        let to_l = c.sun_dir.xyz;
        let cos_t = dot(d, to_l);
        // Forward lobe for the silver lining, a weak back lobe.
        let phase = mix(hg(cos_t, 0.65), hg(cos_t, -0.2), 0.35);
        let steps = max(c.wind.w, 1.0);
        let flat = steps < 1.5;
        // Up to the first hit, for the haze.
        var hit_t = t1;
        let jitter = ign(in.position.xy);
        let ds = (t1 - t0) / steps;
        // Extinction at full density; a flat layer stands in for the whole shell's thickness.
        let sigma = c.shape.y;
        for (var i = 0.0; i < steps; i += 1.0) {
            let t = select(t0 + (i + jitter) * ds, mix(t0, t1, 0.5), flat);
            let p = cam + d * t;
            let dens = density(p, !flat);
            if dens <= 0.001 {
                continue;
            }
            hit_t = min(hit_t, t);
            let h = remap(p.y, base, top);
            // Light: optical depth towards the light from 1-2 taps.
            var tau = 0.0;
            if c.stars.w >= 1.0 {
                tau += density(p + to_l * 180.0, false) * 180.0;
            }
            if c.stars.w >= 2.0 {
                tau += density(p + to_l * 650.0, false) * 470.0;
            }
            if flat {
                // A flat layer: thicker clouds are darker underneath.
                tau = dens * 900.0;
            } else {
                tau += dens * 60.0;
            }
            tau *= sigma;
            // Beer with a multiple-scattering floor (light leaks through thick cloud), and
            // the "powder" darkening of edges facing away from the light.
            let beer = max(exp(-tau), 0.3 * exp(-tau * 0.18));
            let powder = 1.0 - 0.6 * exp(-dens * 4.0);
            let sun = c.sun_rad.rgb * beer * mix(powder, 1.0, 0.5 + 0.5 * cos_t) * phase * 4.0;
            let amb = mix(c.amb_bot.rgb, c.amb_top.rgb, clamp(h * 1.3, 0.0, 1.0)) * mix(0.35, 1.0, h);
            let s = sun + amb;
            // Energy-conserving step (Hillaire): what this slab scatters, seen through what's
            // in front of it.
            let len = select(ds, (t1 - t0), flat);
            let st = exp(-dens * sigma * len);
            radiance += trans * s * (1.0 - st);
            trans *= st;
            if trans < 0.02 {
                break;
            }
        }
        // Far clouds melt into the horizon haze, and thin out towards it.
        let f = 1.0 - exp(-hit_t / c.haze.w);
        let a = 1.0 - trans;
        radiance = mix(radiance, c.haze.rgb * a, f * 0.85);
        trans = 1.0 - a * (1.0 - f * 0.6);
        // Fade out right at the horizon (the shell is a thin line there).
        let edge = smoothstep(-0.02, 0.04, mu);
        radiance *= edge;
        trans = mix(1.0, trans, edge);
    }

    if c.stars.x > 0.0 && mu > 0.0 {
        // Stars behind the clouds, dimmed near the horizon (extinction).
        let s = stars(d, px_angle) * c.stars.x * smoothstep(0.0, 0.25, mu);
        radiance += s * trans;
    }

    return vec4<f32>(radiance * view.exposure, 1.0 - trans);
}
