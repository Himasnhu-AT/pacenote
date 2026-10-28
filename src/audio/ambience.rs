//! Stage ambience, synthesised: a wind bed and leaf rustle, birds (forests), cicadas (dry
//! scrub), a curlew-like call on the moor, crickets at dusk, rain hiss and drops (and on the
//! roof from inside the car). Quiet by design: it sits under the car on its own bus.

use super::dsp::{Biquad, Decay, Noise, OnePole, Osc, RATE, Smooth, Wander, pan, sin01, smoothstep};
use crate::stage::{Biome, TimeOfDay, Weather};

/// What the ambience needs (per stage, plus the camera).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmbParams {
    /// In a stage (false: fades out).
    pub active: bool,
    pub biome: Biome,
    pub weather: Weather,
    pub time: TimeOfDay,
    /// 0 = outside .. 1 = inside the car.
    pub interior: f32,
}

impl Default for AmbParams {
    fn default() -> Self {
        AmbParams { active: false, biome: Biome::PineForest, weather: Weather::Clear, time: TimeOfDay::Noon, interior: 0.0 }
    }
}

/// Levels and event rates derived from the stage.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Recipe {
    pub wind: f32,
    pub gustiness: f32,
    pub leaves: f32,
    /// Bird calls per second.
    pub birds: f32,
    pub cicadas: f32,
    /// Moor calls per second.
    pub curlew: f32,
    pub crickets: f32,
    pub rain: f32,
}

impl Recipe {
    pub fn of(p: &AmbParams) -> Recipe {
        let mut r = match p.biome {
            Biome::PineForest => Recipe { wind: 0.025, gustiness: 0.4, leaves: 0.012, birds: 0.45, ..Default::default() },
            Biome::BirchHills => Recipe { wind: 0.03, gustiness: 0.5, leaves: 0.02, birds: 0.6, crickets: 0.004, ..Default::default() },
            Biome::Scrub => Recipe { wind: 0.02, gustiness: 0.3, leaves: 0.006, birds: 0.12, cicadas: 0.012, crickets: 0.005, ..Default::default() },
            Biome::Moor => Recipe { wind: 0.07, gustiness: 0.9, leaves: 0.0, birds: 0.05, curlew: 0.06, ..Default::default() },
            Biome::SnowForest => Recipe { wind: 0.03, gustiness: 0.6, leaves: 0.006, birds: 0.08, ..Default::default() },
        };
        let (bird_t, insect_t) = match p.time {
            TimeOfDay::Morning => (1.5, 0.3),
            TimeOfDay::Noon => (0.8, 1.0),
            TimeOfDay::Afternoon => (0.9, 1.0),
            TimeOfDay::Dusk => (0.4, 0.2),
        };
        r.birds *= bird_t;
        r.cicadas *= insect_t;
        r.crickets *= if p.time == TimeOfDay::Dusk { 3.0 } else { 0.3 };
        match p.weather {
            Weather::Clear => {}
            Weather::Overcast => {
                r.birds *= 0.7;
                r.cicadas *= 0.4;
                r.wind *= 1.2;
            }
            Weather::Fog => {
                r.birds *= 0.35;
                r.cicadas *= 0.2;
                r.wind *= 0.6;
                r.gustiness *= 0.5;
            }
            Weather::Rain => {
                r.birds *= 0.1;
                r.cicadas = 0.0;
                r.crickets = 0.0;
                r.curlew *= 0.3;
                r.wind *= 1.3;
                r.rain = 1.0;
            }
        }
        r
    }
}

/// One bird call: a few chirps with a pitch sweep.
#[derive(Clone, Copy, Debug, Default)]
struct Call {
    osc: Osc,
    /// Seconds into the current chirp, its length, gap after it, chirps left.
    t: f32,
    len: f32,
    gap: f32,
    left: u8,
    f0: f32,
    f1: f32,
    /// Trill rate (0 = plain).
    trill: f32,
    amp: f32,
    pan: f32,
}

#[derive(Clone, Debug)]
pub struct AmbienceSynth {
    p: AmbParams,
    r: Recipe,
    n: Noise,
    level: Smooth,
    interior: Smooth,
    wind_l: Biquad,
    wind_r: Biquad,
    gust: Wander,
    gust2: Wander,
    leaves: Biquad,
    calls: [Call; 4],
    next_call: usize,
    cicada: Osc,
    cicada_bp: Biquad,
    cicada_swell: Wander,
    cricket: Osc,
    cricket_t: f32,
    rain_hp: Biquad,
    rain_lp: OnePole,
    drops: [(Decay, Biquad, f32); 6],
    next_drop: usize,
    roof: Biquad,
    cabin: OnePole,
    coef_timer: u32,
}

impl AmbienceSynth {
    pub fn new(seed: u32) -> Self {
        AmbienceSynth {
            p: AmbParams::default(),
            r: Recipe::default(),
            n: Noise::new(seed ^ 0xA5A5),
            level: Smooth::new(0.0, 0.8, 0.5, RATE),
            interior: Smooth::new(0.0, 0.05, 0.05, RATE),
            wind_l: Biquad::lowpass(500.0, 0.6),
            wind_r: Biquad::lowpass(500.0, 0.6),
            gust: Wander::new(0.15),
            gust2: Wander::new(0.6),
            leaves: Biquad::bandpass(3200.0, 0.5),
            calls: [Call::default(); 4],
            next_call: 0,
            cicada: Osc::default(),
            cicada_bp: Biquad::bandpass(5600.0, 4.0),
            cicada_swell: Wander::new(0.1),
            cricket: Osc::default(),
            cricket_t: 0.0,
            rain_hp: Biquad::highpass(1400.0, 0.6),
            rain_lp: OnePole::new(7000.0),
            drops: [(Decay::default(), Biquad::default(), 0.0); 6],
            next_drop: 0,
            roof: Biquad::bandpass(900.0, 1.5),
            cabin: OnePole::new(20_000.0),
            coef_timer: 0,
        }
    }

    pub fn set(&mut self, p: AmbParams) {
        let mut p = p;
        p.interior = if p.interior.is_finite() { p.interior.clamp(0.0, 1.0) } else { 0.0 };
        if p != self.p {
            self.r = Recipe::of(&p);
        }
        self.p = p;
    }

    pub fn recipe(&self) -> Recipe {
        self.r
    }

    pub fn render(&mut self, out: &mut [f32]) {
        for fr in out.chunks_exact_mut(2) {
            let (l, r) = self.sample();
            fr[0] += l;
            fr[1] += r;
        }
    }

    fn start_call(&mut self) {
        let n = &mut self.n;
        let c = &mut self.calls[self.next_call];
        self.next_call = (self.next_call + 1) % 4;
        // Three kinds of (invented) songbird: rising whistle, falling trill, two-note chip.
        let kind = n.next_u32() % 3;
        let base = n.range(2600.0, 4800.0);
        let (f0, f1, len, trill) = match kind {
            0 => (base * 0.8, base * 1.25, n.range(0.06, 0.14), 0.0),
            1 => (base * 1.2, base * 0.85, n.range(0.04, 0.08), n.range(40.0, 70.0)),
            _ => (base, base * 0.92, n.range(0.025, 0.05), 0.0),
        };
        *c = Call {
            osc: Osc::default(),
            t: 0.0,
            len,
            gap: n.range(0.04, 0.15),
            left: 2 + (n.next_u32() % 5) as u8,
            f0,
            f1,
            trill,
            amp: n.range(0.004, 0.014),
            pan: n.range(-0.9, 0.9),
        };
    }

    fn start_curlew(&mut self) {
        let n = &mut self.n;
        let c = &mut self.calls[self.next_call];
        self.next_call = (self.next_call + 1) % 4;
        *c = Call {
            osc: Osc::default(),
            t: 0.0,
            len: n.range(0.45, 0.7),
            gap: 0.12,
            left: 2 + (n.next_u32() % 3) as u8,
            f0: n.range(1700.0, 1900.0),
            f1: n.range(2400.0, 2700.0),
            trill: 0.0,
            amp: n.range(0.004, 0.009),
            pan: n.range(-1.0, 1.0),
        };
    }

    #[inline]
    fn sample(&mut self) -> (f32, f32) {
        let level = self.level.next(if self.p.active { 1.0 } else { 0.0 });
        if level < 1e-5 && !self.p.active {
            return (0.0, 0.0);
        }
        let interior = self.interior.next(self.p.interior);
        if self.coef_timer == 0 {
            self.coef_timer = 128;
            self.cabin.set(20_000.0 - 18_000.0 * interior);
        }
        self.coef_timer -= 1;
        let r = self.r;
        let dt = 1.0 / RATE;
        // Wind bed: decorrelated L/R, slow gusts.
        let g = (1.0 + r.gustiness * (0.6 * self.gust.next(&mut self.n) + 0.4 * self.gust2.next(&mut self.n))).max(0.1);
        let (nl, nr) = (self.n.white(), self.n.white());
        let mut l = self.wind_l.run(nl) * r.wind * g;
        let mut rr = self.wind_r.run(nr) * r.wind * g;
        if r.leaves > 0.0 {
            let lv = self.leaves.run(self.n.white()) * r.leaves * g * g;
            l += lv;
            rr += lv * 0.8;
        }
        // Birds and moor calls (outside only; muffled by the cabin filter below).
        if self.n.chance(r.birds * dt) {
            self.start_call();
        }
        if self.n.chance(r.curlew * dt) {
            self.start_curlew();
        }
        for c in &mut self.calls {
            if c.left == 0 {
                continue;
            }
            if c.t < c.len {
                let x = c.t / c.len;
                let f = c.f0 + (c.f1 - c.f0) * x;
                let tr = if c.trill > 0.0 { 0.5 + 0.5 * sin01(c.t * c.trill) } else { 1.0 };
                let env = sin01(x * 0.5) * tr;
                let s = c.osc.next(f) * env * c.amp;
                let (a, b) = pan(c.pan);
                l += s * a;
                rr += s * b;
            }
            c.t += dt;
            if c.t >= c.len + c.gap {
                c.t = 0.0;
                c.left -= 1;
            }
        }
        // Cicadas: a buzzing tone pulsed fast, swelling slowly.
        if r.cicadas > 0.0 {
            let pulse = self.cicada.next(62.0).max(0.0);
            let tone = self.cicada_bp.run(self.n.white()) * 3.0;
            let swell = 0.6 + 0.4 * self.cicada_swell.next(&mut self.n);
            let s = tone * pulse * pulse * swell * r.cicadas;
            l += s;
            rr += s * 0.9;
        }
        // Crickets: chirp triplets.
        if r.crickets > 0.0 {
            self.cricket_t += dt;
            if self.cricket_t > 0.9 {
                self.cricket_t -= 0.9;
            }
            let t = self.cricket_t;
            let on = t < 0.15 && (t * 20.0).fract() < 0.6;
            if on {
                let s = self.cricket.next(4300.0) * r.crickets;
                l += s * 0.7;
                rr += s;
            }
        }
        // Rain: hiss, drops, and on the roof from inside.
        if r.rain > 0.0 {
            let hiss = self.rain_lp.lp(self.rain_hp.run(self.n.white())) * 0.03 * r.rain;
            l += hiss;
            rr += hiss;
            if self.n.chance(140.0 * dt) {
                let n = &mut self.n;
                let d = &mut self.drops[self.next_drop];
                self.next_drop = (self.next_drop + 1) % 6;
                d.0.trigger(n.range(0.005, 0.03), n.range(0.004, 0.012));
                d.1.set_bandpass(n.range(2000.0, 6000.0), 2.0);
                d.2 = n.range(-1.0, 1.0);
            }
            for d in &mut self.drops {
                if d.0.active() {
                    let s = d.1.run(self.n.white()) * d.0.next();
                    let (a, b) = pan(d.2);
                    l += s * a;
                    rr += s * b;
                }
            }
        }
        // Inside: the outside is muffled, rain drums on the roof.
        let lo = self.cabin.lp((l + rr) * 0.5);
        let mut out_l = l + (lo - l) * interior;
        let mut out_r = rr + (lo - rr) * interior;
        if r.rain > 0.0 && interior > 0.0 {
            let drum = self.roof.run(self.n.white() * smoothstep(0.7, 1.0, self.n.unit())) * 0.05 * r.rain * interior;
            out_l += drum;
            out_r += drum;
        }
        (out_l * level, out_r * level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &mut AmbienceSynth, secs: f32) -> Vec<f32> {
        let mut out = vec![0.0; (secs * RATE) as usize * 2];
        s.render(&mut out);
        out
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    #[test]
    fn every_biome_and_weather_is_quiet_and_finite() {
        let biomes = [Biome::PineForest, Biome::BirchHills, Biome::Scrub, Biome::Moor, Biome::SnowForest];
        let weathers = [Weather::Clear, Weather::Overcast, Weather::Fog, Weather::Rain];
        for b in biomes {
            for w in weathers {
                for interior in [0.0, 1.0] {
                    let mut s = AmbienceSynth::new(1);
                    s.set(AmbParams { active: true, biome: b, weather: w, time: TimeOfDay::Morning, interior });
                    let out = run(&mut s, 2.0);
                    assert!(out.iter().all(|v| v.is_finite()));
                    let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                    let level = rms(&out[48_000..]);
                    assert!(peak < 0.5, "{b:?} {w:?}: peak {peak}");
                    assert!(level > 4e-4 && level < 0.12, "{b:?} {w:?} {interior}: rms {level}");
                }
            }
        }
    }

    #[test]
    fn recipes_differ_by_biome_and_weather() {
        let p = |biome, weather| Recipe::of(&AmbParams { active: true, biome, weather, time: TimeOfDay::Noon, interior: 0.0 });
        assert!(p(Biome::Moor, Weather::Clear).wind > p(Biome::PineForest, Weather::Clear).wind);
        assert!(p(Biome::BirchHills, Weather::Clear).birds > p(Biome::Moor, Weather::Clear).birds);
        assert!(p(Biome::Scrub, Weather::Clear).cicadas > 0.0);
        assert_eq!(p(Biome::Scrub, Weather::Rain).cicadas, 0.0);
        assert!(p(Biome::PineForest, Weather::Rain).rain > 0.0);
        assert!(p(Biome::Moor, Weather::Clear).curlew > 0.0);
    }

    #[test]
    fn rain_is_louder_than_clear_and_fades_out() {
        let level = |w| {
            let mut s = AmbienceSynth::new(2);
            s.set(AmbParams { active: true, biome: Biome::PineForest, weather: w, time: TimeOfDay::Noon, interior: 0.0 });
            rms(&run(&mut s, 2.0)[48_000..])
        };
        assert!(level(Weather::Rain) > level(Weather::Clear) * 1.3);
        let mut s = AmbienceSynth::new(2);
        s.set(AmbParams { active: true, ..Default::default() });
        run(&mut s, 2.0);
        s.set(AmbParams { active: false, ..Default::default() });
        let out = run(&mut s, 4.0);
        assert!(rms(&out[out.len() - 4800..]) < 1e-4);
    }
}
