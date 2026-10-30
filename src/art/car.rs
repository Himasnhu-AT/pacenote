//! The car on screen: an original gravel-rally hatchback built procedurally by
//! `tools/art/build_rallycar.py` (livery, wheels, cockpit interior) under the simulated body,
//! its wheels driven from the simulation (steer, spin, suspension travel).
//!
//! The model's wheel hubs keep their own track and wheelbase position (X/Z); their height
//! follows each simulated wheel's suspension. Its tyres are a little bigger than the
//! simulation's (0.325 m vs `Spec::wheel_radius`), so hubs sit that much higher.
//!
//! Cockpit: the steering wheel turns with the road wheels (`STEERING_RATIO`), the tacho and
//! speedo needles follow the engine and road speed, and the bonnet lamp pod is hidden in the
//! cockpit and bonnet views (it would block the road). The body shell is single-sided, so
//! from the driver's eye only the interior's own skin, dash and cage show.

use bevy::prelude::*;
use engine::render_budget::Actor;

use crate::drive::{CameraView, CarSpec, DrivenCar};

pub struct CarArtPlugin;

impl Plugin for CarArtPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (rig, wheels, cockpit).chain());
    }
}

/// Bonnet camera in the model's frame (origin at hub height): over the front of the bonnet
/// (the bonnet top is ~0.53 m above hub height there).
const BONNET_CAM_MODEL: Vec3 = Vec3::new(0.0, 0.72, -1.6);

/// Driver's eye in the model's frame: left seat, 1.12 m above the ground, 0.43 m behind the
/// steering wheel (`build_rallycar.py`: `DRV_X`, `STEER_C`).
const COCKPIT_EYE_MODEL: Vec3 = Vec3::new(-0.37, 0.795, 0.60);

/// Bonnet camera position in the car body's frame.
pub fn bonnet_cam(s: &crate::vehicle::Spec) -> Vec3 {
    BONNET_CAM_MODEL + Vec3::Y * model_height(s)
}

/// Driver's eye in the car body's frame.
pub fn cockpit_eye(s: &crate::vehicle::Spec) -> Vec3 {
    COCKPIT_EYE_MODEL + Vec3::Y * model_height(s)
}

/// Model tyre radius (m), from the prepared model.
pub const MODEL_WHEEL_RADIUS: f32 = 0.325;

/// Steering wheel turn per radian of road-wheel steer (full lock 0.56 rad ≈ 260°).
pub const STEERING_RATIO: f32 = 8.0;

/// The model under the car entity.
#[derive(Component)]
pub struct CarModel;

/// Found once the scene is spawned: hub and spin nodes (FL, FR, RL, RR) and the hubs' rest
/// positions.
#[derive(Component)]
pub struct CarRig {
    hub: [Entity; 4],
    spin: [Entity; 4],
    rest: [Vec3; 4],
    /// Cockpit nodes and their rest rotations: steering wheel, tacho, speedo.
    dials: [Option<(Entity, Quat)>; 3],
    /// The bonnet lamp pod.
    pod: Option<Entity>,
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
    let tf = Transform::from_xyz(0.0, model_height(&spec), 0.0);
    let model = world.spawn((Name::new("car model"), CarModel, WorldAssetRoot(scene), tf, Visibility::default(), Actor)).id();
    world.entity_mut(car).add_child(model);
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
        let mut spin = [Entity::PLACEHOLDER; 4];
        let mut rest = [Vec3::ZERO; 4];
        let mut dials = [None; 3];
        let mut pod = None;
        let mut parts = Vec::new();
        for e in children.iter_descendants(m) {
            parts.push(e);
            let Ok((n, tf)) = names.get(e) else { continue };
            if let Some(i) = DIALS.iter().position(|d| n.as_str() == *d) {
                dials[i] = Some((e, tf.rotation));
            } else if n.as_str() == "LightPod" {
                pod = Some(e);
            }
            for (i, k) in NAMES.iter().enumerate() {
                if n.as_str() == format!("Hub{k}") {
                    hub[i] = e;
                    rest[i] = tf.translation;
                } else if n.as_str() == format!("Spin{k}") {
                    spin[i] = e;
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
        commands.entity(m).insert(CarRig { hub, spin, rest, dials, pod });
    }
}

/// Wheel hubs from the simulation: suspension height, steering, spin.
fn wheels(cars: Query<(&DrivenCar, &Children)>, rigs: Query<&CarRig>, spec: Option<Res<CarSpec>>, mut tfs: Query<&mut Transform>) {
    let Some(spec) = spec else { return };
    let s = &spec.0;
    for (c, kids) in &cars {
        let Some(rig) = kids.iter().find_map(|k| rigs.get(k).ok()) else { continue };
        let (lift, base) = (MODEL_WHEEL_RADIUS - s.wheel_radius, model_height(s));
        for i in 0..4 {
            let w = &c.car.wheels[i];
            // Hub height in the body frame: mount minus the extended spring.
            // …relative to the model's origin.
            let y = s.mounts[i].y - (s.rest - w.compression) + lift - base;
            if let Ok(mut t) = tfs.get_mut(rig.hub[i]) {
                t.translation = Vec3::new(rig.rest[i].x, y, rig.rest[i].z);
                t.rotation = if i < 2 { Quat::from_rotation_y(c.car.steer) } else { Quat::IDENTITY };
            }
            if let Ok(mut t) = tfs.get_mut(rig.spin[i]) {
                t.rotation = Quat::from_rotation_x(-w.angle);
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
fn cockpit(view: Option<Res<CameraView>>, cars: Query<(&DrivenCar, &Children)>, rigs: Query<&CarRig>, mut tfs: Query<&mut Transform>, mut vis: Query<&mut Visibility>) {
    for (c, kids) in &cars {
        let Some(rig) = kids.iter().find_map(|k| rigs.get(k).ok()) else { continue };
        let car = &c.car;
        // Nodes turn about their local Z (the column / dial axis, towards the driver); a positive
        // angle turns anticlockwise as the driver sees it.
        let turn = [car.steer * STEERING_RATIO, -needle(car.rpm / 8000.0), -needle(car.speed().abs() * 3.6 / 200.0)];
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
        // Hubs ~0.5 m below the centre of mass; tyre bottoms reach the ground the sim uses.
        assert!((-0.6..-0.45).contains(&h), "{h}");
        let ground = s.mounts[0].y - s.rest + (s.mass * 9.81 / 4.0 / s.spring) - s.wheel_radius;
        assert!((h - MODEL_WHEEL_RADIUS - ground).abs() < 1e-4);
    }

    #[test]
    fn cameras_sit_in_and_over_the_car() {
        let s = crate::vehicle::Spec::default();
        let ground = model_height(&s) - MODEL_WHEEL_RADIUS;
        // Driver's eye: left seat, ~1.1 m over the ground, behind the front axle.
        let eye = cockpit_eye(&s);
        assert!(eye.x < -0.2 && (1.0..1.25).contains(&(eye.y - ground)) && eye.z > s.mounts[0].z, "{eye}");
        let bonnet = bonnet_cam(&s);
        assert!(bonnet.z < eye.z && bonnet.y - ground > 0.85, "{bonnet}");
    }

    #[test]
    fn needles_sweep_clockwise_from_lower_left() {
        assert!((needle(0.0) + 0.75 * std::f32::consts::PI).abs() < 1e-6);
        assert!(needle(0.5).abs() < 1e-6);
        assert!((needle(2.0) - 0.75 * std::f32::consts::PI).abs() < 1e-6);
    }
}
