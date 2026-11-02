//! Trees, built in code from foliage cards and fed by the dressing graph's `trunks` points (the
//! same points the car collides with). Conifer cards are rendered from Poly Haven's full-size
//! scanned trees (`pine_tree_01`, `fir_tree_01`) by `tools/art/render_tree_cards.py`: the whole
//! tree from the side plus three real branches from above ([`Atlas`]). The birch crown comes from
//! `tree_small_02` and the scrub tree from `island_tree_01`, rendered the same way. Trunks use
//! the `pine_bark` scan.
//!
//! - **Species**: Scots pine (long bare trunk, crown on top), spruce/fir (bare lower trunk,
//!   dense crown), snowy spruce, birch (white trunk, leafy crown) and an olive-like scrub tree.
//!   Each has three seeded variants.
//! - **Near** (within [`NEAR_END`] m of the camera): a trunk (bark) and a crown (three crossed
//!   core cards for the silhouette plus drooping branch cards for depth), two entities per tree
//!   sharing meshes and materials (Bevy batches them), casting shadows. Spawned per PCG cell
//!   around the camera ([`NEAR_LOAD`] / [`NEAR_UNLOAD`]), a few cells per frame.
//! - **Far**: the core cards only, merged into one mesh per PCG cell (no shadows), from
//!   [`FAR_START`] m to the fog distance. Built once when the cell's points arrive.
//! - Both tiers cross-fade with dithered `VisibilityRange`s; between them a tree can be drawn
//!   twice for a few metres, never not at all.

use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::camera::primitives::MeshAabb;
use engine::gfx::MainCamera;
use engine::pcg::PcgEmitted;
use engine::render_budget::{KeepDetail, Scenery};

use super::pbr;
use crate::stage::{Biome, Rng};
use crate::world::StageWorld;

/// Near trees fade out by this distance from the camera (m).
pub const NEAR_END: f32 = 150.0;
/// Far cards fade in from this distance to their cell's centre.
pub const FAR_START: f32 = 104.0;
const FADE: f32 = 12.0;
/// Near entities exist for cells whose centre is within this (and go beyond the unload radius).
pub const NEAR_LOAD: f32 = 220.0;
pub const NEAR_UNLOAD: f32 = 270.0;
/// Cells whose near trees spawn per frame.
const SPAWNS_PER_FRAME: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Species {
    Pine,
    Spruce,
    SnowSpruce,
    Birch,
    Olive,
}

/// Species mix per biome (weights).
pub fn mix(b: Biome) -> &'static [(Species, f32)] {
    use Species::*;
    match b {
        Biome::PineForest => &[(Pine, 4.0), (Spruce, 2.0), (Birch, 0.6)],
        Biome::BirchHills => &[(Birch, 3.0), (Pine, 1.0)],
        Biome::Scrub => &[(Olive, 2.0), (Pine, 0.5)],
        Biome::Moor => &[(Pine, 1.0), (Birch, 1.0)],
        Biome::SnowForest => &[(SnowSpruce, 4.0), (Spruce, 1.5)],
    }
}

fn pick(mix: &[(Species, f32)], seed: u64) -> (Species, usize) {
    let total: f32 = mix.iter().map(|m| m.1).sum();
    let r = (pcg_core::unit(seed) as f32) * total;
    let mut acc = 0.0;
    let v = (pcg_core::mix64(seed ^ 0x7EE5) % VARIANTS as u64) as usize;
    for &(s, w) in mix {
        acc += w;
        if r < acc {
            return (s, v);
        }
    }
    (mix[mix.len() - 1].0, v)
}

pub const VARIANTS: usize = 3;

// ---------------------------------------------------------------- mesh building

/// A CPU mesh being built (positions, normals, uvs, indices).
#[derive(Clone, Default)]
pub struct Geo {
    pub pos: Vec<Vec3>,
    pub nrm: Vec<Vec3>,
    pub uv: Vec<Vec2>,
    pub idx: Vec<u32>,
}

impl Geo {
    fn quad(&mut self, p: [Vec3; 4], n: [Vec3; 4], uv: [Vec2; 4]) {
        let b = self.pos.len() as u32;
        self.pos.extend(p);
        self.nrm.extend(n);
        self.uv.extend(uv);
        self.idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }

    pub fn tris(&self) -> usize {
        self.idx.len() / 3
    }

    fn mesh(&self, tangents: bool) -> Mesh {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.pos.iter().map(|v| v.to_array()).collect::<Vec<_>>());
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm.iter().map(|v| v.to_array()).collect::<Vec<_>>());
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv.iter().map(|v| v.to_array()).collect::<Vec<_>>());
        m.insert_indices(Indices::U32(self.idx.clone()));
        if tangents {
            let _ = m.generate_tangents();
        }
        m
    }

    /// Appends `other` transformed (rotation about Y, uniform-ish scale, offset).
    fn append(&mut self, other: &Geo, yaw: f32, scale: Vec3, offset: Vec3) {
        let q = Quat::from_rotation_y(yaw);
        let b = self.pos.len() as u32;
        self.pos.extend(other.pos.iter().map(|p| q * (*p * scale) + offset));
        self.nrm.extend(other.nrm.iter().map(|n| q * *n));
        self.uv.extend_from_slice(&other.uv);
        self.idx.extend(other.idx.iter().map(|i| i + b));
    }
}

/// Where a card's model sits inside its square texture (u0, v0, u1, v1; v down), from the
/// renderer's `frame.txt`.
#[derive(Clone, Copy)]
struct Frame {
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
}


/// A conifer card atlas (`tools/art/render_tree_cards.py`, from Poly Haven's full-size scans):
/// the whole tree from the side, three real branches from above (trunk end at the bottom).
struct Atlas {
    set: &'static str,
    tree: Frame,
    /// Width / height of the tree picture.
    aspect: f32,
    /// Where the needles start, as a share of the height.
    crown: f32,
    branches: [Frame; 3],
    /// Width / length of each branch picture.
    branch_aspect: [f32; 3],
}

macro_rules! fr {
    ($u0:expr, $v0:expr, $u1:expr, $v1:expr) => {
        Frame { u0: $u0, v0: $v0, u1: $u1, v1: $v1 }
    };
}

/// `fir_tree_01` (variant a): spruce and snowy spruce.
const ATLAS_FIR: Atlas = Atlas {
    set: "card_fir",
    tree: fr!(0.0034, 0.0233, 0.6841, 1.0),
    aspect: 0.3485,
    crown: 0.3619,
    branches: [fr!(0.6906, 0.1125, 0.9969, 0.333), fr!(0.6906, 0.4904, 0.9969, 0.666), fr!(0.6906, 0.8281, 0.9969, 0.999)],
    branch_aspect: [0.6948, 0.8721, 0.8965],
};

/// `pine_tree_01` (variant a): Scots pine.
const ATLAS_PINE: Atlas = Atlas {
    set: "card_pine",
    tree: fr!(0.0034, 0.1529, 0.6841, 1.0),
    aspect: 0.4018,
    crown: 0.3557,
    branches: [fr!(0.6906, 0.1709, 0.9969, 0.333), fr!(0.6906, 0.5345, 0.9969, 0.666), fr!(0.6906, 0.8329, 0.9969, 0.999)],
    branch_aspect: [0.9453, 1.1647, 0.9222],
};

/// `tree_small_02`: birch crowns, rendered without the scan's trunk (ours is white, in code);
/// the three small pictures are leaf clumps seen from the side.
const ATLAS_BIRCH: Atlas = Atlas {
    set: "card_birch",
    tree: fr!(0.0062, 0.0099, 0.6813, 1.0),
    aspect: 0.6818,
    crown: 0.4442,
    branches: [fr!(0.6906, 0.0133, 0.9969, 0.3197), fr!(0.6906, 0.3463, 0.9969, 0.6527), fr!(0.6906, 0.6793, 0.9969, 0.9857)],
    branch_aspect: [1.0000, 1.0000, 1.0000],
};

/// `island_tree_01`: the scrub stages' olive-like tree, trunk included; the three small
/// pictures are leaf clumps seen from the side.
const ATLAS_OLIVE: Atlas = Atlas {
    set: "card_olive",
    tree: fr!(0.0034, 0.4204, 0.6841, 1.0),
    aspect: 1.1745,
    crown: 0.2166,
    branches: [fr!(0.6906, 0.0133, 0.9969, 0.3197), fr!(0.6906, 0.3463, 0.9969, 0.6527), fr!(0.6906, 0.6793, 0.9969, 0.9857)],
    branch_aspect: [1.0000, 1.0000, 1.0000],
};

impl Frame {
    /// The top part of the picture only (`keep` of its height), e.g. a crown without its trunk.
    fn top(self, keep: f32) -> Frame {
        Frame { v1: self.v0 + (self.v1 - self.v0) * keep, ..self }
    }
}

/// Soft "foliage" normal: between the card's face normal and the direction out of the crown.
fn soft(face: Vec3, out: Vec3) -> Vec3 {
    (face * 0.35 + out.normalize_or(face) * 0.65 + Vec3::Y * 0.25).normalize()
}

/// A vertical card through the trunk axis at `yaw`, from height `y0` to `y1`, `w` wide.
fn core_card(g: &mut Geo, yaw: f32, y0: f32, y1: f32, w: f32, f: Frame) {
    let t = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    let face = t.cross(Vec3::Y).normalize();
    let h = w * 0.5;
    let p = [Vec3::Y * y0 - t * h, Vec3::Y * y0 + t * h, Vec3::Y * y1 + t * h, Vec3::Y * y1 - t * h];
    let mid = (y0 + y1) * 0.5;
    let n = p.map(|q| soft(face, q - Vec3::Y * mid));
    let uv = [Vec2::new(f.u0, f.v1), Vec2::new(f.u1, f.v1), Vec2::new(f.u1, f.v0), Vec2::new(f.u0, f.v0)];
    g.quad(p, n, uv);
}

/// A branch card from the trunk at height `y` outwards along `yaw`, `len` long, drooping by
/// `droop` radians, rolled by `roll` about its own axis; the picture's base sits at the trunk.
#[allow(clippy::too_many_arguments)]
fn branch_card(g: &mut Geo, y: f32, yaw: f32, len: f32, width: f32, droop: f32, roll: f32, f: Frame) {
    let out = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    let dir = (out * droop.cos() - Vec3::Y * droop.sin()).normalize();
    let side0 = out.cross(Vec3::Y).normalize();
    let side = Quat::from_axis_angle(dir, roll) * side0;
    let face = side.cross(dir).normalize();
    let base = Vec3::Y * y + out * 0.05;
    let tip = base + dir * len;
    let (hb, ht) = (width * 0.18, width * 0.5);
    let p = [base - side * hb, base + side * hb, tip + side * ht, tip - side * ht];
    let n = p.map(|q| soft(if face.y < 0.0 { -face } else { face }, q - Vec3::Y * y));
    let uv = [Vec2::new(f.u0, f.v1), Vec2::new(f.u1, f.v1), Vec2::new(f.u1, f.v0), Vec2::new(f.u0, f.v0)];
    g.quad(p, n, uv);
}

/// A tapered trunk (open cylinder) from 0 to `h`, radius `r0` → `r1`, bark UVs (`tile` m).
fn trunk(g: &mut Geo, h: f32, r0: f32, r1: f32, sides: u32, tile: f32, lean: Vec2) {
    let rings = ((h / 2.5).ceil() as u32).max(2);
    let b = g.pos.len() as u32;
    for k in 0..=rings {
        let t = k as f32 / rings as f32;
        let y = h * t;
        let r = r0 + (r1 - r0) * t + if k == 0 { r0 * 0.35 } else { 0.0 };
        let c = Vec3::new(lean.x * t * t, y, lean.y * t * t);
        for s in 0..=sides {
            let a = s as f32 / sides as f32 * TAU;
            let n = Vec3::new(a.cos(), 0.0, a.sin());
            g.pos.push(c + n * r + Vec3::Y * if k == 0 { -0.25 } else { 0.0 });
            g.nrm.push(n);
            g.uv.push(Vec2::new(s as f32 / sides as f32, -y / tile));
        }
    }
    let row = sides + 1;
    for k in 0..rings {
        for s in 0..sides {
            let (i0, i1) = (b + k * row + s, b + (k + 1) * row + s);
            g.idx.extend_from_slice(&[i0, i1, i0 + 1, i0 + 1, i1, i1 + 1]);
        }
    }
}

/// One tree variant: near trunk (bark; none for the olive), near crown, far crown.
pub struct TreeGeo {
    pub trunk: Option<Geo>,
    pub crown: Geo,
    pub far: Geo,
}

/// Builds a species variant (unit scale: the PCG point scale multiplies it).
pub fn build(sp: Species, variant: usize) -> TreeGeo {
    let mut rng = Rng::new(0x7EE0_0000 + variant as u64 * 7919 + sp as u64 * 104_729);
    let mut tr = Geo::default();
    let mut crown = Geo::default();
    let mut far = Geo::default();
    let yaw0 = rng.range(0.0, PI);
    match sp {
        Species::Spruce | Species::SnowSpruce => {
            // Tall fir: bare lower trunk with a few dead twigs, dense crown above.
            let a = &ATLAS_FIR;
            let h = rng.range(15.0, 20.0);
            let w = h * a.aspect * rng.range(0.9, 1.1);
            let crown0 = h * a.crown;
            trunk(&mut tr, h * 0.9, 0.25, 0.03, 7, 2.0, Vec2::ZERO);
            for k in 0..3 {
                let yaw = yaw0 + k as f32 * PI / 3.0;
                // Near: the crown only (the 3D trunk stands below; less alpha-tested overdraw).
                core_card(&mut crown, yaw, h * a.crown * 0.8, h, w, a.tree.top(1.0 - a.crown * 0.8));
                core_card(&mut far, yaw, -0.05, h, w, a.tree);
            }
            // Whorls of real branches: longest low down, drooping more there.
            let mut y = crown0 + 0.4;
            let mut i = rng.range(0.0, 3.0) as usize;
            while y < h - 1.5 {
                let t = (y - crown0) / (h - crown0);
                let len = w * 0.5 * (1.0 - t).powf(0.8) + 0.5;
                let n = if t < 0.5 { 4 } else { 3 };
                let a0 = rng.range(0.0, TAU);
                for b in 0..n {
                    let yaw = a0 + b as f32 * TAU / n as f32 + rng.range(-0.3, 0.3);
                    let k = i % 3;
                    i += 1;
                    branch_card(&mut crown, y, yaw, len, len * a.branch_aspect[k], rng.range(0.1, 0.3) + 0.25 * (1.0 - t), rng.range(-0.4, 0.4), a.branches[k]);
                }
                y += rng.range(0.9, 1.3);
            }
        }
        Species::Pine => {
            // Scots pine: long bare trunk, open crown of horizontal branches on top.
            let a = &ATLAS_PINE;
            let h = rng.range(16.0, 22.0);
            let w = h * a.aspect * rng.range(0.85, 1.1);
            let crown0 = h * a.crown;
            let lean = Vec2::new(rng.range(-0.4, 0.4), rng.range(-0.4, 0.4));
            trunk(&mut tr, h * 0.92, 0.27, 0.05, 7, 2.0, lean);
            let leanv = Vec3::new(lean.x, 0.0, lean.y) * 0.7;
            for k in 0..3 {
                let yaw = yaw0 + k as f32 * PI / 3.0;
                let (mut c, mut f) = (Geo::default(), Geo::default());
                // Near: the crown only (the 3D trunk stands below); far: the whole tree.
                core_card(&mut c, yaw, h * a.crown * 0.8, h, w, a.tree.top(1.0 - a.crown * 0.8));
                core_card(&mut f, yaw, -0.05, h, w, a.tree);
                crown.append(&c, 0.0, Vec3::ONE, leanv * 0.5);
                far.append(&f, 0.0, Vec3::ONE, leanv * 0.5);
            }
            let mut y = crown0 + 1.0;
            let mut i = rng.range(0.0, 3.0) as usize;
            while y < h - 1.5 {
                let t = (y - crown0) / (h - crown0);
                let len = w * 0.45 * (1.0 - t * 0.6) + 0.5;
                let a0 = rng.range(0.0, TAU);
                let mut c = Geo::default();
                for b in 0..3 {
                    let yaw = a0 + b as f32 * TAU / 3.0 + rng.range(-0.4, 0.4);
                    let k = i % 3;
                    i += 1;
                    branch_card(&mut c, y, yaw, len, len * a.branch_aspect[k], rng.range(-0.1, 0.2), rng.range(-0.5, 0.5), a.branches[k]);
                }
                crown.append(&c, 0.0, Vec3::ONE, leanv * (y / h).powi(2));
                y += rng.range(1.3, 1.9);
            }
        }
        Species::Birch => {
            // Slender crown from a real broadleaf scan over a white trunk; leaf clumps for volume.
            let a = &ATLAS_BIRCH;
            let h = rng.range(11.0, 15.0);
            let crown0 = h * a.crown;
            // The scan is a wide small tree: narrowed to a birch's proportions.
            let w = h * a.aspect * rng.range(0.66, 0.78);
            let lean = Vec2::new(rng.range(-0.4, 0.4), rng.range(-0.4, 0.4));
            trunk(&mut tr, h * 0.85, 0.16, 0.04, 6, 2.2, lean);
            let leanv = Vec3::new(lean.x, 0.0, lean.y) * 0.6;
            // The picture's lower part is empty (no trunk): start the cards a little below the
            // crown so the lowest branches meet our trunk.
            let f = a.tree.top(1.0 - a.crown * 0.6);
            for k in 0..3 {
                let yaw = yaw0 + k as f32 * PI / 3.0;
                let mut c = Geo::default();
                core_card(&mut c, yaw, h * a.crown * 0.6, h, w, f);
                crown.append(&c, 0.0, Vec3::ONE, leanv);
                far.append(&c, 0.0, Vec3::ONE, leanv);
            }
            for k in 0..7 {
                let y = rng.range(crown0 + 1.0, h - 1.5);
                let yaw = rng.range(0.0, TAU);
                let r = w * rng.range(0.15, 0.3);
                let sz = w * rng.range(0.4, 0.55);
                let mut c = Geo::default();
                core_card(&mut c, yaw + PI / 2.0, -sz * 0.5, sz * 0.5, sz * a.branch_aspect[k % 3], a.branches[k % 3]);
                crown.append(&c, 0.0, Vec3::ONE, Vec3::new(yaw.sin() * r, y, yaw.cos() * r) + leanv);
            }
        }
        Species::Olive => {
            // Gnarled trunk and round crown, both in the picture; leaf clumps for volume.
            let a = &ATLAS_OLIVE;
            let h = rng.range(4.2, 6.0);
            let w = h * a.aspect * rng.range(0.9, 1.05);
            for k in 0..3 {
                let yaw = yaw0 + k as f32 * PI / 3.0;
                core_card(&mut crown, yaw, -0.1, h, w, a.tree);
                core_card(&mut far, yaw, -0.1, h, w, a.tree);
            }
            let crown0 = h * a.crown;
            for k in 0..5 {
                let y = rng.range(crown0 + 0.8, h - 0.8);
                let yaw = rng.range(0.0, TAU);
                let r = w * rng.range(0.1, 0.25);
                let sz = h * rng.range(0.35, 0.5);
                let mut c = Geo::default();
                core_card(&mut c, yaw + PI / 2.0, -sz * 0.5, sz * 0.5, sz * a.branch_aspect[k % 3], a.branches[k % 3]);
                crown.append(&c, 0.0, Vec3::ONE, Vec3::new(yaw.sin() * r, y, yaw.cos() * r));
            }
            return TreeGeo { trunk: None, crown, far };
        }
    }
    TreeGeo { trunk: Some(tr), crown, far }
}

// ---------------------------------------------------------------- ECS

struct Kind {
    trunk: Option<Handle<Mesh>>,
    crown: Handle<Mesh>,
    far: Geo,
}

struct SpeciesKit {
    variants: Vec<Kind>,
    trunk_mat: Option<Handle<StandardMaterial>>,
    crown_mat: Handle<StandardMaterial>,
    far_mat: Handle<StandardMaterial>,
}

/// Meshes and materials for the stage's species.
#[derive(Resource)]
pub struct TreeKit {
    species: HashMap<Species, SpeciesKit>,
    mix: &'static [(Species, f32)],
    view: f32,
    /// The road every ~20 m (x, z): the camera never leaves it, so a forest cell further
    /// than `view` from all of these is never seen and gets no far cards.
    road: Vec<Vec2>,
}

impl TreeKit {
    /// Can a camera on the road see trees around `p` (within `margin` of it)?
    fn seen(&self, p: Vec3, margin: f32) -> bool {
        let reach = (self.view + margin).powi(2);
        let p = Vec2::new(p.x, p.z);
        self.road.iter().any(|r| r.distance_squared(p) <= reach)
    }
}

#[derive(Clone, Copy)]
struct Tree {
    pos: Vec3,
    yaw: f32,
    scale: Vec3,
    species: Species,
    variant: usize,
}

#[derive(Default)]
struct Cell {
    trees: Vec<Tree>,
    center: Vec3,
    far: Vec<Entity>,
    near: Vec<Entity>,
}

/// Trees per PCG cell, and which cells have near entities.
#[derive(Resource, Default)]
pub struct TreeField {
    cells: HashMap<IVec2, Cell>,
    /// Cells in near range still waiting for their near trees (after this frame's spawns).
    waiting: usize,
    pub stats: TreeStats,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TreeStats {
    pub trees: usize,
    pub cells: usize,
    pub near_cells: usize,
    pub near_entities: usize,
    pub far_triangles: usize,
}

/// Creates the kit for a stage (materials, meshes). Visual worlds only.
pub fn setup(world: &mut World, s: &StageWorld, view: f32) {
    let wet = world.get_resource::<super::Wet>().is_some_and(|w| w.0);
    let mix = mix(s.def.biome);
    // Poly Haven's `pine_bark` scan: as is for the pines, greyer for the spruces.
    let bark = pbr(world, "bark", |m| {
        m.base_color = Color::srgb(1.0, 0.96, 0.92);
        m.perceptual_roughness = if wet { 0.6 } else { 0.95 };
    });
    let spruce_bark = pbr(world, "bark", |m| {
        m.base_color = Color::srgb(0.74, 0.72, 0.7);
        m.perceptual_roughness = if wet { 0.6 } else { 0.95 };
    });
    let birch_bark = {
        let img = birch_bark_image();
        let h = world.resource_mut::<Assets<Image>>().add(img);
        world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { base_color_texture: Some(h), perceptual_roughness: 0.85, ..default() })
    };
    let mut species = HashMap::new();
    for &(sp, _) in mix {
        let (set, tint) = match sp {
            Species::Pine => (ATLAS_PINE.set, Color::srgb(0.9, 0.92, 0.85)),
            Species::Spruce => ("card_fir", Color::srgb(0.7, 0.78, 0.68)),
            Species::SnowSpruce => ("card_fir_snow", Color::srgb(0.9, 0.92, 0.94)),
            Species::Birch => (ATLAS_BIRCH.set, Color::srgb(0.95, 1.0, 0.88)),
            Species::Olive => (ATLAS_OLIVE.set, Color::srgb(0.95, 0.98, 0.88)),
        };
        let crown_mat = pbr(world, set, |m| foliage(m, tint, wet));
        let far_mat = pbr(world, set, |m| {
            foliage(m, tint, wet);
            m.normal_map_texture = None;
        });
        let mut variants = Vec::new();
        for v in 0..VARIANTS {
            let g = build(sp, v);
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            variants.push(Kind { trunk: g.trunk.as_ref().map(|t| meshes.add(t.mesh(true))), crown: meshes.add(g.crown.mesh(true)), far: g.far });
        }
        let trunk_mat = match sp {
            Species::Olive => None,
            Species::Birch => Some(birch_bark.clone()),
            Species::Spruce | Species::SnowSpruce => Some(spruce_bark.clone()),
            Species::Pine => Some(bark.clone()),
        };
        species.insert(sp, SpeciesKit { variants, trunk_mat, crown_mat, far_mat });
    }
    let step = (s.road.len() / 400).max(1);
    let road = s.road.pts.iter().step_by(step).map(|p| Vec2::new(p.x, p.z)).collect();
    world.insert_resource(TreeKit { species, mix, view, road });
    world.insert_resource(TreeField::default());
}

/// A tree for `seed` drawn from the stage's mix, as near meshes with their materials:
/// `[(trunk), crown]` (no trunk for species without one). For `crate::props` saplings, which
/// share the trees' meshes.
pub fn near_parts(kit: &TreeKit, seed: u64) -> Vec<(Handle<Mesh>, Handle<StandardMaterial>)> {
    let (sp, v) = pick(kit.mix, seed);
    let sk = &kit.species[&sp];
    let kind = &sk.variants[v];
    let mut out = Vec::with_capacity(2);
    if let (Some(m), Some(mat)) = (&kind.trunk, &sk.trunk_mat) {
        out.push((m.clone(), mat.clone()));
    }
    out.push((kind.crown.clone(), sk.crown_mat.clone()));
    out
}

/// Near trees are out around the camera (true when the stage has no trees).
pub fn near_ready(world: &World) -> bool {
    world.get_resource::<TreeField>().is_none_or(|f| f.waiting == 0)
}

fn foliage(m: &mut StandardMaterial, tint: Color, wet: bool) {
    m.base_color = tint;
    m.alpha_mode = AlphaMode::Mask(0.5);
    m.double_sided = true;
    m.cull_mode = None;
    m.perceptual_roughness = if wet { 0.5 } else { 0.85 };
    m.reflectance = 0.25;
    m.diffuse_transmission = 0.3;
}

/// White birch bark with dark lenticels and patches, made in code (256 × 512).
fn birch_bark_image() -> Image {
    use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let (w, h) = (256u32, 512u32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    let mut rng = Rng::new(0xB1C4);
    // Base: warm white with soft vertical streaks.
    for y in 0..h {
        for x in 0..w {
            let n = crate::stage::noise(Vec2::new(x as f32 / 18.0, y as f32 / 90.0), 5) * 0.08 + crate::stage::noise(Vec2::new(x as f32 / 5.0, y as f32 / 9.0), 9) * 0.05;
            let v = (0.86 + n).clamp(0.0, 1.0);
            let i = ((y * w + x) * 4) as usize;
            px[i..i + 4].copy_from_slice(&[(v * 236.0) as u8, (v * 232.0) as u8, (v * 222.0) as u8, 255]);
        }
    }
    // Lenticels: short dark horizontal dashes; a few larger dark patches.
    for _ in 0..420 {
        let (cx, cy) = (rng.range(0.0, w as f32), rng.range(0.0, h as f32));
        let (lw, lh) = (rng.range(6.0, 34.0), rng.range(1.0, 3.2));
        let dark = rng.range(0.15, 0.5);
        for dy in 0..lh.ceil() as i32 {
            for dx in 0..lw as i32 {
                let x = ((cx as i32 + dx).rem_euclid(w as i32)) as u32;
                let y = ((cy as i32 + dy).rem_euclid(h as i32)) as u32;
                let i = ((y * w + x) * 4) as usize;
                for c in 0..3 {
                    px[i + c] = (px[i + c] as f32 * dark) as u8;
                }
            }
        }
    }
    for _ in 0..14 {
        let (cx, cy, r) = (rng.range(0.0, w as f32), rng.range(0.0, h as f32), rng.range(6.0, 16.0));
        for dy in -(r as i32)..=(r as i32) {
            for dx in -(r as i32 * 2)..=(r as i32 * 2) {
                if (dx as f32 / 2.0).hypot(dy as f32) > r {
                    continue;
                }
                let x = ((cx as i32 + dx).rem_euclid(w as i32)) as u32;
                let y = ((cy as i32 + dy).rem_euclid(h as i32)) as u32;
                let i = ((y * w + x) * 4) as usize;
                for c in 0..3 {
                    px[i + c] = (px[i + c] as f32 * 0.25) as u8;
                }
            }
        }
    }
    let mut img = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, px, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor { address_mode_u: ImageAddressMode::Repeat, address_mode_v: ImageAddressMode::Repeat, anisotropy_clamp: 8, ..ImageSamplerDescriptor::linear() });
    engine::gfx::build_mips(&mut img);
    img
}

/// Takes the dressing graph's `trunks` points as PCG cells finish and builds their far cards.
pub fn ingest(mut commands: Commands, mut events: MessageReader<PcgEmitted>, kit: Option<Res<TreeKit>>, mut field: Option<ResMut<TreeField>>, mut meshes: ResMut<Assets<Mesh>>) {
    let (Some(kit), Some(field)) = (kit, field.as_deref_mut()) else {
        events.clear();
        return;
    };
    for e in events.read() {
        if e.port != "trunks" {
            continue;
        }
        let p = &e.points;
        let cell = field.cells.entry(e.cell).or_default();
        // A re-evaluated cell replaces its trees.
        for id in cell.far.drain(..).chain(cell.near.drain(..)) {
            commands.entity(id).despawn();
        }
        cell.trees.clear();
        for k in 0..p.len() {
            let (species, variant) = pick(kit.mix, p.seed[k]);
            let (yaw, _, _) = p.rot[k].to_euler(EulerRot::YXZ);
            cell.trees.push(Tree { pos: p.pos[k], yaw, scale: p.scale[k], species, variant });
        }
        if cell.trees.is_empty() {
            continue;
        }
        cell.center = cell.trees.iter().map(|t| t.pos).sum::<Vec3>() / cell.trees.len() as f32;
        // A 64 m cell's trees sit within ~46 m of its centre.
        if !kit.seen(cell.center, 50.0) {
            continue;
        }
        // Far cards: one merged mesh per species.
        let mut per: HashMap<Species, Geo> = HashMap::new();
        for t in &cell.trees {
            let g = per.entry(t.species).or_default();
            let kind = &kit.species[&t.species].variants[t.variant];
            g.append(&kind.far, t.yaw, t.scale, t.pos - cell.center);
        }
        for (sp, g) in per {
            field.stats.far_triangles += g.tris();
            // GPU-only (no CPU copy kept), so the bounds go on explicitly.
            let mut m = g.mesh(false);
            let aabb = m.get_aabb();
            m.asset_usage = RenderAssetUsages::RENDER_WORLD;
            let h = meshes.add(m);
            let range = VisibilityRange { start_margin: (FAR_START - FADE)..FAR_START, end_margin: (kit.view - 60.0)..kit.view, use_aabb: false };
            let id = commands
                .spawn((Name::new("trees far"), Mesh3d(h), MeshMaterial3d(kit.species[&sp].far_mat.clone()), Transform::from_translation(cell.center), range, NotShadowCaster, Scenery, KeepDetail))
                .id();
            if let Some(a) = aabb {
                commands.entity(id).insert((a, bevy::camera::visibility::NoAutoAabb));
            }
            cell.far.push(id);
        }
    }
    field.stats.cells = field.cells.len();
    field.stats.trees = field.cells.values().map(|c| c.trees.len()).sum();
}

/// Spawns near trees for cells around the camera and removes them for cells far away.
pub fn stream(
    mut commands: Commands,
    kit: Option<Res<TreeKit>>,
    field: Option<ResMut<TreeField>>,
    cams: Query<&GlobalTransform, With<MainCamera>>,
    loading: Option<Res<crate::loading::Loading>>,
) {
    let (Some(kit), Some(mut field), Ok(cam)) = (kit, field, cams.single()) else { return };
    let eye = cam.translation();
    let d2 = |c: &Cell| Vec2::new(c.center.x - eye.x, c.center.z - eye.z).length_squared();
    // Unload far cells.
    let field = &mut *field;
    for c in field.cells.values_mut() {
        if !c.near.is_empty() && d2(c) > NEAR_UNLOAD * NEAR_UNLOAD {
            for id in c.near.drain(..) {
                commands.entity(id).despawn();
            }
        }
    }
    // Load the nearest few missing cells.
    let mut want: Vec<(f32, IVec2)> = field.cells.iter().filter(|(_, c)| c.near.is_empty() && !c.trees.is_empty() && d2(c) < NEAR_LOAD * NEAR_LOAD).map(|(k, c)| (d2(c), *k)).collect();
    want.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Behind the loading screen nobody sees a long frame: fill the view at once.
    let per_frame = if loading.is_some() { SPAWNS_PER_FRAME * 8 } else { SPAWNS_PER_FRAME };
    field.waiting = want.len().saturating_sub(per_frame);
    for (_, key) in want.into_iter().take(per_frame) {
        let c = field.cells.get_mut(&key).unwrap();
        let range = VisibilityRange { start_margin: 0.0..0.0, end_margin: (NEAR_END - FADE)..NEAR_END, use_aabb: false };
        for t in &c.trees {
            let sk = &kit.species[&t.species];
            let kind = &sk.variants[t.variant];
            let tf = Transform { translation: t.pos, rotation: Quat::from_rotation_y(t.yaw), scale: t.scale };
            if let (Some(m), Some(mat)) = (&kind.trunk, &sk.trunk_mat) {
                c.near.push(commands.spawn((Mesh3d(m.clone()), MeshMaterial3d(mat.clone()), tf, range.clone(), Scenery, KeepDetail)).id());
            }
            c.near.push(commands.spawn((Mesh3d(kind.crown.clone()), MeshMaterial3d(sk.crown_mat.clone()), tf, range.clone(), Scenery, KeepDetail)).id());
        }
    }
    field.stats.near_cells = field.cells.values().filter(|c| !c.near.is_empty()).count();
    field.stats.near_entities = field.cells.values().map(|c| c.near.len()).sum();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_species_builds_cheap_valid_variants() {
        for sp in [Species::Pine, Species::Spruce, Species::SnowSpruce, Species::Birch, Species::Olive] {
            for v in 0..VARIANTS {
                let g = build(sp, v);
                let n = g.crown.pos.len() as u32;
                assert!(g.crown.idx.iter().all(|&i| i < n));
                assert!(g.crown.tris() < 400, "{sp:?} crown {}", g.crown.tris());
                assert!(g.far.tris() <= 12, "{sp:?} far {}", g.far.tris());
                if let Some(t) = &g.trunk {
                    assert!(t.tris() < 120);
                }
                assert!(g.crown.uv.iter().all(|u| (0.0..=1.0).contains(&u.x) && (0.0..=1.0).contains(&u.y)));
            }
        }
    }

    /// The atlas constants match the renderer's `frame.txt` (re-render → update the constants).
    #[test]
    fn atlas_constants_match_the_rendered_frames() {
        for a in [&ATLAS_FIR, &ATLAS_PINE, &ATLAS_BIRCH, &ATLAS_OLIVE] {
            let path = format!("{}/assets/textures/{}/frame.txt", env!("CARGO_MANIFEST_DIR"), a.set);
            let txt = std::fs::read_to_string(&path).expect(&path);
            let row = |key: &str| -> Vec<f32> {
                let line = txt.lines().find(|l| l.split_whitespace().next() == Some(key)).unwrap_or_else(|| panic!("{path}: no {key}"));
                line.split_whitespace().skip(1).map(|v| v.parse().unwrap()).collect()
            };
            let close = |f: Frame, r: &[f32]| (f.u0 - r[0]).abs() < 2e-4 && (f.v0 - r[1]).abs() < 2e-4 && (f.u1 - r[2]).abs() < 2e-4 && (f.v1 - r[3]).abs() < 2e-4;
            let t = row("tree");
            assert!(close(a.tree, &t), "{}: tree frame {t:?}", a.set);
            assert!((a.aspect - t[4] / t[5]).abs() < 2e-3, "{}: aspect {}", a.set, t[4] / t[5]);
            assert!((a.crown - row("crown")[0]).abs() < 2e-3, "{}: crown", a.set);
            for k in 0..3 {
                let b = row(&format!("branch{k}"));
                assert!(close(a.branches[k], &b), "{}: branch{k} frame {b:?}", a.set);
                assert!((a.branch_aspect[k] - b[4] / b[5]).abs() < 2e-3, "{}: branch{k} aspect {}", a.set, b[4] / b[5]);
            }
        }
    }

    #[test]
    fn species_pick_is_deterministic_and_follows_the_mix() {
        let mix = mix(Biome::PineForest);
        let mut n = HashMap::new();
        for s in 0..3000u64 {
            let a = pick(mix, pcg_core::mix64(s));
            assert_eq!(a, pick(mix, pcg_core::mix64(s)));
            *n.entry(a.0).or_insert(0) += 1;
        }
        assert!(n[&Species::Pine] > n[&Species::Spruce] && n[&Species::Spruce] > n[&Species::Birch]);
    }
}
