//! From a 2D technical drawing (DXF; DWG through a converter) to a 3D part.
//!
//! The drawing is read completely (layers, line types, blocks, polylines with arc segments,
//! units, dimensions, texts). Visible lines form a planar graph; every separate outline is a
//! candidate view, the sheet frame and the title block are dropped. Views are matched by their
//! alignment (front and top share their width, front and side their height); where the top
//! view lies tells first-angle (European) from third-angle (American) projection. The scale
//! comes from dimensions (measured value against drawn length), dimension texts or the "1:2"
//! in the title block.
//!
//! The part is built from ordinary, editable Freo features: the top view's outline extruded
//! over the height, everything outside the front and side views' outlines cut away, and every
//! hole or pocket outline inside a view cut over the depth its hidden lines show in the
//! neighbouring view (through, when no hidden lines are drawn). A single view is extruded
//! with a thickness (from a "t=5" note, or as given).

use super::features::*;
use crate::kernel::math::*;
use crate::sketch::sketch::Sketch;
use std::collections::HashMap;

// ------------------------------------------------------------------------------------------
// drawing primitives

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Visible,
    Hidden,
    Center,
    /// dimension, text, hatch, frame and title layers
    Annotation,
}

#[derive(Clone, Debug)]
pub enum Shape2 {
    Line(V2, V2),
    /// counter-clockwise from a0 to a1 (radians)
    Arc { c: V2, r: f64, a0: f64, a1: f64 },
    Circle { c: V2, r: f64 },
}

#[derive(Clone, Debug)]
pub struct Ent {
    pub shape: Shape2,
    pub kind: LineKind,
    pub layer: String,
}

#[derive(Clone, Debug)]
pub struct Text2 {
    pub pos: V2,
    pub h: f64,
    pub angle: f64,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Drawing2 {
    pub ents: Vec<Ent>,
    pub texts: Vec<Text2>,
    /// (value written on a dimension, its drawn length)
    pub dims: Vec<(f64, f64)>,
    /// filled triangles (dimension arrows)
    pub solids: Vec<[V2; 4]>,
    /// millimetres per drawing unit ($INSUNITS)
    pub unit_mm: f64,
}

// ------------------------------------------------------------------------------------------
// DXF

struct Pairs<'a> {
    p: Vec<(i32, &'a str)>,
}

fn num(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0)
}

/// 2D affine map for block references.
#[derive(Clone, Copy)]
struct Xf {
    m: [f64; 4],
    t: V2,
}

impl Xf {
    const ID: Xf = Xf { m: [1.0, 0.0, 0.0, 1.0], t: V2::ZERO };
    fn ap(&self, p: V2) -> V2 {
        V2::new(self.m[0] * p.x + self.m[1] * p.y + self.t.x, self.m[2] * p.x + self.m[3] * p.y + self.t.y)
    }
    fn then(&self, o: &Xf) -> Xf {
        // self after o
        let a = self.m;
        let b = o.m;
        Xf { m: [a[0] * b[0] + a[1] * b[2], a[0] * b[1] + a[1] * b[3], a[2] * b[0] + a[3] * b[2], a[2] * b[1] + a[3] * b[3]], t: self.ap(o.t) }
    }
    fn det(&self) -> f64 {
        self.m[0] * self.m[3] - self.m[1] * self.m[2]
    }
    /// uniform scale and rotation (arcs stay arcs)
    fn similar(&self) -> Option<(f64, f64, bool)> {
        let d = self.det();
        let sx = (self.m[0] * self.m[0] + self.m[2] * self.m[2]).sqrt();
        let sy = (self.m[1] * self.m[1] + self.m[3] * self.m[3]).sqrt();
        if (sx - sy).abs() > 1e-6 * sx.max(1e-9) || (self.m[0] * self.m[1] + self.m[2] * self.m[3]).abs() > 1e-6 * sx * sy {
            return None;
        }
        Some((sx, self.m[2].atan2(self.m[0]), d < 0.0))
    }
}

/// Reads a DXF file (ASCII) into drawing primitives.
pub fn read_dxf(text: &str) -> Result<Drawing2, String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 4 {
        return Err("Die Datei ist keine DXF-Datei.".into());
    }
    let mut p = vec![];
    let mut i = 0;
    while i + 1 < lines.len() {
        if let Ok(code) = lines[i].trim().parse::<i32>() {
            p.push((code, lines[i + 1].trim_end_matches('\r')));
            i += 2;
        } else {
            i += 1;
        }
    }
    let pairs = Pairs { p };
    if !pairs.p.iter().any(|x| x.0 == 0 && x.1.trim() == "SECTION") {
        return Err("Die Datei ist keine DXF-Datei (keine Abschnitte).".into());
    }
    // ---- header and tables
    let mut unit_mm = 1.0;
    let mut layer_lt: HashMap<String, String> = HashMap::new();
    let mut k = 0;
    while k < pairs.p.len() {
        let (c, v) = pairs.p[k];
        if c == 9 && v.trim() == "$INSUNITS" {
            if let Some(&(70, u)) = pairs.p.get(k + 1) {
                unit_mm = match u.trim() {
                    "1" => 25.4,
                    "2" => 304.8,
                    "4" => 1.0,
                    "5" => 10.0,
                    "6" => 1000.0,
                    "8" => 0.0000254,
                    "9" => 0.0254,
                    "10" => 914.4,
                    "13" => 0.001,
                    "14" => 100.0,
                    _ => 1.0,
                };
            }
        }
        if c == 0 && v.trim() == "LAYER" {
            let mut name = String::new();
            let mut lt = String::new();
            let mut j = k + 1;
            while j < pairs.p.len() && pairs.p[j].0 != 0 {
                match pairs.p[j].0 {
                    2 => name = pairs.p[j].1.trim().to_string(),
                    6 => lt = pairs.p[j].1.trim().to_string(),
                    _ => {}
                }
                j += 1;
            }
            if !name.is_empty() {
                layer_lt.insert(name.to_uppercase(), lt.to_uppercase());
            }
        }
        k += 1;
    }
    // ---- records of the BLOCKS and ENTITIES sections
    let mut blocks: HashMap<String, (V2, Vec<(String, Vec<(i32, String)>)>)> = HashMap::new();
    let mut entities: Vec<(String, Vec<(i32, String)>)> = vec![];
    let mut section = String::new();
    let mut k = 0;
    let mut cur_block: Option<(String, V2, Vec<(String, Vec<(i32, String)>)>)> = None;
    while k < pairs.p.len() {
        let (c, v) = pairs.p[k];
        if c != 0 {
            k += 1;
            continue;
        }
        let kind = v.trim().to_uppercase();
        let mut j = k + 1;
        let mut rec: Vec<(i32, String)> = vec![];
        while j < pairs.p.len() && pairs.p[j].0 != 0 {
            rec.push((pairs.p[j].0, pairs.p[j].1.to_string()));
            j += 1;
        }
        match kind.as_str() {
            "SECTION" => section = rec.iter().find(|x| x.0 == 2).map(|x| x.1.trim().to_uppercase()).unwrap_or_default(),
            "ENDSEC" => section.clear(),
            "BLOCK" if section == "BLOCKS" => {
                let name = rec.iter().find(|x| x.0 == 2).map(|x| x.1.trim().to_string()).unwrap_or_default();
                let g = |code: i32| rec.iter().find(|x| x.0 == code).map(|x| num(&x.1)).unwrap_or(0.0);
                cur_block = Some((name, V2::new(g(10), g(20)), vec![]));
            }
            "ENDBLK" => {
                if let Some((n, base, ents)) = cur_block.take() {
                    blocks.insert(n.to_uppercase(), (base, ents));
                }
            }
            _ => {
                if section == "BLOCKS" {
                    if let Some(b) = &mut cur_block {
                        b.2.push((kind, rec));
                    }
                } else if section == "ENTITIES" {
                    entities.push((kind, rec));
                }
            }
        }
        k = j;
    }
    // model space first; paper space only when the model space is empty
    let model: Vec<_> = entities.iter().filter(|(_, r)| !r.iter().any(|x| x.0 == 67 && x.1.trim() == "1")).cloned().collect();
    let used = if model.iter().any(|(k, _)| matches!(k.as_str(), "LINE" | "LWPOLYLINE" | "POLYLINE" | "CIRCLE" | "ARC" | "INSERT")) { model } else { entities };
    let mut d = Drawing2 { unit_mm, ..Default::default() };
    emit(&used, &Xf::ID, "0", &blocks, &layer_lt, &mut d, 0);
    if d.ents.is_empty() {
        return Err("Die Zeichnung enthält keine Linien, Kreise oder Bögen.".into());
    }
    Ok(d)
}

fn line_kind(layer: &str, ltype: &str, layer_lt: &HashMap<String, String>) -> LineKind {
    let l = layer.to_uppercase();
    let mut lt = ltype.to_uppercase();
    if lt.is_empty() || lt == "BYLAYER" {
        lt = layer_lt.get(&l).cloned().unwrap_or_default();
    }
    let has = |s: &str, keys: &[&str]| keys.iter().any(|k| s.contains(k));
    if has(&l, &["DIM", "BEM", "MASS", "MAß", "TEXT", "SCHRIFT", "ANNO", "THIN", "HATCH", "SCHRAFF", "TITLE", "SCHRIFTFELD", "RAHMEN", "FRAME", "BORDER", "DEFPOINTS", "VIEWPORT"]) {
        return LineKind::Annotation;
    }
    if has(&lt, &["CENTER", "CENTRE", "DASHDOT", "STRICHPUNKT", "PHANTOM", "MITTE", "DIVIDE", "ISO10W", "ISO04W", "ISO08W"])
        || has(&l, &["CENTER", "CENTRE", "MITTE", "ACHS"])
    {
        return LineKind::Center;
    }
    if has(&lt, &["HIDDEN", "DASHED", "VERDECKT", "STRICH", "ISO02W", "ISO03W"]) || has(&l, &["HIDDEN", "VERDECKT", "UNSICHTBAR"]) {
        return LineKind::Hidden;
    }
    LineKind::Visible
}

#[allow(clippy::too_many_arguments)]
fn emit(
    recs: &[(String, Vec<(i32, String)>)],
    xf: &Xf,
    parent_layer: &str,
    blocks: &HashMap<String, (V2, Vec<(String, Vec<(i32, String)>)>)>,
    layer_lt: &HashMap<String, String>,
    d: &mut Drawing2,
    depth: usize,
) {
    let mut poly: Option<(String, String, Vec<(V2, f64)>, bool, bool)> = None;
    for (kind, rec) in recs {
        let g = |code: i32| rec.iter().find(|x| x.0 == code).map(|x| num(&x.1)).unwrap_or(0.0);
        let gs = |code: i32| rec.iter().find(|x| x.0 == code).map(|x| x.1.trim().to_string()).unwrap_or_default();
        let mut layer = gs(8);
        if layer.is_empty() || layer == "0" {
            layer = parent_layer.to_string();
        }
        let lk = line_kind(&layer, &gs(6), layer_lt);
        // OCS mirrored by a (0, 0, -1) extrusion direction
        let mirror = rec.iter().any(|x| x.0 == 230) && g(230) < 0.0;
        let o = |p: V2| if mirror { V2::new(-p.x, p.y) } else { p };
        let push = |d: &mut Drawing2, shape: Shape2| d.ents.push(Ent { shape, kind: lk, layer: layer.clone() });
        match kind.as_str() {
            "LINE" => push(d, map_shape(Shape2::Line(V2::new(g(10), g(20)), V2::new(g(11), g(21))), xf)),
            "CIRCLE" => {
                let c = o(V2::new(g(10), g(20)));
                push(d, map_shape(Shape2::Circle { c, r: g(40) }, xf));
            }
            "ARC" => {
                let c = o(V2::new(g(10), g(20)));
                let (mut a0, mut a1) = (g(50).to_radians(), g(51).to_radians());
                if mirror {
                    let (b0, b1) = (std::f64::consts::PI - a1, std::f64::consts::PI - a0);
                    a0 = b0;
                    a1 = b1;
                }
                push(d, map_shape(Shape2::Arc { c, r: g(40), a0, a1 }, xf));
            }
            "LWPOLYLINE" => {
                let closed = (g(70) as i64) & 1 == 1;
                let mut verts: Vec<(V2, f64)> = vec![];
                for (code, v) in rec {
                    match code {
                        10 => verts.push((V2::new(num(v), 0.0), 0.0)),
                        20 => {
                            if let Some(l) = verts.last_mut() {
                                l.0.y = num(v)
                            }
                        }
                        42 => {
                            if let Some(l) = verts.last_mut() {
                                l.1 = num(v)
                            }
                        }
                        _ => {}
                    }
                }
                let verts: Vec<(V2, f64)> = verts.into_iter().map(|(p, b)| (o(p), if mirror { -b } else { b })).collect();
                for s in bulge_shapes(&verts, closed) {
                    push(d, map_shape(s, xf));
                }
            }
            "POLYLINE" => {
                poly = Some((layer.clone(), gs(6), vec![], (g(70) as i64) & 1 == 1, mirror));
            }
            "VERTEX" => {
                if let Some(pl) = &mut poly {
                    pl.2.push((V2::new(g(10), g(20)), g(42)));
                }
            }
            "SEQEND" => {
                if let Some((ly, lt, verts, closed, mir)) = poly.take() {
                    let k2 = line_kind(&ly, &lt, layer_lt);
                    let verts: Vec<(V2, f64)> = verts.into_iter().map(|(p, b)| if mir { (V2::new(-p.x, p.y), -b) } else { (p, b) }).collect();
                    for s in bulge_shapes(&verts, closed) {
                        d.ents.push(Ent { shape: map_shape(s, xf), kind: k2, layer: ly.clone() });
                    }
                }
            }
            "ELLIPSE" => {
                let c = V2::new(g(10), g(20));
                let a = V2::new(g(11), g(21));
                let ratio = g(40);
                let (t0, mut t1) = (g(41), g(42));
                if t1 <= t0 {
                    t1 += std::f64::consts::TAU;
                }
                let b = V2::new(-a.y, a.x) * ratio;
                let n = 72;
                let pts: Vec<V2> = (0..=n).map(|i| {
                    let t = t0 + (t1 - t0) * i as f64 / n as f64;
                    o(c + a * t.cos() + b * t.sin())
                }).collect();
                for w in pts.windows(2) {
                    push(d, map_shape(Shape2::Line(w[0], w[1]), xf));
                }
            }
            "SPLINE" => {
                let xs: Vec<f64> = rec.iter().filter(|x| x.0 == 11).map(|x| num(&x.1)).collect();
                let ys: Vec<f64> = rec.iter().filter(|x| x.0 == 21).map(|x| num(&x.1)).collect();
                let (xs, ys) = if xs.len() >= 2 { (xs, ys) } else { (rec.iter().filter(|x| x.0 == 10).map(|x| num(&x.1)).collect(), rec.iter().filter(|x| x.0 == 20).map(|x| num(&x.1)).collect()) };
                let pts: Vec<V2> = xs.iter().zip(&ys).map(|(x, y)| o(V2::new(*x, *y))).collect();
                for w in pts.windows(2) {
                    push(d, map_shape(Shape2::Line(w[0], w[1]), xf));
                }
            }
            "SOLID" | "TRACE" => {
                let q = [V2::new(g(10), g(20)), V2::new(g(11), g(21)), V2::new(g(12), g(22)), V2::new(g(13), g(23))];
                d.solids.push([xf.ap(q[0]), xf.ap(q[1]), xf.ap(q[2]), xf.ap(q[3])]);
            }
            "TEXT" | "MTEXT" | "ATTRIB" => {
                let mut t: String = rec.iter().filter(|x| x.0 == 3).map(|x| x.1.clone()).collect();
                t.push_str(&gs(1));
                let pos = xf.ap(o(V2::new(g(10), g(20))));
                let h = g(40) * xf.similar().map(|s| s.0).unwrap_or(1.0);
                d.texts.push(Text2 { pos, h, angle: g(50).to_radians(), text: clean_text(&t) });
            }
            "DIMENSION" => {
                // linear and aligned dimensions: written value against drawn length
                let ty = (g(70) as i64) & 7;
                let measured = g(42);
                let (p1, p2) = (xf.ap(V2::new(g(13), g(23))), xf.ap(V2::new(g(14), g(24))));
                let drawn = match ty {
                    0 => {
                        let a = g(50).to_radians() + xf.similar().map(|s| s.1).unwrap_or(0.0);
                        ((p2 - p1).dot(V2::new(a.cos(), a.sin()))).abs()
                    }
                    1 => (p2 - p1).length(),
                    _ => 0.0,
                };
                // a value written over the measurement replaces it
                let over = gs(1);
                let value = parse_number(&clean_text(&over)).filter(|_| !over.contains("<>")).unwrap_or(measured);
                if drawn > 1e-9 && value > 1e-9 {
                    d.dims.push((value, drawn));
                }
            }
            "INSERT" if depth < 8 => {
                let name = gs(2).to_uppercase();
                if let Some((base, sub)) = blocks.get(&name) {
                    let (sx, sy) = (if rec.iter().any(|x| x.0 == 41) { g(41) } else { 1.0 }, if rec.iter().any(|x| x.0 == 42) { g(42) } else { 1.0 });
                    let rot = g(50).to_radians();
                    let ins = o(V2::new(g(10), g(20)));
                    let (c, s) = (rot.cos(), rot.sin());
                    let mut m = [c * sx, -s * sy, s * sx, c * sy];
                    if mirror {
                        m = [-m[0], -m[1], m[2], m[3]];
                    }
                    let local = Xf { m, t: ins - V2::new(m[0] * base.x + m[1] * base.y, m[2] * base.x + m[3] * base.y) };
                    // columns/rows of a MINSERT are rare in drawings: the first instance only
                    emit(sub, &xf.then(&local), &layer, blocks, layer_lt, d, depth + 1);
                }
            }
            _ => {}
        }
    }
}

fn map_shape(s: Shape2, xf: &Xf) -> Shape2 {
    match s {
        Shape2::Line(a, b) => Shape2::Line(xf.ap(a), xf.ap(b)),
        Shape2::Circle { c, r } => match xf.similar() {
            Some((k, _, _)) => Shape2::Circle { c: xf.ap(c), r: r * k },
            None => Shape2::Circle { c: xf.ap(c), r: r * xf.det().abs().sqrt() },
        },
        Shape2::Arc { c, r, a0, a1 } => match xf.similar() {
            Some((k, rot, mirrored)) => {
                if mirrored {
                    // a mirrored arc runs the other way: swap and reflect the angles
                    let p0 = xf.ap(c + V2::new(a1.cos(), a1.sin()) * r);
                    let p1 = xf.ap(c + V2::new(a0.cos(), a0.sin()) * r);
                    let cc = xf.ap(c);
                    Shape2::Arc { c: cc, r: r * k, a0: (p0 - cc).y.atan2((p0 - cc).x), a1: (p1 - cc).y.atan2((p1 - cc).x) }
                } else {
                    Shape2::Arc { c: xf.ap(c), r: r * k, a0: a0 + rot, a1: a1 + rot }
                }
            }
            None => Shape2::Arc { c: xf.ap(c), r: r * xf.det().abs().sqrt(), a0, a1 },
        },
    }
}

/// Polyline vertices with bulges (tan of a quarter of the arc angle) as lines and arcs.
fn bulge_shapes(v: &[(V2, f64)], closed: bool) -> Vec<Shape2> {
    let n = v.len();
    let mut out = vec![];
    let m = if closed { n } else { n.saturating_sub(1) };
    for i in 0..m {
        let (a, bulge) = v[i];
        let b = v[(i + 1) % n].0;
        if (b - a).length() < 1e-12 {
            continue;
        }
        if bulge.abs() < 1e-9 {
            out.push(Shape2::Line(a, b));
            continue;
        }
        let theta = 4.0 * bulge.atan();
        let chord = b - a;
        let l = chord.length();
        let r = l / (2.0 * (theta / 2.0).sin()).abs();
        let mid = (a + b) * 0.5;
        let nrm = V2::new(-chord.y, chord.x) / l;
        // distance from the chord's middle to the centre, on the arc's inner side
        let h = (r * r - l * l / 4.0).max(0.0).sqrt();
        let side = if (theta.abs() > std::f64::consts::PI) == (bulge > 0.0) { -1.0 } else { 1.0 };
        let c = mid + nrm * h * side;
        let (pa, pb) = ((a - c).y.atan2((a - c).x), (b - c).y.atan2((b - c).x));
        out.push(if bulge > 0.0 { Shape2::Arc { c, r, a0: pa, a1: pb } } else { Shape2::Arc { c, r, a0: pb, a1: pa } });
    }
    out
}

/// DXF text with control codes and formatting removed (%%c → ⌀, MTEXT \P, {\f..;}).
fn clean_text(t: &str) -> String {
    let mut s = t.replace("%%c", "⌀").replace("%%C", "⌀").replace("%%d", "°").replace("%%D", "°").replace("%%p", "±").replace("%%P", "±").replace("\\P", " ");
    // MTEXT formatting groups like {\fArial|b0;text} and \A1;
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '\\' => {
                // \U+00D8 style escapes
                if chars.peek() == Some(&'U') {
                    let rest: String = chars.clone().take(6).collect();
                    if rest.starts_with("U+") {
                        if let Ok(v) = u32::from_str_radix(&rest[2..6], 16) {
                            if let Some(ch) = char::from_u32(v) {
                                out.push(ch);
                                for _ in 0..6 {
                                    chars.next();
                                }
                                continue;
                            }
                        }
                    }
                }
                // skip a formatting code up to ';'
                for d in chars.by_ref() {
                    if d == ';' {
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    s = out;
    s.trim().to_string()
}

/// The number in a dimension text ("⌀13.00", "R5", "25,5", "2x 45°" → 45 not used).
fn parse_number(t: &str) -> Option<f64> {
    let t = t.trim().trim_start_matches(['⌀', 'Ø', 'R', 'r', 'M']).trim();
    let num: String = t.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect();
    if num.is_empty() || num.len() < t.split_whitespace().next().unwrap_or("").chars().take_while(|c| !c.is_whitespace() && *c != '±' && *c != '°').count() {
        return None;
    }
    num.replace(',', ".").parse::<f64>().ok()
}

// ------------------------------------------------------------------------------------------
// planar graph of the visible lines

#[derive(Clone, Copy, Debug)]
pub struct ArcSrc {
    pub c: V2,
    pub r: f64,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub a: usize,
    pub b: usize,
    pub arc: Option<u32>,
}

pub struct Graph {
    pub pts: Vec<V2>,
    pub edges: Vec<Edge>,
    pub arcs: Vec<ArcSrc>,
}

/// One closed outline: vertices in order (last joins the first), with the arc each step lies on.
#[derive(Clone, Debug)]
pub struct Loop2 {
    pub pts: Vec<V2>,
    pub arc: Vec<Option<ArcSrc>>,
}

impl Loop2 {
    pub fn area(&self) -> f64 {
        let n = self.pts.len();
        (0..n).map(|i| self.pts[i].x * self.pts[(i + 1) % n].y - self.pts[(i + 1) % n].x * self.pts[i].y).sum::<f64>() * 0.5
    }
    pub fn bbox(&self) -> (V2, V2) {
        let mut lo = V2::splat(f64::INFINITY);
        let mut hi = V2::splat(f64::NEG_INFINITY);
        for p in &self.pts {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        (lo, hi)
    }
    pub fn contains(&self, q: V2) -> bool {
        let n = self.pts.len();
        let mut inside = false;
        let mut j = n - 1;
        for i in 0..n {
            let (a, b) = (self.pts[i], self.pts[j]);
            if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
                inside = !inside;
            }
            j = i;
        }
        inside
    }
    /// How far the outline reaches above / below at x (vertical extent of the region at x).
    pub fn span_at_x(&self, x: f64) -> Option<(f64, f64)> {
        let n = self.pts.len();
        let mut ys = vec![];
        for i in 0..n {
            let (a, b) = (self.pts[i], self.pts[(i + 1) % n]);
            if (a.x - x) * (b.x - x) <= 0.0 && (a.x - b.x).abs() > 1e-12 {
                ys.push(a.y + (b.y - a.y) * (x - a.x) / (b.x - a.x));
            }
        }
        if ys.is_empty() {
            return None;
        }
        Some((ys.iter().cloned().fold(f64::INFINITY, f64::min), ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
    }
}

/// Segments of a shape: lines as they are, arcs and circles in steps of at most 3°.
fn shape_segments(s: &Shape2, arcs: &mut Vec<ArcSrc>) -> Vec<(V2, V2, Option<u32>)> {
    match *s {
        Shape2::Line(a, b) => vec![(a, b, None)],
        Shape2::Circle { c, r } => {
            arcs.push(ArcSrc { c, r });
            let id = (arcs.len() - 1) as u32;
            let n = 120;
            (0..n).map(|i| {
                let (t0, t1) = (std::f64::consts::TAU * i as f64 / n as f64, std::f64::consts::TAU * (i + 1) as f64 / n as f64);
                (c + V2::new(t0.cos(), t0.sin()) * r, c + V2::new(t1.cos(), t1.sin()) * r, Some(id))
            }).collect()
        }
        Shape2::Arc { c, r, a0, a1 } => {
            arcs.push(ArcSrc { c, r });
            let id = (arcs.len() - 1) as u32;
            let mut sweep = a1 - a0;
            while sweep <= 1e-12 {
                sweep += std::f64::consts::TAU;
            }
            let n = ((sweep / 3f64.to_radians()).ceil() as usize).max(1);
            (0..n).map(|i| {
                let (t0, t1) = (a0 + sweep * i as f64 / n as f64, a0 + sweep * (i + 1) as f64 / n as f64);
                (c + V2::new(t0.cos(), t0.sin()) * r, c + V2::new(t1.cos(), t1.sin()) * r, Some(id))
            }).collect()
        }
    }
}

/// Builds the noded planar graph: lines that cross or end on each other are split there.
pub fn build_graph(shapes: &[&Shape2], tol: f64) -> Graph {
    let mut arcs = vec![];
    let mut segs: Vec<(V2, V2, Option<u32>)> = shapes.iter().flat_map(|s| shape_segments(s, &mut arcs)).filter(|s| (s.1 - s.0).length() > tol * 0.5).collect();
    // split points per segment (parameters)
    let n = segs.len();
    let mut cuts: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; n];
    if n > 0 {
        let (mut lo, mut hi) = (V2::splat(f64::INFINITY), V2::splat(f64::NEG_INFINITY));
        for s in &segs {
            lo = lo.min(s.0.min(s.1));
            hi = hi.max(s.0.max(s.1));
        }
        let cell = ((hi - lo).max_element() / 256.0).max(tol * 4.0);
        let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, s) in segs.iter().enumerate() {
            let (a, b) = (s.0.min(s.1) - V2::splat(tol), s.0.max(s.1) + V2::splat(tol));
            for gx in ((a.x - lo.x) / cell).floor() as i64..=((b.x - lo.x) / cell).floor() as i64 {
                for gy in ((a.y - lo.y) / cell).floor() as i64..=((b.y - lo.y) / cell).floor() as i64 {
                    grid.entry((gx, gy)).or_default().push(i);
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        for list in grid.values() {
            for (x, &i) in list.iter().enumerate() {
                for &j in &list[x + 1..] {
                    if !seen.insert((i.min(j), i.max(j))) {
                        continue;
                    }
                    let (p, r) = (segs[i].0, segs[i].1 - segs[i].0);
                    let (q, s) = (segs[j].0, segs[j].1 - segs[j].0);
                    let rxs = r.x * s.y - r.y * s.x;
                    let (lr, ls) = (r.length(), s.length());
                    if rxs.abs() > 1e-12 * lr * ls {
                        let t = ((q - p).x * s.y - (q - p).y * s.x) / rxs;
                        let u = ((q - p).x * r.y - (q - p).y * r.x) / rxs;
                        let (et, eu) = (tol / lr, tol / ls);
                        if t > -et && t < 1.0 + et && u > -eu && u < 1.0 + eu {
                            cuts[i].push(t.clamp(0.0, 1.0));
                            cuts[j].push(u.clamp(0.0, 1.0));
                        }
                    }
                    // end points lying on the other segment (T joints, overlaps)
                    for (a_seg, b_seg, cut_into) in [(i, j, j), (j, i, i)] {
                        let (o, d) = (segs[b_seg].0, segs[b_seg].1 - segs[b_seg].0);
                        let l2 = d.dot(d);
                        for e in [segs[a_seg].0, segs[a_seg].1] {
                            let t = (e - o).dot(d) / l2;
                            if t > 0.0 && t < 1.0 && (o + d * t - e).length() < tol {
                                cuts[cut_into].push(t);
                            }
                        }
                    }
                }
            }
        }
    }
    // vertices merged within tol
    let mut pts: Vec<V2> = vec![];
    let mut vgrid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let mut vid = |p: V2, pts: &mut Vec<V2>| -> usize {
        let k = ((p.x / tol).round() as i64, (p.y / tol).round() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(l) = vgrid.get(&(k.0 + dx, k.1 + dy)) {
                    for &v in l {
                        if (pts[v] - p).length() < tol {
                            return v;
                        }
                    }
                }
            }
        }
        pts.push(p);
        vgrid.entry(k).or_default().push(pts.len() - 1);
        pts.len() - 1
    };
    let mut edges: Vec<Edge> = vec![];
    let mut have = std::collections::HashSet::new();
    for (i, s) in segs.iter_mut().enumerate() {
        let c = &mut cuts[i];
        c.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let ids: Vec<usize> = c.iter().map(|t| vid(s.0 + (s.1 - s.0) * *t, &mut pts)).collect();
        for w in ids.windows(2) {
            if w[0] != w[1] && have.insert((w[0].min(w[1]), w[0].max(w[1]))) {
                edges.push(Edge { a: w[0], b: w[1], arc: s.2 });
            }
        }
    }
    Graph { pts, edges, arcs }
}

/// Connected components of the graph with their outer outline and inner faces.
pub struct Component {
    pub outer: Loop2,
    pub faces: Vec<Loop2>,
    pub edges: usize,
}

pub fn components(g: &Graph) -> Vec<Component> {
    let n = g.pts.len();
    // union-find over vertices
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        let mut y = x;
        while p[y] != r {
            let nx = p[y];
            p[y] = r;
            y = nx;
        }
        r
    }
    for e in &g.edges {
        let (a, b) = (find(&mut parent, e.a), find(&mut parent, e.b));
        if a != b {
            parent[a] = b;
        }
    }
    // half-edges: 2k = a→b, 2k+1 = b→a; around each vertex sorted by angle
    let he_from = |h: usize| if h % 2 == 0 { g.edges[h / 2].a } else { g.edges[h / 2].b };
    let he_to = |h: usize| if h % 2 == 0 { g.edges[h / 2].b } else { g.edges[h / 2].a };
    let mut out_of: Vec<Vec<usize>> = vec![vec![]; n];
    for h in 0..g.edges.len() * 2 {
        out_of[he_from(h)].push(h);
    }
    let ang = |h: usize| {
        let d = g.pts[he_to(h)] - g.pts[he_from(h)];
        d.y.atan2(d.x)
    };
    for l in &mut out_of {
        l.sort_by(|a, b| ang(*a).partial_cmp(&ang(*b)).unwrap());
    }
    let mut pos_in = vec![0usize; g.edges.len() * 2];
    for l in &out_of {
        for (i, &h) in l.iter().enumerate() {
            pos_in[h] = i;
        }
    }
    // next half-edge of a face: at the end vertex, the edge just clockwise of the way back
    let next = |h: usize| {
        let twin = h ^ 1;
        let v = he_to(h);
        let l = &out_of[v];
        let i = pos_in[twin];
        l[(i + l.len() - 1) % l.len()]
    };
    let mut used = vec![false; g.edges.len() * 2];
    let mut by_comp: HashMap<usize, Vec<Loop2>> = HashMap::new();
    let mut edge_count: HashMap<usize, usize> = HashMap::new();
    for e in &g.edges {
        *edge_count.entry(find(&mut parent, e.a)).or_default() += 1;
    }
    for h0 in 0..g.edges.len() * 2 {
        if used[h0] {
            continue;
        }
        let mut lp = Loop2 { pts: vec![], arc: vec![] };
        let mut h = h0;
        let mut guard = 0;
        loop {
            used[h] = true;
            lp.pts.push(g.pts[he_from(h)]);
            lp.arc.push(g.edges[h / 2].arc.map(|i| g.arcs[i as usize]));
            h = next(h);
            guard += 1;
            if h == h0 || guard > 1_000_000 {
                break;
            }
        }
        let c = find(&mut parent, he_from(h0));
        by_comp.entry(c).or_default().push(lp);
    }
    let mut out = vec![];
    for (c, loops) in by_comp {
        // the outer face goes clockwise (negative area) and is the largest in size
        let Some(oi) = (0..loops.len()).min_by(|&a, &b| loops[a].area().partial_cmp(&loops[b].area()).unwrap()) else { continue };
        let mut outer = loops[oi].clone();
        remove_spikes(&mut outer);
        if outer.pts.len() < 3 || outer.area().abs() < 1e-12 {
            continue;
        }
        // as a counter-clockwise outline
        outer.pts.reverse();
        outer.arc.reverse();
        // after reversing, step i runs from pts[i] to pts[i+1], which was step n−2−i before:
        // the reversed tags move one place forward
        outer.arc.rotate_left(1);
        let faces = loops.into_iter().enumerate().filter(|(i, l)| *i != oi && l.area() > 1e-12).map(|(_, l)| l).collect();
        out.push(Component { outer, faces, edges: edge_count.get(&c).copied().unwrap_or(0) });
    }
    out
}


/// Circle through points (least squares for the centre); the radius is the largest distance,
/// as tessellated circles have their corners on the true circle and their chords inside it.
fn fit_circle(pts: &[V2]) -> Option<(V2, f64)> {
    let n = pts.len() as f64;
    if n < 3.0 {
        return None;
    }
    let m = pts.iter().fold(V2::ZERO, |a, p| a + *p) / n;
    let (mut suu, mut svv, mut suv, mut suuu, mut svvv, mut suvv, mut svuu) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for p in pts {
        let (u, v) = (p.x - m.x, p.y - m.y);
        suu += u * u;
        svv += v * v;
        suv += u * v;
        suuu += u * u * u;
        svvv += v * v * v;
        suvv += u * v * v;
        svuu += v * u * u;
    }
    let det = suu * svv - suv * suv;
    if det.abs() < 1e-18 {
        return None;
    }
    let (b1, b2) = (0.5 * (suuu + suvv), 0.5 * (svvv + svuu));
    let uc = (b1 * svv - b2 * suv) / det;
    let vc = (suu * b2 - suv * b1) / det;
    let c = m + V2::new(uc, vc);
    let r = pts.iter().map(|p| (*p - c).length()).fold(0.0, f64::max);
    Some((c, r))
}

/// Drops points that lie on the straight line between their neighbours (edges cut into
/// pieces by the hidden-line removal), on steps not already known to be arcs.
fn merge_collinear(l: &Loop2, tol: f64) -> Loop2 {
    let mut pts = l.pts.clone();
    let mut arc = l.arc.clone();
    let mut changed = true;
    while changed && pts.len() > 3 {
        changed = false;
        let n = pts.len();
        let mut q = 0;
        while q < pts.len() && pts.len() > 3 {
            let n2 = pts.len();
            let prev = (q + n2 - 1) % n2;
            if arc[q].is_none() && arc[prev].is_none() {
                let (a, b, c) = (pts[prev], pts[q], pts[(q + 1) % n2]);
                let ac = c - a;
                let dist = (b - a).perp_dot(ac).abs() / ac.length().max(1e-12);
                if dist < tol && (b - a).dot(ac) > 0.0 && (c - b).dot(ac) > 0.0 {
                    pts.remove(q);
                    arc.remove(q);
                    changed = true;
                    continue;
                }
            }
            q += 1;
        }
        let _ = n;
    }
    Loop2 { pts, arc }
}

/// Outlines exported as many short lines (hidden-line output of CAD programs): straight runs
/// become one line, runs on a circle an arc, a loop on one circle a circle.
pub fn simplify_loop(l: &Loop2, tol: f64) -> Loop2 {
    if l.pts.len() < 3 {
        return l.clone();
    }
    // pieces of straight edges first: what is left of a tessellated arc are its corners,
    // which lie on the true circle
    let l = &merge_collinear(l, tol * 0.25);
    let n = l.pts.len();
    // a whole loop on one circle
    if l.arc.iter().any(|a| a.is_none()) && n >= 8 {
        if let Some((c, r)) = fit_circle(&l.pts) {
            let step_ok = (0..n).all(|i| {
                let (p0, p1) = (l.pts[i] - c, l.pts[(i + 1) % n] - c);
                p0.perp_dot(p1).atan2(p0.dot(p1)).abs() < 0.45
            });
            if step_ok && l.pts.iter().all(|p| ((*p - c).length() - r).abs() < tol.max(r * 5e-4)) && r > tol * 2.0 {
                return Loop2 { pts: l.pts.clone(), arc: vec![Some(ArcSrc { c, r }); n] };
            }
        }
    }
    // start at a corner, so no arc or straight run is cut in two
    let turn = |i: usize| {
        let (a, b, c) = (l.pts[(i + n - 1) % n], l.pts[i], l.pts[(i + 1) % n]);
        let (u, v) = ((b - a).normalize_or_zero(), (c - b).normalize_or_zero());
        u.dot(v).clamp(-1.0, 1.0).acos()
    };
    let start = (0..n).max_by(|&a, &b| turn(a).partial_cmp(&turn(b)).unwrap()).unwrap_or(0);
    let pts: Vec<V2> = (0..n).map(|i| l.pts[(start + i) % n]).collect();
    let tags: Vec<Option<ArcSrc>> = (0..n).map(|i| l.arc[(start + i) % n]).collect();
    let mut arc: Vec<Option<ArcSrc>> = tags.clone();
    // arcs over runs of at least 3 untagged steps
    let mut i = 0;
    while i < n {
        if tags[i].is_some() {
            i += 1;
            continue;
        }
        let mut j = i;
        while j + 1 < n && tags[j + 1].is_none() {
            j += 1;
        }
        let mut k = i;
        while k <= j {
            let mut best = None;
            let mut end = j;
            while end >= k + 2 {
                let run: Vec<V2> = (k..=end + 1).map(|q| pts[q % n]).collect();
                if let Some((ctr, r)) = fit_circle(&run) {
                    let span = (run[run.len() - 1] - run[0]).length();
                    if r > tol * 4.0 && r < span * 50.0 + tol {
                        let ok = run.iter().all(|p| ((*p - ctr).length() - r).abs() < tol.max(r * 5e-4));
                        let mut total = 0.0;
                        let mut sign = 0.0;
                        let mut steady = true;
                        for w in run.windows(2) {
                            let (p0, p1) = (w[0] - ctr, w[1] - ctr);
                            let ang = p0.perp_dot(p1).atan2(p0.dot(p1));
                            if ang.abs() > 0.45 || (sign != 0.0 && ang.signum() != sign) {
                                steady = false;
                            }
                            sign = ang.signum();
                            total += ang.abs();
                        }
                        // tessellated arcs have steps of about one length; a long straight edge
                        // between the ends of two arcs is no arc
                        let lens: Vec<f64> = run.windows(2).map(|w| (w[1] - w[0]).length()).collect();
                        let (lmin, lmax) = (lens.iter().cloned().fold(f64::INFINITY, f64::min), lens.iter().cloned().fold(0.0, f64::max));
                        let even = lmax <= lmin * 3.0 + tol;
                        // it must really turn (several pieces of one straight edge are no arc)
                        if ok && steady && even && total > 0.15 {
                            best = Some((end, ctr, r));
                            break;
                        }
                    }
                }
                end -= 1;
            }
            match best {
                Some((end, c, r)) => {
                    for q in k..=end {
                        arc[q] = Some(ArcSrc { c, r });
                    }
                    k = end + 1;
                }
                None => k += 1,
            }
        }
        i = j + 1;
    }
    Loop2 { pts, arc }
}

/// Removes dead ends the outline walks out and back along (dangling lines).
fn remove_spikes(l: &mut Loop2) {
    let mut changed = true;
    while changed && l.pts.len() >= 3 {
        changed = false;
        let n = l.pts.len();
        for i in 0..n {
            let (a, c) = (l.pts[(i + n - 1) % n], l.pts[(i + 1) % n]);
            if (a - c).length() < 1e-12 {
                // a → b → a: drop b and the second a
                let i1 = (i + 1) % n;
                let mut keep: Vec<bool> = vec![true; n];
                keep[i] = false;
                keep[i1] = false;
                let mut j = 0;
                l.pts.retain(|_| {
                    j += 1;
                    keep[j - 1]
                });
                let mut j = 0;
                l.arc.retain(|_| {
                    j += 1;
                    keep[j - 1]
                });
                changed = true;
                break;
            }
        }
    }
}

// ------------------------------------------------------------------------------------------
// views

/// European (first angle: the top view lies below the front view) or American (third angle).
pub use super::drawing::Projection;

#[derive(Clone, Debug)]
pub struct View2 {
    pub outline: Loop2,
    /// separate outlines inside the view (holes, pockets, bosses seen end on)
    pub inner: Vec<Loop2>,
    pub hidden: Vec<(V2, V2)>,
    pub visible: Vec<(V2, V2)>,
    pub lo: V2,
    pub hi: V2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Front,
    /// above or below the front view: seen along Y
    Plan { below: bool },
    /// beside the front view: seen along X
    Side { right: bool },
}

pub struct Analysis {
    pub views: Vec<View2>,
    pub roles: Vec<(usize, Role)>,
    pub projection: Projection,
    /// model millimetres per drawing unit (units and drawing scale)
    pub scale: f64,
    pub scale_source: String,
    pub thickness: Option<f64>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Options {
    /// None: from the drawing (top view below the front view = European)
    pub projection: Option<Projection>,
    /// model mm per drawing unit; None: from dimensions, dimension texts or the title block
    pub scale: Option<f64>,
    /// thickness of a single view; None: from a "t=…" note, else 10 mm
    pub thickness: Option<f64>,
}

impl Default for Options {
    fn default() -> Self {
        Options { projection: None, scale: None, thickness: None }
    }
}

fn seg_len(s: &Shape2) -> f64 {
    match s {
        Shape2::Line(a, b) => (*b - *a).length(),
        Shape2::Arc { r, a0, a1, .. } => {
            let mut w = a1 - a0;
            while w <= 0.0 {
                w += std::f64::consts::TAU;
            }
            r * w
        }
        Shape2::Circle { r, .. } => std::f64::consts::TAU * r,
    }
}

fn shape_pts(s: &Shape2) -> Vec<V2> {
    let mut arcs = vec![];
    shape_segments(s, &mut arcs).into_iter().flat_map(|(a, b, _)| [a, b]).collect()
}

/// Finds the views of a drawing, their roles, projection method and scale.
pub fn analyze(d: &Drawing2, opt: &Options) -> Result<Analysis, String> {
    let mut notes = vec![];
    // drawing extent
    let (mut lo, mut hi) = (V2::splat(f64::INFINITY), V2::splat(f64::NEG_INFINITY));
    for e in &d.ents {
        for p in shape_pts(&e.shape) {
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    let diag = (hi - lo).length().max(1e-6);
    let tol = (diag * 2e-5).max(1e-5);
    let visible: Vec<&Shape2> = d.ents.iter().filter(|e| e.kind == LineKind::Visible && seg_len(&e.shape) > tol).map(|e| &e.shape).collect();
    let g = build_graph(&visible, tol);
    let mut comps = components(&g);
    comps.retain(|c| c.outer.area().abs() > (diag * 1e-3).powi(2) || c.outer.arc.iter().all(|a| a.is_some()));
    if comps.is_empty() {
        return Err("In der Zeichnung wurde keine geschlossene Kontur gefunden.".into());
    }
    // ---- the sheet frame and the title block
    let n = comps.len();
    let bb: Vec<(V2, V2)> = comps.iter().map(|c| c.outer.bbox()).collect();
    let inside = |a: usize, b: usize| -> bool {
        // a inside b
        a != b && bb[a].0.x >= bb[b].0.x - tol && bb[a].0.y >= bb[b].0.y - tol && bb[a].1.x <= bb[b].1.x + tol && bb[a].1.y <= bb[b].1.y + tol && comps[b].outer.contains(comps[a].outer.pts[0])
    };
    let mut dropped = vec![false; n];
    for c in 0..n {
        let all = (0..n).filter(|&o| o != c).count();
        let contained = (0..n).filter(|&o| inside(o, c)).count();
        let texts_in = d.texts.iter().filter(|t| comps[c].outer.contains(t.pos)).count();
        // the largest outline of all
        let area = |i: usize| (bb[i].1.x - bb[i].0.x) * (bb[i].1.y - bb[i].0.y);
        let full = (0..n).all(|o| o == c || area(o) < area(c));
        if full && all > 0 && (contained * 10 >= all * 8) && (contained >= 2 || texts_in >= 3) {
            dropped[c] = true;
            notes.push("Zeichnungsrahmen und Schriftfeld übergangen.".into());
        }
    }
    // title blocks standing alone: a box with several texts and no circles
    for c in 0..n {
        if dropped[c] {
            continue;
        }
        let texts_in = d.texts.iter().filter(|t| comps[c].outer.contains(t.pos) && !(0..n).any(|o| o != c && !dropped[o] && inside(o, c) && comps[o].outer.contains(t.pos))).count();
        // a title block: a grid of boxes (only horizontal and vertical lines) full of text, at
        // the edge of the drawing
        let o = &comps[c].outer;
        let axis = (0..o.pts.len()).all(|i| {
            let d = o.pts[(i + 1) % o.pts.len()] - o.pts[i];
            d.x.abs() < tol * 4.0 || d.y.abs() < tol * 4.0
        }) && comps[c].faces.iter().all(|f| (0..f.pts.len()).all(|i| {
            let d = f.pts[(i + 1) % f.pts.len()] - f.pts[i];
            d.x.abs() < tol * 4.0 || d.y.abs() < tol * 4.0
        }));
        let edge = (bb[c].0.x - lo.x).abs() < diag * 0.05 || (hi.x - bb[c].1.x).abs() < diag * 0.05 || (bb[c].0.y - lo.y).abs() < diag * 0.05 || (hi.y - bb[c].1.y).abs() < diag * 0.05;
        if texts_in >= 4 && comps[c].faces.len() >= 3 && axis && edge && comps[c].outer.arc.iter().all(|a| a.is_none()) {
            dropped[c] = true;
            notes.push("Schriftfeld übergangen.".into());
        }
    }
    // ---- views: outlines not inside another kept outline; the ones inside are their holes
    let mut views: Vec<View2> = vec![];
    let mut owner = vec![usize::MAX; n];
    let tops: Vec<usize> = (0..n).filter(|&c| !dropped[c] && !(0..n).any(|o| !dropped[o] && inside(c, o))).collect();
    let fit = (diag * 2e-5).max(tol * 2.0);
    for &t in &tops {
        let (l, h) = bb[t];
        views.push(View2 { outline: simplify_loop(&comps[t].outer, fit), inner: vec![], hidden: vec![], visible: vec![], lo: l, hi: h });
        owner[t] = views.len() - 1;
    }
    for c in 0..n {
        if dropped[c] || owner[c] != usize::MAX {
            continue;
        }
        if let Some(v) = (0..views.len()).find(|&v| tops.get(v).is_some_and(|&t| inside(c, t))) {
            views[v].inner.push(simplify_loop(&comps[c].outer, fit));
        }
    }
    // hidden and visible line pieces per view (for depths of holes and pockets)
    for e in &d.ents {
        if !matches!(e.kind, LineKind::Hidden | LineKind::Visible) {
            continue;
        }
        let pts = shape_pts(&e.shape);
        let m = pts.iter().fold(V2::ZERO, |a, p| a + *p) / pts.len().max(1) as f64;
        let grow = diag * 0.004;
        if let Some(v) = views.iter_mut().find(|v| m.x >= v.lo.x - grow && m.x <= v.hi.x + grow && m.y >= v.lo.y - grow && m.y <= v.hi.y + grow) {
            for w in pts.chunks(2) {
                if e.kind == LineKind::Hidden {
                    v.hidden.push((w[0], w[1]));
                } else {
                    v.visible.push((w[0], w[1]));
                }
            }
        }
    }
    // ---- roles from the alignment
    // orthographic views line up exactly (to within the drawing's rounding)
    let atol = |a: f64, b: f64| (a - b).abs() <= (diag * 2e-4).max(tol * 10.0);
    let vert = |a: &View2, b: &View2| atol(a.lo.x, b.lo.x) && atol(a.hi.x, b.hi.x) && (a.hi.y < b.lo.y || b.hi.y < a.lo.y);
    let horz = |a: &View2, b: &View2| atol(a.lo.y, b.lo.y) && atol(a.hi.y, b.hi.y) && (a.hi.x < b.lo.x || b.hi.x < a.lo.x);
    let score = |i: usize| -> (usize, usize) {
        let v = (0..views.len()).filter(|&j| j != i && vert(&views[i], &views[j])).count();
        let h = (0..views.len()).filter(|&j| j != i && horz(&views[i], &views[j])).count();
        (v.min(2) + h.min(2), (views[i].outline.area().abs() * 1e6) as usize)
    };
    let mut roles: Vec<(usize, Role)> = vec![];
    let front = (0..views.len()).max_by_key(|&i| score(i)).unwrap();
    let fv = &views[front];
    let plans: Vec<usize> = (0..views.len()).filter(|&j| j != front && vert(fv, &views[j])).collect();
    let sides: Vec<usize> = (0..views.len()).filter(|&j| j != front && horz(fv, &views[j])).collect();
    // with only two views one above the other and nothing else, the upper one is the front view
    // of a European drawing
    let mut front = front;
    let mut plans = plans;
    if sides.is_empty() && plans.len() == 1 && opt.projection != Some(Projection::Third) && views[plans[0]].lo.y > views[front].hi.y {
        let p = plans[0];
        plans = vec![front];
        front = p;
    }
    roles.push((front, Role::Front));
    let fv = &views[front];
    // a side view is as wide as the top view is deep; an isometric view next to the front view
    // that happens to be as tall is not
    let depth_of = |i: usize| views[i].hi.y - views[i].lo.y;
    let width_of = |i: usize| views[i].hi.x - views[i].lo.x;
    let consistent = |p: usize, s: usize| (depth_of(p) - width_of(s)).abs() <= (diag * 5e-4).max(tol * 20.0);
    let (plans, sides): (Vec<usize>, Vec<usize>) = if !plans.is_empty() && !sides.is_empty() {
        let pairs: Vec<(usize, usize)> = plans.iter().flat_map(|&p| sides.iter().map(move |&s| (p, s))).filter(|&(p, s)| consistent(p, s)).collect();
        if pairs.is_empty() {
            // no side view fits the top view: keep the top view only
            (plans.clone(), vec![])
        } else {
            (pairs.iter().map(|x| x.0).collect(), pairs.iter().map(|x| x.1).collect())
        }
    } else {
        (plans, sides)
    };
    // nearest one on each side
    if let Some(&p) = plans.iter().min_by(|&&a, &&b| ((views[a].lo.y + views[a].hi.y) - (fv.lo.y + fv.hi.y)).abs().partial_cmp(&((views[b].lo.y + views[b].hi.y) - (fv.lo.y + fv.hi.y)).abs()).unwrap()) {
        roles.push((p, Role::Plan { below: views[p].hi.y < fv.lo.y }));
    }
    if let Some(&s) = sides.iter().min_by(|&&a, &&b| ((views[a].lo.x + views[a].hi.x) - (fv.lo.x + fv.hi.x)).abs().partial_cmp(&((views[b].lo.x + views[b].hi.x) - (fv.lo.x + fv.hi.x)).abs()).unwrap()) {
        roles.push((s, Role::Side { right: views[s].lo.x > fv.hi.x }));
    }
    let ignored = views.len() - roles.len();
    if ignored > 0 && roles.len() > 1 {
        notes.push(format!("{} weitere Ansicht(en) ohne Ausrichtung zur Vorderansicht übergangen (z. B. isometrische Ansicht).", ignored));
    }
    let projection = opt.projection.unwrap_or_else(|| match roles.iter().find_map(|r| if let Role::Plan { below } = r.1 { Some(below) } else { None }) {
        Some(false) => Projection::Third,
        _ => Projection::First,
    });
    // ---- scale
    let (scale, scale_source) = match opt.scale {
        Some(s) => (s, "eingegeben".to_string()),
        None => find_scale(d, &views, &roles, diag),
    };
    // ---- thickness note for a single view
    let thickness = opt.thickness.or_else(|| {
        d.texts.iter().find_map(|t| {
            let s = t.text.to_lowercase().replace(' ', "");
            for key in ["t=", "s=", "dicke", "blechdicke", "thk", "thickness", "stärke"] {
                if let Some(i) = s.find(key) {
                    let rest: String = s[i + key.len()..].trim_start_matches([':', '=']).chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect();
                    if let Ok(v) = rest.replace(',', ".").parse::<f64>() {
                        if v > 0.0 {
                            // written in the drawing's units (not scaled)
                            return Some(v * d.unit_mm);
                        }
                    }
                }
            }
            None
        })
    });
    Ok(Analysis { views, roles, projection, scale: scale * d.unit_mm, scale_source, thickness, notes })
}

/// Drawing scale (model length per drawn length) from the drawing itself.
fn find_scale(d: &Drawing2, views: &[View2], roles: &[(usize, Role)], diag: f64) -> (f64, String) {
    let median = |mut v: Vec<f64>| -> Option<f64> {
        if v.is_empty() {
            return None;
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let m = v[v.len() / 2];
        // most of them agree
        let agree = v.iter().filter(|x| (**x - m).abs() <= m * 0.01).count();
        if agree * 2 >= v.len() && agree >= 1 {
            Some(m)
        } else {
            None
        }
    };
    // 1. dimension entities
    if let Some(m) = median(d.dims.iter().map(|(v, l)| v / l).collect()) {
        return (round_scale(m), "aus den Bemaßungen".into());
    }
    // 2. dimension texts next to dimension lines with arrows at both ends
    let tips: Vec<V2> = d.solids.iter().map(|q| {
        // the tip: the corner farthest from the other two
        let c = (q[0] + q[1] + q[2]) / 3.0;
        *[q[0], q[1], q[2]].iter().max_by(|a, b| (**a - c).length().partial_cmp(&(**b - c).length()).unwrap()).unwrap()
    }).collect();
    let near_tip = |p: V2| tips.iter().any(|t| (*t - p).length() < diag * 0.002);
    let dim_lines: Vec<(V2, V2)> = d.ents.iter().filter_map(|e| match e.shape {
        Shape2::Line(a, b) if near_tip(a) && near_tip(b) => Some((a, b)),
        _ => None,
    }).collect();
    let mut ratios = vec![];
    for t in &d.texts {
        if t.text.contains('⌀') || t.text.starts_with('R') {
            continue;
        }
        let Some(v) = parse_number(&t.text) else { continue };
        if v <= 0.0 {
            continue;
        }
        let dir = V2::new(t.angle.cos(), t.angle.sin());
        let centre = t.pos + dir * (t.h * 0.3 * t.text.chars().count() as f64) + V2::new(-dir.y, dir.x) * (t.h * 0.5);
        let best = dim_lines
            .iter()
            .filter(|(a, b)| ((*b - *a).normalize().dot(dir)).abs() > 0.99)
            .map(|(a, b)| {
                let ab = *b - *a;
                let s = ((centre - *a).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0);
                ((*a + ab * s - centre).length(), ab.length())
            })
            .filter(|(dist, _)| *dist < t.h * 2.5)
            .min_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
        if let Some((_, l)) = best {
            ratios.push(v / l);
        }
    }
    if ratios.len() >= 2 {
        if let Some(m) = median(ratios) {
            return (round_scale(m), "aus den Maßzahlen".into());
        }
    }
    // 3. "1:2" in the title block
    for t in &d.texts {
        let s = t.text.replace(' ', "");
        let s = s.trim_start_matches("M").trim_start_matches("Maßstab").trim_start_matches("MASSSTAB");
        if let Some((a, b)) = s.split_once(':') {
            if let (Ok(a), Ok(b)) = (a.replace(',', ".").parse::<f64>(), b.replace(',', ".").parse::<f64>()) {
                if a > 0.0 && b > 0.0 && a.fract() == 0.0 || b.fract() == 0.0 {
                    if a > 0.0 && b > 0.0 {
                        return (b / a, format!("Maßstab {}:{} im Schriftfeld", a, b));
                    }
                }
            }
        }
    }
    let _ = (views, roles);
    (1.0, "1:1 angenommen".into())
}

/// Snaps a measured scale to the standard ones (1:2, 2:1, 1:2.5, ...) when it is that close.
fn round_scale(m: f64) -> f64 {
    for s in [1.0, 2.0, 2.5, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 0.5, 0.4, 0.2, 0.1, 0.05] {
        if (m - s).abs() < s * 0.005 {
            return s;
        }
    }
    m
}

// ------------------------------------------------------------------------------------------
// the 3D model

/// Mapping of a view's drawing coordinates into a sketch on one of the standard planes.
struct Map2 {
    /// sketch = (a.x * (u - ou), a.y * (v - ov)) for drawing (u, v)
    sx: f64,
    sy: f64,
    ou: f64,
    ov: f64,
    k: f64,
}

impl Map2 {
    fn ap(&self, p: V2) -> V2 {
        V2::new(snap(self.sx * (p.x - self.ou) * self.k), snap(self.sy * (p.y - self.ov) * self.k))
    }
}

/// Drawing coordinates carry rounding (often to 0.001 of the drawn size, times the scale):
/// values within 1.5 µm of a hundredth of a millimetre are that hundredth.
fn snap(v: f64) -> f64 {
    let h = (v * 100.0).round() / 100.0;
    if (h - v).abs() < 1.5e-3 {
        return h;
    }
    let r = (v * 1000.0).round() / 1000.0;
    if (r - v).abs() < 2e-3 {
        r
    } else {
        v
    }
}

/// A radius whose diameter is that close to a hundredth of a millimetre.
fn snap_r(r: f64) -> f64 {
    let d = 2.0 * r;
    let h = (d * 100.0).round() / 100.0;
    if (h - d).abs() < 4e-3 {
        h / 2.0
    } else {
        snap(r)
    }
}

/// Adds closed outlines to a sketch, keeping lines, arcs and circles.
fn add_loops(sk: &mut Sketch, loops: &[&Loop2], m: &Map2) {
    let mut pts: Vec<(V2, i64)> = vec![];
    let mut pid = |sk: &mut Sketch, p: V2| -> i64 {
        if let Some(q) = pts.iter().find(|q| (q.0 - p).length() < 1e-6) {
            return q.1;
        }
        let id = sk.add_point(p);
        pts.push((p, id));
        id
    };
    for l in loops {
        let n = l.pts.len();
        // a full circle
        if n >= 8 && l.arc.iter().all(|a| a.is_some()) {
            let a0 = l.arc[0].unwrap();
            if l.arc.iter().all(|a| a.is_some_and(|b| (b.c - a0.c).length() < 1e-9 && (b.r - a0.r).abs() < 1e-9)) {
                let c = sk.add_point(m.ap(a0.c));
                sk.add_circle(c, snap_r(a0.r * m.k));
                continue;
            }
        }
        // start where the arc tag changes, so no arc is split
        let same = |x: Option<ArcSrc>, y: Option<ArcSrc>| match (x, y) {
            (Some(a), Some(b)) => (a.c - b.c).length() < 1e-9 && (a.r - b.r).abs() < 1e-9,
            (None, None) => false,
            _ => false,
        };
        let start = (0..n).find(|&i| !same(l.arc[i], l.arc[(i + n - 1) % n])).unwrap_or(0);
        let mut i = 0;
        while i < n {
            let k = (start + i) % n;
            let a = l.pts[k];
            match l.arc[k] {
                None => {
                    let b = l.pts[(k + 1) % n];
                    let (ia, ib) = (pid(sk, m.ap(a)), pid(sk, m.ap(b)));
                    if ia != ib {
                        sk.add_line(ia, ib);
                    }
                    i += 1;
                }
                Some(arc) => {
                    let mut j = i;
                    while j + 1 < n && same(l.arc[(start + j + 1) % n], Some(arc)) {
                        j += 1;
                    }
                    let b = l.pts[(start + j + 1) % n];
                    let mid = l.pts[(start + (i + j + 1) / 2) % n];
                    let (sa, sb, sm, sc) = (m.ap(a), m.ap(b), m.ap(mid), m.ap(arc.c));
                    // counter-clockwise from start to end in the sketch
                    let ccw = (sm - sc).perp_dot(sb - sc) > 0.0 || (sa - sc).perp_dot(sm - sc) > 0.0;
                    let ic = sk.add_point(sc);
                    let (ia, ib) = (pid(sk, sa), pid(sk, sb));
                    if ia != ib {
                        if ccw {
                            sk.add_arc(ic, ia, ib);
                        } else {
                            sk.add_arc(ic, ib, ia);
                        }
                    }
                    i = j + 1;
                }
            }
        }
    }
}

fn rect_loop(lo: V2, hi: V2) -> Loop2 {
    Loop2 { pts: vec![lo, V2::new(hi.x, lo.y), hi, V2::new(lo.x, hi.y)], arc: vec![None; 4] }
}

/// Depth range of an inner outline of view `v` (along the view axis) from the hidden lines
/// of `other` at its edges; `along` picks the other view's coordinate that runs across.
/// Returns (lo, hi) in the other view's coordinate, or None when nothing is drawn there.
fn hidden_range(lp: &Loop2, other: &View2, across_is_x: bool, depth_is_y: bool) -> Option<(f64, f64, bool)> {
    let (lo, hi) = lp.bbox();
    let (e0, e1) = if across_is_x { (lo.x, hi.x) } else { (lo.y, hi.y) };
    let w = (e1 - e0).abs().max(1e-6);
    let tol = (w * 0.02).max(1e-4);
    let coord = |p: V2, x: bool| if x { p.x } else { p.y };
    let mut range: Option<(f64, f64)> = None;
    fn grow(range: &mut Option<(f64, f64)>, a: f64, b: f64) {
        let (a, b) = (a.min(b), a.max(b));
        *range = Some(match *range {
            Some((x, y)) => (x.min(a), y.max(b)),
            None => (a, b),
        });
    }
    // hidden lines running along the depth at either edge of the outline; without them,
    // visible lines there (a boss or step seen from the side)
    let mut hidden = false;
    for (lines, is_hidden) in [(&other.hidden, true), (&other.visible, false)] {
        if hidden {
            break;
        }
        for (a, b) in lines {
            let (ca, cb) = (coord(*a, across_is_x), coord(*b, across_is_x));
            if (ca - cb).abs() < tol && ((ca - e0).abs() < tol || (ca - e1).abs() < tol) && (coord(*a, !depth_is_y) - coord(*b, !depth_is_y)).abs() > tol {
                grow(&mut range, coord(*a, !depth_is_y), coord(*b, !depth_is_y));
                hidden |= is_hidden;
            }
        }
        if range.is_some() {
            break;
        }
    }
    range.map(|(a, b)| (a, b, hidden))
}

/// Builds the part from an analysed drawing.
pub fn build(a: &Analysis, name: &str) -> Result<(ModelData, Vec<String>), String> {
    let mut notes = a.notes.clone();
    let mut m = ModelData::new_part(name);
    let k = a.scale;
    let role = |r: fn(&Role) -> bool| a.roles.iter().find(|x| r(&x.1)).map(|x| x.0);
    let front = role(|r| matches!(r, Role::Front)).ok_or("Keine Vorderansicht gefunden.")?;
    let plan = a.roles.iter().find_map(|x| if let Role::Plan { below } = x.1 { Some((x.0, below)) } else { None });
    let side = a.roles.iter().find_map(|x| if let Role::Side { right } = x.1 { Some((x.0, right)) } else { None });
    let first = a.projection == Projection::First;
    let fv = &a.views[front];
    // world: X along the front view's width (centred), Y up from its bottom, Z towards the
    // viewer of the front view (centred on the depth)
    let uc = (fv.lo.x + fv.hi.x) / 2.0;
    let vb = fv.lo.y;
    let mut next_id = 10u64;
    let mut push = |m: &mut ModelData, kind: FeatureKind| -> u64 {
        let id = next_id;
        next_id += 1;
        let name = m.next_name(&kind);
        m.features.push(Feature { id, name, suppressed: false, hidden: false, kind });
        id
    };
    let extrude = |plane: PlaneRef, sk: Sketch, remove: bool, depth: DepthType, d: f64, side2: Side2, flip: bool| -> FeatureKind {
        let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
        e.plane = Some(plane);
        e.sketch = sk;
        e.remove = remove;
        e.depth_type = depth;
        e.depth = Dim::v(snap(d));
        e.side2 = side2;
        e.flip = flip;
        FeatureKind::Extrude(e)
    };
    let width = (fv.hi.x - fv.lo.x) * k;
    let height = (fv.hi.y - fv.lo.y) * k;
    // front view in a FRONT sketch: x → X, y → Y
    let fmap = Map2 { sx: 1.0, sy: 1.0, ou: uc, ov: vb, k };
    // plan view in a TOP sketch (x → X, y → −Z); Z = s_v (w − wc)
    let pmap = plan.map(|(p, below)| {
        let pv = &a.views[p];
        let s_v = (if first { -1.0 } else { 1.0 }) * (if below { 1.0 } else { -1.0 });
        let wc = (pv.lo.y + pv.hi.y) / 2.0;
        (p, Map2 { sx: 1.0, sy: -s_v, ou: uc, ov: wc, k }, s_v)
    });
    // side view in a RIGHT sketch (x → −Z, y → Y); Z = s_h (s − sc)
    let smap = side.map(|(s, right)| {
        let sv = &a.views[s];
        let s_h = (if first { 1.0 } else { -1.0 }) * (if right { 1.0 } else { -1.0 });
        let sc = (sv.lo.x + sv.hi.x) / 2.0;
        (s, Map2 { sx: -s_h, sy: 1.0, ou: sc, ov: vb, k }, s_h)
    });
    if plan.is_none() && side.is_none() {
        // a single view: a plate of the given thickness, every inner outline cut through
        let t = a.thickness.unwrap_or(10.0);
        let mut sk = Sketch::default();
        let mut loops: Vec<&Loop2> = vec![&fv.outline];
        loops.extend(fv.inner.iter());
        add_loops(&mut sk, &loops, &fmap);
        push(&mut m, extrude(PlaneRef::Datum(FRONT_ID), sk, false, DepthType::Blind, t, Side2::None, false));
        if a.thickness.is_none() {
            notes.push("Nur eine Ansicht: als Platte mit 10 mm Dicke angelegt (Dicke im Profil änderbar).".into());
        } else {
            notes.push(format!("Nur eine Ansicht: als Platte mit {} mm Dicke angelegt.", fmt_num(t, 3)));
        }
        m.next_id = next_id;
        return Ok((m, notes));
    }
    let depth_span = |v: &View2, is_plan: bool| if is_plan { (v.hi.y - v.lo.y) * k } else { (v.hi.x - v.lo.x) * k };
    let big = 2.0 * (width + height + pmap.as_ref().map(|p| depth_span(&a.views[p.0], true)).or(smap.as_ref().map(|s| depth_span(&a.views[s.0], false))).unwrap_or(0.0)) + 10.0;
    let outside = |outline: &Loop2, map: &Map2| -> Sketch {
        let mut sk = Sketch::default();
        let r = rect_loop(V2::splat(-big), V2::splat(big));
        let id = Map2 { sx: 1.0, sy: 1.0, ou: 0.0, ov: 0.0, k: 1.0 };
        add_loops(&mut sk, &[&r], &id);
        add_loops(&mut sk, &[outline], map);
        sk
    };
    // ---- the base body and the cuts outside the other outlines
    if let Some((p, map, _)) = &pmap {
        let pv = &a.views[*p];
        let mut sk = Sketch::default();
        add_loops(&mut sk, &[&pv.outline], map);
        push(&mut m, extrude(PlaneRef::Datum(TOP_ID), sk, false, DepthType::Blind, height, Side2::None, false));
        push(&mut m, extrude(PlaneRef::Datum(FRONT_ID), outside(&fv.outline, &fmap), true, DepthType::ThroughAll, 0.0, Side2::ThroughAll, false));
    } else if let Some((s, map, _)) = &smap {
        let sv = &a.views[*s];
        let mut sk = Sketch::default();
        add_loops(&mut sk, &[&sv.outline], map);
        push(&mut m, extrude(PlaneRef::Datum(RIGHT_ID), sk, false, DepthType::Symmetric, width, Side2::None, false));
        push(&mut m, extrude(PlaneRef::Datum(FRONT_ID), outside(&fv.outline, &fmap), true, DepthType::ThroughAll, 0.0, Side2::ThroughAll, false));
    }
    if let (Some((s, map, _)), true) = (&smap, pmap.is_some()) {
        push(&mut m, extrude(PlaneRef::Datum(RIGHT_ID), outside(&a.views[*s].outline, map), true, DepthType::ThroughAll, 0.0, Side2::ThroughAll, false));
    }
    // ---- holes and pockets: inner outlines of each view, over the depth their hidden lines show
    let mut cuts = 0;
    let mut bosses = 0;
    let datum = |m: &mut ModelData, reference: u64, offset: f64, next: &mut dyn FnMut(&mut ModelData, FeatureKind) -> u64| -> PlaneRef {
        if offset.abs() < 1e-6 {
            return PlaneRef::Datum(reference);
        }
        let mut d = DatumPlaneDef::builtin(PlaneKind::Offset);
        d.reference = Some(PlaneRef::Datum(reference));
        d.offset = Dim::v(snap(offset));
        PlaneRef::Datum(next(m, FeatureKind::DatumPlane(d)))
    };
    // (view, its sketch map, plane, the other views to look for hidden lines in)
    struct Inner<'a> {
        view: usize,
        map: &'a Map2,
        plane: u64,
    }
    let mut list: Vec<Inner> = vec![Inner { view: front, map: &fmap, plane: FRONT_ID }];
    if let Some((p, map, _)) = &pmap {
        list.push(Inner { view: *p, map, plane: TOP_ID });
    }
    if let Some((s, map, _)) = &smap {
        list.push(Inner { view: *s, map, plane: RIGHT_ID });
    }
    for it in &list {
        let v = &a.views[it.view];
        for lp in &v.inner {
            // depth along the view's axis from the hidden lines of a neighbouring view
            let (range, through) = match it.plane {
                // front view outline (axis Z): the plan view shows it across x, depth in w
                FRONT_ID => {
                    let from_plan = pmap.as_ref().and_then(|(p, _, s_v)| hidden_range(lp, &a.views[*p], true, true).map(|(x, y, h)| ((s_v * (x - (a.views[*p].lo.y + a.views[*p].hi.y) / 2.0) * k, s_v * (y - (a.views[*p].lo.y + a.views[*p].hi.y) / 2.0) * k), h)));
                    let from_side = || {
                        smap.as_ref().and_then(|(s, _, s_h)| {
                            let sv = &a.views[*s];
                            hidden_range(lp, sv, false, false).map(|(x, y, h)| ((s_h * (x - (sv.lo.x + sv.hi.x) / 2.0) * k, s_h * (y - (sv.lo.x + sv.hi.x) / 2.0) * k), h))
                        })
                    };
                    match from_plan.or_else(from_side) {
                        Some((r, h)) => (Some((r.0.min(r.1), r.0.max(r.1))), h),
                        None => (None, true),
                    }
                }
                // plan view outline (axis Y): the front view shows it across x, height in v
                TOP_ID => match hidden_range(lp, fv, true, true) {
                    Some((y0, y1, h)) => (Some(((y0 - vb) * k, (y1 - vb) * k)), h),
                    None => (None, true),
                },
                // side view outline (axis X): the front view shows it across y, width in u
                _ => match hidden_range(lp, fv, false, false) {
                    Some((x0, x1, h)) => (Some(((x0 - uc) * k, (x1 - uc) * k)), h),
                    None => (None, true),
                },
            };
            // (range, hidden): hidden lines give a hole or pocket over the range; visible
            // lines only, a boss or step the outlines already make; nothing, a through hole
            let (range, hidden) = (range, through);
            let mut sk = Sketch::default();
            add_loops(&mut sk, &[lp], it.map);
            if let (Some((r0, r1)), false) = (range, hidden) {
                // a boss seen end on: the other outlines make it as wide as its bounding box;
                // cut what lies around the outline over the boss's height
                let (blo, bhi) = lp.bbox();
                let box_area = (bhi.x - blo.x) * (bhi.y - blo.y);
                if lp.area().abs() < box_area * 0.98 && r1 - r0 > 1e-3 {
                    let g = (bhi - blo).max_element() * 0.02;
                    let ring = rect_loop(blo - V2::splat(g), bhi + V2::splat(g));
                    let mut sk = Sketch::default();
                    add_loops(&mut sk, &[&ring, lp], it.map);
                    let pl = datum(&mut m, it.plane, r0, &mut |m, kind| push(m, kind));
                    push(&mut m, extrude(pl, sk, true, DepthType::Blind, r1 - r0, Side2::None, false));
                }
                bosses += 1;
                continue;
            }
            match range {
                Some((r0, r1)) if r1 - r0 > 1e-3 => {
                    // the plane's normal: FRONT +Z, TOP +Y, RIGHT +X; the RIGHT sketch maps
                    // x → −Z, so its depth runs along +X
                    let pl = datum(&mut m, it.plane, r0, &mut |m, kind| push(m, kind));
                    push(&mut m, extrude(pl, sk, true, DepthType::Blind, r1 - r0, Side2::None, false));
                }
                _ => {
                    push(&mut m, extrude(PlaneRef::Datum(it.plane), sk, true, DepthType::ThroughAll, 0.0, Side2::ThroughAll, false));
                }
            }
            cuts += 1;
        }
    }
    if cuts > 0 {
        notes.push(format!("{} Bohrung(en)/Aussparung(en) aus den Innenkonturen geschnitten.", cuts));
    }
    if bosses > 0 {
        notes.push(format!("{} Zapfen/Absätze aus Innenkonturen mit sichtbaren Kanten geformt.", bosses));
    }
    m.next_id = next_id;
    Ok((m, notes))
}

/// Reads and converts a drawing file: DXF directly, DWG through an installed converter.
pub fn import_file(path: &std::path::Path, opt: &Options) -> Result<(ModelData, Analysis, Vec<String>), String> {
    let text = read_drawing_text(path)?;
    let d = read_dxf(&text)?;
    let a = analyze(&d, opt)?;
    let name: String = path.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_else(|| "ZEICHNUNG".into()).chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    let (m, notes) = build(&a, &name)?;
    Ok((m, a, notes))
}

/// The DXF text of a drawing file; DWG files are converted with LibreDWG's dwg2dxf or the
/// ODA File Converter when one of them is installed.
pub fn read_drawing_text(path: &std::path::Path) -> Result<String, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext == "dwg" {
        return dwg_to_dxf(path);
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Err("Binäre DXF-Dateien werden nicht unterstützt: bitte als ASCII-DXF speichern.".into());
    }
    if bytes.starts_with(b"AC10") {
        return Err("Das ist eine DWG-Datei: bitte mit der Endung .dwg öffnen.".into());
    }
    // DXF up to R2004 is ANSI (Windows-1252), newer ones UTF-8
    Ok(match String::from_utf8(bytes.clone()) {
        Ok(s) => s,
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    })
}

fn dwg_to_dxf(path: &std::path::Path) -> Result<String, String> {
    let tmp = std::env::temp_dir().join(format!("freo-dwg-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    let out = tmp.join("drawing.dxf");
    let _ = std::fs::remove_file(&out);
    // LibreDWG (brew install libredwg)
    for exe in ["dwg2dxf", "/opt/homebrew/bin/dwg2dxf", "/usr/local/bin/dwg2dxf"] {
        if let Ok(st) = std::process::Command::new(exe).arg("-y").arg("-o").arg(&out).arg(path).status() {
            if st.success() || out.exists() {
                if let Ok(s) = std::fs::read(&out) {
                    return Ok(String::from_utf8_lossy(&s).to_string());
                }
            }
        }
    }
    // ODA File Converter (free download from the Open Design Alliance)
    for exe in ["/Applications/ODAFileConverter.app/Contents/MacOS/ODAFileConverter", "ODAFileConverter"] {
        let src = path.parent().unwrap_or(std::path::Path::new("."));
        if let Ok(st) = std::process::Command::new(exe).arg(src).arg(&tmp).arg("ACAD2018").arg("DXF").arg("0").arg("1").arg(path.file_name().unwrap_or_default()).status() {
            if st.success() {
                let f = tmp.join(path.with_extension("dxf").file_name().unwrap_or_default());
                if let Ok(s) = std::fs::read(&f) {
                    return Ok(String::from_utf8_lossy(&s).to_string());
                }
            }
        }
    }
    Err("DWG-Dateien brauchen einen Konverter: LibreDWG (Terminal: brew install libredwg) oder den kostenlosen ODA File Converter installieren – oder die Zeichnung in AutoCAD als DXF speichern.".into())
}
