//! The road network of the open world: a town street grid, a highway ring, country roads from
//! town to the ring, and winding forest roads off the ring. Pure and deterministic (libm, no
//! platform floats), like `stage.rs`: the same seed gives the same bits on every machine.

use std::collections::HashMap;

use bevy::math::{Vec2, Vec3};

use crate::stage::{Rng, Road, RoadPos, STEP, Surface, fbm, noise, smooth};

/// Half the side of the region (m): the world spans -HALF..HALF in x and z.
pub const HALF: f32 = 2000.0;
/// Town streets run on this grid (m), from -TOWN to TOWN.
pub const TOWN: f32 = 330.0;
pub const BLOCK: f32 = 110.0;
/// The town plateau is flat inside this (max-norm) distance and blends out by the next.
pub const PLATEAU: (f32, f32) = (400.0, 640.0);
/// Mean radius of the highway ring (m).
pub const RING_R: f32 = 1450.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoadKind {
    Street,
    Highway,
    Country,
    Forest,
}

impl RoadKind {
    pub fn width(self) -> f32 {
        match self {
            RoadKind::Street => 11.0,
            RoadKind::Highway => 14.0,
            RoadKind::Country => 8.0,
            RoadKind::Forest => 6.6,
        }
    }
}

/// One road of the network.
#[derive(Clone, Debug)]
pub struct NetRoad {
    pub kind: RoadKind,
    pub road: Road,
    /// The last point joins the first (the ring).
    pub closed: bool,
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
    /// The town plateau height (m).
    pub town_y: f32,
}

/// The nearest road to a point.
#[derive(Clone, Copy, Debug)]
pub struct NetPos {
    pub road: usize,
    pub at: RoadPos,
}

const REACH: f32 = 60.0;

impl NetRoad {
    fn new(kind: RoadKind, road: Road, closed: bool) -> NetRoad {
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in &road.pts {
            lo = lo.min(Vec2::new(p.x, p.z));
            hi = hi.max(Vec2::new(p.x, p.z));
        }
        NetRoad { kind, road, closed, lo: lo - REACH, hi: hi + REACH }
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
}

/// Hills before the roads shape them; flat in town.
pub fn base_height(p: Vec2, seed: u64) -> f32 {
    let s = seed ^ 0x0BE7;
    let warp = Vec2::new(fbm(p / 1600.0, 3, s ^ 1), fbm(p / 1600.0, 3, s ^ 2)) * 160.0;
    let q = (p + warp) / 820.0;
    let h = fbm(q, 6, s) * 58.0;
    let ridge = 1.0 - fbm(q * 0.7, 4, s ^ 3).abs();
    let detail = fbm(p / 40.0, 3, s ^ 4) * 1.0;
    let natural = h + (ridge * ridge - 0.4) * 18.0 + detail;
    let town = town_height(seed);
    let d = p.x.abs().max(p.y.abs());
    let t = smoothstep(((d - PLATEAU.0) / (PLATEAU.1 - PLATEAU.0)).clamp(0.0, 1.0));
    town + (natural - town) * t
}

/// The plateau the town sits on: the natural height at the centre.
fn town_height(seed: u64) -> f32 {
    let s = seed ^ 0x0BE7;
    let p = Vec2::ZERO;
    let warp = Vec2::new(fbm(p / 1600.0, 3, s ^ 1), fbm(p / 1600.0, 3, s ^ 2)) * 160.0;
    fbm((p + warp) / 820.0, 6, s) * 58.0
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Heights along a centreline: the ground smoothed and held to `grade` (rise per metre),
/// pinned to `start` / `end` heights (if given) and blended into them over 120 m.
fn road_heights(pts: &[Vec2], seed: u64, grade: f32, start: Option<f32>, end: Option<f32>, closed: bool) -> Vec<f32> {
    let mut y: Vec<f32> = pts.iter().map(|p| base_height(*p, seed)).collect();
    y = smooth(&y, 20);
    let max_step = grade * STEP;
    for _ in 0..2 {
        for i in 1..y.len() {
            y[i] = y[i].clamp(y[i - 1] - max_step, y[i - 1] + max_step);
        }
        for i in (0..y.len() - 1).rev() {
            y[i] = y[i].clamp(y[i + 1] - max_step, y[i + 1] + max_step);
        }
        if closed {
            let n = y.len();
            y[0] = y[0].clamp(y[n - 1] - max_step, y[n - 1] + max_step);
        }
    }
    let n = y.len();
    let blend = (120.0 / STEP) as usize;
    if let Some(a) = start {
        for i in 0..blend.min(n) {
            let t = smoothstep(i as f32 / blend as f32);
            y[i] = a + (y[i] - a) * t;
        }
    }
    if let Some(b) = end {
        for i in 0..blend.min(n) {
            let t = smoothstep(i as f32 / blend as f32);
            let j = n - 1 - i;
            y[j] = b + (y[j] - b) * t;
        }
    }
    smooth(&y, 4)
}

/// Points every STEP m along a straight line from `a` to `b` (both included).
fn line(a: Vec2, b: Vec2) -> Vec<Vec2> {
    let n = ((b - a).length() / STEP).round().max(1.0) as usize;
    (0..=n).map(|i| a + (b - a) * (i as f32 / n as f32)).collect()
}

/// Coarse occupancy of what's built, for keeping new roads apart.
#[derive(Default)]
struct Occupied {
    cells: HashMap<(i32, i32), Vec<Vec2>>,
}

const OCC: f32 = 25.0;

impl Occupied {
    fn add(&mut self, pts: &[Vec2]) {
        for p in pts.iter().step_by(3) {
            self.cells.entry(((p.x / OCC).floor() as i32, (p.y / OCC).floor() as i32)).or_default().push(*p);
        }
    }

    fn near(&self, p: Vec2, r: f32) -> bool {
        let (x0, x1) = (((p.x - r) / OCC).floor() as i32, ((p.x + r) / OCC).floor() as i32);
        let (z0, z1) = (((p.y - r) / OCC).floor() as i32, ((p.y + r) / OCC).floor() as i32);
        for z in z0..=z1 {
            for x in x0..=x1 {
                if let Some(v) = self.cells.get(&(x, z))
                    && v.iter().any(|q| q.distance_squared(p) < r * r)
                {
                    return true;
                }
            }
        }
        false
    }
}

fn surfaces(kind: RoadKind, n: usize, gravel: bool, seed: u64) -> Vec<Surface> {
    (0..n)
        .map(|i| match kind {
            RoadKind::Street | RoadKind::Highway => Surface::Tarmac,
            RoadKind::Country if !gravel => Surface::Tarmac,
            _ => {
                // Gravel with muddy patches.
                if noise(Vec2::splat(i as f32 * STEP / 90.0 + 3.7), seed) * 0.5 + 0.5 > 0.82 { Surface::Mud } else { Surface::Gravel }
            }
        })
        .collect()
}

fn widths(kind: RoadKind, n: usize, seed: u64) -> Vec<f32> {
    let w = kind.width();
    match kind {
        RoadKind::Forest => (0..n)
            .map(|i| {
                let v = noise(Vec2::splat(i as f32 * STEP / 160.0), seed) * 0.5 + 0.5;
                w - 1.2 * v * v * v
            })
            .collect(),
        _ => vec![w; n],
    }
}

/// The ring: a closed loop around the town, its radius wandering with the angle.
fn ring_points(seed: u64) -> Vec<Vec2> {
    let circumference = core::f32::consts::TAU * RING_R;
    let n = (circumference / STEP) as usize;
    let mut pts: Vec<Vec2> = (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * core::f32::consts::TAU;
            let (s, c) = (libm::sinf(a), libm::cosf(a));
            let wob = noise(Vec2::new(c, s) * 2.2, seed ^ 0x2196) * 170.0 + noise(Vec2::new(c, s) * 6.0, seed ^ 0x2197) * 40.0;
            Vec2::new(c, s) * (RING_R + wob)
        })
        .collect();
    // Even spacing (the wobble stretches some stretches).
    pts = resample(&pts, true);
    pts
}

/// Points every STEP m along a polyline.
fn resample(pts: &[Vec2], closed: bool) -> Vec<Vec2> {
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
    } else if out.last().is_some_and(|l| l.distance(src[src.len() - 1]) > STEP * 0.25) {
        out.push(src[src.len() - 1]);
    }
    out
}

/// A road from `a` to `b` that bends: the straight line, pushed sideways by smooth noise up to
/// `amp` m, faded to nothing at both ends (it leaves and arrives straight). Resampled to STEP.
fn curve(a: Vec2, b: Vec2, amp: f32, seed: u64) -> Vec<Vec2> {
    let len = (b - a).length();
    let side = (b - a).perp() / len.max(1e-3);
    let n = (len / 4.0).max(2.0) as usize;
    let raw: Vec<Vec2> = (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            // sin(π) is a hair below zero in f32: clamp before the fractional power (NaN otherwise).
            let fade = libm::sinf(t * core::f32::consts::PI).max(0.0).powf(0.7);
            let off = (noise(Vec2::new(t * len / 520.0, 0.3), seed) * 0.75 + noise(Vec2::new(t * len / 190.0, 2.1), seed ^ 5) * 0.25) * amp * fade;
            a + (b - a) * t + side * off
        })
        .collect();
    let mut pts = resample(&raw, false);
    if let Some(l) = pts.last_mut() {
        *l = b;
    }
    pts
}

/// A winding road from `start` heading `h`, walking in STEP m steps for up to `len` m. It keeps
/// inside `inside` (a test on the next point) and away from `occ`; with `target`, it is drawn
/// towards it and stops on reaching it.
#[allow(clippy::too_many_arguments)]
fn wander(start: Vec2, mut h: f32, len: f32, wiggle: f32, seed: u64, occ: &Occupied, inside: impl Fn(Vec2) -> bool, target: Option<(Vec2, f32)>) -> Vec<Vec2> {
    let mut pts = vec![start];
    let mut p = start;
    let max_turn = STEP / 38.0; // tightest radius ~38 m
    let steps = (len / STEP) as usize;
    let clear = |q: Vec2, own: &[Vec2]| {
        inside(q) && !occ.near(q, 45.0) && own.iter().rev().skip((60.0 / STEP) as usize).step_by(3).all(|o| o.distance_squared(q) > 45.0 * 45.0)
    };
    let dir = |a: f32| Vec2::new(libm::cosf(a), libm::sinf(a));
    let mut boxed = 0;
    for i in 0..steps {
        let s = i as f32 * STEP;
        // Desired turn: noise (bends), plus pull to the target.
        let mut want = noise(Vec2::new(s / 140.0, 0.5), seed) * wiggle + noise(Vec2::new(s / 45.0, 1.5), seed ^ 9) * wiggle * 0.35;
        if let Some((t, _)) = target {
            let to = t - p;
            let a = libm::atan2f(to.y, to.x);
            let mut d = a - h;
            while d > core::f32::consts::PI {
                d -= core::f32::consts::TAU;
            }
            while d < -core::f32::consts::PI {
                d += core::f32::consts::TAU;
            }
            want += d * 0.02;
        }
        // Look ahead: steer to whichever side is clear.
        let ahead = |a: f32| p + dir(a) * 40.0;
        if !clear(ahead(h + want), &pts) {
            let (l, r) = (clear(ahead(h + 0.6), &pts), clear(ahead(h - 0.6), &pts));
            want = match (l, r) {
                (true, false) => max_turn,
                (false, true) => -max_turn,
                (true, true) => if noise(Vec2::new(s / 30.0, 7.0), seed) > 0.0 { max_turn } else { -max_turn },
                (false, false) => {
                    boxed += 1;
                    if boxed > 12 {
                        break;
                    }
                    max_turn
                }
            };
        } else {
            boxed = 0;
        }
        h += want.clamp(-max_turn, max_turn);
        p += dir(h) * STEP;
        pts.push(p);
        if let Some((t, reach)) = target
            && p.distance(t) < reach
        {
            break;
        }
    }
    pts
}

/// Builds the network for `seed`.
pub fn generate(seed: u64) -> RoadNet {
    let town_y = town_height(seed);
    let mut roads: Vec<NetRoad> = Vec::new();
    let mut junctions: Vec<Junction> = Vec::new();
    let mut occ = Occupied::default();
    let mut rng = Rng::new(seed ^ 0x5EED);

    // Town: a street grid, flat.
    let lines: Vec<f32> = {
        let n = (2.0 * TOWN / BLOCK).round() as i32;
        (0..=n).map(|i| -TOWN + i as f32 * BLOCK).collect()
    };
    let mut street_of = HashMap::new(); // (axis, line index) -> road index
    for (axis, li) in [0, 1].iter().flat_map(|a| (0..lines.len()).map(move |i| (*a, i))) {
        let c = lines[li];
        let (a, b) = if axis == 0 { (Vec2::new(-TOWN, c), Vec2::new(TOWN, c)) } else { (Vec2::new(c, -TOWN), Vec2::new(c, TOWN)) };
        let pts = line(a, b);
        let n = pts.len();
        let road = Road::from_centreline(&pts, &vec![town_y; n], widths(RoadKind::Street, n, seed), surfaces(RoadKind::Street, n, false, seed));
        street_of.insert((axis, li), roads.len());
        occ.add(&pts);
        roads.push(NetRoad::new(RoadKind::Street, road, false));
    }
    for (xi, &x) in lines.iter().enumerate() {
        for (zi, &z) in lines.iter().enumerate() {
            let ew = street_of[&(0, zi)]; // runs along x at z
            let ns = street_of[&(1, xi)]; // runs along z at x
            let station = |c: f32| ((c + TOWN) / STEP).round() as usize;
            junctions.push(Junction { pos: Vec2::new(x, z), roads: vec![(ew, station(x)), (ns, station(z))] });
        }
    }

    // The highway ring.
    let ring_pts = ring_points(seed);
    let ring_y = road_heights(&ring_pts, seed, 0.06, None, None, true);
    let n = ring_pts.len();
    let ring = roads.len();
    roads.push(NetRoad::new(RoadKind::Highway, Road::from_centreline(&ring_pts, &ring_y, widths(RoadKind::Highway, n, seed), surfaces(RoadKind::Highway, n, false, seed)), true));
    occ.add(&ring_pts);
    let ring_at = |a: f32| -> (usize, Vec2) {
        // The ring point in direction `a` from the centre.
        let d = Vec2::new(libm::cosf(a), libm::sinf(a));
        let mut best = (0, f32::MIN);
        for (i, p) in ring_pts.iter().enumerate() {
            let dot = p.normalize_or_zero().dot(d);
            if dot > best.1 {
                best = (i, dot);
            }
        }
        (best.0, ring_pts[best.0])
    };

    // Country roads: from the end of the town's middle streets out to the ring.
    let mid = lines.len() / 2;
    let spokes = [
        (0.0f32, Vec2::new(TOWN, 0.0), street_of[&(0, mid)], false),
        (core::f32::consts::FRAC_PI_2, Vec2::new(0.0, TOWN), street_of[&(1, mid)], true),
        (core::f32::consts::PI, Vec2::new(-TOWN, 0.0), street_of[&(0, mid)], false),
        (-core::f32::consts::FRAC_PI_2, Vec2::new(0.0, -TOWN), street_of[&(1, mid)], true),
    ];
    let inside_region = |q: Vec2| q.x.abs() < HALF - 60.0 && q.y.abs() < HALF - 60.0;
    for (k, (a, start, street, gravel)) in spokes.into_iter().enumerate() {
        let (ri, target) = ring_at(a);
        let sseed = seed ^ (0xC0 + k as u64);
        let pts = curve(start, target, 140.0, sseed);
        let y = road_heights(&pts, seed, 0.08, Some(town_y), Some(ring_y[ri]), false);
        let n = pts.len();
        let kind = RoadKind::Country;
        let idx = roads.len();
        roads.push(NetRoad::new(kind, Road::from_centreline(&pts, &y, widths(kind, n, sseed), surfaces(kind, n, gravel, sseed)), false));
        occ.add(&pts);
        let town_end = if start.x > 0.0 || start.y > 0.0 { roads[street].road.len() - 1 } else { 0 };
        junctions.push(Junction { pos: start, roads: vec![(street, town_end), (idx, 0)] });
        junctions.push(Junction { pos: target, roads: vec![(ring, ri), (idx, n - 1)] });
    }

    // Forest roads: from the ring into the quadrants between the spokes, winding, dead ends.
    for k in 0..4 {
        let a = core::f32::consts::FRAC_PI_4 + k as f32 * core::f32::consts::FRAC_PI_2 + rng.range(-0.15, 0.15);
        let (ri, start) = ring_at(a);
        let fseed = seed ^ (0xF0 + k as u64);
        // Inward, between the town (plus a margin) and the ring.
        let inner = TOWN + 160.0;
        let ring_r = |q: Vec2| {
            let (_, rp) = ring_at(libm::atan2f(q.y, q.x));
            rp.length()
        };
        let inside = |q: Vec2| {
            let d = q.length();
            q.x.abs().max(q.y.abs()) > inner && d < ring_r(q) - 70.0 && inside_region(q)
        };
        // Leave the ring heading inward, then wander. Skip the ring's own occupancy at the
        // start by starting 50 m in.
        let h = a + core::f32::consts::PI;
        let first = start + Vec2::new(libm::cosf(h), libm::sinf(h)) * 50.0;
        let mut pts = line(start, first);
        let more = wander(first, h, 3200.0, 0.022, fseed, &occ, inside, None);
        pts.extend_from_slice(&more[1..]);
        let pts = resample(&pts, false);
        if pts.len() < (600.0 / STEP) as usize {
            continue;
        }
        let y = road_heights(&pts, seed, 0.11, Some(ring_y[ri]), None, false);
        let n = pts.len();
        let kind = RoadKind::Forest;
        let idx = roads.len();
        roads.push(NetRoad::new(kind, Road::from_centreline(&pts, &y, widths(kind, n, fseed), surfaces(kind, n, true, fseed)), false));
        occ.add(&pts);
        junctions.push(Junction { pos: start, roads: vec![(ring, ri), (idx, 0)] });
    }

    RoadNet { seed, roads, junctions, town_y }
}

/// A point's height in the finished world: the hills, shaped by the nearest road.
pub fn height(net: &RoadNet, x: f32, z: f32) -> f32 {
    let p = Vec2::new(x, z);
    let base = base_height(p, net.seed);
    match net.nearest(p, crate::stage::SHAPE_REACH) {
        Some(n) => crate::stage::shaped(base, &net.roads[n.road].road, &n.at),
        None => base,
    }
}

/// What the tyres are on at (x, z): a road, its verge, town grass or rough ground.
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
                rough_or_grass(p)
            }
        }
        None => rough_or_grass(p),
    }
}

fn rough_or_grass(p: Vec2) -> Surface {
    if p.x.abs().max(p.y.abs()) < PLATEAU.0 { Surface::Verge } else { Surface::Rough }
}

/// Where to start: on the town's middle east-west street, near its west end, heading east.
pub fn spawn(net: &RoadNet) -> (Vec3, Vec2) {
    let p = Vec2::new(-TOWN + 40.0, 0.0);
    (Vec3::new(p.x, net.town_y, p.y), Vec2::X)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net() -> RoadNet {
        generate(0x0E11_0001)
    }

    #[test]
    fn deterministic_and_complete() {
        let (a, b) = (net(), net());
        assert_eq!(a.roads.len(), b.roads.len());
        for (x, y) in a.roads.iter().zip(&b.roads) {
            assert_eq!(x.road.pts, y.road.pts);
        }
        let count = |k| a.roads.iter().filter(|r| r.kind == k).count();
        assert_eq!(count(RoadKind::Highway), 1);
        assert_eq!(count(RoadKind::Country), 4);
        assert_eq!(count(RoadKind::Street), 14);
        assert!(count(RoadKind::Forest) >= 3, "forest roads: {}", count(RoadKind::Forest));
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
    fn spokes_join_town_and_ring() {
        let n = net();
        for r in n.roads.iter().filter(|r| r.kind == RoadKind::Country) {
            let (a, b) = (r.road.pts[0], *r.road.pts.last().unwrap());
            assert!(Vec2::new(a.x, a.z).length() < TOWN * 1.5, "starts in town: {a}");
            assert!((a.y - n.town_y).abs() < 0.5, "level with town at the start");
            let ring = n.roads.iter().find(|r| r.kind == RoadKind::Highway).unwrap();
            let at = ring.road.nearest(Vec2::new(b.x, b.z), 5.0).expect("ends on the ring");
            assert!((ring.road.height_at(&at) - b.y).abs() < 0.6, "level with the ring at the end");
        }
    }

    #[test]
    fn forest_roads_keep_apart_from_other_roads() {
        let n = net();
        for (k, r) in n.roads.iter().enumerate().filter(|(_, r)| r.kind == RoadKind::Forest) {
            // Past the first 120 m (where it leaves the ring) nothing else is near.
            for p in r.road.pts.iter().skip(60).step_by(5) {
                let q = Vec2::new(p.x, p.z);
                for (j, o) in n.roads.iter().enumerate() {
                    if j == k {
                        continue;
                    }
                    assert!(o.road.nearest(q, 30.0).is_none(), "forest road {k} runs within 30 m of {:?} at {q}", o.kind);
                }
            }
        }
    }

    #[test]
    fn ground_is_level_across_roads_and_flat_in_town() {
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
        let h = height(&n, 150.0, -60.0);
        assert!((h - n.town_y).abs() < 0.8, "town is flat: {h} vs {}", n.town_y);
    }

    #[test]
    fn surfaces() {
        let n = net();
        assert_eq!(surface(&n, 0.0, 0.0), Surface::Tarmac, "town crossroads");
        assert_eq!(surface(&n, 55.0, 55.0), Surface::Verge, "town block is grass");
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
                super::RoadKind::Street => ("#c8c8c8", 11),
                super::RoadKind::Highway => ("#e0a030", 16),
                super::RoadKind::Country => ("#7fb0e0", 10),
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
}
