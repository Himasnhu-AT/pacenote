//! HUD: the Rhythm panel (bottom right), skill lines, Banked / Broken.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §2.4, `docs/festival/UX_PASS.md` §3.1, `docs/festival/DECISIONS.md` S4.
//! A read-only view of F's state; E owns `hud.rs` and the shared HUD look (`hud::plate`).
//! Despawn on leaving `InGame`; allocate nothing per frame while values are unchanged.

use bevy::prelude::*;

pub struct StyleHudPlugin;

impl Plugin for StyleHudPlugin {
    fn build(&self, _app: &mut App) {}
}
