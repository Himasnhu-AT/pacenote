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
        app.add_plugins(MaterialPlugin::<PnTerrain>::default()).add_systems(Update, (build, swap).chain());
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
fn build(
    mut commands: Commands,
    plan: Option<Res<TerrainPlan>>,
    have: Option<Res<PnTerrainHandle>>,
    look: Option<Res<TerrainLook>>,
    engine_mats: Res<Assets<TerrainMaterial>>,
    mut mats: ResMut<Assets<PnTerrain>>,
    server: Res<AssetServer>,
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
    let mut ext = PnTerrainExt { splat: src.extension.splat.clone(), ..default() };
    ext.params.extent = src.extension.params.extent;
    ext.params.misc = Vec4::new(0.16, 0.012, 1.0, 0.0);
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
