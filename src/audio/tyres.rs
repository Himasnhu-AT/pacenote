//! Tyres on each surface class, rolling and sliding, from recorded loops (see
//! [`super::samples`]), plus stones thrown at the arches and the floor (synthesised: short
//! resonant pings and thuds).
//!
//! | Class | Rolling | Sliding |
//! |---|---|---|
//! | gravel | gravel crunch under slow tyres, sped up with the car | dirt skids |
//! | tarmac | tyre roar on a highway | tyre squeal |
//! | snow | tyres creeping over snow and ice | the same, faster and harder, plus some dirt skid |
//! | mud | the gravel crunch, slowed and darkened | the skid, darkened |
//! | off-road | the gravel crunch, darker, with a bumpy rumble | the skid, darker |
//!
//! Every loop is stereo by reading it twice, half a loop apart (decorrelated channels).

use super::car::{CarParams, GRAVEL, MUD, OFFROAD, SNOW, SURFACES, TARMAC};
use super::dsp::{Biquad, Noise, RATE, Ring, Smooth, Wander, smoothstep};
use super::samples::{Bank, Id, Looper};

/// Short resonant clicks with a pan each (stones).
#[derive(Clone, Copy, Debug)]
struct Grains<const N: usize> {
    v: [(Ring, f32); N],
    next: usize,
}

impl<const N: usize> Default for Grains<N> {
    fn default() -> Self {
        Grains { v: [(Ring::default(), 0.0); N], next: 0 }
    }
}

impl<const N: usize> Grains<N> {
    fn trigger(&mut self, hz: f32, secs: f32, amp: f32, pan_x: f32) {
        let g = &mut self.v[self.next];
        self.next = (self.next + 1) % N;
        g.0.trigger(hz, secs, amp);
        g.1 = pan_x;
    }

    #[inline]
    fn run(&mut self) -> (f32, f32) {
        let (mut l, mut r) = (0.0, 0.0);
        for (ring, px) in &mut self.v {
            if ring.active() {
                let s = ring.next();
                l += s * (1.0 - *px);
                r += s * (1.0 + *px);
            }
        }
        (l, r)
    }
}

/// What the tyres send out.
#[derive(Clone, Copy, Debug, Default)]
pub struct TyreOut {
    /// Air-borne tyre noise (muffled inside the car).
    pub l: f32,
    pub r: f32,
    /// Stones hitting the floor and arches (structure-borne: louder inside).
    pub floor_l: f32,
    pub floor_r: f32,
}

/// One recorded loop, stereo, with a low-pass per channel (for the darker surfaces).
#[derive(Clone, Copy, Debug)]
struct Layer {
    id: Id,
    pos: Looper,
    lp: [Biquad; 2],
    filtered: bool,
}

impl Layer {
    fn new(id: Id, start: f64, lowpass: Option<f32>) -> Self {
        let hz = lowpass.unwrap_or(20_000.0);
        Layer { id, pos: Looper::at(start), lp: [Biquad::lowpass(hz, 0.7); 2], filtered: lowpass.is_some() }
    }

    /// Next stereo sample at `rate`, scaled by `gain` (silent and not moving below a floor).
    #[inline]
    fn next(&mut self, bank: &Bank, rate: f32, gain: f32) -> (f32, f32) {
        if gain < 1e-4 {
            if self.filtered {
                self.lp[0].settle();
                self.lp[1].settle();
            }
            return (0.0, 0.0);
        }
        let x = bank.get(self.id);
        let r = self.pos.peek(x, 0.5);
        let l = self.pos.next(x, rate);
        if self.filtered {
            (self.lp[0].run(l) * gain, self.lp[1].run(r) * gain)
        } else {
            (l * gain, r * gain)
        }
    }
}

#[derive(Clone, Debug)]
pub struct Tyres {
    n: Noise,
    pub roll: [Smooth; SURFACES],
    pub slide: [Smooth; SURFACES],
    gravel_roll: Layer,
    gravel_slide: Layer,
    tarmac_roll: Layer,
    squeal: Layer,
    snow_roll: Layer,
    snow_slide: Layer,
    mud_roll: Layer,
    mud_slide: Layer,
    off_roll: Layer,
    off_slide: Layer,
    squeal_wander: Wander,
    rumble: Biquad,
    rumble_wander: Wander,
    pings: Grains<6>,
    thunks: Grains<4>,
    thunk_body: Biquad,
}

impl Tyres {
    pub fn new(seed: u32) -> Self {
        let sm = |up, down| Smooth::new(0.0, up, down, RATE);
        let mut n = Noise::new(seed ^ 0x3333);
        let mut at = |id, lp| Layer::new(id, n.unit() as f64, lp);
        Tyres {
            gravel_roll: at(Id::GravelRoll, None),
            gravel_slide: at(Id::GravelSlide, None),
            tarmac_roll: at(Id::TarmacRoll, None),
            squeal: at(Id::TarmacSqueal, None),
            snow_roll: at(Id::SnowRoll, None),
            snow_slide: at(Id::GravelSlide, Some(2400.0)),
            mud_roll: at(Id::GravelRoll, Some(650.0)),
            mud_slide: at(Id::GravelSlide, Some(900.0)),
            off_roll: at(Id::GravelRoll, Some(1800.0)),
            off_slide: at(Id::GravelSlide, Some(1600.0)),
            n,
            roll: [sm(0.05, 0.08); SURFACES],
            slide: [sm(0.03, 0.09); SURFACES],
            squeal_wander: Wander::new(3.0),
            rumble: Biquad::lowpass(110.0, 0.8),
            rumble_wander: Wander::new(6.0),
            pings: Grains::default(),
            thunks: Grains::default(),
            thunk_body: Biquad::bandpass(420.0, 1.4),
        }
    }

    #[inline]
    pub fn sample(&mut self, p: &CarParams, speed: f32, bank: Option<&Bank>) -> TyreOut {
        let mut roll = [0.0f32; SURFACES];
        let mut slide = [0.0f32; SURFACES];
        for i in 0..SURFACES {
            roll[i] = self.roll[i].next(p.surfaces[i].roll);
            slide[i] = self.slide[i].next(p.surfaces[i].slide);
        }
        let n = &mut self.n;
        let fast = smoothstep(3.0, 15.0, speed);
        // Rolling recordings were made slowly: play them faster with the car.
        let v = (speed / 25.0).min(1.6);
        let (mut l, mut r) = (0.0, 0.0);
        let mut add = |(a, b): (f32, f32)| {
            l += a;
            r += b;
        };
        let (gr, gs) = (roll[GRAVEL], slide[GRAVEL]);
        let (tr, ts) = (roll[TARMAC], slide[TARMAC]);
        let (sr, ss) = (roll[SNOW], slide[SNOW]);
        let (mr, ms) = (roll[MUD], slide[MUD]);
        let (or, os) = (roll[OFFROAD], slide[OFFROAD]);
        if let Some(bank) = bank {
            add(self.gravel_roll.next(bank, 0.8 + 0.45 * v, gr * 0.45));
            add(self.gravel_slide.next(bank, 0.9 + 0.1 * gs.min(2.0), gs * 0.26));
            add(self.tarmac_roll.next(bank, 0.8 + 0.3 * v, tr * tr.min(1.0).sqrt() * 0.26));
            // Squeal past the grip peak; its pitch wanders and rises a little with slip.
            let sq = smoothstep(0.2, 0.9, ts);
            let wob = self.squeal_wander.next(n);
            add(self.squeal.next(bank, 0.94 + 0.05 * ts.min(2.0) + 0.03 * wob, sq * 0.3));
            add(self.snow_roll.next(bank, 0.8 + 0.4 * v + 0.25 * ss.min(1.5), sr * 0.45 + ss * 0.3));
            add(self.snow_slide.next(bank, 1.0, ss * 0.1));
            add(self.mud_roll.next(bank, 0.6 + 0.25 * v, (mr + 0.4 * ms) * 0.5));
            add(self.mud_slide.next(bank, 0.75, ms * 0.3));
            add(self.off_roll.next(bank, 0.7 + 0.35 * v, or * 0.3));
            add(self.off_slide.next(bank, 0.85, os * 0.5));
        }
        // Off the road: a bumpy rumble under it.
        if or + os > 1e-4 {
            let b = self.rumble_wander.next(n).abs();
            let s = self.rumble.run(n.white()) * or * (0.18 + 0.45 * b);
            add((s, s));
        } else {
            self.rumble.settle();
        }
        // Stones: pinging into the arches, thudding on the floor (gravel mostly, some
        // off-road and snow). Sliding throws far more.
        let stone = ((gr + or * 0.4) * 2.0 + (gs + os * 0.3) * 9.0 + ss * 1.5) * fast / RATE;
        if n.chance(stone) {
            self.pings.trigger(n.range(1800.0, 6500.0), n.range(0.02, 0.07), n.range(0.012, 0.05), n.range(-0.8, 0.8));
        }
        if n.chance(stone * 1.3) {
            self.thunks.trigger(n.range(260.0, 720.0), n.range(0.03, 0.08), n.range(0.02, 0.07), n.range(-0.5, 0.5));
        }
        let (pl, pr) = self.pings.run();
        let (tl, trr) = self.thunks.run();
        // The floor pan rings a little with each stone.
        let floor = self.thunk_body.run((tl + trr) * 0.5) * 0.8;
        TyreOut { l, r, floor_l: pl + tl + floor, floor_r: pr + trr + floor }
    }
}
