//! The time of day: a clock that runs at 5× real time and moves the sun, and the World tab's
//! start-time settings.
//!
//! **Owner: stream C (LOOK).** Design: `docs/festival/ART_BIBLE.md` §2.3 (sun path, keyframes)
//! with `docs/festival/TECH_PASS.md` §1 (write `Look` about once a second, no `LookTimeline`,
//! never switch `SkyMode`) and `docs/festival/DECISIONS.md` L1-L8.
//!
//! Look-only: the sim never reads the clock, and it is not part of any event's condition key
//! (TECH_PASS §5). Wave 0 stub: the clock stands still at today's frozen afternoon sun, so the
//! game looks exactly as before.

use bevy::prelude::*;
use engine::settings::add_settings_section;
use engine::ui::prelude::*;
use serde::{Deserialize, Serialize};

/// Game seconds per real second (one day ≈ 4 h 48 min).
pub const DAY_RATE: f32 = 5.0;

/// The World › Start time choices, in `DaySettings::start` order: name and game hour (`None`:
/// the wall clock / the saved clock).
pub const START_PRESETS: [(&str, Option<f32>); 8] = [
    ("Dawn", Some(6.0)),
    ("Morning", Some(9.0)),
    ("Noon", Some(12.5)),
    ("Afternoon", Some(16.0)),
    ("Sunset", Some(19.5)),
    ("Night", Some(23.0)),
    ("Real clock", None),
    ("Last", None),
];
/// Index of "Night" in [`START_PRESETS`]: offered only when [`night_available`].
pub const NIGHT: u32 = 5;

/// The game clock. Advanced on `Time<Virtual>` in `Update` (pause freezes it).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct DayClock {
    /// 0..24.
    pub hours: f32,
    /// Game seconds per real second.
    pub rate: f32,
    pub running: bool,
}

impl Default for DayClock {
    fn default() -> Self {
        DayClock { hours: 16.0, rate: DAY_RATE, running: false }
    }
}

impl DayClock {
    /// Sun azimuth and elevation (degrees). Wave 0 stub: today's fixed open-world sun
    /// (235°, 30°); C replaces it with the solar formula (46° N, solar noon 13:00).
    pub fn sun(&self) -> (f32, f32) {
        (235.0, 30.0)
    }
    /// 0 at night .. 1 in full day, from the sun's elevation (−6° .. +6°).
    pub fn daylight(&self) -> f32 {
        let (_, el) = self.sun();
        ((el + 6.0) / 12.0).clamp(0.0, 1.0)
    }
    pub fn is_night(&self) -> bool {
        self.daylight() < 0.05
    }
}

/// The World tab (section `"day"` of settings.json). C registers its rows (tab "World") in its
/// own plugin with `register_setting`; nothing in `prefs.rs` lists them.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DaySettings {
    /// Index into [`START_PRESETS`]; free roam only. Default Morning.
    pub start: u32,
    /// Off freezes the clock at the start time.
    pub time_passes: bool,
}

impl Default for DaySettings {
    fn default() -> Self {
        DaySettings { start: 1, time_passes: true }
    }
}

/// Whether the Night start (and a night "Real clock" / "Last") may be offered: only once the
/// car has headlights (D inserts `art::headlights::Headlights`, DECISIONS L6). C also requires
/// its moon before it offers Night.
pub fn night_available(world: &World) -> bool {
    world.contains_resource::<crate::art::headlights::Headlights>()
}

pub struct DayPlugin;

impl Plugin for DayPlugin {
    fn build(&self, app: &mut App) {
        add_settings_section::<DaySettings>(app, "day");
        app.settings_defaults::<DaySettings>().init_resource::<DayClock>();
    }
}
