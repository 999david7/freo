//! Toolpath generation. Every operation is built on a height field of the stock that is cut
//! as the paths are made, so each approach knows how high the material really is: rapids
//! always end `clearance` above it, entries into material ramp down, and links that would
//! cross uncut material lift over it.

use super::dropcut::DropCutter;
use super::field::Field;
use super::tools::{Tool, ToolKind};
use super::*;
use crate::slicer::poly::{self, Contour, Pt, Shapes};
use glam::DVec3;

/// Part silhouettes from above: the region the tool may not enter at each height.
pub struct Shadows {
    /// (z, region of the part at heights ≥ z), z descending
    levels: Vec<(f64, Shapes)>,
}

fn clip_above(t: &[DVec3; 3], z: f64) -> Vec<DVec3> {
    // Sutherland–Hodgman against z ≥ zc
    let mut out = vec![];
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        let (ia, ib) = (a.z >= z, b.z >= z);
        if ia {
            out.push(a);
        }
        if ia != ib {
            let s = (z - a.z) / (b.z - a.z);
            out.push(a + (b - a) * s);
        }
    }
    out
}

fn clip_below(poly_in: &[DVec3], z: f64) -> Vec<DVec3> {
    let n = poly_in.len();
    let mut out = vec![];
    for k in 0..n {
        let (a, b) = (poly_in[k], poly_in[(k + 1) % n]);
        let (ia, ib) = (a.z <= z, b.z <= z);
        if ia {
            out.push(a);
        }
        if ia != ib {
            let s = (z - a.z) / (b.z - a.z);
            out.push(a + (b - a) * s);
        }
    }
    out
}

impl Shadows {
    /// Exact silhouettes: the triangles above each height, projected and united.
    pub fn new(part: &Mesh, zs: &[f64]) -> Shadows {
        let mut zs: Vec<f64> = zs.to_vec();
        zs.sort_by(|a, b| b.partial_cmp(a).unwrap());
        zs.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
        let tris: Vec<[DVec3; 3]> = (0..part.t.len()).map(|i| part.tri(i)).collect();
        let mut acc: Shapes = vec![];
        let mut levels = vec![];
        let mut upper = f64::INFINITY;
        for &z in &zs {
            let mut loops: Vec<Contour> = vec![];
            for t in &tris {
                let zmax = t[0].z.max(t[1].z).max(t[2].z);
                let zmin = t[0].z.min(t[1].z).min(t[2].z);
                if zmax < z || zmin > upper {
                    continue;
                }
                let mut p = clip_above(t, z);
                if upper.is_finite() {
                    p = clip_below(&p, upper);
                }
                if p.len() < 3 {
                    continue;
                }
                let mut c: Contour = p.iter().map(|q| [q.x, q.y]).collect();
                let a = poly::contour_area(&c);
                if a.abs() < 1e-10 {
                    continue;
                }
                if a < 0.0 {
                    c.reverse();
                }
                loops.push(c);
            }
            if !loops.is_empty() {
                let band = poly::from_loops(&loops);
                acc = poly::union(&acc, &band);
            }
            levels.push((z, acc.clone()));
            upper = z;
        }
        Shadows { levels }
    }

    /// Region of the part at heights ≥ z (z must be one of the levels).
    pub fn at(&self, z: f64) -> &Shapes {
        static EMPTY: Shapes = Vec::new();
        self.levels.iter().find(|(lz, _)| (lz - z).abs() < 1e-6).map(|(_, s)| s).unwrap_or(&EMPTY)
    }
}

/// Builds the moves of one operation on the shared height field.
struct Builder<'a> {
    f: &'a mut Field,
    t: &'a Tool,
    pos: DVec3,
    moves: Vec<CMove>,
    clearance: f64,
    safe: f64,
    feed: f64,
    plunge: f64,
    ramp: f64,
    part: &'a [f32],
    /// loops left out because they would have touched the part
    skipped: usize,
}

impl Builder<'_> {
    /// Lowest tip height over (x, y) at which the cutter, the shank and the holder are all
    /// `clearance` above the material.
    fn clear_at(&mut self, x: f64, y: f64) -> f64 {
        let t = self.t;
        let a = self.f.max_near(x, y, t.radius());
        let b = self.f.max_near(x, y, t.shank / 2.0) - t.flute_length;
        let c = self.f.max_near(x, y, t.holder / 2.0) - t.stickout;
        a.max(b).max(c) + self.clearance
    }
    fn clear_along(&mut self, from: (f64, f64), to: (f64, f64)) -> f64 {
        let t = self.t;
        let a = self.f.max_along(from, to, t.radius());
        let b = self.f.max_along(from, to, t.shank / 2.0) - t.flute_length;
        let c = self.f.max_along(from, to, t.holder / 2.0) - t.stickout;
        a.max(b).max(c) + self.clearance
    }
    fn stamp(&mut self, a: DVec3, b: DVec3) {
        self.f.sweep(self.t, a, b, Some(self.part));
    }
    fn rapid(&mut self, p: DVec3) {
        if (p - self.pos).length() < 1e-9 {
            return;
        }
        self.moves.push(CMove::line(p, true, 0.0));
        let a = self.pos;
        self.stamp(a, p);
        self.pos = p;
    }
    fn cut(&mut self, p: DVec3, feed: f64) {
        if (p - self.pos).length() < 1e-9 {
            return;
        }
        self.moves.push(CMove::line(p, false, feed));
        let a = self.pos;
        self.stamp(a, p);
        self.pos = p;
    }
    /// Feeds along an arc around `c` (direction `dir`: +1 ccw, -1 cw) to `p`; Z may change
    /// (a helix).
    fn arc(&mut self, p: DVec3, c: [f64; 2], dir: f64, feed: f64) {
        let mv = CMove { to: p, rapid: false, feed, arc: Some([c[0], c[1], dir]) };
        let mut a = self.pos;
        for q in mv.points(a) {
            self.stamp(a, q);
            a = q;
        }
        self.moves.push(mv);
        self.pos = p;
    }
    fn retract(&mut self) {
        if self.pos.z < self.safe {
            let p = DVec3::new(self.pos.x, self.pos.y, self.safe);
            self.rapid(p);
        }
    }
    /// Moves above (x, y): up only as far as the material along the way needs, across, then
    /// down to `clearance` above the material there (not below `z`). Returns the height.
    fn go_to(&mut self, x: f64, y: f64, z: f64) -> f64 {
        let along = self.clear_along((self.pos.x, self.pos.y), (x, y));
        let lift = along.max(self.pos.z).min(self.safe.max(along));
        if self.pos.z < lift {
            let p = DVec3::new(self.pos.x, self.pos.y, lift);
            self.rapid(p);
        }
        let travel = self.pos.z;
        self.rapid(DVec3::new(x, y, travel));
        let local = self.clear_at(x, y);
        let down = local.max(z);
        if down < self.pos.z {
            self.rapid(DVec3::new(x, y, down));
        }
        self.pos.z
    }
    /// Highest material the cutter meets along a closed loop.
    fn top_along(&self, ring: &[Pt]) -> f64 {
        // exactly the cells the cutter takes away (strictly inside its circle)
        let r = self.t.radius();
        let mut m = f64::NEG_INFINITY;
        let n = ring.len();
        for k in 0..n {
            let (a, b) = (ring[k], ring[(k + 1) % n]);
            let l = poly::dist(a, b);
            let steps = (l / (self.t.radius() * 0.25).max(self.f.cell)).ceil().max(1.0) as usize;
            for s in 0..steps {
                let t = s as f64 / steps as f64;
                m = m.max(self.f.max_exact(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, r));
            }
        }
        m
    }
    /// Cuts a closed loop at height z, entering from above: straight down through air,
    /// otherwise ramping down along the loop itself (never plunging into material).
    /// Would cutting this closed loop at height z touch the finished part? (Dry run with the
    /// exact sweep; entries above it are higher, so they are safe when the loop is.)
    fn touches_part(&mut self, ring: &[Pt], z: f64) -> bool {
        let n = ring.len();
        let t = self.t;
        let r = t.radius();
        for k in 0..n {
            let (a, b) = (ring[k], ring[(k + 1) % n]);
            let c = self.f.probe(t, DVec3::new(a[0], a[1], z), DVec3::new(b[0], b[1], z), self.part);
            if c.gouge < -0.002 {
                return true;
            }
            // material the flutes cannot reach: the shank or the holder would rub on it
            let l = poly::dist(a, b);
            let steps = (l / (r * 0.5).max(self.f.cell)).ceil().max(1.0) as usize;
            for s in 0..steps {
                let q = [a[0] + (b[0] - a[0]) * s as f64 / steps as f64, a[1] + (b[1] - a[1]) * s as f64 / steps as f64];
                if !self.point_ok_material(DVec3::new(q[0], q[1], z)) {
                    return true;
                }
            }
        }
        false
    }

    /// Cuts a closed loop unless it would touch the part (then it is left out: the material
    /// stays, which the check reports as rest material, never as a crash).
    fn safe_ring(&mut self, ring: &[Pt], z: f64) -> bool {
        if ring.len() < 3 || poly::contour_area(&ring.to_vec()).abs() < 0.01 {
            return false;
        }
        if self.touches_part(ring, z) {
            self.skipped += 1;
            return false;
        }
        self.ring(ring, z);
        true
    }

    fn ring(&mut self, ring: &[Pt], z: f64) {
        if ring.len() < 2 {
            return;
        }
        let start = ring[0];
        self.go_to(start[0], start[1], z);
        let top = self.top_along(ring);
        if top <= z + 1e-3 {
            self.cut(DVec3::new(start[0], start[1], z), self.plunge);
        } else {
            let from = (top + 0.2).min(self.pos.z);
            if from > z {
                self.cut(DVec3::new(start[0], start[1], from), self.plunge);
                self.ramp_loop(ring, from, z);
            }
            // the ramp ends back at the start, at depth
        }
        let n = ring.len();
        for k in 1..=n {
            let p = ring[k % n];
            self.cut(DVec3::new(p[0], p[1], z), self.feed);
        }
    }
    fn ramp_loop(&mut self, ring: &[Pt], from: f64, to: f64) {
        let tan = self.ramp.to_radians().tan().max(1e-3);
        let n = ring.len();
        let len: f64 = (0..n).map(|k| poly::dist(ring[k], ring[(k + 1) % n])).sum();
        if len < 1e-6 {
            self.cut(DVec3::new(ring[0][0], ring[0][1], to), self.plunge);
            return;
        }
        let mut z = from;
        let mut guard = 0;
        while z > to + 1e-9 && guard < 10_000 {
            for k in 1..=n {
                let (a, b) = (ring[k - 1], ring[k % n]);
                let l = poly::dist(a, b);
                z = (z - l * tan).max(to);
                self.cut(DVec3::new(b[0], b[1], z), self.feed.min(self.plunge * 2.0));
            }
            guard += 1;
        }
    }
    /// An open polyline at its own heights, entered from above.
    fn line(&mut self, pts: &[DVec3]) {
        if pts.is_empty() {
            return;
        }
        let s = pts[0];
        self.go_to(s.x, s.y, s.z);
        let top = self.f.max_exact(s.x, s.y, self.t.radius());
        if top > s.z + 1e-3 && self.pos.z > s.z {
            // material under the start: come down slowly
            self.cut(DVec3::new(s.x, s.y, s.z), self.plunge);
        } else {
            self.cut(s, self.plunge);
        }
        for p in &pts[1..] {
            self.cut(*p, self.feed);
        }
    }

    /// A surface-following line: every move is checked against the finished part with the
    /// exact sweep before it is made. A move that would touch it is split at the surface
    /// height (`drop_at`), and where even that is not enough the tool goes up first and
    /// comes down after (stairs instead of a diagonal through an edge).
    fn surface_line(&mut self, pts: &[DVec3], drop_at: &dyn Fn(f64, f64) -> Option<f64>) {
        // cut only stretches whose every move checks out; elsewhere the tool lifts over
        // (go_to keeps cutter, shank and holder above the material) and starts again
        let mut out: Vec<DVec3> = vec![];
        for &p in pts {
            if !self.point_ok(p) {
                self.skipped += usize::from(!out.is_empty());
                self.flush(&mut out);
                continue;
            }
            match out.last().copied() {
                None => out.push(p),
                Some(prev) => {
                    if !self.safe_segment(prev, p, drop_at, 0, &mut out) {
                        self.skipped += 1;
                        self.flush(&mut out);
                        out.push(p);
                    }
                }
            }
        }
        self.flush(&mut out);
    }

    fn flush(&mut self, out: &mut Vec<DVec3>) {
        if out.len() > 1 {
            let v = std::mem::take(out);
            self.line(&v);
        }
        out.clear();
    }

    /// Material and part allow the tool tip here: no gouge, nothing above the flutes within
    /// the shank, nothing above the stick-out within the holder.
    fn point_ok(&mut self, q: DVec3) -> bool {
        let t = self.t;
        let (sr, hr) = ((t.shank / 2.0).max(t.radius()), t.holder / 2.0);
        if self.f.probe(t, q, q, self.part).gouge < -0.002 {
            return false;
        }
        // holder: conservative (block maxima) — may leave material, never too little room
        if self.f.max_near(q.x, q.y, hr) > q.z + t.stickout - 0.01 {
            return false;
        }
        if self.f.max_near(q.x, q.y, sr) > q.z + t.flute_length - 0.01 && self.f.max_exact(q.x, q.y, sr) > q.z + t.flute_length - 0.01 {
            return false;
        }
        true
    }

    /// The move neither touches the part nor brings shank or holder onto material.
    fn clean(&mut self, a: DVec3, b: DVec3) -> bool {
        if self.f.probe(self.t, a, b, self.part).gouge < -0.002 {
            return false;
        }
        let r = self.t.radius();
        let l = (b - a).truncate().length();
        let steps = (l / (r * 0.5).max(self.f.cell)).ceil().max(1.0) as usize;
        for s in 1..=steps {
            let q = a + (b - a) * (s as f64 / steps as f64);
            // the lower end's height along the way: the stricter case
            let q = DVec3::new(q.x, q.y, a.z.min(b.z));
            if !self.point_ok_material(q) {
                return false;
            }
        }
        true
    }

    fn point_ok_material(&mut self, q: DVec3) -> bool {
        let t = self.t;
        let (sr, hr) = ((t.shank / 2.0).max(t.radius()), t.holder / 2.0);
        if self.f.max_near(q.x, q.y, hr) > q.z + t.stickout - 0.01 {
            return false;
        }
        !(self.f.max_near(q.x, q.y, sr) > q.z + t.flute_length - 0.01 && self.f.max_exact(q.x, q.y, sr) > q.z + t.flute_length - 0.01)
    }

    /// Appends a safe way from `a` to `b` (subdivided at the surface, or stairs); false when
    /// there is none.
    fn safe_segment(&mut self, a: DVec3, b: DVec3, drop_at: &dyn Fn(f64, f64) -> Option<f64>, depth: u32, out: &mut Vec<DVec3>) -> bool {
        if self.clean(a, b) {
            out.push(b);
            return true;
        }
        let m = (a + b) / 2.0;
        if depth < 10 && (b - a).truncate().length() > 1e-3 {
            if let Some(mz) = drop_at(m.x, m.y) {
                let mid = DVec3::new(m.x, m.y, mz.max(m.z.min(a.z.max(b.z))));
                if self.point_ok(mid) {
                    return self.safe_segment(a, mid, drop_at, depth + 1, out) && self.safe_segment(mid, b, drop_at, depth + 1, out);
                }
            }
        }
        // stairs: up at the lower end, across at the higher height, down at the other end
        let top = a.z.max(b.z);
        let up = DVec3::new(a.x, a.y, top);
        let over = DVec3::new(b.x, b.y, top);
        if self.clean(a, up) && self.clean(up, over) && self.clean(over, b) {
            out.push(up);
            out.push(over);
            out.push(b);
            return true;
        }
        false
    }
}

/// Rotates a closed loop to start at the vertex nearest `p`.
fn start_near(c: &Contour, p: (f64, f64)) -> Contour {
    let k = (0..c.len()).min_by(|&a, &b| ((c[a][0] - p.0).hypot(c[a][1] - p.1)).partial_cmp(&(c[b][0] - p.0).hypot(c[b][1] - p.1)).unwrap()).unwrap_or(0);
    c[k..].iter().chain(c[..k].iter()).cloned().collect()
}

fn rect(lo: DVec3, hi: DVec3, grow: f64) -> Shapes {
    poly::rect(lo.x - grow, lo.y - grow, hi.x + grow, hi.y + grow)
}

/// Highest part surface within `r` of (x, y) on the part height grid.
fn part_max_near(f: &Field, part: &[f32], x: f64, y: f64, r: f64) -> f64 {
    let c = f.cell;
    let (i0, i1) = ((((x - r - f.x0) / c) - 0.5).floor().max(0.0) as usize, ((((x + r - f.x0) / c) - 0.5).ceil().max(0.0) as usize).min(f.nx - 1));
    let (j0, j1) = ((((y - r - f.y0) / c) - 0.5).floor().max(0.0) as usize, ((((y + r - f.y0) / c) - 0.5).ceil().max(0.0) as usize).min(f.ny - 1));
    let mut m = f64::NEG_INFINITY;
    for j in j0..=j1 {
        for i in i0..=i1 {
            let (cx, cy) = f.center(i, j);
            if (cx - x).powi(2) + (cy - y).powi(2) <= r * r {
                m = m.max(part[j * f.nx + i] as f64);
            }
        }
    }
    m
}

/// Replaces runs of short feed moves at one height that lie on a circle (within 3 µm) by
/// single arcs (G2/G3): smooth, exact holes and rounds, and no slow-down at every segment.
/// An arc bulges past the straight segments it replaces; it is only used where that stays
/// clear of the part.
fn fit_arcs(moves: Vec<CMove>, start: DVec3, f: &Field, part: &[f32], r: f64) -> Vec<CMove> {
    let mut out: Vec<CMove> = Vec::with_capacity(moves.len());
    let mut prev = start;
    let mut i = 0;
    let pts: Vec<DVec3> = moves.iter().map(|m| m.to).collect();
    let n = moves.len();
    // circle through three points (x, y)
    let circle = |a: DVec3, b: DVec3, c: DVec3| -> Option<([f64; 2], f64)> {
        let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
        if d.abs() < 1e-12 {
            return None;
        }
        let (a2, b2, c2) = (a.x * a.x + a.y * a.y, b.x * b.x + b.y * b.y, c.x * c.x + c.y * c.y);
        let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
        let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
        Some(([ux, uy], (a.x - ux).hypot(a.y - uy)))
    };
    while i < n {
        let m = moves[i];
        let flat = |k: usize, from: DVec3| !moves[k].rapid && moves[k].arc.is_none() && (moves[k].to.z - from.z).abs() < 1e-9 && (moves[k].feed - m.feed).abs() < 1e-9;
        // longest run i..=j of at least 4 segments that fits one arc
        let mut best: Option<(usize, [f64; 2], f64)> = None;
        if flat(i, prev) {
            let mut j = i;
            while j + 1 < n && flat(j + 1, pts[j]) {
                j += 1;
            }
            let mut end = j;
            while end >= i + 3 {
                let seg = |k: usize| if k == i { prev } else { pts[k - 1] };
                let (a, b, c) = (prev, pts[(i + end) / 2], pts[end]);
                if let Some((ctr, rad)) = circle(a, b, c) {
                    if rad > 0.2 && rad < 500.0 {
                        let dir = if (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x) > 0.0 { 1.0 } else { -1.0 };
                        let mut ok = true;
                        let mut total = 0.0;
                        for k in i..=end {
                            let (p0, p1) = (seg(k), pts[k]);
                            if ((p1.x - ctr[0]).hypot(p1.y - ctr[1]) - rad).abs() > 0.003 {
                                ok = false;
                                break;
                            }
                            // every step turns the same way, by less than 30°
                            let s0 = (p0.y - ctr[1]).atan2(p0.x - ctr[0]);
                            let s1 = (p1.y - ctr[1]).atan2(p1.x - ctr[0]);
                            let mut d = s1 - s0;
                            while d > std::f64::consts::PI {
                                d -= std::f64::consts::TAU;
                            }
                            while d < -std::f64::consts::PI {
                                d += std::f64::consts::TAU;
                            }
                            if d * dir <= 0.0 || d.abs() > 0.52 {
                                ok = false;
                                break;
                            }
                            total += d.abs();
                        }
                        if ok && total < 5.5 {
                            // where the arc bulges out between two points: still clear of the part
                            let z = prev.z;
                            for k in i..=end {
                                let (p0, p1) = (seg(k), pts[k]);
                                let a0 = (p0.y - ctr[1]).atan2(p0.x - ctr[0]);
                                let mut d = (p1.y - ctr[1]).atan2(p1.x - ctr[0]) - a0;
                                while d > std::f64::consts::PI {
                                    d -= std::f64::consts::TAU;
                                }
                                while d < -std::f64::consts::PI {
                                    d += std::f64::consts::TAU;
                                }
                                let am = a0 + d / 2.0;
                                let (qx, qy) = (ctr[0] + rad * am.cos(), ctr[1] + rad * am.sin());
                                if part_max_near(f, part, qx, qy, r - 0.012) > z + 0.005 {
                                    ok = false;
                                    break;
                                }
                            }
                            if ok {
                                best = Some((end, ctr, dir));
                                break;
                            }
                        }
                    }
                }
                // shorter run: halve towards the start, then step
                end = if end - i > 12 { i + (end - i) / 2 } else { end - 1 };
            }
        }
        match best {
            Some((end, ctr, dir)) => {
                out.push(CMove { to: pts[end], rapid: false, feed: m.feed, arc: Some([ctr[0], ctr[1], dir]) });
                prev = pts[end];
                i = end + 1;
            }
            None => {
                out.push(m);
                prev = m.to;
                i += 1;
            }
        }
    }
    out
}

/// Round vertical holes the drilling does not make (no drill of their size, or drilling
/// switched off) and that the cutter fits into with room to spare: these are bored with a
/// helix instead of being cleared like pockets.
fn holes_to_bore(job: &Job, s: &Setup, tool: &Tool) -> Vec<(f64, f64, f64, f64, f64)> {
    let drilling = s.ops.iter().any(|o| o.kind == OpKind::Drill && o.enabled);
    let drills: Vec<&Tool> = s.tools.iter().filter(|t| t.kind == ToolKind::Drill).collect();
    find_holes(job)
        .into_iter()
        .filter(|h| !(drilling && drills.iter().any(|t| h.2 - t.diameter >= -1e-3 && h.2 - t.diameter <= 0.1)))
        .filter(|h| tool.kind == ToolKind::Flat && tool.diameter <= h.2 * 0.85 && h.3 - h.4 > 0.5)
        .collect()
}

fn disc(x: f64, y: f64, r: f64) -> Shapes {
    let n = 96;
    vec![vec![(0..n).map(|k| {
        let a = std::f64::consts::TAU * k as f64 / n as f64;
        [x + r * a.cos(), y + r * a.sin()]
    }).collect()]]
}

/// Cells where the part has sloped or curved faces (not flat, not vertical), grown by `grow`:
/// where 3D finishing is needed.
fn slope_mask(job: &Job, f: &Field, grow: f64) -> Vec<bool> {
    let mut m = vec![false; f.nx * f.ny];
    for ti in 0..job.part.t.len() {
        let t = job.part.tri(ti);
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        let l = n.length();
        if l < 1e-12 || n.z <= 0.0 {
            continue;
        }
        let nz = n.z / l;
        if nz < 0.02 || nz > 0.999 {
            continue;
        }
        let (lo, hi) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
        let i0 = (((lo.x - grow - f.x0) / f.cell).floor().max(0.0)) as usize;
        let j0 = (((lo.y - grow - f.y0) / f.cell).floor().max(0.0)) as usize;
        let i1 = ((((hi.x + grow - f.x0) / f.cell).ceil()).max(0.0) as usize).min(f.nx - 1);
        let j1 = ((((hi.y + grow - f.y0) / f.cell).ceil()).max(0.0) as usize).min(f.ny - 1);
        for j in j0..=j1 {
            for i in i0..=i1 {
                if m[j * f.nx + i] {
                    continue;
                }
                let (cx, cy) = f.center(i, j);
                // distance from the cell to the triangle's footprint
                let inside = super::dropcut::tri_z(&t, cx, cy).is_some();
                let near = inside || (0..3).any(|e| {
                    let (a, b) = (t[e], t[(e + 1) % 3]);
                    let (dx, dy) = (b.x - a.x, b.y - a.y);
                    let w = ((cx - a.x) * dx + (cy - a.y) * dy) / (dx * dx + dy * dy).max(1e-18);
                    let w = w.clamp(0.0, 1.0);
                    (a.x + dx * w - cx).hypot(a.y + dy * w - cy) <= grow
                });
                if near {
                    m[j * f.nx + i] = true;
                }
            }
        }
    }
    m
}

/// Whether the part has faces that are neither flat nor vertical (chamfers, slopes, rounds
/// on horizontal edges): contour milling leaves them as steps.
pub fn has_slopes(part: &Mesh) -> bool {
    let mut area = 0.0;
    for i in 0..part.t.len() {
        let [a, b, c] = part.tri(i);
        let n = (b - a).cross(c - a);
        let l = n.length();
        if l < 1e-12 {
            continue;
        }
        let nz = (n.z / l).abs();
        if nz > 0.02 && nz < 0.999 && n.z > 0.0 {
            area += l * 0.5;
        }
    }
    area > 1.0
}

/// Heights of upward-facing flat faces of the part (pocket floors, steps, the top).
fn flat_heights(part: &Mesh) -> Vec<f64> {
    let mut zs: Vec<(f64, f64)> = vec![];
    for i in 0..part.t.len() {
        let [a, b, c] = part.tri(i);
        let n = (b - a).cross(c - a);
        let l = n.length();
        if l < 1e-12 || n.z / l < 0.9999 {
            continue;
        }
        let z = (a.z + b.z + c.z) / 3.0;
        match zs.iter_mut().find(|(zz, _)| (zz - &z).abs() < 1e-3) {
            Some(e) => e.1 += l * 0.5,
            None => zs.push((z, l * 0.5)),
        }
    }
    zs.into_iter().filter(|(_, a)| *a > 1e-3).map(|(z, _)| z).collect()
}

/// Levels from just below `top` down to `bottom` every `step`, plus `extra` heights.
fn levels(top: f64, bottom: f64, step: f64, extra: &[f64]) -> Vec<f64> {
    let mut v = vec![];
    if top <= bottom + 1e-6 {
        return v;
    }
    let n = ((top - bottom) / step).ceil().max(1.0) as usize;
    for k in 1..=n {
        v.push(top - (top - bottom) * k as f64 / n as f64);
    }
    for &e in extra {
        if e < top - 1e-6 && e > bottom - 1e-6 {
            v.push(e);
        }
    }
    v.sort_by(|a, b| b.partial_cmp(a).unwrap());
    v.dedup_by(|a, b| (*a - *b).abs() < 0.02);
    v
}

/// Material per cell that must not be cut: the part's top surface, shrunk sideways by a
/// small tolerance so a cutter touching a wall does not count as a gouge.
pub fn part_height(job: &Job, f: &Field, tol_xy: f64) -> Vec<f32> {
    let mut hmax = vec![f32::NEG_INFINITY; f.nx * f.ny];
    let offsets = [(0.0, 0.0), (tol_xy, 0.0), (-tol_xy, 0.0), (0.0, tol_xy), (0.0, -tol_xy)];
    let mut out = vec![f32::INFINITY; f.nx * f.ny];
    for (ox, oy) in offsets {
        hmax.iter_mut().for_each(|h| *h = f32::NEG_INFINITY);
        for ti in 0..job.part.t.len() {
            let t = job.part.tri(ti);
            let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
            for p in &t {
                lo = [lo[0].min(p.x), lo[1].min(p.y)];
                hi = [hi[0].max(p.x), hi[1].max(p.y)];
            }
            let i0 = (((lo[0] - ox - f.x0) / f.cell - 0.5).floor().max(0.0)) as usize;
            let j0 = (((lo[1] - oy - f.y0) / f.cell - 0.5).floor().max(0.0)) as usize;
            let i1 = ((((hi[0] - ox - f.x0) / f.cell - 0.5).ceil()).max(0.0) as usize).min(f.nx - 1);
            let j1 = ((((hi[1] - oy - f.y0) / f.cell - 0.5).ceil()).max(0.0) as usize).min(f.ny - 1);
            for j in j0..=j1.min(f.ny - 1) {
                for i in i0..=i1 {
                    let (cx, cy) = f.center(i, j);
                    if let Some(z) = super::dropcut::tri_z(&t, cx + ox, cy + oy) {
                        let k = j * f.nx + i;
                        if z as f32 > hmax[k] {
                            hmax[k] = z as f32;
                        }
                    }
                }
            }
        }
        for k in 0..out.len() {
            out[k] = out[k].min(hmax[k]);
        }
    }
    out
}

pub fn generate(job: &Job, s: &Setup, prog: &Progress) -> Result<Program, String> {
    let m = s.material();
    let mc = &s.machine;
    let size = job.stock_size();
    let cell = Field::cell_for(size.x, size.y);
    let mut f = Field::new(job.stock_lo.x, job.stock_lo.y, job.stock_hi.x, job.stock_hi.y, job.stock_lo.z, job.stock_hi.z, cell);
    let part_h = part_height(job, &f, 0.01);
    let safe = job.stock_hi.z + s.safe_z;
    let mut program = Program { safe_z: safe, ..Default::default() };
    let floor_z = job.part_lo.z;
    let top_z = job.part_hi.z;
    let flats: Vec<f64> = flat_heights(&job.part).into_iter().filter(|z| *z < top_z - 1e-6).collect();
    let enabled: Vec<&Op> = s.ops.iter().filter(|o| o.enabled).collect();
    let nops = enabled.len().max(1);
    let mut pos = DVec3::new(job.stock_lo.x, job.stock_lo.y, safe);
    for (oi, op) in enabled.iter().enumerate() {
        if prog.cancelled() {
            return Err("Berechnung abgebrochen.".into());
        }
        prog.set(&format!("Bahnen: {}", op.kind.name()), 20 + oi * 480 / nops);
        if op.kind == OpKind::Drill {
            for path in drill(job, s, &mut f, &part_h, &mut pos, safe) {
                program.ops.push(path);
            }
            continue;
        }
        let mut tool = s.tool(op.tool).ok_or(format!("Werkzeug T{} fehlt", op.tool))?.clone();
        // clamp the cutter as long as the part needs, so the collet nut stays above the part
        // down to the lowest cut (instead of leaving material standing around tall walls)
        let mut slender = 1.0;
        if matches!(op.kind, OpKind::Rough | OpKind::Contour | OpKind::Parallel) {
            let needed = top_z - floor_z + 2.0;
            if needed > tool.stickout + 1e-6 {
                let longest = tool.max_stickout();
                let new = needed.min(longest);
                program.notes.push(format!(
                    "{}: {} mit {:.0} mm Auskraglänge spannen (statt {:.0} mm), damit der Halter über dem Teil bleibt.",
                    op.kind.name(),
                    tool.label(),
                    new,
                    tool.stickout
                ));
                if needed > longest + 1e-6 {
                    program.notes.push(format!(
                        "{}: {} reicht höchstens {:.0} mm aus dem Halter; neben Wänden über {:.0} mm Höhe bleibt Material stehen.",
                        op.kind.name(),
                        tool.label(),
                        longest,
                        longest - 2.0
                    ));
                }
                tool.stickout = new;
                // a long, thin cutter bends: beyond four diameters of reach, feed and depth of
                // cut drop with the square root of the ratio (about 30 % less at 8 × D)
                slender = (4.0 * tool.diameter / tool.stickout).sqrt().clamp(0.6, 1.0);
            }
        }
        if op.kind == OpKind::Contour || op.kind == OpKind::Rough {
            if has_slopes(&job.part) && !s.ops.iter().any(|o| o.kind == OpKind::Parallel && o.enabled) && op.kind == OpKind::Contour {
                program.notes.push("Das Teil hat schräge oder gekrümmte Flächen: ohne 3D-Schlichten bleiben sie gestuft.".into());
            }
        }
        let (mut c, notes) = cutting(&tool, m, mc);
        program.notes.extend(notes);
        c.feed *= slender;
        c.ap *= slender;
        if let Some(r) = op.rpm {
            c.rpm = r;
        }
        if let Some(fd) = op.feed {
            c.feed = fd;
            c.plunge = (fd * 0.3).max(10.0);
        }
        if let Some(sd) = op.stepdown {
            c.ap = sd;
        }
        if let Some(so) = op.stepover {
            c.ae = so * tool.diameter;
        }
        let mut b = Builder { f: &mut f, t: &tool, pos, moves: vec![], clearance: s.clearance, safe, feed: c.feed, plunge: c.plunge, ramp: c.ramp, part: &part_h, skipped: 0 };
        // start each operation from the safe height
        b.moves.push(CMove::line(DVec3::new(pos.x, pos.y, safe), true, 0.0));
        b.pos = DVec3::new(pos.x, pos.y, safe);
        let r = tool.radius();
        match op.kind {
            OpKind::Face => {
                if job.stock_hi.z > top_z + 1e-6 {
                    let d = tool.diameter;
                    let lv = levels(job.stock_hi.z, top_z, c.ap.max(0.05), &[]);
                    let (y0, y1) = (job.stock_lo.y + 0.3 * d, job.stock_hi.y - 0.3 * d);
                    let nl = (((y1 - y0) / (0.7 * d)).ceil() as usize).max(1);
                    let (xa, xb) = (job.stock_lo.x - r - 1.0, job.stock_hi.x + r + 1.0);
                    for z in lv {
                        let mut pts = vec![];
                        for k in 0..=nl {
                            let y = if y1 > y0 { y0 + (y1 - y0) * k as f64 / nl as f64 } else { (y0 + y1) / 2.0 };
                            let (u, v) = if k % 2 == 0 { (xa, xb) } else { (xb, xa) };
                            pts.push(DVec3::new(u, y, z));
                            pts.push(DVec3::new(v, y, z));
                        }
                        b.line(&pts);
                    }
                }
            }
            OpKind::Rough | OpKind::Contour => {
                let finish = op.kind == OpKind::Contour;
                let leave = if finish { 0.0 } else { op.leave };
                let step = if finish { (c.ap * 2.0).min(tool.flute_length * 0.8).max(0.1) } else { c.ap.max(0.05) };
                let extra: Vec<f64> = flats.iter().map(|z| z + leave).chain(std::iter::once(floor_z + leave)).collect();
                let lv = levels(job.stock_hi.z.min(if finish { top_z } else { job.stock_hi.z }), floor_z + leave, step, &extra);
                let mut shadow_z: Vec<f64> = lv.iter().map(|z| z + 1e-4).collect();
                // where the collet nut and a thicker shank pass: part above z + stick-out
                let holder_lift = tool.stickout - 1.0;
                let shank_lift = tool.flute_length - 0.5;
                let thick_shank = tool.shank / 2.0 > r + 1e-6;
                shadow_z.extend(lv.iter().map(|z| z + holder_lift));
                if thick_shank {
                    shadow_z.extend(lv.iter().map(|z| z + shank_lift));
                }
                shadow_z.push(top_z + 1.0);
                let sh = Shadows::new(&job.part, &shadow_z);
                // round holes bored with a helix at the end of roughing: no pocketing there
                let bored = {
                    let rough_tool = s.ops.iter().find(|o| o.kind == OpKind::Rough && o.enabled).and_then(|o| s.tool(o.tool));
                    match rough_tool {
                        Some(rt) => holes_to_bore(job, s, rt),
                        None => vec![],
                    }
                };
                let stock = rect(job.stock_lo, job.stock_hi, 0.0);
                let domain = poly::offset_tool(&stock, r - c.ae.min(r) * 0.5);
                let nlv = lv.len().max(1);
                for (li, &z) in lv.iter().enumerate() {
                    if prog.cancelled() {
                        return Err("Berechnung abgebrochen.".into());
                    }
                    prog.set(&format!("Bahnen: {} ({} von {})", op.kind.name(), li + 1, nlv), 20 + oi * 480 / nops + li * 480 / nops / nlv);
                    let mut forbid = poly::offset_tool(sh.at(z + 1e-4), r + leave);
                    let holder_zone = poly::offset_tool(sh.at(z + holder_lift), tool.holder / 2.0 + 0.5);
                    if !holder_zone.is_empty() {
                        forbid = poly::union(&forbid, &holder_zone);
                    }
                    if thick_shank {
                        let shank_zone = poly::offset_tool(sh.at(z + shank_lift), tool.shank / 2.0 + 0.2);
                        if !shank_zone.is_empty() {
                            forbid = poly::union(&forbid, &shank_zone);
                        }
                    }
                    let mut allowed = poly::diff(&domain, &forbid);
                    for h in bored.iter().filter(|h| z < h.3 - 1e-6) {
                        allowed = poly::diff(&allowed, &disc(h.0, h.1, h.2 / 2.0 + 0.05));
                    }
                    let is_floor = extra.iter().any(|e| (e - z).abs() < 0.02);
                    if finish {
                        // walls: the boundary of the forbidden region, climb milling
                        for shp in &forbid {
                            for cont in shp {
                                let mut c2: Contour = cont.iter().rev().cloned().collect();
                                c2 = poly::simplify_contour(&c2, 0.002);
                                if c2.len() < 3 {
                                    continue;
                                }
                                // only where the cutter centre is inside the reachable area
                                if !poly::in_shapes(&domain, c2[0]) {
                                    continue;
                                }
                                // the walls of bored holes are finished already
                                if bored.iter().any(|h| z < h.3 - 1e-6 && c2.iter().all(|q| (q[0] - h.0).hypot(q[1] - h.1) < h.2 / 2.0 + 0.05)) {
                                    continue;
                                }
                                let c2 = start_near(&c2, (b.pos.x, b.pos.y));
                                b.safe_ring(&c2, z);
                            }
                        }
                        if !is_floor {
                            continue;
                        }
                    }
                    clear_area(&mut b, &allowed, &domain, z, c.ae.max(0.05));
                }
                if !finish {
                    for h in &bored {
                        bore(&mut b, job, h, c.ae.max(0.05), c.ap.max(0.05), c.feed, s.clearance);
                    }
                }
            }
            OpKind::Parallel => {
                let dc = DropCutter::new(&job.part, r + 0.5);
                // the collet nut and a thicker shank must stay above the part as well
                let holder_tool = Tool { kind: ToolKind::Flat, diameter: tool.holder + 1.0, ..tool.clone() };
                let dh = DropCutter::new(&job.part, holder_tool.radius() + 0.5);
                let shank_tool = Tool { kind: ToolKind::Flat, diameter: tool.shank + 0.4, ..tool.clone() };
                let thick_shank = tool.shank > tool.diameter + 1e-6;
                let ds = if thick_shank { Some(DropCutter::new(&job.part, shank_tool.radius() + 0.5)) } else { None };
                let silhouette = Shadows::new(&job.part, &[floor_z - 1.0]);
                // only where the part has sloped or curved faces; walls, floors and holes are
                // finished by contour milling already
                let mask = slope_mask(job, b.f, r + 1.0);
                let (mnx, mx0, my0, mcell, mny) = (b.f.nx, b.f.x0, b.f.y0, b.f.cell, b.f.ny);
                let near_slope = |x: f64, y: f64| {
                    let i = ((x - mx0) / mcell).floor();
                    let j = ((y - my0) / mcell).floor();
                    i >= 0.0 && j >= 0.0 && (i as usize) < mnx && (j as usize) < mny && mask[j as usize * mnx + i as usize]
                };
                let area = poly::inter(&poly::offset_tool(silhouette.at(floor_z - 1.0), r + 1.0), &rect(job.stock_lo, job.stock_hi, 0.0));
                if let Some((lo, hi)) = poly::bbox(&area) {
                    let so = c.ae.min(tool.diameter * 0.15).max(0.05);
                    let n = ((hi[1] - lo[1]) / so).ceil() as usize;
                    let step = (tool.diameter * 0.05).clamp(0.05, 0.3);
                    for k in 0..=n {
                        let y = lo[1] + (hi[1] - lo[1]) * k as f64 / n.max(1) as f64;
                        let mut xs: Vec<f64> = vec![];
                        let mut x = lo[0];
                        while x <= hi[0] + 1e-9 {
                            xs.push(x);
                            x += step;
                        }
                        if k % 2 == 1 {
                            xs.reverse();
                        }
                        // runs of points over the part, each cut as one line; between two
                        // points the straight move must not dip under the surface: halve the
                        // step until the middle lies within 2 µm of the line
                        let mut run: Vec<DVec3> = vec![];
                        let drop2 = |x: f64, y: f64| {
                            let z = dc.drop(&tool, x, y)?;
                            let mut z = z.max(floor_z);
                            if let Some(h) = dh.drop(&holder_tool, x, y) {
                                z = z.max(h - tool.stickout + 1.0);
                            }
                            if let Some(ds) = &ds {
                                if let Some(h) = ds.drop(&shank_tool, x, y) {
                                    z = z.max(h - tool.flute_length + 0.5);
                                }
                            }
                            Some(z)
                        };
                        let drop_at = |x: f64| drop2(x, y);
                        for &x in &xs {
                            match drop_at(x) {
                                Some(z) if near_slope(x, y) && poly::in_shapes(&area, [x, y]) => {
                                    let p = DVec3::new(x, y, z);
                                    if let Some(&prev) = run.last() {
                                        refine(&drop_at, prev, p, 0, &mut run);
                                    }
                                    run.push(p);
                                }
                                _ => {
                                    if run.len() > 1 {
                                        b.surface_line(&run, &drop2);
                                    }
                                    run.clear();
                                }
                            }
                        }
                        if run.len() > 1 {
                            b.surface_line(&run, &drop2);
                        }
                    }
                }
            }
            OpKind::Drill => {}
        }
        b.retract();
        let start = pos;
        pos = b.pos;
        let mut moves = std::mem::take(&mut b.moves);
        if matches!(op.kind, OpKind::Rough | OpKind::Contour) {
            moves = fit_arcs(moves, start, b.f, &part_h, r);
        }
        if b.skipped > 0 {
            program.notes.push(format!("{}: {} Bahnen ausgelassen, weil sie das Teil berührt hätten (Material bleibt stehen).", op.kind.name(), b.skipped));
        }
        drop(b);
        if moves.len() > 1 {
            program.ops.push(OpPath { name: format!("{} – {}", op.kind.name(), tool.label()), kind: op.kind, tool: tool.clone(), rpm: c.rpm, feed: c.feed, coolant: m.coolant && mc.coolant, moves });
        }
    }
    Ok(program)
}

/// Inserts points between `a` and `b` (same y) where the straight move would come closer than
/// the surface allows.
fn refine(drop_at: &dyn Fn(f64) -> Option<f64>, a: DVec3, b: DVec3, depth: u32, out: &mut Vec<DVec3>) {
    if depth >= 14 || (b.x - a.x).abs() < 1e-3 {
        return;
    }
    // several points between: the surface may bulge off-centre
    let mut worst: Option<(f64, f64, f64)> = None;
    for k in 1..8 {
        let t = k as f64 / 8.0;
        let mx = a.x + (b.x - a.x) * t;
        let Some(mz) = drop_at(mx) else { continue };
        let over = mz - (a.z + (b.z - a.z) * t);
        if over > 0.002 && worst.is_none_or(|w| over > w.0) {
            worst = Some((over, mx, mz));
        }
    }
    if let Some((_, mx, mz)) = worst {
        let m = DVec3::new(mx, a.y, mz);
        refine(drop_at, a, m, depth + 1, out);
        out.push(m);
        refine(drop_at, m, b, depth + 1, out);
    }
}

/// Clears an area at height z with rings: regions open to the outside from the outside in,
/// closed pockets from their middle out (constant engagement, no full-width slotting).
fn clear_area(b: &mut Builder, allowed: &Shapes, domain: &Shapes, z: f64, ae: f64) {
    let dbox = poly::bbox(domain);
    for shp in allowed {
        let one: Shapes = vec![shp.clone()];
        let grown = poly::offset(&one, 1e-3);
        let open = match (poly::bbox(&one), dbox) {
            (Some((lo, hi)), Some((dlo, dhi))) => (lo[0] - dlo[0]).abs() < 1e-3 && (lo[1] - dlo[1]).abs() < 1e-3 && (hi[0] - dhi[0]).abs() < 1e-3 && (hi[1] - dhi[1]).abs() < 1e-3,
            _ => false,
        };
        let mut rings: Vec<Vec<Contour>> = vec![];
        let mut cur = one.clone();
        let mut guard = 0;
        while !cur.is_empty() && guard < 5000 {
            let mut level = vec![];
            for s in &cur {
                for c in s {
                    if c.len() >= 3 && poly::contour_len(c) > 0.05 {
                        level.push(poly::simplify_contour(c, 0.002));
                    }
                }
            }
            rings.push(level);
            cur = poly::offset(&cur, -ae);
            guard += 1;
        }
        if !open {
            rings.reverse();
        }
        for level in rings {
            let mut left: Vec<Contour> = level;
            while !left.is_empty() {
                let p = (b.pos.x, b.pos.y);
                let k = (0..left.len())
                    .min_by(|&i, &j| {
                        let di = left[i].iter().map(|q| (q[0] - p.0).hypot(q[1] - p.1)).fold(f64::INFINITY, f64::min);
                        let dj = left[j].iter().map(|q| (q[0] - p.0).hypot(q[1] - p.1)).fold(f64::INFINITY, f64::min);
                        di.partial_cmp(&dj).unwrap()
                    })
                    .unwrap();
                let c = left.swap_remove(k);
                // climb milling: material on the right of the cutter
                let c: Contour = if open { c.iter().rev().cloned().collect() } else { c };
                let c = start_near(&c, p);
                // short link at depth when it stays in the allowed area, else over the top
                let at_depth = (b.pos.z - z).abs() < 1e-6;
                let link = poly::dist([b.pos.x, b.pos.y], c[0]);
                if poly::contour_area(&c).abs() < 0.01 || b.touches_part(&c, z) {
                    b.skipped += usize::from(poly::contour_area(&c).abs() >= 0.01);
                    continue;
                }
                let link_ok = at_depth && link <= ae * 2.0 + 1e-6 && poly::segment_inside(&grown, [b.pos.x, b.pos.y], c[0]) && {
                    let from = b.pos;
                    b.f.probe(b.t, from, DVec3::new(c[0][0], c[0][1], z), b.part).gouge >= -0.002
                };
                if link_ok {
                    let target = DVec3::new(c[0][0], c[0][1], z);
                    let f = b.feed;
                    b.cut(target, f);
                    let n = c.len();
                    for kk in 1..=n {
                        let q = c[kk % n];
                        b.cut(DVec3::new(q[0], q[1], z), f);
                    }
                } else {
                    b.ring(&c, z);
                }
            }
        }
    }
}

/// Bores a round hole with the end mill: helical descent (G2/G3 with Z) at a small radius
/// so the centre is cut too, widened in rings of `ae` to the final size, then one full circle
/// at the bottom. Through holes end 0.3 mm below the part, never in the table.
fn bore(b: &mut Builder, job: &Job, h: &(f64, f64, f64, f64, f64), ae: f64, ap: f64, feed: f64, clearance: f64) {
    let (x, y, d, top, bottom) = *h;
    let rt = b.t.radius();
    let rf = d / 2.0 - rt;
    if rf <= 0.02 {
        return;
    }
    let through = bottom <= job.part_lo.z + 1e-3;
    let depth = (if through { bottom - 0.3 } else { bottom }).max(job.stock_lo.z + 0.2);
    let pitch = ap.min(b.t.diameter * 0.3).max(0.05);
    let mut radii = vec![rf.min(rt * 0.5).max(0.02)];
    while *radii.last().unwrap() < rf - 1e-6 {
        let next = (radii.last().unwrap() + ae).min(rf);
        radii.push(next);
    }
    let c = [x, y];
    for (k, &rr) in radii.iter().enumerate() {
        // approach above the hole: through the cleared bore after the first pass
        let entry = if k == 0 {
            let z0 = b.go_to(x + rr, y, top + clearance);
            z0.max(top + clearance.min(1.0))
        } else {
            b.cut(DVec3::new(x, y, b.pos.z), feed);
            let up = DVec3::new(x, y, top + clearance.min(1.0));
            b.rapid(up);
            b.cut(DVec3::new(x + rr, y, up.z), feed);
            up.z
        };
        let mut z = entry;
        // helix: half turns, each down by half the pitch
        let mut side = 1.0;
        while z > depth + 1e-9 {
            let nz = (z - pitch / 2.0).max(depth);
            side = -side;
            b.arc(DVec3::new(x + rr * side, y, nz), c, 1.0, feed);
            z = nz;
        }
        // a full flat circle at the bottom
        b.arc(DVec3::new(x - rr * side, y, depth), c, 1.0, feed);
        b.arc(DVec3::new(x + rr * side, y, depth), c, 1.0, feed);
    }
    // off the wall to the centre, straight up
    b.cut(DVec3::new(x, y, depth), feed);
    let up = DVec3::new(x, y, top + clearance);
    b.rapid(up);
}

/// Vertical round holes found in the part: (centre x, y, diameter, top z, bottom z).
pub fn find_holes(job: &Job) -> Vec<(f64, f64, f64, f64, f64)> {
    let lo = job.part_lo.z;
    let hi = job.part_hi.z;
    let n = (((hi - lo) / 0.25).ceil() as usize).clamp(4, 400);
    let planes: Vec<f64> = (0..n).map(|k| lo + (hi - lo) * (k as f64 + 0.5) / n as f64).collect();
    let slices = crate::slicer::slice::slice_mesh(&job.part, &planes, 0.002);
    // circles per level
    let mut found: Vec<(f64, f64, f64, f64, f64)> = vec![];
    for (li, sl) in slices.iter().enumerate() {
        for sh in sl {
            for hole in &sh[1..] {
                let m = hole.len() as f64;
                if m < 8.0 {
                    continue;
                }
                let cx = hole.iter().map(|p| p[0]).sum::<f64>() / m;
                let cy = hole.iter().map(|p| p[1]).sum::<f64>() / m;
                let rs: Vec<f64> = hole.iter().map(|p| (p[0] - cx).hypot(p[1] - cy)).collect();
                let r = rs.iter().sum::<f64>() / m;
                if rs.iter().any(|x| (x - r).abs() > r * 0.03 + 0.01) {
                    continue;
                }
                let z = planes[li];
                match found.iter_mut().find(|h| (h.0 - cx).hypot(h.1 - cy) < 0.05 && (h.2 - 2.0 * r).abs() < 0.05) {
                    Some(h) => {
                        h.3 = h.3.max(z);
                        h.4 = h.4.min(z);
                    }
                    None => found.push((cx, cy, 2.0 * r, z, z)),
                }
            }
        }
    }
    let step = (hi - lo) / n as f64;
    found
        .into_iter()
        .map(|(x, y, d, t, bt)| (x, y, d, (t + step / 2.0).min(hi), (bt - step / 2.0).max(lo)))
        .collect()
}

fn drill(job: &Job, s: &Setup, f: &mut Field, part_h: &[f32], pos: &mut DVec3, safe: f64) -> Vec<OpPath> {
    let m = s.material();
    let holes = find_holes(job);
    let drills: Vec<&Tool> = s.tools.iter().filter(|t| t.kind == ToolKind::Drill).collect();
    let mut out = vec![];
    for t in drills {
        // a drill never larger than the hole (that would cut the part), at most 0.1 mm smaller
        let mine: Vec<_> = holes.iter().filter(|h| h.2 - t.diameter >= -1e-3 && h.2 - t.diameter <= 0.1).cloned().collect();
        if mine.is_empty() {
            continue;
        }
        let (mut c, _) = cutting(t, m, &s.machine);
        // drills: about half the end mill feed per edge
        c.feed = (c.feed * 0.5).max(10.0);
        let tip = t.radius() / (t.angle.to_radians() / 2.0).tan();
        let mut b = Builder { f: &mut *f, t, pos: *pos, moves: vec![], clearance: s.clearance, safe, feed: c.feed, plunge: c.feed, ramp: 90.0, part: part_h, skipped: 0 };
        b.moves.push(CMove::line(DVec3::new(pos.x, pos.y, safe), true, 0.0));
        b.pos = DVec3::new(pos.x, pos.y, safe);
        for (x, y, _, top, bottom) in mine {
            // through holes: let the full diameter come out, but never into the table
            let through = bottom <= job.part_lo.z + 1e-3;
            let mut depth = if through { bottom - tip - 0.3 } else { bottom };
            depth = depth.max(job.stock_lo.z + 0.2);
            let peck = t.diameter.max(1.0);
            b.go_to(x, y, top);
            let mut z = b.f.max_exact(x, y, t.radius() + b.f.cell).min(b.pos.z);
            let feed = c.feed;
            while z > depth + 1e-6 {
                let next = (z - peck).max(depth);
                b.cut(DVec3::new(x, y, next), feed);
                // chip break: back up above the hole, then return just above the bottom
                b.rapid(DVec3::new(x, y, top + s.clearance));
                if next > depth + 1e-6 {
                    b.rapid(DVec3::new(x, y, next + 0.3));
                }
                z = next;
            }
            b.retract();
        }
        *pos = b.pos;
        let moves = std::mem::take(&mut b.moves);
        out.push(OpPath { name: format!("Bohren – {}", t.label()), kind: OpKind::Drill, tool: t.clone(), rpm: c.rpm, feed: c.feed, coolant: m.coolant && s.machine.coolant, moves });
    }
    out
}
