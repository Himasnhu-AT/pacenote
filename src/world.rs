//! The stage in the world: terrain (engine `world`), the road mesh, PCG dressing (trees,
//! rocks, bushes, marker posts) and the collision data the car simulation reads.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use engine::pcg::{PcgCatalog, PcgGraphRef, PcgVolume};
use engine::render_budget::Scenery;
use engine::world::{Heightfield, SplatLayer, WorldDesc};
use pcg_core::{Area, Graph, Inputs, Op, Polyline, Rect, Side, SpawnDef, SpawnKind, Tier};

use crate::stage::{self, Biome, Corner, Road, StageDef, Surface};
use crate::vehicle::Ground;

/// The current stage, generated.
#[derive(Resource, Clone)]
pub struct StageWorld {
    pub def: StageDef,
    pub road: Arc<Road>,
    pub heights: Arc<Heightfield>,
    pub corners: Arc<Vec<Corner>>,
    pub crests: Arc<Vec<f32>>,
    /// Trunks and boulders near the road: 8 m grid of (centre, radius, top).
    obstacles: Arc<HashMap<(i32, i32), Vec<(Vec2, f32, f32)>>>,
}

const OBS_CELL: f32 = 8.0;

impl Ground for StageWorld {
    fn height(&self, x: f32, z: f32) -> f32 {
        self.heights.height_at(x, z)
    }
    fn normal(&self, x: f32, z: f32) -> Vec3 {
        self.heights.smooth_normal_at(x, z)
    }
    fn surface(&self, x: f32, z: f32) -> Surface {
        stage::surface_at(x, z, &self.road)
    }
    fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<(Vec2, f32, f32)>) {
        out.clear();
        let (x0, x1) = (((p.x - r) / OBS_CELL).floor() as i32, ((p.x + r) / OBS_CELL).floor() as i32);
        let (z0, z1) = (((p.y - r) / OBS_CELL).floor() as i32, ((p.y + r) / OBS_CELL).floor() as i32);
        for z in z0..=z1 {
            for x in x0..=x1 {
                if let Some(v) = self.obstacles.get(&(x, z)) {
                    out.extend_from_slice(v);
                }
            }
        }
    }
}

/// Builds everything that doesn't need the ECS (also used headless and by the guide).
pub fn generate(def: &StageDef) -> StageWorld {
    let road = stage::generate_road(def);
    let half = stage::half_size(def);
    let n = (2.0 * half / stage::SPACING) as u32 + 1;
    let origin = Vec2::splat(-half);
    let heights = Heightfield::from_fn(n, n, stage::SPACING, origin, |x, z| stage::height(x, z, def, &road));
    let corners = stage::corners(&road);
    let crests = stage::crests(&road);
    let mut world = StageWorld {
        def: def.clone(),
        road: Arc::new(road),
        heights: Arc::new(heights),
        corners: Arc::new(corners),
        crests: Arc::new(crests),
        obstacles: Arc::new(HashMap::new()),
    };
    // Collision for the car: the same points the dressing spawns, evaluated in a band along
    // the road (the trees far away can't be reached).
    let (graph, inputs) = dressing(&world);
    let band = road_area(&world.road, 40.0);
    if let Ok(out) = pcg_core::evaluate(&graph, &inputs, Some(band)) {
        let mut grid: HashMap<(i32, i32), Vec<(Vec2, f32, f32)>> = HashMap::new();
        for (port, radius) in [("trunks", 0.32f32), ("boulders", 0.85)] {
            if let Some(p) = out.emits.get(port) {
                for k in 0..p.len() {
                    let c = Vec2::new(p.pos[k].x, p.pos[k].z);
                    let near = world.road.nearest(c, 30.0).is_some();
                    if !near {
                        continue;
                    }
                    let r = radius * p.scale[k].x;
                    let top = p.pos[k].y + 4.0 * p.scale[k].y;
                    grid.entry(((c.x / OBS_CELL).floor() as i32, (c.y / OBS_CELL).floor() as i32)).or_default().push((c, r, top));
                }
            }
        }
        world.obstacles = Arc::new(grid);
    }
    world
}

fn road_area(road: &Road, pad: f32) -> Area {
    let mut a = Area::EMPTY;
    for p in &road.pts {
        a = a.include(Vec2::new(p.x, p.z));
    }
    a.expand(pad)
}

/// The dressing graph and its inputs for a stage.
pub fn dressing(w: &StageWorld) -> (Graph, Inputs) {
    let road = &w.road;
    let half = stage::half_size(&w.def);
    // The road as rects (20 m pieces) to keep things off it, and as a polyline for posts.
    let mut rects = Vec::new();
    for i in (0..road.len().saturating_sub(10)).step_by(10) {
        let (a, b) = (road.pts[i], road.pts[i + 10]);
        let mid = (a + b) * 0.5;
        let d = Vec2::new(b.x - a.x, b.z - a.z);
        rects.push(Rect { center: Vec2::new(mid.x, mid.z), half: Vec2::new(road.width[i] * 0.5, d.length() * 0.5 + 1.0), yaw: d.x.atan2(d.y), y: mid.y, tag: "road".into() });
    }
    let line = Polyline { points: road.pts.iter().step_by(5).copied().collect(), closed: false, tag: "road".into() };
    let hf = &w.heights;
    let inputs = Inputs::default()
        .rects("area", vec![Rect::new(Vec2::ZERO, Vec2::splat(half - 4.0))])
        .rects("road", rects)
        .lines("line", vec![line])
        .surface(pcg_core::Surface::Heightfield { origin: hf.origin(), cell: hf.spacing(), width: hf.nx() as usize, heights: hf.heights().to_vec() });

    let (tree_r, forest_lo, forest_hi, trees, rock_keep, bush_keep): (f32, f32, f32, Vec<(&str, f32)>, f32, f32) = match w.def.biome {
        Biome::PineForest => (4.2, 0.3, 0.6, vec![("pine", 4.0), ("spruce", 2.0), ("birch", 0.6)], 0.25, 0.3),
        Biome::BirchHills => (6.0, 0.42, 0.7, vec![("birch", 3.0), ("pine", 1.0)], 0.2, 0.45),
        Biome::Scrub => (9.0, 0.45, 0.75, vec![("scrub_tree", 2.0), ("pine", 0.5)], 0.6, 0.6),
        Biome::Moor => (14.0, 0.6, 0.85, vec![("pine", 1.0), ("birch", 1.0)], 0.5, 0.35),
        Biome::SnowForest => (4.6, 0.32, 0.6, vec![("snow_pine", 4.0), ("spruce", 1.5)], 0.2, 0.15),
    };
    let spawn = |items: &[(&str, f32)], tier: Tier, shadows: bool| {
        Op::Spawn(SpawnDef { kind: SpawnKind::Mesh, items: items.iter().map(|(n, w)| (n.to_string(), *w)).collect(), tier, shadows, ..Default::default() })
    };
    let g = Graph::build(w.def.seed)
        .input("area")
        .node("road", Op::Input { name: "road".into() }, &[])
        .node("line", Op::Input { name: "line".into() }, &[])
        // Trees: forests by noise, clear of the road.
        .node("tree_pts", Op::PoissonSample { radius: tree_r }, &["area"])
        .node("forest", Op::DensityNoise { scale: 160.0, octaves: 3, lo: forest_lo, hi: forest_hi }, &["tree_pts"])
        .node("trees_in", Op::DensityFilter { lo: 0.15, hi: 1.0 }, &["forest"])
        .node("trees_clear", Op::Difference { against: "road".into(), pad: 4.5 }, &["trees_in"])
        .node("trees_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.75, 1.35), jitter: 0.0 }, &["trees_clear"])
        .node("trees_on", Op::Project { offset: -0.1, align: false }, &["trees_tf"])
        .node("trees", spawn(&trees, Tier::Essential, true), &["trees_on"])
        .node("trunks", Op::Emit { port: "trunks".into() }, &["trees_on"])
        // Boulders and rocks.
        .node("rock_pts", Op::PoissonSample { radius: 12.0 }, &["area"])
        .node("rock_keep", Op::Keep { fraction: rock_keep }, &["rock_pts"])
        .node("rock_clear", Op::Difference { against: "road".into(), pad: 2.2 }, &["rock_keep"])
        .node("rock_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.6, 1.6), jitter: 0.0 }, &["rock_clear"])
        .node("rock_on", Op::Project { offset: -0.25, align: true }, &["rock_tf"])
        .node("rocks", spawn(&[("rock", 2.0), ("rock_flat", 1.0)], Tier::Essential, true), &["rock_on"])
        .node("boulders", Op::Emit { port: "boulders".into() }, &["rock_on"])
        // Bushes and grass tufts: detail, thinned at low quality.
        .node("bush_pts", Op::PoissonSample { radius: 3.2 }, &["area"])
        .node("bush_keep", Op::Keep { fraction: bush_keep }, &["bush_pts"])
        .node("bush_clear", Op::Difference { against: "road".into(), pad: 1.0 }, &["bush_keep"])
        .node("bush_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.6, 1.4), jitter: 0.0 }, &["bush_clear"])
        .node("bush_on", Op::Project { offset: -0.05, align: true }, &["bush_tf"])
        .node("bushes", spawn(&[("bush", 1.0), ("grass", 2.0)], Tier::Fine, false), &["bush_on"])
        // Marker posts both sides every 50 m.
        .node("posts_l", Op::EdgeSample { spacing: 50.0, offset: road.width[0] * 0.5 + 1.4, side: Side::Left }, &["line"])
        .node("posts_r", Op::EdgeSample { spacing: 50.0, offset: road.width[0] * 0.5 + 1.4, side: Side::Right }, &["line"])
        .node("posts_all", Op::Union, &["posts_l", "posts_r"])
        .node("posts_on", Op::Project { offset: 0.0, align: false }, &["posts_all"])
        .node("posts", spawn(&[("post", 1.0)], Tier::Essential, true), &["posts_on"])
        .finish()
        .expect("dressing graph");
    (g, inputs)
}

/// Terrain layers for a biome.
fn layers(b: Biome) -> Vec<SplatLayer> {
    let l = |name: &str, color: [f32; 3], rough: f32, slope: [f32; 2], noise: f32, weight: f32| SplatLayer {
        name: name.into(),
        color,
        roughness: rough,
        slope,
        noise,
        weight,
        blend: 6.0,
        tiling: 6.0,
        ..default()
    };
    match b {
        Biome::SnowForest => vec![l("snow", [0.88, 0.9, 0.94], 0.6, [0.0, 90.0], 0.2, 1.0), l("rock", [0.42, 0.42, 0.44], 0.85, [32.0, 90.0], 0.0, 1.6)],
        Biome::Scrub => vec![l("dry", [0.62, 0.52, 0.36], 0.95, [0.0, 90.0], 0.3, 1.0), l("scrub", [0.4, 0.42, 0.24], 0.95, [0.0, 22.0], 0.6, 0.7), l("rock", [0.55, 0.45, 0.36], 0.85, [26.0, 90.0], 0.0, 1.6)],
        Biome::Moor => vec![l("heath", [0.36, 0.34, 0.24], 0.95, [0.0, 90.0], 0.3, 1.0), l("grass", [0.38, 0.44, 0.24], 0.95, [0.0, 18.0], 0.5, 0.8), l("rock", [0.44, 0.43, 0.42], 0.85, [24.0, 90.0], 0.0, 1.6)],
        _ => vec![l("grass", [0.27, 0.38, 0.17], 0.95, [0.0, 90.0], 0.3, 1.0), l("forest_floor", [0.28, 0.23, 0.15], 0.95, [0.0, 25.0], 0.7, 0.8), l("rock", [0.42, 0.41, 0.4], 0.85, [30.0, 90.0], 0.0, 1.6)],
    }
}

/// Spawns the stage: terrain, road, dressing volume.
pub fn spawn(world: &mut World, s: &StageWorld) {
    let half = stage::half_size(&s.def);
    let desc = WorldDesc {
        seed: s.def.seed,
        terrain: engine::world::desc::TerrainDesc {
            size: [2.0 * half, 2.0 * half],
            spacing: stage::SPACING,
            chunk_quads: 64,
            origin: Some([-half, -half]),
            layers: layers(s.def.biome),
            ..default()
        },
        cell_size: 512.0,
        ..default()
    };
    if let Err(e) = engine::world::desc::spawn_world_with(world, desc, Some((*s.heights).clone())) {
        error!("terrain: {e}");
    }
    // Headless (tests, a dedicated server) has no materials: physics data only.
    if world.contains_resource::<Assets<StandardMaterial>>() {
        spawn_road(world, s);
        spawn_dressing(world, s);
    }
    world.insert_resource(s.clone());
}

fn surface_color(surf: Surface) -> Color {
    match surf {
        Surface::Gravel => Color::srgb(0.55, 0.49, 0.4),
        Surface::Tarmac => Color::srgb(0.2, 0.2, 0.21),
        Surface::Mud => Color::srgb(0.32, 0.25, 0.18),
        Surface::Snow => Color::srgb(0.85, 0.86, 0.9),
        _ => Color::srgb(0.4, 0.38, 0.3),
    }
}

/// The road surface as ribbon meshes (one per ~200 m, per surface run), 4 cm above the terrain.
fn spawn_road(world: &mut World, s: &StageWorld) {
    let road = &s.road;
    let mut mats: HashMap<u8, Handle<StandardMaterial>> = HashMap::new();
    let mut i = 0;
    while i + 1 < road.len() {
        let surf = road.surface[i];
        let mut j = i;
        while j + 1 < road.len() && road.surface[j] == surf && j - i < 100 {
            j += 1;
        }
        let mesh = ribbon(road, i, j);
        let key = surf as u8;
        let mat = mats
            .entry(key)
            .or_insert_with(|| world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { base_color: surface_color(surf), perceptual_roughness: 0.92, ..default() }))
            .clone();
        let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
        world.spawn((Name::new("road"), Mesh3d(h), MeshMaterial3d(mat), Transform::default(), Scenery, engine::render_budget::KeepDetail));
        i = j;
    }
}

fn ribbon(road: &Road, a: usize, b: usize) -> Mesh {
    let (mut pos, mut nrm, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let across = [-1.0f32, -0.5, 0.0, 0.5, 1.0];
    for i in a..=b {
        let p = road.pts[i];
        let d = road.dir[i];
        // Left of travel = up × direction.
        let left = Vec3::new(d.y, 0.0, -d.x);
        let half = road.width[i] * 0.5 + 0.4;
        for &u in &across {
            let lat = u * half;
            let r = stage::RoadPos { i: i.min(road.len() - 2), t: if i == road.len() - 1 { 1.0 } else { 0.0 }, s: i as f32 * stage::STEP, lateral: lat };
            let y = road.height_at(&r) + 0.04;
            let q = Vec3::new(p.x, y, p.z) + left * lat;
            pos.push(q.to_array());
            nrm.push([0.0, 1.0, 0.0]);
            uv.push([(u * 0.5 + 0.5) * road.width[i] / 4.0, i as f32 * stage::STEP / 4.0]);
        }
    }
    let cols = across.len() as u32;
    for r in 0..(b - a) as u32 {
        for c in 0..cols - 1 {
            let (i0, i1) = (r * cols + c, (r + 1) * cols + c);
            idx.extend_from_slice(&[i0, i1, i0 + 1, i0 + 1, i1, i1 + 1]);
        }
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    m.insert_indices(Indices::U32(idx));
    m.compute_smooth_normals();
    m
}

/// Placeholder vegetation and props until the asset pack lands (milestone 4).
fn spawn_dressing(world: &mut World, s: &StageWorld) {
    let mut cat = PcgCatalog::default();
    {
        let mut add = |w: &mut World, name: &str, mesh: Mesh, color: Color| {
            let m = w.resource_mut::<Assets<Mesh>>().add(mesh);
            let mat = w.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { base_color: color, perceptual_roughness: 0.9, ..default() });
            cat.mesh(name, m, mat);
        };
        add(world, "pine", Cone { radius: 1.8, height: 9.0 }.mesh().resolution(10).build().translated_by(Vec3::Y * 5.0), Color::srgb(0.12, 0.24, 0.13));
        add(world, "spruce", Cone { radius: 2.2, height: 12.0 }.mesh().resolution(10).build().translated_by(Vec3::Y * 6.5), Color::srgb(0.1, 0.2, 0.12));
        add(world, "snow_pine", Cone { radius: 2.0, height: 10.0 }.mesh().resolution(10).build().translated_by(Vec3::Y * 5.5), Color::srgb(0.65, 0.72, 0.7));
        add(world, "birch", Sphere::new(2.4).mesh().ico(2).unwrap().scaled_by(Vec3::new(1.0, 1.6, 1.0)).translated_by(Vec3::Y * 5.5), Color::srgb(0.38, 0.5, 0.22));
        add(world, "scrub_tree", Sphere::new(2.0).mesh().ico(1).unwrap().scaled_by(Vec3::new(1.3, 0.8, 1.3)).translated_by(Vec3::Y * 2.2), Color::srgb(0.35, 0.38, 0.2));
        add(world, "rock", Sphere::new(1.0).mesh().ico(1).unwrap().scaled_by(Vec3::new(1.2, 0.8, 1.0)), Color::srgb(0.45, 0.44, 0.42));
        add(world, "rock_flat", Sphere::new(1.0).mesh().ico(1).unwrap().scaled_by(Vec3::new(1.6, 0.5, 1.3)), Color::srgb(0.5, 0.48, 0.44));
        add(world, "bush", Sphere::new(0.7).mesh().ico(1).unwrap().translated_by(Vec3::Y * 0.4), Color::srgb(0.22, 0.33, 0.14));
        add(world, "grass", Cone { radius: 0.35, height: 0.6 }.mesh().resolution(5).build().translated_by(Vec3::Y * 0.3), Color::srgb(0.35, 0.45, 0.2));
        add(world, "post", Cylinder::new(0.06, 1.1).mesh().resolution(6).build().translated_by(Vec3::Y * 0.55), Color::srgb(0.9, 0.9, 0.88));
    }
    world.insert_resource(cat);
    let (graph, inputs) = dressing(s);
    let half = stage::half_size(&s.def);
    world.spawn(PcgVolume { graph: PcgGraphRef::Inline(Arc::new(graph)), inputs: Arc::new(inputs), area: Area::new(Vec2::splat(-half), Vec2::splat(half)), cell: 128.0 });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_world_has_obstacles_near_the_road_and_none_on_it() {
        let def = &stage::stages()[0];
        let w = generate(def);
        let mut out = Vec::new();
        let mut total = 0;
        for i in (0..w.road.len()).step_by(25) {
            let p = w.road.pts[i];
            w.obstacles(Vec2::new(p.x, p.z), 30.0, &mut out);
            total += out.len();
            for (c, r, _) in &out {
                let rp = w.road.nearest(*c, 50.0).unwrap();
                assert!(rp.lateral.abs() > w.road.half_width(&rp) + 1.0 - r, "obstacle on the road at station {i}");
            }
        }
        assert!(total > 50, "only {total} obstacles near the road");
        // Terrain matches the road surface.
        let p = w.road.pts[w.road.len() / 2];
        assert!((w.height(p.x, p.z) - p.y).abs() < 0.3);
    }
}

#[cfg(test)]
mod debug {
    use super::*;
    use crate::vehicle::{Car, Input, Spec, step};

    #[test]
    #[ignore]
    fn drive_from_the_start() {
        let def = &stage::stages()[0];
        let w = generate(def);
        let (p, h) = w.road.start();
        let spec = Spec::default();
        let mut car = Car::new(p + Vec3::Y * 0.9, h);
        println!("start {p:?} heading {h:.2} ground {:.2} surface {:?}", w.height(p.x, p.z), w.surface(p.x, p.z));
        for i in 0..480 {
            let input = if i < 60 { Input::default() } else { Input { throttle: 1.0, ..Input::default() } };
            step(&mut car, &spec, input, &w, 1.0 / 60.0);
            if i % 40 == 0 {
                let wh = &car.wheels;
                println!(
                    "t {:.2} pos {:.1?} v {:5.1} gear {} rpm {:5.0} up {:.2} contact {:?} comp {:.2?} surf {:?} load {:.0?}",
                    i as f32 / 60.0,
                    car.pos,
                    car.speed(),
                    car.gear,
                    car.rpm,
                    car.up().y,
                    wh.iter().map(|w| w.contact).collect::<Vec<_>>(),
                    wh.iter().map(|w| w.compression).collect::<Vec<_>>(),
                    wh[0].surface,
                    wh.iter().map(|w| w.load).collect::<Vec<_>>()
                );
            }
        }
    }
}

#[cfg(test)]
mod start_tests {
    use super::*;
    use crate::vehicle::{Car, Input, Spec, step};

    /// Every stage: full throttle from the start line gets the car moving down the road.
    #[test]
    fn every_stage_drives_off_the_line() {
        let spec = Spec::default();
        for def in stage::stages() {
            let w = generate(&def);
            let (p, h) = w.road.start();
            let mut car = Car::new(p + Vec3::Y * 0.9, h);
            for i in 0..300 {
                let input = if i < 60 { Input { handbrake: true, ..Input::default() } } else { Input { throttle: 1.0, ..Input::default() } };
                step(&mut car, &spec, input, &w, 1.0 / 60.0);
            }
            let moved = (car.pos - p).length();
            // 4 s of full throttle: about 25–65 km/h depending on surface and slope.
            assert!(car.speed() * 3.6 > 20.0 && moved > 10.0, "stage {} {}: {:.1} km/h after 4 s, moved {moved:.1} m", def.id, def.name, car.speed() * 3.6);
        }
    }
}
