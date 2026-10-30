//! Driving: input, the fixed-step car simulation, the car on screen, cameras, stage timing.

use bevy::input::gamepad::{GamepadAxis, GamepadButton};
use bevy::prelude::*;
use engine::actions::{ActionDef, ActionState, AddAction, Binding};
use engine::gfx::MainCamera;
use engine::render_budget::Actor;

use crate::vehicle::{self, Car, Input, Spec};
use crate::world::StageWorld;

/// The car being driven (on the driver's machine; the guide gets it replicated).
#[derive(Component, Clone, Debug)]
pub struct DrivenCar {
    pub car: Car,
    /// The pose before the last fixed step, for smooth drawing.
    pub prev: (Vec3, Quat),
}

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
    /// The car was wrecked: the run is over without a time.
    pub retired: bool,
}

/// The driver's camera, cycled with the camera action: cockpit → bonnet → chase. A run
/// starts in the cockpit; the choice then sticks for later stages.
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
            .init_resource::<StageRun>()
            .init_resource::<CameraView>()
            .add_action(ActionDef::axis1d("steer").label("Steer").group("Driving").bind(Binding::key_axis(KeyCode::KeyD, KeyCode::KeyA)).bind(Binding::key_axis(KeyCode::ArrowRight, KeyCode::ArrowLeft)).bind(Binding::pad_axis(GamepadAxis::LeftStickX)))
            .add_action(ActionDef::button("throttle").label("Throttle").group("Driving").bind(Binding::key(KeyCode::KeyW)).bind(Binding::key(KeyCode::ArrowUp)).bind(Binding::pad(GamepadButton::RightTrigger2)))
            .add_action(ActionDef::button("brake").label("Brake / reverse").group("Driving").bind(Binding::key(KeyCode::KeyS)).bind(Binding::key(KeyCode::ArrowDown)).bind(Binding::pad(GamepadButton::LeftTrigger2)))
            .add_action(ActionDef::button("handbrake").label("Handbrake").group("Driving").bind(Binding::key(KeyCode::Space)).bind(Binding::pad(GamepadButton::South)))
            .add_action(ActionDef::button("recover").label("Recover (+5 s)").group("Driving").bind(Binding::key(KeyCode::KeyR)).bind(Binding::pad(GamepadButton::North)))
            .add_action(ActionDef::button("camera").label("Camera").group("Driving").bind(Binding::key(KeyCode::KeyC)).bind(Binding::pad(GamepadButton::Select)))
            .add_systems(FixedUpdate, simulate.run_if(resource_exists::<StageWorld>))
            .add_systems(Update, (controls, draw_car, follow_camera).chain().run_if(resource_exists::<StageWorld>));
    }
}

/// Spawns the car at the start line; `art::car` adds the model and drives its wheels.
pub fn spawn_car(world: &mut World) {
    let Some(s) = world.get_resource::<StageWorld>().cloned() else { return };
    let (pos, heading) = s.road.start();
    let car = Car::new(pos + Vec3::Y * 0.9, heading);
    let tf = Transform::from_translation(car.pos).with_rotation(car.rot);
    let id = world.spawn((Name::new("car"), DrivenCar { prev: (car.pos, car.rot), car }, CarBody, tf, Visibility::default(), Actor)).id();
    crate::art::car::spawn(world, id);
    world.insert_resource(StageRun { countdown: 3.0, ..default() });
    // Every stage starts seated in the car.
    world.insert_resource(CameraView::Cockpit);
}

fn controls(actions: Res<ActionState>, mut cam: ResMut<CameraView>) {
    if actions.just_pressed("camera") {
        *cam = cam.next();
    }
}

#[allow(clippy::too_many_arguments)]
fn simulate(
    time: Res<Time<Fixed>>,
    actions: Res<ActionState>,
    spec: Res<CarSpec>,
    stage: Res<StageWorld>,
    mut run: ResMut<StageRun>,
    mut cars: Query<(Entity, &mut DrivenCar)>,
    mut ghost: Option<ResMut<crate::meta::ghost::Recorder>>,
    loading: Option<Res<crate::loading::Loading>>,
) {
    let dt = time.delta_secs();
    // Nothing moves behind the loading screen (the ghost recording starts with the countdown).
    if loading.is_some() {
        return;
    }
    for (e, mut c) in &mut cars {
        let c = &mut *c;
        c.prev = (c.car.pos, c.car.rot);
        let held = run.countdown > 0.0 || run.finished.is_some() || run.retired;
        // Held on the handbrake (the foot brake at a standstill would select reverse).
        let input = if held {
            Input { handbrake: true, ..default() }
        } else {
            Input { steer: actions.value("steer"), throttle: actions.value("throttle"), brake: actions.value("brake"), handbrake: actions.pressed("handbrake") }
        };
        if let Some(g) = ghost.as_deref_mut() {
            g.input(e, input);
        }
        vehicle::step(&mut c.car, &spec.0, input, &*stage, dt);
        if run.countdown > 0.0 {
            run.countdown -= dt;
            continue;
        }
        if run.finished.is_some() || run.retired {
            continue;
        }
        if c.car.damage.wrecked() {
            info!("retired: car wrecked at {:.0} m ({:?})", run.progress, c.car.damage);
            run.retired = true;
            continue;
        }
        run.time += dt;
        let p = Vec2::new(c.car.pos.x, c.car.pos.z);
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
        if actions.just_pressed("recover") {
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
}

fn draw_car(fixed: Res<Time<Fixed>>, mut bodies: Query<(&DrivenCar, &mut Transform), With<CarBody>>) {
    let a = fixed.overstep_fraction();
    let Ok((c, mut tf)) = bodies.single_mut() else { return };
    tf.translation = c.prev.0.lerp(c.car.pos, a);
    tf.rotation = c.prev.1.slerp(c.car.rot, a);
}

fn follow_camera(
    time: Res<Time>,
    view: Res<CameraView>,
    spec: Res<CarSpec>,
    cars: Query<&Transform, (With<CarBody>, Without<MainCamera>)>,
    mut cams: Query<&mut Transform, With<MainCamera>>,
    mut smooth: Local<Option<(Vec3, Vec3)>>,
    mut head: Local<Option<Quat>>,
) {
    let (Ok(car), Ok(mut cam)) = (cars.single(), cams.single_mut()) else { return };
    let fwd = car.rotation * Vec3::NEG_Z;
    let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::NEG_Z);
    match *view {
        CameraView::Cockpit => {
            // Fixed to the seat; the view direction follows the body through a short lag so
            // the suspension's high-frequency shake doesn't reach the eye. Pitched a little
            // down, the way a driver looks at the road.
            let k = 1.0 - (-time.delta_secs() * 18.0).exp();
            let look = match *head {
                Some(q) if q.dot(car.rotation).abs() > 0.97 => q.slerp(car.rotation, k),
                _ => car.rotation,
            };
            *head = Some(look);
            cam.translation = car.translation + car.rotation * crate::art::car::cockpit_eye(&spec.0);
            cam.rotation = look * Quat::from_rotation_x(-0.05);
            *smooth = None;
            return;
        }
        CameraView::Bonnet => {
            cam.translation = car.translation + car.rotation * crate::art::car::bonnet_cam(&spec.0);
            cam.rotation = car.rotation;
            *smooth = None;
            *head = None;
            return;
        }
        CameraView::Chase => *head = None,
    }
    let want_eye = car.translation - flat * 6.8 + Vec3::Y * 2.3;
    let want_at = car.translation + flat * 4.0 + Vec3::Y * 0.9;
    let k = 1.0 - (-time.delta_secs() * 6.0).exp();
    let (eye, at) = match *smooth {
        Some((e, a)) => (e.lerp(want_eye, k), a.lerp(want_at, (k * 1.6).min(1.0))),
        None => (want_eye, want_at),
    };
    *smooth = Some((eye, at));
    *cam = Transform::from_translation(eye).looking_at(at, Vec3::Y);
}
