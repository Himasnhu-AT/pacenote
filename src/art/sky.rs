//! The sky: a camera-following cloud shell (`art/clouds.wesl`, ray-marched 1.8-3.2 km,
//! 4/8 steps, stars at night), the moon (the sun light re-aimed) and street-lamp lights.
//!
//! **Owner: stream C (LOOK).** Design: `docs/festival/ART_BIBLE.md` §2 with `docs/festival/TECH_PASS.md` §1/§4
//! (shell mesh, not a render-graph pass; never switch `SkyMode` in play) and
//! `docs/festival/DECISIONS.md` L2-L4, L7.
//! Wave 0: an empty plugin, registered by the lead so this stream never edits the module list.

use bevy::prelude::*;

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, _app: &mut App) {}
}
