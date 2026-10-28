//! STEP (ISO 10303-21, AP203/AP214/AP242) import: parses the exchange file, follows the
//! B-rep topology (shells, faces, loops, edges) and tessellates analytic and B-spline
//! surfaces into a watertight triangle mesh in millimetres. Edges are sampled once and
//! shared by both faces, so neighbouring faces meet exactly. Assembly files are flattened
//! with the placements of every component occurrence.

use crate::kernel::math::*;
use glam::DMat3;
use crate::kernel::triangulate::triangulate;
use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

// ---------------------------------------------------------------- parser

#[derive(Clone, Debug)]
pub enum P { Num(f64), Str(String), Enum(String), Ref(u64), List(Vec<P>), Typed(String, Box<P>), Null }

impl P {
    fn num(&self) -> Option<f64> { match self { P::Num(v) => Some(*v), P::Typed(_, b) => b.num(), _ => None } }
    fn rf(&self) -> Option<u64> { match self { P::Ref(r) => Some(*r), _ => None } }
    fn list(&self) -> &[P] { match self { P::List(l) => l, _ => &[] } }
    fn boolean(&self) -> bool { !matches!(self, P::Enum(e) if e == "F") }
    fn text(&self) -> String { match self { P::Str(s) => s.clone(), _ => String::new() } }
}

#[derive(Clone, Debug)]
pub struct Entity { pub parts: Vec<(String, Vec<P>)> }

impl Entity {
    #[allow(dead_code)]
    fn ty(&self) -> &str { if self.parts.len() == 1 { &self.parts[0].0 } else { "" } }
    fn part(&self, ty: &str) -> Option<&[P]> { self.parts.iter().find(|p| p.0 == ty).map(|p| p.1.as_slice()) }
    fn has(&self, ty: &str) -> bool { self.parts.iter().any(|p| p.0 == ty) }
    #[allow(dead_code)]
    fn args(&self) -> &[P] { &self.parts[0].1 }
}

struct Lexer<'a> { s: &'a [u8], i: usize }

impl<'a> Lexer<'a> {
    fn ws(&mut self) {
        loop {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() { self.i += 1; }
            if self.s[self.i..].starts_with(b"/*") {
                match find(&self.s[self.i..], b"*/") { Some(k) => self.i += k + 2, None => self.i = self.s.len() }
            } else { break; }
        }
    }
    fn peek(&mut self) -> u8 { self.ws(); *self.s.get(self.i).unwrap_or(&0) }
    fn ident(&mut self) -> String {
        self.ws();
        let st = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_') { self.i += 1; }
        String::from_utf8_lossy(&self.s[st..self.i]).to_uppercase()
    }
    fn param(&mut self) -> P {
        match self.peek() {
            b'#' => { self.i += 1; let st = self.i; while self.i < self.s.len() && self.s[self.i].is_ascii_digit() { self.i += 1; } P::Ref(std::str::from_utf8(&self.s[st..self.i]).ok().and_then(|x| x.parse().ok()).unwrap_or(0)) }
            b'\'' => {
                self.i += 1;
                let mut out = vec![];
                while self.i < self.s.len() {
                    if self.s[self.i] == b'\'' { if self.s.get(self.i + 1) == Some(&b'\'') { out.push(b'\''); self.i += 2; continue; } self.i += 1; break; }
                    out.push(self.s[self.i]); self.i += 1;
                }
                P::Str(String::from_utf8_lossy(&out).to_string())
            }
            b'.' => { self.i += 1; let st = self.i; while self.i < self.s.len() && self.s[self.i] != b'.' { self.i += 1; } let e = String::from_utf8_lossy(&self.s[st..self.i]).to_uppercase(); self.i += 1; P::Enum(e) }
            b'(' => { self.i += 1; P::List(self.list_body()) }
            b'$' | b'*' => { self.i += 1; P::Null }
            b'"' => { self.i += 1; while self.i < self.s.len() && self.s[self.i] != b'"' { self.i += 1; } self.i += 1; P::Null }
            c if c == b'-' || c == b'+' || c.is_ascii_digit() => {
                let st = self.i;
                self.i += 1;
                while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || b".eE+-".contains(&self.s[self.i])) { self.i += 1; }
                P::Num(std::str::from_utf8(&self.s[st..self.i]).ok().and_then(|x| x.parse().ok()).unwrap_or(0.0))
            }
            c if c.is_ascii_alphabetic() => {
                let id = self.ident();
                if self.peek() == b'(' { self.i += 1; let mut l = self.list_body(); P::Typed(id, Box::new(if l.len() == 1 { l.remove(0) } else { P::List(l) })) } else { P::Enum(id) }
            }
            _ => { self.i += 1; P::Null }
        }
    }
    /// Parameters up to and including the closing ')'.
    fn list_body(&mut self) -> Vec<P> {
        let mut out = vec![];
        loop {
            match self.peek() {
                b')' => { self.i += 1; return out; }
                b',' => { self.i += 1; }
                0 => return out,
                _ => out.push(self.param()),
            }
        }
    }
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> { h.windows(n.len()).position(|w| w == n) }

pub fn parse(text: &str) -> Result<HashMap<u64, Entity>, String> {
    let b = text.as_bytes();
    let start = find(b, b"DATA;").ok_or("Keine DATA-Sektion – ist das eine STEP-Datei?")? + 5;
    let mut lx = Lexer { s: b, i: start };
    let mut map = HashMap::new();
    loop {
        let c = lx.peek();
        if c == 0 { break; }
        if c != b'#' {
            let id = lx.ident();
            if id == "ENDSEC" || id.is_empty() { break; }
            continue;
        }
        let P::Ref(id) = lx.param() else { break };
        if lx.peek() != b'=' { break; }
        lx.i += 1;
        let mut parts = vec![];
        if lx.peek() == b'(' {
            lx.i += 1;
            while lx.peek() != b')' && lx.peek() != 0 {
                let ty = lx.ident();
                if lx.peek() != b'(' { break; }
                lx.i += 1;
                parts.push((ty, lx.list_body()));
            }
            lx.i += 1;
        } else {
            let ty = lx.ident();
            if lx.peek() == b'(' { lx.i += 1; parts.push((ty, lx.list_body())); }
        }
        while lx.peek() != b';' && lx.peek() != 0 { lx.i += 1; }
        lx.i += 1;
        if !parts.is_empty() { map.insert(id, Entity { parts }); }
    }
    if map.is_empty() { return Err("Die STEP-Datei enthält keine Daten.".into()); }
    Ok(map)
}

// ---------------------------------------------------------------- geometry

#[derive(Clone, Copy, Debug)]
struct Ax { o: V3, x: V3, y: V3, z: V3 }

impl Ax {
    fn to_xform(self) -> Xform { Xform { m: DMat3::from_cols(self.x, self.y, self.z), t: self.o } }
}

#[derive(Clone, Debug)]
struct Nurbs { deg: usize, knots: Vec<f64>, ctrl: Vec<V3>, w: Vec<f64> }

impl Nurbs {
    fn domain(&self) -> (f64, f64) { (self.knots[self.deg], self.knots[self.knots.len() - 1 - self.deg]) }
    fn span(&self, t: f64) -> usize {
        let n = self.ctrl.len();
        if t >= self.knots[n] { return n - 1; }
        let mut k = self.deg;
        while k + 1 < n && self.knots[k + 1] <= t { k += 1; }
        k
    }
    /// Homogeneous de Boor; returns (point, weight-sum) as 4D.
    fn eval4(&self, t: f64, pts: &[(V3, f64)]) -> (V3, f64) {
        let (a, b) = self.domain();
        let t = t.clamp(a, b);
        let k = self.span(t);
        let p = self.deg;
        let mut d: Vec<(V3, f64)> = (0..=p).map(|j| { let (c, w) = pts[j + k - p]; (c * w, w) }).collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = j + k - p;
                let den = self.knots[i + p - r + 1] - self.knots[i];
                let al = if den.abs() < 1e-14 { 0.0 } else { (t - self.knots[i]) / den };
                d[j] = (d[j - 1].0 * (1.0 - al) + d[j].0 * al, d[j - 1].1 * (1.0 - al) + d[j].1 * al);
            }
        }
        d[p]
    }
    fn eval(&self, t: f64) -> V3 {
        let pts: Vec<(V3, f64)> = self.ctrl.iter().zip(&self.w).map(|(c, w)| (*c, *w)).collect();
        let (c, w) = self.eval4(t, &pts);
        if w.abs() < 1e-14 { c } else { c / w }
    }
}

fn expand_knots(mults: &[P], knots: &[P]) -> Vec<f64> {
    let mut out = vec![];
    for (m, k) in mults.iter().zip(knots) { for _ in 0..m.num().unwrap_or(1.0) as usize { out.push(k.num().unwrap_or(0.0)); } }
    out
}

#[derive(Clone, Debug)]
enum Curve { Line { p: V3, d: V3 }, Circle { ax: Ax, a: f64, b: f64 }, Spline(Nurbs), Poly(Vec<V3>) }

impl Curve {
    fn eval(&self, t: f64) -> V3 {
        match self {
            Curve::Line { p, d } => *p + *d * t,
            Curve::Circle { ax, a, b } => ax.o + ax.x * (a * t.cos()) + ax.y * (b * t.sin()),
            Curve::Spline(n) => n.eval(t),
            Curve::Poly(v) => {
                let n = v.len().saturating_sub(1).max(1);
                let s = (t.clamp(0.0, 1.0) * n as f64).min(n as f64 - 1e-9);
                let i = s as usize;
                v[i].lerp(v[(i + 1).min(v.len() - 1)], s - i as f64)
            }
        }
    }
    fn domain(&self) -> (f64, f64) {
        match self { Curve::Line { .. } => (-1e9, 1e9), Curve::Circle { .. } => (0.0, TAU), Curve::Spline(n) => n.domain(), Curve::Poly(_) => (0.0, 1.0) }
    }
    /// Parameter of the point on the curve nearest `p`.
    fn param(&self, p: V3) -> f64 {
        match self {
            Curve::Line { p: o, d } => (p - *o).dot(*d) / d.length_squared().max(1e-300),
            Curve::Circle { ax, a, b } => { let q = p - ax.o; (q.dot(ax.y) / b).atan2(q.dot(ax.x) / a).rem_euclid(TAU) }
            _ => {
                let (a, b) = self.domain();
                let n = 96;
                let mut best = (f64::MAX, a);
                for i in 0..=n { let t = a + (b - a) * i as f64 / n as f64; let d = (self.eval(t) - p).length_squared(); if d < best.0 { best = (d, t); } }
                let mut h = (b - a) / n as f64;
                let mut t = best.1;
                for _ in 0..40 {
                    let (l, r) = ((t - h).max(a), (t + h).min(b));
                    let (dl, dr, dc) = ((self.eval(l) - p).length_squared(), (self.eval(r) - p).length_squared(), (self.eval(t) - p).length_squared());
                    if dl < dc && dl <= dr { t = l; } else if dr < dc { t = r; } else { h *= 0.5; }
                    if h < 1e-12 * (b - a).abs().max(1.0) { break; }
                }
                t
            }
        }
    }
    fn radius(&self) -> f64 { match self { Curve::Circle { a, b, .. } => a.max(*b), _ => 0.0 } }
}

#[derive(Clone, Debug)]
enum Surf {
    Plane(Ax),
    Cyl(Ax, f64),
    Cone(Ax, f64, f64),
    Sphere(Ax, f64),
    Torus(Ax, f64, f64),
    Spline { u: Nurbs, v: Nurbs, ctrl: Vec<Vec<(V3, f64)>> },
    Revol { c: Curve, o: V3, d: V3 },
    Extru { c: Curve, d: V3 },
}

impl Surf {
    /// +1 when our (u, v) has the same orientation as the STEP parameterisation (whose
    /// Su × Sv defines the natural normal); surfaces of revolution swap u and v.
    fn param_sign(&self) -> f64 { if matches!(self, Surf::Revol { .. }) { -1.0 } else { 1.0 } }
    fn periodic_u(&self) -> bool { matches!(self, Surf::Cyl(..) | Surf::Cone(..) | Surf::Sphere(..) | Surf::Torus(..) | Surf::Revol { .. }) }
    fn periodic_v(&self) -> bool { matches!(self, Surf::Torus(..)) }
    fn eval(&self, u: f64, v: f64) -> V3 {
        match self {
            Surf::Plane(a) => a.o + a.x * u + a.y * v,
            Surf::Cyl(a, r) => a.o + (a.x * u.cos() + a.y * u.sin()) * *r + a.z * v,
            Surf::Cone(a, r, ang) => a.o + (a.x * u.cos() + a.y * u.sin()) * (r + v * ang.tan()) + a.z * v,
            Surf::Sphere(a, r) => a.o + ((a.x * u.cos() + a.y * u.sin()) * v.cos() + a.z * v.sin()) * *r,
            Surf::Torus(a, big, r) => a.o + (a.x * u.cos() + a.y * u.sin()) * (big + r * v.cos()) + a.z * (r * v.sin()),
            Surf::Spline { u: nu, v: nv, ctrl } => {
                let row: Vec<(V3, f64)> = ctrl.iter().map(|r| { let (c, w) = nv.eval4(v, r); (if w.abs() < 1e-14 { c } else { c / w }, w) }).collect();
                let (c, w) = nu.eval4(u, &row);
                if w.abs() < 1e-14 { c } else { c / w }
            }
            Surf::Revol { c, o, d } => rotate_point(c.eval(v), *o, *d, u),
            Surf::Extru { c, d } => c.eval(u) + *d * v,
        }
    }
    fn domain(&self) -> ((f64, f64), (f64, f64)) {
        match self {
            Surf::Spline { u, v, .. } => (u.domain(), v.domain()),
            Surf::Revol { c, .. } => ((0.0, TAU), c.domain()),
            Surf::Extru { c, .. } => (c.domain(), (-1e9, 1e9)),
            _ => ((-1e9, 1e9), (-1e9, 1e9)),
        }
    }
    /// Natural normal Su × Sv (outward for the elementary surfaces).
    fn normal(&self, u: f64, v: f64) -> V3 {
        match self {
            Surf::Plane(a) => a.z,
            Surf::Cyl(a, _) => a.x * u.cos() + a.y * u.sin(),
            Surf::Sphere(a, _) => (a.x * u.cos() + a.y * u.sin()) * v.cos() + a.z * v.sin(),
            Surf::Torus(a, _, _) => (a.x * u.cos() + a.y * u.sin()) * v.cos() + a.z * v.sin(),
            _ => {
                let ((u0, u1), (v0, v1)) = self.domain();
                let hu = ((u1 - u0).abs().min(1e3) * 1e-5).max(1e-7);
                let hv = ((v1 - v0).abs().min(1e3) * 1e-5).max(1e-7);
                let su = self.eval(u + hu, v) - self.eval(u - hu, v);
                let sv = self.eval(u, v + hv) - self.eval(u, v - hv);
                let n = su.cross(sv);
                if n.length() > 1e-20 { n.normalize() } else { V3::ZERO }
            }
        }
    }
    /// (u, v) of point `p`, starting the search near `hint` for the numeric surfaces.
    fn project(&self, p: V3, hint: Option<(f64, f64)>) -> (f64, f64) {
        match self {
            Surf::Plane(a) => { let q = p - a.o; (q.dot(a.x), q.dot(a.y)) }
            Surf::Cyl(a, _) | Surf::Cone(a, _, _) => { let q = p - a.o; (q.dot(a.y).atan2(q.dot(a.x)), q.dot(a.z)) }
            Surf::Sphere(a, r) => { let q = p - a.o; (q.dot(a.y).atan2(q.dot(a.x)), (q.dot(a.z) / r).clamp(-1.0, 1.0).asin()) }
            Surf::Torus(a, big, _) => {
                let q = p - a.o;
                let u = q.dot(a.y).atan2(q.dot(a.x));
                let rad = a.x * u.cos() + a.y * u.sin();
                let w = q - rad * *big;
                (u, w.dot(a.z).atan2(w.dot(rad)))
            }
            _ => self.project_numeric(p, hint),
        }
    }
    fn project_numeric(&self, p: V3, hint: Option<(f64, f64)>) -> (f64, f64) {
        let ((u0, u1), (v0, v1)) = self.domain();
        let (u0, u1) = (u0.max(-1e4), u1.min(1e4));
        let (v0, v1) = (v0.max(-1e4), v1.min(1e4));
        let mut best = (f64::MAX, (u0 + u1) * 0.5, (v0 + v1) * 0.5);
        let g = 14;
        for i in 0..=g { for j in 0..=g {
            let (u, v) = (u0 + (u1 - u0) * i as f64 / g as f64, v0 + (v1 - v0) * j as f64 / g as f64);
            let d = (self.eval(u, v) - p).length_squared();
            if d < best.0 { best = (d, u, v); }
        } }
        let mut starts = vec![(best.1, best.2)];
        if let Some(h) = hint { starts.insert(0, h); }
        let mut res = (f64::MAX, best.1, best.2);
        for (mut u, mut v) in starts {
            for _ in 0..24 {
                let hu = ((u1 - u0) * 1e-6).max(1e-9);
                let hv = ((v1 - v0) * 1e-6).max(1e-9);
                let s = self.eval(u, v);
                let su = (self.eval(u + hu, v) - self.eval(u - hu, v)) / (2.0 * hu);
                let sv = (self.eval(u, v + hv) - self.eval(u, v - hv)) / (2.0 * hv);
                let r = s - p;
                let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
                let (e, f) = (-r.dot(su), -r.dot(sv));
                let det = a * c - b * b;
                if det.abs() < 1e-30 { break; }
                let du = (e * c - b * f) / det;
                let dv = (a * f - b * e) / det;
                u = (u + du).clamp(u0, u1);
                v = (v + dv).clamp(v0, v1);
                if du.abs() < 1e-12 * (u1 - u0).abs().max(1.0) && dv.abs() < 1e-12 * (v1 - v0).abs().max(1.0) { break; }
            }
            let d = (self.eval(u, v) - p).length_squared();
            if d < res.0 { res = (d, u, v); }
        }
        (res.1, res.2)
    }
    /// v at which the surface collapses to a point (sphere poles, cone apex) on the given side.
    fn pole_v(&self, north: bool) -> Option<f64> {
        match self {
            Surf::Sphere(..) => Some(if north { PI / 2.0 } else { -PI / 2.0 }),
            Surf::Cone(_, r, ang) => { let t = ang.tan(); if t.abs() < 1e-12 { None } else { Some(-r / t) } }
            Surf::Spline { .. } | Surf::Revol { .. } => { let (_, (v0, v1)) = self.domain(); Some(if north { v1 } else { v0 }) }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- reader

pub struct FaceMesh { pub verts: Vec<(V3, V3)>, pub tris: Vec<[u32; 3]>, pub kind: &'static str, pub proj_err: f64 }

pub struct ImportResult {
    pub name: String,
    pub faces: Vec<FaceMesh>,
    pub solids: usize,
    pub skipped: usize,
    pub unit: String,
}

struct Reader<'a> {
    e: &'a HashMap<u64, Entity>,
    scale: f64,
    edges: HashMap<u64, Vec<V3>>,
    tol: f64,
    ang_step: f64,
    face_no: usize,
}

impl<'a> Reader<'a> {
    fn get(&self, id: u64) -> Option<&'a Entity> { self.e.get(&id) }
    fn point(&self, id: u64) -> Option<V3> {
        let a = self.get(id)?.part("CARTESIAN_POINT")?;
        let c: Vec<f64> = a.get(1)?.list().iter().filter_map(P::num).collect();
        Some(V3::new(*c.first()?, *c.get(1).unwrap_or(&0.0), *c.get(2).unwrap_or(&0.0)) * self.scale)
    }
    fn dir(&self, id: Option<u64>, default: V3) -> V3 {
        let Some(a) = id.and_then(|i| self.get(i)).and_then(|e| e.part("DIRECTION")) else { return default };
        let c: Vec<f64> = a.get(1).map(|l| l.list().iter().filter_map(P::num).collect()).unwrap_or_default();
        safe_norm(V3::new(*c.first().unwrap_or(&0.0), *c.get(1).unwrap_or(&0.0), *c.get(2).unwrap_or(&0.0)), default)
    }
    fn axis2(&self, id: u64) -> Option<Ax> {
        let a = self.get(id)?.part("AXIS2_PLACEMENT_3D")?;
        let o = self.point(a.get(1)?.rf()?)?;
        let z = self.dir(a.get(2).and_then(P::rf), V3::Z);
        let r = self.dir(a.get(3).and_then(P::rf), if z.x.abs() < 0.9 { V3::X } else { V3::Y });
        let x = safe_norm(r - z * r.dot(z), if z.x.abs() < 0.9 { V3::X } else { V3::Y });
        Some(Ax { o, x, y: z.cross(x), z })
    }
    fn nurbs(&self, deg: f64, ctrl: &P, mults: &P, knots: &P, w: Option<&P>) -> Option<Nurbs> {
        let ctrl: Vec<V3> = ctrl.list().iter().filter_map(|p| p.rf().and_then(|r| self.point(r))).collect();
        let knots = expand_knots(mults.list(), knots.list());
        let w: Vec<f64> = w.map(|w| w.list().iter().filter_map(P::num).collect()).unwrap_or_else(|| vec![1.0; ctrl.len()]);
        let deg = deg as usize;
        if ctrl.len() <= deg || knots.len() != ctrl.len() + deg + 1 || w.len() != ctrl.len() { return None; }
        Some(Nurbs { deg, knots, ctrl, w })
    }
    fn curve(&self, id: u64) -> Option<Curve> {
        let e = self.get(id)?;
        if let Some(a) = e.part("LINE") {
            let p = self.point(a.get(1)?.rf()?)?;
            let v = self.get(a.get(2)?.rf()?)?.part("VECTOR")?;
            let d = self.dir(v.get(1).and_then(P::rf), V3::X) * v.get(2).and_then(P::num).unwrap_or(1.0) * self.scale;
            return Some(Curve::Line { p, d });
        }
        if let Some(a) = e.part("CIRCLE") {
            let r = a.get(2)?.num()? * self.scale;
            return Some(Curve::Circle { ax: self.axis2(a.get(1)?.rf()?)?, a: r, b: r });
        }
        if let Some(a) = e.part("ELLIPSE") {
            return Some(Curve::Circle { ax: self.axis2(a.get(1)?.rf()?)?, a: a.get(2)?.num()? * self.scale, b: a.get(3)?.num()? * self.scale });
        }
        if let Some(a) = e.part("B_SPLINE_CURVE_WITH_KNOTS") {
            if e.parts.len() == 1 { return Some(Curve::Spline(self.nurbs(a.get(1)?.num()?, a.get(2)?, a.get(6)?, a.get(7)?, None)?)); }
            let b = e.part("B_SPLINE_CURVE")?;
            let w = e.part("RATIONAL_B_SPLINE_CURVE").and_then(|r| r.first());
            return Some(Curve::Spline(self.nurbs(b.first()?.num()?, b.get(1)?, a.first()?, a.get(1)?, w)?));
        }
        if let Some(a) = e.part("SURFACE_CURVE").or_else(|| e.part("SEAM_CURVE")).or_else(|| e.part("INTERSECTION_CURVE")) { return self.curve(a.get(1)?.rf()?); }
        if let Some(a) = e.part("TRIMMED_CURVE") { return self.curve(a.get(1)?.rf()?); }
        if let Some(a) = e.part("POLYLINE") { return Some(Curve::Poly(a.get(1)?.list().iter().filter_map(|p| p.rf().and_then(|r| self.point(r))).collect())); }
        None
    }
    fn surface(&self, id: u64) -> Option<Surf> {
        let e = self.get(id)?;
        if let Some(a) = e.part("PLANE") { return Some(Surf::Plane(self.axis2(a.get(1)?.rf()?)?)); }
        if let Some(a) = e.part("CYLINDRICAL_SURFACE") { return Some(Surf::Cyl(self.axis2(a.get(1)?.rf()?)?, a.get(2)?.num()? * self.scale)); }
        if let Some(a) = e.part("CONICAL_SURFACE") { return Some(Surf::Cone(self.axis2(a.get(1)?.rf()?)?, a.get(2)?.num()? * self.scale, self.angle(a.get(3)?.num()?))); }
        if let Some(a) = e.part("SPHERICAL_SURFACE") { return Some(Surf::Sphere(self.axis2(a.get(1)?.rf()?)?, a.get(2)?.num()? * self.scale)); }
        if let Some(a) = e.part("TOROIDAL_SURFACE") { return Some(Surf::Torus(self.axis2(a.get(1)?.rf()?)?, a.get(2)?.num()? * self.scale, a.get(3)?.num()? * self.scale)); }
        if let Some(a) = e.part("B_SPLINE_SURFACE_WITH_KNOTS") {
            let (du, dv, grid, um, vm, uk, vk, w) = if e.parts.len() == 1 {
                (a.get(1)?.num()?, a.get(2)?.num()?, a.get(3)?, a.get(8)?, a.get(9)?, a.get(10)?, a.get(11)?, None)
            } else {
                let b = e.part("B_SPLINE_SURFACE")?;
                (b.first()?.num()?, b.get(1)?.num()?, b.get(2)?, a.first()?, a.get(1)?, a.get(2)?, a.get(3)?, e.part("RATIONAL_B_SPLINE_SURFACE").and_then(|r| r.first()))
            };
            let rows: Vec<Vec<V3>> = grid.list().iter().map(|r| r.list().iter().filter_map(|p| p.rf().and_then(|i| self.point(i))).collect()).collect();
            let ws: Vec<Vec<f64>> = match w { Some(w) => w.list().iter().map(|r| r.list().iter().filter_map(P::num).collect()).collect(), None => rows.iter().map(|r| vec![1.0; r.len()]).collect() };
            let nv = rows.first()?.len();
            let uk = expand_knots(um.list(), uk.list());
            let vk = expand_knots(vm.list(), vk.list());
            let (du, dv) = (du as usize, dv as usize);
            if uk.len() != rows.len() + du + 1 || vk.len() != nv + dv + 1 || ws.len() != rows.len() { return None; }
            let ctrl: Vec<Vec<(V3, f64)>> = rows.iter().zip(&ws).map(|(r, w)| r.iter().copied().zip(w.iter().copied()).collect()).collect();
            if ctrl.iter().any(|r| r.len() != nv) { return None; }
            let u = Nurbs { deg: du, knots: uk, ctrl: vec![V3::ZERO; rows.len()], w: vec![1.0; rows.len()] };
            let v = Nurbs { deg: dv, knots: vk, ctrl: vec![V3::ZERO; nv], w: vec![1.0; nv] };
            return Some(Surf::Spline { u, v, ctrl });
        }
        if let Some(a) = e.part("SURFACE_OF_REVOLUTION") {
            let c = self.curve(a.get(1)?.rf()?)?;
            let ax = self.get(a.get(2)?.rf()?)?.part("AXIS1_PLACEMENT")?;
            return Some(Surf::Revol { c, o: self.point(ax.get(1)?.rf()?)?, d: self.dir(ax.get(2).and_then(P::rf), V3::Z) });
        }
        if let Some(a) = e.part("SURFACE_OF_LINEAR_EXTRUSION") {
            let c = self.curve(a.get(1)?.rf()?)?;
            let v = self.get(a.get(2)?.rf()?)?.part("VECTOR")?;
            return Some(Surf::Extru { c, d: self.dir(v.get(1).and_then(P::rf), V3::Z) * v.get(2).and_then(P::num).unwrap_or(1.0) * self.scale });
        }
        None
    }
    /// STEP angles are radians unless the file says degrees; Creo writes radians.
    fn angle(&self, a: f64) -> f64 { if a.abs() > TAU { a.to_radians() } else { a } }

    /// Polyline of an EDGE_CURVE from its start vertex to its end vertex (cached, shared).
    fn edge(&mut self, id: u64) -> Option<Vec<V3>> {
        if let Some(p) = self.edges.get(&id) { return Some(p.clone()); }
        let a = self.get(id)?.part("EDGE_CURVE")?;
        let vp = |r: &Reader, i: Option<u64>| -> Option<V3> { r.point(r.get(i?)?.part("VERTEX_POINT")?.get(1)?.rf()?) };
        let p1 = vp(self, a.get(1).and_then(P::rf))?;
        let p2 = vp(self, a.get(2).and_then(P::rf))?;
        let sense = a.get(4).map(P::boolean).unwrap_or(true);
        let curve = a.get(3).and_then(P::rf).and_then(|c| self.curve(c));
        let closed = (p1 - p2).length() < self.tol * 0.01;
        let mut pts = match curve {
            None | Some(Curve::Line { .. }) => vec![p1, p2],
            Some(Curve::Poly(v)) => { let mut v = v; if (v[0] - p1).length() > (v[v.len() - 1] - p1).length() { v.reverse(); } v }
            Some(c) => {
                let (d0, d1) = c.domain();
                let mut t1 = c.param(p1);
                let mut t2 = c.param(p2);
                let periodic = matches!(c, Curve::Circle { .. });
                if periodic {
                    if sense { if t2 <= t1 + 1e-9 { t2 += TAU; } } else if t2 >= t1 - 1e-9 { t2 -= TAU; }
                    if closed { t2 = if sense { t1 + TAU } else { t1 - TAU }; }
                } else if closed { t1 = if sense { d0 } else { d1 }; t2 = if sense { d1 } else { d0 }; }
                let n = match &c {
                    Curve::Circle { .. } => {
                        let r = c.radius().max(1e-9);
                        let step = (2.0 * (1.0 - (self.tol / r).min(1.0)).acos()).clamp(2f64.to_radians(), self.ang_step);
                        ((t2 - t1).abs() / step).ceil().max(1.0) as usize
                    }
                    Curve::Spline(nb) => {
                        // spans inside the trimmed range, each sampled by chord deviation
                        let spans = nb.knots.windows(2).filter(|w| w[1] > w[0] + 1e-12 && w[1] > t1.min(t2) && w[0] < t1.max(t2)).count().max(1);
                        let mut n = (spans * 4).max(8);
                        loop {
                            let mut dev: f64 = 0.0;
                            for i in 0..n {
                                let (a, b) = (t1 + (t2 - t1) * i as f64 / n as f64, t1 + (t2 - t1) * (i + 1) as f64 / n as f64);
                                let (pa, pb, pm) = (c.eval(a), c.eval(b), c.eval(0.5 * (a + b)));
                                dev = dev.max((pm - (pa + pb) * 0.5).length());
                            }
                            if dev <= self.tol || n >= 512 { break n; }
                            n *= 2;
                        }
                    }
                    _ => 32,
                };
                let mut v: Vec<V3> = (0..=n).map(|i| c.eval(t1 + (t2 - t1) * i as f64 / n as f64)).collect();
                let _ = (d0, d1);
                v[0] = p1;
                *v.last_mut().unwrap() = p2;
                v
            }
        };
        if pts.len() < 2 { pts = vec![p1, p2]; }
        self.edges.insert(id, pts.clone());
        Some(pts)
    }

    fn loop_points(&mut self, id: u64) -> Option<(Vec<V3>, bool)> {
        let e = self.get(id)?;
        if let Some(a) = e.part("POLY_LOOP") { return Some((a.get(1)?.list().iter().filter_map(|p| p.rf().and_then(|r| self.point(r))).collect(), false)); }
        if let Some(a) = e.part("VERTEX_LOOP") {
            let v = self.get(a.get(1)?.rf()?)?.part("VERTEX_POINT")?;
            return Some((vec![self.point(v.get(1)?.rf()?)?], true));
        }
        let a = e.part("EDGE_LOOP")?;
        let mut out: Vec<V3> = vec![];
        for oe in a.get(1)?.list() {
            let o = self.get(oe.rf()?)?.part("ORIENTED_EDGE")?;
            let mut pts = self.edge(o.get(3)?.rf()?)?;
            if !o.get(4).map(P::boolean).unwrap_or(true) { pts.reverse(); }
            if let Some(l) = out.last() { if (*l - pts[0]).length() < 1e-9 { pts.remove(0); } }
            out.extend(pts);
        }
        if out.len() > 1 && (out[0] - out[out.len() - 1]).length() < 1e-9 { out.pop(); }
        Some((out, false))
    }

    fn face(&mut self, id: u64) -> Result<FaceMesh, ()> {
        let e = self.get(id).ok_or(())?;
        let a = e.part("ADVANCED_FACE").or_else(|| e.part("FACE_SURFACE")).or_else(|| e.part("FACE")).ok_or(())?;
        let sense = a.get(3).map(P::boolean).unwrap_or(true);
        let mut loops: Vec<(Vec<V3>, bool)> = vec![];
        let mut poles: Vec<V3> = vec![];
        for b in a.get(1).map(P::list).unwrap_or(&[]) {
            let be = self.get(b.rf().ok_or(())?).ok_or(())?;
            let (outer, args) = match (be.part("FACE_OUTER_BOUND"), be.part("FACE_BOUND")) { (Some(x), _) => (true, x), (_, Some(x)) => (false, x), _ => return Err(()) };
            let (mut pts, vertex) = self.loop_points(args.get(1).and_then(P::rf).ok_or(())?).ok_or(())?;
            if vertex { poles.extend(pts); continue; }
            if !args.get(2).map(P::boolean).unwrap_or(true) { pts.reverse(); }
            if pts.len() >= 3 { loops.push((pts, outer)); }
        }
        if loops.is_empty() { return Err(()); }
        if std::env::var("STEPDBG").ok().and_then(|v| v.parse::<usize>().ok()) == Some(self.face_no) {
            eprintln!("face #{} surface {:?}", id, a.get(2).and_then(P::rf).and_then(|s| self.get(s)).map(|e| e.parts.iter().map(|p| p.0.clone()).collect::<Vec<_>>()));
            for (l, o) in &loops { eprintln!("  loop outer={} n={} first={:?}", o, l.len(), &l[..l.len().min(4)]); }
        }
        let surf = match a.get(2).and_then(P::rf) {
            Some(s) => self.surface(s),
            None => None,
        };
        let surf = match surf {
            Some(s) => s,
            None => {
                // faceted face or unsupported surface: use the best-fit plane of the outer loop
                let pts = &loops[0].0;
                let mut n = V3::ZERO;
                for i in 0..pts.len() { let (p, q) = (pts[i], pts[(i + 1) % pts.len()]); n += V3::new((p.y - q.y) * (p.z + q.z), (p.z - q.z) * (p.x + q.x), (p.x - q.x) * (p.y + q.y)); }
                let f = Frame::from_normal(pts[0], safe_norm(n, V3::Z));
                Surf::Plane(Ax { o: f.origin, x: f.x, y: f.y, z: f.z() })
            }
        };
        let dbg = std::env::var("STEPDBG").ok().and_then(|v| v.parse::<usize>().ok()) == Some(self.face_no);
        mesh_face(&surf, sense, &loops, &poles, self.tol, dbg).ok_or(())
    }
}

/// Triangulates one trimmed face; boundary vertices keep their exact 3D positions.
fn mesh_face(surf: &Surf, sense: bool, loops: &[(Vec<V3>, bool)], poles: &[V3], tol: f64, dbg: bool) -> Option<FaceMesh> {
    let pu = surf.periodic_u();
    let pv = surf.periodic_v();
    let unwrap = |prev: f64, x: f64| -> f64 { prev + (x - prev + PI).rem_euclid(TAU) - PI };
    // project each loop to UV, unwrapping periodic parameters continuously
    let mut uv_loops: Vec<(Vec<(f64, f64)>, Vec<V3>, bool, f64)> = vec![];
    for (pts, outer) in loops {
        let mut uv = Vec::with_capacity(pts.len());
        let mut hint = None;
        for p in pts {
            let (mut u, mut v) = surf.project(*p, hint);
            if let Some((pu0, pv0)) = uv.last().copied() {
                if pu { u = unwrap(pu0, u); }
                if pv { v = unwrap(pv0, v); }
            }
            hint = Some((u, v));
            uv.push((u, v));
        }
        // seam-crossing singular points (sphere poles): give them the u of a neighbour
        if matches!(surf, Surf::Sphere(..) | Surf::Cone(..)) {
            for i in 0..pts.len() {
                let deg = match surf { Surf::Sphere(..) => (uv[i].1.abs() - PI / 2.0).abs() < 1e-6, Surf::Cone(_, r, ang) => (r + uv[i].1 * ang.tan()).abs() < tol * 1e-3, _ => false };
                if deg { let n = pts.len(); uv[i].0 = 0.5 * (uv[(i + n - 1) % n].0 + uv[(i + 1) % n].0); }
            }
        }
        let du = if pu && uv.len() > 1 { let last = uv[uv.len() - 1].0; unwrap(last, uv[0].0) - uv[0].0 } else { 0.0 };
        uv_loops.push((uv, pts.clone(), *outer, du));
    }
    if dbg {
        eprintln!("domain {:?}", surf.domain());
        for l in &uv_loops { eprintln!("  du={:.3} uv: {:?}", l.3, l.0.iter().map(|(u, v)| format!("({:.3},{:.3})", u, v)).collect::<Vec<_>>().join(" ")); }
    }
    // isotropic scaling for triangle quality
    let (su, sv) = match surf {
        Surf::Cyl(_, r) | Surf::Sphere(_, r) => (*r, if matches!(surf, Surf::Sphere(..)) { *r } else { 1.0 }),
        Surf::Cone(_, r, _) => (r.abs().max(1.0), 1.0),
        Surf::Torus(_, big, r) => (big + r, *r),
        Surf::Plane(_) => (1.0, 1.0),
        _ => {
            // average metric over a few points inside the loop's UV box (central differences)
            let ((u0, u1), (v0, v1)) = surf.domain();
            let (mut lu, mut hu_, mut lv, mut hv_) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for l in &uv_loops { for (u, v) in &l.0 { lu = lu.min(*u); hu_ = hu_.max(*u); lv = lv.min(*v); hv_ = hv_.max(*v); } }
            let hu = ((hu_ - lu).abs() * 1e-3).max(1e-7);
            let hv = ((hv_ - lv).abs() * 1e-3).max(1e-7);
            let (mut a, mut b, mut n) = (0.0, 0.0, 0.0);
            for i in 1..4 { for j in 1..4 {
                let u = (lu + (hu_ - lu) * i as f64 / 4.0).clamp(u0 + hu, u1 - hu);
                let v = (lv + (hv_ - lv) * j as f64 / 4.0).clamp(v0 + hv, v1 - hv);
                a += (surf.eval(u + hu, v) - surf.eval(u - hu, v)).length() / (2.0 * hu);
                b += (surf.eval(u, v + hv) - surf.eval(u, v - hv)).length() / (2.0 * hv);
                n += 1.0;
            } }
            let (a, b) = ((a / n).max(1e-9), (b / n).max(1e-9));
            let r = (a / b).clamp(1e-3, 1e3);
            (r * b, b)
        }
    };
    let wrapping: Vec<usize> = (0..uv_loops.len()).filter(|&i| (uv_loops[i].3.abs() - TAU).abs() < 0.6).collect();
    let mut verts: Vec<(f64, f64, V3, bool)> = vec![]; // (u, v, point, boundary)
    let mut outer_idx: Vec<usize> = vec![];
    let mut holes_idx: Vec<Vec<usize>> = vec![];
    let push = |verts: &mut Vec<(f64, f64, V3, bool)>, uv: (f64, f64), p: V3| { verts.push((uv.0, uv.1, p, true)); verts.len() - 1 };
    let mut used = vec![false; uv_loops.len()];
    if wrapping.len() >= 2 {
        // band between two loops that run around the periodic direction
        let (ia, ib) = (wrapping[0], wrapping[1]);
        used[ia] = true; used[ib] = true;
        let seam = uv_loops[ia].0[0].0;
        let band = |l: &(Vec<(f64, f64)>, Vec<V3>, bool, f64)| -> Vec<((f64, f64), V3)> {
            // start at the point nearest the seam, run with increasing u over one period
            let n = l.0.len();
            let mut pts: Vec<((f64, f64), V3)> = (0..n).map(|i| (l.0[i], l.1[i])).collect();
            if l.3 < 0.0 { pts.reverse(); }
            let k = (0..n).min_by(|&a, &b| ((pts[a].0 .0 - seam).rem_euclid(TAU)).partial_cmp(&((pts[b].0 .0 - seam).rem_euclid(TAU))).unwrap()).unwrap_or(0);
            pts.rotate_left(k);
            let mut prev = seam + (pts[0].0 .0 - seam).rem_euclid(TAU);
            if prev - seam > PI { prev -= TAU; }
            let mut out = vec![];
            for (i, (uv, p)) in pts.into_iter().enumerate() {
                let u = if i == 0 { prev } else { unwrap(prev, uv.0) };
                prev = u;
                out.push(((u, uv.1), p));
            }
            let first = out[0];
            out.push(((first.0 .0 + TAU, first.0 .1), first.1));
            out
        };
        let a = band(&uv_loops[ia]);
        let b = band(&uv_loops[ib]);
        for (uv, p) in &a { let i = push(&mut verts, *uv, *p); outer_idx.push(i); }
        for (uv, p) in b.iter().rev() { let i = push(&mut verts, *uv, *p); outer_idx.push(i); }
    } else if wrapping.len() == 1 {
        // cap closing at a pole or cone apex
        let ia = wrapping[0];
        used[ia] = true;
        let l = &uv_loops[ia];
        let north = (l.3 > 0.0) == sense;
        let pole = surf.pole_v(north)?;
        let n = l.0.len();
        let mut seq: Vec<((f64, f64), V3)> = (0..n).map(|i| (l.0[i], l.1[i])).collect();
        seq.push(((l.0[0].0 + l.3, l.0[0].1), l.1[0]));
        for (uv, p) in &seq { let i = push(&mut verts, *uv, *p); outer_idx.push(i); }
        let (ue, us) = (seq[seq.len() - 1].0 .0, seq[0].0 .0);
        let pole_pt = poles.first().copied().unwrap_or_else(|| surf.eval(us, pole));
        let steps = 12;
        for k in 0..=steps { let u = ue + (us - ue) * k as f64 / steps as f64; let i = push(&mut verts, (u, pole), pole_pt); outer_idx.push(i); }
    }
    // remaining loops: the outer bound (or largest loop) and holes
    let rest: Vec<usize> = (0..uv_loops.len()).filter(|&i| !used[i]).collect();
    let area = |l: &[(f64, f64)]| { let pts: Vec<V2> = l.iter().map(|(u, v)| V2::new(u * su, v * sv)).collect(); polygon_area2(&pts).abs() };
    let outer_loop = if outer_idx.is_empty() {
        rest.iter().copied().find(|&i| uv_loops[i].2 && rest.len() > 1).or_else(|| rest.iter().copied().max_by(|&a, &b| area(&uv_loops[a].0).partial_cmp(&area(&uv_loops[b].0)).unwrap()))
    } else { None };
    if let Some(o) = outer_loop {
        for k in 0..uv_loops[o].0.len() { let i = push(&mut verts, uv_loops[o].0[k], uv_loops[o].1[k]); outer_idx.push(i); }
    }
    if outer_idx.len() < 3 { return None; }
    let (ou_min, ou_max) = outer_idx.iter().fold((f64::MAX, f64::MIN), |a, &i| (a.0.min(verts[i].0), a.1.max(verts[i].0)));
    let (ov_min, ov_max) = outer_idx.iter().fold((f64::MAX, f64::MIN), |a, &i| (a.0.min(verts[i].1), a.1.max(verts[i].1)));
    for &h in rest.iter().filter(|&&i| Some(i) != outer_loop) {
        // shift periodic holes into the outer loop's parameter range
        let (cu, cv) = { let l = &uv_loops[h].0; (l.iter().map(|x| x.0).sum::<f64>() / l.len() as f64, l.iter().map(|x| x.1).sum::<f64>() / l.len() as f64) };
        let mid_u = 0.5 * (ou_min + ou_max);
        let mid_v = 0.5 * (ov_min + ov_max);
        let ku = if pu { ((mid_u - cu) / TAU).round() * TAU } else { 0.0 };
        let kv = if pv { ((mid_v - cv) / TAU).round() * TAU } else { 0.0 };
        let mut idx = vec![];
        for k in 0..uv_loops[h].0.len() { let (u, v) = uv_loops[h].0[k]; idx.push(push(&mut verts, (u + ku, v + kv), uv_loops[h].1[k])); }
        holes_idx.push(idx);
    }
    // ear clipping in scaled UV
    let outer2: Vec<V2> = outer_idx.iter().map(|&i| V2::new(verts[i].0 * su, verts[i].1 * sv)).collect();
    let holes2: Vec<Vec<V2>> = holes_idx.iter().map(|h| h.iter().map(|&i| V2::new(verts[i].0 * su, verts[i].1 * sv)).collect()).collect();
    let tris_local = triangulate(&outer2, &holes2);
    if tris_local.is_empty() { return None; }
    let mut map: Vec<usize> = outer_idx.clone();
    for h in &holes_idx { map.extend(h.iter().copied()); }
    let mut tris: Vec<[usize; 3]> = tris_local.iter().map(|t| [map[t[0]], map[t[1]], map[t[2]]]).collect();
    // refine curved faces: Delaunay flips in scaled UV, then split edges whose chord leaves the surface
    if !matches!(surf, Surf::Plane(_)) {
        flips(&verts, &mut tris, su, sv);
        refine(surf, &mut verts, &mut tris, tol, su, sv);
    }
    // orient against the face normal and emit: triangles are CCW in UV, i.e. along our Su × Sv
    let flip = if sense { 1.0 } else { -1.0 } * surf.param_sign();
    let reverse = (if sense { 1.0 } else { -1.0 }) * surf.param_sign() < 0.0;
    let normal_at = |v: &(f64, f64, V3, bool)| -> V3 { let n = surf.normal(v.0, v.1) * flip; if n.length() > 0.5 { n } else { V3::ZERO } };
    let mut out_v: Vec<(V3, V3)> = verts.iter().map(|v| (v.2, normal_at(v))).collect();
    let mut out_t = vec![];
    for t in tris {
        let (a, b, c) = (verts[t[0]].2, verts[t[1]].2, verts[t[2]].2);
        if (b - a).cross(c - a).length() < 1e-14 { continue; }
        if reverse { out_t.push([t[0] as u32, t[2] as u32, t[1] as u32]); } else { out_t.push([t[0] as u32, t[1] as u32, t[2] as u32]); }
    }
    // degenerate normals (poles): fall back to the triangle normals around the vertex
    for (i, v) in out_v.iter_mut().enumerate() {
        if v.1 == V3::ZERO {
            let mut acc = V3::ZERO;
            for t in &out_t { if t.contains(&(i as u32)) { let (a, b, c) = (verts[t[0] as usize].2, verts[t[1] as usize].2, verts[t[2] as usize].2); acc += (b - a).cross(c - a); } }
            v.1 = safe_norm(acc, V3::Z);
        }
    }
    let kind = match surf { Surf::Plane(_) => "plane", Surf::Cyl(..) => "cyl", Surf::Cone(..) => "cone", Surf::Sphere(..) => "sphere", Surf::Torus(..) => "torus", Surf::Spline { .. } => "bspline", Surf::Revol { .. } => "revol", Surf::Extru { .. } => "extru" };
    let proj_err = uv_loops.iter().flat_map(|l| l.0.iter().zip(&l.1)).map(|(uv, p)| (surf.eval(uv.0, uv.1) - *p).length()).fold(0.0, f64::max);
    Some(FaceMesh { verts: out_v, tris: out_t, kind, proj_err })
}

fn orient2(a: V2, b: V2, c: V2) -> f64 { (b - a).perp_dot(c - a) }

fn in_circle(a: V2, b: V2, c: V2, d: V2) -> f64 {
    let (ad, bd, cd) = (a - d, b - d, c - d);
    (ad.x * ad.x + ad.y * ad.y) * bd.perp_dot(cd) - (bd.x * bd.x + bd.y * bd.y) * ad.perp_dot(cd) + (cd.x * cd.x + cd.y * cd.y) * ad.perp_dot(bd)
}

/// Lawson edge flips towards a Delaunay triangulation in scaled UV (boundary edges stay).
fn flips(verts: &[(f64, f64, V3, bool)], tris: &mut [[usize; 3]], su: f64, sv: f64) {
    let p = |i: usize| V2::new(verts[i].0 * su, verts[i].1 * sv);
    for _ in 0..30 {
        let mut edge: HashMap<(usize, usize), Vec<(usize, usize)>> = HashMap::new();
        for (ti, t) in tris.iter().enumerate() { for k in 0..3 { let (a, b) = (t[k], t[(k + 1) % 3]); edge.entry((a.min(b), a.max(b))).or_default().push((ti, k)); } }
        let mut done = vec![false; tris.len()];
        let mut changed = 0;
        for (_, v) in edge.iter() {
            if v.len() != 2 { continue; }
            let ((t1, k1), (t2, k2)) = (v[0], v[1]);
            if done[t1] || done[t2] { continue; }
            let (a, b, c) = (tris[t1][k1], tris[t1][(k1 + 1) % 3], tris[t1][(k1 + 2) % 3]);
            let (b2, a2, d) = (tris[t2][k2], tris[t2][(k2 + 1) % 3], tris[t2][(k2 + 2) % 3]);
            if a2 != a || b2 != b || c == d { continue; }
            if in_circle(p(a), p(b), p(c), p(d)) <= 1e-12 * (p(a) - p(b)).length_squared().powi(2) { continue; }
            if orient2(p(a), p(d), p(c)) <= 1e-14 || orient2(p(b), p(c), p(d)) <= 1e-14 { continue; }
            tris[t1] = [a, d, c];
            tris[t2] = [b, c, d];
            done[t1] = true; done[t2] = true;
            changed += 1;
        }
        if changed == 0 { break; }
    }
}

fn refine(surf: &Surf, verts: &mut Vec<(f64, f64, V3, bool)>, tris: &mut Vec<[usize; 3]>, tol: f64, su: f64, sv: f64) {
    let budget = 8_000usize;
    for _pass in 0..12 {
        let mut edge_tris: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (ti, t) in tris.iter().enumerate() {
            for k in 0..3 { let (a, b) = (t[k], t[(k + 1) % 3]); edge_tris.entry((a.min(b), a.max(b))).or_default().push(ti); }
        }
        // interior edges with too much chord deviation, worst first
        let mut cand: Vec<(f64, (usize, usize))> = edge_tris.iter().filter(|(_, ts)| ts.len() == 2).filter_map(|(&(a, b), _)| {
            let (va, vb) = (verts[a], verts[b]);
            let m = surf.eval(0.5 * (va.0 + vb.0), 0.5 * (va.1 + vb.1));
            let dev = (m - (va.2 + vb.2) * 0.5).length();
            // short edges near singular points (poles, apex) are left alone
            if dev > tol && (va.2 - vb.2).length() > 2.0 * tol { Some((dev, (a, b))) } else { None }
        }).collect();
        if cand.is_empty() || tris.len() > budget { break; }
        cand.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap());
        let mut touched = vec![false; tris.len()];
        let mut new_tris: Vec<[usize; 3]> = vec![];
        for (_, (a, b)) in cand {
            let ts = &edge_tris[&(a, b)];
            if ts.iter().any(|&t| touched[t]) { continue; }
            let (va, vb) = (verts[a], verts[b]);
            let (u, v) = (0.5 * (va.0 + vb.0), 0.5 * (va.1 + vb.1));
            verts.push((u, v, surf.eval(u, v), false));
            let m = verts.len() - 1;
            for &t in ts {
                touched[t] = true;
                let tri = tris[t];
                // keep the winding: replace edge (p,q) in order by (p,m),(m,q)
                let k = (0..3).find(|&k| { let (p, q) = (tri[k], tri[(k + 1) % 3]); (p == a && q == b) || (p == b && q == a) }).unwrap();
                let (p, q, r) = (tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]);
                new_tris.push([p, m, r]);
                new_tris.push([m, q, r]);
            }
        }
        let mut kept: Vec<[usize; 3]> = tris.iter().enumerate().filter(|(i, _)| !touched[*i]).map(|(_, t)| *t).collect();
        kept.extend(new_tris);
        *tris = kept;
        flips(verts, tris, su, sv);
    }
}

fn unit_factor(e: &HashMap<u64, Entity>, id: u64, depth: usize) -> Option<(f64, String)> {
    let ent = e.get(&id)?;
    if depth > 4 || !ent.has("LENGTH_UNIT") { return None; }
    if let Some(c) = ent.part("CONVERSION_BASED_UNIT") {
        let name = c.first().map(P::text).unwrap_or_default();
        let m = c.get(1).and_then(P::rf).and_then(|m| e.get(&m));
        let mw = m.and_then(|m| m.part("LENGTH_MEASURE_WITH_UNIT").or_else(|| m.part("MEASURE_WITH_UNIT")));
        let val = mw.and_then(|a| a.first().and_then(P::num));
        let base = mw.and_then(|a| a.get(1).and_then(P::rf)).and_then(|b| unit_factor(e, b, depth + 1)).map(|b| b.0);
        let f = match (val, base) { (Some(v), Some(b)) => v * b, _ => match name.to_uppercase().as_str() { "INCH" => 25.4, "FOOT" => 304.8, _ => 1.0 } };
        return Some((f, name.to_lowercase()));
    }
    let s = ent.part("SI_UNIT")?;
    let pre = match s.first() { Some(P::Enum(p)) => p.as_str(), _ => "" };
    Some(match pre { "MILLI" => (1.0, "mm".into()), "CENTI" => (10.0, "cm".into()), "DECI" => (100.0, "dm".into()), "MICRO" => (0.001, "µm".into()), "KILO" => (1e6, "km".into()), _ => (1000.0, "m".into()) })
}

/// Length unit of the geometry context (the one the coordinates are written in).
fn length_unit(e: &HashMap<u64, Entity>) -> (f64, String) {
    let mut ids: Vec<&u64> = e.keys().collect();
    ids.sort();
    for id in &ids {
        if let Some(g) = e[id].part("GLOBAL_UNIT_ASSIGNED_CONTEXT") {
            for u in g.first().map(P::list).unwrap_or(&[]) { if let Some(f) = u.rf().and_then(|u| unit_factor(e, u, 0)) { return f; } }
        }
    }
    for id in ids { if let Some(f) = unit_factor(e, *id, 0) { return f; } }
    (1.0, "mm".into())
}

/// Product/part name from PRODUCT('id','name',...).
fn product_name(e: &HashMap<u64, Entity>) -> String {
    let mut ids: Vec<&u64> = e.keys().collect();
    ids.sort();
    ids.into_iter().filter_map(|i| e[i].part("PRODUCT")).find_map(|a| { let n = a.first().map(P::text).unwrap_or_default(); if n.trim().is_empty() { None } else { Some(n) } }).unwrap_or_else(|| "IMPORT".into())
}

/// World transforms of every occurrence of representation `rep` (assemblies).
fn instance_xforms(r: &Reader, rep: u64) -> Vec<Xform> {
    let e = r.e;
    // representation -> items (to find which placement belongs to which representation)
    let items_of = |rid: u64| -> Vec<u64> { e.get(&rid).map(|x| x.parts.iter().find(|p| p.0.ends_with("REPRESENTATION")).map(|p| p.1.get(1).map(|l| l.list().iter().filter_map(P::rf).collect()).unwrap_or_default()).unwrap_or_default()).unwrap_or_default() };
    // identity links (SHAPE_REPRESENTATION_RELATIONSHIP without transformation)
    let mut same: HashMap<u64, Vec<u64>> = HashMap::new();
    // occurrence links: child rep -> (parent rep, transform child->parent)
    let mut up: HashMap<u64, Vec<(u64, Xform)>> = HashMap::new();
    let mut ids: Vec<&u64> = e.keys().collect();
    ids.sort();
    for id in ids {
        let x = &e[id];
        let Some(rr) = x.part("REPRESENTATION_RELATIONSHIP").or_else(|| x.part("SHAPE_REPRESENTATION_RELATIONSHIP")) else { continue };
        let (Some(r1), Some(r2)) = (rr.get(2).and_then(P::rf), rr.get(3).and_then(P::rf)) else { continue };
        match x.part("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION").and_then(|t| t.first()).and_then(P::rf).and_then(|t| e.get(&t)).and_then(|t| t.part("ITEM_DEFINED_TRANSFORMATION")) {
            Some(t) => {
                let (Some(i1), Some(i2)) = (t.get(2).and_then(P::rf), t.get(3).and_then(P::rf)) else { continue };
                let (Some(a1), Some(a2)) = (r.axis2(i1), r.axis2(i2)) else { continue };
                // the placement that belongs to the child maps onto the one in the parent
                let (child, parent, ac, ap) = if items_of(r1).contains(&i1) || items_of(r2).contains(&i2) { (r1, r2, a1, a2) } else { (r2, r1, a2, a1) };
                let (xc, xp) = (ac.to_xform(), ap.to_xform());
                let inv = xc.m.inverse();
                let m = Xform { m: xp.m * inv, t: xp.t - xp.m * (inv * xc.t) };
                up.entry(child).or_default().push((parent, m));
            }
            None => { same.entry(r1).or_default().push(r2); same.entry(r2).or_default().push(r1); }
        }
    }
    fn walk(rep: u64, up: &HashMap<u64, Vec<(u64, Xform)>>, same: &HashMap<u64, Vec<u64>>, depth: usize, out: &mut Vec<Xform>, acc: Xform) {
        if out.len() > 4000 || depth > 24 { return; }
        // the class of representations sharing coordinates with `rep`
        let mut class = vec![rep];
        let mut k = 0;
        while k < class.len() { for n in same.get(&class[k]).into_iter().flatten() { if !class.contains(n) { class.push(*n); } } k += 1; }
        let links: Vec<&(u64, Xform)> = class.iter().flat_map(|c| up.get(c).into_iter().flatten()).collect();
        if links.is_empty() { out.push(acc); return; }
        for (parent, m) in links {
            let next = Xform { m: m.m * acc.m, t: m.m * acc.t + m.t };
            walk(*parent, up, same, depth + 1, out, next);
        }
    }
    let mut out = vec![];
    walk(rep, &up, &same, 0, &mut out, Xform { m: DMat3::IDENTITY, t: V3::ZERO });
    out
}

/// Reads a STEP file into triangle faces (millimetres).
pub fn import(text: &str) -> Result<ImportResult, String> {
    let e = parse(text)?;
    let (scale, unit) = length_unit(&e);
    let mut bb = BBox::default();
    for x in e.values() { if let Some(a) = x.part("CARTESIAN_POINT") { let c: Vec<f64> = a.get(1).map(|l| l.list().iter().filter_map(P::num).collect()).unwrap_or_default(); if c.len() == 3 { bb.add(V3::new(c[0], c[1], c[2]) * scale); } } }
    let diag = bb.diagonal().max(1.0);
    let mut r = Reader { e: &e, scale, edges: HashMap::new(), tol: (diag * 6e-4).clamp(0.002, 0.5), ang_step: 10f64.to_radians(), face_no: 0 };
    // solids and their shells
    let mut bodies: Vec<(u64, Vec<u64>)> = vec![];
    let mut ids: Vec<u64> = e.keys().copied().collect();
    ids.sort();
    for &id in &ids {
        let x = &e[&id];
        let shells: Vec<u64> = if let Some(a) = x.part("MANIFOLD_SOLID_BREP").or_else(|| x.part("FACETED_BREP")) { a.get(1).and_then(P::rf).into_iter().collect() }
            else if let Some(a) = x.part("BREP_WITH_VOIDS") { a.get(1).and_then(P::rf).into_iter().chain(a.get(2).map(|l| l.list().iter().filter_map(P::rf).collect::<Vec<_>>()).unwrap_or_default()).collect() }
            else if let Some(a) = x.part("SHELL_BASED_SURFACE_MODEL") { a.get(1).map(|l| l.list().iter().filter_map(P::rf).collect()).unwrap_or_default() }
            else { continue };
        bodies.push((id, shells));
    }
    if bodies.is_empty() {
        // loose shells without a solid wrapper
        for &id in &ids { if e[&id].has("CLOSED_SHELL") { bodies.push((id, vec![id])); } }
    }
    if bodies.is_empty() { return Err("Die STEP-Datei enthält keine Volumenkörper (nur Kurven oder Metadaten?).".into()); }
    // representation that owns each body
    let mut owner: HashMap<u64, u64> = HashMap::new();
    for &id in &ids {
        if let Some(p) = e[&id].parts.iter().find(|p| p.0.ends_with("SHAPE_REPRESENTATION")) {
            for it in p.1.get(1).map(P::list).unwrap_or(&[]) { if let Some(i) = it.rf() { owner.insert(i, id); } }
        }
    }
    let mut faces = vec![];
    let mut skipped = 0;
    let mut cache: HashMap<u64, Vec<FaceMesh>> = HashMap::new();
    for (body, shells) in &bodies {
        if !cache.contains_key(body) {
            let mut fm = vec![];
            for s in shells {
                let Some(sh) = e.get(s) else { continue };
                let list = sh.part("CLOSED_SHELL").or_else(|| sh.part("OPEN_SHELL")).and_then(|a| a.get(1)).map(|l| l.list().iter().filter_map(P::rf).collect::<Vec<_>>()).unwrap_or_default();
                for f in list { let res = r.face(f); r.face_no += 1; match res { Ok(m) => fm.push(m), Err(_) => skipped += 1 } }
            }
            cache.insert(*body, fm);
        }
        let xs = match owner.get(body) { Some(rep) => instance_xforms(&r, *rep), None => vec![Xform { m: DMat3::IDENTITY, t: V3::ZERO }] };
        for x in xs {
            let mirror = x.is_mirror();
            for f in &cache[body] {
                let verts = f.verts.iter().map(|(p, n)| (x.point(*p), safe_norm(x.vector(*n), *n))).collect();
                let tris = f.tris.iter().map(|t| if mirror { [t[0], t[2], t[1]] } else { *t }).collect();
                faces.push(FaceMesh { verts, tris, kind: f.kind, proj_err: f.proj_err });
            }
        }
    }
    if faces.is_empty() { return Err("Keine Fläche der STEP-Datei konnte trianguliert werden.".into()); }
    Ok(ImportResult { name: product_name(&e), faces, solids: bodies.len(), skipped, unit })
}

// ---------------------------------------------------------------- STL / OBJ

/// Reads binary or ASCII STL into one face per connected set of triangles (single face).
pub fn import_stl(bytes: &[u8]) -> Result<ImportResult, String> {
    let mut tris: Vec<[V3; 3]> = vec![];
    let ascii = bytes.starts_with(b"solid") && bytes.windows(5).take(2000).any(|w| w == b"facet");
    if ascii {
        let t = String::from_utf8_lossy(bytes);
        let mut cur = vec![];
        for l in t.lines() {
            let mut it = l.split_whitespace();
            if it.next() == Some("vertex") {
                let c: Vec<f64> = it.filter_map(|x| x.parse().ok()).collect();
                if c.len() == 3 { cur.push(V3::new(c[0], c[1], c[2])); }
                if cur.len() == 3 { tris.push([cur[0], cur[1], cur[2]]); cur.clear(); }
            }
        }
    } else {
        if bytes.len() < 84 { return Err("Die STL-Datei ist zu kurz.".into()); }
        let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        if bytes.len() < 84 + n * 50 { return Err("Die STL-Datei ist unvollständig.".into()); }
        let f = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as f64;
        for i in 0..n {
            let b = 84 + i * 50 + 12;
            tris.push([V3::new(f(b), f(b + 4), f(b + 8)), V3::new(f(b + 12), f(b + 16), f(b + 20)), V3::new(f(b + 24), f(b + 28), f(b + 32))]);
        }
    }
    if tris.is_empty() { return Err("Die STL-Datei enthält keine Dreiecke.".into()); }
    Ok(ImportResult { name: "IMPORT".into(), faces: mesh_faces(&tris), solids: 1, skipped: 0, unit: "mm".into() })
}

pub fn import_obj(text: &str) -> Result<ImportResult, String> {
    let mut pos: Vec<V3> = vec![];
    let mut tris = vec![];
    let mut name = String::from("IMPORT");
    for l in text.lines() {
        let mut it = l.split_whitespace();
        match it.next() {
            Some("v") => { let c: Vec<f64> = it.filter_map(|x| x.parse().ok()).collect(); if c.len() >= 3 { pos.push(V3::new(c[0], c[1], c[2])); } }
            Some("o") => { if let Some(n) = it.next() { name = n.to_string(); } }
            Some("f") => {
                let idx: Vec<usize> = it.filter_map(|x| x.split('/').next()?.parse::<i64>().ok()).map(|i| if i < 0 { (pos.len() as i64 + i) as usize } else { (i - 1) as usize }).collect();
                for k in 1..idx.len().saturating_sub(1) {
                    if let (Some(a), Some(b), Some(c)) = (pos.get(idx[0]), pos.get(idx[k]), pos.get(idx[k + 1])) { tris.push([*a, *b, *c]); }
                }
            }
            _ => {}
        }
    }
    if tris.is_empty() { return Err("Die OBJ-Datei enthält keine Flächen.".into()); }
    Ok(ImportResult { name, faces: mesh_faces(&tris), solids: 1, skipped: 0, unit: "mm".into() })
}

/// Groups mesh triangles into faces: flat regions become one face each (so planes are
/// selectable and sketchable), curved regions stay one face per smooth patch.
fn mesh_faces(tris: &[[V3; 3]]) -> Vec<FaceMesh> {
    // weld vertices
    let mut bb = BBox::default();
    for t in tris { for p in t { bb.add(*p); } }
    let q = (bb.diagonal() * 1e-7).max(1e-9);
    let mut index: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut pts: Vec<V3> = vec![];
    let mut it: Vec<[u32; 3]> = vec![];
    for t in tris {
        let mut ids = [0u32; 3];
        for (k, p) in t.iter().enumerate() {
            let key = ((p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64);
            ids[k] = *index.entry(key).or_insert_with(|| { pts.push(*p); (pts.len() - 1) as u32 });
        }
        if ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2] { it.push(ids); }
    }
    let normals: Vec<V3> = it.iter().map(|t| safe_norm((pts[t[1] as usize] - pts[t[0] as usize]).cross(pts[t[2] as usize] - pts[t[0] as usize]), V3::Z)).collect();
    let mut edge: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (i, t) in it.iter().enumerate() { for k in 0..3 { let (a, b) = (t[k], t[(k + 1) % 3]); edge.entry((a.min(b), a.max(b))).or_default().push(i); } }
    let mut group = vec![usize::MAX; it.len()];
    let mut g = 0;
    for s in 0..it.len() {
        if group[s] != usize::MAX { continue; }
        group[s] = g;
        let mut stack = vec![s];
        while let Some(t) = stack.pop() {
            for k in 0..3 {
                let (a, b) = (it[t][k], it[t][(k + 1) % 3]);
                for &n in &edge[&(a.min(b), a.max(b))] {
                    // same face if (nearly) coplanar with the seed — keeps flat faces as one
                    if group[n] == usize::MAX && normals[n].dot(normals[s]) > 0.9999 { group[n] = g; stack.push(n); }
                }
            }
        }
        g += 1;
    }
    let mut faces: Vec<FaceMesh> = (0..g).map(|_| FaceMesh { verts: vec![], tris: vec![], kind: "mesh", proj_err: 0.0 }).collect();
    let mut local: Vec<HashMap<u32, u32>> = vec![HashMap::new(); g];
    for (i, t) in it.iter().enumerate() {
        let gi = group[i];
        let mut tri = [0u32; 3];
        for k in 0..3 {
            let f = &mut faces[gi];
            tri[k] = *local[gi].entry(t[k]).or_insert_with(|| { f.verts.push((pts[t[k] as usize], normals[i])); (f.verts.len() - 1) as u32 });
        }
        faces[gi].tris.push(tri);
    }
    faces
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_complex_entities_and_strings() {
        let e = parse("ISO-10303-21;\nDATA;\n#1=CARTESIAN_POINT('it''s',(1.,2.,3.E0));\n#2=(A(1)B(.T.,#1));\nENDSEC;").unwrap();
        assert_eq!(e[&1].ty(), "CARTESIAN_POINT");
        assert_eq!(e[&1].args()[0].text(), "it's");
        assert!(e[&2].part("B").unwrap()[0].boolean());
    }
}
