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

    /// Ground height (m) at `p`, before roads: the eroded grid plus fine detail.
    pub fn height(&self, p: Vec2) -> f32 {
        Self::bilinear(&self.h, p) + fbm(p / 40.0, 3, self.seed ^ 0x4D) * 0.6
    }

    /// Ground height without the fine detail (for road planning).
    pub fn coarse(&self, p: Vec2) -> f32 {
        Self::bilinear(&self.h, p)
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
}

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

/// The land before erosion: zones, valley, gully and ridges.
fn macro_height(p: Vec2, seed: u64, river: &[Vec2]) -> f32 {
    let s = seed ^ 0x1A4D;
    // Zone edges are warped so nothing runs straight.
    let zw = p + Vec2::new(fbm(p / 1400.0, 3, s ^ 5), fbm(p / 1400.0, 3, s ^ 6)) * 420.0;
    let (x, z) = (zw.x, zw.y);
    // West: the escarpment climbs steadily from the valley to the plateau.
    let west = lerp(115.0, 380.0, smoothstep(-1100.0, 800.0, z)) + 70.0 * smoothstep(700.0, 1600.0, z);
    // East: up to a rolling basin, flat-ish, then a steep wall to the plateau.
    let east = lerp(115.0, 185.0, smoothstep(-1150.0, -450.0, z)) + 245.0 * smoothstep(300.0, 1200.0, z) + 20.0 * smoothstep(1100.0, 1800.0, z);
    let gx = gully_x(p.y) + fbm(Vec2::new(p.y / 500.0, 3.3), 3, s ^ 7) * 160.0;
    let tw = smoothstep(gx - 420.0, gx + 420.0, x);
    let mut h = lerp(west, east, tw);

    // Ridged multifractal, domain-warped: spurs and sharp ridgelines.
    let warp = Vec2::new(fbm(p / 800.0, 3, s ^ 1), fbm(p / 800.0, 3, s ^ 2)) * 300.0;
    let q = (p + warp) / 950.0;
    let (mut ridged, mut amp, mut freq, mut weight, mut sum) = (0.0, 1.0, 1.0, 1.0f32, 0.0);
    for o in 0..6 {
        let r = (1.0 - noise(q * freq, s ^ (10 + o)).abs()).max(0.0);
        let r = r * r * weight;
        weight = (r * 2.0).clamp(0.0, 1.0);
        ridged += r * amp;
        sum += amp;
        amp *= 0.5;
        freq *= 2.1;
    }
    ridged = ridged / sum * 1.6 - 0.75;
    let rolling = fbm(p / 650.0, 4, s ^ 3);
    let amp = if h > 360.0 {
        lerp(70.0, 45.0, smoothstep(360.0, 450.0, h))
    } else {
        lerp(lerp(140.0, 50.0, tw), 12.0, 1.0 - smoothstep(110.0, 190.0, h))
    };
    h += ridged * amp + rolling * amp * 0.4;

    // The stream gully between escarpment and basin, cut below the plateau.
    let dg = (p.x - gx).abs();
    let gully_depth = 85.0 * smoothstep(-1100.0, -300.0, p.y) * (1.0 - smoothstep(800.0, 1300.0, p.y));
    h -= gully_depth * (1.0 - smoothstep(0.0, 300.0, dg)).powf(1.5);

    // The river valley: a broad floor sloping gently east, sides rising.
    let dr = distance_to(river, p);
    let floor = 98.0 - (p.x + HALF) / (2.0 * HALF) * 14.0 + noise(p / 300.0, s ^ 4) * 2.0;
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
    const LIFETIME: usize = 70;
    const RADIUS: i32 = 3;
    // Heights are in m but the model is tuned for unit cells: scale to cells.
    let scale = 1.0 / CELL;
    for v in h.iter_mut() {
        *v *= scale;
    }
    // Erosion brush: weights within RADIUS cells.
    let mut brush: Vec<(i32, i32, f32)> = Vec::new();
    let mut wsum = 0.0;
    for dz in -RADIUS..=RADIUS {
        for dx in -RADIUS..=RADIUS {
            let d = ((dx * dx + dz * dz) as f32).sqrt();
            if d <= RADIUS as f32 {
                let w = 1.0 - d / RADIUS as f32;
                brush.push((dx, dz, w));
                wsum += w;
            }
        }
    }
    for b in &mut brush {
        b.2 /= wsum;
    }
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
            let (u, v) = (px - x as f32, pz - z as f32);
            let (gx, gz, h0) = grad(h, px, pz);
            dx = dx * INERTIA - gx * (1.0 - INERTIA);
            dz = dz * INERTIA - gz * (1.0 - INERTIA);
            let len = (dx * dx + dz * dz).sqrt();
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
                // Deposit on the four corners of the old cell.
                let amt = if dh > 0.0 { dh.min(sed) } else { (sed - cap) * DEPOSIT };
                sed -= amt;
                let i = z * N + x;
                h[i] += amt * (1.0 - u) * (1.0 - v);
                h[i + 1] += amt * u * (1.0 - v);
                h[i + N] += amt * (1.0 - u) * v;
                h[i + N + 1] += amt * u * v;
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
            speed = (speed * speed + dh.abs() * GRAVITY).sqrt().min(6.0);
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

fn build(seed: u64) -> Land {
    let river = river_line(seed);
    let mut h = vec![0.0f32; N * N];
    for z in 0..N {
        for x in 0..N {
            let p = Vec2::new(-HALF + x as f32 * CELL, -HALF + z as f32 * CELL);
            h[Land::idx(x, z)] = macro_height(p, seed, &river);
        }
    }
    let before = h.clone();
    let mut wet = erode(&mut h, 600_000, seed);
    // Erosion digs pits at the grid's edge where drops leave; there the land stays as it was,
    // and nowhere does it cut more than 30 m.
    const EDGE: usize = 12;
    for z in 0..N {
        for x in 0..N {
            let i = Land::idx(x, z);
            let e = x.min(z).min(N - 1 - x).min(N - 1 - z);
            h[i] = h[i].max(before[i] - 30.0);
            if e < EDGE {
                h[i] = lerp(before[i], h[i], e as f32 / EDGE as f32);
            }
        }
    }
    // The river bed: a shallow channel along the line.
    for z in 0..N {
        for x in 0..N {
            let p = Vec2::new(-HALF + x as f32 * CELL, -HALF + z as f32 * CELL);
            let d = distance_to(&river, p);
            if d < 30.0 {
                let i = Land::idx(x, z);
                let bed = 1.0 - smoothstep(6.0, 30.0, d);
                h[i] -= 2.5 * bed;
                wet[i] += 400.0 * bed;
            }
        }
    }
    // Moisture: log of flow, smoothed, 0..1.
    for v in wet.iter_mut() {
        *v = (1.0 + *v).ln();
    }
    blur(&mut wet, 4);
    let max = wet.iter().cloned().fold(0.0f32, f32::max).max(1e-3);
    for v in wet.iter_mut() {
        *v = (*v / max).clamp(0.0, 1.0);
    }
    Land { seed, h, wet, river }
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
