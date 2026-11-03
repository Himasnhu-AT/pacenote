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
use engine::level::RegisterPlaceable;
use engine::level::{LevelEntity, new_stable_id};
use engine::pcg::PcgOverrides;
use engine::spline::{Spline, SplineCache, SplineMode, SplinePoint};
use engine::world::terrain_data::TerrainData;

use super::data::{self, Home, RoadData, RoadPoint, WorldData};
use super::placeable::{self, PointsSpawned};
use super::buildings;
use super::roads::{RoadKind, RoadNet};
use crate::stage::{STEP, TimeOfDay};

/// The longest gap (m) between two control points of a baked road.
pub const MAX_SPAN: f32 = 40.0;
/// How much a road may turn (radians) between two control points.
const MAX_TURN: f32 = 0.25;
/// How far width (m) and crossfall may stray from a straight line between control points.
const WIDTH_TOL: f32 = 0.05;
const BANK_TOL: f32 = 0.02;
/// How far (m) the road's height may stray from a straight line between control points.
const HEIGHT_TOL: f32 = 0.05;
/// How far (m) a station may be from the baked curve: sideways, and in height (tighter: the
/// road surface has to meet the baked terrain).
const CURVE_TOL: f32 = 0.10;
const CURVE_TOL_Y: f32 = 0.03;

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
        RoadSpline { kind: RoadKind::Gravel, speed_kmh: None }
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
            buildings: Vec::new(),
        };
        WorldDoc { data, terrain: String::new() }
    }
}

/// A building. Its `Transform` places it: translation = the footprint centre at floor level,
/// rotation = which way it faces (about +Y). The game builds its mesh as a child.
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct Building {
    pub kind: buildings::Use,
    /// Footprint width (along its local x) and depth (m).
    pub size: Vec2,
    pub floors: u32,
    /// Storey height (m).
    pub floor_height: f32,
    pub roof: buildings::Roof,
    /// Roof slope (rise per metre) of pitched roofs.
    pub roof_pitch: f32,
    /// The side facing the street: 0 = +x, 1 = +z, 2 = −x, 3 = −z (local).
    pub front: u8,
    /// Facade colour 0..=9, roof colour 0..=4.
    pub wall_colour: u8,
    pub roof_colour: u8,
    /// Small variations (balconies, rooftop boxes, signs).
    pub variant: u32,
}

impl Default for Building {
    fn default() -> Self {
        Building { kind: buildings::Use::House, size: Vec2::new(10.0, 8.0), floors: 2, floor_height: 3.0, roof: buildings::Roof::Gable, roof_pitch: 0.7, front: 3, wall_colour: 0, roof_colour: 0, variant: 0 }
    }
}

impl Building {
    /// The component and transform of a laid-out building.
    pub fn from_layout(b: &buildings::Building) -> (Building, Transform) {
        let c = Building {
            kind: b.use_,
            size: b.half * 2.0,
            floors: b.floors,
            floor_height: b.floor_h,
            roof: b.roof,
            roof_pitch: b.pitch,
            front: b.front,
            wall_colour: b.wall,
            roof_colour: b.roof_col,
            variant: b.bits,
        };
        let t = Transform::from_xyz(b.centre.x, b.ground, b.centre.y).with_rotation(Quat::from_rotation_y(libm::atan2f(-b.axis.y, b.axis.x)));
        (c, t)
    }

    /// The laid-out building this describes at `t` (values clamped to what the game builds).
    pub fn to_layout(&self, t: &Transform) -> buildings::Building {
        let x = t.rotation * Vec3::X;
        let axis = Vec2::new(x.x, x.z).normalize_or(Vec2::X);
        let ground = t.translation.y;
        buildings::Building {
            centre: Vec2::new(t.translation.x, t.translation.z),
            axis,
            half: (self.size * 0.5).clamp(Vec2::splat(1.0), Vec2::splat(200.0)),
            front: self.front.min(3),
            floors: self.floors.clamp(1, 60),
            floor_h: self.floor_height.clamp(2.0, 10.0),
            ground,
            base: ground,
            roof: self.roof,
            pitch: self.roof_pitch.clamp(0.0, 2.0),
            use_: self.kind,
            wall: self.wall_colour.min(buildings::WALLS.len() as u8 - 1),
            roof_col: self.roof_colour.min(buildings::ROOFS.len() as u8 - 1),
            bits: self.variant,
        }
    }
}

/// The mesh child of a [`Building`] entity.
#[derive(Component)]
pub struct BuildingShape;

/// Builds the mesh child of every level [`Building`] that is new or changed, and drops the
/// loose (generated) building meshes once the level's buildings exist.
#[allow(clippy::type_complexity)]
fn sync_buildings(world: &mut World) {
    let mut q = world.query_filtered::<(Entity, &Building, &Transform, Option<&Children>), (With<LevelEntity>, Or<(Changed<Building>, Changed<Transform>)>)>();
    let todo: Vec<(Entity, Building, Transform, Vec<Entity>)> = q.iter(world).map(|(e, b, t, c)| (e, *b, *t, c.map(|c| c.iter().collect()).unwrap_or_default())).collect();
    let any = world.query_filtered::<(), (With<Building>, With<LevelEntity>)>().iter(world).next().is_some();
    if any {
        let loose: Vec<Entity> = world.query_filtered::<Entity, With<buildings::LooseBuilding>>().iter(world).collect();
        for e in loose {
            world.entity_mut(e).despawn();
        }
    }
    if todo.is_empty() || !crate::art::visual(world) {
        return;
    }
    debug!("buildings: {} meshes to build", todo.len());
    let mat = buildings::material(world);
    let terrain = world.get_resource::<engine::world::Terrain>().map(|t| t.heightfield().clone());
    for (e, b, t, children) in todo {
        for c in children {
            if world.get::<BuildingShape>(c).is_some() {
                world.entity_mut(c).despawn();
            }
        }
        let lb = b.to_layout(&t);
        let mesh = match &terrain {
            Some(h) => buildings::building_mesh(&lb, &|x, z| h.height_at(x, z)),
            None => buildings::building_mesh(&lb, &|_, _| lb.ground),
        };
        let aabb = bevy::camera::primitives::MeshAabb::get_aabb(&mesh);
        let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
        // The mesh is in world axes about the centre: undo the parent's rotation.
        let local = Transform::from_rotation(t.rotation.inverse());
        let mut c = world.spawn((Name::new("building mesh"), BuildingShape, Mesh3d(h), MeshMaterial3d(mat.clone()), local, engine::prelude::Scenery, engine::prelude::KeepDetail, ChildOf(e)));
        if let Some(a) = aabb {
            c.insert((a, bevy::camera::visibility::NoAutoAabb));
        }
        if !world.entity(e).contains::<Visibility>() {
            world.entity_mut(e).insert(Visibility::default());
        }
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
        app.register_type::<RoadSpline>()
            .register_type::<WorldDoc>()
            .register_type::<Foliage>()
            .register_type::<Building>()
            .register_placeable::<Building>("Building", "building", "Pacenote / Buildings")
            .add_systems(PreStartup, pick_source)
            .add_systems(PostUpdate, sync_buildings.before(bevy::transform::TransformSystems::Propagate))
            .add_systems(Update, adopt_volumes);
        #[cfg(feature = "dev")]
        app.add_systems(PostUpdate, hide_game_ui.run_if(resource_exists::<engine::editor::EditorMode>));
    }
}

/// The level the open world loads: `--level <file>` (the editor's Play passes its copy), else
/// the shipped one under the assets folder.
fn pick_source(config: Option<Res<engine::level::LevelConfig>>, assets: Option<Res<engine::paths::AssetsDir>>) {
    let assets_path = |p: &Path| assets.as_ref().map_or_else(|| p.to_path_buf(), |a| a.0.join(p));
    let path = match config.and_then(|c| c.startup.clone()) {
        Some(p) if p.is_absolute() || p.exists() => p,
        Some(p) => assets_path(&p),
        None => assets_path(Path::new(DEFAULT_LEVEL)),
    };
    info!("open world level: {}", path.display());
    set_source(path);
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
            let off = |k: usize| {
                let p = r.points[k].pos;
                cache.nearest(&spline, p).is_none_or(|n| Vec2::new(n.1.pos.x - p.x, n.1.pos.z - p.z).length() > CURVE_TOL || (n.1.pos.y - p.y).abs() > CURVE_TOL_Y)
            };
            if b - a >= 2 && (a + 1..b).any(off) {
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
    let new_roads = out.roads.clone();
    for e in out.events.iter_mut() {
        e.remap(|road, station| nearest_station(&new_roads[road as usize], at(road, station)));
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
    let doc = WorldDoc { data: WorldData { roads: Vec::new(), grades: Vec::new(), buildings: Vec::new(), ..data.clone() }, terrain: terrain.into() };
    let e = world.spawn((Name::new("open world"), doc, Transform::IDENTITY)).id();
    level(world, e);
    for e in placeable::spawn(world, data, net) {
        level(world, e);
    }
    for (k, b) in data.buildings.iter().enumerate() {
        let (c, t) = Building::from_layout(b);
        let e = world.spawn((Name::new(format!("{} {k}", b.use_.label())), c, t)).id();
        level(world, e);
    }
    for (name, f) in [("Trees", Foliage::Trees), ("Dressing", Foliage::Dressing)] {
        let e = world.spawn((Name::new(name), f, PcgOverrides::default(), engine::pcg::PcgFollowTerrain, Transform::IDENTITY)).id();
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
    let mut q = world.query::<(&LevelEntity, &Building, Option<&Transform>)>();
    let mut built: Vec<_> = q.iter(world).map(|(l, b, t)| (l.id.0, b.to_layout(&t.copied().unwrap_or_default()))).collect();
    built.sort_by_key(|b| b.0);
    let data = WorldData { roads: roads.into_iter().map(|r| r.1).collect(), buildings: built.into_iter().map(|b| b.1).collect(), ..doc.data };
    data.validate()?;
    let net = data.net();
    world.insert_resource(PointsSpawned::default());
    let mut data = placeable::gather(world, &data, &net);
    // An older level's grade notes are read in its own format, then brought up to date.
    data.migrate();
    data.validate()?;
    Ok(data)
}

/// Moves the generated PCG volume `v` (the trees or the dressing) onto the level's [`Foliage`]
/// entity of its kind, if the level has one, so the editor's Foliage tool edits the level's
/// `PcgOverrides` (the level entity's own overrides replace the copy `v` carries).
pub fn adopt_volume(world: &mut World, v: Entity) {
    use crate::art::{DressingVolume, trees::TreeVolume};
    use engine::pcg::{PcgStreaming, PcgVolume};
    let Ok(e) = world.get_entity(v) else { return };
    if e.contains::<LevelEntity>() || !e.contains::<PcgVolume>() {
        return;
    }
    let kind = if e.contains::<TreeVolume>() {
        Foliage::Trees
    } else if e.contains::<DressingVolume>() {
        Foliage::Dressing
    } else {
        return;
    };
    let Some(target) = world.query_filtered::<(Entity, &Foliage), With<LevelEntity>>().iter(world).find(|(_, f)| **f == kind).map(|(t, _)| t) else { return };
    let mut e = world.entity_mut(v);
    let volume = e.take::<PcgVolume>();
    let streaming = e.take::<PcgStreaming>();
    e.despawn();
    let mut t = world.entity_mut(target);
    if let Some(volume) = volume {
        t.insert(volume);
    }
    if let Some(s) = streaming {
        t.insert(s);
    }
    match kind {
        Foliage::Trees => t.insert(TreeVolume),
        Foliage::Dressing => t.insert(DressingVolume),
    };
    if !t.contains::<PcgOverrides>() {
        t.insert(PcgOverrides::default());
    }
    // Trees and rocks follow sculpting (and the ground re-shaped around a moved road).
    if !t.contains::<engine::pcg::PcgFollowTerrain>() {
        t.insert(engine::pcg::PcgFollowTerrain);
    }
    info!("level: the {kind:?} volume is on the level's foliage entity");
}

/// In the editor (`--editor`) the game's own screen UI (HUD, plates, countdown) stays hidden:
/// the editor draws with egui, every Bevy UI root is the game's.
#[cfg(feature = "dev")]
fn hide_game_ui(mut roots: Query<&mut Visibility, (With<bevy::ui::Node>, Without<ChildOf>)>) {
    for mut v in &mut roots {
        if *v != Visibility::Hidden {
            *v = Visibility::Hidden;
        }
    }
}

/// A level loaded after the stage was built (`--editor --level`, a hot reload): its foliage
/// entities take over the generated volumes.
fn adopt_volumes(world: &mut World) {
    use engine::pcg::PcgVolume;
    let waiting = world.query_filtered::<(), (With<Foliage>, With<LevelEntity>, Without<PcgVolume>)>().iter(world).next().is_some();
    if !waiting {
        return;
    }
    let generated: Vec<Entity> = world.query_filtered::<Entity, (With<PcgVolume>, Without<LevelEntity>)>().iter(world).collect();
    for v in generated {
        adopt_volume(world, v);
    }
}

/// Is the level at `path` an open world (does it hold a [`WorldDoc`])? Relative paths are
/// tried as given, then under the assets folder.
pub fn is_open_world(path: &Path) -> bool {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(path);
    [path.to_path_buf(), assets].iter().any(|p| std::fs::read_to_string(p).is_ok_and(|t| t.contains("\"pacenote::open::level::WorldDoc\"")))
}

/// The terrain folder of the level at `level` (`WorldDoc::terrain` next to it).
pub fn terrain_dir(level: &Path, terrain: &str) -> PathBuf {
    level.parent().unwrap_or(Path::new(".")).join(terrain)
}

/// The level the open world loads from: `--level <file>` (set at startup), else
/// `assets/levels/open.level.ron`.
static SOURCE: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

/// The shipped level, under the assets folder.
pub const DEFAULT_LEVEL: &str = "levels/open.level.ron";

pub fn set_source(path: PathBuf) {
    *SOURCE.write().unwrap_or_else(|e| e.into_inner()) = Some(path);
}

pub fn source() -> PathBuf {
    SOURCE.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(DEFAULT_LEVEL))
}

/// Biggest level file read (the baked world is 0.65 MB).
const MAX_LEVEL_BYTES: u64 = 64 << 20;

/// What a level file gives the open world.
pub struct Loaded {
    pub data: WorldData,
    pub heights: engine::world::Heightfield,
    /// Where its terrain files are (sculpts are saved back there).
    pub terrain: PathBuf,
    /// Hand edits of the trees and of the dressing.
    pub trees: PcgOverrides,
    pub dressing: PcgOverrides,
    /// Hash of the level file and its terrain heights: any edit changes it (ghosts and boards
    /// of the open world's events are keyed by it).
    pub content: u64,
}

/// Hash of `bytes` (stable across machines and runs).
fn hash_bytes(h: &mut crate::meta::board::Hasher, bytes: &[u8]) {
    for c in bytes.chunks(8) {
        let mut b = [0u8; 8];
        b[..c.len()].copy_from_slice(c);
        h.u64(u64::from_le_bytes(b));
    }
    h.u64(bytes.len() as u64);
}

/// Reads the open world from the level at `path`: world data ([`from_level`]) and terrain
/// heights. The terrain folder is looked for next to the level, then next to the level the
/// editor's Play copied it from (`--level-origin`), then next to the shipped level.
pub fn load(path: &Path) -> Result<Loaded, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if size > MAX_LEVEL_BYTES {
        return Err(format!("{}: {size} bytes (at most {MAX_LEVEL_BYTES})", path.display()));
    }
    let mut app = level_app();
    let world = app.world_mut();
    engine::level::load_level(world, path).map_err(|e| format!("{}: {e:?}", path.display()))?;
    let data = from_level(world)?;
    let name = world.query::<&WorldDoc>().iter(world).next().map(|d| d.terrain.clone()).unwrap_or_default();
    if name.is_empty() || name.contains("..") || Path::new(&name).is_absolute() {
        return Err(format!("terrain folder {name:?}"));
    }
    // Next to the level; for the editor's Play copy (written elsewhere), next to the level it
    // was copied from (`--level-origin`); else next to the shipped level.
    let origin = engine::cli::arg_value("level-origin").or_else(|| std::env::var("EDITOR_LEVEL_ORIGIN").ok()).map(PathBuf::from);
    let dir = [Some(path.to_path_buf()), origin, Some(source_default())]
        .into_iter()
        .flatten()
        .map(|l| terrain_dir(&l, &name))
        .find(|d| d.join("terrain.ron").exists())
        .unwrap_or_else(|| terrain_dir(path, &name));
    let heights = TerrainData::load(&dir).and_then(|t| t.to_heightfield()).map_err(|e| format!("terrain {}: {e:?}", dir.display()))?;
    let mut overrides = |kind: Foliage| world.query::<(&Foliage, &PcgOverrides)>().iter(world).find(|(f, _)| **f == kind).map(|(_, o)| o.clone()).unwrap_or_default();
    let (trees, dressing) = (overrides(Foliage::Trees), overrides(Foliage::Dressing));
    let mut h = crate::meta::board::Hasher::default();
    for f in [path.to_path_buf(), dir.join("terrain.ron"), dir.join("terrain.r16")] {
        hash_bytes(&mut h, &std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?);
    }
    Ok(Loaded { data, heights, terrain: dir, trees, dressing, content: h.0 })
}

fn source_default() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(DEFAULT_LEVEL)
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
        .register_type::<engine::pcg::PcgFollowTerrain>()
        .add_plugins((engine::level::LevelPlugin, placeable::PlaceablePlugin, LevelTypesPlugin));
    app
}

/// `pacenote --bake-world <file>`: the generated world as a level, with its terrain in the
/// folder of the same name. Prints what it wrote.
pub fn bake(path: &Path) -> Result<String, String> {
    let t = std::time::Instant::now();
    let (mut data, splines) = through_splines(&WorldData::generate(super::SEED));
    let open = super::OpenWorld::from_data(data.clone())?;
    // The buildings as the streets lay them out now; from here on they are data.
    data.buildings = open.buildings.buildings.clone();
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
        "{}: {} roads ({points} control points), {} events, {} buildings, terrain {n}² in {} ({:.1} s)",
        path.display(),
        data.roads.len(),
        data.events.len(),
        data.buildings.len(),
        dir.display(),
        t.elapsed().as_secs_f32()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Through the splines, every road stays within 12 cm of where it was sideways and 8 cm in
    /// height, the junctions and events are the same, and the control points are few.
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
                // (Within the fit's 10 cm, plus the chords of an 8 m hairpin sampled 2 m apart.)
                assert!(Vec2::new(on.x, on.z).distance(q) < 0.17, "road {k}: {q} moved {}", Vec2::new(on.x, on.z).distance(q));
                assert!((on.y - p.y).abs() < 0.08, "road {k}: {q} height {} vs {} (station {} of {}, matched {i} t {} of {}, lateral {})", on.y, p.y, ra.road.pts.iter().position(|x| x == p).unwrap(), ra.road.len(), at.t, rb.road.len(), at.lateral);
            }
        }
        eprintln!("{controls} control points for {stations} stations; worst move {worst_xz:.3} m, height {worst_y:.3} m");
        // Hill roads bend and climb all the time: about one control point per 7 m.
        assert!(controls * 3 < stations, "{controls} control points for {stations} stations");
        assert_eq!(a.junctions.len(), b.junctions.len());
        let (ea, eb) = (super::super::events::build(&a, &data.events), super::super::events::build(&b, &baked.events));
        assert_eq!(ea.len(), eb.len());
        for (x, y) in ea.iter().zip(&eb) {
            // Start and finish each snap to the nearest new station (up to a station apart); the
            // curve measures a little shorter than the generated polyline (0.2 % with hairpins).
            assert!((x.route.length() - y.route.length()).abs() <= (2.0 * STEP + 1.0).max(0.004 * x.route.length()), "{}: {} vs {}", x.name, x.route.length(), y.route.length());
        }
    }

    /// Spawned as a level, saved, loaded into a fresh world: the same data.
    #[test]
    fn level_round_trip() {
        let (mut data, splines) = through_splines(&WorldData::generate(super::super::SEED));
        let net = data.net();
        data.buildings = super::super::buildings::layout_with(&net, &data.lots).buildings;
        assert!(data.buildings.len() > 100, "{} buildings", data.buildings.len());
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
        assert_eq!(back.buildings.len(), data.buildings.len());
        for (a, b) in back.buildings.iter().zip(&data.buildings) {
            assert!(a.centre.distance(b.centre) < 1e-3 && a.axis.distance(b.axis) < 1e-4 && (a.ground - b.ground).abs() < 1e-4, "{a:?} vs {b:?}");
            assert_eq!((a.half, a.front, a.floors, a.floor_h, a.roof, a.pitch, a.use_, a.wall, a.roof_col, a.bits), (b.half, b.front, b.floors, b.floor_h, b.roof, b.pitch, b.use_, b.wall, b.roof_col, b.bits));
        }
        assert_eq!((back.seed, back.town_y, back.time), (data.seed, data.town_y, data.time));
        assert!(back.home.pos.distance(data.home.pos) < 1e-3);
    }

    /// Every level shipped in assets/levels loads (a bad one would fall back to the generator
    /// at start, silently).
    #[test]
    fn committed_levels_load() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/levels");
        let mut n = 0;
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.to_string_lossy().ends_with(".level.ron") {
                let l = load(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                assert!(!l.data.roads.is_empty(), "{}", p.display());
                assert!(!l.data.events.is_empty(), "{}: no events", p.display());
                n += 1;
            }
        }
        assert!(n >= 1);
    }
}
