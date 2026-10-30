//! Ghosts: a run recorded as the driver's per-fixed-step inputs, replayed exactly.
//!
//! `vehicle::step` is deterministic, so the start pose (from the road), the inputs of every
//! fixed step since the car spawned (countdown included) and the recoveries are enough to
//! rebuild the whole run bit for bit. Inputs are stored as raw f32 bits, run-length encoded
//! (keyboard driving repeats the same input for many steps): a 4 min stage is ~2–20 KB of JSON.
//!
//! - [`Recorder`] (resource): `drive::simulate` feeds it the input of every step and every
//!   recovery; [`Recorder::ghost`] turns it into a [`Ghost`].
//! - [`Replay`]: steps a ghost car forward one fixed step at a time; [`replay`] runs a whole
//!   ghost (tests, verification).
//! - A ghost carries the stage hash and a hash of the car spec: when either changed since it
//!   was recorded the replay may drift, so the UI marks it stale.

use bevy::math::Vec3;
use bevy::prelude::{Entity, Resource};
use serde::{Deserialize, Serialize};

use crate::stage::Road;
use crate::vehicle::{self, Car, Ground, Input, Spec};

use super::board::{Hasher, clean_name, team_key};

pub const GHOST_FORMAT: u32 = 1;

/// A recorded run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Ghost {
    pub format: u32,
    pub stage: u32,
    pub stage_hash: String,
    /// [`sim_hash`] of the car spec it was driven with.
    pub sim_hash: String,
    pub driver: String,
    pub guide: String,
    /// Final time with penalties.
    pub total: f32,
    pub date: u64,
    pub version: String,
    /// Fixed steps per second it was recorded at.
    pub hz: u32,
    /// Run-length encoded inputs: `[steer, throttle, brake (f32 bits), handbrake 0/1, steps]`.
    pub inputs: Vec<[u32; 5]>,
    /// Recoveries: `[after this many steps, x, y, z, heading (f32 bits)]`.
    pub recovers: Vec<[u32; 5]>,
}

impl Ghost {
    pub fn steps(&self) -> u32 {
        self.inputs.iter().map(|r| r[4]).sum()
    }
    pub fn team(&self) -> String {
        team_key(&self.driver, &self.guide)
    }
    /// File name inside the ghosts folder: one per stage per team.
    pub fn file_name(&self) -> String {
        ghost_file(self.stage, &self.team())
    }
}

pub fn ghost_file(stage: u32, team: &str) -> String {
    let mut h = Hasher::default();
    h.str(team);
    format!("s{stage:02}-{:012x}.json", h.0 & 0xFFFF_FFFF_FFFF)
}

/// Hash of everything besides the inputs that decides how a run plays out: the car spec.
pub fn sim_hash(spec: &Spec, hz: u32) -> String {
    let mut h = Hasher::default();
    h.str(&format!("{spec:?}"));
    h.u64(hz as u64);
    h.u64(vehicle::SUBSTEPS as u64);
    // Sim revisions that change how the same inputs drive (2: damage).
    h.u64(2);
    format!("{:016x}", h.0)
}

/// The car as `drive::spawn_car` places it on the start line.
pub fn start_car(road: &Road) -> Car {
    let (pos, heading) = road.start();
    Car::new(pos + Vec3::Y * 0.9, heading)
}

fn enc(i: Input) -> [u32; 4] {
    [i.steer.to_bits(), i.throttle.to_bits(), i.brake.to_bits(), i.handbrake as u32]
}

fn dec(r: &[u32; 5]) -> Input {
    Input { steer: f32::from_bits(r[0]), throttle: f32::from_bits(r[1]), brake: f32::from_bits(r[2]), handbrake: r[3] != 0 }
}

/// Records the driven car's inputs (fed by `drive::simulate`).
#[derive(Resource, Clone, Debug, Default)]
pub struct Recorder {
    /// The car being recorded: a different entity restarts the recording.
    pub car: Option<Entity>,
    pub steps: u32,
    pub inputs: Vec<[u32; 5]>,
    pub recovers: Vec<[u32; 5]>,
}

impl Recorder {
    /// The input applied to `car` this fixed step.
    pub fn input(&mut self, car: Entity, i: Input) {
        if self.car != Some(car) {
            self.reset();
            self.car = Some(car);
        }
        let e = enc(i);
        match self.inputs.last_mut() {
            Some(last) if last[..4] == e => last[4] += 1,
            _ => self.inputs.push([e[0], e[1], e[2], e[3], 1]),
        }
        self.steps += 1;
    }
    /// `vehicle::recover(car, pos, heading)` was applied after the steps so far.
    pub fn recover(&mut self, car: Entity, pos: Vec3, heading: f32) {
        if self.car != Some(car) {
            return;
        }
        self.recovers.push([self.steps, pos.x.to_bits(), pos.y.to_bits(), pos.z.to_bits(), heading.to_bits()]);
    }
    pub fn reset(&mut self) {
        *self = Recorder::default();
    }
    #[allow(clippy::too_many_arguments)]
    pub fn ghost(&self, stage: u32, stage_hash: &str, sim_hash: &str, driver: &str, guide: &str, total: f32, date: u64, version: &str, hz: u32) -> Ghost {
        Ghost {
            format: GHOST_FORMAT,
            stage,
            stage_hash: stage_hash.into(),
            sim_hash: sim_hash.into(),
            driver: clean_name(driver),
            guide: clean_name(guide),
            total,
            date,
            version: version.into(),
            hz,
            inputs: self.inputs.clone(),
            recovers: self.recovers.clone(),
        }
    }
}

/// A ghost car being played back, one fixed step at a time.
#[derive(Clone, Debug)]
pub struct Replay {
    pub car: Car,
    /// Steps applied so far.
    pub step: u32,
    run: usize,
    in_run: u32,
    next_recover: usize,
}

impl Replay {
    /// Starts a replay of `g` from `start` (the car on the start line).
    pub fn new(g: &Ghost, start: Car) -> Replay {
        let mut r = Replay { car: start, step: 0, run: 0, in_run: 0, next_recover: 0 };
        r.apply_recovers(g);
        r
    }
    pub fn done(&self, g: &Ghost) -> bool {
        self.run >= g.inputs.len()
    }
    /// Applies the next recorded step (after the end: holds the handbrake).
    pub fn advance(&mut self, g: &Ghost, spec: &Spec, ground: &impl Ground, dt: f32) {
        let input = match g.inputs.get(self.run) {
            Some(r) => {
                let i = dec(r);
                self.in_run += 1;
                if self.in_run >= r[4] {
                    self.run += 1;
                    self.in_run = 0;
                }
                i
            }
            None => Input { handbrake: true, ..Default::default() },
        };
        vehicle::step(&mut self.car, spec, input, ground, dt);
        self.step += 1;
        self.apply_recovers(g);
    }
    /// Recoveries recorded after the steps applied so far.
    fn apply_recovers(&mut self, g: &Ghost) {
        while let Some(r) = g.recovers.get(self.next_recover).filter(|r| r[0] <= self.step) {
            let pos = Vec3::new(f32::from_bits(r[1]), f32::from_bits(r[2]), f32::from_bits(r[3]));
            vehicle::recover(&mut self.car, pos, f32::from_bits(r[4]));
            self.next_recover += 1;
        }
    }
}

/// Plays a whole ghost from `start`; returns the final car.
pub fn replay(g: &Ghost, start: Car, spec: &Spec, ground: &impl Ground, dt: f32) -> Car {
    let mut r = Replay::new(g, start);
    for _ in 0..g.steps() {
        r.advance(g, spec, ground, dt);
    }
    r.car
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::Surface;
    use crate::vehicle::Flat;

    fn input_at(i: u32) -> Input {
        Input {
            steer: if (i / 40).is_multiple_of(3) { 0.0 } else { ((i as f32) * 0.031).sin() },
            throttle: if i % 200 < 150 { 1.0 } else { 0.0 },
            brake: if i % 200 >= 170 { 0.7 } else { 0.0 },
            handbrake: i % 300 > 290,
        }
    }

    #[test]
    fn recorded_run_replays_to_the_same_car() {
        let spec = Spec::default();
        let g = Flat(0.0, Surface::Gravel);
        let dt = 1.0 / 60.0;
        let start = Car::new(Vec3::new(0.0, 0.9, 0.0), 0.4);
        let mut car = start.clone();
        let mut rec = Recorder::default();
        let e = Entity::from_raw_u32(7).unwrap();
        for i in 0..900 {
            let input = input_at(i);
            rec.input(e, input);
            vehicle::step(&mut car, &spec, input, &g, dt);
            if i == 500 {
                let (p, h) = (car.pos + Vec3::new(3.0, 0.0, 1.0), 1.1);
                vehicle::recover(&mut car, p, h);
                rec.recover(e, p, h);
            }
        }
        let ghost = rec.ghost(1, "aa", &sim_hash(&spec, 60), "A", "B", 15.0, 0, "0.1.0", 60);
        assert_eq!(ghost.steps(), 900);
        assert!(ghost.inputs.len() < 900, "run-length encoding merged repeats ({} runs)", ghost.inputs.len());
        assert_eq!(ghost.recovers.len(), 1);
        // Through JSON and back: bit-exact.
        let json = serde_json::to_string(&ghost).unwrap();
        let back: Ghost = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ghost);
        let replayed = replay(&back, start, &spec, &g, dt);
        assert_eq!(replayed, car, "replay must end in exactly the recorded state");
    }

    #[test]
    fn new_car_entity_restarts_the_recording() {
        let mut rec = Recorder::default();
        let (a, b) = (Entity::from_raw_u32(1).unwrap(), Entity::from_raw_u32(2).unwrap());
        rec.input(a, Input::default());
        rec.input(a, Input::default());
        rec.recover(b, Vec3::ZERO, 0.0);
        assert!(rec.recovers.is_empty(), "recoveries of another car are ignored");
        rec.input(b, Input::default());
        assert_eq!((rec.steps, rec.inputs.len()), (1, 1));
    }

    #[test]
    fn spec_changes_change_the_sim_hash() {
        let a = Spec::default();
        let b = Spec { torque: a.torque + 1.0, ..Spec::default() };
        assert_eq!(sim_hash(&a, 60), sim_hash(&Spec::default(), 60));
        assert_ne!(sim_hash(&a, 60), sim_hash(&b, 60));
        assert_ne!(sim_hash(&a, 60), sim_hash(&a, 120));
        assert_ne!(ghost_file(1, "a|b"), ghost_file(1, "a|c"));
    }
}
