//! The open world (see `docs/OPENWORLD.md`): one region with a road network, terrain shaped by
//! the roads, and (in later steps) buildings, traffic and events. [`OpenWorld`] is what the car
//! drives on (`vehicle::Ground`).

pub mod buildings;
pub mod data;
pub mod events;
pub mod forest;
pub mod junctions;
pub mod level;
pub mod placeable;
pub mod roads;
pub mod tour;
pub mod traffic;

use std::sync::Arc;

use bevy::math::{Vec2, Vec3};
use engine::world::Heightfield;

use crate::stage::Surface;
use crate::vehicle::{Ground, Obstacle};
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
    /// Town and farm buildings, and their walls as solid cylinders for the car.
    pub buildings: Arc<buildings::Layout>,
    pub walls: Arc<buildings::ObstacleGrid>,
    /// Where the traffic cars are (written by `traffic::TrafficPlugin` each step; the same
    /// `Arc` as the `TrafficSolids` resource): solid for the car in free roam.
    pub traffic: traffic::TrafficSolids,
    /// Whether traffic is solid for the car (dev: the measuring tour turns it off so the
    /// autopilot, which doesn't look out for traffic, isn't wrecked at a crossing).
    pub traffic_solid: Arc<std::sync::atomic::AtomicBool>,
    /// The terrain folder of the level it was loaded from (editor sculpts are saved there);
    /// `None` when generated.
    pub terrain_dir: Option<std::path::PathBuf>,
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
            OpenWorld::from_data_with(l.data, Some(l.heights)).map(|mut o| {
                o.terrain_dir = Some(dir);
                o
            })
        }) {
            Ok(o) => {
                bevy::log::info!("open world: {} in {:.2} s", path.display(), t.elapsed().as_secs_f32());
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
        let layout = buildings::layout_with(&net, &data.lots);
        let walls = buildings::ObstacleGrid::new(&buildings::obstacles(&layout), 10.0);
        Ok(OpenWorld {
            data: Arc::new(data),
            net: Arc::new(net),
            heights: Arc::new(heights),
            buildings: Arc::new(layout),
            walls: Arc::new(walls),
            traffic: traffic::TrafficSolids::default(),
            traffic_solid: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            terrain_dir: None,
        })
    }

    /// Where the car starts and which way it faces (home).
    pub fn spawn(&self) -> (Vec3, Vec2) {
        (self.data.home.pos, self.data.home.dir)
    }
}

impl Ground for OpenWorld {
    fn height(&self, x: f32, z: f32) -> f32 {
        self.heights.height_at(x, z)
    }
    fn normal(&self, x: f32, z: f32) -> Vec3 {
        self.heights.smooth_normal_at(x, z)
    }
    fn surface(&self, x: f32, z: f32) -> Surface {
        roads::surface(&self.net, x, z)
    }
    fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
        out.clear();
        self.walls.query(p, r, out);
        self.traffic.obstacles(p, r, out);
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
