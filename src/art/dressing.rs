//! PCG dressing assets: the dressing graph's item names (rocks, bushes, grass, posts) mapped
//! to the decimated Poly Haven and Fab (Megascans) models (`assets/models/*.pnm`, `tools/art/build_meshes.py`) and
//! their cooked texture sets. Trees are drawn by [`super::trees`] from the graph's `trunks`.

use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use engine::pcg::PcgCatalog;
use engine::render_budget::DetailTier;
use engine::prelude::*;

use super::pbr;
use crate::stage::Biome;

/// Every mesh file the catalog uses (checked by a test).
pub const MESHES: [&str; 24] = [
    "boulder", "rock_moss_a", "rock_moss_b", "rock_moss_c", "rock_moss_d", "rock_dry", "shrub_a", "shrub_b", "fern_a", "fern_b", "grass_a", "grass_b", "grass_c", "stump",
    // Fab (Megascans) scans, `tools/art/fab_to_gltf.py`.
    "boulder_rauk", "dead_shrub_a", "dead_shrub_b", "dry_branches", "dead_branches", "dry_grass_a", "dry_grass_b", "cocksfoot", "graminea", "kikuyu",
];

/// Rock items (graph node `rocks`) per biome, with weights.
pub fn rocks(b: Biome) -> Vec<(&'static str, f32)> {
    match b {
        Biome::Scrub => vec![("rock_dry", 2.0), ("boulder", 1.0)],
        Biome::SnowForest => vec![("boulder", 1.0), ("rock_moss_c", 1.0), ("rock_moss_d", 1.0)],
        Biome::Moor => vec![("boulder", 1.0), ("boulder_rauk", 1.5), ("rock_moss_a", 0.8), ("rock_moss_b", 0.8), ("rock_moss_d", 0.6)],
        _ => vec![("rock_moss_a", 1.0), ("rock_moss_b", 1.0), ("rock_moss_c", 1.0), ("rock_moss_d", 1.0), ("boulder", 0.8), ("boulder_rauk", 0.8), ("stump", 0.5)],
    }
}

/// Bush / ground-cover items (graph node `bushes`) per biome.
pub fn bushes(b: Biome) -> Vec<(&'static str, f32)> {
    match b {
        Biome::PineForest => vec![("fern_a", 1.0), ("fern_b", 1.0), ("shrub_a", 0.5), ("grass_b", 0.8), ("grass_c", 0.6), ("dead_branches", 0.35), ("dry_branches", 0.25)],
        Biome::BirchHills => vec![("grass_a", 1.5), ("grass_b", 1.2), ("grass_c", 1.0), ("shrub_a", 0.6), ("fern_a", 0.5), ("cocksfoot", 0.6)],
        // Dry country: dead shrubs, bleached grass tufts, fallen branches, a little green.
        Biome::Scrub => vec![("dead_shrub_a", 1.2), ("dead_shrub_b", 1.0), ("dry_grass_a", 1.6), ("dry_grass_b", 1.4), ("dry_branches", 0.4), ("kikuyu", 0.3), ("shrub_a", 0.4)],
        // Golden grassland: tall seed-head grasses and dry tufts.
        Biome::Moor => vec![("cocksfoot", 1.6), ("graminea", 1.2), ("dry_grass_a", 1.4), ("dry_grass_b", 1.2), ("kikuyu", 0.6), ("grass_a", 0.5), ("shrub_b", 0.2)],
        Biome::SnowForest => vec![("shrub_b", 0.4), ("grass_b", 0.3)],
    }
}

/// Dead-shrub tint (linear, over a near-black scan).
pub const DEAD_SHRUB_TINT: Color = Color::linear_rgb(2.1, 1.85, 1.5);

/// Ground cover beyond this from its merged cell's centre is not drawn (dithered).
const COVER_END: f32 = 95.0;

/// Marks merged PCG ground cover (alpha cards) so the render budget's flat far version (an
/// untextured copy: solid quads for foliage) never applies: it fades out instead.
#[derive(Component)]
pub struct Cover;

/// Builds the catalog for a biome: one mesh + material per item name.
pub fn catalog(world: &mut World, biome: Biome, wet: bool) -> PcgCatalog {
    let mut cat = PcgCatalog::default();
    let dir = world.resource::<AssetsDir>().0.clone();
    let rough = |m: &mut StandardMaterial| {
        if wet {
            m.perceptual_roughness = 0.55;
            m.base_color = Color::srgb(0.75, 0.75, 0.76);
        }
    };
    let foliage = |m: &mut StandardMaterial, tint: Color| {
        m.base_color = tint;
        m.alpha_mode = AlphaMode::Mask(0.5);
        m.double_sided = true;
        m.cull_mode = None;
        m.diffuse_transmission = 0.25;
        m.perceptual_roughness = 0.8;
    };
    let snowy = biome == Biome::SnowForest;
    let dry = biome == Biome::Scrub;
    let mut mats = vec![
        ("m_boulder", pbr(world, "m_boulder", |m| rough(m))),
        ("m_rock_moss1", pbr(world, "m_rock_moss1", |m| rough(m))),
        ("m_rock_moss2", pbr(world, "m_rock_moss2", |m| rough(m))),
        ("m_rock_dry", pbr(world, "m_rock_dry", |m| rough(m))),
        ("m_stump", pbr(world, "m_stump", |m| rough(m))),
        ("m_shrub", pbr(world, "m_shrub", |m| foliage(m, if snowy { Color::srgb(0.7, 0.68, 0.62) } else if dry { Color::srgb(0.95, 0.9, 0.7) } else { Color::WHITE }))),
        ("m_fern", pbr(world, "m_fern", |m| foliage(m, Color::srgb(0.85, 0.9, 0.8)))),
        ("m_grass1", pbr(world, "m_grass1", |m| foliage(m, if dry { Color::srgb(1.0, 0.9, 0.62) } else { Color::srgb(0.9, 0.95, 0.85) }))),
        ("m_grass2", pbr(world, "m_grass2", |m| foliage(m, if dry { Color::srgb(1.0, 0.9, 0.62) } else { Color::srgb(0.9, 0.95, 0.85) }))),
    ];
    // The Fab scans load only on biomes that place them (texture memory: the open world is all
    // forest). The older items always load: level edits may place any of them by name.
    let used: Vec<&str> = rocks(biome).into_iter().chain(bushes(biome)).map(|i| i.0).collect();
    let scans: [(&str, &str, &[&str]); 8] = [
        ("m_rock_rauk", "rough", &["boulder_rauk"]),
        ("m_dead_shrub", "dead", &["dead_shrub_a", "dead_shrub_b"]),
        ("m_dry_branches", "rough", &["dry_branches"]),
        ("m_dead_branches", "rough", &["dead_branches"]),
        ("m_dry_grass", "foliage", &["dry_grass_a", "dry_grass_b"]),
        ("m_cocksfoot", "foliage", &["cocksfoot"]),
        ("m_graminea", "foliage", &["graminea"]),
        ("m_kikuyu", "kikuyu", &["kikuyu"]),
    ];
    for (set, how, items) in scans {
        if !items.iter().any(|i| used.contains(i)) {
            continue;
        }
        let h = match how {
            "rough" => pbr(world, set, |m| rough(m)),
            // The scan is near black (burnt wood): lift it to weathered grey-brown.
            "dead" => pbr(world, set, |m| foliage(m, DEAD_SHRUB_TINT)),
            "kikuyu" => pbr(world, set, |m| foliage(m, if dry { Color::srgb(0.95, 0.88, 0.66) } else { Color::WHITE })),
            _ => pbr(world, set, |m| foliage(m, Color::WHITE)),
        };
        mats.push((set, h));
    }
    let mat = |name: &str| mats.iter().find(|m| m.0 == name).map(|m| m.1.clone());
    let items: [(&str, &str); 24] = [
        ("boulder", "m_boulder"),
        ("rock_moss_a", "m_rock_moss1"),
        ("rock_moss_b", "m_rock_moss1"),
        ("rock_moss_c", "m_rock_moss2"),
        ("rock_moss_d", "m_rock_moss2"),
        ("rock_dry", "m_rock_dry"),
        ("stump", "m_stump"),
        ("shrub_a", "m_shrub"),
        ("shrub_b", "m_shrub"),
        ("fern_a", "m_fern"),
        ("fern_b", "m_fern"),
        ("grass_a", "m_grass1"),
        ("grass_b", "m_grass1"),
        ("grass_c", "m_grass2"),
        ("boulder_rauk", "m_rock_rauk"),
        ("dead_shrub_a", "m_dead_shrub"),
        ("dead_shrub_b", "m_dead_shrub"),
        ("dry_branches", "m_dry_branches"),
        ("dead_branches", "m_dead_branches"),
        ("dry_grass_a", "m_dry_grass"),
        ("dry_grass_b", "m_dry_grass"),
        ("cocksfoot", "m_cocksfoot"),
        ("graminea", "m_graminea"),
        ("kikuyu", "m_kikuyu"),
    ];
    for (name, m) in items {
        let Some(m) = mat(m) else { continue };
        match super::meshfile::load(&dir, name) {
            Ok(mesh) => {
                let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
                cat.mesh(name, h, m);
            }
            Err(e) => error!("dressing mesh {name}: {e}"),
        }
    }
    let cover: Vec<AssetId<StandardMaterial>> = ["m_shrub", "m_fern", "m_grass1", "m_grass2", "m_dead_shrub", "m_dry_grass", "m_cocksfoot", "m_graminea", "m_kikuyu", "m_dry_branches", "m_dead_branches"].iter().filter_map(|n| mat(n)).map(|h| h.id()).collect();
    world.insert_resource(CoverMats(cover));
    // Marker posts: plain white posts, made in code.
    let post = Cuboid::new(0.1, 1.05, 0.1).mesh().build().translated_by(Vec3::Y * 0.5);
    let post_mat = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { base_color: Color::srgb(0.88, 0.88, 0.85), perceptual_roughness: 0.6, ..default() });
    let h = world.resource_mut::<Assets<Mesh>>().add(post);
    cat.mesh("post", h, post_mat);
    cat
}

type NewCover = (Added<MeshMaterial3d<StandardMaterial>>, Without<Cover>);

/// Ground-cover (alpha card) materials.
#[derive(Resource, Default)]
pub struct CoverMats(Vec<AssetId<StandardMaterial>>);

/// Ground cover (merged per cell or single plants) gets a dithered fade-out instead of the
/// budget's far version. The budget may classify a piece before this runs (PCG spawns late in
/// the frame): its tier comes off and its flat far twin is despawned (on card bushes the twin
/// is an untextured box).
#[allow(clippy::type_complexity)] // a Bevy query
pub fn tag_cover(
    mut commands: Commands,
    mats: Option<Res<CoverMats>>,
    added: Query<(Entity, &MeshMaterial3d<StandardMaterial>, Has<Name>, Option<&Children>), NewCover>,
    tiers: Query<&DetailTier>,
) {
    let Some(mats) = mats else { return };
    for (e, m, merged, children) in &added {
        if mats.0.contains(&m.id()) {
            for &c in children.into_iter().flatten() {
                if tiers.get(c) == Ok(&DetailTier::Far) {
                    // The streamed dressing volume may drop the cell this frame (try_).
                    commands.entity(c).try_despawn();
                }
            }
            // Merged cells measure from their centre (≤ 45 m from any plant in a 64 m cell).
            let end = if merged { COVER_END } else { COVER_END - 30.0 };
            commands.entity(e).try_remove::<DetailTier>().try_insert((Cover, KeepDetail, NotShadowCaster, VisibilityRange { start_margin: 0.0..0.0, end_margin: (end - 15.0)..end, use_aabb: false }));
        }
    }
}
