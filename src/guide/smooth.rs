//! Smooth car pose for the guide's in-car view.
//!
//! The engine already interpolates [`NetCar`] between 30 Hz snapshots, but in first person
//! two things still show: the interpolation holds the newest sample when a packet is late (the
//! world stops, then jumps), and the 32-bit packed rotation wobbles by up to ~0.3° from one
//! snapshot to the next, which at 100 m is half a metre of shake. [`PoseSmoother`] turns the
//! interpolated stream into a steady pose:
//!
//! - **Extrapolation**: while the input stops changing (a late packet), the target moves on with
//!   the last velocity and turn rate for up to [`MAX_EXTRAPOLATE`] s, like the clock does.
//! - **Prediction + correction**: each frame the shown pose moves with the estimated velocity
//!   and turn rate, then eases towards the target (time constants [`POS_TAU`], [`ROT_TAU`]), so
//!   a steady drive has no lag and quantisation noise is filtered out.
//! - **Snap**: a jump of more than [`SNAP_M`] (recover, restart) is taken at once.
//!
//! The same smoothed velocity gives the car's acceleration for the head motion
//! ([`PoseSmoother::accel`]). Pure maths, tested; the guide never simulates the car.

use bevy::prelude::*;

use super::sync::NetCar;

/// Longest the pose is carried on past the newest sample (s).
pub const MAX_EXTRAPOLATE: f32 = 0.25;
/// Position correction time constant (s).
pub const POS_TAU: f32 = 0.06;
/// Rotation correction time constant (s).
pub const ROT_TAU: f32 = 0.07;
/// Turn-rate estimate filter (s).
pub const OMEGA_TAU: f32 = 0.08;
/// Acceleration estimate filter (s): g-forces, not suspension buzz.
pub const ACCEL_TAU: f32 = 0.12;
/// A target further than this from the shown pose is jumped to (m).
pub const SNAP_M: f32 = 4.0;

/// Steady pose from the interpolated network car.
#[derive(Clone, Debug, Default)]
pub struct PoseSmoother {
    init: bool,
    pub pos: Vec3,
    pub rot: Quat,
    /// Estimated linear velocity (m/s) and turn rate (rad/s, world axis × angle).
    vel: Vec3,
    omega: Vec3,
    /// Filtered world acceleration (m/s²).
    accel: Vec3,
    last_vel: Vec3,
    /// The last input and how long it has been unchanged (s).
    last_in: Option<(Vec3, Quat)>,
    held: f32,
    /// Seconds since the last distinct input (for the turn-rate estimate).
    since_change: f32,
}

fn k(dt: f32, tau: f32) -> f32 {
    1.0 - (-dt / tau.max(1e-4)).exp()
}

/// Rotation `q` as a rotation vector (axis × angle, shortest way).
fn log(q: Quat) -> Vec3 {
    let q = if q.w < 0.0 { -q } else { q };
    let (axis, angle) = q.to_axis_angle();
    if angle.abs() < 1e-6 { Vec3::ZERO } else { axis * angle }
}

fn exp(v: Vec3) -> Quat {
    let a = v.length();
    if a < 1e-7 { Quat::IDENTITY } else { Quat::from_axis_angle(v / a, a) }
}

impl PoseSmoother {
    /// Forget everything (next update snaps to the input).
    pub fn reset(&mut self) {
        *self = PoseSmoother::default();
    }

    /// World acceleration (m/s², without gravity), filtered.
    pub fn accel(&self) -> Vec3 {
        self.accel
    }

    /// Smoothed velocity (m/s).
    pub fn vel(&self) -> Vec3 {
        self.vel
    }

    /// Feed this frame's interpolated car; returns the pose to draw.
    pub fn update(&mut self, car: &NetCar, dt: f32) -> (Vec3, Quat) {
        let dt = dt.clamp(0.0, 0.1);
        let rot_in = car.rot.normalize();
        if !self.init {
            self.init = true;
            self.pos = car.pos;
            self.rot = rot_in;
            self.vel = car.vel;
            self.last_vel = car.vel;
            self.last_in = Some((car.pos, rot_in));
            return (self.pos, self.rot);
        }
        // Is the input moving, or held at the newest sample?
        let changed = self.last_in.is_none_or(|(p, q)| p != car.pos || q != rot_in);
        if changed {
            if let Some((_, q)) = self.last_in {
                let span = (self.since_change + dt).max(1e-3);
                let w = log(rot_in * q.inverse()) / span;
                // A turn rate past ~3 rad/s between frames is a snap, not a turn.
                if w.length() < 6.0 {
                    self.omega = self.omega.lerp(w, k(span, OMEGA_TAU));
                }
            }
            self.last_in = Some((car.pos, rot_in));
            self.held = 0.0;
            self.since_change = 0.0;
        } else {
            self.held += dt;
            self.since_change += dt;
        }
        // Where the car should be now: the input, carried on while it's held.
        let ahead = self.held.min(MAX_EXTRAPOLATE);
        let target_pos = car.pos + car.vel * ahead;
        let target_rot = exp(self.omega * ahead) * rot_in;
        if target_pos.distance(self.pos) > SNAP_M {
            self.pos = target_pos;
            self.rot = target_rot;
            self.vel = car.vel;
            self.last_vel = car.vel;
            self.omega = Vec3::ZERO;
            self.accel = Vec3::ZERO;
            return (self.pos, self.rot);
        }
        // Predict with the estimated motion, then correct towards the target.
        self.vel = self.vel.lerp(car.vel, k(dt, POS_TAU));
        self.pos += self.vel * dt;
        self.pos = self.pos.lerp(target_pos, k(dt, POS_TAU));
        self.rot = (exp(self.omega * dt) * self.rot).normalize();
        self.rot = self.rot.slerp(target_rot, k(dt, ROT_TAU)).normalize();
        // Acceleration from the (already smoothed) velocity, filtered again.
        if dt > 0.0 {
            let a = (self.vel - self.last_vel) / dt;
            self.accel = self.accel.lerp(a.clamp_length_max(60.0), k(dt, ACCEL_TAU));
        }
        self.last_vel = self.vel;
        (self.pos, self.rot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car(pos: Vec3, rot: Quat, vel: Vec3) -> NetCar {
        NetCar { pos, rot, vel, ..default() }
    }

    /// The engine's interpolated stream: a 30 Hz sample stream, linearly interpolated at a
    /// 120 Hz render rate, with every `drop_every`-th packet late (held).
    fn stream(frames: usize, f: impl Fn(f32) -> (Vec3, Quat, Vec3), held: impl Fn(usize) -> bool) -> Vec<NetCar> {
        let dt = 1.0 / 120.0;
        let mut out = Vec::new();
        let mut last = None;
        for i in 0..frames {
            let t = i as f32 * dt;
            if held(i)
                && let Some(c) = last
            {
                out.push(c);
                continue;
            }
            let (p, q, v) = f(t);
            let c = car(p, q, v);
            last = Some(c);
            out.push(c);
        }
        out
    }

    #[test]
    fn straight_drive_has_no_lag_and_steady_steps() {
        let v = Vec3::new(0.0, 0.0, -30.0);
        let s = stream(240, |t| (v * t, Quat::IDENTITY, v), |_| false);
        let mut sm = PoseSmoother::default();
        let mut prev: Option<Vec3> = None;
        for c in &s {
            let (p, _) = sm.update(c, 1.0 / 120.0);
            if let Some(q) = prev {
                let step: Vec3 = p - q;
                assert!((step.length() - 0.25).abs() < 0.02, "step {step}");
            }
            prev = Some(p);
        }
        assert!(prev.unwrap().distance(s.last().unwrap().pos) < 0.05);
    }

    #[test]
    fn late_packets_are_carried_on_not_held() {
        let v = Vec3::new(20.0, 0.0, 0.0);
        // 8 frames (67 ms) of no new data every 40 frames.
        let s = stream(400, |t| (v * t, Quat::IDENTITY, v), |i| i % 40 >= 32);
        let mut sm = PoseSmoother::default();
        let mut prev: Option<Vec3> = None;
        let mut worst = 0.0f32;
        for c in &s {
            let (p, _) = sm.update(c, 1.0 / 120.0);
            if let Some(q) = prev {
                let step = (p - q).length();
                worst = worst.max((step - 20.0 / 120.0).abs());
            }
            prev = Some(p);
        }
        // Raw: a held frame steps 0, then the next steps 9× as far. Smoothed: within 20 %.
        assert!(worst < 0.2 * 20.0 / 120.0 + 1e-3, "worst step error {worst}");
    }

    #[test]
    fn rotation_noise_is_filtered() {
        // A steady turn with ±0.3° packed-quaternion noise on every sample.
        let rate = 0.4;
        let noise = |i: usize| ((i * 7919) % 13) as f32 / 12.0 * 2.0 - 1.0;
        let dt = 1.0 / 120.0;
        let mut sm = PoseSmoother::default();
        let (mut raw_err, mut sm_err) = (0.0f32, 0.0f32);
        let mut sample = Quat::IDENTITY;
        for i in 0..600 {
            let t = i as f32 * dt;
            let truth = Quat::from_rotation_y(rate * t);
            // New sample every 4 frames (30 Hz), noisy.
            if i % 4 == 0 {
                sample = truth * Quat::from_rotation_x(noise(i) * 0.005) * Quat::from_rotation_y(noise(i + 3) * 0.005);
            }
            let (_, q) = sm.update(&car(Vec3::ZERO, sample, Vec3::ZERO), dt);
            if i > 120 {
                raw_err = raw_err.max(sample.angle_between(truth));
                sm_err = sm_err.max(q.angle_between(truth));
            }
        }
        assert!(sm_err < raw_err * 0.75, "smoothed {sm_err} vs raw {raw_err}");
        assert!(sm_err < 0.012, "{sm_err}");
    }

    #[test]
    fn recover_snaps() {
        let mut sm = PoseSmoother::default();
        sm.update(&car(Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO), 0.01);
        let (p, _) = sm.update(&car(Vec3::new(50.0, 0.0, 0.0), Quat::from_rotation_y(2.0), Vec3::ZERO), 0.01);
        assert_eq!(p, Vec3::new(50.0, 0.0, 0.0));
    }

    #[test]
    fn braking_shows_as_backward_acceleration() {
        let mut sm = PoseSmoother::default();
        let dt = 1.0 / 120.0;
        // Forward is −Z; braking at 8 m/s² from 30 m/s.
        let mut z = 0.0;
        for i in 0..120 {
            let v = 30.0 - 8.0 * i as f32 * dt;
            z -= v * dt;
            sm.update(&car(Vec3::new(0.0, 0.0, z), Quat::IDENTITY, Vec3::new(0.0, 0.0, -v)), dt);
        }
        let a = sm.accel();
        assert!((a.z - 8.0).abs() < 1.0, "{a}");
    }
}
