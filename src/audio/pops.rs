//! Exhaust pops and bangs: the main-thread `PopScheduler` decides each pop (lift-off crackle,
//! upshift bang, downshift blip, limiter, misfire) and sends [`ExhaustPop`]; the synth plays it
//! (`CarEvent::Pop`) and `art::exhaust` draws its flame. One source, so the bang and the flame
//! always match.
//!
//! **Owner: stream D (CAR FEEL).** Design: `docs/festival/AUDIO_PASS.md` §1-§2 (trigger rules,
//! burst timing, recipe, routing) and §6; `docs/festival/ART_BIBLE.md` §3.1 (its `Backfire` is
//! this message); `docs/festival/DECISIONS.md` D1-D2.

use bevy::prelude::*;

/// What kind of pop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopKind {
    Crackle,
    Bang,
    /// Damage backfire: black smoke, no flame.
    Misfire,
}

/// One pop, on the frame it is due. Audio and VFX read the same message.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct ExhaustPop {
    /// The car entity (player, guide's remote car, rival).
    pub car: Entity,
    /// 0..1: size of the flame, level of the bang.
    pub intensity: f32,
    pub kind: PopKind,
    /// Per-pop variation (flame shape, synth layers).
    pub seed: u32,
}

pub struct PopsPlugin;

impl Plugin for PopsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ExhaustPop>();
    }
}
