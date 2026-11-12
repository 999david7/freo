//! Generators producing closed polygon solids (extrude, revolve, sweep, blend, hole,
//! round/chamfer edge tools) and the planar-patch offset used by shell and draft.

use super::csg::{self, *};
use super::math::*;
use super::topology::*;
use super::triangulate::triangulate;
use crate::sketch::sketch::{ProfileLoop, ProfileRegion};
use std::collections::{HashMap, HashSet};

/// Allocates surface tags and records the owning feature.
#[derive(Default, Clone)]
pub struct TagAllocator {
    pub counter: u32,
    pub owner: HashMap<u32, u64>,
}

impl TagAllocator {
    pub fn next(&mut self, feature: u64) -> u32 {
        self.counter += 1;
        self.owner.insert(self.counter, feature);
        self.counter
    }
}

pub fn offset_loop(pts: &[V2], d: f64) -> Vec<V2> {
    if d == 0.0 { return pts.to_vec(); }
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (p0, p1, p2) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let n0 = safe_norm2(perp_right(p1 - p0), V2::X);
            let n1 = safe_norm2(perp_right(p2 - p1), V2::X);
            let bis = safe_norm2(n0 + n1, n1);
            p1 + bis * (d / bis.dot(n1).max(0.2))
        })
        .collect()
}

pub fn simple_loop(pts: Vec<V2>) -> ProfileLoop {
    let n = pts.len();
    ProfileLoop { pts, seg_ent: vec![0; n], seg_rad: vec![None; n] }
}

/// Extrude from z0 (sketch side, original profile) to z1 (far side, tapered profile).
pub fn extrude(regions: &[ProfileRegion], frame: &Frame, z0: f64, z1: f64, taper: f64, tags: &mut TagAllocator, owner: u64) -> Vec<CPolygon> {
    let mut polys = Vec::new();
    let (lo, hi) = (z0.min(z1), z0.max(z1));
    let grow = (z1 - z0).abs() * taper.to_radians().tan();
    let cap_lo = tags.next(owner);
    let cap_hi = tags.next(owner);
    for region in regions {
        let loops: Vec<&ProfileLoop> = std::iter::once(&region.outer).chain(region.holes.iter()).collect();
        let orig: Vec<Vec<V2>> = loops.iter().map(|l| l.pts.clone()).collect();
        let far: Vec<Vec<V2>> = loops.iter().map(|l| offset_loop(&l.pts, grow)).collect();
        let (lo_l, hi_l) = if z0 <= z1 { (&orig, &far) } else { (&far, &orig) };
        let tri = |ls: &Vec<Vec<V2>>| (triangulate(&ls[0], &ls[1..]), ls.concat());
        let (t_lo, f_lo) = tri(lo_l);
        let (t_hi, f_hi) = if grow == 0.0 { (t_lo.clone(), f_lo.clone()) } else { tri(hi_l) };
        for [a, b, c] in t_lo {
            if let Some(p) = CPolygon::flat(&[frame.to_world_h(f_lo[a], lo), frame.to_world_h(f_lo[c], lo), frame.to_world_h(f_lo[b], lo)], cap_lo) { polys.push(p); }
        }
        for [a, b, c] in t_hi {
            if let Some(p) = CPolygon::flat(&[frame.to_world_h(f_hi[a], hi), frame.to_world_h(f_hi[b], hi), frame.to_world_h(f_hi[c], hi)], cap_hi) { polys.push(p); }
        }
        for (li, lp) in loops.iter().enumerate() {
            let n = lp.pts.len();
            let mut ent_tag: HashMap<i64, u32> = HashMap::new();
            for i in 0..n {
                let curve = lp.seg_rad[i].is_some();
                let tag = if curve { *ent_tag.entry(lp.seg_ent[i]).or_insert_with(|| tags.next(owner)) } else { tags.next(owner) };
                let (a, b) = (lo_l[li][i], lo_l[li][(i + 1) % n]);
                let (ta, tb) = (hi_l[li][i], hi_l[li][(i + 1) % n]);
                let (na, nb) = lp.normals(i);
                let (na, nb) = (frame.dir_to_world(na), frame.dir_to_world(nb));
                polys.extend(csg::quad(
                    CVertex { p: frame.to_world_h(a, lo), n: na },
                    CVertex { p: frame.to_world_h(b, lo), n: nb },
                    CVertex { p: frame.to_world_h(tb, hi), n: nb },
                    CVertex { p: frame.to_world_h(ta, hi), n: na },
                    tag,
                ));
            }
        }
    }
    orient_outward(polys)
}

pub fn revolve(regions: &[ProfileRegion], frame: &Frame, axis_p: V2, axis_d: V2, a0: f64, a1: f64, tags: &mut TagAllocator, owner: u64) -> Vec<CPolygon> {
    let a = frame.to_world(axis_p);
    let u = safe_norm(frame.dir_to_world(axis_d), V3::Y);
    let sweep = a1 - a0;
    let full = (sweep.abs() - std::f64::consts::TAU).abs() < 1e-9;
    let steps = ((sweep.abs() / std::f64::consts::TAU * 64.0).ceil() as usize).max(4);
    let rot = |p: V3, t: f64| rotate_point(p, a, u, t);
    let rotv = |v: V3, t: f64| rotate_vec(v, u, t);
    let mut polys = Vec::new();
    for region in regions {
        let loops: Vec<&ProfileLoop> = std::iter::once(&region.outer).chain(region.holes.iter()).collect();
        for lp in &loops {
            let n = lp.pts.len();
            let mut ent_tag: HashMap<i64, u32> = HashMap::new();
            for i in 0..n {
                let tag = *ent_tag.entry(lp.seg_ent[i]).or_insert_with(|| tags.next(owner));
                let (pa, pb) = (frame.to_world(lp.pts[i]), frame.to_world(lp.pts[(i + 1) % n]));
                let (na2, nb2) = lp.normals(i);
                let (na, nb) = (frame.dir_to_world(na2), frame.dir_to_world(nb2));
                for k in 0..steps {
                    let t0 = a0 + sweep * k as f64 / steps as f64;
                    let t1 = a0 + sweep * (k + 1) as f64 / steps as f64;
                    let v = [
                        CVertex { p: rot(pa, t0), n: rotv(na, t0) },
                        CVertex { p: rot(pb, t0), n: rotv(nb, t0) },
                        CVertex { p: rot(pb, t1), n: rotv(nb, t1) },
                        CVertex { p: rot(pa, t1), n: rotv(na, t1) },
                    ];
                    let d = dedupe(v.to_vec());
                    if d.len() == 4 { polys.extend(csg::quad(v[0], v[1], v[2], v[3], tag)); }
                    else if d.len() == 3 { if let Some(p) = CPolygon::new(d, tag) { polys.push(p); } }
                }
            }
        }
        if !full {
            let holes: Vec<Vec<V2>> = region.holes.iter().map(|h| h.pts.clone()).collect();
            let tri = triangulate(&region.outer.pts, &holes);
            let flat: Vec<V2> = loops.iter().flat_map(|l| l.pts.iter().copied()).collect();
            let (c0, c1) = (tags.next(owner), tags.next(owner));
            for [x, y, z] in tri {
                let p = [frame.to_world(flat[x]), frame.to_world(flat[y]), frame.to_world(flat[z])];
                if let Some(q) = CPolygon::flat(&[rot(p[0], a0), rot(p[2], a0), rot(p[1], a0)], c0) { polys.push(q); }
                if let Some(q) = CPolygon::flat(&[rot(p[0], a1), rot(p[1], a1), rot(p[2], a1)], c1) { polys.push(q); }
            }
        }
    }
    if let Some(first) = regions.first().and_then(|r| r.outer.pts.first()) {
        let p = frame.to_world(*first);
        let s = u.cross(p - a).dot(frame.z()) * if sweep >= 0.0 { 1.0 } else { -1.0 };
        if s < 0.0 { polys = polys.into_iter().map(|p| p.flipped()).collect(); }
    }
    orient_outward(polys)
}

/// Rotation-minimising frames per path segment with mitered rings at vertices.
pub fn sweep_rings(path: &[V3], closed: bool, initial: &Frame) -> Vec<Box<dyn Fn(V2) -> V3>> {
    let n = path.len();
    let s = if closed { n } else { n - 1 };
    let dseg: Vec<V3> = (0..s).map(|j| safe_norm(path[(j + 1) % n] - path[j], V3::Z)).collect();
    let mut xseg = Vec::with_capacity(s);
    let mut x = safe_norm(initial.x - dseg[0] * initial.x.dot(dseg[0]), V3::X);
    xseg.push(x);
    for j in 1..s {
        let (a, b) = (dseg[j - 1], dseg[j]);
        let ax = a.cross(b);
        let sl = ax.length();
        if sl > 1e-12 { x = rotate_vec(x, ax / sl, sl.atan2(a.dot(b))); }
        x = safe_norm(x - b * x.dot(b), x);
        xseg.push(x);
    }
    let mut rings: Vec<Box<dyn Fn(V2) -> V3>> = Vec::new();
    for k in 0..n {
        let seg_in = if closed { Some((k + s - 1) % s) } else if k > 0 { Some(k - 1) } else { None };
        let seg_out = if closed { Some(k % s) } else if k < s { Some(k) } else { None };
        let base = seg_in.or(seg_out).unwrap();
        let (d, xx) = (dseg[base], xseg[base]);
        let yy = d.cross(xx);
        let origin = path[k];
        let miter = match (seg_in, seg_out) {
            (Some(i), Some(o)) => Some(safe_norm(dseg[i] + dseg[o], dseg[o])),
            _ => None,
        };
        rings.push(Box::new(move |q: V2| {
            let p = origin + xx * q.x + yy * q.y;
            match miter {
                Some(m) => {
                    let den = d.dot(m);
                    if den.abs() < 1e-6 { p } else { p + d * (-(p - origin).dot(m) / den) }
                }
                None => p,
            }
        }));
    }
    rings
}

pub fn sweep(regions: &[ProfileRegion], path: &[V3], closed: bool, section_frame: &Frame, tags: &mut TagAllocator, owner: u64) -> Vec<CPolygon> {
    if path.len() < 2 { return vec![]; }
    let rings = sweep_rings(path, closed, section_frame);
    loft(regions, &rings, closed, tags, owner)
}

/// Solid through a sequence of section placements: `rings[k]` maps a section point to its
/// position at station k. Open lofts are capped at both ends.
pub fn loft(regions: &[ProfileRegion], rings: &[Box<dyn Fn(V2) -> V3>], closed: bool, tags: &mut TagAllocator, owner: u64) -> Vec<CPolygon> {
    let n = rings.len();
    if n < 2 { return vec![]; }
    let mut polys = Vec::new();
    for region in regions {
        let loops: Vec<&ProfileLoop> = std::iter::once(&region.outer).chain(region.holes.iter()).collect();
        for lp in &loops {
            let m = lp.pts.len();
            let tag = tags.next(owner);
            let vert = |k: usize, i: usize| -> CVertex {
                let (na, _) = lp.normals(i);
                let p = rings[k](lp.pts[i]);
                CVertex { p, n: safe_norm(rings[k](lp.pts[i] + na) - p, V3::Z) }
            };
            let ring_count = if closed { n } else { n - 1 };
            for k in 0..ring_count {
                let k2 = (k + 1) % n;
                for i in 0..m {
                    let i2 = (i + 1) % m;
                    polys.extend(csg::quad(vert(k, i), vert(k, i2), vert(k2, i2), vert(k2, i), tag));
                }
            }
        }
        if !closed {
            let holes: Vec<Vec<V2>> = region.holes.iter().map(|h| h.pts.clone()).collect();
            let tri = triangulate(&region.outer.pts, &holes);
            let flat: Vec<V2> = loops.iter().flat_map(|l| l.pts.iter().copied()).collect();
            let (c0, c1) = (tags.next(owner), tags.next(owner));
            let (r0, r1) = (&rings[0], &rings[n - 1]);
            for [a, b, c] in tri {
                if let Some(q) = CPolygon::flat(&[r0(flat[a]), r0(flat[c]), r0(flat[b])], c0) { polys.push(q); }
                if let Some(q) = CPolygon::flat(&[r1(flat[a]), r1(flat[b]), r1(flat[c])], c1) { polys.push(q); }
            }
        }
    }
    orient_outward(polys)
}

/// Blend sections resampled to a common point count, each starting at the point at the
/// smallest polar angle about its centroid so corresponding points line up.
fn blend_sections(sections: &[Vec<V2>]) -> Vec<Vec<V2>> {
    let count = sections.iter().map(|s| s.len()).max().unwrap_or(48).max(48);
    sections.iter().map(|s| {
        let mut pts = s.clone();
        if polygon_area2(&pts) < 0.0 { pts.reverse(); }
        let mut res = simple_loop(pts).resampled(count);
        let c = res.iter().fold(V2::ZERO, |a, b| a + *b) / res.len() as f64;
        let mut best = 0;
        let mut best_a = f64::INFINITY;
        for (i, p) in res.iter().enumerate() {
            let mut a = (p.y - c.y).atan2(p.x - c.x);
            if a < 0.0 { a += std::f64::consts::TAU; }
            if a < best_a { best_a = a; best = i; }
        }
        res.rotate_left(best);
        res
    }).collect()
}

pub fn blend(sections: &[Vec<V2>], frames: &[Frame], tags: &mut TagAllocator, owner: u64) -> Vec<CPolygon> {
    if sections.len() < 2 { return vec![]; }
    let flat = blend_sections(sections);
    let rings: Vec<Vec<V3>> = flat.iter().zip(frames).map(|(s, f)| s.iter().map(|p| f.to_world(*p)).collect()).collect();
    ruled(&rings, &flat, tags, owner)
}

/// Rotational blend: section k lies in `frames[k]`, the sketch plane turned by `angles[k]`
/// (radians) about the axis `axis_p` + t `axis_d`. Sections are interpolated every few
/// degrees so the walls follow the rotation instead of cutting across it.
pub fn rotational_blend(sections: &[Vec<V2>], base: &Frame, axis_p: V3, axis_d: V3, angles: &[f64], tags: &mut TagAllocator, owner: u64) -> Vec<CPolygon> {
    if sections.len() < 2 || angles.len() != sections.len() { return vec![]; }
    let flat = blend_sections(sections);
    let mut rings: Vec<Vec<V3>> = Vec::new();
    let mut flat_all: Vec<Vec<V2>> = Vec::new();
    for k in 0..flat.len() - 1 {
        let (a0, a1) = (angles[k], angles[k + 1]);
        let steps = (((a1 - a0).abs() / 5f64.to_radians()).ceil() as usize).max(1);
        let last = if k == flat.len() - 2 { steps } else { steps - 1 };
        for j in 0..=last {
            let t = j as f64 / steps as f64;
            let ang = a0 + (a1 - a0) * t;
            let pts: Vec<V2> = flat[k].iter().zip(&flat[k + 1]).map(|(p, q)| *p + (*q - *p) * t).collect();
            let rot = |v: V3| rotate_vec(v, axis_d, ang);
            let f = Frame { origin: axis_p + rot(base.origin - axis_p), x: rot(base.x), y: rot(base.y) };
            rings.push(pts.iter().map(|p| f.to_world(*p)).collect());
            flat_all.push(pts);
        }
    }
    ruled(&rings, &flat_all, tags, owner)
}

/// Ruled walls between consecutive rings of equal length, capped with the first and last
/// sections (`flat` are the same rings in their section planes).
fn ruled(rings: &[Vec<V3>], flat: &[Vec<V2>], tags: &mut TagAllocator, owner: u64) -> Vec<CPolygon> {
    let count = rings[0].len();
    let mut polys = Vec::new();
    let wall = tags.next(owner);
    for k in 0..rings.len() - 1 {
        for i in 0..count {
            let i2 = (i + 1) % count;
            let (a, b, c, d) = (rings[k][i], rings[k][i2], rings[k + 1][i2], rings[k + 1][i]);
            let nrm = safe_norm((b - a).cross(d - a), V3::Z);
            let v = |p: V3| CVertex { p, n: nrm };
            polys.extend(csg::quad(v(a), v(b), v(c), v(d), wall));
        }
    }
    let (c0, c1) = (tags.next(owner), tags.next(owner));
    for [a, b, c] in triangulate(&flat[0], &[]) {
        if let Some(q) = CPolygon::flat(&[rings[0][a], rings[0][c], rings[0][b]], c0) { polys.push(q); }
    }
    let l = rings.len() - 1;
    for [a, b, c] in triangulate(&flat[l], &[]) {
        if let Some(q) = CPolygon::flat(&[rings[l][a], rings[l][b], rings[l][c]], c1) { polys.push(q); }
    }
    orient_outward(polys)
}

/// Revolved hole tool; `prof` is (radius, depth-into-material) pairs around the axis.
pub fn hole_tool(prof: &[V2], center: V3, dir: V3, frame_x: V3, tags: &mut TagAllocator, owner: u64) -> Vec<CPolygon> {
    let d = safe_norm(dir, V3::Z);
    let radial = safe_norm(frame_x - d * frame_x.dot(d), Frame::from_normal(V3::ZERO, d).x);
    let pf = Frame { origin: center, x: radial, y: -d };
    let mut pts: Vec<V2> = prof.iter().map(|p| V2::new(p.x, -p.y)).collect();
    if polygon_area2(&pts) < 0.0 { pts.reverse(); }
    let region = ProfileRegion { outer: simple_loop(pts), holes: vec![] };
    revolve(&[region], &pf, V2::ZERO, V2::Y, 0.0, std::f64::consts::TAU, tags, owner)
}

/// Section builder: (point, nA, nB, fA, fB, fraction along the edge 0..1) -> ring.
pub type Section = Box<dyn Fn(V3, V3, V3, V3, V3, f64) -> Vec<V3>>;

/// Swept cutter/filler along an edge chain built from per-vertex sections (point, nA, nB, fA, fB).
pub fn edge_tool(chain: &EdgeChain, section: &Section, extend: f64, tags: &mut TagAllocator, owner: u64) -> Vec<CPolygon> {
    let mut pts = chain.points.clone();
    let (mut na, mut nb, mut fa, mut fb) = (chain.n_a.clone(), chain.n_b.clone(), chain.f_a.clone(), chain.f_b.clone());
    if !chain.closed && pts.len() >= 2 {
        let l = pts.len();
        let d0 = safe_norm(pts[0] - pts[1], V3::X);
        let d1 = safe_norm(pts[l - 1] - pts[l - 2], V3::X);
        pts[0] += d0 * extend;
        pts[l - 1] += d1 * extend;
    }
    if chain.closed {
        pts.push(pts[0]); na.push(na[0]); nb.push(nb[0]); fa.push(fa[0]); fb.push(fb[0]);
    }
    let mut acc = vec![0.0];
    for i in 1..pts.len() { let l = acc[i - 1] + (pts[i] - pts[i - 1]).length(); acc.push(l); }
    let total = acc.last().copied().unwrap_or(1.0).max(1e-12);
    let rings: Vec<Vec<V3>> = (0..pts.len()).map(|i| section(pts[i], na[i], nb[i], fa[i], fb[i], acc[i] / total)).collect();
    let tag = tags.next(owner);
    let m = rings[0].len();
    let mut polys = Vec::new();
    for k in 0..rings.len() - 1 {
        for i in 0..m {
            let i2 = (i + 1) % m;
            let (a, b, c, d) = (rings[k][i], rings[k][i2], rings[k + 1][i2], rings[k + 1][i]);
            let nrm = safe_norm((b - a).cross(d - a), V3::Z);
            let v = |p: V3| CVertex { p, n: nrm };
            polys.extend(csg::quad(v(a), v(b), v(c), v(d), tag));
        }
    }
    if !chain.closed {
        let last = rings.len() - 1;
        for (ring, rev) in [(&rings[0], true), (&rings[last], false)] {
            for i in 1..m - 1 {
                let tri = if rev { [ring[0], ring[i + 1], ring[i]] } else { [ring[0], ring[i], ring[i + 1]] };
                if let Some(q) = CPolygon::flat(&tri, tag) { polys.push(q); }
            }
        }
    }
    orient_outward(polys)
}

fn slerp_arc(c: V3, u1: V3, u2: V3, r: f64, segments: usize) -> Vec<V3> {
    let ang = u1.dot(u2).clamp(-1.0, 1.0).acos();
    (1..segments)
        .map(|s| {
            let t = s as f64 / segments as f64;
            let v = (((1.0 - t) * ang).sin() * u1 + (t * ang).sin() * u2) / ang.sin().max(1e-9);
            c + v * r
        })
        .collect()
}

/// Round cutter/filler section; the radius varies linearly from `r0` to `r1` along the edge.
pub fn round_section(r0: f64, r1: f64, convex: bool) -> Section {
    Box::new(move |p, n1, n2, _, _, frac| {
        let r = r0 + (r1 - r0) * frac;
        let k = n1.dot(n2).clamp(-0.999, 0.999);
        let s = if convex { -1.0 } else { 1.0 };
        let c = p + (n1 + n2) * (s * r / (1.0 + k));
        let (t1, t2) = (c - s * n1 * r, c - s * n2 * r);
        let corner = p - s * safe_norm(n1 + n2, n1) * r * 0.05;
        let mut ring = vec![corner, t1];
        ring.extend(slerp_arc(c, -s * n1, -s * n2, r, 10));
        ring.push(t2);
        ring
    })
}

/// Round section with a curvature-continuous corner shape instead of the arc.
pub fn round_section_shape(r0: f64, r1: f64, convex: bool, shape: super::corner::CornerShape) -> Section {
    Box::new(move |p, n1, n2, _, _, frac| {
        let r = r0 + (r1 - r0) * frac;
        let k = n1.dot(n2).clamp(-0.999, 0.999);
        let s = if convex { -1.0 } else { 1.0 };
        let c = p + (n1 + n2) * (s * r / (1.0 + k));
        // from the edge to the tangent points of the circular round
        let (v1, v2) = (c - s * n1 * r - p, c - s * n2 * r - p);
        let half = safe_norm(v1, n1).dot(safe_norm(v2, n2)).clamp(-1.0, 1.0).acos() / 2.0;
        let corner = p - s * safe_norm(n1 + n2, n1) * r * 0.05;
        let mut ring = vec![corner];
        ring.extend(shape.points(half, 24).into_iter().map(|(a, b)| p + v1 * a + v2 * b));
        ring
    })
}

pub fn chamfer_section(d1: f64, d2: f64, convex: bool) -> Section {
    Box::new(move |p, n1, n2, f1, f2, _| {
        let s = if convex { 1.0 } else { -1.0 };
        let corner = p + s * safe_norm(n1 + n2, n1) * d1.max(d2) * 0.05;
        vec![corner, p + f1 * d1, p + f2 * d2]
    })
}

/// Moves each planar patch onto a target plane and rebuilds it from its boundary loops.
/// `target(face, normal, offset)` returns the patch's new (normal, offset).
pub fn move_planes(topo: &Topology, target: &dyn Fn(usize, V3, f64) -> (V3, f64)) -> Vec<CPolygon> {
    let polys = &topo.polys;
    let pos = &topo.positions;
    let mut edge_polys: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (pi, p) in polys.iter().enumerate() {
        for i in 0..p.len() {
            let (u, v) = (p[i], p[(i + 1) % p.len()]);
            edge_polys.entry((u.min(v), u.max(v))).or_default().push(pi);
        }
    }
    let mut parent: Vec<usize> = (0..polys.len()).collect();
    fn find(p: &mut Vec<usize>, mut x: usize) -> usize { while p[x] != x { p[x] = p[p[x]]; x = p[x]; } x }
    for ps in edge_polys.values() {
        if ps.len() != 2 { continue; }
        let (a, b) = (topo.planes[ps[0]], topo.planes[ps[1]]);
        if a.n.dot(b.n) > 1.0 - 1e-9 && (a.w - b.w).abs() < 1e-6 {
            let (ra, rb) = (find(&mut parent, ps[0]), find(&mut parent, ps[1]));
            if ra != rb { parent[ra] = rb; }
        }
    }
    let mut patches: HashMap<usize, Vec<usize>> = HashMap::new();
    for pi in 0..polys.len() { let r = find(&mut parent, pi); patches.entry(r).or_default().push(pi); }

    let mut rows_at: Vec<Vec<(V3, f64)>> = vec![Vec::new(); pos.len()];
    let mut patch_target: HashMap<usize, (V3, f64)> = HashMap::new();
    for (&root, members) in &patches {
        let pl = topo.planes[members[0]];
        let tgt = target(topo.poly_face[members[0]], pl.n, pl.w);
        patch_target.insert(root, tgt);
        let verts: HashSet<usize> = members.iter().flat_map(|&pi| polys[pi].iter().copied()).collect();
        for v in verts {
            if !rows_at[v].iter().any(|r| r.0.dot(pl.n) > 0.99999) { rows_at[v].push(tgt); }
        }
    }
    let mut new_pos = pos.clone();
    for vi in 0..pos.len() {
        let rows = &rows_at[vi];
        let k = rows.len();
        if k == 0 { continue; }
        let p = pos[vi];
        let mut m = vec![0.0; k * k];
        let mut rhs = vec![0.0; k];
        for i in 0..k {
            rhs[i] = rows[i].1 - rows[i].0.dot(p);
            for j in 0..k { m[i * k + j] = rows[i].0.dot(rows[j].0) + if i == j { 1e-9 } else { 0.0 }; }
        }
        if let Some(lam) = solve_dense(&m, &rhs, k) {
            let d = (0..k).fold(V3::ZERO, |s, i| s + rows[i].0 * lam[i]);
            if d.is_finite() { new_pos[vi] = p + d; }
        }
    }
    let mut out = Vec::new();
    for (root, members) in &patches {
        let member_set: HashSet<usize> = members.iter().copied().collect();
        let mut vnorm: HashMap<usize, V3> = HashMap::new();
        let mut next: HashMap<usize, Vec<usize>> = HashMap::new();
        for &pi in members {
            let p = &polys[pi];
            for i in 0..p.len() {
                vnorm.insert(p[i], topo.vnormals[pi][i]);
                let (u, v) = (p[i], p[(i + 1) % p.len()]);
                let others = edge_polys.get(&(u.min(v), u.max(v))).cloned().unwrap_or_default();
                if others.len() < 2 || others.iter().any(|o| !member_set.contains(o)) {
                    next.entry(u).or_default().push(v);
                }
            }
        }
        let mut loops: Vec<Vec<usize>> = Vec::new();
        let mut guard = 0;
        loop {
            let Some(start) = next.iter().find(|(_, v)| !v.is_empty()).map(|(k, _)| *k) else { break };
            let mut lp = vec![start];
            let mut cur = start;
            while guard < 200000 {
                guard += 1;
                let Some(list) = next.get_mut(&cur) else { break };
                let Some(nx) = list.pop() else { break };
                if nx == start { break; }
                lp.push(nx);
                cur = nx;
            }
            if lp.len() >= 3 { loops.push(lp); }
            if guard >= 200000 { break; }
        }
        let tgt = patch_target[root];
        let fr = Frame::from_normal(new_pos[loops.first().map(|l| l[0]).unwrap_or(0)], tgt.0);
        let (mut outers, mut holes): (Vec<Vec<usize>>, Vec<Vec<usize>>) = (vec![], vec![]);
        for l in loops {
            let n = l.len();
            let simp: Vec<usize> = (0..n)
                .filter(|&i| {
                    let (a, b, c) = (pos[l[(i + n - 1) % n]], pos[l[i]], pos[l[(i + 1) % n]]);
                    safe_norm(b - a, V3::X).cross(safe_norm(c - b, V3::X)).length() > 1e-9
                })
                .map(|i| l[i])
                .collect();
            if simp.len() < 3 { continue; }
            let pts2: Vec<V2> = simp.iter().map(|&v| fr.to_local(new_pos[v])).collect();
            if polygon_area2(&pts2) > 0.0 { outers.push(simp) } else { holes.push(simp) }
        }
        let tag = topo.tags[members[0]];
        let single = outers.len() == 1;
        for o in &outers {
            let o2: Vec<V2> = o.iter().map(|&v| fr.to_local(new_pos[v])).collect();
            let hs: Vec<&Vec<usize>> = holes.iter().filter(|h| single || point_in_polygon2(fr.to_local(new_pos[h[0]]), &o2)).collect();
            let all: Vec<usize> = o.iter().copied().chain(hs.iter().flat_map(|h| h.iter().copied())).collect();
            let h2: Vec<Vec<V2>> = hs.iter().map(|h| h.iter().map(|&v| fr.to_local(new_pos[v])).collect()).collect();
            let pl = CPlane { n: tgt.0, w: tgt.1 };
            for [a, b, c] in triangulate(&o2, &h2) {
                let v: Vec<CVertex> = [all[a], all[b], all[c]].iter().map(|&i| CVertex { p: new_pos[i], n: *vnorm.get(&i).unwrap_or(&tgt.0) }).collect();
                out.push(CPolygon { v, plane: pl, tag });
            }
        }
    }
    out
}
