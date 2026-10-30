//! Quilt (Sammelfläche) geometry: open polygon sheets made, offset, thickened, extended, trimmed,
//! sectioned and blended for the surface features.

use crate::kernel::csg::{CPolygon, CVertex};
use crate::kernel::math::*;
use std::collections::HashMap;

type Key = (i64, i64, i64);
fn key(p: V3) -> Key { ((p.x * 1e5).round() as i64, (p.y * 1e5).round() as i64, (p.z * 1e5).round() as i64) }

/// A polygon from points, its normal from the winding (None when degenerate).
pub fn poly(pts: &[V3], tag: u32) -> Option<CPolygon> { CPolygon::flat(pts, tag) }

/// Fan triangles of a point loop: always planar, which the BSP-based CSG needs (a warped quad
/// makes it split without end).
pub fn tris(pts: &[V3], tag: u32, facing: V3) -> Vec<CPolygon> {
    (1..pts.len().saturating_sub(1)).filter_map(|k| {
        let mut p = poly(&[pts[0], pts[k], pts[k + 1]], tag)?;
        if p.plane.n.dot(facing) < 0.0 { p.flip(); }
        Some(p)
    }).collect()
}

/// Planar faces filling closed 2D regions (outer loop + holes) on `frame`.
pub fn fill(regions: &[(Vec<V2>, Vec<Vec<V2>>)], frame: &Frame, tag: u32) -> Vec<CPolygon> {
    let mut out = vec![];
    for (outer, holes) in regions {
        let all: Vec<V2> = outer.iter().chain(holes.iter().flatten()).copied().collect();
        for t in crate::kernel::triangulate::triangulate(outer, holes) {
            let pts = [frame.to_world(all[t[0]]), frame.to_world(all[t[1]]), frame.to_world(all[t[2]])];
            if let Some(mut p) = poly(&pts, tag) { if p.plane.n.dot(frame.z()) < 0.0 { p.flip(); } out.push(p); }
        }
    }
    out
}

/// Edges used by exactly one polygon, with the polygon's index (direction as wound in that polygon).
pub fn boundary(polys: &[CPolygon]) -> Vec<(V3, V3, usize)> {
    let mut count: HashMap<(Key, Key), i32> = HashMap::new();
    for p in polys {
        let n = p.v.len();
        for i in 0..n {
            let (a, b) = (key(p.v[i].p), key(p.v[(i + 1) % n].p));
            *count.entry(if a < b { (a, b) } else { (b, a) }).or_default() += 1;
        }
    }
    let mut out = vec![];
    for (pi, p) in polys.iter().enumerate() {
        let n = p.v.len();
        for i in 0..n {
            let (a, b) = (p.v[i].p, p.v[(i + 1) % n].p);
            let (ka, kb) = (key(a), key(b));
            if count.get(&if ka < kb { (ka, kb) } else { (kb, ka) }) == Some(&1) { out.push((a, b, pi)); }
        }
    }
    out
}

pub fn is_closed(polys: &[CPolygon]) -> bool { !polys.is_empty() && boundary(polys).is_empty() }

/// Moves every vertex along the averaged normal of the polygons sharing it (a continuous offset).
pub fn offset(polys: &[CPolygon], d: f64) -> Vec<CPolygon> {
    let mut acc: HashMap<Key, V3> = HashMap::new();
    for p in polys { for v in &p.v { *acc.entry(key(v.p)).or_insert(V3::ZERO) += p.plane.n; } }
    polys.iter().flat_map(|p| {
        let pts: Vec<V3> = p.v.iter().map(|v| {
            let n = safe_norm(acc[&key(v.p)], p.plane.n);
            // keep the distance along each face normal close to d at creases
            let s = n.dot(p.plane.n).max(0.5);
            v.p + n * (d / s)
        }).collect();
        // moved vertices need not stay coplanar
        tris(&pts, p.tag, p.plane.n)
    }).collect()
}

/// A wall along a boundary edge (two triangles), facing away from `inside`.
fn wall(a: V3, b: V3, c: V3, d: V3, inside: V3, tag: u32) -> Vec<CPolygon> {
    let mid = (a + b + c + d) * 0.25;
    tris(&[a, b, c, d], tag, mid - inside)
}

/// A closed solid from a quilt: the sheet, its copy moved by `t` (offset, or `shift` when given),
/// and side walls along the open edges.
pub fn thicken(polys: &[CPolygon], t: f64, shift: Option<V3>, tag: u32) -> Vec<CPolygon> {
    let polys: Vec<CPolygon> = polys.iter().flat_map(|p| { let pts: Vec<V3> = p.v.iter().map(|v| v.p).collect(); tris(&pts, p.tag, p.plane.n) }).collect();
    // where each vertex moves: by the shift, or along its averaged normal
    let mut acc: HashMap<Key, V3> = HashMap::new();
    for p in &polys { for v in &p.v { *acc.entry(key(v.p)).or_insert(V3::ZERO) += p.plane.n; } }
    let moved = |q: V3, fallback: V3| -> V3 { match shift { Some(s) => q + s, None => q + safe_norm(acc.get(&key(q)).copied().unwrap_or(fallback), fallback) * t } };
    let mut out: Vec<CPolygon> = vec![];
    for p in &polys {
        let pts: Vec<V3> = p.v.iter().map(|v| v.p).collect();
        let top: Vec<V3> = pts.iter().map(|q| moved(*q, p.plane.n)).collect();
        let c0 = pts.iter().fold(V3::ZERO, |a, b| a + *b) / 3.0;
        let c1 = top.iter().fold(V3::ZERO, |a, b| a + *b) / 3.0;
        // each sheet faces away from the other
        out.extend(tris(&pts, p.tag, c0 - c1));
        out.extend(tris(&top, p.tag, c1 - c0));
    }
    for (a, b, pi) in boundary(&polys) {
        let n = polys[pi].plane.n;
        let (a2, b2) = (moved(a, n), moved(b, n));
        let c0 = polys[pi].centroid();
        let inside = (c0 + moved(c0, n)) * 0.5;
        out.extend(wall(a, b, b2, a2, inside, tag));
    }
    out
}

/// Each open edge carried on in the plane of its polygon by `d`.
pub fn extend(polys: &[CPolygon], d: f64, tag: u32) -> Vec<CPolygon> {
    let mut out = polys.to_vec();
    for (a, b, pi) in boundary(polys) {
        let p = &polys[pi];
        let mut o = safe_norm((b - a).cross(p.plane.n), V3::X);
        if o.dot((a + b) * 0.5 - p.centroid()) < 0.0 { o = -o; }
        if let Some(mut q) = poly(&[a, b, b + o * d, a + o * d], tag) { if q.plane.n.dot(p.plane.n) < 0.0 { q.flip(); } out.push(q); }
    }
    out
}

/// The part of each polygon on the positive side of the plane n·x = w (or the negative side).
pub fn clip(polys: &[CPolygon], n: V3, w: f64, keep_positive: bool) -> Vec<CPolygon> {
    let s = if keep_positive { 1.0 } else { -1.0 };
    polys.iter().filter_map(|p| {
        let d: Vec<f64> = p.v.iter().map(|v| (v.p.dot(n) - w) * s).collect();
        if d.iter().all(|x| *x >= -1e-9) { return Some(p.clone()); }
        if d.iter().all(|x| *x <= 1e-9) { return None; }
        let mut out: Vec<CVertex> = vec![];
        let k = p.v.len();
        for i in 0..k {
            let j = (i + 1) % k;
            if d[i] >= 0.0 { out.push(p.v[i].clone()); }
            if (d[i] >= 0.0) != (d[j] >= 0.0) { out.push(p.v[i].lerp(&p.v[j], d[i] / (d[i] - d[j]))); }
        }
        CPolygon::new(out, p.tag)
    }).collect()
}

/// Where the polygons cross the plane n·x = w, as two-point polylines.
pub fn section(polys: &[CPolygon], n: V3, w: f64) -> Vec<Vec<V3>> {
    let mut segs = vec![];
    for p in polys {
        let d: Vec<f64> = p.v.iter().map(|v| v.p.dot(n) - w).collect();
        let k = p.v.len();
        let mut hits = vec![];
        for i in 0..k {
            let j = (i + 1) % k;
            if (d[i] >= 0.0) != (d[j] >= 0.0) { hits.push(p.v[i].p + (p.v[j].p - p.v[i].p) * (d[i] / (d[i] - d[j]))); }
        }
        if hits.len() >= 2 { segs.push(vec![hits[0], hits[1]]); }
    }
    segs
}

/// `n` points evenly spaced along a polyline.
pub fn resample(pts: &[V3], n: usize) -> Vec<V3> {
    if pts.len() < 2 { return vec![pts.first().copied().unwrap_or(V3::ZERO); n]; }
    let lens: Vec<f64> = pts.windows(2).map(|w| (w[1] - w[0]).length()).collect();
    let total: f64 = lens.iter().sum::<f64>().max(1e-12);
    (0..n).map(|i| {
        let mut t = total * i as f64 / (n - 1) as f64;
        for (k, l) in lens.iter().enumerate() {
            if t <= *l || k == lens.len() - 1 { return pts[k] + (pts[k + 1] - pts[k]) * (t / l.max(1e-12)).min(1.0); }
            t -= l;
        }
        *pts.last().unwrap()
    }).collect()
}

fn grid(rows: &[Vec<V3>], tag: u32) -> Vec<CPolygon> {
    let mut out = vec![];
    for r in 0..rows.len() - 1 {
        for c in 0..rows[r].len() - 1 {
            let (a, b, cc, d) = (rows[r][c], rows[r][c + 1], rows[r + 1][c + 1], rows[r + 1][c]);
            if let Some(p) = poly(&[a, b, cc], tag) { out.push(p); }
            if let Some(p) = poly(&[a, cc, d], tag) { out.push(p); }
        }
    }
    out
}

/// Berandungsverbund: a ruled surface between two boundary curves, or a Coons patch from four.
pub fn boundary_blend(curves: &[Vec<V3>], tag: u32) -> Vec<CPolygon> {
    const N: usize = 24;
    match curves.len() {
        2 => {
            let (a, mut b) = (resample(&curves[0], N), resample(&curves[1], N));
            // run both curves the same way
            if (a[0] - b[0]).length() + (a[N - 1] - b[N - 1]).length() > (a[0] - b[N - 1]).length() + (a[N - 1] - b[0]).length() { b.reverse(); }
            let rows: Vec<Vec<V3>> = (0..N).map(|i| { let v = i as f64 / (N - 1) as f64; (0..N).map(|k| a[k] * (1.0 - v) + b[k] * v).collect() }).collect();
            grid(&rows, tag)
        }
        n if n >= 4 => {
            let (c0, c1, d0, d1) = (resample(&curves[0], N), resample(&curves[1], N), resample(&curves[2], N), resample(&curves[3], N));
            let rows: Vec<Vec<V3>> = (0..N).map(|i| {
                let v = i as f64 / (N - 1) as f64;
                (0..N).map(|k| {
                    let u = k as f64 / (N - 1) as f64;
                    let ruled_u = c0[k] * (1.0 - v) + c1[k] * v;
                    let ruled_v = d0[i] * (1.0 - u) + d1[i] * u;
                    let corners = c0[0] * (1.0 - u) * (1.0 - v) + c0[N - 1] * u * (1.0 - v) + c1[0] * (1.0 - u) * v + c1[N - 1] * u * v;
                    ruled_u + ruled_v - corners
                }).collect()
            }).collect();
            grid(&rows, tag)
        }
        _ => vec![],
    }
}

/// Stil: a bicubic Bézier patch over a w × l rectangle on `frame`, control heights row by row.
pub fn style_patch(frame: &Frame, w: f64, l: f64, heights: &[f64], tag: u32) -> Vec<CPolygon> {
    const N: usize = 20;
    let h = |i: usize, j: usize| heights.get(i * 4 + j).copied().unwrap_or(0.0);
    let b = |t: f64| [(1.0 - t).powi(3), 3.0 * t * (1.0 - t).powi(2), 3.0 * t * t * (1.0 - t), t.powi(3)];
    let rows: Vec<Vec<V3>> = (0..N).map(|r| {
        let v = r as f64 / (N - 1) as f64;
        (0..N).map(|c| {
            let u = c as f64 / (N - 1) as f64;
            let (bu, bv) = (b(u), b(v));
            let mut z = 0.0;
            for i in 0..4 { for j in 0..4 { z += bv[i] * bu[j] * h(i, j); } }
            frame.origin + frame.x * ((u - 0.5) * w) + frame.y * ((v - 0.5) * l) + frame.z() * z
        }).collect()
    }).collect();
    grid(&rows, tag)
}

/// Freistil: a box cage (w × l × h on `frame`) smoothed by Catmull–Clark subdivision.
pub fn freestyle(frame: &Frame, size: [f64; 3], level: u32, tag: u32) -> Vec<CPolygon> {
    let (w, l, h) = (size[0] / 2.0, size[1] / 2.0, size[2]);
    let mut pts: Vec<V3> = vec![];
    for z in [0.0, h] { for (x, y) in [(-w, -l), (w, -l), (w, l), (-w, l)] { pts.push(frame.origin + frame.x * x + frame.y * y + frame.z() * z); } }
    let mut faces: Vec<Vec<usize>> = vec![vec![0, 3, 2, 1], vec![4, 5, 6, 7], vec![0, 1, 5, 4], vec![1, 2, 6, 5], vec![2, 3, 7, 6], vec![3, 0, 4, 7]];
    for _ in 0..level.min(4) {
        let fp: Vec<V3> = faces.iter().map(|f| f.iter().map(|&i| pts[i]).fold(V3::ZERO, |a, b| a + b) / f.len() as f64).collect();
        let mut edge_faces: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (fi, f) in faces.iter().enumerate() { for k in 0..f.len() { let (a, b) = (f[k], f[(k + 1) % f.len()]); edge_faces.entry((a.min(b), a.max(b))).or_default().push(fi); } }
        let mut ep: HashMap<(usize, usize), usize> = HashMap::new();
        let mut np = pts.clone();
        for (e, fs) in &edge_faces {
            let p = (pts[e.0] + pts[e.1] + fs.iter().map(|f| fp[*f]).fold(V3::ZERO, |a, b| a + b)) / (2 + fs.len()) as f64;
            ep.insert(*e, np.len());
            np.push(p);
        }
        // move the old points
        for (i, p) in pts.iter().enumerate() {
            let adj_f: Vec<usize> = faces.iter().enumerate().filter(|(_, f)| f.contains(&i)).map(|(k, _)| k).collect();
            let adj_e: Vec<&(usize, usize)> = edge_faces.keys().filter(|e| e.0 == i || e.1 == i).collect();
            let n = adj_f.len() as f64;
            if n < 3.0 { continue; }
            let f = adj_f.iter().map(|k| fp[*k]).fold(V3::ZERO, |a, b| a + b) / n;
            let r = adj_e.iter().map(|e| (pts[e.0] + pts[e.1]) * 0.5).fold(V3::ZERO, |a, b| a + b) / adj_e.len() as f64;
            np[i] = (f + r * 2.0 + *p * (n - 3.0)) / n;
        }
        let mut nf = vec![];
        for (fi, f) in faces.iter().enumerate() {
            let c = np.len();
            np.push(fp[fi]);
            for k in 0..f.len() {
                let (prev, cur, next) = (f[(k + f.len() - 1) % f.len()], f[k], f[(k + 1) % f.len()]);
                nf.push(vec![cur, ep[&(cur.min(next), cur.max(next))], c, ep[&(prev.min(cur), prev.max(cur))]]);
            }
        }
        pts = np;
        faces = nf;
    }
    let center = frame.origin + frame.z() * (h / 2.0);
    faces.iter().flat_map(|f| {
        let q: Vec<V3> = f.iter().map(|&i| pts[i]).collect();
        let c = q.iter().fold(V3::ZERO, |a, b| a + *b) / q.len() as f64;
        tris(&q, tag, c - center)
    }).collect()
}

/// A big box on the positive side of `frame` (z ≥ 0), as a cutting body.
pub fn half_space(frame: &Frame, size: f64, tag: u32) -> Vec<CPolygon> {
    let c = |x: f64, y: f64, z: f64| frame.origin + frame.x * (x * size) + frame.y * (y * size) + frame.z() * (z * size);
    let bottom: Vec<CPolygon> = [[c(-1.0, -1.0, 0.0), c(1.0, -1.0, 0.0), c(1.0, 1.0, 0.0), c(-1.0, 1.0, 0.0)]].iter().filter_map(|q| poly(q, tag)).collect();
    let mut out = thicken(&bottom, 0.0, Some(frame.z() * (2.0 * size)), tag);
    out = crate::kernel::csg::orient_outward(out);
    out
}
