//! Paints the region's map into an RGBA image once per world (on a worker thread): height
//! tints with hill shading and 50 m contours, the river, buildings, and every road coloured by
//! kind over a dark casing. −z is up (see [`uv`], which maps a world point to the image).
//!
//! Pure: it only reads the inputs it is given, so tests paint small images.

use bevy::math::{Vec2, Vec3};

use crate::open::RoadKind;
use crate::open::buildings::Building;
use crate::open::roads::{HALF, RoadNet};

/// Side of the map image (px): ~1.95 m per pixel over the 4 km region.
pub const SIZE: u32 = 2048;

/// A world point (x, z) → map image coordinates 0..1: u along +x, v along +z. Seen from
/// above with +x to the right, +z is down (the world is right-handed, y up), so the map's top
/// is −z, which the map calls north. This keeps left turns left on the map (the guide map uses
/// the same view).
pub fn uv(p: Vec2) -> Vec2 {
    (p + Vec2::splat(HALF)) / (2.0 * HALF)
}

/// The inverse of [`uv`].
pub fn world(uv: Vec2) -> Vec2 {
    uv * 2.0 * HALF - Vec2::splat(HALF)
}

/// Road fill colour by kind (sRGB 0..1). Matches the legend on the full map.
pub fn road_color(kind: RoadKind) -> [f32; 3] {
    match kind {
        RoadKind::BRoad => [0.98, 0.80, 0.24],
        RoadKind::Street => [0.93, 0.91, 0.86],
        RoadKind::Gravel => [0.88, 0.60, 0.34],
        RoadKind::Forest => [0.74, 0.56, 0.40],
    }
}

/// What the map is painted from.
pub struct Source<'a> {
    pub net: &'a RoadNet,
    /// Ground height (m) at (x, z).
    pub height: &'a (dyn Fn(f32, f32) -> f32 + Sync),
    pub buildings: &'a [Building],
    /// The river's centreline (x, z).
    pub river: &'a [Vec2],
}

struct Canvas {
    n: u32,
    m_per_px: f32,
    px: Vec<[f32; 3]>,
}

impl Canvas {
    fn to_px(&self, p: Vec2) -> Vec2 {
        uv(p) * self.n as f32
    }

    /// Blends `c` over pixel (x, y) by `a`.
    fn blend(&mut self, x: i32, y: i32, c: [f32; 3], a: f32) {
        if x < 0 || y < 0 || x >= self.n as i32 || y >= self.n as i32 || a <= 0.0 {
            return;
        }
        let p = &mut self.px[(y as u32 * self.n + x as u32) as usize];
        for k in 0..3 {
            p[k] += (c[k] - p[k]) * a.min(1.0);
        }
    }

    /// A thick anti-aliased polyline (world points), radius `r_m` metres but never under
    /// `min_px` pixels.
    fn stroke(&mut self, pts: &[Vec2], r_m: f32, min_px: f32, c: [f32; 3], closed: bool) {
        let r = (r_m / self.m_per_px).max(min_px);
        let p: Vec<Vec2> = pts.iter().map(|q| self.to_px(*q)).collect();
        let segs = p.len().saturating_sub(1) + usize::from(closed && p.len() > 2);
        // Coverage per pixel over the whole stroke, so joints don't blend twice.
        let mut cover: std::collections::HashMap<(i32, i32), f32> = std::collections::HashMap::new();
        for k in 0..segs {
            let (a, b) = (p[k], p[(k + 1) % p.len()]);
            let lo = (a.min(b) - Vec2::splat(r + 1.0)).floor();
            let hi = (a.max(b) + Vec2::splat(r + 1.0)).ceil();
            let ab = b - a;
            let l2 = ab.length_squared().max(1e-6);
            for y in lo.y as i32..=hi.y as i32 {
                for x in lo.x as i32..=hi.x as i32 {
                    let q = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let t = ((q - a).dot(ab) / l2).clamp(0.0, 1.0);
                    let d = q.distance(a + ab * t);
                    let cov = (r + 0.5 - d).clamp(0.0, 1.0);
                    if cov > 0.0 {
                        let e = cover.entry((x, y)).or_insert(0.0);
                        *e = e.max(cov);
                    }
                }
            }
        }
        for ((x, y), a) in cover {
            self.blend(x, y, c, a);
        }
    }
}

/// Height tint (sRGB): valley meadow green → forest → rock → light plateau.
fn tint(h: f32) -> [f32; 3] {
    const STOPS: [(f32, [f32; 3]); 5] = [
        (90.0, [0.27, 0.35, 0.25]),
        (180.0, [0.22, 0.30, 0.21]),
        (300.0, [0.25, 0.30, 0.22]),
        (390.0, [0.38, 0.38, 0.32]),
        (500.0, [0.55, 0.55, 0.50]),
    ];
    if h <= STOPS[0].0 {
        return STOPS[0].1;
    }
    for w in STOPS.windows(2) {
        let ((h0, c0), (h1, c1)) = (w[0], w[1]);
        if h <= h1 {
            let t = (h - h0) / (h1 - h0);
            return [c0[0] + (c1[0] - c0[0]) * t, c0[1] + (c1[1] - c0[1]) * t, c0[2] + (c1[2] - c0[2]) * t];
        }
    }
    STOPS[4].1
}

/// The map image, `n`×`n` RGBA8 sRGB, row 0 at the north edge.
pub fn paint(src: &Source, n: u32) -> Vec<u8> {
    let m_per_px = 2.0 * HALF / n as f32;
    let mut cv = Canvas { n, m_per_px, px: vec![[0.0; 3]; (n * n) as usize] };
    // Terrain: one height per pixel (and a row/column more for the shading and contours).
    let w = n as usize + 1;
    let mut h = vec![0.0f32; w * w];
    for y in 0..w {
        for x in 0..w {
            let p = world(Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / n as f32);
            h[y * w + x] = (src.height)(p.x, p.y);
        }
    }
    // Light from the map's top left (−x, −z), fairly low.
    let light = Vec3::new(-0.55, 0.65, -0.52).normalize();
    for y in 0..n as usize {
        for x in 0..n as usize {
            let h0 = h[y * w + x];
            let hx = h[y * w + x + 1];
            let hs = h[(y + 1) * w + x]; // one pixel down the image (+z)
            let nrm = Vec3::new(-(hx - h0) / m_per_px, 1.0, -(hs - h0) / m_per_px).normalize();
            let lit = nrm.dot(light);
            let flat = Vec3::Y.dot(light);
            let shade = (1.0 + (lit - flat) * 1.6).clamp(0.45, 1.35);
            let mut c = tint(h0);
            for v in &mut c {
                *v *= shade;
            }
            // Contours every 50 m (a faint darker line where a band changes).
            let band = |v: f32| (v / 50.0).floor();
            if band(h0) != band(hx) || band(h0) != band(hs) {
                for v in &mut c {
                    *v *= 0.82;
                }
            }
            cv.px[y * n as usize + x] = c;
        }
    }
    // River.
    if src.river.len() > 1 {
        cv.stroke(src.river, 6.0, 1.2, [0.27, 0.45, 0.58], false);
    }
    // Buildings: footprints in roof grey.
    for b in src.buildings {
        let r = b.half.length();
        let lo = cv.to_px(b.centre - Vec2::splat(r)).floor();
        let hi = cv.to_px(b.centre + Vec2::splat(r)).ceil();
        for y in lo.y as i32..=hi.y as i32 {
            for x in lo.x as i32..=hi.x as i32 {
                let p = world(Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / n as f32);
                if b.contains(p, 0.0) {
                    cv.blend(x, y, [0.66, 0.62, 0.57], 0.95);
                }
            }
        }
    }
    // Roads: every casing first, then the fills, smallest kind first so bigger roads sit on top.
    let pts: Vec<Vec<Vec2>> = src.net.roads.iter().map(|r| r.road.pts.iter().map(|p| Vec2::new(p.x, p.z)).collect()).collect();
    let fill_r = |k: RoadKind| match k {
        RoadKind::BRoad => (3.6, 1.7),
        RoadKind::Street => (2.8, 1.3),
        RoadKind::Gravel => (2.6, 1.3),
        RoadKind::Forest => (2.0, 1.0),
    };
    for (r, p) in src.net.roads.iter().zip(&pts) {
        let (m, px) = fill_r(r.kind);
        cv.stroke(p, m, px + 1.1, [0.06, 0.07, 0.06], r.closed);
    }
    for kind in [RoadKind::Forest, RoadKind::Gravel, RoadKind::Street, RoadKind::BRoad] {
        for (r, p) in src.net.roads.iter().zip(&pts).filter(|(r, _)| r.kind == kind) {
            let (m, px) = fill_r(kind);
            cv.stroke(p, m, px, road_color(kind), r.closed);
        }
    }
    let mut out = Vec::with_capacity((n * n * 4) as usize);
    for p in cv.px {
        out.extend_from_slice(&[(p[0].clamp(0.0, 1.0) * 255.0) as u8, (p[1].clamp(0.0, 1.0) * 255.0) as u8, (p[2].clamp(0.0, 1.0) * 255.0) as u8, 255]);
    }
    out
}

/// A small RGBA8 icon (`n`×`n`): `shape` gives the coverage 0..1 and colour at each pixel
/// centre in −1..1 (y down).
pub fn icon(n: u32, shape: impl Fn(Vec2) -> (f32, [f32; 4])) -> Vec<u8> {
    let mut out = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            // 4×4 supersampled for smooth edges.
            let mut acc = [0.0f32; 4];
            for sy in 0..4 {
                for sx in 0..4 {
                    let q = Vec2::new((x as f32 + (sx as f32 + 0.5) / 4.0) / n as f32, (y as f32 + (sy as f32 + 0.5) / 4.0) / n as f32) * 2.0 - Vec2::ONE;
                    let (a, c) = shape(q);
                    for k in 0..3 {
                        acc[k] += c[k] * a * c[3];
                    }
                    acc[3] += a * c[3];
                }
            }
            let a = acc[3] / 16.0;
            let un = |v: f32| if acc[3] > 0.0 { v / acc[3] } else { 0.0 };
            out.extend_from_slice(&[(un(acc[0]) * 255.0) as u8, (un(acc[1]) * 255.0) as u8, (un(acc[2]) * 255.0) as u8, (a * 255.0) as u8]);
        }
    }
    out
}

/// The player's arrow (white with a dark rim, pointing up).
pub fn arrow(n: u32) -> Vec<u8> {
    let tri = |q: Vec2, s: f32| {
        // Arrowhead: tip (0, −0.9), back corners (±0.7, 0.8), notch (0, 0.4); scaled by s.
        let q = q / s;
        let inside_tri = {
            let (a, b, c) = (Vec2::new(0.0, -0.9), Vec2::new(0.7, 0.8), Vec2::new(-0.7, 0.8));
            let e = |p0: Vec2, p1: Vec2| (p1 - p0).perp_dot(q - p0);
            let (e0, e1, e2) = (e(a, b), e(b, c), e(c, a));
            (e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0) || (e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0)
        };
        let notch = {
            let (a, b, c) = (Vec2::new(0.0, 0.4), Vec2::new(0.7, 0.81), Vec2::new(-0.7, 0.81));
            let e = |p0: Vec2, p1: Vec2| (p1 - p0).perp_dot(q - p0);
            let (e0, e1, e2) = (e(a, b), e(b, c), e(c, a));
            (e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0) || (e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0)
        };
        inside_tri && !notch
    };
    icon(n, |q| {
        if tri(q, 0.82) {
            (1.0, [1.0, 1.0, 1.0, 1.0])
        } else if tri(q, 1.0) {
            (1.0, [0.05, 0.06, 0.05, 1.0])
        } else {
            (0.0, [0.0; 4])
        }
    })
}

/// A white disc with a dark rim (tinted per marker).
pub fn dot(n: u32) -> Vec<u8> {
    icon(n, |q| {
        let d = q.length();
        if d < 0.72 {
            (1.0, [1.0, 1.0, 1.0, 1.0])
        } else if d < 0.98 {
            (1.0, [0.05, 0.06, 0.05, 0.9])
        } else {
            (0.0, [0.0; 4])
        }
    })
}

/// A ring (the waypoint flag on the map).
pub fn ring(n: u32) -> Vec<u8> {
    icon(n, |q| {
        let d = q.length();
        if (0.62..0.84).contains(&d) {
            (1.0, [1.0, 1.0, 1.0, 1.0])
        } else if (0.5..0.98).contains(&d) {
            (1.0, [0.05, 0.06, 0.05, 0.85])
        } else {
            (0.0, [0.0; 4])
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open::{SEED, roads};

    #[test]
    fn uv_round_trips_and_north_is_up() {
        let p = Vec2::new(123.0, -456.0);
        assert!(world(uv(p)).distance(p) < 1e-3);
        assert!(uv(Vec2::new(0.0, -1000.0)).y < uv(Vec2::new(0.0, 1000.0)).y, "−z (north) is nearer the top");
        assert_eq!(uv(Vec2::splat(-HALF)), Vec2::ZERO);
    }

    #[test]
    fn roads_are_painted_in_their_colour() {
        let net = roads::generate(SEED);
        let land = net.land.clone();
        let height = move |x: f32, z: f32| land.height(Vec2::new(x, z));
        let src = Source { net: &net, height: &height, buildings: &[], river: &net.land.river };
        let n = 512;
        let img = paint(&src, n);
        assert_eq!(img.len(), (n * n * 4) as usize);
        // The middle of a long B-road is drawn in the B-road colour.
        let r = net.roads.iter().find(|r| r.kind == RoadKind::BRoad).expect("a B-road");
        let p = r.road.pts[r.road.len() / 2];
        let q = (uv(Vec2::new(p.x, p.z)) * n as f32).as_ivec2();
        let i = ((q.y as u32 * n + q.x as u32) * 4) as usize;
        let want = road_color(RoadKind::BRoad).map(|v| v * 255.0);
        let got = [img[i] as f32, img[i + 1] as f32, img[i + 2] as f32];
        assert!((0..3).all(|k| (got[k] - want[k]).abs() < 40.0), "{got:?} vs {want:?}");
    }

    #[test]
    fn icons_have_soft_edges() {
        let d = dot(32);
        let centre = ((16 * 32 + 16) * 4 + 3) as usize;
        assert_eq!(d[centre], 255);
        assert_eq!(d[3], 0, "corner is clear");
        let a = arrow(32);
        assert!(a.chunks(4).any(|p| p[3] == 255) && a.chunks(4).any(|p| p[3] == 0));
    }
}
