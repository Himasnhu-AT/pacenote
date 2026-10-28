//! Wheel spray and weather particles (`engine::vfx`): dust, mud, snow powder and gravel chips
//! thrown up by each wheel according to its surface, speed and slip (from the simulation's
//! wheel data on [`DrivenCar`]); rain around the camera on wet stages.
//!
//! Effects are persistent `ParticleEffect`s (one per wheel and kind) moved to the contact
//! patches every frame; `rate_scale` follows speed and slip, so nothing spawns at rest. The
//! engine's near-camera fade and size cap keep the chase camera from whiting out.

use bevy::prelude::*;
use engine::vfx::{Blend, Curve, EffectDef, EmitterDef, Flipbook, Gradient, MeshShape, ParticleEffect, RenderDef, RenderMode, Shape, Velocity, VfxLibrary, VfxTexture};

use crate::drive::{CarSpec, DrivenCar};
use crate::stage::{StageDef, Surface, Weather};

pub struct SprayPlugin;

impl Plugin for SprayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register).add_systems(PostUpdate, follow_wheels.before(engine::vfx::VfxSet::Prepare));
    }
}

/// One wheel emitter: which wheel, which kind.
#[derive(Component, Clone, Copy, Debug)]
pub struct WheelSpray {
    pub wheel: usize,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Gravel / dirt dust (colour per biome is in the effect).
    Dust,
    /// Flying stones (lit mesh chips).
    Chips,
    Mud,
    Snow,
    /// Faint tyre smoke on tarmac when sliding hard.
    Smoke,
}

impl Kind {
    fn effect(self) -> &'static str {
        match self {
            Kind::Dust => "pn_dust",
            Kind::Chips => "pn_chips",
            Kind::Mud => "pn_mud",
            Kind::Snow => "pn_snow",
            Kind::Smoke => "pn_smoke",
        }
    }
}

fn puff(name: &str, color: [f32; 3], alpha: f32, size: (f32, f32), grow: f32, life: (f32, f32), cap: u32) -> EffectDef {
    EffectDef { cull_distance: 160.0, ..EffectDef::new(name) }.with(EmitterDef {
        name: "puff".into(),
        capacity: cap,
        rate: 28.0,
        lifetime: life,
        shape: Shape::Sphere { radius: 0.15, surface: false },
        velocity: Velocity::Cone { dir: Vec3::new(0.0, 1.0, 0.0), angle: 1.0, speed: (0.6, 2.0) },
        gravity: Vec3::new(0.0, 0.25, 0.0),
        drag: 1.6,
        size: size,
        size_over_life: Curve::linear(0.7, grow),
        color_over_life: Gradient::srgb(&[(0.0, color, 0.0), (0.08, color, alpha), (1.0, color, 0.0)]),
        rotation: (0.0, std::f32::consts::TAU),
        spin: (-0.5, 0.5),
        render: RenderDef { texture: VfxTexture::Smoke, blend: Blend::Alpha, lit: 0.9, soft: 0.6, flipbook: Some(Flipbook { random_start: true, ..default() }), sort: true, ..default() },
        ..default()
    })
}

/// The stage's effects (colours follow the biome's soil).
pub fn defs(def: &StageDef) -> Vec<EffectDef> {
    let dust = match def.biome {
        crate::stage::Biome::Scrub => [0.66, 0.5, 0.36],
        crate::stage::Biome::SnowForest => [0.8, 0.8, 0.8],
        _ => [0.6, 0.53, 0.43],
    };
    let wet = def.weather == Weather::Rain;
    let dust_alpha = if wet { 0.12 } else { 0.32 };
    vec![
        puff("pn_dust", dust, dust_alpha, (0.6, 1.0), 4.5, (1.6, 2.8), 160),
        puff("pn_snow", [0.93, 0.95, 1.0], 0.45, (0.4, 0.8), 3.5, (0.8, 1.6), 120),
        puff("pn_smoke", [0.75, 0.75, 0.76], 0.22, (0.5, 0.8), 4.0, (1.2, 2.0), 80),
        EffectDef { cull_distance: 120.0, ..EffectDef::new("pn_chips") }.with(EmitterDef {
            name: "chips".into(),
            capacity: 120,
            rate: 60.0,
            lifetime: (0.5, 0.9),
            shape: Shape::Sphere { radius: 0.1, surface: false },
            velocity: Velocity::Cone { dir: Vec3::new(0.0, 0.7, 0.0), angle: 0.9, speed: (2.0, 5.0) },
            gravity: Vec3::new(0.0, -9.81, 0.0),
            size: (0.025, 0.05),
            color_over_life: Gradient::srgb(&[(0.0, [0.42, 0.37, 0.32], 1.0)]),
            spin: (-12.0, 12.0),
            render: RenderDef { mode: RenderMode::Mesh { shape: MeshShape::Shard }, ..default() },
            ..default()
        }),
        EffectDef { cull_distance: 120.0, ..EffectDef::new("pn_mud") }.with(EmitterDef {
            name: "clods".into(),
            capacity: 160,
            rate: 70.0,
            lifetime: (0.5, 0.9),
            shape: Shape::Sphere { radius: 0.12, surface: false },
            velocity: Velocity::Cone { dir: Vec3::new(0.0, 0.8, 0.0), angle: 0.8, speed: (2.0, 5.5) },
            gravity: Vec3::new(0.0, -9.81, 0.0),
            size: (0.05, 0.11),
            color_over_life: Gradient::srgb(&[(0.0, [0.2, 0.15, 0.1], 1.0)]),
            render: RenderDef { mode: RenderMode::Stretched { factor: 0.02 }, texture: VfxTexture::Dot, blend: Blend::Alpha, lit: 1.0, ..default() },
            ..default()
        }),
    ]
}

fn register(mut lib: ResMut<VfxLibrary>) {
    // Rain around the camera: the engine preset collides with y = 0 (our terrain isn't there),
    // so this one just falls through its 26 m column.
    let rain = EffectDef { follow_camera: true, cull_distance: f32::MAX, ..EffectDef::new("pn_rain") }.with(EmitterDef {
        name: "drops".into(),
        capacity: 4000,
        rate: 2600.0,
        lifetime: (1.4, 1.6),
        shape: Shape::Box { half: Vec3::new(16.0, 0.5, 16.0) },
        offset: Vec3::new(0.0, 12.0, 0.0),
        velocity: Velocity::Fixed { v: Vec3::new(0.8, -16.0, 0.3), jitter: Vec3::new(0.2, 1.0, 0.2) },
        size: (0.012, 0.018),
        color_over_life: Gradient::srgb(&[(0.0, [0.72, 0.78, 0.86], 0.4)]),
        render: RenderDef { mode: RenderMode::Stretched { factor: 0.03 }, texture: VfxTexture::Spark, blend: Blend::Mix(0.4), intensity: 1.2, ..default() },
        ..default()
    });
    if let Err(e) = lib.register(rain) {
        error!("rain effect: {e}");
    }
}

/// Registers the stage's wheel effects and spawns the emitters (+ rain on wet stages).
pub fn setup(world: &mut World, def: &StageDef) {
    if let Some(mut lib) = world.get_resource_mut::<VfxLibrary>() {
        for d in defs(def) {
            if let Err(e) = lib.register(d) {
                error!("spray effect: {e}");
            }
        }
    } else {
        return;
    }
    let kinds = [Kind::Dust, Kind::Chips, Kind::Mud, Kind::Snow, Kind::Smoke];
    for wheel in 0..4 {
        for kind in kinds {
            let mut fx = ParticleEffect::new(kind.effect());
            fx.rate_scale = 0.0;
            world.spawn((Name::new("wheel spray"), WheelSpray { wheel, kind }, fx, Transform::default()));
        }
    }
    if def.weather == Weather::Rain {
        world.spawn((Name::new("rain"), ParticleEffect::new("pn_rain"), Transform::default()));
    }
}

/// How hard a wheel throws up a kind of spray: 0..1.
pub fn intensity(kind: Kind, surface: Option<Surface>, speed: f32, slip_ratio: f32, slip_angle: f32, rear: bool) -> f32 {
    let Some(s) = surface else { return 0.0 };
    let v = (speed / 22.0).clamp(0.0, 1.0);
    // Nothing at a standstill (the tyres carry a little slip even when held).
    let moving = ((speed - 0.8) / 3.0).clamp(0.0, 1.0);
    let slip = (slip_ratio.abs() * 2.5 + slip_angle.abs() * 3.0).clamp(0.0, 1.0) * moving;
    let roll = v * if rear { 0.7 } else { 0.35 };
    let loose = (roll + slip * (0.4 + 0.6 * v)).clamp(0.0, 1.0);
    match (kind, s) {
        (Kind::Dust, Surface::Gravel) => loose,
        (Kind::Dust, Surface::Verge | Surface::Rough) => loose * 0.5,
        (Kind::Chips, Surface::Gravel) => (slip * v * 1.4).clamp(0.0, 1.0),
        (Kind::Mud, Surface::Mud) => loose,
        (Kind::Mud, Surface::Verge | Surface::Rough) => (slip * v * 0.5).clamp(0.0, 1.0),
        (Kind::Snow, Surface::Snow) => loose,
        (Kind::Smoke, Surface::Tarmac) => ((slip - 0.5) * 2.0).clamp(0.0, 1.0) * v,
        _ => 0.0,
    }
}

fn follow_wheels(cars: Query<&DrivenCar>, spec: Option<Res<CarSpec>>, mut fx: Query<(&WheelSpray, &mut ParticleEffect, &mut Transform)>) {
    let (Ok(c), Some(spec)) = (cars.single(), spec) else { return };
    let car = &c.car;
    let speed = car.vel.length();
    let back = -(car.rot * Vec3::NEG_Z);
    for (w, mut e, mut tf) in &mut fx {
        let wh = &car.wheels[w.wheel];
        let k = if wh.contact { intensity(w.kind, wh.surface, speed, wh.slip_ratio, wh.slip_angle, w.wheel >= 2) } else { 0.0 };
        if (e.rate_scale - k).abs() > 0.02 || (k == 0.0 && e.rate_scale != 0.0) {
            e.rate_scale = k;
        }
        // At the contact patch, a little behind the wheel.
        let p = car.wheel_pos(&spec.0, w.wheel) - car.up() * spec.0.wheel_radius * 0.8 + back * 0.35;
        tf.translation = p;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spray_needs_speed_or_slip_and_the_right_surface() {
        assert_eq!(intensity(Kind::Dust, Some(Surface::Gravel), 0.0, 0.0, 0.0, true), 0.0);
        assert_eq!(intensity(Kind::Dust, Some(Surface::Gravel), 0.2, 0.3, 0.1, true), 0.0);
        assert!(intensity(Kind::Dust, Some(Surface::Gravel), 25.0, 0.0, 0.0, true) > 0.5);
        assert!(intensity(Kind::Dust, Some(Surface::Gravel), 25.0, 0.0, 0.0, false) < intensity(Kind::Dust, Some(Surface::Gravel), 25.0, 0.0, 0.0, true));
        assert_eq!(intensity(Kind::Dust, Some(Surface::Tarmac), 25.0, 0.3, 0.2, true), 0.0);
        assert_eq!(intensity(Kind::Smoke, Some(Surface::Tarmac), 25.0, 0.0, 0.0, true), 0.0);
        assert!(intensity(Kind::Smoke, Some(Surface::Tarmac), 25.0, 0.5, 0.3, true) > 0.3);
        assert!(intensity(Kind::Snow, Some(Surface::Snow), 20.0, 0.1, 0.0, true) > 0.3);
        assert_eq!(intensity(Kind::Mud, None, 20.0, 0.5, 0.5, true), 0.0);
    }

    #[test]
    fn effects_compile() {
        let mut lib = VfxLibrary::default();
        for def in crate::stage::stages() {
            for d in defs(&def) {
                lib.register(d).unwrap();
            }
        }
    }
}
