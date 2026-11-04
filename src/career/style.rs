//! Style scoring: skills detected from the sim, chained into a Rhythm that banks Style and
//! Notes.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §2 (rules and
//! numbers) and SK-1..SK-8; rulings `docs/festival/DECISIONS.md` S1-S5 (GAME_SYSTEMS's
//! `SkillEvent` is folded into [`StyleEvent`]). Readers: the Style HUD (`hud_style`, F), the
//! festival sounds (`audio::festival`, F), Slide Sections (`open::challenges`, F).
//!
//! Detection is a read-only observer of `drive::DrivenCar`, in `FixedUpdate` after
//! `SimulateSet`; it never feeds the sim (TECH_PASS §5).

use bevy::prelude::*;

/// Seconds with no skill live before a Rhythm banks.
pub const RHYTHM_TIMEOUT: f32 = 3.5;
/// The Rhythm multiplier's cap.
pub const MULT_CAP: f32 = 6.0;
/// Style per Note when a Rhythm banks.
pub const STYLE_PER_NOTE: u32 = 25;
/// Most Notes one Rhythm pays.
pub const NOTES_CAP: u32 = 2_000;

/// A skill (GAME_SYSTEMS §2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StyleKind {
    Slide,
    Switch,
    FlatOut,
    Air,
    CloseShave,
    Slipstream,
    Splinter,
    Switchback,
    CleanLine,
    CleanPass,
    Challenge,
}

/// Why a Rhythm broke (the HUD's reason line).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LostReason {
    /// A damaging hit (tree, wall, rock...).
    Hit,
    /// Contact with a traffic or rival car at ≥ 8 km/h closing speed.
    Contact,
    Recover,
    /// Upside down or on its side.
    Rolled,
    CarSwap,
    EventStart,
    Quit,
}

/// What the scorer reports. HUD, audio and challenges listen; live values (pending points,
/// multiplier, timer) are in [`Rhythm`].
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum StyleEvent {
    /// A skill scored (or a continuous skill's running total grew). `chain`: skills in the
    /// Rhythm so far (the audio climbs a semitone per step).
    Tick { kind: StyleKind, points: u32, chain: u8 },
    /// The Rhythm banked: `points` = round(P × M) Style, paying `notes`.
    Bank { points: u32, mult: f32, notes: u32, chain: u8 },
    /// The Rhythm broke: `points` pending were lost.
    Lost { points: u32, why: LostReason },
}

/// The Rhythm in progress (all zero / ×1 when none is open).
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Rhythm {
    /// Pending points P (before the multiplier).
    pub points: u32,
    /// Multiplier M, 1.0..=MULT_CAP.
    pub mult: f32,
    /// Skills completed in this Rhythm.
    pub chain: u8,
    /// Seconds left before it banks.
    pub timer: f32,
    /// The last skill completed (the variety bonus).
    pub last: Option<StyleKind>,
}

impl Default for Rhythm {
    fn default() -> Self {
        Rhythm { points: 0, mult: 1.0, chain: 0, timer: 0.0, last: None }
    }
}
