//! The shortest way along the road network from one point on it to another: what the map's
//! route line follows to a picked event. Pure (no Bevy world), so it's tested here.
//!
//! The graph's nodes are the ends of every road and every station where a junction joins it;
//! edges run along a road between neighbouring nodes (both ways: every road is two-way) and
//! across each junction between the roads meeting there.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use bevy::math::Vec2;

use crate::open::RoadNet;
use crate::stage::STEP;

/// A point on the network: road index and station.
pub type At = (usize, usize);

#[derive(Clone, Copy, Debug)]
struct Edge {
    to: u32,
    cost: f32,
    /// Along a road: (road, from station, to station, direction ±1); `None`: across a junction.
    along: Option<(u32, u32, u32, i8)>,
}

/// The road network as a graph.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    nodes: Vec<At>,
    adj: Vec<Vec<Edge>>,
    /// Per road: its nodes' stations, sorted, with their node ids.
    keys: Vec<Vec<(usize, u32)>>,
    /// Per road: station count and whether it's a loop.
    shape: Vec<(usize, bool)>,
}

/// Stations from `a` to `b` going `dir` (±1) on a road of `n` stations (wrapping on a loop).
fn span(a: usize, b: usize, dir: i8, n: usize, closed: bool) -> usize {
    match (dir > 0, closed) {
        (true, true) => (b + n - a) % n,
        (false, true) => (a + n - b) % n,
        (true, false) => b.saturating_sub(a),
        (false, false) => a.saturating_sub(b),
    }
}

impl Graph {
    pub fn build(net: &RoadNet) -> Graph {
        let mut g = Graph { shape: net.roads.iter().map(|r| (r.road.len(), r.closed)).collect(), ..Graph::default() };
        let mut stations: Vec<Vec<usize>> = net.roads.iter().map(|r| if r.closed { vec![0] } else { vec![0, r.road.len().saturating_sub(1)] }).collect();
        for j in &net.junctions {
            for &(r, s) in &j.roads {
                stations[r].push(s);
            }
        }
        for (r, st) in stations.iter_mut().enumerate() {
            st.sort_unstable();
            st.dedup();
            let keys: Vec<(usize, u32)> = st
                .iter()
                .map(|&s| {
                    g.nodes.push((r, s));
                    g.adj.push(Vec::new());
                    (s, (g.nodes.len() - 1) as u32)
                })
                .collect();
            g.keys.push(keys);
        }
        for r in 0..g.keys.len() {
            let (n, closed) = g.shape[r];
            let keys = g.keys[r].clone();
            let pairs = keys.len().saturating_sub(1) + usize::from(closed && keys.len() > 1);
            for k in 0..pairs {
                let (a, na) = keys[k];
                let (b, nb) = keys[(k + 1) % keys.len()];
                let steps = span(a, b, 1, n, closed);
                let cost = steps as f32 * STEP;
                g.adj[na as usize].push(Edge { to: nb, cost, along: Some((r as u32, a as u32, b as u32, 1)) });
                g.adj[nb as usize].push(Edge { to: na, cost, along: Some((r as u32, b as u32, a as u32, -1)) });
            }
        }
        for j in &net.junctions {
            let ids: Vec<u32> = j.roads.iter().filter_map(|&(r, s)| g.keys[r].iter().find(|k| k.0 == s).map(|k| k.1)).collect();
            for &a in &ids {
                for &b in &ids {
                    if a != b {
                        g.adj[a as usize].push(Edge { to: b, cost: 0.0, along: None });
                    }
                }
            }
        }
        g
    }

    /// Edges from a point on a road to the nodes either side of it (and none if the road has
    /// no nodes, which can't happen for a built graph).
    fn exits(&self, (r, i): At) -> Vec<Edge> {
        let (n, closed) = self.shape[r];
        let keys = &self.keys[r];
        let mut out = Vec::new();
        let next = keys.iter().find(|k| k.0 >= i).or(if closed { keys.first() } else { None });
        let prev = keys.iter().rev().find(|k| k.0 <= i).or(if closed { keys.last() } else { None });
        if let Some(&(s, id)) = next {
            out.push(Edge { to: id, cost: span(i, s, 1, n, closed) as f32 * STEP, along: Some((r as u32, i as u32, s as u32, 1)) });
        }
        if let Some(&(s, id)) = prev {
            out.push(Edge { to: id, cost: span(i, s, -1, n, closed) as f32 * STEP, along: Some((r as u32, i as u32, s as u32, -1)) });
        }
        out
    }

    /// The way from `from` to `to` along the roads: the road points in order (x, z), and its
    /// length (m). `None` when they aren't connected.
    pub fn route(&self, net: &RoadNet, from: At, to: At) -> Option<(Vec<Vec2>, f32)> {
        if from.0 >= self.keys.len() || to.0 >= self.keys.len() {
            return None;
        }
        // Two extra nodes: the start (n) and the target (n + 1).
        let n = self.nodes.len();
        let (start, goal) = (n as u32, n as u32 + 1);
        let mut extra: Vec<Vec<Edge>> = vec![self.exits(from), Vec::new()];
        // Into the target from the nodes either side of it.
        let mut into: Vec<(u32, Edge)> = Vec::new();
        for e in self.exits(to) {
            let (r, a, b, d) = e.along.expect("exits run along the road");
            into.push((e.to, Edge { to: goal, cost: e.cost, along: Some((r, b, a, -d)) }));
        }
        // On the same road: straight there (either way round a loop).
        if from.0 == to.0 {
            let (len, closed) = self.shape[from.0];
            for dir in [1i8, -1] {
                if !closed && (dir > 0) != (to.1 >= from.1) {
                    continue;
                }
                let steps = span(from.1, to.1, dir, len, closed);
                extra[0].push(Edge { to: goal, cost: steps as f32 * STEP, along: Some((from.0 as u32, from.1 as u32, to.1 as u32, dir)) });
            }
        }
        let edges = |v: u32, out: &mut Vec<Edge>| {
            out.clear();
            if v < n as u32 {
                out.extend_from_slice(&self.adj[v as usize]);
            } else {
                out.extend_from_slice(&extra[(v - n as u32) as usize]);
            }
            out.extend(into.iter().filter(|(a, _)| *a == v).map(|(_, e)| *e));
        };

        #[derive(PartialEq)]
        struct Item(f32, u32);
        impl Eq for Item {}
        impl Ord for Item {
            fn cmp(&self, o: &Self) -> Ordering {
                o.0.total_cmp(&self.0).then(self.1.cmp(&o.1))
            }
        }
        impl PartialOrd for Item {
            fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
                Some(self.cmp(o))
            }
        }
        let mut dist = vec![f32::INFINITY; n + 2];
        let mut back: Vec<Option<(u32, Edge)>> = vec![None; n + 2];
        let mut heap = BinaryHeap::new();
        dist[start as usize] = 0.0;
        heap.push(Item(0.0, start));
        let mut buf = Vec::new();
        while let Some(Item(d, v)) = heap.pop() {
            if v == goal {
                break;
            }
            if d > dist[v as usize] {
                continue;
            }
            edges(v, &mut buf);
            for e in &buf {
                let nd = d + e.cost;
                if nd < dist[e.to as usize] {
                    dist[e.to as usize] = nd;
                    back[e.to as usize] = Some((v, *e));
                    heap.push(Item(nd, e.to));
                }
            }
        }
        if !dist[goal as usize].is_finite() {
            return None;
        }
        // Walk back, then lay out the road points of each leg.
        let mut legs = Vec::new();
        let mut v = goal;
        while let Some((u, e)) = back[v as usize] {
            legs.push(e);
            v = u;
        }
        legs.reverse();
        let mut pts: Vec<Vec2> = Vec::new();
        for e in legs {
            let Some((r, a, b, d)) = e.along else { continue };
            let road = &net.roads[r as usize].road;
            let (len, closed) = self.shape[r as usize];
            let steps = span(a as usize, b as usize, d, len, closed);
            for k in 0..=steps {
                let i = if d > 0 { (a as usize + k) % len } else { (a as usize + len * 2 - k) % len };
                let p = road.pts[i];
                let p = Vec2::new(p.x, p.z);
                if pts.last().is_none_or(|q| q.distance_squared(p) > 0.01) {
                    pts.push(p);
                }
            }
        }
        Some((pts, dist[goal as usize]))
    }
}

/// The station of the network nearest `p` (any distance away): every station, checked.
pub fn nearest(net: &RoadNet, p: Vec2) -> Option<At> {
    let mut best: Option<(f32, At)> = None;
    for (r, nr) in net.roads.iter().enumerate() {
        for (i, q) in nr.road.pts.iter().enumerate() {
            let d = Vec2::new(q.x, q.z).distance_squared(p);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, (r, i)));
            }
        }
    }
    best.map(|b| b.1)
}

/// Drops points that lie within `tol` (m) of the line through their neighbours
/// (Ramer–Douglas–Peucker), keeping the ends.
pub fn simplify(pts: &[Vec2], tol: f32) -> Vec<Vec2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (pts[a], pts[b]);
        let ab = pb - pa;
        let l2 = ab.length_squared().max(1e-6);
        let (mut worst, mut at) = (0.0, a);
        for (k, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let t = ((*p - pa).dot(ab) / l2).clamp(0.0, 1.0);
            let d = p.distance(pa + ab * t);
            if d > worst {
                worst = d;
                at = k;
            }
        }
        if worst > tol {
            keep[at] = true;
            stack.push((a, at));
            stack.push((at, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// Length (m) of a polyline.
pub fn length(pts: &[Vec2]) -> f32 {
    pts.windows(2).map(|w| w[0].distance(w[1])).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open::{SEED, roads};

    #[test]
    fn routes_follow_the_roads_between_any_two_points() {
        let net = roads::generate(SEED);
        let g = Graph::build(&net);
        let ends: Vec<At> = net.roads.iter().enumerate().map(|(r, nr)| (r, nr.road.len() / 2)).collect();
        let from = ends[0];
        for &to in &ends {
            let (pts, len) = g.route(&net, from, to).unwrap_or_else(|| panic!("no way from {from:?} to {to:?}"));
            let p0 = net.roads[from.0].road.pts[from.1];
            let p1 = net.roads[to.0].road.pts[to.1];
            assert!(pts[0].distance(Vec2::new(p0.x, p0.z)) < 0.1, "starts at the start");
            assert!(pts.last().unwrap().distance(Vec2::new(p1.x, p1.z)) < 0.1, "ends at the target");
            // No jumps bigger than a junction's join between points.
            for w in pts.windows(2) {
                assert!(w[0].distance(w[1]) < 3.0 * STEP, "a gap of {:.1} m on the way to {to:?}", w[0].distance(w[1]));
            }
            let drawn = length(&pts);
            assert!((drawn - len).abs() < len * 0.1 + 20.0, "length {len} vs drawn {drawn}");
            assert!(len + 1.0 >= Vec2::new(p0.x, p0.z).distance(Vec2::new(p1.x, p1.z)), "never shorter than straight");
        }
    }

    #[test]
    fn same_road_is_direct() {
        let net = roads::generate(SEED);
        let g = Graph::build(&net);
        let r = (0..net.roads.len()).find(|&r| !net.roads[r].closed && net.roads[r].road.len() > 200).unwrap();
        let (_, len) = g.route(&net, (r, 20), (r, 120)).unwrap();
        assert!((len - 100.0 * STEP).abs() < 1.0, "{len}");
    }

    #[test]
    fn simplify_keeps_corners() {
        let pts: Vec<Vec2> = (0..=10).map(|i| Vec2::new(i as f32, 0.0)).chain((1..=10).map(|i| Vec2::new(10.0, i as f32))).collect();
        let s = simplify(&pts, 0.1);
        assert_eq!(s, vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)]);
    }
}
