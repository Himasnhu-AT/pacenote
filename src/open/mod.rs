//! The open world (see `docs/OPENWORLD.md`): one region with a road network, terrain shaped by
//! the roads, and (in later steps) buildings, traffic and events. [`OpenWorld`] is what the car
//! drives on (`vehicle::Ground`).

pub mod roads;

use std::sync::Arc;

use bevy::math::{Vec2, Vec3};
use engine::world::Heightfield;

use crate::stage::Surface;
use crate::vehicle::{Ground, Obstacle};
pub use roads::{RoadKind, RoadNet};

/// Seed of the one world.
pub const SEED: u64 = 0x0E11_0001;
/// Terrain sample spacing (m).
pub const SPACING: f32 = 2.0;

/// The world as the sim sees it.
#[derive(Clone)]
pub struct OpenWorld {
    pub net: Arc<RoadNet>,
    pub heights: Arc<Heightfield>,
}

impl OpenWorld {
    /// Builds the network and the terrain (a second or two).
    pub fn generate(seed: u64) -> OpenWorld {
        let net = roads::generate(seed);
        let n = (2.0 * roads::HALF / SPACING) as u32 + 1;
        let origin = Vec2::splat(-roads::HALF);
        let heights = Heightfield::from_fn(n, n, SPACING, origin, |x, z| roads::height(&net, x, z));
        OpenWorld { net: Arc::new(net), heights: Arc::new(heights) }
    }

    /// Where the car starts and which way it faces.
    pub fn spawn(&self) -> (Vec3, Vec2) {
        roads::spawn(&self.net)
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
    fn obstacles(&self, _p: Vec2, _r: f32, out: &mut Vec<Obstacle>) {
        out.clear();
    }
}
