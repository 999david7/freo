//! Manufacturing outputs: Gerber RS-274X (X2 attributes) for copper, solder mask, paste,
//! silkscreen and board outline, plus Excellon drill files (plated and non-plated).
//!
//! Gerber coordinates are y-up, so the board's y-down coordinates are negated.

use super::board::{layer_name, Board, B_CU, F_CU};
use super::footprints::PadShape;
use super::geom::{p, Hull, P};
use crate::slicer::poly;
use std::collections::BTreeMap;
use std::fmt::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Copper(u8),
    Mask(bool),
    Paste(bool),
    Silk(bool),
    Edge,
}

impl Layer {
    /// File name suffix with Protel extension, as most board houses expect.
    pub fn file_suffix(self, layers: u8) -> String {
        match self {
            Layer::Copper(0) => "-F_Cu.gtl".into(),
            Layer::Copper(1) => "-B_Cu.gbl".into(),
            Layer::Copper(k) => format!("-In{}_Cu.g{}", k - 1, k),
            Layer::Mask(top) => if top { "-F_Mask.gts" } else { "-B_Mask.gbs" }.into(),
            Layer::Paste(top) => if top { "-F_Paste.gtp" } else { "-B_Paste.gbp" }.into(),
            Layer::Silk(top) => if top { "-F_Silkscreen.gto" } else { "-B_Silkscreen.gbo" }.into(),
            Layer::Edge => {
                let _ = layers;
                "-Edge_Cuts.gm1".into()
            }
        }
    }
    fn function(self, layers: u8) -> String {
        match self {
            Layer::Copper(0) => "Copper,L1,Top".into(),
            Layer::Copper(1) => format!("Copper,L{},Bot", layers),
            Layer::Copper(k) => format!("Copper,L{},Inr", k),
            Layer::Mask(t) => format!("Soldermask,{}", if t { "Top" } else { "Bot" }),
            Layer::Paste(t) => format!("Paste,{}", if t { "Top" } else { "Bot" }),
            Layer::Silk(t) => format!("Legend,{}", if t { "Top" } else { "Bot" }),
            Layer::Edge => "Profile,NP".into(),
        }
    }
    pub fn all(layers: u8) -> Vec<Layer> {
        let mut v = vec![Layer::Copper(F_CU)];
        if layers >= 4 {
            v.extend([Layer::Copper(2), Layer::Copper(3)]);
        }
        v.extend([Layer::Copper(B_CU), Layer::Mask(true), Layer::Mask(false), Layer::Paste(true), Layer::Paste(false), Layer::Silk(true), Layer::Silk(false), Layer::Edge]);
        v
    }
}

/// Solder mask opening around pads.
pub const MASK_MARGIN: f64 = 0.05;
pub const SILK_WIDTH: f64 = 0.12;

fn c(v: f64) -> i64 {
    (v * 1e6).round() as i64
}

struct G {
    head: String,
    body: String,
    apertures: BTreeMap<String, usize>,
    current: Option<usize>,
    polarity_dark: bool,
}

impl G {
    fn new(function: &str, name: &str) -> G {
        let mut head = String::new();
        let _ = writeln!(head, "%TF.GenerationSoftware,Freo,PCB,{}*%", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(head, "%TF.ProjectId,{},00000000-0000-0000-0000-000000000000,1*%", name.replace(',', "_"));
        let _ = writeln!(head, "%TF.SameCoordinates,Original*%");
        let _ = writeln!(head, "%TF.FileFunction,{}*%", function);
        let _ = writeln!(head, "%TF.FilePolarity,Positive*%");
        let _ = writeln!(head, "%FSLAX46Y46*%");
        let _ = writeln!(head, "G04 Gerber RS-274X, erzeugt von Freo*");
        let _ = writeln!(head, "%MOMM*%");
        let _ = writeln!(head, "%LPD*%");
        let _ = writeln!(head, "G01*");
        G { head, body: String::new(), apertures: BTreeMap::new(), current: None, polarity_dark: true }
    }
    fn aperture(&mut self, def: String) -> usize {
        let n = self.apertures.len() + 10;
        let d = *self.apertures.entry(def).or_insert(n);
        if self.current != Some(d) {
            let _ = writeln!(self.body, "D{}*", d);
            self.current = Some(d);
        }
        d
    }
    fn polarity(&mut self, dark: bool) {
        if self.polarity_dark != dark {
            let _ = writeln!(self.body, "%LP{}*%", if dark { "D" } else { "C" });
            self.polarity_dark = dark;
        }
    }
    fn xy(q: P) -> String {
        format!("X{}Y{}", c(q.x), c(-q.y))
    }
    fn line(&mut self, a: P, b: P, w: f64) {
        self.aperture(format!("C,{:.6}", w));
        let _ = writeln!(self.body, "{}D02*", Self::xy(a));
        let _ = writeln!(self.body, "{}D01*", Self::xy(b));
    }
    fn flash(&mut self, def: String, at: P) {
        self.aperture(def);
        let _ = writeln!(self.body, "{}D03*", Self::xy(at));
    }
    fn region(&mut self, pts: &[P]) {
        if pts.len() < 3 {
            return;
        }
        let _ = writeln!(self.body, "G36*");
        let _ = writeln!(self.body, "{}D02*", Self::xy(pts[0]));
        for q in &pts[1..] {
            let _ = writeln!(self.body, "{}D01*", Self::xy(*q));
        }
        let _ = writeln!(self.body, "{}D01*", Self::xy(pts[0]));
        let _ = writeln!(self.body, "G37*");
    }
    /// Region with holes: dark outer contour, then clear holes.
    fn shapes(&mut self, s: &poly::Shapes) {
        for shape in s {
            self.polarity(true);
            self.region(&shape[0].iter().map(|q| p(q[0], q[1])).collect::<Vec<_>>());
            if shape.len() > 1 {
                self.polarity(false);
                for h in &shape[1..] {
                    self.region(&h.iter().map(|q| p(q[0], q[1])).collect::<Vec<_>>());
                }
                self.polarity(true);
            }
        }
    }
    fn finish(self) -> String {
        let mut out = self.head;
        let mut ap: Vec<(&String, &usize)> = self.apertures.iter().collect();
        ap.sort_by_key(|(_, d)| **d);
        for (def, d) in ap {
            let _ = writeln!(out, "%ADD{}{}*%", d, def);
        }
        out.push_str(&self.body);
        out.push_str("M02*\n");
        out
    }
}

/// Flash or region for a pad grown by `grow`.
fn pad_out(g: &mut G, hull: &Hull, shape: PadShape, size: P, angle: f64, at: P, grow: f64) {
    let a = angle.rem_euclid(180.0);
    let right = a.abs() < 1e-6 || (a - 90.0).abs() < 1e-6;
    let (w, h) = if (a - 90.0).abs() < 1e-6 { (size.y, size.x) } else { (size.x, size.y) };
    let (w, h) = (w + 2.0 * grow, h + 2.0 * grow);
    match shape {
        PadShape::Circle => g.flash(format!("C,{:.6}", w.max(h)), at),
        PadShape::Rect if right => g.flash(format!("R,{:.6}X{:.6}", w, h), at),
        PadShape::Oval if right => g.flash(format!("O,{:.6}X{:.6}", w, h), at),
        _ => {
            let grown = Hull { pts: hull.pts.clone(), r: hull.r + grow };
            g.region(&grown.polygon(32));
        }
    }
}

/// One Gerber layer of the board.
pub fn layer(b: &Board, l: Layer, name: &str) -> String {
    let mut g = G::new(&l.function(b.layers), name);
    match l {
        Layer::Copper(cu) => {
            // zones first (their holes are drawn with clear polarity)
            for z in b.zones.iter().filter(|z| z.layer == cu) {
                g.shapes(&z.fill);
            }
            g.polarity(true);
            for t in b.tracks.iter().filter(|t| t.layer == cu) {
                g.line(t.a, t.b, t.width);
            }
            for v in &b.vias {
                g.flash(format!("C,{:.6}", v.dia), v.pos);
            }
            for f in &b.footprints {
                for (k, pad) in f.fp.pads.iter().enumerate() {
                    if f.pad_layers(k, b.layers) & (1 << cu) != 0 {
                        pad_out(&mut g, &f.pad_hull(k), pad.shape, pad.size, f.rot + pad.rot, f.pad_pos(k), 0.0);
                    }
                }
            }
        }
        Layer::Mask(top) | Layer::Paste(top) => {
            let mask = matches!(l, Layer::Mask(_));
            for f in &b.footprints {
                for (k, pad) in f.fp.pads.iter().enumerate() {
                    let on_side = if pad.smd() { f.bottom != top } else { mask };
                    if !on_side || (!mask && pad.drill > 0.0) {
                        continue;
                    }
                    if pad.npth && !mask {
                        continue;
                    }
                    pad_out(&mut g, &f.pad_hull(k), pad.shape, pad.size, f.rot + pad.rot, f.pad_pos(k), if mask { MASK_MARGIN } else { 0.0 });
                }
            }
        }
        Layer::Silk(top) => {
            for f in b.footprints.iter().filter(|f| f.bottom != top) {
                for (a, e) in &f.fp.silk {
                    g.line(f.xform(*a), f.xform(*e), SILK_WIDTH);
                }
                // reference above the courtyard
                let (lo, hi) = f.fp.court;
                let at = f.xform(p((lo.x + hi.x) / 2.0, lo.y - 0.4));
                text(&mut g, &f.reference, at, 1.0, !top);
            }
            if top {
                for (at, t, h) in &b.texts {
                    text(&mut g, t, *at, *h, false);
                }
            }
        }
        Layer::Edge => {
            for lp in &b.outline {
                for i in 0..lp.len() {
                    g.line(lp[i], lp[(i + 1) % lp.len()], 0.1);
                }
            }
        }
    }
    g.finish()
}

/// Text centred at `at` (board coordinates), `h` mm high, mirrored for the bottom side,
/// drawn with the stroke font like KiCad.
fn text(g: &mut G, t: &str, at: P, h: f64, mirror: bool) {
    let w = super::stroke::pen(h);
    for line in super::stroke::centered(t, at, h, 0.0, mirror) {
        if line.len() == 1 {
            g.line(line[0], line[0], w);
        }
        for s in line.windows(2) {
            g.line(s[0], s[1], w);
        }
    }
}

/// Drill files: (plated, non-plated), Excellon with metric decimal coordinates.
pub fn drill(b: &Board) -> (String, String) {
    let mut pth: Vec<(f64, P, P)> = b.vias.iter().map(|v| (v.drill, v.pos, v.pos)).collect();
    let mut npth: Vec<(f64, P, P)> = vec![];
    for f in &b.footprints {
        for pad in f.fp.pads.iter() {
            if pad.drill > 0.0 {
                let (a, e) = pad.hole_ends(f.pos, f.rot, f.bottom);
                if pad.npth {
                    npth.push((pad.drill, a, e));
                } else {
                    pth.push((pad.drill, a, e));
                }
            }
        }
    }
    (excellon(&pth, true), excellon(&npth, false))
}

/// Excellon with metric decimal coordinates; slots as routed G85 holes.
fn excellon(holes: &[(f64, P, P)], plated: bool) -> String {
    let mut tools: Vec<f64> = vec![];
    for (d, _, _) in holes {
        let d = (d * 1000.0).round() / 1000.0;
        if !tools.iter().any(|t| (t - d).abs() < 1e-9) {
            tools.push(d);
        }
    }
    tools.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut s = String::new();
    let _ = writeln!(s, "M48");
    let _ = writeln!(s, "; DRILL file {{Freo {}}}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(s, "; FORMAT={{-:-/ absolute / metric / decimal}}");
    let _ = writeln!(s, "; #@! TF.FileFunction,{},1,2,{}", if plated { "Plated" } else { "NonPlated" }, if plated { "PTH" } else { "NPTH" });
    let _ = writeln!(s, "FMAT,2");
    let _ = writeln!(s, "METRIC");
    for (i, t) in tools.iter().enumerate() {
        let _ = writeln!(s, "; #@! TA.AperFunction,{},{},{}", if plated { "Plated" } else { "NonPlated" }, if plated { "PTH" } else { "NPTH" }, if plated { "ComponentDrill" } else { "ComponentDrill" });
        let _ = writeln!(s, "T{}C{:.3}", i + 1, t);
    }
    let _ = writeln!(s, "%");
    let _ = writeln!(s, "G90");
    let _ = writeln!(s, "G05");
    for (i, t) in tools.iter().enumerate() {
        let mut first = true;
        for (d, q, e) in holes {
            if ((d * 1000.0).round() / 1000.0 - t).abs() < 1e-9 {
                if first {
                    let _ = writeln!(s, "T{}", i + 1);
                    first = false;
                }
                if (*q - *e).length() > 1e-6 {
                    let _ = writeln!(s, "X{}Y{}G85X{}Y{}", fmt3(q.x), fmt3(-q.y), fmt3(e.x), fmt3(-e.y));
                } else {
                    let _ = writeln!(s, "X{}Y{}", fmt3(q.x), fmt3(-q.y));
                }
            }
        }
    }
    let _ = writeln!(s, "M30");
    s
}

fn fmt3(v: f64) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" {
        "0".into()
    } else {
        s
    }
}

/// All fabrication files: (file name, contents).
pub fn fab_files(b: &Board, name: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Layer::all(b.layers).into_iter().map(|l| (format!("{}{}", name, l.file_suffix(b.layers)), layer(b, l, name))).collect();
    let (pth, npth) = drill(b);
    out.push((format!("{}-PTH.drl", name), pth));
    out.push((format!("{}-NPTH.drl", name), npth));
    out.push((format!("{}-job.gbrjob", name), super::fab::job_file(b, name)));
    let _ = layer_name;
    out
}

/// Minimal Gerber reader for checks: apertures, flashes and drawn segments, in mm (y up).
#[derive(Debug, Default)]
pub struct Parsed {
    pub apertures: BTreeMap<usize, String>,
    pub flashes: Vec<(usize, P)>,
    pub draws: Vec<(usize, P, P)>,
    pub regions: usize,
    pub ended: bool,
}

pub fn parse(s: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut cur = 0;
    let mut at = P::ZERO;
    let num = |t: &str, k: char| -> Option<f64> {
        let i = t.find(k)? + 1;
        let j = t[i..].find(|c: char| !(c.is_ascii_digit() || c == '-')).map(|j| i + j).unwrap_or(t.len());
        t[i..j].parse::<i64>().ok().map(|v| v as f64 / 1e6)
    };
    let mut in_region = false;
    for raw in s.lines() {
        let t = raw.trim();
        if let Some(rest) = t.strip_prefix("%ADD") {
            let k = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
            if let Ok(d) = rest[..k].parse() {
                out.apertures.insert(d, rest[k..].trim_end_matches("*%").to_string());
            }
            continue;
        }
        if t.starts_with('%') || t.starts_with("G04") {
            continue;
        }
        if t == "G36*" {
            in_region = true;
            out.regions += 1;
            continue;
        }
        if t == "G37*" {
            in_region = false;
            continue;
        }
        if t == "M02*" {
            out.ended = true;
            continue;
        }
        if t.starts_with('D') && !t.contains('X') {
            cur = t[1..].trim_end_matches('*').parse().unwrap_or(0);
            continue;
        }
        let x = num(t, 'X').unwrap_or(at.x);
        let y = num(t, 'Y').unwrap_or(at.y);
        let q = p(x, y);
        if t.ends_with("D03*") {
            out.flashes.push((cur, q));
        } else if t.ends_with("D01*") && !in_region {
            out.draws.push((cur, at, q));
        }
        at = q;
    }
    out
}
