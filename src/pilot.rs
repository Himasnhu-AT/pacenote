//! The driving core of the AI: a pure input function over the route, its corners and the
//! progress, compiled in every build (the dev autopilot becomes a thin shell over it).
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §5 and AI-1 (there
//! called `ai::Driver`; DECISIONS R3 names it `pilot::Pilot`). Users: the rivals
//! (`career::racers`), the dev autopilot (`autopilot.rs`), the challenge star bake.
//!
//! Wave 0 stub: the types only. F moves the path follower out of `autopilot.rs` and adds
//! `Pilot::input(..) -> vehicle::Input`. Pure: no RNG but a seeded one, no frame time.

/// How a pilot drives (a difficulty row of GAME_SYSTEMS §5, plus the per-rival spread).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PilotParams {
    /// Share of grip used in corners.
    pub pace: f32,
    /// Share of grip used braking.
    pub brake: f32,
    /// Lateral line offset (m, left positive).
    pub line_offset: f32,
    /// Seed for mistakes and the pace spread (`hash(event, difficulty, attempt, slot)`).
    pub seed: u64,
}

/// One AI driver's state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pilot {
    pub params: PilotParams,
}
