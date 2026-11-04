//! HUD: the race position block (top left) and rival dots on the progress bar.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §5, `docs/festival/UX_PASS.md` §3.3, `docs/festival/DECISIONS.md` R6.
//! A read-only view of F's state; E owns `hud.rs` and the shared HUD look (`hud::plate`).
//! Despawn on leaving `InGame`; allocate nothing per frame while values are unchanged.

use bevy::prelude::*;

pub struct RaceHudPlugin;

impl Plugin for RaceHudPlugin {
    fn build(&self, _app: &mut App) {}
}
