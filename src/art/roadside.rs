//! Rally road dressing in the open world (docs/RALLY_MAP.md phase 6): what a driver reads the
//! road by, with no map.
//!
//! - **Bridge parapets**: stone walls along each side of a bridge's deck (solid: `open::profile::walls`).
//! - **Culvert headwalls** in the inside ditch of tight corners (solid; the note says "don't cut").
//! - **Snow poles** on the edge of the high roads, so the line shows against the sky over a crest.
//! - **Chevron boards** on the outside of deceptive corners: a tight one after a fast run or
//!   straight over a crest.
//! - **Timber piles** at the road edge of each clear-cut (solid).
//!
//! Event boards (100 m to the finish, the flying finish) are put up with the events
//! (`open::events`). [`items`] is pure (tested); [`spawn`] draws them, one shared mesh and
//! material per kind.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::open::OpenWorld;
use crate::open::profile::{DITCH_W, PARAPET, SHOULDER};
use crate::open::roads::{RoadKind, RoadNet};
use crate::stage::{self, Road, STEP};

/// Snow poles above this road height (m).
pub const SNOW_LINE: f32 = 405.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A stone parapet piece (from `pos` along `yaw`, `len` m).
    Parapet,
    Culvert,
    SnowPole,
    Chevron,
    TimberPile,
}

/// One piece of dressing: where (on the ground), facing (yaw about Y, 0 = +Z), and its length
/// along that direction where it has one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    pub kind: Kind,
    pub pos: Vec2,
    pub yaw: f32,
    pub len: f32,
    /// Which way a chevron points (+1 = left).
    pub turn: f32,
}

fn left(road: &Road, i: usize) -> Vec2 {
    -road.dir[i].perp()
}

fn at(road: &Road, i: usize, lat: f32) -> Vec2 {
    Vec2::new(road.pts[i].x, road.pts[i].z) + left(road, i) * lat
}

/// Facing along the road at station `i` (towards higher stations).
fn along(road: &Road, i: usize) -> f32 {
    libm::atan2f(road.dir[i].x, road.dir[i].y)
}

/// Every piece of dressing for the network, in a fixed order.
pub fn items(net: &RoadNet, clearcuts: &[pcg_core::Rect]) -> Vec<Item> {
    let mut out = Vec::new();
    // (Nothing standing in a junction, where it would be on the other road.)
    let clear = |p: Vec2| net.junctions.iter().all(|j| j.pos.distance(p) > 22.0);
    for nr in &net.roads {
        let road = &nr.road;
        let n = road.len();
        // Parapets: along each edge of each bridge, 2 stations a piece.
        let mut i = 0;
        while i + 2 < nr.bridge.len() {
            if nr.bridge[i] && nr.bridge[i + 2] {
                for side in [1.0f32, -1.0] {
                    let a = at(road, i, side * (road.width[i] * 0.5 + PARAPET.1));
                    let b = at(road, i + 2, side * (road.width[i + 2] * 0.5 + PARAPET.1));
                    let d = b - a;
                    out.push(Item { kind: Kind::Parapet, pos: (a + b) * 0.5, yaw: libm::atan2f(d.x, d.y), len: d.length() + 0.1, turn: 0.0 });
                }
            }
            i += 2;
        }
        for &(i, lat) in &nr.culverts {
            out.push(Item { kind: Kind::Culvert, pos: at(road, i, lat), yaw: along(road, i), len: 1.0, turn: 0.0 });
        }
        if !matches!(nr.kind, RoadKind::Gravel | RoadKind::Forest) {
            continue;
        }
        // Snow poles: every 25 m on the high roads, on the side away from the ditch (the drop).
        let mut i = 0;
        while i < n {
            if road.pts[i].y > SNOW_LINE && !nr.bridge.get(i).copied().unwrap_or(false) {
                let ditch = nr.ditch.get(i).copied().unwrap_or([0.0; 2]);
                let side = if ditch[0] > ditch[1] { -1.0 } else { 1.0 };
                let lat = side * (road.width[i] * 0.5 + SHOULDER + 0.3);
                let p = at(road, i, lat);
                if clear(p) {
                    out.push(Item { kind: Kind::SnowPole, pos: p, yaw: along(road, i), len: 0.0, turn: 0.0 });
                }
            }
            i += (25.0 / STEP) as usize;
        }
        // Chevrons: on the outside of a tight corner (hairpin to 2) that comes after a fast run
        // (nothing tighter than a 4 in the 150 m before) or over a crest.
        let corners = stage::corners(road);
        for (k, c) in corners.iter().enumerate() {
            if c.grade > 2 {
                continue;
            }
            let fast = corners[..k].iter().rev().take_while(|p| p.end > c.start - 150.0).all(|p| p.grade >= 4);
            if !(fast || c.crest || c.crest_into) {
                continue;
            }
            let (a, b) = (road.station(c.start), road.station(c.end));
            let apex = (a..=b).max_by(|&x, &y| road.curv[x].abs().total_cmp(&road.curv[y].abs())).unwrap_or(a);
            let turn = road.curv[apex].signum();
            // The outside of a left turn is its right (lateral −).
            let side = -turn;
            for k in [-6i32, 0, 6] {
                let j = (apex as i32 + k).clamp(0, n as i32 - 1) as usize;
                let lat = side * (road.width[j] * 0.5 + SHOULDER + DITCH_W + 0.8);
                let p = at(road, j, lat);
                if clear(p) {
                    // Facing the cars coming (towards lower stations).
                    out.push(Item { kind: Kind::Chevron, pos: p, yaw: along(road, j) + core::f32::consts::PI, len: 0.0, turn });
                }
            }
        }
    }
    // Timber piles: where each clear-cut meets its road, along the road edge.
    for c in clearcuts {
        let Some(np) = net.nearest(c.center, c.half.x + 40.0) else { continue };
        let road = &net.roads[np.road].road;
        let i = np.at.i;
        let side = np.at.lateral.signum();
        let lat = side * (road.width[i] * 0.5 + SHOULDER + DITCH_W + 2.2);
        out.push(Item { kind: Kind::TimberPile, pos: at(road, i, lat), yaw: along(road, i), len: 7.0, turn: 0.0 });
    }
    out
}

/// What the car hits of these that `open::profile::walls` doesn't have: timber piles.
pub fn walls(items: &[Item], ground: &dyn Fn(Vec2) -> f32) -> Vec<crate::vehicle::Wall> {
    items
        .iter()
        .filter(|it| it.kind == Kind::TimberPile)
        .map(|it| {
            let d = Vec2::new(libm::sinf(it.yaw), libm::cosf(it.yaw)) * (it.len * 0.5 - 0.6);
            crate::vehicle::Wall::new(it.pos - d, it.pos + d, 0.6, ground(it.pos) + 1.2)
        })
        .collect()
}

/// Dressing, for despawning when the roads are rebuilt.
#[derive(Component)]
pub struct Roadside;

/// Draws [`items`] for the open world (nothing in headless runs).
pub fn spawn(world: &mut World, open: &OpenWorld) {
    if !super::visual(world) {
        return;
    }
    let old: Vec<Entity> = world.query_filtered::<Entity, With<Roadside>>().iter(world).collect();
    for e in old {
        world.entity_mut(e).despawn();
    }
    let list = items(&open.net, &crate::open::ecology::clearcuts(&open.net, crate::open::ecology::CLEARCUTS));
    let (parapet, culvert, pipe, pole, band, post, board, stroke, log) = {
        let mut m = world.resource_mut::<Assets<Mesh>>();
        (
            m.add(Cuboid::new(PARAPET.1 * 2.0, PARAPET.0 + 0.3, 1.0)),
            m.add(Cuboid::new(1.6, 0.6, 0.25)),
            m.add(Cylinder::new(0.22, 0.3)),
            m.add(Cylinder::new(0.035, 2.0)),
            m.add(Cylinder::new(0.04, 0.35)),
            m.add(Cylinder::new(0.04, 1.2)),
            m.add(Cuboid::new(0.7, 0.55, 0.03)),
            m.add(Cuboid::new(0.1, 0.36, 0.035)),
            m.add(Cylinder::new(0.2, 6.5)),
        )
    };
    let (stone, concrete, dark, white, red, orange, wood) = {
        let mut m = world.resource_mut::<Assets<StandardMaterial>>();
        let mut mat = |c: Color, rough: f32| m.add(StandardMaterial { base_color: c, perceptual_roughness: rough, ..default() });
        (
            mat(Color::srgb(0.52, 0.5, 0.46), 0.95),
            mat(Color::srgb(0.62, 0.62, 0.6), 0.9),
            mat(Color::srgb(0.05, 0.05, 0.05), 1.0),
            mat(Color::srgb(0.92, 0.92, 0.9), 0.6),
            mat(Color::srgb(0.8, 0.08, 0.06), 0.6),
            mat(Color::srgb(0.95, 0.42, 0.05), 0.6),
            mat(Color::srgb(0.48, 0.33, 0.2), 0.95),
        )
    };
    let ground = |p: Vec2| open.ground_height(p.x, p.y);
    let put = |world: &mut World, mesh: &Handle<Mesh>, m: &Handle<StandardMaterial>, t: Transform| {
        world.spawn((Roadside, Mesh3d(mesh.clone()), MeshMaterial3d(m.clone()), t, engine::prelude::Scenery, DespawnOnExit(engine::flow::AppState::InGame), crate::open::live::LiveRoadMesh));
    };
    for it in &list {
        let y = ground(it.pos);
        let rot = Quat::from_rotation_y(it.yaw);
        let base = Vec3::new(it.pos.x, y, it.pos.y);
        match it.kind {
            Kind::Parapet => {
                let open_y = open.net.nearest(it.pos, 4.0).map_or(y, |n| open.net.roads[n.road].road.height_at(&n.at));
                put(world, &parapet, &stone, Transform::from_translation(Vec3::new(it.pos.x, open_y + PARAPET.0 * 0.5 - 0.1, it.pos.y)).with_rotation(rot).with_scale(Vec3::new(1.0, 1.0, it.len)));
            }
            Kind::Culvert => {
                put(world, &culvert, &concrete, Transform::from_translation(base + Vec3::Y * 0.15).with_rotation(rot * Quat::from_rotation_y(core::f32::consts::FRAC_PI_2)));
                put(world, &pipe, &dark, Transform::from_translation(base + Vec3::Y * 0.1).with_rotation(rot * Quat::from_rotation_x(core::f32::consts::FRAC_PI_2)));
            }
            Kind::SnowPole => {
                put(world, &pole, &white, Transform::from_translation(base + Vec3::Y * 1.0));
                put(world, &band, &orange, Transform::from_translation(base + Vec3::Y * 1.8));
            }
            Kind::Chevron => {
                put(world, &post, &white, Transform::from_translation(base + Vec3::Y * 0.6));
                let c = base + Vec3::Y * 1.35;
                put(world, &board, &red, Transform::from_translation(c).with_rotation(rot));
                // A white ">" (or "<") pointing the way the road goes.
                let face = rot * Vec3::Z * 0.02;
                for (dy, tilt) in [(0.09f32, 0.6f32), (-0.09, -0.6)] {
                    let t = Transform::from_translation(c + face + Vec3::Y * dy).with_rotation(rot * Quat::from_rotation_z(tilt * it.turn));
                    world.spawn((Roadside, Mesh3d(stroke.clone()), MeshMaterial3d(white.clone()), t, engine::prelude::Scenery, NotShadowCaster, DespawnOnExit(engine::flow::AppState::InGame), crate::open::live::LiveRoadMesh));
                }
            }
            Kind::TimberPile => {
                let across = rot * Vec3::X;
                for (row, count) in [(0, 4), (1, 3), (2, 2)] {
                    for k in 0..count {
                        let off = (k as f32 - (count - 1) as f32 * 0.5) * 0.42;
                        let p = base + across * off + Vec3::Y * (0.2 + row as f32 * 0.36);
                        put(world, &log, &wood, Transform::from_translation(p).with_rotation(rot * Quat::from_rotation_x(core::f32::consts::FRAC_PI_2)));
                    }
                }
            }
        }
    }
    info!("roadside: {} pieces", list.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rally_dressing() {
        let net = crate::open::roads::generate(crate::open::SEED);
        let cuts = crate::open::ecology::clearcuts(&net, crate::open::ecology::CLEARCUTS);
        let list = items(&net, &cuts);
        let count = |k: Kind| list.iter().filter(|i| i.kind == k).count();
        eprintln!(
            "parapets {}, culverts {}, snow poles {}, chevrons {}, timber piles {}",
            count(Kind::Parapet),
            count(Kind::Culvert),
            count(Kind::SnowPole),
            count(Kind::Chevron),
            count(Kind::TimberPile)
        );
        assert!(count(Kind::Parapet) >= 8);
        assert!(count(Kind::Culvert) >= 10);
        assert!(count(Kind::SnowPole) >= 20);
        assert!(count(Kind::Chevron) >= 6);
        assert_eq!(count(Kind::TimberPile), cuts.len());
        // None of it on a road surface.
        for it in &list {
            if let Some(n) = net.nearest(it.pos, 30.0) {
                let half = net.roads[n.road].road.half_width(&n.at);
                assert!(n.at.lateral.abs() > half - 0.05, "{:?} on the road at {}", it.kind, it.pos);
            }
        }
    }
}
