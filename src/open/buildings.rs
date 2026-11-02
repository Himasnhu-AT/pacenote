//! Town buildings: lots laid out in the street grid's blocks (rows of shops and houses, apartment
//! slabs, a tower, a plaza, parks and car parks) and a few farms along the country roads near
//! town. Three parts:
//!
//! - [`layout`]: pure and deterministic from the road network (its seed), like `stage.rs`: the
//!   same bits on every machine (no platform trig, no hash-map order).
//! - [`obstacles`] / [`ObstacleGrid`]: the walls as rows of solid cylinders for the car sim
//!   (`vehicle::Obstacle` is a vertical cylinder), overlapping so nothing slips between them.
//! - [`spawn`]: one merged mesh per block (walls, roofs, sidewalks, lot paving) with a single
//!   material: a procedural facade atlas (windows, shopfronts, doors, planks, roof tiles,
//!   paving) tinted by vertex colour. Nothing spawns in headless runs.

use bevy::log::warn;
use bevy::math::{Vec2, Vec3};

use super::roads::{self, BLOCK, PLATEAU, RING_R, RoadKind, RoadNet, TOWN};
use crate::stage::{Rng, STEP};
use crate::vehicle::Obstacle;

/// Sidewalk between the street's edge and the lots (m).
pub const SIDEWALK: f32 = 3.0;
/// Storey height (m) of town buildings; houses use [`HOUSE_FLOOR`].
pub const FLOOR: f32 = 3.2;
pub const HOUSE_FLOOR: f32 = 2.9;
/// Parapet on flat roofs (height above the roof, thickness).
pub const PARAPET: (f32, f32) = (0.9, 0.25);
/// Wall collision: cylinders of this radius, centres this far inside the wall face, at most
/// this far apart. A wall face then bulges out by at most ~0.15 m between centres and is
/// recessed by at most ~0.15 m: the car stops within a hand's width of the plaster.
pub const WALL_R: f32 = 1.2;
pub const WALL_INSET: f32 = 1.05;
pub const WALL_STEP: f32 = 1.6;

/// What a building is (sets its size, storeys, roof and facade).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, bevy::reflect::Reflect)]
pub enum Use {
    /// Downtown: offices over shops, 5–8 storeys, joined in rows.
    Downtown,
    /// Shops with flats over them, 2–4 storeys, rows with the odd alley.
    Shop,
    /// Detached houses, 2 storeys, front gardens, pitched roofs.
    House,
    /// Apartment slabs, 3–6 storeys.
    Apartment,
    /// The one tower by the central plaza.
    Tower,
    /// Farmhouse and barn along a country road.
    FarmHouse,
    Barn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Roof {
    /// Flat with a parapet.
    Flat,
    /// Two slopes; the ridge runs parallel to the front.
    Gable,
    /// Four slopes; the ridge runs along the longer side.
    Hip,
}

/// One building: an oriented box footprint, storeys and a roof.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Building {
    pub centre: Vec2,
    /// Unit direction (world XZ) of the footprint's local x; local z is `axis.perp()`.
    pub axis: Vec2,
    /// Half extents along local x and local z.
    pub half: Vec2,
    /// The side facing the street: 0 = +x, 1 = +z, 2 = −x, 3 = −z (local).
    pub front: u8,
    pub floors: u32,
    pub floor_h: f32,
    /// Floor level (y) and the lowest ground under the footprint (the plinth reaches down to it).
    pub ground: f32,
    pub base: f32,
    pub roof: Roof,
    /// Roof slope (rise per metre) for pitched roofs.
    pub pitch: f32,
    pub use_: Use,
    /// Facade and roof colour indices ([`WALLS`], [`ROOFS`]) and spare random bits.
    pub wall: u8,
    pub roof_col: u8,
    pub bits: u32,
}

impl Building {
    /// Top of the walls (y).
    pub fn eave(&self) -> f32 {
        self.ground + self.floors as f32 * self.floor_h
    }

    /// The ridge direction (local unit vector in world XZ) and half extents (along, across).
    fn ridge(&self) -> (Vec2, f32, f32) {
        let along_x = match self.roof {
            Roof::Gable => self.front % 2 == 1,
            _ => self.half.x >= self.half.y,
        };
        if along_x { (self.axis, self.half.x, self.half.y) } else { (self.axis.perp(), self.half.y, self.half.x) }
    }

    /// Highest point (y): ridge, parapet or rooftop box.
    pub fn top(&self) -> f32 {
        match self.roof {
            Roof::Flat => self.eave() + PARAPET.0.max(if self.has_roof_box() { 2.8 } else { 0.0 }),
            Roof::Gable | Roof::Hip => self.eave() + self.ridge().2 * self.pitch,
        }
    }

    fn has_roof_box(&self) -> bool {
        self.roof == Roof::Flat && self.floors >= 4 && self.bits & 1 == 1 && self.half.x > 4.0 && self.half.y > 4.0
    }

    /// World position of local (x, z).
    pub fn at(&self, l: Vec2) -> Vec2 {
        self.centre + self.axis * l.x + self.axis.perp() * l.y
    }

    /// Footprint corners, counter-clockwise in (x, z).
    pub fn corners(&self) -> [Vec2; 4] {
        let h = self.half;
        [self.at(Vec2::new(-h.x, -h.y)), self.at(Vec2::new(h.x, -h.y)), self.at(Vec2::new(h.x, h.y)), self.at(Vec2::new(-h.x, h.y))]
    }

    /// Is `p` inside the footprint grown by `margin`?
    pub fn contains(&self, p: Vec2, margin: f32) -> bool {
        let d = p - self.centre;
        d.dot(self.axis).abs() <= self.half.x + margin && d.dot(self.axis.perp()).abs() <= self.half.y + margin
    }
}

/// What a lot holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, bevy::reflect::Reflect)]
pub enum LotKind {
    /// Buildings in rows along the block's edges.
    Rows(Use),
    /// Two apartment slabs.
    Apartments,
    /// One tower on a paved square.
    Tower,
    /// Paved, open.
    Plaza,
    /// Grass, no buildings (trees and benches are for the dressing).
    Park,
    /// Asphalt with parking bays.
    Parking,
    /// A farmhouse and maybe a barn by a country road (not a town block).
    Farm,
}

/// A town block (or a farm): its lot and its buildings.
#[derive(Clone, Debug)]
pub struct Block {
    /// Grid cell (0..6, 0..6) of a town block; farms have none.
    pub cell: Option<(u32, u32)>,
    /// The lot inside the sidewalks (town); the farm's buildings' bounds (farms).
    pub min: Vec2,
    pub max: Vec2,
    pub kind: LotKind,
    /// Indices into [`Layout::buildings`].
    pub buildings: Vec<usize>,
}

impl Block {
    pub fn centre(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }
}

/// Every building of the world.
#[derive(Clone, Debug)]
pub struct Layout {
    pub seed: u64,
    pub town_y: f32,
    pub blocks: Vec<Block>,
    pub buildings: Vec<Building>,
}

impl Layout {
    /// Is `p` inside a building (grown by `margin`)? For keeping trees and props out.
    pub fn occupied(&self, p: Vec2, margin: f32) -> bool {
        self.buildings.iter().any(|b| b.contains(p, margin))
    }

    /// The town block lot containing `p`, if any (for dressing parks and keeping plazas clear).
    pub fn lot_at(&self, p: Vec2) -> Option<&Block> {
        self.blocks.iter().find(|b| b.cell.is_some() && p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y)
    }
}

/// Wall colours (sRGB): cream, ochre, brick, grey, pale blue, white, sage, pink, concrete, sand.
pub const WALLS: [[f32; 3]; 10] = [
    [0.93, 0.88, 0.76],
    [0.88, 0.72, 0.48],
    [0.70, 0.42, 0.33],
    [0.78, 0.78, 0.76],
    [0.72, 0.80, 0.86],
    [0.97, 0.96, 0.93],
    [0.74, 0.80, 0.68],
    [0.92, 0.76, 0.72],
    [0.58, 0.58, 0.57],
    [0.86, 0.80, 0.68],
];
/// Roof colours (sRGB): terracotta, slate, brown, light grey (flat), barn red.
pub const ROOFS: [[f32; 3]; 5] = [[0.72, 0.36, 0.25], [0.36, 0.38, 0.42], [0.45, 0.32, 0.24], [0.80, 0.80, 0.80], [0.55, 0.20, 0.16]];

fn pick<T: Copy>(rng: &mut Rng, xs: &[T]) -> T {
    xs[((rng.f() * xs.len() as f32) as usize).min(xs.len() - 1)]
}

fn shuffle<T>(rng: &mut Rng, xs: &mut [T]) {
    for i in (1..xs.len()).rev() {
        let j = ((rng.f() * (i + 1) as f32) as usize).min(i);
        xs.swap(i, j);
    }
}

fn bits(rng: &mut Rng) -> u32 {
    (rng.f() * 16_777_216.0) as u32
}

/// How a row of buildings along a block edge is made.
struct RowStyle {
    use_: Use,
    setback: (f32, f32),
    depth: (f32, f32),
    width: (f32, f32),
    /// Chance of a gap after a building, and its size.
    gap: (f32, f32, f32),
    /// The last building stretches to the row's end.
    fill: bool,
    /// Space between the side rows and the end rows at the corners.
    corner: f32,
}

fn row_style(u: Use) -> RowStyle {
    match u {
        Use::Downtown => RowStyle { use_: u, setback: (0.0, 0.0), depth: (16.0, 22.0), width: (12.0, 24.0), gap: (0.15, 2.0, 3.0), fill: true, corner: 0.0 },
        Use::Shop => RowStyle { use_: u, setback: (0.0, 0.0), depth: (11.0, 15.0), width: (7.0, 14.0), gap: (0.35, 1.5, 4.0), fill: true, corner: 1.5 },
        _ => RowStyle { use_: Use::House, setback: (3.0, 6.0), depth: (8.0, 11.0), width: (8.0, 12.0), gap: (1.0, 3.0, 7.0), fill: false, corner: 3.0 },
    }
}

/// Storeys and roof for a building of `use_` in a block whose centre is `ring` (0 = downtown,
/// 1 = middle, 2 = outer) from the centre.
fn dress(rng: &mut Rng, use_: Use, ring: u32) -> (u32, f32, Roof, u8, u8) {
    let walls_town: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7, 9];
    match use_ {
        Use::Downtown => (rng.range(5.0, 8.99) as u32, FLOOR, Roof::Flat, pick(rng, &[3, 8, 9, 0, 2, 5, 4]), 3),
        Use::Tower => (8, FLOOR, Roof::Flat, pick(rng, &[8, 3, 4]), 3),
        Use::Shop => {
            let lo = if ring == 1 { 3.0 } else { 2.0 };
            let roof = if rng.chance(0.25) { Roof::Gable } else { Roof::Flat };
            (rng.range(lo, lo + 1.99) as u32, FLOOR, roof, pick(rng, walls_town), if roof == Roof::Flat { 3 } else { pick(rng, &[0, 1, 2]) })
        }
        Use::Apartment => {
            let lo = if ring == 1 { 4.0 } else { 3.0 };
            (rng.range(lo, lo + 2.99) as u32, FLOOR, Roof::Flat, pick(rng, &[0, 3, 5, 9, 4, 8]), 3)
        }
        Use::House | Use::FarmHouse => {
            let roof = if rng.chance(0.55) { Roof::Gable } else { Roof::Hip };
            (2, HOUSE_FLOOR, roof, pick(rng, &[0, 1, 2, 4, 5, 6, 7, 9]), pick(rng, &[0, 0, 1, 2]))
        }
        Use::Barn => (1, 5.2, Roof::Gable, pick(rng, &[2, 2, 8]), pick(rng, &[1, 4, 3])),
    }
}

#[allow(clippy::too_many_arguments)]
fn building(rng: &mut Rng, use_: Use, ring: u32, centre: Vec2, axis: Vec2, half: Vec2, front: u8, ground: f32, base: f32) -> Building {
    let (floors, floor_h, roof, wall, roof_col) = dress(rng, use_, ring);
    let pitch = match use_ {
        Use::Barn => rng.range(0.55, 0.75),
        _ => rng.range(0.6, 0.85),
    };
    Building { centre, axis, half, front, floors, floor_h, ground, base, roof, pitch, use_, wall, roof_col, bits: bits(rng) }
}

/// A row along one edge of the lot `(lo, hi)`. `side`: 0 = the −z edge, 1 = +z, 2 = −x, 3 = +x.
/// Along x (sides 0, 1) the row spans `a0..a1` in x; along z (sides 2, 3) in z.
#[allow(clippy::too_many_arguments)]
fn row(out: &mut Vec<Building>, rng: &mut Rng, st: &RowStyle, ring: u32, lo: Vec2, hi: Vec2, side: u8, a0: f32, a1: f32, y: f32) {
    let mut a = a0;
    loop {
        let left = a1 - a;
        if left < st.width.0 {
            break;
        }
        let mut w = rng.range(st.width.0, st.width.1).min(left);
        if st.fill && left - w < st.width.0 {
            w = left;
        }
        let depth = rng.range(st.depth.0, st.depth.1);
        let set = rng.range(st.setback.0, st.setback.1);
        let mid = a + w * 0.5;
        // Front faces the street: 0 → −z (front 3), 1 → +z (front 1), 2 → −x (front 2), 3 → +x (front 0).
        let (centre, front) = match side {
            0 => (Vec2::new(mid, lo.y + set + depth * 0.5), 3),
            1 => (Vec2::new(mid, hi.y - set - depth * 0.5), 1),
            2 => (Vec2::new(lo.x + set + depth * 0.5, mid), 2),
            _ => (Vec2::new(hi.x - set - depth * 0.5, mid), 0),
        };
        let half = if side < 2 { Vec2::new(w * 0.5, depth * 0.5) } else { Vec2::new(depth * 0.5, w * 0.5) };
        out.push(building(rng, st.use_, ring, centre, Vec2::X, half, front, y, y - 0.3));
        a += w;
        // A gap, unless it would leave too little for the next building of a filled row.
        let gap = if rng.chance(st.gap.0) { rng.range(st.gap.1, st.gap.2) } else { 0.0 };
        if !st.fill || a1 - a - gap >= st.width.0 {
            a += gap;
        }
    }
}

/// Builds the layout for a road network (its seed and town height).
pub fn layout(net: &RoadNet) -> Layout {
    layout_with(net, &[])
}

/// A town block's kind set in the world data (`open::data::WorldData::lots`): block (`i`,
/// `j`) of the 6×6 grid, west to east and north to south.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize, bevy::reflect::Reflect)]
pub struct LotData {
    pub i: u32,
    pub j: u32,
    pub kind: LotKind,
}

/// Blocks along each side of the town grid.
pub const TOWN_BLOCKS: u32 = (2.0 * TOWN / BLOCK + 0.5) as u32;

/// Which ring of blocks block (`i`, `j`) is in (0 = the four round the centre).
fn ring_of(i: u32, j: u32) -> u32 {
    let n = TOWN_BLOCKS;
    let c = |k: u32| (k as i32 * 2 - (n as i32 - 1)).unsigned_abs() / 2;
    c(i).max(c(j))
}

/// The town's block kinds as the seed picks them.
fn town_kinds(seed: u64) -> std::collections::BTreeMap<(u32, u32), LotKind> {
    let n = TOWN_BLOCKS;
    // Shuffled per ring.
    let mut rng = Rng::new(seed ^ 0xB11D_0000);
    let mut kinds = std::collections::BTreeMap::new();
    for ring in 0..n.div_ceil(2) {
        let mut cells: Vec<(u32, u32)> = (0..n).flat_map(|j| (0..n).map(move |i| (i, j))).filter(|&(i, j)| ring_of(i, j) == ring).collect();
        let mut list: Vec<LotKind> = match ring {
            0 => vec![LotKind::Plaza, LotKind::Tower],
            1 => vec![LotKind::Park, LotKind::Parking, LotKind::Apartments, LotKind::Apartments],
            _ => vec![LotKind::Park, LotKind::Park, LotKind::Parking, LotKind::Apartments, LotKind::Rows(Use::Shop), LotKind::Rows(Use::Shop), LotKind::Rows(Use::Shop), LotKind::Rows(Use::Shop)],
        };
        let rest = match ring {
            0 => LotKind::Rows(Use::Downtown),
            1 => LotKind::Rows(Use::Shop),
            _ => LotKind::Rows(Use::House),
        };
        while list.len() < cells.len() {
            list.push(rest);
        }
        shuffle(&mut rng, &mut cells);
        for (c, k) in cells.into_iter().zip(list) {
            kinds.insert(c, k);
        }
    }
    kinds
}

/// The town's block kinds as the seed picks them, as world data.
pub fn town_lots(seed: u64) -> Vec<LotData> {
    town_kinds(seed).into_iter().map(|((i, j), kind)| LotData { i, j, kind }).collect()
}

/// [`layout`] with the block kinds in `lots` instead of the generated ones (a block's
/// buildings come from its own seed, so the other blocks don't change). Farms stay generated.
pub fn layout_with(net: &RoadNet, lots: &[LotData]) -> Layout {
    let seed = net.seed;
    let y = net.town_y;
    let n = TOWN_BLOCKS;
    let inset = RoadKind::Street.width() * 0.5 + SIDEWALK;
    let mut kinds = town_kinds(seed);
    for l in lots {
        match kinds.get_mut(&(l.i, l.j)) {
            Some(_) if l.kind == LotKind::Farm => warn!("lot ({}, {}): a town block can't be a farm; kept as generated", l.i, l.j),
            Some(k) => *k = l.kind,
            None => warn!("lot ({}, {}) is outside the town grid; ignored", l.i, l.j),
        }
    }
    let mut blocks = Vec::new();
    let mut buildings = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let x0 = -TOWN + i as f32 * BLOCK;
            let z0 = -TOWN + j as f32 * BLOCK;
            let (lo, hi) = (Vec2::new(x0 + inset, z0 + inset), Vec2::new(x0 + BLOCK - inset, z0 + BLOCK - inset));
            let kind = kinds[&(i, j)];
            let ring = ring_of(i, j);
            let mut rng = Rng::new(seed ^ 0xB10C_0000 ^ ((j * n + i) as u64).wrapping_mul(0x9E37_79B9));
            let first = buildings.len();
            match kind {
                LotKind::Rows(u) => {
                    let st = row_style(u);
                    let band = st.setback.1 + st.depth.1 + st.corner;
                    row(&mut buildings, &mut rng, &st, ring, lo, hi, 0, lo.x, hi.x, y);
                    row(&mut buildings, &mut rng, &st, ring, lo, hi, 1, lo.x, hi.x, y);
                    row(&mut buildings, &mut rng, &st, ring, lo, hi, 2, lo.y + band, hi.y - band, y);
                    row(&mut buildings, &mut rng, &st, ring, lo, hi, 3, lo.y + band, hi.y - band, y);
                }
                LotKind::Apartments => {
                    // Two slabs along opposite edges, set back behind a strip of grass.
                    let along_x = rng.chance(0.5);
                    let size = hi - lo;
                    for s in [0u8, 1] {
                        let depth = rng.range(13.0, 16.0);
                        let len = (if along_x { size.x } else { size.y }) - rng.range(10.0, 22.0);
                        let set = rng.range(4.0, 7.0);
                        let c = lo + size * 0.5;
                        let off = (if along_x { size.y } else { size.x }) * 0.5 - set - depth * 0.5;
                        let sign = if s == 0 { -1.0 } else { 1.0 };
                        let (centre, half, front) = if along_x {
                            (c + Vec2::new(0.0, sign * off), Vec2::new(len * 0.5, depth * 0.5), if s == 0 { 3 } else { 1 })
                        } else {
                            (c + Vec2::new(sign * off, 0.0), Vec2::new(depth * 0.5, len * 0.5), if s == 0 { 2 } else { 0 })
                        };
                        buildings.push(building(&mut rng, Use::Apartment, ring, centre, Vec2::X, half, front, y, y - 0.3));
                    }
                }
                LotKind::Tower => {
                    let s = rng.range(15.0, 19.0);
                    let c = (lo + hi) * 0.5 + Vec2::new(rng.range(-6.0, 6.0), rng.range(-6.0, 6.0));
                    // Its front faces the town centre.
                    let to = -c;
                    let front = if to.x.abs() > to.y.abs() { if to.x > 0.0 { 0 } else { 2 } } else if to.y > 0.0 { 1 } else { 3 };
                    buildings.push(building(&mut rng, Use::Tower, ring, c, Vec2::X, Vec2::splat(s), front, y, y - 0.3));
                }
                LotKind::Plaza | LotKind::Park | LotKind::Parking | LotKind::Farm => {}
            }
            blocks.push(Block { cell: Some((i, j)), min: lo, max: hi, kind, buildings: (first..buildings.len()).collect() });
        }
    }
    farms(net, &mut blocks, &mut buildings);
    Layout { seed, town_y: y, blocks, buildings }
}

/// A farmhouse (and usually a barn behind it) every so often along the country roads, between
/// the town's plateau and the ring, on ground flat enough, clear of every road.
fn farms(net: &RoadNet, blocks: &mut Vec<Block>, buildings: &mut Vec<Building>) {
    let mut rng = Rng::new(net.seed ^ 0xFA53);
    let every = (170.0 / STEP) as usize;
    for r in net.roads.iter().filter(|r| r.kind == RoadKind::Country) {
        let road = &r.road;
        let mut i = every / 2;
        while i < road.len() {
            let p = Vec2::new(road.pts[i].x, road.pts[i].z);
            let d = p.x.abs().max(p.y.abs());
            let r_len = p.length();
            if d > PLATEAU.0 + 60.0 && r_len < RING_R - 300.0 && rng.chance(0.65) {
                let dir = road.dir[i];
                let side = if rng.chance(0.5) { 1.0 } else { -1.0 };
                let out = dir.perp() * side;
                let reach = road.width[i] * 0.5 + rng.range(14.0, 22.0);
                let house_half = Vec2::new(rng.range(5.0, 6.5), rng.range(4.0, 5.0));
                let house_c = p + out * (reach + house_half.y);
                // House axis along the road: its local −z or +z faces the road.
                let front = if dir.perp().dot(-out) > 0.0 { 1 } else { 3 };
                let mut farm = vec![(Use::FarmHouse, house_c, house_half, front)];
                if rng.chance(0.75) {
                    let barn_half = Vec2::new(rng.range(8.0, 11.0), rng.range(5.5, 7.0));
                    let barn_c = house_c + out * (house_half.y + rng.range(8.0, 14.0) + barn_half.y) + dir * rng.range(-10.0, 10.0);
                    farm.push((Use::Barn, barn_c, barn_half, front));
                }
                let first = buildings.len();
                let mut ok = Vec::new();
                for (u, c, half, front) in farm {
                    let probe = Building { centre: c, axis: dir, half, front, floors: 1, floor_h: 3.0, ground: 0.0, base: 0.0, roof: Roof::Flat, pitch: 0.0, use_: u, wall: 0, roof_col: 0, bits: 0 };
                    let corners = probe.corners();
                    let hs: Vec<f32> = corners.iter().chain(std::iter::once(&c)).map(|q| roads::height(net, q.x, q.y)).collect();
                    let (lo_h, hi_h) = hs.iter().fold((f32::MAX, f32::MIN), |(a, b), &h| (a.min(h), b.max(h)));
                    let clear = clear_of_roads(net, &probe, 6.0) && hi_h - lo_h < 2.5 && !buildings.iter().any(|b| overlaps(b, &probe, 3.0));
                    if !clear {
                        // A barn that doesn't fit is dropped; so is a farm without its house.
                        if u == Use::FarmHouse {
                            break;
                        }
                        continue;
                    }
                    let b = building(&mut rng, u, 3, c, dir, half, front, hi_h + 0.15, lo_h - 0.4);
                    ok.push(b);
                    buildings.push(b);
                }
                if !ok.is_empty() {
                    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
                    for b in &ok {
                        for q in b.corners() {
                            lo = lo.min(q);
                            hi = hi.max(q);
                        }
                    }
                    blocks.push(Block { cell: None, min: lo, max: hi, kind: LotKind::Farm, buildings: (first..buildings.len()).collect() });
                }
                i += every;
            } else {
                i += every / 2;
            }
        }
    }
}

/// Points over a footprint (edges and inside, about every metre).
fn samples(b: &Building) -> Vec<Vec2> {
    let nx = (b.half.x * 2.0).ceil() as i32;
    let nz = (b.half.y * 2.0).ceil() as i32;
    let mut out = Vec::new();
    for j in 0..=nz {
        for i in 0..=nx {
            let l = Vec2::new(-b.half.x + 2.0 * b.half.x * i as f32 / nx as f32, -b.half.y + 2.0 * b.half.y * j as f32 / nz as f32);
            out.push(b.at(l));
        }
    }
    out
}

/// No part of the footprint within `margin` of any road's edge.
fn clear_of_roads(net: &RoadNet, b: &Building, margin: f32) -> bool {
    samples(b).into_iter().all(|p| match net.nearest(p, 40.0) {
        Some(n) => n.at.lateral.abs() > net.roads[n.road].road.half_width(&n.at) + margin,
        None => true,
    })
}

/// Do two footprints overlap (grown by `margin` each)? Separating axes on the two boxes.
pub fn overlaps(a: &Building, b: &Building, margin: f32) -> bool {
    let axes = [a.axis, a.axis.perp(), b.axis, b.axis.perp()];
    let d = b.centre - a.centre;
    for ax in axes {
        let ra = (a.half.x + margin) * a.axis.dot(ax).abs() + (a.half.y + margin) * a.axis.perp().dot(ax).abs();
        let rb = (b.half.x + margin) * b.axis.dot(ax).abs() + (b.half.y + margin) * b.axis.perp().dot(ax).abs();
        if d.dot(ax).abs() >= ra + rb {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------- collision

/// The walls as solid cylinders: along each wall [`WALL_INSET`] inside the face, at most
/// [`WALL_STEP`] apart (corners included), up to the roof's top.
pub fn obstacles(layout: &Layout) -> Vec<Obstacle> {
    let mut out = Vec::new();
    for b in &layout.buildings {
        let top = b.top();
        let h = (b.half - Vec2::splat(WALL_INSET)).max(Vec2::splat(0.0));
        if h.x < 0.5 || h.y < 0.5 {
            out.push(Obstacle::solid(b.centre, b.half.max_element() + 0.1, top));
            continue;
        }
        let c = [Vec2::new(-h.x, -h.y), Vec2::new(h.x, -h.y), Vec2::new(h.x, h.y), Vec2::new(-h.x, h.y)];
        for k in 0..4 {
            let (a, e) = (c[k], c[(k + 1) % 4]);
            let n = ((e - a).length() / WALL_STEP).ceil().max(1.0) as usize;
            for s in 0..n {
                let l = a + (e - a) * (s as f32 / n as f32);
                out.push(Obstacle::solid(b.at(l), WALL_R, top));
            }
        }
    }
    out
}

/// Obstacles bucketed on a grid for quick "near the car" queries.
#[derive(Clone, Debug)]
pub struct ObstacleGrid {
    lo: Vec2,
    cell: f32,
    nx: usize,
    nz: usize,
    cells: Vec<Vec<Obstacle>>,
    max_r: f32,
}

impl ObstacleGrid {
    /// Buckets `obs` (by centre) into `cell`-metre cells.
    pub fn new(obs: &[Obstacle], cell: f32) -> ObstacleGrid {
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        let mut max_r: f32 = 0.0;
        for o in obs {
            lo = lo.min(o.pos);
            hi = hi.max(o.pos);
            max_r = max_r.max(o.radius);
        }
        if obs.is_empty() {
            (lo, hi) = (Vec2::ZERO, Vec2::ZERO);
        }
        let nx = ((hi.x - lo.x) / cell).floor() as usize + 1;
        let nz = ((hi.y - lo.y) / cell).floor() as usize + 1;
        let mut cells = vec![Vec::new(); nx * nz];
        for o in obs {
            let (i, j) = (((o.pos.x - lo.x) / cell) as usize, ((o.pos.y - lo.y) / cell) as usize);
            cells[j.min(nz - 1) * nx + i.min(nx - 1)].push(*o);
        }
        ObstacleGrid { lo, cell, nx, nz, cells, max_r }
    }

    /// Appends (doesn't clear) every obstacle whose circle comes within `r` of `p`, in a fixed
    /// order (cells row by row, then insertion order).
    pub fn query(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
        let reach = r + self.max_r;
        let i0 = ((p.x - reach - self.lo.x) / self.cell).floor().max(0.0) as usize;
        let j0 = ((p.y - reach - self.lo.y) / self.cell).floor().max(0.0) as usize;
        let i1 = ((p.x + reach - self.lo.x) / self.cell).floor();
        let j1 = ((p.y + reach - self.lo.y) / self.cell).floor();
        if i1 < 0.0 || j1 < 0.0 {
            return;
        }
        let (i1, j1) = ((i1 as usize).min(self.nx - 1), (j1 as usize).min(self.nz - 1));
        for j in j0..=j1 {
            for i in i0..=i1 {
                for o in &self.cells[j * self.nx + i] {
                    let rr = r + o.radius;
                    if o.pos.distance_squared(p) < rr * rr {
                        out.push(*o);
                    }
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        self.cells.iter().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---------------------------------------------------------------- rendering

pub use render::{BuildingMesh, spawn};

mod render {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    use bevy::prelude::*;
    use engine::prelude::{KeepDetail, Scenery};

    use super::*;

    /// Atlas tiles (4 × 4): one storey × one bay each, or a patch of roof / ground.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[repr(u8)]
    pub(super) enum Tile {
        WinFlat = 0,
        WinBalcony,
        WinOffice,
        WinBand,
        Shopfront,
        ShopDoor,
        Plain,
        WinHouse,
        Planks,
        BarnDoor,
        HouseDoor,
        RoofFlat,
        RoofTiles,
        Paving,
        Asphalt,
        Parking,
    }

    const TILE_PX: u32 = 256;
    /// Texels kept clear at each tile's edge (mip bleeding).
    const MARGIN: f32 = 6.0 / TILE_PX as f32;

    /// Marks a spawned block mesh (one per town block or farm).
    #[derive(Component, Clone, Copy, Debug)]
    pub struct BuildingMesh {
        pub block: usize,
    }

    #[derive(Default)]
    pub(super) struct Buf {
        pub pos: Vec<[f32; 3]>,
        pub nrm: Vec<[f32; 3]>,
        pub uv: Vec<[f32; 2]>,
        pub col: Vec<[f32; 4]>,
        pub idx: Vec<u32>,
        /// Subtracted from every position (the mesh's local origin).
        pub origin: Vec3,
    }

    fn lin(c: [f32; 3], k: f32) -> [f32; 4] {
        let l = Color::srgb(c[0] * k, c[1] * k, c[2] * k).to_linear();
        [l.red, l.green, l.blue, 1.0]
    }

    /// Cheap hash in 0..1 for per-cell variety.
    fn hash(a: u32, b: i32, c: i32) -> f32 {
        let mut x = a.wrapping_mul(0x9E37_79B1) ^ (b as u32).wrapping_mul(0x85EB_CA6B) ^ (c as u32).wrapping_mul(0xC2B2_AE35);
        x ^= x >> 15;
        x = x.wrapping_mul(0x2C1B_3C6D);
        x ^= x >> 12;
        (x & 0xFFFF) as f32 / 65535.0
    }

    /// Clips a convex polygon to an axis-aligned rectangle.
    fn clip(poly: &[Vec2], lo: Vec2, hi: Vec2) -> Vec<Vec2> {
        let mut p = poly.to_vec();
        for (axis, bound, keep_below) in [(0, lo.x, false), (0, hi.x, true), (1, lo.y, false), (1, hi.y, true)] {
            if p.is_empty() {
                break;
            }
            let inside = |v: Vec2| if keep_below { v[axis] <= bound } else { v[axis] >= bound };
            let mut out = Vec::with_capacity(p.len() + 2);
            for k in 0..p.len() {
                let (a, b) = (p[k], p[(k + 1) % p.len()]);
                let (ia, ib) = (inside(a), inside(b));
                if ia {
                    out.push(a);
                }
                if ia != ib {
                    let t = (bound - a[axis]) / (b[axis] - a[axis]);
                    out.push(a + (b - a) * t);
                }
            }
            p = out;
        }
        p
    }

    impl Buf {
        /// A flat convex polygon `poly` in face coordinates (s, t) on the plane through `o`
        /// with up-direction `t_ax` and normal `n` (s = t × n), cut into a grid of `cell`-sized
        /// cells from (0, 0); `pick(i, j)` gives each cell's tile and colour.
        pub fn face(&mut self, o: Vec3, t_ax: Vec3, n: Vec3, poly: &[Vec2], cell: Vec2, mut pick: impl FnMut(i32, i32) -> (Tile, [f32; 4])) {
            let s_ax = t_ax.cross(n);
            let mut poly = poly.to_vec();
            let area: f32 = (0..poly.len()).map(|k| poly[k].perp_dot(poly[(k + 1) % poly.len()])).sum();
            if area < 0.0 {
                poly.reverse();
            }
            let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
            for p in &poly {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            let eps = 1e-3;
            let (i0, i1) = (((lo.x + eps) / cell.x).floor() as i32, ((hi.x - eps) / cell.x).floor() as i32);
            let (j0, j1) = (((lo.y + eps) / cell.y).floor() as i32, ((hi.y - eps) / cell.y).floor() as i32);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let c0 = Vec2::new(i as f32 * cell.x, j as f32 * cell.y);
                    let part = clip(&poly, c0, c0 + cell);
                    if part.len() < 3 {
                        continue;
                    }
                    let (tile, col) = pick(i, j);
                    let t = tile as u8;
                    let (tc, tr) = ((t % 4) as f32, (t / 4) as f32);
                    let base = self.pos.len() as u32;
                    for q in &part {
                        let l = (*q - c0) / cell;
                        let w = o + s_ax * q.x + t_ax * q.y - self.origin;
                        self.pos.push(w.to_array());
                        self.nrm.push(n.to_array());
                        let u = (tc + MARGIN + l.x.clamp(0.0, 1.0) * (1.0 - 2.0 * MARGIN)) / 4.0;
                        let v = (tr + MARGIN + (1.0 - l.y.clamp(0.0, 1.0)) * (1.0 - 2.0 * MARGIN)) / 4.0;
                        self.uv.push([u, v]);
                        self.col.push(col);
                    }
                    for k in 1..part.len() as u32 - 1 {
                        self.idx.extend_from_slice(&[base, base + k, base + k + 1]);
                    }
                }
            }
        }

        /// A horizontal polygon (world x, z) at height `y`, facing up.
        pub fn ground(&mut self, y: f32, poly: &[Vec2], cell: Vec2, pick: impl FnMut(i32, i32) -> (Tile, [f32; 4])) {
            // t = −z, n = +y → s = +x: face coordinates are (x, −z).
            let p: Vec<Vec2> = poly.iter().map(|q| Vec2::new(q.x, -q.y)).collect();
            self.face(Vec3::new(0.0, y, 0.0), Vec3::NEG_Z, Vec3::Y, &p, cell, pick);
        }

        /// [`Buf::ground`] draped on the terrain: every vertex `lift` m above `height(x, z)`.
        pub fn draped(&mut self, height: &dyn Fn(f32, f32) -> f32, lift: f32, poly: &[Vec2], cell: Vec2, pick: impl FnMut(i32, i32) -> (Tile, [f32; 4])) {
            let start = self.pos.len();
            self.ground(0.0, poly, cell, pick);
            let o = self.origin;
            for v in &mut self.pos[start..] {
                v[1] = height(v[0] + o.x, v[2] + o.z) + lift - o.y;
            }
        }

        pub fn into_mesh(self) -> Mesh {
            let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.pos);
            m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv);
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.col);
            m.insert_indices(Indices::U32(self.idx));
            m
        }
    }

    fn v3(p: Vec2, y: f32) -> Vec3 {
        Vec3::new(p.x, y, p.y)
    }

    /// The tile of a wall cell: storey `j` (0 = ground), bay `i` of `n`, on the front or not.
    fn wall_tile(b: &Building, front: bool, side_wall: bool, i: i32, n: i32, j: i32) -> Tile {
        let mid = i == n / 2;
        let alt = hash(b.bits, i, j) < 0.5;
        match b.use_ {
            Use::Downtown | Use::Tower => {
                if j == 0 {
                    if front && mid { Tile::ShopDoor } else if front || !side_wall { Tile::Shopfront } else { Tile::Plain }
                } else if side_wall && b.use_ == Use::Downtown {
                    Tile::Plain
                } else if b.bits & 2 == 0 {
                    Tile::WinOffice
                } else {
                    Tile::WinBand
                }
            }
            Use::Shop => {
                if j == 0 {
                    if front && mid { Tile::ShopDoor } else if front { Tile::Shopfront } else { Tile::Plain }
                } else if side_wall {
                    Tile::Plain
                } else if b.bits & 4 == 0 && alt {
                    Tile::WinBalcony
                } else {
                    Tile::WinFlat
                }
            }
            Use::Apartment => {
                if j == 0 {
                    if front && (i == n / 4 || i == 3 * n / 4) { Tile::ShopDoor } else { Tile::WinFlat }
                } else if b.bits & 8 == 0 && i % 2 == 0 {
                    Tile::WinBalcony
                } else {
                    Tile::WinFlat
                }
            }
            Use::House | Use::FarmHouse => {
                if j == 0 && front && mid {
                    Tile::HouseDoor
                } else if !front && alt && hash(b.bits, i + 7, j) < 0.4 {
                    Tile::Plain
                } else {
                    Tile::WinHouse
                }
            }
            Use::Barn => {
                if front && mid { Tile::BarnDoor } else { Tile::Planks }
            }
        }
    }

    /// Adds a building's walls, roof and details.
    pub(super) fn add_building(buf: &mut Buf, b: &Building) {
        let wall = WALLS[b.wall as usize];
        let roof_c = ROOFS[b.roof_col as usize];
        let ax = b.axis;
        let normals = [ax, ax.perp(), -ax, -ax.perp()];
        let hn = [b.half.x, b.half.y, b.half.x, b.half.y];
        let hw = [b.half.y, b.half.x, b.half.y, b.half.x];
        let eave = b.eave() - b.ground;
        let (ridge, _, hq) = b.ridge();
        let rise = hq * b.pitch;
        let bay = match b.use_ {
            Use::Downtown | Use::Tower => 2.7,
            Use::House | Use::FarmHouse => 3.3,
            Use::Barn => 3.6,
            _ => 3.0,
        };
        let parapet = if b.roof == Roof::Flat { PARAPET.0 } else { 0.0 };
        for k in 0..4 {
            let n2 = normals[k];
            let n = v3(n2, 0.0);
            let s = Vec3::Y.cross(n);
            let len = hw[k] * 2.0;
            let o = v3(b.centre + n2 * hn[k], b.ground) - s * hw[k];
            let cols = (len / bay).round().max(1.0) as i32;
            let cw = len / cols as f32;
            let front = k as u8 == b.front;
            let side_wall = k as u8 % 2 != b.front % 2;
            let mut poly = vec![Vec2::new(0.0, 0.0), Vec2::new(len, 0.0), Vec2::new(len, eave + parapet)];
            // Gable end: a triangle up to the ridge.
            if b.roof == Roof::Gable && n2.dot(ridge).abs() > 0.5 {
                poly.push(Vec2::new(len * 0.5, eave + rise));
            }
            poly.push(Vec2::new(0.0, eave + parapet));
            let floors = b.floors as i32;
            buf.face(o, Vec3::Y, n, &poly, Vec2::new(cw, b.floor_h), |i, j| {
                let jit = 0.94 + 0.1 * hash(b.bits ^ 0x55, i + k as i32 * 97, j);
                if j >= floors {
                    (Tile::Plain, lin(wall, if b.roof == Roof::Flat { 0.9 } else { 1.0 }))
                } else {
                    (wall_tile(b, front, side_wall, i, cols, j), lin(wall, jit))
                }
            });
            // Plinth from the lowest ground up to the floor.
            let depth = b.ground - b.base + 0.05;
            if depth > 0.06 {
                let p = [Vec2::new(0.0, -depth), Vec2::new(len, -depth), Vec2::new(len, 0.0), Vec2::new(0.0, 0.0)];
                buf.face(o, Vec3::Y, n, &p, Vec2::new(cw, 3.0), |_, _| (Tile::Plain, lin([0.55, 0.53, 0.5], 1.0)));
            }
        }
        let top = b.ground + eave;
        match b.roof {
            Roof::Flat => {
                let t = PARAPET.1;
                let h = b.half - Vec2::splat(t);
                let c = [b.at(Vec2::new(-h.x, -h.y)), b.at(Vec2::new(h.x, -h.y)), b.at(Vec2::new(h.x, h.y)), b.at(Vec2::new(-h.x, h.y))];
                buf.ground(top, &c, Vec2::splat(4.0), |_, _| (Tile::RoofFlat, lin(roof_c, 1.0)));
                // Parapet: inner faces and the cap.
                for k in 0..4 {
                    let n2 = -normals[k];
                    let n = v3(n2, 0.0);
                    let s = Vec3::Y.cross(n);
                    let len = (hw[k] - t) * 2.0;
                    let o = v3(b.centre - n2 * (hn[k] - t), top) - s * (hw[k] - t);
                    let p = [Vec2::ZERO, Vec2::new(len, 0.0), Vec2::new(len, PARAPET.0), Vec2::new(0.0, PARAPET.0)];
                    buf.face(o, Vec3::Y, n, &p, Vec2::new(3.0, 3.2), |_, _| (Tile::Plain, lin(wall, 0.8)));
                }
                let full = b.corners();
                for k in 0..4 {
                    let (a, e) = (full[k], full[(k + 1) % 4]);
                    let (ai, ei) = (c[k], c[(k + 1) % 4]);
                    buf.ground(top + PARAPET.0, &[a, e, ei, ai], Vec2::splat(3.0), |_, _| (Tile::Plain, lin([0.75, 0.74, 0.72], 1.0)));
                }
                if b.has_roof_box() {
                    let bh = Vec2::new(rng_box(b, 0, 1.6, 3.0), rng_box(b, 1, 1.4, 2.4));
                    let off = Vec2::new((rng_box(b, 2, 0.0, 1.0) - 0.5) * (h.x - bh.x - 1.0), (rng_box(b, 3, 0.0, 1.0) - 0.5) * (h.y - bh.y - 1.0));
                    let bx = Building { centre: b.at(off), half: bh, floors: 1, floor_h: 2.8, ground: top, base: top, roof: Roof::Flat, use_: Use::Shop, ..*b };
                    roof_box(buf, &bx, wall);
                }
            }
            Roof::Gable | Roof::Hip => {
                let hr = b.ridge().1;
                let q = ridge.perp();
                let k = b.pitch;
                let o_e = 0.45; // eave overhang
                let o_g = if b.roof == Roof::Gable { 0.35 } else { o_e };
                let sl = (1.0 + k * k).sqrt();
                let roof_tint = lin(roof_c, 1.0);
                let under = lin([0.85, 0.82, 0.78], 0.6);
                let mut plane = |nh: Vec2, half_run: f32, half_len: f32, ridge_half: f32| {
                    // nh: outward horizontal normal; half_run: centre to eave (inc. overhang);
                    // half_len: half the eave's length; ridge_half: half the ridge's length.
                    let n = (v3(nh, 0.0) * k + Vec3::Y) / sl;
                    let t_ax = (v3(-nh, 0.0) + Vec3::Y * k) / sl;
                    let o = v3(b.centre + nh * half_run, top - (half_run - hq) * k);
                    let len = half_run * sl;
                    let poly = [Vec2::new(-half_len, 0.0), Vec2::new(half_len, 0.0), Vec2::new(ridge_half, len), Vec2::new(-ridge_half, len)];
                    buf.face(o, t_ax, n, &poly, Vec2::new(2.0, 1.6), |_, _| (Tile::RoofTiles, roof_tint));
                    // Underside (seen under the eaves).
                    let mirrored: Vec<Vec2> = poly.iter().map(|p| Vec2::new(-p.x, p.y)).collect();
                    buf.face(o - Vec3::Y * 0.02, t_ax, -n, &mirrored, Vec2::new(3.0, 3.0), |_, _| (Tile::Plain, under));
                };
                match b.roof {
                    Roof::Gable => {
                        for sgn in [1.0f32, -1.0] {
                            plane(q * sgn, hq + o_e, hr + o_g, hr + o_g);
                        }
                    }
                    _ => {
                        let rh = (hr - hq).max(0.0);
                        for sgn in [1.0f32, -1.0] {
                            plane(q * sgn, hq + o_e, hr + o_e, rh);
                            plane(ridge * sgn, hq + o_e, hq + o_e, 0.0);
                        }
                    }
                }
            }
        }
    }

    fn rng_box(b: &Building, k: i32, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * hash(b.bits, k, 911)
    }

    /// A small box on a flat roof (stair housing, plant room).
    fn roof_box(buf: &mut Buf, b: &Building, wall: [f32; 3]) {
        let normals = [b.axis, b.axis.perp(), -b.axis, -b.axis.perp()];
        let hn = [b.half.x, b.half.y, b.half.x, b.half.y];
        let hw = [b.half.y, b.half.x, b.half.y, b.half.x];
        let h = b.floor_h;
        for k in 0..4 {
            let n = v3(normals[k], 0.0);
            let s = Vec3::Y.cross(n);
            let len = hw[k] * 2.0;
            let o = v3(b.centre + normals[k] * hn[k], b.ground) - s * hw[k];
            let p = [Vec2::ZERO, Vec2::new(len, 0.0), Vec2::new(len, h), Vec2::new(0.0, h)];
            buf.face(o, Vec3::Y, n, &p, Vec2::new(len, 3.2), |_, _| (Tile::Plain, lin(wall, 0.85)));
        }
        buf.ground(b.ground + h, &b.corners(), Vec2::splat(4.0), |_, _| (Tile::RoofFlat, lin(ROOFS[3], 0.9)));
    }

    /// Sidewalks round a town block and its lot's paving, draped on the ground (beside the
    /// streets the terrain has a shoulder, 0.15 m high).
    pub(super) fn add_lot(buf: &mut Buf, blk: &Block, height: &dyn Fn(f32, f32) -> f32) {
        let paving = |_: i32, _: i32| (Tile::Paving, lin([0.86, 0.85, 0.82], 1.0));
        let (lo, hi) = (blk.min, blk.max);
        let (olo, ohi) = (lo - Vec2::splat(SIDEWALK), hi + Vec2::splat(SIDEWALK));
        let rect = |a: Vec2, b: Vec2| [Vec2::new(a.x, a.y), Vec2::new(b.x, a.y), Vec2::new(b.x, b.y), Vec2::new(a.x, b.y)];
        let lift = 0.05;
        // Sidewalks sit up level with the kerb (`art::junctions`), their outer corners rounded
        // to the kerb's fillet at the junctions.
        let town_y = buf.origin.y;
        let walk = |x: f32, z: f32| height(x, z).max(town_y + crate::open::junctions::SIDEWALK_TOP - lift);
        let r = crate::open::junctions::TOWN_KERB_R;
        buf.draped(&walk, lift, &rect(Vec2::new(olo.x + r, olo.y), Vec2::new(ohi.x - r, lo.y)), Vec2::splat(2.0), paving);
        buf.draped(&walk, lift, &rect(Vec2::new(olo.x + r, hi.y), Vec2::new(ohi.x - r, ohi.y)), Vec2::splat(2.0), paving);
        buf.draped(&walk, lift, &rect(Vec2::new(olo.x, olo.y + r), Vec2::new(lo.x, ohi.y - r)), Vec2::splat(2.0), paving);
        buf.draped(&walk, lift, &rect(Vec2::new(hi.x, olo.y + r), Vec2::new(ohi.x, ohi.y - r)), Vec2::splat(2.0), paving);
        for (k, into) in [(olo, Vec2::new(1.0, 1.0)), (Vec2::new(ohi.x, olo.y), Vec2::new(-1.0, 1.0)), (ohi, Vec2::new(-1.0, -1.0)), (Vec2::new(olo.x, ohi.y), Vec2::new(1.0, -1.0))] {
            for corner in crate::open::junctions::sidewalk_corner_pieces(k, into, SIDEWALK) {
                if corner.len() >= 3 {
                    buf.draped(&walk, lift, &corner, Vec2::splat(2.0), paving);
                }
            }
        }
        match blk.kind {
            LotKind::Plaza | LotKind::Tower => {
                buf.draped(height, lift - 0.01, &rect(lo, hi), Vec2::splat(2.0), |i, j| (Tile::Paving, lin([0.9, 0.86, 0.78], 0.93 + 0.07 * hash(7, i, j))));
            }
            LotKind::Parking => {
                // Rows of bays (5.5 m deep, 2.5 m wide), back to back, with aisles between.
                buf.draped(height, lift - 0.01, &rect(lo, hi), Vec2::new(2.5, 5.5), move |_, j| {
                    let bay = j.rem_euclid(3) != 2;
                    (if bay { Tile::Parking } else { Tile::Asphalt }, lin([1.0, 1.0, 1.0], 1.0))
                });
            }
            _ => {}
        }
    }

    /// The procedural facade atlas (colour) and its metallic-roughness twin.
    pub(super) fn atlas() -> (Image, Image) {
        use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        let size = TILE_PX * 4;
        let mut px = vec![0u8; (size * size * 4) as usize];
        let mr_size = size / 2;
        let mut mr = vec![0u8; (mr_size * mr_size * 4) as usize];
        for y in 0..size {
            for x in 0..size {
                let tile = (y / TILE_PX) * 4 + x / TILE_PX;
                let u = (x % TILE_PX) as f32 / TILE_PX as f32;
                let v = 1.0 - (y % TILE_PX) as f32 / TILE_PX as f32;
                let (c, rough) = texel(tile, u, v, Vec2::new(x as f32, y as f32));
                let i = ((y * size + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[(c.x.clamp(0.0, 1.0) * 255.0) as u8, (c.y.clamp(0.0, 1.0) * 255.0) as u8, (c.z.clamp(0.0, 1.0) * 255.0) as u8, 255]);
                if x % 2 == 0 && y % 2 == 0 {
                    let j = (((y / 2) * mr_size + x / 2) * 4) as usize;
                    mr[j..j + 4].copy_from_slice(&[0, (rough * 255.0) as u8, 0, 255]);
                }
            }
        }
        let sampler = ImageSampler::Descriptor(ImageSamplerDescriptor { address_mode_u: ImageAddressMode::ClampToEdge, address_mode_v: ImageAddressMode::ClampToEdge, anisotropy_clamp: 8, ..ImageSamplerDescriptor::linear() });
        let mut img = Image::new(Extent3d { width: size, height: size, depth_or_array_layers: 1 }, TextureDimension::D2, px, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
        img.sampler = sampler.clone();
        engine::gfx::build_mips(&mut img);
        let mut mri = Image::new(Extent3d { width: mr_size, height: mr_size, depth_or_array_layers: 1 }, TextureDimension::D2, mr, TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD);
        mri.sampler = sampler;
        engine::gfx::build_mips(&mut mri);
        (img, mri)
    }

    fn inside(u: f32, v: f32, u0: f32, u1: f32, v0: f32, v1: f32) -> bool {
        u >= u0 && u <= u1 && v >= v0 && v <= v1
    }

    /// A framed window: glass in (u0..u1, v0..v1), frame `f` wide, optional mullion/transom.
    fn window(u: f32, v: f32, r: [f32; 4], f: f32, mullion: bool, transom: Option<f32>, frame: Vec3) -> Option<(Vec3, f32)> {
        let [u0, u1, v0, v1] = r;
        if !inside(u, v, u0 - f, u1 + f, v0 - f, v1 + f) {
            return None;
        }
        let inner = inside(u, v, u0, u1, v0, v1);
        let on_mullion = mullion && (u - (u0 + u1) * 0.5).abs() < f * 0.5;
        let on_transom = transom.is_some_and(|t| (v - t).abs() < f * 0.5);
        if !inner || on_mullion || on_transom {
            return Some((frame, 0.55));
        }
        // Glass: dark, sky reflected brighter at the top, a soft diagonal sheen.
        let g = (v - v0) / (v1 - v0);
        let sheen = (((u - u0) * 2.0 + g * 1.3) * 3.0).fract();
        let sheen = if sheen < 0.18 { 0.05 } else { 0.0 };
        let k = 0.10 + 0.10 * g + sheen;
        Some((Vec3::new(k * 0.85, k * 0.98, k * 1.15), 0.12))
    }

    /// One atlas texel: colour (sRGB 0..1) and roughness. `u` right, `v` up within the tile.
    fn texel(tile: u32, u: f32, v: f32, px: Vec2) -> (Vec3, f32) {
        use crate::stage::noise;
        let n1 = noise(px / 9.0, 11) * 0.5 + noise(px / 2.5, 12) * 0.5;
        let stucco = Vec3::splat(0.82 + 0.035 * n1);
        let frame = Vec3::splat(0.93);
        let dark_frame = Vec3::splat(0.22);
        let floor_line = |c: Vec3| if v < 0.025 { c * 0.88 } else { c };
        match tile {
            0 => {
                // Flat window with a sill.
                if let Some(w) = window(u, v, [0.3, 0.7, 0.32, 0.82], 0.03, true, Some(0.66), frame) {
                    return w;
                }
                if inside(u, v, 0.26, 0.74, 0.27, 0.30) {
                    return (Vec3::splat(0.7), 0.7);
                }
                (floor_line(stucco), 0.9)
            }
            1 => {
                // French window with a balcony rail.
                if inside(u, v, 0.22, 0.78, 0.04, 0.38) && ((u * 40.0).fract() < 0.18 || v > 0.35 || v < 0.06) && !(v > 0.06 && v < 0.35 && u > 0.24 && u < 0.76 && (u * 40.0).fract() >= 0.18) {
                    return (Vec3::splat(0.18), 0.4);
                }
                if let Some(w) = window(u, v, [0.32, 0.68, 0.06, 0.84], 0.03, true, None, frame) {
                    return w;
                }
                if inside(u, v, 0.18, 0.82, 0.0, 0.05) {
                    return (Vec3::splat(0.62), 0.8);
                }
                (floor_line(stucco), 0.9)
            }
            2 => {
                // Office: a wide window over a spandrel panel.
                if let Some(w) = window(u, v, [0.06, 0.94, 0.27, 0.95], 0.025, true, Some(0.78), Vec3::splat(0.4)) {
                    return w;
                }
                if v < 0.24 {
                    return (Vec3::splat(0.62 + 0.03 * n1), 0.6);
                }
                (stucco, 0.85)
            }
            3 => {
                // Ribbon glazing.
                if v > 0.3 {
                    let m = (u - 0.0).abs() < 0.02 || (u - 1.0).abs() < 0.02 || (u - 0.5).abs() < 0.012;
                    if m || v > 0.98 {
                        return (Vec3::splat(0.3), 0.5);
                    }
                    return window(u, v, [0.0, 1.0, 0.3, 0.98], 0.0, false, None, frame).unwrap_or((Vec3::ZERO, 0.2));
                }
                (Vec3::splat(0.55 + 0.03 * n1), 0.7)
            }
            4 | 5 => {
                // Shopfront (5: with a door in the middle).
                if (0.74..=0.93).contains(&v) {
                    return (Vec3::new(0.86, 0.84, 0.78) * (0.9 + 0.05 * n1), 0.7);
                }
                if tile == 5
                    && inside(u, v, 0.34, 0.66, 0.0, 0.70)
                    && let Some(w) = window(u, v, [0.37, 0.63, 0.0, 0.67], 0.03, false, None, dark_frame)
                {
                    if (u - 0.6).abs() < 0.01 && (v - 0.33).abs() < 0.08 {
                        return (Vec3::splat(0.7), 0.3);
                    }
                    return w;
                }
                if let Some(w) = window(u, v, [0.04, 0.96, 0.08, 0.69], 0.025, false, Some(0.55), dark_frame) {
                    return w;
                }
                if v < 0.06 {
                    return (Vec3::splat(0.45 + 0.04 * n1), 0.8);
                }
                (floor_line(stucco), 0.9)
            }
            6 => (floor_line(Vec3::splat(0.80 + 0.04 * n1)), 0.92),
            7 => {
                // House window with shutters.
                let sh = Vec3::new(0.82, 0.86, 0.84);
                if inside(u, v, 0.18, 0.31, 0.36, 0.82) || inside(u, v, 0.69, 0.82, 0.36, 0.82) {
                    let slat = ((v * 60.0).fract() < 0.25) as i32 as f32;
                    return (sh * (0.62 - 0.12 * slat), 0.7);
                }
                if let Some(w) = window(u, v, [0.33, 0.67, 0.38, 0.8], 0.03, true, Some(0.62), frame) {
                    return w;
                }
                if inside(u, v, 0.3, 0.7, 0.34, 0.37) {
                    return (Vec3::splat(0.72), 0.7);
                }
                (stucco, 0.9)
            }
            8 | 9 => {
                // Vertical boards (9: a framed barn door with a brace).
                let board = (u * 9.0).fract();
                let grain = noise(Vec2::new(px.x / 3.0, px.y / 40.0), 21) * 0.08;
                let mut c = Vec3::splat(0.74 + grain) * if board < 0.06 { 0.55 } else { 1.0 };
                if tile == 9 && inside(u, v, 0.08, 0.92, 0.0, 0.9) {
                    let edge = !inside(u, v, 0.13, 0.87, 0.0, 0.85);
                    let d1 = ((u - 0.13) / 0.74 - v / 0.85).abs() < 0.05;
                    let d2 = ((u - 0.13) / 0.74 - (1.0 - v / 0.85)).abs() < 0.05;
                    if edge || d1 || d2 {
                        c = Vec3::splat(0.95);
                    }
                }
                (c, 0.85)
            }
            10 => {
                // A house door with a step and a fanlight.
                if inside(u, v, 0.3, 0.7, 0.0, 0.04) {
                    return (Vec3::splat(0.6), 0.85);
                }
                if inside(u, v, 0.35, 0.65, 0.04, 0.72) {
                    let border = !inside(u, v, 0.37, 0.63, 0.04, 0.70);
                    if border {
                        return (frame, 0.6);
                    }
                    let panel = inside(u, v, 0.41, 0.59, 0.12, 0.36) || inside(u, v, 0.41, 0.59, 0.42, 0.64);
                    let wood = Vec3::new(0.36, 0.22, 0.14) * (1.0 + 0.1 * n1);
                    return (if panel { wood * 0.8 } else { wood }, 0.6);
                }
                if let Some(w) = window(u, v, [0.37, 0.63, 0.74, 0.82], 0.02, false, None, frame) {
                    return w;
                }
                (stucco, 0.9)
            }
            11 => {
                // Flat roof: gravel.
                let g = noise(px / 1.3, 31) * 0.5 + noise(px / 7.0, 32) * 0.5;
                (Vec3::splat(0.55 + 0.1 * g), 0.95)
            }
            12 => {
                // Roof tiles: courses with a shadow under each, staggered joints.
                let rows = 5.0;
                let r = v * rows;
                let course = r.floor();
                let fv = r.fract();
                let off = if course as i32 % 2 == 0 { 0.0 } else { 0.5 };
                let fu = (u * 4.0 + off).fract();
                let mut k = 0.62 + 0.38 * fv.powf(0.6);
                if fu < 0.04 || fu > 0.96 {
                    k *= 0.7;
                }
                (Vec3::splat(k * (0.95 + 0.05 * n1)), 0.75)
            }
            13 => {
                // Paving slabs.
                let (su, sv) = ((u * 3.0).fract(), (v * 3.0).fract());
                let joint = su < 0.03 || sv < 0.03;
                let slab = noise(Vec2::new((u * 3.0).floor() + px.x.div_euclid(256.0) * 3.0, (v * 3.0).floor()), 41) * 0.04;
                (Vec3::splat(if joint { 0.5 } else { 0.78 + slab + 0.02 * n1 }), 0.85)
            }
            14 | 15 => {
                let a = Vec3::splat(0.26 + 0.05 * (noise(px / 1.5, 51) * 0.6 + noise(px / 12.0, 52) * 0.4));
                if tile == 15 && u < 0.035 && v > 0.08 {
                    return (Vec3::splat(0.85), 0.7);
                }
                (a, 0.9)
            }
            _ => (Vec3::ONE, 1.0),
        }
    }

    /// Builds a block's mesh (local to the block's centre) or `None` if it has nothing.
    pub(super) fn block_mesh(layout: &Layout, k: usize, height: &dyn Fn(f32, f32) -> f32) -> Option<(Vec3, Buf)> {
        let blk = &layout.blocks[k];
        let c = blk.centre();
        let origin = Vec3::new(c.x, layout.town_y, c.y);
        let mut buf = Buf { origin, ..default() };
        if blk.cell.is_some() {
            add_lot(&mut buf, blk, height);
        }
        for &i in &blk.buildings {
            add_building(&mut buf, &layout.buildings[i]);
        }
        (!buf.idx.is_empty()).then_some((origin, buf))
    }

    /// Spawns every block's mesh: one entity per town block and farm, all sharing one material,
    /// casting shadows. Static scenery kept at full detail (the far blocks are a few thousand
    /// triangles each; Bevy's per-cascade culling keeps their shadows cheap). `height` is the
    /// rendered terrain's height (`OpenWorld::heights`): sidewalks and lot paving lie on it.
    /// Returns the entities (for despawning). Nothing in headless runs.
    pub fn spawn(world: &mut World, layout: &Layout, height: &dyn Fn(f32, f32) -> f32) -> Vec<Entity> {
        if !crate::art::visual(world) {
            return Vec::new();
        }
        let (color, mr) = atlas();
        let (color, mr) = {
            let mut images = world.resource_mut::<Assets<Image>>();
            (images.add(color), images.add(mr))
        };
        let mat = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
            base_color_texture: Some(color),
            metallic_roughness_texture: Some(mr),
            perceptual_roughness: 1.0,
            metallic: 1.0,
            reflectance: 0.4,
            ..default()
        });
        let mut out = Vec::new();
        for k in 0..layout.blocks.len() {
            let Some((origin, buf)) = block_mesh(layout, k, height) else { continue };
            let mesh = buf.into_mesh();
            let aabb = bevy::camera::primitives::MeshAabb::get_aabb(&mesh);
            let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
            let mut e = world.spawn((Name::new("town block"), BuildingMesh { block: k }, Mesh3d(h), MeshMaterial3d(mat.clone()), Transform::from_translation(origin), Scenery, KeepDetail));
            if let Some(a) = aabb {
                e.insert((a, bevy::camera::visibility::NoAutoAabb));
            }
            out.push(e.id());
        }
        out
    }

    #[cfg(test)]
    pub(super) fn triangles(layout: &Layout) -> (usize, usize, usize) {
        let mut tris = 0;
        let mut verts = 0;
        let mut meshes = 0;
        for k in 0..layout.blocks.len() {
            if let Some((_, b)) = block_mesh(layout, k, &|_, _| layout.town_y) {
                tris += b.idx.len() / 3;
                verts += b.pos.len();
                meshes += 1;
            }
        }
        (meshes, tris, verts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open::SEED;

    fn net() -> RoadNet {
        roads::generate(SEED)
    }

    #[test]
    fn deterministic() {
        let n = net();
        let (a, b) = (layout(&n), layout(&n));
        assert_eq!(a.buildings, b.buildings);
        assert_eq!(obstacles(&a), obstacles(&b));
        let town = a.blocks.iter().filter(|b| b.cell.is_some()).count();
        assert_eq!(town, 36);
        let count = |u| a.buildings.iter().filter(|b| b.use_ == u).count();
        eprintln!(
            "buildings {} (downtown {}, shop {}, house {}, apartment {}, tower {}, farmhouse {}, barn {})",
            a.buildings.len(),
            count(Use::Downtown),
            count(Use::Shop),
            count(Use::House),
            count(Use::Apartment),
            count(Use::Tower),
            count(Use::FarmHouse),
            count(Use::Barn)
        );
        assert!(a.buildings.len() > 250, "{}", a.buildings.len());
        for u in [Use::Downtown, Use::Shop, Use::House, Use::Apartment, Use::Tower] {
            assert!(count(u) > 0, "{u:?}");
        }
        for k in [LotKind::Park, LotKind::Parking, LotKind::Plaza] {
            assert!(a.blocks.iter().any(|b| b.kind == k), "{k:?}");
        }
        // Another seed gives another town.
        let mut other = n.clone();
        other.seed ^= 1;
        assert_ne!(layout(&other).buildings, a.buildings);
    }

    #[test]
    fn footprints_clear_of_streets_sidewalks_and_each_other() {
        let n = net();
        let l = layout(&n);
        for (i, b) in l.buildings.iter().enumerate() {
            let margin = if b.use_ == Use::FarmHouse || b.use_ == Use::Barn { 6.0 } else { SIDEWALK - 0.01 };
            for p in samples(b) {
                if let Some(at) = n.nearest(p, 40.0) {
                    let half = n.roads[at.road].road.half_width(&at.at);
                    assert!(at.at.lateral.abs() > half + margin, "building {i} ({:?}) at {p} is {:.2} m from a {:?} centreline", b.use_, at.at.lateral.abs(), n.roads[at.road].kind);
                }
            }
            for (j, o) in l.buildings.iter().enumerate().skip(i + 1) {
                assert!(!overlaps(b, o, -0.01), "buildings {i} {:?} and {j} {:?} overlap", b, o);
            }
            assert!(b.half.min_element() > 2.0 && b.floors >= 1 && b.floors <= 8, "{b:?}");
            assert!(b.top() > b.eave() && b.eave() > b.ground && b.ground >= b.base);
        }
    }

    #[test]
    fn town_inside_the_plateau_and_blocks() {
        let n = net();
        let l = layout(&n);
        for blk in l.blocks.iter().filter(|b| b.cell.is_some()) {
            for &i in &blk.buildings {
                let b = &l.buildings[i];
                assert!((b.ground - n.town_y).abs() < 1e-4);
                assert!((2..=8).contains(&b.floors), "{b:?}");
                for c in b.corners() {
                    assert!(c.x.abs().max(c.y.abs()) < PLATEAU.0, "{c}");
                    assert!(c.x >= blk.min.x - 1e-3 && c.x <= blk.max.x + 1e-3 && c.y >= blk.min.y - 1e-3 && c.y <= blk.max.y + 1e-3, "{c} outside its lot");
                }
            }
        }
        // Taller downtown than out at the edge.
        let mean = |f: &dyn Fn(f32) -> bool| {
            let v: Vec<f32> = l.buildings.iter().filter(|b| b.ground == l.town_y && f(b.centre.x.abs().max(b.centre.y.abs()))).map(|b| b.floors as f32).collect();
            v.iter().sum::<f32>() / v.len() as f32
        };
        let (inner, outer) = (mean(&|d| d < BLOCK), mean(&|d| d > 2.0 * BLOCK));
        assert!(inner > outer + 2.0, "downtown {inner} vs edge {outer} storeys");
    }

    #[test]
    fn farms_sit_on_the_ground() {
        let n = net();
        let l = layout(&n);
        let farms: Vec<&Building> = l.buildings.iter().filter(|b| b.use_ == Use::FarmHouse || b.use_ == Use::Barn).collect();
        eprintln!("farm buildings: {}", farms.len());
        assert!(!farms.is_empty());
        for b in farms {
            for c in b.corners() {
                let h = roads::height(&n, c.x, c.y);
                assert!(h >= b.base - 1e-3 && h <= b.ground + 1e-3, "ground {h} vs base {} floor {}", b.base, b.ground);
            }
        }
    }

    #[test]
    fn walls_are_solid() {
        let l = layout(&net());
        let obs = obstacles(&l);
        eprintln!("obstacles: {}", obs.len());
        assert!(obs.len() > 5_000 && obs.len() < 60_000, "{}", obs.len());
        let grid = ObstacleGrid::new(&obs, 10.0);
        assert_eq!(grid.len(), obs.len());
        let mut near = Vec::new();
        for b in &l.buildings {
            let top = b.top();
            // Every point 0.2 m behind the wall face (away from the corners) is inside a cylinder
            // (between two, the face itself is up to ~0.16 m uncovered), and no cylinder sticks
            // out more than 0.2 m.
            let h = b.half - Vec2::splat(0.2);
            for (a, e) in [(Vec2::new(-h.x, -h.y), Vec2::new(h.x, -h.y)), (Vec2::new(h.x, -h.y), Vec2::new(h.x, h.y)), (Vec2::new(h.x, h.y), Vec2::new(-h.x, h.y)), (Vec2::new(-h.x, h.y), Vec2::new(-h.x, -h.y))] {
                let len = (e - a).length();
                let mut s = 0.5;
                while s < len - 0.5 {
                    let p = b.at(a + (e - a) * (s / len));
                    near.clear();
                    grid.query(p, 0.0, &mut near);
                    assert!(near.iter().any(|o| o.top >= top - 1e-3), "gap in the wall of {b:?} at {p}");
                    s += 0.37;
                }
            }
        }
        for o in &obs {
            let owner = l.buildings.iter().find(|b| b.contains(o.pos, 0.0)).expect("cylinder inside a building");
            let d = o.pos - owner.centre;
            let lx = d.dot(owner.axis).abs() + o.radius;
            let lz = d.dot(owner.axis.perp()).abs() + o.radius;
            assert!(lx <= owner.half.x + 0.2 && lz <= owner.half.y + 0.2, "cylinder at {} sticks out of {owner:?}", o.pos);
        }
        // Streets stay clear.
        near.clear();
        grid.query(Vec2::new(0.0, 0.0), 6.0, &mut near);
        assert!(near.is_empty());
        grid.query(Vec2::new(-TOWN + 40.0, 0.0), 6.0, &mut near);
        assert!(near.is_empty(), "spawn point is clear");
    }

    /// Flat town ground with the buildings' wall cylinders.
    struct Town(f32, ObstacleGrid);
    impl crate::vehicle::Ground for Town {
        fn height(&self, _: f32, _: f32) -> f32 {
            self.0
        }
        fn normal(&self, _: f32, _: f32) -> Vec3 {
            Vec3::Y
        }
        fn surface(&self, _: f32, _: f32) -> crate::stage::Surface {
            crate::stage::Surface::Tarmac
        }
        fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
            out.clear();
            self.1.query(p, r, out);
        }
    }

    #[test]
    fn the_car_stops_at_the_wall() {
        use crate::vehicle::{Car, Input, Spec, step};
        let n = net();
        let l = layout(&n);
        let g = Town(n.town_y, ObstacleGrid::new(&obstacles(&l), 10.0));
        let spec = Spec::default();
        // Head on and at 35° into a shop facing −z (the car at heading 0 drives towards +z),
        // from the middle of the street in front of it.
        let b = l.buildings.iter().find(|b| b.use_ == Use::Shop && b.front == 3 && b.half.x > 5.0).unwrap();
        let street_z = b.centre.y - b.half.y - SIDEWALK - RoadKind::Street.width() * 0.5;
        for (kmh, heading) in [(60.0f32, 0.0f32), (110.0, 0.0), (90.0, 0.6), (90.0, -0.6)] {
            let start = Vec3::new(b.centre.x - libm::sinf(heading) * 20.0, n.town_y + 0.6, street_z - 12.0);
            let mut car = Car::new(start, heading);
            car.vel = car.forward() * kmh / 3.6;
            for w in &mut car.wheels {
                w.spin = kmh / 3.6 / spec.wheel_radius;
            }
            let mut deepest = f32::MIN;
            for _ in 0..240 {
                step(&mut car, &spec, Input { throttle: 1.0, ..Default::default() }, &g, 1.0 / 60.0);
                deepest = deepest.max(car.pos.z - (b.centre.y - b.half.y));
            }
            assert!(car.impacts > 0.0, "{kmh} km/h at {heading}: no hit");
            // The car's centre stays a good metre outside the wall (it's ~2 m to its nose).
            assert!(deepest < -1.2, "{kmh} km/h at {heading}: centre {deepest:.2} m from the wall");
        }
    }

    #[test]
    fn mesh_budget() {
        let l = layout(&net());
        let (meshes, tris, verts) = render::triangles(&l);
        eprintln!("meshes {meshes}, triangles {tris}, vertices {verts}");
        assert!((30..=80).contains(&meshes));
        assert!(tris < 600_000, "{tris} triangles");
    }
}

#[cfg(test)]
mod map {
    use super::*;

    /// `PACENOTE_TOWN_SVG=town.svg cargo test --lib open::buildings::map -- --ignored`: the town
    /// from above: streets, sidewalks, lots by kind, footprints by use, wall cylinders.
    #[test]
    #[ignore]
    fn draw() {
        let Ok(out) = std::env::var("PACENOTE_TOWN_SVG") else { return };
        let net = roads::generate(crate::open::SEED);
        let l = layout(&net);
        let v = 900.0;
        let mut s = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{} {} {} {}" width="1400" height="1400"><rect x="{}" y="{}" width="{}" height="{}" fill="#3d5236"/>"##, -v, -v, 2.0 * v, 2.0 * v, -v, -v, 2.0 * v, 2.0 * v);
        for r in &net.roads {
            let d: Vec<String> = r.road.pts.iter().map(|p| format!("{:.1},{:.1}", p.x, p.z)).collect();
            s += &format!(r##"<polyline fill="none" stroke="#bdb8ad" stroke-width="{}" points="{}"/>"##, r.road.width[0] + 2.0 * SIDEWALK, d.join(" "));
            s += &format!(r##"<polyline fill="none" stroke="#2c2c2e" stroke-width="{}" points="{}"/>"##, r.road.width[0], d.join(" "));
        }
        for b in &l.blocks {
            let fill = match b.kind {
                LotKind::Park => "#4f7a3f",
                LotKind::Parking => "#3a3a3c",
                LotKind::Plaza | LotKind::Tower => "#cfc6b0",
                LotKind::Farm => continue,
                _ => "#56704a",
            };
            s += &format!(r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{fill}"/>"##, b.min.x, b.min.y, b.max.x - b.min.x, b.max.y - b.min.y);
        }
        for b in &l.buildings {
            let fill = match b.use_ {
                Use::Downtown => "#8c8fa8",
                Use::Tower => "#5a5f80",
                Use::Shop => "#d9a066",
                Use::House => "#e0d0b0",
                Use::Apartment => "#a07f9f",
                Use::FarmHouse => "#f0e0c0",
                Use::Barn => "#a0402c",
            };
            let pts: Vec<String> = b.corners().iter().map(|c| format!("{:.2},{:.2}", c.x, c.y)).collect();
            s += &format!(r##"<polygon points="{}" fill="{fill}" stroke="#202020" stroke-width="0.3"><title>{:?} {} floors {:?}</title></polygon>"##, pts.join(" "), b.use_, b.floors, b.roof);
        }
        for o in obstacles(&l) {
            s += &format!(r##"<circle cx="{:.2}" cy="{:.2}" r="{:.2}" fill="none" stroke="#ff3030" stroke-width="0.12"/>"##, o.pos.x, o.pos.y, o.radius);
        }
        s += "</svg>";
        std::fs::write(out, s).unwrap();
    }
}
