//! Dev viewer for the open world's town buildings (`src/open/buildings.rs`), without the game:
//! the ground around town (from the road network's heights), the roads as dark strips and the
//! buildings, with a camera on a few poses.
//!
//! `memgate -- cargo run --example town -- --play [--pose overview|top|street|downtown|edge|farm|fly]
//! [--obstacles]` (`--obstacles` draws the wall cylinders in red). Offscreen screenshots through
//! the engine's play script (from the engine dir):
//!
//! ```sh
//! python3 -I scripts/play.py town-steps.json --manifest <pacenote>/Cargo.toml --example town --out <dir>
//! ```
//! with steps like `{"cmd": "town.pose", "params": {"name": "street"}}, {"sleep": 2}, {"shot": "street"}`.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use engine::cli::{arg_value, has_flag};
use engine::prelude::*;
use pacenote::open::{OpenWorld, SEED, buildings};

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
enum Pose {
    Overview,
    Top,
    Street,
    Downtown,
    Edge,
    Farm,
    Fly,
}

impl Pose {
    fn parse(s: &str) -> Option<Pose> {
        Some(match s {
            "overview" => Pose::Overview,
            "top" => Pose::Top,
            "street" => Pose::Street,
            "downtown" => Pose::Downtown,
            "edge" => Pose::Edge,
            "farm" => Pose::Farm,
            "fly" => Pose::Fly,
            _ => return None,
        })
    }
}

#[derive(Resource)]
struct Town {
    y: f32,
    farm: Vec2,
    t: f32,
}

fn main() {
    let info = GameInfo::new("pacenote-town", "Pacenote town").assets_fallback(concat!(env!("CARGO_MANIFEST_DIR"), "/assets"));
    let mut app = engine::app_with(info, EngineOptions { ui: false, ..EngineOptions::from_args() });
    app.insert_resource(SceneryRanges { view: 4000.0, ..default() });
    let pose = arg_value("pose").and_then(|p| Pose::parse(&p)).unwrap_or(Pose::Overview);
    #[cfg(feature = "dev")]
    {
        use engine::devtools::{AddControlCommand, params};
        app.add_control_command("town.pose", "Camera pose: overview|top|street|downtown|edge|farm|fly", params(&[("name", "string", "pose")]), |w, p| {
            let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("overview").to_string();
            let pose = Pose::parse(&name).ok_or_else(|| format!("unknown pose {name}"))?;
            w.insert_resource(pose);
            if let Some(mut t) = w.get_resource_mut::<Town>() {
                t.t = 0.0;
            }
            Ok(serde_json::json!({"pose": name}))
        });
        app.add_control_command("town.stats", "Town meshes: entities, triangles, vertices", params(&[]), |w, _| {
            let mut q = w.query_filtered::<&Mesh3d, With<buildings::BuildingMesh>>();
            let handles: Vec<_> = q.iter(w).map(|m| m.0.clone()).collect();
            Ok(serde_json::json!({"entities": handles.len()}))
        });
    }
    app.insert_resource(pose).insert_resource(ClearColor(Color::srgb(0.66, 0.74, 0.84))).add_systems(OnEnter(AppState::InGame), setup).add_systems(Update, camera.run_if(in_game)).run();
}

fn setup(world: &mut World) {
    let t0 = std::time::Instant::now();
    let ow = OpenWorld::generate(SEED);
    let net = ow.net.clone();
    let t1 = std::time::Instant::now();
    let layout = buildings::layout(&net);
    let t2 = std::time::Instant::now();
    let obs = buildings::walls(&layout);
    info!(
        "town: roads + terrain {:.0} ms, layout {:.0} ms ({} buildings, {} blocks, {} walls)",
        (t1 - t0).as_secs_f64() * 1e3,
        (t2 - t1).as_secs_f64() * 1e3,
        layout.buildings.len(),
        layout.blocks.len(),
        obs.len()
    );
    let farm = layout.blocks.iter().find(|b| b.kind == buildings::LotKind::Farm).map_or(Vec2::new(600.0, 0.0), |b| b.centre());
    world.insert_resource(Town { y: net.town_y, farm, t: 0.0 });
    if world.resource::<Headless>().0 {
        return;
    }
    let t3 = std::time::Instant::now();
    let heights = ow.heights.clone();
    let spawned = buildings::spawn(world, &layout, &|x, z| heights.height_at(x, z));
    info!("town: {} block meshes built in {:.0} ms", spawned.len(), t3.elapsed().as_secs_f64() * 1e3);

    // Ground: the world's heightfield (2 m) over 2.2 km, vertex-coloured grass.
    let (half, step) = (1100.0f32, 2.0f32);
    let n = (2.0 * half / step) as u32 + 1;
    let mut pos = Vec::new();
    let mut col = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let (x, z) = (-half + i as f32 * step, -half + j as f32 * step);
            pos.push([x, heights.height_at(x, z), z]);
            let k = 0.9 + 0.1 * pacenote::stage::noise(Vec2::new(x, z) / 60.0, 3);
            col.push(Color::srgb(0.30 * k, 0.42 * k, 0.22 * k).to_linear().to_f32_array());
        }
    }
    let mut idx = Vec::new();
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let a = j * n + i;
            idx.extend_from_slice(&[a, a + n, a + 1, a + 1, a + n, a + n + 1]);
        }
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    m.insert_indices(Indices::U32(idx));
    m.compute_smooth_normals();
    let ground = world.resource_mut::<Assets<Mesh>>().add(m);
    let gmat = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { perceptual_roughness: 0.95, ..default() });
    world.spawn((Name::new("ground"), Mesh3d(ground), MeshMaterial3d(gmat), KeepDetail, Scenery));

    // Roads: flat dark ribbons a little above the ground.
    let mut pos = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    for r in &net.roads {
        let road = &r.road;
        let base = pos.len() as u32;
        for (i, p) in road.pts.iter().enumerate() {
            let d = road.dir[i];
            let left = Vec2::new(d.y, -d.x) * road.width[i] * 0.5;
            pos.push([p.x + left.x, p.y + 0.06, p.z + left.y]);
            pos.push([p.x - left.x, p.y + 0.06, p.z - left.y]);
        }
        for i in 0..road.len() as u32 - 1 {
            let a = base + i * 2;
            idx.extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
    }
    let nrm = vec![[0.0, 1.0, 0.0]; pos.len()];
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    m.insert_indices(Indices::U32(idx));
    let roads_mesh = world.resource_mut::<Assets<Mesh>>().add(m);
    let rmat = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { base_color: Color::srgb(0.16, 0.16, 0.17), perceptual_roughness: 0.85, cull_mode: None, ..default() });
    world.spawn((Name::new("roads"), Mesh3d(roads_mesh), MeshMaterial3d(rmat), KeepDetail, Scenery));

    if has_flag("obstacles") {
        // Walls as red 2 m-tall slabs, their full thickness (the round ends left square).
        let mut pos = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        for w in &obs {
            let base = pos.len() as u32;
            let d = (w.b - w.a).normalize_or(Vec2::X);
            let (s, n) = (d * w.radius, d.perp() * w.radius);
            let y = net.town_y.min(w.top - 2.0);
            for p in [w.a - s - n, w.b + s - n, w.b + s + n, w.a - s + n] {
                pos.push([p.x, y, p.y]);
                pos.push([p.x, y + 2.0, p.y]);
            }
            for k in 0..4u32 {
                let (a, b) = (base + k * 2, base + ((k + 1) % 4) * 2);
                idx.extend_from_slice(&[a, b, a + 1, b, b + 1, a + 1]);
            }
        }
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        m.insert_indices(Indices::U32(idx));
        m.compute_smooth_normals();
        let h = world.resource_mut::<Assets<Mesh>>().add(m);
        let mat = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { base_color: Color::srgb(1.0, 0.1, 0.1), unlit: true, cull_mode: None, ..default() });
        world.spawn((Name::new("obstacles"), Mesh3d(h), MeshMaterial3d(mat), KeepDetail, Scenery));
    }

    let mut commands = world.commands();
    engine::gfx::sky_and_sun_with(
        &mut commands,
        engine::gfx::SunSetup { direction: Vec3::new(0.55, -0.6, 0.3), cascades: 4, first_cascade: 15.0, shadow_distance: 500.0, atmosphere: false, fallback_sky: Color::srgb(0.66, 0.74, 0.84), ambient_brightness: 900.0, ..default() },
    );
    world.flush();
    world.spawn((
        Name::new("camera"),
        MainCamera,
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { far: 5000.0, ..default() }),
        bevy::camera::Exposure { ev100: 13.0 },
        DistanceFog { color: Color::srgb(0.66, 0.74, 0.84), falloff: FogFalloff::from_visibility_colors(3000.0, Color::srgb(0.55, 0.62, 0.72), Color::srgb(1.0, 0.95, 0.85)), ..default() },
        Transform::from_xyz(-400.0, net.town_y + 250.0, -400.0).looking_at(Vec3::new(0.0, net.town_y, 0.0), Vec3::Y),
    ));
}

fn camera(time: Res<Time>, pose: Res<Pose>, town: Option<ResMut<Town>>, mut q: Query<&mut Transform, With<MainCamera>>) {
    let Some(mut town) = town else { return };
    let Ok(mut tf) = q.single_mut() else { return };
    town.t += time.delta_secs();
    let y = town.y;
    let v = |x: f32, h: f32, z: f32| Vec3::new(x, y + h, z);
    let (eye, look) = match *pose {
        Pose::Overview => (v(-430.0, 260.0, -430.0), v(0.0, 0.0, 0.0)),
        Pose::Top => (v(0.0, 900.0, 0.01), v(0.0, 0.0, 0.0)),
        // Driving east on the middle street from the spawn point.
        Pose::Street => (v(-300.0, 1.4, 2.6), v(-200.0, 2.5, 2.6)),
        // Up the north-south middle street towards the centre.
        Pose::Downtown => (v(-2.6, 1.6, -150.0), v(-2.6, 8.0, 0.0)),
        Pose::Edge => (v(-338.0, 7.0, -232.0), v(-220.0, 0.0, -160.0)),
        Pose::Farm => {
            let f = town.farm;
            let dir = f.normalize_or_zero();
            let eye = f - dir * 60.0 + dir.perp() * 30.0;
            (Vec3::new(eye.x, y + 25.0, eye.y), Vec3::new(f.x, y, f.y))
        }
        Pose::Fly => {
            let r = 380.0;
            let a = town.t * 25.0 / r;
            let p = Vec2::new(a.cos(), a.sin()) * r;
            let ahead = Vec2::new(-a.sin(), a.cos());
            (v(p.x, 70.0, p.y), v(p.x + ahead.x * 200.0 - p.x * 0.5, 0.0, p.y + ahead.y * 200.0 - p.y * 0.5))
        }
    };
    *tf = Transform::from_translation(eye).looking_at(look, Vec3::Y);
}
