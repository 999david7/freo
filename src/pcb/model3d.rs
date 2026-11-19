//! Detailed 3D models of electronic parts, generated from the footprint like KiCad's own
//! model generators: chip parts with solder terminations, moulded packages with gull-wing
//! leads, through-hole parts with leads through the board, resistors with colour bands
//! from their value, LEDs with domes, electrolytic cans, pin headers, connector shells.
//!
//! Coordinates: footprint-local millimetres, x right, y down (as the footprint), z up from
//! the board's top surface. Each material is its own mesh, so the 3D board can show colours.

use super::footprints::{Footprint, Pad};
use super::geom::{p, P};

pub type V3 = [f64; 3];

/// Materials (one colour each). The name is used for the part of the 3D assembly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mat {
    Plastic,
    Metal,
    Gold,
    Ceramic,
    Beige,
    Orange,
    Blue,
    Green,
    White,
    LedRed,
    Brass,
    Band(u8),
}

impl Mat {
    pub fn rgb(self) -> [f32; 3] {
        match self {
            Mat::Plastic => [0.10, 0.10, 0.11],
            Mat::Metal => [0.78, 0.79, 0.81],
            Mat::Gold => [0.86, 0.69, 0.28],
            Mat::Ceramic => [0.70, 0.56, 0.38],
            Mat::Beige => [0.86, 0.78, 0.60],
            Mat::Orange => [0.88, 0.48, 0.12],
            Mat::Blue => [0.16, 0.30, 0.62],
            Mat::Green => [0.20, 0.55, 0.32],
            Mat::White => [0.93, 0.93, 0.91],
            Mat::LedRed => [0.92, 0.16, 0.14],
            Mat::Brass => [0.72, 0.60, 0.32],
            Mat::Band(k) => BAND_RGB[k as usize % BAND_RGB.len()],
        }
    }
    pub fn name(self) -> String {
        match self {
            Mat::Plastic => "KUNSTSTOFF".into(),
            Mat::Metal => "METALL".into(),
            Mat::Gold => "GOLD".into(),
            Mat::Ceramic => "KERAMIK".into(),
            Mat::Beige => "BEIGE".into(),
            Mat::Orange => "ORANGE".into(),
            Mat::Blue => "BLAU".into(),
            Mat::Green => "GRUEN".into(),
            Mat::White => "WEISS".into(),
            Mat::LedRed => "LED_ROT".into(),
            Mat::Brass => "MESSING".into(),
            Mat::Band(k) => format!("RING_{}", BAND_NAME[k as usize % BAND_NAME.len()]),
        }
    }
}

/// Resistor colour code 0–9, then gold (10) and silver (11).
const BAND_RGB: [[f32; 3]; 12] = [
    [0.05, 0.05, 0.05],
    [0.45, 0.25, 0.10],
    [0.80, 0.10, 0.10],
    [0.95, 0.50, 0.10],
    [0.95, 0.85, 0.15],
    [0.15, 0.60, 0.20],
    [0.15, 0.30, 0.80],
    [0.55, 0.20, 0.70],
    [0.50, 0.50, 0.50],
    [0.97, 0.97, 0.97],
    [0.83, 0.66, 0.25],
    [0.75, 0.76, 0.78],
];
const BAND_NAME: [&str; 12] = ["SCHWARZ", "BRAUN", "ROT", "ORANGE", "GELB", "GRUEN", "BLAU", "VIOLETT", "GRAU", "WEISS", "GOLD", "SILBER"];

/// One mesh per material: vertices (x, y, z, nx, ny, nz) and triangles.
#[derive(Default, Clone, Debug)]
pub struct Model {
    pub parts: Vec<(Mat, Vec<[f32; 6]>, Vec<[u32; 3]>)>,
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l < 1e-12 {
        [0.0, 0.0, 1.0]
    } else {
        mul(a, 1.0 / l)
    }
}

impl Model {
    fn mesh(&mut self, m: Mat) -> usize {
        match self.parts.iter().position(|x| x.0 == m) {
            Some(i) => i,
            None => {
                self.parts.push((m, vec![], vec![]));
                self.parts.len() - 1
            }
        }
    }
    fn vert(&mut self, k: usize, q: V3, n: V3) -> u32 {
        let v = &mut self.parts[k].1;
        v.push([q[0] as f32, q[1] as f32, q[2] as f32, n[0] as f32, n[1] as f32, n[2] as f32]);
        (v.len() - 1) as u32
    }
    /// Flat polygon (convex, any winding); the normal follows the right-hand rule.
    fn poly(&mut self, m: Mat, pts: &[V3]) {
        if pts.len() < 3 {
            return;
        }
        let n = norm(cross(sub(pts[1], pts[0]), sub(pts[2], pts[0])));
        let k = self.mesh(m);
        let ids: Vec<u32> = pts.iter().map(|q| self.vert(k, *q, n)).collect();
        for i in 1..ids.len() - 1 {
            self.parts[k].2.push([ids[0], ids[i], ids[i + 1]]);
        }
    }
    /// Quad a b c d (counter-clockwise seen from outside).
    fn quad(&mut self, m: Mat, a: V3, b: V3, c: V3, d: V3) {
        self.poly(m, &[a, b, c, d]);
    }
    /// Axis-aligned box.
    pub fn boxx(&mut self, m: Mat, lo: V3, hi: V3) {
        let c = |i: usize| [if i & 1 != 0 { hi[0] } else { lo[0] }, if i & 2 != 0 { hi[1] } else { lo[1] }, if i & 4 != 0 { hi[2] } else { lo[2] }];
        // faces: -z, +z, -y, +y, -x, +x (outward, counter-clockwise)
        for f in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
            self.quad(m, c(f[0]), c(f[1]), c(f[2]), c(f[3]));
        }
    }
    /// Box with chamfered top edges (moulded bodies).
    pub fn body(&mut self, m: Mat, lo: V3, hi: V3, ch: f64) {
        let ch = ch.min((hi[0] - lo[0]) / 3.0).min((hi[1] - lo[1]) / 3.0).min((hi[2] - lo[2]) / 2.0).max(0.0);
        let mid = hi[2] - ch;
        self.boxx(m, lo, [hi[0], hi[1], mid]);
        let bottom = [[lo[0], lo[1], mid], [hi[0], lo[1], mid], [hi[0], hi[1], mid], [lo[0], hi[1], mid]];
        let top = [[lo[0] + ch, lo[1] + ch, hi[2]], [hi[0] - ch, lo[1] + ch, hi[2]], [hi[0] - ch, hi[1] - ch, hi[2]], [lo[0] + ch, hi[1] - ch, hi[2]]];
        for i in 0..4 {
            let j = (i + 1) % 4;
            self.quad(m, bottom[i], bottom[j], top[j], top[i]);
        }
        self.poly(m, &top);
    }
    /// Cylinder between `a` and `b` with radius r, closed ends.
    pub fn cyl(&mut self, m: Mat, a: V3, b: V3, r: f64, seg: usize) {
        let d = norm(sub(b, a));
        let u = norm(cross(if d[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] }, d));
        let v = cross(d, u);
        let k = self.mesh(m);
        let ring = |c: V3, i: usize| {
            let t = i as f64 / seg as f64 * std::f64::consts::TAU;
            let n = add(mul(u, t.cos()), mul(v, t.sin()));
            (add(c, mul(n, r)), n)
        };
        let base = self.parts[k].1.len() as u32;
        for i in 0..seg {
            let (qa, n) = ring(a, i);
            let (qb, _) = ring(b, i);
            self.vert(k, qa, n);
            self.vert(k, qb, n);
        }
        for i in 0..seg as u32 {
            let j = (i + 1) % seg as u32;
            let (a0, b0, a1, b1) = (base + 2 * i, base + 2 * i + 1, base + 2 * j, base + 2 * j + 1);
            self.parts[k].2.push([a0, a1, b1]);
            self.parts[k].2.push([a0, b1, b0]);
        }
        let cap_b: Vec<V3> = (0..seg).map(|i| ring(b, i).0).collect();
        let cap_a: Vec<V3> = (0..seg).rev().map(|i| ring(a, i).0).collect();
        self.poly(m, &cap_b);
        self.poly(m, &cap_a);
    }
    /// Upward hemisphere (dome) of radius r on the circle centred at c.
    pub fn dome(&mut self, m: Mat, c: V3, r: f64) {
        let (seg, rings) = (24, 8);
        let k = self.mesh(m);
        let base = self.parts[k].1.len() as u32;
        for j in 0..=rings {
            let phi = j as f64 / rings as f64 * std::f64::consts::FRAC_PI_2;
            for i in 0..seg {
                let t = i as f64 / seg as f64 * std::f64::consts::TAU;
                let n = [phi.cos() * t.cos(), phi.cos() * t.sin(), phi.sin()];
                self.vert(k, add(c, mul(n, r)), n);
            }
        }
        for j in 0..rings as u32 {
            for i in 0..seg as u32 {
                let i2 = (i + 1) % seg as u32;
                let (a, b, c2, d) = (base + j * seg as u32 + i, base + j * seg as u32 + i2, base + (j + 1) * seg as u32 + i2, base + (j + 1) * seg as u32 + i);
                self.parts[k].2.push([a, b, c2]);
                self.parts[k].2.push([a, c2, d]);
            }
        }
    }
    /// Rectangular wire from a to b: width w (sideways, horizontal) and thickness t.
    pub fn bar(&mut self, m: Mat, a: V3, b: V3, w: f64, t: f64) {
        let d = norm(sub(b, a));
        let s = norm(cross([0.0, 0.0, 1.0], d));
        let s = if dot(s, s) < 0.5 { [1.0, 0.0, 0.0] } else { s };
        let up = cross(d, s);
        let (hs, hu) = (mul(s, w / 2.0), mul(up, t / 2.0));
        let c = |base: V3, i: usize| add(add(base, if i & 1 != 0 { hs } else { mul(hs, -1.0) }), if i & 2 != 0 { hu } else { mul(hu, -1.0) });
        let (a0, a1, a2, a3) = (c(a, 0), c(a, 1), c(a, 3), c(a, 2));
        let (b0, b1, b2, b3) = (c(b, 0), c(b, 1), c(b, 3), c(b, 2));
        for (p0, p1, q1, q0) in [(a0, a1, b1, b0), (a1, a2, b2, b1), (a2, a3, b3, b2), (a3, a0, b0, b3)] {
            self.quad(m, p0, q0, q1, p1);
        }
        self.poly(m, &[a0, a3, a2, a1]);
        self.poly(m, &[b0, b1, b2, b3]);
    }
    /// Vertical prism over a convex outline (counter-clockwise in x/y with y up, any here).
    pub fn prism(&mut self, m: Mat, ring: &[P], z0: f64, z1: f64) {
        let n = ring.len();
        if n < 3 {
            return;
        }
        let ccw = super::geom::area(ring) > 0.0;
        let r: Vec<P> = if ccw { ring.to_vec() } else { ring.iter().rev().copied().collect() };
        let top: Vec<V3> = r.iter().map(|q| [q.x, q.y, z1]).collect();
        let bot: Vec<V3> = r.iter().rev().map(|q| [q.x, q.y, z0]).collect();
        self.poly(m, &top);
        self.poly(m, &bot);
        for i in 0..n {
            let j = (i + 1) % n;
            self.quad(m, [r[i].x, r[i].y, z0], [r[j].x, r[j].y, z0], [r[j].x, r[j].y, z1], [r[i].x, r[i].y, z1]);
        }
    }
    pub fn triangles(&self) -> usize {
        self.parts.iter().map(|p| p.2.len()).sum()
    }
}

fn stadium(w: f64, h: f64, c: P) -> Vec<P> {
    let r = h / 2.0;
    let x = (w / 2.0 - r).max(0.0);
    let mut v = vec![];
    for i in 0..=12 {
        let a = -std::f64::consts::FRAC_PI_2 + i as f64 / 12.0 * std::f64::consts::PI;
        v.push(c + p(x + r * a.cos(), r * a.sin()));
    }
    for i in 0..=12 {
        let a = std::f64::consts::FRAC_PI_2 + i as f64 / 12.0 * std::f64::consts::PI;
        v.push(c + p(-x + r * a.cos(), r * a.sin()));
    }
    v
}

/// Resistance in ohms from a value like "10k", "4k7", "330", "1M", "2R2", "100R".
pub fn ohms(v: &str) -> Option<f64> {
    let s = v.trim().replace(',', ".").replace('Ω', "").to_lowercase();
    let s = s.trim_end_matches("ohm").trim();
    for (ch, mult) in [('k', 1e3), ('m', 1e6), ('r', 1.0)] {
        if let Some(i) = s.find(ch) {
            let (a, b) = (&s[..i], &s[i + 1..]);
            let num = if b.is_empty() { a.to_string() } else { format!("{}.{}", if a.is_empty() { "0" } else { a }, b) };
            return num.parse::<f64>().ok().map(|x| x * mult);
        }
    }
    s.parse().ok()
}

/// Four-band colour code (two digits, multiplier, gold tolerance).
pub fn bands(r: f64) -> Option<[u8; 4]> {
    if r <= 0.0 || !r.is_finite() {
        return None;
    }
    let mut e = r.log10().floor() as i32 - 1;
    let mut digits = (r / 10f64.powi(e)).round() as i32;
    if digits >= 100 {
        digits /= 10;
        e += 1;
    }
    let mult = match e {
        -2 => 11,
        -1 => 10,
        0..=9 => e as u8,
        _ => return None,
    };
    Some([(digits / 10) as u8, (digits % 10) as u8, mult, 10])
}

fn pads_by(fp: &Footprint) -> Vec<&Pad> {
    fp.pads.iter().filter(|q| !q.npth).collect()
}

fn pad_box(fp: &Footprint) -> (P, P) {
    super::geom::bbox(pads_by(fp).iter().flat_map(|q| {
        let (a, b) = q.hull(P::ZERO, 0.0, false).bbox();
        [a, b]
    }))
    .unwrap_or((P::ZERO, P::ZERO))
}

/// A gull-wing lead from the body side at `from` out to the pad centre `to` (both x/y).
fn gull(m: &mut Model, from: P, to: P, w: f64, t: f64, z: f64) {
    let d = (to - from).normalize_or_zero();
    let knee = from + d * 0.25;
    let foot = to - d * 0.35;
    let top = [from.x, from.y, z];
    let k1 = [knee.x, knee.y, z];
    let k2 = [foot.x - d.x * 0.15, foot.y - d.y * 0.15, t / 2.0];
    let end = [to.x + d.x * 0.25, to.y + d.y * 0.25, t / 2.0];
    m.bar(Mat::Metal, top, k1, w, t);
    m.bar(Mat::Metal, k1, k2, w, t);
    m.bar(Mat::Metal, k2, end, w, t);
}

/// A through-hole lead: vertical through the board at the pad, from `top` down below it.
fn tht_lead(m: &mut Model, at: P, top: f64, thick: f64, d: f64, mat: Mat) {
    m.cyl(mat, [at.x, at.y, -thick - 1.5], [at.x, at.y, top], d / 2.0, 10);
}

/// Model of a footprint (None: keep the simple box bodies).
pub fn generate(fp: &Footprint, value: &str, thick: f64) -> Option<Model> {
    let id = fp.short().to_string();
    let mut m = Model::default();
    let pads = pads_by(fp);
    let (plo, phi) = pad_box(fp);
    let name = id.to_uppercase();
    if pads.is_empty() {
        return None;
    }
    // ---- two-terminal chips: R, C, L, fuse, LED
    if (name.starts_with("R_0") || name.starts_with("R_1") || name.starts_with("C_0") || name.starts_with("C_1") || name.starts_with("L_") || name.starts_with("FUSE_") || name.starts_with("LED_0") || name.starts_with("LED_1")) && pads.len() == 2 {
        let span = (pads[1].pos - pads[0].pos).length();
        let (l, w) = chip_size(&name).unwrap_or((span + pads[0].size.x * 0.3, pads[0].size.y * 0.85));
        let cap = (l * 0.2).max(0.2);
        let (hl, hw) = (l / 2.0, w / 2.0);
        let (body, h) = if name.starts_with('R') {
            (Mat::Plastic, (w * 0.35).max(0.35))
        } else if name.starts_with("LED") {
            (Mat::White, (w * 0.6).max(0.5))
        } else if name.starts_with('C') {
            (Mat::Ceramic, (w * 0.8).max(0.5))
        } else {
            (Mat::Plastic, (w * 0.7).max(0.5))
        };
        let along = (pads[1].pos - pads[0].pos).normalize_or_zero();
        let rotated = along.y.abs() > along.x.abs();
        let bx = |m: &mut Model, mat: Mat, x0: f64, x1: f64, y0: f64, y1: f64, z0: f64, z1: f64| {
            if rotated {
                m.boxx(mat, [y0, x0, z0], [y1, x1, z1]);
            } else {
                m.boxx(mat, [x0, y0, z0], [x1, y1, z1]);
            }
        };
        bx(&mut m, body, -hl + cap, hl - cap, -hw, hw, 0.0, h);
        bx(&mut m, Mat::Metal, -hl, -hl + cap, -hw - 0.01, hw + 0.01, 0.0, h + 0.01);
        bx(&mut m, Mat::Metal, hl - cap, hl, -hw - 0.01, hw + 0.01, 0.0, h + 0.01);
        if name.starts_with("LED") {
            // clear lens on top and a green cathode mark
            bx(&mut m, Mat::LedRed, -hl + cap + 0.1, hl - cap - 0.1, -hw + 0.1, hw - 0.1, h, h + 0.15);
            bx(&mut m, Mat::Green, -hl + cap, -hl + cap + 0.15, -hw, hw, h, h + 0.02);
        } else if name.starts_with('R') {
            // printed value on the black top: a light rectangle
            bx(&mut m, Mat::White, -hl * 0.4, hl * 0.4, -hw * 0.25, hw * 0.25, h, h + 0.005);
        }
        return Some(m);
    }
    // ---- SOD-123 diodes: moulded body with flat leads
    if name.starts_with("D_SOD") {
        let span = (pads[1].pos - pads[0].pos).length();
        let (hl, hw, h) = (span / 2.0 - 0.45, 0.8, 1.1);
        m.body(Mat::Plastic, [-hl, -hw, 0.1], [hl, hw, h], 0.15);
        m.boxx(Mat::White, [-hl + 0.15, -hw + 0.1, h], [-hl + 0.45, hw - 0.1, h + 0.01]);
        for (s, q) in [(-1.0, pads[0].pos), (1.0, pads[1].pos)] {
            gull(&mut m, p(s * hl, 0.0), q, 0.6, 0.12, 0.5);
        }
        return Some(m);
    }
    // ---- gull-wing packages: SOT-23, SOT-223, SOIC
    if name.starts_with("SOT-23") || name.starts_with("SOT-223") || name.starts_with("SOIC") || name.starts_with("SSOP") || name.starts_with("TSSOP") || name.contains("QFP") {
        let (lo, hi) = fp.body.iter().find_map(|b| match b {
            super::footprints::Body3::Box { lo, hi, .. } => Some((*lo, *hi)),
            _ => None,
        })?;
        let h = if name.starts_with("SOT-23") && !name.starts_with("SOT-223") { 1.0 } else if name.starts_with("SOT-223") { 1.6 } else { 1.5 };
        m.body(Mat::Plastic, [lo.x, lo.y, 0.1], [hi.x, hi.y, h], 0.2);
        // pin-1 dimple
        if let Some(p1) = pads.iter().find(|q| q.num == "1") {
            let c = p(p1.pos.x.clamp(lo.x + 0.5, hi.x - 0.5), p1.pos.y.clamp(lo.y + 0.5, hi.y - 0.5));
            m.cyl(Mat::Metal, [c.x, c.y, h - 0.02], [c.x, c.y, h + 0.005], 0.25, 12);
        }
        let zl = (h * 0.45).max(0.4);
        for q in &pads {
            // the lead leaves the body on the side facing the pad
            let from = p(q.pos.x.clamp(lo.x, hi.x), q.pos.y.clamp(lo.y, hi.y));
            if (from - q.pos).length() < 1e-6 {
                // pad under the body (exposed pad, SOT-223 tab): a flat tab
                let s = q.size;
                m.boxx(Mat::Metal, [q.pos.x - s.x * 0.4, q.pos.y - s.y * 0.45, 0.0], [q.pos.x + s.x * 0.4, q.pos.y + s.y * 0.45, 0.1]);
                continue;
            }
            let w = (q.size.x.min(q.size.y) * 0.75).min(0.5);
            let w = if name.starts_with("SOT-223") { q.size.y.min(q.size.x) * 0.5 } else { w };
            gull(&mut m, from, q.pos, w, 0.15, zl);
        }
        return Some(m);
    }
    // ---- DIP
    if name.starts_with("DIP-") {
        let (lo, hi) = (plo, phi);
        let (bx0, bx1) = (lo.x + 1.5, hi.x - 1.5);
        let (by0, by1) = (lo.y - 0.4, hi.y + 0.4);
        m.body(Mat::Plastic, [bx0, by0, 0.5], [bx1, by1, 3.8], 0.3);
        // notch at pin 1
        let cx = (bx0 + bx1) / 2.0;
        m.cyl(Mat::Metal, [cx, by0 + 0.01, 3.79], [cx, by0 + 0.01, 3.81], 0.6, 16);
        for q in &pads {
            tht_lead(&mut m, q.pos, 1.0, thick, 0.5, Mat::Metal);
            let toward = if q.pos.x < cx { bx0 } else { bx1 };
            m.bar(Mat::Metal, [toward, q.pos.y, 1.6], [q.pos.x, q.pos.y, 1.6], 1.4, 0.25);
            m.bar(Mat::Metal, [q.pos.x, q.pos.y, 1.6], [q.pos.x, q.pos.y, 0.5], 1.4, 0.25);
        }
        return Some(m);
    }
    // ---- axial resistors and diodes
    if name.starts_with("R_AXIAL") || name.starts_with("D_DO-41") {
        let (a, b) = (pads[0].pos, pads[1].pos);
        let d = (b - a).normalize_or_zero();
        let len = (b - a).length();
        let diode = name.starts_with("D_");
        let (bl, r) = if diode { (5.2, 1.35) } else { (6.3, 1.25) };
        let zc = r + 0.3;
        let s = a + d * ((len - bl) / 2.0);
        let e = s + d * bl;
        let v = |q: P, z: f64| -> V3 { [q.x, q.y, z] };
        m.cyl(if diode { Mat::Plastic } else { Mat::Beige }, v(s, zc), v(e, zc), r, 24);
        if !diode {
            // end bulges
            m.cyl(Mat::Beige, v(s - d * 0.05, zc), v(s + d * 0.9, zc), r * 1.1, 24);
            m.cyl(Mat::Beige, v(e - d * 0.9, zc), v(e + d * 0.05, zc), r * 1.1, 24);
            if let Some(code) = ohms(value).and_then(bands) {
                for (i, c) in code.iter().enumerate() {
                    let x0 = if i < 3 { 1.3 + i as f64 * 0.75 } else { bl - 1.4 };
                    m.cyl(Mat::Band(*c), v(s + d * x0, zc), v(s + d * (x0 + 0.45), zc), r * 1.04, 24);
                }
            }
        } else {
            // cathode band at pad 1
            m.cyl(Mat::Metal, v(s + d * 0.4, zc), v(s + d * 1.0, zc), r * 1.03, 24);
        }
        for (q, end) in [(a, s), (b, e)] {
            m.cyl(Mat::Metal, v(end, zc), v(q, zc), 0.3, 10);
            m.cyl(Mat::Metal, v(q, zc), v(q, -thick - 1.5), 0.3, 10);
        }
        return Some(m);
    }
    // ---- disc capacitor, electrolytic, LED 5 mm, buzzer, crystal, TO-92, TO-220
    if name.starts_with("C_DISC") {
        let c = (pads[0].pos + pads[1].pos) / 2.0;
        let d = (pads[1].pos - pads[0].pos).normalize_or_zero();
        let n = p(-d.y, d.x);
        m.cyl(Mat::Orange, [c.x - n.x * 1.2, c.y - n.y * 1.2, 4.0], [c.x + n.x * 1.2, c.y + n.y * 1.2, 4.0], 2.5, 28);
        for q in &pads {
            tht_lead(&mut m, q.pos, 2.5, thick, 0.5, Mat::Metal);
        }
        return Some(m);
    }
    if name.starts_with("CP_RADIAL") {
        let c = (pads[0].pos + pads[1].pos) / 2.0;
        let dia: f64 = name.split("_D").nth(1).and_then(|s| s.split("MM").next()).and_then(|s| s.parse().ok()).unwrap_or(5.0);
        let h = (dia * 2.2).clamp(5.0, 25.0);
        m.cyl(Mat::Blue, [c.x, c.y, 0.4], [c.x, c.y, h - 0.3], dia / 2.0, 32);
        m.cyl(Mat::Metal, [c.x, c.y, h - 0.3], [c.x, c.y, h], dia / 2.0 - 0.15, 32);
        // minus stripe on the side of pad 2
        let side = (pads[1].pos - c).normalize_or_zero();
        let q = c + side * (dia / 2.0 - 0.05);
        m.boxx(Mat::White, [q.x - 0.4, q.y - 0.4, 0.6], [q.x + 0.4, q.y + 0.4, h - 0.5]);
        for q in &pads {
            tht_lead(&mut m, q.pos, 0.5, thick, 0.5, Mat::Metal);
        }
        return Some(m);
    }
    if name.starts_with("CP_ELEC") {
        m.boxx(Mat::Plastic, [-2.65, -2.65, 0.0], [2.65, 2.65, 1.0]);
        m.cyl(Mat::Metal, [0.0, 0.0, 1.0], [0.0, 0.0, 5.3], 2.5, 32);
        m.boxx(Mat::Blue, [-2.4, -1.2, 5.3], [-1.6, 1.2, 5.31]);
        for q in &pads {
            m.boxx(Mat::Metal, [q.pos.x - 0.6, q.pos.y - 0.3, 0.0], [q.pos.x + 0.6, q.pos.y + 0.3, 0.12]);
        }
        return Some(m);
    }
    if name.starts_with("LED_D") {
        let c = (pads[0].pos + pads[1].pos) / 2.0;
        let r = 2.5;
        m.cyl(Mat::LedRed, [c.x, c.y, 1.0], [c.x, c.y, 1.8], r + 0.4, 32);
        m.cyl(Mat::LedRed, [c.x, c.y, 1.8], [c.x, c.y, 8.6 - r], r, 32);
        m.dome(Mat::LedRed, [c.x, c.y, 8.6 - r], r);
        for q in &pads {
            tht_lead(&mut m, q.pos, 1.0, thick, 0.5, Mat::Metal);
        }
        return Some(m);
    }
    if name.starts_with("BUZZER") {
        let c = (pads[0].pos + pads[1].pos) / 2.0;
        m.cyl(Mat::Plastic, [c.x, c.y, 0.0], [c.x, c.y, 9.5], 6.0, 40);
        m.cyl(Mat::Metal, [c.x, c.y, 9.5], [c.x, c.y, 9.52], 1.0, 16);
        return Some(m);
    }
    if name.starts_with("CRYSTAL_HC49") {
        let c = (pads[0].pos + pads[1].pos) / 2.0;
        m.prism(Mat::Metal, &stadium(11.0, 4.6, c), 0.5, 13.5);
        m.prism(Mat::Metal, &stadium(11.4, 5.0, c), 0.0, 0.5);
        for q in &pads {
            tht_lead(&mut m, q.pos, 0.5, thick, 0.45, Mat::Metal);
        }
        return Some(m);
    }
    if name.starts_with("TO-92") {
        let c = (pads[0].pos + pads[2].pos) / 2.0;
        // D-shaped body: flat side towards -y
        let mut ring = vec![p(c.x - 2.3, c.y - 0.6)];
        for i in 0..=16 {
            let a = std::f64::consts::PI * (1.0 - i as f64 / 16.0);
            ring.push(p(c.x + 2.3 * a.cos(), c.y - 0.6 + 2.3 * a.sin().abs() * 1.0));
        }
        m.prism(Mat::Plastic, &ring, 3.0, 7.8);
        for q in &pads {
            tht_lead(&mut m, q.pos, 3.0, thick, 0.45, Mat::Metal);
        }
        return Some(m);
    }
    if name.starts_with("TO-220") {
        let (x0, x1) = (pads[0].pos.x - 2.5, pads[2].pos.x + 2.5);
        let y = pads[0].pos.y;
        m.body(Mat::Plastic, [x0, y - 1.4, 3.0], [x1, y + 3.2, 12.0], 0.3);
        m.boxx(Mat::Metal, [x0, y + 1.95, 12.0], [x1, y + 3.25, 19.0]);
        m.cyl(Mat::Plastic, [(x0 + x1) / 2.0, y + 1.9, 16.0], [(x0 + x1) / 2.0, y + 3.3, 16.0], 1.8, 20);
        for q in &pads {
            m.bar(Mat::Metal, [q.pos.x, q.pos.y, -thick - 1.5], [q.pos.x, q.pos.y, 3.0], 0.8, 0.4);
        }
        return Some(m);
    }
    // ---- pin headers and terminal blocks
    if name.starts_with("PINHEADER") {
        for q in &pads {
            m.boxx(Mat::Plastic, [q.pos.x - 1.27, q.pos.y - 1.27, 0.0], [q.pos.x + 1.27, q.pos.y + 1.27, 2.5]);
            m.boxx(Mat::Gold, [q.pos.x - 0.32, q.pos.y - 0.32, -thick - 3.0], [q.pos.x + 0.32, q.pos.y + 0.32, 8.5]);
        }
        return Some(m);
    }
    if name.starts_with("TERMINALBLOCK") {
        let (lo, hi) = (plo, phi);
        m.body(Mat::Green, [lo.x - 1.2, lo.y - 2.7, 0.0], [hi.x + 1.2, hi.y + 2.7, 10.0], 0.5);
        for q in &pads {
            m.cyl(Mat::Metal, [q.pos.x, q.pos.y - 0.5, 10.0], [q.pos.x, q.pos.y - 0.5, 10.3], 1.4, 20);
            m.boxx(Mat::Plastic, [q.pos.x - 0.2, q.pos.y - 1.7, 10.3], [q.pos.x + 0.2, q.pos.y + 0.7, 10.31]);
            // wire entry on the front
            m.boxx(Mat::Plastic, [q.pos.x - 1.5, hi.y + 2.69, 2.5], [q.pos.x + 1.5, hi.y + 2.71, 6.0]);
            tht_lead(&mut m, q.pos, 0.0, thick, 1.0, Mat::Metal);
        }
        return Some(m);
    }
    if name.starts_with("SW_PUSH") {
        let c = (plo + phi) / 2.0;
        m.boxx(Mat::Plastic, [c.x - 3.0, c.y - 3.0, 0.0], [c.x + 3.0, c.y + 3.0, 3.5]);
        m.boxx(Mat::Metal, [c.x - 3.1, c.y - 3.1, 3.4], [c.x + 3.1, c.y + 3.1, 3.6]);
        m.cyl(Mat::Plastic, [c.x, c.y, 3.6], [c.x, c.y, 5.0], 1.75, 24);
        for q in &pads {
            tht_lead(&mut m, q.pos, 0.5, thick, 0.6, Mat::Metal);
        }
        return Some(m);
    }
    if name.starts_with("POTENTIOMETER") {
        let c = (plo + phi) / 2.0;
        m.body(Mat::Blue, [c.x - 5.0, c.y - 5.0, 0.0], [c.x + 5.0, c.y + 5.0, 5.0], 0.3);
        m.cyl(Mat::White, [c.x, c.y, 5.0], [c.x, c.y, 6.0], 2.2, 24);
        m.boxx(Mat::Plastic, [c.x - 1.6, c.y - 0.25, 6.0], [c.x + 1.6, c.y + 0.25, 6.01]);
        for q in &pads {
            tht_lead(&mut m, q.pos, 0.0, thick, 0.6, Mat::Metal);
        }
        return Some(m);
    }
    if name.starts_with("BATTERYHOLDER") {
        m.cyl(Mat::Metal, [0.0, 0.0, 0.0], [0.0, 0.0, 3.2], 10.0, 48);
        m.cyl(Mat::Metal, [0.0, 0.0, 3.2], [0.0, 0.0, 4.2], 9.8, 48);
        m.boxx(Mat::Metal, [-15.5, -2.4, 0.0], [-11.0, 2.4, 0.3]);
        m.boxx(Mat::Metal, [11.0, -2.4, 0.0], [15.5, 2.4, 0.3]);
        return Some(m);
    }
    // ---- connectors with housing openings: shells from the footprint body
    if let Some((lo, hi, h)) = fp.body.iter().find_map(|b| match b {
        super::footprints::Body3::Box { lo, hi, z1, .. } => Some((*lo, *hi, *z1)),
        _ => None,
    }) {
        if name.starts_with("USB_C") {
            // stadium shell lying on the board, opening towards -y
            let w = hi.x - lo.x;
            let ring = stadium(w, h, p(0.0, h / 2.0));
            let pts: Vec<V3> = ring.iter().map(|q| [q.x, lo.y, q.y]).collect();
            let back: Vec<V3> = ring.iter().map(|q| [q.x, hi.y, q.y]).collect();
            for i in 0..pts.len() {
                let j = (i + 1) % pts.len();
                m.quad(Mat::Metal, pts[i], back[i], back[j], pts[j]);
            }
            m.poly(Mat::Metal, &back.iter().rev().copied().collect::<Vec<_>>());
            // dark opening with the tongue
            let inner: Vec<V3> = stadium(w - 0.6, h - 0.6, p(0.0, h / 2.0)).iter().map(|q| [q.x, lo.y - 0.01, q.y]).collect();
            m.poly(Mat::Plastic, &inner);
            m.boxx(Mat::Plastic, [-w * 0.33, lo.y - 0.02, h / 2.0 - 0.35], [w * 0.33, lo.y + 2.0, h / 2.0 + 0.35]);
            for q in &pads {
                if q.drill > 0.0 {
                    tht_lead(&mut m, q.pos, 0.3, thick, 0.6, Mat::Metal);
                } else {
                    m.boxx(Mat::Metal, [q.pos.x - 0.15, q.pos.y - 0.5, 0.0], [q.pos.x + 0.15, q.pos.y + 0.7, 0.1]);
                }
            }
            return Some(m);
        }
        if name.starts_with("USB_") || name.starts_with("RJ45") {
            // metal shell with a dark front opening (front = -y)
            m.boxx(Mat::Metal, [lo.x, lo.y, 0.0], [hi.x, hi.y, h]);
            let (ow, oh) = if name.starts_with("RJ45") { (11.7, 6.1) } else if name.starts_with("USB_A") { (12.5, 5.12) } else { (6.85, 1.8) };
            let cx = (lo.x + hi.x) / 2.0;
            let z0 = (h - oh) / 2.0;
            m.boxx(Mat::Plastic, [cx - ow / 2.0, lo.y - 0.02, z0], [cx + ow / 2.0, lo.y + 0.5, z0 + oh]);
            if name.starts_with("RJ45") {
                m.boxx(Mat::Green, [lo.x + 0.5, lo.y - 0.03, h - 2.0], [lo.x + 2.5, lo.y, h - 0.8]);
                m.boxx(Mat::Orange, [hi.x - 2.5, lo.y - 0.03, h - 2.0], [hi.x - 0.5, lo.y, h - 0.8]);
            }
            for q in &pads {
                if q.drill > 0.0 {
                    tht_lead(&mut m, q.pos, 0.3, thick, q.drill * 0.8, Mat::Metal);
                }
            }
            return Some(m);
        }
        if name.starts_with("BARRELJACK") || name.starts_with("JACK_3") {
            let jack = name.starts_with("JACK");
            m.body(Mat::Plastic, [lo.x, lo.y, 0.0], [hi.x, hi.y, h], 0.4);
            // round opening on the -x (barrel) / +x (audio) face
            let (x, dir) = if jack { (hi.x, 1.0) } else { (lo.x, -1.0) };
            let zc = h / 2.0;
            m.cyl(Mat::Plastic, [x, (lo.y + hi.y) / 2.0, zc], [x + dir * 1.5, (lo.y + hi.y) / 2.0, zc], if jack { 2.5 } else { 4.0 }, 28);
            m.cyl(Mat::Metal, [x + dir * 1.51, (lo.y + hi.y) / 2.0, zc], [x + dir * 1.52, (lo.y + hi.y) / 2.0, zc], if jack { 1.8 } else { 3.0 }, 28);
            for q in &pads {
                tht_lead(&mut m, q.pos, 0.3, thick, 0.8, Mat::Metal);
            }
            return Some(m);
        }
    }
    None
}

/// Body size (length, width) of chip parts from the metric code ("2012Metric" → 2.0 × 1.25).
fn chip_size(name: &str) -> Option<(f64, f64)> {
    let i = name.find("METRIC")?;
    let code = &name[..i];
    let code = &code[code.len().checked_sub(4)?..];
    let l: f64 = code[..2].parse::<f64>().ok()? / 10.0;
    let w: f64 = code[2..].parse::<f64>().ok()? / 10.0;
    // 2012 means 2.0 × 1.2; the real widths are a bit larger
    let w = match code {
        "2012" => 1.25,
        "1005" => 0.5,
        "1608" => 0.8,
        "3216" => 1.6,
        _ => w,
    };
    Some((l, w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_code() {
        assert_eq!(ohms("10k"), Some(10_000.0));
        assert_eq!(ohms("4k7"), Some(4_700.0));
        assert_eq!(ohms("2R2"), Some(2.2));
        assert_eq!(ohms("1M"), Some(1e6));
        assert_eq!(ohms("330"), Some(330.0));
        // brown black orange gold
        assert_eq!(bands(10_000.0), Some([1, 0, 3, 10]));
        // orange orange brown
        assert_eq!(bands(330.0), Some([3, 3, 1, 10]));
        // yellow violet red
        assert_eq!(bands(4_700.0), Some([4, 7, 2, 10]));
        // red red gold (2.2 Ω)
        assert_eq!(bands(2.2), Some([2, 2, 10, 10]));
    }

    #[test]
    fn every_library_footprint_gets_a_model() {
        for f in super::super::footprints::library() {
            let m = generate(f, "10k", 1.6).unwrap_or_else(|| panic!("kein Modell für {}", f.id));
            assert!(m.triangles() > 10, "{}", f.id);
            // nothing floats far above or sinks into the part side of the board (only leads go down)
            for (_, v, _) in &m.parts {
                for q in v {
                    assert!(q[2] > -10.0 && q[2] < 30.0, "{}", f.id);
                }
            }
        }
    }
}
