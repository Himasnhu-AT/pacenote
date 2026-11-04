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
