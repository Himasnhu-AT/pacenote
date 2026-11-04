//! Junction geometry for the drawn roads (pure, deterministic; `art::junctions` meshes it).
//!
//! For every node of the network (junctions closer than [`MERGE`] are one, as in the lane
//! graph) each road meeting there gives one or two *arms* (one at a road's end, two where it
//! runs through). The arms' edges (the drawn ribbon's, half the width plus [`OVERHANG`]) are
//! intersected with their neighbours', the corners filleted ([`radius`]: tighter in town, wide
//! on the ring), and the arms cut back to the first whole station past the fillets: the
//! *mouth*. The outline (each mouth's ribbon row, then the corner to the next arm) is the
//! junction's tarmac; the ribbons stop at the mouths, so nothing is drawn twice.
//!
//! Visual only: the car still drives on the terrain heightfield and `Road::height_at`.

use bevy::math::Vec2;

use super::roads::{RoadKind, RoadNet};
use crate::stage::{Road, STEP};

/// The drawn ribbon reaches this far past the road's edge (m).
pub const OVERHANG: f32 = 0.4;
/// Kerb corner radius in town (m, at the street edge): the sidewalk corners follow it.
pub const TOWN_KERB_R: f32 = 7.0;
/// Radius where the outline turns outward (the outside of a bend).
const CONVEX_R: f32 = 3.0;
/// Junctions closer than this are one node (same as the lane graph's).
pub const MERGE: f32 = 10.0;
/// Longest tangent a fillet may have (acute corners would otherwise run far down the road).
const MAX_TANGENT: f32 = 40.0;

/// Concave corner radius at the road edge for a road kind (m): the corner between two arms
/// takes the smaller of theirs.
pub fn radius(kind: RoadKind) -> f32 {
    match kind {
        RoadKind::Street => TOWN_KERB_R,
        RoadKind::BRoad => 22.0,
        RoadKind::Gravel => 18.0,
        RoadKind::Forest => 15.0,
    }
}

/// Lateral sample positions (m, signed, ascending) across a ribbon of half-width `e` (its
/// edge): fine near the middle for the wheel-line shading.
pub fn ribbon_columns(e: f32) -> Vec<f32> {
    let inner = [0.0f32, 0.35, 0.52, 0.76, 1.0, 1.3, 1.7, 2.2, 2.8];
    let mut v: Vec<f32> = inner.iter().copied().filter(|&x| x < e - 0.25).collect();
    v.push(e);
    let mut all: Vec<f32> = v.iter().rev().map(|x| -x).filter(|x| *x != 0.0).collect();
    all.extend(v);
    all
}

/// The drawn half-width at a station.
pub fn ribbon_half(road: &Road, i: usize) -> f32 {
    road.width[i] * 0.5 + OVERHANG
}

/// One road leaving a junction.
#[derive(Clone, Debug, PartialEq)]
pub struct Arm {
    pub road: usize,
    pub kind: RoadKind,
    /// Station at the junction, and the way the arm runs (+1: towards higher stations).
    pub at: usize,
    pub sign: i32,
    /// Station where the ribbon starts again, and how many stations from `at` that is.
    pub mouth: usize,
    pub steps: usize,
    /// Whether the road ends at this junction (else it runs through).
    pub ends: bool,
}

/// A corner of the outline between two neighbouring arms.
#[derive(Clone, Debug, PartialEq)]
pub struct Corner {
    /// From the first arm's mouth edge to the next arm's (both included), along the outline.
    pub pts: Vec<Vec2>,
    /// Turns into the road area (the inside of a crossing's corner): the kerb side.
    pub concave: bool,
    /// Fillet radius used at the drawn edge (0: a sharp or straight corner).
    pub radius: f32,
}

/// A junction's drawn geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct JunctionGeom {
    pub pos: Vec2,
    /// Arms counter-clockwise (in x, z) from +x.
    pub arms: Vec<Arm>,
    /// `corners[k]` joins `arms[k]` to `arms[k + 1]`.
    pub corners: Vec<Corner>,
    /// The outline, counter-clockwise in (x, z): each arm's mouth row (the ribbon's columns,
    /// in order), then its corner (without the end points, which are mouth points).
    pub outline: Vec<Vec2>,
    /// Range of `outline` holding each arm's mouth row.
    pub mouths: Vec<std::ops::Range<usize>>,
}

impl JunctionGeom {
    /// In town (every arm a street, or the town's edge where the country roads leave).
    pub fn town(&self) -> bool {
        self.arms.iter().any(|a| a.kind == RoadKind::Street)
    }

    /// The outline's area (m², positive: counter-clockwise).
    pub fn area(&self) -> f32 {
        signed_area(&self.outline)
    }
}

fn perp(v: Vec2) -> Vec2 {
    Vec2::new(-v.y, v.x)
}

fn xz(p: bevy::math::Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}

/// Station `m` steps from `at` in direction `sign`, wrapping on a closed road.
fn step(n: usize, closed: bool, at: usize, sign: i32, m: usize) -> Option<usize> {
    let j = at as i64 + sign as i64 * m as i64;
    if closed {
        Some(j.rem_euclid(n as i64) as usize)
    } else if j < 0 || j >= n as i64 {
        None
    } else {
        Some(j as usize)
    }
}

/// A fillet between line A (through `pa`, outward `a`: travelled inward along −a) and line B
/// (through `pb`, outward `b`: travelled outward along +b).
#[derive(Clone, Copy, Debug)]
struct Fillet {
    /// Distances from `pa` / `pb` inward to the lines' crossing.
    s: f32,
    u: f32,
    /// Tangent length from the crossing, radius, signed turn (+ = left, convex).
    t: f32,
    r: f32,
    turn: f32,
}

fn fillet(pa: Vec2, a: Vec2, pb: Vec2, b: Vec2, r_concave: f32) -> Option<Fillet> {
    // pa − s·a = pb − u·b  →  s·a − u·b = pa − pb.
    let det = a.perp_dot(-b);
    let turn = libm::atan2f((-a).perp_dot(b), (-a).dot(b));
    if det.abs() < 0.05 || turn.abs() < 0.05 {
        return None; // (nearly) straight on: no corner
    }
    let rhs = pa - pb;
    let s = rhs.perp_dot(-b) / det;
    let u = a.perp_dot(rhs) / det;
    let r = if turn > 0.0 { CONVEX_R } else { r_concave };
    let half = libm::tanf(turn.abs() * 0.5);
    let t = (r * half).min(MAX_TANGENT);
    Some(Fillet { s, u, t, r: t / half, turn })
}

/// The arc of a fillet from line A's tangent point to line B's (both included).
fn arc(pa: Vec2, a: Vec2, f: &Fillet) -> Vec<Vec2> {
    let u_in = -a;
    let t1 = pa + u_in * (f.s - f.t);
    if f.t < 1e-3 {
        return vec![t1];
    }
    let side = if f.turn > 0.0 { 1.0 } else { -1.0 };
    let c = t1 + perp(u_in) * side * f.r;
    let d = t1 - c;
    let phi = libm::atan2f(d.y, d.x);
    let n = ((f.turn.abs() / 0.14).ceil() as usize).max(1);
    (0..=n)
        .map(|i| {
            let ang = phi + f.turn * i as f32 / n as f32;
            c + Vec2::new(libm::cosf(ang), libm::sinf(ang)) * f.r
        })
        .collect()
}

/// The nodes of the network: (position, the (road, station) pairs meeting there).
pub fn nodes(net: &RoadNet) -> Vec<(Vec2, Vec<(usize, usize)>)> {
    let mut nodes: Vec<(Vec2, Vec<(usize, usize)>)> = Vec::new();
    for j in &net.junctions {
        let k = match nodes.iter().position(|(p, _)| p.distance(j.pos) < MERGE) {
            Some(k) => k,
            None => {
                nodes.push((j.pos, Vec::new()));
                nodes.len() - 1
            }
        };
        for rs in &j.roads {
            if !nodes[k].1.contains(rs) {
                nodes[k].1.push(*rs);
            }
        }
    }
    nodes
}

/// Geometry for every junction of `net` (those whose outline comes out valid; see the tests).
pub fn build(net: &RoadNet) -> Vec<JunctionGeom> {
    nodes(net).into_iter().filter_map(|(pos, pairs)| junction(net, pos, &pairs)).collect()
}

/// One junction's geometry, or `None` if it can't be built cleanly (left as plain ribbons).
pub fn junction(net: &RoadNet, c: Vec2, pairs: &[(usize, usize)]) -> Option<JunctionGeom> {
    // Arms, with a first direction (the chord out to 12 m).
    struct Raw {
        road: usize,
        at: usize,
        sign: i32,
        ends: bool,
        d: Vec2,
    }
    let mut raw: Vec<Raw> = Vec::new();
    for &(r, at) in pairs {
        let nr = &net.roads[r];
        let n = nr.road.len();
        let ends = !nr.closed && (at == 0 || at == n - 1);
        for sign in [1, -1] {
            let Some(j) = step(n, nr.closed, at, sign, 6) else { continue };
            let d = (xz(nr.road.pts[j]) - c).normalize_or_zero();
            if d == Vec2::ZERO {
                continue;
            }
            raw.push(Raw { road: r, at, sign, ends, d });
        }
    }
    if raw.len() < 2 {
        return None;
    }
    raw.sort_by(|p, q| libm::atan2f(p.d.y, p.d.x).total_cmp(&libm::atan2f(q.d.y, q.d.x)).then((p.road, p.sign).cmp(&(q.road, q.sign))));
    let m = raw.len();
    let half = |r: usize, i: usize| ribbon_half(&net.roads[r].road, i);
    let kind = |r: usize| net.roads[r].kind;
    let r_concave = |a: usize, b: usize| radius(kind(a)).min(radius(kind(b))) - OVERHANG;

    // First pass: straight edges from the centre give each arm's cut-back distance.
    const FAR: f32 = 200.0;
    let mut trim: Vec<f32> = raw.iter().map(|a| raw.iter().filter(|o| o.road != a.road).map(|o| half(o.road, o.at)).fold(0.0, f32::max) + 1.0).collect();
    for k in 0..m {
        let (a, b) = (&raw[k], &raw[(k + 1) % m]);
        let pa = c + perp(a.d) * half(a.road, a.at) + a.d * FAR;
        let pb = c - perp(b.d) * half(b.road, b.at) + b.d * FAR;
        if let Some(f) = fillet(pa, a.d, pb, b.d, r_concave(a.road, b.road)) {
            trim[k] = trim[k].max(FAR - (f.s - f.t));
            trim[(k + 1) % m] = trim[(k + 1) % m].max(FAR - (f.u - f.t));
        }
    }

    // Mouths: the first whole station past the cut-back.
    let mut arms: Vec<Arm> = Vec::with_capacity(m);
    for (k, a) in raw.iter().enumerate() {
        let nr = &net.roads[a.road];
        let steps = (trim[k] / STEP).ceil() as usize;
        let mouth = step(nr.road.len(), nr.closed, a.at, a.sign, steps)?;
        // A short road must keep some ribbon between its two ends' junctions.
        if steps * 2 + 4 > nr.road.len() {
            return None;
        }
        arms.push(Arm { road: a.road, kind: nr.kind, at: a.at, sign: a.sign, mouth, steps, ends: a.ends });
    }

    // Mouth rows (counter-clockwise: from the arm's clockwise side to its counter-clockwise side).
    let rows: Vec<(Vec<Vec2>, Vec2)> = arms
        .iter()
        .map(|a| {
            let road = &net.roads[a.road].road;
            let d = road.dir[a.mouth];
            let left = Vec2::new(d.y, -d.x);
            let out = d * a.sign as f32;
            let p = xz(road.pts[a.mouth]);
            let mut lats = ribbon_columns(half(a.road, a.mouth));
            // Counter-clockwise side of `out` is lateral −sign: start from +sign.
            if a.sign > 0 {
                lats.reverse();
            }
            (lats.into_iter().map(|l| p + left * l).collect(), out)
        })
        .collect();

    let mut outline: Vec<Vec2> = Vec::new();
    let mut mouths = Vec::with_capacity(m);
    let mut corners = Vec::with_capacity(m);
    for k in 0..m {
        let (row, a) = &rows[k];
        let start = outline.len();
        outline.extend_from_slice(row);
        mouths.push(start..outline.len());
        let (next, b) = &rows[(k + 1) % m];
        let (pa, pb) = (*row.last()?, next[0]);
        let mut pts = vec![pa];
        let mut corner_r = 0.0;
        let mut concave = false;
        if let Some(mut f) = fillet(pa, *a, pb, *b, r_concave(arms[k].road, arms[(k + 1) % m].road)) {
            if f.s < -0.05 || f.u < -0.05 {
                return None; // the edges cross outside the mouths
            }
            // Keep the tangent points between the crossing and the mouths.
            let room = f.s.min(f.u).max(0.0);
            if f.t > room {
                f.r *= room / f.t;
                f.t = room;
            }
            concave = f.turn < 0.0;
            corner_r = f.r;
            for q in arc(pa, *a, &f) {
                if pts.last().is_none_or(|l| l.distance(q) > 0.02) {
                    pts.push(q);
                }
            }
        }
        if pts.last().is_some_and(|l| l.distance(pb) > 0.02) {
            pts.push(pb);
        } else if let Some(l) = pts.last_mut() {
            *l = pb;
        }
        if pts.len() < 2 {
            pts.push(pb);
        }
        outline.extend_from_slice(&pts[1..pts.len() - 1]);
        corners.push(Corner { pts, concave, radius: corner_r });
    }
    let g = JunctionGeom { pos: c, arms, corners, outline, mouths };
    (g.area() > 1.0 && is_simple(&g.outline)).then_some(g)
}

/// Per road, per segment (station i to i + 1; the closing one last on a closed road): inside a
/// junction (not drawn as ribbon).
pub fn trimmed(net: &RoadNet, geoms: &[JunctionGeom]) -> Vec<Vec<bool>> {
    let mut out: Vec<Vec<bool>> = net.roads.iter().map(|r| vec![false; if r.closed { r.road.len() } else { r.road.len() - 1 }]).collect();
    for g in geoms {
        for a in &g.arms {
            let nr = &net.roads[a.road];
            let n = nr.road.len();
            for k in 0..a.steps {
                let seg = if a.sign > 0 { step(n, nr.closed, a.at, 1, k) } else { step(n, nr.closed, a.at, -1, k + 1) };
                if let Some(s) = seg
                    && s < out[a.road].len()
                {
                    out[a.road][s] = true;
                }
            }
        }
    }
    out
}

/// The station runs still drawn as ribbon, given each segment's `inside` flag: each run lists
/// its stations in order (a closed road's run may wrap past the last station to 0).
pub fn kept_runs(n: usize, closed: bool, inside: &[bool]) -> Vec<Vec<usize>> {
    let segs = inside.len();
    let mut runs = Vec::new();
    if !inside.iter().any(|&x| x) {
        let mut all: Vec<usize> = (0..n).collect();
        if closed {
            all.push(0);
        }
        return vec![all];
    }
    // Start just after a trimmed segment so a wrapping run comes out whole.
    let first = if closed { (0..segs).find(|&s| inside[s]).map_or(0, |s| s + 1) } else { 0 };
    let mut cur: Vec<usize> = Vec::new();
    for k in 0..segs {
        let s = (first + k) % segs;
        if inside[s] {
            if !cur.is_empty() {
                runs.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if cur.is_empty() {
            cur.push(s);
        }
        cur.push((s + 1) % n);
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    runs
}

// ---------------------------------------------------------------- kerbs and sidewalks

/// Kerb height above the road (m) and the sidewalk's top behind it (a centimetre lower, so the
/// kerb's top never fights the paving).
pub const KERB_TOP: f32 = 0.17;
pub const SIDEWALK_TOP: f32 = 0.16;
/// Arc segments per quarter circle of a kerb corner (the sidewalk corner uses the same).
pub const ARC_STEPS: usize = 16;

/// A point on the kerb fillet circle centred `c`: angle `k` quarter-steps (π / 2 / ARC_STEPS).
fn arc_point(c: Vec2, r: f32, k: i32) -> Vec2 {
    let a = k as f32 * core::f32::consts::FRAC_PI_2 / ARC_STEPS as f32;
    c + Vec2::new(libm::cosf(a), libm::sinf(a)) * r
}

/// The kerb line round a town block: the rectangle `lo..hi` (the street edges) with its corners
/// rounded at [`TOWN_KERB_R`], counter-clockwise, starting in the middle of the first corner's
/// arc (never on a straight, where gaps go).
pub fn kerb_loop(lo: Vec2, hi: Vec2) -> Vec<Vec2> {
    let r = TOWN_KERB_R;
    let q = ARC_STEPS as i32;
    // Corners counter-clockwise from (lo.x, lo.y): centre and starting angle (quarter index).
    let corners = [(Vec2::new(lo.x + r, lo.y + r), 2), (Vec2::new(hi.x - r, lo.y + r), 3), (Vec2::new(hi.x - r, hi.y - r), 0), (Vec2::new(lo.x + r, hi.y - r), 1)];
    let mut pts = Vec::new();
    for (c, quarter) in corners {
        for k in 0..=q {
            pts.push(arc_point(c, r, quarter * q + k));
        }
    }
    // Start mid-arc.
    pts.rotate_left((q / 2) as usize);
    pts
}

/// The sidewalk round a block's outer corner `k` (`into` the block: a pair of ±1), `depth` m
/// deep, as two convex pieces cut by the kerb fillet: along x and along y, each reaching
/// [`TOWN_KERB_R`] from the corner (where the kerb's arc meets its straights).
pub fn sidewalk_corner_pieces(k: Vec2, into: Vec2, depth: f32) -> [Vec<Vec2>; 2] {
    let r = TOWN_KERB_R;
    [sidewalk_corner(k, into, Vec2::ZERO, Vec2::new(r, depth)), sidewalk_corner(k, into, Vec2::new(0.0, depth), Vec2::new(depth, r))]
}

/// The rectangle `a..b` (local: metres from the outer corner `k` along `into`) cut by the
/// kerb fillet: the part inside the fillet circle (convex).
pub fn sidewalk_corner(k: Vec2, into: Vec2, a: Vec2, b: Vec2) -> Vec<Vec2> {
    let r = TOWN_KERB_R;
    let c = k + into * r;
    let at = |x: f32, y: f32| k + into * Vec2::new(x, y);
    let mut poly = vec![at(a.x, a.y), at(b.x, a.y), at(b.x, b.y), at(a.x, b.y)];
    if signed_area(&poly) < 0.0 {
        poly.reverse();
    }
    // Clip by the circle's polygon (same vertices as the kerb arcs), edge by edge.
    let n = 4 * ARC_STEPS as i32;
    for e in 0..n {
        let (a, b) = (arc_point(c, r, e), arc_point(c, r, e + 1));
        let inside = |p: Vec2| (b - a).perp_dot(p - a) >= 0.0;
        let mut out = Vec::new();
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            let (ip, iq) = (inside(p), inside(q));
            if ip {
                out.push(p);
            }
            if ip != iq {
                let (dp, dq) = ((b - a).perp_dot(p - a), (b - a).perp_dot(q - a));
                out.push(p + (q - p) * (dp / (dp - dq)));
            }
        }
        poly = out;
        if poly.len() < 3 {
            return Vec::new();
        }
    }
    poly.dedup_by(|a, b| a.distance_squared(*b) < 1e-8);
    poly
}

// ---------------------------------------------------------------- polygons

/// The signed area (positive counter-clockwise in (x, z)).
pub fn signed_area(p: &[Vec2]) -> f32 {
    let n = p.len();
    (0..n).map(|k| p[k].perp_dot(p[(k + 1) % n])).sum::<f32>() * 0.5
}

fn segments_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let o = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
    let (d1, d2, d3, d4) = (o(a, b, c), o(a, b, d), o(c, d, a), o(c, d, b));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// No two non-neighbouring edges cross, and no two vertices coincide.
pub fn is_simple(p: &[Vec2]) -> bool {
    let n = p.len();
    if n < 3 {
        return false;
    }
    for i in 0..n {
        for j in i + 1..n {
            if p[i].distance_squared(p[j]) < 1e-6 {
                return false;
            }
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if segments_cross(p[i], p[(i + 1) % n], p[j], p[(j + 1) % n]) {
                return false;
            }
        }
    }
    true
}

fn in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let e = -1e-6;
    (b - a).perp_dot(p - a) >= e && (c - b).perp_dot(p - b) >= e && (a - c).perp_dot(p - c) >= e
}

/// Ear clipping of a simple counter-clockwise polygon: triangles (counter-clockwise) of indices
/// into `p`. Collinear runs (a mouth row) are fine: their middle vertices are clipped once a
/// neighbour has been.
pub fn triangulate(p: &[Vec2]) -> Vec<[u32; 3]> {
    let mut idx: Vec<usize> = (0..p.len()).collect();
    let mut out = Vec::with_capacity(p.len().saturating_sub(2));
    let cross = |a: usize, b: usize, c: usize| (p[b] - p[a]).perp_dot(p[c] - p[b]);
    while idx.len() > 3 {
        let n = idx.len();
        let mut best: Option<(usize, f32)> = None;
        for k in 0..n {
            let (a, b, c) = (idx[(k + n - 1) % n], idx[k], idx[(k + 1) % n]);
            let x = cross(a, b, c);
            if x <= 1e-7 {
                continue;
            }
            let blocked = idx.iter().any(|&o| o != a && o != b && o != c && in_triangle(p[o], p[a], p[b], p[c]));
            if blocked {
                continue;
            }
            // Prefer fat ears (fewer slivers): the smallest angle's sine.
            let (ab, bc, ca) = ((p[b] - p[a]).length(), (p[c] - p[b]).length(), (p[a] - p[c]).length());
            let q = x / (ab.max(bc).max(ca).powi(2)).max(1e-9);
            if best.is_none_or(|(_, bq)| q > bq) {
                best = Some((k, q));
            }
        }
        // Numerical trouble: clip the most convex vertex anyway.
        let k = match best {
            Some((k, _)) => k,
            None => (0..n).max_by(|&i, &j| cross(idx[(i + n - 1) % n], idx[i], idx[(i + 1) % n]).total_cmp(&cross(idx[(j + n - 1) % n], idx[j], idx[(j + 1) % n]))).unwrap(),
        };
        let (a, b, c) = (idx[(k + n - 1) % n], idx[k], idx[(k + 1) % n]);
        if cross(a, b, c) > 0.0 {
            out.push([a as u32, b as u32, c as u32]);
        }
        idx.remove(k);
    }
    if idx.len() == 3 && cross(idx[0], idx[1], idx[2]) > 0.0 {
        out.push([idx[0] as u32, idx[1] as u32, idx[2] as u32]);
    }
    out
}

/// Splits triangle edges until the surface follows `height`: an edge is halved when longer
/// than `max_len`, or longer than `min_len` with `height` off the straight line by more than
/// `tol` (m) at its quarter points. `frozen(a, b)` edges are never split (the mouths: the
/// ribbon's row must match vertex for vertex). Edges decide alone, so neighbours agree and
/// no cracks open. Returns the new points (heights alongside) and triangles.
#[allow(clippy::too_many_arguments)]
pub fn refine(
    mut pts: Vec<Vec2>,
    mut ys: Vec<f32>,
    mut tris: Vec<[u32; 3]>,
    height: impl Fn(Vec2) -> f32,
    frozen: impl Fn(u32, u32) -> bool,
    max_len: f32,
    min_len: f32,
    tol: f32,
) -> (Vec<Vec2>, Vec<f32>, Vec<[u32; 3]>) {
    for _ in 0..8 {
        let mut mids: std::collections::BTreeMap<(u32, u32), Option<u32>> = Default::default();
        let mut split = |a: u32, b: u32, pts: &mut Vec<Vec2>, ys: &mut Vec<f32>| -> Option<u32> {
            let key = (a.min(b), a.max(b));
            if let Some(m) = mids.get(&key) {
                return *m;
            }
            let (pa, pb) = (pts[key.0 as usize], pts[key.1 as usize]);
            let (ya, yb) = (ys[key.0 as usize], ys[key.1 as usize]);
            let len = pa.distance(pb);
            let want = !frozen(key.0, key.1)
                && (len > max_len || (len > min_len && [0.25f32, 0.5, 0.75].iter().any(|&t| (height(pa.lerp(pb, t)) - (ya + (yb - ya) * t)).abs() > tol)));
            let m = want.then(|| {
                let q = (pa + pb) * 0.5;
                pts.push(q);
                ys.push(height(q));
                (pts.len() - 1) as u32
            });
            mids.insert(key, m);
            m
        };
        let mut out = Vec::with_capacity(tris.len() * 2);
        let mut changed = false;
        for t in &tris {
            let m = [split(t[0], t[1], &mut pts, &mut ys), split(t[1], t[2], &mut pts, &mut ys), split(t[2], t[0], &mut pts, &mut ys)];
            let count = m.iter().filter(|x| x.is_some()).count();
            if count == 0 {
                out.push(*t);
                continue;
            }
            changed = true;
            // Rotate so the split edges come first (edge k joins v[k] and v[k + 1]).
            let rot = match count {
                1 => m.iter().position(|x| x.is_some()).unwrap(),
                2 => (0..3).find(|&k| m[k].is_some() && m[(k + 1) % 3].is_some()).unwrap(),
                _ => 0,
            };
            let v = [t[rot], t[(rot + 1) % 3], t[(rot + 2) % 3]];
            let mm = [m[rot], m[(rot + 1) % 3], m[(rot + 2) % 3]];
            match count {
                1 => {
                    let m0 = mm[0].unwrap();
                    out.push([v[0], m0, v[2]]);
                    out.push([m0, v[1], v[2]]);
                }
                2 => {
                    let (m0, m1) = (mm[0].unwrap(), mm[1].unwrap());
                    out.push([m0, v[1], m1]);
                    out.push([v[0], m0, m1]);
                    out.push([v[0], m1, v[2]]);
                }
                _ => {
                    let (m0, m1, m2) = (mm[0].unwrap(), mm[1].unwrap(), mm[2].unwrap());
                    out.extend_from_slice(&[[v[0], m0, m2], [m0, v[1], m1], [m2, m1, v[2]], [m0, m1, m2]]);
                }
            }
        }
        tris = out;
        if !changed {
            break;
        }
    }
    (pts, ys, tris)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open::roads;

    fn net() -> RoadNet {
        roads::generate(crate::open::SEED)
    }

    #[test]
    fn every_junction_builds_a_simple_ccw_outline() {
        let n = net();
        let nodes = nodes(&n);
        let geoms = build(&n);
        assert_eq!(geoms.len(), nodes.len(), "every node gets a junction mesh");
        for g in &geoms {
            // Two 3.4 m forest tracks meeting make a small one.
            assert!(g.area() > 25.0, "junction at {} area {}", g.pos, g.area());
            assert!(is_simple(&g.outline), "junction at {} self-intersects", g.pos);
            assert!(g.outline.iter().all(|p| p.is_finite()));
            // Everything within reach of the centre.
            assert!(g.outline.iter().all(|p| p.distance(g.pos) < 90.0), "junction at {} sprawls", g.pos);
        }
        // Fernhollow's streets meet Main Street, the spine and each other.
        let town = geoms.iter().filter(|g| g.town()).count();
        assert!((20..=32).contains(&town), "town junctions {town}");
        assert!(geoms.len() >= 12, "{} junctions", geoms.len());
    }

    /// `cargo test --lib open::junctions::tests::summary -- --nocapture`: where they are.
    #[test]
    fn summary() {
        let n = net();
        for (p, arms) in nodes(&n) {
            println!("node ({:.0}, {:.0}) {:?}", p.x, p.y, arms);
        }
        for g in build(&n) {
            let kinds: Vec<String> = g.arms.iter().map(|a| format!("{:?}{}{}", a.kind, if a.ends { "·end" } else { "" }, a.steps)).collect();
            let r: Vec<String> = g.corners.iter().map(|c| format!("{:.1}{}", c.radius, if c.concave { "" } else { "v" })).collect();
            println!("junction ({:.0}, {:.0}) area {:.0} m² outline {} arms {:?} radii {:?}", g.pos.x, g.pos.y, g.area(), g.outline.len(), kinds, r);
        }
    }

    #[test]
    fn deterministic() {
        let (a, b) = (build(&net()), build(&net()));
        assert_eq!(a, b);
    }

    #[test]
    fn ribbons_are_trimmed_out_of_the_junctions() {
        let n = net();
        let geoms = build(&n);
        let inside = trimmed(&n, &geoms);
        for (r, nr) in n.roads.iter().enumerate() {
            let runs = kept_runs(nr.road.len(), nr.closed, &inside[r]);
            for run in &runs {
                for w in run.windows(2) {
                    // Every kept segment's midpoint is outside every junction outline (the
                    // mouth rows are on the boundary).
                    let m = (xz(nr.road.pts[w[0]]) + xz(nr.road.pts[w[1]])) * 0.5;
                    for g in &geoms {
                        assert!(!point_in(&g.outline, m), "road {r} ({:?}) segment {}..{} inside the junction at {}", nr.kind, w[0], w[1], g.pos);
                    }
                }
            }
            // Each mouth station is the end of a kept run.
            for g in &geoms {
                for a in g.arms.iter().filter(|a| a.road == r) {
                    assert!(runs.iter().any(|run| run[0] == a.mouth || *run.last().unwrap() == a.mouth), "mouth {} of road {r} not a run end", a.mouth);
                }
            }
        }
    }

    #[test]
    fn mouth_rows_match_the_ribbon() {
        let n = net();
        for g in build(&n) {
            for (a, m) in g.arms.iter().zip(&g.mouths) {
                let road = &n.roads[a.road].road;
                assert_eq!(m.len(), ribbon_columns(ribbon_half(road, a.mouth)).len());
                let p = xz(road.pts[a.mouth]);
                for q in &g.outline[m.clone()] {
                    assert!((q.distance(p) - 0.0).abs() <= ribbon_half(road, a.mouth) + 1e-3);
                }
            }
        }
    }

    fn point_in(poly: &[Vec2], p: Vec2) -> bool {
        let mut inside = false;
        let n = poly.len();
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                inside = !inside;
            }
        }
        inside
    }

    #[test]
    fn ear_clipping_covers_the_polygon() {
        // An L with a collinear run on one side.
        let p = [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(6.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 4.0), Vec2::new(4.0, 4.0), Vec2::new(4.0, 10.0), Vec2::new(0.0, 10.0)];
        let t = triangulate(&p);
        assert_eq!(t.len(), p.len() - 2);
        let area: f32 = t.iter().map(|t| signed_area(&[p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]])).sum();
        assert!((area - signed_area(&p)).abs() < 1e-3, "{area}");
        assert!(t.iter().all(|t| signed_area(&[p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]]) > 0.0));
    }

    #[test]
    fn junction_outlines_triangulate_whole() {
        for g in build(&net()) {
            let t = triangulate(&g.outline);
            assert_eq!(t.len(), g.outline.len() - 2, "junction at {}", g.pos);
            let area: f32 = t.iter().map(|t| signed_area(&[g.outline[t[0] as usize], g.outline[t[1] as usize], g.outline[t[2] as usize]])).sum();
            assert!((area - g.area()).abs() < g.area() * 1e-3, "junction at {}: {area} vs {}", g.pos, g.area());
        }
    }

    #[test]
    fn refinement_keeps_area_and_frozen_edges() {
        let p = vec![Vec2::new(0.0, 0.0), Vec2::new(30.0, 0.0), Vec2::new(30.0, 20.0), Vec2::new(0.0, 20.0)];
        let t = triangulate(&p);
        let h = |q: Vec2| libm::sinf(q.x * 0.3);
        let ys = p.iter().map(|q| h(*q)).collect();
        let (pts, ys, tris) = refine(p.clone(), ys, t, h, |a, b| (a, b) == (0, 1), 8.0, 1.0, 0.01);
        let area: f32 = tris.iter().map(|t| signed_area(&[pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]])).sum();
        assert!((area - 600.0).abs() < 1e-2, "{area}");
        assert!(tris.len() > 8);
        assert_eq!(pts.len(), ys.len());
        // Edge 0–1 never split: no vertex strictly between them on y = 0.
        assert!(!pts.iter().any(|q| q.y.abs() < 1e-4 && q.x > 0.1 && q.x < 29.9));
        // No cracks: every interior edge is shared by exactly two triangles.
        let mut count: std::collections::BTreeMap<(u32, u32), i32> = Default::default();
        for t in &tris {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *count.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        let boundary = count.values().filter(|&&c| c == 1).count();
        assert!(count.values().all(|&c| c == 1 || c == 2));
        // The boundary edges add up to the perimeter.
        let per: f32 = count.iter().filter(|(_, c)| **c == 1).map(|((a, b), _)| pts[*a as usize].distance(pts[*b as usize])).sum();
        assert!((per - 100.0).abs() < 1e-2, "{per} over {boundary} edges");
    }

    #[test]
    fn kerb_loop_and_sidewalk_corners_share_the_fillet() {
        let (lo, hi) = (Vec2::new(5.5, 5.5), Vec2::new(104.5, 104.5));
        let l = kerb_loop(lo, hi);
        assert!(signed_area(&l) > 0.0 && is_simple(&l));
        let c = Vec2::splat(5.5 + TOWN_KERB_R);
        let [x, y] = sidewalk_corner_pieces(lo, Vec2::ONE, 3.0);
        let mut area = 0.0;
        for corner in [&x, &y] {
            assert!(corner.len() > 4 && signed_area(corner) > 0.0);
            area += signed_area(corner);
            for p in corner {
                assert!(p.distance(c) <= TOWN_KERB_R + 1e-3, "{p}");
                assert!(p.x >= 5.5 - 1e-4 && p.y >= 5.5 - 1e-4 && p.x <= 12.5 + 1e-4 && p.y <= 12.5 + 1e-4);
                // Within the sidewalk's 3 m.
                assert!(p.x <= 8.5 + 1e-4 || p.y <= 8.5 + 1e-4);
            }
            assert!(!corner.iter().any(|p| p.distance(lo) < 1.0), "the outer corner is cut away");
        }
        // The L of sidewalk (7 × 3 + 3 × 4 m²) less what the fillet takes.
        assert!((area - (33.0 - (49.0 - core::f32::consts::PI * 49.0 / 4.0))).abs() < 0.1, "{area}"); // (the arc is a 64-gon)
        // The pieces reach the arc's ends (on the straights, 7 m from the corner).
        assert!(x.iter().any(|p| p.distance(Vec2::new(12.5, 5.5)) < 1e-3));
        assert!(y.iter().any(|p| p.distance(Vec2::new(5.5, 12.5)) < 1e-3));
        // Every kerb point near that corner lies on the circle the sidewalk was cut with.
        for p in l.iter().filter(|p| p.x < 12.5 && p.y < 12.5) {
            assert!((p.distance(c) - TOWN_KERB_R).abs() < 1e-3);
        }
    }

    #[test]
    fn kept_runs_wrap_on_a_closed_road() {
        let mut inside = vec![false; 10];
        inside[2] = true;
        inside[3] = true;
        let runs = kept_runs(10, true, &inside);
        assert_eq!(runs, vec![vec![4, 5, 6, 7, 8, 9, 0, 1, 2]]);
        let runs = kept_runs(10, false, &inside[..9]);
        assert_eq!(runs, vec![vec![0, 1, 2], vec![4, 5, 6, 7, 8, 9]]);
    }
}
