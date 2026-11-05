//! Festival gameplay sounds, synthesised at startup (no assets): Style ticks, bank and
//! break, Radar flash and readout, countdown, checkpoint, finish, fail, money. Bus `hud`
//! (child of `ui`: the UI volume covers it).
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/AUDIO_PASS.md` §3 and §5 (ducking), `docs/festival/DECISIONS.md` S2, C1;
//! listens to `career::style::StyleEvent`, `open::challenges::{RadarFlash, RunCue}`,
//! `career::MoneyEarned`. Gameplay sends facts; the recipes and levels are here.
//!
//! Rules from AUDIO_PASS §8 item 7: ticks never pass 15 a second (the cue's cooldown), a bank
//! stops pending ticks, and Lost (−18 dBFS) is never louder than Bank (−14).

use std::f32::consts::TAU;

use bevy::prelude::*;
use engine::sound::{Attenuation, BusId, Mixer, PlayParams, SoundClip, SoundCtl, SoundCue, SoundCues, Steal};

use crate::career::MoneyEarned;
use crate::career::style::StyleEvent;
use crate::open::challenges::{RadarFlash, RunCue};

const RATE: u32 = 48_000;
/// E5: the festival cues' home pitch.
const E5: f32 = 659.25;
/// Ticks per second at most.
pub const MAX_TICKS: f32 = 15.0;
/// Peak levels (dBFS) of the bank and break cues: a break is never louder than a bank.
pub const BANK_DB: f32 = -14.0;
pub const LOST_DB: f32 = -18.0;

pub struct FestivalAudioPlugin;

/// The recorded effects and ambience beds (`assets/sounds/sfx`).
#[path = "festival_recorded.rs"]
pub mod recorded;

impl Plugin for FestivalAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FestivalAudioStats>().add_systems(Startup, setup).add_systems(Update, play);
        recorded::register(app);
    }
}

/// How many festival cues were asked for (test hook, `hud_cues` in AUDIO_PASS §6).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct FestivalAudioStats {
    pub ticks: u32,
    pub banks: u32,
    pub lost: u32,
    pub radar: u32,
    pub cues: u32,
    pub money: u32,
}

fn setup(mixer: Option<ResMut<Mixer>>, cues: Option<ResMut<SoundCues>>, clips: Option<ResMut<Assets<SoundClip>>>) {
    let (Some(mut mixer), Some(mut cues), Some(mut clips)) = (mixer, cues, clips) else { return };
    mixer.add_bus("hud", BusId::UI);
    let mut add = |name: &'static str, clip: Vec<f32>, db: f32, f: fn(SoundCue) -> SoundCue| {
        let h = clips.add(SoundClip::from_mono(normalise(clip), RATE));
        cues.add(f(SoundCue::new(name).clip(h).bus("hud").volume(db)));
    };
    add("fest.tick", tick(), -22.0, |c| c.cooldown(1.0 / MAX_TICKS).max_instances(3, Steal::Oldest));
    for (k, n) in [(0, 3), (1, 4), (2, 5)] {
        add(["fest.bank.s", "fest.bank.m", "fest.bank.l"][k], bank(n), BANK_DB, |c| c.max_instances(1, Steal::Oldest));
    }
    add("fest.lost", lost(), LOST_DB, |c| c.max_instances(1, Steal::Oldest));
    add("fest.radar", shutter(), -10.0, |c| c.max_instances(1, Steal::Oldest).spatial(Attenuation { min_distance: 5.0, max_distance: 120.0, occlusion: false, doppler: 0.0, ..default() }).bus("sfx"));
    add("fest.readout.up", chime(&[0.0, 7.0], false), -16.0, |c| c.delay(0.15));
    add("fest.readout.best", chime(&[0.0, 12.0], true), -16.0, |c| c.delay(0.15));
    add("fest.readout.down", chime(&[0.0, -3.0], false), -16.0, |c| c.delay(0.15));
    add("fest.count", beep(880.0, 0.12), -14.0, |c| c);
    add("fest.go", beep(1760.0, 0.35), -14.0, |c| c);
    add("fest.checkpoint", chime(&[0.0, 5.0], false), -18.0, |c| c);
    add("fest.finish", arpeggio(&[0.0, 4.0, 7.0, 12.0]), -12.0, |c| c.max_instances(1, Steal::Oldest));
    add("fest.finish.best", arpeggio(&[0.0, 4.0, 7.0, 12.0, 19.0]), -12.0, |c| c.max_instances(1, Steal::Oldest));
    add("fest.fail", fail(), -16.0, |c| c.max_instances(1, Steal::Oldest));
    for (k, secs) in [(0, 0.45), (1, 0.75), (2, 1.2)] {
        add(["fest.money.s", "fest.money.m", "fest.money.l"][k], money(secs), -18.0, |c| c.max_instances(1, Steal::Oldest));
    }
}

#[allow(clippy::too_many_arguments)]
fn play(
    ctl: Option<ResMut<SoundCtl>>,
    mut stats: ResMut<FestivalAudioStats>,
    mut style: MessageReader<StyleEvent>,
    mut radar: MessageReader<RadarFlash>,
    mut cues: MessageReader<RunCue>,
    mut money: MessageReader<MoneyEarned>,
) {
    let Some(mut ctl) = ctl else {
        style.clear();
        radar.clear();
        cues.clear();
        money.clear();
        return;
    };
    for e in style.read() {
        match *e {
            StyleEvent::Tick { chain, .. } => {
                stats.ticks += 1;
                ctl.play_with("fest.tick", PlayParams { pitch: chain_pitch(chain), ..default() });
            }
            StyleEvent::Bank { points, chain, .. } => {
                stats.banks += 1;
                ctl.stop_cue("fest.tick", 0.02);
                let cue = if points < 1_000 {
                    "fest.bank.s"
                } else if points < 10_000 {
                    "fest.bank.m"
                } else {
                    "fest.bank.l"
                };
                ctl.play_with(cue, PlayParams { pitch: chain_pitch(chain), ..default() });
            }
            StyleEvent::Lost { .. } => {
                stats.lost += 1;
                ctl.stop_cue("fest.tick", 0.02);
                ctl.play("fest.lost");
            }
        }
    }
    for r in radar.read() {
        stats.radar += 1;
        // The recorded shutter (the synthesised one stays registered as `fest.radar`).
        ctl.play_at("rec.shutter", r.pos + Vec3::Y * 3.0);
        ctl.play(if r.personal_best {
            "fest.readout.best"
        } else if r.target_beaten {
            "fest.readout.up"
        } else {
            "fest.readout.down"
        });
    }
    for c in cues.read() {
        stats.cues += 1;
        match *c {
            RunCue::Countdown(_) => ctl.play("fest.count"),
            RunCue::Go => ctl.play("fest.go"),
            RunCue::Checkpoint => ctl.play("fest.checkpoint"),
            RunCue::Finish { place, best } => ctl.play(if place == 1 || best { "fest.finish.best" } else { "fest.finish" }),
            RunCue::Fail => ctl.play("fest.fail"),
        };
    }
    for _ in money.read() {
        stats.money += 1;
        // The recorded coins (`recorded`); the synthesised roll stays as `fest.money.*`.
        ctl.play("rec.money");
    }
}

/// A semitone up per chain step, up to an octave.
pub fn chain_pitch(chain: u8) -> f32 {
    2f32.powf((chain.max(1) - 1).min(12) as f32 / 12.0)
}

// --- Recipes (mono, 48 kHz, peak-normalised; the cue sets the level) ------------------

fn n(secs: f32) -> usize {
    (secs * RATE as f32) as usize
}

fn normalise(mut s: Vec<f32>) -> Vec<f32> {
    let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if peak > 0.0 {
        for x in &mut s {
            *x /= peak;
        }
    }
    s
}

/// Deterministic white noise.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// One-pole low-pass at `hz`.
fn lowpass(s: &mut [f32], hz: impl Fn(usize) -> f32) {
    let mut y = 0.0;
    for (i, x) in s.iter_mut().enumerate() {
        let a = 1.0 - (-TAU * hz(i) / RATE as f32).exp();
        y += a * (*x - y);
        *x = y;
    }
}

fn semis(st: f32) -> f32 {
    2f32.powf(st / 12.0)
}

/// Pluck: sine + 0.3 × 2nd harmonic, 1 ms attack, τ 60 ms.
fn tick() -> Vec<f32> {
    (0..n(0.25))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let env = (t / 0.001).min(1.0) * (-t / 0.06).exp();
            ((TAU * E5 * t).sin() + 0.3 * (TAU * 2.0 * E5 * t).sin()) * env
        })
        .collect()
}

/// FM bell (ratio 3.5, index 2 → 0 over 300 ms), `notes` notes 50 ms apart up the major
/// chord, plus a 70 Hz thump of 80 ms.
fn bank(notes: usize) -> Vec<f32> {
    let steps = [0.0, 4.0, 7.0, 12.0, 16.0];
    let mut s = vec![0.0; n(0.05 * notes as f32 + 0.9)];
    for (k, st) in steps.iter().take(notes).enumerate() {
        let f = E5 * semis(*st);
        let at = n(0.05 * k as f32);
        for (i, x) in s[at..].iter_mut().enumerate() {
            let t = i as f32 / RATE as f32;
            let index = 2.0 * (1.0 - t / 0.3).max(0.0);
            let env = (t / 0.002).min(1.0) * (-t / 0.25).exp();
            *x += (TAU * f * t + index * (TAU * f * 3.5 * t).sin()).sin() * env * 0.5;
        }
    }
    for (i, x) in s.iter_mut().take(n(0.08)).enumerate() {
        let t = i as f32 / RATE as f32;
        *x += (TAU * 70.0 * t).sin() * (1.0 - t / 0.08) * 0.8;
    }
    s
}

/// A saw gliding 600 → 150 Hz over 350 ms through a 1.2 kHz low-pass, plus a puff of noise.
fn lost() -> Vec<f32> {
    let mut noise = Noise(0x5eed_1234);
    let len = n(0.4);
    let mut phase = 0.0f32;
    let mut s: Vec<f32> = (0..len)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let f = 600.0 * (150.0f32 / 600.0).powf((t / 0.35).min(1.0));
            phase = (phase + f / RATE as f32).fract();
            let env = (t / 0.005).min(1.0) * (1.0 - t / 0.4).max(0.0);
            let puff = if t < 0.12 { noise.next() * 0.35 * (1.0 - t / 0.12) } else { 0.0 };
            (phase * 2.0 - 1.0) * env + puff
        })
        .collect();
    lowpass(&mut s, |_| 1200.0);
    s
}

/// The radar camera: two 4 ms clacks 25 ms apart (noise around 3 kHz), then the flash
/// capacitor's whine (3.2 → 8 kHz over 250 ms, 30 dB down).
fn shutter() -> Vec<f32> {
    let mut noise = Noise(0x0bad_cafe);
    let len = n(0.32);
    let mut phase = 0.0f32;
    (0..len)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let clack = |t0: f32| if (t0..t0 + 0.004).contains(&t) { (TAU * 3000.0 * t).sin() * 0.6 + 0.4 } else { 0.0 };
            let c = noise.next() * (clack(0.0) + clack(0.025));
            let wt = t - 0.03;
            let whine = if wt > 0.0 && wt < 0.25 {
                let f = 3200.0 * (8000.0f32 / 3200.0).powf(wt / 0.25);
                phase = (phase + f / RATE as f32).fract();
                (TAU * phase).sin() * 0.032 * (1.0 - wt / 0.25)
            } else {
                0.0
            };
            c + whine
        })
        .collect()
}

/// Notes (semitones from E5) 90 ms apart, triangle-ish plucks; `sparkle` adds a high shimmer.
fn chime(steps: &[f32], sparkle: bool) -> Vec<f32> {
    let mut s = vec![0.0; n(0.09 * steps.len() as f32 + 0.5)];
    for (k, st) in steps.iter().enumerate() {
        let f = E5 * semis(*st);
        let at = n(0.09 * k as f32);
        for (i, x) in s[at..].iter_mut().enumerate() {
            let t = i as f32 / RATE as f32;
            let env = (t / 0.002).min(1.0) * (-t / 0.14).exp();
            *x += ((TAU * f * t).sin() + 0.2 * (TAU * 3.0 * f * t).sin()) * env;
            if sparkle && k + 1 == steps.len() {
                *x += (TAU * f * 4.0 * t).sin() * 0.15 * (-t / 0.3).exp();
            }
        }
    }
    s
}

/// A sine-square blend, low-passed at 3 kHz.
fn beep(f: f32, secs: f32) -> Vec<f32> {
    let mut s: Vec<f32> = (0..n(secs + 0.03))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let env = (t / 0.004).min(1.0) * ((secs + 0.03 - t) / 0.03).clamp(0.0, 1.0);
            let w = (TAU * f * t).sin();
            (0.6 * w + 0.4 * w.signum()) * env
        })
        .collect();
    lowpass(&mut s, |_| 3000.0);
    s
}

/// A rising arpeggio of saws through a low-pass sweeping 800 Hz → 4 kHz (brassy), 1.2 s.
fn arpeggio(steps: &[f32]) -> Vec<f32> {
    let len = n(1.2);
    let mut s = vec![0.0; len];
    let gap = 0.11;
    for (k, st) in steps.iter().enumerate() {
        let f = E5 * 0.5 * semis(*st);
        let at = n(gap * k as f32);
        let mut phase = 0.0f32;
        for (i, x) in s[at..].iter_mut().enumerate() {
            let t = i as f32 / RATE as f32;
            phase = (phase + f / RATE as f32).fract();
            let env = (t / 0.01).min(1.0) * (-t / 0.5).exp();
            *x += (phase * 2.0 - 1.0) * env * 0.4;
        }
    }
    lowpass(&mut s, |i| 800.0 * 5f32.powf(i as f32 / len as f32));
    // Fade the tail out over the last 100 ms (no click).
    let tail = n(0.1);
    for (k, x) in s.iter_mut().rev().take(tail).enumerate() {
        *x *= k as f32 / tail as f32;
    }
    s
}

/// Two notes falling a minor third, dull (low-pass 1.5 kHz), 500 ms.
fn fail() -> Vec<f32> {
    let mut s = vec![0.0; n(0.5)];
    for (k, st) in [0.0, -3.0].iter().enumerate() {
        let f = E5 * 0.5 * semis(*st);
        let at = n(0.18 * k as f32);
        for (i, x) in s[at..].iter_mut().enumerate() {
            let t = i as f32 / RATE as f32;
            let env = (t / 0.005).min(1.0) * (-t / 0.15).exp();
            let w = (TAU * f * t).sin();
            *x += (0.7 * w + 0.3 * w.signum()) * env;
        }
    }
    lowpass(&mut s, |_| 1500.0);
    s
}

/// A roll of 6 ms ticks (partials 3.1 and 4.7 kHz) at up to 30 a second, rising 5 semitones
/// across `secs`, ending on a bell (1.3 + 2.6 kHz, τ 400 ms).
fn money(secs: f32) -> Vec<f32> {
    let mut s = vec![0.0; n(secs + 0.9)];
    let count = (secs * 30.0) as usize;
    for k in 0..count {
        let u = k as f32 / count.max(1) as f32;
        let p = semis(5.0 * u);
        let at = n(k as f32 / 30.0);
        for (i, x) in s[at..].iter_mut().take(n(0.006)).enumerate() {
            let t = i as f32 / RATE as f32;
            let env = 1.0 - t / 0.006;
            *x += ((TAU * 3100.0 * p * t).sin() + (TAU * 4700.0 * p * t).sin()) * env * 0.3;
        }
    }
    let at = n(secs);
    for (i, x) in s[at..].iter_mut().enumerate() {
        let t = i as f32 / RATE as f32;
        *x += ((TAU * 1300.0 * t).sin() + 0.5 * (TAU * 2600.0 * t).sin()) * (-t / 0.4).exp() * (t / 0.002).min(1.0) * 0.6;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipes_are_finite_and_peak_normalised() {
        for (name, clip) in [("tick", tick()), ("bank", bank(5)), ("lost", lost()), ("shutter", shutter()), ("chime", chime(&[0.0, 7.0], true)), ("beep", beep(880.0, 0.12)), ("arp", arpeggio(&[0.0, 4.0, 7.0, 12.0])), ("fail", fail()), ("money", money(1.2))] {
            let c = normalise(clip);
            assert!(c.iter().all(|x| x.is_finite()), "{name}");
            let peak = c.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!((peak - 1.0).abs() < 1e-4, "{name}: {peak}");
            assert!(c.len() > 1000, "{name}");
        }
    }

    #[test]
    fn chain_climbs_a_semitone_up_to_an_octave() {
        assert_eq!(chain_pitch(1), 1.0);
        assert!((chain_pitch(2) - semis(1.0)).abs() < 1e-5);
        assert!((chain_pitch(13) - 2.0).abs() < 1e-5);
        assert!((chain_pitch(40) - 2.0).abs() < 1e-5);
    }

    #[test]
    fn lost_is_never_louder_than_bank() {
        // Both clips are peak-normalised: the cue levels decide (AUDIO_PASS §8 item 7).
        const { assert!(LOST_DB < BANK_DB) };
        const { assert!(MAX_TICKS <= 15.0) };
    }
}
