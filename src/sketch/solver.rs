//! Levenberg–Marquardt geometric constraint solver for sketches.

use super::sketch::*;
use crate::kernel::math::*;
use std::collections::HashMap;

struct Ctx<'a> {
    sk: &'a Sketch,
    point_base: HashMap<i64, usize>,
    radius_idx: HashMap<i64, usize>,
    ents: HashMap<i64, &'a SkEntity>,
    n: usize,
}

fn ctx(sk: &Sketch) -> Ctx<'_> {
    let mut point_base = HashMap::new();
    let mut radius_idx = HashMap::new();
    let mut n = 0;
    for p in &sk.points { if p.id != 0 { point_base.insert(p.id, n); n += 2; } }
    for e in &sk.entities { if matches!(e.kind, SkKind::Circle | SkKind::Ellipse | SkKind::Conic) { radius_idx.insert(e.id, n); n += 1; } }
    let ents = sk.entities.iter().map(|e| (e.id, e)).collect();
    Ctx { sk, point_base, radius_idx, ents, n }
}

fn pack(c: &Ctx) -> Vec<f64> {
    let mut x = vec![0.0; c.n];
    for p in &c.sk.points { if let Some(&b) = c.point_base.get(&p.id) { x[b] = p.p.x; x[b + 1] = p.p.y; } }
    for e in &c.sk.entities { if let Some(&i) = c.radius_idx.get(&e.id) { x[i] = e.r; } }
    x
}

fn residuals(c: &Ctx, x: &[f64], drag: Option<(i64, V2, f64)>) -> Vec<f64> {
    let p = |id: i64| -> V2 {
        if id == 0 { return V2::ZERO; }
        match c.point_base.get(&id) { Some(&b) => V2::new(x[b], x[b + 1]), None => c.sk.point(id) }
    };
    let center = |id: i64| p(c.ents.get(&id).map(|e| e.pts[0]).unwrap_or(0));
    let rr = |id: i64| -> f64 {
        match c.ents.get(&id) {
            Some(e) if matches!(e.kind, SkKind::Circle | SkKind::Ellipse | SkKind::Conic) => x[c.radius_idx[&id]],
            Some(e) if e.kind == SkKind::Arc => (p(e.pts[1]) - p(e.pts[0])).length(),
            _ => 0.0,
        }
    };
    let line = |id: i64| -> (V2, V2) {
        if id == REF_H { return (V2::ZERO, V2::X); }
        if id == REF_V { return (V2::ZERO, V2::Y); }
        if id <= -10 { return c.sk.ref_lines.get((-10 - id) as usize).map(|l| (l[0], l[1])).unwrap_or((V2::ZERO, V2::X)); }
        match c.ents.get(&id) { Some(e) if e.kind == SkKind::Line => (p(e.pts[0]), p(e.pts[1])), _ => (V2::ZERO, V2::X) }
    };
    let is_line = |id: i64| id < 0 || c.ents.get(&id).map(|e| e.kind == SkKind::Line).unwrap_or(false);
    let dist_pl = |q: V2, (a, b): (V2, V2)| cross2(b - a, q - a) / (b - a).length().max(1e-12);

    let mut r = Vec::with_capacity(c.sk.constraints.len() * 2 + 4);
    for e in &c.sk.entities {
        if e.kind == SkKind::Arc { r.push((p(e.pts[1]) - p(e.pts[0])).length() - (p(e.pts[2]) - p(e.pts[0])).length()); }
    }
    for k in &c.sk.constraints {
        use SkCType::*;
        if k.weak || k.reference || k.var { continue; }
        match k.ty {
            Coincident if k.pts.len() == 2 => { let d = p(k.pts[0]) - p(k.pts[1]); r.push(d.x); r.push(d.y); }
            Horizontal => {
                if let Some(&l) = k.ents.first() { let (a, b) = line(l); r.push(a.y - b.y); }
                else if k.pts.len() == 2 { r.push(p(k.pts[0]).y - p(k.pts[1]).y); }
            }
            Vertical => {
                if let Some(&l) = k.ents.first() { let (a, b) = line(l); r.push(a.x - b.x); }
                else if k.pts.len() == 2 { r.push(p(k.pts[0]).x - p(k.pts[1]).x); }
            }
            Parallel if k.ents.len() == 2 => {
                let ((a1, b1), (a2, b2)) = (line(k.ents[0]), line(k.ents[1]));
                r.push(cross2(safe_norm2(b1 - a1, V2::X), safe_norm2(b2 - a2, V2::X)));
            }
            Perpendicular if k.ents.len() == 2 => {
                let ((a1, b1), (a2, b2)) = (line(k.ents[0]), line(k.ents[1]));
                r.push(safe_norm2(b1 - a1, V2::X).dot(safe_norm2(b2 - a2, V2::X)));
            }
            Equal if k.ents.len() == 2 => {
                if is_line(k.ents[0]) && is_line(k.ents[1]) {
                    let ((a1, b1), (a2, b2)) = (line(k.ents[0]), line(k.ents[1]));
                    r.push((b1 - a1).length() - (b2 - a2).length());
                } else {
                    r.push(rr(k.ents[0]) - rr(k.ents[1]));
                }
            }
            Tangent if k.ents.len() == 2 => {
                let (e0, e1) = (k.ents[0], k.ents[1]);
                if is_line(e0) || is_line(e1) {
                    let (l, ci) = if is_line(e0) { (e0, e1) } else { (e1, e0) };
                    r.push(dist_pl(center(ci), line(l)).abs() - rr(ci));
                } else {
                    let d = (center(e0) - center(e1)).length();
                    r.push(if k.aux == 0.0 { d - (rr(e0) + rr(e1)) } else { d - (rr(e0) - rr(e1)).abs() });
                }
            }
            PointOn if !k.pts.is_empty() && !k.ents.is_empty() => {
                let (q, e) = (p(k.pts[0]), k.ents[0]);
                if is_line(e) { r.push(dist_pl(q, line(e))); } else { r.push((q - center(e)).length() - rr(e)); }
            }
            Midpoint if !k.pts.is_empty() && !k.ents.is_empty() => {
                let (a, b) = line(k.ents[0]);
                let d = p(k.pts[0]) - (a + b) * 0.5;
                r.push(d.x); r.push(d.y);
            }
            Symmetric if k.pts.len() == 2 && !k.ents.is_empty() => {
                let (a, b) = line(k.ents[0]);
                let u = safe_norm2(b - a, V2::X);
                let (p1, p2) = (p(k.pts[0]), p(k.pts[1]));
                r.push(cross2(u, (p1 + p2) * 0.5 - a));
                r.push((p2 - p1).dot(u));
            }
            Fix if !k.pts.is_empty() => {
                if let Some(f) = k.fixed { let d = p(k.pts[0]) - f; r.push(d.x); r.push(d.y); }
            }
            Distance if k.pts.len() == 2 => r.push((p(k.pts[1]) - p(k.pts[0])).length() - k.value),
            HDistance if k.pts.len() == 2 => r.push((p(k.pts[1]).x - p(k.pts[0]).x).abs() - k.value),
            VDistance if k.pts.len() == 2 => r.push((p(k.pts[1]).y - p(k.pts[0]).y).abs() - k.value),
            PointLine if !k.pts.is_empty() && !k.ents.is_empty() => r.push(dist_pl(p(k.pts[0]), line(k.ents[0])).abs() - k.value),
            Radius if !k.ents.is_empty() => r.push(rr(k.ents[0]) - k.value),
            Diameter if !k.ents.is_empty() => r.push(2.0 * rr(k.ents[0]) - k.value),
            Rho if !k.ents.is_empty() => r.push(rr(k.ents[0]) - k.value),
            Perimeter if !k.ents.is_empty() => {
                let mut total = 0.0;
                for id in &k.ents {
                    let Some(e) = c.ents.get(id) else { continue };
                    total += match e.kind {
                        SkKind::Line => (p(e.pts[1]) - p(e.pts[0])).length(),
                        SkKind::Circle => std::f64::consts::TAU * rr(*id),
                        SkKind::Arc => {
                            let (cc, s, t) = (p(e.pts[0]), p(e.pts[1]), p(e.pts[2]));
                            let mut sw = (t.y - cc.y).atan2(t.x - cc.x) - (s.y - cc.y).atan2(s.x - cc.x);
                            if sw < 0.0 { sw += std::f64::consts::TAU; }
                            (s - cc).length() * sw
                        }
                        _ => c.sk.length(e),
                    };
                }
                r.push(total - k.value);
            }
            Ordinate if !k.pts.is_empty() => {
                if let Some(b) = c.sk.constraints.iter().find(|x| x.id == k.base) {
                    let d = crate::sketch::sketch::ordinate(p(k.pts[0]), b, |id| Some(line(id)), |id| p(id));
                    r.push(d - k.value);
                }
            }
            Angle if k.ents.len() == 2 => {
                let ((a1, b1), (a2, b2)) = (line(k.ents[0]), line(k.ents[1]));
                let d1 = b1 - a1;
                let d2 = if k.aux != 0.0 { a2 - b2 } else { b2 - a2 };
                r.push(cross2(d1, d2).abs().atan2(d1.dot(d2)) - k.value.to_radians());
            }
            _ => {}
        }
    }
    if let Some((pid, target, w)) = drag {
        let d = (p(pid) - target) * w;
        r.push(d.x); r.push(d.y);
    }
    r
}

fn jacobian(c: &Ctx, x: &[f64], r0: &[f64], drag: Option<(i64, V2, f64)>) -> Vec<f64> {
    let (n, m) = (x.len(), r0.len());
    let mut j = vec![0.0; m * n];
    let mut xx = x.to_vec();
    for col in 0..n {
        let h = 1e-7 * x[col].abs().max(1.0);
        xx[col] = x[col] + h;
        let r1 = residuals(c, &xx, drag);
        xx[col] = x[col];
        for i in 0..m { j[i * n + col] = (r1[i] - r0[i]) / h; }
    }
    j
}

fn unpack(sk: &mut Sketch, point_base: &HashMap<i64, usize>, radius_idx: &HashMap<i64, usize>, x: &[f64]) {
    for p in &mut sk.points { if let Some(&b) = point_base.get(&p.id) { p.p = V2::new(x[b], x[b + 1]); } }
    for e in &mut sk.entities { if let Some(&i) = radius_idx.get(&e.id) { e.r = x[i].abs(); } }
}

fn max_residual(sk: &Sketch) -> f64 {
    let c = ctx(sk);
    let x = pack(&c);
    residuals(&c, &x, None).iter().fold(0.0f64, |m, v| m.max(v.abs()))
}

/// Solve in place; returns true when all constraints are satisfied.
pub fn solve(sk: &mut Sketch, drag: Option<(i64, V2)>) -> bool {
    let (x, pb, ri) = {
        let c = ctx(sk);
        let n = c.n;
        if n == 0 { return true; }
        let dragw = drag.map(|(a, b)| (a, b, 0.08));
        let mut x = pack(&c);
        let mut lambda = 1e-6;
        let mut r = residuals(&c, &x, dragw);
        let mut cost: f64 = r.iter().map(|v| v * v).sum();
        let mut iter = 0;
        while iter < 80 && cost > 1e-22 {
            iter += 1;
            let m = r.len();
            let jm = jacobian(&c, &x, &r, dragw);
            let mut a = vec![0.0; n * n];
            let mut g = vec![0.0; n];
            for i in 0..m {
                let row = &jm[i * n..(i + 1) * n];
                for p in 0..n {
                    let jp = row[p];
                    if jp == 0.0 { continue; }
                    g[p] += jp * r[i];
                    for q in p..n { if row[q] != 0.0 { a[p * n + q] += jp * row[q]; } }
                }
            }
            for p in 0..n { for q in 0..p { a[p * n + q] = a[q * n + p]; } }
            let mut improved = false;
            for _ in 0..12 {
                let mut ad = a.clone();
                for p in 0..n { ad[p * n + p] += lambda * (1.0 + a[p * n + p]) + 1e-12; }
                let neg: Vec<f64> = g.iter().map(|v| -v).collect();
                let Some(d) = solve_dense(&ad, &neg, n) else { lambda *= 10.0; continue };
                let xn: Vec<f64> = x.iter().zip(&d).map(|(a, b)| a + b).collect();
                let rn = residuals(&c, &xn, dragw);
                let cn: f64 = rn.iter().map(|v| v * v).sum();
                if cn < cost {
                    x = xn; r = rn; cost = cn;
                    lambda = (lambda / 10.0).max(1e-12);
                    improved = true;
                    break;
                }
                lambda *= 10.0;
                if lambda > 1e10 { break; }
            }
            if !improved { break; }
        }
        (x, c.point_base.clone(), c.radius_idx.clone())
    };
    unpack(sk, &pb, &ri, &x);
    if drag.is_some() { return solve(sk, None); }
    max_residual(sk) < 1e-6
}

/// Greedy independence test: which candidate constraints add a new, independent equation
/// to the (strong) constraint system, accepting them one by one in order.
pub fn independent(sk: &Sketch, cands: &[SkConstraint]) -> Vec<bool> {
    let c = ctx(sk);
    let n = c.n;
    if n == 0 { return vec![false; cands.len()]; }
    let x = pack(&c);
    let r = residuals(&c, &x, None);
    let mut basis: Vec<Vec<f64>> = vec![];
    let add = |row: &[f64], basis: &mut Vec<Vec<f64>>| -> bool {
        let norm0 = row.iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm0 < 1e-12 { return false; }
        let mut v = row.to_vec();
        for b in basis.iter() {
            let d: f64 = v.iter().zip(b).map(|(a, b)| a * b).sum();
            for (vi, bi) in v.iter_mut().zip(b) { *vi -= d * bi; }
        }
        let norm = v.iter().map(|q| q * q).sum::<f64>().sqrt();
        if norm < 1e-6 * norm0.max(1.0) { return false; }
        for vi in v.iter_mut() { *vi /= norm; }
        basis.push(v);
        true
    };
    if !r.is_empty() {
        let j = jacobian(&c, &x, &r, None);
        for i in 0..r.len() { add(&j[i * n..(i + 1) * n], &mut basis); }
    }
    let arcs = sk.entities.iter().filter(|e| e.kind == SkKind::Arc).count();
    let mut out = vec![];
    for cand in cands {
        let mut single = sk.clone();
        let mut k = cand.clone();
        k.weak = false;
        single.constraints = vec![k];
        let cs = ctx(&single);
        let r0 = residuals(&cs, &x, None);
        if r0.len() <= arcs { out.push(false); continue; }
        let mut row = vec![0.0; n];
        let mut xx = x.clone();
        for col in 0..n {
            let h = 1e-7 * x[col].abs().max(1.0);
            xx[col] = x[col] + h;
            let r1 = residuals(&cs, &xx, None);
            xx[col] = x[col];
            row[col] = (r1[arcs] - r0[arcs]) / h;
        }
        out.push(add(&row, &mut basis));
    }
    out
}

/// Regenerate weak dimensions so the sketch is fully dimensioned, keeping label positions.
pub fn refresh_weak(sk: &mut Sketch, off: f64) {
    let old: std::collections::HashMap<_, V2> = sk.constraints.iter().filter(|c| c.weak).map(|c| (c.key(), c.label)).collect();
    sk.constraints.retain(|c| !c.weak);
    let mut cands = sk.weak_candidates(off);
    for c in &mut cands {
        c.value = sk.measure(c);
        if let Some(l) = old.get(&c.key()) { c.label = *l; }
    }
    cands.retain(|c| c.value.is_finite() && c.value > 1e-7);
    let ok = independent(sk, &cands);
    for (mut c, k) in cands.into_iter().zip(ok) {
        if k { c.weak = true; sk.add_constraint(c); }
    }
}

/// Update the values shown by weak dimensions after geometry moved.
pub fn update_weak_values(sk: &mut Sketch) {
    let vals: Vec<(usize, f64)> = sk.constraints.iter().enumerate().filter(|(_, c)| c.weak || c.reference || c.var).map(|(i, c)| (i, sk.measure(c))).collect();
    for (i, v) in vals { sk.constraints[i].value = v; }
}

pub fn rank(sk: &Sketch) -> usize {
    let c = ctx(sk);
    if c.n == 0 { return 0; }
    let x = pack(&c);
    let r = residuals(&c, &x, None);
    if r.is_empty() { return 0; }
    matrix_rank(&jacobian(&c, &x, &r, None), r.len(), c.n, 1e-6)
}

/// Remaining degrees of freedom.
pub fn dof(sk: &Sketch) -> usize {
    let n = ctx(sk).n;
    n - rank(sk).min(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rectangle_dimensions() {
        let mut s = Sketch::rect(10.0, 5.0, 3.0, 1.0);
        let l1 = s.entities[0].id;
        let (a, b) = (s.entities[0].pts[0], s.entities[0].pts[1]);
        let mut d = SkConstraint::new(SkCType::Distance, vec![a, b], vec![l1]);
        d.value = 40.0;
        s.add_constraint(d);
        assert!(solve(&mut s, None));
        assert!((s.point(b) - s.point(a)).length() - 40.0 < 1e-6);
        assert_eq!(dof(&s), 8 - 4 - 1);
    }

    #[test]
    fn weak_dimensions_fully_define_rectangle() {
        let mut s = Sketch::rect(40.0, 20.0, 30.0, 15.0);
        refresh_weak(&mut s, 5.0);
        let weak: Vec<_> = s.constraints.iter().filter(|c| c.weak).collect();
        assert_eq!(weak.len(), 4, "{:?}", weak.iter().map(|c| c.ty).collect::<Vec<_>>());
        assert_eq!(weak.iter().filter(|c| c.ty == SkCType::Distance).count(), 2);
        assert_eq!(weak.iter().filter(|c| c.ty == SkCType::PointLine).count(), 2);
        // weak dims never drive the solver
        assert_eq!(dof(&s), 4);
        // a strong width dimension replaces the weak one
        let l = s.entities[0].clone();
        let mut d = SkConstraint::new(SkCType::Distance, l.pts.clone(), vec![l.id]);
        d.value = 40.0;
        s.add_constraint(d);
        refresh_weak(&mut s, 5.0);
        assert_eq!(s.constraints.iter().filter(|c| c.weak).count(), 3);
        // circle: diameter + two centre positions
        let mut c = Sketch::circle(5.0, 10.0, 8.0);
        refresh_weak(&mut c, 2.0);
        assert_eq!(c.constraints.iter().filter(|c| c.weak).count(), 3);
    }
}
