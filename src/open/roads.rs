//! The road network of the open world, laid on the land (`open::land`) by the planner
//! (`open::plan`): a valley B-road along the river through the village and service park, a
//! tarmac hillclimb up the west side, a gravel road through the basin and up the east wall, a
//! mountain pass over the plateau, forestry roads on the escarpment with a logging loop, a
//! gully road and a southern gravel loop over two river crossings. Every road ends on another
//! road: no dead ends. Pure and deterministic (libm, no platform floats), like `stage.rs`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use bevy::math::{Vec2, Vec3};

use super::land::{self, Land, Zone};
use super::plan::{self, Planner, Rules};
use crate::stage::{Road, RoadPos, STEP, Surface, noise, smooth};

/// Half the side of the region (m): the world spans -HALF..HALF in x and z.
pub const HALF: f32 = land::HALF;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, bevy::reflect::Reflect)]
pub enum RoadKind {
    /// Village lanes (tarmac).
    Street,
    /// The valley B-road and the tarmac hillclimb. (`Highway`: the old ring, in old files.)
    #[serde(alias = "Highway")]
    BRoad,
    /// Fast gravel roads. (`Country`: the old spokes.)
    #[serde(alias = "Country")]
    Gravel,
    /// Forestry and mountain tracks (gravel, single car width).
    Forest,
}

impl RoadKind {
    /// Typical full width (m); gravel and forest roads vary around it.
    pub fn width(self) -> f32 {
        match self {
            RoadKind::Street => 5.0,
            RoadKind::BRoad => 6.5,
            RoadKind::Gravel => 4.6,
            RoadKind::Forest => 3.4,
        }
    }
}

/// One road of the network.
#[derive(Clone, Debug)]
pub struct NetRoad {
    pub kind: RoadKind,
    pub road: Road,
    /// The last point joins the first.
    pub closed: bool,
    /// Traffic speed limit (m/s) when set in the world data; else by kind (`traffic::limit`).
    pub limit: Option<f32>,
    /// Bounding box (XZ) grown by the widest reach anyone asks for, for quick rejects.
    lo: Vec2,
    hi: Vec2,
}

/// Where roads meet: (road index, station index) of each road at the junction.
#[derive(Clone, Debug)]
pub struct Junction {
    pub pos: Vec2,
    pub roads: Vec<(usize, usize)>,
}

/// The whole network.
#[derive(Clone, Debug)]
pub struct RoadNet {
    pub seed: u64,
    pub roads: Vec<NetRoad>,
    pub junctions: Vec<Junction>,
    /// The village's ground height (m).
    pub town_y: f32,
    /// The land under it (`land::land(seed)`).
    pub land: Arc<Land>,
}

/// The nearest road to a point.
#[derive(Clone, Copy, Debug)]
pub struct NetPos {
    pub road: usize,
    pub at: RoadPos,
}

const REACH: f32 = 60.0;

impl NetRoad {
    pub(crate) fn new(kind: RoadKind, road: Road, closed: bool) -> NetRoad {
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in &road.pts {
            lo = lo.min(Vec2::new(p.x, p.z));
            hi = hi.max(Vec2::new(p.x, p.z));
        }
        NetRoad { kind, road, closed, limit: None, lo: lo - REACH, hi: hi + REACH }
    }
}

impl RoadNet {
    /// The road closest to `p` within `max` m (by distance from the centreline).
    pub fn nearest(&self, p: Vec2, max: f32) -> Option<NetPos> {
        let mut best: Option<NetPos> = None;
        for (k, r) in self.roads.iter().enumerate() {
            if p.x < r.lo.x || p.y < r.lo.y || p.x > r.hi.x || p.y > r.hi.y {
                continue;
            }
            if let Some(at) = r.road.nearest(p, max)
                && best.is_none_or(|b| at.lateral.abs() < b.at.lateral.abs())
            {
                best = Some(NetPos { road: k, at });
            }
        }
        best
    }

    /// Total length of road (m).
    pub fn length(&self) -> f32 {
        self.roads.iter().map(|r| r.road.length()).sum()
    }

    /// The places the network connects.
    pub fn places(&self) -> plan::Places {
        plan::places(&self.land)
    }
}

/// The junctions of a set of roads, found from where they meet: a station of one road lying
/// on another (within [`JOIN`] m). Used when the network comes from the world data
/// (`open::data`), where only the roads are stored. Sorted by position, one per meeting point.
pub fn find_junctions(roads: &[NetRoad]) -> Vec<Junction> {
    let mut out: Vec<Junction> = Vec::new();
    for (a, ra) in roads.iter().enumerate() {
        for (i, p3) in ra.road.pts.iter().enumerate() {
            let p = Vec2::new(p3.x, p3.z);
            for (b, rb) in roads.iter().enumerate() {
                if b == a || p.x < rb.lo.x || p.y < rb.lo.y || p.x > rb.hi.x || p.y > rb.hi.y {
                    continue;
                }
                // On `b`'s centreline (anywhere along a segment, not only at its stations).
                let Some(at) = rb.road.nearest(p, JOIN) else { continue };
                // The station of `b` nearest the point (the end of the segment past half way).
                let j = if at.t < 0.5 { at.i } else { (at.i + 1).min(rb.road.len() - 1) };
                // Only where `a` meets `b`: an end of `a`, or a crossing (`a` runs on along
                // both sides of the point, so it doesn't just touch).
                let end = !ra.closed && (i == 0 || i + 1 == ra.road.len());
                let at_end_of_b = !rb.closed && (j == 0 || j + 1 == rb.road.len());
                if !end && !at_end_of_b && !crosses(&ra.road, i, &rb.road, j) {
                    continue;
                }
                // A road crossing between stations can come within JOIN of the other at two of
                // its stations: one junction.
                let k = match out.iter().position(|o| o.pos.distance(p) <= 2.0 * STEP) {
                    Some(k) => k,
                    None => {
                        out.push(Junction { pos: p, roads: Vec::new() });
                        out.len() - 1
                    }
                };
                for rs in [(a, i), (b, j)] {
                    if !out[k].roads.iter().any(|r| r.0 == rs.0) {
                        out[k].roads.push(rs);
                    }
                }
            }
        }
    }
    for j in &mut out {
        j.roads.sort_unstable();
    }
    out.sort_by(|x, y| x.pos.x.total_cmp(&y.pos.x).then(x.pos.y.total_cmp(&y.pos.y)));
    out
}

/// How close (m) a station must be to another road to join it.
pub const JOIN: f32 = 0.75;

/// Whether road `a` crosses road `b` at their stations `i` and `j` (rather than running
/// alongside it): `a`'s direction there isn't along `b`'s.
fn crosses(a: &Road, i: usize, b: &Road, j: usize) -> bool {
    a.dir[i].dot(b.dir[j]).abs() < 0.9
}

/// The land's height before the roads shape it.
pub fn base_height(p: Vec2, seed: u64) -> f32 {
    land::land(seed).height(p)
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Heights along a centreline: the land smoothed and held to `grade` (rise per metre), pinned
/// to `start` / `end` heights (if given) and blended into them over 40 m.
fn road_heights(land: &Land, pts: &[Vec2], grade: f32, start: Option<f32>, end: Option<f32>) -> Vec<f32> {
    let mut y: Vec<f32> = pts.iter().map(|p| land.height(*p)).collect();
    y = smooth(&y, 6);
    let n = y.len();
    let pin = |y: &mut Vec<f32>, metres: f32| {
        let blend = ((metres / STEP) as usize).min(n / 2).max(1);
        if let Some(a) = start {
            for (i, v) in y.iter_mut().take(blend).enumerate() {
                let t = smoothstep(i as f32 / blend as f32);
                *v = a + (*v - a) * t;
            }
        }
        if let Some(b) = end {
            for i in 0..blend {
                let t = smoothstep(i as f32 / blend as f32);
                let j = n - 1 - i;
                y[j] = b + (y[j] - b) * t;
            }
        }
    };
    pin(&mut y, 40.0);
    // Held to the grade, the pinned ends fixed.
    let max_step = grade * STEP;
    for _ in 0..4 {
        for i in 1..n {
            if end.is_some() && i == n - 1 {
                break;
            }
            y[i] = y[i].clamp(y[i - 1] - max_step, y[i - 1] + max_step);
        }
        for i in (0..n - 1).rev() {
            if start.is_some() && i == 0 {
                break;
            }
            y[i] = y[i].clamp(y[i + 1] - max_step, y[i + 1] + max_step);
        }
    }
    // Smoothing moves the pinned ends a little: put them back with a linear correction (a
    // smoothstep blend would concentrate it in one step on a road already at its grade).
    let mut y = smooth(&y, 2);
    let blend = ((12.0 / STEP) as usize).min(n / 2).max(1);
    if let Some(a) = start {
        let d = a - y[0];
        for (i, v) in y.iter_mut().take(blend).enumerate() {
            *v += d * (1.0 - i as f32 / blend as f32);
        }
    }
    if let Some(b) = end {
        let d = b - y[n - 1];
        for i in 0..blend {
            y[n - 1 - i] += d * (1.0 - i as f32 / blend as f32);
        }
    }
    y
}

fn surfaces(kind: RoadKind, pts: &[Vec2], land: &Land, seed: u64) -> Vec<Surface> {
    pts.iter()
        .enumerate()
        .map(|(i, p)| match kind {
            RoadKind::Street | RoadKind::BRoad => Surface::Tarmac,
            RoadKind::Gravel | RoadKind::Forest => {
                // Mud where the water runs, and in patches.
                let patch = noise(Vec2::splat(i as f32 * STEP / 90.0 + 3.7), seed) * 0.5 + 0.5;
                if kind == RoadKind::Forest && (land.wet(*p) > 0.62 || patch > 0.86) { Surface::Mud } else { Surface::Gravel }
            }
        })
        .collect()
}

fn widths(kind: RoadKind, n: usize, seed: u64) -> Vec<f32> {
    let w = kind.width();
    let vary = |amp: f32, wave: f32| -> Vec<f32> { (0..n).map(|i| w + amp * noise(Vec2::new(i as f32 * STEP / wave, 0.4), seed)).collect() };
    match kind {
        RoadKind::Gravel => vary(0.4, 220.0),
        RoadKind::Forest => vary(0.4, 160.0),
        _ => vec![w; n],
    }
}

/// Points every STEP m along a polyline.
fn resample(pts: &[Vec2], closed: bool) -> Vec<Vec2> {
    if pts.is_empty() {
        return Vec::new();
    }
    let mut src = pts.to_vec();
    if closed {
        src.push(pts[0]);
    }
    let mut out = vec![src[0]];
    let mut carry = 0.0;
    for w in src.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (b - a).length();
        let mut d = STEP - carry;
        while d <= len {
            out.push(a + (b - a) * (d / len));
            d += STEP;
        }
        carry = len - (d - STEP);
    }
    if closed {
        // Drop the duplicate of the first point if the last landed on it.
        if out.len() > 1 && out[out.len() - 1].distance(out[0]) < STEP * 0.5 {
            out.pop();
        }
    } else {
        // The road ends exactly at its last point (a junction is found there): a station just
        // short of it moves onto it, else the point is added.
        let end = src[src.len() - 1];
        // (Stations closer than half a step to the end go: no sliver of a last segment.)
        while out.len() > 1 && out[out.len() - 1].distance(end) < STEP * 0.5 {
            out.pop();
        }
        out.push(end);
    }
    out
}

/// A station of a road to join: its point, height and the direction across the road towards
/// `toward`.
#[derive(Clone, Copy, Debug)]
struct Attach {
    pos: Vec2,
    y: f32,
    out: Vec2,
}

/// The station of `roads[k]` nearest `near` (not within 40 m of either end), heading out
/// towards `toward`.
fn attach(roads: &[NetRoad], k: usize, near: Vec2, toward: Vec2) -> Attach {
    let r = &roads[k].road;
    let keep = (40.0 / STEP) as usize;
    // Nearest, on a fairly straight stretch (a junction in a hairpin doesn't build).
    let bend = |i: usize| r.curv[i.saturating_sub(15)..(i + 15).min(r.len())].iter().fold(0.0f32, |a, k| a.max(k.abs()));
    let score = |i: usize| Vec2::new(r.pts[i].x, r.pts[i].z).distance(near) + if bend(i) > 1.0 / 60.0 { 400.0 } else { 0.0 };
    let mut best = keep.min(r.len() - 1);
    for i in keep..r.len().saturating_sub(keep) {
        if score(i) < score(best) {
            best = i;
        }
    }
    let p = r.pts[best];
    let pos = Vec2::new(p.x, p.z);
    let side = r.dir[best].perp();
    let out = if side.dot(toward - pos) >= 0.0 { side } else { -side };
    Attach { pos, y: p.y, out }
}

/// The end of `roads[k]` (first or last station).
fn end_of(roads: &[NetRoad], k: usize, last: bool) -> Attach {
    let r = &roads[k].road;
    let i = if last { r.len() - 1 } else { 0 };
    let p = r.pts[i];
    let d = if last { r.dir[i] } else { -r.dir[i] };
    Attach { pos: Vec2::new(p.x, p.z), y: p.y, out: d }
}

struct Builder<'a> {
    seed: u64,
    land: &'a Land,
    planner: Planner<'a>,
    roads: Vec<NetRoad>,
}

impl Builder<'_> {
    /// Plans a road through `via` (start, waypoints, end), leaving `from` (if attached) across
    /// its road and arriving on `to`.
    fn road(&mut self, kind: RoadKind, rules: Rules, from: Option<Attach>, via: &[Vec2], to: Option<Attach>) -> Option<usize> {
        // Leave and arrive straight across (or along) the joined road: the planner runs between
        // points LEAD m out, and the last few metres are added straight.
        const LEAD: f32 = 16.0;
        let mut stops: Vec<Vec2> = Vec::new();
        if let Some(a) = from {
            stops.push(a.pos + a.out * LEAD);
        }
        stops.extend_from_slice(via);
        if let Some(b) = to {
            stops.push(b.pos + b.out * LEAD);
        }
        if stops.len() < 2 {
            return None;
        }
        let mut cells: Vec<Vec2> = Vec::new();
        let mut dir = from.map(|a| a.out);
        let legs = stops.len() - 1;
        for (li, w) in stops.windows(2).enumerate() {
            let arrive = if li + 1 == legs { to.map(|b| -b.out) } else { None };
            let leg = self.planner.route(w[0], dir, w[1], arrive, rules, 90.0)?;
            if let (Some(a), Some(b)) = (leg.get(leg.len().saturating_sub(2)), leg.last()) {
                dir = Some(*b - *a);
            }
            // Later legs keep off this one (a road doesn't cross itself).
            self.planner.take(&leg, 30.0);
            if !cells.is_empty() {
                cells.pop();
            }
            cells.extend(leg);
        }
        if let Some(a) = from {
            cells.splice(0..0, [a.pos, a.pos + a.out * (LEAD * 0.5)]);
        }
        if let Some(b) = to {
            cells.extend([b.pos + b.out * (LEAD * 0.5), b.pos]);
        }
        // (Untangled again after smoothing, and the cut rounded off.)
        let pts = resample(&plan::chaikin(&plan::untangle(&plan::chaikin(&plan::untangle(&cells), 4)), 2), false);
        if pts.len() < 4 {
            return None;
        }
        let k = self.roads.len();
        let sseed = self.seed ^ (0xA0 + k as u64);
        let y = road_heights(self.land, &pts, rules.max_grade, from.map(|a| a.y), to.map(|b| b.y));
        let n = pts.len();
        let road = Road::from_centreline(&pts, &y, widths(kind, n, sseed), surfaces(kind, &pts, self.land, sseed));
        self.planner.take(&pts, 40.0);
        self.roads.push(NetRoad::new(kind, road, false));
        Some(k)
    }
}

/// Builds the network for `seed` (once per seed; later calls share it).
pub fn generate(seed: u64) -> RoadNet {
    static CACHE: OnceLock<Mutex<HashMap<u64, RoadNet>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(n) = cache.lock().unwrap().get(&seed) {
        return n.clone();
    }
    let n = build(seed);
    cache.lock().unwrap().entry(seed).or_insert(n).clone()
}

fn build(seed: u64) -> RoadNet {
    let land = land::land(seed);
    let pl = plan::places(&land);
    let mut b = Builder { seed, land: &land, planner: Planner::new(&land), roads: Vec::new() };
    let tarmac_climb = Rules { max_turn: 3, w_turn: 0.9, ..Rules::TARMAC };
    let flat = Rules { max_grade: 0.08, ..Rules::TARMAC };

    // The valley B-road, west to east through the village and the service park.
    let valley = b.road(RoadKind::BRoad, flat, None, &[pl.west, pl.village, pl.park, pl.east], None).expect("valley road");
    // Tarmac hillclimb from its west end to the plateau.
    let w_end = end_of(&b.roads, valley, false);
    let climb = b.road(RoadKind::BRoad, tarmac_climb, Some(w_end), &[pl.pass_w], None);
    // Basin gravel from its east end, by the quarry, up the east wall.
    let e_end = end_of(&b.roads, valley, true);
    let basin = b.road(RoadKind::Gravel, Rules::GRAVEL, Some(e_end), &[pl.quarry, pl.pass_e], None);
    // The mountain pass over the plateau by the radio tower, joining the two climbs.
    let from = climb.map(|k| end_of(&b.roads, k, true));
    let to = basin.map(|k| end_of(&b.roads, k, true));
    let pass = b.road(RoadKind::Forest, Rules::FOREST, from, &[pl.tower], to);
    // Forestry road from the village up the escarpment past the logging camp to the pass.
    let v_at = attach(&b.roads, valley, pl.village + Vec2::new(-80.0, 0.0), pl.camp);
    let to = pass.map(|k| attach(&b.roads, k, pl.tower + Vec2::new(-500.0, -150.0), pl.camp));
    let escarp = b.road(RoadKind::Forest, Rules::FOREST, Some(v_at), &[pl.camp], to);
    // A logging track from the camp west to the hillclimb (a loop with both).
    if let (Some(e), Some(c)) = (escarp, climb) {
        let west = attach(&b.roads, c, pl.camp + Vec2::new(-700.0, 100.0), pl.camp);
        let a = attach(&b.roads, e, pl.camp, west.pos);
        b.road(RoadKind::Forest, Rules::FOREST, Some(a), &[], Some(west));
    }
    // The gully road from the service park up between escarpment and basin to the basin road.
    let p_at = attach(&b.roads, valley, pl.park, pl.park + Vec2::Y * 100.0);
    let to = basin.map(|k| attach(&b.roads, k, pl.quarry + Vec2::new(-150.0, 450.0), pl.park));
    b.road(RoadKind::Gravel, Rules::GRAVEL, Some(p_at), &[], to);
    // The southern loop: over the river, through the hills, back over the river.
    let s0 = attach(&b.roads, valley, pl.ford_w + Vec2::Y * 150.0, pl.ford_w);
    let s1 = attach(&b.roads, valley, pl.ford_e + Vec2::Y * 150.0, pl.ford_e);
    let bridge = Rules { bridge: true, ..Rules::GRAVEL };
    b.road(RoadKind::Gravel, bridge, Some(s0), &[pl.ford_w, pl.south, pl.ford_e], Some(s1));
    // Village lanes: a loop north of the B-road.
    let l0 = attach(&b.roads, valley, pl.village + Vec2::new(-170.0, 0.0), pl.village + Vec2::Y * 200.0);
    let l1 = attach(&b.roads, valley, pl.village + Vec2::new(170.0, 0.0), pl.village + Vec2::Y * 200.0);
    let lane = Rules { max_turn: 3, w_turn: 0.3, ..Rules::TARMAC };
    b.road(RoadKind::Street, lane, Some(l0), &[pl.village + Vec2::new(0.0, 150.0)], Some(l1));

    let roads = b.roads;
    let junctions = find_junctions(&roads);
    let town_y = land.coarse(pl.village);
    RoadNet { seed, roads, junctions, town_y, land }
}

/// A point's height in the finished world: the land, shaped by the nearest road.
pub fn height(net: &RoadNet, x: f32, z: f32) -> f32 {
    let p = Vec2::new(x, z);
    let base = net.land.height(p);
    match net.nearest(p, crate::stage::SHAPE_REACH) {
        Some(n) => crate::stage::shaped(base, &net.roads[n.road].road, &n.at),
        None => base,
    }
}

/// What the tyres are on at (x, z): a road, its verge, valley grass or rough ground.
pub fn surface(net: &RoadNet, x: f32, z: f32) -> Surface {
    let p = Vec2::new(x, z);
    match net.nearest(p, 14.0) {
        Some(n) => {
            let r = &net.roads[n.road].road;
            let half = r.half_width(&n.at);
            if n.at.lateral.abs() <= half {
                r.surface[n.at.i]
            } else if n.at.lateral.abs() <= half + 3.0 {
                Surface::Verge
            } else {
                off_road(net, p)
            }
        }
        None => off_road(net, p),
    }
}

fn off_road(net: &RoadNet, p: Vec2) -> Surface {
    if net.land.zone(p) == Zone::Valley { Surface::Verge } else { Surface::Rough }
}

/// Where to start: on the B-road by the service park, heading east.
pub fn spawn(net: &RoadNet) -> (Vec3, Vec2) {
    let park = net.places().park;
    let r = &net.roads[0].road;
    let i = r.nearest(park, 400.0).map_or(r.len() / 2, |p| p.i);
    let p = r.pts[i];
    (p, r.dir[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net() -> RoadNet {
        generate(0x0E11_0001)
    }

    #[test]
    fn deterministic_and_complete() {
        let a = build(0x0E11_0001);
        let b = build(0x0E11_0001);
        assert_eq!(a.roads.len(), b.roads.len());
        for (x, y) in a.roads.iter().zip(&b.roads) {
            assert_eq!(x.road.pts, y.road.pts);
        }
        let count = |k| a.roads.iter().filter(|r| r.kind == k).count();
        assert_eq!(count(RoadKind::BRoad), 2, "valley road and hillclimb");
        assert!(count(RoadKind::Gravel) >= 3, "gravel roads: {}", count(RoadKind::Gravel));
        assert!(count(RoadKind::Forest) >= 3, "forest roads: {}", count(RoadKind::Forest));
        assert!(count(RoadKind::Street) >= 1);
        // Plenty to drive: tens of km.
        assert!(a.length() > 20_000.0, "{} m", a.length());
    }

    #[test]
    fn roads_have_no_gaps() {
        for r in &net().roads {
            for w in r.road.pts.windows(2) {
                let d = w[0].distance(w[1]);
                assert!(d.is_finite() && d < 2.6 * STEP, "{:?}: {d} m between {} and {}", r.kind, w[0], w[1]);
            }
        }
    }

    #[test]
    fn everything_stays_inside_the_region() {
        let n = net();
        for r in &n.roads {
            for p in &r.road.pts {
                assert!(p.x.abs() < HALF && p.z.abs() < HALF, "{:?} at {p}", r.kind);
                assert!(p.is_finite());
            }
        }
    }

    #[test]
    fn no_dead_ends() {
        let n = net();
        for (k, r) in n.roads.iter().enumerate() {
            let (a, b) = (r.road.pts[0], r.road.pts[r.road.len() - 1]);
            eprintln!("road {k} {:?} {} m: {a} .. {b}", r.kind, r.road.length());
        }
        for j in &n.junctions {
            eprintln!("junction {} {:?}", j.pos, j.roads);
        }
        for (k, r) in n.roads.iter().enumerate() {
            for (i, end) in [(0, "start"), (r.road.len() - 1, "end")] {
                let joined = n.junctions.iter().any(|j| j.roads.iter().any(|&(rk, ri)| rk == k && ri.abs_diff(i) <= 1));
                // The valley road's ends are where the hill roads start.
                assert!(joined, "road {k} ({:?}) {end} joins nothing", r.kind);
            }
        }
    }

    #[test]
    fn grades_and_hairpins() {
        let n = net();
        let mut tightest = f32::MAX;
        for r in &n.roads {
            let (at, max) = r.road.pts.windows(2).map(|w| (w[1].y - w[0].y).abs() / STEP).enumerate().fold((0, 0.0f32), |a, (i, g)| if g > a.1 { (i, g) } else { a });
            if max >= 0.16 {
                for i in at.saturating_sub(8)..(at + 8).min(r.road.len()) {
                    eprintln!("  {i}: {}", r.road.pts[i]);
                }
            }
            // 14% at most, a little more for a few metres where a road meets another.
            assert!(max < 0.18, "{:?} climbs {max} at station {at}/{}", r.kind, r.road.len());
            if r.kind == RoadKind::Forest || r.kind == RoadKind::Gravel {
                let (i, k) = r.road.curv.iter().enumerate().fold((0, 0.0f32), |a, (i, k)| if k.abs() > a.1 { (i, k.abs()) } else { a });
                eprintln!("{:?}: tightest {} m at station {i}/{} {}", r.kind, 1.0 / k, r.road.len(), r.road.pts[i]);
                tightest = tightest.min(1.0 / k);
            }
        }
        // Real hairpins somewhere on the hill roads.
        assert!(tightest < 14.0, "tightest corner {tightest} m");
        assert!(tightest > 6.0, "tightest corner {tightest} m");
    }

    #[test]
    fn ground_is_level_across_roads() {
        let n = net();
        for r in &n.roads {
            for i in (10..r.road.len() - 10).step_by(97) {
                let p = r.road.pts[i];
                let side = Vec2::new(-r.road.dir[i].y, r.road.dir[i].x);
                let edge = Vec2::new(p.x, p.z) + side * (r.road.width[i] * 0.4);
                let h = height(&n, edge.x, edge.y);
                assert!((h - p.y).abs() < 0.8, "{:?} station {i}: ground {h} vs road {}", r.kind, p.y);
            }
        }
    }

    #[test]
    fn surfaces() {
        let n = net();
        let valley = &n.roads[0].road;
        let p = valley.pts[valley.len() / 2];
        assert_eq!(surface(&n, p.x, p.z), Surface::Tarmac, "valley road");
        let forest = n.roads.iter().find(|r| r.kind == RoadKind::Forest).unwrap();
        let p = forest.road.pts[forest.road.len() / 2];
        assert!(matches!(surface(&n, p.x, p.z), Surface::Gravel | Surface::Mud));
    }
}

#[cfg(test)]
mod map {
    /// `PACENOTE_MAP_SVG=out.svg cargo test --lib open::roads::map -- --ignored`: the network
    /// drawn from above (grey = ground height, colours = road kinds).
    #[test]
    #[ignore]
    fn draw() {
        let Ok(out) = std::env::var("PACENOTE_MAP_SVG") else { return };
        let n = super::generate(crate::open::SEED);
        let mut s = String::from(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-2000 -2000 4000 4000" width="1000" height="1000"><rect x="-2000" y="-2000" width="4000" height="4000" fill="#1b1f1a"/>"##);
        for r in &n.roads {
            let (c, w) = match r.kind {
                super::RoadKind::Street => ("#c8c8c8", 10),
                super::RoadKind::BRoad => ("#e0a030", 14),
                super::RoadKind::Gravel => ("#7fb0e0", 10),
                super::RoadKind::Forest => ("#80c070", 8),
            };
            let d: Vec<String> = r.road.pts.iter().step_by(3).map(|p| format!("{:.0},{:.0}", p.x, p.z)).collect();
            s += &format!(r##"<polyline fill="none" stroke="{c}" stroke-width="{w}" points="{}"/>"##, d.join(" "));
        }
        for j in &n.junctions {
            s += &format!(r##"<circle cx="{:.0}" cy="{:.0}" r="14" fill="#e05050"/>"##, j.pos.x, j.pos.y);
        }
        s += "</svg>";
        std::fs::write(out, s).unwrap();
    }

    /// `PACENOTE_MAP_PPM=out.ppm cargo test --lib open::roads::map -- --ignored`: hillshade with
    /// the roads (yellow B-road, blue gravel, green forest, white lanes), junctions red.
    #[test]
    #[ignore]
    fn picture() {
        use bevy::math::{Vec2, Vec3};
        let Ok(out) = std::env::var("PACENOTE_MAP_PPM") else { return };
        let n = super::generate(crate::open::SEED);
        let px = 1000usize;
        let mut img = vec![[0u8; 3]; px * px];
        for row in 0..px {
            for col in 0..px {
                let p = Vec2::new(-2000.0 + (col as f32 + 0.5) * 4.0, 2000.0 - (row as f32 + 0.5) * 4.0);
                let e = 4.0;
                let h = |q: Vec2| n.land.height(q);
                let nrm = Vec3::new(-(h(p + Vec2::X * e) - h(p - Vec2::X * e)), 2.0 * e, -(h(p + Vec2::Y * e) - h(p - Vec2::Y * e))).normalize();
                let shade = nrm.dot(Vec3::new(-0.5, 0.7, 0.5).normalize()).max(0.0);
                let t = ((h(p) - 80.0) / 400.0).clamp(0.0, 1.0);
                let c = [0.35 + 0.45 * t, 0.5 + 0.2 * t, 0.3 + 0.5 * t];
                img[row * px + col] = c.map(|ch| ((ch * (0.35 + 0.75 * shade)).clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
        let mut dot = |p: Vec2, r: i32, c: [u8; 3]| {
            let (cx, cy) = (((p.x + 2000.0) / 4.0) as i32, ((2000.0 - p.y) / 4.0) as i32);
            for dy in -r..=r {
                for dx in -r..=r {
                    let (x, y) = (cx + dx, cy + dy);
                    if dx * dx + dy * dy <= r * r && x >= 0 && y >= 0 && (x as usize) < px && (y as usize) < px {
                        img[y as usize * px + x as usize] = c;
                    }
                }
            }
        };
        for r in &n.roads {
            let c = match r.kind {
                super::RoadKind::Street => [240, 240, 240],
                super::RoadKind::BRoad => [240, 190, 40],
                super::RoadKind::Gravel => [90, 160, 240],
                super::RoadKind::Forest => [60, 200, 70],
            };
            for q in &r.road.pts {
                dot(Vec2::new(q.x, q.z), 1, c);
            }
        }
        for j in &n.junctions {
            dot(j.pos, 4, [230, 40, 40]);
        }
        let mut f = format!("P6 {px} {px} 255\n").into_bytes();
        for c in img {
            f.extend_from_slice(&c);
        }
        std::fs::write(out, f).unwrap();
    }
}
