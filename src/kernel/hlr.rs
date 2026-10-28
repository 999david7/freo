//! Hidden-line removal for drawing views. Model edges and the silhouettes of curved faces are
//! projected with a view rotation and split into visible and hidden polylines by depth tests
//! against the model's facets; circles seen face-on and axes seen side-on are reported for
//! centre marks and centre lines.

use super::math::*;
use super::topology::Topology;
use glam::DQuat;
use std::collections::{HashMap, HashSet};

/// Occlusion queries in view space (x right, y up, z toward the viewer).
pub struct Projector {
    /// world → view rotation
    pub rot: DQuat,
    tris: Vec<([V3; 3], usize)>,
    grid: HashMap<(i64, i64), Vec<u32>>,
    cell: f64,
    eps: f64,
}

fn tri_depth(t: &[V3; 3], p: V2) -> Option<f64> {
    let (a, b, c) = (t[0], t[1], t[2]);
    let d = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
    if d.abs() < 1e-14 { return None; }
    let w1 = ((p.x - a.x) * (c.y - a.y) - (c.x - a.x) * (p.y - a.y)) / d;
    let w2 = ((b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y)) / d;
    let w0 = 1.0 - w1 - w2;
    let e = -1e-9;
    if w0 < e || w1 < e || w2 < e { return None; }
    Some(a.z * w0 + b.z * w1 + c.z * w2)
}

impl Projector {
    pub fn new(topo: &Topology, rot: DQuat) -> Projector {
        let diag = topo.bbox.diagonal().max(1.0);
        let mut tris = vec![];
        for (pi, p) in topo.polys.iter().enumerate() {
            let q: Vec<V3> = p.iter().map(|&v| rot * topo.positions[v]).collect();
            for k in 1..q.len().saturating_sub(1) {
                let t = [q[0], q[k], q[k + 1]];
                let area = ((t[1].x - t[0].x) * (t[2].y - t[0].y) - (t[2].x - t[0].x) * (t[1].y - t[0].y)).abs();
                if area > 1e-12 * diag * diag { tris.push((t, pi)); }
            }
        }
        let n = (tris.len() as f64).sqrt().clamp(8.0, 256.0);
        let cell = (diag / n).max(1e-6);
        let mut grid: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
        for (i, (t, _)) in tris.iter().enumerate() {
            let (mut lo, mut hi) = (V2::splat(f64::INFINITY), V2::splat(f64::NEG_INFINITY));
            for v in t { lo = lo.min(v.truncate()); hi = hi.max(v.truncate()); }
            for x in (lo.x / cell).floor() as i64..=(hi.x / cell).floor() as i64 {
                for y in (lo.y / cell).floor() as i64..=(hi.y / cell).floor() as i64 { grid.entry((x, y)).or_default().push(i as u32); }
            }
        }
        Projector { rot, tris, grid, cell, eps: diag * 1e-6 + 1e-9 }
    }

    pub fn to_view(&self, p: V3) -> V3 { self.rot * p }

    /// True when the view-space point is behind a facet that is not one of `own`.
    pub fn hidden(&self, q: V3, own: &[usize]) -> bool {
        let key = ((q.x / self.cell).floor() as i64, (q.y / self.cell).floor() as i64);
        let Some(list) = self.grid.get(&key) else { return false };
        for &i in list {
            let (t, pi) = &self.tris[i as usize];
            if own.contains(pi) { continue; }
            if let Some(z) = tri_depth(t, q.truncate()) { if z > q.z + self.eps { return true; } }
        }
        false
    }
}

#[derive(Clone, Debug, Default)]
pub struct ViewChain {
    /// topology edge chain
    pub chain: usize,
    pub pts: Vec<V2>,
    pub closed: bool,
    /// centre and radius when the chain is a circle seen face-on
    pub circle: Option<(V2, f64)>,
    pub visible: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ViewLines {
    pub visible: Vec<Vec<V2>>,
    pub hidden: Vec<Vec<V2>>,
    /// circles seen face-on (centre, radius), for centre marks
    pub circles: Vec<(V2, f64)>,
    /// axes of revolution lying in the view plane, for centre lines
    pub axes: Vec<[V2; 2]>,
    pub min: V2,
    pub max: V2,
    pub chains: Vec<ViewChain>,
}

/// Circle through a closed chain: (centre, unit normal, radius) when its points are concyclic.
pub fn chain_circle(pts: &[V3]) -> Option<(V3, V3, f64)> {
    if pts.len() < 6 { return None; }
    let c0 = pts.iter().copied().sum::<V3>() / pts.len() as f64;
    let mut n = V3::ZERO;
    for i in 0..pts.len() { let (a, b) = (pts[i] - c0, pts[(i + 1) % pts.len()] - c0); n += a.cross(b); }
    if n.length() < 1e-12 { return None; }
    let n = n.normalize();
    let x = safe_norm(n.cross(if n.x.abs() < 0.9 { V3::X } else { V3::Y }), V3::X);
    let y = n.cross(x);
    // algebraic least squares: u² + v² + D u + E v + F = 0 (independent of vertex spacing)
    let mut a = [0.0f64; 9];
    let mut b = [0.0f64; 3];
    for p in pts {
        let d = *p - c0;
        let (u, v) = (d.dot(x), d.dot(y));
        let row = [u, v, 1.0];
        let rhs = -(u * u + v * v);
        for i in 0..3 { for j in 0..3 { a[i * 3 + j] += row[i] * row[j]; } b[i] += row[i] * rhs; }
    }
    let s = solve_dense(&a, &b, 3)?;
    let (cu, cv) = (-s[0] / 2.0, -s[1] / 2.0);
    let r2 = cu * cu + cv * cv - s[2];
    if r2 <= 1e-18 { return None; }
    let r = r2.sqrt();
    let c = c0 + x * cu + y * cv;
    let ok = pts.iter().all(|p| ((*p - c).length() - r).abs() < r * 0.01 + 1e-6 && (*p - c).dot(n).abs() < r * 0.01 + 1e-6);
    ok.then_some((c, n, r))
}

/// Projects the model seen through `rot` (world → view). `axes` are (point, direction) pairs.
pub fn project(topo: &Topology, axes: &[(V3, V3)], rot: DQuat) -> ViewLines {
    let mut out = ViewLines::default();
    if topo.polys.is_empty() { return out; }
    let pr = Projector::new(topo, rot);
    let diag = topo.bbox.diagonal().max(1.0);
    let q: Vec<V3> = topo.positions.iter().map(|p| rot * *p).collect();
    let (mut lo, mut hi) = (V2::splat(f64::INFINITY), V2::splat(f64::NEG_INFINITY));
    for (i, p) in q.iter().enumerate() {
        if topo.polys.is_empty() || i >= q.len() { break; }
        lo = lo.min(p.truncate());
        hi = hi.max(p.truncate());
    }
    out.min = lo;
    out.max = hi;
    // polygon adjacency per undirected edge
    let mut adj: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (pi, p) in topo.polys.iter().enumerate() {
        for k in 0..p.len() {
            let (u, v) = (p[k], p[(k + 1) % p.len()]);
            if u != v { adj.entry((u.min(v), u.max(v))).or_default().push(pi); }
        }
    }
    let feature: HashSet<(usize, usize)> = topo.draw_edges.iter().map(|&(a, b)| (a.min(b), a.max(b))).collect();
    let front = |pi: usize| (rot * topo.planes[pi].n).z > 1e-9;
    let mut segs: Vec<((usize, usize), Vec<usize>)> = vec![];
    let mut keys: Vec<&(usize, usize)> = adj.keys().collect();
    keys.sort();
    for k in keys {
        let ps = &adj[k];
        let take = feature.contains(k) || ps.len() == 1 || (ps.len() == 2 && front(ps[0]) != front(ps[1]));
        if take { segs.push((*k, ps.clone())); }
    }
    // classify each segment along its length
    let step = diag / 160.0;
    let mut vis: Vec<[V2; 2]> = vec![];
    let mut hid: Vec<[V2; 2]> = vec![];
    for ((a, b), own) in &segs {
        let (qa, qb) = (q[*a], q[*b]);
        let len = (qb.truncate() - qa.truncate()).length();
        if len < diag * 1e-7 { continue; }
        let at = |t: f64| qa + (qb - qa) * t;
        let n = ((len / step).ceil() as usize).clamp(1, 64);
        let ts: Vec<f64> = (0..n).map(|k| (k as f64 + 0.5) / n as f64).collect();
        let v: Vec<bool> = ts.iter().map(|t| !pr.hidden(at(*t), own)).collect();
        let mut start = 0.0;
        for k in 1..=n {
            if k < n && v[k] == v[k - 1] { continue; }
            let end = if k == n { 1.0 } else {
                let (mut l, mut h) = (ts[k - 1], ts[k]);
                for _ in 0..12 { let m = (l + h) * 0.5; if !pr.hidden(at(m), own) == v[k - 1] { l = m; } else { h = m; } }
                (l + h) * 0.5
            };
            let s = [at(start).truncate(), at(end).truncate()];
            if (s[1] - s[0]).length() > diag * 1e-7 { if v[k - 1] { vis.push(s) } else { hid.push(s) } }
            start = end;
        }
    }
    // hidden lines lying under visible ones are not drawn
    let tol = diag * 1e-5;
    let gc = diag / 64.0;
    let mut vg: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in vis.iter().enumerate() {
        let (l, h) = (s[0].min(s[1]), s[0].max(s[1]));
        for x in ((l.x - tol) / gc).floor() as i64..=((h.x + tol) / gc).floor() as i64 {
            for y in ((l.y - tol) / gc).floor() as i64..=((h.y + tol) / gc).floor() as i64 { vg.entry((x, y)).or_default().push(i); }
        }
    }
    let covered = |p: V2| vg.get(&((p.x / gc).floor() as i64, (p.y / gc).floor() as i64)).map(|l| l.iter().any(|&i| dist_point_segment2(p, vis[i][0], vis[i][1]) < tol)).unwrap_or(false);
    hid.retain(|s| ![0.2, 0.5, 0.8].iter().all(|t| covered(s[0] + (s[1] - s[0]) * *t)));
    out.visible = chain_segments(vis, diag);
    out.hidden = chain_segments(hid, diag);
    // circles and axes
    let mut circles3: Vec<(V3, V3, f64)> = vec![];
    for (ci, c) in topo.chains.iter().enumerate() {
        let pts2: Vec<V2> = c.points.iter().map(|p| (rot * *p).truncate()).collect();
        let mut vc = ViewChain { chain: ci, pts: pts2, closed: c.closed, circle: None, visible: false };
        let mid = rot * c.mid();
        let own: Vec<usize> = vec![];
        vc.visible = !pr.hidden(mid + V3::Z * (diag * 1e-5), &own);
        if c.closed {
            if let Some((cc, nn, r)) = chain_circle(&c.points) {
                circles3.push((cc, nn, r));
                if (rot * nn).z.abs() > 0.999 {
                    let p = (rot * cc).truncate();
                    vc.circle = Some((p, r));
                    match out.circles.iter_mut().find(|(q2, _)| (*q2 - p).length() < diag * 1e-6) {
                        Some(e) => e.1 = e.1.max(r),
                        None => out.circles.push((p, r)),
                    }
                }
            }
        }
        out.chains.push(vc);
    }
    for &(ap, ad) in axes {
        let d = safe_norm(ad, V3::Z);
        if (rot * d).z.abs() > 1e-3 { continue; }
        let (mut smin, mut smax) = (f64::INFINITY, f64::NEG_INFINITY);
        for (cc, nn, _) in &circles3 {
            if nn.cross(d).length() > 1e-3 { continue; }
            let v = *cc - ap;
            if (v - d * v.dot(d)).length() > diag * 1e-4 { continue; }
            smin = smin.min(v.dot(d));
            smax = smax.max(v.dot(d));
        }
        if smin.is_finite() && smax - smin > diag * 1e-6 {
            let seg = [(rot * (ap + d * smin)).truncate(), (rot * (ap + d * smax)).truncate()];
            if !out.axes.iter().any(|s| (s[0] - seg[0]).length() + (s[1] - seg[1]).length() < diag * 1e-5) { out.axes.push(seg); }
        }
    }
    out
}

/// Joins segments meeting end to end (at points shared by exactly two) into polylines,
/// without turning sharp corners, so dashed lines run continuously along curves.
fn chain_segments(segs: Vec<[V2; 2]>, diag: f64) -> Vec<Vec<V2>> {
    let tol = diag * 1e-7;
    let key = |p: V2| ((p.x / tol.max(1e-12)).round() as i64, (p.y / tol.max(1e-12)).round() as i64);
    let mut at: HashMap<(i64, i64), Vec<(usize, usize)>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() { for e in 0..2 { at.entry(key(s[e])).or_default().push((i, e)); } }
    let mut used = vec![false; segs.len()];
    let mut out = vec![];
    let cos_max = 35f64.to_radians().cos();
    for i in 0..segs.len() {
        if used[i] { continue; }
        used[i] = true;
        let mut line = vec![segs[i][0], segs[i][1]];
        for dir in 0..2 {
            loop {
                let (tip, prev) = if dir == 0 { (line[line.len() - 1], line[line.len() - 2]) } else { (line[0], line[1]) };
                let Some(list) = at.get(&key(tip)) else { break };
                if list.len() != 2 { break; }
                let Some(&(j, e)) = list.iter().find(|(j, _)| !used[*j]) else { break };
                let next = segs[j][1 - e];
                let (d0, d1) = (safe_norm2(tip - prev, V2::X), safe_norm2(next - tip, V2::X));
                if d0.dot(d1) < cos_max { break; }
                used[j] = true;
                if dir == 0 { line.push(next); } else { line.insert(0, next); }
            }
        }
        out.push(line);
    }
    out
}
