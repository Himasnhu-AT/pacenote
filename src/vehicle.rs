//! The rally car: a pure, fixed-step simulation (no Bevy systems, no physics engine), so it is
//! deterministic (ghosts and replays re-run it) and unit-tested.
//!
//! - Chassis: rigid body (mass, box inertia), integrated semi-implicitly.
//! - Suspension: per-wheel spring + damper along the body's down axis, anti-roll bar per axle.
//! - Tyres: slip ratio / slip angle through a simplified Pacejka curve shaped per surface
//!   (loose surfaces peak later and fall off gently), combined with a friction ellipse,
//!   rolling resistance and loose-surface drag; a viscous blend at walking pace (no jitter).
//! - Drivetrain: torque curve, 5-speed auto gearbox (brake at standstill = reverse), 4WD with
//!   an even split; handbrake locks the rears.
//! - Collisions: chassis corners against the ground, and against vertical cylinders (trees,
//!   rocks, posts) from [`Ground::obstacles`].
//!
//! Frame: local +X right, +Y up, −Z forward (Bevy's convention).

use bevy::math::{Mat3, Quat, Vec2, Vec3};

use crate::stage::Surface;

/// What the car drives on.
pub trait Ground {
    fn height(&self, x: f32, z: f32) -> f32;
    fn normal(&self, x: f32, z: f32) -> Vec3;
    fn surface(&self, x: f32, z: f32) -> Surface;
    /// Vertical cylinders `(centre xz, radius, top y)` near `p` (within `r`).
    fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<(Vec2, f32, f32)>);
}

/// Flat ground of one surface (tests, the garage).
pub struct Flat(pub f32, pub Surface);

impl Ground for Flat {
    fn height(&self, _: f32, _: f32) -> f32 {
        self.0
    }
    fn normal(&self, _: f32, _: f32) -> Vec3 {
        Vec3::Y
    }
    fn surface(&self, _: f32, _: f32) -> Surface {
        self.1
    }
    fn obstacles(&self, _: Vec2, _: f32, _: &mut Vec<(Vec2, f32, f32)>) {}
}

/// The car's fixed numbers.
#[derive(Clone, Debug)]
pub struct Spec {
    pub mass: f32,
    /// Chassis box (width, height, length) for inertia and collision.
    pub size: Vec3,
    /// Wheel mounts relative to the centre of mass: FL, FR, RL, RR.
    pub mounts: [Vec3; 4],
    pub wheel_radius: f32,
    pub wheel_inertia: f32,
    /// Suspension length at full droop, travel, spring (N/m), damping (N·s/m).
    pub rest: f32,
    pub travel: f32,
    pub spring: f32,
    pub damper: f32,
    /// Anti-roll bar stiffness (N/m of compression difference), front / rear.
    pub arb: [f32; 2],
    pub max_steer: f32,
    /// Steering lock at speed: `max_steer` at 0 down to this at `steer_speed` m/s.
    pub high_speed_steer: f32,
    pub steer_speed: f32,
    /// Steering rate (rad/s).
    pub steer_rate: f32,
    /// Peak engine torque (N·m) and the rpm band.
    pub torque: f32,
    pub idle_rpm: f32,
    pub redline: f32,
    pub gears: [f32; 5],
    pub reverse: f32,
    pub final_drive: f32,
    /// Share of drive torque to the front axle.
    pub front_split: f32,
    pub brake_torque: f32,
    /// Front share of brake torque.
    pub brake_bias: f32,
    pub handbrake_torque: f32,
    /// Drag area (Cd·A, m²).
    pub drag: f32,
    /// Trim torque when the driven wheels spin (a driver aid, on by default).
    pub traction_assist: bool,
    /// Limited-slip coupling between wheels on an axle and between the axles (1/s).
    pub lsd: f32,
    /// Ease the brakes on a locking wheel (on by default).
    pub abs: bool,
}

impl Default for Spec {
    fn default() -> Self {
        Spec {
            mass: 1250.0,
            size: Vec3::new(1.75, 1.0, 3.9),
            mounts: [Vec3::new(-0.76, -0.2, -1.25), Vec3::new(0.76, -0.2, -1.25), Vec3::new(-0.76, -0.2, 1.22), Vec3::new(0.76, -0.2, 1.22)],
            wheel_radius: 0.32,
            wheel_inertia: 1.1,
            rest: 0.42,
            travel: 0.26,
            spring: 42_000.0,
            damper: 3_800.0,
            arb: [9_000.0, 6_000.0],
            max_steer: 0.56,
            high_speed_steer: 0.17,
            steer_speed: 38.0,
            steer_rate: 2.6,
            torque: 520.0,
            idle_rpm: 950.0,
            redline: 7600.0,
            gears: [3.25, 2.05, 1.48, 1.14, 0.92],
            reverse: 3.2,
            final_drive: 4.4,
            front_split: 0.45,
            brake_torque: 4_200.0,
            brake_bias: 0.62,
            handbrake_torque: 3_200.0,
            drag: 0.72,
            traction_assist: true,
            lsd: 25.0,
            abs: true,
        }
    }
}

/// Driver input (one step).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    /// −1 (right) .. 1 (left).
    pub steer: f32,
    pub throttle: f32,
    pub brake: f32,
    pub handbrake: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wheel {
    /// Spin (rad/s; + = rolling forward).
    pub spin: f32,
    /// Spin angle (rad), for drawing.
    pub angle: f32,
    /// Suspension compression (m, 0 = full droop).
    pub compression: f32,
    pub contact: bool,
    /// Load (N), slip ratio, slip angle (rad).
    pub load: f32,
    pub slip_ratio: f32,
    pub slip_angle: f32,
    pub surface: Option<Surface>,
}

/// The car's state.
#[derive(Clone, Debug, PartialEq)]
pub struct Car {
    pub pos: Vec3,
    pub rot: Quat,
    pub vel: Vec3,
    /// Angular velocity (world, rad/s).
    pub ang: Vec3,
    pub wheels: [Wheel; 4],
    /// Actual steering angle (rad, + = left).
    pub steer: f32,
    pub rpm: f32,
    /// 1..=5 forward, −1 reverse.
    pub gear: i8,
    /// Seconds left in a gear change (no drive), and seconds since the last change.
    pub shifting: f32,
    pub in_gear: f32,
    /// Seconds held at a standstill on the brake (reverse engages after 0.5 s).
    pub stopped: f32,
    /// Sum of impact speeds (m/s) this run: for damage and the HUD.
    pub impacts: f32,
}

impl Car {
    pub fn new(pos: Vec3, heading: f32) -> Car {
        Car {
            pos,
            rot: Quat::from_rotation_y(heading + std::f32::consts::PI),
            vel: Vec3::ZERO,
            ang: Vec3::ZERO,
            wheels: [Wheel::default(); 4],
            steer: 0.0,
            rpm: 950.0,
            gear: 1,
            shifting: 0.0,
            in_gear: 0.0,
            stopped: 0.0,
            impacts: 0.0,
        }
    }

    pub fn forward(&self) -> Vec3 {
        self.rot * Vec3::NEG_Z
    }

    pub fn up(&self) -> Vec3 {
        self.rot * Vec3::Y
    }

    /// Speed along the car's nose (m/s).
    pub fn speed(&self) -> f32 {
        self.vel.dot(self.forward())
    }

    /// Wheel centre (world), for drawing.
    pub fn wheel_pos(&self, spec: &Spec, i: usize) -> Vec3 {
        let up = self.up();
        self.pos + self.rot * spec.mounts[i] - up * (spec.rest - self.wheels[i].compression)
    }
}

/// Substeps per [`step`] call (the tyres are stiff; small steps keep them stable).
pub const SUBSTEPS: usize = 8;

/// Advances the car by `dt` seconds (internally [`SUBSTEPS`] steps).
pub fn step(car: &mut Car, spec: &Spec, input: Input, ground: &impl Ground, dt: f32) {
    let h = dt / SUBSTEPS as f32;
    let mut obstacles = Vec::new();
    ground.obstacles(Vec2::new(car.pos.x, car.pos.z), 6.0, &mut obstacles);
    for _ in 0..SUBSTEPS {
        substep(car, spec, input, ground, &obstacles, h);
    }
}

fn pacejka(x: f32, b: f32, c: f32, e: f32) -> f32 {
    let bx = b * x;
    (c * (bx - e * (bx - bx.atan())).atan()).sin()
}

fn substep(car: &mut Car, spec: &Spec, input: Input, ground: &impl Ground, obstacles: &[(Vec2, f32, f32)], h: f32) {
    let g = Vec3::new(0.0, -9.81, 0.0);
    let mut force = g * spec.mass;
    let mut torque = Vec3::ZERO;
    let up = car.up();
    let speed = car.speed();
    let at = |p: Vec3, f: Vec3, force: &mut Vec3, torque: &mut Vec3, pos: Vec3| {
        *force += f;
        *torque += (p - pos).cross(f);
    };

    // Steering: speed-sensitive lock, rate-limited.
    let lock = spec.max_steer + (spec.high_speed_steer - spec.max_steer) * (speed.abs() / spec.steer_speed).clamp(0.0, 1.0);
    let want = input.steer.clamp(-1.0, 1.0) * lock;
    car.steer += (want - car.steer).clamp(-spec.steer_rate * h, spec.steer_rate * h);

    // Gearbox (auto) and engine.
    let fwd_wheel_spin = (car.wheels[0].spin + car.wheels[1].spin + car.wheels[2].spin + car.wheels[3].spin) * 0.25;
    if car.shifting > 0.0 {
        car.shifting -= h;
    }
    let reversing = car.gear < 0;
    if !reversing {
        // Brake held at a standstill for a moment: reverse.
        if speed < 0.8 && input.brake > 0.5 && input.throttle < 0.05 {
            car.stopped += h;
            if car.stopped > 0.5 {
                car.gear = -1;
            }
        } else {
            car.stopped = 0.0;
        }
    } else if input.throttle > 0.05 && speed > -0.8 {
        car.gear = 1;
    }
    let ratio = |gear: i8| if gear < 0 { -spec.reverse } else { spec.gears[(gear as usize - 1).min(4)] };
    let wheel_rpm = |gear: i8| fwd_wheel_spin * ratio(gear) * spec.final_drive * 60.0 / std::f32::consts::TAU;
    // Shift on road speed (not wheel spin: a spinning wheel would make the box hunt), with
    // hysteresis and a minimum time in gear.
    let road_rpm = |gear: i8| speed.abs() / spec.wheel_radius * ratio(gear).abs() * spec.final_drive * 60.0 / std::f32::consts::TAU;
    car.in_gear += h;
    if car.gear > 0 && car.shifting <= 0.0 && car.in_gear > 0.6 {
        // Up on road speed, or after a moment bouncing off the limiter (wheelspin on loose
        // ground: road speed alone would never shift and the limiter would cut drive).
        // Only when the next gear would sit clear of the downshift point, or the box hunts.
        let limiter = wheel_rpm(car.gear) > spec.redline - 150.0 && car.in_gear > 1.0 && road_rpm(car.gear + 1) > 3400.0;
        if (road_rpm(car.gear) > spec.redline - 400.0 || limiter) && car.gear < 5 {
            car.gear += 1;
            car.shifting = 0.12;
            car.in_gear = 0.0;
        } else if car.gear > 1 && road_rpm(car.gear) < 3200.0 && road_rpm(car.gear - 1) < spec.redline - 1500.0 {
            car.gear -= 1;
            car.shifting = 0.1;
            car.in_gear = 0.0;
        }
    }
    car.rpm = wheel_rpm(car.gear).abs().max(spec.idle_rpm);
    // Throttle (reverse: the brake pedal drives backwards).
    let pedal = if reversing { input.brake } else { input.throttle };
    let brake = if reversing { input.throttle } else { input.brake };
    // Hill hold: stopped with no pedals, the brakes hold the car (a driver would), so it
    // doesn't roll off a sloped start line or creep while the guide reads.
    let idle = input.throttle < 0.05 && input.brake < 0.05;
    let brake = if idle && speed.abs() < 1.0 { brake.max(0.5) } else { brake };
    let curve = {
        let x = (car.rpm / spec.redline).clamp(0.0, 1.0);
        // Rises to a broad plateau, falls past ~85%.
        (0.55 + 0.9 * x - 0.6 * x * x).clamp(0.3, 1.0)
    };
    let cut = car.rpm >= spec.redline || car.shifting > 0.0;
    // Traction assist: trim torque when the driven wheels slip far past peak grip.
    let slip = car.wheels.iter().map(|w| w.slip_ratio).fold(0.0f32, f32::max);
    let assist = if spec.traction_assist { (1.0 - (slip - 0.18).max(0.0) * 2.5).clamp(0.35, 1.0) } else { 1.0 };
    let engine = if cut { 0.0 } else { spec.torque * curve * pedal * assist };
    let drive_total = engine * ratio(car.gear) * spec.final_drive * 0.9;

    // Suspension pass first (loads), then tyres.
    let mut comp = [0.0f32; 4];
    let mut contact = [None; 4];
    for i in 0..4 {
        let mount = car.pos + car.rot * spec.mounts[i];
        let n = ground.normal(mount.x, mount.z);
        let gy = ground.height(mount.x, mount.z);
        // Distance from the mount down the body's down axis to the ground (upright enough).
        let dist = (mount.y - gy) / up.y.max(0.3);
        let reach = spec.rest + spec.wheel_radius;
        if dist < reach && up.y > 0.2 {
            comp[i] = (reach - dist).min(spec.travel);
            contact[i] = Some((mount - up * dist, n));
        }
    }
    for i in 0..4 {
        let w = &mut car.wheels[i];
        let Some((c, n)) = contact[i] else {
            w.contact = false;
            w.load = 0.0;
            w.compression = 0.0;
            w.surface = None;
            w.spin *= 1.0 - 0.3 * h;
            continue;
        };
        let comp_vel = (comp[i] - w.compression) / h;
        w.compression = comp[i];
        w.contact = true;
        let axle = i / 2;
        let other = comp[i ^ 1];
        let mut fs = spec.spring * comp[i] + spec.damper * comp_vel + spec.arb[axle] * (comp[i] - other);
        // Bump stop at full travel.
        if comp[i] >= spec.travel {
            fs += 60_000.0 * 0.02;
        }
        let fs = fs.max(0.0);
        w.load = fs;
        at(c, up * fs, &mut force, &mut torque, car.pos);

        // Tyre frame on the ground plane.
        let steer = if i < 2 { car.steer } else { 0.0 };
        let wheel_fwd = car.rot * (Quat::from_rotation_y(steer) * Vec3::NEG_Z);
        let f = (wheel_fwd - n * wheel_fwd.dot(n)).normalize_or(Vec3::NEG_Z);
        let right = f.cross(n);
        let r = c - car.pos;
        let vc = car.vel + car.ang.cross(r);
        let v_long = vc.dot(f);
        let v_lat = vc.dot(right);

        let surf = ground.surface(c.x, c.z);
        w.surface = Some(surf);
        let (mu, loose) = surf.grip();
        // Load sensitivity: grip per newton drops a little with load.
        let fz = fs.min(14_000.0);
        let mu = mu * (1.0 - 0.06 * (fz / 4_000.0 - 1.0)).clamp(0.85, 1.1);
        let peak = mu * fz;
        let (bx, by, e) = (12.0 - 5.0 * loose, 9.0 - 3.5 * loose, -0.6 - 1.4 * loose);

        // Wheel spin: drive, brake, tyre reaction.
        let drive = drive_total * if i < 2 { spec.front_split } else { 1.0 - spec.front_split } * 0.5;
        let mut brake_t = brake * spec.brake_torque * if i < 2 { spec.brake_bias } else { 1.0 - spec.brake_bias } * 0.5;
        // ABS (a driver aid): ease the brake on a wheel that starts to lock. Not on the
        // handbrake, which is meant to lock the rears.
        if spec.abs && w.slip_ratio < -0.14 {
            brake_t *= (1.0 - (-0.14 - w.slip_ratio) * 4.0).clamp(0.25, 1.0);
        }
        if input.handbrake && i >= 2 {
            brake_t += spec.handbrake_torque * 0.5;
        }
        let denom = v_long.abs().max(3.0);
        let slip_ratio = ((w.spin * spec.wheel_radius - v_long) / denom).clamp(-1.0, 1.0);
        let slip_angle = (-v_lat).atan2(v_long.abs().max(1.5));
        let mut fx = peak * pacejka(slip_ratio, bx, 1.65, e);
        let mut fy = peak * pacejka(slip_angle, by, 1.35, e * 0.6);
        // Friction ellipse.
        let m = (fx * fx + fy * fy).sqrt();
        if m > peak {
            fx *= peak / m;
            fy *= peak / m;
        }
        // Walking pace: viscous grip instead of slip (no jitter at rest).
        let slow = (1.0 - vc.length() / 2.5).clamp(0.0, 1.0);
        let share = spec.mass * 0.25 / h * 0.25;
        fy = fy * (1.0 - slow) + (-v_lat * share).clamp(-peak, peak) * slow;
        // Rolling resistance and loose-surface drag.
        let roll = -(v_long.signum()) * fz * (0.014 + 0.05 * loose) * (v_long.abs() / 1.0).min(1.0);
        at(c, f * (fx + roll) + right * fy, &mut force, &mut torque, car.pos);

        // Wheel angular update (brakes can't reverse spin within the step).
        let net = drive - fx * spec.wheel_radius;
        let mut spin = w.spin + net / spec.wheel_inertia * h;
        let bstep = brake_t / spec.wheel_inertia * h;
        if spin.abs() <= bstep {
            spin = 0.0;
        } else {
            spin -= bstep * spin.signum();
        }
        w.spin = spin;
        w.angle = (w.angle + spin * h) % std::f32::consts::TAU;
        w.slip_ratio = slip_ratio;
        w.slip_angle = slip_angle;
    }
    // Limited-slip diffs (viscous): pull each axle's wheels together, then the axles. Without
    // them an unloaded wheel (a rut, a crest) spins up and takes the drive with it. The
    // handbrake disconnects the rear (rally cars have a hydraulic handbrake cutoff).
    let k = 1.0 - (-spec.lsd * h).exp();
    let pair = |a: &mut f32, b: &mut f32| {
        let d = (*b - *a) * 0.5 * k;
        *a += d;
        *b -= d;
    };
    let [w0, w1, w2, w3] = &mut car.wheels;
    pair(&mut w0.spin, &mut w1.spin);
    if !input.handbrake {
        pair(&mut w2.spin, &mut w3.spin);
        let (mut f, mut r) = ((w0.spin + w1.spin) * 0.5, (w2.spin + w3.spin) * 0.5);
        let (f0, r0) = (f, r);
        pair(&mut f, &mut r);
        w0.spin += f - f0;
        w1.spin += f - f0;
        w2.spin += r - r0;
        w3.spin += r - r0;
    }

    // Aero drag (and a little downforce).
    let v2 = car.vel.length_squared();
    force += -car.vel * (0.5 * 1.2 * spec.drag * car.vel.length());
    force += -up * (0.3 * v2);

    // Chassis contact with the ground (rolled, bottomed out) and obstacles.
    let half = spec.size * 0.5;
    let mut hits: Vec<(Vec3, Vec3, f32)> = Vec::new();
    // A 3×2×3 grid over the box (corners, edge and face midpoints): narrow trunks between the
    // corners still hit.
    for sx in [-1.0f32, 0.0, 1.0] {
        for sy in [-1.0f32, 1.0] {
            for sz in [-1.0f32, 0.0, 1.0] {
                let local = Vec3::new(sx * half.x, sy * half.y, sz * half.z);
                let p = car.pos + car.rot * local;
                let gy = ground.height(p.x, p.z);
                let depth = gy - p.y;
                if depth > 0.0 {
                    let n = ground.normal(p.x, p.z);
                    let vp = car.vel + car.ang.cross(p - car.pos);
                    let vn = vp.dot(n);
                    let fnorm = (depth * 250_000.0 - vn * 12_000.0).max(0.0);
                    let vt = vp - n * vn;
                    let ft = -vt.normalize_or_zero() * (fnorm * 0.6).min(vt.length() * spec.mass * 0.5 / h);
                    at(p, n * fnorm + ft, &mut force, &mut torque, car.pos);
                }
                for (c, r, top) in obstacles {
                    let d = Vec2::new(p.x, p.z) - *c;
                    let dist = d.length();
                    if dist < *r && p.y < *top {
                        let n = d / dist.max(1e-4);
                        hits.push((p, Vec3::new(n.x, 0.0, n.y), r - dist));
                    }
                }
            }
        }
    }

    // Integrate.
    car.vel += force / spec.mass * h;
    let rot = Mat3::from_quat(car.rot);
    let s = spec.size;
    let ib = Vec3::new(s.y * s.y + s.z * s.z, s.x * s.x + s.z * s.z, s.x * s.x + s.y * s.y) * (spec.mass / 12.0) * 1.15;
    let iw = rot * Mat3::from_diagonal(ib) * rot.transpose();
    let iw_inv = rot * Mat3::from_diagonal(ib.recip()) * rot.transpose();
    let gyro = car.ang.cross(iw * car.ang);
    car.ang += iw_inv * (torque - gyro) * h;
    car.pos += car.vel * h;
    let w = car.ang * h;
    if w.length_squared() > 0.0 {
        car.rot = (Quat::from_scaled_axis(w) * car.rot).normalize();
    }
    // Obstacles: impulses (a penalty spring can't stop a tonne at speed on a 0.4 m trunk).
    // Deepest contact first; each cancels the closing speed (a little bounce, some friction)
    // and pushes the car out.
    hits.sort_by(|a, b| b.2.total_cmp(&a.2));
    for (p, n, depth) in hits.into_iter().take(3) {
        let r = p - car.pos;
        let vp = car.vel + car.ang.cross(r);
        let vn = vp.dot(n);
        if vn < 0.0 {
            if vn < -2.0 {
                car.impacts += -vn;
            }
            let rn = r.cross(n);
            let k = 1.0 / spec.mass + rn.dot(iw_inv * rn);
            let j = -(1.0 + 0.2) * vn / k;
            let vt = vp - n * vn;
            let jt = (vt.length() / k).min(j * 0.5);
            let imp = n * j - vt.normalize_or_zero() * jt;
            car.vel += imp / spec.mass;
            car.ang += iw_inv * r.cross(imp);
        }
        car.pos += n * depth * 0.8;
    }
}

/// Puts the car back on the road (the recover button): at `pos`, facing `heading`, stopped.
pub fn recover(car: &mut Car, pos: Vec3, heading: f32) {
    let impacts = car.impacts;
    *car = Car::new(pos + Vec3::Y * 0.6, heading);
    car.impacts = impacts;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(car: &mut Car, spec: &Spec, input: Input, ground: &impl Ground, secs: f32) {
        let dt = 1.0 / 60.0;
        for _ in 0..(secs / dt) as usize {
            step(car, spec, input, ground, dt);
        }
    }

    #[test]
    fn settles_level_at_rest() {
        let spec = Spec::default();
        let g = Flat(0.0, Surface::Tarmac);
        let mut car = Car::new(Vec3::new(0.0, 0.8, 0.0), 0.0);
        run(&mut car, &spec, Input::default(), &g, 3.0);
        assert!(car.vel.length() < 0.05, "still moving {:?}", car.vel);
        assert!(car.up().y > 0.999, "tilted {:?}", car.up());
        let static_comp = spec.mass * 9.81 / 4.0 / spec.spring;
        for w in &car.wheels {
            assert!(w.contact);
            assert!((w.compression - static_comp).abs() < 0.03, "compression {} vs {static_comp}", w.compression);
        }
    }

    #[test]
    fn accelerates_shifts_and_brakes() {
        let spec = Spec::default();
        let g = Flat(0.0, Surface::Tarmac);
        let mut car = Car::new(Vec3::new(0.0, 0.6, 0.0), 0.0);
        run(&mut car, &spec, Input::default(), &g, 1.0);
        run(&mut car, &spec, Input { throttle: 1.0, ..default() }, &g, 6.0);
        let v = car.speed();
        assert!(v > 25.0, "0-6 s only {v:.1} m/s");
        assert!(car.gear >= 3, "gear {}", car.gear);
        // Tarmac with ABS: at least 0.8 g on average.
        let secs = v / (0.8 * 9.81);
        run(&mut car, &spec, Input { brake: 1.0, ..default() }, &g, secs);
        assert!(car.speed().abs() < 1.0, "still {:.1} m/s after {secs:.1} s braking from {v:.1}", car.speed());
        // Held at a standstill, the brake becomes reverse.
        run(&mut car, &spec, Input { brake: 1.0, ..default() }, &g, 2.0);
        assert!(car.gear == -1 && car.speed() < -0.5, "no reverse: gear {} speed {}", car.gear, car.speed());
    }

    #[test]
    fn gravel_has_less_grip_than_tarmac() {
        let spec = Spec::default();
        let lateral_g = |surf| {
            let g = Flat(0.0, surf);
            let mut car = Car::new(Vec3::new(0.0, 0.6, 0.0), 0.0);
            run(&mut car, &spec, Input::default(), &g, 1.0);
            run(&mut car, &spec, Input { throttle: 0.6, ..default() }, &g, 4.0);
            let mut peak = 0.0f32;
            for _ in 0..180 {
                let v0 = car.vel;
                step(&mut car, &spec, Input { throttle: 0.3, steer: 1.0, ..default() }, &g, 1.0 / 60.0);
                let a = (car.vel - v0) * 60.0;
                peak = peak.max(a.dot(car.rot * Vec3::NEG_X).abs());
            }
            peak
        };
        let (tar, grav) = (lateral_g(Surface::Tarmac), lateral_g(Surface::Gravel));
        assert!(tar > grav * 1.1, "tarmac {tar:.1} vs gravel {grav:.1} m/s²");
        assert!(tar > 6.0 && grav > 3.0, "tarmac {tar:.1}, gravel {grav:.1}");
    }

    #[test]
    fn steering_turns_left_for_positive_input() {
        let spec = Spec::default();
        let g = Flat(0.0, Surface::Gravel);
        let mut car = Car::new(Vec3::new(0.0, 0.6, 0.0), 0.0);
        run(&mut car, &spec, Input::default(), &g, 1.0);
        run(&mut car, &spec, Input { throttle: 0.5, ..default() }, &g, 3.0);
        let before = car.forward();
        run(&mut car, &spec, Input { throttle: 0.3, steer: 1.0, ..default() }, &g, 1.0);
        // Left of the old heading = positive cross with up.
        assert!(before.cross(car.forward()).y > 0.05, "turned the wrong way");
    }

    #[test]
    fn deterministic() {
        let spec = Spec::default();
        let g = Flat(0.0, Surface::Gravel);
        let mk = || {
            let mut car = Car::new(Vec3::new(0.0, 0.6, 0.0), 0.3);
            for i in 0..300 {
                let input = Input { throttle: 0.8, steer: ((i as f32) * 0.05).sin(), handbrake: i % 50 < 5, ..default() };
                step(&mut car, &spec, input, &g, 1.0 / 60.0);
            }
            car
        };
        assert_eq!(mk(), mk());
    }

    #[test]
    fn hits_obstacles() {
        struct Tree;
        impl Ground for Tree {
            fn height(&self, _: f32, _: f32) -> f32 {
                0.0
            }
            fn normal(&self, _: f32, _: f32) -> Vec3 {
                Vec3::Y
            }
            fn surface(&self, _: f32, _: f32) -> Surface {
                Surface::Tarmac
            }
            fn obstacles(&self, _: Vec2, _: f32, out: &mut Vec<(Vec2, f32, f32)>) {
                out.clear();
                out.push((Vec2::new(0.0, 20.0), 0.4, 10.0));
            }
        }
        let spec = Spec::default();
        let mut car = Car::new(Vec3::new(0.0, 0.6, 0.0), 0.0);
        run(&mut car, &spec, Input::default(), &Tree, 1.0);
        run(&mut car, &spec, Input { throttle: 1.0, ..default() }, &Tree, 5.0);
        // Heading 0 faces +Z: the tree is ahead.
        assert!(car.pos.z < 20.0 - 1.5, "drove through the tree: z {}", car.pos.z);
        assert!(car.impacts > 0.0);
    }

    fn default() -> Input {
        Input::default()
    }
}

#[cfg(test)]
mod debug {
    use super::*;

    #[test]
    #[ignore]
    fn launch_trace() {
        let spec = Spec::default();
        let g = Flat(0.0, Surface::Tarmac);
        let mut car = Car::new(Vec3::new(0.0, 0.6, 0.0), 0.0);
        for _ in 0..60 {
            step(&mut car, &spec, Input::default(), &g, 1.0 / 60.0);
        }
        for i in 0..360 {
            step(&mut car, &spec, Input { throttle: 1.0, ..Input::default() }, &g, 1.0 / 60.0);
            if i % 20 == 0 {
                let w = &car.wheels;
                println!(
                    "t {:.2} v {:5.1} rpm {:5.0} gear {} shift {:.2} spin {:5.1} slip {:5.2} {:5.2} load {:5.0} {:5.0} up {:.3}",
                    i as f32 / 60.0, car.speed(), car.rpm, car.gear, car.shifting, w[0].spin, w[0].slip_ratio, w[2].slip_ratio, w[0].load, w[2].load, car.up().y
                );
            }
        }
    }
}
