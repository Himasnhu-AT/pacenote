//! The look of road conditions (potholes, patches, puddles, washouts): mesh patches 2 mm over
//! the road, merged per tile (not `ForwardDecal`: Low has no depth prepass).
//!
//! **Owner: stream B (ROADSIDE).** Design: `docs/festival/ART_BIBLE.md` §4 "Road conditions",
//! `docs/festival/WORLD_PASS.md` §4, `docs/festival/DECISIONS.md` W5.
//! Wave 0: an empty plugin, registered by the lead so this stream never edits the module list.

use bevy::prelude::*;

pub struct ConditionsPlugin;

impl Plugin for ConditionsPlugin {
    fn build(&self, _app: &mut App) {}
}
