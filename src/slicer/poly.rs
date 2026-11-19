//! 2D polygons for the slicer: booleans, offsets and line clipping on a fixed 0.1 µm grid
//! (through `i_overlay`), plus the small helpers the slicer needs (area, point in polygon,
//! simplification, nearest points).
//!
//! A `Shape` is one outer contour (counter-clockwise) followed by its holes (clockwise);
//! `Shapes` is a set of disjoint shapes. All lengths are millimetres.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::clip::FloatClip;
use i_overlay::float::scale::FixedScaleFloatOverlay;
use i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_overlay::mesh::float::style::{LineJoin, OutlineStyle};
use i_overlay::string::clip::ClipRule;

pub type Pt = [f64; 2];
pub type Contour = Vec<Pt>;
pub type Shape = Vec<Contour>;
pub type Shapes = Vec<Shape>;

/// Float-to-integer scale of every polygon operation: 1e4 per mm (0.1 µm grid). Keeps
/// coordinates of beds up to several metres well inside the engine's integer range.
pub const SCALE: f64 = 1e4;

fn rule_fill(even_odd: bool) -> FillRule {
    if even_odd {
        FillRule::EvenOdd
    } else {
        FillRule::NonZero
    }
}

fn clean(s: Shapes) -> Shapes {
    s.into_iter()
        .map(|sh| sh.into_iter().filter(|c| c.len() >= 3).collect::<Shape>())
        .filter(|sh: &Shape| !sh.is_empty() && sh[0].len() >= 3)
        .collect()
}

fn finite(c: &Contour) -> bool {
    c.iter().all(|p| p[0].is_finite() && p[1].is_finite())
}

/// Region enclosed by a set of closed loops (non-zero winding: loops in either direction fill,
/// holes are loops running the other way inside them).
pub fn from_loops(loops: &[Contour]) -> Shapes {
    let loops: Vec<Contour> = loops.iter().filter(|c| c.len() >= 3 && finite(c)).cloned().collect();
    if loops.is_empty() {
        return vec![];
    }
    let empty: Vec<Contour> = vec![];
    match loops.overlay_with_fixed_scale(&empty, OverlayRule::Subject, FillRule::NonZero, SCALE) {
        Ok(s) => clean(s),
        Err(_) => vec![],
    }
}

fn flat(s: &Shapes) -> Vec<Contour> {
    s.iter().flat_map(|sh| sh.iter().cloned()).collect()
}

fn op(a: &Shapes, b: &Shapes, rule: OverlayRule) -> Shapes {
    if a.is_empty() {
        return match rule {
            OverlayRule::Union | OverlayRule::Xor | OverlayRule::InverseDifference => b.clone(),
            _ => vec![],
        };
    }
    if b.is_empty() {
        return match rule {
            OverlayRule::Union | OverlayRule::Xor | OverlayRule::Difference | OverlayRule::Subject => a.clone(),
            _ => vec![],
        };
    }
    // shapes are disjoint with properly oriented holes: even-odd and non-zero agree, but
    // even-odd is immune to any stray orientation
    match flat(a).overlay_with_fixed_scale(&flat(b), rule, rule_fill(true), SCALE) {
        Ok(s) => clean(s),
        Err(_) => vec![],
    }
}

pub fn union(a: &Shapes, b: &Shapes) -> Shapes {
    op(a, b, OverlayRule::Union)
}

pub fn diff(a: &Shapes, b: &Shapes) -> Shapes {
    op(a, b, OverlayRule::Difference)
}

pub fn inter(a: &Shapes, b: &Shapes) -> Shapes {
    op(a, b, OverlayRule::Intersect)
}

/// Union of many shape sets.
pub fn union_all(sets: &[&Shapes]) -> Shapes {
    let all: Vec<Contour> = sets.iter().flat_map(|s| flat(s)).collect();
    if all.is_empty() {
        return vec![];
    }
    let empty: Vec<Contour> = vec![];
    // the sets overlap each other: non-zero fills the overlaps once
    match all.overlay_with_fixed_scale(&empty, OverlayRule::Subject, FillRule::NonZero, SCALE) {
        Ok(s) => clean(s),
        Err(_) => vec![],
    }
}

/// Grow (`d` > 0) or shrink (`d` < 0) a region. Sharp corners are mitred like Orca's
/// perimeters; `round` gives round joins (brims, skirts, support).
pub fn offset_join(s: &Shapes, d: f64, round: bool) -> Shapes {
    if s.is_empty() {
        return vec![];
    }
    if d.abs() < 0.5 / SCALE {
        return s.clone();
    }
    let join = if round { LineJoin::Round(0.25) } else { LineJoin::Miter(0.35) };
    let style = OutlineStyle::new(d).line_join(join);
    match s.outline_fixed_scale(&style, SCALE) {
        Ok(r) => clean(r),
        Err(_) => vec![],
    }
}

/// Tool-radius offset for machining: round corners on fine arcs (chord error below a
/// thousandth of the radius), so a cutter centred on the result never reaches into `s`
/// by more than that.
pub fn offset_tool(s: &Shapes, d: f64) -> Shapes {
    if s.is_empty() {
        return vec![];
    }
    if d.abs() < 0.5 / SCALE {
        return s.clone();
    }
    let style = OutlineStyle::new(d).line_join(LineJoin::Round(0.05));
    match s.outline_fixed_scale(&style, SCALE) {
        Ok(r) => clean(r),
        Err(_) => vec![],
    }
}

pub fn offset(s: &Shapes, d: f64) -> Shapes {
    offset_join(s, d, false)
}

/// Morphological opening: removes parts thinner than `2 r` and slivers.
pub fn open(s: &Shapes, r: f64) -> Shapes {
    offset(&offset(s, -r), r)
}

/// Morphological closing: fills gaps narrower than `2 r`.
pub fn close(s: &Shapes, r: f64) -> Shapes {
    offset_join(&offset_join(s, r, true), -r, true)
}

/// Parts of the polylines inside the region.
pub fn clip_lines(lines: &[Vec<Pt>], region: &Shapes) -> Vec<Vec<Pt>> {
    clip_lines_rule(lines, region, false)
}

/// Parts of the polylines outside the region (`invert`).
pub fn clip_lines_rule(lines: &[Vec<Pt>], region: &Shapes, invert: bool) -> Vec<Vec<Pt>> {
    let lines: Vec<Vec<Pt>> = lines.iter().filter(|l| l.len() >= 2).cloned().collect();
    if lines.is_empty() {
        return vec![];
    }
    if region.is_empty() {
        return if invert { lines } else { vec![] };
    }
    let rule = ClipRule { invert, boundary_included: false };
    match lines.clip_by_fixed_scale(&flat(region), FillRule::EvenOdd, rule, SCALE) {
        Ok(r) => r.into_iter().filter(|l| l.len() >= 2 && polyline_len(l) > 1e-4).collect(),
        Err(_) => vec![],
    }
}

/// Signed area (counter-clockwise positive).
pub fn contour_area(c: &Contour) -> f64 {
    let n = c.len();
    if n < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    for i in 0..n {
        let p = c[i];
        let q = c[(i + 1) % n];
        a += p[0] * q[1] - q[0] * p[1];
    }
    a * 0.5
}

/// Area of a shape (outer minus holes).
pub fn shape_area(s: &Shape) -> f64 {
    s.iter().map(|c| contour_area(c)).sum::<f64>().abs()
}

pub fn area(s: &Shapes) -> f64 {
    s.iter().map(shape_area).sum()
}

pub fn contour_len(c: &Contour) -> f64 {
    let n = c.len();
    (0..n).map(|i| dist(c[i], c[(i + 1) % n])).sum()
}

pub fn polyline_len(c: &[Pt]) -> f64 {
    c.windows(2).map(|w| dist(w[0], w[1])).sum()
}

#[inline]
pub fn dist(a: Pt, b: Pt) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

#[inline]
pub fn dist2(a: Pt, b: Pt) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

/// Winding-free point in contour test (crossing number).
pub fn in_contour(c: &Contour, p: Pt) -> bool {
    let n = c.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (c[i], c[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0];
            if p[0] < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

pub fn in_shape(s: &Shape, p: Pt) -> bool {
    !s.is_empty() && in_contour(&s[0], p) && !s[1..].iter().any(|h| in_contour(h, p))
}

pub fn in_shapes(s: &Shapes, p: Pt) -> bool {
    s.iter().any(|sh| in_shape(sh, p))
}

/// Bounding box (min, max) of a set of shapes.
pub fn bbox(s: &Shapes) -> Option<(Pt, Pt)> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for sh in s {
        for p in &sh[0] {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
    }
    (lo[0] <= hi[0]).then_some((lo, hi))
}

fn seg_dist2(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    dist2(p, [a[0] + dx * t, a[1] + dy * t])
}

/// Douglas–Peucker on an open polyline.
pub fn simplify_polyline(pts: &[Pt], tol: f64) -> Vec<Pt> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    let t2 = tol * tol;
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (mut best, mut bi) = (0.0, a);
        for i in a + 1..b {
            let d = seg_dist2(pts[i], pts[a], pts[b]);
            if d > best {
                best = d;
                bi = i;
            }
        }
        if best > t2 {
            keep[bi] = true;
            stack.push((a, bi));
            stack.push((bi, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// Douglas–Peucker on a closed contour (split at the vertex farthest from the first).
pub fn simplify_contour(c: &Contour, tol: f64) -> Contour {
    if c.len() < 4 {
        return c.clone();
    }
    // start at an extreme vertex (always a real corner), split at the vertex farthest from it
    let s0 = (0..c.len()).min_by(|&i, &j| c[i][0].partial_cmp(&c[j][0]).unwrap().then(c[i][1].partial_cmp(&c[j][1]).unwrap())).unwrap();
    let c: Contour = c[s0..].iter().chain(c[..s0].iter()).cloned().collect();
    let far = (1..c.len()).max_by(|&i, &j| dist2(c[0], c[i]).partial_cmp(&dist2(c[0], c[j])).unwrap()).unwrap();
    let mut a: Vec<Pt> = c[..=far].to_vec();
    let mut b: Vec<Pt> = c[far..].to_vec();
    b.push(c[0]);
    a = simplify_polyline(&a, tol);
    b = simplify_polyline(&b, tol);
    a.pop();
    b.pop();
    a.extend(b);
    if a.len() < 3 {
        c
    } else {
        a
    }
}

pub fn simplify(s: &Shapes, tol: f64) -> Shapes {
    s.iter()
        .map(|sh| sh.iter().map(|c| simplify_contour(c, tol)).filter(|c| c.len() >= 3).collect::<Shape>())
        .filter(|sh: &Shape| !sh.is_empty())
        .collect()
}

/// Shapes whose area is at least `min`.
pub fn drop_small(s: Shapes, min: f64) -> Shapes {
    s.into_iter().filter(|sh| shape_area(sh) >= min).collect()
}

/// True when the segment a-b stays inside the region (no boundary crossing, midpoint inside).
pub fn segment_inside(region: &Shapes, a: Pt, b: Pt) -> bool {
    if !in_shapes(region, [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]) {
        return false;
    }
    for sh in region {
        for c in sh {
            let n = c.len();
            for i in 0..n {
                if segments_cross(a, b, c[i], c[(i + 1) % n]) {
                    return false;
                }
            }
        }
    }
    true
}

fn orient(a: Pt, b: Pt, c: Pt) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Proper or touching intersection of two segments.
pub fn segments_cross(a: Pt, b: Pt, c: Pt, d: Pt) -> bool {
    let (d1, d2) = (orient(c, d, a), orient(c, d, b));
    let (d3, d4) = (orient(a, b, c), orient(a, b, d));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

pub fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Shapes {
    vec![vec![vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_and_booleans() {
        let sq = rect(0.0, 0.0, 10.0, 10.0);
        assert!((area(&sq) - 100.0).abs() < 1e-6);
        let inner = offset(&sq, -1.0);
        assert!((area(&inner) - 64.0).abs() < 1e-3, "{}", area(&inner));
        let outer = offset(&sq, 1.0);
        assert!((area(&outer) - 144.0).abs() < 1e-2, "{}", area(&outer));
        let hole = rect(3.0, 3.0, 7.0, 7.0);
        let ring = diff(&sq, &hole);
        assert_eq!(ring.len(), 1);
        assert_eq!(ring[0].len(), 2);
        assert!((area(&ring) - 84.0).abs() < 1e-6);
        // the hole grows when the ring shrinks
        let ring_in = offset(&ring, -0.5);
        assert!((area(&ring_in) - (81.0 - 25.0)).abs() < 1e-3, "{}", area(&ring_in));
        assert!((area(&inter(&sq, &rect(5.0, 5.0, 15.0, 15.0))) - 25.0).abs() < 1e-6);
        assert!((area(&union(&sq, &rect(5.0, 5.0, 15.0, 15.0))) - 175.0).abs() < 1e-6);
        assert!(offset(&sq, -6.0).is_empty());
    }

    #[test]
    fn loops_fill_nonzero() {
        let outer: Contour = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let hole: Contour = vec![[2.0, 2.0], [2.0, 8.0], [8.0, 8.0], [8.0, 2.0]];
        let s = from_loops(&[outer.clone(), hole.clone()]);
        assert!((area(&s) - 64.0).abs() < 1e-6);
        // an inside-out mesh gives both loops reversed: still the same region
        let r = from_loops(&[outer.iter().rev().cloned().collect(), hole.iter().rev().cloned().collect()]);
        assert!((area(&r) - 64.0).abs() < 1e-6);
    }

    #[test]
    fn clipping_lines() {
        let sq = rect(0.0, 0.0, 10.0, 10.0);
        let lines = vec![vec![[-5.0, 5.0], [15.0, 5.0]], vec![[-5.0, 20.0], [15.0, 20.0]]];
        let r = clip_lines(&lines, &sq);
        assert_eq!(r.len(), 1);
        assert!((polyline_len(&r[0]) - 10.0).abs() < 1e-6);
        let ring = diff(&sq, &rect(3.0, 3.0, 7.0, 7.0));
        let r = clip_lines(&lines, &ring);
        assert_eq!(r.len(), 2);
        assert!((r.iter().map(|l| polyline_len(l)).sum::<f64>() - 6.0).abs() < 1e-6);
    }

    #[test]
    fn simplify_keeps_corners() {
        let mut c: Contour = vec![];
        for i in 0..=100 {
            c.push([i as f64 * 0.1, 0.0]);
        }
        c.push([10.0, 10.0]);
        c.push([0.0, 10.0]);
        let s = simplify_contour(&c, 0.01);
        assert_eq!(s.len(), 4, "{:?}", s);
        assert!((contour_area(&s) - 100.0).abs() < 1e-9);
    }
}
