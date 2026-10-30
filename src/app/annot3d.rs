//! Anmerkungen erstellen: 3D annotations on the model — dimensions, ordinate and reference
//! dimensions, tolerances, notes, surface finish, datum symbols and targets, geometric tolerances —
//! with Anmerkungs-KEs grouping them, moving / reattaching / removing, Bemaßungen bereinigen and
//! the Abfrage list.

use super::creo::{dialog_header, fieldset};
use super::drawing::{gsymbol, Prim};
use super::icons::{self, Icon};
use super::ribbon::{group, group_drop, group_menu, large, menu_item, stack, theme::*, tiny};
use super::*;
use crate::model::drawing::{FinishKind, GChar, MatCond};
use crate::model::features::{ARef, Annot3, AnnotGroup, AnnotKind};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, RichText, Stroke};

/// A command waiting for picks or a click in the graphics window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ATool { Dim, RefDim, Ordinate, Tol, Note, Finish, Datum, DatumTarget, Gtol, Show, Move, Reattach }

impl ATool {
    fn prompt(self) -> &'static str {
        match self {
            ATool::Dim | ATool::RefDim => "Ein oder zwei Referenzen wählen (Kante, Fläche, Ebene, Achse, Punkt), dann zum Platzieren in einen leeren Bereich klicken.",
            ATool::Ordinate => "Die Basisebene oder -fläche wählen, dann die zu bemaßenden Elemente. Leerer Bereich oder Esc beendet.",
            ATool::Tol => "Die Bemaßung wählen, die eine Toleranz erhalten soll.",
            ATool::Note => "Optional eine Referenz für den Hinweispfeil wählen, dann zum Platzieren der Notiz in einen leeren Bereich klicken.",
            ATool::Finish => "Die Fläche wählen, dann zum Platzieren der Oberflächengüte in einen leeren Bereich klicken.",
            ATool::Datum => "Die Bezugsfläche oder -ebene wählen, dann zum Platzieren des Bezugssymbols in einen leeren Bereich klicken.",
            ATool::DatumTarget => "Punkt, Kante oder Fläche des Bezugsziels wählen, dann zum Platzieren in einen leeren Bereich klicken.",
            ATool::Gtol => "Das tolerierte Element wählen, dann zum Platzieren des Toleranzrahmens in einen leeren Bereich klicken.",
            ATool::Show => "Ein KE im Grafikfenster oder Modellbaum wählen, um seine Bemaßungen anzuzeigen.",
            ATool::Move => "In das Grafikfenster an die neue Position der gewählten Anmerkung klicken.",
            ATool::Reattach => "Die neuen Referenzen der gewählten Anmerkung wählen.",
        }
    }
    /// references a placed annotation needs: (min, max)
    fn refs(self) -> (usize, usize) {
        match self { ATool::Dim | ATool::RefDim => (1, 2), ATool::Note => (0, 1), ATool::Finish | ATool::Datum | ATool::DatumTarget | ATool::Gtol => (1, 1), _ => (0, 0) }
    }
}

/// Session state of the annotation commands.
#[derive(Default)]
pub struct Annot3Ui {
    pub tool: Option<ATool>,
    /// references picked for the running command
    pub refs: Vec<Pick>,
    /// base of the running Ordinatenbemaßung
    pub base: Option<ARef>,
    /// selected annotations
    pub sel: Vec<u64>,
    /// annotation whose properties dialog is open
    pub edit: Option<u64>,
    /// Abfrage list open
    pub query: bool,
    /// Anmerkungs-KE that new annotations go into
    pub active_group: Option<u64>,
}

/// Geometry a reference resolves to in the current model.
#[derive(Clone)]
enum G { Plane { n: V3, w: f64, c: V3 }, Pts(Vec<V3>), Line { p: V3, d: V3 }, Circle { c: V3, r: f64, n: V3 }, Point(V3) }

impl G {
    fn anchor(&self) -> V3 {
        match self {
            G::Plane { c, .. } => *c,
            G::Pts(p) => p.iter().fold(V3::ZERO, |a, b| a + *b) / p.len().max(1) as f64,
            G::Line { p, .. } => *p,
            G::Circle { c, .. } => *c,
            G::Point(p) => *p,
        }
    }
    fn sample(&self) -> Vec<V3> {
        match self {
            G::Pts(p) => p.clone(),
            G::Line { p, d } => (-40..=40).map(|k| *p + *d * (k as f64 * 2.5)).collect(),
            G::Plane { c, .. } => vec![*c],
            G::Circle { c, r, n } => { let u = safe_norm(n.any_orthonormal_vector(), V3::X); let v = n.cross(u); (0..48).map(|k| { let t = k as f64 / 48.0 * std::f64::consts::TAU; *c + (u * t.cos() + v * t.sin()) * *r }).collect() }
            G::Point(p) => vec![*p],
        }
    }
}

/// A dimension as drawn: value, end points on the geometry, and how it reads.
struct Measured { value: f64, a: V3, b: V3, angle: bool, diameter: bool }

const INK: Color32 = Color32::from_rgb(24, 30, 40);
const SELC: Color32 = Color32::from_rgb(20, 170, 60);

fn kind_icon(k: &AnnotKind) -> Icon {
    match k {
        AnnotKind::Dim { reference: false, .. } => Icon::DwgDim, AnnotKind::Dim { .. } => Icon::SwitchDims, AnnotKind::Ordinate { .. } => Icon::DimBound,
        AnnotKind::Note { .. } => Icon::Note3d, AnnotKind::Finish { .. } => Icon::SurfFinish, AnnotKind::Datum { .. } => Icon::DatumFeature,
        AnnotKind::DatumTarget { .. } => Icon::DatumTarget, AnnotKind::Gtol { .. } => Icon::GeomTol,
    }
}

fn kind_name(k: &AnnotKind) -> &'static str {
    match k {
        AnnotKind::Dim { reference: false, .. } => "Bemaßung", AnnotKind::Dim { .. } => "Referenzbemaßung", AnnotKind::Ordinate { .. } => "Ordinatenbemaßung",
        AnnotKind::Note { .. } => "Notiz", AnnotKind::Finish { .. } => "Oberflächengüte", AnnotKind::Datum { .. } => "Bezugssymbol",
        AnnotKind::DatumTarget { .. } => "Bezugsziel", AnnotKind::Gtol { .. } => "Geometrische Toleranz",
    }
}

fn tol_text(tol: Option<(f64, f64)>) -> String {
    match tol {
        Some((u, l)) if (u + l).abs() < 1e-12 => format!(" ±{}", fmt_num(u.abs(), 3)),
        Some((u, l)) => format!(" {}{} / {}{}", if u >= 0.0 { "+" } else { "−" }, fmt_num(u.abs(), 3), if l >= 0.0 { "+" } else { "−" }, fmt_num(l.abs(), 3)),
        None => String::new(),
    }
}

impl App {
    // ---------------- references ----------------

    fn aref_of(&self, p: Pick) -> Option<ARef> {
        let r = self.displayed();
        let t = &r.topo;
        Some(match p {
            Pick::Face(f) => { let face = t.faces.get(f)?; ARef::Face { p: face.center, n: face.normal } }
            Pick::Edge(e) => ARef::Edge { p: t.chains.get(e)?.mid() },
            Pick::Plane(id) => ARef::Plane(id),
            Pick::Axis(id) => ARef::Axis(id),
            Pick::Point(id) => ARef::Point(id),
            Pick::Feature(_) => return None,
        })
    }

    fn ref_geo(&self, a: &ARef) -> Option<G> {
        let r = self.displayed();
        let t = &r.topo;
        match a {
            ARef::Face { p, n } => {
                let face = &t.faces[t.match_face(*p, *n)?];
                if face.planar { return Some(G::Plane { n: face.normal, w: face.offset, c: face.center }); }
                let pts: Vec<V3> = face.polys.iter().flat_map(|&pi| t.polys[pi].iter().map(|&i| t.positions[i])).take(400).collect();
                Some(G::Pts(pts))
            }
            ARef::Edge { p } => {
                let tol = (t.bbox.diagonal() * 1e-3).max(1e-4);
                let c = t.chains.iter().min_by(|a, b| a.distance_to(*p).partial_cmp(&b.distance_to(*p)).unwrap()).filter(|c| c.distance_to(*p) < tol)?;
                if c.closed && c.points.len() > 8 {
                    let cen = c.points.iter().fold(V3::ZERO, |a, b| a + *b) / c.points.len() as f64;
                    let rad = c.points.iter().map(|q| (*q - cen).length()).sum::<f64>() / c.points.len() as f64;
                    let n = safe_norm((c.points[0] - cen).cross(c.points[c.points.len() / 4] - cen), V3::Z);
                    return Some(G::Circle { c: cen, r: rad, n });
                }
                Some(G::Pts(c.points.clone()))
            }
            ARef::Plane(id) => { let p = r.plane(*id)?; let n = p.frame.z(); Some(G::Plane { n, w: n.dot(p.frame.origin), c: p.frame.origin }) }
            ARef::Axis(id) => { let a = r.axis(*id)?; Some(G::Line { p: a.point, d: a.dir }) }
            ARef::Point(id) => r.points.iter().find(|q| q.id == *id).map(|q| G::Point(q.p)),
        }
    }

    /// The value of a dimension on these references (one circle or edge, or two elements).
    fn measure_refs(&self, refs: &[ARef]) -> Option<Measured> {
        let g: Vec<G> = refs.iter().map(|r| self.ref_geo(r)).collect::<Option<_>>()?;
        match g.as_slice() {
            [G::Circle { c, r, n }] => {
                let u = safe_norm(self.camera.right() - *n * self.camera.right().dot(*n), n.any_orthonormal_vector());
                Some(Measured { value: 2.0 * r, a: *c + u * *r, b: *c - u * *r, angle: false, diameter: true })
            }
            [G::Pts(p)] if p.len() >= 2 => {
                let (a, b) = (p[0], *p.last().unwrap());
                Some(Measured { value: (b - a).length(), a, b, angle: false, diameter: false })
            }
            [x, y] => {
                if let (G::Plane { n: n1, w: w1, c: c1 }, G::Plane { n: n2, w: w2, c: c2 }) = (x, y) {
                    let cos = n1.dot(*n2);
                    if cos.abs() < 0.9999 {
                        let ang = cos.clamp(-1.0, 1.0).acos().to_degrees();
                        return Some(Measured { value: ang.min(180.0 - ang), a: *c1, b: *c2, angle: true, diameter: false });
                    }
                    let d = (w1 - w2 * cos.signum()).abs();
                    let b = *c1 - *n1 * (n1.dot(*c1) - w2 * cos.signum());
                    let _ = c2;
                    return Some(Measured { value: d, a: *c1, b, angle: false, diameter: false });
                }
                let plane_pts = |n: V3, w: f64, g: &G| -> Option<Measured> {
                    let pts = g.sample();
                    let q = pts.iter().copied().min_by(|a, b| (a.dot(n) - w).abs().partial_cmp(&(b.dot(n) - w).abs()).unwrap())?;
                    let d = q.dot(n) - w;
                    Some(Measured { value: d.abs(), a: q, b: q - n * d, angle: false, diameter: false })
                };
                match (x, y) {
                    (G::Plane { n, w, .. }, g) | (g, G::Plane { n, w, .. }) => plane_pts(*n, *w, g),
                    (G::Line { p: p1, d: d1 }, G::Line { p: p2, d: d2 }) if d1.cross(*d2).length() < 1e-9 => {
                        let off = *p2 - *p1;
                        let perp = off - *d1 * off.dot(*d1);
                        Some(Measured { value: perp.length(), a: *p1, b: *p1 + perp, angle: false, diameter: false })
                    }
                    (ga, gb) => {
                        let (pa, pb) = (ga.sample(), gb.sample());
                        let mut best = (f64::INFINITY, V3::ZERO, V3::ZERO);
                        for p in &pa { for q in &pb { let d = (*p - *q).length(); if d < best.0 { best = (d, *p, *q); } } }
                        best.0.is_finite().then_some(Measured { value: best.0, a: best.1, b: best.2, angle: false, diameter: false })
                    }
                }
            }
            _ => None,
        }
    }

    /// The text an annotation shows.
    fn annot_text(&self, a: &Annot3) -> String {
        match &a.kind {
            AnnotKind::Dim { reference, tol } => {
                let Some(m) = self.measure_refs(&a.refs) else { return "?".into() };
                let v = format!("{}{}{}", if m.diameter { "⌀" } else { "" }, fmt_num(m.value, 2), if m.angle { "°" } else { "" });
                let t = format!("{}{}", v, tol_text(*tol));
                if *reference { format!("({})", t) } else { t }
            }
            AnnotKind::Ordinate { base } => {
                let (Some(bg), Some(tg)) = (self.ref_geo(base), a.refs.first().and_then(|r| self.ref_geo(r))) else { return "?".into() };
                match bg { G::Plane { n, w, .. } => fmt_num((tg.anchor().dot(n) - w).abs(), 2), _ => "?".into() }
            }
            AnnotKind::Note { text } => text.clone(),
            AnnotKind::Finish { value, .. } => value.clone(),
            AnnotKind::Datum { letter } => letter.clone(),
            AnnotKind::DatumTarget { label } => label.clone(),
            AnnotKind::Gtol { ch, diameter, value, mat, datums } => {
                let mut s = format!("{} {}{}", ch.name(), if *diameter { "⌀" } else { "" }, fmt_num(*value, 3));
                if let Some(l) = mat.letter() { s += &format!(" ({})", l); }
                for d in datums.iter().filter(|d| !d.is_empty()) { s += &format!(" | {}", d); }
                s
            }
        }
    }

    // ---------------- placement ----------------

    /// A screen point (relative to the viewport) on the active annotation plane through `anchor`.
    fn place_point(&self, px: (f32, f32), anchor: V3) -> V3 {
        let ap = &self.creo.annot;
        let n = ap.list.get(ap.active).and_then(|p| p.rot).map(|q| q * V3::Z).unwrap_or(self.camera.back());
        let (o, d) = self.camera.ray(px, self.viewport_size);
        let try_plane = |n: V3| { let den = d.dot(n); (den.abs() > 1e-6).then(|| o + d * ((anchor - o).dot(n) / den)) };
        try_plane(n).or_else(|| try_plane(self.camera.back())).unwrap_or(anchor)
    }

    fn annot_plane_name(&self) -> String { self.creo.annot.list.get(self.creo.annot.active).map(|p| p.name.clone()).unwrap_or_default() }

    pub fn start_annot_tool(&mut self, t: ATool) {
        if self.in_asm() || self.sketch.is_some() { self.say("Anmerkungen werden im Teilemodus erzeugt.", MsgKind::Warning); return; }
        self.creo.annot3.base = None;
        // geometry already selected becomes the references
        self.creo.annot3.refs = if t.refs().1 > 0 { self.selection.iter().copied().filter(|p| !matches!(p, Pick::Feature(_))).take(t.refs().1).collect() } else { vec![] };
        match t {
            ATool::Move | ATool::Reattach | ATool::Tol if self.creo.annot3.sel.is_empty() => {
                self.say("Zuerst eine Anmerkung im Grafikfenster wählen.", MsgKind::Prompt);
            }
            ATool::Tol => {
                let dim = self.creo.annot3.sel.iter().copied().find(|id| self.data.annots.iter().any(|a| a.id == *id && matches!(a.kind, AnnotKind::Dim { .. })));
                if let Some(id) = dim {
                    if let Some(a) = self.data.annots.iter_mut().find(|a| a.id == id) { if let AnnotKind::Dim { tol, .. } = &mut a.kind { if tol.is_none() { *tol = Some((0.1, -0.1)); self.dirty = true; } } }
                    self.creo.annot3.edit = Some(id);
                    self.creo.annot3.tool = None;
                    return;
                }
            }
            ATool::Show => {
                if let Some(id) = self.tree_selection.first().copied().filter(|id| self.data.feature(*id).is_some()) { self.start_edit_dims(id); self.creo.annot3.tool = None; return; }
            }
            _ => {}
        }
        self.creo.annot3.tool = Some(t);
        self.say(t.prompt(), MsgKind::Prompt);
    }

    fn add_annot(&mut self, kind: AnnotKind, refs: Vec<ARef>, pos: V3) -> u64 {
        let id = self.data.annots.iter().map(|a| a.id).max().unwrap_or(0).max(self.data.annot_groups.iter().map(|g| g.id).max().unwrap_or(0)) + 1;
        let a = Annot3 { id, kind, refs, pos, plane: self.annot_plane_name(), group: self.creo.annot3.active_group.filter(|g| self.data.annot_groups.iter().any(|x| x.id == *g)) };
        let name = kind_name(&a.kind);
        self.commit("", |d| d.annots.push(a));
        self.creo.annot3.sel = vec![id];
        self.say(format!("{} erzeugt.", name), MsgKind::Success);
        id
    }

    fn next_datum_letter(&self) -> String {
        let used: Vec<String> = self.data.annots.iter().filter_map(|a| if let AnnotKind::Datum { letter } = &a.kind { Some(letter.clone()) } else { None }).collect();
        ('A'..='Z').map(|c| c.to_string()).find(|l| !used.contains(l)).unwrap_or_else(|| "A".into())
    }

    /// A click in the graphics window while annotations are active. Returns true when it was used.
    pub fn annot_click(&mut self, ctx: &egui::Context, hp: Pos2, local: (f32, f32), pick: Option<Pick>, additive: bool) -> bool {
        let hits: Vec<(Rect, u64)> = ctx.data(|d| d.get_temp(egui::Id::new("annot_hits"))).unwrap_or_default();
        let hit = hits.into_iter().find(|(r, _)| r.contains(hp)).map(|h| h.1);
        let Some(tool) = self.creo.annot3.tool else {
            if let Some(id) = hit {
                if additive { if let Some(i) = self.creo.annot3.sel.iter().position(|s| *s == id) { self.creo.annot3.sel.remove(i); } else { self.creo.annot3.sel.push(id); } }
                else { self.creo.annot3.sel = vec![id]; }
                self.selection.clear();
                return true;
            }
            if !additive { self.creo.annot3.sel.clear(); }
            return false;
        };
        match tool {
            ATool::Move => {
                let Some(&id) = self.creo.annot3.sel.first() else { return true };
                let anchor = self.data.annots.iter().find(|a| a.id == id).map(|a| a.pos).unwrap_or(V3::ZERO);
                let p = self.place_point(local, anchor);
                self.commit("Anmerkung verschoben.", |d| if let Some(a) = d.annots.iter_mut().find(|a| a.id == id) { a.pos = p; });
                self.creo.annot3.tool = None;
            }
            ATool::Tol => {
                if let Some(id) = hit.filter(|id| self.data.annots.iter().any(|a| a.id == *id && matches!(a.kind, AnnotKind::Dim { .. }))) {
                    self.creo.annot3.sel = vec![id];
                    self.creo.annot3.tool = None;
                    self.start_annot_tool(ATool::Tol);
                } else { self.say(ATool::Tol.prompt(), MsgKind::Prompt); }
            }
            ATool::Show => {
                if let Some(o) = pick.and_then(|p| self.owner(p)).filter(|o| self.data.feature(*o).is_some()) { self.creo.annot3.tool = None; self.start_edit_dims(o); }
            }
            ATool::Reattach => {
                let Some(&id) = self.creo.annot3.sel.first() else { return true };
                let Some(p) = pick else { return true };
                if !self.creo.annot3.refs.contains(&p) { self.creo.annot3.refs.push(p); }
                let need = self.data.annots.iter().find(|a| a.id == id).map(|a| a.refs.len().max(1)).unwrap_or(1);
                if self.creo.annot3.refs.len() >= need {
                    let refs: Vec<ARef> = self.creo.annot3.refs.iter().filter_map(|p| self.aref_of(*p)).collect();
                    self.commit("Anmerkung neu verbunden.", |d| if let Some(a) = d.annots.iter_mut().find(|a| a.id == id) { a.refs = refs; });
                    self.creo.annot3.tool = None;
                    self.creo.annot3.refs.clear();
                } else { self.say(format!("Noch {} Referenz(en) wählen.", need - self.creo.annot3.refs.len()), MsgKind::Prompt); }
            }
            ATool::Ordinate => {
                match pick.and_then(|p| self.aref_of(p)) {
                    Some(r) => {
                        if self.creo.annot3.base.is_none() {
                            if !matches!(self.ref_geo(&r), Some(G::Plane { .. })) { self.say("Die Basis muss eine Ebene oder ebene Fläche sein.", MsgKind::Warning); return true; }
                            let p = self.ref_geo(&r).map(|g| g.anchor()).unwrap_or(V3::ZERO) + self.camera.up() * self.camera.pixel_size(self.viewport_size.1) * 30.0;
                            self.creo.annot3.base = Some(r.clone());
                            self.add_annot(AnnotKind::Ordinate { base: r.clone() }, vec![r], p);
                            self.say("Basis gesetzt. Jetzt die zu bemaßenden Elemente wählen.", MsgKind::Prompt);
                        } else {
                            let base = self.creo.annot3.base.clone().unwrap();
                            let p = self.ref_geo(&r).map(|g| g.anchor()).unwrap_or(V3::ZERO) + self.camera.up() * self.camera.pixel_size(self.viewport_size.1) * 30.0;
                            self.add_annot(AnnotKind::Ordinate { base }, vec![r], p);
                        }
                    }
                    None => { self.creo.annot3.tool = None; self.creo.annot3.base = None; }
                }
            }
            _ => {
                let (min, max) = tool.refs();
                if let Some(p) = pick {
                    if max == 0 { return true; }
                    if let Some(i) = self.creo.annot3.refs.iter().position(|q| *q == p) { self.creo.annot3.refs.remove(i); }
                    else { if self.creo.annot3.refs.len() >= max { self.creo.annot3.refs.remove(0); } self.creo.annot3.refs.push(p); }
                    return true;
                }
                if self.creo.annot3.refs.len() < min { self.say(tool.prompt(), MsgKind::Prompt); return true; }
                let refs: Vec<ARef> = self.creo.annot3.refs.iter().filter_map(|p| self.aref_of(*p)).collect();
                let anchor = refs.first().and_then(|r| self.ref_geo(r)).map(|g| g.anchor()).unwrap_or(self.camera.target);
                let pos = self.place_point(local, anchor);
                let kind = match tool {
                    ATool::Dim | ATool::RefDim => {
                        if self.measure_refs(&refs).is_none() { self.say("Diese Referenzen lassen sich nicht bemaßen — eine Kreiskante, eine gerade Kante oder zwei Elemente wählen.", MsgKind::Warning); self.creo.annot3.refs.clear(); return true; }
                        AnnotKind::Dim { reference: tool == ATool::RefDim, tol: None }
                    }
                    ATool::Note => AnnotKind::Note { text: "NOTIZ".into() },
                    ATool::Finish => AnnotKind::Finish { kind: FinishKind::Removal, value: "Ra 3.2".into() },
                    ATool::Datum => AnnotKind::Datum { letter: self.next_datum_letter() },
                    ATool::DatumTarget => {
                        let n = self.data.annots.iter().filter(|a| matches!(a.kind, AnnotKind::DatumTarget { .. })).count() + 1;
                        AnnotKind::DatumTarget { label: format!("A{}", n) }
                    }
                    _ => AnnotKind::Gtol { ch: GChar::Flatness, diameter: false, value: 0.05, mat: MatCond::Rfs, datums: vec![String::new(); 3] },
                };
                let opens = !matches!(kind, AnnotKind::Dim { .. });
                let id = self.add_annot(kind, refs, pos);
                if opens { self.creo.annot3.edit = Some(id); }
                self.creo.annot3.tool = None;
                self.creo.annot3.refs.clear();
            }
        }
        true
    }

    fn remove_annots(&mut self) {
        let ids = self.creo.annot3.sel.clone();
        if ids.is_empty() { self.say("Zuerst Anmerkungen im Grafikfenster wählen.", MsgKind::Prompt); return; }
        self.commit(&format!("{} Anmerkung(en) entfernt.", ids.len()), |d| d.annots.retain(|a| !ids.contains(&a.id)));
        self.creo.annot3.sel.clear();
        if self.creo.annot3.edit.is_some_and(|e| ids.contains(&e)) { self.creo.annot3.edit = None; }
    }

    /// Bemaßungen bereinigen: dimensions placed at even distances from their geometry.
    fn clean_dims(&mut self) {
        let px = self.camera.pixel_size(self.viewport_size.1);
        let center = self.displayed().topo.bbox.center();
        let sel = self.creo.annot3.sel.clone();
        let mut moves = vec![];
        let mut k = 0;
        for a in &self.data.annots {
            if !matches!(a.kind, AnnotKind::Dim { .. }) || (!sel.is_empty() && !sel.contains(&a.id)) { continue; }
            let Some(m) = self.measure_refs(&a.refs) else { continue };
            let mid = (m.a + m.b) * 0.5;
            let along = m.b - m.a;
            let mut out = safe_norm(along.cross(self.camera.back()), self.camera.up());
            if out.dot(mid - center) < 0.0 { out = -out; }
            moves.push((a.id, mid + out * px * (34.0 + 22.0 * (k % 4) as f64)));
            k += 1;
        }
        if moves.is_empty() { self.say("Keine Bemaßungen zum Bereinigen.", MsgKind::Info); return; }
        let n = moves.len();
        self.commit(&format!("{} Bemaßung(en) bereinigt.", n), |d| for (id, p) in moves { if let Some(a) = d.annots.iter_mut().find(|a| a.id == id) { a.pos = p; } });
    }

    fn new_annot_group(&mut self, datum_target: bool) {
        let id = self.data.annots.iter().map(|a| a.id).max().unwrap_or(0).max(self.data.annot_groups.iter().map(|g| g.id).max().unwrap_or(0)) + 1;
        let n = self.data.annot_groups.iter().filter(|g| g.datum_target == datum_target).count() + 1;
        let name = if datum_target { format!("Bezugsziel-Anmerkungs-KE {}", n) } else { format!("Anmerkungs-KE {}", n) };
        let sel = self.creo.annot3.sel.clone();
        let g = AnnotGroup { id, name: name.clone(), datum_target };
        self.commit("", |d| {
            d.annot_groups.push(g);
            for a in d.annots.iter_mut().filter(|a| sel.contains(&a.id)) { a.group = Some(id); }
        });
        self.creo.annot3.active_group = Some(id);
        self.say(format!("{} erzeugt — neue Anmerkungen werden ihm hinzugefügt.", name), MsgKind::Success);
        if datum_target { self.start_annot_tool(ATool::DatumTarget); }
    }

    /// FREO_DEMO_ANNOTS: one annotation of each kind on the bracket, made through the commands'
    /// own pick-and-place path (documentation screenshots).
    pub fn demo_annotations(&mut self, ctx: &egui::Context) {
        let t = &self.displayed().topo;
        let (w, h) = self.viewport_size;
        let flat: Vec<(usize, f64, f64)> = t.faces.iter().enumerate().filter(|(_, f)| f.planar && f.normal.y.abs() > 0.99).map(|(i, f)| (i, f.normal.y, f.center.y)).collect();
        let by = |up: bool, hi: bool| flat.iter().filter(|f| (f.1 > 0.0) == up).max_by(|a, b| if hi { a.2.partial_cmp(&b.2).unwrap() } else { b.2.partial_cmp(&a.2).unwrap() }).map(|f| f.0);
        let (Some(bottom), Some(boss_top), Some(base_top)) = (by(false, false), by(true, true), by(true, false)) else { return };
        let circles: Vec<(usize, f64)> = t.chains.iter().enumerate().filter(|(_, c)| c.closed && c.points.len() > 8).map(|(i, c)| (i, c.points.iter().map(|p| p.y).sum::<f64>() / c.points.len() as f64)).collect();
        let top_circle = circles.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).map(|c| c.0);
        let low_circle = circles.iter().min_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).map(|c| c.0);
        let rect_hp = |fx: f32, fy: f32| (pos2(fx * w, fy * h), (fx * w, fy * h));
        let place = |app: &mut App, tool: ATool, refs: Vec<Pick>, fx: f32, fy: f32| {
            app.start_annot_tool(tool);
            app.creo.annot3.refs = refs;
            let (hp, l) = rect_hp(fx, fy);
            app.annot_click(ctx, hp, l, None, false);
        };
        if let Some(c) = top_circle { place(self, ATool::Dim, vec![Pick::Edge(c)], 0.80, 0.22); }
        place(self, ATool::Dim, vec![Pick::Face(bottom), Pick::Face(base_top)], 0.12, 0.70);
        place(self, ATool::Gtol, vec![Pick::Face(boss_top)], 0.66, 0.10);
        place(self, ATool::Datum, vec![Pick::Face(bottom)], 0.30, 0.90);
        place(self, ATool::Finish, vec![Pick::Face(base_top)], 0.20, 0.40);
        place(self, ATool::Note, vec![], 0.10, 0.12);
        if let Some(c) = low_circle { place(self, ATool::DatumTarget, vec![Pick::Edge(c)], 0.88, 0.62); }
        place(self, ATool::RefDim, vec![Pick::Face(boss_top), Pick::Face(base_top)], 0.90, 0.40);
        // Ordinatenbemaßung: base, then one element
        self.start_annot_tool(ATool::Ordinate);
        let (hp, l) = rect_hp(0.5, 0.5);
        self.annot_click(ctx, hp, l, Some(Pick::Face(bottom)), false);
        self.annot_click(ctx, hp, l, Some(Pick::Face(boss_top)), false);
        self.annot_click(ctx, hp, l, None, false);
        self.creo.annot3.edit = None;
        // the gtol goes into an Anmerkungs-KE
        self.creo.annot3.sel = self.data.annots.iter().filter(|a| matches!(a.kind, AnnotKind::Gtol { .. } | AnnotKind::Datum { .. })).map(|a| a.id).collect();
        self.new_annot_group(false);
        match std::env::var("FREO_DEMO_POPUP").as_deref() {
            Ok("annot_query") => self.creo.annot3.query = true,
            Ok("annot_props") => self.creo.annot3.edit = self.data.annots.iter().find(|a| matches!(a.kind, AnnotKind::Gtol { .. })).map(|a| a.id),
            _ => self.creo.annot3.sel.clear(),
        }
    }

    // ---------------- ribbon ----------------

    pub fn annot3_ribbon_groups(&mut self, ui: &mut egui::Ui) {
        let tool = self.creo.annot3.tool;
        let on = |t: ATool| tool == Some(t);
        let ((), t) = group_drop(ui, "Anmerkungen verwalten", |ui| {
            if large(ui, Icon::ShowAnnot, "Anmerkungen\nanzeigen", true, self.edit_dims.is_some() || on(ATool::Show)).on_hover_text("Die Bemaßungen eines KE anzeigen").clicked() { self.start_annot_tool(ATool::Show); }
            stack(ui, |ui| {
                if tiny(ui, Icon::AnnotMove, "Verschieben", true, on(ATool::Move), false).clicked() { self.start_annot_tool(ATool::Move); }
                if tiny(ui, Icon::AnnotReattach, "Neu verbinden", true, on(ATool::Reattach), false).clicked() { self.start_annot_tool(ATool::Reattach); }
                if tiny(ui, Icon::Delete, "Entfernen", true, false, false).clicked() { self.remove_annots(); }
            });
        });
        group_menu(ui, "annot_manage", &t, |ui| {
            if menu_item(ui, Icon::ShowAnnot, "Anmerkungen anzeigen", true) { self.start_annot_tool(ATool::Show); }
            if menu_item(ui, Icon::AnnotMove, "Verschieben", true) { self.start_annot_tool(ATool::Move); }
            if menu_item(ui, Icon::AnnotReattach, "Neu verbinden", true) { self.start_annot_tool(ATool::Reattach); }
            if menu_item(ui, Icon::Delete, "Entfernen", true) { self.remove_annots(); }
            let shown = self.creo.gfx.annotations;
            if menu_item(ui, Icon::AnnotShow, if shown { "Alle Anmerkungen ausblenden" } else { "Alle Anmerkungen einblenden" }, true) { self.creo.gfx.annotations = !shown; }
        });
        let tools: [[(Icon, &str, ATool); 3]; 3] = [
            [(Icon::DwgDim, "Bemaßung", ATool::Dim), (Icon::DimBound, "Ordinatenbemaßung", ATool::Ordinate), (Icon::DimTol, "Toleranz", ATool::Tol)],
            [(Icon::SwitchDims, "Referenzbemaßung", ATool::RefDim), (Icon::Cleanup, "Bemaßungen bereinigen", ATool::Show), (Icon::SurfFinish, "Oberflächengüte", ATool::Finish)],
            [(Icon::DatumFeature, "Bezugssymbol", ATool::Datum), (Icon::Note3d, "Notiz", ATool::Note), (Icon::GeomTol, "Geometrische Toleranz", ATool::Gtol)],
        ];
        let mut run: Option<(ATool, bool)> = None;
        let ((), t) = group_drop(ui, "Anmerkungen", |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add_space(2.0);
                for row in &tools {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        for (ic, name, t) in row {
                            let clean = *name == "Bemaßungen bereinigen";
                            if tiny(ui, *ic, name, true, !clean && on(*t), false).clicked() { run = Some((*t, clean)); }
                        }
                    });
                }
            });
        });
        group_menu(ui, "annot_tools", &t, |ui| {
            for row in &tools { for (ic, name, t) in row { if menu_item(ui, *ic, name, true) { run = Some((*t, *name == "Bemaßungen bereinigen")); } } }
        });
        match run { Some((_, true)) => self.clean_dims(), Some((t, false)) => self.start_annot_tool(t), None => {} }
        let ((), t) = group_drop(ui, "Anmerkungs-KEs", |ui| {
            if large(ui, Icon::AnnotFeature, "Anmerkungs-\nKE", true, false).on_hover_text("Ein Anmerkungs-KE erzeugen; gewählte und neue Anmerkungen gehören dazu").clicked() { self.new_annot_group(false); }
            if large(ui, Icon::DatumTarget, "Bezugsziel-\nAnmerkungs-KE", true, false).on_hover_text("Ein Bezugsziel-Anmerkungs-KE erzeugen und ein Bezugsziel platzieren").clicked() { self.new_annot_group(true); }
        });
        group_menu(ui, "annot_feats", &t, |ui| {
            if menu_item(ui, Icon::AnnotFeature, "Anmerkungs-KE", true) { self.new_annot_group(false); }
            if menu_item(ui, Icon::DatumTarget, "Bezugsziel-Anmerkungs-KE", true) { self.new_annot_group(true); }
        });
        group(ui, "Abfrage", |ui| {
            if large(ui, Icon::Probe, "", true, self.creo.annot3.query).on_hover_text("Anmerkungen abfragen: alle Anmerkungen, ihre Referenzen und Anmerkungs-KEs").clicked() { self.creo.annot3.query = !self.creo.annot3.query; }
        });
        let free = self.editor.is_none() && self.sketch.is_none();
        let ((), t) = group_drop(ui, "Bezüge", |ui| {
            stack(ui, |ui| {
                if tiny(ui, Icon::Plane, "Ebene", free, false, false).clicked() { self.start_feature(FeatureKind::new_plane()); }
                if tiny(ui, Icon::Axis, "Achse", free, false, false).clicked() { self.start_feature(FeatureKind::new_axis()); }
                if tiny(ui, Icon::Curvature, "Kurve", free, false, false).clicked() { self.start_feature(FeatureKind::new_curve()); }
            });
        });
        group_menu(ui, "annot_datums", &t, |ui| {
            if menu_item(ui, Icon::Plane, "Ebene", free) { self.start_feature(FeatureKind::new_plane()); }
            if menu_item(ui, Icon::Axis, "Achse", free) { self.start_feature(FeatureKind::new_axis()); }
            if menu_item(ui, Icon::Curvature, "Kurve", free) { self.start_feature(FeatureKind::new_curve()); }
            if menu_item(ui, Icon::Point, "Punkt", free) { self.start_feature(FeatureKind::new_point()); }
            if menu_item(ui, Icon::Csys, "Koordinatensystem", free) { self.start_feature(FeatureKind::new_csys()); }
        });
    }

    // ---------------- drawing ----------------

    /// All annotations over the model; their label rectangles are kept for picking.
    pub fn draw_annots(&self, painter: &egui::Painter, to_screen: &dyn Fn(V3) -> Option<Pos2>) {
        let mut hits: Vec<(Rect, u64)> = vec![];
        let show = self.creo.gfx.annotations && !self.in_asm() && self.sketch.is_none();
        if show {
            for a in &self.data.annots {
                let sel = self.creo.annot3.sel.contains(&a.id) || self.creo.annot3.edit == Some(a.id);
                let col = if sel { SELC } else { INK };
                let Some(p) = to_screen(a.pos) else { continue };
                let anchors: Vec<V3> = a.refs.iter().filter_map(|r| self.ref_geo(r)).map(|g| g.anchor()).collect();
                let anchor_s = anchors.first().and_then(|q| to_screen(*q));
                let st = Stroke::new(if sel { 1.6_f32 } else { 1.0 }, col);
                let r = match &a.kind {
                    AnnotKind::Dim { .. } => {
                        let text = self.annot_text(a);
                        match self.measure_refs(&a.refs) {
                            Some(m) if !m.diameter && !m.angle => {
                                if let (Some(sa), Some(sb)) = (to_screen(m.a), to_screen(m.b)) { dim_lines(painter, sa, sb, p, st); }
                            }
                            Some(m) => { if let Some(sa) = to_screen(m.a) { leader(painter, sa, p, st, true); } }
                            None => {}
                        }
                        label(painter, p, &text, col, true)
                    }
                    AnnotKind::Ordinate { .. } => {
                        if let Some(s) = anchor_s { painter.line_segment([s, p], st); }
                        label(painter, p, &self.annot_text(a), col, true)
                    }
                    AnnotKind::Note { text } => {
                        let r = label(painter, p, text, col, true);
                        if let Some(s) = anchor_s { leader(painter, s, nearest_edge(r, s), st, true); }
                        r
                    }
                    AnnotKind::Finish { kind, value } => {
                        if let Some(s) = anchor_s { leader(painter, s, p + vec2(-4.0, 9.0), st, true); }
                        finish_symbol(painter, p, *kind, value, col)
                    }
                    AnnotKind::Datum { letter } => {
                        if let Some(s) = anchor_s {
                            painter.line_segment([s, p], st);
                            let d = (p - s).normalized();
                            let n = vec2(-d.y, d.x);
                            painter.add(egui::Shape::convex_polygon(vec![s + n * 6.0, s - n * 6.0, s + d * 9.0], col, Stroke::NONE));
                        }
                        let r = Rect::from_center_size(p, vec2(24.0, 24.0));
                        painter.rect(r, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 220), st, egui::StrokeKind::Inside);
                        painter.text(p, Align2::CENTER_CENTER, letter, FontId::proportional(15.0), col);
                        r
                    }
                    AnnotKind::DatumTarget { label: l } => {
                        let rad = 18.0;
                        if let Some(s) = anchor_s {
                            painter.line_segment([s, p + (s - p).normalized() * rad], st);
                            for d in [vec2(4.0, 4.0), vec2(4.0, -4.0)] { painter.line_segment([s - d, s + d], Stroke::new(1.6_f32, col)); }
                        }
                        painter.circle(p, rad, Color32::from_rgba_unmultiplied(255, 255, 255, 220), st);
                        painter.line_segment([p - vec2(rad, 0.0), p + vec2(rad, 0.0)], st);
                        painter.text(p + vec2(0.0, 8.0), Align2::CENTER_CENTER, l, FontId::proportional(12.0), col);
                        Rect::from_center_size(p, vec2(2.0 * rad, 2.0 * rad))
                    }
                    AnnotKind::Gtol { ch, diameter, value, mat, datums } => {
                        let r = gtol_frame(painter, p, *ch, *diameter, *value, *mat, datums, col);
                        if let Some(s) = anchor_s { leader(painter, s, pos2(r.left(), r.center().y), st, true); }
                        r
                    }
                };
                if sel { painter.rect_stroke(r.expand(3.0), 2.0, Stroke::new(1.0_f32, SELC), egui::StrokeKind::Outside); }
                hits.push((r, a.id));
            }
        }
        // references picked for the running command
        for p in &self.creo.annot3.refs {
            if let Some(s) = self.aref_of(*p).and_then(|r| self.ref_geo(&r)).and_then(|g| to_screen(g.anchor())) {
                painter.circle(s, 5.0, Color32::from_rgb(250, 140, 0), Stroke::new(1.0_f32, Color32::WHITE));
            }
        }
        painter.ctx().data_mut(|d| d.insert_temp(egui::Id::new("annot_hits"), hits));
    }

    // ---------------- dialogs ----------------

    /// Properties of one annotation, the Abfrage list, and Esc for a running command.
    pub fn annot3_windows(&mut self, ctx: &egui::Context) {
        if self.creo.annot3.tool.is_some() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.creo.annot3.tool = None;
            self.creo.annot3.refs.clear();
            self.creo.annot3.base = None;
            self.say("Befehl abgebrochen.", MsgKind::Info);
        }
        if !self.creo.annot3.sel.is_empty() && self.creo.annot3.tool.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Delete)) && !ctx.wants_keyboard_input() { self.remove_annots(); }
        self.annot_props(ctx);
        self.annot_query(ctx);
    }

    fn annot_props(&mut self, ctx: &egui::Context) {
        let Some(id) = self.creo.annot3.edit else { return };
        let Some(idx) = self.data.annots.iter().position(|a| a.id == id) else { self.creo.annot3.edit = None; return };
        let mut a = self.data.annots[idx].clone();
        let groups = self.data.annot_groups.clone();
        let value = self.annot_text(&a);
        let (mut open, mut close) = (true, false);
        egui::Window::new("Anmerkung").id(egui::Id::new("annot_props")).title_bar(false).collapsible(false).resizable(false).default_pos(pos2(470.0, 160.0)).show(ctx, |ui| {
            if dialog_header(ui, kind_name(&a.kind)) { open = false; }
            ui.set_width(320.0);
            ui.add_space(6.0);
            let row = |ui: &mut egui::Ui, l: &str| { ui.add_sized(vec2(110.0, 24.0), egui::Label::new(RichText::new(l).size(14.0))); };
            match &mut a.kind {
                AnnotKind::Dim { reference, tol } => {
                    ui.horizontal(|ui| { row(ui, "Wert:"); ui.label(RichText::new(value.clone()).size(14.0).strong()); });
                    ui.checkbox(reference, "Referenzbemaßung (in Klammern)");
                    fieldset(ui, "Toleranz", |ui| {
                        let mut on = tol.is_some();
                        if ui.checkbox(&mut on, "Toleranz anzeigen").changed() { *tol = if on { Some((0.1, -0.1)) } else { None }; }
                        if let Some((u, l)) = tol {
                            ui.horizontal(|ui| { row(ui, "Oberes Abmaß:"); ui.add(egui::DragValue::new(u).speed(0.01).max_decimals(3)); });
                            ui.horizontal(|ui| { row(ui, "Unteres Abmaß:"); ui.add(egui::DragValue::new(l).speed(0.01).max_decimals(3)); });
                            if ui.button("Symmetrisch (±)").clicked() { *l = -*u; }
                        }
                    });
                }
                AnnotKind::Ordinate { .. } => { ui.horizontal(|ui| { row(ui, "Wert:"); ui.label(RichText::new(value.clone()).size(14.0).strong()); }); }
                AnnotKind::Note { text } => { ui.label("Text:"); ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(300.0)); }
                AnnotKind::Finish { kind, value } => {
                    ui.horizontal(|ui| { row(ui, "Verfahren:"); egui::ComboBox::from_id_salt("fin_kind").width(190.0).selected_text(kind.name()).show_ui(ui, |ui| for k in FinishKind::ALL { ui.selectable_value(kind, k, k.name()); }); });
                    ui.horizontal(|ui| { row(ui, "Anforderung:"); ui.add(egui::TextEdit::singleline(value).desired_width(180.0)); });
                }
                AnnotKind::Datum { letter } => { ui.horizontal(|ui| { row(ui, "Bezug:"); ui.add(egui::TextEdit::singleline(letter).desired_width(60.0).char_limit(3)); }); }
                AnnotKind::DatumTarget { label } => { ui.horizontal(|ui| { row(ui, "Bezeichnung:"); ui.add(egui::TextEdit::singleline(label).desired_width(80.0).char_limit(5)); }); }
                AnnotKind::Gtol { ch, diameter, value, mat, datums } => {
                    ui.horizontal(|ui| { row(ui, "Eigenschaft:"); egui::ComboBox::from_id_salt("gt_ch").width(180.0).selected_text(ch.name()).show_ui(ui, |ui| for c in GChar::ALL { ui.selectable_value(ch, c, c.name()); }); });
                    ui.horizontal(|ui| { row(ui, "Toleranzwert:"); ui.add(egui::DragValue::new(value).speed(0.005).range(0.0..=100.0).max_decimals(3)); ui.checkbox(diameter, "⌀ Zone"); });
                    ui.horizontal(|ui| { row(ui, "Materialbed.:"); egui::ComboBox::from_id_salt("gt_mat").width(180.0).selected_text(mat.name()).show_ui(ui, |ui| for m in MatCond::ALL { ui.selectable_value(mat, m, m.name()); }); });
                    if !ch.form() {
                        datums.resize(3, String::new());
                        ui.horizontal(|ui| {
                            row(ui, "Bezüge:");
                            for d in datums.iter_mut() { ui.add(egui::TextEdit::singleline(d).desired_width(40.0).char_limit(3)); }
                        });
                    }
                }
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                row(ui, "Anmerkungs-KE:");
                let cur = a.group.and_then(|g| groups.iter().find(|x| x.id == g)).map(|g| g.name.clone()).unwrap_or_else(|| "—".into());
                egui::ComboBox::from_id_salt("an_grp").width(180.0).selected_text(cur).show_ui(ui, |ui| {
                    ui.selectable_value(&mut a.group, None, "—");
                    for g in &groups { ui.selectable_value(&mut a.group, Some(g.id), &g.name); }
                });
            });
            ui.horizontal(|ui| { row(ui, "Ebene:"); ui.label(RichText::new(&a.plane).size(14.0)); });
            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("Verschieben").min_size(vec2(100.0, 28.0))).clicked() { self.creo.annot3.sel = vec![id]; self.creo.annot3.tool = Some(ATool::Move); self.say(ATool::Move.prompt(), MsgKind::Prompt); }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("Schließen").min_size(vec2(100.0, 28.0))).clicked() { close = true; }
                });
            });
        });
        if a != self.data.annots[idx] { self.data.annots[idx] = a; self.dirty = true; }
        if close || !open { self.creo.annot3.edit = None; }
    }

    fn annot_query(&mut self, ctx: &egui::Context) {
        if !self.creo.annot3.query { return; }
        let mut open = true;
        // with geometry selected, only the annotations on it
        let on_sel: Vec<ARef> = self.selection.iter().filter_map(|p| self.aref_of(*p)).collect();
        let rows: Vec<(u64, Icon, String, String, String, String)> = self.data.annots.iter()
            .filter(|a| on_sel.is_empty() || a.refs.iter().any(|r| on_sel.iter().any(|s| same_ref(r, s))))
            .map(|a| {
                let refs = a.refs.iter().map(|r| match r { ARef::Face { .. } => "Fläche".to_string(), ARef::Edge { .. } => "Kante".into(), ARef::Plane(id) | ARef::Axis(id) | ARef::Point(id) => self.data.feature(*id).map(|f| f.name.clone()).unwrap_or_else(|| "?".into()) }).collect::<Vec<_>>().join(", ");
                let grp = a.group.and_then(|g| self.data.annot_groups.iter().find(|x| x.id == g)).map(|g| g.name.clone()).unwrap_or_else(|| "—".into());
                (a.id, kind_icon(&a.kind), kind_name(&a.kind).to_string(), self.annot_text(a), refs, grp)
            }).collect();
        let mut pick = None;
        let mut edit = None;
        let mut remove = false;
        egui::Window::new("Anmerkungsabfrage").id(egui::Id::new("annot_query")).open(&mut open).collapsible(false).default_width(560.0).default_pos(pos2(420.0, 160.0)).show(ctx, |ui| {
            ui.label(RichText::new(if on_sel.is_empty() { format!("{} Anmerkung(en) im Modell", rows.len()) } else { format!("{} Anmerkung(en) an der gewählten Geometrie", rows.len()) }).color(LABEL));
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                egui::Grid::new("aq_grid").num_columns(4).striped(true).spacing(vec2(14.0, 6.0)).show(ui, |ui| {
                    for h in ["Typ", "Text / Wert", "Referenzen", "Anmerkungs-KE"] { ui.label(RichText::new(h).strong()); }
                    ui.end_row();
                    for (id, ic, k, txt, refs, grp) in &rows {
                        let sel = self.creo.annot3.sel.contains(id);
                        let r = ui.horizontal(|ui| {
                            let (rr, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                            icons::paint(ui.painter(), rr, *ic, true);
                            ui.selectable_label(sel, k)
                        }).inner;
                        if r.clicked() { pick = Some(*id); }
                        if r.double_clicked() { edit = Some(*id); }
                        ui.label(txt);
                        ui.label(refs);
                        ui.label(grp);
                        ui.end_row();
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                let one = self.creo.annot3.sel.first().copied();
                if ui.add_enabled(one.is_some(), egui::Button::new("Bearbeiten")).clicked() { edit = one; }
                if ui.add_enabled(one.is_some(), egui::Button::new("Entfernen")).clicked() { remove = true; }
            });
        });
        if let Some(id) = pick { self.creo.annot3.sel = vec![id]; }
        if let Some(id) = edit { self.creo.annot3.sel = vec![id]; self.creo.annot3.edit = Some(id); }
        if remove { self.remove_annots(); }
        self.creo.annot3.query = open;
    }

    /// Anmerkungs-KEs and their annotations at the end of the model tree.
    pub fn annot_tree_rows(&mut self, ui: &mut egui::Ui) {
        let groups = self.data.annot_groups.clone();
        for g in &groups {
            let active = self.creo.annot3.active_group == Some(g.id);
            let (r, _) = Self::paint_row(ui, 0, if g.datum_target { Icon::DatumTarget } else { Icon::AnnotFeature }, &g.name, TEXT, active, false, None);
            let r = r.on_hover_text(if active { "Aktives Anmerkungs-KE — neue Anmerkungen kommen hierher" } else { "Klicken: aktiv setzen und seine Anmerkungen wählen" });
            if r.clicked() {
                self.creo.annot3.active_group = Some(g.id);
                self.creo.annot3.sel = self.data.annots.iter().filter(|a| a.group == Some(g.id)).map(|a| a.id).collect();
            }
            let members: Vec<(u64, Icon, String)> = self.data.annots.iter().filter(|a| a.group == Some(g.id)).map(|a| (a.id, kind_icon(&a.kind), format!("{} {}", kind_name(&a.kind), self.annot_text(a)))).collect();
            for (id, ic, name) in members {
                let sel = self.creo.annot3.sel.contains(&id);
                let (r, _) = Self::paint_row(ui, 1, ic, &name, TEXT, false, sel, None);
                if r.clicked() { self.creo.annot3.sel = vec![id]; }
                if r.double_clicked() { self.creo.annot3.edit = Some(id); }
            }
        }
    }
}

fn same_ref(a: &ARef, b: &ARef) -> bool {
    match (a, b) {
        (ARef::Face { p: p1, n: n1 }, ARef::Face { p: p2, n: n2 }) => (*p1 - *p2).length() < 1e-3 && n1.dot(*n2) > 0.999,
        (ARef::Edge { p: p1 }, ARef::Edge { p: p2 }) => (*p1 - *p2).length() < 1e-3,
        _ => a == b,
    }
}

fn label(p: &egui::Painter, at: Pos2, text: &str, col: Color32, bg: bool) -> Rect {
    let g = p.layout(text.to_string(), FontId::proportional(14.0), col, 400.0);
    let r = Rect::from_center_size(at, g.size()).expand2(vec2(4.0, 2.0));
    if bg { p.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 170)); }
    p.galley(r.min + vec2(4.0, 2.0), g, col);
    r
}

fn nearest_edge(r: Rect, s: Pos2) -> Pos2 { pos2(s.x.clamp(r.left(), r.right()), s.y.clamp(r.top(), r.bottom())) }

fn arrow(p: &egui::Painter, tip: Pos2, from: Pos2, col: Color32) {
    let d = (tip - from).normalized();
    let n = vec2(-d.y, d.x);
    p.add(egui::Shape::convex_polygon(vec![tip, tip - d * 9.0 + n * 3.2, tip - d * 9.0 - n * 3.2], col, Stroke::NONE));
}

fn leader(p: &egui::Painter, from: Pos2, to: Pos2, st: Stroke, head: bool) {
    p.line_segment([from, to], st);
    if head { arrow(p, from, to, st.color); }
}

/// Extension lines from the geometry and the dimension line through the text position.
fn dim_lines(p: &egui::Painter, a: Pos2, b: Pos2, at: Pos2, st: Stroke) {
    let ab = b - a;
    if ab.length() < 2.0 { return; }
    let d = ab.normalized();
    let n = vec2(-d.y, d.x);
    let off = n * (at - a).dot(n);
    let (a2, b2) = (a + off, b + off);
    let gap = if off.length() > 1.0 { off.normalized() * 5.0 } else { vec2(0.0, 0.0) };
    p.line_segment([a + gap * 0.4, a2 + gap], Stroke::new(0.8_f32, st.color));
    p.line_segment([b + gap * 0.4, b2 + gap], Stroke::new(0.8_f32, st.color));
    p.line_segment([a2, b2], st);
    arrow(p, a2, b2, st.color);
    arrow(p, b2, a2, st.color);
    // text outside the extension lines: carry the dimension line on to it
    let t = (at - a2).dot(d);
    if t < 0.0 || t > ab.length() { p.line_segment([if t < 0.0 { a2 } else { b2 }, a2 + d * t], st); }
}

fn finish_symbol(p: &egui::Painter, at: Pos2, kind: FinishKind, value: &str, col: Color32) -> Rect {
    let st = Stroke::new(1.2_f32, col);
    let (l, b, t) = (at + vec2(-8.0, -2.0), at + vec2(-3.0, 9.0), at + vec2(10.0, -18.0));
    p.line_segment([l, b], st);
    p.line_segment([b, t], st);
    let tw = p.layout_no_wrap(value.to_string(), FontId::proportional(13.0), col).size().x;
    p.line_segment([t, t + vec2(tw + 6.0, 0.0)], st);
    match kind {
        FinishKind::Removal => { p.line_segment([l, l + vec2(9.2, 0.0)], st); }
        FinishKind::NoRemoval => { p.circle_stroke(at + vec2(-2.6, 2.4), 3.6, st); }
        FinishKind::Any => {}
    }
    p.text(t + vec2(3.0, -2.0), Align2::LEFT_BOTTOM, value, FontId::proportional(13.0), col);
    Rect::from_min_max(pos2(l.x - 2.0, t.y - 18.0), pos2(t.x + tw + 8.0, b.y + 2.0))
}

/// Feature control frame: symbol | ⌀ value (M) | datums, lower-left corner beside `at`.
fn gtol_frame(p: &egui::Painter, at: Pos2, ch: GChar, diameter: bool, value: f64, mat: MatCond, datums: &[String], col: Color32) -> Rect {
    let h = 24.0;
    let font = FontId::proportional(14.0);
    let tol = format!("{}{}{}", if diameter { "⌀" } else { "" }, fmt_num(value, 3), mat.letter().map(|l| format!(" ({})", l)).unwrap_or_default());
    let mut cells: Vec<(f32, Option<String>)> = vec![(h, None)];
    cells.push((p.layout_no_wrap(tol.clone(), font.clone(), col).size().x + 14.0, Some(tol)));
    if !ch.form() { for d in datums.iter().filter(|d| !d.is_empty()) { cells.push((p.layout_no_wrap(d.clone(), font.clone(), col).size().x + 14.0, Some(d.clone()))); } }
    let w: f32 = cells.iter().map(|c| c.0).sum();
    let r = Rect::from_min_size(pos2(at.x, at.y - h / 2.0), vec2(w, h));
    let st = Stroke::new(1.1_f32, col);
    p.rect(r, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 200), st, egui::StrokeKind::Inside);
    let mut x = r.left();
    for (k, (cw, t)) in cells.iter().enumerate() {
        if k > 0 { p.line_segment([pos2(x, r.top()), pos2(x, r.bottom())], st); }
        let c = pos2(x + cw / 2.0, r.center().y);
        match t {
            Some(t) => { p.text(c, Align2::CENTER_CENTER, t, font.clone(), col); }
            None => {
                // the drawing's characteristic symbol, drawn in a 10-unit cell and scaled to the frame
                let mut prims = vec![];
                gsymbol(&mut prims, V2::ZERO, 10.0, ch, [col.r(), col.g(), col.b()]);
                let s = 2.0;
                let map = |v: V2| pos2(c.x + v.x as f32 * s, c.y - v.y as f32 * s);
                for pr in prims {
                    match pr {
                        Prim::Line { pts, .. } => { p.add(egui::Shape::line(pts.into_iter().map(map).collect(), Stroke::new(1.3_f32, col))); }
                        Prim::Circle { c: cc, r: rr, .. } => { p.circle_stroke(map(cc), rr as f32 * s, Stroke::new(1.3_f32, col)); }
                        _ => {}
                    }
                }
            }
        }
        x += cw;
    }
    r
}

