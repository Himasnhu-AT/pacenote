//! Menu sounds (bus `ui`, never ducked by the car), synthesised: hover, select, back, screen
//! transition, error.
//!
//! **Owner: stream E (UI).** Design: `docs/festival/AUDIO_PASS.md` §3 "Main menu UI" and §6,
//! `docs/festival/DECISIONS.md` U7.
//!
//! The clips are made once at startup (a few kB each) and registered as engine sound cues
//! (`ui.hover`, `ui.select`, ...) with the recipe's peak level, cooldown and instance limit;
//! a [`UiSound`] message plays one. Nothing is heard when the engine has no sound device
//! (headless, `--mute`, tests): the engine's mixer drops the voices.

use bevy::prelude::*;
use engine::sound::{SoundClip, SoundCtl, SoundCue, SoundCues, Steal};

/// A menu sound. E's screens send it; this module plays it.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiSound {
    Hover,
    Select,
    Back,
    Transition { forward: bool },
    Error,
}

impl UiSound {
    /// The engine cue that plays it.
    pub fn cue(self) -> &'static str {
        match self {
            UiSound::Hover => "ui.hover",
            UiSound::Select => "ui.select",
            UiSound::Back => "ui.back",
            UiSound::Transition { forward: true } => "ui.transition_fwd",
            UiSound::Transition { forward: false } => "ui.transition_back",
            UiSound::Error => "ui.error",
        }
    }
}

pub struct UiSoundPlugin;

impl Plugin for UiSoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<UiSound>().add_systems(Startup, register_cues).add_systems(PostUpdate, play.before(engine::sound::SoundSet::Gather));
    }
}

/// Output rate of the synthesised clips.
const RATE: u32 = 48_000;

fn register_cues(cues: Option<ResMut<SoundCues>>, clips: Option<ResMut<Assets<SoundClip>>>) {
    let (Some(mut cues), Some(mut clips)) = (cues, clips) else { return };
    let ui = |name: &'static str, clip: SoundClip, db: f32, clips: &mut Assets<SoundClip>| SoundCue::new(name).clip(clips.add(clip)).bus("ui").volume(db).max_instances(1, Steal::Oldest);
    cues.add(ui(UiSound::Hover.cue(), SoundClip::from_mono(hover(), RATE), -26.0, &mut clips).cooldown(0.04).max_instances(2, Steal::Oldest).pitch_jitter(0.5));
    cues.add(ui(UiSound::Select.cue(), SoundClip::from_mono(select(false), RATE), -16.0, &mut clips));
    cues.add(ui(UiSound::Back.cue(), SoundClip::from_mono(select(true), RATE), -18.0, &mut clips));
    for forward in [true, false] {
        cues.add(ui(UiSound::Transition { forward }.cue(), SoundClip::from_interleaved(transition(forward), 2, RATE), -20.0, &mut clips));
    }
    cues.add(ui(UiSound::Error.cue(), SoundClip::from_mono(error(), RATE), -20.0, &mut clips).cooldown(0.15));
}

fn play(mut msgs: MessageReader<UiSound>, ctl: Option<ResMut<SoundCtl>>) {
    let Some(mut ctl) = ctl else {
        msgs.clear();
        return;
    };
    // One of each kind per frame (a rebuild can focus and select in the same frame).
    let mut seen: Vec<UiSound> = Vec::new();
    for s in msgs.read() {
        if !seen.contains(s) {
            seen.push(*s);
            ctl.play(s.cue());
        }
    }
}

// ---------------------------------------------------------------- recipes (AUDIO_PASS §3)

fn samples(secs: f32) -> usize {
    (secs * RATE as f32) as usize
}

/// Scales `v` so its peak is 1 (the cue's volume sets the level).
fn normalise(mut v: Vec<f32>) -> Vec<f32> {
    let peak = v.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-6);
    for s in &mut v {
        *s /= peak;
    }
    v
}

/// One-pole low-pass in place.
fn lowpass(v: &mut [f32], hz: f32) {
    let a = 1.0 - (-std::f32::consts::TAU * hz / RATE as f32).exp();
    let mut y = 0.0;
    for s in v.iter_mut() {
        y += a * (*s - y);
        *s = y;
    }
}

/// Hover: a 12 ms tick, 2.2 kHz plus a 3.3 kHz partial, τ 18 ms.
pub fn hover() -> Vec<f32> {
    let tau = 0.018;
    normalise(
        (0..samples(0.012))
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let env = (-t / tau).exp() * (1.0 - (-t / 0.0008).exp());
                env * ((std::f32::consts::TAU * 2200.0 * t).sin() + 0.45 * (std::f32::consts::TAU * 3300.0 * t).sin())
            })
            .collect(),
    )
}

fn triangle(phase: f32) -> f32 {
    4.0 * (phase.fract() - 0.5).abs() - 1.0
}

/// Select: a 110 Hz thump (40 ms), a 1.6 kHz click (8 ms) and two notes E5 → B5 (triangle, LP
/// 4 kHz, 45 ms each). `back`: the notes reversed (B5 → E5), LP 2.5 kHz.
pub fn select(back: bool) -> Vec<f32> {
    let (e5, b5) = (659.25, 987.77);
    let notes = if back { [b5, e5] } else { [e5, b5] };
    let mut v = vec![0.0f32; samples(0.15)];
    for (i, s) in v.iter_mut().enumerate() {
        let t = i as f32 / RATE as f32;
        if !back && t < 0.04 {
            *s += 0.9 * (std::f32::consts::TAU * 110.0 * t).sin() * (-t / 0.012).exp();
        }
        if !back && t < 0.008 {
            *s += 0.35 * (std::f32::consts::TAU * 1600.0 * t).sin() * (1.0 - t / 0.008);
        }
        // The notes start just after the click.
        let tn = t - 0.01;
        if tn >= 0.0 {
            let k = (tn / 0.045) as usize;
            if k < 2 {
                let local = tn - k as f32 * 0.045;
                let env = (local / 0.004).min(1.0) * (1.0 - local / 0.045).max(0.0).powf(0.6);
                *s += 0.5 * triangle(notes[k] * tn) * env;
            }
        }
    }
    lowpass(&mut v, if back { 2500.0 } else { 4000.0 });
    normalise(v)
}

/// Transition: pink noise for 380 ms through a band-pass sweeping 400 Hz → 2.8 kHz (down when
/// going back), fade in 220 ms, fade out 160 ms, panned −0.4 → +0.4. Stereo, interleaved.
pub fn transition(forward: bool) -> Vec<f32> {
    let n = samples(0.38);
    let mut rng = 0x1234_5678u32;
    // Pink noise (Paul Kellet's economy filter) through a state-variable band-pass.
    let (mut b0, mut b1, mut b2) = (0.0f32, 0.0f32, 0.0f32);
    let (mut low, mut band) = (0.0f32, 0.0f32);
    let q = 0.8;
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let white = (rng as f32 / u32::MAX as f32) * 2.0 - 1.0;
        b0 = 0.99765 * b0 + white * 0.099_046;
        b1 = 0.96300 * b1 + white * 0.296_516_4;
        b2 = 0.57000 * b2 + white * 1.052_691_3;
        let pink = (b0 + b1 + b2 + white * 0.1848) * 0.2;
        let x = i as f32 / n as f32;
        let sweep = if forward { x } else { 1.0 - x };
        let fc = 400.0 * (2800.0f32 / 400.0).powf(sweep);
        let f = 2.0 * (std::f32::consts::PI * fc / RATE as f32).sin();
        let high = pink - low - band / q;
        band += f * high;
        low += f * band;
        let t = i as f32 / RATE as f32;
        let env = (t / 0.22).min(1.0) * ((0.38 - t) / 0.16).clamp(0.0, 1.0);
        let pan = -0.4 + 0.8 * x;
        let s = band * env;
        out.push(s * ((1.0 - pan) * 0.5).sqrt());
        out.push(s * ((1.0 + pan) * 0.5).sqrt());
    }
    normalise(out)
}

/// Error: two 90 Hz square bursts of 60 ms (30 ms apart), LP 800 Hz.
pub fn error() -> Vec<f32> {
    let mut v: Vec<f32> = (0..samples(0.15))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let on = t < 0.06 || (0.09..0.15).contains(&t);
            match (on, (90.0 * t).fract() < 0.5) {
                (false, _) => 0.0,
                (true, true) => 0.6,
                (true, false) => -0.6,
            }
        })
        .collect();
    lowpass(&mut v, 800.0);
    normalise(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_have_the_recipe_lengths_and_peaks() {
        let ms = |v: &[f32], ch: usize| v.len() as f32 / ch as f32 / RATE as f32 * 1000.0;
        assert!((ms(&hover(), 1) - 12.0).abs() < 1.0);
        assert!((ms(&transition(true), 2) - 380.0).abs() < 1.0);
        for v in [hover(), select(false), select(true), transition(false), error()] {
            let peak = v.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!((peak - 1.0).abs() < 1e-3, "normalised");
            assert!(v.iter().all(|s| s.is_finite()));
        }
    }

    #[test]
    fn every_sound_has_its_own_cue() {
        let all = [UiSound::Hover, UiSound::Select, UiSound::Back, UiSound::Transition { forward: true }, UiSound::Transition { forward: false }, UiSound::Error];
        let names: std::collections::HashSet<&str> = all.iter().map(|s| s.cue()).collect();
        assert_eq!(names.len(), all.len());
    }
}
