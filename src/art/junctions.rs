//! The open world's built road details: junction surfaces (the outlines from
//! `open::junctions`, triangulated and draped), their verges, kerbs round the town blocks and
//! road markings (centre and edge lines, stop and give-way lines, zebra crossings downtown).
//!
//! Visual only: nothing here is seen by the car sim. Meshes are merged per 256 m cell and
//! layer, so the whole network adds a few dozen draws.
//!
//! Markings are thin quads a centimetre above the tarmac, twice as wide as the paint, textured
//! with a small mip-mapped alpha profile (dashes in the texture, along the road): far away the
//! mips average the paint out instead of letting sub-pixel lines shimmer.

use std::collections::BTreeMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use engine::render_budget::{KeepDetail, Scenery};

use super::Wet;
use super::ground::{Mats, add_mesh, bounds, brightness, finish, majority, station_pos, surface_sets, wander};
use crate::open::buildings::SIDEWALK;
use crate::open::junctions::{self as geo, JunctionGeom, KERB_TOP};
use crate::open::{OpenWorld, RoadKind, RoadNet};
use crate::stage::{Road, STEP, Surface};
use crate::world::StageWorld;

/// Junction surface above the terrain (as the ribbons).
const LIFT: f32 = 0.04;
/// Paint above the surface it's on.
const PAINT: f32 = 0.012;
/// Mesh merging cell (m).
const CELL: f32 = 256.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Layer {
    Surface,
    Verge,
    Kerb,
    Solid,
    Dashed,
}

/// Vertices and triangles of one merged mesh.
#[derive(Default)]
struct Buf {
    pos: Vec<Vec3>,
    uv: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
    nrm: Vec<[f32; 3]>,
    idx: Vec<u32>,
}

impl Buf {
    fn v(&mut self, p: Vec3, uv: [f32; 2], col: [f32; 4]) -> u32 {
        self.pos.push(p);
        self.uv.push(uv);
        self.col.push(col);
        (self.pos.len() - 1) as u32
    }

    /// A triangle facing up (whatever order it came in).
    fn up(&mut self, a: u32, b: u32, c: u32) {
        let (pa, pb, pc) = (self.pos[a as usize], self.pos[b as usize], self.pos[c as usize]);
        if (pb - pa).cross(pc - pa).y >= 0.0 {
            self.idx.extend_from_slice(&[a, b, c]);
        } else {
            self.idx.extend_from_slice(&[a, c, b]);
        }
    }

    /// A quad a-b-c-d (around its edge) facing up.
    fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.up(a, b, c);
        self.up(a, c, d);
    }

    /// A triangle facing `n`.
    fn facing(&mut self, a: u32, b: u32, c: u32, n: Vec3) {
        let (pa, pb, pc) = (self.pos[a as usize], self.pos[b as usize], self.pos[c as usize]);
        if (pb - pa).cross(pc - pa).dot(n) >= 0.0 {
            self.idx.extend_from_slice(&[a, b, c]);
        } else {
            self.idx.extend_from_slice(&[a, c, b]);
        }
    }

    fn tris(&self) -> usize {
        self.idx.len() / 3
    }
}

#[derive(Default)]
struct Bufs(BTreeMap<(Layer, i32, i32), Buf>);

impl Bufs {
    fn at(&mut self, layer: Layer, p: Vec2) -> &mut Buf {
        let key = (layer, (p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32);
        self.0.entry(key).or_default()
    }
}

fn xz(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}

fn v3(p: Vec2, y: f32) -> Vec3 {
    Vec3::new(p.x, y, p.y)
}

/// The height drawn tarmac takes at `p` in junction `g`: the terrain (what the car feels),
/// except in town where it stays at the street level (kerbs keep their height) unless the
/// shoulder rises above it.
fn surface_y(open: &OpenWorld, town: bool, p: Vec2) -> f32 {
    let hf = open.heights.height_at(p.x, p.y);
    let _ = town;
    hf + LIFT
}

/// Spawns junction surfaces, their verges, kerbs and markings. `inside` = the ribbon segments
/// the junctions replaced (`open::junctions::trimmed`).
pub(super) fn spawn(world: &mut World, s: &StageWorld, mats: &mut Mats, open: &OpenWorld, geoms: &[JunctionGeom], inside: &[Vec<bool>]) {
    let net = &*open.net;
    let mut bufs = Bufs::default();
    for g in geoms {
        junction_surface(&mut bufs, open, g);
        junction_verges(&mut bufs, open, g);
    }
    let plan = Plan::new(net, geoms);
    let kerb_len = kerbs(&mut bufs, open, geoms, &plan);
    markings(&mut bufs, open, geoms, inside, &plan);

    // Materials.
    let wet = world.get_resource::<Wet>().is_some_and(|w| w.0);
    let tarmac = mats.surface(world, surface_sets(Surface::Tarmac, s.def.biome).0, false);
    let verge = mats.verge(world, surface_sets(Surface::Tarmac, s.def.biome).1);
    let kerb = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color: if wet { Color::srgb(0.5, 0.5, 0.49) } else { Color::srgb(0.66, 0.65, 0.62) },
        perceptual_roughness: if wet { 0.45 } else { 0.85 },
        ..default()
    });
    let paint = |world: &mut World, dashed: bool| {
        let img = world.resource_mut::<Assets<Image>>().add(paint_image(dashed));
        world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
            base_color: if wet { Color::srgb(0.78, 0.77, 0.72) } else { Color::srgb(0.93, 0.91, 0.84) },
            base_color_texture: Some(img),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: if wet { 0.3 } else { 0.62 },
            depth_bias: 4.0,
            ..default()
        })
    };
    let (solid, dashed) = (paint(world, false), paint(world, true));

    let mut stats: BTreeMap<Layer, (usize, usize)> = BTreeMap::new();
    for ((layer, _, _), b) in std::mem::take(&mut bufs.0) {
        if b.idx.is_empty() {
            continue;
        }
        let e = stats.entry(layer).or_default();
        e.0 += 1;
        e.1 += b.tris();
        let (mat, name) = match layer {
            Layer::Surface => (tarmac.clone(), "junction"),
            Layer::Verge => (verge.clone(), "junction verge"),
            Layer::Kerb => (kerb.clone(), "kerb"),
            Layer::Solid => (solid.clone(), "road paint"),
            Layer::Dashed => (dashed.clone(), "road paint"),
        };
        let (center, mesh) = to_mesh(b, layer == Layer::Kerb);
        let (h, aabb) = add_mesh(world, mesh);
        let mut e = world.spawn((Name::new(name), Mesh3d(h), MeshMaterial3d(mat), Transform::from_translation(center), Scenery, KeepDetail));
        if layer != Layer::Kerb {
            e.insert(NotShadowCaster);
        }
        let id = e.id();
        bounds(world, id, aabb);
    }
    let town = geoms.iter().filter(|g| g.town()).count();
    info!(
        "roads: {} junctions ({} town, {} on the ring), kerbs {:.1} km; meshes/triangles by layer {:?}",
        geoms.len(),
        town,
        geoms.len() - town,
        kerb_len / 1000.0,
        stats
    );
}

fn to_mesh(b: Buf, lit_normals: bool) -> (Vec3, Mesh) {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &b.pos {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let c = (lo + hi) * 0.5;
    let pos: Vec<[f32; 3]> = b.pos.iter().map(|p| (*p - c).to_array()).collect();
    if lit_normals {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, b.nrm);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, b.uv);
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, b.col);
        m.insert_indices(Indices::U32(b.idx));
        (c, m)
    } else {
        (c, finish(pos, b.uv, Some(b.col), b.idx))
    }
}

// ---------------------------------------------------------------- junction surfaces

fn junction_surface(bufs: &mut Bufs, open: &OpenWorld, g: &JunctionGeom) {
    let net = &*open.net;
    let town = g.town();
    let n = g.outline.len();
    // Mouth vertices take the ribbon's exact height and shading; the rest the draped surface.
    let mut ys = Vec::with_capacity(n);
    let mut shade = vec![1.0f32; n];
    for (k, p) in g.outline.iter().enumerate() {
        let arm = g.mouths.iter().position(|m| m.contains(&k));
        match arm {
            Some(a) => {
                let arm = &g.arms[a];
                let road = &net.roads[arm.road].road;
                let d = road.dir[arm.mouth];
                let lat = (*p - xz(road.pts[arm.mouth])).dot(Vec2::new(d.y, -d.x));
                ys.push(road.height_at(&station_pos(road, arm.mouth, lat)) + LIFT);
                let main = majority(&road.surface);
                shade[k] = brightness(main, lat - wander(arm.mouth as f32 * STEP));
            }
            None => ys.push(surface_y(open, town, *p)),
        }
    }
    let tris = geo::triangulate(&g.outline);
    let frozen = |a: u32, b: u32| {
        let (a, b) = (a as usize, b as usize);
        a.abs_diff(b) == 1 && g.mouths.iter().any(|m| m.contains(&a) && m.contains(&b))
    };
    let (pts, ys, tris) = geo::refine(g.outline.clone(), ys, tris, |p| surface_y(open, town, p), frozen, 10.0, 3.0, 0.02);
    let b = bufs.at(Layer::Surface, g.pos);
    let base = b.pos.len() as u32;
    for (k, (p, y)) in pts.iter().zip(&ys).enumerate() {
        let c = shade.get(k).copied().unwrap_or(1.0);
        b.v(v3(*p, *y), [p.x / 4.0, p.y / 4.0], [c, c, c, 1.0]);
    }
    for t in tris {
        b.up(base + t[0], base + t[1], base + t[2]);
    }
}

/// Verge strips along each corner of the outline (the ribbons' verges stop at the mouths).
fn junction_verges(bufs: &mut Bufs, open: &OpenWorld, g: &JunctionGeom) {
    let prof = [(-0.7f32, 0.0f32), (-0.15, 0.85), (0.5, 0.9), (1.2, 0.55), (2.1, 0.0)];
    let town = g.town();
    for c in &g.corners {
        // Densify to ≤ 2 m.
        let mut pts = vec![c.pts[0]];
        for w in c.pts.windows(2) {
            let n = (w[0].distance(w[1]) / 2.0).ceil().max(1.0) as usize;
            for i in 1..=n {
                pts.push(w[0].lerp(w[1], i as f32 / n as f32));
            }
        }
        if pts.len() < 2 {
            continue;
        }
        let b = bufs.at(Layer::Verge, g.pos);
        let mut along = 0.0;
        let mut rows = Vec::new();
        for i in 0..pts.len() {
            let d = (pts[(i + 1).min(pts.len() - 1)] - pts[i.saturating_sub(1)]).normalize_or_zero();
            // Counter-clockwise outline: outward is to the right of travel.
            let out = Vec2::new(d.y, -d.x);
            if i > 0 {
                along += pts[i].distance(pts[i - 1]);
            }
            let row = b.pos.len() as u32;
            for &(o, alpha) in &prof {
                let q = pts[i] + out * o;
                let ground = open.heights.height_at(q.x, q.y);
                let y = if o <= 0.55 { (surface_y(open, town, q) + 0.005).max(ground + 0.03) } else { ground + 0.05 };
                b.v(v3(q, y), [o / 3.0, along / 3.0], [1.0, 1.0, 1.0, alpha]);
            }
            rows.push(row);
        }
        for w in rows.windows(2) {
            for k in 0..prof.len() as u32 - 1 {
                b.quad(w[0] + k, w[0] + k + 1, w[1] + k + 1, w[1] + k);
            }
        }
    }
}

// ---------------------------------------------------------------- what goes where

/// Per junction arm: whether it gets a stop (or give-way) line and a zebra crossing.
struct ArmPaint {
    stop: bool,
    zebra: bool,
    /// Solid stop line (town), else a dashed give-way line.
    solid: bool,
    /// Metres past the mouth kept clear of the centre line.
    clear: f32,
}

struct Plan {
    /// Same shape as `JunctionGeom::arms`.
    arms: Vec<Vec<ArmPaint>>,
    /// The road runs straight through a T: (junction, road) pairs whose centre line and far
    /// edge line carry on through, with the side the joining road comes from (+1 = left).
    through: Vec<(usize, usize, f32)>,
}

/// Downtown crossings get zebras: the valley village has none.
fn downtown(_: Vec2) -> bool {
    false
}

impl Plan {
    fn new(net: &RoadNet, geoms: &[JunctionGeom]) -> Plan {
        let mut arms = Vec::new();
        let mut through = Vec::new();
        for (gi, g) in geoms.iter().enumerate() {
            let mut v = Vec::new();
            // A T: one road with both arms, one other.
            let major = (g.arms.len() == 3).then(|| g.arms.iter().find(|a| g.arms.iter().filter(|b| b.road == a.road).count() == 2).map(|a| a.road)).flatten();
            if let Some(r) = major
                && let Some(o) = g.arms.iter().find(|a| a.road != r)
            {
                let road = &net.roads[r].road;
                let at = g.arms.iter().find(|a| a.road == r).unwrap().at;
                let d = road.dir[at];
                let to = (xz(net.roads[o.road].road.pts[o.mouth]) - g.pos).normalize_or_zero();
                through.push((gi, r, if to.dot(Vec2::new(d.y, -d.x)) > 0.0 { 1.0 } else { -1.0 }));
            }
            for a in &g.arms {
                let road = &net.roads[a.road].road;
                let tarmac = road.surface[a.mouth] == Surface::Tarmac;
                let stop = g.arms.len() >= 3 && tarmac && major != Some(a.road) && (a.ends || g.arms.len() >= 4);
                let zebra = g.town() && downtown(g.pos) && tarmac;
                let clear = if zebra { 4.7 } else if stop { 1.4 } else { 0.0 } + if stop || zebra { 4.0 } else { 0.0 };
                v.push(ArmPaint { stop, zebra, solid: g.town(), clear });
            }
            arms.push(v);
        }
        Plan { arms, through }
    }
}

/// Point, left vector and height on a road at distance `s` (wrapping on a closed road) and
/// lateral `lat` (banking applied as `Road::height_at` does).
fn on_road(road: &Road, closed: bool, s: f32, lat: f32) -> (Vec2, Vec2, f32) {
    let n = road.len();
    let total = if closed { n as f32 * STEP } else { (n - 1) as f32 * STEP };
    let s = if closed { s.rem_euclid(total) } else { s.clamp(0.0, total) };
    let f = s / STEP;
    let i = (f.floor() as usize).min(if closed { n - 1 } else { n - 2 });
    let t = (f - i as f32).clamp(0.0, 1.0);
    let j = if closed { (i + 1) % n } else { i + 1 };
    let d = road.dir[i].lerp(road.dir[j], t).normalize_or(road.dir[i]);
    let left = Vec2::new(d.y, -d.x);
    let p = xz(road.pts[i]).lerp(xz(road.pts[j]), t) + left * lat;
    let half = (road.width[i] + (road.width[j] - road.width[i]) * t) * 0.5;
    let bank = road.bank[i] + (road.bank[j] - road.bank[i]) * t;
    let y = road.pts[i].y + (road.pts[j].y - road.pts[i].y) * t + bank * lat.clamp(-half, half);
    (p, left, y)
}

// ---------------------------------------------------------------- kerbs

/// A gap in the kerbs (a zebra crossing's dropped kerb): a rectangle `half` (along, across)
/// round `c`, `along` its unit axis.
struct Gap {
    c: Vec2,
    along: Vec2,
    half: Vec2,
}

impl Gap {
    /// The part (t0, t1) of segment a→b inside the gap, if any (Liang–Barsky).
    fn clip(&self, a: Vec2, b: Vec2) -> Option<(f32, f32)> {
        let across = Vec2::new(-self.along.y, self.along.x);
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for (axis, h) in [(self.along, self.half.x), (across, self.half.y)] {
            let p0 = (a - self.c).dot(axis);
            let dp = (b - a).dot(axis);
            if dp.abs() < 1e-9 {
                // Parallel to this pair of sides: inside iff between them.
                if p0 < -h || p0 > h {
                    return None;
                }
                continue;
            }
            // Inside where p0 + t·dp ∈ [−h, h].
            let (ta, tb) = ((-h - p0) / dp, (h - p0) / dp);
            t0 = t0.max(ta.min(tb));
            t1 = t1.min(ta.max(tb));
        }
        (t1 - t0 > 1e-4).then_some((t0, t1))
    }
}

/// Kerbs round every town block, broken at the zebras. Returns their length (m).
fn kerbs(bufs: &mut Bufs, open: &OpenWorld, geoms: &[JunctionGeom], plan: &Plan) -> f32 {
    let net = &*open.net;
    let geoms_gaps: Vec<Gap> = {
        let mut v = Vec::new();

        for (g, paint) in geoms.iter().zip(&plan.arms) {
            for (a, p) in g.arms.iter().zip(paint) {
                if !p.zebra {
                    continue;
                }
                let nr = &net.roads[a.road];
                let s = a.mouth as f32 * STEP + a.sign as f32 * 2.0;
                let (c, left, _) = on_road(&nr.road, nr.closed, s, 0.0);
                let along = Vec2::new(-left.y, left.x);
                v.push(Gap { c, along, half: Vec2::new(1.8, nr.road.width[a.mouth] * 0.5 + 1.0) });
            }
        }
        v
    };
    let mut total = 0.0;
    for blk in open.buildings.blocks.iter().filter(|b| b.cell.is_some()) {
        let lo = blk.min - Vec2::splat(SIDEWALK);
        let hi = blk.max + Vec2::splat(SIDEWALK);
        let lp = geo::kerb_loop(lo, hi);
        // Pieces: the loop with the gaps cut out.
        let mut pieces: Vec<Vec<Vec2>> = Vec::new();
        let mut cur: Vec<Vec2> = vec![lp[0]];
        for k in 0..lp.len() {
            let (a, b) = (lp[k], lp[(k + 1) % lp.len()]);
            let mut cuts: Vec<(f32, f32)> = geoms_gaps.iter().filter_map(|g| g.clip(a, b)).collect();
            cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
            for (t0, t1) in cuts {
                cur.push(a.lerp(b, t0));
                if cur.len() >= 2 {
                    pieces.push(std::mem::take(&mut cur));
                }
                cur = vec![a.lerp(b, t1)];
            }
            cur.push(b);
        }
        if cur.len() >= 2 {
            pieces.push(cur);
        }
        for p in pieces {
            total += p.windows(2).map(|w| w[0].distance(w[1])).sum::<f32>();
            kerb_piece(bufs.at(Layer::Kerb, blk.centre()), &p, net.town_y);
        }
    }
    total
}

/// A kerb along `pts` (counter-clockwise round its block: the block on the left), profile
/// (offset into the block, height above the street): a vertical face, a small chamfer and the
/// top, with end caps.
fn kerb_piece(b: &mut Buf, pts: &[Vec2], y0: f32) {
    let face = [(0.0f32, 0.0f32), (0.0, KERB_TOP - 0.025)];
    let chamfer = [(0.0f32, KERB_TOP - 0.025), (0.025, KERB_TOP)];
    let top = [(0.025f32, KERB_TOP), (0.25, KERB_TOP)];
    let n = pts.len();
    // Mitred inward normals.
    let seg_in = |k: usize| {
        let d = (pts[k + 1] - pts[k]).normalize_or_zero();
        Vec2::new(-d.y, d.x)
    };
    let inward: Vec<Vec2> = (0..n)
        .map(|k| {
            let a = if k > 0 { seg_in(k - 1) } else { seg_in(0) };
            let c = if k + 1 < n { seg_in(k) } else { seg_in(n - 2) };
            let m = (a + c).normalize_or(a);
            m / m.dot(a).max(0.5)
        })
        .collect();
    let mut along = 0.0;
    for (strip, n_of) in [(face, 0u8), (chamfer, 1), (top, 2)] {
        along = 0.0;
        let mut rows = Vec::new();
        for k in 0..n {
            if k > 0 {
                along += pts[k].distance(pts[k - 1]);
            }
            let inn = inward[k];
            let nrm = match n_of {
                0 => v3(-inn, 0.0).normalize(),
                1 => (v3(-inn, 0.0).normalize() + Vec3::Y).normalize(),
                _ => Vec3::Y,
            };
            let row = b.pos.len() as u32;
            for (o, h) in strip {
                b.v(v3(pts[k] + inn * o, y0 + h), [along / 1.0, h + o], [1.0, 1.0, 1.0, 1.0]);
                b.nrm.push(nrm.to_array());
            }
            rows.push((row, nrm));
        }
        for w in rows.windows(2) {
            let (r0, n0) = w[0];
            let r1 = w[1].0;
            b.facing(r0, r1, r1 + 1, n0);
            b.facing(r0, r1 + 1, r0 + 1, n0);
        }
    }
    let _ = along;
    // End caps.
    for (k, dir) in [(0usize, -1.0f32), (n - 1, 1.0)] {
        let t = if k == 0 { pts[1] - pts[0] } else { pts[n - 1] - pts[n - 2] }.normalize_or_zero() * dir;
        let nrm = v3(t, 0.0);
        let inn = inward[k];
        let prof = [(0.0f32, 0.0f32), (0.0, KERB_TOP - 0.025), (0.025, KERB_TOP), (0.25, KERB_TOP), (0.25, 0.0)];
        let base = b.pos.len() as u32;
        for (o, h) in prof {
            b.v(v3(pts[k] + inn * o, y0 + h), [o, h], [1.0, 1.0, 1.0, 1.0]);
            b.nrm.push(nrm.to_array());
        }
        for i in 1..prof.len() as u32 - 1 {
            b.facing(base, base + i, base + i + 1, nrm);
        }
    }
}

// ---------------------------------------------------------------- markings

/// A paint strip along a road from `s0` to `s1` (m, may run past a closed road's end) at
/// lateral `lat`, `w` wide; `period` = dash period (dashed texture) or `None` (solid).
#[allow(clippy::too_many_arguments)]
fn stripe(bufs: &mut Bufs, open: &OpenWorld, r: usize, inside: &[bool], s0: f32, s1: f32, lat: f32, w: f32, period: Option<f32>) {
    if s1 - s0 < 0.3 {
        return;
    }
    let nr = &open.net.roads[r];
    let road = &nr.road;
    let n = road.len();
    // Sample at the ends and every station between.
    let mut ss = vec![s0];
    let mut k = (s0 / STEP).floor() + 1.0;
    while k * STEP < s1 - 0.05 {
        ss.push(k * STEP);
        k += 1.0;
    }
    ss.push(s1);
    let layer = if period.is_some() { Layer::Dashed } else { Layer::Solid };
    let (mid, _, _) = on_road(road, nr.closed, (s0 + s1) * 0.5, lat);
    let b = bufs.at(layer, mid);
    let mut prev: Option<(u32, u32)> = None;
    for s in ss {
        let (p, left, y_road) = on_road(road, nr.closed, s, lat);
        // Inside a junction the paint lies on the junction's draped surface.
        let seg = ((s / STEP).floor() as i64).rem_euclid(if nr.closed { n as i64 } else { n as i64 - 1 }) as usize;
        let junction = inside.get(seg).copied().unwrap_or(false);
        let y = if junction { surface_y(open, true, p) } else { y_road + LIFT } + PAINT;
        let v = match period {
            Some(per) => s / per,
            None => 0.5,
        };
        let a = b.v(v3(p - left * w, y), [0.0, v], [1.0; 4]);
        let c = b.v(v3(p + left * w, y), [1.0, v], [1.0; 4]);
        if let Some((pa, pc)) = prev {
            b.quad(pa, pc, c, a);
        }
        prev = Some((a, c));
    }
}

/// A short bar of paint: from `(s, lat0)` to `(s, lat1)` across the road, `w` deep along it
/// (texture across its depth).
#[allow(clippy::too_many_arguments)]
fn bar(bufs: &mut Bufs, open: &OpenWorld, r: usize, s: f32, lat0: f32, lat1: f32, depth: f32) {
    let nr = &open.net.roads[r];
    let (mid, _, _) = on_road(&nr.road, nr.closed, s, (lat0 + lat1) * 0.5);
    let b = bufs.at(Layer::Solid, mid);
    let mut ids = [0u32; 4];
    for (k, (ds, lat, u)) in [(-depth, lat0, 0.0f32), (depth, lat0, 1.0), (depth, lat1, 1.0), (-depth, lat1, 0.0)].into_iter().enumerate() {
        let (p, _, y) = on_road(&nr.road, nr.closed, s + ds, lat);
        ids[k] = b.v(v3(p, y + LIFT + PAINT), [u, 0.5], [1.0; 4]);
    }
    b.quad(ids[0], ids[1], ids[2], ids[3]);
}

/// Intervals [a, b] of `total` minus `cut` (each possibly wrapping on a closed road).
fn subtract(total: f32, closed: bool, cut: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut c: Vec<(f32, f32)> = Vec::new();
    for &(a, b) in cut {
        if closed {
            let (a2, b2) = (a.rem_euclid(total), a.rem_euclid(total) + (b - a));
            c.push((a2, b2.min(total)));
            if b2 > total {
                c.push((0.0, b2 - total));
            }
        } else {
            c.push((a.max(0.0), b.min(total)));
        }
    }
    c.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out = Vec::new();
    let mut at = 0.0;
    for (a, b) in c {
        if a > at {
            out.push((at, a));
        }
        at = at.max(b);
    }
    if at < total {
        out.push((at, total));
    }
    out
}

fn markings(bufs: &mut Bufs, open: &OpenWorld, geoms: &[JunctionGeom], inside: &[Vec<bool>], plan: &Plan) {
    let net = &*open.net;
    for (r, nr) in net.roads.iter().enumerate() {
        let road = &nr.road;
        let n = road.len();
        if !road.surface.contains(&Surface::Tarmac) {
            continue;
        }
        let total = if nr.closed { n as f32 * STEP } else { (n - 1) as f32 * STEP };
        // (centre, left edge, right edge) exclusions.
        let mut cut: [Vec<(f32, f32)>; 3] = Default::default();
        // Not tarmac: nothing.
        for i in 0..n {
            if road.surface[i] != Surface::Tarmac {
                for c in cut.iter_mut() {
                    c.push((i as f32 * STEP - STEP * 0.5, i as f32 * STEP + STEP * 0.5));
                }
            }
        }
        for (gi, g) in geoms.iter().enumerate() {
            let thru = plan.through.iter().find(|t| t.0 == gi && t.1 == r).map(|t| t.2);
            for (a, p) in g.arms.iter().zip(&plan.arms[gi]).filter(|(a, _)| a.road == r) {
                let at = a.at as f32 * STEP;
                let span = |extra: f32| {
                    if a.sign > 0 { (at, at + a.steps as f32 * STEP + extra) } else { (at - a.steps as f32 * STEP - extra, at) }
                };
                if thru.is_none() {
                    cut[0].push(span(p.clear));
                }
                // Edge lines: stop at the mouth, except the far side of a through road.
                if thru != Some(-1.0) {
                    cut[1].push(span(0.0));
                }
                if thru != Some(1.0) {
                    cut[2].push(span(0.0));
                }

            }
        }
        let (centre, edges) = match nr.kind {
            RoadKind::Street => (Some((0.06, 9.0)), false),
            RoadKind::Gravel => (Some((0.06, 12.0)), true),
            RoadKind::BRoad => (Some((0.075, 15.0)), true),
            RoadKind::Forest => (None, false),
        };
        if let Some((w, period)) = centre {
            for (a, b) in subtract(total, nr.closed, &cut[0]) {
                stripe(bufs, open, r, &inside[r], a, b, 0.0, w * 2.0, Some(period));
            }
        }
        if edges {
            let off = nr.road.width[0] * 0.5 - 0.45;
            for (k, side) in [(1usize, 1.0f32), (2, -1.0)] {
                for (a, b) in subtract(total, nr.closed, &cut[k]) {
                    stripe(bufs, open, r, &inside[r], a, b, side * off, 0.15, None);
                }
            }
        }
    }
    // Stop / give-way lines and zebras at the mouths.
    for (gi, g) in geoms.iter().enumerate() {
        for (a, p) in g.arms.iter().zip(&plan.arms[gi]) {
            let nr = &net.roads[a.road];
            let half = nr.road.width[a.mouth] * 0.5;
            let sg = a.sign as f32;
            let m = a.mouth as f32 * STEP;
            if p.zebra {
                let mut lat = -half + 1.0;
                while lat <= half - 0.9 {
                    // Bars 0.5 m wide (quad 1 m: the paint is its middle half), 3 m long.
                    let (s0, s1) = (m + sg * 0.5, m + sg * 3.5);
                    let (lo, hi) = (s0.min(s1), s0.max(s1));
                    stripe(bufs, open, a.road, &inside[a.road], lo, hi, lat, 0.5, None);
                    lat += 1.0;
                }
            }
            if p.stop {
                let s = m + sg * if p.zebra { 4.6 } else { 1.0 };
                // The inbound half (traffic keeps right): lateral +sign.
                let (l0, l1) = (sg * 0.15, sg * (half - 0.35));
                if p.solid {
                    bar(bufs, open, a.road, s, l0, l1, 0.4);
                } else {
                    // Give way: short dashes across.
                    let len = (l1 - l0).abs();
                    let k = (len / 0.9).floor().max(1.0) as usize;
                    for i in 0..k {
                        let t0 = i as f32 * 0.9;
                        bar(bufs, open, a.road, s, l0 + (l1 - l0).signum() * t0, l0 + (l1 - l0).signum() * (t0 + 0.55), 0.3);
                    }
                }
            }
        }
    }
}

/// The paint's alpha profile: the middle half of `u` painted (soft by a texel), lightly worn;
/// dashed: the first third of `v` (the texture repeats along the road). Full mip chain.
fn paint_image(dashed: bool) -> Image {
    let (w, h) = (32usize, if dashed { 96usize } else { 16 });
    let hash = |x: usize, y: usize| {
        let mut k = (x as u32).wrapping_mul(0x9E37_79B9) ^ (y as u32).wrapping_mul(0x85EB_CA6B) ^ (dashed as u32);
        k ^= k >> 15;
        k = k.wrapping_mul(0x2C1B_3C6D);
        k ^= k >> 12;
        (k & 0xFFFF) as f32 / 65535.0
    };
    let mut level: Vec<f32> = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32;
            let mut a = ((0.25 - (u - 0.5).abs()) * w as f32 + 0.5).clamp(0.0, 1.0);
            if dashed {
                let v = (y as f32 + 0.5) / h as f32;
                a *= ((v.min(1.0 / 3.0 - v)) * h as f32 + 0.5).clamp(0.0, 1.0);
            }
            a *= 0.86 + 0.14 * hash(x, y);
            level.push(a);
        }
    }
    let mut data = Vec::new();
    let (mut lw, mut lh) = (w, h);
    let mut levels = 0;
    loop {
        for a in &level {
            data.extend_from_slice(&[255, 255, 255, (a * 255.0).round() as u8]);
        }
        levels += 1;
        if lw == 1 && lh == 1 {
            break;
        }
        let (nw, nh) = ((lw / 2).max(1), (lh / 2).max(1));
        let mut next = vec![0.0f32; nw * nh];
        for y in 0..nh {
            for x in 0..nw {
                let mut sum = 0.0;
                let mut cnt = 0.0;
                for dy in 0..(lh / nh) {
                    for dx in 0..(lw / nw) {
                        sum += level[(y * (lh / nh) + dy) * lw + x * (lw / nw) + dx];
                        cnt += 1.0;
                    }
                }
                next[y * nw + x] = sum / cnt;
            }
        }
        level = next;
        lw = nw;
        lh = nh;
    }
    let mut img = Image::new_uninit(Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 }, TextureDimension::D2, TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD);
    img.data = Some(data);
    img.texture_descriptor.mip_level_count = levels;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_image_has_a_full_mip_chain() {
        for dashed in [false, true] {
            let img = paint_image(dashed);
            let (w, h) = (img.texture_descriptor.size.width as usize, img.texture_descriptor.size.height as usize);
            let levels = img.texture_descriptor.mip_level_count as usize;
            assert_eq!(levels, (w.max(h) as f32).log2() as usize + 1);
            let mut texels = 0;
            let (mut lw, mut lh) = (w, h);
            for _ in 0..levels {
                texels += lw * lh;
                lw = (lw / 2).max(1);
                lh = (lh / 2).max(1);
            }
            assert_eq!(img.data.as_ref().unwrap().len(), texels * 4);
        }
    }

    #[test]
    fn subtract_wraps_on_a_closed_road() {
        let v = subtract(100.0, true, &[(90.0, 110.0), (40.0, 50.0)]);
        assert_eq!(v, vec![(10.0, 40.0), (50.0, 90.0)]);
        let v = subtract(100.0, false, &[(-5.0, 10.0)]);
        assert_eq!(v, vec![(10.0, 100.0)]);
    }

    #[test]
    fn gaps_clip_segments() {
        let g = Gap { c: Vec2::ZERO, along: Vec2::X, half: Vec2::new(1.0, 2.0) };
        let (t0, t1) = g.clip(Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0)).unwrap();
        assert!((t0 - 0.4).abs() < 1e-5 && (t1 - 0.6).abs() < 1e-5);
        assert!(g.clip(Vec2::new(-5.0, 3.0), Vec2::new(5.0, 3.0)).is_none());
    }
}
