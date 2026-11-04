//! Events in the open world: routes along the road network you drive to and start by stopping
//! in their marker. While one runs, its route is the stage's road (`StageWorld::road`), so the
//! clock, progress, finish, pace notes (the AI voice), records, ghosts and the results screen
//! all work as for a stage. Leaving (results' Free roam, or the pause menu) goes back to free
//! roam where the car is.
//!
//! An event's route is a chain of road pieces across junctions (`EventData::then`), so a stage
//! can start on village tarmac, cross the valley on gravel and climb a forest track. Kinds:
//! rallies (long, point to point), sprints (short), laps (once round a closed road).
//!
//! Rally and Sprint events run as a **Race** (5 rivals, a difficulty, a purse) or a **Time
//! Trial** (solo, ghost, records); laps are Time Trials only (DECISIONS R1). In the marker:
//! 1-4 pick the difficulty (Rookie .. Legend), Enter starts a Race, T a Time Trial, and
//! holding still starts the mode used last ([`Choice`]).

use std::sync::Arc;

use bevy::prelude::*;
use engine::flow::AppState;

use super::roads::{RoadKind, RoadNet};
use crate::career::racers::{Difficulty, EventMode};
use crate::drive::DrivenCar;
use crate::stage::{self, Road, STEP};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, bevy::reflect::Reflect)]
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
    /// The road of the network it starts on, and the station it starts at.
    pub road: usize,
    pub from: usize,
    /// Its pieces and where each starts along the route.
    pub legs: Vec<LegAt>,
}

/// A piece of a route: (road, from station, to station, index along the route where it starts).
pub type LegAt = (usize, usize, usize, usize);

/// A further piece of an event's route: stations `from` to `to` of road `road` (driven
/// backwards when `from > to`), starting where the piece before ends (at a junction).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, bevy::reflect::Reflect)]
pub struct Leg {
    pub road: u32,
    pub from: u32,
    pub to: u32,
}

/// How close (m) one piece's end must be to the next one's start.
pub const LEG_JOIN: f32 = 3.0;

/// A closed road driven once round from station `from`, ending where it started.
fn lap(road: &Road, from: usize) -> Road {
    let n = road.len();
    let idx: Vec<usize> = (0..=n).map(|k| (from + k) % n).collect();
    let pts: Vec<Vec2> = idx.iter().map(|&i| Vec2::new(road.pts[i].x, road.pts[i].z)).collect();
    let y: Vec<f32> = idx.iter().map(|&i| road.pts[i].y).collect();
    let mut r = Road::from_centreline(&pts, &y, idx.iter().map(|&i| road.width[i]).collect(), idx.iter().map(|&i| road.surface[i]).collect());
    r.bank = idx.iter().map(|&i| road.bank[i]).collect();
    r.dont_cut = idx.iter().map(|&i| road.dont_cut[i]).collect();
    r.crown = idx.iter().map(|&i| road.crown[i]).collect();
    r.rut = idx.iter().map(|&i| road.rut[i]).collect();
    r
}

/// Every event of the world, in a fixed order (their ids): the generated set
/// ([`specs`]) built on `net`.
pub fn list(net: &RoadNet) -> Vec<EventDef> {
    build(net, &specs(net))
}

/// An event as data (`open::data::WorldData`): a stretch of one road of the network.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bevy::reflect::Reflect)]
pub struct EventData {
    pub id: u32,
    pub name: String,
    pub kind: EventKind,
    /// Index into the network's roads.
    pub road: u32,
    /// First and last station (inclusive) of the first piece (backwards when `from > to`). A
    /// lap (`EventKind::Lap`) runs once round a closed road from `from` and ignores `to`.
    pub from: u32,
    pub to: u32,
    /// The pieces after the first, in order. (Absent in older level files: one piece.)
    #[serde(default)]
    #[reflect(default)]
    pub then: Vec<Leg>,
}

impl EventData {
    /// Every piece of the route, the first one included.
    pub fn legs(&self) -> Vec<Leg> {
        let mut v = vec![Leg { road: self.road, from: self.from, to: self.to }];
        v.extend_from_slice(&self.then);
        v
    }

    /// Moves every station of the route through `f(road, station)` (roads re-sampled).
    pub fn remap(&mut self, f: impl Fn(u32, u32) -> u32) {
        self.from = f(self.road, self.from);
        self.to = f(self.road, self.to);
        for l in &mut self.then {
            l.from = f(l.road, l.from);
            l.to = f(l.road, l.to);
        }
    }
}

/// Where roads `a` and `b` meet: (station of `a`, station of `b`).
fn meet(net: &RoadNet, a: usize, b: usize) -> Option<(usize, usize)> {
    net.junctions.iter().find_map(|j| {
        let x = j.roads.iter().find(|r| r.0 == a)?;
        let y = j.roads.iter().find(|r| r.0 == b)?;
        Some((x.1, y.1))
    })
}

/// The last station of road `k`.
fn end(net: &RoadNet, k: usize) -> Option<usize> {
    net.roads.get(k).map(|r| r.road.len() - 1)
}

/// The events the generator puts on `net` (as data): stages chained across junctions, 3-13 km,
/// several on mixed surfaces, each finishing onto a road that leads on (a flying finish, then a
/// liaison). The network's roads by build order (`roads::build`): 0 valley B-road, 1 hillclimb,
/// 2 basin gravel, 3 mountain pass, 4 forestry road, 5 logging track, 6 gully road, 7 southern
/// loop, 8 village lane. An event whose roads aren't there (another seed) is left out.
pub fn specs(net: &RoadNet) -> Vec<EventData> {
    let mut out = Vec::new();
    let mut id = FIRST_ID;
    // Off the junction at the start (the car waits on its own road).
    let lead = (16.0 / STEP) as usize;
    let mut push = |name: &str, kind: EventKind, legs: Option<Vec<(usize, usize, usize)>>| {
        let Some(legs) = legs else { return };
        let mut v = legs.iter().map(|&(road, from, to)| Leg { road: road as u32, from: from as u32, to: to as u32 });
        let first = v.next().unwrap();
        out.push(EventData { id, name: name.into(), kind, road: first.road, from: first.from, to: first.to, then: v.collect() });
        id += 1;
    };
    let kind = |k: usize| net.roads.get(k).map(|r| r.kind);
    let roles = [RoadKind::BRoad, RoadKind::BRoad, RoadKind::Gravel, RoadKind::Forest, RoadKind::Forest];
    if (0..roles.len()).any(|k| kind(k) != Some(roles[k])) {
        warn!("events: the network isn't the one the events are planned for; none");
        return out;
    }
    // Up the escarpment from the village on the forestry road, then west along the pass to
    // finish above the hillclimb.
    push(
        "Fernhollow escarpment",
        EventKind::Rally,
        (|| {
            let (top, _) = meet(net, 4, 3)?;
            let (_, at3) = meet(net, 4, 3)?;
            Some(vec![(4, lead, top), (3, at3, lead)])
        })(),
    );
    // The marathon: tarmac hillclimb, gravel over the plateau, down the basin road to the
    // gully junction.
    push(
        "Blackpine marathon",
        EventKind::Rally,
        (|| {
            let (gully, _) = meet(net, 2, 6)?;
            Some(vec![(1, lead, end(net, 1)?), (3, 0, end(net, 3)?), (2, end(net, 2)?, gully)])
        })(),
    );
    // Round the basin: east along the B-road from the service park, up the basin road, down
    // the gully road back to the park.
    push(
        "Quarry loop",
        EventKind::Sprint,
        (|| {
            let (park, _) = meet(net, 0, 6)?;
            let (up, _) = meet(net, 2, 6)?;
            let (_, gully_top) = meet(net, 2, 6)?;
            Some(vec![(0, park + lead, end(net, 0)?), (2, 0, up), (6, gully_top, lead)])
        })(),
    );
    // Over the river and back: the southern gravel loop between two stretches of B-road.
    push(
        "River crossings",
        EventKind::Sprint,
        (|| {
            let (a, _) = meet(net, 0, 7)?;
            let (b, last) = net.junctions.iter().find_map(|j| {
                let x = j.roads.iter().find(|r| r.0 == 0)?;
                let y = j.roads.iter().find(|r| r.0 == 7 && r.1 > 0)?;
                Some((x.1, y.1))
            })?;
            let back = (300.0 / STEP) as usize;
            Some(vec![(0, a.saturating_sub(back), a), (7, 0, last), (0, b, (b + back).min(end(net, 0)?))])
        })(),
    );
    // The tarmac hillclimb on its own.
    push("Hillclimb", EventKind::Sprint, end(net, 1).map(|e| vec![(1, lead, e - lead)]));
    out
}

/// The events described by `data`, built on `net`. Ones that don't fit the network (a road
/// that isn't there, a range off its end, pieces that don't meet, a lap on an open road) are
/// left out with a warning.
pub fn build(net: &RoadNet, data: &[EventData]) -> Vec<EventDef> {
    let mut out = Vec::new();
    for d in data {
        let Some(nr) = net.roads.get(d.road as usize) else {
            warn!("event {} \"{}\": no road {}", d.id, d.name, d.road);
            continue;
        };
        let (from, n) = (d.from as usize, nr.road.len());
        let (route, legs) = match d.kind {
            EventKind::Lap if nr.closed && from < n => (lap(&nr.road, from), vec![(d.road as usize, from, n, 0)]),
            EventKind::Lap => {
                warn!("event {} \"{}\": a lap needs a closed road", d.id, d.name);
                continue;
            }
            _ => match chain(net, &d.legs()) {
                Ok(r) => r,
                Err(e) => {
                    warn!("event {} \"{}\": {e}", d.id, d.name);
                    continue;
                }
            },
        };
        out.push(EventDef { id: d.id, name: d.name.clone(), kind: d.kind, route: Arc::new(route), road: d.road as usize, from, legs });
    }
    out
}

/// Checks a chain of pieces against the network: roads there, stations on them, each piece
/// at least two stations long, each starting where the one before ends.
pub fn check_legs(net_len: impl Fn(u32) -> Option<usize>, pos: impl Fn(u32, u32) -> Vec2, legs: &[Leg]) -> Result<(), String> {
    for (k, l) in legs.iter().enumerate() {
        let n = net_len(l.road).ok_or_else(|| format!("piece {k}: no road {}", l.road))?;
        if l.from.max(l.to) as usize >= n || l.from.abs_diff(l.to) < 2 {
            return Err(format!("piece {k}: stations {}..{} on road {} ({n} points)", l.from, l.to, l.road));
        }
        if k > 0 {
            let p = legs[k - 1];
            let gap = pos(p.road, p.to).distance(pos(l.road, l.from));
            if gap > LEG_JOIN {
                return Err(format!("piece {k} starts {gap:.1} m from where piece {} ends", k - 1));
            }
        }
    }
    Ok(())
}

/// The route along `legs`: each piece's stations in driving order (banking turned round on a
/// piece driven backwards), the turn from one piece onto the next rounded off. With where each
/// piece starts along it.
fn chain(net: &RoadNet, legs: &[Leg]) -> Result<(Road, Vec<LegAt>), String> {
    let pos = |r: u32, i: u32| {
        let p = net.roads[r as usize].road.pts[i as usize];
        Vec2::new(p.x, p.z)
    };
    check_legs(|r| net.roads.get(r as usize).map(|x| x.road.len()), pos, legs)?;
    let (mut pts, mut y, mut w, mut s, mut bank, mut crown, mut rut, mut dc) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut starts = Vec::new();
    let mut joints = Vec::new();
    for l in legs {
        let road = &net.roads[l.road as usize].road;
        let fwd = l.from <= l.to;
        let idx: Vec<usize> = if fwd { (l.from as usize..=l.to as usize).collect() } else { (l.to as usize..=l.from as usize).rev().collect() };
        if !pts.is_empty() {
            joints.push(pts.len());
        }
        starts.push((l.road as usize, l.from as usize, l.to as usize, pts.len()));
        for &i in &idx {
            let p = Vec2::new(road.pts[i].x, road.pts[i].z);
            // (The junction's own point once.)
            if pts.last().is_some_and(|q: &Vec2| q.distance(p) < 1.5) {
                continue;
            }
            pts.push(p);
            y.push(road.pts[i].y);
            w.push(road.width[i]);
            s.push(road.surface[i]);
            bank.push(if fwd { road.bank[i] } else { -road.bank[i] });
            crown.push(road.crown[i]);
            rut.push(road.rut[i]);
            dc.push(road.dont_cut[i]);
        }
    }
    if pts.len() < 2 {
        return Err("route too short".into());
    }
    // Round the turns at the joints (a junction turn is a square, not a 1 m kink).
    for &j in &joints {
        let (a, b) = (j.saturating_sub(6).max(1), (j + 6).min(pts.len() - 2));
        for _ in 0..10 {
            for i in a..=b {
                pts[i] = (pts[i - 1] + pts[i + 1]) * 0.5;
            }
        }
    }
    let mut r = Road::from_centreline(&pts, &y, w, s);
    r.bank = bank;
    r.crown = crown;
    r.rut = rut;
    r.dont_cut = dc;
    Ok((r, starts))
}

/// Where station `station` of road `road` is along event `e`'s route (m), if the route
/// covers it.
pub fn route_s(e: &EventDef, net: &RoadNet, road: usize, station: usize) -> Option<f32> {
    let n = net.roads.get(road)?.road.len();
    if station >= n {
        return None;
    }
    if e.kind == EventKind::Lap {
        if road != e.road {
            return None;
        }
        let k = (station + n - e.from) % n;
        return (k < e.route.len()).then_some(k as f32 * STEP);
    }
    for &(r, from, to, at) in &e.legs {
        if r != road || station < from.min(to) || station > from.max(to) {
            continue;
        }
        let k = at + station.abs_diff(from);
        return (k < e.route.len()).then_some(k as f32 * STEP);
    }
    None
}

/// The events of the world loaded now.
#[derive(Resource, Clone, Default)]
pub struct Events(pub Vec<EventDef>);

#[derive(Component)]
struct Marker;

/// Free-roam state near markers: which one the car is in, and for how long it's been still
/// (`meta::coach` shows the "stop here to start" prompt from it).
#[derive(Resource, Default)]
pub struct Approach {
    pub inside: Option<u32>,
    pub still: f32,
}

/// How the next event starts: Race (at a difficulty) or Time Trial. Starts as the mode used
/// last (kept in the career save); 1-4 / Enter / T in a marker change it. E's marker pill
/// shows it.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub race: bool,
    pub difficulty: Difficulty,
}

impl Default for Choice {
    fn default() -> Self {
        Choice { race: true, difficulty: Difficulty::Pro }
    }
}

impl Choice {
    /// The mode for an event of `kind` (laps are always Time Trials).
    pub fn mode_for(&self, kind: EventKind) -> EventMode {
        if self.race && kind != EventKind::Lap { EventMode::Race(self.difficulty) } else { EventMode::TimeTrial }
    }
}

/// The mode the running event was started in (`TimeTrial` in free roam).
pub fn running_mode(world: &World) -> EventMode {
    match world.get_resource::<crate::career::racers::RaceField>() {
        Some(f) => EventMode::Race(f.difficulty),
        None => EventMode::TimeTrial,
    }
}

pub struct EventsPlugin;

impl Plugin for EventsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Approach>()
            .init_resource::<Choice>()
            .add_systems(Update, (setup, approach).chain().run_if(resource_exists::<StageWorld>))
            .add_systems(Update, choice_from_save.run_if(resource_changed::<crate::career::save::CareerProgress>));
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
        .add_control_command(
            "events.start",
            "Start an open-world event now (as if stopped in its marker): a Time Trial, or a Race with `race`",
            params(&[("id", "integer", "event id (events.list)"), ("race", "string", "rookie | pro | ace | legend (omit: Time Trial)")]),
            |w, p: Value| {
                let id = p.get("id").and_then(Value::as_u64).ok_or("`id` is required")? as u32;
                let mode = match p.get("race").and_then(Value::as_str) {
                    None => super::EventMode::TimeTrial,
                    Some(d) => super::EventMode::Race(super::Difficulty::ALL.into_iter().find(|x| x.label().eq_ignore_ascii_case(d)).ok_or("race: rookie | pro | ace | legend")?),
                };
                let ok = super::start_as(w, id, mode);
                Ok(json!({"started": ok.then_some(id), "mode": format!("{mode:?}")}))
            },
        )
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

/// Drops the event list and the markers; `setup` makes them again from the stage's world
/// (after the editor changed it).
pub fn reset(world: &mut World) {
    let markers: Vec<Entity> = world.query_filtered::<Entity, With<Marker>>().iter(world).collect();
    for e in markers {
        world.entity_mut(e).despawn();
    }
    world.remove_resource::<Events>();
}

/// Where an event's boards stand: (position, facing yaw, kind) — a yellow board each side
/// 100 m before the finish, then at the line a red finish board and a marshal post.
pub fn board_spots(e: &EventDef) -> Vec<(Vec2, f32, Board)> {
    let r = &e.route;
    let mut out = Vec::new();
    let put = |out: &mut Vec<_>, s: f32, side: f32, gap: f32, kind: Board| {
        let i = r.station(s);
        let d = r.dir[i];
        let left = Vec2::new(d.y, -d.x);
        let p = Vec2::new(r.pts[i].x, r.pts[i].z) + left * side * (r.width[i] * 0.5 + gap);
        // Facing the cars coming.
        out.push((p, libm::atan2f(-d.x, -d.y), kind));
    };
    let end = r.length();
    for side in [1.0, -1.0] {
        put(&mut out, end - 100.0, side, 1.6, Board::Hundred);
        put(&mut out, end, side, 1.6, Board::Finish);
    }
    put(&mut out, end + 4.0, -1.0, 4.0, Board::Marshal);
    out
}

/// An event board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Board {
    /// Yellow: 100 m to the finish.
    Hundred,
    /// Red: the flying finish line.
    Finish,
    /// The timing marshal's post at the line.
    Marshal,
}

/// Puts up every event's boards (static, all the time: the course is marked out).
fn boards(world: &mut World, open: &super::OpenWorld, events: &[EventDef]) {
    let (post, board, hut) = {
        let mut m = world.resource_mut::<Assets<Mesh>>();
        (m.add(Cylinder::new(0.05, 1.6)), m.add(Cuboid::new(0.8, 0.8, 0.04)), m.add(Cuboid::new(1.4, 2.1, 1.4)))
    };
    let (white, yellow, red, orange) = {
        let mut m = world.resource_mut::<Assets<StandardMaterial>>();
        let mut mat = |c: Color| m.add(StandardMaterial { base_color: c, perceptual_roughness: 0.6, ..default() });
        (mat(Color::srgb(0.92, 0.92, 0.9)), mat(Color::srgb(0.98, 0.8, 0.05)), mat(Color::srgb(0.82, 0.08, 0.06)), mat(Color::srgb(0.95, 0.45, 0.08)))
    };
    for e in events {
        for (p, yaw, kind) in board_spots(e) {
            let y = open.ground_height(p.x, p.y);
            let rot = Quat::from_rotation_y(yaw);
            let base = Vec3::new(p.x, y, p.y);
            let put = |w: &mut World, mesh: &Handle<Mesh>, m: &Handle<StandardMaterial>, t: Transform| {
                w.spawn((Name::new(format!("event board: {}", e.name)), Marker, Mesh3d(mesh.clone()), MeshMaterial3d(m.clone()), t, DespawnOnExit(AppState::InGame)));
            };
            match kind {
                Board::Hundred | Board::Finish => {
                    put(world, &post, &white, Transform::from_translation(base + Vec3::Y * 0.8));
                    let m = if kind == Board::Hundred { &yellow } else { &red };
                    put(world, &board, m, Transform::from_translation(base + Vec3::Y * 1.9).with_rotation(rot));
                }
                Board::Marshal => put(world, &hut, &orange, Transform::from_translation(base + Vec3::Y * 1.05).with_rotation(rot)),
            }
        }
    }
}

/// Once per world: the event list and a marker at each start.
fn setup(world: &mut World) {
    let Some(stage) = world.get_resource::<StageWorld>() else { return };
    let Some(open) = stage.open.clone() else { return };
    if world.contains_resource::<Events>() {
        return;
    }
    let events = build(&open.net, &open.data.events);
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
                engine::level::GeneratedBy("pacenote::events"),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat),
                Transform::from_translation(p + Vec3::Y * 3.5),
                bevy::light::NotShadowCaster,
                DespawnOnExit(AppState::InGame),
            ));
        }
        boards(world, &open, &events);
    }
    info!("open world: {} events", events.len());
    world.insert_resource(Events(events));
}

/// In free roam: notes which marker the car is in; stopping in it starts the event.
fn approach(world: &mut World) {
    let Some(stage) = world.get_resource::<StageWorld>() else { return };
    if !stage.free_roam() || crate::loading::is_loading(world) {
        return;
    }
    let mut q = world.query::<&DrivenCar>();
    let Some((pos, speed)) = q.iter(world).next().map(|c| (c.car.pos, c.car.vel.length())) else { return };
    let Some(events) = world.get_resource::<Events>() else { return };
    let here = events.0.iter().find(|e| e.route.pts[0].distance(pos) < MARKER_R).map(|e| e.id);
    let dt = world.resource::<Time>().delta_secs();
    let mut a = world.resource_mut::<Approach>();
    match here {
        None => {
            a.inside = None;
            a.still = 0.0;
        }
        Some(id) => {
            // The HUD names the event and says to stop (`meta::coach`).
            if a.inside != Some(id) {
                a.inside = Some(id);
                a.still = 0.0;
                return;
            }
            a.still = if speed < 1.5 { a.still + dt } else { 0.0 };
            let still = a.still;
            // 1-4: difficulty; Enter: Race now; T: Time Trial now (when stopped).
            let keys = world.get_resource::<ButtonInput<KeyCode>>().map(|k| {
                let d = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4].iter().position(|c| k.just_pressed(*c));
                (d, k.just_pressed(KeyCode::Enter) || k.just_pressed(KeyCode::NumpadEnter), k.just_pressed(KeyCode::KeyT))
            });
            let mut go = still > 0.6;
            if let Some((d, race, trial)) = keys {
                let mut c = *world.resource::<Choice>();
                if let Some(d) = d {
                    c = Choice { race: true, difficulty: Difficulty::from_index(d as u8) };
                }
                if race || trial {
                    c.race = race;
                    go |= speed < 1.5;
                }
                if c != *world.resource::<Choice>() {
                    world.insert_resource(c);
                }
            }
            if go {
                world.resource_mut::<Approach>().still = f32::MIN;
                let kind = world.resource::<Events>().0.iter().find(|e| e.id == id).map_or(EventKind::Lap, |e| e.kind);
                let mode = world.resource::<Choice>().mode_for(kind);
                start_as(world, id, mode);
            }
        }
    }
}

/// The marker's default mode comes back from the career save.
fn choice_from_save(p: Res<crate::career::save::CareerProgress>, mut c: ResMut<Choice>, mut done: Local<bool>) {
    if *done {
        return;
    }
    *done = true;
    let want = Choice { race: p.last_mode != "trial", difficulty: Difficulty::from_index(p.last_difficulty) };
    if *c != want {
        *c = want;
    }
}

/// Starts event `id` as a Time Trial (see [`start_as`]).
pub fn start(world: &mut World, id: u32) {
    start_as(world, id, EventMode::TimeTrial);
}

/// Starts event `id` in `mode`: its route becomes the stage's road, the car goes to its start
/// line (a Race: its grid slot) with a countdown; `meta` picks up the new route (records,
/// ghost) next frame. A Race whose entry fee the wallet can't cover is refused (false).
pub fn start_as(world: &mut World, id: u32, mode: EventMode) -> bool {
    let Some(e) = world.get_resource::<Events>().and_then(|ev| ev.0.iter().find(|e| e.id == id).cloned()) else { return false };
    let mode = if e.kind == EventKind::Lap { EventMode::TimeTrial } else { mode };
    if let EventMode::Race(d) = mode {
        let fee = crate::career::economy::entry_fee(crate::career::economy::purse(e.route.length() / 1000.0), d);
        let have = world.get_resource::<crate::career::wallet::Wallet>().map_or(0, |w| w.balance);
        if have < fee {
            world.write_message(engine::ui::prelude::Toast::new("Not enough Notes").body(format!("The {} entry fee is {} N", d.label(), crate::career::notes(fee))));
            return false;
        }
    }
    crate::career::race_flow::end(world);
    if let Some(mut p) = world.get_resource_mut::<crate::career::save::CareerProgress>() {
        let (m, d) = match mode {
            EventMode::Race(d) => ("race", d.index()),
            EventMode::TimeTrial => ("trial", p.last_difficulty),
        };
        if e.kind != EventKind::Lap && (p.last_mode != m || p.last_difficulty != d) {
            p.last_mode = m.into();
            p.last_difficulty = d;
        }
    }
    {
        let mut s = world.resource_mut::<StageWorld>();
        let open = s.open.clone();
        let mut corners = stage::corners(&e.route);
        if let Some(o) = &open {
            o.data.apply_grades(&e, &o.net, &mut corners);
        }
        s.corners = Arc::new(corners);
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
    info!("event {} \"{}\": {:.1} km, {mode:?}", e.id, e.name, e.route.length() / 1000.0);
    tape(world, &e);
    if let EventMode::Race(d) = mode {
        crate::career::race_flow::begin(world, d);
    }
    world.remove_resource::<crate::meta::StageNow>();
    crate::meta::restart_stage(world);
    true
}

/// Red and white tape across a junction's branches the route doesn't take.
#[derive(Component)]
pub struct EventTape;

/// For each junction the route drives through: the arms it doesn't use, as (road, the arm's
/// mouth station). The way through, not a map arrow, tells the driver where to go.
pub fn closed_arms(net: &RoadNet, e: &EventDef) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for g in super::junctions::build(net) {
        // Arms the route uses: from each piece passing the junction, the side it comes from
        // and the side it leaves by.
        let mut used = Vec::new();
        for &(road, from, to, _) in &e.legs {
            for a in g.arms.iter().filter(|a| a.road == road) {
                let (lo, hi) = (from.min(to), from.max(to));
                if a.at < lo || a.at > hi {
                    continue;
                }
                let toward_from = (from as i64 - a.at as i64).signum() as i32;
                let toward_to = (to as i64 - a.at as i64).signum() as i32;
                for s in [toward_from, toward_to] {
                    if s == a.sign {
                        used.push((a.road, a.sign));
                    }
                }
            }
        }
        // Only where the route goes through (in on one arm, out on another).
        if used.len() < 2 {
            continue;
        }
        for a in &g.arms {
            if !used.contains(&(a.road, a.sign)) {
                out.push((a.road, a.mouth));
            }
        }
    }
    out
}

/// Spawns the tape for event `e` (replacing any left from before).
fn tape(world: &mut World, e: &EventDef) {
    let old: Vec<Entity> = world.query_filtered::<Entity, With<EventTape>>().iter(world).collect();
    for t in old {
        world.entity_mut(t).despawn();
    }
    if !crate::art::visual(world) {
        return;
    }
    let Some(open) = world.resource::<StageWorld>().open.clone() else { return };
    let arms = closed_arms(&open.net, e);
    let (post, strip) = {
        let mut m = world.resource_mut::<Assets<Mesh>>();
        (m.add(Cuboid::new(0.08, 1.1, 0.08)), m.add(Cuboid::new(0.5, 0.07, 0.01)))
    };
    let (red, white) = {
        let mut m = world.resource_mut::<Assets<StandardMaterial>>();
        let mat = |c: Color| StandardMaterial { base_color: c, perceptual_roughness: 0.6, ..default() };
        (m.add(mat(Color::srgb(0.85, 0.08, 0.06))), m.add(mat(Color::srgb(0.95, 0.95, 0.95))))
    };
    let ground = |p: Vec2| crate::vehicle::Ground::height(&*open, p.x, p.y);
    for (road, at) in arms {
        let r = &open.net.roads[road].road;
        let at = at.min(r.len() - 1);
        let c = Vec2::new(r.pts[at].x, r.pts[at].z);
        let across = r.dir[at].perp();
        let half = r.width[at] * 0.5 + 0.8;
        let yaw = Quat::from_rotation_y(-libm::atan2f(across.y, across.x));
        for s in [-half, half] {
            let p = c + across * s;
            world.spawn((EventTape, Mesh3d(post.clone()), MeshMaterial3d(white.clone()), Transform::from_xyz(p.x, ground(p) + 0.55, p.y), DespawnOnExit(AppState::InGame)));
        }
        let n = (2.0 * half / 0.5).ceil() as i32;
        for k in 0..n {
            let s = -half + (k as f32 + 0.5) * (2.0 * half / n as f32);
            let p = c + across * s;
            let y = ground(c - across * half).max(ground(c + across * half)) + 0.9;
            let m = if k % 2 == 0 { red.clone() } else { white.clone() };
            world.spawn((EventTape, Mesh3d(strip.clone()), MeshMaterial3d(m), Transform::from_xyz(p.x, y, p.y).with_rotation(yaw), DespawnOnExit(AppState::InGame)));
        }
    }
}

/// Back to free roam where the car is (after the results, or abandoning the event).
pub fn leave(world: &mut World) {
    crate::career::race_flow::end(world);
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
    let tape: Vec<Entity> = world.query_filtered::<Entity, With<EventTape>>().iter(world).collect();
    for t in tape {
        world.entity_mut(t).despawn();
    }
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
        assert_eq!(ev.len(), 5, "{:?}", ev.iter().map(|e| &e.name).collect::<Vec<_>>());
        assert!(ev.iter().filter(|e| e.kind == EventKind::Rally).count() >= 2);
        assert!(ev.iter().any(|e| e.name == "Hillclimb"), "a tarmac hillclimb");
        for (k, e) in ev.iter().enumerate() {
            assert_eq!(e.id, FIRST_ID + k as u32, "ids in order");
            eprintln!("{}: {:.1} km, {} pieces, surfaces {:?}", e.name, e.route.length() / 1000.0, e.legs.len(), {
                let mut v: Vec<_> = e.route.surface.clone();
                v.dedup();
                v
            });
            assert!(e.route.length() > 2_500.0, "{} is {} m", e.name, e.route.length());
            // A route is drivable: on the network (within a few metres where it turns from one
            // road onto the next), no gaps.
            let joints: Vec<usize> = e.legs.iter().skip(1).map(|l| l.3).collect();
            for (i, p) in e.route.pts.iter().enumerate().step_by(5) {
                let near = joints.iter().any(|&j| i.abs_diff(j) <= 8);
                assert!(net.nearest(Vec2::new(p.x, p.z), if near { 4.0 } else { 1.0 }).is_some(), "{} leaves the roads at {p}", e.name);
            }
            for w in e.route.pts.windows(2) {
                assert!(w[0].distance(w[1]) < 3.0, "{}: gap at {}", e.name, w[0]);
            }
        }
        // The long ones are long and mixed.
        let marathon = ev.iter().find(|e| e.name == "Blackpine marathon").unwrap();
        assert!(marathon.route.length() > 10_000.0, "{}", marathon.route.length());
        assert!(marathon.route.surface.contains(&crate::stage::Surface::Tarmac) && marathon.route.surface.contains(&crate::stage::Surface::Gravel));
        assert!(!stage::corners(&ev[0].route).is_empty(), "a rally has corners for the pace notes");
        // Deterministic.
        let again = list(&net);
        assert_eq!(ev.iter().map(|e| e.route.pts.clone()).collect::<Vec<_>>(), again.iter().map(|e| e.route.pts.clone()).collect::<Vec<_>>());
    }

    #[test]
    fn junctions_on_the_route_are_taped_off() {
        let net = super::super::roads::generate(super::super::SEED);
        let ev = list(&net);
        let quarry = ev.iter().find(|e| e.name == "Quarry loop").unwrap();
        let closed = closed_arms(&net, quarry);
        eprintln!("Quarry loop: {} branches taped", closed.len());
        // The B-road's east end turns up the basin road (no other way at that joint), the basin
        // road passes the gully junction's other branch... at least one branch somewhere.
        assert!(!closed.is_empty());
        // Never the route's own way.
        for &(road, at) in &closed {
            let p = net.roads[road].road.pts[at];
            let s = quarry.route.nearest(Vec2::new(p.x, p.z), 1.0);
            assert!(s.is_none(), "tape across the route at {p}");
        }
    }
}
