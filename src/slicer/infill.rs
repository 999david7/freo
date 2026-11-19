//! Infill patterns. Every generator returns centre lines clipped to the region the line
//! centres may cover; spacings follow the density so the extruded volume matches it.

use super::config::InfillPattern;
use super::poly::{self, Pt, Shapes};
use std::f64::consts::{PI, SQRT_2, TAU};

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub pts: Vec<Pt>,
    pub closed: bool,
}

impl Line {
    pub fn open(pts: Vec<Pt>) -> Line {
        Line { pts, closed: false }
    }
    pub fn len(&self) -> f64 {
        let mut l = poly::polyline_len(&self.pts);
        if self.closed && self.pts.len() > 2 {
            l += poly::dist(self.pts[0], *self.pts.last().unwrap());
        }
        l
    }
    pub fn is_empty(&self) -> bool {
        self.pts.len() < 2
    }
}

/// Parallel lines `spacing` apart at `angle` (degrees) covering the region, on a grid that is
/// the same on every layer (lines stack), shifted by `shift` across the lines.
pub fn parallel(region: &Shapes, angle: f64, spacing: f64, shift: f64) -> Vec<Vec<Pt>> {
    parallel_in(region, angle, spacing, Some(shift))
}

/// Parallel lines centred in the region's extent across them (solid infill: no line falls
/// on the boundary, the gaps at both sides are equal).
pub fn parallel_centred(region: &Shapes, angle: f64, spacing: f64) -> Vec<Vec<Pt>> {
    parallel_in(region, angle, spacing, None)
}

fn parallel_in(region: &Shapes, angle: f64, spacing: f64, shift: Option<f64>) -> Vec<Vec<Pt>> {
    let Some((lo, hi)) = poly::bbox(region) else { return vec![] };
    if spacing <= 1e-6 {
        return vec![];
    }
    let a = angle.to_radians();
    let u = [a.cos(), a.sin()];
    let v = [-a.sin(), a.cos()];
    let corners = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]];
    let pu: Vec<f64> = corners.iter().map(|c| c[0] * u[0] + c[1] * u[1]).collect();
    let pv: Vec<f64> = corners.iter().map(|c| c[0] * v[0] + c[1] * v[1]).collect();
    let (umin, umax) = (pu.iter().cloned().fold(f64::INFINITY, f64::min) - 1.0, pu.iter().cloned().fold(f64::NEG_INFINITY, f64::max) + 1.0);
    let (vmin, vmax) = (pv.iter().cloned().fold(f64::INFINITY, f64::min), pv.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    let shift = match shift {
        Some(s) => s,
        None => {
            let e = (spacing * 0.02).min(0.005);
            let n = ((vmax - vmin - 2.0 * e).max(0.0) / spacing).floor();
            vmin + (vmax - vmin - n * spacing) / 2.0
        }
    };
    let k0 = ((vmin - shift) / spacing).floor() as i64;
    let k1 = ((vmax - shift) / spacing).ceil() as i64;
    if k1 - k0 > 200_000 {
        return vec![];
    }
    let mut raw = Vec::with_capacity((k1 - k0 + 1) as usize);
    for k in k0..=k1 {
        let s = shift + k as f64 * spacing;
        let p = |t: f64| [u[0] * t + v[0] * s, u[1] * t + v[1] * s];
        raw.push(vec![p(umin), p(umax)]);
    }
    poly::clip_lines(&raw, region)
}

/// Sorts lines across their direction and alternates them (zig-zag order), as a monotonic
/// pass over the surface. All lines are straight at `angle`.
pub fn monotonic_order(lines: Vec<Vec<Pt>>, angle: f64) -> Vec<Vec<Pt>> {
    let a = angle.to_radians();
    let u = [a.cos(), a.sin()];
    let v = [-a.sin(), a.cos()];
    let mut keyed: Vec<(f64, f64, Vec<Pt>)> = lines
        .into_iter()
        .filter(|l| l.len() >= 2)
        .map(|mut l| {
            let pu = |p: Pt| p[0] * u[0] + p[1] * u[1];
            if pu(l[0]) > pu(*l.last().unwrap()) {
                l.reverse();
            }
            let m = l[0];
            ((m[0] * v[0] + m[1] * v[1] * 1.0), pu(m), l)
        })
        .collect();
    keyed.sort_by(|a, b| {
        let dv = a.0 - b.0;
        if dv.abs() > 1e-6 {
            dv.partial_cmp(&0.0).unwrap()
        } else {
            a.1.partial_cmp(&b.1).unwrap()
        }
    });
    let mut out = Vec::with_capacity(keyed.len());
    let mut flip = false;
    let mut last_v = f64::NAN;
    for (kv, _, mut l) in keyed {
        if last_v.is_nan() {
            last_v = kv;
        } else if (kv - last_v).abs() > 1e-6 {
            flip = !flip;
            last_v = kv;
        }
        if flip {
            l.reverse();
        }
        out.push(l);
    }
    out
}

/// Concentric loops `spacing` apart inside the region (the region is already the area of
/// line centres, so the first loop is its boundary).
pub fn concentric(region: &Shapes, spacing: f64) -> Vec<Line> {
    let mut out = vec![];
    let mut cur = region.clone();
    let mut guard = 0;
    while !cur.is_empty() && guard < 10_000 {
        for sh in &cur {
            for c in sh {
                if c.len() >= 3 {
                    out.push(Line { pts: c.clone(), closed: true });
                }
            }
        }
        cur = poly::offset(&cur, -spacing);
        guard += 1;
    }
    out
}

/// Zero set of the gyroid sin x cos y + sin y cos z + sin z cos x at height z, by marching
/// squares, as polylines (period `p`).
pub fn gyroid_raw(lo: Pt, hi: Pt, p: f64, z: f64) -> Vec<Vec<Pt>> {
    let k = TAU / p;
    let g = p / 24.0;
    let nx = (((hi[0] - lo[0]) / g).ceil() as usize + 2).min(6000);
    let ny = (((hi[1] - lo[1]) / g).ceil() as usize + 2).min(6000);
    let x0 = lo[0] - g;
    let y0 = lo[1] - g;
    let zz = z * k;
    let (sz, cz) = zz.sin_cos();
    let f = |x: f64, y: f64| {
        let (sx, cx) = (x * k).sin_cos();
        let (sy, cy) = (y * k).sin_cos();
        sx * cy + sy * cz + sz * cx
    };
    let w = nx + 1;
    let mut val = vec![0.0; w * (ny + 1)];
    for j in 0..=ny {
        for i in 0..=nx {
            val[j * w + i] = f(x0 + i as f64 * g, y0 + j as f64 * g);
        }
    }
    let at = |i: usize, j: usize| val[j * w + i];
    // edge ids: horizontal (i,j)-(i+1,j) = 2*(j*w+i), vertical (i,j)-(i,j+1) = 2*(j*w+i)+1
    let hid = |i: usize, j: usize| 2 * (j * w + i);
    let vid = |i: usize, j: usize| 2 * (j * w + i) + 1;
    let point = |e: usize| -> Pt {
        let cell = e / 2;
        let (i, j) = (cell % w, cell / w);
        let (a, b, pa, pb) = if e % 2 == 0 {
            (at(i, j), at(i + 1, j), [x0 + i as f64 * g, y0 + j as f64 * g], [x0 + (i + 1) as f64 * g, y0 + j as f64 * g])
        } else {
            (at(i, j), at(i, j + 1), [x0 + i as f64 * g, y0 + j as f64 * g], [x0 + i as f64 * g, y0 + (j + 1) as f64 * g])
        };
        let t = if (b - a).abs() > 1e-15 { (a / (a - b)).clamp(0.0, 1.0) } else { 0.5 };
        [pa[0] + (pb[0] - pa[0]) * t, pa[1] + (pb[1] - pa[1]) * t]
    };
    let mut adj: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    let mut link = |a: usize, b: usize| {
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    };
    for j in 0..ny {
        for i in 0..nx {
            let c = [at(i, j) >= 0.0, at(i + 1, j) >= 0.0, at(i + 1, j + 1) >= 0.0, at(i, j + 1) >= 0.0];
            // edges: bottom, right, top, left
            let e = [hid(i, j), vid(i + 1, j), hid(i, j + 1), vid(i, j)];
            let cut: Vec<usize> = (0..4).filter(|&q| c[q] != c[(q + 1) % 4]).collect();
            match cut.len() {
                2 => link(e[cut[0]], e[cut[1]]),
                4 => {
                    let centre = (at(i, j) + at(i + 1, j) + at(i + 1, j + 1) + at(i, j + 1)) >= 0.0;
                    // corners 0 and 2 have the same sign: join the edges around the corners
                    // whose sign differs from the centre
                    if centre == c[0] {
                        link(e[0], e[1]);
                        link(e[2], e[3]);
                    } else {
                        link(e[3], e[0]);
                        link(e[1], e[2]);
                    }
                }
                _ => {}
            }
        }
    }
    let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut out = vec![];
    let mut keys: Vec<usize> = adj.keys().cloned().collect();
    keys.sort_unstable();
    // start at chain ends first so open polylines come out whole
    keys.sort_by_key(|k| adj[k].len());
    for s in keys {
        if seen.contains(&s) {
            continue;
        }
        let mut line = vec![point(s)];
        seen.insert(s);
        let mut cur = s;
        let mut prev = usize::MAX;
        loop {
            let next = adj[&cur].iter().find(|&&n| n != prev && !seen.contains(&n)).cloned();
            match next {
                Some(n) => {
                    seen.insert(n);
                    line.push(point(n));
                    prev = cur;
                    cur = n;
                }
                None => {
                    // closed loop: back to the start
                    if adj[&cur].contains(&s) && line.len() > 2 {
                        line.push(line[0]);
                    }
                    break;
                }
            }
        }
        if line.len() >= 2 {
            out.push(line);
        }
    }
    out
}

/// Line length per area of the gyroid's zero set for period 1 (stereology: (π/4)·S_V with
/// S_V ≈ 3.0919 per unit cell).
pub const GYROID_LA: f64 = PI / 4.0 * 3.0919;

/// Honeycomb (pointy-top hexagons of side `a`): one zig-zag per row plus the vertical walls.
pub fn honeycomb_raw(lo: Pt, hi: Pt, a: f64) -> Vec<Vec<Pt>> {
    let hw = 3f64.sqrt() * a / 2.0; // half the hexagon width
    let row = 1.5 * a;
    let r0 = ((lo[1] - a) / row).floor() as i64;
    let r1 = ((hi[1] + a) / row).ceil() as i64;
    let c0 = ((lo[0] - 2.0 * hw) / (2.0 * hw)).floor() as i64;
    let c1 = ((hi[0] + 2.0 * hw) / (2.0 * hw)).ceil() as i64;
    if (r1 - r0) * (c1 - c0) > 4_000_000 {
        return vec![];
    }
    let mut out = vec![];
    for r in r0..=r1 {
        let cy = r as f64 * row;
        let off = if r.rem_euclid(2) == 1 { hw } else { 0.0 };
        // top zig-zag of the row: side corners at cy + a/2, tips at cy + a
        let mut zig = vec![];
        for c in c0..=c1 {
            let cx = c as f64 * 2.0 * hw + off;
            zig.push([cx - hw, cy + a / 2.0]);
            zig.push([cx, cy + a]);
        }
        let cx = (c1 + 1) as f64 * 2.0 * hw + off;
        zig.push([cx - hw, cy + a / 2.0]);
        out.push(zig);
        for c in c0..=c1 + 1 {
            let x = c as f64 * 2.0 * hw + off - hw;
            out.push(vec![[x, cy - a / 2.0], [x, cy + a / 2.0]]);
        }
    }
    out
}

/// Sparse infill of `region` (area of line centres). `ls`: extrusion spacing of the infill
/// line, `density` 0..1, `angle` degrees, `z` print height, `layer` index.
pub fn sparse(pattern: InfillPattern, region: &Shapes, ls: f64, density: f64, angle: f64, z: f64, layer: usize) -> Vec<Line> {
    if density <= 1e-4 || region.is_empty() {
        return vec![];
    }
    let d = density.min(1.0);
    let open = |v: Vec<Vec<Pt>>| v.into_iter().map(Line::open).collect::<Vec<_>>();
    match pattern {
        InfillPattern::Rectilinear | InfillPattern::Line | InfillPattern::Monotonic | InfillPattern::ZigZag => {
            let a = angle + if layer % 2 == 1 { 90.0 } else { 0.0 };
            open(parallel(region, a, ls / d, 0.0))
        }
        InfillPattern::Grid => {
            let s = 2.0 * ls / d;
            let mut v = parallel(region, angle, s, 0.0);
            v.extend(parallel(region, angle + 90.0, s, 0.0));
            open(v)
        }
        InfillPattern::Triangles => {
            let s = 3.0 * ls / d;
            let mut v = vec![];
            for k in 0..3 {
                v.extend(parallel(region, angle + 60.0 * k as f64, s, 0.0));
            }
            open(v)
        }
        InfillPattern::Cubic => {
            // a cube standing on its corner, cut at height z: three line sets that move
            // z/√2 across per mm of height
            let s = 3.0 * ls / d;
            let mut v = vec![];
            for k in 0..3 {
                let shift = (z / SQRT_2).rem_euclid(s);
                v.extend(parallel(region, angle + 60.0 * k as f64, s, shift));
            }
            open(v)
        }
        InfillPattern::Honeycomb => {
            let Some((lo, hi)) = poly::bbox(region) else { return vec![] };
            // edge length per hexagon area: 2 / (√3 a) = d / ls
            let a = 2.0 * ls / (3f64.sqrt() * d);
            open(poly::clip_lines(&honeycomb_raw(lo, hi, a), region))
        }
        InfillPattern::Gyroid => {
            let Some((lo, hi)) = poly::bbox(region) else { return vec![] };
            let p = GYROID_LA * ls / d;
            let raw = gyroid_raw(lo, hi, p, z);
            let raw: Vec<Vec<Pt>> = raw.into_iter().map(|l| poly::simplify_polyline(&l, 0.01)).collect();
            open(poly::clip_lines(&raw, region))
        }
        InfillPattern::Concentric => concentric(region, ls / d),
    }
}

/// Solid infill: lines one extrusion spacing apart.
pub fn solid(pattern: InfillPattern, region: &Shapes, ls: f64, angle: f64) -> Vec<Line> {
    if region.is_empty() {
        return vec![];
    }
    match pattern {
        InfillPattern::Concentric => concentric(region, ls),
        InfillPattern::Monotonic | InfillPattern::ZigZag => monotonic_order(parallel_centred(region, angle, ls), angle).into_iter().map(Line::open).collect(),
        _ => parallel_centred(region, angle, ls).into_iter().map(Line::open).collect(),
    }
}

/// Bridge direction over `region`: the angle (15° steps) whose clipped lines are shortest
/// on average, so each line spans the gap the short way.
pub fn bridge_angle(region: &Shapes, ls: f64) -> f64 {
    let mut best = (0.0, f64::INFINITY);
    for k in 0..12 {
        let a = k as f64 * 15.0;
        let l = parallel(region, a, ls * 3.0, 0.0);
        if l.is_empty() {
            continue;
        }
        let m = l.iter().map(|x| poly::polyline_len(x)).fold(0.0, f64::max);
        if m < best.1 - 1e-6 {
            best = (a, m);
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length(v: &[Line]) -> f64 {
        v.iter().map(|l| l.len()).sum()
    }

    #[test]
    fn densities_match() {
        let region = poly::rect(0.0, 0.0, 60.0, 60.0);
        let ls = 0.45;
        for pat in InfillPattern::SPARSE {
            for d in [0.1, 0.2, 0.4] {
                let mut acc = 0.0;
                let n = 6;
                for layer in 0..n {
                    acc += length(&sparse(pat, &region, ls, d, 45.0, 0.2 + layer as f64 * 0.7, layer));
                }
                let got = acc / n as f64 * ls / 3600.0;
                let tol = match pat {
                    // the outermost loop runs on the region boundary: more line near edges
                    InfillPattern::Concentric => 0.2,
                    _ => 0.06,
                };
                assert!((got - d).abs() / d < tol, "{:?} {}: density {:.4}", pat, d, got);
            }
        }
    }

    #[test]
    fn solid_covers_and_alternates() {
        let region = poly::rect(0.0, 0.0, 10.0, 10.0);
        let l = solid(InfillPattern::Monotonic, &region, 0.5, 0.0);
        assert_eq!(l.len(), 20);
        // serpentine: consecutive lines run opposite ways
        let d0 = l[0].pts[1][0] - l[0].pts[0][0];
        let d1 = l[1].pts[1][0] - l[1].pts[0][0];
        assert!(d0 * d1 < 0.0);
        assert!(l.windows(2).all(|w| w[0].pts[0][1] < w[1].pts[0][1]));
    }

    #[test]
    fn bridge_spans_short_way() {
        let region = poly::rect(0.0, 0.0, 40.0, 5.0);
        assert_eq!(bridge_angle(&region, 0.45), 90.0);
    }
}
