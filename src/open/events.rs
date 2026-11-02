//! Events in the open world: routes along the road network you drive to and start by stopping
//! in their marker. While one runs, its route is the stage's road (`StageWorld::road`), so the
//! clock, progress, finish, pace notes (the AI voice), records, ghosts and the results screen
//! all work as for a stage. Leaving (results' Free roam, or the pause menu) goes back to free
//! roam where the car is.
//!
//! Kinds: a rally on each forest road (point to point, gravel), a sprint on each gravel country
//! road (town to ring) and a lap of the highway ring.

use std::sync::Arc;

use bevy::prelude::*;
use engine::flow::AppState;
use engine::ui::toast::Toast;

use super::roads::{RoadKind, RoadNet};
use crate::drive::DrivenCar;
use crate::stage::{self, Road, STEP, Surface};
use crate::world::StageWorld;

/// Event ids start here (stages are 1..=20, the open world is 0).
pub const FIRST_ID: u32 = 100;
/// Stop within this distance (m) of a marker to start its event.
const MARKER_R: f32 = 9.0;

/// The running event (on `StageWorld::event`).
#[derive(Clone, Debug, PartialEq)]
pub struct EventRef {
    pub id: u32,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Rally,
    Sprint,
    Lap,
}

impl EventKind {
    pub fn label(self) -> &'static str {
        match self {
            EventKind::Rally => "Rally",
            EventKind::Sprint => "Sprint",
            EventKind::Lap => "Lap",
        }
    }
}

/// One event: its route starts where the marker is.
#[derive(Clone, Debug)]
pub struct EventDef {
    pub id: u32,
    pub name: String,
    pub kind: EventKind,
    pub route: Arc<Road>,
}

const RALLY_NAMES: [&str; 4] = ["Fernhollow", "Blackpine", "Mossgate", "Cold Spring"];

/// The part of `road` from station `a` to `b` (inclusive) as its own road.
fn slice(road: &Road, a: usize, b: usize) -> Road {
    let pts: Vec<Vec2> = road.pts[a..=b].iter().map(|p| Vec2::new(p.x, p.z)).collect();
    let y: Vec<f32> = road.pts[a..=b].iter().map(|p| p.y).collect();
    Road::from_centreline(&pts, &y, road.width[a..=b].to_vec(), road.surface[a..=b].to_vec())
}

/// A closed road driven once round from station `from`, ending where it started.
fn lap(road: &Road, from: usize) -> Road {
    let n = road.len();
    let idx: Vec<usize> = (0..=n).map(|k| (from + k) % n).collect();
    let pts: Vec<Vec2> = idx.iter().map(|&i| Vec2::new(road.pts[i].x, road.pts[i].z)).collect();
    let y: Vec<f32> = idx.iter().map(|&i| road.pts[i].y).collect();
    Road::from_centreline(&pts, &y, idx.iter().map(|&i| road.width[i]).collect(), idx.iter().map(|&i| road.surface[i]).collect())
}

/// Every event of the world, in a fixed order (their ids).
pub fn list(net: &RoadNet) -> Vec<EventDef> {
    let mut out = Vec::new();
    let mut id = FIRST_ID;
    let mut push = |name: String, kind: EventKind, route: Road| {
        out.push(EventDef { id, name, kind, route: Arc::new(route) });
        id += 1;
    };
    // Rallies: each forest road from just off the ring to its end.
    for (k, r) in net.roads.iter().filter(|r| r.kind == RoadKind::Forest).enumerate() {
        let from = (16.0 / STEP) as usize;
        push(format!("{} rally", RALLY_NAMES[k % RALLY_NAMES.len()]), EventKind::Rally, slice(&r.road, from, r.road.len() - 1));
    }
    // Sprints: the gravel country roads, town to ring.
    for r in net.roads.iter().filter(|r| r.kind == RoadKind::Country && r.road.surface.iter().filter(|s| **s != Surface::Tarmac).count() * 2 > r.road.len()) {
        let p = r.road.pts[0];
        let dir = if p.x.abs() > p.z.abs() {
            if p.x > 0.0 { "East" } else { "West" }
        } else if p.z > 0.0 {
            "South"
        } else {
            "North"
        };
        let from = (16.0 / STEP) as usize;
        push(format!("{dir} road sprint"), EventKind::Sprint, slice(&r.road, from, r.road.len() - 1 - from));
    }
    // A lap of the ring from where the east country road joins it.
    if let Some(ring) = net.roads.iter().find(|r| r.kind == RoadKind::Highway) {
        let from = ring.road.nearest(Vec2::new(super::roads::RING_R, 0.0), 400.0).map_or(0, |p| p.i);
        // Start a little past the junction.
        let from = (from + (40.0 / STEP) as usize) % ring.road.len();
        push("Ring lap".into(), EventKind::Lap, lap(&ring.road, from));
    }
    out
}

/// The events of the world loaded now.
#[derive(Resource, Clone, Default)]
pub struct Events(pub Vec<EventDef>);

#[derive(Component)]
struct Marker;

/// Free-roam state near markers: which one the car is in, and for how long it's been still.
#[derive(Resource, Default)]
struct Approach {
    inside: Option<u32>,
    still: f32,
}

pub struct EventsPlugin;

impl Plugin for EventsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Approach>().add_systems(Update, (setup, approach).chain().run_if(resource_exists::<StageWorld>));
        #[cfg(feature = "dev")]
        dev::register(app);
    }
}

#[cfg(feature = "dev")]
mod dev {
    use bevy::prelude::*;
    use engine::devtools::{AddControlCommand, params};
    use serde_json::{Value, json};

    pub fn register(app: &mut App) {
        app.add_control_command("events.list", "Open-world events: id, name, kind, length, start", params(&[]), |w, _| {
            let ev = w.get_resource::<super::Events>().ok_or("no open world loaded")?;
            Ok(json!(ev.0.iter().map(|e| json!({"id": e.id, "name": e.name, "kind": e.kind.label(), "km": e.route.length() / 1000.0, "start": [e.route.pts[0].x, e.route.pts[0].z]})).collect::<Vec<_>>()))
        })
        .add_control_command("events.start", "Start an open-world event now (as if stopped in its marker)", params(&[("id", "integer", "event id (events.list)")]), |w, p: Value| {
            let id = p.get("id").and_then(Value::as_u64).ok_or("`id` is required")? as u32;
            super::start(w, id);
            Ok(json!({"started": id}))
        })
        .add_control_command("open.tour", "Measuring drive: town street, east country road, 1 km of ring as the road (pair with autopilot.set {on: true})", params(&[]), |w, _| {
            let len = super::super::tour::start(w)?;
            Ok(json!({"tour_m": len}))
        })
        .add_control_command("events.leave", "Back to free roam", params(&[]), |w, _| {
            super::leave(w);
            Ok(json!({"free_roam": true}))
        });
    }
}

/// Once per world: the event list and a marker at each start.
fn setup(world: &mut World) {
    let Some(stage) = world.get_resource::<StageWorld>() else { return };
    let Some(open) = stage.open.clone() else { return };
    if world.contains_resource::<Events>() {
        return;
    }
    let events = list(&open.net);
    if crate::art::visual(world) {
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Cylinder::new(MARKER_R * 0.55, 7.0));
        for e in &events {
            let color = match e.kind {
                EventKind::Rally => Color::srgba(1.0, 0.62, 0.15, 0.32),
                EventKind::Sprint => Color::srgba(0.35, 0.75, 1.0, 0.32),
                EventKind::Lap => Color::srgba(0.75, 0.45, 1.0, 0.32),
            };
            let mat = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
                base_color: color,
                emissive: LinearRgba::from(color) * 2.5,
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                cull_mode: None,
                ..default()
            });
            let p = e.route.pts[0];
            world.spawn((
                Name::new(format!("event marker: {}", e.name)),
                Marker,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat),
                Transform::from_translation(p + Vec3::Y * 3.5),
                bevy::light::NotShadowCaster,
                DespawnOnExit(AppState::InGame),
            ));
        }
    }
    info!("open world: {} events", events.len());
    world.insert_resource(Events(events));
}

/// In free roam: entering a marker says what it is; stopping in it starts the event.
fn approach(world: &mut World) {
    let Some(stage) = world.get_resource::<StageWorld>() else { return };
    if !stage.free_roam() || crate::loading::is_loading(world) {
        return;
    }
    let mut q = world.query::<&DrivenCar>();
    let Some((pos, speed)) = q.iter(world).next().map(|c| (c.car.pos, c.car.vel.length())) else { return };
    let Some(events) = world.get_resource::<Events>() else { return };
    let here = events.0.iter().find(|e| e.route.pts[0].distance(pos) < MARKER_R).map(|e| (e.id, e.name.clone(), e.kind, e.route.length()));
    let dt = world.resource::<Time>().delta_secs();
    let mut a = world.resource_mut::<Approach>();
    match here {
        None => {
            a.inside = None;
            a.still = 0.0;
        }
        Some((id, name, kind, len)) => {
            if a.inside != Some(id) {
                a.inside = Some(id);
                a.still = 0.0;
                world.write_message(Toast::new(name).body(format!("{} \u{b7} {:.1} km \u{b7} stop here to start", kind.label(), len / 1000.0)).secs(4.0));
                return;
            }
            a.still = if speed < 1.5 { a.still + dt } else { 0.0 };
            if a.still > 0.6 {
                a.still = f32::MIN;
                start(world, id);
            }
        }
    }
}

/// Starts event `id`: its route becomes the stage's road, the car goes to its start line with
/// a countdown; `meta` picks up the new route (records, ghost) next frame.
pub fn start(world: &mut World, id: u32) {
    let Some(e) = world.get_resource::<Events>().and_then(|ev| ev.0.iter().find(|e| e.id == id).cloned()) else { return };
    {
        let mut s = world.resource_mut::<StageWorld>();
        s.corners = Arc::new(stage::corners(&e.route));
        s.crests = Arc::new(stage::crests(&e.route));
        s.road = e.route.clone();
        s.event = Some(EventRef { id: e.id, name: e.name.clone() });
    }
    // Events run without traffic (clean racing; ghosts replay exactly).
    world.remove_resource::<super::traffic::Traffic>();
    if let Some(t) = world.get_resource::<super::traffic::TrafficSolids>()
        && let Ok(mut v) = t.0.write()
    {
        v.clear();
    }
    info!("event {} \"{}\": {:.1} km", e.id, e.name, e.route.length() / 1000.0);
    world.remove_resource::<crate::meta::StageNow>();
    crate::meta::restart_stage(world);
}

/// Back to free roam where the car is (after the results, or abandoning the event).
pub fn leave(world: &mut World) {
    let Some(mut s) = world.get_resource_mut::<StageWorld>() else { return };
    if s.event.is_none() {
        return;
    }
    let Some(open) = s.open.clone() else { return };
    let (start, _) = open.spawn();
    let street = open.net.nearest(Vec2::new(start.x, start.z), 20.0).map_or(0, |n| n.road);
    s.road = Arc::new(open.net.roads[street].road.clone());
    s.corners = Arc::new(Vec::new());
    s.crests = Arc::new(Vec::new());
    s.event = None;
    world.remove_resource::<crate::meta::StageNow>();
    world.insert_resource(crate::drive::StageRun::default());
    world.resource_mut::<crate::meta::Finished>().0 = false;
    let ghosts: Vec<Entity> = world.query_filtered::<Entity, With<crate::meta::GhostCar>>().iter(world).collect();
    for g in ghosts {
        world.entity_mut(g).despawn();
    }
    world.insert_resource(engine::flow::PauseAllowed(true));
    // Traffic back.
    if crate::art::visual(world) {
        world.insert_resource(super::traffic::Traffic::new(&open.net, super::traffic::TrafficConfig::default()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_from_the_network() {
        let net = super::super::roads::generate(super::super::SEED);
        let ev = list(&net);
        assert!(ev.iter().filter(|e| e.kind == EventKind::Rally).count() >= 3);
        assert_eq!(ev.iter().filter(|e| e.kind == EventKind::Lap).count(), 1);
        for (k, e) in ev.iter().enumerate() {
            assert_eq!(e.id, FIRST_ID + k as u32, "ids in order");
            assert!(e.route.length() > 800.0, "{} is {} m", e.name, e.route.length());
            // A route is drivable: corners found, all on the network.
            for p in e.route.pts.iter().step_by(25) {
                assert!(net.nearest(Vec2::new(p.x, p.z), 1.0).is_some(), "{} leaves the roads at {p}", e.name);
            }
        }
        let lap = ev.iter().find(|e| e.kind == EventKind::Lap).unwrap();
        assert!(lap.route.pts[0].distance(*lap.route.pts.last().unwrap()) < 0.1, "a lap ends where it starts");
        assert!(!stage::corners(&ev[0].route).is_empty(), "a rally has corners for the pace notes");
        // Deterministic.
        let again = list(&net);
        assert_eq!(ev.iter().map(|e| e.route.pts.clone()).collect::<Vec<_>>(), again.iter().map(|e| e.route.pts.clone()).collect::<Vec<_>>());
    }
}
