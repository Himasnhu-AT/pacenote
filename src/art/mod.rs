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
pub mod look;
pub mod meshfile;
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
        app.add_systems(Update, (trees::ingest, trees::stream).chain())
            .add_systems(Update, dressing::tag_cover)
            .add_plugins((car::CarArtPlugin, spray::SprayPlugin, terrain::TerrainArtPlugin));
        #[cfg(feature = "dev")]
        dev::register(app);
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
    let view = look::view_distance(&s.def);
    world.insert_resource(look::stage_look(&s.def));
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
    trees::setup(world, s, view);
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

    pub fn register(app: &mut App) {
        app.add_control_command("art.stats", "Trees (near/far cells, entities, far triangles)", params(&[]), stats).add_control_command(
            "art.place",
            "Put the car on the road `s` metres from the start (clock and progress follow), for views along the stage",
            params(&[("s", "number", "metres along the road"), ("lateral", "number", "metres left of the centre (default 0)")]),
            place,
        );
    }

    fn stats(world: &mut World, _: Value) -> Result<Value, String> {
        let t = world.get_resource::<super::trees::TreeField>().map(|f| f.stats).unwrap_or_default();
        Ok(json!({"trees": t.trees, "cells": t.cells, "near_cells": t.near_cells, "near_entities": t.near_entities, "far_triangles": t.far_triangles}))
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
