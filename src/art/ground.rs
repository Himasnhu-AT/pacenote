//! The ground: textured terrain splat layers per biome, the road ribbon (surface materials,
//! UVs along the road, darker compacted wheel lines, loose gravel at the edges and centre) and
//! a soft verge strip each side that blends the road edge into the terrain.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use engine::render_budget::{KeepDetail, Scenery};
use engine::world::SplatLayer;

use super::{Wet, pbr};
use crate::stage::{self, Biome, Road, RoadPos, Surface};
use crate::world::StageWorld;

/// Terrain layers for a biome: up to four, later ones paint over earlier ones.
pub fn layers(b: Biome, wet: bool) -> Vec<SplatLayer> {
    // `color` multiplies the texture ×2 in the engine's shader: 0.5 = the texture as is.
    let l = |name: &str, set: &str, tint: [f32; 3], rough: f32, slope: [f32; 2], noise: f32, weight: f32, tiling: f32| {
        let k = if wet { 0.8 } else { 1.0 };
        SplatLayer {
            name: name.into(),
            color: [tint[0] * k, tint[1] * k, tint[2] * k],
            roughness: if wet { rough * 0.7 } else { rough },
            texture: Some(format!("textures/{set}/color.ktx2")),
            tiling,
            slope,
            noise,
            weight,
            blend: 7.0,
            ..default()
        }
    };
    let rock = |set: &str, tint: [f32; 3], from: f32| l("rock", set, tint, 0.85, [from, 90.0], 0.0, 1.7, 9.0);
    match b {
        Biome::PineForest => vec![
            l("grass", "heath", [0.52, 0.55, 0.47], 0.95, [0.0, 90.0], 0.3, 1.0, 5.0),
            l("needles", "needles", [0.3, 0.29, 0.27], 0.95, [0.0, 28.0], 0.75, 0.7, 4.0),
            l("moss", "moss", [0.42, 0.46, 0.36], 0.95, [0.0, 22.0], 0.6, 0.75, 5.0),
            rock("rock", [0.5, 0.5, 0.5], 31.0),
        ],
        Biome::BirchHills => vec![
            l("meadow", "meadow", [0.5, 0.52, 0.46], 0.95, [0.0, 90.0], 0.3, 1.0, 5.0),
            l("moss", "moss", [0.46, 0.48, 0.4], 0.95, [0.0, 24.0], 0.6, 0.7, 5.0),
            l("leaves", "leaves", [0.44, 0.42, 0.38], 0.95, [0.0, 26.0], 0.75, 0.55, 4.0),
            rock("rock", [0.5, 0.5, 0.5], 30.0),
        ],
        Biome::Scrub => vec![
            l("dry", "heath", [0.64, 0.56, 0.36], 0.95, [0.0, 90.0], 0.3, 1.0, 5.0),
            l("dirt", "dirt", [0.48, 0.45, 0.41], 0.95, [0.0, 30.0], 0.65, 0.75, 6.0),
            l("gravel", "gravel_red", [0.48, 0.46, 0.43], 0.9, [0.0, 30.0], 0.75, 0.4, 4.0),
            rock("cliff", [0.5, 0.48, 0.46], 25.0),
        ],
        Biome::Moor => vec![
            l("heath", "heath", [0.5, 0.48, 0.42], 0.95, [0.0, 90.0], 0.3, 1.0, 5.0),
            l("grass", "meadow", [0.46, 0.46, 0.38], 0.95, [0.0, 18.0], 0.55, 0.8, 5.0),
            l("moss", "moss", [0.44, 0.44, 0.38], 0.95, [0.0, 20.0], 0.7, 0.5, 6.0),
            rock("rock", [0.48, 0.48, 0.48], 24.0),
        ],
        Biome::SnowForest => vec![
            l("snow", "snow", [0.52, 0.53, 0.56], 0.55, [0.0, 90.0], 0.2, 1.0, 6.0),
            l("needles", "needles", [0.3, 0.29, 0.28], 0.9, [0.0, 20.0], 0.8, 0.35, 4.0),
            rock("rock", [0.46, 0.46, 0.48], 33.0),
        ],
    }
}

/// Texture set and verge set for a road surface in a biome.
fn surface_sets(surf: Surface, biome: Biome) -> (&'static str, &'static str) {
    match (surf, biome) {
        (Surface::Tarmac, Biome::SnowForest) => ("tarmac", "snow"),
        (Surface::Tarmac, Biome::Scrub) => ("tarmac", "dirt"),
        (Surface::Tarmac, _) => ("tarmac", "shoulder"),
        (Surface::Mud, _) => ("mud", "shoulder"),
        (Surface::Snow, _) => ("snow", "snow"),
        (_, Biome::Scrub) => ("gravel_red", "dirt"),
        (_, Biome::SnowForest) => ("gravel", "snow"),
        _ => ("gravel", "shoulder"),
    }
}

/// Wheel-line shading across the road: (lateral metres from the centre, brightness).
/// Compacted lines are darker, loose stones gather in the middle and at the edges.
fn profile(surf: Surface) -> [(f32, f32); 7] {
    match surf {
        Surface::Tarmac => [(0.0, 1.0), (0.4, 1.0), (0.55, 0.93), (0.76, 0.88), (0.97, 0.93), (1.3, 1.0), (9.0, 1.04)],
        Surface::Snow => [(0.0, 1.03), (0.35, 1.0), (0.52, 0.82), (0.76, 0.7), (1.0, 0.82), (1.3, 0.97), (9.0, 1.04)],
        Surface::Mud => [(0.0, 1.06), (0.35, 1.0), (0.52, 0.8), (0.76, 0.62), (1.0, 0.8), (1.3, 0.98), (9.0, 1.08)],
        _ => [(0.0, 1.1), (0.35, 1.03), (0.52, 0.86), (0.76, 0.7), (1.0, 0.86), (1.3, 1.0), (9.0, 1.14)],
    }
}

fn brightness(surf: Surface, lat: f32) -> f32 {
    let p = profile(surf);
    let a = lat.abs();
    for w in p.windows(2) {
        if a <= w[1].0 {
            let t = (a - w[0].0) / (w[1].0 - w[0].0);
            return w[0].1 + (w[1].1 - w[0].1) * t;
        }
    }
    p[6].1
}

/// Lateral sample positions (metres, signed) across the road at half-width `e` (edge).
fn columns(e: f32) -> Vec<f32> {
    let inner = [0.0f32, 0.35, 0.52, 0.76, 1.0, 1.3, 1.7, 2.2, 2.8];
    let mut v: Vec<f32> = inner.iter().copied().filter(|&x| x < e - 0.25).collect();
    v.push(e);
    let mut all: Vec<f32> = v.iter().rev().map(|x| -x).filter(|x| *x != 0.0).collect();
    all.extend(v);
    all
}

/// The driving line wanders a little along the road.
fn wander(s: f32) -> f32 {
    0.12 * (s / 37.0).sin() + 0.07 * (s / 13.0 + 1.3).sin()
}

/// Spawns the road ribbon and verge strips.
pub fn spawn_road(world: &mut World, s: &StageWorld) {
    let road = &s.road;
    let wet = world.get_resource::<Wet>().is_some_and(|w| w.0);
    let mut road_mats: HashMap<&'static str, Handle<StandardMaterial>> = HashMap::new();
    let mut verge_mats: HashMap<&'static str, Handle<StandardMaterial>> = HashMap::new();
    let mut i = 0;
    while i + 1 < road.len() {
        let surf = road.surface[i];
        let mut j = i;
        while j + 1 < road.len() && road.surface[j] == surf && j - i < 100 {
            j += 1;
        }
        let (set, verge_set) = surface_sets(surf, s.def.biome);
        let mat = road_mats
            .entry(set)
            .or_insert_with(|| {
                pbr(world, set, |m| {
                    if wet {
                        m.base_color = Color::srgb(0.62, 0.62, 0.64);
                        m.perceptual_roughness = 0.42;
                    }
                    if set == "snow" {
                        m.perceptual_roughness = 0.75;
                    }
                })
            })
            .clone();
        let vmat = verge_mats
            .entry(verge_set)
            .or_insert_with(|| {
                pbr(world, verge_set, |m| {
                    m.alpha_mode = AlphaMode::Blend;
                    m.base_color = Color::srgb(0.8, 0.8, 0.8);
                    m.depth_bias = 2.0;
                    if wet {
                        m.base_color = Color::srgb(0.72, 0.72, 0.74);
                        m.perceptual_roughness = 0.6;
                    }
                })
            })
            .clone();
        let (center, mesh) = ribbon(road, i, j, surf);
        let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
        world.spawn((Name::new("road"), Mesh3d(h), MeshMaterial3d(mat), Transform::from_translation(center), Scenery, KeepDetail));
        for side in [-1.0f32, 1.0] {
            let (center, mesh) = verge(s, i, j, side);
            let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
            world.spawn((Name::new("verge"), Mesh3d(h), MeshMaterial3d(vmat.clone()), Transform::from_translation(center), Scenery, KeepDetail, NotShadowCaster));
        }
        i = j;
    }
}

fn station_pos(road: &Road, i: usize, lat: f32) -> RoadPos {
    RoadPos { i: i.min(road.len() - 2), t: if i == road.len() - 1 { 1.0 } else { 0.0 }, s: i as f32 * stage::STEP, lateral: lat }
}

/// Left of travel at a station.
fn left(road: &Road, i: usize) -> Vec3 {
    let d = road.dir[i];
    Vec3::new(d.y, 0.0, -d.x)
}

/// Road ribbon for stations a..=b, positioned around its centre (so distance-based detail and
/// transparency sorting see where it is). Returns (centre, mesh).
fn ribbon(road: &Road, a: usize, b: usize, surf: Surface) -> (Vec3, Mesh) {
    let center = (road.pts[a] + road.pts[b]) * 0.5;
    let (mut pos, mut uv, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut rows: Vec<u32> = Vec::new();
    for i in a..=b {
        let p = road.pts[i];
        let l = left(road, i);
        let e = road.width[i] * 0.5 + 0.4;
        let cols = columns(e);
        let st = i as f32 * stage::STEP;
        let w = wander(st);
        rows.push(pos.len() as u32);
        for &lat in &cols {
            let y = road.height_at(&station_pos(road, i, lat)) + 0.04;
            let q = Vec3::new(p.x, y, p.z) + l * lat;
            pos.push((q - center).to_array());
            uv.push([lat / 4.0, st / 4.0]);
            let k = brightness(surf, lat - w);
            col.push([k, k, k, 1.0]);
        }
    }
    // Rows differ in column count where the width changes: stitch handles it.
    for r in 0..rows.len() - 1 {
        let (r0, r1) = (rows[r], rows[r + 1]);
        let n0 = rows.get(r + 1).copied().unwrap_or(pos.len() as u32) - r0;
        let n1 = rows.get(r + 2).copied().unwrap_or(pos.len() as u32) - r1;
        stitch(&mut idx, r0, n0, r1, n1);
    }
    (center, finish(pos, uv, Some(col), idx))
}

/// Joins two rows of vertices (left to right) with triangles, whatever their counts.
fn stitch(idx: &mut Vec<u32>, r0: u32, n0: u32, r1: u32, n1: u32) {
    let (mut i, mut j) = (0u32, 0u32);
    while i + 1 < n0 || j + 1 < n1 {
        // Advance the row whose next vertex is "behind" (by normalised position).
        let ti = (i + 1) as f32 / (n0 - 1).max(1) as f32;
        let tj = (j + 1) as f32 / (n1 - 1).max(1) as f32;
        if j + 1 >= n1 || (i + 1 < n0 && ti <= tj) {
            idx.extend_from_slice(&[r0 + i, r1 + j, r0 + i + 1]);
            i += 1;
        } else {
            idx.extend_from_slice(&[r0 + i, r1 + j, r1 + j + 1]);
            j += 1;
        }
    }
}

/// A verge strip on one side (`side` −1 = right, +1 = left): from just inside the road edge
/// (transparent) over the edge (opaque) out onto the terrain (fading out).
fn verge(s: &StageWorld, a: usize, b: usize, side: f32) -> (Vec3, Mesh) {
    let road = &s.road;
    let center = (road.pts[a] + road.pts[b]) * 0.5 + left(road, (a + b) / 2) * side * (road.width[(a + b) / 2] * 0.5 + 1.0);
    // (offset from the ribbon edge, alpha)
    let prof = [(-0.7f32, 0.0f32), (-0.15, 0.85), (0.5, 0.9), (1.2, 0.55), (2.1, 0.0)];
    let (mut pos, mut uv, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in a..=b {
        let p = road.pts[i];
        let l = left(road, i);
        let e = road.width[i] * 0.5 + 0.4;
        let st = i as f32 * stage::STEP;
        // Ragged edge: the verge creeps in and out along the road.
        let rag = 0.25 * (st / 7.3 + side * 2.0).sin() + 0.15 * (st / 2.9).sin();
        for &(o, alpha) in &prof {
            let lat = side * (e + o + rag * (1.0 - (o + 0.7) / 2.8));
            let q = Vec3::new(p.x, 0.0, p.z) + l * lat;
            let ground = s.heights.height_at(q.x, q.z);
            let on_road = road.height_at(&station_pos(road, i, lat)) + 0.045;
            let y = if lat.abs() <= road.width[i] * 0.5 + 0.55 { on_road.max(ground + 0.03) } else { ground + 0.05 };
            pos.push((Vec3::new(q.x, y, q.z) - center).to_array());
            uv.push([lat / 3.0, st / 3.0]);
            col.push([1.0, 1.0, 1.0, alpha]);
        }
    }
    let n = prof.len() as u32;
    for r in 0..(b - a) as u32 {
        for c in 0..n - 1 {
            let (i0, i1) = (r * n + c, (r + 1) * n + c);
            // Winding so the face points up on both sides (columns run outwards: towards +lat
            // on the left, like the ribbon's, towards −lat on the right).
            if side > 0.0 {
                idx.extend_from_slice(&[i0, i1, i0 + 1, i0 + 1, i1, i1 + 1]);
            } else {
                idx.extend_from_slice(&[i0, i0 + 1, i1, i0 + 1, i1 + 1, i1]);
            }
        }
    }
    (center, finish(pos, uv, Some(col), idx))
}

fn finish(pos: Vec<[f32; 3]>, uv: Vec<[f32; 2]>, col: Option<Vec<[f32; 4]>>, idx: Vec<u32>) -> Mesh {
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    if let Some(c) = col {
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, c);
    }
    m.insert_indices(Indices::U32(idx));
    m.compute_smooth_normals();
    // Upward normals regardless of winding mistakes (the ground is never seen from below).
    if let Some(bevy::mesh::VertexAttributeValues::Float32x3(n)) = m.attribute_mut(Mesh::ATTRIBUTE_NORMAL) {
        for v in n.iter_mut() {
            if v[1] < 0.0 {
                *v = [-v[0], -v[1], -v[2]];
            }
        }
    }
    let _ = m.generate_tangents();
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_are_symmetric_and_sorted() {
        for e in [2.2f32, 2.9, 3.6, 4.5] {
            let c = columns(e);
            assert!(c.windows(2).all(|w| w[0] < w[1]), "{c:?}");
            assert_eq!(c.first().copied(), Some(-e));
            assert_eq!(c.last().copied(), Some(e));
            assert!(c.contains(&0.0));
        }
    }

    #[test]
    fn wheel_lines_are_darker_than_the_middle() {
        for s in [Surface::Gravel, Surface::Mud, Surface::Snow, Surface::Tarmac] {
            assert!(brightness(s, 0.76) < brightness(s, 0.0), "{s:?}");
            assert!(brightness(s, -0.76) < brightness(s, 3.0), "{s:?}");
        }
    }

    #[test]
    fn stitch_covers_rows_of_different_length() {
        let mut idx = Vec::new();
        stitch(&mut idx, 0, 5, 5, 7);
        assert_eq!(idx.len() / 3, 4 + 6);
        assert!(idx.iter().all(|&i| i < 12));
    }

    #[test]
    fn road_and_verges_build_for_every_biome() {
        for def in stage::stages().into_iter().filter(|d| [1, 5, 9].contains(&d.id)) {
            let s = crate::world::generate(&def);
            let n = s.road.len().min(120);
            let (_, m) = ribbon(&s.road, 0, n - 1, def.surface);
            assert!(m.count_vertices() > 100);
            for side in [-1.0, 1.0] {
                let (_, v) = verge(&s, 0, n - 1, side);
                assert_eq!(v.count_vertices(), n * 5);
            }
        }
    }
}
