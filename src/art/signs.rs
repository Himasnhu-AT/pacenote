//! Meshes for the furniture `open::furniture` places: guard rails, warning / speed / town
//! signs, km and reflector posts, street-light poles. One shared mesh + material (or atlas) per
//! kind, merged per tile, no shadows on posts and signs.
//!
//! **Owner: stream B (ROADSIDE).** Design: `docs/festival/ART_BIBLE.md` §4 (look), `docs/festival/WORLD_PASS.md` §4
//! (placement), `docs/festival/TECH_PASS.md` §4 (≤ +20 draws), `docs/festival/DECISIONS.md` W3-W4.
//! Wave 0: an empty plugin, registered by the lead so this stream never edits the module list.

use bevy::prelude::*;

pub struct SignsPlugin;

impl Plugin for SignsPlugin {
    fn build(&self, _app: &mut App) {}
}
