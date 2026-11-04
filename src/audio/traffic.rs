//! The other cars on the road (open world traffic), heard from the listener: the nearest
//! [`VOICES`] cars each get an engine (a recorded loop of the player's engine, pitched by a
//! road-car gearbox from the car's speed) and tyre roar (band-passed noise growing with speed),
//! attenuated with distance, darkened by air absorption, Doppler-shifted by the closing speed
//! and panned by bearing. Inside the car they're muffled like the outside wind.
//!
//! Voices are stable per car (`id`): a new car takes a voice only once that voice has faded
//! out, so nothing clicks when cars come and go.

use super::dsp::{Biquad, Noise, OnePole, RATE, Smooth, finite, pan};
use super::samples::{self, Looper};

/// Cars heard at once.
pub const VOICES: usize = 4;
/// Beyond this (m) a car isn't heard.
pub const RANGE: f32 = 140.0;
/// Engine loop the traffic plays (index into `powertrain::LOOPS`: on-load 1925 rpm) and its rpm.
const LOOP: usize = 3;
const LOOP_RPM: f32 = 1925.0;
/// A road car's gearbox: road speed (m/s) per 1000 rpm in each gear.
const GEARS: [f32; 5] = [2.3, 4.0, 5.9, 7.8, 9.8];
/// Speed of sound (m/s), for the Doppler shift.
const SOUND: f32 = 343.0;

/// One car as the listener hears it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OtherCar {
    pub active: bool,
    /// Stable for the car's life (voice assignment).
    pub id: u32,
    /// Distance (m).
    pub dist: f32,
    /// −1 (left) .. 1 (right) of the listener.
    pub pan: f32,
    /// Road speed (m/s).
    pub speed: f32,
    /// Closing speed (m/s, + = coming nearer).
    pub closing: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrafficParams {
    pub cars: [OtherCar; VOICES],
    /// 0 = outside .. 1 = inside the car.
    pub interior: f32,
}

/// The engine speed of a road car doing `v` m/s: the highest gear that keeps it over 1700 rpm.
pub fn rpm_at(v: f32) -> f32 {
    let v = v.abs();
    let ratio = GEARS.iter().rev().copied().find(|r| v / r * 1000.0 >= 1700.0).unwrap_or(GEARS[0]);
    (v / ratio * 1000.0).max(850.0)
}

/// Gain of a car at `d` metres (1 at 3 m, inverse distance, faded to 0 at [`RANGE`]).
pub fn distance_gain(d: f32) -> f32 {
    let fade = ((RANGE - d) / (RANGE * 0.25)).clamp(0.0, 1.0);
    (3.0 / d.max(3.0)) * fade
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    id: Option<u32>,
    target: OtherCar,
    looper: Looper,
    gain: Smooth,
    rate: Smooth,
    pan: Smooth,
    roar_l: Biquad,
    roar_r: Biquad,
    air: [OnePole; 2],
}

impl Voice {
    fn new(k: usize) -> Self {
        Voice {
            id: None,
            target: OtherCar::default(),
            looper: Looper::at(0.23 * k as f64),
            gain: Smooth::new(0.0, 0.08, 0.12, RATE),
            rate: Smooth::new(0.5, 0.15, 0.15, RATE),
            pan: Smooth::new(0.0, 0.05, 0.05, RATE),
            roar_l: Biquad::bandpass(700.0, 0.6),
            roar_r: Biquad::bandpass(760.0, 0.6),
            air: [OnePole::new(8000.0); 2],
        }
    }
}

#[derive(Clone, Debug)]
pub struct TrafficSynth {
    p: TrafficParams,
    v: [Voice; VOICES],
    n: Noise,
    interior: Smooth,
    cabin: [OnePole; 2],
    coef_timer: u32,
}

impl TrafficSynth {
    pub fn new(seed: u32) -> Self {
        TrafficSynth {
            p: TrafficParams::default(),
            v: std::array::from_fn(Voice::new),
            n: Noise::new(seed ^ 0x7AF1),
            interior: Smooth::new(0.0, 0.05, 0.05, RATE),
            cabin: [OnePole::new(1500.0); 2],
            coef_timer: 0,
        }
    }

    /// Takes the cars heard now: each keeps its voice; a new one takes a silent voice.
    pub fn set(&mut self, p: TrafficParams) {
        self.p = p;
        let heard = |id: u32| p.cars.iter().find(|c| c.active && c.id == id).copied();
        for v in &mut self.v {
            match v.id.and_then(heard) {
                Some(c) => v.target = c,
                None => v.target.active = false,
            }
        }
        for c in p.cars.iter().filter(|c| c.active) {
            if self.v.iter().any(|v| v.id == Some(c.id)) {
                continue;
            }
            if let Some(v) = self.v.iter_mut().find(|v| !v.target.active && v.gain.v < 1e-4) {
                v.id = Some(c.id);
                v.target = *c;
            }
        }
    }

    /// Whether anything is (or is still fading) audible.
    pub fn active(&self) -> bool {
        self.v.iter().any(|v| v.target.active || v.gain.v > 1e-5)
    }

    pub fn render(&mut self, out: &mut [f32]) {
        if !self.active() {
            return;
        }
        let bank = samples::bank();
        let exhaust = bank.map_or(&[][..], |b| b.engine(LOOP).1);
        for fr in out.chunks_exact_mut(2) {
            if self.coef_timer == 0 {
                self.coef_timer = 64;
                for v in &mut self.v {
                    let c = v.target;
                    let doppler = (SOUND / (SOUND - c.closing.clamp(-60.0, 60.0))).clamp(0.85, 1.18);
                    let hz = (700.0 + 25.0 * c.speed) * doppler;
                    v.roar_l.set_bandpass(hz, 0.6);
                    v.roar_r.set_bandpass(hz * 1.08, 0.6);
                    let air = 11_000.0 / (1.0 + c.dist / 28.0);
                    for a in &mut v.air {
                        a.set(air);
                    }
                }
            }
            self.coef_timer -= 1;
            let inside = self.interior.next(self.p.interior);
            let (mut l, mut r) = (0.0, 0.0);
            for v in &mut self.v {
                let c = v.target;
                let g = v.gain.next(if c.active { distance_gain(c.dist) } else { 0.0 });
                if g < 1e-6 {
                    continue;
                }
                let doppler = (SOUND / (SOUND - c.closing.clamp(-60.0, 60.0))).clamp(0.85, 1.18);
                let rate = v.rate.next(rpm_at(c.speed) / LOOP_RPM * doppler);
                let engine = v.looper.next(exhaust, rate) * (0.03 + 0.02 * (c.speed / 25.0).min(1.0));
                let s = (c.speed / 22.0).min(1.6);
                let roar = 0.05 * s * s.sqrt();
                let nl = v.roar_l.run(self.n.white()) * roar;
                let nr = v.roar_r.run(self.n.white()) * roar;
                let (pl, pr) = pan(v.pan.next(c.pan * 0.85));
                l += v.air[0].lp(engine + nl) * g * pl;
                r += v.air[1].lp(engine + nr) * g * pr;
            }
            // Inside: through the body (low-passed, quieter).
            let li = self.cabin[0].lp(l);
            let ri = self.cabin[1].lp(r);
            let k = 1.0 - 0.45 * inside;
            fr[0] += finite((l + (li - l) * inside) * k);
            fr[1] += finite((r + (ri - r) * inside) * k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car(id: u32, dist: f32, speed: f32, closing: f32) -> OtherCar {
        OtherCar { active: true, id, dist, pan: 0.3, speed, closing }
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn render(s: &mut TrafficSynth, secs: f32) -> Vec<f32> {
        let mut out = vec![0.0; (RATE * secs) as usize * 2];
        s.render(&mut out);
        out
    }

    #[test]
    fn gearbox_keeps_a_road_car_in_its_band() {
        assert!(rpm_at(0.0) >= 850.0);
        for v in [5.0, 10.0, 15.0, 20.0, 25.0, 30.0] {
            let r = rpm_at(v);
            assert!((1700.0..4400.0).contains(&r), "{v} m/s: {r} rpm");
        }
    }

    #[test]
    fn nearer_and_faster_is_louder_and_far_is_silent() {
        let level = |c: OtherCar| {
            let mut s = TrafficSynth::new(1);
            s.set(TrafficParams { cars: [c, OtherCar::default(), OtherCar::default(), OtherCar::default()], interior: 0.0 });
            let out = render(&mut s, 1.0);
            rms(&out[out.len() / 2..])
        };
        let near = level(car(1, 5.0, 20.0, 0.0));
        let far = level(car(1, 60.0, 20.0, 0.0));
        let slow = level(car(1, 5.0, 5.0, 0.0));
        assert!(near > far * 3.0 && near > slow, "{near} {far} {slow}");
        assert!(level(car(1, RANGE + 5.0, 20.0, 0.0)) < 1e-5);
        assert!(near < 0.2, "{near}");
    }

    #[test]
    fn inside_is_quieter() {
        let level = |interior: f32| {
            let mut s = TrafficSynth::new(1);
            s.set(TrafficParams { cars: [car(1, 8.0, 20.0, 0.0), OtherCar::default(), OtherCar::default(), OtherCar::default()], interior });
            let out = render(&mut s, 1.0);
            rms(&out[out.len() / 2..])
        };
        assert!(level(1.0) < level(0.0) * 0.6);
    }

    #[test]
    fn voices_follow_their_cars_and_fade_without_clicks() {
        let mut s = TrafficSynth::new(3);
        let mut cars = [OtherCar::default(); VOICES];
        cars[0] = car(7, 10.0, 15.0, 5.0);
        cars[1] = car(9, 20.0, 12.0, -3.0);
        s.set(TrafficParams { cars, interior: 0.0 });
        let a = render(&mut s, 0.5);
        // Car 7 leaves; car 11 arrives in its slot of the list.
        cars[0] = car(11, 12.0, 18.0, 0.0);
        s.set(TrafficParams { cars, interior: 0.0 });
        let b = render(&mut s, 1.5);
        assert!(s.v.iter().any(|v| v.id == Some(9)) && s.v.iter().any(|v| v.id == Some(11)));
        // No sample-to-sample jumps.
        let all: Vec<f32> = a.iter().chain(&b).copied().collect();
        let jump = all.chunks_exact(2).collect::<Vec<_>>().windows(2).map(|w| (w[1][0] - w[0][0]).abs()).fold(0.0f32, f32::max);
        assert!(jump < 0.05, "{jump}");
        // Everyone leaves: silent again.
        s.set(TrafficParams::default());
        render(&mut s, 3.0);
        assert!(!s.active());
    }
}
