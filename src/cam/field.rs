//! Height field of the stock: the material's top surface on a fine grid. Tools are stamped
//! along every move (with their real profile), which removes material exactly the way the
//! cutter does in 3 axes. The same field answers "how high is the material around here",
//! which keeps every rapid move above the material.

use super::tools::Tool;

#[derive(Clone)]
pub struct Field {
    pub x0: f64,
    pub y0: f64,
    pub cell: f64,
    pub nx: usize,
    pub ny: usize,
    pub h: Vec<f32>,
    /// height of the stock bottom (the table)
    pub floor: f64,
    pub top: f64,
    bmax: Vec<f32>,
    dirty: Vec<bool>,
    bnx: usize,
    bny: usize,
}

const B: usize = 8;

/// What a stamp did.
#[derive(Clone, Copy, Default)]
pub struct Cut {
    /// largest amount of material removed at one cell
    pub depth: f32,
    /// cells cut
    pub cells: u32,
    /// lowest new height relative to the protected part height (negative = into the part)
    pub gouge: f32,
    pub gouge_at: (usize, usize),
}

impl Field {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64, floor: f64, top: f64, cell: f64) -> Field {
        let nx = (((x1 - x0) / cell).ceil() as usize).max(2);
        let ny = (((y1 - y0) / cell).ceil() as usize).max(2);
        let bnx = nx.div_ceil(B);
        let bny = ny.div_ceil(B);
        Field { x0, y0, cell, nx, ny, h: vec![top as f32; nx * ny], floor, top, bmax: vec![top as f32; bnx * bny], dirty: vec![false; bnx * bny], bnx, bny }
    }

    /// Cell size for a stock: about a million cells, at least 0.04 mm.
    pub fn cell_for(w: f64, d: f64) -> f64 {
        (w * d / 1.0e6).sqrt().max(0.04)
    }

    #[inline]
    pub fn center(&self, i: usize, j: usize) -> (f64, f64) {
        (self.x0 + (i as f64 + 0.5) * self.cell, self.y0 + (j as f64 + 0.5) * self.cell)
    }

    #[inline]
    fn index(&self, x: f64, y: f64) -> (i64, i64) {
        (((x - self.x0) / self.cell - 0.5).round() as i64, ((y - self.y0) / self.cell - 0.5).round() as i64)
    }

    pub fn at(&self, x: f64, y: f64) -> Option<f32> {
        let (i, j) = self.index(x, y);
        (i >= 0 && j >= 0 && (i as usize) < self.nx && (j as usize) < self.ny).then(|| self.h[j as usize * self.nx + i as usize])
    }

    fn refresh(&mut self, b: usize) {
        if !self.dirty[b] {
            return;
        }
        let (bi, bj) = (b % self.bnx, b / self.bnx);
        let mut m = f32::NEG_INFINITY;
        for j in bj * B..((bj + 1) * B).min(self.ny) {
            for i in bi * B..((bi + 1) * B).min(self.nx) {
                m = m.max(self.h[j * self.nx + i]);
            }
        }
        self.bmax[b] = m;
        self.dirty[b] = false;
    }

    /// Highest material within `r` of (x, y) — conservative (whole blocks), never too low.
    pub fn max_near(&mut self, x: f64, y: f64, r: f64) -> f64 {
        let bc = self.cell * B as f64;
        let bi0 = (((x - r - self.x0) / bc).floor() as i64).max(0);
        let bj0 = (((y - r - self.y0) / bc).floor() as i64).max(0);
        let bi1 = (((x + r - self.x0) / bc).floor() as i64).min(self.bnx as i64 - 1);
        let bj1 = (((y + r - self.y0) / bc).floor() as i64).min(self.bny as i64 - 1);
        let mut m = f64::NEG_INFINITY;
        if bi0 > bi1 || bj0 > bj1 {
            return m;
        }
        for bj in bj0..=bj1 {
            for bi in bi0..=bi1 {
                let b = bj as usize * self.bnx + bi as usize;
                self.refresh(b);
                m = m.max(self.bmax[b] as f64);
            }
        }
        m
    }

    /// Highest material along a straight move within `r` of the path.
    pub fn max_along(&mut self, a: (f64, f64), b: (f64, f64), r: f64) -> f64 {
        let l = (b.0 - a.0).hypot(b.1 - a.1);
        let step = (self.cell * B as f64 * 0.5).max(0.1);
        let n = (l / step).ceil().max(1.0) as usize;
        let mut m = f64::NEG_INFINITY;
        for k in 0..=n {
            let t = k as f64 / n as f64;
            m = m.max(self.max_near(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, r));
        }
        m
    }

    /// Exact: highest material at cells within `r` of (x, y).
    pub fn max_exact(&self, x: f64, y: f64, r: f64) -> f64 {
        let (ci, cj) = self.index(x, y);
        let n = (r / self.cell).ceil() as i64 + 1;
        let mut m = f64::NEG_INFINITY;
        for j in (cj - n).max(0)..=(cj + n).min(self.ny as i64 - 1) {
            for i in (ci - n).max(0)..=(ci + n).min(self.nx as i64 - 1) {
                let (cx, cy) = self.center(i as usize, j as usize);
                if (cx - x).hypot(cy - y) <= r {
                    m = m.max(self.h[j as usize * self.nx + i as usize] as f64);
                }
            }
        }
        m
    }

    /// Cuts the tool with its tip at (x, y, z): every cell whose centre lies strictly inside
    /// the cutting circle drops to the tool's surface there. `part` is the protected height
    /// of the finished part per cell (−∞ where there is none).
    pub fn stamp(&mut self, t: &Tool, x: f64, y: f64, z: f64, part: Option<&[f32]>) -> Cut {
        let r = t.radius();
        let (ci, cj) = self.index(x, y);
        let n = (r / self.cell).ceil() as i64 + 1;
        let mut cut = Cut { gouge: f32::INFINITY, ..Default::default() };
        for j in (cj - n).max(0)..=(cj + n).min(self.ny as i64 - 1) {
            for i in (ci - n).max(0)..=(ci + n).min(self.nx as i64 - 1) {
                let (cx, cy) = self.center(i as usize, j as usize);
                let d = (cx - x).hypot(cy - y);
                if d >= r - 1e-9 {
                    continue;
                }
                let Some(p) = t.profile(d) else { continue };
                let idx = j as usize * self.nx + i as usize;
                let new = (z + p) as f32;
                let old = self.h[idx];
                if new < old {
                    cut.depth = cut.depth.max(old - new);
                    cut.cells += 1;
                    self.h[idx] = new;
                    self.dirty[(j as usize / B) * self.bnx + i as usize / B] = true;
                    if let Some(part) = part {
                        let g = new - part[idx];
                        if g < cut.gouge {
                            cut.gouge = g;
                            cut.gouge_at = (i as usize, j as usize);
                        }
                    }
                }
            }
        }
        cut
    }

    /// Cuts everything the tool sweeps through on a straight move from `a` to `b` — exactly:
    /// for every cell the lowest the tool's surface gets over that cell during the move.
    /// (The tool's surface height over a fixed point is convex along a straight move, so
    /// the minimum is found by ternary search; flat moves and plunges in closed form.)
    pub fn sweep(&mut self, t: &Tool, a: glam::DVec3, b: glam::DVec3, part: Option<&[f32]>) -> Cut {
        self.sweep_mode(t, a, b, part, true)
    }

    /// Like `sweep`, without cutting: how deep the move would go into the part (`gouge`)
    /// and into the material (`depth`).
    pub fn probe(&mut self, t: &Tool, a: glam::DVec3, b: glam::DVec3, part: &[f32]) -> Cut {
        self.sweep_mode(t, a, b, Some(part), false)
    }

    fn sweep_mode(&mut self, t: &Tool, a: glam::DVec3, b: glam::DVec3, part: Option<&[f32]>, write: bool) -> Cut {
        let (vx, vy) = (b.x - a.x, b.y - a.y);
        let l2 = vx * vx + vy * vy;
        if l2 < 1e-18 && write {
            return self.stamp(t, b.x, b.y, a.z.min(b.z), part);
        }
        // a plunge probed as a tiny move
        let (b, vx, vy, l2) = if l2 < 1e-18 { (glam::DVec3::new(b.x + 1e-7, b.y, b.z), 1e-7, 0.0, 1e-14) } else { (b, vx, vy, l2) };
        let r = t.radius();
        let rr = r - 1e-9;
        let dz = b.z - a.z;
        let flat_tool = matches!(t.kind, super::tools::ToolKind::Flat);
        let mut cut = Cut { gouge: f32::INFINITY, ..Default::default() };
        let i0 = (((a.x.min(b.x) - r - self.x0) / self.cell - 0.5).floor() as i64).max(0);
        let i1 = (((a.x.max(b.x) + r - self.x0) / self.cell - 0.5).ceil() as i64).min(self.nx as i64 - 1);
        let j0 = (((a.y.min(b.y) - r - self.y0) / self.cell - 0.5).floor() as i64).max(0);
        let j1 = (((a.y.max(b.y) + r - self.y0) / self.cell - 0.5).ceil() as i64).min(self.ny as i64 - 1);
        if i0 > i1 || j0 > j1 {
            return cut;
        }
        for j in j0..=j1 {
            for i in i0..=i1 {
                let (cx, cy) = self.center(i as usize, j as usize);
                let (wx, wy) = (cx - a.x, cy - a.y);
                // parameters where the cell centre is inside the cutting circle
                let wv = wx * vx + wy * vy;
                let ww = wx * wx + wy * wy;
                let disc = wv * wv - l2 * (ww - rr * rr);
                if disc <= 0.0 {
                    continue;
                }
                let sq = disc.sqrt();
                let t0 = ((wv - sq) / l2).max(0.0);
                let t1 = ((wv + sq) / l2).min(1.0);
                if t0 > t1 {
                    continue;
                }
                let f = |s: f64| {
                    let (px, py) = (wx - s * vx, wy - s * vy);
                    let d = (px * px + py * py).sqrt();
                    a.z + dz * s + t.profile(d.min(r)).unwrap_or(0.0)
                };
                let idx = j as usize * self.nx + i as usize;
                // lower bound (lowest z of the move, closest approach): nothing to cut when
                // even that stays above the material
                let s_near = (wv / l2).clamp(t0, t1);
                let (nx_, ny_) = (wx - s_near * vx, wy - s_near * vy);
                let dmin = (nx_ * nx_ + ny_ * ny_).sqrt().min(r);
                let zlow = (a.z + dz * t0).min(a.z + dz * t1);
                if (zlow + t.profile(dmin).unwrap_or(0.0)) as f32 >= self.h[idx] {
                    continue;
                }
                let low = if dz.abs() < 1e-12 {
                    // level move: closest approach
                    let s = (wv / l2).clamp(t0, t1);
                    f(s)
                } else if flat_tool {
                    f(t0).min(f(t1))
                } else {
                    let (mut lo, mut hi) = (t0, t1);
                    for _ in 0..24 {
                        let m1 = lo + (hi - lo) / 3.0;
                        let m2 = hi - (hi - lo) / 3.0;
                        if f(m1) <= f(m2) {
                            hi = m2;
                        } else {
                            lo = m1;
                        }
                    }
                    f((lo + hi) / 2.0).min(f(t0)).min(f(t1))
                };
                let new = low as f32;
                let old = self.h[idx];
                if new < old {
                    cut.depth = cut.depth.max(old - new);
                    cut.cells += 1;
                    if write {
                        self.h[idx] = new;
                        self.dirty[(j as usize / B) * self.bnx + i as usize / B] = true;
                    }
                    if let Some(part) = part {
                        let g = new - part[idx];
                        if g < cut.gouge {
                            cut.gouge = g;
                            cut.gouge_at = (i as usize, j as usize);
                        }
                    }
                }
            }
        }
        cut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact sweep cuts at least as deep as dense stamping along the move, and never
    /// deeper than the tool can reach.
    #[test]
    fn sweep_matches_dense_stamping() {
        let moves = [
            (glam::DVec3::new(3.0, 4.0, -1.0), glam::DVec3::new(15.0, 9.0, -1.0)),
            (glam::DVec3::new(3.0, 15.0, 0.5), glam::DVec3::new(16.0, 4.0, -3.0)),
            (glam::DVec3::new(10.0, 10.0, 1.0), glam::DVec3::new(10.0, 10.0, -2.0)),
            (glam::DVec3::new(5.0, 5.0, -2.5), glam::DVec3::new(5.3, 5.1, -1.0)),
        ];
        for tool in [Tool::flat(1, 4.0, 2), Tool::ball(2, 5.0, 2), Tool::vbit(3, 6.0, 90.0)] {
            for (a, b) in moves {
                let mut s = Field::new(0.0, 0.0, 20.0, 20.0, -10.0, 0.0, 0.1);
                let mut d = s.clone();
                s.sweep(&tool, a, b, None);
                let n = 4000;
                for k in 0..=n {
                    let p = a + (b - a) * (k as f64 / n as f64);
                    d.stamp(&tool, p.x, p.y, p.z, None);
                }
                for k in 0..s.h.len() {
                    assert!(s.h[k] <= d.h[k] + 1e-4, "{:?} {:?}->{:?}: sweep {} stamps {}", tool.kind, a, b, s.h[k], d.h[k]);
                    assert!(s.h[k] >= d.h[k] - 0.01, "{:?}: sweep {} much deeper than {}", tool.kind, s.h[k], d.h[k]);
                }
            }
        }
    }

    #[test]
    fn stamping_and_queries() {
        let mut f = Field::new(0.0, 0.0, 20.0, 20.0, -10.0, 0.0, 0.1);
        let t = Tool::flat(1, 4.0, 2);
        let c = f.stamp(&t, 10.0, 10.0, -2.0, None);
        assert!(c.cells > 1000 && (c.depth - 2.0).abs() < 1e-6);
        assert!((f.at(10.0, 10.0).unwrap() + 2.0).abs() < 1e-6);
        assert!((f.at(13.0, 10.0).unwrap()).abs() < 1e-6);
        // the conservative maximum is never below the exact one, and sees the stock beside
        assert!(f.max_near(10.0, 10.0, 1.0) >= f.max_exact(10.0, 10.0, 1.0) - 1e-9);
        assert!(f.max_near(10.0, 10.0, 2.5) >= -1e-9);
        assert!((f.max_exact(10.0, 10.0, 1.5) + 2.0).abs() < 1e-6);
        let b = Tool::ball(2, 4.0, 2);
        f.stamp(&b, 5.0, 5.0, -1.0, None);
        assert!((f.at(5.0, 5.0).unwrap() + 1.0).abs() < 0.01);
    }
}
