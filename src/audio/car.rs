//! The car, synthesised: engine (pulse train through exhaust formants), turbo (whistle,
//! blow-off, flutter), backfire pops, rev limiter, gear whine and clunk, tyres per surface,
//! stones, wind, suspension thumps and impacts. Runs on the audio thread; parameters arrive
//! once per block (see [`CarParams`]) and are smoothed per sample.

use super::dsp::{Biquad, Decay, Noise, OnePole, Osc, RATE, Ring, Smooth, Wander, pan, smoothstep};

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
        }
    }
}

/// One-shot car events (detected on the main thread from state changes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CarEvent {
    /// Hit a tree/rock/post: impact speed m/s.
    Impact(f32),
    /// Suspension slam: strength 0..1, pan −1..1.
    Thump(f32, f32),
    /// Gear change (clunk).
    Shift,
}

const CYL_GAIN: [f32; 4] = [1.0, 0.84, 0.95, 0.89];

/// Engine: four cylinders, 2 firings per revolution, each an impulse shaped into a pulse
/// (brighter with load) plus combustion noise, through exhaust/intake formants.
#[derive(Clone, Debug)]
struct Engine {
    n: Noise,
    phase: f32,
    rpm: Smooth,
    load: Smooth,
    level: Smooth,
    shape_a: OnePole,
    shape_b: OnePole,
    burst: Decay,
    body: Biquad,
    mid: Biquad,
    rasp: Biquad,
    intake: Biquad,
    tone: Biquad,
    dc: OnePole,
    wobble: Wander,
    limiter: f32,
    /// 1 right after a lift (pops are likely), decays.
    crackle: f32,
    last_throttle: f32,
    pops: [Pop; 3],
    next_pop: usize,
    coef_timer: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Pop {
    noise: Decay,
    boom: Ring,
    crack: Biquad,
}

#[derive(Clone, Debug)]
struct Turbo {
    n: Noise,
    boost: f32,
    shaft: Smooth,
    whistle: Osc,
    hiss: Biquad,
    bov: Decay,
    bov_f: f32,
    bov_bp: Biquad,
    flutter: Decay,
    flutter_osc: Osc,
}

#[derive(Clone, Debug)]
struct Tyres {
    n: Noise,
    roll: [Smooth; SURFACES],
    slide: [Smooth; SURFACES],
    gravel_bp: Biquad,
    gravel_lo: Biquad,
    grain: Decay,
    grain_f: Biquad,
    slide_bp: Biquad,
    tarmac_lo: Biquad,
    squeal: Osc,
    squeal_bp: Biquad,
    squeal_wander: Wander,
    mud_lo: Biquad,
    mud_slurp: Biquad,
    mud_wander: Wander,
    snow_bp: Biquad,
    snow_grain: Decay,
    grass_hp: Biquad,
    rumble: Biquad,
    rumble_wander: Wander,
    pings: [(Ring, f32); 8],
    next_ping: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct Hit {
    thud: Ring,
    metal: [Ring; 3],
    noise: Decay,
    pan: f32,
}

/// The whole car sound. `render` writes stereo frames.
#[derive(Clone, Debug)]
pub struct CarSynth {
    p: CarParams,
    n: Noise,
    engine: Engine,
    turbo: Turbo,
    tyres: Tyres,
    whine: Osc,
    whine_lvl: Smooth,
    wind_lp: Biquad,
    wind_hiss: Biquad,
    gust: Wander,
    speed: Smooth,
    interior: Smooth,
    cabin: OnePole,
    active: Smooth,
    hits: [Hit; 3],
    next_hit: usize,
    thumps: [(Ring, Decay, f32); 4],
    next_thump: usize,
    clunk: Ring,
    clunk_tick: Ring,
}

impl CarSynth {
    pub fn new(seed: u32) -> Self {
        let sm = |up, down| Smooth::new(0.0, up, down, RATE);
        CarSynth {
            p: CarParams::default(),
            n: Noise::new(seed),
            engine: Engine {
                n: Noise::new(seed ^ 0x1111),
                phase: 0.0,
                rpm: Smooth::new(950.0, 0.02, 0.03, RATE),
                load: sm(0.03, 0.06),
                level: sm(0.05, 0.2),
                shape_a: OnePole::new(1500.0),
                shape_b: OnePole::new(1500.0),
                burst: Decay::default(),
                body: Biquad::bandpass(115.0, 1.1),
                mid: Biquad::bandpass(430.0, 2.2),
                rasp: Biquad::bandpass(1350.0, 1.8),
                intake: Biquad::bandpass(820.0, 0.9),
                tone: Biquad::lowpass(3000.0, 0.7),
                dc: OnePole::new(28.0),
                wobble: Wander::new(3.0),
                limiter: 0.0,
                crackle: 0.0,
                last_throttle: 0.0,
                pops: [Pop::default(); 3],
                next_pop: 0,
                coef_timer: 0,
            },
            turbo: Turbo {
                n: Noise::new(seed ^ 0x2222),
                boost: 0.0,
                shaft: sm(0.5, 0.9),
                whistle: Osc::default(),
                hiss: Biquad::bandpass(5000.0, 6.0),
                bov: Decay::default(),
                bov_f: 3500.0,
                bov_bp: Biquad::bandpass(3500.0, 1.2),
                flutter: Decay::default(),
                flutter_osc: Osc::default(),
            },
            tyres: Tyres {
                n: Noise::new(seed ^ 0x3333),
                roll: [sm(0.05, 0.08); SURFACES],
                slide: [sm(0.04, 0.1); SURFACES],
                gravel_bp: Biquad::bandpass(1900.0, 0.8),
                gravel_lo: Biquad::lowpass(240.0, 0.7),
                grain: Decay::default(),
                grain_f: Biquad::bandpass(2600.0, 1.4),
                slide_bp: Biquad::bandpass(3200.0, 0.7),
                tarmac_lo: Biquad::lowpass(480.0, 0.7),
                squeal: Osc::default(),
                squeal_bp: Biquad::bandpass(1000.0, 8.0),
                squeal_wander: Wander::new(5.0),
                mud_lo: Biquad::lowpass(380.0, 0.9),
                mud_slurp: Biquad::bandpass(420.0, 3.0),
                mud_wander: Wander::new(4.0),
                snow_bp: Biquad::bandpass(3600.0, 1.6),
                snow_grain: Decay::default(),
                grass_hp: Biquad::highpass(3500.0, 0.7),
                rumble: Biquad::lowpass(110.0, 0.8),
                rumble_wander: Wander::new(6.0),
                pings: [(Ring::default(), 0.0); 8],
                next_ping: 0,
            },
            whine: Osc::default(),
            whine_lvl: sm(0.1, 0.1),
            wind_lp: Biquad::lowpass(600.0, 0.6),
            wind_hiss: Biquad::bandpass(2500.0, 0.6),
            gust: Wander::new(0.4),
            speed: sm(0.1, 0.1),
            interior: sm(0.05, 0.05),
            cabin: OnePole::new(20_000.0),
            active: sm(0.05, 0.3),
            hits: [Hit::default(); 3],
            next_hit: 0,
            thumps: [(Ring::default(), Decay::default(), 0.0); 4],
            next_thump: 0,
            clunk: Ring::default(),
            clunk_tick: Ring::default(),
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
        for c in &mut p.surfaces {
            c.roll = ok(c.roll, 0.0, 3.0, 0.0);
            c.slide = ok(c.slide, 0.0, 4.0, 0.0);
        }
        // Throttle lift at high rpm: crackle window and blow-off.
        let e = &mut self.engine;
        if p.throttle < 0.25 && e.last_throttle >= 0.5 {
            if p.rpm > 3200.0 {
                e.crackle = 1.0;
            }
            let b = self.turbo.boost;
            if b > 0.3 {
                self.turbo.bov.trigger(b * 0.22, 0.45);
                self.turbo.bov_f = 4200.0;
                self.turbo.flutter.trigger(b * 0.12, 0.35);
            }
        }
        e.last_throttle = p.throttle;
        self.p = p;
    }

    pub fn event(&mut self, ev: CarEvent) {
        match ev {
            CarEvent::Impact(v) => {
                let a = (v / 8.0).clamp(0.05, 1.0);
                let h = &mut self.hits[self.next_hit];
                self.next_hit = (self.next_hit + 1) % 3;
                let n = &mut self.n;
                h.thud.trigger(n.range(42.0, 58.0), 0.35 + 0.3 * a, 0.35 * a);
                for (m, f) in h.metal.iter_mut().zip([1130.0, 2370.0, 3710.0]) {
                    m.trigger(f * n.range(0.9, 1.1), n.range(0.25, 0.5), 0.05 * a * n.range(0.5, 1.0));
                }
                h.noise.trigger(0.3 * a, 0.18 + 0.15 * a);
                h.pan = n.range(-0.4, 0.4);
            }
            CarEvent::Thump(s, p) => {
                let s = s.clamp(0.0, 1.0);
                let t = &mut self.thumps[self.next_thump];
                self.next_thump = (self.next_thump + 1) % 4;
                t.0.trigger(self.n.range(55.0, 80.0), 0.18, 0.4 * s);
                t.1.trigger(0.12 * s, 0.02);
                t.2 = p.clamp(-1.0, 1.0) * 0.5;
            }
            CarEvent::Shift => {
                self.clunk.trigger(self.n.range(150.0, 190.0), 0.08, 0.06);
                self.clunk_tick.trigger(self.n.range(2600.0, 3100.0), 0.03, 0.025);
            }
        }
    }

    /// Updates filter coefficients from the smoothed state (every 64 samples).
    fn update_coeffs(&mut self) {
        let e = &mut self.engine;
        let load = e.load.v;
        let rpm_n = ((e.rpm.v - self.p.idle_rpm) / (self.p.redline - self.p.idle_rpm)).clamp(0.0, 1.0);
        // Pulse width: soft on overrun, sharp under load and at high rpm.
        let shape = 500.0 + 1700.0 * load + 900.0 * rpm_n;
        e.shape_a.set(shape);
        e.shape_b.set(shape);
        e.tone.set_lowpass(1100.0 + 3600.0 * load + 1800.0 * rpm_n, 0.75);
        e.rasp.set_bandpass(1250.0 + 500.0 * rpm_n, 1.8);
        let t = &mut self.turbo;
        t.hiss.set_bandpass(2400.0 + 5200.0 * t.shaft.v, 7.0);
        t.bov_bp.set_bandpass(t.bov_f, 1.3);
        let speed = self.speed.v;
        self.wind_lp.set_lowpass(180.0 + 28.0 * speed, 0.6);
        self.wind_hiss.set_bandpass(1500.0 + 40.0 * speed, 0.5);
        let ty = &mut self.tyres;
        let sq = 820.0 + 260.0 * ty.slide[TARMAC].v.min(2.0);
        ty.squeal_bp.set_bandpass(sq, 9.0);
        // Inside the car: the outside world (tyres, wind) is muffled.
        self.cabin.set(20_000.0 - 17_500.0 * self.interior.v);
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
        if self.engine.coef_timer == 0 {
            self.update_coeffs();
            self.engine.coef_timer = 64;
        }
        self.engine.coef_timer -= 1;
        let act = self.active.next(if p.active { 1.0 } else { 0.0 });
        let speed = self.speed.next(p.speed);
        let interior = self.interior.next(p.interior);
        let (eng, pops) = self.engine_sample(p);
        let (tl, tr) = self.tyre_sample(speed);
        let turbo = self.turbo_sample(p);
        // Gear whine: straight-cut gears, pitch from the input shaft and gear.
        let e = &self.engine;
        let shaft = e.rpm.v / 60.0;
        let wf = shaft * (17.0 + 2.0 * p.gear.max(1) as f32);
        let wl = self.whine_lvl.next(if p.gear != 0 { (0.25 + e.load.v) * smoothstep(2.0, 12.0, speed) } else { 0.0 });
        let whine = self.whine.next(wf) * wl * (0.006 + 0.01 * interior);
        // Wind.
        let gust = 0.8 + 0.2 * self.gust.next(&mut self.n);
        let wn = self.n.white();
        let w = self.wind_lp.run(wn) * 1.4 + self.wind_hiss.run(wn) * 0.35;
        let wind = w * (speed / 40.0).powi(2).min(1.5) * 0.22 * gust;
        // One-shots.
        let mut hl = 0.0;
        let mut hr = 0.0;
        for h in &mut self.hits {
            if h.noise.active() || h.thud.active() {
                let nz = self.n.white() * h.noise.next();
                let m: f32 = h.metal.iter_mut().map(|m| m.next()).sum();
                let s = h.thud.next() + nz * 0.6 + m;
                let (a, b) = pan(h.pan);
                hl += s * a;
                hr += s * b;
            }
        }
        for t in &mut self.thumps {
            if t.0.active() || t.1.active() {
                let s = t.0.next() + self.n.white() * t.1.next();
                let (a, b) = pan(t.2);
                hl += s * a;
                hr += s * b;
            }
        }
        let clunk = self.clunk.next() + self.clunk_tick.next();
        // Outside sounds through the cabin filter.
        let outside_l = self.cabin.lp(tl + wind * (1.0 - 0.5 * interior));
        let outside_r = outside_l + (tr - tl) * (1.0 - interior);
        // The engine is louder/boomier from the bonnet camera, pops louder outside.
        let eng = eng * (1.0 + 0.3 * interior) + pops * (1.0 - 0.4 * interior);
        let mono = eng + turbo + whine + clunk;
        let l = (mono + outside_l + hl) * act;
        let r = (mono + outside_r + hr) * act;
        (l, r)
    }

    #[inline]
    fn engine_sample(&mut self, p: CarParams) -> (f32, f32) {
        let e = &mut self.engine;
        let n = &mut e.n;
        let redline = p.redline;
        let at_limit = p.rpm >= redline - 80.0 && p.throttle > 0.5;
        // Limiter bounce: cut ignition about half of each ~70 ms cycle.
        let mut cut = p.shifting;
        if at_limit {
            e.limiter += 14.0 / RATE;
            if e.limiter >= 1.0 {
                e.limiter -= 1.0;
            }
            cut |= e.limiter < 0.45;
        } else {
            e.limiter = 0.5;
        }
        let load = e.load.next(if cut { 0.0 } else { p.throttle });
        let level = e.level.next(if p.active { 1.0 } else { 0.0 });
        let idle_factor = 1.0 - smoothstep(p.idle_rpm, p.idle_rpm + 1500.0, p.rpm);
        let wob = e.wobble.next(n);
        let dip = if at_limit && cut { 0.97 } else { 1.0 };
        let rpm = e.rpm.next(p.rpm * dip) * (1.0 + 0.008 * wob * idle_factor);
        let rpm_n = ((rpm - p.idle_rpm) / (redline - p.idle_rpm)).clamp(0.0, 1.0);
        let overrun = (1.0 - load) * smoothstep(2200.0, 4500.0, rpm);
        e.crackle *= 1.0 - 1.0 / (1.4 * RATE);
        // Fire at quarter-cycle boundaries (one cycle = two revolutions).
        let prev = (e.phase * 4.0) as u32;
        e.phase += rpm / 120.0 / RATE;
        if e.phase >= 1.0 {
            e.phase -= 1.0;
        }
        let cyl = (e.phase * 4.0) as u32;
        let mut impulse = 0.0;
        let mut pops = 0.0;
        if cyl != prev {
            let jitter = 1.0 + (0.18 * idle_factor + 0.05) * n.white();
            // Cylinder-to-cylinder differences: a lumpy burble at idle, evened out higher up.
            let cg = 1.0 + (CYL_GAIN[cyl as usize & 3] - 1.0) * (0.25 + 0.75 * idle_factor);
            let base = (0.3 + 0.7 * load) * (0.55 + 0.45 * rpm_n) * cg * jitter;
            let fired = if cut { 0.08 } else { 1.0 - 0.55 * overrun };
            impulse = base * fired;
            e.burst.trigger(base * fired * (0.15 + 0.5 * load), 0.006 + 0.01 * (1.0 - rpm_n));
            // Backfire: unburnt fuel lighting in the exhaust on overrun and on the limiter.
            let chance = if at_limit && cut { 0.22 } else { 0.07 * overrun * e.crackle };
            if p.active && n.chance(chance) {
                let pop = &mut e.pops[e.next_pop];
                e.next_pop = (e.next_pop + 1) % 3;
                let a = n.range(0.5, 1.0);
                pop.noise.trigger(0.6 * a, n.range(0.02, 0.05));
                pop.boom.trigger(n.range(65.0, 95.0), 0.12, 0.5 * a);
                pop.crack.set_bandpass(n.range(1800.0, 3200.0), 0.9);
            }
        }
        // Pulse shaping: impulse → two one-poles (normalised to ~unit peak per firing).
        let k = e.shape_a.coeff().max(1e-4);
        let x = e.shape_a.lp(impulse * std::f32::consts::E / k);
        let pulse = e.shape_b.lp(x);
        let comb = n.white() * e.burst.next();
        let src = pulse + comb;
        let body = e.body.run(src) * 1.1;
        let mid = e.mid.run(src) * 0.6;
        let rasp = e.rasp.run(src) * (0.15 + 0.4 * load);
        let intake = e.intake.run(n.white() * (0.2 + pulse.abs())) * 0.12 * load * (0.3 + rpm_n);
        let raw = e.tone.run(pulse * 0.35 + body + mid + rasp) + intake;
        let eng = e.dc.hp(raw) * 1.1 * level;
        for pop in &mut e.pops {
            if pop.noise.active() || pop.boom.active() {
                let nz = n.white() * pop.noise.next();
                pops += pop.crack.run(nz) * 1.4 + nz * 0.3 + pop.boom.next();
            }
        }
        (eng, pops * 0.5 * level)
    }

    #[inline]
    fn turbo_sample(&mut self, p: CarParams) -> f32 {
        let t = &mut self.turbo;
        let e = &self.engine;
        // Boost builds with load above ~2500 rpm (lag), bleeds off on lift.
        let target = e.load.v * smoothstep(2400.0, 5200.0, e.rpm.v);
        let rate = if target > t.boost { 1.3 } else { 4.0 };
        t.boost += (target - t.boost) * rate / RATE;
        let shaft = t.shaft.next(t.boost.max(smoothstep(1500.0, 7000.0, e.rpm.v) * 0.25));
        let wf = 2400.0 + 5200.0 * shaft;
        let n = &mut t.n;
        let whistle = t.whistle.next(wf) * shaft * shaft * 0.012 + t.hiss.run(n.white()) * shaft * 0.05;
        let mut bov = 0.0;
        if t.bov.active() {
            t.bov_f = (t.bov_f * (1.0 - 1.6 / RATE)).max(1200.0);
            bov = t.bov_bp.run(n.white()) * t.bov.next();
        }
        if t.flutter.active() {
            // Compressor surge "stu-tu-tu".
            let am = 0.5 + 0.5 * t.flutter_osc.next(22.0);
            bov += t.bov_bp.run(n.white()) * t.flutter.next() * am * am;
        }
        let _ = p;
        whistle + bov
    }

    #[inline]
    fn tyre_sample(&mut self, speed: f32) -> (f32, f32) {
        let p = self.p;
        let t = &mut self.tyres;
        let n = &mut t.n;
        let mut roll = [0.0f32; SURFACES];
        let mut slide = [0.0f32; SURFACES];
        for i in 0..SURFACES {
            roll[i] = t.roll[i].next(p.surfaces[i].roll);
            slide[i] = t.slide[i].next(p.surfaces[i].slide);
        }
        let wn = n.white();
        let mut mono = 0.0;
        // Gravel: crunch grains + low rumble, a broadband hiss when sliding, stones.
        let (gr, gs) = (roll[GRAVEL], slide[GRAVEL]);
        // Snow: denser, higher, squeakier grains.
        let (sr, ss) = (roll[SNOW], slide[SNOW]);
        if gr + gs + sr + ss > 1e-4 {
            let rate = (gr * 70.0 + gs * 160.0) / RATE;
            if n.chance(rate) {
                t.grain.trigger(n.range(0.3, 1.0), n.range(0.003, 0.009));
            }
            let srate = (sr * 110.0 + ss * 200.0) / RATE;
            if n.chance(srate) {
                t.snow_grain.trigger(n.range(0.3, 1.0), n.range(0.002, 0.005));
            }
            let g = t.gravel_bp.run(wn) * (gr * 0.05 + gs * 0.045)
                + t.grain_f.run(wn * t.grain.next()) * (0.16 + 0.1 * gs) * (gr + gs).min(1.5)
                + t.gravel_lo.run(wn) * gr * 0.25
                + t.slide_bp.run(wn) * gs * 0.045;
            let s = t.snow_bp.run(wn * (0.12 + t.snow_grain.next())) * (sr * 0.25 + ss * 0.35);
            mono += g + s;
        }
        // Tarmac: low road roar; squeal when slip goes past the peak.
        let (tr, ts) = (roll[TARMAC], slide[TARMAC]);
        if tr + ts > 1e-4 {
            mono += t.tarmac_lo.run(wn) * tr * 0.12;
            let sq = smoothstep(0.25, 0.9, ts);
            if sq > 1e-4 {
                let v = t.squeal_wander.next(n);
                let f = (820.0 + 260.0 * ts.min(2.0)) * (1.0 + 0.04 * v);
                let tone = t.squeal.next(f) * 0.6 + t.squeal_bp.run(wn) * 2.0;
                mono += tone * sq * 0.07 * (0.7 + 0.3 * v.abs());
            }
        }
        // Mud: low squelch with a wet resonance that wanders.
        let (mr, ms) = (roll[MUD], slide[MUD]);
        if mr + ms > 1e-4 {
            let v = t.mud_wander.next(n);
            let am = (0.6 + 0.4 * v).max(0.0);
            mono += t.mud_lo.run(wn) * (mr * 0.3 + ms * 0.25) * am + t.mud_slurp.run(wn) * (ms * 0.12 + mr * 0.04) * am * am;
        }
        // Off the road: grass swish and bumps.
        let (or, os) = (roll[OFFROAD], slide[OFFROAD]);
        if or + os > 1e-4 {
            let b = t.rumble_wander.next(n).abs();
            mono += t.grass_hp.run(wn) * (or * 0.03 + os * 0.04) + t.rumble.run(wn) * or * (0.18 + 0.45 * b);
        }
        // Stones flicked into the arches (gravel mostly, some off-road).
        let stone_rate = ((gr + or * 0.4) * 2.5 + (gs + os * 0.3) * 6.0) * smoothstep(3.0, 15.0, speed) / RATE;
        if n.chance(stone_rate) {
            let (ring, pan_x) = &mut t.pings[t.next_ping];
            t.next_ping = (t.next_ping + 1) % 8;
            ring.trigger(n.range(1800.0, 6500.0), n.range(0.02, 0.07), n.range(0.015, 0.06));
            *pan_x = n.range(-0.8, 0.8);
        }
        let mut l = mono;
        let mut r = mono;
        for (ring, px) in &mut t.pings {
            if ring.active() {
                let s = ring.next();
                let (a, b) = pan(*px);
                l += s * a;
                r += s * b;
            }
        }
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

    fn driving(rpm: f32, throttle: f32) -> CarParams {
        CarParams { active: true, rpm, throttle, gear: 3, speed: 20.0, ..default() }
    }

    fn default() -> CarParams {
        CarParams::default()
    }

    fn check(out: &[f32], max: f32) {
        assert!(out.iter().all(|v| v.is_finite()), "NaN/inf");
        let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < max, "peak {peak}");
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
                s.set(p);
                let out = run(&mut s, 0.25);
                check(&out, 1.6);
            }
        }
        for _ in 0..3 {
            s.event(CarEvent::Impact(30.0));
            s.event(CarEvent::Thump(1.0, -1.0));
            s.event(CarEvent::Shift);
        }
        let ev = run(&mut s, 0.5);
        check(&ev, 3.5); // nine stacked worst-case one-shots; the stream soft-clips
        // Garbage in, silence-or-sound out but never NaN.
        s.set(CarParams { active: true, rpm: f32::NAN, throttle: f32::INFINITY, speed: -5.0, interior: 7.0, ..default() });
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
            pops += s.engine.pops.iter().filter(|p| p.noise.active()).count().min(1);
        }
        assert!(pops > 0, "some backfire pops in the first 0.8 s of overrun");
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

    #[test]
    fn tyres_by_surface_and_slip() {
        let level = |surface: usize, roll: f32, slide: f32| {
            let mut s = CarSynth::new(8);
            let mut p = CarParams { active: true, rpm: 0.0, speed: 20.0, ..default() };
            p.surfaces[surface] = Contact { roll, slide };
            s.set(p);
            // Subtract the engine/wind-only reference.
            let out = mono(&run(&mut s, 0.6));
            let mut r = CarSynth::new(8);
            r.set(CarParams { active: true, rpm: 0.0, speed: 20.0, ..default() });
            let base = mono(&run(&mut r, 0.6));
            let d: Vec<f32> = out.iter().zip(&base).map(|(a, b)| a - b).collect();
            rms(&d[9600..])
        };
        for surf in [GRAVEL, TARMAC, MUD, SNOW, OFFROAD] {
            let roll = level(surf, 1.0, 0.0);
            let slide = level(surf, 1.0, 1.5);
            assert!(roll > 1e-3 || surf == TARMAC && roll > 3e-4, "surface {surf} rolls: {roll}");
            assert!(slide > roll * 1.2, "surface {surf}: slide {slide} vs roll {roll}");
            assert!(slide < 0.5);
        }
        // Squeal is tonal on tarmac.
        let mut s = CarSynth::new(1);
        let mut p = CarParams { active: true, rpm: 0.0, speed: 20.0, ..default() };
        p.surfaces[TARMAC] = Contact { roll: 1.0, slide: 1.5 };
        s.set(p);
        let out = mono(&run(&mut s, 0.6));
        let f = 820.0 + 260.0 * 1.5;
        let near: f32 = [-30.0, -15.0, 0.0, 15.0, 30.0].iter().map(|d| goertzel(&out[9600..], f + d)).sum();
        let far: f32 = [-30.0, -15.0, 0.0, 15.0, 30.0].iter().map(|d| goertzel(&out[9600..], 2.6 * f + d)).sum();
        assert!(near > 4.0 * far, "near {near} far {far}");
    }

    /// `cargo test --lib report_levels -- --ignored --nocapture`: peak/RMS per scenario.
    #[test]
    #[ignore]
    fn report_levels() {
        let parts = |s: &mut CarSynth| {
            let mut o = [0.0f32; 4];
            for _ in 0..4800 {
                let p = s.p;
                let (e, pops) = s.engine_sample(p);
                let t = s.turbo_sample(p);
                let (tl, _) = s.tyre_sample(p.speed);
                for (i, v) in [e, pops, t, tl].into_iter().enumerate() {
                    o[i] = o[i].max(v.abs());
                }
            }
            o
        };
        for rpm in [950.0, 2500.0, 5000.0, 7600.0, 8200.0] {
            for th in [0.0, 1.0] {
                let mut s = CarSynth::new(9);
                let mut p = driving(rpm, th);
                p.speed = 35.0;
                p.surfaces[GRAVEL] = Contact { roll: 1.4, slide: 2.0 };
                s.set(p);
                run(&mut s, 0.5);
                let out = mono(&run(&mut s, 0.5));
                let pk = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                println!("rpm {rpm:5} th {th}: rms {:.3} peak {pk:.3} parts(eng,pops,turbo,tyre) {:?}", rms(&out), parts(&mut s));
            }
        }
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
        for (name, ev) in [("impact", CarEvent::Impact(30.0)), ("impact5", CarEvent::Impact(5.0)), ("thump", CarEvent::Thump(1.0, 0.0)), ("shift", CarEvent::Shift)] {
            let mut s = CarSynth::new(9);
            s.set(CarParams { active: true, rpm: 0.0, ..default() });
            run(&mut s, 0.2);
            s.event(ev);
            let out = run(&mut s, 0.5);
            println!("{name}: peak {:.3}", out.iter().fold(0.0f32, |m, v| m.max(v.abs())));
        }
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
}
