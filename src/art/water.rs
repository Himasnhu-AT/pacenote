//! The open world's river: a flat water surface along the river line (`open::land`), laid in
//! the shallow bed the land carves for it. Its level follows the bed (lowest ground across the
//! channel, smoothed along it) and its width runs out to where the banks rise above it, so the
//! terrain hides the edges.

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use engine::render_budget::{KeepDetail, Scenery};

use crate::world::StageWorld;

/// Water above the lowest point of the bed (m). The bed is a flat 12 m bottom 2.5 m below the
/// valley floor, its sides rising over the next 24 m: 0.6 m of water is ~27 m across.
const DEPTH: f32 = 0.6;
/// The water never reaches past this either side (m): the level drops where the banks are low.
const BANK: f32 = 16.0;
/// Margin the mesh runs on under the banks (m).
const MARGIN: f32 = 3.0;
/// Points per mesh (the line has a point every ~8 m: ~500 m chunks, culled separately).
const CHUNK: usize = 64;

/// The water level and half-widths (left, right) per point of `line`, for heights `height`.
pub fn profile(line: &[Vec2], height: impl Fn(f32, f32) -> f32) -> Vec<(f32, f32, f32)> {
    let n = line.len();
    let side = |i: usize| -> Vec2 {
        let a = line[i.saturating_sub(1)];
        let b = line[(i + 1).min(n - 1)];
        let d = (b - a).normalize_or_zero();
        Vec2::new(-d.y, d.x)
    };
    let at = |p: Vec2| height(p.x, p.y);
    // Never above the lower bank at `BANK` out, so the water stays in its channel.
    let cap: Vec<f32> = (0..n).map(|i| at(line[i] + side(i) * BANK).min(at(line[i] - side(i) * BANK)) - 0.2).collect();
    // Lowest ground across the channel, then smoothed along it.
    let raw: Vec<f32> = (0..n)
        .map(|i| {
            let (p, s) = (line[i], side(i));
            let bed = (-4..=4).map(|k| at(p + s * k as f32)).fold(f32::INFINITY, f32::min);
            (bed + DEPTH).min(cap[i])
        })
        .collect();
    let level: Vec<f32> = (0..n)
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(4), (i + 4).min(n - 1));
            (raw[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32).min(cap[i])
        })
        .collect();
    (0..n)
        .map(|i| {
            let (p, s, y) = (line[i], side(i), level[i]);
            // Out to where the bank rises above the water, plus a margin under the ground.
            let reach = |dir: f32| {
                let mut w = 1.0;
                while w < BANK && at(p + s * dir * w) <= y + 0.05 {
                    w += 1.0;
                }
                // On under the bank, but not out over ground that drops away again (the edge
                // would float in the air).
                let mut m = 0.0;
                while m < MARGIN && at(p + s * dir * (w + m + 0.5)) > y + 0.02 {
                    m += 0.5;
                }
                w + m
            };
            (y, reach(1.0), reach(-1.0))
        })
        .collect()
}

/// Where the water is, as rectangles along the line, the way `world::dressing` lays the roads
/// (centre, half extents across / along, yaw `atan2(dx, dz)`): trees, rocks and bushes keep
/// out of it.
pub fn channel(line: &[Vec2], height: impl Fn(f32, f32) -> f32) -> Vec<(Vec2, Vec2, f32)> {
    let prof = profile(line, height);
    line.windows(2)
        .zip(prof.windows(2))
        .map(|(l, p)| {
            let d = l[1] - l[0];
            let half_w = p[0].1.max(p[0].2).max(p[1].1).max(p[1].2) - MARGIN + 1.5;
            ((l[0] + l[1]) * 0.5, Vec2::new(half_w, d.length() * 0.5 + 1.0), libm::atan2f(d.x, d.y))
        })
        .collect()
}

/// Spawns the river's water meshes (open world only; nothing for the old stages).
pub fn spawn(world: &mut World, s: &StageWorld) {
    let Some(open) = s.open.as_ref() else { return };
    let line = &open.net.land.river;
    if line.len() < 2 {
        return;
    }
    let hf = open.heights.clone();
    let prof = profile(line, |x, z| hf.height_at(x, z));
    // Dark, clear water; the mud set's normal map, tiled small and alone, reads as ripples.
    let mat = super::pbr(world, "mud", |m| {
        m.base_color = Color::srgba(0.03, 0.09, 0.08, 0.86);
        m.base_color_texture = None;
        m.occlusion_texture = None;
        m.metallic_roughness_texture = None;
        m.metallic = 0.0;
        m.perceptual_roughness = 0.12;
        m.reflectance = 0.35;
        m.alpha_mode = AlphaMode::Blend;
    });
    let n = line.len();
    let mut start = 0;
    while start + 1 < n {
        let end = (start + CHUNK).min(n - 1);
        let (center, mesh) = strip(line, &prof, start, end);
        let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
        world.spawn((Name::new("river water"), Mesh3d(h), MeshMaterial3d(mat.clone()), Transform::from_translation(center), Scenery, KeepDetail, NotShadowCaster));
        start = end;
    }
    info!("river: {} points, water {:.1}..{:.1} m", n, prof.iter().map(|p| p.0).fold(f32::MAX, f32::min), prof.iter().map(|p| p.0).fold(f32::MIN, f32::max));
}

/// The water from point `a` to `b` (inclusive): three vertices across (left bank, line, right
/// bank) per point, around their centre.
fn strip(line: &[Vec2], prof: &[(f32, f32, f32)], a: usize, b: usize) -> (Vec3, Mesh) {
    let n = line.len();
    let mut pos = Vec::new();
    let mut uv = Vec::new();
    let mut along = 0.0;
    for i in a..=b {
        let d = (line[(i + 1).min(n - 1)] - line[i.saturating_sub(1)]).normalize_or_zero();
        let s = Vec2::new(-d.y, d.x);
        let (y, wl, wr) = prof[i];
        if i > a {
            along += line[i].distance(line[i - 1]);
        }
        for (off, u) in [(wl, 0.0), (0.0, 0.5), (-wr, 1.0)] {
            let p = line[i] + s * off;
            pos.push(Vec3::new(p.x, y, p.y));
            // Texture repeats every ~4 m across and along.
            uv.push([u * (wl + wr) / 4.0, along / 4.0]);
        }
    }
    let (lo, hi) = pos.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(l, h), p| (l.min(*p), h.max(*p)));
    let c = (lo + hi) * 0.5;
    let rows = (b - a) as u32;
    let mut idx = Vec::new();
    for r in 0..rows {
        let (k, m) = (r * 3, r * 3 + 3);
        for j in 0..2 {
            // Wound so the face points up whichever way `s` lies.
            idx.extend_from_slice(&[k + j, m + j, k + j + 1, k + j + 1, m + j, m + j + 1]);
        }
    }
    let pos_local: Vec<[f32; 3]> = pos.iter().map(|p| (*p - c).to_array()).collect();
    let up = vec![[0.0, 1.0, 0.0]; pos_local.len()];
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos_local);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, up);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_indices(Indices::U32(idx));
    // Make sure the triangles face up (the winding depends on which way the line runs).
    if let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        && p.len() >= 4
    {
        let (p0, p1, p3) = (Vec3::from(p[0]), Vec3::from(p[3]), Vec3::from(p[1]));
        if (p1 - p0).cross(p3 - p0).y < 0.0
            && let Some(Indices::U32(ix)) = mesh.indices_mut()
        {
            for t in ix.as_chunks_mut::<3>().0 {
                t.swap(1, 2);
            }
        }
    }
    // The normal map needs tangents.
    if let Err(e) = mesh.generate_tangents() {
        warn!("river: no tangents ({e})");
    }
    (c, mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_sits_in_the_bed_and_reaches_the_banks() {
        // A straight channel 2.5 m deep, 10 m wide at the bottom, banks rising either side.
        let line: Vec<Vec2> = (0..40).map(|i| Vec2::new(i as f32 * 8.0, 0.0)).collect();
        let ground = |_x: f32, z: f32| 100.0 - 2.5 * (1.0 - ((z.abs() - 5.0) / 15.0).clamp(0.0, 1.0));
        for (y, l, r) in profile(&line, ground) {
            assert!((y - 98.1).abs() < 0.01, "level {y}");
            // The bank passes the water at |z| = 5 + 15 * 0.6 / 2.5 = 8.6: reach 9 + 3.
            assert!((l - 12.0).abs() < 0.01 && (r - 12.0).abs() < 0.01, "{l} {r}");
        }
    }

    #[test]
    fn low_banks_lower_the_water() {
        // A shallow dip 0.3 m deep: the water stays under the banks at 16 m.
        let line: Vec<Vec2> = (0..20).map(|i| Vec2::new(i as f32 * 8.0, 0.0)).collect();
        let ground = |_x: f32, z: f32| 50.0 - 0.3 * (1.0 - (z.abs() / 30.0).clamp(0.0, 1.0));
        for (y, l, r) in profile(&line, ground) {
            assert!(y < ground(0.0, 16.0), "level {y}");
            assert!(l <= BANK + MARGIN && r <= BANK + MARGIN);
        }
    }
}
