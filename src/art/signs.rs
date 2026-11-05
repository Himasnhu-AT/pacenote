//! Meshes for the furniture `open::furniture` places: guard rails (steel W-beam and timber),
//! stone blocks, warning / speed / town signs, chevron boards, km and reflector posts, street
//! lights. Everything is one material over one procedural atlas (original pictograms and
//! lettering, drawn here: no fonts, no real-world sign designs), merged into one mesh per
//! 512 m tile, range 250 m, no shadows: a handful of draws.
//!
//! **Owner: stream B (ROADSIDE).** Design: `docs/festival/ART_BIBLE.md` §4 (look), `docs/festival/WORLD_PASS.md` §4
//! (placement), `docs/festival/TECH_PASS.md` §4 (≤ +20 draws), `docs/festival/DECISIONS.md` W3-W4.

use std::collections::BTreeMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::VisibilityRange;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use engine::render_budget::Scenery;

use crate::open::OpenWorld;
use crate::open::furniture::{Furniture, RAIL_TOP, RailKind, RailRun, SignKind, warn};

pub struct SignsPlugin;

impl Plugin for SignsPlugin {
    fn build(&self, _app: &mut App) {
        #[cfg(feature = "dev")]
        dev::register(_app);
    }
}

#[cfg(feature = "dev")]
mod dev {
    use bevy::prelude::*;
    use engine::devtools::{AddControlCommand, params};
    use serde_json::{Value, json};

    use super::FurnitureArt;

    pub fn register(app: &mut App) {
        app.add_control_command(
            "roadside.art",
            "Show or hide the roadside furniture and road-condition meshes (A/B for perf); no params: count them",
            params(&[("on", "boolean", "show (true) or hide (false)")]),
            toggle,
        );
    }

    fn toggle(world: &mut World, p: Value) -> Result<Value, String> {
        let on = p.get("on").and_then(Value::as_bool);
        let mut q = world.query_filtered::<&mut Visibility, With<FurnitureArt>>();
        let mut n = 0;
        for mut v in q.iter_mut(world) {
            n += 1;
            if let Some(on) = on {
                *v = if on { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
        Ok(json!({"meshes": n, "on": on}))
    }
}

/// Furniture meshes, for despawning when the roads are rebuilt.
#[derive(Component)]
pub struct FurnitureArt;

/// Merged per tile this size (m).
pub const TILE: f32 = 512.0;
/// Everything is drawn at least this far from the camera (m).
pub const RANGE: f32 = 250.0;
/// Bevy measures a `VisibilityRange` to the mesh's centre, so a tile stays drawn while its
/// centre is within the range plus its half-diagonal: every piece within [`RANGE`] shows.
const TILE_RANGE: f32 = RANGE + TILE * 0.71;

// ------------------------------------------------------------------------ atlas layout

const ATLAS: usize = 1024;
const CELL_PX: usize = 128;
const COLS: usize = ATLAS / CELL_PX;

/// Speed-sign faces in the atlas (0 = end of limit).
const SPEEDS: [u8; 13] = [0, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130];

/// A region of the atlas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    Warn(u8),
    Speed(u8),
    Chevron,
    Km(u16),
    /// Reflector post face: amber (the driver's right) or white.
    Reflector { amber: bool },
    Town,
    TownEnd,
    Swatch(Swatch),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Swatch {
    White,
    Steel,
    Timber,
    Stone,
    Post,
    Back,
    Black,
    Lamp,
}

const SWATCHES: [Swatch; 8] = [Swatch::White, Swatch::Steel, Swatch::Timber, Swatch::Stone, Swatch::Post, Swatch::Back, Swatch::Black, Swatch::Lamp];

impl Cell {
    /// First cell index and width in cells.
    fn index(self) -> (usize, usize) {
        match self {
            Cell::Warn(w) => (w.min(warn::COUNT - 1) as usize, 1),
            Cell::Speed(v) => (17 + SPEEDS.iter().position(|&s| s == v).unwrap_or(0), 1),
            Cell::Chevron => (30, 1),
            Cell::Km(k) => (30 + k.clamp(1, 9) as usize, 1),
            Cell::Reflector { amber } => (if amber { 40 } else { 41 }, 1),
            Cell::Town => (48, 3),
            Cell::TownEnd => (51, 3),
            Cell::Swatch(s) => (56 + SWATCHES.iter().position(|&x| x == s).unwrap_or(0), 1),
        }
    }

    /// UV rectangle (u0, v0, u1, v1), inset a little against bleeding.
    fn uv(self) -> [f32; 4] {
        let (k, w) = self.index();
        let (c, r) = (k % COLS, k / COLS);
        let inset = 2.0 / ATLAS as f32;
        let s = 1.0 / COLS as f32;
        [c as f32 * s + inset, r as f32 * s + inset, (c + w) as f32 * s - inset, (r + 1) as f32 * s - inset]
    }

    /// A point of the cell (0..1 each way across the whole cell, v down) in atlas UVs.
    fn at(self, u: f32, v: f32) -> [f32; 2] {
        let [u0, v0, u1, v1] = self.uv();
        [u0 + (u1 - u0) * u, v0 + (v1 - v0) * v]
    }

    /// The cell's middle (for solid swatches).
    fn mid(self) -> [f32; 2] {
        self.at(0.5, 0.5)
    }

    /// (roughness, metallic) for the metallic-roughness map.
    fn rough_metal(self) -> (f32, f32) {
        match self {
            Cell::Swatch(Swatch::Steel) => (0.45, 0.9),
            Cell::Swatch(Swatch::Timber | Swatch::Stone) => (0.95, 0.0),
            Cell::Swatch(Swatch::Back | Swatch::Post) => (0.55, 0.6),
            Cell::Swatch(_) => (0.6, 0.0),
            Cell::Reflector { .. } => (0.4, 0.0),
            // Retro-reflective sheeting reads glossy.
            _ => (0.3, 0.0),
        }
    }
}

/// Every cell the atlas draws.
fn all_cells() -> Vec<Cell> {
    let mut v: Vec<Cell> = (0..warn::COUNT).map(Cell::Warn).collect();
    v.extend(SPEEDS.iter().map(|&s| Cell::Speed(s)));
    v.push(Cell::Chevron);
    v.extend((1..=9).map(Cell::Km));
    v.extend([Cell::Reflector { amber: true }, Cell::Reflector { amber: false }, Cell::Town, Cell::TownEnd]);
    v.extend(SWATCHES.iter().map(|&s| Cell::Swatch(s)));
    v
}

// ------------------------------------------------------------------------ drawing

/// sRGB colours.
mod col {
    pub const RED: [f32; 3] = [0.80, 0.08, 0.06];
    pub const WHITE: [f32; 3] = [0.94, 0.94, 0.92];
    pub const BLACK: [f32; 3] = [0.08, 0.08, 0.08];
    pub const YELLOW: [f32; 3] = [0.96, 0.78, 0.12];
    pub const AMBER: [f32; 3] = [1.0, 0.58, 0.0];
    pub const STEEL: [f32; 3] = [0.62, 0.63, 0.62];
    pub const TIMBER: [f32; 3] = [0.48, 0.36, 0.24];
    pub const STONE: [f32; 3] = [0.52, 0.50, 0.46];
    pub const POST: [f32; 3] = [0.30, 0.31, 0.32];
    pub const BACK: [f32; 3] = [0.55, 0.56, 0.55];
    pub const GREY: [f32; 3] = [0.45, 0.45, 0.45];
    pub const LAMP: [f32; 3] = [1.0, 0.95, 0.80];
}

/// A shape drawn in a cell's 0..1 square (v down).
enum Prim {
    /// A stroke along a polyline, this wide.
    Line(Vec<Vec2>, f32),
    Disc(Vec2, f32),
    /// A filled polygon.
    Poly(Vec<Vec2>),
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let d = b - a;
    let t = ((p - a).dot(d) / d.length_squared().max(1e-12)).clamp(0.0, 1.0);
    p.distance(a + d * t)
}

/// Signed distance to a polygon (negative inside).
fn poly_dist(p: Vec2, pts: &[Vec2]) -> f32 {
    let n = pts.len();
    let mut d = f32::MAX;
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        d = d.min(seg_dist(p, a, b));
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (b.x - a.x) * (p.y - a.y) / (b.y - a.y) {
            inside = !inside;
        }
    }
    if inside { -d } else { d }
}

impl Prim {
    fn dist(&self, p: Vec2) -> f32 {
        match self {
            Prim::Line(pts, w) => pts.windows(2).map(|s| seg_dist(p, s[0], s[1])).fold(f32::MAX, f32::min) - w * 0.5,
            Prim::Disc(c, r) => p.distance(*c) - r,
            Prim::Poly(pts) => poly_dist(p, pts),
        }
    }

    fn mirrored(&self) -> Prim {
        let m = |v: &Vec<Vec2>| v.iter().map(|p| Vec2::new(1.0 - p.x, p.y)).collect();
        match self {
            Prim::Line(pts, w) => Prim::Line(m(pts), *w),
            Prim::Disc(c, r) => Prim::Disc(Vec2::new(1.0 - c.x, c.y), *r),
            Prim::Poly(pts) => Prim::Poly(m(pts)),
        }
    }
}

fn v2(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn pl(pts: &[(f32, f32)]) -> Vec<Vec2> {
    pts.iter().map(|&(x, y)| v2(x, y)).collect()
}

/// A closed oval (as a polyline).
fn oval(c: (f32, f32), r: (f32, f32), n: usize) -> Vec<(f32, f32)> {
    (0..=n)
        .map(|k| {
            let a = k as f32 / n as f32 * core::f32::consts::TAU;
            (c.0 + r.0 * libm::cosf(a), c.1 + r.1 * libm::sinf(a))
        })
        .collect()
}

/// A filled arrowhead with its tip at `tip`, pointing along `dir`.
fn arrow(tip: Vec2, dir: Vec2, size: f32) -> Prim {
    let d = dir.normalize();
    let side = d.perp() * size * 0.6;
    let base = tip - d * size;
    Prim::Poly(vec![tip, base + side, base - side])
}

/// Strokes of a glyph in a 1 (wide) × 1 (tall) box, y down; drawn condensed.
fn glyph(c: char) -> Vec<Vec<(f32, f32)>> {
    match c {
        '0' => vec![oval((0.5, 0.5), (0.4, 0.5), 20)],
        '1' => vec![vec![(0.2, 0.22), (0.58, 0.0), (0.58, 1.0)]],
        '2' => vec![vec![(0.1, 0.22), (0.3, 0.02), (0.7, 0.02), (0.9, 0.22), (0.9, 0.4), (0.1, 0.98), (0.92, 0.98)]],
        '3' => vec![vec![(0.1, 0.12), (0.3, 0.0), (0.7, 0.0), (0.9, 0.15), (0.9, 0.35), (0.7, 0.48), (0.4, 0.48)], vec![(0.7, 0.48), (0.92, 0.64), (0.92, 0.85), (0.7, 1.0), (0.3, 1.0), (0.08, 0.88)]],
        '4' => vec![vec![(0.72, 1.0), (0.72, 0.0), (0.08, 0.7), (0.95, 0.7)]],
        '5' => vec![vec![(0.88, 0.0), (0.18, 0.0), (0.12, 0.45), (0.6, 0.4), (0.9, 0.6), (0.9, 0.85), (0.7, 1.0), (0.3, 1.0), (0.08, 0.88)]],
        '6' => vec![vec![(0.82, 0.06), (0.55, 0.0), (0.25, 0.12), (0.1, 0.5), (0.1, 0.8), (0.3, 1.0), (0.7, 1.0), (0.9, 0.8), (0.9, 0.62), (0.7, 0.45), (0.3, 0.45), (0.1, 0.6)]],
        '7' => vec![vec![(0.08, 0.0), (0.92, 0.0), (0.4, 1.0)]],
        '8' => vec![oval((0.5, 0.25), (0.33, 0.25), 18), oval((0.5, 0.73), (0.4, 0.27), 18)],
        '9' => vec![vec![(0.18, 0.94), (0.45, 1.0), (0.75, 0.88), (0.9, 0.5), (0.9, 0.2), (0.7, 0.0), (0.3, 0.0), (0.1, 0.2), (0.1, 0.38), (0.3, 0.55), (0.7, 0.55), (0.9, 0.4)]],
        '%' => vec![vec![(0.85, 0.0), (0.15, 1.0)], oval((0.22, 0.18), (0.14, 0.17), 12), oval((0.78, 0.82), (0.14, 0.17), 12)],
        'F' => vec![vec![(0.88, 0.0), (0.15, 0.0), (0.15, 1.0)], vec![(0.15, 0.48), (0.7, 0.48)]],
        'E' => vec![vec![(0.88, 0.0), (0.15, 0.0), (0.15, 1.0), (0.88, 1.0)], vec![(0.15, 0.48), (0.72, 0.48)]],
        'R' => vec![vec![(0.15, 1.0), (0.15, 0.0), (0.65, 0.0), (0.88, 0.12), (0.88, 0.38), (0.65, 0.5), (0.15, 0.5)], vec![(0.52, 0.5), (0.9, 1.0)]],
        'N' => vec![vec![(0.13, 1.0), (0.13, 0.0), (0.87, 1.0), (0.87, 0.0)]],
        'H' => vec![vec![(0.13, 0.0), (0.13, 1.0)], vec![(0.87, 0.0), (0.87, 1.0)], vec![(0.13, 0.5), (0.87, 0.5)]],
        'O' => vec![oval((0.5, 0.5), (0.42, 0.5), 22)],
        'L' => vec![vec![(0.15, 0.0), (0.15, 1.0), (0.88, 1.0)]],
        'W' => vec![vec![(0.0, 0.0), (0.22, 1.0), (0.5, 0.3), (0.78, 1.0), (1.0, 0.0)]],
        _ => vec![],
    }
}

/// Text centred on `c`, letters `h` tall (cell units).
fn text(s: &str, c: Vec2, h: f32) -> Vec<Prim> {
    let w = h * 0.58;
    let gap = h * 0.2;
    let width = |ch: char| if ch == 'W' { w * 1.3 } else { w };
    let total: f32 = s.chars().map(width).sum::<f32>() + (s.chars().count() as f32 - 1.0).max(0.0) * gap;
    let mut x = c.x - total * 0.5;
    let mut out = Vec::new();
    for ch in s.chars() {
        let cw = width(ch);
        for stroke in glyph(ch) {
            out.push(Prim::Line(stroke.iter().map(|&(gx, gy)| v2(x + gx * cw, c.y - h * 0.5 + gy * h)).collect(), h * 0.15));
        }
        x += cw + gap;
    }
    out
}

/// The warning triangle's corners in its cell.
const TRI: [(f32, f32); 3] = [(0.5, 0.06), (0.03, 0.88), (0.97, 0.88)];

/// The black pictogram of a warning sign (left-hand versions; the right ones are mirrored).
fn picto(w: u8) -> Vec<Prim> {
    let mirror = matches!(w, warn::HAIRPIN_R | warn::DOUBLE_R | warn::JUNCTION_R | warn::BEND_R);
    let base = match w {
        warn::HAIRPIN_L | warn::HAIRPIN_R => {
            let mut pts = vec![v2(0.6, 0.82), v2(0.6, 0.52)];
            for k in 0..=8 {
                let a = k as f32 / 8.0 * core::f32::consts::PI;
                pts.push(v2(0.5 + 0.1 * libm::cosf(a), 0.52 - 0.11 * libm::sinf(a)));
            }
            pts.push(v2(0.4, 0.62));
            vec![Prim::Line(pts, 0.06), arrow(v2(0.4, 0.73), v2(0.0, 1.0), 0.1)]
        }
        warn::DOUBLE_L | warn::DOUBLE_R => vec![
            Prim::Line(pl(&[(0.56, 0.84), (0.56, 0.74), (0.52, 0.68), (0.44, 0.63), (0.42, 0.56), (0.46, 0.5), (0.55, 0.46), (0.58, 0.42)]), 0.06),
            arrow(v2(0.6, 0.31), v2(0.12, -1.0), 0.1),
        ],
        warn::BEND_L | warn::BEND_R => vec![Prim::Line(pl(&[(0.56, 0.84), (0.56, 0.62), (0.53, 0.54), (0.47, 0.49), (0.4, 0.47)]), 0.065), arrow(v2(0.29, 0.46), v2(-1.0, -0.05), 0.11)],
        warn::CREST => {
            let mut pts: Vec<Vec2> = (0..=16).map(|k| k as f32 / 16.0).map(|t| v2(0.24 + 0.52 * t, 0.82 - 0.22 * libm::sinf(core::f32::consts::PI * t))).collect();
            pts.push(v2(0.76, 0.82));
            vec![Prim::Poly(pts)]
        }
        warn::JUMP => {
            let mut hump: Vec<Vec2> = (0..=16).map(|k| k as f32 / 16.0).map(|t| v2(0.24 + 0.52 * t, 0.84 - 0.12 * libm::sinf(core::f32::consts::PI * t))).collect();
            hump.push(v2(0.76, 0.84));
            let car = |x: f32, y: f32| v2(0.5 + (x - 0.5) * 0.98 + (y - 0.5) * 0.2, 0.5 + (y - 0.5) * 0.98 - (x - 0.5) * 0.2);
            vec![
                Prim::Poly(hump),
                Prim::Poly(vec![car(0.37, 0.52), car(0.63, 0.52), car(0.63, 0.6), car(0.37, 0.6)]),
                Prim::Poly(vec![car(0.43, 0.45), car(0.57, 0.45), car(0.6, 0.52), car(0.4, 0.52)]),
                Prim::Disc(car(0.42, 0.62), 0.035),
                Prim::Disc(car(0.58, 0.62), 0.035),
            ]
        }
        warn::JUNCTION_L | warn::JUNCTION_R => vec![Prim::Line(pl(&[(0.5, 0.84), (0.5, 0.34)]), 0.08), Prim::Line(pl(&[(0.5, 0.56), (0.3, 0.56)]), 0.06)],
        warn::CROSSROAD => vec![Prim::Line(pl(&[(0.5, 0.84), (0.5, 0.34)]), 0.08), Prim::Line(pl(&[(0.3, 0.6), (0.7, 0.6)]), 0.07)],
        warn::STEEP => {
            let mut v = vec![Prim::Poly(pl(&[(0.22, 0.83), (0.78, 0.83), (0.22, 0.66)]))];
            v.extend(text("12%", v2(0.56, 0.6), 0.1));
            v
        }
        warn::ROCKS => vec![
            Prim::Poly(pl(&[(0.24, 0.84), (0.26, 0.42), (0.36, 0.38), (0.42, 0.5), (0.4, 0.84)])),
            Prim::Disc(v2(0.5, 0.52), 0.04),
            Prim::Disc(v2(0.58, 0.66), 0.035),
            Prim::Disc(v2(0.66, 0.8), 0.03),
            Prim::Disc(v2(0.52, 0.8), 0.025),
        ],
        warn::DEER => vec![
            Prim::Line(pl(&[(0.38, 0.6), (0.6, 0.58)]), 0.1),
            Prim::Line(pl(&[(0.39, 0.62), (0.36, 0.82)]), 0.035),
            Prim::Line(pl(&[(0.44, 0.63), (0.45, 0.82)]), 0.035),
            Prim::Line(pl(&[(0.56, 0.62), (0.55, 0.82)]), 0.035),
            Prim::Line(pl(&[(0.6, 0.61), (0.64, 0.82)]), 0.035),
            Prim::Line(pl(&[(0.6, 0.57), (0.66, 0.45)]), 0.05),
            Prim::Line(pl(&[(0.65, 0.45), (0.73, 0.48)]), 0.045),
            Prim::Line(pl(&[(0.65, 0.43), (0.62, 0.34), (0.58, 0.32)]), 0.025),
            Prim::Line(pl(&[(0.67, 0.43), (0.71, 0.34), (0.75, 0.33)]), 0.025),
            Prim::Line(pl(&[(0.37, 0.58), (0.33, 0.55)]), 0.03),
        ],
        warn::BRIDGE => vec![Prim::Line(pl(&[(0.36, 0.84), (0.44, 0.66), (0.44, 0.54), (0.36, 0.38)]), 0.06), Prim::Line(pl(&[(0.64, 0.84), (0.56, 0.66), (0.56, 0.54), (0.64, 0.38)]), 0.06)],
        warn::SLIPPERY | warn::LOOSE => {
            let mut v = vec![
                Prim::Poly(pl(&[(0.38, 0.5), (0.62, 0.5), (0.62, 0.58), (0.38, 0.58)])),
                Prim::Poly(pl(&[(0.43, 0.43), (0.57, 0.43), (0.6, 0.5), (0.4, 0.5)])),
                Prim::Disc(v2(0.43, 0.6), 0.035),
                Prim::Disc(v2(0.57, 0.6), 0.035),
            ];
            if w == warn::SLIPPERY {
                v.push(Prim::Line(pl(&[(0.42, 0.66), (0.38, 0.72), (0.44, 0.78), (0.4, 0.85)]), 0.035));
                v.push(Prim::Line(pl(&[(0.6, 0.66), (0.56, 0.72), (0.62, 0.78), (0.58, 0.85)]), 0.035));
            } else {
                for (x, y, r) in [(0.3, 0.8, 0.025), (0.4, 0.74, 0.02), (0.5, 0.82, 0.025), (0.62, 0.76, 0.02), (0.7, 0.82, 0.025), (0.35, 0.66, 0.018), (0.68, 0.64, 0.018)] {
                    v.push(Prim::Disc(v2(x, y), r));
                }
            }
            v
        }
        _ => vec![],
    };
    if mirror { base.iter().map(Prim::mirrored).collect() } else { base }
}

/// Coverage (0..1) of `prims` at `p`, anti-aliased over a pixel `px` wide.
fn cover(prims: &[Prim], p: Vec2, px: f32) -> f32 {
    let d = prims.iter().map(|q| q.dist(p)).fold(f32::MAX, f32::min);
    (0.5 - d / px).clamp(0.0, 1.0)
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn scale3(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// The colour of `cell` at (u, v) (v down, 0..1; `u` runs 0..width in cells). `px`: one pixel.
fn paint(cell: Cell, u: f32, v: f32, px: f32, prims: &[Prim]) -> [f32; 3] {
    let p = v2(u, v);
    let edge = |d: f32| (d / px + 0.5).clamp(0.0, 1.0);
    match cell {
        Cell::Warn(_) => {
            let tri: Vec<Vec2> = TRI.iter().map(|&(x, y)| v2(x, y)).collect();
            let d = poly_dist(p, &tri);
            // Red border 7.5 % wide, a white field, black ink; red outside (only mips see it).
            let c = lerp3(col::RED, col::WHITE, edge(-d - 0.075));
            lerp3(c, col::BLACK, cover(prims, p, px))
        }
        Cell::Speed(0) => {
            let r = p.distance(v2(0.5, 0.5));
            let mut c = col::WHITE;
            // The end-of-limit bar: black diagonal stripes across the disc.
            if (p.x + p.y - 1.0).abs() < 0.12 * core::f32::consts::SQRT_2 {
                c = if ((p.x - p.y) * 14.0).rem_euclid(2.0) < 1.0 { col::BLACK } else { col::GREY };
            }
            lerp3(c, col::BLACK, edge(r - 0.47))
        }
        Cell::Speed(_) => {
            let r = p.distance(v2(0.5, 0.5));
            let c = lerp3(col::RED, col::WHITE, edge(0.36 - r));
            lerp3(c, col::BLACK, cover(prims, p, px))
        }
        Cell::Chevron => lerp3(col::RED, col::WHITE, cover(prims, p, px)),
        Cell::Km(_) => {
            let base = lerp3(col::RED, col::WHITE, edge(v - 0.26));
            lerp3(base, col::BLACK, cover(prims, p, px))
        }
        Cell::Reflector { amber } => {
            if v < 0.3 {
                let lens = (u - 0.5).abs() < 0.28 && (v - 0.15).abs() < 0.09;
                if !lens {
                    col::BLACK
                } else if amber {
                    col::AMBER
                } else {
                    col::WHITE
                }
            } else {
                col::WHITE
            }
        }
        Cell::Town | Cell::TownEnd => {
            let border = u.min(3.0 - u).min(v).min(1.0 - v);
            let mut c = lerp3(col::BLACK, col::YELLOW, edge(border - 0.05));
            c = lerp3(c, col::BLACK, cover(prims, p, px));
            if cell == Cell::TownEnd {
                c = lerp3(c, col::RED, edge(0.06 - seg_dist(p, v2(0.15, 0.88), v2(2.85, 0.12))));
            }
            c
        }
        Cell::Swatch(s) => match s {
            Swatch::White => col::WHITE,
            // Dull streaks along the beam.
            Swatch::Steel => scale3(col::STEEL, 0.94 + 0.06 * libm::sinf(v * 61.0 + libm::sinf(u * 13.0) * 2.0)),
            Swatch::Timber => scale3(col::TIMBER, 0.88 + 0.12 * libm::sinf(v * 40.0 + libm::sinf(u * 9.0) * 3.0)),
            Swatch::Stone => scale3(col::STONE, 0.9 + 0.1 * libm::sinf(u * 37.0) * libm::sinf(v * 29.0)),
            Swatch::Post => col::POST,
            Swatch::Back => col::BACK,
            Swatch::Black => col::BLACK,
            Swatch::Lamp => col::LAMP,
        },
    }
}

/// The shapes of a cell's ink.
fn ink(cell: Cell) -> Vec<Prim> {
    match cell {
        Cell::Warn(w) => picto(w),
        Cell::Speed(0) => vec![],
        Cell::Speed(s) => text(&s.to_string(), v2(0.5, 0.5), if s >= 100 { 0.3 } else { 0.36 }),
        Cell::Chevron => vec![Prim::Line(pl(&[(0.68, 0.14), (0.3, 0.5), (0.68, 0.86)]), 0.2)],
        Cell::Km(k) => text(&k.to_string(), v2(0.5, 0.62), 0.38),
        Cell::Town | Cell::TownEnd => {
            // As big as fits the board.
            let n = TOWN_NAME.chars().count() as f32 + 0.3;
            text(TOWN_NAME, v2(1.5, 0.5), (2.6 / (n * 0.78)).min(0.5))
        }
        _ => vec![],
    }
}

/// The town's name on its boards (`docs/festival/WORLD_PASS.md` §3).
pub const TOWN_NAME: &str = "FERNHOLLOW";

/// The sign atlas (sRGB, full mip chain) and its metallic-roughness map.
fn atlas() -> (Image, Image) {
    let mut px = vec![0u8; ATLAS * ATLAS * 4];
    let mr_n = COLS * 8;
    let mut mr = vec![0u8; mr_n * mr_n * 4];
    for cell in all_cells() {
        let (k, w) = cell.index();
        let (c0, r0) = (k % COLS, k / COLS);
        let prims = ink(cell);
        let pxu = 1.0 / CELL_PX as f32;
        for y in 0..CELL_PX {
            for x in 0..CELL_PX * w {
                let c = paint(cell, (x as f32 + 0.5) * pxu, (y as f32 + 0.5) * pxu, pxu, &prims);
                let o = ((r0 * CELL_PX + y) * ATLAS + c0 * CELL_PX + x) * 4;
                for i in 0..3 {
                    px[o + i] = (c[i].clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                px[o + 3] = 255;
            }
        }
        let (rough, metal) = cell.rough_metal();
        for y in 0..8 {
            for x in 0..8 * w {
                let o = ((r0 * 8 + y) * mr_n + c0 * 8 + x) * 4;
                mr[o..o + 4].copy_from_slice(&[255, (rough * 255.0) as u8, (metal * 255.0) as u8, 255]);
            }
        }
    }
    let sampler = || {
        ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::ClampToEdge,
            address_mode_v: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            anisotropy_clamp: 8,
            ..default()
        })
    };
    let size = |n: usize| Extent3d { width: n as u32, height: n as u32, depth_or_array_layers: 1 };
    let mut img = Image::new(size(ATLAS), TextureDimension::D2, px, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
    img.sampler = sampler();
    engine::gfx::build_mips(&mut img);
    let mut mr = Image::new(size(mr_n), TextureDimension::D2, mr, TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD);
    mr.sampler = sampler();
    (img, mr)
}

/// The atlas material, made once per app.
#[derive(Resource, Clone)]
struct FurnitureMaterial(Handle<StandardMaterial>);

fn material(world: &mut World) -> Handle<StandardMaterial> {
    if let Some(m) = world.get_resource::<FurnitureMaterial>() {
        return m.0.clone();
    }
    let t = std::time::Instant::now();
    let (img, mr) = atlas();
    let (img, mr) = {
        let mut images = world.resource_mut::<Assets<Image>>();
        (images.add(img), images.add(mr))
    };
    let m = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color_texture: Some(img),
        metallic_roughness_texture: Some(mr),
        metallic: 1.0,
        perceptual_roughness: 1.0,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    world.insert_resource(FurnitureMaterial(m.clone()));
    info!("signs: atlas drawn in {:.0} ms", t.elapsed().as_secs_f32() * 1000.0);
    m
}

// ------------------------------------------------------------------------ meshes

/// Vertices and triangles of one merged mesh (flat-shaded: every face has its own vertices).
#[derive(Default)]
pub(super) struct Mesher {
    pos: Vec<Vec3>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl Mesher {
    pub(super) fn v(&mut self, p: Vec3, n: Vec3, uv: [f32; 2], c: [f32; 4]) -> u32 {
        self.pos.push(p);
        self.nrm.push(n.to_array());
        self.uv.push(uv);
        self.col.push(c);
        (self.pos.len() - 1) as u32
    }

    pub(super) fn idx(&mut self, k: &[u32]) {
        self.idx.extend_from_slice(k);
    }

    /// A flat quad a-b-c-d (around its edge) with `uv` at each corner; the normal from its
    /// winding, flipped to face `towards` when given.
    pub(super) fn quad(&mut self, q: [Vec3; 4], uv: [[f32; 2]; 4], c: [f32; 4], towards: Option<Vec3>) {
        let mut n = (q[1] - q[0]).cross(q[3] - q[0]).normalize_or_zero();
        if towards.is_some_and(|t| n.dot(t) < 0.0) {
            n = -n;
        }
        let k: Vec<u32> = (0..4).map(|i| self.v(q[i], n, uv[i], c)).collect();
        self.idx.extend_from_slice(&[k[0], k[1], k[2], k[0], k[2], k[3]]);
    }

    fn tri(&mut self, t: [Vec3; 3], uv: [[f32; 2]; 3], c: [f32; 4], towards: Vec3) {
        let mut n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
        if n.dot(towards) < 0.0 {
            n = -n;
        }
        let k: Vec<u32> = (0..3).map(|i| self.v(t[i], n, uv[i], c)).collect();
        self.idx.extend_from_slice(&k);
    }

    /// A box: centre, rotation, half extents; a UV rectangle per face (+X, −X, +Y, −Y, +Z, −Z).
    fn cuboid(&mut self, c: Vec3, rot: Quat, h: Vec3, faces: [[f32; 4]; 6], col: [f32; 4]) {
        let axes = [Vec3::X, Vec3::Y, Vec3::Z];
        let hs = [h.x, h.y, h.z];
        for (f, uvr) in faces.iter().enumerate() {
            let a = f / 2;
            let s = if f % 2 == 0 { 1.0 } else { -1.0 };
            let n = rot * axes[a] * s;
            // The face's "right" and "up" (+Y, or +Z on the top and bottom).
            let (ri, ui) = match a {
                0 => (2, 1),
                1 => (0, 2),
                _ => (0, 1),
            };
            let right = rot * axes[ri] * hs[ri] * if a == 2 { s } else if a == 0 { -s } else { 1.0 };
            let up = rot * axes[ui] * hs[ui];
            let o = c + n * hs[a];
            let q = [o - right - up, o + right - up, o + right + up, o - right + up];
            let [u0, v0, u1, v1] = *uvr;
            self.quad(q, [[u0, v1], [u1, v1], [u1, v0], [u0, v0]], col, Some(n));
        }
    }

    /// A cylinder from `a` to `b`, `sides` faces, one swatch.
    fn cylinder(&mut self, a: Vec3, b: Vec3, r: f32, sides: usize, uv: [f32; 2], col: [f32; 4]) {
        let axis = (b - a).normalize_or(Vec3::Y);
        let x = axis.any_orthonormal_vector();
        let z = axis.cross(x);
        for k in 0..sides {
            let (t0, t1) = (k as f32 / sides as f32 * core::f32::consts::TAU, (k + 1) as f32 / sides as f32 * core::f32::consts::TAU);
            let (d0, d1) = ((x * libm::cosf(t0) + z * libm::sinf(t0)) * r, (x * libm::cosf(t1) + z * libm::sinf(t1)) * r);
            let mid = (d0 + d1).normalize_or_zero();
            self.quad([a + d0, a + d1, b + d1, b + d0], [uv; 4], col, Some(mid));
        }
    }

    /// Vertices so far.
    pub(super) fn len(&self) -> usize {
        self.pos.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }

    pub(super) fn tris(&self) -> usize {
        self.idx.len() / 3
    }

    /// The mesh, positions relative to its bounds' centre (returned).
    pub(super) fn finish(self) -> (Vec3, Mesh) {
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for p in &self.pos {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        let c = (lo + hi) * 0.5;
        let pos: Vec<[f32; 3]> = self.pos.iter().map(|p| (*p - c).to_array()).collect();
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv);
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.col);
        m.insert_indices(Indices::U32(self.idx));
        (c, m)
    }
}

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

fn sw(s: Swatch) -> [f32; 2] {
    Cell::Swatch(s).mid()
}

fn sw4(s: Swatch) -> [f32; 4] {
    let [u, v] = sw(s);
    [u, v, u, v]
}

/// Unit facing `yaw` (the face's normal), in XZ.
fn facing(yaw: f32) -> Vec3 {
    Vec3::new(libm::sinf(yaw), 0.0, libm::cosf(yaw))
}

/// A seeded number in 0..1 from a position (looks only).
pub(super) fn jitter(p: Vec3, k: u32) -> f32 {
    let mut h = (p.x.to_bits() ^ p.z.to_bits().rotate_left(13) ^ k.wrapping_mul(0x9E37_79B9)).wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    (h & 0xFFFF) as f32 / 65535.0
}

/// A flat sign plate centred at `c`, facing `n`, `w` × `h` m, mapped to `cell` (`shape`: its
/// outline in the cell, u across the whole width, v down, as a fan), with a grey back.
#[allow(clippy::too_many_arguments)]
fn plate(m: &mut Mesher, c: Vec3, n: Vec3, up: Vec3, w: f32, h: f32, cell: Cell, shape: &[(f32, f32)], tint: [f32; 4]) {
    let right = up.cross(n).normalize();
    let wcells = cell.index().1 as f32;
    let at = |u: f32, v: f32, back: bool| c + right * (u / wcells - 0.5) * w + up * (0.5 - v) * h - if back { n * 0.012 } else { Vec3::ZERO };
    let (cu, cv) = shape.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (cu, cv) = (cu / shape.len() as f32, cv / shape.len() as f32);
    for k in 0..shape.len() {
        let (p0, p1) = (shape[k], shape[(k + 1) % shape.len()]);
        m.tri([at(cu, cv, false), at(p0.0, p0.1, false), at(p1.0, p1.1, false)], [cell.at(cu / wcells, cv), cell.at(p0.0 / wcells, p0.1), cell.at(p1.0 / wcells, p1.1)], tint, n);
        m.tri([at(cu, cv, true), at(p1.0, p1.1, true), at(p0.0, p0.1, true)], [sw(Swatch::Back); 3], WHITE, -n);
    }
}

/// A circle's outline in a cell, `n` points.
fn circle(n: usize, r: f32) -> Vec<(f32, f32)> {
    (0..n)
        .map(|k| {
            let a = k as f32 / n as f32 * core::f32::consts::TAU;
            (0.5 + r * libm::cosf(a), 0.5 + r * libm::sinf(a))
        })
        .collect()
}

/// A galvanised pole from just under the ground to `top` m up.
fn pole(m: &mut Mesher, p: Vec3, up: Vec3, top: f32, r: f32) {
    m.cylinder(p - Vec3::Y * 0.3, p + up * top, r, 6, sw(Swatch::Steel), WHITE);
}

fn sign_mesh(m: &mut Mesher, kind: SignKind, pos: Vec3, yaw: f32) {
    let n = facing(yaw);
    // 10 % lean 2-6° (looks only); 5 % faded.
    let lean = if jitter(pos, 1) < 0.1 { (2.0 + jitter(pos, 2) * 4.0).to_radians() * if jitter(pos, 3) < 0.5 { 1.0 } else { -1.0 } } else { 0.0 };
    let up = Quat::from_axis_angle(n, lean) * Vec3::Y;
    let tint = if jitter(pos, 4) < 0.05 { [1.0, 0.88, 0.84, 1.0] } else { WHITE };
    let top = |h: f32| pos + up * h;
    match kind {
        SignKind::Warning(w) => {
            pole(m, pos, up, 2.2, 0.035);
            plate(m, top(2.25) + n * 0.05, n, up, 1.0, 1.0, Cell::Warn(w), &TRI, tint);
        }
        SignKind::Speed(v) => {
            pole(m, pos, up, 2.0, 0.035);
            plate(m, top(2.05) + n * 0.05, n, up, 0.7, 0.7, Cell::Speed(v), &circle(20, 0.49), tint);
        }
        SignKind::TownBoard | SignKind::TownEnd => {
            let right = up.cross(n).normalize();
            for s in [-0.75f32, 0.75] {
                pole(m, pos + right * s, up, 2.1, 0.04);
            }
            let cell = if kind == SignKind::TownBoard { Cell::Town } else { Cell::TownEnd };
            plate(m, top(1.85) + n * 0.06, n, up, 2.1, 0.7, cell, &[(0.0, 0.0), (3.0, 0.0), (3.0, 1.0), (0.0, 1.0)], WHITE);
        }
        SignKind::Chevron(turn) => {
            // Two boards on one post, each turned 30° towards one way of travel; the arrow
            // points the way the road goes for the driver facing it.
            pole(m, pos, up, 1.7, 0.035);
            for (k, way) in [(0usize, 1.0f32), (1, -1.0)] {
                let face = Quat::from_rotation_y(way * 0.55) * n;
                let right = up.cross(face).normalize();
                let c = top(1.3) + face * (0.06 + 0.03 * k as f32);
                // `turn` > 0: the road turns left for the traffic going towards higher
                // stations, and for the other way it's a right turn.
                let points_left = (turn as f32) * way > 0.0;
                let corner = |u: f32, v: f32| c + right * (u - 0.5) * 0.6 + up * (0.5 - v) * 0.75;
                let uv = |u: f32, v: f32| Cell::Chevron.at(if points_left { u } else { 1.0 - u }, v);
                let q = [corner(0.0, 1.0), corner(1.0, 1.0), corner(1.0, 0.0), corner(0.0, 0.0)];
                m.quad(q, [uv(0.0, 1.0), uv(1.0, 1.0), uv(1.0, 0.0), uv(0.0, 0.0)], tint, Some(face));
                m.quad(q.map(|p| p - face * 0.012), [sw(Swatch::Back); 4], WHITE, Some(-face));
            }
        }
        SignKind::KmPost(k) => {
            let rot = Quat::from_rotation_y(yaw);
            let face = Cell::Km(k).uv();
            let side = sw4(Swatch::White);
            // Faces ±Z carry the number (both ways of travel); the cap is red in the texture.
            m.cuboid(pos + Vec3::Y * 0.25, rot, Vec3::new(0.14, 0.55, 0.08), [side, side, side, side, face, face], WHITE);
        }
        SignKind::Reflector => {
            let rot = Quat::from_rotation_y(yaw);
            let (amber, white) = (Cell::Reflector { amber: true }.uv(), Cell::Reflector { amber: false }.uv());
            let side = sw4(Swatch::White);
            // +Z (the yaw) shows white, −Z amber: furniture turns each post by its side.
            m.cuboid(pos + Vec3::Y * 0.35, rot, Vec3::new(0.06, 0.65, 0.05), [side, side, sw4(Swatch::Black), side, white, amber], WHITE);
        }
        SignKind::StreetLight => {
            pole(m, pos, Vec3::Y, 6.2, 0.07);
            let arm_end = pos + Vec3::Y * 6.1 + n * 1.6;
            m.cylinder(pos + Vec3::Y * 5.9, arm_end, 0.045, 5, sw(Swatch::Steel), WHITE);
            let rot = Quat::from_rotation_y(yaw);
            let p = sw4(Swatch::Post);
            m.cuboid(arm_end - Vec3::Y * 0.05, rot, Vec3::new(0.18, 0.06, 0.32), [p, p, p, sw4(Swatch::Lamp), p, p], WHITE);
        }
    }
}

/// The W-beam's cross-section: (towards the road, up from the post's foot) per point.
const W_BEAM: [(f32, f32); 5] = [(0.0, 0.6), (0.07, 0.65), (0.03, 0.75), (0.07, 0.85), (0.0, 0.9)];

/// A run of rail: posts every 2 m and the beam (or log), the ends flared away from the road and
/// buried over their last 4 m. `to_road(k)`: the unit vector from point `k` towards the road.
fn rail_mesh(m: &mut Mesher, r: &RailRun, to_road: &dyn Fn(usize) -> Vec3) {
    let n = r.pts.len();
    if n < 2 {
        return;
    }
    let pts: Vec<(Vec3, Vec3, f32)> = (0..n)
        .map(|k| {
            let t_road = to_road(k);
            let from_end = k.min(n - 1 - k) as f32 * 2.0;
            let t = (from_end / 4.0).clamp(0.0, 1.0);
            (r.pts[k] - t_road * (1.0 - t) * 0.6, t_road, t)
        })
        .collect();
    let steel = r.kind == RailKind::Steel;
    for (k, &(p, t_road, t)) in pts.iter().enumerate() {
        if t < 0.5 {
            continue;
        }
        if steel {
            let rot = Quat::from_rotation_arc(Vec3::Z, t_road);
            m.cuboid(p + Vec3::Y * 0.35 - t_road * 0.04, rot, Vec3::new(0.06, 0.7, 0.04), [sw4(Swatch::Post); 6], WHITE);
            // A white reflector button on every 4th post.
            if k % 4 == 0 {
                m.cuboid(p + Vec3::Y * 0.96, rot, Vec3::new(0.035, 0.035, 0.05), [sw4(Swatch::White); 6], WHITE);
            }
        } else {
            m.cylinder(p - Vec3::Y * 0.3, p + Vec3::Y * 0.9, 0.08, 6, sw(Swatch::Timber), WHITE);
        }
    }
    // Height of a beam point `h` up: full height in the run, sinking into the ground at the ends.
    let lift = |h: f32, s: f32| Vec3::Y * (h * (0.25 + 0.75 * s) - 0.15 * (1.0 - s));
    for k in 0..n - 1 {
        let (a, ta, sa) = pts[k];
        let (b, tb, sb) = pts[k + 1];
        if steel {
            for j in 0..W_BEAM.len() - 1 {
                let (o0, h0) = W_BEAM[j];
                let (o1, h1) = W_BEAM[j + 1];
                let q = [a + ta * (o0 + 0.04) + lift(h0, sa), b + tb * (o0 + 0.04) + lift(h0, sb), b + tb * (o1 + 0.04) + lift(h1, sb), a + ta * (o1 + 0.04) + lift(h1, sa)];
                let uv = Cell::Swatch(Swatch::Steel).at(0.5, (j as f32 + 0.5) / 4.0);
                // Folds catch the light differently.
                let shade = if j % 2 == 0 { 1.0 } else { 0.9 };
                m.quad(q, [uv; 4], [shade, shade, shade, 1.0], Some(ta + Vec3::Y * (if j < 2 { -0.5 } else { 0.5 })));
            }
        } else {
            m.cylinder(a + lift(RAIL_TOP - 0.1, sa) + ta * 0.05, b + lift(RAIL_TOP - 0.1, sb) + tb * 0.05, 0.11, 6, sw(Swatch::Timber), WHITE);
        }
    }
}

/// Spawns one merged mesh per tile with the furniture material.
pub(super) fn spawn_tiles(world: &mut World, tiles: BTreeMap<(i32, i32), Mesher>, mat: Handle<StandardMaterial>, name: &'static str, extra: impl Fn(&mut EntityWorldMut) + Copy) -> (usize, usize) {
    let (mut meshes, mut tris) = (0, 0);
    for (_, m) in tiles {
        if m.is_empty() {
            continue;
        }
        meshes += 1;
        tris += m.tris();
        let (c, mesh) = m.finish();
        let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
        let mut e = world.spawn((
            Name::new(name),
            FurnitureArt,
            super::roadside::Roadside,
            Mesh3d(h),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(c),
            Scenery,
            NotShadowCaster,
            VisibilityRange { start_margin: 0.0..0.0, end_margin: (TILE_RANGE - 30.0)..TILE_RANGE, use_aabb: true },
            DespawnOnExit(engine::flow::AppState::InGame),
            crate::open::live::LiveRoadMesh,
        ));
        extra(&mut e);
    }
    (meshes, tris)
}

/// The tile a point falls in.
pub(super) fn tile_of(p: Vec3) -> (i32, i32) {
    ((p.x / TILE).floor() as i32, (p.z / TILE).floor() as i32)
}

/// Draws `open.furniture`'s rails, blocks, signs and posts (nothing in headless runs).
pub fn spawn(world: &mut World, open: &OpenWorld) {
    if !super::visual(world) {
        return;
    }
    let old: Vec<Entity> = world.query_filtered::<Entity, With<FurnitureArt>>().iter(world).collect();
    for e in old {
        world.entity_mut(e).despawn();
    }
    let t = std::time::Instant::now();
    let mat = material(world);
    let f: &Furniture = &open.furniture;
    let mut tiles: BTreeMap<(i32, i32), Mesher> = BTreeMap::new();
    for r in &f.rails {
        let road = &open.net.roads[r.road as usize].road;
        let i0 = (r.from / crate::stage::STEP).round() as usize;
        let side = r.side as f32;
        let to_road = |k: usize| {
            let d = road.dir[(i0 + k).min(road.len() - 1)];
            // Left of travel is (d.y, −d.x); the road is the other way from the rail's side.
            -Vec3::new(d.y, 0.0, -d.x) * side
        };
        // A run goes in the tile of its middle.
        rail_mesh(tiles.entry(tile_of(r.pts[r.pts.len() / 2])).or_default(), r, &to_road);
    }
    for &(p, radius) in &f.blocks {
        let rot = Quat::from_rotation_y(jitter(p, 7) * core::f32::consts::TAU);
        let h = Vec3::new(radius * 1.1, 0.35 + jitter(p, 8) * 0.15, radius * 0.9);
        tiles.entry(tile_of(p)).or_default().cuboid(p + Vec3::Y * (h.y - 0.15), rot, h, [sw4(Swatch::Stone); 6], WHITE);
    }
    for s in &f.signs {
        sign_mesh(tiles.entry(tile_of(s.pos)).or_default(), s.kind, s.pos, s.yaw);
    }
    let (meshes, tris) = spawn_tiles(world, tiles, mat, "roadside furniture", |_| {});
    info!(
        "signs: {:.1} km of rail, {} blocks, {} signs and posts in {meshes} meshes, {tris} triangles ({:.0} ms)",
        f.rail_length() / 1000.0,
        f.blocks.len(),
        f.signs.len(),
        t.elapsed().as_secs_f32() * 1000.0
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cell_fits_the_atlas_without_overlap() {
        let mut used = vec![false; COLS * COLS];
        for c in all_cells() {
            let (k, w) = c.index();
            assert!(k % COLS + w <= COLS, "{c:?} runs off its row");
            for j in k..k + w {
                assert!(!used[j], "{c:?} overlaps cell {j}");
                used[j] = true;
            }
        }
        for v in SPEEDS {
            assert_eq!(Cell::Speed(v).index().0, 17 + SPEEDS.iter().position(|&s| s == v).unwrap());
        }
    }

    /// Writes the atlas as a PPM to look at: `ATLAS_OUT=/path/atlas.ppm cargo test write_atlas -- --ignored`.
    #[test]
    #[ignore]
    fn write_atlas() {
        let (img, _) = atlas();
        let data = img.data.unwrap();
        let mut out = format!("P6 {ATLAS} {ATLAS} 255\n").into_bytes();
        for p in data[..ATLAS * ATLAS * 4].chunks(4) {
            out.extend_from_slice(&p[..3]);
        }
        let path = std::env::var("ATLAS_OUT").unwrap_or_else(|_| std::env::temp_dir().join("atlas.ppm").display().to_string());
        std::fs::write(&path, out).unwrap();
        eprintln!("wrote {path}");
    }
}
