//! The car on screen (`assets/models/rallycar.glb`) under the simulated body, its wheels driven
//! from the simulation (steer, spin, suspension travel). The model is the Mercedes-AMG GT3 Evo
//! download (`assets_src/SOURCES.md`), converted by `tools/art/import_sketchfab.py --profile
//! gt3`: scaled to the sim's wheelbase and track, decimated, its own cabin kept. The earlier
//! procedural hatchback (`tools/art/build_rallycar.py`) uses the same node names.
//!
//! The rig (node names from the importer):
//! - `Car` > `Chassis`: everything the sprung body carries (`Body`, the cabin, the cockpit
//!   nodes and the detachable parts in [`DETACHABLE`]). The simulated body already pitches and
//!   rolls, so `Chassis` stays at rest; it's the node to lean or shake the body on top of that.
//! - `Car` > `Hub<FL|FR|RL|RR>` (suspension: its height follows the simulated wheel) >
//!   `Steer<FL|FR>` (front knuckle: turns with the steering; the calliper rides it) >
//!   `Spin<..>` (rolls with the wheel) > `Wheel<..>` (rim, tyre, disc). Rear callipers ride the
//!   hub. Without `Steer` nodes (older models) the front hubs steer.
//! - `Car` > `CoDriverEye`, `CoDriverTablet`: anchors for the co-driver's view (`src/guide`).
//!
//! The hubs keep the model's track and wheelbase position (X/Z). Its tyres are a little bigger
//! than the simulation's (`MODEL_WHEEL_RADIUS` vs `Spec::wheel_radius`), so hubs sit that much
//! higher.
//!
//! The moving parts read [`DrivenCar`] where the car is simulated here, else a [`CarPose`]
//! (the guide's copy of the driver's car fills one each frame).
//!
//! Cockpit: the steering wheel turns with the road wheels (`STEERING_RATIO`); tacho and
//! speedo needles (when the model has them: the GT3 has a screen instead) follow the engine
//! and road speed, and a bonnet lamp pod (if any) is hidden in the cockpit and bonnet views.
//! The driver's hands, arms and the gear and handbrake levers are animated in `hands.rs`.

use bevy::prelude::*;
use engine::render_budget::Actor;

use crate::drive::{CameraView, CarSpec, DrivenCar, DriverInput};

pub struct CarArtPlugin;

impl Plugin for CarArtPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (rig, wheels, cockpit, super::hands::find_levers, super::hands::find_arms, super::hands::animate).chain());
    }
}

/// Bonnet camera in the model's frame (origin at hub height): over the front of the bonnet
/// (the GT3's bonnet top is ~0.39 m above hub height there).
const BONNET_CAM_MODEL: Vec3 = Vec3::new(0.0, 0.55, -1.55);

/// Driver's eye in the model's frame: left seat, 0.93 m above the ground (a low GT seat, the
/// roof bar just above the windscreen's top edge), 0.47 m behind the steering wheel's centre
/// (`hands.rs` `WHEEL_C`).
const COCKPIT_EYE_MODEL: Vec3 = Vec3::new(-0.36, 0.595, 0.69);

/// Bonnet camera position in the car body's frame.
pub fn bonnet_cam(s: &crate::vehicle::Spec) -> Vec3 {
    BONNET_CAM_MODEL + model_offset(s)
}

/// Driver's eye in the car body's frame.
pub fn cockpit_eye(s: &crate::vehicle::Spec) -> Vec3 {
    COCKPIT_EYE_MODEL + model_offset(s)
}

/// The model's wheel centres (`import_sketchfab.py` `HALF_TRACK`, `FRONT_Y`; `build_rallycar.py`
/// `HUB`): half track and half wheelbase, about
/// the model origin. They match the sim's mounts (tested), so drawn and simulated wheels agree.
pub const MODEL_HALF_TRACK: f32 = 0.78;
pub const MODEL_HALF_WHEELBASE: f32 = 1.275;

/// Where the model's origin sits in the body frame: at hub height (static load), midway between
/// the axles (the centre of mass is nearer the front axle).
pub fn model_offset(s: &crate::vehicle::Spec) -> Vec3 {
    Vec3::new(0.0, model_height(s), (s.mounts[0].z + s.mounts[2].z) * 0.5)
}

/// Model tyre radius (m), from the prepared model (the importer prints it).
pub const MODEL_WHEEL_RADIUS: f32 = 0.337;

/// Steering wheel turn per radian of road-wheel steer (full lock 0.56 rad ≈ 260°).
pub const STEERING_RATIO: f32 = 8.0;

/// Parts of the body that can come off, each its own node under `Chassis` (origin at the
/// part's middle, model frame). Not every model has every part.
pub const DETACHABLE: [&str; 7] = ["MirrorL", "MirrorR", "RearWing", "FrontSplitter", "FrontBumper", "RearBumper", "RearDiffuser"];

/// The model under the car entity.
#[derive(Component)]
pub struct CarModel;

/// What the car's moving parts show (steering, wheels, suspension, gauges, levers), for a car
/// entity that isn't simulated here (no [`DrivenCar`]): whoever owns that entity fills it
/// every frame.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct CarPose {
    /// Road-wheel steer (rad, + = left).
    pub steer: f32,
    pub rpm: f32,
    /// Forward speed (m/s).
    pub speed: f32,
    /// 1..=5 forward, −1 reverse.
    pub gear: i8,
    /// Wheel roll angles (rad): FL, FR, RL, RR.
    pub wheel_angle: [f32; 4],
    /// Spring compression (m): FL, FR, RL, RR.
    pub compression: [f32; 4],
    pub handbrake: bool,
}

impl CarPose {
    pub fn of(car: &crate::vehicle::Car, handbrake: bool) -> Self {
        CarPose {
            steer: car.steer,
            rpm: car.rpm,
            speed: car.speed(),
            gear: car.gear,
            wheel_angle: std::array::from_fn(|i| car.wheels[i].angle),
            compression: std::array::from_fn(|i| car.wheels[i].compression),
            handbrake,
        }
    }

    /// The pose of a car entity: its simulation if it has one, else its `CarPose`.
    pub fn read(driven: Option<&DrivenCar>, input: Option<&DriverInput>, shown: Option<&CarPose>) -> Option<Self> {
        match driven {
            Some(d) => Some(CarPose::of(&d.car, input.is_some_and(|i| i.0.handbrake))),
            None => shown.copied(),
        }
    }
}

/// Found once the scene is spawned: hub, knuckle and spin nodes (FL, FR, RL, RR), the hubs'
/// rest positions, the cockpit nodes, the chassis and the detachable parts.
#[derive(Component)]
pub struct CarRig {
    hub: [Entity; 4],
    /// Front knuckles (FL, FR), if the model has them.
    knuckle: [Option<Entity>; 2],
    spin: [Entity; 4],
    rest: [Vec3; 4],
    /// Cockpit nodes and their rest rotations: steering wheel, tacho, speedo.
    dials: [Option<(Entity, Quat)>; 3],
    /// The bonnet lamp pod.
    pod: Option<Entity>,
    chassis: Option<Entity>,
    parts: Vec<(&'static str, Entity)>,
}

impl CarRig {
    /// The sprung body's node (`Chassis`), if the model has one.
    pub fn chassis(&self) -> Option<Entity> {
        self.chassis
    }

    /// A detachable part's node by its [`DETACHABLE`] name.
    pub fn part(&self, name: &str) -> Option<Entity> {
        self.parts.iter().find(|(n, _)| *n == name).map(|(_, e)| *e)
    }

    /// Every detachable part the model has, with its name.
    pub fn parts(&self) -> &[(&'static str, Entity)] {
        &self.parts
    }
}

const NAMES: [&str; 4] = ["FL", "FR", "RL", "RR"];
const DIALS: [&str; 3] = ["SteeringWheel", "Tacho", "Speedo"];

/// Where the model's origin (its hub height) sits in the body frame: the front mounts at static
/// load, raised by the model's bigger tyres.
pub fn model_height(s: &crate::vehicle::Spec) -> f32 {
    let static_comp = (s.mass * 9.81 / 4.0 / s.spring).min(s.travel);
    s.mounts[0].y - (s.rest - static_comp) + (MODEL_WHEEL_RADIUS - s.wheel_radius)
}

/// Adds the model to the car entity. Visual worlds only.
pub fn spawn(world: &mut World, car: Entity) {
    if !super::visual(world) {
        return;
    }
    let spec = world.get_resource::<CarSpec>().map(|s| s.0.clone()).unwrap_or_default();
    let scene = world.resource::<AssetServer>().load(GltfAssetLabel::Scene(0).from_asset("models/rallycar.glb"));
    let tf = Transform::from_translation(model_offset(&spec));
    let model = world.spawn((Name::new("car model"), CarModel, WorldAssetRoot(scene), tf, Visibility::default(), Actor)).id();
    world.entity_mut(car).add_child(model);
    super::hands::spawn(world, car, model);
}

/// Finds the hub/spin nodes once the scene is in, tags every part as an `Actor` and gives the
/// model's textures mips.
fn rig(
    mut commands: Commands,
    models: Query<Entity, (With<CarModel>, Without<CarRig>)>,
    children: Query<&Children>,
    names: Query<(&Name, &Transform)>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>>,
    mats: Res<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for m in &models {
        let mut hub = [Entity::PLACEHOLDER; 4];
        let mut knuckle = [None; 2];
        let mut spin = [Entity::PLACEHOLDER; 4];
        let mut rest = [Vec3::ZERO; 4];
        let mut dials = [None; 3];
        let mut pod = None;
        let mut chassis = None;
        let mut detached = Vec::new();
        let mut parts = Vec::new();
        for e in children.iter_descendants(m) {
            parts.push(e);
            let Ok((n, tf)) = names.get(e) else { continue };
            if let Some(i) = DIALS.iter().position(|d| n.as_str() == *d) {
                dials[i] = Some((e, tf.rotation));
            } else if n.as_str() == "LightPod" {
                pod = Some(e);
            } else if n.as_str() == "Chassis" {
                chassis = Some(e);
            } else if let Some(p) = DETACHABLE.iter().find(|p| n.as_str() == **p) {
                detached.push((*p, e));
            }
            for (i, k) in NAMES.iter().enumerate() {
                if n.as_str() == format!("Hub{k}") {
                    hub[i] = e;
                    rest[i] = tf.translation;
                } else if n.as_str() == format!("Spin{k}") {
                    spin[i] = e;
                } else if i < 2 && n.as_str() == format!("Steer{k}") {
                    knuckle[i] = Some(e);
                }
            }
        }
        if hub.contains(&Entity::PLACEHOLDER) || spin.contains(&Entity::PLACEHOLDER) {
            continue;
        }
        // glTF textures come without mips (shimmer at distance): build them once, on the CPU.
        let mut done = std::collections::HashSet::new();
        for e in parts {
            commands.entity(e).insert(Actor);
            if let Ok(mm) = meshes.get(e)
                && let Some(mat) = mats.get(&mm.0)
            {
                for img in [&mat.base_color_texture, &mat.normal_map_texture, &mat.metallic_roughness_texture, &mat.emissive_texture, &mat.occlusion_texture].into_iter().flatten() {
                    if done.insert(img.id())
                        && let Some(mut i) = images.get_mut(img.id())
                    {
                        engine::gfx::build_mips(&mut i);
                        // The GPU has it now; drop the CPU copy.
                        i.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
                    }
                }
            }
        }
        commands.entity(m).insert(CarRig { hub, knuckle, spin, rest, dials, pod, chassis, parts: detached });
    }
}

/// Wheel hubs from the simulation: suspension height (hub), steering (front knuckle, else the
/// front hub), spin.
fn wheels(
    cars: Query<(Option<&DrivenCar>, Option<&CarPose>, &Children)>,
    rigs: Query<&CarRig>,
    spec: Option<Res<CarSpec>>,
    mut tfs: Query<&mut Transform>,
) {
    let Some(spec) = spec else { return };
    let s = &spec.0;
    for (driven, shown, kids) in &cars {
        let Some(p) = CarPose::read(driven, None, shown) else { continue };
        let Some(rig) = kids.iter().find_map(|k| rigs.get(k).ok()) else { continue };
        let (lift, base) = (MODEL_WHEEL_RADIUS - s.wheel_radius, model_height(s));
        for i in 0..4 {
            // Hub height in the body frame: mount minus the extended spring.
            // …relative to the model's origin.
            let y = s.mounts[i].y - (s.rest - p.compression[i]) + lift - base;
            let steer = if i < 2 { Quat::from_rotation_y(p.steer) } else { Quat::IDENTITY };
            let knuckle = rig.knuckle.get(i).copied().flatten();
            if let Ok(mut t) = tfs.get_mut(rig.hub[i]) {
                t.translation = Vec3::new(rig.rest[i].x, y, rig.rest[i].z);
                t.rotation = if knuckle.is_some() { Quat::IDENTITY } else { steer };
            }
            if let Some(k) = knuckle
                && let Ok(mut t) = tfs.get_mut(k)
            {
                t.rotation = steer;
            }
            if let Ok(mut t) = tfs.get_mut(rig.spin[i]) {
                t.rotation = Quat::from_rotation_x(-p.wheel_angle[i]);
            }
        }
    }
}

/// Needle angle (rad, clockwise from straight up as the driver sees it) for a reading
/// `frac` of full scale: a 270° sweep from lower left.
fn needle(frac: f32) -> f32 {
    (-0.75 + 1.5 * frac.clamp(0.0, 1.0)) * std::f32::consts::PI
}

/// Steering wheel and gauges from the simulation; the lamp pod hidden from the inside views.
fn cockpit(
    view: Option<Res<CameraView>>,
    cars: Query<(Option<&DrivenCar>, Option<&CarPose>, &Children)>,
    rigs: Query<&CarRig>,
    mut tfs: Query<&mut Transform>,
    mut vis: Query<&mut Visibility>,
) {
    for (driven, shown, kids) in &cars {
        let Some(car) = CarPose::read(driven, None, shown) else { continue };
        let Some(rig) = kids.iter().find_map(|k| rigs.get(k).ok()) else { continue };
        // Nodes turn about their local Z (the column / dial axis, towards the driver); a positive
        // angle turns anticlockwise as the driver sees it.
        let turn = [car.steer * STEERING_RATIO, -needle(car.rpm / 8000.0), -needle(car.speed.abs() * 3.6 / 200.0)];
        for (d, a) in rig.dials.iter().zip(turn) {
            if let Some((e, rest)) = *d
                && let Ok(mut t) = tfs.get_mut(e)
            {
                t.rotation = rest * Quat::from_rotation_z(a);
            }
        }
        if let Some(pod) = rig.pod
            && let Ok(mut v) = vis.get_mut(pod)
        {
            let want = if view.as_deref().is_some_and(|v| *v != CameraView::Chase) { Visibility::Hidden } else { Visibility::Inherited };
            if *v != want {
                *v = want;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_sits_on_its_wheels() {
        let s = crate::vehicle::Spec::default();
        let h = model_height(&s);
        // Hubs ~0.15 m below the centre of mass (which sits ~0.48 m off the ground); tyre
        // bottoms reach the ground the sim uses.
        assert!((-0.25..-0.08).contains(&h), "{h}");
        let ground = s.mounts[0].y - s.rest + (s.mass * 9.81 / 4.0 / s.spring) - s.wheel_radius;
        assert!((h - MODEL_WHEEL_RADIUS - ground).abs() < 1e-4);
    }

    #[test]
    fn model_wheels_match_the_sim_mounts() {
        let s = crate::vehicle::Spec::default();
        let off = model_offset(&s);
        let model = [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)];
        for (m, (sx, sz)) in s.mounts.iter().zip(model) {
            let x = sx * MODEL_HALF_TRACK + off.x;
            let z = sz * MODEL_HALF_WHEELBASE + off.z;
            assert!((m.x - x).abs() < 0.005 && (m.z - z).abs() < 0.005, "mount {m} vs model ({x}, {z})");
        }
    }

    #[test]
    fn cameras_sit_in_and_over_the_car() {
        let s = crate::vehicle::Spec::default();
        let ground = model_height(&s) - MODEL_WHEEL_RADIUS;
        // Driver's eye: left seat, ~0.9-1.2 m over the ground (a GT seat is low), behind the
        // front axle.
        let eye = cockpit_eye(&s);
        assert!(eye.x < -0.2 && (0.88..1.25).contains(&(eye.y - ground)) && eye.z > s.mounts[0].z, "{eye}");
        let bonnet = bonnet_cam(&s);
        assert!(bonnet.z < eye.z && bonnet.y - ground > 0.85, "{bonnet}");
    }

    /// Node names and parents in the shipped model (the glTF JSON chunk).
    fn shipped_nodes() -> Vec<(String, Option<String>)> {
        let b = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/models/rallycar.glb")).unwrap();
        let len = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
        let j: serde_json::Value = serde_json::from_slice(&b[20..20 + len]).unwrap();
        let nodes = j["nodes"].as_array().unwrap();
        let name = |i: usize| nodes[i]["name"].as_str().unwrap_or("").to_string();
        let mut parent = vec![None; nodes.len()];
        for (i, n) in nodes.iter().enumerate() {
            for c in n["children"].as_array().into_iter().flatten() {
                parent[c.as_u64().unwrap() as usize] = Some(name(i));
            }
        }
        (0..nodes.len()).map(|i| (name(i), parent[i].clone())).collect()
    }

    #[test]
    fn shipped_model_has_the_rig() {
        let nodes = shipped_nodes();
        let parent = |n: &str| nodes.iter().find(|(m, _)| m == n).unwrap_or_else(|| panic!("no node {n}")).1.clone();
        for k in NAMES {
            assert_eq!(parent(&format!("Hub{k}")).as_deref(), Some("Car"));
            let carrier = if k.starts_with('F') { format!("Steer{k}") } else { format!("Hub{k}") };
            if k.starts_with('F') {
                assert_eq!(parent(&carrier), Some(format!("Hub{k}")));
            }
            assert_eq!(parent(&format!("Spin{k}")), Some(carrier.clone()), "Spin{k}");
            assert_eq!(parent(&format!("Caliper{k}")), Some(carrier), "Caliper{k}: steers, doesn't spin");
            assert_eq!(parent(&format!("Wheel{k}")), Some(format!("Spin{k}")));
        }
        assert_eq!(parent("Chassis").as_deref(), Some("Car"));
        for n in ["Body", "SteeringWheel", "Shifter", "Handbrake"].into_iter().chain(DETACHABLE) {
            assert_eq!(parent(n).as_deref(), Some("Chassis"), "{n}");
        }
        for n in ["CoDriverEye", "CoDriverTablet"] {
            assert_eq!(parent(n).as_deref(), Some("Car"), "{n}");
        }
    }

    #[test]
    fn pose_comes_from_the_simulation_when_there_is_one() {
        let mut car = crate::vehicle::Car::new(Vec3::ZERO, 0.0);
        car.steer = 0.3;
        car.gear = 3;
        car.wheels[2].compression = 0.07;
        let shown = CarPose { steer: -0.1, gear: 2, ..default() };
        let driven = DrivenCar { car: car.clone(), prev: (Vec3::ZERO, Quat::IDENTITY) };
        let p = CarPose::read(Some(&driven), None, Some(&shown)).unwrap();
        assert_eq!((p.steer, p.gear, p.compression[2]), (0.3, 3, 0.07));
        assert_eq!(CarPose::read(None, None, Some(&shown)), Some(shown));
        assert_eq!(CarPose::read(None, None, None), None);
    }

    #[test]
    fn needles_sweep_clockwise_from_lower_left() {
        assert!((needle(0.0) + 0.75 * std::f32::consts::PI).abs() < 1e-6);
        assert!(needle(0.5).abs() < 1e-6);
        assert!((needle(2.0) - 0.75 * std::f32::consts::PI).abs() < 1e-6);
    }
}
