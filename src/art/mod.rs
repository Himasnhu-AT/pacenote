//! Pacenote's look: materials, terrain layers, the road surface, trees and dressing, the car,
//! sky and weather, wheel spray. Everything visual about a stage that isn't gameplay.
//!
//! Hooks (called by the stage loader): [`ground::layers`] for the terrain, [`spawn_stage`]
//! after the terrain (road, dressing, trees, sky), [`car::spawn`] for the car's visuals.
//! Assets: cooked texture sets under `assets/textures/` (see `tools/art/prep_textures.sh`),
//! meshes under `assets/models/` (`tools/art/build_meshes.py`), foliage cards rendered by
//! `tools/art/render_cards.py`. Credits: `assets/credits.txt`.

pub mod car;
pub mod dressing;
pub mod ground;
pub mod hands;
pub mod junctions;
pub mod look;
pub mod meshfile;
pub mod props;
pub mod spray;
pub mod terrain;
pub mod trees;

use std::sync::Arc;

use bevy::prelude::*;
use engine::pcg::{PcgGraphRef, PcgVolume};
use engine::prelude::*;
use pcg_core::Area;

use crate::stage::{self, StageDef, Weather};
use crate::world::StageWorld;

/// The stage is wet (rain): darker, glossier materials.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct Wet(pub bool);

pub struct ArtPlugin;

impl Plugin for ArtPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "dev")]
        dev::register(app);
        // Headless runs (tests, servers) have no renderer: nothing to draw.
        if !app.is_plugin_added::<bevy::pbr::MaterialPlugin<StandardMaterial>>() {
            return;
        }
        trees::register(app);
        app.add_systems(Update, (trees::ingest, trees::stream).chain())
            .add_systems(Update, dressing::tag_cover)
            .add_plugins((car::CarArtPlugin, spray::SprayPlugin, terrain::TerrainArtPlugin, props::PropArtPlugin));
    }
}

/// Whether this run draws anything (headless tests and servers don't).
pub fn visual(world: &World) -> bool {
    world.contains_resource::<Assets<StandardMaterial>>() && world.contains_resource::<AssetServer>()
}

/// A cooked PBR set (`assets/textures/<set>`) as a material, adjusted by `f`. Falls back to a
/// flat grey material (with an error) when the set is missing.
pub fn pbr(world: &mut World, set: &str, f: impl FnOnce(&mut StandardMaterial)) -> Handle<StandardMaterial> {
    let dir = world.resource::<AssetsDir>().clone();
    let mut m = world.resource_scope(|world, server: Mut<AssetServer>| {
        let mut pending = world.resource_mut::<engine::gfx::PendingMips>();
        engine::gfx::load_pbr(&server, &dir, &mut pending, set, Color::WHITE)
    });
    if m.is_none() {
        error!("art: texture set `{set}` missing");
    }
    let mut mat = m.take().unwrap_or(StandardMaterial { base_color: Color::srgb(0.5, 0.5, 0.5), perceptual_roughness: 0.9, ..default() });
    f(&mut mat);
    world.resource_mut::<Assets<StandardMaterial>>().add(mat)
}

/// Is the stage wet?
pub fn wet(def: &StageDef) -> bool {
    def.weather == Weather::Rain
}

/// Spawns the stage's visuals after the terrain: road, dressing, trees, sky and atmosphere.
pub fn spawn_stage(world: &mut World, s: &StageWorld) {
    world.insert_resource(Wet(wet(&s.def)));
    let q = world.get_resource::<engine::settings::CoreSettings>().map_or(2, |c| c.quality.index());
    let view = look::quality_view(look::view_distance(&s.def), q);
    world.insert_resource(look::stage_look(&s.def, view));
    // Scenery (rocks, bushes) stops where the fog hides it.
    if let Some(mut r) = world.get_resource_mut::<SceneryRanges>() {
        r.view = view.min(400.0);
        r.far = 90.0;
    }
    if !visual(world) {
        return;
    }
    world.remove_resource::<terrain::PnTerrainHandle>();
    world.insert_resource(terrain::TerrainPlan(ground::layers(s.def.biome, wet(&s.def))));
    ground::spawn_road(world, s);
    let cat = dressing::catalog(world, s.def.biome, wet(&s.def));
    world.insert_resource(cat);
    trees::setup(world, s, view, q);
    props::spawn(world, s);
    let (graph, inputs) = crate::world::dressing(s);
    let half = stage::half_size(&s.def);
    world.spawn(PcgVolume { graph: PcgGraphRef::Inline(Arc::new(graph)), inputs: Arc::new(inputs), area: Area::new(Vec2::splat(-half), Vec2::splat(half)), cell: 64.0 });
    spray::setup(world, &s.def);
}

#[cfg(feature = "dev")]
mod dev {
    use bevy::prelude::*;
    use engine::devtools::{AddControlCommand, params};
    use serde_json::{Value, json};

    use crate::drive::{DrivenCar, StageRun};
    use crate::world::StageWorld;

    /// Triangles per mesh asset, noted when it is added (render-only meshes leave the main
    /// world once extracted), for `art.triangles`.
    #[derive(Resource, Default)]
    struct MeshTris(std::collections::HashMap<AssetId<Mesh>, u32>);

    fn note_tris(mut ev: MessageReader<AssetEvent<Mesh>>, meshes: Res<Assets<Mesh>>, mut tris: ResMut<MeshTris>) {
        for e in ev.read() {
            match e {
                AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                    if let Some(m) = meshes.get(*id) {
                        let n = m.try_indices_option().ok().flatten().map_or_else(|| m.count_vertices(), |i| i.len()) / 3;
                        tris.0.insert(*id, n as u32);
                    }
                }
                AssetEvent::Removed { id } => {
                    tris.0.remove(id);
                }
                _ => {}
            }
        }
    }

    /// Triangles the main camera draws this frame (meshes it sees after frustum and distance
    /// culling; shadow passes not counted), the biggest groups by name.
    fn triangles(world: &mut World, _: Value) -> Result<Value, String> {
        use std::any::TypeId;
        use std::collections::HashMap;
        let mut cams = world.query_filtered::<&bevy::camera::visibility::VisibleEntities, With<engine::gfx::MainCamera>>();
        let visible: Vec<Entity> = cams.iter(world).flat_map(|v| v.get(TypeId::of::<Mesh3d>()).to_vec()).collect();
        let mut q = world.query::<(&Mesh3d, Option<&Name>)>();
        let tris = world.resource::<MeshTris>();
        let (mut total, mut unknown) = (0u64, 0u32);
        let mut by: HashMap<String, (u32, u64)> = HashMap::new();
        for e in &visible {
            let Ok((m, name)) = q.get(world, *e) else { continue };
            let Some(n) = tris.0.get(&m.0.id()) else {
                unknown += 1;
                continue;
            };
            total += *n as u64;
            let g = by.entry(name.map_or("(unnamed)".into(), |n| n.as_str().to_string())).or_default();
            g.0 += 1;
            g.1 += *n as u64;
        }
        let mut rows: Vec<_> = by.into_iter().collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.1.1));
        let rows: Vec<_> = rows.into_iter().take(10).map(|(k, (n, t))| json!([k, n, t])).collect();
        Ok(json!({"visible_meshes": visible.len(), "triangles": total, "unknown_meshes": unknown, "by_name": rows}))
    }

    pub fn register(app: &mut App) {
        app.init_resource::<MeshTris>().add_systems(Last, note_tris);
        app.add_control_command("art.triangles", "Triangles the main camera draws this frame (no shadow passes), by name", params(&[]), triangles);
        app.add_control_command("art.stats", "Trees (cells, LOD0/LOD1 entities, impostor chunks and triangles, LOD distances)", params(&[]), stats)
            .add_control_command("art.textures", "Load state of every material texture (counts, and the first paths per state)", params(&[]), textures)
            .add_control_command(
                "ui.click_at",
                "Click the primary window at logical (x, y) the way winit does (CursorMoved + button WindowEvents)",
                params(&[("x", "number", "logical px from the left"), ("y", "number", "logical px from the top"), ("action", "string", "move | down | up | tap (default: move + down; send up next)")]),
                click_at,
            )
            .add_control_command("art.memory", "Mesh and image memory by entity name (CPU copies; render-only meshes counted, not sized)", params(&[]), memory)
            .add_control_command(
            "art.place",
            "Put the car on the road `s` metres from the start (clock and progress follow), for views along the stage",
            params(&[("s", "number", "metres along the road"), ("lateral", "number", "metres left of the centre (default 0)")]),
            place,
        )
            .add_control_command(
                "art.teleport",
                "Put the car at world (x, z) facing `heading` degrees (0 = +z, 90 = +x), on the ground; for screenshots of the open world",
                params(&[("x", "number", "world x (m)"), ("z", "number", "world z (m)"), ("heading", "number", "degrees, 0 = +z (default 0)")]),
                teleport,
            )
            .add_control_command(
                "art.overhead",
                "Hold the camera at `eye` looking at `at` (heights above the ground at `at`; screenshots from above); no params: back to the car",
                params(&[("eye", "array", "[x, y, z]"), ("at", "array", "[x, y, z]")]),
                overhead,
            )
            .init_resource::<Overhead>()
            .add_systems(PostUpdate, hold_overhead.before(bevy::transform::TransformSystems::Propagate));
    }

    /// Where `art.overhead` holds the camera.
    #[derive(Resource, Default)]
    struct Overhead(Option<(Vec3, Vec3)>);

    fn hold_overhead(o: Res<Overhead>, mut cams: Query<&mut Transform, With<engine::gfx::MainCamera>>) {
        if let Some((eye, at)) = o.0
            && let Ok(mut t) = cams.single_mut()
        {
            let up = if (at - eye).normalize_or_zero().y.abs() > 0.99 { Vec3::NEG_Z } else { Vec3::Y };
            *t = Transform::from_translation(eye).looking_at(at, up);
        }
    }

    fn vec3(v: Option<&Value>) -> Option<Vec3> {
        let a = v?.as_array()?;
        Some(Vec3::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32))
    }

    fn overhead(world: &mut World, p: Value) -> Result<Value, String> {
        // Heights are above the ground at `at`.
        let ground = |at: Vec3| world.get_resource::<StageWorld>().map_or(0.0, |s| s.heights.height_at(at.x, at.z));
        let v = vec3(p.get("eye")).zip(vec3(p.get("at"))).map(|(e, a)| {
            let g = ground(a);
            (e + Vec3::Y * g, a + Vec3::Y * g)
        });
        world.resource_mut::<Overhead>().0 = v;
        Ok(json!({"held": v.is_some()}))
    }

    fn teleport(world: &mut World, p: Value) -> Result<Value, String> {
        let x = p.get("x").and_then(Value::as_f64).ok_or("`x` is required")? as f32;
        let z = p.get("z").and_then(Value::as_f64).ok_or("`z` is required")? as f32;
        let h = (p.get("heading").and_then(Value::as_f64).unwrap_or(0.0) as f32).to_radians();
        let stage = world.get_resource::<StageWorld>().ok_or("no stage")?.clone();
        let y = stage.heights.height_at(x, z);
        let mut q = world.query::<&mut DrivenCar>();
        let mut c = q.single_mut(world).map_err(|e| e.to_string())?;
        crate::vehicle::recover(&mut c.car, Vec3::new(x, y, z), h);
        c.prev = (c.car.pos, c.car.rot);
        let pos = c.car.pos;
        Ok(json!({"pos": [pos.x, pos.y, pos.z]}))
    }

    fn stats(world: &mut World, _: Value) -> Result<Value, String> {
        let t = world.get_resource::<super::trees::TreeField>().map(|f| f.stats).unwrap_or_default();
        let lods = t.lods.map(|l| json!({"lod0": l.lod0, "lod1": l.lod1, "reach": l.reach, "thin": l.thin}));
        Ok(json!({"trees": t.trees, "cells": t.cells, "near_cells": t.near_cells, "near_entities": t.near_entities, "lod0_entities": t.lod0_entities, "lod1_entities": t.lod1_entities, "chunks": t.chunks, "chunks_total": t.chunks_total, "far_triangles": t.far_triangles, "lods": lods}))
    }

    fn textures(world: &mut World, _: Value) -> Result<Value, String> {
        use std::collections::BTreeMap;
        let server = world.resource::<AssetServer>();
        let mut seen = std::collections::HashSet::new();
        let mut by: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
        for (_, m) in world.resource::<Assets<StandardMaterial>>().iter() {
            for h in [&m.base_color_texture, &m.normal_map_texture, &m.metallic_roughness_texture, &m.occlusion_texture].into_iter().flatten() {
                if !seen.insert(h.id()) {
                    continue;
                }
                let state = match server.get_load_state(h.id()) {
                    Some(bevy::asset::LoadState::Failed(e)) => format!("failed: {e}").chars().take(160).collect(),
                    Some(s) => format!("{s:?}"),
                    None => "untracked".into(),
                };
                let e = by.entry(state).or_default();
                e.0 += 1;
                if e.1.len() < 3 {
                    e.1.push(h.path().map_or("(no path)".into(), |p| p.to_string()));
                }
            }
        }
        Ok(json!(by))
    }

    fn click_at(world: &mut World, p: Value) -> Result<Value, String> {
        use bevy::input::{ButtonState, mouse::MouseButtonInput};
        use bevy::window::{CursorMoved, PrimaryWindow, WindowEvent};
        if p.get("action").and_then(Value::as_str) == Some("list") {
            let scale = world.query_filtered::<&Window, With<PrimaryWindow>>().single(world).map_or(1.0, |w| w.scale_factor());
            let list: Vec<Value> = world
                .query::<(&engine::ui::widgets::ActionButton, &bevy::ui::UiGlobalTransform, &bevy::ui::ComputedUiTargetCamera, Option<&bevy::picking::hover::Hovered>)>()
                .iter(world)
                .map(|(b, t, cam, h)| json!([format!("{:?}", b.0), [t.translation.x / scale, t.translation.y / scale], format!("{:?}", cam.get()), h.map(|h| h.0)]))
                .collect();
            return Ok(json!({"scale": scale, "buttons": list}));
        }
        let x = p.get("x").and_then(Value::as_f64).ok_or("`x` is required")? as f32;
        let y = p.get("y").and_then(Value::as_f64).ok_or("`y` is required")? as f32;
        let action = p.get("action").and_then(Value::as_str).unwrap_or("tap");
        let mut q = world.query_filtered::<(Entity, &Window), With<PrimaryWindow>>();
        let (window, w) = q.single(world).map_err(|_| "no primary window")?;
        let scale = w.scale_factor();
        let at = Vec2::new(x, y);
        if let Ok(mut w) = world.query_filtered::<&mut Window, With<PrimaryWindow>>().single_mut(world) {
            w.set_physical_cursor_position(Some((at * scale).as_dvec2()));
        }
        if matches!(action, "move" | "tap") {
            let moved = CursorMoved { window, position: at, delta: None };
            world.write_message(moved.clone());
            world.write_message(WindowEvent::CursorMoved(moved));
        }
        let button = |state| MouseButtonInput { button: MouseButton::Left, state, window };
        if matches!(action, "down" | "tap") {
            world.write_message(button(ButtonState::Pressed));
            world.write_message(WindowEvent::MouseButtonInput(button(ButtonState::Pressed)));
        }
        if action == "up" {
            world.write_message(button(ButtonState::Released));
            world.write_message(WindowEvent::MouseButtonInput(button(ButtonState::Released)));
        }
        let hovered: Vec<String> = world
            .query::<(&engine::ui::widgets::ActionButton, &bevy::picking::hover::Hovered)>()
            .iter(world)
            .filter(|(_, h)| h.0)
            .map(|(b, _)| format!("{:?}", b.0))
            .collect();
        Ok(json!({"at": [x, y], "scale": scale, "hovered": hovered}))
    }

    /// Vertex and index bytes of a mesh still on the CPU (`None`: render-world only).
    fn mesh_bytes(m: &Mesh) -> Option<usize> {
        let v: usize = m.try_attributes().ok()?.map(|(_, a)| a.get_bytes().len()).sum();
        Some(v + m.try_indices_option().ok().flatten().map_or(0, |i| i.len() * 4))
    }

    fn memory(world: &mut World, _: Value) -> Result<Value, String> {
        use std::collections::{HashMap, HashSet};
        let mut q = world.query::<(&Mesh3d, Option<&Name>)>();
        let meshes = world.resource::<Assets<Mesh>>();
        // name -> (entities, unique meshes, sized bytes, render-only meshes)
        let mut by: HashMap<String, (usize, HashSet<AssetId<Mesh>>, usize, usize)> = HashMap::new();
        for (m, name) in q.iter(world) {
            let key = name.map_or("(unnamed)".to_string(), |n| n.as_str().to_string());
            let e = by.entry(key).or_default();
            e.0 += 1;
            if e.1.insert(m.0.id()) {
                match meshes.get(&m.0).and_then(mesh_bytes) {
                    Some(b) => e.2 += b,
                    None => e.3 += 1,
                }
            }
        }
        let mut rows: Vec<_> = by.into_iter().collect();
        rows.sort_by(|a, b| (b.1.2, b.1.0).cmp(&(a.1.2, a.1.0)));
        // Compact rows, biggest first: [name, entities, unique meshes, MB on the CPU, render-only meshes].
        let rows: Vec<_> = rows.into_iter().take(14).map(|(k, (n, u, b, r))| json!([k, n, u.len(), b / 1_000_000, r])).collect();
        let images = world.resource::<Assets<Image>>();
        let img_mb: usize = images.iter().map(|(_, i)| i.data.as_ref().map_or(0, |d| d.len())).sum();
        let all_mb: usize = meshes.iter().filter_map(|(_, m)| mesh_bytes(m)).sum();
        Ok(json!({"mesh_assets": meshes.len(), "mesh_mb_cpu": all_mb / 1_000_000, "images": images.len(), "image_mb_cpu": img_mb / 1_000_000, "by_name": rows}))
    }

    fn place(world: &mut World, p: Value) -> Result<Value, String> {
        let s = p.get("s").and_then(Value::as_f64).ok_or("`s` is required")? as f32;
        let lat = p.get("lateral").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let stage = world.get_resource::<StageWorld>().ok_or("no stage")?.clone();
        let i = stage.road.station(s);
        let (pt, d) = (stage.road.pts[i], stage.road.dir[i]);
        let left = Vec3::new(d.y, 0.0, -d.x);
        let mut q = world.query::<&mut DrivenCar>();
        let mut c = q.single_mut(world).map_err(|e| e.to_string())?;
        crate::vehicle::recover(&mut c.car, pt + left * lat, d.x.atan2(d.y));
        c.prev = (c.car.pos, c.car.rot);
        let pos = c.car.pos;
        if let Some(mut run) = world.get_resource_mut::<StageRun>() {
            run.progress = s;
        }
        Ok(json!({"s": s, "pos": [pos.x, pos.y, pos.z]}))
    }
}
