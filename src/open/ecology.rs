//! Where things grow (docs/RALLY_MAP.md phase 6): masks of the land for the dressing graph
//! (`world::dressing`), from elevation, slope and moisture, so each part of the map reads
//! differently from the car (there is no map).
//!
//! - **Alpine** (high plateau, or steep and high): above the treeline. Stunted trees, bare rock.
//! - **Fields** (the valley floor, flat): meadows and pasture with scattered trees.
//! - **Clear-cuts**: 150-300 m logged patches on the forested slopes, beside a gravel or forest
//!   road, full of stumps; they open long views down the valley.
//!
//! Each mask is a list of rectangles (40 m cells merged along rows; clear-cuts are rotated
//! rectangles, as logging is).

use bevy::math::Vec2;
use pcg_core::Rect;

use super::land::{HALF, Land, Zone};
use super::roads::{RoadKind, RoadNet};
use crate::stage::{Rng, fbm, noise};

/// Most clear-cuts on the map.
pub const CLEARCUTS: usize = 9;

/// Mask cell (m).
const CELL: f32 = 40.0;

/// Above the treeline here: high, or high and steep (wind-blasted).
pub fn alpine(land: &Land, p: Vec2) -> bool {
    let h = land.coarse(p) + noise(p / 300.0, land.seed ^ 0xA1) * 18.0;
    h > 412.0 || (h > 385.0 && land.slope(p) > 0.55)
}

/// Open pasture on the valley floor (not the river bank's trees).
pub fn field(land: &Land, p: Vec2) -> bool {
    land.zone(p) == Zone::Valley && land.slope(p) < 0.08 && land.river_distance(p) > 45.0 && fbm(p / 260.0, 2, land.seed ^ 0xF1) > -0.25
}

/// A mask's cells as rectangles: runs of cells along each row, merged.
fn cells(tag: &str, f: impl Fn(Vec2) -> bool) -> Vec<Rect> {
    let n = (2.0 * HALF / CELL) as i32;
    let mut out = Vec::new();
    for j in 0..n {
        let z = -HALF + (j as f32 + 0.5) * CELL;
        let mut run: Option<i32> = None;
        for i in 0..=n {
            let on = i < n && f(Vec2::new(-HALF + (i as f32 + 0.5) * CELL, z));
            match (on, run) {
                (true, None) => run = Some(i),
                (false, Some(a)) => {
                    let (x0, x1) = (-HALF + a as f32 * CELL, -HALF + i as f32 * CELL);
                    out.push(Rect { center: Vec2::new((x0 + x1) * 0.5, z), half: Vec2::new((x1 - x0) * 0.5, CELL * 0.5), yaw: 0.0, y: 0.0, tag: tag.into() });
                    run = None;
                }
                _ => {}
            }
        }
    }
    out
}

/// Clear-cut patches: up to `count` rotated rectangles on forested slopes (150-350 m, not
/// steep), each beside a gravel or forest road (its near edge 8-20 m off it) and lined up
/// with it, 600 m or more apart.
pub fn clearcuts(net: &RoadNet, count: usize) -> Vec<Rect> {
    let land = &net.land;
    let mut rng = Rng::new(net.seed ^ 0xC1EA);
    let mut out: Vec<Rect> = Vec::new();
    for (k, r) in net.roads.iter().enumerate().filter(|(_, r)| matches!(r.kind, RoadKind::Gravel | RoadKind::Forest)) {
        let road = &r.road;
        let mut i = 60 + (k * 37) % 90;
        while i + 60 < road.len() && out.len() < count {
            let p = Vec2::new(road.pts[i].x, road.pts[i].z);
            let d = road.dir[i];
            let side = if rng.chance(0.5) { 1.0 } else { -1.0 };
            // (A rect's local y runs along `yaw`'s direction: half.x across the road, half.y along.)
            let half = Vec2::new(rng.range(55.0, 100.0), rng.range(75.0, 150.0));
            let c = p + d.perp() * side * (road.width[i] * 0.5 + rng.range(8.0, 20.0) + half.x);
            let h = land.coarse(c);
            let ok = (150.0..350.0).contains(&h) && land.slope(c) < 0.35 && !alpine(land, c) && out.iter().all(|o| o.center.distance(c) > 600.0) && c.x.abs() < HALF - 200.0 && c.y.abs() < HALF - 200.0;
            let rect = Rect { center: c, half, yaw: libm::atan2f(d.x, d.y), y: h, tag: "clearcut".into() };
            // Over no other road (another leg of this one, a road nearby): points every 6 m over it.
            let clear = || {
                let k = rect.corners();
                let mid = (k[0] + k[1] + k[2] + k[3]) * 0.25;
                let (na, nb) = ((k[0].distance(k[1]) / 6.0).ceil() as i32, (k[0].distance(k[3]) / 6.0).ceil() as i32);
                (0..=na).all(|a| {
                    (0..=nb).all(|b| {
                        let (u, v) = (a as f32 / na as f32, b as f32 / nb as f32);
                        let q = k[0].lerp(k[1], u).lerp(k[3].lerp(k[2], u), v);
                        net.nearest(mid + (q - mid) * 0.97, 5.0).is_none()
                    })
                })
            };
            if ok && clear() {
                out.push(rect);
                i += 300;
            } else {
                i += 40;
            }
        }
    }
    out
}

/// The three masks: (alpine, fields, clear-cuts).
pub fn masks(net: &RoadNet) -> (Vec<Rect>, Vec<Rect>, Vec<Rect>) {
    let land = &net.land;
    (cells("alpine", |p| alpine(land, p)), cells("fields", |p| field(land, p)), clearcuts(net, CLEARCUTS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_kinds_of_country() {
        let net = crate::open::roads::generate(crate::open::SEED);
        let (alp, fields, cuts) = masks(&net);
        let area = |v: &[Rect]| v.iter().map(|r| 4.0 * r.half.x * r.half.y).sum::<f32>() / 1e6;
        eprintln!("alpine {:.2} km², fields {:.2} km², {} clear-cuts {:.2} km²", area(&alp), area(&fields), cuts.len(), area(&cuts));
        assert!(area(&alp) > 0.5 && area(&alp) < 6.0, "alpine {}", area(&alp));
        assert!(area(&fields) > 0.5, "fields {}", area(&fields));
        assert!((5..=9).contains(&cuts.len()), "{} clear-cuts", cuts.len());
        for c in &cuts {
            assert!(c.half.y >= 75.0 && c.half.y <= 150.0);
            // Never over a road (its corners and middle).
            for p in c.corners().into_iter().chain([c.center]) {
                let q = c.center + (p - c.center) * 0.9;
                assert!(net.nearest(q, 3.0).is_none(), "clear-cut over a road at {}", c.center);
            }
        }
    }
}
