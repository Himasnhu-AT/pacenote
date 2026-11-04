//! Where roads go: places on the land and an anisotropic A* between them.
//!
//! Roads solve the terrain (docs/RALLY_MAP.md): a path on a 10 m grid in 16 headings, each
//! step costing its length times grade above the target, cross-slope and turning. Where the
//! land is steeper than a road may climb the cheapest path traverses the slope and turns back
//! in switchbacks; the turn limit per step decides the tightest hairpin (~8.5 m radius at
//! three 22.5° headings per 10 m). Pure and deterministic.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use bevy::math::Vec2;

use super::land::{HALF, Land};

/// Grid spacing of the planner (m).
pub const GRID: f32 = 10.0;
const G: usize = (2.0 * HALF / GRID) as usize + 1;
/// Keep this far (m) inside the region.
const MARGIN: f32 = 70.0;

/// The 16 step directions (grid cells): the 8 neighbours and the 8 knight moves, in angle order.
const DIRS: [(i32, i32); 16] = [(1, 0), (2, 1), (1, 1), (1, 2), (0, 1), (-1, 2), (-1, 1), (-2, 1), (-1, 0), (-2, -1), (-1, -1), (-1, -2), (0, -1), (1, -2), (1, -1), (2, -1)];

/// The places the roads connect.
#[derive(Clone, Debug)]
pub struct Places {
    /// Valley village, on the river's north bank.
    pub village: Vec2,
    /// Service park, east of the village on the valley floor.
    pub park: Vec2,
    /// Where the valley B-road leaves the map's west and east sides (turning up the hills).
    pub west: Vec2,
    pub east: Vec2,
    /// Logging camp on the escarpment.
    pub camp: Vec2,
    /// Quarry in the basin.
    pub quarry: Vec2,
    /// Radio tower, the plateau's high point.
    pub tower: Vec2,
    /// Plateau corners the hill roads arrive at.
    pub pass_w: Vec2,
    pub pass_e: Vec2,
    /// Two river crossings and the southern hills between them.
    pub ford_w: Vec2,
    pub ford_e: Vec2,
    pub south: Vec2,
}

/// The flattest point in a box (slope, plus `pull` per m from the box centre), sampled every
/// 20 m; `score` adds a term (lower is better).
fn best(land: &Land, lo: Vec2, hi: Vec2, score: impl Fn(Vec2) -> f32) -> Vec2 {
    let mut out = (lo + hi) * 0.5;
    let mut best = f32::MAX;
    let mut z = lo.y;
    while z <= hi.y {
        let mut x = lo.x;
        while x <= hi.x {
            let p = Vec2::new(x, z);
            let mut sl = 0.0f32;
            for d in [Vec2::X, Vec2::Y, -Vec2::X, -Vec2::Y] {
                sl = sl.max(land.slope(p + d * 30.0));
            }
            let s = land.slope(p) + sl + score(p);
            if s < best {
                best = s;
                out = p;
            }
            x += 20.0;
        }
        z += 20.0;
    }
    out
}

/// The point on the river nearest `x` (on the line, west to east).
fn river_at(land: &Land, x: f32) -> Vec2 {
    let mut best = land.river[0];
    for p in &land.river {
        if (p.x - x).abs() < (best.x - x).abs() {
            best = *p;
        }
    }
    best
}

pub fn places(land: &Land) -> Places {
    let north_bank = |x: f32, off: f32| river_at(land, x) + Vec2::Y * off;
    let valley = |p: Vec2, want: f32| {
        let d = land.river_distance(p);
        let north = p.y > river_at(land, p.x).y;
        (d - want).abs() * 0.01 + if north { 0.0 } else { 10.0 }
    };
    let v0 = north_bank(-350.0, 160.0);
    let village = best(land, v0 - Vec2::new(250.0, 90.0), v0 + Vec2::new(250.0, 90.0), |p| valley(p, 160.0) + p.distance(v0) * 0.0005);
    let p0 = north_bank(420.0, 150.0);
    let park = best(land, p0 - Vec2::new(200.0, 80.0), p0 + Vec2::new(200.0, 80.0), |p| valley(p, 150.0));
    let west = north_bank(-1780.0, 190.0);
    let east = north_bank(1780.0, 190.0);
    let camp = best(land, Vec2::new(-1500.0, -350.0), Vec2::new(-700.0, 350.0), |p| p.distance(Vec2::new(-1100.0, 0.0)) * 0.0003);
    let quarry = best(land, Vec2::new(800.0, -550.0), Vec2::new(1500.0, 150.0), |p| p.distance(Vec2::new(1150.0, -200.0)) * 0.0003);
    let tower = {
        let mut t = Vec2::new(0.0, 1500.0);
        let mut z = 1250.0;
        while z <= 1800.0 {
            let mut x = -700.0;
            while x <= 700.0 {
                let p = Vec2::new(x, z);
                if land.coarse(p) > land.coarse(t) {
                    t = p;
                }
                x += 20.0;
            }
            z += 20.0;
        }
        t
    };
    let pass_w = best(land, Vec2::new(-1750.0, 1050.0), Vec2::new(-1050.0, 1650.0), |p| p.distance(Vec2::new(-1400.0, 1350.0)) * 0.0003);
    let pass_e = best(land, Vec2::new(1050.0, 1050.0), Vec2::new(1750.0, 1650.0), |p| p.distance(Vec2::new(1400.0, 1350.0)) * 0.0003);
    let ford_w = river_at(land, -900.0);
    let ford_e = river_at(land, 900.0);
    let south = best(land, Vec2::new(-400.0, -1900.0), Vec2::new(400.0, -1650.0), |_| 0.0);
    Places { village, park, west, east, camp, quarry, tower, pass_w, pass_e, ford_w, ford_e, south }
}

/// How a road is planned.
#[derive(Clone, Copy, Debug)]
pub struct Rules {
    /// Comfortable and hard-limit grade (rise per metre).
    pub grade: f32,
    pub max_grade: f32,
    /// Most heading change per step, in 22.5° units (3 ≈ 8.5 m hairpins, 2 ≈ 13 m).
    pub max_turn: usize,
    pub w_grade: f32,
    pub w_cross: f32,
    pub w_turn: f32,
    /// May cross the river (a bridge).
    pub bridge: bool,
}

impl Rules {
    pub const TARMAC: Rules = Rules { grade: 0.05, max_grade: 0.11, max_turn: 2, w_grade: 500.0, w_cross: 6.0, w_turn: 1.2, bridge: false };
    pub const GRAVEL: Rules = Rules { grade: 0.05, max_grade: 0.13, max_turn: 3, w_grade: 400.0, w_cross: 4.0, w_turn: 0.7, bridge: false };
    pub const FOREST: Rules = Rules { grade: 0.04, max_grade: 0.14, max_turn: 3, w_grade: 350.0, w_cross: 3.0, w_turn: 0.45, bridge: false };
}

/// The planning grid: heights and slopes at the cell centres, roads already built.
pub struct Planner<'a> {
    land: &'a Land,
    h: Vec<f32>,
    /// Cells next to a road already built (no new road runs along it).
    taken: Vec<bool>,
    /// Cells on a road already built (a new road may only cross one near its own ends).
    core: Vec<bool>,
    river: Vec<bool>,
    /// A little cost that varies smoothly over the land, so roads on the flat meander.
    rough: Vec<f32>,
}

fn cell_of(p: Vec2) -> (i32, i32) {
    (((p.x + HALF) / GRID).round() as i32, ((p.y + HALF) / GRID).round() as i32)
}

fn centre(x: i32, z: i32) -> Vec2 {
    Vec2::new(-HALF + x as f32 * GRID, -HALF + z as f32 * GRID)
}

impl<'a> Planner<'a> {
    pub fn new(land: &'a Land) -> Planner<'a> {
        let mut h = vec![0.0; G * G];
        let mut river = vec![false; G * G];
        let mut rough = vec![0.0; G * G];
        for z in 0..G {
            for x in 0..G {
                let p = centre(x as i32, z as i32);
                h[z * G + x] = land.coarse(p);
                river[z * G + x] = land.river_distance(p) < 14.0;
                rough[z * G + x] = (crate::stage::fbm(p / 260.0, 3, land.seed ^ 0x2A0) * 0.5 + 0.5).clamp(0.0, 1.0) * 0.9;
            }
        }
        Planner { land, h, taken: vec![false; G * G], core: vec![false; G * G], river, rough }
    }

    /// Marks a road's cells (and `r` m around) as taken, and the cells it runs through.
    pub fn take(&mut self, pts: &[Vec2], r: f32) {
        let k = (r / GRID).ceil() as i32;
        for p in pts.iter().step_by(2) {
            let (cx, cz) = cell_of(*p);
            for dz in -k..=k {
                for dx in -k..=k {
                    let (x, z) = (cx + dx, cz + dz);
                    if x >= 0 && z >= 0 && (x as usize) < G && (z as usize) < G {
                        let d = centre(x, z).distance(*p);
                        let i = z as usize * G + x as usize;
                        if d <= r {
                            self.taken[i] = true;
                        }
                        if d <= GRID * 0.75 {
                            self.core[i] = true;
                        }
                    }
                }
            }
        }
    }

    fn height(&self, x: i32, z: i32) -> f32 {
        self.h[z as usize * G + x as usize]
    }

    /// The taken cells (to put back with [`restore`](Self::restore) if a plan is dropped).
    pub fn snapshot(&self) -> (Vec<bool>, Vec<bool>) {
        (self.taken.clone(), self.core.clone())
    }

    pub fn restore(&mut self, (taken, core): (Vec<bool>, Vec<bool>)) {
        self.taken = taken;
        self.core = core;
    }

    /// The cheapest path from `a` to `b` (cell centres, then the exact ends), leaving `a`
    /// heading about `dir` and arriving heading about `arrive` if given. Near either end (`free` m) taken cells cost nothing extra.
    /// With `no_cross`, the path never runs over a road already built (except within 24 m of
    /// its ends).
    #[allow(clippy::too_many_arguments)]
    pub fn route(&self, a: Vec2, dir: Option<Vec2>, b: Vec2, arrive: Option<Vec2>, rules: Rules, free: f32, no_cross: bool) -> Option<Vec<Vec2>> {
        let (ax, az) = cell_of(a);
        let (bx, bz) = cell_of(b);
        let n = G * G * 16;
        let mut cost = vec![f32::INFINITY; n];
        let mut from = vec![u32::MAX; n];
        let mut heap = BinaryHeap::new();
        let lo = ((MARGIN / GRID) as i32, (G as i32) - 1 - (MARGIN / GRID) as i32);
        let state = |x: i32, z: i32, d: usize| (z as usize * G + x as usize) * 16 + d;
        let heading = |v: Vec2| -> usize {
            let a = libm::atan2f(v.y, v.x).rem_euclid(core::f32::consts::TAU);
            ((a / (core::f32::consts::TAU / 16.0)).round() as usize) % 16
        };
        let starts: Vec<usize> = match dir {
            Some(v) => vec![heading(v)],
            None => (0..16).collect(),
        };
        for d in starts {
            let s = state(ax, az, d);
            cost[s] = 0.0;
            heap.push(Reverse((0u32, s as u32)));
        }
        let goal = centre(bx, bz);
        let h_est = |x: i32, z: i32| centre(x, z).distance(goal);
        let key = |f: f32| f.to_bits();
        let mut done = None;
        while let Some(Reverse((_, s))) = heap.pop() {
            let s = s as usize;
            let (cell, d) = (s / 16, s % 16);
            let (x, z) = ((cell % G) as i32, (cell / G) as i32);
            // At the goal, heading about `arrive` (if given).
            let facing = arrive.is_none_or(|v| {
                let h = heading(v);
                let diff = (d + 16 - h) % 16;
                diff <= 1 || diff >= 15
            });
            if x == bx && z == bz && facing {
                done = Some(s);
                break;
            }
            let c0 = cost[s];
            let h0 = self.height(x, z);
            for t in 0..=(2 * rules.max_turn) {
                let nd = (d + 16 + t - rules.max_turn) % 16;
                let (dx, dz) = DIRS[nd];
                let (nx, nz) = (x + dx, z + dz);
                if nx < lo.0 || nz < lo.0 || nx > lo.1 || nz > lo.1 {
                    continue;
                }
                let ni = nz as usize * G + nx as usize;
                if self.river[ni] && !rules.bridge {
                    continue;
                }
                if no_cross {
                    // The cell and, for a knight's move, the one it passes over.
                    let (mx, mz) = (x + (dx as f32 * 0.5).round() as i32, z + (dz as f32 * 0.5).round() as i32);
                    let p1 = centre(nx, nz);
                    let near_end = p1.distance(a) < 24.0 || p1.distance(b) < 24.0;
                    if !near_end && (self.core[ni] || self.core[mz as usize * G + mx as usize]) {
                        continue;
                    }
                }
                let step = Vec2::new(dx as f32, dz as f32);
                let len = step.length() * GRID;
                let h1 = self.height(nx, nz);
                let s_long = (h1 - h0) / len;
                // Cross-slope: the land's fall across the step at its middle.
                let mid = (centre(x, z) + centre(nx, nz)) * 0.5;
                let side = step.normalize().perp() * GRID;
                let s_cross = (self.land.coarse(mid + side) - self.land.coarse(mid - side)) / (2.0 * GRID);
                let turn = (t as f32 - rules.max_turn as f32) * core::f32::consts::TAU / 16.0;
                let over = (s_long.abs() - rules.grade).max(0.0);
                let mut k = 1.0 + rules.w_grade * over * over + rules.w_cross * s_cross * s_cross + rules.w_turn * turn * turn;
                // Steeper than a road may climb: only where nothing else gets through (the road
                // is cut into the land there).
                let steep = (s_long.abs() - rules.max_grade).max(0.0);
                k += 4000.0 * steep * steep + if steep > 0.0 { 20.0 } else { 0.0 };
                let p1 = centre(nx, nz);
                if self.taken[ni] && p1.distance(a) > free && p1.distance(b) > free {
                    k += 60.0;
                }
                k += self.rough[ni];
                if self.river[ni] {
                    k += 6.0;
                }
                let c = c0 + len * k;
                let ns = state(nx, nz, nd);
                if c < cost[ns] {
                    cost[ns] = c;
                    from[ns] = s as u32;
                    heap.push(Reverse((key(c + h_est(nx, nz)), ns as u32)));
                }
            }
        }
        let mut s = done?;
        let mut cells = vec![];
        loop {
            let cell = s / 16;
            cells.push(centre((cell % G) as i32, (cell / G) as i32));
            if from[s] == u32::MAX {
                break;
            }
            s = from[s] as usize;
        }
        cells.reverse();
        cells[0] = a;
        let last = cells.len() - 1;
        cells[last] = b;
        Some(cells)
    }
}

/// Corner-cutting smoothing (Chaikin), keeping the ends: the grid path becomes a curve whose
/// tightest radius follows the turns the planner allowed.
pub fn chaikin(pts: &[Vec2], rounds: usize) -> Vec<Vec2> {
    let mut v = pts.to_vec();
    for _ in 0..rounds {
        if v.len() < 3 {
            break;
        }
        let mut out = vec![v[0]];
        for w in v.windows(2) {
            out.push(w[0] * 0.75 + w[1] * 0.25);
            out.push(w[0] * 0.25 + w[1] * 0.75);
        }
        out.push(v[v.len() - 1]);
        // The first and last cuts sit next to the kept ends: drop them so the road leaves and
        // arrives along the first and last steps.
        out.remove(1);
        out.remove(out.len() - 2);
        v = out;
    }
    v
}

/// Whether segments `a0-a1` and `b0-b1` cross.
fn cross(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2) -> bool {
    let d = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
    let (d1, d2) = (d(b0, b1, a0), d(b0, b1, a1));
    let (d3, d4) = (d(a0, a1, b0), d(a0, a1, b1));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// The path with any loop it makes cut out: where it crosses itself or comes back to a point it
/// passed, it goes straight on from there (a road never runs over itself).
pub fn untangle(pts: &[Vec2]) -> Vec<Vec2> {
    let mut v = pts.to_vec();
    let mut i = 0;
    while i + 3 < v.len() {
        let mut cut = None;
        for j in (i + 2..v.len() - 1).rev() {
            // Crossing, or back on the same spot (out and back along itself).
            if cross(v[i], v[i + 1], v[j], v[j + 1]) || (j >= i + 3 && v[j].distance(v[i]) < 4.0) {
                cut = Some(j);
                break;
            }
        }
        if let Some(j) = cut {
            v.drain(i + 1..=j);
        } else {
            i += 1;
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untangle_cuts_loops() {
        // Along x, a loop up and back across, then on.
        let p: Vec<Vec2> = [(0.0, 0.0), (10.0, 0.0), (20.0, 10.0), (15.0, 15.0), (12.0, -5.0), (30.0, -5.0)].iter().map(|&(x, y)| Vec2::new(x, y)).collect();
        let u = untangle(&p);
        assert_eq!(u, vec![p[0], p[1], p[4], p[5]]);
        // A hairpin that doesn't cross stays.
        let h: Vec<Vec2> = [(0.0, 0.0), (10.0, 0.0), (15.0, 5.0), (10.0, 10.0), (0.0, 10.0)].iter().map(|&(x, y)| Vec2::new(x, y)).collect();
        assert_eq!(untangle(&h), h);
    }
}
