//! Ear-clipping triangulation for simple polygons with holes (holes bridged into the outer ring).

use super::math::*;

/// Returns CCW triangles as index triples into `outer ++ holes...`.
pub fn triangulate(outer_in: &[V2], holes_in: &[Vec<V2>]) -> Vec<[usize; 3]> {
    let mut pts: Vec<V2> = outer_in.to_vec();
    let mut outer: Vec<usize> = (0..outer_in.len()).collect();
    if polygon_area2(outer_in) < 0.0 { outer.reverse(); }
    let mut holes: Vec<Vec<usize>> = Vec::new();
    for h in holes_in {
        if h.len() < 3 { continue; }
        let base = pts.len();
        pts.extend_from_slice(h);
        let mut idx: Vec<usize> = (base..base + h.len()).collect();
        if polygon_area2(h) > 0.0 { idx.reverse(); }
        holes.push(idx);
    }
    if outer.len() < 3 { return vec![]; }
    let maxx = |h: &Vec<usize>| h.iter().map(|&i| pts[i].x).fold(f64::NEG_INFINITY, f64::max);
    holes.sort_by(|a, b| maxx(b).partial_cmp(&maxx(a)).unwrap_or(std::cmp::Ordering::Equal));

    let mut ring = outer;
    for hi in 0..holes.len() {
        let hole = holes[hi].clone();
        let mut mi = 0;
        for k in 1..hole.len() { if pts[hole[k]].x > pts[hole[mi]].x { mi = k; } }
        let m = hole[mi];
        let mp = pts[m];
        let mut order: Vec<usize> = (0..ring.len()).collect();
        order.sort_by(|&a, &b| (pts[ring[a]] - mp).length().partial_cmp(&(pts[ring[b]] - mp).length()).unwrap());
        let others = &holes[hi..];
        let mut bridge = None;
        for ri in order {
            let p = ring[ri];
            let pp = pts[p];
            if (pp - mp).length() < 1e-12 { continue; }
            let prev = pts[ring[(ri + ring.len() - 1) % ring.len()]];
            let next = pts[ring[(ri + 1) % ring.len()]];
            if !locally_inside(prev, pp, next, mp) { continue; }
            if blocked(mp, pp, &ring, others, &pts, m, p) { continue; }
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
    ear_clip(ring, &pts)
}

fn locally_inside(a: V2, b: V2, c: V2, p: V2) -> bool {
    let d = p - b;
    if cross2(b - a, c - b) >= 0.0 {
        cross2(c - b, d) >= 0.0 && cross2(d, a - b) >= 0.0
    } else {
        !(cross2(a - b, d) > 0.0 && cross2(d, c - b) > 0.0)
    }
}

fn seg_intersect(p1: V2, p2: V2, q1: V2, q2: V2) -> bool {
    let e = 1e-12;
    let d1 = cross2(q2 - q1, p1 - q1);
    let d2 = cross2(q2 - q1, p2 - q1);
    let d3 = cross2(p2 - p1, q1 - p1);
    let d4 = cross2(p2 - p1, q2 - p1);
    ((d1 > e && d2 < -e) || (d1 < -e && d2 > e)) && ((d3 > e && d4 < -e) || (d3 < -e && d4 > e))
}

fn blocked(a: V2, b: V2, ring: &[usize], holes: &[Vec<usize>], pts: &[V2], sa: usize, sb: usize) -> bool {
    let check = |lp: &[usize]| -> bool {
        let n = lp.len();
        for i in 0..n {
            let (u, v) = (lp[i], lp[(i + 1) % n]);
            if u == sa || v == sa || u == sb || v == sb { continue; }
            let (pu, pv) = (pts[u], pts[v]);
            let near = |x: V2, y: V2| (x - y).length() < 1e-12;
            if near(pu, a) || near(pv, a) || near(pu, b) || near(pv, b) { continue; }
            if seg_intersect(a, b, pu, pv) { return true; }
        }
        false
    };
    check(ring) || holes.iter().any(|h| check(h))
}

fn ear_clip(ring_in: Vec<usize>, pts: &[V2]) -> Vec<[usize; 3]> {
    let mut ring: Vec<usize> = Vec::with_capacity(ring_in.len());
    for (i, &v) in ring_in.iter().enumerate() {
        if ring_in[(i + 1) % ring_in.len()] != v { ring.push(v); }
    }
    let mut tris = Vec::new();
    let mut guard = 0usize;
    let mut i = 0usize;
    while ring.len() > 3 && guard < ring.len() * 4 + 10 {
        let n = ring.len();
        let (ia, ib, ic) = (ring[(i + n - 1) % n], ring[i % n], ring[(i + 1) % n]);
        if is_ear(ia, ib, ic, &ring, pts) {
            tris.push([ia, ib, ic]);
            ring.remove(i % n);
            guard = 0;
            i = if i > 0 { (i - 1) % ring.len().max(1) } else { 0 };
            continue;
        }
        i = (i + 1) % n;
        guard += 1;
        if guard > n {
            let mut removed = false;
            for k in 0..n {
                let (a, b, c) = (pts[ring[(k + n - 1) % n]], pts[ring[k]], pts[ring[(k + 1) % n]]);
                if cross2(b - a, c - b).abs() < 1e-12 { ring.remove(k); removed = true; break; }
            }
            if !removed {
                let mut best = 0;
                let mut best_v = f64::NEG_INFINITY;
                for k in 0..n {
                    let (a, b, c) = (pts[ring[(k + n - 1) % n]], pts[ring[k]], pts[ring[(k + 1) % n]]);
                    let cr = cross2(b - a, c - b);
                    if cr > best_v { best_v = cr; best = k; }
                }
                if best_v > 0.0 { tris.push([ring[(best + n - 1) % n], ring[best], ring[(best + 1) % n]]); }
                ring.remove(best);
            }
            guard = 0;
            i = 0;
        }
    }
    if ring.len() == 3 {
        let (a, b, c) = (pts[ring[0]], pts[ring[1]], pts[ring[2]]);
        if cross2(b - a, c - b) > 0.0 { tris.push([ring[0], ring[1], ring[2]]); }
    }
    tris
}

fn is_ear(ia: usize, ib: usize, ic: usize, ring: &[usize], pts: &[V2]) -> bool {
    let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
    if cross2(b - a, c - b) <= 1e-14 { return false; }
    let n = ring.len();
    for k in 0..n {
        let ip = ring[k];
        if ip == ia || ip == ib || ip == ic { continue; }
        let p = pts[ip];
        if (p - a).length() < 1e-12 || (p - b).length() < 1e-12 || (p - c).length() < 1e-12 { continue; }
        let pp = pts[ring[(k + n - 1) % n]];
        let pn = pts[ring[(k + 1) % n]];
        if cross2(p - pp, pn - p) > 1e-14 { continue; }
        let e = -1e-12;
        if cross2(b - a, p - a) >= e && cross2(c - b, p - b) >= e && cross2(a - c, p - c) >= e { return false; }
    }
    true
}
