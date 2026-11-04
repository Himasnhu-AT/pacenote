//! The sky: a camera-following cloud shell (`art/clouds.wesl`, ray-marched 1.8-3.2 km,
//! 4/8 steps, stars at night), the moon (the sun light re-aimed, `art::look`) and street-lamp
//! lights.
//!
//! **Owner: stream C (LOOK).** Design: `docs/festival/ART_BIBLE.md` §2 with `docs/festival/TECH_PASS.md` §1/§4
//! (shell mesh, not a render-graph pass; never switch `SkyMode` in play) and
//! `docs/festival/DECISIONS.md` L2-L4, L7.
//!
//! **Clouds.** One mesh (a sphere of radius [`DOME_R`] centred on the camera in the vertex
//! shader), unlit, premultiplied alpha, depth-tested and not written: only sky pixels shade, in
//! the transparent pass after Bevy's atmosphere. The fragment shader intersects the 1.8-3.2 km
//! shell of a curved earth and marches it (Low: one sample of a flat layer; Medium 4 steps,
//! 1 light tap; High+ 8 steps, 2 taps). Density = a macro coverage map × a Perlin-Worley shape
//! (2D, 256², tiling) with a height profile, eroded by a 3D Worley detail volume (32³). Light:
//! Beer with a multiple-scattering floor, dual Henyey-Greenstein phase (silver lining towards
//! the sun), sky fill from above and ground bounce from below; far clouds melt into the haze.
//! Both noise textures are generated at startup (≈ 0.3 MB). Wind drifts the layer; coverage
//! follows the time-of-day keys plus a slow wander. Stars: the same shader adds a rotating star
//! field where the clouds let it through once the sun is 6° under the horizon.

use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;
use engine::flow::AppState;
use engine::gfx::MainCamera;
use engine::look::Look;
#[cfg(feature = "dev")]
use engine::devtools::AddControlCommand;
use engine::render_budget::Actor;

use super::look::{self, MOON_LUX};
use crate::daytime::DayClock;
use crate::open::furniture::SignKind;
use crate::world::StageWorld;

/// Radius of the cloud mesh around the camera (m): beyond any terrain, so the depth test keeps
/// clouds to the sky.
pub const DOME_R: f32 = 40_000.0;
/// Cloud layer altitude (m, world Y).
pub const CLOUD_BASE: f32 = 1800.0;
pub const CLOUD_TOP: f32 = 3200.0;
/// Wind at cloud height (m/s, world XZ): from the west-south-west.
const WIND: Vec2 = Vec2::new(11.0, -3.5);

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "clouds.wesl");
        app.add_plugins(MaterialPlugin::<CloudMaterial>::default())
            .init_resource::<CloudState>()
            .init_resource::<Lamps>()
            .add_systems(Update, (spawn_clouds, update_clouds).chain().after(engine::gfx::QualitySet::Look))
            .add_systems(Update, (spawn_lamps, update_lamps).chain())
            .add_systems(OnExit(AppState::InGame), |mut l: ResMut<Lamps>| *l = Lamps::default());
        look::register(app);
        #[cfg(feature = "dev")]
        app.add_control_command(
            "sky.clouds",
            "Show or hide the cloud layer (A/B cost); no params: state",
            engine::devtools::params(&[("on", "boolean", "draw the clouds")]),
            |w, p| {
                let on = p.get("on").and_then(serde_json::Value::as_bool);
                let mut q = w.query_filtered::<&mut Visibility, With<CloudDome>>();
                let mut n = 0;
                for mut v in q.iter_mut(w) {
                    if let Some(on) = on {
                        *v = if on { Visibility::Inherited } else { Visibility::Hidden };
                    }
                    n += 1;
                }
                let writes = w.get_resource::<look::DayLookState>().map_or(0, |s| s.writes);
                Ok(serde_json::json!({"domes": n, "on": on, "look_writes": writes}))
            },
        );
    }
}

// ---------------------------------------------------------------- cloud material

/// Shader parameters (`clouds.wesl` `Clouds`). Colours are linear radiance in cd/m² (the
/// shader multiplies by the view's exposure, as Bevy's sky does).
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct CloudUniform {
    /// xyz: towards the light (sun or moon); w: 1 when it is the moon.
    pub sun_dir: Vec4,
    /// rgb: illuminance of the light on the clouds (colour × lux); w: daylight 0..1.
    pub sun_rad: Vec4,
    /// rgb: sky radiance lighting the clouds from above / from the ground below.
    pub amb_top: Vec4,
    pub amb_bot: Vec4,
    /// rgb: horizon haze radiance; w: distance (m) at which clouds are half haze.
    pub haze: Vec4,
    /// coverage 0..1, extinction (1/m) at full density, base and top altitude (m).
    pub shape: Vec4,
    /// xy: wind offset (m), z: detail offset (m), w: march steps (1 = flat layer).
    pub wind: Vec4,
    /// x: star radiance, y: sky rotation (rad), z: time (s), w: light taps.
    pub stars: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct CloudMaterial {
    #[uniform(100)]
    pub p: CloudUniform,
    #[texture(101)]
    #[sampler(102)]
    pub weather: Handle<Image>,
    #[texture(103, dimension = "3d")]
    #[sampler(104)]
    pub detail: Handle<Image>,
}

impl Material for CloudMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://pacenote/art/clouds.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://pacenote/art/clouds.wesl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(_: &MaterialPipeline, descriptor: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, _: MaterialPipelineKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        // The camera is inside the dome.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// The cloud dome entity.
#[derive(Component)]
pub struct CloudDome;

/// Cloud bookkeeping: the material, drift, the tier's step counts.
#[derive(Resource, Default)]
pub struct CloudState {
    pub material: Option<Handle<CloudMaterial>>,
    textures: Option<(Handle<Image>, Handle<Image>)>,
    /// Accumulated wind drift (m).
    pub drift: Vec2,
}

/// A unit ico-sphere with positions only (the vertex shader scales and centres it).
fn dome_mesh() -> Mesh {
    let s = Sphere::new(1.0).mesh().ico(3).expect("ico sphere");
    let pos = s.attribute(Mesh::ATTRIBUTE_POSITION).cloned().expect("positions");
    let idx: Vec<u32> = s.indices().expect("indices").iter().map(|i| i as u32).collect();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_indices(Indices::U32(idx))
}

/// Spawns the dome once per stage (an atmosphere sky only: grey weather keeps its gradient).
fn spawn_clouds(
    mut commands: Commands,
    sw: Option<Res<StageWorld>>,
    look: Option<Res<Look>>,
    domes: Query<(), With<CloudDome>>,
    mut st: ResMut<CloudState>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<CloudMaterial>>,
) {
    let (Some(_), Some(look)) = (sw, look) else { return };
    if !domes.is_empty() || look.sky.mode != engine::look::SkyMode::Atmosphere {
        return;
    }
    let (weather, detail) = st.textures.get_or_insert_with(|| (images.add(weather_image()), images.add(detail_image()))).clone();
    let mat = mats.add(CloudMaterial { p: CloudUniform::default(), weather, detail });
    st.material = Some(mat.clone());
    commands.spawn((
        Name::new("Clouds"),
        CloudDome,
        Mesh3d(meshes.add(dome_mesh())),
        MeshMaterial3d(mat),
        Transform::default(),
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
        Actor,
        DespawnOnExit(AppState::InGame),
    ));
}

/// Steps and light taps per graphics quality (Low: a flat layer, one sample).
fn tier_steps(q: usize) -> (f32, f32) {
    match q {
        0 => (1.0, 0.0),
        1 => (4.0, 1.0),
        _ => (8.0, 2.0),
    }
}

/// Feeds the clock, the look's light and the wind to the cloud shader each frame.
#[allow(clippy::too_many_arguments)]
fn update_clouds(
    time: Res<Time<Virtual>>,
    real: Res<Time<Real>>,
    clock: Res<DayClock>,
    sw: Option<Res<StageWorld>>,
    look: Option<Res<Look>>,
    core: Option<Res<engine::settings::CoreSettings>>,
    mut st: ResMut<CloudState>,
    mut mats: ResMut<Assets<CloudMaterial>>,
    cam: Query<&GlobalTransform, With<MainCamera>>,
    mut dome: Query<&mut Transform, With<CloudDome>>,
) {
    let (Some(sw), Some(look)) = (sw, look) else { return };
    let Some(h) = st.material.clone() else { return };
    // Wind runs on game time at the real wind speed (pause stops it).
    st.drift += WIND * time.delta_secs();
    let hours = clock.hours;
    let light = look::light_at(hours);
    let (_, sun_el) = crate::daytime::solar(hours);
    let dir = look.sun.direction();
    let to_light = -dir;
    // What the engine actually gives the light (it fades it under the horizon).
    let fade = ((look.sun.elevation_deg + 3.0) / 3.0).clamp(0.0, 1.0);
    let lux = look.sun.illuminance * fade;
    let c = look.sun.color.linear();
    let daylight = ((sun_el + 6.0) / 12.0).clamp(0.0, 1.0);
    // The light as the clouds get it: through the atmosphere (Bevy does this for the sky and
    // the ground; the clouds are ours). The moon is high: no reddening.
    let trans = if light.moon > 0.0 { Vec3::splat(0.85) } else { sun_transmittance(look.sun.elevation_deg.max(sun_el)) };
    let sin_el = look.sun.elevation_deg.to_radians().sin();
    let up = sin_el.max(0.0);
    // Sky light on the clouds: blue from above, warmer while the sun is low.
    let sky_col = Vec3::new(0.9, 0.72, 0.62).lerp(Vec3::new(0.45, 0.62, 1.0), (sin_el * 5.0).clamp(0.0, 1.0));
    let sky = sky_col * (lux * (0.006 + 0.045 * up)) + look.ambient.color.linear() * look.ambient.brightness * 0.1;
    let ground = Vec3::new(0.32, 0.34, 0.3) * (lux * up * 0.12 / PI) * trans + look.ambient.color.linear() * look.ambient.brightness * 0.03;
    let haze = look.fog.color.linear() / exposure_scale(&look);
    let q = core.map_or(2, |c| c.quality.index());
    let (steps, taps) = tier_steps(q);
    // Coverage: the keys plus a slow wander (a few tenths over a couple of game hours).
    let t = hours / 24.0 * 2.0 * PI;
    let wander = 0.08 * (t * 3.0 + 1.3).sin() + 0.05 * (t * 7.0 + 0.4).sin();
    let cover = (look::cloud_cover(hours, sw.def.weather) + wander).clamp(0.0, 1.0);
    let stars = ((-6.0 - sun_el) / 6.0).clamp(0.0, 1.0);
    // The sky turns once a sidereal day about the pole (46° up, due north); the clock's hours
    // are the hour angle.
    let rot = hours / 24.0 * 2.0 * PI;
    let Some(mut m) = mats.get_mut(&h) else { return };
    m.p = CloudUniform {
        sun_dir: to_light.extend(light.moon),
        sun_rad: (c * lux * trans).extend(daylight),
        amb_top: sky.extend(0.0),
        amb_bot: ground.extend(0.0),
        haze: haze.extend(30_000.0),
        shape: Vec4::new(cover, 0.045, CLOUD_BASE, CLOUD_TOP),
        wind: Vec4::new(st.drift.x, st.drift.y, st.drift.x * 0.6, steps),
        // Stars at ≈ 3× mid-grey of the night exposure at full strength.
        stars: Vec4::new(stars * 2.2 * MOON_LUX.max(0.6) / 0.6, rot, real.elapsed_secs_wrapped(), taps),
    };
    // Sorted as the farthest transparent thing (drawn first): far ahead of the camera.
    if let (Some(cam), Ok(mut tf)) = (cam.iter().next(), dome.single_mut()) {
        let p = cam.translation() + cam.forward() * DOME_R;
        if tf.translation.distance_squared(p) > 1.0 {
            tf.translation = p;
        }
    }
}

/// Rough sunlight transmittance through the air at sun elevation `el` (degrees), seen from
/// cloud height: Kasten-Young air mass, Rayleigh + haze optical depths per channel.
pub fn sun_transmittance(el: f32) -> Vec3 {
    let e = el.max(-1.5);
    let mass = 1.0 / (e.to_radians().sin() + 0.505_72 * (e + 6.079_95).powf(-1.636_4));
    let tau = Vec3::new(0.1, 0.17, 0.32) * 0.8;
    (-tau * mass.max(1.0)).exp()
}

/// Display value of 1 cd/m² at the look's exposure (the fog colour is display-referred).
fn exposure_scale(look: &Look) -> f32 {
    1.0 / (1.2 * 2f32.powf(look.exposure.ev100))
}

// ---------------------------------------------------------------- noise textures

fn hash3(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ (z as u32).wrapping_mul(0xcb1a_b31f) ^ seed.wrapping_mul(0x165_667b1);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^ (h >> 15)
}

fn unit(h: u32) -> f32 {
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

/// Tileable gradient noise (Perlin) at `p` with lattice period `period` (≈ −1..1).
fn perlin2(p: Vec2, period: i32, seed: u32) -> f32 {
    let i = p.floor();
    let f = p - i;
    let g = |dx: i32, dy: i32| {
        let (x, y) = ((i.x as i32 + dx).rem_euclid(period), (i.y as i32 + dy).rem_euclid(period));
        let a = unit(hash3(x, y, 0, seed)) * 2.0 * PI;
        Vec2::new(a.cos(), a.sin()).dot(f - Vec2::new(dx as f32, dy as f32))
    };
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let (a, b, c, d) = (g(0, 0), g(1, 0), g(0, 1), g(1, 1));
    (a + (b - a) * u.x + (c - a) * u.y + (a - b - c + d) * u.x * u.y) * 1.4
}

/// Tileable Worley noise: distance to the nearest feature point (0..≈1), period in cells.
fn worley3(p: Vec3, period: i32, seed: u32) -> f32 {
    let i = p.floor();
    let mut best = 9.0f32;
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let c = i + Vec3::new(dx as f32, dy as f32, dz as f32);
                let (x, y, z) = ((c.x as i32).rem_euclid(period), (c.y as i32).rem_euclid(period), (c.z as i32).rem_euclid(period));
                let h = hash3(x, y, z, seed);
                let o = Vec3::new(unit(h), unit(h.rotate_left(11)), unit(h.rotate_left(22)));
                best = best.min((c + o).distance_squared(p));
            }
        }
    }
    best.sqrt()
}

fn remap(v: f32, lo: f32, hi: f32) -> f32 {
    ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// Weather map, 256² RGBA8, tiling: R macro coverage (Perlin fbm), G cloud shape (Perlin-Worley),
/// B puffs (Worley fbm).
pub fn weather_pixels(n: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let uv = Vec2::new(x as f32, y as f32) / n as f32;
            let fbm = |base: i32, oct: u32, seed: u32| {
                let (mut s, mut a, mut w) = (0.0, 0.5, 0.0);
                for o in 0..oct {
                    let p = 1 << o;
                    s += a * perlin2(uv * (base * p) as f32, base * p, seed + o);
                    w += a;
                    a *= 0.5;
                }
                (s / w) * 0.5 + 0.5
            };
            let wor = |base: i32, seed: u32| {
                let p = uv.extend(0.37);
                let f = |k: i32| 1.0 - worley3(p * (base * k) as f32, base * k, seed + k as u32).min(1.0);
                f(1) * 0.625 + f(2) * 0.25 + f(4) * 0.125
            };
            let macro_ = remap(fbm(3, 4, 11), 0.2, 0.8);
            let pw = fbm(6, 5, 23);
            let w = wor(6, 31);
            // Perlin-Worley: billowy Worley carving the Perlin's lumps.
            let shape = remap(pw, (1.0 - w) * 0.75, 1.0);
            let puffs = wor(12, 47);
            out.extend_from_slice(&[(macro_ * 255.0) as u8, (shape * 255.0) as u8, (puffs * 255.0) as u8, 255]);
        }
    }
    out
}

/// Detail volume, 32³ R8, tiling: inverted Worley fbm (erodes cloud edges into wisps).
pub fn detail_pixels(n: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((n * n * n) as usize);
    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                let p = Vec3::new(x as f32, y as f32, z as f32) / n as f32;
                let f = |k: i32, s: u32| 1.0 - worley3(p * k as f32, k, s).min(1.0);
                let v = f(4, 3) * 0.55 + f(8, 5) * 0.3 + f(16, 7) * 0.15;
                out.push((v.clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
    }
    out
}

fn repeat_sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    })
}

fn weather_image() -> Image {
    const N: u32 = 256;
    let mut img = Image::new(Extent3d { width: N, height: N, depth_or_array_layers: 1 }, TextureDimension::D2, weather_pixels(N), TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD);
    img.sampler = repeat_sampler();
    img
}

fn detail_image() -> Image {
    const N: u32 = 32;
    let mut img = Image::new(Extent3d { width: N, height: N, depth_or_array_layers: N }, TextureDimension::D3, detail_pixels(N), TextureFormat::R8Unorm, RenderAssetUsages::RENDER_WORLD);
    img.sampler = repeat_sampler();
    img
}

// ---------------------------------------------------------------- street lamps

/// Street-lamp lights: real `PointLight`s for the nearest few of B's street-light posts
/// (DECISIONS L7), spawned at load with intensity 0 and moved to the nearest posts.
#[derive(Resource, Default)]
pub struct Lamps {
    /// Lamp head positions (m).
    posts: Vec<Vec3>,
    lights: Vec<Entity>,
    /// `StageWorld` insert tick the lamps belong to.
    tick: Option<u32>,
    last_pick: f64,
}

/// Lamp head height over the post's base (m).
const LAMP_HEIGHT: f32 = 5.8;
/// Lumens per lamp (ART_BIBLE §2.3), warm LED colour.
const LAMP_LM: f32 = 1800.0;
const LAMP_COLOR: Color = Color::srgb(1.0, 0.85, 0.65);

fn spawn_lamps(mut commands: Commands, sw: Option<Res<StageWorld>>, core: Option<Res<engine::settings::CoreSettings>>, mut lamps: ResMut<Lamps>) {
    let Some(sw) = sw else { return };
    let tick = sw.added().get();
    if lamps.tick == Some(tick) {
        return;
    }
    lamps.tick = Some(tick);
    lamps.lights.clear();
    lamps.posts = sw
        .open
        .as_ref()
        .map(|o| o.furniture.signs.iter().filter(|s| s.kind == SignKind::StreetLight).map(|s| s.pos + Vec3::Y * LAMP_HEIGHT).collect())
        .unwrap_or_default();
    if lamps.posts.is_empty() {
        return;
    }
    let n = if core.is_some_and(|c| c.quality.index() == 0) { 8 } else { 24 };
    for _ in 0..n.min(lamps.posts.len()) {
        let e = commands
            .spawn((
                Name::new("StreetLamp"),
                PointLight { intensity: 0.0, range: 18.0, radius: 0.15, color: LAMP_COLOR, shadow_maps_enabled: false, ..default() },
                Transform::from_xyz(0.0, -1000.0, 0.0),
                DespawnOnExit(AppState::InGame),
            ))
            .id();
        lamps.lights.push(e);
    }
}

/// Lights the nearest posts when the sun is under 4°, re-picked twice a second.
fn update_lamps(real: Res<Time<Real>>, clock: Res<DayClock>, mut lamps: ResMut<Lamps>, cam: Query<&GlobalTransform, With<MainCamera>>, mut lights: Query<(&mut PointLight, &mut Transform)>) {
    if lamps.lights.is_empty() {
        return;
    }
    let now = real.elapsed_secs_f64();
    if now - lamps.last_pick < 0.5 {
        return;
    }
    lamps.last_pick = now;
    let (_, el) = clock.sun();
    let on = ((4.0 - el) / 3.0).clamp(0.0, 1.0);
    let Some(cam) = cam.iter().next().map(|c| c.translation()) else { return };
    let mut near: Vec<(f32, Vec3)> = lamps.posts.iter().map(|p| (p.distance_squared(cam), *p)).collect();
    let k = lamps.lights.len().min(near.len());
    if k < near.len() {
        near.select_nth_unstable_by(k, |a, b| a.0.total_cmp(&b.0));
    }
    for (e, (_, p)) in lamps.lights.iter().zip(near.iter().take(k)) {
        if let Ok((mut l, mut tf)) = lights.get_mut(*e) {
            let want = LAMP_LM * on;
            if l.intensity != want {
                l.intensity = want;
            }
            if tf.translation != *p {
                tf.translation = *p;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_tiles_and_spans_its_range() {
        let n = 64;
        let w = weather_pixels(n);
        assert_eq!(w.len(), (n * n * 4) as usize);
        for ch in 0..3 {
            let v: Vec<u8> = w.iter().skip(ch).step_by(4).copied().collect();
            let (lo, hi) = (*v.iter().min().unwrap(), *v.iter().max().unwrap());
            assert!(lo < 60 && hi > 150, "channel {ch}: {lo}..{hi}");
        }
        // Tiling: the noise functions wrap at their period.
        let a = perlin2(Vec2::new(0.3, 0.7), 5, 9);
        let b = perlin2(Vec2::new(5.3, 0.7), 5, 9);
        assert!((a - b).abs() < 1e-4);
        let a = worley3(Vec3::new(0.2, 0.4, 0.9), 4, 1);
        let b = worley3(Vec3::new(4.2, 0.4, 4.9), 4, 1);
        assert!((a - b).abs() < 1e-4);
        let d = detail_pixels(16);
        assert_eq!(d.len(), 16 * 16 * 16);
    }
}
