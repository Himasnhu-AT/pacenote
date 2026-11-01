//! The car, synthesised, and its inside/outside mix. The parts:
//! - [`super::powertrain`]: engine (per-cylinder exhaust pulses through pipe waveguides), intake,
//!   pops, limiter, misfires and knock when damaged; turbo whistle, blow-off and flutter.
//! - [`super::tyres`]: rolling and sliding per surface, stones on the arches and floor.
//! - [`super::body`]: impacts, ground hits, suspension thumps and bottoming, scrapes,
//!   rattles, gear clunks.
//! - here: gear whine, wind, and the mix for the chase camera (outside) or the bonnet camera
//!   (inside: exhaust through the bulkhead, tyres muffled, intake/whine/stones/rattles up).
//!
//! Runs on the audio thread; parameters arrive once per block (see [`CarParams`]) and are
//! smoothed per sample.

use super::body::Body;
use super::dsp::{Biquad, Noise, Osc, RATE, Smooth, Wander, smoothstep};
use super::powertrain::{Engine, Turbo};
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
    pub engine: Engine,
    pub turbo: Turbo,
    pub tyres: Tyres,
    pub body: Body,
    whine: Osc,
    whine2: Osc,
    whine_lvl: Smooth,
    wind_lp: Biquad,
    wind_hiss: Biquad,
    buffet: Biquad,
    seal: Biquad,
    gust: Wander,
    speed: Smooth,
    interior: Smooth,
    /// Inside: the exhaust through the body (low-passed, with the cabin's boom).
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
            engine: Engine::new(seed),
            turbo: Turbo::new(seed),
            tyres: Tyres::new(seed),
            body: Body::new(seed),
            whine: Osc::default(),
            whine2: Osc::default(),
            whine_lvl: sm(0.1, 0.1),
            wind_lp: Biquad::lowpass(600.0, 0.6),
            wind_hiss: Biquad::bandpass(2500.0, 0.6),
            buffet: Biquad::lowpass(160.0, 0.9),
            seal: Biquad::bandpass(3300.0, 5.0),
            gust: Wander::new(0.4),
            speed: sm(0.1, 0.1),
            interior: sm(0.05, 0.05),
            cabin_eng: Biquad::lowpass(750.0, 0.6),
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
        self.turbo.set(&p);
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
                self.turbo.shift(up);
            }
        }
    }

    /// Updates filter coefficients from the smoothed state (every 32 samples: small enough
    /// steps that sweeps don't zipper).
    fn update_coeffs(&mut self) {
        let p = self.p;
        self.engine.update_coeffs(&p);
        self.turbo.update_coeffs();
        self.tyres.update_coeffs();
        let speed = self.speed.v;
        self.wind_lp.set_lowpass(180.0 + 28.0 * speed, 0.6);
        self.wind_hiss.set_bandpass(1500.0 + 40.0 * speed, 0.5);
        self.seal.set_bandpass(2900.0 + 12.0 * speed, 5.0);
    }

    /// Renders `frames` stereo frames, adding into `out` (interleaved L R).
    pub fn render(&mut self, out: &mut [f32]) {
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
        let eng = self.engine.sample(&p);
        let ty = self.tyres.sample(&p, speed);
        let body = self.body.sample(&p);
        let turbo = self.turbo.sample(self.engine.load.v, self.engine.rpm.v);
        // Gear whine: straight-cut gears; the input shaft mesh and the final drive, louder
        // under load and on the overrun, much louder inside.
        let e = &self.engine;
        let shaft = e.rpm.v / 60.0;
        let g = p.gear.max(1) as f32;
        let wf = shaft * (17.0 + 2.0 * g);
        let wl = self.whine_lvl.next(if p.gear != 0 { (0.3 + 0.7 * e.load.v) * smoothstep(2.0, 12.0, speed) } else { 0.0 });
        let whine = (self.whine.next(wf) + 0.35 * self.whine2.next(wf * 2.0)) * wl * (0.004 + 0.012 * inside);
        // Wind. Outside: rushing air. Inside: low buffeting and a door-seal whistle.
        let gust = 0.8 + 0.2 * self.gust.next(&mut self.n);
        let wn = self.n.white();
        let v2 = (speed / 40.0).powi(2).min(1.5) * gust;
        let rush = (self.wind_lp.run(wn) * 1.4 + self.wind_hiss.run(wn) * 0.35) * v2 * 0.22;
        let cabin_wind = (self.buffet.run(wn) * 2.2 + self.seal.run(wn) * 0.08) * v2 * 0.2;
        // Exhaust: direct outside; through the bulkhead inside (dark, boomy).
        let ex_out = eng.exhaust + eng.pops;
        let ex_in = self.cabin_eng.run(eng.exhaust + 0.5 * eng.pops);
        let boom = self.cabin_boom.run(eng.exhaust);
        let exhaust = ex_out * out + (ex_in * 1.25 + boom * 0.7) * inside;
        let mech = eng.mech * (0.6 + 0.9 * inside);
        // Tyres: full band outside, muffled inside; stones on the floor are louder inside.
        let tl = ty.l * out + self.cabin_l.run(ty.l + rush) * 0.6 * inside;
        let tr = ty.r * out + self.cabin_r.run(ty.r + rush) * 0.6 * inside;
        let floor = 0.65 + 0.7 * inside;
        let shell = body.shell * (0.6 * inside);
        let mono = exhaust + mech + turbo * (0.9 + 0.2 * inside) + whine + rush * out + cabin_wind * inside + shell;
        let l = (mono + tl + ty.floor_l * floor + body.l) * act;
        let r = (mono + tr + ty.floor_r * floor + body.r) * act;
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

    /// Energy at the firing-order harmonics (rpm/30 Hz and multiples) dominates energy between
    /// them, and moves with rpm.
    #[test]
    fn spectrum_follows_rpm() {
        for rpm in [1800.0f32, 3000.0, 4500.0, 6000.0] {
            let mut s = CarSynth::new(3);
            let mut p = driving(rpm, 1.0);
            p.speed = 0.0;
            s.set(p);
            let out = mono(&run(&mut s, 1.0));
            let x = &out[24_000..];
            let f = rpm / 30.0;
            let on: f32 = (1..=6).map(|k| goertzel(x, f * k as f32)).sum();
            let off: f32 = (1..=6).map(|k| goertzel(x, f * (k as f32 + 0.37))).sum();
            assert!(on > 8.0 * off, "rpm {rpm}: on {on} off {off}");
            // Wrong rpm's harmonics are weak.
            let wrong: f32 = (1..=3).map(|k| goertzel(x, f * 1.37 * k as f32)).sum();
            assert!(on > 4.0 * wrong, "rpm {rpm}: on {on} wrong {wrong}");
            // Uneven cylinders: some half-order (rpm/60 multiples) content, well below the
            // firing harmonics.
            let half: f32 = (0..6).map(|k| goertzel(x, f * (k as f32 + 0.5))).sum();
            assert!(half > on * 1e-4 && half < on * 0.5, "rpm {rpm}: half-orders {half} vs {on}");
        }
    }

    #[test]
    fn load_makes_it_louder_and_brighter() {
        let measure = |th: f32| {
            let mut s = CarSynth::new(4);
            let mut p = driving(4500.0, th);
            p.speed = 0.0;
            s.set(p);
            let out = mono(&run(&mut s, 0.8));
            let x = &out[19_200..];
            let hi: f32 = (8..=14).map(|k| goertzel(x, 150.0 * k as f32)).sum();
            (rms(x), hi)
        };
        let (quiet, dull) = measure(0.0);
        let (loud, bright) = measure(1.0);
        assert!(loud > quiet * 1.5, "{loud} vs {quiet}");
        assert!(bright > dull * 2.0, "{bright} vs {dull}");
    }

    /// At low rpm the firings are pulses with the pipe ringing between them, not isolated
    /// clicks in silence.
    #[test]
    fn low_rpm_is_pulses_not_clicks() {
        let mut s = CarSynth::new(4);
        let mut p = driving(1300.0, 1.0);
        p.speed = 0.0;
        s.set(p);
        let x = mono(&run(&mut s, 1.0));
        let x = &x[24_000..];
        // Over one firing period (rpm/30 Hz → 1100 samples), the share of samples within
        // 5 % of the peak level must not be tiny (a click train sits near zero most of the
        // time).
        let pk = peak(x);
        let busy = x.iter().filter(|v| v.abs() > pk * 0.05).count() as f32 / x.len() as f32;
        assert!(busy > 0.3, "busy {busy}");
        // Crest factor of a click train is huge; pulses with ringing are moderate.
        let crest = pk / rms(x);
        assert!(crest < 8.0, "crest {crest}");
    }

    #[test]
    fn lift_off_pops_and_blow_off() {
        let mut s = CarSynth::new(11);
        s.set(driving(6500.0, 1.0));
        run(&mut s, 2.0); // spool up
        assert!(s.turbo.boost > 0.5, "boost {}", s.turbo.boost);
        s.set(driving(6000.0, 0.0));
        assert!(s.turbo.bov.active(), "blow-off on lift");
        assert!(s.engine.crackle > 0.9);
        let mut pops = 0;
        for _ in 0..40 {
            run(&mut s, 0.02);
            pops += s.engine.popping() as u32;
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
        // Squeal is tonal on tarmac: the saw's harmonics stand out of the scrub around them.
        let (_, d) = tyre_level(TARMAC, 1.0, 1.5);
        let f0 = 420.0 + 90.0 * 1.5;
        let on: f32 = (1..=5).map(|k| goertzel(&d, f0 * k as f32)).sum();
        let off: f32 = (1..=5).map(|k| goertzel(&d, f0 * (k as f32 + 0.5))).sum();
        assert!(on > 3.0 * off, "on {on} off {off}");
        // Surfaces sound different: their spectra (brightness above 1.5 kHz) differ.
        let b: Vec<f32> = [GRAVEL, TARMAC, MUD, SNOW].iter().map(|&s| brightness(&tyre_level(s, 1.0, 0.0).1, 1500.0)).collect();
        assert!(b[MUD] < b[GRAVEL] && b[MUD] < b[SNOW], "mud is the darkest: {b:?}");
        assert!(b[SNOW] > b[TARMAC], "snow squeaks above tarmac hum: {b:?}");
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
    fn damaged_engine_misfires_and_knocks() {
        // Engine alone: per-firing energy varies far more when damaged (misfires), and the
        // mechanical output (intake, valve train, knock) gains a knock band.
        let measure = |dmg: f32| {
            let mut e = crate::audio::powertrain::Engine::new(3);
            let mut p = driving(3000.0, 1.0);
            p.engine_damage = dmg;
            e.set(&p);
            let (mut ex, mut mech) = (Vec::new(), Vec::new());
            for i in 0..144_000 {
                if i % 32 == 0 {
                    e.update_coeffs(&p);
                }
                let o = e.sample(&p);
                ex.push(o.exhaust);
                mech.push(o.mech);
            }
            // 100 Hz firing → 480 samples per firing.
            let fe: Vec<f32> = ex[48_000..].chunks(480).map(rms).collect();
            let mean = fe.iter().sum::<f32>() / fe.len() as f32;
            let var = fe.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / fe.len() as f32;
            let knock: f32 = (0..10).map(|k| goertzel(&mech[48_000..], 1100.0 + 80.0 * k as f32)).sum();
            (var.sqrt() / mean, knock)
        };
        let (ok, ok_k) = measure(0.0);
        let (bad, bad_k) = measure(0.85);
        assert!(bad > ok * 2.0, "firing spread healthy {ok} damaged {bad}");
        assert!(bad_k > ok_k * 3.0, "knock {ok_k} → {bad_k}");
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
    fn inside_is_darker_with_more_mechanism() {
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
        // Gear whine (input shaft × 23 at 3rd) is louder inside.
        let wf = 5000.0 / 60.0 * 23.0;
        assert!(goertzel(&inside, wf) > goertzel(&outside, wf) * 2.0);
    }

    /// No discontinuities: a long rpm sweep with throttle stabs never jumps sample to sample
    /// more than the signal's own movement allows, and the output never sits in denormals.
    #[test]
    fn sweeps_are_smooth_and_never_denormal() {
        let mut s = CarSynth::new(12);
        let mut out = Vec::new();
        for i in 0..300 {
            let t = i as f32 / 100.0;
            let mut p = driving(1000.0 + 2000.0 * t, if (i / 40) % 2 == 0 { 1.0 } else { 0.0 });
            p.speed = 10.0 + 5.0 * t;
            p.surfaces[TARMAC] = Contact { roll: 0.8, slide: if (i / 70) % 2 == 0 { 0.0 } else { 1.2 } };
            s.set(p);
            out.extend(mono(&run(&mut s, 0.01)));
        }
        let d2: Vec<f32> = out.windows(3).map(|w| (w[2] - 2.0 * w[1] + w[0]).abs()).collect();
        let typical = rms(&d2);
        let worst = d2.iter().fold(0.0f32, |m, v| m.max(*v));
        assert!(worst < typical * 40.0, "worst step {worst} vs typical {typical}");
        // Silence after: decays to exact zeros, not denormals.
        s.set(CarParams { active: false, ..default() });
        let tail = run(&mut s, 3.0);
        assert!(tail.iter().all(|v| *v == 0.0 || v.abs() > 1e-30), "denormal output");
    }

    /// `cargo test --lib report_levels -- --ignored --nocapture`: levels per scenario.
    #[test]
    #[ignore]
    fn report_levels() {
        let lvl = |p: CarParams| {
            let mut s = CarSynth::new(9);
            s.set(p);
            run(&mut s, 0.5);
            rms(&mono(&run(&mut s, 1.0)))
        };
        let quiet = CarParams { active: true, rpm: 0.0, ..default() };
        for rpm in [2000.0f32, 4000.0, 6100.0] {
            let mut s = CarSynth::new(9);
            s.set(CarParams { active: true, rpm, throttle: 1.0, ..default() });
            run(&mut s, 0.5);
            let x = mono(&run(&mut s, 0.5));
            let f = rpm / 60.0;
            let v: Vec<String> = (1..=12).map(|k| format!("{:.1}", 10.0 * goertzel(&x, f * k as f32).max(1e-20).log10())).collect();
            println!("rpm {rpm}: half-order dB {}", v.join(" "));
        }
        for (name, p) in [
            ("engine idle", CarParams { active: true, rpm: 950.0, ..default() }),
            ("engine 3000 full", CarParams { active: true, rpm: 3000.0, throttle: 1.0, ..default() }),
            ("engine 6500 full", CarParams { active: true, rpm: 6500.0, throttle: 1.0, ..default() }),
            ("engine 6500 lift", CarParams { active: true, rpm: 6500.0, throttle: 0.0, ..default() }),
            ("wind 15", CarParams { speed: 15.0, ..quiet }),
            ("wind 35", CarParams { speed: 35.0, ..quiet }),
        ] {
            println!("{name}: rms {:.4}", lvl(p));
        }
        for surf in 0..SURFACES {
            for (r, sl) in [(1.0, 0.0), (1.0, 1.0), (1.0, 2.0)] {
                let mut p = quiet;
                p.surfaces[surf] = Contact { roll: r, slide: sl };
                println!("surface {surf} roll {r} slide {sl}: rms {:.4}", lvl(p));
            }
        }
        for (name, ev) in [("impact", CarEvent::Impact(30.0)), ("impact5", CarEvent::Impact(5.0)), ("thump", CarEvent::Thump(1.0, 0.0)), ("bottom", CarEvent::Bottom(1.0, 0.0)), ("shift", CarEvent::Shift(true))] {
            let mut s = CarSynth::new(9);
            s.set(CarParams { active: true, rpm: 0.0, ..default() });
            run(&mut s, 0.2);
            s.event(ev);
            let out = run(&mut s, 0.5);
            println!("{name}: peak {:.3}", peak(&out));
        }
    }
}
