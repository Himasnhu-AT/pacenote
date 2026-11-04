//! Exhaust flames, flash light, sparks and smoke, driven by `audio::pops::ExhaustPop` (one
//! message for the bang and the flame).
//!
//! **Owner: stream D (CAR FEEL).** Design: `docs/festival/ART_BIBLE.md` §3.1 (look, tip position) and
//! `docs/festival/AUDIO_PASS.md` §2 (the scheduler), `docs/festival/DECISIONS.md` D1.
//! Wave 0: an empty plugin, registered by the lead so this stream never edits the module list.

use bevy::prelude::*;

pub struct ExhaustPlugin;

impl Plugin for ExhaustPlugin {
    fn build(&self, _app: &mut App) {}
}
