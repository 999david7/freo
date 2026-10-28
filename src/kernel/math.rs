use glam::{DMat3, DVec2, DVec3};
use serde::{Deserialize, Serialize};

pub type V2 = DVec2;
pub type V3 = DVec3;

pub fn safe_norm(v: V3, fallback: V3) -> V3 {
    let l = v.length();
    if l < 1e-14 { fallback } else { v / l }
}

pub fn safe_norm2(v: V2, fallback: V2) -> V2 {
    let l = v.length();
    if l < 1e-14 { fallback } else { v / l }
}

#[inline]
pub fn cross2(a: V2, b: V2) -> f64 { a.x * b.y - a.y * b.x }
#[inline]
pub fn perp_right(d: V2) -> V2 { V2::new(d.y, -d.x) }
#[inline]
pub fn perp_left(d: V2) -> V2 { V2::new(-d.y, d.x) }

pub fn rotate_vec(v: V3, u: V3, t: f64) -> V3 {
    let (s, c) = t.sin_cos();
    v * c + u.cross(v) * s + u * u.dot(v) * (1.0 - c)
}

pub fn rotate_point(p: V3, a: V3, u: V3, t: f64) -> V3 { a + rotate_vec(p - a, u, t) }

/// Orthonormal plane coordinate system; z = normal.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub origin: V3,
    pub x: V3,
    pub y: V3,
}

impl Frame {
    pub const WORLD: Frame = Frame { origin: V3::ZERO, x: V3::X, y: V3::Y };
    pub fn z(&self) -> V3 { self.x.cross(self.y) }
    pub fn to_world(&self, p: V2) -> V3 { self.origin + self.x * p.x + self.y * p.y }
    pub fn to_world_h(&self, p: V2, h: f64) -> V3 { self.origin + self.x * p.x + self.y * p.y + self.z() * h }
    pub fn dir_to_world(&self, d: V2) -> V3 { self.x * d.x + self.y * d.y }
    pub fn to_local(&self, p: V3) -> V2 { let d = p - self.origin; V2::new(d.dot(self.x), d.dot(self.y)) }
    pub fn height(&self, p: V3) -> f64 { (p - self.origin).dot(self.z()) }
    pub fn offset(&self, d: f64) -> Frame { Frame { origin: self.origin + self.z() * d, ..*self } }

    /// Deterministic in-plane x axis for a normal (FRONT -> +X, TOP -> +X/-Z, RIGHT -> -Z).
    pub fn from_normal(origin: V3, normal: V3) -> Frame {
        let n = safe_norm(normal, V3::Z);
        let xd = if n.y.abs() > 0.9 {
            safe_norm(V3::X - n * V3::X.dot(n), V3::X)
        } else {
            safe_norm(V3::Y.cross(n), V3::X)
        };
        Frame { origin, x: xd, y: n.cross(xd) }
    }

    /// Frame whose origin is the projection of the world origin onto the plane.
    pub fn plane_through(p: V3, normal: V3) -> Frame {
        let n = safe_norm(normal, V3::Z);
        Frame::from_normal(n * p.dot(n), n)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BBox {
    pub min: V3,
    pub max: V3,
}

impl Default for BBox {
    fn default() -> Self { BBox { min: V3::splat(f64::INFINITY), max: V3::splat(f64::NEG_INFINITY) } }
}

impl BBox {
    pub fn is_empty(&self) -> bool { self.min.x > self.max.x }
    pub fn add(&mut self, p: V3) { self.min = self.min.min(p); self.max = self.max.max(p); }
    pub fn add_box(&mut self, b: &BBox) { if !b.is_empty() { self.add(b.min); self.add(b.max); } }
    pub fn center(&self) -> V3 { if self.is_empty() { V3::ZERO } else { (self.min + self.max) * 0.5 } }
    pub fn size(&self) -> V3 { if self.is_empty() { V3::ZERO } else { self.max - self.min } }
    pub fn diagonal(&self) -> f64 { if self.is_empty() { 0.0 } else { (self.max - self.min).length() } }
    pub fn intersects(&self, o: &BBox, eps: f64) -> bool {
        if self.is_empty() || o.is_empty() { return false; }
        !(o.min.x > self.max.x + eps || o.max.x < self.min.x - eps
            || o.min.y > self.max.y + eps || o.max.y < self.min.y - eps
            || o.min.z > self.max.z + eps || o.max.z < self.min.z - eps)
    }
}

/// Affine transform (rotation/reflection + translation).
#[derive(Clone, Copy, Debug)]
pub struct Xform {
    pub m: DMat3,
    pub t: V3,
}

impl Xform {
    pub fn point(&self, p: V3) -> V3 { self.m * p + self.t }
    pub fn vector(&self, v: V3) -> V3 { self.m * v }
    pub fn is_mirror(&self) -> bool { self.m.determinant() < 0.0 }
    pub fn translation(v: V3) -> Xform { Xform { m: DMat3::IDENTITY, t: v } }
    pub fn rotation(a: V3, u: V3, angle: f64) -> Xform {
        let m = DMat3::from_cols(rotate_vec(V3::X, u, angle), rotate_vec(V3::Y, u, angle), rotate_vec(V3::Z, u, angle));
        Xform { m, t: a - m * a }
    }
    pub fn mirror(f: &Frame) -> Xform {
        let n = f.z();
        let m = DMat3::IDENTITY - 2.0 * DMat3::from_cols(n * n.x, n * n.y, n * n.z);
        Xform { m, t: 2.0 * n * f.origin.dot(n) }
    }
}

pub fn fmt_num(v: f64, decimals: usize) -> String {
    if !v.is_finite() { return "—".into(); }
    let mut s = format!("{:.*}", decimals, v);
    if s.contains('.') {
        while s.ends_with('0') { s.pop(); }
        if s.ends_with('.') { s.pop(); }
    }
    if s == "-0" { s = "0".into(); }
    s
}

pub fn closest_on_segment(p: V3, a: V3, b: V3) -> (V3, f64) {
    let ab = b - a;
    let l2 = ab.dot(ab);
    if l2 < 1e-24 { return (a, 0.0); }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    (a + ab * t, t)
}

pub fn dist_point_segment2(p: V2, a: V2, b: V2) -> f64 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    if l2 < 1e-24 { return (p - a).length(); }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

pub fn polygon_area2(pts: &[V2]) -> f64 {
    let n = pts.len();
    if n < 3 { return 0.0; }
    let mut a = 0.0;
    for i in 0..n {
        let p = pts[i];
        let q = pts[(i + 1) % n];
        a += p.x * q.y - q.x * p.y;
    }
    a * 0.5
}

pub fn point_in_polygon2(p: V2, poly: &[V2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n == 0 { return false; }
    let mut j = n - 1;
    for i in 0..n {
        let a = poly[i];
        let b = poly[j];
        if (a.y > p.y) != (b.y > p.y) {
            let x = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
            if p.x < x { inside = !inside; }
        }
        j = i;
    }
    inside
}

/// Dense Gaussian elimination with partial pivoting (row-major n x n).
pub fn solve_dense(a_in: &[f64], b_in: &[f64], n: usize) -> Option<Vec<f64>> {
    let mut a = a_in.to_vec();
    let mut x = b_in.to_vec();
    for col in 0..n {
        let mut piv = col;
        let mut best = a[col * n + col].abs();
        for r in col + 1..n {
            if a[r * n + col].abs() > best { best = a[r * n + col].abs(); piv = r; }
        }
        if best < 1e-300 { return None; }
        if piv != col {
            for k in 0..n { a.swap(col * n + k, piv * n + k); }
            x.swap(col, piv);
        }
        let d = a[col * n + col];
        for r in col + 1..n {
            let f = a[r * n + col] / d;
            if f == 0.0 { continue; }
            for k in col..n { a[r * n + k] -= f * a[col * n + k]; }
            x[r] -= f * x[col];
        }
    }
    let mut res = vec![0.0; n];
    for r in (0..n).rev() {
        let mut s = x[r];
        for k in r + 1..n { s -= a[r * n + k] * res[k]; }
        res[r] = s / a[r * n + r];
    }
    Some(res)
}

pub fn matrix_rank(m_in: &[f64], rows: usize, cols: usize, tol: f64) -> usize {
    let mut a = m_in.to_vec();
    let mut rank = 0;
    let mut row = 0;
    for col in 0..cols {
        if row >= rows { break; }
        let mut piv = row;
        let mut best = 0.0;
        for r in row..rows {
            if a[r * cols + col].abs() > best { best = a[r * cols + col].abs(); piv = r; }
        }
        if best < tol { continue; }
        if piv != row { for k in 0..cols { a.swap(row * cols + k, piv * cols + k); } }
        let d = a[row * cols + col];
        for r in row + 1..rows {
            let f = a[r * cols + col] / d;
            if f == 0.0 { continue; }
            for k in col..cols { a[r * cols + k] -= f * a[row * cols + k]; }
        }
        row += 1;
        rank += 1;
    }
    rank
}

pub fn deg(r: f64) -> f64 { r.to_degrees() }
pub fn rad(d: f64) -> f64 { d.to_radians() }
