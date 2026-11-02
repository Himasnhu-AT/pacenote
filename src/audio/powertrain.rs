//! The engine, from recordings: a Mercedes-Benz 190E 2.3-16V (a 2.3 litre 16-valve
//! 4-cylinder) on a dyno, steady-rpm loops at full load and at no load, each from two mics
//! recorded together: the engine bay (intake, valve train, block) and the exhaust. Sources in
//! `assets_src/SOURCES.md`; cooked by `tools/audio/cook_car.py`.
//!
//! Every loop plays all the time at `rpm / its recorded rpm`, so all of them sit at the car's
//! rpm. Two equal-power crossfades pick what is heard: by rpm between the two loops whose
//! recorded rpm brackets the car's (in log rpm), and by load between the full-load and no-load
//! sets. The idle loop is the bottom of both sets.
//!
//! On top: the rev limiter (ignition cut ~14 times a second), gear changes (the sim's cut
//! drops to overrun; an upshift under power bangs; a downshift blips), recorded backfire pops
//! on the overrun after a lift and on the limiter, and a damaged engine misfiring.

use super::car::CarParams;
use super::dsp::{Noise, RATE, Smooth, Wander, smoothstep};
use super::samples::{Bank, Id, Looper, Shots};

/// The loops: recorded rpm (measured from the firing frequency, see `cook_car.py --report`)
/// and the two mics.
const LOOPS: [(f32, Id, Id); 5] = [
    (1006.0, Id::IdleBay, Id::IdleExh),
    (2431.0, Id::OffLoBay, Id::OffLoExh),
    (6297.0, Id::OffHiBay, Id::OffHiExh),
    (2556.0, Id::OnLoBay, Id::OnLoExh),
    (6208.0, Id::OnHiBay, Id::OnHiExh),
];
/// No-load and full-load sets, by index into [`LOOPS`], in rpm order.
const OFF: [usize; 3] = [0, 1, 2];
const ON: [usize; 3] = [0, 3, 4];

/// What the engine sends out, per sample.
#[derive(Clone, Copy, Debug, Default)]
pub struct EngineOut {
    /// Engine bay mic (the cockpit hears mostly this, through the bulkhead).
    pub bay: f32,
    /// Exhaust mic (outside sound; boomy and dark inside).
    pub exhaust: f32,
    /// Backfire pops and bangs (stereo; outside sound).
    pub pops: (f32, f32),
}

/// Equal-power weights over one set at `rpm`: (index into LOOPS, weight) × 2.
fn weights(set: &[usize; 3], rpm: f32) -> [(usize, f32); 2] {
    let r = |i: usize| LOOPS[set[i]].0;
    if rpm <= r(0) {
        return [(set[0], 1.0), (set[1], 0.0)];
    }
    for k in 0..2 {
        if rpm < r(k + 1) {
            let t = (rpm / r(k)).ln() / (r(k + 1) / r(k)).ln();
            let a = t * std::f32::consts::FRAC_PI_2;
            return [(set[k], a.cos()), (set[k + 1], a.sin())];
        }
    }
    [(set[2], 1.0), (set[1], 0.0)]
}

#[derive(Clone, Debug)]
pub struct Engine {
    n: Noise,
    pub rpm: Smooth,
    pub load: Smooth,
    level: Smooth,
    loops: [Looper; 5],
    /// Gain of each loop (smoothed a little so a crossfade step never clicks).
    gain: [Smooth; 5],
    /// Firing clock (0..1 per firing, rpm/30 Hz): pops and misfires happen on firings.
    fire: f32,
    /// Ignition cut by the limiter (fast smoothing of the dip).
    cut_gain: Smooth,
    limiter: f32,
    /// 1 right after a lift (pops are likely), decays.
    pub crackle: f32,
    /// Seconds left of a downshift throttle blip.
    blip: f32,
    /// A misfire left unburnt fuel: the next firing may bang in the exhaust.
    unburnt: bool,
    /// A misfire's dip (1 at the misfire, falls over about a firing).
    misfire: f32,
    wobble: Wander,
    last_throttle: f32,
    pops: Shots<4>,
}

impl Engine {
    pub fn new(seed: u32) -> Self {
        let mut n = Noise::new(seed ^ 0x1111);
        // Each loop starts somewhere else, so a crossfade never lines two up in phase.
        let loops = std::array::from_fn(|_| Looper::at(n.unit() as f64));
        Engine {
            n,
            rpm: Smooth::new(950.0, 0.02, 0.03, RATE),
            load: Smooth::new(0.0, 0.03, 0.06, RATE),
            level: Smooth::new(0.0, 0.05, 0.2, RATE),
            loops,
            gain: [Smooth::new(0.0, 0.01, 0.01, RATE); 5],
            fire: 0.0,
            cut_gain: Smooth::new(1.0, 0.004, 0.004, RATE),
            limiter: 0.5,
            crackle: 0.0,
            blip: 0.0,
            unburnt: false,
            misfire: 0.0,
            wobble: Wander::new(3.0),
            last_throttle: 0.0,
            pops: Shots::default(),
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
        self.pops.active()
    }

    fn pop(&mut self, strength: f32) {
        let n = &mut self.n;
        let id = [Id::Pop1, Id::Pop2, Id::Pop3][(n.next_u32() % 3) as usize];
        let gain = n.range(0.5, 1.0) * strength;
        self.pops.play(id, n.range(0.88, 1.12), gain, n.range(-0.2, 0.2));
    }

    /// One sample. Silent (but keeping time) until the bank has loaded.
    #[inline]
    pub fn sample(&mut self, p: &CarParams, bank: Option<&Bank>) -> EngineOut {
        let redline = p.redline;
        let at_limit = p.rpm >= redline - 80.0 && p.throttle > 0.5;
        // Limiter bounce: cut ignition about half of each ~70 ms cycle.
        let mut cut = p.shifting;
        let mut lim_cut = false;
        let mut lim_start = false;
        if at_limit {
            let before = self.limiter;
            self.limiter += 14.0 / RATE;
            if self.limiter >= 1.0 {
                self.limiter -= 1.0;
            }
            lim_cut = self.limiter < 0.45;
            lim_start = lim_cut && before >= 0.45;
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
        let wob = self.wobble.next(&mut self.n);
        let dip = if lim_cut { 0.97 } else { 1.0 };
        // A hurt engine hunts at idle.
        let rpm = self.rpm.next(p.rpm * dip) * (1.0 + (0.006 + 0.03 * dmg) * wob * idle_factor);
        // A stopped engine (rpm 0) is silent.
        let running = smoothstep(150.0, 450.0, rpm);
        let rpm = rpm.max(200.0);
        let rpm_n = ((rpm - p.idle_rpm) / (redline - p.idle_rpm)).clamp(0.0, 1.0);
        let overrun = (1.0 - load) * smoothstep(2200.0, 4500.0, rpm);
        self.crackle *= 1.0 - 1.0 / (1.4 * RATE);
        // Firings: misfires and backfires.
        self.fire += rpm / 30.0 / RATE;
        if self.fire >= 1.0 {
            self.fire -= 1.0;
            let n = &mut self.n;
            let misfire = dmg * dmg * 0.28 * (0.6 + 0.4 * load);
            let mut bang = 0.0;
            if p.active && n.chance(misfire) {
                self.misfire = 1.0;
                self.unburnt = true;
            } else if self.unburnt {
                self.unburnt = false;
                if n.chance(0.5) {
                    bang = 0.6;
                }
            }
            // Unburnt fuel lighting in the exhaust on the overrun after a lift.
            if p.active && n.chance(0.02 * overrun * self.crackle) {
                bang = 1.0;
            }
            if bang > 0.0 {
                self.pop(bang);
            }
        }
        if lim_start && p.active && self.n.chance(0.45) {
            self.pop(0.8);
        }
        // A misfire is a missing firing: the level drops for about a firing.
        self.misfire *= 1.0 - (rpm / 30.0 * 0.8) / RATE;
        // The limiter's cut is heard as the engine dropping out.
        let cutg = self.cut_gain.next(if lim_cut { 0.35 } else { 1.0 });
        // Loudness: idle is quiet, full load at the top loudest; the overrun sits below the
        // same rpm under load.
        let loud = (0.32 + 0.68 * rpm_n.powf(0.8)) * (0.5 + 0.5 * load);
        let gain = level * running * loud * cutg * (1.0 - 0.85 * self.misfire.min(1.0));
        let mut target = [0.0f32; 5];
        let (on, off) = ((load * std::f32::consts::FRAC_PI_2).sin(), (load * std::f32::consts::FRAC_PI_2).cos());
        for (i, w) in weights(&ON, rpm) {
            target[i] += w * on;
        }
        for (i, w) in weights(&OFF, rpm) {
            target[i] += w * off;
        }
        let Some(bank) = bank else {
            return EngineOut::default();
        };
        let (mut bay, mut exhaust) = (0.0, 0.0);
        for (i, (rec_rpm, b, e)) in LOOPS.iter().enumerate() {
            let g = self.gain[i].next(target[i]);
            let rate = rpm / rec_rpm;
            // The bay and exhaust mics were recorded together: one position for both.
            let pos = self.loops[i];
            let vb = self.loops[i].next(bank.get(*b), rate);
            let mut pe = pos;
            let ve = pe.next(bank.get(*e), rate);
            if g > 1e-4 {
                bay += vb * g;
                exhaust += ve * g;
            }
        }
        let pops = self.pops.next(bank);
        EngineOut { bay: bay * gain, exhaust: exhaust * gain, pops: (pops.0 * level, pops.1 * level) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossfade_weights_are_equal_power_and_bracket_the_rpm() {
        for rpm in [500.0, 1006.0, 1800.0, 2556.0, 4000.0, 6208.0, 7600.0] {
            let w = weights(&ON, rpm);
            let p: f32 = w.iter().map(|(_, g)| g * g).sum();
            assert!((p - 1.0).abs() < 1e-4, "rpm {rpm}: power {p}");
            for (i, g) in w {
                if g > 0.01 {
                    let ratio = rpm / LOOPS[i].0;
                    // Never pitched beyond the neighbour's recorded rpm.
                    assert!(ratio > 0.3 && ratio < 2.6, "rpm {rpm}: loop {i} at ×{ratio}");
                }
            }
        }
        assert_eq!(weights(&OFF, 6297.0)[0], (2, 1.0));
    }
}
