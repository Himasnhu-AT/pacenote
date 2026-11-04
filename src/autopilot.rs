//! Dev autopilot (feature `dev`): the car drives itself down the road, for recording videos
//! and screenshots frame-accurately. It feeds [`AutoInput`], the same input path as the player
//! (recorded by the ghost, animated by the cockpit hands, shifter and handbrake).
//!
//! Control port:
//! - `autopilot.set {on: bool, pace: 0..1.2, style: "clean" | "drift", crash_at_m: number}`:
//!   any subset. `pace` is the share of the surface's grip used in corners (default 0.6).
//!   `drift` pulls the handbrake into hairpins (grade 1–2) on loose surfaces and carries the
//!   slide. `crash_at_m` (negative or null to clear) drives off the road into the scenery once
//!   past that many metres.
//! - `autopilot.state`: current settings and what it's doing.
//!
//! The driving is `pilot::Pilot` (the rivals' driver, compiled in every build): pure pursuit
//! on the centreline with yaw-rate feedback, speed from the tightest corner within braking
//! distance. This file is only the dev shell: settings, control port, `AutoInput`.

use bevy::prelude::*;
use engine::devtools::{AddControlCommand, params};
use serde_json::{Value, json};

use crate::drive::{AutoInput, CarSpec, DrivenCar, SimulateSet, StageRun};
use crate::pilot::{Pilot, PilotParams};
#[cfg(test)]
use crate::stage::{self, Surface};
use crate::vehicle::{Car, Input, Spec};
use crate::world::StageWorld;

pub struct AutopilotPlugin;

impl Plugin for AutopilotPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Autopilot>()
            .add_systems(FixedUpdate, drive.in_set(AutopilotSet).before(SimulateSet).run_if(resource_exists::<StageWorld>))
            .add_control_command(
                "autopilot.set",
                "Dev autopilot: the car drives itself",
                params(&[
                    ("on", "boolean", "drive (default: unchanged)"),
                    ("pace", "number", "share of grip in corners, 0..1.2 (default 0.6)"),
                    ("style", "string", "clean | drift"),
                    ("crash_at_m", "number", "leave the road into the scenery past this many metres (negative: off)"),
                ]),
                set,
            )
            .add_control_command("autopilot.state", "Dev autopilot state", params(&[]), |w, _| Ok(state(w)));
    }
}

/// The autopilot's fixed-step system (the race's start / recover hold runs after it).
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AutopilotSet;

/// How the autopilot drives.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Autopilot {
    pub on: bool,
    pub pace: f32,
    pub drift: bool,
    pub crash_at_m: Option<f32>,
    /// The driver (its state between steps).
    pilot: Pilot,
    /// What it's doing, for `autopilot.state`.
    doing: &'static str,
    /// Drift tuning: shortest handbrake pull (s), the slide angle that ends it (°), and the
    /// slide angle past which the power-out eases off (°).
    pub hb_min: f32,
    pub hb_release: f32,
    pub power_beta: f32,
    /// Steering into the corner during the pull (share of full input).
    pub hb_steer: f32,
}

impl Default for Autopilot {
    fn default() -> Self {
        let p = PilotParams::default();
        Autopilot { on: false, pace: p.pace, drift: p.drift, crash_at_m: None, pilot: Pilot::new(p), doing: "off", hb_min: p.hb_min, hb_release: p.hb_release, power_beta: p.power_beta, hb_steer: p.hb_steer }
    }
}

fn set(w: &mut World, v: Value) -> Result<Value, String> {
    let mut ap = w.get_resource_mut::<Autopilot>().ok_or("no autopilot")?;
    if let Some(on) = v.get("on").and_then(Value::as_bool) {
        ap.on = on;
    }
    if let Some(p) = v.get("pace").and_then(Value::as_f64) {
        ap.pace = (p as f32).clamp(0.1, 1.2);
    }
    if let Some(s) = v.get("style").and_then(Value::as_str) {
        ap.drift = match s {
            "drift" => true,
            "clean" => false,
            _ => return Err(format!("style: clean | drift, not {s}")),
        };
    }
    if let Some(c) = v.get("crash_at_m") {
        ap.crash_at_m = c.as_f64().filter(|m| *m >= 0.0).map(|m| m as f32);
    }
    Ok(state(w))
}

fn state(w: &mut World) -> Value {
    let Some(ap) = w.get_resource::<Autopilot>().cloned() else { return json!({}) };
    // Where the car is: position, speed, metres along the road, damage (for tracing a drive).
    let car = w.query::<&crate::drive::DrivenCar>().iter(w).next().map(|c| c.car.clone());
    let along = car.as_ref().and_then(|c| w.get_resource::<StageWorld>().and_then(|st| st.road.nearest(Vec2::new(c.pos.x, c.pos.z), 60.0)).map(|r| (r.s, r.lateral)));
    json!({
        "on": ap.on, "pace": ap.pace, "style": if ap.drift { "drift" } else { "clean" }, "crash_at_m": ap.crash_at_m, "doing": ap.doing,
        "pos": car.as_ref().map(|c| [c.pos.x, c.pos.y, c.pos.z]),
        "kmh": car.as_ref().map(|c| c.vel.length() * 3.6),
        "s_m": along.map(|a| a.0), "lateral_m": along.map(|a| a.1),
        "damage": car.as_ref().map(|c| [c.damage.engine, c.damage.steering, c.damage.suspension]),
    })
}

fn drive(time: Res<Time<Fixed>>, mut ap: ResMut<Autopilot>, mut out: ResMut<AutoInput>, stage: Res<StageWorld>, spec: Res<CarSpec>, run: Res<StageRun>, cars: Query<&DrivenCar>) {
    out.0 = None;
    if !ap.on {
        ap.doing = "off";
        return;
    }
    let Ok(c) = cars.single() else { return };
    out.0 = Some(ap.input(&c.car, &spec.0, &stage, run.progress, time.delta_secs()));
}

impl Autopilot {
    /// The input for one fixed step (the settings above, driven by `pilot::Pilot`).
    pub fn input(&mut self, car: &Car, spec: &Spec, w: &StageWorld, progress: f32, dt: f32) -> Input {
        let p = &mut self.pilot.params;
        p.pace = self.pace;
        p.drift = self.drift;
        p.hb_min = self.hb_min;
        p.hb_release = self.hb_release;
        p.power_beta = self.power_beta;
        p.hb_steer = self.hb_steer;
        self.pilot.crash_at_m = self.crash_at_m;
        let input = self.pilot.input(car, spec, &w.road, &w.corners, progress, dt);
        self.doing = self.pilot.doing;
        input
    }
}

#[cfg(test)]
fn left_of(car: &Car) -> Vec3 {
    crate::pilot::left_of(car)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::step;

    /// Drives a stage with the autopilot; returns (metres covered, wrecked, peak sideslip °).
    fn run(stage_id: u32, ap: &mut Autopilot, secs: f32) -> (f32, bool, f32) {
        let def = stage::stages().into_iter().find(|d| d.id == stage_id).unwrap();
        let w = crate::world::generate(&def);
        let (p, h) = w.road.start();
        let spec = Spec::default();
        let mut car = Car::new(p + Vec3::Y * 0.9, h);
        let mut progress = 0.0f32;
        let mut beta = 0.0f32;
        for _ in 0..(secs * 60.0) as usize {
            let input = ap.input(&car, &spec, &w, progress, 1.0 / 60.0);
            step(&mut car, &spec, input, &w, 1.0 / 60.0);
            if let Some(rp) = w.road.nearest(Vec2::new(car.pos.x, car.pos.z), 40.0)
                && rp.s > progress
                && rp.s < progress + 40.0
            {
                progress = rp.s;
            }
            beta = beta.max(car.vel.dot(left_of(&car)).atan2(car.speed().abs().max(1.0)).abs().to_degrees());
            if car.damage.wrecked() {
                return (progress, true, beta);
            }
        }
        (progress, false, beta)
    }

    #[test]
    fn drives_a_stage_clean() {
        let mut ap = Autopilot { on: true, ..Autopilot::default() };
        let (m, wrecked, _) = run(1, &mut ap, 60.0);
        assert!(!wrecked && m > 700.0, "{m:.0} m in a minute, wrecked {wrecked}");
    }

    #[test]
    fn crashes_on_request() {
        let mut ap = Autopilot { on: true, crash_at_m: Some(150.0), ..Autopilot::default() };
        let (m, _, _) = run(1, &mut ap, 40.0);
        assert!(m < 400.0, "still going at {m:.0} m");
    }

    /// Hairpins (grade 1–2) on loose ground in the first ~900 m of each stage, and how long the
    /// autopilot takes to get there.
    #[test]
    #[ignore]
    fn early_hairpins() {
        for def in stage::stages() {
            let w = crate::world::generate(&def);
            let pins: Vec<String> = w
                .corners
                .iter()
                .filter(|c| c.grade <= 2 && c.start < 900.0 && w.road.surface[w.road.station(c.start)] != Surface::Tarmac)
                .map(|c| format!("{:.0} m grade {} {:?} {:?}", c.start, c.grade, c.turn, w.road.surface[w.road.station(c.start)]))
                .collect();
            if pins.is_empty() {
                continue;
            }
            let mut ap = Autopilot { on: true, ..Autopilot::default() };
            let first = w.corners.iter().find(|c| c.grade <= 2 && c.start < 900.0).map_or(0.0, |c| c.start);
            let def_id = def.id;
            // Time to reach the first one.
            let wr = w.clone();
            let (p, h) = wr.road.start();
            let spec = Spec::default();
            let mut car = Car::new(p + Vec3::Y * 0.9, h);
            let (mut progress, mut t) = (0.0f32, 0.0f32);
            while progress < first && t < 120.0 {
                let input = ap.input(&car, &spec, &wr, progress, 1.0 / 60.0);
                step(&mut car, &spec, input, &wr, 1.0 / 60.0);
                if let Some(rp) = wr.road.nearest(Vec2::new(car.pos.x, car.pos.z), 40.0)
                    && rp.s > progress
                    && rp.s < progress + 40.0
                {
                    progress = rp.s;
                }
                t += 1.0 / 60.0;
            }
            println!("stage {def_id:2} {:14} first at {t:4.1} s: {}", def.name, pins.join(", "));
        }
    }
}

#[cfg(test)]
mod drift_tests {
    use super::*;
    use crate::vehicle::step;

    /// Drift style through the early gravel hairpins: real slip angle, no wreck.
    #[test]
    #[ignore]
    fn drift_report() {
        for (id, secs) in [(15u32, 40.0f32), (16, 45.0), (1, 60.0), (17, 35.0), (5, 30.0), (8, 30.0)] {
            let def = stage::stages().into_iter().find(|d| d.id == id).unwrap();
            let w = crate::world::generate(&def);
            for drift in [false, true] {
                let mut ap = Autopilot { on: true, drift, pace: std::env::var("PACE").ok().and_then(|s| s.parse().ok()).unwrap_or(0.6), ..Autopilot::default() };
                let (p, h) = w.road.start();
                let spec = Spec::default();
                let mut car = Car::new(p + Vec3::Y * 0.9, h);
                let (mut progress, mut beta, mut flicks, mut was, mut offs, mut off) = (0.0f32, 0.0f32, 0, false, 0, false);
                let mut slides: Vec<f32> = Vec::new();
                for _ in 0..(secs * 60.0) as usize {
                    let input = ap.input(&car, &spec, &w, progress, 1.0 / 60.0);
                    flicks += (input.handbrake && !was) as u32;
                    was = input.handbrake;
                    step(&mut car, &spec, input, &w, 1.0 / 60.0);
                    if let Some(rp) = w.road.nearest(Vec2::new(car.pos.x, car.pos.z), 40.0)
                        && rp.s > progress
                        && rp.s < progress + 40.0
                    {
                        progress = rp.s;
                    }
                    let b = car.vel.dot(left_of(&car)).atan2(car.speed().abs().max(1.0)).abs().to_degrees();
                    beta = beta.max(b);
                    if was {
                        slides.push(0.0);
                    }
                    if let Some(l) = slides.last_mut() {
                        *l = l.max(b);
                    }
                    let now_off = w.road.nearest(Vec2::new(car.pos.x, car.pos.z), 40.0).is_none_or(|r| r.lateral.abs() > w.road.half_width(&r) + 0.5);
                    offs += (now_off && !off) as u32;
                    off = now_off;
                }
                slides.dedup();
                println!("stage {id} drift {drift}: {progress:.0} m in {secs} s, peak β {beta:.0}°, flicks {flicks}, offs {offs}, damage {:.2}, hits {}", car.damage.worst(), car.damage.hits);
            }
        }
    }
}

#[cfg(test)]
mod drift_tune {
    use super::*;
    use crate::vehicle::step;

    /// Sweeps the drift tuning over the early-hairpin stages: per setting, the peak slide of
    /// each flick, spins (> 60°) and trips off the road.
    #[test]
    #[ignore]
    fn sweep() {
        let worlds: Vec<_> = [15u32, 16, 17, 5, 8].iter().map(|id| crate::world::generate(&stage::stages().into_iter().find(|d| d.id == *id).unwrap())).collect();
        for hb_steer in [0.5f32, 0.7, 1.0] {
            let hb_min = 0.15;
            for hb_release in [12.0f32, 15.0, 20.0] {
                for power_beta in [15.0f32] {
                    let (mut peaks, mut spins, mut offs) = (Vec::new(), 0, 0);
                    for w in &worlds {
                        let mut ap = Autopilot { on: true, drift: true, pace: 0.6, hb_min, hb_release, power_beta, hb_steer, ..Autopilot::default() };
                        let (p, h) = w.road.start();
                        let spec = Spec::default();
                        let mut car = Car::new(p + Vec3::Y * 0.9, h);
                        let (mut progress, mut cur, mut sliding, mut off) = (0.0f32, 0.0f32, 0u32, false);
                        for _ in 0..(40 * 60) {
                            let input = ap.input(&car, &spec, w, progress, 1.0 / 60.0);
                            step(&mut car, &spec, input, w, 1.0 / 60.0);
                            if let Some(rp) = w.road.nearest(Vec2::new(car.pos.x, car.pos.z), 40.0)
                                && rp.s > progress
                                && rp.s < progress + 40.0
                            {
                                progress = rp.s;
                            }
                            let b = car.vel.dot(left_of(&car)).atan2(car.speed().abs().max(1.0)).abs().to_degrees();
                            if input.handbrake {
                                sliding = 120;
                            }
                            if sliding > 0 {
                                cur = cur.max(b);
                                sliding -= 1;
                                if sliding == 0 {
                                    peaks.push(cur);
                                    spins += (cur > 60.0) as u32;
                                    cur = 0.0;
                                }
                            }
                            let now_off = w.road.nearest(Vec2::new(car.pos.x, car.pos.z), 40.0).is_none_or(|r| r.lateral.abs() > w.road.half_width(&r) + 0.5);
                            offs += (now_off && !off) as u32;
                            off = now_off;
                        }
                    }
                    let good = peaks.iter().filter(|p| (20.0..=45.0).contains(*p)).count();
                    println!("hb_steer {hb_steer:.1} hb_min {hb_min:.2} release {hb_release:2.0} power {power_beta:2.0}: flicks {} good {good} spins {spins} offs {offs} peaks {:?}", peaks.len(), peaks.iter().map(|p| p.round() as i32).collect::<Vec<_>>());
                }
            }
        }
    }
}

#[cfg(test)]
mod drift_trace {
    use super::*;
    use crate::vehicle::step;

    #[test]
    #[ignore]
    fn trace() {
        let id: u32 = std::env::var("STAGE").ok().and_then(|s| s.parse().ok()).unwrap_or(16);
        let (a, b): (f32, f32) = (std::env::var("FROM").ok().and_then(|s| s.parse().ok()).unwrap_or(500.0), std::env::var("TO").ok().and_then(|s| s.parse().ok()).unwrap_or(620.0));
        let w = crate::world::generate(&stage::stages().into_iter().find(|d| d.id == id).unwrap());
        for c in w.corners.iter().filter(|c| c.start < 1200.0) {
            println!("corner {:.0}-{:.0} grade {} {:?} {:.0}° {:?}", c.start, c.end, c.grade, c.turn, c.angle, w.road.surface[w.road.station(c.start)]);
        }
        let mut ap = Autopilot { on: true, drift: true, pace: 0.65, ..Autopilot::default() };
        let (p, h) = w.road.start();
        let spec = Spec::default();
        let mut car = Car::new(p + Vec3::Y * 0.9, h);
        let mut progress = 0.0f32;
        for i in 0..(60 * 60) {
            let input = ap.input(&car, &spec, &w, progress, 1.0 / 60.0);
            step(&mut car, &spec, input, &w, 1.0 / 60.0);
            let rp = w.road.nearest(Vec2::new(car.pos.x, car.pos.z), 40.0);
            if let Some(rp) = rp
                && rp.s > progress
                && rp.s < progress + 40.0
            {
                progress = rp.s;
            }
            if progress > a && progress < b && i % 6 == 0 {
                let beta = car.vel.dot(left_of(&car)).atan2(car.speed().abs().max(1.0)).to_degrees();
                println!("s {progress:5.0} v {:5.1} β {beta:6.1} yaw {:6.1} st {:5.2} th {:.2} br {:.2} hb {} lat {:5.1} {} dmg {:.2}", car.speed() * 3.6, car.ang.dot(car.up()).to_degrees(), input.steer, input.throttle, input.brake, input.handbrake as u8, rp.map_or(99.0, |r| r.lateral), ap.doing, car.damage.worst());
            }
            if progress > b {
                break;
            }
        }
    }
}
