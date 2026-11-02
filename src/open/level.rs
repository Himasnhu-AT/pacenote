//! The open world as a level file (engine docs/EDITOR.md "Bake"). `pacenote --bake-world
//! <file>` writes it once from the generated world; from then on the level is the source of
//! truth and the editor (`--editor`) changes it.
//!
//! What goes where:
//! - each road: a level entity with a [`Spline`] (control points a few metres apart in bends,
//!   up to [`MAX_SPAN`] on straights; `tags` = surface and kind, `bank` in radians) and a
//!   [`RoadSpline`] (kind, speed limit). Roads are numbered in stable-id order: events and
//!   grades refer to roads by that number. Loading samples each spline every `stage::STEP` m.
//! - home, event starts, grade notes: the placeables of `placeable.rs`.
//! - the rest of [`WorldData`] (seed, town, time of day, events, town lots): [`WorldDoc`].
//! - terrain heights: `terrain.ron` + `terrain.r16` in the folder [`WorldDoc::terrain`] names
//!   (relative to the level file).
//! - the trees and the dressing (rocks, bushes): one entity each with empty `PcgOverrides`
//!   and a [`Foliage`] marker; the game adds the `PcgVolume` when it builds the world.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use engine::level::{LevelEntity, new_stable_id};
use engine::pcg::PcgOverrides;
use engine::spline::{Spline, SplineCache, SplineMode, SplinePoint};
use engine::world::terrain_data::TerrainData;

use super::data::{self, Home, RoadData, RoadPoint, WorldData};
use super::placeable::{self, PointsSpawned};
use super::roads::{RoadKind, RoadNet};
use crate::stage::{STEP, TimeOfDay};

/// The longest gap (m) between two control points of a baked road.
pub const MAX_SPAN: f32 = 40.0;
/// How much a road may turn (radians) between two control points.
const MAX_TURN: f32 = 0.25;
/// How far width (m) and crossfall may stray from a straight line between control points.
const WIDTH_TOL: f32 = 0.05;
const BANK_TOL: f32 = 0.004;
/// How far (m) the road's height may stray from a straight line between control points.
const HEIGHT_TOL: f32 = 0.05;
/// How far (m, 3D) a station may be from the baked curve.
const CURVE_TOL: f32 = 0.03;

/// A road of the open world: the [`Spline`] on the same entity is its centreline.
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct RoadSpline {
    pub kind: RoadKind,
    /// Traffic speed limit (km/h); `None`: by kind.
    pub speed_kmh: Option<f32>,
}

impl Default for RoadSpline {
    fn default() -> Self {
        RoadSpline { kind: RoadKind::Country, speed_kmh: None }
    }
}

/// What of the world data has no entity of its own. `data.roads` is empty (the splines hold
/// them); `data.home` and `data.grades` are only a fallback (the placeables hold them).
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct WorldDoc {
    pub data: WorldData,
    /// The terrain folder, relative to the level file.
    pub terrain: String,
}

impl Default for WorldDoc {
    fn default() -> Self {
        let data = WorldData {
            version: data::VERSION,
            seed: super::SEED,
            town_y: 0.0,
            time: TimeOfDay::Afternoon,
            home: Home { pos: Vec3::ZERO, dir: Vec2::X },
            roads: Vec::new(),
            events: Vec::new(),
            grades: Vec::new(),
            lots: Vec::new(),
        };
        WorldDoc { data, terrain: String::new() }
    }
}

/// Which generated scenery a `PcgOverrides` entity edits.
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[reflect(Component, Default)]
pub enum Foliage {
    #[default]
    Trees,
    /// Rocks, bushes, ground cover.
    Dressing,
}

pub struct LevelTypesPlugin;

impl Plugin for LevelTypesPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<RoadSpline>().register_type::<WorldDoc>().register_type::<Foliage>();
    }
}

/// The stations of `r` kept as control points: every surface change, and as few others as
/// keep the turn, the width and the crossfall between two of them within bounds.
fn control_points(r: &RoadData) -> Vec<usize> {
    let p = &r.points;
    let n = p.len();
    let heading = |i: usize| {
        let (a, b) = if i + 1 < n { (p[i].pos, p[i + 1].pos) } else { (p[i - 1].pos, p[i].pos) };
        libm::atan2f(b.z - a.z, b.x - a.x)
    };
    let turn = |a: f32, b: f32| {
        let d = (b - a).rem_euclid(std::f32::consts::TAU);
        d.min(std::f32::consts::TAU - d)
    };
    let fits = |i: usize, j: usize| {
        let span: f32 = (i..j).map(|k| p[k].pos.distance(p[k + 1].pos)).sum();
        if span > MAX_SPAN {
            return false;
        }
        let h0 = heading(i);
        for k in i + 1..j {
            let t = (k - i) as f32 / (j - i) as f32;
            let lerp = |a: f32, b: f32| a + (b - a) * t;
            if p[k].surface != p[i].surface
                || turn(h0, heading(k)) > MAX_TURN
                || (p[k].width - lerp(p[i].width, p[j].width)).abs() > WIDTH_TOL
                || (p[k].bank - lerp(p[i].bank, p[j].bank)).abs() > BANK_TOL
                || (p[k].pos.y - lerp(p[i].pos.y, p[j].pos.y)).abs() > HEIGHT_TOL
            {
                return false;
            }
        }
        turn(h0, heading(j)) <= MAX_TURN
    };
    let mut keep = vec![0];
    let mut i = 0;
    while i + 1 < n {
        let mut j = i + 1;
        while j + 1 < n && fits(i, j + 1) {
            j += 1;
        }
        keep.push(j);
        i = j;
    }
    keep
}

/// Road `r` as a spline: Catmull-Rom through [`control_points`], with a station added in the
/// middle of any span where the curve passes more than [`CURVE_TOL`] from a station.
pub fn road_spline(r: &RoadData) -> (Spline, RoadSpline) {
    let make = |keep: &[usize]| Spline {
        points: keep
            .iter()
            .map(|&i| {
                let q = &r.points[i];
                SplinePoint { pos: q.pos, width: q.width, bank: q.bank.atan(), tags: data::tags(r.kind, q.surface), data: None }
            })
            .collect(),
        closed: r.closed,
        mode: SplineMode::Catmull,
    };
    let mut keep = control_points(r);
    for _ in 0..8 {
        let spline = make(&keep);
        let cache = SplineCache::build(&spline);
        let mut split = Vec::new();
        for m in 0..keep.len() - 1 {
            let (a, b) = (keep[m], keep[m + 1]);
            if b - a >= 2 && (a + 1..b).any(|k| cache.nearest(&spline, r.points[k].pos).is_none_or(|n| n.2 > CURVE_TOL)) {
                split.push((a + b) / 2);
            }
        }
        if split.is_empty() {
            break;
        }
        keep.extend(split);
        keep.sort_unstable();
    }
    (make(&keep), RoadSpline { kind: r.kind, speed_kmh: r.speed_kmh })
}

/// The road a spline describes: a station every `stage::STEP` m (the last one at the end of an
/// open spline). `at` places the spline (its entity's transform).
pub fn spline_road(spline: &Spline, road: &RoadSpline, at: &Transform) -> RoadData {
    let cache = SplineCache::build(spline);
    let mut points: Vec<RoadPoint> = cache
        .samples(spline, STEP)
        .map(|s| RoadPoint { pos: at.transform_point(s.pos), width: s.width, bank: s.bank.tan(), surface: data::from_tags(s.tags).1 })
        .collect();
    // An open spline ends with a short last step: merge it into the one before.
    let n = points.len();
    if !spline.is_loop() && n >= 3 && points[n - 1].pos.distance(points[n - 2].pos) < 0.5 * STEP {
        points.remove(n - 2);
    }
    RoadData { kind: road.kind, closed: spline.is_loop(), speed_kmh: road.speed_kmh, points }
}

/// The station of `road` nearest `p` (XZ).
fn nearest_station(road: &RoadData, p: Vec3) -> u32 {
    let d = |q: &RoadPoint| Vec2::new(q.pos.x - p.x, q.pos.z - p.z).length_squared();
    road.points.iter().enumerate().min_by(|a, b| d(a.1).total_cmp(&d(b.1))).map_or(0, |(i, _)| i as u32)
}

/// `data` as the level will hold it, and its roads' splines: every road through its spline and
/// back (stations move a few centimetres and renumber), events and grades moved to the nearest
/// new station.
pub fn through_splines(data: &WorldData) -> (WorldData, Vec<(Spline, RoadSpline)>) {
    let mut out = data.clone();
    let splines: Vec<(Spline, RoadSpline)> = data.roads.iter().map(road_spline).collect();
    out.roads = splines.iter().map(|(s, info)| spline_road(s, info, &Transform::IDENTITY)).collect();
    let at = |road: u32, station: u32| data.roads[road as usize].points[(station as usize).min(data.roads[road as usize].points.len() - 1)].pos;
    for (e, old) in out.events.iter_mut().zip(&data.events) {
        let new = &out.roads[old.road as usize];
        e.from = nearest_station(new, at(old.road, old.from));
        e.to = nearest_station(new, at(old.road, old.to));
    }
    for (g, old) in out.grades.iter_mut().zip(&data.grades) {
        g.station = nearest_station(&out.roads[old.road as usize], at(old.road, old.station));
    }
    (out, splines)
}

/// Spawns `data` and its road `splines` (from [`through_splines`]) as level entities in
/// `world`, which needs the engine's `LevelPlugin`. Roads get the first stable ids, in order.
pub fn spawn_level(world: &mut World, data: &WorldData, splines: &[(Spline, RoadSpline)], net: &RoadNet, terrain: &str) {
    let level = |world: &mut World, e: Entity| {
        let id = new_stable_id(world);
        world.entity_mut(e).insert((LevelEntity { id }, Visibility::default()));
    };
    for (k, (spline, info)) in splines.iter().enumerate() {
        let e = world.spawn((Name::new(format!("road {k} ({:?})", info.kind)), spline.clone(), *info, Transform::IDENTITY)).id();
        level(world, e);
    }
    let doc = WorldDoc { data: WorldData { roads: Vec::new(), grades: Vec::new(), ..data.clone() }, terrain: terrain.into() };
    let e = world.spawn((Name::new("open world"), doc, Transform::IDENTITY)).id();
    level(world, e);
    for e in placeable::spawn(world, data, net) {
        level(world, e);
    }
    for (name, f) in [("trees", Foliage::Trees), ("dressing", Foliage::Dressing)] {
        let e = world.spawn((Name::new(name), f, PcgOverrides::default(), Transform::IDENTITY)).id();
        level(world, e);
    }
}

/// The world data a loaded level describes: the doc, roads from the splines (in stable-id
/// order), then home, event starts and grades from the placeables. Checked with
/// [`WorldData::validate`].
pub fn from_level(world: &mut World) -> Result<WorldData, String> {
    let mut q = world.query::<&WorldDoc>();
    let mut docs = q.iter(world);
    let doc = docs.next().ok_or("the level has no WorldDoc")?.clone();
    if docs.next().is_some() {
        return Err("the level has more than one WorldDoc".into());
    }
    let mut q = world.query::<(&LevelEntity, &Spline, &RoadSpline, Option<&Transform>)>();
    let mut roads: Vec<_> = q.iter(world).map(|(l, s, r, t)| (l.id.0, spline_road(s, r, &t.copied().unwrap_or_default()))).collect();
    roads.sort_by_key(|r| r.0);
    let data = WorldData { roads: roads.into_iter().map(|r| r.1).collect(), ..doc.data };
    data.validate()?;
    let net = data.net();
    world.insert_resource(PointsSpawned::default());
    let data = placeable::gather(world, &data, &net);
    data.validate()?;
    Ok(data)
}

/// The terrain folder of the level at `level` (`WorldDoc::terrain` next to it).
pub fn terrain_dir(level: &Path, terrain: &str) -> PathBuf {
    level.parent().unwrap_or(Path::new(".")).join(terrain)
}

/// A headless app with what saving and loading the open world's level needs.
pub fn level_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::world_serialization::WorldSerializationPlugin))
        .register_type::<Name>()
        .register_type::<Transform>()
        .register_type::<Visibility>()
        .register_type::<ChildOf>()
        .register_type::<Spline>()
        .register_type::<PcgOverrides>()
        .add_plugins((engine::level::LevelPlugin, placeable::PlaceablePlugin, LevelTypesPlugin));
    app
}

/// `pacenote --bake-world <file>`: the generated world as a level, with its terrain in the
/// folder of the same name. Prints what it wrote.
pub fn bake(path: &Path) -> Result<String, String> {
    let t = std::time::Instant::now();
    let (data, splines) = through_splines(&WorldData::generate(super::SEED));
    let open = super::OpenWorld::from_data(data.clone())?;
    let name = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".level.ron")).ok_or("the level file must end in .level.ron")?;
    let mut app = level_app();
    let world = app.world_mut();
    spawn_level(world, &data, &splines, &open.net, name);
    let report = engine::level::save_level(world, path).map_err(|e| format!("{e:?}"))?;
    if !report.skipped.is_empty() {
        return Err(format!("components not saved: {:?}", report.skipped));
    }
    let n = open.heights.nx();
    let dir = terrain_dir(path, name);
    TerrainData::from_heightfield(&open.heights, [n, open.heights.nz()], None).save(&dir).map_err(|e| format!("{e:?}"))?;
    let points: usize = world.query::<&Spline>().iter(world).map(|s| s.points.len()).sum();
    Ok(format!(
        "{}: {} roads ({points} control points), {} events, terrain {n}² in {} ({:.1} s)",
        path.display(),
        data.roads.len(),
        data.events.len(),
        dir.display(),
        t.elapsed().as_secs_f32()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Through the splines, every road stays within 8 cm of where it was (centreline and
    /// height), the junctions and events are the same, and the control points are few.
    #[test]
    fn roads_survive_the_splines() {
        let data = WorldData::generate(super::super::SEED);
        let (baked, _) = through_splines(&data);
        baked.validate().unwrap();
        let (a, b) = (data.net(), baked.net());
        let mut controls = 0;
        let mut stations = 0;
        let (mut worst_xz, mut worst_y) = (0.0f32, 0.0f32);
        for (k, (ra, rb)) in a.roads.iter().zip(&b.roads).enumerate() {
            controls += road_spline(&data.roads[k]).0.points.len();
            stations += ra.road.len();
            let seam = ra.road.pts[0];
            for p in &ra.road.pts {
                let q = Vec2::new(p.x, p.z);
                // The generated ring steps 0.6 m in height where its ends meet; the spline
                // smooths that out.
                if data.roads[k].closed && q.distance(Vec2::new(seam.x, seam.z)) < 12.0 {
                    continue;
                }
                let at = rb.road.nearest(q, 5.0).unwrap_or_else(|| panic!("road {k}: {q} left the road"));
                let i = at.i;
                let on = rb.road.pts[i].lerp(rb.road.pts[(i + 1).min(rb.road.len() - 1)], at.t);
                worst_xz = worst_xz.max(Vec2::new(on.x, on.z).distance(q));
                worst_y = worst_y.max((on.y - p.y).abs());
                assert!(Vec2::new(on.x, on.z).distance(q) < 0.08, "road {k}: {q} moved {}", Vec2::new(on.x, on.z).distance(q));
                assert!((on.y - p.y).abs() < 0.08, "road {k}: {q} height {} vs {}", on.y, p.y);
            }
        }
        eprintln!("{controls} control points for {stations} stations; worst move {worst_xz:.3} m, height {worst_y:.3} m");
        assert!(controls * 4 < stations, "{controls} control points for {stations} stations");
        assert_eq!(a.junctions.len(), b.junctions.len());
        let (ea, eb) = (super::super::events::build(&a, &data.events), super::super::events::build(&b, &baked.events));
        assert_eq!(ea.len(), eb.len());
        for (x, y) in ea.iter().zip(&eb) {
            // Start and finish each snap to the nearest new station (up to a station apart); the
            // curve measures a little shorter than the generated polyline (0.15 % on the ring).
            assert!((x.route.length() - y.route.length()).abs() <= (2.0 * STEP + 1.0).max(0.002 * x.route.length()), "{}: {} vs {}", x.name, x.route.length(), y.route.length());
        }
    }

    /// Spawned as a level, saved, loaded into a fresh world: the same data.
    #[test]
    fn level_round_trip() {
        let (data, splines) = through_splines(&WorldData::generate(super::super::SEED));
        let net = data.net();
        let dir = std::env::temp_dir().join(format!("pacenote-level-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("open.level.ron");
        let mut app = level_app();
        spawn_level(app.world_mut(), &data, &splines, &net, "open");
        let report = engine::level::save_level(app.world_mut(), &path).unwrap();
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        let mut app = level_app();
        engine::level::load_level(app.world_mut(), &path).unwrap();
        let back = from_level(app.world_mut()).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(back.roads.len(), data.roads.len());
        for (k, (a, b)) in back.roads.iter().zip(&data.roads).enumerate() {
            assert_eq!((a.kind, a.closed, a.speed_kmh, a.points.len()), (b.kind, b.closed, b.speed_kmh, b.points.len()), "road {k}");
            for (i, (p, q)) in a.points.iter().zip(&b.points).enumerate() {
                assert_eq!(p, q, "road {k} station {i}");
            }
        }
        assert_eq!(back.events, data.events);
        assert_eq!(back.lots, data.lots);
        assert_eq!((back.seed, back.town_y, back.time), (data.seed, data.town_y, data.time));
        assert!(back.home.pos.distance(data.home.pos) < 1e-3);
    }
}
