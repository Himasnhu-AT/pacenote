//! The body and chassis: impacts, the body hitting the ground, suspension thumps and
//! bottoming out, scraping along the ground, and rattles (loose parts when the suspension is
//! damaged, a little trim rattle on rough ground anyway).

use super::car::CarParams;
use super::dsp::{Biquad, Decay, Noise, RATE, Ring, Saw, Smooth, Wander, pan};

#[derive(Clone, Copy, Debug, Default)]
struct Hit {
    thud: Ring,
    metal: [Ring; 3],
    noise: Decay,
    panel: Biquad,
    crunch: Biquad,
    /// Bits falling off and settling after a big hit (rate per second, decays).
    debris: f32,
    pan: f32,
}

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
    hits: [Hit; 3],
    next_hit: usize,
    thumps: [(Ring, Decay, f32); 4],
    next_thump: usize,
    bottoms: [(Ring, Ring, Decay, f32); 2],
    next_bottom: usize,
    tinkles: [(Ring, f32); 6],
    next_tinkle: usize,
    clunk: Ring,
    clunk_tick: Ring,
    scrape: Smooth,
    scrape_bp: Biquad,
    scrape_hi: Biquad,
    scrape_am: Wander,
    scrape_ring: [Biquad; 2],
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
            hits: [Hit::default(); 3],
            next_hit: 0,
            thumps: [(Ring::default(), Decay::default(), 0.0); 4],
            next_thump: 0,
            bottoms: [(Ring::default(), Ring::default(), Decay::default(), 0.0); 2],
            next_bottom: 0,
            tinkles: [(Ring::default(), 0.0); 6],
            next_tinkle: 0,
            clunk: Ring::default(),
            clunk_tick: Ring::default(),
            scrape: Smooth::new(0.0, 0.02, 0.08, RATE),
            scrape_bp: Biquad::bandpass(1400.0, 0.9),
            scrape_hi: Biquad::highpass(3500.0, 0.7),
            scrape_am: Wander::new(30.0),
            scrape_ring: [Biquad::bandpass(1730.0, 25.0), Biquad::bandpass(3310.0, 30.0)],
            rough: Smooth::new(0.0, 0.05, 0.3, RATE),
            rattles: [Ring::default(); 5],
            next_rattle: 0,
            buzz: Saw::default(),
            buzz_bp: Biquad::bandpass(1600.0, 2.0),
            buzz_env: Decay::default(),
        }
    }

    /// Hit a tree/rock/post at `v` m/s (closing speed): a deep thud, a crunch of panels,
    /// metal rings, then debris settling. Bigger hits are lower, longer and louder.
    pub fn impact(&mut self, v: f32, metal: bool) {
        let a = (v / 12.0).clamp(0.05, 1.0);
        let n = &mut self.n;
        let h = &mut self.hits[self.next_hit];
        self.next_hit = (self.next_hit + 1) % 3;
        h.thud.trigger(n.range(38.0, 52.0) * (1.25 - 0.3 * a), 0.3 + 0.35 * a, 0.65 * a.sqrt());
        let m = if metal { 1.0 } else { 0.35 };
        for (r, f) in h.metal.iter_mut().zip([1130.0, 2370.0, 3710.0]) {
            r.trigger(f * n.range(0.88, 1.12), n.range(0.2, 0.45), 0.045 * a * m * n.range(0.5, 1.0));
        }
        h.noise.trigger(0.5 * a, 0.08 + 0.2 * a);
        h.panel.set_bandpass(n.range(650.0, 950.0), 1.1);
        h.crunch.set_bandpass(n.range(2200.0, 3200.0), 0.9);
        h.debris = if v > 6.0 { 40.0 * a } else { 0.0 };
        h.pan = n.range(-0.4, 0.4);
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
    pub fn sample(&mut self, p: &CarParams) -> BodyOut {
        let n = &mut self.n;
        let (mut l, mut r) = (0.0, 0.0);
        for h in &mut self.hits {
            if h.noise.active() || h.thud.active() {
                let nz = n.white() * h.noise.next();
                let m: f32 = h.metal.iter_mut().map(|m| m.tick()).sum();
                let s = h.thud.tick() + h.panel.run(nz) * 1.3 + h.crunch.run(nz) * 0.8 + nz * 0.15 + m;
                let (a, b) = pan(h.pan);
                l += s * a;
                r += s * b;
            } else {
                h.panel.settle();
                h.crunch.settle();
            }
            if h.debris > 0.05 {
                if n.chance(h.debris / RATE) {
                    let t = &mut self.tinkles[self.next_tinkle];
                    self.next_tinkle = (self.next_tinkle + 1) % 6;
                    t.0.trigger(n.range(1800.0, 5200.0), n.range(0.02, 0.06), n.range(0.004, 0.02) * (h.debris / 40.0).min(1.0));
                    t.1 = n.range(-0.7, 0.7);
                }
                h.debris *= 1.0 - 4.0 / RATE;
            }
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
        // Scrape: the body sliding on the ground, gritty noise with a couple of panel modes
        // ringing.
        let sc = self.scrape.next((p.scrape / 15.0).min(1.5));
        let mut shell = 0.0;
        if sc > 1e-4 {
            let am = (0.55 + 0.45 * self.scrape_am.next(n)).max(0.0);
            let wn = n.white() * am;
            let ring = self.scrape_ring[0].run(wn) + self.scrape_ring[1].run(wn);
            shell += (self.scrape_bp.run(wn) * 0.16 + self.scrape_hi.run(wn) * 0.05 + ring * 0.35) * sc;
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
