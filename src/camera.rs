//! Camera feel: the driver's cameras (cockpit, bonnet, the chase camera on a spring arm), the
//! shared speed sensation, shake and landing kicks, speed motion blur, and the Camera tab.
//!
//! **Owner: stream D (CAR FEEL).** Design: `docs/festival/ART_BIBLE.md` §3.2-§3.4 (spring arm,
//! motion blur, shake), `docs/festival/AUDIO_PASS.md` §5 (`SpeedFeel` drives the wind too),
//! `docs/festival/TECH_PASS.md` §1/§4 (never insert or remove `MotionBlur` in play),
//! `docs/festival/DECISIONS.md` D3-D6.
//!
//! - [`SpeedFeel`] (0..1, smoothstep 40 → 180 km/h) is the one value the FOV kick, the motion
//!   blur and the wind (`audio::car`) read, so sight and sound ramp together.
//! - The chase camera hangs on a spring arm ([`Arm`]): one damped spring per axis in the arm's
//!   yaw frame (closed form, so frame-rate independent), clamped, then the collision probe
//!   pulls it in.
//! - Motion blur: `Look.lens.motion_blur` is switched on once (if the setting allows) and the
//!   engine inserts `MotionBlur` where the tier allows it (High+); every frame only its
//!   `shutter_angle` is written, after `QualitySet::Look`.
//!
//! Look-only: nothing here feeds the sim.

use bevy::post_process::motion_blur::MotionBlur;
use bevy::prelude::*;
use engine::gfx::{MainCamera, QualitySet};
use engine::settings::{FieldDef, FieldKind, RegisterSettings, add_settings_section};
use engine::ui::prelude::*;
use serde::{Deserialize, Serialize};

use crate::drive::{CameraView, CarBody, CarSpec, DrivenCar};
use crate::vehicle;
use crate::world::StageWorld;

/// How fast it feels, 0..1: smoothstep over 40 → 180 km/h. FOV, motion blur and the wind all
/// read this one value. Look- and sound-only (never feeds the sim).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct SpeedFeel(pub f32);

/// The Camera tab (section `"camera_feel"` of settings.json). D registers its rows (tab
/// "Camera") in its own plugin with `register_setting`; nothing in `prefs.rs` lists them.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraFeel {
    /// Motion blur: 0 Off, 1 Low (0.6×), 2 High. Default Low (DECISIONS D3). Only where the
    /// quality tier allows it; reduce motion forces Off.
    pub motion_blur: u32,
    /// Chase distance: 0 Near, 1 Normal, 2 Far.
    pub chase_distance: u32,
    /// Chase stiffness, 0.5 (loose) ..= 1.5 (tight): scales the spring arm's ω.
    pub chase_stiffness: f32,
    /// A dip of the chase camera on hard landings.
    pub landing_kick: bool,
}

impl Default for CameraFeel {
    fn default() -> Self {
        CameraFeel { motion_blur: 1, chase_distance: 1, chase_stiffness: 1.0, landing_kick: true }
    }
}

/// Frames left with no motion blur (a camera cut or a teleport: no full-screen smear).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct CameraCut(pub u8);

pub struct CameraFeelPlugin;

impl Plugin for CameraFeelPlugin {
    fn build(&self, app: &mut App) {
        add_settings_section::<CameraFeel>(app, "camera_feel");
        app.settings_defaults::<CameraFeel>().init_resource::<SpeedFeel>().init_resource::<CameraCut>();
        register(app);
        app.add_systems(Update, (blur_switch.in_set(QualitySet::Look).before(engine::look::apply::apply_look), blur_amount.after(QualitySet::Look)));
    }
}

fn cf(w: &World) -> &CameraFeel {
    w.resource::<CameraFeel>()
}
fn cfm(w: &mut World) -> Mut<'_, CameraFeel> {
    w.resource_mut::<CameraFeel>()
}

fn register(app: &mut App) {
    app.register_setting(FieldDef {
        key: "motion_blur",
        tab: "Camera",
        label: "Motion blur",
        hint: "The world streaks past at speed. On High and Ultra graphics only; Reduce motion turns it off.",
        kind: FieldKind::Choice { options: vec!["Off", "Low", "High"] },
        get: |w| cf(w).motion_blur as f32,
        set: |w, v| cfm(w).motion_blur = (v.round().max(0.0) as u32).min(2),
    })
    .register_setting(FieldDef {
        key: "chase_distance",
        tab: "Camera",
        label: "Chase distance",
        hint: "How far behind the car the chase camera hangs.",
        kind: FieldKind::Choice { options: vec!["Near", "Normal", "Far"] },
        get: |w| cf(w).chase_distance as f32,
        set: |w, v| cfm(w).chase_distance = (v.round().max(0.0) as u32).min(2),
    })
    .register_setting(FieldDef {
        key: "chase_stiffness",
        tab: "Camera",
        label: "Chase stiffness",
        hint: "Loose lets the chase camera swing out in slides and lag under braking; tight keeps it locked behind the car.",
        kind: FieldKind::Slider { min: 0.5, max: 1.5, step: 0.1, format: engine::settings::format_multiplier },
        get: |w| cf(w).chase_stiffness,
        set: |w, v| cfm(w).chase_stiffness = v.clamp(0.5, 1.5),
    })
    .register_setting(FieldDef {
        key: "landing_kick",
        tab: "Camera",
        label: "Landing kick",
        hint: "The chase camera dips when the car lands hard after a jump.",
        kind: FieldKind::Toggle,
        get: |w| if cf(w).landing_kick { 1.0 } else { 0.0 },
        set: |w, v| cfm(w).landing_kick = v > 0.5,
    });
}

// ---------------------------------------------------------------- speed feel

/// [`SpeedFeel`] at `speed` m/s: 0 to 40 km/h, 1 from 180 km/h, smooth between.
pub fn speed_feel(speed: f32) -> f32 {
    smoothstep(40.0, 180.0, speed.abs() * 3.6)
}

/// Extra vertical field of view (degrees) for a speed feel: up to +7 (ART_BIBLE §3.4; the
/// cockpit caps it at +5, its frame makes it look larger).
pub fn fov_kick(feel: f32, inside: bool) -> f32 {
    (if inside { 5.0 } else { 7.0 }) * feel.clamp(0.0, 1.0)
}

/// Extra vertical field of view (degrees) at `speed` m/s: none to 40 km/h, +7 by 180 km/h.
pub fn speed_fov(speed: f32) -> f32 {
    fov_kick(speed_feel(speed), false)
}

/// Motion blur shutter angle for a speed feel at full strength (High, chase): none to ~60 km/h,
/// ≈ 0.35 by 160 km/h, 0.4 flat out (ART_BIBLE §3.3).
pub fn shutter_for(feel: f32) -> f32 {
    let x = ((feel - 0.05) / 0.95).clamp(0.0, 1.0);
    0.4 * x.powf(1.2)
}

/// The car's speed → [`SpeedFeel`] (the player's car, else the guide's replicated one).
pub fn update_speed_feel(cars: Query<&DrivenCar>, remote: Query<&crate::guide::sync::NetCar, With<engine::net::Remote>>, mut feel: ResMut<SpeedFeel>) {
    let v = cars.iter().next().map(|c| c.car.vel.length()).or_else(|| remote.iter().next().map(|n| n.vel.length())).unwrap_or(0.0);
    let f = speed_feel(v);
    if feel.0 != f {
        feel.0 = f;
    }
}

// ---------------------------------------------------------------- motion blur

/// Motion blur on or off in the `Look` (the engine adds `MotionBlur` only where the tier allows
/// it). Changes only with the setting, never during a drive; re-asserted when someone rewrites
/// the `Look` (the time of day does, about once a second), before it is applied.
fn blur_switch(cfg: Option<Res<CameraFeel>>, access: Option<Res<engine::actions::Accessibility>>, look: Option<ResMut<engine::look::desc::Look>>) {
    let (Some(cfg), Some(mut look)) = (cfg, look) else { return };
    let want = cfg.motion_blur > 0 && !access.as_deref().is_some_and(|a| a.reduce_motion);
    let mb = &look.bypass_change_detection().lens.motion_blur;
    if mb.enabled != want || (want && mb.samples != 4) {
        let mb = &mut look.lens.motion_blur;
        mb.enabled = want;
        mb.samples = 4;
        mb.shutter_angle = 0.0;
    }
}

/// The blur follows the speed: `shutter_angle` written every frame after the look applied.
fn blur_amount(
    feel: Res<SpeedFeel>,
    cfg: Option<Res<CameraFeel>>,
    view: Option<Res<CameraView>>,
    access: Option<Res<engine::actions::Accessibility>>,
    mut cut: ResMut<CameraCut>,
    mut blur: Query<&mut MotionBlur, With<MainCamera>>,
) {
    let level = match cfg.as_deref().map_or(1, |c| c.motion_blur) {
        0 => 0.0,
        1 => 0.6,
        _ => 1.0,
    };
    let reduce = access.as_deref().is_some_and(|a| a.reduce_motion);
    let inside = view.as_deref().is_some_and(|v| *v != CameraView::Chase);
    let s = if reduce || cut.0 > 0 { 0.0 } else { shutter_for(feel.0) * level * if inside { 0.6 } else { 1.0 } };
    cut.0 = cut.0.saturating_sub(1);
    let s = (s * 100.0).round() / 100.0;
    for mut mb in &mut blur {
        if mb.shutter_angle != s {
            mb.shutter_angle = s;
        }
    }
}

// ---------------------------------------------------------------- springs

/// A damped spring on one axis, stepped in closed form (exact for a target held over the step,
/// so the feel doesn't change with the frame rate).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spring {
    /// Offset from the target.
    pub x: f32,
    pub v: f32,
}

impl Spring {
    /// Steps `dt` seconds towards 0 with natural frequency `omega` (rad/s) and damping ratio
    /// `zeta` (≤ 1).
    pub fn step(&mut self, omega: f32, zeta: f32, dt: f32) {
        let (w, z, t) = (omega.max(0.01), zeta.clamp(0.05, 1.0), dt.max(0.0));
        let (y0, v0) = (self.x, self.v);
        if z > 0.999 {
            let e = (-w * t).exp();
            let b = v0 + w * y0;
            self.x = (y0 + b * t) * e;
            self.v = (v0 - w * b * t) * e;
        } else {
            let a = z * w;
            let wd = w * (1.0 - z * z).sqrt();
            let e = (-a * t).exp();
            let (s, c) = (wd * t).sin_cos();
            let b = (v0 + a * y0) / wd;
            self.x = e * (y0 * c + b * s);
            self.v = e * (v0 * c - (a * v0 + w * w * y0) / wd * s);
        }
    }

    /// Keeps the offset within `lo..=hi` (stops the motion into the limit).
    pub fn clamp(&mut self, lo: f32, hi: f32) {
        if self.x < lo {
            self.x = lo;
            self.v = self.v.max(0.0);
        } else if self.x > hi {
            self.x = hi;
            self.v = self.v.min(0.0);
        }
    }
}

/// One axis of the arm: ω (rad/s), ζ, limits (m or rad) (ART_BIBLE §3.2).
#[derive(Clone, Copy, Debug)]
pub struct Axis {
    pub omega: f32,
    pub zeta: f32,
    pub lo: f32,
    pub hi: f32,
}

/// Boom length (positive: stretched away from the car).
pub const LONG: Axis = Axis { omega: 5.0, zeta: 0.8, lo: -1.0, hi: 1.6 };
pub const LAT: Axis = Axis { omega: 4.0, zeta: 0.9, lo: -1.2, hi: 1.2 };
pub const VERT: Axis = Axis { omega: 7.0, zeta: 1.0, lo: -0.4, hi: 0.6 };
/// Heading of the arm (rad); at most 25° off the car's nose.
pub const HEADING: Axis = Axis { omega: 3.5, zeta: 1.0, lo: -0.436, hi: 0.436 };
/// The point looked at.
pub const AIM_OMEGA: f32 = 9.0;

/// The chase camera's spring arm, in the arm's yaw frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Arm {
    /// Eye offset from its rest pose: along the boom (+ = further back), right, up.
    pub long: Spring,
    pub lat: Spring,
    pub vert: Spring,
    /// Arm heading offset from where it wants to point (rad).
    pub yaw: Spring,
    /// The arm's heading (rad, atan2(x, z) of its forward).
    pub heading: f32,
    /// Aim point offset from its target (world x, y, z).
    pub aim: [Spring; 3],
    /// Last rest eye and aim (world) and how fast they moved, to see their acceleration.
    rest_eye: Vec3,
    rest_aim: Vec3,
    vel: Option<(Vec3, Vec3)>,
}

fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Forward (flat) of a heading.
pub fn heading_dir(h: f32) -> Vec3 {
    Vec3::new(h.sin(), 0.0, h.cos())
}

/// The rest pose of the arm at `speed` m/s: boom length and height (ART_BIBLE §3.2: 6.8 m back
/// and 2.3 m up, +0.8 m and −0.25 m from 60 → 180 km/h), scaled by the Chase distance.
pub fn rest_boom(speed: f32, distance: u32) -> (f32, f32) {
    let s = smoothstep(60.0, 180.0, speed.abs() * 3.6);
    let k = [0.8, 1.0, 1.25][distance.min(2) as usize];
    ((6.8 + 0.8 * s) * k, (2.3 - 0.25 * s) * k.sqrt())
}

impl Arm {
    /// Starts at rest.
    pub fn new(car: Vec3, want_heading: f32, boom: (f32, f32)) -> Self {
        let f = heading_dir(want_heading);
        let rest_eye = car - f * boom.0 + Vec3::Y * boom.1;
        let rest_aim = car + f * 4.0 + Vec3::Y * 0.9;
        Arm { heading: want_heading, rest_eye, rest_aim, ..default() }
    }

    /// Kicks the vertical spring down by about `m` metres at its deepest (a landing).
    pub fn kick(&mut self, m: f32, stiffness: f32) {
        self.vert.v -= m * VERT.omega * stiffness * std::f32::consts::E;
    }

    /// One step: the car at `car` with its nose at `car_heading` wants the arm at `want_heading`
    /// with `boom` (length, height). Returns (eye, aim) in the world.
    pub fn step(&mut self, car: Vec3, car_heading: f32, want_heading: f32, boom: (f32, f32), stiffness: f32, dt: f32) -> (Vec3, Vec3) {
        let k = stiffness.clamp(0.5, 1.5);
        // Heading: a spring on the error to where it wants to point, ≤ 25° off the nose.
        self.yaw.x = wrap(self.heading - want_heading);
        self.yaw.step(HEADING.omega * k, HEADING.zeta, dt);
        let off = wrap(want_heading + self.yaw.x - car_heading).clamp(HEADING.lo, HEADING.hi);
        self.heading = wrap(car_heading + off);
        let f = heading_dir(self.heading);
        let right = Vec3::new(-f.z, 0.0, f.x);
        let rest_eye = car - f * boom.0 + Vec3::Y * boom.1;
        let rest_aim = car + f * 4.0 + Vec3::Y * 0.9;
        // The camera is a mass on springs to its rest pose: only the rest pose's acceleration
        // moves it off (y'' = −ω²y − 2ζωy' − a). Speeding up stretches the boom, braking
        // squashes it, a steady speed leaves it at rest.
        if dt > 1e-5 {
            let v = ((rest_eye - self.rest_eye) / dt, (rest_aim - self.rest_aim) / dt);
            if let Some((ve, va)) = self.vel {
                // Teleports and cuts reset the arm; clamp what's left of a hitch.
                let de = (v.0 - ve).clamp_length_max(20.0);
                let da = (v.1 - va).clamp_length_max(20.0);
                self.long.v += de.dot(f);
                self.lat.v -= de.dot(right);
                self.vert.v -= de.y;
                for (s, c) in self.aim.iter_mut().zip([da.x, da.y, da.z]) {
                    s.v -= c;
                }
            }
            self.vel = Some(v);
        }
        self.rest_eye = rest_eye;
        self.rest_aim = rest_aim;
        for (s, a) in [(&mut self.long, LONG), (&mut self.lat, LAT), (&mut self.vert, VERT)] {
            s.step(a.omega * k, a.zeta, dt);
            s.clamp(a.lo, a.hi);
        }
        for s in &mut self.aim {
            s.step(AIM_OMEGA * k, 1.0, dt);
            s.clamp(-3.0, 3.0);
        }
        let eye = rest_eye - f * self.long.x + right * self.lat.x + Vec3::Y * self.vert.x;
        let aim = rest_aim + Vec3::new(self.aim[0].x, self.aim[1].x, self.aim[2].x);
        (eye, aim)
    }
}

// ---------------------------------------------------------------- the cameras

/// Camera state kept between frames.
#[derive(Default)]
pub struct CamFx {
    /// The speed FOV kick (degrees, eased).
    fov: f32,
    /// Chase camera lift (m) and pull-in (share of the boom out), each eased back out.
    lift: f32,
    pull: f32,
    /// The view and the car's position last frame (to notice a switch or a teleport).
    last_view: Option<CameraView>,
    last_pos: Option<Vec3>,
    head: Option<Quat>,
    arm: Option<Arm>,
    /// In the air (no wheel touching) and the lowest vertical speed since leaving the ground.
    airborne: bool,
    fall: f32,
    /// Impacts so far (`Car::impacts`) and the rotational jolt: seconds left, degrees.
    impacts: f32,
    jolt: (f32, f32),
}

/// The drawn car and its sim state, for the cameras.
type CarPose = (&'static Transform, Option<&'static DrivenCar>);

#[allow(clippy::too_many_arguments)]
pub fn follow_camera(
    time: Res<Time>,
    view: Res<CameraView>,
    spec: Res<CarSpec>,
    stage: Res<StageWorld>,
    (feel, cfg): (Res<SpeedFeel>, Option<Res<CameraFeel>>),
    cars: Query<CarPose, (With<CarBody>, Without<MainCamera>)>,
    mut cams: Query<(&mut Transform, Option<&mut Projection>), With<MainCamera>>,
    access: Option<Res<engine::actions::Accessibility>>,
    core: Option<Res<engine::settings::CoreSettings>>,
    traffic: Option<Res<crate::open::traffic::TrafficSolids>>,
    mut cut: ResMut<CameraCut>,
    mut fx: Local<CamFx>,
) {
    // Settings > Camera shake (0..1; Reduce motion = 0): how much of the body's pitch and roll
    // reaches the cockpit and bonnet views, how much the speed widens the view, the road buzz,
    // the chase shake, landing kicks and impact jolts.
    let shake = access.as_deref().map_or(1.0, |a| if a.reduce_motion { 0.0 } else { a.camera_shake.clamp(0.0, 1.0) });
    let cfg = cfg.map(|c| c.clone()).unwrap_or_default();
    let (Ok((car, driven)), Ok((mut cam, proj))) = (cars.single(), cams.single_mut()) else { return };
    let dt = time.delta_secs();
    // A new view, or the car put somewhere else (a recover, back home after a wreck): start
    // the chase camera fresh instead of easing out of where it was, and no motion blur for a
    // few frames.
    let jumped = fx.last_pos.is_some_and(|p: Vec3| p.distance(car.translation) > 8.0);
    let fresh = fx.last_pos.is_none() || jumped || fx.last_view != Some(*view);
    if fresh {
        fx.last_view = Some(*view);
        fx.lift = 0.0;
        fx.pull = 0.0;
        fx.arm = None;
        fx.head = None;
        cut.0 = 3;
    }
    fx.last_pos = Some(car.translation);
    let fwd = car.rotation * Vec3::NEG_Z;
    let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::NEG_Z);
    let vel = driven.map_or(Vec3::ZERO, |d| d.car.vel);
    let speed = vel.length();
    let inside = *view != CameraView::Chase;
    // Speed widens the view (SpeedFeel, the same value the blur and the wind use), eased so a
    // hit doesn't snap it.
    let kick = fov_kick(feel.0, inside) * shake;
    fx.fov += (kick - fx.fov) * (1.0 - (-dt * 2.0).exp());
    if let (Some(mut proj), Some(core)) = (proj, core.as_deref())
        && let Projection::Perspective(pp) = proj.as_mut()
    {
        let fov = (core.fov_deg.clamp(30.0, 150.0) + fx.fov).to_radians();
        if (pp.fov - fov).abs() > 1e-4 {
            pp.fov = fov;
        }
    }
    let t = time.elapsed_secs();
    let rough = {
        use vehicle::Ground;
        surface_buzz(stage.surface(car.translation.x, car.translation.z))
    };
    let on_ground = driven.is_none_or(|d| d.car.wheels.iter().any(|w| w.contact));
    // Road buzz in the cockpit and bonnet views: a few millimetres on gravel at speed, more on
    // rough ground, next to nothing on tarmac (camera only: the sim doesn't see it).
    let buzz = {
        let amp = if on_ground { 0.004 * rough * (speed / 25.0).min(1.5) * shake } else { 0.0 };
        let n = |a: f32, b: f32, c: f32| (t * a).sin() * 0.5 + (t * b + 1.3).sin() * 0.3 + (t * c + 2.1).sin() * 0.2;
        (Vec3::new(n(17.0, 29.3, 41.1) * 0.4, n(13.1, 23.7, 37.9), 0.0) * amp, n(11.3, 19.1, 33.7) * amp * 0.35)
    };
    // Landings and impacts: the lowest vertical speed in the air, the hits since last frame.
    let mut landing = 0.0;
    if let Some(d) = driven {
        let c = &d.car;
        let airborne = !c.wheels.iter().any(|w| w.contact);
        if airborne {
            fx.fall = if fx.airborne { fx.fall.min(c.vel.y) } else { c.vel.y };
        } else if fx.airborne {
            landing = (-fx.fall - 2.0).max(0.0);
            fx.fall = 0.0;
        }
        fx.airborne = airborne;
        let hit = c.impacts - fx.impacts;
        if hit > 0.4 && !fresh {
            fx.jolt = (0.15, (hit * 0.1).min(1.5) * shake);
        }
        fx.impacts = c.impacts;
    }
    match *view {
        CameraView::Cockpit => {
            // Fixed to the seat; the view direction follows the body through a short lag so
            // the suspension's high-frequency shake doesn't reach the eye. Pitched a little
            // down, the way a driver looks at the road.
            let k = 1.0 - (-dt * (3.0 + 15.0 * shake)).exp();
            let look = match fx.head {
                Some(q) if q.dot(car.rotation).abs() > 0.97 => q.slerp(car.rotation, k),
                _ => car.rotation,
            };
            fx.head = Some(look);
            cam.translation = car.translation + car.rotation * (crate::art::car::cockpit_eye(&spec.0) + buzz.0);
            cam.rotation = look * Quat::from_rotation_x(-0.05 + buzz.1);
            return;
        }
        CameraView::Bonnet => {
            cam.translation = car.translation + car.rotation * (crate::art::car::bonnet_cam(&spec.0) + buzz.0 * 0.7);
            // Fixed to the bonnet at full shake; smoothed towards the body below that.
            let body = if shake >= 0.99 {
                car.rotation
            } else {
                let k = 1.0 - (-dt * (3.0 + 27.0 * shake)).exp();
                match fx.head {
                    Some(q) if q.dot(car.rotation).abs() > 0.97 => q.slerp(car.rotation, k),
                    _ => car.rotation,
                }
            };
            fx.head = Some(body);
            cam.rotation = body * Quat::from_rotation_x(buzz.1 * 0.7);
            return;
        }
        CameraView::Chase => {}
    }
    // The arm points partly along the direction of travel, so a slide shows as the car at an
    // angle.
    let dir = chase_dir(flat, vel);
    let want = dir.x.atan2(dir.z);
    let nose = flat.x.atan2(flat.z);
    let boom = rest_boom(speed, cfg.chase_distance);
    let k = cfg.chase_stiffness.clamp(0.5, 1.5);
    let arm = fx.arm.get_or_insert_with(|| Arm::new(car.translation, want, boom));
    if landing > 0.0 && cfg.landing_kick {
        arm.kick((0.06 * landing).min(0.35) * shake, k);
    }
    let (mut eye, at) = arm.step(car.translation, nose, want, boom, k, dt);
    // Chase shake at speed: low-frequency noise (6-9 Hz) scaled by the road's roughness.
    let amp = if on_ground { 0.012 * rough * shake * smoothstep(110.0, 160.0, speed * 3.6) } else { 0.0 };
    if amp > 0.0 {
        let n = |a: f32, b: f32, c: f32| (t * a).sin() * 0.5 + (t * b + 0.7).sin() * 0.3 + (t * c + 1.9).sin() * 0.2;
        let f = heading_dir(arm.heading);
        let right = Vec3::new(-f.z, 0.0, f.x);
        eye += right * n(6.3, 7.7, 8.9) * amp * 0.6 + Vec3::Y * n(7.1, 8.3, 6.1) * amp;
    }
    // Never under the ground behind the car (a crest, a bank).
    {
        use vehicle::Ground;
        eye.y = eye.y.max(stage.height(eye.x, eye.z) + 0.8);
    }
    // Clear of whatever is between the car and the camera (a car behind, a building, a bank,
    // a big rock; a thin trunk only when the eye would be inside it): in at once, back out
    // slowly.
    let pivot = car.translation + Vec3::Y * 1.1;
    let mut solids = Vec::new();
    if let Some(tr) = traffic.as_deref() {
        let m = (pivot + eye) * 0.5;
        tr.obstacles(Vec2::new(m.x, m.z), 8.0, &mut solids);
    }
    // First up over it (a car close behind, a low wall), then in if that isn't enough.
    let lift = [0.0, 1.2, 2.4].into_iter().find(|&y| chase_clearance(pivot, eye + Vec3::Y * y, &*stage, &solids) >= 1.0).unwrap_or(2.4);
    let rate = if lift > fx.lift { 8.0 } else { 2.0 };
    if fx.pull <= 0.0 {
        // Fresh (just switched to, or the car was moved): straight to where it should be.
        fx.lift = lift;
    } else {
        fx.lift += (lift - fx.lift) * (1.0 - (-dt * rate).exp());
    }
    let eye = eye + Vec3::Y * fx.lift;
    let clear = chase_clearance(pivot, eye, &*stage, &solids);
    fx.pull = if clear < fx.pull || fx.pull <= 0.0 { clear } else { fx.pull + (clear - fx.pull) * (1.0 - (-dt * 2.5).exp()) };
    let eye = pivot + (eye - pivot) * fx.pull;
    let mut tf = Transform::from_translation(eye).looking_at(at, Vec3::Y);
    // An impact: a short rotational jolt (≤ 1.5°).
    if fx.jolt.0 > 0.0 {
        let a = (fx.jolt.0 / 0.15) * fx.jolt.1.to_radians();
        let w = (fx.jolt.0 * 60.0).sin();
        tf.rotation *= Quat::from_euler(EulerRot::XYZ, a * w * 0.6, 0.0, a * w);
        fx.jolt.0 -= dt;
    }
    *cam = tf;
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much road buzz a surface sends through the seat (gravel = 1).
pub fn surface_buzz(s: crate::stage::Surface) -> f32 {
    use crate::stage::Surface::*;
    match s {
        Tarmac | Wet => 0.15,
        Dusty => 0.3,
        Snow => 0.6,
        Gravel | Mud => 1.0,
        Loose => 1.2,
        Verge => 1.6,
        Rough | Ditch => 2.0,
    }
}

/// Radius (m) of the chase camera's collision probe (ART_BIBLE §3.2: a 0.35 m sphere).
const CAM_RADIUS: f32 = 0.35;
/// The chase camera never pulls in closer than this share of the boom (it sits over the roof).
const CAM_MIN_PULL: f32 = 0.22;

/// How far from `pivot` towards `eye` (share, [`CAM_MIN_PULL`]..1) the chase camera can sit
/// before something solid is in the way: the ground, walls, big obstacles (radius ≥ 0.6 m:
/// boulders, traffic, `extra`) along the whole line; a thin trunk only near the eye (one
/// passing between the camera and the car isn't worth a jump).
pub fn chase_clearance(pivot: Vec3, eye: Vec3, ground: &impl vehicle::Ground, extra: &[vehicle::Obstacle]) -> f32 {
    use vehicle::Give;
    let (a, b) = (Vec2::new(pivot.x, pivot.z), Vec2::new(eye.x, eye.z));
    let mid = (a + b) * 0.5;
    let r = a.distance(b) * 0.5 + 2.0;
    let mut obs = Vec::new();
    ground.obstacles(mid, r, &mut obs);
    obs.extend_from_slice(extra);
    obs.retain(|o| matches!(o.give, Give::Solid));
    let mut walls = Vec::new();
    ground.walls(mid, r, &mut walls);
    const N: usize = 32;
    for i in 1..=N {
        let t = i as f32 / N as f32;
        let p = pivot.lerp(eye, t);
        let q = Vec2::new(p.x, p.z);
        let blocked = p.y < ground.height(p.x, p.z) + CAM_RADIUS
            || walls.iter().any(|w| p.y < w.top + CAM_RADIUS && w.distance(q) < CAM_RADIUS)
            || obs.iter().any(|o| (o.radius >= 0.6 || t > 0.8) && p.y < o.top + CAM_RADIUS && q.distance(o.pos) < o.radius + CAM_RADIUS);
        if blocked {
            return ((i - 1) as f32 / N as f32).max(CAM_MIN_PULL);
        }
    }
    1.0
}

/// The chase camera's heading: the car's nose (`flat`), turned up to 60 % towards the direction
/// of travel from about 10 km/h up to 47 km/h, so sideslip shows on screen. On the nose at walking
/// pace, and whenever the car moves backwards (reversing, a spin), so the camera never swings
/// round behind it.
pub fn chase_dir(flat: Vec3, vel: Vec3) -> Vec3 {
    let v = Vec3::new(vel.x, 0.0, vel.z);
    let speed = v.length();
    if speed < 1e-3 || v.dot(flat) <= 0.0 {
        return flat;
    }
    let w = ((speed - 3.0) / 10.0).clamp(0.0, 1.0) * 0.6;
    if w <= 0.0 {
        return flat;
    }
    flat.lerp(v / speed, w).normalize_or(flat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chase_camera_pulls_in_front_of_a_car_behind() {
        use crate::stage::Surface;
        use crate::vehicle::{Flat, Ground, Obstacle, Wall};
        let pivot = Vec3::new(0.0, 1.1, 0.0);
        let eye = Vec3::new(0.0, 2.3, 6.8);
        let g = Flat(0.0, Surface::Gravel);
        assert_eq!(chase_clearance(pivot, eye, &g, &[]), 1.0, "nothing in the way");
        // A traffic car 4 m behind (its cylinders, 1.4 m tall): the camera stops short of it.
        let car = [Obstacle::solid(Vec2::new(0.0, 4.0), 0.95, 1.4), Obstacle::solid(Vec2::new(0.0, 5.3), 0.95, 1.4)];
        let c = chase_clearance(pivot, eye, &g, &car);
        let at = pivot.lerp(eye, c);
        assert!(c < 1.0 && at.z < 4.0 - 0.95, "{c} {at}");
        // A thin trunk between the car and the camera: no jump; one at the eye: in front of it.
        let mid = [Obstacle::solid(Vec2::new(0.0, 3.0), 0.25, 9.0)];
        assert_eq!(chase_clearance(pivot, eye, &g, &mid), 1.0);
        let at_eye = [Obstacle::solid(Vec2::new(0.0, 6.7), 0.3, 9.0)];
        assert!(chase_clearance(pivot, eye, &g, &at_eye) < 0.9);
        // A building wall behind.
        struct Walled;
        impl Ground for Walled {
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
            }
            fn walls(&self, _: Vec2, _: f32, out: &mut Vec<Wall>) {
                out.clear();
                out.push(Wall::new(Vec2::new(-10.0, 3.5), Vec2::new(10.0, 3.5), 0.2, 6.0));
            }
        }
        let c = chase_clearance(pivot, eye, &Walled, &[]);
        assert!(pivot.lerp(eye, c).z < 3.5 - 0.2, "{c}");
        // The speed kick: none at hairpin speed, a few degrees flat out.
        assert_eq!(speed_fov(30.0 / 3.6), 0.0);
        assert!((5.0..=7.0).contains(&speed_fov(160.0 / 3.6)));
    }

    #[test]
    fn chase_camera_shows_the_slide() {
        let nose = Vec3::NEG_Z;
        // 10 degrees of sideslip at 25 m/s: the camera turns about 6 degrees towards travel.
        let travel = (Quat::from_rotation_y(0.1745) * nose) * 25.0;
        let d = chase_dir(nose, travel);
        let deg = d.angle_between(nose).to_degrees();
        assert!((5.0..7.0).contains(&deg), "{deg}");
        assert_eq!(chase_dir(nose, travel * 0.05), nose, "walking pace: on the nose");
        assert_eq!(chase_dir(nose, -nose * 5.0), nose, "reversing: on the nose");
    }

    /// One value drives the FOV, the blur and the wind (AUDIO_PASS §8.8).
    #[test]
    fn speed_feel_drives_fov_blur_and_wind_together() {
        for kmh in [0.0f32, 30.0, 40.0, 60.0, 90.0, 120.0, 150.0, 180.0, 220.0] {
            let v = kmh / 3.6;
            let f = speed_feel(v);
            assert_eq!(speed_fov(v), fov_kick(f, false));
            assert_eq!(crate::audio::car::wind_air(v), crate::audio::car::wind_air_for(f));
            if kmh <= 40.0 {
                assert_eq!((f, speed_fov(v), shutter_for(f), crate::audio::car::wind_air(v)), (0.0, 0.0, 0.0, 0.0), "{kmh}");
            }
        }
        // Each rises with the feel.
        let mut last = (-1.0, -1.0, -1.0);
        for i in 0..=20 {
            let f = i as f32 / 20.0;
            let now = (fov_kick(f, false), shutter_for(f), crate::audio::car::wind_air_for(f));
            assert!(now.0 >= last.0 && now.1 >= last.1 && now.2 >= last.2);
            last = now;
        }
        assert!(shutter_for(speed_feel(60.0 / 3.6)) < 0.01, "no blur at 60");
        let at160 = shutter_for(speed_feel(160.0 / 3.6));
        assert!((0.3..0.4).contains(&at160), "{at160}");
        assert!(speed_feel(180.0 / 3.6) > 0.99);
        assert!(fov_kick(1.0, true) <= 5.0, "cockpit cap");
    }

    #[test]
    fn springs_are_frame_rate_independent_and_settle() {
        for (w, z) in [(5.0, 0.8), (7.0, 1.0), (3.5, 1.0), (4.0, 0.9)] {
            let run = |dt: f32| {
                let mut s = Spring { x: 1.0, v: 0.0 };
                let steps = (0.5 / dt).round() as usize;
                for _ in 0..steps {
                    s.step(w, z, dt);
                }
                s.x
            };
            let (a, b) = (run(1.0 / 30.0), run(1.0 / 240.0));
            assert!((a - b).abs() < 1e-4, "ω {w} ζ {z}: {a} vs {b}");
            let mut s = Spring { x: 1.0, v: 0.0 };
            for _ in 0..240 {
                s.step(w, z, 1.0 / 60.0);
            }
            assert!(s.x.abs() < 0.01, "settles: {}", s.x);
        }
        // Critically damped: never overshoots.
        let mut s = Spring { x: 1.0, v: 0.0 };
        for _ in 0..200 {
            s.step(7.0, 1.0, 1.0 / 60.0);
            assert!(s.x >= -1e-6);
        }
    }

    /// Drives the arm: a car accelerating along +z, braking, a hairpin and a landing.
    #[test]
    fn spring_arm_stretches_squashes_swings_and_dips() {
        let dt = 1.0 / 60.0;
        let boom = rest_boom(0.0, 1);
        assert_eq!(boom, (6.8, 2.3));
        let mut arm = Arm::new(Vec3::ZERO, 0.0, boom);
        // Accelerating at 8 m/s²: the boom stretches.
        let (mut pos, mut v) = (Vec3::ZERO, 0.0f32);
        let mut max_long = 0.0f32;
        for _ in 0..90 {
            v += 8.0 * dt;
            pos.z += v * dt;
            arm.step(pos, 0.0, 0.0, boom, 1.0, dt);
            max_long = max_long.max(arm.long.x);
        }
        assert!(max_long > 0.2 && max_long <= LONG.hi, "stretch {max_long}");
        // Braking hard: it closes in.
        let mut min_long = 0.0f32;
        for _ in 0..60 {
            v = (v - 12.0 * dt).max(0.0);
            pos.z += v * dt;
            arm.step(pos, 0.0, 0.0, boom, 1.0, dt);
            min_long = min_long.min(arm.long.x);
        }
        assert!(min_long < -0.2 && min_long >= LONG.lo, "squash {min_long}");
        // A hairpin: the car turns 180° in 1.5 s; the arm lags up to 25° and catches up.
        let mut arm = Arm::new(Vec3::ZERO, 0.0, boom);
        let mut worst = 0.0f32;
        for i in 0..90 {
            let h = std::f32::consts::PI * (i as f32 / 90.0);
            let (eye, _) = arm.step(Vec3::ZERO, h, h, boom, 1.0, dt);
            worst = worst.max(wrap(arm.heading - h).abs());
            assert!(eye.y > 1.5);
        }
        assert!(worst > 0.1 && worst <= HEADING.hi + 1e-4, "lag {} deg", worst.to_degrees());
        for _ in 0..120 {
            arm.step(Vec3::ZERO, std::f32::consts::PI, std::f32::consts::PI, boom, 1.0, dt);
        }
        assert!(wrap(arm.heading - std::f32::consts::PI).abs() < 0.01, "caught up");
        // A landing kick dips about the asked depth, within the limit, and comes back.
        let mut arm = Arm::new(Vec3::ZERO, 0.0, boom);
        arm.kick(0.3, 1.0);
        let mut deepest = 0.0f32;
        for _ in 0..120 {
            arm.step(Vec3::ZERO, 0.0, 0.0, boom, 1.0, dt);
            deepest = deepest.min(arm.vert.x);
        }
        assert!((-0.4..-0.2).contains(&deepest), "dip {deepest}");
        assert!(arm.vert.x.abs() < 0.01);
    }

    #[test]
    fn chase_rest_pose_moves_with_speed_and_distance() {
        let (l0, h0) = rest_boom(0.0, 1);
        let (l1, h1) = rest_boom(200.0 / 3.6, 1);
        assert!((l1 - l0 - 0.8).abs() < 1e-4 && (h0 - h1 - 0.25).abs() < 1e-4);
        assert!(rest_boom(0.0, 0).0 < l0 && rest_boom(0.0, 2).0 > l0);
    }
}
