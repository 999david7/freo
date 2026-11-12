//! Incompressible flow by the lattice Boltzmann method (D3Q19, BGK collision with a
//! Smagorinsky sub-grid model) on a uniform grid around or inside the part.

use super::fea::Ctl;
use super::mesh::{par_chunks, Grid};
use crate::kernel::math::*;

pub const Q: usize = 19;
pub const C: [[i32; 3]; Q] = [
    [0, 0, 0],
    [1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1],
    [1, 1, 0], [-1, -1, 0], [1, -1, 0], [-1, 1, 0],
    [1, 0, 1], [-1, 0, -1], [1, 0, -1], [-1, 0, 1],
    [0, 1, 1], [0, -1, -1], [0, 1, -1], [0, -1, 1],
];
pub const OPP: [usize; Q] = [0, 2, 1, 4, 3, 6, 5, 8, 7, 10, 9, 12, 11, 14, 13, 16, 15, 18, 17];
pub const W: [f32; Q] = [
    1.0 / 3.0,
    1.0 / 18.0, 1.0 / 18.0, 1.0 / 18.0, 1.0 / 18.0, 1.0 / 18.0, 1.0 / 18.0,
    1.0 / 36.0, 1.0 / 36.0, 1.0 / 36.0, 1.0 / 36.0, 1.0 / 36.0, 1.0 / 36.0, 1.0 / 36.0, 1.0 / 36.0, 1.0 / 36.0, 1.0 / 36.0, 1.0 / 36.0, 1.0 / 36.0,
];

pub const FLUID: u8 = 0;
pub const WALL: u8 = 1;
/// fixed velocity (inlet, far field)
pub const VELOCITY: u8 = 2;
/// outflow at ambient pressure
pub const OUTLET: u8 = 3;

fn feq(rho: f32, u: [f32; 3]) -> [f32; Q] {
    let uu = 1.5 * (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]);
    let mut f = [0.0; Q];
    for q in 0..Q {
        let cu = 3.0 * (C[q][0] as f32 * u[0] + C[q][1] as f32 * u[1] + C[q][2] as f32 * u[2]);
        f[q] = W[q] * rho * (1.0 + cu + 0.5 * cu * cu - uu);
    }
    f
}

pub struct Lbm {
    pub n: [usize; 3],
    pub kind: Vec<u8>,
    /// lattice velocity of VELOCITY cells
    pub bc_u: Vec<[f32; 3]>,
    /// OUTLET cells and the interior cell they copy from
    pub outlets: Vec<(u32, u32)>,
    pub f: Vec<f32>,
    f2: Vec<f32>,
    /// relaxation time of the molecular viscosity
    pub tau: f32,
    pub steps: usize,
}

impl Lbm {
    pub fn new(n: [usize; 3], kind: Vec<u8>, bc_u: Vec<[f32; 3]>, outlets: Vec<(u32, u32)>, tau: f32, u_init: [f32; 3]) -> Lbm {
        let cells = n[0] * n[1] * n[2];
        let mut f = vec![0.0f32; cells * Q];
        for c in 0..cells {
            let u = match kind[c] { VELOCITY => bc_u[c], FLUID | OUTLET => u_init, _ => [0.0; 3] };
            f[c * Q..(c + 1) * Q].copy_from_slice(&feq(1.0, u));
        }
        Lbm { n, kind, bc_u, outlets, f2: f.clone(), f, tau, steps: 0 }
    }

    pub fn step(&mut self) {
        let n = self.n;
        let (kind, f, tau0) = (&self.kind, &self.f, self.tau);
        let slab = n[0] * n[1] * Q;
        par_chunks(&mut self.f2, slab, |start, chunk| {
            let c0 = start / Q;
            for (ci, out) in chunk.chunks_mut(Q).enumerate() {
                let c = c0 + ci;
                if kind[c] != FLUID { continue; }
                let (i, j, k) = ((c % n[0]) as i32, ((c / n[0]) % n[1]) as i32, (c / (n[0] * n[1])) as i32);
                let mut fin = [0.0f32; Q];
                for q in 0..Q {
                    let (si, sj, sk) = (i - C[q][0], j - C[q][1], k - C[q][2]);
                    let inside = si >= 0 && sj >= 0 && sk >= 0 && (si as usize) < n[0] && (sj as usize) < n[1] && (sk as usize) < n[2];
                    let src = if inside { si as usize + n[0] * (sj as usize + n[1] * sk as usize) } else { usize::MAX };
                    // a wall sends the population back the way it came
                    fin[q] = if !inside || kind[src] == WALL { f[c * Q + OPP[q]] } else { f[src * Q + q] };
                }
                let mut rho = 0.0;
                let mut m = [0.0f32; 3];
                for q in 0..Q { rho += fin[q]; m[0] += fin[q] * C[q][0] as f32; m[1] += fin[q] * C[q][1] as f32; m[2] += fin[q] * C[q][2] as f32; }
                let u = [m[0] / rho, m[1] / rho, m[2] / rho];
                let eq = feq(rho, u);
                // Smagorinsky: eddy viscosity from the non-equilibrium stress
                let mut pi = [0.0f32; 6];
                for q in 0..Q {
                    let d = fin[q] - eq[q];
                    let (cx, cy, cz) = (C[q][0] as f32, C[q][1] as f32, C[q][2] as f32);
                    pi[0] += d * cx * cx; pi[1] += d * cy * cy; pi[2] += d * cz * cz;
                    pi[3] += d * cx * cy; pi[4] += d * cy * cz; pi[5] += d * cz * cx;
                }
                let pn = (pi[0] * pi[0] + pi[1] * pi[1] + pi[2] * pi[2] + 2.0 * (pi[3] * pi[3] + pi[4] * pi[4] + pi[5] * pi[5])).sqrt();
                const CS2: f32 = 0.17 * 0.17;
                let tau = 0.5 * (tau0 + (tau0 * tau0 + 18.0 * std::f32::consts::SQRT_2 * CS2 * pn / rho).sqrt());
                let om = 1.0 / tau;
                for q in 0..Q { out[q] = fin[q] - om * (fin[q] - eq[q]); }
            }
        });
        // outflow: ambient density with the velocity of the cell upstream
        for &(c, src) in &self.outlets {
            let (c, s) = (c as usize, src as usize);
            let fs = &self.f2[s * Q..(s + 1) * Q];
            let mut rho = 0.0;
            let mut m = [0.0f32; 3];
            for q in 0..Q { rho += fs[q]; m[0] += fs[q] * C[q][0] as f32; m[1] += fs[q] * C[q][1] as f32; m[2] += fs[q] * C[q][2] as f32; }
            let e = if self.kind[s] == FLUID && rho > 0.0 { feq(1.0, [m[0] / rho, m[1] / rho, m[2] / rho]) } else { feq(1.0, [0.0; 3]) };
            self.f2[c * Q..(c + 1) * Q].copy_from_slice(&e);
        }
        std::mem::swap(&mut self.f, &mut self.f2);
        self.steps += 1;
    }

    /// Density and velocity of every cell (lattice units).
    pub fn moments(&self) -> (Vec<f32>, Vec<[f32; 3]>) {
        let cells = self.kind.len();
        let mut rho = vec![1.0f32; cells];
        let mut u = vec![[0.0f32; 3]; cells];
        for c in 0..cells {
            if self.kind[c] == WALL { continue; }
            let f = &self.f[c * Q..(c + 1) * Q];
            let mut r = 0.0;
            let mut m = [0.0f32; 3];
            for q in 0..Q { r += f[q]; m[0] += f[q] * C[q][0] as f32; m[1] += f[q] * C[q][1] as f32; m[2] += f[q] * C[q][2] as f32; }
            rho[c] = r;
            if r > 0.0 { u[c] = [m[0] / r, m[1] / r, m[2] / r]; }
        }
        (rho, u)
    }

    /// Force of the fluid on the walls marked in `body` (momentum exchange, lattice units).
    pub fn wall_force(&self, body: &[bool]) -> V3 {
        let n = self.n;
        let mut force = V3::ZERO;
        for c in 0..self.kind.len() {
            if self.kind[c] != FLUID { continue; }
            let (i, j, k) = ((c % n[0]) as i32, ((c / n[0]) % n[1]) as i32, (c / (n[0] * n[1])) as i32);
            for q in 1..Q {
                let (si, sj, sk) = (i + C[q][0], j + C[q][1], k + C[q][2]);
                if si < 0 || sj < 0 || sk < 0 || si as usize >= n[0] || sj as usize >= n[1] || sk as usize >= n[2] { continue; }
                let w = si as usize + n[0] * (sj as usize + n[1] * sk as usize);
                if self.kind[w] == WALL && body[w] {
                    let v = 2.0 * self.f[c * Q + q] as f64;
                    force += V3::new(C[q][0] as f64, C[q][1] as f64, C[q][2] as f64) * v;
                }
            }
        }
        force
    }
}

/// A flow problem ready to run, with the factors that turn lattice units into SI.
pub struct FlowSetup {
    pub origin: V3,
    /// cell size mm
    pub dx: f64,
    pub lbm: Lbm,
    /// cells of the part itself (walls added around a cavity are not part of it)
    pub body: Vec<bool>,
    /// m/s per lattice velocity unit
    pub u_scale: f64,
    /// fluid density kg/m³
    pub rho: f64,
    pub total_steps: usize,
    pub internal: bool,
    /// start points of the stream lines (mm)
    pub seeds: Vec<V3>,
    /// cells just inside each inlet, for the pressure drop
    pub inlet_cells: Vec<u32>,
    pub reynolds: f64,
}

pub struct FlowField {
    pub origin: V3,
    pub dx: f64,
    pub n: [usize; 3],
    pub kind: Vec<u8>,
    /// m/s
    pub u: Vec<[f32; 3]>,
    /// Pa relative to ambient
    pub p: Vec<f32>,
    /// N on the part
    pub force: V3,
    pub internal: bool,
    pub seeds: Vec<V3>,
    pub dp: f64,
    pub done: f32,
}

impl FlowField {
    fn idx(&self, p: V3) -> Option<usize> {
        let r = (p - self.origin) / self.dx;
        if r.x < 0.0 || r.y < 0.0 || r.z < 0.0 { return None; }
        let c = [r.x as usize, r.y as usize, r.z as usize];
        (c[0] < self.n[0] && c[1] < self.n[1] && c[2] < self.n[2]).then(|| c[0] + self.n[0] * (c[1] + self.n[1] * c[2]))
    }
    /// Velocity at `p` (trilinear between cell centres; zero inside walls).
    pub fn velocity(&self, p: V3) -> Option<V3> {
        let r = (p - self.origin) / self.dx - V3::splat(0.5);
        let b = [r.x.floor(), r.y.floor(), r.z.floor()];
        if b[0] < 0.0 || b[1] < 0.0 || b[2] < 0.0 { return None; }
        let (i, j, k) = (b[0] as usize, b[1] as usize, b[2] as usize);
        if i + 1 >= self.n[0] || j + 1 >= self.n[1] || k + 1 >= self.n[2] { return None; }
        let t = [r.x - b[0], r.y - b[1], r.z - b[2]];
        let mut v = V3::ZERO;
        for a in 0..8 {
            let c = (i + (a & 1)) + self.n[0] * ((j + (a >> 1 & 1)) + self.n[1] * (k + (a >> 2 & 1)));
            let w = (if a & 1 == 1 { t[0] } else { 1.0 - t[0] }) * (if a >> 1 & 1 == 1 { t[1] } else { 1.0 - t[1] }) * (if a >> 2 & 1 == 1 { t[2] } else { 1.0 - t[2] });
            if self.kind[c] != WALL { let u = self.u[c]; v += V3::new(u[0] as f64, u[1] as f64, u[2] as f64) * w; }
        }
        Some(v)
    }
    /// Pressure and speed in the fluid next to surface point `p` with outward normal `n`
    /// (for a cavity the fluid lies on the same side: the wall normal points into it).
    pub fn near_wall(&self, p: V3, n: V3) -> Option<(f32, f32)> {
        for s in [0.8, 1.5, 2.3, 3.2] {
            if let Some(c) = self.idx(p + n * (self.dx * s)) {
                if self.kind[c] == FLUID { let u = self.u[c]; return Some((self.p[c], (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt())); }
            }
        }
        None
    }
    /// Stream lines from the seed points: polylines of (position mm, speed m/s).
    pub fn streamlines(&self) -> Vec<Vec<(V3, f32)>> {
        let mut out = vec![];
        let step = self.dx * 0.5;
        let vmax = self.u.iter().map(|u| u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).fold(0.0f32, f32::max).sqrt() as f64;
        if vmax <= 0.0 { return out; }
        for &seed in &self.seeds {
            let mut p = seed;
            let mut line = vec![];
            for _ in 0..(self.n[0] + self.n[1] + self.n[2]) * 6 {
                let Some(v1) = self.velocity(p) else { break };
                let s1 = v1.length();
                if s1 < vmax * 2e-3 { break; }
                line.push((p, s1 as f32));
                let Some(v2) = self.velocity(p + v1 / s1 * (step * 0.5)) else { break };
                let s2 = v2.length();
                if s2 < vmax * 2e-3 { break; }
                p += v2 / s2 * step;
            }
            if line.len() > 3 { out.push(line); }
        }
        out
    }
}

/// Runs the flow; `snapshot` receives the field a few times on the way and at the end.
pub fn run(setup: &mut FlowSetup, ctl: &Ctl, snapshot: &mut dyn FnMut(FlowField)) -> Result<(), String> {
    let total = setup.total_steps;
    let avg_from = total * 2 / 3;
    let cells = setup.lbm.kind.len();
    let mut sum_u = vec![[0.0f32; 3]; cells];
    let mut sum_r = vec![0.0f32; cells];
    let mut sum_f = V3::ZERO;
    let mut n_avg = 0u32;
    let every = (total / 8).max(1);
    let field = |s: &FlowSetup, rho: &[f32], u: &[[f32; 3]], force: V3, done: f32| {
        let us = s.u_scale as f32;
        // p = ρ c_s² (ρ_lat − 1) in lattice units, scaled by the velocity scale squared
        let ps = (s.rho * s.u_scale * s.u_scale / 3.0) as f32;
        let p: Vec<f32> = rho.iter().map(|r| (r - 1.0) * ps).collect();
        let dp = if s.inlet_cells.is_empty() { 0.0 } else { s.inlet_cells.iter().map(|&c| p[c as usize] as f64).sum::<f64>() / s.inlet_cells.len() as f64 };
        let m = s.dx * 1e-3;
        FlowField {
            origin: s.origin, dx: s.dx, n: s.lbm.n, kind: s.lbm.kind.clone(),
            u: u.iter().map(|v| [v[0] * us, v[1] * us, v[2] * us]).collect(), p,
            force: force * (s.rho * m * m * s.u_scale * s.u_scale), internal: s.internal, seeds: s.seeds.clone(), dp, done,
        }
    };
    for step in 0..total {
        if ctl.cancelled() { return Err("abgebrochen".into()); }
        setup.lbm.step();
        if step % 16 == 0 {
            ctl.set(0.05 + 0.93 * step as f32 / total as f32);
            // a diverging run shows up as NaN in the first fluid cell it reaches
            if setup.lbm.f.iter().step_by(Q * 97 + 1).any(|v| !v.is_finite()) { return Err("Die Strömungsberechnung ist instabil geworden. Geschwindigkeit verringern oder die Genauigkeit erhöhen.".into()); }
        }
        if step >= avg_from && step % 4 == 0 {
            let (rho, u) = setup.lbm.moments();
            for c in 0..cells { sum_r[c] += rho[c]; for k in 0..3 { sum_u[c][k] += u[c][k]; } }
            sum_f += setup.lbm.wall_force(&setup.body);
            n_avg += 1;
        }
        if step % every == every - 1 && step + 1 < total {
            let (rho, u) = setup.lbm.moments();
            let force = setup.lbm.wall_force(&setup.body);
            snapshot(field(setup, &rho, &u, force, (step + 1) as f32 / total as f32));
        }
    }
    let k = 1.0 / n_avg.max(1) as f32;
    let rho: Vec<f32> = sum_r.iter().map(|r| r * k).collect();
    let u: Vec<[f32; 3]> = sum_u.iter().map(|v| [v[0] * k, v[1] * k, v[2] * k]).collect();
    snapshot(field(setup, &rho, &u, sum_f / n_avg.max(1) as f64, 1.0));
    Ok(())
}

/// Lattice speed of the reference velocity: low enough to stay incompressible.
const U_LAT: f64 = 0.06;

fn tau_for(u_ref: f64, len_mm: f64, dx: f64, nu: f64) -> (f32, f64) {
    let re = u_ref * len_mm * 1e-3 / nu;
    let nu_lat = U_LAT * (len_mm / dx) / re.max(1e-9);
    // under-resolved (high Reynolds) flows are held stable by the sub-grid model
    ((0.5 + 3.0 * nu_lat).clamp(0.5025, 1.9) as f32, re)
}

/// Wind tunnel around the part: `dir` is the flow direction, `speed` in m/s.
pub fn external(tris: &[[V3; 3]], bbox: &BBox, dir: V3, speed: f64, fl: &crate::model::sim::Fluid, cells: usize) -> Result<FlowSetup, String> {
    let d = dir.normalize_or_zero();
    if d == V3::ZERO || speed <= 0.0 { return Err("Anströmung: Richtung und Geschwindigkeit müssen ungleich null sein.".into()); }
    let size = bbox.size();
    let l = size.max_element();
    let (mut lo, mut hi) = (bbox.min, bbox.max);
    for a in 0..3 {
        let (up, down) = (l * (0.7 + 0.5 * d[a].abs()), l * (0.7 + 1.8 * d[a].abs()));
        if d[a] >= 0.0 { lo[a] -= up; hi[a] += down; } else { lo[a] -= down; hi[a] += up; }
    }
    let ext = hi - lo;
    let dx = (ext.x * ext.y * ext.z / cells as f64).cbrt();
    let n = [(ext.x / dx).ceil() as usize, (ext.y / dx).ceil() as usize, (ext.z / dx).ceil() as usize];
    let g = Grid::voxelize(tris, lo, V3::splat(dx), n);
    if !g.solid.iter().any(|s| *s) { return Err("Das Modell ist für das Strömungsgitter zu klein.".into()); }
    let total = g.cells();
    let mut kind: Vec<u8> = g.solid.iter().map(|s| if *s { WALL } else { FLUID }).collect();
    let ul = [(d.x * U_LAT) as f32, (d.y * U_LAT) as f32, (d.z * U_LAT) as f32];
    let mut bc_u = vec![[0.0f32; 3]; total];
    let mut outlets = vec![];
    for k in 0..n[2] {
        for j in 0..n[1] {
            for i in 0..n[0] {
                let idx = [i, j, k];
                // inflow faces hold the free-stream velocity; all other faces are open
                // (ambient pressure, velocity taken from inside), so the flow can give way
                let mut open_axis = None;
                let mut inflow = false;
                for a in 0..3 {
                    let side = if idx[a] == 0 { -1.0 } else if idx[a] == n[a] - 1 { 1.0 } else { continue };
                    if d[a] * side < -0.3 { inflow = true; } else if open_axis.is_none() { open_axis = Some((a, side)); }
                }
                let c = g.cell(i, j, k);
                match (inflow, open_axis) {
                    (true, _) => { kind[c] = VELOCITY; bc_u[c] = ul; }
                    (false, Some((a, side))) => {
                        let mut s = idx;
                        s[a] = (idx[a] as f64 - side) as usize;
                        kind[c] = OUTLET;
                        outlets.push((c as u32, g.cell(s[0], s[1], s[2]) as u32));
                    }
                    _ => {}
                }
            }
        }
    }
    let (tau, re) = tau_for(speed, l, dx, fl.nu);
    let along = (0..3).map(|a| ext[a] * d[a].abs()).sum::<f64>() / dx;
    // stream-line seeds on a lattice upstream of the part
    let (t1, t2) = { let f = Frame::from_normal(V3::ZERO, d); (f.x, f.y) };
    let half = |t: V3| 0.5 * (size.x * t.x.abs() + size.y * t.y.abs() + size.z * t.z.abs());
    let start = bbox.center() - d * (half(d) + 0.45 * l);
    let mut seeds = vec![];
    let (na, nb) = (11, 9);
    for a in 0..na {
        for b in 0..nb {
            let (fa, fb) = (a as f64 / (na - 1) as f64 - 0.5, b as f64 / (nb - 1) as f64 - 0.5);
            seeds.push(start + t1 * (fa * 2.0 * (half(t1) + 0.12 * l) * 1.15) + t2 * (fb * 2.0 * (half(t2) + 0.12 * l) * 1.15));
        }
    }
    let body = g.solid.clone();
    Ok(FlowSetup {
        origin: lo, dx, lbm: Lbm::new(n, kind, bc_u, outlets, tau, ul), body, u_scale: speed / U_LAT, rho: fl.rho,
        total_steps: ((2.6 * along / U_LAT) as usize).clamp(600, 12_000), internal: false, seeds, inlet_cells: vec![], reynolds: re,
    })
}

/// An opening of a cavity: the plane of the picked end face and the box around it.
pub struct Opening { pub point: V3, pub normal: V3, pub bbox: BBox, pub inlet: bool, /** m/s */ pub speed: f64 }

/// Flow through the cavity of the part, closed by the picked inlet and outlet faces.
pub fn internal(tris: &[[V3; 3]], bbox: &BBox, openings: &[Opening], fl: &crate::model::sim::Fluid, cells: usize) -> Result<FlowSetup, String> {
    let u_ref = openings.iter().filter(|o| o.inlet).map(|o| o.speed).fold(0.0, f64::max);
    if u_ref <= 0.0 { return Err("Einlass: die Geschwindigkeit muss größer als null sein.".into()); }
    if !openings.iter().any(|o| !o.inlet) { return Err("Für eine interne Strömung wird mindestens ein Auslass benötigt.".into()); }
    let size = bbox.size();
    let dx = (size.x * size.y * size.z / cells as f64).cbrt();
    let pad = 4.0 * dx;
    let lo = bbox.min - V3::splat(pad);
    let ext = size + V3::splat(2.0 * pad);
    let n = [(ext.x / dx).ceil() as usize, (ext.y / dx).ceil() as usize, (ext.z / dx).ceil() as usize];
    let g = Grid::voxelize(tris, lo, V3::splat(dx), n);
    let total = g.cells();
    const CAP: u8 = 100;
    let mut kind: Vec<u8> = g.solid.iter().map(|s| if *s { WALL } else { FLUID }).collect();
    let mut cap_of = vec![u8::MAX; total];
    for (oi, o) in openings.iter().enumerate() {
        for k in 0..n[2] { for j in 0..n[1] { for i in 0..n[0] {
            let c = g.cell(i, j, k);
            if g.solid[c] { continue; }
            let q = g.center(i, j, k);
            let hgt = (q - o.point).dot(o.normal);
            if hgt <= 0.0 || hgt > 1.6 * dx { continue; }
            let foot = q - o.normal * hgt;
            if (0..3).all(|a| foot[a] >= o.bbox.min[a] - 0.6 * dx && foot[a] <= o.bbox.max[a] + 0.6 * dx) { kind[c] = CAP; cap_of[c] = oi as u8; }
        } } }
    }
    // everything reachable from outside without crossing a cap is not part of the cavity
    let mut outside = vec![false; total];
    let mut stack = vec![0usize];
    outside[0] = true;
    while let Some(c) = stack.pop() {
        let (i, j, k) = (c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1]));
        let mut visit = |q: usize| if kind[q] == FLUID && !outside[q] { outside[q] = true; stack.push(q); };
        if i > 0 { visit(c - 1); }
        if i + 1 < n[0] { visit(c + 1); }
        if j > 0 { visit(c - n[0]); }
        if j + 1 < n[1] { visit(c + n[0]); }
        if k > 0 { visit(c - n[0] * n[1]); }
        if k + 1 < n[2] { visit(c + n[0] * n[1]); }
    }
    let nfluid = (0..total).filter(|&c| kind[c] == FLUID && !outside[c]).count();
    if nfluid < 8 {
        return Err("Kein geschlossenes Fluidvolumen gefunden: jede Öffnung des Hohlraums braucht einen Einlass oder Auslass (die ebene Stirnfläche um die Öffnung wählen).".into());
    }
    for c in 0..total { if outside[c] { kind[c] = WALL; } }
    let mut bc_u = vec![[0.0f32; 3]; total];
    let mut outlets = vec![];
    let mut inlet_cells = vec![];
    let mut seeds = vec![];
    let mut found = vec![false; openings.len()];
    for k in 0..n[2] { for j in 0..n[1] { for i in 0..n[0] {
        let c = g.cell(i, j, k);
        if kind[c] != CAP { continue; }
        let o = &openings[cap_of[c] as usize];
        // the cavity cell straight behind the cap
        let back = g.cell_at(g.center(i, j, k) - o.normal * dx).map(|b| g.cell(b[0], b[1], b[2])).filter(|&b| kind[b] == FLUID || kind[b] == CAP);
        let touches = [(1i64, 0i64, 0i64), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)].iter().any(|d| {
            let (a, b, e) = (i as i64 + d.0, j as i64 + d.1, k as i64 + d.2);
            a >= 0 && b >= 0 && e >= 0 && (a as usize) < n[0] && (b as usize) < n[1] && (e as usize) < n[2] && kind[g.cell(a as usize, b as usize, e as usize)] == FLUID
        });
        let Some(back) = back else { kind[c] = WALL; continue };
        found[cap_of[c] as usize] |= touches;
        if o.inlet {
            kind[c] = VELOCITY;
            let v = -o.normal * (U_LAT * o.speed / u_ref);
            bc_u[c] = [v.x as f32, v.y as f32, v.z as f32];
            if touches && kind[back] == FLUID { inlet_cells.push(back as u32); seeds.push(g.center(i, j, k) - o.normal * (dx * 1.2)); }
        } else {
            kind[c] = OUTLET;
            outlets.push((c as u32, back as u32));
        }
    } } }
    if let Some(i) = found.iter().position(|f| !*f) {
        return Err(format!("{} liegt an keiner Öffnung des Hohlraums.", if openings[i].inlet { "Der Einlass" } else { "Ein Auslass" }));
    }
    // outlets were assigned while caps were still marked: resolve their final kinds
    for c in 0..total { if kind[c] == CAP { kind[c] = WALL; } }
    if seeds.len() > 70 { let st = seeds.len().div_ceil(70); seeds = seeds.into_iter().step_by(st).collect(); }
    let width = openings.iter().filter(|o| o.inlet).map(|o| o.bbox.diagonal() / 1.6).fold(0.0, f64::max).max(dx);
    let (tau, re) = tau_for(u_ref, width, dx, fl.nu);
    let path = (n[0] + n[1] + n[2]) as f64;
    let body = g.solid.clone();
    Ok(FlowSetup {
        origin: lo, dx, lbm: Lbm::new(n, kind, bc_u, outlets, tau, [0.0; 3]), body, u_scale: u_ref / U_LAT, rho: fl.rho,
        total_steps: ((3.0 * path / U_LAT) as usize).clamp(800, 14_000), internal: true, seeds, inlet_cells, reynolds: re,
    })
}
