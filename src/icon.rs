//! Freo's app icon: a dark slate tile (Creo's tab-bar colour) with an extruded green "F"
//! standing on an orange datum plane. Drawn as vector polygons with supersampling and
//! returned as RGBA, so it works for the .icns bundle and the Dock alike.

type Pt = (f64, f64);

struct Layer { loops: Vec<Vec<Pt>>, fill: Fill }

enum Fill { Solid([f64; 4]), VGrad([f64; 4], [f64; 4], f64, f64) }

impl Fill {
    fn at(&self, y: f64) -> [f64; 4] {
        match self {
            Fill::Solid(c) => *c,
            Fill::VGrad(a, b, y0, y1) => { let t = ((y - y0) / (y1 - y0)).clamp(0.0, 1.0); [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t) }
        }
    }
}

fn rgb(h: u32, a: f64) -> [f64; 4] { [((h >> 16) & 255) as f64 / 255.0, ((h >> 8) & 255) as f64 / 255.0, (h & 255) as f64 / 255.0, a] }

/// Even-odd point-in-polygon over several loops.
fn inside(loops: &[Vec<Pt>], p: Pt) -> bool {
    let mut c = false;
    for l in loops {
        let n = l.len();
        for i in 0..n {
            let (a, b) = (l[i], l[(i + 1) % n]);
            if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) * (b.0 - a.0) / (b.1 - a.1) { c = !c; }
        }
    }
    c
}

fn rounded_rect(x0: f64, y0: f64, x1: f64, y1: f64, r: f64) -> Vec<Pt> {
    let mut v = vec![];
    for (cx, cy, a0) in [(x1 - r, y0 + r, -90.0f64), (x1 - r, y1 - r, 0.0), (x0 + r, y1 - r, 90.0), (x0 + r, y0 + r, 180.0)] {
        for k in 0..=16 { let a = (a0 + 90.0 * k as f64 / 16.0).to_radians(); v.push((cx + r * a.cos(), cy + r * a.sin())); }
    }
    v
}

/// Icon as RGBA bytes, `size` × `size`.
pub fn render_icon(size: usize) -> Vec<u8> {
    let s = size as f64;
    // macOS icon grid: 824/1024 body with a continuous-corner radius
    let (m, r) = (0.098, 0.181);
    let body = rounded_rect(m, m, 1.0 - m, 1.0 - m, r);
    let shadow = rounded_rect(m + 0.004, m + 0.018, 1.0 - m - 0.004, 1.0 - m + 0.014, r);
    let mut layers = vec![
        Layer { loops: vec![shadow], fill: Fill::Solid([0.0, 0.0, 0.0, 0.28]) },
        Layer { loops: vec![body.clone()], fill: Fill::VGrad(rgb(0x55_61_63, 1.0), rgb(0x23_29_2a, 1.0), m, 1.0 - m) },
        // thin highlight along the top edge
        Layer { loops: vec![rounded_rect(m, m, 1.0 - m, 1.0 - m, r), rounded_rect(m + 0.006, m + 0.006, 1.0 - m - 0.006, 1.0 - m - 0.006, r - 0.006)], fill: Fill::VGrad(rgb(0xff_ff_ff, 0.22), rgb(0xff_ff_ff, 0.0), m, 0.5) },
    ];
    // letter F in its own units (x right, y up), extruded backwards; oblique projection
    let f: Vec<Pt> = vec![(0.0, 0.0), (2.1, 0.0), (2.1, 4.3), (5.6, 4.3), (5.6, 6.3), (2.1, 6.3), (2.1, 8.0), (7.0, 8.0), (7.0, 10.0), (0.0, 10.0)];
    let depth = 2.6;
    let dir = (0.58, 0.46);
    let k = 0.052; // units → icon fraction
    let (ox, oy) = (0.5 - (7.0 + depth * dir.0) * k / 2.0 - 0.01, 0.5 + (10.0 + depth * dir.1) * k / 2.0 - depth * dir.1 * k + 0.035);
    let proj = |x: f64, y: f64, z: f64| -> Pt { (ox + (x + z * dir.0) * k, oy - (y + z * dir.1) * k) };
    // datum plane under the F (like Creo's TOP plane), outline and a faint fill
    let plane = |e: f64| vec![proj(-1.6 - e, -0.15, -1.2 - e), proj(8.6 + e, -0.15, -1.2 - e), proj(8.6 + e, -0.15, depth + 2.6 + e), proj(-1.6 - e, -0.15, depth + 2.6 + e)];
    let datum = rgb(0xd8_9c_48, 1.0);
    layers.push(Layer { loops: vec![plane(0.0)], fill: Fill::Solid([datum[0], datum[1], datum[2], 0.16]) });
    layers.push(Layer { loops: vec![plane(0.0), plane(-0.16)], fill: Fill::Solid(datum) });
    // side faces that face the viewer (normals towards the receding direction), far ones first
    let n = f.len();
    let mut sides: Vec<(f64, Layer)> = vec![];
    for i in 0..n {
        let (a, b) = (f[i], f[(i + 1) % n]);
        let nrm = (b.1 - a.1, -(b.0 - a.0));
        if nrm.0 * dir.0 + nrm.1 * dir.1 <= 0.0 { continue; }
        let quad = vec![proj(a.0, a.1, 0.0), proj(b.0, b.1, 0.0), proj(b.0, b.1, depth), proj(a.0, a.1, depth)];
        let top = nrm.1.abs() > nrm.0.abs();
        let fill = if top { Fill::Solid(rgb(0x9c_e3_7a, 1.0)) } else { Fill::VGrad(rgb(0x3c_93_2a, 1.0), rgb(0x24_66_19, 1.0), 0.2, 0.8) };
        let key = (a.0 + b.0) * 0.5 * dir.0 + (a.1 + b.1) * 0.5 * dir.1;
        sides.push((key, Layer { loops: vec![quad], fill }));
    }
    sides.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap());
    layers.extend(sides.into_iter().map(|s| s.1));
    let front: Vec<Pt> = f.iter().map(|p| proj(p.0, p.1, 0.0)).collect();
    let (fy0, fy1) = (proj(0.0, 10.0, 0.0).1, proj(0.0, 0.0, 0.0).1);
    layers.push(Layer { loops: vec![front], fill: Fill::VGrad(rgb(0x7e_d6_55, 1.0), rgb(0x4c_ab_31, 1.0), fy0, fy1) });

    // supersampled rasterisation with per-layer bounding boxes
    let boxes: Vec<(f64, f64, f64, f64)> = layers.iter().map(|l| l.loops.iter().flatten().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, p| (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1)))).collect();
    let ss = if size <= 64 { 6 } else { 4 };
    let mut out = vec![0u8; size * size * 4];
    for py in 0..size {
        for px in 0..size {
            let mut acc = [0.0f64; 4];
            for sy in 0..ss {
                for sx in 0..ss {
                    let p = ((px as f64 + (sx as f64 + 0.5) / ss as f64) / s, (py as f64 + (sy as f64 + 0.5) / ss as f64) / s);
                    // premultiplied "over" compositing, bottom layer first
                    let mut c = [0.0f64; 4];
                    for (l, b) in layers.iter().zip(&boxes) {
                        if p.0 < b.0 || p.0 > b.2 || p.1 < b.1 || p.1 > b.3 || !inside(&l.loops, p) { continue; }
                        let f = l.fill.at(p.1);
                        let a = f[3];
                        for i in 0..3 { c[i] = f[i] * a + c[i] * (1.0 - a); }
                        c[3] = a + c[3] * (1.0 - a);
                    }
                    for i in 0..4 { acc[i] += c[i]; }
                }
            }
            let nss = (ss * ss) as f64;
            let a = acc[3] / nss;
            let o = (py * size + px) * 4;
            for i in 0..3 { out[o + i] = if a > 1e-6 { ((acc[i] / nss / a) * 255.0).round().clamp(0.0, 255.0) as u8 } else { 0 }; }
            out[o + 3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}
