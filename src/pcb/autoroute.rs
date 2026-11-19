//! Grid router (Lee/A* with 45° moves and through vias) on all copper layers of the board.
//! The autorouter routes one ratsnest connection at a time, shortest first; the same search
//! gives the interactive router its walkaround path. Widths, clearances and vias follow the
//! net classes; what cannot be routed stays in the ratsnest.

use super::board::{Board, Copper, Item, Track, Via, B_CU, F_CU};
use super::geom::{p, Hull, P};
use crate::slicer::poly;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

const FREE: i32 = -1;
const WALL: i32 = -2;

#[derive(Debug, Default, Clone)]
pub struct Report {
    pub routed: usize,
    pub failed: Vec<String>,
    pub vias: usize,
}

/// Copper layers in stacking order (top, inner …, bottom).
pub fn stack(b: &Board) -> Vec<u8> {
    if b.layers >= 4 {
        vec![F_CU, 2, 3, B_CU]
    } else {
        vec![F_CU, B_CU]
    }
}

pub struct Grid {
    lo: P,
    g: f64,
    nx: usize,
    ny: usize,
    /// copper layer of each grid layer
    lays: Vec<u8>,
    /// per grid layer: owner of each cell for track centres
    track: Vec<Vec<i32>>,
    /// owner for via centres (through all layers)
    via: Vec<i32>,
}

impl Grid {
    fn idx(&self, x: usize, y: usize) -> usize {
        y * self.nx + x
    }
    fn center(&self, i: usize) -> P {
        self.lo + p((i % self.nx) as f64 * self.g, (i / self.nx) as f64 * self.g)
    }
    fn cell(&self, q: P) -> (i64, i64) {
        (((q.x - self.lo.x) / self.g).round() as i64, ((q.y - self.lo.y) / self.g).round() as i64)
    }
    fn mark(v: &mut [i32], i: usize, net: i32) {
        let o = v[i];
        if o == FREE {
            v[i] = net;
        } else if o != net {
            v[i] = WALL;
        }
    }
    fn cells_near(&self, h: &Hull, r: f64) -> Vec<usize> {
        let (lo, hi) = h.bbox();
        let (x0, y0) = self.cell(lo - P::splat(r));
        let (x1, y1) = self.cell(hi + P::splat(r));
        let mut out = vec![];
        for y in y0.max(0)..=y1.min(self.ny as i64 - 1) {
            for x in x0.max(0)..=x1.min(self.nx as i64 - 1) {
                let i = self.idx(x as usize, y as usize);
                if h.dist(&Hull::disc(self.center(i), 0.0)) < r - 1e-9 {
                    out.push(i);
                }
            }
        }
        out
    }
    fn layer_index(&self, cu: u8) -> Option<usize> {
        self.lays.iter().position(|l| *l == cu)
    }
}

/// Obstacles for routing `net` with track width `w` and vias of diameter `vd`.
fn build_grid(b: &Board, net: &str, w: f64, vd: f64, vdrill: f64, lays: Vec<u8>, net_ids: &HashMap<String, i32>) -> Grid {
    let (lo, hi) = b.size().unwrap_or((P::ZERO, p(10.0, 10.0)));
    let g = 0.25_f64.min(w.max(0.1));
    let nx = ((hi.x - lo.x) / g).ceil() as usize + 1;
    let ny = ((hi.y - lo.y) / g).ceil() as usize + 1;
    let n = nx * ny;
    let mut grid = Grid { lo, g, nx, ny, track: vec![vec![FREE; n]; lays.len()], via: vec![FREE; n], lays };
    let nid = *net_ids.get(net).unwrap_or(&-3);
    let id_of = |c: &Copper| if c.net.is_empty() { -10 } else { *net_ids.get(&c.net).unwrap_or(&-11) };
    for c in b.copper() {
        let id = id_of(&c);
        if id == nid {
            continue;
        }
        let clr = b.rules.clearance_between(net, &c.net);
        for (k, cu) in grid.lays.clone().iter().enumerate() {
            if c.layers & (1 << cu) != 0 {
                for i in grid.cells_near(&c.hull, clr + w / 2.0) {
                    Grid::mark(&mut grid.track[k], i, id);
                }
            }
        }
        for i in grid.cells_near(&c.hull, clr + vd / 2.0) {
            Grid::mark(&mut grid.via, i, id);
        }
    }
    for z in &b.zones {
        if z.net == net {
            continue;
        }
        let Some(k) = grid.layer_index(z.layer) else { continue };
        for i in 0..n {
            let c = grid.center(i);
            if z.fill.iter().any(|s| poly::in_shape(s, [c.x, c.y])) {
                grid.track[k][i] = WALL;
                grid.via[i] = WALL;
            }
        }
    }
    let region = poly::offset(&b.region(), -(b.rules.edge_clearance + w / 2.0));
    let via_region = poly::offset(&b.region(), -(b.rules.edge_clearance + vd / 2.0));
    for i in 0..n {
        let c = grid.center(i);
        if !poly::in_shapes(&region, [c.x, c.y]) {
            for t in &mut grid.track {
                t[i] = WALL;
            }
        }
        if !poly::in_shapes(&via_region, [c.x, c.y]) {
            grid.via[i] = WALL;
        }
    }
    // drills of other nets keep the hole clearance from new vias
    let mut holes: Vec<Hull> = vec![];
    for f in &b.footprints {
        for (k, pad) in f.fp.pads.iter().enumerate() {
            if pad.drill > 0.0 && f.nets.get(k).is_none_or(|x| x != net) {
                let (a, e) = pad.hole_ends(f.pos, f.rot, f.bottom);
                holes.push(Hull::capsule(a, e, pad.drill / 2.0));
            }
        }
    }
    for v in &b.vias {
        if v.net != net {
            holes.push(Hull::disc(v.pos, v.drill / 2.0));
        }
    }
    for h in holes {
        for i in grid.cells_near(&h, b.rules.hole_clearance + vdrill / 2.0) {
            grid.via[i] = WALL;
        }
    }
    grid
}

/// A* over (grid layer, cell, direction). Returns the path as (grid layer, cell).
fn astar(grid: &Grid, nid: i32, starts: &[(usize, usize)], is_target: &dyn Fn(usize, usize) -> bool, aim: P, vias: bool, via_cost: u64) -> Option<Vec<(usize, usize)>> {
    const DIRS: [(i64, i64); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
    let n = grid.nx * grid.ny;
    let key = |l: usize, i: usize, d: usize| (l * n + i) * 9 + d;
    let mut dist: HashMap<usize, u64> = HashMap::new();
    let mut prev: HashMap<usize, usize> = HashMap::new();
    let mut heap = BinaryHeap::new();
    let h = |i: usize| (((grid.center(i) - aim).length() / grid.g) * 10.0) as u64;
    let ok = |l: usize, i: usize| {
        let o = grid.track[l][i];
        o == FREE || o == nid
    };
    for &(l, i) in starts {
        let k = key(l, i, 8);
        dist.insert(k, 0);
        heap.push(Reverse((h(i), 0u64, k)));
    }
    let nl = grid.lays.len();
    let mut steps = 0usize;
    while let Some(Reverse((_, d0, k))) = heap.pop() {
        steps += 1;
        if steps > 4_000_000 {
            return None;
        }
        if dist.get(&k).is_some_and(|&d| d < d0) {
            continue;
        }
        let dir = k % 9;
        let li = k / 9;
        let (l, i) = (li / n, li % n);
        if is_target(l, i) {
            let mut path = vec![];
            let mut c = k;
            loop {
                let li = c / 9;
                path.push((li / n, li % n));
                match prev.get(&c) {
                    Some(&pk) => c = pk,
                    None => break,
                }
            }
            path.reverse();
            return Some(path);
        }
        let (x, y) = ((i % grid.nx) as i64, (i / grid.nx) as i64);
        for (di, (dx, dy)) in DIRS.iter().enumerate() {
            let (x2, y2) = (x + dx, y + dy);
            if x2 < 0 || y2 < 0 || x2 >= grid.nx as i64 || y2 >= grid.ny as i64 {
                continue;
            }
            let j = grid.idx(x2 as usize, y2 as usize);
            if !ok(l, j) {
                continue;
            }
            if *dx != 0 && *dy != 0 && (!ok(l, grid.idx(x2 as usize, y as usize)) || !ok(l, grid.idx(x as usize, y2 as usize))) {
                continue;
            }
            let step = if *dx != 0 && *dy != 0 { 14 } else { 10 };
            let turn = if dir == 8 || dir == di {
                0
            } else {
                let t = (dir as i64 - di as i64).rem_euclid(8);
                let t = t.min(8 - t);
                if t > 2 {
                    continue;
                }
                t as u64 * 6
            };
            // outer layers prefer one direction each, inner layers the other diagonal grid
            let bias = match (l % 2, *dx == 0, *dy == 0) {
                (0, true, false) | (1, false, true) => 3,
                _ => 0,
            };
            let nd = d0 + step + turn + bias;
            let nk = key(l, j, di);
            if dist.get(&nk).is_none_or(|&d| nd < d) {
                dist.insert(nk, nd);
                prev.insert(nk, k);
                heap.push(Reverse((nd + h(j), nd, nk)));
            }
        }
        if vias && (grid.via[i] == FREE || grid.via[i] == nid) {
            for o in 0..nl {
                if o == l || !ok(o, i) {
                    continue;
                }
                let nd = d0 + via_cost;
                let nk = key(o, i, 8);
                if dist.get(&nk).is_none_or(|&d| nd < d) {
                    dist.insert(nk, nd);
                    prev.insert(nk, k);
                    heap.push(Reverse((nd + h(i), nd, nk)));
                }
            }
        }
    }
    None
}

/// Drops points on straight runs.
pub fn simplify(pts: &[P]) -> Vec<P> {
    let mut out: Vec<P> = vec![];
    for q in pts {
        if out.last().is_some_and(|l| (*l - *q).length() < 1e-9) {
            continue;
        }
        if out.len() >= 2 {
            let (a, m) = (out[out.len() - 2], out[out.len() - 1]);
            let (d1, d2) = ((m - a).normalize_or_zero(), (*q - m).normalize_or_zero());
            if (d1 - d2).length() < 1e-6 {
                out.pop();
            }
        }
        out.push(*q);
    }
    out
}

fn net_ids(b: &Board) -> HashMap<String, i32> {
    b.nets().into_iter().enumerate().map(|(i, n)| (n, i as i32)).collect()
}

/// Walkaround for the interactive router: a clearance-correct path on one layer from
/// `from` to `to` around the copper of other nets (None when there is none).
pub fn find_path(b: &Board, net: &str, layer: u8, from: P, to: P, width: f64) -> Option<Vec<P>> {
    let ids = net_ids(b);
    let cr = b.rules.class_of(net);
    let grid = build_grid(b, net, width, cr.via_dia, cr.via_drill, vec![layer], &ids);
    let nid = *ids.get(net).unwrap_or(&-3);
    let (sx, sy) = grid.cell(from);
    let (tx, ty) = grid.cell(to);
    let inside = |x: i64, y: i64| x >= 0 && y >= 0 && (x as usize) < grid.nx && (y as usize) < grid.ny;
    if !inside(sx, sy) || !inside(tx, ty) {
        return None;
    }
    let s = grid.idx(sx as usize, sy as usize);
    let t = grid.idx(tx as usize, ty as usize);
    let mut g2 = grid;
    // the end cells themselves are passable (they sit on the net's own copper or free space)
    g2.track[0][s] = FREE;
    g2.track[0][t] = FREE;
    let path = astar(&g2, nid, &[(0, s)], &|_, i| i == t, to, false, 0)?;
    let mut pts: Vec<P> = path.iter().map(|&(_, i)| g2.center(i)).collect();
    pts[0] = from;
    let last = pts.len() - 1;
    pts[last] = to;
    Some(simplify(&pts))
}

/// Routes the open connections of all nets (or only `only`).
pub fn route(b: &mut Board, only: Option<&str>) -> Report {
    let mut rep = Report::default();
    if b.size().is_none() {
        rep.failed.push("Kein Platinenumriss".into());
        return rep;
    }
    let ids = net_ids(b);
    let lays = stack(b);
    let mut tried: HashMap<(String, i64, i64), usize> = HashMap::new();
    for _round in 0..400 {
        let rats: Vec<(P, P, String)> = b.ratsnest().into_iter().filter(|r| only.is_none_or(|o| o == r.2)).collect();
        let mut todo: Vec<&(P, P, String)> = rats.iter().filter(|r| tried.get(&(r.2.clone(), (r.0.x * 100.0) as i64, (r.0.y * 100.0) as i64)).copied().unwrap_or(0) < 3).collect();
        if todo.is_empty() {
            break;
        }
        todo.sort_by(|a, c| (a.0 - a.1).length().partial_cmp(&(c.0 - c.1).length()).unwrap());
        let (a, e, net) = todo[0].clone();
        let tkey = (net.clone(), (a.x * 100.0) as i64, (a.y * 100.0) as i64);
        *tried.entry(tkey.clone()).or_default() += 1;
        let cr = b.rules.class_of(&net);
        let w = cr.track;
        let nid = *ids.get(&net).unwrap_or(&-3);
        let cu = b.copper();
        let cl = b.clusters(&cu);
        let mut grid = build_grid(b, &net, w, cr.via_dia, cr.via_drill, lays.clone(), &ids);
        let which = |q: P| cu.iter().position(|c| c.net == net && (c.hull.contains(q) || c.hull.pts.iter().any(|k| (*k - q).length() < 1e-6)));
        let (Some(sa), Some(sb)) = (which(a), which(e)) else {
            rep.failed.push(net.clone());
            tried.insert(tkey, 99);
            continue;
        };
        let (ca, cb) = (cl[sa], cl[sb]);
        let n = grid.nx * grid.ny;
        let mut is_target = vec![vec![false; n]; lays.len()];
        let mut starts: Vec<(usize, usize)> = vec![];
        let mut anchor: HashMap<(usize, usize), P> = HashMap::new();
        for (ci, c) in cu.iter().enumerate() {
            if cl[ci] != ca && cl[ci] != cb {
                continue;
            }
            let cells = grid.cells_near(&c.hull, 1e-6 + 0.0);
            let cells: Vec<usize> = if cells.is_empty() { grid.cells_near(&c.hull, grid.g * 0.75) } else { cells };
            for i in cells {
                for (k, layer) in lays.iter().enumerate() {
                    if c.layers & (1 << layer) == 0 {
                        continue;
                    }
                    if cl[ci] == cb {
                        is_target[k][i] = true;
                    } else {
                        starts.push((k, i));
                        if let Item::Pad(..) | Item::Via(_) = c.what {
                            anchor.insert((k, i), super::board::center(&c.hull));
                        }
                    }
                    grid.track[k][i] = FREE;
                }
            }
        }
        let via_cost = (cr.via_dia * 6.0 / grid.g) as u64 * 10;
        let Some(path) = astar(&grid, nid, &starts, &|l, i| is_target[l][i], e, true, via_cost) else {
            rep.failed.push(net.clone());
            tried.insert(tkey, 99);
            continue;
        };
        // ---- back to tracks and vias
        let mut pts: Vec<(usize, P)> = path.iter().map(|&(l, i)| (l, grid.center(i))).collect();
        if let Some(c) = anchor.get(&path[0]) {
            pts.insert(0, (path[0].0, *c));
        }
        let mut run: Vec<P> = vec![];
        let mut layer = pts[0].0;
        let flush = |run: &mut Vec<P>, layer: usize, b: &mut Board| {
            for s in simplify(run).windows(2) {
                b.tracks.push(Track { a: s[0], b: s[1], width: w, layer: lays[layer], net: net.clone() });
            }
            run.clear();
        };
        for (l, q) in pts {
            if l != layer {
                flush(&mut run, layer, b);
                if !b.vias.iter().any(|v| (v.pos - q).length() < 1e-6) {
                    b.vias.push(Via { pos: q, dia: cr.via_dia, drill: cr.via_drill, net: net.clone() });
                    rep.vias += 1;
                }
                layer = l;
            }
            run.push(q);
        }
        flush(&mut run, layer, b);
        rep.routed += 1;
    }
    b.tidy();
    rep.failed.sort();
    rep.failed.dedup();
    rep
}
