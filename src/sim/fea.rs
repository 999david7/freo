//! Finite elements on the voxel mesh: linear statics, steady heat conduction and modal
//! analysis. All elements are the same box, so one element matrix serves the whole mesh and
//! the system matrix is never assembled (matrix-free conjugate gradients).

use super::mesh::{par_chunks, Mesh, NONE};
use crate::kernel::math::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Cancellation and progress shared with the caller's thread.
pub struct Ctl<'a> { pub cancel: &'a AtomicBool, pub progress: &'a AtomicU32 }

impl Ctl<'_> {
    pub fn set(&self, f: f32) { self.progress.store(f.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed); }
    pub fn cancelled(&self) -> bool { self.cancel.load(Ordering::Relaxed) }
}

const GP: f64 = 0.577_350_269_189_625_8;

/// dN_a/dx of the 8 shape functions at natural coordinates `xi`.
fn shape_grad(xi: [f64; 3], h: V3) -> [[f64; 3]; 8] {
    let mut g = [[0.0; 3]; 8];
    for a in 0..8 {
        let s = [if a & 1 == 1 { 1.0 } else { -1.0 }, if a >> 1 & 1 == 1 { 1.0 } else { -1.0 }, if a >> 2 & 1 == 1 { 1.0 } else { -1.0 }];
        let f = [1.0 + s[0] * xi[0], 1.0 + s[1] * xi[1], 1.0 + s[2] * xi[2]];
        g[a] = [s[0] * f[1] * f[2] / 8.0 * 2.0 / h.x, s[1] * f[0] * f[2] / 8.0 * 2.0 / h.y, s[2] * f[0] * f[1] / 8.0 * 2.0 / h.z];
    }
    g
}

/// Strain (xx, yy, zz, xy, yz, zx) of displacement component `c` with gradient `g`.
fn strain_col(g: [f64; 3], c: usize) -> [f64; 6] {
    match c { 0 => [g[0], 0.0, 0.0, g[1], 0.0, g[2]], 1 => [0.0, g[1], 0.0, g[0], g[2], 0.0], _ => [0.0, 0.0, g[2], 0.0, g[1], g[0]] }
}

fn b_matrix(xi: [f64; 3], h: V3) -> [[f64; 24]; 6] {
    let g = shape_grad(xi, h);
    let mut b = [[0.0; 24]; 6];
    for a in 0..8 { for c in 0..3 { let col = strain_col(g[a], c); for r in 0..6 { b[r][3 * a + c] = col[r]; } } }
    b
}

/// Strains of the nine incompatible modes (1 − ξ², 1 − η², 1 − ζ² per displacement component):
/// they let the box element bend without shear locking.
fn g_matrix(xi: [f64; 3], h: V3) -> [[f64; 9]; 6] {
    let mut g = [[0.0; 9]; 6];
    for m in 0..3 {
        let mut grad = [0.0; 3];
        grad[m] = -2.0 * xi[m] * 2.0 / h[m];
        for c in 0..3 { let col = strain_col(grad, c); for r in 0..6 { g[r][3 * m + c] = col[r]; } }
    }
    g
}

pub fn elasticity(e: f64, nu: f64) -> [[f64; 6]; 6] {
    let (lam, mu) = (e * nu / ((1.0 + nu) * (1.0 - 2.0 * nu)), e / (2.0 * (1.0 + nu)));
    let mut d = [[0.0; 6]; 6];
    for i in 0..3 { for j in 0..3 { d[i][j] = lam; } d[i][i] = lam + 2.0 * mu; d[i + 3][i + 3] = mu; }
    d
}

pub struct ElemStruct {
    /// 24×24 stiffness, row-major
    pub ke: Vec<f64>,
    /// stress at each corner from the element displacements (6×24 each)
    pub stress: Vec<[[f64; 24]; 6]>,
    /// nodal forces of a unit thermal strain
    pub fth: [f64; 24],
    pub d: [[f64; 6]; 6],
}

pub fn elem_struct(h: V3, e: f64, nu: f64) -> ElemStruct {
    let d = elasticity(e, nu);
    let w = h.x * h.y * h.z / 8.0;
    let mut kuu = vec![0.0; 576];
    let mut kua = vec![0.0; 24 * 9];
    let mut kaa = vec![0.0; 81];
    let mut fth = [0.0; 24];
    for gp in 0..8 {
        let xi = [if gp & 1 == 1 { GP } else { -GP }, if gp >> 1 & 1 == 1 { GP } else { -GP }, if gp >> 2 & 1 == 1 { GP } else { -GP }];
        let (b, g) = (b_matrix(xi, h), g_matrix(xi, h));
        let mut db = [[0.0; 24]; 6];
        let mut dg = [[0.0; 9]; 6];
        for r in 0..6 { for s in 0..6 { if d[r][s] != 0.0 { for c in 0..24 { db[r][c] += d[r][s] * b[s][c]; } for c in 0..9 { dg[r][c] += d[r][s] * g[s][c]; } } } }
        for i in 0..24 {
            for r in 0..6 {
                if b[r][i] == 0.0 { continue; }
                for j in 0..24 { kuu[i * 24 + j] += b[r][i] * db[r][j] * w; }
                for j in 0..9 { kua[i * 9 + j] += b[r][i] * dg[r][j] * w; }
            }
            fth[i] += (db[0][i] + db[1][i] + db[2][i]) * w;
        }
        for i in 0..9 { for r in 0..6 { for j in 0..9 { kaa[i * 9 + j] += g[r][i] * dg[r][j] * w; } } }
    }
    // static condensation of the incompatible modes: A = −Kaa⁻¹ Kau
    let mut a = vec![0.0; 9 * 24];
    for j in 0..24 {
        let rhs: Vec<f64> = (0..9).map(|i| -kua[j * 9 + i]).collect();
        let x = solve_dense(&kaa, &rhs, 9).unwrap_or(vec![0.0; 9]);
        for i in 0..9 { a[i * 24 + j] = x[i]; }
    }
    let mut ke = kuu;
    for i in 0..24 { for j in 0..24 { let mut s = 0.0; for m in 0..9 { s += kua[i * 9 + m] * a[m * 24 + j]; } ke[i * 24 + j] += s; } }
    for i in 0..24 { for j in 0..i { let m = 0.5 * (ke[i * 24 + j] + ke[j * 24 + i]); ke[i * 24 + j] = m; ke[j * 24 + i] = m; } }
    let mut stress = vec![];
    for cn in 0..8 {
        let xi = [if cn & 1 == 1 { 1.0 } else { -1.0 }, if cn >> 1 & 1 == 1 { 1.0 } else { -1.0 }, if cn >> 2 & 1 == 1 { 1.0 } else { -1.0 }];
        let (b, g) = (b_matrix(xi, h), g_matrix(xi, h));
        let mut eps = b;
        for r in 0..6 { for j in 0..24 { for m in 0..9 { eps[r][j] += g[r][m] * a[m * 24 + j]; } } }
        let mut s = [[0.0; 24]; 6];
        for r in 0..6 { for q in 0..6 { if d[r][q] != 0.0 { for j in 0..24 { s[r][j] += d[r][q] * eps[q][j]; } } } }
        stress.push(s);
    }
    ElemStruct { ke, stress, fth, d }
}

/// 8×8 conduction matrix and the temperature gradient at the element centre.
pub fn elem_thermal(h: V3, k: f64) -> (Vec<f64>, [[f64; 8]; 3]) {
    let w = h.x * h.y * h.z / 8.0;
    let mut ke = vec![0.0; 64];
    for gp in 0..8 {
        let xi = [if gp & 1 == 1 { GP } else { -GP }, if gp >> 1 & 1 == 1 { GP } else { -GP }, if gp >> 2 & 1 == 1 { GP } else { -GP }];
        let g = shape_grad(xi, h);
        for i in 0..8 { for j in 0..8 { ke[i * 8 + j] += k * (g[i][0] * g[j][0] + g[i][1] * g[j][1] + g[i][2] * g[j][2]) * w; } }
    }
    let g0 = shape_grad([0.0; 3], h);
    let mut grad = [[0.0; 8]; 3];
    for a in 0..8 { for c in 0..3 { grad[c][a] = g0[a][c]; } }
    (ke, grad)
}

/// The system matrix as an operator: K restricted to the free degrees of freedom, identity on
/// the others, plus per-node additions (penalty supports, convection).
pub struct Op<'a> {
    pub mesh: &'a Mesh,
    /// degrees of freedom per node (3 mechanics, 1 heat)
    pub d: usize,
    pub ke: &'a [f64],
    pub free: &'a [bool],
    /// node, symmetric d×d block added to the diagonal (row-major 3×3; heat uses [0])
    pub extra: &'a [(u32, [f64; 9])],
}

impl Op<'_> {
    pub fn apply(&self, x: &[f64], y: &mut [f64], fe: &mut Vec<f64>) {
        let (d, m) = (self.d, self.mesh);
        let ed = 8 * d;
        fe.resize(m.elems.len() * ed, 0.0);
        let (ke, free) = (self.ke, self.free);
        par_chunks(fe, ed, |start, chunk| {
            let mut u = [0.0f64; 24];
            for (ei, out) in chunk.chunks_mut(ed).enumerate() {
                let en = &m.elems[start / ed + ei];
                for a in 0..8 { let b = en[a] as usize * d; for c in 0..d { u[a * d + c] = if free[b + c] { x[b + c] } else { 0.0 }; } }
                for (i, o) in out.iter_mut().enumerate() {
                    let row = &ke[i * ed..(i + 1) * ed];
                    let mut s = 0.0;
                    for j in 0..ed { s += row[j] * u[j]; }
                    *o = s;
                }
            }
        });
        let fe = &*fe;
        par_chunks(y, d, |start, chunk| {
            for (ni, out) in chunk.chunks_mut(d).enumerate() {
                let n = start / d + ni;
                let ne = &m.node_elems[n];
                for c in 0..d {
                    if !free[n * d + c] { out[c] = x[n * d + c]; continue; }
                    let mut s = 0.0;
                    for a in 0..8 { if ne[a] != NONE { s += fe[ne[a] as usize * ed + a * d + c]; } }
                    out[c] = s;
                }
            }
        });
        for (n, blk) in self.extra {
            let b = *n as usize * d;
            for r in 0..d { if !self.free[b + r] { continue; } for c in 0..d { if self.free[b + c] { y[b + r] += blk[r * 3 + c] * x[b + c]; } } }
        }
    }

    /// Inverse of the diagonal d×d blocks (block-Jacobi preconditioner).
    pub fn precond(&self) -> Vec<[f64; 9]> {
        let (d, m) = (self.d, self.mesh);
        let ed = 8 * d;
        let mut blocks = vec![[0.0f64; 9]; m.nodes.len()];
        for (n, ne) in m.node_elems.iter().enumerate() {
            for a in 0..8 {
                if ne[a] == NONE { continue; }
                for r in 0..d { for c in 0..d { blocks[n][r * 3 + c] += self.ke[(a * d + r) * ed + a * d + c]; } }
            }
        }
        for (n, blk) in self.extra { for i in 0..9 { blocks[*n as usize][i] += blk[i]; } }
        for (n, b) in blocks.iter_mut().enumerate() {
            for r in 0..3 {
                if r >= d || !self.free[n * d + r] { for c in 0..3 { b[r * 3 + c] = 0.0; b[c * 3 + r] = 0.0; } b[r * 3 + r] = 1.0; }
            }
            let mat = glam::DMat3::from_cols_array(b);
            let det = mat.determinant();
            *b = if det.abs() > 1e-300 { mat.inverse().to_cols_array() } else { glam::DMat3::IDENTITY.to_cols_array() };
        }
        blocks
    }
}

fn apply_prec(inv: &[[f64; 9]], d: usize, r: &[f64], z: &mut [f64]) {
    if d == 1 { for i in 0..r.len() { z[i] = inv[i][0] * r[i]; } return; }
    for (n, b) in inv.iter().enumerate() {
        let (x, y, w) = (r[3 * n], r[3 * n + 1], r[3 * n + 2]);
        // symmetric block: row- and column-major agree
        z[3 * n] = b[0] * x + b[1] * y + b[2] * w;
        z[3 * n + 1] = b[3] * x + b[4] * y + b[5] * w;
        z[3 * n + 2] = b[6] * x + b[7] * y + b[8] * w;
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 { a.iter().zip(b).map(|(x, y)| x * y).sum() }

/// Preconditioned conjugate gradients; `x` holds the start vector and the solution.
pub fn pcg(op: &Op, b: &[f64], x: &mut [f64], tol: f64, max_it: usize, ctl: &Ctl, span: (f32, f32)) -> Result<(usize, f64), String> {
    let n = b.len();
    let inv = op.precond();
    let mut fe = vec![];
    let mut r = vec![0.0; n];
    op.apply(x, &mut r, &mut fe);
    for i in 0..n { r[i] = b[i] - r[i]; }
    let bnorm = dot(b, b).sqrt().max(1e-300);
    let mut z = vec![0.0; n];
    apply_prec(&inv, op.d, &r, &mut z);
    let mut p = z.clone();
    let mut ap = vec![0.0; n];
    let mut rz = dot(&r, &z);
    let mut res = dot(&r, &r).sqrt() / bnorm;
    let res0 = res.max(1e-300);
    let mut it = 0;
    while res > tol && it < max_it {
        if ctl.cancelled() { return Err("abgebrochen".into()); }
        op.apply(&p, &mut ap, &mut fe);
        let pap = dot(&p, &ap);
        if !(pap > 0.0) || !pap.is_finite() { break; }
        let alpha = rz / pap;
        for i in 0..n { x[i] += alpha * p[i]; r[i] -= alpha * ap[i]; }
        apply_prec(&inv, op.d, &r, &mut z);
        let rz_new = dot(&r, &z);
        let beta = rz_new / rz;
        rz = rz_new;
        for i in 0..n { p[i] = z[i] + beta * p[i]; }
        res = dot(&r, &r).sqrt() / bnorm;
        it += 1;
        if it % 8 == 0 {
            let f = ((res0 / res.max(1e-300)).ln() / (res0 / tol).ln().max(1e-9)).clamp(0.0, 1.0) as f32;
            ctl.set(span.0 + (span.1 - span.0) * f);
        }
    }
    if !res.is_finite() { return Err("Die Lösung divergiert.".into()); }
    Ok((it, res))
}

// ---------------------------------------------------------------- statics

#[derive(Default)]
pub struct StructIn {
    pub e: f64,
    pub nu: f64,
    /// t/mm³
    pub rho: f64,
    pub alpha: f64,
    /// node and the prescribed displacement per component (None = free)
    pub fixed: Vec<(u32, [Option<f64>; 3])>,
    /// node held against moving along a direction (frictionless support, symmetry)
    pub slide: Vec<(u32, V3)>,
    pub forces: Vec<(u32, V3)>,
    /// uniform acceleration load: force per mass (mm/s²)
    pub accel: V3,
    /// rotation: (point on axis, axis direction, ω rad/s)
    pub spin: Option<(V3, V3, f64)>,
    /// temperature rise per node over the stress-free state
    pub dtemp: Option<Vec<f64>>,
}

pub struct StructOut {
    pub u: Vec<V3>,
    /// xx, yy, zz, xy, yz, zx per node
    pub stress: Vec<[f64; 6]>,
    pub iters: usize,
    pub resid: f64,
    pub applied: V3,
    pub reaction: V3,
}

/// Free-dof mask, prescribed values and penalty blocks of the supports.
fn supports(mesh: &Mesh, inp: &StructIn, kref: f64) -> (Vec<bool>, Vec<f64>, Vec<(u32, [f64; 9])>) {
    let nn = mesh.nodes.len();
    let mut free = vec![true; 3 * nn];
    let mut u0 = vec![0.0; 3 * nn];
    for (n, v) in &inp.fixed {
        for c in 0..3 { if let Some(val) = v[c] { free[*n as usize * 3 + c] = false; u0[*n as usize * 3 + c] = val; } }
    }
    let mut extra = vec![];
    for (n, dir) in &inp.slide {
        let d = dir.normalize_or_zero();
        if d == V3::ZERO { continue; }
        let a = d.abs();
        // along a global axis the degree of freedom is simply removed
        if let Some(c) = (0..3).find(|&c| a[c] > 0.9999) { free[*n as usize * 3 + c] = false; u0[*n as usize * 3 + c] = 0.0; continue; }
        let k = kref * 1.0e4;
        let mut blk = [0.0; 9];
        for r in 0..3 { for c in 0..3 { blk[r * 3 + c] = k * d[r] * d[c]; } }
        extra.push((*n, blk));
    }
    (free, u0, extra)
}

pub fn solve_struct(mesh: &Mesh, inp: &StructIn, ctl: &Ctl) -> Result<StructOut, String> {
    let nn = mesh.nodes.len();
    let el = elem_struct(mesh.grid.h, inp.e, inp.nu);
    let (free, u0, extra) = supports(mesh, inp, el.ke[0]);
    if free.iter().all(|f| *f) && extra.is_empty() {
        return Err("Das Modell ist nicht eingespannt. Mindestens eine Randbedingung (Fest, Verschiebung …) definieren.".into());
    }
    let mut f = vec![0.0; 3 * nn];
    let mut applied = V3::ZERO;
    for (n, v) in &inp.forces { for c in 0..3 { f[*n as usize * 3 + c] += v[c]; } applied += *v; }
    let vol = mesh.elem_volume();
    if inp.accel != V3::ZERO || inp.spin.is_some() {
        for (e, en) in mesh.elems.iter().enumerate() {
            let [i, j, k] = mesh.elem_ijk[e];
            let c = mesh.grid.center(i as usize, j as usize, k as usize);
            let mut a = inp.accel;
            if let Some((p, ax, w)) = inp.spin { let r = c - p; a += (r - ax * r.dot(ax)) * (w * w); }
            let fe = a * (inp.rho * vol / 8.0);
            applied += fe * 8.0;
            for &n in en { for c in 0..3 { f[n as usize * 3 + c] += fe[c]; } }
        }
    }
    let etemp: Option<Vec<f64>> = inp.dtemp.as_ref().map(|t| mesh.elems.iter().map(|en| en.iter().map(|&n| t[n as usize]).sum::<f64>() / 8.0).collect());
    if let Some(et) = &etemp {
        for (e, en) in mesh.elems.iter().enumerate() {
            let s = inp.alpha * et[e];
            if s == 0.0 { continue; }
            for a in 0..8 { for c in 0..3 { f[en[a] as usize * 3 + c] += el.fth[a * 3 + c] * s; } }
        }
    }
    let all = vec![true; 3 * nn];
    let full = Op { mesh, d: 3, ke: &el.ke, free: &all, extra: &extra };
    let op = Op { mesh, d: 3, ke: &el.ke, free: &free, extra: &extra };
    let mut fe = vec![];
    let mut ku0 = vec![0.0; 3 * nn];
    let lifted = u0.iter().any(|v| *v != 0.0);
    if lifted { full.apply(&u0, &mut ku0, &mut fe); }
    let rhs: Vec<f64> = (0..3 * nn).map(|i| if free[i] { f[i] - ku0[i] } else { u0[i] }).collect();
    let mut x = u0.clone();
    ctl.set(0.05);
    let (iters, resid) = pcg(&op, &rhs, &mut x, 1e-6, 40_000, ctl, (0.05, 0.9))?;
    if resid > 1e-2 { return Err("Die Lösung konvergiert nicht: das Modell ist vermutlich nicht ausreichend eingespannt.".into()); }
    // reactions at the supports
    let mut kx = vec![0.0; 3 * nn];
    full.apply(&x, &mut kx, &mut fe);
    let mut reaction = V3::ZERO;
    for n in 0..nn { for c in 0..3 { if !free[n * 3 + c] { reaction[c] += kx[n * 3 + c] - f[n * 3 + c]; } } }
    for (n, blk) in &extra { let b = *n as usize * 3; for r in 0..3 { for c in 0..3 { reaction[r] -= blk[r * 3 + c] * x[b + c]; } } }
    // corner stresses averaged at the nodes
    let mut stress = vec![[0.0f64; 6]; nn];
    let mut count = vec![0u8; nn];
    for (e, en) in mesh.elems.iter().enumerate() {
        let mut ue = [0.0; 24];
        for a in 0..8 { for c in 0..3 { ue[a * 3 + c] = x[en[a] as usize * 3 + c]; } }
        let th = etemp.as_ref().map_or(0.0, |t| inp.alpha * t[e]);
        for a in 0..8 {
            let s = &el.stress[a];
            let n = en[a] as usize;
            for r in 0..6 {
                let mut v = 0.0;
                for j in 0..24 { v += s[r][j] * ue[j]; }
                if th != 0.0 && r < 3 { v -= (el.d[r][0] + el.d[r][1] + el.d[r][2]) * th; }
                stress[n][r] += v;
            }
            count[n] += 1;
        }
    }
    for n in 0..nn { let c = count[n].max(1) as f64; for r in 0..6 { stress[n][r] /= c; } }
    let u = (0..nn).map(|n| V3::new(x[3 * n], x[3 * n + 1], x[3 * n + 2])).collect();
    Ok(StructOut { u, stress, iters, resid, applied, reaction })
}

pub fn von_mises(s: &[f64; 6]) -> f64 {
    (0.5 * ((s[0] - s[1]).powi(2) + (s[1] - s[2]).powi(2) + (s[2] - s[0]).powi(2)) + 3.0 * (s[3] * s[3] + s[4] * s[4] + s[5] * s[5])).sqrt()
}

pub fn max_principal(s: &[f64; 6]) -> f64 {
    let (vals, _) = crate::kernel::topology::sym_eigen3([[s[0], s[3], s[5]], [s[3], s[1], s[4]], [s[5], s[4], s[2]]]);
    vals[0].max(vals[1]).max(vals[2])
}

// ---------------------------------------------------------------- heat

#[derive(Default)]
pub struct ThermalIn {
    /// W/(mm·K)
    pub k: f64,
    pub fixed: Vec<(u32, f64)>,
    /// node, film coefficient × area (W/K), ambient temperature
    pub conv: Vec<(u32, f64, f64)>,
    /// node, heat W
    pub heat: Vec<(u32, f64)>,
}

pub struct ThermalOut {
    pub t: Vec<f64>,
    /// heat flux W/mm² per node
    pub flux: Vec<V3>,
    pub iters: usize,
    /// heat entering through the loads, W
    pub heat_in: f64,
}

pub fn solve_thermal(mesh: &Mesh, inp: &ThermalIn, ctl: &Ctl, span: (f32, f32)) -> Result<ThermalOut, String> {
    let nn = mesh.nodes.len();
    if inp.fixed.is_empty() && inp.conv.is_empty() {
        return Err("Die Temperatur ist unbestimmt: eine Temperatur- oder Konvektionsbedingung definieren.".into());
    }
    let (ke, grad) = elem_thermal(mesh.grid.h, inp.k);
    let mut free = vec![true; nn];
    let mut t0 = vec![0.0; nn];
    for (n, v) in &inp.fixed { free[*n as usize] = false; t0[*n as usize] = *v; }
    let mut q = vec![0.0; nn];
    let mut heat_in = 0.0;
    for (n, v) in &inp.heat { q[*n as usize] += v; heat_in += v; }
    let mut extra = vec![];
    for (n, ha, tinf) in &inp.conv { let mut b = [0.0; 9]; b[0] = *ha; extra.push((*n, b)); q[*n as usize] += ha * tinf; }
    let all = vec![true; nn];
    let full = Op { mesh, d: 1, ke: &ke, free: &all, extra: &extra };
    let op = Op { mesh, d: 1, ke: &ke, free: &free, extra: &extra };
    let mut fe = vec![];
    let mut kt0 = vec![0.0; nn];
    full.apply(&t0, &mut kt0, &mut fe);
    let rhs: Vec<f64> = (0..nn).map(|i| if free[i] { q[i] - kt0[i] } else { t0[i] }).collect();
    // start from the mean of the known temperatures
    let known: Vec<f64> = inp.fixed.iter().map(|x| x.1).chain(inp.conv.iter().map(|x| x.2)).collect();
    let mean = known.iter().sum::<f64>() / known.len() as f64;
    let mut x: Vec<f64> = (0..nn).map(|i| if free[i] { mean } else { t0[i] }).collect();
    let (iters, _) = pcg(&op, &rhs, &mut x, 1e-8, 40_000, ctl, span)?;
    let mut flux = vec![V3::ZERO; nn];
    let mut count = vec![0u8; nn];
    for en in &mesh.elems {
        let mut g = V3::ZERO;
        for a in 0..8 { let t = x[en[a] as usize]; g += V3::new(grad[0][a], grad[1][a], grad[2][a]) * t; }
        for &n in en { flux[n as usize] += g * -inp.k; count[n as usize] += 1; }
    }
    for n in 0..nn { flux[n] /= count[n].max(1) as f64; }
    Ok(ThermalOut { t: x, flux, iters, heat_in })
}

// ---------------------------------------------------------------- modal

pub struct Mode { pub freq: f64, pub shape: Vec<V3> }

/// Eigen-decomposition of a small dense symmetric matrix (cyclic Jacobi), eigenvalues ascending.
pub fn sym_eig(a_in: &[f64], n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut a = a_in.to_vec();
    let mut v = vec![0.0; n * n];
    for i in 0..n { v[i * n + i] = 1.0; }
    for _ in 0..100 {
        let off: f64 = (0..n).flat_map(|i| (0..i).map(move |j| (i, j))).map(|(i, j)| a[i * n + j] * a[i * n + j]).sum();
        let diag: f64 = (0..n).map(|i| a[i * n + i] * a[i * n + i]).sum();
        if off <= 1e-26 * diag.max(1e-300) { break; }
        for p in 0..n {
            for q in p + 1..n {
                let apq = a[p * n + q];
                if apq.abs() < 1e-300 { continue; }
                let theta = (a[q * n + q] - a[p * n + p]) / (2.0 * apq);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n { let (akp, akq) = (a[k * n + p], a[k * n + q]); a[k * n + p] = c * akp - s * akq; a[k * n + q] = s * akp + c * akq; }
                for k in 0..n { let (apk, aqk) = (a[p * n + k], a[q * n + k]); a[p * n + k] = c * apk - s * aqk; a[q * n + k] = s * apk + c * aqk; }
                for k in 0..n { let (vkp, vkq) = (v[k * n + p], v[k * n + q]); v[k * n + p] = c * vkp - s * vkq; v[k * n + q] = s * vkp + c * vkq; }
            }
        }
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| a[i * n + i].partial_cmp(&a[j * n + j]).unwrap());
    let vals = order.iter().map(|&i| a[i * n + i]).collect();
    let mut vecs = vec![0.0; n * n];
    for (c, &o) in order.iter().enumerate() { for k in 0..n { vecs[k * n + c] = v[k * n + o]; } }
    (vals, vecs)
}

/// v·(m ∘ w) for each basis vector, in parallel.
fn mdots(basis: &[&[f64]], m: &[f64], w: &[f64]) -> Vec<f64> {
    let n = w.len();
    let t = super::mesh::threads().min((n / 20_000).max(1));
    let per = n.div_ceil(t);
    let parts: Vec<Vec<f64>> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..t).map(|k| {
            let (a, b) = (k * per, ((k + 1) * per).min(n));
            s.spawn(move || basis.iter().map(|v| { let mut acc = 0.0; if m.is_empty() { for i in a..b { acc += v[i] * w[i]; } } else { for i in a..b { acc += v[i] * m[i] * w[i]; } } acc }).collect::<Vec<f64>>())
        }).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    (0..basis.len()).map(|i| parts.iter().map(|p| p[i]).sum()).collect()
}

/// out = scale·out + Σ c_i basis_i
fn combine(out: &mut [f64], scale: f64, basis: &[&[f64]], c: &[f64]) {
    par_chunks(out, 1, |start, chunk| {
        for (i, o) in chunk.iter_mut().enumerate() {
            let mut s = scale * *o;
            for (v, ci) in basis.iter().zip(c) { s += ci * v[start + i]; }
            *o = s;
        }
    });
}

/// Lowest natural frequencies by LOBPCG on K φ = ω² M φ with lumped masses.
pub fn solve_modal(mesh: &Mesh, inp: &StructIn, want: usize, ctl: &Ctl) -> Result<Vec<Mode>, String> {
    let nn = mesh.nodes.len();
    let n = 3 * nn;
    let el = elem_struct(mesh.grid.h, inp.e, inp.nu);
    let (free, _, extra) = supports(mesh, inp, el.ke[0]);
    let unconstrained = free.iter().all(|f| *f) && extra.is_empty();
    let op = Op { mesh, d: 3, ke: &el.ke, free: &free, extra: &extra };
    let inv = op.precond();
    let mnode = inp.rho * mesh.elem_volume() / 8.0;
    let mut mass = vec![0.0; n];
    for (i, ne) in mesh.node_elems.iter().enumerate() {
        let c = ne.iter().filter(|e| **e != NONE).count() as f64 * mnode;
        for k in 0..3 { mass[3 * i + k] = if free[3 * i + k] { c } else { 0.0 }; }
    }
    let nfree = free.iter().filter(|f| **f).count();
    let skip = if unconstrained { 6 } else { 0 };
    let m = (want + skip + 2).min(nfree.max(1));
    if nfree < want + skip { return Err("Das Netz ist für eine Modalanalyse zu grob.".into()); }
    // start vectors: smooth polynomial fields plus a little noise
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut rnd = || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5 };
    let c0 = mesh.grid.origin + mesh.grid.h * V3::new(mesh.grid.n[0] as f64, mesh.grid.n[1] as f64, mesh.grid.n[2] as f64) * 0.5;
    let mut xs: Vec<Vec<f64>> = (0..m).map(|k| {
        let mut v = vec![0.0; n];
        for i in 0..nn {
            let p = mesh.nodes[i] - c0;
            let poly = match k / 3 { 0 => 1.0, 1 => p.x, 2 => p.y, 3 => p.z, 4 => p.x * p.x, _ => p.x * p.y + p.z };
            for c in 0..3 { if free[3 * i + c] { v[3 * i + c] = if c == k % 3 { poly } else { 0.0 } + rnd() * 1e-2 * (1.0 + poly.abs()); } }
        }
        v
    }).collect();
    let mut fe = vec![];
    let apply_k = |v: &[f64], out: &mut Vec<f64>, fe: &mut Vec<f64>| {
        out.resize(n, 0.0);
        op.apply(v, out, fe);
        for i in 0..n { if !free[i] { out[i] = 0.0; } }
    };
    // M-orthonormalise `v` (and its K-image) against `basis`; false if nothing is left of it
    let ortho = |v: &mut Vec<f64>, kv: &mut Vec<f64>, basis: &[Vec<f64>], kbasis: &[Vec<f64>]| -> bool {
        let n0 = mdots(&[v.as_slice()], &mass, v)[0].sqrt();
        for _ in 0..2 {
            if basis.is_empty() { break; }
            let refs: Vec<&[f64]> = basis.iter().map(|b| b.as_slice()).collect();
            let krefs: Vec<&[f64]> = kbasis.iter().map(|b| b.as_slice()).collect();
            let c: Vec<f64> = mdots(&refs, &mass, v).iter().map(|x| -x).collect();
            combine(v, 1.0, &refs, &c);
            combine(kv, 1.0, &krefs, &c);
        }
        let nrm = mdots(&[v.as_slice()], &mass, v)[0].sqrt();
        if !(nrm > 1e-10 * n0.max(1e-300)) || !nrm.is_finite() { return false; }
        let s = 1.0 / nrm;
        v.iter_mut().for_each(|x| *x *= s);
        kv.iter_mut().for_each(|x| *x *= s);
        true
    };
    let mut kxs: Vec<Vec<f64>> = vec![];
    {
        let (mut b, mut kb): (Vec<Vec<f64>>, Vec<Vec<f64>>) = (vec![], vec![]);
        for mut v in xs.drain(..) {
            let mut kv = vec![];
            apply_k(&v, &mut kv, &mut fe);
            if ortho(&mut v, &mut kv, &b, &kb) { b.push(v); kb.push(kv); }
        }
        xs = b;
        kxs = kb;
    }
    let mut ps: Vec<Vec<f64>> = vec![];
    let mut kps: Vec<Vec<f64>> = vec![];
    let mut lam = vec![0.0; xs.len()];
    let max_it = 600;
    let need = want + skip;
    for it in 0..max_it {
        if ctl.cancelled() { return Err("abgebrochen".into()); }
        // Rayleigh–Ritz on span{X, W, P}
        let m_now = xs.len();
        let mut s: Vec<Vec<f64>> = std::mem::take(&mut xs);
        let mut ks: Vec<Vec<f64>> = std::mem::take(&mut kxs);
        let mut worst = 0.0f64;
        {
            // residuals of the current Ritz pairs, preconditioned into new search directions
            for i in 0..m_now { lam[i] = dot(&s[i], &ks[i]); }
            let lscale = lam.iter().cloned().fold(0.0, f64::max) * 1e-6;
            let mut ws: Vec<Vec<f64>> = vec![];
            for i in 0..m_now {
                let r: Vec<f64> = (0..n).map(|j| ks[i][j] - lam[i] * mass[j] * s[i][j]).collect();
                let rn = dot(&r, &r).sqrt();
                let mx = (0..n).map(|j| (mass[j] * s[i][j]).powi(2)).sum::<f64>().sqrt();
                let rel = rn / ((lam[i].abs() + lscale) * mx).max(1e-300);
                if i < need { worst = worst.max(rel); }
                let mut w = vec![0.0; n];
                apply_prec(&inv, 3, &r, &mut w);
                for j in 0..n { if !free[j] { w[j] = 0.0; } }
                ws.push(w);
            }
            ctl.set(0.05 + 0.9 * ((1.0 / worst.max(1e-300)).ln() / (1.0f64 / 2e-4).ln()).clamp(it as f64 / max_it as f64, 1.0) as f32);
            if worst < 2e-4 && it > 2 { xs = s; kxs = ks; break; }
            for mut w in ws {
                let mut kw = vec![];
                apply_k(&w, &mut kw, &mut fe);
                if ortho(&mut w, &mut kw, &s, &ks) { s.push(w); ks.push(kw); }
            }
            for (mut p, mut kp) in ps.drain(..).zip(kps.drain(..)) {
                if ortho(&mut p, &mut kp, &s, &ks) { s.push(p); ks.push(kp); }
            }
        }
        let q = s.len();
        let mut t = vec![0.0; q * q];
        let refs: Vec<&[f64]> = s.iter().map(|b| b.as_slice()).collect();
        for j in 0..q { let col = mdots(&refs, &[], &ks[j]); for i in 0..q { t[i * q + j] = col[i]; } }
        for i in 0..q { for j in 0..i { let a = 0.5 * (t[i * q + j] + t[j * q + i]); t[i * q + j] = a; t[j * q + i] = a; } }
        let (vals, vecs) = sym_eig(&t, q);
        let krefs: Vec<&[f64]> = ks.iter().map(|b| b.as_slice()).collect();
        let (mut nx, mut nkx, mut np, mut nkp) = (vec![], vec![], vec![], vec![]);
        for c in 0..m_now.min(q) {
            let coef: Vec<f64> = (0..q).map(|i| vecs[i * q + c]).collect();
            let mut x = vec![0.0; n];
            let mut kx = vec![0.0; n];
            combine(&mut x, 0.0, &refs, &coef);
            combine(&mut kx, 0.0, &krefs, &coef);
            if q > m_now {
                // search direction: the part of the new vector outside the old X
                let mut cp = coef.clone();
                for v in cp.iter_mut().take(m_now) { *v = 0.0; }
                let mut p = vec![0.0; n];
                let mut kp = vec![0.0; n];
                combine(&mut p, 0.0, &refs, &cp);
                combine(&mut kp, 0.0, &krefs, &cp);
                np.push(p);
                nkp.push(kp);
            }
            lam[c] = vals[c];
            nx.push(x);
            nkx.push(kx);
        }
        xs = nx; kxs = nkx; ps = np; kps = nkp;
    }
    let mut out = vec![];
    for i in skip..(skip + want).min(xs.len()) {
        let l = dot(&xs[i], &kxs[i]).max(0.0);
        let mut shape: Vec<V3> = (0..nn).map(|k| V3::new(xs[i][3 * k], xs[i][3 * k + 1], xs[i][3 * k + 2])).collect();
        let mx = shape.iter().map(|v| v.length()).fold(0.0, f64::max).max(1e-300);
        // sign: largest component positive, so the shape does not flip between runs
        let big = shape.iter().copied().max_by(|a, b| a.length().partial_cmp(&b.length()).unwrap()).unwrap_or(V3::X);
        let sg = if big.x + big.y + big.z < 0.0 { -1.0 } else { 1.0 };
        shape.iter_mut().for_each(|v| *v *= sg / mx);
        out.push(Mode { freq: l.sqrt() / std::f64::consts::TAU, shape });
    }
    out.sort_by(|a, b| a.freq.partial_cmp(&b.freq).unwrap());
    Ok(out)
}
