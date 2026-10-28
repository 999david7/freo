//! Welded mesh topology over a CSG polygon soup: T-junction repair, faces (smooth patches),
//! and edge chains (selectable edges for rounds/chamfers).

use super::csg::*;
use super::math::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Polygon tags above this bit identify separate bodies (assembly components).
pub const BODY_SHIFT: u32 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceRef {
    pub point: V3,
    pub normal: V3,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeRef {
    pub a: V3,
    pub m: V3,
    pub b: V3,
    pub n1: V3,
    pub n2: V3,
    pub closed: bool,
}

#[derive(Clone, Debug)]
pub struct EdgeChain {
    pub points: Vec<V3>,
    pub closed: bool,
    pub face_a: usize,
    pub face_b: usize,
    pub n_a: Vec<V3>,
    pub n_b: Vec<V3>,
    pub f_a: Vec<V3>,
    pub f_b: Vec<V3>,
    pub convex: bool,
}

impl EdgeChain {
    fn seg_count(&self) -> usize { if self.closed { self.points.len() } else { self.points.len() - 1 } }
    pub fn length(&self) -> f64 {
        let n = self.points.len();
        (0..self.seg_count()).map(|i| (self.points[(i + 1) % n] - self.points[i]).length()).sum()
    }
    pub fn mid(&self) -> V3 {
        let total = self.length();
        let n = self.points.len();
        let mut acc = 0.0;
        for i in 0..self.seg_count() {
            let (a, b) = (self.points[i], self.points[(i + 1) % n]);
            let l = (b - a).length();
            if acc + l >= total / 2.0 && l > 0.0 { return a + (b - a) * ((total / 2.0 - acc) / l); }
            acc += l;
        }
        self.points[n / 2]
    }
    pub fn is_straight(&self) -> bool {
        if self.points.len() < 2 || self.closed { return false; }
        let a = self.points[0];
        let d = safe_norm(*self.points.last().unwrap() - a, V3::X);
        self.points.iter().all(|p| (*p - a).cross(d).length() < 1e-4)
    }
    pub fn distance_to(&self, q: V3) -> f64 {
        let n = self.points.len();
        if self.seg_count() == 0 { return (q - self.points[0]).length(); }
        (0..self.seg_count())
            .map(|i| (q - closest_on_segment(q, self.points[i], self.points[(i + 1) % n]).0).length())
            .fold(f64::INFINITY, f64::min)
    }
}

#[derive(Clone, Debug)]
pub struct TopoFace {
    pub polys: Vec<usize>,
    pub planar: bool,
    pub normal: V3,
    pub offset: f64,
    pub area: f64,
    pub center: V3,
}

#[derive(Default)]
pub struct Topology {
    pub positions: Vec<V3>,
    pub polys: Vec<Vec<usize>>,
    pub vnormals: Vec<Vec<V3>>,
    pub planes: Vec<CPlane>,
    pub tags: Vec<u32>,
    pub poly_face: Vec<usize>,
    pub faces: Vec<TopoFace>,
    pub chains: Vec<EdgeChain>,
    pub draw_edges: Vec<(usize, usize)>,
    pub bbox: BBox,
}

type Key = (i64, i64, i64);

impl Topology {
    pub fn new(input: &[CPolygon]) -> Topology {
        let mut t = Topology::default();
        for p in input { for q in &p.v { t.bbox.add(q.p); } }
        if input.is_empty() { return t; }
        let tol = (t.bbox.diagonal() * 2e-8).max(1e-6);
        let cell = tol * 10.0;
        let mut weld: HashMap<Key, Vec<usize>> = HashMap::new();
        let key = |p: V3, c: f64| -> Key { ((p.x / c).floor() as i64, (p.y / c).floor() as i64, (p.z / c).floor() as i64) };
        // separate bodies (assembly components, tag >> BODY_SHIFT) never share vertices
        let mut pos_body: Vec<u32> = Vec::new();
        for p in input {
            let body = p.tag >> BODY_SHIFT;
            let mut idx: Vec<usize> = Vec::new();
            let mut nrm: Vec<V3> = Vec::new();
            for q in &p.v {
                let k = key(q.p, cell);
                let mut found = None;
                'outer: for dx in -1..=1 { for dy in -1..=1 { for dz in -1..=1 {
                    if let Some(list) = weld.get(&(k.0 + dx, k.1 + dy, k.2 + dz)) {
                        for &i in list { if pos_body[i] == body && (t.positions[i] - q.p).length() <= tol { found = Some(i); break 'outer; } }
                    }
                } } }
                let i = found.unwrap_or_else(|| {
                    t.positions.push(q.p);
                    pos_body.push(body);
                    weld.entry(k).or_default().push(t.positions.len() - 1);
                    t.positions.len() - 1
                });
                if idx.last() == Some(&i) { continue; }
                idx.push(i);
                nrm.push(q.n);
            }
            if idx.len() > 1 && idx.first() == idx.last() { idx.pop(); nrm.pop(); }
            if idx.len() < 3 { continue; }
            t.polys.push(idx);
            t.vnormals.push(nrm);
            t.planes.push(p.plane);
            t.tags.push(p.tag);
        }
        t.repair_t_junctions(tol, &pos_body);
        t.build_edges();
        t
    }

    fn repair_t_junctions(&mut self, tol: f64, pos_body: &[u32]) {
        let diag = self.bbox.diagonal().max(1e-6);
        let cell = diag / 64.0;
        let bmin = self.bbox.min;
        let key = |p: V3| -> Key {
            (((p.x - bmin.x) / cell).floor() as i64, ((p.y - bmin.y) / cell).floor() as i64, ((p.z - bmin.z) / cell).floor() as i64)
        };
        let cells_along = |a: V3, b: V3| -> Vec<Key> {
            let mut cur = key(a);
            let end = key(b);
            let mut out = vec![cur];
            let d = b - a;
            let step = |v: f64| -> i64 { if v > 0.0 { 1 } else if v < 0.0 { -1 } else { 0 } };
            let (sx, sy, sz) = (step(d.x), step(d.y), step(d.z));
            let tfor = |c: i64, s: i64, o: f64, a0: f64, dd: f64| -> f64 {
                if s == 0 { return f64::INFINITY; }
                let boundary = o + (c + if s > 0 { 1 } else { 0 }) as f64 * cell;
                (boundary - a0) / dd
            };
            let mut tx = tfor(cur.0, sx, bmin.x, a.x, d.x);
            let mut ty = tfor(cur.1, sy, bmin.y, a.y, d.y);
            let mut tz = tfor(cur.2, sz, bmin.z, a.z, d.z);
            let tdx = if sx == 0 { f64::INFINITY } else { cell / d.x.abs() };
            let tdy = if sy == 0 { f64::INFINITY } else { cell / d.y.abs() };
            let tdz = if sz == 0 { f64::INFINITY } else { cell / d.z.abs() };
            let mut g = 0;
            while cur != end && g < 4096 {
                g += 1;
                if tx <= ty && tx <= tz {
                    if tx > 1.0 { break; }
                    cur.0 += sx; tx += tdx;
                } else if ty <= tz {
                    if ty > 1.0 { break; }
                    cur.1 += sy; ty += tdy;
                } else {
                    if tz > 1.0 { break; }
                    cur.2 += sz; tz += tdz;
                }
                out.push(cur);
            }
            if out.last() != Some(&end) { out.push(end); }
            out
        };
        let mut grid: HashMap<Key, Vec<(usize, usize)>> = HashMap::new();
        for (pi, poly) in self.polys.iter().enumerate() {
            let n = poly.len();
            for e in 0..n {
                for c in cells_along(self.positions[poly[e]], self.positions[poly[(e + 1) % n]]) {
                    grid.entry(c).or_default().push((pi, e));
                }
            }
        }
        let mut inserts: HashMap<usize, HashMap<usize, Vec<(f64, usize)>>> = HashMap::new();
        for (vi, &p) in self.positions.iter().enumerate() {
            let mut keys = HashSet::new();
            for sx in [-tol, tol] { for sy in [-tol, tol] { for sz in [-tol, tol] { keys.insert(key(p + V3::new(sx, sy, sz))); } } }
            let mut seen = HashSet::new();
            for k in keys {
                let Some(list) = grid.get(&k) else { continue };
                for &(pi, e) in list {
                    if !seen.insert((pi, e)) { continue; }
                    if self.tags[pi] >> BODY_SHIFT != pos_body[vi] { continue; }
                    let poly = &self.polys[pi];
                    let (ia, ib) = (poly[e], poly[(e + 1) % poly.len()]);
                    if ia == vi || ib == vi { continue; }
                    let (a, b) = (self.positions[ia], self.positions[ib]);
                    let ab = b - a;
                    let l2 = ab.dot(ab);
                    if l2 < 1e-24 { continue; }
                    let t = (p - a).dot(ab) / l2;
                    if t <= 1e-9 || t >= 1.0 - 1e-9 { continue; }
                    if (a + ab * t - p).length() <= tol * 4.0 {
                        inserts.entry(pi).or_default().entry(e).or_default().push((t, vi));
                    }
                }
            }
        }
        for (pi, edges) in inserts {
            let poly = self.polys[pi].clone();
            let nrm = self.vnormals[pi].clone();
            let mut np = Vec::new();
            let mut nn = Vec::new();
            for e in 0..poly.len() {
                np.push(poly[e]);
                nn.push(nrm[e]);
                if let Some(list) = edges.get(&e) {
                    let mut l = list.clone();
                    l.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                    let (n0, n1) = (nrm[e], nrm[(e + 1) % poly.len()]);
                    for (t, v) in l {
                        if np.last() == Some(&v) { continue; }
                        np.push(v);
                        nn.push(safe_norm(n0 + (n1 - n0) * t, n0));
                    }
                }
            }
            self.polys[pi] = np;
            self.vnormals[pi] = nn;
        }
    }

    fn build_edges(&mut self) {
        let mut edge_map: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (pi, poly) in self.polys.iter().enumerate() {
            let n = poly.len();
            for e in 0..n {
                let (u, v) = (poly[e], poly[(e + 1) % n]);
                if u != v { edge_map.entry((u.min(v), u.max(v))).or_default().push(pi); }
            }
        }
        let cos_sharp = 25f64.to_radians().cos();
        let cos_tagged = 4f64.to_radians().cos();
        let mut parent: Vec<usize> = (0..self.polys.len()).collect();
        fn find(p: &mut Vec<usize>, mut x: usize) -> usize {
            while p[x] != x { p[x] = p[p[x]]; x = p[x]; }
            x
        }
        let mut sharp: Vec<((usize, usize), usize, usize)> = Vec::new();
        let mut keys: Vec<_> = edge_map.keys().copied().collect();
        keys.sort();
        for k in keys {
            let ps = &edge_map[&k];
            if ps.len() == 2 {
                let (p, q) = (ps[0], ps[1]);
                let c = self.planes[p].n.dot(self.planes[q].n);
                if c < cos_sharp || (self.tags[p] != self.tags[q] && c < cos_tagged) {
                    sharp.push((k, p, q));
                } else {
                    let (rp, rq) = (find(&mut parent, p), find(&mut parent, q));
                    if rp != rq { parent[rp] = rq; }
                }
            } else {
                self.draw_edges.push(k);
            }
        }
        let mut face_index: HashMap<usize, usize> = HashMap::new();
        self.poly_face = vec![0; self.polys.len()];
        for pi in 0..self.polys.len() {
            let r = find(&mut parent, pi);
            let f = *face_index.entry(r).or_insert_with(|| {
                self.faces.push(TopoFace { polys: vec![], planar: true, normal: self.planes[pi].n, offset: self.planes[pi].w, area: 0.0, center: V3::ZERO });
                self.faces.len() - 1
            });
            self.poly_face[pi] = f;
            let a = self.poly_area(pi);
            let c = self.poly_centroid(pi);
            let face = &mut self.faces[f];
            face.polys.push(pi);
            face.area += a;
            face.center += c * a;
            if self.planes[pi].n.dot(face.normal) < 1.0 - 1e-7 || (self.planes[pi].w - face.offset).abs() > 1e-4 { face.planar = false; }
        }
        for f in &mut self.faces { if f.area > 0.0 { f.center /= f.area; } }

        struct Seg { a: usize, b: usize, pa: usize, pb: usize, fa: usize, fb: usize }
        let mut segs: Vec<Seg> = Vec::new();
        for &(k, p, q) in &sharp {
            self.draw_edges.push(k);
            let (mut fp, mut fq, mut pp, mut qq) = (self.poly_face[p], self.poly_face[q], p, q);
            if fp > fq { std::mem::swap(&mut fp, &mut fq); std::mem::swap(&mut pp, &mut qq); }
            segs.push(Seg { a: k.0, b: k.1, pa: pp, pb: qq, fa: fp, fb: fq });
        }
        let mut vert_segs: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, s) in segs.iter().enumerate() {
            vert_segs.entry(s.a).or_default().push(i);
            vert_segs.entry(s.b).or_default().push(i);
        }
        let mut used = vec![false; segs.len()];
        let pos = &self.positions;
        let dir = |s: &Seg, v: usize| -> V3 { let o = if s.a == v { s.b } else { s.a }; safe_norm(pos[o] - pos[v], V3::X) };
        let cos50 = 50f64.to_radians().cos();
        let next_seg = |v: usize, si: usize, used: &Vec<bool>| -> Option<usize> {
            let list = vert_segs.get(&v)?;
            if list.len() != 2 { return None; }
            let other = if list[0] == si { list[1] } else { list[0] };
            if used[other] { return None; }
            let (s0, s1) = (&segs[si], &segs[other]);
            if s0.fa != s1.fa || s0.fb != s1.fb { return None; }
            if (-dir(s0, v)).dot(dir(s1, v)) < cos50 { return None; }
            Some(other)
        };
        let mut chains_raw: Vec<(Vec<usize>, Vec<usize>, bool, usize, usize)> = Vec::new();
        for start in 0..segs.len() {
            if used[start] { continue; }
            used[start] = true;
            let mut verts = vec![segs[start].a, segs[start].b];
            let mut order = vec![start];
            let mut cur = start;
            let mut v = segs[start].b;
            let mut closed = false;
            while let Some(nx) = next_seg(v, cur, &used) {
                used[nx] = true;
                let s = &segs[nx];
                let o = if s.a == v { s.b } else { s.a };
                order.push(nx);
                cur = nx;
                if o == verts[0] { closed = true; break; }
                verts.push(o);
                v = o;
            }
            if !closed {
                cur = start;
                v = segs[start].a;
                while let Some(nx) = next_seg(v, cur, &used) {
                    used[nx] = true;
                    let s = &segs[nx];
                    let o = if s.a == v { s.b } else { s.a };
                    order.insert(0, nx);
                    cur = nx;
                    verts.insert(0, o);
                    v = o;
                }
            }
            chains_raw.push((verts, order, closed, segs[start].fa, segs[start].fb));
        }
        for (verts, order, closed, fa, fb) in chains_raw {
            let seg_info: Vec<(usize, usize, usize, usize)> = order.iter().map(|&i| (segs[i].a, segs[i].b, segs[i].pa, segs[i].pb)).collect();
            let c = self.make_chain(&verts, &seg_info, closed, fa, fb);
            self.chains.push(c);
        }
    }

    fn make_chain(&self, verts: &[usize], segs: &[(usize, usize, usize, usize)], closed: bool, fa: usize, fb: usize) -> EdgeChain {
        let pts: Vec<V3> = verts.iter().map(|&v| self.positions[v]).collect();
        let n = pts.len();
        let seg_count = if closed { n } else { n - 1 };
        let mut lookup: HashMap<(usize, usize), (usize, usize)> = HashMap::new();
        for s in segs { lookup.insert((s.0.min(s.1), s.0.max(s.1)), (s.2, s.3)); }
        let (mut sna, mut snb, mut sfa, mut sfb) = (vec![], vec![], vec![], vec![]);
        for i in 0..seg_count {
            let (u, w) = (verts[i], verts[(i + 1) % n]);
            let (pa, pb) = *lookup.get(&(u.min(w), u.max(w))).unwrap_or(&(0, 0));
            let (a, b) = (self.positions[u], self.positions[w]);
            let e = safe_norm(b - a, V3::X);
            let mid = (a + b) * 0.5;
            let (na, nb) = (self.planes[pa].n, self.planes[pb].n);
            let mut fa_ = safe_norm(na.cross(e), V3::X);
            if fa_.dot(self.poly_centroid(pa) - mid) < 0.0 { fa_ = -fa_; }
            let mut fb_ = safe_norm(nb.cross(e), V3::X);
            if fb_.dot(self.poly_centroid(pb) - mid) < 0.0 { fb_ = -fb_; }
            sna.push(na); snb.push(nb); sfa.push(fa_); sfb.push(fb_);
        }
        let (mut n_a, mut n_b, mut f_a, mut f_b) = (vec![], vec![], vec![], vec![]);
        for i in 0..n {
            let idx: Vec<usize> = if closed {
                vec![(i + seg_count - 1) % seg_count, i % seg_count]
            } else {
                let mut v = vec![];
                if i > 0 { v.push(i - 1); }
                if i < seg_count { v.push(i); }
                v
            };
            let avg = |arr: &Vec<V3>| safe_norm(idx.iter().fold(V3::ZERO, |s, &k| s + arr[k]), arr[idx[0]]);
            n_a.push(avg(&sna)); n_b.push(avg(&snb)); f_a.push(avg(&sfa)); f_b.push(avg(&sfb));
        }
        let mut votes = 0i32;
        for i in 0..seg_count { votes += if sfa[i].dot(snb[i]) < 0.0 { 1 } else { -1 }; }
        EdgeChain { points: pts, closed, face_a: fa, face_b: fb, n_a, n_b, f_a, f_b, convex: votes >= 0 }
    }

    pub fn poly_area(&self, pi: usize) -> f64 {
        let p = &self.polys[pi];
        let mut s = V3::ZERO;
        for i in 1..p.len() - 1 {
            s += (self.positions[p[i]] - self.positions[p[0]]).cross(self.positions[p[i + 1]] - self.positions[p[0]]);
        }
        s.length() * 0.5
    }

    pub fn poly_centroid(&self, pi: usize) -> V3 {
        let p = &self.polys[pi];
        p.iter().fold(V3::ZERO, |s, &i| s + self.positions[i]) / p.len() as f64
    }

    pub fn dist_point_poly(&self, q: V3, pi: usize) -> f64 {
        let p = &self.polys[pi];
        let n = self.planes[pi].n;
        let h = (q - self.positions[p[0]]).dot(n);
        let proj = q - n * h;
        let inside = (0..p.len()).all(|i| {
            let (a, b) = (self.positions[p[i]], self.positions[p[(i + 1) % p.len()]]);
            (b - a).cross(proj - a).dot(n) >= -1e-9
        });
        if inside { return h.abs(); }
        (0..p.len())
            .map(|i| {
                let (a, b) = (self.positions[p[i]], self.positions[p[(i + 1) % p.len()]]);
                (q - closest_on_segment(q, a, b).0).length()
            })
            .fold(f64::INFINITY, f64::min)
    }

    pub fn distance_to_face(&self, q: V3, f: usize) -> f64 {
        self.faces[f].polys.iter().map(|&pi| self.dist_point_poly(q, pi)).fold(f64::INFINITY, f64::min)
    }

    pub fn face_frame(&self, f: usize) -> Option<Frame> {
        let face = self.faces.get(f)?;
        if !face.planar { return None; }
        Some(Frame::plane_through(face.center, face.normal))
    }

    pub fn match_face(&self, point: V3, normal: V3) -> Option<usize> {
        let mut best = None;
        let mut best_d = f64::INFINITY;
        for (i, f) in self.faces.iter().enumerate() {
            if !f.planar || f.normal.dot(normal) <= 0.9995 { continue; }
            let d = self.distance_to_face(point, i);
            if d < best_d { best_d = d; best = Some(i); }
        }
        best
    }

    pub fn match_chain(&self, r: &EdgeRef) -> Option<usize> {
        let mut best = None;
        let mut best_s = f64::INFINITY;
        for (i, c) in self.chains.iter().enumerate() {
            let k = c.points.len() / 2;
            let (ma, mb) = (c.n_a[k], c.n_b[k]);
            let ok = (ma.dot(r.n1) > 0.95 && mb.dot(r.n2) > 0.95) || (ma.dot(r.n2) > 0.95 && mb.dot(r.n1) > 0.95);
            if !ok { continue; }
            let s = c.distance_to(r.a) + 2.0 * c.distance_to(r.m) + c.distance_to(r.b);
            if s < best_s { best_s = s; best = Some(i); }
        }
        best
    }

    pub fn edge_ref(&self, i: usize) -> EdgeRef {
        let c = &self.chains[i];
        let k = c.points.len() / 2;
        EdgeRef {
            a: c.points[0],
            m: c.mid(),
            b: if c.closed { c.points[c.points.len() / 3] } else { *c.points.last().unwrap() },
            n1: c.n_a[k],
            n2: c.n_b[k],
            closed: c.closed,
        }
    }

    pub fn face_ref(&self, f: usize) -> FaceRef { FaceRef { point: self.faces[f].center, normal: self.faces[f].normal } }

    /// Reference for any face: planar faces use their centre, curved faces a point on the
    /// surface (centroid of the largest facet) and that facet's normal.
    pub fn surface_ref(&self, f: usize) -> FaceRef {
        let face = &self.faces[f];
        if face.planar { return self.face_ref(f); }
        let mut best = (face.polys[0], -1.0);
        for &pi in &face.polys {
            let p = &self.polys[pi];
            let a: f64 = (1..p.len() - 1).map(|k| (self.positions[p[k]] - self.positions[p[0]]).cross(self.positions[p[k + 1]] - self.positions[p[0]]).length()).sum();
            if a > best.1 { best = (pi, a); }
        }
        let p = &self.polys[best.0];
        let c = p.iter().map(|&v| self.positions[v]).sum::<V3>() / p.len() as f64;
        FaceRef { point: c, normal: self.planes[best.0].n }
    }

    /// Curved face nearest to `point` whose facets there face along `normal`.
    pub fn match_surface(&self, point: V3, normal: V3) -> Option<usize> {
        let mut best = None;
        let mut best_d = f64::INFINITY;
        for (i, f) in self.faces.iter().enumerate() {
            if f.planar { continue; }
            for &pi in &f.polys {
                let d = self.dist_point_poly(point, pi);
                if d < best_d && self.planes[pi].n.dot(normal) > 0.7 { best_d = d; best = Some(i); }
            }
        }
        let tol = (self.bbox.diagonal() * 1e-3).max(1e-3);
        best.filter(|_| best_d <= tol)
    }

    /// Axis of a surface of revolution (cylinder or cone): (point, direction, mean radius).
    pub fn surface_axis(&self, f: usize) -> Option<(V3, V3, f64)> {
        let face = self.faces.get(f)?;
        if face.planar || face.polys.len() < 3 { return None; }
        let mut m = [[0.0f64; 3]; 3];
        let mut items = vec![];
        for &pi in &face.polys {
            let p = &self.polys[pi];
            let n = self.planes[pi].n;
            let a: f64 = (1..p.len() - 1).map(|k| (self.positions[p[k]] - self.positions[p[0]]).cross(self.positions[p[k + 1]] - self.positions[p[0]]).length()).sum::<f64>() * 0.5;
            let c = p.iter().map(|&v| self.positions[v]).sum::<V3>() / p.len() as f64;
            let na = n.to_array();
            for r in 0..3 { for k in 0..3 { m[r][k] += a * na[r] * na[k]; } }
            items.push((c, n, a));
        }
        let (vals, vecs) = sym_eigen3(m);
        // the axis is the eigenvector whose eigenvalue stands apart from the other two
        let mut k = 0;
        let mut best = -1.0;
        for i in 0..3 {
            let (o1, o2) = (vals[(i + 1) % 3], vals[(i + 2) % 3]);
            let sep = (vals[i] - (o1 + o2) * 0.5).abs() - (o1 - o2).abs();
            if sep > best { best = sep; k = i; }
        }
        let tr = vals[0] + vals[1] + vals[2];
        if tr <= 0.0 || best < tr * 0.2 { return None; }
        let d = vecs[k].normalize();
        // axis point: every facet plane lies at the same distance ρ from the axis, so solve
        // n·p − ρ = n·c in least squares (exact however the facets were split)
        let mut a = [0.0f64; 16];
        let mut b = [0.0f64; 4];
        let mut mean = V3::ZERO;
        let mut wsum = 0.0;
        for &(c, n, w) in &items {
            let row = [n.x, n.y, n.z, -1.0];
            let rhs = n.dot(c);
            for r in 0..4 { for q in 0..4 { a[r * 4 + q] += w * row[r] * row[q]; } b[r] += w * row[r] * rhs; }
            mean += c * w;
            wsum += w;
        }
        if wsum <= 0.0 { return None; }
        mean /= wsum;
        let (da, md) = (d.to_array(), mean.dot(d));
        for r in 0..3 { for q in 0..3 { a[r * 4 + q] += wsum * da[r] * da[q]; } b[r] += wsum * da[r] * md; }
        let x = solve_dense(&a, &b, 4)?;
        let p = V3::new(x[0], x[1], x[2]);
        let mut rs = 0.0;
        let mut n = 0.0f64;
        for &pi in &face.polys { for &v in &self.polys[pi] { let q = self.positions[v] - p; rs += (q - d * q.dot(d)).length(); n += 1.0; } }
        Some((p, d, rs / n.max(1.0)))
    }

    /// Distance to the first triangle hit by the ray `o + t d` with `t > min_t`.
    pub fn ray_first_hit(&self, o: V3, d: V3, min_t: f64) -> Option<f64> {
        let mut best: Option<f64> = None;
        for p in &self.polys {
            for k in 1..p.len() - 1 {
                let (a, b, c) = (self.positions[p[0]], self.positions[p[k]], self.positions[p[k + 1]]);
                let (e1, e2) = (b - a, c - a);
                let pv = d.cross(e2);
                let det = e1.dot(pv);
                if det.abs() < 1e-14 { continue; }
                let inv = 1.0 / det;
                let s = o - a;
                let u = s.dot(pv) * inv;
                if !(-1e-9..=1.0 + 1e-9).contains(&u) { continue; }
                let q = s.cross(e1);
                let v = d.dot(q) * inv;
                if v < -1e-9 || u + v > 1.0 + 1e-9 { continue; }
                let t = e2.dot(q) * inv;
                if t > min_t && best.map_or(true, |b| t < b) { best = Some(t); }
            }
        }
        best
    }

    /// Owning surface tag of a face.
    pub fn face_tag(&self, f: usize) -> u32 { self.tags[self.faces[f].polys[0]] }
}

/// Eigen-decomposition of a symmetric 3x3 matrix (cyclic Jacobi): eigenvalues and vectors.
pub fn sym_eigen3(m_in: [[f64; 3]; 3]) -> ([f64; 3], [V3; 3]) {
    let mut a = m_in;
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..50 {
        let off = a[0][1].abs() + a[0][2].abs() + a[1][2].abs();
        if off < 1e-15 * (a[0][0].abs() + a[1][1].abs() + a[2][2].abs()).max(1e-300) { break; }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1e-300 { continue; }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for k in 0..3 {
                let (akp, akq) = (a[k][p], a[k][q]);
                a[k][p] = c * akp - s * akq;
                a[k][q] = s * akp + c * akq;
            }
            for k in 0..3 {
                let (apk, aqk) = (a[p][k], a[q][k]);
                a[p][k] = c * apk - s * aqk;
                a[q][k] = s * apk + c * aqk;
            }
            for k in 0..3 {
                let (vkp, vkq) = (v[k][p], v[k][q]);
                v[k][p] = c * vkp - s * vkq;
                v[k][q] = s * vkp + c * vkq;
            }
        }
    }
    ([a[0][0], a[1][1], a[2][2]], [V3::new(v[0][0], v[1][0], v[2][0]), V3::new(v[0][1], v[1][1], v[2][1]), V3::new(v[0][2], v[1][2], v[2][2])])
}
