//! Rival drivers: five AI cars on the same deterministic sim, the race field and its result,
//! and the list of other cars that style scoring and the HUD read.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §5 and AI-1..AI-8;
//! rulings `docs/festival/DECISIONS.md` R1-R7; determinism `docs/festival/TECH_PASS.md` §5.
//!
//! Rules that keep ghosts exact: only a run started in [`EventMode::Race`] has rivals; Race
//! runs record no ghost and no board time. Racers have their own [`Racer`] component (never
//! `drive::DrivenCar`) and their own `FixedUpdate` system after `drive::SimulateSet`, stepped in
//! `slot` order. The player sees them through [`RacerSolids`] (hooked into
//! `world::StageWorld::obstacles` in wave 0).

use std::sync::{Arc, RwLock};

use bevy::math::Vec2;
use bevy::prelude::*;

use crate::pilot::Pilot;
use crate::vehicle::{Car, Obstacle, Spec};

/// Race difficulty (GAME_SYSTEMS §5 table).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Rookie,
    #[default]
    Pro,
    Ace,
    Legend,
}

/// How an event run is driven (DECISIONS R1). Lap events are Time Trial only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EventMode {
    /// Solo, ghost, records (today's events).
    #[default]
    TimeTrial,
    /// Against 5 rivals.
    Race(Difficulty),
}

/// Start format, decided by the road's width over the first 400 m (GAME_SYSTEMS §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartFormat {
    /// 2 wide, 3 rows, one launch (≥ 6 m wide).
    Grid,
    /// Single file, released 1.5 s apart (narrower).
    Pursuit,
}

/// One AI car in a race: its own car, spec and pilot.
#[derive(Component, Clone, Debug)]
pub struct Racer {
    /// Start slot and step order.
    pub slot: u8,
    pub car: Car,
    pub spec: Spec,
    pub pilot: Pilot,
    /// Pose after the previous step (interpolation and the solids).
    pub prev: (Vec3, Quat),
}

/// The racers' poses after the last step, `(position, forward)` per car (the same shape as
/// `open::traffic::TrafficSolids`, DECISIONS R4). Shared by `Arc` with the `StageWorld`.
#[derive(Resource, Clone, Default)]
pub struct RacerSolids(pub Arc<RwLock<Vec<(Vec3, Vec3)>>>);

impl RacerSolids {
    /// Collision cylinders for racers within `r` of `p`, appended to `out` (the traffic shape:
    /// three circles along the car).
    pub fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
        if let Ok(poses) = self.0.read() {
            for pose in poses.iter() {
                crate::open::traffic::push_solids(*pose, p, r, out);
            }
        }
    }
}

/// Who an entrant is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Who {
    Player,
    /// A rival by start slot.
    Rival(u8),
}

/// One entrant's metadata (the sim state is on the `Racer`).
#[derive(Clone, Debug, PartialEq)]
pub struct Entrant {
    pub who: Who,
    /// Invented driver name ("A. Varenne").
    pub name: String,
    /// `garage::CarDef::id`.
    pub car: String,
    /// Fixed step it is released on (pursuit starts).
    pub start_step: u32,
    pub finish: Option<Finish>,
}

/// How an entrant ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Finish {
    /// Race clock (s); estimated for rivals still running when the player finishes.
    Time(f32),
    Dnf,
}

/// The race in progress (absent outside Race runs).
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct RaceField {
    pub event: u32,
    pub difficulty: Difficulty,
    pub format: StartFormat,
    pub entrants: Vec<Entrant>,
}

/// A race finished (the player crossed the line or retired). Economy, results screen, career.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct RaceResult {
    pub event: u32,
    pub difficulty: Difficulty,
    /// Finishing order, first to last.
    pub order: Vec<(Who, Finish)>,
    /// The player's position, 1-based.
    pub player_pos: u8,
    /// No logged contact with a rival (the clean bonus).
    pub clean: bool,
}

/// What another car is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OtherKind {
    Traffic,
    Rival(u8),
}

/// Another car near the player, as style scoring, the HUD and the minimap see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OtherCar {
    pub id: u32,
    pub kind: OtherKind,
    pub pos: Vec3,
    pub fwd: Vec3,
    /// m/s.
    pub speed: f32,
}

/// Every other car (traffic and rivals), rebuilt by F each fixed step (DECISIONS R5).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct OtherCars(pub Vec<OtherCar>);
