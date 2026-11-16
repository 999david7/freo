//! Solids as closed sets of convex planar polygons, combined with BSP-tree CSG
//! (after Evan Wallace's csg.js), using an index arena instead of pointers.

use super::math::*;

#[derive(Clone, Copy, Debug)]
pub struct CVertex {
    pub p: V3,
    pub n: V3,
}

impl CVertex {
    #[inline]
    pub fn lerp(&self, o: &CVertex, t: f64) -> CVertex {
        CVertex { p: self.p + (o.p - self.p) * t, n: self.n + (o.n - self.n) * t }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CPlane {
    pub n: V3,
    pub w: f64,
}

pub const EPS: f64 = 1e-5;

impl CPlane {
    pub fn flip(&mut self) {
        self.n = -self.n;
        self.w = -self.w;
    }

    pub fn newell(pts: &[V3]) -> Option<CPlane> {
        let k = pts.len();
        if k < 3 {
            return None;
        }
        let mut n = V3::ZERO;
        let mut c = V3::ZERO;
        for i in 0..k {
            let a = pts[i];
            let b = pts[(i + 1) % k];
            n.x += (a.y - b.y) * (a.z + b.z);
            n.y += (a.z - b.z) * (a.x + b.x);
            n.z += (a.x - b.x) * (a.y + b.y);
            c += a;
        }
        let l = n.length();
        if l < 1e-14 {
            return None;
        }
        n /= l;
        c /= k as f64;
        Some(CPlane { n, w: n.dot(c) })
    }
}

#[derive(Clone, Debug)]
pub struct CPolygon {
    pub v: Vec<CVertex>,
    pub plane: CPlane,
    pub tag: u32,
}

impl CPolygon {
    pub fn new(v: Vec<CVertex>, tag: u32) -> Option<CPolygon> {
        let pts: Vec<V3> = v.iter().map(|q| q.p).collect();
        let plane = CPlane::newell(&pts)?;
        Some(CPolygon { v, plane, tag })
    }

    pub fn flat(points: &[V3], tag: u32) -> Option<CPolygon> {
        let plane = CPlane::newell(points)?;
        Some(CPolygon { v: points.iter().map(|&p| CVertex { p, n: plane.n }).collect(), plane, tag })
    }

    pub fn flip(&mut self) {
        self.v.reverse();
        for q in &mut self.v {
            q.n = -q.n;
        }
        self.plane.flip();
    }

    pub fn flipped(&self) -> CPolygon {
        let mut c = self.clone();
        c.flip();
        c
    }

    pub fn transformed(&self, x: &Xform) -> CPolygon {
        let mut c = self.clone();
        for q in &mut c.v {
            q.p = x.point(q.p);
            q.n = safe_norm(x.vector(q.n), q.n);
        }
        let n = safe_norm(x.vector(self.plane.n), self.plane.n);
        c.plane = CPlane { n, w: n.dot(c.v[0].p) };
        if x.is_mirror() {
            c.v.reverse();
        }
        c
    }

    pub fn centroid(&self) -> V3 {
        self.v.iter().fold(V3::ZERO, |s, q| s + q.p) / self.v.len().max(1) as f64
    }
}

const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

fn split(plane: &CPlane, poly: CPolygon, cf: &mut Vec<CPolygon>, cb: &mut Vec<CPolygon>, f: &mut Vec<CPolygon>, b: &mut Vec<CPolygon>) {
    // Most polygons lie wholly on one side: classify without allocating, and keep the
    // per-vertex classes (on the stack for ordinary polygons) only for the ones that are cut.
    let mut ptype = 0u8;
    let side = |q: &CVertex| {
        let t = plane.n.dot(q.p) - plane.w;
        if t < -EPS {
            BACK
        } else if t > EPS {
            FRONT
        } else {
            COPLANAR
        }
    };
    for q in &poly.v {
        ptype |= side(q);
    }
    let mut stack = [0u8; 16];
    let heap: Vec<u8>;
    let types: &[u8] = if ptype != SPANNING {
        &[]
    } else if poly.v.len() <= 16 {
        for (i, q) in poly.v.iter().enumerate() {
            stack[i] = side(q);
        }
        &stack[..poly.v.len()]
    } else {
        heap = poly.v.iter().map(side).collect();
        &heap
    };
    match ptype {
        COPLANAR => {
            if plane.n.dot(poly.plane.n) > 0.0 {
                cf.push(poly)
            } else {
                cb.push(poly)
            }
        }
        FRONT => f.push(poly),
        BACK => b.push(poly),
        _ => {
            let n = poly.v.len();
            let mut fv = Vec::with_capacity(n + 1);
            let mut bv = Vec::with_capacity(n + 1);
            for i in 0..n {
                let j = (i + 1) % n;
                let (ti, tj) = (types[i], types[j]);
                let (vi, vj) = (poly.v[i], poly.v[j]);
                if ti != BACK {
                    fv.push(vi);
                }
                if ti != FRONT {
                    bv.push(vi);
                }
                if (ti | tj) == SPANNING {
                    let denom = plane.n.dot(vj.p - vi.p);
                    let t = if denom == 0.0 { 0.0 } else { (plane.w - plane.n.dot(vi.p)) / denom };
                    let nv = vi.lerp(&vj, t);
                    fv.push(nv);
                    bv.push(nv);
                }
            }
            if fv.len() >= 3 {
                f.push(CPolygon { v: fv, plane: poly.plane, tag: poly.tag });
            }
            if bv.len() >= 3 {
                b.push(CPolygon { v: bv, plane: poly.plane, tag: poly.tag });
            }
        }
    }
}

#[derive(Default)]
struct Node {
    plane: Option<CPlane>,
    front: Option<usize>,
    back: Option<usize>,
    polys: Vec<CPolygon>,
}

pub struct Bsp {
    nodes: Vec<Node>,
}

impl Bsp {
    pub fn new(polys: Vec<CPolygon>) -> Bsp {
        let mut b = Bsp { nodes: vec![Node::default()] };
        b.build(polys);
        b
    }

    pub fn build(&mut self, polys: Vec<CPolygon>) {
        let mut stack = vec![(0usize, polys)];
        while let Some((ni, ps)) = stack.pop() {
            if ps.is_empty() {
                continue;
            }
            if self.nodes[ni].plane.is_none() {
                self.nodes[ni].plane = Some(ps[0].plane);
            }
            let pl = self.nodes[ni].plane.unwrap();
            let (mut f, mut b, mut cf, mut cb) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for p in ps {
                split(&pl, p, &mut cf, &mut cb, &mut f, &mut b);
            }
            self.nodes[ni].polys.extend(cf);
            self.nodes[ni].polys.extend(cb);
            if !f.is_empty() {
                let fi = match self.nodes[ni].front {
                    Some(i) => i,
                    None => {
                        self.nodes.push(Node::default());
                        let i = self.nodes.len() - 1;
                        self.nodes[ni].front = Some(i);
                        i
                    }
                };
                stack.push((fi, f));
            }
            if !b.is_empty() {
                let bi = match self.nodes[ni].back {
                    Some(i) => i,
                    None => {
                        self.nodes.push(Node::default());
                        let i = self.nodes.len() - 1;
                        self.nodes[ni].back = Some(i);
                        i
                    }
                };
                stack.push((bi, b));
            }
        }
    }

    pub fn invert(&mut self) {
        for n in &mut self.nodes {
            for p in &mut n.polys {
                p.flip();
            }
            if let Some(pl) = &mut n.plane {
                pl.flip();
            }
            std::mem::swap(&mut n.front, &mut n.back);
        }
    }

    pub fn clip_polygons(&self, polys: Vec<CPolygon>) -> Vec<CPolygon> {
        let mut result = Vec::new();
        let mut stack = vec![(0usize, polys)];
        while let Some((ni, ps)) = stack.pop() {
            let node = &self.nodes[ni];
            let Some(pl) = node.plane else {
                result.extend(ps);
                continue;
            };
            let (mut f, mut b, mut cf, mut cb) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for p in ps {
                split(&pl, p, &mut cf, &mut cb, &mut f, &mut b);
            }
            f.extend(cf);
            b.extend(cb);
            if !f.is_empty() {
                match node.front {
                    Some(fi) => stack.push((fi, f)),
                    None => result.extend(f),
                }
            }
            if !b.is_empty() {
                if let Some(bi) = node.back {
                    stack.push((bi, b));
                }
            }
        }
        result
    }

    pub fn clip_to(&mut self, other: &Bsp) {
        for i in 0..self.nodes.len() {
            let ps = std::mem::take(&mut self.nodes[i].polys);
            self.nodes[i].polys = other.clip_polygons(ps);
        }
    }

    pub fn all_polygons(&self) -> Vec<CPolygon> {
        self.nodes.iter().flat_map(|n| n.polys.iter().cloned()).collect()
    }
}

pub fn bbox(polys: &[CPolygon]) -> BBox {
    let mut b = BBox::default();
    for p in polys {
        for q in &p.v {
            b.add(q.p);
        }
    }
    b
}

pub fn union(a: &[CPolygon], b: &[CPolygon]) -> Vec<CPolygon> {
    if a.is_empty() {
        return b.to_vec();
    }
    if b.is_empty() {
        return a.to_vec();
    }
    if !bbox(a).intersects(&bbox(b), 1e-6) {
        return [a, b].concat();
    }
    let mut aa = Bsp::new(a.to_vec());
    let mut bb = Bsp::new(b.to_vec());
    aa.clip_to(&bb);
    bb.clip_to(&aa);
    bb.invert();
    bb.clip_to(&aa);
    bb.invert();
    aa.build(bb.all_polygons());
    aa.all_polygons()
}

pub fn subtract(a: &[CPolygon], b: &[CPolygon]) -> Vec<CPolygon> {
    if a.is_empty() || b.is_empty() {
        return a.to_vec();
    }
    if !bbox(a).intersects(&bbox(b), 1e-6) {
        return a.to_vec();
    }
    let mut aa = Bsp::new(a.to_vec());
    let mut bb = Bsp::new(b.to_vec());
    aa.invert();
    aa.clip_to(&bb);
    bb.clip_to(&aa);
    bb.invert();
    bb.clip_to(&aa);
    bb.invert();
    aa.build(bb.all_polygons());
    aa.invert();
    aa.all_polygons()
}

/// Splits `a` into the polygons whose bounding box touches `bx` and the ones that lie clear of it.
fn near_far(a: &[CPolygon], bx: &BBox) -> (Vec<CPolygon>, Vec<CPolygon>) {
    let (mut near, mut far) = (Vec::new(), Vec::new());
    for p in a {
        let mut pb = BBox::default();
        for q in &p.v {
            pb.add(q.p);
        }
        if pb.intersects(bx, 1e-6) {
            near.push(p.clone())
        } else {
            far.push(p.clone())
        }
    }
    (near, far)
}

fn area(p: &CPolygon) -> f64 {
    let mut n = V3::ZERO;
    for i in 1..p.v.len().saturating_sub(1) {
        n += (p.v[i].p - p.v[0].p).cross(p.v[i + 1].p - p.v[0].p);
    }
    n.length() * 0.5
}

/// Joins `a` and `b` along an edge they share (b runs it the other way) if the joined polygon is
/// still convex; collinear vertices of the result are dropped.
fn join_convex(a: &CPolygon, b: &CPolygon) -> Option<CPolygon> {
    let (na, nb) = (a.v.len(), b.v.len());
    let same = |p: V3, q: V3| (p - q).length_squared() < 1e-20;
    for i in 0..na {
        let (p0, p1) = (a.v[i].p, a.v[(i + 1) % na].p);
        for j in 0..nb {
            if !(same(b.v[j].p, p1) && same(b.v[(j + 1) % nb].p, p0)) {
                continue;
            }
            // a from p1 round to p0, then b from p0 round to p1 (without repeating the shared ends)
            let mut v: Vec<CVertex> = (0..na).map(|k| a.v[(i + 1 + k) % na]).collect();
            v.extend((0..nb.saturating_sub(2)).map(|k| b.v[(j + 2 + k) % nb]));
            // drop collinear vertices
            let n = a.plane.n;
            let mut changed = true;
            while changed && v.len() > 3 {
                changed = false;
                for k in 0..v.len() {
                    let (pa, pb, pc) = (v[(k + v.len() - 1) % v.len()].p, v[k].p, v[(k + 1) % v.len()].p);
                    let cr = (pb - pa).cross(pc - pb);
                    if cr.length() <= 1e-9 * (pb - pa).length().max(1e-9) * (pc - pb).length().max(1e-9) + 1e-14 {
                        v.remove(k);
                        changed = true;
                        break;
                    }
                }
            }
            // convex: every corner turns the same way as the polygon's normal
            let ok = (0..v.len()).all(|k| {
                let (pa, pb, pc) = (v[(k + v.len() - 1) % v.len()].p, v[k].p, v[(k + 1) % v.len()].p);
                (pb - pa).cross(pc - pb).dot(n) > -1e-12
            });
            if !ok || v.len() < 3 {
                return None;
            }
            return Some(CPolygon { v, plane: a.plane, tag: a.tag });
        }
    }
    None
}

/// Joins the pieces of one split polygon back into as few convex polygons as possible.
fn rejoin(mut parts: Vec<CPolygon>) -> Vec<CPolygon> {
    if parts.len() < 2 || parts.len() > 4000 {
        return parts;
    }
    let mut merged = true;
    while merged {
        merged = false;
        'outer: for i in 0..parts.len() {
            for j in i + 1..parts.len() {
                if let Some(u) = join_convex(&parts[i], &parts[j]) {
                    parts[i] = u;
                    parts.swap_remove(j);
                    merged = true;
                    break 'outer;
                }
            }
        }
    }
    parts
}

/// Clips each polygon by `tree` on its own: a polygon that comes through whole (it was only split
/// by planes that pass by it, nothing was removed) is kept as the original, unsplit polygon.
fn clip_whole(tree: &Bsp, polys: Vec<CPolygon>) -> Vec<CPolygon> {
    let mut out = Vec::with_capacity(polys.len());
    for p in polys {
        let a0 = area(&p);
        let parts = tree.clip_polygons(vec![p.clone()]);
        let a1: f64 = parts.iter().map(area).sum();
        if parts.len() > 1 && (a1 - a0).abs() <= 1e-9 * a0.max(1e-12) {
            out.push(p)
        } else {
            out.extend(rejoin(parts))
        }
    }
    out
}

/// `Bsp::clip_to` with `clip_whole`.
fn clip_to_whole(t: &mut Bsp, other: &Bsp) {
    for i in 0..t.nodes.len() {
        let ps = std::mem::take(&mut t.nodes[i].polys);
        t.nodes[i].polys = clip_whole(other, ps);
    }
}

/// `subtract` for a small tool on a large solid (rounds, chamfers). The result is the same as
/// `subtract`, but the solid's polygons are only cut where the tool is: the polygons away from
/// the tool are kept whole, and the solid's BSP tree is used only to decide which parts of the
/// tool lie inside it, not to cut up the solid itself. This keeps the polygon count (and the time
/// of every later operation) from growing with each edge.
pub fn subtract_local(a: &[CPolygon], b: &[CPolygon]) -> Vec<CPolygon> {
    if a.is_empty() || b.is_empty() {
        return a.to_vec();
    }
    let bx = bbox(b);
    if !bbox(a).intersects(&bx, 1e-6) {
        return a.to_vec();
    }
    let (near, mut out) = near_far(a, &bx);
    let bb = Bsp::new(b.to_vec());
    // the solid's surface outside the tool (clipped as the inverted solid, like `subtract`)
    let kept = clip_whole(&bb, near.into_iter().map(|p| p.flipped()).collect());
    out.extend(kept.into_iter().map(|p| p.flipped()));
    // the tool's surface inside the solid, decided by the whole solid's tree
    let mut aa = Bsp::new(a.to_vec());
    aa.invert();
    let mut bt = bb;
    clip_to_whole(&mut bt, &aa);
    bt.invert();
    clip_to_whole(&mut bt, &aa);
    bt.invert();
    out.extend(bt.all_polygons().into_iter().map(|p| p.flipped()));
    out
}

/// `union` for a small tool on a large solid (concave rounds and chamfers), see `subtract_local`.
pub fn union_local(a: &[CPolygon], b: &[CPolygon]) -> Vec<CPolygon> {
    if a.is_empty() {
        return b.to_vec();
    }
    if b.is_empty() {
        return a.to_vec();
    }
    let bx = bbox(b);
    if !bbox(a).intersects(&bx, 1e-6) {
        return [a, b].concat();
    }
    let (near, mut out) = near_far(a, &bx);
    let bb = Bsp::new(b.to_vec());
    out.extend(clip_whole(&bb, near));
    let aa = Bsp::new(a.to_vec());
    let mut bt = bb;
    clip_to_whole(&mut bt, &aa);
    bt.invert();
    clip_to_whole(&mut bt, &aa);
    bt.invert();
    out.extend(bt.all_polygons());
    out
}

pub fn intersect(a: &[CPolygon], b: &[CPolygon]) -> Vec<CPolygon> {
    if a.is_empty() || b.is_empty() {
        return vec![];
    }
    if !bbox(a).intersects(&bbox(b), 1e-6) {
        return vec![];
    }
    let mut aa = Bsp::new(a.to_vec());
    let mut bb = Bsp::new(b.to_vec());
    aa.invert();
    bb.clip_to(&aa);
    bb.invert();
    aa.clip_to(&bb);
    bb.clip_to(&aa);
    aa.build(bb.all_polygons());
    aa.invert();
    aa.all_polygons()
}

pub fn signed_volume(polys: &[CPolygon]) -> f64 {
    let mut vol = 0.0;
    for p in polys {
        if p.v.len() < 3 {
            continue;
        }
        let a = p.v[0].p;
        for i in 1..p.v.len() - 1 {
            vol += a.dot(p.v[i].p.cross(p.v[i + 1].p));
        }
    }
    vol / 6.0
}

/// Orient a closed solid outward and make smooth vertex normals agree with their faces.
pub fn orient_outward(polys: Vec<CPolygon>) -> Vec<CPolygon> {
    let mut polys = if signed_volume(&polys) < 0.0 { polys.into_iter().map(|p| p.flipped()).collect() } else { polys };
    for p in &mut polys {
        let n = p.plane.n;
        for q in &mut p.v {
            if q.n.dot(n) < 0.0 {
                q.n = -q.n;
            }
        }
    }
    polys
}

pub fn dedupe(vs: Vec<CVertex>) -> Vec<CVertex> {
    let mut out: Vec<CVertex> = Vec::with_capacity(vs.len());
    for v in vs {
        if let Some(l) = out.last() {
            if (l.p - v.p).length() < 1e-9 {
                continue;
            }
        }
        out.push(v);
    }
    if out.len() > 1 && (out[0].p - out[out.len() - 1].p).length() < 1e-9 {
        out.pop();
    }
    out
}

/// Planar quad, or two triangles when non-planar.
pub fn quad(a: CVertex, b: CVertex, c: CVertex, d: CVertex, tag: u32) -> Vec<CPolygon> {
    let pts = [a.p, b.p, c.p, d.p];
    if let Some(pl) = CPlane::newell(&pts) {
        if pts.iter().all(|p| (pl.n.dot(*p) - pl.w).abs() < 1e-7) {
            let v = dedupe(vec![a, b, c, d]);
            if v.len() >= 3 {
                return vec![CPolygon { v, plane: pl, tag }];
            }
            return vec![];
        }
    }
    let mut out = Vec::new();
    for tri in [vec![a, b, c], vec![a, c, d]] {
        let t = dedupe(tri);
        if t.len() >= 3 {
            if let Some(p) = CPolygon::new(t, tag) {
                out.push(p);
            }
        }
    }
    out
}
