//! Schematic symbol library: the built-in parts (passives, diodes, transistors, ICs,
//! connectors, switches, power symbols) drawn on KiCad's 2.54 mm (100 mil) grid.
//!
//! Symbol coordinates are millimetres with y pointing down. Each pin's `pos` is its
//! electrical end (where wires attach); the pin line runs from there towards the body along
//! `dir` (degrees: 0 = to the right, 90 = up, 180 = left, 270 = down) for `len`.

use super::geom::{p, P};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const G: f64 = 2.54;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PinType {
    Input,
    Output,
    Bidi,
    TriState,
    Passive,
    PowerIn,
    PowerOut,
    OpenCollector,
    Unspecified,
    NoConnect,
}

impl PinType {
    pub fn name(self) -> &'static str {
        match self {
            PinType::Input => "Eingang",
            PinType::Output => "Ausgang",
            PinType::Bidi => "Bidirektional",
            PinType::TriState => "Tri-State",
            PinType::Passive => "Passiv",
            PinType::PowerIn => "Versorgungseingang",
            PinType::PowerOut => "Versorgungsausgang",
            PinType::OpenCollector => "Open Collector",
            PinType::Unspecified => "Nicht spezifiziert",
            PinType::NoConnect => "Nicht verbunden",
        }
    }
    pub fn kicad(self) -> &'static str {
        match self {
            PinType::Input => "input",
            PinType::Output => "output",
            PinType::Bidi => "bidirectional",
            PinType::TriState => "tri_state",
            PinType::Passive => "passive",
            PinType::PowerIn => "power_in",
            PinType::PowerOut => "power_out",
            PinType::OpenCollector => "open_collector",
            PinType::Unspecified => "unspecified",
            PinType::NoConnect => "no_connect",
        }
    }
    pub fn from_kicad(s: &str) -> PinType {
        match s {
            "input" => PinType::Input,
            "output" => PinType::Output,
            "bidirectional" => PinType::Bidi,
            "tri_state" => PinType::TriState,
            "power_in" => PinType::PowerIn,
            "power_out" => PinType::PowerOut,
            "open_collector" | "open_emitter" => PinType::OpenCollector,
            "no_connect" => PinType::NoConnect,
            "unspecified" | "free" => PinType::Unspecified,
            _ => PinType::Passive,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    pub num: String,
    pub name: String,
    pub pos: P,
    pub dir: f64,
    pub len: f64,
    pub kind: PinType,
    #[serde(default)]
    pub hidden: bool,
    /// unit the pin belongs to (0 = every unit, e.g. shared supply pins)
    #[serde(default)]
    pub unit: u32,
}

impl Pin {
    /// The inner end of the pin line (on the body).
    pub fn inner(&self) -> P {
        let a = self.dir.to_radians();
        self.pos + p(a.cos(), -a.sin()) * self.len
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fill {
    None,
    Outline,
    Background,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Gfx {
    Poly(Vec<P>, Fill),
    Rect(P, P, Fill),
    Circle(P, f64, Fill),
    Arc(P, P, P),
    Text(P, String, f64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Symbol {
    /// library id, KiCad style ("Device:R")
    pub id: String,
    pub category: String,
    pub description: String,
    pub prefix: String,
    pub value: String,
    pub footprint: String,
    pub pins: Vec<Pin>,
    pub gfx: Vec<Gfx>,
    /// power symbol: its value is a global net name
    pub power: bool,
    pub show_pin_numbers: bool,
    pub show_pin_names: bool,
    /// footprints that fit (first = default)
    #[serde(default)]
    pub alt_footprints: Vec<String>,
    /// number of units (gates) in the package, e.g. 4 for a 74HC00
    #[serde(default = "one")]
    pub units: u32,
    /// graphics of single units (the common ones are in `gfx`)
    #[serde(default)]
    pub unit_gfx: Vec<(u32, Gfx)>,
}

fn one() -> u32 {
    1
}

/// Unit letter as KiCad shows it (U1A, U1B …).
pub fn unit_letter(u: u32) -> String {
    if u == 0 {
        return String::new();
    }
    let mut n = u;
    let mut s = String::new();
    while n > 0 {
        n -= 1;
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    s
}

impl Symbol {
    pub fn short(&self) -> &str {
        self.id.rsplit(':').next().unwrap_or(&self.id)
    }
    pub fn pin(&self, num: &str) -> Option<&Pin> {
        self.pins.iter().find(|q| q.num == num)
    }
    /// Pins of one unit (with the shared ones).
    pub fn pins_of(&self, unit: u32) -> impl Iterator<Item = &Pin> {
        self.pins.iter().filter(move |q| q.unit == 0 || q.unit == unit || self.units <= 1)
    }
    /// Graphics of one unit (with the shared ones).
    pub fn gfx_of(&self, unit: u32) -> Vec<&Gfx> {
        self.gfx.iter().chain(self.unit_gfx.iter().filter(|(u, _)| *u == unit || self.units <= 1).map(|(_, g)| g)).collect()
    }
    /// Bounds of one unit.
    pub fn unit_bounds(&self, unit: u32) -> (P, P) {
        let mut pts: Vec<P> = self.pins_of(unit).flat_map(|q| [q.pos, q.inner()]).collect();
        for g in self.gfx_of(unit) {
            match g {
                Gfx::Poly(v, _) => pts.extend(v.iter().copied()),
                Gfx::Rect(a, b, _) => pts.extend([*a, *b]),
                Gfx::Circle(c, r, _) => pts.extend([*c - P::splat(*r), *c + P::splat(*r)]),
                Gfx::Arc(a, m, b) => pts.extend([*a, *m, *b]),
                Gfx::Text(c, _, s) => pts.extend([*c - P::splat(*s), *c + P::splat(*s)]),
            }
        }
        super::geom::bbox(pts).unwrap_or((p(-1.0, -1.0), p(1.0, 1.0)))
    }
    pub fn bounds(&self) -> (P, P) {
        let mut pts: Vec<P> = self.pins.iter().flat_map(|q| [q.pos, q.inner()]).collect();
        for g in &self.gfx {
            match g {
                Gfx::Poly(v, _) => pts.extend(v.iter().copied()),
                Gfx::Rect(a, b, _) => pts.extend([*a, *b]),
                Gfx::Circle(c, r, _) => pts.extend([*c - P::splat(*r), *c + P::splat(*r)]),
                Gfx::Arc(a, m, b) => pts.extend([*a, *m, *b]),
                Gfx::Text(c, _, s) => pts.extend([*c - P::splat(*s), *c + P::splat(*s)]),
            }
        }
        super::geom::bbox(pts).unwrap_or((p(-1.0, -1.0), p(1.0, 1.0)))
    }
}

fn pin(num: &str, name: &str, pos: P, dir: f64, len: f64, kind: PinType) -> Pin {
    Pin { num: num.into(), name: name.into(), pos, dir, len, kind, hidden: false, unit: 0 }
}

struct S {
    s: Symbol,
    unit: u32,
}

impl S {
    fn new(id: &str, cat: &str, desc: &str, prefix: &str, value: &str, fp: &[&str]) -> S {
        S {
            s: Symbol {
                id: id.into(),
                category: cat.into(),
                description: desc.into(),
                prefix: prefix.into(),
                value: value.into(),
                footprint: fp.first().map(|s| s.to_string()).unwrap_or_default(),
                pins: vec![],
                gfx: vec![],
                power: false,
                show_pin_numbers: false,
                show_pin_names: false,
                alt_footprints: fp.iter().map(|s| s.to_string()).collect(),
                units: 1,
                unit_gfx: vec![],
            },
            unit: 0,
        }
    }
    /// Following pins and graphics belong to unit `u` (0 = shared).
    fn unit(mut self, u: u32) -> S {
        self.unit = u;
        self.s.units = self.s.units.max(u);
        self
    }
    fn pin(mut self, num: &str, name: &str, pos: P, dir: f64, len: f64, kind: PinType) -> S {
        let mut q = pin(num, name, pos, dir, len, kind);
        q.unit = self.unit;
        self.s.pins.push(q);
        self
    }
    fn g(mut self, g: Gfx) -> S {
        if self.unit == 0 {
            self.s.gfx.push(g);
        } else {
            self.s.unit_gfx.push((self.unit, g));
        }
        self
    }
    fn poly(self, pts: &[(f64, f64)], fill: Fill) -> S {
        self.g(Gfx::Poly(pts.iter().map(|q| p(q.0, q.1)).collect(), fill))
    }
    fn rect(self, a: (f64, f64), b: (f64, f64), fill: Fill) -> S {
        self.g(Gfx::Rect(p(a.0, a.1), p(b.0, b.1), fill))
    }
    fn circle(self, c: (f64, f64), r: f64, fill: Fill) -> S {
        self.g(Gfx::Circle(p(c.0, c.1), r, fill))
    }
    #[allow(dead_code)]
    fn text(self, c: (f64, f64), t: &str, size: f64) -> S {
        self.g(Gfx::Text(p(c.0, c.1), t.into(), size))
    }
    fn numbers(mut self) -> S {
        self.s.show_pin_numbers = true;
        self
    }
    fn names(mut self) -> S {
        self.s.show_pin_names = true;
        self.s.show_pin_numbers = true;
        self
    }
    fn done(self) -> Symbol {
        self.s
    }
}

/// Two-pin vertical part (pin 1 on top) with pins ending at ±3.81.
fn vertical(id: &str, cat: &str, desc: &str, prefix: &str, value: &str, fp: &[&str], body: f64) -> S {
    let l = 3.81 - body;
    S::new(id, cat, desc, prefix, value, fp).pin("1", "~", p(0.0, -3.81), 270.0, l, PinType::Passive).pin("2", "~", p(0.0, 3.81), 90.0, l, PinType::Passive)
}

/// Two-pin horizontal diode-like part: pin 1 (K) left, pin 2 (A) right.
fn diode_base(id: &str, desc: &str, prefix: &str, value: &str, fp: &[&str]) -> S {
    S::new(id, "Dioden", desc, prefix, value, fp)
        .pin("1", "K", p(-3.81, 0.0), 0.0, 2.54, PinType::Passive)
        .pin("2", "A", p(3.81, 0.0), 180.0, 2.54, PinType::Passive)
        .poly(&[(1.27, -1.27), (1.27, 1.27), (-1.27, 0.0), (1.27, -1.27)], Fill::None)
        .poly(&[(-1.27, 0.0), (1.27, 0.0)], Fill::None)
}

/// IC box: `left`/`right` pins top to bottom as (number, name, type); "" names leave a gap.
fn ic(id: &str, cat: &str, desc: &str, value: &str, fp: &[&str], left: &[(&str, &str, PinType)], right: &[(&str, &str, PinType)], top_pins: &[(&str, &str, PinType)], bottom_pins: &[(&str, &str, PinType)], width: f64) -> S {
    let rows = left.len().max(right.len()).max(1) as f64;
    let top = -((rows + 1.0) / 2.0).ceil() * G;
    let bottom = top + (rows + 1.0) * G;
    let x0 = -width / 2.0;
    let mut s = S::new(id, cat, desc, "U", value, fp).rect((x0, top), (-x0, bottom), Fill::Background).names();
    let ys = |i: usize| top + (i as f64 + 1.0) * G;
    for (i, (n, name, k)) in left.iter().enumerate() {
        if !n.is_empty() {
            s = s.pin(n, name, p(x0 - G, ys(i)), 0.0, G, *k);
        }
    }
    for (i, (n, name, k)) in right.iter().enumerate() {
        if !n.is_empty() {
            s = s.pin(n, name, p(-x0 + G, ys(i)), 180.0, G, *k);
        }
    }
    let cols = |n: usize, i: usize| (i as f64 - (n as f64 - 1.0) / 2.0) * 2.0 * G;
    for (i, (n, name, k)) in top_pins.iter().enumerate() {
        s = s.pin(n, name, p(cols(top_pins.len(), i), top - G), 270.0, G, *k);
    }
    for (i, (n, name, k)) in bottom_pins.iter().enumerate() {
        s = s.pin(n, name, p(cols(bottom_pins.len(), i), bottom + G), 90.0, G, *k);
    }
    s
}

/// One op-amp triangle with +, - and output (unit `u`).
fn opamp_unit(s: S, u: u32, plus: &str, minus: &str, out: &str) -> S {
    use PinType::*;
    s.unit(u)
        .poly(&[(-5.08, -5.08), (5.08, 0.0), (-5.08, 5.08), (-5.08, -5.08)], Fill::Background)
        .pin(plus, "+", p(-7.62, 2.54), 0.0, 2.54, Input)
        .pin(minus, "-", p(-7.62, -2.54), 0.0, 2.54, Input)
        .pin(out, "~", p(7.62, 0.0), 180.0, 2.54, Output)
        .text((-3.81, 2.54), "+", 1.6)
        .text((-3.81, -2.54), "−", 1.6)
}

/// LM358: two op-amps (units A, B) and the supply (unit C), as in KiCad.
fn opamp_dual() -> Symbol {
    use PinType::*;
    let mut s = S::new("Amplifier_Operational:LM358", "ICs", "Zweifach-Operationsverstärker (Einheiten A, B; Versorgung C)", "U", "LM358", &["Package_SO:SOIC-8_3.9x4.9mm_P1.27mm", "Package_DIP:DIP-8_W7.62mm"]).numbers();
    s = opamp_unit(s, 1, "3", "2", "1");
    s = opamp_unit(s, 2, "5", "6", "7");
    s = s.unit(3).rect((-2.54, -5.08), (2.54, 5.08), Fill::Background).pin("8", "V+", p(0.0, -7.62), 270.0, 2.54, PowerIn).pin("4", "V-", p(0.0, 7.62), 90.0, 2.54, PowerIn);
    s.s.show_pin_names = false;
    s.done()
}

/// 74HC00: four NAND gates (units A–D) and the supply (unit E).
fn nand_quad() -> Symbol {
    use PinType::*;
    let mut s = S::new("74xx:74HC00", "ICs", "Vier NAND-Gatter mit 2 Eingängen (Einheiten A–D; Versorgung E)", "U", "74HC00", &["Package_SO:SOIC-14_3.9x8.7mm_P1.27mm", "Package_DIP:DIP-14_W7.62mm"]).numbers();
    for (u, (a, b, y)) in [("1", "2", "3"), ("4", "5", "6"), ("9", "10", "8"), ("12", "13", "11")].iter().enumerate() {
        s = s
            .unit(u as u32 + 1)
            .poly(&[(0.0, -3.81), (-3.81, -3.81), (-3.81, 3.81), (0.0, 3.81)], Fill::None)
            .g(Gfx::Arc(p(0.0, -3.81), p(3.81, 0.0), p(0.0, 3.81)))
            .circle((4.445, 0.0), 0.635, Fill::None)
            .pin(a, "~", p(-7.62, -2.54), 0.0, 3.81, Input)
            .pin(b, "~", p(-7.62, 2.54), 0.0, 3.81, Input)
            .pin(y, "~", p(7.62, 0.0), 180.0, 2.54, Output);
    }
    s = s.unit(5).rect((-2.54, -5.08), (2.54, 5.08), Fill::Background).pin("14", "VCC", p(0.0, -7.62), 270.0, 2.54, PowerIn).pin("7", "GND", p(0.0, 7.62), 90.0, 2.54, PowerIn);
    s.done()
}

fn power(name: &str, gnd: bool) -> Symbol {
    let mut s = S::new(&format!("power:{}", name), "Versorgung", &format!("Versorgungssymbol {}", name), "#PWR", name, &[]);
    s = s.pin("1", name, p(0.0, 0.0), if gnd { 270.0 } else { 90.0 }, 0.0, PinType::PowerIn);
    s.s.pins[0].hidden = true;
    s.s.power = true;
    if gnd {
        s = s.poly(&[(0.0, 0.0), (0.0, 1.27), (1.27, 1.27), (0.0, 2.54), (-1.27, 1.27), (0.0, 1.27)], Fill::None);
    } else {
        s = s.poly(&[(0.0, 0.0), (0.0, -1.27)], Fill::None).poly(&[(-0.762, -1.27 + 0.0), (0.0, -2.54), (0.762, -1.27), (-0.762, -1.27)], Fill::None);
    }
    s.done()
}

fn conn(n: usize) -> Symbol {
    let h = n as f64 * G;
    let y0 = -((n as f64 - 1.0) / 2.0 * G / G).floor() * G;
    let mut s = S::new(
        &format!("Connector_Generic:Conn_01x{:02}", n),
        "Steckverbinder",
        &format!("Stiftleiste, einreihig, {} Kontakte", n),
        "J",
        &format!("Conn_01x{:02}", n),
        &[&format!("Connector_PinHeader_2.54mm:PinHeader_1x{:02}_P2.54mm_Vertical", n), &format!("TerminalBlock:TerminalBlock_1x{:02}_P5.08mm", n)],
    )
    .rect((-1.27, y0 - 1.27), (1.27, y0 + h - 1.27), Fill::Background)
    .numbers();
    for i in 0..n {
        let y = y0 + i as f64 * G;
        s = s.pin(&(i + 1).to_string(), &format!("Pin_{}", i + 1), p(-5.08, y), 0.0, 3.81, PinType::Passive).poly(&[(-1.27, y), (-0.508, y)], Fill::None);
    }
    s.done()
}

fn build() -> Vec<Symbol> {
    use PinType::*;
    let r_fp = ["Resistor_SMD:R_0805_2012Metric", "Resistor_SMD:R_0603_1608Metric", "Resistor_SMD:R_1206_3216Metric", "Resistor_SMD:R_0402_1005Metric", "Resistor_THT:R_Axial_DIN0207_L6.3mm_D2.5mm_P10.16mm_Horizontal"];
    let c_fp = ["Capacitor_SMD:C_0805_2012Metric", "Capacitor_SMD:C_0603_1608Metric", "Capacitor_SMD:C_1206_3216Metric", "Capacitor_SMD:C_0402_1005Metric", "Capacitor_THT:C_Disc_D5.0mm_W2.5mm_P5.00mm"];
    let mut v = vec![
        vertical("Device:R", "Passiv", "Widerstand", "R", "10k", &r_fp, 2.54).rect((-1.016, -2.54), (1.016, 2.54), Fill::None).done(),
        vertical("Device:C", "Passiv", "Kondensator, ungepolt", "C", "100n", &c_fp, 0.762)
            .poly(&[(-2.032, -0.762), (2.032, -0.762)], Fill::None)
            .poly(&[(-2.032, 0.762), (2.032, 0.762)], Fill::None)
            .done(),
        vertical("Device:C_Polarized", "Passiv", "Elektrolytkondensator, gepolt", "C", "10u", &["Capacitor_THT:CP_Radial_D5.0mm_P2.00mm", "Capacitor_SMD:CP_Elec_5x5.4"], 0.762)
            .rect((-2.286, -0.762), (2.286, -0.381), Fill::None)
            .rect((-2.286, 0.762), (2.286, 0.381), Fill::Outline)
            .poly(&[(-1.778, -2.286), (-0.762, -2.286)], Fill::None)
            .poly(&[(-1.27, -2.794), (-1.27, -1.778)], Fill::None)
            .done(),
        vertical("Device:L", "Passiv", "Spule", "L", "10u", &["Inductor_SMD:L_0805_2012Metric", "Inductor_SMD:L_1206_3216Metric"], 2.54)
            .g(Gfx::Arc(p(0.0, -2.54), p(0.635, -1.905), p(0.0, -1.27)))
            .g(Gfx::Arc(p(0.0, -1.27), p(0.635, -0.635), p(0.0, 0.0)))
            .g(Gfx::Arc(p(0.0, 0.0), p(0.635, 0.635), p(0.0, 1.27)))
            .g(Gfx::Arc(p(0.0, 1.27), p(0.635, 1.905), p(0.0, 2.54)))
            .done(),
        vertical("Device:Fuse", "Passiv", "Sicherung", "F", "500mA", &["Fuse:Fuse_1206_3216Metric"], 2.54).rect((-0.762, -2.54), (0.762, 2.54), Fill::None).poly(&[(0.0, -2.54), (0.0, 2.54)], Fill::None).done(),
        S::new("Device:Crystal", "Passiv", "Quarz", "Y", "16MHz", &["Crystal:Crystal_HC49-U_Vertical"])
            .pin("1", "1", p(-3.81, 0.0), 0.0, 1.905, Passive)
            .pin("2", "2", p(3.81, 0.0), 180.0, 1.905, Passive)
            .rect((-1.143, -2.54), (1.143, 2.54), Fill::None)
            .poly(&[(-1.905, -1.27), (-1.905, 1.27)], Fill::None)
            .poly(&[(1.905, -1.27), (1.905, 1.27)], Fill::None)
            .done(),
        S::new("Device:R_Potentiometer", "Passiv", "Potentiometer", "RV", "10k", &["Potentiometer_THT:Potentiometer_Piher_PT-10-V10_Vertical"])
            .pin("1", "1", p(0.0, -3.81), 270.0, 1.27, Passive)
            .pin("2", "2", p(3.81, 0.0), 180.0, 2.794, Passive)
            .pin("3", "3", p(0.0, 3.81), 90.0, 1.27, Passive)
            .rect((-1.016, -2.54), (1.016, 2.54), Fill::None)
            .poly(&[(1.016, 0.0), (1.778, -0.508), (1.778, 0.508), (1.016, 0.0)], Fill::Outline)
            .done(),
        diode_base("Device:D", "Diode", "D", "1N4148", &["Diode_SMD:D_SOD-123", "Diode_THT:D_DO-41_SOD81_P10.16mm_Horizontal"]).poly(&[(-1.27, -1.27), (-1.27, 1.27)], Fill::None).done(),
        diode_base("Device:D_Zener", "Z-Diode", "D", "BZX84C5V1", &["Diode_SMD:D_SOD-123"]).poly(&[(-0.762, -1.27), (-1.27, -1.27), (-1.27, 1.27), (-1.778, 1.27)], Fill::None).done(),
        diode_base("Device:D_Schottky", "Schottky-Diode", "D", "1N5819", &["Diode_SMD:D_SOD-123", "Diode_THT:D_DO-41_SOD81_P10.16mm_Horizontal"])
            .poly(&[(-0.762, -0.762), (-0.762, -1.27), (-1.27, -1.27), (-1.27, 1.27), (-1.778, 1.27), (-1.778, 0.762)], Fill::None)
            .done(),
        diode_base("Device:LED", "Leuchtdiode", "D", "LED", &["LED_SMD:LED_0805_2012Metric", "LED_THT:LED_D5.0mm"])
            .poly(&[(-1.27, -1.27), (-1.27, 1.27)], Fill::None)
            .poly(&[(-0.254, -1.778), (-1.524, -3.048), (-1.016, -3.048)], Fill::None)
            .poly(&[(1.016, -1.778), (-0.254, -3.048), (0.254, -3.048)], Fill::None)
            .done(),
        // transistors (KiCad pin order of the SOT-23 / TO-92 variants)
        S::new("Device:Q_NPN_BEC", "Transistoren", "NPN-Transistor (B, E, C)", "Q", "BC847", &["Package_TO_SOT_SMD:SOT-23", "Package_TO_SOT_THT:TO-92_Inline"])
            .pin("1", "B", p(-5.08, 0.0), 0.0, 3.81, Input)
            .pin("3", "C", p(2.54, -5.08), 270.0, 2.794, Passive)
            .pin("2", "E", p(2.54, 5.08), 90.0, 2.794, Passive)
            .circle((1.27, 0.0), 2.794, Fill::None)
            .poly(&[(0.635, -1.905), (0.635, 1.905)], Fill::None)
            .poly(&[(0.635, -0.635), (2.54, -2.286)], Fill::None)
            .poly(&[(0.635, 0.635), (2.54, 2.286)], Fill::None)
            .poly(&[(-1.27, 0.0), (0.635, 0.0)], Fill::None)
            .poly(&[(1.778, 1.778), (2.286, 1.27), (2.54, 2.286), (1.778, 1.778)], Fill::Outline)
            .done(),
        S::new("Device:Q_PNP_BEC", "Transistoren", "PNP-Transistor (B, E, C)", "Q", "BC857", &["Package_TO_SOT_SMD:SOT-23", "Package_TO_SOT_THT:TO-92_Inline"])
            .pin("1", "B", p(-5.08, 0.0), 0.0, 3.81, Input)
            .pin("3", "C", p(2.54, 5.08), 90.0, 2.794, Passive)
            .pin("2", "E", p(2.54, -5.08), 270.0, 2.794, Passive)
            .circle((1.27, 0.0), 2.794, Fill::None)
            .poly(&[(0.635, -1.905), (0.635, 1.905)], Fill::None)
            .poly(&[(0.635, -0.635), (2.54, -2.286)], Fill::None)
            .poly(&[(0.635, 0.635), (2.54, 2.286)], Fill::None)
            .poly(&[(-1.27, 0.0), (0.635, 0.0)], Fill::None)
            .poly(&[(1.27, -1.016), (1.778, -1.778), (0.889, -1.524), (1.27, -1.016)], Fill::Outline)
            .done(),
        S::new("Device:Q_NMOS_GSD", "Transistoren", "N-Kanal-MOSFET (G, S, D)", "Q", "2N7002", &["Package_TO_SOT_SMD:SOT-23", "Package_TO_SOT_THT:TO-220-3_Vertical"])
            .pin("1", "G", p(-5.08, 0.0), 0.0, 4.064, Input)
            .pin("3", "D", p(2.54, -5.08), 270.0, 2.794, Passive)
            .pin("2", "S", p(2.54, 5.08), 90.0, 2.794, Passive)
            .circle((1.27, 0.0), 2.794, Fill::None)
            .poly(&[(-0.508, -1.524), (-0.508, 1.524)], Fill::None)
            .poly(&[(0.254, -1.778), (0.254, -1.016)], Fill::None)
            .poly(&[(0.254, -0.381), (0.254, 0.381)], Fill::None)
            .poly(&[(0.254, 1.016), (0.254, 1.778)], Fill::None)
            .poly(&[(0.254, -1.397), (2.54, -1.397), (2.54, -2.286)], Fill::None)
            .poly(&[(0.254, 1.397), (2.54, 1.397), (2.54, 2.286)], Fill::None)
            .poly(&[(0.254, 0.0), (2.54, 0.0), (2.54, 1.397)], Fill::None)
            .poly(&[(0.254, 0.0), (1.016, -0.381), (1.016, 0.381), (0.254, 0.0)], Fill::Outline)
            .done(),
        S::new("Device:Q_PMOS_GSD", "Transistoren", "P-Kanal-MOSFET (G, S, D)", "Q", "AO3401", &["Package_TO_SOT_SMD:SOT-23"])
            .pin("1", "G", p(-5.08, 0.0), 0.0, 4.064, Input)
            .pin("3", "D", p(2.54, 5.08), 90.0, 2.794, Passive)
            .pin("2", "S", p(2.54, -5.08), 270.0, 2.794, Passive)
            .circle((1.27, 0.0), 2.794, Fill::None)
            .poly(&[(-0.508, -1.524), (-0.508, 1.524)], Fill::None)
            .poly(&[(0.254, -1.778), (0.254, -1.016)], Fill::None)
            .poly(&[(0.254, -0.381), (0.254, 0.381)], Fill::None)
            .poly(&[(0.254, 1.016), (0.254, 1.778)], Fill::None)
            .poly(&[(0.254, -1.397), (2.54, -1.397), (2.54, -2.286)], Fill::None)
            .poly(&[(0.254, 1.397), (2.54, 1.397), (2.54, 2.286)], Fill::None)
            .poly(&[(0.254, 0.0), (2.54, 0.0), (2.54, -1.397)], Fill::None)
            .poly(&[(1.778, 0.0), (1.016, -0.381), (1.016, 0.381), (1.778, 0.0)], Fill::Outline)
            .done(),
        // ICs
        ic(
            "Timer:NE555P",
            "ICs",
            "Timer NE555, DIP-8",
            "NE555",
            &["Package_DIP:DIP-8_W7.62mm", "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm"],
            &[("2", "TR", Input), ("5", "CV", Input), ("4", "R", Input)],
            &[("3", "Q", Output), ("7", "DIS", OpenCollector), ("6", "THR", Input)],
            &[("8", "VCC", PowerIn)],
            &[("1", "GND", PowerIn)],
            15.24,
        )
        .done(),
        opamp_dual(),
        nand_quad(),
        ic(
            "Regulator_Linear:AMS1117-3.3",
            "ICs",
            "Linearregler 3,3 V, 1 A, SOT-223",
            "AMS1117-3.3",
            &["Package_TO_SOT_SMD:SOT-223-3_TabPin2"],
            &[("3", "VI", PowerIn)],
            &[("2", "VO", PowerOut)],
            &[],
            &[("1", "GND", PowerIn)],
            10.16,
        )
        .done(),
        ic("Regulator_Linear:L7805", "ICs", "Linearregler 5 V, 1,5 A, TO-220", "L7805", &["Package_TO_SOT_THT:TO-220-3_Vertical"], &[("1", "IN", PowerIn)], &[("3", "OUT", PowerOut)], &[], &[("2", "GND", PowerIn)], 10.16).done(),
        ic(
            "MCU_Microchip_ATtiny:ATtiny85-20P",
            "ICs",
            "8-Bit-Mikrocontroller, 8 kB Flash, DIP-8",
            "ATtiny85-20P",
            &["Package_DIP:DIP-8_W7.62mm", "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm"],
            &[("5", "PB0", Bidi), ("6", "PB1", Bidi), ("7", "PB2", Bidi), ("2", "PB3", Bidi), ("3", "PB4", Bidi), ("1", "PB5/~{RESET}", Bidi)],
            &[],
            &[("8", "VCC", PowerIn)],
            &[("4", "GND", PowerIn)],
            15.24,
        )
        .done(),
        ic(
            "MCU_Microchip_ATmega:ATmega328P-P",
            "ICs",
            "8-Bit-Mikrocontroller, 32 kB Flash, DIP-28",
            "ATmega328P-P",
            &["Package_DIP:DIP-28_W7.62mm"],
            &[
                ("1", "~{RESET}/PC6", Bidi),
                ("9", "XTAL1/PB6", Bidi),
                ("10", "XTAL2/PB7", Bidi),
                ("21", "AREF", Passive),
                ("", "", Passive),
                ("23", "PC0", Bidi),
                ("24", "PC1", Bidi),
                ("25", "PC2", Bidi),
                ("26", "PC3", Bidi),
                ("27", "PC4", Bidi),
                ("28", "PC5", Bidi),
            ],
            &[
                ("14", "PB0", Bidi),
                ("15", "PB1", Bidi),
                ("16", "PB2", Bidi),
                ("17", "PB3", Bidi),
                ("18", "PB4", Bidi),
                ("19", "PB5", Bidi),
                ("2", "PD0", Bidi),
                ("3", "PD1", Bidi),
                ("4", "PD2", Bidi),
                ("5", "PD3", Bidi),
                ("6", "PD4", Bidi),
                ("11", "PD5", Bidi),
                ("12", "PD6", Bidi),
                ("13", "PD7", Bidi),
            ],
            &[("7", "VCC", PowerIn), ("20", "AVCC", PowerIn)],
            &[("8", "GND", PowerIn), ("22", "GND", PowerIn)],
            20.32,
        )
        .done(),
        ic(
            "74xx:74HC595",
            "ICs",
            "8-Bit-Schieberegister mit Ausgangslatch",
            "74HC595",
            &["Package_SO:SOIC-16_3.9x9.9mm_P1.27mm", "Package_DIP:DIP-16_W7.62mm"],
            &[("14", "SER", Input), ("11", "SRCLK", Input), ("10", "~{SRCLR}", Input), ("12", "RCLK", Input), ("13", "~{OE}", Input)],
            &[("15", "QA", TriState), ("1", "QB", TriState), ("2", "QC", TriState), ("3", "QD", TriState), ("4", "QE", TriState), ("5", "QF", TriState), ("6", "QG", TriState), ("7", "QH", TriState), ("9", "QH'", Output)],
            &[("16", "VCC", PowerIn)],
            &[("8", "GND", PowerIn)],
            15.24,
        )
        .done(),
        // connectors
        S::new("Connector:USB_C_Receptacle_PowerOnly_6P", "Steckverbinder", "USB-C-Buchse, nur Versorgung (6 Kontakte) – Gehäuseöffnung aus der Skizzenpalette „Ports“", "J", "USB_C_Power", &["Connector_USB:USB_C_Receptacle_PowerOnly_6P"])
            .pin("A9", "VBUS", p(7.62, -5.08), 180.0, 2.54, Passive)
            .pin("B9", "VBUS", p(7.62, -5.08), 180.0, 2.54, Passive)
            .pin("A5", "CC1", p(7.62, 0.0), 180.0, 2.54, Bidi)
            .pin("B5", "CC2", p(7.62, 2.54), 180.0, 2.54, Bidi)
            .pin("A12", "GND", p(2.54, 10.16), 90.0, 2.54, Passive)
            .pin("B12", "GND", p(2.54, 10.16), 90.0, 2.54, Passive)
            .pin("S1", "SHIELD", p(-2.54, 10.16), 90.0, 2.54, Passive)
            .rect((-5.08, -7.62), (5.08, 7.62), Fill::Background)
            .rect((-4.064, -3.81), (-2.54, 2.54), Fill::Outline)
            .names()
            .done(),
        S::new("Connector:USB_B_Micro", "Steckverbinder", "Micro-USB-B-Buchse – Gehäuseöffnung aus der Skizzenpalette „Ports“", "J", "USB_B_Micro", &["Connector_USB:USB_Micro-B_Molex_47346-0001"])
            .pin("1", "VBUS", p(7.62, -5.08), 180.0, 2.54, PowerOut)
            .pin("2", "D-", p(7.62, -2.54), 180.0, 2.54, Bidi)
            .pin("3", "D+", p(7.62, 0.0), 180.0, 2.54, Bidi)
            .pin("4", "ID", p(7.62, 2.54), 180.0, 2.54, Passive)
            .pin("5", "GND", p(0.0, 10.16), 90.0, 2.54, PowerOut)
            .pin("6", "Shield", p(-2.54, 10.16), 90.0, 2.54, Passive)
            .rect((-5.08, -7.62), (5.08, 7.62), Fill::Background)
            .names()
            .done(),
        S::new("Connector:USB_A", "Steckverbinder", "USB-A-Buchse – Gehäuseöffnung aus der Skizzenpalette „Ports“", "J", "USB_A", &["Connector_USB:USB_A_Molex_67643_Horizontal"])
            .pin("1", "VBUS", p(7.62, -5.08), 180.0, 2.54, Passive)
            .pin("2", "D-", p(7.62, -2.54), 180.0, 2.54, Bidi)
            .pin("3", "D+", p(7.62, 0.0), 180.0, 2.54, Bidi)
            .pin("4", "GND", p(0.0, 10.16), 90.0, 2.54, Passive)
            .pin("5", "Shield", p(-2.54, 10.16), 90.0, 2.54, Passive)
            .rect((-5.08, -7.62), (5.08, 7.62), Fill::Background)
            .names()
            .done(),
        S::new("Connector:Barrel_Jack", "Steckverbinder", "Hohlstecker-Buchse 5,5 × 2,1 mm – Gehäuseöffnung aus der Skizzenpalette „Ports“", "J", "Barrel_Jack", &["Connector_BarrelJack:BarrelJack_Horizontal"])
            .pin("1", "~", p(5.08, -2.54), 180.0, 2.54, Passive)
            .pin("2", "~", p(5.08, 2.54), 180.0, 2.54, Passive)
            .rect((-5.08, -3.81), (2.54, 3.81), Fill::Background)
            .rect((-4.318, -3.048), (-2.286, -2.032), Fill::Outline)
            .poly(&[(-2.286, -2.54), (2.54, -2.54)], Fill::None)
            .poly(&[(2.54, 2.54), (-1.27, 2.54), (-1.778, 1.778), (-2.286, 2.54)], Fill::None)
            .numbers()
            .done(),
        S::new("Connector:AudioJack3", "Steckverbinder", "Klinkenbuchse 3,5 mm, 3-polig – Gehäuseöffnung aus der Skizzenpalette „Ports“", "J", "AudioJack3", &["Connector_Audio:Jack_3.5mm_PJ320D_Horizontal"])
            .pin("S", "S", p(5.08, 2.54), 180.0, 2.54, Passive)
            .pin("T", "T", p(5.08, -2.54), 180.0, 2.54, Passive)
            .pin("R", "R", p(5.08, 0.0), 180.0, 2.54, Passive)
            .rect((-5.08, -3.81), (2.54, 3.81), Fill::Background)
            .names()
            .done(),
        S::new("Connector:RJ45", "Steckverbinder", "RJ45-Buchse (Ethernet) – Gehäuseöffnung aus der Skizzenpalette „Ports“", "J", "RJ45", &["Connector_RJ:RJ45_Amphenol_54602-x08_Horizontal"])
            .pin("1", "1", p(7.62, -7.62), 180.0, 2.54, Passive)
            .pin("2", "2", p(7.62, -5.08), 180.0, 2.54, Passive)
            .pin("3", "3", p(7.62, -2.54), 180.0, 2.54, Passive)
            .pin("4", "4", p(7.62, 0.0), 180.0, 2.54, Passive)
            .pin("5", "5", p(7.62, 2.54), 180.0, 2.54, Passive)
            .pin("6", "6", p(7.62, 5.08), 180.0, 2.54, Passive)
            .pin("7", "7", p(7.62, 7.62), 180.0, 2.54, Passive)
            .pin("8", "8", p(7.62, 10.16), 180.0, 2.54, Passive)
            .rect((-5.08, -10.16), (5.08, 12.7), Fill::Background)
            .names()
            .done(),
        // switches and others
        S::new("Switch:SW_Push", "Schalter", "Taster, Schließer", "SW", "SW_Push", &["Button_Switch_THT:SW_PUSH_6mm"])
            .pin("1", "1", p(-5.08, 0.0), 0.0, 2.54, Passive)
            .pin("2", "2", p(5.08, 0.0), 180.0, 2.54, Passive)
            .circle((-2.032, 0.0), 0.508, Fill::None)
            .circle((2.032, 0.0), 0.508, Fill::None)
            .poly(&[(-2.54, -1.27), (2.54, -1.27)], Fill::None)
            .poly(&[(0.0, -1.27), (0.0, -3.048)], Fill::None)
            .done(),
        vertical("Device:Battery_Cell", "Sonstige", "Batteriezelle", "BT", "CR2032", &["Battery:BatteryHolder_Keystone_3034_1x20mm"], 1.016)
            .rect((-2.286, -1.016), (2.286, -0.762), Fill::Outline)
            .rect((-1.524, 0.508), (1.524, 0.762), Fill::Outline)
            .poly(&[(0.762, -2.032), (1.778, -2.032)], Fill::None)
            .poly(&[(1.27, -2.54), (1.27, -1.524)], Fill::None)
            .done(),
        S::new("Device:Buzzer", "Sonstige", "Summer", "BZ", "Buzzer", &["Buzzer_Beeper:Buzzer_12x9.5RM7.6"])
            .pin("1", "-", p(-2.54, 5.08), 90.0, 2.54, Passive)
            .pin("2", "+", p(2.54, 5.08), 90.0, 2.54, Passive)
            .g(Gfx::Arc(p(-3.81, 2.54), p(0.0, -1.27), p(3.81, 2.54)))
            .poly(&[(-3.81, 2.54), (3.81, 2.54)], Fill::None)
            .numbers()
            .done(),
        // power
        power("GND", true),
        power("VCC", false),
        power("+5V", false),
        power("+3V3", false),
        power("+12V", false),
        power("VBUS", false),
    ];
    let mut flag = S::new("power:PWR_FLAG", "Versorgung", "Kennzeichnet ein Netz als versorgt (ERC)", "#FLG", "PWR_FLAG", &[]).pin("1", "pwr", p(0.0, 0.0), 90.0, 0.0, PinType::PowerOut).poly(
        &[(0.0, 0.0), (0.0, -1.27), (-1.016, -1.905), (0.0, -2.54), (1.016, -1.905), (0.0, -1.27)],
        Fill::None,
    );
    flag.s.power = true;
    flag.s.pins[0].hidden = true;
    v.push(flag.done());
    for n in [2, 3, 4, 5, 6, 8, 10] {
        v.push(conn(n));
    }
    v
}

/// The built-in library. Stacked pins (same place, e.g. both VBUS contacts of USB-C) are
/// drawn once: the later ones are hidden but stay electrically connected.
pub fn library() -> &'static [Symbol] {
    static LIB: OnceLock<Vec<Symbol>> = OnceLock::new();
    LIB.get_or_init(|| {
        let mut v = build();
        for s in &mut v {
            for i in 1..s.pins.len() {
                let (head, tail) = s.pins.split_at_mut(i);
                if head.iter().any(|q| q.unit == tail[0].unit && (q.pos - tail[0].pos).length() < 1e-6) {
                    tail[0].hidden = true;
                }
            }
        }
        v
    })
}

static EXTRA: std::sync::RwLock<Vec<&'static Symbol>> = std::sync::RwLock::new(Vec::new());

/// Adds symbols from loaded libraries (later ones replace earlier ones with the same id).
pub fn register(syms: Vec<Symbol>) -> usize {
    let mut g = EXTRA.write().unwrap();
    let n = syms.len();
    for s in syms {
        g.retain(|x| x.id != s.id);
        g.push(Box::leak(Box::new(s)));
    }
    n
}

/// Built-in and loaded symbols.
pub fn all() -> Vec<&'static Symbol> {
    let g = EXTRA.read().unwrap();
    library().iter().chain(g.iter().copied()).collect()
}

pub fn find(id: &str) -> Option<&'static Symbol> {
    {
        let g = EXTRA.read().unwrap();
        if let Some(s) = g.iter().rev().find(|s| s.id == id) {
            return Some(s);
        }
    }
    library().iter().find(|s| s.id == id || s.short() == id)
}

pub fn categories() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = vec![];
    for s in all() {
        if !out.contains(&s.category.as_str()) {
            out.push(&s.category);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_sit_on_the_grid() {
        for s in library() {
            for q in &s.pins {
                for c in [q.pos.x, q.pos.y] {
                    let k = c / 1.27;
                    assert!((k - k.round()).abs() < 1e-6, "{} Pin {} liegt nicht im Raster: {:?}", s.id, q.num, q.pos);
                }
            }
        }
    }

    #[test]
    fn default_footprints_exist() {
        for s in library() {
            for f in &s.alt_footprints {
                assert!(super::super::footprints::find(f).is_some(), "{}: Footprint {} fehlt", s.id, f);
            }
        }
    }
}

/// Side of an IC box a pin sits on (symbol editor).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Left, Side::Right, Side::Top, Side::Bottom];
    pub fn name(self) -> &'static str {
        match self {
            Side::Left => "links",
            Side::Right => "rechts",
            Side::Top => "oben",
            Side::Bottom => "unten",
        }
    }
    /// Side of an existing pin from its direction.
    pub fn of(dir: f64) -> Side {
        match dir.rem_euclid(360.0).round() as i64 {
            0 => Side::Left,
            180 => Side::Right,
            270 => Side::Top,
            _ => Side::Bottom,
        }
    }
}

/// One row of the symbol editor's pin table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PinRow {
    pub num: String,
    pub name: String,
    pub kind: PinType,
    pub side: Side,
}

pub const PIN_TYPES: [PinType; 10] = [PinType::Input, PinType::Output, PinType::Bidi, PinType::TriState, PinType::Passive, PinType::PowerIn, PinType::PowerOut, PinType::OpenCollector, PinType::Unspecified, PinType::NoConnect];

/// An IC box symbol from a pin table, on the 2,54 mm grid (KiCad style): pins spaced
/// one grid apart on each side, the box sized to the longest name.
pub fn box_symbol(id: &str, prefix: &str, value: &str, footprint: &str, description: &str, rows: &[PinRow]) -> Symbol {
    let on = |s: Side| rows.iter().filter(move |r| r.side == s).collect::<Vec<_>>();
    let (l, r, t, b) = (on(Side::Left), on(Side::Right), on(Side::Top), on(Side::Bottom));
    let longest = |v: &[&PinRow]| v.iter().map(|x| x.name.chars().count()).max().unwrap_or(0) as f64;
    let name_w = (longest(&l) + longest(&r)) * 1.0 + 2.0 * G;
    let cols = t.len().max(b.len()) as f64;
    let half_w = ((name_w.max((cols + 1.0) * G) / 2.0) / G).ceil() * G;
    let rows_n = l.len().max(r.len()).max(1) as f64;
    let top = -((rows_n + 1.0) / 2.0).ceil() * G;
    let bottom = top + (rows_n + 1.0) * G;
    let mut s = Symbol {
        id: id.into(),
        category: "Eigene".into(),
        description: description.into(),
        prefix: if prefix.is_empty() { "U".into() } else { prefix.into() },
        value: value.into(),
        footprint: footprint.into(),
        pins: vec![],
        gfx: vec![Gfx::Rect(p(-half_w, top), p(half_w, bottom), Fill::Background)],
        power: false,
        show_pin_numbers: true,
        show_pin_names: true,
        alt_footprints: if footprint.is_empty() { vec![] } else { vec![footprint.into()] },
        units: 1,
        unit_gfx: vec![],
    };
    for (i, x) in l.iter().enumerate() {
        s.pins.push(pin(&x.num, &x.name, p(-half_w - G, top + (i as f64 + 1.0) * G), 0.0, G, x.kind));
    }
    for (i, x) in r.iter().enumerate() {
        s.pins.push(pin(&x.num, &x.name, p(half_w + G, top + (i as f64 + 1.0) * G), 180.0, G, x.kind));
    }
    let col = |n: usize, i: usize| ((i as f64 - (n as f64 - 1.0) / 2.0) * G / G).round() * G;
    for (i, x) in t.iter().enumerate() {
        s.pins.push(pin(&x.num, &x.name, p(col(t.len(), i), top - G), 270.0, G, x.kind));
    }
    for (i, x) in b.iter().enumerate() {
        s.pins.push(pin(&x.num, &x.name, p(col(b.len(), i), bottom + G), 90.0, G, x.kind));
    }
    s
}
