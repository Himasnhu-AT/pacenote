//! The driving core of the AI: a pure input function over the route, its corners and the
//! progress, compiled in every build. Users: the rivals (`career::racers`), the dev autopilot
//! (`autopilot.rs`, a thin shell over this), the challenge star bake.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §5 and AI-1 (there
//! called `ai::Driver`; DECISIONS R3 names it `pilot::Pilot`).
//!
//! The path follower from the handling tests: pure pursuit on the centreline (or a line
//! offset from it) with yaw-rate feedback, the speed from the tightest corner within braking
//! distance at `pace` of the surface's grip. Racing adds a line offset, a speed cap (gap
//! keeping, yielding), a pace modifier (the rubber band) and seeded mistakes. Pure: no RNG but
//! a hash of the seed, no frame time (the step's `dt` is fixed).

use bevy::prelude::*;

use crate::stage::{self, Corner, Road, Surface, Turn};
use crate::vehicle::{self, Car, Input, Spec};

/// How a pilot drives (a difficulty row of GAME_SYSTEMS §5, plus the per-rival spread).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PilotParams {
    /// Share of grip used in corners.
    pub pace: f32,
    /// Share of grip used braking.
    pub brake: f32,
    /// Lateral line offset (m, left positive) it holds when nothing else asks for one.
    pub line_offset: f32,
    /// Seed for mistakes (`hash(event, difficulty, attempt, slot)`).
    pub seed: u64,
    /// Chance of a mistake at a corner of grade ≤ 2 (0 for the autopilot).
    pub mistake: f32,
    /// Drift style: a handbrake flick into loose hairpins, carried on the power.
    pub drift: bool,
    /// Drift tuning: shortest handbrake pull (s), the slide angle that ends it (°), the slide
    /// angle past which the power-out eases off (°), steering during the pull (share).
    pub hb_min: f32,
    pub hb_release: f32,
    pub power_beta: f32,
    pub hb_steer: f32,
}

impl Default for PilotParams {
    fn default() -> Self {
        PilotParams { pace: 0.6, brake: 0.55, line_offset: 0.0, seed: 0, mistake: 0.0, drift: false, hb_min: 0.15, hb_release: 20.0, power_beta: 15.0, hb_steer: 0.5 }
    }
}

/// One AI driver's state.
#[derive(Clone, Debug, PartialEq)]
pub struct Pilot {
    pub params: PilotParams,
    /// Leave the road into the scenery past this many metres (dev autopilot only).
    pub crash_at_m: Option<f32>,
    /// Pace multiplier set by the race each step (the light rubber band).
    pub pace_mod: f32,
    /// Most speed (m/s) the race allows this step (gap keeping, yielding).
    pub speed_cap: Option<f32>,
    /// Lateral offset (m, left +) the race asks for this step (overtaking, yielding); `None`:
    /// the pilot's own line.
    pub offset_target: Option<f32>,
    /// The offset it is steering at now (eased towards the target).
    pub offset: f32,
    /// Seconds of handbrake left in a drift entry, and the corner (start m) it was for.
    handbrake: f32,
    last_hairpin: f32,
    /// A mistake in progress: seconds of it left.
    mistake_left: f32,
    /// Corners already rolled for a mistake (by start m), newest last (a short ring).
    rolled: [f32; 4],
    /// What it's doing (dev state).
    pub doing: &'static str,
}

impl Default for Pilot {
    fn default() -> Self {
        Pilot::new(PilotParams::default())
    }
}

impl Pilot {
    pub fn new(params: PilotParams) -> Pilot {
        Pilot { params, crash_at_m: None, pace_mod: 1.0, speed_cap: None, offset_target: None, offset: params.line_offset, handbrake: 0.0, last_hairpin: -1.0, mistake_left: 0.0, rolled: [-1.0; 4], doing: "off" }
    }

    /// Is a mistake under way?
    pub fn in_mistake(&self) -> bool {
        self.mistake_left > 0.0
    }

    /// The input for one fixed step. `progress`: metres along `road` (best so far).
    pub fn input(&mut self, car: &Car, spec: &Spec, road: &Road, corners: &[Corner], progress: f32, dt: f32) -> Input {
        let p = self.params;
        let pos = Vec2::new(car.pos.x, car.pos.z);
        let rp = road.nearest(pos, 40.0);
        let s_here = rp.map_or(progress, |r| r.s);
        let off_road = rp.is_none_or(|r| r.lateral.abs() > road.half_width(&r) + 0.3);
        let v = car.speed().max(0.0);
        let look = (v * 0.7).clamp(6.0, 30.0);

        // Crash on request: aim at a point well off the road ahead, flat out.
        if let Some(m) = self.crash_at_m
            && s_here >= m
        {
            self.doing = "crashing";
            let i = road.station(s_here + 25.0);
            let d = road.dir[i];
            let left = Vec2::new(d.y, -d.x);
            let q = road.pts[i];
            let target = Vec3::new(q.x + left.x * 14.0, q.y, q.z + left.y * 14.0);
            return Input { steer: pursuit(car, spec, target), throttle: 1.0, ..Input::default() };
        }

        // The line: the centre, or offset from it (kept 1.1 m inside each edge).
        let want = self.offset_target.unwrap_or(p.line_offset);
        self.offset += (want - self.offset).clamp(-1.2 * dt, 1.2 * dt);
        let i_look = road.station(s_here + look);
        let mut target = road.pts[i_look];
        if self.offset != 0.0 {
            let d = road.dir[i_look];
            let room = (road.width[i_look] * 0.5 - 1.1).max(0.0);
            target += Vec3::new(d.y, 0.0, -d.x) * self.offset.clamp(-room, room);
        }
        let mut steer = pursuit(car, spec, target);

        // Speed: the slowest corner within braking distance, at `pace` of the surface's grip.
        let pace = p.pace * self.pace_mod;
        let i0 = road.station(s_here);
        let mut vt = 30.0f32;
        let horizon = (v * v / 6.0 + 30.0) / stage::STEP;
        for k in 0..horizon as usize {
            let i = (i0 + k).min(road.len() - 1);
            let (mu, _) = road.surface[i].grip();
            let mu = mu * spec.grip;
            let a_lat = pace * mu * 9.81;
            let a_brk = p.brake * mu * 9.81;
            let curv = road.curv[i].abs().max(1e-4);
            let vc = (a_lat / curv).sqrt();
            vt = vt.min((vc * vc + 2.0 * a_brk * k as f32 * stage::STEP).sqrt());
        }
        if off_road {
            vt = vt.min(12.0);
        }

        // A mistake: at a tight corner (grade ≤ 2), now and then (seeded), it lifts and
        // over-slows into it (a fluffed entry: costs time, never a crash).
        if p.mistake > 0.0 {
            if let Some(c) = corners.iter().find(|c| c.grade <= 2 && c.start > s_here && c.start - s_here < 25.0)
                && !self.rolled.contains(&c.start)
            {
                self.rolled.rotate_left(1);
                self.rolled[3] = c.start;
                if unit(p.seed, c.start.to_bits() as u64) < p.mistake {
                    self.mistake_left = 0.4 + 1.2 * unit(p.seed ^ 0x9e37, c.start.to_bits() as u64);
                }
            }
            if self.mistake_left > 0.0 {
                self.mistake_left -= dt;
                vt *= 0.72;
            }
        }
        if let Some(cap) = self.speed_cap {
            vt = vt.min(cap);
        }
        let err = vt - car.speed();
        let mut throttle = (err * 0.6).clamp(0.0, 1.0);
        let brake = (-err * 0.4).clamp(0.0, 1.0);
        // Ease off in a big slide, as a driver would (less so when drifting on purpose).
        let beta = car.vel.dot(left_of(car)).atan2(car.speed().abs().max(1.0)).abs().to_degrees();
        throttle *= (1.0 - (beta - if p.drift { 25.0 } else { 12.0 }) / 20.0).clamp(0.2, 1.0);
        self.doing = if off_road {
            "off the road"
        } else if self.mistake_left > 0.0 {
            "fluffed it"
        } else {
            "driving"
        };

        // Drift style: brake to a drift entry speed, a handbrake flick at the turn-in of a hairpin
        // on loose ground, hold it until the car has rotated, then power through the corner.
        let mut handbrake = false;
        let mut brake = brake;
        if p.drift {
            let loose = |c: &Corner| road.surface[road.station(c.start)] != Surface::Tarmac;
            // Entry speed for a grade 1 / 2 hairpin (≈45 / 55 km/h at pace 0.6).
            let entry = |c: &Corner| (if c.grade <= 1 { 12.5 } else { 15.0 }) * (p.pace / 0.6).sqrt();
            if let Some(c) = corners.iter().find(|c| c.grade <= 2 && c.start > s_here && c.start - s_here < 60.0 && c.start != self.last_hairpin && loose(c)) {
                let d = c.start - s_here;
                // Braking towards the entry speed (the planner alone carried ~95 km/h into one).
                let (mu, _) = road.surface[road.station(c.start)].grip();
                let cap = (entry(c) * entry(c) + 2.0 * 0.6 * mu * 9.81 * (d - 8.0).max(0.0)).sqrt();
                if v > cap {
                    throttle = 0.0;
                    brake = brake.max(((v - cap) * 0.5).clamp(0.3, 1.0));
                    self.doing = "braking for a hairpin";
                }
                // The flick: just before the turn-in, at about the entry speed.
                if self.handbrake <= 0.0 && d < (v * 0.35).clamp(5.0, 12.0) && v > 8.0 && v < entry(c) * 1.25 {
                    self.handbrake = 0.7;
                    self.last_hairpin = c.start;
                }
            }
            if self.handbrake > 0.0 {
                self.handbrake -= dt;
                // Hold at least `hb_min`, then let go once it has started to rotate (it carries on).
                if self.handbrake < 0.7 - p.hb_min && beta > p.hb_release {
                    self.handbrake = 0.0;
                }
            }
            let corner = corners.iter().find(|c| c.start == self.last_hairpin);
            if self.handbrake > 0.0 {
                handbrake = true;
                self.doing = "drifting";
                let into = corner.map_or(steer.signum(), |c| if c.turn == Turn::Left { 1.0 } else { -1.0 });
                // Not full lock: at hairpin speeds that plus the handbrake spins the car.
                steer = p.hb_steer * into;
                throttle = 0.4;
            } else if let Some(c) = corner
                && s_here >= c.start - 2.0
                && s_here < c.end
                && beta > 8.0
            {
                // In the hairpin on the power: the slide carries through (easing off if it grows
                // past ~35°, or it spins).
                throttle = (1.0 - (beta - p.power_beta) / 15.0).clamp(0.2, 1.0);
                brake = 0.0;
                self.doing = "drifting";
            }
        }
        Input { steer, throttle, brake: if handbrake { 0.0 } else { brake }, handbrake }
    }
}

/// The car's left (body frame, turned by its attitude).
pub fn left_of(car: &Car) -> Vec3 {
    car.up().cross(car.forward())
}

/// Pure pursuit towards `target` with yaw-rate feedback, through the car's own steering map.
pub fn pursuit(car: &Car, spec: &Spec, target: Vec3) -> f32 {
    let local = car.rot.inverse() * (target - car.pos);
    let d2 = (local.x * local.x + local.z * local.z).max(1.0);
    let kappa = 2.0 * -local.x / d2;
    let wheelbase = spec.mounts[2].z - spec.mounts[0].z;
    let yaw = car.ang.dot(car.up());
    let delta = (kappa * wheelbase).atan() + 0.12 * (car.speed().max(0.0) * kappa - yaw);
    vehicle::steer_input_for(car, spec, delta)
}

/// A deterministic number in 0..1 from a seed and a key (SplitMix64).
pub fn unit(seed: u64, key: u64) -> f32 {
    let mut z = seed ^ key.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// A seed from parts (event, difficulty, attempt, slot...).
pub fn seed(parts: &[u64]) -> u64 {
    parts.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, p| (h ^ p).wrapping_mul(0x0100_0000_01b3).rotate_left(17))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_is_uniform_enough_and_deterministic() {
        let n = 10_000;
        let mean: f32 = (0..n).map(|k| unit(42, k)).sum::<f32>() / n as f32;
        assert!((mean - 0.5).abs() < 0.02, "{mean}");
        assert_eq!(unit(7, 9), unit(7, 9));
        assert_ne!(seed(&[100, 1, 0, 2]), seed(&[100, 1, 0, 3]));
        assert!((0..1000).all(|k| (0.0..1.0).contains(&unit(3, k))));
    }
}
