//! The body and chassis: impacts, the body hitting the ground and snapping saplings and posts
//! (recorded one-shots), scraping along the ground (a recorded loop), suspension thumps and
//! bottoming out, and rattles (loose parts when the suspension is damaged, a little trim rattle
//! on rough ground anyway; these are synthesised).

use super::car::CarParams;
use super::dsp::{Biquad, Decay, Noise, RATE, Ring, Saw, Smooth, Wander, pan};
use super::samples::{Bank, Id, Looper, Shots};

/// What the body sends out (stereo; heard both inside and out).
#[derive(Clone, Copy, Debug, Default)]
pub struct BodyOut {
    pub l: f32,
    pub r: f32,
    /// The scrape and rattles alone (louder inside: they're the car's own shell).
    pub shell: f32,
}

#[derive(Clone, Debug)]
pub struct Body {
    n: Noise,
    hits: Shots<4>,
    /// Bits falling off and settling after a big hit (rate per second, decays).
    debris: f32,
    thumps: [(Ring, Decay, f32); 4],
    next_thump: usize,
    bottoms: [(Ring, Ring, Decay, f32); 2],
    next_bottom: usize,
    tinkles: [(Ring, f32); 6],
    next_tinkle: usize,
    clunk: Ring,
    clunk_tick: Ring,
    scrape: Smooth,
    scrape_loop: Looper,
    rustle: Smooth,
    rustle_bp: Biquad,
    rustle_lo: Biquad,
    rustle_am: Wander,
    rough: Smooth,
    rattles: [Ring; 5],
    next_rattle: usize,
    buzz: Saw,
    buzz_bp: Biquad,
    buzz_env: Decay,
}

impl Body {
    pub fn new(seed: u32) -> Self {
        Body {
            n: Noise::new(seed ^ 0x4444),
            hits: Shots::default(),
            debris: 0.0,
            thumps: [(Ring::default(), Decay::default(), 0.0); 4],
            next_thump: 0,
            bottoms: [(Ring::default(), Ring::default(), Decay::default(), 0.0); 2],
            next_bottom: 0,
            tinkles: [(Ring::default(), 0.0); 6],
            next_tinkle: 0,
            clunk: Ring::default(),
            clunk_tick: Ring::default(),
            scrape: Smooth::new(0.0, 0.02, 0.08, RATE),
            scrape_loop: Looper::default(),
            rustle: Smooth::new(0.0, 0.03, 0.15, RATE),
            rustle_bp: Biquad::bandpass(3200.0, 0.8),
            rustle_lo: Biquad::bandpass(700.0, 0.7),
            rustle_am: Wander::new(45.0),
            rough: Smooth::new(0.0, 0.05, 0.3, RATE),
            rattles: [Ring::default(); 5],
            next_rattle: 0,
            buzz: Saw::default(),
            buzz_bp: Biquad::bandpass(1600.0, 2.0),
            buzz_env: Decay::default(),
        }
    }

    /// Hit a tree/rock/post at `v` m/s (closing speed), or the ground (`metal` false: darker,
    /// slower): a recorded crash, then debris settling. Bigger hits are lower and louder; small
    /// ones are a lighter clank.
    pub fn impact(&mut self, v: f32, metal: bool) {
        let a = (v / 12.0).clamp(0.05, 1.0);
        let n = &mut self.n;
        let pan = n.range(-0.4, 0.4);
        let pitch = if metal { 1.0 } else { 0.72 } * n.range(0.92, 1.08);
        if a > 0.35 {
            self.hits.play(Id::ImpactHeavy, pitch * (1.12 - 0.25 * a), 0.9 * a.sqrt(), pan);
        }
        // The light clank on its own for small hits, under the crash for big ones.
        self.hits.play(Id::ImpactLight, pitch * n.range(0.9, 1.1), 0.7 * a.sqrt() * if a > 0.35 { 0.5 } else { 1.0 }, pan);
        if v > 6.0 {
            self.debris = self.debris.max(40.0 * a);
        }
    }

    /// Snapping a sapling, post or sign at `v` m/s: a woody crack, a few chips rattling off
    /// the bodywork.
    pub fn snap(&mut self, v: f32) {
        let a = (v / 20.0).clamp(0.2, 1.0);
        let pan = self.n.range(-0.5, 0.5);
        let rate = self.n.range(0.85, 1.15);
        self.hits.play(Id::Snap, rate, 0.6 * a.sqrt(), pan);
        self.debris = self.debris.max(10.0 * a);
    }

    /// Suspension slam: strength 0..1, pan −1..1.
    pub fn thump(&mut self, s: f32, p: f32) {
        let s = s.clamp(0.0, 1.0);
        let t = &mut self.thumps[self.next_thump];
        self.next_thump = (self.next_thump + 1) % 4;
        t.0.trigger(self.n.range(55.0, 80.0), 0.18, 0.4 * s);
        t.1.trigger(0.12 * s, 0.02);
        t.2 = p.clamp(-1.0, 1.0) * 0.5;
    }

    /// The suspension ran out of travel: a hard metallic clonk on the bump stop.
    pub fn bottom(&mut self, s: f32, p: f32) {
        let s = s.clamp(0.0, 1.0);
        let n = &mut self.n;
        let b = &mut self.bottoms[self.next_bottom];
        self.next_bottom = (self.next_bottom + 1) % 2;
        b.0.trigger(n.range(85.0, 120.0), 0.12, 0.45 * s);
        b.1.trigger(n.range(780.0, 1100.0), 0.07, 0.07 * s);
        b.2.trigger(0.2 * s, 0.03);
        b.3 = p.clamp(-1.0, 1.0) * 0.5;
    }

    /// Gear change: the dog box clunks.
    pub fn shift(&mut self) {
        self.clunk.trigger(self.n.range(150.0, 190.0), 0.08, 0.06);
        self.clunk_tick.trigger(self.n.range(2600.0, 3100.0), 0.03, 0.025);
    }

    #[inline]
    pub fn sample(&mut self, p: &CarParams, bank: Option<&Bank>) -> BodyOut {
        let n = &mut self.n;
        let (mut l, mut r) = match bank {
            Some(b) => self.hits.next(b),
            None => (0.0, 0.0),
        };
        if self.debris > 0.05 {
            if n.chance(self.debris / RATE) {
                let t = &mut self.tinkles[self.next_tinkle];
                self.next_tinkle = (self.next_tinkle + 1) % 6;
                t.0.trigger(n.range(1800.0, 5200.0), n.range(0.02, 0.06), n.range(0.004, 0.02) * (self.debris / 40.0).min(1.0));
                t.1 = n.range(-0.7, 0.7);
            }
            self.debris *= 1.0 - 4.0 / RATE;
        }
        for (ring, px) in &mut self.tinkles {
            if ring.active() {
                let s = ring.next();
                l += s * (1.0 - *px);
                r += s * (1.0 + *px);
            }
        }
        for t in &mut self.thumps {
            if t.0.active() || t.1.active() {
                let s = t.0.tick() + n.white() * t.1.next();
                let (a, b) = pan(t.2);
                l += s * a;
                r += s * b;
            }
        }
        for b in &mut self.bottoms {
            if b.0.active() || b.2.active() {
                let s = b.0.tick() + b.1.tick() + n.white() * b.2.next();
                let (a, bb) = pan(b.3);
                l += s * a;
                r += s * bb;
            }
        }
        let clunk = self.clunk.tick() + self.clunk_tick.tick();
        l += clunk;
        r += clunk;
        // Scrape: the body sliding on the ground (recorded metal scraping, faster with speed).
        let sc = self.scrape.next((p.scrape / 15.0).min(1.5));
        let mut shell = 0.0;
        if sc > 1e-4
            && let Some(b) = bank
        {
            shell += self.scrape_loop.next(b.get(Id::Scrape), 0.8 + 0.3 * sc) * sc * 0.3;
        }
        // Brush: leaves and twigs swishing along the sides, crackling with the branches.
        let ru = self.rustle.next((p.brush / 12.0).min(1.2));
        if ru > 1e-4 {
            let am = (0.35 + 0.65 * self.rustle_am.next(n)).max(0.0);
            let wn = n.white() * am;
            let s = (self.rustle_bp.run(wn) * 0.22 + self.rustle_lo.run(wn) * 0.1) * ru;
            l += s;
            r += s;
        }
        // Rattles: rough ground shakes loose parts (much more with damaged suspension).
        let rough = self.rough.next(p.rough.min(3.0));
        let loose = 0.15 + 2.5 * p.susp_damage;
        let rate = rough * loose * 30.0 / RATE;
        if n.chance(rate) {
            let ring = &mut self.rattles[self.next_rattle];
            self.next_rattle = (self.next_rattle + 1) % 5;
            ring.trigger(n.range(320.0, 1250.0), n.range(0.01, 0.035), n.range(0.006, 0.02) * (0.4 + p.susp_damage));
            // A loose heat shield buzzes against the exhaust.
            if p.susp_damage > 0.3 && n.chance(0.3) {
                self.buzz_env.trigger(0.02 * p.susp_damage, 0.06);
            }
        }
        for ring in &mut self.rattles {
            shell += ring.tick();
        }
        if self.buzz_env.active() {
            shell += self.buzz_bp.run(self.buzz.next(95.0)) * self.buzz_env.next();
        } else {
            self.buzz_bp.settle();
        }
        BodyOut { l: l + shell, r: r + shell, shell }
    }
}
