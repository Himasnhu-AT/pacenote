//! The stage in the world: terrain (engine `world`), the dressing graph (trees, rocks, bushes,
//! marker posts) and the collision data the car simulation reads. The visuals (road surface,
//! materials, trees, sky) are `crate::art`'s.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use engine::world::{Heightfield, WorldDesc};
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

impl StageWorld {
    /// Every obstacle the car can hit (trunk / boulder centre, radius, top), sorted: for
    /// checking stages generate the same scenery on every machine.
    pub fn obstacle_list(&self) -> Vec<(Vec2, f32, f32)> {
        let mut v: Vec<(Vec2, f32, f32)> = self.obstacles.values().flatten().copied().collect();
        v.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.0.y.total_cmp(&b.0.y)).then(a.1.total_cmp(&b.1)).then(a.2.total_cmp(&b.2)));
        v
    }
}

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
        rects.push(Rect { center: Vec2::new(mid.x, mid.z), half: Vec2::new(road.width[i] * 0.5, d.length() * 0.5 + 1.0), yaw: libm::atan2f(d.x, d.y), y: mid.y, tag: "road".into() });
    }
    let line = Polyline { points: road.pts.iter().step_by(5).copied().collect(), closed: false, tag: "road".into() };
    let hf = &w.heights;
    let inputs = Inputs::default()
        .rects("area", vec![Rect::new(Vec2::ZERO, Vec2::splat(half - 4.0))])
        .rects("road", rects)
        .lines("line", vec![line])
        .surface(pcg_core::Surface::Heightfield { origin: hf.origin(), cell: hf.spacing(), width: hf.nx() as usize, heights: hf.heights().to_vec() });

    // Trees are drawn by `art::trees` from the `trunks` points (species mix per biome there).
    let (tree_r, forest_lo, forest_hi, rock_keep, bush_keep): (f32, f32, f32, f32, f32) = match w.def.biome {
        Biome::PineForest => (4.2, 0.3, 0.6, 0.25, 0.3),
        Biome::BirchHills => (6.0, 0.42, 0.7, 0.2, 0.45),
        Biome::Scrub => (9.0, 0.45, 0.75, 0.6, 0.6),
        Biome::Moor => (14.0, 0.6, 0.85, 0.5, 0.35),
        Biome::SnowForest => (4.6, 0.32, 0.6, 0.2, 0.15),
    };
    let (rocks, bushes) = (crate::art::dressing::rocks(w.def.biome), crate::art::dressing::bushes(w.def.biome));
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
        .node("trunks", Op::Emit { port: "trunks".into() }, &["trees_on"])
        // Boulders and rocks.
        .node("rock_pts", Op::PoissonSample { radius: 12.0 }, &["area"])
        .node("rock_keep", Op::Keep { fraction: rock_keep }, &["rock_pts"])
        .node("rock_clear", Op::Difference { against: "road".into(), pad: 2.2 }, &["rock_keep"])
        .node("rock_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.6, 1.6), jitter: 0.0 }, &["rock_clear"])
        .node("rock_on", Op::Project { offset: -0.25, align: true }, &["rock_tf"])
        .node("rocks", spawn(&rocks, Tier::Essential, true), &["rock_on"])
        .node("boulders", Op::Emit { port: "boulders".into() }, &["rock_on"])
        // Bushes and grass tufts: detail, thinned at low quality.
        .node("bush_pts", Op::PoissonSample { radius: 3.2 }, &["area"])
        .node("bush_keep", Op::Keep { fraction: bush_keep }, &["bush_pts"])
        // Ground cover only matters near the road (the camera never leaves it): a 90 m band.
        .node("bush_band", Op::Intersect { with: "road".into(), pad: 90.0 }, &["bush_keep"])
        .node("bush_clear", Op::Difference { against: "road".into(), pad: 1.0 }, &["bush_band"])
        .node("bush_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.6, 1.4), jitter: 0.0 }, &["bush_clear"])
        .node("bush_on", Op::Project { offset: -0.05, align: true }, &["bush_tf"])
        .node("bushes", spawn(&bushes, Tier::Fine, false), &["bush_on"])
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
            layers: crate::art::ground::layers(s.def.biome, crate::art::wet(&s.def)),
            ..default()
        },
        cell_size: 512.0,
        ..default()
    };
    if let Err(e) = engine::world::desc::spawn_world_with(world, desc, Some((*s.heights).clone())) {
        error!("terrain: {e}");
    }
    crate::art::spawn_stage(world, s);
    world.insert_resource(s.clone());
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
            // Countdown on the handbrake, then 8 s with no pedals (the guide reading the first
            // notes): hill hold keeps the car on the line.
            for i in 0..(60 * 9) {
                let input = if i < 60 { Input { handbrake: true, ..Input::default() } } else { Input::default() };
                step(&mut car, &spec, input, &w, 1.0 / 60.0);
            }
            let drift = (car.pos - p).length() - 0.9;
            assert!(drift.abs() < 0.5, "stage {} {}: rolled {drift:.2} m while idle", def.id, def.name);
            let p = car.pos;
            for _ in 0..150 {
                step(&mut car, &spec, Input { throttle: 1.0, ..Input::default() }, &w, 1.0 / 60.0);
            }
            let moved = (car.pos - p).length();
            // 2.5 s of full throttle with no steering (longer and some starts' first bend puts
            // the car in the trees): about 30–60 km/h depending on surface and slope.
            assert!(car.speed() * 3.6 > 20.0 && moved > 10.0, "stage {} {}: {:.1} km/h after 2.5 s, moved {moved:.1} m", def.id, def.name, car.speed() * 3.6);
        }
    }
}

#[cfg(test)]
mod idle_tests {
    use super::*;
    use crate::vehicle::{Car, Input, Spec, step};

    #[test]
    #[ignore]
    fn stage5_idle_then_throttle() {
        let spec = Spec::default();
        let def = &stage::stages()[4];
        let w = generate(def);
        let (p, h) = w.road.start();
        let mut car = Car::new(p + Vec3::Y * 0.9, h);
        for i in 0..(60 * 30) {
            let t = i as f32 / 60.0;
            let input = if t < 3.0 { Input { handbrake: true, ..Input::default() } } else if t < 12.0 { Input::default() } else { Input { throttle: 1.0, ..Input::default() } };
            step(&mut car, &spec, input, &w, 1.0 / 60.0);
            if i % 60 == 0 {
                let wh = &car.wheels;
                println!("t {t:4.1} up {:.2} pos {:.2?} v {:5.2} gear {} rpm {:5.0} spin {:.1?} contact {:?} surf {:?} slip {:.2?}",
                    car.up().y, car.pos - p, car.speed() * 3.6, car.gear, car.rpm, wh.iter().map(|w| w.spin).collect::<Vec<_>>(), wh.iter().map(|w| w.contact).collect::<Vec<_>>(), wh[0].surface, wh.iter().map(|w| w.slip_ratio).collect::<Vec<_>>());
            }
        }
    }
}
