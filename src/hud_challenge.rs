//! HUD: challenge approach cards, live readouts and result cards (top centre).
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §3, `docs/festival/UX_PASS.md` §3.2, `docs/festival/DECISIONS.md` C1.
//! A read-only view of F's state; E owns `hud.rs` and the shared HUD look (`hud::plate`).
//! Despawn on leaving `InGame`; allocate nothing per frame while values are unchanged.

use bevy::prelude::*;

pub struct ChallengeHudPlugin;

impl Plugin for ChallengeHudPlugin {
    fn build(&self, _app: &mut App) {}
}
