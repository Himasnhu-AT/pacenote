//! Exhaust flames, flash light, sparks and smoke, driven by `audio::pops::ExhaustPop` (one
//! message for the bang and the flame).
//!
//! **Owner: stream D (CAR FEEL).** Design: `docs/festival/ART_BIBLE.md` §3.1 (look, tip position) and
//! `docs/festival/AUDIO_PASS.md` §2 (the scheduler), `docs/festival/DECISIONS.md` D1.
//!
//! Each car that pops gets one pooled rig at its tail pipe: two nested additive cones (a
//! blue-white core in an orange flame), shared meshes and materials, `NotShadowCaster`; a point
//! light for the flash (hidden when idle); bangs add a spark burst and a grey-blue puff
//! (`engine::vfx`, pooled one-shots); a misfire is black smoke and no flame. The flame's
//! emissive ignores the camera exposure, so it blooms the same at noon and at night. From the
//! cockpit only the light flash shows (it lights the road behind in the mirrors).

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use engine::render_budget::Actor;
use engine::vfx::{Blend, Burst, Curve, EffectDef, EmitterDef, Gradient, RenderDef, RenderMode, Shape, Velocity, Vfx, VfxLibrary, VfxTexture};

use crate::audio::pops::{ExhaustPop, PopKind, PopSet};
use crate::drive::{CameraView, CarSpec, DrivenCar};

/// The tail pipe's mouth in the model's frame (right rear, under the bumper, 0.27 m above the
/// ground; the model origin is at hub height), its axis rearward (+Z).
pub const TIP_MODEL: Vec3 = Vec3::new(0.46, 0.27 - crate::art::car::MODEL_WHEEL_RADIUS, 1.99);

/// The tail pipe's mouth in the car body's frame.
pub fn tip(s: &crate::vehicle::Spec) -> Vec3 {
    TIP_MODEL + crate::art::car::model_offset(s)
}

pub struct ExhaustPlugin;

impl Plugin for ExhaustPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, (attach, fire, animate).chain().after(PopSet));
    }
}

/// The shared meshes and materials (one of each, every car).
#[derive(Resource, Clone)]
struct FlameAssets {
    cone: Handle<Mesh>,
    core: Handle<StandardMaterial>,
    outer: Handle<StandardMaterial>,
}

/// The rig at a car's tail pipe.
#[derive(Component, Clone, Copy, Debug)]
pub struct ExhaustRig {
    core: Entity,
    outer: Entity,
    light: Entity,
    /// The flame playing: seconds since it lit, its length, kind and size.
    t: f32,
    dur: f32,
    kind: PopKind,
    intensity: f32,
    len: f32,
    roll: f32,
    on: bool,
}

/// On the car: its rig.
#[derive(Component, Clone, Copy, Debug)]
pub struct HasExhaust(pub Entity);

/// Core (blue-white) and outer (orange) colours, and their brightness against scene white
/// (ART_BIBLE §3.1: 8× and 3×).
const CORE: [f32; 3] = [0.55, 0.7, 1.0];
const OUTER: [f32; 3] = [1.0, 0.45, 0.12];
const FLASH: [f32; 3] = [1.0, 0.55, 0.25];

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>, lib: Option<ResMut<VfxLibrary>>) {
    let flame = |c: [f32; 3], k: f32| StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::rgb(c[0] * k, c[1] * k, c[2] * k),
        emissive_exposure_weight: 0.0,
        unlit: false,
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        alpha_mode: AlphaMode::Add,
        double_sided: true,
        cull_mode: None,
        fog_enabled: false,
        ..default()
    };
    commands.insert_resource(FlameAssets { cone: meshes.add(flame_mesh()), core: mats.add(flame(CORE, 8.0)), outer: mats.add(flame(OUTER, 3.0)) });
    if let Some(mut lib) = lib {
        for def in effects() {
            if let Err(e) = lib.register(def) {
                warn!("exhaust: {e}");
            }
        }
    }
}

/// The flame: a cone of unit length along +Z and unit radius, bulging a third of the way out,
/// with a ragged edge (8 sides, alternate ones pulled in) so a random roll changes its shape.
/// 24 triangles.
pub fn flame_mesh() -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    const SIDES: usize = 8;
    let mut pos = Vec::new();
    let mut nor = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let ring = |z: f32, r: f32, ragged: f32| {
        (0..SIDES)
            .map(move |i| {
                let a = i as f32 / SIDES as f32 * std::f32::consts::TAU;
                let rr = r * if i % 2 == 0 { 1.0 } else { 1.0 - ragged };
                Vec3::new(a.cos() * rr, a.sin() * rr, z)
            })
            .collect::<Vec<_>>()
    };
    let r0 = ring(0.0, 0.55, 0.0);
    let r1 = ring(0.35, 1.0, 0.3);
    for p in r0.iter().chain(&r1) {
        pos.push(*p);
        nor.push(Vec3::new(p.x, p.y, 0.3).normalize_or(Vec3::Z));
    }
    pos.push(Vec3::Z);
    nor.push(Vec3::Z);
    let apex = (SIDES * 2) as u32;
    for i in 0..SIDES as u32 {
        let j = (i + 1) % SIDES as u32;
        let (a, b, c, d) = (i, j, SIDES as u32 + i, SIDES as u32 + j);
        idx.extend_from_slice(&[a, c, b, b, c, d, c, apex, d]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
        .with_inserted_indices(Indices::U32(idx))
}

/// Sparks (bangs), a grey-blue puff (bangs) and black smoke (misfires): pooled one-shots.
fn effects() -> Vec<EffectDef> {
    let sparks = EffectDef { cull_distance: 90.0, ..EffectDef::new("pn_exhaust_sparks") }.once(0.05).with(EmitterDef {
        name: "sparks".into(),
        capacity: 10,
        bursts: vec![Burst::at(0.0, 8)],
        lifetime: (0.1, 0.18),
        shape: Shape::Sphere { radius: 0.04, surface: false },
        velocity: Velocity::Cone { dir: Vec3::new(0.0, 0.25, 1.0).normalize(), angle: 0.5, speed: (3.0, 6.0) },
        gravity: Vec3::new(0.0, -9.81, 0.0),
        size: (0.012, 0.02),
        color_over_life: Gradient::srgb(&[(0.0, [1.0, 0.85, 0.5], 1.0), (1.0, [1.0, 0.4, 0.1], 0.0)]),
        render: RenderDef { mode: RenderMode::Stretched { factor: 0.03 }, texture: VfxTexture::Spark, blend: Blend::Additive, intensity: 6.0, ..default() },
        ..default()
    });
    let puff = |name: &str, c: [f32; 3], alpha: f32| {
        EffectDef { cull_distance: 90.0, ..EffectDef::new(name) }.once(0.05).with(EmitterDef {
            name: "puff".into(),
            capacity: 3,
            bursts: vec![Burst::at(0.0, 2)],
            lifetime: (0.5, 0.7),
            shape: Shape::Sphere { radius: 0.05, surface: false },
            velocity: Velocity::Cone { dir: Vec3::new(0.0, 0.3, 1.0).normalize(), angle: 0.4, speed: (0.6, 1.4) },
            drag: 2.0,
            size: (0.1, 0.14),
            size_over_life: Curve::linear(1.0, 4.5),
            color_over_life: Gradient::srgb(&[(0.0, c, 0.0), (0.1, c, alpha), (1.0, c, 0.0)]),
            rotation: (0.0, std::f32::consts::TAU),
            render: RenderDef { texture: VfxTexture::Smoke, blend: Blend::Alpha, lit: 0.8, soft: 0.5, ..default() },
            ..default()
        })
    };
    vec![sparks, puff("pn_exhaust_puff", [0.62, 0.66, 0.74], 0.25), puff("pn_exhaust_misfire", [0.08, 0.08, 0.08], 0.55)]
}

/// Every car that can pop gets its rig (the player's car; the guide's replicated one).
#[allow(clippy::type_complexity)]
fn attach(
    mut commands: Commands,
    assets: Option<Res<FlameAssets>>,
    spec: Option<Res<CarSpec>>,
    cars: Query<Entity, (Or<(With<DrivenCar>, With<crate::guide::sync::NetCar>)>, Without<HasExhaust>)>,
) {
    let Some(a) = assets else { return };
    let spec = spec.map(|s| s.0.clone()).unwrap_or_default();
    for car in &cars {
        let cone = |m: &Handle<StandardMaterial>| (Mesh3d(a.cone.clone()), MeshMaterial3d(m.clone()), Transform::from_scale(Vec3::ZERO), Visibility::Hidden, NotShadowCaster, Actor);
        let core = commands.spawn((Name::new("exhaust flame core"), cone(&a.core))).id();
        let outer = commands.spawn((Name::new("exhaust flame"), cone(&a.outer))).id();
        let light = commands
            .spawn((
                Name::new("exhaust flash"),
                PointLight { color: Color::srgb(FLASH[0], FLASH[1], FLASH[2]), intensity: 0.0, range: 4.0, radius: 0.05, shadow_maps_enabled: false, ..default() },
                Transform::from_xyz(0.0, 0.05, 0.25),
                Visibility::Hidden,
                Actor,
            ))
            .id();
        let rig = ExhaustRig { core, outer, light, t: 0.0, dur: 0.0, kind: PopKind::Crackle, intensity: 0.0, len: 0.0, roll: 0.0, on: false };
        let r = commands.spawn((Name::new("exhaust"), rig, Transform::from_translation(tip(&spec)), Visibility::default(), Actor)).add_children(&[core, outer, light]).id();
        commands.entity(car).add_child(r).insert(HasExhaust(r));
    }
}

/// Small deterministic hash → 0..1 (flame shape from the pop's seed).
fn h01(seed: u32, k: u32) -> f32 {
    let mut x = seed ^ k.wrapping_mul(0x9E37_79B9);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x >> 8) as f32 / 16_777_216.0
}

/// A pop lights its car's flame (and on a bang throws sparks and a puff).
fn fire(mut pops: MessageReader<ExhaustPop>, cars: Query<(&HasExhaust, &GlobalTransform)>, mut rigs: Query<&mut ExhaustRig>, mut vfx: Vfx, spec: Option<Res<CarSpec>>) {
    let spec = spec.map(|s| s.0.clone()).unwrap_or_default();
    for p in pops.read() {
        let Ok((has, tf)) = cars.get(p.car) else { continue };
        let Ok(mut rig) = rigs.get_mut(has.0) else { continue };
        let i = p.intensity.clamp(0.0, 1.0);
        let (len, dur) = match p.kind {
            PopKind::Crackle => (0.25, 0.04 + 0.03 * h01(p.seed, 1)),
            PopKind::Bang => (0.45, 0.09 + 0.03 * h01(p.seed, 1)),
            PopKind::Misfire => (0.0, 0.0),
        };
        if p.kind != PopKind::Misfire {
            *rig = ExhaustRig {
                t: 0.0,
                dur,
                kind: p.kind,
                intensity: i,
                // ±15 % length per shot, bigger with the intensity.
                len: len * (0.85 + 0.3 * h01(p.seed, 2)) * (0.7 + 0.3 * i),
                roll: h01(p.seed, 3) * std::f32::consts::TAU,
                on: true,
                ..*rig
            };
        }
        let at = Transform::from_translation(tf.transform_point(tip(&spec))).with_rotation(tf.to_scale_rotation_translation().1);
        match p.kind {
            PopKind::Bang => {
                vfx.play("pn_exhaust_sparks", at);
                vfx.play("pn_exhaust_puff", at);
            }
            PopKind::Misfire => {
                vfx.play("pn_exhaust_misfire", at);
            }
            PopKind::Crackle => {}
        }
    }
}

/// The flame's envelope: scales in over 15 ms, out over the rest.
pub fn envelope(t: f32, dur: f32) -> f32 {
    if dur <= 0.0 || t >= dur {
        return 0.0;
    }
    let attack = 0.015f32.min(dur * 0.5);
    if t < attack { t / attack } else { 1.0 - (t - attack) / (dur - attack) }
}

fn animate(
    time: Res<Time>,
    view: Option<Res<CameraView>>,
    mut rigs: Query<(&ChildOf, &mut ExhaustRig)>,
    driven: Query<(), With<DrivenCar>>,
    mut parts: Query<(&mut Transform, &mut Visibility), Without<ExhaustRig>>,
    mut lights: Query<&mut PointLight>,
) {
    let dt = time.delta_secs();
    let cockpit = view.as_deref().is_some_and(|v| *v == CameraView::Cockpit);
    for (parent, mut rig) in &mut rigs {
        if !rig.on {
            continue;
        }
        rig.t += dt;
        let e = envelope(rig.t, rig.dur);
        let done = rig.t >= rig.dur;
        // From the cockpit, the player's own flame is behind the car: only its light shows.
        let hide_flame = done || (cockpit && driven.contains(parent.parent()));
        let bang = rig.kind == PopKind::Bang;
        let (r_out, r_core) = if bang { (0.13, 0.05) } else { (0.09, 0.035) };
        let core_len = (0.12 + 0.06 * rig.intensity) * if bang { 1.0 } else { 0.8 };
        for (part, len, r) in [(rig.outer, rig.len, r_out), (rig.core, core_len, r_core)] {
            if let Ok((mut tf, mut vis)) = parts.get_mut(part) {
                let want = if hide_flame { Visibility::Hidden } else { Visibility::Inherited };
                if *vis != want {
                    *vis = want;
                }
                let w = e.max(0.0);
                tf.scale = Vec3::new(r * (0.6 + 0.4 * w), r * (0.6 + 0.4 * w), (len * w).max(1e-4));
                tf.rotation = Quat::from_rotation_z(rig.roll);
            }
        }
        if let Ok((_, mut vis)) = parts.get_mut(rig.light) {
            let want = if done { Visibility::Hidden } else { Visibility::Inherited };
            if *vis != want {
                *vis = want;
            }
        }
        if let Ok(mut l) = lights.get_mut(rig.light) {
            let lumens = if bang { 9000.0 } else { 4000.0 };
            l.intensity = lumens * e.max(0.0) * (0.6 + 0.4 * rig.intensity);
        }
        if done {
            rig.on = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flame_mesh_is_small_and_points_back() {
        let m = flame_mesh();
        let idx = m.indices().unwrap().len();
        assert_eq!(idx / 3, 24, "24 triangles");
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        assert!(p.iter().all(|v| (0.0..=1.0).contains(&v[2])), "along +Z (rearward)");
    }

    #[test]
    fn flame_envelope_snaps_in_and_fades() {
        assert_eq!(envelope(0.0, 0.05), 0.0);
        assert!((envelope(0.015, 0.05) - 1.0).abs() < 1e-5);
        assert!(envelope(0.04, 0.05) < 0.5);
        assert_eq!(envelope(0.06, 0.05), 0.0);
        assert_eq!(envelope(0.01, 0.0), 0.0);
    }

    #[test]
    fn the_tip_is_behind_the_right_rear_wheel() {
        let s = crate::vehicle::Spec::default();
        let t = tip(&s);
        assert!(t.x > 0.3 && t.z > s.mounts[2].z + 0.3, "{t}");
        // About 0.27 m above the ground at rest.
        let ground = crate::art::car::model_offset(&s).y - crate::art::car::MODEL_WHEEL_RADIUS;
        assert!((t.y - ground - 0.27).abs() < 0.02, "{t} over {ground}");
    }
}
