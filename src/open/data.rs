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
/// 2: grade overrides use the sim-rally scale, 0 = hairpin, 1 = square ([`WorldData::migrate`]).
pub const VERSION: u32 = 2;

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
    /// Every building, once the world is a level (`open::level`); empty: laid out from the
    /// streets and lots (`buildings::layout_with`).
    #[serde(default)]
    pub buildings: Vec<buildings::Building>,
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
/// `road` is called as `grade` (`stage::HAIRPIN` = 0, 1 square … 6 flat-out kink; in version 1
/// data 1 was the hairpin, see [`WorldData::migrate`]).
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
            lots: buildings::town_lots(seed),
            buildings: Vec::new(),
        }
    }

    /// The road network: every road rebuilt from its points, junctions found where they meet.
    /// Road `k` of the network is `self.roads[k]` (events and grades refer to roads by index).
    /// Needs data that passed [`WorldData::validate`] (a road needs two points).
    pub fn net(&self) -> RoadNet {
        let roads: Vec<NetRoad> = self.roads.iter().map(net_road).collect();
        let junctions = roads::find_junctions(&roads);
        RoadNet { seed: self.seed, roads, junctions, town_y: self.town_y, land: super::land::land(self.seed) }
    }

    /// Checks data from a level file before anything is built from it: version, sizes, finite
    /// numbers in range, and every road, event, grade and lot referring to something that exists.
    /// Building from data that fails it can panic or run out of memory.
    pub fn validate(&self) -> Result<(), String> {
        let fin = |v: f32| v.is_finite();
        let inside = |p: Vec3| fin(p.x) && fin(p.y) && fin(p.z) && p.x.abs() <= LIMIT_XZ && p.z.abs() <= LIMIT_XZ && p.y.abs() <= LIMIT_Y;
        if self.version == 0 || self.version > VERSION {
            return Err(format!("world data version {} (this build reads 1..={VERSION})", self.version));
        }
        if !fin(self.town_y) || self.town_y.abs() > LIMIT_Y {
            return Err(format!("town height {}", self.town_y));
        }
        if !inside(self.home.pos) || !fin(self.home.dir.x) || !fin(self.home.dir.y) || self.home.dir.length() < 0.5 {
            return Err(format!("home {:?}", self.home));
        }
        if self.roads.is_empty() || self.roads.len() > MAX_ROADS {
            return Err(format!("{} roads (1..={MAX_ROADS})", self.roads.len()));
        }
        let mut points = 0;
        for (k, r) in self.roads.iter().enumerate() {
            let n = r.points.len();
            points += n;
            if n < 2 || points > MAX_POINTS {
                return Err(format!("road {k}: {n} points (2 or more, {MAX_POINTS} in all)"));
            }
            if let Some(v) = r.speed_kmh
                && !(fin(v) && (5.0..=300.0).contains(&v))
            {
                return Err(format!("road {k}: speed limit {v} km/h"));
            }
            for (i, p) in r.points.iter().enumerate() {
                if !inside(p.pos) || !(fin(p.width) && (MIN_WIDTH..=MAX_WIDTH).contains(&p.width)) || !(fin(p.bank) && p.bank.abs() <= MAX_BANK) {
                    return Err(format!("road {k} point {i}: {p:?}"));
                }
                // Stations about `STEP` apart (the road is sampled every STEP m).
                let next = if i + 1 < n { Some(&r.points[i + 1]) } else if r.closed { Some(&r.points[0]) } else { None };
                if let Some(q) = next {
                    let d = Vec2::new(q.pos.x - p.pos.x, q.pos.z - p.pos.z).length();
                    if !(MIN_GAP..=MAX_GAP).contains(&d) {
                        return Err(format!("road {k}: point {i} is {d:.2} m from the next ({MIN_GAP}..={MAX_GAP})"));
                    }
                }
            }
        }
        let len = |road: u32| self.roads.get(road as usize).map(|r| r.points.len());
        for e in &self.events {
            let Some(n) = len(e.road) else { return Err(format!("event {}: no road {}", e.id, e.road)) };
            let ok = if e.kind == events::EventKind::Lap { self.roads[e.road as usize].closed && (e.from as usize) < n } else { e.from < e.to && (e.to as usize) < n };
            if !ok {
                return Err(format!("event {}: stations {}..={} on road {} ({n} points)", e.id, e.from, e.to, e.road));
            }
        }
        if self.events.iter().enumerate().any(|(i, e)| self.events[..i].iter().any(|o| o.id == e.id)) {
            return Err("two events share an id".into());
        }
        for g in &self.grades {
            if len(g.road).is_none_or(|n| g.station as usize >= n) || g.grade > 6 {
                return Err(format!("grade {g:?}"));
            }
        }
        if self.lots.len() > 1_000 {
            return Err(format!("{} lots", self.lots.len()));
        }
        if self.buildings.len() > MAX_BUILDINGS {
            return Err(format!("{} buildings (at most {MAX_BUILDINGS})", self.buildings.len()));
        }
        for (k, b) in self.buildings.iter().enumerate() {
            let ok = inside(Vec3::new(b.centre.x, b.ground, b.centre.y))
                && fin(b.base)
                && fin(b.axis.x)
                && fin(b.axis.y)
                && (b.axis.length() - 1.0).abs() < 0.01
                && (1.0..=200.0).contains(&b.half.x)
                && (1.0..=200.0).contains(&b.half.y)
                && (1..=60).contains(&b.floors)
                && (2.0..=10.0).contains(&b.floor_h)
                && (0.0..=2.0).contains(&b.pitch)
                && b.front < 4
                && (b.wall as usize) < buildings::WALLS.len()
                && (b.roof_col as usize) < buildings::ROOFS.len();
            if !ok {
                return Err(format!("building {k}: {b:?}"));
            }
        }
        Ok(())
    }

    /// Brings data of an older format version up to [`VERSION`]. Version 1 graded 1 (hairpin)
    /// … 6, and every radius under 22 m was called "hairpin": its 1 becomes the hairpin (0),
    /// so a hand-set note is still called as it was.
    pub fn migrate(&mut self) {
        if self.version < 2 {
            for g in &mut self.grades {
                if g.grade <= 1 {
                    g.grade = crate::stage::HAIRPIN;
                }
            }
        }
        self.version = self.version.max(VERSION);
    }

    /// Sets the grade of every corner of event `e` that a [`GradeOverride`] falls in.
    pub fn apply_grades(&self, e: &EventDef, net: &RoadNet, corners: &mut [Corner]) {
        for g in &self.grades {
            let Some(s) = events::route_s(e, net, g.road as usize, g.station as usize) else { continue };
            // (A corner's `end` is where the next one may start: half-open.)
            if let Some(c) = corners.iter_mut().find(|c| c.start <= s && s < c.end) {
                c.grade = g.grade.min(6);
                // Keep "tightens" / "opens" only while they still go looser than the grade.
                c.tightens_from = c.tightens_from.filter(|t| *t > c.grade);
                c.opens_to = c.opens_to.filter(|t| *t > c.grade);
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

// Limits for [`WorldData::validate`].
const LIMIT_XZ: f32 = roads::HALF + 200.0;
const LIMIT_Y: f32 = 2_000.0;
const MAX_ROADS: usize = 1_000;
const MAX_POINTS: usize = 200_000;
const MAX_BUILDINGS: usize = 20_000;
const MIN_WIDTH: f32 = 2.0;
const MAX_WIDTH: f32 = 40.0;
const MAX_BANK: f32 = 0.5;
const MIN_GAP: f32 = 0.01;
const MAX_GAP: f32 = 4.0 * crate::stage::STEP;

const SURFACES: [Surface; 6] = [Surface::Gravel, Surface::Tarmac, Surface::Mud, Surface::Snow, Surface::Verge, Surface::Rough];
const KINDS: [RoadKind; 4] = [RoadKind::Street, RoadKind::BRoad, RoadKind::Gravel, RoadKind::Forest];

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
    fn generated_data_is_valid() {
        WorldData::generate(crate::open::SEED).validate().unwrap();
    }

    /// Bad level data is refused before anything is built from it (no panic, no huge work).
    #[test]
    fn validate_refuses_bad_data() {
        let good = WorldData::generate(crate::open::SEED);
        type Break = Box<dyn Fn(&mut WorldData)>;
        let bad: Vec<(&str, Break)> = vec![
            ("negative width", Box::new(|d| d.roads[0].points[3].width = -4.0)),
            ("NaN width", Box::new(|d| d.roads[0].points[3].width = f32::NAN)),
            ("infinite position", Box::new(|d| d.roads[1].points[0].pos.x = f32::INFINITY)),
            ("far position", Box::new(|d| d.roads[1].points[0].pos.x = 1e9)),
            ("one-point road", Box::new(|d| d.roads[2].points.truncate(1))),
            ("stretched segment", Box::new(|d| d.roads[0].points[5].pos.x += 50.0)),
            ("no roads", Box::new(|d| d.roads.clear())),
            ("event on a missing road", Box::new(|d| d.events[0].road = 999)),
            ("event past the road's end", Box::new(|d| d.events[0].to = 1_000_000)),
            ("event backwards", Box::new(|d| d.events[0].from = d.events[0].to)),
            ("lap on an open road", Box::new(|d| {
                let open = d.roads.iter().position(|r| !r.closed).unwrap() as u32;
                d.events[0].kind = events::EventKind::Lap;
                d.events[0].road = open;
            })),
            ("duplicate event id", Box::new(|d| d.events[1].id = d.events[0].id)),
            ("grade 9", Box::new(|d| d.grades.push(GradeOverride { road: 0, station: 1, grade: 9 }))),
            ("grade off the road", Box::new(|d| d.grades.push(GradeOverride { road: 0, station: 99_999, grade: 2 }))),
            ("speed limit NaN", Box::new(|d| d.roads[0].speed_kmh = Some(f32::NAN))),
            ("home NaN", Box::new(|d| d.home.pos.y = f32::NAN)),
            ("too many lots", Box::new(|d| d.lots.extend(std::iter::repeat_n(LotData { i: 0, j: 0, kind: buildings::LotKind::Park }, 1_001)))),
            ("future version", Box::new(|d| d.version = VERSION + 1)),
        ];
        for (what, f) in bad {
            let mut d = good.clone();
            f(&mut d);
            assert!(d.validate().is_err(), "accepted: {what}");
            assert!(crate::open::OpenWorld::from_data(d).is_err(), "built: {what}");
        }
    }

    /// A road ending part way along another road's segment (not on a station, as an editor
    /// draws them) still makes a junction.
    #[test]
    fn junction_between_stations() {
        let pt = |x: f32, z: f32| RoadPoint { pos: Vec3::new(x, 0.0, z), width: 7.0, bank: 0.0, surface: Surface::Tarmac };
        let main: Vec<RoadPoint> = (0..=50).map(|i| pt(i as f32 * 2.0, 0.0)).collect();
        // Ends 0.5 m short of the main road's centreline, at x = 41 (between stations 20 and 21).
        let side: Vec<RoadPoint> = (0..=30).map(|i| pt(41.0, 60.5 - i as f32 * 2.0)).collect();
        let mut d = WorldData::generate(crate::open::SEED);
        d.roads = vec![
            RoadData { kind: RoadKind::Gravel, closed: false, speed_kmh: None, points: main },
            RoadData { kind: RoadKind::Gravel, closed: false, speed_kmh: None, points: side },
        ];
        let net = d.net();
        assert_eq!(net.junctions.len(), 1, "{:?}", net.junctions);
        let j = &net.junctions[0];
        assert!(j.roads.contains(&(1, 30)), "{j:?}");
        assert!(j.roads.iter().any(|r| r.0 == 0 && (20..=21).contains(&r.1)), "{j:?}");
    }

    /// Event routes drive on the road's own banking (an edit to it reaches the event).
    #[test]
    fn edited_bank_reaches_the_event() {
        let mut d = WorldData::generate(crate::open::SEED);
        let e = d.events[0].clone();
        d.roads[e.road as usize].points[e.from as usize + 10].bank = 0.05;
        let net = d.net();
        let ev = events::build(&net, &d.events);
        assert_eq!(ev[0].route.bank[10], 0.05);
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
        data.grades.push(GradeOverride { road: e.road as u32, station, grade: crate::stage::HAIRPIN });
        data.apply_grades(e, &net, &mut corners);
        assert_eq!(corners.iter().find(|x| x.start == c.start).unwrap().grade, crate::stage::HAIRPIN);
        assert!(data.validate().is_ok(), "a hairpin override (0) is valid");
    }

    /// Version 1 grades (1 = hairpin) read as they were meant: 1 → hairpin, the rest unchanged.
    #[test]
    fn version_1_grades_migrate() {
        let mut data = WorldData::generate(crate::open::SEED);
        data.version = 1;
        data.grades = vec![GradeOverride { road: 0, station: 1, grade: 1 }, GradeOverride { road: 0, station: 2, grade: 4 }];
        assert!(data.validate().is_ok());
        data.migrate();
        assert_eq!(data.version, VERSION);
        assert_eq!(data.grades.iter().map(|g| g.grade).collect::<Vec<_>>(), [crate::stage::HAIRPIN, 4]);
        // Current data is left alone.
        data.grades[1].grade = 1;
        data.migrate();
        assert_eq!(data.grades[1].grade, 1, "square stays square");
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
