//! Car lights: two low-beam spots and the 4-lamp pod (toggled by the lights key, on by
//! themselves in events once the sun is below 2°), emissive lenses, brake and tail lamps.
//!
//! **Owner: stream D (CAR FEEL).** Design: `docs/festival/ART_BIBLE.md` §2.3 "Night
//! requirements", `docs/festival/TECH_PASS.md` §4/§7 (the shadowed spot is spawned at load with
//! intensity 0 by day; never toggle `shadows_enabled`), `docs/festival/DECISIONS.md` L6-L7.

use bevy::prelude::*;

/// The car has working headlights. **Its existence is the capability flag**: C offers the
/// Night start time only when this resource exists (`daytime::night_available`). Wave 0
/// defines it but nothing inserts it; D's [`HeadlightsPlugin`] does once the lights work.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Headlights {
    /// The lights are switched on.
    pub on: bool,
}

pub struct HeadlightsPlugin;

impl Plugin for HeadlightsPlugin {
    fn build(&self, _app: &mut App) {}
}
