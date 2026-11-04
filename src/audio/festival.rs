//! Festival gameplay sounds, synthesised at startup (no assets): Style ticks, bank and
//! break, Radar flash and readout, countdown, checkpoint, finish, fail, money. Bus `hud`.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/AUDIO_PASS.md` §3 and §5 (ducking), `docs/festival/DECISIONS.md` S2, C1;
//! listens to `career::style::StyleEvent`, `open::challenges::{RadarFlash, RunCue}`,
//! `career::MoneyEarned`.
//! Wave 0: an empty plugin, registered by the lead so this stream never edits the module list.

use bevy::prelude::*;

pub struct FestivalAudioPlugin;

impl Plugin for FestivalAudioPlugin {
    fn build(&self, _app: &mut App) {}
}
