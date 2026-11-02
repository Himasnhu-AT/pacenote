//! Stages: 20 fixed seeds, and everything generated from one (road, terrain shape, corners).
//! Pure Rust (no Bevy systems): deterministic, unit-tested, the same on driver and guide.
//!
//! - **Road**: a sequence of features (straights, corners with clothoid-like entries and
//!   exits, the odd hairpin) grown from the start, kept inside the map and away from itself,
//!   sampled every [`STEP`] metres. Its height follows the land, smoothed and grade-limited.
//! - **Terrain**: base hills for the biome, then shaped to the road: flat across the road
//!   (with banking), a flat shoulder each side, then gentle fills and banks blended back into
//!   the hills. The road follows the lower ground, so it mostly runs in a shallow cutting.
//! - **Corners**: runs of curvature graded 1 (hairpin) … 6 (flat-out kink), with crests.

use bevy::math::{Vec2, Vec3};
use pcg_core::{mix64, unit};
use serde::{Deserialize, Serialize};

/// Map half-size (metres) for a stage: longer stages get more room (3.2 km square at 3 km,
/// ~4 km at 6 km).
pub fn half_size(def: &StageDef) -> f32 {
    if def.id == crate::open::ID {
        return crate::open::roads::HALF;
    }
    (1000.0 + def.length * 0.17).clamp(1200.0, 2100.0)
}
/// Metres between road stations.
pub const STEP: f32 = 2.0;
/// Terrain sample spacing (metres).
pub const SPACING: f32 = 2.0;
/// Keep the road this far inside the map edge.
const MARGIN: f32 = 260.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Biome {
    /// Dense conifers, rolling hills.
    PineForest,
    /// Birch and meadows, gentle.
    BirchHills,
    /// Dry scrub, rocky, twisty.
    Scrub,
    /// Open moor, few trees, big crests.
    Moor,
    /// Snow under pines.
    SnowForest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, bevy::reflect::Reflect)]
pub enum Surface {
    Gravel,
    Tarmac,
    Mud,
    Snow,
    /// Off the road: grass/dirt verge.
    Verge,
    /// Off the road further out: rough ground.
    Rough,
}

impl Surface {
    /// Peak tyre friction coefficient and how loose it is (0 = sealed, 1 = very loose:
    /// grip falls off slowly past the peak, extra rolling drag).
    pub fn grip(self) -> (f32, f32) {
        match self {
            Surface::Tarmac => (1.05, 0.05),
            Surface::Gravel => (0.78, 0.55),
            Surface::Mud => (0.55, 0.8),
            Surface::Snow => (0.42, 0.6),
            Surface::Verge => (0.6, 0.75),
            Surface::Rough => (0.5, 0.9),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, bevy::reflect::Reflect)]
pub enum TimeOfDay {
    Morning,
    Noon,
    Afternoon,
    Dusk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Weather {
    Clear,
    Overcast,
    Fog,
    Rain,
}

/// A published stage: never change one once released (leaderboards depend on it); add new ids.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StageDef {
    pub id: u32,
    pub name: String,
    pub seed: u64,
    pub biome: Biome,
    /// Target length (metres).
    pub length: f32,
    pub surface: Surface,
    /// Share of the road that is the secondary surface (mud patches on gravel, gravel cuts on
    /// tarmac…), 0..1.
    pub patches: f32,
    pub time: TimeOfDay,
    pub weather: Weather,
    /// 0..1: more and tighter corners.
    pub twist: f32,
}

/// The 20 stages.
pub fn stages() -> Vec<StageDef> {
    use Biome::*;
    use Surface::*;
    use TimeOfDay::*;
    use Weather::*;
    let s = |id: u32, name: &str, seed: u64, biome, length, surface, patches, time, weather, twist| StageDef {
        id,
        name: name.into(),
        seed,
        biome,
        length,
        surface,
        patches,
        time,
        weather,
        twist,
    };
    vec![
        s(1, "Pinewater", 0x51A6_0001, PineForest, 3200.0, Gravel, 0.1, Morning, Clear, 0.45),
        s(2, "Kettle Ridge", 0x51A6_0002, PineForest, 4100.0, Gravel, 0.15, Afternoon, Overcast, 0.55),
        s(3, "Birchmoor", 0x51A6_0003, BirchHills, 2800.0, Gravel, 0.05, Noon, Clear, 0.35),
        s(4, "Low Fen", 0x51A6_0004, BirchHills, 3600.0, Mud, 0.3, Morning, Fog, 0.5),
        s(5, "Sunscar", 0x51A6_0005, Scrub, 3000.0, Gravel, 0.1, Noon, Clear, 0.7),
        s(6, "Thornback", 0x51A6_0006, Scrub, 4400.0, Tarmac, 0.2, Afternoon, Clear, 0.65),
        s(7, "High Cairn", 0x51A6_0007, Moor, 5200.0, Gravel, 0.1, Dusk, Overcast, 0.4),
        s(8, "Grey Hollow", 0x51A6_0008, Moor, 3800.0, Mud, 0.4, Morning, Rain, 0.45),
        s(9, "Frostline", 0x51A6_0009, SnowForest, 3400.0, Snow, 0.1, Noon, Overcast, 0.5),
        s(10, "White Spruce", 0x51A6_000A, SnowForest, 4600.0, Snow, 0.2, Afternoon, Clear, 0.55),
        s(11, "Long Drop", 0x51A6_000B, PineForest, 5800.0, Gravel, 0.1, Afternoon, Clear, 0.5),
        s(12, "Saw Mill", 0x51A6_000C, PineForest, 2600.0, Tarmac, 0.25, Dusk, Clear, 0.6),
        s(13, "Meadow Run", 0x51A6_000D, BirchHills, 4800.0, Gravel, 0.15, Afternoon, Overcast, 0.3),
        s(14, "Bramble", 0x51A6_000E, BirchHills, 3300.0, Tarmac, 0.3, Morning, Rain, 0.55),
        s(15, "Dust Devil", 0x51A6_000F, Scrub, 5000.0, Gravel, 0.05, Dusk, Clear, 0.6),
        s(16, "Red Rocks", 0x51A6_0010, Scrub, 2700.0, Gravel, 0.2, Noon, Clear, 0.8),
        s(17, "Moorgate", 0x51A6_0011, Moor, 4200.0, Gravel, 0.3, Noon, Fog, 0.45),
        s(18, "Tor Crossing", 0x51A6_0012, Moor, 6000.0, Tarmac, 0.15, Morning, Overcast, 0.4),
        s(19, "Iceback", 0x51A6_0013, SnowForest, 2900.0, Snow, 0.3, Dusk, Fog, 0.65),
        s(20, "Silent Pines", 0x51A6_0014, SnowForest, 5400.0, Snow, 0.15, Morning, Clear, 0.5),
    ]
}

/// The angle of `v` (as glam's `to_angle`), with libm's atan2: std's sin / cos / atan2 / powf
/// differ between platforms (Apple arm64, x86_64, Windows), and a stage must grow the same
/// road on every machine, or boards and ghosts split by CPU.
fn angle_of(v: Vec2) -> f32 {
    libm::atan2f(v.y, v.x)
}

/// Deterministic random numbers from a seed.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(mix64(seed ^ 0x9E37_79B9_7F4A_7C15))
    }
    pub fn f(&mut self) -> f32 {
        self.0 = mix64(self.0.wrapping_add(0x9E37_79B9_7F4A_7C15));
        unit(self.0)
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f()
    }
    pub fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }
}

// ---------------------------------------------------------------- terrain noise

fn lattice(x: i32, z: i32, seed: u64) -> f32 {
    unit(mix64(seed ^ mix64(((x as u32 as u64) << 32) | z as u32 as u64))) * 2.0 - 1.0
}

/// Smooth value noise in [-1, 1].
pub fn noise(p: Vec2, seed: u64) -> f32 {
    let i = p.floor();
    let f = p - i;
    let s = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let (x, z) = (i.x as i32, i.y as i32);
    let a = lattice(x, z, seed);
    let b = lattice(x + 1, z, seed);
    let c = lattice(x, z + 1, seed);
    let d = lattice(x + 1, z + 1, seed);
    let top = a + (b - a) * s.x;
    let bot = c + (d - c) * s.x;
    top + (bot - top) * s.y
}

pub(crate) fn fbm(p: Vec2, octaves: u32, seed: u64) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += noise(p * freq, seed ^ (o as u64).wrapping_mul(0x632B_E59B)) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / norm
}

/// Hills before the road shapes them.
pub fn base_height(x: f32, z: f32, def: &StageDef) -> f32 {
    let p = Vec2::new(x, z);
    let (amp, scale, ridge) = match def.biome {
        Biome::PineForest => (70.0, 1.0 / 700.0, 0.3),
        Biome::BirchHills => (40.0, 1.0 / 900.0, 0.1),
        Biome::Scrub => (90.0, 1.0 / 520.0, 0.5),
        Biome::Moor => (110.0, 1.0 / 1100.0, 0.25),
        Biome::SnowForest => (85.0, 1.0 / 650.0, 0.35),
    };
    let seed = def.seed ^ 0x7E44;
    let warp = Vec2::new(fbm(p * scale * 0.5, 3, seed ^ 1), fbm(p * scale * 0.5, 3, seed ^ 2)) * 180.0;
    let q = (p + warp) * scale;
    let h = fbm(q, 6, seed);
    let r = 1.0 - fbm(q * 0.7, 4, seed ^ 3).abs();
    let detail = fbm(p / 40.0, 3, seed ^ 4) * 1.2;
    (h * (1.0 - ridge) + (r * r - 0.4) * ridge) * amp + detail
}

// ---------------------------------------------------------------- road

/// The road: stations every [`STEP`] m from start to finish.
#[derive(Clone, Debug)]
pub struct Road {
    /// Centreline (y = road height).
    pub pts: Vec<Vec3>,
    /// Unit direction (XZ) at each station.
    pub dir: Vec<Vec2>,
    /// Signed curvature (1/m, + = left turn) at each station.
    pub curv: Vec<f32>,
    /// Full width (m).
    pub width: Vec<f32>,
    /// Crossfall: height change per metre to the left (banking into corners).
    pub bank: Vec<f32>,
    pub surface: Vec<Surface>,
    grid: SegGrid,
}

/// Where a point is relative to the road.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadPos {
    /// Station index (segment start) and how far along the segment (0..1).
    pub i: usize,
    pub t: f32,
    /// Metres from the start along the road.
    pub s: f32,
    /// Signed lateral offset (+ = left of travel).
    pub lateral: f32,
}

impl Road {
    /// A road from its centreline (XZ, one point every [`STEP`] m), heights, widths and
    /// surfaces (one per point): direction, curvature, banking and the lookup grid derived.
    /// Used by the open world (`crate::open`) for every road in its network.
    pub fn from_centreline(pts2: &[Vec2], y: &[f32], width: Vec<f32>, surface: Vec<Surface>) -> Road {
        let n = pts2.len();
        assert!(n >= 2 && y.len() == n && width.len() == n && surface.len() == n, "road arrays must match");
        let pts: Vec<Vec3> = pts2.iter().zip(y).map(|(p, h)| Vec3::new(p.x, *h, p.y)).collect();
        let dir: Vec<Vec2> = (0..n)
            .map(|i| {
                let (a, b) = (pts2[i.saturating_sub(1)], pts2[(i + 1).min(n - 1)]);
                (b - a).normalize_or(Vec2::Y)
            })
            .collect();
        let raw: Vec<f32> = (0..n)
            .map(|i| {
                if i == 0 || i == n - 1 {
                    return 0.0;
                }
                let (a, b) = (pts2[i] - pts2[i - 1], pts2[i + 1] - pts2[i]);
                let len = 0.5 * (a.length() + b.length());
                if len < 1e-4 { 0.0 } else { libm::atan2f(a.perp_dot(b), a.dot(b)) / len }
            })
            .collect();
        let curv = smooth(&raw, 3);
        let bank: Vec<f32> = curv.iter().map(|k| (-k * 9.0).clamp(-0.07, 0.07)).collect();
        let grid = SegGrid::build(&pts, 32.0);
        Road { pts, dir, curv, width, bank, surface, grid }
    }

    pub fn len(&self) -> usize {
        self.pts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pts.is_empty()
    }

    /// Total length (m).
    pub fn length(&self) -> f32 {
        (self.pts.len().saturating_sub(1)) as f32 * STEP
    }

    /// The nearest point of the road to `p` (XZ) within `max` metres.
    pub fn nearest(&self, p: Vec2, max: f32) -> Option<RoadPos> {
        let mut best: Option<(f32, RoadPos)> = None;
        self.grid.visit(p, max, |i| {
            let (a, b) = (xz(self.pts[i]), xz(self.pts[i + 1]));
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            let q = a + ab * t;
            let d2 = q.distance_squared(p);
            if best.is_none_or(|(bd, _)| d2 < bd) {
                // Left of travel = up × direction = (dz, -dx) in XZ.
                let side = (p - a).dot(Vec2::new(ab.y, -ab.x)).signum();
                best = Some((d2, RoadPos { i, t, s: (i as f32 + t) * STEP, lateral: side * d2.sqrt() }));
            }
        });
        best.filter(|(d2, _)| *d2 <= max * max).map(|(_, r)| r)
    }

    /// Road surface height at a road position (banking applied).
    pub fn height_at(&self, r: &RoadPos) -> f32 {
        let j = (r.i + 1).min(self.pts.len() - 1);
        let y = self.pts[r.i].y + (self.pts[j].y - self.pts[r.i].y) * r.t;
        let bank = self.bank[r.i] + (self.bank[j] - self.bank[r.i]) * r.t;
        let half = self.half_width(r);
        y + bank * r.lateral.clamp(-half, half)
    }

    pub fn half_width(&self, r: &RoadPos) -> f32 {
        let j = (r.i + 1).min(self.pts.len() - 1);
        (self.width[r.i] + (self.width[j] - self.width[r.i]) * r.t) * 0.5
    }

    /// Station index at a distance along the road.
    pub fn station(&self, s: f32) -> usize {
        ((s / STEP).round().max(0.0) as usize).min(self.pts.len().saturating_sub(1))
    }

    /// Start position and heading (radians, about Y, 0 = +Z).
    pub fn start(&self) -> (Vec3, f32) {
        let d = self.dir[0];
        (self.pts[0], libm::atan2f(d.x, d.y))
    }
}

fn xz(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

/// Segment index buckets on a 32 m grid.
#[derive(Clone, Debug, Default)]
struct SegGrid {
    cell: f32,
    cells: std::collections::HashMap<(i32, i32), Vec<usize>>,
}

impl SegGrid {
    fn build(pts: &[Vec3], cell: f32) -> SegGrid {
        let mut g = SegGrid { cell, cells: Default::default() };
        for i in 0..pts.len().saturating_sub(1) {
            let (a, b) = (xz(pts[i]), xz(pts[i + 1]));
            let (lo, hi) = (a.min(b), a.max(b));
            for cz in (lo.y / cell).floor() as i32..=(hi.y / cell).floor() as i32 {
                for cx in (lo.x / cell).floor() as i32..=(hi.x / cell).floor() as i32 {
                    g.cells.entry((cx, cz)).or_default().push(i);
                }
            }
        }
        g
    }

    fn visit(&self, p: Vec2, r: f32, mut f: impl FnMut(usize)) {
        let (x0, x1) = (((p.x - r) / self.cell).floor() as i32, ((p.x + r) / self.cell).floor() as i32);
        let (z0, z1) = (((p.y - r) / self.cell).floor() as i32, ((p.y + r) / self.cell).floor() as i32);
        for cz in z0..=z1 {
            for cx in x0..=x1 {
                if let Some(v) = self.cells.get(&(cx, cz)) {
                    for &i in v {
                        f(i);
                    }
                }
            }
        }
    }
}

/// Grows the road for a stage. If a layout boxes itself in before the target length, the
/// whole road is regrown from a derived seed (deterministic), keeping the longest.
pub fn generate_road(def: &StageDef) -> Road {
    let mut best: Option<Road> = None;
    for attempt in 0..8u64 {
        let r = grow_road(def, if attempt == 0 { def.seed } else { mix64(def.seed ^ attempt) });
        let done = r.length() >= def.length - STEP * 2.0;
        if best.as_ref().is_none_or(|b| r.length() > b.length()) {
            best = Some(r);
        }
        if done {
            break;
        }
    }
    best.unwrap()
}

fn grow_road(def: &StageDef, seed: u64) -> Road {
    let mut rng = Rng::new(seed);
    let n = (def.length / STEP).ceil() as usize + 1;
    // Start near a map edge, heading inwards.
    let a = rng.range(0.0, std::f32::consts::TAU);
    let half = half_size(def);
    let start = Vec2::new(libm::cosf(a), libm::sinf(a)) * (half - MARGIN - 80.0);
    let mut heading = angle_of(-start) + rng.range(-0.5, 0.5);
    let mut pos = start;
    let mut pts2 = vec![pos];
    let mut curv = vec![0.0f32];
    let mut heads = vec![heading];
    let mut backtracks = 0;
    let mut occupied = SegGrid { cell: 40.0, cells: Default::default() };
    let mut k = 0.0f32;
    // A loose route: waypoints the road wanders towards (it would box itself in otherwise).
    let mut waypoints = Vec::new();
    let mut wp = start;
    for _ in 0..12 {
        let mut best = (f32::NEG_INFINITY, wp);
        for _ in 0..16 {
            let c = Vec2::new(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)) * (half - MARGIN - 150.0);
            let d = c.distance(wp);
            // 500-900 m on, far from earlier waypoints.
            let away = waypoints.iter().map(|w: &Vec2| w.distance(c)).fold(f32::INFINITY, f32::min).min(start.distance(c));
            let score = -((d - 700.0).abs()) + away * 0.6;
            if score > best.0 {
                best = (score, c);
            }
        }
        wp = best.1;
        waypoints.push(wp);
    }
    let mut target = 0usize;
    while pts2.len() < n {
        if pos.distance(waypoints[target]) < 160.0 && target + 1 < waypoints.len() {
            target += 1;
        }
        let toward = angle_diff(angle_of(waypoints[target] - pos), heading);
        // Plan the next feature (a straight or a corner); try a few variants and take the
        // first that stays inside the map and clear of the road so far.
        let mut chosen: Option<Vec<(Vec2, f32, f32)>> = None;
        let mut best_partial: Vec<(Vec2, f32, f32)> = Vec::new();
        for attempt in 0..8 {
            let corner = attempt > 0 || rng.chance(0.55 + def.twist * 0.35);
            let (target_k, len) = if corner {
                let tight = libm::powf(rng.f(), 1.5 - def.twist);
                let radius = 170.0 - 156.0 * tight;
                let angle = rng.range(15.0, 80.0 + 100.0 * tight).to_radians();
                // Mostly towards the next waypoint (the more off-course, the surer). A target at
                // a larger angle is to the right (see `trace`).
                let p_left = 0.5 - (toward / std::f32::consts::FRAC_PI_2).clamp(-1.0, 1.0) * 0.4;
                let mut left = rng.chance(p_left);
                if attempt == 0 && pos.length() > half - MARGIN - 220.0 {
                    left = angle_diff(angle_of(-pos), heading) < 0.0;
                }
                let sign = if left { 1.0 } else { -1.0 };
                (sign / radius, angle * radius)
            } else {
                (0.0, rng.range(25.0, 160.0 - 110.0 * def.twist))
            };
            let trial = trace(pos, heading, k, target_k, len, &pts2, &occupied, half);
            if trial.len() as f32 * STEP >= len - STEP {
                chosen = Some(trial);
                break;
            }
            if trial.len() > best_partial.len() {
                best_partial = trial;
            }
        }
        let steps = match chosen {
            Some(t) => t,
            None if best_partial.len() >= 6 => best_partial,
            None => {
                // Boxed in: a tight turn towards open space (the centre).
                // Larger angle = right turn = negative curvature.
                let toward = -angle_diff(angle_of(-pos), heading).signum();
                let t = trace(pos, heading, k, toward / 18.0, 60.0, &pts2, &occupied, half);
                if t.is_empty() {
                    // Backtrack: drop the last 120 m and grow again with other choices.
                    if backtracks >= 300 || pts2.len() < 70 {
                        break;
                    }
                    backtracks += 1;
                    let keep = pts2.len() - 60;
                    pts2.truncate(keep);
                    curv.truncate(keep);
                    heads.truncate(keep);
                    pos = pts2[keep - 1];
                    heading = heads[keep - 1];
                    k = curv[keep - 1];
                    occupied = SegGrid { cell: 40.0, cells: Default::default() };
                    for (i, p) in pts2.iter().enumerate() {
                        occupied.cells.entry(((p.x / 40.0).floor() as i32, (p.y / 40.0).floor() as i32)).or_default().push(i);
                    }
                    continue;
                }
                t
            }
        };
        for (p, kk, h) in steps {
            pos = p;
            k = kk;
            heading = h;
            push(&mut pts2, &mut curv, &mut occupied, pos, k);
            heads.push(h);
            if pts2.len() >= n {
                break;
            }
        }
    }
    pts2.truncate(n);
    curv.truncate(n);
    let n = pts2.len();
    let _ = n;
    // Height: follow the lower ground either side (so the road runs in a shallow cutting
    // rather than on an embankment), sunk a little, then smoothed and grade-limited.
    let raw: Vec<f32> = (0..pts2.len())
        .map(|i| {
            let p = pts2[i];
            let d = (pts2[(i + 1).min(pts2.len() - 1)] - pts2[i.saturating_sub(1)]).normalize_or(Vec2::Y);
            let n = Vec2::new(d.y, -d.x);
            let side = |o: f32| base_height(p.x + n.x * o, p.y + n.y * o, def);
            base_height(p.x, p.y, def).min(side(10.0)).min(side(-10.0)) - ROAD_SINK
        })
        .collect();
    let mut y = smooth(&raw, 40);
    let max_grade = 0.11 * STEP;
    for i in 1..y.len() {
        y[i] = y[i].clamp(y[i - 1] - max_grade, y[i - 1] + max_grade);
    }
    for i in (0..y.len() - 1).rev() {
        y[i] = y[i].clamp(y[i + 1] - max_grade, y[i + 1] + max_grade);
    }
    let y = smooth(&y, 6);
    let pts: Vec<Vec3> = pts2.iter().zip(&y).map(|(p, h)| Vec3::new(p.x, *h, p.y)).collect();
    let dir: Vec<Vec2> = (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i.saturating_sub(1)], pts[(i + 1).min(pts.len() - 1)]);
            (xz(b) - xz(a)).normalize_or(Vec2::Y)
        })
        .collect();
    let curv = smooth(&curv, 3);
    // Width and surface: base width per surface, narrows here and there; patches by noise.
    let base_w = match def.surface {
        Surface::Tarmac => 7.2,
        Surface::Snow => 6.0,
        _ => 6.4,
    };
    let width: Vec<f32> = (0..pts.len()).map(|i| base_w - 1.2 * {
        let w = noise(Vec2::splat(i as f32 * STEP / 160.0), def.seed ^ 11) * 0.5 + 0.5;
        w * w * w
    }).collect();
    let second = match def.surface {
        Surface::Gravel => Surface::Mud,
        Surface::Tarmac => Surface::Gravel,
        Surface::Mud => Surface::Gravel,
        Surface::Snow => Surface::Gravel,
        s => s,
    };
    let surface: Vec<Surface> = (0..pts.len())
        .map(|i| if noise(Vec2::splat(i as f32 * STEP / 90.0 + 3.7), def.seed ^ 12) * 0.5 + 0.5 > 1.0 - def.patches { second } else { def.surface })
        .collect();
    // Banked into the corner: a left-hander is lower on the left (lateral +).
    let bank: Vec<f32> = curv.iter().map(|k| (-k * 9.0).clamp(-0.07, 0.07)).collect();
    let grid = SegGrid::build(&pts, 32.0);
    Road { pts, dir, curv, width, bank, surface, grid }
}

/// Steps along a feature from `pos`: curvature eases towards `target_k` then back to zero.
/// Stops before leaving the map or coming within 45 m of older road.
#[allow(clippy::too_many_arguments)]
fn trace(mut p: Vec2, mut h: f32, mut kk: f32, target_k: f32, len: f32, pts: &[Vec2], occ: &SegGrid, half: f32) -> Vec<(Vec2, f32, f32)> {
    let ease = (len * 0.3).clamp(8.0, 40.0);
    let steps = (len / STEP).ceil() as usize;
    let mut out = Vec::with_capacity(steps);
    for s in 0..steps {
        let goal = if (s as f32) * STEP > len - ease { 0.0 } else { target_k };
        kk += (goal - kk) * (STEP / ease).min(1.0);
        // +k turns left: seen from above (+Y), heading angles grow from +X towards +Z, which
        // is a right turn, so left curvature decreases the angle.
        h -= kk * STEP;
        p += Vec2::new(libm::cosf(h), libm::sinf(h)) * STEP;
        if p.abs().max_element() > half - MARGIN {
            break;
        }
        let mut near = false;
        let recent = pts.len() + out.len();
        occ.visit(p, 45.0, |j| {
            if j + 60 < recent && pts.get(j).is_some_and(|q| xz_dist2(*q, p) < 45.0 * 45.0) {
                near = true;
            }
        });
        if near {
            break;
        }
        out.push((p, kk, h));
    }
    out
}

fn push(pts: &mut Vec<Vec2>, curv: &mut Vec<f32>, occ: &mut SegGrid, p: Vec2, k: f32) {
    let i = pts.len();
    pts.push(p);
    curv.push(k);
    let c = ((p.x / occ.cell).floor() as i32, (p.y / occ.cell).floor() as i32);
    occ.cells.entry(c).or_default().push(i);
}

fn xz_dist2(a: Vec2, b: Vec2) -> f32 {
    a.distance_squared(b)
}

fn angle_diff(a: f32, b: f32) -> f32 {
    let mut d = a - b;
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    while d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}

pub(crate) fn smooth(v: &[f32], r: usize) -> Vec<f32> {
    let n = v.len();
    let mut pre = vec![0.0f64; n + 1];
    for i in 0..n {
        pre[i + 1] = pre[i] + v[i] as f64;
    }
    (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(r), (i + r + 1).min(n));
            ((pre[b] - pre[a]) / (b - a) as f64) as f32
        })
        .collect()
}

/// Terrain height at (x, z): hills, shaped to the road.
///
/// The road sits in the land, never on a lifted ribbon: a flat shoulder level with the road
/// (rising a touch, so the edge reads as a lip), then the ground may fall away no faster than
/// [`FILL_GRADE`] (a car that runs wide can drive back up) and rise no faster than
/// [`CUT_GRADE`] (a bank), blending into the natural hills [`BLEND`] out.
pub fn height(x: f32, z: f32, def: &StageDef, road: &Road) -> f32 {
    let base = base_height(x, z, def);
    match road.nearest(Vec2::new(x, z), BLEND.1) {
        Some(r) => shaped(base, road, &r),
        None => base,
    }
}

/// How far from a road (m) the ground is shaped by it ([`shaped`]); beyond it the land is
/// its own.
pub const SHAPE_REACH: f32 = BLEND.1;

/// The ground `base` m high at `r` (a point beside `road`), shaped by the road: see [`height`].
pub fn shaped(base: f32, road: &Road, r: &RoadPos) -> f32 {
    let half = road.half_width(r);
    let d = r.lateral.abs();
    let road_y = road.height_at(r);
    let o = d - half - 0.6;
    if o <= 0.0 {
        return road_y;
    }
    let lip = road_y + SHOULDER_RISE * smoothstep((o / SHOULDER).min(1.0));
    if o <= SHOULDER {
        return lip;
    }
    let out = o - SHOULDER;
    let shaped = base.clamp(lip - FILL_GRADE * out, lip + CUT_GRADE * out);
    let t = smoothstep(((d - BLEND.0) / (BLEND.1 - BLEND.0)).clamp(0.0, 1.0));
    shaped + (base - shaped) * t
}

/// How far below the lower ground beside it the road is laid (m).
const ROAD_SINK: f32 = 0.6;
/// Flat-ish shoulder beyond the road edge (m), rising [`SHOULDER_RISE`] m across it.
const SHOULDER: f32 = 3.0;
const SHOULDER_RISE: f32 = 0.15;
/// Steepest fall (fill) / rise (cut) of the ground beyond the shoulder (rise per metre).
const FILL_GRADE: f32 = 0.16;
const CUT_GRADE: f32 = 0.45;
/// Lateral distances (m) between which the shaped ground blends back into the hills.
const BLEND: (f32, f32) = (34.0, 52.0);

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// What the tyres are on at (x, z).
pub fn surface_at(x: f32, z: f32, road: &Road) -> Surface {
    match road.nearest(Vec2::new(x, z), 12.0) {
        Some(r) if r.lateral.abs() <= road.half_width(&r) => road.surface[r.i],
        Some(r) if r.lateral.abs() <= road.half_width(&r) + 3.0 => Surface::Verge,
        _ => Surface::Rough,
    }
}

// ---------------------------------------------------------------- corners

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Turn {
    Left,
    Right,
}

/// One pace-note entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Corner {
    /// Metres from the start where it begins / ends.
    pub start: f32,
    pub end: f32,
    pub turn: Turn,
    /// 1 = hairpin … 6 = flat-out kink.
    pub grade: u8,
    /// Total direction change (degrees).
    pub angle: f32,
    /// The road goes over a crest in it (or just before).
    pub crest: bool,
    /// Narrower than usual.
    pub narrows: bool,
}

/// Corners along the road, graded.
pub fn corners(road: &Road) -> Vec<Corner> {
    let mut out = Vec::new();
    let mut i = 0;
    let n = road.len();
    while i < n {
        let k = road.curv[i];
        if k.abs() < 1.0 / 400.0 {
            i += 1;
            continue;
        }
        let sign = k.signum();
        let start = i;
        let mut peak = 0.0f32;
        let mut angle = 0.0;
        while i < n && road.curv[i].signum() == sign && road.curv[i].abs() >= 1.0 / 600.0 {
            peak = peak.max(road.curv[i].abs());
            angle += road.curv[i].abs() * STEP;
            i += 1;
        }
        let angle = angle.to_degrees();
        if angle < 8.0 {
            continue;
        }
        let radius = 1.0 / peak.max(1e-4);
        let mut grade = match radius {
            r if r < 22.0 => 1,
            r if r < 40.0 => 2,
            r if r < 65.0 => 3,
            r if r < 100.0 => 4,
            r if r < 160.0 => 5,
            _ => 6,
        };
        if angle > 150.0 && grade <= 2 {
            grade = 1;
        }
        let (s0, s1) = (start as f32 * STEP, i as f32 * STEP);
        let crest = crest_between(road, start.saturating_sub(15), i.min(n - 1));
        let narrows = road.width[start..i.min(n)].iter().any(|w| *w < road.width[0] - 0.8);
        out.push(Corner { start: s0, end: s1, turn: if sign > 0.0 { Turn::Left } else { Turn::Right }, grade, angle, crest, narrows });
    }
    out
}

/// A crest: the road rises then falls sharply (vertical curvature) somewhere in `a..b`.
fn crest_between(road: &Road, a: usize, b: usize) -> bool {
    (a.max(5)..b.min(road.len().saturating_sub(5))).any(|i| {
        let g0 = (road.pts[i].y - road.pts[i - 5].y) / (5.0 * STEP);
        let g1 = (road.pts[i + 5].y - road.pts[i].y) / (5.0 * STEP);
        g0 - g1 > 0.06
    })
}

/// Crests on straights too (the guide calls them).
pub fn crests(road: &Road) -> Vec<f32> {
    let mut out = Vec::new();
    let mut last = -1e9;
    for i in 10..road.len().saturating_sub(10) {
        let g0 = (road.pts[i].y - road.pts[i - 10].y) / (10.0 * STEP);
        let g1 = (road.pts[i + 10].y - road.pts[i].y) / (10.0 * STEP);
        let s = i as f32 * STEP;
        if g0 - g1 > 0.07 && s - last > 60.0 {
            out.push(s);
            last = s;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_stages_unique_seeds() {
        let s = stages();
        assert_eq!(s.len(), 20);
        let mut seeds: Vec<u64> = s.iter().map(|d| d.seed).collect();
        seeds.sort();
        seeds.dedup();
        assert_eq!(seeds.len(), 20);
    }

    #[test]
    fn roads_are_deterministic_long_enough_and_inside() {
        for def in stages() {
            let a = generate_road(&def);
            let b = generate_road(&def);
            assert_eq!(a.pts, b.pts, "{}", def.name);
            assert!((a.length() - def.length).abs() < STEP * 2.0, "{} length {}", def.name, a.length());
            let half = half_size(&def);
            assert!(a.pts.iter().all(|p| p.x.abs() < half - 100.0 && p.z.abs() < half - 100.0), "{} leaves the map", def.name);
            // Grade limit holds.
            for w in a.pts.windows(2) {
                assert!((w[1].y - w[0].y).abs() / STEP < 0.13, "{} too steep", def.name);
            }
        }
    }

    #[test]
    fn road_does_not_run_into_itself() {
        for def in stages() {
            let r = generate_road(&def);
            for i in (0..r.len()).step_by(5) {
                for j in (i + 80..r.len()).step_by(5) {
                    let d = xz(r.pts[i]).distance(xz(r.pts[j]));
                    assert!(d > 25.0, "{}: stations {i} and {j} only {d:.1} m apart", def.name);
                }
            }
        }
    }

    #[test]
    fn terrain_is_flat_across_the_road() {
        let def = &stages()[0];
        let road = generate_road(def);
        for i in (10..road.len() - 10).step_by(50) {
            let p = road.pts[i];
            let h = height(p.x, p.z, def, &road);
            assert!((h - p.y).abs() < 0.5, "station {i}: terrain {h} road {}", p.y);
            assert_eq!(surface_at(p.x, p.z, &road), road.surface[i]);
        }
    }

    #[test]
    fn running_wide_is_recoverable() {
        // No lifted ribbon and no ditch: within 20 m of the edge the ground is never further
        // below the road than a gentle FILL_GRADE slope from the shoulder (the natural hills
        // may still fall away beyond a bench, but never straight off the road edge).
        for def in &stages() {
            let road = generate_road(def);
            for i in (10..road.len() - 10).step_by(7) {
                let p = road.pts[i];
                let n = Vec2::new(road.dir[i].y, -road.dir[i].x);
                let r = road.nearest(Vec2::new(p.x, p.z), 5.0).unwrap();
                let (edge, road_y) = (road.half_width(&r) + 0.6, road.height_at(&r));
                for side in [-1.0f32, 1.0] {
                    for k in 0..=20 {
                        let o = k as f32;
                        let q = Vec2::new(p.x, p.z) + n * side * (edge + o);
                        // Stop where another stretch of road is nearer (hairpins).
                        if road.nearest(q, 60.0).is_some_and(|rr| rr.i.abs_diff(i) > 20) {
                            break;
                        }
                        let floor = road_y - FILL_GRADE * (o - SHOULDER).max(0.0) - 0.4;
                        let h = height(q.x, q.y, def, &road);
                        assert!(h >= floor, "stage {} station {i}: {:.2} m below the road {o} m off the edge", def.id, road_y - h);
                    }
                }
            }
        }
    }

    #[test]
    fn left_means_left() {
        // A left-hander: the road ahead lies to the left of the start direction (up × dir).
        let def = &stages()[0];
        let r = generate_road(def);
        for c in corners(&r).iter().filter(|c| c.angle > 40.0) {
            let (i0, i1) = (r.station(c.start), r.station(c.end));
            let d0 = r.dir[i0];
            let left = Vec3::new(d0.y, 0.0, -d0.x);
            let d1 = r.dir[i1];
            let turned_left = Vec3::new(d1.x, 0.0, d1.y).dot(left) > 0.0;
            assert_eq!(turned_left, c.turn == Turn::Left, "corner at {} m", c.start);
            // A point on the left of the road reads positive lateral.
            let p = r.pts[i0] + left * 2.0;
            assert!(r.nearest(Vec2::new(p.x, p.z), 10.0).unwrap().lateral > 0.0);
        }
    }

    #[test]
    fn corners_are_graded() {
        let def = &stages()[4];
        let road = generate_road(def);
        let c = corners(&road);
        assert!(c.len() > 10, "{} corners", c.len());
        assert!(c.iter().all(|c| (1..=6).contains(&c.grade) && c.end > c.start));
        assert!(c.iter().any(|c| c.grade <= 3));
    }
}

#[cfg(test)]
mod debug {
    use super::*;

    #[test]
    #[ignore]
    fn print_relief() {
        // How far the road sits above (+) / below (-) the natural hills, 10 m and 25 m out.
        for def in stages() {
            let r = generate_road(&def);
            let mut v10 = Vec::new();
            let mut v25 = Vec::new();
            for i in (0..r.len()).step_by(5) {
                let p = r.pts[i];
                let n = Vec2::new(r.dir[i].y, -r.dir[i].x);
                for side in [-1.0f32, 1.0] {
                    let q = Vec2::new(p.x, p.z) + n * side * 10.0;
                    v10.push(p.y - base_height(q.x, q.y, &def));
                    let q = Vec2::new(p.x, p.z) + n * side * 25.0;
                    v25.push(p.y - base_height(q.x, q.y, &def));
                }
            }
            let pct = |v: &mut Vec<f32>, f: f32| { v.sort_by(|a, b| a.total_cmp(b)); v[((v.len() - 1) as f32 * f) as usize] };
            println!("{:>2} 10m: p5 {:>6.1} p50 {:>5.1} p95 {:>5.1} max {:>5.1} | 25m: p5 {:>6.1} p95 {:>5.1} max {:>5.1}", def.id,
                pct(&mut v10, 0.05), pct(&mut v10, 0.5), pct(&mut v10, 0.95), pct(&mut v10, 1.0), pct(&mut v25, 0.05), pct(&mut v25, 0.95), pct(&mut v25, 1.0));
        }
    }

    #[test]
    #[ignore]
    fn print_stats() {
        for def in stages() {
            let r = generate_road(&def);
            let c = corners(&r);
            let tight = c.iter().filter(|c| c.grade <= 3).count();
            let maxk = r.curv.iter().fold(0.0f32, |a, k| a.max(k.abs()));
            println!("{:>2} {:<14} len {:>6.0} corners {:>3} (tight {:>2}) min radius {:>5.1}", def.id, def.name, r.length(), c.len(), tight, 1.0 / maxk);
        }
    }
}
