//! The car on screen: the Car Concept model (CC BY 4.0, prepared by `tools/art/build_car.py`:
//! scaled to the simulation's wheelbase, logos removed, rally livery) under the simulated
//! body, its wheels driven from the simulation (steer, spin, suspension travel).
//!
//! The model's wheel hubs keep their own track and wheelbase position (X/Z); their height
//! follows each simulated wheel's suspension. Its tyres are a little bigger than the
//! simulation's (0.339 m vs `Spec::wheel_radius`), so hubs sit that much higher.

use bevy::prelude::*;
use engine::render_budget::Actor;

use crate::drive::{CarSpec, DrivenCar};

pub struct CarArtPlugin;

impl Plugin for CarArtPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (rig, wheels).chain());
    }
}

/// Model tyre radius (m), from the prepared model.
pub const MODEL_WHEEL_RADIUS: f32 = 0.339;

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
}

const NAMES: [&str; 4] = ["FL", "FR", "RL", "RR"];

/// Adds the model to the car entity. Visual worlds only.
pub fn spawn(world: &mut World, car: Entity) {
    if !super::visual(world) {
        return;
    }
    let scene = world.resource::<AssetServer>().load(GltfAssetLabel::Scene(0).from_asset("models/car.glb"));
    let model = world.spawn((Name::new("car model"), CarModel, WorldAssetRoot(scene), Transform::default(), Visibility::default(), Actor)).id();
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
        let mut parts = Vec::new();
        for e in children.iter_descendants(m) {
            parts.push(e);
            let Ok((n, tf)) = names.get(e) else { continue };
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
                    }
                }
            }
        }
        commands.entity(m).insert(CarRig { hub, spin, rest });
    }
}

/// Wheel hubs from the simulation: suspension height, steering, spin.
fn wheels(cars: Query<(&DrivenCar, &Children)>, rigs: Query<&CarRig>, spec: Option<Res<CarSpec>>, mut tfs: Query<&mut Transform>) {
    let Some(spec) = spec else { return };
    let s = &spec.0;
    for (c, kids) in &cars {
        let Some(rig) = kids.iter().find_map(|k| rigs.get(k).ok()) else { continue };
        let lift = MODEL_WHEEL_RADIUS - s.wheel_radius;
        for i in 0..4 {
            let w = &c.car.wheels[i];
            // Hub height in the body frame: mount minus the extended spring.
            let y = s.mounts[i].y - (s.rest - w.compression) + lift;
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
