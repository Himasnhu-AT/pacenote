//! The car on screen (`assets/models/rallycar.glb`) under the simulated body, its wheels driven
//! from the simulation (steer, spin, suspension travel). The model is "Talus", an original
//! 5-door rally hatch built procedurally by `tools/art/build_rallycar.py` (design sheet:
//! `docs/cars/rally_hatch.md`): body, livery, wheels, detachable parts, cockpit, seated driver.
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
//! Levels of detail: a mesh `X` with lower-detail twins `X_lod1`, `X_lod2` (the body and the
//! wheels) crossfades to them with distance ([`LOD_RANGES`], Bevy `VisibilityRange`); meshes
//! without twins fade out with LOD0 (cabin, driver, callipers), except the small parts drawn at
//! every distance ([`EVERY_LOD`]: the detachable parts and the lamp pod).
//!
//! The hubs keep the model's track and wheelbase position (X/Z). Its tyres are a little bigger
//! than the simulation's (`MODEL_WHEEL_RADIUS` vs `Spec::wheel_radius`), so hubs sit that much
//! higher.
//!
//! The moving parts read [`DrivenCar`] where the car is simulated here, else a [`CarPose`]
//! (the guide's copy of the driver's car fills one each frame).
//!
//! Cockpit: the steering wheel turns with the road wheels (`STEERING_RATIO`); tacho and
//! speedo needles follow the engine and road speed, and the bonnet lamp pod is hidden in the
//! cockpit and bonnet views.
//! The driver's hands, arms and the gear and handbrake levers are animated in `hands.rs`.

use bevy::camera::visibility::VisibilityRange;
use bevy::gltf::{GltfMaterialName, GltfMeshName};
use bevy::prelude::*;
use engine::render_budget::Actor;

use crate::drive::{CameraView, CarSpec, DrivenCar, DriverInput};

pub struct CarArtPlugin;

impl Plugin for CarArtPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, cabin_fill.run_if(resource_exists::<crate::daytime::DayClock>))
            .add_systems(Update, (rig, wheels, cockpit, wear, super::hands::find_levers, super::hands::find_arms, super::hands::animate).chain()).add_plugins(super::mirror::MirrorPlugin);
    }
}

/// Bonnet camera in the model's frame (origin at hub height): over the front of the bonnet
/// (the bonnet top is ~0.56 m above hub height there).
const BONNET_CAM_MODEL: Vec3 = Vec3::new(0.0, 0.72, -1.6);

/// Driver's eye in the model's frame: left seat, 1.12 m above the ground, 0.43 m behind the
/// steering wheel (`build_rallycar.py` / `cockpit.py`: `DRV_X`, `STEER_C`).
const COCKPIT_EYE_MODEL: Vec3 = Vec3::new(-0.37, 0.795, 0.60);

/// Bonnet camera position in the car body's frame.
pub fn bonnet_cam(s: &crate::vehicle::Spec) -> Vec3 {
    BONNET_CAM_MODEL + model_offset(s)
}

/// Driver's eye in the car body's frame.
pub fn cockpit_eye(s: &crate::vehicle::Spec) -> Vec3 {
    COCKPIT_EYE_MODEL + model_offset(s)
}

/// The model's wheel centres (`build_rallycar.py` `HUB`): half track and half wheelbase, about
/// the model origin. They match the sim's mounts (tested), so drawn and simulated wheels agree.
pub const MODEL_HALF_TRACK: f32 = 0.78;
pub const MODEL_HALF_WHEELBASE: f32 = 1.275;

/// Where the model's origin sits in the body frame: at hub height (static load), midway between
/// the axles (the centre of mass is nearer the front axle).
pub fn model_offset(s: &crate::vehicle::Spec) -> Vec3 {
    Vec3::new(0.0, model_height(s), (s.mounts[0].z + s.mounts[2].z) * 0.5)
}

/// The cabin's fill light: under the roof between the seats (model frame) and its power.
const CABIN_FILL_MODEL: Vec3 = Vec3::new(0.0, 0.80, 0.50);
const CABIN_FILL_LUMENS: f32 = 6000.0;

/// Model tyre radius (m): `build_rallycar.py` `R`.
pub const MODEL_WHEEL_RADIUS: f32 = 0.325;

/// Steering wheel turn per radian of road-wheel steer (full lock 0.56 rad ≈ 260°).
pub const STEERING_RATIO: f32 = 8.0;

/// Parts of the body that can come off, each its own node under `Chassis` (origin at the
/// part's middle, model frame). Not every model has every part.
pub const DETACHABLE: [&str; 7] = ["MirrorL", "MirrorR", "RearWing", "FrontSplitter", "FrontBumper", "RearBumper", "RearDiffuser"];

/// Distance bands (m, from the camera to the mesh's origin) of the levels of detail: LOD0
/// fades out over the first, LOD1 in; LOD1 out over the second, LOD2 in.
pub const LOD_RANGES: [std::ops::Range<f32>; 2] = [28.0..34.0, 85.0..95.0];

/// Meshes drawn at every distance (small, no twins): the detachable parts and the lamp pod.
pub const EVERY_LOD: [&str; 8] = ["MirrorL", "MirrorR", "RearWing", "FrontSplitter", "FrontBumper", "RearBumper", "RearDiffuser", "LightPodMesh"];

/// Level of detail of a model mesh (by its glTF mesh name): 0, 1, 2, or `None` for every
/// distance.
pub fn lod_of(mesh: &str) -> Option<u8> {
    if mesh.ends_with("_lod1") {
        Some(1)
    } else if mesh.ends_with("_lod2") {
        Some(2)
    } else if EVERY_LOD.contains(&mesh) {
        None
    } else {
        Some(0)
    }
}

/// The visibility range of a level of detail.
pub fn lod_range(level: u8) -> VisibilityRange {
    let [a, b] = LOD_RANGES;
    match level {
        0 => VisibilityRange { start_margin: 0.0..0.0, end_margin: a, use_aabb: false },
        1 => VisibilityRange { start_margin: a, end_margin: b, use_aabb: false },
        _ => VisibilityRange { start_margin: b, end_margin: 1.0e9..1.0e9, use_aabb: false },
    }
}

/// The car model's asset path.
pub const MODEL: &str = "models/rallycar.glb";

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
    /// The seated driver's body (helmet, suit, legs): hidden in this car's own cockpit view.
    driver: Option<Entity>,
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
/// The cabin's fill light.
#[derive(Component)]
pub struct CabinFill;

/// How much of the cabin fill is kept at clock hour `hours`: it stands in for daylight leaking
/// in, so it falls with the keyed exposure (`art::look::daylight_fill_scale`, ~14 stops to
/// night). Without this the cockpit glares white-cyan at dusk and night.
pub fn cabin_fill_scale(hours: f32) -> f32 {
    super::look::daylight_fill_scale(hours)
}

/// The fill follows the day (only rewritten when it moves by more than 5 %).
fn cabin_fill(clock: Res<crate::daytime::DayClock>, mut fills: Query<&mut PointLight, With<CabinFill>>) {
    let want = CABIN_FILL_LUMENS * cabin_fill_scale(clock.hours);
    for mut l in &mut fills {
        let now = l.bypass_change_detection().intensity;
        if (now - want).abs() > 0.05 * now.max(want) {
            l.intensity = want;
        }
    }
}

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
    let scene = world.resource::<AssetServer>().load(GltfAssetLabel::Scene(0).from_asset(MODEL));
    let tf = Transform::from_translation(model_offset(&spec));
    let model = world.spawn((Name::new("car model"), CarModel, WorldAssetRoot(scene), tf, Visibility::default(), Actor)).id();
    world.entity_mut(car).add_child(model);
    // A soft fill in the cabin: the roof shades it from the sun and sky, and without this the
    // interior reads as a cave from both seats. No shadows (cheap), short range.
    let fill = world
        .spawn((
            Name::new("cabin fill light"),
            CabinFill,
            PointLight { color: Color::srgb(1.0, 0.97, 0.92), intensity: CABIN_FILL_LUMENS, range: 1.6, radius: 0.3, shadow_maps_enabled: false, ..default() },
            Transform::from_translation(CABIN_FILL_MODEL),
            Actor,
        ))
        .id();
    world.entity_mut(model).add_child(fill);
    super::mirror::spawn(world, model);
    super::hands::spawn(world, car, model);
}

/// Finds the hub/spin nodes once the scene is in, tags every part as an `Actor` and gives the
/// model's textures mips.
#[allow(clippy::too_many_arguments)]
fn rig(
    mut commands: Commands,
    models: Query<Entity, (With<CarModel>, Without<CarRig>)>,
    children: Query<&Children>,
    names: Query<(&Name, &Transform)>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>>,
    mesh_names: Query<&GltfMeshName>,
    mat_names: Query<&GltfMaterialName>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut mesh_assets: ResMut<Assets<Mesh>>,
    mirror: Option<Res<super::mirror::MirrorImage>>,
    placed: Query<(&Mesh3d, &ChildOf, &Transform)>,
) {
    for m in &models {
        let mut hub = [Entity::PLACEHOLDER; 4];
        let mut knuckle = [None; 2];
        let mut spin = [Entity::PLACEHOLDER; 4];
        let mut rest = [Vec3::ZERO; 4];
        let mut dials = [None; 3];
        let mut pod = None;
        let mut driver = None;
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
            } else if n.as_str() == "DriverBody" {
                driver = Some(e);
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
        let mut glass_done = std::collections::HashSet::new();
        // This car's own copies of the materials that change while it drives (paint gets
        // dusty, the tail lamps light up under braking and with the lights, the headlamp
        // lenses glow with the lights): one copy per source material.
        let mut own: std::collections::HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>> = default();
        let mut wear = CarWear::default();
        let mut rear_mirror = None;
        for &e in &parts {
            let (Ok(mm), Ok(name)) = (meshes.get(e), mat_names.get(e)) else { continue };
            let kind = match name.0.as_str() {
                "Paint" | "Chalk" => Own::Paint,
                "TailLamp" => Own::Tail,
                "Lamp" => Own::Lens,
                "Mirror" => {
                    rear_mirror = Some(mm.0.clone());
                    continue;
                }
                _ => continue,
            };
            // The body's lamp glass covers the reversing lamps at the back too: those stay dark
            // (the back half moves to its own entity with the original material).
            if kind == Own::Lens
                && let Ok((m3, parent, tf)) = placed.get(e)
                && let Some((front, back)) = mesh_assets.get(&m3.0).and_then(split_front_back)
            {
                let (fh, bh) = (mesh_assets.add(front), mesh_assets.add(back));
                commands.entity(e).insert(Mesh3d(fh));
                let mut back = commands.spawn((Name::new("reversing lamps"), Mesh3d(bh), MeshMaterial3d(mm.0.clone()), *tf, Actor));
                if let Ok(n) = mesh_names.get(e)
                    && let Some(level) = lod_of(&n.0)
                {
                    back.insert(lod_range(level));
                }
                let back = back.id();
                commands.entity(parent.parent()).add_child(back);
            }
            let h = own
                .entry(mm.0.id())
                .or_insert_with(|| {
                    let m = mats.get(&mm.0).cloned().unwrap_or_default();
                    let h = mats.add(m.clone());
                    match kind {
                        Own::Paint => wear.paint.push((h.clone(), m.base_color, m.perceptual_roughness, m.clearcoat)),
                        Own::Tail => wear.tail.push((h.clone(), m.emissive)),
                        Own::Lens => wear.lens.push((h.clone(), m.emissive)),
                    }
                    h
                })
                .clone();
            commands.entity(e).insert(MeshMaterial3d(h));
        }
        let doors = ["MirrorL", "MirrorR"].map(|n| detached.iter().find(|(p, _)| *p == n).map(|(_, e)| *e));
        super::mirror::dress(&mut commands, &mut mats, &mut mesh_assets, rear_mirror, doors, mirror.as_deref());
        commands.entity(m).insert(wear);
        for e in parts {
            commands.entity(e).insert(Actor);
            if let Ok(n) = mesh_names.get(e)
                && let Some(level) = lod_of(&n.0)
            {
                commands.entity(e).insert(lod_range(level));
            }
            if let Ok(mm) = meshes.get(e)
                && glass_done.insert(mm.0.id())
                && let Some(mut mat) = mats.get_mut(&mm.0)
                && is_glass(&mat)
            {
                tint_glass(&mut mat);
            }
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
        commands.entity(m).insert(CarRig { hub, knuckle, spin, rest, dials, pod, driver, chassis, parts: detached });
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Own {
    Paint,
    Tail,
    Lens,
}

/// Splits a mesh into its triangles in front of the model's middle (z < 0) and behind it, or
/// `None` if it's all on one side (or not readable).
pub fn split_front_back(m: &Mesh) -> Option<(Mesh, Mesh)> {
    use bevy::mesh::{Indices, VertexAttributeValues};
    let Some(VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let idx: Vec<u32> = match m.indices()? {
        Indices::U16(v) => v.iter().map(|&i| i as u32).collect(),
        Indices::U32(v) => v.clone(),
    };
    let (mut front, mut back) = (Vec::new(), Vec::new());
    for t in idx.as_chunks::<3>().0 {
        let z: f32 = t.iter().map(|&i| p.get(i as usize).map_or(0.0, |v| v[2])).sum();
        if z < 0.0 { &mut front } else { &mut back }.extend_from_slice(t);
    }
    if front.is_empty() || back.is_empty() {
        return None;
    }
    let mut f = m.clone();
    f.insert_indices(Indices::U32(front));
    let mut b = m.clone();
    b.insert_indices(Indices::U32(back));
    Some((f, b))
}

/// The model's windows: blended and see-through (the importer gives glass a fixed alpha).
fn is_glass(m: &StandardMaterial) -> bool {
    matches!(m.alpha_mode, AlphaMode::Blend | AlphaMode::Premultiplied | AlphaMode::Add) && m.base_color.alpha() < 0.9
}

/// Clear, slightly smoked glass. The import's white at 45% read as frosted from the cockpit
/// with Bevy 0.20 (the whole outside view washed out to white).
fn tint_glass(m: &mut StandardMaterial) {
    // Single-sided: only seen from outside (the cockpit looks through its back faces), so it
    // can be as dark and glossy as real tinted glass without dimming the driver's view.
    m.base_color = Color::srgba(0.025, 0.03, 0.035, 0.42);
    m.perceptual_roughness = 0.04;
    m.reflectance = 0.6;
    m.metallic = 0.0;
    m.specular_transmission = 0.0;
    m.diffuse_transmission = 0.0;
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
        let inside = view.as_deref().is_some_and(|v| *v != CameraView::Chase);
        // The lamp pod would block the road from the bonnet and cockpit; the driver's own body
        // would fill the cockpit camera (it's inside the helmet). The co-driver (no
        // `DrivenCar` here) and the chase camera see both.
        let hide = [(rig.pod, inside), (rig.driver, driven.is_some() && view.as_deref().is_none_or(|v| *v == CameraView::Cockpit))];
        for (e, hidden) in hide {
            if let Some(e) = e
                && let Ok(mut v) = vis.get_mut(e)
            {
                let want = if hidden { Visibility::Hidden } else { Visibility::Inherited };
                if *v != want {
                    *v = want;
                }
            }
        }
    }
}

/// The car's materials that change with the run: its own copies of the paint (base colour,
/// roughness, clear coat as built), the tail lamps and the headlamp lenses (emissive as built).
#[derive(Component, Default)]
pub struct CarWear {
    /// (material, livery colour as built or as the garage paints it, roughness, clear coat).
    pub paint: Vec<(Handle<StandardMaterial>, Color, f32, f32)>,
    tail: Vec<(Handle<StandardMaterial>, LinearRgba)>,
    lens: Vec<(Handle<StandardMaterial>, LinearRgba)>,
    /// What the lamps show now: (braking, lights on).
    lamps: Option<(bool, bool)>,
    /// Dust and mud on the paint, 0 (clean) ..= 1 (caked).
    pub dirt: f32,
    shown_dirt: f32,
}

/// Tail lamps: as built, ×2.2 with the lights on, ×5 under braking. Headlamp lenses with the
/// lights on (display-relative: they bloom by day and night alike).
const TAIL_LIT: f32 = 2.2;
const TAIL_BRAKE: f32 = 5.0;
const LENS_LIT: LinearRgba = LinearRgba::rgb(7.0, 6.5, 5.6);

/// How far a fully dirty car's paint goes towards [`DUST`].
const MAX_DIRT: f32 = 0.6;
/// The colour dirt takes the paint towards (a multiplier on the livery).
const DUST: [f32; 3] = [0.62, 0.54, 0.43];

/// Dirt per metre driven with all four wheels on a surface (a full coat after ~3 km of
/// gravel, ~800 m of mud; tarmac stays clean).
pub fn dirt_rate(s: crate::stage::Surface) -> f32 {
    use crate::stage::Surface::*;
    match s {
        Tarmac => 0.0,
        Gravel | Loose => 1.0 / 3000.0,
        Verge | Rough => 1.0 / 1500.0,
        Mud => 1.0 / 800.0,
        Snow => 1.0 / 2500.0,
        _ => 1.0 / 2000.0,
    }
}

/// The colour of livery `c` under dirt `d` (0..=1).
pub fn dusty(c: Color, d: f32) -> Color {
    let d = d.clamp(0.0, 1.0) * MAX_DIRT;
    let s = c.to_srgba();
    Color::srgba(s.red * (1.0 - d + d * DUST[0]), s.green * (1.0 - d + d * DUST[1]), s.blue * (1.0 - d + d * DUST[2]), s.alpha)
}

/// Paint gets dusty as the car drives on loose surfaces (faster while the wheels slip), the
/// tail lamps light up under braking and with the lights, the headlamps glow with the lights.
fn wear(
    time: Res<Time>,
    lights: Option<Res<super::headlights::Headlights>>,
    cars: Query<(&DrivenCar, Option<&DriverInput>, &Children)>,
    mut wears: Query<&mut CarWear>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let lit = lights.is_some_and(|l| l.on);
    let dt = time.delta_secs();
    for (driven, input, kids) in &cars {
        let Some(k) = kids.iter().find(|k| wears.contains(*k)) else { continue };
        let Ok(mut w) = wears.get_mut(k) else { continue };
        let car = &driven.car;
        let v = car.speed().abs();
        let mut rate = 0.0;
        for wh in &car.wheels {
            if let (true, Some(s)) = (wh.contact, wh.surface) {
                rate += dirt_rate(s) * 0.25 * (1.0 + 2.0 * (wh.slip_ratio.abs() + wh.slip_angle.abs()).min(1.0));
            }
        }
        w.dirt = (w.dirt + rate * v * dt).min(1.0);
        // Re-upload the paint only when the dirt has visibly changed.
        if (w.dirt - w.shown_dirt).abs() > 1.0 / 48.0 {
            w.shown_dirt = w.dirt;
            let d = w.dirt;
            for (h, base, rough, coat) in &w.paint {
                if let Some(mut m) = mats.get_mut(h) {
                    m.base_color = dusty(*base, d);
                    m.perceptual_roughness = rough + (0.85 - rough) * d * MAX_DIRT;
                    m.clearcoat = coat * (1.0 - d);
                }
            }
        }
        let braking = input.is_some_and(|i| i.0.brake > 0.05) && v > 0.5;
        if w.lamps != Some((braking, lit)) {
            let (was_braking, was_lit) = w.lamps.unwrap_or((!braking, !lit));
            w.lamps = Some((braking, lit));
            if was_braking != braking || was_lit != lit {
                let k = if braking { TAIL_BRAKE } else if lit { TAIL_LIT } else { 1.0 };
                for (h, e) in &w.tail {
                    if let Some(mut m) = mats.get_mut(h) {
                        m.emissive = *e * k;
                    }
                }
            }
            if was_lit != lit {
                for (h, e) in &w.lens {
                    if let Some(mut m) = mats.get_mut(h) {
                        m.emissive = if lit { LENS_LIT } else { *e };
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cabin_fill_fades_with_the_day() {
        assert_eq!(cabin_fill_scale(12.0), 1.0);
        assert!(cabin_fill_scale(23.0) < 3e-4);
        assert!(cabin_fill_scale(19.0) > cabin_fill_scale(20.5));
    }

    #[test]
    fn lamp_glass_splits_front_from_back() {
        use bevy::asset::RenderAssetUsages;
        use bevy::mesh::{Indices, PrimitiveTopology};
        let tri = |z: f32| [[0.0, 0.0, z], [1.0, 0.0, z], [0.0, 1.0, z]];
        let pos: Vec<[f32; 3]> = tri(-2.0).into_iter().chain(tri(2.0)).collect();
        let m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()).with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos).with_inserted_indices(Indices::U16(vec![0, 1, 2, 3, 4, 5]));
        let (f, b) = split_front_back(&m).unwrap();
        assert_eq!((f.indices().unwrap().len(), b.indices().unwrap().len()), (3, 3));
        assert_eq!(f.indices().unwrap().iter().next(), Some(0));
        let front_only = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()).with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, tri(-2.0).to_vec()).with_inserted_indices(Indices::U16(vec![0, 1, 2]));
        assert!(split_front_back(&front_only).is_none());
    }

    #[test]
    fn dirt_builds_on_loose_surfaces_only() {
        use crate::stage::Surface;
        assert_eq!(dirt_rate(Surface::Tarmac), 0.0);
        assert!(dirt_rate(Surface::Mud) > dirt_rate(Surface::Gravel));
        let c = Color::srgb(0.2, 0.5, 0.8);
        assert_eq!(dusty(c, 0.0), c);
        let d = dusty(c, 1.0).to_srgba();
        // Darker and browner (blue drops most).
        assert!(d.blue < 0.8 * 0.7 && d.red < 0.2 && d.red / d.blue > 0.2 / 0.8);
    }

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
        // Driver's eye: left seat, ~1.1 m over the ground, behind the front axle.
        let eye = cockpit_eye(&s);
        assert!(eye.x < -0.2 && (1.0..1.25).contains(&(eye.y - ground)) && eye.z > s.mounts[0].z, "{eye}");
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
        for n in ["Body", "SteeringWheel", "Shifter", "Handbrake", "DriverBody", "SeatHarness"].into_iter().chain(DETACHABLE) {
            assert_eq!(parent(n).as_deref(), Some("Chassis"), "{n}");
        }
        for n in ["CoDriverEye", "CoDriverTablet"] {
            assert_eq!(parent(n).as_deref(), Some("Car"), "{n}");
        }
        // Lower-detail twins sit beside their LOD0 mesh.
        for lv in [1, 2] {
            assert_eq!(parent(&format!("Body_lod{lv}")).as_deref(), Some("Chassis"));
            for k in NAMES {
                assert_eq!(parent(&format!("Wheel{k}_lod{lv}")), Some(format!("Spin{k}")));
            }
        }
    }

    #[test]
    fn levels_of_detail_hand_over() {
        assert_eq!(lod_of("Body"), Some(0));
        assert_eq!(lod_of("Interior"), Some(0));
        assert_eq!(lod_of("WheelFL_lod1"), Some(1));
        assert_eq!(lod_of("Body_lod2"), Some(2));
        assert_eq!(lod_of("FrontBumper"), None);
        // Each level fades in over the band the one before fades out.
        for lv in 0..2 {
            assert_eq!(lod_range(lv).end_margin, lod_range(lv + 1).start_margin);
        }
        assert!(LOD_RANGES[0].end <= LOD_RANGES[1].start);
        for p in DETACHABLE {
            assert!(EVERY_LOD.contains(&p), "{p}: drawn at every distance");
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
