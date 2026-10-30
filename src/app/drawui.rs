//! Drawing mode UI (Creo Drawing): the Layout / Table / Annotate / Publish ribbons, the
//! drawing tree, the sheet canvas (view placement, selection, dragging, dimensioning and
//! in-place text editing), the Drawing View and Drawing Properties dialogs, and the drawing
//! file operations.

use super::drawing::*;
use super::icons::{self, Icon};
use super::ribbon::{group, icon_button, large, menu_entry, menu_spacing, small, stack, theme::*};
use super::*;
use crate::model::drawing::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke};

/// State of the New Drawing dialog.
#[derive(Clone)]
pub struct NewDrawing { pub name: String, pub size: SheetSize, pub orient: Orientation, pub template: bool, pub projection: Projection }

fn c32(c: Rgb) -> Color32 { Color32::from_rgb(c[0], c[1], c[2]) }

fn parse_scale(s: &str) -> Option<f64> {
    let t = s.trim();
    if let Some((a, b)) = t.split_once(':') { let (a, b) = (a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?); return (a > 0.0 && b > 0.0).then_some(a / b); }
    t.parse::<f64>().ok().filter(|v| *v > 0.0)
}

/// Draws sheet primitives through a sheet → screen mapping.
pub fn paint_prims(painter: &egui::Painter, prims: &[Prim], to_s: &dyn Fn(V2) -> Pos2, zoom: f64) {
    let px = |w: f64| ((w * zoom) as f32 * 0.8).clamp(0.6, 3.5);
    let dashed = |pts: &[Pos2], st: Stroke, d: Dash| {
        let p = d.pattern();
        let dashes: Vec<f32> = p.iter().step_by(2).map(|v| (*v * zoom) as f32).collect();
        let gaps: Vec<f32> = p.iter().skip(1).step_by(2).map(|v| (*v * zoom) as f32).collect();
        if dashes.iter().any(|v| *v < 0.5) { return painter.add(Shape::line(pts.to_vec(), st)); }
        let mut shapes = vec![];
        Shape::dashed_line_many_with_offset(pts, st, &dashes, &gaps, 0.0, &mut shapes);
        painter.add(Shape::Vec(shapes))
    };
    for p in prims {
        match p {
            Prim::Line { pts, w, c, dash } => {
                let sp: Vec<Pos2> = pts.iter().map(|q| to_s(*q)).collect();
                let st = Stroke::new(px(*w), c32(*c));
                if *dash == Dash::Solid { painter.add(Shape::line(sp, st)); } else { dashed(&sp, st, *dash); }
            }
            Prim::Fill { pts, c } => { painter.add(Shape::convex_polygon(pts.iter().map(|q| to_s(*q)).collect(), c32(*c), Stroke::NONE)); }
            Prim::Circle { c, r, w, col, dash, fill } => {
                let cs = to_s(*c);
                let rs = (*r * zoom) as f32;
                if *fill { painter.circle_filled(cs, rs.max(1.0), c32(*col)); }
                else if *dash == Dash::Solid { painter.circle_stroke(cs, rs, Stroke::new(px(*w), c32(*col))); }
                else {
                    let pts: Vec<Pos2> = (0..=96).map(|k| { let t = std::f64::consts::TAU * k as f64 / 96.0; to_s(*c + V2::new(t.cos(), t.sin()) * *r) }).collect();
                    dashed(&pts, Stroke::new(px(*w), c32(*col)), *dash);
                }
            }
            Prim::Text { pos, text, h, angle, halign, valign, c } => {
                if text.is_empty() { continue; }
                let size = (text_em(*h) * zoom) as f32;
                if size < 2.0 { continue; }
                let o = text_origin(*pos, text, *h, *angle, *halign, *valign);
                let os = to_s(o);
                let up = V2::new(-angle.sin(), angle.cos());
                let tl = os + vec2(up.x as f32, -up.y as f32) * (size * 0.93);
                let g = painter.layout_no_wrap(text.clone(), FontId::proportional(size), c32(*c));
                painter.add(egui::epaint::TextShape::new(tl, g, c32(*c)).with_angle(-*angle as f32));
            }
        }
    }
}

impl App {
    // ---------------- lifecycle ----------------

    pub fn model_kind_name(&self) -> (bool, String) {
        match self.asm.as_ref().filter(|a| a.editing.is_none()) { Some(a) => (true, a.data.name.clone()), None => (false, self.data.name.clone()) }
    }

    pub fn new_drawing(&mut self, nd: &NewDrawing) {
        self.leave_home();
        if self.busy() { self.say("Vor dem Erzeugen einer Zeichnung das aktive KE abschließen.", MsgKind::Warning); return; }
        if self.drw.as_ref().map(|d| d.dirty).unwrap_or(false) {
            let r = rfd::MessageDialog::new().set_title("Nicht gespeicherte Zeichnung").set_description("Nicht gespeicherte Änderungen an der aktuellen Zeichnung verwerfen?").set_buttons(rfd::MessageButtons::OkCancel).show();
            if r != rfd::MessageDialogResult::Ok { return; }
        }
        let (assembly, model) = self.model_kind_name();
        let mut data = DrawingData::new(&nd.name.to_uppercase(), &model, nd.size, nd.orient);
        data.projection = nd.projection;
        self.drw = Some(DrawSession::new(data, assembly, &model));
        if nd.template {
            let m = self.drawing_model(false);
            let b = m.topo.bbox;
            let size = if b.is_empty() { V3::splat(50.0) } else { b.size() };
            if let Some(d) = &mut self.drw { standard_views(&mut d.data, 0, size); }
        }
        if let Some(d) = &mut self.drw { d.dirty = true; }
        self.tab = Tab::Layout;
        self.say(format!("Zeichnung {} von {} erzeugt.", nd.name.to_uppercase(), model), MsgKind::Success);
    }

    /// Shows the drawing's model in 3D; the drawing waits in the background.
    pub fn drawing_open_model(&mut self) {
        let Some(d) = &mut self.drw else { return };
        d.active = false;
        d.tool = DTool::None;
        self.tab = Tab::Model;
        self.fit_pending = true;
        self.say("Modell geöffnet. Mit Zurück zur Zeichnung zum Blatt zurückkehren.", MsgKind::Prompt);
    }

    pub fn drawing_return(&mut self) {
        if self.busy() { self.say("Zuerst das aktive KE abschließen.", MsgKind::Warning); return; }
        let (assembly, name) = self.model_kind_name();
        let Some(d) = &mut self.drw else { return };
        if d.assembly != assembly || d.model_name != name {
            let msg = format!("Die Zeichnung dokumentiert {}, das nicht mehr das Modell in der Sitzung ist.", d.model_name);
            self.say(msg, MsgKind::Error);
            return;
        }
        d.active = true;
        d.sel.clear();
        self.tab = Tab::Layout;
    }

    pub fn close_drawing(&mut self) {
        let dirty = self.drw.as_ref().map(|d| d.dirty).unwrap_or(false);
        if dirty {
            let r = rfd::MessageDialog::new().set_title("Nicht gespeicherte Zeichnung").set_description("Nicht gespeicherte Änderungen an der Zeichnung verwerfen?").set_buttons(rfd::MessageButtons::OkCancel).show();
            if r != rfd::MessageDialogResult::Ok { return; }
        }
        self.drw = None;
        self.tab = Tab::Model;
        self.fit_pending = true;
        self.say("Zeichnung geschlossen.", MsgKind::Info);
    }

    fn write_model_file(&mut self) -> Result<(), String> {
        let Some(p) = self.path.clone() else { return Ok(()) };
        if !self.dirty { return Ok(()); }
        if self.in_asm() { self.asm_save(false); return Ok(()); }
        serde_json::to_string_pretty(&self.data).map_err(|e| e.to_string()).and_then(|s| std::fs::write(&p, s).map_err(|e| e.to_string()))?;
        self.dirty = false;
        Ok(())
    }

    pub fn save_drawing(&mut self, save_as: bool) {
        let Some(d) = &self.drw else { return };
        let (cur, file) = (d.path.clone(), format!("{}.pfdrw", d.data.name.to_lowercase()));
        let path = self.save_target(cur.as_ref(), save_as, &file, super::creo::SaveKind::Drawing);
        if let Some(p) = path { self.save_drawing_to(p); }
    }

    /// Writes the drawing (and its model) to `p`.
    pub fn save_drawing_to(&mut self, p: PathBuf) {
        if let Err(e) = self.write_model_file() { self.say(format!("Modell konnte nicht gespeichert werden: {}", e), MsgKind::Error); }
        let assembly = self.drw.as_ref().map(|d| d.assembly).unwrap_or(false);
        let file = DrawingFile {
            drawing: self.drw.as_ref().unwrap().data.clone(),
            model_path: self.path.clone(),
            part: (!assembly).then(|| self.data.clone()),
            assembly: if assembly { self.asm.as_ref().map(|a| a.data.clone()) } else { None },
        };
        match serde_json::to_string_pretty(&file).map_err(|e| e.to_string()).and_then(|s| std::fs::write(&p, s).map_err(|e| e.to_string())) {
            Ok(_) => {
                self.say(format!("{} gespeichert.", p.display()), MsgKind::Success);
                self.add_recent(&p);
                if let Some(d) = &mut self.drw { d.path = Some(p); d.dirty = false; }
            }
            Err(e) => self.say(format!("Speichern fehlgeschlagen: {}", e), MsgKind::Error),
        }
    }

    pub fn load_drawing(&mut self, p: PathBuf) {
        let file = match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<DrawingFile>(&s).map_err(|e| e.to_string())) {
            Ok(f) => f,
            Err(e) => { self.say(format!("Zeichnung konnte nicht geöffnet werden: {}", e), MsgKind::Error); return; }
        };
        let from_file = file.model_path.as_ref().filter(|m| m.is_file()).cloned();
        match (from_file, file.part, file.assembly) {
            (Some(mp), _, _) => self.load(mp),
            (None, Some(part), _) => { self.new_part_named(&part.name); self.data = part; self.regenerate(false); }
            (None, None, Some(asm)) => { self.new_assembly(&asm.name); if let Some(a) = &mut self.asm { a.data = asm; } self.asm_regenerate(false); }
            _ => { self.say("Die Zeichnung hat kein Modell.", MsgKind::Error); return; }
        }
        let (assembly, name) = self.model_kind_name();
        let mut s = DrawSession::new(file.drawing, assembly, &name);
        s.path = Some(p.clone());
        self.drw = Some(s);
        self.tab = Tab::Layout;
        self.add_recent(&p);
        if let Some(dir) = p.parent() { self.browser_dir = dir.to_path_buf(); }
        self.say(format!("Zeichnung {} geöffnet.", p.display()), MsgKind::Success);
    }

    /// Datei ▸ PDF / DXF / SVG: asks where in Freo's explorer, then writes.
    pub fn export_drawing(&mut self, kind: &str) {
        let Some(d) = &self.drw else { return };
        let name = d.data.name.to_lowercase();
        let dir = self.save_dir(d.path.as_ref());
        self.browse(super::creo::BrowsePurpose::DrawingExport(kind.to_string()), dir, &name);
    }

    pub fn export_drawing_to(&mut self, kind: &str, p: PathBuf) {
        let Some(d) = &self.drw else { return };
        let si = d.sheet;
        let n = d.data.sheets.len();
        let result = match kind {
            "pdf" => {
                let mut pages = vec![];
                for i in 0..n {
                    let (w, h) = self.drw.as_ref().unwrap().data.sheets[i].dims_mm();
                    pages.push((w, h, self.sheet_scene(i, &[]).0));
                }
                let title = self.drw.as_ref().unwrap().data.title.clone();
                std::fs::write(&p, publish::pdf(&pages, &title)).map_err(|e| e.to_string())
            }
            "svg" => {
                let (w, h) = self.drw.as_ref().unwrap().data.sheets[si].dims_mm();
                let prims = self.sheet_scene(si, &[]).0;
                std::fs::write(&p, publish::svg(w, h, &prims)).map_err(|e| e.to_string())
            }
            _ => {
                let prims = self.sheet_scene(si, &[]).0;
                std::fs::write(&p, publish::dxf(&prims)).map_err(|e| e.to_string())
            }
        };
        match result {
            Ok(_) => self.say(format!("{}{} exportiert.", p.display(), if kind == "pdf" && n > 1 { format!(" ({} Blätter)", n) } else { String::new() }), MsgKind::Success),
            Err(e) => self.say(format!("Export fehlgeschlagen: {}", e), MsgKind::Error),
        }
    }

    // ---------------- editing operations ----------------

    /// Views that deleting the selection removes without being selected themselves
    /// (projection and detail views derived from a selected view).
    pub fn delete_dependents(&self) -> Vec<String> {
        let Some(d) = &self.drw else { return vec![] };
        let sh = d.cur();
        let picked: Vec<u32> = d.sel.iter().filter_map(|s| if let DSel::View(id) = s { Some(*id) } else { None }).collect();
        let mut out = vec![];
        for id in &picked {
            for f in sh.family(*id) {
                if picked.contains(&f) { continue; }
                if let Some(v) = sh.view(f) { let n = format!("{} ({})", v.name, match v.kind { ViewKind::Detail => "Detail", ViewKind::Projection => "Projektion", ViewKind::General => "Allgemein" }); if !out.contains(&n) { out.push(n); } }
            }
        }
        out
    }

    /// Deletes the selection, asking first when dependent views would go with it.
    pub fn drw_delete_interactive(&mut self) {
        let deps = self.delete_dependents();
        if !deps.is_empty() {
            let msg = format!("Die gewählte Ansicht hat {} abhängige Ansicht(en), die ebenfalls gelöscht werden:\n\n{}\n\nAlle löschen?", deps.len(), deps.join("\n"));
            let r = rfd::MessageDialog::new().set_title("Ansichten löschen").set_description(msg).set_buttons(rfd::MessageButtons::OkCancel).show();
            if r != rfd::MessageDialogResult::Ok { return; }
        }
        self.drw_delete_selection();
    }

    pub fn drw_delete_selection(&mut self) {
        let Some(d) = &self.drw else { return };
        let sel = d.sel.clone();
        if sel.is_empty() { return; }
        let si = d.sheet;
        let mut views: Vec<u32> = vec![];
        for s in &sel { if let DSel::View(id) = s { views.extend(d.data.sheets[si].family(*id)); } }
        self.drw_commit(|dd| {
            let sh = &mut dd.sheets[si];
            let mut shown: Vec<usize> = sel.iter().filter_map(|s| if let DSel::Shown(i) = s { Some(*i) } else { None }).collect();
            let mut balloons: Vec<usize> = sel.iter().filter_map(|s| if let DSel::Balloon(i) = s { Some(*i) } else { None }).collect();
            shown.sort_unstable_by(|a, b| b.cmp(a));
            balloons.sort_unstable_by(|a, b| b.cmp(a));
            for i in shown { if i < sh.shown.len() { sh.shown.remove(i); } }
            for i in balloons { if i < sh.balloons.len() { sh.balloons.remove(i); } }
            sh.dims.retain(|x| !sel.contains(&DSel::Dim(x.id)) && !views.contains(&x.view));
            sh.notes.retain(|x| !sel.contains(&DSel::Note(x.id)));
            sh.gtols.retain(|x| !sel.contains(&DSel::Gtol(x.id)) && !views.contains(&x.view));
            sh.datums.retain(|x| !sel.contains(&DSel::Datum(x.id)) && !views.contains(&x.view));
            sh.finishes.retain(|x| !sel.contains(&DSel::Finish(x.id)) && !views.contains(&x.view));
            // frames riding on erased dimensions go with them
            let drawn: Vec<u32> = sh.dims.iter().map(|x| x.id).collect();
            let shown: Vec<String> = sh.shown.iter().map(|x| x.key.clone()).collect();
            sh.gtols.retain(|g| match &g.target { Attach::Dim(DimTarget::Drawn(id)) => drawn.contains(id), Attach::Dim(DimTarget::Shown(k)) => shown.contains(k), _ => true });
            if sel.contains(&DSel::Bom) { sh.bom = None; }
            sh.views.retain(|v| !views.contains(&v.id));
            sh.shown.retain(|x| !views.contains(&x.view));
            sh.balloons.retain(|x| !views.contains(&x.view));
        });
        if let Some(d) = &mut self.drw { d.sel.clear(); }
        self.say("Gelöscht.", MsgKind::Info);
    }

    pub fn set_tool(&mut self, t: DTool) {
        let prompt = match &t {
            DTool::General => "MITTELPUNKT für die Zeichnungsansicht wählen.",
            DTool::Projection { parent: None } => "Die übergeordnete Ansicht für die Projektion wählen.",
            DTool::Projection { .. } => "MITTELPUNKT für die Projektionsansicht wählen (links, rechts, über oder unter der übergeordneten Ansicht).",
            DTool::Detail { .. } => "Den Mittelpunkt des Detailbereichs in einer vorhandenen Ansicht wählen.",
            DTool::Dimension { .. } => "Zu bemaßende Elemente wählen: einen Eckpunkt, eine Kante oder einen Kreis; dann klicken, um die Bemaßung zu platzieren. Esc zum Beenden.",
            DTool::Note => "Die Position der Notiz wählen.",
            DTool::Datum { at: None } => "Das Bezugselement wählen: eine Kante oder einen Eckpunkt in einer Ansicht.",
            DTool::Datum { .. } => "Klicken, um das Bezugssymbol zu platzieren.",
            DTool::Gtol { at: None } => "Eine Bemaßung wählen, an die die geometrische Toleranz angehängt wird, oder eine Kante für eine Hinweislinie.",
            DTool::Gtol { .. } => "Klicken, um den Toleranzrahmen zu platzieren.",
            DTool::Finish => "Die Kante der Fläche wählen, die das Oberflächensymbol erhält.",
            DTool::None => "",
        };
        if !prompt.is_empty() { self.say(prompt, MsgKind::Prompt); }
        if let Some(d) = &mut self.drw { d.tool = t; d.snap = None; }
    }

    fn selected_views(&self) -> Vec<u32> {
        self.drw.as_ref().map(|d| d.sel.iter().filter_map(|s| if let DSel::View(id) = s { Some(*id) } else { None }).collect()).unwrap_or_default()
    }

    fn open_view_dialog(&mut self, id: u32, is_new: bool) {
        let Some(d) = &mut self.drw else { return };
        let Some(v) = d.data.view(id).cloned() else { return };
        let custom = v.scale.map(scale_text).unwrap_or_default();
        d.view_dialog = Some(ViewDialog { view: id, draft: v, cat: 0, is_new, custom_scale: custom });
    }

    /// Moves a view and the views derived from it; projection views slide along their parent's axis.
    pub fn move_view(&mut self, id: u32, delta: V2) {
        let Some(d) = &mut self.drw else { return };
        if d.lock_views { return; }
        let si = d.sheet;
        let sh = &mut d.data.sheets[si];
        let Some(v) = sh.view(id) else { return };
        let delta = match (v.kind, v.side) {
            (ViewKind::Projection, Some(s)) if s.horizontal() => V2::new(delta.x, 0.0),
            (ViewKind::Projection, Some(_)) => V2::new(0.0, delta.y),
            _ => delta,
        };
        for fid in sh.family(id) {
            // details keep their own placement
            if fid != id && sh.view(fid).map(|x| x.kind == ViewKind::Detail).unwrap_or(false) { continue; }
            if let Some(x) = sh.view_mut(fid) { x.pos += delta; }
        }
    }

    fn drag_item(&mut self, sel: DSel, delta: V2) {
        if let DSel::View(id) = sel { self.move_view(id, delta); return; }
        let Some(d) = &mut self.drw else { return };
        let si = d.sheet;
        let sh = &mut d.data.sheets[si];
        match sel {
            DSel::Shown(i) => if let Some(s) = sh.shown.get_mut(i) { s.label = Some(s.label.unwrap_or(V2::ZERO) + delta); },
            DSel::Dim(id) => if let Some(x) = sh.dims.iter_mut().find(|x| x.id == id) { x.label += delta; },
            DSel::Note(id) => if let Some(x) = sh.notes.iter_mut().find(|x| x.id == id) { x.pos += delta; },
            DSel::Balloon(i) => if let Some(b) = sh.balloons.get_mut(i) { b.pos += delta; },
            DSel::Bom => if let Some(t) = &mut sh.bom { t.pos += delta; },
            DSel::Gtol(id) => if let Some(g) = sh.gtols.iter_mut().find(|x| x.id == id) { if matches!(g.target, Attach::Geom(_)) { g.pos += delta; } },
            DSel::Datum(id) => if let Some(x) = sh.datums.iter_mut().find(|x| x.id == id) { x.pos += delta; },
            DSel::View(_) | DSel::Finish(_) => {}
        }
    }

    /// Pickable geometry near a sheet point (dimension tool).
    pub fn snap_at(&mut self, p: V2, tol: f64) -> Option<Snap> {
        let d = self.drw.as_ref()?;
        let sheet = d.cur().clone();
        for v in sheet.views.iter().rev() {
            let Some(g) = self.view_geom(v.id) else { continue };
            let (rmin, rmax) = g.rect(v.pos);
            if p.x < rmin.x - tol || p.y < rmin.y - tol || p.x > rmax.x + tol || p.y > rmax.y + tol { continue; }
            let t = &g.model.topo;
            let mut best: Option<(f64, Snap)> = None;
            let mut offer = |score: f64, s: Snap| { if best.as_ref().map(|b| score < b.0).unwrap_or(true) { best = Some((score, s)); } };
            for ch in &g.lines.chains {
                let Some(tc) = t.chains.get(ch.chain) else { continue };
                let er = t.edge_ref(ch.chain);
                if let Some((c, r)) = ch.circle {
                    let (cs, rs) = (g.to_sheet(v.pos, c), r * g.scale);
                    let c3 = crate::kernel::hlr::chain_circle(&tc.points).map(|x| x.0).unwrap_or(tc.mid());
                    let a = DimRef { point: c3, edge: Some(er), at: RefAt::Center, radius: r, t: 0.5 };
                    let dc = (p - cs).length();
                    if (dc - rs).abs() < tol { offer((dc - rs).abs(), Snap { view: v.id, kind: SnapKind::Circle { c: cs, r: rs }, sheet: cs + safe_norm2(p - cs, V2::X) * rs, a, b: None }); }
                    if dc < tol { offer(dc - tol * 0.5, Snap { view: v.id, kind: SnapKind::Point, sheet: cs, a, b: None }); }
                    continue;
                }
                if ch.closed || ch.pts.len() < 2 { continue; }
                let (s0, s1) = (g.to_sheet(v.pos, ch.pts[0]), g.to_sheet(v.pos, *ch.pts.last().unwrap()));
                let (r0, r1) = (DimRef { point: tc.points[0], edge: Some(er), at: RefAt::Start, radius: 0.0, t: 0.0 }, DimRef { point: *tc.points.last().unwrap(), edge: Some(er), at: RefAt::End, radius: 0.0, t: 1.0 });
                for (s, r) in [(s0, r0), (s1, r1)] { let dd = (p - s).length(); if dd < tol { offer(dd - tol * 0.6, Snap { view: v.id, kind: SnapKind::Point, sheet: s, a: r, b: None }); } }
                if tc.is_straight() && (s1 - s0).length() > 1e-6 {
                    let dd = dist_point_segment2(p, s0, s1);
                    if dd < tol { offer(dd + tol * 0.2, Snap { view: v.id, kind: SnapKind::Edge { a: s0, b: s1 }, sheet: p, a: r0, b: Some(r1) }); }
                }
            }
            if let Some((_, s)) = best { return Some(s); }
        }
        None
    }

    fn dim_type_for(pa: V2, pb: V2, l: V2) -> DimType {
        let (dx, dy) = ((pb.x - pa.x).abs(), (pb.y - pa.y).abs());
        if dx < 1e-6 { return DimType::Vertical; }
        if dy < 1e-6 { return DimType::Horizontal; }
        let (x0, x1, y0, y1) = (pa.x.min(pb.x), pa.x.max(pb.x), pa.y.min(pb.y), pa.y.max(pb.y));
        if l.x >= x0 && l.x <= x1 && (l.y > y1 || l.y < y0) { DimType::Horizontal }
        else if l.y >= y0 && l.y <= y1 && (l.x < x0 || l.x > x1) { DimType::Vertical }
        else { DimType::Aligned }
    }

    pub fn place_dimension(&mut self, first: Snap, second: Option<Snap>, at: V2) {
        let Some(d) = &self.drw else { return };
        let Some(v) = d.cur().view(first.view).cloned() else { return };
        let (ty, a, b) = match (first.kind, second) {
            (SnapKind::Circle { .. }, _) => (DimType::Diameter, first.a, None),
            (SnapKind::Edge { a: sa, b: sb }, _) => (Self::dim_type_for(sa, sb, at), first.a, first.b),
            (SnapKind::Point, Some(s2)) => (Self::dim_type_for(first.sheet, s2.sheet, at), first.a, Some(s2.a)),
            _ => return,
        };
        let si = d.sheet;
        self.drw_commit(|dd| {
            let id = dd.take_id();
            dd.sheets[si].dims.push(DrawnDim { id, view: v.id, ty, a, b, label: at - v.pos, text: None, tol: Default::default() });
        });
    }

    fn start_edit(&mut self, sel: DSel, screen: Pos2) {
        let dims = self.model_dims();
        let Some(d) = &mut self.drw else { return };
        let sh = d.cur();
        let (text, multiline) = match sel {
            DSel::Shown(i) => {
                let Some(md) = sh.shown.get(i).and_then(|s| dims.iter().find(|m| m.key == s.key)) else { return };
                if md.dk.is_none() { self.say("Diese Bemaßung stammt aus einer Normbohrungstabelle; die Bohrungsgröße im Modell ändern.", MsgKind::Info); return; }
                (md.edit.clone(), false)
            }
            DSel::Dim(id) => (sh.dims.iter().find(|x| x.id == id).and_then(|x| x.text.clone()).unwrap_or_else(|| "@D".into()), false),
            DSel::Note(id) => (sh.notes.iter().find(|x| x.id == id).map(|x| x.text.clone()).unwrap_or_default(), true),
            _ => return,
        };
        d.edit = Some(TextEdit { sel, text, pos: screen, focus: true, multiline });
    }

    pub fn commit_edit(&mut self, sel: DSel, text: String) {
        let dims = self.model_dims();
        let Some(si) = self.drw.as_ref().map(|d| d.sheet) else { return };
        match sel {
            DSel::Shown(i) => {
                let key = self.drw.as_ref().and_then(|d| d.data.sheets[si].shown.get(i).map(|s| s.key.clone()));
                if let Some(md) = key.and_then(|k| dims.into_iter().find(|m| m.key == k)) {
                    if let Some(dk) = md.dk { self.apply_dim_edit(md.feature, dk, text); }
                }
            }
            DSel::Dim(id) => self.drw_commit(|d| if let Some(x) = d.sheets[si].dims.iter_mut().find(|x| x.id == id) { x.text = if text.trim().is_empty() || text.trim() == "@D" { None } else { Some(text) }; }),
            DSel::Note(id) => self.drw_commit(|d| {
                if text.trim().is_empty() { d.sheets[si].notes.retain(|x| x.id != id); }
                else if let Some(x) = d.sheets[si].notes.iter_mut().find(|x| x.id == id) { x.text = text; }
            }),
            _ => {}
        }
    }

    // ---------------- frame ----------------

    pub fn drawing_frame(&mut self, ctx: &egui::Context) {
        self.drawing_shortcuts(ctx);
        self.sync_context_tab();
        let name = self.drw.as_ref().map(|d| d.data.name.clone()).unwrap_or_default();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} (Active) - Freo", name)));
        self.title_bar(ctx);
        self.tab_strip(ctx);
        egui::TopBottomPanel::top("ribbon").frame(egui::Frame::NONE.inner_margin(egui::Margin::same(4))).show(ctx, |ui| {
            super::ribbon::ribbon_background(ui);
            egui::ScrollArea::horizontal().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                ui.horizontal(|ui| { ui.spacing_mut().item_spacing.x = 0.0; self.drawing_ribbon(ui); });
            });
            let r = ui.max_rect();
            ui.painter().line_segment([pos2(r.left() - 4.0, r.bottom() + 3.5), pos2(r.right() + 4.0, r.bottom() + 3.5)], Stroke::new(1.0_f32, BORDER));
        });
        self.drawing_status(ctx);
        if self.show_nav {
            egui::SidePanel::left("navigator").resizable(true).default_width(250.0).width_range(170.0..=520.0)
                .frame(egui::Frame::NONE.fill(Color32::WHITE).inner_margin(egui::Margin::same(4)))
                .show(ctx, |ui| self.drawing_tree(ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| self.drawing_canvas(ui));
        self.file_menu_ui(ctx);
        self.drawing_dialogs(ctx);
        self.dialogs(ctx);
        self.new_dialogs(ctx);
    }

    fn drawing_shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.wants_keyboard_input();
        let (cmd, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |k: egui::Key| ctx.input(|i| i.key_pressed(k));
        if cmd && pressed(egui::Key::S) { self.save(shift); }
        if cmd && pressed(egui::Key::O) { self.open(); }
        if cmd && pressed(egui::Key::N) { self.confirm_new(); }
        if typing { return; }
        if cmd && pressed(egui::Key::Z) { self.drw_undo(shift); }
        if cmd && pressed(egui::Key::F) { if let Some(d) = &mut self.drw { d.fit = true; } }
        if pressed(egui::Key::Escape) {
            if self.file_menu { self.file_menu = false; }
            else if let Some(d) = &mut self.drw {
                if d.edit.is_some() { d.edit = None; }
                else if !matches!(d.tool, DTool::None) {
                    d.tool = match d.tool { DTool::Dimension { first: Some(_) } => DTool::Dimension { first: None }, _ => DTool::None };
                    d.snap = None;
                } else { d.sel.clear(); }
            }
        }
        // ⌘⌫ belongs to the file dialogs (Papierkorb); plain ⌫ deletes drawing items
        let file_ui = self.creo.open.is_some() || self.creo.export.is_some() || self.creo.name_dialog.is_some() || self.creo.browser.is_some();
        if !ctx.input(|i| i.modifiers.command) && !file_ui && (pressed(egui::Key::Delete) || pressed(egui::Key::Backspace)) { self.drw_delete_interactive(); }
    }

    fn drawing_status(&mut self, ctx: &egui::Context) {
        let info = self.drw.as_ref().map(|d| {
            let s = d.cur();
            (d.sheet + 1, d.data.sheets.len(), scale_text(s.scale), s.size.name(), if d.assembly { "BAUGRUPPE" } else { "TEIL" }, d.model_name.clone(), d.sel.len())
        });
        egui::TopBottomPanel::bottom("status").exact_height(25.0).frame(egui::Frame::NONE.fill(STATUS_BG).inner_margin(2.0)).show(ctx, |ui| {
            let r = ui.max_rect();
            ui.painter().line_segment([r.left_top() - vec2(4.0, 2.0), r.right_top() + vec2(4.0, -2.0)], Stroke::new(1.0_f32, BORDER));
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if icon_button(ui, Icon::TreeTab, "Navigator ein-/ausblenden", true, self.show_nav, 14.0).clicked() { self.show_nav = !self.show_nav; }
                ui.separator();
                if let Some(m) = self.messages.last() {
                    let c = match m.kind { MsgKind::Error => Color32::from_rgb(200, 36, 28), MsgKind::Warning => Color32::from_rgb(190, 120, 0), MsgKind::Success => Color32::from_rgb(20, 120, 50), MsgKind::Prompt => Color32::from_rgb(20, 100, 200), MsgKind::Info => Color32::from_rgb(50, 60, 75) };
                    let resp = ui.add(egui::Label::new(RichText::new(format!("•  {}", m.text)).color(c).size(12.0)).truncate().sense(egui::Sense::click()));
                    if resp.clicked() { self.windows.messages = true; }
                }
                if let Some((s, n, sc, size, ty, name, sel)) = info {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        for t in [format!("{} ausgewählt", sel), format!("Name: {}", name), format!("Typ: {}", ty), format!("Format: {}", size), format!("Maßstab: {}", sc), format!("Blatt {} von {}", s, n)] {
                            ui.label(RichText::new(t).size(11.5).color(LABEL));
                        }
                    });
                }
            });
        });
    }

    // ---------------- ribbon ----------------

    fn drawing_ribbon(&mut self, ui: &mut egui::Ui) {
        let Some(d) = &self.drw else { return };
        let assembly = d.assembly;
        let lock = d.lock_views;
        let views = self.selected_views();
        let one_view = views.len() == 1;
        let has_views = !d.cur().views.is_empty();
        let tool = d.tool.clone();
        let cur_style = views.first().and_then(|id| d.data.view(*id)).map(|v| (v.style, v.axes));
        match self.tab {
            Tab::Layout => {
                group(ui, "Dokument", |ui| {
                    if large(ui, Icon::NewSheet, "Neues\nBlatt", true, false).on_hover_text("Ein Blatt hinzufügen").clicked() { self.add_sheet(); }
                    stack(ui, |ui| {
                        if small(ui, Icon::TitleBlock, "Blatteinrichtung", true, false).clicked() { if let Some(d) = &mut self.drw { d.props = true; } }
                        let n = self.drw.as_ref().map(|d| d.data.sheets.len()).unwrap_or(1);
                        if small(ui, Icon::Delete, "Blatt löschen", n > 1, false).clicked() { self.delete_sheet(); }
                        if small(ui, Icon::Lock, "Ansichtsbewegung sperren", true, lock).clicked() { if let Some(d) = &mut self.drw { d.lock_views = !d.lock_views; } }
                    });
                });
                group(ui, "Modellansichten", |ui| {
                    if large(ui, Icon::GeneralView, "Allgemeine\nAnsicht", true, tool == DTool::General).on_hover_text("Eine allgemeine Ansicht des Modells platzieren").clicked() { self.set_tool(DTool::General); }
                    let parent = views.first().copied().filter(|id| self.drw.as_ref().and_then(|d| d.data.view(*id)).map(|v| v.kind != ViewKind::Detail).unwrap_or(false));
                    if large(ui, Icon::ProjView, "Projektions-\nansicht", has_views, matches!(tool, DTool::Projection { .. })).on_hover_text("Eine Ansicht aus einer übergeordneten Ansicht projizieren").clicked() { self.set_tool(DTool::Projection { parent }); }
                    if large(ui, Icon::DetailView, "Detail-\nansicht", has_views, matches!(tool, DTool::Detail { .. })).on_hover_text("Einen Bereich einer Ansicht vergrößern").clicked() { self.set_tool(DTool::Detail { parent: None, center: None, radius: None }); }
                    stack(ui, |ui| {
                        if small(ui, Icon::SectionView, "Schnitt", one_view, false).on_hover_text("Die gewählte Ansicht durch eine Bezugsebene schneiden").clicked() { if let Some(id) = views.first() { self.open_view_dialog(*id, false); if let Some(vd) = self.drw.as_mut().and_then(|d| d.view_dialog.as_mut()) { vd.cat = 2; } } }
                        if small(ui, Icon::EditDef, "Ansichtseigenschaften", one_view, false).clicked() { if let Some(id) = views.first() { self.open_view_dialog(*id, false); } }
                        if small(ui, Icon::Delete, "Ansicht löschen", !views.is_empty(), false).clicked() { self.drw_delete_interactive(); }
                    });
                });
                group(ui, "Anzeige", |ui| {
                    stack(ui, |ui| {
                        for (st, icon) in [(LineStyle::Wireframe, Icon::DispWire), (LineStyle::Hidden, Icon::DispHidden), (LineStyle::NoHidden, Icon::DispNoHidden)] {
                            if small(ui, icon, st.name(), !views.is_empty(), cur_style.map(|c| c.0 == st).unwrap_or(false)).clicked() { self.set_view_style(&views, Some(st), None); }
                        }
                    });
                    stack(ui, |ui| {
                        let ax = cur_style.map(|c| c.1).unwrap_or(false);
                        if small(ui, Icon::Axis, "Achsen & Mittelpunktmarken", !views.is_empty(), ax).clicked() { self.set_view_style(&views, None, Some(!ax)); }
                    });
                });
                group(ui, "Modell", |ui| {
                    if large(ui, if assembly { Icon::Assembly } else { Icon::Part }, "Modell\nöffnen", true, false).on_hover_text("Das Modell in 3D zeigen; Änderungen aktualisieren die Zeichnung").clicked() { self.drawing_open_model(); }
                    if large(ui, Icon::Regenerate, "Regenerieren", true, false).on_hover_text("Die Ansichten neu berechnen").clicked() { if let Some(d) = &mut self.drw { d.cache.clear(); } self.say("Zeichnung regeneriert.", MsgKind::Success); }
                });
                group(ui, "Format", |ui| {
                    if large(ui, Icon::TitleBlock, "Zeichnungs-\neigenschaften", true, false).on_hover_text("Schriftfeld, Bemaßungsformat, Projektion, Blatt").clicked() { if let Some(d) = &mut self.drw { d.props = true; } }
                });
            }
            Tab::Table => {
                group(ui, "Tabelle", |ui| {
                    let has = self.drw.as_ref().map(|d| d.cur().bom.is_some()).unwrap_or(false);
                    if large(ui, Icon::Bom, "Stückliste\nTabelle", assembly && !has, false).on_hover_text(if assembly { "Die Stücklistentabelle einfügen" } else { "Für Baugruppenzeichnungen verfügbar" }).clicked() { if self.insert_bom() { self.say("Stücklistentabelle über dem Schriftfeld eingefügt.", MsgKind::Success); } }
                    stack(ui, |ui| {
                        if small(ui, Icon::Delete, "Tabelle löschen", has, false).clicked() { if let Some(d) = &mut self.drw { d.sel = vec![DSel::Bom]; } self.drw_delete_interactive(); }
                    });
                });
                group(ui, "Positionsnummern", |ui| {
                    let target = views.first().copied().or_else(|| self.drw.as_ref().and_then(|d| d.cur().views.iter().find(|v| v.kind == ViewKind::General).map(|v| v.id)));
                    if large(ui, Icon::Balloon, "Positions-\nnummern", assembly && target.is_some(), false).on_hover_text("Positionsnummern für jede Stücklistenposition in der gewählten Ansicht").clicked() {
                        if let Some(t) = target { let n = self.create_balloons(t); self.say(format!("{} Positionsnummer(n) erzeugt.", n), MsgKind::Success); }
                    }
                    stack(ui, |ui| {
                        if small(ui, Icon::Delete, "Positionsnummern löschen", assembly, false).clicked() {
                            let si = self.drw.as_ref().map(|d| d.sheet).unwrap_or(0);
                            let vs = views.clone();
                            self.drw_commit(|d| d.sheets[si].balloons.retain(|b| !vs.is_empty() && !vs.contains(&b.view)));
                        }
                    });
                });
            }
            Tab::Annotate => {
                group(ui, "Beschriftungen", |ui| {
                    if large(ui, Icon::ShowAnnot, "Modellbeschrif-\ntungen anzeigen", !assembly && has_views, false).on_hover_text("Die Bemaßungen des Modells in den gewählten Ansichten anzeigen (alle Ansichten, wenn keine gewählt ist)").clicked() {
                        let targets = if views.is_empty() { self.drw.as_ref().map(|d| d.cur().views.iter().map(|v| v.id).collect()).unwrap_or_default() } else { views.clone() };
                        let n = self.show_model_dims(&targets);
                        self.say(format!("{} Modellbemaßung(en) angezeigt.", n), MsgKind::Success);
                    }
                    if large(ui, Icon::DwgDim, "Bemaßung", has_views, matches!(tool, DTool::Dimension { .. })).clicked() { self.set_tool(DTool::Dimension { first: None }); }
                    if large(ui, Icon::Note, "Notiz", true, tool == DTool::Note).clicked() { self.set_tool(DTool::Note); }
                    stack(ui, |ui| {
                        if small(ui, Icon::DimTol, "Bemaßungseigenschaften", self.selected_dim().is_some(), false).on_hover_text("Toleranz und Wert der gewählten Bemaßung").clicked() { self.tab = Tab::Context; }
                        if small(ui, Icon::Cleanup, "Bemaßungen bereinigen", !assembly && has_views, false).clicked() {
                            let targets = if views.is_empty() { self.drw.as_ref().map(|d| d.cur().views.iter().map(|v| v.id).collect()).unwrap_or_default() } else { views.clone() };
                            self.cleanup_dims(&targets);
                        }
                        let any = self.drw.as_ref().map(|d| !d.sel.is_empty()).unwrap_or(false);
                        if small(ui, Icon::Delete, "Entfernen", any, false).clicked() { self.drw_delete_interactive(); }
                    });
                });
                group(ui, "Bezug & GD&T", |ui| {
                    if large(ui, Icon::GeomTol, "Geometrische\nToleranz", has_views, matches!(tool, DTool::Gtol { .. })).on_hover_text("Toleranzrahmen an einer Bemaßung oder mit Hinweislinie").clicked() {
                        match self.selected_dim() { Some(t) => self.add_gtol_on_dim(t), None => self.set_tool(DTool::Gtol { at: None }) }
                    }
                    stack(ui, |ui| {
                        if small(ui, Icon::DatumFeature, "Bezugssymbol", has_views, matches!(tool, DTool::Datum { .. })).clicked() { self.set_tool(DTool::Datum { at: None }); }
                        if small(ui, Icon::SurfFinish, "Oberflächengüte", has_views, tool == DTool::Finish).clicked() { self.set_tool(DTool::Finish); }
                    });
                });
                group(ui, "Format", |ui| {
                    stack(ui, |ui| {
                        let Some(d) = &mut self.drw else { return };
                        let (mut dec, mut th) = (d.data.decimals, d.data.text_height);
                        ui.horizontal(|ui| { ui.label(RichText::new("Dezimalstellen").size(11.0).color(LABEL)); ui.add(egui::DragValue::new(&mut dec).range(0..=4)); });
                        ui.horizontal(|ui| { ui.label(RichText::new("Texthöhe").size(11.0).color(LABEL)); ui.add(egui::DragValue::new(&mut th).range(1.5..=10.0).speed(0.1).suffix(" mm")); });
                        if dec != d.data.decimals || (th - d.data.text_height).abs() > 1e-9 {
                            d.data.decimals = dec;
                            d.data.text_height = th;
                            d.dirty = true;
                        }
                    });
                });
            }
            Tab::Publish => {
                group(ui, "Export", |ui| {
                    if large(ui, Icon::Pdf, "PDF", true, false).on_hover_text("Alle Blätter als Vektor-PDF").clicked() { self.export_drawing("pdf"); }
                    if large(ui, Icon::Dxf, "DXF", true, false).on_hover_text("Aktuelles Blatt als AutoCAD-DXF").clicked() { self.export_drawing("dxf"); }
                    if large(ui, Icon::Svg, "SVG", true, false).on_hover_text("Aktuelles Blatt als SVG").clicked() { self.export_drawing("svg"); }
                });
            }
            Tab::Context => self.annotation_ribbon(ui),
            _ => { self.tab = Tab::Layout; }
        }
    }

    fn set_view_style(&mut self, views: &[u32], style: Option<LineStyle>, axes: Option<bool>) {
        let ids = views.to_vec();
        self.drw_commit(|d| for s in &mut d.sheets { for v in &mut s.views { if ids.contains(&v.id) { if let Some(st) = style { v.style = st; } if let Some(a) = axes { v.axes = a; } } } });
    }

    fn add_sheet(&mut self) {
        let Some(d) = &self.drw else { return };
        let (size, orient, scale) = { let s = d.cur(); (s.size, s.orient, s.scale) };
        self.drw_commit(|dd| { let mut s = Sheet::new(size, orient); s.scale = scale; dd.sheets.push(s); });
        if let Some(d) = &mut self.drw { d.sheet = d.data.sheets.len() - 1; d.sel.clear(); d.fit = true; }
    }

    fn delete_sheet(&mut self) {
        let Some(d) = &self.drw else { return };
        if d.data.sheets.len() < 2 { return; }
        let si = d.sheet;
        self.drw_commit(|dd| { dd.sheets.remove(si); });
        if let Some(d) = &mut self.drw { d.sheet = si.min(d.data.sheets.len() - 1); d.sel.clear(); }
    }

    // ---------------- drawing tree ----------------

    fn drawing_tree(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Zeichnungsbaum").strong().size(13.0));
        ui.add_space(2.0);
        let Some(d) = &self.drw else { return };
        let data = d.data.clone();
        let cur = d.sheet;
        let sel = d.sel.clone();
        let model = d.model_name.clone();
        let assembly = d.assembly;
        let mut click: Option<(usize, Option<u32>)> = None;
        let mut dbl: Option<u32> = None;
        egui::ScrollArea::vertical().id_salt("dwg_tree").max_height(ui.available_height() * 0.55).auto_shrink([false, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let _ = Self::paint_row(ui, 0, Icon::Drawing, &format!("{}.DRW", data.name), TEXT, true, false, None);
            for (si, s) in data.sheets.iter().enumerate() {
                let (r, _) = Self::paint_row(ui, 1, Icon::Sheet, &format!("Blatt {}   {} {}", si + 1, s.size.name(), scale_text(s.scale)), TEXT, si == cur, false, None);
                if r.clicked() { click = Some((si, None)); }
                for v in &s.views {
                    let what = match (v.kind, v.orient) {
                        (ViewKind::Detail, _) => format!("Detail {}", v.letter),
                        (ViewKind::Projection, _) => "Projektion".to_string(),
                        (_, Some(Orient::Default) | None) => "Allgemein".to_string(),
                        (_, Some(o)) => o.name().to_string(),
                    };
                    let sec = if v.section.is_some() { format!(", Schnitt {}-{}", v.letter, v.letter) } else { String::new() };
                    let icon = match v.kind { ViewKind::General => Icon::GeneralView, ViewKind::Projection => Icon::ProjView, ViewKind::Detail => Icon::DetailView };
                    let (r, _) = Self::paint_row(ui, 2, icon, &format!("{}  ({}{})", v.name, what, sec), TEXT, false, sel.contains(&DSel::View(v.id)), None);
                    if r.clicked() { click = Some((si, Some(v.id))); }
                    if r.double_clicked() { dbl = Some(v.id); }
                }
            }
        });
        ui.separator();
        ui.label(RichText::new("Modellbaum").strong().size(13.0));
        egui::ScrollArea::vertical().id_salt("dwg_model_tree").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if assembly {
                let _ = Self::paint_row(ui, 0, Icon::Assembly, &format!("{}.ASM", model), TEXT, true, false, None);
                let comps: Vec<String> = self.asm.as_ref().map(|a| a.data.components.iter().filter(|c| !c.suppressed).map(|c| format!("{}.PRT", c.part)).collect()).unwrap_or_default();
                for c in comps { let _ = Self::paint_row(ui, 1, Icon::Part, &c, TEXT, false, false, None); }
            } else {
                let _ = Self::paint_row(ui, 0, Icon::Part, &format!("{}.PRT", model), TEXT, true, false, None);
                for f in self.data.features.iter().filter(|f| !f.suppressed) { let _ = Self::paint_row(ui, 1, icons::for_feature(&f.kind), &f.name, TEXT, false, false, None); }
            }
        });
        if let Some(d) = &mut self.drw {
            if let Some((si, v)) = click {
                if d.sheet != si { d.sheet = si; d.fit = true; }
                d.sel = v.map(|id| vec![DSel::View(id)]).unwrap_or_default();
            }
        }
        if let Some(id) = dbl { self.open_view_dialog(id, false); }
    }

    // ---------------- canvas ----------------

    fn drawing_canvas(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_rect_before_wrap();
        let tabs_h = 24.0;
        let rect = Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - tabs_h));
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        super::ui::no_hand(ui.ctx(), resp.id);
        let Some(d) = &mut self.drw else { return };
        let si = d.sheet.min(d.data.sheets.len() - 1);
        d.sheet = si;
        let (sw, sh) = d.data.sheets[si].dims_mm();
        if d.fit {
            d.cam.zoom = ((rect.width() as f64 / sw).min(rect.height() as f64 / sh) * 0.93) as f32;
            d.cam.center = V2::new(sw / 2.0, sh / 2.0);
            d.fit = false;
        }
        // navigation: wheel / pinch zoom at the cursor, ⇧ scroll / middle / right drag pans
        let (scroll, zoomd, mods, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers, i.pointer.clone()));
        let hover = resp.hover_pos();
        let to_sheet = |cam: SheetCam, p: Pos2| V2::new(cam.center.x + (p.x - rect.center().x) as f64 / cam.zoom as f64, cam.center.y - (p.y - rect.center().y) as f64 / cam.zoom as f64);
        if let Some(hp) = hover {
            let mut factor = zoomd as f64;
            if !mods.shift && scroll.y != 0.0 { factor *= (scroll.y as f64 * 0.0025).exp(); }
            if mods.shift && (scroll.x != 0.0 || scroll.y != 0.0) { d.cam.center -= V2::new(-scroll.x as f64, scroll.y as f64) / d.cam.zoom as f64; }
            if factor != 1.0 {
                let before = to_sheet(d.cam, hp);
                d.cam.zoom = (d.cam.zoom as f64 * factor).clamp(0.05, 80.0) as f32;
                let after = to_sheet(d.cam, hp);
                d.cam.center += before - after;
            }
        }
        if resp.dragged() && (pointer.middle_down() || pointer.secondary_down()) {
            let dl = pointer.delta();
            d.cam.center -= V2::new(dl.x as f64, -dl.y as f64) / d.cam.zoom as f64;
        }
        let cam = d.cam;
        let zoom = cam.zoom as f64;
        let to_s = move |p: V2| pos2(rect.center().x + ((p.x - cam.center.x) * zoom) as f32, rect.center().y - ((p.y - cam.center.y) * zoom) as f32);
        let mouse = hover.map(|p| to_sheet(cam, p));
        d.pointer = mouse;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(158, 168, 182));
        let paper = Rect::from_two_pos(to_s(V2::new(0.0, sh)), to_s(V2::new(sw, 0.0)));
        painter.rect_filled(paper.translate(vec2(4.0, 4.0)), 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 60));
        painter.rect_filled(paper, 0.0, Color32::WHITE);
        // highlight colours
        let mut hl: Vec<(DSel, Rgb)> = d.sel.iter().map(|s| (*s, SELC)).collect();
        if let Some(h) = d.hover { if !d.sel.contains(&h) { hl.push((h, HOVC)); } }
        let tool = d.tool.clone();
        let (prims, hits) = self.sheet_scene(si, &hl);
        paint_prims(&painter, &prims, &to_s, zoom);
        let Some(d) = &mut self.drw else { return };
        d.hits = hits.clone();
        let hit_at = |p: V2| hits.iter().rev().find(|h| p.x >= h.min.x && p.y >= h.min.y && p.x <= h.max.x && p.y <= h.max.y).map(|h| h.sel);
        // selected views get a frame
        for h in &hits {
            if let DSel::View(_) = h.sel {
                if d.sel.contains(&h.sel) || d.hover == Some(h.sel) {
                    let r = Rect::from_two_pos(to_s(h.min), to_s(h.max));
                    let col = if d.sel.contains(&h.sel) { Color32::from_rgb(20, 160, 60) } else { Color32::from_rgb(30, 120, 230) };
                    painter.extend(Shape::dashed_line(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], Stroke::new(1.0_f32, col), 5.0, 3.0));
                }
            }
        }
        // hover and tool previews
        d.hover = if tool == DTool::None { mouse.and_then(hit_at) } else { None };
        let tol = 8.0 / zoom;
        let mut action: Option<Box<dyn FnOnce(&mut App)>> = None;
        match tool.clone() {
            DTool::Dimension { first } => {
                let snap = mouse.and_then(|m| self.snap_at(m, tol));
                if let Some(s) = &snap {
                    let c = Color32::from_rgb(30, 120, 230);
                    match s.kind {
                        SnapKind::Point => { painter.circle_stroke(to_s(s.sheet), 5.0, Stroke::new(2.0_f32, c)); }
                        SnapKind::Circle { c: cc, r } => { painter.circle_stroke(to_s(cc), (r * zoom) as f32, Stroke::new(2.0_f32, c)); }
                        SnapKind::Edge { a, b } => { painter.line_segment([to_s(a), to_s(b)], Stroke::new(2.5_f32, c)); }
                    }
                }
                if let (Some(f), Some(m)) = (first, mouse) {
                    let c = Color32::from_rgb(20, 160, 60);
                    match f.kind { SnapKind::Point => { painter.circle_stroke(to_s(f.sheet), 5.0, Stroke::new(2.0_f32, c)); } SnapKind::Circle { c: cc, r } => { painter.circle_stroke(to_s(cc), (r * zoom) as f32, Stroke::new(2.0_f32, c)); } SnapKind::Edge { a, b } => { painter.line_segment([to_s(a), to_s(b)], Stroke::new(2.5_f32, c)); } }
                    // live preview of the dimension being placed
                    let h = self.drw.as_ref().map(|d| d.data.text_height).unwrap_or(3.5);
                    let mut pv = vec![];
                    match f.kind {
                        SnapKind::Circle { c: cc, r } => { radial_dim(&mut pv, cc, r, m, &DimLabel::plain("⌀"), h, HOVC); }
                        SnapKind::Edge { a, b } => { let ty = Self::dim_type_for(a, b, m); let u = match ty { DimType::Horizontal => V2::X, DimType::Vertical => V2::Y, _ => safe_norm2(b - a, V2::X) }; linear_dim(&mut pv, a, b, u, m, &DimLabel::plain(""), h, HOVC); }
                        SnapKind::Point => {}
                    }
                    paint_prims(&painter, &pv, &to_s, zoom);
                }
                if resp.clicked_by(egui::PointerButton::Primary) {
                    if let Some(m) = mouse {
                        match (first, snap) {
                            (None, Some(s)) => { if let Some(d) = &mut self.drw { d.tool = DTool::Dimension { first: Some(s) }; } }
                            (Some(f), s) if matches!(f.kind, SnapKind::Circle { .. } | SnapKind::Edge { .. }) => { let _ = s; action = Some(Box::new(move |app: &mut App| { app.place_dimension(f, None, m); if let Some(d) = &mut app.drw { d.tool = DTool::Dimension { first: None }; } })); }
                            (Some(f), Some(s)) if s.view == f.view && (s.sheet - f.sheet).length() > 1e-6 && s.kind == SnapKind::Point => {
                                // second point: the next click places it
                                let pending = Snap { b: Some(s.a), kind: SnapKind::Edge { a: f.sheet, b: s.sheet }, ..f };
                                if let Some(d) = &mut self.drw { d.tool = DTool::Dimension { first: Some(pending) }; }
                            }
                            _ => {}
                        }
                    }
                }
            }
            DTool::General => {
                if let Some(m) = mouse { let p = to_s(m); painter.line_segment([p - vec2(10.0, 0.0), p + vec2(10.0, 0.0)], Stroke::new(1.0_f32, Color32::from_rgb(30, 120, 230))); painter.line_segment([p - vec2(0.0, 10.0), p + vec2(0.0, 10.0)], Stroke::new(1.0_f32, Color32::from_rgb(30, 120, 230))); }
                if resp.clicked_by(egui::PointerButton::Primary) {
                    if let Some(m) = mouse {
                        action = Some(Box::new(move |app: &mut App| {
                            let si = app.drw.as_ref().map(|d| d.sheet).unwrap_or(0);
                            let mut id = 0;
                            app.drw_commit(|d| {
                                id = d.take_id();
                                let mut v = DrawView::new(id, ViewKind::General, Orient::Default.rotation(), m);
                                v.orient = Some(Orient::Default);
                                v.style = LineStyle::NoHidden;
                                v.axes = false;
                                d.sheets[si].views.push(v);
                            });
                            if let Some(d) = &mut app.drw { d.tool = DTool::None; d.sel = vec![DSel::View(id)]; }
                            app.open_view_dialog(id, true);
                        }));
                    }
                }
            }
            DTool::Projection { parent } => {
                match parent {
                    None => if resp.clicked_by(egui::PointerButton::Primary) {
                        if let Some(DSel::View(id)) = mouse.and_then(hit_at) {
                            let ok = self.drw.as_ref().and_then(|d| d.data.view(id)).map(|v| v.kind != ViewKind::Detail).unwrap_or(false);
                            if ok { action = Some(Box::new(move |app: &mut App| app.set_tool(DTool::Projection { parent: Some(id) }))); }
                        }
                    },
                    Some(pid) => {
                        let pv = self.drw.as_ref().and_then(|d| d.data.view(pid)).cloned();
                        let pg = self.view_geom(pid);
                        if let (Some(pv), Some(pg), Some(m)) = (pv, pg, mouse) {
                            let side = ProjSide::from_offset(m - pv.pos);
                            let at = if side.horizontal() { V2::new(m.x, pv.pos.y) } else { V2::new(pv.pos.x, m.y) };
                            let (lo, hi) = pg.rect(pv.pos);
                            let half = (hi - lo) * 0.5;
                            let r = Rect::from_two_pos(to_s(at - half), to_s(at + half));
                            painter.extend(Shape::dashed_line(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], Stroke::new(1.2_f32, Color32::from_rgb(30, 120, 230)), 6.0, 3.0));
                            if resp.clicked_by(egui::PointerButton::Primary) {
                                action = Some(Box::new(move |app: &mut App| {
                                    let si = app.drw.as_ref().map(|d| d.sheet).unwrap_or(0);
                                    let proj = app.drw.as_ref().map(|d| d.data.projection).unwrap_or(Projection::Third);
                                    let mut id = 0;
                                    app.drw_commit(|d| {
                                        id = d.take_id();
                                        let mut v = DrawView::new(id, ViewKind::Projection, projection_rotation(pv.rot, side, proj), at);
                                        v.parent = Some(pid);
                                        v.side = Some(side);
                                        v.style = if pv.style == LineStyle::NoHidden && pv.orient == Some(Orient::Default) { LineStyle::NoHidden } else { pv.style };
                                        v.axes = true;
                                        v.exploded = pv.exploded;
                                        d.sheets[si].views.push(v);
                                    });
                                    if let Some(d) = &mut app.drw { d.tool = DTool::None; d.sel = vec![DSel::View(id)]; }
                                }));
                            }
                        }
                    }
                }
            }
            DTool::Detail { parent, center, radius } => {
                let c = Color32::from_rgb(30, 120, 230);
                match (parent, center, radius) {
                    (None, _, _) => if resp.clicked_by(egui::PointerButton::Primary) {
                        if let (Some(DSel::View(id)), Some(m)) = (mouse.and_then(hit_at), mouse) {
                            if let (Some(pv), Some(pg)) = (self.drw.as_ref().and_then(|d| d.data.view(id)).cloned(), self.view_geom(id)) {
                                if pv.kind != ViewKind::Detail {
                                    let cm = pg.from_sheet(pv.pos, m);
                                    action = Some(Box::new(move |app: &mut App| { if let Some(d) = &mut app.drw { d.tool = DTool::Detail { parent: Some(id), center: Some(cm), radius: None }; } app.say("Klicken, um die Größe des Detailbereichs festzulegen.", MsgKind::Prompt); }));
                                }
                            }
                        }
                    },
                    (Some(pid), Some(cm), None) => {
                        if let (Some(pv), Some(pg), Some(m)) = (self.drw.as_ref().and_then(|d| d.data.view(pid)).cloned(), self.view_geom(pid), mouse) {
                            let cs = pg.to_sheet(pv.pos, cm);
                            let r = (m - cs).length();
                            painter.circle_stroke(to_s(cs), (r * zoom) as f32, Stroke::new(1.5_f32, c));
                            if resp.clicked_by(egui::PointerButton::Primary) && r > 1.0 {
                                let rm = r / pg.scale;
                                action = Some(Box::new(move |app: &mut App| { if let Some(d) = &mut app.drw { d.tool = DTool::Detail { parent: Some(pid), center: Some(cm), radius: Some(rm) }; } app.say("MITTELPUNKT für die Detailansicht wählen.", MsgKind::Prompt); }));
                            }
                        }
                    }
                    (Some(pid), Some(cm), Some(rm)) => {
                        let scale = self.drw.as_ref().map(|d| d.cur().view_scale(pid)).unwrap_or(1.0);
                        let ds = SCALES.iter().rev().copied().find(|s| *s >= scale * 2.0 - 1e-9).unwrap_or(scale * 2.0);
                        if let Some(m) = mouse {
                            painter.circle_stroke(to_s(m), (rm * ds * zoom) as f32, Stroke::new(1.2_f32, c));
                            if resp.clicked_by(egui::PointerButton::Primary) {
                                action = Some(Box::new(move |app: &mut App| {
                                    let si = app.drw.as_ref().map(|d| d.sheet).unwrap_or(0);
                                    let mut id = 0;
                                    app.drw_commit(|d| {
                                        id = d.take_id();
                                        let letter = d.next_letter();
                                        let prot = d.view(pid).map(|v| v.rot).unwrap_or_default();
                                        let mut v = DrawView::new(id, ViewKind::Detail, prot, m);
                                        v.parent = Some(pid);
                                        v.detail = Some(Detail { center: cm, radius: rm });
                                        v.scale = Some(ds);
                                        v.letter = letter;
                                        v.style = d.view(pid).map(|p| p.style).unwrap_or(LineStyle::Hidden);
                                        d.sheets[si].views.push(v);
                                    });
                                    if let Some(d) = &mut app.drw { d.tool = DTool::None; d.sel = vec![DSel::View(id)]; }
                                }));
                            }
                        }
                    }
                    _ => {}
                }
            }
            DTool::Datum { .. } | DTool::Gtol { .. } | DTool::Finish => {
                if let Some(m) = mouse { action = self.gdt_tool_frame(&painter, &to_s, zoom, m, &hits, resp.clicked_by(egui::PointerButton::Primary)); }
            }
            DTool::Note => {
                if resp.clicked_by(egui::PointerButton::Primary) {
                    if let (Some(m), Some(hp)) = (mouse, hover) {
                        action = Some(Box::new(move |app: &mut App| {
                            let si = app.drw.as_ref().map(|d| d.sheet).unwrap_or(0);
                            let mut id = 0;
                            app.drw_commit(|d| { id = d.take_id(); d.sheets[si].notes.push(Note { id, pos: m, text: "HINWEIS".into(), leader: None }); });
                            if let Some(d) = &mut app.drw { d.tool = DTool::None; d.sel = vec![DSel::Note(id)]; d.edit = Some(TextEdit { sel: DSel::Note(id), text: String::new(), pos: hp, focus: true, multiline: true }); }
                        }));
                    }
                }
            }
            DTool::None => {
                let Some(d) = &mut self.drw else { return };
                if resp.drag_started_by(egui::PointerButton::Primary) {
                    if let Some(m) = mouse {
                        if let Some(h) = hit_at(m) {
                            if !d.sel.contains(&h) { d.sel = vec![h]; }
                            let sh = d.cur();
                            let movable = match h {
                                DSel::View(_) => !d.lock_views,
                                DSel::Finish(_) => false,
                                DSel::Gtol(id) => sh.gtols.iter().any(|g| g.id == id && matches!(g.target, Attach::Geom(_))),
                                _ => true,
                            };
                            if movable {
                                d.undo.push(d.data.clone());
                                d.redo.clear();
                                d.drag = Some((h, m));
                            }
                        }
                    }
                }
                if resp.dragged_by(egui::PointerButton::Primary) {
                    if let (Some((s, last)), Some(m)) = (d.drag, mouse) {
                        let delta = m - last;
                        d.drag = Some((s, m));
                        d.dirty = true;
                        action = Some(Box::new(move |app: &mut App| app.drag_item(s, delta)));
                    }
                }
                if resp.drag_stopped() { d.drag = None; }
                if resp.clicked_by(egui::PointerButton::Primary) {
                    let add = mods.shift || mods.command;
                    match mouse.and_then(hit_at) {
                        Some(h) => { if add { if let Some(i) = d.sel.iter().position(|x| *x == h) { d.sel.remove(i); } else { d.sel.push(h); } } else { d.sel = vec![h]; } }
                        None => if !add { d.sel.clear(); },
                    }
                }
                if resp.double_clicked() {
                    if let (Some(m), Some(hp)) = (mouse, hover) {
                        match hit_at(m) {
                            Some(DSel::View(id)) => action = Some(Box::new(move |app: &mut App| app.open_view_dialog(id, false))),
                            Some(s @ (DSel::Shown(_) | DSel::Dim(_) | DSel::Note(_))) => action = Some(Box::new(move |app: &mut App| app.start_edit(s, hp))),
                            _ => {}
                        }
                    }
                }
            }
        }
        if resp.secondary_clicked() {
            let h = mouse.and_then(hit_at);
            ui.ctx().data_mut(|dm| dm.insert_temp(egui::Id::new("dwg_ctx"), h));
            if let (Some(h), Some(d)) = (h, &mut self.drw) { if !d.sel.contains(&h) { d.sel = vec![h]; } }
        }
        resp.context_menu(|ui| self.drawing_context_menu(ui));
        if let Some(a) = action { a(self); }
        // sheet tabs
        let tab_rect = Rect::from_min_max(pos2(full.min.x, full.max.y - tabs_h), full.max);
        let mut ui2 = ui.new_child(egui::UiBuilder::new().max_rect(tab_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        ui2.painter().rect_filled(tab_rect, 0.0, Color32::from_rgb(232, 236, 241));
        let Some(d) = &self.drw else { return };
        let n = d.data.sheets.len();
        let cur = d.sheet;
        let mut switch = None;
        let mut add = false;
        ui2.add_space(6.0);
        for i in 0..n {
            let r = ui2.add(egui::Button::new(RichText::new(format!("Blatt {}", i + 1)).size(12.0)).selected(i == cur));
            if r.clicked() { switch = Some(i); }
        }
        if ui2.add(egui::Button::new(RichText::new("+").size(13.0))).on_hover_text("Neues Blatt").clicked() { add = true; }
        if let Some(i) = switch { if let Some(d) = &mut self.drw { d.sheet = i; d.sel.clear(); d.fit = true; } }
        if add { self.add_sheet(); }
        // in-place text editor
        self.drawing_text_editor(ui.ctx());
    }

    fn drawing_text_editor(&mut self, ctx: &egui::Context) {
        let Some(d) = &mut self.drw else { return };
        let Some(te) = &mut d.edit else { return };
        let (mut commit, mut cancel) = (false, false);
        let mut text = te.text.clone();
        let focus = te.focus;
        let multiline = te.multiline;
        egui::Area::new(egui::Id::new("dwg_text_edit")).fixed_pos(te.pos - vec2(60.0, 12.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(4.0).show(ui, |ui| {
                let r = if multiline { ui.add(egui::TextEdit::multiline(&mut text).desired_width(220.0).desired_rows(3)) } else { ui.add(egui::TextEdit::singleline(&mut text).desired_width(140.0)) };
                if focus { r.request_focus(); }
                ui.horizontal(|ui| {
                    if ui.small_button("OK").clicked() { commit = true; }
                    if ui.small_button("Abbrechen").clicked() { cancel = true; }
                    if multiline { ui.label(RichText::new("⌘↩ zum Abschließen").size(10.5).color(LABEL)); }
                });
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                let cmd = ui.input(|i| i.modifiers.command);
                if r.lost_focus() && enter && !multiline { commit = true; }
                if multiline && enter && cmd { commit = true; }
            });
        });
        te.text = text.clone();
        te.focus = false;
        let sel = te.sel;
        if commit || cancel { d.edit = None; }
        if commit { self.commit_edit(sel, text); }
    }

    fn drawing_context_menu(&mut self, ui: &mut egui::Ui) {
        menu_spacing(ui);
        let h: Option<DSel> = ui.ctx().data(|d| d.get_temp(egui::Id::new("dwg_ctx"))).flatten();
        let item = |ui: &mut egui::Ui, icon: Icon, label: &str| menu_entry(ui, icon, label, true, false).clicked();
        match h {
            Some(DSel::View(id)) => {
                if item(ui, Icon::EditDef, "Eigenschaften") { self.open_view_dialog(id, false); ui.close_menu(); }
                let detail = self.drw.as_ref().and_then(|d| d.data.view(id)).map(|v| v.kind == ViewKind::Detail).unwrap_or(true);
                if !detail && item(ui, Icon::ProjView, "Projektionsansicht") { self.set_tool(DTool::Projection { parent: Some(id) }); ui.close_menu(); }
                if !detail && item(ui, Icon::ShowAnnot, "Modellbeschriftungen anzeigen") { let n = self.show_model_dims(&[id]); self.say(format!("{} Modellbemaßung(en) angezeigt.", n), MsgKind::Success); ui.close_menu(); }
                if item(ui, Icon::Lock, "Ansichtsbewegung sperren") { if let Some(d) = &mut self.drw { d.lock_views = !d.lock_views; } ui.close_menu(); }
                ui.separator();
                if item(ui, Icon::Delete, "Löschen") { self.drw_delete_interactive(); ui.close_menu(); }
            }
            Some(s @ (DSel::Shown(_) | DSel::Dim(_) | DSel::Note(_))) => {
                let pos = ui.ctx().input(|i| i.pointer.interact_pos()).unwrap_or(Pos2::ZERO);
                if item(ui, Icon::SkModify, if matches!(s, DSel::Shown(_)) { "Nennwert ändern" } else { "Text editieren" }) { self.start_edit(s, pos); ui.close_menu(); }
                if item(ui, Icon::Delete, if matches!(s, DSel::Shown(_)) { "Entfernen" } else { "Löschen" }) { self.drw_delete_interactive(); ui.close_menu(); }
            }
            Some(_) => { if item(ui, Icon::Delete, "Löschen") { self.drw_delete_interactive(); ui.close_menu(); } }
            None => {
                if item(ui, Icon::GeneralView, "Allgemeine Ansicht") { self.set_tool(DTool::General); ui.close_menu(); }
                if item(ui, Icon::Note, "Notiz") { self.set_tool(DTool::Note); ui.close_menu(); }
                if item(ui, Icon::TitleBlock, "Zeichnungseigenschaften") { if let Some(d) = &mut self.drw { d.props = true; } ui.close_menu(); }
                if item(ui, Icon::Refit, "Blatt einpassen") { if let Some(d) = &mut self.drw { d.fit = true; } ui.close_menu(); }
            }
        }
    }

    // ---------------- dialogs ----------------

    fn drawing_dialogs(&mut self, ctx: &egui::Context) {
        self.view_dialog_ui(ctx);
        self.props_dialog_ui(ctx);
    }

    fn view_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(mut vd) = self.drw.as_mut().and_then(|d| d.view_dialog.take()) else { return };
        let planes: Vec<(u64, String)> = {
            let m = self.drawing_model(vd.draft.exploded);
            let z = vd.draft.rot * V3::Z;
            m.planes.iter().filter(|p| p.frame.z().cross(z).length() < 1e-6).map(|p| (p.id, p.name.clone())).collect()
        };
        let sheet_scale = self.drw.as_ref().map(|d| d.cur().scale).unwrap_or(1.0);
        let assembly = self.drw.as_ref().map(|d| d.assembly).unwrap_or(false);
        let (mut ok, mut cancel, mut apply) = (false, false, false);
        let mut x_open = true;
        egui::Window::new("Zeichnungsansicht").title_bar(false).collapsible(false).resizable(false).default_pos(pos2(320.0, 180.0)).show(ctx, |ui| {
            if super::creo::dialog_header(ui, "Zeichnungsansicht") { x_open = false; }
            ui.set_width(440.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(120.0);
                    ui.label(RichText::new("Kategorien").size(11.0).color(LABEL));
                    for (i, n) in ["Ansichtstyp", "Maßstab", "Schnitte", "Ansichtsanzeige"].iter().enumerate() {
                        if ui.selectable_label(vd.cat == i as u8, *n).clicked() { vd.cat = i as u8; }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    let v = &mut vd.draft;
                    match vd.cat {
                        0 => {
                            ui.horizontal(|ui| { ui.label("Ansichtsname"); ui.add(egui::TextEdit::singleline(&mut v.name).desired_width(160.0)); });
                            ui.label(RichText::new(format!("Typ: {}", match v.kind { ViewKind::General => "Allgemein", ViewKind::Projection => "Projektion", ViewKind::Detail => "Detail" })).size(11.5).color(LABEL));
                            if v.kind == ViewKind::General {
                                ui.add_space(4.0);
                                ui.label("Ansichtsausrichtung — gespeicherte Ansichten:");
                                egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                                    for o in Orient::ALL {
                                        if ui.selectable_label(v.orient == Some(o), o.name()).clicked() {
                                            v.orient = Some(o);
                                            v.rot = o.rotation();
                                            if o.pictorial() { v.style = LineStyle::NoHidden; v.axes = false; } else if v.style == LineStyle::NoHidden { v.style = LineStyle::Hidden; v.axes = true; }
                                        }
                                    }
                                });
                            } else if v.kind == ViewKind::Projection {
                                ui.label(RichText::new(format!("Aus der übergeordneten Ansicht projiziert ({} Seite)", v.side.unwrap_or(ProjSide::Right).label())).size(11.5));
                            } else if let Some(det) = &mut v.detail {
                                ui.horizontal(|ui| { ui.label("Begrenzungsradius (Modell)"); ui.add(egui::DragValue::new(&mut det.radius).range(0.5..=1e5).speed(0.5)); });
                                ui.horizontal(|ui| { ui.label("Beschriftung"); ui.add(egui::TextEdit::singleline(&mut v.letter).desired_width(40.0)); });
                            }
                        }
                        1 => {
                            if v.kind == ViewKind::Projection {
                                ui.label("Projektionsansichten verwenden den Maßstab ihrer übergeordneten Ansicht.");
                            } else {
                                if ui.radio(v.scale.is_none(), format!("Standardmaßstab des Blatts ({})", scale_text(sheet_scale))).clicked() { v.scale = None; }
                                let custom = v.scale.is_some();
                                ui.horizontal(|ui| {
                                    if ui.radio(custom, "Benutzerdefinierter Maßstab").clicked() && !custom { v.scale = Some(sheet_scale); vd.custom_scale = scale_text(sheet_scale); }
                                    let r = ui.add_enabled(custom, egui::TextEdit::singleline(&mut vd.custom_scale).desired_width(80.0));
                                    if r.changed() { if let Some(s) = parse_scale(&vd.custom_scale) { v.scale = Some(s); } }
                                });
                                ui.label(RichText::new("z. B. 2:1, 1:5 oder 0.25").size(10.5).color(LABEL));
                            }
                        }
                        2 => {
                            if v.kind == ViewKind::Detail { ui.label("Detailansichten zeigen den Schnitt ihrer übergeordneten Ansicht."); }
                            else {
                                if ui.radio(v.section.is_none(), "Kein Schnitt").clicked() { v.section = None; }
                                if ui.radio(v.section.is_some(), "2D-Querschnitt").clicked() && v.section.is_none() {
                                    v.section = planes.first().map(|p| p.0);
                                    if v.letter.is_empty() { v.letter = self.drw.as_ref().map(|d| d.data.next_letter()).unwrap_or("A".into()); }
                                }
                                if v.section.is_some() {
                                    egui::ComboBox::from_label("Durch Bezugsebene").selected_text(planes.iter().find(|p| Some(p.0) == v.section).map(|p| p.1.clone()).unwrap_or("—".into())).show_ui(ui, |ui| {
                                        for (id, n) in &planes { ui.selectable_value(&mut v.section, Some(*id), n); }
                                    });
                                    ui.horizontal(|ui| { ui.label("Name"); ui.add(egui::TextEdit::singleline(&mut v.letter).desired_width(40.0)); });
                                    if planes.is_empty() { ui.label(RichText::new("Keine Bezugsebene liegt parallel zu dieser Ansicht.").color(Color32::from_rgb(200, 36, 28))); }
                                    else { ui.label(RichText::new("Material vor der Ebene wird entfernt; Schnittflächen werden schraffiert. Die übergeordnete Ansicht zeigt den Schnittverlauf.").size(10.5).color(LABEL)); }
                                }
                            }
                        }
                        _ => {
                            egui::ComboBox::from_label("Anzeigestil").selected_text(v.style.name()).show_ui(ui, |ui| {
                                for s in LineStyle::ALL { ui.selectable_value(&mut v.style, s, s.name()); }
                            });
                            ui.checkbox(&mut v.axes, "Achsen und Mittelpunktmarken anzeigen");
                            if assembly { ui.checkbox(&mut v.exploded, "Komponenten in der Ansicht explodieren"); }
                        }
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Abbrechen").clicked() { cancel = true; }
                if ui.button("Anwenden").clicked() { apply = true; }
                if ui.button("  OK  ").clicked() { ok = true; }
            }));
        });
        let id = vd.view;
        if ok || apply {
            let draft = vd.draft.clone();
            let si = self.drw.as_ref().and_then(|d| d.data.sheet_of(id)).unwrap_or(0);
            // the orientation of a general view carries over to its projections
            let proj = self.drw.as_ref().map(|d| d.data.projection).unwrap_or(Projection::Third);
            self.drw_commit(|d| {
                let old_rot = d.sheets[si].view(id).map(|v| v.rot);
                if let Some(v) = d.sheets[si].view_mut(id) { *v = draft.clone(); }
                if old_rot != Some(draft.rot) { reproject_children(&mut d.sheets[si], id, proj); }
            });
            vd.is_new = false;
        }
        if cancel && vd.is_new {
            if let Some(d) = &mut self.drw {
                if let Some(si) = d.data.sheet_of(id) { d.data.sheets[si].views.retain(|v| v.id != id); }
                d.undo.pop();
                d.sel.clear();
            }
        }
        if !(ok || cancel || !x_open) { if let Some(d) = &mut self.drw { d.view_dialog = Some(vd); } }
    }

    fn props_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(d) = &mut self.drw else { return };
        if !d.props { return; }
        let mut data = d.data.clone();
        let si = d.sheet;
        let (mut ok, mut cancel) = (false, false);
        let mut x_open = true;
        egui::Window::new("Zeichnungseigenschaften").title_bar(false).collapsible(false).resizable(false).default_pos(pos2(360.0, 160.0)).show(ctx, |ui| {
            if super::creo::dialog_header(ui, "Zeichnungseigenschaften") { x_open = false; }
            ui.set_width(380.0);
            ui.label(RichText::new("Schriftfeld").strong());
            egui::Grid::new("tb").num_columns(2).show(ui, |ui| {
                for (n, v) in [("Benennung", &mut data.title), ("Firma", &mut data.company), ("Gezeichnet", &mut data.drawn_by), ("Werkstoff", &mut data.material), ("Allgemeintoleranz", &mut data.general_tol), ("Datum", &mut data.date), ("Zeichnungsnummer", &mut data.name)] {
                    ui.label(n);
                    ui.add(egui::TextEdit::singleline(v).desired_width(220.0));
                    ui.end_row();
                }
            });
            ui.separator();
            ui.label(RichText::new("Detailoptionen").strong());
            ui.horizontal(|ui| { ui.label("Dezimalstellen"); ui.add(egui::DragValue::new(&mut data.decimals).range(0..=4)); ui.label("Toleranz"); ui.add(egui::DragValue::new(&mut data.tol_decimals).range(1..=5)); ui.label("Texthöhe"); ui.add(egui::DragValue::new(&mut data.text_height).range(1.5..=10.0).speed(0.1).suffix(" mm")); });
            ui.horizontal(|ui| {
                ui.label("Projektion");
                ui.radio_value(&mut data.projection, Projection::Third, "Projektionsmethode 3 (ANSI/ISO A)");
                ui.radio_value(&mut data.projection, Projection::First, "Projektionsmethode 1 (DIN/ISO E)");
            });
            ui.separator();
            ui.label(RichText::new(format!("Blatt {}", si + 1)).strong());
            let s = &mut data.sheets[si];
            ui.horizontal(|ui| {
                egui::ComboBox::from_label("Format").selected_text(s.size.name()).show_ui(ui, |ui| { for z in SheetSize::ALL { ui.selectable_value(&mut s.size, z, z.name()); } });
                ui.radio_value(&mut s.orient, Orientation::Landscape, "Querformat");
                ui.radio_value(&mut s.orient, Orientation::Portrait, "Hochformat");
            });
            egui::ComboBox::from_label("Blattmaßstab").selected_text(scale_text(s.scale)).show_ui(ui, |ui| { for sc in SCALES { ui.selectable_value(&mut s.scale, sc, scale_text(sc)); } });
            ui.separator();
            ui.horizontal(|ui| ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Abbrechen").clicked() { cancel = true; }
                if ui.button("  OK  ").clicked() { ok = true; }
            }));
        });
        if ok {
            let proj_changed = data.projection != self.drw.as_ref().map(|d| d.data.projection).unwrap_or(Projection::Third);
            self.drw_commit(|d| {
                *d = data;
                if proj_changed {
                    let p = d.projection;
                    for s in &mut d.sheets {
                        let roots: Vec<u32> = s.views.iter().filter(|v| v.kind == ViewKind::General).map(|v| v.id).collect();
                        for r in roots { reproject_children(s, r, p); }
                    }
                }
            });
        }
        if ok || cancel || !x_open { if let Some(d) = &mut self.drw { d.props = false; d.fit = true; } }
    }
}

/// Recomputes the rotations of the projection views below `id` after its orientation changed.
fn reproject_children(s: &mut Sheet, id: u32, proj: Projection) {
    let Some(prot) = s.view(id).map(|v| v.rot) else { return };
    let kids: Vec<u32> = s.views.iter().filter(|v| v.parent == Some(id)).map(|v| v.id).collect();
    for k in kids {
        if let Some(v) = s.view_mut(k) {
            match (v.kind, v.side) {
                (ViewKind::Projection, Some(side)) => v.rot = projection_rotation(prot, side, proj),
                (ViewKind::Detail, _) => v.rot = prot,
                _ => {}
            }
        }
        reproject_children(s, k, proj);
    }
}

#[allow(dead_code)]
fn unused_align() -> Align2 { Align2::CENTER_CENTER }

impl App {
    /// Creo's New Drawing dialog: default model, template, orientation, size, projection.
    pub fn new_drawing_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut nd) = self.windows.new_drawing.take() else { return };
        let (assembly, model) = self.model_kind_name();
        let has_geometry = !self.result.topo.polys.is_empty();
        let (mut keep, mut ok, mut browse) = (true, false, false);
        let mut x_open = true;
        egui::Window::new("Neue Zeichnung").title_bar(false).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -60.0)).show(ctx, |ui| {
            if super::creo::dialog_header(ui, "Neue Zeichnung") { x_open = false; }
            ui.set_width(360.0);
            ui.label(RichText::new("Standardmodell").strong());
            ui.horizontal(|ui| {
                ui.label(format!("{}.{}", model, if assembly { "ASM" } else { "PRT" }));
                if ui.button("Durchsuchen...").clicked() { browse = true; }
            });
            if !has_geometry { ui.label(RichText::new("Das Modell hat noch keine Volumengeometrie.").size(11.0).color(Color32::from_rgb(190, 120, 0))); }
            ui.separator();
            ui.label(RichText::new("Schablone festlegen").strong());
            ui.radio_value(&mut nd.template, true, "Schablone verwenden (Vorder-, Drauf-, Seiten- und räumliche Ansicht)");
            ui.radio_value(&mut nd.template, false, "Leer mit Format");
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Ausrichtung").strong());
                ui.radio_value(&mut nd.orient, Orientation::Landscape, "Querformat");
                ui.radio_value(&mut nd.orient, Orientation::Portrait, "Hochformat");
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Format").strong());
                egui::ComboBox::from_id_salt("nd_size").selected_text(nd.size.name()).show_ui(ui, |ui| {
                    for s in SheetSize::ALL { let (w, h) = s.landscape(); ui.selectable_value(&mut nd.size, s, format!("{}  ({} × {} mm)", s.name(), fmt_num(w, 1), fmt_num(h, 1))); }
                });
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Projektion").strong());
                ui.radio_value(&mut nd.projection, Projection::Third, "Projektionsmethode 3 (ANSI/ISO A)");
                ui.radio_value(&mut nd.projection, Projection::First, "Projektionsmethode 1 (DIN/ISO E)");
            });
            ui.separator();
            ui.horizontal(|ui| ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("  Abbrechen  ").clicked() { keep = false; }
                if ui.button("    OK    ").clicked() { ok = true; }
            }));
        });
        if browse { let dir = self.browser_dir.clone(); self.browse(super::creo::BrowsePurpose::DrawingModel, dir, ""); }
        if ok { keep = false; self.new_drawing(&nd); }
        if keep && x_open { self.windows.new_drawing = Some(nd); }
    }
}

impl App {
    /// Opens the Drawing View dialog for a view (demo scenes and scripted use).
    pub fn open_view_dialog_pub(&mut self, id: u32) { self.open_view_dialog(id, false); }
}

impl App {
    /// In-place editor for a dimension or note (ribbon and scripted use).
    pub fn start_edit_pub(&mut self, sel: DSel, screen: Pos2) { self.start_edit(sel, screen); }
}

impl App {
    /// Sheet operations for scripted use and tests.
    pub fn add_sheet_pub(&mut self) { self.add_sheet(); }
    pub fn delete_sheet_pub(&mut self) { self.delete_sheet(); }
}
