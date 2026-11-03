//! Traffic: AI cars driving the open world's roads (`docs/OPENWORLD.md`). Ambient life, not
//! racing: they keep to their lane, slow behind each other and the player, give way at
//! junctions and pick a random way on.
//!
//! The pure part ([`LaneGraph`], [`Traffic`]) is deterministic for a seed and the same player
//! inputs, and tested; the ECS part ([`TrafficPlugin`]) steps it at the fixed rate and draws
//! the cars.
//!
//! - **Lanes** ([`LaneGraph`]): every road between two junctions (or a dead end) gives two
//!   lanes, one each way, on the right a quarter of the width from the centreline (banking
//!   applied to the height). Lanes stop short of junctions; inside a junction, *turn* lanes
//!   (cubic curves) join each incoming lane to every outgoing one that isn't a U-turn
//!   (straight on, left, right); a dead end (the forest roads) has a U-turn. Speed limits by
//!   road kind, lower where the lane curves, and an *advisory* speed per point that brakes
//!   comfortably into the next limit.
//! - **Cars** ([`Traffic`]): kinematic, a lane and a distance along it. The intelligent driver
//!   model (IDM) gives the acceleration: toward the limit, behind the car ahead (looking
//!   across lane joins), behind the player when they're on the path ahead.
//! - **Junctions**: a turn lane is used by one car at a time among the turns that cross or
//!   merge (precomputed). The front car of a lane asks for its turn as it approaches;
//!   requests are served first come, first served, and only when the lane it turns into has
//!   room (no blocking the box). A car without its turn stops at the line.
//! - **Spawning**: about [`TrafficConfig::cars`] cars within [`TrafficConfig::radius`] of the
//!   player, spawned on lanes out of sight (past [`TrafficConfig::spawn_min`]), removed past
//!   [`TrafficConfig::despawn`].
//! - **Solid**: [`Traffic::obstacles`] gives three cylinders per car for the car sim; hit hard,
//!   a car stops for a few seconds.

use std::sync::{Arc, RwLock};

use bevy::math::{Vec2, Vec3};
use bevy::prelude::*;

use super::roads::{RoadKind, RoadNet};
use crate::stage::{Rng, STEP};
use crate::vehicle::{Give, Obstacle};

// ---------------------------------------------------------------- numbers

/// Car length and width (m), and the centre-to-centre spacing of its collision cylinders.
pub const CAR_LEN: f32 = 4.4;
pub const CAR_WIDTH: f32 = 1.8;
const HALF_LEN: f32 = CAR_LEN * 0.5;
/// Collision cylinders: radius, offsets along the car, top above the road.
const SOLID_R: f32 = 0.95;
const SOLID_AT: [f32; 3] = [-1.3, 0.0, 1.3];
const SOLID_TOP: f32 = 1.4;

/// IDM: max acceleration, comfortable braking, standstill gap, time headway; emergency brake.
const A_MAX: f32 = 1.4;
const B_COMF: f32 = 2.0;
const S0: f32 = 2.0;
const HEADWAY: f32 = 1.2;
const B_MAX: f32 = 9.0;
/// Comfortable sideways acceleration on lanes and in junctions (sets speeds in curves).
const A_LAT: f32 = 2.0;
const A_LAT_TURN: f32 = 2.5;
/// Slowest a car is asked to drive anywhere (a U-turn).
const V_MIN: f32 = 2.5;
/// Junctions closer than this are one node.
const MERGE: f32 = 10.0;
/// Two turn lanes conflict when they pass closer than this.
const CONFLICT: f32 = 2.5;
/// Lanes end this far before a dead end (the U-turn fits in it).
const DEAD_END_TRIM: f32 = 6.0;
/// How far ahead a car looks for a leader (m).
const LOOK: f32 = 150.0;
/// Spawn spots every this many metres along road lanes.
const SPOT_STEP: f32 = 15.0;
/// Spawn-spot grid cell (m).
const CELL: f32 = 250.0;
/// Seconds a car stands after a hard hit.
const HAZARD_S: f32 = 4.0;

/// Speed limit by road kind (m/s): 40, 100, 70, 50 km/h.
pub fn limit(kind: RoadKind) -> f32 {
    match kind {
        RoadKind::Street => 40.0 / 3.6,
        RoadKind::Highway => 100.0 / 3.6,
        RoadKind::Country => 70.0 / 3.6,
        RoadKind::Forest => 50.0 / 3.6,
    }
}

/// Lane centre's distance from the road's centreline (m).
pub fn lane_offset(kind: RoadKind) -> f32 {
    match kind {
        RoadKind::Forest => 1.6,
        k => k.width() * 0.25,
    }
}

// ---------------------------------------------------------------- lane graph

/// Which way a turn lane goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    Straight,
    Left,
    Right,
    UTurn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneKind {
    /// Along a road of the network (index into `RoadNet::roads`), `forward` = with the
    /// road's station order.
    Road { road: usize, forward: bool },
    /// Inside junction node `node` (index into [`LaneGraph::nodes`]); `idx` is its index in
    /// that node's [`Node::turns`].
    Turn { node: u32, idx: u32, turn: Turn },
}

/// A lane: a polyline on the road surface, driven from the first point to the last.
#[derive(Clone, Debug)]
pub struct Lane {
    pub kind: LaneKind,
    pub pts: Vec<Vec3>,
    /// Distance along the lane at each point (m); the last is the lane's length.
    pub cum: Vec<f32>,
    /// Speed limit at each point (m/s): the road's, lower in curves.
    pub limit: Vec<f32>,
    /// Speed to be at each point to brake comfortably into every limit further on (m/s).
    pub advisory: Vec<f32>,
    /// Lanes a car can go on to from the end of this one.
    pub next: Vec<u32>,
    /// The junction node at the end (road lanes): cars ask it for their turn.
    pub end_node: Option<u32>,
}

impl Lane {
    pub fn length(&self) -> f32 {
        *self.cum.last().unwrap()
    }

    /// Segment index and fraction at distance `s` (clamped to the lane).
    fn seg(&self, s: f32) -> (usize, f32) {
        let n = self.pts.len();
        let s = s.clamp(0.0, self.length());
        let i = self.cum.partition_point(|c| *c <= s).saturating_sub(1).min(n - 2);
        let d = self.cum[i + 1] - self.cum[i];
        (i, if d > 1e-6 { (s - self.cum[i]) / d } else { 0.0 })
    }

    /// Point at distance `s` (clamped to the lane).
    pub fn point(&self, s: f32) -> Vec3 {
        let (i, t) = self.seg(s);
        self.pts[i].lerp(self.pts[i + 1], t)
    }

    /// The limit at `s` (the lower of the two points around it).
    pub fn limit_at(&self, s: f32) -> f32 {
        let (i, _) = self.seg(s);
        self.limit[i].min(self.limit[i + 1])
    }

    pub fn advisory_at(&self, s: f32) -> f32 {
        let (i, t) = self.seg(s);
        self.advisory[i] + (self.advisory[i + 1] - self.advisory[i]) * t
    }

    pub fn is_turn(&self) -> bool {
        matches!(self.kind, LaneKind::Turn { .. })
    }
}

/// A junction (or a dead end): where lanes end and turn lanes join them.
#[derive(Clone, Debug)]
pub struct Node {
    pub pos: Vec2,
    pub dead_end: bool,
    /// Its turn lanes (indices into [`LaneGraph::lanes`]).
    pub turns: Vec<u32>,
    /// `conflicts[i]` bit `j`: turns `i` and `j` cross or merge (a turn never conflicts with
    /// itself: cars on one turn follow each other).
    pub conflicts: Vec<u64>,
}

/// Every lane of the network and how they join.
#[derive(Clone, Debug)]
pub struct LaneGraph {
    pub lanes: Vec<Lane>,
    pub nodes: Vec<Node>,
}

/// One end of a road segment at a node: the lane leaving the node and the one arriving.
#[derive(Clone, Copy)]
struct Arm {
    out_lane: u32,
    in_lane: u32,
}

fn xz(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

/// Cumulative XZ distance along a polyline.
fn cumulative(pts: &[Vec3]) -> Vec<f32> {
    let mut cum = Vec::with_capacity(pts.len());
    let mut d = 0.0;
    for (i, p) in pts.iter().enumerate() {
        if i > 0 {
            d += xz(*p).distance(xz(pts[i - 1]));
        }
        cum.push(d);
    }
    cum
}

/// Curvature (1/m, unsigned) at each point of a polyline.
fn curvature(pts: &[Vec3]) -> Vec<f32> {
    let n = pts.len();
    (0..n)
        .map(|i| {
            if i == 0 || i == n - 1 {
                return 0.0;
            }
            let (a, b) = (xz(pts[i] - pts[i - 1]), xz(pts[i + 1] - pts[i]));
            let len = 0.5 * (a.length() + b.length());
            if len < 1e-4 { 0.0 } else { libm::atan2f(a.perp_dot(b), a.dot(b)).abs() / len }
        })
        .collect()
}

/// Backward pass: the speed at each point from which a car brakes at `B_COMF` into every
/// later limit, ending at `end`.
fn advisory(limit: &[f32], cum: &[f32], end: f32) -> Vec<f32> {
    let mut v = limit.to_vec();
    let n = v.len();
    v[n - 1] = v[n - 1].min(end);
    for i in (0..n - 1).rev() {
        let ds = cum[i + 1] - cum[i];
        v[i] = v[i].min((v[i + 1] * v[i + 1] + 2.0 * B_COMF * ds).sqrt());
    }
    v
}

impl LaneGraph {
    /// Lanes for every road of `net`, joined at its junctions.
    pub fn build(net: &RoadNet) -> LaneGraph {
        // Nodes: junctions closer than MERGE are one (a town street's end and the country
        // road leaving it). Each lists the (road, station) pairs that meet there.
        // (position, the (road, station) pairs meeting there, dead end)
        type Raw = (Vec2, Vec<(usize, usize)>, bool);
        let mut nodes: Vec<Raw> = Vec::new();
        for j in &net.junctions {
            let k = match nodes.iter().position(|(p, _, _)| p.distance(j.pos) < MERGE) {
                Some(k) => k,
                None => {
                    nodes.push((j.pos, Vec::new(), false));
                    nodes.len() - 1
                }
            };
            for rs in &j.roads {
                if !nodes[k].1.contains(rs) {
                    nodes[k].1.push(*rs);
                }
            }
        }
        // Stops along each road (station, node), and dead ends at open ends no junction has.
        let mut stops: Vec<Vec<(usize, usize)>> = vec![Vec::new(); net.roads.len()];
        for (k, (_, arms, _)) in nodes.iter().enumerate() {
            for &(r, st) in arms {
                stops[r].push((st, k));
            }
        }
        for (r, nr) in net.roads.iter().enumerate() {
            if nr.closed {
                continue;
            }
            let last = nr.road.len() - 1;
            for end in [0, last] {
                if !stops[r].iter().any(|(st, _)| *st == end) {
                    let p = nr.road.pts[end];
                    nodes.push((xz(p), vec![(r, end)], true));
                    stops[r].push((end, nodes.len() - 1));
                }
            }
        }
        for s in &mut stops {
            s.sort();
            s.dedup();
        }
        // How far lanes stop short of each node: past the widest road meeting there.
        let trim: Vec<f32> = nodes
            .iter()
            .map(|(_, arms, dead)| {
                if *dead {
                    DEAD_END_TRIM
                } else {
                    arms.iter().map(|&(r, st)| net.roads[r].road.width[st] * 0.5).fold(0.0f32, f32::max) + 2.0
                }
            })
            .collect();

        let mut lanes: Vec<Lane> = Vec::new();
        let mut arms: Vec<Vec<Arm>> = vec![Vec::new(); nodes.len()];
        for (r, nr) in net.roads.iter().enumerate() {
            let road = &nr.road;
            let n = road.len();
            let st = &stops[r];
            let mut segs: Vec<(usize, usize, usize, usize)> = st.windows(2).map(|w| (w[0].0, w[1].0, w[0].1, w[1].1)).collect();
            if nr.closed && !st.is_empty() {
                let (a, b) = (st[st.len() - 1], st[0]);
                segs.push((a.0, b.0 + n, a.1, b.1));
            }
            let off = lane_offset(nr.kind);
            let lim = nr.limit.unwrap_or_else(|| limit(nr.kind));
            for (a, b, na, nb) in segs {
                let ta = (trim[na] / STEP).ceil() as usize;
                let tb = (trim[nb] / STEP).ceil() as usize;
                let (i0, i1) = (a.saturating_add(ta), b.saturating_sub(tb));
                // (Edited roads can put junctions too close together for a lane between them.)
                if i1 <= i0 {
                    warn!("traffic: road {r} segment {a}..{b} too short for its junctions; no lanes on it");
                    continue;
                }
                for forward in [true, false] {
                    let lat = if forward { -off } else { off };
                    let mut pts = Vec::with_capacity(i1 - i0 + 1);
                    let mut limits = Vec::with_capacity(i1 - i0 + 1);
                    for i in i0..=i1 {
                        let i = i % n;
                        let d = road.dir[i];
                        let left = Vec2::new(d.y, -d.x);
                        let p = xz(road.pts[i]) + left * lat;
                        let y = road.pts[i].y + road.bank[i] * lat;
                        pts.push(Vec3::new(p.x, y, p.y));
                        let k = road.curv[i].abs().max(1e-5);
                        limits.push(lim.min((A_LAT / k).sqrt()).max(V_MIN));
                    }
                    if !forward {
                        pts.reverse();
                        limits.reverse();
                    }
                    let cum = cumulative(&pts);
                    let end_node = if forward { nb } else { na } as u32;
                    lanes.push(Lane { kind: LaneKind::Road { road: r, forward }, pts, cum, advisory: Vec::new(), limit: limits, next: Vec::new(), end_node: Some(end_node) });
                }
                let (f, b_) = ((lanes.len() - 2) as u32, (lanes.len() - 1) as u32);
                arms[na].push(Arm { out_lane: f, in_lane: b_ });
                arms[nb].push(Arm { out_lane: b_, in_lane: f });
            }
        }

        // Turn lanes inside each node.
        let mut out_nodes: Vec<Node> = Vec::with_capacity(nodes.len());
        for (k, (pos, _, dead)) in nodes.iter().enumerate() {
            let mut turns: Vec<u32> = Vec::new();
            for ai in &arms[k] {
                let inl = &lanes[ai.in_lane as usize];
                let p0 = *inl.pts.last().unwrap();
                let d_in = xz(p0 - inl.pts[inl.pts.len() - 2]).normalize_or_zero();
                let in_limit = *inl.limit.last().unwrap();
                let mut made = Vec::new();
                let mut fallback: Option<(f32, usize)> = None;
                for (bi, bo) in arms[k].iter().enumerate() {
                    let same = bo.in_lane == ai.in_lane;
                    if same != *dead {
                        continue;
                    }
                    let outl = &lanes[bo.out_lane as usize];
                    let p3 = outl.pts[0];
                    let d_out = xz(outl.pts[1] - p3).normalize_or_zero();
                    // + = left (left = up × direction: (dz, -dx)).
                    let angle = libm::atan2f(d_out.x * d_in.y - d_out.y * d_in.x, d_in.dot(d_out));
                    let turn = if *dead {
                        Turn::UTurn
                    } else if angle.abs() < 0.5 {
                        Turn::Straight
                    } else if angle.abs() > 2.6 {
                        if fallback.is_none_or(|(a, _)| angle.abs() < a) {
                            fallback = Some((angle.abs(), bi));
                        }
                        continue;
                    } else if angle > 0.0 {
                        Turn::Left
                    } else {
                        Turn::Right
                    };
                    made.push((bi, turn));
                }
                if made.is_empty()
                    && let Some((_, bi)) = fallback
                {
                    made.push((bi, Turn::UTurn));
                }
                for (bi, turn) in made {
                    let bo = arms[k][bi];
                    let outl = &lanes[bo.out_lane as usize];
                    let p3 = outl.pts[0];
                    let d_out = xz(outl.pts[1] - p3).normalize_or_zero();
                    let chord = xz(p3 - p0).length();
                    let (c1, c2) = if turn == Turn::UTurn {
                        let k = chord + 2.0;
                        (xz(p0) + d_in * k, xz(p3) - d_out * k)
                    } else {
                        let k = chord * 0.45;
                        (xz(p0) + d_in * k, xz(p3) - d_out * k)
                    };
                    let m = ((chord * 1.6).ceil() as usize).max(6);
                    let pts: Vec<Vec3> = (0..=m)
                        .map(|i| {
                            let t = i as f32 / m as f32;
                            let u = 1.0 - t;
                            let q = xz(p0) * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + xz(p3) * (t * t * t);
                            Vec3::new(q.x, p0.y + (p3.y - p0.y) * t, q.y)
                        })
                        .collect();
                    let base = in_limit.min(outl.limit[0]);
                    let limits: Vec<f32> = curvature(&pts).iter().map(|c| base.min((A_LAT_TURN / c.max(1e-5)).sqrt()).max(V_MIN)).collect();
                    // Smooth the curvature limits a little (sampled curvature is noisy).
                    let limits: Vec<f32> = (0..limits.len()).map(|i| limits[i.saturating_sub(1)..(i + 2).min(limits.len())].iter().copied().fold(f32::MAX, f32::min)).collect();
                    let cum = cumulative(&pts);
                    let idx = turns.len() as u32;
                    let id = lanes.len() as u32;
                    lanes.push(Lane { kind: LaneKind::Turn { node: k as u32, idx, turn }, pts, cum, limit: limits, advisory: Vec::new(), next: vec![bo.out_lane], end_node: None });
                    lanes[ai.in_lane as usize].next.push(id);
                    turns.push(id);
                }
            }
            assert!(turns.len() <= 64, "node {k} has {} turns", turns.len());
            let conflicts = (0..turns.len())
                .map(|i| {
                    let mut bits = 0u64;
                    for j in 0..turns.len() {
                        if i != j && min_distance(&lanes[turns[i] as usize].pts, &lanes[turns[j] as usize].pts) < CONFLICT {
                            bits |= 1 << j;
                        }
                    }
                    bits
                })
                .collect();
            out_nodes.push(Node { pos: *pos, dead_end: *dead, turns, conflicts });
        }

        // Advisory speeds: each lane brakes into the slowest start among the lanes after it.
        for i in 0..lanes.len() {
            let end = lanes[i].next.iter().map(|&n| lanes[n as usize].limit[0]).fold(f32::MAX, f32::min);
            let (lim, cum) = (&lanes[i].limit, &lanes[i].cum);
            lanes[i].advisory = advisory(lim, cum, end.min(*lim.last().unwrap()));
        }
        LaneGraph { lanes, nodes: out_nodes }
    }

    /// Point and unit direction (3D, along the lane) at `s` on `lane`, reading on into `next`
    /// past its end: the line from the rear axle to the front one, so the body yaws through
    /// curves like a car.
    pub fn pose(&self, lane: u32, s: f32, next: u32) -> (Vec3, Vec3) {
        let ahead = |s: f32| {
            let l = &self.lanes[lane as usize];
            if s > l.length() { self.lanes[next as usize].point(s - l.length()) } else { l.point(s) }
        };
        let (front, rear) = (ahead(s + 1.3), ahead((s - 1.3).max(0.0)));
        let mid = ahead(s);
        let d = front - rear;
        let fwd = if d.length_squared() > 1e-6 { d / d.length() } else { Vec3::Z };
        (mid, fwd)
    }
}

/// The closest two polylines come (sampled at their points and a midpoint per segment).
fn min_distance(a: &[Vec3], b: &[Vec3]) -> f32 {
    let dense = |p: &[Vec3]| -> Vec<Vec2> {
        let mut v = Vec::with_capacity(p.len() * 2);
        for w in p.windows(2) {
            v.push(xz(w[0]));
            v.push(xz(w[0].lerp(w[1], 0.5)));
        }
        v.push(xz(*p.last().unwrap()));
        v
    };
    let (a, b) = (dense(a), dense(b));
    let mut best = f32::MAX;
    for p in &a {
        for q in &b {
            best = best.min(p.distance_squared(*q));
        }
    }
    best.sqrt()
}

// ---------------------------------------------------------------- cars

/// How many cars, where.
#[derive(Clone, Copy, Debug)]
pub struct TrafficConfig {
    pub cars: usize,
    /// Cars are kept within this distance of the player (m)...
    pub radius: f32,
    /// ...spawned no closer than this once the player is driving (m)...
    pub spawn_min: f32,
    /// ...and removed past this (m).
    pub despawn: f32,
    pub seed: u64,
}

impl Default for TrafficConfig {
    fn default() -> Self {
        TrafficConfig { cars: 40, radius: 600.0, spawn_min: 220.0, despawn: 800.0, seed: 0x007A_FF1C }
    }
}

/// The player's car as traffic sees it.
#[derive(Clone, Copy, Debug)]
pub struct Player {
    pub pos: Vec3,
    pub vel: Vec3,
}

/// One AI car.
#[derive(Clone, Debug)]
struct Agent {
    id: u32,
    lane: u32,
    /// Centre's distance along the lane (m), speed (m/s).
    s: f32,
    v: f32,
    /// The lane after this one (chosen on entering it).
    next: u32,
    /// Drives at this fraction of the limit.
    temper: f32,
    color: u8,
    /// Seconds left standing after a hit.
    hazard: f32,
    /// Seconds stood still (not counting hits or the player in the way).
    waited: f32,
    /// Tick it first asked for its turn at the junction ahead.
    asked: Option<u64>,
    /// The turn lane it may drive (granted), at that node.
    hold: Option<(u32, u32)>,
    pose: (Vec3, Vec3),
    prev: (Vec3, Vec3),
    /// The player was the car's leader on the last step.
    behind_player: bool,
}

/// What's drawn of a car: its pose now and before the last step (for interpolation).
#[derive(Clone, Copy, Debug)]
pub struct CarView {
    /// Stable for the car's life.
    pub id: u32,
    /// Centre of the car on the road surface, and unit forward (3D, along the lane).
    pub pos: Vec3,
    pub fwd: Vec3,
    pub prev_pos: Vec3,
    pub prev_fwd: Vec3,
    pub speed: f32,
    /// Paint (0..[`COLORS`]).
    pub color: u8,
    /// Stopped after a hit.
    pub hazard: bool,
}

/// Paint variants.
pub const COLORS: u8 = 7;

/// A spawn spot: a road lane and a distance along it.
#[derive(Clone, Copy)]
struct Spot {
    lane: u32,
    s: f32,
    pos: Vec2,
}

/// The traffic around the player.
#[derive(Resource, Clone)]
pub struct Traffic {
    pub graph: Arc<LaneGraph>,
    pub config: TrafficConfig,
    agents: Vec<Agent>,
    rng: Rng,
    tick: u64,
    next_id: u32,
    /// Cars on each lane, sorted by `s` (indices into `agents`), rebuilt each step.
    on_lane: Vec<Vec<u32>>,
    dirty: Vec<u32>,
    /// Granted turns per node: (car id, turn index in the node).
    holds: Vec<Vec<(u32, u32)>>,
    spots: Vec<Spot>,
    cells: Vec<Vec<u32>>,
    cells_n: i32,
    last_player: Option<Vec3>,
}

impl Traffic {
    pub fn new(net: &RoadNet, config: TrafficConfig) -> Traffic {
        Traffic::with_graph(Arc::new(LaneGraph::build(net)), config)
    }

    pub fn with_graph(graph: Arc<LaneGraph>, config: TrafficConfig) -> Traffic {
        let mut spots = Vec::new();
        for (i, l) in graph.lanes.iter().enumerate() {
            if l.is_turn() {
                continue;
            }
            let len = l.length();
            // Not near the ends: room to brake for the stop line, and for a car turning in.
            let mut s = 25.0;
            while s < len - 40.0 {
                spots.push(Spot { lane: i as u32, s, pos: xz(l.point(s)) });
                s += SPOT_STEP;
            }
        }
        let cells_n = (2.0 * super::roads::HALF / CELL).ceil() as i32 + 1;
        let mut cells = vec![Vec::new(); (cells_n * cells_n) as usize];
        for (k, sp) in spots.iter().enumerate() {
            cells[Self::cell_index(cells_n, sp.pos)].push(k as u32);
        }
        let nodes = graph.nodes.len();
        let lanes = graph.lanes.len();
        Traffic {
            graph,
            config,
            agents: Vec::new(),
            rng: Rng::new(config.seed),
            tick: 0,
            next_id: 1,
            on_lane: vec![Vec::new(); lanes],
            dirty: Vec::new(),
            holds: vec![Vec::new(); nodes],
            spots,
            cells,
            cells_n,
            last_player: None,
        }
    }

    fn cell_xy(n: i32, p: Vec2) -> (i32, i32) {
        let h = super::roads::HALF;
        (((p.x + h) / CELL).floor().clamp(0.0, (n - 1) as f32) as i32, ((p.y + h) / CELL).floor().clamp(0.0, (n - 1) as f32) as i32)
    }

    fn cell_index(n: i32, p: Vec2) -> usize {
        let (x, z) = Self::cell_xy(n, p);
        (z * n + x) as usize
    }

    /// How many cars there are.
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Every car, for drawing.
    pub fn cars(&self) -> impl Iterator<Item = CarView> + '_ {
        self.agents.iter().map(|a| CarView { id: a.id, pos: a.pose.0, fwd: a.pose.1, prev_pos: a.prev.0, prev_fwd: a.prev.1, speed: a.v, color: a.color, hazard: a.hazard > 0.0 })
    }

    /// Solid cylinders for cars within `r` of `p`, appended to `out` (not cleared: merge
    /// them with the world's).
    pub fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
        for a in &self.agents {
            push_solids(a.pose, p, r, out);
        }
    }

    /// Advances every car by `dt` (the fixed step), around the player (`None`: nobody to
    /// keep cars near, nothing spawns).
    pub fn step(&mut self, dt: f32, player: Option<Player>) {
        self.tick += 1;
        if let Some(p) = player {
            self.despawn_far(p.pos);
            self.rebuild();
            self.spawn(p.pos);
            self.last_player = Some(p.pos);
        }
        self.rebuild();
        self.grant();
        let acc: Vec<f32> = (0..self.agents.len()).map(|i| self.accel(i, player)).collect();
        self.integrate(&acc, dt);
        if let Some(p) = player {
            self.hits(p);
        }
    }

    // ------------------------------------------------------------ spawning

    fn despawn_far(&mut self, at: Vec3) {
        let far = self.config.despawn;
        let mut i = 0;
        while i < self.agents.len() {
            if xz(self.agents[i].pose.0).distance(xz(at)) > far {
                let a = self.agents.swap_remove(i);
                self.release(a.id);
            } else {
                i += 1;
            }
        }
    }

    fn release(&mut self, id: u32) {
        for h in &mut self.holds {
            h.retain(|(c, _)| *c != id);
        }
    }

    fn spawn(&mut self, at: Vec3) {
        let want = self.config.cars;
        if self.agents.len() >= want {
            return;
        }
        let p = xz(at);
        // Fresh (first step or the player jumped): fill the whole disc, else only out of sight.
        let fresh = self.last_player.is_none_or(|l| xz(l).distance(p) > 50.0);
        let (lo, hi) = (if fresh { 40.0 } else { self.config.spawn_min }, self.config.radius);
        let n = self.cells_n;
        let (c0, c1) = (Self::cell_xy(n, p - Vec2::splat(hi)), Self::cell_xy(n, p + Vec2::splat(hi)));
        let mut cands: Vec<u32> = Vec::new();
        for z in c0.1..=c1.1 {
            for x in c0.0..=c1.0 {
                for &k in &self.cells[(z * n + x) as usize] {
                    let d = self.spots[k as usize].pos.distance(p);
                    if d >= lo && d <= hi {
                        cands.push(k);
                    }
                }
            }
        }
        if cands.is_empty() {
            return;
        }
        let budget = if fresh { want } else { 2 };
        let mut made = 0;
        for _ in 0..budget * 6 {
            if made >= budget || self.agents.len() >= want {
                break;
            }
            let k = cands[((self.rng.f() * cands.len() as f32) as usize).min(cands.len() - 1)];
            let sp = self.spots[k as usize];
            // Clear of other cars on the lane: 40 m ahead, 30 m behind.
            if self.on_lane[sp.lane as usize].iter().any(|&j| {
                let d = self.agents[j as usize].s - sp.s;
                d > -30.0 && d < 40.0
            }) {
                continue;
            }
            let temper = self.rng.range(0.85, 1.0);
            let color = ((self.rng.f() * COLORS as f32) as u8).min(COLORS - 1);
            let next = self.choose_next(sp.lane);
            let lane = &self.graph.lanes[sp.lane as usize];
            let v = lane.advisory_at(sp.s) * temper * 0.9;
            let pose = self.graph.pose(sp.lane, sp.s, next);
            let id = self.next_id;
            self.next_id += 1;
            self.agents.push(Agent { id, lane: sp.lane, s: sp.s, v, next, temper, color, hazard: 0.0, waited: 0.0, asked: None, hold: None, pose, prev: pose, behind_player: false });
            let idx = (self.agents.len() - 1) as u32;
            let list = &mut self.on_lane[sp.lane as usize];
            list.push(idx);
            if !self.dirty.contains(&sp.lane) {
                self.dirty.push(sp.lane);
            }
            made += 1;
        }
    }

    /// A random way on from the end of `lane`: straight on twice as likely as a turn.
    fn choose_next(&mut self, lane: u32) -> u32 {
        let next = &self.graph.lanes[lane as usize].next;
        if next.len() == 1 {
            return next[0];
        }
        let weight = |l: u32| match self.graph.lanes[l as usize].kind {
            LaneKind::Turn { turn: Turn::Straight, .. } => 2.0,
            _ => 1.0,
        };
        let total: f32 = next.iter().map(|&l| weight(l)).sum();
        let mut x = self.rng.f() * total;
        for &l in next {
            x -= weight(l);
            if x < 0.0 {
                return l;
            }
        }
        *next.last().unwrap()
    }

    // ------------------------------------------------------------ the step

    fn rebuild(&mut self) {
        for &l in &self.dirty {
            self.on_lane[l as usize].clear();
        }
        self.dirty.clear();
        for (i, a) in self.agents.iter().enumerate() {
            let l = a.lane as usize;
            if self.on_lane[l].is_empty() {
                self.dirty.push(a.lane);
            }
            self.on_lane[l].push(i as u32);
        }
        let agents = &self.agents;
        for &l in &self.dirty {
            self.on_lane[l as usize].sort_by(|&x, &y| {
                let (a, b) = (&agents[x as usize], &agents[y as usize]);
                a.s.total_cmp(&b.s).then(a.id.cmp(&b.id))
            });
        }
    }

    /// Room at the start of `lane` for `k` more cars to come in.
    fn room(&self, lane: u32, k: usize) -> bool {
        let rear = self.on_lane[lane as usize].first().map_or(f32::MAX, |&j| self.agents[j as usize].s - HALF_LEN);
        rear > (k + 1) as f32 * (CAR_LEN + S0 + 1.0)
    }

    /// Junctions: the front car of each lane asks for its turn as it nears the end; first come,
    /// first served among turns that conflict, and only with room after the turn.
    fn grant(&mut self) {
        let g = self.graph.clone();
        let mut asks: Vec<(u64, u32, usize)> = Vec::new(); // (tick asked, car id, agent index)
        for &l in &self.dirty {
            let list = &self.on_lane[l as usize];
            let Some(&front) = list.last() else { continue };
            let lane = &g.lanes[l as usize];
            if lane.end_node.is_none() {
                continue;
            }
            let a = &self.agents[front as usize];
            if a.hold.is_some() {
                continue;
            }
            let to_end = lane.length() - a.s - HALF_LEN;
            let reach = (a.v * a.v / (2.0 * B_COMF) + 2.0 * S0 + 10.0).min(120.0);
            if to_end < reach {
                asks.push((0, a.id, front as usize));
            }
        }
        for ask in &mut asks {
            let a = &mut self.agents[ask.2];
            ask.0 = *a.asked.get_or_insert(self.tick);
        }
        asks.sort();
        // Turns an older request is waiting for, per node: younger conflicting ones queue.
        let mut waiting: Vec<(u32, u32)> = Vec::new();
        for (_, id, i) in asks {
            let a = &self.agents[i];
            let LaneKind::Turn { node, idx, .. } = g.lanes[a.next as usize].kind else { continue };
            let nd = &g.nodes[node as usize];
            let bits = nd.conflicts[idx as usize];
            let clash = |t: u32| t == idx || bits & (1 << t) != 0;
            let held = self.holds[node as usize].iter().any(|&(_, t)| t != idx && clash(t));
            let queued = waiting.iter().any(|&(n, t)| n == node && clash(t));
            let out = g.lanes[a.next as usize].next[0];
            let into = self.holds[node as usize].iter().filter(|&&(c, t)| g.lanes[nd.turns[t as usize] as usize].next[0] == out && self.agents.iter().any(|b| b.id == c && b.lane != out)).count();
            if !held && !queued && self.room(out, into) {
                self.holds[node as usize].push((id, idx));
                let a = &mut self.agents[i];
                a.hold = Some((node, idx));
                a.asked = None;
            } else {
                waiting.push((node, idx));
            }
        }
    }

    /// The car's leader: (gap bumper to bumper, its speed), the nearest of the car ahead (on
    /// this lane or the next two), the stop line without a turn granted, and the player.
    fn accel(&self, i: usize, player: Option<Player>) -> f32 {
        let a = &self.agents[i];
        if a.hazard > 0.0 {
            return -B_MAX;
        }
        let g = &self.graph;
        let lane = &g.lanes[a.lane as usize];
        let len = lane.length();
        let mut lead: Option<(f32, f32)> = None;
        let mut take = |gap: f32, v: f32| {
            if lead.is_none_or(|(g, _)| gap < g) {
                lead = Some((gap, v));
            }
        };
        // Same lane.
        let list = &self.on_lane[a.lane as usize];
        let pos = list.iter().position(|&j| j as usize == i).unwrap();
        if let Some(&j) = list.get(pos + 1) {
            let b = &self.agents[j as usize];
            take(b.s - a.s - CAR_LEN, b.v);
        } else {
            // On along the next lanes.
            let mut base = len - a.s;
            let mut l = a.next;
            for _ in 0..2 {
                if base > LOOK {
                    break;
                }
                if let Some(&j) = self.on_lane[l as usize].first() {
                    let b = &self.agents[j as usize];
                    take(base + b.s - CAR_LEN, b.v);
                    break;
                }
                let nl = &g.lanes[l as usize];
                if !nl.is_turn() {
                    break;
                }
                base += nl.length();
                l = nl.next[0];
            }
        }
        // The stop line.
        if lane.end_node.is_some() && a.hold.is_none() {
            take(len - a.s - HALF_LEN - 1.0, 0.0);
        }
        // The player, on the path ahead.
        if let Some(p) = player
            && let Some((gap, v)) = self.player_ahead(a, p)
        {
            take(gap, v);
        }
        let v0 = self.desired(a);
        idm(a.v, v0, lead)
    }

    /// The speed the car wants here: the advisory, braking into the next lane's.
    fn desired(&self, a: &Agent) -> f32 {
        let g = &self.graph;
        let lane = &g.lanes[a.lane as usize];
        let to_end = (lane.length() - a.s).max(0.0);
        let next0 = g.lanes[a.next as usize].advisory[0];
        let v = lane.advisory_at(a.s).min((next0 * next0 + 2.0 * B_COMF * to_end).sqrt());
        v * a.temper
    }

    /// The player as a leader: (gap, speed along the path) when they're within reach of the
    /// path ahead.
    fn player_ahead(&self, a: &Agent, p: Player) -> Option<(f32, f32)> {
        let d = xz(p.pos) - xz(a.pose.0);
        let fwd = xz(a.pose.1).normalize_or_zero();
        let along = d.dot(fwd);
        if !(-1.0..=100.0).contains(&along) || d.length_squared() > 110.0 * 110.0 {
            return None;
        }
        // The path point that far along (following the chosen lanes).
        let g = &self.graph;
        let (mut l, mut s, mut next) = (a.lane, a.s + along.max(0.0), a.next);
        for _ in 0..3 {
            let len = g.lanes[l as usize].length();
            if s <= len {
                break;
            }
            s -= len;
            l = next;
            let nl = &g.lanes[l as usize];
            next = if nl.is_turn() { nl.next[0] } else { nl.next.first().copied().unwrap_or(l) };
        }
        let lane = &g.lanes[l as usize];
        let q = xz(lane.point(s));
        let side = (xz(p.pos) - q).length();
        if side > CAR_WIDTH * 0.5 + 1.4 {
            return None;
        }
        let dir = xz(lane.point(s + 1.0) - lane.point(s - 1.0)).normalize_or_zero();
        let v = Vec2::new(p.vel.x, p.vel.z).dot(dir).max(0.0);
        Some((along - HALF_LEN - 2.3, v))
    }

    fn integrate(&mut self, acc: &[f32], dt: f32) {
        let g = self.graph.clone();
        // Hard limits from the start-of-step positions: never closer than 0.3 m to the car
        // ahead in the lane, never past an ungranted stop line.
        let mut room: Vec<f32> = vec![f32::MAX; self.agents.len()];
        for &l in &self.dirty {
            let list = &self.on_lane[l as usize];
            for w in list.windows(2) {
                let (a, b) = (&self.agents[w[0] as usize], &self.agents[w[1] as usize]);
                room[w[0] as usize] = (b.s - a.s - CAR_LEN - 0.3).max(0.0);
            }
            if let Some(&front) = list.last() {
                let a = &self.agents[front as usize];
                let lane = &g.lanes[l as usize];
                // Into the next lane: up to the rear of the first car there.
                let mut r = f32::MAX;
                if let Some(&j) = self.on_lane[a.next as usize].first() {
                    r = lane.length() - a.s + self.agents[j as usize].s - CAR_LEN - 0.3;
                }
                if lane.end_node.is_some() && a.hold.is_none() {
                    r = r.min(lane.length() - a.s - HALF_LEN - 0.5);
                }
                room[front as usize] = r.max(0.0);
            }
        }
        for i in 0..self.agents.len() {
            let mut a = self.agents[i].clone();
            a.prev = a.pose;
            if a.hazard > 0.0 {
                a.hazard -= dt;
            }
            let lane = &g.lanes[a.lane as usize];
            let mut v = (a.v + acc[i] * dt).max(0.0);
            // Never over the limit here, nor too fast to brake (hard) to the next lane's.
            let next0 = g.lanes[a.next as usize].advisory[0] * a.temper;
            let to_end = (lane.length() - a.s).max(0.0);
            v = v.min(lane.limit_at(a.s) * a.temper).min((next0 * next0 + 2.0 * B_MAX * to_end).sqrt());
            let mut ds = v * dt;
            if ds > room[i] {
                ds = room[i];
                v = v.min(ds / dt);
            }
            a.v = v;
            a.s += ds;
            a.waited = if v < 0.3 && a.hazard <= 0.0 && !a.behind_player { a.waited + dt } else { 0.0 };
            // On into the next lanes.
            loop {
                let len = g.lanes[a.lane as usize].length();
                if a.s < len {
                    break;
                }
                a.s -= len;
                a.lane = a.next;
                a.next = self.choose_next(a.lane);
            }
            // (The limit where it ends up: a few cm/s off at most.)
            a.v = a.v.min(g.lanes[a.lane as usize].limit_at(a.s) * a.temper);
            self.agents[i] = a;
        }
        // Release turns whose car is clear of the junction (its rear on the lane after).
        for i in 0..self.agents.len() {
            let a = &self.agents[i];
            if let Some((node, idx)) = a.hold {
                let t = g.nodes[node as usize].turns[idx as usize];
                let out = g.lanes[t as usize].next[0];
                // Clear of it, or (never expected) somewhere else altogether.
                if (a.lane == out && a.s - HALF_LEN > 0.5) || (a.lane != t && a.lane != out && a.next != t) {
                    let id = a.id;
                    self.holds[node as usize].retain(|(c, _)| *c != id);
                    self.agents[i].hold = None;
                }
            }
        }
        for a in &mut self.agents {
            a.pose = g.pose(a.lane, a.s, a.next);
        }
    }

    /// The player hitting a car hard stops it for a few seconds.
    fn hits(&mut self, p: Player) {
        for a in &mut self.agents {
            let d = xz(a.pose.0) - xz(p.pos);
            let dist = d.length();
            let fwd = xz(a.pose.1).normalize_or_zero();
            // Which car is ahead along the car's own path: the player ahead counts as its leader.
            let ahead = d.dot(fwd) < 0.0 && (d - fwd * d.dot(fwd)).length() < CAR_WIDTH * 0.5 + 1.4;
            a.behind_player = ahead && dist < 12.0;
            if dist > 3.6 {
                continue;
            }
            let rel = Vec2::new(p.vel.x, p.vel.z) - fwd * a.v;
            if rel.dot(d / dist.max(1e-3)) > 3.0 {
                a.hazard = HAZARD_S;
                a.v = 0.0;
            }
        }
    }

    // ------------------------------------------------------------ for tests and tools

    /// Longest any car has stood waiting (not behind the player, not after a hit), now (s).
    pub fn longest_wait(&self) -> f32 {
        self.agents.iter().map(|a| a.waited).fold(0.0, f32::max)
    }

    /// (lane, s, speed) of every car.
    pub fn placements(&self) -> Vec<(u32, f32, f32)> {
        self.agents.iter().map(|a| (a.lane, a.s, a.v)).collect()
    }

    /// A top-down SVG of the lanes (thin) and cars (arrows) within `half` m of `centre`;
    /// `trail` adds earlier positions (dots).
    pub fn svg(&self, net: &RoadNet, centre: Vec2, half: f32, player: Option<Vec3>, trail: &[Vec2]) -> String {
        let (x0, z0, w) = (centre.x - half, centre.y - half, 2.0 * half);
        let px = w / 1200.0;
        let mut s = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x0} {z0} {w} {w}" width="1200" height="1200"><rect x="{x0}" y="{z0}" width="{w}" height="{w}" fill="#1b1f1a"/>"##);
        let inside = |p: Vec2| (p - centre).abs().max_element() < half + 20.0;
        for r in &net.roads {
            let mut d: Vec<String> = r.road.pts.iter().filter(|p| inside(xz(**p))).map(|p| format!("{:.1},{:.1}", p.x, p.z)).collect();
            if d.is_empty() {
                continue;
            }
            if r.closed && inside(xz(r.road.pts[0])) && inside(xz(*r.road.pts.last().unwrap())) {
                d.push(d[0].clone());
            }
            s += &format!(r##"<polyline fill="none" stroke="#3a3d3a" stroke-linejoin="round" stroke-width="{:.1}" points="{}"/>"##, r.kind.width(), d.join(" "));
        }
        for l in &self.graph.lanes {
            if !l.pts.iter().any(|p| inside(xz(*p))) {
                continue;
            }
            let c = if l.is_turn() { "#d0a040" } else { "#8a8f88" };
            let d: Vec<String> = l.pts.iter().map(|p| format!("{:.1},{:.1}", p.x, p.z)).collect();
            s += &format!(r##"<polyline fill="none" stroke="{c}" stroke-width="{:.2}" points="{}"/>"##, px * 1.2, d.join(" "));
        }
        for t in trail {
            if inside(*t) {
                s += &format!(r##"<circle cx="{:.1}" cy="{:.1}" r="{:.2}" fill="#4a7fb0" fill-opacity="0.5"/>"##, t.x, t.y, px * 1.5);
            }
        }
        const PAINT: [&str; COLORS as usize] = ["#f2f2f2", "#a8adb3", "#2b2b2e", "#c0302a", "#2f5fb0", "#2f6b45", "#e0b030"];
        for a in &self.agents {
            let p = xz(a.pose.0);
            if !inside(p) {
                continue;
            }
            let f = xz(a.pose.1).normalize_or_zero();
            let r = Vec2::new(-f.y, f.x);
            let corners = [p + f * HALF_LEN + r * 0.9, p + f * HALF_LEN - r * 0.9, p - f * HALF_LEN - r * 0.9, p - f * HALF_LEN + r * 0.9];
            let pts: Vec<String> = corners.iter().map(|c| format!("{:.2},{:.2}", c.x, c.y)).collect();
            let stroke = if a.hazard > 0.0 { "#ff3030" } else if a.hold.is_some() { "#40e040" } else { "#000" };
            s += &format!(r##"<polygon points="{}" fill="{}" stroke="{stroke}" stroke-width="{:.2}"/>"##, pts.join(" "), PAINT[a.color as usize], px * 1.5);
            let nose = p + f * HALF_LEN;
            s += &format!(r##"<circle cx="{:.2}" cy="{:.2}" r="0.5" fill="#ffd060"/>"##, nose.x, nose.y);
        }
        if let Some(p) = player {
            s += &format!(r##"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="none" stroke="#40a0ff" stroke-width="{:.2}"/>"##, p.x, p.z, 3.0, px * 3.0);
        }
        s += "</svg>";
        s
    }
}

/// The intelligent driver model: acceleration at speed `v` wanting `v0`, behind `lead` (gap,
/// its speed).
fn idm(v: f32, v0: f32, lead: Option<(f32, f32)>) -> f32 {
    let v0 = v0.max(0.1);
    let free = if v <= v0 {
        let r = v / v0;
        A_MAX * (1.0 - r * r * r * r)
    } else {
        -(B_COMF * 2.0).min((v - v0) * 3.0)
    };
    let inter = match lead {
        Some((gap, vl)) => {
            let star = S0 + (v * HEADWAY + v * (v - vl) / (2.0 * (A_MAX * B_COMF).sqrt())).max(0.0);
            let gap = gap.max(0.1);
            A_MAX * (star / gap) * (star / gap)
        }
        None => 0.0,
    };
    (free - inter).clamp(-B_MAX, A_MAX)
}

fn push_solids(pose: (Vec3, Vec3), p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
    let c = xz(pose.0);
    if c.distance_squared(p) > (r + 3.0) * (r + 3.0) {
        return;
    }
    let f = xz(pose.1).normalize_or_zero();
    for k in SOLID_AT {
        out.push(Obstacle { pos: c + f * k, radius: SOLID_R, top: pose.0.y + SOLID_TOP, give: Give::Solid });
    }
}

/// A point that follows lanes at a set speed, turning at random: a stand-in player for
/// tests, the example and tools.
#[derive(Clone, Debug)]
pub struct LaneWalker {
    pub lane: u32,
    pub s: f32,
    pub speed: f32,
    next: u32,
    rng: Rng,
}

impl LaneWalker {
    /// Starts on the road lane nearest `at`.
    pub fn new(graph: &LaneGraph, at: Vec2, speed: f32, seed: u64) -> LaneWalker {
        let mut best = (0u32, 0.0f32, f32::MAX);
        for (i, l) in graph.lanes.iter().enumerate() {
            if l.is_turn() {
                continue;
            }
            for (k, p) in l.pts.iter().enumerate() {
                let d = xz(*p).distance_squared(at);
                if d < best.2 {
                    best = (i as u32, l.cum[k], d);
                }
            }
        }
        let mut rng = Rng::new(seed);
        let nx = &graph.lanes[best.0 as usize].next;
        let next = nx[((rng.f() * nx.len() as f32) as usize).min(nx.len() - 1)];
        LaneWalker { lane: best.0, s: best.1, speed, next, rng }
    }

    pub fn step(&mut self, graph: &LaneGraph, dt: f32) {
        self.s += self.speed * dt;
        loop {
            let len = graph.lanes[self.lane as usize].length();
            if self.s < len {
                break;
            }
            self.s -= len;
            self.lane = self.next;
            let nx = &graph.lanes[self.lane as usize].next;
            self.next = nx[((self.rng.f() * nx.len() as f32) as usize).min(nx.len() - 1)];
        }
    }

    /// As the player: position and velocity.
    pub fn player(&self, graph: &LaneGraph) -> Player {
        let (pos, fwd) = graph.pose(self.lane, self.s, self.next);
        Player { pos, vel: fwd * self.speed }
    }
}

// ---------------------------------------------------------------- ECS

/// Steps [`Traffic`] (when the resource exists) after the car sim each fixed step around the
/// driven car (else [`TrafficFocus`]), publishes the solids to [`TrafficSolids`], and draws the
/// cars (shared meshes and materials, no shadows past [`SHADOW_RANGE`] from the camera).
pub struct TrafficPlugin;

/// The cars' collision cylinders after the last step, shareable with a `Ground` (clone the
/// `Arc` into it and read with [`TrafficSolids::obstacles`]).
#[derive(Resource, Clone, Default)]
pub struct TrafficSolids(pub Arc<RwLock<Vec<(Vec3, Vec3)>>>);

impl TrafficSolids {
    /// Cylinders for cars within `r` of `p`, appended to `out`.
    pub fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
        if let Ok(poses) = self.0.read() {
            for pose in poses.iter() {
                push_solids(*pose, p, r, out);
            }
        }
    }
}

impl Plugin for TrafficPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TrafficSolids>()
            .add_systems(FixedUpdate, step.after(crate::drive::SimulateSet).run_if(resource_exists::<Traffic>))
            .add_systems(Update, (draw.run_if(resource_exists::<Traffic>), clear.run_if(not(resource_exists::<Traffic>))));
    }
}

/// Where traffic gathers when there's no driven car (a menu backdrop, a spectator, the
/// `traffic_view` example).
#[derive(Resource, Clone, Copy, Debug)]
pub struct TrafficFocus(pub Player);

fn step(time: Res<Time<Fixed>>, mut traffic: ResMut<Traffic>, cars: Query<&crate::drive::DrivenCar>, focus: Option<Res<TrafficFocus>>, solids: Res<TrafficSolids>) {
    let player = cars.iter().next().map(|c| Player { pos: c.car.pos, vel: c.car.vel }).or(focus.map(|f| f.0));
    traffic.step(time.delta_secs(), player);
    if let Ok(mut s) = solids.0.write() {
        s.clear();
        s.extend(traffic.agents.iter().map(|a| a.pose));
    }
}

/// A drawn traffic car (the root; its meshes are children).
#[derive(Component)]
pub struct TrafficCar {
    pub id: u32,
    shadows: bool,
}

/// Shared meshes and materials for every traffic car.
#[derive(Resource)]
struct TrafficArt {
    body: Handle<Mesh>,
    glass: Handle<Mesh>,
    wheels: Handle<Mesh>,
    paint: Vec<Handle<StandardMaterial>>,
    glass_mat: Handle<StandardMaterial>,
    tyre_mat: Handle<StandardMaterial>,
}

/// Cars beyond this from the camera don't cast shadows (m).
pub const SHADOW_RANGE: f32 = 70.0;

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    fixed: Res<Time<Fixed>>,
    traffic: Res<Traffic>,
    art: Option<Res<TrafficArt>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    mut shown: Query<(Entity, &mut TrafficCar, &mut Transform, &Children)>,
    cams: Query<&GlobalTransform, With<engine::gfx::MainCamera>>,
) {
    let (Some(mut meshes), Some(mut materials)) = (meshes, materials) else { return };
    let Some(art) = art else {
        commands.insert_resource(make_art(&mut meshes, &mut materials));
        return;
    };
    let a = fixed.overstep_fraction();
    let cam = cams.iter().next().map(|g| g.translation());
    let pose = |c: &CarView| {
        let p = c.prev_pos.lerp(c.pos, a);
        let f = c.prev_fwd.lerp(c.fwd, a).normalize_or(Vec3::Z);
        Transform::from_translation(p).looking_to(f, Vec3::Y)
    };
    let mut views: Vec<CarView> = traffic.cars().collect();
    views.sort_by_key(|c| c.id);
    let mut seen = vec![false; views.len()];
    for (e, mut car, mut tf, children) in &mut shown {
        match views.binary_search_by_key(&car.id, |c| c.id) {
            Ok(k) => {
                seen[k] = true;
                *tf = pose(&views[k]);
                let near = cam.is_some_and(|c| c.distance(tf.translation) < SHADOW_RANGE);
                if near != car.shadows {
                    car.shadows = near;
                    for ch in children.iter() {
                        if near {
                            commands.entity(ch).remove::<bevy::light::NotShadowCaster>();
                        } else {
                            commands.entity(ch).insert(bevy::light::NotShadowCaster);
                        }
                    }
                }
            }
            Err(_) => commands.entity(e).despawn(),
        }
    }
    for (k, c) in views.iter().enumerate() {
        if seen[k] {
            continue;
        }
        let paint = art.paint[c.color as usize % art.paint.len()].clone();
        commands
            .spawn((Name::new("traffic car"), engine::level::GeneratedBy("pacenote::traffic"), TrafficCar { id: c.id, shadows: false }, pose(c), Visibility::default(), engine::render_budget::Actor, DespawnOnExit(engine::flow::AppState::InGame)))
            .with_children(|p| {
                p.spawn((Mesh3d(art.body.clone()), MeshMaterial3d(paint), bevy::light::NotShadowCaster));
                p.spawn((Mesh3d(art.glass.clone()), MeshMaterial3d(art.glass_mat.clone()), bevy::light::NotShadowCaster));
                p.spawn((Mesh3d(art.wheels.clone()), MeshMaterial3d(art.tyre_mat.clone()), bevy::light::NotShadowCaster));
            });
    }
}

fn clear(mut commands: Commands, shown: Query<Entity, With<TrafficCar>>) {
    for e in &shown {
        commands.entity(e).despawn();
    }
}

fn make_art(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> TrafficArt {
    const PAINT: [[f32; 3]; COLORS as usize] = [[0.85, 0.85, 0.83], [0.55, 0.57, 0.6], [0.04, 0.04, 0.05], [0.55, 0.06, 0.05], [0.08, 0.2, 0.5], [0.08, 0.25, 0.14], [0.8, 0.6, 0.12]];
    let paint = PAINT
        .iter()
        .map(|c| materials.add(StandardMaterial { base_color: Color::srgb(c[0], c[1], c[2]), metallic: 0.3, perceptual_roughness: 0.35, ..default() }))
        .collect();
    let glass_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.05, 0.06, 0.08), metallic: 0.0, perceptual_roughness: 0.08, reflectance: 0.6, ..default() });
    let tyre_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.03, 0.03, 0.03), perceptual_roughness: 0.9, ..default() });
    let (body, glass, wheels) = car_meshes();
    TrafficArt { body: meshes.add(body), glass: meshes.add(glass), wheels: meshes.add(wheels), paint, glass_mat, tyre_mat }
}

/// A low-poly car (local −Z forward, origin on the ground at the centre): body, cabin glass
/// and the four wheels, each one mesh.
fn car_meshes() -> (Mesh, Mesh, Mesh) {
    let mut body = MeshBuild::default();
    // Side profiles (z back, y up), extruded across x. Convex each.
    let lower = [Vec2::new(2.2, 0.3), Vec2::new(2.22, 0.92), Vec2::new(-1.55, 0.86), Vec2::new(-2.2, 0.62), Vec2::new(-2.15, 0.3)];
    body.prism(&lower, -0.9, 0.9);
    let roof = [Vec2::new(1.05, 1.36), Vec2::new(1.0, 1.42), Vec2::new(-0.45, 1.42), Vec2::new(-0.5, 1.36)];
    body.prism(&roof, -0.74, 0.74);
    let mut glass = MeshBuild::default();
    let cabin = [Vec2::new(1.55, 0.9), Vec2::new(1.05, 1.37), Vec2::new(-0.5, 1.37), Vec2::new(-1.35, 0.88)];
    glass.prism(&cabin, -0.8, 0.8);
    let mut wheels = MeshBuild::default();
    let circle: Vec<Vec2> = (0..10).map(|i| {
        let a = i as f32 / 10.0 * core::f32::consts::TAU;
        Vec2::new(libm::cosf(a), libm::sinf(a)) * 0.33
    }).collect();
    for (x, z) in [(-0.8, -1.35), (0.8, -1.35), (-0.8, 1.35), (0.8, 1.35)] {
        let prof: Vec<Vec2> = circle.iter().map(|p| *p + Vec2::new(z, 0.33)).collect();
        wheels.prism(&prof, x - 0.13, x + 0.13);
    }
    (body.mesh(), glass.mesh(), wheels.mesh())
}

/// Flat-shaded triangles.
#[derive(Default)]
struct MeshBuild {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
}

impl MeshBuild {
    /// A triangle facing `out` (wound to face it).
    fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, out: Vec3) {
        let n = (b - a).cross(c - a);
        let (b, c) = if n.dot(out) < 0.0 { (c, b) } else { (b, c) };
        let n = (b - a).cross(c - a).normalize_or_zero();
        for p in [a, b, c] {
            self.pos.push(p.to_array());
            self.nrm.push(n.to_array());
        }
    }

    /// A convex side profile (`(z, y)` points) extruded from `x0` to `x1`.
    fn prism(&mut self, prof: &[Vec2], x0: f32, x1: f32) {
        let c: Vec2 = prof.iter().copied().sum::<Vec2>() / prof.len() as f32;
        let at = |p: Vec2, x: f32| Vec3::new(x, p.y, p.x);
        for i in 1..prof.len() - 1 {
            self.tri(at(prof[0], x0), at(prof[i], x0), at(prof[i + 1], x0), -Vec3::X);
            self.tri(at(prof[0], x1), at(prof[i], x1), at(prof[i + 1], x1), Vec3::X);
        }
        for i in 0..prof.len() {
            let (p, q) = (prof[i], prof[(i + 1) % prof.len()]);
            let mid = (p + q) * 0.5 - c;
            let out = Vec3::new(0.0, mid.y, mid.x);
            self.tri(at(p, x0), at(q, x0), at(q, x1), out);
            self.tri(at(p, x0), at(q, x1), at(p, x1), out);
        }
    }

    fn mesh(self) -> Mesh {
        use bevy::asset::RenderAssetUsages;
        use bevy::mesh::{Indices, PrimitiveTopology};
        let n = self.pos.len() as u32;
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
            .with_inserted_indices(Indices::U32((0..n).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> (RoadNet, Arc<LaneGraph>) {
        let net = super::super::roads::generate(super::super::SEED);
        let g = Arc::new(LaneGraph::build(&net));
        (net, g)
    }

    #[test]
    fn every_lane_goes_somewhere() {
        let (net, g) = world();
        let roads = g.lanes.iter().filter(|l| !l.is_turn()).count();
        assert!(roads >= 2 * net.roads.len(), "{roads} road lanes for {} roads", net.roads.len());
        for (i, l) in g.lanes.iter().enumerate() {
            assert!(!l.next.is_empty(), "lane {i} ({:?}) is a dead end", l.kind);
            assert!(l.length() > 1.0, "lane {i} is {} m", l.length());
            for &n in &l.next {
                // The next lane starts where this one ends.
                let gap = xz(*l.pts.last().unwrap()).distance(xz(g.lanes[n as usize].pts[0]));
                assert!(gap < 0.01, "lane {i} → {n}: {gap} m apart");
            }
        }
        // Dead ends turn round; junctions don't.
        let dead = g.nodes.iter().filter(|n| n.dead_end).count();
        let forest = net.roads.iter().filter(|r| r.kind == RoadKind::Forest).count();
        assert_eq!(dead, forest, "one dead end per forest road");
        for l in &g.lanes {
            if let LaneKind::Turn { node, turn, .. } = l.kind {
                assert_eq!(turn == Turn::UTurn, g.nodes[node as usize].dead_end, "U-turns only at dead ends");
            }
        }
        // Every road lane can reach every other (strongly connected).
        let reach = |from: u32, back: bool| {
            let mut seen = vec![false; g.lanes.len()];
            let mut stack = vec![from];
            let mut prev: Vec<Vec<u32>> = vec![Vec::new(); g.lanes.len()];
            if back {
                for (i, l) in g.lanes.iter().enumerate() {
                    for &n in &l.next {
                        prev[n as usize].push(i as u32);
                    }
                }
            }
            while let Some(i) = stack.pop() {
                if std::mem::replace(&mut seen[i as usize], true) {
                    continue;
                }
                let nx = if back { &prev[i as usize] } else { &g.lanes[i as usize].next };
                stack.extend(nx.iter().copied());
            }
            seen
        };
        let (f, b) = (reach(0, false), reach(0, true));
        for (i, l) in g.lanes.iter().enumerate() {
            assert!(f[i] && b[i], "lane {i} ({:?}) is cut off", l.kind);
        }
        // Junction types: town crossroads have straight, left and right.
        let centre = g.nodes.iter().find(|n| n.pos.length() < 1.0).expect("a node at the town centre");
        let kinds: Vec<Turn> = centre.turns.iter().map(|&t| match g.lanes[t as usize].kind {
            LaneKind::Turn { turn, .. } => turn,
            _ => unreachable!(),
        }).collect();
        assert_eq!(kinds.len(), 12);
        for t in [Turn::Straight, Turn::Left, Turn::Right] {
            assert_eq!(kinds.iter().filter(|k| **k == t).count(), 4, "{t:?}");
        }
    }

    #[test]
    fn lanes_keep_right_and_on_the_road() {
        let (net, g) = world();
        for l in &g.lanes {
            let LaneKind::Road { road, .. } = l.kind else { continue };
            let r = &net.roads[road].road;
            for k in (0..l.pts.len() - 1).step_by(7) {
                let p = xz(l.pts[k]);
                let at = r.nearest(p, 20.0).expect("near its road");
                let d = xz(l.pts[k + 1] - l.pts[k]);
                let travel_left = Vec2::new(d.y, -d.x);
                let road_left = Vec2::new(r.dir[at.i].y, -r.dir[at.i].x);
                // Right of travel: lateral (left of the road's direction) has the opposite
                // sign to how the travel direction relates to the road's.
                let side = at.lateral * travel_left.dot(road_left).signum();
                assert!(side < -1.0, "lane on the left: {side} at {p}");
                assert!(at.lateral.abs() < r.half_width(&at) - 0.8, "lane at the edge");
            }
        }
    }

    /// Runs `minutes` of traffic around a walker, checking invariants as it goes. Returns
    /// (cars per step on average, the longest wait seen).
    fn run(cars: usize, minutes: f32, seed: u64, speed: f32, check: bool) -> (Traffic, f32, f32) {
        let (net, g) = world();
        let mut t = Traffic::with_graph(g.clone(), TrafficConfig { cars, seed, ..default() });
        let (spawn, _) = super::super::roads::spawn(&net);
        let mut walker = LaneWalker::new(&g, xz(spawn), speed, seed ^ 1);
        let dt = 1.0 / 60.0;
        let steps = (minutes * 60.0 * 60.0) as usize;
        let (mut total, mut worst) = (0usize, 0.0f32);
        for k in 0..steps {
            walker.step(&g, dt);
            let p = walker.player(&g);
            t.step(dt, Some(p));
            total += t.len();
            worst = worst.max(t.longest_wait());
            if !check || k % 6 != 0 {
                continue;
            }
            // Never overlapping in a lane (centres a car length apart).
            for list in &t.on_lane {
                for w in list.windows(2) {
                    let (a, b) = (&t.agents[w[0] as usize], &t.agents[w[1] as usize]);
                    if a.lane == b.lane {
                        assert!(b.s - a.s >= CAR_LEN, "step {k}: cars {} and {} overlap on lane {} ({} m)", a.id, b.id, a.lane, b.s - a.s);
                    }
                }
            }
            // Nor anywhere else (bodies are 1.8 m wide; opposing forest lanes are 3.2 m apart).
            for i in 0..t.agents.len() {
                for j in i + 1..t.agents.len() {
                    let (a, b) = (&t.agents[i], &t.agents[j]);
                    let d = xz(a.pose.0).distance(xz(b.pose.0));
                    assert!(d > 2.4, "step {k}: cars {} (lane {} {:?}) and {} (lane {} {:?}) {d} m apart", a.id, a.lane, g.lanes[a.lane as usize].kind, b.id, b.lane, g.lanes[b.lane as usize].kind);
                }
            }
            for a in &t.agents {
                let lane = &g.lanes[a.lane as usize];
                // Within the limit.
                assert!(a.v <= lane.limit_at(a.s) + 0.01, "car {} at {} m/s, limit {}", a.id, a.v, lane.limit_at(a.s));
                // On a road.
                let p = xz(a.pose.0);
                let out = net
                    .roads
                    .iter()
                    .filter_map(|r| r.road.nearest(p, 30.0).map(|at| at.lateral.abs() - r.road.half_width(&at)))
                    .fold(f32::MAX, f32::min);
                assert!(out < 0.0, "car {} off the road at {p} ({:?}): {out} m out", a.id, lane.kind);
                // Near the player.
                assert!(p.distance(xz(p_pos(&walker, &g))) < t.config.despawn + 5.0);
            }
        }
        (t, total as f32 / steps as f32, worst)
    }

    fn p_pos(w: &LaneWalker, g: &LaneGraph) -> Vec3 {
        w.player(g).pos
    }

    #[test]
    fn ten_minutes_of_sixty_cars() {
        let (t, mean, worst) = run(60, 10.0, 7, 13.0, true);
        eprintln!("60 cars, 10 min: {mean:.1} cars on average, longest wait {worst:.1} s, {} cars spawned", t.next_id - 1);
        assert!(mean > 50.0, "kept ~60 cars around: {mean}");
        assert!(worst < 60.0, "someone waited {worst} s");
        assert!(t.next_id > 60, "cars came and went: {}", t.next_id);
    }

    #[test]
    fn other_seeds_and_a_fast_player() {
        for (seed, speed) in [(1, 25.0), (2, 30.0), (3, 8.0)] {
            let (_, mean, worst) = run(60, 5.0, seed, speed, true);
            eprintln!("seed {seed}, player at {speed} m/s: {mean:.1} cars, longest wait {worst:.1} s");
            assert!(worst < 60.0, "seed {seed}: someone waited {worst} s");
        }
    }

    #[test]
    fn deterministic() {
        let (a, _, _) = run(40, 1.0, 3, 13.0, false);
        let (b, _, _) = run(40, 1.0, 3, 13.0, false);
        let (pa, pb) = (a.placements(), b.placements());
        assert_eq!(pa.len(), pb.len());
        for (x, y) in pa.iter().zip(&pb) {
            assert_eq!(x.0, y.0);
            assert_eq!(x.1.to_bits(), y.1.to_bits());
            assert_eq!(x.2.to_bits(), y.2.to_bits());
        }
    }

    #[test]
    fn cars_queue_behind_a_stopped_player() {
        let (_, g) = world();
        // A long straight-ish country lane; the player stands on it 300 m along.
        let (li, lane) = g.lanes.iter().enumerate().find(|(_, l)| matches!(l.kind, LaneKind::Road { .. }) && l.length() > 600.0).unwrap();
        let mut t = Traffic::with_graph(g.clone(), TrafficConfig { cars: 0, ..default() });
        let next = lane.next[0];
        for k in 0..5 {
            let s = 20.0 + k as f32 * 30.0;
            let pose = g.pose(li as u32, s, next);
            t.agents.push(Agent { id: k + 1, lane: li as u32, s, v: 10.0, next, temper: 1.0, color: 0, hazard: 0.0, waited: 0.0, asked: None, hold: None, pose, prev: pose, behind_player: false });
        }
        let stand = lane.point(300.0);
        let p = Player { pos: stand, vel: Vec3::ZERO };
        for _ in 0..60 * 60 {
            t.step(1.0 / 60.0, Some(p));
        }
        for a in &t.agents {
            assert!(a.s < 300.0 - HALF_LEN - 2.0, "car {} drove into the player: s {}", a.id, a.s);
            assert!(a.v < 0.1);
        }
        let front = t.agents.iter().map(|a| a.s).fold(0.0, f32::max);
        assert!(front > 300.0 - 12.0, "queued close behind: {front}");
    }

    #[test]
    fn hit_hard_a_car_stops_then_goes_on() {
        let (_, g) = world();
        let (li, lane) = g.lanes.iter().enumerate().find(|(_, l)| matches!(l.kind, LaneKind::Road { .. }) && l.length() > 600.0).unwrap();
        let mut t = Traffic::with_graph(g.clone(), TrafficConfig { cars: 0, ..default() });
        let next = lane.next[0];
        let pose = g.pose(li as u32, 100.0, next);
        t.agents.push(Agent { id: 1, lane: li as u32, s: 100.0, v: 10.0, next, temper: 1.0, color: 0, hazard: 0.0, waited: 0.0, asked: None, hold: None, pose, prev: pose, behind_player: false });
        // The player slams in from the side.
        let side = Vec3::new(-pose.1.z, 0.0, pose.1.x);
        t.step(1.0 / 60.0, Some(Player { pos: pose.0 + side * 2.5, vel: -side * 12.0 }));
        assert!(t.agents[0].hazard > 0.0 && t.agents[0].v == 0.0);
        let far = Player { pos: pose.0 + side * 200.0, vel: Vec3::ZERO };
        for _ in 0..60 * 3 {
            t.step(1.0 / 60.0, Some(far));
        }
        assert!(t.agents[0].v == 0.0, "still standing after 3 s");
        for _ in 0..60 * 3 {
            t.step(1.0 / 60.0, Some(far));
        }
        assert!(t.agents[0].v > 1.0, "going again");
        // Solid meanwhile.
        let mut out = Vec::new();
        t.obstacles(xz(t.agents[0].pose.0), 5.0, &mut out);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn car_mesh_faces_outward() {
        let (body, glass, wheels) = car_meshes();
        for m in [body, glass, wheels] {
            assert!(m.count_vertices() > 0);
        }
    }
}

#[cfg(test)]
mod pictures {
    use super::*;

    /// `PACENOTE_TRAFFIC_SVG=dir cargo test --lib open::traffic::pictures -- --ignored`: frames
    /// of traffic around a walker in town and on the ring, as SVG.
    #[test]
    #[ignore]
    fn draw() {
        let Ok(dir) = std::env::var("PACENOTE_TRAFFIC_SVG") else { return };
        let net = super::super::roads::generate(super::super::SEED);
        let g = Arc::new(LaneGraph::build(&net));
        let mut t = Traffic::with_graph(g.clone(), TrafficConfig { cars: 60, ..default() });
        let mut w = LaneWalker::new(&g, Vec2::new(-290.0, 0.0), 12.0, 5);
        let mut trail = Vec::new();
        for k in 0..60 * 60 * 3 {
            w.step(&g, 1.0 / 60.0);
            let p = w.player(&g);
            t.step(1.0 / 60.0, Some(p));
            if k % 30 == 0 {
                trail.push(xz(p.pos));
            }
            if k % (60 * 30) == 0 {
                let svg = t.svg(&net, xz(p.pos), 250.0, Some(p.pos), &trail);
                std::fs::write(format!("{dir}/traffic_{:03}.svg", k / 60), svg).unwrap();
            }
        }
    }
}
