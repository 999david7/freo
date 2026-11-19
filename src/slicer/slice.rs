//! Cutting the mesh into layers: every triangle crossing a slicing plane gives one oriented
//! segment, segments are chained into closed loops and the loops become regions (non-zero
//! winding, so overlapping bodies merge and inside-out meshes still slice).

use super::mesh::Mesh;
use super::poly::{self, Contour, Pt, Shapes};
use std::collections::HashMap;

/// Layer boundaries: (bottom, top) of every layer, first layer `first` high, the others `h`.
pub fn layer_bounds(first: f64, h: f64, top: f64) -> Vec<(f64, f64)> {
    let mut out = vec![];
    if top <= 0.0 || first <= 0.0 || h <= 0.0 {
        return out;
    }
    out.push((0.0, first));
    // a layer is added while at least half of it holds part; heights are computed from the
    // layer number so they do not drift
    let mut k = 1usize;
    loop {
        let z0 = first + (k - 1) as f64 * h;
        if z0 + h * 0.5 >= top - 1e-9 {
            break;
        }
        out.push((z0, first + k as f64 * h));
        k += 1;
    }
    out
}

/// Crossing segment of triangle `t` with the plane `z`: from where the triangle goes down
/// through the plane to where it comes up, which keeps the solid on the left (outer loops
/// counter-clockwise seen from above). A corner exactly on the plane counts as above.
fn cut(m: &Mesh, t: [u32; 3], z: f64) -> Option<(Pt, Pt)> {
    let p = [m.v[t[0] as usize], m.v[t[1] as usize], m.v[t[2] as usize]];
    let above = [p[0].z >= z, p[1].z >= z, p[2].z >= z];
    if above[0] == above[1] && above[1] == above[2] {
        return None;
    }
    let mut start = None;
    let mut end = None;
    for k in 0..3 {
        let (i, j) = (k, (k + 1) % 3);
        if above[i] == above[j] {
            continue;
        }
        // interpolate from the lower vertex index so both triangles of an edge agree exactly
        let (a, b) = if t[i] < t[j] { (p[i], p[j]) } else { (p[j], p[i]) };
        let s = (z - a.z) / (b.z - a.z);
        let q = [a.x + (b.x - a.x) * s, a.y + (b.y - a.y) * s];
        if above[i] {
            start = Some(q);
        } else {
            end = Some(q);
        }
    }
    Some((start?, end?))
}

type Key = (i64, i64);

fn key(p: Pt, cell: f64) -> Key {
    ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64)
}

/// Chains oriented segments into loops. Endpoints closer than `tol` are the same point; open
/// chains (holes in the mesh) are joined to the nearest chain start or closed on themselves.
pub fn chain(segs: &[(Pt, Pt)], tol: f64) -> Vec<Contour> {
    let n = segs.len();
    let mut starts: HashMap<Key, Vec<usize>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        starts.entry(key(s.0, tol)).or_default().push(i);
    }
    let mut used = vec![false; n];
    let find = |p: Pt, used: &Vec<bool>, starts: &HashMap<Key, Vec<usize>>| -> Option<usize> {
        let k = key(p, tol);
        let mut best: Option<(usize, f64)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(v) = starts.get(&(k.0 + dx, k.1 + dy)) {
                    for &i in v {
                        if used[i] {
                            continue;
                        }
                        let d = poly::dist2(segs[i].0, p);
                        if d <= tol * tol && best.is_none_or(|b| d < b.1) {
                            best = Some((i, d));
                        }
                    }
                }
            }
        }
        best.map(|b| b.0)
    };
    let mut loops = vec![];
    let mut open: Vec<Contour> = vec![];
    for s0 in 0..n {
        if used[s0] {
            continue;
        }
        used[s0] = true;
        let mut pts = vec![segs[s0].0];
        let mut cur = segs[s0].1;
        let mut closed = false;
        loop {
            if poly::dist2(cur, pts[0]) <= tol * tol && pts.len() >= 2 {
                closed = true;
                break;
            }
            match find(cur, &used, &starts) {
                Some(i) => {
                    used[i] = true;
                    pts.push(segs[i].0);
                    cur = segs[i].1;
                }
                None => break,
            }
        }
        if closed {
            if pts.len() >= 3 {
                loops.push(pts);
            }
        } else {
            pts.push(cur);
            open.push(pts);
        }
    }
    // repair: join open chains end to the nearest open start, then close what is left
    while let Some(mut c) = open.pop() {
        loop {
            let end = *c.last().unwrap();
            let mut best: Option<(usize, f64)> = None;
            for (i, o) in open.iter().enumerate() {
                let d = poly::dist2(end, o[0]);
                if best.is_none_or(|b| d < b.1) {
                    best = Some((i, d));
                }
            }
            let own = poly::dist2(end, c[0]);
            match best {
                Some((i, d)) if d < own && d < 4.0 => {
                    let o = open.swap_remove(i);
                    c.extend(o);
                }
                _ => break,
            }
        }
        if c.len() >= 3 && poly::contour_area(&c).abs() > 1e-6 {
            loops.push(c);
        }
    }
    loops
}

/// Regions of every slicing plane, in parallel. `resolution` simplifies the loops (Orca's
/// "resolution", 0.012 mm).
pub fn slice_mesh(m: &Mesh, planes: &[f64], resolution: f64) -> Vec<Shapes> {
    let nl = planes.len();
    if nl == 0 || m.is_empty() {
        return vec![vec![]; nl];
    }
    // triangles per plane: zmin < z <= zmax crosses with the "on the plane is above" rule
    let mut buckets: Vec<Vec<u32>> = vec![vec![]; nl];
    for (ti, t) in m.t.iter().enumerate() {
        let zs = [m.v[t[0] as usize].z, m.v[t[1] as usize].z, m.v[t[2] as usize].z];
        let zmin = zs[0].min(zs[1]).min(zs[2]);
        let zmax = zs[0].max(zs[1]).max(zs[2]);
        let i0 = planes.partition_point(|&p| p <= zmin);
        let i1 = planes.partition_point(|&p| p <= zmax);
        for b in buckets.iter_mut().take(i1).skip(i0) {
            b.push(ti as u32);
        }
    }
    let mut out: Vec<Shapes> = vec![vec![]; nl];
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 16);
    let chunk = nl.div_ceil(threads);
    std::thread::scope(|sc| {
        for (ci, slot) in out.chunks_mut(chunk).enumerate() {
            let buckets = &buckets;
            sc.spawn(move || {
                for (k, s) in slot.iter_mut().enumerate() {
                    let li = ci * chunk + k;
                    let z = planes[li];
                    let segs: Vec<(Pt, Pt)> = buckets[li].iter().filter_map(|&ti| cut(m, m.t[ti as usize], z)).filter(|s| poly::dist2(s.0, s.1) > 1e-18).collect();
                    let loops = chain(&segs, 1e-4);
                    let loops: Vec<Contour> = loops.into_iter().map(|c| poly::simplify_contour(&c, resolution * 0.5)).collect();
                    *s = poly::drop_small(poly::from_loops(&loops), 1e-4);
                }
            });
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    fn cube(s: f64) -> Mesh {
        let v = [[0.0, 0.0, 0.0], [s, 0.0, 0.0], [s, s, 0.0], [0.0, s, 0.0], [0.0, 0.0, s], [s, 0.0, s], [s, s, s], [0.0, s, s]].map(DVec3::from_array);
        let t = vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
        Mesh { v: v.to_vec(), t }
    }

    #[test]
    fn bounds() {
        let b = layer_bounds(0.2, 0.2, 10.0);
        assert_eq!(b.len(), 50);
        assert!((b[49].1 - 10.0).abs() < 1e-9);
        let b = layer_bounds(0.3, 0.2, 1.0);
        assert_eq!(b.len(), 4); // 0.3 0.5 0.7 0.9 (1.1 would be mostly above the part)
    }

    #[test]
    fn cube_slices_ccw_square() {
        let m = cube(10.0);
        let s = slice_mesh(&m, &[0.0, 0.1, 5.0, 9.999, 10.001], 0.012);
        assert!((poly::area(&s[1]) - 100.0).abs() < 1e-6, "{}", poly::area(&s[1]));
        assert!((poly::area(&s[2]) - 100.0).abs() < 1e-6);
        assert!((poly::area(&s[3]) - 100.0).abs() < 0.02, "{}", poly::area(&s[3]));
        // above the part, and on the bottom face (counts as above): nothing
        assert!(s[4].is_empty());
        assert!(s[0].is_empty());
        assert!(poly::contour_area(&s[2][0][0]) > 0.0);
        // inside-out cube still slices
        let mut inv = m.clone();
        for t in &mut inv.t {
            t.swap(1, 2);
        }
        let s = slice_mesh(&inv, &[5.0], 0.012);
        assert!((poly::area(&s[0]) - 100.0).abs() < 1e-6);
    }

    #[test]
    fn chains_with_gaps() {
        // square with a missing segment closes anyway
        let segs = vec![([0.0, 0.0], [1.0, 0.0]), ([1.0, 0.0], [1.0, 1.0]), ([1.0, 1.0], [0.0, 1.0])];
        let l = chain(&segs, 1e-4);
        assert_eq!(l.len(), 1);
        assert!((poly::contour_area(&l[0]) - 1.0).abs() < 1e-9);
    }
}
