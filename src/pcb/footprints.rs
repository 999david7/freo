//! Footprint library: SMD and through-hole land patterns with KiCad names and IPC-style pad
//! sizes, a silkscreen outline, a courtyard and a simple 3D body (boxes and cylinders) for
//! the board model. Connectors that need a housing opening name the matching item of the
//! sketch palette's "Ports" tab, so the enclosure cut-out and the board agree.
//!
//! Footprint coordinates: millimetres, y down, origin as in KiCad (part centre for SMD,
//! pin 1 for DIP packages and pin headers).

use super::geom::{p, rot, Hull, P};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PadShape {
    Circle,
    Rect,
    Oval,
    /// rounded rectangle; corner radius = ratio × the smaller side
    RoundRect(f64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pad {
    pub num: String,
    pub pos: P,
    pub size: P,
    pub shape: PadShape,
    /// drill diameter; 0 = SMD pad on the footprint's side only
    pub drill: f64,
    /// pad rotation in degrees, on top of the footprint's
    #[serde(default)]
    pub rot: f64,
    /// non-plated hole (mounting hole): no copper
    #[serde(default)]
    pub npth: bool,
    /// slotted hole: overall slot length along the pad's longer side (0 = round drill)
    #[serde(default)]
    pub slot: f64,
    /// drill position relative to the pad centre (KiCad's drill offset)
    #[serde(default)]
    pub drill_off: P,
}

impl Pad {
    /// The drill as a capsule: both centre ends of the slot (equal for a round hole) in
    /// board coordinates.
    pub fn hole_ends(&self, at: P, angle: f64, bottom: bool) -> (P, P) {
        let half = ((self.slot - self.drill) / 2.0).max(0.0);
        let local = if self.size.x >= self.size.y { p(half, 0.0) } else { p(0.0, half) };
        let place = |q: P| {
            let q = rot(q + self.drill_off, self.rot) + self.pos;
            let q = if bottom { p(-q.x, q.y) } else { q };
            at + rot(q, angle)
        };
        (place(-local), place(local))
    }
    pub fn smd(&self) -> bool {
        self.drill <= 0.0
    }
    /// Copper shape in footprint coordinates moved by `at`, rotated by `angle`, mirrored for
    /// the bottom side.
    pub fn hull(&self, at: P, angle: f64, bottom: bool) -> Hull {
        let place = |q: P| {
            let q = rot(q, self.rot);
            let q = q + self.pos;
            let q = if bottom { p(-q.x, q.y) } else { q };
            at + rot(q, angle)
        };
        let (w, h) = (self.size.x / 2.0, self.size.y / 2.0);
        match self.shape {
            PadShape::Circle => Hull::disc(place(P::ZERO), w.max(h)),
            PadShape::Oval => {
                if (w - h).abs() < 1e-9 {
                    Hull::disc(place(P::ZERO), w)
                } else if w > h {
                    Hull::capsule(place(p(-(w - h), 0.0)), place(p(w - h, 0.0)), h)
                } else {
                    Hull::capsule(place(p(0.0, -(h - w))), place(p(0.0, h - w)), w)
                }
            }
            PadShape::Rect => Hull { pts: vec![place(p(-w, -h)), place(p(w, -h)), place(p(w, h)), place(p(-w, h))], r: 0.0 },
            PadShape::RoundRect(k) => {
                let r = (k * w.min(h) * 2.0).min(w.min(h));
                let (a, b) = (w - r, h - r);
                Hull { pts: vec![place(p(-a, -b)), place(p(a, -b)), place(p(a, b)), place(p(-a, b))], r }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Body3 {
    /// box from `lo` to `hi` in footprint xy, between heights z0 and z1 above the board
    Box { lo: P, hi: P, z0: f64, z1: f64, color: [u8; 3] },
    Cyl { c: P, r: f64, z0: f64, z1: f64, color: [u8; 3] },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Footprint {
    pub id: String,
    pub description: String,
    pub pads: Vec<Pad>,
    /// silkscreen lines (F.SilkS)
    pub silk: Vec<(P, P)>,
    /// courtyard rectangle
    pub court: (P, P),
    pub body: Vec<Body3>,
    /// item of the sketch palette's "Ports" tab for the housing opening
    #[serde(default)]
    pub port: Option<String>,
    /// 3D model file (STEP/WRL path as in KiCad, may use ${KICAD…} variables)
    #[serde(default)]
    pub model: Option<String>,
    /// model offset (mm, KiCad's xyz with y up) and rotation (degrees about x, y, z)
    #[serde(default)]
    pub model_offset: [f64; 3],
    #[serde(default)]
    pub model_rot: [f64; 3],
}

impl Footprint {
    pub fn short(&self) -> &str {
        self.id.rsplit(':').next().unwrap_or(&self.id)
    }
    pub fn smd(&self) -> bool {
        self.pads.iter().all(|q| q.smd())
    }
    pub fn height(&self) -> f64 {
        self.body
            .iter()
            .map(|b| match b {
                Body3::Box { z1, .. } | Body3::Cyl { z1, .. } => *z1,
            })
            .fold(0.0, f64::max)
    }
}

const BLACK: [u8; 3] = [38, 38, 40];
const TAN: [u8; 3] = [196, 160, 100];
const BEIGE: [u8; 3] = [214, 196, 160];
const GOLD: [u8; 3] = [212, 175, 80];
const STEEL: [u8; 3] = [190, 194, 198];
const BLUE: [u8; 3] = [60, 100, 170];
const GREEN_T: [u8; 3] = [70, 160, 110];

struct F {
    f: Footprint,
}

impl F {
    fn new(id: &str, desc: &str) -> F {
        F { f: Footprint { id: id.into(), description: desc.into(), pads: vec![], silk: vec![], court: (P::ZERO, P::ZERO), body: vec![], port: None, model: None, model_offset: [0.0; 3], model_rot: [0.0; 3] } }
    }
    fn pad(mut self, num: &str, pos: P, size: P, shape: PadShape, drill: f64) -> F {
        self.f.pads.push(Pad { num: num.into(), pos, size, shape, drill, rot: 0.0, npth: false, slot: 0.0, drill_off: P::ZERO });
        self
    }
    /// Makes the oval pads added so far slotted holes of length `len`.
    fn slots(mut self, len: f64) -> F {
        for q in &mut self.f.pads {
            if q.shape == PadShape::Oval && q.drill > 0.0 && (q.size.x - q.size.y).abs() > 1e-6 {
                q.slot = len;
            }
        }
        self
    }
    fn npth(mut self, pos: P, d: f64) -> F {
        self.f.pads.push(Pad { num: String::new(), pos, size: p(d, d), shape: PadShape::Circle, drill: d, rot: 0.0, npth: true, slot: 0.0, drill_off: P::ZERO });
        self
    }
    fn silk_rect(mut self, lo: P, hi: P) -> F {
        let c = [lo, p(hi.x, lo.y), hi, p(lo.x, hi.y)];
        for i in 0..4 {
            self.f.silk.push((c[i], c[(i + 1) % 4]));
        }
        self
    }
    fn silk(mut self, a: P, b: P) -> F {
        self.f.silk.push((a, b));
        self
    }
    fn bx(mut self, lo: P, hi: P, z0: f64, z1: f64, color: [u8; 3]) -> F {
        self.f.body.push(Body3::Box { lo, hi, z0, z1, color });
        self
    }
    fn cyl(mut self, c: P, r: f64, z0: f64, z1: f64, color: [u8; 3]) -> F {
        self.f.body.push(Body3::Cyl { c, r, z0, z1, color });
        self
    }
    fn port(mut self, name: &str) -> F {
        self.f.port = Some(name.into());
        self
    }
    /// Courtyard around pads and body with 0.25 mm margin (0.5 mm for connectors).
    fn done(mut self) -> Footprint {
        let mut pts = vec![];
        for q in &self.f.pads {
            let (lo, hi) = q.hull(P::ZERO, 0.0, false).bbox();
            pts.extend([lo, hi]);
        }
        for b in &self.f.body {
            match b {
                Body3::Box { lo, hi, .. } => pts.extend([*lo, *hi]),
                Body3::Cyl { c, r, .. } => pts.extend([*c - P::splat(*r), *c + P::splat(*r)]),
            }
        }
        let m = if self.f.port.is_some() { 0.5 } else { 0.25 };
        let (lo, hi) = super::geom::bbox(pts).unwrap_or((P::ZERO, P::ZERO));
        let g = |v: f64| (v / 0.05).round() * 0.05;
        self.f.court = (p(g(lo.x - m), g(lo.y - m)), p(g(hi.x + m), g(hi.y + m)));
        // KiCad's standard 3D model of the same name (used when KiCad's models are installed)
        if self.f.model.is_none() {
            if let Some((lib, name)) = self.f.id.split_once(':') {
                if lib != "Projekt" && lib != "X" {
                    self.f.model = Some(format!("${{KICAD8_3DMODEL_DIR}}/{}.3dshapes/{}.wrl", lib, name));
                }
            }
        }
        self.f
    }
}

/// Two-pad chip part (0402 … 1206, SOD-123): pads at ±`x`, `pw` × `ph`, body `bl` × `bw` × `bh`.
fn chip(id: &str, desc: &str, x: f64, pw: f64, ph: f64, bl: f64, bw: f64, bh: f64, color: [u8; 3], polar: bool) -> Footprint {
    let rr = PadShape::RoundRect(0.25);
    let mut f = F::new(id, desc).pad("1", p(-x, 0.0), p(pw, ph), rr, 0.0).pad("2", p(x, 0.0), p(pw, ph), rr, 0.0);
    let (sx, sy) = (bl / 2.0, bw / 2.0 + 0.2);
    let inner = (x - pw / 2.0 - 0.2).max(0.1);
    f = f.silk(p(-inner.min(sx), -sy), p(inner.min(sx), -sy)).silk(p(-inner.min(sx), sy), p(inner.min(sx), sy));
    if polar {
        f = f.silk(p(-x - pw / 2.0 - 0.3, -ph / 2.0), p(-x - pw / 2.0 - 0.3, ph / 2.0));
    }
    f = f.bx(p(-bl / 2.0, -bw / 2.0), p(bl / 2.0, bw / 2.0), 0.0, bh, color);
    if !polar {
        // tinned end caps
        let cap = (bl * 0.18).max(0.15);
        f = f.bx(p(-bl / 2.0, -bw / 2.0), p(-bl / 2.0 + cap, bw / 2.0), 0.0, bh + 0.01, STEEL).bx(p(bl / 2.0 - cap, -bw / 2.0), p(bl / 2.0, bw / 2.0), 0.0, bh + 0.01, STEEL);
    }
    f.done()
}

fn soic(id: &str, desc: &str, n: usize, pitch: f64, x: f64, pw: f64, ph: f64, bl: f64, bw: f64) -> Footprint {
    let half = n / 2;
    let y0 = -(half as f64 - 1.0) / 2.0 * pitch;
    let rr = PadShape::RoundRect(0.25);
    let mut f = F::new(id, desc);
    for i in 0..half {
        f = f.pad(&(i + 1).to_string(), p(-x, y0 + i as f64 * pitch), p(pw, ph), rr, 0.0);
    }
    for i in 0..half {
        f = f.pad(&(half + i + 1).to_string(), p(x, -y0 - i as f64 * pitch), p(pw, ph), rr, 0.0);
    }
    let (sx, sy) = (bw / 2.0, bl / 2.0);
    f = f.silk(p(-sx, -sy - 0.1), p(sx, -sy - 0.1)).silk(p(-sx, sy + 0.1), p(sx, sy + 0.1)).silk(p(-x - pw / 2.0, -sy - 0.1), p(-sx, -sy - 0.1));
    f.bx(p(-bw / 2.0, -bl / 2.0), p(bw / 2.0, bl / 2.0), 0.1, 1.55, BLACK).done()
}

fn dip(n: usize) -> Footprint {
    let half = n / 2;
    let w = 7.62;
    let mut f = F::new(&format!("Package_DIP:DIP-{}_W7.62mm", n), &format!("DIP-{}, Reihenabstand 7,62 mm, THT", n));
    for i in 0..half {
        let shape = if i == 0 { PadShape::Rect } else { PadShape::Oval };
        f = f.pad(&(i + 1).to_string(), p(0.0, i as f64 * 2.54), p(1.6, 1.6), shape, 0.8);
    }
    for i in 0..half {
        f = f.pad(&(half + i + 1).to_string(), p(w, (half - 1 - i) as f64 * 2.54), p(1.6, 1.6), PadShape::Oval, 0.8);
    }
    let len = (half - 1) as f64 * 2.54;
    f = f.silk_rect(p(1.16, -1.33), p(w - 1.16, len + 1.33)).silk(p(w / 2.0 - 1.0, -1.33), p(w / 2.0, -0.5)).silk(p(w / 2.0, -0.5), p(w / 2.0 + 1.0, -1.33));
    f.bx(p(0.635, -1.27), p(w - 0.635, len + 1.27), 0.5, 3.8, BLACK).done()
}

fn header1(n: usize) -> Footprint {
    let mut f = F::new(&format!("Connector_PinHeader_2.54mm:PinHeader_1x{:02}_P2.54mm_Vertical", n), &format!("Stiftleiste 1×{}, Raster 2,54 mm, THT", n));
    for i in 0..n {
        f = f.pad(&(i + 1).to_string(), p(0.0, i as f64 * 2.54), p(1.7, 1.7), if i == 0 { PadShape::Rect } else { PadShape::Oval }, 1.0);
    }
    let len = (n - 1) as f64 * 2.54;
    f = f.silk_rect(p(-1.33, -1.33), p(1.33, len + 1.33));
    f = f.bx(p(-1.27, -1.27), p(1.27, len + 1.27), 0.0, 2.54, BLACK);
    for i in 0..n {
        f = f.bx(p(-0.32, i as f64 * 2.54 - 0.32), p(0.32, i as f64 * 2.54 + 0.32), 2.54, 8.5, GOLD);
    }
    f.done()
}

fn terminal(n: usize) -> Footprint {
    let mut f = F::new(&format!("TerminalBlock:TerminalBlock_1x{:02}_P5.08mm", n), &format!("Schraubklemme {}-polig, Raster 5,08 mm", n));
    for i in 0..n {
        f = f.pad(&(i + 1).to_string(), p(i as f64 * 5.08, 0.0), p(2.6, 2.6), if i == 0 { PadShape::Rect } else { PadShape::Circle }, 1.3);
    }
    let len = (n - 1) as f64 * 5.08;
    f = f.silk_rect(p(-2.64, -4.2), p(len + 2.64, 4.2));
    f = f.bx(p(-2.54, -4.0), p(len + 2.54, 4.0), 0.0, 10.0, GREEN_T);
    for i in 0..n {
        f = f.cyl(p(i as f64 * 5.08, -0.5), 1.4, 10.0, 10.2, STEEL);
    }
    f.done()
}

fn build() -> Vec<Footprint> {
    let rr = PadShape::RoundRect(0.25);
    let mut v = vec![];
    // ---- chip resistors, capacitors, inductors, LEDs, fuses (IPC nominal density)
    for (cls, metric, x, pw, ph, bl, bw) in [
        ("0402", "1005", 0.51, 0.54, 0.64, 1.0, 0.5),
        ("0603", "1608", 0.825, 0.8, 0.95, 1.6, 0.8),
        ("0805", "2012", 0.9125, 1.025, 1.4, 2.0, 1.25),
        ("1206", "3216", 1.4625, 1.125, 1.75, 3.2, 1.6),
    ] {
        v.push(chip(&format!("Resistor_SMD:R_{}_{}Metric", cls, metric), &format!("Chip-Widerstand {} ({} metrisch)", cls, metric), x, pw, ph, bl, bw, bw * 0.35 + 0.2, BLACK, false));
        v.push(chip(&format!("Capacitor_SMD:C_{}_{}Metric", cls, metric), &format!("Keramikkondensator {} ({} metrisch)", cls, metric), x, pw, ph, bl, bw, bw * 0.8, TAN, false));
    }
    for (cls, metric, x, pw, ph, bl, bw) in [("0805", "2012", 0.9375, 0.975, 1.4, 2.0, 1.25), ("1206", "3216", 1.4, 1.25, 1.75, 3.2, 1.6)] {
        v.push(chip(&format!("Inductor_SMD:L_{}_{}Metric", cls, metric), &format!("Chip-Induktivität {}", cls), x, pw, ph, bl, bw, bw * 0.8, [90, 90, 92], false));
    }
    v.push(chip("LED_SMD:LED_0805_2012Metric", "Chip-LED 0805", 0.9375, 0.975, 1.4, 2.0, 1.25, 0.8, [230, 230, 220], true));
    v.push(chip("Fuse:Fuse_1206_3216Metric", "Chip-Sicherung 1206", 1.4, 1.25, 1.75, 3.2, 1.6, 0.9, [235, 235, 230], false));
    v.push(chip("Diode_SMD:D_SOD-123", "Diode SOD-123", 1.635, 0.91, 1.22, 2.7, 1.6, 1.15, BLACK, true));
    // ---- SOT
    v.push(
        F::new("Package_TO_SOT_SMD:SOT-23", "SOT-23, 3 Anschlüsse")
            .pad("1", p(-0.9375, -0.95), p(1.475, 0.6), rr, 0.0)
            .pad("2", p(-0.9375, 0.95), p(1.475, 0.6), rr, 0.0)
            .pad("3", p(0.9375, 0.0), p(1.475, 0.6), rr, 0.0)
            .silk(p(0.0, -1.56), p(0.65, -1.56))
            .silk(p(0.0, 1.56), p(0.65, 1.56))
            .silk(p(0.65, -1.56), p(0.65, -0.6))
            .silk(p(0.65, 1.56), p(0.65, 0.6))
            .bx(p(-0.65, -1.45), p(0.65, 1.45), 0.1, 1.1, BLACK)
            .done(),
    );
    v.push(
        F::new("Package_TO_SOT_SMD:SOT-223-3_TabPin2", "SOT-223, Kühlfahne an Pin 2")
            .pad("1", p(-3.15, -2.3), p(2.0, 1.5), rr, 0.0)
            .pad("2", p(-3.15, 0.0), p(2.0, 1.5), rr, 0.0)
            .pad("3", p(-3.15, 2.3), p(2.0, 1.5), rr, 0.0)
            .pad("2", p(3.15, 0.0), p(2.0, 3.8), rr, 0.0)
            .silk_rect(p(-1.85, -3.41), p(1.85, 3.41))
            .bx(p(-1.75, -3.25), p(1.75, 3.25), 0.1, 1.7, BLACK)
            .done(),
    );
    v.push(soic("Package_SO:SOIC-8_3.9x4.9mm_P1.27mm", "SOIC-8, 3,9 × 4,9 mm", 8, 1.27, 2.475, 1.95, 0.6, 4.9, 3.9));
    v.push(soic("Package_SO:SOIC-14_3.9x8.7mm_P1.27mm", "SOIC-14, 3,9 × 8,7 mm", 14, 1.27, 2.475, 1.95, 0.6, 8.7, 3.9));
    v.push(soic("Package_SO:SOIC-16_3.9x9.9mm_P1.27mm", "SOIC-16, 3,9 × 9,9 mm", 16, 1.27, 2.475, 1.95, 0.6, 9.9, 3.9));
    for n in [8, 14, 16, 28] {
        v.push(dip(n));
    }
    // ---- through-hole passives and semiconductors
    v.push(
        F::new("Resistor_THT:R_Axial_DIN0207_L6.3mm_D2.5mm_P10.16mm_Horizontal", "Widerstand axial DIN0207, Raster 10,16 mm")
            .pad("1", p(0.0, 0.0), p(1.6, 1.6), PadShape::Circle, 0.8)
            .pad("2", p(10.16, 0.0), p(1.6, 1.6), PadShape::Oval, 0.8)
            .silk_rect(p(1.81, -1.37), p(8.35, 1.37))
            .bx(p(1.93, -1.25), p(8.23, 1.25), 0.3, 2.8, BEIGE)
            .bx(p(3.0, -1.26), p(3.5, 1.26), 0.3, 2.81, [150, 70, 40])
            .bx(p(4.2, -1.26), p(4.7, 1.26), 0.3, 2.81, BLACK)
            .bx(p(5.4, -1.26), p(5.9, 1.26), 0.3, 2.81, [220, 120, 30])
            .done(),
    );
    v.push(
        F::new("Capacitor_THT:C_Disc_D5.0mm_W2.5mm_P5.00mm", "Scheibenkondensator Ø5 mm, Raster 5 mm")
            .pad("1", p(0.0, 0.0), p(1.6, 1.6), PadShape::Circle, 0.8)
            .pad("2", p(5.0, 0.0), p(1.6, 1.6), PadShape::Circle, 0.8)
            .silk_rect(p(-0.12, -1.37), p(5.12, 1.37))
            .bx(p(0.0, -1.25), p(5.0, 1.25), 1.0, 6.0, [214, 140, 40])
            .done(),
    );
    v.push(
        F::new("Capacitor_THT:CP_Radial_D5.0mm_P2.00mm", "Elko radial Ø5 mm, Raster 2 mm")
            .pad("1", p(0.0, 0.0), p(1.6, 1.6), PadShape::Rect, 0.8)
            .pad("2", p(2.0, 0.0), p(1.6, 1.6), PadShape::Circle, 0.8)
            .silk(p(-1.6, -1.2), p(-1.6, -0.4))
            .silk(p(-2.0, -0.8), p(-1.2, -0.8))
            .cyl(p(1.0, 0.0), 2.5, 0.0, 11.0, [30, 50, 120])
            .done(),
    );
    v.push(
        F::new("Capacitor_SMD:CP_Elec_5x5.4", "SMD-Elko 5 × 5,4 mm")
            .pad("1", p(-2.2, 0.0), p(3.0, 1.6), rr, 0.0)
            .pad("2", p(2.2, 0.0), p(3.0, 1.6), rr, 0.0)
            .silk_rect(p(-2.76, -2.76), p(2.76, 2.76))
            .bx(p(-2.65, -2.65), p(2.65, 2.65), 0.0, 1.0, BLACK)
            .cyl(p(0.0, 0.0), 2.5, 1.0, 5.4, STEEL)
            .done(),
    );
    v.push(
        F::new("Diode_THT:D_DO-41_SOD81_P10.16mm_Horizontal", "Diode DO-41, Raster 10,16 mm")
            .pad("1", p(0.0, 0.0), p(2.2, 2.2), PadShape::Rect, 1.1)
            .pad("2", p(10.16, 0.0), p(2.2, 2.2), PadShape::Oval, 1.1)
            .silk_rect(p(2.47, -1.47), p(7.69, 1.47))
            .silk(p(3.25, -1.47), p(3.25, 1.47))
            .bx(p(2.6, -1.35), p(7.56, 1.35), 0.3, 3.0, BLACK)
            .bx(p(2.8, -1.36), p(3.5, 1.36), 0.3, 3.01, [200, 200, 200])
            .done(),
    );
    v.push(
        F::new("LED_THT:LED_D5.0mm", "LED Ø5 mm, Raster 2,54 mm")
            .pad("1", p(0.0, 0.0), p(1.8, 1.8), PadShape::Rect, 0.9)
            .pad("2", p(2.54, 0.0), p(1.8, 1.8), PadShape::Circle, 0.9)
            .silk(p(-1.29, -1.545), p(-1.29, 1.545))
            .cyl(p(1.27, 0.0), 2.9, 0.0, 1.0, [210, 40, 40])
            .cyl(p(1.27, 0.0), 2.5, 1.0, 8.6, [230, 60, 60])
            .done(),
    );
    v.push(
        F::new("Package_TO_SOT_THT:TO-92_Inline", "TO-92, Anschlüsse in einer Reihe")
            .pad("1", p(0.0, 0.0), p(1.05, 1.5), PadShape::Rect, 0.75)
            .pad("2", p(1.27, 0.0), p(1.05, 1.5), PadShape::Oval, 0.75)
            .pad("3", p(2.54, 0.0), p(1.05, 1.5), PadShape::Oval, 0.75)
            .silk(p(-0.53, 1.85), p(3.07, 1.85))
            .bx(p(-1.0, -1.0), p(3.54, 1.75), 3.0, 8.0, BLACK)
            .done(),
    );
    v.push(
        F::new("Package_TO_SOT_THT:TO-220-3_Vertical", "TO-220, stehend")
            .pad("1", p(0.0, 0.0), p(1.905, 2.0), PadShape::Rect, 1.1)
            .pad("2", p(2.54, 0.0), p(1.905, 2.0), PadShape::Oval, 1.1)
            .pad("3", p(5.08, 0.0), p(1.905, 2.0), PadShape::Oval, 1.1)
            .silk_rect(p(-2.58, -3.27), p(7.62, 1.371))
            .bx(p(-2.46, -1.4), p(7.54, 3.2), 3.0, 12.0, BLACK)
            .bx(p(-2.46, 1.95), p(7.54, 3.25), 12.0, 19.0, STEEL)
            .done(),
    );
    v.push(
        F::new("Crystal:Crystal_HC49-U_Vertical", "Quarz HC-49/U, stehend")
            .pad("1", p(0.0, 0.0), p(1.5, 1.5), PadShape::Circle, 0.8)
            .pad("2", p(4.88, 0.0), p(1.5, 1.5), PadShape::Circle, 0.8)
            .silk_rect(p(-3.2, -2.525), p(8.08, 2.525))
            .bx(p(-3.1, -2.4), p(7.98, 2.4), 0.0, 13.5, STEEL)
            .done(),
    );
    v.push(
        F::new("Potentiometer_THT:Potentiometer_Piher_PT-10-V10_Vertical", "Trimmpotentiometer 10 mm, stehend")
            .pad("1", p(0.0, 0.0), p(1.8, 1.8), PadShape::Rect, 1.0)
            .pad("2", p(2.5, -5.0), p(1.8, 1.8), PadShape::Circle, 1.0)
            .pad("3", p(5.0, 0.0), p(1.8, 1.8), PadShape::Circle, 1.0)
            .silk_rect(p(-2.6, -7.6), p(7.6, 1.6))
            .bx(p(-2.5, -7.5), p(7.5, 1.5), 0.0, 5.0, BLUE)
            .cyl(p(2.5, -3.0), 2.0, 5.0, 6.0, [240, 240, 240])
            .done(),
    );
    v.push(
        F::new("Button_Switch_THT:SW_PUSH_6mm", "Taster 6 × 6 mm, THT")
            .pad("1", p(0.0, 0.0), p(2.0, 2.0), PadShape::Circle, 1.0)
            .pad("2", p(6.5, 0.0), p(2.0, 2.0), PadShape::Circle, 1.0)
            .pad("1", p(0.0, 4.5), p(2.0, 2.0), PadShape::Circle, 1.0)
            .pad("2", p(6.5, 4.5), p(2.0, 2.0), PadShape::Circle, 1.0)
            .silk_rect(p(0.25, -0.75), p(6.25, 5.25))
            .bx(p(0.25, -0.75), p(6.25, 5.25), 0.0, 3.5, BLACK)
            .cyl(p(3.25, 2.25), 1.75, 3.5, 5.0, [40, 40, 40])
            .done(),
    );
    v.push(
        F::new("Battery:BatteryHolder_Keystone_3034_1x20mm", "Knopfzellenhalter CR2032 (SMD)")
            .pad("1", p(-14.68, 0.0), p(2.54, 5.08), rr, 0.0)
            .pad("1", p(14.68, 0.0), p(2.54, 5.08), rr, 0.0)
            .pad("2", p(0.0, 0.0), p(17.8, 17.8), PadShape::Circle, 0.0)
            .cyl(p(0.0, 0.0), 10.0, 0.0, 4.2, STEEL)
            .done(),
    );
    v.push(
        F::new("Buzzer_Beeper:Buzzer_12x9.5RM7.6", "Summer Ø12 mm, Raster 7,6 mm")
            .pad("1", p(0.0, 0.0), p(2.0, 2.0), PadShape::Rect, 1.0)
            .pad("2", p(7.6, 0.0), p(2.0, 2.0), PadShape::Circle, 1.0)
            .cyl(p(3.8, 0.0), 6.0, 0.0, 9.5, BLACK)
            .done(),
    );
    for n in [2, 3, 4, 5, 6, 8, 10] {
        v.push(header1(n));
        v.push(terminal(n));
    }
    // ---- connectors with housing openings (sketch palette "Ports")
    // USB-C receptacle, power only: front at y = -3.65 (board edge), opening 8.34 × 2.56
    v.push(
        F::new("Connector_USB:USB_C_Receptacle_PowerOnly_6P", "USB-C-Buchse, nur Versorgung, liegend – Öffnung „USB-C / Thunderbolt“")
            .pad("A12", p(-2.75, 3.05), p(0.7, 1.2), rr, 0.0)
            .pad("A9", p(-1.75, 3.05), p(0.7, 1.2), rr, 0.0)
            .pad("A5", p(-0.5, 3.05), p(0.8, 1.2), rr, 0.0)
            .pad("B5", p(0.5, 3.05), p(0.8, 1.2), rr, 0.0)
            .pad("B9", p(1.75, 3.05), p(0.7, 1.2), rr, 0.0)
            .pad("B12", p(2.75, 3.05), p(0.7, 1.2), rr, 0.0)
            .pad("S1", p(-4.32, -0.95), p(1.6, 1.6), PadShape::Circle, 1.0)
            .pad("S1", p(4.32, -0.95), p(1.6, 1.6), PadShape::Circle, 1.0)
            .pad("S1", p(-4.32, 2.2), p(1.6, 1.6), PadShape::Circle, 1.0)
            .pad("S1", p(4.32, 2.2), p(1.6, 1.6), PadShape::Circle, 1.0)
            .silk(p(-4.47, -3.65), p(4.47, -3.65))
            .bx(p(-4.47, -3.65), p(4.47, 3.65), 0.0, 3.26, STEEL)
            .port("USB-C / Thunderbolt")
            .done(),
    );
    v.push(
        F::new("Connector_USB:USB_Micro-B_Molex_47346-0001", "Micro-USB-B-Buchse, liegend – Öffnung „Micro-USB“")
            .pad("1", p(-1.3, -2.675), p(0.4, 1.35), rr, 0.0)
            .pad("2", p(-0.65, -2.675), p(0.4, 1.35), rr, 0.0)
            .pad("3", p(0.0, -2.675), p(0.4, 1.35), rr, 0.0)
            .pad("4", p(0.65, -2.675), p(0.4, 1.35), rr, 0.0)
            .pad("5", p(1.3, -2.675), p(0.4, 1.35), rr, 0.0)
            .pad("6", p(-3.6, 0.0), p(1.6, 2.1), PadShape::Oval, 0.6)
            .pad("6", p(3.6, 0.0), p(1.6, 2.1), PadShape::Oval, 0.6)
            .slots(1.3)
            .silk(p(-3.8, 2.5), p(3.8, 2.5))
            .bx(p(-3.75, -2.0), p(3.75, 3.0), 0.0, 2.5, STEEL)
            .port("Micro-USB")
            .done(),
    );
    v.push(
        F::new("Connector_USB:USB_A_Molex_67643_Horizontal", "USB-A-Buchse, liegend – Öffnung „USB-A“")
            .pad("1", p(0.0, 0.0), p(1.6, 1.6), PadShape::Rect, 0.92)
            .pad("2", p(2.5, 0.0), p(1.6, 1.6), PadShape::Circle, 0.92)
            .pad("3", p(4.5, 0.0), p(1.6, 1.6), PadShape::Circle, 0.92)
            .pad("4", p(7.0, 0.0), p(1.6, 1.6), PadShape::Circle, 0.92)
            .pad("5", p(-3.07, 2.71), p(3.0, 3.0), PadShape::Circle, 2.3)
            .pad("5", p(10.07, 2.71), p(3.0, 3.0), PadShape::Circle, 2.3)
            .bx(p(-3.07, -1.0), p(10.07, 13.0), 0.0, 7.0, STEEL)
            .port("USB-A")
            .done(),
    );
    v.push(
        F::new("Connector_BarrelJack:BarrelJack_Horizontal", "Hohlstecker-Buchse 5,5 × 2,1 mm, liegend – Öffnung „DC-Buchse 5,5×2,1“")
            .pad("1", p(0.0, 0.0), p(3.5, 3.5), PadShape::Rect, 1.6)
            .pad("2", p(-6.0, 0.0), p(3.0, 3.0), PadShape::Circle, 1.6)
            .pad("", p(-3.0, 4.7), p(3.0, 3.0), PadShape::Circle, 1.6)
            .silk_rect(p(-13.8, -4.6), p(0.9, 4.6))
            .bx(p(-13.7, -4.5), p(0.8, 4.5), 0.0, 11.0, BLACK)
            .port("DC-Buchse 5,5×2,1")
            .done(),
    );
    v.push(
        F::new("Connector_Audio:Jack_3.5mm_PJ320D_Horizontal", "Klinkenbuchse 3,5 mm, liegend – Öffnung „Klinke 3,5 mm“")
            .pad("S", p(0.0, 0.0), p(2.0, 2.0), PadShape::Circle, 1.2)
            .pad("T", p(-5.0, -2.2), p(2.0, 2.0), PadShape::Circle, 1.2)
            .pad("R", p(-5.0, 2.2), p(2.0, 2.0), PadShape::Circle, 1.2)
            .npth(p(-3.4, 0.0), 1.2)
            .npth(p(3.6, 0.0), 1.2)
            .silk_rect(p(-6.0, -3.1), p(6.0, 3.1))
            .bx(p(-6.0, -3.0), p(6.0, 3.0), 0.0, 5.0, BLACK)
            .cyl(p(6.5, 0.0), 2.5, 0.0, 5.0, BLACK)
            .port("Klinke 3,5 mm")
            .done(),
    );
    {
        let mut f = F::new("Connector_RJ:RJ45_Amphenol_54602-x08_Horizontal", "RJ45-Buchse, liegend – Öffnung „RJ45 (Ethernet)“");
        for i in 0..8 {
            let x = i as f64 * 1.02;
            let y = if i % 2 == 0 { 0.0 } else { 1.78 };
            f = f.pad(&(i + 1).to_string(), p(x, y), p(1.5, 1.5), if i == 0 { PadShape::Rect } else { PadShape::Circle }, 0.89);
        }
        f = f.npth(p(-1.13, 2.54), 3.25).npth(p(8.27, 2.54), 3.25);
        f = f.silk_rect(p(-4.37, -8.1), p(11.51, 9.0));
        v.push(f.bx(p(-4.27, -8.0), p(11.41, 8.9), 0.0, 13.3, STEEL).port("RJ45 (Ethernet)").done());
    }
    v
}

pub fn library() -> &'static [Footprint] {
    static LIB: OnceLock<Vec<Footprint>> = OnceLock::new();
    LIB.get_or_init(build)
}

static EXTRA: std::sync::RwLock<Vec<&'static Footprint>> = std::sync::RwLock::new(Vec::new());

/// Adds footprints from loaded libraries (later ones replace earlier ones with the same id).
pub fn register(fps: Vec<Footprint>) -> usize {
    let mut g = EXTRA.write().unwrap();
    let n = fps.len();
    for f in fps {
        g.retain(|x| x.id != f.id);
        g.push(Box::leak(Box::new(f)));
    }
    n
}

/// Built-in and loaded footprints.
pub fn all() -> Vec<&'static Footprint> {
    let g = EXTRA.read().unwrap();
    library().iter().chain(g.iter().copied()).collect()
}

pub fn find(id: &str) -> Option<&'static Footprint> {
    {
        let g = EXTRA.read().unwrap();
        if let Some(f) = g.iter().rev().find(|f| f.id == id) {
            return Some(f);
        }
    }
    library().iter().find(|f| f.id == id || f.short() == id).or_else(|| EXTRA.read().unwrap().iter().rev().find(|f| f.short() == id).copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_match_the_sketch_palette() {
        let ports = crate::sketch::palette::tabs().iter().find(|t| t.name == "Ports").expect("Ports-Reiter");
        for f in library() {
            if let Some(name) = &f.port {
                assert!(ports.items.iter().any(|i| &i.name == name), "{}: Port {} fehlt in der Palette", f.id, name);
            }
        }
    }

    #[test]
    fn soic_pin_one_top_left() {
        let f = find("SOIC-8_3.9x4.9mm_P1.27mm").unwrap();
        let p1 = &f.pads[0];
        assert_eq!(p1.num, "1");
        assert!(p1.pos.x < 0.0 && p1.pos.y < 0.0);
        let p8 = f.pads.iter().find(|q| q.num == "8").unwrap();
        assert!(p8.pos.x > 0.0 && (p8.pos.y - p1.pos.y).abs() < 1e-9);
    }
}

/// Footprint generators of the footprint editor (like KiCad's footprint wizards).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Gen {
    /// two-pad chip: pad centre distance, pad width (along x) and height
    Chip { span: f64, pw: f64, ph: f64 },
    /// SOIC/SSOP-style two rows: pins, pitch, row distance (pad centres), pad w × h
    Dual { n: usize, pitch: f64, span: f64, pw: f64, ph: f64 },
    /// QFP/QFN: pins per side, pitch, distance between opposite rows, pad w × h, exposed pad
    Quad { per_side: usize, pitch: f64, span: f64, pw: f64, ph: f64, ep: f64 },
    /// THT grid (pin headers, DIP): rows, pins per row, pitch along a row, distance
    /// between rows, drill, pad diameter
    Tht { rows: usize, cols: usize, pitch: f64, span: f64, drill: f64, pad: f64 },
}

impl Gen {
    pub fn name(&self) -> &'static str {
        match self {
            Gen::Chip { .. } => "Chip (2 Pads)",
            Gen::Dual { .. } => "Zwei Reihen (SOIC, SSOP)",
            Gen::Quad { .. } => "Vier Seiten (QFP, QFN)",
            Gen::Tht { .. } => "THT-Raster (Stiftleiste, DIP)",
        }
    }
    pub fn defaults() -> [Gen; 4] {
        [
            Gen::Chip { span: 1.825, pw: 1.025, ph: 1.4 },
            Gen::Dual { n: 8, pitch: 1.27, span: 4.95, pw: 1.95, ph: 0.6 },
            Gen::Quad { per_side: 8, pitch: 0.8, span: 8.4, pw: 1.5, ph: 0.55, ep: 0.0 },
            Gen::Tht { rows: 1, cols: 4, pitch: 2.54, span: 2.54, drill: 1.0, pad: 1.7 },
        ]
    }
}

/// Builds a footprint from a generator; silkscreen, courtyard and a dark body follow.
pub fn generate(id: &str, desc: &str, g: &Gen, body_h: f64) -> Footprint {
    let rr = PadShape::RoundRect(0.25);
    let mut f = F::new(id, desc);
    let body: (P, P);
    match *g {
        Gen::Chip { span, pw, ph } => {
            f = f.pad("1", p(-span / 2.0, 0.0), p(pw, ph), rr, 0.0).pad("2", p(span / 2.0, 0.0), p(pw, ph), rr, 0.0);
            body = (p(-span / 2.0 - pw * 0.3, -ph * 0.45), p(span / 2.0 + pw * 0.3, ph * 0.45));
        }
        Gen::Dual { n, pitch, span, pw, ph } => {
            let half = (n / 2).max(1);
            let y0 = -(half as f64 - 1.0) / 2.0 * pitch;
            for i in 0..half {
                f = f.pad(&(i + 1).to_string(), p(-span / 2.0, y0 + i as f64 * pitch), p(pw, ph), rr, 0.0);
            }
            for i in 0..half {
                f = f.pad(&(half + i + 1).to_string(), p(span / 2.0, -y0 - i as f64 * pitch), p(pw, ph), rr, 0.0);
            }
            let bw = (span - pw).max(0.5) / 2.0 + 0.1;
            body = (p(-bw, y0 - pitch * 0.7), p(bw, -y0 + pitch * 0.7));
        }
        Gen::Quad { per_side, pitch, span, pw, ph, ep } => {
            let n = per_side.max(1);
            let o = (n as f64 - 1.0) / 2.0 * pitch;
            let mut k = 1;
            // counter-clockwise from pin 1 at the top of the left side
            for i in 0..n {
                f = f.pad(&k.to_string(), p(-span / 2.0, -o + i as f64 * pitch), p(pw, ph), rr, 0.0);
                k += 1;
            }
            for i in 0..n {
                let mut x = Pad { num: k.to_string(), pos: p(-o + i as f64 * pitch, span / 2.0), size: p(pw, ph), shape: rr, drill: 0.0, rot: 90.0, npth: false, slot: 0.0, drill_off: P::ZERO };
                x.rot = 90.0;
                f.f.pads.push(x);
                k += 1;
            }
            for i in 0..n {
                f = f.pad(&k.to_string(), p(span / 2.0, o - i as f64 * pitch), p(pw, ph), rr, 0.0);
                k += 1;
            }
            for i in 0..n {
                f.f.pads.push(Pad { num: k.to_string(), pos: p(o - i as f64 * pitch, -span / 2.0), size: p(pw, ph), shape: rr, drill: 0.0, rot: 90.0, npth: false, slot: 0.0, drill_off: P::ZERO });
                k += 1;
            }
            if ep > 0.0 {
                f = f.pad(&k.to_string(), P::ZERO, p(ep, ep), PadShape::RoundRect(0.05), 0.0);
            }
            let bw = (span - pw).max(1.0) / 2.0;
            body = (p(-bw, -bw), p(bw, bw));
        }
        Gen::Tht { rows, cols, pitch, span, drill, pad } => {
            // numbering like a DIP for two rows (down the left, up the right), otherwise row by row
            let (rows, cols) = (rows.max(1), cols.max(1));
            let mut k = 1;
            if rows == 2 {
                for i in 0..cols {
                    f = f.pad(&k.to_string(), p(0.0, i as f64 * pitch), p(pad, pad), if k == 1 { PadShape::Rect } else { PadShape::Oval }, drill);
                    k += 1;
                }
                for i in (0..cols).rev() {
                    f = f.pad(&k.to_string(), p(span, i as f64 * pitch), p(pad, pad), PadShape::Oval, drill);
                    k += 1;
                }
            } else {
                for r in 0..rows {
                    for c in 0..cols {
                        f = f.pad(&k.to_string(), p(r as f64 * span, c as f64 * pitch), p(pad, pad), if k == 1 { PadShape::Rect } else { PadShape::Oval }, drill);
                        k += 1;
                    }
                }
            }
            let (lo, hi) = super::geom::bbox(f.f.pads.iter().map(|q| q.pos)).unwrap_or((P::ZERO, P::ZERO));
            body = (lo - P::splat(pitch / 2.0), hi + P::splat(pitch / 2.0));
        }
    }
    let (lo, hi) = body;
    f = f.silk_rect(lo - P::splat(0.1), hi + P::splat(0.1));
    if !matches!(g, Gen::Chip { .. }) {
        f = f.silk(lo + p(-0.6, -0.1), lo + p(-0.6, 0.9));
    }
    f.bx(lo, hi, 0.0, body_h, BLACK).done()
}
