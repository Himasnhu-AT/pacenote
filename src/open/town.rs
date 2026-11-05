//! Fernhollow's plan (docs/festival/WORLD_PASS.md §3): the street grid north of Main Street
//! (the B-road) and the blocks between the streets. The road builder lays the streets from it
//! (`roads::town_streets`) and the building layout fills the blocks (`buildings::town`), so
//! the two agree to the centimetre. Pure data: no randomness here.
//!
//! ```text
//!  z −860  ┌──── Upper Road E ────┐ (outskirts north)
//!  z −890 ─┼ Upper Road W ─┐      │
//!          │   C-W         │ C-E  │ car park
//!  z −960  │               ├ Mill E ┤
//!  z −990  ├── Mill W ─────┤ B-E  │
//!          │ B-W           │      │
//!  z −1040 │               ├ Church E ┤
//!  z −1070 ├── Church W ───┤ A-E  │
//!          │ A-W (3)       │ square, A-E (3)
//!  z −1140 ═══════ Main Street (B-road) ═══════
//!            riverside row     ~~ river ~~
//!        x −680         x −290 (spine)   x +40
//! ```

use bevy::math::Vec2;

use super::buildings::{LotKind, SIDEWALK, Use};
use super::land::{MAIN_Z, SPINE_X, TOWN_HI, TOWN_LO};

/// West Lane's and East Lane's x.
pub const WEST_X: f32 = TOWN_LO.x + 40.0;
pub const EAST_X: f32 = TOWN_HI.x - 40.0;
/// Each street's z, west half and east half (they meet the spine 30 m apart: T-junctions).
pub const CHURCH: (f32, f32) = (-1070.0, -1040.0);
pub const MILL: (f32, f32) = (-990.0, -960.0);
pub const UPPER: (f32, f32) = (-890.0, -860.0);
/// Cross lanes' x: Main Street to Church Street (west, east), Church Street to Mill Street.
pub const LANES_A: ([f32; 2], [f32; 2]) = ([-560.0, -420.0], [-160.0, -20.0]);
pub const LANES_B: (f32, f32) = (-490.0, -100.0);

/// Half widths (m): Main Street (the B-road), the spine where it runs through the town, a
/// street.
pub const MAIN_HALF: f32 = 3.25;
pub const SPINE_HALF: f32 = 3.0;
pub const STREET_HALF: f32 = 2.5;
/// The spine's width (m): a street from Main Street to the top of the town (`UPPER.1`), where
/// the forestry road starts.
pub const SPINE_W: f32 = 2.0 * SPINE_HALF;

/// One block of the plan: its lot (inside the sidewalks), what fills it and which of its
/// edges face a street with buildings (west, east, south, north).
#[derive(Clone, Copy, Debug)]
pub struct PlanBlock {
    pub name: &'static str,
    pub min: Vec2,
    pub max: Vec2,
    pub kind: LotKind,
    /// Buildings along the west (−x), east, south (−z) and north edges.
    pub edges: [bool; 4],
}

/// The lot between streets at `x0` (half width `hx0`) .. `x1` and `z0` .. `z1`.
fn lot(x0: (f32, f32), x1: (f32, f32), z0: (f32, f32), z1: (f32, f32)) -> (Vec2, Vec2) {
    (Vec2::new(x0.0 + x0.1 + SIDEWALK, z0.0 + z0.1 + SIDEWALK), Vec2::new(x1.0 - x1.1 - SIDEWALK, z1.0 - z1.1 - SIDEWALK))
}

/// Every block of the town, in a fixed order.
pub fn blocks() -> Vec<PlanBlock> {
    let (s, m, sp) = (STREET_HALF, MAIN_HALF, SPINE_HALF);
    let main = (MAIN_Z, m);
    let west = (WEST_X, s);
    let east = (EAST_X, s);
    let spine = (SPINE_X, sp);
    let st = |v: f32| (v, s);
    let mut out = Vec::new();
    let mut add = |name, (min, max): (Vec2, Vec2), kind, edges| out.push(PlanBlock { name, min, max, kind, edges });
    let terrace = LotKind::Rows(Use::Terrace);
    let shops = LotKind::Rows(Use::Shop);
    let all = [true; 4];
    // Between Main Street and Church Street: shops on Main Street, terraces behind.
    let (aw, ae) = LANES_A;
    add("A-W1", lot(west, st(aw[0]), main, st(CHURCH.0)), shops, all);
    add("A-W2", lot(st(aw[0]), st(aw[1]), main, st(CHURCH.0)), shops, all);
    add("A-W3", lot(st(aw[1]), spine, main, st(CHURCH.0)), shops, all);
    // The market square on Main Street by the spine, the town hall across a lane from it.
    let (sq_min, sq_max) = lot(spine, st(-224.0), main, st(CHURCH.1));
    add("square", (sq_min, sq_max + Vec2::new(SIDEWALK + STREET_HALF, 0.0)), LotKind::Plaza, [false; 4]);
    add("A-E1", lot((sq_max.x + SIDEWALK + STREET_HALF + 6.0, 0.0), st(ae[0]), main, st(CHURCH.1)), shops, all);
    add("A-E2", lot(st(ae[0]), st(ae[1]), main, st(CHURCH.1)), shops, all);
    add("A-E3", lot(st(ae[1]), east, main, st(CHURCH.1)), LotKind::Rows(Use::Apartment), all);
    // Church Street to Mill Street.
    add("B-W1", lot(west, st(LANES_B.0), st(CHURCH.0), st(MILL.0)), terrace, all);
    add("B-W2", lot(st(LANES_B.0), spine, st(CHURCH.0), st(MILL.0)), terrace, all);
    add("B-E1", lot(spine, st(LANES_B.1), st(CHURCH.1), st(MILL.1)), terrace, all);
    add("B-E2", lot(st(LANES_B.1), east, st(CHURCH.1), st(MILL.1)), LotKind::Rows(Use::Apartment), all);
    // Mill Street to Upper Road; a car park by East Lane.
    add("C-W", lot(west, spine, st(MILL.0), st(UPPER.0)), terrace, all);
    let (ce_min, ce_max) = lot(spine, east, st(MILL.1), st(UPPER.1));
    add("C-E", (ce_min, Vec2::new(ce_max.x - 64.0, ce_max.y)), terrace, all);
    add("car park", (Vec2::new(ce_max.x - 52.0, ce_min.y + 20.0), Vec2::new(ce_max.x, ce_min.y + 50.0)), LotKind::Parking, [false; 4]);
    add("C-E apartments", (Vec2::new(ce_max.x - 52.0, ce_min.y + 56.0), ce_max), LotKind::Rows(Use::Apartment), [false, true, false, true]);
    // The riverside row: one deep, backing onto the river, facing Main Street.
    add("riverside", (Vec2::new(WEST_X - 20.0, MAIN_Z - m - SIDEWALK - 18.0), Vec2::new(EAST_X + 20.0, MAIN_Z - m - SIDEWALK)), shops, [false, false, false, true]);
    // North of Upper Road: a row of semis facing it (the outskirts start behind them).
    add("upper W", lot(west, spine, st(UPPER.0), (UPPER.0 + 2.0 * (s + SIDEWALK) + 26.0, s)), LotKind::Rows(Use::House), [false, false, true, false]);
    add("upper E", lot(spine, east, st(UPPER.1), (UPPER.1 + 2.0 * (s + SIDEWALK) + 26.0, s)), LotKind::Rows(Use::House), [false, false, true, false]);
    out
}

/// Inside the town's core (where its blocks are)?
pub fn in_core(p: Vec2) -> bool {
    p.x >= TOWN_LO.x && p.x <= TOWN_HI.x && p.y >= TOWN_LO.y - 20.0 && p.y <= TOWN_HI.y
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Blocks are real rectangles, inside the core (bar the row north of Upper Road), and
    /// don't overlap.
    #[test]
    fn blocks_are_sound() {
        let b = blocks();
        for x in &b {
            assert!(x.max.x - x.min.x > 20.0 && x.max.y - x.min.y > 15.0, "{x:?}");
        }
        for (i, x) in b.iter().enumerate() {
            for y in &b[i + 1..] {
                let apart = x.max.x <= y.min.x || y.max.x <= x.min.x || x.max.y <= y.min.y || y.max.y <= x.min.y;
                assert!(apart, "{} and {} overlap", x.name, y.name);
            }
        }
    }
}

/// A parked car in Fernhollow: where (on the ground), which way it points, and which of the
/// four parked looks it has (two body shapes × two paints, so they batch into few draws).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parked {
    pub pos: Vec2,
    pub dir: Vec2,
    pub look: u8,
}

/// Parked cars' size (m): a traffic car's.
pub const PARKED_LEN: f32 = super::traffic::CAR_LEN;
pub const PARKED_WIDTH: f32 = super::traffic::CAR_WIDTH;

/// Fernhollow's parked cars: most bays of the car park filled, and cars pulled up on the
/// sidewalk (town style, two wheels on the kerb side) along the town's streets and Main
/// Street, clear of junctions and of each other. Pure: the same from the roads and blocks.
pub fn parked(net: &super::RoadNet, layout: &super::buildings::Layout) -> Vec<Parked> {
    use super::buildings::{LotKind, SIDEWALK};
    let mut rng = crate::stage::Rng::new(net.seed ^ 0x9A2C_ED);
    let mut out: Vec<Parked> = Vec::new();
    let lots: Vec<&super::buildings::Block> = layout.blocks.iter().filter(|b| b.cell.is_some()).collect();
    // The car park: its bays (2.5 x 5.5 m cells on the world grid, every third row an aisle).
    for blk in lots.iter().filter(|b| b.kind == LotKind::Parking) {
        let (i0, i1) = ((blk.min.x / 2.5).ceil() as i32, (blk.max.x / 2.5).floor() as i32 - 1);
        let (j0, j1) = ((-blk.max.y / 5.5).ceil() as i32, (-blk.min.y / 5.5).floor() as i32 - 1);
        for j in j0..=j1 {
            if j.rem_euclid(3) == 2 {
                continue;
            }
            for i in i0..=i1 {
                if rng.chance(0.65) {
                    let dir = if j.rem_euclid(3) == 0 { Vec2::Y } else { -Vec2::Y };
                    out.push(Parked { pos: Vec2::new(2.5 * (i as f32 + 0.5), -5.5 * (j as f32 + 0.5)), dir, look: (rng.f() * 4.0) as u8 % 4 });
                }
            }
        }
    }
    // On the sidewalk along the streets (roads 8 on) and Main Street in town.
    let on_sidewalk = |p: Vec2| {
        lots.iter().any(|b| {
            let (lo, hi) = (b.min - Vec2::splat(SIDEWALK), b.max + Vec2::splat(SIDEWALK));
            let inside_ring = p.x > lo.x && p.y > lo.y && p.x < hi.x && p.y < hi.y;
            let in_lot = p.x > b.min.x - 0.3 && p.y > b.min.y - 0.3 && p.x < b.max.x + 0.3 && p.y < b.max.y + 0.3;
            inside_ring && !in_lot
        })
    };
    for (k, r) in net.roads.iter().enumerate() {
        if !(k == 0 || k >= 8) {
            continue;
        }
        let road = &r.road;
        let here: Vec<usize> = net.junctions.iter().flat_map(|j| j.roads.iter().filter(|x| x.0 == k).map(|x| x.1)).collect();
        let mut i = 0;
        while i < road.len() {
            let c = Vec2::new(road.pts[i].x, road.pts[i].z);
            if !in_core(c) || here.iter().any(|&j| j.abs_diff(i) as f32 * crate::stage::STEP < 16.0) {
                i += 3;
                continue;
            }
            let d = road.dir[i];
            for side in [1.0f32, -1.0] {
                if !rng.chance(0.1) {
                    continue;
                }
                // Left of travel = (dz, −dx); the car's centre 1 m in from the kerb.
                let p = c + Vec2::new(d.y, -d.x) * side * (road.width[i] * 0.5 + 1.0);
                let corners = [p + d * (PARKED_LEN * 0.5), p - d * (PARKED_LEN * 0.5)];
                if corners.iter().all(|q| on_sidewalk(*q)) && out.iter().all(|o| o.pos.distance(p) > PARKED_LEN + 1.5) && !layout.occupied(p, 1.2) {
                    let dir = if rng.chance(0.5) { d } else { -d };
                    out.push(Parked { pos: p, dir, look: (rng.f() * 4.0) as u8 % 4 });
                }
            }
            i += (6.0 / crate::stage::STEP) as usize;
        }
    }
    out
}

/// A parked car as a solid: a capsule along it, as wide as the car, `top` m high.
pub fn parked_wall(p: &Parked, ground: f32) -> crate::vehicle::Wall {
    let r = PARKED_WIDTH * 0.5;
    let h = p.dir * (PARKED_LEN * 0.5 - r);
    crate::vehicle::Wall::new(p.pos - h, p.pos + h, r, ground + 1.5)
}

#[cfg(test)]
mod parked_tests {
    /// Cars park in town, off the traffic lanes (on sidewalks or in the car park), apart.
    #[test]
    fn parked_cars_keep_off_the_lanes() {
        let net = crate::open::roads::generate(crate::open::SEED);
        let layout = crate::open::buildings::layout(&net);
        let cars = super::parked(&net, &layout);
        eprintln!("{} parked cars", cars.len());
        assert!((30..=160).contains(&cars.len()), "{}", cars.len());
        for c in &cars {
            for q in [c.pos + c.dir * 2.2, c.pos - c.dir * 2.2, c.pos] {
                if let Some(n) = net.nearest(q, 30.0) {
                    let half = net.roads[n.road].road.half_width(&n.at);
                    assert!(n.at.lateral.abs() > half - 0.1, "parked car at {} on a road", c.pos);
                }
            }
        }
    }
}
