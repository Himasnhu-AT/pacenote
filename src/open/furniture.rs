//! Roadside furniture and road conditions the car feels: guard rails (solid), signs and posts
//! (placement only; the meshes are `art::signs`), and patches of potholes, wet tarmac and
//! washouts. Pure, seeded and sorted, built with the `OpenWorld` from the road net and the
//! heights, so the same level always gives the same furniture.
//!
//! **Owner: stream B (ROADSIDE).** Design: `docs/festival/WORLD_PASS.md` §4 (placement rules,
//! measured), `docs/festival/ART_BIBLE.md` §4 (look), `docs/festival/TECH_PASS.md` §3.0/§5,
//! `docs/festival/DECISIONS.md` W3-W7.
//!
//! Wave 0: the hooks are wired into `open::OpenWorld` ([`dip`] in `ground_height`,
//! [`surface_override`] in `surface_at`, [`walls`] in `from_data_with`) and all return
//! "nothing", so the world drives exactly as before. Anything here that changes the sim bumps
//! [`REV`], which goes into the event `stage_hash`.

use bevy::math::{Vec2, Vec3};
use engine::world::Heightfield;

use super::RoadNet;
use crate::stage::Surface;
use crate::vehicle::Wall;

/// Furniture revision: bump when the generated furniture changes how the car drives.
pub const REV: u32 = 0;

/// Guard-rail material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailKind {
    /// Steel W-beam (tarmac).
    Steel,
    /// Timber-faced steel (gravel, forest).
    Timber,
}

/// One run of rail along a road side.
#[derive(Clone, Debug, PartialEq)]
pub struct RailRun {
    pub road: u32,
    /// +1 left, −1 right of the road's direction.
    pub side: i8,
    /// Stations (m).
    pub from: f32,
    pub to: f32,
    pub kind: RailKind,
}

/// What a sign or post is. B may add variants (only B's files match on it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignKind {
    /// Warning triangle (B numbers the pictograms).
    Warning(u8),
    /// Speed limit (km/h); 0 = end of limit.
    Speed(u8),
    TownBoard,
    KmPost(u16),
    Reflector,
    StreetLight,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sign {
    pub pos: Vec3,
    /// Facing (radians about Y).
    pub yaw: f32,
    pub kind: SignKind,
}

/// A road condition the car feels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PatchKind {
    /// A height dip (m deep); grip unchanged.
    Pothole { depth: f32 },
    /// `Surface::Wet`.
    Wet,
    /// `Surface::Loose` plus corrugation.
    Washout,
}

/// One condition patch. B may change the shape (only B's files read it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Patch {
    pub centre: Vec2,
    pub radius: f32,
    pub kind: PatchKind,
}

/// Everything B places. `Default` = none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Furniture {
    pub rails: Vec<RailRun>,
    pub signs: Vec<Sign>,
    /// Sorted and grid-indexed by B.
    pub patches: Vec<Patch>,
}

/// The furniture for a road net on its terrain. Wave 0: none.
pub fn build(_net: &RoadNet, _heights: &Heightfield) -> Furniture {
    Furniture::default()
}

/// How far the ground dips at (x, z) (potholes, washout corrugation), m ≥ 0, subtracted from
/// the ground height. Wave 0: 0.
pub fn dip(_f: &Furniture, _x: f32, _z: f32) -> f32 {
    0.0
}

/// The surface a patch puts at (x, z), if any. Wave 0: none.
pub fn surface_override(_f: &Furniture, _x: f32, _z: f32) -> Option<Surface> {
    None
}

/// The solid furniture (guard rails, street-light stubs) as walls, appended to `out`.
/// Wave 0: none.
pub fn walls(_f: &Furniture, _out: &mut Vec<Wall>) {}
