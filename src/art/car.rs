//! The car on screen (`assets/models/rallycar.glb`) under the simulated body, its wheels driven
//! from the simulation (steer, spin, suspension travel). The model is the Mercedes-AMG GT3 Evo
//! download (`assets_src/SOURCES.md`), converted by `tools/art/import_sketchfab.py --profile
//! gt3`: scaled to the sim's wheelbase and track, decimated, its own cabin kept. The earlier
//! procedural hatchback (`tools/art/build_rallycar.py`) uses the same node names.
//!
//! The model's wheel hubs keep their own track and wheelbase position (X/Z); their height
//! follows each simulated wheel's suspension. Its tyres are a little bigger than the
//! simulation's (`MODEL_WHEEL_RADIUS` vs `Spec::wheel_radius`), so hubs sit that much higher.
//!
//! Cockpit: the steering wheel turns with the road wheels (`STEERING_RATIO`); tacho and
//! speedo needles (when the model has them: the GT3 has a screen instead) follow the engine
//! and road speed, and a bonnet lamp pod (if any) is hidden in the cockpit and bonnet views.
//! The driver's hands, arms and the gear and handbrake levers are animated in `hands.rs`.

use bevy::prelude::*;
use engine::render_budget::Actor;

use crate::drive::{CameraView, CarSpec, DrivenCar};

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

    #[test]
    fn needles_sweep_clockwise_from_lower_left() {
        assert!((needle(0.0) + 0.75 * std::f32::consts::PI).abs() < 1e-6);
        assert!(needle(0.5).abs() < 1e-6);
        assert!((needle(2.0) - 0.75 * std::f32::consts::PI).abs() < 1e-6);
    }
}
