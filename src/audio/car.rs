//! The car and its inside/outside mix. The parts:
//! - [`super::powertrain`]: the engine from recorded loops (engine bay and exhaust mics,
//!   crossfaded by rpm and load), recorded backfire pops, the limiter, misfires when damaged.
//! - [`super::tyres`]: recorded rolling and sliding loops per surface, stones on the arches
//!   and floor.
//! - [`super::body`]: recorded impacts, ground hits, snaps and scrapes; suspension thumps and
//!   bottoming, rattles, gear clunks.
//! - here: wind, and the mix for the chase camera (outside) or the cockpit and bonnet cameras
//!   (inside: the engine bay mic through the bulkhead, the exhaust dark and boomy, tyres
//!   muffled, stones and rattles up).
//!
//! Runs on the audio thread; parameters arrive once per block (see [`CarParams`]) and are
//! smoothed per sample. The recordings come from [`super::samples::bank`]; until it has loaded
//! the recorded layers are silent.

use super::body::Body;
use super::dsp::{Biquad, Noise, RATE, Smooth, Wander};
use super::powertrain::Engine;
use super::samples::{self, Bank};
use super::tyres::Tyres;

/// Surface classes the tyres sound different on.
pub const SURFACES: usize = 5;
pub const GRAVEL: usize = 0;
pub const TARMAC: usize = 1;
pub const MUD: usize = 2;
pub const SNOW: usize = 3;
/// Verge and rough ground (grass, dirt, bumps).
pub const OFFROAD: usize = 4;

/// How much the tyres roll and slide on one surface class (sums over the wheels on it).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Contact {
    /// Rolling: wheel share × speed factor (about 0..1.5).
    pub roll: f32,
    /// Sliding: wheel share × slip speed factor (about 0..2).
    pub slide: f32,
}

/// What the car sound needs from the simulation, sent once per frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarParams {
    /// A car exists (false: everything fades out).
    pub active: bool,
    pub rpm: f32,
    pub idle_rpm: f32,
    pub redline: f32,
    /// Throttle 0..1 as the engine sees it (0 while held at the start or shifting).
    pub throttle: f32,
    /// Ignition cut for a gear change.
    pub shifting: bool,
    pub gear: i8,
    /// |speed| m/s.
    pub speed: f32,
    pub surfaces: [Contact; SURFACES],
    /// 0 = outside (chase camera) .. 1 = inside (bonnet camera).
    pub interior: f32,
    /// Engine and suspension damage 0..1 (misfires and knock; loose rattles).
    pub engine_damage: f32,
    pub susp_damage: f32,
    /// Speed (m/s) of the body sliding on the ground (0 when it isn't touching).
    pub scrape: f32,
    /// Speed (m/s) through brush, times how deep in it the car is (0 out of the shrubs).
    pub brush: f32,
    /// How hard the suspension is working (mean |compression speed| m/s over the wheels).
    pub rough: f32,
}

impl Default for CarParams {
    fn default() -> Self {
        CarParams {
            active: false,
            rpm: 950.0,
            idle_rpm: 950.0,
            redline: 7600.0,
            throttle: 0.0,
            shifting: false,
            gear: 1,
            speed: 0.0,
            surfaces: [Contact::default(); SURFACES],
            interior: 0.0,
            engine_damage: 0.0,
            susp_damage: 0.0,
            scrape: 0.0,
            brush: 0.0,
            rough: 0.0,
        }
    }
}

/// One-shot car events (detected on the main thread from state changes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CarEvent {
    /// Hit a tree/rock/post: closing speed m/s.
    Impact(f32),
    /// Snapped a sapling / post / sign: car speed m/s.
    Snap(f32),
    /// The body hit the ground (a roll, nosing into a bank, a heavy landing): m/s.
    Ground(f32),
    /// Suspension slam: strength 0..1, pan −1..1.
    Thump(f32, f32),
    /// The suspension ran out of travel: strength 0..1, pan −1..1.
    Bottom(f32, f32),
    /// Gear change (`true` = up).
    Shift(bool),
}

/// The whole car sound. `render` writes stereo frames.
#[derive(Clone, Debug)]
pub struct CarSynth {
    p: CarParams,
    n: Noise,
    bank: Option<&'static Bank>,
    pub engine: Engine,
    pub tyres: Tyres,
    pub body: Body,
    wind_lp: Biquad,
    wind_hiss: Biquad,
    buffet: Biquad,
    seal: Biquad,
    gust: Wander,
    speed: Smooth,
    interior: Smooth,
    /// Inside: the engine bay through the bulkhead, the exhaust through the body (low-passed,
    /// with the cabin's boom).
    cabin_bay: Biquad,
    cabin_eng: Biquad,
    cabin_boom: Biquad,
    /// Inside: the tyres and the world outside, muffled.
    cabin_l: Biquad,
    cabin_r: Biquad,
    active: Smooth,
    coef_timer: u32,
}

impl CarSynth {
    pub fn new(seed: u32) -> Self {
        let sm = |up, down| Smooth::new(0.0, up, down, RATE);
        CarSynth {
            p: CarParams::default(),
            n: Noise::new(seed),
            bank: samples::bank(),
            engine: Engine::new(seed),
            tyres: Tyres::new(seed),
            body: Body::new(seed),
            wind_lp: Biquad::lowpass(600.0, 0.6),
            wind_hiss: Biquad::bandpass(2500.0, 0.6),
            buffet: Biquad::lowpass(160.0, 0.9),
            seal: Biquad::bandpass(3300.0, 5.0),
            gust: Wander::new(0.4),
            speed: sm(0.1, 0.1),
            interior: sm(0.05, 0.05),
            cabin_bay: Biquad::lowpass(2600.0, 0.6),
            cabin_eng: Biquad::lowpass(850.0, 0.6),
            cabin_boom: Biquad::bandpass(118.0, 2.2),
            cabin_l: Biquad::lowpass(1700.0, 0.6),
            cabin_r: Biquad::lowpass(1700.0, 0.6),
            active: sm(0.05, 0.3),
            coef_timer: 0,
        }
    }

    pub fn params(&self) -> &CarParams {
        &self.p
    }

    pub fn set(&mut self, p: CarParams) {
        let mut p = p;
        // Never trust the input: clamp to ranges the synthesis is tuned for.
        let ok = |v: f32, lo: f32, hi: f32, d: f32| if v.is_finite() { v.clamp(lo, hi) } else { d };
        p.idle_rpm = ok(p.idle_rpm, 300.0, 3000.0, 950.0);
        p.redline = ok(p.redline, p.idle_rpm + 500.0, 12_000.0, 7600.0);
        p.rpm = ok(p.rpm, 0.0, p.redline + 500.0, p.idle_rpm);
        p.throttle = ok(p.throttle, 0.0, 1.0, 0.0);
        p.speed = ok(p.speed, 0.0, 120.0, 0.0);
        p.interior = ok(p.interior, 0.0, 1.0, 0.0);
        p.engine_damage = ok(p.engine_damage, 0.0, 1.0, 0.0);
        p.susp_damage = ok(p.susp_damage, 0.0, 1.0, 0.0);
        p.scrape = ok(p.scrape, 0.0, 60.0, 0.0);
        p.brush = ok(p.brush, 0.0, 60.0, 0.0);
        p.rough = ok(p.rough, 0.0, 10.0, 0.0);
        for c in &mut p.surfaces {
            c.roll = ok(c.roll, 0.0, 3.0, 0.0);
            c.slide = ok(c.slide, 0.0, 4.0, 0.0);
        }
        self.engine.set(&p);
        self.p = p;
    }

    pub fn event(&mut self, ev: CarEvent) {
        match ev {
            CarEvent::Impact(v) => self.body.impact(v, true),
            CarEvent::Snap(v) => self.body.snap(v),
            CarEvent::Ground(v) => self.body.impact(v * 0.8, false),
            CarEvent::Thump(s, p) => self.body.thump(s, p),
            CarEvent::Bottom(s, p) => self.body.bottom(s, p),
            CarEvent::Shift(up) => {
                self.body.shift();
                self.engine.shift(up, &self.p);
            }
        }
    }

    /// Updates filter coefficients from the smoothed state (every 32 samples: small enough
    /// steps that sweeps don't zipper).
    fn update_coeffs(&mut self) {
        let speed = self.speed.v;
        self.wind_lp.set_lowpass(180.0 + 28.0 * speed, 0.6);
        self.wind_hiss.set_bandpass(1500.0 + 40.0 * speed, 0.5);
        self.seal.set_bandpass(2900.0 + 12.0 * speed, 5.0);
    }

    /// Renders `frames` stereo frames, adding into `out` (interleaved L R).
    pub fn render(&mut self, out: &mut [f32]) {
        if self.bank.is_none() {
            self.bank = samples::bank();
        }
        for fr in out.chunks_exact_mut(2) {
            let (l, r) = self.sample();
            fr[0] += l;
            fr[1] += r;
        }
    }

    #[inline]
    fn sample(&mut self) -> (f32, f32) {
        let p = self.p;
        if self.coef_timer == 0 {
            self.update_coeffs();
            self.coef_timer = 32;
        }
        self.coef_timer -= 1;
        let act = self.active.next(if p.active { 1.0 } else { 0.0 });
        let speed = self.speed.next(p.speed);
        let inside = self.interior.next(p.interior);
        let out = 1.0 - inside;
        let bank = self.bank;
        let eng = self.engine.sample(&p, bank);
        let ty = self.tyres.sample(&p, speed, bank);
        let body = self.body.sample(&p, bank);
        // Wind. Outside: rushing air. Inside: low buffeting and a door-seal whistle.
        let gust = 0.8 + 0.2 * self.gust.next(&mut self.n);
        let wn = self.n.white();
        let v2 = (speed / 40.0).powi(2).min(1.5) * gust;
        let rush = (self.wind_lp.run(wn) * 1.4 + self.wind_hiss.run(wn) * 0.35) * v2 * 0.22;
        let cabin_wind = (self.buffet.run(wn) * 2.2 + self.seal.run(wn) * 0.08) * v2 * 0.2;
        // Engine. Outside: mostly the exhaust, some engine bay. Inside: the engine bay through
        // the bulkhead (intake, valve train), the exhaust dark and boomy through the body.
        let ex_out = eng.exhaust * 0.9 + eng.bay * 0.4;
        let ex_in = self.cabin_bay.run(eng.bay) * 0.85 + self.cabin_eng.run(eng.exhaust) * 0.75 + self.cabin_boom.run(eng.exhaust) * 0.5;
        let engine = ex_out * out + ex_in * inside;
        // Pops: direct outside, softer inside.
        let pop_g = out + 0.5 * inside;
        let (pop_l, pop_r) = (eng.pops.0 * pop_g, eng.pops.1 * pop_g);
        // Tyres: full band outside, muffled inside; stones on the floor are louder inside.
        let tl = ty.l * out + self.cabin_l.run(ty.l + rush) * 0.6 * inside;
        let tr = ty.r * out + self.cabin_r.run(ty.r + rush) * 0.6 * inside;
        let floor = 0.65 + 0.7 * inside;
        let shell = body.shell * (0.6 * inside);
        let mono = engine + rush * out + cabin_wind * inside + shell;
        let l = (mono + pop_l + tl + ty.floor_l * floor + body.l) * act;
        let r = (mono + pop_r + tr + ty.floor_r * floor + body.r) * act;
        (l, r)
    }
}


#[cfg(test)]
mod tests {
    use super::super::dsp::goertzel;
    use super::*;

    fn run(s: &mut CarSynth, secs: f32) -> Vec<f32> {
        let mut out = vec![0.0; (secs * RATE) as usize * 2];
        s.render(&mut out);
        out
    }

    fn mono(st: &[f32]) -> Vec<f32> {
        st.chunks_exact(2).map(|c| (c[0] + c[1]) * 0.5).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    fn driving(rpm: f32, throttle: f32) -> CarParams {
        CarParams { active: true, rpm, throttle, gear: 3, speed: 20.0, ..default() }
    }

    fn default() -> CarParams {
        CarParams::default()
    }

    fn check(out: &[f32], max: f32) {
        assert!(out.iter().all(|v| v.is_finite()), "NaN/inf");
        let peak = peak(out);
        assert!(peak < max, "peak {peak}");
    }

    /// Energy above `hz` over total (Goertzel probes on a log grid).
    fn brightness(x: &[f32], hz: f32) -> f32 {
        let (mut hi, mut all) = (0.0, 0.0);
        let mut f = 60.0;
        while f < 12_000.0 {
            let e = goertzel(x, f);
            all += e;
            if f > hz {
                hi += e;
            }
            f *= 1.12;
        }
        hi / all.max(1e-20)
    }

    #[test]
    fn deterministic() {
        let mut a = CarSynth::new(5);
        let mut b = CarSynth::new(5);
        a.set(driving(4000.0, 1.0));
        b.set(driving(4000.0, 1.0));
        assert_eq!(run(&mut a, 0.2), run(&mut b, 0.2));
    }

    #[test]
    fn silent_when_inactive() {
        let mut s = CarSynth::new(1);
        let out = run(&mut s, 0.3);
        assert!(out.iter().all(|v| *v == 0.0));
        s.set(driving(5000.0, 1.0));
        let on = run(&mut s, 0.5);
        assert!(rms(&on[24_000..]) > 0.01);
        s.set(CarParams { active: false, ..driving(5000.0, 1.0) });
        let off = run(&mut s, 2.0);
        assert!(rms(&off[off.len() - 4800..]) < 1e-3, "fades out");
    }

    #[test]
    fn levels_bounded_across_the_whole_range() {
        let mut s = CarSynth::new(9);
        for rpm in [950.0, 2500.0, 5000.0, 7600.0, 8200.0] {
            for th in [0.0, 0.5, 1.0] {
                let mut p = driving(rpm, th);
                p.speed = 35.0;
                p.surfaces[GRAVEL] = Contact { roll: 1.4, slide: 2.0 };
                p.surfaces[TARMAC] = Contact { roll: 0.5, slide: 2.0 };
                p.surfaces[OFFROAD] = Contact { roll: 1.0, slide: 1.0 };
                p.surfaces[MUD] = Contact { roll: 1.0, slide: 1.0 };
                p.surfaces[SNOW] = Contact { roll: 1.0, slide: 1.0 };
                p.engine_damage = 1.0;
                p.susp_damage = 1.0;
                p.scrape = 30.0;
                p.rough = 3.0;
                s.set(p);
                let out = run(&mut s, 0.25);
                check(&out, 2.0);
            }
        }
        for _ in 0..3 {
            s.event(CarEvent::Impact(30.0));
            s.event(CarEvent::Ground(20.0));
            s.event(CarEvent::Thump(1.0, -1.0));
            s.event(CarEvent::Bottom(1.0, 1.0));
            s.event(CarEvent::Shift(true));
        }
        let ev = run(&mut s, 0.5);
        check(&ev, 6.0); // 15 stacked worst-case one-shots; the stream's dynamics catch them
        // Garbage in, silence-or-sound out but never NaN.
        s.set(CarParams { active: true, rpm: f32::NAN, throttle: f32::INFINITY, speed: -5.0, interior: 7.0, engine_damage: f32::NAN, scrape: f32::INFINITY, ..default() });
        let g = run(&mut s, 0.2);
        check(&g, 2.0);
    }

    /// The engine's firing harmonics (rpm/30 Hz and multiples) sit where the rpm says, at the
    /// recorded rpms and between them (two loops pitched to the same rpm), on and off load.
    #[test]
    fn pitch_follows_rpm() {
        for th in [0.0, 1.0] {
            for rpm in [1500.0f32, 2500.0, 3300.0, 4500.0, 6000.0, 7200.0] {
                let mut s = CarSynth::new(3);
                let mut p = driving(rpm, th);
                p.speed = 0.0;
                s.set(p);
                let out = mono(&run(&mut s, 1.2));
                let x = &out[24_000..];
                let f = rpm / 30.0;
                let score = |f: f32| (1..=6).map(|k| goertzel(x, f * k as f32)).sum::<f32>();
                let on = score(f);
                // Harmonics of a wrong rpm (±18 %, between the half-orders) are weaker.
                for wrong in [f * 0.82, f * 1.18] {
                    let w = score(wrong);
                    assert!(on > 2.5 * w, "rpm {rpm} throttle {th}: on {on} vs {:.0} rpm {w}", wrong * 30.0);
                }
            }
        }
    }

    #[test]
    fn load_makes_it_louder() {
        let measure = |th: f32| {
            let mut s = CarSynth::new(4);
            let mut p = driving(4500.0, th);
            p.speed = 0.0;
            s.set(p);
            let out = mono(&run(&mut s, 0.8));
            rms(&out[19_200..])
        };
        let (quiet, loud) = (measure(0.0), measure(1.0));
        assert!(loud > quiet * 1.5, "{loud} vs {quiet}");
        // And louder with rpm.
        let rev = |rpm: f32| {
            let mut s = CarSynth::new(4);
            s.set(CarParams { speed: 0.0, ..driving(rpm, 1.0) });
            rms(&mono(&run(&mut s, 0.8))[19_200..])
        };
        assert!(rev(6500.0) > rev(1200.0) * 2.0);
    }

    #[test]
    fn lift_off_pops() {
        let mut pops = 0;
        for seed in 0..6 {
            let mut s = CarSynth::new(seed);
            s.set(driving(6500.0, 1.0));
            run(&mut s, 0.5);
            s.set(driving(6000.0, 0.0));
            assert!(s.engine.crackle > 0.9);
            for _ in 0..40 {
                run(&mut s, 0.02);
                pops += s.engine.popping() as u32;
            }
        }
        assert!(pops > 0, "some backfire pops in the first 0.8 s of overrun");
    }

    #[test]
    fn upshift_under_power_bangs_and_downshift_blips() {
        let mut bangs = 0;
        for seed in 0..8 {
            let mut s = CarSynth::new(seed);
            s.set(driving(7000.0, 1.0));
            run(&mut s, 0.3);
            s.event(CarEvent::Shift(true));
            bangs += s.engine.popping() as u32;
        }
        assert!(bangs >= 4, "upshift bangs {bangs}/8");
        // Downshift: a short blip of load on the overrun.
        let mut s = CarSynth::new(1);
        s.set(driving(4000.0, 0.0));
        run(&mut s, 0.5);
        let before = s.engine.load.v;
        s.event(CarEvent::Shift(false));
        run(&mut s, 0.05);
        assert!(s.engine.load.v > before + 0.3, "blip {} → {}", before, s.engine.load.v);
        run(&mut s, 0.4);
        assert!(s.engine.load.v < 0.1, "blip ends");
    }

    #[test]
    fn limiter_bounces() {
        let mut s = CarSynth::new(2);
        let mut p = driving(7600.0, 1.0);
        p.speed = 0.0;
        s.set(p);
        let out = mono(&run(&mut s, 1.0));
        // Envelope modulated at ~14 Hz: RMS of 10 ms windows varies a lot.
        let w: Vec<f32> = out[9600..].chunks(480).map(rms).collect();
        let (lo, hi) = w.iter().fold((f32::MAX, 0.0f32), |(a, b), v| (a.min(*v), b.max(*v)));
        assert!(hi > lo * 2.5, "lo {lo} hi {hi}");
    }

    /// The tyre part alone: the car with and without the contact, same seed, subtracted.
    fn tyre_level(surface: usize, roll: f32, slide: f32) -> (f32, Vec<f32>) {
        let mut s = CarSynth::new(8);
        let mut p = CarParams { active: true, rpm: 0.0, speed: 20.0, ..default() };
        p.surfaces[surface] = Contact { roll, slide };
        s.set(p);
        let out = mono(&run(&mut s, 0.8));
        let mut r = CarSynth::new(8);
        r.set(CarParams { active: true, rpm: 0.0, speed: 20.0, ..default() });
        let base = mono(&run(&mut r, 0.8));
        let d: Vec<f32> = out.iter().zip(&base).map(|(a, b)| a - b).collect();
        (rms(&d[9600..]), d[9600..].to_vec())
    }

    #[test]
    fn tyres_by_surface_and_slip() {
        for surf in [GRAVEL, TARMAC, MUD, SNOW, OFFROAD] {
            let (roll, _) = tyre_level(surf, 1.0, 0.0);
            let (slide, _) = tyre_level(surf, 1.0, 1.5);
            assert!(roll > 3e-3, "surface {surf} rolls: {roll}");
            assert!(slide > roll * 1.4, "surface {surf}: slide {slide} vs roll {roll}");
            assert!(slide < 0.5);
        }
        // Surfaces sound different: mud is the darkest.
        let b: Vec<f32> = [GRAVEL, TARMAC, MUD, SNOW].iter().map(|&s| brightness(&tyre_level(s, 1.0, 0.0).1, 1500.0)).collect();
        assert!(b[MUD] < b[GRAVEL] && b[MUD] < b[SNOW], "mud is the darkest: {b:?}");
    }

    #[test]
    fn wind_grows_with_speed() {
        let level = |speed: f32| {
            let mut s = CarSynth::new(6);
            s.set(CarParams { active: true, rpm: 0.0, speed, ..default() });
            let out = mono(&run(&mut s, 0.5));
            rms(&out[12_000..])
        };
        let (slow, fast) = (level(5.0), level(40.0));
        assert!(fast > slow * 10.0, "{slow} {fast}");
        assert!(fast < 0.5);
    }

    #[test]
    fn damaged_engine_misfires() {
        // Engine alone: the level from firing to firing varies far more when damaged.
        let measure = |dmg: f32| {
            let mut e = crate::audio::powertrain::Engine::new(3);
            let mut p = driving(3000.0, 1.0);
            p.engine_damage = dmg;
            e.set(&p);
            let bank = crate::audio::samples::bank();
            let ex: Vec<f32> = (0..144_000).map(|_| e.sample(&p, bank).exhaust).collect();
            // 100 Hz firing → 480 samples per firing; 2 firings per window.
            let fe: Vec<f32> = ex[48_000..].chunks(960).map(rms).collect();
            let mean = fe.iter().sum::<f32>() / fe.len() as f32;
            let var = fe.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / fe.len() as f32;
            var.sqrt() / mean
        };
        let (ok, bad) = (measure(0.0), measure(0.85));
        assert!(bad > ok * 1.5, "firing spread healthy {ok} damaged {bad}");
    }

    #[test]
    fn scrape_rattle_and_hits_are_heard() {
        let lvl = |p: CarParams, ev: Option<CarEvent>| {
            let mut s = CarSynth::new(2);
            s.set(p);
            run(&mut s, 0.2);
            if let Some(e) = ev {
                s.event(e);
            }
            let x = mono(&run(&mut s, 0.6));
            (rms(&x), peak(&x))
        };
        let quiet = CarParams { active: true, rpm: 0.0, speed: 8.0, ..default() };
        let (base, _) = lvl(quiet, None);
        let (scrape, _) = lvl(CarParams { scrape: 8.0, ..quiet }, None);
        assert!(scrape > base * 3.0, "scrape {scrape} vs {base}");
        let (brush, _) = lvl(CarParams { brush: 10.0, ..quiet }, None);
        assert!(brush > base * 2.0, "brush {brush} vs {base}");
        let (_, snap) = lvl(quiet, Some(CarEvent::Snap(16.0)));
        let (_, crash) = lvl(quiet, Some(CarEvent::Impact(16.0)));
        assert!(snap > 0.02 && snap < crash, "snap {snap} vs crash {crash}");
        let rough = CarParams { rough: 1.5, ..quiet };
        let (tight, _) = lvl(rough, None);
        let (loose, _) = lvl(CarParams { susp_damage: 0.8, ..rough }, None);
        assert!(loose > tight * 1.5, "rattles with damage {loose} vs {tight}");
        let (_, small) = lvl(quiet, Some(CarEvent::Impact(4.0)));
        let (_, big) = lvl(quiet, Some(CarEvent::Impact(20.0)));
        assert!(big > small * 1.8, "impact scales: {small} → {big}");
        let (_, bottom) = lvl(quiet, Some(CarEvent::Bottom(1.0, 0.0)));
        assert!(bottom > 0.1, "bottoming out {bottom}");
    }

    #[test]
    fn inside_is_darker() {
        let render = |interior: f32| {
            let mut s = CarSynth::new(5);
            let mut p = driving(5000.0, 1.0);
            p.speed = 25.0;
            p.surfaces[GRAVEL] = Contact { roll: 1.0, slide: 0.3 };
            p.interior = interior;
            s.set(p);
            run(&mut s, 0.5);
            mono(&run(&mut s, 1.0))
        };
        let (outside, inside) = (render(0.0), render(1.0));
        let (bo, bi) = (brightness(&outside, 2000.0), brightness(&inside, 2000.0));
        assert!(bi < bo * 0.7, "inside is muffled: {bi} vs {bo}");
    }

    /// No clicks: a long rpm sweep through every crossfade with throttle stabs and slides never
    /// jumps sample to sample far more than the recordings' own movement, and the output never
    /// sits in denormals.
    #[test]
    fn sweeps_are_smooth_and_never_denormal() {
        let mut s = CarSynth::new(12);
        let mut out = Vec::new();
        for i in 0..600 {
            let t = i as f32 / 100.0;
            let mut p = driving(900.0 + 1150.0 * t, if (i / 40) % 2 == 0 { 1.0 } else { 0.0 });
            p.speed = 10.0 + 5.0 * t;
            p.surfaces[TARMAC] = Contact { roll: 0.8, slide: if (i / 70) % 2 == 0 { 0.0 } else { 1.2 } };
            s.set(p);
            out.extend(mono(&run(&mut s, 0.01)));
        }
        let d: Vec<f32> = out.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        let typical = rms(&d);
        let worst = d.iter().fold(0.0f32, |m, v| m.max(*v));
        assert!(worst < typical * 25.0, "worst step {worst} vs typical {typical}");
        // Silence after: decays to exact zeros, not denormals.
        s.set(CarParams { active: false, ..default() });
        let tail = run(&mut s, 3.0);
        assert!(tail.iter().all(|v| *v == 0.0 || v.abs() > 1e-30), "denormal output");
    }

    /// The level doesn't jump at a crossfade point: RMS in 150 ms steps (long enough to hold a
    /// few idle firings) along a slow rpm sweep changes smoothly: no step bigger than 3 dB
    /// between neighbouring windows. (Two recordings pitched to the same rpm drift in and out
    /// of phase, so the middle of a crossfade can dip ~2 dB for a moment; a gain error at a
    /// crossover would be a step of 6 dB or more.)
    #[test]
    fn crossfades_hold_the_level() {
        for th in [0.0, 1.0] {
            let mut s = CarSynth::new(7);
            let mut lv = Vec::new();
            for i in 0..200 {
                let rpm = 1000.0 * (7.4f32).powf(i as f32 / 200.0);
                s.set(CarParams { speed: 0.0, ..driving(rpm, th) });
                let x = mono(&run(&mut s, 0.15));
                if i > 10 {
                    lv.push(20.0 * rms(&x).log10());
                }
            }
            let (at, worst) = lv.windows(2).enumerate().map(|(i, w)| (i, (w[1] - w[0]).abs())).fold((0, 0.0f32), |a, b| if b.1 > a.1 { b } else { a });
            let rpm = 1000.0 * (7.4f32).powf((at + 12) as f32 / 200.0);
            assert!(worst < 3.0, "throttle {th}: level step {worst} dB at {rpm:.0} rpm: {:?}", &lv[at.saturating_sub(4)..(at + 5).min(lv.len())]);
        }
    }

    /// `cargo test --lib report_levels -- --ignored --nocapture`: levels per scenario.
    #[test]
    #[ignore]
    fn report_levels() {
        let lvl = |p: CarParams| {
            let mut s = CarSynth::new(9);
            s.set(p);
            run(&mut s, 0.5);
            let x = mono(&run(&mut s, 1.0));
            (20.0 * rms(&x).log10(), 20.0 * peak(&x).log10())
        };
        let quiet = CarParams { active: true, rpm: 0.0, ..default() };
        for interior in [0.0, 1.0] {
            for (name, rpm, th) in [("idle", 950.0, 0.0), ("2500 lift", 2500.0, 0.0), ("2500 full", 2500.0, 1.0), ("4000 full", 4000.0, 1.0), ("6500 lift", 6500.0, 0.0), ("6500 full", 6500.0, 1.0)] {
                let (r, p) = lvl(CarParams { active: true, rpm, throttle: th, interior, ..default() });
                println!("engine {name} inside {interior}: rms {r:.1} dB peak {p:.1} dB");
            }
        }
        for (name, p) in [("wind 15", CarParams { speed: 15.0, ..quiet }), ("wind 35", CarParams { speed: 35.0, ..quiet })] {
            let (r, _) = lvl(p);
            println!("{name}: rms {r:.1} dB");
        }
        for surf in 0..SURFACES {
            for (r, sl) in [(1.0, 0.0), (1.0, 1.0), (1.0, 2.0)] {
                let mut p = CarParams { speed: 20.0, ..quiet };
                p.surfaces[surf] = Contact { roll: r, slide: sl };
                let (rm, _) = lvl(p);
                println!("surface {surf} roll {r} slide {sl}: rms {rm:.1} dB");
            }
        }
        for (name, ev) in [("impact", CarEvent::Impact(30.0)), ("impact5", CarEvent::Impact(5.0)), ("ground", CarEvent::Ground(10.0)), ("snap", CarEvent::Snap(15.0)), ("thump", CarEvent::Thump(1.0, 0.0)), ("bottom", CarEvent::Bottom(1.0, 0.0)), ("shift", CarEvent::Shift(true))] {
            let mut s = CarSynth::new(9);
            s.set(CarParams { active: true, rpm: 0.0, ..default() });
            run(&mut s, 0.2);
            s.event(ev);
            let out = run(&mut s, 0.5);
            println!("{name}: peak {:.1} dB", 20.0 * peak(&out).log10());
        }
    }
}
