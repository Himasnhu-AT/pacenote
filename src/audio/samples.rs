//! Recorded sounds: `assets/sounds/car/*.ogg` (sources and licences in `assets_src/SOURCES.md`),
//! decoded once into mono 48 kHz buffers and played on the audio thread by [`Looper`] (a
//! seamless loop at a live playback rate) and [`Shots`] (a small pool of one-shots).
//!
//! Loading happens off the main thread at startup ([`start_loading`]); until the bank is in,
//! every layer that reads it is silent. Unit tests load it blocking from this crate's
//! `assets/` (see [`bank`]).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::dsp::RATE;

/// Every recorded sound, by name (the file is `assets/sounds/car/<name>.ogg`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Id {
    /// Tyres sliding (loops).
    GravelSlide,
    TarmacSqueal,
    /// Body (scrape is a loop, the rest one-shots).
    Scrape,
    ImpactHeavy,
    ImpactLight,
    Snap,
    Pop1,
    Pop2,
    Pop3,
}

pub const COUNT: usize = Id::Pop3 as usize + 1;

/// File stems, by [`Id`].
pub const FILES: [&str; COUNT] = [
    "gravel_slide",
    "tarmac_squeal",
    "scrape",
    "impact_heavy",
    "impact_light",
    "snap",
    "pop1",
    "pop2",
    "pop3",
];

/// The decoded sounds (mono, [`RATE`]). A file that is missing or doesn't decode is empty, so
/// its layer is silent.
pub struct Bank {
    clips: Vec<Box<[f32]>>,
    /// Engine loops by index into [`super::powertrain::LOOPS`]: (engine bay mic, exhaust mic).
    engine: Vec<[Box<[f32]>; 2]>,
}

impl std::fmt::Debug for Bank {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Bank({} sounds + {} engine loops, {:.1} s)", self.clips.len(), self.engine.len(), self.seconds())
    }
}

/// Decodes `path` to mono at its own rate (empty, with a warning, if it's missing or bad).
pub fn load_mono(path: &Path) -> (Box<[f32]>, u32) {
    match std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| engine::sound::SoundClip::decode(&b)) {
        Ok(clip) => {
            let ch = (clip.channels() as usize).max(1);
            (clip.samples().chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect(), clip.rate())
        }
        Err(e) => {
            bevy::log::warn!("audio: can't load {}: {e}", path.display());
            (Box::default(), RATE as u32)
        }
    }
}

fn load_48k(path: &Path) -> Box<[f32]> {
    let (mono, rate) = load_mono(path);
    to_mono_48k(&mono, 1, rate)
}

impl Bank {
    /// Reads and decodes `dir/sounds/car/*.ogg` (blocking: ~2 MB of Vorbis).
    pub fn load(assets: &Path) -> Bank {
        let dir = assets.join("sounds/car");
        let clips = FILES.iter().map(|name| load_48k(&dir.join(format!("{name}.ogg")))).collect();
        let engine = super::powertrain::LOOPS
            .iter()
            .map(|(stem, _, _)| [load_48k(&dir.join(format!("engine_{stem}_bay.ogg"))), load_48k(&dir.join(format!("engine_{stem}_exh.ogg")))])
            .collect();
        Bank { clips, engine }
    }

    pub fn get(&self, id: Id) -> &[f32] {
        &self.clips[id as usize]
    }

    /// Engine loop `i` (index into [`super::powertrain::LOOPS`]): engine bay mic, exhaust mic.
    pub fn engine(&self, i: usize) -> (&[f32], &[f32]) {
        self.engine.get(i).map_or((&[], &[]), |[b, e]| (&b[..], &e[..]))
    }

    /// Every sound decoded (none missing).
    pub fn complete(&self) -> bool {
        self.clips.iter().all(|c| !c.is_empty()) && self.engine.iter().flatten().all(|c| !c.is_empty())
    }

    /// Seconds of audio held (mono, [`RATE`], f32: 192 KB a second).
    pub fn seconds(&self) -> f32 {
        let n: usize = self.clips.iter().chain(self.engine.iter().flatten()).map(|c| c.len()).sum();
        n as f32 / RATE
    }
}

/// Downmix to mono and resample (linear) to [`RATE`] if needed.
fn to_mono_48k(s: &[f32], ch: usize, rate: u32) -> Box<[f32]> {
    let ch = ch.max(1);
    let mono: Vec<f32> = s.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    if rate == RATE as u32 || mono.len() < 2 {
        return mono.into();
    }
    let step = rate as f64 / RATE as f64;
    let n = ((mono.len() - 1) as f64 / step) as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * step;
            let k = p as usize;
            let t = (p - k as f64) as f32;
            mono[k] + (mono[(k + 1).min(mono.len() - 1)] - mono[k]) * t
        })
        .collect()
}

static BANK: OnceLock<Bank> = OnceLock::new();

/// Starts decoding the bank from `assets` on a worker thread (once; later calls do nothing).
pub fn start_loading(assets: PathBuf) {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new().name("car-audio-load".into()).spawn(move || {
        let t0 = std::time::Instant::now();
        let bank = Bank::load(&assets);
        bevy::log::info!(
            "car audio: {:?} decoded in {:.0} ms ({:.1} MB)",
            bank,
            t0.elapsed().as_secs_f32() * 1000.0,
            bank.seconds() * RATE * 4.0 / 1e6
        );
        let _ = BANK.set(bank);
        super::voice::load(&assets);
    });
    if let Err(e) = spawned {
        bevy::log::warn!("car audio: can't start the loader: {e}");
    }
}

/// The bank once it has loaded. In unit tests, loads this crate's `assets/` on first use.
pub fn bank() -> Option<&'static Bank> {
    #[cfg(test)]
    {
        Some(BANK.get_or_init(|| Bank::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")))))
    }
    #[cfg(not(test))]
    {
        BANK.get()
    }
}

/// 4-point Hermite read at a fractional position, wrapping around (for loops).
#[inline]
pub(crate) fn read_wrap(x: &[f32], pos: f64) -> f32 {
    let n = x.len();
    let i = pos as usize;
    let t = (pos - i as f64) as f32;
    let at = |k: usize| x[k % n];
    let (xm, x0, x1, x2) = (at(i + n - 1), at(i), at(i + 1), at(i + 2));
    hermite(xm, x0, x1, x2, t)
}

/// 4-point Hermite read, zero outside the clip (for one-shots).
#[inline]
pub(crate) fn read_clamped(x: &[f32], pos: f64) -> f32 {
    let i = pos as isize;
    let t = (pos - i as f64) as f32;
    let at = |k: isize| if k >= 0 && (k as usize) < x.len() { x[k as usize] } else { 0.0 };
    hermite(at(i - 1), at(i), at(i + 1), at(i + 2), t)
}

#[inline]
fn hermite(xm: f32, x0: f32, x1: f32, x2: f32, t: f32) -> f32 {
    let c1 = 0.5 * (x1 - xm);
    let c2 = xm - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
    let c3 = 0.5 * (x2 - xm) + 1.5 * (x0 - x1);
    ((c3 * t + c2) * t + c1) * t + x0
}

/// A play position in a seamless loop.
#[derive(Clone, Copy, Debug, Default)]
pub struct Looper {
    pos: f64,
    /// Where to start (0..1 of the loop), applied on the first read.
    start: Option<f64>,
}

impl Looper {
    /// Starts at `frac` (0..1) of the way through (so two loopers on one clip don't comb).
    pub fn at(frac: f64) -> Self {
        Looper { pos: 0.0, start: Some(frac.rem_euclid(1.0)) }
    }

    /// The sample here, then moves on by `rate` (1 = recorded pitch).
    #[inline]
    pub fn next(&mut self, x: &[f32], rate: f32) -> f32 {
        let n = x.len();
        if n < 4 {
            return 0.0;
        }
        if let Some(f) = self.start.take() {
            self.pos = f * n as f64;
        }
        if self.pos >= n as f64 {
            self.pos %= n as f64;
        }
        let v = read_wrap(x, self.pos);
        self.pos += rate.clamp(0.0, 4.0) as f64;
        v
    }


    /// The sample `offset` (0..1 of the loop) ahead of this position, without moving (a
    /// second, decorrelated read of the same loop: the other stereo channel).
    #[inline]
    pub fn peek(&self, x: &[f32], offset: f64) -> f32 {
        let n = x.len();
        if n < 4 {
            return 0.0;
        }
        read_wrap(x, (self.pos + offset * n as f64) % n as f64)
    }
}

#[derive(Clone, Copy, Debug)]
struct Shot {
    id: Id,
    pos: f64,
    rate: f32,
    gain: f32,
    /// Left / right gains.
    lr: (f32, f32),
    on: bool,
}

/// One-shots (impacts, pops): up to `N` at once; a new one takes the oldest slot.
#[derive(Clone, Copy, Debug)]
pub struct Shots<const N: usize> {
    v: [Shot; N],
    next: usize,
}

impl<const N: usize> Default for Shots<N> {
    fn default() -> Self {
        Shots { v: [Shot { id: Id::Pop1, pos: 0.0, rate: 1.0, gain: 0.0, lr: (0.0, 0.0), on: false }; N], next: 0 }
    }
}

impl<const N: usize> Shots<N> {
    /// Plays `id` at `rate` (pitch), `gain`, pan −1..1.
    pub fn play(&mut self, id: Id, rate: f32, gain: f32, pan: f32) {
        let (l, r) = super::dsp::pan(pan);
        self.v[self.next] = Shot { id, pos: 0.0, rate: rate.clamp(0.25, 4.0), gain, lr: (l, r), on: true };
        self.next = (self.next + 1) % N;
    }

    pub fn active(&self) -> bool {
        self.v.iter().any(|s| s.on)
    }

    /// Next stereo sample of everything playing.
    #[inline]
    pub fn next(&mut self, bank: &Bank) -> (f32, f32) {
        let (mut l, mut r) = (0.0, 0.0);
        for s in self.v.iter_mut().filter(|s| s.on) {
            let x = bank.get(s.id);
            if s.pos >= x.len() as f64 {
                s.on = false;
                continue;
            }
            let v = read_clamped(x, s.pos) * s.gain;
            s.pos += s.rate as f64;
            l += v * s.lr.0;
            r += v * s.lr.1;
        }
        (l, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bank_loads_every_sound() {
        let b = bank().unwrap();
        for (i, name) in FILES.iter().enumerate() {
            assert!(b.clips[i].len() > 4800, "{name} is missing or shorter than 0.1 s");
            assert!(b.clips[i].iter().all(|v| v.is_finite() && v.abs() <= 1.0), "{name}");
        }
        for (i, (stem, _, _)) in super::super::powertrain::LOOPS.iter().enumerate() {
            let (bay, exh) = b.engine(i);
            assert!(bay.len() > 24_000 && bay.len() == exh.len(), "engine {stem}: {} / {}", bay.len(), exh.len());
            assert!(bay.iter().chain(exh).all(|v| v.is_finite() && v.abs() <= 1.0), "engine {stem}");
            // Seamless: the wrap is no bigger a step than the loop's own steps.
            for x in [bay, exh] {
                let p99 = {
                    let mut d: Vec<f32> = x.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
                    d.sort_by(f32::total_cmp);
                    d[d.len() * 99 / 100]
                };
                let wrap = (x[0] - x[x.len() - 1]).abs();
                assert!(wrap <= p99 * 1.5, "engine {stem}: wrap step {wrap} vs p99 {p99}");
            }
        }
        assert!(b.complete());
        // Memory: everything decoded stays well inside the audio budget (60 MB on Low).
        assert!(b.seconds() * RATE * 4.0 < 25e6, "{b:?}");
    }

    #[test]
    fn looper_wraps_and_interpolates() {
        let x: Vec<f32> = (0..100).map(|i| (i as f32 / 100.0 * std::f32::consts::TAU).sin()).collect();
        let mut l = Looper::default();
        let out: Vec<f32> = (0..1000).map(|_| l.next(&x, 0.37)).collect();
        // A sine read slowly through a seamless loop stays smooth across the wrap.
        let worst = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(worst < 0.03, "{worst}");
        assert!(Looper::default().next(&[], 1.0) == 0.0);
    }

    #[test]
    fn shots_play_once() {
        let b = bank().unwrap();
        let mut s = Shots::<4>::default();
        s.play(Id::ImpactLight, 1.0, 1.0, 0.0);
        let n = b.get(Id::ImpactLight).len();
        let mut e = 0.0;
        for _ in 0..n + 10 {
            let (l, r) = s.next(b);
            e += l * l + r * r;
        }
        assert!(e > 0.0 && !s.active());
    }
}
