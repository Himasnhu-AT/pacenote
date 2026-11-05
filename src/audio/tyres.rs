//! Tyres: drift only. The sliding recordings play when the tyres slip (see
//! [`super::samples`]); rolling road noise and stones were removed (players found the gravel
//! crunch tiring), so a car that grips is quiet and the engine carries the sound.
//!
//! | Class | Sliding |
//! |---|---|
//! | gravel | dirt skids |
//! | tarmac | tyre squeal |
//! | snow | the skid, brighter and softer |
//! | mud | the skid, darkened |
//! | off-road | the skid, darker |
//!
//! Every loop is stereo by reading it twice, half a loop apart (decorrelated channels).

use super::car::{CarParams, GRAVEL, MUD, OFFROAD, SNOW, SURFACES, TARMAC};
use super::dsp::{Biquad, Noise, RATE, Smooth, Wander, smoothstep};
use super::samples::{Bank, Id, Looper};

/// What the tyres send out (air-borne: muffled inside the car).
#[derive(Clone, Copy, Debug, Default)]
pub struct TyreOut {
    pub l: f32,
    pub r: f32,
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
    pub slide: [Smooth; SURFACES],
    gravel_slide: Layer,
    squeal: Layer,
    snow_slide: Layer,
    mud_slide: Layer,
    off_slide: Layer,
    squeal_wander: Wander,
}

impl Tyres {
    pub fn new(seed: u32) -> Self {
        let mut n = Noise::new(seed ^ 0x3333);
        let mut at = |id, lp| Layer::new(id, n.unit() as f64, lp);
        Tyres {
            gravel_slide: at(Id::GravelSlide, None),
            squeal: at(Id::TarmacSqueal, None),
            snow_slide: at(Id::GravelSlide, Some(2400.0)),
            mud_slide: at(Id::GravelSlide, Some(900.0)),
            off_slide: at(Id::GravelSlide, Some(1600.0)),
            n,
            slide: [Smooth::new(0.0, 0.03, 0.09, RATE); SURFACES],
            squeal_wander: Wander::new(3.0),
        }
    }

    #[inline]
    pub fn sample(&mut self, p: &CarParams, speed: f32, bank: Option<&Bank>) -> TyreOut {
        let slide: [f32; SURFACES] = std::array::from_fn(|i| self.slide[i].next(p.surfaces[i].slide));
        // A car crawling sideways barely makes a sound.
        let moving = smoothstep(1.0, 6.0, speed);
        let (mut l, mut r) = (0.0, 0.0);
        let mut add = |(a, b): (f32, f32)| {
            l += a * moving;
            r += b * moving;
        };
        let (gs, ts, ss, ms, os) = (slide[GRAVEL], slide[TARMAC], slide[SNOW], slide[MUD], slide[OFFROAD]);
        if let Some(bank) = bank {
            add(self.gravel_slide.next(bank, 0.9 + 0.1 * gs.min(2.0), gs * 0.3));
            // Squeal past the grip peak; its pitch wanders and rises a little with slip.
            let sq = smoothstep(0.2, 0.9, ts);
            let wob = self.squeal_wander.next(&mut self.n);
            add(self.squeal.next(bank, 0.94 + 0.05 * ts.min(2.0) + 0.03 * wob, sq * 0.3));
            add(self.snow_slide.next(bank, 1.05 + 0.1 * ss.min(1.5), ss * 0.22));
            add(self.mud_slide.next(bank, 0.75, ms * 0.3));
            add(self.off_slide.next(bank, 0.85, os * 0.5));
        }
        TyreOut { l, r }
    }
}
