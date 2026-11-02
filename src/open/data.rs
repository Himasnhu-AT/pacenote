//! The open world as data: everything an editor changes, in one value.
//!
//! [`WorldData::generate`] captures the world the seed makes; [`WorldData::net`] builds the
//! road network back from it (junctions found again from where the roads meet). The game
//! always goes through this (`OpenWorld::generate` = capture, then build), so a world loaded
//! from a level file runs exactly the code a generated one does.
//!
//! Still generated from `seed` until the level holds them: the terrain away from the roads,
//! the farms, the trees, rocks and bushes.
//!
//! Roads map onto the engine's spline contract (docs/EDITOR.md "World tools"): a
//! [`RoadPoint`] is a `SplinePoint` (pos, width, bank) whose surface and the road's kind are
//! packed in its `tags` ([`tags`] / [`from_tags`]).

use bevy::math::{Vec2, Vec3};
use bevy::reflect::Reflect;
use serde::{Deserialize, Serialize};

use super::buildings::{self, LotData};
use super::events::{self, EventData, EventDef};
use super::roads::{self, NetRoad, RoadKind, RoadNet};
use crate::stage::{Corner, Road, Surface, TimeOfDay};

/// Format version of [`WorldData`] (bump with a migration when fields change meaning).
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Reflect)]
pub struct WorldData {
    pub version: u32,
    /// Drives what isn't data yet (terrain noise, farms, dressing).
    pub seed: u64,
    /// The town plateau height (m).
    pub town_y: f32,
    pub time: TimeOfDay,
    /// Where the car starts in free roam and respawns after a fatal crash.
    pub home: Home,
    /// Every road, in a fixed order (events and overrides refer to roads by index).
    pub roads: Vec<RoadData>,
    pub events: Vec<EventData>,
    /// Pace-note grades set by hand (the co-driver's call for a corner).
    pub grades: Vec<GradeOverride>,
    /// Town block kinds.
    pub lots: Vec<LotData>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Reflect)]
pub struct Home {
    pub pos: Vec3,
    /// Facing (unit, XZ).
    pub dir: Vec2,
}

/// One road: its stations every `stage::STEP` m.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Reflect)]
pub struct RoadData {
    pub kind: RoadKind,
    /// The last point joins the first (the ring).
    pub closed: bool,
    /// Traffic speed limit (km/h); `None`: by kind.
    pub speed_kmh: Option<f32>,
    pub points: Vec<RoadPoint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Reflect)]
pub struct RoadPoint {
    /// Centreline, y = road height.
    pub pos: Vec3,
    /// Full width (m).
    pub width: f32,
    /// Crossfall (height change per metre to the left).
    pub bank: f32,
    pub surface: Surface,
}

/// A hand-set pace-note grade: the corner of any event that covers station `station` of road
/// `road` is called as `grade` (1 hairpin … 6 flat-out kink).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Reflect)]
pub struct GradeOverride {
    pub road: u32,
    pub station: u32,
    pub grade: u8,
}

impl WorldData {
    /// The world the seed makes, as data.
    pub fn generate(seed: u64) -> WorldData {
        let net = roads::generate(seed);
        let layout = buildings::layout(&net);
        let (pos, dir) = roads::spawn(&net);
        WorldData {
            version: VERSION,
            seed,
            town_y: net.town_y,
            time: super::def().time,
            home: Home { pos, dir },
            roads: net.roads.iter().map(road_data).collect(),
            events: events::specs(&net),
            grades: Vec::new(),
            lots: buildings::lots(&layout),
        }
    }

    /// The road network: every road rebuilt from its points, junctions found where they meet.
    pub fn net(&self) -> RoadNet {
        let roads: Vec<NetRoad> = self.roads.iter().filter(|r| r.points.len() >= 2).map(net_road).collect();
        let junctions = roads::find_junctions(&roads);
        RoadNet { seed: self.seed, roads, junctions, town_y: self.town_y }
    }

    /// Sets the grade of every corner of event `e` that a [`GradeOverride`] falls in.
    pub fn apply_grades(&self, e: &EventDef, net: &RoadNet, corners: &mut [Corner]) {
        for g in &self.grades {
            let Some(s) = events::route_s(e, net, g.road as usize, g.station as usize) else { continue };
            if let Some(c) = corners.iter_mut().find(|c| c.start <= s && s <= c.end) {
                c.grade = g.grade.clamp(1, 6);
            }
        }
    }
}

fn road_data(r: &NetRoad) -> RoadData {
    let road = &r.road;
    RoadData {
        kind: r.kind,
        closed: r.closed,
        speed_kmh: r.limit.map(|v| v * 3.6),
        points: (0..road.len()).map(|i| RoadPoint { pos: road.pts[i], width: road.width[i], bank: road.bank[i], surface: road.surface[i] }).collect(),
    }
}

fn net_road(d: &RoadData) -> NetRoad {
    let pts: Vec<Vec2> = d.points.iter().map(|p| Vec2::new(p.pos.x, p.pos.z)).collect();
    let y: Vec<f32> = d.points.iter().map(|p| p.pos.y).collect();
    let mut road = Road::from_centreline(&pts, &y, d.points.iter().map(|p| p.width).collect(), d.points.iter().map(|p| p.surface).collect());
    road.bank = d.points.iter().map(|p| p.bank).collect();
    let mut nr = NetRoad::new(d.kind, road, d.closed);
    nr.limit = d.speed_kmh.map(|v| v / 3.6);
    nr
}

const SURFACES: [Surface; 6] = [Surface::Gravel, Surface::Tarmac, Surface::Mud, Surface::Snow, Surface::Verge, Surface::Rough];
const KINDS: [RoadKind; 4] = [RoadKind::Street, RoadKind::Highway, RoadKind::Country, RoadKind::Forest];

/// A spline point's `tags` for a road point: bits 0–3 the surface, 4–7 the road's kind.
pub fn tags(kind: RoadKind, surface: Surface) -> u32 {
    let s = SURFACES.iter().position(|x| *x == surface).unwrap_or(1) as u32;
    let k = KINDS.iter().position(|x| *x == kind).unwrap_or(0) as u32;
    s | k << 4
}

/// The road kind and surface packed by [`tags`] (unknown values: street, tarmac).
pub fn from_tags(tags: u32) -> (RoadKind, Surface) {
    let s = SURFACES.get((tags & 0xF) as usize).copied().unwrap_or(Surface::Tarmac);
    let k = KINDS.get((tags >> 4 & 0xF) as usize).copied().unwrap_or(RoadKind::Street);
    (k, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open::junctions;

    fn nodes_sorted(net: &RoadNet) -> Vec<(Vec2, Vec<(usize, usize)>)> {
        let mut n = junctions::nodes(net);
        for (_, pairs) in &mut n {
            pairs.sort_unstable();
        }
        n.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.0.y.total_cmp(&b.0.y)));
        n
    }

    /// Generated world → data → network: the same roads, junctions, events and buildings.
    #[test]
    fn data_rebuilds_the_generated_world() {
        let seed = crate::open::SEED;
        let generated = roads::generate(seed);
        let data = WorldData::generate(seed);
        let net = data.net();
        assert_eq!(net.roads.len(), generated.roads.len());
        for (a, b) in generated.roads.iter().zip(&net.roads) {
            assert_eq!((a.kind, a.closed), (b.kind, b.closed));
            assert_eq!(a.road.pts, b.road.pts);
            assert_eq!(a.road.dir, b.road.dir);
            assert_eq!(a.road.curv, b.road.curv);
            assert_eq!(a.road.width, b.road.width);
            assert_eq!(a.road.bank, b.road.bank);
            assert_eq!(a.road.surface, b.road.surface);
        }
        // Junctions are found again, not stored: the same meeting points and road stations.
        let (ga, na) = (nodes_sorted(&generated), nodes_sorted(&net));
        assert_eq!(ga.len(), na.len(), "junction count\ngenerated {ga:?}\nfound {na:?}");
        for (g, n) in ga.iter().zip(&na) {
            assert!(g.0.distance(n.0) < 0.01, "junction moved: {g:?} vs {n:?}");
            assert_eq!(g.1, n.1, "junction at {:?}", g.0);
        }
        assert_eq!(junctions::build(&generated).len(), junctions::build(&net).len());
        // Events: same names, kinds and routes.
        let (ge, ne) = (events::list(&generated), events::build(&net, &data.events));
        assert_eq!(ge.len(), ne.len());
        for (a, b) in ge.iter().zip(&ne) {
            assert_eq!((a.id, &a.name, a.kind), (b.id, &b.name, b.kind));
            assert_eq!(a.route.pts, b.route.pts);
            assert_eq!(a.route.bank, b.route.bank);
        }
        // Buildings from the stored lot kinds.
        let (gl, nl) = (buildings::layout(&generated), buildings::layout_with(&net, &data.lots));
        assert_eq!(gl.buildings, nl.buildings);
        // Heights along and off the roads.
        for p in [Vec2::new(-290.0, 0.0), Vec2::new(900.0, 700.0), Vec2::new(-1400.0, 300.0), Vec2::new(1450.0, 5.0)] {
            assert_eq!(roads::height(&generated, p.x, p.y), roads::height(&net, p.x, p.y), "height at {p}");
        }
        assert_eq!(roads::spawn(&generated), (data.home.pos, data.home.dir));
    }

    /// The data survives a save and load (RON, as levels are written).
    #[test]
    fn data_round_trips_through_ron() {
        let mut data = WorldData::generate(crate::open::SEED);
        data.grades.push(GradeOverride { road: 3, station: 120, grade: 2 });
        data.roads[0].speed_kmh = Some(30.0);
        let text = ron::ser::to_string(&data).expect("serialise");
        let back: WorldData = ron::from_str(&text).expect("parse");
        assert_eq!(back, data);
    }

    #[test]
    fn tags_round_trip() {
        for k in KINDS {
            for s in SURFACES {
                assert_eq!(from_tags(tags(k, s)), (k, s));
            }
        }
    }

    /// A grade override changes the call of the corner it falls in, on the event that covers it.
    #[test]
    fn grade_overrides_apply() {
        let mut data = WorldData::generate(crate::open::SEED);
        let net = data.net();
        let ev = events::build(&net, &data.events);
        let e = ev.iter().find(|e| e.kind == events::EventKind::Rally).unwrap();
        let mut corners = crate::stage::corners(&e.route);
        let c = corners.iter().find(|c| c.grade > 2).cloned().expect("a corner above grade 2");
        let station = e.from as u32 + ((c.start + c.end) * 0.5 / crate::stage::STEP) as u32;
        data.grades.push(GradeOverride { road: e.road as u32, station, grade: 1 });
        data.apply_grades(e, &net, &mut corners);
        assert_eq!(corners.iter().find(|x| x.start == c.start).unwrap().grade, 1);
    }

    /// An edited speed limit reaches the traffic lanes' road.
    #[test]
    fn speed_limit_in_data() {
        let mut data = WorldData::generate(crate::open::SEED);
        data.roads[0].speed_kmh = Some(30.0);
        let net = data.net();
        assert!((net.roads[0].limit.unwrap() - 30.0 / 3.6).abs() < 1e-5);
        assert_eq!(net.roads[1].limit, None);
    }
}
