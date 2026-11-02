//! Traffic in 3D, without the rest of the game: flat ground, the roads as plain strips, the
//! traffic plugin's cars, and a camera over a town junction (or the ring with `--ring`). For
//! screenshots through the engine's play script:
//!
//! ```sh
//! cd /Users/himanshu/codes/game/engine
//! python3 -I scripts/play.py steps.json --manifest <pacenote>/Cargo.toml --example traffic_view --out <dir> [-- --ring]
//! ```
//! with steps like `[{"sleep": 4}, {"shot": "town"}]`. `--follow` chases a car.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use engine::prelude::*;
use engine::render_budget::KeepDetail;
use pacenote::open::traffic::{Player, Traffic, TrafficCar, TrafficConfig, TrafficFocus, TrafficPlugin};
use pacenote::open::{SEED, roads};

fn main() {
    let mut app = engine::app(GameInfo::new("pacenote-traffic-view", "Traffic view"));
    app.add_plugins(TrafficPlugin).add_systems(Startup, setup);
    if std::env::args().any(|a| a == "--follow") {
        app.add_systems(PostUpdate, chase.before(TransformSystems::Propagate));
    }
    app.run();
}

/// `--follow`: the camera chases the oldest car near the focus, behind and above it.
fn chase(traffic: Res<Traffic>, focus: Res<TrafficFocus>, cars: Query<(&TrafficCar, &Transform), Without<MainCamera>>, mut cam: Query<&mut Transform, With<MainCamera>>, mut target: Local<Option<u32>>) {
    let near = |id: u32| traffic.cars().any(|c| c.id == id && c.pos.distance(focus.0.pos) < 200.0);
    if target.is_none_or(|t| !near(t)) {
        *target = traffic.cars().filter(|c| c.pos.distance(focus.0.pos) < 120.0).map(|c| c.id).min();
    }
    let Some(t) = *target else { return };
    let Some((_, tf)) = cars.iter().find(|(c, _)| c.id == t) else { return };
    let Ok(mut cam) = cam.single_mut() else { return };
    let back = tf.back();
    let eye = tf.translation + back * 9.0 + Vec3::Y * 3.2 + tf.right() * 2.0;
    *cam = Transform::from_translation(eye).looking_at(tf.translation + Vec3::Y * 0.8 - back * 6.0, Vec3::Y);
}

fn setup(mut commands: Commands, meshes: Option<ResMut<Assets<Mesh>>>, mats: Option<ResMut<Assets<StandardMaterial>>>) {
    let ring = std::env::args().any(|a| a == "--ring");
    let net = roads::generate(SEED);
    // Where to look: the crossroads one block east and north of the centre, or where the east
    // country road meets the ring.
    let (focus, eye) = if ring {
        let hw = &net.roads.iter().find(|r| r.kind == roads::RoadKind::Highway).unwrap().road;
        let j = net.junctions.iter().filter(|j| j.pos.x > 1000.0).min_by(|a, b| a.pos.y.abs().total_cmp(&b.pos.y.abs())).unwrap().pos;
        let y = hw.nearest(j, 5.0).map_or(net.town_y, |at| hw.height_at(&at));
        (Vec3::new(j.x, y, j.y), Vec3::new(j.x - 30.0, y + 14.0, j.y + 34.0))
    } else {
        let c = Vec3::new(roads::BLOCK, net.town_y, roads::BLOCK);
        (c, c + Vec3::new(-14.0, 4.5, 17.0))
    };
    let mut traffic = Traffic::new(&net, TrafficConfig { cars: 90, radius: 280.0, despawn: 400.0, spawn_min: 150.0, ..default() });
    // Half a minute of driving first, so cars are spread over the lanes and in the junctions.
    let p = Player { pos: Vec3::new(focus.x, focus.y, focus.z) + Vec3::new(0.0, 0.0, 60.0), vel: Vec3::ZERO };
    for _ in 0..60 * 30 {
        traffic.step(1.0 / 60.0, Some(p));
    }
    commands.insert_resource(traffic);
    commands.insert_resource(TrafficFocus(p));

    commands.spawn((DirectionalLight { illuminance: 12_000.0, shadow_maps_enabled: true, ..default() }, Transform::from_xyz(0.3, 1.0, 0.45).looking_at(Vec3::ZERO, Vec3::Y)));
    commands.spawn((MainCamera, Camera3d::default(), Transform::from_translation(eye).looking_at(focus, Vec3::Y)));

    let (Some(mut meshes), Some(mut mats)) = (meshes, mats) else { return };
    let grass = mats.add(StandardMaterial { base_color: Color::srgb(0.25, 0.32, 0.2), perceptual_roughness: 0.95, ..default() });
    let tarmac = mats.add(StandardMaterial { base_color: Color::srgb(0.2, 0.2, 0.21), perceptual_roughness: 0.85, ..default() });
    commands.spawn((Mesh3d(meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(800.0)))), MeshMaterial3d(grass), Transform::from_xyz(focus.x, focus.y - 0.6, focus.z), KeepDetail));
    // Road strips near the view, a touch above their centreline height.
    for r in &net.roads {
        let pts = &r.road.pts;
        let mut pos = Vec::new();
        for (i, p) in pts.iter().enumerate() {
            let d = r.road.dir[i];
            let left = Vec2::new(d.y, -d.x) * (r.road.width[i] * 0.5);
            let y = p.y + 0.02;
            pos.push([p.x + left.x, y, p.z + left.y]);
            pos.push([p.x - left.x, y, p.z - left.y]);
        }
        if r.closed {
            pos.push(pos[0]);
            pos.push(pos[1]);
        }
        let n = pos.len() as u32 / 2;
        let mut idx = Vec::new();
        for i in 0..n - 1 {
            let (a, b, c, d) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
            idx.extend_from_slice(&[a, b, c, b, d, c]);
        }
        let normals = vec![[0.0, 1.0, 0.0]; pos.len()];
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_indices(Indices::U32(idx));
        commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(tarmac.clone()), KeepDetail));
    }
}
