//! The look of road conditions `open::furniture` places: potholes with broken edges, darker
//! patch repairs, wet tarmac (dark and glossy), mud, washed-out gravel with its corrugation,
//! shaded ice. Mesh patches just over the drawn road, soft-edged (alpha), merged per tile with
//! one material (not `ForwardDecal`: Low has no depth prepass).
//!
//! **Owner: stream B (ROADSIDE).** Design: `docs/festival/ART_BIBLE.md` §4 "Road conditions",
//! `docs/festival/WORLD_PASS.md` §4, `docs/festival/DECISIONS.md` W5.

use std::collections::BTreeMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::signs::{Mesher, jitter, spawn_tiles, tile_of};
use crate::open::OpenWorld;
use crate::open::furniture::{Patch, PatchKind};

pub struct ConditionsPlugin;

impl Plugin for ConditionsPlugin {
    fn build(&self, _app: &mut App) {}
}

/// How far over the physics surface the patches are drawn: the road ribbon's own lift
/// (`art::ground`, 4.5 cm) and a little more.
const LIFT: f32 = 0.06;

/// UVs into the 2-texel roughness map: glossy (wet, ice) or matt.
const GLOSSY: [f32; 2] = [0.25, 0.5];
const MATT: [f32; 2] = [0.75, 0.5];

#[derive(Resource, Clone)]
struct ConditionsMaterial(Handle<StandardMaterial>);

fn material(world: &mut World) -> Handle<StandardMaterial> {
    if let Some(m) = world.get_resource::<ConditionsMaterial>() {
        return m.0.clone();
    }
    // Metallic-roughness: G = roughness (0.06 | 0.88), B = metallic 0.
    let mut img = Image::new(Extent3d { width: 2, height: 1, depth_or_array_layers: 1 }, TextureDimension::D2, vec![255, 15, 0, 255, 255, 225, 0, 255], TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD);
    img.sampler = ImageSampler::nearest();
    let img = world.resource_mut::<Assets<Image>>().add(img);
    let m = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color: Color::WHITE,
        metallic_roughness_texture: Some(img),
        metallic: 1.0,
        perceptual_roughness: 1.0,
        alpha_mode: AlphaMode::Blend,
        depth_bias: 6.0,
        // (Seen from above only; whichever way the triangles wind.)
        cull_mode: None,
        ..default()
    });
    world.insert_resource(ConditionsMaterial(m.clone()));
    m
}

/// The drawn road's height at `p` (the physics surface where there's no road).
fn road_y(open: &OpenWorld, p: Vec2) -> f32 {
    match open.net.nearest(p, 12.0) {
        Some(n) => open.net.roads[n.road].road.height_at(&n.at),
        None => open.ground_height(p.x, p.y),
    }
}

/// Colour (sRGB, alpha) and roughness of a strip kind.
fn strip_look(kind: PatchKind) -> ([f32; 4], [f32; 2]) {
    match kind {
        PatchKind::Wet => ([0.10, 0.10, 0.11, 0.62], GLOSSY),
        PatchKind::Mud => ([0.27, 0.19, 0.11, 0.9], MATT),
        PatchKind::Washout => ([0.52, 0.47, 0.40, 0.8], MATT),
        PatchKind::Ice => ([0.82, 0.88, 0.94, 0.72], GLOSSY),
        PatchKind::Repair => ([0.13, 0.13, 0.13, 0.9], MATT),
        PatchKind::Pothole { .. } => ([0.05, 0.05, 0.05, 0.95], MATT),
    }
}

fn lin(c: [f32; 4]) -> [f32; 4] {
    let l = Color::srgba(c[0], c[1], c[2], c[3]).to_linear();
    [l.red, l.green, l.blue, l.alpha]
}

/// A capsule patch as a grid across and along it; alpha fades at the sides (and at the ends
/// when `open_a` / `open_b`: the first or last of its strip).
fn capsule(m: &mut Mesher, open: &OpenWorld, p: &Patch, open_a: bool, open_b: bool) {
    let (rgba, uv) = strip_look(p.kind);
    let d = p.b - p.a;
    let len = d.length();
    let dir = if len > 1e-3 { d / len } else { Vec2::X };
    let across = dir.perp();
    let cols = [-1.0f32, -0.8, -0.4, 0.0, 0.4, 0.8, 1.0];
    // Rows every 0.3 m on a washout (its ripples), else every 1 m; the ends reach a radius out
    // where the strip starts or stops.
    let step = if p.kind == PatchKind::Washout { 0.3 } else { 1.0 };
    let (s0, s1) = (if open_a { -p.radius * 0.6 } else { 0.0 }, len + if open_b { p.radius * 0.6 } else { 0.0 });
    let rows = (((s1 - s0) / step).ceil() as usize).max(1);
    let base = m_len(m);
    for r in 0..=rows {
        let s = s0 + (s1 - s0) * r as f32 / rows as f32;
        // End fade.
        let fa = if open_a { ((s - s0) / (p.radius * 0.8)).clamp(0.0, 1.0) } else { 1.0 };
        let fb = if open_b { ((s1 - s) / (p.radius * 0.8)).clamp(0.0, 1.0) } else { 1.0 };
        let ripple = if p.kind == PatchKind::Washout { 0.9 + 0.1 * libm::sinf(core::f32::consts::TAU * (p.s0 + s) / 0.9) } else { 1.0 };
        for &c in &cols {
            let q = p.a + dir * s + across * c * p.radius;
            // Ragged sides: the outer columns pull in a little, seeded by position.
            let rag = if c.abs() > 0.9 { 0.3 + 0.7 * jitter(Vec3::new(q.x, 0.0, q.y), 11) } else { 1.0 };
            let side = if c.abs() >= 1.0 { 0.0 } else if c.abs() > 0.7 { 0.55 * rag } else { 1.0 };
            let a = rgba[3] * side * fa * fb;
            let shade = ripple * (0.92 + 0.16 * jitter(Vec3::new(q.x, 1.0, q.y), 12));
            let col = lin([rgba[0] * shade, rgba[1] * shade, rgba[2] * shade, a]);
            m.v(Vec3::new(q.x, road_y(open, q) + LIFT, q.y), Vec3::Y, uv, col);
        }
    }
    let w = cols.len() as u32;
    for r in 0..rows as u32 {
        for c in 0..w - 1 {
            let k = base + r * w + c;
            m.idx(&[k, k + w, k + 1, k + 1, k + w, k + w + 1]);
        }
    }
}

fn m_len(m: &Mesher) -> u32 {
    m.len() as u32
}

/// A pothole: a dark ragged hole with a lighter broken rim fading into the road.
fn pothole(m: &mut Mesher, open: &OpenWorld, p: &Patch) {
    let n = 14;
    let centre = p.a;
    let seedp = Vec3::new(centre.x, 0.0, centre.y);
    let base = m_len(m);
    let y = |q: Vec2| road_y(open, q) + LIFT;
    let hole = lin([0.045, 0.043, 0.04, 0.97]);
    m.v(Vec3::new(centre.x, y(centre), centre.y), Vec3::Y, MATT, hole);
    for k in 0..n {
        let a = k as f32 / n as f32 * core::f32::consts::TAU;
        let r = p.radius * (0.78 + 0.34 * jitter(seedp, 20 + k as u32));
        let dir = Vec2::new(libm::cosf(a), libm::sinf(a));
        // The broken rim: lighter crumbs round the hole, fading into the road.
        for (f, col) in [(1.0f32, hole), (1.22, lin([0.46, 0.44, 0.41, 0.9])), (1.55, lin([0.3, 0.3, 0.29, 0.0]))] {
            let q = centre + dir * r * f;
            m.v(Vec3::new(q.x, y(q), q.y), Vec3::Y, MATT, col);
        }
    }
    for k in 0..n as u32 {
        let (a, b) = (base + 1 + k * 3, base + 1 + ((k + 1) % n as u32) * 3);
        m.idx(&[base, a, b]);
        for ring in 0..2 {
            let (a0, b0, a1, b1) = (a + ring, b + ring, a + ring + 1, b + ring + 1);
            m.idx(&[a0, a1, b0, b0, a1, b1]);
        }
    }
}

/// Draws `open.furniture`'s patches (nothing in headless runs).
pub fn spawn(world: &mut World, open: &OpenWorld) {
    if !super::visual(world) {
        return;
    }
    let t = std::time::Instant::now();
    let mat = material(world);
    let patches = &open.furniture.patches;
    let mut tiles: BTreeMap<(i32, i32), Mesher> = BTreeMap::new();
    // Repairs first, so the holes draw over them (alpha order within a mesh).
    let mut order: Vec<&Patch> = patches.iter().collect();
    order.sort_by_key(|p| !matches!(p.kind, PatchKind::Repair));
    for p in order {
        let c = p.centre();
        let m = tiles.entry(tile_of(Vec3::new(c.x, 0.0, c.y))).or_default();
        match p.kind {
            PatchKind::Pothole { .. } => pothole(m, open, p),
            kind => {
                let same = |q: &&Patch| std::mem::discriminant(&q.kind) == std::mem::discriminant(&kind);
                let open_a = !patches.iter().filter(same).any(|q| q.b == p.a);
                let open_b = !patches.iter().filter(same).any(|q| q.a == p.b);
                capsule(m, open, p, open_a, open_b);
            }
        }
    }
    let (meshes, tris) = spawn_tiles(world, tiles, mat, "road conditions", |_| {});
    info!("conditions: {} patches in {meshes} meshes, {tris} triangles ({:.0} ms)", patches.len(), t.elapsed().as_secs_f32() * 1000.0);
}
