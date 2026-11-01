//! The guide in the passenger seat (Co-driver view: In car).
//!
//! - **The stage**: the guide's machine builds the stage's scenery (terrain, road, trees, sky)
//!   behind the same loading screen as the driver, but never the simulation: no `StageWorld`
//!   resource and no `DrivenCar` (the drive systems, HUD, AI co-driver and meta key on those).
//!   [`build_scene`] is the guide's half of `loading.rs`'s build step.
//! - **The car**: the same model as the driver's (`art::car::spawn`) on a [`GuideCar`] entity,
//!   posed every frame from the replicated car through [`PoseSmoother`] (no jitter, late
//!   packets carried on). [`ShownCar`] carries what the model animates from (steer, revs, wheel
//!   spin) for the car art to read.
//! - **The seat**: the camera sits at the co-driver's eye, opposite the driver (the model's
//!   `CoDriverEye` node if it has one, else the driver's eye mirrored across the centreline).
//!   The head is thrown about by the car's acceleration ([`HeadMotion`], scaled by Settings >
//!   Camera shake) and turned with the mouse within a natural range ([`Look`]).
//! - **The driver**: a seated silhouette (helmet, suit, arms to the wheel) in the other seat.
//!   The model's own driver arms (`art::hands`) take over the arms when they animate here.
//! - **The tablet**: `tablet.rs`.

use bevy::prelude::*;
use engine::gfx::MainCamera;
use engine::render_budget::Actor;
use engine::world::WorldDesc;

use super::head::{HeadMotion, Look};
use super::smooth::PoseSmoother;
use super::{GuideFeed, GuideScreen, GuideStage, GuideView};
use crate::drive::CarSpec;
use crate::stage;
use crate::world::StageWorld;

/// Name of the passenger eye node in `rallycar.glb` (model frame).
pub const EYE_NODE: &str = "CoDriverEye";

/// The car on the guide's machine (drawn, never simulated).
#[derive(Component)]
pub struct GuideCar;

/// What the car model animates from, on a car that isn't simulated here (the guide's).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct ShownCar {
    /// Road-wheel steer (rad, + = left).
    pub steer: f32,
    pub rpm: f32,
    /// Forward speed (m/s).
    pub speed: f32,
    pub gear: i8,
    /// Wheel rotation angles (rad): FL, FR, RL, RR.
    pub wheel_angle: [f32; 4],
    /// Spring compression (m), at static load (not replicated).
    pub compression: [f32; 4],
}

/// The co-driver's race suit.
pub const CODRIVER_SUIT: Color = Color::srgb(0.1, 0.11, 0.14);

/// The passenger-seat camera.
#[derive(Component)]
pub struct SeatCamera;

/// The driver silhouette's arms (hidden once the model's own arms animate).
#[derive(Component)]
struct SilhouetteArm;

/// Which stage's scenery is built on this machine (id, weather, time).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuideScene {
    pub id: u32,
    pub weather: stage::Weather,
    pub time: stage::TimeOfDay,
}

/// The ride: pose smoothing, head and look.
#[derive(Resource, Default)]
pub struct Ride {
    pub smoother: PoseSmoother,
    pub head: HeadMotion,
    pub look: Look,
    /// Co-driver eye in the car body's frame, once known.
    pub eye: Option<Vec3>,
    /// The eye came from the model's node (not the fallback).
    pub eye_from_model: bool,
}

/// System set of the seat's per-frame posing (the tablet runs after it).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeatSystems;

pub(super) fn build(app: &mut App) {
    app.init_resource::<Ride>().add_systems(OnExit(engine::flow::AppState::InGame), forget_scene).add_systems(
        Update,
        (load_scene, spawn_seat.run_if(no_seat), find_eye, spawn_body, ride, silhouette_arms)
            .chain()
            .in_set(SeatSystems)
            .after(super::FeedSystems)
            .run_if(in_car),
    );
}

/// Run condition: the guide's in-car view is wanted.
pub fn in_car(screen: Res<GuideScreen>, role: Res<crate::net_session::Role>, state: Option<Res<State<engine::flow::AppState>>>) -> bool {
    *screen == GuideScreen::InCar && super::view_wanted(role, state)
}

fn no_seat(q: Query<(), With<SeatCamera>>, scene: Option<Res<GuideScene>>) -> bool {
    q.is_empty() && scene.is_some()
}

/// Left the game: `meta::teardown` removed the scenery and the car.
fn forget_scene(mut commands: Commands, mut ride: ResMut<Ride>) {
    commands.remove_resource::<GuideScene>();
    ride.smoother.reset();
    ride.look = Look::default();
}

/// Starts building the stage's scenery once the guide knows the stage (and again when the
/// driver picks another stage or other conditions).
fn load_scene(world: &mut World) {
    let Some(g) = world.get_resource::<GuideStage>() else { return };
    let want = GuideScene { id: g.def.id, weather: g.def.weather, time: g.def.time };
    let def = g.def.clone();
    if world.get_resource::<GuideScene>() == Some(&want) || crate::loading::is_loading(world) || !crate::art::visual(world) {
        return;
    }
    if world.contains_resource::<GuideScene>() {
        info!("guide: the driver changed stage or conditions; rebuilding the scenery");
        crate::meta::teardown(world);
        world.remove_resource::<GuideScene>();
        world.resource_mut::<Ride>().smoother.reset();
    }
    crate::loading::begin(world, def);
}

/// The guide's half of the loading screen's build step: the stage's scenery without the
/// simulation (no `StageWorld` resource, no driven car). Same terrain as `world::spawn`.
pub fn build_scene(world: &mut World, s: &StageWorld) {
    let half = stage::half_size(&s.def);
    let desc = WorldDesc {
        seed: s.def.seed,
        terrain: engine::world::desc::TerrainDesc {
            size: [2.0 * half, 2.0 * half],
            spacing: stage::SPACING,
            chunk_quads: 64,
            origin: Some([-half, -half]),
            layers: crate::art::ground::layers(s.def.biome, crate::art::wet(&s.def)),
            ..default()
        },
        cell_size: 512.0,
        ..default()
    };
    if let Err(e) = engine::world::desc::spawn_world_with(world, desc, Some((*s.heights).clone())) {
        error!("terrain: {e}");
    }
    crate::art::spawn_stage(world, s);
    // Car and camera at the start line until the driver's car arrives.
    let (pos, heading) = s.road.start();
    world.insert_resource(GuideScene { id: s.def.id, weather: s.def.weather, time: s.def.time });
    spawn_car(world, pos + Vec3::Y * 0.9, Quat::from_rotation_y(heading));
}

/// The co-driver's own body under the camera: chest and legs (the tablet rests on the
/// thighs; the arms holding it are `tablet.rs`'s). Placed from the eye once it's known.
#[derive(Component)]
struct CoDriverBody;

fn spawn_body(mut commands: Commands, ride: Res<Ride>, cars: Query<Entity, With<GuideCar>>, old: Query<Entity, With<CoDriverBody>>, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>, mut placed: Local<Option<Vec3>>) {
    let (Some(eye), Some(car)) = (ride.eye, cars.iter().next()) else { return };
    if *placed == Some(eye) && !old.is_empty() {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    *placed = Some(eye);
    let suit = mats.add(StandardMaterial { base_color: CODRIVER_SUIT, perceptual_roughness: 0.85, ..default() });
    let boot = mats.add(StandardMaterial { base_color: Color::srgb(0.04, 0.04, 0.045), perceptual_roughness: 0.6, ..default() });
    let chest = meshes.add(Capsule3d::new(0.15, 0.2));
    let thigh = meshes.add(Capsule3d::new(0.075, 0.36));
    let shin = meshes.add(Capsule3d::new(0.055, 0.34));
    let foot = meshes.add(Cuboid::new(0.1, 0.08, 0.24));
    let mut part = |mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, tf: Transform| {
        commands.spawn((Name::new("co-driver body"), CoDriverBody, Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), tf, Actor, ChildOf(car)));
    };
    part(&chest, &suit, capsule_between(eye + Vec3::new(0.0, -0.36, 0.17), eye + Vec3::new(0.0, -0.62, 0.15), 0.5));
    for s in [-1.0f32, 1.0] {
        let hip = eye + Vec3::new(s * 0.1, -0.68, 0.1);
        let knee = eye + Vec3::new(s * 0.11, -0.58, -0.4);
        let ankle = eye + Vec3::new(s * 0.12, -0.98, -0.62);
        part(&thigh, &suit, capsule_between(hip, knee, 0.51));
        part(&shin, &suit, capsule_between(knee, ankle, 0.45));
        part(&foot, &boot, Transform::from_translation(ankle + Vec3::new(0.0, -0.03, -0.06)));
    }
}

/// The guide's car: model, wheels and the driver silhouette.
fn spawn_car(world: &mut World, pos: Vec3, rot: Quat) {
    let tf = Transform::from_translation(pos).with_rotation(rot);
    let car = world.spawn((Name::new("guide car"), GuideCar, ShownCar::default(), tf, Visibility::default(), Actor)).id();
    crate::art::car::spawn(world, car);
    spawn_driver(world, car);
}

/// The seat camera (and the tablet, `tablet.rs`), when the in-car view is (re)built.
fn spawn_seat(mut commands: Commands, cars: Query<&Transform, With<GuideCar>>) {
    let tf = cars.iter().next().copied().unwrap_or_default();
    commands.spawn((
        Name::new("co-driver camera"),
        GuideView,
        SeatCamera,
        MainCamera,
        Camera3d::default(),
        bevy::ui::IsDefaultUiCamera,
        Transform::from_translation(tf.translation + Vec3::Y * 1.2).with_rotation(tf.rotation),
    ));
}

/// The co-driver's eye in the car body's frame: the model's [`EYE_NODE`] when it has one,
/// else the driver's eye mirrored to the other seat.
fn find_eye(
    mut ride: ResMut<Ride>,
    spec: Option<Res<CarSpec>>,
    cars: Query<(Entity, &GlobalTransform), With<GuideCar>>,
    children: Query<&Children>,
    nodes: Query<(&Name, &GlobalTransform)>,
    rigs: Query<(), With<crate::art::car::CarRig>>,
) {
    if ride.eye_from_model {
        return;
    }
    let spec = spec.map(|s| s.0.clone()).unwrap_or_default();
    if ride.eye.is_none() {
        ride.eye = Some(mirrored_eye(&spec));
    }
    let Some((car, car_gt)) = cars.iter().next() else { return };
    // Look once the model is in (its rig found).
    if !children.iter_descendants(car).any(|e| rigs.contains(e)) {
        return;
    }
    for e in children.iter_descendants(car) {
        if let Ok((n, gt)) = nodes.get(e)
            && n.as_str() == EYE_NODE
        {
            let local = car_gt.affine().inverse().transform_point3(gt.translation());
            info!("guide: co-driver eye from the model at {local}");
            ride.eye = Some(local);
            ride.eye_from_model = true;
            return;
        }
    }
    // Not in this model: keep the mirrored eye (and stop looking).
    ride.eye_from_model = true;
}

/// The driver's eye mirrored across the car's centreline: the co-driver sits opposite.
pub fn mirrored_eye(spec: &crate::vehicle::Spec) -> Vec3 {
    let d = crate::art::car::cockpit_eye(spec);
    Vec3::new(-d.x, d.y, d.z)
}

/// Which side the driver sits: −1 left-hand drive, +1 right-hand drive.
pub fn driver_side(spec: &crate::vehicle::Spec) -> f32 {
    if crate::art::car::cockpit_eye(spec).x < 0.0 { -1.0 } else { 1.0 }
}

/// Every frame: the car from the smoothed network pose, the co-driver's head and look.
#[allow(clippy::too_many_arguments)]
fn ride(
    time: Res<Time>,
    feed: Res<GuideFeed>,
    look_in: Option<Res<engine::input::LookInput>>,
    access: Option<Res<engine::actions::Accessibility>>,
    paused: Option<Res<engine::pause::Paused>>,
    mut ride: ResMut<Ride>,
    mut cars: Query<(&mut Transform, &mut ShownCar), With<GuideCar>>,
    mut cams: Query<&mut Transform, (With<SeatCamera>, Without<GuideCar>)>,
) {
    let dt = time.delta_secs();
    let ride = &mut *ride;
    let Ok((mut car_tf, mut shown)) = cars.single_mut() else { return };
    if let Some(c) = feed.car {
        let (pos, rot) = ride.smoother.update(&c, dt);
        car_tf.translation = pos;
        car_tf.rotation = rot;
        let mut s = *shown;
        s.steer = c.steer;
        s.rpm = c.rpm;
        s.speed = c.speed();
        s.gear = c.gear;
        for i in 0..4 {
            s.wheel_angle[i] = (s.wheel_angle[i] + c.spin[i] * dt).rem_euclid(std::f32::consts::TAU);
        }
        shown.set_if_neq(s);
    }
    let shake = access.as_deref().map_or(1.0, |a| if a.reduce_motion { 0.0 } else { a.camera_shake.clamp(0.0, 1.0) });
    let accel_local = car_tf.rotation.inverse() * ride.smoother.accel();
    ride.head.step(accel_local, dt, shake);
    if !paused.is_some_and(|p| p.0)
        && let Some(l) = look_in
    {
        // Mouse right = look right (negative yaw about +Y).
        ride.look.add(-l.yaw, l.pitch);
    }
    let Ok(mut cam) = cams.single_mut() else { return };
    let eye = ride.eye.unwrap_or(Vec3::new(0.37, 0.6, 0.5));
    // The neck keeps the head a little steadier than the body (less so at full shake).
    let body = car_tf.rotation;
    let fwd = body * Vec3::NEG_Z;
    let level = Quat::from_rotation_y(f32::atan2(-fwd.x, -fwd.z));
    let head_frame = level.slerp(body, 0.55 + 0.4 * shake);
    cam.translation = car_tf.translation + body * (eye + ride.head.offset);
    cam.rotation = head_frame * ride.look.rotation() * ride.head.tilt();
}

/// Builds the driver silhouette in the other seat: helmet, suit, arms to the wheel.
fn spawn_driver(world: &mut World, car: Entity) {
    let spec = world.get_resource::<CarSpec>().map(|s| s.0.clone()).unwrap_or_default();
    let d = crate::art::car::cockpit_eye(&spec);
    let side = driver_side(&spec);
    let (helmet, suit, visor, glove) = {
        let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
        (
            mats.add(StandardMaterial { base_color: Color::srgb(0.92, 0.92, 0.94), perceptual_roughness: 0.35, ..default() }),
            mats.add(StandardMaterial { base_color: Color::srgb(0.08, 0.12, 0.22), perceptual_roughness: 0.85, ..default() }),
            mats.add(StandardMaterial { base_color: Color::srgb(0.03, 0.03, 0.04), perceptual_roughness: 0.1, metallic: 0.3, ..default() }),
            mats.add(StandardMaterial { base_color: Color::srgb(0.05, 0.05, 0.06), perceptual_roughness: 0.8, ..default() }),
        )
    };
    let (head, visor_mesh, torso, arm, hand) = {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        (
            meshes.add(Sphere::new(0.125).mesh().uv(24, 16)),
            meshes.add(Cuboid::new(0.19, 0.065, 0.06)),
            meshes.add(Capsule3d::new(0.17, 0.38)),
            meshes.add(Capsule3d::new(0.045, 0.27)),
            meshes.add(Sphere::new(0.05).mesh().uv(12, 8)),
        )
    };
    let part = |mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, tf: Transform| (Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), tf, Actor, ChildOf(car));
    // Helmet a little behind and above the eye, visor in front of it.
    world.spawn((Name::new("driver helmet"), part(&head, &helmet, Transform::from_translation(d + Vec3::new(0.0, 0.0, 0.02)))));
    world.spawn((Name::new("driver visor"), part(&visor_mesh, &visor, Transform::from_translation(d + Vec3::new(0.0, 0.0, -0.075)))));
    // Torso, leaning back against the seat.
    let chest = d + Vec3::new(0.0, -0.42, 0.14);
    world.spawn((Name::new("driver torso"), part(&torso, &suit, Transform::from_translation(chest).with_rotation(Quat::from_rotation_x(0.22)))));
    // Arms from the shoulders to the rim at quarter to three.
    let wheel = d + Vec3::new(0.0, -0.28, -0.42);
    for s in [-1.0f32, 1.0] {
        let shoulder = d + Vec3::new(s * 0.2, -0.22, 0.12);
        let grip = wheel + Vec3::new(s * 0.16, 0.04, 0.0);
        let elbow = (shoulder + grip) * 0.5 + Vec3::new(s * 0.08, -0.1, 0.0);
        for (a, b) in [(shoulder, elbow), (elbow, grip)] {
            world.spawn((Name::new("driver arm"), SilhouetteArm, part(&arm, &suit, capsule_between(a, b, 0.27 + 0.09))));
        }
        world.spawn((Name::new("driver glove"), SilhouetteArm, part(&hand, &glove, Transform::from_translation(grip))));
    }
    let _ = side;
}

/// A capsule (along +Y, total length `len`) placed from `a` to `b`.
pub fn capsule_between(a: Vec3, b: Vec3, len: f32) -> Transform {
    let d = b - a;
    let l = d.length().max(1e-4);
    Transform { translation: (a + b) * 0.5, rotation: Quat::from_rotation_arc(Vec3::Y, d / l), scale: Vec3::new(1.0, (l / len).max(0.2), 1.0) }
}

/// The silhouette's arms give way to the model's animated arms when those show.
fn silhouette_arms(cars: Query<Entity, With<GuideCar>>, children: Query<&Children>, arms: Query<&Visibility, With<crate::art::hands::Arms>>, mut ours: Query<&mut Visibility, (With<SilhouetteArm>, Without<crate::art::hands::Arms>)>) {
    let Some(car) = cars.iter().next() else { return };
    let model_arms = children.iter_descendants(car).any(|e| arms.get(e).is_ok_and(|v| *v != Visibility::Hidden));
    let want = if model_arms { Visibility::Hidden } else { Visibility::Inherited };
    for mut v in &mut ours {
        v.set_if_neq(want);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_co_driver_sits_opposite_the_driver() {
        let s = crate::vehicle::Spec::default();
        let d = crate::art::car::cockpit_eye(&s);
        let e = mirrored_eye(&s);
        assert!(d.x * e.x < 0.0, "driver {d}, co-driver {e}");
        assert_eq!((d.y, d.z), (e.y, e.z));
        assert_eq!(driver_side(&s), d.x.signum());
    }

    #[test]
    fn capsules_span_their_ends() {
        let (a, b) = (Vec3::new(0.1, 0.2, 0.3), Vec3::new(0.4, -0.1, 0.0));
        let t = capsule_between(a, b, 0.2);
        let tip = t.transform_point(Vec3::Y * 0.1);
        assert!(tip.distance(b) < 1e-4, "{tip} vs {b}");
    }
}
