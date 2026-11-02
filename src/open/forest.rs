//! Every tree trunk and boulder in the open world is solid, not just the ones near a road.
//!
//! The scenery graph (`world::dressing`) is evaluated on 128 m tiles. Tiles a road passes
//! through are built while the world loads. The rest are built in the background around the
//! car (`ForestPlugin`). If the car reaches a tile that isn't ready, the sim builds it on the
//! spot. A tile's trunks are a pure function of the tile, so the car hits the same trees on
//! every machine and ghosts replay exactly.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use pcg_core::{Area, Graph, Inputs};

use crate::vehicle::Obstacle;

/// Tile side (m): what one graph evaluation covers.
pub const TILE: f32 = 128.0;
/// Grid cell inside a tile (m), for quick lookups (16 per tile side).
const CELL: f32 = 8.0;
const CELLS: i32 = (TILE / CELL) as i32;
/// How far around the car tiles are built ahead of time (m).
const REACH: f32 = 384.0;

/// One tile's solids, by 8 m cell (global cell coordinates).
type Tile = HashMap<(i32, i32), Vec<Obstacle>>;

pub struct ForestSolids {
    graph: Graph,
    inputs: Inputs,
    /// Hand edits (the level's tree and dressing `PcgOverrides`), as the volumes apply them.
    trees: Option<engine::pcg::OverrideIndex>,
    dressing: Option<engine::pcg::OverrideIndex>,
    tiles: RwLock<HashMap<(i32, i32), Arc<Tile>>>,
    pending: Mutex<HashSet<(i32, i32)>>,
}

impl ForestSolids {
    pub fn new(graph: Graph, inputs: Inputs) -> ForestSolids {
        ForestSolids::with_overrides(graph, inputs, &Default::default(), &Default::default())
    }

    /// With the hand edits of the trees (`trunks`) and the dressing (`boulders`).
    pub fn with_overrides(graph: Graph, inputs: Inputs, trees: &engine::pcg::PcgOverrides, dressing: &engine::pcg::PcgOverrides) -> ForestSolids {
        let ix = |o: &engine::pcg::PcgOverrides| (!o.is_empty()).then(|| o.index(TILE));
        ForestSolids { graph, inputs, trees: ix(trees), dressing: ix(dressing), tiles: RwLock::default(), pending: Mutex::default() }
    }

    pub fn tile_of(p: Vec2) -> (i32, i32) {
        ((p.x / TILE).floor() as i32, (p.y / TILE).floor() as i32)
    }

    fn in_world(t: (i32, i32)) -> bool {
        let n = (super::roads::HALF / TILE).ceil() as i32;
        (-n..n).contains(&t.0) && (-n..n).contains(&t.1)
    }

    /// Evaluates one tile: trunks and boulders inside it, sorted (the same on every machine).
    fn build(&self, t: (i32, i32)) -> Tile {
        let mut tile = Tile::new();
        if !Self::in_world(t) {
            return tile;
        }
        let a = Area::new(Vec2::new(t.0 as f32, t.1 as f32) * TILE, Vec2::new(t.0 as f32 + 1.0, t.1 as f32 + 1.0) * TILE);
        let Ok(out) = pcg_core::evaluate(&self.graph, &self.inputs, Some(a)) else { return tile };
        for (port, radius, edits) in [("trunks", 0.32f32, &self.trees), ("boulders", 0.85, &self.dressing)] {
            let Some(p) = out.emits.get(port) else { continue };
            let edited;
            let p = match edits {
                Some(ix) => {
                    let mut q = p.clone();
                    ix.apply_points(port, a, &mut q);
                    edited = q;
                    &edited
                }
                None => p,
            };
            for k in 0..p.len() {
                let c = Vec2::new(p.pos[k].x, p.pos[k].z);
                // Only this tile's own points (a neighbour finds the rest).
                if c.x < a.min.x || c.y < a.min.y || c.x >= a.max.x || c.y >= a.max.y {
                    continue;
                }
                let cell = ((c.x / CELL).floor() as i32, (c.y / CELL).floor() as i32);
                tile.entry(cell).or_default().push(Obstacle::solid(c, radius * p.scale[k].x, p.pos[k].y + 4.0 * p.scale[k].y));
            }
        }
        for v in tile.values_mut() {
            v.sort_by(|a, b| a.pos.x.total_cmp(&b.pos.x).then(a.pos.y.total_cmp(&b.pos.y)).then(a.radius.total_cmp(&b.radius)));
            v.dedup_by(|a, b| a.pos == b.pos);
        }
        tile
    }

    fn store(&self, t: (i32, i32), tile: Tile) -> Arc<Tile> {
        let mut tiles = self.tiles.write().unwrap_or_else(|e| e.into_inner());
        tiles.entry(t).or_insert_with(|| Arc::new(tile)).clone()
    }

    fn get(&self, t: (i32, i32)) -> Option<Arc<Tile>> {
        self.tiles.read().unwrap_or_else(|e| e.into_inner()).get(&t).cloned()
    }

    /// The tile, built now if it isn't ready.
    fn ensure(&self, t: (i32, i32)) -> Arc<Tile> {
        if let Some(tile) = self.get(t) {
            return tile;
        }
        let tile = self.build(t);
        self.store(t, tile)
    }

    /// Builds these tiles on all cores (at load).
    pub fn fill(&self, tiles: &[(i32, i32)]) {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
        let chunk = tiles.len().div_ceil(threads).max(1);
        std::thread::scope(|sc| {
            for part in tiles.chunks(chunk) {
                sc.spawn(move || {
                    for &t in part {
                        let tile = self.build(t);
                        self.store(t, tile);
                    }
                });
            }
        });
    }

    /// The solids within `r` of `p` (appended to `out`).
    pub fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
        let (x0, x1) = (((p.x - r) / CELL).floor() as i32, ((p.x + r) / CELL).floor() as i32);
        let (z0, z1) = (((p.y - r) / CELL).floor() as i32, ((p.y + r) / CELL).floor() as i32);
        let mut held: Option<((i32, i32), Arc<Tile>)> = None;
        for z in z0..=z1 {
            for x in x0..=x1 {
                let t = (x.div_euclid(CELLS), z.div_euclid(CELLS));
                if held.as_ref().is_none_or(|h| h.0 != t) {
                    held = Some((t, self.ensure(t)));
                }
                if let Some(v) = held.as_ref().and_then(|h| h.1.get(&(x, z))) {
                    out.extend_from_slice(v);
                }
            }
        }
    }

    /// Starts building the missing tiles within [`REACH`] of `p` in the background.
    pub fn prefetch(self: &Arc<Self>, p: Vec2) {
        let n = (REACH / TILE).ceil() as i32;
        let c = Self::tile_of(p);
        let mut want: Vec<(i32, i32)> = Vec::new();
        {
            let tiles = self.tiles.read().unwrap_or_else(|e| e.into_inner());
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            for dz in -n..=n {
                for dx in -n..=n {
                    let t = (c.0 + dx, c.1 + dz);
                    if Self::in_world(t) && !tiles.contains_key(&t) && pending.insert(t) {
                        want.push(t);
                    }
                }
            }
        }
        // Nearest first.
        want.sort_by_key(|t| (t.0 - c.0).pow(2) + (t.1 - c.1).pow(2));
        let pool = AsyncComputeTaskPool::get();
        for t in want {
            let me = self.clone();
            pool.spawn(async move {
                let tile = me.build(t);
                me.store(t, tile);
                me.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&t);
            })
            .detach();
        }
    }

    /// Every solid in the tiles built so far.
    pub fn built_solids(&self) -> Vec<Obstacle> {
        self.tiles.read().unwrap_or_else(|e| e.into_inner()).values().flat_map(|t| t.values().flatten().copied()).collect()
    }

    /// Tiles built so far.
    pub fn built(&self) -> usize {
        self.tiles.read().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// Keeps the tiles around the car built.
pub struct ForestPlugin;

impl Plugin for ForestPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, prefetch_around_car.run_if(resource_exists::<crate::world::StageWorld>));
        #[cfg(feature = "dev")]
        dev::register(app);
    }
}

#[cfg(feature = "dev")]
mod dev {
    use super::*;
    use engine::devtools::{AddControlCommand, params};
    use serde_json::json;

    pub fn register(app: &mut App) {
        app.add_control_command(
            "forest.aim",
            "Put the car 25 m from a trunk well off the roads, rolling at it at 54 km/h (collision checks)",
            params(&[]),
            |w, _| {
                let stage = w.get_resource::<crate::world::StageWorld>().ok_or("no stage")?.clone();
                let forest = stage.forest.clone().ok_or("not the open world")?;
                let net = stage.open.as_ref().ok_or("not the open world")?.net.clone();
                let mut q = w.query::<&mut crate::drive::DrivenCar>();
                let at = q.iter(w).next().map(|c| Vec2::new(c.car.pos.x, c.car.pos.z)).ok_or("no car")?;
                // The nearest big trunk 80 m+ from any road, with 30 m clear in front of it.
                let mut best: Option<(f32, Obstacle)> = None;
                for r in [200.0f32, 500.0, 1000.0] {
                    let mut out = Vec::new();
                    forest.obstacles(at, r, &mut out);
                    for o in &out {
                        if o.radius < 0.25 || net.nearest(o.pos, 80.0).is_some() {
                            continue;
                        }
                        let d = o.pos.distance(at);
                        if best.is_none_or(|b| d < b.0) {
                            best = Some((d, *o));
                        }
                    }
                    if best.is_some() {
                        break;
                    }
                }
                let (_, tree) = best.ok_or("no trunk off the roads nearby")?;
                // Approach from whichever side has the fewest other trunks in the way.
                let mut around = Vec::new();
                forest.obstacles(tree.pos, 30.0, &mut around);
                let (dir, _) = (0..16)
                    .map(|k| {
                        let a = k as f32 / 16.0 * std::f32::consts::TAU;
                        let d = Vec2::new(a.sin(), a.cos());
                        let blocked = around.iter().filter(|o| o.pos != tree.pos && {
                            let rel = o.pos - tree.pos;
                            let along = rel.dot(d);
                            along > 0.0 && along < 28.0 && (rel - d * along).length() < 2.0 + o.radius
                        }).count();
                        (d, blocked)
                    })
                    .min_by_key(|x| x.1)
                    .unwrap();
                let start = tree.pos + dir * 25.0;
                let h = crate::vehicle::Ground::height(&stage, start.x, start.y);
                let toward = -dir;
                let heading = toward.x.atan2(toward.y);
                let wheel = w.get_resource::<crate::drive::CarSpec>().map_or(0.32, |s| s.0.wheel_radius);
                let mut q = w.query::<&mut crate::drive::DrivenCar>();
                for mut c in q.iter_mut(w) {
                    crate::vehicle::recover(&mut c.car, Vec3::new(start.x, h, start.y), heading);
                    c.car.vel = c.car.forward() * 15.0;
                    for wh in &mut c.car.wheels {
                        wh.spin = 15.0 / wheel;
                    }
                    c.prev = (c.car.pos, c.car.rot);
                }
                Ok(json!({ "tree": [tree.pos.x, tree.pos.y], "radius": tree.radius, "from": [start.x, start.y], "built_tiles": forest.built() }))
            },
        );
    }
}

fn prefetch_around_car(stage: Res<crate::world::StageWorld>, cars: Query<&crate::drive::DrivenCar>, mut last: Local<Option<(usize, (i32, i32))>>) {
    let Some(forest) = stage.forest.as_ref() else { return };
    let Ok(c) = cars.single() else { return };
    let p = Vec2::new(c.car.pos.x, c.car.pos.z);
    // (Keyed on the forest too: a new world starts over.)
    let key = (Arc::as_ptr(forest) as usize, ForestSolids::tile_of(p));
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    forest.prefetch(p);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::Ground;

    /// A no-tree area painted in the level (the trees' `PcgOverrides`) clears the trunks there
    /// for the car too; the boulders (dressing) stay.
    #[test]
    fn a_cleared_area_has_no_trunks() {
        use engine::pcg::{OverrideArea, PcgOverrides};
        let w = crate::world::generate(&super::super::def());
        let (graph, inputs) = crate::world::dressing(&w);
        let plain = ForestSolids::new(graph.clone(), inputs.clone());
        // A tile away from the roads with trunks in it.
        let net = &w.open.as_ref().unwrap().net;
        let t = (-15..15)
            .flat_map(|z| (-15..15).map(move |x| (x, z)))
            .find(|&(x, z)| net.nearest(Vec2::new(x as f32 + 0.5, z as f32 + 0.5) * TILE, 100.0).is_none() && plain.build((x, z)).values().flatten().any(|o| o.radius < 0.6))
            .expect("a tile of forest");
        let c = Vec2::new(t.0 as f32 + 0.5, t.1 as f32 + 0.5) * TILE;
        let square = vec![c + Vec2::new(-20.0, -20.0), c + Vec2::new(20.0, -20.0), c + Vec2::new(20.0, 20.0), c + Vec2::new(-20.0, 20.0)];
        let trees = PcgOverrides { areas: vec![OverrideArea { polygon: square, density: 0.0, nodes: None }], ..default() };
        let edited = ForestSolids::with_overrides(graph, inputs, &trees, &PcgOverrides::default());
        let inside = |tile: &Tile, trunk: bool| tile.values().flatten().filter(|o| (o.pos - c).abs().max_element() < 19.0 && (o.radius < 0.6) == trunk).count();
        let (before, after) = (plain.build(t), edited.build(t));
        eprintln!("trunks in the square: {} -> {}; boulders {} -> {}", inside(&before, true), inside(&after, true), inside(&before, false), inside(&after, false));
        assert!(inside(&before, true) > 0, "no trunks to clear at {c}");
        assert_eq!(inside(&after, true), 0);
        assert_eq!(inside(&after, false), inside(&before, false));
        assert!(after.values().flatten().count() > 0, "the rest of the tile keeps its trees");
    }

    /// Trees well away from any road are solid too, and a tile builds the same twice.
    #[test]
    fn trees_off_the_road_are_solid() {
        let w = crate::world::generate(&super::super::def());
        let forest = w.forest.as_ref().expect("open world has a forest");
        let net = &w.open.as_ref().unwrap().net;
        let loaded = forest.built();
        // A point 100 m+ from every road with trunks around it.
        let mut found = None;
        'search: for zi in -15..15 {
            for xi in -15..15 {
                let p = Vec2::new(xi as f32 * 128.0 + 64.0, zi as f32 * 128.0 + 64.0);
                if net.nearest(p, 100.0).is_some() {
                    continue;
                }
                let mut out = Vec::new();
                w.obstacles(p, 40.0, &mut out);
                if !out.is_empty() {
                    found = Some((p, out));
                    break 'search;
                }
            }
        }
        let (p, out) = found.expect("no trees found away from the roads");
        assert!(forest.built() > loaded, "the query built the tile");
        let t = std::time::Instant::now();
        let again = forest.build(ForestSolids::tile_of(p));
        eprintln!("one tile: {:.0} ms, {} solids near {p}", t.elapsed().as_secs_f32() * 1000.0, out.len());
        let mut a: Vec<Obstacle> = again.values().flatten().copied().collect();
        let mut b: Vec<Obstacle> = forest.get(ForestSolids::tile_of(p)).unwrap().values().flatten().copied().collect();
        let key = |o: &Obstacle| (o.pos.x.to_bits(), o.pos.y.to_bits());
        a.sort_by_key(key);
        b.sort_by_key(key);
        assert_eq!(a, b);
    }
}
