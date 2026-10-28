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

#[derive(Component)]
struct CarWheel(usize);

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
}

/// Bonnet (true) or chase camera.
#[derive(Resource, Clone, Copy, Default)]
pub struct CameraMode(pub bool);

pub struct DrivePlugin;

impl Plugin for DrivePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CarSpec>()
            .init_resource::<StageRun>()
            .init_resource::<CameraMode>()
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

/// Spawns the car at the start line, with placeholder visuals (the asset car replaces them).
pub fn spawn_car(world: &mut World) {
    let Some(s) = world.get_resource::<StageWorld>().cloned() else { return };
    let (pos, heading) = s.road.start();
    let spec = world.resource::<CarSpec>().0.clone();
    let car = Car::new(pos + Vec3::Y * 0.9, heading);
    let tf = Transform::from_translation(car.pos).with_rotation(car.rot);
    // Headless (tests, a dedicated server): the car without visuals.
    if !world.contains_resource::<Assets<StandardMaterial>>() {
        world.spawn((Name::new("car"), DrivenCar { prev: (car.pos, car.rot), car }, CarBody, tf, Visibility::default()));
        world.insert_resource(StageRun { countdown: 3.0, ..default() });
        return;
    }
    let body_mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(spec.size.x, spec.size.y * 0.75, spec.size.z));
    let cabin_mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(spec.size.x * 0.82, spec.size.y * 0.5, spec.size.z * 0.5));
    let wheel_mesh = world.resource_mut::<Assets<Mesh>>().add(Cylinder::new(spec.wheel_radius, 0.24).mesh().resolution(18).build().rotated_by(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)));
    let paint = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { base_color: Color::srgb(0.85, 0.12, 0.1), perceptual_roughness: 0.35, metallic: 0.2, ..default() });
    let glass = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { base_color: Color::srgb(0.08, 0.1, 0.12), perceptual_roughness: 0.1, ..default() });
    let tyre = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { base_color: Color::srgb(0.06, 0.06, 0.06), perceptual_roughness: 0.9, ..default() });
    world
        .spawn((Name::new("car"), DrivenCar { prev: (car.pos, car.rot), car }, CarBody, tf, Visibility::default(), Actor))
        .with_children(|p| {
            p.spawn((Mesh3d(body_mesh), MeshMaterial3d(paint), Transform::from_xyz(0.0, -0.05, 0.0), Actor));
            p.spawn((Mesh3d(cabin_mesh), MeshMaterial3d(glass), Transform::from_xyz(0.0, 0.4, 0.25), Actor));
        });
    for i in 0..4 {
        world.spawn((Name::new("wheel"), CarWheel(i), Mesh3d(wheel_mesh.clone()), MeshMaterial3d(tyre.clone()), tf, Actor));
    }
    world.insert_resource(StageRun { countdown: 3.0, ..default() });
}

fn controls(actions: Res<ActionState>, mut cam: ResMut<CameraMode>) {
    if actions.just_pressed("camera") {
        cam.0 = !cam.0;
    }
}

fn simulate(
    time: Res<Time<Fixed>>,
    actions: Res<ActionState>,
    spec: Res<CarSpec>,
    stage: Res<StageWorld>,
    mut run: ResMut<StageRun>,
    mut cars: Query<(Entity, &mut DrivenCar)>,
    mut ghost: Option<ResMut<crate::meta::ghost::Recorder>>,
) {
    let dt = time.delta_secs();
    for (e, mut c) in &mut cars {
        let c = &mut *c;
        c.prev = (c.car.pos, c.car.rot);
        let held = run.countdown > 0.0 || run.finished.is_some();
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
        // Clock.
        if run.countdown > 0.0 {
            run.countdown -= dt;
            continue;
        }
        if run.finished.is_some() {
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

fn draw_car(
    fixed: Res<Time<Fixed>>,
    spec: Res<CarSpec>,
    mut bodies: Query<(&DrivenCar, &mut Transform), With<CarBody>>,
    mut wheels: Query<(&CarWheel, &mut Transform), Without<CarBody>>,
) {
    let a = fixed.overstep_fraction();
    let Ok((c, mut tf)) = bodies.single_mut() else { return };
    tf.translation = c.prev.0.lerp(c.car.pos, a);
    tf.rotation = c.prev.1.slerp(c.car.rot, a);
    let offset = tf.translation - c.car.pos;
    for (w, mut wt) in &mut wheels {
        let i = w.0;
        let steer = if i < 2 { c.car.steer } else { 0.0 };
        wt.translation = c.car.wheel_pos(&spec.0, i) + offset;
        wt.rotation = tf.rotation * Quat::from_rotation_y(steer) * Quat::from_rotation_x(-c.car.wheels[i].angle);
    }
}

fn follow_camera(
    time: Res<Time>,
    mode: Res<CameraMode>,
    cars: Query<&Transform, (With<CarBody>, Without<MainCamera>)>,
    mut cams: Query<&mut Transform, With<MainCamera>>,
    mut smooth: Local<Option<(Vec3, Vec3)>>,
) {
    let (Ok(car), Ok(mut cam)) = (cars.single(), cams.single_mut()) else { return };
    let fwd = car.rotation * Vec3::NEG_Z;
    let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::NEG_Z);
    if mode.0 {
        cam.translation = car.translation + car.rotation * Vec3::new(0.0, 0.62, -0.55);
        cam.rotation = car.rotation;
        *smooth = None;
        return;
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
