//! The open world (see `docs/OPENWORLD.md`): one region with a road network, terrain shaped by
//! the roads, and (in later steps) buildings, traffic and events. [`OpenWorld`] is what the car
//! drives on (`vehicle::Ground`).

pub mod buildings;
pub mod challenges;
pub mod data;
pub mod ecology;
pub mod events;
pub mod forest;
pub mod furniture;
pub mod junctions;
pub mod land;
pub mod plan;
pub mod profile;
pub mod level;
pub mod live;
pub mod placeable;
pub mod roads;
pub mod tour;
pub mod traffic;
pub mod walls;

use std::sync::Arc;

use bevy::math::{Vec2, Vec3};
use engine::world::Heightfield;

use crate::stage::Surface;
use crate::vehicle::{Ground, Obstacle, Wall};
pub use roads::{RoadKind, RoadNet};

/// Seed of the one world.
pub const SEED: u64 = 0x0E11_0001;
/// The open world's stage id: everything that loads "a stage" loads the world for this id
/// (`world::generate`). Real stages are 1..=20.
pub const ID: u32 = 0;

/// The open world as a stage definition (its look, biome and seed; no route of its own).
pub fn def() -> crate::stage::StageDef {
    use crate::stage::{Biome, StageDef, TimeOfDay, Weather};
    StageDef {
        id: ID,
        name: "Open world".into(),
        seed: SEED,
        biome: Biome::PineForest,
        length: 0.0,
        surface: Surface::Tarmac,
        patches: 0.0,
        time: TimeOfDay::Afternoon,
        weather: Weather::Clear,
        twist: 0.0,
    }
}
/// Terrain sample spacing (m).
pub const SPACING: f32 = 2.0;

/// The world as the sim sees it.
#[derive(Clone)]
pub struct OpenWorld {
    /// What the world is made from (`data`): roads, events, lots, home, overrides.
    pub data: Arc<data::WorldData>,
    pub net: Arc<RoadNet>,
    pub heights: Arc<Heightfield>,
    /// Town and farm buildings, and the straight walls the car can hit (their facades).
    pub buildings: Arc<buildings::Layout>,
    pub walls: Arc<walls::WallGrid>,
    /// Guard rails, signs and road conditions (`furniture`, stream B): built from the roads and
    /// the heights. Empty until B fills it in.
    pub furniture: Arc<furniture::Furniture>,
    /// Where the traffic cars are (written by `traffic::TrafficPlugin` each step; the same
    /// `Arc` as the `TrafficSolids` resource): solid for the car in free roam.
    pub traffic: traffic::TrafficSolids,
    /// Whether traffic is solid for the car (dev: the measuring tour turns it off so the
    /// autopilot, which doesn't look out for traffic, isn't wrecked at a crossing).
    pub traffic_solid: Arc<std::sync::atomic::AtomicBool>,
    /// The terrain folder of the level it was loaded from (editor sculpts are saved there);
    /// `None` when generated.
    pub terrain_dir: Option<std::path::PathBuf>,
    /// Hand edits of the trees and of the dressing (rocks, bushes), from the level.
    pub trees: Arc<engine::pcg::PcgOverrides>,
    pub dressing: Arc<engine::pcg::PcgOverrides>,
    /// Hash of the level it was loaded from (file and terrain; 0 when generated).
    pub content: u64,
}

impl OpenWorld {
    /// The world the seed makes (a second or two): captured as data, then built from it.
    pub fn generate(seed: u64) -> OpenWorld {
        // (Covered by `data::tests::generated_data_is_valid`.)
        OpenWorld::from_data(data::WorldData::generate(seed)).expect("the generated world data is valid")
    }

    /// Builds the network, the terrain and the town from the world data, once it passes
    /// [`data::WorldData::validate`].
    pub fn from_data(data: data::WorldData) -> Result<OpenWorld, String> {
        OpenWorld::from_data_with(data, None)
    }

    /// The world from the level file the game plays ([`level::source`]): its data and its
    /// terrain. Falls back to the generated world (with an error in the log) if the level is
    /// missing or bad.
    pub fn load_or_generate(seed: u64) -> OpenWorld {
        let path = level::source();
        let t = std::time::Instant::now();
        match level::load(&path).and_then(|l| {
            let dir = l.terrain.clone();
            let (trees, dressing, content) = (l.trees, l.dressing, l.content);
            OpenWorld::from_data_with(l.data, Some(l.heights)).map(|mut o| {
                o.terrain_dir = Some(dir);
                o.trees = Arc::new(trees);
                o.dressing = Arc::new(dressing);
                o.content = content;
                o
            })
        }) {
            Ok(o) => {
                let terrain = o.terrain_dir.as_deref().map(|d| d.display().to_string()).unwrap_or_default();
                bevy::log::info!("open world: {} (terrain {terrain}) in {:.2} s", path.display(), t.elapsed().as_secs_f32());
                o
            }
            Err(e) => {
                bevy::log::error!("open world: can't load {}: {e}; generating it from the seed", path.display());
                OpenWorld::generate(seed)
            }
        }
    }

    /// [`from_data`](Self::from_data) with the terrain heights given (a level's terrain files)
    /// rather than shaped from the roads. They must cover the region at [`SPACING`].
    pub fn from_data_with(data: data::WorldData, heights: Option<Heightfield>) -> Result<OpenWorld, String> {
        data.validate()?;
        let net = data.net();
        let n = (2.0 * roads::HALF / SPACING) as u32 + 1;
        let origin = Vec2::splat(-roads::HALF);
        let heights = match heights {
            Some(h) => {
                if h.nx() != n || h.nz() != n || h.spacing() != SPACING || h.origin() != origin {
                    return Err(format!("terrain {}x{} at {} m from {}: needs {n}x{n} at {SPACING} m from {origin}", h.nx(), h.nz(), h.spacing(), h.origin()));
                }
                h
            }
            None => Heightfield::from_fn(n, n, SPACING, origin, |x, z| roads::height(&net, x, z)),
        };
        let layout = buildings::layout_baked(&net, &data.lots, &data.buildings);
        // Every straight wall the car can hit: facades now; parapets and barriers go here too.
        let mut all_walls = buildings::walls(&layout);
        all_walls.extend(profile::walls(&net));
        {
            let cuts = ecology::clearcuts(&net, ecology::CLEARCUTS);
            let hf = &heights;
            all_walls.extend(crate::art::roadside::walls(&crate::art::roadside::items(&net, &cuts), &|p| hf.height_at(p.x, p.y)));
        }
        let furniture = furniture::build(&net, &heights);
        furniture::walls(&furniture, &mut all_walls);
        let walls = walls::WallGrid::new(&all_walls, 10.0);
        Ok(OpenWorld {
            data: Arc::new(data),
            net: Arc::new(net),
            heights: Arc::new(heights),
            buildings: Arc::new(layout),
            walls: Arc::new(walls),
            furniture: Arc::new(furniture),
            traffic: traffic::TrafficSolids::default(),
            traffic_solid: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            terrain_dir: None,
            trees: Arc::default(),
            dressing: Arc::default(),
            content: 0,
        })
    }

    /// Where the car starts and which way it faces (home).
    pub fn spawn(&self) -> (Vec3, Vec2) {
        (self.data.home.pos, self.data.home.dir)
    }
}

impl OpenWorld {
    /// The ground the car drives on: the exact road cross-section near a road (crown, wheel
    /// tracks, shoulder, ditch; `profile::ground`), the terrain elsewhere.
    pub fn ground_height(&self, x: f32, z: f32) -> f32 {
        let hf = self.heights.height_at(x, z);
        profile::ground(&self.net, Vec2::new(x, z), hf).unwrap_or(hf) - furniture::dip(&self.furniture, x, z)
    }

    /// What the tyres are on at (x, z): a road condition patch (`furniture`), else the road's
    /// or the land's surface (`roads::surface`).
    pub fn surface_at(&self, x: f32, z: f32) -> Surface {
        furniture::surface_override(&self.furniture, x, z).unwrap_or_else(|| roads::surface(&self.net, x, z))
    }

    /// The ground's normal: from the exact profile near a road, the terrain's elsewhere.
    pub fn ground_normal(&self, x: f32, z: f32) -> Vec3 {
        let p = Vec2::new(x, z);
        if self.net.nearest(p, 9.0).is_none() {
            return self.heights.smooth_normal_at(x, z);
        }
        let e = 0.25;
        let dx = self.ground_height(x + e, z) - self.ground_height(x - e, z);
        let dz = self.ground_height(x, z + e) - self.ground_height(x, z - e);
        Vec3::new(-dx, 2.0 * e, -dz).normalize()
    }
}

impl Ground for OpenWorld {
    fn height(&self, x: f32, z: f32) -> f32 {
        self.ground_height(x, z)
    }
    fn normal(&self, x: f32, z: f32) -> Vec3 {
        self.ground_normal(x, z)
    }
    fn surface(&self, x: f32, z: f32) -> Surface {
        self.surface_at(x, z)
    }
    fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
        out.clear();
        self.traffic.obstacles(p, r, out);
    }
    fn walls(&self, p: Vec2, r: f32, out: &mut Vec<Wall>) {
        out.clear();
        self.walls.query(p, r, out);
    }
}

#[cfg(test)]
mod timing {
    #[test]
    #[ignore]
    fn generation_cost() {
        let t = std::time::Instant::now();
        let net = super::roads::generate(super::SEED);
        eprintln!("roads {:.2} s", t.elapsed().as_secs_f32());
        let t = std::time::Instant::now();
        let mut acc = 0.0;
        for i in 0..200_000 {
            let p = bevy::math::Vec2::new((i % 2000) as f32 * 2.0 - 2000.0, (i / 2000) as f32 * 20.0 - 2000.0);
            acc += super::roads::base_height(p, net.seed);
        }
        eprintln!("base x200k {:.3} s ({acc})", t.elapsed().as_secs_f32());
        let t = std::time::Instant::now();
        let mut n = 0;
        for i in 0..200_000 {
            let p = bevy::math::Vec2::new((i % 2000) as f32 * 2.0 - 2000.0, (i / 2000) as f32 * 20.0 - 2000.0);
            n += net.nearest(p, 52.0).is_some() as u32;
        }
        eprintln!("nearest x200k {:.3} s ({n})", t.elapsed().as_secs_f32());
        let t = std::time::Instant::now();
        let w = crate::world::generate(&super::def());
        eprintln!("world::generate {:.2} s, obstacles near spawn {}", t.elapsed().as_secs_f32(), w.obstacle_list().len());
    }
}
