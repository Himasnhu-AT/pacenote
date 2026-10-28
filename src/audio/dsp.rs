//! Small DSP building blocks for the car and stage sounds. Pure, allocation-free, `Copy` where
//! they can be, and deterministic (seeded noise), so the synthesis is unit-tested offline.

// `next()` on envelopes and resonators reads as "next sample"; they aren't iterators.
#![allow(clippy::should_implement_trait)]

use std::f32::consts::{PI, TAU};

/// Output sample rate of the rally stream (the engine mixer runs at the same rate).
pub const RATE: f32 = 48_000.0;

/// Xorshift32 noise.
#[derive(Clone, Copy, Debug)]
pub struct Noise(u32);

impl Noise {
    pub fn new(seed: u32) -> Self {
        Noise(seed.wrapping_mul(0x9E37_79B9) | 1)
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in [-1, 1).
    #[inline]
    pub fn white(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (2.0 / 16_777_216.0) - 1.0
    }

    /// Uniform in [0, 1).
    #[inline]
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// Uniform in [a, b).
    #[inline]
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.unit()
    }

    /// True with probability `p` (per call).
    #[inline]
    pub fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }
}

/// One-pole low-pass (6 dB/oct); `hp` gives the complementary high-pass.
#[derive(Clone, Copy, Debug, Default)]
pub struct OnePole {
    a: f32,
    z: f32,
}

impl OnePole {
    pub fn new(hz: f32) -> Self {
        let mut f = OnePole::default();
        f.set(hz);
        f
    }

    pub fn set(&mut self, hz: f32) {
        self.a = 1.0 - (-TAU * hz.clamp(1.0, RATE * 0.45) / RATE).exp();
    }

    #[inline]
    pub fn lp(&mut self, x: f32) -> f32 {
        self.z += self.a * (x - self.z);
        self.z
    }

    #[inline]
    pub fn hp(&mut self, x: f32) -> f32 {
        x - self.lp(x)
    }

    pub fn coeff(&self) -> f32 {
        self.a
    }
}

/// RBJ biquad, transposed direct form II.
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn coeffs(hz: f32, q: f32) -> (f32, f32, f32) {
        let w = TAU * hz.clamp(10.0, RATE * 0.45) / RATE;
        let (s, c) = w.sin_cos();
        (s, c, s / (2.0 * q.max(0.05)))
    }

    fn set_norm(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        let k = 1.0 / a0;
        self.b0 = b0 * k;
        self.b1 = b1 * k;
        self.b2 = b2 * k;
        self.a1 = a1 * k;
        self.a2 = a2 * k;
    }

    pub fn lowpass(hz: f32, q: f32) -> Self {
        let mut b = Biquad::default();
        b.set_lowpass(hz, q);
        b
    }

    pub fn bandpass(hz: f32, q: f32) -> Self {
        let mut b = Biquad::default();
        b.set_bandpass(hz, q);
        b
    }

    pub fn highpass(hz: f32, q: f32) -> Self {
        let mut b = Biquad::default();
        b.set_highpass(hz, q);
        b
    }

    pub fn set_lowpass(&mut self, hz: f32, q: f32) {
        let (_, c, al) = Self::coeffs(hz, q);
        self.set_norm((1.0 - c) * 0.5, 1.0 - c, (1.0 - c) * 0.5, 1.0 + al, -2.0 * c, 1.0 - al);
    }

    /// Band-pass with 0 dB peak gain.
    pub fn set_bandpass(&mut self, hz: f32, q: f32) {
        let (_, c, al) = Self::coeffs(hz, q);
        self.set_norm(al, 0.0, -al, 1.0 + al, -2.0 * c, 1.0 - al);
    }

    pub fn set_highpass(&mut self, hz: f32, q: f32) {
        let (_, c, al) = Self::coeffs(hz, q);
        self.set_norm((1.0 + c) * 0.5, -(1.0 + c), (1.0 + c) * 0.5, 1.0 + al, -2.0 * c, 1.0 - al);
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    /// Clears the state (e.g. after a long silence; also guards against denormals).
    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// Exponential approach to a target (parameter smoothing, envelopes with attack/release).
#[derive(Clone, Copy, Debug, Default)]
pub struct Smooth {
    pub v: f32,
    up: f32,
    down: f32,
}

impl Smooth {
    /// Time constants in seconds for rising and falling, at `rate` updates per second.
    pub fn new(v: f32, up_secs: f32, down_secs: f32, rate: f32) -> Self {
        let k = |t: f32| if t <= 0.0 { 1.0 } else { 1.0 - (-1.0 / (t * rate)).exp() };
        Smooth { v, up: k(up_secs), down: k(down_secs) }
    }

    #[inline]
    pub fn next(&mut self, target: f32) -> f32 {
        let k = if target > self.v { self.up } else { self.down };
        self.v += k * (target - self.v);
        self.v
    }
}

/// Exponentially decaying amplitude (one-shot envelopes).
#[derive(Clone, Copy, Debug, Default)]
pub struct Decay {
    pub v: f32,
    k: f32,
}

impl Decay {
    /// Jump to `amp`, then fall by 60 dB over `secs`.
    pub fn trigger(&mut self, amp: f32, secs: f32) {
        self.v = amp;
        self.k = (-6.9 / (secs.max(1e-3) * RATE)).exp();
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        let v = self.v;
        self.v *= self.k;
        if self.v < 1e-5 {
            self.v = 0.0;
        }
        v
    }

    pub fn active(&self) -> bool {
        self.v > 0.0
    }
}

/// Damped sine (a recursive two-pole resonator): stone pings, metal rings, thumps.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ring {
    c: f32,
    r2: f32,
    y1: f32,
    y2: f32,
}

impl Ring {
    /// Starts a ring at `hz` that falls by 60 dB over `secs`, with peak `amp`.
    pub fn trigger(&mut self, hz: f32, secs: f32, amp: f32) {
        let w = TAU * hz.clamp(10.0, RATE * 0.45) / RATE;
        let r = (-6.9 / (secs.max(1e-3) * RATE)).exp();
        self.c = 2.0 * r * w.cos();
        self.r2 = r * r;
        // State chosen so the output is amp·rⁿ·sin(n·w) from n = 0.
        self.y1 = -amp * w.sin() / r;
        self.y2 = -amp * (2.0 * w).sin() / (r * r);
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        let y = self.c * self.y1 - self.r2 * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub fn active(&self) -> bool {
        self.y1.abs() + self.y2.abs() > 1e-5
    }

    pub fn stop(&mut self) {
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// Sine of a phase in turns (0..1), parabolic approximation (error < 0.1 %).
#[inline]
pub fn sin01(p: f32) -> f32 {
    let x = (p - p.floor()) * 2.0 - 1.0; // −1..1 ↔ −π..π
    let y = 4.0 * x * (1.0 - x.abs());
    -(0.225 * (y * y.abs() - y) + y)
}

/// Phase-accumulator sine oscillator.
#[derive(Clone, Copy, Debug, Default)]
pub struct Osc {
    pub phase: f32,
}

impl Osc {
    #[inline]
    pub fn next(&mut self, hz: f32) -> f32 {
        let s = sin01(self.phase);
        self.phase += hz / RATE;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        s
    }
}

/// Smooth random wander in −1..1 (low-passed noise, renormalised): gusts, vibrato, idle wobble.
#[derive(Clone, Copy, Debug)]
pub struct Wander {
    a: OnePole,
    b: OnePole,
    gain: f32,
}

impl Wander {
    pub fn new(hz: f32) -> Self {
        let a = OnePole::new(hz);
        // Two cascaded one-poles on white noise have std ≈ sqrt(a/2)·0.58; undo it.
        let gain = 1.0 / ((a.coeff() * 0.25).sqrt() * 0.58);
        Wander { a, b: OnePole::new(hz), gain }
    }

    #[inline]
    pub fn next(&mut self, n: &mut Noise) -> f32 {
        let x = self.a.lp(n.white());
        (self.b.lp(x) * self.gain).clamp(-1.0, 1.0)
    }
}

/// Soft clip: unity near zero, limits to ±1.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

/// Hermite smoothstep of `x` between `a` and `b`.
#[inline]
pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Equal-power pan: −1 left .. 1 right → (left, right) gains.
#[inline]
pub fn pan(p: f32) -> (f32, f32) {
    let a = (p.clamp(-1.0, 1.0) + 1.0) * 0.25 * PI;
    (a.cos() * std::f32::consts::SQRT_2, a.sin() * std::f32::consts::SQRT_2)
}

/// Replaces NaN/inf with 0 (the stream must never emit garbage).
#[inline]
pub fn finite(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

/// Goertzel power of `freq` in `x` (normalised by length): for spectrum checks in tests.
pub fn goertzel(x: &[f32], freq: f32) -> f32 {
    let w = TAU * freq / RATE;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s = v as f64 + c as f64 * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    let p = s1 * s1 + s2 * s2 - c as f64 * s1 * s2;
    (p / (x.len() as f64 * x.len() as f64)) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn noise_is_deterministic_and_bounded() {
        let (mut a, mut b) = (Noise::new(7), Noise::new(7));
        let mut mean = 0.0;
        for _ in 0..100_000 {
            let (x, y) = (a.white(), b.white());
            assert_eq!(x, y);
            assert!((-1.0..1.0).contains(&x));
            mean += x;
        }
        assert!((mean / 100_000.0).abs() < 0.01);
        assert_ne!(Noise::new(1).white(), Noise::new(2).white());
    }

    #[test]
    fn sin01_matches_sin() {
        for i in 0..1000 {
            let p = i as f32 / 1000.0;
            assert!((sin01(p) - (p * TAU).sin()).abs() < 2e-3, "{p}");
        }
    }

    #[test]
    fn biquad_bandpass_passes_centre_rejects_far() {
        let tone = |hz: f32| -> f32 {
            let mut f = Biquad::bandpass(1000.0, 2.0);
            let mut o = Osc::default();
            let out: Vec<f32> = (0..9600).map(|_| f.run(o.next(hz))).collect();
            rms(&out[4800..]) / std::f32::consts::FRAC_1_SQRT_2
        };
        assert!((tone(1000.0) - 1.0).abs() < 0.05);
        assert!(tone(100.0) < 0.1);
        assert!(tone(10_000.0) < 0.1);
        let mut lp = Biquad::lowpass(500.0, 0.707);
        let mut hp = Biquad::highpass(500.0, 0.707);
        let mut o = Osc::default();
        let (mut l, mut h) = (0.0f32, 0.0f32);
        for i in 0..9600 {
            let x = o.next(5000.0);
            let (a, b) = (lp.run(x), hp.run(x));
            if i > 4800 {
                l = l.max(a.abs());
                h = h.max(b.abs());
            }
        }
        assert!(l < 0.02 && h > 0.95, "lp {l} hp {h}");
    }

    #[test]
    fn ring_decays_at_its_frequency() {
        let mut r = Ring::default();
        r.trigger(2000.0, 0.1, 0.5);
        let out: Vec<f32> = (0..4800).map(|_| r.next()).collect();
        assert!(out[0].abs() < 1e-6);
        let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.03, "{peak}");
        // −60 dB at 0.1 s.
        let late = out[4700..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(late < 0.5e-3 * 1.5, "{late}");
        assert!(goertzel(&out[..1200], 2000.0) > 20.0 * goertzel(&out[..1200], 3000.0));
        assert!(!r.active() || late < 1e-3);
    }

    #[test]
    fn envelopes_and_smoothing() {
        let mut d = Decay::default();
        d.trigger(1.0, 0.05);
        let v: Vec<f32> = (0..4800).map(|_| d.next()).collect();
        assert_eq!(v[0], 1.0);
        assert!(v[2400] < 1.2e-3);
        assert!(!d.active());
        let mut s = Smooth::new(0.0, 0.01, 0.1, RATE);
        for _ in 0..4800 {
            s.next(1.0);
        }
        assert!(s.v > 0.999);
        for _ in 0..480 {
            s.next(0.0);
        }
        assert!(s.v > 0.85, "slow release {}", s.v);
    }

    #[test]
    fn wander_is_bounded_and_moves() {
        let mut w = Wander::new(2.0);
        let mut n = Noise::new(3);
        let v: Vec<f32> = (0..96_000).map(|_| w.next(&mut n)).collect();
        assert!(v.iter().all(|x| (-1.0..=1.0).contains(x)));
        let r = rms(&v);
        assert!(r > 0.15 && r < 0.9, "{r}");
    }

    #[test]
    fn clip_and_pan() {
        assert!((soft_clip(0.1) - 0.1).abs() < 2e-3);
        assert!(soft_clip(10.0) <= 1.0 && soft_clip(-10.0) >= -1.0);
        let (l, r) = pan(0.0);
        assert!((l - 1.0).abs() < 1e-5 && (r - 1.0).abs() < 1e-5);
        let (l, r) = pan(-1.0);
        assert!(r.abs() < 1e-5 && l > 1.4);
        assert_eq!(finite(f32::NAN), 0.0);
    }
}
