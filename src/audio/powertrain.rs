//! The engine, from recordings: a Mercedes-Benz 190E 2.3-16V (a 2.3 litre 16-valve
//! 4-cylinder) on a dyno, each loop from two mics recorded together: the engine bay (intake,
//! valve train, block) and the exhaust. Sources in `assets_src/SOURCES.md`; cooked by
//! `tools/audio/cook_car.py`.
//!
//! Twelve loops at full load and thirteen at no load, ~16 % apart in rpm, plus idle. Five are the
//! steady-rpm recordings themselves; the rest were re-synthesised from them at cook time by
//! pitch-synchronous overlap-add (each firing laid down again at the new firing period, so the
//! resonances stay where they were recorded). Every loop plays at `rpm / its rpm`, which never
//! goes past ±8 % while it is heard. Two equal-power crossfades pick what is heard: by rpm
//! between the two loops that bracket the car's rpm (in log rpm), and by load between the
//! full-load and no-load sets. The idle loop is the bottom of both sets.
//!
//! On top: the rev limiter (ignition cut ~14 times a second), gear changes (the sim's cut
//! drops to overrun; an upshift under power bangs; a downshift blips), recorded backfire pops
//! on the overrun after a lift and on the limiter, and a damaged engine misfiring.

use super::car::CarParams;
use super::dsp::{Noise, RATE, Smooth, Wander, smoothstep};
use super::samples::{Bank, Id, Shots, read_wrap};

/// The loops: file stem (`assets/sounds/car/engine_<stem>_{bay,exh}.ogg`), rpm (measured from
/// the firing frequency for the recordings) and whole firings in the loop, which starts on a
/// firing's pulse. From `cook_car.py --report`.
pub const LOOPS: [(&str, f32, u32); 26] = [
    ("idle", 1006.1, 87),
    ("on_1450", 1450.0, 63),
    ("on_1670", 1670.0, 72),
    ("on_1925", 1925.0, 83),
    ("on_2218", 2218.0, 96),
    ("on_2556", 2556.4, 221),
    ("on_2963", 2963.0, 128),
    ("on_3434", 3434.0, 149),
    ("on_3980", 3980.0, 172),
    ("on_4613", 4613.0, 200),
    ("on_5347", 5347.0, 232),
    ("on_6208", 6208.3, 315),
    ("on_7200", 7200.0, 312),
    ("off_1166", 1166.0, 51),
    ("off_1351", 1351.0, 59),
    ("off_1566", 1566.0, 68),
    ("off_1815", 1815.0, 79),
    ("off_2103", 2103.0, 91),
    ("off_2431", 2431.3, 210),
    ("off_2849", 2849.0, 123),
    ("off_3339", 3339.0, 145),
    ("off_3913", 3913.0, 170),
    ("off_4586", 4586.0, 199),
    ("off_5374", 5374.0, 233),
    ("off_6297", 6297.5, 381),
    ("off_7300", 7300.0, 316),
];
const N: usize = LOOPS.len();
/// Full-load and no-load sets, by index into [`LOOPS`], in rpm order (idle at the bottom).
const ON: [usize; 13] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
const OFF: [usize; 14] = [0, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25];

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

/// Crossfade weights over one set at `rpm`: (index into LOOPS, weight) × 2. The loops pulse
/// together (one firing clock), so where the firings are clear (full load) they add in phase and
/// an equal-gain crossfade would hold the level; where noise dominates (no load) they add like
/// unrelated sounds and equal power would. The weights sit between the two (their 1.5-norm is
/// 1): within ~2 dB either way, and the firing's shape morphs from one loop into the next.
fn weights(set: &[usize], rpm: f32) -> [(usize, f32); 2] {
    let r = |i: usize| LOOPS[set[i]].1;
    let last = set.len() - 1;
    if rpm <= r(0) {
        return [(set[0], 1.0), (set[1], 0.0)];
    }
    for k in 0..last {
        if rpm < r(k + 1) {
            let t = (rpm / r(k)).ln() / (r(k + 1) / r(k)).ln();
            let n = ((1.0 - t).powf(1.5) + t.powf(1.5)).powf(1.0 / 1.5);
            return [(set[k], (1.0 - t) / n), (set[k + 1], t / n)];
        }
    }
    [(set[last], 1.0), (set[last - 1], 0.0)]
}
#[derive(Clone, Debug)]
pub struct Engine {
    n: Noise,
    pub rpm: Smooth,
    pub load: Smooth,
    level: Smooth,
    /// The shared firing clock (firings since start): loop `i` plays at firing `clock mod its
    /// firings`, so every loop's pulses line up.
    clock: f64,
    /// Gain of each loop (smoothed a little so a crossfade step never clicks).
    gain: [Smooth; N],
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
        let n = Noise::new(seed ^ 0x1111);
        Engine {
            n,
            rpm: Smooth::new(950.0, 0.02, 0.03, RATE),
            load: Smooth::new(0.0, 0.03, 0.06, RATE),
            level: Smooth::new(0.0, 0.05, 0.2, RATE),
            clock: 0.0,
            gain: [Smooth::new(0.0, 0.01, 0.01, RATE); N],
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
        let mut target = [0.0f32; N];
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
        let clock = self.clock;
        self.clock += (rpm / 30.0 / RATE) as f64;
        for (i, &(_, _, firings)) in LOOPS.iter().enumerate() {
            let g = self.gain[i].next(target[i]);
            // Out of the mix (all but about four of the 26): nothing to read.
            if g <= 1e-4 {
                continue;
            }
            let (b, e) = bank.engine(i);
            if b.len() < 4 || e.len() != b.len() {
                continue;
            }
            // The bay and exhaust mics were recorded together: one position for both.
            let pos = (clock / firings as f64).fract() * b.len() as f64;
            bay += read_wrap(b, pos) * g;
            exhaust += read_wrap(e, pos) * g;
        }
        let pops = self.pops.next(bank);
        EngineOut { bay: bay * gain, exhaust: exhaust * gain, pops: (pops.0 * level, pops.1 * level) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossfade_weights_are_normalised_and_bracket_the_rpm() {
        for set in [&ON[..], &OFF[..]] {
            let mut rpm = 500.0;
            while rpm < 8000.0 {
                let w = weights(set, rpm);
                let p: f32 = w.iter().map(|(_, g)| g.powf(1.5)).sum();
                assert!((p - 1.0).abs() < 1e-4, "rpm {rpm}: 1.5-norm {p}");
                for (i, g) in w {
                    if g > 0.01 && rpm > 1500.0 && rpm < 7600.0 {
                        // Between 1.5k and the redline no loop is heard pitched past ±8.5 %.
                        // No loop heard past its neighbour's rpm (±17 %); the louder of the
                        // two is within ±8.5 %.
                        let ratio = rpm / LOOPS[i].1;
                        assert!(ratio > 1.0 / 1.18 && ratio < 1.18, "rpm {rpm}: loop {} at ×{ratio}", LOOPS[i].0);
                        if g >= w[0].1.max(w[1].1) {
                            assert!(ratio > 1.0 / 1.085 && ratio < 1.085, "rpm {rpm}: loudest loop {} at ×{ratio}", LOOPS[i].0);
                        }
                    }
                }
                rpm *= 1.01;
            }
        }
        assert_eq!(weights(&OFF, 6297.5)[0], (24, 1.0));
        assert_eq!(weights(&ON, 9000.0)[0], (12, 1.0));
        // Sets are in rpm order.
        for set in [&ON[..], &OFF[..]] {
            assert!(set.windows(2).all(|w| LOOPS[w[0]].1 < LOOPS[w[1]].1));
        }
    }
}
