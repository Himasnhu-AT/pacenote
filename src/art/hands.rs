//! The driver's gloved hands and arms in the cockpit, and the gear lever and handbrake they work.
//!
//! - The left hand stays on the steering wheel and turns with it.
//! - The right hand (left-hand-drive car: the levers are on the console to the driver's right)
//!   holds the wheel too, until the gearbox changes gear or the handbrake goes on. Then it
//!   reaches for the lever (~0.26 s, eased, along a small arc), works it and goes back to the
//!   wheel (~0.32 s). The lever only moves while the hand is on it.
//! - Shifter (sequential): pulled back for an upshift, pushed forward for a downshift, then it
//!   springs back to the centre. Reverse is a gate: across towards the driver and forward, and
//!   it stays there until the box leaves reverse.
//! - Handbrake (hydraulic, vertical): pulled back while the handbrake is held, at least
//!   [`MIN_PULL`] s for a tap.
//!
//! The levers are the model's `Shifter` and `Handbrake` nodes (`tools/art/cockpit.py`, put in by
//! `build_rallycar.py` or `import_sketchfab.py`), tilted about their foot. Hands and arms are a few shared primitive meshes (finger rings
//! around the grip, an ellipsoid palm, thumb, cuff with an orange band, sleeve capsules); the
//! arms bend with two-bone IK from fixed shoulders. They show from every camera, joined to the
//! model's seated `DriverBody` (car.rs hides that body in the driver's own cockpit view).
//!
//! Everything here is per-frame transform maths: no allocations after spawning. The timing
//! logic ([`Driver`]) is pure and tested.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::math::{Affine3A, Vec3A};
use bevy::prelude::*;
use engine::render_budget::Actor;

use super::car::{CarModel, CarPose, CarRig, STEERING_RATIO};
use crate::drive::{DriverInput, DrivenCar};

// ---------------------------------------------------------------- the model (glTF frame)
// Mirrors tools/art/cockpit.py: Blender (x, y, z) → model (x, z, −y).

/// Where the driver sits in this body relative to the cockpit the rig was laid out for
/// (`tools/art/cockpit.py`): a downloaded body converted by `import_sketchfab.py` moves the
/// cockpit nodes by this (it prints it). Points of the rig are cockpit.py's + `RIG`; zero for
/// the procedural rally hatch (`build_rallycar.py`).
const RIG: Vec3 = Vec3::ZERO;

const fn rig(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x + RIG.x, y + RIG.y, z + RIG.z)
}

/// Steering wheel centre, its rest rotation (column tilted 22°) and rim radius
/// (`cockpit.py` `STEER_C`, `STEER_TILT`, `RIM`).
const WHEEL_C: Vec3 = rig(-0.37, 0.515, 0.17);
const WHEEL_TILT: f32 = -22.0 * PI / 180.0;
const RIM: f32 = 0.175;

/// Shifter: pivot, knob centre and stick direction (in the lever's frame).
const SHIFT_PIVOT: Vec3 = rig(-0.10, 0.255, 0.05);
const SHIFT_GRIP: Vec3 = Vec3::new(-0.03, 0.30, 0.05);
const SHIFT_AXIS: Vec3 = Vec3::new(-0.03, 0.28, 0.05);
/// Handbrake: pivot, grip centre and stick direction.
const HB_PIVOT: Vec3 = rig(-0.03, 0.255, 0.20);
const HB_GRIP: Vec3 = Vec3::new(-0.0086, 0.2137, 0.0342);
const HB_AXIS: Vec3 = Vec3::new(-0.01, 0.25, 0.04);

/// Shoulders (left, right): 0.17 m below the eye, a little behind it.
const SHOULDER: [Vec3; 2] = [rig(-0.58, 0.575, 0.74), rig(-0.16, 0.575, 0.74)];
/// Which way each elbow points (out and down).
const POLE: [Vec3; 2] = [Vec3::new(-0.9, -0.8, 0.2), Vec3::new(0.6, -0.9, 0.2)];
/// Upper arm and forearm (elbow to wrist) lengths.
const UPPER: f32 = 0.31;
const FORE: f32 = 0.30;
/// Bulge of the hand's path between wheel and lever.
const TRAVEL_ARC: Vec3 = Vec3::new(0.0, -0.01, 0.04);
/// Wrist behind the grip centre, along the hand's −Z.
const WRIST: f32 = 0.10;

/// Grip on the rim above the horizontal ("quarter to three"), and how far the hands follow the
/// wheel round before they slip on the rim (rad, soft limit).
const GRIP_UP: f32 = 0.22;
const HAND_FOLLOW: f32 = 0.45;
/// Most the wrist bends sideways off the forearm before the grip slips round the rim (rad),
/// and the slip search's step and reach.
const MAX_WRIST: f32 = 30.0 * PI / 180.0;
const SLIP_STEP: f32 = 0.06;
const SLIP_STEPS: usize = 10;
/// Most the bar may cross the fist off square (rad).
const FIST_TILT: f32 = 25.0 * PI / 180.0;

// ---------------------------------------------------------------- timing

/// Hand travel wheel → lever and lever → wheel (s).
pub const REACH_LEVER: f32 = 0.26;
pub const REACH_WHEEL: f32 = 0.32;
/// One shift: lever out, hold, back (s).
pub const SHIFT_OP: f32 = 0.24;
/// Handbrake lever travel (s) and the least time it stays up for a tap.
pub const HB_TRAVEL: f32 = 0.12;
pub const MIN_PULL: f32 = 0.15;

/// Lever angles (rad): shift throw; reverse gate forward and across; handbrake pull.
const SHIFT_TILT: f32 = 0.26;
const REV_PITCH: f32 = -0.22;
const REV_ROLL: f32 = 0.20;
const HB_PULL: f32 = 0.26;

/// Where the right hand is (going).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    Wheel,
    Shifter,
    Handbrake,
}

/// A gear change for the hand to make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shift {
    Up,
    Down,
    IntoReverse,
    OutOfReverse,
}

impl Shift {
    fn between(from: i8, to: i8) -> Self {
        match (from, to) {
            (_, t) if t < 0 => Shift::IntoReverse,
            (f, _) if f < 0 => Shift::OutOfReverse,
            (f, t) if t > f => Shift::Up,
            _ => Shift::Down,
        }
    }
}

/// The right hand's errand and the levers' travel, from the gearbox and the handbrake input.
#[derive(Clone, Debug)]
pub struct Driver {
    gear: i8,
    /// A gear change waiting for the hand (the latest wins).
    queued: Option<Shift>,
    /// The shift being made, and seconds into it.
    shifting: Option<(Shift, f32)>,
    pub job: Job,
    /// Seconds since `job` began.
    pub t: f32,
    /// Handbrake lever travel 0..1 (linear) and the hold left for a tap.
    hb: f32,
    hb_hold: f32,
    hb_was: bool,
    /// The shifter sits in the reverse gate.
    reverse: bool,
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

impl Driver {
    pub fn new(gear: i8) -> Self {
        Driver { gear, queued: None, shifting: None, job: Job::Wheel, t: 1.0, hb: 0.0, hb_hold: 0.0, hb_was: false, reverse: gear < 0 }
    }

    /// Seconds the hand takes to get to `job`.
    pub fn reach_time(job: Job) -> f32 {
        if job == Job::Wheel { REACH_WHEEL } else { REACH_LEVER }
    }

    /// 0 → 1 over the reach (eased).
    pub fn reach(&self) -> f32 {
        smooth(self.t / Self::reach_time(self.job))
    }

    pub fn arrived(&self) -> bool {
        self.t >= Self::reach_time(self.job)
    }

    /// Advances by `dt`; returns whether the hand set off somewhere new.
    pub fn step(&mut self, dt: f32, gear: i8, handbrake: bool) -> bool {
        if gear != self.gear {
            self.queued = Some(Shift::between(self.gear, gear));
            self.gear = gear;
        }
        if handbrake && !self.hb_was {
            self.hb_hold = MIN_PULL;
        }
        self.hb_was = handbrake;
        if let Some((k, ts)) = &mut self.shifting {
            *ts += dt;
            if *ts >= SHIFT_OP {
                match k {
                    Shift::IntoReverse => self.reverse = true,
                    Shift::OutOfReverse => self.reverse = false,
                    _ => {}
                }
                self.shifting = None;
            }
        }
        // A shift under way finishes first; then the handbrake; then a waiting shift.
        let brake = handbrake || self.hb_hold > 0.0 || self.hb > 0.0;
        let want = if self.shifting.is_some() {
            Job::Shifter
        } else if brake {
            Job::Handbrake
        } else if self.queued.is_some() {
            Job::Shifter
        } else {
            Job::Wheel
        };
        let moved = want != self.job;
        if moved {
            self.job = want;
            self.t = 0.0;
        } else {
            self.t += dt;
        }
        let there = self.arrived();
        if self.job == Job::Shifter && there && self.shifting.is_none() {
            self.shifting = self.queued.take().map(|k| (k, 0.0));
        }
        if self.job == Job::Handbrake && there && (handbrake || self.hb_hold > 0.0) {
            self.hb = (self.hb + dt / HB_TRAVEL).min(1.0);
            if self.hb >= 1.0 {
                self.hb_hold = (self.hb_hold - dt).max(0.0);
            }
        } else {
            self.hb = (self.hb - dt / HB_TRAVEL).max(0.0);
        }
        moved
    }

    /// Shifter tilt (pitch about the lever's X: + pulls the top back; roll about its Z: + leans
    /// it towards the driver).
    pub fn shifter(&self) -> (f32, f32) {
        let gate = match self.shifting {
            Some((Shift::IntoReverse, ts)) => smooth(ts / SHIFT_OP),
            Some((Shift::OutOfReverse, ts)) => 1.0 - smooth(ts / SHIFT_OP),
            _ if self.reverse => 1.0,
            _ => 0.0,
        };
        let throw = match self.shifting {
            Some((Shift::Up, ts)) => pulse(ts),
            Some((Shift::Down, ts)) => -pulse(ts),
            _ => 0.0,
        };
        (gate * REV_PITCH + throw * SHIFT_TILT, gate * REV_ROLL)
    }

    /// Handbrake pull (rad, top back).
    pub fn handbrake(&self) -> f32 {
        smooth(self.hb) * HB_PULL
    }
}

/// A sequential throw: out in 0.08 s, held, back by the end of [`SHIFT_OP`].
fn pulse(ts: f32) -> f32 {
    if ts < 0.08 {
        smooth(ts / 0.08)
    } else if ts < 0.13 {
        1.0
    } else {
        1.0 - smooth((ts - 0.13) / (SHIFT_OP - 0.13))
    }
}

// ---------------------------------------------------------------- poses

/// A hand pose in the model frame. The hand's frame: X along the gripped bar (index finger at
/// +X for the right hand, −X for the left), Y out of the back of the hand, −Z towards the wrist.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub pos: Vec3,
    pub rot: Quat,
}

fn frame(x: Vec3, z: Vec3) -> Quat {
    let x = x.normalize();
    let z = (z - x * z.dot(x)).normalize();
    Quat::from_mat3(&Mat3::from_cols(x, z.cross(x), z))
}

/// A hand on the rim, `side` 0 left / 1 right, the wheel turned `wheel` rad (anticlockwise as
/// the driver sees it).
fn wheel_grip(side: usize, wheel: f32) -> Pose {
    let follow = HAND_FOLLOW * (wheel / HAND_FOLLOW).tanh();
    let base = if side == 0 { PI - GRIP_UP } else { GRIP_UP };
    let target = base + follow;
    // The nearest grip to where the rim has turned the hand that keeps the wrist straight
    // enough; past that the hand slips round the rim (towards quarter to three first).
    let back = if follow > 0.0 { -1.0 } else { 1.0 };
    for k in 0..=SLIP_STEPS {
        for dir in [back, -back] {
            let (pose, bend) = rim_pose(side, target + dir * k as f32 * SLIP_STEP);
            if bend <= MAX_WRIST {
                return pose;
            }
            if k == 0 {
                break;
            }
        }
    }
    rim_pose(side, target).0
}

/// A hand on the rim at angle `phi` (in the wheel's plane, anticlockwise from 3 o'clock as the
/// driver sees it), and how far its wrist bends.
fn rim_pose(side: usize, phi: f32) -> (Pose, f32) {
    let rest = Quat::from_rotation_x(WHEEL_TILT);
    let r = Vec3::new(phi.cos(), phi.sin(), 0.0);
    // The rim runs through the fist (anticlockwise tangent), the back of the hand outward.
    let x = rest * r.cross(Vec3::NEG_Z);
    aligned(side, WHEEL_C + rest * (r * RIM), x, FIST_TILT)
}

/// The right hand on a lever (pivot, grip and stick axis in the lever's frame), tilted `rot`.
fn lever_grip(pivot: Vec3, grip: Vec3, axis: Vec3, rot: Quat) -> Pose {
    aligned(1, pivot + rot * grip, rot * axis, FIST_TILT).0
}

/// A hand gripping a bar through `pos` along `x`, its wrist straight: the hand points along the
/// forearm (as far as gripping that bar allows). A few passes settle wrist, elbow and hand.
/// Returns the pose and the wrist's bend (rad) the bar forces.
fn aligned(side: usize, pos: Vec3, bar: Vec3, tilt: f32) -> (Pose, f32) {
    let bar = bar.normalize();
    let mut fore = (pos - SHOULDER[side]).normalize_or(Vec3::NEG_Z);
    let mut x = bar;
    let mut rot = frame(x, fore - x * fore.dot(x) + Vec3::NEG_Z * 1e-3);
    for _ in 0..5 {
        let wrist = pos + rot * Vec3::new(0.0, 0.0, -WRIST);
        let e = elbow(SHOULDER[side], wrist, POLE[side]);
        fore = (wrist - e).normalize_or(fore);
        // The bar may cross the fist a little diagonally (a natural grip): turn the fist's axis
        // from the bar towards square to the forearm, at most `tilt`.
        let square = (bar - fore * bar.dot(fore)).normalize_or(bar);
        let ang = bar.angle_between(square);
        x = if ang > 1e-4 { Quat::from_rotation_arc(bar, square).slerp(Quat::IDENTITY, 1.0 - (tilt / ang).min(1.0)) * bar } else { bar };
        if fore.dot(x).abs() < 0.98 {
            rot = frame(x, fore);
        }
    }
    (Pose { pos, rot }, fore.dot(x).abs().clamp(0.0, 1.0).asin())
}

fn lever_rot(pitch: f32, roll: f32) -> Quat {
    Quat::from_rotation_x(pitch) * Quat::from_rotation_z(roll)
}

/// The hand on its way between two poses, `k` 0 → 1 (eased): drawn back towards the driver
/// a little, clear of the rim and the console.
fn travel(from: Pose, to: Pose, k: f32) -> Pose {
    let arc = (PI * k).sin();
    Pose { pos: from.pos.lerp(to.pos, k) + TRAVEL_ARC * arc, rot: from.rot.slerp(to.rot, k) }
}

/// Wrist and elbow for a hand pose.
fn arm(side: usize, pose: Pose) -> (Vec3, Vec3) {
    let wrist = pose.pos + pose.rot * Vec3::new(0.0, 0.0, -WRIST);
    (wrist, elbow(SHOULDER[side], wrist, POLE[side]))
}

/// Where the elbow goes for a shoulder `s` and wrist `w` (two-bone IK, bending towards `pole`).
pub fn elbow(s: Vec3, w: Vec3, pole: Vec3) -> Vec3 {
    elbow_with(s, w, pole, UPPER, FORE)
}

/// Two-bone IK with given upper-arm and forearm lengths.
fn elbow_with(s: Vec3, w: Vec3, pole: Vec3, upper: f32, fore: f32) -> Vec3 {
    let d = w - s;
    let len = d.length().max(1e-4);
    let dir = d / len;
    let l = len.clamp((upper - fore).abs() + 1e-3, upper + fore - 1e-3);
    let a = (upper * upper - fore * fore + l * l) / (2.0 * l);
    let h = (upper * upper - a * a).max(0.0).sqrt();
    let p = (pole - dir * pole.dot(dir)).normalize_or(dir.any_orthonormal_vector());
    s + dir * a + p * h
}

// ---------------------------------------------------------------- entities

/// Shared meshes and materials (made once).
#[derive(Resource, Clone)]
struct ArmAssets {
    finger: Handle<Mesh>,
    palm: Handle<Mesh>,
    thumb: Handle<Mesh>,
    cuff: Handle<Mesh>,
    band: Handle<Mesh>,
    fore: Handle<Mesh>,
    upper: Handle<Mesh>,
    glove: Handle<StandardMaterial>,
    accent: Handle<StandardMaterial>,
    suit: Handle<StandardMaterial>,
}

impl FromWorld for ArmAssets {
    fn from_world(world: &mut World) -> Self {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let finger = meshes.add(Torus { minor_radius: 0.0105, major_radius: 0.027 }.mesh().minor_resolution(8).major_resolution(14));
        let palm = meshes.add(Sphere::new(1.0).mesh().uv(14, 9));
        let thumb = meshes.add(Capsule3d::new(0.011, 0.04).mesh().rings(1).latitudes(6).longitudes(8));
        let cuff = meshes.add(Cylinder::new(0.036, 0.05).mesh().resolution(12));
        let band = meshes.add(Cylinder::new(0.0372, 0.007).mesh().resolution(12));
        let fore = meshes.add(Capsule3d::new(0.038, FORE).mesh().rings(1).latitudes(6).longitudes(12));
        let upper = meshes.add(Capsule3d::new(0.048, UPPER).mesh().rings(1).latitudes(6).longitudes(12));
        let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
        let glove = mats.add(StandardMaterial { base_color: Color::srgb(0.24, 0.24, 0.26), perceptual_roughness: 0.7, reflectance: 0.35, ..default() });
        let accent = mats.add(StandardMaterial { base_color: Color::linear_rgb(1.0, 0.42, 0.04), perceptual_roughness: 0.5, ..default() });
        let suit = mats.add(StandardMaterial { base_color: Color::srgb(0.16, 0.28, 0.55), perceptual_roughness: 0.85, ..default() });
        ArmAssets { finger, palm, thumb, cuff, band, fore, upper, glove, accent, suit }
    }
}

/// The arms rig (on its root, a child of the car model).
#[derive(Component)]
pub struct Arms {
    car: Entity,
    hand: [Entity; 2],
    fore: [Entity; 2],
    upper: [Entity; 2],
    /// Shifter and handbrake nodes, once the model is in (None: not looked for yet).
    levers: Option<[Option<Entity>; 2]>,
    pub driver: Driver,
    /// The right hand: where its current reach started, and where it is.
    from: Pose,
    now: Option<Pose>,
    /// The downloaded arms (`driver_arms.glb`): its scene root, and its rig once found.
    model: Option<Entity>,
    skinned: Option<Box<Skinned>>,
}

/// Adds hands and arms under the car model `model` of `car`. Visual worlds only.
pub fn spawn(world: &mut World, car: Entity, model: Entity) {
    if !world.contains_resource::<ArmAssets>() {
        let a = ArmAssets::from_world(world);
        world.insert_resource(a);
    }
    let a = world.resource::<ArmAssets>().clone();
    let gear = world.get::<DrivenCar>(car).map(|c| c.car.gear).or_else(|| world.get::<CarPose>(car).map(|p| p.gear)).unwrap_or(1);
    let part = |mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, tf: Transform| (Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), tf, Actor);
    let mut hand = [Entity::PLACEHOLDER; 2];
    let mut fore = [Entity::PLACEHOLDER; 2];
    let mut upper = [Entity::PLACEHOLDER; 2];
    let root = world.spawn((Name::new("driver arms"), Transform::IDENTITY, Visibility::Hidden, Actor)).id();
    for side in 0..2 {
        // Thumb on the index end of the fist: −X for the left hand, +X for the right.
        let s = if side == 0 { -1.0 } else { 1.0 };
        let h = world.spawn((Name::new(if side == 0 { "left hand" } else { "right hand" }), Transform::IDENTITY, Visibility::Inherited, Actor)).id();
        // Four fingers curled round the bar (X), little finger a bit smaller.
        for (i, x) in [-0.031f32, -0.0105, 0.0105, 0.031].into_iter().enumerate() {
            let k = if (i == 0 && s > 0.0) || (i == 3 && s < 0.0) { 0.9 } else { 1.0 };
            let tf = Transform::from_xyz(x, 0.0, 0.0).with_rotation(Quat::from_rotation_z(FRAC_PI_2)).with_scale(Vec3::splat(k));
            world.spawn((part(&a.finger, &a.glove, tf), ChildOf(h)));
        }
        // Palm and back of the hand, behind the fist towards the wrist.
        world.spawn((part(&a.palm, &a.glove, Transform::from_xyz(0.0, 0.006, -0.05).with_scale(Vec3::new(0.047, 0.03, 0.05))), ChildOf(h)));
        // Thumb: from beside the index finger along the inside of the bar, towards the little
        // finger (on the wheel it rests over the spoke below the hand).
        let (t0, t1) = (Vec3::new(s * 0.040, -0.016, -0.042), Vec3::new(-s * 0.002, -0.032, -0.010));
        let tf = Transform::from_translation((t0 + t1) * 0.5).with_rotation(Quat::from_rotation_arc(Vec3::Y, (t1 - t0).normalize()));
        world.spawn((part(&a.thumb, &a.glove, tf), ChildOf(h)));
        // Gauntlet cuff with an orange band at its edge.
        let along_z = Quat::from_rotation_x(FRAC_PI_2);
        world.spawn((part(&a.cuff, &a.glove, Transform::from_xyz(0.0, 0.0, -0.105).with_rotation(along_z)), ChildOf(h)));
        world.spawn((part(&a.band, &a.accent, Transform::from_xyz(0.0, 0.0, -0.127).with_rotation(along_z)), ChildOf(h)));
        world.entity_mut(h).insert(ChildOf(root));
        hand[side] = h;
        fore[side] = world.spawn((part(&a.fore, &a.suit, Transform::IDENTITY), ChildOf(root))).id();
        upper[side] = world.spawn((part(&a.upper, &a.suit, Transform::IDENTITY), ChildOf(root))).id();
    }
    let from = wheel_grip(1, 0.0);
    // The downloaded arms, when converted (tools/art/import_sketchfab.py arms): posed from the
    // same targets; the code-built hands stay until its rig is found (and if it never is).
    let arms_model = world.resource::<engine::paths::AssetsDir>().has(ARMS_GLB).then(|| {
        let scene = world.resource::<AssetServer>().load(GltfAssetLabel::Scene(0).from_asset(ARMS_GLB));
        world.spawn((Name::new("driver arms model"), ArmsModel, WorldAssetRoot(scene), Transform::IDENTITY, Visibility::Inherited, Actor, ChildOf(root))).id()
    });
    world.entity_mut(root).insert(Arms { car, hand, fore, upper, levers: None, driver: Driver::new(gear), from, now: None, model: arms_model, skinned: None });
    world.entity_mut(model).add_child(root);
}

// ---------------------------------------------------------------- downloaded arms

/// The converted arms model (see `tools/art/import_sketchfab.py`); optional.
pub const ARMS_GLB: &str = "models/driver_arms.glb";
const FINGER_NAMES: [&str; 5] = ["Thumb", "Index", "Middle", "Ring", "Pinky"];
/// Finger curl added to the model's rest pose for a grip (rad, per joint), thumb less.
const CURL: [f32; 3] = [1.15, 1.25, 0.85];
const THUMB_CURL: [f32; 3] = [0.25, 0.45, 0.45];

#[derive(Component)]
struct ArmsModel;

/// One arm of the downloaded model: its joints and what the rest pose says about them, all in
/// the arms root's frame (the car model's).
struct SkinSide {
    upper: Entity,
    fore: Entity,
    hand: Entity,
    fingers: [[Option<(Entity, Quat, Vec3)>; 3]; 5],
    /// Rest: the upper arm's parent (model frame), the joints' rest rotations (model frame) and
    /// directions, segment lengths.
    upper_parent: Affine3A,
    upper_rot: Quat,
    fore_rot: Quat,
    upper_dir: Vec3,
    fore_dir: Vec3,
    upper_len: f32,
    fore_len: f32,
    /// Hand joint rotation → our hand frame (X along the bar, Y back of the hand, Z to the
    /// fingers), and the grip centre in our hand frame relative to the wrist joint.
    to_frame: Quat,
    grip: Vec3,
}

pub struct Skinned {
    sides: [SkinSide; 2],
}

/// The model-frame transform of `e` (under `stop`) from local transforms.
fn model_affine(e: Entity, stop: Entity, parents: &Query<&ChildOf>, tfs: &Query<&Transform>) -> Affine3A {
    let mut m = Affine3A::IDENTITY;
    let mut cur = e;
    while cur != stop {
        let Ok(t) = tfs.get(cur) else { break };
        m = t.compute_affine() * m;
        let Ok(p) = parents.get(cur) else { break };
        cur = p.parent();
    }
    m
}

fn rot_of(a: &Affine3A) -> Quat {
    Quat::from_mat3a(&a.matrix3).normalize()
}

/// Finds the arm joints once the model's scene is in, moves the model so its shoulders sit on
/// ours, measures the rest pose, and hides the code-built arms.
#[allow(clippy::type_complexity, clippy::too_many_arguments, clippy::needless_range_loop)]
pub fn find_arms(
    mut commands: Commands,
    mut arms: Query<(Entity, &mut Arms)>,
    children: Query<&Children>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    mut tfs: ParamSet<(Query<&Transform>, Query<&mut Transform>)>,
    meshes: Query<(), With<Mesh3d>>,
    mut vis: Query<&mut Visibility>,
) {
    for (root, mut a) in &mut arms {
        let (Some(model), None) = (a.model, a.skinned.as_ref()) else { continue };
        let mut joints = std::collections::HashMap::new();
        for e in children.iter_descendants(model) {
            if let Ok(n) = names.get(e) {
                joints.insert(n.as_str().to_string(), e);
            }
            if meshes.contains(e) {
                commands.entity(e).insert((Actor, bevy::camera::visibility::NoFrustumCulling));
            }
        }
        let need = |n: &str| joints.get(n).copied();
        let (Some(ul), Some(ur)) = (need("UpperArm.L"), need("UpperArm.R")) else { continue };
        let (Some(fl), Some(fr), Some(hl), Some(hr)) = (need("ForeArm.L"), need("ForeArm.R"), need("Hand.L"), need("Hand.R")) else { continue };
        // Shoulders onto ours.
        let shift = {
            let q = tfs.p0();
            let pl = model_affine(ul, root, &parents, &q).translation;
            let pr = model_affine(ur, root, &parents, &q).translation;
            (SHOULDER[0] + SHOULDER[1]) * 0.5 - Vec3::from((pl + pr) * Vec3A::splat(0.5))
        };
        if let Ok(mut t) = tfs.p1().get_mut(model) {
            t.translation = shift;
        }
        let q = tfs.p0();
        let mut sides = Vec::with_capacity(2);
        for (side, (u, f, h)) in [(ul, fl, hl), (ur, fr, hr)].into_iter().enumerate() {
            let s = if side == 0 { "L" } else { "R" };
            // Model-frame rest transforms (the scene root already carries the shift).
            let at = |e: Entity| model_affine(e, root, &parents, &q);
            let (mu, mf, mh) = (at(u), at(f), at(h));
            let (pu, pf, ph) = (Vec3::from(mu.translation), Vec3::from(mf.translation), Vec3::from(mh.translation));
            let parent = parents.get(u).map(|p| p.parent()).unwrap_or(model);
            // Fingers: chains Thumb1..3 etc.; their roots give the hand's frame.
            let mut fingers = [[None; 3]; 5];
            let mut roots = [None; 5];
            for (i, fname) in FINGER_NAMES.iter().enumerate() {
                for k in 0..3 {
                    if let Some(e) = need(&format!("{fname}{}.{s}", k + 1)) {
                        fingers[i][k] = Some((e, Quat::IDENTITY, Vec3::X));
                        if k == 0 {
                            roots[i] = Some(Vec3::from(at(e).translation));
                        }
                    }
                }
            }
            let knuckles: Vec<Vec3> = roots[1..].iter().flatten().copied().collect();
            let kc = if knuckles.is_empty() { pf + (ph - pf) * 1.3 } else { knuckles.iter().copied().sum::<Vec3>() / knuckles.len() as f32 };
            let z = (kc - ph).normalize_or(Vec3::NEG_Z);
            let across = match (roots[1], roots[4]) {
                (Some(i), Some(p)) => if side == 1 { i - p } else { p - i },
                _ => Vec3::NEG_X,
            };
            let x = (across - z * across.dot(z)).normalize_or(Vec3::X);
            let y = z.cross(x);
            let frame0 = Quat::from_mat3(&Mat3::from_cols(x, y, z));
            let qh0 = rot_of(&mh);
            let to_frame = qh0.inverse() * frame0;
            let reach = (kc - ph).length();
            let grip_model = kc + z * (0.25 * reach) - y * (0.45 * reach);
            let grip = frame0.inverse() * (grip_model - ph);
            // Curl axes: each joint bends towards the palm (−Y).
            for (i, chain) in fingers.iter_mut().enumerate() {
                for k in 0..3 {
                    let Some((e, _, _)) = chain[k] else { continue };
                    let m = at(e);
                    let p = Vec3::from(m.translation);
                    let next = chain.get(k + 1).and_then(|c| c.map(|(n, _, _)| Vec3::from(at(n).translation)));
                    let d = match next {
                        Some(n) => (n - p).normalize_or(z),
                        None => if i == 0 { (p - pf).normalize_or(z) } else { z },
                    };
                    let axis = d.cross(-y).normalize_or(x);
                    let rest = q.get(e).map_or(Quat::IDENTITY, |t| t.rotation);
                    chain[k] = Some((e, rest, rot_of(&m).inverse() * axis));
                }
            }
            sides.push(SkinSide {
                upper: u,
                fore: f,
                hand: h,
                fingers,
                upper_parent: at(parent),
                upper_rot: rot_of(&mu),
                fore_rot: rot_of(&mf),
                upper_dir: (pf - pu).normalize_or(Vec3::NEG_Z),
                fore_dir: (ph - pf).normalize_or(Vec3::NEG_Z),
                upper_len: (pf - pu).length(),
                fore_len: (ph - pf).length(),
                to_frame,
                grip,
            });
        }
        let r = sides.pop().unwrap();
        let l = sides.pop().unwrap();
        info!("driver arms: rig found (upper {:.2} m, forearm {:.2} m)", l.upper_len, l.fore_len);
        a.skinned = Some(Box::new(Skinned { sides: [l, r] }));
        for e in a.hand.iter().chain(a.fore.iter()).chain(a.upper.iter()) {
            if let Ok(mut v) = vis.get_mut(*e) {
                *v = Visibility::Hidden;
            }
        }
    }
}

/// Poses one arm of the downloaded model: hand frame at `pose`, elbow by IK, fingers by `grip`.
fn pose_skinned(sk: &SkinSide, side: usize, pose: Pose, grip: f32, tfs: &mut Query<&mut Transform>) {
    let qh = pose.rot * sk.to_frame.inverse();
    let wrist = pose.pos - pose.rot * sk.grip;
    let s = SHOULDER[side];
    let e = elbow_with(s, wrist, POLE[side], sk.upper_len, sk.fore_len);
    let du = Quat::from_rotation_arc(sk.upper_dir, (e - s).normalize_or(sk.upper_dir));
    let qu = du * sk.upper_rot;
    let fore_after = du * sk.fore_dir;
    let df = Quat::from_rotation_arc(fore_after, (wrist - e).normalize_or(fore_after));
    let qf = df * du * sk.fore_rot;
    if let Ok(mut t) = tfs.get_mut(sk.upper) {
        let local = sk.upper_parent.inverse() * Affine3A::from_rotation_translation(qu, s);
        t.translation = local.translation.into();
        t.rotation = rot_of(&local);
    }
    if let Ok(mut t) = tfs.get_mut(sk.fore) {
        t.rotation = (qu.inverse() * qf).normalize();
    }
    if let Ok(mut t) = tfs.get_mut(sk.hand) {
        t.rotation = (qf.inverse() * qh).normalize();
    }
    for (i, chain) in sk.fingers.iter().enumerate() {
        let curl = if i == 0 { THUMB_CURL } else { CURL };
        for (k, j) in chain.iter().enumerate() {
            if let Some((e, rest, axis)) = *j
                && let Ok(mut t) = tfs.get_mut(e)
            {
                t.rotation = rest * Quat::from_axis_angle(axis, curl[k] * grip);
            }
        }
    }
}

/// Finds the lever nodes once the model's scene is in.
pub fn find_levers(models: Query<(&Children, Has<CarRig>), With<CarModel>>, mut arms: Query<(&mut Arms, &ChildOf)>, children: Query<&Children>, names: Query<&Name>) {
    for (mut a, parent) in &mut arms {
        if a.levers.is_some() {
            continue;
        }
        let Ok((_, rigged)) = models.get(parent.parent()) else { continue };
        if !rigged {
            continue;
        }
        let mut found = [None; 2];
        for e in children.iter_descendants(parent.parent()) {
            match names.get(e).map(|n| n.as_str()) {
                Ok("Shifter") => found[0] = Some(e),
                Ok("Handbrake") => found[1] = Some(e),
                _ => {}
            }
        }
        if found.contains(&None) {
            warn!("car model: no Shifter / Handbrake node (rebuild rallycar.glb with tools/art/build_rallycar.py)");
        }
        a.levers = Some(found);
    }
}

/// Places a capsule mesh of length `len` between `a` and `b` (stretched if they're further).
fn segment(a: Vec3, b: Vec3, len: f32) -> Transform {
    let d = b - a;
    let l = d.length().max(1e-4);
    Transform { translation: (a + b) * 0.5, rotation: Quat::from_rotation_arc(Vec3::Y, d / l), scale: Vec3::new(1.0, (l / len).max(1.0), 1.0) }
}

/// Hands, arms and levers, every frame.
pub fn animate(
    time: Res<Time>,
    cars: Query<(Option<&DrivenCar>, Option<&DriverInput>, Option<&CarPose>)>,
    mut arms: Query<(&mut Arms, &mut Visibility)>,
    mut tfs: Query<&mut Transform>,
) {
    let dt = time.delta_secs();
    for (mut a, mut vis) in &mut arms {
        let a = &mut *a;
        let Ok((driven, input, shown)) = cars.get(a.car) else { continue };
        // The simulated car, else the pose its owner fills (the guide's copy of the car).
        let Some(c) = CarPose::read(driven, input, shown) else { continue };
        // The arms belong to the seated driver's body (car.rs `DriverBody`), seen from every
        // camera: the cockpit, the co-driver's seat, the chase camera through the glass.
        let want = Visibility::Inherited;
        if *vis != want {
            *vis = want;
        }
        let moved = a.driver.step(dt, c.gear, c.handbrake);
        // Levers.
        let (pitch, roll) = a.driver.shifter();
        let shift_rot = lever_rot(pitch, roll);
        let hb_rot = lever_rot(a.driver.handbrake(), 0.0);
        if let Some([sh, hb]) = a.levers {
            for (e, r) in [(sh, shift_rot), (hb, hb_rot)] {
                if let Some(e) = e
                    && let Ok(mut t) = tfs.get_mut(e)
                {
                    t.rotation = r;
                }
            }
        }
        if *vis == Visibility::Hidden {
            // Keep the timing going, skip the arms.
            a.now = None;
            continue;
        }
        let wheel = c.steer * STEERING_RATIO;
        let target = match a.driver.job {
            Job::Wheel => wheel_grip(1, wheel),
            Job::Shifter => lever_grip(SHIFT_PIVOT, SHIFT_GRIP, SHIFT_AXIS, shift_rot),
            Job::Handbrake => lever_grip(HB_PIVOT, HB_GRIP, HB_AXIS, hb_rot),
        };
        if moved {
            a.from = a.now.unwrap_or(target);
        }
        let right = match a.now {
            None => target,
            Some(_) if a.driver.arrived() => target,
            Some(_) => travel(a.from, target, a.driver.reach()),
        };
        a.now = Some(right);
        if let Some(sk) = a.skinned.as_deref() {
            // Fingers open a little while the hand travels between grips.
            let open = if a.driver.arrived() { 0.0 } else { (PI * a.driver.reach()).sin() * 0.6 };
            pose_skinned(&sk.sides[0], 0, wheel_grip(0, wheel), 1.0, &mut tfs);
            pose_skinned(&sk.sides[1], 1, right, 1.0 - open, &mut tfs);
            continue;
        }
        for (side, pose) in [(0, wheel_grip(0, wheel)), (1, right)] {
            let (wrist, el) = arm(side, pose);
            if let Ok(mut t) = tfs.get_mut(a.hand[side]) {
                *t = Transform::from_translation(pose.pos).with_rotation(pose.rot);
            }
            if let Ok(mut t) = tfs.get_mut(a.fore[side]) {
                *t = segment(wrist, el, FORE);
            }
            if let Ok(mut t) = tfs.get_mut(a.upper[side]) {
                *t = segment(el, SHOULDER[side], UPPER);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn run(d: &mut Driver, secs: f32, gear: i8, hb: bool) {
        for _ in 0..(secs / DT).round() as usize {
            d.step(DT, gear, hb);
        }
    }

    #[test]
    fn upshift_reaches_throws_and_returns() {
        let mut d = Driver::new(1);
        run(&mut d, 0.5, 1, false);
        assert_eq!(d.job, Job::Wheel);
        assert!(d.step(DT, 2, false), "hand sets off");
        assert_eq!(d.job, Job::Shifter);
        // Lever still until the hand gets there.
        run(&mut d, REACH_LEVER - 0.03, 2, false);
        assert_eq!(d.shifter(), (0.0, 0.0));
        // Pulled back during the throw.
        run(&mut d, 0.12, 2, false);
        assert!(d.shifter().0 > SHIFT_TILT * 0.9, "{:?}", d.shifter());
        // Back to centre, then to the wheel.
        run(&mut d, 0.2, 2, false);
        assert_eq!(d.shifter(), (0.0, 0.0));
        assert_eq!(d.job, Job::Wheel);
        run(&mut d, REACH_WHEEL, 2, false);
        assert!(d.arrived());
    }

    #[test]
    fn downshift_pushes_forward() {
        let mut d = Driver::new(3);
        run(&mut d, REACH_LEVER + 0.1, 2, false);
        assert!(d.shifter().0 < -SHIFT_TILT * 0.9, "{:?}", d.shifter());
    }

    #[test]
    fn reverse_latches_in_its_gate() {
        let mut d = Driver::new(1);
        run(&mut d, 1.0, -1, false);
        let (p, r) = d.shifter();
        assert!((p - REV_PITCH).abs() < 1e-5 && (r - REV_ROLL).abs() < 1e-5, "{p} {r}");
        assert_eq!(d.job, Job::Wheel);
        run(&mut d, 1.0, 1, false);
        assert_eq!(d.shifter(), (0.0, 0.0));
    }

    #[test]
    fn handbrake_tap_still_pulls_and_releases() {
        let mut d = Driver::new(2);
        d.step(DT, 2, true);
        let mut top: f32 = 0.0;
        for _ in 0..90 {
            d.step(DT, 2, false);
            top = top.max(d.handbrake());
        }
        assert!(top > HB_PULL * 0.99, "{top}");
        assert_eq!(d.handbrake(), 0.0);
        assert_eq!(d.job, Job::Wheel);
    }

    #[test]
    fn handbrake_held_keeps_the_hand_there_and_a_shift_waits() {
        let mut d = Driver::new(2);
        run(&mut d, 1.0, 2, true);
        assert_eq!(d.job, Job::Handbrake);
        assert!((d.handbrake() - HB_PULL).abs() < 1e-5);
        // The box changes down meanwhile: the shift happens once the handbrake is off.
        run(&mut d, 0.3, 1, true);
        assert_eq!(d.job, Job::Handbrake);
        run(&mut d, 0.2, 1, false);
        assert_eq!(d.job, Job::Shifter);
        run(&mut d, 1.0, 1, false);
        assert_eq!(d.job, Job::Wheel);
    }

    #[test]
    fn grips_and_elbows_are_reachable() {
        for side in 0..2 {
            for w in [-4.5f32, -1.0, 0.0, 1.0, 4.5] {
                let p = wheel_grip(side, w);
                assert!(((p.pos - WHEEL_C).length() - RIM).abs() < 1e-4);
                let wrist = p.pos + p.rot * Vec3::new(0.0, 0.0, -WRIST);
                let d = (wrist - SHOULDER[side]).length();
                assert!(d < UPPER + FORE, "side {side} wheel {w}: {d}");
            }
        }
        for (pivot, grip, axis, rot) in [(SHIFT_PIVOT, SHIFT_GRIP, SHIFT_AXIS, lever_rot(SHIFT_TILT, 0.0)), (HB_PIVOT, HB_GRIP, HB_AXIS, lever_rot(HB_PULL, 0.0))] {
            let p = lever_grip(pivot, grip, axis, rot);
            let wrist = p.pos + p.rot * Vec3::new(0.0, 0.0, -WRIST);
            let d = (wrist - SHOULDER[1]).length();
            assert!(d < UPPER + FORE, "{d}");
            let e = elbow(SHOULDER[1], wrist, POLE[1]);
            assert!(((e - SHOULDER[1]).length() - UPPER).abs() < 1e-3);
            assert!(((e - wrist).length() - FORE).abs() < 1e-3);
            assert!(e.y < SHOULDER[1].y, "elbow down: {e}");
        }
    }

    /// The cockpit camera: eye (car.rs `COCKPIT_EYE_MODEL`), pitched 0.05 rad down, 80°
    /// vertical field of view, 16:9. Returns normalised screen coordinates (−1..1).
    fn screen(p: Vec3) -> Option<Vec2> {
        const EYE: Vec3 = Vec3::new(-0.37, 0.795, 0.60);
        let d = Quat::from_rotation_x(0.05) * (p - EYE);
        let depth = -d.z;
        if depth < 0.05 {
            return None;
        }
        let t = 40f32.to_radians().tan();
        Some(Vec2::new(d.x / (depth * t * 16.0 / 9.0), d.y / (depth * t)))
    }

    /// Screen points along an arm's sleeves (top edge), with their screen radius.
    fn sleeve_points(side: usize, pose: Pose, out: &mut Vec<(Vec2, f32)>) {
        let (w, e) = arm(side, pose);
        for (a, b, r) in [(w, e, 0.038), (e, SHOULDER[side], 0.048)] {
            for i in 0..=12 {
                let p = a.lerp(b, i as f32 / 12.0);
                if let (Some(c), Some(top)) = (screen(p), screen(p + Vec3::Y * r)) {
                    out.push((top, (top - c).length()));
                }
            }
        }
    }

    /// Every pose the right hand passes through: on the wheel, on the way to and from a lever,
    /// on the lever at every throw.
    fn right_poses() -> Vec<Pose> {
        let mut v = Vec::new();
        let levers = [
            lever_grip(SHIFT_PIVOT, SHIFT_GRIP, SHIFT_AXIS, lever_rot(0.0, 0.0)),
            lever_grip(SHIFT_PIVOT, SHIFT_GRIP, SHIFT_AXIS, lever_rot(SHIFT_TILT, 0.0)),
            lever_grip(SHIFT_PIVOT, SHIFT_GRIP, SHIFT_AXIS, lever_rot(-SHIFT_TILT, 0.0)),
            lever_grip(SHIFT_PIVOT, SHIFT_GRIP, SHIFT_AXIS, lever_rot(REV_PITCH, REV_ROLL)),
            lever_grip(HB_PIVOT, HB_GRIP, HB_AXIS, lever_rot(0.0, 0.0)),
            lever_grip(HB_PIVOT, HB_GRIP, HB_AXIS, lever_rot(HB_PULL, 0.0)),
        ];
        for w in [-1.0f32, 0.0, 1.0] {
            let on = wheel_grip(1, w);
            for l in levers {
                for k in 0..=20 {
                    v.push(travel(on, l, smooth(k as f32 / 20.0)));
                }
            }
        }
        v
    }

    #[test]
    fn arms_stay_below_the_dash_and_off_the_rev_gauge() {
        // The dash top's near edge straight ahead of the driver, and the rev gauge with its
        // radius (cockpit.py `dash`, `TACHO_C`, `DIAL_R`).
        let dash = screen(Vec3::new(-0.37, 0.65, -0.24)).unwrap().y;
        let tacho = screen(Vec3::new(-0.445, 0.51, -0.19)).unwrap();
        let tacho_r = (screen(Vec3::new(-0.445 + 0.062, 0.51, -0.19)).unwrap() - tacho).length();
        let mut worst = (f32::MIN, String::new());
        let mut pts = Vec::new();
        let mut check = |what: String, pts: &[(Vec2, f32)], gauge: bool| {
            for &(p, r) in pts {
                if p.x.abs() > 1.0 || p.y < -1.0 {
                    continue;
                }
                let over = p.y - (dash - 0.03);
                if over > worst.0 {
                    worst = (over, what.clone());
                }
                if gauge {
                    assert!((p - tacho).length() > tacho_r + r * 0.5, "{what}: sleeve over the rev gauge at {p}");
                }
            }
        };
        for i in 0..=36 {
            let w = -4.5 + 0.25 * i as f32;
            for side in 0..2 {
                pts.clear();
                sleeve_points(side, wheel_grip(side, w), &mut pts);
                check(format!("side {side} wheel {w}"), &pts, false);
            }
        }
        for (n, pose) in right_poses().into_iter().enumerate() {
            pts.clear();
            sleeve_points(1, pose, &mut pts);
            check(format!("right hand pose {n}"), &pts, true);
        }
        eprintln!("worst {:?}", worst); assert!(worst.0 < -0.06, "a sleeve rises over the dash top: {} by {:.3}", worst.1, worst.0);
    }

    /// Angle between the hand's long axis and the forearm (elbow → wrist).
    fn wrist_bend(side: usize, pose: Pose) -> f32 {
        let (w, e) = arm(side, pose);
        let hand = pose.rot * Vec3::Z;
        hand.angle_between((w - e).normalize())
    }

    #[test]
    fn wrists_stay_straight_on_the_wheel_and_levers() {
        for i in 0..=36 {
            let w = -4.5 + 0.25 * i as f32;
            for side in 0..2 {
                let b = wrist_bend(side, wheel_grip(side, w));
                assert!(b <= MAX_WRIST + 0.02, "side {side} wheel {w}: wrist bent {:.0}°", b.to_degrees());
            }
        }
        for (name, pose) in [
            ("shifter", lever_grip(SHIFT_PIVOT, SHIFT_GRIP, SHIFT_AXIS, lever_rot(0.0, 0.0))),
            ("shifter up", lever_grip(SHIFT_PIVOT, SHIFT_GRIP, SHIFT_AXIS, lever_rot(SHIFT_TILT, 0.0))),
            ("shifter reverse", lever_grip(SHIFT_PIVOT, SHIFT_GRIP, SHIFT_AXIS, lever_rot(REV_PITCH, REV_ROLL))),
            ("handbrake", lever_grip(HB_PIVOT, HB_GRIP, HB_AXIS, lever_rot(0.0, 0.0))),
            ("handbrake pulled", lever_grip(HB_PIVOT, HB_GRIP, HB_AXIS, lever_rot(HB_PULL, 0.0))),
        ] {
            let b = wrist_bend(1, pose);
            eprintln!("{name}: wrist {:.0}°", b.to_degrees());
            assert!(b <= 40f32.to_radians(), "{name}: wrist bent {:.0}°", b.to_degrees());
        }
        // At rest both hands hold quarter to three: no slip.
        for side in 0..2 {
            let p = wheel_grip(side, 0.0);
            let want = rim_pose(side, if side == 0 { PI - GRIP_UP } else { GRIP_UP }).0;
            assert!((p.pos - want.pos).length() < 1e-4, "side {side} slipped at rest");
        }
    }
}
