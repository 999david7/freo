//! Sketcher: drawing directly on the sketch plane inside the 3D graphics window (Creo style),
//! with snapping and auto-constraints, constraint and dimension tools, weak dimensions,
//! live solving, dragging and inline dimension editing.

use super::camera::Camera;
use super::{MsgKind, Slot};
use crate::kernel::math::*;
use crate::kernel::triangulate::triangulate;
use crate::model::expr::{self, Vars};
use crate::sketch::sketch::*;
use crate::sketch::sketch::catmull_rom;
use crate::sketch::solver;
use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke};

pub const GEOM: Color32 = Color32::from_rgb(24, 86, 224);
pub const CONSTRUCT: Color32 = Color32::from_rgb(105, 150, 225);
pub const CLINE: Color32 = Color32::from_rgb(40, 58, 120);
pub const SEL: Color32 = Color32::from_rgb(0, 178, 60);
pub const PRE: Color32 = Color32::from_rgb(0, 165, 255);
pub const STRONG: Color32 = Color32::from_rgb(24, 46, 150);
pub const WEAKC: Color32 = Color32::from_rgb(92, 150, 226);
pub const CONC: Color32 = Color32::from_rgb(36, 96, 210);
/// Sketch references: Creo draws them as magenta dashed lines.
pub const REFC: Color32 = Color32::from_rgb(214, 44, 214);
pub const RUBBER: Color32 = Color32::from_rgb(230, 130, 20);

const PICK: f32 = 8.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select, Line, Rect, CenterRect, Circle, Circle3, Arc3, ArcCenter, Centerline, Point, Polygon, Spline, Ellipse,
    Fillet, Chamfer, Trim, Divide, Corner, Mirror, Project, Dimension, Con(SkCType),
    LineTangent, CenterlineTangent, SlantRect, Parallelogram, CircleConc, Circle3T, Arc3T, ArcConc, Conic, EllipseAxis,
    FilletCirc, FilletEllip, FilletEllipTrim, ChamferKeep,
    /// Krümmungsstetige Verrundung ("Apple corner") in one of the smooth corner shapes
    FilletSmooth(crate::kernel::corner::CornerShape),
    /// Operationen ▸ Ersetzen: the entity whose dimensions and constraints move to the next pick
    Replace(i64),
    InfoIntersect, InfoTangent, InfoEntity,
    Text, Csys, Offset, Thicken, AddRef, RefDimension, Perimeter, Baseline,
    /// Skizzierer-Palette: the next click places the chosen shape
    Palette,
}

/// Value prompts and dialogs that a tool opens after its picks (Creo's message-area inputs).
#[derive(Clone, Debug)]
pub enum Pending {
    /// Versatz: entity or reference, the picked spot (arrow side) and the typed value
    Offset { src: i64, side: V2, value: String },
    /// Aufdicken: thickness, offset and end type (0 Offen, 1 Flach, 2 Kreisförmig)
    Thicken { src: i64, side: V2, thick: String, off: String, cap: u8 },
    Text { a: V2, b: V2, t: SkText },
}

/// A row of the Referenzen dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefRow { Axis(usize), Line(usize), Point(i64) }

/// "Drehen-Größe ändern" applied to the selection, live from the stored start positions.
#[derive(Clone, Debug)]
pub struct RotResize { pub pts: Vec<(i64, V2)>, pub radii: Vec<(i64, f64)>, pub pivot: V2, pub dx: f64, pub dy: f64, pub angle: f64, pub scale: f64 }

/// Selection filter of the sketcher (status bar: Alle Skizzierer / Skizzierer-Geometrie / Bemaßung / Bedingung).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkFilter { All, Geometry, Dimension, Constraint }

impl SkFilter {
    pub const ALL: [SkFilter; 4] = [SkFilter::All, SkFilter::Geometry, SkFilter::Dimension, SkFilter::Constraint];
    pub fn name(&self) -> &'static str {
        match self { SkFilter::All => "Alle Skizzierer", SkFilter::Geometry => "Skizzierer-Geometrie", SkFilter::Dimension => "Bemaßung", SkFilter::Constraint => "Bedingung" }
    }
    pub fn icon(&self) -> super::icons::Icon {
        use super::icons::Icon;
        match self { SkFilter::All => Icon::SelAll, SkFilter::Geometry => Icon::SkLine, SkFilter::Dimension => Icon::SkDimension, SkFilter::Constraint => Icon::ConPerp }
    }
}

impl Tool {
    pub fn hint(&self) -> &'static str {
        match self {
            Tool::Select => "Elemente wählen, Geometrie oder Bemaßungen ziehen; Bemaßung doppelklicken, um sie zu ändern.",
            Tool::Line => "Den Anfang der Linie wählen. Mittel-/Rechtsklick oder Esc beendet die Kette.",
            Tool::Rect => "Zwei gegenüberliegende Ecken des Rechtecks wählen.",
            Tool::CenterRect => "Den Mittelpunkt, dann eine Ecke des Rechtecks wählen.",
            Tool::Circle => "Den Mittelpunkt, dann einen Punkt auf dem Kreis wählen.",
            Tool::Circle3 => "Drei Punkte auf dem Kreis wählen.",
            Tool::Arc3 => "Den Anfang, das Ende, dann einen Punkt auf dem Bogen wählen.",
            Tool::ArcCenter => "Den Mittelpunkt, den Anfang, dann das Ende wählen (gegen den Uhrzeigersinn).",
            Tool::Centerline => "Zwei Punkte der Mittellinie wählen.",
            Tool::Point => "Eine Position für den Punkt wählen.",
            Tool::Polygon => "Den Mittelpunkt, dann einen Eckpunkt der Form wählen.",
            Tool::Palette => "Eine Position für die Form aus der Palette wählen.",
            Tool::Spline => "Splinepunkte wählen; letzten Punkt erneut klicken, Mittel-/Rechtsklick oder Esc zum Beenden.",
            Tool::Ellipse => "Den Mittelpunkt, das Ende der Hauptachse, dann einen Punkt der Nebenachse wählen.",
            Tool::Fillet => "Eine Ecke zweier Linien wählen.",
            Tool::Chamfer => "Eine Ecke zweier Linien wählen.",
            Tool::Trim => "Das zu löschende Segment wählen; es wird an seinen Schnittpunkten getrimmt.",
            Tool::Divide => "Die Stelle auf einem Element wählen, an der es geteilt wird.",
            Tool::Corner => "Zwei Linien wählen, um sie zu einer Ecke zu trimmen oder zu verlängern.",
            Tool::Mirror => "Eine Mittellinie wählen, an der die gewählten Elemente gespiegelt werden.",
            Tool::Project => "Eine Referenzkante wählen, um sie in die Skizze zu projizieren.",
            Tool::Dimension => "Elemente wählen, dann klicken, um die Bemaßung zu platzieren.",
            Tool::LineTangent => "Zwei Kreise oder Bögen wählen; die Linie wird an beide tangential gelegt.",
            Tool::CenterlineTangent => "Zwei Kreise oder Bögen wählen; die Mittellinie wird an beide tangential gelegt.",
            Tool::SlantRect => "Die ersten zwei Ecken (eine Seite), dann die Breite des Rechtecks wählen.",
            Tool::Parallelogram => "Die ersten zwei Ecken (eine Seite), dann die dritte Ecke wählen.",
            Tool::CircleConc => "Einen Kreis oder Bogen wählen, dann den Radius des konzentrischen Kreises. Mittelklick beendet.",
            Tool::Circle3T => "Drei Elemente (Linien, Kreise, Bögen) wählen, an die der Kreis tangential sein soll.",
            Tool::Arc3T => "Drei Elemente wählen; der Bogen beginnt am ersten, endet am dritten und ist an alle tangential.",
            Tool::ArcConc => "Einen Kreis oder Bogen wählen, dann den Anfang und das Ende des konzentrischen Bogens.",
            Tool::Conic => "Den Anfang, das Ende, dann den Schulterpunkt des konischen Bogens wählen.",
            Tool::EllipseAxis => "Die beiden Enden einer Achse, dann einen Punkt für die andere Achse wählen.",
            Tool::FilletCirc | Tool::FilletEllip => "Eine Ecke zweier Linien wählen; die Ecke bleibt als Konstruktionslinien erhalten.",
            Tool::FilletEllipTrim => "Eine Ecke zweier Linien wählen.",
            Tool::FilletSmooth(_) => "Eine Ecke zweier Linien wählen; sie wird krümmungsstetig verrundet.",
            Tool::ChamferKeep => "Eine Ecke zweier Linien wählen; die Ecke bleibt als Konstruktionslinien erhalten.",
            Tool::Replace(_) => "Das neue Element wählen, das Bemaßungen und Bedingungen des gewählten Elements übernimmt.",
            Tool::InfoIntersect => "Zwei Elemente wählen, um ihren Schnittpunkt anzuzeigen.",
            Tool::InfoTangent => "Zwei Elemente wählen, um ihren Tangentialpunkt anzuzeigen.",
            Tool::InfoEntity => "Ein Element wählen, um seine Informationen anzuzeigen.",
            Tool::Text => "Den Anfangspunkt des Texts, dann einen Punkt für Höhe und Richtung wählen.",
            Tool::Csys => "Die Position des Koordinatensystems wählen.",
            Tool::Offset => "Eine Kante oder ein Element für den Versatz wählen.",
            Tool::Thicken => "Eine Kante oder ein Element zum Aufdicken wählen.",
            Tool::AddRef => "Kanten oder Ebenen als Referenzen wählen.",
            Tool::RefDimension => "Elemente wählen, dann klicken, um die Referenzbemaßung zu platzieren.",
            Tool::Perimeter => "Die Kette für die Umfangsbemaßung wählen, dann die variable Bemaßung.",
            Tool::Baseline => "Eine Linie oder einen Punkt als Basislinie wählen, dann die Position der Bemaßung.",
            Tool::Con(c) => match c {
                SkCType::Vertical | SkCType::Horizontal => "Eine Linie oder zwei Eckpunkte wählen.",
                SkCType::Perpendicular | SkCType::Parallel => "Zwei Linien wählen.",
                SkCType::Tangent => "Eine Linie und einen Bogen/Kreis oder zwei Bögen/Kreise wählen.",
                SkCType::Midpoint => "Einen Punkt und eine Linie wählen.",
                SkCType::Coincident => "Zwei Punkte oder einen Punkt und ein Element wählen.",
                SkCType::Symmetric => "Zwei Eckpunkte und eine Mittellinie wählen.",
                SkCType::Equal => "Zwei Linien oder zwei Bögen/Kreise wählen.",
                _ => "Die Elemente für die Bedingung wählen.",
            },
        }
    }
}

/// What the owning feature needs from the sketch (for Feature Requirements).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Requirement { ClosedLoops, Revolve, Trajectory, Helix, Any }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkSel { Point(i64), Ent(i64), Con(i64) }

pub enum Outcome { None, Done(Sketch), Cancel }

#[derive(Clone, Copy, Debug)]
struct Snap { pos: V2, point: Option<i64>, on: Option<i64> }

#[derive(Clone, Copy, Debug)]
enum Drag { Point(i64), Ent(i64, V2), Circle(i64), Label(i64), Box(Pos2) }

pub struct SketchSession {
    pub sketch: Sketch,
    pub frame: Frame,
    refs: Vec<[V2; 2]>,
    ghosts: Vec<Vec<V2>>,
    pub slot: Slot,
    pub title: String,
    vars: Vars,
    pub requirement: Requirement,
    pub tool: Tool,
    pub construction: bool,
    pub sides: usize,
    pub grid: bool,
    pub snap_grid: bool,
    pub shade: bool,
    pub show_dims: bool,
    pub show_cons: bool,
    pub show_vertices: bool,
    pub open_ends: bool,
    pub modify_open: bool,
    pub requirements_open: bool,
    pub last_rect: Tool,
    pub last_circle: Tool,
    pub last_arc: Tool,
    pub last_fillet: Tool,
    pub last_chamfer: Tool,
    pub last_line: Tool,
    pub last_ellipse: Tool,
    pub last_centerline: Tool,
    /// Auswählen ▸ Kette: a click selects the whole connected chain
    pub chain_select: bool,
    pub filter: SkFilter,
    pub show_locks: bool,
    pub hl_overlap: bool,
    pub hl_intersect: bool,
    pub hl_branch: bool,
    /// names of `refs` and of the two sketch axes (the references from the default datums)
    pub ref_names: Vec<String>,
    pub axis_names: [String; 2],
    /// model edges that can be added as references (projected onto the sketch plane)
    pub cand_refs: Vec<(String, [V2; 2])>,
    /// datum points, perpendicular axes and model vertices that can become reference points
    pub cand_points: Vec<(String, V2)>,
    pub refs_open: bool,
    pub ref_sel: Option<RefRow>,
    pub pending: Option<Pending>,
    pub rot_resize: Option<RotResize>,
    /// Skizzierer-Palette dialog: open, shown tab and the chosen (tab, item)
    pub palette_open: bool,
    pub palette_tab: usize,
    pub palette_item: Option<(usize, usize)>,
    clipboard: Option<Sketch>,
    clicks: Vec<Snap>,
    chain: Option<(i64, i64)>,
    picks: Vec<SkSel>,
    pub selection: Vec<SkSel>,
    hover: Option<SkSel>,
    drag: Option<Drag>,
    edit: Option<(i64, String, bool)>,
    undo: Vec<Sketch>,
    redo: Vec<Sketch>,
    pub dof: usize,
    pub messages: Vec<(String, MsgKind)>,
    cursor: Option<Pos2>,
    rect: Rect,
    cam: Camera,
    ppu: f64,
    weak_ready: bool,
    modify_text: std::collections::HashMap<i64, String>,
}

/// Measured anchor of a dimension in sketch coordinates (the label is placed relative to it).
pub fn dim_anchor(sk: &Sketch, c: &SkConstraint) -> V2 {
    match c.ty {
        SkCType::Distance | SkCType::HDistance | SkCType::VDistance => (sk.point(c.pts[0]) + sk.point(c.pts[1])) * 0.5,
        SkCType::PointLine => {
            let p = sk.point(c.pts[0]);
            let (a, b) = sk.line_ends(c.ents[0]).unwrap_or((V2::ZERO, V2::X));
            let d = safe_norm2(b - a, V2::X);
            (p + a + d * (p - a).dot(d)) * 0.5
        }
        SkCType::Radius | SkCType::Diameter => sk.entity(c.ents[0]).map(|e| sk.point(e.pts[0])).unwrap_or_default(),
        SkCType::Rho => sk.entity(c.ents[0]).map(|e| sk.conic_shoulder(e)).unwrap_or_default(),
        SkCType::Perimeter => c.ents.first().and_then(|id| sk.entity(*id)).map(|e| { let s = sk.sample(e, 32); s[s.len() / 2] }).unwrap_or_default(),
        SkCType::Baseline => match (c.ents.first(), c.pts.first()) {
            (Some(l), _) => sk.line_ends(*l).map(|(a, b)| (a + b) * 0.5).unwrap_or_default(),
            (_, Some(p)) => sk.point(*p),
            _ => V2::ZERO,
        },
        SkCType::Ordinate => sk.point(c.pts[0]),
        SkCType::Angle => {
            let (Some((a1, b1)), Some((a2, b2))) = (sk.line_ends(c.ents[0]), sk.line_ends(c.ents[1])) else { return V2::ZERO };
            line_intersection(a1, b1, a2, b2).unwrap_or((a1 + a2) * 0.5)
        }
        _ => V2::ZERO,
    }
}

pub fn dim_text(c: &SkConstraint) -> String {
    let v = fmt_num(c.value, 2);
    let base = match c.ty {
        SkCType::Rho => fmt_num(c.value, 2),
        SkCType::Baseline => "0.00".to_string(),
        SkCType::Perimeter => format!("{} UMFANG", v),
        SkCType::Radius => format!("R{}", v),
        SkCType::Diameter => format!("⌀{}", v),
        SkCType::Angle => format!("{}°", v),
        _ => v,
    };
    let base = if c.reference { format!("{} REF", base) } else if c.var { format!("{} VAR", base) } else { base };
    match &c.expr { Some(e) if !e.trim().is_empty() => format!("{} = {}", e, base), _ => base }
}

/// Draws a sketch dimension through an arbitrary projection; returns the label rectangle.
pub fn draw_dim(painter: &egui::Painter, sk: &Sketch, c: &SkConstraint, ts: &dyn Fn(V2) -> Pos2, col: Color32, text: &str) -> Rect {
    let st = Stroke::new(1.0_f32, col);
    let lp = dim_anchor(sk, c) + c.label;
    let ls = ts(lp);
    let arrow = |tip: Pos2, dir: egui::Vec2| {
        if dir.length() < 1e-3 { return; }
        let d = dir.normalized();
        let n = egui::vec2(-d.y, d.x);
        painter.add(Shape::convex_polygon(vec![tip, tip - d * 9.0 + n * 3.0, tip - d * 9.0 - n * 3.0], col, Stroke::NONE));
    };
    match c.ty {
        SkCType::Distance | SkCType::HDistance | SkCType::VDistance | SkCType::PointLine => {
            let (p1, p2) = if c.ty == SkCType::PointLine {
                let p = sk.point(c.pts[0]);
                let (a, b) = sk.line_ends(c.ents[0]).unwrap_or((V2::ZERO, V2::X));
                let d = safe_norm2(b - a, V2::X);
                (p, a + d * (p - a).dot(d))
            } else { (sk.point(c.pts[0]), sk.point(c.pts[1])) };
            let u = match c.ty { SkCType::HDistance => V2::X, SkCType::VDistance => V2::Y, _ => safe_norm2(p2 - p1, V2::X) };
            let n = perp_left(u);
            let a = p1 + n * (lp - p1).dot(n);
            let b = p2 + n * (lp - p2).dot(n);
            let (sa, sb, s1, s2) = (ts(a), ts(b), ts(p1), ts(p2));
            painter.line_segment([s1, sa], Stroke::new(0.8_f32, col));
            painter.line_segment([s2, sb], Stroke::new(0.8_f32, col));
            painter.line_segment([sa, sb], st);
            if (sb - sa).length() > 2.0 { arrow(sa, sa - sb); arrow(sb, sb - sa); }
        }
        SkCType::Radius | SkCType::Diameter => {
            if let Some(e) = sk.entity(c.ents[0]) {
                let cc = sk.point(e.pts[0]);
                let r = sk.radius(e);
                let d = safe_norm2(lp - cc, V2::X);
                let tip = ts(cc + d * r);
                let from = if c.ty == SkCType::Diameter { ts(cc - d * r) } else { ts(cc) };
                painter.line_segment([from, ls], st);
                arrow(tip, tip - from);
                if c.ty == SkCType::Diameter { arrow(from, from - tip); }
            }
        }
        SkCType::Rho | SkCType::Perimeter | SkCType::Ordinate => { painter.line_segment([ts(dim_anchor(sk, c)), ls], Stroke::new(0.8_f32, col)); }
        SkCType::Baseline => {
            // baseline: leader from the line/point, short tick marking the zero position
            let a = ts(dim_anchor(sk, c));
            painter.line_segment([a, ls], Stroke::new(0.8_f32, col));
            painter.circle_filled(a, 2.5, col);
        }
        SkCType::Angle => {
            let ip = dim_anchor(sk, c);
            if let (Some((a1, b1)), Some((a2, b2))) = (sk.line_ends(c.ents[0]), sk.line_ends(c.ents[1])) {
                let d1 = safe_norm2(b1 - a1, V2::X);
                let mut d2 = safe_norm2(b2 - a2, V2::Y);
                if c.aux != 0.0 { d2 = -d2; }
                let r = (lp - ip).length().max(1e-6);
                let (t1, mut t2) = (d1.y.atan2(d1.x), d2.y.atan2(d2.x));
                while t2 - t1 > std::f64::consts::PI { t2 -= std::f64::consts::TAU; }
                while t1 - t2 > std::f64::consts::PI { t2 += std::f64::consts::TAU; }
                let pts: Vec<Pos2> = (0..=24).map(|k| { let t = t1 + (t2 - t1) * k as f64 / 24.0; ts(ip + V2::new(t.cos(), t.sin()) * r) }).collect();
                painter.add(Shape::line(pts, st));
            }
        }
        _ => {}
    }
    let g = painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(12.0), col);
    let r = Rect::from_center_size(ls, g.size() + egui::vec2(6.0, 2.0));
    painter.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 200));
    painter.galley(ls - g.size() / 2.0, g, col);
    r
}

fn seg_px(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_sq();
    if l2 < 1e-9 { return (p - a).length(); }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

impl SketchSession {
    pub fn new(sketch: Sketch, frame: Frame, refs: Vec<[V2; 2]>, ghosts: Vec<Vec<V2>>, slot: Slot, title: String, vars: Vars) -> Self {
        let mut sk = sketch;
        sk.ref_lines = refs.clone();
        let mut cam = Camera::default();
        cam.rot = glam::DQuat::from_mat3(&glam::DMat3::from_cols(frame.x, frame.y, frame.z()));
        cam.target = frame.origin;
        cam.spin_center = frame.origin;
        let tool = if sk.is_empty() { Tool::Line } else { Tool::Select };
        let mut s = SketchSession {
            sketch: sk, frame, refs, ghosts, slot, title, vars, requirement: Requirement::ClosedLoops,
            tool, construction: false, sides: 6, grid: false, snap_grid: false, shade: true, show_dims: true, show_cons: true,
            show_vertices: true, open_ends: true, modify_open: false, requirements_open: false,
            last_rect: Tool::Rect, last_circle: Tool::Circle, last_arc: Tool::Arc3, last_fillet: Tool::FilletCirc, last_chamfer: Tool::ChamferKeep,
            last_line: Tool::Line, last_ellipse: Tool::EllipseAxis, last_centerline: Tool::Centerline, chain_select: false, filter: SkFilter::All,
            show_locks: false, hl_overlap: false, hl_intersect: false, hl_branch: false,
            ref_names: vec![], axis_names: [String::new(), String::new()], cand_refs: vec![], cand_points: vec![], refs_open: false, ref_sel: None, pending: None, rot_resize: None, palette_open: false, palette_tab: 8, palette_item: None, clipboard: None,
            clicks: vec![], chain: None, picks: vec![], selection: vec![], hover: None, drag: None, edit: None,
            undo: vec![], redo: vec![], dof: 0, messages: vec![], cursor: None,
            rect: Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 800.0)), cam, ppu: 4.0, weak_ready: false,
            modify_text: Default::default(),
        };
        s.dof = solver::dof(&s.sketch);
        s
    }

    fn say(&mut self, t: impl Into<String>, k: MsgKind) { self.messages.push((t.into(), k)); }

    fn size(&self) -> (f32, f32) { (self.rect.width().max(1.0), self.rect.height().max(1.0)) }

    pub fn to_screen(&self, p: V2) -> Pos2 {
        match self.cam.project(self.frame.to_world(p), self.size()) {
            Some((x, y, _)) => Pos2::new(self.rect.min.x + x, self.rect.min.y + y),
            None => Pos2::new(-1.0e5, -1.0e5),
        }
    }

    pub fn from_screen(&self, s: Pos2) -> V2 {
        let (o, d) = self.cam.ray((s.x - self.rect.min.x, s.y - self.rect.min.y), self.size());
        let n = self.frame.z();
        let den = d.dot(n);
        let w = if den.abs() < 1e-9 { o } else { o + d * ((self.frame.origin - o).dot(n) / den) };
        self.frame.to_local(w)
    }

    fn update_ppu(&mut self) {
        let c = self.from_screen(self.rect.center());
        let a = self.to_screen(c);
        let px = (self.to_screen(c + V2::X) - a).length();
        let py = (self.to_screen(c + V2::Y) - a).length();
        self.ppu = (((px + py) * 0.5) as f64).max(1e-6);
    }

    fn view_extent(&self) -> f64 { (self.rect.width().max(self.rect.height()) as f64) / self.ppu }

    /// Bounding box of the sketch for "Sketch View" fitting.
    pub fn fit_bounds(&self) -> (V2, V2) {
        let mut mn = V2::splat(-40.0);
        let mut mx = V2::splat(40.0);
        if let Some((a, b)) = self.sketch.bounds() { mn = a; mx = b; }
        else if !self.refs.is_empty() {
            mn = V2::splat(f64::INFINITY);
            mx = V2::splat(f64::NEG_INFINITY);
            for l in &self.refs { for p in l { mn = mn.min(*p); mx = mx.max(*p); } }
        }
        (mn.min(V2::ZERO), mx.max(V2::ZERO))
    }

    fn push_undo(&mut self) {
        self.undo.push(self.sketch.clone());
        if self.undo.len() > 100 { self.undo.remove(0); }
        self.redo.clear();
    }

    pub fn undo(&mut self) { if let Some(p) = self.undo.pop() { self.redo.push(std::mem::replace(&mut self.sketch, p)); self.after_change(); } }
    pub fn redo(&mut self) { if let Some(n) = self.redo.pop() { self.undo.push(std::mem::replace(&mut self.sketch, n)); self.after_change(); } }
    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }

    fn refresh_weak(&mut self) {
        let off = 22.0 / self.ppu;
        solver::refresh_weak(&mut self.sketch, off);
    }

    fn after_change(&mut self) {
        self.refresh_weak();
        self.dof = solver::dof(&self.sketch);
        let sk = &self.sketch;
        let valid = |s: &SkSel| match s {
            SkSel::Point(id) => sk.points.iter().any(|p| p.id == *id),
            SkSel::Ent(id) => *id < 0 || sk.entity(*id).is_some(),
            SkSel::Con(id) => sk.constraints.iter().any(|c| c.id == *id),
        };
        let keep: Vec<SkSel> = self.selection.iter().copied().filter(valid).collect();
        self.selection = keep;
    }

    fn solve(&mut self) -> bool {
        let ok = solver::solve(&mut self.sketch, None);
        self.after_change();
        ok
    }

    pub fn set_tool(&mut self, t: Tool) {
        self.tool = t;
        self.clicks.clear();
        self.chain = None;
        self.picks.clear();
        self.say(t.hint(), MsgKind::Prompt);
        match t {
            Tool::Rect | Tool::CenterRect | Tool::SlantRect | Tool::Parallelogram => self.last_rect = t,
            Tool::Circle | Tool::Circle3 | Tool::CircleConc | Tool::Circle3T => self.last_circle = t,
            Tool::Arc3 | Tool::ArcCenter | Tool::Arc3T | Tool::ArcConc | Tool::Conic => self.last_arc = t,
            Tool::Fillet | Tool::FilletCirc | Tool::FilletEllip | Tool::FilletEllipTrim | Tool::FilletSmooth(_) => self.last_fillet = t,
            Tool::Chamfer | Tool::ChamferKeep => self.last_chamfer = t,
            Tool::Line | Tool::LineTangent => self.last_line = t,
            Tool::Ellipse | Tool::EllipseAxis => self.last_ellipse = t,
            Tool::Centerline | Tool::CenterlineTangent => self.last_centerline = t,
            Tool::Con(c) => {
                let sel = self.selection.clone();
                if !sel.is_empty() {
                    self.picks = sel;
                    if !self.try_constraint(c) { self.picks.clear(); }
                }
            }
            Tool::Mirror if !self.selection.iter().any(|s| matches!(s, SkSel::Ent(_))) => {
                self.say("Zuerst die zu spiegelnden Elemente wählen, dann Spiegeln wählen.", MsgKind::Warning);
                self.tool = Tool::Select;
            }
            _ => {}
        }
    }

    // ---------- hit testing (screen space) ----------

    fn poly_px(&self, s: Pos2, pts: &[V2]) -> f32 {
        let sp: Vec<Pos2> = pts.iter().map(|p| self.to_screen(*p)).collect();
        if sp.len() == 1 { return (sp[0] - s).length(); }
        sp.windows(2).map(|w| seg_px(s, w[0], w[1])).fold(f32::INFINITY, f32::min)
    }

    fn long_line(&self, a: V2, b: V2) -> (V2, V2) {
        let d = safe_norm2(b - a, V2::X);
        let l = self.view_extent() * 3.0 + (a.length() + b.length());
        (a - d * l, b + d * l)
    }

    fn ent_px(&self, e: &SkEntity, s: Pos2) -> f32 {
        match e.kind {
            SkKind::Line => {
                let (a, b) = (self.sketch.point(e.pts[0]), self.sketch.point(e.pts[1]));
                if e.centerline { let (a2, b2) = self.long_line(a, b); self.poly_px(s, &[a2, b2]) } else { self.poly_px(s, &[a, b]) }
            }
            SkKind::Circle | SkKind::Arc | SkKind::Spline | SkKind::Ellipse | SkKind::Conic | SkKind::Corner => self.poly_px(s, &self.sketch.sample(e, 64)),
            SkKind::Point | SkKind::Csys => (self.to_screen(self.sketch.point(e.pts[0])) - s).length(),
            SkKind::Text => self.sketch.text_loops(e).iter().map(|l| { let mut c = l.clone(); if let Some(f) = l.first() { c.push(*f); } self.poly_px(s, &c) }).fold(f32::INFINITY, f32::min),
        }
    }

    fn ref_px(&self, id: i64, s: Pos2) -> f32 {
        match id {
            REF_H if self.sketch.axes_off[0] => f32::INFINITY,
            REF_V if self.sketch.axes_off[1] => f32::INFINITY,
            REF_H => { let (a, b) = self.long_line(V2::ZERO, V2::X); self.poly_px(s, &[a, b]) }
            REF_V => { let (a, b) = self.long_line(V2::ZERO, V2::Y); self.poly_px(s, &[a, b]) }
            k => self.refs.get((-10 - k) as usize).map(|l| self.poly_px(s, &[l[0], l[1]])).unwrap_or(f32::INFINITY),
        }
    }

    fn hit_point(&self, s: Pos2, exclude: &[i64]) -> Option<i64> {
        let mut best = (PICK, None);
        for q in &self.sketch.points {
            if exclude.contains(&q.id) { continue; }
            let used = q.id == 0 || self.sketch.entities.iter().any(|e| e.pts.contains(&q.id));
            if !used { continue; }
            let d = (self.to_screen(q.p) - s).length();
            if d < best.0 { best = (d, Some(q.id)); }
        }
        best.1
    }

    fn hit_entity(&self, s: Pos2, include_refs: bool) -> Option<i64> {
        let mut best = (PICK, None);
        for e in &self.sketch.entities {
            let d = self.ent_px(e, s);
            if d < best.0 { best = (d, Some(e.id)); }
        }
        if best.1.is_none() && include_refs {
            for i in 0..self.refs.len() {
                let id = -10 - i as i64;
                let d = self.ref_px(id, s);
                if d < best.0 { best = (d, Some(id)); }
            }
            for id in [REF_H, REF_V] { let d = self.ref_px(id, s); if d < best.0 { best = (d, Some(id)); } }
        }
        best.1
    }

    fn snap(&self, s: Pos2, exclude: &[i64]) -> Snap {
        let p = self.from_screen(s);
        if let Some(id) = self.hit_point(s, exclude) { return Snap { pos: self.sketch.point(id), point: Some(id), on: None }; }
        for (i, l) in self.refs.iter().enumerate() {
            for q in l { if (self.to_screen(*q) - s).length() < PICK { return Snap { pos: *q, point: None, on: Some(-10 - i as i64) }; } }
        }
        let mut best = (PICK, None);
        for e in &self.sketch.entities {
            if e.kind == SkKind::Line && e.pts.iter().any(|x| exclude.contains(x)) { continue; }
            let d = self.ent_px(e, s);
            if d < best.0 { best = (d, Some(e.id)); }
        }
        if let Some(id) = best.1 {
            let e = self.sketch.entity(id).unwrap().clone();
            let pos = match e.kind {
                SkKind::Line => { let (a, b) = (self.sketch.point(e.pts[0]), self.sketch.point(e.pts[1])); let d = safe_norm2(b - a, V2::X); a + d * (p - a).dot(d) }
                SkKind::Circle | SkKind::Arc => { let c = self.sketch.point(e.pts[0]); c + safe_norm2(p - c, V2::X) * self.sketch.radius(&e) }
                SkKind::Point | SkKind::Csys => self.sketch.point(e.pts[0]),
                SkKind::Spline | SkKind::Ellipse | SkKind::Conic | SkKind::Text | SkKind::Corner => {
                    // nearest point on the sampled curve (no point-on constraint for free-form curves)
                    let pts = self.sketch.sample(&e, 96);
                    let q = pts.windows(2).map(|w| { let d = w[1] - w[0]; let t = ((p - w[0]).dot(d) / d.length_squared().max(1e-12)).clamp(0.0, 1.0); w[0] + d * t })
                        .min_by(|a, b| (*a - p).length().partial_cmp(&(*b - p).length()).unwrap()).unwrap_or(p);
                    return Snap { pos: q, point: None, on: None };
                }
            };
            return Snap { pos, point: None, on: Some(id) };
        }
        for (i, l) in self.refs.iter().enumerate() {
            if self.ref_px(-10 - i as i64, s) < PICK {
                let d = safe_norm2(l[1] - l[0], V2::X);
                return Snap { pos: l[0] + d * (p - l[0]).dot(d), point: None, on: Some(-10 - i as i64) };
            }
        }
        if self.ref_px(REF_H, s) < PICK { return Snap { pos: V2::new(p.x, 0.0), point: None, on: Some(REF_H) }; }
        if self.ref_px(REF_V, s) < PICK { return Snap { pos: V2::new(0.0, p.y), point: None, on: Some(REF_V) }; }
        let mut pos = p;
        if self.snap_grid { let g = self.grid_step(); pos = V2::new((p.x / g).round() * g, (p.y / g).round() * g); }
        Snap { pos, point: None, on: None }
    }

    fn resolve(&mut self, s: &Snap) -> i64 {
        if let Some(id) = s.point { return id; }
        let id = self.sketch.add_point(s.pos);
        if let Some(on) = s.on { self.sketch.add_constraint(SkConstraint::new(SkCType::PointOn, vec![id], vec![on])); }
        id
    }

    fn grid_step(&self) -> f64 {
        let target = 40.0 / self.ppu;
        let p = 10f64.powf(target.log10().floor());
        for m in [1.0, 2.0, 5.0, 10.0] { if p * m >= target { return p * m; } }
        p * 10.0
    }

    fn add_line(&mut self, a: i64, b: i64) -> i64 {
        let c = self.construction;
        self.sketch.add_entity(SkKind::Line, vec![a, b], 0.0, c, false)
    }

    fn con(&mut self, ty: SkCType, pts: Vec<i64>, ents: Vec<i64>) { self.sketch.add_constraint(SkConstraint::new(ty, pts, ents)); }

    // ---------- tool clicks ----------

    fn finish_spline(&mut self) {
        let clicks = std::mem::take(&mut self.clicks);
        if clicks.len() < 2 { return; }
        self.push_undo();
        let mut ids = vec![];
        for c in &clicks { let id = self.resolve(c); if ids.last() != Some(&id) { ids.push(id); } }
        if ids.len() < 2 { return; }
        let cons = self.construction;
        self.sketch.add_entity(SkKind::Spline, ids, 0.0, cons, false);
        self.solve();
    }

    fn click(&mut self, s: Pos2, secondary: bool) {
        if secondary {
            if self.tool == Tool::Spline { self.finish_spline(); }
            self.clicks.clear();
            self.chain = None;
            self.picks.clear();
            return;
        }
        let exclude: Vec<i64> = self.chain.map(|c| vec![c.1]).unwrap_or_default();
        let sn = self.snap(s, &exclude);
        match self.tool {
            Tool::Select => {}
            Tool::Centerline => {
                self.clicks.push(sn);
                if self.clicks.len() == 2 {
                    self.push_undo();
                    let (c0, c1) = (self.clicks[0], self.clicks[1]);
                    let a = self.resolve(&c0);
                    let b = self.resolve(&c1);
                    let id = self.sketch.add_entity(SkKind::Line, vec![a, b], 0.0, true, true);
                    let d = c1.pos - c0.pos;
                    if d.x.abs() < d.y.abs() * 0.07 { self.con(SkCType::Vertical, vec![], vec![id]); }
                    else if d.y.abs() < d.x.abs() * 0.07 { self.con(SkCType::Horizontal, vec![], vec![id]); }
                    self.clicks.clear();
                    self.solve();
                }
            }
            Tool::Line => match self.chain {
                None => {
                    self.push_undo();
                    let a = self.resolve(&sn);
                    self.chain = Some((a, a));
                }
                Some((start, last)) => {
                    if sn.point == Some(last) { return; }
                    let lp = self.sketch.point(last);
                    let mut sn2 = sn;
                    let d = sn.pos - lp;
                    let mut hv = None;
                    if sn.point.is_none() {
                        if d.y.abs() < d.x.abs() * 0.07 { sn2.pos.y = lp.y; hv = Some(SkCType::Horizontal); }
                        else if d.x.abs() < d.y.abs() * 0.07 { sn2.pos.x = lp.x; hv = Some(SkCType::Vertical); }
                    } else if let Some(pid) = sn.point {
                        let dq = self.sketch.point(pid) - lp;
                        if dq.y.abs() < 1e-9 * (1.0 + dq.x.abs()) { hv = Some(SkCType::Horizontal); }
                        else if dq.x.abs() < 1e-9 * (1.0 + dq.y.abs()) { hv = Some(SkCType::Vertical); }
                    }
                    self.push_undo();
                    let b = self.resolve(&sn2);
                    let id = self.add_line(last, b);
                    if let Some(t) = hv { self.con(t, vec![], vec![id]); }
                    self.solve();
                    if b == start { self.chain = None; self.say("Schleife geschlossen.", MsgKind::Success); } else { self.chain = Some((start, b)); }
                }
            },
            Tool::Rect | Tool::CenterRect => {
                self.clicks.push(sn);
                if self.clicks.len() < 2 { return; }
                self.push_undo();
                let (c0, c1) = (self.clicks[0], self.clicks[1]);
                self.clicks.clear();
                let (lo, hi) = if self.tool == Tool::Rect { (c0.pos, c1.pos) } else { (c0.pos * 2.0 - c1.pos, c1.pos) };
                if (hi.x - lo.x).abs() < 1e-9 || (hi.y - lo.y).abs() < 1e-9 { return; }
                let p1 = if self.tool == Tool::Rect { self.resolve(&c0) } else { self.sketch.add_point(lo) };
                let p3 = self.resolve(&c1);
                let p2 = self.sketch.add_point(V2::new(hi.x, lo.y));
                let p4 = self.sketch.add_point(V2::new(lo.x, hi.y));
                let l1 = self.add_line(p1, p2);
                let l2 = self.add_line(p2, p3);
                let l3 = self.add_line(p3, p4);
                let l4 = self.add_line(p4, p1);
                self.con(SkCType::Horizontal, vec![], vec![l1]);
                self.con(SkCType::Vertical, vec![], vec![l2]);
                self.con(SkCType::Horizontal, vec![], vec![l3]);
                self.con(SkCType::Vertical, vec![], vec![l4]);
                if self.tool == Tool::CenterRect {
                    let c = self.resolve(&c0);
                    let d1 = self.sketch.add_entity(SkKind::Line, vec![p1, p3], 0.0, true, false);
                    let d2 = self.sketch.add_entity(SkKind::Line, vec![p2, p4], 0.0, true, false);
                    self.con(SkCType::Midpoint, vec![c], vec![d1]);
                    self.con(SkCType::Midpoint, vec![c], vec![d2]);
                }
                self.solve();
            }
            Tool::Circle => {
                self.clicks.push(sn);
                if self.clicks.len() < 2 { return; }
                self.push_undo();
                let (c0, c1) = (self.clicks[0], self.clicks[1]);
                self.clicks.clear();
                let r = (c1.pos - c0.pos).length();
                if r < 1e-9 { return; }
                let c = self.resolve(&c0);
                let cons = self.construction;
                self.sketch.add_entity(SkKind::Circle, vec![c], r, cons, false);
                self.solve();
            }
            Tool::Circle3 => {
                self.clicks.push(sn);
                if self.clicks.len() < 3 { return; }
                let (a, b, c) = (self.clicks[0].pos, self.clicks[1].pos, self.clicks[2].pos);
                self.clicks.clear();
                let Some(cen) = circumcenter(a, b, c) else { self.say("Die Punkte sind kollinear.", MsgKind::Warning); return };
                self.push_undo();
                let ci = self.sketch.add_point(cen);
                let cons = self.construction;
                self.sketch.add_entity(SkKind::Circle, vec![ci], (a - cen).length(), cons, false);
                self.solve();
            }
            Tool::Arc3 | Tool::ArcCenter => {
                self.clicks.push(sn);
                if self.clicks.len() < 3 { return; }
                let (c0, c1, c2) = (self.clicks[0], self.clicks[1], self.clicks[2]);
                self.clicks.clear();
                let (center, s0, e0) = if self.tool == Tool::Arc3 {
                    let Some(c) = circumcenter(c0.pos, c1.pos, c2.pos) else { self.say("Die Punkte sind kollinear.", MsgKind::Warning); return };
                    let ang = |p: V2| (p.y - c.y).atan2(p.x - c.x);
                    let tau = std::f64::consts::TAU;
                    let rel = |a: f64| ((a - ang(c0.pos)) % tau + tau) % tau;
                    if rel(ang(c2.pos)) < rel(ang(c1.pos)) { (c, c0, c1) } else { (c, c1, c0) }
                } else {
                    let r = (c1.pos - c0.pos).length();
                    let mut e = c2;
                    e.pos = c0.pos + safe_norm2(c2.pos - c0.pos, V2::X) * r;
                    if let Some(pid) = e.point { if ((self.sketch.point(pid) - c0.pos).length() - r).abs() > 1e-6 { e.point = None; } }
                    if cross2(c1.pos - c0.pos, e.pos - c0.pos) >= 0.0 { (c0.pos, c1, e) } else { (c0.pos, e, c1) }
                };
                self.push_undo();
                let cid = if self.tool == Tool::ArcCenter { self.resolve(&c0) } else { self.sketch.add_point(center) };
                let sid = self.resolve(&s0);
                let eid = self.resolve(&e0);
                let cons = self.construction;
                self.sketch.add_entity(SkKind::Arc, vec![cid, sid, eid], 0.0, cons, false);
                self.solve();
            }
            Tool::Point => {
                self.push_undo();
                let p = self.resolve(&sn);
                self.sketch.add_entity(SkKind::Point, vec![p], 0.0, true, false);
                self.solve();
            }
            Tool::Palette => self.place_palette(sn.pos),
            Tool::Polygon => {
                self.clicks.push(sn);
                if self.clicks.len() < 2 { return; }
                self.push_undo();
                let (c0, c1) = (self.clicks[0], self.clicks[1]);
                self.clicks.clear();
                let r = (c1.pos - c0.pos).length();
                if r < 1e-9 { return; }
                let c = self.resolve(&c0);
                let circ = self.sketch.add_entity(SkKind::Circle, vec![c], r, true, false);
                let a0 = (c1.pos.y - c0.pos.y).atan2(c1.pos.x - c0.pos.x);
                let n = self.sides.max(3);
                let ids: Vec<i64> = (0..n).map(|k| {
                    let t = a0 + std::f64::consts::TAU * k as f64 / n as f64;
                    self.sketch.add_point(c0.pos + V2::new(t.cos(), t.sin()) * r)
                }).collect();
                let mut lines = vec![];
                for k in 0..n { lines.push(self.add_line(ids[k], ids[(k + 1) % n])); }
                for &id in &ids { self.con(SkCType::PointOn, vec![id], vec![circ]); }
                for k in 1..n { self.con(SkCType::Equal, vec![], vec![lines[0], lines[k]]); }
                self.solve();
            }
            Tool::Spline => {
                if let Some(last) = self.clicks.last() {
                    let closes = self.clicks.len() >= 3 && sn.point.is_some() && sn.point == self.clicks[0].point;
                    if (self.to_screen(last.pos) - self.to_screen(sn.pos)).length() < PICK || closes {
                        if closes { self.clicks.push(sn); }
                        self.finish_spline();
                        return;
                    }
                }
                self.clicks.push(sn);
            }
            Tool::Ellipse => {
                self.clicks.push(sn);
                if self.clicks.len() < 3 { return; }
                let (c0, c1, c2) = (self.clicks[0], self.clicks[1], self.clicks[2]);
                self.clicks.clear();
                let u = safe_norm2(c1.pos - c0.pos, V2::X);
                let ry = cross2(u, c2.pos - c0.pos).abs();
                if (c1.pos - c0.pos).length() < 1e-9 || ry < 1e-9 { return; }
                self.push_undo();
                let c = self.resolve(&c0);
                let a = self.sketch.add_point(c1.pos);
                let cons = self.construction;
                self.sketch.add_entity(SkKind::Ellipse, vec![c, a], ry, cons, false);
                self.solve();
            }
            Tool::Fillet | Tool::Chamfer | Tool::FilletCirc | Tool::ChamferKeep | Tool::FilletEllip | Tool::FilletEllipTrim | Tool::FilletSmooth(_) => {
                let Some(pid) = self.hit_point(s, &[0]) else { self.say("Eine Ecke zweier Linien wählen.", MsgKind::Warning); return };
                match self.tool {
                    Tool::Fillet => self.fillet(pid, false),
                    Tool::FilletCirc => self.fillet(pid, true),
                    Tool::Chamfer => self.chamfer(pid, false),
                    Tool::ChamferKeep => self.chamfer(pid, true),
                    Tool::FilletSmooth(shape) => self.fillet_smooth(pid, shape),
                    t => self.fillet_elliptic(pid, t == Tool::FilletEllip),
                }
            }
            Tool::LineTangent | Tool::CenterlineTangent | Tool::CircleConc | Tool::Circle3T | Tool::Arc3T | Tool::ArcConc
            | Tool::Replace(_) | Tool::InfoIntersect | Tool::InfoTangent | Tool::InfoEntity => self.click_pick_tool(s, sn),
            Tool::SlantRect | Tool::Parallelogram | Tool::Conic | Tool::EllipseAxis => {
                self.clicks.push(sn);
                if self.clicks.len() == 3 { let c = std::mem::take(&mut self.clicks); self.three_point_shape(&c); }
            }
            Tool::Perimeter => {
                // chain entities first; a strong dimension of the chain then becomes the variable one
                if let Some(d) = self.hit_dimension(s).filter(|_| !self.picks.is_empty()) {
                    let ents: Vec<i64> = self.picks.iter().filter_map(|x| if let SkSel::Ent(e) = x { Some(*e) } else { None }).collect();
                    let pts: Vec<i64> = ents.iter().filter_map(|e| self.sketch.entity(*e)).flat_map(|e| e.pts.clone()).collect();
                    let Some(dc) = self.sketch.constraints.iter().find(|c| c.id == d && c.ty.is_dimension() && !c.reference && matches!(c.ty, SkCType::Distance | SkCType::HDistance | SkCType::VDistance | SkCType::PointLine | SkCType::Radius | SkCType::Diameter | SkCType::Angle)).cloned() else {
                        self.say("Eine Längen-, Radius- oder Winkelbemaßung als variable Bemaßung wählen.", MsgKind::Warning); return
                    };
                    if !(dc.ents.iter().any(|e| ents.contains(e)) || dc.pts.iter().any(|p| pts.contains(p))) { self.say("Die variable Bemaßung muss zur gewählten Kette gehören.", MsgKind::Warning); return; }
                    self.push_undo();
                    if let Some(c) = self.sketch.constraints.iter_mut().find(|c| c.id == d) { c.weak = false; c.var = true; }
                    let mut k = SkConstraint::new(SkCType::Perimeter, vec![], ents);
                    k.value = self.sketch.measure(&k);
                    k.label = V2::new(0.0, self.view_extent() * 0.06);
                    self.sketch.add_constraint(k);
                    self.picks.clear();
                    self.solve();
                    return;
                }
                let Some(id) = self.hit_entity(s, false).filter(|id| matches!(self.sketch.entity(*id).map(|e| e.kind), Some(SkKind::Line | SkKind::Arc | SkKind::Circle | SkKind::Spline | SkKind::Conic))) else { return };
                let sel = SkSel::Ent(id);
                if let Some(i) = self.picks.iter().position(|x| *x == sel) { self.picks.remove(i); } else { self.picks.push(sel); }
                self.say("Weitere Elemente der Kette wählen oder die variable Bemaßung wählen.", MsgKind::Prompt);
            }
            Tool::Baseline => {
                let p = self.from_screen(s);
                if self.picks.is_empty() {
                    let hit = self.hit_point(s, &[]).map(SkSel::Point).or_else(|| self.hit_entity(s, true).filter(|e| *e < 0 || self.sketch.entity(*e).map(|x| x.kind) == Some(SkKind::Line)).map(SkSel::Ent));
                    if let Some(h) = hit { self.picks.push(h); }
                    return;
                }
                let k = match self.picks[0] {
                    SkSel::Ent(l) => SkConstraint::new(SkCType::Baseline, vec![], vec![l]),
                    SkSel::Point(q) => {
                        let d = p - self.sketch.point(q);
                        let mut c = SkConstraint::new(SkCType::Baseline, vec![q], vec![]);
                        if d.y.abs() > d.x.abs() { c.aux = 1.0; }
                        c
                    }
                    _ => return,
                };
                self.picks.clear();
                self.push_undo();
                let mut k = k;
                k.label = p - dim_anchor(&self.sketch, &k);
                self.sketch.add_constraint(k);
                self.after_change();
            }
            Tool::Text => {
                self.clicks.push(sn);
                if self.clicks.len() == 2 {
                    let c = std::mem::take(&mut self.clicks);
                    if (c[1].pos - c[0].pos).length() < 1e-9 { return; }
                    self.pending = Some(Pending::Text { a: c[0].pos, b: c[1].pos, t: SkText { text: String::new(), font: 0, aspect: 1.0, slant: 0.0, family: String::new() } });
                }
            }
            Tool::Csys => {
                self.push_undo();
                let p = self.resolve(&sn);
                self.sketch.add_entity(SkKind::Csys, vec![p], 0.0, true, false);
                self.solve();
            }
            Tool::Offset | Tool::Thicken => {
                let p = self.from_screen(s);
                let Some(src) = self.hit_entity(s, true).filter(|id| *id <= -10 || matches!(self.sketch.entity(*id).map(|e| e.kind), Some(SkKind::Line | SkKind::Circle | SkKind::Arc))) else {
                    self.say("Eine Kante, Linie, einen Kreis oder Bogen wählen.", MsgKind::Warning); return
                };
                self.pending = Some(if self.tool == Tool::Offset { Pending::Offset { src, side: p, value: String::new() } }
                    else { Pending::Thicken { src, side: p, thick: String::new(), off: String::new(), cap: 1 } });
                self.say(if self.tool == Tool::Offset { "Versatz in Pfeilrichtung eingeben." } else { "Dicke und Versatz in Pfeilrichtung eingeben." }, MsgKind::Prompt);
            }
            Tool::AddRef => {
                let ph = self.cand_points.iter().enumerate().map(|(i, c)| (i, (self.to_screen(c.1) - s).length())).filter(|x| x.1 < PICK).min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                if let Some((i, _)) = ph {
                    let (name, p) = self.cand_points.remove(i);
                    self.push_undo();
                    let id = self.sketch.add_point(p);
                    let e = self.sketch.add_entity(SkKind::Point, vec![id], 0.0, true, false);
                    if let Some(x) = self.sketch.entities.iter_mut().find(|x| x.id == e) { x.ref_name = name; }
                    let mut k = SkConstraint::new(SkCType::Fix, vec![id], vec![]);
                    k.fixed = Some(p);
                    self.sketch.add_constraint(k);
                    self.after_change();
                    return;
                }
                let hit = self.cand_refs.iter().enumerate().map(|(i, c)| (i, self.poly_px(s, &[c.1[0], c.1[1]]))).filter(|x| x.1 < PICK).min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                let Some((i, _)) = hit else { self.say("Eine Kante des Modells wählen.", MsgKind::Warning); return };
                let (name, l) = self.cand_refs.remove(i);
                if let Some(k) = (0..2).find(|k| self.sketch.axes_off[*k] && self.axis_names[*k] == name) {
                    // a deleted default reference comes back
                    self.sketch.axes_off[k] = false;
                } else {
                    if let Some(h) = self.sketch.hidden_refs.iter().position(|h| *h == l) { self.sketch.hidden_refs.remove(h); } else { self.sketch.extra_refs.push((name.clone(), l)); }
                    self.refs.push(l);
                    self.ref_names.push(name);
                    self.sketch.ref_lines = self.refs.clone();
                }
                self.after_change();
            }
            Tool::Trim => {
                if let Some(id) = self.hit_entity(s, false) { let p = self.from_screen(s); self.trim_at(id, p); }
            }
            Tool::Divide => {
                let Some(id) = self.hit_entity(s, false) else { return };
                let p = self.from_screen(s);
                self.divide(id, p);
            }
            Tool::Corner => {
                let Some(id) = self.hit_entity(s, false) else { return };
                if self.sketch.entity(id).map(|e| e.kind) != Some(SkKind::Line) { self.say("Linien wählen.", MsgKind::Warning); return; }
                self.picks.push(SkSel::Ent(id));
                if self.picks.len() == 2 {
                    let (SkSel::Ent(a), SkSel::Ent(b)) = (self.picks[0], self.picks[1]) else { return };
                    self.picks.clear();
                    self.corner(a, b);
                }
            }
            Tool::Mirror => {
                let Some(id) = self.hit_entity(s, false) else { return };
                if !self.sketch.entity(id).map(|e| e.centerline).unwrap_or(false) { self.say("Eine Mittellinie wählen.", MsgKind::Warning); return; }
                self.mirror(id);
                self.tool = Tool::Select;
            }
            Tool::Project => {
                let hit = (0..self.refs.len()).map(|i| (i, self.ref_px(-10 - i as i64, s))).filter(|x| x.1 < PICK).min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                let Some((i, _)) = hit else { self.say("Eine hervorgehobene Referenzkante wählen.", MsgKind::Warning); return };
                let l = self.refs[i];
                self.push_undo();
                let (a, b) = (self.sketch.add_point(l[0]), self.sketch.add_point(l[1]));
                self.sketch.add_line(a, b);
                for (p, q) in [(a, l[0]), (b, l[1])] { let mut c = SkConstraint::new(SkCType::Fix, vec![p], vec![]); c.fixed = Some(q); self.sketch.add_constraint(c); }
                self.solve();
            }
            Tool::Dimension | Tool::RefDimension => {
                let p = self.from_screen(s);
                let base = self.hit_dimension(s).filter(|id| self.sketch.constraints.iter().any(|c| c.id == *id && c.ty == SkCType::Baseline));
                let mut hit = base.map(SkSel::Con).or_else(|| self.hit_point(s, &[]).map(SkSel::Point)).or_else(|| self.hit_entity(s, true).map(SkSel::Ent));
                // a reference that is not parallel to an already picked line is a placement click
                if let (Some(SkSel::Ent(r)), [SkSel::Ent(first)]) = (hit, self.picks.as_slice()) {
                    if r < 0 {
                        let par = match (self.sketch.line_ends(r), self.sketch.line_ends(*first)) {
                            (Some((a1, b1)), Some((a2, b2))) => cross2(safe_norm2(b1 - a1, V2::X), safe_norm2(b2 - a2, V2::X)).abs() < 1e-6,
                            _ => false,
                        };
                        if !par { hit = None; }
                    }
                }
                match hit {
                    Some(h) if self.picks.len() < 2 && !self.picks.contains(&h) => self.picks.push(h),
                    _ => {
                        if !self.picks.is_empty() { let picks = std::mem::take(&mut self.picks); self.create_dimension(&picks, p); }
                    }
                }
            }
            Tool::Con(c) => {
                let hit = self.hit_point(s, &[]).map(SkSel::Point).or_else(|| self.hit_entity(s, true).map(SkSel::Ent));
                if let Some(h) = hit {
                    if !self.picks.contains(&h) { self.picks.push(h); }
                    self.try_constraint(c);
                }
            }
        }
    }

    fn corner_lines(&self, pid: i64) -> Option<(SkEntity, SkEntity)> {
        let lines: Vec<SkEntity> = self.sketch.entities.iter().filter(|e| e.kind == SkKind::Line && !e.construction && e.pts.contains(&pid)).cloned().collect();
        if lines.len() != 2 { return None; }
        Some((lines[0].clone(), lines[1].clone()))
    }

    fn fillet(&mut self, pid: i64, keep: bool) {
        let Some((l0, l1)) = self.corner_lines(pid) else { self.say("Eine Verrundungsecke muss genau zwei Linien verbinden.", MsgKind::Warning); return };
        let p = self.sketch.point(pid);
        let other = |e: &SkEntity| if e.pts[0] == pid { e.pts[1] } else { e.pts[0] };
        let (a, b) = (self.sketch.point(other(&l0)), self.sketch.point(other(&l1)));
        let (u1, u2) = (safe_norm2(a - p, V2::X), safe_norm2(b - p, V2::Y));
        let th = u1.dot(u2).clamp(-1.0, 1.0).acos() / 2.0;
        if th < 1e-3 || th > std::f64::consts::FRAC_PI_2 - 1e-3 { self.say("Die Linien sind parallel.", MsgKind::Warning); return; }
        let r = 0.25 * (a - p).length().min((b - p).length()) * th.tan();
        let t = r / th.tan();
        let (t1, t2) = (p + u1 * t, p + u2 * t);
        let c = p + safe_norm2(u1 + u2, u1) * (r / th.sin());
        self.push_undo();
        let (i1, i2) = (self.sketch.add_point(t1), self.sketch.add_point(t2));
        let ci = self.sketch.add_point(c);
        for (e, np) in [(&l0, i1), (&l1, i2)] {
            if let Some(x) = self.sketch.entities.iter_mut().find(|x| x.id == e.id) { for q in &mut x.pts { if *q == pid { *q = np; } } }
        }
        let (s0, en) = if cross2(t1 - c, t2 - c) >= 0.0 { (i1, i2) } else { (i2, i1) };
        let arc = self.sketch.add_arc(ci, s0, en);
        self.con(SkCType::Tangent, vec![], vec![l0.id, arc]);
        self.con(SkCType::Tangent, vec![], vec![l1.id, arc]);
        if keep { self.keep_corner(pid, i1, i2); }
        self.drop_point_if_unused(pid);
        if !self.solve() { self.say("Verrundung konnte nicht gelöst werden.", MsgKind::Warning); }
    }

    /// Krümmungsstetige Verrundung: the corner becomes one corner-blend entity between a point on
    /// each line; the corner point stays on both lines, so the blend always leaves them tangentially
    /// and with zero curvature. Its size is driven by the two end points, like any trimmed corner.
    fn fillet_smooth(&mut self, pid: i64, shape: crate::kernel::corner::CornerShape) {
        let Some((l0, l1)) = self.corner_lines(pid) else { self.say("Eine Verrundungsecke muss genau zwei Linien verbinden.", MsgKind::Warning); return };
        let p = self.sketch.point(pid);
        let other = |e: &SkEntity| if e.pts[0] == pid { e.pts[1] } else { e.pts[0] };
        let (a, b) = (self.sketch.point(other(&l0)), self.sketch.point(other(&l1)));
        if cross2(a - p, b - p).abs() < 1e-9 { self.say("Die Linien sind parallel.", MsgKind::Warning); return; }
        // the same setback on both lines keeps the shape symmetric
        let t = 0.4 * (a - p).length().min((b - p).length());
        let (t1, t2) = (p + safe_norm2(a - p, V2::X) * t, p + safe_norm2(b - p, V2::Y) * t);
        self.push_undo();
        let (i1, i2) = (self.sketch.add_point(t1), self.sketch.add_point(t2));
        for (e, np) in [(&l0, i1), (&l1, i2)] {
            if let Some(x) = self.sketch.entities.iter_mut().find(|x| x.id == e.id) { for q in &mut x.pts { if *q == pid { *q = np; } } }
        }
        let cons = self.construction;
        self.sketch.add_entity(SkKind::Corner, vec![i1, i2, pid], shape.code(), cons, false);
        self.con(SkCType::PointOn, vec![pid], vec![l0.id]);
        self.con(SkCType::PointOn, vec![pid], vec![l1.id]);
        self.solve();
    }

    fn chamfer(&mut self, pid: i64, keep: bool) {
        let Some((l0, l1)) = self.corner_lines(pid) else { self.say("Eine Fasenecke muss genau zwei Linien verbinden.", MsgKind::Warning); return };
        let p = self.sketch.point(pid);
        let other = |e: &SkEntity| if e.pts[0] == pid { e.pts[1] } else { e.pts[0] };
        let (a, b) = (self.sketch.point(other(&l0)), self.sketch.point(other(&l1)));
        let d = 0.25 * (a - p).length().min((b - p).length());
        let (t1, t2) = (p + safe_norm2(a - p, V2::X) * d, p + safe_norm2(b - p, V2::Y) * d);
        self.push_undo();
        let (i1, i2) = (self.sketch.add_point(t1), self.sketch.add_point(t2));
        for (e, np) in [(&l0, i1), (&l1, i2)] {
            if let Some(x) = self.sketch.entities.iter_mut().find(|x| x.id == e.id) { for q in &mut x.pts { if *q == pid { *q = np; } } }
        }
        self.add_line(i1, i2);
        if keep { self.keep_corner(pid, i1, i2); }
        self.drop_point_if_unused(pid);
        self.solve();
    }

    /// Kreisförmig / Fase: the trimmed corner stays as two construction lines.
    fn keep_corner(&mut self, pid: i64, i1: i64, i2: i64) {
        for i in [i1, i2] { self.sketch.add_entity(SkKind::Line, vec![i, pid], 0.0, true, false); }
    }

    /// Elliptische Verrundung: a quarter ellipse tangent to both lines — a conic with the corner as
    /// control point and rho √2 − 1; the corner stays on both lines, so the ends stay tangent.
    fn fillet_elliptic(&mut self, pid: i64, keep: bool) {
        let Some((l0, l1)) = self.corner_lines(pid) else { self.say("Eine Verrundungsecke muss genau zwei Linien verbinden.", MsgKind::Warning); return };
        let p = self.sketch.point(pid);
        let other = |e: &SkEntity| if e.pts[0] == pid { e.pts[1] } else { e.pts[0] };
        let (a, b) = (self.sketch.point(other(&l0)), self.sketch.point(other(&l1)));
        if cross2(a - p, b - p).abs() < 1e-9 { self.say("Die Linien sind parallel.", MsgKind::Warning); return; }
        let (t1, t2) = (p + (a - p) * 0.3, p + (b - p) * 0.2);
        self.push_undo();
        let (i1, i2) = (self.sketch.add_point(t1), self.sketch.add_point(t2));
        for (e, np) in [(&l0, i1), (&l1, i2)] {
            if let Some(x) = self.sketch.entities.iter_mut().find(|x| x.id == e.id) { for q in &mut x.pts { if *q == pid { *q = np; } } }
        }
        self.sketch.add_entity(SkKind::Conic, vec![i1, i2, pid], std::f64::consts::SQRT_2 - 1.0, false, false);
        self.con(SkCType::PointOn, vec![pid], vec![l0.id]);
        self.con(SkCType::PointOn, vec![pid], vec![l1.id]);
        if keep { self.keep_corner(pid, i1, i2); }
        self.drop_point_if_unused(pid);
        self.solve();
    }

    /// Circle or arc under the cursor (for the tangent and concentric tools).
    fn hit_round(&self, s: Pos2) -> Option<i64> {
        self.hit_entity(s, false).filter(|id| matches!(self.sketch.entity(*id).map(|e| e.kind), Some(SkKind::Circle | SkKind::Arc)))
    }

    /// Tools that pick existing entities first.
    fn click_pick_tool(&mut self, s: Pos2, sn: Snap) {
        let p = self.from_screen(s);
        match self.tool {
            Tool::LineTangent | Tool::CenterlineTangent => {
                let Some(id) = self.hit_round(s) else { self.say("Einen Kreis oder Bogen wählen.", MsgKind::Warning); return };
                self.picks.push(SkSel::Ent(id));
                self.clicks.push(Snap { pos: p, point: None, on: Some(id) });
                if self.picks.len() == 2 {
                    let (picks, clicks) = (std::mem::take(&mut self.picks), std::mem::take(&mut self.clicks));
                    let (SkSel::Ent(a), SkSel::Ent(b)) = (picks[0], picks[1]) else { return };
                    self.tangent_line(a, b, clicks[0].pos, clicks[1].pos, self.tool == Tool::CenterlineTangent);
                }
            }
            Tool::CircleConc | Tool::ArcConc => {
                if self.picks.is_empty() {
                    let Some(id) = self.hit_round(s) else { self.say("Einen Kreis oder Bogen wählen.", MsgKind::Warning); return };
                    self.picks.push(SkSel::Ent(id));
                    return;
                }
                let SkSel::Ent(base) = self.picks[0] else { return };
                let Some(cid) = self.sketch.entity(base).map(|e| e.pts[0]) else { self.picks.clear(); return };
                let c = self.sketch.point(cid);
                if self.tool == Tool::CircleConc {
                    let r = (sn.pos - c).length();
                    if r < 1e-9 { return; }
                    self.push_undo();
                    let cons = self.construction;
                    self.sketch.add_entity(SkKind::Circle, vec![cid], r, cons, false);
                    self.solve();
                    return;
                }
                self.clicks.push(sn);
                if self.clicks.len() < 2 { return; }
                let (c0, mut c1) = (self.clicks[0], self.clicks[1]);
                self.clicks.clear();
                self.picks.clear();
                let r = (c0.pos - c).length();
                if r < 1e-9 { return; }
                c1.pos = c + safe_norm2(c1.pos - c, V2::X) * r;
                c1.point = None;
                self.push_undo();
                let sid = self.resolve(&c0);
                let eid = self.sketch.add_point(c1.pos);
                let cons = self.construction;
                self.sketch.add_entity(SkKind::Arc, vec![cid, sid, eid], 0.0, cons, false);
                self.solve();
            }
            Tool::Circle3T | Tool::Arc3T => {
                let Some(id) = self.hit_entity(s, true).filter(|id| *id < 0 || matches!(self.sketch.entity(*id).map(|e| e.kind), Some(SkKind::Line | SkKind::Circle | SkKind::Arc))) else {
                    self.say("Eine Linie, einen Kreis oder einen Bogen wählen.", MsgKind::Warning); return
                };
                self.picks.push(SkSel::Ent(id));
                self.clicks.push(Snap { pos: p, point: None, on: Some(id) });
                if self.picks.len() == 3 {
                    let (picks, clicks) = (std::mem::take(&mut self.picks), std::mem::take(&mut self.clicks));
                    let ents: Vec<i64> = picks.iter().filter_map(|x| if let SkSel::Ent(e) = x { Some(*e) } else { None }).collect();
                    let pts: Vec<V2> = clicks.iter().map(|c| c.pos).collect();
                    self.tangent_round(&ents, &pts, self.tool == Tool::Arc3T);
                }
            }
            Tool::Replace(from) => {
                let Some(to) = self.hit_entity(s, false).filter(|t| *t != from) else { self.say("Ein anderes Element wählen.", MsgKind::Warning); return };
                self.replace_entity(from, to);
                self.tool = Tool::Select;
            }
            Tool::InfoEntity => {
                let Some(id) = self.hit_entity(s, false) else { return };
                let msg = self.entity_info(id);
                self.say(msg, MsgKind::Info);
            }
            Tool::InfoIntersect | Tool::InfoTangent => {
                let Some(id) = self.hit_entity(s, false) else { return };
                self.picks.push(SkSel::Ent(id));
                if self.picks.len() < 2 { return; }
                let picks = std::mem::take(&mut self.picks);
                let (SkSel::Ent(a), SkSel::Ent(b)) = (picks[0], picks[1]) else { return };
                let msg = if self.tool == Tool::InfoIntersect { self.intersect_info(a, b) } else { self.tangent_info(a, b) };
                self.say(msg, MsgKind::Info);
            }
            _ => {}
        }
    }

    /// Linientangentiale: the common tangent of two circles/arcs nearest to the picked spots.
    fn tangent_line(&mut self, a: i64, b: i64, pa: V2, pb: V2, centerline: bool) {
        let (Some(ea), Some(eb)) = (self.sketch.entity(a).cloned(), self.sketch.entity(b).cloned()) else { return };
        let (c1, r1, c2, r2) = (self.sketch.point(ea.pts[0]), self.sketch.radius(&ea), self.sketch.point(eb.pts[0]), self.sketch.radius(&eb));
        let Some((t1, t2)) = common_tangents(c1, r1, c2, r2).into_iter().min_by(|x, y| {
            let f = |t: &(V2, V2)| (t.0 - pa).length_squared() + (t.1 - pb).length_squared();
            f(x).partial_cmp(&f(y)).unwrap()
        }) else { self.say("Die Kreise haben keine gemeinsame Tangente.", MsgKind::Warning); return };
        self.push_undo();
        let (i1, i2) = (self.sketch.add_point(t1), self.sketch.add_point(t2));
        let id = if centerline { self.sketch.add_entity(SkKind::Line, vec![i1, i2], 0.0, true, true) } else { self.add_line(i1, i2) };
        self.con(SkCType::PointOn, vec![i1], vec![a]);
        self.con(SkCType::PointOn, vec![i2], vec![b]);
        self.con(SkCType::Tangent, vec![], vec![id, a]);
        self.con(SkCType::Tangent, vec![], vec![id, b]);
        self.solve();
    }

    /// Kreis / Bogen „3 Tangenten“: start from the circle through the picked spots and let the
    /// solver make it tangent to all three elements.
    fn tangent_round(&mut self, ents: &[i64], pts: &[V2], arc: bool) {
        let cen = circumcenter(pts[0], pts[1], pts[2]).unwrap_or((pts[0] + pts[1] + pts[2]) / 3.0);
        let r = (pts.iter().map(|p| (*p - cen).length()).sum::<f64>() / 3.0).max(1e-3);
        let before = self.sketch.clone();
        self.push_undo();
        let ci = self.sketch.add_point(cen);
        let cons = self.construction;
        let id = if arc {
            let on = |p: V2| cen + safe_norm2(p - cen, V2::X) * r;
            let (s0, m, e0) = (on(pts[0]), on(pts[1]), on(pts[2]));
            let ang = |p: V2| (p.y - cen.y).atan2(p.x - cen.x);
            let tau = std::f64::consts::TAU;
            let rel = |x: f64| ((x - ang(s0)) % tau + tau) % tau;
            let (st, en) = if rel(ang(m)) < rel(ang(e0)) { (s0, e0) } else { (e0, s0) };
            let (si, ei) = (self.sketch.add_point(st), self.sketch.add_point(en));
            let id = self.sketch.add_entity(SkKind::Arc, vec![ci, si, ei], 0.0, cons, false);
            let (first, last) = if st == s0 { (ents[0], ents[2]) } else { (ents[2], ents[0]) };
            self.con(SkCType::PointOn, vec![si], vec![first]);
            self.con(SkCType::PointOn, vec![ei], vec![last]);
            id
        } else { self.sketch.add_entity(SkKind::Circle, vec![ci], r, cons, false) };
        for &e in ents {
            let mut k = SkConstraint::new(SkCType::Tangent, vec![], vec![id, e]);
            if let Some(x) = self.sketch.entity(e).filter(|x| x.kind != SkKind::Line) {
                // inside the other circle: internal tangency
                if (cen - self.sketch.point(x.pts[0])).length() < self.sketch.radius(x) { k.aux = 1.0; }
            }
            self.sketch.add_constraint(k);
        }
        if !self.solve() {
            self.sketch = before;
            self.undo.pop();
            self.after_change();
            self.say("Für diese Elemente gibt es keine tangentiale Lösung in der Nähe der gewählten Stellen.", MsgKind::Warning);
        }
    }

    /// Geneigtes Rechteck, Parallelogramm, Konisch and Achsenendenellipse (three clicks each).
    fn three_point_shape(&mut self, c: &[Snap]) {
        let (p0, p1, p2) = (c[0].pos, c[1].pos, c[2].pos);
        if (p1 - p0).length() < 1e-9 { return; }
        match self.tool {
            Tool::SlantRect | Tool::Parallelogram => {
                let (q2, q3) = if self.tool == Tool::SlantRect {
                    let u = safe_norm2(p1 - p0, V2::X);
                    let n = perp_left(u) * cross2(u, p2 - p0);
                    (p1 + n, p0 + n)
                } else { (p2, p0 + (p2 - p1)) };
                if cross2(p1 - p0, q2 - p1).abs() < 1e-9 { return; }
                self.push_undo();
                let a = self.resolve(&c[0]);
                let b = self.resolve(&c[1]);
                let cc = if self.tool == Tool::Parallelogram { self.resolve(&c[2]) } else { self.sketch.add_point(q2) };
                let d = self.sketch.add_point(q3);
                let l1 = self.add_line(a, b);
                let l2 = self.add_line(b, cc);
                let l3 = self.add_line(cc, d);
                let l4 = self.add_line(d, a);
                if self.tool == Tool::SlantRect { self.con(SkCType::Perpendicular, vec![], vec![l1, l2]); }
                self.con(SkCType::Parallel, vec![], vec![l1, l3]);
                self.con(SkCType::Parallel, vec![], vec![l2, l4]);
                self.solve();
            }
            Tool::Conic => {
                // conic through start, end and the shoulder point; rho 0.5 (editable dimension)
                let ctrl = p2 * 2.0 - (p0 + p1) * 0.5;
                self.push_undo();
                let a = self.resolve(&c[0]);
                let b = self.resolve(&c[1]);
                let ci = self.sketch.add_point(ctrl);
                let cons = self.construction;
                self.sketch.add_entity(SkKind::Conic, vec![a, b, ci], 0.5, cons, false);
                self.solve();
            }
            Tool::EllipseAxis => {
                let cen = (p0 + p1) * 0.5;
                let ry = cross2(safe_norm2(p1 - p0, V2::X), p2 - cen).abs();
                if ry < 1e-9 { return; }
                self.push_undo();
                let ci = self.sketch.add_point(cen);
                let a = self.resolve(&c[1]);
                let cons = self.construction;
                self.sketch.add_entity(SkKind::Ellipse, vec![ci, a], ry, cons, false);
                self.solve();
            }
            _ => {}
        }
    }

    /// Ersetzen: dimensions and constraints of `from` move to `to`, then `from` is deleted.
    fn replace_entity(&mut self, from: i64, to: i64) {
        let (Some(a), Some(b)) = (self.sketch.entity(from).cloned(), self.sketch.entity(to).cloned()) else { return };
        if a.kind != b.kind { self.say("Nur durch ein Element derselben Art ersetzbar.", MsgKind::Warning); return; }
        self.push_undo();
        let map: Vec<(i64, i64)> = a.pts.iter().copied().zip(b.pts.iter().copied()).collect();
        for c in &mut self.sketch.constraints {
            for e in &mut c.ents { if *e == from { *e = to; } }
            for p in &mut c.pts { if let Some(m) = map.iter().find(|m| m.0 == *p) { *p = m.1; } }
        }
        self.sketch.delete_entity(from);
        self.selection.clear();
        self.say("Element ersetzt.", MsgKind::Success);
        self.solve();
    }

    fn entity_info(&self, id: i64) -> String {
        let sk = &self.sketch;
        let Some(e) = sk.entity(id) else { return String::new() };
        let f = |v: f64| fmt_num(v, 3);
        match e.kind {
            SkKind::Line => {
                let (a, b) = sk.line_ends(id).unwrap_or_default();
                let ang = (b.y - a.y).atan2(b.x - a.x).to_degrees();
                format!("{}: Länge {}, Winkel {}°, von ({}, {}) bis ({}, {}).", if e.centerline { "Mittellinie" } else { "Linie" }, f((b - a).length()), f(ang), f(a.x), f(a.y), f(b.x), f(b.y))
            }
            SkKind::Circle => { let c = sk.point(e.pts[0]); format!("Kreis: Mittelpunkt ({}, {}), Radius {}, Durchmesser {}.", f(c.x), f(c.y), f(e.r), f(2.0 * e.r)) }
            SkKind::Arc => {
                let c = sk.point(e.pts[0]);
                let (a0, a1) = sk.arc_angles(e);
                let mut sweep = (a1 - a0).to_degrees();
                if sweep < 0.0 { sweep += 360.0; }
                format!("Bogen: Mittelpunkt ({}, {}), Radius {}, Winkel {}°, Bogenlänge {}.", f(c.x), f(c.y), f(sk.radius(e)), f(sweep), f(sk.radius(e) * sweep.to_radians()))
            }
            SkKind::Ellipse => { let c = sk.point(e.pts[0]); format!("Ellipse: Mittelpunkt ({}, {}), Halbachsen {} und {}.", f(c.x), f(c.y), f((sk.point(e.pts[1]) - c).length()), f(e.r)) }
            SkKind::Spline => format!("Spline mit {} Punkten, Länge {}.", e.pts.len(), f(polyline_len(&sk.sample(e, 96)))),
            SkKind::Point => { let c = sk.point(e.pts[0]); format!("Punkt ({}, {}).", f(c.x), f(c.y)) }
            SkKind::Conic => format!("Kegelschnitt: Rho {}, Länge {}.", f(e.r), f(polyline_len(&sk.sample(e, 96)))),
            SkKind::Corner => format!("Krümmungsstetige Ecke ({}), Länge {}.", crate::kernel::corner::CornerShape::from_code(e.r).label(), f(polyline_len(&sk.sample(e, 96)))),
            SkKind::Text => format!("Text \"{}\": Höhe {}.", e.text.as_ref().map(|t| t.text.as_str()).unwrap_or(""), f((sk.point(e.pts[1]) - sk.point(e.pts[0])).length())),
            SkKind::Csys => { let c = sk.point(e.pts[0]); format!("Koordinatensystem ({}, {}).", f(c.x), f(c.y)) }
        }
    }

    fn intersect_info(&self, a: i64, b: i64) -> String {
        let pts = self.crossings(a, b);
        if pts.is_empty() { return "Die Elemente schneiden sich nicht.".into(); }
        let list: Vec<String> = pts.iter().map(|(p, ang)| format!("({}, {}) unter {}°", fmt_num(p.x, 3), fmt_num(p.y, 3), fmt_num(*ang, 2))).collect();
        format!("Schnittpunkt: {}.", list.join("; "))
    }

    fn tangent_info(&self, a: i64, b: i64) -> String {
        let (Some(ea), Some(eb)) = (self.sketch.entity(a), self.sketch.entity(b)) else { return String::new() };
        let (pa, pb) = (self.sketch.sample(ea, 192), self.sketch.sample(eb, 192));
        let mut best = (f64::INFINITY, V2::ZERO);
        for q in &pa { for w in pb.windows(2) { let (d, x) = seg_point_dist(*q, w[0], w[1]); if d < best.0 { best = (d, x); } } }
        let scale = self.sketch.bounds().map(|(lo, hi)| (hi - lo).length()).unwrap_or(1.0).max(1.0);
        if best.0 > scale * 2e-3 || !self.crossings(a, b).iter().all(|c| c.1 < 5.0) {
            return "Die Elemente sind nicht tangential.".into();
        }
        format!("Tangentialpunkt: ({}, {}).", fmt_num(best.1.x, 3), fmt_num(best.1.y, 3))
    }

    /// Intersections of two entities (sampled), with the crossing angle in degrees.
    fn crossings(&self, a: i64, b: i64) -> Vec<(V2, f64)> {
        let (Some(ea), Some(eb)) = (self.sketch.entity(a), self.sketch.entity(b)) else { return vec![] };
        let (pa, pb) = (self.sketch.sample(ea, 192), self.sketch.sample(eb, 192));
        let mut out: Vec<(V2, f64)> = vec![];
        for s in pa.windows(2) {
            for t in pb.windows(2) {
                if let Some(x) = seg_intersection(s[0], s[1], t[0], t[1]) {
                    let (u, v) = (safe_norm2(s[1] - s[0], V2::X), safe_norm2(t[1] - t[0], V2::X));
                    let ang = cross2(u, v).abs().atan2(u.dot(v).abs()).to_degrees();
                    if !out.iter().any(|o| (o.0 - x).length() < 1e-6) { out.push((x, ang)); }
                }
            }
        }
        out
    }

    /// Prüfen: overlapping entities, crossings that are not shared vertices, and vertices with
    /// more than two entities (branches).
    pub fn check_marks(&self) -> (Vec<i64>, Vec<V2>, Vec<V2>) {
        let sk = &self.sketch;
        let geo: Vec<&SkEntity> = sk.entities.iter().filter(|e| !e.construction && !e.centerline && !matches!(e.kind, SkKind::Point | SkKind::Text | SkKind::Csys)).collect();
        let tol = sk.bounds().map(|(lo, hi)| (hi - lo).length()).unwrap_or(1.0).max(1.0) * 1e-6;
        let mut overlap = vec![];
        let mut cross = vec![];
        for i in 0..geo.len() {
            for j in i + 1..geo.len() {
                let (a, b) = (geo[i], geo[j]);
                let ov = match (a.kind, b.kind) {
                    (SkKind::Line, SkKind::Line) => {
                        let ((a0, a1), (b0, b1)) = (sk.line_ends(a.id).unwrap(), sk.line_ends(b.id).unwrap());
                        let u = safe_norm2(a1 - a0, V2::X);
                        let col = cross2(u, b0 - a0).abs() < tol * 10.0 && cross2(u, b1 - a0).abs() < tol * 10.0;
                        let (t0, t1) = ((b0 - a0).dot(u), (b1 - a0).dot(u));
                        col && t0.max(t1) > tol && t0.min(t1) < (a1 - a0).length() - tol
                    }
                    (SkKind::Circle | SkKind::Arc, SkKind::Circle | SkKind::Arc) => {
                        (sk.point(a.pts[0]) - sk.point(b.pts[0])).length() < tol && (sk.radius(a) - sk.radius(b)).abs() < tol
                    }
                    _ => false,
                };
                if ov { overlap.push(a.id); overlap.push(b.id); continue; }
                let shared: Vec<V2> = a.pts.iter().filter(|p| b.pts.contains(p)).map(|p| sk.point(*p)).collect();
                for (x, _) in self.crossings(a.id, b.id) {
                    let ends = |e: &SkEntity| -> Vec<V2> { match e.kind { SkKind::Line | SkKind::Spline => vec![sk.point(e.pts[0]), sk.point(*e.pts.last().unwrap())], SkKind::Arc => vec![sk.point(e.pts[1]), sk.point(e.pts[2])], _ => vec![] } };
                    let at_end = ends(a).iter().chain(ends(b).iter()).chain(shared.iter()).any(|q| (*q - x).length() < tol * 1e3);
                    if !at_end { cross.push(x); }
                }
            }
        }
        let branches = sk.points.iter().filter(|p| geo.iter().filter(|e| match e.kind { SkKind::Line | SkKind::Spline => e.pts.first() == Some(&p.id) || e.pts.last() == Some(&p.id), SkKind::Arc => e.pts[1] == p.id || e.pts[2] == p.id, _ => false }).count() > 2).map(|p| p.p).collect();
        (overlap, cross, branches)
    }

    /// Auswählen ▸ Kette: the entities connected to `id` through shared end points.
    fn chain_of(&self, id: i64) -> Vec<i64> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            let pts = self.sketch.entity(out[i]).map(|e| e.pts.clone()).unwrap_or_default();
            for e in &self.sketch.entities {
                if !out.contains(&e.id) && e.kind != SkKind::Point && e.pts.iter().any(|p| pts.contains(p) && *p != 0) && !e.centerline { out.push(e.id); }
            }
            i += 1;
        }
        out
    }

    /// Auswählen ▸ Alle Geometrie (`with_annotations` false) / Alle (Strg+Alt+A).
    pub fn select_all(&mut self, with_annotations: bool) {
        self.tool = Tool::Select;
        self.selection = self.sketch.entities.iter().map(|e| SkSel::Ent(e.id)).collect();
        if with_annotations { self.selection.extend(self.sketch.constraints.iter().filter(|c| !c.weak).map(|c| SkSel::Con(c.id))); }
    }

    /// Operationen ▸ Sperre umschalten for the selected dimensions (weak ones become strong).
    pub fn toggle_lock(&mut self) {
        let ids: Vec<i64> = self.selection.iter().filter_map(|s| if let SkSel::Con(c) = s { Some(*c) } else { None }).collect();
        if ids.is_empty() { return; }
        self.push_undo();
        for c in &mut self.sketch.constraints {
            if ids.contains(&c.id) && c.ty.is_dimension() { c.locked = !c.locked; if c.locked { c.weak = false; } }
        }
        self.show_locks = true;
        self.after_change();
    }

    /// Operationen ▸ Konvertieren in ▸ Referenz: the dimension no longer drives the sketch.
    pub fn convert_to_reference(&mut self) {
        let ids: Vec<i64> = self.selection.iter().filter_map(|s| if let SkSel::Con(c) = s { Some(*c) } else { None }).collect();
        self.push_undo();
        for c in &mut self.sketch.constraints { if ids.contains(&c.id) && c.ty.is_dimension() { c.reference = true; c.weak = false; c.locked = false; } }
        self.after_change();
    }

    /// Operationen ▸ Konvertieren in ▸ Stark.
    pub fn strengthen_selection(&mut self) {
        let ids: Vec<i64> = self.selection.iter().filter_map(|s| if let SkSel::Con(c) = s { Some(*c) } else { None }).collect();
        if !self.sketch.constraints.iter().any(|c| ids.contains(&c.id) && (c.weak || c.reference)) { return; }
        self.push_undo();
        for c in &mut self.sketch.constraints { if ids.contains(&c.id) { c.weak = false; c.reference = false; } }
        self.after_change();
    }

    pub fn start_replace(&mut self) {
        let ents: Vec<i64> = self.selection.iter().filter_map(|s| if let SkSel::Ent(e) = s { Some(*e) } else { None }).collect();
        if ents.len() != 1 { self.say("Genau ein Element wählen, das ersetzt werden soll.", MsgKind::Warning); return; }
        self.set_tool(Tool::Replace(ents[0]));
    }

    pub fn has_selected(&self, dims: bool) -> bool {
        self.selection.iter().any(|s| match s { SkSel::Con(c) if dims => self.sketch.constraints.iter().any(|k| k.id == *c && k.ty.is_dimension()), SkSel::Ent(_) if !dims => true, _ => false })
    }
    pub fn has_weak_selected(&self) -> bool {
        self.selection.iter().any(|s| matches!(s, SkSel::Con(c) if self.sketch.constraints.iter().any(|k| k.id == *c && (k.weak || k.reference))))
    }

    fn drop_point_if_unused(&mut self, pid: i64) {
        if !self.sketch.entities.iter().any(|e| e.pts.contains(&pid)) {
            self.sketch.points.retain(|q| q.id != pid);
            self.sketch.constraints.retain(|k| !k.pts.contains(&pid));
        }
    }

    fn divide(&mut self, id: i64, p: V2) {
        let Some(e) = self.sketch.entity(id).cloned() else { return };
        match e.kind {
            SkKind::Line => {
                let (a, b) = (self.sketch.point(e.pts[0]), self.sketch.point(e.pts[1]));
                let d = b - a;
                let t = ((p - a).dot(d) / d.length_squared().max(1e-12)).clamp(0.05, 0.95);
                self.push_undo();
                let m = self.sketch.add_point(a + d * t);
                let end = e.pts[1];
                if let Some(x) = self.sketch.entities.iter_mut().find(|x| x.id == id) { x.pts[1] = m; }
                let cons = e.construction;
                self.sketch.add_entity(SkKind::Line, vec![m, end], 0.0, cons, false);
                self.sketch.constraints.retain(|c| !(c.ty.is_dimension() && c.ents.contains(&id)));
                self.solve();
            }
            SkKind::Arc => {
                let c = self.sketch.point(e.pts[0]);
                let r = self.sketch.radius(&e);
                self.push_undo();
                let m = self.sketch.add_point(c + safe_norm2(p - c, V2::X) * r);
                let end = e.pts[2];
                if let Some(x) = self.sketch.entities.iter_mut().find(|x| x.id == id) { x.pts[2] = m; }
                let cons = e.construction;
                self.sketch.add_entity(SkKind::Arc, vec![e.pts[0], m, end], 0.0, cons, false);
                self.solve();
            }
            _ => self.say("Nur Linien und Bögen können geteilt werden.", MsgKind::Warning),
        }
    }

    fn corner(&mut self, a: i64, b: i64) {
        let (Some((a1, b1)), Some((a2, b2))) = (self.sketch.line_ends(a), self.sketch.line_ends(b)) else { return };
        let Some(x) = line_intersection(a1, b1, a2, b2) else { self.say("Die Linien sind parallel.", MsgKind::Warning); return };
        let (ea, eb) = (self.sketch.entity(a).unwrap().clone(), self.sketch.entity(b).unwrap().clone());
        let near = |e: &SkEntity| if (self.sketch.point(e.pts[0]) - x).length() < (self.sketch.point(e.pts[1]) - x).length() { e.pts[0] } else { e.pts[1] };
        let (pa, pb) = (near(&ea), near(&eb));
        self.push_undo();
        self.sketch.set_point(pa, x);
        self.sketch.set_point(pb, x);
        if pa != pb { self.sketch.merge_point(pb, pa); }
        self.solve();
    }

    fn mirror(&mut self, line: i64) {
        let Some((a, b)) = self.sketch.line_ends(line) else { return };
        let d = safe_norm2(b - a, V2::Y);
        let reflect = |p: V2| { let q = p - a; a + d * (2.0 * q.dot(d)) - q };
        let ents: Vec<SkEntity> = self.selection.iter().filter_map(|s| if let SkSel::Ent(e) = s { self.sketch.entity(*e).cloned() } else { None }).filter(|e| !e.centerline).collect();
        if ents.is_empty() { return; }
        self.push_undo();
        let mut map: std::collections::HashMap<i64, i64> = Default::default();
        let mut pairs = vec![];
        for e in &ents {
            for &p in &e.pts {
                if map.contains_key(&p) { continue; }
                let pp = self.sketch.point(p);
                if cross2(d, pp - a).abs() < 1e-9 { map.insert(p, p); continue; }
                let np = self.sketch.add_point(reflect(pp));
                map.insert(p, np);
                pairs.push((p, np));
            }
        }
        for e in &ents {
            let mut pts: Vec<i64> = e.pts.iter().map(|p| map[p]).collect();
            if e.kind == SkKind::Arc { pts.swap(1, 2); }
            self.sketch.add_entity(e.kind, pts, e.r, e.construction, false);
        }
        for (p, q) in pairs { self.con(SkCType::Symmetric, vec![p, q], vec![line]); }
        self.selection.clear();
        if !self.solve() { self.say("Gespiegelte Geometrie konnte nicht vollständig gelöst werden.", MsgKind::Warning); }
    }

    /// Parameter (0..1) of the closest point to `p` on a line, arc or circle.
    fn param_of(&self, e: &SkEntity, p: V2) -> f64 {
        let sk = &self.sketch;
        match e.kind {
            SkKind::Line => { let (a, b) = (sk.point(e.pts[0]), sk.point(e.pts[1])); let d = b - a; ((p - a).dot(d) / d.length_squared().max(1e-12)).clamp(0.0, 1.0) }
            SkKind::Arc => {
                let c = sk.point(e.pts[0]);
                let (a0, a1) = sk.arc_angles(e);
                let rel = ((p.y - c.y).atan2(p.x - c.x) - a0).rem_euclid(std::f64::consts::TAU);
                (rel / (a1 - a0)).clamp(0.0, 1.0)
            }
            SkKind::Circle => { let c = sk.point(e.pts[0]); ((p.y - c.y).atan2(p.x - c.x)).rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU }
            _ => 0.0,
        }
    }

    fn point_at(&self, e: &SkEntity, t: f64) -> V2 {
        let sk = &self.sketch;
        match e.kind {
            SkKind::Line => sk.point(e.pts[0]) + (sk.point(e.pts[1]) - sk.point(e.pts[0])) * t,
            SkKind::Arc => { let c = sk.point(e.pts[0]); let (a0, a1) = sk.arc_angles(e); let a = a0 + (a1 - a0) * t; c + V2::new(a.cos(), a.sin()) * sk.radius(e) }
            SkKind::Circle => { let c = sk.point(e.pts[0]); let a = t * std::f64::consts::TAU; c + V2::new(a.cos(), a.sin()) * e.r }
            _ => V2::ZERO,
        }
    }

    /// Intersections of a line/arc/circle with every other entity, as (parameter, other entity).
    fn trim_params(&self, e: &SkEntity) -> Vec<(f64, i64)> {
        let sk = &self.sketch;
        let mut out = vec![];
        for o in &sk.entities {
            if o.id == e.id || o.kind == SkKind::Point { continue; }
            let q = sk.sample(o, 128);
            for w in q.windows(2) {
                let (q0, q1) = (w[0], w[1]);
                match e.kind {
                    SkKind::Line => {
                        let (a, b) = (sk.point(e.pts[0]), sk.point(e.pts[1]));
                        let (d1, d2) = (b - a, q1 - q0);
                        let den = cross2(d1, d2);
                        if den.abs() < 1e-12 { continue; }
                        let t = cross2(q0 - a, d2) / den;
                        let u = cross2(q0 - a, d1) / den;
                        if (-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u) { out.push((t.clamp(0.0, 1.0), o.id)); }
                    }
                    SkKind::Arc | SkKind::Circle => {
                        let c = sk.point(e.pts[0]);
                        let r = sk.radius(e);
                        let d = q1 - q0;
                        let f = q0 - c;
                        let (aa, bb, cc) = (d.dot(d), 2.0 * f.dot(d), f.dot(f) - r * r);
                        let disc = bb * bb - 4.0 * aa * cc;
                        if aa < 1e-18 || disc < 0.0 { continue; }
                        for sgn in [-1.0, 1.0] {
                            let u = (-bb + sgn * disc.sqrt()) / (2.0 * aa);
                            if !(-1e-9..=1.0 + 1e-9).contains(&u) { continue; }
                            let pt = q0 + d * u;
                            if e.kind == SkKind::Circle { out.push((self.param_of(e, pt), o.id)); continue; }
                            let (a0, a1) = sk.arc_angles(e);
                            let rel = ((pt.y - c.y).atan2(pt.x - c.x) - a0).rem_euclid(std::f64::consts::TAU);
                            if rel <= a1 - a0 + 1e-9 { out.push(((rel / (a1 - a0)).clamp(0.0, 1.0), o.id)); }
                        }
                    }
                    _ => {}
                }
            }
        }
        out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        out.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-7);
        out
    }

    /// Creo's Delete Segment: remove the part of an entity between its neighbouring intersections.
    fn trim_at(&mut self, id: i64, click: V2) {
        let Some(e) = self.sketch.entity(id).cloned() else { return };
        if !matches!(e.kind, SkKind::Line | SkKind::Arc | SkKind::Circle) {
            self.push_undo(); self.sketch.delete_entity(id); self.solve(); return;
        }
        let cuts = self.trim_params(&e);
        let tc = self.param_of(&e, click);
        let on_cut = |me: &mut Self, pid: i64, other: i64| {
            if me.sketch.entity(other).map(|o| matches!(o.kind, SkKind::Line | SkKind::Arc | SkKind::Circle)).unwrap_or(false) {
                me.sketch.add_constraint(SkConstraint::new(SkCType::PointOn, vec![pid], vec![other]));
            }
        };
        if e.kind == SkKind::Circle {
            let interior: Vec<(f64, i64)> = cuts;
            if interior.len() < 2 { self.push_undo(); self.sketch.delete_entity(id); self.solve(); return; }
            let lo = interior.iter().rev().find(|c| c.0 <= tc).copied().unwrap_or(*interior.last().unwrap());
            let hi = interior.iter().find(|c| c.0 > tc).copied().unwrap_or(interior[0]);
            self.push_undo();
            let center = e.pts[0];
            let (ps, pe) = (self.point_at(&e, hi.0), self.point_at(&e, lo.0));
            let cons = e.construction;
            self.sketch.entities.retain(|x| x.id != id);
            self.sketch.constraints.retain(|c| !c.ents.contains(&id));
            let (a, b) = (self.sketch.add_point(ps), self.sketch.add_point(pe));
            self.sketch.add_entity(SkKind::Arc, vec![center, a, b], 0.0, cons, false);
            on_cut(self, a, hi.1);
            on_cut(self, b, lo.1);
            self.solve();
            return;
        }
        let inner: Vec<(f64, i64)> = cuts.into_iter().filter(|c| c.0 > 1e-6 && c.0 < 1.0 - 1e-6).collect();
        let lo = inner.iter().rev().find(|c| c.0 <= tc).copied();
        let hi = inner.iter().find(|c| c.0 > tc).copied();
        let (si, ei) = if e.kind == SkKind::Line { (0, 1) } else { (1, 2) };
        self.push_undo();
        match (lo, hi) {
            (None, None) => { self.sketch.delete_entity(id); }
            (None, Some(h)) => {
                let np = self.sketch.add_point(self.point_at(&e, h.0));
                let old = e.pts[si];
                if let Some(x) = self.sketch.entities.iter_mut().find(|x| x.id == id) { x.pts[si] = np; }
                on_cut(self, np, h.1);
                self.sketch.constraints.retain(|c| !(c.ty.is_dimension() && c.ents.contains(&id)));
                self.drop_point_if_unused(old);
            }
            (Some(l), None) => {
                let np = self.sketch.add_point(self.point_at(&e, l.0));
                let old = e.pts[ei];
                if let Some(x) = self.sketch.entities.iter_mut().find(|x| x.id == id) { x.pts[ei] = np; }
                on_cut(self, np, l.1);
                self.sketch.constraints.retain(|c| !(c.ty.is_dimension() && c.ents.contains(&id)));
                self.drop_point_if_unused(old);
            }
            (Some(l), Some(h)) => {
                let (m1, m2) = (self.sketch.add_point(self.point_at(&e, l.0)), self.sketch.add_point(self.point_at(&e, h.0)));
                let end = e.pts[ei];
                if let Some(x) = self.sketch.entities.iter_mut().find(|x| x.id == id) { x.pts[ei] = m1; }
                let mut pts = e.pts.clone();
                pts[si] = m2;
                pts[ei] = end;
                let nid = self.sketch.add_entity(e.kind, pts, e.r, e.construction, false);
                let copies: Vec<SkConstraint> = self.sketch.constraints.iter().filter(|c| matches!(c.ty, SkCType::Horizontal | SkCType::Vertical) && c.ents == vec![id]).cloned().collect();
                for mut c in copies { c.ents = vec![nid]; self.sketch.add_constraint(c); }
                self.sketch.constraints.retain(|c| !(c.ty.is_dimension() && c.ents.contains(&id)));
                on_cut(self, m1, l.1);
                on_cut(self, m2, h.1);
            }
        }
        self.solve();
    }

    fn kind_of(&self, s: SkSel) -> Option<SkKind> {
        match s {
            SkSel::Ent(id) if id < 0 => Some(SkKind::Line),
            SkSel::Ent(id) => self.sketch.entity(id).map(|e| e.kind),
            SkSel::Point(_) => Some(SkKind::Point),
            _ => None,
        }
    }

    /// Adds the constraint when the picks fit; returns true when consumed.
    fn try_constraint(&mut self, c: SkCType) -> bool {
        use SkKind::*;
        let picks = self.picks.clone();
        let ents: Vec<i64> = picks.iter().filter_map(|p| if let SkSel::Ent(e) = p { Some(*e) } else { None }).collect();
        let pts: Vec<i64> = picks.iter().filter_map(|p| if let SkSel::Point(e) = p { Some(*e) } else { None }).collect();
        let kinds: Vec<SkKind> = picks.iter().filter_map(|p| self.kind_of(*p)).collect();
        let is_curve = |k: &SkKind| matches!(k, Circle | Arc);
        let k = match c {
            SkCType::Horizontal | SkCType::Vertical => {
                if ents.len() == 1 && kinds.first() == Some(&Line) { Some(SkConstraint::new(c, vec![], ents.clone())) }
                else if pts.len() == 2 { Some(SkConstraint::new(c, pts.clone(), vec![])) } else { None }
            }
            SkCType::Parallel | SkCType::Perpendicular if ents.len() == 2 && kinds.iter().all(|k| *k == Line) => Some(SkConstraint::new(c, vec![], ents.clone())),
            SkCType::Equal if ents.len() == 2 && (kinds.iter().all(|k| *k == Line) || kinds.iter().all(is_curve)) => Some(SkConstraint::new(c, vec![], ents.clone())),
            SkCType::Tangent if ents.len() == 2 && kinds.iter().any(is_curve) => {
                let mut k = SkConstraint::new(c, vec![], ents.clone());
                if kinds.iter().all(is_curve) {
                    let e0 = self.sketch.entity(ents[0]).unwrap().clone();
                    let e1 = self.sketch.entity(ents[1]).unwrap().clone();
                    let d = (self.sketch.point(e0.pts[0]) - self.sketch.point(e1.pts[0])).length();
                    let (r0, r1) = (self.sketch.radius(&e0), self.sketch.radius(&e1));
                    k.aux = if (d - (r0 - r1).abs()).abs() < (d - (r0 + r1)).abs() { 1.0 } else { 0.0 };
                }
                Some(k)
            }
            SkCType::Coincident if pts.len() == 2 => Some(SkConstraint::new(c, pts.clone(), vec![])),
            SkCType::Coincident if pts.len() == 1 && ents.len() == 1 => Some(SkConstraint::new(SkCType::PointOn, pts.clone(), ents.clone())),
            SkCType::Midpoint if pts.len() == 1 && ents.len() == 1 && kinds.contains(&Line) => Some(SkConstraint::new(c, pts.clone(), ents.clone())),
            SkCType::Symmetric if pts.len() == 2 && ents.len() == 1 => Some(SkConstraint::new(c, pts.clone(), ents.clone())),
            SkCType::Fix if pts.len() == 1 => { let mut k = SkConstraint::new(c, pts.clone(), vec![]); k.fixed = Some(self.sketch.point(pts[0])); Some(k) }
            _ => None,
        };
        let needed = match c { SkCType::Fix => 1, SkCType::Symmetric => 3, SkCType::Horizontal | SkCType::Vertical => if pts.is_empty() { 1 } else { 2 }, _ => 2 };
        let Some(k) = k else {
            if picks.len() >= needed { self.picks.clear(); self.say("Diese Elemente können diese Bedingung nicht annehmen.", MsgKind::Warning); }
            return false;
        };
        self.picks.clear();
        self.add_checked(k);
        true
    }

    fn add_checked(&mut self, k: SkConstraint) -> bool {
        let before = self.sketch.clone();
        let rank0 = solver::rank(&self.sketch);
        self.sketch.add_constraint(k);
        let rank1 = solver::rank(&self.sketch);
        if rank1 <= rank0 {
            self.sketch = before;
            self.say("Redundante oder widersprüchliche Bedingung — nicht hinzugefügt. Zuerst die widersprüchliche Bedingung löschen.", MsgKind::Warning);
            return false;
        }
        if !solver::solve(&mut self.sketch, None) {
            self.sketch = before;
            self.say("Widersprüchliche Bedingung — die Skizze kann damit nicht gelöst werden.", MsgKind::Error);
            return false;
        }
        self.undo.push(before);
        self.redo.clear();
        self.after_change();
        true
    }

    fn create_dimension(&mut self, picks: &[SkSel], place: V2) {
        let sk = &self.sketch;
        let pts: Vec<i64> = picks.iter().filter_map(|p| if let SkSel::Point(e) = p { Some(*e) } else { None }).collect();
        let ents: Vec<i64> = picks.iter().filter_map(|p| if let SkSel::Ent(e) = p { Some(*e) } else { None }).collect();
        let kind = |id: i64| if id < 0 { Some(SkKind::Line) } else { sk.entity(id).map(|e| e.kind) };
        let mut k: Option<SkConstraint> = None;
        if pts.is_empty() && ents.len() == 1 {
            match kind(ents[0]) {
                Some(SkKind::Line) if ents[0] > 0 => { let e = sk.entity(ents[0]).unwrap(); k = Some(SkConstraint::new(SkCType::Distance, e.pts.clone(), ents.clone())); }
                Some(SkKind::Circle) => k = Some(SkConstraint::new(SkCType::Diameter, vec![], ents.clone())),
                Some(SkKind::Arc) | Some(SkKind::Ellipse) => k = Some(SkConstraint::new(SkCType::Radius, vec![], ents.clone())),
                _ => {}
            }
        } else if pts.len() == 2 {
            let (a, b) = (sk.point(pts[0]), sk.point(pts[1]));
            let off = place - (a + b) * 0.5;
            let (dx, dy) = ((b - a).x.abs(), (b - a).y.abs());
            let ty = if dy < 1e-9 { SkCType::HDistance } else if dx < 1e-9 { SkCType::VDistance }
                else if off.y.abs() > off.x.abs() * 1.5 { SkCType::HDistance } else if off.x.abs() > off.y.abs() * 1.5 { SkCType::VDistance } else { SkCType::Distance };
            k = Some(SkConstraint::new(ty, pts.clone(), vec![]));
        } else if pts.len() == 1 && ents.len() == 1 {
            match kind(ents[0]) {
                Some(SkKind::Line) => k = Some(SkConstraint::new(SkCType::PointLine, pts.clone(), ents.clone())),
                Some(SkKind::Circle) | Some(SkKind::Arc) => { let c = sk.entity(ents[0]).unwrap().pts[0]; k = Some(SkConstraint::new(SkCType::Distance, vec![pts[0], c], vec![])); }
                _ => {}
            }
        } else if ents.len() == 2 {
            match (kind(ents[0]), kind(ents[1])) {
                (Some(SkKind::Line), Some(SkKind::Line)) => {
                    let (Some((a1, b1)), Some((a2, b2))) = (sk.line_ends(ents[0]), sk.line_ends(ents[1])) else { return };
                    if cross2(safe_norm2(b1 - a1, V2::X), safe_norm2(b2 - a2, V2::X)).abs() < 1e-6 {
                        let (line, other) = if ents[0] > 0 { (ents[1], ents[0]) } else { (ents[0], ents[1]) };
                        if let Some(p0) = sk.entity(other).map(|e| e.pts[0]) { k = Some(SkConstraint::new(SkCType::PointLine, vec![p0], vec![line])); }
                    } else {
                        let mut c = SkConstraint::new(SkCType::Angle, vec![], ents.clone());
                        if (b1 - a1).dot(b2 - a2) < 0.0 { c.aux = 1.0; }
                        k = Some(c);
                    }
                }
                (Some(a), Some(b)) if matches!(a, SkKind::Circle | SkKind::Arc) && matches!(b, SkKind::Circle | SkKind::Arc) => {
                    let (c1, c2) = (sk.entity(ents[0]).unwrap().pts[0], sk.entity(ents[1]).unwrap().pts[0]);
                    k = Some(SkConstraint::new(SkCType::Distance, vec![c1, c2], vec![]));
                }
                _ => {}
            }
        }
        // Ordinate: baseline (0.00) plus a point or the centre of a circle/arc
        let base = picks.iter().find_map(|p| if let SkSel::Con(c) = p { Some(*c) } else { None });
        if let Some(b) = base {
            let q = pts.first().copied().or_else(|| ents.first().and_then(|e| sk.entity(*e)).filter(|e| matches!(e.kind, SkKind::Circle | SkKind::Arc)).map(|e| e.pts[0]));
            match q {
                Some(q) => { let mut c = SkConstraint::new(SkCType::Ordinate, vec![q], vec![]); c.base = b; k = Some(c); }
                None => { self.say("Einen Punkt oder einen Kreismittelpunkt wählen.", MsgKind::Warning); return; }
            }
        }
        if k.is_none() && pts.is_empty() && ents.len() == 1 && kind(ents[0]) == Some(SkKind::Conic) {
            k = Some(SkConstraint::new(SkCType::Rho, vec![], ents.clone()));
        }
        let Some(mut k) = k else { self.say("Diese Auswahl kann nicht bemaßt werden.", MsgKind::Warning); return };
        k.value = self.sketch.measure(&k);
        k.label = place - dim_anchor(&self.sketch, &k);
        if self.tool == Tool::RefDimension {
            // Referenzbemaßung: shown and kept up to date, never drives the sketch
            self.push_undo();
            k.reference = true;
            self.sketch.add_constraint(k);
            self.after_change();
            return;
        }
        if self.add_checked(k) {
            if let Some(c) = self.sketch.constraints.iter().rev().find(|c| !c.weak && c.ty.is_dimension()) {
                self.edit = Some((c.id, fmt_num(c.value, 3), true));
            }
        }
    }

    pub fn delete_selection(&mut self) {
        if self.selection.is_empty() { return; }
        if self.selection.iter().all(|s| matches!(s, SkSel::Con(id) if self.sketch.constraints.iter().any(|c| c.id == *id && c.weak))) {
            self.say("Schwache Bemaßungen können nicht gelöscht werden; stattdessen eine starke Bemaßung oder Bedingung hinzufügen.", MsgKind::Warning);
            return;
        }
        self.push_undo();
        for s in std::mem::take(&mut self.selection) {
            match s {
                SkSel::Ent(id) if id > 0 => self.sketch.delete_entity(id),
                SkSel::Con(id) => self.sketch.constraints.retain(|c| c.id != id || c.weak),
                SkSel::Point(id) if id != 0 => {
                    let ents: Vec<i64> = self.sketch.entities.iter().filter(|e| e.pts.contains(&id)).map(|e| e.id).collect();
                    for e in ents { self.sketch.delete_entity(e); }
                    self.sketch.points.retain(|q| q.id != id);
                    self.sketch.constraints.retain(|c| !c.pts.contains(&id));
                }
                _ => {}
            }
        }
        self.solve();
    }

    pub fn toggle_construction(&mut self) {
        let ids: Vec<i64> = self.selection.iter().filter_map(|s| if let SkSel::Ent(e) = s { Some(*e) } else { None }).collect();
        if ids.is_empty() { self.construction = !self.construction; return; }
        self.push_undo();
        for e in &mut self.sketch.entities { if ids.contains(&e.id) && !e.centerline { e.construction = !e.construction; } }
        self.after_change();
    }

    fn label_rect(&self, c: &SkConstraint) -> Rect {
        let lp = self.to_screen(dim_anchor(&self.sketch, c) + c.label);
        let w = dim_text(c).chars().count() as f32 * 7.0 + 8.0;
        Rect::from_center_size(lp, egui::vec2(w.max(24.0), 18.0))
    }

    fn glyph_positions(&self) -> Vec<(i64, Pos2, &'static str)> {
        let sk = &self.sketch;
        let mut count: std::collections::HashMap<i64, usize> = Default::default();
        let mut out = vec![];
        for c in &sk.constraints {
            if c.ty.is_dimension() || (c.ty == SkCType::Coincident && c.pts.len() < 2) { continue; }
            let anchor_ent = c.ents.iter().copied().find(|e| *e > 0);
            let (pos, key) = if let Some(e) = anchor_ent {
                let ent = sk.entity(e).unwrap();
                let s = sk.sample(ent, 32);
                (s[s.len() / 2], e)
            } else if let Some(p) = c.pts.first() { (sk.point(*p), -1000 - *p) } else { continue };
            let n = count.entry(key).or_default();
            let sp = self.to_screen(pos) + egui::vec2(10.0 + *n as f32 * 13.0, -12.0);
            *n += 1;
            out.push((c.id, sp, c.ty.glyph()));
        }
        out
    }

    fn hit_dimension(&self, s: Pos2) -> Option<i64> {
        if self.show_dims {
            for c in self.sketch.constraints.iter().rev() {
                if c.ty.is_dimension() && self.label_rect(c).contains(s) { return Some(c.id); }
            }
        }
        if self.show_cons {
            for (id, p, _) in self.glyph_positions() { if (p - s).length() < 8.0 { return Some(id); } }
        }
        None
    }

    fn apply_edit(&mut self, id: i64, text: &str) {
        let t = text.trim();
        let (v, ex) = match t.parse::<f64>() {
            Ok(v) => (v, None),
            Err(_) => match expr::eval(t, &self.vars) {
                Ok(v) => (v, Some(t.to_string())),
                Err(e) => { self.say(format!("Ungültiger Wert: {}", e), MsgKind::Error); return; }
            },
        };
        let Some(c) = self.sketch.constraints.iter().find(|c| c.id == id).cloned() else { return };
        if c.ty == SkCType::Baseline || c.reference || c.var { self.say("Diese Bemaßung wird nicht geändert, sie folgt der Geometrie.", MsgKind::Warning); return; }
        if v <= 0.0 && c.ty != SkCType::Angle { self.say("Die Bemaßung muss positiv sein.", MsgKind::Error); return; }
        let before = self.sketch.clone();
        if let Some(k) = self.sketch.constraints.iter_mut().find(|c| c.id == id) { k.weak = false; k.expr = ex; }
        let start = c.value;
        let mut ok = true;
        for s in 1..=4 {
            let val = start + (v - start) * s as f64 / 4.0;
            if let Some(k) = self.sketch.constraints.iter_mut().find(|c| c.id == id) { k.value = val; }
            if !solver::solve(&mut self.sketch, None) { ok = false; break; }
        }
        if !ok {
            self.sketch = before;
            self.say("Die Skizze kann mit diesem Wert nicht gelöst werden.", MsgKind::Error);
            return;
        }
        self.undo.push(before);
        self.redo.clear();
        self.after_change();
    }

    // ---------- interaction ----------

    /// Handle sketch interaction for this frame (camera navigation is handled by the viewport).
    pub fn interact(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, cam: &Camera) {
        self.rect = rect;
        self.cam = *cam;
        self.update_ppu();
        if !self.weak_ready { self.refresh_weak(); self.weak_ready = true; }
        let (mods, pointer, esc, del) = ui.input(|i| (i.modifiers, i.pointer.clone(), i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)));
        let hp = resp.hover_pos();
        self.cursor = hp;
        let typing = self.edit.is_some() || ui.ctx().wants_keyboard_input();
        if !typing {
            if esc {
                if self.tool == Tool::Spline && self.clicks.len() >= 2 { self.finish_spline(); }
                if !self.clicks.is_empty() || self.chain.is_some() || !self.picks.is_empty() { self.clicks.clear(); self.chain = None; self.picks.clear(); }
                else if self.tool != Tool::Select { self.set_tool(Tool::Select); }
                else { self.selection.clear(); }
            }
            if del { self.delete_selection(); }
            // Creo's sketcher keys: L Linienkette, R Ecken-Rechteck, Strg+Alt+A Alle, Umschalt+G Konstruktion
            let (key_l, key_r, key_a, key_g) = ui.input(|i| (i.key_pressed(egui::Key::L), i.key_pressed(egui::Key::R), i.key_pressed(egui::Key::A), i.key_pressed(egui::Key::G)));
            if !mods.command && !mods.alt && !mods.shift && key_l { self.set_tool(Tool::Line); }
            if !mods.command && !mods.alt && !mods.shift && key_r { self.set_tool(Tool::Rect); }
            if mods.command && mods.alt && key_a { self.select_all(true); }
            if mods.shift && !mods.command && key_g { self.toggle_construction(); }
            let (kc, kx, kv) = ui.input(|i| (i.key_pressed(egui::Key::C), i.key_pressed(egui::Key::X), i.key_pressed(egui::Key::V)));
            if mods.command && !mods.alt && kc { self.copy_selection(false); }
            if mods.command && !mods.alt && kx { self.copy_selection(true); }
            if mods.command && !mods.alt && kv { let at = self.cursor.filter(|c| self.rect.contains(*c)).map(|c| self.from_screen(c)); self.paste(at); }
        }
        if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select && !mods.alt {
            if let Some(start) = pointer.press_origin() {
                let p = self.from_screen(start);
                let label = self.hit_dimension(start).filter(|id| self.sketch.constraints.iter().any(|c| c.id == *id && c.ty.is_dimension()));
                self.drag = if let Some(d) = label { Some(Drag::Label(d)) }
                    else if let Some(pt) = self.hit_point(start, &[0]) { Some(Drag::Point(pt)) }
                    else if let Some(e) = self.hit_entity(start, false) {
                        match self.sketch.entity(e).map(|x| x.kind) { Some(SkKind::Circle) => Some(Drag::Circle(e)), _ => Some(Drag::Ent(e, p)) }
                    } else { Some(Drag::Box(start)) };
                if matches!(self.drag, Some(Drag::Point(_)) | Some(Drag::Ent(..)) | Some(Drag::Circle(_))) { self.push_undo(); }
            }
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            if let (Some(d), Some(h)) = (self.drag, hp) {
                let p = self.from_screen(h);
                match d {
                    Drag::Point(id) => { solver::solve(&mut self.sketch, Some((id, p))); solver::update_weak_values(&mut self.sketch); }
                    Drag::Ent(id, last) => {
                        let delta = p - last;
                        if let Some(e) = self.sketch.entity(id).cloned() {
                            for q in &e.pts { let np = self.sketch.point(*q) + delta; self.sketch.set_point(*q, np); }
                            solver::solve(&mut self.sketch, None);
                            solver::update_weak_values(&mut self.sketch);
                        }
                        self.drag = Some(Drag::Ent(id, p));
                    }
                    Drag::Circle(id) => {
                        if let Some(c) = self.sketch.entity(id).map(|e| self.sketch.point(e.pts[0])) {
                            if let Some(e) = self.sketch.entities.iter_mut().find(|e| e.id == id) { e.r = (p - c).length().max(1e-3); }
                            solver::solve(&mut self.sketch, None);
                            solver::update_weak_values(&mut self.sketch);
                        }
                    }
                    Drag::Label(cid) => {
                        if let Some(c) = self.sketch.constraints.iter().find(|c| c.id == cid).cloned() {
                            let a = dim_anchor(&self.sketch, &c);
                            if let Some(k) = self.sketch.constraints.iter_mut().find(|c| c.id == cid) { k.label = p - a; }
                        }
                    }
                    Drag::Box(_) => {}
                }
            }
        }
        if resp.drag_stopped() {
            if let (Some(Drag::Box(start)), Some(h)) = (self.drag, hp) {
                let r = Rect::from_two_pos(start, h);
                if !mods.shift { self.selection.clear(); }
                if matches!(self.filter, SkFilter::All | SkFilter::Geometry) {
                    for e in &self.sketch.entities {
                        if self.sketch.sample(e, 16).iter().all(|p| r.contains(self.to_screen(*p))) { self.selection.push(SkSel::Ent(e.id)); }
                    }
                }
                if matches!(self.filter, SkFilter::All | SkFilter::Dimension) && self.show_dims {
                    for c in &self.sketch.constraints { if c.ty.is_dimension() && r.contains(self.label_rect(c).center()) { self.selection.push(SkSel::Con(c.id)); } }
                }
            }
            if matches!(self.drag, Some(Drag::Point(_)) | Some(Drag::Ent(..)) | Some(Drag::Circle(_))) { self.after_change(); }
            self.drag = None;
        }
        if resp.clicked() && !typing && !mods.alt {
            if let Some(h) = hp {
                if self.tool == Tool::Select {
                    let hit = self.hit_select(h);
                    let group = match hit { Some(SkSel::Ent(e)) if self.chain_select => self.chain_of(e).into_iter().map(SkSel::Ent).collect(), Some(s) => vec![s], None => vec![] };
                    match hit {
                        Some(s) if mods.shift || mods.command => {
                            if self.selection.contains(&s) { self.selection.retain(|x| !group.contains(x)); }
                            else { for g in group { if !self.selection.contains(&g) { self.selection.push(g); } } }
                        }
                        Some(_) => self.selection = group,
                        None => self.selection.clear(),
                    }
                } else { self.click(h, false); }
            }
        }
        if resp.secondary_clicked() || resp.middle_clicked() { self.click(Pos2::ZERO, true); }
        if resp.double_clicked() {
            if let Some(h) = hp {
                if let Some(id) = self.hit_dimension(h) {
                    if let Some(c) = self.sketch.constraints.iter().find(|c| c.id == id && c.ty.is_dimension()) {
                        self.edit = Some((id, c.expr.clone().unwrap_or_else(|| fmt_num(c.value, 4)), true));
                    }
                }
            }
        }
        self.hover = hp.and_then(|h| if self.tool == Tool::Select { self.hit_select(h) } else { self.hit_dimension(h).map(SkSel::Con).or_else(|| self.hit_point(h, &[]).map(SkSel::Point)).or_else(|| self.hit_entity(h, false).map(SkSel::Ent)) });
    }

    /// What a Select click picks, honouring the sketcher selection filter.
    fn hit_select(&self, h: Pos2) -> Option<SkSel> {
        let is_dim = |id: i64| self.sketch.constraints.iter().any(|c| c.id == id && c.ty.is_dimension());
        let ann = self.hit_dimension(h).filter(|id| match self.filter { SkFilter::All => true, SkFilter::Dimension => is_dim(*id), SkFilter::Constraint => !is_dim(*id), SkFilter::Geometry => false });
        if let Some(a) = ann { return Some(SkSel::Con(a)); }
        if !matches!(self.filter, SkFilter::All | SkFilter::Geometry) { return None; }
        self.hit_point(h, &[]).map(SkSel::Point).or_else(|| self.hit_entity(h, false).map(SkSel::Ent))
    }

    /// Simulate a click at sketch coordinates (used by tests and scripted demos).
    pub fn click_at(&mut self, p: V2) {
        self.update_ppu();
        let s = self.to_screen(p);
        if self.tool == Tool::Select { return; }
        self.click(s, false);
    }

    /// Simulate ending the current chain (right-click / Esc).
    pub fn end_chain(&mut self) { self.click(Pos2::ZERO, true); }

    /// Set a dimension value or expression directly (tests, Modify dialog).
    pub fn set_dimension(&mut self, id: i64, text: &str) { self.apply_edit(id, text); self.edit = None; }

    // ---------- drawing ----------

    pub fn draw(&self, painter: &egui::Painter) {
        let sk = &self.sketch;
        let ts = |p: V2| self.to_screen(p);
        if self.grid {
            let g = self.grid_step();
            let corners = [self.rect.left_top(), self.rect.right_top(), self.rect.right_bottom(), self.rect.left_bottom()].map(|c| self.from_screen(c));
            let (mut mn, mut mx) = (corners[0], corners[0]);
            for c in corners { mn = mn.min(c); mx = mx.max(c); }
            let gc = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(120, 130, 150, 50));
            let (x0, x1, y0, y1) = ((mn.x / g).floor() as i64, (mx.x / g).ceil() as i64, (mn.y / g).floor() as i64, (mx.y / g).ceil() as i64);
            if (x1 - x0) < 300 && (y1 - y0) < 300 {
                for i in x0..=x1 { let x = i as f64 * g; painter.line_segment([ts(V2::new(x, mn.y)), ts(V2::new(x, mx.y))], gc); }
                for j in y0..=y1 { let y = j as f64 * g; painter.line_segment([ts(V2::new(mn.x, y)), ts(V2::new(mx.x, y))], gc); }
            }
        }
        let refc = Stroke::new(1.0_f32, REFC);
        for (k, (a, b)) in [self.long_line(V2::ZERO, V2::X), self.long_line(V2::ZERO, V2::Y)].into_iter().enumerate() {
            if !sk.axes_off[k] { painter.extend(Shape::dashed_line(&[ts(a), ts(b)], refc, 8.0, 5.0)); }
        }
        for l in &self.refs {
            painter.extend(Shape::dashed_line(&[ts(l[0]), ts(l[1])], Stroke::new(1.0_f32, REFC), 8.0, 5.0));
        }
        for g in &self.ghosts {
            let pts: Vec<Pos2> = g.iter().map(|p| ts(*p)).collect();
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.2_f32, Color32::from_rgb(160, 90, 200)), 3.0, 3.0));
        }
        if self.shade {
            if let Ok(regions) = sk.profile_regions() {
                for r in regions {
                    let holes: Vec<Vec<V2>> = r.holes.iter().map(|h| h.pts.clone()).collect();
                    let flat: Vec<V2> = r.outer.pts.iter().copied().chain(holes.iter().flatten().copied()).collect();
                    let mut mesh = egui::Mesh::default();
                    for p in &flat { mesh.colored_vertex(ts(*p), Color32::from_rgba_unmultiplied(255, 196, 120, 80)); }
                    for [a, b, c] in triangulate(&r.outer.pts, &holes) { mesh.add_triangle(a as u32, b as u32, c as u32); }
                    painter.add(Shape::mesh(mesh));
                }
            }
        }
        let sel_ent = |id: i64| self.selection.contains(&SkSel::Ent(id)) || self.picks.contains(&SkSel::Ent(id));
        if self.tool == Tool::AddRef {
            let grey = Stroke::new(1.0_f32, Color32::from_rgb(150, 156, 166));
            for (_, l) in &self.cand_refs { painter.extend(Shape::dashed_line(&[ts(l[0]), ts(l[1])], grey, 3.0, 3.0)); }
            for (_, p) in &self.cand_points { let c = ts(*p); painter.line_segment([c - egui::vec2(3.0, 3.0), c + egui::vec2(3.0, 3.0)], grey); painter.line_segment([c - egui::vec2(3.0, -3.0), c + egui::vec2(3.0, -3.0)], grey); }
        }
        for e in &sk.entities {
            if e.kind == SkKind::Point && e.ref_name.is_empty() { continue; }
            let col = if sel_ent(e.id) { SEL } else if self.hover == Some(SkSel::Ent(e.id)) { PRE } else if e.centerline { CLINE } else if e.construction { CONSTRUCT } else { GEOM };
            if e.kind == SkKind::Text {
                // the construction line from start to height point, then the glyph outlines
                painter.extend(Shape::dashed_line(&[ts(sk.point(e.pts[0])), ts(sk.point(e.pts[1]))], Stroke::new(1.0_f32, CONSTRUCT), 6.0, 4.0));
                for l in sk.text_loops(e) { painter.add(Shape::closed_line(l.iter().map(|p| ts(*p)).collect(), Stroke::new(1.5_f32, col))); }
                continue;
            }
            if !e.ref_name.is_empty() {
                // reference point: magenta cross like the reference lines
                let c = ts(sk.point(e.pts[0]));
                let rc = if sel_ent(e.id) { SEL } else { REFC };
                painter.line_segment([c - egui::vec2(5.0, 5.0), c + egui::vec2(5.0, 5.0)], Stroke::new(1.4_f32, rc));
                painter.line_segment([c - egui::vec2(5.0, -5.0), c + egui::vec2(5.0, -5.0)], Stroke::new(1.4_f32, rc));
                continue;
            }
            if e.kind == SkKind::Csys {
                let o = ts(sk.point(e.pts[0]));
                for (d, n) in [(egui::vec2(22.0, 0.0), "X"), (egui::vec2(0.0, -22.0), "Y")] {
                    painter.line_segment([o, o + d], Stroke::new(1.4_f32, col));
                    painter.text(o + d * 1.25, egui::Align2::CENTER_CENTER, n, egui::FontId::proportional(10.0), col);
                }
                painter.circle_stroke(o, 3.0, Stroke::new(1.2_f32, col));
                continue;
            }
            if e.centerline {
                let (a, b) = self.long_line(sk.point(e.pts[0]), sk.point(e.pts[1]));
                painter.extend(Shape::dashed_line(&[ts(a), ts(b)], Stroke::new(1.2_f32, col), 16.0, 4.0));
                continue;
            }
            let pts: Vec<Pos2> = sk.sample(e, 96).iter().map(|p| ts(*p)).collect();
            if e.construction { painter.extend(Shape::dashed_line(&pts, Stroke::new(1.2_f32, col), 6.0, 4.0)); }
            else { painter.add(Shape::line(pts, Stroke::new(2.0_f32, col))); }
        }
        if self.show_vertices {
            for p in &sk.points {
                if p.id == 0 || !sk.entities.iter().any(|e| e.pts.contains(&p.id)) { continue; }
                let s = ts(p.p);
                let col = if self.selection.contains(&SkSel::Point(p.id)) || self.picks.contains(&SkSel::Point(p.id)) { SEL } else if self.hover == Some(SkSel::Point(p.id)) { PRE } else { Color32::from_rgb(20, 40, 110) };
                painter.rect_filled(Rect::from_center_size(s, egui::vec2(4.5, 4.5)), 0.0, col);
            }
        }
        if self.open_ends && !matches!(self.requirement, Requirement::Trajectory | Requirement::Helix) {
            for p in sk.open_ends() {
                painter.rect_stroke(Rect::from_center_size(ts(p), egui::vec2(9.0, 9.0)), 0.0, Stroke::new(2.0_f32, Color32::RED), egui::StrokeKind::Middle);
            }
        }
        if self.hl_overlap || self.hl_intersect || self.hl_branch {
            let (ov, cross, branch) = self.check_marks();
            let hl = Color32::from_rgb(255, 120, 0);
            if self.hl_overlap {
                for e in sk.entities.iter().filter(|e| ov.contains(&e.id)) {
                    painter.add(Shape::line(sk.sample(e, 96).iter().map(|p| ts(*p)).collect(), Stroke::new(4.0_f32, hl)));
                }
            }
            if self.hl_intersect { for x in cross { painter.circle_stroke(ts(x), 6.0, Stroke::new(2.0_f32, Color32::RED)); } }
            if self.hl_branch { for x in branch { painter.rect_stroke(Rect::from_center_size(ts(x), egui::vec2(10.0, 10.0)), 0.0, Stroke::new(2.0_f32, Color32::from_rgb(200, 0, 200)), egui::StrokeKind::Middle); } }
        }
        if self.show_cons {
            for (id, p, g) in self.glyph_positions() {
                let col = if self.selection.contains(&SkSel::Con(id)) { SEL } else if self.hover == Some(SkSel::Con(id)) { PRE } else { CONC };
                painter.text(p, egui::Align2::CENTER_CENTER, g, egui::FontId::proportional(11.0), col);
            }
        }
        if self.show_dims {
            for c in &sk.constraints {
                if !c.ty.is_dimension() { continue; }
                let col = if self.selection.contains(&SkSel::Con(c.id)) { SEL } else if self.hover == Some(SkSel::Con(c.id)) { PRE } else if c.weak { WEAKC } else { STRONG };
                let r = draw_dim(painter, sk, c, &ts, col, &dim_text(c));
                if self.show_locks && c.locked {
                    // small padlock right of the label
                    let o = r.right_center() + egui::vec2(8.0, 1.0);
                    painter.rect_filled(Rect::from_center_size(o + egui::vec2(0.0, 2.0), egui::vec2(8.0, 6.0)), 1.0, col);
                    painter.circle_stroke(o + egui::vec2(0.0, -2.0), 2.6, Stroke::new(1.3_f32, col));
                }
            }
        }
        if let (Some(Drag::Box(start)), Some(h)) = (self.drag, self.cursor) {
            let r = Rect::from_two_pos(start, h);
            painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(60, 140, 255, 30));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(60, 140, 255)), egui::StrokeKind::Inside);
        }
        let Some(h) = self.cursor else { return };
        if self.drag.is_some() { return; }
        let exclude: Vec<i64> = self.chain.map(|c| vec![c.1]).unwrap_or_default();
        let sn = self.snap(h, &exclude);
        let cur = ts(sn.pos);
        let rb = Stroke::new(1.5_f32, RUBBER);
        if self.tool != Tool::Select {
            if sn.point.is_some() { painter.circle_stroke(cur, 6.0, rb); }
            else if sn.on.is_some() { painter.circle_stroke(cur, 4.0, Stroke::new(1.0_f32, RUBBER)); }
        }
        let circle = |c: V2, r: f64| -> Vec<Pos2> { (0..=64).map(|k| { let t = std::f64::consts::TAU * k as f64 / 64.0; ts(c + V2::new(t.cos(), t.sin()) * r) }).collect() };
        match self.tool {
            Tool::Line => if let Some((_, last)) = self.chain {
                let lp = sk.point(last);
                let mut q = sn.pos;
                let d = q - lp;
                let mut tag = "";
                if sn.point.is_none() {
                    if d.y.abs() < d.x.abs() * 0.07 { q.y = lp.y; tag = "H"; } else if d.x.abs() < d.y.abs() * 0.07 { q.x = lp.x; tag = "V"; }
                }
                painter.line_segment([ts(lp), ts(q)], rb);
                if !tag.is_empty() { painter.text(ts((lp + q) * 0.5) + egui::vec2(0.0, -12.0), egui::Align2::CENTER_CENTER, tag, egui::FontId::proportional(12.0), RUBBER); }
            },
            Tool::Centerline if self.clicks.len() == 1 => { painter.extend(Shape::dashed_line(&[ts(self.clicks[0].pos), cur], rb, 10.0, 4.0)); }
            Tool::Rect | Tool::CenterRect if self.clicks.len() == 1 => {
                let c0 = self.clicks[0].pos;
                let (lo, hi) = if self.tool == Tool::Rect { (c0, sn.pos) } else { (c0 * 2.0 - sn.pos, sn.pos) };
                let pts = [lo, V2::new(hi.x, lo.y), hi, V2::new(lo.x, hi.y), lo].map(|p| ts(p));
                painter.add(Shape::line(pts.to_vec(), rb));
            }
            Tool::Circle if self.clicks.len() == 1 => { painter.add(Shape::line(circle(self.clicks[0].pos, (sn.pos - self.clicks[0].pos).length()), rb)); }
            Tool::Circle3 if self.clicks.len() == 2 => {
                if let Some(c) = circumcenter(self.clicks[0].pos, self.clicks[1].pos, sn.pos) { painter.add(Shape::line(circle(c, (self.clicks[0].pos - c).length()), rb)); }
            }
            Tool::Palette => if let Some((it, k)) = self.palette_choice() {
                let shape = it.scaled(k);
                for e in &shape.entities {
                    let pts: Vec<Pos2> = shape.sample(e, 48).iter().map(|q| ts(sn.pos + *q)).collect();
                    if e.centerline || e.construction { painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0_f32, RUBBER), 6.0, 4.0)); } else { painter.add(Shape::line(pts, rb)); }
                }
            },
            Tool::Polygon if self.clicks.len() == 1 => {
                let c0 = self.clicks[0].pos;
                let r = (sn.pos - c0).length();
                let a0 = (sn.pos.y - c0.y).atan2(sn.pos.x - c0.x);
                let n = self.sides.max(3);
                let pts: Vec<Pos2> = (0..=n).map(|k| { let t = a0 + std::f64::consts::TAU * k as f64 / n as f64; ts(c0 + V2::new(t.cos(), t.sin()) * r) }).collect();
                painter.add(Shape::line(pts, rb));
            }
            Tool::Spline if !self.clicks.is_empty() => {
                let mut pts: Vec<V2> = self.clicks.iter().map(|c| c.pos).collect();
                pts.push(sn.pos);
                let curve: Vec<Pos2> = catmull_rom(&pts, 12, false).iter().map(|p| ts(*p)).collect();
                painter.add(Shape::line(curve, rb));
                for c in &self.clicks { painter.rect_filled(Rect::from_center_size(ts(c.pos), egui::vec2(5.0, 5.0)), 0.0, RUBBER); }
            }
            Tool::Ellipse if !self.clicks.is_empty() => {
                let c0 = self.clicks[0].pos;
                let (a, ry) = if self.clicks.len() == 1 { (sn.pos, (sn.pos - c0).length() * 0.5) } else {
                    let a = self.clicks[1].pos;
                    (a, cross2(safe_norm2(a - c0, V2::X), sn.pos - c0).abs())
                };
                let (rx, u) = ((a - c0).length(), safe_norm2(a - c0, V2::X));
                let v = perp_left(u);
                let pts: Vec<Pos2> = (0..=64).map(|k| { let t = std::f64::consts::TAU * k as f64 / 64.0; ts(c0 + u * (rx * t.cos()) + v * (ry * t.sin())) }).collect();
                painter.add(Shape::line(pts, rb));
            }
            Tool::SlantRect | Tool::Parallelogram if self.clicks.len() == 2 => {
                let (p0, p1) = (self.clicks[0].pos, self.clicks[1].pos);
                let (q2, q3) = if self.tool == Tool::SlantRect {
                    let u = safe_norm2(p1 - p0, V2::X);
                    let n = perp_left(u) * cross2(u, sn.pos - p0);
                    (p1 + n, p0 + n)
                } else { (sn.pos, p0 + (sn.pos - p1)) };
                painter.add(Shape::closed_line(vec![ts(p0), ts(p1), ts(q2), ts(q3)], rb));
            }
            Tool::SlantRect | Tool::Parallelogram | Tool::Conic | Tool::EllipseAxis if self.clicks.len() == 1 => { painter.line_segment([ts(self.clicks[0].pos), cur], rb); }
            Tool::EllipseAxis if self.clicks.len() == 2 => {
                let (p0, p1) = (self.clicks[0].pos, self.clicks[1].pos);
                let c0 = (p0 + p1) * 0.5;
                let u = safe_norm2(p1 - c0, V2::X);
                let (rx, ry, v) = ((p1 - c0).length(), cross2(u, sn.pos - c0).abs(), perp_left(u));
                let pts: Vec<Pos2> = (0..=64).map(|k| { let t = std::f64::consts::TAU * k as f64 / 64.0; ts(c0 + u * (rx * t.cos()) + v * (ry * t.sin())) }).collect();
                painter.add(Shape::line(pts, rb));
            }
            Tool::Conic if self.clicks.len() == 2 => {
                let (p0, p1) = (self.clicks[0].pos, self.clicks[1].pos);
                let ctrl = sn.pos * 2.0 - (p0 + p1) * 0.5;
                painter.add(Shape::line(conic_points(p0, ctrl, p1, 0.5, 32).iter().map(|p| ts(*p)).collect(), rb));
            }
            Tool::Text if self.clicks.len() == 1 => { painter.extend(Shape::dashed_line(&[ts(self.clicks[0].pos), cur], rb, 6.0, 4.0)); }
            Tool::CircleConc if !self.picks.is_empty() => {
                if let Some(SkSel::Ent(b)) = self.picks.first() {
                    if let Some(e) = sk.entity(*b) { let c = sk.point(e.pts[0]); painter.add(Shape::line(circle(c, (sn.pos - c).length()), rb)); }
                }
            }
            Tool::Arc3 | Tool::ArcCenter | Tool::Circle3 if !self.clicks.is_empty() => {
                let pts: Vec<Pos2> = self.clicks.iter().map(|c| ts(c.pos)).chain(std::iter::once(cur)).collect();
                painter.extend(Shape::dashed_line(&pts, rb, 4.0, 3.0));
            }
            _ => {}
        }
    }

    /// Inline dimension editor and the Modify / Feature Requirements dialogs.
    pub fn overlays(&mut self, ctx: &egui::Context) {
        self.pending_ui(ctx);
        self.rot_resize_ui(ctx);
        self.palette_ui(ctx);
        self.refs_ui(ctx);
        if let Some((id, mut text, focus)) = self.edit.clone() {
            if let Some(c) = self.sketch.constraints.iter().find(|c| c.id == id).cloned() {
                let pos = self.label_rect(&c).center();
                let (mut commit, mut cancel) = (false, false);
                egui::Area::new(egui::Id::new("dim_edit")).fixed_pos(pos - egui::vec2(52.0, 12.0)).order(egui::Order::Foreground).show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).inner_margin(3.0).show(ui, |ui| {
                        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(104.0).hint_text("Wert oder Beziehung"));
                        if focus { r.request_focus(); }
                        if r.lost_focus() { if ui.input(|i| i.key_pressed(egui::Key::Enter)) { commit = true; } else { cancel = true; } }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) { cancel = true; }
                    });
                });
                if commit { self.apply_edit(id, &text); self.edit = None; }
                else if cancel { self.edit = None; }
                else { self.edit = Some((id, text, false)); }
            } else { self.edit = None; }
        }
        if self.modify_open {
            let mut open = true;
            let dims: Vec<SkConstraint> = self.sketch.constraints.iter().filter(|c| c.ty.is_dimension()).cloned().collect();
            let mut apply: Option<(i64, String)> = None;
            let texts = &mut self.modify_text;
            egui::Window::new("Bemaßungen ändern").open(&mut open).collapsible(false).default_width(260.0).show(ctx, |ui| {
                ui.label("Werte editieren und Enter drücken. Geänderte schwache Bemaßungen werden stark.");
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    egui::Grid::new("moddims").striped(true).show(ui, |ui| {
                        for c in &dims {
                            ui.label(egui::RichText::new(format!("sd{}", c.id)).color(if c.weak { WEAKC } else { STRONG }));
                            let t = texts.entry(c.id).or_insert_with(|| c.expr.clone().unwrap_or_else(|| fmt_num(c.value, 4)));
                            let r = ui.add(egui::TextEdit::singleline(t).desired_width(110.0));
                            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { apply = Some((c.id, t.clone())); }
                            ui.label(if c.weak { "weak" } else { "strong" });
                            ui.end_row();
                        }
                    });
                });
            });
            if let Some((id, t)) = apply { self.apply_edit(id, &t); self.modify_text.clear(); }
            if !open { self.modify_open = false; self.modify_text.clear(); }
        }
        if self.requirements_open {
            let mut open = true;
            let rows = self.requirement_rows();
            egui::Window::new("KE-Anforderungen").open(&mut open).collapsible(false).default_width(330.0).show(ctx, |ui| {
                egui::Grid::new("req").striped(true).show(ui, |ui| {
                    for (ok, t) in &rows {
                        ui.label(egui::RichText::new(if *ok { "✔" } else { "✖" }).color(if *ok { Color32::from_rgb(30, 150, 50) } else { Color32::from_rgb(200, 40, 30) }));
                        ui.label(t);
                        ui.end_row();
                    }
                });
            });
            if !open { self.requirements_open = false; }
        }
    }

    /// Creo's input line in the message area for Versatz / Aufdicken, and the Text dialog.
    fn pending_ui(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.pending.clone() else { return };
        let (mut ok, mut cancel) = (false, false);
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let title = match p { Pending::Offset { .. } => "Versatz", Pending::Thicken { .. } => "Aufdicken", Pending::Text { .. } => "Text" };
        egui::Window::new(title).id(egui::Id::new("sk_pending")).collapsible(false).resizable(false).default_pos(self.rect.center() - egui::vec2(160.0, 80.0)).show(ctx, |ui| {
            ui.set_width(320.0);
            match &mut p {
                Pending::Offset { value, .. } => {
                    ui.label("Versatz in Pfeilrichtung eingeben");
                    let r = ui.add(egui::TextEdit::singleline(value).desired_width(300.0));
                    r.request_focus();
                    if enter { ok = true; }
                }
                Pending::Thicken { thick, off, cap, .. } => {
                    egui::Grid::new("thk").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Endtyp");
                        egui::ComboBox::from_id_salt("thk_cap").width(160.0).selected_text(["Offen", "Flach", "Kreisförmig"][*cap as usize]).show_ui(ui, |ui| {
                            for (i, n) in ["Offen", "Flach", "Kreisförmig"].iter().enumerate() { ui.selectable_value(cap, i as u8, *n); }
                        });
                        ui.end_row();
                        ui.label("Dicke");
                        ui.add(egui::TextEdit::singleline(thick).desired_width(160.0));
                        ui.end_row();
                        ui.label("Versatz in Pfeilrichtung");
                        ui.add(egui::TextEdit::singleline(off).desired_width(160.0));
                        ui.end_row();
                    });
                    if enter { ok = true; }
                }
                Pending::Text { t, .. } => {
                    ui.label(egui::RichText::new("Text").strong());
                    let r = ui.add(egui::TextEdit::singleline(&mut t.text).desired_width(300.0));
                    if t.text.is_empty() { r.request_focus(); }
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Schriftart").strong());
                    egui::Grid::new("txt").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Schriftart");
                        let shown = if t.family.is_empty() { crate::sketch::text::FONTS[t.font as usize].to_string() } else { t.family.clone() };
                        egui::ComboBox::from_id_salt("txt_font").width(200.0).height(320.0).selected_text(shown).show_ui(ui, |ui| {
                            for (i, n) in crate::sketch::text::FONTS.iter().enumerate() {
                                if ui.selectable_label(t.family.is_empty() && t.font == i as u8, *n).clicked() { t.font = i as u8; t.family.clear(); }
                            }
                            ui.separator();
                            for (n, _) in crate::sketch::text::system_fonts() {
                                if ui.selectable_label(&t.family == n, n).clicked() { t.family = n.clone(); }
                            }
                        });
                        ui.end_row();
                        ui.label("Seitenverhältnis");
                        ui.add(egui::DragValue::new(&mut t.aspect).speed(0.01).range(0.1..=10.0));
                        ui.end_row();
                        ui.label("Neigungswinkel");
                        ui.add(egui::DragValue::new(&mut t.slant).speed(0.5).range(-60.0..=60.0).suffix("°"));
                        ui.end_row();
                    });
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("Abbrechen").min_size(egui::vec2(90.0, 26.0))).clicked() { cancel = true; }
                    if ui.add(egui::Button::new("OK").min_size(egui::vec2(70.0, 26.0))).clicked() { ok = true; }
                });
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) { cancel = true; }
        if cancel { self.pending = None; return; }
        if !ok { self.pending = Some(p); return; }
        let vars = self.vars.clone();
        let num = |t: &str| expr::eval(t, &vars).ok();
        match p {
            Pending::Offset { src, side, value } => match num(&value) {
                Some(d) => { self.pending = None; self.offset_from(src, side, &[d], 0); }
                None => { self.say("Einen Zahlenwert eingeben.", MsgKind::Warning); self.pending = Some(Pending::Offset { src, side, value }); }
            },
            Pending::Thicken { src, side, thick, off, cap } => match (num(&thick), num(&off)) {
                (Some(t), Some(o)) if t > 0.0 => { self.pending = None; self.offset_from(src, side, &[o, o + t], cap); }
                _ => { self.say("Dicke (größer als 0) und Versatz eingeben.", MsgKind::Warning); self.pending = Some(Pending::Thicken { src, side, thick, off, cap }); }
            },
            Pending::Text { a, b, t } => {
                self.pending = None;
                if t.text.trim().is_empty() { return; }
                self.push_undo();
                let (ia, ib) = (self.sketch.add_point(a), self.sketch.add_point(b));
                let id = self.sketch.add_entity(SkKind::Text, vec![ia, ib], 0.0, false, false);
                if let Some(e) = self.sketch.entities.iter_mut().find(|e| e.id == id) { e.text = Some(t); }
                self.solve();
            }
        }
    }

    /// Versatz (one distance) or Aufdicken (two distances plus end caps) of a line, circle, arc or
    /// reference edge, on the side of the pick; a line offset gets a parallel constraint and its
    /// offset dimension, like Creo.
    pub fn offset_from(&mut self, src: i64, side: V2, dists: &[f64], cap: u8) {
        let kind = if src < 0 { Some(SkKind::Line) } else { self.sketch.entity(src).map(|e| e.kind) };
        self.push_undo();
        let mut ends: Vec<(i64, i64)> = vec![];
        let mut dir = (V2::ZERO, V2::ZERO);
        for &d in dists {
            match kind {
                Some(SkKind::Line) => {
                    let Some((a, b)) = self.sketch.line_ends(src) else { return };
                    let u = safe_norm2(b - a, V2::X);
                    let n = perp_left(u) * if cross2(u, side - a) >= 0.0 { 1.0 } else { -1.0 };
                    let (ia, ib) = (self.sketch.add_point(a + n * d), self.sketch.add_point(b + n * d));
                    let id = self.add_line(ia, ib);
                    self.con(SkCType::Parallel, vec![], vec![id, src]);
                    if d.abs() > 1e-9 {
                        let mut k = SkConstraint::new(SkCType::PointLine, vec![ia], vec![src]);
                        k.value = d.abs();
                        k.label = n * (d * 0.5) - u * (b - a).length() * 0.1;
                        self.sketch.add_constraint(k);
                    }
                    ends.push((ia, ib));
                    dir = (-u, u);
                }
                Some(SkKind::Circle) | Some(SkKind::Arc) => {
                    let e = self.sketch.entity(src).cloned().unwrap();
                    let c = self.sketch.point(e.pts[0]);
                    let r0 = self.sketch.radius(&e);
                    let r = r0 + d * if (side - c).length() >= r0 { 1.0 } else { -1.0 };
                    if r <= 1e-9 { self.say("Der Versatz ist größer als der Radius.", MsgKind::Warning); continue; }
                    let cons = self.construction;
                    if e.kind == SkKind::Circle { self.sketch.add_entity(SkKind::Circle, vec![e.pts[0]], r, cons, false); }
                    else {
                        let (s0, s1) = (self.sketch.point(e.pts[1]), self.sketch.point(e.pts[2]));
                        let ia = self.sketch.add_point(c + safe_norm2(s0 - c, V2::X) * r);
                        let ib = self.sketch.add_point(c + safe_norm2(s1 - c, V2::X) * r);
                        self.sketch.add_entity(SkKind::Arc, vec![e.pts[0], ia, ib], 0.0, cons, false);
                        let (a0, a1) = self.sketch.arc_angles(&e);
                        dir = (V2::new(a0.sin(), -a0.cos()), V2::new(-a1.sin(), a1.cos()));
                        ends.push((ia, ib));
                    }
                }
                _ => { self.say("Nur Linien, Kreise, Bögen und Kanten können versetzt werden.", MsgKind::Warning); return; }
            }
        }
        if ends.len() == 2 && cap > 0 {
            for (k, out) in [(0usize, dir.0), (1, dir.1)] {
                let (p, q) = if k == 0 { (ends[0].0, ends[1].0) } else { (ends[0].1, ends[1].1) };
                if cap == 1 { self.add_line(p, q); continue; }
                let (pp, qq) = (self.sketch.point(p), self.sketch.point(q));
                let m = (pp + qq) * 0.5;
                let ci = self.sketch.add_point(m);
                // round end: the half circle that bulges away from the body
                let (s0, e0) = if cross2(pp - m, out) >= 0.0 { (p, q) } else { (q, p) };
                self.sketch.add_arc(ci, s0, e0);
            }
        }
        self.tool = Tool::Select;
        self.solve();
    }

    /// Names for the reference dialog, set when the sketch starts.
    pub fn set_ref_info(&mut self, names: Vec<String>, axis_names: [String; 2], cand: Vec<(String, [V2; 2])>, cand_points: Vec<(String, V2)>) {
        self.ref_names = names;
        self.axis_names = axis_names;
        self.cand_refs = cand;
        self.cand_points = cand_points;
        // deleted default references can be picked again
        for k in 0..2 {
            if self.sketch.axes_off[k] && !self.axis_names[k].is_empty() {
                let d = if k == 0 { V2::X } else { V2::Y };
                let (a, b) = self.long_line(-d, d);
                self.cand_refs.push((self.axis_names[k].clone(), [a, b]));
            }
        }
    }

    /// Deletes a default reference (sketch axis): dimensions and constraints on it go.
    pub fn delete_axis_ref(&mut self, k: usize) {
        self.push_undo();
        let id = if k == 0 { REF_H } else { REF_V };
        self.sketch.constraints.retain(|c| !c.ents.contains(&id) || c.weak);
        self.sketch.axes_off[k] = true;
        let d = if k == 0 { V2::X } else { V2::Y };
        let (a, b) = self.long_line(-d, d);
        self.cand_refs.push((self.axis_names[k].clone(), [a, b]));
        self.after_change();
    }

    /// Whether the references locate the sketch (Creo's "Vollständig platziert").
    pub fn refs_placed(&self) -> bool {
        let mut dirs: Vec<V2> = vec![];
        if !self.sketch.axes_off[0] { dirs.push(V2::X); }
        if !self.sketch.axes_off[1] { dirs.push(V2::Y); }
        for l in &self.refs { dirs.push(safe_norm2(l[1] - l[0], V2::X)); }
        let pts = self.sketch.entities.iter().filter(|e| !e.ref_name.is_empty()).count();
        let two_dirs = dirs.iter().any(|a| dirs.iter().any(|b| cross2(*a, *b).abs() > 1e-6));
        two_dirs || (!dirs.is_empty() && pts >= 1) || pts >= 2
    }

    /// Removes reference `i`; constraints on it go, later references move up.
    pub fn delete_ref(&mut self, i: usize) {
        if i >= self.refs.len() { return; }
        self.push_undo();
        let id = -10 - i as i64;
        self.sketch.constraints.retain(|c| !c.ents.contains(&id));
        for c in &mut self.sketch.constraints { for e in &mut c.ents { if *e < id { *e += 1; } } }
        let l = self.refs.remove(i);
        let n = self.ref_names.remove(i);
        if let Some(x) = self.sketch.extra_refs.iter().position(|x| x.1 == l) { self.sketch.extra_refs.remove(x); } else { self.sketch.hidden_refs.push(l); }
        self.cand_refs.push((n, l));
        self.sketch.ref_lines = self.refs.clone();
        self.after_change();
    }

    /// Creo's "Referenzen" dialog: the sketch references, Löschen, and picking new ones.
    fn refs_ui(&mut self, ctx: &egui::Context) {
        if !self.refs_open { return; }
        let mut open = true;
        let mut del = None;
        let rows: Vec<(RefRow, String)> = (0..2).filter(|k| !self.sketch.axes_off[*k] && !self.axis_names[*k].is_empty()).map(|k| (RefRow::Axis(k), self.axis_names[k].clone()))
            .chain(self.ref_names.iter().enumerate().map(|(i, n)| (RefRow::Line(i), n.clone())))
            .chain(self.sketch.entities.iter().filter(|e| !e.ref_name.is_empty()).map(|e| (RefRow::Point(e.id), e.ref_name.clone())))
            .collect();
        egui::Window::new("Referenzen").id(egui::Id::new("sk_refs")).open(&mut open).collapsible(false).resizable(false).default_width(340.0).show(ctx, |ui| {
            egui::Frame::NONE.stroke(Stroke::new(1.0_f32, Color32::from_rgb(190, 194, 200))).inner_margin(4.0).show(ui, |ui| {
                ui.set_min_height(150.0);
                ui.set_width(320.0);
                for (r, n) in &rows { if ui.selectable_label(self.ref_sel == Some(*r), n).clicked() { self.ref_sel = Some(*r); } }
            });
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("Auswählen").selected(self.tool == Tool::AddRef)).on_hover_text("Kanten, Achsen, Bezugspunkte oder Eckpunkte als Referenzen hinzufügen").clicked() { self.set_tool(Tool::AddRef); }
                if ui.add_enabled(self.ref_sel.is_some(), egui::Button::new("Löschen")).clicked() { del = self.ref_sel; }
            });
            ui.label(format!("Referenzstatus: {}", if self.refs_placed() { "Vollständig platziert" } else { "Unvollständig platziert" }));
        });
        match del {
            Some(RefRow::Axis(k)) => self.delete_axis_ref(k),
            Some(RefRow::Line(i)) => self.delete_ref(i),
            Some(RefRow::Point(id)) => {
                self.push_undo();
                if let Some(e) = self.sketch.entity(id).cloned() { self.cand_points.push((e.ref_name.clone(), self.sketch.point(e.pts[0]))); }
                self.sketch.delete_entity(id);
                self.after_change();
            }
            None => {}
        }
        if del.is_some() { self.ref_sel = None; }
        if !open { self.refs_open = false; if self.tool == Tool::AddRef { self.set_tool(Tool::Select); } }
    }

    /// Opens "Drehen-Größe ändern" for the selected entities.
    pub fn open_rot_resize(&mut self) {
        let ents: Vec<i64> = self.selection.iter().filter_map(|s| if let SkSel::Ent(e) = s { Some(*e) } else { None }).collect();
        if ents.is_empty() { self.say("Zuerst die Elemente wählen.", MsgKind::Warning); return; }
        let mut pts: Vec<(i64, V2)> = vec![];
        let mut radii = vec![];
        for e in self.sketch.entities.iter().filter(|e| ents.contains(&e.id)) {
            for &p in &e.pts { if p != 0 && !pts.iter().any(|q| q.0 == p) { pts.push((p, self.sketch.point(p))); } }
            if matches!(e.kind, SkKind::Circle | SkKind::Ellipse) { radii.push((e.id, e.r)); }
        }
        let (mut mn, mut mx) = (V2::splat(f64::INFINITY), V2::splat(f64::NEG_INFINITY));
        for e in self.sketch.entities.iter().filter(|e| ents.contains(&e.id)) { for q in self.sketch.sample(e, 32) { mn = mn.min(q); mx = mx.max(q); } }
        self.push_undo();
        self.rot_resize = Some(RotResize { pts, radii, pivot: (mn + mx) * 0.5, dx: 0.0, dy: 0.0, angle: 0.0, scale: 1.0 });
    }

    fn apply_rot_resize(&mut self, r: &RotResize) {
        let (sn, cs) = r.angle.to_radians().sin_cos();
        for (id, p) in &r.pts {
            let d = (*p - r.pivot) * r.scale;
            self.sketch.set_point(*id, r.pivot + V2::new(d.x * cs - d.y * sn, d.x * sn + d.y * cs) + V2::new(r.dx, r.dy));
        }
        for (id, rad) in &r.radii { if let Some(e) = self.sketch.entities.iter_mut().find(|e| e.id == *id) { e.r = rad * r.scale; } }
    }

    fn rot_resize_ui(&mut self, ctx: &egui::Context) {
        let Some(mut r) = self.rot_resize.clone() else { return };
        let (mut ok, mut cancel) = (false, false);
        egui::Window::new("Drehen-Größe ändern").id(egui::Id::new("sk_rotres")).collapsible(false).resizable(false).default_pos(self.rect.left_top() + egui::vec2(20.0, 60.0)).show(ctx, |ui| {
            egui::Grid::new("rr").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Verschieben H"); ui.add(egui::DragValue::new(&mut r.dx).speed(0.5)); ui.end_row();
                ui.label("Verschieben V"); ui.add(egui::DragValue::new(&mut r.dy).speed(0.5)); ui.end_row();
                ui.label("Drehen"); ui.add(egui::DragValue::new(&mut r.angle).speed(0.5).suffix("°")); ui.end_row();
                ui.label("Skalieren"); ui.add(egui::DragValue::new(&mut r.scale).speed(0.01).range(0.001..=1000.0)); ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() { ok = true; }
                if ui.button("Abbrechen").clicked() { cancel = true; }
            });
        });
        self.apply_rot_resize(&r);
        if cancel {
            if let Some(prev) = self.undo.pop() { self.sketch = prev; }
            self.rot_resize = None;
            self.after_change();
        } else if ok {
            // constraints that no longer hold (e.g. H/V after a rotation) are dropped, as in Creo
            let ids: Vec<i64> = r.pts.iter().map(|p| p.0).collect();
            let ents: Vec<i64> = self.sketch.entities.iter().filter(|e| e.pts.iter().any(|p| ids.contains(p))).map(|e| e.id).collect();
            if r.angle.rem_euclid(90.0).abs() > 1e-9 {
                self.sketch.constraints.retain(|c| !(matches!(c.ty, SkCType::Horizontal | SkCType::Vertical) && c.ents.iter().any(|e| ents.contains(e))));
            }
            self.sketch.constraints.retain(|c| !(c.ty == SkCType::Fix && c.pts.iter().any(|p| ids.contains(p))));
            let vals: Vec<(usize, f64)> = self.sketch.constraints.iter().enumerate()
                .filter(|(_, c)| !c.weak && c.ty.is_dimension() && (c.pts.iter().any(|p| ids.contains(p)) || c.ents.iter().any(|e| ents.contains(e))))
                .map(|(i, c)| (i, self.sketch.measure(c))).collect();
            for (i, v) in vals { self.sketch.constraints[i].value = v; }
            self.rot_resize = None;
            self.solve();
        } else { self.rot_resize = Some(r); }
    }

    /// Ausschneiden / Kopieren: selected entities with their points and inner constraints.
    pub fn copy_selection(&mut self, cut: bool) {
        let ents: Vec<i64> = self.selection.iter().filter_map(|s| if let SkSel::Ent(e) = s { Some(*e) } else { None }).collect();
        if ents.is_empty() { return; }
        let mut clip = Sketch::default();
        clip.entities = self.sketch.entities.iter().filter(|e| ents.contains(&e.id)).cloned().collect();
        let pts: Vec<i64> = clip.entities.iter().flat_map(|e| e.pts.clone()).collect();
        clip.points = self.sketch.points.iter().filter(|p| pts.contains(&p.id)).cloned().collect();
        clip.constraints = self.sketch.constraints.iter().filter(|c| !c.weak && !c.ents.is_empty() && c.ents.iter().all(|e| ents.contains(e)) && c.pts.iter().all(|p| pts.contains(p))).cloned().collect();
        self.clipboard = Some(clip);
        if cut { self.delete_selection(); }
    }

    pub fn can_paste(&self) -> bool { self.clipboard.is_some() }

    /// Einfügen: a copy with new ids at `at` (or offset), selected and ready for Drehen-Größe ändern.
    pub fn paste(&mut self, at: Option<V2>) {
        let Some(clip) = self.clipboard.clone() else { return };
        let (mut mn, mut mx) = (V2::splat(f64::INFINITY), V2::splat(f64::NEG_INFINITY));
        for p in &clip.points { mn = mn.min(p.p); mx = mx.max(p.p); }
        let shift = match at { Some(a) => a - (mn + mx) * 0.5, None => (mx - mn) * 0.1 + V2::splat(5.0) };
        self.insert_sketch(&clip, shift);
    }

    /// Adds a copy of `clip` moved by `shift`; the copy is selected and opened in Drehen-Größe ändern.
    fn insert_sketch(&mut self, clip: &Sketch, shift: V2) {
        self.push_undo();
        let used: Vec<i64> = clip.entities.iter().flat_map(|e| e.pts.clone()).collect();
        let mut map = std::collections::HashMap::new();
        for p in clip.points.iter().filter(|p| used.contains(&p.id)) { map.insert(p.id, self.sketch.add_point(p.p + shift)); }
        let mut emap = std::collections::HashMap::new();
        let mut new_sel = vec![];
        for e in &clip.entities {
            let id = self.sketch.add_entity(e.kind, e.pts.iter().map(|p| *map.get(p).unwrap_or(p)).collect(), e.r, e.construction, e.centerline);
            if let Some(x) = self.sketch.entities.iter_mut().find(|x| x.id == id) { x.text = e.text.clone(); }
            emap.insert(e.id, id);
            new_sel.push(SkSel::Ent(id));
        }
        for c in &clip.constraints {
            let mut k = c.clone();
            k.pts = k.pts.iter().map(|p| *map.get(p).unwrap_or(p)).collect();
            k.ents = k.ents.iter().map(|e| *emap.get(e).unwrap_or(e)).collect();
            self.sketch.add_constraint(k);
        }
        self.undo.pop();
        self.selection = new_sel;
        self.solve();
        self.open_rot_resize();
    }

    /// The shape chosen in the palette and the factor it is placed with: steel sections keep
    /// their real size, the other shapes get a round size that suits the current view.
    fn palette_choice(&self) -> Option<(&'static crate::sketch::palette::PaletteItem, f64)> {
        let (t, i) = self.palette_item?;
        let it = crate::sketch::palette::tabs().get(t)?.items.get(i)?;
        if it.real { return Some((it, 1.0)); }
        let target = 140.0 / self.ppu;
        let p = 10f64.powf(target.log10().floor());
        let size = [1.0, 2.0, 5.0, 10.0].iter().map(|m| p * m).filter(|s| *s <= target * 1.0001).fold(p, f64::max);
        Some((it, size / it.extent()))
    }

    /// Arms the palette tool with a shape (tab and item index of `palette::tabs()`).
    pub fn pick_palette(&mut self, tab: usize, item: usize) {
        self.palette_tab = tab;
        self.palette_item = Some((tab, item));
        self.set_tool(Tool::Palette);
    }

    /// Places the chosen palette shape with its origin at `at`.
    pub fn place_palette(&mut self, at: V2) {
        let Some((it, k)) = self.palette_choice() else { return };
        let shape = it.scaled(k);
        self.insert_sketch(&shape, at);
        self.tool = Tool::Select;
    }

    /// Creo's "Skizzierer-Palette": preview, the tab rows and the shape list with thumbnails.
    fn palette_ui(&mut self, ctx: &egui::Context) {
        if !self.palette_open { return; }
        const BG: Color32 = Color32::from_rgb(64, 92, 122);
        const INK: Color32 = Color32::from_rgb(255, 255, 0);
        const SEL_ROW: Color32 = Color32::from_rgb(179, 224, 255);
        let border = Stroke::new(1.0_f32, Color32::from_rgb(190, 194, 200));
        let thumb = |p: &egui::Painter, r: Rect, it: &crate::sketch::palette::PaletteItem| {
            p.rect_filled(r, 0.0, BG);
            let (mn, mx) = it.bounds(false);
            let c = (mn + mx) * 0.5;
            let k = ((r.width() - 12.0) as f64 / (mx.x - mn.x).max(1e-9)).min((r.height() - 12.0) as f64 / (mx.y - mn.y).max(1e-9));
            let ts = |q: V2| r.center() + egui::vec2(((q.x - c.x) * k) as f32, (-(q.y - c.y) * k) as f32);
            let p = p.with_clip_rect(r);
            for e in &it.sketch.entities {
                let pts: Vec<Pos2> = it.sketch.sample(e, 48).iter().map(|q| ts(*q)).collect();
                if e.centerline || e.construction { p.extend(Shape::dashed_line(&pts, Stroke::new(1.0_f32, Color32::from_rgb(214, 222, 232)), 2.0, 2.0)); }
            }
            for e in it.sketch.entities.iter().filter(|e| !(e.centerline || e.construction)) {
                p.add(Shape::line(it.sketch.sample(e, 48).iter().map(|q| ts(*q)).collect(), Stroke::new(1.5_f32, INK)));
            }
        };
        let tabs = crate::sketch::palette::tabs();
        let (mut open, mut close) = (true, false);
        let mut pick = None;
        egui::Window::new("Skizzierer-Palette").id(egui::Id::new("sk_palette")).open(&mut open).collapsible(false).resizable(false)
            .default_pos(self.rect.right_top() + egui::vec2(-360.0, 20.0)).show(ctx, |ui| {
            let w = 320.0;
            ui.set_width(w);
            // preview of the chosen shape
            let (pr, _) = ui.allocate_exact_size(egui::vec2(w, 96.0), egui::Sense::hover());
            ui.painter().rect_filled(pr, 0.0, BG);
            if let Some(it) = self.palette_item.and_then(|(t, i)| tabs.get(t)?.items.get(i)) { thumb(ui.painter(), pr, it); }
            ui.add_space(6.0);
            // three rows of tabs; the row of the active tab sits directly on the list, as in Creo
            let font = egui::FontId::proportional(14.0);
            let mut rows: Vec<usize> = (0..tabs.len().div_ceil(3)).collect();
            let active_row = self.palette_tab / 3;
            rows.retain(|r| *r != active_row);
            rows.push(active_row);
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in rows {
                let n = tabs.len().min(row * 3 + 3) - row * 3;
                let (rr, _) = ui.allocate_exact_size(egui::vec2(w, 30.0), egui::Sense::hover());
                for k in 0..n {
                    let i = row * 3 + k;
                    let r = Rect::from_min_max(egui::pos2(rr.left() + w * k as f32 / n as f32, rr.top()), egui::pos2(rr.left() + w * (k + 1) as f32 / n as f32, rr.bottom()));
                    let resp = ui.interact(r, ui.id().with(("pal_tab", i)), egui::Sense::click());
                    let on = i == self.palette_tab;
                    ui.painter().rect_filled(r, 0.0, if on { Color32::WHITE } else if resp.hovered() { Color32::from_rgb(226, 238, 250) } else { Color32::from_rgb(240, 241, 243) });
                    ui.painter().rect_stroke(r, 0.0, border, egui::StrokeKind::Inside);
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, tabs[i].name, font.clone(), Color32::from_rgb(40, 44, 50));
                    if resp.clicked() { self.palette_tab = i; }
                }
            }
            let tab = self.palette_tab.min(tabs.len() - 1);
            egui::Frame::NONE.fill(Color32::WHITE).stroke(border).inner_margin(4.0).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt(("pal_list", tab)).max_height(330.0).min_scrolled_height(330.0).auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    for (i, it) in tabs[tab].items.iter().enumerate() {
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::click());
                        if !ui.is_rect_visible(r) { continue; }
                        let chosen = self.palette_item == Some((tab, i));
                        if chosen { ui.painter().rect_filled(r, 0.0, SEL_ROW); } else if resp.hovered() { ui.painter().rect_filled(r, 0.0, Color32::from_rgb(226, 238, 250)); }
                        thumb(ui.painter(), Rect::from_min_size(r.left_top() + egui::vec2(2.0, 2.0), egui::vec2(40.0, 40.0)), it);
                        ui.painter().text(r.left_center() + egui::vec2(54.0, 0.0), egui::Align2::LEFT_CENTER, &it.name, font.clone(), Color32::from_rgb(40, 44, 50));
                        if resp.clicked() || resp.double_clicked() { pick = Some((tab, i)); }
                        resp.on_hover_text("Klicken mit LMT, dann in der Skizze die Position der Form wählen.");
                    }
                });
            });
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { if ui.button("Schließen").clicked() { close = true; } });
        });
        if let Some((t, i)) = pick { self.pick_palette(t, i); }
        if !open || close {
            self.palette_open = false;
            if self.tool == Tool::Palette { self.set_tool(Tool::Select); }
        }
    }

    /// Importieren of a DXF file (LINE, CIRCLE, ARC, LWPOLYLINE with bulges, POLYLINE, SPLINE,
    /// ELLIPSE, POINT).
    pub fn import_dxf(&mut self, text: &str) -> Result<usize, String> { self.import_curves(crate::sketch::import2d::parse_dxf(text)?) }

    /// Importieren by file type: DXF, IGES (.igs/.iges) or Adobe Illustrator (.ai).
    pub fn import_file(&mut self, path: &std::path::Path) -> Result<usize, String> {
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let curves = match ext.as_str() {
            "dxf" => crate::sketch::import2d::parse_dxf(&String::from_utf8_lossy(&data))?,
            "igs" | "iges" => crate::sketch::import2d::parse_iges(&String::from_utf8_lossy(&data))?,
            "ai" => crate::sketch::import2d::parse_ai(&data)?,
            _ => return Err(format!("Dateityp .{} wird nicht unterstützt.", ext)),
        };
        self.import_curves(curves)
    }

    /// Adds imported curves (shared end points merged); they are selected and opened in
    /// Drehen-Größe ändern, as after Creo's import.
    pub fn import_curves(&mut self, ents: Vec<crate::sketch::import2d::Curve2>) -> Result<usize, String> {
        use crate::sketch::import2d::Curve2;
        if ents.is_empty() { return Err("Die Datei enthält keine unterstützte Geometrie.".into()); }
        self.push_undo();
        let before: Vec<i64> = self.sketch.entities.iter().map(|e| e.id).collect();
        let mut pts: Vec<(V2, i64)> = vec![];
        let tol = 1e-4;
        let mut pid = |sk: &mut Sketch, p: V2| -> i64 {
            if let Some(q) = pts.iter().find(|q| (q.0 - p).length() < tol) { return q.1; }
            let id = sk.add_point(p);
            pts.push((p, id));
            id
        };
        for e in ents {
            match e {
                Curve2::Line(a, b) => { let (ia, ib) = (pid(&mut self.sketch, a), pid(&mut self.sketch, b)); if ia != ib { self.sketch.add_line(ia, ib); } }
                Curve2::Circle(c, r) => { let ic = self.sketch.add_point(c); self.sketch.add_circle(ic, r); }
                Curve2::Arc(c, r, a0, a1) => {
                    let ic = self.sketch.add_point(c);
                    let (s, t) = (c + V2::new(a0.cos(), a0.sin()) * r, c + V2::new(a1.cos(), a1.sin()) * r);
                    let (is, it) = (pid(&mut self.sketch, s), pid(&mut self.sketch, t));
                    self.sketch.add_arc(ic, is, it);
                }
                Curve2::Spline(p) => { let ids: Vec<i64> = p.iter().map(|q| pid(&mut self.sketch, *q)).collect(); if ids.len() >= 2 { self.sketch.add_entity(SkKind::Spline, ids, 0.0, false, false); } }
                Curve2::Ellipse(c, a, rb) => { let (ic, ia) = (self.sketch.add_point(c), self.sketch.add_point(c + a)); self.sketch.add_entity(SkKind::Ellipse, vec![ic, ia], rb, false, false); }
                Curve2::Point(p) => { let i = pid(&mut self.sketch, p); self.sketch.add_entity(SkKind::Point, vec![i], 0.0, true, false); }
            }
        }
        self.undo.pop();
        self.selection = self.sketch.entities.iter().filter(|e| !before.contains(&e.id)).map(|e| SkSel::Ent(e.id)).collect();
        let n = self.selection.len();
        self.after_change();
        self.open_rot_resize();
        Ok(n)
    }

    pub fn requirement_rows(&self) -> Vec<(bool, String)> {
        let sk = &self.sketch;
        let mut rows = vec![];
        let open = sk.open_ends().len();
        match self.requirement {
            Requirement::Trajectory | Requirement::Helix => {
                let t = sk.trajectory();
                rows.push((t.is_ok(), t.err().unwrap_or_else(|| "Die Leitkurve ist eine zusammenhängende Kette.".into())));
            }
            _ => {
                let r = sk.profile_regions();
                rows.push((open == 0, if open == 0 { "Die Skizze ist geschlossen.".into() } else { format!("{} offene(s) Ende(n) — Querschnitte müssen geschlossen sein.", open) }));
                rows.push((r.is_ok(), match &r { Ok(v) => format!("{} geschlossene(r) Bereich(e).", v.len()), Err(e) => e.clone() }));
            }
        }
        if self.requirement == Requirement::Helix {
            let has = sk.centerline_axis().is_some();
            rows.push((has, if has { "Die Mittellinie definiert die Spiralachse.".into() } else { "Eine Mittellinie für die Spiralachse skizzieren.".into() }));
        }
        if self.requirement == Requirement::Revolve {
            let has = sk.centerline_axis().is_some();
            rows.push((has, if has { "Die Mittellinie definiert die Drehachse.".into() } else { "Eine Mittellinie für die Drehachse skizzieren.".into() }));
        }
        let strong = sk.constraints.iter().filter(|c| c.ty.is_dimension() && !c.weak).count();
        let weak = sk.constraints.iter().filter(|c| c.weak).count();
        rows.push((true, format!("{} starke und {} schwache Bemaßung(en).", strong, weak)));
        rows.push((self.dof == 0, if self.dof == 0 { "Durch starke Bemaßungen/Bedingungen vollständig definiert.".into() } else { format!("{} Freiheitsgrad(e) durch schwache Bemaßungen gehalten.", self.dof) }));
        rows
    }
}

fn circumcenter(a: V2, b: V2, c: V2) -> Option<V2> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 { return None; }
    let (a2, b2, c2) = (a.length_squared(), b.length_squared(), c.length_squared());
    Some(V2::new((a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d, (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d))
}

pub fn line_intersection(a1: V2, b1: V2, a2: V2, b2: V2) -> Option<V2> {
    let (d1, d2) = (b1 - a1, b2 - a2);
    let den = cross2(d1, d2);
    if den.abs() < 1e-12 { return None; }
    Some(a1 + d1 * (cross2(a2 - a1, d2) / den))
}

/// Intersection of segments a1-b1 and a2-b2 (end points included).
fn seg_intersection(a1: V2, b1: V2, a2: V2, b2: V2) -> Option<V2> {
    let (d1, d2) = (b1 - a1, b2 - a2);
    let den = cross2(d1, d2);
    if den.abs() < 1e-12 { return None; }
    let t = cross2(a2 - a1, d2) / den;
    let u = cross2(a2 - a1, d1) / den;
    ((-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u)).then(|| a1 + d1 * t)
}

/// Distance from `q` to segment a-b and the closest point.
fn seg_point_dist(q: V2, a: V2, b: V2) -> (f64, V2) {
    let ab = b - a;
    let t = if ab.length_squared() < 1e-18 { 0.0 } else { ((q - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) };
    let x = a + ab * t;
    ((q - x).length(), x)
}

fn polyline_len(p: &[V2]) -> f64 { p.windows(2).map(|w| (w[1] - w[0]).length()).sum() }

/// Common tangents of two circles as (point on circle 1, point on circle 2): up to four.
pub fn common_tangents(c1: V2, r1: f64, c2: V2, r2: f64) -> Vec<(V2, V2)> {
    let d = c2 - c1;
    let l = d.length();
    if l < 1e-12 { return vec![]; }
    let u = d / l;
    let mut out = vec![];
    for s in [1.0, -1.0] {
        // unit normal n of the tangent line with n·(c2 - c1) = s·r2 - r1
        let k = (s * r2 - r1) / l;
        if k.abs() > 1.0 { continue; }
        let h = (1.0 - k * k).sqrt();
        for sign in [1.0, -1.0] {
            let n = u * k + perp_left(u) * (h * sign);
            out.push((c1 - n * r1, c2 - n * (s * r2)));
        }
    }
    out
}

