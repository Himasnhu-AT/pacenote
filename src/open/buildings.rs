//! Town buildings: lots laid out in the street grid's blocks (rows of shops and houses, apartment
//! slabs, a tower, a plaza, parks and car parks) and a few farms along the country roads near
//! town. Three parts:
//!
//! - [`layout`]: pure and deterministic from the road network (its seed), like `stage.rs`: the
//!   same bits on every machine (no platform trig, no hash-map order).
//! - [`walls`]: each facade as one straight wall (`vehicle::Wall`) for the car sim, so a car
//!   brushing a building scrapes along it.
//! - [`spawn`]: one merged mesh per block (walls, roofs, sidewalks, lot paving) with a single
//!   material: a procedural facade atlas (windows, shopfronts, doors, planks, roof tiles,
//!   paving) tinted by vertex colour. Nothing spawns in headless runs.

use bevy::log::warn;
use bevy::math::{Vec2, Vec3};

use super::land;
use super::roads::{self, RoadKind, RoadNet};
use crate::stage::{Rng, STEP};
use crate::vehicle::Wall;

/// Sidewalk between the street's edge and the lots (m).
pub const SIDEWALK: f32 = 3.0;
/// Storey height (m) of town buildings; houses use [`HOUSE_FLOOR`].
pub const FLOOR: f32 = 3.2;
pub const HOUSE_FLOOR: f32 = 2.9;
/// Parapet on flat roofs (height above the roof, thickness).
pub const PARAPET: (f32, f32) = (0.9, 0.25);
/// Wall collision: each facade is a segment this far inside the face, grown by as much, so
/// the car stops at the plaster. (The corners are rounded by it: ~0.2 m off the corner.)
pub const WALL_R: f32 = 0.5;

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
    /// Terraced houses: narrow units joined in rows, 2-4 storeys, pitched roofs.
    Terrace,
    /// The church (with its tower), the town hall, the square's fountain, the mill (`bits & 7`:
    /// 1, 2, 3, 4).
    Civic,
    /// A timber hut in the mountains: one storey, a gable.
    Hut,
}

impl Use {
    pub fn label(self) -> &'static str {
        match self {
            Use::Downtown => "office",
            Use::Shop => "shop",
            Use::House => "house",
            Use::Apartment => "apartments",
            Use::Tower => "tower",
            Use::FarmHouse => "farmhouse",
            Use::Barn => "barn",
            Use::Terrace => "terrace",
            Use::Civic => "civic",
            Use::Hut => "hut",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, bevy::reflect::Reflect)]
pub enum Roof {
    /// Flat with a parapet.
    #[default]
    Flat,
    /// Two slopes; the ridge runs parallel to the front.
    Gable,
    /// Four slopes; the ridge runs along the longer side.
    Hip,
}

/// One building: an oriented box footprint, storeys and a roof.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize, bevy::reflect::Reflect)]
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


fn bits(rng: &mut Rng) -> u32 {
    (rng.f() * 16_777_216.0) as u32
}

/// Storeys and roof for a building of `use_` in a block whose centre is `ring` (0 = downtown,
/// 1 = middle, 2 = outer) from the centre.
fn dress(rng: &mut Rng, use_: Use, ring: u32) -> (u32, f32, Roof, u8, u8) {
    let walls_town: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7, 9];
    match use_ {
        Use::Downtown => (rng.range(5.0, 8.99) as u32, FLOOR, Roof::Flat, pick(rng, &[3, 8, 9, 0, 2, 5, 4]), 3),
        Use::Tower => (8, FLOOR, Roof::Flat, pick(rng, &[8, 3, 4]), 3),
        Use::Shop => {
            let lo = if ring == 0 { 3.0 } else { 2.0 };
            let roof = if rng.chance(0.25) { Roof::Gable } else { Roof::Flat };
            (rng.range(lo, lo + 1.99) as u32, FLOOR, roof, pick(rng, walls_town), if roof == Roof::Flat { 3 } else { pick(rng, &[0, 1, 2]) })
        }
        Use::Apartment => (rng.range(3.0, 4.99) as u32, FLOOR, Roof::Flat, pick(rng, &[0, 3, 5, 9, 4, 8]), 3),
        Use::House | Use::FarmHouse => {
            let roof = if rng.chance(0.55) { Roof::Gable } else { Roof::Hip };
            (2, HOUSE_FLOOR, roof, pick(rng, &[0, 1, 2, 4, 5, 6, 7, 9]), pick(rng, &[0, 0, 1, 2]))
        }
        Use::Barn => (1, 5.2, Roof::Gable, pick(rng, &[2, 2, 8]), pick(rng, &[1, 4, 3])),
        Use::Terrace => {
            let floors = match ring {
                0 => rng.range(3.0, 4.99) as u32,
                1 => rng.range(2.0, 3.99) as u32,
                _ => rng.range(2.0, 2.99) as u32,
            };
            let roof = if rng.chance(0.9) { Roof::Gable } else { Roof::Flat };
            (floors, 3.0, roof, pick(rng, &[0, 1, 4, 5, 7, 9, 2]), if roof == Roof::Flat { 3 } else { pick(rng, &[0, 0, 1, 2]) })
        }
        Use::Civic => (rng.range(2.0, 3.99) as u32, 4.2, Roof::Hip, pick(rng, &[9, 3, 5]), 1),
        Use::Hut => (1, 2.8, Roof::Gable, pick(rng, &[9, 8, 2]), pick(rng, &[1, 2])),
    }
}

#[allow(clippy::too_many_arguments)]
fn building(rng: &mut Rng, use_: Use, ring: u32, centre: Vec2, axis: Vec2, half: Vec2, front: u8, ground: f32, base: f32) -> Building {
    let (floors, floor_h, roof, wall, roof_col) = dress(rng, use_, ring);
    let pitch = match use_ {
        Use::Barn | Use::Hut => rng.range(0.55, 0.75),
        _ => rng.range(0.6, 0.85),
    };
    Building { centre, axis, half, front, floors, floor_h, ground, base, roof, pitch, use_, wall, roof_col, bits: bits(rng) }
}

/// Builds the layout for a road network (its seed and town height).
pub fn layout(net: &RoadNet) -> Layout {
    layout_with(net, &[])
}

/// A town block's kind set in the world data (`open::data::WorldData::lots`): block (`i`,
/// `j`) of the old 6×6 grid, west to east and north to south.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize, bevy::reflect::Reflect)]
pub struct LotData {
    pub i: u32,
    pub j: u32,
    pub kind: LotKind,
}

/// Lot kinds set in the world data. Fernhollow's blocks come from its plan (`open::town`),
/// so none are generated (kept for old level files: they're ignored with a warning).
pub fn town_lots(_seed: u64) -> Vec<LotData> {
    Vec::new()
}

/// Where the outskirts' hamlets are (nominal; each is centred on the nearest road): Westbrook
/// at the hillclimb's foot, Southford on the southern loop, Quarryside on the basin road.
pub const HAMLETS: [(&str, Vec2); 3] = [("Westbrook", Vec2::new(-1700.0, -1180.0)), ("Southford", Vec2::new(0.0, -1640.0)), ("Quarryside", Vec2::new(1300.0, -900.0))];

/// The buildings of the world (docs/festival/WORLD_PASS.md §3): Fernhollow's blocks (rows of
/// shops and terraces, apartments, a church, a town hall, a mill), its outskirts (houses along
/// the B-road and up the spine, three hamlets), sheds round the service park, farms on the low
/// roads and a few huts in the mountains. `lots` is ignored (the town's blocks are its plan's).
pub fn layout_with(net: &RoadNet, lots: &[LotData]) -> Layout {
    if !lots.is_empty() {
        warn!("{} town lot kinds in the world data: the town's blocks come from its plan; ignored", lots.len());
    }
    let mut blocks = Vec::new();
    let mut buildings = Vec::new();
    town(net, &mut blocks, &mut buildings);
    service_park(net, &mut blocks, &mut buildings);
    outskirts(net, &mut blocks, &mut buildings);
    farms(net, &mut blocks, &mut buildings);
    huts(net, &mut blocks, &mut buildings);
    Layout { seed: net.seed, town_y: net.town_y, blocks, buildings }
}

/// A footprint at `centre` (axis `axis`, half extents `half`, front side `front`) that fits:
/// on ground flat enough (under 2.5 m across it), `margin` m clear of every road's edge, clear
/// of the river, overlapping no building by more than `-gap` m. Its floor and plinth heights.
#[allow(clippy::too_many_arguments)]
fn fits(net: &RoadNet, buildings: &[Building], use_: Use, centre: Vec2, axis: Vec2, half: Vec2, front: u8, margin: f32, gap: f32) -> Option<(f32, f32)> {
    let probe = Building { centre, axis, half, front, floors: 1, floor_h: 3.0, ground: 0.0, base: 0.0, roof: Roof::Flat, pitch: 0.0, use_, wall: 0, roof_col: 0, bits: 0 };
    let corners = probe.corners();
    if corners.iter().any(|c| net.land.river_distance(*c) < 24.0) {
        return None;
    }
    let reach = half.length() + gap.max(0.0) + 1.0;
    if buildings.iter().any(|b| b.centre.distance(centre) < reach + b.half.length() && overlaps(b, &probe, gap)) {
        return None;
    }
    let hs: Vec<f32> = corners.iter().chain(std::iter::once(&centre)).map(|q| roads::height(net, q.x, q.y)).collect();
    let (lo, hi) = hs.iter().fold((f32::MAX, f32::MIN), |(a, b), &h| (a.min(h), b.max(h)));
    (hi - lo < 2.5 && clear_of_roads(net, &probe, margin)).then_some((hi + 0.15, lo - 0.4))
}

/// A building of `use_` and `half` size beside road station `p` (direction `dir`), on the
/// `out` side `reach` m from the centreline, if the ground is flat enough and nothing is in the
/// way. Its front faces the road.
#[allow(clippy::too_many_arguments)]
fn frontage(net: &RoadNet, rng: &mut Rng, buildings: &[Building], use_: Use, ring: u32, p: Vec2, dir: Vec2, out: Vec2, reach: f32, half: Vec2) -> Option<Building> {
    let c = p + out * (reach + half.y);
    let front = if dir.perp().dot(-out) > 0.0 { 1 } else { 3 };
    let (ground, base) = fits(net, buildings, use_, c, dir, half, front, 3.0, 2.5)?;
    Some(building(rng, use_, ring, c, dir, half, front, ground, base))
}

/// How far from the town's centre (the market square) a point is, as a ring: 0 within 200 m,
/// 1 within 420 m, 2 beyond.
fn ring_of(p: Vec2) -> u32 {
    let d = p.distance(Vec2::new(land::SPINE_X + 40.0, land::MAIN_Z + 40.0));
    if d < 200.0 { 0 } else if d < 420.0 { 1 } else { 2 }
}

/// What a row is made of: unit use, frontage and depth ranges (m), set back from the lot's
/// edge (m), row length before an alley (m).
#[derive(Clone, Copy)]
struct RowStyle {
    use_: Use,
    frontage: (f32, f32),
    depth: (f32, f32),
    setback: (f32, f32),
    row: (f32, f32),
    /// Chance the gap after a row is a 3-4 m alley, else a yard this wide (m).
    alley: f32,
    yard: (f32, f32),
}

/// Main Street: shops, 2-4 storeys, wide fronts, a near-continuous row.
const SHOPS: RowStyle = RowStyle { use_: Use::Shop, frontage: (9.0, 14.0), depth: (12.0, 16.0), setback: (0.0, 0.8), row: (40.0, 70.0), alley: 0.85, yard: (5.0, 10.0) };
/// The centre (within ~200 m of the square): tall terraces, wide fronts, rows broken only by
/// alleys.
const CENTRE_ROWS: RowStyle = RowStyle { use_: Use::Terrace, frontage: (8.0, 12.0), depth: (10.0, 13.0), setback: (0.0, 1.5), row: (35.0, 70.0), alley: 0.85, yard: (6.0, 12.0) };
/// The inner ring: 2-3 storey terraces in shorter rows.
const INNER_ROWS: RowStyle = RowStyle { use_: Use::Terrace, frontage: (7.5, 10.5), depth: (9.0, 12.0), setback: (0.5, 4.0), row: (25.0, 45.0), alley: 0.5, yard: (10.0, 18.0) };
const APARTMENTS: RowStyle = RowStyle { use_: Use::Apartment, frontage: (18.0, 30.0), depth: (12.0, 14.0), setback: (4.0, 6.0), row: (18.0, 30.0), alley: 0.0, yard: (8.0, 16.0) };
/// Towards the town's edge: semi-detached pairs with front gardens and a gap between pairs.
const SEMIS: RowStyle = RowStyle { use_: Use::House, frontage: (8.0, 10.0), depth: (8.5, 10.0), setback: (3.0, 6.0), row: (16.0, 20.0), alley: 0.0, yard: (7.0, 12.0) };

/// Fernhollow: every block of its plan (`open::town`), rows of buildings along the edges that
/// face a street, fronts on the sidewalk; the church, the town hall and the mill first.
fn town(net: &RoadNet, blocks: &mut Vec<Block>, buildings: &mut Vec<Building>) {
    let mut rng = Rng::new(net.seed ^ 0x7111_A6E5);
    for (bi, pb) in super::town::blocks().iter().enumerate() {
        let first = buildings.len();
        // One roof colour for most of a block (towns grow in phases): terracotta, slate,
        // brown.
        let roof = pick(&mut rng, &[0u8, 0, 0, 1, 1, 2]);
        match (pb.name, pb.kind) {
            ("B-W2", _) => civic(net, &mut rng, buildings, Vec2::new(-372.0, pb.min.y + 14.0), Vec2::X, Vec2::new(7.5, 14.0), 3, 1),
            ("A-E1", _) => civic(net, &mut rng, buildings, Vec2::new(pb.min.x + 8.0, pb.min.y + 22.0), Vec2::Y, Vec2::new(10.0, 8.0), 0, 2),
            ("riverside", _) => civic(net, &mut rng, buildings, Vec2::new(pb.min.x + 16.0, pb.max.y - 8.0), Vec2::X, Vec2::new(13.0, 7.5), 1, 4),
            ("square", _) => civic(net, &mut rng, buildings, (pb.min + pb.max) * 0.5, Vec2::X, Vec2::new(6.0, 6.0), 0, 3),
            _ => {}
        }
        if let LotKind::Rows(u) = pb.kind {
            // Long edges first: the corners go to them.
            let mut edges: Vec<usize> = (0..4).filter(|&e| pb.edges[e]).collect();
            let len = |e: usize| if e < 2 { pb.max.y - pb.min.y } else { pb.max.x - pb.min.x };
            edges.sort_by(|a, b| len(*b).total_cmp(&len(*a)).then(a.cmp(b)));
            for e in edges {
                // Main Street's frontage is shops; elsewhere rows by how central the block is:
                // tall terraces in the centre, lower ones round it, semis towards the edge.
                let main = e == 2 && (pb.min.y - (land::MAIN_Z + super::town::MAIN_HALF + SIDEWALK)).abs() < 0.5 || pb.name == "riverside";
                let rows = match ring_of((pb.min + pb.max) * 0.5) {
                    _ if pb.min.y > super::town::MILL.0 => SEMIS,
                    0 => CENTRE_ROWS,
                    _ => INNER_ROWS,
                };
                let style = match u {
                    Use::Shop if main => SHOPS,
                    Use::Apartment => APARTMENTS,
                    Use::House => SEMIS,
                    _ => rows,
                };
                row_edge(net, &mut rng, buildings, pb.min, pb.max, e, style, roof);
            }
        }
        blocks.push(Block { cell: Some((bi as u32, 0)), min: pb.min, max: pb.max, kind: pb.kind, buildings: (first..buildings.len()).collect() });
    }
}

/// A civic building (`kind`: 1 church, with its tower at the street end; 2 town hall; 3 the
/// square's fountain; 4 mill)
/// at `c`, its local x along `axis`, front side `front`.
#[allow(clippy::too_many_arguments)]
fn civic(net: &RoadNet, rng: &mut Rng, buildings: &mut Vec<Building>, c: Vec2, axis: Vec2, half: Vec2, front: u8, kind: u32) {
    if let Some((ground, base)) = fits(net, buildings, Use::Civic, c, axis, half, front, SIDEWALK - 0.02, 1.0) {
        let mut b = building(rng, Use::Civic, 0, c, axis, half, front, ground, base);
        b.bits = (b.bits & !7) | kind;
        if kind == 4 {
            // The mill: brick, a gable, three tall storeys.
            (b.wall, b.roof, b.roof_col, b.floors) = (2, Roof::Gable, 1, 3);
        }
        if kind == 3 {
            // The fountain (drawn as a basin and a column, `add_fountain`; one low storey here,
            // the least a level building keeps).
            (b.wall, b.roof, b.floors, b.floor_h) = (9, Roof::Flat, 1, 2.0);
        }
        buildings.push(b);
    }
}

/// A row of buildings along edge `e` (0 west, 1 east, 2 south, 3 north) of the lot
/// `min`..`max`, fronts facing out across the sidewalk to the street, in rows broken by alleys
/// and yards.
#[allow(clippy::too_many_arguments)]
fn row_edge(net: &RoadNet, rng: &mut Rng, buildings: &mut Vec<Building>, min: Vec2, max: Vec2, e: usize, style: RowStyle, roof: u8) {
    // Start of the edge, direction along it, and the outward normal (towards the street).
    let (start, d, n) = match e {
        0 => (Vec2::new(min.x, min.y), Vec2::Y, -Vec2::X),
        1 => (Vec2::new(max.x, min.y), Vec2::Y, Vec2::X),
        2 => (Vec2::new(min.x, min.y), Vec2::X, -Vec2::Y),
        _ => (Vec2::new(min.x, max.y), Vec2::X, Vec2::Y),
    };
    let len = if e < 2 { max.y - min.y } else { max.x - min.x };
    let front = if d.perp().dot(n) > 0.0 { 1 } else { 3 };
    let inside = |b: &Building| b.corners().iter().all(|c| c.x >= min.x - 0.01 && c.x <= max.x + 0.01 && c.y >= min.y - 0.01 && c.y <= max.y + 0.01);
    let mut t = 0.0;
    let mut last_wall = u8::MAX;
    while t < len - style.frontage.0 {
        let row_end = (t + rng.range(style.row.0, style.row.1)).min(len);
        let depth = rng.range(style.depth.0, style.depth.1);
        let setback = rng.range(style.setback.0, style.setback.1);
        let mut placed = 0;
        while t < row_end - style.frontage.0 * 0.9 {
            let mut w = rng.range(style.frontage.0, style.frontage.1);
            if t + w > row_end {
                w = row_end - t;
            }
            // Shops' depths vary unit by unit; a terrace's back line is the row's.
            let dep = if style.use_ == Use::Shop { rng.range(style.depth.0, style.depth.1) } else { depth };
            let half = Vec2::new(w * 0.5, dep * 0.5);
            let c = start + d * (t + w * 0.5) - n * (setback + dep * 0.5);
            let fit = fits(net, buildings, style.use_, c, d, half, front, SIDEWALK - 0.02, -0.005);
            match fit {
                Some((ground, base)) => {
                    let mut b = building(rng, style.use_, ring_of(c), c, d, half, front, ground, base);
                    if !inside(&b) {
                        t += 1.0;
                        continue;
                    }
                    // Most of a block under one roof colour; never two neighbours alike.
                    if b.roof != Roof::Flat && rng.chance(0.75) {
                        b.roof_col = roof;
                    }
                    while b.wall == last_wall {
                        b.wall = pick(rng, &[0u8, 1, 4, 5, 6, 7, 9, 3]);
                    }
                    last_wall = b.wall;
                    // A row's end house stands a storey taller now and then (a corner).
                    if placed == 0 && style.use_ != Use::House && rng.chance(0.4) {
                        b.floors = (b.floors + 1).min(4);
                    }
                    buildings.push(b);
                    placed += 1;
                    t += w;
                }
                None => t += 1.0,
            }
        }
        let _ = placed;
        last_wall = u8::MAX;
        t += if rng.chance(style.alley) { rng.range(3.0, 4.0) } else { rng.range(style.yard.0, style.yard.1) };
    }
}

/// Sheds round the service park: a row set back on the far side of the B-road.
fn service_park(net: &RoadNet, blocks: &mut Vec<Block>, buildings: &mut Vec<Building>) {
    let pl = net.places();
    let mut rng = Rng::new(net.seed ^ 0x5E7_1CE5);
    let first = buildings.len();
    let valley = &net.roads[0].road;
    if let Some(at) = valley.nearest(pl.park, 300.0) {
        for k in -4i32..=4 {
            let i = (at.i as i32 + k * 13).clamp(0, valley.len() as i32 - 1) as usize;
            let p = Vec2::new(valley.pts[i].x, valley.pts[i].z);
            let dir = valley.dir[i];
            let half = Vec2::new(rng.range(9.0, 13.0), rng.range(7.0, 10.0));
            let side = if pl.park.y > valley.pts[at.i].z { -1.0 } else { 1.0 };
            if let Some(b) = frontage(net, &mut rng, buildings, Use::Barn, 3, p, dir, dir.perp() * side, valley.width[i] * 0.5 + 14.0, half) {
                buildings.push(b);
            }
        }
    }
    group(blocks, buildings, first, LotKind::Parking);
}

/// Houses with gardens along `road` at the stations `keep` allows, one plot every 18-26 m on
/// each side (filled `chance` of the time), at most `max`.
#[allow(clippy::too_many_arguments)]
fn ribbon(net: &RoadNet, rng: &mut Rng, buildings: &mut Vec<Building>, road: usize, keep: &dyn Fn(Vec2) -> bool, chance: f32, max: usize) -> usize {
    let r = &net.roads[road].road;
    let near_junction = |p: Vec2| net.junctions.iter().any(|j| j.pos.distance(p) < 22.0);
    let mut made = 0;
    let mut i = 0;
    while i < r.len() && made < max {
        let p = Vec2::new(r.pts[i].x, r.pts[i].z);
        if !keep(p) || near_junction(p) {
            i += (8.0 / STEP) as usize;
            continue;
        }
        let dir = r.dir[i];
        for side in [1.0, -1.0] {
            if made >= max || !rng.chance(chance) {
                continue;
            }
            let half = Vec2::new(rng.range(4.0, 6.0), rng.range(4.0, 5.0));
            let reach = r.width[i] * 0.5 + rng.range(5.0, 9.0);
            if let Some(b) = frontage(net, rng, buildings, Use::House, 2, p, dir, dir.perp() * side, reach, half) {
                buildings.push(b);
                made += 1;
            }
        }
        i += (rng.range(18.0, 26.0) / STEP) as usize;
    }
    made
}

/// The outskirts: houses along the B-road west and east of the town, up the spine above
/// Upper Road, and three hamlets of 8-14 houses.
fn outskirts(net: &RoadNet, blocks: &mut Vec<Block>, buildings: &mut Vec<Building>) {
    let mut rng = Rng::new(net.seed ^ 0x0075_C127);
    let first = buildings.len();
    ribbon(net, &mut rng, buildings, 0, &|p| (-1300.0..super::town::WEST_X - 60.0).contains(&p.x), 0.55, 40);
    ribbon(net, &mut rng, buildings, 0, &|p| (super::town::EAST_X + 60.0..220.0).contains(&p.x), 0.7, 14);
    let top = super::town::UPPER.1;
    ribbon(net, &mut rng, buildings, 4, &|p| p.y > top + 40.0 && p.y < top + 420.0, 0.7, 26);
    group(blocks, buildings, first, LotKind::Rows(Use::House));
    for (_, at) in HAMLETS {
        let first = buildings.len();
        // Centred on the nearest road.
        let Some(c) = net.nearest(at, 400.0).map(|n| {
            let r = &net.roads[n.road].road;
            Vec2::new(r.pts[n.at.i].x, r.pts[n.at.i].z)
        }) else {
            continue;
        };
        let mut left = 14;
        for k in 0..net.roads.len() {
            if left == 0 {
                break;
            }
            left -= ribbon(net, &mut rng, buildings, k, &|p| p.distance(c) < 150.0, 0.85, left);
        }
        group(blocks, buildings, first, LotKind::Rows(Use::House));
    }
}

/// A hut (1 storey, timber) near station `s` (m) of `road`, the first spot that fits within
/// 60 m of it either side, 8-20 m off the road.
fn hut(net: &RoadNet, rng: &mut Rng, buildings: &mut Vec<Building>, road: usize, s: f32) -> bool {
    let Some(r) = net.roads.get(road).map(|r| &r.road) else { return false };
    for k in 0..20 {
        let off = (k / 2) as f32 * 6.0 * if k % 2 == 0 { 1.0 } else { -1.0 };
        let i = ((s + off) / STEP).clamp(5.0, r.len() as f32 - 6.0) as usize;
        let p = Vec2::new(r.pts[i].x, r.pts[i].z);
        let dir = r.dir[i];
        for side in [1.0, -1.0] {
            let half = Vec2::new(rng.range(2.0, 3.0), rng.range(2.5, 3.5));
            let reach = r.width[i] * 0.5 + rng.range(8.0, 20.0);
            if let Some(b) = frontage(net, rng, buildings, Use::Hut, 3, p, dir, dir.perp() * side, reach, half) {
                buildings.push(b);
                return true;
            }
        }
    }
    false
}

/// Huts in the mountains, only at places of interest: three at the logging camp, the radio
/// tower, a café at the top of the hillclimb, two shelters on the pass, the quarry office and
/// two forest cabins on the forestry road.
fn huts(net: &RoadNet, blocks: &mut Vec<Block>, buildings: &mut Vec<Building>) {
    let pl = net.places();
    let mut rng = Rng::new(net.seed ^ 0x40_7500);
    let at = |road: usize, p: Vec2| net.roads.get(road).and_then(|r| r.road.nearest(p, 600.0)).map_or(0.0, |n| n.s);
    let len = |road: usize| net.roads.get(road).map_or(0.0, |r| r.road.length());
    let spots = [
        (4, at(4, pl.camp)),
        (4, at(4, pl.camp) + 70.0),
        (5, 60.0),
        (3, at(3, pl.tower)),
        (1, len(1) - 60.0),
        (3, len(3) * 0.33),
        (3, len(3) * 0.66),
        (2, at(2, pl.quarry)),
        (4, len(4) * 0.45),
        (4, len(4) * 0.7),
    ];
    for (road, s) in spots {
        let first = buildings.len();
        hut(net, &mut rng, buildings, road, s);
        group(blocks, buildings, first, LotKind::Farm);
    }
}

/// A block (no lot drawn) holding `buildings[first..]`.
fn group(blocks: &mut Vec<Block>, buildings: &[Building], first: usize, kind: LotKind) {
    if first == buildings.len() {
        return;
    }
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for b in &buildings[first..] {
        for q in b.corners() {
            lo = lo.min(q);
            hi = hi.max(q);
        }
    }
    blocks.push(Block { cell: None, min: lo, max: hi, kind, buildings: (first..buildings.len()).collect() });
}

/// [`layout_with`], with `baked` buildings (a level's) in place of the generated ones when
/// there are any: the blocks keep their lots, each building goes to the block whose bounds hold
/// its centre (farms: the nearest farm block).
pub fn layout_baked(net: &RoadNet, lots: &[LotData], baked: &[Building]) -> Layout {
    let mut l = layout_with(net, lots);
    if baked.is_empty() {
        return l;
    }
    for blk in &mut l.blocks {
        blk.buildings.clear();
    }
    l.buildings = baked.to_vec();
    for (i, b) in l.buildings.iter().enumerate() {
        let d = |blk: &Block| {
            let c = b.centre.clamp(blk.min, blk.max);
            c.distance_squared(b.centre)
        };
        if let Some(blk) = l.blocks.iter_mut().min_by(|x, y| d(x).total_cmp(&d(y))) {
            blk.buildings.push(i);
        }
    }
    l
}

/// Farms: a farmhouse (and usually a barn behind it) every so often along the gravel roads
/// below the plateau and the B-road out in the valley, away from the town, the park and the
/// hamlets, on ground flat enough, clear of every road.
fn farms(net: &RoadNet, blocks: &mut Vec<Block>, buildings: &mut Vec<Building>) {
    let mut rng = Rng::new(net.seed ^ 0xFA53);
    let pl = net.places();
    let every = (170.0 / STEP) as usize;
    let away = |p: Vec2| {
        !super::town::in_core(p) && p.distance(super::land::TOWN_C) > 700.0 && p.distance(pl.park) > 260.0 && HAMLETS.iter().all(|(_, h)| p.distance(*h) > 300.0) && !(-1320.0..260.0).contains(&p.x)
    };
    for (k, r) in net.roads.iter().enumerate().filter(|(_, r)| matches!(r.kind, RoadKind::Gravel | RoadKind::BRoad)) {
        // The B-road: only the valley one (not the hillclimb).
        if r.kind == RoadKind::BRoad && k != 0 {
            continue;
        }
        let road = &r.road;
        let mut i = every / 2;
        while i < road.len() {
            let p = Vec2::new(road.pts[i].x, road.pts[i].z);
            let low = road.pts[i].y < 260.0 && (r.kind == RoadKind::Gravel && p.distance(super::land::TOWN_C) > 600.0 || away(p));
            if low && rng.chance(0.6) {
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
                for (u, c, half, front) in farm {
                    match fits(net, buildings, u, c, dir, half, front, 6.0, 3.0) {
                        Some((ground, base)) => {
                            let b = building(&mut rng, u, 3, c, dir, half, front, ground, base);
                            buildings.push(b);
                        }
                        // A barn that doesn't fit is dropped; so is a farm without its house.
                        None if u == Use::FarmHouse => break,
                        None => {}
                    }
                }
                group(blocks, buildings, first, LotKind::Farm);
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

/// The facades as solid walls: one segment per side, [`WALL_R`] inside the face and grown
/// by it, up to the roof's top. A footprint thinner than that is one segment down its middle.
/// Facades in line (a row of shops) are joined into one wall: apart, their round ends would
/// leave a notch at every party wall for a scraping car to catch on.
pub fn walls(layout: &Layout) -> Vec<Wall> {
    let mut out = Vec::new();
    for b in &layout.buildings {
        let top = b.top();
        let r = WALL_R.min(b.half.min_element());
        let h = b.half - Vec2::splat(r);
        let c = [Vec2::new(-h.x, -h.y), Vec2::new(h.x, -h.y), Vec2::new(h.x, h.y), Vec2::new(-h.x, h.y)];
        let first = out.len();
        for k in 0..4 {
            let (a, e) = (b.at(c[k]), b.at(c[(k + 1) % 4]));
            // (A thin footprint's sides repeat each other: one is enough.)
            if !(a == e && k > 0 || out[first..].iter().any(|w: &Wall| (w.a, w.b) == (a, e) || (w.a, w.b) == (e, a))) {
                out.push(Wall::new(a, e, r, top));
            }
        }
    }
    join_in_line(out)
}

/// A wall's line (direction angle and offset, quantised), its start and end along it, and its
/// direction.
type Line = ((i64, i64), f32, f32, Vec2);

/// Joins walls on one line (to a mrad and 1 cm) whose ends meet (within their two radii and a
/// few cm): sorted by line then position along it, so the result is the same on every machine.
fn join_in_line(walls: Vec<Wall>) -> Vec<Wall> {
    // Each wall as (line: direction angle in [0, π) and offset, quantised; start, end along
    // it; direction).
    let line = |w: &Wall| -> Line {
        let d = (w.b - w.a).normalize_or_zero();
        let d = if d.x < 0.0 || d.x == 0.0 && d.y < 0.0 { -d } else { d };
        let (s, e) = (w.a.dot(d), w.b.dot(d));
        let key = ((libm::atan2f(d.y, d.x) * 1000.0).round() as i64, (d.perp().dot(w.a) * 100.0).round() as i64);
        (key, s.min(e), s.max(e), d)
    };
    let mut keyed: Vec<_> = walls.iter().filter(|w| w.a != w.b).map(|w| (line(w), *w)).collect();
    keyed.sort_by(|(a, _), (b, _)| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.total_cmp(&b.2)));
    let mut out: Vec<Wall> = walls.iter().filter(|w| w.a == w.b).copied().collect();
    let mut run: Option<(Line, Wall)> = None;
    for (l, w) in keyed {
        if let Some((rl, rw)) = &mut run
            && l.0 == rl.0
            && (l.1 - rl.2) <= rw.radius + w.radius + 0.05
            && (w.radius - rw.radius).abs() < 1e-3
        {
            if l.2 > rl.2 {
                rl.2 = l.2;
                // The far end, on the run's line.
                rw.b = rw.a + rl.3 * (rl.2 - rl.1);
            }
            rw.top = rw.top.max(w.top);
            continue;
        }
        if let Some((_, rw)) = run.take() {
            out.push(rw);
        }
        // Start a run along the canonical direction.
        let (a, b) = (on_line(&w, l.3, l.1), on_line(&w, l.3, l.2));
        run = Some((l, Wall { a, b, ..w }));
    }
    if let Some((_, rw)) = run {
        out.push(rw);
    }
    out
}

/// The point of `w`'s line at `t` along `d`.
fn on_line(w: &Wall, d: Vec2, t: f32) -> Vec2 {
    w.a + d * (t - w.a.dot(d))
}

// ---------------------------------------------------------------- rendering

pub use render::{BuildingTile, building_mesh, material, spawn, tile_of};

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
            Use::Terrace => {
                // A door on the ground floor's first bay, windows over it; the odd blind bay.
                if j == 0 && front && i == 0 {
                    Tile::HouseDoor
                } else if side_wall || (!front && hash(b.bits, i + 3, j) < 0.3) {
                    Tile::Plain
                } else if j > 0 && b.bits & 16 != 0 && hash(b.bits, i, j) < 0.2 {
                    Tile::WinBalcony
                } else {
                    Tile::WinHouse
                }
            }
            Use::Civic => {
                if j == 0 && front && mid {
                    Tile::ShopDoor
                } else if b.bits & 7 == 4 {
                    // The mill: small windows in brick.
                    if hash(b.bits, i, j) < 0.5 { Tile::WinHouse } else { Tile::Plain }
                } else {
                    Tile::WinOffice
                }
            }
            Use::Hut => {
                if front && mid && j == 0 {
                    Tile::HouseDoor
                } else if front || hash(b.bits, i, j) < 0.3 {
                    Tile::WinHouse
                } else {
                    Tile::Planks
                }
            }
        }
    }

    /// Adds a building's walls, roof and details (and the church's tower).
    pub(super) fn add_building(buf: &mut Buf, b: &Building) {
        if b.use_ == Use::Civic && b.bits & 7 == 3 {
            add_fountain(buf, b);
            return;
        }
        add_body(buf, b);
        if b.use_ == Use::Civic && b.bits & 7 == 1 {
            add_tower(buf, b);
        }
    }

    fn add_body(buf: &mut Buf, b: &Building) {
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
            Use::Civic => 3.6,
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

    /// The church's tower: a square stone tower at the street end of the nave, its top well
    /// over the roof, with a steep pyramid roof.
    pub(super) fn add_tower(buf: &mut Buf, b: &Building) {
        let side = b.half.x.min(b.half.y) * 0.75;
        // The street end: the front face's middle, the tower just inside it.
        let n = match b.front {
            0 => b.axis,
            1 => b.axis.perp(),
            2 => -b.axis,
            _ => -b.axis.perp(),
        };
        let reach = if b.front.is_multiple_of(2) { b.half.x } else { b.half.y };
        let c = b.centre + n * (reach - side * 0.5);
        let height = b.top() - b.ground + 9.0;
        let t = Building { centre: c, half: Vec2::splat(side * 0.5), floors: 1, floor_h: height, roof: Roof::Hip, pitch: 1.8, bits: b.bits & !7, ..*b };
        add_building(buf, &t);
    }

    /// The square's fountain: a stone basin with water in it, a column and its cap.
    fn add_fountain(buf: &mut Buf, b: &Building) {
        let stone = [0.78, 0.75, 0.68];
        let block = |buf: &mut Buf, half: Vec2, y0: f32, h: f32| {
            let part = Building { half, ground: y0, base: y0, floor_h: h, floors: 1, roof: Roof::Flat, ..*b };
            roof_box(buf, &part, stone);
        };
        // A stone basin 0.7 m high.
        let top = b.ground + 0.7;
        block(buf, b.half, b.base, top - b.base);
        // The water, a hand's width under the rim.
        let w = b.half - Vec2::splat(0.45);
        let water = [b.at(Vec2::new(-w.x, -w.y)), b.at(Vec2::new(w.x, -w.y)), b.at(Vec2::new(w.x, w.y)), b.at(Vec2::new(-w.x, w.y))];
        buf.ground(top + 0.01, &water, Vec2::splat(4.0), |_, _| (Tile::Plain, lin([0.32, 0.46, 0.55], 1.0)));
        block(buf, Vec2::splat(1.6), top, 0.5);
        block(buf, Vec2::splat(0.7), top + 0.5, 3.0);
        block(buf, Vec2::splat(1.1), top + 3.5, 0.35);
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
        // Sidewalks sit up level with the kerb (`art::junctions`) on the ground (the town slopes
        // gently up from Main Street), their outer corners rounded to the kerb's fillet at the
        // junctions.
        let walk = |x: f32, z: f32| height(x, z) + crate::open::junctions::SIDEWALK_TOP - lift;
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
                buf.draped(&walk, lift - 0.01, &rect(lo, hi), Vec2::splat(2.0), |i, j| (Tile::Paving, lin([0.9, 0.86, 0.78], 0.93 + 0.07 * hash(7, i, j))));
                // Benches round the middle (the fountain), facing it.
                let c = (lo + hi) * 0.5;
                for k in 0..8 {
                    let a = k as f32 * std::f32::consts::TAU / 8.0 + 0.39;
                    let out = Vec2::new(libm::cosf(a), libm::sinf(a));
                    let p = c + out * 11.0;
                    let y = walk(p.x, p.y) + lift;
                    let seat = Building { centre: p, axis: out.perp(), half: Vec2::new(0.9, 0.25), front: 1, floors: 1, floor_h: 0.45, ground: y, base: y, roof: Roof::Flat, pitch: 0.0, use_: Use::Civic, wall: 0, roof_col: 0, bits: 0 };
                    roof_box(buf, &seat, [0.45, 0.32, 0.22]);
                    let back = Building { centre: p + out * 0.22, half: Vec2::new(0.9, 0.05), floor_h: 0.9, ..seat };
                    roof_box(buf, &back, [0.45, 0.32, 0.22]);
                }
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

    /// The shared building material (the facade / roof atlas), made once.
    #[derive(Resource, Clone)]
    struct BuildingMaterial(Handle<StandardMaterial>);

    pub fn material(world: &mut World) -> Handle<StandardMaterial> {
        if let Some(m) = world.get_resource::<BuildingMaterial>() {
            return m.0.clone();
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
        world.insert_resource(BuildingMaterial(mat.clone()));
        mat
    }

    /// One building's mesh, its origin at the footprint centre on the floor (`b.ground`). The
    /// plinth reaches down to the lowest `height` under the footprint.
    pub fn building_mesh(b: &Building, height: &dyn Fn(f32, f32) -> f32) -> Mesh {
        let mut b = *b;
        let low = b.corners().iter().chain(std::iter::once(&b.centre)).map(|p| height(p.x, p.y)).fold(b.ground, f32::min);
        b.base = low.min(b.ground);
        let origin = Vec3::new(b.centre.x, b.ground, b.centre.y);
        let mut buf = Buf { origin, ..default() };
        add_building(&mut buf, &b);
        buf.into_mesh()
    }

    /// The town drawn in tiles: one merged mesh of every building and lot (sidewalks, paving)
    /// whose centre is in it (WORLD_PASS §3, TECH_PASS §4: a draw per tile, not per building).
    /// In the editor the level's buildings draw themselves one by one (`level::sync_buildings`)
    /// and the tiles hide.
    #[derive(Component, Clone, Copy, Debug)]
    pub struct BuildingTile {
        pub key: (i32, i32),
    }

    /// Tile side (m) in the town's core and out in the country (fewer, emptier tiles there).
    pub const TILE: f32 = 128.0;
    pub const COUNTRY_TILE: f32 = 512.0;

    /// The tile a point falls in: 128 m cells in the core, 512 m cells (keyed apart) outside.
    pub fn tile_of(p: Vec2) -> (i32, i32) {
        if crate::open::town::in_core(p) {
            ((p.x / TILE).floor() as i32, (p.y / TILE).floor() as i32)
        } else {
            (10_000 + (p.x / COUNTRY_TILE).floor() as i32, (p.y / COUNTRY_TILE).floor() as i32)
        }
    }

    /// Every tile's geometry, sorted by key (lots first, then buildings, each in layout order),
    /// local to the tile's origin. `height` is the rendered terrain's height.
    pub(super) fn tiles(layout: &Layout, height: &dyn Fn(f32, f32) -> f32) -> Vec<((i32, i32), Vec3, Buf)> {
        let mut keys: Vec<(i32, i32)> = layout.blocks.iter().filter(|b| b.cell.is_some()).map(|b| tile_of(b.centre())).chain(layout.buildings.iter().map(|b| tile_of(b.centre))).collect();
        keys.sort_unstable();
        keys.dedup();
        let mut out = Vec::new();
        for key in keys {
            // The tile's origin: the first thing in it, at the town's height (positions stay
            // small in f32).
            let first = layout.blocks.iter().filter(|b| b.cell.is_some()).map(|b| b.centre()).chain(layout.buildings.iter().map(|b| b.centre)).find(|c| tile_of(*c) == key).unwrap_or_default();
            let origin = Vec3::new(first.x, layout.town_y, first.y);
            let mut buf = Buf { origin, ..default() };
            for blk in layout.blocks.iter().filter(|b| b.cell.is_some() && tile_of(b.centre()) == key) {
                add_lot(&mut buf, blk, height);
            }
            for b in layout.buildings.iter().filter(|b| tile_of(b.centre) == key) {
                // The plinth reaches down to the ground drawn under it.
                let mut b = *b;
                let low = b.corners().iter().chain(std::iter::once(&b.centre)).map(|p| height(p.x, p.y)).fold(b.ground, f32::min);
                b.base = low.min(b.ground);
                add_building(&mut buf, &b);
            }
            if !buf.idx.is_empty() {
                out.push((key, origin, buf));
            }
        }
        out
    }

    /// Spawns the town as merged tiles ([`BuildingTile`]), all sharing one material: static
    /// scenery kept at full detail (seen from the hills). `height` is the rendered terrain's
    /// height (`OpenWorld::heights`). Returns the entities (for despawning). Nothing in headless
    /// runs.
    pub fn spawn(world: &mut World, layout: &Layout, height: &dyn Fn(f32, f32) -> f32) -> Vec<Entity> {
        if !crate::art::visual(world) {
            return Vec::new();
        }
        let mat = material(world);
        let editing = crate::open::level::editing(world);
        let mut out = Vec::new();
        for (key, origin, buf) in tiles(layout, height) {
            let mesh = buf.into_mesh();
            let aabb = bevy::camera::primitives::MeshAabb::get_aabb(&mesh);
            let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
            let vis = if editing { Visibility::Hidden } else { Visibility::Inherited };
            let mut e = world.spawn((Name::new(format!("town tile {} {}", key.0, key.1)), Mesh3d(h), MeshMaterial3d(mat.clone()), Transform::from_translation(origin), vis, Scenery, KeepDetail, BuildingTile { key }, engine::level::GeneratedBy("pacenote::buildings (merged tiles)")));
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
        for (_, _, b) in tiles(layout, &|_, _| layout.town_y) {
            tris += b.idx.len() / 3;
            verts += b.pos.len();
            meshes += 1;
        }
        (meshes, tris, verts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open::SEED;
    use crate::open::walls::WallGrid;

    fn net() -> RoadNet {
        roads::generate(SEED)
    }

    #[test]
    fn deterministic() {
        let n = net();
        let (a, b) = (layout(&n), layout(&n));
        assert_eq!(a.buildings, b.buildings);
        assert_eq!(walls(&a), walls(&b));
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
        assert!((560..=800).contains(&a.buildings.len()), "{}", a.buildings.len());
        for u in [Use::Shop, Use::House, Use::Barn, Use::Terrace, Use::Apartment, Use::Civic, Use::Hut] {
            assert!(count(u) > 0, "{u:?}");
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

    /// WORLD_PASS §3's counts: a dense core, sparse outskirts and hamlets, farms on the low
    /// roads, rare huts in the mountains; every building on its ground.
    #[test]
    fn fernhollow_and_its_country() {
        let n = net();
        let l = layout(&n);
        let farm = |b: &&Building| matches!(b.use_, Use::FarmHouse | Use::Barn);
        let core = l.buildings.iter().filter(|b| super::super::town::in_core(b.centre) && !farm(b)).count();
        let outskirts = l.buildings.iter().filter(|b| !super::super::town::in_core(b.centre) && b.use_ == Use::House).count();
        let steadings = l.buildings.iter().filter(|b| b.use_ == Use::FarmHouse).count();
        let huts = l.buildings.iter().filter(|b| b.use_ == Use::Hut).count();
        let civic = l.buildings.iter().filter(|b| b.use_ == Use::Civic).count();
        let high = l.buildings.iter().filter(|b| b.use_ == Use::Hut && b.ground > 250.0).count();
        eprintln!("core {core}, outskirts and hamlets {outskirts}, farm steadings {steadings}, huts {huts} ({high} above 250 m), civic {civic}, all {}", l.buildings.len());
        assert!((380..=520).contains(&core), "{core} buildings in the core");
        assert!((80..=130).contains(&outskirts), "{outskirts} in the outskirts and hamlets");
        assert!((25..=35).contains(&steadings), "{steadings} farm steadings");
        assert!((8..=12).contains(&huts), "{huts} huts");
        assert_eq!(civic, 4, "church, town hall, fountain, mill");
        // Nothing else lives up in the mountains.
        let others_high = l.buildings.iter().filter(|b| b.use_ != Use::Hut && b.ground > 250.0).count();
        assert!(others_high <= 2, "{others_high} buildings other than huts above 250 m");
        for b in &l.buildings {
            assert!((1..=4).contains(&b.floors), "{b:?}");
            for c in b.corners() {
                let h = roads::height(&n, c.x, c.y);
                assert!(h >= b.base - 1e-3 && h <= b.ground + 1e-3, "ground {h} vs base {} floor {}", b.base, b.ground);
            }
        }
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
        let ws = walls(&l);
        // Four sides a building, less the facades joined in rows.
        let joined = 4 * l.buildings.len() - ws.len();
        eprintln!("walls: {} for {} buildings ({joined} facades joined onto a neighbour's)", ws.len(), l.buildings.len());
        // (Terraces join their fronts and backs into one wall a row.)
        assert!(ws.len() >= 2 * l.buildings.len(), "{}", ws.len());
        let grid = WallGrid::new(&ws, 10.0);
        assert_eq!(grid.len(), ws.len());
        let mut near = Vec::new();
        for b in &l.buildings {
            let top = b.top();
            // The whole face, to within a few cm of the plaster, is inside a wall up to the top
            // (corners: the round ends cut ~0.2 m off).
            let h = b.half - Vec2::splat(0.03);
            for (a, e) in [(Vec2::new(-h.x, -h.y), Vec2::new(h.x, -h.y)), (Vec2::new(h.x, -h.y), Vec2::new(h.x, h.y)), (Vec2::new(h.x, h.y), Vec2::new(-h.x, h.y)), (Vec2::new(-h.x, h.y), Vec2::new(-h.x, -h.y))] {
                let len = (e - a).length();
                let mut s = WALL_R.min(b.half.min_element());
                while s < len - WALL_R {
                    let p = b.at(a + (e - a) * (s / len));
                    near.clear();
                    grid.query(p, 0.0, &mut near);
                    assert!(near.iter().any(|w| w.top >= top - 1e-3), "gap in the wall of {b:?} at {p}");
                    s += 0.37;
                }
            }
            // Solid through the middle of each side: a point just outside it is within reach.
            near.clear();
            grid.query(b.at(Vec2::new(0.0, -b.half.y - 0.1)), 0.2, &mut near);
            assert!(!near.is_empty(), "{b:?} open in the middle");
        }
        // No wall sticks out of the buildings: a point 5 cm outside a face, away from other
        // buildings, is outside every wall.
        for b in &l.buildings {
            for l2 in [Vec2::new(0.0, -b.half.y - 0.05), Vec2::new(0.0, b.half.y + 0.05), Vec2::new(-b.half.x - 0.05, 0.0), Vec2::new(b.half.x + 0.05, 0.0)] {
                let p = b.at(l2);
                if l.buildings.iter().any(|o| !std::ptr::eq(o, b) && o.contains(p, 0.1)) {
                    continue;
                }
                near.clear();
                grid.query(p, 0.0, &mut near);
                assert!(near.is_empty(), "wall sticks out of {b:?} at {p}: {near:?}");
            }
        }
        // Streets stay clear.
        near.clear();
        let (spawn, _) = roads::spawn(&net());
        grid.query(Vec2::new(spawn.x, spawn.z), 6.0, &mut near);
        assert!(near.is_empty(), "spawn point is clear");
    }

    /// Flat town ground with the buildings' walls.
    struct Town(f32, WallGrid);
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
        fn obstacles(&self, _: Vec2, _: f32, out: &mut Vec<crate::vehicle::Obstacle>) {
            out.clear();
        }
        fn walls(&self, p: Vec2, r: f32, out: &mut Vec<Wall>) {
            out.clear();
            self.1.query(p, r, out);
        }
    }

    #[test]
    fn a_car_scrapes_along_a_facade() {
        use crate::vehicle::{Car, Input, Spec, step};
        let n = net();
        let l = layout(&n);
        let ws = walls(&l);
        let spec = Spec::default();
        // The longest joined facade with open street in front of it.
        let mut best: Option<(Wall, Vec2)> = None;
        for w in &ws {
            let d = (w.b - w.a).normalize_or_zero();
            let mid = (w.a + w.b) * 0.5;
            for side in [d.perp(), -d.perp()] {
                let clear = (0..=10).all(|k| {
                    let p = w.a.lerp(w.b, k as f32 / 10.0) + side * 4.0;
                    !l.buildings.iter().any(|b| b.contains(p, 1.5))
                });
                if clear && best.is_none_or(|(b, _)| w.a.distance(w.b) > b.a.distance(b.b)) && !l.buildings.iter().any(|b| b.contains(mid + side * (w.radius + 0.1), 0.0)) {
                    best = Some((*w, side));
                }
            }
        }
        let (w, out) = best.unwrap();
        let d = (w.b - w.a).normalize();
        eprintln!("facade {:.0} m", w.a.distance(w.b));
        assert!(w.a.distance(w.b) > 15.0);
        // Flat ground at the building's floor.
        let floor = l.buildings.iter().find(|b| b.contains((w.a + w.b) * 0.5, 1.0)).unwrap().ground;
        let g = Town(floor, WallGrid::new(&ws, 10.0));
        // 110 km/h at 4° toward it, from 1.5 m off the plaster, 5 m along it.
        let ang = 4f32.to_radians();
        let dir = d * libm::cosf(ang) - out * libm::sinf(ang);
        let start = w.a + d * 5.0 + out * (w.radius + 1.5);
        let heading = libm::atan2f(dir.x, dir.y);
        let mut car = Car::new(Vec3::new(start.x, floor + 0.6, start.y), heading);
        assert!(Vec2::new(car.forward().x, car.forward().z).dot(dir) > 0.999, "heading");
        let v = 110.0 / 3.6;
        car.vel = car.forward() * v;
        for wh in &mut car.wheels {
            wh.spin = v / spec.wheel_radius;
        }
        let (mut yaw, mut closest) = (0.0f32, f32::MAX);
        for _ in 0..60 {
            step(&mut car, &spec, Input::default(), &g, 1.0 / 60.0);
            yaw = yaw.max(car.ang.y.abs());
            closest = closest.min((Vec2::new(car.pos.x, car.pos.z) - w.a).dot(out) - w.radius);
        }
        let along = Vec2::new(car.vel.x, car.vel.z).normalize().dot(out);
        eprintln!("after 1 s: {:.1} km/h, yaw ≤ {yaw:.2} rad/s, centre came within {closest:.2} m of the plaster, drift {along:.3}", car.vel.length() * 3.6);
        // It touched (the centre is ~0.9 m in from the side), kept its speed and didn't spin
        // in, and is moving along the wall or away from it.
        assert!(closest < 0.95 && closest > 0.8, "{closest}");
        assert!(car.vel.length() > 0.85 * v && yaw < 0.8, "{} m/s, yaw {yaw}", car.vel.length());
        assert!(along > -0.01, "into the wall: {along}");
    }

    #[test]
    fn the_car_stops_at_the_wall() {
        use crate::vehicle::{Car, Input, Spec, step};
        let n = net();
        let l = layout(&n);
        let spec = Spec::default();
        // Head on and at 35° into the biggest shed's front, from 14 m out.
        let b = l.buildings.iter().filter(|b| b.use_ == Use::Barn).max_by(|x, y| x.half.x.total_cmp(&y.half.x)).unwrap();
        let g = Town(b.ground, WallGrid::new(&walls(&l), 10.0));
        let face = b.at(Vec2::new(0.0, if b.front == 1 { b.half.y } else { -b.half.y }));
        let out = (face - b.centre).normalize();
        let h0 = libm::atan2f(-out.x, -out.y);
        for (kmh, turn) in [(60.0f32, 0.0f32), (110.0, 0.0), (90.0, 0.6), (90.0, -0.6)] {
            let heading = h0 + turn;
            let fwd = Vec2::new(libm::sinf(heading), libm::cosf(heading));
            let start = face - fwd * 14.0;
            let mut car = Car::new(Vec3::new(start.x, b.ground + 0.6, start.y), heading);
            car.vel = car.forward() * kmh / 3.6;
            for w in &mut car.wheels {
                w.spin = kmh / 3.6 / spec.wheel_radius;
            }
            let mut deepest = f32::MIN;
            for _ in 0..240 {
                step(&mut car, &spec, Input { throttle: 1.0, ..Default::default() }, &g, 1.0 / 60.0);
                deepest = deepest.max((Vec2::new(car.pos.x, car.pos.z) - face).dot(-out));
            }
            assert!(car.impacts > 0.0, "{kmh} km/h at {turn}: no hit");
            // The car's centre stays a good metre outside the wall (it's ~2 m to its nose).
            assert!(deepest < -1.2, "{kmh} km/h at {turn}: centre {deepest:.2} m from the wall");
        }
    }

    #[test]
    fn mesh_budget() {
        let l = layout(&net());
        let (meshes, tris, verts) = render::triangles(&l);
        eprintln!("meshes {meshes} (tiles), triangles {tris}, vertices {verts}");
        // Merged per tile: a draw each, not one a building (TECH_PASS §4).
        assert!(meshes <= 60, "{meshes} meshes");
        assert!(tris < 600_000, "{tris} triangles");
    }
}

#[cfg(test)]
mod map {
    use super::*;

    /// `PACENOTE_TOWN_SVG=town.svg cargo test --lib open::buildings::map -- --ignored`: the town
    /// from above: streets, sidewalks, lots by kind, footprints by use, wall segments.
    #[test]
    #[ignore]
    fn draw() {
        let Ok(out) = std::env::var("PACENOTE_TOWN_SVG") else { return };
        let net = roads::generate(crate::open::SEED);
        let l = layout(&net);
        // Round the town (`PACENOTE_TOWN_VIEW=x,z,half` for elsewhere).
        let (cx, cz, v) = std::env::var("PACENOTE_TOWN_VIEW").ok().and_then(|s| {
            let p: Vec<f32> = s.split(',').filter_map(|x| x.parse().ok()).collect();
            (p.len() == 3).then(|| (p[0], p[1], p[2]))
        }).unwrap_or((-320.0, -1010.0, 450.0));
        let mut s = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{} {} {} {}" width="1400" height="1400"><rect x="{}" y="{}" width="{}" height="{}" fill="#3d5236"/>"##, cx - v, cz - v, 2.0 * v, 2.0 * v, cx - v, cz - v, 2.0 * v, 2.0 * v);
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
                Use::Terrace => "#e8b890",
                Use::Civic => "#f0f0f0",
                Use::Hut => "#806040",
            };
            let pts: Vec<String> = b.corners().iter().map(|c| format!("{:.2},{:.2}", c.x, c.y)).collect();
            s += &format!(r##"<polygon points="{}" fill="{fill}" stroke="#202020" stroke-width="0.3"><title>{:?} {} floors {:?}</title></polygon>"##, pts.join(" "), b.use_, b.floors, b.roof);
        }
        for w in walls(&l) {
            s += &format!(r##"<line x1="{:.2}" y1="{:.2}" x2="{:.2}" y2="{:.2}" stroke="#ff3030" stroke-opacity="0.5" stroke-linecap="round" stroke-width="{:.2}"/>"##, w.a.x, w.a.y, w.b.x, w.b.y, 2.0 * w.radius);
        }
        s += "</svg>";
        std::fs::write(out, s).unwrap();
    }
}
