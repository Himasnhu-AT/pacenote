//! Engine and turbo for the car sound.
//!
//! Engine: a 4-cylinder, 4-stroke (firing order 1-3-4-2, two firings per revolution, so the
//! firing frequency is rpm/30 Hz). Each cylinder's exhaust pulse is a shape over crank angle
//! (so its harmonics stay locked to engine orders at any rpm), sharper and taller under load,
//! with its own small gain, timing and width offsets (the half-orders that make a real engine
//! gruff instead of a buzz). A short sharp "bark" at each firing, placed with sub-sample
//! timing, adds bite under load. The pulses run through two pipe waveguides (header and
//! tail pipe: fixed resonances the harmonics sweep through, the "vowel"), exhaust formants and
//! a muffler low-pass that opens with load. Intake roar and valve-train ticks are a separate
//! output (louder inside the car).
//!
//! Turbo: boost builds with load above ~2.4k rpm and bleeds off on lift; the whistle follows
//! the shaft speed; lifting with boost gives a blow-off and compressor flutter.

use super::car::CarParams;
use super::dsp::{Biquad, Decay, Delay, Noise, OnePole, Osc, RATE, Ring, Smooth, Wander, smoothstep};

/// Per-cylinder gain, timing (fraction of the cycle) and pulse width offsets: a slightly
/// uneven engine, like a real one.
const CYL_GAIN: [f32; 4] = [1.0, 0.8, 0.92, 0.85];
const CYL_TIME: [f32; 4] = [0.0, 0.006, -0.004, 0.003];
const CYL_WIDTH: [f32; 4] = [1.0, 1.12, 0.92, 1.05];
/// Firing points in the 720° cycle (0..1), firing order 1-3-4-2.
const FIRE_AT: [f32; 4] = [0.0, 0.75, 0.25, 0.5];

/// What the engine sends out, per sample.
#[derive(Clone, Copy, Debug, Default)]
pub struct EngineOut {
    /// Exhaust note (heard best outside, through the body inside).
    pub exhaust: f32,
    /// Backfire pops and bangs (outside sound).
    pub pops: f32,
    /// Intake roar, valve train, knock (heard best inside).
    pub mech: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Cyl {
    /// Strength of the current pulse (set at firing).
    amp: f32,
    /// Phase since this cylinder last fired (cycles).
    last_u: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Pop {
    noise: Decay,
    boom: Ring,
    crack: Biquad,
}

#[derive(Clone, Debug)]
pub struct Engine {
    n: Noise,
    /// Crank phase over one 720° cycle (0..1).
    theta: f32,
    pub rpm: Smooth,
    pub load: Smooth,
    level: Smooth,
    cyl: [Cyl; 4],
    /// Pulse width (cycle fraction) and sharpness blend (0 soft .. 1 sharp), from load.
    width: f32,
    sharp: f32,
    bark_a: OnePole,
    bark_b: OnePole,
    /// Sub-sample remainder of the last bark impulse (added on the next sample).
    bark_next: f32,
    burst: Decay,
    header: Delay<256>,
    header_lp: OnePole,
    pipe: Delay<1024>,
    pipe_lp: OnePole,
    body: Biquad,
    mid: Biquad,
    rasp: Biquad,
    muffler: Biquad,
    dc: Biquad,
    intake_bp: Biquad,
    intake_honk: Biquad,
    tick: Ring,
    knock: Ring,
    knock_bp: Biquad,
    leak: Biquad,
    wobble: Wander,
    limiter: f32,
    /// 1 right after a lift (pops are likely), decays.
    pub crackle: f32,
    /// Seconds left of a downshift throttle blip.
    blip: f32,
    /// A misfire left unburnt fuel: the next firing may bang in the exhaust.
    unburnt: bool,
    last_throttle: f32,
    pops: [Pop; 3],
    next_pop: usize,
}

impl Engine {
    pub fn new(seed: u32) -> Self {
        Engine {
            n: Noise::new(seed ^ 0x1111),
            theta: 0.0,
            rpm: Smooth::new(950.0, 0.02, 0.03, RATE),
            load: Smooth::new(0.0, 0.03, 0.06, RATE),
            level: Smooth::new(0.0, 0.05, 0.2, RATE),
            cyl: [Cyl::default(); 4],
            width: 0.11,
            sharp: 0.0,
            bark_a: OnePole::new(1500.0),
            bark_b: OnePole::new(1500.0),
            bark_next: 0.0,
            burst: Decay::default(),
            header: Delay::default(),
            header_lp: OnePole::new(3500.0),
            pipe: Delay::default(),
            pipe_lp: OnePole::new(1800.0),
            body: Biquad::bandpass(110.0, 1.0),
            mid: Biquad::bandpass(420.0, 2.0),
            rasp: Biquad::bandpass(1350.0, 1.6),
            muffler: Biquad::lowpass(3000.0, 0.7),
            dc: Biquad::highpass(28.0, 0.7),
            intake_bp: Biquad::bandpass(900.0, 0.8),
            intake_honk: Biquad::bandpass(360.0, 4.0),
            tick: Ring::default(),
            knock: Ring::default(),
            knock_bp: Biquad::bandpass(1500.0, 2.0),
            leak: Biquad::bandpass(1900.0, 1.2),
            wobble: Wander::new(3.0),
            limiter: 0.5,
            crackle: 0.0,
            blip: 0.0,
            unburnt: false,
            last_throttle: 0.0,
            pops: [Pop::default(); 3],
            next_pop: 0,
        }
    }

    /// New parameters (once per block): lift-off crackle window.
    pub fn set(&mut self, p: &CarParams) {
        if p.throttle < 0.25 && self.last_throttle >= 0.5 && p.rpm > 3200.0 {
            self.crackle = 1.0;
        }
        self.last_throttle = p.throttle;
    }

    /// A gear change: an upshift under power bangs (ignition cut with fuel in the pipe); a
    /// downshift blips the throttle (rev-matching) and crackles.
    pub fn shift(&mut self, up: bool, p: &CarParams) {
        if up {
            if p.throttle > 0.5 && p.rpm > 4200.0 && self.n.chance(0.75) {
                self.pop(1.0);
            }
        } else {
            self.blip = 0.09;
            if p.rpm > 2800.0 {
                self.crackle = self.crackle.max(0.7);
            }
        }
    }

    /// A backfire is sounding.
    pub fn popping(&self) -> bool {
        self.pops.iter().any(|p| p.noise.active())
    }

    fn pop(&mut self, strength: f32) {
        let n = &mut self.n;
        let pop = &mut self.pops[self.next_pop];
        self.next_pop = (self.next_pop + 1) % 3;
        let a = n.range(0.5, 1.0) * strength;
        pop.noise.trigger(0.6 * a, n.range(0.02, 0.05));
        pop.boom.trigger(n.range(60.0, 95.0), n.range(0.08, 0.14), 0.5 * a);
        pop.crack.set_bandpass(n.range(1600.0, 3200.0), 0.9);
    }

    /// Filter coefficients from the smoothed state (every few dozen samples).
    pub fn update_coeffs(&mut self, p: &CarParams) {
        let load = self.load.v;
        let rpm_n = ((self.rpm.v - p.idle_rpm) / (p.redline - p.idle_rpm)).clamp(0.0, 1.0);
        let bark = 600.0 + 1900.0 * load + 900.0 * rpm_n;
        self.bark_a.set(bark);
        self.bark_b.set(bark);
        self.muffler.set_lowpass(1500.0 + 3300.0 * load + 1800.0 * rpm_n, 0.75);
        self.rasp.set_bandpass(1250.0 + 500.0 * rpm_n, 1.6);
        self.intake_bp.set_bandpass(700.0 + 900.0 * rpm_n, 0.8);
        // Narrower, taller pulses under load.
        // (In a 720° cycle with 4 firings, 0.1 ≈ 72° of crank: the exhaust blowdown.)
        self.width = 0.11 - 0.035 * load;
        self.sharp = load;
    }

    /// One sample.
    #[inline]
    pub fn sample(&mut self, p: &CarParams) -> EngineOut {
        let n = &mut self.n;
        let redline = p.redline;
        let at_limit = p.rpm >= redline - 80.0 && p.throttle > 0.5;
        // Limiter bounce: cut ignition about half of each ~70 ms cycle.
        let mut cut = p.shifting;
        let mut lim_cut = false;
        if at_limit {
            self.limiter += 14.0 / RATE;
            if self.limiter >= 1.0 {
                self.limiter -= 1.0;
            }
            lim_cut = self.limiter < 0.45;
            cut |= lim_cut;
        } else {
            self.limiter = 0.5;
        }
        let mut throttle = if cut { 0.0 } else { p.throttle };
        if self.blip > 0.0 {
            self.blip -= 1.0 / RATE;
            throttle = throttle.max(0.65);
        }
        let load = self.load.next(throttle);
        let level = self.level.next(if p.active { 1.0 } else { 0.0 });
        let dmg = p.engine_damage;
        let idle_factor = 1.0 - smoothstep(p.idle_rpm, p.idle_rpm + 1500.0, p.rpm);
        let wob = self.wobble.next(n);
        let dip = if at_limit && cut { 0.97 } else { 1.0 };
        // A hurt engine hunts at idle.
        let rpm = self.rpm.next(p.rpm * dip) * (1.0 + (0.008 + 0.03 * dmg) * wob * idle_factor);
        let rpm_n = ((rpm - p.idle_rpm) / (redline - p.idle_rpm)).clamp(0.0, 1.0);
        let overrun = (1.0 - load) * smoothstep(2200.0, 4500.0, rpm);
        self.crackle *= 1.0 - 1.0 / (1.4 * RATE);
        let inc = rpm / 120.0 / RATE;
        self.theta += inc;
        if self.theta >= 1.0 {
            self.theta -= 1.0;
        }
        // Pulse shapes: x(1−x)² (round, overrun) blended to x(1−x)⁵ (sharp, load), peak 1.
        let mut pulse = 0.0;
        let mut bark = self.bark_next;
        self.bark_next = 0.0;
        let mut pops = 0.0;
        let mut mech = 0.0;
        let w_base = self.width;
        for k in 0..4 {
            let mut u = self.theta - FIRE_AT[k] - CYL_TIME[k];
            u -= u.floor();
            let c = &mut self.cyl[k];
            if u < c.last_u {
                // This cylinder just fired (`u` cycles ago = u / inc samples ago).
                let jitter = 1.0 + (0.16 * idle_factor + 0.04 + 0.1 * dmg) * n.white();
                let cg = 1.0 + (CYL_GAIN[k] - 1.0) * (0.6 + 0.4 * idle_factor);
                let base = (0.42 + 0.58 * load) * (0.6 + 0.4 * rpm_n) * cg * jitter;
                // A gear change drops to an overrun burble; the limiter cuts harder.
                let mut fired = if lim_cut { 0.1 } else if cut { 0.45 } else { 1.0 - 0.4 * overrun };
                // Damage: misfires (more under load), each leaving fuel to bang later.
                let misfire = dmg * dmg * 0.18 * (0.6 + 0.4 * load);
                if p.active && n.chance(misfire) {
                    fired *= 0.08;
                    self.unburnt = true;
                } else if self.unburnt {
                    self.unburnt = false;
                    if n.chance(0.5) {
                        let pop = &mut self.pops[self.next_pop];
                        self.next_pop = (self.next_pop + 1) % 3;
                        let a = n.range(0.3, 0.8);
                        pop.noise.trigger(0.5 * a, n.range(0.015, 0.04));
                        pop.boom.trigger(n.range(55.0, 80.0), 0.1, 0.45 * a);
                        pop.crack.set_bandpass(n.range(1400.0, 2600.0), 0.9);
                    }
                }
                c.amp = base * fired;
                // Bark: an impulse split over this and the next sample by where between
                // them the firing fell (sub-sample timing: no jitter at high rpm).
                let frac = (u / inc.max(1e-9)).clamp(0.0, 1.0);
                let imp = c.amp * (0.45 + 0.55 * load);
                bark += imp * frac;
                self.bark_next += imp * (1.0 - frac);
                self.burst.trigger(c.amp * (0.12 + 0.45 * load), 0.006 + 0.01 * (1.0 - rpm_n));
                // Valve train: a light tick each firing; a damaged engine knocks.
                if p.active {
                    self.tick.trigger(n.range(4200.0, 5600.0), 0.006, 0.006 * (0.3 + rpm_n));
                    if dmg > 0.25 && n.chance(0.35 + 0.5 * dmg) {
                        let a = (dmg - 0.25) * (0.4 + 0.6 * load);
                        self.knock.trigger(n.range(1100.0, 1900.0), n.range(0.01, 0.022), 0.3 * a);
                    }
                }
                // Backfire: unburnt fuel lighting in the exhaust on overrun and on the limiter.
                let chance = if lim_cut { 0.22 } else { 0.07 * overrun * self.crackle };
                if p.active && n.chance(chance) {
                    let pop = &mut self.pops[self.next_pop];
                    self.next_pop = (self.next_pop + 1) % 3;
                    let a = n.range(0.5, 1.0);
                    pop.noise.trigger(0.6 * a, n.range(0.02, 0.05));
                    pop.boom.trigger(n.range(65.0, 95.0), 0.12, 0.5 * a);
                    pop.crack.set_bandpass(n.range(1800.0, 3200.0), 0.9);
                }
            }
            c.last_u = u;
            let w = w_base * CYL_WIDTH[k];
            if u < w && c.amp > 0.0 {
                let x = u / w;
                let y = 1.0 - x;
                let y2 = y * y;
                let soft = 6.75 * x * y2;
                let sharp = 14.93 * x * y2 * y2 * y;
                pulse += c.amp * (soft + (sharp - soft) * self.sharp);
            }
        }
        // Bark: impulse → two one-poles (normalised to ~unit peak per firing).
        let k = self.bark_a.coeff().max(1e-4);
        let x = self.bark_a.lp(bark * std::f32::consts::E / k);
        let bark = self.bark_b.lp(x);
        let comb = n.white() * self.burst.next();
        let src = pulse * 0.55 + bark * (0.35 + 0.35 * load) + comb;
        // Header (short) and tail pipe (long) waveguides: open ends reflect inverted, losses
        // grow with frequency.
        let h = src - 0.32 * self.header_lp.lp(self.header.read(118.0));
        self.header.write(h);
        let t = h - 0.42 * self.pipe_lp.lp(self.pipe.read(436.0));
        self.pipe.write(t);
        let body = self.body.run(t) * 0.5;
        let mid = self.mid.run(t) * 0.6;
        let rasp = self.rasp.run(src) * (0.12 + 0.4 * load);
        // A cracked manifold: a breathy chuff with each pulse.
        let leak = if dmg > 0.2 { self.leak.run(n.white() * pulse) * 0.5 * (dmg - 0.2) } else { 0.0 };
        let raw = self.muffler.run(t * 0.4 + body + mid + rasp) + leak;
        let exhaust = self.dc.run(raw) * 0.75 * level;
        // Intake: suction noise and the airbox resonance, both with load and rpm.
        let suction = n.white() * (0.15 + pulse.abs());
        mech += self.intake_bp.run(suction) * 0.09 * load * (0.3 + rpm_n);
        mech += self.intake_honk.run(pulse) * 0.18 * (0.2 + load) * (0.3 + 0.7 * rpm_n);
        mech += self.tick.tick();
        if self.knock.active() {
            let kv = self.knock.next();
            mech += self.knock_bp.run(kv + n.white() * 0.3 * kv.abs());
        } else {
            self.knock_bp.settle();
        }
        for pop in &mut self.pops {
            if pop.noise.active() || pop.boom.active() {
                let nz = n.white() * pop.noise.next();
                pops += pop.crack.run(nz) * 1.4 + nz * 0.3 + pop.boom.tick();
            } else {
                pop.crack.settle();
            }
        }
        EngineOut { exhaust, pops: pops * 0.5 * level, mech: mech * level }
    }
}

#[derive(Clone, Debug)]
pub struct Turbo {
    n: Noise,
    pub boost: f32,
    shaft: Smooth,
    whistle: Osc,
    whistle2: Osc,
    hiss: Biquad,
    pub bov: Decay,
    bov_f: f32,
    bov_bp: Biquad,
    flutter: Decay,
    flutter_osc: Osc,
    last_throttle: f32,
}

impl Turbo {
    pub fn new(seed: u32) -> Self {
        Turbo {
            n: Noise::new(seed ^ 0x2222),
            boost: 0.0,
            shaft: Smooth::new(0.0, 0.5, 0.9, RATE),
            whistle: Osc::default(),
            whistle2: Osc::default(),
            hiss: Biquad::bandpass(5000.0, 6.0),
            bov: Decay::default(),
            bov_f: 3500.0,
            bov_bp: Biquad::bandpass(3500.0, 1.2),
            flutter: Decay::default(),
            flutter_osc: Osc::default(),
            last_throttle: 0.0,
        }
    }

    /// New parameters: a lift with boost blows off.
    pub fn set(&mut self, p: &CarParams) {
        if p.throttle < 0.25 && self.last_throttle >= 0.5 {
            let b = self.boost;
            if b > 0.3 {
                self.bov.trigger(b * 0.22, 0.45);
                self.bov_f = 4200.0;
                self.flutter.trigger(b * 0.12, 0.35);
            }
        }
        self.last_throttle = p.throttle;
    }

    /// An upshift under boost: a short chuff of the valve.
    pub fn shift(&mut self, up: bool) {
        if up && self.boost > 0.4 && !self.bov.active() {
            self.bov.trigger(self.boost * 0.1, 0.18);
            self.bov_f = 3600.0;
        }
    }

    pub fn update_coeffs(&mut self) {
        self.hiss.set_bandpass(2400.0 + 5200.0 * self.shaft.v, 7.0);
        self.bov_bp.set_bandpass(self.bov_f, 1.3);
    }

    #[inline]
    pub fn sample(&mut self, load: f32, rpm: f32) -> f32 {
        // Boost builds with load above ~2500 rpm (lag), bleeds off on lift.
        let target = load * smoothstep(2400.0, 5200.0, rpm);
        let rate = if target > self.boost { 1.3 } else { 4.0 };
        self.boost += (target - self.boost) * rate / RATE;
        let shaft = self.shaft.next(self.boost.max(smoothstep(1500.0, 7000.0, rpm) * 0.25));
        let wf = 2400.0 + 5200.0 * shaft;
        let n = &mut self.n;
        // Compressor whistle: the blade-pass tone and a weaker partial, plus air hiss.
        let whistle = (self.whistle.next(wf) + 0.3 * self.whistle2.next(wf * 1.52)) * shaft * shaft * 0.011
            + self.hiss.run(n.white()) * shaft * 0.05;
        let mut bov = 0.0;
        if self.bov.active() {
            self.bov_f = (self.bov_f * (1.0 - 1.6 / RATE)).max(1200.0);
            bov = self.bov_bp.run(n.white()) * self.bov.next();
        }
        if self.flutter.active() {
            // Compressor surge "stu-tu-tu".
            let am = 0.5 + 0.5 * self.flutter_osc.next(22.0);
            bov += self.bov_bp.run(n.white()) * self.flutter.next() * am * am;
        }
        if !self.bov.active() && !self.flutter.active() {
            self.bov_bp.settle();
        }
        whistle + bov
    }
}
