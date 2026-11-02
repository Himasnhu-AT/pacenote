//! Gameplay points the editor places and edits (docs/EDITOR.md "Gameplay points"): the home
//! spawn, each event's start, hand-set pace-note grades. Reflect components on entities with a
//! `Transform`, spawned from the world data ([`spawn`]) and read back into it ([`gather`]),
//! snapping to the roads: drag an event start along its road and its route starts there.
//!
//! [`PLACEABLES`] is what the editor's Place palette lists. The engine's `Placeable`
//! registration isn't on the engine yet; [`PlaceablePlugin`] registers the reflected types and
//! keeps the table for when it lands.

use bevy::prelude::*;

use super::data::{GradeOverride, Home, WorldData};
use super::roads::RoadNet;

/// Where the car starts in free roam and respawns after a fatal crash. Faces its local −Z.
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct HomePoint;

/// The start of an event (where its marker stands). Its route runs along the road it's on.
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct EventStart {
    pub event: u32,
}

/// A hand-set pace-note grade for the corner at this point of the road (1 hairpin … 6 kink).
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct GradeNote {
    pub grade: u8,
    /// The road it's on (kept while the note stays within reach of it, so a note by a
    /// junction doesn't jump to the crossing road); `None` for a new note: the nearest road.
    pub road: Option<u32>,
}

impl Default for GradeNote {
    fn default() -> Self {
        GradeNote { grade: 3, road: None }
    }
}

/// Set by [`spawn`]: the world holds the editable points, so [`gather`] may read them back.
/// Grades [`spawn`] couldn't place are kept here and saved unchanged.
#[derive(Resource, Default)]
pub struct PointsSpawned {
    pub unplaced_grades: Vec<GradeOverride>,
}

/// An entry of the editor's Place palette (mirrors the engine's `Placeable`).
pub struct PlaceInfo {
    pub type_path: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub category: &'static str,
}

pub const PLACEABLES: &[PlaceInfo] = &[
    PlaceInfo { type_path: "pacenote::open::placeable::HomePoint", name: "Home", icon: "home", category: "Pacenote" },
    PlaceInfo { type_path: "pacenote::open::placeable::EventStart", name: "Event start", icon: "flag", category: "Pacenote" },
    PlaceInfo { type_path: "pacenote::open::placeable::GradeNote", name: "Pace-note grade", icon: "note", category: "Pacenote" },
];

pub struct PlaceablePlugin;

impl Plugin for PlaceablePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<HomePoint>().register_type::<EventStart>().register_type::<GradeNote>();
    }
}

/// The shortest event (stations between start and finish) an event start can be dragged to.
const MIN_EVENT_STATIONS: u32 = (200.0 / crate::stage::STEP) as u32;

/// How far (m) a point may be from a road to snap onto it.
const SNAP: f32 = 15.0;

fn facing(dir: Vec2) -> Quat {
    // Local −Z along `dir` (XZ).
    Quat::from_rotation_y(libm::atan2f(-dir.x, -dir.y))
}

/// The gameplay points of `data` as entities (each with a `Transform` and a `Name`).
pub fn spawn(world: &mut World, data: &WorldData, net: &RoadNet) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut unplaced = Vec::new();
    out.push(world.spawn((Name::new("home"), HomePoint, Transform::from_translation(data.home.pos).with_rotation(facing(data.home.dir)))).id());
    for e in &data.events {
        let Some(r) = net.roads.get(e.road as usize) else { continue };
        let Some(&p) = r.road.pts.get(e.from as usize) else { continue };
        let d = r.road.dir[e.from as usize];
        out.push(world.spawn((Name::new(format!("event start: {}", e.name)), EventStart { event: e.id }, Transform::from_translation(p).with_rotation(facing(d)))).id());
    }
    for g in &data.grades {
        let Some(&p) = net.roads.get(g.road as usize).and_then(|r| r.road.pts.get(g.station as usize)) else {
            warn!("grade {g:?}: no such road station; kept in the data, not shown");
            unplaced.push(*g);
            continue;
        };
        out.push(world.spawn((Name::new(format!("grade {}", g.grade)), GradeNote { grade: g.grade, road: Some(g.road) }, Transform::from_translation(p))).id());
    }
    world.insert_resource(PointsSpawned { unplaced_grades: unplaced });
    out
}

/// `data` with the gameplay points read back from the entities: home from its transform,
/// event starts and grades snapped to the nearest road station. An event start moved off its
/// road keeps its old start; grade notes away from every road are dropped. Without [`spawn`]
/// having run (no editable points in the world) the data is returned as it was.
pub fn gather(world: &mut World, data: &WorldData, net: &RoadNet) -> WorldData {
    let mut out = data.clone();
    let Some(unplaced) = world.get_resource::<PointsSpawned>().map(|p| p.unplaced_grades.clone()) else {
        warn!("gather: the gameplay points weren't spawned; data unchanged");
        return out;
    };
    let station = |p: Vec3, only: Option<usize>| -> Option<(usize, usize)> {
        let p = Vec2::new(p.x, p.z);
        let (k, at) = match only {
            Some(k) => net.roads.get(k)?.road.nearest(p, SNAP).map(|at| (k, at)),
            None => net.nearest(p, SNAP).map(|n| (n.road, n.at)),
        }?;
        let road = &net.roads[k];
        let len = road.road.len();
        let mut i = if at.t < 0.5 { at.i } else { (at.i + 1).min(len - 1) };
        // A closed road's last station joins its first: past the last, the nearest may be 0.
        if road.closed {
            let first = road.road.pts[0];
            let last = road.road.pts[i];
            if Vec2::new(first.x, first.z).distance(p) < Vec2::new(last.x, last.z).distance(p) {
                i = 0;
            }
        }
        Some((k, i))
    };
    let mut q = world.query::<(&Transform, &HomePoint)>();
    if let Some((t, _)) = q.iter(world).next() {
        let f = t.rotation * Vec3::NEG_Z;
        out.home = Home { pos: t.translation, dir: Vec2::new(f.x, f.z).normalize_or(Vec2::X) };
    }
    let mut q = world.query::<(&Transform, &EventStart)>();
    for (t, s) in q.iter(world) {
        let Some(e) = out.events.iter_mut().find(|e| e.id == s.event) else { continue };
        if let Some((_, i)) = station(t.translation, Some(e.road as usize)) {
            let i = i as u32;
            if e.kind == super::events::EventKind::Lap || i + MIN_EVENT_STATIONS <= e.to {
                e.from = i;
            } else {
                warn!("event {} \"{}\": start moved to or past its finish; kept at station {}", e.id, e.name, e.from);
            }
        }
    }
    let mut q = world.query::<(&Transform, &GradeNote)>();
    let mut grades: Vec<GradeOverride> = q
        .iter(world)
        .filter_map(|(t, g)| {
            let on = g.road.and_then(|r| station(t.translation, Some(r as usize))).or_else(|| station(t.translation, None));
            on.map(|(road, i)| GradeOverride { road: road as u32, station: i as u32, grade: g.grade.clamp(1, 6) })
        })
        .collect();
    grades.extend(unplaced);
    grades.sort_by_key(|g| (g.road, g.station));
    out.grades = grades;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> (WorldData, RoadNet) {
        let mut d = WorldData::generate(crate::open::SEED);
        d.grades.push(GradeOverride { road: 2, station: 40, grade: 2 });
        d.grades.push(GradeOverride { road: 0, station: 10, grade: 5 });
        d.grades.sort_by_key(|g| (g.road, g.station));
        let net = d.net();
        (d, net)
    }

    /// Spawned and gathered with nothing moved: the same data.
    #[test]
    fn points_round_trip() {
        let (d, net) = data();
        let mut w = World::new();
        spawn(&mut w, &d, &net);
        let back = gather(&mut w, &d, &net);
        assert_eq!(back.events, d.events);
        assert_eq!(back.grades, d.grades);
        assert!(back.home.pos.distance(d.home.pos) < 1e-4 && back.home.dir.distance(d.home.dir) < 1e-4, "{:?} vs {:?}", back.home, d.home);
    }

    /// Gathering from a world without the editable points (the plain game) changes nothing;
    /// a grade spawn can't place survives the save.
    #[test]
    fn gather_keeps_what_it_cannot_see() {
        let (mut d, net) = data();
        let mut w = World::new();
        assert_eq!(gather(&mut w, &d, &net), d);
        d.grades.push(GradeOverride { road: 999, station: 1, grade: 4 });
        spawn(&mut w, &d, &net);
        assert!(gather(&mut w, &d, &net).grades.contains(&GradeOverride { road: 999, station: 1, grade: 4 }));
    }

    /// A grade note nudged towards a crossing road stays on its own road.
    #[test]
    fn grade_note_keeps_its_road() {
        let (mut d, net) = data();
        // A forest road's first station is on the ring: a note just off it is nearer the ring.
        let forest = net.roads.iter().position(|r| r.kind == super::super::roads::RoadKind::Forest).unwrap();
        d.grades = vec![GradeOverride { road: forest as u32, station: 2, grade: 2 }];
        let mut w = World::new();
        spawn(&mut w, &d, &net);
        let p0 = net.roads[forest].road.pts[0];
        let mut q = w.query::<&mut Transform>();
        for mut t in q.iter_mut(&mut w) {
            if t.translation == net.roads[forest].road.pts[2] {
                t.translation = p0;
            }
        }
        let back = gather(&mut w, &d, &net);
        assert_eq!(back.grades[0].road, forest as u32, "{:?}", back.grades);
    }

    /// Dragging an event start 100 m along its road moves the route's start there.
    #[test]
    fn moving_an_event_start_moves_its_route() {
        let (d, net) = data();
        let mut w = World::new();
        spawn(&mut w, &d, &net);
        let e = d.events[0].clone();
        let to = net.roads[e.road as usize].road.pts[e.from as usize + 50];
        let mut q = w.query::<(&mut Transform, &EventStart)>();
        for (mut t, s) in q.iter_mut(&mut w) {
            if s.event == e.id {
                t.translation = to + Vec3::new(0.4, 0.0, 0.0);
            }
        }
        let back = gather(&mut w, &d, &net);
        assert_eq!(back.events[0].from, e.from + 50);
        assert_eq!(back.events[1..], d.events[1..]);
    }
}
