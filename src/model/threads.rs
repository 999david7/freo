//! Thread and clearance-hole tables for standard holes (ISO metric coarse, UNC).

use super::features::{HoleFit, ThreadStd};

pub struct ThreadSpec {
    pub name: &'static str,
    /// major (nominal) diameter, mm
    pub major: f64,
    /// thread pitch, mm
    pub pitch: f64,
    /// tap drill diameter, mm
    pub tap: f64,
    /// clearance hole diameters (close, medium, free), mm
    pub clear: [f64; 3],
}

const fn iso(name: &'static str, major: f64, pitch: f64, tap: f64, clear: [f64; 3]) -> ThreadSpec {
    ThreadSpec { name, major, pitch, tap, clear }
}

/// ISO 261 coarse threads with ISO 273 clearance holes.
pub const ISO: &[ThreadSpec] = &[
    iso("M2", 2.0, 0.4, 1.6, [2.2, 2.4, 2.6]),
    iso("M2.5", 2.5, 0.45, 2.05, [2.7, 2.9, 3.1]),
    iso("M3", 3.0, 0.5, 2.5, [3.2, 3.4, 3.6]),
    iso("M4", 4.0, 0.7, 3.3, [4.3, 4.5, 4.8]),
    iso("M5", 5.0, 0.8, 4.2, [5.3, 5.5, 5.8]),
    iso("M6", 6.0, 1.0, 5.0, [6.4, 6.6, 7.0]),
    iso("M8", 8.0, 1.25, 6.8, [8.4, 9.0, 10.0]),
    iso("M10", 10.0, 1.5, 8.5, [10.5, 11.0, 12.0]),
    iso("M12", 12.0, 1.75, 10.2, [13.0, 13.5, 14.5]),
    iso("M14", 14.0, 2.0, 12.0, [15.0, 15.5, 16.5]),
    iso("M16", 16.0, 2.0, 14.0, [17.0, 17.5, 18.5]),
    iso("M20", 20.0, 2.5, 17.5, [21.0, 22.0, 24.0]),
    iso("M24", 24.0, 3.0, 21.0, [25.0, 26.0, 28.0]),
];

const IN: f64 = 25.4;

const fn unc(name: &'static str, major_in: f64, tpi: f64, tap_in: f64, clear_in: [f64; 3]) -> ThreadSpec {
    ThreadSpec { name, major: major_in * IN, pitch: IN / tpi, tap: tap_in * IN, clear: [clear_in[0] * IN, clear_in[1] * IN, clear_in[2] * IN] }
}

/// Unified coarse threads (ASME B1.1) with common tap and clearance drills.
pub const UNC: &[ThreadSpec] = &[
    unc("#4-40", 0.112, 40.0, 0.0890, [0.1160, 0.1220, 0.1285]),
    unc("#6-32", 0.138, 32.0, 0.1065, [0.1440, 0.1470, 0.1495]),
    unc("#8-32", 0.164, 32.0, 0.1360, [0.1695, 0.1730, 0.1770]),
    unc("#10-24", 0.190, 24.0, 0.1495, [0.1960, 0.1990, 0.2010]),
    unc("1/4-20", 0.250, 20.0, 0.2010, [0.2570, 0.2610, 0.2660]),
    unc("5/16-18", 0.3125, 18.0, 0.2570, [0.3230, 0.3280, 0.3320]),
    unc("3/8-16", 0.375, 16.0, 0.3125, [0.3860, 0.3910, 0.3970]),
    unc("1/2-13", 0.500, 13.0, 0.4219, [0.5156, 0.5234, 0.5312]),
];

pub fn table(s: ThreadStd) -> &'static [ThreadSpec] {
    match s {
        ThreadStd::Iso => ISO,
        ThreadStd::Unc => UNC,
    }
}

pub fn find(s: ThreadStd, name: &str) -> Option<&'static ThreadSpec> {
    table(s).iter().find(|t| t.name == name)
}

/// Drill diameter for a standard hole.
pub fn drill(spec: &ThreadSpec, tapped: bool, fit: HoleFit) -> f64 {
    if tapped {
        spec.tap
    } else {
        spec.clear[match fit {
            HoleFit::Close => 0,
            HoleFit::Medium => 1,
            HoleFit::Free => 2,
        }]
    }
}

/// Hole callout, e.g. "M6x1" or "1/4-20 UNC".
pub fn callout(s: ThreadStd, spec: &ThreadSpec) -> String {
    match s {
        ThreadStd::Iso => format!("{}x{}", spec.name, crate::kernel::math::fmt_num(spec.pitch, 2)),
        ThreadStd::Unc => format!("{} UNC", spec.name),
    }
}

/// Display mesh of a cosmetic thread: the helical turns (60° flanks, flat crest and root) on
/// the cylinder `wall` the solid has there. Triangles as (position, normal) triples, wound so
/// the side facing out of the material is the front face.
pub fn thread_mesh(c: &super::regen::Cosmetic) -> Vec<[(crate::kernel::math::V3, crate::kernel::math::V3); 3]> {
    use crate::kernel::math::{safe_norm, V3};
    let p = c.pitch;
    let len = c.length;
    if !(p > 1e-6 && len > 1e-6 && c.wall > 1e-6) {
        return vec![];
    }
    // ISO basic profile: thread depth 0.54 p between the crest and root flats
    let depth = 0.5413 * p;
    let (crest, root) = if c.internal {
        (c.wall, c.wall.max(c.radius).max(c.wall + depth * 0.6))
    } else {
        (c.radius.max(c.wall), (c.radius.max(c.wall) - depth).max(c.wall * 0.2))
    };
    // knots across one pitch: crest flat, flank, root flat, flank, crest flat
    let knots = [(0.0, crest), (0.0625, crest), (0.3750, root), (0.6250, root), (0.9375, crest), (1.0, crest)];
    let dir = safe_norm(c.dir, V3::Y);
    let w = safe_norm(dir.cross(if dir.x.abs() < 0.9 { V3::X } else { V3::Y }), V3::X);
    let v = dir.cross(w);
    let n = ((crest.max(root) * 6.0) as usize).clamp(24, 72);
    let turns = (len / p).min(600.0);
    let (k0, k1) = (-(n as i64) - 1, ((turns + 1.0) * n as f64).ceil() as i64);
    // the visible side looks out of the material: toward the axis in a hole, away from it on a shaft
    let out = if c.internal { -1.0 } else { 1.0 };
    let pos = |k: i64, j: usize| -> (V3, bool) {
        let t = k as f64 / n as f64;
        let a = p * (t + knots[j].0);
        // the end rings sit just inside the threaded length, so the face they end at covers them
        let eps = (p * 0.03).min(len * 0.25);
        let ac = a.clamp(eps, len - eps);
        let th = std::f64::consts::TAU * t;
        (c.base + dir * ac + (w * th.cos() + v * th.sin()) * knots[j].1, a < eps || a > len - eps)
    };
    let mut tris = Vec::with_capacity(((k1 - k0) as usize) * 10);
    let mut push = |q: [(V3, bool); 3], smooth: [V3; 3]| {
        let (a, b, d) = (q[0].0, q[1].0, q[2].0);
        let geo = (b - a).cross(d - a);
        if geo.length_squared() < 1e-18 {
            return;
        }
        let cen = (a + b + d) / 3.0;
        let rel = cen - c.base;
        let ax = rel.dot(dir);
        let radial = safe_norm(rel - dir * ax, w);
        let mut want = radial * out;
        let gn = geo.normalize();
        if gn.dot(radial).abs() < 0.2 {
            // end ring: in a hole it faces the opening the hole was drilled from, on a shaft
            // it faces out of the nearer shaft end
            want = if c.internal {
                -dir
            } else if ax > len * 0.5 {
                dir
            } else {
                -dir
            };
        }
        let flip = gn.dot(want) < 0.0;
        // squashed triangles at the ends are lit flat, the turns themselves smooth
        let face = if flip { -gn } else { gn };
        let nr = |i: usize| if q.iter().any(|x| x.1) { face } else if smooth[i].dot(face) < 0.0 { -smooth[i] } else { smooth[i] };
        let (na, nb, nd) = (nr(0), nr(1), nr(2));
        tris.push(if flip { [(a, na), (d, nd), (b, nb)] } else { [(a, na), (b, nb), (d, nd)] });
    };
    for k in k0..k1 {
        for j in 0..knots.len() - 1 {
            let (q00, q10, q11, q01) = (pos(k, j), pos(k + 1, j), pos(k + 1, j + 1), pos(k, j + 1));
            if q00.1 && q10.1 && q11.1 && q01.1 && ((q00.0 - q11.0).dot(dir)).abs() < 1e-12 && (knots[j].1 - knots[j + 1].1).abs() < 1e-12 {
                continue; // fully outside, collapsed onto a circle
            }
            // smooth normal of this strip at both columns of the quad
            let sn = |kk: i64| {
                let th = std::f64::consts::TAU * kk as f64 / n as f64;
                let er = w * th.cos() + v * th.sin();
                let et = -w * th.sin() + v * th.cos();
                let r = (knots[j].1 + knots[j + 1].1) * 0.5;
                let d_th = et * r + dir * (p / std::f64::consts::TAU);
                let d_u = dir * (p * (knots[j + 1].0 - knots[j].0)) + er * (knots[j + 1].1 - knots[j].1);
                safe_norm(d_th.cross(d_u), er)
            };
            let (s0, s1) = (sn(k), sn(k + 1));
            push([q00, q10, q11], [s0, s1, s1]);
            push([q00, q11, q01], [s0, s1, s0]);
        }
    }
    tris
}

/// The smooth wall triangle `tri` (positions and normals) with the threaded length of `c` cut
/// out: `None` when it is not part of that wall, otherwise the pieces that stay visible.
pub fn clip_wall(c: &super::regen::Cosmetic, tri: &[(crate::kernel::math::V3, crate::kernel::math::V3); 3]) -> Option<Vec<[(crate::kernel::math::V3, crate::kernel::math::V3); 3]>> {
    use crate::kernel::math::V3;
    let dir = c.dir.normalize_or_zero();
    let tol = (c.wall * 0.03).max(0.02);
    let ax = |q: V3| (q - c.base).dot(dir);
    for &(q, n) in tri {
        let a = ax(q);
        let r = (q - c.base - dir * a).length();
        if (r - c.wall).abs() > tol || n.dot(dir).abs() > 0.35 {
            return None;
        }
    }
    let (lo, hi) = (tri.iter().map(|t| ax(t.0)).fold(f64::INFINITY, f64::min), tri.iter().map(|t| ax(t.0)).fold(f64::NEG_INFINITY, f64::max));
    if hi <= -tol || lo >= c.length + tol {
        return None;
    }
    // keep the parts before 0 and beyond the thread length
    let clip = |poly: &[(V3, V3)], keep: &dyn Fn(f64) -> f64| -> Vec<(V3, V3)> {
        let mut out = vec![];
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let (da, db) = (keep(ax(a.0)), keep(ax(b.0)));
            if da >= 0.0 {
                out.push(a);
            }
            if (da >= 0.0) != (db >= 0.0) {
                let t = da / (da - db);
                out.push((a.0 + (b.0 - a.0) * t, (a.1 + (b.1 - a.1) * t).normalize_or_zero()));
            }
        }
        out
    };
    let mut pieces = vec![];
    for poly in [clip(tri, &|a| -a), clip(tri, &|a| a - c.length)] {
        for k in 1..poly.len().saturating_sub(1) {
            pieces.push([poly[0], poly[k], poly[k + 1]]);
        }
    }
    Some(pieces)
}
