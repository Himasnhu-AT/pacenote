//! Driving: input, the fixed-step car simulation, the car on screen, cameras, stage timing.

use bevy::input::gamepad::{GamepadAxis, GamepadButton};
use bevy::prelude::*;
use engine::actions::{ActionDef, ActionState, AddAction, Binding, Device};
use engine::gfx::MainCamera;
use engine::render_budget::Actor;

use crate::vehicle::{self, Car, DamageMode, Input, Spec};
use crate::world::StageWorld;

/// The car being driven (on the driver's machine; the guide gets it replicated).
#[derive(Component, Clone, Debug)]
pub struct DrivenCar {
    pub car: Car,
    /// The pose before the last fixed step, for smooth drawing.
    pub prev: (Vec3, Quat),
}

/// The input the simulation applied on the last step (the cockpit animates the handbrake from it).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct DriverInput(pub Input);

#[derive(Component)]
struct CarBody;

#[derive(Resource, Clone, Default)]
pub struct CarSpec(pub Spec);

/// The run: countdown, clock, penalties, result.
#[derive(Resource, Clone, Debug, Default)]
pub struct StageRun {
    /// Seconds of countdown left (3 → 0), then the clock runs.
    pub countdown: f32,
    pub time: f32,
    pub penalty: f32,
    pub finished: Option<f32>,
    /// Metres along the road (best so far: driving backwards doesn't count).
    pub progress: f32,
    /// Split times at each quarter of the stage.
    pub splits: Vec<f32>,
    /// The run is over without a time (dev `meta.retire`, the guide's view of a retired run).
    pub retired: bool,
    /// The car is out of action: wrecked (waiting for a recover) or the driver is dead.
    pub down: Option<Down>,
    /// The crash in progress: (`Damage::total` before its first hit, seconds since its last
    /// hit). Hits less than [`CRASH_GAP`] apart are one crash (a trunk, then the bank).
    pub crash: Option<(f32, f32)>,
}

/// The driver's camera, cycled with the camera action: cockpit → bonnet → chase. The choice is
/// remembered (`meta::Profile::camera`, synced both ways by meta) and runs start in it.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraView {
    #[default]
    Cockpit,
    Bonnet,
    Chase,
}

impl CameraView {
    pub fn next(self) -> Self {
        match self {
            CameraView::Cockpit => CameraView::Bonnet,
            CameraView::Bonnet => CameraView::Chase,
            CameraView::Chase => CameraView::Cockpit,
        }
    }
}

pub struct DrivePlugin;

impl Plugin for DrivePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CarSpec>()
            .init_resource::<SteerSettings>()
            .init_resource::<RecoverPress>()
            .init_resource::<StageRun>()
            .init_resource::<CameraView>()
            .add_action(ActionDef::axis1d("steer").label("Steer").group("Driving").bind(Binding::key_axis(KeyCode::KeyD, KeyCode::KeyA)).bind(Binding::key_axis(KeyCode::ArrowRight, KeyCode::ArrowLeft)).bind(Binding::pad_axis(GamepadAxis::LeftStickX)))
            .add_action(ActionDef::button("throttle").label("Throttle").group("Driving").bind(Binding::key(KeyCode::KeyW)).bind(Binding::key(KeyCode::ArrowUp)).bind(Binding::pad(GamepadButton::RightTrigger2)))
            .add_action(ActionDef::button("brake").label("Brake / reverse").group("Driving").bind(Binding::key(KeyCode::KeyS)).bind(Binding::key(KeyCode::ArrowDown)).bind(Binding::pad(GamepadButton::LeftTrigger2)))
            .add_action(ActionDef::button("handbrake").label("Handbrake").group("Driving").bind(Binding::key(KeyCode::Space)).bind(Binding::pad(GamepadButton::South)))
            .add_action(ActionDef::button("recover").label("Recover (+5 s)").group("Driving").bind(Binding::key(KeyCode::KeyR)).bind(Binding::pad(GamepadButton::North)))
            .add_action(ActionDef::button("camera").label("Camera").group("Driving").bind(Binding::key(KeyCode::KeyC)).bind(Binding::pad(GamepadButton::Select)))
            .init_resource::<AutoInput>()
            .add_systems(FixedUpdate, simulate.in_set(SimulateSet).run_if(resource_exists::<StageWorld>))
            .add_systems(Update, (controls, draw_car, follow_camera).chain().run_if(resource_exists::<StageWorld>));
    }
}

/// Spawns the car at the start line; `art::car` adds the model and drives its wheels.
pub fn spawn_car(world: &mut World) {
    let Some(s) = world.get_resource::<StageWorld>().cloned() else { return };
    let (pos, heading) = match &s.open {
        // Free roam: in town, no countdown.
        Some(o) => {
            let (p, d) = o.spawn();
            (Vec3::new(p.x, s.heights.height_at(p.x, p.z), p.z), d.x.atan2(d.y))
        }
        None => s.road.start(),
    };
    let car = Car::new(pos + Vec3::Y * 0.9, heading);
    let tf = Transform::from_translation(car.pos).with_rotation(car.rot);
    let id = world.spawn((Name::new("car"), DrivenCar { prev: (car.pos, car.rot), car }, DriverInput::default(), CarBody, tf, Visibility::default(), Actor)).id();
    crate::art::car::spawn(world, id);
    world.insert_resource(StageRun { countdown: if s.free_roam() { 0.0 } else { 3.0 }, ..default() });
    // The view is the player's choice (Settings > Game > Camera view, kept in meta::Profile).
}

fn controls(actions: Res<ActionState>, mut cam: ResMut<CameraView>, mut recover: ResMut<RecoverPress>) {
    if actions.just_pressed("camera") {
        *cam = cam.next();
    }
    // Latched here (once a frame) and used up by the next fixed step: read in the fixed step
    // directly, a press was lost on frames with no step (high frame rates) and counted twice
    // on frames with two (below 60 fps, +10 s).
    if actions.just_pressed("recover") {
        recover.0 = true;
    }
}

/// The fixed step that runs the car (for systems that must run just before it).
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SimulateSet;

/// When set, the input for this fixed step instead of the player's (dev autopilot,
/// `src/autopilot.rs`). Systems that set it run `.before(SimulateSet)`.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct AutoInput(pub Option<Input>);

/// The recover button was pressed since the last fixed step.
#[derive(Resource, Default)]
pub struct RecoverPress(pub bool);

#[allow(clippy::too_many_arguments)]
fn simulate(
    time: Res<Time<Fixed>>,
    actions: Res<ActionState>,
    spec: Res<CarSpec>,
    stage: Res<StageWorld>,
    mut run: ResMut<StageRun>,
    mut cars: Query<(Entity, &mut DrivenCar, Option<&mut DriverInput>)>,
    mut ghost: Option<ResMut<crate::meta::ghost::Recorder>>,
    loading: Option<Res<crate::loading::Loading>>,
    mut log: Local<Option<CarLog>>,
    settings: Option<Res<SteerSettings>>,
    mut recover: Option<ResMut<RecoverPress>>,
    auto: Option<Res<AutoInput>>,
) {
    let settings = settings.map(|s| *s).unwrap_or_default();
    let dt = time.delta_secs();
    // Nothing moves behind the loading screen (the ghost recording starts with the countdown).
    if loading.is_some() {
        return;
    }
    for (e, mut c, last) in &mut cars {
        let c = &mut *c;
        c.prev = (c.car.pos, c.car.rot);
        let held = run.countdown > 0.0 || run.finished.is_some() || run.retired || run.down.is_some();
        // Held on the handbrake (the foot brake at a standstill would select reverse).
        let input = if held {
            Input { handbrake: true, ..default() }
        } else if let Some(i) = auto.as_ref().and_then(|a| a.0) {
            // A dev autopilot is driving (the same path as the player's input: recorded,
            // animated by the cockpit hands).
            i
        } else {
            // Keys are on/off: ramp them (the ghost records the ramped value, so replays match).
            // A stick is already analogue and passes straight through.
            let raw = actions.value("steer");
            let steer = if actions.last_device == Device::KeyboardMouse {
                let prev = last.as_ref().map_or(0.0, |l| l.0.steer);
                keyboard_steer_at(prev, raw, c.car.speed(), dt, settings.keyboard_speed)
            } else {
                settings.stick(raw)
            };
            Input { steer, throttle: actions.value("throttle"), brake: actions.value("brake"), handbrake: actions.pressed("handbrake") }
        };
        if let Some(g) = ghost.as_deref_mut() {
            g.input(e, input);
        }
        if let Some(mut l) = last {
            l.0 = input;
        }
        let (before, hits) = (c.car.damage.total(), c.car.damage.hits);
        vehicle::step(&mut c.car, &spec.0, input, &*stage, dt);
        log.get_or_insert_with(CarLog::open).write(&c.car, input, actions.value("steer"), &stage);
        if run.countdown > 0.0 {
            run.countdown -= dt;
            continue;
        }
        if run.finished.is_some() || run.retired {
            continue;
        }
        // Damage (Simulation only): a single crash that takes half the car or more (the mean
        // of engine, steering and suspension) kills the driver (respawn at home, or the event
        // restarts); a broken part stops the car until the player recovers it (back on the road
        // where it is, repaired).
        run.crash = match run.crash {
            _ if c.car.damage.hits > hits => Some((run.crash.map_or(before, |(b, _)| b), 0.0)),
            Some((b, t)) if t + dt < CRASH_GAP => Some((b, t + dt)),
            _ => None,
        };
        if spec.0.damage == DamageMode::Simulation && run.down.is_none() {
            let crash = run.crash.map_or(0.0, |(b, _)| c.car.damage.total() - b);
            if crash >= DEATH_HIT {
                info!("dead: one crash took {:.0} % of the car ({:?})", crash * 100.0, c.car.damage);
                run.down = Some(Down::Dead(DEAD_SECS));
            } else if c.car.damage.wrecked() {
                info!("not drivable at {:.0} m ({:?})", run.progress, c.car.damage);
                run.down = Some(Down::Wrecked);
            }
        }
        match run.down {
            Some(Down::Dead(t)) => {
                let t = t - dt;
                run.down = Some(Down::Dead(t));
                // An event restarts from its line (`meta::restart_after_death`); free roam
                // puts the car back at home, repaired.
                if t <= 0.0 && stage.free_roam() {
                    let (sp, sd) = stage.open.as_ref().map_or((c.car.pos, Vec2::Y), |o| o.spawn());
                    let at = Vec3::new(sp.x, stage.heights.height_at(sp.x, sp.z), sp.z);
                    let h = sd.x.atan2(sd.y);
                    put_back(&mut c.car, e, at, h, ghost.as_deref_mut());
                    c.prev = (c.car.pos, c.car.rot);
                    run.down = None;
                }
                continue;
            }
            Some(Down::Wrecked) => {
                if !stage.free_roam() {
                    run.time += dt;
                }
                if recover.as_deref_mut().is_some_and(|r| std::mem::take(&mut r.0)) {
                    let (at, h) = road_here(&stage, &c.car, run.progress);
                    put_back(&mut c.car, e, at, h, ghost.as_deref_mut());
                    c.prev = (c.car.pos, c.car.rot);
                    run.down = None;
                    if !stage.free_roam() {
                        run.penalty += 5.0;
                    }
                }
                continue;
            }
            None => {}
        }
        run.time += dt;
        let p = Vec2::new(c.car.pos.x, c.car.pos.z);
        // Free roam: no route, so no progress, splits or finish; recover onto the nearest road,
        // facing the way the car was going.
        if stage.free_roam() {
            if recover.as_deref_mut().is_some_and(|r| std::mem::take(&mut r.0)) {
                let (at, h) = road_here(&stage, &c.car, run.progress);
                vehicle::recover(&mut c.car, at, h);
                c.prev = (c.car.pos, c.car.rot);
            }
            continue;
        }
        if let Some(rp) = stage.road.nearest(p, 40.0) {
            // Progress only counts near the road and forwards.
            if rp.s > run.progress && rp.s < run.progress + 40.0 {
                run.progress = rp.s;
            }
        }
        let len = stage.road.length();
        let quarter = len / 4.0;
        while run.splits.len() < 3 && run.progress >= quarter * (run.splits.len() + 1) as f32 {
            let t = run.time + run.penalty;
            run.splits.push(t);
        }
        if run.progress >= len - 4.0 {
            run.finished = Some(run.time + run.penalty);
        }
        if recover.as_deref_mut().is_some_and(|r| std::mem::take(&mut r.0)) {
            let i = stage.road.station(run.progress);
            let (p, d) = (stage.road.pts[i], stage.road.dir[i]);
            vehicle::recover(&mut c.car, p, d.x.atan2(d.y));
            if let Some(g) = ghost.as_deref_mut() {
                g.recover(e, p, d.x.atan2(d.y));
            }
            c.prev = (c.car.pos, c.car.rot);
            run.penalty += 5.0;
        }
    }
    // A press only counts for the step that sees it (not, say, held over from the countdown).
    if let Some(mut r) = recover {
        r.0 = false;
    }
}

/// Dev aid: with `PACENOTE_CARLOG=<file>` set, every fixed step of the driven car is appended
/// to that file as CSV (input, attitude, rates, gear, each wheel's load / slip / compression),
/// for reviewing how the car drives in the real game. Off (no file) otherwise.
pub struct CarLog {
    out: Option<std::io::BufWriter<std::fs::File>>,
    step: u64,
    prev: Option<Vec3>,
}

impl CarLog {
    fn open() -> CarLog {
        let out = std::env::var("PACENOTE_CARLOG").ok().and_then(|p| std::fs::File::create(p).ok()).map(std::io::BufWriter::new);
        let mut log = CarLog { out, step: 0, prev: None };
        if let Some(o) = log.out.as_mut() {
            use std::io::Write;
            let _ = writeln!(
                o,
                "step,raw_steer,steer_in,throttle,brake,hb,steer_rad,kmh,beta_deg,yaw_dps,roll_deg,pitch_deg,lat_g,long_g,gear,rpm,x,y,z,surface,load,slip_ratio,slip_angle_deg,comp,contact"
            );
        }
        log
    }

    fn write(&mut self, car: &Car, input: Input, raw: f32, stage: &StageWorld) {
        use std::io::Write;
        use vehicle::Ground;
        let Some(o) = self.out.as_mut() else { return };
        let (fwd, up) = (car.forward(), car.up());
        let left = up.cross(fwd);
        let v = car.vel;
        let beta = v.dot(left).atan2(v.dot(fwd).abs().max(0.5)).to_degrees();
        // Accelerations from the velocity change since the last logged step.
        let a = self.prev.map_or(Vec3::ZERO, |p| (v - p) * 60.0);
        self.prev = Some(v);
        let w = &car.wheels;
        let f4 = |f: &dyn Fn(usize) -> f32| (0..4).map(|i| format!("{:.3}", f(i))).collect::<Vec<_>>().join(" ");
        let _ = writeln!(
            o,
            "{},{:.2},{:.3},{:.2},{:.2},{},{:.4},{:.2},{:.2},{:.2},{:.2},{:.2},{:.3},{:.3},{},{:.0},{:.2},{:.2},{:.2},{:?},{},{},{},{},{}",
            self.step,
            raw,
            input.steer,
            input.throttle,
            input.brake,
            input.handbrake as u8,
            car.steer,
            car.speed() * 3.6,
            beta,
            car.ang.dot(up).to_degrees(),
            left.y.asin().to_degrees(),
            fwd.y.asin().to_degrees(),
            a.dot(left) / 9.81,
            a.dot(fwd) / 9.81,
            car.gear,
            car.rpm,
            car.pos.x,
            car.pos.y,
            car.pos.z,
            stage.surface(car.pos.x, car.pos.z),
            f4(&|i| w[i].load),
            f4(&|i| w[i].slip_ratio),
            f4(&|i| w[i].slip_angle.to_degrees()),
            f4(&|i| w[i].compression),
            (0..4).map(|i| if w[i].contact { '1' } else { '0' }).collect::<String>(),
        );
        self.step += 1;
        if self.step.is_multiple_of(60) {
            let _ = o.flush();
        }
    }
}

/// The car is out of action (Simulation damage only, `StageRun::down`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Down {
    /// A part broke: the car won't drive until the player recovers it (repaired, back on the
    /// road where it is; +5 s on a timed route).
    Wrecked,
    /// One crash took [`DEATH_HIT`] or more: "You're dead" for the seconds left, then home
    /// (free roam) or the start line (events).
    Dead(f32),
}

/// Damage from one crash ([`vehicle::Damage::total`], 0..1) that kills the driver: about a
/// 125 km/h head-on into a trunk, ~110 km/h on a front corner. An 80 km/h tree costs ~0.2.
pub const DEATH_HIT: f32 = 0.6;
/// Hits closer together than this (s) are one crash.
pub const CRASH_GAP: f32 = 0.5;
/// How long "You're dead" shows before the respawn.
pub const DEAD_SECS: f32 = 2.5;

/// The road where the car is: the route at the progress on a timed route, else the nearest
/// road (facing the way the car was going), else home.
fn road_here(stage: &StageWorld, car: &Car, progress: f32) -> (Vec3, f32) {
    let fwd = car.rot * Vec3::NEG_Z;
    if !stage.free_roam() {
        let i = stage.road.station(progress);
        let d = stage.road.dir[i];
        return (stage.road.pts[i], d.x.atan2(d.y));
    }
    match stage.nearest_road(Vec2::new(car.pos.x, car.pos.z), 400.0) {
        Some((road, rp)) => {
            let i = rp.i.min(road.len() - 1);
            let d = road.dir[i];
            let d = if d.dot(Vec2::new(fwd.x, fwd.z)) < 0.0 { -d } else { d };
            (road.pts[i], d.x.atan2(d.y))
        }
        None => match stage.open.as_ref() {
            Some(o) => {
                let (sp, sd) = o.spawn();
                (Vec3::new(sp.x, stage.heights.height_at(sp.x, sp.z), sp.z), sd.x.atan2(sd.y))
            }
            None => (car.pos, fwd.x.atan2(-fwd.z)),
        },
    }
}

/// Puts the car down at `at` facing `heading`, repaired (and records both for the ghost).
fn put_back(car: &mut Car, e: Entity, at: Vec3, heading: f32, ghost: Option<&mut crate::meta::ghost::Recorder>) {
    vehicle::recover(car, at, heading);
    vehicle::repair(car);
    if let Some(g) = ghost {
        g.recover(e, at, heading);
        g.repair(e);
    }
}

/// Player steering settings (the Settings page's Driving tab). Input shaping only: it runs
/// before the input is recorded, so ghosts replay exactly whatever the setting was, and it
/// doesn't touch the car spec or the sim hash.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct SteerSettings {
    /// Keyboard steering speed: scales how fast a held key winds the steering on (0.6–1.6,
    /// 1 = default).
    pub keyboard_speed: f32,
    /// Stick linearity: 1 = linear, up to ~2.5 = less sensitive around the centre (input
    /// raised to this power, sign kept).
    pub stick_linearity: f32,
    /// Stick deadzone (0–0.3): smaller deflections are centre, the rest rescaled to 0–1.
    pub stick_deadzone: f32,
}

impl Default for SteerSettings {
    fn default() -> Self {
        SteerSettings { keyboard_speed: 1.0, stick_linearity: 1.0, stick_deadzone: 0.05 }
    }
}

impl SteerSettings {
    /// A stick deflection (−1..1) shaped by the deadzone and the linearity.
    pub fn stick(&self, x: f32) -> f32 {
        let dz = self.stick_deadzone.clamp(0.0, 0.5);
        let m = ((x.abs() - dz) / (1.0 - dz)).clamp(0.0, 1.0);
        m.powf(self.stick_linearity.clamp(1.0, 3.0)) * x.signum()
    }
}

/// Keyboard steering: the input ramps towards the held key instead of jumping to full lock.
/// It winds on fairly quickly at low speed (hairpins, ~0.35 s to full) and progressively slower
/// with speed (~0.85 s at 110 km/h), so a tap is a small correction and a held key a committed
/// turn. (Playtesters found 0.2 s / 0.6 s twitchy.)
/// Letting go, or switching sides, unwinds faster than it winds on (a driver lets the wheel
/// self-centre). The vehicle's own speed-sensitive lock ([`vehicle::steer_lock`]) then makes
/// full input the most useful angle at that speed.
pub fn keyboard_steer(prev: f32, target: f32, speed: f32, dt: f32) -> f32 {
    keyboard_steer_at(prev, target, speed, dt, 1.0)
}

/// [`keyboard_steer`] with the wind-on rate scaled by `speed_scale` (the player's keyboard
/// steering speed; letting go always unwinds at the full rate).
pub fn keyboard_steer_at(prev: f32, target: f32, speed: f32, dt: f32, speed_scale: f32) -> f32 {
    let t = (speed.abs() / 30.0).clamp(0.0, 1.0);
    let wind = (3.2 + (1.2 - 3.2) * t) * speed_scale.clamp(0.3, 3.0);
    let unwind = 7.0;
    let target = target.clamp(-1.0, 1.0);
    // Winding on: moving away from centre on the same side as the key.
    let rate = if target * prev >= 0.0 && target.abs() > prev.abs() { wind } else { unwind };
    prev + (target - prev).clamp(-rate * dt, rate * dt)
}

fn draw_car(fixed: Res<Time<Fixed>>, mut bodies: Query<(&DrivenCar, &mut Transform), With<CarBody>>) {
    let a = fixed.overstep_fraction();
    let Ok((c, mut tf)) = bodies.single_mut() else { return };
    tf.translation = c.prev.0.lerp(c.car.pos, a);
    tf.rotation = c.prev.1.slerp(c.car.rot, a);
}

/// The drawn car and its sim state, for the cameras.
type CarPose = (&'static Transform, Option<&'static DrivenCar>);

#[allow(clippy::too_many_arguments)]
fn follow_camera(
    time: Res<Time>,
    view: Res<CameraView>,
    spec: Res<CarSpec>,
    stage: Res<StageWorld>,
    cars: Query<CarPose, (With<CarBody>, Without<MainCamera>)>,
    mut cams: Query<(&mut Transform, Option<&mut Projection>), With<MainCamera>>,
    access: Option<Res<engine::actions::Accessibility>>,
    core: Option<Res<engine::settings::CoreSettings>>,
    traffic: Option<Res<crate::open::traffic::TrafficSolids>>,
    mut smooth: Local<Option<(Vec3, Vec3)>>,
    mut head: Local<Option<Quat>>,
    mut cam_fx: Local<CamFx>,
) {
    // Settings > Driving > Camera shake (0..1; Reduce motion = 0): how much of the body's
    // pitch and roll reaches the cockpit and bonnet views, and how much the speed widens the
    // view and the road buzzes through the seat.
    let shake = access.as_deref().map_or(1.0, |a| if a.reduce_motion { 0.0 } else { a.camera_shake.clamp(0.0, 1.0) });
    let (Ok((car, driven)), Ok((mut cam, proj))) = (cars.single(), cams.single_mut()) else { return };
    let dt = time.delta_secs();
    let fwd = car.rotation * Vec3::NEG_Z;
    let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::NEG_Z);
    let speed = driven.map_or(0.0, |d| d.car.vel.length());
    // Speed widens the view a little (up to +7° by 180 km/h), eased so a hit doesn't snap it.
    let kick = speed_fov(speed) * shake;
    cam_fx.fov += (kick - cam_fx.fov) * (1.0 - (-dt * 2.0).exp());
    if let (Some(mut proj), Some(core)) = (proj, core.as_deref())
        && let Projection::Perspective(pp) = proj.as_mut()
    {
        let fov = (core.fov_deg.clamp(30.0, 150.0) + cam_fx.fov).to_radians();
        if (pp.fov - fov).abs() > 1e-4 {
            pp.fov = fov;
        }
    }
    // Road buzz in the cockpit and bonnet views: a few millimetres on gravel at speed, more on
    // rough ground, next to nothing on tarmac (camera only: the sim doesn't see it).
    let buzz = {
        use vehicle::Ground;
        let rough = surface_buzz(stage.surface(car.translation.x, car.translation.z));
        let on_ground = driven.is_none_or(|d| d.car.wheels.iter().any(|w| w.contact));
        let amp = if on_ground { 0.004 * rough * (speed / 25.0).min(1.5) * shake } else { 0.0 };
        let t = time.elapsed_secs();
        let n = |a: f32, b: f32, c: f32| (t * a).sin() * 0.5 + (t * b + 1.3).sin() * 0.3 + (t * c + 2.1).sin() * 0.2;
        (Vec3::new(n(17.0, 29.3, 41.1) * 0.4, n(13.1, 23.7, 37.9), 0.0) * amp, n(11.3, 19.1, 33.7) * amp * 0.35)
    };
    match *view {
        CameraView::Cockpit => {
            // Fixed to the seat; the view direction follows the body through a short lag so
            // the suspension's high-frequency shake doesn't reach the eye. Pitched a little
            // down, the way a driver looks at the road.
            let k = 1.0 - (-dt * (3.0 + 15.0 * shake)).exp();
            let look = match *head {
                Some(q) if q.dot(car.rotation).abs() > 0.97 => q.slerp(car.rotation, k),
                _ => car.rotation,
            };
            *head = Some(look);
            cam.translation = car.translation + car.rotation * (crate::art::car::cockpit_eye(&spec.0) + buzz.0);
            cam.rotation = look * Quat::from_rotation_x(-0.05 + buzz.1);
            *smooth = None;
            return;
        }
        CameraView::Bonnet => {
            cam.translation = car.translation + car.rotation * (crate::art::car::bonnet_cam(&spec.0) + buzz.0 * 0.7);
            // Fixed to the bonnet at full shake; smoothed towards the body below that.
            let body = if shake >= 0.99 {
                car.rotation
            } else {
                let k = 1.0 - (-dt * (3.0 + 27.0 * shake)).exp();
                match *head {
                    Some(q) if q.dot(car.rotation).abs() > 0.97 => q.slerp(car.rotation, k),
                    _ => car.rotation,
                }
            };
            *head = Some(body);
            cam.rotation = body * Quat::from_rotation_x(buzz.1 * 0.7);
            *smooth = None;
            return;
        }
        CameraView::Chase => *head = None,
    }
    // Partly along the direction of travel, so a slide shows as the car at an angle.
    let dir = chase_dir(flat, driven.map_or(Vec3::ZERO, |d| d.car.vel));
    let mut want_eye = car.translation - dir * 6.8 + Vec3::Y * 2.3;
    let want_at = car.translation + dir * 4.0 + Vec3::Y * 0.9;
    // Never under the ground behind the car (a crest, a bank).
    {
        use vehicle::Ground;
        want_eye.y = want_eye.y.max(stage.height(want_eye.x, want_eye.z) + 0.8);
    }
    let k = 1.0 - (-dt * 6.0).exp();
    let (eye, at) = match *smooth {
        Some((e, a)) => (e.lerp(want_eye, k), a.lerp(want_at, (k * 1.6).min(1.0))),
        None => (want_eye, want_at),
    };
    *smooth = Some((eye, at));
    // Pulled in front of whatever is between the car and the camera (a car behind, a
    // building, a bank, a big rock; a thin trunk only when the eye would be inside it): in
    // at once, back out slowly.
    let pivot = car.translation + Vec3::Y * 1.1;
    let mut solids = Vec::new();
    if let Some(t) = traffic.as_deref() {
        let m = (pivot + eye) * 0.5;
        t.obstacles(Vec2::new(m.x, m.z), 8.0, &mut solids);
    }
    let clear = chase_clearance(pivot, eye, &*stage, &solids);
    cam_fx.pull = if clear < cam_fx.pull || cam_fx.pull <= 0.0 { clear } else { cam_fx.pull + (clear - cam_fx.pull) * (1.0 - (-dt * 2.5).exp()) };
    let eye = pivot + (eye - pivot) * cam_fx.pull;
    *cam = Transform::from_translation(eye).looking_at(at, Vec3::Y);
}

/// Camera effects state: the speed FOV kick (degrees, eased) and the chase camera's pull-in
/// (share of the boom out, eased out).
#[derive(Default)]
struct CamFx {
    fov: f32,
    pull: f32,
}

/// Extra vertical field of view (degrees) at `speed` m/s: none to 40 km/h, +7 by 180 km/h.
pub fn speed_fov(speed: f32) -> f32 {
    let x = ((speed * 3.6 - 40.0) / 140.0).clamp(0.0, 1.0);
    7.0 * x * x * (3.0 - 2.0 * x)
}

/// How much road buzz a surface sends through the seat (gravel = 1).
fn surface_buzz(s: crate::stage::Surface) -> f32 {
    use crate::stage::Surface::*;
    match s {
        Tarmac => 0.15,
        Dusty => 0.3,
        Snow => 0.6,
        Gravel | Mud => 1.0,
        Loose => 1.2,
        Verge => 1.6,
        Rough | Ditch => 2.0,
    }
}

/// Radius (m) the chase camera keeps from solids and the ground.
const CAM_RADIUS: f32 = 0.3;
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
    use super::{chase_dir, keyboard_steer};
    use bevy::math::{Quat, Vec3};

    #[test]
    fn chase_camera_pulls_in_front_of_a_car_behind() {
        use crate::stage::Surface;
        use crate::vehicle::{Flat, Ground, Obstacle, Wall};
        use bevy::math::Vec2;
        let pivot = Vec3::new(0.0, 1.1, 0.0);
        let eye = Vec3::new(0.0, 2.3, 6.8);
        let g = Flat(0.0, Surface::Gravel);
        assert_eq!(super::chase_clearance(pivot, eye, &g, &[]), 1.0, "nothing in the way");
        // A traffic car 4 m behind (its cylinders, 1.4 m tall): the camera stops short of it.
        let car = [Obstacle::solid(Vec2::new(0.0, 4.0), 0.95, 1.4), Obstacle::solid(Vec2::new(0.0, 5.3), 0.95, 1.4)];
        let c = super::chase_clearance(pivot, eye, &g, &car);
        let at = pivot.lerp(eye, c);
        assert!(c < 1.0 && at.z < 4.0 - 0.95, "{c} {at}");
        // A thin trunk between the car and the camera: no jump; one at the eye: in front of it.
        let mid = [Obstacle::solid(Vec2::new(0.0, 3.0), 0.25, 9.0)];
        assert_eq!(super::chase_clearance(pivot, eye, &g, &mid), 1.0);
        let at_eye = [Obstacle::solid(Vec2::new(0.0, 6.7), 0.3, 9.0)];
        assert!(super::chase_clearance(pivot, eye, &g, &at_eye) < 0.9);
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
        let c = super::chase_clearance(pivot, eye, &Walled, &[]);
        assert!(pivot.lerp(eye, c).z < 3.5 - 0.2, "{c}");
        // The speed kick: none at hairpin speed, a few degrees flat out.
        assert_eq!(super::speed_fov(30.0 / 3.6), 0.0);
        assert!((5.0..=7.0).contains(&super::speed_fov(160.0 / 3.6)));
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

    /// Seconds for the keyboard ramp to reach `level` from centre at `kmh`.
    fn time_to(level: f32, kmh: f32) -> f32 {
        let (mut s, mut t) = (0.0, 0.0);
        while s < level {
            s = keyboard_steer(s, 1.0, kmh / 3.6, 1.0 / 60.0);
            t += 1.0 / 60.0;
        }
        t
    }

    #[test]
    fn keyboard_steering_ramps_with_speed() {
        // Quick at hairpin speed, progressive at speed: neither twitchy nor sluggish.
        let slow = time_to(1.0, 20.0);
        let fast = time_to(1.0, 110.0);
        assert!((0.25..0.45).contains(&slow), "{slow}");
        assert!((0.7..1.0).contains(&fast), "{fast}");
        // A one-frame tap at speed is a small correction.
        assert!(keyboard_steer(0.0, 1.0, 30.0, 1.0 / 60.0) < 0.05);
        // Releasing unwinds faster than winding on; switching sides passes through centre.
        let release = keyboard_steer(1.0, 0.0, 30.0, 0.1);
        assert!(release < 0.35, "{release}");
        let swap = keyboard_steer(0.5, -1.0, 30.0, 1.0 / 60.0);
        assert!(swap < 0.5 && swap > 0.3, "{swap}");
        assert_eq!(keyboard_steer(0.3, 0.3, 10.0, 1.0 / 60.0), 0.3);
        // The steering-speed setting scales winding on, not letting go.
        let slow = super::keyboard_steer_at(0.0, 1.0, 20.0, 0.05, 0.6);
        let quick = super::keyboard_steer_at(0.0, 1.0, 20.0, 0.05, 1.6);
        assert!((quick / slow - 1.6 / 0.6).abs() < 0.01, "{slow} {quick}");
        assert_eq!(super::keyboard_steer_at(1.0, 0.0, 20.0, 0.05, 0.6), keyboard_steer(1.0, 0.0, 20.0, 0.05));
    }

    #[test]
    fn stick_shaping() {
        let s = super::SteerSettings::default();
        assert_eq!(s.stick(0.03), 0.0);
        assert_eq!(s.stick(1.0), 1.0);
        assert_eq!(s.stick(-1.0), -1.0);
        let soft = super::SteerSettings { stick_linearity: 2.0, ..s };
        assert!(soft.stick(0.5) < s.stick(0.5) * 0.6 && soft.stick(-0.5) < 0.0);
    }
}
