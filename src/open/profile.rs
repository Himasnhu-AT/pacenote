//! The open world's road cross-section (docs/RALLY_MAP.md phase 4): what a road is like from
//! one side to the other, for the car and for the terrain.
//!
//! ```text
//!   cut slope                                                  fill
//!        \   ditch    shoulder   left  crown  right  shoulder    \
//!         \  (V)                track        track                \ (steep where the land falls)
//!          ╰──╮ ╭───╮──────────────┬──────┬──────────────╮────────╲
//!             ╰─╯   0.5 m           +6-8 cm              0.5 m      ╲
//!          0.8-1.2 m
//! ```
//!
//! The road itself (crown, banking, worn wheel tracks) is `stage::Road::height_at`, which the
//! mesh and the car share. Beyond its edge: a soft shoulder, then on the side the road is cut
//! into the hill a V ditch, on the fill side the ground (steep where the land falls away). The
//! car feels this exactly within a few metres of a road ([`ground`]); the 2 m terrain carries
//! a coarse version of it ([`shaped`]).

use bevy::math::Vec2;

use super::roads::{RoadKind, RoadNet};
use crate::stage::{Road, RoadPos, STEP, noise};

/// Soft shoulder beyond the road edge (m), falling [`SHOULDER_DROP`].
pub const SHOULDER: f32 = 0.5;
pub const SHOULDER_DROP: f32 = 0.04;
/// Width of a ditch (m) and its depth range.
pub const DITCH_W: f32 = 1.0;
pub const DITCH_DEPTH: (f32, f32) = (0.25, 0.45);
/// Past the ditch (or shoulder), the exact profile blends into the terrain over this (m).
const BLEND: f32 = 1.0;
/// How far (m) from a junction ditches fade out (cars cut across corners there).
const JUNCTION_CLEAR: f32 = 26.0;
/// Dusty tarmac reaches this far (m) from a junction with a gravel road.
pub const DUSTY: f32 = 40.0;

/// Crown (m) and wheel-track depth (m) by kind.
fn crown_rut(kind: RoadKind) -> (f32, f32) {
    match kind {
        RoadKind::Street => (0.05, 0.0),
        RoadKind::BRoad => (0.07, 0.0),
        RoadKind::Gravel => (0.08, 0.02),
        RoadKind::Forest => (0.08, 0.025),
    }
}

/// Sets every road's crown and wheel tracks, its ditches (on the side cut into the hill) and its
/// dusty tarmac. Derived from the land and the junctions, so it isn't stored in the level.
pub fn dress(net: &mut RoadNet) {
    let land = net.land.clone();
    let junctions = net.junctions.clone();
    let gravel = |k: RoadKind| matches!(k, RoadKind::Gravel | RoadKind::Forest);
    // Junctions where a loose surface meets (dusty tarmac round them).
    let loose: Vec<bool> = junctions.iter().map(|j| j.roads.iter().any(|&(r, i)| gravel(net.roads[r].kind) && net.roads[r].road.surface[i] != crate::stage::Surface::Tarmac)).collect();
    for (ri, nr) in net.roads.iter_mut().enumerate() {
        let n = nr.road.len();
        let (crown, rut) = crown_rut(nr.kind);
        let road = &mut nr.road;
        // Less crown in bends (the banking takes over), none on mud ruts deeper than gravel's.
        road.crown = (0..n).map(|i| crown * (1.0 - (road.curv[i].abs() * 60.0).min(1.0) * 0.7)).collect();
        road.rut = (0..n)
            .map(|i| match road.surface[i] {
                crate::stage::Surface::Mud => rut * 1.5,
                crate::stage::Surface::Tarmac => 0.0,
                _ => rut,
            })
            .collect();
        // Where junctions are on this road (station), to keep ditches out of them.
        let here: Vec<usize> = junctions.iter().flat_map(|j| j.roads.iter().filter(|r| r.0 == ri).map(|r| r.1)).collect();
        let clear = |i: usize| here.iter().map(|&s| (s as f32 - i as f32).abs() * STEP).fold(f32::MAX, f32::min);
        let seed = net.seed ^ (0xD17C + ri as u64);
        let mut ditch = vec![[0.0f32; 2]; n];
        if nr.kind != RoadKind::Street {
            // Cut side: the land 5 m out stands well above the road.
            let cut: Vec<[bool; 2]> = (0..n)
                .map(|i| {
                    let p = road.pts[i];
                    let left = road.dir[i].perp() * -1.0;
                    let half = road.width[i] * 0.5;
                    let side = |s: f32| land.coarse(Vec2::new(p.x, p.z) + left * s * (half + 5.0)) > p.y + 0.6;
                    [side(1.0), side(-1.0)]
                })
                .collect();
            // A ditch where most of the 40 m around is cut (no flicker), fading at the ends
            // and near junctions.
            let w = (20.0 / STEP) as usize;
            for side in 0..2 {
                let mut run = vec![0.0f32; n];
                for (i, v) in run.iter_mut().enumerate() {
                    let (a, b) = (i.saturating_sub(w), (i + w).min(n - 1));
                    let k = (a..=b).filter(|&j| cut[j][side]).count() as f32 / (b - a + 1) as f32;
                    *v = ((k - 0.5) * 4.0).clamp(0.0, 1.0);
                }
                for i in 0..n {
                    let depth = DITCH_DEPTH.0 + (DITCH_DEPTH.1 - DITCH_DEPTH.0) * (noise(Vec2::new(i as f32 * STEP / 90.0, side as f32 * 7.0), seed) * 0.5 + 0.5);
                    let near = ((clear(i) - JUNCTION_CLEAR) / 12.0).clamp(0.0, 1.0);
                    ditch[i][side] = depth * run[i] * near;
                }
            }
        }
        // Bridges: over the river (no ditches there).
        nr.bridge = (0..n).map(|i| land.river_distance(Vec2::new(road.pts[i].x, road.pts[i].z)) < super::roads::BRIDGE).collect();
        for (d, b) in ditch.iter_mut().zip(&nr.bridge) {
            if *b {
                *d = [0.0, 0.0];
            }
        }
        // Culverts (or a rock) in the inside ditch of tight corners, at most one per 150 m: cut
        // the corner there and the car hits it, so the corner's note gets "don't cut".
        nr.culverts.clear();
        if matches!(nr.kind, RoadKind::Gravel | RoadKind::Forest) {
            let mut last = f32::MIN;
            for c in crate::stage::corners(road) {
                if c.grade > 3 || c.start - last < 150.0 {
                    continue;
                }
                let (a, b) = (road.station(c.start), road.station(c.end));
                let apex = (a..=b).max_by(|&x, &y| road.curv[x].abs().total_cmp(&road.curv[y].abs())).unwrap_or(a);
                // The inside: a left turn's inside is its left (lateral +).
                let side = if road.curv[apex] > 0.0 { 0 } else { 1 };
                if ditch[apex][side] < 0.2 {
                    continue;
                }
                let lat = if side == 0 { 1.0 } else { -1.0 } * (road.width[apex] * 0.5 + SHOULDER + DITCH_W * 0.5);
                nr.culverts.push((apex, lat));
                for i in a..=b {
                    road.dont_cut[i] = true;
                }
                last = c.start;
            }
        }
        nr.ditch = ditch;
        // How fast the land falls away 8 m beyond each edge, averaged over ±10 m along the
        // road (per point it flickers between a gentle fill and a drop-off).
        let raw: Vec<[f32; 2]> = (0..n)
            .map(|i| {
                let p = road.pts[i];
                let left = road.dir[i].perp() * -1.0;
                let half = road.width[i] * 0.5;
                let side = |s: f32| ((p.y - land.coarse(Vec2::new(p.x, p.z) + left * s * (half + 8.0))) / 8.0).max(0.0);
                [side(1.0), side(-1.0)]
            })
            .collect();
        let w = (10.0 / STEP) as usize;
        nr.fall = (0..n)
            .map(|i| {
                let (a, b) = (i.saturating_sub(w), (i + w).min(n - 1));
                let k = (b - a + 1) as f32;
                let sum = raw[a..=b].iter().fold([0.0f32; 2], |s, f| [s[0] + f[0], s[1] + f[1]]);
                [sum[0] / k, sum[1] / k]
            })
            .collect();
        // Dusty tarmac near a junction with a gravel or forest road.
        nr.dusty = vec![false; n];
        if !gravel(nr.kind) {
            for (j, _) in junctions.iter().zip(&loose).filter(|(_, l)| **l) {
                for &(r, st) in &j.roads {
                    if r != ri {
                        continue;
                    }
                    let reach = (DUSTY / STEP) as usize;
                    for i in st.saturating_sub(reach)..(st + reach + 1).min(n) {
                        nr.dusty[i] = true;
                    }
                }
            }
        }
    }
}

/// Ditch depth (m) at a road position on the side it lies (0 = none).
fn ditch_at(ditch: &[[f32; 2]], r: &RoadPos) -> f32 {
    let side = if r.lateral >= 0.0 { 0 } else { 1 };
    let j = (r.i + 1).min(ditch.len() - 1);
    ditch[r.i][side] + (ditch[j][side] - ditch[r.i][side]) * r.t
}

/// The road's edge height on the side of `r`.
fn edge(road: &Road, r: &RoadPos) -> f32 {
    let half = road.half_width(r);
    road.height_at(&RoadPos { lateral: r.lateral.signum() * half, ..*r })
}

/// The exact ground at `p` near a road, given the terrain there (`terrain`, the heightfield):
/// the road, its shoulder, its ditch, blended into the terrain a little further out. `None`
/// beyond that (the terrain is the ground).
pub fn ground(net: &RoadNet, p: Vec2, terrain: f32) -> Option<f32> {
    let n = net.nearest(p, 3.0 + SHOULDER + DITCH_W + BLEND + 1.0)?;
    let nr = &net.roads[n.road];
    let (road, r) = (&nr.road, &n.at);
    let half = road.half_width(r);
    let o = r.lateral.abs() - half;
    if o <= 0.0 {
        return Some(road.height_at(r));
    }
    let lip = edge(road, r);
    if o <= SHOULDER {
        return Some(lip - SHOULDER_DROP * o / SHOULDER);
    }
    let shoulder = lip - SHOULDER_DROP;
    let d = if nr.ditch.is_empty() { 0.0 } else { ditch_at(&nr.ditch, r) };
    let o = o - SHOULDER;
    if d > 0.0 {
        if o <= DITCH_W {
            let u = o / DITCH_W;
            return Some(shoulder - d * (1.0 - (2.0 * u - 1.0).abs()));
        }
        let t = (o - DITCH_W) / BLEND;
        return (t < 1.0).then(|| shoulder + (terrain - shoulder) * smooth(t));
    }
    let t = o / BLEND;
    (t < 1.0).then(|| shoulder + (terrain - shoulder) * smooth(t))
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The terrain `base` m high at `r` beside road `nr`, shaped by it (the coarse 2 m
/// heightfield): the road sunk into the land, a short shoulder, then on the cut side the
/// ditch's bottom and a bank no steeper than [`CUT_GRADE`], on the fill side the land held up
/// no less steeply than [`FILL_GRADE`] where it falls gently and left to drop (a bank of
/// [`STEEP_FILL`]) where it falls away faster than [`STEEP`]; back to the natural land from
/// [`REACH`], over a width that keeps the blend itself under ~60 % ([`REACH_MAX`] at most).
pub fn shaped(base: f32, nr: &super::roads::NetRoad, r: &RoadPos) -> f32 {
    let (road, ditch) = (&nr.road, &nr.ditch);
    let half = road.half_width(r);
    let a = r.lateral.abs();
    let o = a - half;
    if o <= 0.0 {
        return road.height_at(r);
    }
    let lip = edge(road, r) - SHOULDER_DROP;
    if o <= SHOULDER {
        return lip;
    }
    let d = if ditch.is_empty() { 0.0 } else { ditch_at(ditch, r) };
    // In the ditch: its bottom (the terrain stays under the exact ditch the car feels).
    if d > 0.0 && o <= SHOULDER + DITCH_W {
        return lip - d;
    }
    let out = o - SHOULDER - if d > 0.0 { DITCH_W } else { 0.0 };
    // How fast the land falls away beside the road: steep means a drop-off, not a gentle fill
    // (per station, averaged along the road; per point before the dressing).
    let fall = if nr.fall.is_empty() {
        ((lip - base) / out.max(1.0)).max(0.0)
    } else {
        let side = if r.lateral >= 0.0 { 0 } else { 1 };
        let j = (r.i + 1).min(nr.fall.len() - 1);
        nr.fall[r.i][side] + (nr.fall[j][side] - nr.fall[r.i][side]) * r.t
    };
    let fill = FILL_GRADE + (STEEP_FILL - FILL_GRADE) * smooth((fall - STEEP) / 0.25);
    let shaped = base.clamp(lip - fill * out, lip + CUT_GRADE * out);
    let width = ((shaped - base).abs() / BLEND_GRADE).clamp(16.0, REACH_MAX - REACH.0);
    let t = smooth((a - REACH.0) / width);
    shaped + (base - shaped) * t
}

/// Gentlest fill (rise per metre) where the land beside falls gently: a car can drive back up.
const FILL_GRADE: f32 = 0.16;
/// Where the land falls faster than this, the road edge drops off ...
const STEEP: f32 = 0.2;
/// ... as steeply as this (a loose bank, at the land's talus).
const STEEP_FILL: f32 = 0.7;
/// Steepest cut bank.
const CUT_GRADE: f32 = 0.6;
/// Lateral distance (m) where the shaped ground starts to blend back into the land, and the
/// gentlest slope (rise per metre) of the blend's width ...
pub const REACH: (f32, f32) = (30.0, 46.0);
const BLEND_GRADE: f32 = 0.4;
/// ... which ends by this far (m) from the centreline at the latest.
pub const REACH_MAX: f32 = 110.0;

/// Parapet height (m) above a bridge's deck, and its half thickness.
pub const PARAPET: (f32, f32) = (0.8, 0.25);

/// What the car hits beside the roads: bridge parapets (stone walls along each side of a deck)
/// and culvert headwalls in the ditches.
pub fn walls(net: &RoadNet) -> Vec<crate::vehicle::Wall> {
    use crate::vehicle::Wall;
    let mut out = Vec::new();
    for nr in &net.roads {
        let road = &nr.road;
        let left = |i: usize| -road.dir[i].perp();
        let at = |i: usize, lat: f32| Vec2::new(road.pts[i].x, road.pts[i].z) + left(i) * lat;
        // Parapets: a segment every 2 stations along each edge of a bridge.
        let mut i = 0;
        while i + 2 < nr.bridge.len() {
            if nr.bridge[i] && nr.bridge[i + 2] {
                for side in [1.0f32, -1.0] {
                    let lat = |k: usize| side * (road.width[k] * 0.5 + PARAPET.1);
                    let top = road.pts[i].y.max(road.pts[i + 2].y) + PARAPET.0;
                    out.push(Wall::new(at(i, lat(i)), at(i + 2, lat(i + 2)), PARAPET.1, top));
                }
            }
            i += 2;
        }
        // Culverts: a 1 m concrete headwall across the ditch.
        for &(i, lat) in &nr.culverts {
            let c = at(i, lat);
            let d = road.dir[i] * 0.5;
            out.push(Wall::new(c - d, c + d, 0.3, road.pts[i].y + 0.2));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open::roads;

    #[test]
    fn crowned_ditched_and_dropping_off() {
        let net = roads::generate(crate::open::SEED);
        // Somewhere on a forest road with a ditch on one side.
        let (k, i, side) = net
            .roads
            .iter()
            .enumerate()
            .filter(|(_, r)| r.kind == RoadKind::Forest)
            .find_map(|(k, r)| (100..r.road.len() - 100).find_map(|i| (0..2).find(|&s| r.ditch[i][s] > 0.3).map(|s| (k, i, s))))
            .expect("a forest road with a ditch");
        let nr = &net.roads[k];
        let road = &nr.road;
        let p = road.pts[i];
        let left = -road.dir[i].perp();
        let sign = if side == 0 { 1.0 } else { -1.0 };
        let half = road.width[i] * 0.5;
        let at = |lat: f32| ground(&net, bevy::math::Vec2::new(p.x, p.z) + left * lat, 1e4);
        // Crowned: the middle stands above the edges (by the crown, ± the banking).
        let mid = at(0.0).unwrap();
        let (l, r) = (at(half - 0.01).unwrap(), at(-(half - 0.01)).unwrap());
        assert!(road.crown[i] > 0.02, "crown {}", road.crown[i]);
        assert!((mid - (l + r) * 0.5 - road.crown[i]).abs() < 0.01, "crown {}: mid {mid} edges {l} {r}", road.crown[i]);
        // The ditch: its bottom 0.25-0.45 m below the shoulder.
        let lip = at(sign * (half + SHOULDER)).unwrap();
        let bottom = at(sign * (half + SHOULDER + DITCH_W * 0.5)).unwrap();
        assert!((0.2..0.5).contains(&(lip - bottom)), "ditch {} deep", lip - bottom);
        // Wheel tracks are worn in.
        let track = at(crate::stage::TRACK_LINE).unwrap();
        assert!(track < at(crate::stage::TRACK_LINE + 0.6).unwrap() + 0.01 || track < mid, "tracks");
        // No ditches at junctions.
        for j in &net.junctions {
            for &(rk, st) in &j.roads {
                assert_eq!(net.roads[rk].ditch[st], [0.0, 0.0], "ditch in the junction at {}", j.pos);
            }
        }
        // Dusty tarmac where gravel roads join the B-road.
        assert!(net.roads.iter().any(|r| r.dusty.iter().any(|d| *d)));
    }
}

#[cfg(test)]
mod drive {
    use bevy::math::{Vec2, Vec3};

    use crate::open::roads::RoadKind;
    use crate::open::{OpenWorld, SEED};
    use crate::stage::{self, Surface};
    use crate::vehicle::{Car, Ground, Input, Spec, step};

    /// One in seven or so gravel corners is off-camber, a quarter flat, the rest lean in.
    #[test]
    fn banking_mix() {
        let net = crate::open::roads::generate(SEED);
        let (mut off, mut flat, mut all) = (0, 0, 0);
        for r in net.roads.iter().filter(|r| matches!(r.kind, RoadKind::Gravel | RoadKind::Forest)) {
            for c in stage::corners(&r.road) {
                let i = r.road.station((c.start + c.end) * 0.5);
                let lean = -r.road.bank[i] * r.road.curv[i].signum();
                all += 1;
                if c.off_camber {
                    off += 1;
                }
                if lean.abs() < 0.01 {
                    flat += 1;
                }
            }
        }
        let (off, flat) = (off as f32 / all as f32, flat as f32 / all as f32);
        eprintln!("{all} corners: {:.0} % off-camber, {:.0} % flat", off * 100.0, flat * 100.0);
        assert!((0.06..0.28).contains(&off), "off-camber {off}");
        assert!((0.1..0.45).contains(&flat), "flat {flat}");
    }

    /// Two wheels dropped into the ditch drag the car: it slows much more than on the road,
    /// and stays on its wheels.
    #[test]
    fn the_ditch_drags() {
        let w = OpenWorld::generate(SEED);
        let net = &w.net;
        // The straight stretch with a ditch along 120 m that climbs least.
        let mut best: Option<(f32, usize, usize, usize)> = None;
        for (k, r) in net.roads.iter().enumerate().filter(|(_, r)| matches!(r.kind, RoadKind::Gravel | RoadKind::Forest)) {
            for i in (150..r.road.len() - 150).step_by(5) {
                for s in 0..2 {
                    if (i - 30..i + 30).all(|j| r.road.curv[j].abs() < 1.0 / 250.0) {
                        let depth = (i - 30..i + 30).map(|j| r.ditch[j][s]).fold(f32::MAX, f32::min);
                        let climb = (r.road.pts[i + 30].y - r.road.pts[i - 30].y).abs();
                        if depth > 0.25 && best.is_none_or(|b| climb < b.0) {
                            best = Some((climb, k, i, s));
                        }
                    }
                }
            }
        }
        let (_, k, i, side) = best.expect("a straight stretch with a ditch");
        let road = &net.roads[k].road;
        let sign = if side == 0 { 1.0 } else { -1.0 };
        let left = -road.dir[i].perp();
        let half = road.width[i] * 0.5;
        let spec = Spec::default();
        let coast = |lat: f32| {
            let p = Vec2::new(road.pts[i].x, road.pts[i].z) + left * lat;
            let d = road.dir[i];
            let mut car = Car::new(Vec3::new(p.x, w.height(p.x, p.y) + 0.5, p.y), libm::atan2f(d.x, d.y));
            let v = 40.0 / 3.6;
            car.vel = car.forward() * v;
            for wh in &mut car.wheels {
                wh.spin = v / spec.wheel_radius;
            }
            let mut ditch = 0;
            for _ in 0..120 {
                step(&mut car, &spec, Input::default(), &w, 1.0 / 60.0);
                ditch += car.wheels.iter().filter(|wh| wh.surface == Some(Surface::Ditch)).count();
            }
            assert!(car.pos.is_finite() && car.up().y > 0.7, "upright at {lat}");
            (car.vel.length(), ditch)
        };
        let (on_road, _) = coast(0.0);
        // Two wheels in the ditch (the car's centre ~0.8 m in from them).
        let (in_ditch, wheels) = coast(sign * (half + super::SHOULDER + super::DITCH_W * 0.5 - 0.8));
        eprintln!("after 2 s coasting from 40 km/h: road {:.1} km/h, ditch {:.1} km/h ({wheels} wheel-steps in it)", on_road * 3.6, in_ditch * 3.6);
        assert!(wheels > 100, "wheels in the ditch: {wheels}");
        // (On the stretch the smoothed land leaves, a climb, the car loses ~1.3 km/h more in
        // the ditch over 2 s; the old land's level stretch showed ~4.5.)
        assert!(in_ditch < on_road - 0.3, "the ditch drags: {in_ditch} vs {on_road}");
    }
}

#[cfg(test)]
mod hazards {
    use bevy::math::Vec2;

    use crate::open::roads::{BRIDGE_W, RoadKind};
    use crate::stage;

    #[test]
    fn bridges_and_culverts() {
        let net = crate::open::roads::generate(crate::open::SEED);
        // The southern loop crosses the river twice: two bridges, narrow, with parapets.
        let south = net.roads.iter().find(|r| r.kind == RoadKind::Gravel && r.bridge.iter().any(|b| *b)).expect("a bridge");
        let spans = south.bridge.windows(2).filter(|w| !w[0] && w[1]).count();
        assert_eq!(spans, 2, "bridges on the southern loop");
        let i = south.bridge.iter().position(|b| *b).unwrap() + 3;
        assert!((south.road.width[i] - BRIDGE_W).abs() < 0.3, "deck {}", south.road.width[i]);
        let walls = super::walls(&net);
        let p = Vec2::new(south.road.pts[i].x, south.road.pts[i].z);
        let near: Vec<_> = walls.iter().filter(|w| w.distance(p) < BRIDGE_W).collect();
        assert!(near.len() >= 2, "parapets each side: {}", near.len());
        assert!(near.iter().all(|w| w.distance(p) > BRIDGE_W * 0.5 - 0.05), "parapet on the deck: {:?} at {p}, widths {:?}, curv {:?}", near.iter().map(|w| (w.distance(p), w.a, w.b)).collect::<Vec<_>>(), &south.road.width[i - 4..i + 4], &south.road.curv[i - 4..i + 4]);
        // The pace notes call the narrowing onto a bridge.
        assert!(stage::corners(&south.road).iter().any(|c| c.narrows) || {
            // (Or on a straight approach: the width drops by more than a metre.)
            let w0 = south.road.width[i.saturating_sub(15)];
            w0 - south.road.width[i] > 1.0
        });
        // Culverts: on gravel / forest corners with a ditch inside, and their notes say so.
        let mut culverts = 0;
        for r in net.roads.iter().filter(|r| !r.culverts.is_empty()) {
            culverts += r.culverts.len();
            let calls = stage::corners(&r.road);
            for &(apex, _) in &r.culverts {
                let s = apex as f32 * stage::STEP;
                assert!(calls.iter().any(|c| c.dont_cut && c.start <= s && s <= c.end), "culvert at station {apex} without a don't-cut call");
            }
        }
        eprintln!("{culverts} culverts, {} parapet / headwall pieces", walls.len());
        assert!(culverts >= 10, "{culverts} culverts");
    }
}
