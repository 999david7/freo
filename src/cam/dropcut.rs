//! Drop-cutter: how low a ball or flat end mill can go at (x, y) before it touches the part
//! (vertex, edge and facet contacts against every nearby triangle).

use super::tools::{Tool, ToolKind};
use crate::slicer::mesh::Mesh;
use glam::DVec3;

pub struct DropCutter {
    tris: Vec<[DVec3; 3]>,
    x0: f64,
    y0: f64,
    cell: f64,
    nx: usize,
    ny: usize,
    bins: Vec<Vec<u32>>,
}

impl DropCutter {
    pub fn new(m: &Mesh, reach: f64) -> DropCutter {
        let tris: Vec<[DVec3; 3]> = (0..m.t.len()).map(|i| m.tri(i)).collect();
        let (lo, hi) = m.bbox();
        // the grid reaches `reach` beyond the part: the cutter touches edges from outside
        let lo = lo - DVec3::splat(reach);
        let hi = hi + DVec3::splat(reach);
        let cell = reach.max(1.0) * 2.0;
        let nx = (((hi.x - lo.x) / cell).ceil() as usize + 1).clamp(1, 4000);
        let ny = (((hi.y - lo.y) / cell).ceil() as usize + 1).clamp(1, 4000);
        let mut bins = vec![vec![]; nx * ny];
        for (k, t) in tris.iter().enumerate() {
            let (mut a, mut b) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
            for p in t {
                a = a.min(*p);
                b = b.max(*p);
            }
            let i0 = (((a.x - reach - lo.x) / cell).floor().max(0.0) as usize).min(nx - 1);
            let i1 = (((b.x + reach - lo.x) / cell).floor().max(0.0) as usize).min(nx - 1);
            let j0 = (((a.y - reach - lo.y) / cell).floor().max(0.0) as usize).min(ny - 1);
            let j1 = (((b.y + reach - lo.y) / cell).floor().max(0.0) as usize).min(ny - 1);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    bins[j * nx + i].push(k as u32);
                }
            }
        }
        DropCutter { tris, x0: lo.x, y0: lo.y, cell, nx, ny, bins }
    }

    /// Lowest tip height of the tool at (x, y) not cutting into the part (None: no part under
    /// the tool).
    pub fn drop(&self, t: &Tool, x: f64, y: f64) -> Option<f64> {
        let i = ((x - self.x0) / self.cell).floor();
        let j = ((y - self.y0) / self.cell).floor();
        if i < 0.0 || j < 0.0 || i as usize >= self.nx || j as usize >= self.ny {
            return None;
        }
        let r = t.radius();
        let mut best: Option<f64> = None;
        let mut up = |z: f64| {
            if best.is_none_or(|b| z > b) {
                best = Some(z);
            }
        };
        for &k in &self.bins[j as usize * self.nx + i as usize] {
            let tri = &self.tris[k as usize];
            match t.kind {
                ToolKind::Ball => ball(tri, x, y, r, &mut up),
                _ => flat(tri, x, y, r, &mut up),
            }
        }
        best
    }
}

/// Height of the triangle's plane at (x, y) when the point lies inside its projection.
pub fn tri_z(tri: &[DVec3; 3], x: f64, y: f64) -> Option<f64> {
    let (a, b, c) = (tri[0], tri[1], tri[2]);
    let d = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
    if d.abs() < 1e-14 {
        return None;
    }
    let l1 = ((b.y - c.y) * (x - c.x) + (c.x - b.x) * (y - c.y)) / d;
    let l2 = ((c.y - a.y) * (x - c.x) + (a.x - c.x) * (y - c.y)) / d;
    let l3 = 1.0 - l1 - l2;
    let e = -1e-9;
    (l1 >= e && l2 >= e && l3 >= e).then_some(l1 * a.z + l2 * b.z + l3 * c.z)
}

/// Ball end mill (radius r): highest tip z touching the triangle.
fn ball(tri: &[DVec3; 3], x: f64, y: f64, r: f64, up: &mut impl FnMut(f64)) {
    // vertices
    for p in tri {
        let d2 = (p.x - x).powi(2) + (p.y - y).powi(2);
        if d2 <= r * r {
            up(p.z + (r * r - d2).sqrt() - r);
        }
    }
    // edges
    for k in 0..3 {
        let (a, b) = (tri[k], tri[(k + 1) % 3]);
        let e = b - a;
        let lxy = (e.x * e.x + e.y * e.y).sqrt();
        if lxy < 1e-12 {
            continue;
        }
        let u = DVec3::new(e.x / lxy, e.y / lxy, 0.0);
        // horizontal distance from the line
        let w = DVec3::new(x - a.x, y - a.y, 0.0);
        let along = w.x * u.x + w.y * u.y;
        let dist = (w.x * u.y - w.y * u.x).abs();
        if dist > r {
            continue;
        }
        let rr = (r * r - dist * dist).sqrt();
        // in the vertical plane of the edge: circle of radius rr touching the line z = a.z + m s
        let m = e.z / lxy;
        // contact point: the foot of the perpendicular from the circle centre to the line,
        // further along the edge where it climbs
        let s = along + rr * m / (1.0 + m * m).sqrt();
        if s < 0.0 || s > lxy {
            continue;
        }
        // circle centre at `along`, rr from the line
        let cz = a.z + m * along + rr * (1.0 + m * m).sqrt();
        up(cz - r);
    }
    // facet
    let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
    let l = n.length();
    if l < 1e-14 {
        return;
    }
    let mut n = n / l;
    if n.z < 0.0 {
        n = -n;
    }
    if n.z < 1e-9 {
        return;
    }
    // contact point below the centre along the normal
    let (cx, cy) = (x - n.x * r, y - n.y * r);
    if let Some(z) = tri_z(tri, cx, cy) {
        up(z + n.z * r - r);
    }
}

/// Flat end mill: highest tip z touching the triangle within the cutter's disk.
fn flat(tri: &[DVec3; 3], x: f64, y: f64, r: f64, up: &mut impl FnMut(f64)) {
    for p in tri {
        if (p.x - x).powi(2) + (p.y - y).powi(2) <= r * r {
            up(p.z);
        }
    }
    for k in 0..3 {
        let (a, b) = (tri[k], tri[(k + 1) % 3]);
        let d = b - a;
        // points of the segment inside the disk: |a + t d - c|² ≤ r² in xy
        let (fx, fy) = (a.x - x, a.y - y);
        let qa = d.x * d.x + d.y * d.y;
        if qa < 1e-14 {
            continue;
        }
        let qb = 2.0 * (fx * d.x + fy * d.y);
        let qc = fx * fx + fy * fy - r * r;
        let disc = qb * qb - 4.0 * qa * qc;
        if disc < 0.0 {
            continue;
        }
        let sq = disc.sqrt();
        let t0 = ((-qb - sq) / (2.0 * qa)).max(0.0);
        let t1 = ((-qb + sq) / (2.0 * qa)).min(1.0);
        if t0 <= t1 {
            up(a.z + d.z * t0);
            up(a.z + d.z * t1);
        }
    }
    let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
    let l = n.length();
    if l < 1e-14 {
        return;
    }
    let mut n = n / l;
    if n.z < 0.0 {
        n = -n;
    }
    if n.z < 1e-9 {
        return;
    }
    // highest point of the plane under the disk: towards the uphill direction
    let hx = (n.x * n.x + n.y * n.y).sqrt();
    let (cx, cy) = if hx < 1e-12 { (x, y) } else { (x - n.x / hx * r, y - n.y / hx * r) };
    if let Some(z) = tri_z(tri, cx, cy) {
        up(z);
    }
    if let Some(z) = tri_z(tri, x, y) {
        up(z);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane_mesh(z: f64, tilt: f64) -> Mesh {
        let v = vec![DVec3::new(0.0, 0.0, z), DVec3::new(100.0, 0.0, z + 100.0 * tilt), DVec3::new(100.0, 100.0, z + 100.0 * tilt), DVec3::new(0.0, 100.0, z)];
        Mesh { v, t: vec![[0, 1, 2], [0, 2, 3]] }
    }

    /// Random triangles of every steepness against brute force: the cutter never ends up
    /// inside a triangle, and never more than a hair above the true contact.
    #[test]
    fn matches_brute_force() {
        let mut seed = 0x2545F4914F6CDD1Du64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for tool in [Tool::ball(1, 6.0, 2), Tool::flat(2, 6.0, 2)] {
            for _ in 0..300 {
                let p = |r: &mut dyn FnMut() -> f64| DVec3::new(r() * 8.0 - 4.0, r() * 8.0 - 4.0, r() * 8.0 - 4.0);
                let t = [p(&mut rnd), p(&mut rnd), p(&mut rnd)];
                let m = Mesh { v: t.to_vec(), t: vec![[0, 1, 2]] };
                let dc = DropCutter::new(&m, 3.5);
                let (x, y) = (rnd() * 2.0 - 1.0, rnd() * 2.0 - 1.0);
                let r = tool.radius();
                let mut need = f64::NEG_INFINITY;
                for i in 0..=120 {
                    for j in 0..=(120 - i) {
                        let (u, v) = (i as f64 / 120.0, j as f64 / 120.0);
                        let q = t[0] + (t[1] - t[0]) * u + (t[2] - t[0]) * v;
                        let d = (q.x - x).hypot(q.y - y);
                        if let Some(pr) = tool.profile(d) {
                            if d < r {
                                need = need.max(q.z - pr);
                            }
                        }
                    }
                }
                let got = dc.drop(&tool, x, y);
                if need.is_finite() {
                    let z = got.expect("contact missed");
                    assert!(z >= need - 1e-9, "{:?} {:?}: {} below {}", tool.kind, t, z, need);
                    assert!(z <= need + 0.05, "{:?} {:?}: {} far above {}", tool.kind, t, z, need);
                }
            }
        }
    }

    #[test]
    fn drops_on_planes_and_edges() {
        let ball = Tool::ball(1, 6.0, 2);
        let flat_t = Tool::flat(2, 6.0, 2);
        let m = plane_mesh(5.0, 0.0);
        let dc = DropCutter::new(&m, 3.0);
        assert!((dc.drop(&ball, 50.0, 50.0).unwrap() - 5.0).abs() < 1e-9);
        assert!((dc.drop(&flat_t, 50.0, 50.0).unwrap() - 5.0).abs() < 1e-9);
        // 45° slope: a ball's tip sits r(√2 − 1) above the plane point under the axis
        let s = plane_mesh(0.0, 1.0);
        let dc = DropCutter::new(&s, 3.0);
        let z = dc.drop(&ball, 50.0, 50.0).unwrap();
        assert!((z - (50.0 + 3.0 * (2f64.sqrt() - 1.0))).abs() < 1e-6, "{}", z);
        // a flat cutter rests on its rim, r up the slope
        let z = dc.drop(&flat_t, 50.0, 50.0).unwrap();
        assert!((z - 53.0).abs() < 1e-6, "{}", z);
        // over the edge of a raised plate: the ball hangs on the edge
        let dc = DropCutter::new(&m, 3.0);
        let z = dc.drop(&ball, 102.0, 50.0).unwrap();
        assert!((z - (5.0 + (9.0f64 - 4.0).sqrt() - 3.0)).abs() < 1e-6, "{}", z);
    }
}
