//! Roadside props drawn one by one so they can react to the car (`engine::props`): marker posts,
//! kilometre boards and saplings that topple when hit, and shrubs that bend as the car goes
//! through and spring back.
//!
//! - **What breaks is the sim's call.** The car simulation knocks a prop down from sim state
//!   only (`vehicle::Give::Breaks`, the ids in `Car::broken`), so ghosts, replays and both
//!   players agree. This file only shows it: each new id in `Car::broken` sends a `Knock`, and
//!   the engine tips the prop over; debris, leaves and dust fly (visual only, not
//!   deterministic). A restart (fewer ids) stands everything back up.
//! - **The guide's car** (in-car view) isn't simulated on the guide's machine: the same
//!   footprint test runs on its drawn pose (any [`SwayPusher`] without a `DrivenCar`) and knocks
//!   what it drives into, with the sim's thresholds. Visual only; it matches the driver's screen
//!   except in near misses.
//! - **Body parts** (`art::car::DETACHABLE`: mirrors, wing, splitter, bumpers, diffuser) come
//!   off when the sim says so (`Damage::detached`, set by hard hits in their zone): the part's
//!   node is hidden and a copy of it tumbles away as debris. A restart puts them back. The
//!   guide's car drops the same parts, from the replicated damage (`guide::sync::NetDamage`).
//! - **Shrubs** bend from the car's drawn pose (`SwayPusher` on the car); leaves fly when it
//!   ploughs in. The drag is the sim's (`vehicle::Give::Brush`), the rustle `audio::body`'s.

use std::sync::Arc;

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use engine::props::{Breakable, Brushed, Debris, DebrisSpec, Fallen, Knock, PropGround, Sway, SwayPusher, Toppled, spawn_debris};
use engine::render_budget::Scenery;
use engine::vfx::Vfx;

use super::pbr;
use crate::drive::DrivenCar;
use crate::vehicle::Give;
use crate::world::{Prop, PropKind, StageWorld};

/// Small props fade out by this distance from the camera (m).
const PROP_END: f32 = 170.0;

/// The car's footprint for pushing plants and (on the guide) knocking props: half width, half
/// length (m), as `vehicle::Spec::size`.
pub fn car_pusher() -> SwayPusher {
    SwayPusher { half_extents: Vec2::new(0.9, 2.15) }
}

pub struct PropArtPlugin;

impl Plugin for PropArtPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "dev")]
        dev::register(app);
        app.add_plugins(engine::props::PropsPlugin)
            .add_systems(Update, (tag_car, knock_from_sim, knock_from_pose, respawn).chain())
            .add_systems(Update, (on_toppled, on_brushed, car_parts));
    }
}

/// Which prop an entity draws.
#[derive(Component, Clone, Copy, Debug)]
pub struct PropId(pub u32);

/// Meshes and materials, built once per stage.
#[derive(Clone)]
struct Kit {
    post: Vec<(Handle<Mesh>, Handle<StandardMaterial>)>,
    sign: Vec<(Handle<Mesh>, Handle<StandardMaterial>)>,
    /// Shrub variants: mesh, material, scale that gives a 1 m footprint radius.
    shrubs: Vec<(Handle<Mesh>, Handle<StandardMaterial>, f32)>,
    /// Sapling parts per prop come from the tree kit (`trees::near_parts`), with the full
    /// tree's height at scale 1.
    tree_height: f32,
    chips: [DebrisSpec; 3],
}

/// The stage's props and the entities drawing them.
#[derive(Resource)]
pub struct StageProps {
    list: Arc<Vec<Prop>>,
    /// Collision footprint per prop (radius, height, the closing speed that snaps it; 0 for
    /// shrubs) at the prop's scale.
    give: Vec<(f32, f32, f32)>,
    entities: Vec<Option<Entity>>,
    /// Ids knocked down on screen, in order.
    knocked: Vec<u32>,
    kit: Kit,
}

impl StageProps {
    /// How many are down on screen.
    pub fn knocked(&self) -> usize {
        self.knocked.len()
    }
}

/// Spawns the stage's props (visual runs only; after `trees::setup`, whose meshes saplings
/// share).
pub fn spawn(world: &mut World, s: &StageWorld) {
    let heights = s.heights.clone();
    world.insert_resource(PropGround::new(move |x, z| heights.height_at(x, z)));
    let kit = kit(world);
    let give = s
        .props
        .iter()
        .map(|p| {
            let (r, h) = p.kind.size();
            let speed = match p.kind.give(p.id, p.scale) {
                Give::Breaks { speed, .. } => speed,
                _ => 0.0,
            };
            (r * p.scale, h * p.scale, speed)
        })
        .collect();
    let mut props = StageProps { list: s.props.clone(), give, entities: vec![None; s.props.len()], knocked: Vec::new(), kit };
    for i in 0..props.list.len() {
        props.entities[i] = spawn_one(world, &props, i);
    }
    info!("props: {} ({} shrubs)", props.list.len(), props.list.iter().filter(|p| p.kind == PropKind::Shrub).count());
    world.insert_resource(props);
}

fn kit(world: &mut World) -> Kit {
    let dir = world.resource::<engine::prelude::AssetsDir>().0.clone();
    let wet = world.get_resource::<super::Wet>().is_some_and(|w| w.0);
    let mat = |world: &mut World, c: Color, rough: f32| world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { base_color: c, perceptual_roughness: if wet { rough * 0.6 } else { rough }, ..default() });
    let white = mat(world, Color::srgb(0.88, 0.88, 0.85), 0.6);
    let reflector = mat(world, Color::srgb(0.85, 0.22, 0.08), 0.35);
    let grey = mat(world, Color::srgb(0.42, 0.43, 0.45), 0.45);
    let board = mat(world, Color::srgb(0.95, 0.76, 0.12), 0.5);
    let black = mat(world, Color::srgb(0.05, 0.05, 0.05), 0.6);
    let wood = mat(world, Color::srgb(0.55, 0.42, 0.3), 0.9);
    let mut meshes = world.resource_mut::<Assets<Mesh>>();
    // Origin at the base: the engine tips props over about it.
    let post = vec![
        (meshes.add(Cuboid::new(0.1, 1.05, 0.1).mesh().build().translated_by(Vec3::Y * 0.525)), white.clone()),
        (meshes.add(Cuboid::new(0.106, 0.12, 0.106).mesh().build().translated_by(Vec3::Y * 0.86)), reflector),
    ];
    let sign = vec![
        (meshes.add(Cuboid::new(0.06, 2.0, 0.06).mesh().build().translated_by(Vec3::Y * 1.0)), grey),
        (meshes.add(Cuboid::new(0.62, 0.44, 0.02).mesh().build().translated_by(Vec3::new(0.0, 1.72, 0.035))), black),
        (meshes.add(Cuboid::new(0.56, 0.38, 0.02).mesh().build().translated_by(Vec3::new(0.0, 1.72, 0.045))), board.clone()),
    ];
    let chip = meshes.add(Cuboid::new(0.12, 0.03, 0.05));
    let chips = [
        DebrisSpec { mesh: chip.clone(), material: white, count: 6, size: (0.5, 1.0), speed: (1.0, 3.0), life: (1.5, 2.5) },
        DebrisSpec { mesh: chip.clone(), material: board, count: 5, size: (0.6, 1.2), speed: (1.0, 3.0), life: (1.5, 2.5) },
        DebrisSpec { mesh: chip, material: wood, count: 10, size: (0.6, 1.4), speed: (1.5, 4.0), life: (1.8, 3.0) },
    ];
    // Shrubs: the dressing's shrub models, foliage like `dressing::catalog`'s.
    let shrub_mat = pbr(world, "m_shrub", |m| {
        m.alpha_mode = AlphaMode::Mask(0.5);
        m.double_sided = true;
        m.cull_mode = None;
        m.diffuse_transmission = 0.25;
        m.perceptual_roughness = 0.8;
    });
    let mut shrubs = Vec::new();
    for name in ["shrub_a", "shrub_b"] {
        match super::meshfile::load(&dir, name) {
            Ok(mut mesh) => {
                // Normalise to a 1 m footprint radius (the collision's, at scale 1 × kind size).
                let unit = mesh_reach(&mut mesh).map_or(1.0, |r| 1.0 / r.max(0.05));
                let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
                shrubs.push((h, shrub_mat.clone(), unit));
            }
            Err(e) => error!("props: shrub mesh {name}: {e}"),
        }
    }
    let tree_height = tree_height(world).unwrap_or(11.0);
    Kit { post, sign, shrubs, tree_height, chips }
}

/// Horizontal reach of a mesh from its origin (m).
fn mesh_reach(mesh: &mut Mesh) -> Option<f32> {
    use bevy::camera::primitives::MeshAabb;
    let a = mesh.compute_aabb()?;
    let (lo, hi) = (Vec3::from(a.min()), Vec3::from(a.max()));
    Some(lo.x.abs().max(hi.x.abs()).max(lo.z.abs()).max(hi.z.abs()) * 0.8)
}

/// Height of the stage's trees at scale 1 (the tallest crown of the mix).
fn tree_height(world: &mut World) -> Option<f32> {
    use bevy::camera::primitives::MeshAabb;
    let kit = world.get_resource::<super::trees::TreeKit>()?;
    let parts = super::trees::near_parts(kit, 1);
    let meshes = world.resource::<Assets<Mesh>>();
    let mut top: f32 = 0.0;
    for (m, _) in parts {
        if let Some(a) = meshes.get(&m).and_then(|m| m.compute_aabb()) {
            top = top.max(a.max().y);
        }
    }
    (top > 1.0).then_some(top)
}

/// Spawns prop `i` (standing), or nothing when its meshes are missing.
fn spawn_one(world: &mut World, props: &StageProps, i: usize) -> Option<Entity> {
    let p = props.list[i];
    let kit = &props.kit;
    let (radius, height, _) = props.give[i];
    let range = VisibilityRange { start_margin: 0.0..0.0, end_margin: (PROP_END - 20.0)..PROP_END, use_aabb: false };
    let rot = Quat::from_rotation_y(p.yaw);
    let (parts, scale, tag): (Vec<Part>, f32, Tag) = match p.kind {
        PropKind::Post => (kit.post.clone(), p.scale, Tag::Breaks(Breakable { height, radius, fallen: Fallen::Stay })),
        PropKind::Sign => (kit.sign.clone(), p.scale, Tag::Breaks(Breakable { height, radius, fallen: Fallen::Stay })),
        PropKind::Sapling => {
            let tk = world.get_resource::<super::trees::TreeKit>()?;
            let parts = super::trees::near_parts(tk, p.seed);
            (parts, height / kit.tree_height, Tag::Breaks(Breakable { height, radius, fallen: Fallen::Stay }))
        }
        PropKind::Shrub => {
            let (m, mat, unit) = kit.shrubs.get((p.seed % kit.shrubs.len().max(1) as u64) as usize)?.clone();
            (vec![(m, mat)], radius * unit, Tag::Bends(Sway::new(radius, height)))
        }
    };
    let (first, rest) = parts.split_first()?;
    let tf = Transform { translation: p.pos, rotation: rot, scale: Vec3::splat(scale) };
    let mut e = world.spawn((Name::new("prop"), PropId(p.id), Scenery, tf, Mesh3d(first.0.clone()), MeshMaterial3d(first.1.clone()), range.clone()));
    match tag {
        Tag::Breaks(b) => e.insert(b),
        Tag::Bends(s) => e.insert(s),
    };
    let id = e.id();
    for (m, mat) in rest {
        world.spawn((Mesh3d(m.clone()), MeshMaterial3d(mat.clone()), Transform::IDENTITY, range.clone(), ChildOf(id)));
    }
    Some(id)
}

/// A mesh with its material.
type Part = (Handle<Mesh>, Handle<StandardMaterial>);

enum Tag {
    Breaks(Breakable),
    Bends(Sway),
}

/// The driven car (and the guide's, drawn from the network) pushes plants.
type NewCar = (Or<(Added<DrivenCar>, Added<crate::guide::seat::GuideCar>)>, Without<SwayPusher>);

fn tag_car(mut commands: Commands, cars: Query<Entity, NewCar>) {
    for e in &cars {
        commands.entity(e).insert(car_pusher());
    }
}

/// New ids in the sim's `Car::broken` → knock those props over.
fn knock_from_sim(cars: Query<&DrivenCar>, props: Option<ResMut<StageProps>>, mut knocks: MessageWriter<Knock>) {
    let (Ok(c), Some(mut props)) = (cars.single(), props) else { return };
    let broken = &c.car.broken;
    // Fewer than shown: a restart (`respawn` stands them back up).
    if broken.len() < props.knocked.len() || !broken.starts_with(&props.knocked) {
        return;
    }
    for &id in &broken[props.knocked.len()..] {
        let Some(p) = props.list.get(id as usize) else { continue };
        if let Some(Some(e)) = props.entities.get(id as usize) {
            knocks.write(Knock { entity: *e, velocity: c.car.vel, point: p.pos + Vec3::Y * 0.5 });
        }
        props.knocked.push(id);
    }
}

/// Cars that aren't simulated here (the guide's): knock what their drawn footprint drives into,
/// with the sim's thresholds.
fn knock_from_pose(
    time: Res<Time>,
    cars: Query<(Entity, &GlobalTransform, &SwayPusher), Without<DrivenCar>>,
    mut last: Local<Vec<(Entity, Vec3)>>,
    props: Option<ResMut<StageProps>>,
    mut knocks: MessageWriter<Knock>,
) {
    let Some(mut props) = props else { return };
    let dt = time.delta_secs();
    let mut seen = Vec::new();
    for (e, gt, pusher) in &cars {
        let pos = gt.translation();
        let prev = last.iter().find(|l| l.0 == e).map(|l| l.1);
        seen.push((e, pos));
        let Some(prev) = prev else { continue };
        if dt <= 0.0 {
            continue;
        }
        let vel = (pos - prev) / dt;
        // A jump (recover, teleport) isn't a hit.
        if vel.length() > 80.0 {
            continue;
        }
        let inv = gt.rotation().inverse();
        for i in 0..props.list.len() {
            let p = props.list[i];
            let (r, h, speed) = props.give[i];
            if p.kind == PropKind::Shrub || props.knocked.contains(&p.id) || (p.pos.xz() - pos.xz()).length_squared() > 36.0 || pos.y - 0.6 > p.pos.y + h {
                continue;
            }
            let local = inv * (p.pos - pos);
            let q = Vec2::new(local.x.clamp(-pusher.half_extents.x, pusher.half_extents.x), local.z.clamp(-pusher.half_extents.y, pusher.half_extents.y));
            let d = Vec2::new(local.x, local.z) - q;
            if d.length() >= r {
                continue;
            }
            let n = (p.pos.xz() - pos.xz()).normalize_or_zero();
            if vel.xz().dot(n) >= speed {
                if let Some(Some(pe)) = props.entities.get(i) {
                    knocks.write(Knock { entity: *pe, velocity: vel, point: p.pos + Vec3::Y * 0.5 });
                }
                props.knocked.push(p.id);
            }
        }
    }
    *last = seen;
}

/// After a restart (the sim's list is shorter than what's down), stands the props back up.
fn respawn(world: &mut World) {
    if world.get_resource::<StageProps>().is_none_or(|p| p.knocked.is_empty()) {
        return;
    }
    let mut q = world.query::<&DrivenCar>();
    let broken = match q.iter(world).next() {
        Some(c) => c.car.broken.clone(),
        // The guide: the driver's run restarting (back in the countdown) stands everything up.
        None if world.get_resource::<crate::guide::GuideFeed>().is_some_and(|f| f.run.countdown > 0.0) => Vec::new(),
        None => return,
    };
    let knocked = &world.resource::<StageProps>().knocked;
    if broken.len() >= knocked.len() && broken.starts_with(knocked) {
        return;
    }
    world.resource_scope(|world, mut props: Mut<StageProps>| {
        let keep: Vec<u32> = props.knocked.iter().copied().filter(|id| broken.contains(id)).collect();
        let reset: Vec<u32> = props.knocked.iter().copied().filter(|id| !broken.contains(id)).collect();
        for id in reset {
            let i = id as usize;
            if let Some(e) = props.entities[i].take()
                && let Ok(em) = world.get_entity_mut(e)
            {
                em.despawn();
            }
            props.entities[i] = spawn_one(world, &props, i);
        }
        props.knocked = keep;
    });
}

/// A prop starts falling: chips, dust and (saplings) leaves.
fn on_toppled(mut commands: Commands, mut events: MessageReader<Toppled>, ids: Query<&PropId>, props: Option<Res<StageProps>>, mut vfx: Vfx) {
    let Some(props) = props else {
        events.clear();
        return;
    };
    for t in events.read() {
        let Ok(id) = ids.get(t.entity) else { continue };
        let p = props.list[id.0 as usize];
        let at = Transform::from_translation(t.base);
        let chips = match p.kind {
            PropKind::Post => &props.kit.chips[0],
            PropKind::Sign => &props.kit.chips[1],
            _ => &props.kit.chips[2],
        };
        spawn_debris(&mut commands, chips, t.base + Vec3::Y * 0.4, t.velocity * 0.5, p.seed);
        vfx.play("snap_dust", at);
        vfx.play("splinters", at);
        // (The engine's leaves settle at the height they're played at: play them on the ground.)
        if p.kind == PropKind::Sapling {
            vfx.play("leaves", at);
        }
    }
}

/// The car ploughs into a shrub: leaves fly (from its base: they settle where they're played).
fn on_brushed(mut events: MessageReader<Brushed>, plants: Query<&Transform, With<Sway>>, mut vfx: Vfx) {
    for b in events.read() {
        if b.speed < 3.0 {
            continue;
        }
        let base = plants.get(b.plant).map_or(b.point, |t| t.translation);
        vfx.play("leaves", Transform::from_translation(base));
    }
}

#[cfg(feature = "dev")]
mod dev {
    use bevy::prelude::*;
    use engine::devtools::{AddControlCommand, params};
    use serde_json::{Value, json};

    use super::StageProps;
    use crate::drive::DrivenCar;
    use crate::world::{PropKind, StageWorld};

    pub fn register(app: &mut App) {
        app.add_control_command("props.stats", "Roadside props: how many of each, how many down", params(&[]), stats).add_control_command(
            "props.charge",
            "Aim the car at a prop and send it in (screenshots of things breaking)",
            params(&[
                ("kind", "string", "post | sign | sapling | shrub | tree (a solid trunk)"),
                ("nth", "integer", "which one along the stage (default 3)"),
                ("kmh", "number", "speed (default 60)"),
                ("dist", "number", "metres before it (default 14)"),
            ]),
            charge,
        );
    }

    fn stats(world: &mut World, _: Value) -> Result<Value, String> {
        let broken = world.query::<&DrivenCar>().iter(world).next().map_or(0, |c| c.car.broken.len());
        let p = world.get_resource::<StageProps>().ok_or("no props")?;
        let n = |k: PropKind| p.list.iter().filter(|x| x.kind == k).count();
        Ok(json!({"posts": n(PropKind::Post), "signs": n(PropKind::Sign), "saplings": n(PropKind::Sapling), "shrubs": n(PropKind::Shrub), "knocked": p.knocked.len(), "broken": broken}))
    }

    fn charge(world: &mut World, v: Value) -> Result<Value, String> {
        let name = v.get("kind").and_then(Value::as_str).unwrap_or("sapling");
        let kind = match name {
            "post" => PropKind::Post,
            "sign" => PropKind::Sign,
            "shrub" => PropKind::Shrub,
            _ => PropKind::Sapling,
        };
        let nth = v.get("nth").and_then(Value::as_u64).unwrap_or(3) as usize;
        let kmh = v.get("kmh").and_then(Value::as_f64).unwrap_or(60.0) as f32;
        let dist = v.get("dist").and_then(Value::as_f64).unwrap_or(14.0) as f32;
        let stage = world.get_resource::<StageWorld>().ok_or("no stage")?.clone();
        // Along the road, so `nth` counts from the start.
        // A solid trunk stands in as a pseudo-prop (id u32::MAX).
        let trees: Vec<crate::world::Prop> = stage
            .obstacle_list()
            .into_iter()
            .filter(|o| o.1 > 0.2 && o.1 < 0.5)
            .map(|o| crate::world::Prop { id: u32::MAX, kind, pos: Vec3::new(o.0.x, crate::vehicle::Ground::height(&stage, o.0.x, o.0.y), o.0.y), yaw: 0.0, scale: 1.0, seed: 0 })
            .collect();
        let pool: Vec<crate::world::Prop> = if name == "tree" { trees } else { stage.props.iter().copied().filter(|p| p.kind == kind).collect() };
        let mut found: Vec<(f32, crate::world::Prop)> = pool
            .iter()
            .filter(|p| stage.road.nearest(Vec2::new(p.pos.x, p.pos.z), 30.0).is_some_and(|r| r.lateral.abs() < 12.0))
            .filter_map(|p| stage.road.nearest(Vec2::new(p.pos.x, p.pos.z), 30.0).map(|r| (r.s, *p)))
            .filter(|(s, _)| *s > 60.0)
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (s, p) = *found.get(nth).ok_or("not that many")?;
        // Come in along the road, from the road side of it.
        let i = stage.road.station(s);
        let d = stage.road.dir[i];
        let fwd = Vec3::new(d.x, 0.0, d.y).normalize_or_zero();
        let start = p.pos - fwd * dist;
        let start = Vec3::new(start.x, crate::vehicle::Ground::height(&stage, start.x, start.z) + 0.6, start.z);
        let mut q = world.query::<&mut DrivenCar>();
        let mut c = q.single_mut(world).map_err(|e| e.to_string())?;
        let target = p.pos - start;
        crate::vehicle::recover(&mut c.car, start, target.x.atan2(target.z));
        let v = kmh / 3.6;
        c.car.vel = c.car.forward() * v;
        for w in &mut c.car.wheels {
            w.spin = v / 0.31;
        }
        c.prev = (c.car.pos, c.car.rot);
        Ok(json!({"id": p.id, "s": s, "prop": [p.pos.x, p.pos.y, p.pos.z], "car": [start.x, start.y, start.z]}))
    }
}

/// Body parts the sim knocked off: hide the part on the car, throw a copy of it (same meshes)
/// as debris. Fewer bits than shown (a restart): show them again.
#[allow(clippy::too_many_arguments)]
fn car_parts(
    mut commands: Commands,
    cars: Query<(Entity, &DrivenCar, &Children)>,
    guide_cars: Query<(Entity, &Children), With<crate::guide::seat::GuideCar>>,
    feed: Option<Res<crate::guide::GuideFeed>>,
    rigs: Query<&super::car::CarRig>,
    kids: Query<&Children>,
    meshes: Query<(&Mesh3d, &MeshMaterial3d<StandardMaterial>, &GlobalTransform)>,
    gts: Query<&GlobalTransform>,
    ground: Option<Res<PropGround>>,
    mut shown: Local<Vec<(Entity, u8)>>,
) {
    // (car, parts off, velocity, rotation, its children): the driven car from the sim, the
    // guide's from the network.
    let mut all: Vec<(Entity, u8, Vec3, Quat, &Children)> = cars.iter().map(|(e, c, k)| (e, c.car.damage.detached, c.car.vel, c.car.rot, k)).collect();
    if let Some(f) = feed.as_deref() {
        let (vel, rot) = f.car.map_or((Vec3::ZERO, Quat::IDENTITY), |c| (c.vel, c.rot));
        all.extend(guide_cars.iter().map(|(e, k)| (e, f.damage.detached, vel, rot, k)));
    }
    for (car_e, now, car_vel, car_rot, children) in all {
        let Some(rig) = children.iter().find_map(|k| rigs.get(k).ok()) else { continue };
        let i = match shown.iter().position(|s| s.0 == car_e) {
            Some(i) => i,
            None => {
                shown.push((car_e, 0));
                shown.len() - 1
            }
        };
        let was = shown[i].1;
        if now == was {
            continue;
        }
        for (bit, name) in crate::vehicle::PARTS.iter().enumerate() {
            let (on, before) = (now & (1 << bit) != 0, was & (1 << bit) != 0);
            let Some(part) = rig.part(name) else { continue };
            if on && !before {
                let Ok(pg) = gts.get(part) else { continue };
                // The copy: every mesh under the part, placed relative to the part.
                let inv = pg.affine().inverse();
                let mut stack = vec![part];
                let mut pieces = Vec::new();
                while let Some(e) = stack.pop() {
                    if let Ok((m, mat, g)) = meshes.get(e) {
                        pieces.push((m.clone(), mat.clone(), Transform::from_matrix((inv * g.affine()).into())));
                    }
                    if let Ok(k) = kids.get(e) {
                        stack.extend(k.iter());
                    }
                }
                let (_, rot, pos) = pg.to_scale_rotation_translation();
                let seed = ((bit as u64 + 1) * 0x9E37_79B9) ^ car_e.to_bits();
                let side = if seed.is_multiple_of(2) { 1.0 } else { -1.0 };
                let right = car_rot * Vec3::X;
                let floor = ground.as_ref().map_or(pos.y - 1.0, |g| g.height(pos.x, pos.z));
                let root = commands
                    .spawn((
                        Name::new(format!("{name} (off)")),
                        Debris {
                            vel: car_vel * 0.6 + Vec3::Y * 2.5 + right * side * 1.5,
                            spin: Vec3::new(3.0, 5.0 * side, 2.0),
                            age: 0.0,
                            life: 12.0,
                            size: 1.0,
                            floor,
                        },
                        engine::render_budget::Actor,
                        Transform::from_translation(pos).with_rotation(rot),
                        Visibility::default(),
                    ))
                    .id();
                for (m, mat, tf) in pieces {
                    commands.spawn((m, mat, tf, ChildOf(root)));
                }
                commands.entity(part).insert(Visibility::Hidden);
            } else if !on && before {
                commands.entity(part).insert(Visibility::Inherited);
            }
        }
        shown[i].1 = now;
    }
}
