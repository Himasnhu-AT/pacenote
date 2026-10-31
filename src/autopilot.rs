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
//! The driving is the path follower from the handling tests: pure pursuit on the centreline
//! with yaw-rate feedback, speed from the tightest corner within braking distance.

use bevy::prelude::*;
use engine::devtools::{AddControlCommand, params};
use serde_json::{Value, json};

use crate::drive::{AutoInput, CarSpec, DrivenCar, SimulateSet, StageRun};
use crate::stage::{self, Surface, Turn};
use crate::vehicle::{self, Car, Input, Spec};
use crate::world::StageWorld;

pub struct AutopilotPlugin;

impl Plugin for AutopilotPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Autopilot>()
            .add_systems(FixedUpdate, drive.before(SimulateSet).run_if(resource_exists::<StageWorld>))
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

/// How the autopilot drives.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Autopilot {
    pub on: bool,
    pub pace: f32,
    pub drift: bool,
    pub crash_at_m: Option<f32>,
    /// Seconds of handbrake left in a drift entry, and the corner (start m) it was for.
    handbrake: f32,
    last_hairpin: f32,
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
        Autopilot { on: false, pace: 0.6, drift: false, crash_at_m: None, handbrake: 0.0, last_hairpin: -1.0, doing: "off", hb_min: 0.15, hb_release: 20.0, power_beta: 15.0, hb_steer: 0.5 }
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
    let Some(ap) = w.get_resource::<Autopilot>() else { return json!({}) };
    json!({"on": ap.on, "pace": ap.pace, "style": if ap.drift { "drift" } else { "clean" }, "crash_at_m": ap.crash_at_m, "doing": ap.doing})
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
    /// The input for one fixed step.
    pub fn input(&mut self, car: &Car, spec: &Spec, w: &StageWorld, progress: f32, dt: f32) -> Input {
        let road = &w.road;
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
            let p = road.pts[i];
            let target = Vec3::new(p.x + left.x * 14.0, p.y, p.z + left.y * 14.0);
            return Input { steer: pursuit(car, spec, target), throttle: 1.0, ..Input::default() };
        }

        let target = road.pts[road.station(s_here + look)];
        let mut steer = pursuit(car, spec, target);

        // Speed: the slowest corner within braking distance, at `pace` of the surface's grip.
        let i0 = road.station(s_here);
        let mut vt = 30.0f32;
        let horizon = (v * v / 6.0 + 30.0) / stage::STEP;
        for k in 0..horizon as usize {
            let i = (i0 + k).min(road.len() - 1);
            let (mu, _) = road.surface[i].grip();
            let a_lat = self.pace * mu * 9.81;
            let a_brk = 0.55 * mu * 9.81;
            let curv = road.curv[i].abs().max(1e-4);
            let vc = (a_lat / curv).sqrt();
            vt = vt.min((vc * vc + 2.0 * a_brk * k as f32 * stage::STEP).sqrt());
        }
        if off_road {
            vt = vt.min(12.0);
        }
        let err = vt - car.speed();
        let mut throttle = (err * 0.6).clamp(0.0, 1.0);
        let brake = (-err * 0.4).clamp(0.0, 1.0);
        // Ease off in a big slide, as a driver would (less so when drifting on purpose).
        let beta = car.vel.dot(left_of(car)).atan2(car.speed().abs().max(1.0)).abs().to_degrees();
        throttle *= (1.0 - (beta - if self.drift { 25.0 } else { 12.0 }) / 20.0).clamp(0.2, 1.0);
        self.doing = if off_road { "off the road" } else { "driving" };

        // Drift style: brake to a drift entry speed, a handbrake flick at the turn-in of a hairpin
        // on loose ground, hold it until the car has rotated, then power through the corner.
        let mut handbrake = false;
        let mut brake = brake;
        if self.drift {
            let loose = |c: &stage::Corner| road.surface[road.station(c.start)] != Surface::Tarmac;
            // Entry speed for a grade 1 / 2 hairpin (≈45 / 55 km/h at pace 0.6).
            let entry = |c: &stage::Corner| (if c.grade <= 1 { 12.5 } else { 15.0 }) * (self.pace / 0.6).sqrt();
            if let Some(c) = w.corners.iter().find(|c| c.grade <= 2 && c.start > s_here && c.start - s_here < 60.0 && c.start != self.last_hairpin && loose(c)) {
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
                if self.handbrake < 0.7 - self.hb_min && beta > self.hb_release {
                    self.handbrake = 0.0;
                }
            }
            let corner = w.corners.iter().find(|c| c.start == self.last_hairpin);
            if self.handbrake > 0.0 {
                handbrake = true;
                self.doing = "drifting";
                let into = corner.map_or(steer.signum(), |c| if c.turn == Turn::Left { 1.0 } else { -1.0 });
                // Not full lock: at hairpin speeds that plus the handbrake spins the car.
                steer = self.hb_steer * into;
                throttle = 0.4;
            } else if let Some(c) = corner
                && s_here >= c.start - 2.0
                && s_here < c.end
                && beta > 8.0
            {
                // In the hairpin on the power: the slide carries through (easing off if it grows
                // past ~35°, or it spins).
                throttle = (1.0 - (beta - self.power_beta) / 15.0).clamp(0.2, 1.0);
                brake = 0.0;
                self.doing = "drifting";
            }
        }
        Input { steer, throttle, brake: if handbrake { 0.0 } else { brake }, handbrake }
    }
}

fn left_of(car: &Car) -> Vec3 {
    car.up().cross(car.forward())
}

/// Pure pursuit towards `target` with yaw-rate feedback, through the car's own steering map.
fn pursuit(car: &Car, spec: &Spec, target: Vec3) -> f32 {
    let local = car.rot.inverse() * (target - car.pos);
    let d2 = (local.x * local.x + local.z * local.z).max(1.0);
    let kappa = 2.0 * -local.x / d2;
    let wheelbase = spec.mounts[2].z - spec.mounts[0].z;
    let yaw = car.ang.dot(car.up());
    let delta = (kappa * wheelbase).atan() + 0.12 * (car.speed().max(0.0) * kappa - yaw);
    vehicle::steer_input_for(car, spec, delta)
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
