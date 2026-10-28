//! Drawing mode: the drawing session, view geometry generated from the model in session
//! (hidden-line projection, cross-sections, detail views), shown model dimensions and created
//! driven dimensions, the BOM table and balloons, and the sheet's vector primitives, which
//! the screen and the PDF / SVG / DXF writers all draw from.

use super::dims3d::{prepared, DimKey};
use super::*;
use crate::kernel::builders;
use crate::kernel::csg::{self, CPolygon};
use crate::kernel::hlr::{self, ViewLines};
use crate::kernel::topology::{Topology, BODY_SHIFT};
use crate::model::assembly as am;
use crate::model::drawing::*;
use crate::sketch::sketch::{ProfileRegion, SkCType};
use glam::DQuat;
use std::collections::HashMap;

// ---------------------------------------------------------------- primitives

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dash { Solid, Hidden, Center, Phantom }

impl Dash {
    /// Dash and gap lengths in mm.
    pub fn pattern(&self) -> &'static [f64] {
        match self { Dash::Solid => &[], Dash::Hidden => &[3.0, 1.5], Dash::Center => &[10.0, 1.5, 1.5, 1.5], Dash::Phantom => &[12.0, 1.5, 2.0, 1.5, 2.0, 1.5] }
    }
}

pub type Rgb = [u8; 3];
pub const INK: Rgb = [0, 0, 0];
pub const GREY: Rgb = [105, 105, 105];
pub const SELC: Rgb = [20, 160, 60];
pub const HOVC: Rgb = [30, 120, 230];
pub const MISSING: Rgb = [210, 40, 30];

#[derive(Clone, Debug)]
pub enum Prim {
    Line { pts: Vec<V2>, w: f64, c: Rgb, dash: Dash },
    Fill { pts: Vec<V2>, c: Rgb },
    Circle { c: V2, r: f64, w: f64, col: Rgb, dash: Dash, fill: bool },
    /// `pos` is the alignment point; `halign`/`valign` are -1, 0, 1 (left/bottom … right/top)
    Text { pos: V2, text: String, h: f64, angle: f64, halign: i8, valign: i8, c: Rgb },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DSel { View(u32), Shown(usize), Dim(u32), Note(u32), Balloon(usize), Bom, Gtol(u32), Datum(u32), Finish(u32) }

/// A clickable region of the sheet.
#[derive(Clone, Copy, Debug)]
pub struct Hit { pub min: V2, pub max: V2, pub sel: DSel }

/// Font size (em) for a text of cap height `h`.
pub fn text_em(h: f64) -> f64 { h / 0.718 }
pub fn text_width(s: &str, h: f64) -> f64 { s.chars().map(super::publish::char_width).sum::<f64>() / 1000.0 * text_em(h) }

/// Start of the baseline for an aligned text (rotation applied about `pos`).
pub fn text_origin(pos: V2, text: &str, h: f64, angle: f64, halign: i8, valign: i8) -> V2 {
    let w = text_width(text, h);
    let local = V2::new(-w * (halign as f64 + 1.0) / 2.0, -h * (valign as f64 + 1.0) / 2.0);
    let (s, c) = angle.sin_cos();
    pos + V2::new(local.x * c - local.y * s, local.x * s + local.y * c)
}

fn rot2(v: V2, a: f64) -> V2 { let (s, c) = a.sin_cos(); V2::new(v.x * c - v.y * s, v.x * s + v.y * c) }

// ---------------------------------------------------------------- session

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetCam { pub center: V2, pub zoom: f32 }

#[derive(Clone, Debug, PartialEq)]
pub enum DTool {
    None,
    General,
    Projection { parent: Option<u32> },
    Detail { parent: Option<u32>, center: Option<V2>, radius: Option<f64> },
    Dimension { first: Option<Snap> },
    Note,
    /// datum feature symbol: pick the feature edge, then place the box
    Datum { at: Option<(u32, DimRef)> },
    /// feature control frame: pick a dimension, or an edge and then the frame position
    Gtol { at: Option<(u32, DimRef)> },
    /// surface texture symbol: pick an edge
    Finish,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapKind { Point, Circle { c: V2, r: f64 }, Edge { a: V2, b: V2 } }

/// Pickable view geometry under the pointer while dimensioning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap { pub view: u32, pub kind: SnapKind, pub sheet: V2, pub a: DimRef, pub b: Option<DimRef> }

#[derive(Clone, Debug)]
pub struct ViewDialog { pub view: u32, pub draft: DrawView, pub cat: u8, pub is_new: bool, pub custom_scale: String }

pub struct TextEdit { pub sel: DSel, pub text: String, pub pos: eframe::egui::Pos2, pub focus: bool, pub multiline: bool }

/// Geometry of one view in its own model coordinates (projected, millimetres).
pub struct ViewGeom {
    pub lines: ViewLines,
    pub hatch: Vec<[V2; 2]>,
    /// model-space point that sits at the view position (alignment origin)
    pub c: V2,
    pub scale: f64,
    /// world → view
    pub w2v: DQuat,
    /// detail views: clipping circle (centre, radius) in model coordinates
    pub clip: Option<(V2, f64)>,
    pub key: u64,
    pub model: Arc<RegenResult>,
}

impl ViewGeom {
    pub fn to_sheet(&self, pos: V2, q: V2) -> V2 { pos + (q - self.c) * self.scale }
    pub fn world(&self, pos: V2, p: V3) -> V2 { self.to_sheet(pos, (self.w2v * p).truncate()) }
    pub fn from_sheet(&self, pos: V2, s: V2) -> V2 { self.c + (s - pos) / self.scale }
    /// Sheet rectangle of the view outline.
    pub fn rect(&self, pos: V2) -> (V2, V2) {
        if let Some((c, r)) = self.clip { return (self.to_sheet(pos, c - V2::splat(r)), self.to_sheet(pos, c + V2::splat(r))); }
        if self.lines.min.x > self.lines.max.x { return (pos - V2::splat(10.0), pos + V2::splat(10.0)); }
        (self.to_sheet(pos, self.lines.min), self.to_sheet(pos, self.lines.max))
    }
}

/// A dimension of the model that a drawing can show.
#[derive(Clone, Debug)]
pub struct ModelDim {
    pub key: String,
    pub feature: u64,
    pub dk: Option<DimKey>,
    pub geom: MGeom,
    pub value: f64,
    pub prefix: &'static str,
    /// callout template; each `#` is replaced by the next of `args` (formatted)
    pub text: Option<String>,
    pub args: Vec<f64>,
    pub edit: String,
}

impl ModelDim {
    pub fn display(&self, d: &DrawingData) -> String {
        match &self.text {
            None => format!("{}{}", self.prefix, d.fmt(self.value)),
            Some(t) => {
                let mut out = String::new();
                let mut k = 0;
                for c in t.chars() { if c == '#' { out.push_str(&d.fmt(self.args.get(k).copied().unwrap_or(0.0))); k += 1; } else { out.push(c); } }
                out
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum MGeom {
    /// distance between `a` and `b` measured along `dir`; `plane` is the sketch plane normal
    Linear { a: V3, b: V3, dir: V3, plane: Option<V3>, label: V3 },
    Radial { c: V3, r: f64, normal: V3, label: V3, diameter: bool },
}

pub struct DrawSession {
    pub data: DrawingData,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub undo: Vec<DrawingData>,
    pub redo: Vec<DrawingData>,
    /// drawing window shown (false while its model is open in 3D)
    pub active: bool,
    pub sheet: usize,
    pub cam: SheetCam,
    pub fit: bool,
    pub assembly: bool,
    pub model_name: String,
    pub cache: HashMap<u32, Arc<ViewGeom>>,
    models: HashMap<bool, (u64, Arc<RegenResult>)>,
    pub dims: Vec<ModelDim>,
    dims_gen: u64,
    pub sel: Vec<DSel>,
    pub hover: Option<DSel>,
    pub tool: DTool,
    pub snap: Option<Snap>,
    pub view_dialog: Option<ViewDialog>,
    pub props: bool,
    pub edit: Option<TextEdit>,
    pub drag: Option<(DSel, V2)>,
    pub lock_views: bool,
    /// property edits since the last selection change form one undo step
    pub coalesce: bool,
    pub last_sel: Vec<DSel>,
    pub hits: Vec<Hit>,
    pub pointer: Option<V2>,
}

impl DrawSession {
    pub fn new(data: DrawingData, assembly: bool, model_name: &str) -> DrawSession {
        DrawSession {
            data, path: None, dirty: false, undo: vec![], redo: vec![], active: true, sheet: 0,
            cam: SheetCam { center: V2::new(210.0, 148.5), zoom: 2.0 }, fit: true, assembly, model_name: model_name.into(),
            cache: HashMap::new(), models: HashMap::new(), dims: vec![], dims_gen: 0, sel: vec![], hover: None, tool: DTool::None,
            snap: None, view_dialog: None, props: false, edit: None, drag: None, lock_views: false, coalesce: false, last_sel: vec![], hits: vec![], pointer: None,
        }
    }
    pub fn cur(&self) -> &Sheet { &self.data.sheets[self.sheet.min(self.data.sheets.len() - 1)] }
}

fn hash_of(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn projected_centre(b: &BBox, w2v: DQuat) -> V2 {
    if b.is_empty() { return V2::ZERO; }
    let (mut lo, mut hi) = (V2::splat(f64::INFINITY), V2::splat(f64::NEG_INFINITY));
    for k in 0..8 {
        let p = V3::new(if k & 1 == 0 { b.min.x } else { b.max.x }, if k & 2 == 0 { b.min.y } else { b.max.y }, if k & 4 == 0 { b.min.z } else { b.max.z });
        let q = (w2v * p).truncate();
        lo = lo.min(q);
        hi = hi.max(q);
    }
    (lo + hi) * 0.5
}

/// Hatch lines of a convex polygon (global spacing so neighbouring pieces line up).
fn hatch_polygon(poly: &[V2], angle: f64, spacing: f64, out: &mut Vec<[V2; 2]>) {
    if poly.len() < 3 || spacing <= 0.0 { return; }
    let n = V2::new(-angle.sin(), angle.cos());
    let (lo, hi) = poly.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| (l.min(p.dot(n)), h.max(p.dot(n))));
    let mut k = (lo / spacing).ceil() as i64;
    while (k as f64) * spacing <= hi && out.len() < 200_000 {
        let t = k as f64 * spacing;
        let mut hits = vec![];
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let (da, db) = (a.dot(n) - t, b.dot(n) - t);
            if (da > 0.0) != (db > 0.0) { hits.push(a + (b - a) * (da / (da - db))); }
        }
        if hits.len() >= 2 { out.push([hits[0], hits[1]]); }
        k += 1;
    }
}

/// Parameter interval of segment `p→q` inside the circle.
fn clip_interval(p: V2, q: V2, c: V2, r: f64) -> Option<(f64, f64)> {
    let d = q - p;
    let f = p - c;
    let a = d.dot(d);
    if a < 1e-18 { return ((p - c).length() <= r).then_some((0.0, 1.0)); }
    let b = 2.0 * f.dot(d);
    let cc = f.dot(f) - r * r;
    let disc = b * b - 4.0 * a * cc;
    if disc <= 0.0 { return None; }
    let s = disc.sqrt();
    let (t1, t2) = ((-b - s) / (2.0 * a), (-b + s) / (2.0 * a));
    let (l, h) = (t1.max(0.0), t2.min(1.0));
    (h > l).then_some((l, h))
}

fn clip_polylines(lines: &[Vec<V2>], c: V2, r: f64) -> Vec<Vec<V2>> {
    let mut out = vec![];
    for l in lines {
        let mut cur: Vec<V2> = vec![];
        for w in l.windows(2) {
            match clip_interval(w[0], w[1], c, r) {
                Some((t0, t1)) => {
                    let (a, b) = (w[0] + (w[1] - w[0]) * t0, w[0] + (w[1] - w[0]) * t1);
                    if cur.last().map(|p| (*p - a).length() > 1e-9).unwrap_or(true) {
                        if cur.len() >= 2 { out.push(std::mem::take(&mut cur)); } else { cur.clear(); }
                        cur.push(a);
                    }
                    cur.push(b);
                    if t1 < 1.0 - 1e-9 { if cur.len() >= 2 { out.push(std::mem::take(&mut cur)); } else { cur.clear(); } }
                }
                None => { if cur.len() >= 2 { out.push(std::mem::take(&mut cur)); } else { cur.clear(); } }
            }
        }
        if cur.len() >= 2 { out.push(cur); }
    }
    out
}

/// Model cut by `plane`, keeping the material behind it as seen with `w2v`, with the cut
/// faces hatched (spacing in model units).
fn section_geometry(model: &RegenResult, plane: &Frame, w2v: DQuat, axes: &[(V3, V3)], spacing: f64) -> (ViewLines, Vec<[V2; 2]>) {
    let toward = w2v.inverse() * V3::Z;
    let o = plane.origin;
    let fr = Frame::from_normal(o, toward);
    let l = model.topo.bbox.diagonal().max(1.0) * 2.0;
    let sq = builders::simple_loop(vec![V2::new(-l, -l), V2::new(l, -l), V2::new(l, l), V2::new(-l, l)]);
    let mut tags = builders::TagAllocator::default();
    let half = builders::extrude(&[ProfileRegion { outer: sq, holes: vec![] }], &fr, -l, 0.0, 0.0, &mut tags, 0);
    let mut bodies: std::collections::BTreeMap<u32, Vec<CPolygon>> = Default::default();
    for p in &model.polys { bodies.entry(p.tag >> BODY_SHIFT).or_default().push(p.clone()); }
    let mut all = vec![];
    let mut hatch = vec![];
    let tol = model.topo.bbox.diagonal().max(1.0) * 1e-6;
    for (k, (body, polys)) in bodies.iter().enumerate() {
        let mut cut = csg::intersect(polys, &half);
        let angle = [45f64, 135.0, 60.0, 120.0, 30.0, 150.0][k % 6].to_radians();
        for p in &mut cut {
            if p.tag >> BODY_SHIFT == 0 { p.tag |= body << BODY_SHIFT; }
            if p.plane.n.dot(toward) > 0.999 && p.v.iter().all(|v| (v.p - o).dot(toward).abs() < tol * 10.0) {
                let poly: Vec<V2> = p.v.iter().map(|v| (w2v * v.p).truncate()).collect();
                hatch_polygon(&poly, angle, spacing, &mut hatch);
            }
        }
        all.extend(cut);
    }
    let topo = Topology::new(&all);
    (hlr::project(&topo, axes, w2v), hatch)
}

fn detail_geometry(pg: &ViewGeom, d: &Detail, scale: f64, key: u64) -> ViewGeom {
    let (c, r) = (d.center, d.radius.max(1e-3));
    let lines = ViewLines {
        visible: clip_polylines(&pg.lines.visible, c, r),
        hidden: clip_polylines(&pg.lines.hidden, c, r),
        circles: pg.lines.circles.iter().copied().filter(|(p, _)| (*p - c).length() < r).collect(),
        axes: pg.lines.axes.iter().filter_map(|s| clip_interval(s[0], s[1], c, r).map(|(t0, t1)| [s[0] + (s[1] - s[0]) * t0, s[0] + (s[1] - s[0]) * t1])).collect(),
        min: c - V2::splat(r),
        max: c + V2::splat(r),
        chains: pg.lines.chains.iter().filter(|ch| ch.pts.iter().any(|p| (*p - c).length() < r)).cloned().collect(),
    };
    let hatch = pg.hatch.iter().filter_map(|s| clip_interval(s[0], s[1], c, r).map(|(t0, t1)| [s[0] + (s[1] - s[0]) * t0, s[0] + (s[1] - s[0]) * t1])).collect();
    ViewGeom { lines, hatch, c, scale, w2v: pg.w2v, clip: Some((c, r)), key, model: pg.model.clone() }
}

/// Re-resolves a created-dimension reference on the current model: (point, radius, found).
pub fn resolve_ref(r: &DimRef, topo: &Topology) -> (V3, f64, bool) {
    let Some(er) = r.edge else { return (r.point, r.radius, true) };
    let Some(ci) = topo.match_chain(&er) else { return (r.point, r.radius, false) };
    let ch = &topo.chains[ci];
    match r.at {
        RefAt::Start | RefAt::End => {
            let (s, e) = (ch.points[0], *ch.points.last().unwrap());
            // keep the end nearest the stored point (chains may reverse)
            let want = if (s - r.point).length() <= (e - r.point).length() { s } else { e };
            (want, 0.0, true)
        }
        RefAt::Center => match hlr::chain_circle(&ch.points) { Some((c, _, rad)) => (c, rad, true), None => (r.point, r.radius, false) },
        RefAt::Edge => { let (p, _) = edge_at(&ch.points, r.t, r.point); (p, 0.0, true) }
    }
}

/// Point and tangent at arc-length fraction `t` of a polyline; `near` picks the sense
/// (the chain may come back reversed after the model changes).
pub fn edge_at(pts: &[V3], t: f64, near: V3) -> (V3, V3) {
    let at = |t: f64| -> (V3, V3) {
        let total: f64 = pts.windows(2).map(|w| (w[1] - w[0]).length()).sum();
        if pts.len() < 2 || total <= 0.0 { return (pts.first().copied().unwrap_or(near), V3::X); }
        let mut want = t.clamp(0.0, 1.0) * total;
        for w in pts.windows(2) {
            let l = (w[1] - w[0]).length();
            if want <= l || std::ptr::eq(w.as_ptr(), pts[pts.len() - 2..].as_ptr()) { let f = if l > 0.0 { (want / l).min(1.0) } else { 0.0 }; return (w[0] + (w[1] - w[0]) * f, safe_norm(w[1] - w[0], V3::X)); }
            want -= l;
        }
        (*pts.last().unwrap(), V3::X)
    };
    let (a, b) = (at(t), at(1.0 - t));
    if (a.0 - near).length() <= (b.0 - near).length() { a } else { b }
}

/// Resolves an annotation reference: point and tangent of its edge.
pub fn resolve_edge(r: &DimRef, topo: &Topology) -> (V3, V3, bool) {
    let (p, tan, _, ok) = resolve_edge_n(r, topo);
    (p, tan, ok)
}

/// As `resolve_edge`, also giving the normals of the two faces meeting at the edge.
pub fn resolve_edge_n(r: &DimRef, topo: &Topology) -> (V3, V3, [V3; 2], bool) {
    let Some(ci) = r.edge.and_then(|e| topo.match_chain(&e)) else { return (r.point, V3::X, [V3::ZERO; 2], r.edge.is_none()) };
    let ch = &topo.chains[ci];
    let (p, tan) = edge_at(&ch.points, r.t, r.point);
    let k = (0..ch.points.len()).min_by(|a, b| (ch.points[*a] - p).length().partial_cmp(&(ch.points[*b] - p).length()).unwrap()).unwrap_or(0);
    let normals = [ch.n_a.get(k).copied().unwrap_or(V3::ZERO), ch.n_b.get(k).copied().unwrap_or(V3::ZERO)];
    match r.at { RefAt::Edge => (p, tan, normals, true), _ => { let (q, _, ok) = resolve_ref(r, topo); (q, tan, normals, ok) } }
}

/// Outward direction (sheet) at an edge seen in a view: the projection of the adjacent face
/// that is seen edge-on; falls back to `fallback`.
fn outward(w2v: DQuat, normals: [V3; 2], fallback: V2) -> V2 {
    let (a, b) = ((w2v * normals[0]).truncate(), (w2v * normals[1]).truncate());
    let m = if a.length() >= b.length() { a } else { b };
    if m.length() > 0.3 { m.normalize() } else { fallback }
}

/// Dimension text: the value, optional stacked tolerance pair, optionally boxed (basic).
#[derive(Clone, Debug, Default)]
pub struct DimLabel { pub main: String, pub stack: Option<(String, String)>, pub stack_h: f64, pub boxed: bool }

impl DimLabel {
    pub fn plain(s: &str) -> DimLabel { DimLabel { main: s.into(), stack: None, stack_h: 0.7, boxed: false } }
    /// Width and height for text height `h`.
    pub fn size(&self, h: f64) -> (f64, f64) {
        let mw = text_width(&self.main, h);
        let sh = h * self.stack_h;
        let sw = self.stack.as_ref().map(|(a, b)| text_width(a, sh).max(text_width(b, sh))).unwrap_or(0.0);
        let gap = if !self.main.is_empty() && self.stack.is_some() { 0.8 } else { 0.0 };
        (mw + gap + sw, if self.stack.is_some() { (sh * 2.0 + h * 0.3).max(h) } else { h })
    }
    /// Draws the label with its lower-left corner at `o`, rotated by `angle`.
    pub fn draw(&self, prims: &mut Vec<Prim>, o: V2, angle: f64, h: f64, c: Rgb) {
        let (_, th) = self.size(h);
        let at = |x: f64, y: f64| o + rot2(V2::new(x, y), angle);
        let main_y = (th - h) / 2.0;
        let mw = text_width(&self.main, h);
        if !self.main.is_empty() { prims.push(Prim::Text { pos: at(0.0, main_y), text: self.main.clone(), h, angle, halign: -1, valign: -1, c }); }
        if self.boxed {
            let b = [at(-0.8, main_y - 0.8), at(mw + 0.8, main_y - 0.8), at(mw + 0.8, main_y + h + 0.8), at(-0.8, main_y + h + 0.8)];
            prims.push(Prim::Line { pts: vec![b[0], b[1], b[2], b[3], b[0]], w: 0.25, c, dash: Dash::Solid });
        }
        if let Some((up, lo)) = &self.stack {
            let sh = h * self.stack_h;
            let x = if self.main.is_empty() { 0.0 } else { mw + 0.8 };
            prims.push(Prim::Text { pos: at(x, 0.0), text: lo.clone(), h: sh, angle, halign: -1, valign: -1, c });
            prims.push(Prim::Text { pos: at(x, sh + h * 0.3), text: up.clone(), h: sh, angle, halign: -1, valign: -1, c });
        }
    }
}

/// Dimension label for a value and its tolerance.
pub fn tol_label(d: &DrawingData, base: String, value: f64, prefix: &str, tol: &Tolerance) -> DimLabel {
    let mut l = DimLabel::plain(&base);
    match tol.mode {
        TolMode::Nominal => {}
        TolMode::Reference => l.main = format!("({})", base),
        TolMode::Basic => l.boxed = true,
        TolMode::Symmetric => l.main = format!("{} ±{}", base, d.fmt_tol(tol.upper.abs())),
        TolMode::PlusMinus => l.stack = Some((d.fmt_dev(tol.upper), d.fmt_dev(tol.lower))),
        TolMode::Fit => {
            l.main = format!("{} {}", base, tol.fit.trim());
            if let Some((u, lo)) = tol.deviations(value) { l.stack = Some((d.fmt_dev(u), d.fmt_dev(lo))); }
        }
        TolMode::Limits => {
            let f = |v: f64| format!("{}{:.*}", prefix, d.tol_decimals.max(d.decimals), v);
            l.main = String::new();
            l.stack = Some((f(value + tol.upper), f(value + tol.lower)));
            l.stack_h = 0.85;
        }
    }
    l
}

/// The 14 geometric characteristic symbols, drawn in a cell centred on `c` of size `s`.
pub fn gsymbol(prims: &mut Vec<Prim>, c: V2, s: f64, ch: GChar, col: Rgb) {
    let u = s * 0.28;
    let w = 0.3;
    let mut line = |pts: Vec<V2>| prims.push(Prim::Line { pts, w, c: col, dash: Dash::Solid });
    let p = |x: f64, y: f64| c + V2::new(x * u, y * u);
    match ch {
        GChar::Straightness => line(vec![p(-1.0, 0.0), p(1.0, 0.0)]),
        GChar::Flatness => line(vec![p(-1.0, -0.5), p(0.55, -0.5), p(1.0, 0.5), p(-0.55, 0.5), p(-1.0, -0.5)]),
        GChar::Circularity => prims.push(Prim::Circle { c, r: 0.85 * u, w, col, dash: Dash::Solid, fill: false }),
        GChar::Cylindricity => {
            prims.push(Prim::Circle { c, r: 0.65 * u, w, col, dash: Dash::Solid, fill: false });
            prims.push(Prim::Line { pts: vec![p(-1.15, -0.9), p(-0.25, 0.95)], w, c: col, dash: Dash::Solid });
            prims.push(Prim::Line { pts: vec![p(0.25, -0.95), p(1.15, 0.9)], w, c: col, dash: Dash::Solid });
        }
        GChar::LineProfile | GChar::SurfaceProfile => {
            let arc: Vec<V2> = (0..=24).map(|k| { let t = std::f64::consts::PI * k as f64 / 24.0; p(t.cos(), t.sin() - 0.45) }).collect();
            let closed = ch == GChar::SurfaceProfile;
            let mut pts = arc;
            if closed { pts.push(pts[0]); }
            prims.push(Prim::Line { pts, w, c: col, dash: Dash::Solid });
        }
        GChar::Angularity => line(vec![p(0.85, 0.8), p(-1.0, -0.65), p(1.0, -0.65)]),
        GChar::Perpendicularity => { line(vec![p(-1.0, -0.8), p(1.0, -0.8)]); prims.push(Prim::Line { pts: vec![p(0.0, -0.8), p(0.0, 0.95)], w, c: col, dash: Dash::Solid }); }
        GChar::Parallelism => { line(vec![p(-0.95, -0.8), p(-0.15, 0.85)]); prims.push(Prim::Line { pts: vec![p(0.15, -0.85), p(0.95, 0.8)], w, c: col, dash: Dash::Solid }); }
        GChar::Position => {
            prims.push(Prim::Circle { c, r: 0.6 * u, w, col, dash: Dash::Solid, fill: false });
            prims.push(Prim::Line { pts: vec![p(-1.05, 0.0), p(1.05, 0.0)], w, c: col, dash: Dash::Solid });
            prims.push(Prim::Line { pts: vec![p(0.0, -1.05), p(0.0, 1.05)], w, c: col, dash: Dash::Solid });
        }
        GChar::Concentricity => {
            prims.push(Prim::Circle { c, r: 0.4 * u, w, col, dash: Dash::Solid, fill: false });
            prims.push(Prim::Circle { c, r: 0.9 * u, w, col, dash: Dash::Solid, fill: false });
        }
        GChar::Symmetry => {
            line(vec![p(-1.0, 0.0), p(1.0, 0.0)]);
            prims.push(Prim::Line { pts: vec![p(-0.6, 0.5), p(0.6, 0.5)], w, c: col, dash: Dash::Solid });
            prims.push(Prim::Line { pts: vec![p(-0.6, -0.5), p(0.6, -0.5)], w, c: col, dash: Dash::Solid });
        }
        GChar::CircularRunout | GChar::TotalRunout => {
            let runs: &[f64] = if ch == GChar::TotalRunout { &[-0.45, 0.45] } else { &[0.0] };
            for &dx in runs {
                let (a, b) = (p(dx - 0.55, -0.9), p(dx + 0.45, 0.8));
                prims.push(Prim::Line { pts: vec![a, b], w, c: col, dash: Dash::Solid });
                let d = safe_norm2(b - a, V2::Y);
                let nn = perp_left(d);
                prims.push(Prim::Fill { pts: vec![b + d * (u * 0.15), b - d * (u * 0.55) + nn * (u * 0.28), b - d * (u * 0.55) - nn * (u * 0.28)], c: col });
            }
            if ch == GChar::TotalRunout { prims.push(Prim::Line { pts: vec![p(-1.0, -0.9), p(0.9, -0.9)], w, c: col, dash: Dash::Solid }); }
        }
    }
}

fn mat_circle(prims: &mut Vec<Prim>, c: V2, h: f64, m: MatCond, col: Rgb) -> f64 {
    let Some(letter) = m.letter() else { return 0.0 };
    prims.push(Prim::Circle { c, r: h * 0.48, w: 0.25, col, dash: Dash::Solid, fill: false });
    prims.push(Prim::Text { pos: c, text: letter.into(), h: h * 0.62, angle: 0.0, halign: 0, valign: 0, c: col });
    h * 1.1
}

/// Feature control frame with its lower-left corner at `o`; returns its box.
pub fn gtol_frame(prims: &mut Vec<Prim>, o: V2, g: &Gtol, d: &DrawingData, h: f64, c: Rgb) -> (V2, V2) {
    let hh = h * 2.0;
    let pad = h * 0.45;
    let mut x = o.x;
    let mut seps = vec![];
    let cy = o.y + hh / 2.0;
    gsymbol(prims, V2::new(x + hh / 2.0, cy), hh, g.ch, c);
    x += hh;
    seps.push(x);
    let tol = format!("{}{}", if g.diameter { "⌀" } else { "" }, d.fmt_tol(g.value));
    let tw = text_width(&tol, h);
    prims.push(Prim::Text { pos: V2::new(x + pad, cy), text: tol, h, angle: 0.0, halign: -1, valign: 0, c });
    x += pad + tw;
    if g.mat != MatCond::Rfs { x += h * 0.2; mat_circle(prims, V2::new(x + h * 0.48, cy), h, g.mat, c); x += h * 1.0; }
    x += pad;
    if !g.ch.form() {
        for (letter, m) in g.datums.iter().filter(|x| !x.0.trim().is_empty()) {
            seps.push(x);
            let lw = text_width(letter.trim(), h);
            let extra = if *m != MatCond::Rfs { h * 1.2 } else { 0.0 };
            let w = (lw + extra + 2.0 * pad).max(hh);
            let start = x + (w - lw - extra) / 2.0;
            prims.push(Prim::Text { pos: V2::new(start, cy), text: letter.trim().into(), h, angle: 0.0, halign: -1, valign: 0, c });
            if *m != MatCond::Rfs { mat_circle(prims, V2::new(start + lw + h * 0.68, cy), h, *m, c); }
            x += w;
        }
    }
    let hi = V2::new(x, o.y + hh);
    prims.push(Prim::Line { pts: vec![o, V2::new(hi.x, o.y), hi, V2::new(o.x, hi.y), o], w: 0.35, c, dash: Dash::Solid });
    for sx in seps { prims.push(Prim::Line { pts: vec![V2::new(sx, o.y), V2::new(sx, hi.y)], w: 0.35, c, dash: Dash::Solid }); }
    (o, hi)
}

/// Point on the frame/box outline nearest to `p` (leader start).
fn box_anchor(lo: V2, hi: V2, p: V2) -> V2 {
    let c = (lo + hi) * 0.5;
    let cands = [V2::new(lo.x, c.y), V2::new(hi.x, c.y), V2::new(c.x, lo.y), V2::new(c.x, hi.y)];
    cands.into_iter().min_by(|a, b| (*a - p).length().partial_cmp(&(*b - p).length()).unwrap()).unwrap()
}

impl App {
    pub fn drawing_active(&self) -> bool { self.drw.as_ref().map(|d| d.active).unwrap_or(false) }

    /// The model documented by the drawing, regenerated (exploded for exploded views).
    pub fn drawing_model(&mut self, exploded: bool) -> Arc<RegenResult> {
        let gen = self.result.generation;
        if let Some((g, m)) = self.drw.as_ref().and_then(|d| d.models.get(&exploded)) { if *g == gen { return m.clone(); } }
        let m = match self.asm.as_mut().filter(|a| a.editing.is_none()) {
            Some(a) if exploded || a.exploded => {
                let f = if exploded { if a.explode > 0.0 { a.explode } else { 0.6 } } else { 0.0 };
                Arc::new(am::regen(&a.data, &mut a.cache, &am::RegenOpts { explode: f, editing: None }).display)
            }
            _ => Arc::new(self.result.clone()),
        };
        if let Some(d) = &mut self.drw {
            if d.models.values().any(|(g, _)| *g != gen) { d.cache.clear(); d.models.clear(); }
            d.models.insert(exploded, (gen, m.clone()));
        }
        m
    }

    /// Geometry of a view (cached until the model or the view's projection changes).
    pub fn view_geom(&mut self, id: u32) -> Option<Arc<ViewGeom>> {
        let d = self.drw.as_ref()?;
        let si = d.data.sheet_of(id)?;
        let sheet = &d.data.sheets[si];
        let v = sheet.view(id)?.clone();
        let scale = sheet.view_scale(id);
        let parent = if v.kind == ViewKind::Detail { v.parent } else { None };
        let pg = match parent { Some(p) => Some(self.view_geom(p)?), None => None };
        let model = self.drawing_model(v.exploded);
        let key = hash_of(&format!("{}|{:?}|{:?}|{:?}|{}|{}|{}", model.generation, v.rot, v.section, v.detail, v.exploded, scale, pg.as_ref().map(|g| g.key).unwrap_or(0)));
        if let Some(g) = self.drw.as_ref()?.cache.get(&id) { if g.key == key { return Some(g.clone()); } }
        let g = if let (ViewKind::Detail, Some(det), Some(pg)) = (v.kind, &v.detail, &pg) {
            detail_geometry(pg, det, scale, key)
        } else {
            let w2v = v.world_to_view();
            let full = self.drawing_model(false).topo.bbox;
            let c = projected_centre(&full, w2v);
            let axes: Vec<(V3, V3)> = model.axes.iter().map(|a| (a.point, a.dir)).collect();
            let sec = v.section.and_then(|pid| model.plane(pid)).filter(|p| (w2v * p.frame.z()).z.abs() > 0.999).map(|p| p.frame);
            let (lines, hatch) = match sec {
                Some(fr) => section_geometry(&model, &fr, w2v, &axes, 3.0 / scale),
                None => (hlr::project(&model.topo, &axes, w2v), vec![]),
            };
            ViewGeom { lines, hatch, c, scale, w2v, clip: None, key, model: model.clone() }
        };
        let g = Arc::new(g);
        self.drw.as_mut()?.cache.insert(id, g.clone());
        Some(g)
    }

    // ---------- model dimensions ----------

    /// Dimensions of the part that a drawing can show (sketch dimensions, depths, holes, patterns).
    pub fn model_dims(&mut self) -> Vec<ModelDim> {
        let gen = self.result.generation;
        if let Some(d) = &self.drw { if d.dims_gen == gen { return d.dims.clone(); } }
        let out = if self.asm.as_ref().map(|a| a.editing.is_none()).unwrap_or(false) { vec![] } else { self.collect_model_dims() };
        if let Some(d) = &mut self.drw { d.dims = out.clone(); d.dims_gen = gen; }
        out
    }

    fn collect_model_dims(&self) -> Vec<ModelDim> {
        let r = &self.result;
        let vars = &r.vars;
        let b = r.topo.bbox;
        let size = if b.is_empty() { 50.0 } else { b.diagonal() };
        let mut out = vec![];
        for f in &self.data.features {
            if f.suppressed || r.rolled_back.contains(&f.id) || r.errors.contains_key(&f.id) { continue; }
            if let FeatureKind::Pattern(d) = &f.kind {
                let v = |d: &Dim| d.value(vars, 0.0);
                if d.kind == PatternKind::Direction {
                    // the leader's axis point (holes) or the centre of its geometry
                    let Some(&leader) = d.features.first() else { continue };
                    let axis = r.axes.iter().find(|a| a.feature == leader).map(|a| (a.point, a.dir));
                    let Some(src) = axis.map(|a| a.0).or_else(|| r.tools.get(&leader).map(|t| csg::bbox(&t.polys).center())) else { continue };
                    let dirv = |a: &AxisRef| match a { AxisRef::WorldX => V3::X, AxisRef::WorldY => V3::Y, AxisRef::WorldZ => V3::Z, AxisRef::Axis(id) => r.axis(*id).map(|x| x.dir).unwrap_or(V3::X) };
                    let mut dirs = vec![("spacing1", dirv(&d.dir1), &d.spacing1, &d.count1)];
                    if d.use_dir2 { dirs.push(("spacing2", dirv(&d.dir2), &d.spacing2, &d.count2)); }
                    for (k, dir, sp, n) in dirs {
                        if v(n) < 2.0 { continue; }
                        let e = src + dir * v(sp);
                        out.push(ModelDim { key: format!("{}:f:{}", f.id, k), feature: f.id, dk: Some(DimKey::Field(k)), geom: MGeom::Linear { a: src, b: e, dir, plane: axis.map(|a| a.1), label: (src + e) * 0.5 }, value: v(sp), prefix: "", text: None, args: vec![], edit: sp.0.clone() });
                    }
                }
                continue;
            }
            let Some(fr) = r.frames.get(&f.id).copied() else { continue };
            let sketches: Vec<(usize, Sketch, Frame)> = match &f.kind {
                FeatureKind::Extrude(d) => vec![(0, d.sketch.clone(), fr)],
                FeatureKind::Revolve(d) => vec![(0, d.sketch.clone(), fr)],
                FeatureKind::Sketch(d) => vec![(0, d.sketch.clone(), fr)],
                FeatureKind::Sweep(d) => vec![(0, d.trajectory.clone(), fr)],
                FeatureKind::Blend(d) => {
                    let offs: Vec<f64> = d.offsets.iter().map(|o| o.value(vars, 20.0)).collect();
                    let frames = regen::blend_frames(&fr, &offs, d.flip);
                    d.sections.iter().enumerate().filter_map(|(i, s)| frames.get(i).map(|f2| (i, s.clone(), *f2))).collect()
                }
                _ => vec![],
            };
            for (slot, raw, frame) in sketches {
                let sk = prepared(&raw, vars);
                for c in sk.constraints.iter().filter(|c| c.ty.is_dimension()) {
                    let key = format!("{}:s{}:{:?}:{:?}:{:?}", f.id, slot, c.ty, c.pts, c.ents);
                    let edit = c.expr.clone().unwrap_or_else(|| fmt_num(c.value, 4));
                    let lp = super::sketcher::dim_anchor(&sk, c) + c.label;
                    let geom = match c.ty {
                        SkCType::Distance | SkCType::HDistance | SkCType::VDistance | SkCType::PointLine => {
                            let (p1, p2) = if c.ty == SkCType::PointLine {
                                let p = sk.point(c.pts[0]);
                                let Some((a, bb)) = sk.line_ends(c.ents[0]) else { continue };
                                let dd = safe_norm2(bb - a, V2::X);
                                (p, a + dd * (p - a).dot(dd))
                            } else { (sk.point(c.pts[0]), sk.point(c.pts[1])) };
                            let u = match c.ty { SkCType::HDistance => V2::X, SkCType::VDistance => V2::Y, _ => safe_norm2(p2 - p1, V2::X) };
                            MGeom::Linear { a: frame.to_world(p1), b: frame.to_world(p2), dir: frame.dir_to_world(u), plane: Some(frame.z()), label: frame.to_world(lp) }
                        }
                        SkCType::Radius | SkCType::Diameter => {
                            let Some(e) = c.ents.first().and_then(|id| sk.entity(*id)) else { continue };
                            MGeom::Radial { c: frame.to_world(sk.point(e.pts[0])), r: sk.radius(e), normal: frame.z(), label: frame.to_world(lp), diameter: c.ty == SkCType::Diameter }
                        }
                        _ => continue,
                    };
                    let prefix = match c.ty { SkCType::Radius => "R", SkCType::Diameter => "⌀", _ => "" };
                    out.push(ModelDim { key, feature: f.id, dk: Some(DimKey::Sketch(slot, c.id, c.key())), geom, value: c.value.abs(), prefix, text: None, args: vec![], edit });
                }
            }
            let v = |d: &Dim| d.value(vars, 0.0);
            match &f.kind {
                FeatureKind::Extrude(d) if matches!(d.depth_type, DepthType::Blind | DepthType::Symmetric) => {
                    let sk = prepared(&d.sketch, vars);
                    let (mn, mx) = sk.bounds().unwrap_or((V2::ZERO, V2::ZERO));
                    let n = fr.z() * if d.flip { -1.0 } else { 1.0 };
                    let base = fr.to_world(V2::new(mx.x, mn.y));
                    let depth = v(&d.depth);
                    let (a, bb) = if d.depth_type == DepthType::Symmetric { (base - n * (depth / 2.0), base + n * (depth / 2.0)) } else { (base, base + n * depth) };
                    out.push(ModelDim { key: format!("{}:f:depth", f.id), feature: f.id, dk: Some(DimKey::Field("depth")), geom: MGeom::Linear { a, b: bb, dir: n, plane: None, label: (a + bb) * 0.5 + fr.x * (size * 0.08) }, value: depth, prefix: "", text: None, args: vec![], edit: d.depth.0.clone() });
                    if d.side2 == Side2::Blind && d.depth_type != DepthType::Symmetric {
                        let e = base - n * v(&d.depth2);
                        out.push(ModelDim { key: format!("{}:f:depth2", f.id), feature: f.id, dk: Some(DimKey::Field("depth2")), geom: MGeom::Linear { a: base, b: e, dir: -n, plane: None, label: (base + e) * 0.5 + fr.x * (size * 0.08) }, value: v(&d.depth2), prefix: "", text: None, args: vec![], edit: d.depth2.0.clone() });
                    }
                }
                FeatureKind::Hole(d) => {
                    let c = fr.to_world(V2::new(v(&d.x), v(&d.y)));
                    let drill = regen::hole_drill(d, vars).unwrap_or(0.0);
                    let r = drill / 2.0;
                    let std = d.ty == HoleType::Standard;
                    let depth = if d.through_all { " DURCHG.".to_string() } else { " x #".to_string() };
                    let (text, args) = match d.ty {
                        HoleType::Standard => match crate::model::threads::find(d.standard, &d.size) {
                            Some(s) if d.tapped => (Some(format!("{}{}", crate::model::threads::callout(d.standard, s), if d.through_all { " DURCHG." } else { "" })), vec![]),
                            _ => (Some(format!("⌀#{} ({} DL)", depth, d.size)), vec![drill, v(&d.depth)]),
                        },
                        HoleType::Counterbore => (Some(format!("⌀#{}, ZYL.SENK. ⌀# x #", depth)), if d.through_all { vec![drill, v(&d.cb_diameter), v(&d.cb_depth)] } else { vec![drill, v(&d.depth), v(&d.cb_diameter), v(&d.cb_depth)] }),
                        HoleType::Countersink => (Some(format!("⌀#{}, KEG.SENK. ⌀# x #°", depth)), if d.through_all { vec![drill, v(&d.cs_diameter), v(&d.cs_angle)] } else { vec![drill, v(&d.depth), v(&d.cs_diameter), v(&d.cs_angle)] }),
                        HoleType::Simple => (Some(format!("⌀#{}", depth)), vec![drill, v(&d.depth)]),
                    };
                    out.push(ModelDim {
                        key: format!("{}:f:diameter", f.id), feature: f.id, dk: if std { None } else { Some(DimKey::Field("diameter")) },
                        geom: MGeom::Radial { c, r, normal: fr.z(), label: c + fr.x * (r + size * 0.1) + fr.y * (size * 0.06), diameter: true },
                        value: drill, prefix: "⌀", text, args, edit: d.diameter.0.clone(),
                    });
                }
                _ => {}
            }
        }
        out
    }

    /// Shows every model dimension not yet shown in one of `views` (Creo's Show Model Annotations).
    pub fn show_model_dims(&mut self, views: &[u32]) -> usize {
        let dims = self.model_dims();
        let Some(si) = self.drw.as_ref().map(|d| d.sheet) else { return 0 };
        let mut geoms = vec![];
        for id in views { if let Some(g) = self.view_geom(*id) { geoms.push((*id, g)); } }
        let Some(d) = &self.drw else { return 0 };
        let shown: Vec<String> = d.data.sheets.iter().flat_map(|s| s.shown.iter().map(|x| x.key.clone())).collect();
        let sheet = &d.data.sheets[si];
        let mut add: Vec<ShownDim> = vec![];
        let mut stack: HashMap<(u32, i32, i64), usize> = HashMap::new();
        for md in dims.iter().filter(|m| !shown.contains(&m.key)) {
            // views showing the dimension in true length, preferring the sketch plane's view
            let mut best: Option<(u32, i32)> = None;
            for (id, g) in &geoms {
                let Some(v) = sheet.view(*id) else { continue };
                if v.kind == ViewKind::Detail { continue; }
                let z = g.w2v.inverse() * V3::Z;
                let rank = match md.geom {
                    MGeom::Linear { dir, plane, .. } => {
                        if dir.dot(z).abs() > 1e-6 { continue; }
                        if plane.map(|n| n.dot(z).abs() > 0.9999).unwrap_or(false) { 0 } else { 1 }
                    }
                    MGeom::Radial { normal, .. } => if normal.dot(z).abs() > 0.9999 { 0 } else { continue },
                };
                if best.map(|b| rank < b.1).unwrap_or(true) { best = Some((*id, rank)); }
            }
            let Some((vid, _)) = best else { continue };
            let (g, v) = (&geoms.iter().find(|x| x.0 == vid).unwrap().1, sheet.view(vid).unwrap());
            let label = auto_label(md, g, v.pos, &mut stack, vid);
            add.push(ShownDim { key: md.key.clone(), view: vid, label: Some(label - v.pos), tol: Default::default() });
        }
        let n = add.len();
        if n > 0 { self.drw_commit(|d| d.sheets[si].shown.extend(add)); }
        n
    }

    /// Re-places the shown dimensions of `views` outside the view outlines (Cleanup Dimensions).
    pub fn cleanup_dims(&mut self, views: &[u32]) {
        let dims = self.model_dims();
        let Some(si) = self.drw.as_ref().map(|d| d.sheet) else { return };
        let mut geoms = HashMap::new();
        for id in views { if let Some(g) = self.view_geom(*id) { geoms.insert(*id, g); } }
        let Some(d) = &self.drw else { return };
        let sheet = d.data.sheets[si].clone();
        let mut stack = HashMap::new();
        let mut labels = vec![];
        for (i, s) in sheet.shown.iter().enumerate() {
            let (Some(g), Some(v), Some(md)) = (geoms.get(&s.view), sheet.view(s.view), dims.iter().find(|m| m.key == s.key)) else { continue };
            labels.push((i, auto_label(md, g, v.pos, &mut stack, s.view) - v.pos));
        }
        self.drw_commit(|d| for (i, l) in labels { d.sheets[si].shown[i].label = Some(l); });
    }

    pub fn drw_commit(&mut self, f: impl FnOnce(&mut DrawingData)) {
        let Some(d) = &mut self.drw else { return };
        d.undo.push(d.data.clone());
        if d.undo.len() > 80 { d.undo.remove(0); }
        d.redo.clear();
        f(&mut d.data);
        d.dirty = true;
    }

    pub fn drw_undo(&mut self, redo: bool) {
        let Some(d) = &mut self.drw else { return };
        let (from, to) = if redo { (&mut d.redo, &mut d.undo) } else { (&mut d.undo, &mut d.redo) };
        let Some(prev) = from.pop() else { return };
        to.push(std::mem::replace(&mut d.data, prev));
        d.dirty = true;
        d.sel.clear();
        d.sheet = d.sheet.min(d.data.sheets.len() - 1);
    }

    // ---------- BOM ----------

    /// BOM rows (index, part, quantity, mass in kg) of the assembly in session.
    pub fn bom_rows(&self) -> Vec<(usize, String, usize, f64)> {
        let Some(a) = self.asm.as_ref().filter(|a| a.editing.is_none()) else { return vec![] };
        am::bom(&a.data, &a.res).into_iter().enumerate().map(|(i, r)| (i + 1, r.part, r.qty, r.mass)).collect()
    }

    /// Places index balloons around `view`, one per BOM part, attached to a visible surface.
    pub fn create_balloons(&mut self, view: u32) -> usize {
        let rows = self.bom_rows();
        if rows.is_empty() { return 0; }
        let Some(g) = self.view_geom(view) else { return 0 };
        let Some(d) = &self.drw else { return 0 };
        let Some(si) = d.data.sheet_of(view) else { return 0 };
        let Some(v) = d.data.sheets[si].view(view).cloned() else { return 0 };
        let Some(a) = self.asm.as_ref() else { return 0 };
        let model = &g.model;
        let t = &model.topo;
        let pr = hlr::Projector::new(t, g.w2v);
        let (rmin, rmax) = g.rect(v.pos);
        let centre = (rmin + rmax) * 0.5;
        let reach = (rmax - rmin).length() * 0.5 + 14.0;
        let mut out: Vec<Balloon> = vec![];
        for (_, part, _, _) in &rows {
            let comps: Vec<u64> = a.data.components.iter().filter(|c| &c.part == part && !c.suppressed && !c.hidden).map(|c| c.id).collect();
            // the largest visible face of any instance
            let mut best: Option<(f64, V3)> = None;
            for f in 0..t.faces.len() {
                let Some(owner) = model.tag_owner.get(&t.face_tag(f)) else { continue };
                if !comps.contains(owner) { continue; }
                let fr = t.surface_ref(f);
                let q = g.w2v * fr.point;
                if (g.w2v * fr.normal).z < 0.05 { continue; }
                if pr.hidden(q + V3::Z * (t.bbox.diagonal() * 1e-5), &t.faces[f].polys) { continue; }
                let area = t.faces[f].area * (g.w2v * fr.normal).z;
                if best.map(|b| area > b.0).unwrap_or(true) { best = Some((area, fr.point)); }
            }
            let Some((_, anchor)) = best else { continue };
            let s = g.world(v.pos, anchor);
            let dir = safe_norm2(s - centre, V2::new(1.0, 1.0).normalize());
            let mut pos = centre + dir * reach;
            // keep balloons apart
            for _ in 0..24 {
                if out.iter().all(|b| (b.pos + v.pos - pos).length() > 16.0) { break; }
                pos = centre + rot2(pos - centre, 0.22);
            }
            out.push(Balloon { view, part: part.clone(), anchor, pos: pos - v.pos });
        }
        let n = out.len();
        self.drw_commit(|d| {
            d.sheets[si].balloons.retain(|b| b.view != view);
            d.sheets[si].balloons.extend(out);
        });
        n
    }

    /// Inserts the BOM table above the title block.
    pub fn insert_bom(&mut self) -> bool {
        let rows = self.bom_rows();
        if rows.is_empty() { return false; }
        let Some(d) = &self.drw else { return false };
        let si = d.sheet;
        let sheet = &d.data.sheets[si];
        let cols = vec![BomCol::Index, BomCol::Part, BomCol::Qty, BomCol::Mass];
        let w: f64 = cols.iter().map(|c| c.width()).sum();
        let rh = d.data.text_height * 2.0;
        let tb = sheet.title_block();
        let pos = V2::new(tb.1.x - w, tb.1.y + rh * (rows.len() + 1) as f64);
        self.drw_commit(|d| d.sheets[si].bom = Some(BomTable { pos, columns: cols }));
        true
    }

    // ---------- scene ----------

    /// Vector primitives and clickable regions of a sheet. `hl` colours selected / hovered items.
    pub fn sheet_scene(&mut self, si: usize, hl: &[(DSel, Rgb)]) -> (Vec<Prim>, Vec<Hit>) {
        let mut prims = vec![];
        let mut hits = vec![];
        let Some(d) = &self.drw else { return (prims, hits) };
        let Some(sheet) = d.data.sheets.get(si).cloned() else { return (prims, hits) };
        let data = d.data.clone();
        let h = data.text_height;
        let colour = |s: DSel, base: Rgb| hl.iter().find(|x| x.0 == s).map(|x| x.1).unwrap_or(base);
        format_prims(&data, &sheet, si, &mut prims);
        let ids: Vec<u32> = sheet.views.iter().map(|v| v.id).collect();
        let mut geoms: HashMap<u32, Arc<ViewGeom>> = HashMap::new();
        for id in &ids { if let Some(g) = self.view_geom(*id) { geoms.insert(*id, g); } }
        let model_dims = self.model_dims();
        let bom = self.bom_rows();
        for v in &sheet.views {
            let Some(g) = geoms.get(&v.id) else { continue };
            let sel = DSel::View(v.id);
            let ink = colour(sel, INK);
            let to = |q: V2| g.to_sheet(v.pos, q);
            for l in &g.lines.visible { prims.push(Prim::Line { pts: l.iter().map(|p| to(*p)).collect(), w: 0.5, c: ink, dash: Dash::Solid }); }
            // section views omit the hidden lines behind the cut, as in ISO 128 practice
            let sectioned = v.section.is_some() || (v.kind == ViewKind::Detail && !g.hatch.is_empty());
            match v.style {
                LineStyle::Hidden if sectioned => {}
                LineStyle::Hidden => for l in &g.lines.hidden { prims.push(Prim::Line { pts: l.iter().map(|p| to(*p)).collect(), w: 0.3, c: if ink == INK { GREY } else { ink }, dash: Dash::Hidden }); },
                LineStyle::Wireframe => for l in &g.lines.hidden { prims.push(Prim::Line { pts: l.iter().map(|p| to(*p)).collect(), w: 0.5, c: ink, dash: Dash::Solid }); },
                LineStyle::NoHidden => {}
            }
            for s in &g.hatch { prims.push(Prim::Line { pts: vec![to(s[0]), to(s[1])], w: 0.18, c: ink, dash: Dash::Solid }); }
            if v.axes {
                for s in &g.lines.axes {
                    let (a, b) = (to(s[0]), to(s[1]));
                    let u = safe_norm2(b - a, V2::X);
                    prims.push(Prim::Line { pts: vec![a - u * 3.0, b + u * 3.0], w: 0.18, c: ink, dash: Dash::Center });
                }
                for (c, r) in &g.lines.circles {
                    let (cs, rs) = (to(*c), r * g.scale + 2.5);
                    prims.push(Prim::Line { pts: vec![cs - V2::X * rs, cs + V2::X * rs], w: 0.18, c: ink, dash: Dash::Center });
                    prims.push(Prim::Line { pts: vec![cs - V2::Y * rs, cs + V2::Y * rs], w: 0.18, c: ink, dash: Dash::Center });
                }
            }
            let (rmin, rmax) = g.rect(v.pos);
            hits.push(Hit { min: rmin - V2::splat(2.0), max: rmax + V2::splat(2.0), sel });
            // view label
            let mut label = vec![];
            if v.section.is_some() { label.push(format!("SCHNITT {}-{}", v.letter, v.letter)); }
            if v.kind == ViewKind::Detail { label.push(format!("EINZELHEIT {}", v.letter)); }
            let vs = sheet.view_scale(v.id);
            if (vs - sheet.scale).abs() > 1e-9 || v.kind == ViewKind::Detail { label.push(format!("MASSSTAB {}", scale_text(vs))); }
            for (k, t) in label.iter().enumerate() {
                prims.push(Prim::Text { pos: V2::new((rmin.x + rmax.x) * 0.5, rmin.y - 6.0 - k as f64 * h * 1.6), text: t.clone(), h, angle: 0.0, halign: 0, valign: 1, c: ink });
            }
        }
        // section cutting lines and detail circles in the parent views
        for v in &sheet.views {
            let (Some(pid), Some(g)) = (v.parent, geoms.get(&v.id)) else { continue };
            let (Some(pv), Some(pg)) = (sheet.view(pid), geoms.get(&pid)) else { continue };
            if let (Some(plane), ViewKind::Projection | ViewKind::General) = (v.section.and_then(|id| g.model.plane(id)), v.kind) {
                let n2 = (pg.w2v * plane.frame.z()).truncate();
                if n2.length() < 0.999 { continue; }
                let o2 = (pg.w2v * plane.frame.origin).truncate();
                let t = perp_left(n2);
                let (lo, hi) = (pg.lines.min, pg.lines.max);
                let corners = [lo, hi, V2::new(lo.x, hi.y), V2::new(hi.x, lo.y)];
                let (tmin, tmax) = corners.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), c| (a.min((*c - o2).dot(t)), b.max((*c - o2).dot(t))));
                let half = (tmax - tmin) * 0.5 + 6.0 / pg.scale;
                let mid = o2 + t * ((tmin + tmax) * 0.5);
                let (a, b) = (pg.to_sheet(pv.pos, mid - t * half), pg.to_sheet(pv.pos, mid + t * half));
                prims.push(Prim::Line { pts: vec![a, b], w: 0.5, c: INK, dash: Dash::Phantom });
                // arrows show the direction of sight (into the remaining material)
                let sight = -(pg.w2v * (g.w2v.inverse() * V3::Z)).truncate();
                let sd = safe_norm2(sight, -n2);
                for p in [a, b] {
                    let base = p + sd * 0.0;
                    let tip = base + sd * 8.0;
                    prims.push(Prim::Line { pts: vec![base, tip - sd * 2.5], w: 0.35, c: INK, dash: Dash::Solid });
                    arrow(&mut prims, tip, sd, h, INK);
                    prims.push(Prim::Text { pos: tip + sd * 2.5, text: v.letter.clone(), h, angle: 0.0, halign: 0, valign: 0, c: INK });
                }
            }
            if let (ViewKind::Detail, Some(det)) = (v.kind, &v.detail) {
                let c = pg.to_sheet(pv.pos, det.center);
                let r = det.radius * pg.scale;
                prims.push(Prim::Circle { c, r, w: 0.3, col: INK, dash: Dash::Phantom, fill: false });
                prims.push(Prim::Text { pos: c + V2::new(r, r) * 0.75 + V2::new(2.0, 2.0), text: v.letter.clone(), h, angle: 0.0, halign: -1, valign: -1, c: INK });
            }
        }
        // shown model dimensions
        let mut dim_boxes: Vec<(DimTarget, (V2, V2))> = vec![];
        for (i, s) in sheet.shown.iter().enumerate() {
            let (Some(v), Some(g), Some(md)) = (sheet.view(s.view), geoms.get(&s.view), model_dims.iter().find(|m| m.key == s.key)) else { continue };
            let sel = DSel::Shown(i);
            let c = colour(sel, INK);
            let lab = tol_label(&data, md.display(&data), md.value, md.prefix, &s.tol);
            let label = v.pos + s.label.unwrap_or(V2::ZERO);
            let r = match md.geom {
                MGeom::Linear { a, b, dir, .. } => {
                    let u = safe_norm2((g.w2v * dir).truncate(), V2::X);
                    linear_dim(&mut prims, g.world(v.pos, a), g.world(v.pos, b), u, label, &lab, h, c)
                }
                MGeom::Radial { c: cc, r, .. } => radial_dim(&mut prims, g.world(v.pos, cc), r * g.scale, label, &lab, h, c),
            };
            dim_boxes.push((DimTarget::Shown(s.key.clone()), r));
            hits.push(Hit { min: r.0, max: r.1, sel });
        }
        // created dimensions
        for dd in &sheet.dims {
            let (Some(v), Some(g)) = (sheet.view(dd.view), geoms.get(&dd.view)) else { continue };
            let sel = DSel::Dim(dd.id);
            let (pa, ra, oka) = resolve_ref(&dd.a, &g.model.topo);
            let (pb, _, okb) = dd.b.map(|b| resolve_ref(&b, &g.model.topo)).unwrap_or((pa, 0.0, true));
            let c = colour(sel, if oka && okb { INK } else { MISSING });
            let label = v.pos + dd.label;
            let (qa, qb) = (g.w2v * pa, g.w2v * pb);
            let (value, u) = match dd.ty {
                DimType::Horizontal => ((qb.x - qa.x).abs(), V2::X),
                DimType::Vertical => ((qb.y - qa.y).abs(), V2::Y),
                DimType::Aligned => ((qb - qa).truncate().length(), safe_norm2((qb - qa).truncate(), V2::X)),
                DimType::Radius => (ra, V2::X),
                DimType::Diameter => (ra * 2.0, V2::X),
            };
            let prefix = match dd.ty { DimType::Radius => "R", DimType::Diameter => "⌀", _ => "" };
            let measured = format!("{}{}", prefix, data.fmt(value));
            let text = dd.text.clone().map(|t| t.replace("@D", &measured)).unwrap_or(measured);
            let lab = tol_label(&data, text, value, prefix, &dd.tol);
            let r = match dd.ty {
                DimType::Radius | DimType::Diameter => radial_dim(&mut prims, g.world(v.pos, pa), ra * g.scale, label, &lab, h, c),
                _ => linear_dim(&mut prims, g.world(v.pos, pa), g.world(v.pos, pb), u, label, &lab, h, c),
            };
            dim_boxes.push((DimTarget::Drawn(dd.id), r));
            hits.push(Hit { min: r.0, max: r.1, sel });
        }
        // feature control frames: on a dimension (below its text) or with a leader to geometry
        for gt in &sheet.gtols {
            let (Some(v), Some(g)) = (sheet.view(gt.view), geoms.get(&gt.view)) else { continue };
            let sel = DSel::Gtol(gt.id);
            let mut c = colour(sel, INK);
            let r = match &gt.target {
                Attach::Dim(t) => {
                    let Some((_, b)) = dim_boxes.iter().find(|x| &x.0 == t) else { continue };
                    gtol_frame(&mut prims, V2::new(b.0.x + 1.0, b.0.y - h * 2.0 - 0.5), gt, &data, h, c)
                }
                Attach::Geom(at) => {
                    let (p, _, ok) = resolve_edge(at, &g.model.topo);
                    if !ok && c == INK { c = MISSING; }
                    let r = gtol_frame(&mut prims, v.pos + gt.pos, gt, &data, h, c);
                    let a = g.world(v.pos, p);
                    let s = box_anchor(r.0, r.1, a);
                    // leader leaves the frame horizontally, then bends to the feature
                    let knee = if (s.y - (r.0.y + r.1.y) * 0.5).abs() < 1e-9 { s + V2::X * if a.x > s.x { 4.0 } else { -4.0 } } else { s };
                    prims.push(Prim::Line { pts: vec![s, knee, a], w: 0.25, c, dash: Dash::Solid });
                    arrow(&mut prims, a, a - knee, h, c);
                    r
                }
            };
            hits.push(Hit { min: r.0, max: r.1, sel });
        }
        // datum feature symbols
        for ds in &sheet.datums {
            let (Some(v), Some(g)) = (sheet.view(ds.view), geoms.get(&ds.view)) else { continue };
            let sel = DSel::Datum(ds.id);
            let (p, tan, ok) = resolve_edge(&ds.at, &g.model.topo);
            let c = colour(sel, if ok { INK } else { MISSING });
            let a = g.world(v.pos, p);
            let bc = v.pos + ds.pos;
            let half = h * 1.0;
            let (lo, hi) = (bc - V2::splat(half), bc + V2::splat(half));
            prims.push(Prim::Line { pts: vec![lo, V2::new(hi.x, lo.y), hi, V2::new(lo.x, hi.y), lo], w: 0.35, c, dash: Dash::Solid });
            prims.push(Prim::Text { pos: bc, text: ds.letter.clone(), h, angle: 0.0, halign: 0, valign: 0, c });
            // filled triangle standing on the edge, pointing to the box
            let t2 = safe_norm2((g.w2v * tan).truncate(), V2::X);
            let mut n = perp_left(t2);
            if (bc - a).dot(n) < 0.0 { n = -n; }
            let (tw, th) = (h * 0.75, h * 0.9);
            let apex = a + n * th;
            prims.push(Prim::Fill { pts: vec![a - t2 * tw, a + t2 * tw, apex], c });
            let s = box_anchor(lo, hi, apex);
            prims.push(Prim::Line { pts: vec![apex, s], w: 0.25, c, dash: Dash::Solid });
            hits.push(Hit { min: lo.min(a - V2::splat(tw)), max: hi.max(a + V2::splat(tw)), sel });
        }
        // surface texture symbols
        for f in &sheet.finishes {
            let (Some(v), Some(g)) = (sheet.view(f.view), geoms.get(&f.view)) else { continue };
            let sel = DSel::Finish(f.id);
            let (p, tan, normals, ok) = resolve_edge_n(&f.at, &g.model.topo);
            let c = colour(sel, if ok { INK } else { MISSING });
            let a = g.world(v.pos, p);
            let t2 = safe_norm2((g.w2v * tan).truncate(), V2::X);
            let (vlo, vhi) = g.rect(v.pos);
            let mut n = perp_left(t2);
            if (a - (vlo + vhi) * 0.5).dot(n) < 0.0 { n = -n; }
            let o = outward(g.w2v, normals, n);
            n = if o.dot(n) >= 0.0 { n } else { -n };
            if f.flip { n = -n; }
            let x = V2::new(n.y, -n.x);
            let at = |lx: f64, ly: f64| a + x * lx + n * ly;
            let (h1, h2) = (h * 1.4, h * 2.8);
            let k = 0.577;
            prims.push(Prim::Line { pts: vec![at(-k * h1, h1), at(0.0, 0.0), at(k * h2, h2)], w: 0.3, c, dash: Dash::Solid });
            match f.kind {
                FinishKind::Removal => prims.push(Prim::Line { pts: vec![at(-k * h1, h1), at(k * h1, h1)], w: 0.3, c, dash: Dash::Solid }),
                FinishKind::NoRemoval => prims.push(Prim::Circle { c: at(0.0, h1 * 0.56), r: h1 * 0.3, w: 0.3, col: c, dash: Dash::Solid, fill: false }),
                FinishKind::Any => {}
            }
            let tw = text_width(&f.value, h).max(text_width(&f.method, h)) + 1.5;
            if !f.value.is_empty() || !f.method.is_empty() {
                prims.push(Prim::Line { pts: vec![at(k * h2, h2), at(k * h2 + tw, h2)], w: 0.3, c, dash: Dash::Solid });
            }
            let mut ang = x.y.atan2(x.x);
            let flipped = ang > std::f64::consts::FRAC_PI_2 + 1e-6 || ang < -std::f64::consts::FRAC_PI_2 - 1e-6;
            if flipped { ang += std::f64::consts::PI; }
            let (ha, sgn) = if flipped { (1i8, -1.0) } else { (-1i8, 1.0) };
            let _ = sgn;
            if !f.value.is_empty() { prims.push(Prim::Text { pos: at(k * h2 * 0.92 + 0.8, h2 - 1.0), text: f.value.clone(), h, angle: ang, halign: ha, valign: if flipped { -1 } else { 1 }, c }); }
            if !f.method.is_empty() { prims.push(Prim::Text { pos: at(k * h2 + 0.8, h2 + 0.8), text: f.method.clone(), h, angle: ang, halign: ha, valign: if flipped { 1 } else { -1 }, c }); }
            let pts = [at(-k * h1, h1), a, at(k * h2 + tw, h2 + h * 1.3), at(k * h2 + tw, h2 - h * 1.3)];
            let lo = pts.iter().fold(V2::splat(f64::INFINITY), |m, q| m.min(*q));
            let hi = pts.iter().fold(V2::splat(f64::NEG_INFINITY), |m, q| m.max(*q));
            hits.push(Hit { min: lo, max: hi, sel });
        }
        // notes
        for n in &sheet.notes {
            let sel = DSel::Note(n.id);
            let c = colour(sel, INK);
            let lines: Vec<&str> = n.text.lines().collect();
            let mut w: f64 = 4.0;
            for (k, l) in lines.iter().enumerate() {
                prims.push(Prim::Text { pos: n.pos - V2::Y * (k as f64 * h * 1.6), text: l.to_string(), h, angle: 0.0, halign: -1, valign: 1, c });
                w = w.max(text_width(l, h));
            }
            if let Some(lp) = n.leader {
                prims.push(Prim::Line { pts: vec![n.pos - V2::new(1.0, h * 0.5), lp], w: 0.25, c, dash: Dash::Solid });
                arrow(&mut prims, lp, safe_norm2(lp - n.pos, V2::X), h, c);
            }
            let hh = lines.len().max(1) as f64 * h * 1.6;
            hits.push(Hit { min: n.pos - V2::new(1.0, hh), max: n.pos + V2::new(w + 1.0, 1.0), sel });
        }
        // balloons
        for (i, b) in sheet.balloons.iter().enumerate() {
            let (Some(v), Some(g)) = (sheet.view(b.view), geoms.get(&b.view)) else { continue };
            let Some(idx) = bom.iter().find(|r| r.1 == b.part).map(|r| r.0) else { continue };
            let sel = DSel::Balloon(i);
            let c = colour(sel, INK);
            let p = v.pos + b.pos;
            let a = g.world(v.pos, b.anchor);
            let rr = h * 2.0;
            let dir = safe_norm2(a - p, V2::X);
            prims.push(Prim::Line { pts: vec![p + dir * rr, a], w: 0.25, c, dash: Dash::Solid });
            prims.push(Prim::Circle { c: a, r: 0.6, w: 0.0, col: c, dash: Dash::Solid, fill: true });
            prims.push(Prim::Circle { c: p, r: rr, w: 0.35, col: c, dash: Dash::Solid, fill: false });
            prims.push(Prim::Text { pos: p, text: idx.to_string(), h, angle: 0.0, halign: 0, valign: 0, c });
            hits.push(Hit { min: p - V2::splat(rr), max: p + V2::splat(rr), sel });
        }
        // BOM table
        if let Some(t) = &sheet.bom {
            let c = colour(DSel::Bom, INK);
            let rh = h * 2.0;
            let w: f64 = t.columns.iter().map(|c| c.width()).sum();
            let n = bom.len() + 1;
            let (x0, y0) = (t.pos.x, t.pos.y);
            for k in 0..=n {
                let y = y0 - rh * k as f64;
                prims.push(Prim::Line { pts: vec![V2::new(x0, y), V2::new(x0 + w, y)], w: if k == 0 || k == n || k == 1 { 0.5 } else { 0.25 }, c, dash: Dash::Solid });
            }
            let mut x = x0;
            for (ci, col) in t.columns.iter().enumerate() {
                prims.push(Prim::Line { pts: vec![V2::new(x, y0), V2::new(x, y0 - rh * n as f64)], w: if ci == 0 { 0.5 } else { 0.25 }, c, dash: Dash::Solid });
                prims.push(Prim::Text { pos: V2::new(x + col.width() / 2.0, y0 - rh * 0.5), text: col.title().into(), h: h * 0.8, angle: 0.0, halign: 0, valign: 0, c });
                for (ri, row) in bom.iter().enumerate() {
                    let cell = match col {
                        BomCol::Index => row.0.to_string(),
                        BomCol::Part => format!("{}.PRT", row.1),
                        BomCol::Qty => row.2.to_string(),
                        BomCol::Mass => format!("{:.3}", row.3),
                        BomCol::File => String::new(),
                    };
                    let left = matches!(col, BomCol::Part | BomCol::File);
                    let px = if left { x + 2.0 } else { x + col.width() / 2.0 };
                    prims.push(Prim::Text { pos: V2::new(px, y0 - rh * (ri as f64 + 1.5)), text: cell, h, angle: 0.0, halign: if left { -1 } else { 0 }, valign: 0, c });
                }
                x += col.width();
            }
            prims.push(Prim::Line { pts: vec![V2::new(x0 + w, y0), V2::new(x0 + w, y0 - rh * n as f64)], w: 0.5, c, dash: Dash::Solid });
            hits.push(Hit { min: V2::new(x0, y0 - rh * n as f64), max: V2::new(x0 + w, y0), sel: DSel::Bom });
        }
        (prims, hits)
    }
}

fn arrow(prims: &mut Vec<Prim>, tip: V2, dir: V2, h: f64, c: Rgb) {
    let d = safe_norm2(dir, V2::X);
    let n = perp_left(d);
    let (l, w) = (h * 0.85, h * 0.18);
    prims.push(Prim::Fill { pts: vec![tip, tip - d * l + n * w, tip - d * l - n * w], c });
}

/// Linear dimension between sheet points measured along `u`; returns the text box.
pub fn linear_dim(prims: &mut Vec<Prim>, pa: V2, pb: V2, u: V2, label: V2, lab: &DimLabel, h: f64, c: Rgb) -> (V2, V2) {
    let n = perp_left(u);
    let (oa, ob) = ((label - pa).dot(n), (label - pb).dot(n));
    let (da, db) = (pa + n * oa, pb + n * ob);
    for (p, o) in [(pa, oa), (pb, ob)] {
        if o.abs() > 1.0 {
            let s = o.signum();
            prims.push(Prim::Line { pts: vec![p + n * s, p + n * (o + 2.0 * s)], w: 0.25, c, dash: Dash::Solid });
        }
    }
    // text along the line, extended to reach a label placed outside the extension lines
    let t = (label - da).dot(u);
    let span = (db - da).dot(u);
    let (lo, hi) = (span.min(0.0), span.max(0.0));
    let (tw, th) = lab.size(h);
    let mut ends = (da + u * lo, da + u * hi);
    if t - tw / 2.0 < lo { ends.0 = da + u * (t - tw / 2.0 - 1.0); }
    if t + tw / 2.0 > hi { ends.1 = da + u * (t + tw / 2.0 + 1.0); }
    prims.push(Prim::Line { pts: vec![ends.0, ends.1], w: 0.25, c, dash: Dash::Solid });
    if (db - da).length() > 1e-6 {
        arrow(prims, da, da - db, h, c);
        arrow(prims, db, db - da, h, c);
    }
    let mut ang = u.y.atan2(u.x);
    if ang > std::f64::consts::FRAC_PI_2 + 1e-6 { ang -= std::f64::consts::PI; }
    if ang < -std::f64::consts::FRAC_PI_2 + 1e-6 { ang += std::f64::consts::PI; }
    let up = rot2(V2::Y, ang);
    let tc = da + u * t + up * (h * 0.4 + if lab.boxed { 0.8 } else { 0.0 });
    lab.draw(prims, tc - rot2(V2::X, ang) * (tw / 2.0), ang, h, c);
    let corners = [V2::new(-tw / 2.0, 0.0), V2::new(tw / 2.0, 0.0), V2::new(tw / 2.0, th * 1.1), V2::new(-tw / 2.0, th * 1.1)].map(|q| tc + rot2(q, ang));
    let lo2 = corners.iter().fold(V2::splat(f64::INFINITY), |a, p| a.min(*p));
    let hi2 = corners.iter().fold(V2::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
    (lo2 - V2::splat(1.0), hi2 + V2::splat(1.0))
}

/// Radius / diameter dimension with a leader from the text to the circle.
pub fn radial_dim(prims: &mut Vec<Prim>, centre: V2, r: f64, label: V2, lab: &DimLabel, h: f64, c: Rgb) -> (V2, V2) {
    let dir = safe_norm2(label - centre, V2::new(1.0, 1.0).normalize());
    let edge = centre + dir * r;
    let right = label.x >= centre.x;
    let (tw, th) = lab.size(h);
    let shoulder = label + V2::X * if right { tw + 2.0 } else { -(tw + 2.0) };
    prims.push(Prim::Line { pts: vec![edge, label, shoulder], w: 0.25, c, dash: Dash::Solid });
    arrow(prims, edge, edge - label, h, c);
    let lo_x = label.x.min(shoulder.x) + 1.0;
    lab.draw(prims, V2::new(lo_x, label.y + h * 0.4 + if lab.boxed { 0.8 } else { 0.0 }), 0.0, h, c);
    (V2::new(label.x.min(shoulder.x) - 1.0, label.y - 1.0), V2::new(label.x.max(shoulder.x) + 1.0, label.y + h * 0.4 + th * 1.15))
}

/// Default placement of a shown dimension, outside the view outline, stacked per side.
fn auto_label(md: &ModelDim, g: &ViewGeom, pos: V2, stack: &mut HashMap<(u32, i32, i64), usize>, vid: u32) -> V2 {
    let (rmin, rmax) = g.rect(pos);
    match md.geom {
        MGeom::Linear { a, b, dir, label, .. } => {
            let (pa, pb) = (g.world(pos, a), g.world(pos, b));
            let mut u = safe_norm2((g.w2v * dir).truncate(), V2::X);
            if u.x < -1e-9 || (u.x.abs() <= 1e-9 && u.y < 0.0) { u = -u; }
            let n = perp_left(u);
            let hint = g.world(pos, label);
            let mid = (pa + pb) * 0.5;
            let side = if (hint - mid).dot(n) >= 0.0 { 1.0 } else { -1.0 };
            let corners = [rmin, rmax, V2::new(rmin.x, rmax.y), V2::new(rmax.x, rmin.y)];
            let ext = corners.iter().map(|c| (*c - pa).dot(n) * side).fold(0.0f64, f64::max);
            let key = (vid, side as i32, (u.x.abs() * 10.0).round() as i64);
            let level = stack.entry(key).or_insert(0);
            let off = ext + 10.0 + *level as f64 * 9.0;
            *level += 1;
            mid + n * (side * off)
        }
        MGeom::Radial { c, r, label, .. } => {
            let cs = g.world(pos, c);
            let d = safe_norm2(g.world(pos, label) - cs, V2::new(1.0, 1.0).normalize());
            let key = (vid, 9, 0);
            let level = stack.entry(key).or_insert(0);
            *level += 1;
            cs + d * (r * g.scale + 10.0 + (*level as f64 - 1.0) * 3.0)
        }
    }
}

/// Border, frame, zone markers and the title block.
fn format_prims(d: &DrawingData, sheet: &Sheet, si: usize, prims: &mut Vec<Prim>) {
    let (w, h) = sheet.dims_mm();
    let rect = |a: V2, b: V2| vec![a, V2::new(b.x, a.y), b, V2::new(a.x, b.y), a];
    prims.push(Prim::Line { pts: rect(V2::ZERO, V2::new(w, h)), w: 0.25, c: INK, dash: Dash::Solid });
    let (fmin, fmax) = sheet.frame();
    prims.push(Prim::Line { pts: rect(fmin, fmax), w: 0.7, c: INK, dash: Dash::Solid });
    // zones
    let nx = ((fmax.x - fmin.x) / 50.0).round().max(2.0) as usize;
    let ny = ((fmax.y - fmin.y) / 50.0).round().max(2.0) as usize;
    for i in 0..nx {
        let x0 = fmin.x + (fmax.x - fmin.x) * i as f64 / nx as f64;
        let xm = x0 + (fmax.x - fmin.x) / nx as f64 * 0.5;
        if i > 0 { for (ya, yb) in [(0.0, fmin.y), (fmax.y, h)] { prims.push(Prim::Line { pts: vec![V2::new(x0, ya), V2::new(x0, yb)], w: 0.25, c: INK, dash: Dash::Solid }); } }
        let label = (nx - i).to_string();
        for y in [fmin.y * 0.5, (fmax.y + h) * 0.5] { prims.push(Prim::Text { pos: V2::new(xm, y), text: label.clone(), h: 2.5, angle: 0.0, halign: 0, valign: 0, c: INK }); }
    }
    for j in 0..ny {
        let y0 = fmin.y + (fmax.y - fmin.y) * j as f64 / ny as f64;
        let ym = y0 + (fmax.y - fmin.y) / ny as f64 * 0.5;
        if j > 0 { for (xa, xb) in [(0.0, fmin.x), (fmax.x, w)] { prims.push(Prim::Line { pts: vec![V2::new(xa, y0), V2::new(xb, y0)], w: 0.25, c: INK, dash: Dash::Solid }); } }
        let label = ((b'A' + j as u8) as char).to_string();
        for x in [fmin.x * 0.5, (fmax.x + w) * 0.5] { prims.push(Prim::Text { pos: V2::new(x, ym), text: label.clone(), h: 2.5, angle: 0.0, halign: 0, valign: 0, c: INK }); }
    }
    // title block: 180 x 40 in the lower right corner
    let (tmin, tmax) = sheet.title_block();
    prims.push(Prim::Line { pts: rect(tmin, tmax), w: 0.7, c: INK, dash: Dash::Solid });
    let rows = [tmin.y + 26.0, tmin.y + 13.0];
    for y in rows { prims.push(Prim::Line { pts: vec![V2::new(tmin.x, y), V2::new(tmax.x, y)], w: 0.35, c: INK, dash: Dash::Solid }); }
    let cell = |prims: &mut Vec<Prim>, x0: f64, x1: f64, y0: f64, y1: f64, label: &str, value: &str, vh: f64| {
        prims.push(Prim::Line { pts: vec![V2::new(x0, y0), V2::new(x0, y1)], w: 0.35, c: INK, dash: Dash::Solid });
        prims.push(Prim::Text { pos: V2::new(x0 + 1.5, y1 - 1.2), text: label.into(), h: 1.8, angle: 0.0, halign: -1, valign: 1, c: INK });
        prims.push(Prim::Text { pos: V2::new((x0 + x1) * 0.5, (y0 + y1) * 0.5 - 1.2), text: value.into(), h: vh, angle: 0.0, halign: 0, valign: 0, c: INK });
    };
    let (x, y) = (tmin.x, tmin.y);
    // top row: company and title
    cell(prims, x, x + 60.0, rows[0], tmax.y, "FIRMA", &d.company, 4.0);
    cell(prims, x + 60.0, tmax.x, rows[0], tmax.y, "BENENNUNG", &d.title, 5.0);
    // middle row
    cell(prims, x, x + 60.0, rows[1], rows[0], "ZEICHNUNGSNR.", &d.name, 3.5);
    cell(prims, x + 60.0, x + 90.0, rows[1], rows[0], "MASSSTAB", &scale_text(sheet.scale), 3.5);
    cell(prims, x + 90.0, x + 110.0, rows[1], rows[0], "FORMAT", sheet.size.name(), 3.5);
    cell(prims, x + 110.0, x + 145.0, rows[1], rows[0], "BLATT", &format!("{} VON {}", si + 1, d.sheets.len()), 3.5);
    cell(prims, x + 145.0, tmax.x, rows[1], rows[0], "PROJEKTION", "", 3.5);
    // projection symbol (truncated cone)
    let pc = V2::new(x + 162.5, rows[1] + 5.0);
    let (cone, ring) = (V2::new(pc.x + if d.projection == Projection::Third { -7.0 } else { 3.0 }, pc.y), V2::new(pc.x + if d.projection == Projection::Third { 5.0 } else { -5.0 }, pc.y));
    prims.push(Prim::Line { pts: vec![cone + V2::new(0.0, -1.5), cone + V2::new(6.0, -3.0), cone + V2::new(6.0, 3.0), cone + V2::new(0.0, 1.5), cone + V2::new(0.0, -1.5)], w: 0.25, c: INK, dash: Dash::Solid });
    prims.push(Prim::Circle { c: ring, r: 3.0, w: 0.25, col: INK, dash: Dash::Solid, fill: false });
    prims.push(Prim::Circle { c: ring, r: 1.5, w: 0.25, col: INK, dash: Dash::Solid, fill: false });
    // bottom row
    cell(prims, x, x + 60.0, y, rows[1], "GEZEICHNET", &d.drawn_by, 3.5);
    cell(prims, x + 60.0, x + 110.0, y, rows[1], "DATUM", &d.date, 3.5);
    cell(prims, x + 110.0, x + 145.0, y, rows[1], "WERKSTOFF", &d.material, 3.0);
    cell(prims, x + 145.0, tmax.x, y, rows[1], "ALLGEMEINTOL.", &d.general_tol, 2.6);
}
