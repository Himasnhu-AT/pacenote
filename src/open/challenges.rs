//! Roadside challenges: Radar Post, Pace Section, Slide Section, Yump Board. Drive-through
//! (no stop, no menu): the first gate starts the attempt.
//!
//! **Owner: stream F (GAMEPLAY, stream CHALLENGES).** Design: `docs/festival/GAME_SYSTEMS.md`
//! §3 and CH-1..CH-7 (rules, stars, rewards); the 13 anchors in `docs/festival/WORLD_PASS.md`
//! §5; rulings `docs/festival/DECISIONS.md` C1-C5 (13 now, the seeded placer later; stored in
//! [`CHALLENGES_FILE`] by road index + station, not in the level file A re-bakes).
//! Read-only observer of `drive::DrivenCar` after `SimulateSet`.

use bevy::math::Vec2;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The challenges, under the assets dir (F's file; star thresholds baked by
/// `--bake-challenges` after A's re-bake).
pub const CHALLENGES_FILE: &str = "levels/open/challenges.ron";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChallengeKind {
    /// Speed at the gate line (km/h).
    RadarPost,
    /// Mean speed between the start and end gates (km/h).
    PaceSection,
    /// Raw Slide + Switch points between the gates.
    SlideSection,
    /// Horizontal distance from take-off to touchdown (m).
    YumpBoard,
}

/// One challenge as stored in [`CHALLENGES_FILE`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChallengeDef {
    /// Stable id: saves and payouts key by it.
    pub id: u32,
    pub kind: ChallengeKind,
    /// Road index in the `RoadNet` (stable through the re-bake, DECISIONS W1).
    pub road: u32,
    /// Gate stations (m along the road): one for a Radar Post / Yump crest, start and end
    /// (plus hidden mid-gates) for sections.
    pub gates: Vec<f32>,
    /// 1★, 2★, 3★ thresholds.
    pub thresholds: [f32; 3],
    /// The bake car's reference value R.
    pub reference: f32,
    /// World x,z of the first gate, as a check against the road (a test fails if they differ).
    pub at: Vec2,
}

/// A challenge attempt finished. Economy (stars pay once), style (the Challenge skill), HUD.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct ChallengeResult {
    pub id: u32,
    pub value: f32,
    /// Stars earned on this run, 0..=3.
    pub stars: u8,
    pub new_best: bool,
}

/// The car crossed a Radar Post gate: the flash and readout sounds (AUDIO_PASS §3; was
/// `SpeedTrap`).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct RadarFlash {
    pub pos: Vec3,
    pub kmh: f32,
    pub target_beaten: bool,
    pub personal_best: bool,
}

/// Run cues for the festival sounds (AUDIO_PASS §3; was `ActivityCue`): event and race
/// countdowns, challenge checkpoints, finishes.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunCue {
    Countdown(u8),
    Go,
    Checkpoint,
    /// `place`: 1-based in a race, 0 when solo.
    Finish { place: u8, best: bool },
    Fail,
}

pub struct ChallengesPlugin;

impl Plugin for ChallengesPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ChallengeResult>().add_message::<RadarFlash>().add_message::<RunCue>();
    }
}
