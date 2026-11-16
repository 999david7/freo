//! The commands of the Analyse tab: each one measures the regenerated model and shows the result
//! in the Informationsfenster; where it helps, the geometry found is selected in the model.

use super::*;
use crate::kernel::csg::{self, CPolygon};
use crate::kernel::topology::{EdgeChain, Topology};
use std::collections::HashMap;

/// Cut of a solid by the plane n·x = w: area, perimeter and centroid of the section.
pub fn section(polys: &[CPolygon], n: V3, w: f64) -> (f64, f64, V3) {
    let o = n * w;
    let (mut a2, mut per, mut c) = (0.0, 0.0, V3::ZERO);
    for p in polys {
        // the two points where the polygon's outline crosses the plane
        let mut pts: Vec<V3> = vec![];
        let k = p.v.len();
        for i in 0..k {
            let (a, b) = (p.v[i].p, p.v[(i + 1) % k].p);
            let (da, db) = (n.dot(a) - w, n.dot(b) - w);
            if (da > 0.0) != (db > 0.0) {
                pts.push(a + (b - a) * (da / (da - db)));
            }
        }
        if pts.len() != 2 {
            continue;
        }
        // run the segment so that the solid lies to its left, seen against n
        let (mut s, mut e) = (pts[0] - o, pts[1] - o);
        if (e - s).dot(n.cross(p.plane.n)) < 0.0 {
            std::mem::swap(&mut s, &mut e);
        }
        let cr = s.cross(e).dot(n);
        a2 += cr;
        c += (s + e) * cr;
        per += (e - s).length();
    }
    let area = a2.abs() / 2.0;
    (area, per, if a2.abs() > 1e-12 { o + c / (3.0 * a2) } else { o })
}

/// Line, circle, arc or free curve, with the radius of circles and arcs.
pub fn edge_kind(c: &EdgeChain) -> (&'static str, Option<f64>) {
    if c.is_straight() {
        return ("Linie", None);
    }
    let n = c.points.len();
    if n < 3 {
        return ("Linie", None);
    }
    // The kernel stores circles as polygons and may add points on their chords, so a point
    // counts as "on the circle" when it lies between the circle and its chords.
    let cen = if c.closed {
        // centre of the outline, weighted by segment length (extra chord points do not shift it)
        let (mut sum, mut len) = (V3::ZERO, 0.0);
        for i in 0..n {
            let (a, b) = (c.points[i], c.points[(i + 1) % n]);
            let l = (b - a).length();
            sum += (a + b) * 0.5 * l;
            len += l;
        }
        if len < 1e-12 {
            return ("Kurve", None);
        }
        sum / len
    } else {
        // circle through the ends and the point farthest from their chord
        let (p0, p2) = (c.points[0], c.points[n - 1]);
        let ch = safe_norm(p2 - p0, V3::X);
        let p1 = *c.points.iter().max_by(|a, b| ((**a - p0) - ch * (**a - p0).dot(ch)).length().partial_cmp(&((**b - p0) - ch * (**b - p0).dot(ch)).length()).unwrap()).unwrap();
        let (u, v) = (p1 - p0, p2 - p0);
        let nn = u.cross(v);
        if nn.length() < 1e-9 {
            return ("Kurve", None);
        }
        p0 + (v.length_squared() * nn.cross(u) + u.length_squared() * v.cross(nn)) / (2.0 * nn.length_squared())
    };
    let r = c.points.iter().map(|p| (*p - cen).length()).fold(0.0, f64::max);
    if r < 1e-9 {
        return ("Kurve", None);
    }
    // flat, and no point further inside than the chords of a coarse polygon reach
    let normal = {
        let mut m = V3::ZERO;
        for i in 0..n - 1 {
            m += (c.points[i] - cen).cross(c.points[i + 1] - cen);
        }
        safe_norm(m, V3::Z)
    };
    let on = c.points.iter().all(|p| {
        let d = *p - cen;
        d.dot(normal).abs() < 1e-3 * r.max(1.0) && d.length() > r * 0.97
    });
    if !on {
        return ("Kurve", None);
    }
    (if c.closed { "Kreis" } else { "Bogen" }, Some(r))
}

fn tri_area(p: &CPolygon) -> f64 {
    let mut n = V3::ZERO;
    for i in 1..p.v.len().saturating_sub(1) {
        n += (p.v[i].p - p.v[0].p).cross(p.v[i + 1].p - p.v[0].p);
    }
    n.length() * 0.5
}

fn centroid(p: &CPolygon) -> V3 {
    p.v.iter().fold(V3::ZERO, |s, q| s + q.p) / p.v.len().max(1) as f64
}

/// Smallest and largest wall thickness: from points on the surface straight into the material.
pub fn thickness(polys: &[CPolygon], topo: &Topology) -> Option<(f64, V3, f64, V3)> {
    let step = (polys.len() / 1500).max(1);
    let (mut lo, mut hi): (Option<(f64, V3)>, Option<(f64, V3)>) = (None, None);
    for p in polys.iter().step_by(step) {
        let c = centroid(p);
        // the ray starts just under the surface: add that step back
        let Some(t) = topo.ray_first_hit(c - p.plane.n * 1e-4, -p.plane.n, 1e-4).map(|t| t + 1e-4) else { continue };
        if lo.is_none_or(|l| t < l.0) {
            lo = Some((t, c));
        }
        if hi.is_none_or(|h| t > h.0) {
            hi = Some((t, c));
        }
    }
    Some((lo?.0, lo?.1, hi?.0, hi?.1))
}

/// The polygons of each body (the result keeps them in body order).
fn bodies(r: &RegenResult) -> Vec<(u64, Vec<CPolygon>)> {
    let mut at = 0;
    r.bodies
        .iter()
        .map(|(id, n)| {
            let v = r.polys[at.min(r.polys.len())..(at + n).min(r.polys.len())].to_vec();
            at += n;
            (*id, v)
        })
        .filter(|b| !b.1.is_empty())
        .collect()
}

fn min_distance(a: &[CPolygon], b: &[CPolygon]) -> f64 {
    let tb = Topology::new(b);
    let step = (a.len() / 300).max(1);
    let mut best = f64::INFINITY;
    for p in a.iter().step_by(step) {
        for q in &p.v {
            for pi in 0..tb.polys.len() {
                best = best.min(tb.dist_point_poly(q.p, pi));
            }
        }
    }
    best
}

/// Colours an analysis paints on the model, with the legend shown beside it.
pub struct Overlay {
    pub title: String,
    pub legend: Vec<(String, [f32; 4])>,
    pub tris: Arc<Vec<(Vec<f32>, [f32; 4])>>,
    /// the regeneration it belongs to: it disappears when the model changes
    pub generation: u64,
}

/// Blue – cyan – green – yellow – orange – red, for `k` of `n` steps.
pub fn ramp(k: usize, n: usize) -> [f32; 4] {
    const C: [[f32; 3]; 6] = [[0.16, 0.35, 0.85], [0.20, 0.70, 0.90], [0.25, 0.75, 0.35], [0.95, 0.85, 0.20], [0.96, 0.55, 0.15], [0.88, 0.20, 0.16]];
    let i = if n <= 1 { 0 } else { (k * 5 + (n - 1) / 2) / (n - 1) };
    let c = C[i.min(5)];
    [c[0], c[1], c[2], 0.92]
}

/// Curvature (1/mm) at every polygon: the turn of the surface normal towards the neighbouring
/// polygons of the same face, over the distance between them. Zero on planar faces.
pub fn curvature(t: &Topology) -> Vec<f64> {
    let mut edge: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (pi, p) in t.polys.iter().enumerate() {
        for k in 0..p.len() {
            let (u, v) = (p[k], p[(k + 1) % p.len()]);
            edge.entry((u.min(v), u.max(v))).or_default().push(pi);
        }
    }
    let cen: Vec<V3> = (0..t.polys.len()).map(|pi| t.poly_centroid(pi)).collect();
    let mut out = vec![0.0f64; t.polys.len()];
    for ps in edge.values() {
        for &a in ps {
            for &b in ps {
                if a >= b || t.poly_face[a] != t.poly_face[b] || t.faces[t.poly_face[a]].planar {
                    continue;
                }
                let ang = t.planes[a].n.dot(t.planes[b].n).clamp(-1.0, 1.0).acos();
                // the distance across the bend: along the surface, at right angles to the shared edge
                let d = cen[b] - cen[a];
                let across = safe_norm(t.planes[a].n.cross(t.planes[b].n), V3::ZERO);
                let l = (d - across * d.dot(across)).length();
                if ang < 1e-6 || l < 1e-9 {
                    continue;
                }
                let k = ang / l;
                out[a] = out[a].max(k);
                out[b] = out[b].max(k);
            }
        }
    }
    out
}

const AXES: [(&str, V3); 6] = [("+X", V3::X), ("-X", V3::NEG_X), ("+Y", V3::Y), ("-Y", V3::NEG_Y), ("+Z", V3::Z), ("-Z", V3::NEG_Z)];

impl App {
    /// Paints polygons of the displayed model: `bins[i]` is the legend entry of polygon `i` (None: not painted).
    fn paint(&mut self, title: &str, legend: Vec<(String, [f32; 4])>, polys: &[CPolygon], bins: &[Option<usize>]) {
        let mut tris: Vec<(Vec<f32>, [f32; 4])> = legend.iter().map(|l| (vec![], l.1)).collect();
        for (p, b) in polys.iter().zip(bins) {
            let Some(b) = b.filter(|b| *b < tris.len()) else { continue };
            let n = p.plane.n;
            for k in 1..p.v.len().saturating_sub(1) {
                for j in [0, k, k + 1] {
                    let v = p.v[j].p;
                    tris[b].0.extend_from_slice(&[v.x as f32, v.y as f32, v.z as f32, n.x as f32, n.y as f32, n.z as f32]);
                }
            }
        }
        let generation = self.displayed().generation;
        self.creo.tools.overlay = Some(Overlay { title: title.to_string(), legend, tris: Arc::new(tris), generation });
    }

    /// The analysis colours for the renderer (none once the model has been regenerated).
    pub fn analysis_overlay(&self) -> Arc<Vec<(Vec<f32>, [f32; 4])>> {
        match &self.creo.tools.overlay {
            Some(o) if o.generation == self.displayed().generation && self.sketch.is_none() => o.tris.clone(),
            _ => Arc::new(vec![]),
        }
    }

    /// Legend of the analysis colours, bottom left of the graphics window.
    pub fn draw_analysis_legend(&self, painter: &egui::Painter, rect: egui::Rect) {
        let Some(o) = self.creo.tools.overlay.as_ref().filter(|o| o.generation == self.displayed().generation && self.sketch.is_none()) else { return };
        use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Stroke, StrokeKind};
        let font = FontId::proportional(12.5);
        let w = o
            .legend
            .iter()
            .map(|l| painter.layout_no_wrap(l.0.clone(), font.clone(), Color32::BLACK).size().x)
            .fold(painter.layout_no_wrap(o.title.clone(), font.clone(), Color32::BLACK).size().x, f32::max)
            + 44.0;
        let h = 26.0 + o.legend.len() as f32 * 20.0 + 6.0;
        let r = Rect::from_min_size(pos2(rect.left() + 14.0, rect.bottom() - h - 14.0), vec2(w, h));
        painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(250, 251, 252, 235));
        painter.rect_stroke(r, 3.0, Stroke::new(1.0_f32, Color32::from_rgb(170, 176, 184)), StrokeKind::Inside);
        painter.text(r.left_top() + vec2(10.0, 14.0), Align2::LEFT_CENTER, &o.title, font.clone(), Color32::from_rgb(40, 44, 50));
        for (i, (label, c)) in o.legend.iter().enumerate() {
            let y = r.top() + 26.0 + i as f32 * 20.0 + 9.0;
            let sw = Rect::from_min_size(pos2(r.left() + 10.0, y - 7.0), vec2(18.0, 14.0));
            painter.rect_filled(sw, 0.0, Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
            painter.rect_stroke(sw, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(90, 96, 104)), StrokeKind::Inside);
            painter.text(pos2(r.left() + 36.0, y), Align2::LEFT_CENTER, label, font.clone(), Color32::from_rgb(40, 44, 50));
        }
    }

    fn report(&mut self, file: &str, title: &str, body: String) {
        let text = format!("{} FÜR TEIL {}\n================================================\n\n{}", title, self.data.name.to_uppercase(), body);
        self.creo.tools.analyses.push((title.to_string(), text.clone()));
        if self.creo.tools.analyses.len() > 40 {
            self.creo.tools.analyses.remove(0);
        }
        self.creo.tools.info = Some((format!("Informationsfenster ({})", file), text));
    }

    /// The selected plane or planar face as (name, normal, offset).
    fn picked_plane_nw(&self) -> Option<(String, V3, f64)> {
        let pr = self.plane_ref(&self.selection)?;
        let f = self.displayed().frame_for(&pr)?;
        Some((self.plane_name(&Some(pr)), f.z(), f.z().dot(f.origin)))
    }

    fn numeric_params(&self) -> Vec<(usize, f64)> {
        self.data.parameters.iter().enumerate().filter_map(|(i, p)| p.expr.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.abs() > 1e-12).map(|v| (i, v))).collect()
    }

    /// Mass, volume and surface of the model with parameters replaced (None when it fails to regenerate).
    fn variant(&self, set: &[(usize, f64)]) -> Option<(f64, f64, f64)> {
        let mut d = self.data.clone();
        for (i, v) in set {
            d.parameters[*i].expr = format!("{}", v);
        }
        let r = regen::run(&d);
        if !r.errors.is_empty() || r.polys.is_empty() {
            return None;
        }
        let m = export::mass_properties(&r, d.density);
        Some((m.mass, m.volume, m.area))
    }

    /// Runs the Analyse command `name`.
    pub fn run_analysis(&mut self, name: &str) {
        if self.in_asm() {
            self.say("Diese Analyse arbeitet auf einem Teil: eine Komponente öffnen.", MsgKind::Warning);
            return;
        }
        let r = self.displayed().clone();
        let t = r.topo.clone();
        let solid = !r.polys.is_empty();
        let needs_solid = !matches!(
            name,
            "Analyse"
                | "Gespeicherte Analyse"
                | "Leistungsüberwachung"
                | "Alle ausblenden"
                | "Analyse löschen"
                | "Alle temporären löschen"
                | "Simulate Analyse"
                | "Toolkit-basiert"
                | "Externe Analyse"
        );
        if needs_solid && !solid {
            self.say("Das Modell enthält keine Volumengeometrie, die analysiert werden kann.", MsgKind::Warning);
            return;
        }
        let f = |v: f64| fmt_num(v, 3);
        let sel_edges: Vec<usize> = self.selection.iter().filter_map(|p| if let Pick::Edge(e) = p { Some(*e) } else { None }).filter(|e| *e < t.chains.len()).collect();
        let sel_faces: Vec<usize> = self.selection.iter().filter_map(|p| if let Pick::Face(e) = p { Some(*e) } else { None }).filter(|e| *e < t.faces.len()).collect();
        let dir = self.picked_plane_nw().map(|p| (p.0, p.1)).unwrap_or(("OBEN (+Y)".into(), V3::Y));
        match name {
            // ---------------- Verwalten ----------------
            "Analyse" => match self.creo.tools.analyses.last().cloned() {
                Some((n, _)) => {
                    self.creo.tools.saved_analyses.push(self.creo.tools.analyses.last().cloned().unwrap());
                    self.say(format!("Analyse „{}“ wurde gespeichert.", n), MsgKind::Success);
                }
                None => self.say("Zuerst eine Analyse ausführen; sie wird dann als Analyse-KE gespeichert.", MsgKind::Prompt),
            },
            "Gespeicherte Analyse" => {
                let s = &self.creo.tools.saved_analyses;
                let body = if s.is_empty() {
                    "Es sind keine Analysen gespeichert. „Analyse“ speichert die zuletzt ausgeführte Analyse.\n".to_string()
                } else {
                    s.iter().enumerate().map(|(i, a)| format!("---- {} · {} ----\n{}\n", i + 1, a.0, a.1)).collect()
                };
                self.creo.tools.info =
                    Some(("Informationsfenster (gespeicherte Analysen)".into(), format!("GESPEICHERTE ANALYSEN\n================================================\n\n{}", body)));
            }
            "Leistungsüberwachung" => {
                let mut body = format!("{:<5}{:<34}{:>12}{:>12}\n", "Nr.", "KE", "Zeit (ms)", "Polygone");
                let mut last = (0.0, 0usize);
                for i in 1..=self.data.features.len() {
                    let rr = regen::run_features(&self.data, &self.data.features[..i], None);
                    let ft = &self.data.features[i - 1];
                    if !ft.suppressed {
                        body += &format!("{:<5}{:<34}{:>12.1}{:>12}\n", i, ft.name.to_uppercase(), ((rr.time - last.0) * 1000.0).max(0.0), rr.polys.len() as i64 - last.1 as i64);
                    }
                    last = (rr.time, rr.polys.len());
                }
                body += &format!("\nGesamte Regenerierung: {:.1} ms, {} Polygone\n", last.0 * 1000.0, last.1);
                self.report("leistung.inf", "LEISTUNGSÜBERWACHUNG", body);
            }
            "Alle ausblenden" => {
                self.selection.clear();
                self.creo.tools.overlay = None;
                self.creo.tools.info = None;
                self.measure = false;
                self.say("Alle Analysen wurden ausgeblendet.", MsgKind::Info);
            }
            "Analyse löschen" | "Alle temporären löschen" => {
                let n = self.creo.tools.analyses.len();
                self.creo.tools.analyses.clear();
                if name == "Analyse löschen" {
                    self.creo.tools.saved_analyses.pop();
                }
                self.say(format!("{} temporäre Analyse(n) gelöscht.", n), MsgKind::Info);
            }
            // ---------------- Benutzerdefiniert ----------------
            "Benutzerdefinierte Analyse" => {
                let m = export::mass_properties(&r, self.data.density);
                let short = t.chains.iter().map(|c| c.length()).fold(f64::INFINITY, f64::min);
                let rad = (0..t.faces.len()).filter_map(|i| t.surface_axis(i)).map(|a| a.2).fold(f64::INFINITY, f64::min);
                let th = thickness(&r.polys, &t);
                let s = m.bbox.size();
                let body = format!("Volumen:              {} mm³\nOberfläche:           {} mm²\nMasse:                {} kg\nBegrenzungsrahmen:    {} × {} × {} mm\nKürzeste Kante:       {} mm\nKleinster Radius:     {}\nKleinste Wanddicke:   {}\nFlächen / Kanten:     {} / {}\n",
                    f(m.volume), f(m.area), fmt_num(m.mass, 5), f(s.x), f(s.y), f(s.z), f(short), if rad.is_finite() { format!("{} mm", f(rad)) } else { "keine gekrümmten Flächen".into() }, th.map(|x| format!("{} mm", f(x.0))).unwrap_or("–".into()), t.faces.len(), t.chains.len());
                self.report("benutzer.inf", "BENUTZERDEFINIERTE ANALYSE", body);
            }
            "Excel-Analyse" | "Externe Analyse" => {
                let m = export::mass_properties(&r, self.data.density);
                let mut csv = format!(
                    "Größe;Wert;Einheit\nVolumen;{};mm3\nOberfläche;{};mm2\nMasse;{};kg\nSchwerpunkt X;{};mm\nSchwerpunkt Y;{};mm\nSchwerpunkt Z;{};mm\n",
                    m.volume, m.area, m.mass, m.cog.x, m.cog.y, m.cog.z
                );
                for p in &self.data.parameters {
                    csv += &format!("Parameter {};{};\n", p.name.to_uppercase(), r.vars.get(&p.name.to_uppercase()).map(|v| v.to_string()).unwrap_or(p.expr.clone()));
                }
                let file = format!("{}_analyse.{}", self.data.name.to_lowercase(), if name == "Excel-Analyse" { "csv" } else { "txt" });
                self.save_text(name, &file, &if name == "Excel-Analyse" { csv } else { csv.replace(';', "\t") });
            }
            "Toolkit-basiert" => self.creo.tools.aux = true,
            // ---------------- Modellbericht ----------------
            "Kurze Kante" => {
                let mut v: Vec<(usize, f64)> = t.chains.iter().enumerate().map(|(i, c)| (i, c.length())).collect();
                v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                let Some(&(i, l)) = v.first() else { return };
                let mut body = format!("Kürzeste Kante: {} mm ({})\nDie Kante ist im Modell ausgewählt.\n\nDie zehn kürzesten Kanten:\n", f(l), edge_kind(&t.chains[i]).0);
                for (k, (j, l)) in v.iter().take(10).enumerate() {
                    body += &format!("  {:>2}. {:>12} mm  {}\n", k + 1, f(*l), edge_kind(&t.chains[*j]).0);
                }
                self.selection = vec![Pick::Edge(i)];
                self.report("kurze_kante.inf", "KURZE KANTE", body);
            }
            "Dicke" | "Schnittdicke" => {
                let polys: Vec<CPolygon> = if name == "Schnittdicke" {
                    let (n, w) = self.picked_plane_nw().map(|p| (p.1, p.2)).unwrap_or((V3::X, t.bbox.center().x));
                    r.polys
                        .iter()
                        .filter(|p| {
                            let d: Vec<f64> = p.v.iter().map(|q| n.dot(q.p) - w).collect();
                            d.iter().any(|x| *x > 0.0) && d.iter().any(|x| *x <= 0.0)
                        })
                        .cloned()
                        .collect()
                } else {
                    r.polys.clone()
                };
                let Some((lo, plo, hi, phi)) = thickness(&polys, &t) else {
                    self.say("Die Dicke konnte nicht bestimmt werden.", MsgKind::Warning);
                    return;
                };
                // wall thickness at every polygon, in five classes from thin (red) to thick (blue)
                let th: Vec<Option<f64>> = r.polys.iter().map(|p| t.ray_first_hit(centroid(p) - p.plane.n * 1e-4, -p.plane.n, 1e-4).map(|x| x + 1e-4)).collect();
                let cls = |x: f64| (((x - lo) / (hi - lo).max(1e-9)) * 5.0).floor().clamp(0.0, 4.0) as usize;
                let legend = (0..5).map(|k| (format!("{} – {} mm", f(lo + (hi - lo) * k as f64 / 5.0), f(lo + (hi - lo) * (k + 1) as f64 / 5.0)), ramp(4 - k, 5))).collect();
                self.paint("Dicke", legend, &r.polys, &th.iter().map(|x| x.map(cls)).collect::<Vec<_>>());
                let at = |p: V3| format!("({}, {}, {})", f(p.x), f(p.y), f(p.z));
                let head = if name == "Schnittdicke" {
                    format!("Schnittebene: {}\n", self.picked_plane_nw().map(|p| p.0).unwrap_or("Mitte des Modells, senkrecht zu X".into()))
                } else {
                    String::new()
                };
                self.report("dicke.inf", &name.to_uppercase(), format!("{}Minimale Dicke: {} mm bei {}\nMaximale Dicke: {} mm bei {}\n\nGemessen wird von der Oberfläche senkrecht ins Material bis zur gegenüberliegenden Wand.\n", head, f(lo), at(plo), f(hi), at(phi)));
            }
            "Querschnitt-Masseneigenschaften" | "Schnitte" => {
                let (pn, n, w) = self.picked_plane_nw().unwrap_or(("Mitte des Modells, senkrecht zu X".into(), V3::X, t.bbox.center().x));
                let (a, per, c) = section(&r.polys, n, w);
                if a < 1e-9 {
                    self.say("Die gewählte Ebene schneidet das Modell nicht. Eine Ebene oder ebene Fläche wählen, die durch das Modell geht.", MsgKind::Warning);
                    return;
                }
                self.report("querschnitt.inf", "QUERSCHNITT-MASSENEIGENSCHAFTEN", format!("Schnittebene:    {}\nFlächeninhalt:   {} mm²\nUmfang:          {} mm\nFlächenschwerpunkt: ({}, {}, {}) mm\nMasse je mm Länge: {} kg/mm\n\nOhne gewählte Ebene wird durch die Mitte des Modells geschnitten.\n", pn, f(a), f(per), f(c.x), f(c.y), f(c.z), fmt_num(a * self.data.density, 6)));
            }
            // ---------------- Geometrie prüfen ----------------
            "Geometriebericht" | "Kantentyp" => {
                let list: Vec<usize> = if sel_edges.is_empty() { (0..t.chains.len()).collect() } else { sel_edges.clone() };
                let mut count: std::collections::BTreeMap<&str, (usize, f64)> = Default::default();
                let mut body = String::new();
                for &i in &list {
                    let (k, rad) = edge_kind(&t.chains[i]);
                    let e = count.entry(k).or_default();
                    e.0 += 1;
                    e.1 += t.chains[i].length();
                    if list.len() <= 40 {
                        body += &format!(
                            "  Kante {:>3}: {:<6} Länge {:>10} mm{}{}\n",
                            i + 1,
                            k,
                            f(t.chains[i].length()),
                            rad.map(|x| format!("  Radius {} mm", f(x))).unwrap_or_default(),
                            if t.chains[i].convex { "  konvex" } else { "  konkav" }
                        );
                    }
                }
                let mut head = format!("{} Kanten{}\n\n", list.len(), if sel_edges.is_empty() { " (ganzes Modell)" } else { " (Auswahl)" });
                for (k, (n, l)) in &count {
                    head += &format!("{:<8}{:>5} Stück   Gesamtlänge {} mm\n", k, n, f(*l));
                }
                let planar = t.faces.iter().filter(|x| x.planar).count();
                head += &format!("\nFlächen: {} eben, {} gekrümmt\n\n", planar, t.faces.len() - planar);
                self.report("geometrie.inf", "GEOMETRIEBERICHT", head + &body);
            }
            "Punkt" => {
                let mut body = String::new();
                for p in &self.selection {
                    match *p {
                        Pick::Face(i) if i < t.faces.len() => {
                            let x = &t.faces[i];
                            body += &format!(
                                "Fläche {}: Mittelpunkt ({}, {}, {})  Normale ({}, {}, {})\n",
                                i + 1,
                                f(x.center.x),
                                f(x.center.y),
                                f(x.center.z),
                                f(x.normal.x),
                                f(x.normal.y),
                                f(x.normal.z)
                            );
                        }
                        Pick::Edge(i) if i < t.chains.len() => {
                            let c = &t.chains[i];
                            let (a, m, b) = (c.points[0], c.mid(), *c.points.last().unwrap());
                            body += &format!(
                                "Kante {}: Anfang ({}, {}, {})  Mitte ({}, {}, {})  Ende ({}, {}, {})\n",
                                i + 1,
                                f(a.x),
                                f(a.y),
                                f(a.z),
                                f(m.x),
                                f(m.y),
                                f(m.z),
                                f(b.x),
                                f(b.y),
                                f(b.z)
                            );
                        }
                        Pick::Point(id) => {
                            if let Some(q) = r.points.iter().find(|q| q.id == id) {
                                body += &format!("Punkt {}: ({}, {}, {})\n", q.name, f(q.p.x), f(q.p.y), f(q.p.z));
                            }
                        }
                        _ => {}
                    }
                }
                if body.is_empty() {
                    self.say("Punkt: zuerst eine Fläche, eine Kante oder einen Bezugspunkt im Modell auswählen.", MsgKind::Prompt);
                    return;
                }
                self.report("punkt.inf", "PUNKTANALYSE", body);
            }
            "Schräge" | "Steigung" | "Abweichwinkel" => {
                let d = dir.1;
                // area by angle between the surface and the pull direction
                let limits = [0.5, 1.0, 3.0, 10.0, 45.0, 90.1];
                let mut bins = [0.0f64; 6];
                let mut undercut = 0.0;
                let mut steep: Vec<usize> = vec![];
                for (fi, face) in t.faces.iter().enumerate() {
                    for &pi in &face.polys {
                        let n = t.planes[pi].n;
                        let ang = n.dot(d).abs().clamp(0.0, 1.0).asin().to_degrees();
                        let a = t.poly_area(pi);
                        bins[limits.iter().position(|l| ang < *l).unwrap_or(5)] += a;
                        if n.dot(d) < -0.02 && ang < 89.0 {
                            undercut += a;
                        }
                    }
                    if face.planar && face.normal.dot(d).abs().asin().to_degrees() < 0.5 {
                        steep.push(fi);
                    }
                }
                // the same classes as colours on the model
                let labels = ["0° – 0,5° (ohne Schräge)", "0,5° – 1°", "1° – 3°", "3° – 10°", "10° – 45°", "45° – 90°"];
                let pb: Vec<Option<usize>> = r.polys.iter().map(|p| limits.iter().position(|l| p.plane.n.dot(d).abs().clamp(0.0, 1.0).asin().to_degrees() < *l)).collect();
                self.paint(&format!("{} zu {}", name, dir.0), labels.iter().enumerate().map(|(i, l)| (l.to_string(), ramp(5 - i, 6))).collect(), &r.polys, &pb);
                let total: f64 = bins.iter().sum();
                let mut body = format!("Zugrichtung / Referenz: {}\n\nWinkel zwischen Fläche und Richtung     Flächeninhalt      Anteil\n", dir.0);
                for (i, l) in ["0° – 0,5° (senkrecht, ohne Schräge)", "0,5° – 1°", "1° – 3°", "3° – 10°", "10° – 45°", "45° – 90°"].iter().enumerate() {
                    body += &format!("  {:<38}{:>12} mm² {:>8.1} %\n", l, f(bins[i]), 100.0 * bins[i] / total.max(1e-12));
                }
                body += &format!("\nFlächen, die von der Richtung abgewandt sind (Hinterschnitt beim Entformen): {} mm²\n", f(undercut));
                if name == "Schräge" {
                    body += &format!("{} ebene Fläche(n) ohne Schräge sind im Modell ausgewählt.\n", steep.len());
                    self.selection = steep.into_iter().map(Pick::Face).collect();
                }
                body += "\nOhne gewählte Ebene gilt OBEN (+Y) als Richtung.\n";
                self.report("schraege.inf", &format!("{}-ANALYSE", name.to_uppercase()), body);
            }
            "Konstruktionsrichtung" => {
                let mut rows: Vec<(usize, f64, f64, f64)> = vec![];
                for (i, (_, d)) in AXES.iter().enumerate() {
                    let lo = r.polys.iter().flat_map(|p| p.v.iter()).map(|q| q.p.dot(*d)).fold(f64::INFINITY, f64::min);
                    let hi = r.polys.iter().flat_map(|p| p.v.iter()).map(|q| q.p.dot(*d)).fold(f64::NEG_INFINITY, f64::max);
                    // surfaces facing down more than 45° need support, unless they rest on the plate
                    let (mut over, mut base) = (0.0, 0.0);
                    for p in &r.polys {
                        let down = p.plane.n.dot(*d);
                        if down > -0.7071 {
                            continue;
                        }
                        if p.v.iter().all(|q| q.p.dot(*d) - lo < 1e-3) {
                            base += tri_area(p);
                        } else {
                            over += tri_area(p);
                        }
                    }
                    rows.push((i, over, base, hi - lo));
                }
                let best = rows.iter().min_by(|a, b| (a.1, -a.2).partial_cmp(&(b.1, -b.2)).unwrap()).unwrap().0;
                let mut body = format!(
                    "Empfohlene Konstruktionsrichtung für den 3D-Druck: {} nach oben\n\n{:<10}{:>20}{:>20}{:>14}\n",
                    AXES[best].0, "Richtung", "Stützfläche (mm²)", "Auflage (mm²)", "Höhe (mm)"
                );
                for (i, over, base, h) in &rows {
                    body += &format!("{:<10}{:>20}{:>20}{:>14}{}\n", AXES[*i].0, f(*over), f(*base), f(*h), if *i == best { "   <- am besten" } else { "" });
                }
                body += "\nStützfläche: Flächen, die mehr als 45° nach unten zeigen und nicht auf der Bauplatte liegen.\n";
                let bd = AXES[best].1;
                let lo = r.polys.iter().flat_map(|p| p.v.iter()).map(|q| q.p.dot(bd)).fold(f64::INFINITY, f64::min);
                let pb: Vec<Option<usize>> = r
                    .polys
                    .iter()
                    .map(|p| {
                        if p.plane.n.dot(bd) > -0.7071 {
                            None
                        } else if p.v.iter().all(|q| q.p.dot(bd) - lo < 1e-3) {
                            Some(0)
                        } else {
                            Some(1)
                        }
                    })
                    .collect();
                self.paint(
                    &format!("Konstruktionsrichtung {}", AXES[best].0),
                    vec![("Auflage auf der Bauplatte".into(), ramp(2, 6)), ("Braucht Stützen".into(), ramp(5, 6))],
                    &r.polys,
                    &pb,
                );
                self.report("richtung.inf", "KONSTRUKTIONSRICHTUNG", body);
            }
            "Globaler Abstand" | "Globale Durchdringung" | "Volumendurchdringung" => {
                let bs = bodies(&r);
                if bs.len() < 2 {
                    self.report(
                        "abstand.inf",
                        &name.to_uppercase(),
                        "Das Teil enthält nur einen Körper. Abstand und Durchdringung werden zwischen den Körpern eines Teils berechnet (Modell ▸ Körper ▸ Neuer Körper).\n".into(),
                    );
                    return;
                }
                let mut body = String::new();
                for i in 0..bs.len() {
                    for j in i + 1..bs.len() {
                        let vol = csg::signed_volume(&csg::intersect(&bs[i].1, &bs[j].1)).abs();
                        let label = format!("Körper {} – Körper {}", i + 1, j + 1);
                        if name == "Globaler Abstand" {
                            body += &format!(
                                "{}: {}\n",
                                label,
                                if vol > 1e-6 {
                                    "durchdringen sich (Abstand 0)".to_string()
                                } else {
                                    format!("kleinster Abstand {} mm", f(min_distance(&bs[i].1, &bs[j].1).min(min_distance(&bs[j].1, &bs[i].1))))
                                }
                            );
                        } else {
                            body += &format!("{}: {}\n", label, if vol > 1e-6 { format!("Durchdringung, Volumen {} mm³", f(vol)) } else { "keine Durchdringung".into() });
                        }
                    }
                }
                self.report("abstand.inf", &name.to_uppercase(), body);
            }
            "Netzfläche" | "Knoten" => {
                let list: Vec<usize> = if sel_faces.is_empty() { (0..t.faces.len()).collect() } else { sel_faces.clone() };
                let mut body = format!(
                    "{} Fläche(n){}\n\n{:<10}{:<12}{:>10}{:>10}{:>16}\n",
                    list.len(),
                    if sel_faces.is_empty() { " (ganzes Modell)" } else { " (Auswahl)" },
                    "Fläche",
                    "Typ",
                    "Facetten",
                    "Knoten",
                    "Inhalt (mm²)"
                );
                let (mut tp, mut tk) = (0, 0);
                for &i in &list {
                    let x = &t.faces[i];
                    let knots: HashSet<usize> = x.polys.iter().flat_map(|pi| t.polys[*pi].iter().copied()).collect();
                    tp += x.polys.len();
                    tk += knots.len();
                    if list.len() <= 60 {
                        body += &format!("{:<10}{:<12}{:>10}{:>10}{:>16}\n", i + 1, if x.planar { "eben" } else { "gekrümmt" }, x.polys.len(), knots.len(), f(x.area));
                    }
                }
                body += &format!("\nGesamt: {} Facetten, {} Knoten\n", tp, tk);
                self.report("netz.inf", &name.to_uppercase(), body);
            }
            "Krümmung" | "Schattierte Krümmung" | "Radius" | "Versatz-Analyse" => {
                // curvature of every curved face (cylinders exactly, other surfaces from the facets)
                // and every circular edge, for the selection or the whole model
                let all = sel_faces.is_empty() && sel_edges.is_empty();
                let faces: Vec<usize> = if all { (0..t.faces.len()).collect() } else { sel_faces.clone() };
                let edges: Vec<usize> = if all { (0..t.chains.len()).collect() } else { sel_edges.clone() };
                let kpoly = curvature(&t);
                let mut rows: Vec<(f64, String, Pick)> = vec![];
                for &i in &faces {
                    if t.faces[i].planar {
                        continue;
                    }
                    let est = t.faces[i].polys.iter().map(|pi| kpoly[*pi]).fold(0.0, f64::max);
                    let rad = t.surface_axis(i).map(|a| a.2).or(if est > 1e-9 { Some(1.0 / est) } else { None });
                    if let Some(rad) = rad {
                        rows.push((rad, format!("Fläche {:>3}  kleinster Radius {:>10} mm  Krümmung {:>10} 1/mm", i + 1, f(rad), fmt_num(1.0 / rad, 5)), Pick::Face(i)));
                    }
                }
                for &i in &edges {
                    if let (k, Some(rad)) = edge_kind(&t.chains[i]) {
                        rows.push((
                            rad,
                            format!(
                                "Kante  {:>3}  Radius           {:>10} mm  Krümmung {:>10} 1/mm  ({}, {})",
                                i + 1,
                                f(rad),
                                fmt_num(1.0 / rad, 5),
                                k,
                                if t.chains[i].convex { "konvex" } else { "konkav" }
                            ),
                            Pick::Edge(i),
                        ));
                    }
                }
                if rows.is_empty() {
                    self.creo.tools.overlay = None;
                    self.report(
                        "kruemmung.inf",
                        &name.to_uppercase(),
                        "Die Auswahl (oder das Modell) enthält keine gekrümmten Flächen oder Kanten: die Krümmung ist überall 0.\n".into(),
                    );
                    return;
                }
                rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                // colours: curvature of each polygon of the displayed model, six classes up to the largest
                let kmax = 1.0 / rows[0].0;
                let tp = Topology::new(&r.polys);
                let mut kp = if tp.polys.len() == r.polys.len() { curvature(&tp) } else { vec![0.0; r.polys.len()] };
                // a cylinder has one curvature all over: use its exact radius instead of the facet estimate
                if tp.polys.len() == r.polys.len() {
                    for fi in 0..tp.faces.len() {
                        if let Some((_, _, rad)) = tp.surface_axis(fi) {
                            for &pi in &tp.faces[fi].polys {
                                kp[pi] = 1.0 / rad;
                            }
                        }
                    }
                }
                let legend: Vec<(String, [f32; 4])> =
                    std::iter::once(("eben (0)".to_string(), ramp(0, 6))).chain((1..6).map(|k| (format!("bis {} 1/mm", fmt_num(kmax * k as f64 / 5.0, 4)), ramp(k, 6)))).collect();
                self.paint("Krümmung", legend, &r.polys, &kp.iter().map(|k| Some(if *k < 1e-9 { 0 } else { ((k / kmax * 5.0).ceil() as usize).clamp(1, 5) })).collect::<Vec<_>>());
                let mut body = format!("Kleinster Radius: {} mm (größte Krümmung {} 1/mm) – im Modell ausgewählt\nGrößter Radius:   {} mm\nDie Krümmung ist im Modell farbig dargestellt (Legende unten links).\n", f(rows[0].0), fmt_num(1.0 / rows[0].0, 5), f(rows.last().unwrap().0));
                if name == "Versatz-Analyse" {
                    let concave = edges.iter().filter(|i| !t.chains[**i].convex).filter_map(|i| edge_kind(&t.chains[*i]).1).fold(f64::INFINITY, f64::min);
                    body += &format!(
                        "\nGrößter Versatz, bei dem keine Fläche entartet: {} mm (kleinster Radius{})\n",
                        f(rows[0].0),
                        if concave.is_finite() { format!("; kleinster konkaver Kantenradius {} mm", f(concave)) } else { String::new() }
                    );
                }
                body += "\n";
                for row in rows.iter().take(60) {
                    body += &format!("  {}\n", row.1);
                }
                self.selection = vec![rows[0].2];
                self.report("kruemmung.inf", &name.to_uppercase(), body);
            }
            "Abweichung" => {
                let Some((pn, n, w)) = self.picked_plane_nw() else {
                    self.say("Abweichung: eine Bezugsebene oder ebene Fläche und dazu Kanten oder Flächen auswählen (⇧-Klick).", MsgKind::Prompt);
                    return;
                };
                let mut pts: Vec<V3> = sel_edges.iter().flat_map(|i| t.chains[*i].points.clone()).collect();
                let plane_face = self.selection.iter().find_map(|p| if let Pick::Face(i) = p { Some(*i) } else { None }).filter(|i| t.faces.get(*i).is_some_and(|x| x.planar));
                for &i in sel_faces.iter().filter(|i| Some(**i) != plane_face || matches!(self.selection.first(), Some(Pick::Plane(_)))) {
                    pts.extend(t.faces[i].polys.iter().flat_map(|pi| t.polys[*pi].iter().map(|k| t.positions[*k])));
                }
                if pts.is_empty() {
                    pts = t.positions.clone();
                }
                let d: Vec<f64> = pts.iter().map(|p| n.dot(*p) - w).collect();
                let (lo, hi) = (d.iter().cloned().fold(f64::INFINITY, f64::min), d.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                self.report("abweichung.inf", "ABWEICHUNG", format!("Referenz: {}\nGeprüfte Punkte: {}\n\nKleinster Abstand zur Referenz: {} mm\nGrößter Abstand zur Referenz:   {} mm\nSpanne (Abweichung):            {} mm\n", pn, pts.len(), f(lo), f(hi), f(hi - lo)));
            }
            "Schatten" | "Reflexion" => {
                let d = if name == "Reflexion" { self.camera.back() } else { dir.1 };
                let step = (r.polys.len() / 3000).max(1);
                let (mut lit, mut shadow, mut away, mut mirror) = (0.0, 0.0, 0.0, 0.0);
                let mut pb: Vec<Option<usize>> = vec![None; r.polys.len()];
                for (pi, p) in r.polys.iter().enumerate().step_by(step) {
                    let a = tri_area(p) * step as f64;
                    let k = p.plane.n.dot(d);
                    let class = if k <= 1e-6 {
                        away += a;
                        2
                    } else if t.ray_first_hit(centroid(p) + p.plane.n * 1e-4, d, 1e-4).is_some() {
                        shadow += a;
                        1
                    } else {
                        lit += a;
                        if k > 0.985 {
                            mirror += a;
                            3
                        } else {
                            0
                        }
                    };
                    for q in pi..(pi + step).min(r.polys.len()) {
                        pb[q] = Some(class);
                    }
                }
                if name == "Schatten" {
                    self.paint(
                        &format!("Schatten, Licht aus {}", dir.0),
                        vec![("Beleuchtet".into(), [0.98, 0.92, 0.55, 0.9]), ("Schlagschatten".into(), [0.25, 0.28, 0.36, 0.9]), ("Abgewandt".into(), [0.50, 0.54, 0.62, 0.9])],
                        &r.polys,
                        &pb.iter().map(|b| b.map(|x| if x == 3 { 0 } else { x })).collect::<Vec<_>>(),
                    );
                } else {
                    self.paint(
                        "Reflexion",
                        vec![
                            ("Sichtbar".into(), [0.60, 0.72, 0.86, 0.9]),
                            ("Verdeckt".into(), [0.25, 0.28, 0.36, 0.9]),
                            ("Abgewandt".into(), [0.50, 0.54, 0.62, 0.9]),
                            ("Spiegelt zum Betrachter".into(), [1.0, 1.0, 1.0, 0.95]),
                        ],
                        &r.polys,
                        &pb,
                    );
                }
                let total = (lit + shadow + away).max(1e-12);
                let body = if name == "Schatten" {
                    format!("Lichtrichtung: aus {}\n\nBeleuchtet:                    {:>12} mm² {:>6.1} %\nIm Schlagschatten des Modells: {:>12} mm² {:>6.1} %\nVom Licht abgewandt:           {:>12} mm² {:>6.1} %\n\nOhne gewählte Ebene kommt das Licht von oben (+Y).\n", dir.0, f(lit), 100.0 * lit / total, f(shadow), 100.0 * shadow / total, f(away), 100.0 * away / total)
                } else {
                    format!("Blickrichtung: aktuelle Ansicht\n\nSichtbar:                               {:>12} mm² {:>6.1} %\nDavon spiegelnd zum Betrachter (< 10°): {:>12} mm²\nVerdeckt:                               {:>12} mm² {:>6.1} %\nAbgewandt:                              {:>12} mm² {:>6.1} %\n", f(lit), 100.0 * lit / total, f(mirror), f(shadow), 100.0 * shadow / total, f(away), 100.0 * away / total)
                };
                self.report("schatten.inf", &name.to_uppercase(), body);
            }
            "Verbindung" => {
                let list: Vec<usize> = if sel_edges.is_empty() { (0..t.chains.len()).collect() } else { sel_edges.clone() };
                let (mut g0, mut g1) = (vec![], 0usize);
                let mut body = String::new();
                for &i in &list {
                    let c = &t.chains[i];
                    let ang = c.n_a.iter().zip(&c.n_b).map(|(a, b)| a.dot(*b).clamp(-1.0, 1.0).acos().to_degrees()).fold(0.0, f64::max);
                    if ang < 1.0 {
                        g1 += 1;
                    } else {
                        g0.push(i);
                    }
                    if list.len() <= 40 {
                        body +=
                            &format!("  Kante {:>3}: {}  (Winkel zwischen den Flächen {}°)\n", i + 1, if ang < 1.0 { "G1 – tangential" } else { "G0 – Knick   " }, fmt_num(ang, 2));
                    }
                }
                let head = format!(
                    "{} Kante(n){}\nTangentiale Übergänge (G1): {}\nÜbergänge mit Knick (G0):   {}\n\n",
                    list.len(),
                    if sel_edges.is_empty() { " (ganzes Modell)" } else { " (Auswahl)" },
                    g1,
                    g0.len()
                );
                self.report("verbindung.inf", "VERBINDUNGSANALYSE", head + &body);
            }
            // ---------------- Konstruktionsstudie ----------------
            "Sensitivitätsanalyse" | "Durchführbarkeit/Optimierung" | "Multiziel-Konstruktionsstudie" | "Statistische Konstruktionsstudie" | "Bemaßungsberandungen" => {
                let ps = self.numeric_params();
                // the parameter selected in the Parameter table, else the first one
                let Some(&(pi, v0)) = ps.first() else {
                    self.report(
                        "studie.inf",
                        &name.to_uppercase(),
                        "Die Studie variiert Parameter des Modells. Dieses Teil hat keine Parameter mit Zahlenwert (Werkzeuge ▸ Parameter).\n".into(),
                    );
                    return;
                };
                let pname = self.data.parameters[pi].name.to_uppercase();
                let nominal = self.variant(&[]);
                let mut body = String::new();
                let row = |label: String, x: Option<(f64, f64, f64)>| match x {
                    Some((m, v, a)) => format!("{:<26}{:>14}{:>16}{:>16}\n", label, fmt_num(m, 5), fmt_num(v, 2), fmt_num(a, 2)),
                    None => format!("{:<26}{:>46}\n", label, "regeneriert nicht"),
                };
                let head = format!("{:<26}{:>14}{:>16}{:>16}\n", "", "Masse (kg)", "Volumen (mm³)", "Fläche (mm²)");
                match name {
                    "Sensitivitätsanalyse" => {
                        body += &format!("Parameter {} von −20 % bis +20 % (Nennwert {})\n\n{}", pname, fmt_num(v0, 4), head);
                        for k in -4..=4 {
                            let v = v0 * (1.0 + 0.05 * k as f64);
                            body += &row(format!("{} = {}", pname, fmt_num(v, 4)), self.variant(&[(pi, v)]));
                        }
                    }
                    "Durchführbarkeit/Optimierung" => {
                        let mut best: Option<(f64, f64)> = None;
                        let mut fail = 0;
                        for k in -10..=10 {
                            let v = v0 * (1.0 + 0.05 * k as f64);
                            match self.variant(&[(pi, v)]) {
                                Some((m, _, _)) => {
                                    if best.is_none_or(|b| m < b.1) {
                                        best = Some((v, m));
                                    }
                                }
                                None => fail += 1,
                            }
                        }
                        body += &format!(
                            "Ziel: Masse minimieren\nKonstruktionsvariable: {} von {} bis {} (Nennwert {})\n\n",
                            pname,
                            fmt_num(v0 * 0.5, 4),
                            fmt_num(v0 * 1.5, 4),
                            fmt_num(v0, 4)
                        );
                        body += &match best { Some((v, m)) => format!("Durchführbar: ja ({} von 21 Varianten regenerieren)\nOptimum: {} = {}  →  Masse {} kg (nominal {} kg)\n\nDas Modell wurde nicht verändert; den Wert bei Bedarf unter Werkzeuge ▸ Parameter eintragen.\n", 21 - fail, pname, fmt_num(v, 4), fmt_num(m, 5), nominal.map(|x| fmt_num(x.0, 5)).unwrap_or("–".into())), None => "Durchführbar: nein – keine Variante regeneriert.\n".to_string() };
                    }
                    "Multiziel-Konstruktionsstudie" => match ps.get(1) {
                        Some(&(qi, w0)) => {
                            let qname = self.data.parameters[qi].name.to_uppercase();
                            body += &format!("Variablen: {} und {} je −20 %, −10 %, 0, +10 %, +20 %\nZiele: Masse, Volumen, Oberfläche\n\n{}", pname, qname, head);
                            for a in -2..=2 {
                                for b in -2..=2 {
                                    let (v, w) = (v0 * (1.0 + 0.1 * a as f64), w0 * (1.0 + 0.1 * b as f64));
                                    body += &row(format!("{}={} {}={}", pname, fmt_num(v, 3), qname, fmt_num(w, 3)), self.variant(&[(pi, v), (qi, w)]));
                                }
                            }
                        }
                        None => {
                            body += &format!("Das Teil hat nur einen Zahlenparameter ({}); die Studie variiert ihn allein.\n\n{}", pname, head);
                            for k in -4..=4 {
                                let v = v0 * (1.0 + 0.05 * k as f64);
                                body += &row(format!("{} = {}", pname, fmt_num(v, 4)), self.variant(&[(pi, v)]));
                            }
                        }
                    },
                    "Statistische Konstruktionsstudie" => {
                        // 40 samples, every parameter uniformly within ±5 % (fixed seed: repeatable)
                        let mut seed = 0x2545F4914F6CDD1Du64;
                        let mut rnd = || {
                            seed ^= seed << 13;
                            seed ^= seed >> 7;
                            seed ^= seed << 17;
                            (seed >> 11) as f64 / (1u64 << 53) as f64
                        };
                        let mut ms = vec![];
                        for _ in 0..40 {
                            let set: Vec<(usize, f64)> = ps.iter().map(|(i, v)| (*i, v * (0.95 + 0.1 * rnd()))).collect();
                            if let Some((m, _, _)) = self.variant(&set) {
                                ms.push(m);
                            }
                        }
                        if ms.is_empty() {
                            body += "Keine der 40 Varianten regeneriert.\n";
                        } else {
                            let mean = ms.iter().sum::<f64>() / ms.len() as f64;
                            let sd = (ms.iter().map(|m| (m - mean).powi(2)).sum::<f64>() / ms.len() as f64).sqrt();
                            body += &format!("40 Varianten, alle {} Parameter gleichverteilt innerhalb ±5 %\n\nRegeneriert:          {} von 40\nMasse Mittelwert:     {} kg\nStandardabweichung:   {} kg\nKleinste / größte:    {} / {} kg\nNominal:              {} kg\n", ps.len(), ms.len(), fmt_num(mean, 5), fmt_num(sd, 6), fmt_num(ms.iter().cloned().fold(f64::INFINITY, f64::min), 5), fmt_num(ms.iter().cloned().fold(f64::NEG_INFINITY, f64::max), 5), nominal.map(|x| fmt_num(x.0, 5)).unwrap_or("–".into()));
                        }
                    }
                    _ => {
                        body += &format!("Alle {} Zahlenparameter an der unteren und oberen Berandung (±1 %)\n\n{}", ps.len(), head);
                        for (l, k) in [("Untere Berandung (−1 %)", 0.99), ("Nennmaß", 1.0), ("Obere Berandung (+1 %)", 1.01)] {
                            let set: Vec<(usize, f64)> = ps.iter().map(|(i, v)| (*i, v * k)).collect();
                            body += &row(l.to_string(), self.variant(&set));
                        }
                    }
                }
                self.report("studie.inf", &name.to_uppercase(), body);
            }
            "Simulate Analyse" => {
                self.tab = Tab::LiveSim;
                self.say("Live-Simulation: eine Studie anlegen und „Simulieren“ wählen.", MsgKind::Prompt);
            }
            n => self.say(format!("„{}“ ist in Freo noch nicht verfügbar.", n), MsgKind::Info),
        }
    }

    /// Modell ▸ Daten abrufen ▸ Zusammenführung/Vererbung: the geometry of another part becomes
    /// an import feature of this part (merged into the current body).
    pub fn merge_inherit(&mut self) {
        let Some(p) =
            rfd::FileDialog::new().set_title("Zusammenführung/Vererbung: Referenzmodell wählen").set_directory(self.work_dir()).add_filter("Freo-Teil", &["pfprt"]).pick_file()
        else {
            return;
        };
        let other = match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<ModelData>(&s).map_err(|e| e.to_string())) {
            Ok(d) => d,
            Err(e) => {
                self.say(format!("Das Referenzmodell konnte nicht geladen werden: {}", e), MsgKind::Error);
                return;
            }
        };
        let r = regen::run(&other);
        if r.polys.is_empty() {
            self.say("Das Referenzmodell enthält keine Volumengeometrie.", MsgKind::Warning);
            return;
        }
        self.inherit_geometry(&other.name, &r);
    }

    /// Adds the regenerated geometry `r` of the part `name` as an import feature.
    pub fn inherit_geometry(&mut self, name: &str, r: &RegenResult) {
        let t = &r.topo;
        // one imported face per face of the reference model, so the faces stay selectable
        let faces: Vec<ImportFace> = t
            .faces
            .iter()
            .map(|face| {
                let (mut v, mut tr) = (vec![], vec![]);
                for &pi in &face.polys {
                    let base = v.len() as u32;
                    for (k, &vi) in t.polys[pi].iter().enumerate() {
                        let (p, n) = (t.positions[vi], t.vnormals[pi].get(k).copied().unwrap_or(t.planes[pi].n));
                        v.push([p.x as f32, p.y as f32, p.z as f32, n.x as f32, n.y as f32, n.z as f32]);
                    }
                    for k in 1..t.polys[pi].len() as u32 - 1 {
                        tr.push([base, base + k, base + k + 1]);
                    }
                }
                ImportFace { v, t: tr }
            })
            .collect();
        let kind = FeatureKind::Import(ImportDef { source: format!("{}.prt", name.to_lowercase()), format: "Zusammenführung".into(), faces });
        let id = self.data.next_id;
        let f = Feature { id, name: self.data.next_name(&kind), suppressed: false, hidden: false, kind };
        let label = format!("Geometrie von {} wurde zusammengeführt.", name.to_uppercase());
        let at = self.insert_position();
        self.commit(&label, |d| {
            let i = at.min(d.features.len());
            d.features.insert(i, f);
            d.next_id = id + 1;
            if let Some(ii) = d.insert_index {
                d.insert_index = Some(ii + 1);
            }
        });
    }

    /// Modell ▸ Editieren ▸ Flächen vereinigen: Freo keeps faces with the same geometry as one
    /// face when it regenerates, so the command regenerates and reports the result.
    pub fn unite_faces(&mut self) {
        let before = self.result.topo.faces.len();
        self.regenerate(false);
        let after = self.result.topo.faces.len();
        self.say(
            if after < before {
                format!("{} Fläche(n) wurden vereinigt; das Modell hat jetzt {} Flächen.", before - after, after)
            } else {
                format!("Alle Flächen mit derselben Geometrie sind bereits vereinigt ({} Flächen).", after)
            },
            MsgKind::Info,
        );
    }

    /// Modell ▸ Editieren ▸ Entfernen: the selected faces are taken out of the solid and the
    /// openings closed, as a feature of its own (it can be redefined or deleted like any other).
    pub fn remove_faces(&mut self) {
        let t = self.result.topo.clone();
        let faces: Vec<FaceRef> =
            self.selection.iter().filter_map(|p| if let Pick::Face(f) = p { Some(*f) } else { None }).filter(|f| *f < t.faces.len()).map(|f| t.surface_ref(f)).collect();
        self.start_feature(FeatureKind::new_surface(SurfOp::Remove));
        if faces.is_empty() {
            self.say("Entfernen: die zu entfernenden Flächen auswählen (z. B. die Wand einer Bohrung, die Flächen einer Tasche oder eine Rundung).", MsgKind::Prompt);
            return;
        }
        self.update_editor(|f| {
            if let FeatureKind::Surface(d) = &mut f.kind {
                d.faces = faces;
            }
        });
        self.set_pick_target(None);
    }
}
