//! The stage in the world: terrain (engine `world`), the dressing graph (trees, rocks, bushes,
//! marker posts) and the collision data the car simulation reads. The visuals (road surface,
//! materials, trees, sky) are `crate::art`'s.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use engine::world::{Heightfield, WorldDesc};
use pcg_core::{Area, Graph, Inputs, Op, Rect, SpawnDef, SpawnKind, Tier};

use crate::stage::{self, Biome, Corner, Road, StageDef, Surface};
use crate::vehicle::{Give, Ground, Obstacle, Wall};

/// A roadside thing the car can knock down or drive through (drawn by `crate::props`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prop {
    /// Index in [`StageWorld::props`], and the obstacle's [`Give::Breaks`] id.
    pub id: u32,
    pub kind: PropKind,
    /// Base (on the ground), yaw and size (1 = the kind's standard size).
    pub pos: Vec3,
    pub yaw: f32,
    pub scale: f32,
    /// Stable per prop (variant picks, debris).
    pub seed: u64,
}

/// What a [`Prop`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropKind {
    /// White marker post (1.05 m): snaps at walking pace.
    Post,
    /// Kilometre board on a post (2 m).
    Sign,
    /// A young tree (3–5 m, thin trunk): snaps from ~11 km/h.
    Sapling,
    /// A bush the car drives through.
    Shrub,
    /// Open-world roadside furniture (`open::furniture`, drawn by `art::signs`): a reflector
    /// post, a km stone, a chevron board on its pole. `seed` is the index in
    /// `Furniture::signs`.
    Reflector,
    KmStone,
    RoadSign,
}

impl PropKind {
    /// Collision radius (m) and height (m) at scale 1.
    pub fn size(self) -> (f32, f32) {
        match self {
            PropKind::Post => (0.07, 1.05),
            PropKind::Sign => (0.09, 2.0),
            PropKind::Sapling => (0.3, 11.0),
            PropKind::Shrub => (0.85, 1.1),
            PropKind::Reflector => (0.07, 1.0),
            PropKind::KmStone => (0.14, 0.8),
            PropKind::RoadSign => (0.05, 2.7),
        }
    }

    /// Least distance (m) from the road's edge to its centre.
    pub fn clearance(self) -> f32 {
        match self {
            PropKind::Post | PropKind::Sign | PropKind::Reflector | PropKind::KmStone | PropKind::RoadSign => 0.8,
            PropKind::Sapling => 2.0,
            PropKind::Shrub => 0.6,
        }
    }

    /// How the car interacts with one of `scale`: mass (kg) and the closing speed (m/s) that
    /// snaps it, or the brush drag.
    pub fn give(self, id: u32, scale: f32) -> Give {
        match self {
            PropKind::Post => Give::Breaks { id, mass: 6.0, speed: 1.0 },
            PropKind::Sign => Give::Breaks { id, mass: 25.0, speed: 2.0 },
            // ~50–110 kg of green wood.
            PropKind::Sapling => Give::Breaks { id, mass: 250.0 * scale, speed: 3.0 },
            PropKind::Shrub => Give::Brush { drag: 0.15 * scale },
            PropKind::Reflector => Give::Breaks { id, mass: 6.0, speed: 1.0 },
            PropKind::KmStone => Give::Breaks { id, mass: 40.0, speed: 2.5 },
            PropKind::RoadSign => Give::Breaks { id, mass: 20.0, speed: 2.0 },
        }
    }
}

/// The current stage, generated.
#[derive(Resource, Clone)]
pub struct StageWorld {
    pub def: StageDef,
    pub road: Arc<Road>,
    pub heights: Arc<Heightfield>,
    pub corners: Arc<Vec<Corner>>,
    pub crests: Arc<Vec<f32>>,
    /// Roadside things the car can knock down (posts, signs, saplings) or drive through
    /// (shrubs), in a fixed order (their ids).
    pub props: Arc<Vec<Prop>>,
    /// Trunks, boulders and props near the road: 8 m grid.
    obstacles: Arc<HashMap<(i32, i32), Vec<Obstacle>>>,
    /// The open world (`def.id == open::ID`): every road, its surfaces. `road` is then the
    /// street the car starts on, or the running event's route.
    pub open: Option<Arc<crate::open::OpenWorld>>,
    /// The event being driven in the open world (`road`, `corners`, `crests` are its route).
    pub event: Option<crate::open::events::EventRef>,
    /// The open world's trunks and boulders, everywhere (built by tile, `open::forest`).
    pub forest: Option<Arc<crate::open::forest::ForestSolids>>,
    /// The rivals' cars in a Race run (`career::racers`, stream F): solid for the player.
    /// `None` outside races.
    pub racers: Option<Arc<crate::career::racers::RacerSolids>>,
}

const OBS_CELL: f32 = 8.0;

impl StageWorld {
    /// In the open world (free roam or an event).
    pub fn in_world(&self) -> bool {
        self.open.is_some()
    }

    /// Free roam: in the open world with no event running (no route, clock or finish).
    pub fn free_roam(&self) -> bool {
        self.open.is_some() && self.event.is_none()
    }

    /// What the route's records, ghosts and pace notes are keyed by: the event, else the stage.
    pub fn route_id(&self) -> u32 {
        self.event.as_ref().map_or(self.def.id, |e| e.id)
    }

    /// Every road: the stage's one, or the open world's network.
    pub fn roads(&self) -> Vec<&Road> {
        match &self.open {
            Some(o) => o.net.roads.iter().map(|r| &r.road).collect(),
            None => vec![&*self.road],
        }
    }

    /// The nearest road to `p` within `max` (any road of the network in the open world).
    pub fn nearest_road(&self, p: Vec2, max: f32) -> Option<(&Road, stage::RoadPos)> {
        match &self.open {
            Some(o) => o.net.nearest(p, max).map(|n| (&o.net.roads[n.road].road, n.at)),
            None => self.road.nearest(p, max).map(|r| (&*self.road, r)),
        }
    }

    /// Every solid obstacle the car can hit (trunk / boulder centre, radius, top), sorted: for
    /// checking stages generate the same scenery on every machine. (In the open world: the
    /// forest tiles built so far.)
    pub fn obstacle_list(&self) -> Vec<(Vec2, f32, f32)> {
        let forest = self.forest.as_ref().map(|f| f.built_solids()).unwrap_or_default();
        let mut v: Vec<(Vec2, f32, f32)> = self.obstacles.values().flatten().chain(forest.iter()).filter(|o| o.give == Give::Solid).map(|o| (o.pos, o.radius, o.top)).collect();
        v.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.0.y.total_cmp(&b.0.y)).then(a.1.total_cmp(&b.1)).then(a.2.total_cmp(&b.2)));
        v
    }
}

impl Ground for StageWorld {
    fn height(&self, x: f32, z: f32) -> f32 {
        match &self.open {
            Some(o) => o.ground_height(x, z),
            None => self.heights.height_at(x, z),
        }
    }
    fn normal(&self, x: f32, z: f32) -> Vec3 {
        match &self.open {
            Some(o) => o.ground_normal(x, z),
            None => self.heights.smooth_normal_at(x, z),
        }
    }
    fn surface(&self, x: f32, z: f32) -> Surface {
        match &self.open {
            Some(o) => o.surface_at(x, z),
            None => stage::surface_at(x, z, &self.road),
        }
    }
    fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
        out.clear();
        let (x0, x1) = (((p.x - r) / OBS_CELL).floor() as i32, ((p.x + r) / OBS_CELL).floor() as i32);
        let (z0, z1) = (((p.y - r) / OBS_CELL).floor() as i32, ((p.y + r) / OBS_CELL).floor() as i32);
        for z in z0..=z1 {
            for x in x0..=x1 {
                if let Some(v) = self.obstacles.get(&(x, z)) {
                    out.extend_from_slice(v);
                }
            }
        }
        if let Some(o) = &self.open {
            // Traffic only in free roam (events run without it, so ghosts replay exactly).
            if let Some(f) = &self.forest {
                f.obstacles(p, r, out);
            }
            if self.event.is_none() && o.traffic_solid.load(std::sync::atomic::Ordering::Relaxed) {
                o.traffic.obstacles(p, r, out);
            }
        }
        // Rivals, in Race runs only (never in Time Trials, so ghosts replay exactly).
        if let Some(racers) = &self.racers {
            racers.obstacles(p, r, out);
        }
    }
    fn walls(&self, p: Vec2, r: f32, out: &mut Vec<Wall>) {
        out.clear();
        if let Some(o) = &self.open {
            o.walls.query(p, r, out);
        }
    }
}

/// Builds everything that doesn't need the ECS (also used headless and by the guide).
pub fn generate(def: &StageDef) -> StageWorld {
    if def.id == crate::open::ID {
        return generate_open(def);
    }
    let road = stage::generate_road(def);
    let half = stage::half_size(def);
    let n = (2.0 * half / stage::SPACING) as u32 + 1;
    let origin = Vec2::splat(-half);
    let heights = Heightfield::from_fn(n, n, stage::SPACING, origin, |x, z| stage::height(x, z, def, &road));
    let corners = stage::corners(&road);
    let crests = stage::crests(&road);
    let mut world = StageWorld {
        def: def.clone(),
        road: Arc::new(road),
        heights: Arc::new(heights),
        corners: Arc::new(corners),
        crests: Arc::new(crests),
        props: Arc::new(Vec::new()),
        obstacles: Arc::new(HashMap::new()),
        open: None,
        event: None,
        forest: None,
        racers: None,
    };
    // Collision for the car: the same points the dressing spawns, evaluated in a band along
    // the road (the trees far away can't be reached).
    let (graph, inputs) = dressing(&world);
    let band = road_area(&world.road, 40.0);
    if let Ok(out) = pcg_core::evaluate(&graph, &inputs, Some(band)) {
        let mut grid: HashMap<(i32, i32), Vec<Obstacle>> = HashMap::new();
        let cell = |c: Vec2| ((c.x / OBS_CELL).floor() as i32, (c.y / OBS_CELL).floor() as i32);
        for (port, radius) in [("trunks", 0.32f32), ("boulders", 0.85), ("stumps", 0.3)] {
            if let Some(p) = out.emits.get(port) {
                for k in 0..p.len() {
                    let c = Vec2::new(p.pos[k].x, p.pos[k].z);
                    let near = world.road.nearest(c, 30.0).is_some();
                    if !near {
                        continue;
                    }
                    let r = radius * p.scale[k].x;
                    let top = p.pos[k].y + 4.0 * p.scale[k].y;
                    grid.entry(cell(c)).or_default().push(Obstacle::solid(c, r, top));
                }
            }
        }
        // Props: sorted by position so ids are the same on every machine.
        let mut found = roadside(&world);
        for (port, kind) in [("saplings", PropKind::Sapling), ("shrubs", PropKind::Shrub)] {
            if let Some(p) = out.emits.get(port) {
                for k in 0..p.len() {
                    let c = Vec2::new(p.pos[k].x, p.pos[k].z);
                    // Near the road but never on it (the graph keeps clear of 20 m chords of
                    // the road, which cut inside a hairpin).
                    let Some(rp) = world.road.nearest(c, 30.0) else { continue };
                    if rp.lateral.abs() < world.road.half_width(&rp) + kind.clearance() {
                        continue;
                    }
                    // A turn about Y (not projected onto the slope); libm, same bits everywhere.
                    let q = p.rot[k];
                    let yaw = 2.0 * libm::atan2f(q.y, q.w);
                    found.push((kind, p.pos[k], yaw, p.scale[k].x, p.seed[k]));
                }
            }
        }
        found.sort_by(|a, b| a.1.x.total_cmp(&b.1.x).then(a.1.z.total_cmp(&b.1.z)).then((a.0 as u8).cmp(&(b.0 as u8))));
        let mut props = Vec::with_capacity(found.len());
        for (i, (kind, pos, yaw, scale, seed)) in found.into_iter().enumerate() {
            let id = i as u32;
            let (r, h) = kind.size();
            let c = Vec2::new(pos.x, pos.z);
            grid.entry(cell(c)).or_default().push(Obstacle { pos: c, radius: r * scale, top: pos.y + h * scale, give: kind.give(id, scale) });
            props.push(Prop { id, kind, pos, yaw, scale, seed });
        }
        world.obstacles = Arc::new(grid);
        world.props = Arc::new(props);
    }
    world
}

/// The open world as a [`StageWorld`]: the network and terrain from [`crate::open`], every trunk
/// and boulder solid (the dressing graph keeps them off every road and out of town).
fn generate_open(def: &StageDef) -> StageWorld {
    let open = crate::open::OpenWorld::load_or_generate(def.seed);
    let (start, _) = open.spawn();
    // The world data sets the time of day.
    let def = &StageDef { time: open.data.time, ..def.clone() };
    let street = open.net.nearest(Vec2::new(start.x, start.z), 20.0).map_or(0, |n| n.road);
    let mut world = StageWorld {
        def: def.clone(),
        road: Arc::new(open.net.roads[street].road.clone()),
        heights: open.heights.clone(),
        corners: Arc::new(Vec::new()),
        crests: Arc::new(Vec::new()),
        props: Arc::new(Vec::new()),
        obstacles: Arc::new(HashMap::new()),
        open: Some(Arc::new(open)),
        event: None,
        forest: None,
        racers: None,
    };
    let (graph, inputs) = dressing(&world);
    let net = world.open.as_ref().unwrap().net.clone();
    // Every trunk and boulder is solid (the car can leave the roads). The tiles a road passes
    // through (and their neighbours) are built now, on all cores; the rest around the car as
    // it drives (`open::forest`).
    let o = world.open.as_ref().unwrap();
    let forest = crate::open::forest::ForestSolids::with_overrides(graph, inputs, &o.trees, &o.dressing);
    let mut tiles: Vec<(i32, i32)> = net
        .roads
        .iter()
        .flat_map(|r| r.road.pts.iter().step_by(16))
        .flat_map(|p| {
            let (tx, tz) = crate::open::forest::ForestSolids::tile_of(Vec2::new(p.x, p.z));
            (-1..=1).flat_map(move |dz| (-1..=1).map(move |dx| (tx + dx, tz + dz)))
        })
        .collect();
    tiles.sort_unstable();
    tiles.dedup();
    forest.fill(&tiles);
    world.forest = Some(Arc::new(forest));
    // Posts and chevrons the car knocks down (`open::furniture::breakable`): props, in the furniture's own (fixed) order, so ids are the same everywhere.
    let mut grid: HashMap<(i32, i32), Vec<Obstacle>> = HashMap::new();
    let mut props = Vec::new();
    for (k, s) in o.furniture.signs.iter().enumerate() {
        use crate::open::furniture::SignKind;
        let kind = match s.kind {
            SignKind::Reflector => PropKind::Reflector,
            SignKind::KmPost(_) => PropKind::KmStone,
            k if crate::open::furniture::breakable(k) => PropKind::RoadSign,
            _ => continue,
        };
        let id = props.len() as u32;
        let (r, h) = kind.size();
        let c = Vec2::new(s.pos.x, s.pos.z);
        grid.entry(((c.x / OBS_CELL).floor() as i32, (c.y / OBS_CELL).floor() as i32)).or_default().push(Obstacle { pos: c, radius: r, top: s.pos.y + h, give: kind.give(id, 1.0) });
        props.push(Prop { id, kind, pos: s.pos, yaw: s.yaw, scale: 1.0, seed: k as u64 });
    }
    world.obstacles = Arc::new(grid);
    world.props = Arc::new(props);
    world
}

/// Marker posts both sides every 50 m and a kilometre board on the right every 1000 m, set
/// off the edge by the road's own width (plain arithmetic on the road and the heightfield: the
/// same on every machine). (kind, base, yaw, scale, seed)
fn roadside(w: &StageWorld) -> Vec<(PropKind, Vec3, f32, f32, u64)> {
    let road = &w.road;
    let mut out = Vec::new();
    let len = road.length();
    let mut put = |kind: PropKind, s: f32, side: f32, gap: f32| {
        let i = road.station(s);
        let (p, d) = (road.pts[i], road.dir[i]);
        let left = Vec2::new(d.y, -d.x);
        let c = Vec2::new(p.x, p.z) + left * side * (road.width[i] * 0.5 + gap);
        // Boards face the cars coming.
        let yaw = libm::atan2f(-d.x, -d.y);
        let seed = pcg_core::mix64(w.def.seed ^ ((kind as u64) << 48) ^ ((s as u64) << 1) ^ (side > 0.0) as u64);
        out.push((kind, Vec3::new(c.x, w.heights.height_at(c.x, c.y), c.y), yaw, 1.0, seed));
    };
    let mut s = 25.0;
    while s < len - 10.0 {
        put(PropKind::Post, s, 1.0, 1.4);
        put(PropKind::Post, s, -1.0, 1.4);
        s += 50.0;
    }
    let mut s = 1000.0;
    while s < len - 50.0 {
        put(PropKind::Sign, s, -1.0, 2.2);
        s += 1000.0;
    }
    out
}

fn road_area(road: &Road, pad: f32) -> Area {
    let mut a = Area::EMPTY;
    for p in &road.pts {
        a = a.include(Vec2::new(p.x, p.z));
    }
    a.expand(pad)
}

/// The dressing graph and its inputs for a stage.
pub fn dressing(w: &StageWorld) -> (Graph, Inputs) {
    let half = stage::half_size(&w.def);
    // Every road as rects (20 m pieces) to keep things off it.
    let mut rects = Vec::new();
    for road in w.roads() {
        for i in (0..road.len().saturating_sub(10)).step_by(10) {
            let (a, b) = (road.pts[i], road.pts[i + 10]);
            let mid = (a + b) * 0.5;
            let d = Vec2::new(b.x - a.x, b.z - a.z);
            rects.push(Rect { center: Vec2::new(mid.x, mid.z), half: Vec2::new(road.width[i] * 0.5, d.length() * 0.5 + 1.0), yaw: libm::atan2f(d.x, d.y), y: mid.y, tag: "road".into() });
        }
    }
    // Fernhollow: nothing grows on its sidewalks, the square or the car park, nor against a
    // building (the back yards keep a few garden trees: the valley's scattered field trees);
    // the service park stays clear; houses out in the country keep a garden.
    if let Some(o) = &w.open {
        use crate::open::buildings::{LotKind, SIDEWALK, Use};
        let road = |center: Vec2, half: Vec2, yaw: f32| Rect { center, half, yaw, y: o.net.town_y, tag: "road".into() };
        for blk in o.buildings.blocks.iter().filter(|b| b.cell.is_some()) {
            let (lo, hi) = (blk.min - Vec2::splat(SIDEWALK + 0.5), blk.max + Vec2::splat(SIDEWALK + 0.5));
            if matches!(blk.kind, LotKind::Plaza | LotKind::Parking) {
                rects.push(road((lo + hi) * 0.5, (hi - lo) * 0.5, 0.0));
                continue;
            }
            // The sidewalk and the row of buildings behind it (22 m: the deepest unit and its
            // setback), round the block; the yard in the middle keeps a few trees. (One rect per
            // edge, not one per building: the tree graph checks every rect.)
            let strip = SIDEWALK + 0.5 + 22.0;
            let c = (lo + hi) * 0.5;
            let h = (hi - lo) * 0.5;
            rects.push(road(Vec2::new(c.x, lo.y + strip * 0.5), Vec2::new(h.x, strip * 0.5), 0.0));
            rects.push(road(Vec2::new(c.x, hi.y - strip * 0.5), Vec2::new(h.x, strip * 0.5), 0.0));
            rects.push(road(Vec2::new(lo.x + strip * 0.5, c.y), Vec2::new(strip * 0.5, h.y), 0.0));
            rects.push(road(Vec2::new(hi.x - strip * 0.5, c.y), Vec2::new(strip * 0.5, h.y), 0.0));
        }
        rects.push(road(o.net.places().park, Vec2::splat(150.0), 0.0));
        // Nothing grows in the river (`art::water` lays the water in its bed).
        let hf = &w.heights;
        for (c, h, yaw) in crate::art::water::channel(&o.net.land.river, |x, z| hf.height_at(x, z)) {
            rects.push(Rect { center: c, half: h, yaw, y: hf.height_at(c.x, c.y), tag: "road".into() });
        }
        for b in o.buildings.buildings.iter().filter(|b| !crate::open::town::in_core(b.centre) || b.use_ == Use::Civic) {
            let garden = if b.use_ == Use::House { 7.0 } else { 3.0 };
            rects.push(Rect { center: b.centre, half: b.half + Vec2::splat(garden), yaw: libm::atan2f(b.axis.y, b.axis.x), y: 0.0, tag: "road".into() });
        }
    }
    // The open world's ecology (`open::ecology`): above the treeline, valley fields, clear-cuts.
    let (alpine, fields, clearcut) = match &w.open {
        Some(o) => crate::open::ecology::masks(&o.net),
        None => (Vec::new(), Vec::new(), Vec::new()),
    };
    // Fernhollow's back yards (each block's middle, behind its rows) get garden trees, and the
    // market square a row of trees along each long side.
    let (mut gardens, mut planted) = (Vec::new(), pcg_core::PointSet::default());
    if let Some(o) = &w.open {
        use crate::open::buildings::LotKind;
        for blk in o.buildings.blocks.iter().filter(|b| b.cell.is_some()) {
            match blk.kind {
                LotKind::Rows(_) => {
                    let (lo, hi) = (blk.min + Vec2::splat(22.0), blk.max - Vec2::splat(22.0));
                    if hi.x - lo.x > 4.0 && hi.y - lo.y > 4.0 {
                        gardens.push(Rect::new((lo + hi) * 0.5, (hi - lo) * 0.5));
                    }
                }
                LotKind::Plaza => {
                    let mut plant = |p: Vec2| planted.push(pcg_core::Point { pos: Vec3::new(p.x, w.heights.height_at(p.x, p.y), p.y), seed: pcg_core::mix64(w.def.seed ^ (p.x.to_bits() as u64) << 32 ^ p.y.to_bits() as u64), ..Default::default() });
                    // Rows round the edge, 10 m apart, 3 m in.
                    let (lo, hi) = (blk.min + Vec2::splat(3.0), blk.max - Vec2::splat(3.0));
                    let mut z = lo.y;
                    while z <= hi.y + 0.1 {
                        plant(Vec2::new(lo.x, z));
                        plant(Vec2::new(hi.x, z));
                        z += 10.0;
                    }
                    let mut x = lo.x + 10.0;
                    while x < hi.x - 5.0 {
                        plant(Vec2::new(x, lo.y));
                        plant(Vec2::new(x, hi.y));
                        x += 10.0;
                    }
                }
                _ => {}
            }
        }
    }
    let hf = &w.heights;
    let inputs = Inputs::default()
        .rects("area", vec![Rect::new(Vec2::ZERO, Vec2::splat(half - 4.0))])
        .rects("gardens", gardens)
        .points("planted", planted)
        .rects("road", rects)
        .rects("alpine", alpine)
        .rects("fields", fields)
        .rects("clearcut", clearcut)
        .surface(pcg_core::Surface::Heightfield { origin: hf.origin(), cell: hf.spacing(), width: hf.nx() as usize, heights: hf.heights().to_vec() });

    // Trees are drawn by `art::trees` from the `trunks` points (species mix per biome there).
    let (tree_r, forest_lo, forest_hi, rock_keep, bush_keep): (f32, f32, f32, f32, f32) = match w.def.biome {
        Biome::PineForest => (4.2, 0.3, 0.6, 0.25, 0.3),
        Biome::BirchHills => (6.0, 0.42, 0.7, 0.2, 0.45),
        Biome::Scrub => (9.0, 0.45, 0.75, 0.6, 0.6),
        Biome::Moor => (14.0, 0.6, 0.85, 0.5, 0.35),
        Biome::SnowForest => (4.6, 0.32, 0.6, 0.2, 0.15),
    };
    // Roadside props (`crate::props`): saplings and shrubs kept per biome.
    let (sapling_keep, shrub_keep): (f32, f32) = match w.def.biome {
        Biome::PineForest => (0.35, 0.3),
        Biome::BirchHills => (0.45, 0.35),
        Biome::Scrub => (0.15, 0.7),
        Biome::Moor => (0.12, 0.4),
        Biome::SnowForest => (0.3, 0.15),
    };
    let (rocks, bushes) = (crate::art::dressing::rocks(w.def.biome), crate::art::dressing::bushes(w.def.biome));
    let spawn = |items: &[(&str, f32)], tier: Tier, shadows: bool| {
        Op::Spawn(SpawnDef { kind: SpawnKind::Mesh, items: items.iter().map(|(n, w)| (n.to_string(), *w)).collect(), tier, shadows, ..Default::default() })
    };
    let g = Graph::build(w.def.seed)
        .input("area")
        .node("road", Op::Input { name: "road".into() }, &[])
        // Trees: forests by noise, clear of the road.
        .node("tree_pts", Op::PoissonSample { radius: tree_r }, &["area"])
        .node("forest", Op::DensityNoise { scale: 160.0, octaves: 3, lo: forest_lo, hi: forest_hi }, &["tree_pts"])
        .node("trees_in", Op::DensityFilter { lo: 0.15, hi: 1.0 }, &["forest"])
        .node("trees_clear", Op::Difference { against: "road".into(), pad: 4.5 }, &["trees_in"])
        // None above the treeline, on the valley fields or in the clear-cuts.
        .node("alpine", Op::Input { name: "alpine".into() }, &[])
        .node("fields", Op::Input { name: "fields".into() }, &[])
        .node("clearcut", Op::Input { name: "clearcut".into() }, &[])
        .node("trees_low", Op::Difference { against: "alpine".into(), pad: 0.0 }, &["trees_clear"])
        .node("trees_wood", Op::Difference { against: "fields".into(), pad: 0.0 }, &["trees_low"])
        .node("trees_cut", Op::Difference { against: "clearcut".into(), pad: 0.0 }, &["trees_wood"])
        .node("trees_kept", Op::Difference { against: "gardens".into(), pad: 0.0 }, &["trees_cut"])
        .node("trees_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.75, 1.35), jitter: 0.0 }, &["trees_kept"])
        // Stunted trees, few, above the treeline; single trees out on the fields.
        .node("alp_pts", Op::PoissonSample { radius: 24.0 }, &["area"])
        .node("alp_in", Op::Intersect { with: "alpine".into(), pad: 0.0 }, &["alp_pts"])
        .node("alp_keep", Op::Keep { fraction: 0.35 }, &["alp_in"])
        .node("alp_clear", Op::Difference { against: "road".into(), pad: 4.5 }, &["alp_keep"])
        .node("alp_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.4, 0.7), jitter: 0.0 }, &["alp_clear"])
        .node("field_pts", Op::PoissonSample { radius: 30.0 }, &["area"])
        .node("field_in", Op::Intersect { with: "fields".into(), pad: 0.0 }, &["field_pts"])
        .node("field_keep", Op::Keep { fraction: 0.4 }, &["field_in"])
        .node("field_clear", Op::Difference { against: "road".into(), pad: 6.0 }, &["field_keep"])
        .node("field_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.8, 1.25), jitter: 0.0 }, &["field_clear"])
        // Garden trees (smaller, a few to each yard), and the square's planted rows.
        .node("gardens", Op::Input { name: "gardens".into() }, &[])
        .node("garden_pts", Op::PoissonSample { radius: 9.0 }, &["gardens"])
        .node("garden_keep", Op::Keep { fraction: 0.75 }, &["garden_pts"])
        .node("garden_clear", Op::Difference { against: "road".into(), pad: 2.0 }, &["garden_keep"])
        .node("garden_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.55, 0.9), jitter: 0.0 }, &["garden_clear"])
        .node("planted", Op::Input { name: "planted".into() }, &[])
        .node("planted_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.7, 0.8), jitter: 0.0 }, &["planted"])
        .node("trees_all", Op::Union, &["trees_tf", "alp_tf", "field_tf", "garden_tf", "planted_tf"])
        .node("trees_on", Op::Project { offset: -0.1, align: false }, &["trees_all"])
        .node("trunks", Op::Emit { port: "trunks".into() }, &["trees_on"])
        // Boulders and rocks.
        .node("rock_pts", Op::PoissonSample { radius: 12.0 }, &["area"])
        .node("rock_keep", Op::Keep { fraction: rock_keep }, &["rock_pts"])
        .node("rock_clear", Op::Difference { against: "road".into(), pad: 2.2 }, &["rock_keep"])
        .node("rock_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.6, 1.6), jitter: 0.0 }, &["rock_clear"])
        // Bare rock above the treeline.
        .node("scree_pts", Op::PoissonSample { radius: 7.0 }, &["area"])
        .node("scree_in", Op::Intersect { with: "alpine".into(), pad: 0.0 }, &["scree_pts"])
        .node("scree_keep", Op::Keep { fraction: 0.55 }, &["scree_in"])
        .node("scree_clear", Op::Difference { against: "road".into(), pad: 2.2 }, &["scree_keep"])
        .node("scree_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.7, 2.2), jitter: 0.0 }, &["scree_clear"])
        .node("rock_all", Op::Union, &["rock_tf", "scree_tf"])
        .node("rock_on", Op::Project { offset: -0.25, align: true }, &["rock_all"])
        .node("rocks", spawn(&rocks, Tier::Essential, true), &["rock_on"])
        .node("boulders", Op::Emit { port: "boulders".into() }, &["rock_on"])
        // Stumps in the clear-cuts: solid (0.3 m), low.
        .node("stump_pts", Op::PoissonSample { radius: 5.5 }, &["area"])
        .node("stump_in", Op::Intersect { with: "clearcut".into(), pad: 0.0 }, &["stump_pts"])
        .node("stump_clear", Op::Difference { against: "road".into(), pad: 2.5 }, &["stump_in"])
        .node("stump_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.8, 1.3), jitter: 0.0 }, &["stump_clear"])
        .node("stump_on", Op::Project { offset: -0.05, align: true }, &["stump_tf"])
        .node("stump_mesh", spawn(&[("stump", 1.0)], Tier::Essential, true), &["stump_on"])
        .node("stumps", Op::Emit { port: "stumps".into() }, &["stump_on"])
        // Bushes and grass tufts: detail, thinned at low quality.
        .node("bush_pts", Op::PoissonSample { radius: 3.2 }, &["area"])
        .node("bush_keep", Op::Keep { fraction: bush_keep }, &["bush_pts"])
        // Ground cover only matters near the road (the camera never leaves it): a 90 m band.
        .node("bush_band", Op::Intersect { with: "road".into(), pad: 90.0 }, &["bush_keep"])
        .node("bush_clear", Op::Difference { against: "road".into(), pad: 1.0 }, &["bush_band"])
        .node("bush_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.6, 1.4), jitter: 0.0 }, &["bush_clear"])
        .node("bush_on", Op::Project { offset: -0.05, align: true }, &["bush_tf"])
        .node("bushes", spawn(&bushes, Tier::Fine, false), &["bush_on"])
        // Breakable and bendable props are drawn one by one by `art::props` (from
        // `StageWorld::props`), so they can fall over: emitted, not spawned. (Marker posts and
        // kilometre boards are placed along the road in `roadside`.)
        // Saplings 2.5–9 m off the edge: small enough to snap.
        .node("sap_pts", Op::PoissonSample { radius: 7.0 }, &["area"])
        .node("sap_band", Op::Intersect { with: "road".into(), pad: 9.0 }, &["sap_pts"])
        .node("sap_keep", Op::Keep { fraction: sapling_keep }, &["sap_band"])
        .node("sap_clear", Op::Difference { against: "road".into(), pad: 2.5 }, &["sap_keep"])
        .node("sap_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.25, 0.42), jitter: 0.0 }, &["sap_clear"])
        .node("sap_on", Op::Project { offset: -0.05, align: false }, &["sap_tf"])
        .node("saplings", Op::Emit { port: "saplings".into() }, &["sap_on"])
        // Shrubs right by the road (from 0.6 m off the edge): cut a corner and you're in them.
        .node("shrub_pts", Op::PoissonSample { radius: 3.5 }, &["area"])
        .node("shrub_band", Op::Intersect { with: "road".into(), pad: 6.0 }, &["shrub_pts"])
        .node("shrub_keep", Op::Keep { fraction: shrub_keep }, &["shrub_band"])
        .node("shrub_clear", Op::Difference { against: "road".into(), pad: 0.6 }, &["shrub_keep"])
        .node("shrub_tf", Op::Transform { offset: Vec3::ZERO, yaw: (0.0, 360.0), scale: (0.7, 1.3), jitter: 0.0 }, &["shrub_clear"])
        .node("shrub_on", Op::Project { offset: -0.05, align: false }, &["shrub_tf"])
        .node("shrubs", Op::Emit { port: "shrubs".into() }, &["shrub_on"])
        .finish()
        .expect("dressing graph");
    (g, inputs)
}

/// Only the tree part of [`dressing`] (same node ids and seed, so the same trunk points), for a
/// volume that covers the whole area unstreamed: far tree impostors need every cell's trees,
/// while rocks and bushes stream near the camera (`engine::pcg::PcgStreaming`).
pub fn tree_graph(w: &StageWorld) -> (Graph, Inputs) {
    let (full, inputs) = dressing(w);
    let keep = [
        "area", "road", "tree_pts", "forest", "trees_in", "trees_clear", "alpine", "fields", "clearcut", "trees_low", "trees_wood", "trees_cut", "gardens", "trees_kept", "trees_tf", "garden_pts", "garden_keep", "garden_clear", "garden_tf", "planted", "planted_tf", "alp_pts", "alp_in", "alp_keep", "alp_clear", "alp_tf", "field_pts", "field_in", "field_keep", "field_clear", "field_tf", "trees_all", "trees_on", "trunks",
    ];
    let mut g = full;
    g.nodes.retain(|n| keep.contains(&n.id.as_str()));
    (g, inputs)
}

/// Spawns the stage: terrain, road, dressing volume.
pub fn spawn(world: &mut World, s: &StageWorld) {
    let half = stage::half_size(&s.def);
    let desc = WorldDesc {
        seed: s.def.seed,
        terrain: engine::world::desc::TerrainDesc {
            size: [2.0 * half, 2.0 * half],
            spacing: stage::SPACING,
            // The open world's horizon is ~4 km away: 256 m chunks (128 quads) keep its draws
            // under the Low budget (docs/festival/PERF_REPORT.md action 1); stages keep 128 m.
            chunk_quads: if s.open.is_some() { 128 } else { 64 },
            origin: Some([-half, -half]),
            layers: crate::art::ground::layers(s.def.biome, crate::art::wet(&s.def)),
            ..default()
        },
        cell_size: 512.0,
        ..default()
    };
    // Editor sculpts of a level's terrain are saved back to its files with the level.
    let terrain_dir = s.open.as_ref().and_then(|o| o.terrain_dir.clone());
    world.insert_resource(engine::world::TerrainSaveTarget { dir: terrain_dir, heights_dirty: false, splat_dirty: false });
    if let Err(e) = engine::world::desc::spawn_world_with(world, desc, Some((*s.heights).clone())) {
        error!("terrain: {e}");
    }
    if s.open.is_some() {
        // Big chunks: allow LOD from 128 m (not the 256 m chunk size), so GPU cost stays flat.
        if let Some(mut ws) = world.get_resource_mut::<engine::world::WorldSettings>() {
            ws.terrain.lod_distance = 128.0;
        }
    }
    crate::art::spawn_stage(world, s);
    if let Some(o) = &s.open {
        crate::art::terrain::spawn_horizon(world, &o.heights, crate::open::roads::HALF, o.net.seed);
        crate::art::roadside::spawn(world, o);
        let hf = o.heights.clone();
        for e in crate::open::buildings::spawn(world, &o.buildings, &move |x, z| hf.height_at(x, z)) {
            if let Ok(mut e) = world.get_entity_mut(e) {
                e.insert(crate::open::live::LiveBuilding);
            }
        }
        spawn_parked(world, o);
        // Traffic shares its solids with the car's ground (the same Arc).
        world.insert_resource(o.traffic.clone());
        if crate::art::visual(world) {
            world.insert_resource(crate::open::traffic::Traffic::new(&o.net, crate::open::traffic::TrafficConfig::default()));
        }
    }
    world.insert_resource(s.clone());
}

/// Fernhollow's parked cars (`open::town::parked`; solid as walls in `OpenWorld`): traffic car
/// bodies standing still, in four looks only (two shapes, two paints) so their parts batch
/// into ~10 draws. Nothing in headless runs.
fn spawn_parked(world: &mut World, o: &crate::open::OpenWorld) {
    use crate::art::traffic_car::{TrafficArt, shape_of, spawn_parts};
    if !crate::art::visual(world) {
        return;
    }
    if !world.contains_resource::<TrafficArt>() {
        let art = world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| TrafficArt::new(&mut meshes, &mut world.resource_mut::<Assets<StandardMaterial>>()));
        world.insert_resource(art);
    }
    // A car id per look: shapes 0 and 1, the solid paint (id bit 3 clear).
    let id_for = |shape: usize| (1u32..10_000).find(|&id| shape_of(id) == shape && (id >> 3) & 1 == 0).unwrap_or(1);
    let looks = [(id_for(0), 1u8), (id_for(0), 4), (id_for(1), 1), (id_for(1), 4)];
    let parking: Vec<(Vec2, Vec2)> = o.buildings.blocks.iter().filter(|b| b.kind == crate::open::buildings::LotKind::Parking && b.cell.is_some()).map(|b| (b.min, b.max)).collect();
    let cars = crate::open::town::parked(&o.net, &o.buildings);
    world.resource_scope(|world, art: Mut<TrafficArt>| {
        let mut commands = world.commands();
        for c in &cars {
            let in_lot = parking.iter().any(|(lo, hi)| c.pos.cmpge(*lo).all() && c.pos.cmple(*hi).all());
            // On the car park's paving, or up on the sidewalk.
            let y = o.heights.height_at(c.pos.x, c.pos.y) + if in_lot { 0.04 } else { crate::open::junctions::SIDEWALK_TOP };
            let (id, paint) = looks[c.look as usize % 4];
            let at = Transform::from_translation(Vec3::new(c.pos.x, y, c.pos.y)).looking_to(Vec3::new(c.dir.x, 0.0, c.dir.y), Vec3::Y);
            commands
                .spawn((Name::new("parked car"), at, Visibility::default(), engine::render_budget::Scenery, crate::open::live::LiveBuilding, engine::level::GeneratedBy("pacenote::town (parked cars)")))
                .with_children(|p| spawn_parts(p, &art, id, paint));
        }
    });
    world.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_world_has_obstacles_near_the_road_and_none_on_it() {
        let def = &stage::stages()[0];
        let w = generate(def);
        let mut out = Vec::new();
        let mut total = 0;
        for i in (0..w.road.len()).step_by(25) {
            let p = w.road.pts[i];
            w.obstacles(Vec2::new(p.x, p.z), 30.0, &mut out);
            total += out.len();
            for o in &out {
                let rp = w.road.nearest(o.pos, 50.0).unwrap();
                // Trunks, boulders, posts and saplings clear of the road; shrubs may overhang
                // the edge (cutting a corner puts you in them) but stand beyond it.
                let clear = match o.give {
                    Give::Brush { .. } => rp.lateral.abs() > w.road.half_width(&rp) + 0.5,
                    _ => rp.lateral.abs() > w.road.half_width(&rp) + 1.0 - o.radius,
                };
                assert!(clear, "obstacle on the road at station {i}: {o:?}");
            }
        }
        assert!(total > 50, "only {total} obstacles near the road");
        // Terrain matches the road surface.
        let p = w.road.pts[w.road.len() / 2];
        assert!((w.height(p.x, p.z) - p.y).abs() < 0.3);
    }
}

#[cfg(test)]
mod debug {
    use super::*;

    /// How many points each dressing port emits along stage 1's road, and the props kept.
    #[test]
    #[ignore]
    fn dressing_ports() {
        let w = generate(&stage::stages()[0]);
        let (graph, inputs) = dressing(&w);
        let out = pcg_core::evaluate(&graph, &inputs, Some(road_area(&w.road, 40.0))).unwrap();
        for (port, p) in &out.emits {
            println!("{port}: {}", p.len());
        }
        for k in [PropKind::Post, PropKind::Sign, PropKind::Sapling, PropKind::Shrub] {
            println!("kept {k:?}: {}", w.props.iter().filter(|p| p.kind == k).count());
        }
    }
    use crate::vehicle::{Car, Input, Spec, step};

    #[test]
    #[ignore]
    fn drive_from_the_start() {
        let def = &stage::stages()[0];
        let w = generate(def);
        let (p, h) = w.road.start();
        let spec = Spec::default();
        let mut car = Car::new(p + Vec3::Y * 0.9, h);
        println!("start {p:?} heading {h:.2} ground {:.2} surface {:?}", w.height(p.x, p.z), w.surface(p.x, p.z));
        for i in 0..480 {
            let input = if i < 60 { Input::default() } else { Input { throttle: 1.0, ..Input::default() } };
            step(&mut car, &spec, input, &w, 1.0 / 60.0);
            if i % 40 == 0 {
                let wh = &car.wheels;
                println!(
                    "t {:.2} pos {:.1?} v {:5.1} gear {} rpm {:5.0} up {:.2} contact {:?} comp {:.2?} surf {:?} load {:.0?}",
                    i as f32 / 60.0,
                    car.pos,
                    car.speed(),
                    car.gear,
                    car.rpm,
                    car.up().y,
                    wh.iter().map(|w| w.contact).collect::<Vec<_>>(),
                    wh.iter().map(|w| w.compression).collect::<Vec<_>>(),
                    wh[0].surface,
                    wh.iter().map(|w| w.load).collect::<Vec<_>>()
                );
            }
        }
    }
}

#[cfg(test)]
mod start_tests {
    use super::*;
    use crate::vehicle::{Car, Input, Spec, step};

    /// Every stage: full throttle from the start line gets the car moving down the road.
    #[test]
    fn every_stage_drives_off_the_line() {
        let spec = Spec::default();
        for def in stage::stages() {
            let w = generate(&def);
            let (p, h) = w.road.start();
            let mut car = Car::new(p + Vec3::Y * 0.9, h);
            // Countdown on the handbrake, then 8 s with no pedals (the guide reading the first
            // notes): hill hold keeps the car on the line.
            for i in 0..(60 * 9) {
                let input = if i < 60 { Input { handbrake: true, ..Input::default() } } else { Input::default() };
                step(&mut car, &spec, input, &w, 1.0 / 60.0);
            }
            let drift = (car.pos - p).length() - 0.9;
            assert!(drift.abs() < 0.5, "stage {} {}: rolled {drift:.2} m while idle", def.id, def.name);
            let p = car.pos;
            for _ in 0..150 {
                step(&mut car, &spec, Input { throttle: 1.0, ..Input::default() }, &w, 1.0 / 60.0);
            }
            let moved = (car.pos - p).length();
            // 2.5 s of full throttle with no steering (longer and some starts' first bend puts
            // the car in the trees): about 30–60 km/h depending on surface and slope.
            assert!(car.speed() * 3.6 > 20.0 && moved > 10.0, "stage {} {}: {:.1} km/h after 2.5 s, moved {moved:.1} m", def.id, def.name, car.speed() * 3.6);
        }
    }
}

#[cfg(test)]
mod idle_tests {
    use super::*;
    use crate::vehicle::{Car, Input, Spec, step};

    #[test]
    #[ignore]
    fn stage5_idle_then_throttle() {
        let spec = Spec::default();
        let def = &stage::stages()[4];
        let w = generate(def);
        let (p, h) = w.road.start();
        let mut car = Car::new(p + Vec3::Y * 0.9, h);
        for i in 0..(60 * 30) {
            let t = i as f32 / 60.0;
            let input = if t < 3.0 { Input { handbrake: true, ..Input::default() } } else if t < 12.0 { Input::default() } else { Input { throttle: 1.0, ..Input::default() } };
            step(&mut car, &spec, input, &w, 1.0 / 60.0);
            if i % 60 == 0 {
                let wh = &car.wheels;
                println!("t {t:4.1} up {:.2} pos {:.2?} v {:5.2} gear {} rpm {:5.0} spin {:.1?} contact {:?} surf {:?} slip {:.2?}",
                    car.up().y, car.pos - p, car.speed() * 3.6, car.gear, car.rpm, wh.iter().map(|w| w.spin).collect::<Vec<_>>(), wh.iter().map(|w| w.contact).collect::<Vec<_>>(), wh[0].surface, wh.iter().map(|w| w.slip_ratio).collect::<Vec<_>>());
            }
        }
    }
}
