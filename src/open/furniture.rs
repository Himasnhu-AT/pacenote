//! Roadside furniture and road conditions the car feels: guard rails (solid), signs and posts
//! (placement only; the meshes are `art::signs`), and patches of potholes, wet tarmac, mud,
//! washouts and ice. Pure, seeded and sorted, built with the `OpenWorld` from the road net and
//! the heights, so the same level always gives the same furniture, and a re-baked level gets
//! furniture that fits its new roads.
//!
//! **Owner: stream B (ROADSIDE).** Design: `docs/festival/WORLD_PASS.md` §4 (placement rules,
//! measured), `docs/festival/ART_BIBLE.md` §4 (look), `docs/festival/TECH_PASS.md` §3.0/§5,
//! `docs/festival/DECISIONS.md` W3-W7.
//!
//! The hooks are wired into `open::OpenWorld`: [`dip`] in `ground_height`, [`surface_override`]
//! in `surface_at`, [`walls`] in `from_data_with`. Anything here that changes how the car drives
//! bumps [`REV`], which goes into the event `stage_hash`.
//!
//! Placement in short (all distances along the road, both directions of travel):
//! - **Guard rails** where the ground 8 m past the edge is more than 3 m below it, on the outside
//!   of corners up to grade 4 (grade 5/6 above 250 m), or anywhere it drops over 6 m; extended
//!   20 m, gaps under 30 m merged, at least 24 m. Steel on tarmac, timber on gravel and forest.
//! - **Chevrons** round every hairpin (5 boards), tarmac 3-4s that need a big brake (3), and
//!   gravel 1-2s after a fast run or over a crest (3).
//! - **Warning triangles** 120 m (tarmac) / 70 m (gravel) before the hazard on the right verge,
//!   at most one per 50 m: junction > hairpin > crest/jump > double bend > steep, rocks, bridge >
//!   slippery, loose, deer > bend.
//! - **Speed and town signs** wherever the posted limit ([`posted_kmh`], the traffic's) changes,
//!   at town street mouths and at the town limits.
//! - **Km posts** on roads 0 and 1, **reflector posts** on rural tarmac (50 m, 25 m in bends)
//!   and fast gravel (100 m), **street lights** on town streets (30 m, solid stubs).
//! - **Conditions**: potholes on old stretches of roads 0 and 1 (a dip; grip unchanged), wet
//!   tarmac in damp dips, mud on wet gravel, washouts on steep wet gravel (loose and corrugated),
//!   ice on the shaded inside of high forest hairpins.

use bevy::math::{Vec2, Vec3};
use engine::world::Heightfield;

use super::RoadNet;
use super::profile::SHOULDER;
use super::roads::RoadKind;
use crate::stage::{self, Road, STEP, Surface};
use crate::vehicle::Wall;

/// Furniture revision: bump when the generated furniture changes how the car drives.
/// 1: rails, street-light stubs, potholes, wet, mud, washouts, ice.
pub const REV: u32 = 1;

/// Guard-rail material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailKind {
    /// Steel W-beam (tarmac).
    Steel,
    /// Timber-faced steel (gravel, forest).
    Timber,
}

/// One run of rail along a road side.
#[derive(Clone, Debug, PartialEq)]
pub struct RailRun {
    pub road: u32,
    /// +1 left, −1 right of the road's direction.
    pub side: i8,
    /// Stations (m).
    pub from: f32,
    pub to: f32,
    pub kind: RailKind,
    /// Foot of the rail line, one point per road station (on the ground).
    pub pts: Vec<Vec3>,
}

/// Warning-triangle pictograms ([`SignKind::Warning`]). `_L`/`_R`: the hand as the driver
/// facing the sign sees it.
pub mod warn {
    pub const HAIRPIN_L: u8 = 0;
    pub const HAIRPIN_R: u8 = 1;
    pub const DOUBLE_L: u8 = 2;
    pub const DOUBLE_R: u8 = 3;
    pub const CREST: u8 = 4;
    pub const JUNCTION_L: u8 = 5;
    pub const JUNCTION_R: u8 = 6;
    pub const CROSSROAD: u8 = 7;
    pub const STEEP: u8 = 8;
    pub const ROCKS: u8 = 9;
    pub const DEER: u8 = 10;
    pub const BRIDGE: u8 = 11;
    pub const JUMP: u8 = 12;
    pub const SLIPPERY: u8 = 13;
    pub const LOOSE: u8 = 14;
    pub const BEND_L: u8 = 15;
    pub const BEND_R: u8 = 16;
    /// How many there are.
    pub const COUNT: u8 = 17;
}

/// What a sign or post is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignKind {
    /// Warning triangle ([`warn`] pictogram).
    Warning(u8),
    /// Speed limit (km/h); 0 = end of limit.
    Speed(u8),
    /// Town name board (entering).
    TownBoard,
    /// Town name board struck through (leaving).
    TownEnd,
    KmPost(u16),
    Reflector,
    StreetLight,
    /// A red and white chevron board; +1 the road turns left as the board's viewer sees it.
    Chevron(i8),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sign {
    /// Foot of the post, on the ground.
    pub pos: Vec3,
    /// Facing (radians about Y; the face's normal is `(sin, 0, cos)`).
    pub yaw: f32,
    pub kind: SignKind,
}

/// A road condition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PatchKind {
    /// A height dip (m deep); grip unchanged.
    Pothole { depth: f32 },
    /// `Surface::Wet`.
    Wet,
    /// `Surface::Mud`.
    Mud,
    /// `Surface::Loose` plus corrugation.
    Washout,
    /// `Surface::Snow`: shaded ice.
    Ice,
    /// A darker patch repair: looks only.
    Repair,
}

/// One condition patch: a capsule (a round patch when `a == b`) on the road.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Patch {
    pub a: Vec2,
    pub b: Vec2,
    pub radius: f32,
    pub kind: PatchKind,
    /// Distance along the road at `a` (m): the washout's corrugation runs on it.
    pub s0: f32,
}

impl Patch {
    /// Where `p` is along the axis (m from `a`) and how far from it.
    pub fn local(&self, p: Vec2) -> (f32, f32) {
        let d = self.b - self.a;
        let l = d.length();
        if l < 1e-4 {
            return (0.0, p.distance(self.a));
        }
        let t = ((p - self.a).dot(d) / l).clamp(0.0, l);
        (t, p.distance(self.a + d * (t / l)))
    }

    pub fn centre(&self) -> Vec2 {
        (self.a + self.b) * 0.5
    }
}

/// Patches bucketed in square cells: (cell key, patch index), sorted.
#[derive(Clone, Debug, Default, PartialEq)]
struct PatchGrid {
    keys: Vec<(u32, u32)>,
}

const CELL: f32 = 16.0;
const GRID_ORIGIN: f32 = -8192.0;

fn cell_of(x: f32, z: f32) -> (u32, u32) {
    let f = |v: f32| (((v - GRID_ORIGIN) / CELL).floor().max(0.0) as u32).min(0xFFFF);
    (f(x), f(z))
}

fn key(i: u32, j: u32) -> u32 {
    (i << 16) | j
}

impl PatchGrid {
    fn new(patches: &[Patch]) -> PatchGrid {
        let mut keys = Vec::new();
        for (k, p) in patches.iter().enumerate() {
            let r = Vec2::splat(p.radius);
            let (lo, hi) = (p.a.min(p.b) - r, p.a.max(p.b) + r);
            let (i0, j0) = cell_of(lo.x, lo.y);
            let (i1, j1) = cell_of(hi.x, hi.y);
            for i in i0..=i1 {
                for j in j0..=j1 {
                    keys.push((key(i, j), k as u32));
                }
            }
        }
        keys.sort_unstable();
        PatchGrid { keys }
    }

    /// Indices of the patches whose cells hold (x, z).
    fn at(&self, x: f32, z: f32) -> &[(u32, u32)] {
        let (i, j) = cell_of(x, z);
        let k = key(i, j);
        let a = self.keys.partition_point(|e| e.0 < k);
        let b = a + self.keys[a..].partition_point(|e| e.0 == k);
        &self.keys[a..b]
    }
}

/// Everything B places. `Default` = none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Furniture {
    pub rails: Vec<RailRun>,
    /// Stone blocks on the outside of gravel and forest corners with a drop and no rail:
    /// (foot on the ground, radius). Solid.
    pub blocks: Vec<(Vec3, f32)>,
    pub signs: Vec<Sign>,
    /// Sorted (by position) and grid-indexed.
    pub patches: Vec<Patch>,
    grid: PatchGrid,
}

impl Furniture {
    /// From parts: sorts the patches and indexes them.
    pub fn new(rails: Vec<RailRun>, blocks: Vec<(Vec3, f32)>, signs: Vec<Sign>, mut patches: Vec<Patch>) -> Furniture {
        patches.sort_by(|p, q| p.a.x.total_cmp(&q.a.x).then(p.a.y.total_cmp(&q.a.y)).then(p.b.x.total_cmp(&q.b.x)).then(p.b.y.total_cmp(&q.b.y)));
        let grid = PatchGrid::new(&patches);
        Furniture { rails, blocks, signs, patches, grid }
    }

    /// Patches that may hold (x, z), in sorted order.
    fn near(&self, x: f32, z: f32) -> impl Iterator<Item = &Patch> {
        self.grid.at(x, z).iter().map(|&(_, k)| &self.patches[k as usize])
    }

    /// Total rail length (m).
    pub fn rail_length(&self) -> f32 {
        self.rails.iter().map(|r| r.to - r.from).sum()
    }
}

// ------------------------------------------------------------------------ the rules

/// Rail: the drop 8 m past the edge must be over this (m)…
pub const RAIL_DROP: f32 = 3.0;
/// …or over this anywhere, corner or not. (WORLD_PASS says 6 m; on the cliffy committed land
/// that put 4-6 km of rail up, against the 1.2-1.6 km it aims for.)
pub const RAIL_DROP_ANY: f32 = 8.0;
/// Where the drop is measured, past the edge (m).
const DROP_AT: f32 = 8.0;
/// Rail centre past the road edge (m): shoulder plus 0.4.
pub const RAIL_OFF: f32 = SHOULDER + 0.4;
/// Height of the rail's top over the ground (m).
pub const RAIL_TOP: f32 = 0.75;
pub const RAIL_RADIUS: f32 = 0.12;
/// Signs stand this far past the edge (m).
const SIGN_OFF: f32 = SHOULDER + 1.0;
/// Nothing stands within this of a junction (m), where it would be on the other road.
const JUNCTION_CLEAR: f32 = 24.0;
/// Lateral acceleration the corner speeds are worked out with (m/s²).
const A_LAT: f32 = 8.0;

fn xz(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}

fn left(road: &Road, i: usize) -> Vec2 {
    -road.dir[i].perp()
}

fn at(road: &Road, i: usize, lat: f32) -> Vec2 {
    xz(road.pts[i]) + left(road, i) * lat
}

/// Facing along the road at station `i` (towards higher stations).
fn along(road: &Road, i: usize) -> f32 {
    libm::atan2f(road.dir[i].x, road.dir[i].y)
}

/// The road's surface height at lateral `lat` (banking included, no crown).
fn edge_y(road: &Road, i: usize, lat: f32) -> f32 {
    road.pts[i].y + road.bank[i] * lat
}

fn mix(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^ (h >> 33)
}

/// A seeded number in 0..1 from three keys.
fn rnd(seed: u64, a: u64, b: u64) -> f32 {
    (mix(seed ^ mix(a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ mix(b.wrapping_add(0x632B_E59B_D9B4_E019)))) >> 40) as f32 / (1u64 << 24) as f32
}

/// Smooth 1-D value noise in 0..1.
fn noise1(seed: u64, key: u64, x: f32) -> f32 {
    let i = x.floor();
    let f = x - i;
    let f = f * f * (3.0 - 2.0 * f);
    let i = i as i64 as u64;
    let (a, b) = (rnd(seed, key, i), rnd(seed, key, i.wrapping_add(1)));
    a + (b - a) * f
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The town the speed signs and boards mark: (centre, radius). Until the town mask lands
/// (stream A), the village circle the buildings use.
pub fn town(net: &RoadNet) -> (Vec2, f32) {
    (net.places().village, super::buildings::VILLAGE_R)
}

/// The posted speed limit (km/h, rounded to 10) on road `r` at station `i`: the traffic's
/// own limit for it (`traffic::limit`, or the road's override), so the signs say what the
/// traffic drives.
pub fn posted_kmh(net: &RoadNet, r: usize, _i: usize) -> u8 {
    let nr = &net.roads[r];
    let v = nr.limit.unwrap_or_else(|| super::traffic::limit(nr.kind)) * 3.6;
    ((v / 10.0).round() * 10.0).clamp(10.0, 250.0) as u8
}

/// What every rule reads, worked out once per road.
struct RoadInfo {
    /// Near a junction (within [`JUNCTION_CLEAR`]).
    near_j: Vec<bool>,
    bridge: Vec<bool>,
    corners: Vec<stage::Corner>,
    /// Per station: the corner it's in (index into `corners`).
    corner_at: Vec<Option<usize>>,
    /// Per corner: its apex station and the outside's lateral sign.
    apex: Vec<(usize, f32)>,
    in_town: Vec<bool>,
    tarmac: bool,
}

fn road_info(net: &RoadNet, r: usize, town: (Vec2, f32)) -> RoadInfo {
    let nr = &net.roads[r];
    let road = &nr.road;
    let n = road.len();
    let mut near_j = vec![false; n];
    for j in &net.junctions {
        // Every station near it, whichever road the junction lists.
        for (i, p) in road.pts.iter().enumerate() {
            if xz(*p).distance_squared(j.pos) < JUNCTION_CLEAR * JUNCTION_CLEAR {
                near_j[i] = true;
            }
        }
    }
    let bridge = (0..n).map(|i| nr.bridge.get(i).copied().unwrap_or(false)).collect();
    let corners = stage::corners(road);
    let mut corner_at = vec![None; n];
    let mut apex = Vec::with_capacity(corners.len());
    for (k, c) in corners.iter().enumerate() {
        let (a, b) = (road.station(c.start), road.station(c.end));
        for slot in &mut corner_at[a..=b] {
            *slot = Some(k);
        }
        let ap = (a..=b).max_by(|&x, &y| road.curv[x].abs().total_cmp(&road.curv[y].abs())).unwrap_or(a);
        // The outside of a left turn is its right (lateral −).
        apex.push((ap, -road.curv[ap].signum()));
    }
    let in_town = road.pts.iter().map(|p| xz(*p).distance(town.0) < town.1).collect();
    RoadInfo { near_j, bridge, corners, corner_at, apex, in_town, tarmac: matches!(nr.kind, RoadKind::BRoad | RoadKind::Street) }
}

/// The furniture for a road net on its terrain.
pub fn build(net: &RoadNet, heights: &Heightfield) -> Furniture {
    let town = town(net);
    let infos: Vec<RoadInfo> = (0..net.roads.len()).map(|r| road_info(net, r, town)).collect();
    let ground = |p: Vec2| {
        let hf = heights.height_at(p.x, p.y);
        super::profile::ground(net, p, hf).unwrap_or(hf)
    };
    let rails = rails(net, heights, &infos, &ground);
    let blocks = blocks(net, heights, &infos, &rails, &ground);
    let patches = patches(net, heights, &infos);
    let signs = signs(net, heights, &infos, &rails, &patches, &ground);
    Furniture::new(rails, blocks, signs, patches)
}

// ------------------------------------------------------------------------ rails

/// The drop (m) from the road edge on `side` to the ground [`DROP_AT`] past it.
fn drop_at(road: &Road, heights: &Heightfield, i: usize, side: f32) -> f32 {
    drop_over(road, heights, i, side, DROP_AT)
}

/// The drop (m) from the road edge on `side` to the ground `out` m past it.
fn drop_over(road: &Road, heights: &Heightfield, i: usize, side: f32, out: f32) -> f32 {
    let half = road.width[i] * 0.5;
    let far = at(road, i, side * (half + out));
    edge_y(road, i, side * half) - heights.height_at(far.x, far.y)
}

/// Stations along a road and a side where the rule wants a rail (before extending).
fn rail_wanted(road: &Road, info: &RoadInfo, heights: &Heightfield, side: f32, rule: &RailRule) -> Vec<bool> {
    (0..road.len())
        .map(|i| {
            if info.near_j[i] || info.bridge[i] {
                return false;
            }
            let drop = drop_over(road, heights, i, side, rule.at);
            if drop <= rule.drop {
                return false;
            }
            if drop > rule.any {
                return true;
            }
            match info.corner_at[i] {
                Some(k) if info.apex[k].1 == side => {
                    let g = info.corners[k].grade;
                    if info.tarmac { g <= rule.grade || road.pts[i].y > 250.0 } else { g <= rule.loose_grade }
                }
                _ => false,
            }
        })
        .collect()
}

/// Runs (first, last station) of `want` extended by `grow` stations each way, gaps under
/// `gap` merged, shorter than `min` dropped, and never over a `blocked` station.
fn runs(want: &[bool], blocked: &[bool], grow: usize, gap: usize, min: usize) -> Vec<(usize, usize)> {
    let n = want.len();
    let mut on = vec![false; n];
    for (i, &w) in want.iter().enumerate() {
        if w {
            for slot in &mut on[i.saturating_sub(grow)..(i + grow + 1).min(n)] {
                *slot = true;
            }
        }
    }
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        if !on[i] || blocked[i] {
            i += 1;
            continue;
        }
        let a = i;
        while i < n && on[i] && !blocked[i] {
            i += 1;
        }
        let b = i - 1;
        match out.last_mut() {
            // Close to the last one, with nothing in between that must stay open.
            Some(last) if a - last.1 <= gap && !blocked[last.1..a].iter().any(|&x| x) => last.1 = b,
            _ => out.push((a, b)),
        }
    }
    out.retain(|&(a, b)| b - a >= min);
    out
}

/// The rail rule's numbers.
#[derive(Clone, Copy, Debug)]
struct RailRule {
    /// Drop measured this far past the edge (m), over `drop` on a corner's outside…
    at: f32,
    drop: f32,
    /// …or over `any` anywhere.
    any: f32,
    /// Corners up to this grade (tarmac; gravel and forest: `loose_grade`, stone blocks on
    /// the outside of the rest).
    grade: u8,
    loose_grade: u8,
    /// Extended this far each way, gaps under `gap` merged, at least `min` long (m).
    grow: f32,
    gap: f32,
    min: f32,
}

const RAIL_RULE: RailRule = RailRule { at: DROP_AT, drop: RAIL_DROP, any: RAIL_DROP_ANY, grade: 4, loose_grade: 0, grow: 20.0, gap: 30.0, min: 24.0 };

fn rails(net: &RoadNet, heights: &Heightfield, infos: &[RoadInfo], ground: &dyn Fn(Vec2) -> f32) -> Vec<RailRun> {
    rails_with(net, heights, infos, ground, &RAIL_RULE)
}

fn rails_with(net: &RoadNet, heights: &Heightfield, infos: &[RoadInfo], ground: &dyn Fn(Vec2) -> f32, rule: &RailRule) -> Vec<RailRun> {
    let st = |m: f32| (m / STEP).round() as usize;
    let mut out = Vec::new();
    for (r, nr) in net.roads.iter().enumerate() {
        let info = &infos[r];
        let road = &nr.road;
        let blocked: Vec<bool> = (0..road.len()).map(|i| info.near_j[i] || info.bridge[i]).collect();
        for side in [1.0f32, -1.0] {
            let want = rail_wanted(road, info, heights, side, rule);
            for (a, b) in runs(&want, &blocked, st(rule.grow), st(rule.gap), st(rule.min)) {
                let pts = (a..=b)
                    .map(|i| {
                        let p = at(road, i, side * (road.width[i] * 0.5 + RAIL_OFF));
                        Vec3::new(p.x, ground(p), p.y)
                    })
                    .collect();
                let kind = if info.tarmac { RailKind::Steel } else { RailKind::Timber };
                out.push(RailRun { road: r as u32, side: side as i8, from: a as f32 * STEP, to: b as f32 * STEP, kind, pts });
            }
        }
    }
    out
}

/// Stone blocks: one every 6-8 m on the outside of gravel and forest corners (grade 1-4) where
/// the ground drops over [`RAIL_DROP`] and there is no rail.
fn blocks(net: &RoadNet, heights: &Heightfield, infos: &[RoadInfo], rails: &[RailRun], ground: &dyn Fn(Vec2) -> f32) -> Vec<(Vec3, f32)> {
    let mut out = Vec::new();
    for (r, nr) in net.roads.iter().enumerate() {
        let info = &infos[r];
        if info.tarmac {
            continue;
        }
        let road = &nr.road;
        let seed = road_seed(net, r);
        for (k, c) in info.corners.iter().enumerate() {
            let (_, side) = info.apex[k];
            if !(1..=4).contains(&c.grade) {
                continue;
            }
            let (a, b) = (road.station(c.start), road.station(c.end));
            let mut i = a;
            while i <= b {
                let s = i as f32 * STEP;
                let railed = rails.iter().any(|x| x.road as usize == r && x.side as f32 == side && s >= x.from - 2.0 && s <= x.to + 2.0);
                if !railed && !info.near_j[i] && !info.bridge[i] && drop_at(road, heights, i, side) > RAIL_DROP {
                    let radius = 0.4 + rnd(seed, 30, i as u64) * 0.2;
                    let p = at(road, i, side * (road.width[i] * 0.5 + SHOULDER + 0.2 + radius));
                    out.push((Vec3::new(p.x, ground(p), p.y), radius));
                }
                i += 3 + (rnd(seed, 31, i as u64) * 1.99) as usize;
            }
        }
    }
    out
}

/// The solid furniture (guard rails, stone blocks, street-light stubs) as walls, appended to `out`: a rail
/// is a wall every 2 stations, [`RAIL_RADIUS`] thick, its top [`RAIL_TOP`] over the ground.
pub fn walls(f: &Furniture, out: &mut Vec<Wall>) {
    for r in &f.rails {
        let n = r.pts.len();
        let mut k = 0;
        while k + 1 < n {
            let j = (k + 2).min(n - 1);
            let (a, b) = (r.pts[k], r.pts[j]);
            out.push(Wall::new(xz(a), xz(b), RAIL_RADIUS, a.y.max(b.y) + RAIL_TOP));
            k = j;
        }
    }
    for &(p, radius) in &f.blocks {
        let q = xz(p);
        out.push(Wall::new(q, q + Vec2::X * 0.01, radius, p.y + 0.6));
    }
    for s in &f.signs {
        if s.kind == SignKind::StreetLight {
            let p = xz(s.pos);
            out.push(Wall::new(p, p + Vec2::X * 0.01, 0.15, s.pos.y + 5.0));
        }
    }
}

// ------------------------------------------------------------------------ conditions

/// The vertical curvature (1/m, − = crest) at station `i`, over ±10 m.
fn kv(road: &Road, i: usize) -> Option<f32> {
    let k = 5;
    if i < k || i + k >= road.len() {
        return None;
    }
    let d = k as f32 * STEP;
    Some((road.pts[i + k].y - 2.0 * road.pts[i].y + road.pts[i - k].y) / (d * d))
}

/// The grade (rise per metre) at station `i`, over ±10 m.
fn grade(road: &Road, i: usize) -> f32 {
    let (a, b) = (i.saturating_sub(5), (i + 5).min(road.len() - 1));
    if b == a {
        return 0.0;
    }
    (road.pts[b].y - road.pts[a].y) / ((b - a) as f32 * STEP)
}

/// Capsules along the road from station `a` to `b` (every 2 stations), `lat` m off the centre.
fn strip(road: &Road, a: usize, b: usize, lat: &dyn Fn(usize) -> f32, radius: f32, kind: PatchKind, out: &mut Vec<Patch>) {
    let mut i = a;
    while i < b {
        let j = (i + 2).min(b);
        out.push(Patch { a: at(road, i, lat(i)), b: at(road, j, lat(j)), radius, kind, s0: i as f32 * STEP });
        i = j;
    }
}

/// Seed for one road's rules.
fn road_seed(net: &RoadNet, r: usize) -> u64 {
    mix(net.seed ^ 0xF00F_0001 ^ ((r as u64) << 32))
}

/// Runs of stations where `f` holds: (first, one past the last).
fn spans(n: usize, f: impl Fn(usize) -> bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if !f(i) {
            i += 1;
            continue;
        }
        let a = i;
        while i < n && f(i) {
            i += 1;
        }
        out.push((a, i));
    }
    out
}

fn patches(net: &RoadNet, heights: &Heightfield, infos: &[RoadInfo]) -> Vec<Patch> {
    let mut out = Vec::new();
    for (r, nr) in net.roads.iter().enumerate() {
        let road = &nr.road;
        let info = &infos[r];
        let n = road.len();
        if n < 50 {
            continue;
        }
        let seed = road_seed(net, r);
        let wet = |i: usize| net.land.wet(xz(road.pts[i]));
        let clear = |i: usize| !info.near_j[i] && !info.bridge[i];
        // Stations used by a hazard, so two never share a braking zone.
        let mut taken: Vec<(usize, usize)> = Vec::new();
        let free = |taken: &[(usize, usize)], a: usize, b: usize| taken.iter().all(|&(x, y)| b + 40 < x || a > y + 40);
        let half = |i: usize| road.width[i] * 0.5;

        // Ice: the shaded (north-facing) cut on the inside of high forest hairpins.
        if nr.kind == RoadKind::Forest {
            for (k, c) in info.corners.iter().enumerate() {
                let (ap, out_side) = info.apex[k];
                if c.grade != stage::HAIRPIN || road.pts[ap].y < 440.0 {
                    continue;
                }
                let inside = -out_side;
                let cut = at(road, ap, inside * (half(ap) + DROP_AT));
                let normal = heights.smooth_normal_at(cut.x, cut.y);
                let cut_h = heights.height_at(cut.x, cut.y) - edge_y(road, ap, inside * half(ap));
                if cut_h < 0.5 || normal.z < 0.05 {
                    continue;
                }
                let (a, b) = (ap.saturating_sub(8), (ap + 8).min(n - 1));
                strip(road, a, b, &|i| inside * (half(i) - 0.75), 0.75, PatchKind::Ice, &mut out);
                taken.push((a, b));
            }
        }

        // Washouts: steep, wet gravel and forest (10-30 m).
        if matches!(nr.kind, RoadKind::Gravel | RoadKind::Forest) {
            let mut last_washout: Option<usize> = None;
            for (a, e) in spans(n, |i| grade(road, i).abs() > 0.09 && wet(i) > 0.5 && clear(i)) {
                let mid = (a + e) / 2;
                let len = 5 + (rnd(seed, 1, mid as u64) * 10.0) as usize;
                let (pa, pb) = (mid.saturating_sub(len / 2), (mid + len / 2).min(n - 1));
                // At most one every 300 m.
                if free(&taken, pa, pb) && last_washout.is_none_or(|l| pa > l + 150) {
                    strip(road, pa, pb, &|_| 0.0, (half(mid) - 0.1).max(1.0), PatchKind::Washout, &mut out);
                    taken.push((pa, pb));
                    last_washout = Some(pb);
                }
            }
        }

        // Mud: wet gravel, 15-40 m (forest roads already have theirs, `roads::surface`).
        if nr.kind == RoadKind::Gravel {
            for (a, e) in spans(n, |i| wet(i) > 0.70 && clear(i)) {
                let mid = (a + e) / 2;
                let len = 8 + (rnd(seed, 2, mid as u64) * 12.0) as usize;
                let (pa, pb) = (mid.saturating_sub(len / 2), (mid + len / 2).min(n - 1));
                if free(&taken, pa, pb) {
                    strip(road, pa, pb, &|_| 0.0, (half(mid) - 0.1).max(1.0), PatchKind::Mud, &mut out);
                    taken.push((pa, pb));
                }
            }
        }

        // Wet tarmac: damp dips (a low point of the road over ±40 m), 8-25 m.
        if info.tarmac {
            for i in 20..n.saturating_sub(20) {
                let y = road.pts[i].y;
                let low = (i - 20..=i + 20).all(|j| j == i || road.pts[j].y > y || (road.pts[j].y == y && j > i));
                if !low || wet(i) <= 0.75 || !clear(i) {
                    continue;
                }
                let len = 2 + (rnd(seed, 3, i as u64) * 4.5) as usize;
                let (pa, pb) = (i.saturating_sub(len), (i + len).min(n - 1));
                if free(&taken, pa, pb) {
                    strip(road, pa, pb, &|_| 0.0, (half(i) - 0.2).max(1.0), PatchKind::Wet, &mut out);
                    taken.push((pa, pb));
                }
            }
        }

        // Potholes and repairs: old stretches of roads 0 and 1, outside the town.
        if r <= 1 && info.tarmac {
            let mut s = 60.0 + rnd(seed, 4, 0) * 100.0;
            let mut k = 0u64;
            while (s / STEP) as usize + 2 < n {
                let i = road.station(s);
                k += 1;
                let old = noise1(seed, 5, s / 150.0) > 0.55;
                if old && clear(i) && !info.in_town[i] && free(&taken, i, i) {
                    // In a wheel track of either lane; now and then a second one by it.
                    let lane = if rnd(seed, 6, k) < 0.5 { -1.0 } else { 1.0 };
                    let lat = lane * (half(i) * 0.5 + (rnd(seed, 7, k) - 0.5) * 1.6);
                    let dia = 0.6 + rnd(seed, 8, k) * 0.8;
                    let depth = 0.04 + rnd(seed, 9, k) * 0.04;
                    let p = at(road, i, lat);
                    // A darker repair under some, bigger than the hole.
                    if rnd(seed, 10, k) < 0.5 {
                        let q = at(road, (i + 1).min(n - 1), lat);
                        out.push(Patch { a: p, b: q, radius: dia * 0.5 + 0.5 + rnd(seed, 11, k) * 0.6, kind: PatchKind::Repair, s0: s });
                    }
                    out.push(Patch { a: p, b: p, radius: dia * 0.5, kind: PatchKind::Pothole { depth }, s0: s });
                    if rnd(seed, 12, k) < 0.35 {
                        let q = p + road.dir[i] * (1.2 + rnd(seed, 13, k) * 1.5);
                        out.push(Patch { a: q, b: q, radius: dia * 0.35, kind: PatchKind::Pothole { depth: (depth * 0.8).max(0.04) }, s0: s });
                    }
                    taken.push((i, i));
                }
                s += 120.0 + rnd(seed, 14, k) * 130.0;
            }
        }
    }
    out
}

/// How far the ground dips at (x, z) (potholes, washout corrugation), m ≥ 0, subtracted from
/// the ground height.
pub fn dip(f: &Furniture, x: f32, z: f32) -> f32 {
    if f.patches.is_empty() {
        return 0.0;
    }
    let p = Vec2::new(x, z);
    let mut d = 0.0f32;
    for patch in f.near(x, z) {
        match patch.kind {
            PatchKind::Pothole { depth } => {
                let r = p.distance(patch.a);
                if r < patch.radius {
                    d = d.max(depth * (1.0 - smoothstep(0.7, 1.0, r / patch.radius)));
                }
            }
            PatchKind::Washout => {
                let (t, off) = patch.local(p);
                if off < patch.radius {
                    let fade = 1.0 - smoothstep(patch.radius - 0.6, patch.radius, off);
                    let s = patch.s0 + t;
                    let wave = 0.02 * (1.0 + libm::sinf(core::f32::consts::TAU * s / 0.9));
                    d = d.max(wave * fade);
                }
            }
            _ => {}
        }
    }
    d
}

/// The surface a patch puts at (x, z), if any.
pub fn surface_override(f: &Furniture, x: f32, z: f32) -> Option<Surface> {
    if f.patches.is_empty() {
        return None;
    }
    let p = Vec2::new(x, z);
    for patch in f.near(x, z) {
        let s = match patch.kind {
            PatchKind::Wet => Surface::Wet,
            PatchKind::Mud => Surface::Mud,
            PatchKind::Washout => Surface::Loose,
            PatchKind::Ice => Surface::Snow,
            PatchKind::Pothole { .. } | PatchKind::Repair => continue,
        };
        if patch.local(p).1 < patch.radius {
            return Some(s);
        }
    }
    None
}

// ------------------------------------------------------------------------ signs

/// A sign wanted at station `i` for the traffic going `dir` (+1 = towards higher stations).
struct Want {
    road: usize,
    i: usize,
    dir: i32,
    kind: SignKind,
    prio: u8,
}

/// Priority of town boards and speed signs (they are placed first and may stand together).
const REGULATORY: u8 = 7;

/// A sign for traffic going `dir` at station `i`: `off` past the right edge, facing the cars.
fn sign_at(road: &Road, i: usize, dir: i32, off: f32, kind: SignKind, ground: &dyn Fn(Vec2) -> f32) -> Sign {
    let d = dir as f32;
    let p = at(road, i, -d * (road.width[i] * 0.5 + off));
    let yaw = along(road, i) + if dir > 0 { core::f32::consts::PI } else { 0.0 };
    Sign { pos: Vec3::new(p.x, ground(p), p.y), yaw, kind }
}

/// The station `m` metres before station `i` for traffic going `dir`, if on the road.
fn before(n: usize, i: usize, dir: i32, m: f32) -> Option<usize> {
    let k = (m / STEP).round() as i64;
    let j = i as i64 - dir as i64 * k;
    (j >= 0 && (j as usize) < n).then_some(j as usize)
}

/// Chevron boards round the outside of the corners that need them; which corners got them.
fn chevrons(road: &Road, info: &RoadInfo, ground: &dyn Fn(Vec2) -> f32, out: &mut Vec<Sign>) -> Vec<bool> {
    let n = road.len();
    let speed = |i: usize| {
        let k = (i.saturating_sub(5)..=(i + 5).min(n - 1)).map(|j| road.curv[j].abs()).fold(1e-4, f32::max);
        (A_LAT / k).sqrt().min(45.0)
    };
    let mut chevroned = vec![false; info.corners.len()];
    for (k, c) in info.corners.iter().enumerate() {
        let (ap, out_side) = info.apex[k];
        let boards = if c.grade == stage::HAIRPIN {
            5
        } else if info.tarmac && (3..=4).contains(&c.grade) {
            // The speed 150 m before (either way in) is 30 km/h over the corner's.
            let a = before(n, road.station(c.start), 1, 150.0).map_or(0.0, |b| speed(b) - speed(ap));
            let b = before(n, road.station(c.end), -1, 150.0).map_or(0.0, |b| speed(b) - speed(ap));
            if a.max(b) >= 30.0 / 3.6 { 3 } else { 0 }
        } else if !info.tarmac && c.grade <= 2 {
            let fast = info.corners[..k].iter().rev().take_while(|p| p.end > c.start - 150.0).all(|p| p.grade >= 4);
            if fast || c.crest || c.crest_into { 3 } else { 0 }
        } else {
            0
        };
        if boards == 0 {
            continue;
        }
        chevroned[k] = true;
        let turn = road.curv[ap].signum() as i8;
        // Hairpins are tight: boards 6 m apart round the arc.
        let spread = 3i64;
        for b in 0..boards {
            let j = (ap as i64 + (b as i64 - (boards as i64 - 1) / 2) * spread).clamp(0, n as i64 - 1) as usize;
            if info.near_j[j] {
                continue;
            }
            let p = at(road, j, out_side * (road.width[j] * 0.5 + RAIL_OFF + 0.7));
            // Facing the inside of the bend: both ways round see it.
            let to_road = -left(road, j) * out_side;
            let yaw = libm::atan2f(to_road.x, to_road.y);
            out.push(Sign { pos: Vec3::new(p.x, ground(p), p.y), yaw, kind: SignKind::Chevron(turn) });
        }
    }
    chevroned
}

/// Warning triangles wanted on road `r` for traffic going `dir`.
#[allow(clippy::too_many_arguments)]
fn warnings(net: &RoadNet, heights: &Heightfield, r: usize, info: &RoadInfo, chevroned: &[bool], patches: &[Patch], dir: i32, wants: &mut Vec<Want>) {
    let nr = &net.roads[r];
    let road = &nr.road;
    let n = road.len();
    let d = dir as f32;
    let half = |i: usize| road.width[i] * 0.5;
    let warn_m = if info.tarmac { 120.0 } else { 70.0 };
    let mut push = |hazard: usize, kind: u8, prio: u8| {
        if let Some(i) = before(n, hazard, dir, warn_m) {
            wants.push(Want { road: r, i, dir, kind: SignKind::Warning(kind), prio });
        }
    };
    let hand = |left: bool, l: u8, rt: u8| if left { l } else { rt };
    // The first station of a run as this way meets it.
    let first = |a: usize, e: usize| if dir > 0 { a } else { e - 1 };

    // Corners in the order this way meets them.
    let order: Vec<usize> = if dir > 0 { (0..info.corners.len()).collect() } else { (0..info.corners.len()).rev().collect() };
    for (o, &k) in order.iter().enumerate() {
        let c = &info.corners[k];
        let (ap, _) = info.apex[k];
        let entry = road.station(if dir > 0 { c.start } else { c.end });
        let turns_left = road.curv[ap] * d > 0.0;
        if c.grade == stage::HAIRPIN {
            push(entry, hand(turns_left, warn::HAIRPIN_L, warn::HAIRPIN_R), 5);
            continue;
        }
        if c.grade <= 2
            && let Some(&k2) = order.get(o + 1)
        {
            let c2 = &info.corners[k2];
            let gap = if dir > 0 { c2.start - c.end } else { c.start - c2.end };
            let opposite = road.curv[info.apex[k2].0] * road.curv[ap] < 0.0;
            if c2.grade <= 2 && c2.grade != stage::HAIRPIN && gap < 150.0 && opposite {
                push(entry, hand(turns_left, warn::DOUBLE_L, warn::DOUBLE_R), 3);
                continue;
            }
        }
        if (1..=2).contains(&c.grade) && !chevroned[k] {
            push(entry, hand(turns_left, warn::BEND_L, warn::BEND_R), 0);
        }
    }

    // Blind crests (R_v under 100 m), and jumps (under 55 m: the yumps).
    for (a, e) in spans(n, |i| kv(road, i).is_some_and(|v| v < -1.0 / 100.0)) {
        let top = (a..e).map(|i| kv(road, i).unwrap_or(0.0)).fold(0.0f32, f32::min);
        push(first(a, e), if top < -1.0 / 55.0 { warn::JUMP } else { warn::CREST }, 4);
    }

    // Side roads joining the B-road.
    if nr.kind == RoadKind::BRoad {
        for j in &net.junctions {
            let Some(&(_, ji)) = j.roads.iter().find(|x| x.0 == r) else { continue };
            if ji == 0 || ji + 1 >= n {
                continue;
            }
            let (mut l, mut rt) = (false, false);
            for &(r2, j2) in j.roads.iter().filter(|x| x.0 != r) {
                let o = &net.roads[r2].road;
                for k in [j2 as i64 - 10, j2 as i64 + 10] {
                    if k >= 0 && (k as usize) < o.len() {
                        let side = (xz(o.pts[k as usize]) - j.pos).dot(left(road, ji)) * d;
                        if side > 0.0 { l = true } else { rt = true }
                    }
                }
            }
            let kind = match (l, rt) {
                (true, true) => warn::CROSSROAD,
                (true, false) => warn::JUNCTION_L,
                (false, true) => warn::JUNCTION_R,
                _ => continue,
            };
            push(ji, kind, 6);
        }
    }

    // Steep descents: over 10 % for more than 200 m, going down.
    for (a, e) in spans(n, |i| grade(road, i) * d < -0.10) {
        if (e - a) as f32 * STEP > 200.0 {
            push(first(a, e), warn::STEEP, 2);
        }
    }

    // Falling rocks: a cut bank over 6 m for more than 60 m, either side.
    let cut = |i: usize| [1.0f32, -1.0].iter().any(|&s| -drop_at(road, heights, i, s) > 6.0);
    for (a, e) in spans(n, cut) {
        if (e - a) as f32 * STEP > 60.0 {
            push(first(a, e), warn::ROCKS, 2);
        }
    }

    // Bridges (narrow).
    for (a, e) in spans(n, |i| info.bridge[i]) {
        push(first(a, e), warn::BRIDGE, 2);
    }

    // Deer on forest roads, every 1.5 km.
    if nr.kind == RoadKind::Forest {
        let mut s = 750.0 + rnd(road_seed(net, r), 20, dir as u64) * 300.0;
        while ((s / STEP) as usize) < n {
            push(road.station(s), warn::DEER, 1);
            s += 1500.0;
        }
    }

    // Slippery and loose before wet patches and washouts on this road (from the first capsule
    // of each strip as this way meets it).
    for p in patches {
        let kind = match p.kind {
            PatchKind::Wet => warn::SLIPPERY,
            PatchKind::Washout => warn::LOOSE,
            _ => continue,
        };
        let starts = if dir > 0 { !patches.iter().any(|q| q.kind == p.kind && q.b == p.a) } else { !patches.iter().any(|q| q.kind == p.kind && q.a == p.b) };
        if !starts {
            continue;
        }
        let end = if dir > 0 { p.a } else { p.b };
        let Some(np) = road.nearest(end, half(0) + 2.0) else { continue };
        push(np.i, kind, 1);
    }
}

fn signs(net: &RoadNet, heights: &Heightfield, infos: &[RoadInfo], rails: &[RailRun], patches: &[Patch], ground: &dyn Fn(Vec2) -> f32) -> Vec<Sign> {
    let mut out = Vec::new();
    let mut wants: Vec<Want> = Vec::new();
    // Is there a rail on this road and side at station i?
    let railed = |r: usize, side: f32, i: usize| {
        let s = i as f32 * STEP;
        rails.iter().any(|x| x.road as usize == r && x.side as f32 == side && s >= x.from - 4.0 && s <= x.to + 4.0)
    };
    for (r, nr) in net.roads.iter().enumerate() {
        let road = &nr.road;
        let info = &infos[r];
        let n = road.len();
        if n < 10 {
            continue;
        }
        let chevroned = chevrons(road, info, ground, &mut out);
        for dir in [1i32, -1] {
            warnings(net, heights, r, info, &chevroned, patches, dir, &mut wants);
        }

        // Posted speed: wherever it changes along the road.
        for i in 1..n {
            let (a, b) = (posted_kmh(net, r, i - 1), posted_kmh(net, r, i));
            if a != b {
                wants.push(Want { road: r, i, dir: 1, kind: SignKind::Speed(b), prio: REGULATORY });
                wants.push(Want { road: r, i: i - 1, dir: -1, kind: SignKind::Speed(a), prio: REGULATORY });
            }
        }
        // The town limits: the board (and the town's limit if it differs) on the right going
        // in; the struck-through board for those leaving, on the other side.
        if nr.kind != RoadKind::Street {
            for i in 1..n {
                if info.in_town[i] == info.in_town[i - 1] {
                    continue;
                }
                let dir_in = if info.in_town[i] { 1 } else { -1 };
                let (inside, outside) = if dir_in > 0 { (i, i - 1) } else { (i - 1, i) };
                wants.push(Want { road: r, i: inside, dir: dir_in, kind: SignKind::TownBoard, prio: REGULATORY + 1 });
                wants.push(Want { road: r, i: inside, dir: -dir_in, kind: SignKind::TownEnd, prio: REGULATORY + 1 });
                let (vi, vo) = (posted_kmh(net, r, inside), posted_kmh(net, r, outside));
                if vi != vo {
                    wants.push(Want { road: r, i: inside, dir: dir_in, kind: SignKind::Speed(vi), prio: REGULATORY });
                    wants.push(Want { road: r, i: inside, dir: -dir_in, kind: SignKind::Speed(vo), prio: REGULATORY });
                }
            }
        }
        // Town street mouths: the street's limit, 15 m in, for the cars turning in.
        if nr.kind == RoadKind::Street {
            for j in &net.junctions {
                let Some(&(_, ji)) = j.roads.iter().find(|x| x.0 == r) else { continue };
                let here = posted_kmh(net, r, ji);
                if !j.roads.iter().filter(|x| x.0 != r).any(|x| posted_kmh(net, x.0, x.1) != here) {
                    continue;
                }
                for dir in [1i32, -1] {
                    if let Some(k) = before(n, ji, -dir, 15.0) {
                        wants.push(Want { road: r, i: k, dir, kind: SignKind::Speed(here), prio: REGULATORY });
                    }
                }
            }
        }

        // Km posts on roads 0 and 1, square to the road so both ways read them.
        if r <= 1 {
            let mut km = 1u16;
            while ((km as f32 * 1000.0 / STEP) as usize) < n {
                let i = road.station(km as f32 * 1000.0);
                if !info.near_j[i] {
                    let mut s = sign_at(road, i, 1, SHOULDER + 0.6, SignKind::KmPost(km), ground);
                    s.yaw = along(road, i) + core::f32::consts::FRAC_PI_2;
                    out.push(s);
                }
                km += 1;
            }
        }

        // Reflector posts: rural tarmac (50 m, 25 m in bends under R 150), fast gravel (100 m).
        let every = match nr.kind {
            RoadKind::BRoad => Some(50.0),
            RoadKind::Gravel => Some(100.0),
            _ => None,
        };
        if let Some(every) = every {
            let mut s = 10.0;
            while ((s / STEP) as usize) < n {
                let i = road.station(s);
                let bend = (i.saturating_sub(12)..=(i + 12).min(n - 1)).any(|j| road.curv[j].abs() > 1.0 / 150.0);
                if !info.near_j[i] && !info.bridge[i] && !info.in_town[i] {
                    for side in [1.0f32, -1.0] {
                        if !railed(r, side, i) {
                            let mut p = sign_at(road, i, -side as i32, SHOULDER + 0.15, SignKind::Reflector, ground);
                            // The face towards +yaw is white (seen on the driver's left), the
                            // other amber (on the right).
                            p.yaw = along(road, i) + if side > 0.0 { core::f32::consts::PI } else { 0.0 };
                            out.push(p);
                        }
                    }
                }
                s += if every <= 50.0 && bend { 25.0 } else { every };
            }
        }

        // Street lights on town streets: every 30 m, alternating sides, the arm over the road.
        if nr.kind == RoadKind::Street {
            let mut s = 8.0;
            let mut side = 1;
            while ((s / STEP) as usize) < n {
                let i = road.station(s);
                let mut p = sign_at(road, i, side, SHOULDER + 0.9, SignKind::StreetLight, ground);
                let to_road = left(road, i) * side as f32;
                p.yaw = libm::atan2f(to_road.x, to_road.y);
                out.push(p);
                side = -side;
                s += 30.0;
            }
        }
    }

    // Warning, speed and town signs: the most important first, at most one warning per 50 m
    // each way of each road (regulatory signs only keep off each other), never in a junction
    // mouth (warnings), on a bridge or on another road.
    wants.sort_by(|a, b| b.prio.cmp(&a.prio).then(a.road.cmp(&b.road)).then(a.i.cmp(&b.i)).then(a.dir.cmp(&b.dir)));
    let mut placed: Vec<(usize, i32, usize, bool)> = Vec::new();
    for w in wants {
        let road = &net.roads[w.road].road;
        let info = &infos[w.road];
        let n = road.len();
        let regulatory = w.prio >= REGULATORY;
        for shift in [0i64, -8, 8, -15, 15] {
            let i = w.i as i64 + shift;
            if i < 0 || i as usize >= n {
                continue;
            }
            let i = i as usize;
            if (!regulatory && info.near_j[i]) || info.bridge[i] {
                continue;
            }
            // Regulatory signs at one spot stand side by side; a warning needs 50 m clear.
            let crowded = placed.iter().any(|&(r, d, j, reg)| r == w.road && d == w.dir && if regulatory { reg && j != i && j.abs_diff(i) < 3 } else { j.abs_diff(i) < 25 });
            if crowded {
                continue;
            }
            let mut s = sign_at(road, i, w.dir, SIGN_OFF, w.kind, ground);
            if net.nearest(xz(s.pos), 12.0).is_some_and(|np| np.road != w.road && np.at.lateral.abs() < net.roads[np.road].road.half_width(&np.at) + 1.0) {
                continue;
            }
            let twins = placed.iter().filter(|&&(r, d, j, reg)| r == w.road && d == w.dir && j == i && reg).count();
            if twins > 0 {
                // Further along the verge, away from the cars coming.
                let on = road.dir[i] * w.dir as f32 * 1.4 * twins as f32;
                s.pos += Vec3::new(on.x, 0.0, on.y);
                s.pos.y = ground(xz(s.pos));
            }
            out.push(s);
            placed.push((w.road, w.dir, i, regulatory));
            break;
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::OnceLock;

    use super::*;
    use crate::open::OpenWorld;

    /// The committed level, loaded once for these tests.
    pub(crate) fn world() -> &'static OpenWorld {
        static W: OnceLock<OpenWorld> = OnceLock::new();
        W.get_or_init(|| {
            let path = crate::open::level::source();
            let l = crate::open::level::load(&path).expect("the committed level loads");
            OpenWorld::from_data_with(l.data, Some(l.heights)).expect("valid")
        })
    }

    #[test]
    fn the_same_furniture_twice() {
        let w = world();
        let again = build(&w.net, &w.heights);
        assert_eq!(*w.furniture, again);
        assert!(!again.rails.is_empty() && !again.signs.is_empty() && !again.patches.is_empty());
        // Sorted, and the grid finds every patch from its own centre.
        assert!(again.patches.windows(2).all(|p| p[0].a.x <= p[1].a.x));
        for (k, p) in again.patches.iter().enumerate() {
            let c = p.centre();
            assert!(again.grid.at(c.x, c.y).iter().any(|e| e.1 as usize == k), "patch {k} not indexed");
        }
    }

    /// A car driven into a guard rail at speed scrapes along it and stays on the road side.
    #[test]
    fn a_rail_holds_a_car_at_speed() {
        use crate::vehicle::{Car, Input, Spec, step};
        let w = world();
        let f = &*w.furniture;
        let rail = f.rails.iter().filter(|r| r.kind == RailKind::Steel).max_by(|a, b| (a.to - a.from).total_cmp(&(b.to - b.from))).expect("a steel rail");
        let road = &w.net.roads[rail.road as usize].road;
        let side = rail.side as f32;
        let i = (((rail.from + rail.to) * 0.5) / STEP) as usize;
        let spec = Spec::default();
        for (kmh, deg) in [(60.0f32, 20.0f32), (100.0, 12.0), (90.0, 30.0)] {
            let d = road.dir[i];
            let towards = left(road, i) * side;
            let a = deg.to_radians();
            let v = d * libm::cosf(a) + towards * libm::sinf(a);
            // From the far lane, 1 m off the centre.
            let start = xz(road.pts[i]) - towards * 1.0;
            let heading = libm::atan2f(v.x, v.y);
            let mut car = Car::new(Vec3::new(start.x, w.ground_height(start.x, start.y) + 0.6, start.y), heading);
            // Settle on the springs first.
            for _ in 0..90 {
                step(&mut car, &spec, Input { handbrake: true, ..Default::default() }, w, 1.0 / 60.0);
            }
            car.vel = car.forward() * kmh / 3.6;
            for wh in &mut car.wheels {
                wh.spin = kmh / 3.6 / spec.wheel_radius;
            }
            let mut worst = f32::MIN;
            for _ in 0..180 {
                step(&mut car, &spec, Input { throttle: 0.3, ..Default::default() }, w, 1.0 / 60.0);
                let p = Vec2::new(car.pos.x, car.pos.z);
                // How far past the rail line the car's centre got (− = still on the road side).
                if let Some(np) = road.nearest(p, 30.0) {
                    let rail_lat = road.half_width(&np) + RAIL_OFF;
                    worst = worst.max(np.lateral * side - rail_lat);
                }
            }
            eprintln!("{kmh} km/h at {deg}°: impacts {:.2}, centre {worst:.2} m from the rail line, {:.0} km/h after", car.impacts, car.vel.length() * 3.6);
            assert!(car.impacts > 0.0, "{kmh} km/h at {deg}°: never hit the rail");
            // Its centre stays more than half a car width inside the rail.
            assert!(worst < -0.6, "{kmh} km/h at {deg}°: through the rail ({worst:.2} m)");
        }
    }

    #[test]
    fn potholes_dip_the_ground_and_keep_the_grip() {
        let w = world();
        let f = &*w.furniture;
        let holes: Vec<&Patch> = f.patches.iter().filter(|p| matches!(p.kind, PatchKind::Pothole { .. })).collect();
        assert!(holes.len() >= 5, "{} potholes", holes.len());
        for p in holes {
            let PatchKind::Pothole { depth } = p.kind else { unreachable!() };
            let c = p.a;
            let hf = w.heights.height_at(c.x, c.y);
            let without = super::super::profile::ground(&w.net, c, hf).unwrap_or(hf);
            assert!((without - w.ground_height(c.x, c.y) - depth).abs() < 1e-3 || dip(f, c.x, c.y) >= depth, "pothole at {c} doesn't dip {depth}");
            assert!((0.04..=0.08).contains(&depth));
            // Grip unchanged: the road's own surface.
            assert_eq!(w.surface_at(c.x, c.y), super::super::roads::surface(&w.net, c.x, c.y));
            // Nothing past its edge.
            let out = c + Vec2::X * (p.radius + 0.05);
            assert!(f.patches.iter().any(|q| q != p && q.local(out).1 < q.radius) || dip(f, out.x, out.y) == 0.0);
        }
    }

    /// The surface at the middle of the first patch of `kind` (a synthetic one on a copy of
    /// the world when the level has none).
    fn surface_on(kind: PatchKind) -> (Surface, f32, f32) {
        let w = world();
        let p = w.furniture.patches.iter().find(|p| p.kind == kind).copied();
        let (o, p) = match p {
            Some(p) => (w.clone(), p),
            None => {
                let road = &w.net.roads[0].road;
                let (a, b) = (xz(road.pts[200]), xz(road.pts[202]));
                let p = Patch { a, b, radius: 1.5, kind, s0: 400.0 };
                let mut o = w.clone();
                o.furniture = std::sync::Arc::new(Furniture::new(vec![], vec![], vec![], vec![p]));
                (o, p)
            }
        };
        let c = p.centre();
        // Dips along the patch's axis.
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for k in 0..=40 {
            let q = p.a + (p.b - p.a) * (k as f32 / 40.0);
            let d = dip(&o.furniture, q.x, q.y);
            lo = lo.min(d);
            hi = hi.max(d);
        }
        (o.surface_at(c.x, c.y), lo, hi)
    }

    #[test]
    fn wet_patches_are_wet() {
        let (s, _, hi) = surface_on(PatchKind::Wet);
        assert_eq!(s, Surface::Wet);
        assert_eq!(hi, 0.0, "wet tarmac doesn't dip");
    }

    #[test]
    fn mud_patches_are_mud() {
        assert_eq!(surface_on(PatchKind::Mud).0, Surface::Mud);
    }

    #[test]
    fn washouts_are_loose_and_corrugated() {
        let (s, lo, hi) = surface_on(PatchKind::Washout);
        assert_eq!(s, Surface::Loose);
        // ±2 cm about a 2 cm dip, at 0.9 m: sampled every 0.1 m over 4 m, it spans most of it.
        assert!(hi > 0.03 && hi <= 0.0401 && lo < 0.01, "corrugation {lo}..{hi}");
    }

    #[test]
    fn shaded_ice_is_snow() {
        assert_eq!(surface_on(PatchKind::Ice).0, Surface::Snow);
    }

    /// Where a sign stands along road `r` (m), if it's by that road.
    fn station_of(w: &OpenWorld, r: usize, s: &Sign) -> Option<(f32, f32)> {
        w.net.roads[r].road.nearest(xz(s.pos), 8.0).map(|at| (at.s, at.lateral))
    }

    #[test]
    fn every_hairpin_has_chevrons_and_a_warning() {
        let w = world();
        let f = &*w.furniture;
        let town = town(&w.net);
        let (mut hairpins, mut warned) = (0, 0);
        for r in 0..w.net.roads.len() {
            let info = road_info(&w.net, r, town);
            let road = &w.net.roads[r].road;
            let n = road.len();
            let tarmac = info.tarmac;
            let warn_m = if tarmac { 120.0 } else { 70.0 };
            let hairpin_signs: Vec<(f32, f32)> = f.signs.iter().filter(|s| matches!(s.kind, SignKind::Warning(warn::HAIRPIN_L | warn::HAIRPIN_R))).filter_map(|s| station_of(w, r, s)).collect();
            let starts: Vec<usize> = info.corners.iter().enumerate().filter(|(_, c)| c.grade == stage::HAIRPIN).map(|(k, _)| k).collect();
            for &k in &starts {
                let (ap, _) = info.apex[k];
                if ap < 20 || ap + 20 >= n || info.near_j[ap] {
                    continue;
                }
                hairpins += 1;
                let apex = at(road, ap, 0.0);
                assert!(f.signs.iter().any(|s| matches!(s.kind, SignKind::Chevron(_)) && xz(s.pos).distance(apex) < road.width[ap] + 12.0), "road {r} hairpin at {:.0} m: no chevrons", ap as f32 * STEP);
                let c = &info.corners[k];
                for dir in [1i32, -1] {
                    let entry = if dir > 0 { c.start } else { c.end };
                    // The road must reach far enough back for a sign.
                    let back = entry - dir as f32 * warn_m;
                    if back < 10.0 || back > road.length() - 10.0 {
                        continue;
                    }
                    // A hairpin sign before it on the right, or one before the hairpin just
                    // ahead of it (a ladder), within 170 m.
                    let ok = hairpin_signs.iter().any(|&(s, lat)| {
                        let before = (entry - s) * dir as f32;
                        (30.0..=warn_m + 170.0).contains(&before) && lat * dir as f32 <= 0.0
                    });
                    warned += ok as usize;
                    assert!(ok, "road {r} hairpin at {:.0} m going {dir}: no warning sign", ap as f32 * STEP);
                }
            }
        }
        eprintln!("{hairpins} hairpins, {warned} warned approaches");
        assert!(hairpins > 10);
    }

    #[test]
    fn reflector_posts_every_50_m_on_rural_tarmac() {
        let w = world();
        let f = &*w.furniture;
        let town = town(&w.net);
        let mut checked = 0.0;
        for (r, nr) in w.net.roads.iter().enumerate() {
            if nr.kind != RoadKind::BRoad {
                continue;
            }
            let info = road_info(&w.net, r, town);
            let road = &nr.road;
            for side in [1.0f32, -1.0] {
                let mut at_s: Vec<f32> = f.signs.iter().filter(|s| s.kind == SignKind::Reflector).filter_map(|s| station_of(w, r, s)).filter(|&(_, lat)| lat * side > 0.0).map(|(s, _)| s).collect();
                at_s.sort_by(f32::total_cmp);
                for pair in at_s.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    if b - a <= 55.0 {
                        checked += b - a;
                        continue;
                    }
                    // A longer gap must have a reason: a junction, a bridge, the town or a rail.
                    let (i0, i1) = (road.station(a), road.station(b));
                    let excused = (i0..=i1).any(|i| info.near_j[i] || info.bridge[i] || info.in_town[i] || f.rails.iter().any(|x| x.road as usize == r && x.side as f32 == side && (x.from - 6.0..=x.to + 6.0).contains(&(i as f32 * STEP))));
                    assert!(excused, "road {r} side {side}: no reflector post from {a:.0} to {b:.0} m");
                }
            }
        }
        assert!(checked > 4000.0, "only {checked} m checked");
    }

    #[test]
    fn speed_signs_say_what_the_traffic_drives() {
        let w = world();
        let f = &*w.furniture;
        let mut n = 0;
        for s in &f.signs {
            let SignKind::Speed(v) = s.kind else { continue };
            n += 1;
            let np = w.net.nearest(xz(s.pos), 15.0).expect("by a road");
            // The limit where it stands, or a few metres on (it marks a change).
            let ok = (-10..=10).any(|k| {
                let i = (np.at.i as i64 + k).clamp(0, w.net.roads[np.road].road.len() as i64 - 1) as usize;
                posted_kmh(&w.net, np.road, i) == v
            });
            assert!(ok, "{v} sign by road {} at {:.0} m: posted {}", np.road, np.at.s, posted_kmh(&w.net, np.road, np.at.i));
        }
        assert!(n > 0, "no speed signs");
        assert!(f.signs.iter().any(|s| s.kind == SignKind::TownBoard) && f.signs.iter().any(|s| s.kind == SignKind::TownEnd));
    }

    #[test]
    fn nothing_stands_on_a_road() {
        let w = world();
        let f = &*w.furniture;
        let on_road = |p: Vec2| w.net.nearest(p, 30.0).is_some_and(|n| n.at.lateral.abs() < w.net.roads[n.road].road.half_width(&n.at) - 0.05);
        for s in &f.signs {
            // Street lights' arms reach over; their poles don't.
            assert!(!on_road(xz(s.pos)), "{:?} on a road at {}", s.kind, s.pos);
        }
        for r in &f.rails {
            for p in &r.pts {
                assert!(!on_road(xz(*p)), "rail on a road at {p}");
            }
        }
    }

    #[test]
    fn report() {
        let f = &*world().furniture;
        let count = |pred: &dyn Fn(&SignKind) -> bool| f.signs.iter().filter(|s| pred(&s.kind)).count();
        let pc = |pred: &dyn Fn(&PatchKind) -> bool| f.patches.iter().filter(|p| pred(&p.kind)).count();
        eprintln!("blocks {}", f.blocks.len());
        eprintln!(
            "rails {:.0} m in {} runs (steel {:.0} m); warnings {}, speed {}, town {}, km {}, reflectors {}, lights {}, chevrons {}",
            f.rail_length(),
            f.rails.len(),
            f.rails.iter().filter(|r| r.kind == RailKind::Steel).map(|r| r.to - r.from).sum::<f32>(),
            count(&|k| matches!(k, SignKind::Warning(_))),
            count(&|k| matches!(k, SignKind::Speed(_))),
            count(&|k| matches!(k, SignKind::TownBoard | SignKind::TownEnd)),
            count(&|k| matches!(k, SignKind::KmPost(_))),
            count(&|k| matches!(k, SignKind::Reflector)),
            count(&|k| matches!(k, SignKind::StreetLight)),
            count(&|k| matches!(k, SignKind::Chevron(_))),
        );
        for w in 0..warn::COUNT {
            eprint!("w{w}:{} ", count(&|k| *k == SignKind::Warning(w)));
        }
        eprintln!();
        eprintln!(
            "patches: potholes {}, repairs {}, wet {}, mud {}, washout {}, ice {}",
            pc(&|k| matches!(k, PatchKind::Pothole { .. })),
            pc(&|k| matches!(k, PatchKind::Repair)),
            pc(&|k| matches!(k, PatchKind::Wet)),
            pc(&|k| matches!(k, PatchKind::Mud)),
            pc(&|k| matches!(k, PatchKind::Washout)),
            pc(&|k| matches!(k, PatchKind::Ice)),
        );
        for r in &f.rails {
            eprintln!("  rail road {} side {} {:.0}-{:.0} {:?}", r.road, r.side, r.from, r.to, r.kind);
        }
    }
}

#[cfg(test)]
mod measure {
    use super::*;

    #[test]
    #[ignore]
    fn rail_rule_shares() {
        let w = tests::world();
        let town = town(&w.net);
        for (r, nr) in w.net.roads.iter().enumerate() {
            let info = road_info(&w.net, r, town);
            let road = &nr.road;
            let mut c = [0usize; 6];
            for side in [1.0f32, -1.0] {
                for i in 0..road.len() {
                    if info.near_j[i] || info.bridge[i] {
                        continue;
                    }
                    let d8 = drop_at(road, &w.heights, i, side);
                    let half = road.width[i] * 0.5;
                    let far = at(road, i, side * (half + 6.0));
                    let d6 = edge_y(road, i, side * half) - w.heights.height_at(far.x, far.y);
                    let corner = info.corner_at[i].filter(|&k| info.apex[k].1 == side).map(|k| info.corners[k].grade);
                    if d6 > 3.0 { c[0] += 1 }
                    if d8 > 3.0 { c[1] += 1 }
                    if d8 > 6.0 { c[2] += 1 }
                    if d8 > 3.0 && corner.is_some_and(|g| g <= 4) { c[3] += 1 }
                    if d8 > 3.0 && corner.is_some_and(|g| g > 4) && road.pts[i].y > 250.0 { c[4] += 1 }
                    if d6 > 6.0 { c[5] += 1 }
                }
            }
            let m = |k: usize| c[k] as f32 * STEP;
            let _ = 0;
            eprintln!("road {r} {:?} len {:.0}: D6>3 {:.0}  D8>3 {:.0}  D8>6 {:.0}  out<=4 {:.0}  out5/6 high {:.0}  D6>6 {:.0}", nr.kind, road.length(), m(0), m(1), m(2), m(3), m(4), m(5));
        }
    }

    #[test]
    #[ignore]
    fn rail_variants() {
        let w = tests::world();
        let town = town(&w.net);
        let infos: Vec<RoadInfo> = (0..w.net.roads.len()).map(|r| road_info(&w.net, r, town)).collect();
        let ground = |p: Vec2| w.heights.height_at(p.x, p.y);
        for (at, drop, any, loose_grade, grow) in [(8.0, 3.0, 6.0, 2, 20.0), (8.0, 3.0, 6.0, 1, 20.0), (8.0, 3.0, 8.0, 2, 20.0), (8.0, 3.0, 8.0, 0, 20.0), (6.0, 3.0, 6.0, 2, 20.0), (8.0, 3.0, 10.0, 0, 20.0)] {
            let rule = RailRule { at, drop, any, grade: 4, loose_grade, grow, gap: 30.0, min: 24.0 };
            let rails = rails_with(&w.net, &w.heights, &infos, &ground, &rule);
            let len: f32 = rails.iter().map(|r| r.to - r.from).sum();
            let steel: f32 = rails.iter().filter(|r| r.kind == RailKind::Steel).map(|r| r.to - r.from).sum();
            eprintln!("{rule:?}: {len:.0} m ({steel:.0} steel) in {} runs", rails.len());
        }
    }

    #[test]
    #[ignore]
    fn condition_candidates() {
        let w = tests::world();
        let town = town(&w.net);
        for (r, nr) in w.net.roads.iter().enumerate() {
            let info = road_info(&w.net, r, town);
            let road = &nr.road;
            let n = road.len();
            let mut lows = Vec::new();
            for i in 20..n.saturating_sub(20) {
                let y = road.pts[i].y;
                if (i - 20..=i + 20).all(|j| j == i || road.pts[j].y > y) {
                    lows.push(format!("{:.2}", w.net.land.wet(xz(road.pts[i]))));
                }
            }
            eprintln!("road {r} {:?}: lows wet {:?}", nr.kind, lows);
            for (k, c) in info.corners.iter().enumerate() {
                if c.grade != stage::HAIRPIN { continue; }
                let (ap, out_side) = info.apex[k];
                let inside = -out_side;
                let cut = at(road, ap, inside * (road.width[ap] * 0.5 + DROP_AT));
                let nz = w.heights.smooth_normal_at(cut.x, cut.y).z;
                let cut_h = -drop_at(road, &w.heights, ap, inside);
                eprintln!("   hairpin at {:.0} y {:.0} cut {:.1} nz {:.2}", ap as f32 * STEP, road.pts[ap].y, cut_h, nz);
            }
        }
    }

    /// Camera spots for screenshots: `eye -> at` for `art.overhead`, and car poses.
    #[test]
    #[ignore]
    fn spots() {
        let w = tests::world();
        let f = &*w.furniture;
        let net = &w.net;
        let pose = |what: &str, r: usize, i: usize, back: f32, up: f32, side: f32| {
            let road = &net.roads[r].road;
            let i = i.min(road.len() - 1);
            let d = road.dir[i];
            let p = road.pts[i];
            let j = ((i as f32) - back / STEP).max(0.0) as usize;
            let q = road.pts[j];
            let l = -road.dir[j].perp();
            let eye = Vec3::new(q.x + l.x * side, q.y + up, q.z + l.y * side);
            let heading = libm::atan2f(d.x, d.y).to_degrees();
            eprintln!("{what}: road {r} s {:.0}: at [{:.1}, {:.1}, {:.1}] eye [{:.1}, {:.1}, {:.1}] heading {heading:.0} car@ x {:.1} z {:.1}", i as f32 * STEP, p.x, p.y, p.z, eye.x, eye.y, eye.z, q.x, q.z);
        };
        for rail in f.rails.iter().filter(|r| r.kind == RailKind::Steel).take(4) {
            let mid = ((rail.from + rail.to) * 0.5 / STEP) as usize;
            pose("steel rail", rail.road as usize, mid, 60.0, 4.0, 0.0);
        }
        for rail in f.rails.iter().filter(|r| r.kind == RailKind::Timber).take(2) {
            let mid = ((rail.from + rail.to) * 0.5 / STEP) as usize;
            pose("timber rail", rail.road as usize, mid, 50.0, 3.0, 0.0);
        }
        for s in f.signs.iter().filter(|s| matches!(s.kind, SignKind::TownBoard | SignKind::Speed(_))).take(6) {
            let np = net.nearest(xz(s.pos), 20.0).unwrap();
            eprintln!("{:?} at {:.1} {:.1} {:.1} yaw {:.0} (road {} s {:.0})", s.kind, s.pos.x, s.pos.y, s.pos.z, s.yaw.to_degrees(), np.road, np.at.s);
        }
        for kind in ["wet", "pothole", "washout", "mud", "ice"] {
            if let Some(p) = f.patches.iter().find(|p| match kind { "wet" => p.kind == PatchKind::Wet, "pothole" => matches!(p.kind, PatchKind::Pothole { .. }), "washout" => p.kind == PatchKind::Washout, "mud" => p.kind == PatchKind::Mud, _ => p.kind == PatchKind::Ice }) {
                let np = net.nearest(p.centre(), 20.0).unwrap();
                pose(kind, np.road, np.at.i, 25.0, 2.5, 0.0);
            }
        }
    }

    /// `art.overhead` views (heights relative to the ground at `at`) for screenshots.
    #[test]
    #[ignore]
    fn views() {
        let w = tests::world();
        let f = &*w.furniture;
        let net = &w.net;
        let view = |name: &str, eye: Vec3, at: Vec3| {
            let g = w.ground_height(at.x, at.z);
            println!("{{\"cmd\": \"art.overhead\", \"params\": {{\"eye\": [{:.1}, {:.2}, {:.1}], \"at\": [{:.1}, {:.2}, {:.1}]}}}}, {{\"sleep\": 2.5}}, {{\"shot\": \"{name}\"}},", eye.x, eye.y - g, eye.z, at.x, at.y - g, at.z);
        };
        let road_pt = |r: usize, i: usize, lat: f32, up: f32| {
            let road = &net.roads[r].road;
            let i = i.min(road.len() - 1);
            let p = at(road, i, lat);
            Vec3::new(p.x, road.pts[i].y + up, p.y)
        };
        // Hairpins with steel rails: from 50 m before, driver height, looking at the apex.
        let town = town(net);
        let mut k = 0;
        for rail in f.rails.iter().filter(|r| r.kind == RailKind::Steel) {
            let r = rail.road as usize;
            let info = road_info(net, r, town);
            for (c, &(ap, _)) in info.corners.iter().zip(&info.apex) {
                let s = ap as f32 * STEP;
                if c.grade == stage::HAIRPIN && s > rail.from && s < rail.to && k < 3 {
                    k += 1;
                    view(&format!("hairpin{k}"), road_pt(r, ap.saturating_sub(16), -1.0, 1.6), road_pt(r, ap, 0.0, 0.8));
                    view(&format!("hairpin{k}_high"), road_pt(r, ap.saturating_sub(45), 0.0, 9.0), road_pt(r, ap, 0.0, 0.0));
                }
            }
        }
        for rail in f.rails.iter().filter(|r| r.kind == RailKind::Timber).take(1) {
            let i = (rail.from / STEP) as usize + 10;
            view("timber", road_pt(rail.road as usize, i.saturating_sub(20), 0.0, 1.6), road_pt(rail.road as usize, i + 5, rail.side as f32 * 4.0, 0.6));
        }
        for (name, kind) in [("town_board", SignKind::TownBoard), ("town_end", SignKind::TownEnd)] {
            if let Some(s) = f.signs.iter().find(|s| s.kind == kind) {
                let n = Vec3::new(libm::sinf(s.yaw), 0.0, libm::cosf(s.yaw));
                view(name, s.pos + n * 14.0 + Vec3::Y * 1.8 + n.cross(Vec3::Y) * 2.0, s.pos + Vec3::Y * 1.6);
            }
        }
        for (name, want) in [("wet", 0), ("pothole", 1), ("washout", 2), ("mud", 3)] {
            let p = f.patches.iter().find(|p| match want { 0 => p.kind == PatchKind::Wet, 1 => matches!(p.kind, PatchKind::Pothole { .. }), 2 => p.kind == PatchKind::Washout, _ => p.kind == PatchKind::Mud });
            if let Some(p) = p {
                let np = net.nearest(p.centre(), 20.0).unwrap();
                let back = if want == 1 { 4 } else { 10 };
                view(name, road_pt(np.road, np.at.i.saturating_sub(back), np.at.lateral * 0.3, 1.5), Vec3::new(p.centre().x, net.roads[np.road].road.height_at(&np.at), p.centre().y));
            }
        }
        if let Some(s) = f.signs.iter().find(|s| matches!(s.kind, SignKind::Warning(_))) {
            let n = Vec3::new(libm::sinf(s.yaw), 0.0, libm::cosf(s.yaw));
            view("warning", s.pos + n * 10.0 + Vec3::Y * 1.8 + n.cross(Vec3::Y) * 1.5, s.pos + Vec3::Y * 2.0);
        }
    }

    /// What the hooks cost: `dip` + `surface_override` at points along the roads.
    #[test]
    #[ignore]
    fn hook_cost() {
        let w = tests::world();
        let f = &*w.furniture;
        let pts: Vec<Vec2> = w.net.roads.iter().flat_map(|r| r.road.pts.iter().map(|p| xz(*p))).collect();
        let t = std::time::Instant::now();
        let mut acc = 0.0;
        for _ in 0..10 {
            for p in &pts {
                acc += dip(f, p.x, p.y);
                acc += surface_override(f, p.x, p.y).is_some() as u8 as f32;
            }
        }
        let n = pts.len() * 10;
        eprintln!("{n} calls: {:.1} ns each ({acc})", t.elapsed().as_secs_f64() * 1e9 / n as f64);
    }
}
