//! Camera feel: the shared speed sensation and the Camera tab's settings. D may move the
//! cameras here from `drive.rs` (chase spring arm, shake, landing kick, motion blur).
//!
//! **Owner: stream D (CAR FEEL).** Design: `docs/festival/ART_BIBLE.md` §3.2-§3.4 (spring arm,
//! motion blur, shake), `docs/festival/AUDIO_PASS.md` §5 (`SpeedFeel` drives the wind too),
//! `docs/festival/TECH_PASS.md` §1/§4 (never insert or remove `MotionBlur` in play),
//! `docs/festival/DECISIONS.md` D3-D6.

use bevy::prelude::*;
use engine::settings::add_settings_section;
use engine::ui::prelude::*;
use serde::{Deserialize, Serialize};

/// How fast it feels, 0..1: smoothstep over 40 → 180 km/h. FOV, motion blur and the wind all
/// read this one value. Look- and sound-only (never feeds the sim).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct SpeedFeel(pub f32);

/// The Camera tab (section `"camera_feel"` of settings.json). D registers its rows (tab
/// "Camera") in its own plugin with `register_setting`; nothing in `prefs.rs` lists them.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraFeel {
    /// Motion blur: 0 Off, 1 Low (0.6×), 2 High. Default Low (DECISIONS D3). Only where the
    /// quality tier allows it; reduce motion forces Off.
    pub motion_blur: u32,
    /// Chase distance: 0 Near, 1 Normal, 2 Far.
    pub chase_distance: u32,
    /// Chase stiffness, 0.5 (loose) ..= 1.5 (tight): scales the spring arm's ω.
    pub chase_stiffness: f32,
    /// A dip of the chase camera on hard landings.
    pub landing_kick: bool,
}

impl Default for CameraFeel {
    fn default() -> Self {
        CameraFeel { motion_blur: 1, chase_distance: 1, chase_stiffness: 1.0, landing_kick: true }
    }
}

pub struct CameraFeelPlugin;

impl Plugin for CameraFeelPlugin {
    fn build(&self, app: &mut App) {
        add_settings_section::<CameraFeel>(app, "camera_feel");
        app.settings_defaults::<CameraFeel>().init_resource::<SpeedFeel>();
    }
}
