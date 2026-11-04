//! Handling harness: drives the car sim through standard manoeuvres (step steer, full-lock
//! circle, lift-off, handbrake, slalom, ramp steer to the limit) on flat ground, and along the
//! real stage roads with a simple path-following driver.
//!
//! `cargo test --lib handling -- --ignored --nocapture` prints the full report; the
//! non-ignored tests lock the numbers in with tolerances.

use super::*;
use crate::stage::{self, Surface};
use crate::world;

const DT: f32 = 1.0 / 60.0;

fn settled(surface: Surface) -> (Car, Flat) {
    let g = Flat(0.0, surface);
    let mut car = Car::new(Vec3::new(0.0, 0.9, 0.0), 0.0);
    for _ in 0..90 {
        step(&mut car, &Spec::default(), Input { handbrake: true, ..Input::default() }, &g, DT);
    }
    (car, g)
}

fn left(car: &Car) -> Vec3 {
    car.up().cross(car.forward())
}

/// Throttle/brake towards `target` m/s.
fn pedals(car: &Car, target: f32) -> (f32, f32) {
    let err = target - car.speed();
    ((err * 0.6).clamp(0.0, 1.0), (-err * 0.4).clamp(0.0, 1.0))
}

/// Brings a settled car up to `kmh` in a straight line.
fn up_to_speed(car: &mut Car, spec: &Spec, g: &impl Ground, kmh: f32) {
    let v = kmh / 3.6;
    for _ in 0..(60 * 30) {
        let (t, b) = pedals(car, v);
        step(car, spec, Input { throttle: t, brake: b, ..Input::default() }, g, DT);
        if (car.speed() - v).abs() < 0.3 && car.speed() > v - 0.3 {
            break;
        }
    }
    // A moment to settle.
    for _ in 0..30 {
        let (t, b) = pedals(car, v);
        step(car, spec, Input { throttle: t, brake: b, ..Input::default() }, g, DT);
    }
}

/// Body roll angle (rad) relative to level, and whether any wheel left the ground.
fn roll(car: &Car) -> f32 {
    left(car).y.asin().abs()
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Steady {
    pub yaw: f32,
    pub lat_g: f32,
    pub beta_deg: f32,
    pub radius: f32,
    pub roll_deg: f32,
    pub min_up: f32,
    pub lifted: bool,
    pub speed: f32,
}

/// Step steer at `kmh`, holding speed; averages the last second of `secs`.
pub fn step_steer(surface: Surface, kmh: f32, steer: f32, secs: f32) -> Steady {
    let spec = Spec::default();
    let (mut car, g) = settled(surface);
    up_to_speed(&mut car, &spec, &g, kmh);
    let n = (secs / DT) as usize;
    let tail = 60.min(n);
    let mut acc = Steady { min_up: 1.0, ..Steady::default() };
    for i in 0..n {
        let v0 = car.vel;
        let (t, b) = pedals(&car, kmh / 3.6);
        step(&mut car, &spec, Input { steer, throttle: t, brake: b, ..Input::default() }, &g, DT);
        acc.min_up = acc.min_up.min(car.up().y);
        acc.lifted |= car.wheels.iter().any(|w| !w.contact);
        if i >= n - tail {
            let a = (car.vel - v0) / DT;
            let k = 1.0 / tail as f32;
            let yaw = car.ang.dot(car.up());
            acc.yaw += yaw * k;
            acc.lat_g += a.dot(left(&car)).abs() / 9.81 * k;
            acc.beta_deg += car.vel.dot(left(&car)).atan2(car.speed().max(0.1)).to_degrees() * k;
            acc.roll_deg += roll(&car).to_degrees() * k;
            acc.speed += car.vel.length() * 3.6 * k;
        }
    }
    acc.radius = acc.speed / 3.6 / acc.yaw.abs().max(1e-3);
    acc
}

/// Full-lock circle at walking pace: kerb-to-kerb diameter of the outer front wheel's path (m).
pub fn turning_circle(surface: Surface) -> f32 {
    let spec = Spec::default();
    let (mut car, g) = settled(surface);
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for i in 0..(60 * 40) {
        let (t, b) = pedals(&car, 2.5);
        step(&mut car, &spec, Input { steer: 1.0, throttle: t, brake: b, ..Input::default() }, &g, DT);
        if i > 60 * 8 {
            // Turning left: the outer front wheel is FR, plus its tyre's outer edge.
            let p = car.wheel_pos(&spec, 1) - left(&car) * 0.1;
            let p = Vec2::new(p.x, p.z);
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    let d = hi - lo;
    (d.x + d.y) * 0.5
}

/// Handbrake turn: at `kmh`, full lock and the handbrake for `pull` s, then off the handbrake
/// with the steering held. Returns (heading change after 1.5 s in degrees, peak yaw rate deg/s,
/// min up.y).
pub fn handbrake_turn(surface: Surface, kmh: f32, pull: f32) -> (f32, f32, f32) {
    let spec = Spec::default();
    let (mut car, g) = settled(surface);
    up_to_speed(&mut car, &spec, &g, kmh);
    let f0 = car.forward();
    let (mut peak, mut min_up) = (0.0f32, 1.0f32);
    for i in 0..90 {
        let hb = (i as f32) * DT < pull;
        step(&mut car, &spec, Input { steer: 1.0, handbrake: hb, ..Input::default() }, &g, DT);
        peak = peak.max(car.ang.dot(car.up()).abs());
        min_up = min_up.min(car.up().y);
    }
    let f1 = car.forward();
    let turned = f0.cross(f1).y.atan2(f0.dot(f1)).to_degrees();
    (turned, peak.to_degrees(), min_up)
}

/// The same manoeuvre without the handbrake (the reference for how much it adds).
pub fn lift_off(surface: Surface, kmh: f32, steer: f32) -> (f32, f32) {
    let spec = Spec::default();
    let (mut car, g) = settled(surface);
    up_to_speed(&mut car, &spec, &g, kmh);
    // Steady on throttle in the corner, then lift.
    for _ in 0..90 {
        let (t, b) = pedals(&car, kmh / 3.6);
        step(&mut car, &spec, Input { steer, throttle: t, brake: b, ..Input::default() }, &g, DT);
    }
    let yaw0 = car.ang.dot(car.up());
    let mut peak = yaw0;
    for _ in 0..45 {
        step(&mut car, &spec, Input { steer, ..Input::default() }, &g, DT);
        peak = peak.max(car.ang.dot(car.up()));
    }
    (yaw0.to_degrees(), peak.to_degrees())
}

/// Sine slalom at `kmh`: steer amplitude `amp`, `hz`. Returns (peak lat g, peak roll deg, min up.y).
pub fn slalom(surface: Surface, kmh: f32, amp: f32, hz: f32) -> (f32, f32, f32) {
    let spec = Spec::default();
    let (mut car, g) = settled(surface);
    up_to_speed(&mut car, &spec, &g, kmh);
    let (mut lat, mut rl, mut up) = (0.0f32, 0.0f32, 1.0f32);
    for i in 0..(60 * 8) {
        let v0 = car.vel;
        let t = i as f32 * DT;
        let (th, b) = pedals(&car, kmh / 3.6);
        step(&mut car, &spec, Input { steer: amp * (std::f32::consts::TAU * hz * t).sin(), throttle: th, brake: b, ..Input::default() }, &g, DT);
        lat = lat.max(((car.vel - v0) / DT).dot(left(&car)).abs() / 9.81);
        rl = rl.max(roll(&car).to_degrees());
        up = up.min(car.up().y);
    }
    (lat, rl, up)
}

/// Ramp steer at constant speed: steering input 0 → 1 over `secs`. Returns (peak lat g, peak
/// roll deg, min up.y, rolled over).
pub fn ramp_steer(surface: Surface, kmh: f32, secs: f32) -> (f32, f32, f32, bool) {
    let spec = Spec::default();
    let (mut car, g) = settled(surface);
    up_to_speed(&mut car, &spec, &g, kmh);
    let (mut lat, mut rl, mut up) = (0.0f32, 0.0f32, 1.0f32);
    let n = (secs / DT) as usize;
    for i in 0..n + 120 {
        let v0 = car.vel;
        let s = (i as f32 / n as f32).min(1.0);
        let (th, b) = pedals(&car, kmh / 3.6);
        step(&mut car, &spec, Input { steer: s, throttle: th, brake: b, ..Input::default() }, &g, DT);
        // Lateral g from the velocity change (smoothed by the averaging of a 1/60 s step).
        lat = lat.max(((car.vel - v0) / DT).dot(left(&car)).abs() / 9.81);
        rl = rl.max(roll(&car).to_degrees());
        up = up.min(car.up().y);
    }
    (lat, rl, up, up < 0.3)
}

/// Constant-radius circle (path follower), speed ramped up until the car can't hold the line.
/// Returns (max steady lat g, peak roll deg, min up.y, rolled over).
pub fn circle_ramp(surface: Surface, radius: f32) -> (f32, f32, f32, bool) {
    let spec = Spec::default();
    let (mut car, g) = settled(surface);
    // Start on the circle centred at (−radius, 0) (to the car's left: heading +Z, left = +X).
    let centre = Vec2::new(radius, 0.0);
    let (mut best, mut rl, mut up) = (0.0f32, 0.0f32, 1.0f32);
    let mut lat_avg = 0.0f32;
    for i in 0..(60 * 60) {
        let t = i as f32 * DT;
        let target = 3.0 + t * 0.6;
        let p = Vec2::new(car.pos.x, car.pos.z);
        let rel = p - centre;
        let ang = rel.y.atan2(rel.x);
        // Look ahead along the circle (counter-clockwise seen from above = turning left here).
        let la = (car.speed().max(4.0) * 0.6 / radius).min(0.8);
        let dir = if (Vec2::new(-rel.y, rel.x)).dot(Vec2::new(car.forward().x, car.forward().z)) > 0.0 { 1.0 } else { -1.0 };
        let a2 = ang + dir * la;
        let tp = centre + Vec2::new(a2.cos(), a2.sin()) * radius;
        let steer = pursuit(&car, &spec, Vec3::new(tp.x, car.pos.y, tp.y));
        let (th, b) = pedals(&car, target);
        let v0 = car.vel;
        step(&mut car, &spec, Input { steer, throttle: th, brake: b, ..Input::default() }, &g, DT);
        let lat = ((car.vel - v0) / DT).dot(left(&car)).abs() / 9.81;
        lat_avg += (lat - lat_avg) * 0.05;
        let off = (rel.length() - radius).abs();
        if off < 2.0 {
            best = best.max(lat_avg);
        }
        rl = rl.max(roll(&car).to_degrees());
        up = up.min(car.up().y);
        if off > 15.0 {
            break;
        }
    }
    (best, rl, up, up < 0.3)
}

/// Pure-pursuit steering input towards a world point, with yaw-rate feedback (a driver adds
/// lock when the car turns less than asked: understeer, a slide, a slippery surface).
fn pursuit(car: &Car, spec: &Spec, target: Vec3) -> f32 {
    let local = car.rot.inverse() * (target - car.pos);
    let lat_left = -local.x;
    let d2 = (local.x * local.x + local.z * local.z).max(1.0);
    let kappa = 2.0 * lat_left / d2;
    let wheelbase = spec.mounts[2].z - spec.mounts[0].z;
    let v = car.speed().max(0.0);
    let yaw = car.ang.dot(car.up());
    let delta = (kappa * wheelbase).atan() + 0.12 * (v * kappa - yaw);
    // Wheel angle → input through the car's own mapping.
    steer_input_for(car, spec, delta)
}

#[derive(Debug, Default, Clone)]
pub struct StageReport {
    pub id: u32,
    pub name: String,
    pub length: f32,
    pub time: f32,
    pub done: bool,
    /// Upside down (each one recovered), with what happened in the 3 s before.
    pub rollovers: Vec<String>,
    /// The car was wrecked (damage), which ends the run.
    pub wrecked: Option<String>,
    pub offs: u32,
    pub hits: u32,
    pub damage: f32,
    /// Saplings, posts and signs knocked down.
    pub broken: usize,
    pub recovers: u32,
    /// Gear changes, and changes reversed within 0.7 s (the box hunting).
    pub shifts: u32,
    pub hunts: u32,
}

/// Drives the road centreline of a stage with a path-following driver at `pace` (share of the
/// surface's grip used for cornering).
pub fn drive_stage(def: &stage::StageDef, pace: f32, max_secs: f32) -> StageReport {
    drive_stage_traced(def, pace, max_secs, None, None)
}

/// A keyboard player: on/off keys, decided every 0.1 s (reaction time), steering through the
/// game's keyboard ramp. `events` is the key timeline (s from the start, key, down).
#[derive(Default, Debug)]
pub struct Keys {
    steer: f32,
    key: f32,
    gas: bool,
    brake: bool,
    pub events: Vec<(f32, &'static str, bool)>,
}

pub fn drive_stage_traced(def: &stage::StageDef, pace: f32, max_secs: f32, trace: Option<(f32, f32)>, mut keys: Option<&mut Keys>) -> StageReport {
    let w = world::generate(def);
    let road = &w.road;
    let spec = Spec::default();
    let (p, h) = road.start();
    let mut car = Car::new(p + Vec3::Y * 0.9, h);
    let mut rep = StageReport { id: def.id, name: def.name.clone(), length: road.length(), ..Default::default() };
    let mut progress = 0.0f32;
    let mut upside = 0.0f32;
    let mut stuck = 0.0f32;
    let mut off_road = false;
    // Recent events for classifying a roll: (time, what).
    let mut log: Vec<(f32, String)> = Vec::new();
    let mut last_impacts = 0.0;
    let mut last_broken = 0;
    let mut last_shift: Option<(f32, bool)> = None;
    let mut t = 0.0;
    for _ in 0..60 {
        step(&mut car, &spec, Input { handbrake: true, ..Input::default() }, &w, DT);
    }
    while t < max_secs {
        let pos2 = Vec2::new(car.pos.x, car.pos.z);
        let rp = road.nearest(pos2, 40.0);
        if let Some(rp) = rp {
            if rp.s > progress && rp.s < progress + 40.0 {
                progress = rp.s;
            }
            let off = rp.lateral.abs() > road.half_width(&rp) + 0.3;
            if off && !off_road {
                rep.offs += 1;
                log.push((t, format!("off road at {:.0} m (lat {:.1})", rp.s, rp.lateral)));
            }
            off_road = off;
        }
        if progress >= road.length() - 4.0 {
            rep.done = true;
            break;
        }
        let v = car.speed().max(0.0);
        // Steering: pure pursuit on the centreline, from the car's own station.
        let s_here = rp.map_or(progress, |r| r.s);
        let look = (v * 0.7).clamp(6.0, 30.0);
        let target = road.pts[road.station(s_here + look)];
        let steer = pursuit(&car, &spec, target);
        // Speed: the slowest corner ahead within braking distance.
        let i0 = road.station(s_here);
        let mut vt = 30.0f32;
        let horizon = (v * v / 6.0 + 30.0) / stage::STEP;
        for k in 0..horizon as usize {
            let i = (i0 + k).min(road.len() - 1);
            let (mu, _) = road.surface[i].grip();
            let a_lat = pace * mu * 9.81;
            let a_brk = 0.55 * mu * 9.81;
            let c = road.curv[i].abs().max(1e-4);
            let vc = (a_lat / c).sqrt();
            let d = k as f32 * stage::STEP;
            vt = vt.min((vc * vc + 2.0 * a_brk * d).sqrt());
        }
        // Off the road: slow down and get back on.
        let vt = if off_road { vt.min(12.0) } else { vt };
        let (th, b) = pedals(&car, vt);
        // A driver feathers the throttle in a big slide.
        let beta = car.vel.dot(left(&car)).atan2(car.speed().abs().max(1.0)).abs().to_degrees();
        let th = th * (1.0 - (beta - 12.0) / 20.0).clamp(0.2, 1.0);
        let mut input = Input { steer, throttle: th, brake: b, ..Input::default() };
        if let Some(k) = keys.as_deref_mut() {
            if ((t / DT).round() as u32).is_multiple_of(6) {
                // A player lets go (and lifts) when the nose is already well into the turn: the
                // steering self-aligns and catches the slide.
                let nose_in = -car.vel.dot(left(&car)).atan2(car.speed().abs().max(1.0)).to_degrees() * car.ang.dot(car.up()).signum();
                let _ = nose_in;
                let key = if steer > 0.3 { 1.0 } else if steer < -0.3 { -1.0 } else { 0.0 };
                if key != k.key {
                    if k.key > 0.0 {
                        k.events.push((t, "KeyA", false));
                    } else if k.key < 0.0 {
                        k.events.push((t, "KeyD", false));
                    }
                    if key > 0.0 {
                        k.events.push((t, "KeyA", true));
                    } else if key < 0.0 {
                        k.events.push((t, "KeyD", true));
                    }
                    k.key = key;
                }
                let (gas, brake) = (th > 0.3, b > 0.3);
                if gas != k.gas {
                    k.events.push((t, "KeyW", gas));
                    k.gas = gas;
                }
                if brake != k.brake {
                    k.events.push((t, "KeyS", brake));
                    k.brake = brake;
                }
            }
            k.steer = crate::drive::keyboard_steer(k.steer, k.key, car.speed(), DT);
            input = Input { steer: k.steer, throttle: k.gas as u8 as f32, brake: k.brake as u8 as f32, ..Input::default() };
        }
        let pre = car.damage.worst();
        let gear = car.gear;
        step(&mut car, &spec, input, &w, DT);
        t += DT;
        if car.gear != gear && gear > 0 && car.gear > 0 {
            rep.shifts += 1;
            if let Some((when, dir)) = last_shift
                && t - when < 0.7
                && dir != (car.gear > gear)
            {
                rep.hunts += 1;
                if std::env::var("HUNTS").is_ok() {
                    println!("  hunt at {:.0} m: {} → {} after {:.2} s, {:.0} km/h, throttle {:.1} brake {:.1} hb {} rpm {:.0} spin-rpm slip {:.2?}", progress, gear, car.gear, t - when, car.speed() * 3.6, input.throttle, input.brake, input.handbrake, car.rpm, car.wheels.iter().map(|w| w.slip_ratio).collect::<Vec<_>>());
                }
            }
            last_shift = Some((t, car.gear > gear));
        }
        if let Some((a, b)) = trace
            && progress >= a
            && progress <= b
        {
            let wh = &car.wheels;
            let ry = road.pts[road.station(progress)].y;
            let lat = road.nearest(Vec2::new(car.pos.x, car.pos.z), 40.0).map_or(99.0, |r| r.lateral);
            let mut obs = Vec::new();
            w.obstacles(Vec2::new(car.pos.x, car.pos.z), 8.0, &mut obs);
            let near = obs.iter().map(|o| o.pos.distance(Vec2::new(car.pos.x, car.pos.z)) - o.radius).fold(99.0f32, f32::min);
            print!("lat {lat:5.1} tree {near:4.1} ");
            println!(
                "s {:6.1} β {:5.1} yaw {:5.1} v {:5.1} vt {:5.1} in {:5.2} δ {:5.3} want {:5.2} up {:5.2} fwd.y {:5.2} vy {:5.1} h {:5.2} gnd {:5.2} road {:5.2} cont {}{}{}{} comp {:.2} {:.2} {:.2} {:.2} body {} dmg {:.2}{}",
                progress, car.vel.dot(left(&car)).atan2(car.speed()).to_degrees(), car.ang.dot(car.up()).to_degrees(), car.speed() * 3.6, vt * 3.6, input.steer, car.steer, steer, car.up().y, car.forward().y, car.vel.y,
                car.pos.y - w.height(car.pos.x, car.pos.z), w.height(car.pos.x, car.pos.z), ry,
                wh[0].contact as u8, wh[1].contact as u8, wh[2].contact as u8, wh[3].contact as u8,
                wh[0].compression, wh[1].compression, wh[2].compression, wh[3].compression,
                car.body_contact as u8, car.damage.worst(), if car.damage.worst() > pre + 0.01 { "  <== HIT" } else { "" }
            );
        }
        if car.impacts > last_impacts + 2.0 {
            rep.hits += 1;
            log.push((t, format!("obstacle hit {:.1} m/s at {:.0} m", car.impacts - last_impacts, progress)));
        }
        last_impacts = car.impacts;
        if car.broken.len() > last_broken {
            log.push((t, format!("snapped obstacle hit at {:.0} m ({:.0} km/h)", progress, car.speed() * 3.6)));
            last_broken = car.broken.len();
        }
        let note = |log: &mut Vec<(f32, String)>, what: String| {
            if !log.iter().any(|l| l.1 == what && l.0 > t - 3.0) {
                log.push((t, what));
            }
        };
        if car.brush > 0.0 {
            note(&mut log, "in brush".into());
        }
        if car.body_contact {
            let tilt = car.up().y.acos().to_degrees();
            note(&mut log, format!("body on ground (tilt {:.0}°)", (tilt / 15.0).round() * 15.0));
        }
        if car.wheels.iter().all(|w| !w.contact) {
            note(&mut log, "airborne".into());
        }
        // Steep ground under the car (a bank or ditch).
        let n = w.normal(car.pos.x, car.pos.z);
        if n.y < 0.85 {
            note(&mut log, format!("slope {:.0}°", (n.y.acos().to_degrees() / 5.0).round() * 5.0));
        }
        upside = if car.up().y < 0.2 { upside + DT } else { 0.0 };
        stuck = if car.speed().abs() < 1.0 { stuck + DT } else { 0.0 };
        let wrecked = car.damage.wrecked();
        if upside > 0.5 || stuck > 6.0 || wrecked {
            let s = progress;
            let i = road.station(s);
            let why: Vec<String> = log.iter().filter(|l| l.0 > t - 3.0).map(|l| l.1.clone()).collect();
            if upside > 0.5 {
                let rp2 = road.nearest(Vec2::new(car.pos.x, car.pos.z), 40.0);
                // Where it rolled: 2 m or more past the road's edge counts as off the road.
                let away = rp2.is_none_or(|r| r.lateral.abs() > road.half_width(&r) + 2.0);
                rep.rollovers.push(format!(
                    "{:.0} m curv {:.3} lat {:.1}{} [{}]",
                    s,
                    road.curv[i],
                    rp2.map_or(99.0, |r| r.lateral),
                    if away { " OFF THE ROAD" } else { "" },
                    why.join("; ")
                ));
            }
            if wrecked {
                rep.wrecked = Some(format!("at {:.0} m after {} hits [{}]", s, car.damage.hits, why.join("; ")));
                break;
            }
            rep.recovers += 1;
            let i = road.station(progress + 4.0);
            let d = road.dir[i];
            recover(&mut car, road.pts[i], d.x.atan2(d.y));
            upside = 0.0;
            stuck = 0.0;
            log.clear();
        }
    }
    rep.time = t;
    rep.damage = car.damage.worst();
    rep.broken = car.broken.len();
    rep.hits = rep.hits.max(car.damage.hits);
    rep
}

#[test]
#[ignore]
fn report() {
    println!("\n== step steer (full input / half input), steady state after 4 s ==");
    for surf in [Surface::Tarmac, Surface::Gravel, Surface::Snow, Surface::Mud] {
        for kmh in [30.0, 60.0, 90.0] {
            for steer in [0.25, 0.5, 1.0] {
                let s = step_steer(surf, kmh, steer, 4.0);
                println!(
                    "{surf:?} {kmh:3.0} km/h steer {steer:.2}: yaw {:5.1}°/s  lat {:.2} g  β {:5.1}°  R {:6.1} m  roll {:4.1}°  v {:5.1}  min_up {:.2} lift {}",
                    s.yaw.to_degrees(),
                    s.lat_g,
                    s.beta_deg,
                    s.radius,
                    s.roll_deg,
                    s.speed,
                    s.min_up,
                    s.lifted
                );
            }
        }
    }
    println!("\n== turning circle (kerb to kerb) ==");
    for surf in [Surface::Tarmac, Surface::Gravel] {
        println!("{surf:?}: {:.1} m", turning_circle(surf));
    }
    println!("\n== handbrake turn (50 km/h, 0.6 s pull) vs. same without ==");
    for surf in [Surface::Gravel, Surface::Tarmac, Surface::Snow] {
        let hb = handbrake_turn(surf, 50.0, 0.6);
        let no = handbrake_turn(surf, 50.0, 0.0);
        println!("{surf:?}: with {:.0}° (peak {:.0}°/s, min up {:.2}), without {:.0}° (peak {:.0}°/s)", hb.0, hb.1, hb.2, no.0, no.1);
    }
    println!("\n== lift-off (70 km/h, steer 0.4): steady yaw → peak after lifting ==");
    for surf in [Surface::Gravel, Surface::Tarmac] {
        let (a, b) = lift_off(surf, 70.0, 0.4);
        println!("{surf:?}: {a:.1}°/s → {b:.1}°/s");
    }
    println!("\n== slalom (60 km/h, amp 0.6, 0.6 Hz) ==");
    for surf in [Surface::Gravel, Surface::Tarmac] {
        let (l, r, u) = slalom(surf, 60.0, 0.6, 0.6);
        println!("{surf:?}: peak lat {l:.2} g, roll {r:.1}°, min up {u:.2}");
    }
    println!("\n== ramp steer 0→1 over 6 s ==");
    for surf in [Surface::Tarmac, Surface::Gravel] {
        for kmh in [50.0, 80.0, 110.0] {
            let (l, r, u, rolled) = ramp_steer(surf, kmh, 6.0);
            println!("{surf:?} {kmh:3.0}: peak lat {l:.2} g, roll {r:.1}°, min up {u:.2}, ROLLED {rolled}");
        }
    }
    println!("\n== 0-100 km/h ==");
    for surf in [Surface::Tarmac, Surface::Gravel, Surface::Snow] {
        let spec = Spec::default();
        let (mut car, g) = settled(surf);
        let mut t = 0.0;
        while car.speed() < 100.0 / 3.6 && t < 30.0 {
            step(&mut car, &spec, Input { throttle: 1.0, ..Input::default() }, &g, DT);
            t += DT;
        }
        println!("{surf:?}: {t:.1} s");
    }
    {
        // Cost of one 60 Hz step (8 substeps), this build.
        let spec = Spec::default();
        let (mut car, g) = settled(Surface::Gravel);
        let n = 6000;
        let t0 = std::time::Instant::now();
        for i in 0..n {
            step(&mut car, &spec, Input { throttle: 0.7, steer: ((i as f32) * 0.01).sin(), ..Input::default() }, &g, DT);
        }
        println!("\n== cost: {:.1} µs per step ==", t0.elapsed().as_secs_f64() * 1e6 / n as f64);
    }
    println!("\n== constant radius 40 m, speed ramp ==");
    for surf in [Surface::Tarmac, Surface::Gravel] {
        let (l, r, u, rolled) = circle_ramp(surf, 40.0);
        println!("{surf:?}: max lat {l:.2} g, roll {r:.1}°, min up {u:.2}, ROLLED {rolled}");
    }
}

#[test]
#[ignore]
fn stage_report() {
    let pace: f32 = std::env::var("PACE").ok().and_then(|s| s.parse().ok()).unwrap_or(0.55);
    let only: Option<u32> = std::env::var("STAGE").ok().and_then(|s| s.parse().ok());
    let (mut total, mut done, mut wrecks, mut shifts, mut hunts) = (0, 0, 0, 0, 0);
    for def in stage::stages() {
        if only.is_some_and(|o| o != def.id) {
            continue;
        }
        let trace = std::env::var("TRACE").ok().and_then(|s| {
            let (a, b) = s.split_once(',')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        });
        let mut keys = Keys::default();
        let use_keys = std::env::var("KEYS").is_ok();
        let r = drive_stage_traced(&def, pace, 900.0, trace, use_keys.then_some(&mut keys));
        if let Ok(path) = std::env::var("KEYS_OUT") {
            let lines: Vec<String> = keys.events.iter().map(|(t, k, d)| format!("{t:.3},{k},{}", *d as u8)).collect();
            std::fs::write(format!("{path}-{}.csv", def.id), lines.join("\n")).unwrap();
        }
        total += r.rollovers.len();
        done += r.done as usize;
        wrecks += r.wrecked.is_some() as usize;
        shifts += r.shifts;
        hunts += r.hunts;
        println!(
            "stage {:2} {:24} {:5.0} m  {:5.0} s  done {}  rolls {}  wrecked {}  offs {}  hits {}  broken {}  recovers {}  damage {:.2}",
            r.id, r.name, r.length, r.time, r.done, r.rollovers.len(), r.wrecked.is_some(), r.offs, r.hits, r.broken, r.recovers, r.damage
        );
        for x in &r.rollovers {
            println!("    roll: {x}");
        }
        if let Some(x) = &r.wrecked {
            println!("    wrecked: {x}");
        }
    }
    println!("pace {pace}: finished {done}, rollovers {total}, wrecked {wrecks}, shifts {shifts}, hunting {hunts}");
}

#[test]
#[ignore]
fn road_profile() {
    let id: u32 = std::env::var("STAGE").ok().and_then(|s| s.parse().ok()).unwrap_or(8);
    let (a, b): (f32, f32) = std::env::var("TRACE").ok().and_then(|s| {
        let (a, b) = s.split_once(',')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    }).unwrap_or((700.0, 800.0));
    let def = stage::stages().into_iter().find(|d| d.id == id).unwrap();
    let w = world::generate(&def);
    let r = &w.road;
    let mut s = a;
    while s <= b {
        let i = r.station(s);
        let p = r.pts[i];
        let d = r.dir[i];
        let lft = Vec2::new(d.y, -d.x);
        let row: Vec<String> = [-6.0f32, -4.5, -3.0, -1.5, 0.0, 1.5, 3.0, 4.5, 6.0]
            .iter()
            .map(|o| {
                let q = Vec2::new(p.x, p.z) + lft * *o;
                format!("{:5.2}", w.height(q.x, q.y) - p.y)
            })
            .collect();
        println!("s {s:5.0} y {:6.2} width {:.1} bank {:.3} curv {:.4} surf {:?} | right→left {}", p.y, r.width[i], r.bank[i], r.curv[i], r.surface[i], row.join(" "));
        s += 10.0;
    }
}

// ---------------------------------------------------------------- locked-in behaviour

/// Best steady lateral g at `kmh` over a few steering inputs, and the g at full input.
fn best_lateral(surface: Surface, kmh: f32) -> (f32, f32) {
    let mut best = 0.0f32;
    let mut full = 0.0;
    for steer in [0.25, 0.4, 0.6, 0.8, 1.0] {
        let s = step_steer(surface, kmh, steer, 4.0);
        assert!(s.min_up > 0.95, "{surface:?} {kmh} km/h steer {steer}: tipped to up.y {}", s.min_up);
        best = best.max(s.lat_g);
        if steer == 1.0 {
            full = s.lat_g;
        }
    }
    (best, full)
}

#[test]
fn turning_circle_is_a_rally_cars() {
    for surf in [Surface::Tarmac, Surface::Gravel] {
        let d = turning_circle(surf);
        // (A 4WD with locked axles scrubs in a full-lock circle: ~11.5–12 m.)
        assert!((10.0..12.5).contains(&d), "{surf:?}: {d:.1} m kerb to kerb");
    }
}

#[test]
fn lateral_grip_per_surface() {
    // (surface, min g, max g): tarmac ~1.0, gravel ~0.75, mud ~0.5, snow on studs ~0.4.
    for (surf, lo, hi) in [(Surface::Tarmac, 0.92, 1.15), (Surface::Gravel, 0.66, 0.88), (Surface::Mud, 0.42, 0.6), (Surface::Snow, 0.33, 0.5)] {
        let (best, _) = best_lateral(surf, 60.0);
        assert!((lo..hi).contains(&best), "{surf:?}: {best:.2} g");
    }
}

#[test]
fn full_input_is_useful_at_speed() {
    // Keyboard steering ends at full input: that must take the corner, not plough the fronts.
    for surf in [Surface::Tarmac, Surface::Gravel] {
        for kmh in [60.0, 90.0] {
            let (best, full) = best_lateral(surf, kmh);
            assert!(full > best * 0.85, "{surf:?} {kmh} km/h: full input {full:.2} g vs best {best:.2} g");
        }
    }
}

#[test]
fn slides_never_rolls_on_flat_ground() {
    for surf in [Surface::Tarmac, Surface::Gravel] {
        for kmh in [50.0, 80.0, 110.0] {
            let (lat, roll, up, rolled) = ramp_steer(surf, kmh, 6.0);
            assert!(!rolled && up > 0.95 && roll < 5.0, "{surf:?} {kmh} km/h: rolled {rolled}, roll {roll:.1}°, up {up:.2}, {lat:.2} g");
        }
        let (lat, roll, up, rolled) = circle_ramp(surf, 40.0);
        assert!(!rolled && up > 0.95 && roll < 5.0, "{surf:?} 40 m circle: roll {roll:.1}°, up {up:.2}, {lat:.2} g");
        let (_, roll, up) = slalom(surf, 60.0, 0.6, 0.6);
        assert!(up > 0.95 && roll < 5.0, "{surf:?} slalom: roll {roll:.1}°");
    }
}

#[test]
fn handbrake_and_lift_rotate_the_car() {
    for surf in [Surface::Gravel, Surface::Tarmac, Surface::Snow] {
        let (with, _, up) = handbrake_turn(surf, 50.0, 0.6);
        let (without, _, _) = handbrake_turn(surf, 50.0, 0.0);
        assert!(with > without * 1.4 && with - without > 20.0, "{surf:?}: handbrake {with:.0}° vs {without:.0}°");
        assert!(up > 0.9, "{surf:?}: handbrake turn tipped to {up:.2}");
    }
    // Lifting rotates the car: clearly on gravel, mildly on tarmac (a 4WD on grip; the steering,
    // measured from the direction of travel, unwinds a little as the nose comes in).
    for (surf, more) in [(Surface::Gravel, 1.15), (Surface::Tarmac, 1.07)] {
        let (steady, peak) = lift_off(surf, 70.0, 0.4);
        assert!(peak > steady * more, "{surf:?}: lift-off {steady:.1} → {peak:.1}°/s");
    }
}

/// A smooth crest `height` m high and `length` m long across the road, at z = 60.
struct Crest(f32, f32);

impl Ground for Crest {
    fn height(&self, _: f32, z: f32) -> f32 {
        let x = (z - 60.0) / (self.1 * 0.5);
        if x.abs() < 1.0 { self.0 * 0.5 * (1.0 + (x * std::f32::consts::PI).cos()) } else { 0.0 }
    }
    fn normal(&self, x: f32, z: f32) -> Vec3 {
        let e = 0.05;
        let dz = (self.height(x, z + e) - self.height(x, z - e)) / (2.0 * e);
        Vec3::new(0.0, 1.0, -dz).normalize()
    }
    fn surface(&self, _: f32, _: f32) -> Surface {
        Surface::Gravel
    }
    fn obstacles(&self, _: Vec2, _: f32, out: &mut Vec<Obstacle>) {
        out.clear();
    }
}

#[test]
fn crests_land_on_the_wheels() {
    let spec = Spec::default();
    for (kmh, h, len) in [(80.0, 1.2, 30.0), (110.0, 1.0, 30.0), (70.0, 1.5, 24.0)] {
        let g = Crest(h, len);
        let mut car = Car::new(Vec3::new(0.0, 0.9, 0.0), 0.0);
        for _ in 0..60 {
            step(&mut car, &spec, Input { handbrake: true, ..Input::default() }, &g, DT);
        }
        let v = kmh / 3.6;
        car.vel = car.forward() * v;
        for wh in &mut car.wheels {
            wh.spin = v / spec.wheel_radius;
        }
        let mut min_up = 1.0f32;
        for _ in 0..(60 * 4) {
            let (t, b) = pedals(&car, v);
            step(&mut car, &spec, Input { throttle: t, brake: b, ..Input::default() }, &g, DT);
            min_up = min_up.min(car.up().y);
        }
        assert!(min_up > 0.85, "{kmh} km/h over a {h} m crest: tipped to up.y {min_up:.2}");
        assert!(car.damage.worst() < 0.1, "{kmh} km/h over a {h} m crest: damage {:?}", car.damage);
        assert!(car.speed() > v * 0.8 && car.pos.z > 60.0 + len * 0.5 && car.pos.x.abs() < 3.0, "{kmh} km/h over a {h} m crest: {:.1} m/s at {:?}", car.speed(), car.pos);
    }
}

/// Flat ground with a 0.45 m step (a bank or a kerb face) from x = 8 on.
struct Step;

impl Ground for Step {
    fn height(&self, x: f32, _: f32) -> f32 {
        if x > 8.0 { 0.45 } else { 0.0 }
    }
    fn normal(&self, _: f32, _: f32) -> Vec3 {
        Vec3::Y
    }
    fn surface(&self, _: f32, _: f32) -> Surface {
        Surface::Gravel
    }
    fn obstacles(&self, _: Vec2, _: f32, out: &mut Vec<Obstacle>) {
        out.clear();
    }
}

#[test]
fn sliding_sideways_into_a_bank_trips_it() {
    // The car slides sideways (its left side leading) into a step: that may roll it. The same
    // slide on open ground must not.
    let spec = Spec::default();
    let run = |ground: &dyn Ground, kmh: f32| {
        let mut car = Car::new(Vec3::new(0.0, 0.9, 0.0), 0.0);
        for _ in 0..60 {
            step(&mut car, &spec, Input { handbrake: true, ..Input::default() }, &DynGround(ground), DT);
        }
        // Heading 0 faces +Z, so its left is +X: slide towards the step.
        car.vel = Vec3::X * kmh / 3.6;
        let mut min_up = 1.0f32;
        for _ in 0..(60 * 3) {
            step(&mut car, &spec, Input::default(), &DynGround(ground), DT);
            min_up = min_up.min(car.up().y);
        }
        min_up
    };
    let open = run(&Flat(0.0, Surface::Gravel), 50.0);
    assert!(open > 0.9, "a sideways slide on open gravel tipped to {open:.2}");
    let tripped = run(&Step, 50.0);
    assert!(tripped < 0.5, "sliding into a bank at 50 km/h only tipped to {tripped:.2}");
}

struct DynGround<'a>(&'a dyn Ground);

impl Ground for DynGround<'_> {
    fn height(&self, x: f32, z: f32) -> f32 {
        self.0.height(x, z)
    }
    fn normal(&self, x: f32, z: f32) -> Vec3 {
        self.0.normal(x, z)
    }
    fn surface(&self, x: f32, z: f32) -> Surface {
        self.0.surface(x, z)
    }
    fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
        self.0.obstacles(p, r, out)
    }
}

#[test]
fn drives_every_stage_without_rolling() {
    // A path-following driver on the road centreline. At a steady pace (55 % of the surface's
    // grip in corners) every stage finishes clean; pushing (70 %) it may go off and hit
    // something, but rarely, and a roll only ever follows a hit (a trip), never a corner.
    let (mut finished, mut rolls, mut wrecks) = (0, 0, 0);
    for def in stage::stages() {
        let r = drive_stage(&def, 0.55, 900.0);
        assert!(r.done && r.rollovers.is_empty() && r.wrecked.is_none() && r.recovers == 0, "stage {} {} at a steady pace: {r:?}", def.id, def.name);
        let p = drive_stage(&def, 0.7, 900.0);
        finished += p.done as u32;
        rolls += p.rollovers.len();
        wrecks += p.wrecked.is_some() as u32;
        for x in &p.rollovers {
            assert!(x.contains("obstacle hit"), "stage {} {}: rolled without a trip: {x}", def.id, def.name);
        }
    }
    assert!(finished >= 16 && rolls <= 3 && wrecks <= 4, "pushing: {finished}/20 finished, {rolls} rollovers, {wrecks} wrecked");
}


// ---------------------------------------------------------------- the way a player drives

/// Keys → input the way the game does it (`drive::keyboard_steer`).
fn key_steer(car: &Car, prev: f32, key: f32) -> f32 {
    crate::drive::keyboard_steer(prev, key, car.speed(), DT)
}

fn yaw(car: &Car) -> f32 {
    car.ang.dot(car.up()).to_degrees()
}

#[derive(Debug, Default)]
pub struct KeyStep {
    /// How fast the path turns at the end of the hold (lateral g / speed, °/s), and the swing of
    /// the sideslip over the last second of the hold (°).
    pub path: f32,
    pub beta_swing: f32,
    /// Steady yaw rate (°/s), time to 90 % of it (s), overshoot (share), yaw gain (°/s per °
    /// of road-wheel steer), sideslip in the steady turn (°).
    pub steady: f32,
    pub t90: f32,
    pub overshoot: f32,
    pub gain: f32,
    pub beta: f32,
    /// After letting go: seconds to |yaw| < 2°/s, yaw sign changes (wobble), worst |β| (°).
    pub settle: f32,
    pub wobble: u32,
    pub beta_after: f32,
}

/// Holds a steering key for `hold` s at `kmh` (speed held with the pedals), then lets go.
pub fn key_step(surface: Surface, kmh: f32, hold: f32) -> KeyStep {
    let spec = Spec::default();
    let (mut car, g) = settled(surface);
    up_to_speed(&mut car, &spec, &g, kmh);
    let mut s = 0.0;
    let mut trace = Vec::new();
    let n = (hold / DT) as usize;
    let mut path_acc = 0.0f32;
    for i in 0..n + 240 {
        let key = if i < n { 1.0 } else { 0.0 };
        let v0 = car.vel;
        s = key_steer(&car, s, key);
        let (t, b) = pedals(&car, kmh / 3.6);
        step(&mut car, &spec, Input { steer: s, throttle: t, brake: b, ..Input::default() }, &g, DT);
        let beta = car.vel.dot(left(&car)).atan2(car.speed().max(0.5)).to_degrees();
        trace.push((yaw(&car), car.steer.to_degrees(), beta));
        if i < n && i + 30 >= n {
            path_acc += ((car.vel - v0) / DT).dot(left(&car)) / car.vel.length().max(1.0);
        }
    }
    let tail = &trace[n.saturating_sub(30)..n];
    let steady = tail.iter().map(|t| t.0).sum::<f32>() / tail.len() as f32;
    let mut r = KeyStep { steady, path: (path_acc / 30.0).to_degrees(), ..Default::default() };
    let last = &trace[n.saturating_sub(60)..n];
    r.beta_swing = last.iter().map(|t| t.2).fold(f32::MIN, f32::max) - last.iter().map(|t| t.2).fold(f32::MAX, f32::min);
    r.t90 = trace.iter().position(|t| t.0 >= 0.9 * steady).map_or(99.0, |i| i as f32 * DT);
    r.overshoot = trace[..n].iter().map(|t| t.0).fold(0.0f32, f32::max) / steady.max(0.1) - 1.0;
    r.gain = steady / tail.iter().map(|t| t.1).sum::<f32>().max(1e-3) * tail.len() as f32;
    r.beta = tail.iter().map(|t| t.2).sum::<f32>() / tail.len() as f32;
    let after = &trace[n..];
    r.settle = after.iter().rposition(|t| t.0.abs() > 2.0).map_or(0.0, |i| (i + 1) as f32 * DT);
    let mut last = 0.0f32;
    for t in after {
        if t.0.abs() > 1.0 {
            if last != 0.0 && t.0.signum() != last {
                r.wobble += 1;
            }
            last = t.0.signum();
        }
    }
    r.beta_after = after.iter().map(|t| t.2.abs()).fold(0.0, f32::max);
    r
}

/// Heading change (°) from one tap of `secs` at `kmh`, measured 1.5 s later.
pub fn key_tap(surface: Surface, kmh: f32, secs: f32) -> f32 {
    let spec = Spec::default();
    let (mut car, g) = settled(surface);
    up_to_speed(&mut car, &spec, &g, kmh);
    let f0 = car.forward();
    let mut s = 0.0;
    for i in 0..90 {
        let key = if (i as f32) * DT < secs { 1.0 } else { 0.0 };
        s = key_steer(&car, s, key);
        let (t, b) = pedals(&car, kmh / 3.6);
        step(&mut car, &spec, Input { steer: s, throttle: t, brake: b, ..Input::default() }, &g, DT);
    }
    let f1 = car.forward();
    f0.cross(f1).y.atan2(f0.dot(f1)).to_degrees()
}

/// Straight braking from `kmh` with the brake key: (stopping distance m, heading drift °,
/// peak nose-down pitch °, pitch swings after stopping).
pub fn brake_test(surface: Surface, kmh: f32) -> (f32, f32, f32, u32) {
    let spec = Spec::default();
    let (mut car, g) = settled(surface);
    up_to_speed(&mut car, &spec, &g, kmh);
    let (p0, f0) = (car.pos, car.forward());
    let pitch0 = car.forward().y.asin().to_degrees();
    let mut peak = 0.0f32;
    let mut stop = None;
    let mut pitches = Vec::new();
    for i in 0..600 {
        let braking = stop.is_none();
        step(&mut car, &spec, Input { brake: if braking { 1.0 } else { 0.0 }, ..Input::default() }, &g, DT);
        let pitch = car.forward().y.asin().to_degrees() - pitch0;
        peak = peak.min(pitch);
        if stop.is_none() && car.speed() < 0.3 {
            stop = Some((i, car.pos.distance(p0), car.forward()));
        } else if stop.is_some() {
            pitches.push(pitch);
        }
    }
    let (_, d, f1) = stop.unwrap_or((0, f32::NAN, car.forward()));
    let drift = f0.cross(f1).y.atan2(f0.dot(f1)).to_degrees();
    let mut swings = 0;
    for w in pitches.windows(3) {
        if (w[1] - w[0]) * (w[2] - w[1]) < 0.0 && w[1].abs() > 0.15 {
            swings += 1;
        }
    }
    (d, drift, -peak, swings)
}

/// Full throttle from rest: (rpm after 0.5 s, 0–50 and 0–100 km/h in s, gear changes up to
/// 100 km/h, top speed km/h after 60 s).
pub fn launch_test(surface: Surface) -> (f32, f32, f32, u32, f32) {
    let spec = Spec::default();
    let (mut car, g) = settled(surface);
    let (mut rpm05, mut t50, mut t100, mut shifts) = (0.0, 99.0, 99.0, 0);
    let mut gear = car.gear;
    for i in 0..(60 * 60) {
        step(&mut car, &spec, Input { throttle: 1.0, ..Input::default() }, &g, DT);
        let t = (i + 1) as f32 * DT;
        if i == 29 {
            rpm05 = car.rpm;
        }
        let kmh = car.speed() * 3.6;
        if kmh >= 50.0 && t50 > 98.0 {
            t50 = t;
        }
        if kmh >= 100.0 && t100 > 98.0 {
            t100 = t;
        }
        if car.gear != gear {
            if t100 > 98.0 {
                shifts += 1;
            }
            gear = car.gear;
        }
    }
    (rpm05, t50, t100, shifts, car.speed() * 3.6)
}

/// Washboard: 0.06 m bumps every 3 m at `kmh`. Returns (peak pitch rate °/s, peak heave
/// speed m/s, wheels-off-the-ground share).
pub fn bumps_test(kmh: f32) -> (f32, f32, f32) {
    struct Wash;
    impl Ground for Wash {
        fn height(&self, _: f32, z: f32) -> f32 {
            if z > 30.0 { 0.03 * (1.0 - (z * std::f32::consts::TAU / 3.0).cos()) } else { 0.0 }
        }
        fn normal(&self, _: f32, z: f32) -> Vec3 {
            let dz = if z > 30.0 { 0.03 * std::f32::consts::TAU / 3.0 * (z * std::f32::consts::TAU / 3.0).sin() } else { 0.0 };
            Vec3::new(0.0, 1.0, -dz).normalize()
        }
        fn surface(&self, _: f32, _: f32) -> Surface {
            Surface::Gravel
        }
        fn obstacles(&self, _: Vec2, _: f32, out: &mut Vec<Obstacle>) {
            out.clear();
        }
    }
    let spec = Spec::default();
    let mut car = Car::new(Vec3::new(0.0, 0.9, 0.0), 0.0);
    for _ in 0..60 {
        step(&mut car, &spec, Input { handbrake: true, ..Input::default() }, &Wash, DT);
    }
    let v = kmh / 3.6;
    car.vel = car.forward() * v;
    for w in &mut car.wheels {
        w.spin = v / spec.wheel_radius;
    }
    let (mut pr, mut hv, mut off, mut n) = (0.0f32, 0.0f32, 0, 0);
    for _ in 0..(60 * 4) {
        let (t, b) = pedals(&car, v);
        step(&mut car, &spec, Input { throttle: t, brake: b, ..Input::default() }, &Wash, DT);
        if car.pos.z > 35.0 {
            pr = pr.max(car.ang.dot(left(&car)).abs().to_degrees());
            hv = hv.max(car.vel.y.abs());
            off += car.wheels.iter().filter(|w| !w.contact).count();
            n += 4;
        }
    }
    (pr, hv, off as f32 / n.max(1) as f32)
}

#[test]
#[ignore]
fn player_report() {
    println!("\n== key held (keyboard ramp) then let go ==");
    for surf in [Surface::Gravel, Surface::Tarmac] {
        for kmh in [40.0, 70.0, 100.0] {
            let r = key_step(surf, kmh, 2.0);
            println!(
                "{surf:?} {kmh:3.0}: path {:5.1}°/s  yaw {:5.1}°/s  t90 {:.2} s  overshoot {:4.0}%  β {:5.1}° swing {:4.1}°  | let go: settle {:.2} s  wobble {}  β max {:4.1}°",
                r.path, r.steady, r.t90, r.overshoot * 100.0, r.beta, r.beta_swing, r.settle, r.wobble, r.beta_after
            );
        }
    }
    println!("\n== key held at hairpin speeds: turning radius ==");
    for surf in [Surface::Gravel, Surface::Tarmac] {
        for kmh in [15.0, 25.0, 35.0, 50.0] {
            let r = key_step(surf, kmh, 3.0);
            let radius = kmh / 3.6 / r.path.to_radians().max(1e-3);
            println!("{surf:?} {kmh:3.0} km/h: radius {radius:5.1} m  ({:.2} g)", (kmh / 3.6) * r.path.to_radians() / 9.81);
        }
    }
    println!("\n== taps ==");
    for surf in [Surface::Gravel, Surface::Tarmac] {
        for (kmh, secs) in [(60.0, 0.1), (60.0, 0.25), (100.0, 0.1), (100.0, 0.25)] {
            println!("{surf:?} {kmh:3.0} km/h tap {secs:.2} s: {:5.1}°", key_tap(surf, kmh, secs));
        }
    }
    println!("\n== straight braking ==");
    for surf in [Surface::Gravel, Surface::Tarmac, Surface::Snow] {
        for kmh in [60.0, 100.0] {
            let (d, drift, pitch, swings) = brake_test(surf, kmh);
            println!("{surf:?} {kmh:3.0}: {d:5.1} m ({:.2} g)  drift {drift:5.2}°  pitch {pitch:4.1}°  swings after {swings}", (kmh / 3.6f32).powi(2) / (2.0 * d * 9.81));
        }
    }
    println!("\n== launch ==");
    for surf in [Surface::Gravel, Surface::Tarmac, Surface::Snow] {
        let (rpm, t50, t100, shifts, top) = launch_test(surf);
        println!("{surf:?}: rpm @0.5 s {rpm:5.0}  0-50 {t50:.1} s  0-100 {t100:.1} s  shifts to 100: {shifts}  top {top:.0} km/h");
    }
    println!("\n== washboard ==");
    for kmh in [40.0, 80.0, 120.0] {
        let (pr, hv, off) = bumps_test(kmh);
        println!("{kmh:3.0} km/h: pitch rate {pr:4.1}°/s  heave {hv:.2} m/s  wheels off {:.0}%", off * 100.0);
    }
}

#[test]
#[ignore]
fn key_trace() {
    let surf = if std::env::var("SURF").ok().as_deref() == Some("tarmac") { Surface::Tarmac } else { Surface::Gravel };
    let kmh: f32 = std::env::var("KMH").ok().and_then(|s| s.parse().ok()).unwrap_or(100.0);
    let spec = Spec::default();
    let (mut car, g) = settled(surf);
    up_to_speed(&mut car, &spec, &g, kmh);
    let mut s = 0.0;
    for i in 0..150 {
        s = key_steer(&car, s, 1.0);
        let (t, b) = pedals(&car, kmh / 3.6);
        let v0 = car.vel;
        step(&mut car, &spec, Input { steer: s, throttle: t, brake: b, ..Input::default() }, &g, DT);
        let a = (car.vel - v0) / DT;
        let w = &car.wheels;
        if i % 5 == 0 {
            println!(
                "{:.2} in {:.2} δ {:5.3} v {:5.1} yaw {:5.1} β {:5.1} lat {:4.2} th {:.2} | αF {:5.1} {:5.1} αR {:5.1} {:5.1} | κ {:5.2} {:5.2} {:5.2} {:5.2} | Fz {:4.0} {:4.0} {:4.0} {:4.0}",
                i as f32 * DT, s, car.steer, car.speed() * 3.6, yaw(&car),
                car.vel.dot(left(&car)).atan2(car.speed()).to_degrees(), a.dot(left(&car)) / 9.81, t,
                w[0].slip_angle.to_degrees(), w[1].slip_angle.to_degrees(), w[2].slip_angle.to_degrees(), w[3].slip_angle.to_degrees(),
                w[0].slip_ratio, w[1].slip_ratio, w[2].slip_ratio, w[3].slip_ratio, w[0].load, w[1].load, w[2].load, w[3].load
            );
        }
    }
}

// ---------------------------------------------------------------- locked in: the way a player drives

#[test]
fn keyboard_steering_turns_at_the_limit_without_wobble() {
    // A held key works the car to its grip at any speed, the sideslip stays steady (no
    // swinging at the limit), and letting go settles in at most a swing or two.
    for (surf, g) in [(Surface::Gravel, 0.66), (Surface::Tarmac, 0.88)] {
        for kmh in [40.0, 70.0, 100.0] {
            let r = key_step(surf, kmh, 2.0);
            let lat = (kmh / 3.6) * r.path.to_radians() / 9.81;
            assert!(lat > g, "{surf:?} {kmh} km/h: a held key only turns at {lat:.2} g ({r:?})");
            assert!(r.beta_swing < 6.0 && r.wobble <= 2 && r.settle < 1.5, "{surf:?} {kmh} km/h: {r:?}");
        }
    }
    // Hairpins: a held key at 25 km/h makes a tight hairpin.
    for surf in [Surface::Gravel, Surface::Tarmac] {
        let r = key_step(surf, 25.0, 3.0);
        let radius = 25.0 / 3.6 / r.path.to_radians();
        assert!(radius < 8.5, "{surf:?}: {radius:.1} m at 25 km/h");
    }
}

#[test]
fn launch_brakes_and_gearing_are_a_rally_cars() {
    for surf in [Surface::Gravel, Surface::Tarmac] {
        let (rpm, _, t100, _, top) = launch_test(surf);
        // The clutch slips at launch (no bogging at idle); 0–100 in 3.5–5.5 s; gravel gearing.
        assert!(rpm > 3_500.0, "{surf:?}: {rpm:.0} rpm 0.5 s into a launch");
        assert!((3.5..5.5).contains(&t100), "{surf:?}: 0–100 km/h in {t100:.1} s");
        assert!((180.0..205.0).contains(&top), "{surf:?}: top speed {top:.0} km/h");
    }
    for (surf, g) in [(Surface::Gravel, 0.78), (Surface::Tarmac, 0.98)] {
        let (d, drift, pitch, swings) = brake_test(surf, 100.0);
        let decel = (100.0f32 / 3.6).powi(2) / (2.0 * d * 9.81);
        assert!(decel > g && drift.abs() < 0.5 && pitch < 3.0 && swings <= 1, "{surf:?}: {d:.1} m ({decel:.2} g), drift {drift:.2}°, pitch {pitch:.1}°, swings {swings}");
    }
}

#[test]
fn a_keyboard_player_gets_through_the_stages() {
    // On/off keys every 0.1 s through the game's keyboard ramp, at a steady pace. Not all
    // stages: a bang-bang bot is a poor driver, but most, and a roll only after a hit.
    let mut finished = 0;
    for def in stage::stages() {
        let mut keys = Keys::default();
        let r = drive_stage_traced(&def, 0.55, 900.0, None, Some(&mut keys));
        finished += r.done as u32;
        for x in &r.rollovers {
            assert!(x.contains("obstacle hit"), "stage {} {}: rolled without a trip: {x}", def.id, def.name);
        }
        assert!(r.hunts <= 4, "stage {} {}: the gearbox hunted {} times", def.id, def.name, r.hunts);
    }
    assert!(finished >= 16, "a keyboard player finished {finished}/20");
}

// ---------------------------------------------------------------- audit regressions

/// Sloped ground (a uniform grade rising towards +Z), for hill tests.
struct Slope(f32, Surface);

impl Ground for Slope {
    fn height(&self, _: f32, z: f32) -> f32 {
        z * self.0
    }
    fn normal(&self, _: f32, _: f32) -> Vec3 {
        Vec3::new(0.0, 1.0, -self.0).normalize()
    }
    fn surface(&self, _: f32, _: f32) -> Surface {
        self.1
    }
    fn obstacles(&self, _: Vec2, _: f32, out: &mut Vec<Obstacle>) {
        out.clear();
    }
}

#[test]
fn engine_braking_slows_a_car_rolling_backwards() {
    let spec = Spec::default();
    let (mut car, g) = settled(Surface::Tarmac);
    car.vel = -car.forward() * 8.0;
    for w in &mut car.wheels {
        w.spin = -8.0 / spec.wheel_radius;
    }
    for _ in 0..180 {
        step(&mut car, &spec, Input::default(), &g, DT);
    }
    assert!(car.speed() > -6.0, "rolling back in 1st sped up to {:.1} m/s", car.speed());
}

#[test]
fn heavy_landings_stay_on_the_wheels() {
    // Dropped flat at 7 m/s: the bump stops and dampers soak it up, no bounce into the air.
    let spec = Spec::default();
    let (mut car, g) = settled(Surface::Gravel);
    car.pos.y += 2.5;
    car.vel = Vec3::new(0.0, -7.0, 0.0);
    let (mut touched, mut airborne_after) = (false, 0);
    for _ in 0..120 {
        step(&mut car, &spec, Input::default(), &g, DT);
        let any = car.wheels.iter().any(|w| w.contact);
        touched |= any;
        if touched && !any {
            airborne_after += 1;
        }
    }
    assert!(touched && airborne_after == 0, "bounced off the ground for {airborne_after} steps after landing");
}

#[test]
fn brakes_hold_on_a_slope() {
    // Stopped on a 20 % grade with no pedals (hill hold), facing up and down hill: it stays.
    let spec = Spec::default();
    for heading in [0.0, std::f32::consts::PI] {
        for surf in [Surface::Gravel, Surface::Tarmac] {
            let g = Slope(0.2, surf);
            let mut car = Car::new(Vec3::new(0.0, 0.9, 0.0), heading);
            for _ in 0..120 {
                step(&mut car, &spec, Input { handbrake: true, ..Input::default() }, &g, DT);
            }
            let p0 = car.pos;
            for _ in 0..(60 * 60) {
                step(&mut car, &spec, Input::default(), &g, DT);
            }
            let moved = (car.pos - p0).length();
            assert!(moved < 0.05, "{surf:?} heading {heading}: crept {moved:.3} m in a minute");
        }
    }
}

#[test]
fn wheel_slip_is_steady_at_low_speed() {
    // Driving along at 4 m/s: the slip ratio doesn't flip sign substep to
    // substep (the explicit spin update did, below ~7 m/s on tarmac).
    let spec = Spec::default();
    let (mut car, g) = settled(Surface::Tarmac);
    car.vel = car.forward() * 3.0;
    for w in &mut car.wheels {
        w.spin = 3.0 / spec.wheel_radius;
    }
    let h = DT / SUBSTEPS as f32;
    let obstacles = Vec::new();
    let mut prev = [0.0f32; 4];
    let mut flips = 0;
    for i in 0..(SUBSTEPS * 60 * 2) {
        let (t, b) = pedals(&car, 4.0);
        substep(&mut car, &spec, Input { throttle: t.max(0.05), brake: b, ..Input::default() }, &g, &obstacles, &[], h);
        for (k, w) in car.wheels.iter().enumerate() {
            if i > SUBSTEPS * 10 && w.slip_ratio * prev[k] < 0.0 && w.slip_ratio.abs() > 0.005 {
                flips += 1;
            }
            prev[k] = w.slip_ratio;
        }
    }
    assert!(car.speed() < 7.0 && flips == 0, "slip ratio changed sign {flips} times ({:.1} m/s)", car.speed());
}

#[test]
fn bad_inputs_keep_the_car_finite() {
    let spec = Spec::default();
    let (mut car, g) = settled(Surface::Gravel);
    for bad in [f32::NAN, f32::INFINITY, -1e30, 1e30] {
        for _ in 0..30 {
            step(&mut car, &spec, Input { steer: bad, throttle: bad, brake: bad, handbrake: false }, &g, DT);
        }
        assert!(car.pos.is_finite() && car.vel.is_finite() && car.rot.is_finite() && car.steer.is_finite(), "{bad}: {car:?}");
    }
    // A wild spin in the air doesn't blow up either.
    car.pos.y += 50.0;
    car.ang = Vec3::new(40.0, 60.0, 30.0);
    for _ in 0..180 {
        step(&mut car, &spec, Input::default(), &g, DT);
    }
    assert!(car.ang.is_finite() && car.ang.length() <= 25.01, "{:?}", car.ang);
}

/// The scripted drive's hash on every machine. If it changes on purpose (a sim change), bump
/// `vehicle::REVISION` and update it here. If it differs only on some CPU, the sim has picked
/// up a platform-dependent call (see the comments at the libm calls in vehicle.rs).
const SCRIPTED_HASH: u64 = 0x55e6_02b7_93c1_5097;

#[test]
fn scripted_drive_is_bit_identical() {
    let (_, snapped, brushed) = check::scripted();
    assert!(snapped >= 2 && brushed > 30, "the scripted drive should snap saplings ({snapped}) and plough through shrubs ({brushed} steps)");
    let h = check::scripted_hash();
    assert_eq!(h, check::scripted_hash(), "the sim isn't deterministic in-process");
    assert_eq!(h, SCRIPTED_HASH, "the sim's bits changed: {h:#018x}");
}

/// Every stage's road hash, the same on every machine (checked on Apple arm64 and x86_64:
/// before the generator used libm, 10 of 20 differed between them, so boards split by CPU).
/// A stage that changes on purpose gets a new id (docs/DESIGN.md), so these never change.
const STAGE_HASHES: [(u32, &str); 20] = [
    (1, "bf93b5a296b08f8a"),
    (2, "027b59fdfa5f194f"),
    (3, "28add4ad69c60021"),
    (4, "854c0823ad772af4"),
    (5, "52a7edeed30bd877"),
    (6, "d88733b6ac14f295"),
    (7, "65626ebaf80e5e67"),
    (8, "483ea741cb9165f4"),
    (9, "2f7111bdb4e48867"),
    (10, "5344116b8bf79f53"),
    (11, "81042b2b840e9078"),
    (12, "f7cb9e4e9b9cf201"),
    (13, "72dc104008fc890c"),
    (14, "e9992b5fb52165e2"),
    (15, "9d5112586d248967"),
    (16, "0cda48ddfbe7af9d"),
    (17, "998286ad3b47713e"),
    (18, "8f66ca6e81ea59d6"),
    (19, "0b11c1b768cdd60c"),
    (20, "c589becb13a7d501"),
];

#[test]
#[ignore]
fn print_stage_hashes() {
    for def in stage::stages() {
        let road = stage::generate_road(&def);
        println!("stagehash {} {}", def.id, crate::meta::stages::stage_hash(&def, &road));
    }
}

#[test]
fn stages_hash_the_same_everywhere() {
    for def in stage::stages() {
        let road = stage::generate_road(&def);
        let h = crate::meta::stages::stage_hash(&def, &road);
        let want = STAGE_HASHES.iter().find(|s| s.0 == def.id).map(|s| s.1);
        assert_eq!(Some(h.as_str()), want, "stage {} {}: the road generated differently", def.id, def.name);
    }
}

/// Hash of a stage's obstacles (positions, radii, tops after clearing), and how many.
pub fn obstacle_hash(w: &world::StageWorld) -> (u64, usize) {
    let list = w.obstacle_list();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (c, r, top) in &list {
        for v in [c.x, c.y, *r, *top] {
            h ^= v.to_bits() as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    (h, list.len())
}

#[test]
#[ignore]
fn print_obstacle_hashes() {
    for def in stage::stages() {
        let (h, n) = obstacle_hash(&world::generate(&def));
        println!("obstacles {:2} {h:016x} {n}", def.id);
    }
}

/// Every stage's obstacles (hash of the sorted trunk / boulder list, and how many), the same on
/// every machine: the car collides with these, so a tree on one CPU and not another would
/// break ghosts. Placement goes through pcg-core, whose rotations use libm. Regenerate with
/// `print_obstacle_hashes` only if the dressing changes on purpose.
const OBSTACLE_HASHES: [(u32, &str, usize); 20] = [
    (1, "0eaaae3943497242", 2644),
    (2, "e5acd3a5baf81537", 3151),
    (3, "9d9fe3e2448faf60", 556),
    (4, "9839138f129dec40", 911),
    (5, "3560c5d217b9cb29", 482),
    (6, "86f978b3fce63560", 673),
    (7, "2706f6140a436d91", 320),
    (8, "03a54225f4f9bab2", 274),
    (9, "61ade8201eb8aa21", 2127),
    (10, "9e49c7abad6c34a0", 2712),
    (11, "bbc60a3e6fe7d54d", 4886),
    (12, "5601288e7d0d0aa9", 1631),
    (13, "a504c72f1a690c8d", 1237),
    (14, "527eae37c50d0883", 857),
    (15, "107648e0b0265f40", 880),
    (16, "44a9d7704c59557e", 339),
    (17, "fcbeaeb024dbef36", 278),
    (18, "404d8102e6d54692", 484),
    (19, "a5941684e703a3e0", 1698),
    (20, "4986cb3f344b703d", 3306),
];

#[test]
fn obstacles_the_same_everywhere() {
    for def in stage::stages() {
        let (h, n) = obstacle_hash(&world::generate(&def));
        let want = OBSTACLE_HASHES.iter().find(|o| o.0 == def.id).map(|o| (o.1, o.2));
        assert_eq!(Some((format!("{h:016x}").as_str(), n)), want, "stage {} {}: the obstacles generated differently", def.id, def.name);
    }
}

/// Hash of a stage's roadside props (kind, base, yaw, size, in id order), and how many.
pub fn prop_hash(w: &world::StageWorld) -> (u64, usize) {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in w.props.iter() {
        for v in [p.kind as u32 as f32, p.pos.x, p.pos.y, p.pos.z, p.yaw, p.scale] {
            h ^= v.to_bits() as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    (h, w.props.len())
}

#[test]
#[ignore]
fn print_prop_hashes() {
    for def in stage::stages() {
        let (h, n) = prop_hash(&world::generate(&def));
        println!("    ({}, \"{h:016x}\", {n}),", def.id);
    }
}

/// Every stage's breakable / brush props, the same on every machine (which ones exist and
/// their ids decide what a run knocks down, so ghosts depend on them). Regenerate with
/// `print_prop_hashes` only if the props change on purpose.
const PROP_HASHES: [(u32, &str, usize); 20] = [
    (1, "36823cabe476b049", 513),
    (2, "0a943d7a8d8e6b37", 633),
    (3, "049ef7218ffee1fb", 515),
    (4, "5e679023458dbce1", 662),
    (5, "4e3693f6bcccd22e", 771),
    (6, "f456ed3a5763cd2d", 1097),
    (7, "dff689be6760c96b", 932),
    (8, "e1d47b8cca3e02f4", 646),
    (9, "e3ca692c5d408df0", 360),
    (10, "e4e94e58bfc3e30b", 505),
    (11, "cffbfc6d504a57d8", 918),
    (12, "65072dba51a89697", 405),
    (13, "b8fa97c611af753b", 876),
    (14, "671950e19c13052d", 605),
    (15, "3b88911f91b547d5", 1316),
    (16, "fa4432ecee96d725", 722),
    (17, "6622f09f0309ca04", 741),
    (18, "853639c408cc7d29", 1012),
    (19, "873cbf23108cbb4c", 309),
    (20, "591d4f9764e5fe48", 598),
];

#[test]
fn props_the_same_everywhere() {
    for def in stage::stages() {
        let (h, n) = prop_hash(&world::generate(&def));
        let want = PROP_HASHES.iter().find(|o| o.0 == def.id).map(|o| (o.1, o.2));
        assert_eq!(Some((format!("{h:016x}").as_str(), n)), want, "stage {} {}: the props generated differently", def.id, def.name);
    }
}

/// Diagnostic: throttle only, no steering, from each stage's start line. Prints where the car
/// leaves the road and every damaging hit.
#[test]
#[ignore]
fn throttle_off_the_line() {
    let only: Option<u32> = std::env::var("STAGE").ok().and_then(|s| s.parse().ok());
    for def in stage::stages() {
        if only.is_some_and(|o| o != def.id) {
            continue;
        }
        let w = world::generate(&def);
        let road = &w.road;
        let spec = Spec::default();
        let (p, h) = road.start();
        let mut car = Car::new(p + Vec3::Y * 0.9, h);
        for _ in 0..60 {
            step(&mut car, &spec, Input { handbrake: true, ..Input::default() }, &w, DT);
        }
        let curv: Vec<String> = (0..8).map(|k| format!("{:.3}", road.curv[road.station(k as f32 * 10.0)])).collect();
        let mut off_at = None;
        let mut t = 0.0;
        let mut lines = Vec::new();
        while t < 8.0 {
            let before = car.damage;
            step(&mut car, &spec, Input { throttle: 1.0, ..Input::default() }, &w, DT);
            t += DT;
            let rp = road.nearest(Vec2::new(car.pos.x, car.pos.z), 40.0);
            if off_at.is_none() && rp.is_none_or(|r| r.lateral.abs() > road.half_width(&r)) {
                off_at = Some((t, rp.map_or(0.0, |r| r.s), car.speed() * 3.6));
            }
            if car.damage.hits > before.hits {
                let d = car.damage;
                lines.push(format!(
                    "    t {:.2} {:.0} km/h sev {:.2}: eng {:.2}→{:.2} steer {:.2}→{:.2} susp {:.2}→{:.2}",
                    t, car.speed() * 3.6, d.last, before.engine, d.engine, before.steering, d.steering, before.suspension, d.suspension
                ));
            }
        }
        println!("stage {:2} curv/10m [{}] off {:?} dmg {:.2} wrecked {}", def.id, curv.join(" "), off_at, car.damage.worst(), car.damage.wrecked());
        for l in lines.iter().take(12) {
            println!("{l}");
        }
    }
}

/// Prints launch numbers per surface (for docs/HANDLING.md).
#[test]
#[ignore]
fn launch_table() {
    for surf in [Surface::Tarmac, Surface::Gravel, Surface::Snow] {
        let (rpm, t50, t100, shifts, top) = launch_test(surf);
        println!("{surf:?}: rpm@0.5s {rpm:.0}  0-50 {t50:.2} s  0-100 {t100:.2} s  shifts {shifts}  top {top:.0} km/h");
    }
}

/// A flat road for x < 4, then a side slope (`grade`, rise per metre; − = falling away)
/// covered in rough ground; a lip of `lip` m at the edge.
struct Verge {
    grade: f32,
    lip: f32,
}

impl Ground for Verge {
    fn height(&self, x: f32, _: f32) -> f32 {
        if x > 4.0 { self.lip + (x - 4.0) * self.grade } else { 0.0 }
    }
    fn normal(&self, x: f32, _: f32) -> Vec3 {
        if x > 4.0 { Vec3::new(-self.grade, 1.0, 0.0).normalize() } else { Vec3::Y }
    }
    fn surface(&self, x: f32, _: f32) -> Surface {
        if x > 4.0 { Surface::Rough } else { Surface::Gravel }
    }
    fn obstacles(&self, _: Vec2, _: f32, out: &mut Vec<Obstacle>) {
        out.clear();
    }
}

/// Leaving the road: at `kmh`, `angle`° off the road's line (0 = straight along, 90 =
/// sideways), sliding with `beta`° of slip, onto `g`. Returns the lowest `up.y` in 4 s
/// (< 0 = on its roof or side past 90°).
fn leave_road(g: &impl Ground, kmh: f32, angle: f32, beta: f32) -> f32 {
    let spec = Spec::default();
    let (a, b) = (angle.to_radians(), beta.to_radians());
    // Heading h faces (sin h, cos h): travel along the road (+Z) turned `angle` towards +X.
    let mut car = Car::new(Vec3::new(0.0, 0.9, 0.0), a - b);
    for _ in 0..60 {
        step(&mut car, &spec, Input { handbrake: true, ..Input::default() }, g, DT);
    }
    let dir = Vec3::new(libm::sinf(a), 0.0, libm::cosf(a));
    car.vel = dir * kmh / 3.6;
    for w in &mut car.wheels {
        w.spin = kmh / 3.6 * libm::cosf(b) / spec.wheel_radius;
    }
    let mut min_up = 1.0f32;
    for _ in 0..(60 * 4) {
        step(&mut car, &spec, Input { brake: 0.3, ..Input::default() }, g, DT);
        min_up = min_up.min(car.up().y);
    }
    min_up
}

/// Diagnostic: where leaving the road rolls the car.
#[test]
#[ignore]
fn roll_report() {
    for (name, g) in [
        ("flat rough", Verge { grade: 0.0, lip: 0.0 }),
        ("ditch -0.35", Verge { grade: -0.35, lip: 0.0 }),
        ("ditch -0.6", Verge { grade: -0.6, lip: 0.0 }),
        ("bank +0.35", Verge { grade: 0.35, lip: 0.0 }),
        ("bank +0.6", Verge { grade: 0.6, lip: 0.0 }),
        ("lip 0.15", Verge { grade: 0.0, lip: 0.15 }),
    ] {
        for (angle, beta) in [(10.0, 0.0), (20.0, 0.0), (20.0, 30.0), (90.0, 90.0)] {
            let row: Vec<String> = [50.0, 70.0, 90.0, 110.0, 130.0].iter().map(|&v| format!("{:5.2}", leave_road(&g, v, angle, beta))).collect();
            println!("{name:12} angle {angle:3.0} β {beta:3.0}: 50..130 km/h min up {}", row.join(" "));
        }
    }
}

/// Flat rough ground with one trunk at (x, 0).
struct TrunkAt(f32);

impl Ground for TrunkAt {
    fn height(&self, _: f32, _: f32) -> f32 {
        0.0
    }
    fn normal(&self, _: f32, _: f32) -> Vec3 {
        Vec3::Y
    }
    fn surface(&self, _: f32, _: f32) -> Surface {
        Surface::Rough
    }
    fn obstacles(&self, _: Vec2, _: f32, out: &mut Vec<Obstacle>) {
        out.clear();
        out.push(Obstacle { pos: Vec2::new(self.0, 0.0), radius: 0.35, top: 10.0, give: Give::Solid });
    }
}

/// Diagnostic: sliding sideways (and at 45°) into a trunk.
#[test]
#[ignore]
fn trunk_roll_report() {
    for (angle, beta) in [(90.0, 90.0), (90.0, 45.0), (60.0, 60.0)] {
        let row: Vec<String> = [40.0, 60.0, 80.0, 100.0, 120.0].iter().map(|&v| format!("{:5.2}", leave_road(&TrunkAt(12.0), v, angle, beta))).collect();
        println!("trunk angle {angle:3.0} β {beta:3.0}: 40-120 km/h min up {}", row.join(" "));
    }
}

#[test]
fn sliding_sideways_into_a_trunk_spins_it_without_rolling() {
    // Side-on (and 45° nose-in) into a trunk: the car stops and spins; it may bend, but up to
    // 100 km/h it stays on its wheels. (Pushed at the body box's middle it rolled from 60.)
    for (angle, beta) in [(90.0, 90.0), (90.0, 45.0)] {
        for kmh in [60.0, 80.0, 100.0] {
            let up = leave_road(&TrunkAt(12.0), kmh, angle, beta);
            assert!(up > 0.5, "{kmh} km/h, {beta}° into a trunk tipped to up.y {up:.2}");
        }
    }
    // Still rolls: sideways off the road down a steep (31°) bank.
    assert!(leave_road(&Verge { grade: -0.6, lip: 0.0 }, 70.0, 90.0, 90.0) < 0.0);
}
