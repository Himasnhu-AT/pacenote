//! Tyres on each surface class, rolling and sliding, plus stones thrown at the arches and the
//! floor.
//!
//! | Class | Rolling | Sliding |
//! |---|---|---|
//! | gravel | dense crunch grains (small resonant clicks) + compression crunch + rumble | scrabble (many more grains), spray hiss, ploughing roar, stones on the floor |
//! | tarmac | tread hum (~1 kHz band) + low road roar | squeal: band-limited saw with stick-slip chatter through two resonances; scrub hiss |
//! | snow | squeaky tonal grains + soft crunch | more squeak, hiss |
//! | mud | low squelch + rising "bubble" chirps | slurp, many more bubbles |
//! | off-road | grass swish + bumps | more swish |

use super::car::{CarParams, GRAVEL, MUD, OFFROAD, SNOW, SURFACES, TARMAC};
use super::dsp::{Biquad, Decay, Noise, Osc, RATE, Ring, Saw, Smooth, Wander, smoothstep};

/// Short resonant clicks with a pan each (gravel and snow grains, stones).
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
                // Cheap linear pan (the grains are tiny; equal power isn't audible here).
                l += s * (1.0 - *px);
                r += s * (1.0 + *px);
            }
        }
        (l, r)
    }
}

/// A bubble in mud: a sine chirping upward as it decays.
#[derive(Clone, Copy, Debug, Default)]
struct Bubble {
    osc: Osc,
    hz: f32,
    rise: f32,
    env: Decay,
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

#[derive(Clone, Debug)]
pub struct Tyres {
    n: Noise,
    pub roll: [Smooth; SURFACES],
    pub slide: [Smooth; SURFACES],
    grains: Grains<10>,
    crunch: Biquad,
    crunch_am: Wander,
    gravel_lo: Biquad,
    spray: Biquad,
    spray_am: Wander,
    plough: Biquad,
    tread: Biquad,
    tarmac_lo: Biquad,
    squeal: Saw,
    squeal_lo: Biquad,
    squeal_hi: Biquad,
    squeal_wander: Wander,
    chatter: Wander,
    scrub: Biquad,
    snow: Grains<6>,
    snow_crunch: Biquad,
    mud_lo: Biquad,
    mud_slurp: Biquad,
    mud_wander: Wander,
    bubbles: [Bubble; 4],
    next_bubble: usize,
    grass_hp: Biquad,
    rumble: Biquad,
    rumble_wander: Wander,
    pings: Grains<6>,
    thunks: Grains<4>,
    thunk_body: Biquad,
}

impl Tyres {
    pub fn new(seed: u32) -> Self {
        let sm = |up, down| Smooth::new(0.0, up, down, RATE);
        Tyres {
            n: Noise::new(seed ^ 0x3333),
            roll: [sm(0.05, 0.08); SURFACES],
            slide: [sm(0.03, 0.09); SURFACES],
            grains: Grains::default(),
            crunch: Biquad::bandpass(650.0, 0.9),
            crunch_am: Wander::new(45.0),
            gravel_lo: Biquad::lowpass(200.0, 0.7),
            spray: Biquad::bandpass(4800.0, 0.55),
            spray_am: Wander::new(14.0),
            plough: Biquad::bandpass(480.0, 0.8),
            tread: Biquad::bandpass(950.0, 0.9),
            tarmac_lo: Biquad::lowpass(420.0, 0.7),
            squeal: Saw::default(),
            squeal_lo: Biquad::bandpass(900.0, 7.0),
            squeal_hi: Biquad::bandpass(2000.0, 6.0),
            squeal_wander: Wander::new(3.0),
            chatter: Wander::new(28.0),
            scrub: Biquad::bandpass(1500.0, 0.7),
            snow: Grains::default(),
            snow_crunch: Biquad::bandpass(1300.0, 1.0),
            mud_lo: Biquad::lowpass(320.0, 0.9),
            mud_slurp: Biquad::bandpass(420.0, 3.0),
            mud_wander: Wander::new(4.0),
            bubbles: [Bubble::default(); 4],
            next_bubble: 0,
            grass_hp: Biquad::highpass(3500.0, 0.7),
            rumble: Biquad::lowpass(110.0, 0.8),
            rumble_wander: Wander::new(6.0),
            pings: Grains::default(),
            thunks: Grains::default(),
            thunk_body: Biquad::bandpass(420.0, 1.4),
        }
    }

    pub fn update_coeffs(&mut self) {
        let ts = self.slide[TARMAC].v.min(2.0);
        // Squeal pitch rises a little with slip.
        self.squeal_lo.set_bandpass(860.0 + 180.0 * ts, 7.0);
        self.squeal_hi.set_bandpass(1950.0 + 350.0 * ts, 6.0);
    }

    #[inline]
    pub fn sample(&mut self, p: &CarParams, speed: f32) -> TyreOut {
        let n = &mut self.n;
        let mut roll = [0.0f32; SURFACES];
        let mut slide = [0.0f32; SURFACES];
        for i in 0..SURFACES {
            roll[i] = self.roll[i].next(p.surfaces[i].roll);
            slide[i] = self.slide[i].next(p.surfaces[i].slide);
        }
        let wn = n.white();
        let mut mono = 0.0;
        let fast = smoothstep(3.0, 15.0, speed);
        // Gravel.
        let (gr, gs) = (roll[GRAVEL], slide[GRAVEL]);
        if gr + gs > 1e-4 {
            // Grains: stones crunching under the tread. Rate grows with roll and much more
            // with slide (scrabble).
            let rate = (gr * 320.0 + gs * 1400.0) / RATE;
            if n.chance(rate) {
                let a = n.range(0.008, 0.035) * (1.0 + 0.8 * gs.min(1.5));
                self.grains.trigger(n.range(700.0, 3800.0), n.range(0.003, 0.012), a, n.range(-0.7, 0.7));
            }
            let am = (0.55 + 0.45 * self.crunch_am.next(n)).max(0.0);
            let spray_am = (0.6 + 0.4 * self.spray_am.next(n)).max(0.0);
            mono += self.crunch.run(wn) * (gr * 0.08 + gs * 0.08) * am * am
                + self.gravel_lo.run(wn) * gr * 0.22
                + self.spray.run(wn) * gs * 0.15 * spray_am
                + self.plough.run(wn) * gs * 0.14 * am;
        }
        // Snow: squeaky tonal grains over a soft crunch.
        let (sr, ss) = (roll[SNOW], slide[SNOW]);
        if sr + ss > 1e-4 {
            let rate = (sr * 140.0 + ss * 600.0) / RATE;
            if n.chance(rate) {
                self.snow.trigger(n.range(1100.0, 2900.0), n.range(0.008, 0.022), n.range(0.006, 0.022), n.range(-0.6, 0.6));
            }
            let am = (0.5 + 0.5 * self.crunch_am.next(n)).max(0.0);
            mono += self.snow_crunch.run(wn) * (sr * 0.05 + ss * 0.12) * am + self.spray.run(wn) * ss * 0.05;
        }
        // Tarmac: tread hum and road roar; squeal past the grip peak, scrub when locked.
        let (tr, ts) = (roll[TARMAC], slide[TARMAC]);
        if tr + ts > 1e-4 {
            mono += self.tread.run(wn) * tr * tr.min(1.0) * 0.05 + self.tarmac_lo.run(wn) * tr * 0.11;
            let sq = smoothstep(0.2, 0.9, ts);
            if sq > 1e-4 {
                let v = self.squeal_wander.next(n);
                // Stick-slip: the rubber grabs and lets go tens of times a second.
                let ch = self.chatter.next(n);
                let f = (420.0 + 90.0 * ts.min(2.0)) * (1.0 + 0.06 * v + 0.015 * ch);
                let saw = self.squeal.next(f);
                let tone = self.squeal_lo.run(saw) * 0.9 + self.squeal_hi.run(saw) * 0.5;
                let am = (0.65 + 0.35 * ch).max(0.0);
                mono += tone * sq * 0.42 * am * (0.75 + 0.25 * v.abs());
            }
            mono += self.scrub.run(wn) * ts * 0.05;
        }
        // Mud: low squelch with a wet resonance, and bubbles that chirp upward.
        let (mr, ms) = (roll[MUD], slide[MUD]);
        if mr + ms > 1e-4 {
            let v = self.mud_wander.next(n);
            let am = (0.6 + 0.4 * v).max(0.0);
            mono += self.mud_lo.run(wn) * (mr * 0.24 + ms * 0.3) * am + self.mud_slurp.run(wn) * (ms * 0.16 + mr * 0.03) * am * am;
            let rate = (mr * 14.0 + ms * 45.0) / RATE;
            if n.chance(rate) {
                let b = &mut self.bubbles[self.next_bubble];
                self.next_bubble = (self.next_bubble + 1) % 4;
                b.hz = n.range(110.0, 260.0);
                b.rise = 1.0 + n.range(3.0, 9.0) / RATE;
                b.env.trigger(n.range(0.02, 0.05) * (0.6 + 0.4 * (mr + ms).min(1.5)), n.range(0.04, 0.09));
            }
        }
        for b in &mut self.bubbles {
            if b.env.active() {
                b.hz *= b.rise;
                mono += b.osc.next(b.hz) * b.env.next();
            }
        }
        // Off the road: grass swish and bumps.
        let (or, os) = (roll[OFFROAD], slide[OFFROAD]);
        if or + os > 1e-4 {
            let b = self.rumble_wander.next(n).abs();
            mono += self.grass_hp.run(wn) * (or * 0.03 + os * 0.04) + self.rumble.run(wn) * or * (0.18 + 0.45 * b);
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
        let (gl, grr) = self.grains.run();
        let (sl, srr) = self.snow.run();
        let (pl, pr) = self.pings.run();
        let (tl, trr) = self.thunks.run();
        // The floor pan rings a little with each stone.
        let thunk_mono = (tl + trr) * 0.5;
        let floor = self.thunk_body.run(thunk_mono) * 0.8;
        TyreOut { l: mono + gl + sl, r: mono + grr + srr, floor_l: pl + tl + floor, floor_r: pr + trr + floor }
    }
}
