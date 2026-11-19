//! 2D helpers shared by the schematic and the board: points, rotations, distances and the
//! "rounded convex hull" shape used for copper clearance checks.

use glam::DVec2;

pub type P = DVec2;

pub fn p(x: f64, y: f64) -> P {
    DVec2::new(x, y)
}

/// Rotates `v` by `deg` degrees counter-clockwise in a y-down frame as KiCad does
/// (i.e. visually counter-clockwise on screen).
pub fn rot(v: P, deg: f64) -> P {
    let a = -deg.to_radians();
    let (s, c) = a.sin_cos();
    let r = p(v.x * c - v.y * s, v.x * s + v.y * c);
    // snap the usual right angles exactly
    p(snap(r.x), snap(r.y))
}

fn snap(x: f64) -> f64 {
    let k = (x * 1e6).round() / 1e6;
    if k == 0.0 {
        0.0
    } else {
        k
    }
}

/// Distance from point `q` to segment a–b.
pub fn pt_seg(q: P, a: P, b: P) -> f64 {
    let ab = b - a;
    let l = ab.length_squared();
    if l < 1e-18 {
        return (q - a).length();
    }
    let t = ((q - a).dot(ab) / l).clamp(0.0, 1.0);
    (a + ab * t - q).length()
}

/// Closest point of segment a–b to `q`.
pub fn closest_on_seg(q: P, a: P, b: P) -> P {
    let ab = b - a;
    let l = ab.length_squared();
    if l < 1e-18 {
        return a;
    }
    let t = ((q - a).dot(ab) / l).clamp(0.0, 1.0);
    a + ab * t
}

fn cross(a: P, b: P) -> f64 {
    a.x * b.y - a.y * b.x
}

/// True when the segments a–b and c–d intersect (touching counts).
pub fn segs_cross(a: P, b: P, c: P, d: P) -> bool {
    let d1 = cross(b - a, c - a);
    let d2 = cross(b - a, d - a);
    let d3 = cross(d - c, a - c);
    let d4 = cross(d - c, b - c);
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    let on = |p0: P, p1: P, q: P| pt_seg(q, p0, p1) < 1e-9;
    on(a, b, c) || on(a, b, d) || on(c, d, a) || on(c, d, b)
}

/// Distance between segments a–b and c–d.
pub fn seg_seg(a: P, b: P, c: P, d: P) -> f64 {
    if segs_cross(a, b, c, d) {
        return 0.0;
    }
    pt_seg(a, c, d).min(pt_seg(b, c, d)).min(pt_seg(c, a, b)).min(pt_seg(d, a, b))
}

/// Point in (any) polygon, even-odd.
pub fn in_poly(q: P, poly: &[P]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Signed area (positive = counter-clockwise in a y-up frame).
pub fn area(poly: &[P]) -> f64 {
    let n = poly.len();
    (0..n).map(|i| cross(poly[i], poly[(i + 1) % n])).sum::<f64>() / 2.0
}

pub fn bbox(pts: impl IntoIterator<Item = P>) -> Option<(P, P)> {
    let mut it = pts.into_iter();
    let f = it.next()?;
    Some(it.fold((f, f), |(lo, hi), q| (lo.min(q), hi.max(q))))
}

/// A convex polygon (1 point = disc, 2 = capsule, ≥3 = polygon) grown by radius `r`: every
/// pad, track and via is one of these, which makes clearances one distance formula.
#[derive(Clone, Debug, PartialEq)]
pub struct Hull {
    pub pts: Vec<P>,
    pub r: f64,
}

impl Hull {
    pub fn disc(c: P, r: f64) -> Hull {
        Hull { pts: vec![c], r }
    }
    pub fn capsule(a: P, b: P, r: f64) -> Hull {
        Hull { pts: vec![a, b], r }
    }
    fn edges(&self) -> Vec<(P, P)> {
        let n = self.pts.len();
        match n {
            0 => vec![],
            1 => vec![(self.pts[0], self.pts[0])],
            2 => vec![(self.pts[0], self.pts[1])],
            _ => (0..n).map(|i| (self.pts[i], self.pts[(i + 1) % n])).collect(),
        }
    }
    /// Distance between the outlines (0 when they overlap).
    pub fn dist(&self, o: &Hull) -> f64 {
        let core = hull_core_dist(self, o);
        (core - self.r - o.r).max(0.0)
    }
    /// Signed gap: negative when the shapes overlap by that much (approximately).
    pub fn gap(&self, o: &Hull) -> f64 {
        hull_core_dist(self, o) - self.r - o.r
    }
    pub fn contains(&self, q: P) -> bool {
        if self.pts.len() >= 3 && in_poly(q, &self.pts) {
            return true;
        }
        self.edges().iter().any(|(a, b)| pt_seg(q, *a, *b) <= self.r + 1e-9)
    }
    pub fn bbox(&self) -> (P, P) {
        let (lo, hi) = bbox(self.pts.iter().copied()).unwrap_or((P::ZERO, P::ZERO));
        (lo - P::splat(self.r), hi + P::splat(self.r))
    }
    /// Outline as a polygon (arcs as `seg` segments per full turn), counter-clockwise in a
    /// y-up frame.
    pub fn polygon(&self, seg: usize) -> Vec<P> {
        if self.r <= 1e-9 {
            return convex_hull(self.pts.clone());
        }
        let mut cloud = vec![];
        for c in &self.pts {
            for i in 0..seg {
                let a = i as f64 / seg as f64 * std::f64::consts::TAU;
                cloud.push(*c + p(a.cos(), a.sin()) * self.r);
            }
        }
        convex_hull(cloud)
    }
}

/// Convex hull (Andrew's monotone chain), counter-clockwise in a y-up frame.
pub fn convex_hull(mut pts: Vec<P>) -> Vec<P> {
    pts.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap().then(a.y.partial_cmp(&b.y).unwrap()));
    pts.dedup_by(|a, b| (*a - *b).length() < 1e-12);
    if pts.len() < 3 {
        return pts;
    }
    let mut lower: Vec<P> = vec![];
    for q in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 1] - lower[lower.len() - 2], *q - lower[lower.len() - 2]) <= 0.0 {
            lower.pop();
        }
        lower.push(*q);
    }
    let mut upper: Vec<P> = vec![];
    for q in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 1] - upper[upper.len() - 2], *q - upper[upper.len() - 2]) <= 0.0 {
            upper.pop();
        }
        upper.push(*q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

fn hull_core_dist(a: &Hull, b: &Hull) -> f64 {
    if a.pts.len() >= 3 && b.pts.iter().any(|q| in_poly(*q, &a.pts)) {
        return 0.0;
    }
    if b.pts.len() >= 3 && a.pts.iter().any(|q| in_poly(*q, &b.pts)) {
        return 0.0;
    }
    let mut d = f64::MAX;
    for (p0, p1) in a.edges() {
        for (q0, q1) in b.edges() {
            d = d.min(seg_seg(p0, p1, q0, q1));
        }
    }
    d
}

/// Flattens an arc from `a` through `mid` to `b` into points (including both ends).
pub fn arc3(a: P, mid: P, b: P, seg_per_turn: usize) -> Vec<P> {
    let Some(c) = circumcenter(a, mid, b) else { return vec![a, b] };
    let r = (a - c).length();
    let ang = |q: P| (q.y - c.y).atan2(q.x - c.x);
    let (a0, am, mut a1) = (ang(a), ang(mid), ang(b));
    // choose the direction that passes through mid
    let tau = std::f64::consts::TAU;
    let norm = |x: f64| ((x % tau) + tau) % tau;
    let ccw = norm(am - a0) < norm(a1 - a0);
    if ccw {
        while a1 < a0 {
            a1 += tau;
        }
    } else {
        while a1 > a0 {
            a1 -= tau;
        }
    }
    let n = (((a1 - a0).abs() / tau) * seg_per_turn as f64).ceil().max(2.0) as usize;
    (0..=n).map(|i| c + p((a0 + (a1 - a0) * i as f64 / n as f64).cos(), (a0 + (a1 - a0) * i as f64 / n as f64).sin()) * r).collect()
}

pub fn circumcenter(a: P, b: P, c: P) -> Option<P> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (a.length_squared(), b.length_squared(), c.length_squared());
    Some(p((a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d, (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d))
}

/// Formats a millimetre value the German way ("0,25").
pub fn mm(v: f64) -> String {
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        return "0".into();
    }
    s.replace('.', ",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_distances() {
        let a = Hull::disc(p(0.0, 0.0), 1.0);
        let b = Hull::disc(p(3.0, 0.0), 1.0);
        assert!((a.dist(&b) - 1.0).abs() < 1e-9);
        let r = Hull { pts: vec![p(-1.0, -1.0), p(1.0, -1.0), p(1.0, 1.0), p(-1.0, 1.0)], r: 0.0 };
        let t = Hull::capsule(p(-5.0, 2.5), p(5.0, 2.5), 0.25);
        assert!((r.dist(&t) - 1.25).abs() < 1e-9);
        assert_eq!(r.dist(&Hull::disc(p(0.2, 0.2), 0.1)), 0.0);
        assert!((rot(p(1.0, 0.0), 90.0) - p(0.0, -1.0)).length() < 1e-9);
    }

    #[test]
    fn hull_polygon_is_closed_outline() {
        let h = Hull::capsule(p(0.0, 0.0), p(4.0, 0.0), 1.0);
        let poly = h.polygon(32);
        assert!(poly.len() > 10);
        assert!((area(&poly).abs() - (8.0 + std::f64::consts::PI)).abs() < 0.1, "{}", area(&poly));
    }
}
