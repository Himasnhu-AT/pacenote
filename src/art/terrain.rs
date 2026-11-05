//! Full-PBR terrain: replaces the engine's albedo-only splat material on the terrain chunks
//! with one that also uses each layer's normal and ORM maps (cooked sets), anti-tiling and a
//! height-ish blend between layers (`terrain.wesl`). It reuses the engine's splat map and
//! layer rules ([`super::ground::layers`]), so the layout is identical; only shading changes.

use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use engine::world::terrain::TerrainLook;
use engine::world::{SplatLayer, TerrainChunk, TerrainMaterial};

pub type PnTerrain = ExtendedMaterial<StandardMaterial, PnTerrainExt>;

#[derive(ShaderType, Reflect, Clone, Debug, Default)]
pub struct PnTerrainParams {
    pub tint: [Vec4; 4],
    pub layer: [Vec4; 4],
    pub extent: Vec4,
    pub misc: Vec4,
    /// Cloud shadows (`art::sky`, stream C): xy wind offset (m), z coverage, w strength (0 = off).
    pub cloud: Vec4,
    /// xyz towards the sun, w cloud mid-height (m).
    pub cloud_sun: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct PnTerrainExt {
    #[uniform(100)]
    pub params: PnTerrainParams,
    #[texture(101)]
    #[sampler(102)]
    pub splat: Option<Handle<Image>>,
    #[texture(103)]
    #[sampler(115)]
    pub c0: Option<Handle<Image>>,
    #[texture(104)]
    pub c1: Option<Handle<Image>>,
    #[texture(105)]
    pub c2: Option<Handle<Image>>,
    #[texture(106)]
    pub c3: Option<Handle<Image>>,
    #[texture(107)]
    pub n0: Option<Handle<Image>>,
    #[texture(108)]
    pub n1: Option<Handle<Image>>,
    #[texture(109)]
    pub n2: Option<Handle<Image>>,
    #[texture(110)]
    pub n3: Option<Handle<Image>>,
    #[texture(111)]
    pub o0: Option<Handle<Image>>,
    #[texture(112)]
    pub o1: Option<Handle<Image>>,
    #[texture(113)]
    pub o2: Option<Handle<Image>>,
    #[texture(114)]
    pub o3: Option<Handle<Image>>,
    /// Light under the forest canopy over the terrain (R: 1 = open, 0 = dense forest), see
    /// [`Canopy`].
    #[texture(116)]
    #[sampler(117)]
    pub canopy: Option<Handle<Image>>,
    /// The cloud layer's weather map (`art::sky`), for cloud shadows.
    #[texture(118)]
    #[sampler(119)]
    pub clouds: Option<Handle<Image>>,
}

/// How dense the forest is over the terrain, as a small texture the terrain shader darkens the
/// ground with beyond the shadow distance (where the trees cast no shadows of their own): far
/// forest floors read as shade instead of lit grass between the trunks. Rebuilt from the
/// trees known (`trees::TreeField`) at most once a second while they stream in.
#[derive(Resource)]
pub struct Canopy {
    image: Handle<Image>,
    size: UVec2,
    /// As the splat map: xy = min corner (Bevy XZ), zw = 1 / size (m).
    extent: Vec4,
    since: f32,
}

/// Canopy texel size (m).
pub const CANOPY_TEXEL: f32 = 16.0;
/// Trees per texel that make a closed canopy.
const CANOPY_FULL: f32 = 7.0;

/// The light under the canopy (0..=255 per texel) from tree counts per texel: a 3×3 blur of
/// the density, 255 where open.
pub fn canopy_light(counts: &[u16], size: UVec2) -> Vec<u8> {
    let (w, h) = (size.x as i32, size.y as i32);
    let at = |x: i32, y: i32| counts[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize] as f32;
    let mut out = vec![255u8; counts.len()];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for (dx, dy, k) in [(0, 0, 4.0), (1, 0, 2.0), (-1, 0, 2.0), (0, 1, 2.0), (0, -1, 2.0), (1, 1, 1.0), (-1, 1, 1.0), (1, -1, 1.0), (-1, -1, 1.0)] {
                s += at(x + dx, y + dy) * k;
            }
            let d = (s / 16.0 / CANOPY_FULL).min(1.0);
            out[(y * w + x) as usize] = ((1.0 - d) * 255.0).round() as u8;
        }
    }
    out
}

fn canopy_update(time: Res<Time>, field: Option<Res<super::trees::TreeField>>, canopy: Option<ResMut<Canopy>>, mut images: ResMut<Assets<Image>>) {
    let (Some(field), Some(mut c)) = (field, canopy) else { return };
    c.since += time.delta_secs();
    if c.since < 1.0 || !field.is_changed() {
        return;
    }
    c.since = 0.0;
    let (w, h) = (c.size.x as usize, c.size.y as usize);
    let mut counts = vec![0u16; w * h];
    for p in field.positions() {
        let u = ((p.x - c.extent.x) * c.extent.z * w as f32) as i32;
        let v = ((p.z - c.extent.y) * c.extent.w * h as f32) as i32;
        if u >= 0 && v >= 0 && (u as usize) < w && (v as usize) < h {
            let k = v as usize * w + u as usize;
            counts[k] = counts[k].saturating_add(1);
        }
    }
    let light = canopy_light(&counts, c.size);
    if let Some(mut img) = images.get_mut(&c.image) {
        img.data = Some(light);
    }
}

impl MaterialExtension for PnTerrainExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://pacenote/art/terrain.wesl".into()
    }
}

pub struct TerrainArtPlugin;

impl Plugin for TerrainArtPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<MaterialPlugin<StandardMaterial>>() {
            return; // headless
        }
        bevy::asset::embedded_asset!(app, "terrain.wesl");
        app.add_plugins(MaterialPlugin::<PnTerrain>::default()).add_systems(Update, (build, swap, canopy_update).chain());
    }
}

/// The stage's layers (set + tint + tiling), waiting for the engine's material to exist.
#[derive(Resource, Clone)]
pub struct TerrainPlan(pub Vec<SplatLayer>);

#[derive(Resource, Clone)]
pub struct PnTerrainHandle(pub Handle<PnTerrain>);

fn set_of(l: &SplatLayer) -> Option<String> {
    let t = l.texture.as_ref()?;
    t.strip_prefix("textures/")?.split('/').next().map(str::to_string)
}

/// Builds our material once the engine's terrain material (and its splat map) exists.
#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    plan: Option<Res<TerrainPlan>>,
    have: Option<Res<PnTerrainHandle>>,
    look: Option<Res<TerrainLook>>,
    engine_mats: Res<Assets<TerrainMaterial>>,
    mut mats: ResMut<Assets<PnTerrain>>,
    mut images: ResMut<Assets<Image>>,
    server: Res<AssetServer>,
    clouds: Option<Res<super::sky::CloudWeather>>,
) {
    let (Some(plan), None, Some(look)) = (plan, have, look) else { return };
    let TerrainLook::Splat(h) = &*look else { return };
    let Some(src) = engine_mats.get(h) else { return };
    let load = |p: String, srgb: bool| -> Handle<Image> {
        server
            .load_builder()
            .with_settings(move |s: &mut ImageLoaderSettings| {
                s.is_srgb = srgb;
                s.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
                s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    anisotropy_clamp: 16,
                    ..ImageSamplerDescriptor::linear()
                });
            })
            .load(p)
    };
    let mut ext = PnTerrainExt { splat: src.extension.splat.clone(), clouds: clouds.map(|c| c.0.clone()), ..default() };
    ext.params.extent = src.extension.params.extent;
    // Macro variation: patchy fields and slopes rather than one flat colour.
    ext.params.misc = Vec4::new(0.26, 0.011, 1.0, 0.0);
    // Missing layers repeat the last one with zero weight in the splat map.
    let layers: Vec<SplatLayer> = (0..4).map(|i| plan.0.get(i).or(plan.0.last()).cloned().unwrap_or_default()).collect();
    for (i, l) in layers.iter().enumerate() {
        let set = set_of(l).unwrap_or_else(|| "rock".into());
        // Engine convention: colour 0.5 = texture as is.
        let c = Color::srgb(l.color[0] * 2.0, l.color[1] * 2.0, l.color[2] * 2.0).to_linear();
        ext.params.tint[i] = Vec4::new(c.red, c.green, c.blue, l.roughness / 0.9);
        ext.params.layer[i] = Vec4::new(1.0 / l.tiling.max(0.1), 1.0, 0.75, 0.0);
        let (cl, nl, ol) = (load(format!("textures/{set}/color.ktx2"), true), load(format!("textures/{set}/normal.ktx2"), false), load(format!("textures/{set}/orm.ktx2"), false));
        match i {
            0 => (ext.c0, ext.n0, ext.o0) = (Some(cl), Some(nl), Some(ol)),
            1 => (ext.c1, ext.n1, ext.o1) = (Some(cl), Some(nl), Some(ol)),
            2 => (ext.c2, ext.n2, ext.o2) = (Some(cl), Some(nl), Some(ol)),
            _ => (ext.c3, ext.n3, ext.o3) = (Some(cl), Some(nl), Some(ol)),
        }
    }
    // The canopy texture over the same extent as the splat map, open everywhere until the
    // trees come in.
    let e = ext.params.extent;
    let size = (Vec2::new(1.0 / e.z, 1.0 / e.w) / CANOPY_TEXEL).ceil().as_uvec2().clamp(UVec2::splat(1), UVec2::splat(1024));
    let mut img = Image::new_fill(
        bevy::render::render_resource::Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        &[255],
        bevy::render::render_resource::TextureFormat::R8Unorm,
        bevy::asset::RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::linear();
    let canopy = images.add(img);
    ext.canopy = Some(canopy.clone());
    commands.insert_resource(Canopy { image: canopy, size, extent: e, since: 10.0 });
    let m = mats.add(PnTerrain { base: StandardMaterial { perceptual_roughness: 0.9, ..default() }, extension: ext });
    commands.insert_resource(PnTerrainHandle(m));
}

/// Puts our material on every terrain chunk (chunks keep their entity across LOD changes).
fn swap(mut commands: Commands, mine: Option<Res<PnTerrainHandle>>, chunks: Query<Entity, (With<TerrainChunk>, With<MeshMaterial3d<TerrainMaterial>>)>) {
    let Some(mine) = mine else { return };
    for e in &chunks {
        commands.entity(e).remove::<MeshMaterial3d<TerrainMaterial>>().insert(MeshMaterial3d(mine.0.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canopy_is_open_without_trees_and_shaded_in_a_forest() {
        let size = UVec2::new(8, 8);
        let mut counts = vec![0u16; 64];
        assert!(canopy_light(&counts, size).iter().all(|&l| l == 255));
        // A dense block in the middle, one stray tree in a corner.
        for y in 2..6 {
            for x in 2..6 {
                counts[y * 8 + x] = 12;
            }
        }
        counts[0] = 1;
        let l = canopy_light(&counts, size);
        assert!(l[4 * 8 + 4] < 20, "{}", l[4 * 8 + 4]);
        assert!(l[0] > 200 && l[7 * 8 + 7] == 255);
        // Soft edge: the block's border is between.
        assert!(l[2 * 8 + 1] > l[3 * 8 + 3] && l[2 * 8 + 1] < 255);
    }
}
