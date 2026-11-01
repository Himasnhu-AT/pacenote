//! The co-driver's head in the passenger seat: thrown about by the car's acceleration, and
//! turned with the mouse within a natural range.
//!
//! [`HeadMotion`] is a damped spring per axis, driven by the acceleration in the car's frame
//! (from the smoothed network pose, `smooth.rs`): braking throws the head forward, a left
//! corner pushes it right, a landing drops it. The head also rolls and nods a little with the
//! offset. [`Look`] holds the mouse yaw and pitch, clamped to what a belted passenger can turn.
//! Pure maths, tested.

use bevy::prelude::*;

/// Head displacement per m/s² of acceleration (m): 1 g ≈ 4 cm.
pub const GAIN: f32 = 0.0042;
/// Furthest the head moves from its rest point (m).
pub const MAX_OFFSET: f32 = 0.075;
/// Spring natural frequency (Hz) and damping ratio of the neck.
pub const FREQ_HZ: f32 = 1.8;
pub const DAMPING: f32 = 0.75;
/// Roll and nod per metre of offset (rad/m).
pub const ROLL_PER_M: f32 = 1.4;
pub const NOD_PER_M: f32 = 1.1;

/// Look limits (rad): yaw either way, pitch down (to the lap) and up.
pub const YAW_MAX: f32 = 105.0 * std::f32::consts::PI / 180.0;
pub const PITCH_DOWN: f32 = 68.0 * std::f32::consts::PI / 180.0;
pub const PITCH_UP: f32 = 28.0 * std::f32::consts::PI / 180.0;
/// Resting pitch: a little down, the way a co-driver looks at the road.
pub const REST_PITCH: f32 = -0.06;

/// How far the head leans when looking round (m): forward past the seat's head wings, and in
/// towards the car's centre when looking across at the driver.
pub const LEAN_FORWARD: f32 = 0.17;
pub const LEAN_INWARD: f32 = 0.07;

/// Where the head moves for a look `yaw` (rad, + = left), car frame. `centre` is the side the
/// car's centre is on from this seat: +1 = to the left (right-hand seat), −1 = to the right.
/// Nothing straight ahead; full lean by ~80° either way, inward only towards the centre.
pub fn lean(yaw: f32, centre: f32) -> Vec3 {
    let k = (yaw.abs() / 80f32.to_radians()).clamp(0.0, 1.0);
    let k = k * k * (3.0 - 2.0 * k);
    let inward = if yaw * centre > 0.0 { LEAN_INWARD * k } else { 0.0 };
    // Centre to the left (+1) is −X.
    Vec3::new(-centre * inward, 0.015 * k, -LEAN_FORWARD * k)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeadMotion {
    /// Offset from the rest eye point, car frame (m).
    pub offset: Vec3,
    vel: Vec3,
}

impl HeadMotion {
    /// Step the neck spring by `dt` with car-frame acceleration `accel` (m/s², gravity
    /// excluded; car frame: +X right, +Y up, −Z forward). `amount` 0..1 scales it (camera
    /// shake setting, 0 = reduce motion).
    pub fn step(&mut self, accel: Vec3, dt: f32, amount: f32) {
        let amount = amount.clamp(0.0, 1.0);
        let target = (-accel * GAIN * amount).clamp_length_max(MAX_OFFSET);
        let w = std::f32::consts::TAU * FREQ_HZ;
        // Semi-implicit Euler in steps of at most 4 ms (stable at any frame rate).
        let mut left = dt.clamp(0.0, 0.1);
        while left > 0.0 {
            let h = left.min(0.004);
            let a = (target - self.offset) * (w * w) - self.vel * (2.0 * DAMPING * w);
            self.vel += a * h;
            self.offset += self.vel * h;
            left -= h;
        }
        self.offset = self.offset.clamp_length_max(MAX_OFFSET * 1.2);
        if amount == 0.0 {
            self.offset = Vec3::ZERO;
            self.vel = Vec3::ZERO;
        }
    }

    /// The head's tilt from the offset: rolls away from the corner, nods under braking.
    pub fn tilt(&self) -> Quat {
        // Head pushed right (+X) rolls clockwise as seen from behind (negative about −Z… i.e.
        // about +Z it's −). Pushed forward (−Z) nods down (negative pitch about +X).
        Quat::from_rotation_z(-self.offset.x * ROLL_PER_M) * Quat::from_rotation_x(self.offset.z * NOD_PER_M)
    }
}

/// Where the co-driver is looking (relative to the car), from the mouse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for Look {
    fn default() -> Self {
        Look { yaw: 0.0, pitch: REST_PITCH }
    }
}

impl Look {
    /// Add a look delta (rad; + yaw = left, + pitch = up), clamped. Near the yaw limits the
    /// pitch range narrows a little (the seat's wings and the B-pillar).
    pub fn add(&mut self, yaw: f32, pitch: f32) {
        self.yaw = (self.yaw + yaw).clamp(-YAW_MAX, YAW_MAX);
        let side = (self.yaw.abs() / YAW_MAX).clamp(0.0, 1.0);
        let down = PITCH_DOWN * (1.0 - 0.35 * side * side);
        self.pitch = (self.pitch + pitch).clamp(-down, PITCH_UP);
    }

    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(self.pitch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(accel: Vec3) -> HeadMotion {
        let mut h = HeadMotion::default();
        for _ in 0..600 {
            h.step(accel, 1.0 / 120.0, 1.0);
        }
        h
    }

    #[test]
    fn braking_throws_the_head_forward() {
        // Braking: the car accelerates backwards (+Z in the car frame).
        let h = settle(Vec3::new(0.0, 0.0, 8.0));
        assert!(h.offset.z < -0.025, "{}", h.offset);
        // …and nods it down.
        let fwd = h.tilt() * Vec3::NEG_Z;
        assert!(fwd.y < -0.01, "{fwd}");
    }

    #[test]
    fn a_left_corner_pushes_the_head_right() {
        // Turning left: centripetal acceleration points left (−X).
        let h = settle(Vec3::new(-9.0, 0.0, 0.0));
        assert!(h.offset.x > 0.03, "{}", h.offset);
    }

    #[test]
    fn big_hits_stay_bounded_and_settle() {
        let mut h = HeadMotion::default();
        for i in 0..240 {
            let a = if i < 6 { Vec3::new(0.0, 200.0, -150.0) } else { Vec3::ZERO };
            h.step(a, 1.0 / 120.0, 1.0);
            assert!(h.offset.length() <= MAX_OFFSET * 1.2 + 1e-5);
        }
        assert!(h.offset.length() < 0.005, "{}", h.offset);
    }

    #[test]
    fn reduce_motion_keeps_the_head_still() {
        let mut h = HeadMotion::default();
        for _ in 0..100 {
            h.step(Vec3::new(5.0, 3.0, 8.0), 1.0 / 60.0, 0.0);
        }
        assert_eq!(h.offset, Vec3::ZERO);
    }

    #[test]
    fn frame_rate_does_not_change_the_motion() {
        let (mut a, mut b) = (HeadMotion::default(), HeadMotion::default());
        for i in 0..240 {
            let acc = Vec3::new((i as f32 * 0.05).sin() * 6.0, 0.0, 4.0);
            a.step(acc, 1.0 / 240.0, 1.0);
            if i % 8 == 7 {
                b.step(acc, 1.0 / 30.0, 1.0);
            }
        }
        assert!(a.offset.distance(b.offset) < 0.006, "{} vs {}", a.offset, b.offset);
    }

    #[test]
    fn looking_across_leans_forward_and_in() {
        assert_eq!(lean(0.0, 1.0), Vec3::ZERO);
        // Right-hand seat looking left at the driver: forward and to the left (−X).
        let l = lean(1.4, 1.0);
        assert!(l.z < -0.15 && l.x < -0.05, "{l}");
        // Looking out of the window: forward only.
        let r = lean(-1.4, 1.0);
        assert!(r.z < -0.15 && r.x == 0.0, "{r}");
    }

    #[test]
    fn look_is_limited_to_a_natural_range() {
        let mut l = Look::default();
        l.add(10.0, -10.0);
        assert!((l.yaw - YAW_MAX).abs() < 1e-6);
        assert!(l.pitch > -PITCH_DOWN && l.pitch < -0.5 * PITCH_DOWN);
        l.add(-20.0, 20.0);
        assert!((l.yaw + YAW_MAX).abs() < 1e-6 && (l.pitch - PITCH_UP).abs() < 1e-6);
        // Straight ahead, looking down reaches the lap.
        let mut l = Look::default();
        l.add(0.0, -5.0);
        assert!((l.pitch + PITCH_DOWN).abs() < 1e-6);
    }
}
