//! The rally car: a pure, fixed-step simulation (no Bevy systems, no physics engine), so it is
//! deterministic (ghosts and replays re-run it) and unit-tested.
//!
//! - Chassis: rigid body (mass, measured-style inertia), integrated semi-implicitly at
//!   [`SUBSTEPS`] × the fixed step.
//! - Suspension: per-wheel spring + damper (stiffer in rebound) along the body's down axis,
//!   anti-roll bar per axle, a progressive bump stop past the travel.
//! - Tyres ([`Tyre`], per surface): combined slip from the slip ratio and slip angle, each
//!   normalised by its peak slip, through one grip curve; the force points along the combined
//!   slip, so a locked wheel slides and gives up its side grip. Load sensitivity, rolling
//!   resistance and loose-surface drag; a viscous blend at walking pace (no jitter).
//! - Drivetrain: torque curve, engine braking, 5-speed auto gearbox (brake at standstill =
//!   reverse), 4WD; handbrake locks the rears.
//!
//! The numbers are a ~1,250 kg 4WD rally car (Group A / R5 class), see [`Spec::default`] and
//! `docs/HANDLING.md` for sources and the measured behaviour.
//! - Collisions: chassis corners against the ground, and against vertical cylinders
//!   ([`Obstacle`]) from [`Ground::obstacles`]: big trunks and boulders are solid (the car
//!   bounces off), saplings, posts and signs snap when hit hard enough (the car shoves them
//!   away, losing only the momentum it gives them, and they stay down for the run:
//!   [`Car::broken`]), and shrubs are brush the car drives through with a little drag.
//! - Damage ([`Damage`]): hits above walking pace against solid things (trunks, boulders, the
//!   ground in a roll or a heavy landing) damage the engine (power), the steering (less lock,
//!   slower, a pull to one side) and the suspension (less grip, wallowing), by where the car
//!   was hit and how hard, and knock body parts off ([`Damage::off`]). One part fully broken
//!   retires the car. Snapping a sapling, post or sign and driving through shrubs do none.
//!
//! Frame: local +X right, +Y up, −Z forward (Bevy's convention).

use bevy::math::{Mat3, Quat, Vec2, Vec3};

use crate::stage::Surface;

/// What the car drives on.
pub trait Ground {
    fn height(&self, x: f32, z: f32) -> f32;
    fn normal(&self, x: f32, z: f32) -> Vec3;
    fn surface(&self, x: f32, z: f32) -> Surface;
    /// Obstacles near `p` (within `r`), into `out` (cleared first).
    fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>);
}

/// A vertical cylinder the car can run into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obstacle {
    /// Centre (world xz), radius and top (world y).
    pub pos: Vec2,
    pub radius: f32,
    pub top: f32,
    pub give: Give,
}

impl Obstacle {
    /// A solid trunk or boulder.
    pub fn solid(pos: Vec2, radius: f32, top: f32) -> Obstacle {
        Obstacle { pos, radius, top, give: Give::Solid }
    }
}

/// How an obstacle reacts to the car.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Give {
    /// Big trunks and boulders: the car bounces off.
    Solid,
    /// Snaps when the car closes on it at `speed` m/s or more (slower, it's solid). The car
    /// shoves its `mass` kg away and drives on; it stays down for the run ([`Car::broken`]).
    /// `id` is unique on the stage.
    Breaks { id: u32, mass: f32, speed: f32 },
    /// Brush (shrubs): the car drives through, slowed by `drag` (1/s: deceleration per m/s of
    /// speed with the shrub fully under the car).
    Brush { drag: f32 },
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
    fn obstacles(&self, _: Vec2, _: f32, out: &mut Vec<Obstacle>) {
        out.clear();
    }
}

/// The car's fixed numbers.
#[derive(Clone, Debug)]
pub struct Spec {
    pub mass: f32,
    /// Moments of inertia about the centre of mass (kg·m²): pitch (X), yaw (Y), roll (Z).
    pub inertia: Vec3,
    /// Body box (width, height, length) for collision, centred at `body_centre` (relative to
    /// the centre of mass, which sits low in the box).
    pub size: Vec3,
    pub body_centre: Vec3,
    /// Wheel mounts relative to the centre of mass: FL, FR, RL, RR.
    pub mounts: [Vec3; 4],
    pub wheel_radius: f32,
    pub wheel_inertia: f32,
    /// Suspension length with the main spring fully extended, bump travel from there, spring
    /// (N/m), damping (N·s/m).
    pub rest: f32,
    pub travel: f32,
    pub spring: f32,
    pub damper: f32,
    /// Helper spring: extra droop past the main spring's full extension (m) and its rate (N/m).
    /// It keeps a wheel on the ground over a dip or on the inside of a corner (gravel cars run
    /// them); a spring alone only droops its static deflection, g/ω² ≈ 7 cm at 1.8 Hz.
    pub helper_travel: f32,
    pub helper_spring: f32,
    /// Anti-roll bar stiffness (N/m of compression difference), front / rear.
    pub arb: [f32; 2],
    pub max_steer: f32,
    /// Speed-sensitive lock (see [`steer_lock`]): full input at speed asks for the kinematic
    /// angle of a corner taken at `steer_grip` m/s² plus `steer_slip` × the front tyres' peak
    /// slip angle (so it works each surface's tyre to just past its peak).
    pub steer_grip: f32,
    pub steer_slip: f32,
    /// Steering rate (rad/s).
    pub steer_rate: f32,
    /// Peak engine torque (N·m) and the rpm band.
    pub torque: f32,
    /// Engine braking off throttle, as a share of peak torque at the redline.
    pub engine_brake: f32,
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
        // A ~1,250 kg 4WD rally car (R5 / Group A class: Fabia R5, i20 R5, Polo GTI R5 are
        // 1,230 kg minimum, ~2.55 m wheelbase, ~1.55–1.6 m track, ~3.9–4.0 m long).
        // Centre of mass ~0.48 m above the ground (typical hatchback 0.5–0.55, rally cars sit
        // lower), ~54 % on the front axle (transverse engine).
        // Static stability factor track / (2·h) = 1.56 / 0.96 ≈ 1.6: no tyre can roll it on
        // flat ground (the most grip anywhere is ~1.2), so only a trip can.
        Spec {
            mass: 1250.0,
            // Yaw ≈ m·(0.4·L)² ≈ 2,000 (published sedan yaw inertias 1,800–2,600); roll is
            // about a quarter of yaw for a car this low; pitch a little under yaw.
            inertia: Vec3::new(1_850.0, 2_000.0, 520.0),
            // 1.78 m wide, 3.95 m long; floor 0.16 m off the ground, roof ~1.36 m.
            size: Vec3::new(1.78, 1.2, 3.95),
            body_centre: Vec3::new(0.0, 0.28, 0.0),
            // Wheelbase 2.55 m (front axle 1.17 m ahead of the centre of mass: 54 % front),
            // track 1.56 m. Mount height puts the ground 0.48 m below the centre of mass at
            // static load.
            mounts: [Vec3::new(-0.78, 0.17, -1.17), Vec3::new(0.78, 0.17, -1.17), Vec3::new(-0.78, 0.17, 1.38), Vec3::new(0.78, 0.17, 1.38)],
            wheel_radius: 0.32,
            wheel_inertia: 1.1,
            rest: 0.40,
            travel: 0.24,
            // Ride frequency √(k/m_corner)/2π ≈ 1.8 Hz front, 1.9 Hz rear (gravel rally cars
            // run ~1.5–2 Hz, tarmac cars ~2.5+), damping ratio ~0.35 bump / 0.6 rebound.
            spring: 42_000.0,
            damper: 3_600.0,
            helper_travel: 0.08,
            helper_spring: 8_000.0,
            // Roll stiffness ~58 % front with the weight 54 % front: stable under power, loose
            // only on a lift or the handbrake. Roll gradient ~2.5°/g.
            arb: [13_000.0, 7_000.0],
            // 35° of lock: a ~10.5 m kerb-to-kerb circle (rally cars run extra lock for
            // hairpins; road hatchbacks are ~10–11 m).
            max_steer: 0.62,
            steer_grip: 8.0,
            steer_slip: 1.0,
            steer_rate: 3.0,
            // Peak torque at ~4,200 rpm; with the curve below, power peaks at ~210 kW (~285 hp)
            // around 6,700 rpm, an R5's (1.6 l turbo on a 32 mm restrictor: ~420 N·m at the
            // crank, ~210 kW).
            torque: 360.0,
            engine_brake: 0.16,
            idle_rpm: 950.0,
            redline: 7600.0,
            gears: [3.25, 2.05, 1.48, 1.14, 0.92],
            reverse: 3.2,
            // Gravel gearing: ~195 km/h at the limiter in 5th (R5 gravel ratios top out at
            // ~180–200 km/h), 1st to ~55 km/h.
            final_drive: 5.1,
            front_split: 0.5,
            brake_torque: 4_200.0,
            brake_bias: 0.62,
            handbrake_torque: 3_200.0,
            drag: 0.72,
            traction_assist: true,
            // Plated limited-slip diffs: an unloaded wheel can't spin away the drive.
            lsd: 60.0,
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
    /// Suspension compression before clamping to the travel (into the bump stop), for the
    /// damper's speed: the clamped value stops changing in the stop and the damper went dead.
    pub raw: f32,
}

/// Accumulated damage per part: 0 = as new, 1 = broken (the car is out).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Damage {
    pub engine: f32,
    pub steering: f32,
    pub suspension: f32,
    /// Which way bent steering pulls (+ = left), from the side of the steering hits.
    pub pull: f32,
    /// Damaging hits so far, and the severity of the last one (0.03 a knock … ≥ 1 fatal).
    pub hits: u32,
    pub last: f32,
    /// Body parts knocked off, one bit per [`PARTS`] entry ([`Damage::off`]).
    pub detached: u8,
}

/// Body parts that can come off, in bit order (`art::car::DETACHABLE` names).
pub const PARTS: [&str; 7] = ["MirrorL", "MirrorR", "RearWing", "FrontSplitter", "FrontBumper", "RearBumper", "RearDiffuser"];

/// Where on the body a hit landed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitZone {
    Front,
    Side,
    Rear,
    /// Roof or underside (rolls, heavy landings).
    Body,
}

/// Hits slower than this (m/s of closing speed) do no damage.
pub const DAMAGE_FROM: f32 = 4.0;
/// Closing speed (m/s) that breaks a part outright head-on (~80 km/h into a tree).
pub const DAMAGE_FATAL: f32 = 22.0;

impl Damage {
    /// Severity of a hit at closing speed `v` (m/s): 0 below [`DAMAGE_FROM`], 1 at
    /// [`DAMAGE_FATAL`], quadratic between (energy-like).
    pub fn severity(v: f32) -> f32 {
        let x = (v - DAMAGE_FROM).max(0.0) / (DAMAGE_FATAL - DAMAGE_FROM);
        x * x
    }

    /// A part is broken: the car can't continue.
    pub fn wrecked(&self) -> bool {
        self.engine >= 1.0 || self.steering >= 1.0 || self.suspension >= 1.0
    }

    /// The worst part (0..1).
    pub fn worst(&self) -> f32 {
        self.engine.max(self.steering).max(self.suspension).min(1.0)
    }

    /// Applies a hit of severity `sev` at `local` (body frame, −Z forward) in `zone`.
    pub fn hit(&mut self, sev: f32, local: Vec3, zone: HitZone) {
        if sev < 0.005 {
            return;
        }
        // Front corners hurt the steering more than the middle of the nose.
        let corner = (local.x.abs() / 0.8).clamp(0.0, 1.0);
        let front_half = local.z < 0.0;
        let (e, st, su) = match zone {
            HitZone::Front => (1.0, 0.35 + 0.5 * corner, 0.25 + 0.2 * corner),
            HitZone::Side if front_half => (0.15, 0.9, 0.7),
            HitZone::Side => (0.15, 0.2, 0.9),
            HitZone::Rear => (0.35, 0.05, 0.6),
            HitZone::Body => (0.35, 0.3, 0.45),
        };
        self.engine = (self.engine + sev * e).min(1.0);
        self.suspension = (self.suspension + sev * su).min(1.0);
        let ds = sev * st;
        self.steering = (self.steering + ds).min(1.0);
        // Bent on the left (−X) pulls left.
        if ds > 0.0 && local.x.abs() > 0.2 {
            self.pull = (self.pull - local.x.signum() * ds * 2.0).clamp(-1.0, 1.0);
        }
        self.hits += 1;
        self.last = sev;
        // Parts that come off: the low, light ones first, by zone and severity
        // (30 km/h head-on ≈ 0.06, 50 ≈ 0.17, 70 ≈ 0.36).
        let side = if local.x < 0.0 { "MirrorL" } else { "MirrorR" };
        let off: &[(&str, f32)] = match zone {
            HitZone::Front => &[("FrontSplitter", 0.05), ("FrontBumper", 0.2)],
            HitZone::Side if front_half => &[(side, 0.04)],
            HitZone::Side => &[],
            HitZone::Rear => &[("RearDiffuser", 0.05), ("RearBumper", 0.15), ("RearWing", 0.3)],
            HitZone::Body => &[("RearWing", 0.15), (side, 0.1)],
        };
        for &(name, at) in off {
            if sev >= at {
                self.detached |= 1 << PARTS.iter().position(|p| *p == name).unwrap_or(0);
            }
        }
    }

    /// Whether body part `name` ([`PARTS`]) has come off.
    pub fn off(&self, name: &str) -> bool {
        PARTS.iter().position(|p| *p == name).is_some_and(|i| self.detached & (1 << i) != 0)
    }
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
    /// Sum of impact speeds (m/s) against solid obstacles this run: for damage and the HUD.
    pub impacts: f32,
    pub damage: Damage,
    /// Breakable obstacles ([`Give::Breaks`] ids) the car has knocked down this run, in order:
    /// they're gone for the rest of it.
    pub broken: Vec<u32>,
    /// How far the body is in brush (0..1, the deepest shrub on the last substep): sounds.
    pub brush: f32,
    /// The body touched the ground last substep (a new touch is a hit).
    pub body_contact: bool,
    /// Where the brakes are holding the stopped car (hill hold, the handbrake, the foot brake):
    /// it's pinned there. Slip-based tyre forces alone act as a damper, not a hold, and the car
    /// crept down slopes (10 % grade: 0.8 m a minute on tarmac, 1.6 m on gravel).
    pub hold: Option<Vec3>,
}

impl Car {
    pub fn new(pos: Vec3, heading: f32) -> Car {
        Car {
            pos,
            // A turn about Y, with libm's sin / cos (same bits on every CPU).
            rot: {
                let a = (heading + std::f32::consts::PI) * 0.5;
                Quat::from_xyzw(0.0, libm::sinf(a), 0.0, libm::cosf(a))
            },
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
            damage: Damage::default(),
            broken: Vec::new(),
            brush: 0.0,
            body_contact: false,
            hold: None,
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

/// Bumped when the same inputs would drive differently (ghosts and boards key on it, with the
/// [`Spec`]): 3 = realistic handling (low centre of mass, combined-slip tyres, helper springs,
/// engine braking, box-vs-trunk collisions); 4 = steering lock from the direction of travel,
/// launch clutch slip, gravel gearing, stiffer diffs; 5 = the same bits on every CPU (libm),
/// bump-stop damping, engine braking either way, hill hold that holds, implicit wheel spin;
/// 6 = no damage, breakable saplings / posts / signs, shrubs that drag; 7 = damage again (from
/// solid hits only), body parts that come off.
pub const REVISION: u64 = 7;

/// Fastest the body may spin (rad/s); normal crashes stay under ~15.
const MAX_SPIN: f32 = 25.0;

/// Substeps per [`step`] call (the tyres are stiff; small steps keep them stable).
pub const SUBSTEPS: usize = 8;

/// Advances the car by `dt` seconds (internally [`SUBSTEPS`] steps).
pub fn step(car: &mut Car, spec: &Spec, input: Input, ground: &impl Ground, dt: f32) {
    // Inputs come from devices and from shared ghost files: keep them finite and in range.
    let clean = |v: f32, lo: f32| if v.is_finite() { v.clamp(lo, 1.0) } else { 0.0 };
    let input = Input { steer: clean(input.steer, -1.0), throttle: clean(input.throttle, 0.0), brake: clean(input.brake, 0.0), handbrake: input.handbrake };
    let before = car.clone();
    let h = dt / SUBSTEPS as f32;
    let mut obstacles = Vec::new();
    ground.obstacles(Vec2::new(car.pos.x, car.pos.z), 6.0, &mut obstacles);
    for _ in 0..SUBSTEPS {
        substep(car, spec, input, ground, &obstacles, h);
    }
    // Should the state ever go non-finite, keep the last good pose, stopped.
    let finite = car.pos.is_finite() && car.vel.is_finite() && car.ang.is_finite() && car.rot.is_finite() && car.steer.is_finite() && car.wheels.iter().all(|w| w.spin.is_finite());
    if !finite {
        *car = before;
        car.vel = Vec3::ZERO;
        car.ang = Vec3::ZERO;
        car.steer = if car.steer.is_finite() { car.steer } else { 0.0 };
        for w in &mut car.wheels {
            w.spin = 0.0;
        }
    }
}

/// Road-wheel steering angle (rad) at full input for a car doing `speed` m/s (undamaged).
///
/// Full lock when parking; at speed, the angle that takes the tightest corner the car can
/// hold (`steer_grip` m/s² of lateral acceleration: wheelbase × a / v²) plus `steer_slip` ×
/// the front tyres' peak slip angle, enough to work them to just past their peak and to catch
/// a slide. So full input is about the most useful steering at any speed and on any surface,
/// which keeps a digital keyboard from just ploughing the front tyres (a real driver would
/// never wind on 30° at 90 km/h). [`substep`] measures it from the front axle's direction of
/// travel. This is the lock on gravel; see [`steer_lock_on`].
pub fn steer_lock(spec: &Spec, speed: f32) -> f32 {
    steer_lock_on(spec, speed, tyre(Surface::Gravel).peak_angle)
}

/// [`steer_lock`] for front tyres peaking at `peak_angle` rad.
pub fn steer_lock_on(spec: &Spec, speed: f32, peak_angle: f32) -> f32 {
    let wheelbase = spec.mounts[2].z - spec.mounts[0].z;
    let v2 = (speed * speed).max(1.0);
    (wheelbase * spec.steer_grip / v2 + spec.steer_slip * peak_angle).min(spec.max_steer)
}

/// Where the steering puts the road wheels (rad, + = left) for each input: (full right,
/// centred, full left). Input between maps linearly on each side.
///
/// - **Lock at speed** ([`steer_lock_on`], for the tyres under the front wheels) is measured
///   from the way the front axle is travelling. Drifting nose-in, a held key can't wind on
///   more lock than the front tyres can use: with the lock fixed to the body, a held key at
///   speed put the fronts at twice their peak slip and the car yawed back and forth at the
///   limit (gravel at 100 km/h: 33 → 1 → 19°/s). Sliding nose-out, full opposite input still
///   points the wheels past the direction of travel. Once the travel direction is known it
///   already holds the corner's kinematic angle, so only the slip part of the lock is added.
/// - **Centred** follows the front axle's travel at speed (by [`ALIGN`]), as a real car's
///   caster and self-aligning torque turn the wheels when the driver's grip relaxes. So
///   letting go in a slide countersteers instead of holding the wheels straight to the body
///   (keyboard play left every slide uncaught). At parking speeds centred is straight ahead,
///   blended in between ~20 and ~55 km/h, so letting go in a hairpin straightens up.
/// - Bent steering has less lock.
pub fn steer_frame(car: &Car, spec: &Spec) -> (f32, f32, f32) {
    // The front tyres' peak slip angle, from the surfaces they were on last substep.
    let peak = {
        let f = [car.wheels[0].surface, car.wheels[1].surface];
        let n = f.iter().flatten().count();
        if n == 0 { tyre(Surface::Gravel).peak_angle } else { f.iter().flatten().map(|s| tyre(*s).peak_angle).sum::<f32>() / n as f32 }
    };
    let at_rest = steer_lock_on(spec, car.speed(), peak);
    let front = car.rot * Vec3::new(0.0, 0.0, spec.mounts[0].z);
    let vf = car.rot.inverse() * (car.vel + car.ang.cross(front));
    let blend = ((-vf.z - 2.0) / 4.0).clamp(0.0, 1.0);
    let travel = libm::atan2f(-vf.x, (-vf.z).max(0.1)).clamp(-spec.max_steer, spec.max_steer) * blend;
    let slip_part = spec.steer_slip * peak;
    let lock = (at_rest + (slip_part.min(at_rest) - at_rest) * blend) * (1.0 - 0.45 * car.damage.steering);
    let hi = (travel + lock).max(0.03).min(spec.max_steer);
    let lo = (travel - lock).min(-0.03).max(-spec.max_steer);
    let align = ((-vf.z - 5.5) / 9.5).clamp(0.0, 1.0) * ALIGN;
    let centre = (travel * align).clamp(lo, hi);
    (lo, centre, hi)
}

/// How far centred steering follows the front axle's travel at speed (1 = a free wheel; less
/// is the driver's hands relaxed but on the wheel: a free wheel let the car wobble in yaw).
const ALIGN: f32 = 0.5;

/// The road-wheel angle the steering asks for at `input` (−1 right … 1 left); see [`steer_frame`].
pub fn steer_target(car: &Car, spec: &Spec, input: f32) -> f32 {
    let (lo, c, hi) = steer_frame(car, spec);
    let u = input.clamp(-1.0, 1.0);
    if u >= 0.0 { c + u * (hi - c) } else { c + u * (c - lo) }
}

/// The input that asks for road-wheel angle `delta` (the inverse of [`steer_target`]).
pub fn steer_input_for(car: &Car, spec: &Spec, delta: f32) -> f32 {
    let (lo, c, hi) = steer_frame(car, spec);
    let u = if delta >= c { (delta - c) / (hi - c).max(1e-3) } else { (delta - c) / (c - lo).max(1e-3) };
    u.clamp(-1.0, 1.0)
}

/// A tyre on one surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tyre {
    /// Peak friction coefficient at the nominal load.
    pub mu: f32,
    /// Slip angle (rad) and slip ratio at peak force.
    pub peak_angle: f32,
    pub peak_ratio: f32,
    /// Share of the peak left when sliding far past it.
    pub tail: f32,
    /// 0 = sealed … 1 = very loose (rolling drag).
    pub loose: f32,
}

/// Tyre per surface: rally tyres for the stage's surface (slicks / tarmac compounds, gravel
/// tyres, studded snow tyres). Sources: Pacejka, *Tyre and Vehicle Dynamics* (peak slip angle
/// 4–8° on dry tarmac, peak slip ratio ~0.1); loose-surface tyre tests put gravel's peak at
/// ~0.8–0.9 g reached at 10–15° and staying flat past it; studded tyres on packed snow ~0.4–0.5.
pub fn tyre(s: Surface) -> Tyre {
    let t = |mu, deg: f32, peak_ratio, tail, loose| Tyre { mu, peak_angle: deg.to_radians(), peak_ratio, tail, loose };
    match s {
        Surface::Tarmac => t(1.15, 7.0, 0.10, 0.78, 0.0),
        Surface::Gravel => t(0.85, 11.0, 0.15, 0.88, 0.55),
        Surface::Snow => t(0.48, 10.0, 0.14, 0.85, 0.6),
        Surface::Mud => t(0.58, 12.0, 0.17, 0.85, 0.8),
        Surface::Verge => t(0.62, 12.0, 0.17, 0.82, 0.75),
        Surface::Rough => t(0.55, 13.0, 0.18, 0.85, 0.9),
    }
}

/// Grip curve over normalised slip `s` (1 = peak), divided by `s` (finite at 0): rises with
/// slope 2 (cornering stiffness = 2·μ·Fz / peak slip, ~16 Fz/rad on tarmac), flat at the peak,
/// then eases down to `tail` by 3× the peak slip.
fn grip_over_slip(s: f32, tail: f32) -> f32 {
    if s < 1.0 {
        2.0 - s
    } else {
        let x = ((s - 1.0) / 2.0).min(1.0);
        let f = 1.0 - (1.0 - tail) * x * x * (3.0 - 2.0 * x);
        f / s
    }
}

fn substep(car: &mut Car, spec: &Spec, input: Input, ground: &impl Ground, obstacles: &[Obstacle], h: f32) {
    let g = Vec3::new(0.0, -9.81, 0.0);
    let mut force = g * spec.mass;
    let mut torque = Vec3::ZERO;
    let up = car.up();
    let speed = car.speed();
    let at = |p: Vec3, f: Vec3, force: &mut Vec3, torque: &mut Vec3, pos: Vec3| {
        *force += f;
        *torque += (p - pos).cross(f);
    };

    // Steering: speed-sensitive lock, rate-limited. Damage takes lock and speed off it and
    // bends the straight-ahead to one side.
    let dmg = car.damage;
    let want = steer_target(car, spec, input.steer) + dmg.pull * dmg.steering * 0.09;
    let rate = spec.steer_rate * (1.0 - 0.5 * dmg.steering);
    car.steer += (want - car.steer).clamp(-rate * h, rate * h);

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
        // Not on a lift (a driver lifting at the top of a gear is about to brake, and an upshift
        // there would only come straight back down), unless the engine is at the limiter.
        let pulling = input.throttle > 0.3 || road_rpm(car.gear) > spec.redline;
        if (road_rpm(car.gear) > spec.redline - 400.0 && pulling || limiter) && car.gear < 5 {
            car.gear += 1;
            car.shifting = 0.12;
            car.in_gear = 0.0;
        } else if car.gear > 1 && road_rpm(car.gear) < 3200.0 && road_rpm(car.gear - 1) < spec.redline - 1500.0 {
            car.gear -= 1;
            car.shifting = 0.1;
            car.in_gear = 0.0;
        }
    }
    // Throttle (reverse: the brake pedal drives backwards).
    let pedal = if reversing { input.brake } else { input.throttle };
    // Pulling away the clutch slips: the engine holds launch revs (up to ~4,500 rpm with the
    // pedal down) until the wheels catch up, instead of bogging at idle.
    let launch = if car.gear.abs() == 1 { spec.idle_rpm + (4_500.0 - spec.idle_rpm) * pedal } else { 0.0 };
    car.rpm = wheel_rpm(car.gear).abs().max(spec.idle_rpm).max(launch);
    let brake = if reversing { input.throttle } else { input.brake };
    // Hill hold: stopped with no pedals, the brakes hold the car (a driver would), so it
    // doesn't roll off a sloped start line or creep while the guide reads.
    let idle = input.throttle < 0.05 && input.brake < 0.05;
    let brake = if idle && speed.abs() < 1.0 { brake.max(0.5) } else { brake };
    let curve = {
        let x = (car.rpm / spec.redline).clamp(0.0, 1.0);
        // A restricted turbo engine: torque peaks mid-range (~4,200 rpm) and falls away, so
        // power plateaus at ~210 kW (~285 hp) from ~5,500 rpm, as an R5's does.
        (0.55 + 1.6 * x - 1.45 * x * x).clamp(0.3, 1.0)
    };
    let cut = car.rpm >= spec.redline || car.shifting > 0.0;
    // Traction assist: trim torque when the driven wheels slip far past peak grip.
    let slip = car.wheels.iter().map(|w| w.slip_ratio / w.surface.map_or(0.15, |s| tyre(s).peak_ratio)).fold(0.0f32, f32::max);
    let assist = if spec.traction_assist { (1.0 - (slip - 1.0).max(0.0) * 0.5).clamp(0.35, 1.0) } else { 1.0 };
    // A damaged engine makes less power; a wrecked car makes none.
    // It runs out of breath up the rev range (at low revs on loose ground grip limits anyway).
    let x = (car.rpm / spec.redline).clamp(0.0, 1.0);
    let health = if dmg.wrecked() { 0.0 } else { (1.0 - dmg.engine * (0.3 + 0.6 * x)).max(0.1) };
    let mut engine = if cut { 0.0 } else { spec.torque * curve * pedal * assist * health };
    // Engine braking off the throttle (in a forward gear, above idle): it slows the car and
    // moves weight onto the front, which is what makes a lift rotate the car.
    if !reversing && !cut && pedal < 0.05 && car.rpm > spec.idle_rpm * 1.3 {
        engine -= fwd_wheel_spin.signum() * spec.torque * spec.engine_brake * (0.3 + 0.7 * x);
    }
    let drive_total = engine * ratio(car.gear) * spec.final_drive * 0.9;

    // Suspension pass first (loads), then tyres.
    // (An airborne wheel hangs at full droop: that's what its anti-roll bar partner sees.)
    let mut comp = [-spec.helper_travel; 4];
    // Compression past the travel (into the bump stop).
    let mut over = [0.0f32; 4];
    let mut contact = [None; 4];
    let mut raw = [0.0f32; 4];
    for i in 0..4 {
        let mount = car.pos + car.rot * spec.mounts[i];
        let n = ground.normal(mount.x, mount.z);
        let gy = ground.height(mount.x, mount.z);
        // Distance from the mount down the body's down axis to the ground (upright enough).
        // Along the body's down axis to the ground plane at the mount (on a slope, dividing by
        // up.y alone sat the car ~7 cm low on a 30 % grade or a ditch bank).
        let dist = (mount.y - gy) * n.y / up.dot(n).max(0.3);
        // Compression is measured from the main spring's full extension: down to
        // −helper_travel on the helper spring.
        let reach = spec.rest + spec.helper_travel + spec.wheel_radius;
        if dist < reach && up.y > 0.2 {
            let x = reach - dist - spec.helper_travel;
            comp[i] = x.min(spec.travel);
            raw[i] = x;
            over[i] = (x - spec.travel).max(0.0);
            contact[i] = Some((mount - up * dist, n));
        }
    }
    for i in 0..4 {
        let w = &mut car.wheels[i];
        let Some((c, n)) = contact[i] else {
            w.contact = false;
            w.load = 0.0;
            w.compression = -spec.helper_travel;
            w.raw = -spec.helper_travel;
            w.surface = None;
            // No slip off the ground (a stale value let a lifted wheel trip traction assist).
            w.slip_ratio = 0.0;
            w.slip_angle = 0.0;
            // Bearing drag, and the brakes still stop a wheel in the air.
            let mut bt = brake * spec.brake_torque * if i < 2 { spec.brake_bias } else { 1.0 - spec.brake_bias } * 0.5;
            if input.handbrake && i >= 2 {
                bt += spec.handbrake_torque * 0.5;
            }
            let b = bt / spec.wheel_inertia * h;
            w.spin *= 1.0 - 0.3 * h;
            w.spin = if w.spin.abs() <= b { 0.0 } else { w.spin - b * w.spin.signum() };
            continue;
        };
        let comp_vel = (raw[i] - w.raw) / h;
        w.raw = raw[i];
        w.compression = comp[i];
        w.contact = true;
        let axle = i / 2;
        let other = comp[i ^ 1];
        // Dampers are stiffer in rebound than in bump (so a wheel soaks up a bump, and the
        // inside of a corner doesn't spring up). Damaged dampers wallow.
        let damp = spec.damper * (1.0 - 0.5 * dmg.suspension) * if comp_vel > 0.0 { 0.7 } else { 1.3 };
        let x = comp[i];
        let springs = if x >= 0.0 { spec.spring * x + spec.helper_spring * spec.helper_travel } else { spec.helper_spring * (x + spec.helper_travel) };
        let mut fs = springs + damp * comp_vel + spec.arb[axle] * (x - other);
        // Progressive bump stop past full travel (polyurethane: stiff and getting stiffer).
        if over[i] > 0.0 {
            // (Depth capped: a car buried in the ground mustn't be fired out of it.)
            let o = over[i].min(0.12);
            fs += 250_000.0 * o + 2.5e6 * o * o + 6_000.0 * comp_vel.max(0.0);
        }
        let fs = fs.max(0.0);
        w.load = fs;
        at(c, up * fs, &mut force, &mut torque, car.pos);

        // Tyre frame on the ground plane.
        let steer = if i < 2 { car.steer } else { 0.0 };
        let wheel_fwd = car.rot * (Vec3::new(-libm::sinf(steer), 0.0, -libm::cosf(steer)));
        let f = (wheel_fwd - n * wheel_fwd.dot(n)).normalize_or(Vec3::NEG_Z);
        let right = f.cross(n);
        let r = c - car.pos;
        let vc = car.vel + car.ang.cross(r);
        let v_long = vc.dot(f);
        let v_lat = vc.dot(right);

        let surf = ground.surface(c.x, c.z);
        w.surface = Some(surf);
        let ty = tyre(surf);
        let loose = ty.loose;
        // Bent suspension: the tyres don't sit square, less grip.
        let mu = ty.mu * (1.0 - 0.25 * dmg.suspension);
        // Load sensitivity: grip per newton drops with load (~8 % per extra static load).
        let fz = fs.min(14_000.0);
        let nominal = spec.mass * 9.81 * 0.25;
        let mu = mu * (1.0 - 0.08 * (fz / nominal - 1.0)).clamp(0.8, 1.08);
        let peak = mu * fz;

        // Wheel spin: drive, brake, tyre reaction.
        let drive = drive_total * if i < 2 { spec.front_split } else { 1.0 - spec.front_split } * 0.5;
        let mut brake_t = brake * spec.brake_torque * if i < 2 { spec.brake_bias } else { 1.0 - spec.brake_bias } * 0.5;
        // ABS (a driver aid): ease the brake on a wheel that starts to lock. Not on the
        // handbrake, which is meant to lock the rears.
        let abs_at = -1.4 * ty.peak_ratio;
        if spec.abs && w.slip_ratio < abs_at {
            brake_t *= (1.0 - (abs_at - w.slip_ratio) * 4.0).clamp(0.25, 1.0);
        }
        if input.handbrake && i >= 2 {
            brake_t += spec.handbrake_torque * 0.5;
        }
        let denom = v_long.abs().max(3.0);
        let slip_ratio = ((w.spin * spec.wheel_radius - v_long) / denom).clamp(-1.0, 1.0);
        let slip_angle = libm::atan2f(-v_lat, v_long.abs().max(1.5));
        // Combined slip: each slip normalised by its peak, one curve over the combined slip,
        // the force along the slip direction. A locked or spinning wheel (big slip ratio) thus
        // gives up its side force, and a tyre sliding sideways can't also brake hard.
        let (sx, sy) = (slip_ratio / ty.peak_ratio, slip_angle / ty.peak_angle);
        let k = peak * grip_over_slip((sx * sx + sy * sy).sqrt(), ty.tail);
        let fx = k * sx;
        let mut fy = k * sy;
        // Walking pace: viscous grip instead of slip (no jitter at rest).
        let slow = (1.0 - vc.length() / 2.5).clamp(0.0, 1.0);
        let share = spec.mass * 0.25 / h * 0.25;
        fy = fy * (1.0 - slow) + (-v_lat * share).clamp(-peak, peak) * slow;
        // Rolling resistance and loose-surface drag.
        let roll = -(v_long.signum()) * fz * (0.014 + 0.05 * loose) * (v_long.abs() / 1.0).min(1.0);
        at(c, f * (fx + roll) + right * fy, &mut force, &mut torque, car.pos);

        // Wheel angular update, implicit in the tyre's own torque (brakes can't reverse spin
        // within the step). Explicitly, the tyre's grip (dT/dω = slope) made the spin flip
        // sides every substep below ~7 m/s on tarmac, so slip, ABS, traction assist and the
        // tyre audio read noise there.
        let net = drive - fx * spec.wheel_radius;
        let slope = 2.0 * peak / ty.peak_ratio * spec.wheel_radius * spec.wheel_radius / denom;
        // The brake is part of the same solve (applied after it, the tyre had to push back
        // (1 + h·slope/I) times the brake torque and the wheels over-slipped). A brake strong
        // enough to stop the wheel within the step holds it.
        let hi = h / spec.wheel_inertia;
        let free = w.spin + hi * (net + slope * w.spin);
        let b = hi * brake_t;
        let spin = if free.abs() <= b { 0.0 } else { (free - b * free.signum()) / (1.0 + hi * slope) };
        w.spin = spin;
        w.angle = (w.angle + spin * h) % std::f32::consts::TAU;
        w.slip_ratio = slip_ratio;
        w.slip_angle = slip_angle;
    }
    // Holding stopped on the brakes: a stiff spring and damper to where it stopped, along the
    // ground, no stronger than the tyres' grip (a hard enough push still slides it).
    let holding = (brake >= 0.5 || input.handbrake) && pedal < 0.05 && car.wheels.iter().any(|w| w.contact);
    let slow = car.vel.length() < if car.hold.is_some() { 0.6 } else { 0.3 };
    if holding && slow {
        let anchor = *car.hold.get_or_insert(car.pos);
        let grip: f32 = car.wheels.iter().filter(|w| w.contact).map(|w| w.load * w.surface.map_or(0.6, |s| tyre(s).mu)).sum();
        let along = |v: Vec3| v - up * v.dot(up);
        let f = -(along(car.pos - anchor) * 40.0 + along(car.vel) * 12.6) * spec.mass;
        force += f.clamp_length_max(grip * 0.9);
    } else {
        car.hold = None;
    }

    // Limited-slip diffs (viscous): pull each axle's wheels together, then the axles. Without
    // them an unloaded wheel (a rut, a crest) spins up and takes the drive with it. The
    // handbrake disconnects the rear (rally cars have a hydraulic handbrake cutoff).
    // 1 − e^(−x) as a short series (x = lsd·h ≈ 0.13): plain arithmetic, the same bits on
    // every CPU.
    let x = spec.lsd * h;
    let k = x * (1.0 - x * (0.5 - x * (1.0 / 6.0 - x * (1.0 / 24.0 - x / 120.0))));
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
    // Torn bodywork drags.
    force += -car.vel * (0.5 * 1.2 * spec.drag * (1.0 + 0.4 * dmg.worst()) * car.vel.length());
    force += -up * (0.3 * v2);

    // Chassis contact with the ground (rolled, bottomed out) and obstacles.
    let half = spec.size * 0.5;
    let mut hits: Vec<(Vec3, Vec3, f32, Give)> = Vec::new();
    // The hardest new body-on-ground touch this substep: (closing speed, local point).
    let mut touch: Option<(f32, Vec3)> = None;
    let mut body_contact = false;
    // The ground against a 3×2×3 grid over the box (corners, edge and face midpoints).
    for sx in [-1.0f32, 0.0, 1.0] {
        for sy in [-1.0f32, 1.0] {
            for sz in [-1.0f32, 0.0, 1.0] {
                // The floor's ends sit higher (bumper approach / departure angles).
                let lift = if sy < 0.0 && sz != 0.0 { 0.12 } else { 0.0 };
                let local = spec.body_centre + Vec3::new(sx * half.x, sy * half.y + lift, sz * half.z);
                let p = car.pos + car.rot * local;
                let gy = ground.height(p.x, p.z);
                let depth = gy - p.y;
                if depth > 0.0 {
                    let n = ground.normal(p.x, p.z);
                    let vp = car.vel + car.ang.cross(p - car.pos);
                    let vn = vp.dot(n);
                    body_contact = true;
                    if !car.body_contact && -vn > touch.map_or(0.0, |t| t.0) {
                        touch = Some((-vn, local));
                    }
                    let fnorm = (depth.min(0.25) * 250_000.0 - vn * 12_000.0).max(0.0);
                    let vt = vp - n * vn;
                    let ft = -vt.normalize_or_zero() * (fnorm * 0.6).min(vt.length() * spec.mass * 0.5 / h);
                    at(p, n * fnorm + ft, &mut force, &mut torque, car.pos);
                }
            }
        }
    }
    // Obstacles (vertical cylinders) against the body's footprint: the circle against the box
    // in the body's frame, so a trunk can't slip between sample points.
    let bottom = car.pos.y + (car.rot * spec.body_centre).y - half.y;
    car.brush = 0.0;
    for o in obstacles {
        let (c, r, top) = (&o.pos, &o.radius, &o.top);
        if bottom > *top {
            continue;
        }
        if let Give::Breaks { id, .. } = o.give
            && car.broken.contains(&id)
        {
            continue;
        }
        let centre = Vec3::new(c.x, car.pos.y + (car.rot * spec.body_centre).y, c.y);
        let local = car.rot.inverse() * (centre - car.pos) - spec.body_centre;
        let q = Vec2::new(local.x.clamp(-half.x, half.x), local.z.clamp(-half.z, half.z));
        let d = Vec2::new(local.x, local.z) - q;
        let dist = d.length();
        let (n_local, depth) = if dist > 1e-4 {
            if dist >= *r {
                continue;
            }
            (d / dist, r - dist)
        } else {
            // The centre is inside the footprint: out through the nearest side.
            let (ex, ez) = (half.x - local.x.abs(), half.z - local.z.abs());
            if ex < ez { (Vec2::new(local.x.signum(), 0.0), ex + r) } else { (Vec2::new(0.0, local.z.signum()), ez + r) }
        };
        // Contact on the body surface facing the obstacle; the push is horizontal (away from
        // the trunk), the car's frame mapped to the world.
        let p = car.pos + car.rot * (spec.body_centre + Vec3::new(q.x, 0.0, q.y));
        let n = car.rot * Vec3::new(-n_local.x, 0.0, -n_local.y);
        let n = Vec3::new(n.x, 0.0, n.z).normalize_or_zero();
        if let Give::Brush { drag } = o.give {
            // Brush drags on the body where it touches, more the further it's under the car
            // (and a little yaw toward the side in the bush).
            let vp = car.vel + car.ang.cross(p - car.pos);
            let vh = Vec3::new(vp.x, 0.0, vp.z);
            let cover = (depth / r).min(1.0);
            car.brush = car.brush.max(cover);
            at(p, -vh * (spec.mass * drag * cover), &mut force, &mut torque, car.pos);
            continue;
        }
        hits.push((p, n, depth, o.give));
    }

    // Integrate.
    car.vel += force / spec.mass * h;
    let rot = Mat3::from_quat(car.rot);
    let ib = spec.inertia;
    let iw = rot * Mat3::from_diagonal(ib) * rot.transpose();
    let iw_inv = rot * Mat3::from_diagonal(ib.recip()) * rot.transpose();
    let gyro = car.ang.cross(iw * car.ang);
    car.ang += iw_inv * (torque - gyro) * h;
    // The explicit gyroscopic term gains energy at high spin rates (≥ ~50 rad/s it blows up):
    // nothing real spins a car faster than this.
    car.ang = car.ang.clamp_length_max(MAX_SPIN);
    car.pos += car.vel * h;
    let w = car.ang * h;
    if w.length_squared() > 0.0 {
        // Spelled out (not glam's from_scaled_axis / normalize): libm's sin / cos and a plain
        // divide give the same bits on every CPU (glam's NEON normalize multiplies by the
        // reciprocal, SSE2 divides), so a ghost replays the same on arm64 and x86_64.
        let wl = w.length();
        let (sh, ch) = (libm::sinf(wl * 0.5), libm::cosf(wl * 0.5));
        let ax = w / wl;
        let q = Quat::from_xyzw(ax.x * sh, ax.y * sh, ax.z * sh, ch) * car.rot;
        let n = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
        car.rot = Quat::from_xyzw(q.x / n, q.y / n, q.z / n, q.w / n);
    }
    // The body hitting the ground (a roll, nosing into a bank, a heavy landing): damage on the
    // first touch. Softer than a tree: the ground gives and the car slides.
    if let Some((v, local)) = touch {
        let rel = local - spec.body_centre;
        let zone = if rel.y > 0.0 || rel.y < -half.y * 0.9 && rel.z.abs() < half.z * 0.5 {
            HitZone::Body
        } else {
            zone_of(local, half)
        };
        car.damage.hit(Damage::severity(v) * 0.6, local, zone);
    }
    car.body_contact = body_contact;
    // Obstacles: impulses (a penalty spring can't stop a tonne at speed on a 0.4 m trunk).
    // Deepest contact first; each cancels the closing speed (a little bounce, some friction)
    // and pushes the car out.
    hits.sort_by(|a, b| b.2.total_cmp(&a.2));
    for (p, n, depth, give) in hits.into_iter().take(3) {
        let r = p - car.pos;
        let vp = car.vel + car.ang.cross(r);
        let vn = vp.dot(n);
        let rn = r.cross(n);
        let k = 1.0 / spec.mass + rn.dot(iw_inv * rn);
        if let Give::Breaks { id, mass, speed } = give
            && -vn >= speed
        {
            // Snapped: the car takes it along (a plastic hit between the car and a free
            // `mass`), loses that momentum and drives on. No push-out: it's gone.
            car.broken.push(id);
            let imp = n * (-vn / (k + 1.0 / mass));
            car.vel += imp / spec.mass;
            car.ang += iw_inv * r.cross(imp);
            continue;
        }
        if vn < 0.0 {
            if vn < -2.0 {
                // (A sapling, post or sign below its snapping speed is solid too, but that slow
                // a hit is under `DAMAGE_FROM`: no damage.)
                car.impacts += -vn;
                let local = car.rot.inverse() * r;
                car.damage.hit(Damage::severity(-vn), local, zone_of(local, half));
            }
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

/// Front, side or rear, from a point in the body frame (−Z forward).
fn zone_of(local: Vec3, half: Vec3) -> HitZone {
    if local.z < -half.z * 0.55 {
        HitZone::Front
    } else if local.z > half.z * 0.55 {
        HitZone::Rear
    } else {
        HitZone::Side
    }
}

/// Puts the car back on the road (the recover button): at `pos`, facing `heading`, stopped.
/// The damage and what it knocked down stay.
pub fn recover(car: &mut Car, pos: Vec3, heading: f32) {
    let (impacts, damage, broken) = (car.impacts, car.damage, std::mem::take(&mut car.broken));
    *car = Car::new(pos + Vec3::Y * 0.6, heading);
    car.impacts = impacts;
    car.damage = damage;
    car.broken = broken;
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
        // Tarmac with ABS: at least 0.8 g on average. (Measured to the first stop: a car that
        // stops early would otherwise sit on the brake long enough to select reverse.)
        let secs = v / (0.8 * 9.81);
        let mut t = 0.0;
        while car.speed() > 0.5 && t < secs + 1.0 {
            step(&mut car, &spec, Input { brake: 1.0, ..default() }, &g, 1.0 / 60.0);
            t += 1.0 / 60.0;
        }
        assert!(t <= secs, "{t:.1} s braking from {v:.1} m/s (allowed {secs:.1})");
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

    /// Flat tarmac with one obstacle.
    struct One(Obstacle);
    impl Ground for One {
        fn height(&self, _: f32, _: f32) -> f32 {
            0.0
        }
        fn normal(&self, _: f32, _: f32) -> Vec3 {
            Vec3::Y
        }
        fn surface(&self, _: f32, _: f32) -> Surface {
            Surface::Tarmac
        }
        fn obstacles(&self, _: Vec2, _: f32, out: &mut Vec<Obstacle>) {
            out.clear();
            out.push(self.0);
        }
    }

    /// A big trunk 20 m ahead (heading 0 faces +Z).
    const TREE: Obstacle = Obstacle { pos: Vec2::new(0.0, 20.0), radius: 0.4, top: 10.0, give: Give::Solid };
    /// A sapling there: 80 kg, snaps from 3 m/s.
    const SAPLING: Obstacle = Obstacle { pos: Vec2::new(0.0, 20.0), radius: 0.12, top: 4.0, give: Give::Breaks { id: 7, mass: 80.0, speed: 3.0 } };

    #[test]
    fn hits_obstacles() {
        let spec = Spec::default();
        let g = One(TREE);
        let mut car = Car::new(Vec3::new(0.0, 0.6, 0.0), 0.0);
        run(&mut car, &spec, Input::default(), &g, 1.0);
        // Full throttle until it hits, then half a second more. (Held for seconds, the car can
        // shove itself around a trunk: that's fine; passing through it isn't.)
        let mut after = 0;
        for _ in 0..(60 * 6) {
            step(&mut car, &spec, Input { throttle: 1.0, ..default() }, &g, 1.0 / 60.0);
            if car.impacts > 0.0 {
                after += 1;
                if after > 30 {
                    break;
                }
            }
        }
        assert!(car.impacts > 0.0, "never hit the tree: {:?}", car.pos);
        assert!(car.pos.z < 20.0 - 1.5, "drove through the tree: {:?} up {:?}", car.pos, car.up());
    }

    /// The car already rolling at `kmh` toward `obstacle` 20 m ahead, held at that speed until
    /// 3 m before it, then coasting for the rest of 5 s.
    fn drive_at(kmh: f32, obstacle: Obstacle) -> (Car, f32) {
        let spec = Spec::default();
        let g = One(obstacle);
        let mut car = Car::new(Vec3::new(0.0, 0.6, 0.0), 0.0);
        run(&mut car, &spec, Input { handbrake: true, ..default() }, &Flat(0.0, Surface::Tarmac), 1.0);
        car.vel = car.forward() * kmh / 3.6;
        for w in &mut car.wheels {
            w.spin = kmh / 3.6 / spec.wheel_radius;
        }
        let v = kmh / 3.6;
        let mut before = 0.0;
        for _ in 0..300 {
            let err = v - car.speed();
            let input = if car.pos.z < 17.0 { Input { throttle: (err * 0.6).clamp(0.0, 1.0), brake: (-err * 0.4).clamp(0.0, 1.0), ..default() } } else { Input::default() };
            if car.pos.z < 17.0 {
                before = car.speed();
            }
            step(&mut car, &spec, input, &g, 1.0 / 60.0);
        }
        (car, before)
    }

    /// The same coast with nothing in the way: the speed it would have had.
    fn coast_at(kmh: f32) -> Car {
        drive_at(kmh, Obstacle { give: Give::Brush { drag: 0.0 }, ..TREE }).0
    }

    #[test]
    fn big_trees_stop_the_car() {
        for kmh in [20.0, 50.0, 90.0] {
            let (car, _) = drive_at(kmh, TREE);
            assert!(car.pos.z < 20.0 - 1.5 && car.speed() < 3.0, "{kmh} km/h: {:?} at {:.1} m/s", car.pos, car.speed());
            assert!(car.broken.is_empty());
        }
    }

    #[test]
    fn damage_scales_with_the_hit() {
        let knock = drive_at(20.0, TREE).0.damage;
        assert!(knock.worst() < 0.08 && !knock.wrecked(), "{knock:?}");
        let mid = drive_at(50.0, TREE).0.damage;
        assert!(mid.engine > 0.15 && !mid.wrecked(), "{mid:?}");
        let big = drive_at(90.0, TREE).0.damage;
        assert!(big.wrecked(), "{big:?}");
        // (`last` can be a light knock after the rebound, so check the part it broke.)
        assert!(big.hits >= 1 && big.engine >= 1.0, "{big:?}");
    }

    #[test]
    fn damage_costs_power_and_steering() {
        let g = Flat(0.0, Surface::Gravel);
        let spec = Spec::default();
        let drive = |d: Damage, input: Input| {
            let mut car = Car::new(Vec3::new(0.0, 0.6, 0.0), 0.0);
            car.damage = d;
            run(&mut car, &spec, Input::default(), &g, 1.0);
            run(&mut car, &spec, input, &g, 9.0);
            car
        };
        let full = Input { throttle: 1.0, ..default() };
        let ok = drive(Damage::default(), full);
        let hurt = drive(Damage { engine: 0.7, ..Damage::default() }, full);
        assert!(hurt.speed() < ok.speed() * 0.85, "{} vs {}", hurt.speed(), ok.speed());
        let wrecked = drive(Damage { engine: 1.0, ..Damage::default() }, full);
        assert!(wrecked.speed().abs() < 1.0, "wrecked car drove off: {}", wrecked.speed());
        // Bent steering pulls left with the wheel straight.
        let bent = drive(Damage { steering: 0.6, pull: 1.0, ..Damage::default() }, full);
        assert!(bent.steer > 0.02, "{}", bent.steer);
        // Recovering keeps the damage.
        let mut c = hurt.clone();
        recover(&mut c, Vec3::ZERO, 0.0);
        assert_eq!(c.damage, hurt.damage);
    }

    #[test]
    fn hard_hits_knock_parts_off() {
        // Into a trunk nose first: the splitter at 30 km/h, the bumper too at 60; a nudge
        // takes nothing.
        assert_eq!(drive_at(15.0, TREE).0.damage.detached, 0);
        let d = drive_at(35.0, TREE).0.damage;
        assert!(d.off("FrontSplitter") && !d.off("RearWing"), "{d:?}");
        let d = drive_at(60.0, TREE).0.damage;
        assert!(d.off("FrontSplitter") && d.off("FrontBumper"), "{d:?}");
        // Zones: a side hit on the left front takes the left mirror, a hard rear hit the wing.
        let mut side = Damage::default();
        side.hit(0.1, Vec3::new(-0.9, 0.0, -0.5), HitZone::Side);
        assert!(side.off("MirrorL") && !side.off("MirrorR"), "{side:?}");
        let mut rear = Damage::default();
        rear.hit(0.4, Vec3::new(0.0, 0.0, 2.0), HitZone::Rear);
        assert!(rear.off("RearWing") && rear.off("RearBumper") && rear.off("RearDiffuser"), "{rear:?}");
        // They stay off through a recover.
        let mut c = drive_at(60.0, TREE).0;
        let before = c.damage.detached;
        recover(&mut c, Vec3::ZERO, 0.0);
        assert_eq!(c.damage.detached, before);
    }

    #[test]
    fn saplings_snap_and_the_car_drives_on() {
        let (car, before) = drive_at(60.0, SAPLING);
        assert_eq!(car.broken, vec![7], "{:?}", car.pos);
        assert!(car.pos.z > 25.0, "stopped at the sapling: {:?}", car.pos);
        // Snapping it does no damage.
        assert_eq!(car.impacts, 0.0);
        assert_eq!(car.damage, Damage::default());
        // It costs a little speed (the sapling's momentum), compared with an empty road.
        let free = coast_at(60.0);
        let lost = (free.speed() - car.speed()) * 3.6;
        assert!(lost > 0.5 && lost < 8.0, "lost {lost:.2} km/h (from {:.1})", before * 3.6);
    }

    #[test]
    fn a_nudge_does_not_snap_a_sapling() {
        // Walking pace (5 km/h, under its 3 m/s): it holds and the car stops against it.
        let (car, _) = drive_at(5.0, SAPLING);
        assert!(car.broken.is_empty() && car.pos.z < 20.0 - 1.5, "{:?} {:?}", car.broken, car.pos);
        // A light hit: under the speed that damages anything.
        assert_eq!(car.damage.worst(), 0.0, "{:?}", car.damage);
    }

    #[test]
    fn shrubs_slow_the_car_a_little() {
        let shrub = Obstacle { pos: Vec2::new(0.4, 20.0), radius: 0.9, top: 1.0, give: Give::Brush { drag: 0.15 } };
        let (car, _) = drive_at(60.0, shrub);
        let free = coast_at(60.0);
        let lost = (free.speed() - car.speed()) * 3.6;
        assert!(lost > 0.5 && lost < 6.0, "lost {lost:.2} km/h through a shrub");
        assert!(car.broken.is_empty() && car.impacts == 0.0 && car.damage == Damage::default());
        assert!(car.pos.z > 30.0, "{:?}", car.pos);
    }

    /// Speed lost (km/h) to each kind of roadside thing, for tuning:
    /// `cargo test --lib feel_numbers -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn feel_numbers() {
        let things = [
            ("post", Obstacle { radius: 0.07, top: 1.05, give: Give::Breaks { id: 1, mass: 6.0, speed: 1.0 }, ..TREE }),
            ("sign", Obstacle { radius: 0.09, top: 2.0, give: Give::Breaks { id: 1, mass: 25.0, speed: 2.0 }, ..TREE }),
            ("sapling 0.25", Obstacle { radius: 0.075, top: 2.8, give: Give::Breaks { id: 1, mass: 62.5, speed: 3.0 }, ..TREE }),
            ("sapling 0.42", Obstacle { radius: 0.126, top: 4.6, give: Give::Breaks { id: 1, mass: 105.0, speed: 3.0 }, ..TREE }),
            ("shrub 1.0", Obstacle { pos: Vec2::new(0.4, 20.0), radius: 0.85, top: 1.1, give: Give::Brush { drag: 0.15 } }),
            ("shrub 1.3", Obstacle { pos: Vec2::new(0.4, 20.0), radius: 1.1, top: 1.4, give: Give::Brush { drag: 0.195 } }),
        ];
        for kmh in [30.0, 60.0, 100.0] {
            let free = coast_at(kmh).speed();
            for (name, o) in things {
                let (car, _) = drive_at(kmh, o);
                println!("{kmh:5.0} km/h {name:13} lost {:5.2} km/h broken {}", (free - car.speed()) * 3.6, !car.broken.is_empty());
            }
        }
    }

    #[test]
    fn breaking_is_deterministic() {
        let a = drive_at(60.0, SAPLING).0;
        let b = drive_at(60.0, SAPLING).0;
        assert_eq!(a, b);
    }

    fn default() -> Input {
        Input::default()
    }
}

#[cfg(test)]
#[path = "handling_tests.rs"]
mod handling_tests;

/// A fixed drive for checking the sim gives the same bits everywhere: the unit test
/// `scripted_drive_is_bit_identical` pins its hash, and `tools/simcheck` (which compiles this
/// file on its own) compares it between CPUs.
#[doc(hidden)]
pub mod check {
    use super::*;

    /// Rolling hills from plain arithmetic (no transcendentals), gravel and tarmac, one tree, and
    /// a grid of saplings and shrubs (every 10 m, alternating) to snap and plough through.
    struct Hills;

    impl Ground for Hills {
        fn height(&self, x: f32, z: f32) -> f32 {
            let (a, b) = (x * 0.05, z * 0.04);
            (a - a * a * a / 6.0) * 1.5 + (b - b * b * b / 6.0) * 1.2
        }
        fn normal(&self, x: f32, z: f32) -> Vec3 {
            let (a, b) = (x * 0.05, z * 0.04);
            Vec3::new(-(1.0 - a * a * 0.5) * 0.075, 1.0, -(1.0 - b * b * 0.5) * 0.048).normalize()
        }
        fn surface(&self, x: f32, _: f32) -> Surface {
            if x > 0.0 { Surface::Gravel } else { Surface::Tarmac }
        }
        fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
            out.clear();
            out.push(Obstacle::solid(Vec2::new(14.0, 60.0), 0.4, 10.0));
            let (i0, i1) = (((p.x - r) / 10.0).floor() as i32, ((p.x + r) / 10.0).ceil() as i32);
            let (j0, j1) = (((p.y - r) / 10.0).floor() as i32, ((p.y + r) / 10.0).ceil() as i32);
            for i in i0..=i1 {
                for j in j0..=j1 {
                    if !(-20..=20).contains(&i) || !(-20..=20).contains(&j) {
                        continue;
                    }
                    let c = Vec2::new(i as f32 * 10.0 + 5.0, j as f32 * 10.0 + 3.0);
                    let id = ((i + 20) * 41 + j + 20) as u32;
                    let give = if (i + j) % 2 == 0 { Give::Breaks { id, mass: 80.0, speed: 3.0 } } else { Give::Brush { drag: 0.15 } };
                    let radius = if (i + j) % 2 == 0 { 0.1 } else { 0.9 };
                    out.push(Obstacle { pos: c, radius, top: self.height(c.x, c.y) + 3.0, give });
                }
            }
        }
    }

    /// Hash of every step's state over a minute of scripted driving (throttle, brakes,
    /// steering both ways, handbrake, a tree, saplings and shrubs).
    pub fn scripted_hash() -> u64 {
        scripted().0
    }

    /// The scripted drive: its hash, saplings snapped, and steps spent in brush.
    pub fn scripted() -> (u64, usize, usize) {
        let spec = Spec::default();
        let mut car = Car::new(Vec3::new(0.0, 1.2, 0.0), 0.3);
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut in_brush = 0;
        let mut mix = |v: f32| {
            h ^= v.to_bits() as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        for i in 0..3600 {
            let input = Input {
                steer: if (i / 90) % 3 == 0 { 0.0 } else if (i / 90) % 3 == 1 { 1.0 } else { -0.6 },
                throttle: if i % 400 < 300 { 1.0 } else { 0.0 },
                brake: if i % 400 >= 330 { 0.8 } else { 0.0 },
                handbrake: i % 700 > 680,
            };
            step(&mut car, &spec, input, &Hills, 1.0 / 60.0);
            for v in [car.pos.x, car.pos.y, car.pos.z, car.rot.x, car.rot.y, car.rot.z, car.rot.w, car.vel.x, car.vel.y, car.vel.z, car.steer, car.rpm, car.brush, car.broken.len() as f32] {
                mix(v);
            }
            in_brush += (car.brush > 0.0) as usize;
        }
        (h, car.broken.len(), in_brush)
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
