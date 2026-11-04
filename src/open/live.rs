//! Live editing of the open world in the editor (`--editor`): a moved road point, a moved event
//! start, home or grade note, or an edited `WorldDoc` / `RoadSpline` shows at once.
//!
//! Each change is noted ([`watch`]); once nothing has changed for [`SETTLE`] seconds the
//! world is rebuilt from the level entities ([`refresh`]):
//! - the world data again ([`super::level::from_level`]); event finishes on an edited road
//!   follow it (moved to the nearest station of the new road) and are written back to the
//!   `WorldDoc`, so the next save keeps them;
//! - the ground around the part of a road that moved is shaped again (`TerrainEdit`): within
//!   the road's reach of the old and of the new course, heights become the generator's hills
//!   shaped by the new roads; everything else (sculpts) stays;
//! - road ribbons, verges, patches and junctions; buildings when a street or the lots changed;
//!   event markers; the trees' and dressing's inputs (only the cells near the change
//!   re-evaluate); traffic lanes.
//!
//! Undo works through the same path: undoing a spline edit changes it back, and the ground
//! around it is shaped for the old course again.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::prelude::*;
use engine::level::LevelEntity;
use engine::spline::Spline;
use engine::world::brush::SampleRect;
use engine::world::{Terrain, TerrainEdit};

use super::data::WorldData;
use super::level::{RoadSpline, WorldDoc};
use super::placeable::{EventStart, GradeNote, HomePoint};
use super::roads::RoadNet;
use crate::world::StageWorld;

/// Seconds without a change before the world is rebuilt (a drag sends many).
const SETTLE: f32 = 0.3;

/// Road meshes (ribbons, verges, patches, junctions), rebuilt on a refresh.
#[derive(Component)]
pub struct LiveRoadMesh;

/// Town and farm buildings, rebuilt on a refresh that changes streets or lots.
#[derive(Component)]
pub struct LiveBuilding;

#[derive(Resource, Default)]
struct Live {
    /// Each road's spline as last rebuilt from, by stable id (to find what moved; a level
    /// opened again has new entities with the same ids and splines: nothing moved).
    splines: HashMap<u64, Spline>,
    /// Stable id of each road entity (to tell a deleted road from a reloaded one).
    ids: HashMap<Entity, u64>,
    /// Areas (XZ min, max) whose ground needs shaping again.
    reshape: Vec<(Vec2, Vec2)>,
    /// A change waits: real seconds when the last one came.
    dirty: Option<f64>,
    /// Streets or lots changed (buildings follow).
    town: bool,
    started: bool,
}

pub struct LivePlugin;

impl Plugin for LivePlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "dev")]
        dev::register(app);
        #[cfg(feature = "dev")]
        app.init_resource::<Live>().add_systems(Update, (watch, settle).chain().run_if(resource_exists::<engine::editor::EditorMode>).run_if(resource_exists::<StageWorld>));
        let _ = app;
    }
}

/// The entities `f` spawns (roots only).
pub fn spawned(world: &mut World, f: impl FnOnce(&mut World)) -> Vec<Entity> {
    let before: HashSet<Entity> = world.query::<Entity>().iter(world).collect();
    f(world);
    world.query_filtered::<Entity, Without<ChildOf>>().iter(world).filter(|e| !before.contains(e)).collect()
}

/// The XZ box of control points `from..to` (clamped, wrapping for a closed spline).
fn span_box(s: &Spline, from: isize, to: isize) -> Option<(Vec2, Vec2)> {
    let n = s.points.len() as isize;
    if n == 0 {
        return None;
    }
    let mut lo = Vec2::MAX;
    let mut hi = Vec2::MIN;
    for i in from..=to {
        let k = if s.closed { i.rem_euclid(n) } else { i.clamp(0, n - 1) };
        let p = s.points[k as usize].pos;
        lo = lo.min(Vec2::new(p.x, p.z));
        hi = hi.max(Vec2::new(p.x, p.z));
    }
    Some((lo, hi))
}

/// Where `new` differs from `old`: boxes over the changed points and the two either side
/// (a Catmull-Rom segment depends on four points), in both versions.
fn changed_boxes(old: &Spline, new: &Spline) -> Vec<(Vec2, Vec2)> {
    if old.points.len() != new.points.len() || old.closed != new.closed {
        // Points added or removed: everything from the first difference to the end.
        let first = old.points.iter().zip(&new.points).position(|(a, b)| a.pos != b.pos).unwrap_or(old.points.len().min(new.points.len())) as isize;
        return [span_box(old, first - 2, old.points.len() as isize), span_box(new, first - 2, new.points.len() as isize)].into_iter().flatten().collect();
    }
    let mut out = Vec::new();
    for i in 0..new.points.len() {
        let (a, b) = (&old.points[i], &new.points[i]);
        if a.pos != b.pos || a.width != b.width || a.bank != b.bank {
            let i = i as isize;
            out.extend(span_box(old, i - 2, i + 2));
            out.extend(span_box(new, i - 2, i + 2));
        }
    }
    out
}

#[cfg(feature = "dev")]
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn watch(
    mut live: ResMut<Live>,
    time: Res<Time<Real>>,
    loading: Option<Res<crate::loading::Loading>>,
    roads: Query<(Entity, &LevelEntity, Ref<Spline>, Ref<RoadSpline>)>,
    points: Query<(), (With<LevelEntity>, Or<(With<HomePoint>, With<EventStart>, With<GradeNote>)>, Changed<Transform>)>,
    doc: Query<Ref<WorldDoc>>,
    mut removed: RemovedComponents<Spline>,
    houses: Query<(), (With<LevelEntity>, With<super::level::Building>, Or<(Changed<super::level::Building>, Changed<Transform>)>)>,
    mut houses_gone: RemovedComponents<super::level::Building>,
) {
    if loading.is_some() {
        return;
    }
    let now = time.elapsed_secs_f64();
    if !live.started {
        // The world was just built from these: remember them, nothing to do.
        live.splines = roads.iter().map(|(_, l, s, _)| (l.id.0, (*s).clone())).collect();
        live.ids = roads.iter().map(|(e, l, _, _)| (e, l.id.0)).collect();
        live.started = true;
        return;
    }
    let mut any = false;
    for (e, l, s, info) in &roads {
        live.ids.insert(e, l.id.0);
        if !(s.is_changed() || info.is_changed()) {
            continue;
        }
        let boxes = match live.splines.get(&l.id.0) {
            Some(old) if *old == *s => Vec::new(),
            Some(old) => changed_boxes(old, &s),
            None => span_box(&s, 0, s.points.len() as isize).into_iter().collect(),
        };
        if boxes.is_empty() && (info.is_added() || !info.is_changed()) {
            continue;
        }
        if info.kind == super::RoadKind::Street {
            live.town = true;
        }
        live.reshape.extend(boxes);
        live.splines.insert(l.id.0, (*s).clone());
        any = true;
    }
    let current: HashSet<u64> = roads.iter().map(|(_, l, _, _)| l.id.0).collect();
    for e in removed.read() {
        let Some(id) = live.ids.remove(&e) else { continue };
        if current.contains(&id) {
            continue;
        }
        if let Some(old) = live.splines.remove(&id) {
            live.reshape.extend(span_box(&old, 0, old.points.len() as isize));
            live.town = true;
            any = true;
        }
    }
    if !points.is_empty() {
        any = true;
    }
    // Buildings: the car's walls and where trees may grow follow them.
    if !houses.is_empty() || houses_gone.read().next().is_some() {
        any = true;
    }
    if doc.iter().any(|d| d.is_changed() && !d.is_added()) {
        live.town = true;
        any = true;
    }
    if any {
        live.dirty = Some(now);
    }
}

#[cfg(feature = "dev")]
fn settle(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    let Some(t) = world.resource::<Live>().dirty else { return };
    if now - t < SETTLE as f64 {
        return;
    }
    let (reshape, town) = {
        let mut live = world.resource_mut::<Live>();
        live.dirty = None;
        (std::mem::take(&mut live.reshape), std::mem::take(&mut live.town))
    };
    let t0 = std::time::Instant::now();
    match refresh(world, &reshape, town) {
        Ok(()) => info!("open world: rebuilt from the level in {:.0} ms", t0.elapsed().as_secs_f32() * 1000.0),
        Err(e) => error!("open world: can't rebuild from the level: {e}"),
    }
}

/// Event finishes on roads whose course changed follow them: the nearest station of the new
/// road to where the finish was (and the start too, for events without an `EventStart`).
fn follow_events(old: &RoadNet, new: &mut WorldData) {
    let roads: Vec<_> = new.roads.clone();
    for e in &mut new.events {
        let from = e.from;
        // Every station of the route goes to the nearest one of its road's new course (the start
        // keeps its own: the `EventStart` marker sets it).
        e.remap(|road, station| {
            let (Some(o), Some(r)) = (old.roads.get(road as usize), roads.get(road as usize)) else { return station };
            let p = o.road.pts[(station as usize).min(o.road.len() - 1)];
            let d = |q: &super::data::RoadPoint| Vec2::new(q.pos.x - p.x, q.pos.z - p.z).length_squared();
            r.points.iter().enumerate().min_by(|a, b| d(a.1).total_cmp(&d(b.1))).map_or(station, |(i, _)| i as u32)
        });
        e.from = from;
    }
}

/// Shapes the ground again in `boxes` (grown by the road's reach): samples within reach of a
/// road in `old` or `new` there get the generator's height for `new`; the rest stays.
fn reshape(world: &mut World, old: &RoadNet, new: &RoadNet, boxes: &[(Vec2, Vec2)]) -> Result<(), String> {
    let reach = super::profile::REACH_MAX;
    let Some(t) = world.get_resource::<Terrain>() else { return Ok(()) };
    let hf = t.heightfield();
    let (spacing, origin, nx, nz) = (hf.spacing(), hf.origin(), hf.nx(), hf.nz());
    for (lo, hi) in boxes {
        let (lo, hi) = (*lo - Vec2::splat(reach + spacing), *hi + Vec2::splat(reach + spacing));
        let x0 = (((lo.x - origin.x) / spacing).floor().max(0.0) as u32).min(nx - 1);
        let z0 = (((lo.y - origin.y) / spacing).floor().max(0.0) as u32).min(nz - 1);
        let x1 = (((hi.x - origin.x) / spacing).ceil().max(0.0) as u32).min(nx - 1);
        let z1 = (((hi.y - origin.y) / spacing).ceil().max(0.0) as u32).min(nz - 1);
        let rect = SampleRect::new(x0, z0, x1 - x0 + 1, z1 - z0 + 1);
        let hf = world.resource::<Terrain>().heightfield();
        let values: Vec<f32> = (z0..=z1)
            .flat_map(|z| (x0..=x1).map(move |x| (x, z)))
            .map(|(x, z)| {
                let (px, pz) = (origin.x + x as f32 * spacing, origin.y + z as f32 * spacing);
                let p = Vec2::new(px, pz);
                if old.nearest(p, reach).is_some() || new.nearest(p, reach).is_some() {
                    super::roads::height(new, px, pz)
                } else {
                    hf.height_at(px, pz)
                }
            })
            .collect();
        TerrainEdit::set_heights(world, rect, &values).map_err(|e| format!("{e:?}"))?;
    }
    Ok(())
}

/// Rebuilds the open world from the level entities (see the module docs).
pub fn refresh(world: &mut World, boxes: &[(Vec2, Vec2)], town: bool) -> Result<(), String> {
    let stage = world.get_resource::<StageWorld>().ok_or("no stage")?.clone();
    let old = stage.open.clone().ok_or("not the open world")?;
    let mut data = super::level::from_level(world)?;
    follow_events(&old.net, &mut data);
    // Keep the doc's events in step (the next save writes them).
    {
        let mut q = world.query::<&mut WorldDoc>();
        if let Some(mut doc) = q.iter_mut(world).next()
            && doc.data.events != data.events
        {
            doc.bypass_change_detection().data.events = data.events.clone();
        }
    }
    let new_net = data.net();
    reshape(world, &old.net, &new_net, boxes)?;
    let heights = world.get_resource::<Terrain>().map(|t| t.heightfield().clone());
    let heights = match heights {
        // The terrain is padded to whole chunks: crop to the region.
        Some(h) if h.nx() != old.heights.nx() || h.nz() != old.heights.nz() => {
            let (n, s, o) = (old.heights.nx(), old.heights.spacing(), old.heights.origin());
            engine::world::Heightfield::from_fn(n, n, s, o, |x, z| h.height_at(x, z))
        }
        Some(h) => h,
        None => (*old.heights).clone(),
    };
    let mut open = super::OpenWorld::from_data_with(data, Some(heights))?;
    open.terrain_dir = old.terrain_dir.clone();
    // The hand edits as they are now on the level's foliage entities.
    let mut q = world.query::<(&super::level::Foliage, &engine::pcg::PcgOverrides)>();
    for (f, o) in q.iter(world) {
        match f {
            super::level::Foliage::Trees => open.trees = Arc::new(o.clone()),
            super::level::Foliage::Dressing => open.dressing = Arc::new(o.clone()),
        }
    }
    open.content = old.content;
    let open = Arc::new(open);
    let mut s = stage.clone();
    s.heights = open.heights.clone();
    s.open = Some(open.clone());
    s.forest = None;
    // Roads and junctions.
    let roads: Vec<Entity> = world.query_filtered::<Entity, With<LiveRoadMesh>>().iter(world).collect();
    for e in roads {
        world.entity_mut(e).despawn();
    }
    for e in spawned(world, |w| crate::art::ground::spawn_road(w, &s)) {
        world.entity_mut(e).insert((LiveRoadMesh, engine::level::GeneratedBy("pacenote::roads")));
    }
    crate::art::roadside::spawn(world, &open);
    // Buildings.
    if town {
        let old_b: Vec<Entity> = world.query_filtered::<Entity, With<LiveBuilding>>().iter(world).collect();
        for e in old_b {
            world.entity_mut(e).despawn();
        }
        let hf = open.heights.clone();
        for e in super::buildings::spawn(world, &open.buildings, &move |x, z| hf.height_at(x, z)) {
            if let Ok(mut e) = world.get_entity_mut(e) {
                e.insert(LiveBuilding);
            }
        }
    }
    // Trees and dressing keep off the new roads: new inputs, same graphs (only cells whose
    // inputs changed evaluate again).
    let (mut dressing, dressing_in) = crate::world::dressing(&s);
    dressing.nodes.retain(|n| n.id != "trunks");
    let (trees, trees_in) = crate::world::tree_graph(&s);
    let mut q = world.query::<(&mut engine::pcg::PcgVolume, Has<crate::art::trees::TreeVolume>)>();
    for (mut v, is_trees) in q.iter_mut(world) {
        let (g, i) = if is_trees { (trees.clone(), trees_in.clone()) } else { (dressing.clone(), dressing_in.clone()) };
        v.graph = engine::pcg::PcgGraphRef::Inline(Arc::new(g));
        v.inputs = Arc::new(i);
    }
    // Event markers (rebuilt by the events plugin), traffic lanes.
    super::events::reset(world);
    if world.contains_resource::<super::traffic::Traffic>() {
        world.insert_resource(super::traffic::Traffic::new(&open.net, super::traffic::TrafficConfig::default()));
    }
    // The forest colliders follow the new roads when the car next needs them.
    let (g, i) = crate::world::dressing(&s);
    s.forest = Some(Arc::new(super::forest::ForestSolids::with_overrides(g, i, &open.trees, &open.dressing)));
    world.insert_resource(s);
    Ok(())
}

#[cfg(feature = "dev")]
pub mod dev {
    use bevy::prelude::*;
    use engine::devtools::{AddControlCommand, params};
    use serde_json::json;

    pub fn register(app: &mut App) {
        app.add_control_command(
            "open.check",
            "Roads more than 5 cm above or below the live terrain anywhere: the worst place of each",
            params(&[]),
            |w, _| {
                let s = w.get_resource::<crate::world::StageWorld>().ok_or("no stage")?;
                let open = s.open.clone().ok_or("not the open world")?;
                let t = w.get_resource::<engine::world::Terrain>().ok_or("no terrain")?;
                let hf = t.heightfield();
                let mut worst = Vec::new();
                for (k, r) in open.net.roads.iter().enumerate() {
                    let (mut above, mut below) = ((0.0f32, Vec3::ZERO), (0.0f32, Vec3::ZERO));
                    for p in &r.road.pts {
                        let d = p.y - hf.height_at(p.x, p.z);
                        if d > above.0 {
                            above = (d, *p);
                        }
                        if d < below.0 {
                            below = (d, *p);
                        }
                    }
                    if above.0 > 0.05 || below.0 < -0.05 {
                        worst.push(json!({"road": k, "above": above.0, "above_at": [above.1.x, above.1.z], "below": below.0, "below_at": [below.1.x, below.1.z]}));
                    }
                }
                Ok(json!({"roads": open.net.roads.len(), "off_the_ground": worst}))
            },
        );
    }
}
