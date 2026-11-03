//! Straight walls ([`Wall`] segments) on a grid, for quick "near the car" lookups.
//!
//! A wall goes into every cell its box (grown by its radius) touches, so one longer than a
//! cell is found from any of them, and a query hands it out once. Building facades use it
//! now; bridge parapets and barriers go in the same list (see [`super::OpenWorld::from_data_with`]).

use bevy::math::Vec2;

use crate::vehicle::Wall;

#[derive(Clone, Debug, Default)]
pub struct WallGrid {
    lo: Vec2,
    cell: f32,
    nx: usize,
    nz: usize,
    walls: Vec<Wall>,
    /// Per wall, the cells it's in: (i0, j0, i1, j1), inclusive.
    spans: Vec<[usize; 4]>,
    /// Per cell, indices into `walls` in insertion order.
    cells: Vec<Vec<u32>>,
}

impl WallGrid {
    /// Buckets `walls` into `cell`-metre cells.
    pub fn new(walls: &[Wall], cell: f32) -> WallGrid {
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for w in walls {
            lo = lo.min(w.a.min(w.b) - Vec2::splat(w.radius));
            hi = hi.max(w.a.max(w.b) + Vec2::splat(w.radius));
        }
        if walls.is_empty() {
            (lo, hi) = (Vec2::ZERO, Vec2::ZERO);
        }
        let nx = ((hi.x - lo.x) / cell).floor() as usize + 1;
        let nz = ((hi.y - lo.y) / cell).floor() as usize + 1;
        let mut g = WallGrid { lo, cell, nx, nz, walls: walls.to_vec(), spans: Vec::with_capacity(walls.len()), cells: vec![Vec::new(); nx * nz] };
        for (k, w) in walls.iter().enumerate() {
            let (a, b) = (g.index(w.a.min(w.b) - Vec2::splat(w.radius)), g.index(w.a.max(w.b) + Vec2::splat(w.radius)));
            for j in a.1..=b.1 {
                for i in a.0..=b.0 {
                    g.cells[j * nx + i].push(k as u32);
                }
            }
            g.spans.push([a.0, a.1, b.0, b.1]);
        }
        g
    }

    /// The cell holding `p`, clamped to the grid.
    fn index(&self, p: Vec2) -> (usize, usize) {
        let f = (p - self.lo) / self.cell;
        ((f.x.floor().max(0.0) as usize).min(self.nx - 1), (f.y.floor().max(0.0) as usize).min(self.nz - 1))
    }

    /// Appends (doesn't clear) every wall whose surface comes within `r` of `p`, each once, in
    /// a fixed order (cells row by row, then insertion order).
    pub fn query(&self, p: Vec2, r: f32, out: &mut Vec<Wall>) {
        if self.walls.is_empty() {
            return;
        }
        let hi = p + Vec2::splat(r);
        if hi.x < self.lo.x || hi.y < self.lo.y {
            return;
        }
        let (i0, j0) = self.index(p - Vec2::splat(r));
        let (i1, j1) = self.index(hi);
        for j in j0..=j1 {
            for i in i0..=i1 {
                for &k in &self.cells[j * self.nx + i] {
                    // Only from the first cell the wall and the query share.
                    let s = self.spans[k as usize];
                    if i != s[0].max(i0) || j != s[1].max(j0) {
                        continue;
                    }
                    let w = &self.walls[k as usize];
                    if w.distance(p) < r {
                        out.push(*w);
                    }
                }
            }
        }
    }

    /// Every wall, in insertion order.
    pub fn walls(&self) -> &[Wall] {
        &self.walls
    }

    pub fn len(&self) -> usize {
        self.walls.len()
    }

    pub fn is_empty(&self) -> bool {
        self.walls.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_wall_is_found_from_every_cell_once() {
        // 100 m diagonal across 10 m cells, and a short one beside it.
        let walls = [Wall::new(Vec2::new(0.0, 0.0), Vec2::new(70.0, 70.0), 0.3, 5.0), Wall::new(Vec2::new(50.0, 0.0), Vec2::new(52.0, 0.0), 0.3, 5.0)];
        let g = WallGrid::new(&walls, 10.0);
        let mut out = Vec::new();
        for s in 0..=70 {
            let p = Vec2::splat(s as f32) + Vec2::new(1.0, -1.0);
            out.clear();
            g.query(p, 2.0, &mut out);
            assert_eq!(out, vec![walls[0]], "at {p}");
        }
        out.clear();
        g.query(Vec2::new(51.0, 1.0), 1.0, &mut out);
        assert_eq!(out, vec![walls[1]]);
        out.clear();
        g.query(Vec2::new(-30.0, 40.0), 5.0, &mut out);
        assert!(out.is_empty());
    }
}
