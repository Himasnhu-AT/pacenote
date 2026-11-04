//! The land of the open world before any road: terrain first, roads solve it.
//!
//! Four zones (see docs/RALLY_MAP.md): a river valley across the south (80-120 m) with the
//! village, a fast gravel basin in the east (140-240 m), a forestry escarpment in the west
//! (180-380 m) and an alpine plateau in the north (380-480 m), with a stream gully between the
//! escarpment and the basin. Macro shape + domain-warped ridge noise, then droplet hydraulic
//! erosion on a 4 m grid (gullies, spurs, fans) and a moisture map from where the water ran.
//!
//! Pure and deterministic (libm, fixed order): the same seed gives the same bits everywhere.
//! Built once per seed and shared ([`land`]); about a second.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use bevy::math::Vec2;

use crate::stage::{Rng, fbm, noise};

/// Half the side of the region (m).
pub const HALF: f32 = 2000.0;
/// Grid spacing of the eroded land (m).
pub const CELL: f32 = 4.0;
/// Grid points per side.
pub const N: usize = (2.0 * HALF / CELL) as usize + 1;

/// What part of the map a point is in (by position and height).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Zone {
    Valley,
    Basin,
    Escarpment,
    Plateau,
}

/// The eroded land.
pub struct Land {
    pub seed: u64,
    /// Heights (m), row-major, z rows from -HALF, x columns from -HALF.
    h: Vec<f32>,
    /// Moisture 0..1: how much water ran through (smoothed), for ecology.
    wet: Vec<f32>,
    /// Crag mask 0..1 per grid point ([`Land::crag`]).
    crag: Vec<f32>,
    /// The river centreline (west to east), points every ~8 m.
    pub river: Vec<Vec2>,
}

impl std::fmt::Debug for Land {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Land(seed {:#x})", self.seed)
    }
}

impl Land {
    fn idx(x: usize, z: usize) -> usize {
        z * N + x
    }

    fn bilinear(v: &[f32], p: Vec2) -> f32 {
        let fx = ((p.x + HALF) / CELL).clamp(0.0, (N - 1) as f32 - 1e-3);
        let fz = ((p.y + HALF) / CELL).clamp(0.0, (N - 1) as f32 - 1e-3);
        let (x, z) = (fx as usize, fz as usize);
        let (tx, tz) = (fx - x as f32, fz - z as f32);
        let a = v[Self::idx(x, z)] + (v[Self::idx(x + 1, z)] - v[Self::idx(x, z)]) * tx;
        let b = v[Self::idx(x, z + 1)] + (v[Self::idx(x + 1, z + 1)] - v[Self::idx(x, z + 1)]) * tx;
        a + (b - a) * tz
    }

    /// Catmull-Rom (bicubic) sampling of the grid: smooth slopes between the 4 m points, so the
    /// 2 m terrain has no crease lines along the grid (WORLD_PASS T9).
    fn bicubic(v: &[f32], p: Vec2) -> f32 {
        let fx = ((p.x + HALF) / CELL).clamp(0.0, (N - 1) as f32 - 1e-3);
        let fz = ((p.y + HALF) / CELL).clamp(0.0, (N - 1) as f32 - 1e-3);
        let (x, z) = (fx as usize, fz as usize);
        let (tx, tz) = (fx - x as f32, fz - z as f32);
        let at = |i: isize, j: isize| {
            let xi = (x as isize + i).clamp(0, N as isize - 1) as usize;
            let zj = (z as isize + j).clamp(0, N as isize - 1) as usize;
            v[Self::idx(xi, zj)]
        };
        let cr = |a: f32, b: f32, c: f32, d: f32, t: f32| b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)));
        let row = |j: isize| cr(at(-1, j), at(0, j), at(1, j), at(2, j), tx);
        cr(row(-1), row(0), row(1), row(2), tz)
    }

    /// Ground height (m) at `p`, before roads: the eroded grid plus fine detail.
    pub fn height(&self, p: Vec2) -> f32 {
        // (Less of it in town: the lots are level.)
        let detail = 0.6 * (1.0 - 0.8 * town_mask(p));
        Self::bicubic(&self.h, p) + fbm(p / 40.0, 3, self.seed ^ 0x4D) * detail
    }

    /// Ground height without the fine detail (for road planning).
    pub fn coarse(&self, p: Vec2) -> f32 {
        Self::bicubic(&self.h, p)
    }

    /// Moisture 0..1 at `p`.
    pub fn wet(&self, p: Vec2) -> f32 {
        Self::bilinear(&self.wet, p)
    }

    /// Slope (rise per metre) at `p`.
    pub fn slope(&self, p: Vec2) -> f32 {
        let e = CELL;
        let dx = self.coarse(p + Vec2::X * e) - self.coarse(p - Vec2::X * e);
        let dz = self.coarse(p + Vec2::Y * e) - self.coarse(p - Vec2::Y * e);
        Vec2::new(dx, dz).length() / (2.0 * e)
    }

    /// Distance (m) from `p` to the river.
    pub fn river_distance(&self, p: Vec2) -> f32 {
        distance_to(&self.river, p)
    }

    pub fn zone(&self, p: Vec2) -> Zone {
        zone_of(p, self.coarse(p))
    }

    /// Crag mask 0..1: where cliffs are meant to be (patches of the plateau rim, the quarry
    /// face). Outside it the land is held to a talus slope ([`TALUS`]).
    pub fn crag(&self, p: Vec2) -> f32 {
        Self::bilinear(&self.crag, p)
    }

    /// Town mask 0..1: Fernhollow's built-up core (flattened, no trees).
    pub fn town(&self, p: Vec2) -> f32 {
        town_mask(p)
    }
}

/// Fernhollow's core (x, z bounds, m): north of the B-road on the river's north bank. The
/// B-road runs through it as Main Street ([`MAIN_Z`]); the forestry road is its spine
/// ([`SPINE_X`]).
pub const TOWN_LO: Vec2 = Vec2::new(-720.0, -1175.0);
pub const TOWN_HI: Vec2 = Vec2::new(80.0, -840.0);
/// Main Street's line (z) and the spine's (x).
pub const MAIN_Z: f32 = -1140.0;
pub const SPINE_X: f32 = -290.0;
/// The town's centre (for the zoning rings).
pub const TOWN_C: Vec2 = Vec2::new(-320.0, -1020.0);

/// 1 inside the core's bounds, falling to 0 over 80 m outside them.
fn town_mask(p: Vec2) -> f32 {
    let dx = (TOWN_LO.x - p.x).max(p.x - TOWN_HI.x);
    let dz = (TOWN_LO.y - p.y).max(p.y - TOWN_HI.y);
    1.0 - smoothstep(0.0, 80.0, dx.max(dz))
}

/// The town's floor height (m): Main Street's level.
const TOWN_FLOOR: f32 = 94.0;

/// Steepest slope (rise per metre) the land keeps outside the crags, and inside them.
pub const TALUS: f32 = 0.75;
pub const CRAG_TALUS: f32 = 1.1;

fn zone_of(p: Vec2, h: f32) -> Zone {
    if h > 370.0 {
        Zone::Plateau
    } else if h < 135.0 && p.y < -500.0 {
        Zone::Valley
    } else if p.x > gully_x(p.y) {
        Zone::Basin
    } else {
        Zone::Escarpment
    }
}

/// The shared land for `seed` (built on first use).
pub fn land(seed: u64) -> Arc<Land> {
    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<Land>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(l) = cache.lock().unwrap().get(&seed) {
        return l.clone();
    }
    let l = Arc::new(build(seed));
    cache.lock().unwrap().entry(seed).or_insert(l).clone()
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Half smoothstep, half linear: a climb that keeps an even grade through its middle (more of
/// the map at mid heights than a smoothstep's steep middle gives).
fn ramp(e0: f32, e1: f32, x: f32) -> f32 {
    0.5 * (smoothstep(e0, e1, x) + ((x - e0) / (e1 - e0)).clamp(0.0, 1.0))
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn distance_to(line: &[Vec2], p: Vec2) -> f32 {
    let mut best = f32::MAX;
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
        best = best.min((a + ab * t).distance_squared(p));
    }
    best.sqrt()
}

/// x of the stream gully between escarpment (west) and basin (east), by z.
fn gully_x(z: f32) -> f32 {
    120.0 + 0.18 * z
}

/// The river: west edge to east edge across the south, meandering.
fn river_line(seed: u64) -> Vec<Vec2> {
    let n = 500;
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let x = -HALF - 40.0 + t * (2.0 * HALF + 80.0);
            let z = -1250.0 - 250.0 * t + noise(Vec2::new(t * 3.2, 0.7), seed ^ 0x61) * 170.0 + noise(Vec2::new(t * 9.0, 1.9), seed ^ 0x62) * 45.0;
            Vec2::new(x, z)
        })
        .collect()
}

/// The zones' base height (no ridges, valley or gully) and where across the gully `p` is
/// (0 west .. 1 east). Zone edges are warped so nothing runs straight; the warp is gentle
/// enough (260 m at 1.8 km) that the ramps keep their width (T4).
fn zone_base(p: Vec2, seed: u64) -> (f32, f32) {
    let s = seed ^ 0x1A4D;
    let zw = p + Vec2::new(fbm(p / 1800.0, 3, s ^ 5), fbm(p / 1800.0, 3, s ^ 6)) * 260.0;
    let (x, z) = (zw.x, zw.y);
    // West: the escarpment climbs steadily from the valley to the plateau.
    let west = lerp(115.0, 380.0, ramp(-1100.0, 800.0, z)) + 70.0 * smoothstep(700.0, 1600.0, z);
    // East: up to a rolling basin, then a wall (≤ ~33 %) to the plateau.
    let east = lerp(115.0, 185.0, smoothstep(-1150.0, -450.0, z)) + 245.0 * ramp(150.0, 1350.0, z) + 20.0 * smoothstep(1100.0, 1800.0, z);
    let gx = gully_x(p.y) + fbm(Vec2::new(p.y / 500.0, 3.3), 3, s ^ 7) * 160.0;
    let tw = smoothstep(gx - 420.0, gx + 420.0, x);
    (lerp(west, east, tw), tw)
}

/// The land before erosion: zones, valley, gully and ridges. `crag` (0..1) keeps the ridges
/// knife-edged (cliffs); elsewhere they are rounded (T2).
fn macro_height(p: Vec2, seed: u64, river: &[Vec2], crag: f32) -> f32 {
    let s = seed ^ 0x1A4D;
    let (base, tw) = zone_base(p, seed);
    let mut h = base;

    // Ridged multifractal, domain-warped: spurs and ridgelines, crests rounded (radius ~40 m)
    // outside the crags.
    let warp = Vec2::new(fbm(p / 800.0, 3, s ^ 1), fbm(p / 800.0, 3, s ^ 2)) * 300.0;
    let q = (p + warp) / 950.0;
    let (mut ridged, mut amp, mut freq, mut weight, mut sum) = (0.0, 1.0, 1.0, 1.0f32, 0.0);
    for o in 0..5 {
        let n = noise(q * freq, s ^ (10 + o));
        let a = lerp(libm::sqrtf(n * n + 0.015), n.abs(), crag);
        let r = (1.0 - a).max(0.0);
        let r = r * r * weight;
        weight = (r * 2.0).clamp(0.0, 0.9);
        ridged += r * amp;
        sum += amp;
        amp *= 0.5;
        freq *= 2.1;
    }
    ridged = ridged / sum * 1.6 - 0.75;
    let rolling = fbm(p / 650.0, 4, s ^ 3);
    // One amplitude, continuous in height (T1): the valley's (12 m, 20 m far from the town),
    // the escarpment's and basin's, the plateau's.
    let valley_amp = lerp(12.0, 20.0, smoothstep(700.0, 1100.0, p.distance(TOWN_C)));
    let amp_low = lerp(lerp(110.0, 50.0, tw), valley_amp, 1.0 - smoothstep(110.0, 190.0, h));
    let amp_high = lerp(70.0, 45.0, smoothstep(360.0, 450.0, h));
    let amp = lerp(amp_low, amp_high, smoothstep(320.0, 400.0, h));
    h += ridged * amp + rolling * amp * 0.4;

    // The stream gully between escarpment and basin, cut below the plateau.
    let gx = gully_x(p.y) + fbm(Vec2::new(p.y / 500.0, 3.3), 3, s ^ 7) * 160.0;
    let dg = (p.x - gx).abs();
    let gully_depth = 85.0 * smoothstep(-1100.0, -300.0, p.y) * (1.0 - smoothstep(800.0, 1300.0, p.y));
    h -= gully_depth * libm::powf(1.0 - smoothstep(0.0, 300.0, dg), 1.5);

    // The river valley: a broad floor sloping gently east, two river terraces (T8), sides rising.
    let dr = distance_to(river, p);
    let wob = noise(p / 260.0, s ^ 9);
    // (None through the town: its lots are level.)
    let terraces = (1.0 - town_mask(p)) * 3.0 * smoothstep(-5.0, 5.0, dr - (200.0 + 20.0 * wob)) + (1.0 - town_mask(p)) * 3.0 * smoothstep(-5.0, 5.0, dr - (415.0 + 35.0 * wob));
    let floor = 98.0 - (p.x + HALF) / (2.0 * HALF) * 14.0 + noise(p / 300.0, s ^ 4) * 2.0 + terraces;
    let t = smoothstep(80.0, 700.0, dr + fbm(p / 400.0, 2, s ^ 8) * 120.0);
    h = floor + (h.max(floor) - floor) * t;
    // South of the river: low wooded hills to the edge.
    if p.y < -1300.0 {
        let south = floor + 30.0 * smoothstep(-1300.0, -2000.0, p.y) + ridged.max(-0.2) * 45.0 * smoothstep(250.0, 700.0, dr);
        let ts = smoothstep(150.0, 600.0, dr) * smoothstep(-1450.0, -1900.0, p.y);
        h = lerp(h, south.max(floor), ts);
    }
    h
}

/// Droplet hydraulic erosion (the classic particle model): each drop runs downhill with
/// inertia, picking up sediment where it speeds up and dropping it where it slows. Heights in
/// m on the CELL grid. Returns how much water ran through each cell.
fn erode(h: &mut [f32], drops: usize, seed: u64) -> Vec<f32> {
    const INERTIA: f32 = 0.06;
    const CAPACITY: f32 = 6.0;
    const MIN_CAPACITY: f32 = 0.01;
    const DEPOSIT: f32 = 0.25;
    const ERODE: f32 = 0.35;
    const EVAPORATE: f32 = 0.015;
    const GRAVITY: f32 = 9.0;
    const LIFETIME: usize = 90;
    const RADIUS: i32 = 3;
    // Heights are in m but the model is tuned for unit cells: scale to cells.
    let scale = 1.0 / CELL;
    for v in h.iter_mut() {
        *v *= scale;
    }
    // Erosion brush: weights within RADIUS cells; deposits spread over a radius-2 brush (no
    // spikes or pits from four-corner deposits).
    let brush_of = |radius: i32| {
        let mut brush: Vec<(i32, i32, f32)> = Vec::new();
        let mut wsum = 0.0;
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let d = libm::sqrtf((dx * dx + dz * dz) as f32);
                if d <= radius as f32 {
                    let w = 1.0 - d / (radius as f32 + 0.5);
                    brush.push((dx, dz, w));
                    wsum += w;
                }
            }
        }
        for b in &mut brush {
            b.2 /= wsum;
        }
        brush
    };
    let brush = brush_of(RADIUS);
    let drop_brush = brush_of(2);
    let n = N as i32;
    let mut flow = vec![0.0f32; h.len()];
    let grad = |h: &[f32], px: f32, pz: f32| -> (f32, f32, f32) {
        let (x, z) = (px as usize, pz as usize);
        let (u, v) = (px - x as f32, pz - z as f32);
        let i = z * N + x;
        let (a, b, c, d) = (h[i], h[i + 1], h[i + N], h[i + N + 1]);
        let gx = (b - a) * (1.0 - v) + (d - c) * v;
        let gz = (c - a) * (1.0 - u) + (d - b) * u;
        let height = a * (1.0 - u) * (1.0 - v) + b * u * (1.0 - v) + c * (1.0 - u) * v + d * u * v;
        (gx, gz, height)
    };
    let mut rng = Rng::new(seed ^ 0xE20D);
    for _ in 0..drops {
        let (mut px, mut pz) = (rng.range(2.0, (N - 3) as f32), rng.range(2.0, (N - 3) as f32));
        let (mut dx, mut dz) = (0.0f32, 0.0f32);
        let (mut speed, mut water, mut sed) = (1.0f32, 1.0f32, 0.0f32);
        for _ in 0..LIFETIME {
            let (x, z) = (px as usize, pz as usize);
            let (gx, gz, h0) = grad(h, px, pz);
            dx = dx * INERTIA - gx * (1.0 - INERTIA);
            dz = dz * INERTIA - gz * (1.0 - INERTIA);
            let len = libm::sqrtf(dx * dx + dz * dz);
            if len < 1e-6 {
                break;
            }
            dx /= len;
            dz /= len;
            px += dx;
            pz += dz;
            if px < 1.0 || pz < 1.0 || px >= (n - 2) as f32 || pz >= (n - 2) as f32 {
                break;
            }
            flow[z * N + x] += water;
            let (_, _, h1) = grad(h, px, pz);
            let dh = h1 - h0;
            let cap = (-dh * speed * water * CAPACITY).max(MIN_CAPACITY);
            if sed > cap || dh > 0.0 {
                let amt = if dh > 0.0 { dh.min(sed) } else { (sed - cap) * DEPOSIT };
                sed -= amt;
                for &(bx, bz, w) in &drop_brush {
                    let (cx, cz) = (x as i32 + bx, z as i32 + bz);
                    if cx < 0 || cz < 0 || cx >= n || cz >= n {
                        continue;
                    }
                    h[cz as usize * N + cx as usize] += amt * w;
                }
            } else {
                let amt = ((cap - sed) * ERODE).min(-dh);
                for &(bx, bz, w) in &brush {
                    let (cx, cz) = (x as i32 + bx, z as i32 + bz);
                    if cx < 0 || cz < 0 || cx >= n || cz >= n {
                        continue;
                    }
                    let j = cz as usize * N + cx as usize;
                    let take = (amt * w).min(h[j].max(0.0));
                    h[j] -= take;
                    sed += take;
                }
            }
            speed = libm::sqrtf(speed * speed + dh.abs() * GRAVITY).min(6.0);
            water *= 1.0 - EVAPORATE;
        }
    }
    for v in h.iter_mut() {
        *v /= scale;
    }
    flow
}

/// Box blur (separable, `r` cells), in place.
fn blur(v: &mut [f32], r: usize) {
    let mut tmp = vec![0.0f32; v.len()];
    for pass in 0..2 {
        for a in 0..N {
            let mut acc = 0.0;
            let at = |b: usize| if pass == 0 { a * N + b } else { b * N + a };
            for b in 0..=r.min(N - 1) {
                acc += v[at(b)];
            }
            for b in 0..N {
                let lo = b.saturating_sub(r);
                let hi = (b + r).min(N - 1);
                tmp[at(b)] = acc / (hi - lo + 1) as f32;
                if b + r + 1 < N {
                    acc += v[at(b + r + 1)];
                }
                if b >= r {
                    acc -= v[at(b - r)];
                }
            }
        }
        v.copy_from_slice(&tmp);
    }
}

/// Gaussian blur (separable, `sigma` cells, radius `r` cells), edges clamped.
fn gauss(v: &[f32], sigma: f32, r: usize) -> Vec<f32> {
    let w: Vec<f32> = (0..=r).map(|k| libm::expf(-((k * k) as f32) / (2.0 * sigma * sigma))).collect();
    let norm = w[0] + 2.0 * w[1..].iter().sum::<f32>();
    let w: Vec<f32> = w.iter().map(|x| x / norm).collect();
    let mut tmp = vec![0.0f32; v.len()];
    let mut out = vec![0.0f32; v.len()];
    let ri = r as isize;
    for z in 0..N {
        for x in 0..N {
            let mut acc = 0.0;
            for k in -ri..=ri {
                let xx = (x as isize + k).clamp(0, N as isize - 1) as usize;
                acc += v[z * N + xx] * w[k.unsigned_abs()];
            }
            tmp[z * N + x] = acc;
        }
    }
    for z in 0..N {
        for x in 0..N {
            let mut acc = 0.0;
            for k in -ri..=ri {
                let zz = (z as isize + k).clamp(0, N as isize - 1) as usize;
                acc += tmp[zz * N + x] * w[k.unsigned_abs()];
            }
            out[z * N + x] = acc;
        }
    }
    out
}

/// Runs `f(row index, row)` over the rows of a `w`-wide grid on all cores. Each row is
/// computed on its own, so the result doesn't depend on the threads.
pub(crate) fn par_rows(v: &mut [f32], w: usize, f: impl Fn(usize, &mut [f32]) + Sync) {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
    let rows = v.len() / w;
    let per = rows.div_ceil(threads).max(1);
    std::thread::scope(|sc| {
        for (c, chunk) in v.chunks_mut(per * w).enumerate() {
            let f = &f;
            sc.spawn(move || {
                for (k, row) in chunk.chunks_mut(w).enumerate() {
                    f(c * per + k, row);
                }
            });
        }
    });
}

/// Thermal weathering (T7, T11): wherever a cell stands above a neighbour (8 of them) by
/// more than the talus slope allows, the two meet half way at the talus slope. Gauss-Seidel
/// sweeps in a fixed order (alternating directions), so the excess runs down a whole slope in
/// one sweep and the result is the same on every machine. `talus` per cell (rise per metre),
/// `cell` the spacing (m); `frozen` cells keep their height (the partner moves the whole way).
pub(crate) fn thermal(h: &mut [f32], nx: usize, nz: usize, cell: f32, talus: &dyn Fn(usize) -> f32, frozen: &dyn Fn(usize) -> bool, sweeps: usize) {
    let diag = cell * std::f32::consts::SQRT_2;
    for k in 0..sweeps {
        // Sweep order: +x/+z, -x/-z, +x/-z, -x/+z.
        let (sx, sz): (isize, isize) = [(1, 1), (-1, -1), (1, -1), (-1, 1)][k % 4];
        // The neighbours ahead in this sweep.
        let ahead: [(isize, isize, f32); 4] = [(sx, 0, cell), (0, sz, cell), (sx, sz, diag), (-sx, sz, diag)];
        for zi in 0..nz {
            let z = if sz > 0 { zi } else { nz - 1 - zi };
            for xi in 0..nx {
                let x = if sx > 0 { xi } else { nx - 1 - xi };
                let i = z * nx + x;
                for &(dx, dz, len) in &ahead {
                    let (xx, zz) = (x as isize + dx, z as isize + dz);
                    if xx < 0 || zz < 0 || xx >= nx as isize || zz >= nz as isize {
                        continue;
                    }
                    let j = zz as usize * nx + xx as usize;
                    let dh = h[i] - h[j];
                    let excess = dh.abs() - talus(i).min(talus(j)) * len;
                    if excess <= 0.0 {
                        continue;
                    }
                    let (hi, lo) = if dh > 0.0 { (i, j) } else { (j, i) };
                    match (frozen(hi), frozen(lo)) {
                        (false, false) => {
                            h[hi] -= 0.5 * excess;
                            h[lo] += 0.5 * excess;
                        }
                        (false, true) => h[hi] -= excess,
                        (true, false) => h[lo] += excess,
                        (true, true) => {}
                    }
                }
            }
        }
    }
}

fn build(seed: u64) -> Land {
    let river = river_line(seed);
    let pos = |x: usize, z: usize| Vec2::new(-HALF + x as f32 * CELL, -HALF + z as f32 * CELL);
    // The crag mask (T3) from the zones' base: the plateau rim where it climbs steeply, in
    // patches (a few cliff bands, not a wall).
    let mut base = vec![0.0f32; N * N];
    par_rows(&mut base, N, |z, row| {
        for (x, v) in row.iter_mut().enumerate() {
            *v = zone_base(pos(x, z), seed).0;
        }
    });
    let mut crag = vec![0.0f32; N * N];
    for z in 1..N - 1 {
        for x in 1..N - 1 {
            let i = Land::idx(x, z);
            let gx = (base[i + 1] - base[i - 1]) / (2.0 * CELL);
            let gz = (base[i + N] - base[i - N]) / (2.0 * CELL);
            let slope = libm::sqrtf(gx * gx + gz * gz);
            let b = base[i];
            let band = smoothstep(350.0, 372.0, b) * (1.0 - smoothstep(418.0, 440.0, b));
            let patch = smoothstep(0.25, 0.45, noise(pos(x, z) / 320.0, seed ^ 0xC4A6));
            crag[i] = band * smoothstep(0.17, 0.25, slope) * patch;
        }
    }
    drop(base);
    blur(&mut crag, 3);

    let mut h = vec![0.0f32; N * N];
    par_rows(&mut h, N, |z, row| {
        for (x, v) in row.iter_mut().enumerate() {
            *v = macro_height(pos(x, z), seed, &river, crag[Land::idx(x, z)]);
        }
    });
    // Per-cell creases out before erosion puts structure back (T5).
    let mut h = gauss(&h, 1.5, 5);
    let before = h.clone();
    let mut wet = erode(&mut h, 1_200_000, seed);
    // Erosion digs pits at the grid's edge where drops leave; there the land stays as it was.
    // Elsewhere deep cuts are eased off (no flat-floored trenches): at most ~35 m.
    const EDGE: usize = 12;
    for z in 0..N {
        for x in 0..N {
            let i = Land::idx(x, z);
            let e = x.min(z).min(N - 1 - x).min(N - 1 - z);
            let cut = before[i] - h[i];
            if cut > 0.0 {
                h[i] = before[i] - 35.0 * (1.0 - libm::expf(-cut / 35.0));
            }
            if e < EDGE {
                h[i] = lerp(before[i], h[i], e as f32 / EDGE as f32);
            }
        }
    }
    // Thermal pass: a hard cap on slope outside the crags.
    let talus: Vec<f32> = crag.iter().map(|c| lerp(TALUS, CRAG_TALUS, *c)).collect();
    thermal(&mut h, N, N, CELL, &|i| talus[i], &|_| false, 16);
    // The town: level lots (T8). Most of the way to a broad blur (~80 m), and that blur's own
    // rise toward the hills halved round the town's floor, so streets climb gently.
    let mut flat = h.clone();
    for _ in 0..3 {
        blur(&mut flat, 20);
    }
    for z in 0..N {
        for x in 0..N {
            let i = Land::idx(x, z);
            let t = town_mask(pos(x, z));
            if t > 0.0 {
                let level = TOWN_FLOOR + (flat[i] - TOWN_FLOOR) * 0.5;
                h[i] = lerp(h[i], level, 0.95 * t);
            }
        }
    }
    // (Levelling steepens the ground round the town's edge: back to the talus there.)
    thermal(&mut h, N, N, CELL, &|i| talus[i], &|_| false, 4);
    // The river bed: a shallow channel along the line.
    let mut bed = vec![0.0f32; N * N];
    par_rows(&mut bed, N, |z, row| {
        for (x, v) in row.iter_mut().enumerate() {
            let d = distance_to(&river, pos(x, z));
            *v = 1.0 - smoothstep(6.0, 30.0, d);
        }
    });
    for (i, b) in bed.iter().enumerate() {
        h[i] -= 2.5 * b;
        wet[i] += 400.0 * b;
    }
    // Moisture: log of flow, smoothed, 0..1.
    for v in wet.iter_mut() {
        *v = libm::logf(1.0 + *v);
    }
    blur(&mut wet, 4);
    let max = wet.iter().cloned().fold(0.0f32, f32::max).max(1e-3);
    for v in wet.iter_mut() {
        *v = (*v / max).clamp(0.0, 1.0);
    }
    Land { seed, h, wet, crag, river }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_span_the_target_heights() {
        let l = land(crate::open::SEED);
        let at = |x: f32, z: f32| l.coarse(Vec2::new(x, z));
        let valley = at(0.0, -1350.0);
        assert!((70.0..130.0).contains(&valley), "valley {valley}");
        let plateau = at(-200.0, 1600.0);
        assert!((360.0..500.0).contains(&plateau), "plateau {plateau}");
        let basin = at(900.0, -100.0);
        assert!((130.0..260.0).contains(&basin), "basin {basin}");
        let (lo, hi) = l.h.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
        let at_lo = l.h.iter().position(|v| *v == lo).unwrap();
        assert!(lo > 60.0 && hi - lo > 360.0 && hi - lo < 480.0, "relief {lo}..{hi}, lowest at cell {} {}", at_lo % N, at_lo / N);
    }

    #[test]
    fn deterministic() {
        let a = build(7);
        let b = build(7);
        assert!(a.h == b.h && a.wet == b.wet);
    }

    /// The eroded land keeps to its talus outside the crags (no cliffs or steps from noise),
    /// has its mid heights (the climb isn't squeezed into walls) and a level town.
    #[test]
    fn land_holds_its_talus() {
        let l = land(crate::open::SEED);
        let mut worst = 0.0f32;
        let mut at = (0, 0);
        let mut mid = 0;
        for z in 0..N - 1 {
            for x in 0..N - 1 {
                let i = Land::idx(x, z);
                mid += (180.0..340.0).contains(&l.h[i]) as usize;
                if l.crag[i] > 0.01 || l.crag[i + 1] > 0.01 || l.crag[i + N] > 0.01 {
                    continue;
                }
                let d = (l.h[i + 1] - l.h[i]).abs().max((l.h[i + N] - l.h[i]).abs());
                if d > worst {
                    worst = d;
                    at = (x, z);
                }
            }
        }
        let mid = 100.0 * mid as f32 / (N * N) as f32;
        eprintln!("steepest 4 m step outside crags {worst:.2} m ({:.0} %) at {:?}, {mid:.1} % of the land at 180-340 m", worst / CELL * 100.0, (-HALF + at.0 as f32 * CELL, -HALF + at.1 as f32 * CELL));
        // (The sweeps leave a few pairs a little over.)
        assert!(worst <= TALUS * CELL * 1.08, "a {worst} m step between 4 m neighbours");
        assert!(mid >= 20.0, "{mid} % at 180-340 m");
        // The town's lots: level (90 % of them under 6 %).
        let mut s: Vec<f32> = Vec::new();
        let mut z = TOWN_LO.y;
        while z <= TOWN_HI.y {
            let mut x = TOWN_LO.x;
            while x <= TOWN_HI.x {
                s.push(l.slope(Vec2::new(x, z)));
                x += 8.0;
            }
            z += 8.0;
        }
        s.sort_by(f32::total_cmp);
        let p90 = s[s.len() * 9 / 10];
        assert!(p90 < 0.06, "town slope p90 {p90}");
    }

    /// `cargo test --lib open::land::tests::slope_report -- --ignored --nocapture`: the
    /// WORLD_PASS §7 numbers on the finished 2 m terrain (what `--bake-world` writes), checked
    /// against its targets.
    #[test]
    #[ignore]
    fn slope_report() {
        let o = crate::open::OpenWorld::generate(crate::open::SEED);
        let r = report::measure(&o.heights, &o.net);
        eprintln!("{r}");
        assert!(r.over_100 <= 0.8 && (r.over_100 < 0.05 || r.over_100_in_crag >= 70.0), "> 100 %: {} ({} % in crags)", r.over_100, r.over_100_in_crag);
        assert!(r.over_150_outside_crag <= 0.05);
        assert!(r.steps_3m < 1000);
        assert!(r.worst_band.1 <= 50.0);
        assert!(r.mid_hyps >= 20.0);
        assert!((30.0..=38.0).contains(&r.flat));
        assert!(r.town_p90 <= 6.0);
        assert!(r.steep_near <= r.steep_far);
        assert!(r.road_steps < 100, "{} steps over 1.5 m near roads", r.road_steps);
    }
}

/// The terrain measured the WORLD_PASS way (§1, §7): slope is the central difference over 4 m
/// on the 2 m grid, shares are of the map's area.
#[cfg(test)]
pub(crate) mod report {
    use bevy::math::Vec2;
    use engine::world::Heightfield;

    use crate::open::RoadNet;

    pub const BINS: [f32; 7] = [0.1, 0.2, 0.3, 0.45, 0.7, 1.0, 1.5];

    #[derive(Debug, Default)]
    pub struct Report {
        /// Share (%) of the area per slope bin (< 10 %, 10-20, ... > 150 %).
        pub hist: [f32; 8],
        pub over_100: f32,
        /// Share (%) of the > 100 % area that is inside the crag mask.
        pub over_100_in_crag: f32,
        pub over_150_outside_crag: f32,
        /// Neighbouring 2 m samples more than 3 m apart, outside the crag mask.
        pub steps_3m: usize,
        /// Mean slope (%) of each 20 m height band (from 80 m), and the worst.
        pub bands: Vec<(f32, f32, f32)>,
        pub worst_band: (f32, f32),
        /// Share (%) of the area at 180-340 m.
        pub mid_hyps: f32,
        pub flat: f32,
        /// 90th percentile slope (%) inside the town mask.
        pub town_p90: f32,
        /// ... of the land before the roads, and of the lots (away from roads and the river).
        pub town_land_p90: f32,
        pub town_lots_p90: f32,
        /// Share (%) steeper than 70 % at 30-46 m and at 60-90 m from a road.
        pub steep_near: f32,
        pub steep_far: f32,
        /// 2 m steps over 1.5 m within 50 m of a road.
        pub road_steps: usize,
        /// ... by distance beyond the road's edge: on it, 0-3, 3-12, 12-30, 30+ m.
        pub step_bins: [usize; 5],
        pub step_where: Vec<(Vec2, usize, usize, f32)>,
    }

    impl std::fmt::Display for Report {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            writeln!(f, "slope   <10   10-20 20-30 30-45 45-70 70-100 100-150 >150 (% of area)")?;
            write!(f, "share ")?;
            for h in self.hist {
                write!(f, " {h:5.2}")?;
            }
            writeln!(f)?;
            writeln!(f, "> 100 %: {:.2} % of the area, {:.0} % of it in crags", self.over_100, self.over_100_in_crag)?;
            writeln!(f, "> 150 % outside crags: {:.3} %", self.over_150_outside_crag)?;
            writeln!(f, "2 m neighbours > 3 m apart outside crags: {}", self.steps_3m)?;
            write!(f, "band means:")?;
            for (lo, mean, share) in &self.bands {
                write!(f, " {lo:.0}:{mean:.0}%({share:.1})")?;
            }
            writeln!(f)?;
            writeln!(f, "worst band: {:.0} m at {:.0} % mean slope", self.worst_band.0, self.worst_band.1)?;
            writeln!(f, "area at 180-340 m: {:.1} %", self.mid_hyps)?;
            writeln!(f, "area < 10 %: {:.1} %", self.flat)?;
            writeln!(f, "town mask p90 slope: {:.1} % (land {:.1} %, lots {:.1} %)", self.town_p90, self.town_land_p90, self.town_lots_p90)?;
            writeln!(f, "> 70 % at 30-46 m from a road: {:.2} %, at 60-90 m: {:.2} %", self.steep_near, self.steep_far)?;
            writeln!(f, "2 m steps > 1.5 m within 50 m of a road: {} (on, 0-3, 3-12, 12-30, 30+ m off: {:?})", self.road_steps, self.step_bins)?;
            write!(f, "e.g. {:?}", self.step_where)
        }
    }

    pub fn measure(hf: &Heightfield, net: &RoadNet) -> Report {
        let land = &net.land;
        let (nx, nz) = (hf.nx() as i64, hf.nz() as i64);
        let sp = hf.spacing();
        let at = |x: i64, z: i64| hf.get(x, z);
        let mut r = Report::default();
        let mut count = 0usize;
        let mut hist = [0usize; 8];
        let (mut o100, mut o100c, mut o150) = (0usize, 0usize, 0usize);
        let mut bands = vec![(0.0f64, 0usize); 24];
        let mut mid = 0usize;
        let mut town: Vec<f32> = Vec::new();
        let mut town_land: Vec<f32> = Vec::new();
        let mut town_lots: Vec<f32> = Vec::new();
        let (mut near, mut near_n, mut far, mut far_n) = (0usize, 0usize, 0usize, 0usize);
        for z in 1..nz - 1 {
            for x in 1..nx - 1 {
                let p = hf.sample_pos(x as u32, z as u32);
                let gx = (at(x + 1, z) - at(x - 1, z)) / (2.0 * sp);
                let gz = (at(x, z + 1) - at(x, z - 1)) / (2.0 * sp);
                let s = Vec2::new(gx, gz).length();
                let h = at(x, z);
                count += 1;
                hist[BINS.iter().position(|b| s < *b).unwrap_or(7)] += 1;
                let crag = land.crag(p) > 0.5;
                if s > 1.0 {
                    o100 += 1;
                    o100c += crag as usize;
                }
                if s > 1.5 && !crag {
                    o150 += 1;
                }
                let b = (((h - 80.0) / 20.0).floor().max(0.0) as usize).min(bands.len() - 1);
                bands[b].0 += s as f64;
                bands[b].1 += 1;
                mid += (180.0..340.0).contains(&h) as usize;
                if land.town(p) > 0.999 && (x + z) % 2 == 0 {
                    town.push(s);
                    let e = 4.0;
                    let lg = Vec2::new(land.height(p + Vec2::X * e) - land.height(p - Vec2::X * e), land.height(p + Vec2::Y * e) - land.height(p - Vec2::Y * e)).length() / (2.0 * e);
                    town_land.push(lg);
                    if net.nearest(p, 15.0).is_none() && land.river_distance(p) > 40.0 {
                        town_lots.push(s);
                    }
                }
                // Steps between neighbours (right and up), outside crags.
                if !crag {
                    for (dx, dz) in [(1, 0), (0, 1)] {
                        if x + dx < nx && z + dz < nz && (at(x + dx, z + dz) - h).abs() > 3.0 {
                            r.steps_3m += 1;
                        }
                    }
                }
                // Near the roads (every other cell: the nearest-road search is the cost).
                if x % 2 == 0
                    && z % 2 == 0
                    && let Some(n) = net.nearest(p, 90.0)
                {
                    let d = n.at.lateral.abs();
                    if (30.0..46.0).contains(&d) {
                        near_n += 1;
                        near += (s > 0.7) as usize;
                    } else if (60.0..90.0).contains(&d) {
                        far_n += 1;
                        far += (s > 0.7) as usize;
                    }
                    if d < 50.0 {
                        for (dx, dz) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                            if (at(x + dx, z + dz) - h).abs() > 1.5 {
                                r.road_steps += 1;
                                let o = d - net.roads[n.road].road.half_width(&n.at);
                                let bin = if o < 0.0 { 0 } else if o < 3.0 { 1 } else if o < 12.0 { 2 } else if o < 30.0 { 3 } else { 4 };
                                r.step_bins[bin] += 1;
                                if r.step_where.len() < 12 && bin <= 2 && r.step_bins[bin] % 20 == 1 {
                                    r.step_where.push((p, n.road, n.at.i, o));
                                }
                            }
                        }
                    }
                }
            }
        }
        let pc = |n: usize| 100.0 * n as f32 / count as f32;
        for (k, h) in hist.iter().enumerate() {
            r.hist[k] = pc(*h);
        }
        r.over_100 = pc(o100);
        r.over_100_in_crag = 100.0 * o100c as f32 / o100.max(1) as f32;
        r.over_150_outside_crag = pc(o150);
        for (k, (sum, n)) in bands.iter().enumerate() {
            if *n > count / 1000 {
                let lo = 80.0 + 20.0 * k as f32;
                let mean = 100.0 * (*sum / *n as f64) as f32;
                r.bands.push((lo, mean, pc(*n)));
                if mean > r.worst_band.1 {
                    r.worst_band = (lo, mean);
                }
            }
        }
        r.mid_hyps = pc(mid);
        r.flat = r.hist[0];
        town.sort_by(f32::total_cmp);
        r.town_p90 = town.get(town.len() * 9 / 10).map_or(0.0, |s| 100.0 * s);
        for v in [&mut town_land, &mut town_lots] {
            v.sort_by(f32::total_cmp);
        }
        r.town_land_p90 = town_land.get(town_land.len() * 9 / 10).map_or(0.0, |s| 100.0 * s);
        r.town_lots_p90 = town_lots.get(town_lots.len() * 9 / 10).map_or(0.0, |s| 100.0 * s);
        r.steep_near = 100.0 * near as f32 / near_n.max(1) as f32;
        r.steep_far = 100.0 * far as f32 / far_n.max(1) as f32;
        r
    }
}

#[cfg(test)]
mod picture {
    /// `PACENOTE_LAND_PPM=out.ppm cargo test --lib open::land::picture -- --ignored`: hillshade
    /// with height tint and water.
    #[test]
    #[ignore]
    fn draw() {
        use bevy::math::Vec2;
        let Ok(out) = std::env::var("PACENOTE_LAND_PPM") else { return };
        let l = super::land(crate::open::SEED);
        let px = 1000;
        let mut img = format!("P6 {px} {px} 255\n").into_bytes();
        for row in 0..px {
            for col in 0..px {
                // North (+z) up.
                let p = Vec2::new(-2000.0 + (col as f32 + 0.5) * 4.0, 2000.0 - (row as f32 + 0.5) * 4.0);
                let e = 4.0;
                let dx = l.height(p + Vec2::X * e) - l.height(p - Vec2::X * e);
                let dz = l.height(p + Vec2::Y * e) - l.height(p - Vec2::Y * e);
                let n = bevy::math::Vec3::new(-dx, 2.0 * e, -dz).normalize();
                let shade = n.dot(bevy::math::Vec3::new(-0.5, 0.7, 0.5).normalize()).max(0.0);
                let h = l.height(p);
                let t = ((h - 80.0) / 400.0).clamp(0.0, 1.0);
                let mut c = [0.35 + 0.45 * t, 0.5 + 0.2 * t, 0.3 + 0.5 * t];
                let w = l.wet(p);
                if w > 0.55 {
                    let k = ((w - 0.55) / 0.3).min(1.0);
                    c = [c[0] * (1.0 - k) + 0.2 * k, c[1] * (1.0 - k) + 0.35 * k, c[2] * (1.0 - k) + 0.7 * k];
                }
                for ch in c {
                    img.push(((ch * (0.35 + 0.75 * shade)).clamp(0.0, 1.0) * 255.0) as u8);
                }
            }
        }
        std::fs::write(out, img).unwrap();
    }
}
