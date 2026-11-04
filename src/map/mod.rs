//! The open world's map: a minimap in the driver HUD during free roam (`mini.rs`) and a
//! full-screen map (`screen.rs`, `M` / d-pad up, or Map on the pause menu) with every event
//! start, where you can pick one and get a route line to it along the roads.
//!
//! Both draw one image of the region, painted once per world on a worker thread
//! (`paint.rs`: terrain shading, contours, river, buildings, roads by kind). Markers, the
//! player's arrow and the route are a few UI nodes over it, moved each frame; nothing is
//! respawned per frame.
//!
//! The design keeps the driver map-less inside an event (pace notes are the map), so the
//! minimap hides during an event and the full map won't open then.

pub mod paint;
pub mod path;
mod mini;
mod screen;

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use engine::actions::{ActionDef, ActionState, AddAction, Binding};
use engine::ui::screens::{ScreenRequest, Screens};
use engine::ui::toast::Toast;

use crate::drive::DrivenCar;
use crate::open::OpenWorld;
use crate::open::events::{EventDef, EventKind, Events};
use crate::world::StageWorld;

/// Id of the full-map screen.
pub const SCREEN: &str = "map";

/// Marker colour by event kind: the glowing columns' colours (`open::events::setup`).
pub fn kind_color(kind: EventKind) -> Color {
    match kind {
        EventKind::Rally => Color::srgb(1.0, 0.62, 0.15),
        EventKind::Sprint => Color::srgb(0.35, 0.75, 1.0),
        EventKind::Lap => Color::srgb(0.75, 0.45, 1.0),
    }
}

/// Route line colour (the theme's rally-plate yellow would clash with B-roads: cyan-white).
pub const ROUTE: Color = Color::srgb(0.55, 0.95, 1.0);

/// The painted map and the icons drawn over it.
#[derive(Resource, Clone)]
pub struct MapArt {
    /// The region; `None` until painted.
    pub map: Option<Handle<Image>>,
    pub arrow: Handle<Image>,
    pub dot: Handle<Image>,
    pub ring: Handle<Image>,
    /// Which world it shows (the road network's address).
    world: usize,
}

/// The road graph for routes, for the world loaded now.
#[derive(Resource, Clone)]
pub struct MapGraph {
    pub graph: Arc<path::Graph>,
    world: usize,
}

#[derive(Resource)]
struct PaintTask(Task<Vec<u8>>, usize);

/// The event the player picked on the map, and the way there along the roads.
#[derive(Resource, Default, Clone, Debug)]
pub struct Waypoint {
    pub event: Option<u32>,
    /// The way to the event's start (x, z), simplified; empty when none. The car is on (or
    /// near) its first segment, so the line is drawn from the car to `path[1]` and on.
    pub path: Vec<Vec2>,
    /// The way's length from the car (m): to `path[1]`, then along the rest.
    pub length: f32,
    /// Length of `path[1..]`.
    rest: f32,
    /// Bumped whenever `path` changes (the line's nodes are rebuilt then).
    pub version: u32,
    /// Seconds since the route was last worked out.
    age: f32,
}

impl Waypoint {
    /// Picks `id` (or clears with `None`); the route is worked out next frame.
    pub fn set(&mut self, id: Option<u32>) {
        self.event = id;
        self.path.clear();
        self.length = 0.0;
        self.rest = 0.0;
        self.version += 1;
        self.age = f32::INFINITY;
    }
}

fn world_key(o: &OpenWorld) -> usize {
    Arc::as_ptr(&o.net) as usize
}

/// The open world, in free roam or an event (`None` on the old stages).
pub(crate) fn open_world(stage: &StageWorld) -> Option<&Arc<OpenWorld>> {
    stage.open.as_ref()
}

/// The car's position (x, z) and heading (unit, x, z).
pub(crate) fn car_pose(c: &DrivenCar) -> (Vec2, Vec2) {
    let f = c.car.forward();
    let h = Vec2::new(f.x, f.z);
    (Vec2::new(c.car.pos.x, c.car.pos.z), if h.length_squared() > 1e-6 { h.normalize() } else { Vec2::Y })
}

/// Event start (x, z).
pub(crate) fn start_of(e: &EventDef) -> Vec2 {
    let p = e.route.pts[0];
    Vec2::new(p.x, p.z)
}

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Waypoint>()
            .add_action(ActionDef::button("world_map").label("Map").group("Driving").bind(Binding::key(KeyCode::KeyM)).bind(Binding::pad(GamepadButton::DPadUp)))
            .add_systems(Update, (prepare, open_key, route).chain().run_if(resource_exists::<StageWorld>))
            .add_systems(OnExit(engine::flow::AppState::InGame), forget);
        mini::build(app);
        screen::build(app);
    }
}

/// Leaving the game drops the waypoint (a new world may load next time).
fn forget(mut wp: ResMut<Waypoint>) {
    wp.set(None);
}

/// Once per world: the road graph, and (when drawing) the map painted on a worker.
fn prepare(world: &mut World) {
    let Some(open) = world.get_resource::<StageWorld>().and_then(|s| open_world(s).cloned()) else { return };
    let key = world_key(&open);
    if world.get_resource::<MapGraph>().is_none_or(|g| g.world != key) {
        world.insert_resource(MapGraph { graph: Arc::new(path::Graph::build(&open.net)), world: key });
        world.resource_mut::<Waypoint>().age = f32::INFINITY;
    }
    if !crate::art::visual(world) {
        return;
    }
    if world.get_resource::<MapArt>().is_none() {
        let mut images = world.resource_mut::<Assets<Image>>();
        let icon = |images: &mut Assets<Image>, n: u32, px: Vec<u8>| images.add(image(n, px, 1));
        let arrow = icon(&mut images, 64, paint::arrow(64));
        let dot = icon(&mut images, 32, paint::dot(32));
        let ring = icon(&mut images, 48, paint::ring(48));
        world.insert_resource(MapArt { map: None, arrow, dot, ring, world: 0 });
    }
    let art_world = world.resource::<MapArt>().world;
    let painting = world.get_resource::<PaintTask>().map(|t| t.1);
    if art_world != key && painting != Some(key) {
        let o = open.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let t = std::time::Instant::now();
            let hf = o.heights.clone();
            let height = move |x: f32, z: f32| hf.height_at(x, z);
            let src = paint::Source { net: &o.net, height: &height, buildings: &o.buildings.buildings, river: &o.net.land.river };
            let px = with_mips(paint::paint(&src, paint::SIZE), paint::SIZE);
            info!("map: painted {}² in {:.2} s", paint::SIZE, t.elapsed().as_secs_f32());
            px
        });
        world.insert_resource(PaintTask(task, key));
        return;
    }
    let done = world.get_resource_mut::<PaintTask>().and_then(|mut t| block_on(poll_once(&mut t.0)).map(|px| (px, t.1)));
    if let Some((px, k)) = done {
        world.remove_resource::<PaintTask>();
        let levels = paint::SIZE.ilog2() + 1;
        let handle = world.resource_mut::<Assets<Image>>().add(image(paint::SIZE, px, levels));
        let mut art = world.resource_mut::<MapArt>();
        art.map = Some(handle);
        art.world = k;
    }
}

/// An sRGB image `n`² with `levels` mip levels (the data holds every level, largest first).
fn image(n: u32, data: Vec<u8>, levels: u32) -> Image {
    // (`Image::new` checks the data is one level; the mips go in after.)
    let base = (n * n * 4) as usize;
    let mut img = Image::new(Extent3d { width: n, height: n, depth_or_array_layers: 1 }, TextureDimension::D2, data[..base].to_vec(), TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
    img.texture_descriptor.mip_level_count = levels;
    img.data = Some(data);
    img.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
        mipmap_filter: bevy::image::ImageFilterMode::Linear,
        ..bevy::image::ImageSamplerDescriptor::linear()
    });
    img
}

/// `px` (RGBA8, `n`²) followed by its box-filtered mip chain down to 1×1.
fn with_mips(mut px: Vec<u8>, n: u32) -> Vec<u8> {
    let (mut start, mut size) = (0usize, n as usize);
    while size > 1 {
        let half = size / 2;
        let mut next = Vec::with_capacity(half * half * 4);
        for y in 0..half {
            for x in 0..half {
                for k in 0..4 {
                    let at = |xx: usize, yy: usize| px[start + (yy * size + xx) * 4 + k] as u32;
                    let s = at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1);
                    next.push(((s + 2) / 4) as u8);
                }
            }
        }
        start = px.len();
        px.extend_from_slice(&next);
        size = half;
    }
    px
}

/// `M` / d-pad up while driving: opens the full map (free roam only).
fn open_key(world: &mut World) {
    if !world.get_resource::<ActionState>().is_some_and(|a| a.just_pressed("world_map")) {
        return;
    }
    let Some(stage) = world.get_resource::<StageWorld>() else { return };
    if open_world(stage).is_none() || crate::loading::is_loading(world) {
        return;
    }
    if !stage.free_roam() {
        world.write_message(Toast::new("No map during an event").body("Follow the pace notes. Leave the event from the pause menu.").secs(3.0));
        return;
    }
    if world.get_resource::<Screens>().is_some_and(|s| s.stack.is_empty()) {
        world.write_message(ScreenRequest::Push(SCREEN));
    }
}

/// Keeps the route to the waypoint current: worked out again when the car has left it (or
/// every few seconds while off it); cleared on arrival or when an event starts.
fn route(time: Res<Time<Real>>, stage: Res<StageWorld>, events: Option<Res<Events>>, graph: Option<Res<MapGraph>>, cars: Query<&DrivenCar>, mut wp: ResMut<Waypoint>) {
    let Some(id) = wp.event else { return };
    let (Some(open), Some(events), Some(graph)) = (open_world(&stage), events, graph) else { return };
    if !stage.free_roam() {
        wp.set(None);
        return;
    }
    let Some(e) = events.0.iter().find(|e| e.id == id) else {
        wp.set(None);
        return;
    };
    let Ok(car) = cars.single() else { return };
    let (pos, _) = car_pose(car);
    if pos.distance(start_of(e)) < 20.0 {
        wp.set(None);
        return;
    }
    wp.age += time.delta_secs();
    // How far the car is from the line it's following.
    let off = wp.path.windows(2).take(40).map(|w| seg_distance(pos, w[0], w[1])).fold(f32::INFINITY, f32::min);
    if wp.path.len() < 2 || (off > 25.0 && wp.age > 1.5) {
        let Some(from) = path::nearest(&open.net, pos) else { return };
        wp.age = 0.0;
        let mut pts = match graph.graph.route(&open.net, from, (e.road, e.from)) {
            Some((pts, _)) => path::simplify(&pts, 1.5),
            None => vec![pos, start_of(e)],
        };
        if pts.len() < 2 {
            pts = vec![pos, start_of(e)];
        }
        wp.rest = path::length(&pts[1..]);
        wp.path = pts;
        wp.version += 1;
    } else if let Some(k) = (0..wp.path.len().saturating_sub(1).min(40)).find(|&k| seg_distance(pos, wp.path[k], wp.path[k + 1]) <= off + 0.01)
        && k > 0
    {
        // Past the first points: drop them (the line's first segment is the one the car is on).
        wp.path.drain(..k);
        wp.rest = path::length(&wp.path[1..]);
        wp.version += 1;
    }
    wp.length = pos.distance(wp.path[1]) + wp.rest;
}

fn seg_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// The event to show on the HUD: the waypoint's, else the nearest start (straight line).
pub(crate) fn guide_target<'a>(events: &'a Events, wp: &Waypoint, pos: Vec2) -> Option<(&'a EventDef, f32, bool)> {
    if let Some(e) = wp.event.and_then(|id| events.0.iter().find(|e| e.id == id)) {
        let d = if wp.path.len() < 2 { pos.distance(start_of(e)) } else { wp.length };
        return Some((e, d, true));
    }
    events.0.iter().map(|e| (e, pos.distance(start_of(e)))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(e, d)| (e, d, false))
}

/// Spawns a polyline (world x, z) as UI nodes under `parent`, a square node showing the
/// whole map image: a thin rotated bar per segment and a round joint at each bend, placed in
/// percent so they follow the image as it pans, zooms and turns.
pub(crate) fn spawn_line(parent: &mut ChildSpawnerCommands, pts: &[Vec2], thick: f32, color: Color) {
    if pts.len() < 2 {
        return;
    }
    let uv: Vec<Vec2> = pts.iter().map(|p| paint::uv(*p)).collect();
    for w in uv.windows(2) {
        let (a, b) = (w[0], w[1]);
        let d = b - a;
        let len = d.length();
        if len < 1e-6 {
            continue;
        }
        let mid = (a + b) * 0.5;
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent((mid.x - len * 0.5) * 100.0),
                top: percent(mid.y * 100.0),
                width: percent(len * 100.0),
                height: px(thick),
                margin: UiRect::top(px(-thick * 0.5)),
                ..default()
            },
            UiTransform { rotation: Rot2::radians(d.y.atan2(d.x)), ..default() },
            BackgroundColor(color),
            Pickable::IGNORE,
        ));
    }
    for p in &uv[1..uv.len() - 1] {
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(p.x * 100.0),
                top: percent(p.y * 100.0),
                width: px(thick),
                height: px(thick),
                margin: UiRect { left: px(-thick * 0.5), top: px(-thick * 0.5), ..default() },
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(color),
            Pickable::IGNORE,
        ));
    }
}

/// "850 M" / "1.2 KM".
pub(crate) fn distance_label(m: f32, mph: bool) -> String {
    if mph {
        let mi = m / 1609.34;
        if mi < 0.2 { format!("{:.0} YD", m * 1.0936) } else { format!("{mi:.1} MI") }
    } else if m < 1000.0 {
        format!("{:.0} M", (m / 10.0).round() * 10.0)
    } else {
        format!("{:.1} KM", m / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mips_halve_down_to_one_pixel() {
        let px = with_mips(vec![200; 8 * 8 * 4], 8);
        assert_eq!(px.len(), (64 + 16 + 4 + 1) * 4);
        assert!(px.iter().all(|&v| v == 200));
    }

    #[test]
    fn distances_read_naturally() {
        assert_eq!(distance_label(847.0, false), "850 M");
        assert_eq!(distance_label(1234.0, false), "1.2 KM");
        assert_eq!(distance_label(3218.0, true), "2.0 MI");
    }
}
