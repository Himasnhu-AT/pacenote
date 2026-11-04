//! What a traffic car looks like (`open::traffic` decides where they are and calls
//! [`spawn_parts`] once per car it shows): four everyday body styles (hatch, saloon, estate,
//! a tall SUV) lofted from side-profile knots, with wheel arches, glazing and pillars, dark
//! bumpers and sills, wheels with rims, grille, plates, mirrors, head and tail lamps, brake
//! lamps that light when the car slows and hazard lamps that blink after a hit.
//!
//! Cheap by design: every mesh and material is shared by all cars (Bevy batches them), six
//! child entities per car, ~1.5 k triangles for the body. Paint is one material per colour
//! (solid or metallic by the car's id), the body's non-painted trim is darkened by vertex
//! colour, glass and lamps are their own shared materials (lamps unlit, so they read in shade
//! and fog).
//!
//! Frame: local −Z forward, +Y up, origin on the ground at the car's centre (as the traffic
//! poses are). Shapes are authored with `z` = metres forward and flipped when written.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// Body styles.
pub const SHAPES: usize = 4;

/// Paint colours (sRGB), as the traffic's `COLORS` indices: white, silver, black, red, blue,
/// green, yellow. Each also comes metallic.
const PAINT: [[f32; 3]; 7] = [[0.86, 0.86, 0.84], [0.56, 0.58, 0.61], [0.035, 0.037, 0.045], [0.56, 0.06, 0.05], [0.07, 0.17, 0.46], [0.07, 0.24, 0.14], [0.82, 0.62, 0.12]];

/// A body style: side profile knots and where its glass and wheels are.
#[derive(Clone, Copy, Debug)]
struct Shape {
    /// Overall length (m); the car spans `-len/2 ..= len/2` (front positive).
    len: f32,
    half_w: f32,
    /// Roof half width as a share of the body's.
    roof_w: f32,
    /// Axles at ±this (m from the centre).
    half_base: f32,
    wheel_r: f32,
    /// Sill height between the arches.
    sill: f32,
    /// `(z, belt / bonnet / boot top, roof)`, front to back; where the roof is at the belt
    /// there is no cabin.
    profile: &'static [(f32, f32, f32)],
    /// Top-surface glass: windscreen and rear window (`z` ranges, back..front).
    windscreen: (f32, f32),
    rear_glass: Option<(f32, f32)>,
    /// Side windows (`z` range) and the pillars between them.
    side_glass: (f32, f32),
    pillars: &'static [f32],
    /// A vertical rear window on the tailgate (heights), for estates and SUVs.
    tail_glass: Option<(f32, f32)>,
}

const HATCH: Shape = Shape {
    len: 4.05,
    half_w: 0.86,
    roof_w: 0.84,
    half_base: 1.25,
    wheel_r: 0.31,
    sill: 0.3,
    profile: &[(2.025, 0.7, 0.7), (1.85, 0.8, 0.8), (0.75, 0.92, 0.92), (0.05, 0.95, 1.43), (-1.45, 0.98, 1.42), (-1.92, 1.0, 1.12), (-2.025, 0.98, 1.05)],
    windscreen: (0.08, 0.72),
    rear_glass: Some((-1.88, -1.48)),
    side_glass: (-1.5, 0.02),
    pillars: &[-0.6],
    tail_glass: None,
};

const SALOON: Shape = Shape {
    len: 4.5,
    half_w: 0.88,
    roof_w: 0.83,
    half_base: 1.36,
    wheel_r: 0.31,
    sill: 0.3,
    profile: &[(2.25, 0.7, 0.7), (2.05, 0.8, 0.8), (0.95, 0.93, 0.93), (0.2, 0.96, 1.42), (-0.85, 0.98, 1.42), (-1.6, 1.0, 1.0), (-2.25, 0.99, 0.99)],
    windscreen: (0.23, 0.92),
    rear_glass: Some((-1.55, -0.88)),
    side_glass: (-1.1, 0.17),
    pillars: &[-0.42],
    tail_glass: None,
};

const ESTATE: Shape = Shape {
    len: 4.6,
    half_w: 0.88,
    roof_w: 0.84,
    half_base: 1.38,
    wheel_r: 0.31,
    sill: 0.3,
    profile: &[(2.3, 0.7, 0.7), (2.1, 0.8, 0.8), (1.0, 0.93, 0.93), (0.25, 0.96, 1.44), (-2.1, 0.99, 1.42), (-2.3, 0.98, 1.36)],
    windscreen: (0.28, 0.97),
    rear_glass: None,
    side_glass: (-2.05, 0.22),
    pillars: &[-0.45, -1.35],
    tail_glass: Some((1.02, 1.3)),
};

const SUV: Shape = Shape {
    len: 4.45,
    half_w: 0.92,
    roof_w: 0.86,
    half_base: 1.33,
    wheel_r: 0.36,
    sill: 0.42,
    profile: &[(2.225, 0.85, 0.85), (2.05, 0.95, 0.95), (1.15, 1.08, 1.08), (0.5, 1.1, 1.72), (-2.05, 1.12, 1.72), (-2.225, 1.1, 1.62)],
    windscreen: (0.53, 1.12),
    rear_glass: None,
    side_glass: (-2.0, 0.47),
    pillars: &[-0.35, -1.3],
    tail_glass: Some((1.16, 1.56)),
};

const ALL: [Shape; SHAPES] = [HATCH, SALOON, ESTATE, SUV];

impl Shape {
    /// Belt and roof height at `z` (linear between knots).
    fn heights(&self, z: f32) -> (f32, f32) {
        let p = self.profile;
        if z >= p[0].0 {
            return (p[0].1, p[0].2);
        }
        for w in p.windows(2) {
            let (a, b) = (w[0], w[1]);
            if z <= a.0 && z >= b.0 {
                let t = (a.0 - z) / (a.0 - b.0).max(1e-4);
                return (a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t);
            }
        }
        let l = p[p.len() - 1];
        (l.1, l.2)
    }

    /// Half width at `z`: tapered over the last 0.45 m at each end.
    fn width(&self, z: f32) -> f32 {
        let t = ((z.abs() - (self.len * 0.5 - 0.45)) / 0.45).max(0.0);
        self.half_w * (1.0 - 0.1 * t * t)
    }

    /// The underside at `z`: the sill, raised round each wheel (the arches).
    fn underside(&self, z: f32) -> f32 {
        let ra = self.wheel_r + 0.07;
        let mut y = self.sill;
        for zw in [self.half_base, -self.half_base] {
            let dz = z - zw;
            if dz.abs() < ra {
                y = y.max(self.wheel_r - 0.02 + (ra * ra - dz * dz).sqrt());
            }
        }
        y
    }

    fn in_side_glass(&self, z: f32) -> bool {
        z > self.side_glass.0 && z < self.side_glass.1 && self.pillars.iter().all(|p| (z - p).abs() > 0.05)
    }

    fn in_top_glass(&self, z: f32) -> bool {
        let inside = |r: (f32, f32)| z > r.0 && z < r.1;
        inside(self.windscreen) || self.rear_glass.is_some_and(inside)
    }
}

/// Vertex colour of the body's trim (sills, bumpers, underside): multiplies the paint.
const TRIM: f32 = 0.1;

/// Triangles with normals and colours.
#[derive(Default)]
struct Build {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

/// Authoring frame (`z` forward) to the car's (−Z forward).
fn out(p: Vec3) -> Vec3 {
    Vec3::new(p.x, p.y, -p.z)
}

impl Build {
    fn vert(&mut self, p: Vec3, n: Vec3, c: [f32; 4]) -> u32 {
        self.pos.push(out(p).to_array());
        self.nrm.push(out(n).to_array());
        self.col.push(c);
        self.pos.len() as u32 - 1
    }

    /// A quad `a b c d` (any winding), facing `facing`, with per-corner normals.
    fn quad_n(&mut self, p: [Vec3; 4], n: [Vec3; 4], facing: Vec3, c: [f32; 4]) {
        let face = (p[1] - p[0]).cross(p[2] - p[0]) + (p[2] - p[0]).cross(p[3] - p[0]);
        let order: [usize; 4] = if face.dot(facing) >= 0.0 { [0, 1, 2, 3] } else { [0, 3, 2, 1] };
        let i: Vec<u32> = order.iter().map(|&k| self.vert(p[k], n[k], c)).collect();
        // The car's frame flips z, which flips the winding: emit reversed.
        self.idx.extend_from_slice(&[i[0], i[2], i[1], i[0], i[3], i[2]]);
    }

    /// A flat quad.
    fn quad(&mut self, p: [Vec3; 4], facing: Vec3, c: [f32; 4]) {
        let n = facing.normalize_or(Vec3::Y);
        self.quad_n(p, [n; 4], facing, c);
    }

    /// An axis-aligned box (authoring frame).
    fn cuboid(&mut self, lo: Vec3, hi: Vec3, c: [f32; 4]) {
        let v = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
        let (a, b) = (lo, hi);
        self.quad([v(b.x, a.y, a.z), v(b.x, b.y, a.z), v(b.x, b.y, b.z), v(b.x, a.y, b.z)], Vec3::X, c);
        self.quad([v(a.x, a.y, a.z), v(a.x, b.y, a.z), v(a.x, b.y, b.z), v(a.x, a.y, b.z)], -Vec3::X, c);
        self.quad([v(a.x, b.y, a.z), v(b.x, b.y, a.z), v(b.x, b.y, b.z), v(a.x, b.y, b.z)], Vec3::Y, c);
        self.quad([v(a.x, a.y, b.z), v(b.x, a.y, b.z), v(b.x, b.y, b.z), v(a.x, b.y, b.z)], Vec3::Z, c);
        self.quad([v(a.x, a.y, a.z), v(b.x, a.y, a.z), v(b.x, b.y, a.z), v(a.x, b.y, a.z)], -Vec3::Z, c);
    }

    fn mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}

fn grey(v: f32) -> [f32; 4] {
    [v, v, v, 1.0]
}

/// Ring points of one side (x ≥ 0) at `z`, bottom centre to roof centre: underside, rocker,
/// lower side, upper side, belt, roof rail, roof edge, roof centre.
fn half_ring(s: &Shape, z: f32) -> [Vec2; 8] {
    let (belt, roof) = s.heights(z);
    let roof = roof.max(belt);
    let hw = s.width(z);
    let rw = hw * s.roof_w;
    let y0 = s.underside(z).min(belt - 0.16);
    let mut p = [
        Vec2::new(0.0, y0),
        Vec2::new(hw - 0.07, y0),
        Vec2::new(hw, y0 + 0.07),
        Vec2::new(hw, belt - 0.1),
        Vec2::new(hw - 0.025, belt),
        Vec2::new(rw, (roof - 0.05).max(belt)),
        Vec2::new(rw - 0.07, roof),
        Vec2::new(0.0, roof + if roof > belt + 0.1 { 0.025 } else { 0.015 }),
    ];
    for i in 1..p.len() {
        p[i].y = p[i].y.max(p[i - 1].y);
    }
    p
}

/// The full ring (right side up, then the left side back down), 14 points.
fn ring(s: &Shape, z: f32) -> Vec<Vec3> {
    let h = half_ring(s, z);
    let mut r: Vec<Vec3> = h.iter().map(|p| Vec3::new(p.x, p.y, z)).collect();
    r.extend(h[1..7].iter().rev().map(|p| Vec3::new(-p.x, p.y, z)));
    r
}

/// What the ring segment `k` (from point `k` to `k + 1`) is at `z`: 0 paint, 1 trim, 2 glass.
fn segment(s: &Shape, k: usize, z: f32) -> u8 {
    // Mirror the left side's segments onto the right's.
    let k = if k < 7 { k } else { 13 - k };
    let (belt, roof) = s.heights(z);
    let cabin = roof > belt + 0.2;
    let bumper = z.abs() > s.len * 0.5 - 0.32;
    match k {
        0 | 1 => 1,
        2 if bumper => 1,
        4 if cabin && s.in_side_glass(z) => 2,
        6 if s.in_top_glass(z) => 2,
        _ => 0,
    }
}

/// The body (paint, trim by vertex colour) and its glass, for one shape.
fn body(s: &Shape) -> (Mesh, Mesh) {
    let mut zs: Vec<f32> = Vec::new();
    let n = (s.len / 0.075).ceil() as usize;
    for i in 0..=n {
        zs.push(s.len * 0.5 - s.len * i as f32 / n as f32);
    }
    let grid: Vec<Vec<Vec3>> = zs.iter().map(|&z| ring(s, z)).collect();
    let m = grid[0].len();
    // Smooth normals from the grid (around the ring × along the car), pointing outward.
    let normal = |i: usize, k: usize| -> Vec3 {
        let around = grid[i][(k + 1) % m] - grid[i][(k + m - 1) % m];
        let along = grid[(i + 1).min(grid.len() - 1)][k] - grid[i.saturating_sub(1)][k];
        let mut n = around.cross(along).normalize_or(Vec3::Y);
        let centre = Vec3::new(0.0, 0.8, grid[i][k].z);
        if n.dot(grid[i][k] - centre) < 0.0 {
            n = -n;
        }
        n
    };
    let mut paint = Build::default();
    let mut glass = Build::default();
    for i in 0..grid.len() - 1 {
        let zm = (zs[i] + zs[i + 1]) * 0.5;
        for k in 0..m {
            let k1 = (k + 1) % m;
            let p = [grid[i][k], grid[i][k1], grid[i + 1][k1], grid[i + 1][k]];
            let nn = [normal(i, k), normal(i, k1), normal(i + 1, k1), normal(i + 1, k)];
            let facing = nn[0] + nn[1] + nn[2] + nn[3];
            match segment(s, k, zm) {
                2 => glass.quad_n(p, nn, facing, grey(1.0)),
                1 => paint.quad_n(p, nn, facing, grey(TRIM)),
                _ => paint.quad_n(p, nn, facing, grey(1.0)),
            }
        }
    }
    // End caps: fans from the ring's middle.
    for (i, dir) in [(0usize, 1.0f32), (grid.len() - 1, -1.0)] {
        let r = &grid[i];
        let c = r.iter().copied().sum::<Vec3>() / m as f32;
        let f = Vec3::Z * dir;
        for k in 0..m {
            let (a, b) = (r[k], r[(k + 1) % m]);
            let col = if (a.y + b.y) * 0.5 < s.sill + 0.12 { grey(TRIM) } else { grey(1.0) };
            paint.quad([c, a, b, c], f, col);
        }
        // The bumper's band across the end.
        let y0 = s.underside(c.z);
        let hw = s.width(c.z) - 0.02;
        let z = c.z + dir * 0.004;
        paint.quad([Vec3::new(-hw, y0, z), Vec3::new(hw, y0, z), Vec3::new(hw, y0 + 0.2, z), Vec3::new(-hw, y0 + 0.2, z)], f, grey(TRIM));
    }
    if let Some((y0, y1)) = s.tail_glass {
        let z = -s.len * 0.5 - 0.006;
        let hw = s.width(z) * s.roof_w - 0.06;
        glass.quad([Vec3::new(-hw, y0, z), Vec3::new(hw, y0, z), Vec3::new(hw, y1, z), Vec3::new(-hw, y1, z)], -Vec3::Z, grey(1.0));
    }
    (paint.mesh(), glass.mesh())
}

/// Wheels, grille, plates and mirrors (vertex coloured, one shared material).
fn parts(s: &Shape) -> Mesh {
    let mut b = Build::default();
    let r = s.wheel_r;
    const SIDES: usize = 16;
    for zw in [s.half_base, -s.half_base] {
        for side in [1.0f32, -1.0] {
            let xo = side * (s.width(zw) - 0.05);
            let xi = xo - side * 0.2;
            let at = |a: f32, rad: f32, x: f32| Vec3::new(x, r + rad * a.sin(), zw + rad * a.cos());
            for i in 0..SIDES {
                let (a0, a1) = (i as f32 / SIDES as f32 * std::f32::consts::TAU, (i + 1) as f32 / SIDES as f32 * std::f32::consts::TAU);
                let n0 = Vec3::new(0.0, a0.sin(), a0.cos());
                let n1 = Vec3::new(0.0, a1.sin(), a1.cos());
                // Tread.
                b.quad_n([at(a0, r, xi), at(a1, r, xi), at(a1, r, xo), at(a0, r, xo)], [n0, n1, n1, n0], n0 + n1, grey(0.03));
                // Sidewall (outer ring), rim, hub; inner face.
                let fo = Vec3::X * side;
                b.quad([at(a0, r, xo), at(a1, r, xo), at(a1, r * 0.68, xo - side * 0.01), at(a0, r * 0.68, xo - side * 0.01)], fo, grey(0.035));
                let rim = if i % 4 == 0 { 0.16 } else { 0.42 };
                b.quad([at(a0, r * 0.68, xo - side * 0.01), at(a1, r * 0.68, xo - side * 0.01), at(a1, r * 0.2, xo - side * 0.03), at(a0, r * 0.2, xo - side * 0.03)], fo, grey(rim));
                b.quad([at(a0, r * 0.2, xo - side * 0.03), at(a1, r * 0.2, xo - side * 0.03), at(a1, 0.0, xo - side * 0.02), at(a0, 0.0, xo - side * 0.02)], fo, grey(0.55));
                b.quad([at(a0, r, xi), at(a1, r, xi), at(a1, 0.0, xi), at(a0, 0.0, xi)], -fo, grey(0.03));
            }
        }
    }
    // Grille and front plate on the nose; rear plate; mirrors at the windscreen's foot.
    let zf = s.len * 0.5 + 0.005;
    let (nose, _) = s.heights(s.len * 0.5);
    let y0 = s.underside(s.len * 0.5);
    let q = |b: &mut Build, x: f32, ya: f32, yb: f32, z: f32, f: Vec3, c: [f32; 4]| {
        b.quad([Vec3::new(-x, ya, z), Vec3::new(x, ya, z), Vec3::new(x, yb, z), Vec3::new(-x, yb, z)], f, c);
    };
    q(&mut b, 0.32, nose - 0.2, nose - 0.09, zf, Vec3::Z, grey(0.025));
    q(&mut b, 0.25, y0 + 0.05, y0 + 0.16, zf + 0.002, Vec3::Z, grey(0.8));
    let zr = -s.len * 0.5 - 0.005;
    let (tail, _) = s.heights(-s.len * 0.5);
    q(&mut b, 0.25, (tail - 0.42).max(y0 + 0.22), (tail - 0.31).max(y0 + 0.33), zr, -Vec3::Z, grey(0.8));
    let zm = s.windscreen.1 - 0.12;
    let (belt, _) = s.heights(zm);
    for side in [1.0f32, -1.0] {
        let x = s.width(zm) * side;
        let (lo, hi) = (Vec3::new(x, belt + 0.03, zm - 0.06), Vec3::new(x + side * 0.16, belt + 0.15, zm + 0.04));
        b.cuboid(lo.min(hi), lo.max(hi), grey(0.03));
    }
    b.mesh()
}

/// Lamp quads on the ends: head and tail lamps (always lit), brake lamps over the tail lamps,
/// indicators at the four corners.
fn lamps(s: &Shape) -> (Mesh, Mesh, Mesh) {
    let (mut lit, mut brake, mut hazard) = (Build::default(), Build::default(), Build::default());
    let quad = |b: &mut Build, x0: f32, x1: f32, y0: f32, y1: f32, z: f32, f: Vec3, c: [f32; 4]| {
        for side in [1.0f32, -1.0] {
            b.quad([Vec3::new(x0 * side, y0, z), Vec3::new(x1 * side, y0, z), Vec3::new(x1 * side, y1, z), Vec3::new(x0 * side, y1, z)], f, c);
        }
    };
    let (nose, _) = s.heights(s.len * 0.5);
    let (tail, _) = s.heights(-s.len * 0.5);
    let wf = s.width(s.len * 0.5);
    let zf = s.len * 0.5 + 0.007;
    let zr = -s.len * 0.5 - 0.007;
    quad(&mut lit, wf - 0.3, wf - 0.07, nose - 0.17, nose - 0.05, zf, Vec3::Z, [0.95, 0.93, 0.86, 1.0]);
    quad(&mut hazard, wf - 0.07, wf - 0.02, nose - 0.17, nose - 0.08, zf, Vec3::Z, [1.0, 0.42, 0.02, 1.0]);
    let (t0, t1) = (tail - 0.24, tail - 0.07);
    quad(&mut lit, wf - 0.27, wf - 0.04, t0, t1, zr, -Vec3::Z, [0.42, 0.015, 0.015, 1.0]);
    quad(&mut brake, wf - 0.27, wf - 0.04, t0, t1, zr - 0.003, -Vec3::Z, [1.0, 0.06, 0.04, 1.0]);
    quad(&mut hazard, wf - 0.27, wf - 0.18, t0 - 0.06, t0 - 0.005, zr - 0.003, -Vec3::Z, [1.0, 0.42, 0.02, 1.0]);
    (lit.mesh(), brake.mesh(), hazard.mesh())
}

/// One body style's meshes.
#[derive(Clone)]
struct ShapeMeshes {
    body: Handle<Mesh>,
    glass: Handle<Mesh>,
    parts: Handle<Mesh>,
    lamps: Handle<Mesh>,
    brake: Handle<Mesh>,
    hazard: Handle<Mesh>,
}

/// Shared meshes and materials for every traffic car.
#[derive(Resource)]
pub struct TrafficArt {
    shapes: Vec<ShapeMeshes>,
    /// Per colour: solid, metallic.
    paint: Vec<[Handle<StandardMaterial>; 2]>,
    glass: Handle<StandardMaterial>,
    parts: Handle<StandardMaterial>,
    lamp: Handle<StandardMaterial>,
}

impl TrafficArt {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> TrafficArt {
        let shapes = ALL
            .iter()
            .map(|s| {
                let (body, glass) = body(s);
                let (lamps, brake, hazard) = lamps(s);
                ShapeMeshes { body: meshes.add(body), glass: meshes.add(glass), parts: meshes.add(parts(s)), lamps: meshes.add(lamps), brake: meshes.add(brake), hazard: meshes.add(hazard) }
            })
            .collect();
        let paint = PAINT
            .iter()
            .map(|c| {
                let base = Color::srgb(c[0], c[1], c[2]);
                let solid = StandardMaterial { base_color: base, metallic: 0.0, perceptual_roughness: 0.42, clearcoat: 0.7, clearcoat_perceptual_roughness: 0.12, ..default() };
                let metal = StandardMaterial { base_color: Color::srgb(c[0] * 0.8, c[1] * 0.8, c[2] * 0.8), metallic: 0.35, perceptual_roughness: 0.34, clearcoat: 0.9, clearcoat_perceptual_roughness: 0.08, ..default() };
                [materials.add(solid), materials.add(metal)]
            })
            .collect();
        let glass = materials.add(StandardMaterial { base_color: Color::srgb(0.02, 0.025, 0.03), perceptual_roughness: 0.06, reflectance: 0.7, ..default() });
        let parts = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.6, ..default() });
        let lamp = materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, ..default() });
        TrafficArt { shapes, paint, glass, parts, lamp }
    }
}

/// The brake lamps of a traffic car (shown while it slows or stands).
#[derive(Component)]
pub struct BrakeLamps;

/// The hazard lamps of a traffic car (blink while it is stopped after a hit).
#[derive(Component)]
pub struct HazardLamps;

/// The body style of car `id` (stable for its life).
pub fn shape_of(id: u32) -> usize {
    (id.wrapping_mul(2_654_435_761) >> 13) as usize % SHAPES
}

/// Spawns a traffic car's meshes under its root: `color` is the traffic's paint index.
pub fn spawn_parts(p: &mut ChildSpawnerCommands, art: &TrafficArt, id: u32, color: u8) {
    let m = &art.shapes[shape_of(id)];
    let paint = art.paint[color as usize % art.paint.len()][((id >> 3) & 1) as usize].clone();
    use bevy::light::NotShadowCaster as NoShadow;
    p.spawn((Mesh3d(m.body.clone()), MeshMaterial3d(paint), NoShadow, super::mirror::seen()));
    p.spawn((Mesh3d(m.glass.clone()), MeshMaterial3d(art.glass.clone()), NoShadow, super::mirror::seen()));
    p.spawn((Mesh3d(m.parts.clone()), MeshMaterial3d(art.parts.clone()), NoShadow, super::mirror::seen()));
    p.spawn((Mesh3d(m.lamps.clone()), MeshMaterial3d(art.lamp.clone()), NoShadow, super::mirror::seen(), bevy::light::NotShadowReceiver));
    p.spawn((Mesh3d(m.brake.clone()), MeshMaterial3d(art.lamp.clone()), NoShadow, super::mirror::seen(), BrakeLamps, Visibility::Hidden));
    p.spawn((Mesh3d(m.hazard.clone()), MeshMaterial3d(art.lamp.clone()), NoShadow, super::mirror::seen(), HazardLamps, Visibility::Hidden));
}

/// Shows brake lamps on cars that slow down or stand, blinks hazards on cars stopped by a hit.
pub fn lamps_update(
    time: Res<Time>,
    traffic: Option<Res<crate::open::traffic::Traffic>>,
    roots: Query<(&crate::open::traffic::TrafficCar, &Children)>,
    mut brakes: Query<&mut Visibility, (With<BrakeLamps>, Without<HazardLamps>)>,
    mut hazards: Query<&mut Visibility, (With<HazardLamps>, Without<BrakeLamps>)>,
    mut last: Local<std::collections::HashMap<u32, (f32, f32)>>,
) {
    let Some(traffic) = traffic else { return };
    let cars: std::collections::HashMap<u32, (f32, bool)> = traffic.cars().map(|c| (c.id, (c.speed, c.hazard))).collect();
    let dt = time.delta_secs().max(1e-4);
    let blink = (time.elapsed_secs() * 1.5).fract() < 0.5;
    let mut seen = std::collections::HashMap::with_capacity(cars.len());
    for (car, kids) in roots.iter() {
        let Some(&(v, hazard)) = cars.get(&car.id) else { continue };
        // Smoothed deceleration (m/s²), so the lamps don't flicker with the sim's steps.
        let (pv, pd) = last.get(&car.id).copied().unwrap_or((v, 0.0));
        let decel = pd + ((pv - v) / dt - pd) * (dt / 0.3).min(1.0);
        seen.insert(car.id, (v, decel));
        let braking = decel > 0.9 || v < 0.3;
        for k in kids.iter() {
            if let Ok(mut vis) = brakes.get_mut(k) {
                vis.set_if_neq(if braking { Visibility::Inherited } else { Visibility::Hidden });
            }
            if let Ok(mut vis) = hazards.get_mut(k) {
                vis.set_if_neq(if hazard && blink { Visibility::Inherited } else { Visibility::Hidden });
            }
        }
    }
    *last = seen;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_builds_and_fits_the_traffic_box() {
        for s in &ALL {
            let (b, g) = body(s);
            assert!(b.count_vertices() > 500 && g.count_vertices() > 20);
            assert!(parts(s).count_vertices() > 100);
            // Within the traffic's collision footprint, give or take a bumper.
            assert!((s.len - crate::open::traffic::CAR_LEN).abs() < 0.45, "{}", s.len);
            assert!((s.half_w * 2.0 - crate::open::traffic::CAR_WIDTH).abs() < 0.1);
            // Wheels sit on the ground and in their arches.
            assert!(s.underside(s.half_base) > s.wheel_r * 2.0 - 0.02);
        }
    }

    #[test]
    fn body_normals_point_out() {
        for s in &ALL {
            let (b, _) = body(s);
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = b.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(n)) = b.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!() };
            let out = p.iter().zip(n).filter(|(p, n)| {
                let p = Vec3::from_array(**p);
                let d = Vec3::new(p.x, p.y - 0.8, 0.0);
                d.length() > 0.3 && d.dot(Vec3::from_array(**n)) < -0.2
            });
            assert!(out.count() < p.len() / 50);
        }
    }

    #[test]
    fn triangles_face_their_normals() {
        for s in &ALL {
            let (b, _) = body(s);
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = b.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(n)) = b.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!() };
            let Some(Indices::U32(idx)) = b.indices() else { panic!() };
            let mut bad = 0;
            for t in idx.chunks(3) {
                let [a, b2, c] = [t[0], t[1], t[2]].map(|i| Vec3::from_array(p[i as usize]));
                let face = (b2 - a).cross(c - a);
                if face.length() > 1e-6 && face.dot(Vec3::from_array(n[t[0] as usize])) < 0.0 {
                    bad += 1;
                }
            }
            assert!(bad < idx.len() / 3 / 50, "{bad} of {}", idx.len() / 3);
        }
    }

    #[test]
    fn shapes_spread_over_ids() {
        let mut n = [0; SHAPES];
        for id in 0..400 {
            n[shape_of(id)] += 1;
        }
        assert!(n.iter().all(|&k| k > 60), "{n:?}");
    }
}
