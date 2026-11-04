//! Menu sounds (bus `ui`, never ducked by the car), synthesised: hover, select, back, screen
//! transition, error.
//!
//! **Owner: stream E (UI).** Design: `docs/festival/AUDIO_PASS.md` §3 "Main menu UI" and §6,
//! `docs/festival/DECISIONS.md` U7.

use bevy::prelude::*;

/// A menu sound. E's screens send it; this module plays it.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiSound {
    Hover,
    Select,
    Back,
    Transition { forward: bool },
    Error,
}

pub struct UiSoundPlugin;

impl Plugin for UiSoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<UiSound>();
    }
}
