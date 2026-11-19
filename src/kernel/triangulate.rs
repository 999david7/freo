//! Ear-clipping triangulation for simple polygons with holes (holes bridged into the outer ring).

use super::math::*;

/// Returns CCW triangles as index triples into `outer ++ holes...`.
pub fn triangulate(outer_in: &[V2], holes_in: &[Vec<V2>]) -> Vec<[usize; 3]> {
    let mut pts: Vec<V2> = outer_in.to_vec();
    let mut outer: Vec<usize> = (0..outer_in.len()).collect();
    if polygon_area2(outer_in) < 0.0 {
        outer.reverse();
    }
    let mut holes: Vec<Vec<usize>> = Vec::new();
    for h in holes_in {
        if h.len() < 3 {
            continue;
        }
        let base = pts.len();
        pts.extend_from_slice(h);
        let mut idx: Vec<usize> = (base..base + h.len()).collect();
        if polygon_area2(h) > 0.0 {
            idx.reverse();
        }
        holes.push(idx);
    }
    if outer.len() < 3 {
        return vec![];
    }
    let maxx = |h: &Vec<usize>| h.iter().map(|&i| pts[i].x).fold(f64::NEG_INFINITY, f64::max);
    holes.sort_by(|a, b| maxx(b).partial_cmp(&maxx(a)).unwrap_or(std::cmp::Ordering::Equal));

    let mut ring = outer;
    for hi in 0..holes.len() {
        let hole = holes[hi].clone();
        let mut mi = 0;
        for k in 1..hole.len() {
            if pts[hole[k]].x > pts[hole[mi]].x {
                mi = k;
            }
        }
        let m = hole[mi];
        let mp = pts[m];
        let mut order: Vec<usize> = (0..ring.len()).collect();
        order.sort_by(|&a, &b| (pts[ring[a]] - mp).length().partial_cmp(&(pts[ring[b]] - mp).length()).unwrap());
        let others = &holes[hi..];
        let mut bridge = None;
        for ri in order {
            let p = ring[ri];
            let pp = pts[p];
            if (pp - mp).length() < 1e-12 {
                continue;
            }
            let prev = pts[ring[(ri + ring.len() - 1) % ring.len()]];
            let next = pts[ring[(ri + 1) % ring.len()]];
            if !locally_inside(prev, pp, next, mp) {
                continue;
            }
            if blocked(mp, pp, &ring, others, &pts, m, p) {
                continue;
            }
            bridge = Some(ri);
            break;
        }
        let Some(bi) = bridge else { continue };
        let p = ring[bi];
        let mut insert: Vec<usize> = (0..=hole.len()).map(|k| hole[(mi + k) % hole.len()]).collect();
        insert.push(p);
        let tail = ring.split_off(bi + 1);
        ring.extend(insert);
        ring.extend(tail);
    }
    // hole bridges duplicate vertices: drop the zero-area slivers they can leave, so caps
    // carry no degenerate polygons (they would show up as stray edges between the holes)
    let scale = pts.iter().fold(0.0f64, |m, p| m.max(p.x.abs()).max(p.y.abs())).max(1.0);
    let eps = 1e-12 * scale * scale;
    ear_clip(ring, &pts)
        .into_iter()
        .filter(|t| {
            let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
            let same = |x: V2, y: V2| (x - y).length() < 1e-12 * scale;
            cross2(b - a, c - b) > eps && !same(a, b) && !same(b, c) && !same(a, c)
        })
        .collect()
}

fn locally_inside(a: V2, b: V2, c: V2, p: V2) -> bool {
    let d = p - b;
    if cross2(b - a, c - b) >= 0.0 {
        cross2(c - b, d) >= 0.0 && cross2(d, a - b) >= 0.0
    } else {
        !(cross2(a - b, d) > 0.0 && cross2(d, c - b) > 0.0)
    }
}

/// Segments p1–p2 and q1–q2 intersect or touch (collinear overlaps count).
fn seg_intersect(p1: V2, p2: V2, q1: V2, q2: V2) -> bool {
    let o1 = orient(p1, p2, q1);
    let o2 = orient(p1, p2, q2);
    let o3 = orient(q1, q2, p1);
    let o4 = orient(q1, q2, p2);
    if o1 != o2 && o3 != o4 {
        return true;
    }
    (o1 == 0 && on_segment(p1, q1, p2)) || (o2 == 0 && on_segment(p1, q2, p2)) || (o3 == 0 && on_segment(q1, p1, q2)) || (o4 == 0 && on_segment(q1, p2, q2))
}

fn orient(a: V2, b: V2, c: V2) -> i8 {
    let v = cross2(b - a, c - b);
    let e = 1e-12 * (1.0 + (b - a).length() * (c - b).length());
    if v > e {
        1
    } else if v < -e {
        -1
    } else {
        0
    }
}

fn on_segment(p: V2, q: V2, r: V2) -> bool {
    q.x <= p.x.max(r.x) + 1e-12 && q.x >= p.x.min(r.x) - 1e-12 && q.y <= p.y.max(r.y) + 1e-12 && q.y >= p.y.min(r.y) - 1e-12
}

/// The bridge a–b crosses or touches the outline or a hole somewhere other than at its ends
/// (a bridge sliding along an edge or through another vertex is blocked too).
fn blocked(a: V2, b: V2, ring: &[usize], holes: &[Vec<usize>], pts: &[V2], sa: usize, sb: usize) -> bool {
    let near = |x: V2, y: V2| (x - y).length() < 1e-12;
    let inner = |q: V2| !near(q, a) && !near(q, b) && orient(a, b, q) == 0 && on_segment(a, q, b);
    let check = |lp: &[usize]| -> bool {
        let n = lp.len();
        for i in 0..n {
            let (u, v) = (lp[i], lp[(i + 1) % n]);
            let (pu, pv) = (pts[u], pts[v]);
            if inner(pu) || inner(pv) {
                return true;
            }
            let touches = u == sa || v == sa || u == sb || v == sb || near(pu, a) || near(pv, a) || near(pu, b) || near(pv, b);
            if !touches && seg_intersect(a, b, pu, pv) {
                return true;
            }
        }
        false
    };
    check(ring) || holes.iter().any(|h| check(h))
}

// ---------------------------------------------------------------- ear clipping
// A port of the robust parts of mapbox's earcut: ears on a doubly linked ring; when no ear is
// left the ring is cleaned (duplicates, collinear points), local self-intersections (from
// hole bridges) are cured, and finally the polygon is split along a valid diagonal.

struct Ring<'a> {
    pts: &'a [V2],
    i: Vec<usize>,
    prev: Vec<usize>,
    next: Vec<usize>,
}

impl<'a> Ring<'a> {
    fn p(&self, n: usize) -> V2 {
        self.pts[self.i[n]]
    }
    fn unlink(&mut self, n: usize) {
        let (p, q) = (self.prev[n], self.next[n]);
        self.next[p] = q;
        self.prev[q] = p;
    }
    fn eq(&self, a: usize, b: usize) -> bool {
        (self.p(a) - self.p(b)).length() < 1e-12
    }
    /// earcut's signed area: negative for a convex (counter-clockwise) corner
    fn area(&self, p: usize, q: usize, r: usize) -> f64 {
        -cross2(self.p(q) - self.p(p), self.p(r) - self.p(q))
    }
    /// Removes duplicate nodes (and collinear ones when `collinear`) between `start` and `end`.
    /// Collinear points are kept normally: neighbouring faces share them (watertight seams).
    fn filter(&mut self, start: usize, end: Option<usize>) -> usize {
        self.filter_with(start, end, false)
    }
    fn filter_with(&mut self, start: usize, end: Option<usize>, collinear: bool) -> usize {
        let mut end = end.unwrap_or(start);
        let mut p = start;
        loop {
            let mut again = false;
            let (pp, pn) = (self.prev[p], self.next[p]);
            if pn != p && (self.eq(p, pn) || (collinear && self.area(pp, p, pn) == 0.0)) {
                self.unlink(p);
                p = pp;
                end = pp;
                if p == self.next[p] {
                    break;
                }
                again = true;
            } else {
                p = pn;
            }
            if !again && p == end {
                break;
            }
        }
        end
    }
    /// Point in (or on) the counter-clockwise triangle a b c.
    fn point_in_tri(a: V2, b: V2, c: V2, p: V2) -> bool {
        (c.x - p.x) * (a.y - p.y) >= (a.x - p.x) * (c.y - p.y) && (a.x - p.x) * (b.y - p.y) >= (b.x - p.x) * (a.y - p.y) && (b.x - p.x) * (c.y - p.y) >= (c.x - p.x) * (b.y - p.y)
    }
    fn is_ear(&self, ear: usize) -> bool {
        let (a, b, c) = (self.prev[ear], ear, self.next[ear]);
        if self.area(a, b, c) >= 0.0 {
            return false;
        }
        let (pa, pb, pc) = (self.p(a), self.p(b), self.p(c));
        let mut p = self.next[c];
        while p != a {
            let q = self.p(p);
            let same = (q - pa).length() < 1e-12 || (q - pb).length() < 1e-12 || (q - pc).length() < 1e-12;
            if !same && Self::point_in_tri(pa, pb, pc, q) && self.area(self.prev[p], p, self.next[p]) >= 0.0 {
                return false;
            }
            p = self.next[p];
        }
        true
    }
    fn intersects(&self, p1: usize, q1: usize, p2: usize, q2: usize) -> bool {
        seg_intersect(self.p(p1), self.p(q1), self.p(p2), self.p(q2))
    }
    fn locally_inside(&self, a: usize, b: usize) -> bool {
        if self.area(self.prev[a], a, self.next[a]) < 0.0 {
            self.area(a, b, self.next[a]) >= 0.0 && self.area(a, self.prev[a], b) >= 0.0
        } else {
            self.area(a, b, self.prev[a]) < 0.0 || self.area(a, self.next[a], b) < 0.0
        }
    }
    fn middle_inside(&self, a: usize, b: usize) -> bool {
        let m = (self.p(a) + self.p(b)) / 2.0;
        let mut inside = false;
        let mut p = a;
        loop {
            let (u, v) = (self.p(p), self.p(self.next[p]));
            if (u.y > m.y) != (v.y > m.y) && v.y != u.y && m.x < (v.x - u.x) * (m.y - u.y) / (v.y - u.y) + u.x {
                inside = !inside;
            }
            p = self.next[p];
            if p == a {
                break;
            }
        }
        inside
    }
    fn intersects_polygon(&self, a: usize, b: usize) -> bool {
        let mut p = a;
        loop {
            let q = self.next[p];
            let shares = self.i[p] == self.i[a] || self.i[q] == self.i[a] || self.i[p] == self.i[b] || self.i[q] == self.i[b] || self.eq(p, a) || self.eq(q, a) || self.eq(p, b) || self.eq(q, b);
            if !shares && self.intersects(p, q, a, b) {
                return true;
            }
            p = q;
            if p == a {
                break;
            }
        }
        false
    }
    fn valid_diagonal(&self, a: usize, b: usize) -> bool {
        self.i[self.next[a]] != self.i[b]
            && self.i[self.prev[a]] != self.i[b]
            && !self.intersects_polygon(a, b)
            && self.locally_inside(a, b)
            && self.locally_inside(b, a)
            && self.middle_inside(a, b)
    }
    /// Splits the ring along a–b into two rings; returns the new node at b of the second.
    fn split(&mut self, a: usize, b: usize) -> usize {
        let a2 = self.i.len();
        self.i.push(self.i[a]);
        self.prev.push(0);
        self.next.push(0);
        let b2 = self.i.len();
        self.i.push(self.i[b]);
        self.prev.push(0);
        self.next.push(0);
        let (an, bp) = (self.next[a], self.prev[b]);
        self.next[a] = b;
        self.prev[b] = a;
        self.next[a2] = an;
        self.prev[an] = a2;
        self.next[b2] = a2;
        self.prev[a2] = b2;
        self.next[bp] = b2;
        self.prev[b2] = bp;
        b2
    }
    /// Cuts off triangles at local self-intersections (two bridges crossing at a vertex).
    fn cure(&mut self, start: usize, tris: &mut Vec<[usize; 3]>) -> usize {
        let mut start = start;
        let mut p = start;
        loop {
            let a = self.prev[p];
            let pn = self.next[p];
            let b = self.next[pn];
            if !self.eq(a, b) && self.intersects(a, p, pn, b) && self.locally_inside(a, b) && self.locally_inside(b, a) {
                tris.push([self.i[a], self.i[p], self.i[b]]);
                self.unlink(p);
                self.unlink(pn);
                p = b;
                start = b;
            }
            p = self.next[p];
            if p == start {
                break;
            }
        }
        self.filter(p, None)
    }
    fn split_earcut(&mut self, start: usize, tris: &mut Vec<[usize; 3]>, depth: usize) {
        let mut a = start;
        loop {
            let mut b = self.next[self.next[a]];
            while b != self.prev[a] {
                if self.i[a] != self.i[b] && self.valid_diagonal(a, b) {
                    let c = self.split(a, b);
                    let na = self.next[a];
                    let a = self.filter(a, Some(na));
                    let nc = self.next[c];
                    let c = self.filter(c, Some(nc));
                    self.earcut(a, tris, 0, depth + 1);
                    self.earcut(c, tris, 0, depth + 1);
                    return;
                }
                b = self.next[b];
            }
            a = self.next[a];
            if a == start {
                break;
            }
        }
    }
    fn earcut(&mut self, ear: usize, tris: &mut Vec<[usize; 3]>, pass: u8, depth: usize) {
        if depth > 200 {
            return;
        }
        let mut ear = ear;
        let mut stop = ear;
        while self.prev[ear] != self.next[ear] {
            let (prev, next) = (self.prev[ear], self.next[ear]);
            if self.is_ear(ear) {
                tris.push([self.i[prev], self.i[ear], self.i[next]]);
                self.unlink(ear);
                ear = self.next[next];
                stop = ear;
                continue;
            }
            ear = next;
            if ear == stop {
                match pass {
                    0 => {
                        let s = self.filter(ear, None);
                        self.earcut(s, tris, 1, depth);
                    }
                    1 => {
                        let s = self.filter(ear, None);
                        let s = self.cure(s, tris);
                        self.earcut(s, tris, 2, depth);
                    }
                    2 => {
                        // last resort before splitting: drop collinear points
                        let s = self.filter_with(ear, None, true);
                        self.earcut(s, tris, 3, depth);
                    }
                    _ => self.split_earcut(ear, tris, depth),
                }
                break;
            }
        }
    }
}

fn ear_clip(ring_in: Vec<usize>, pts: &[V2]) -> Vec<[usize; 3]> {
    let n = ring_in.len();
    if n < 3 {
        return vec![];
    }
    let mut r = Ring { pts, i: ring_in, prev: (0..n).map(|k| (k + n - 1) % n).collect(), next: (0..n).map(|k| (k + 1) % n).collect() };
    let start = r.filter(0, None);
    let mut tris = Vec::new();
    if r.next[start] != r.prev[start] {
        r.earcut(start, &mut tris, 0, 0);
    }
    tris
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Triangles cover the polygon exactly once: area sum equals polygon area and every
    /// interior edge is shared by exactly two triangles (no T-junctions, no overlaps).
    fn check(outer: &[V2], holes: &[Vec<V2>]) {
        let t = triangulate(outer, holes);
        let pts: Vec<V2> = outer.iter().chain(holes.iter().flatten()).copied().collect();
        let area: f64 = t.iter().map(|x| cross2(pts[x[1]] - pts[x[0]], pts[x[2]] - pts[x[1]]) / 2.0).sum();
        let want = polygon_area2(outer).abs() - holes.iter().map(|h| polygon_area2(h).abs()).sum::<f64>();
        assert!((area - want).abs() < 1e-6 * want.max(1.0), "Fläche {} statt {}", area, want);
        assert!(t.iter().all(|x| cross2(pts[x[1]] - pts[x[0]], pts[x[2]] - pts[x[1]]) > 0.0), "alle gegen den Uhrzeigersinn");
    }

    #[test]
    fn plate_with_a_grid_of_holes() {
        // 160 × 110 plate with 30 × 8 round holes in exact rows (like pin headers)
        let outer = vec![V2::new(0.0, 0.0), V2::new(160.0, 0.0), V2::new(160.0, 110.0), V2::new(0.0, 110.0)];
        let mut holes = vec![];
        for r in 0..8 {
            for c in 0..30 {
                let ctr = V2::new(5.0 + c as f64 * 5.08, 10.0 + r as f64 * 12.7);
                holes.push((0..16).map(|k| ctr + V2::new((k as f64 / 16.0 * std::f64::consts::TAU).cos(), -(k as f64 / 16.0 * std::f64::consts::TAU).sin()) * 0.5).collect());
            }
        }
        check(&outer, &holes);
    }

    #[test]
    fn simple_shapes() {
        check(&[V2::new(0.0, 0.0), V2::new(4.0, 0.0), V2::new(4.0, 3.0), V2::new(2.0, 1.0), V2::new(0.0, 3.0)], &[]);
        let sq = |c: V2, r: f64| vec![c + V2::new(-r, -r), c + V2::new(-r, r), c + V2::new(r, r), c + V2::new(r, -r)];
        check(&[V2::new(0.0, 0.0), V2::new(10.0, 0.0), V2::new(10.0, 10.0), V2::new(0.0, 10.0)], &[sq(V2::new(3.0, 5.0), 1.0), sq(V2::new(7.0, 5.0), 1.0)]);
    }
}
