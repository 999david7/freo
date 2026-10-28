//! GD&T in drawings: datum feature symbols, feature control frames and surface finish
//! symbols (tools on the Annotate tab), and the contextual property tabs for dimensions
//! (value and tolerance) and those annotations.

use super::drawing::*;
use super::icons::Icon;
use super::ribbon::{group, large, small, stack, theme::*};
use super::*;
use crate::model::drawing::*;
use crate::model::tolerance::{fit_deviations, COMMON_FITS};
use eframe::egui::{self, Color32, Pos2, RichText, Stroke};

type Action = Option<Box<dyn FnOnce(&mut App)>>;

impl App {
    /// A point on a visible model edge near `p`: (view, reference, sheet point).
    pub fn anchor_at(&mut self, p: V2, tol: f64) -> Option<(u32, DimRef, V2)> {
        let sheet = self.drw.as_ref()?.cur().clone();
        for v in sheet.views.iter().rev() {
            let Some(g) = self.view_geom(v.id) else { continue };
            let (lo, hi) = g.rect(v.pos);
            if p.x < lo.x - tol || p.y < lo.y - tol || p.x > hi.x + tol || p.y > hi.y + tol { continue; }
            let t = &g.model.topo;
            let mut best: Option<(f64, usize, usize, f64)> = None;
            for ch in g.lines.chains.iter().filter(|c| c.visible) {
                for (k, w) in ch.pts.windows(2).enumerate() {
                    let (a, b) = (g.to_sheet(v.pos, w[0]), g.to_sheet(v.pos, w[1]));
                    let ab = b - a;
                    let f = if ab.length_squared() < 1e-18 { 0.0 } else { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) };
                    let d = (a + ab * f - p).length();
                    if d < tol && best.map(|x| d < x.0).unwrap_or(true) { best = Some((d, ch.chain, k, f)); }
                }
            }
            let Some((_, ci, k, f)) = best else { continue };
            let pts = &t.chains.get(ci)?.points;
            let lens: Vec<f64> = pts.windows(2).map(|w| (w[1] - w[0]).length()).collect();
            let total: f64 = lens.iter().sum();
            let along = lens[..k].iter().sum::<f64>() + lens[k] * f;
            let point = pts[k] + (pts[k + 1] - pts[k]) * f;
            let r = DimRef { point, edge: Some(t.edge_ref(ci)), at: RefAt::Edge, radius: 0.0, t: if total > 0.0 { along / total } else { 0.5 } };
            return Some((v.id, r, g.world(v.pos, point)));
        }
        None
    }

    /// The single selected dimension, if any.
    pub fn selected_dim(&self) -> Option<DimTarget> {
        let d = self.drw.as_ref()?;
        if d.sel.len() != 1 { return None; }
        match d.sel[0] {
            DSel::Shown(i) => d.cur().shown.get(i).map(|s| DimTarget::Shown(s.key.clone())),
            DSel::Dim(id) => Some(DimTarget::Drawn(id)),
            _ => None,
        }
    }

    fn dim_view(&self, t: &DimTarget) -> Option<(u32, bool)> {
        let sh = self.drw.as_ref()?.cur();
        match t {
            DimTarget::Shown(k) => sh.shown.iter().find(|s| &s.key == k).map(|s| (s.view, s.key.ends_with(":f:diameter") || s.key.contains("Diameter") || s.key.contains("Radius"))),
            DimTarget::Drawn(id) => sh.dims.iter().find(|x| x.id == *id).map(|x| (x.view, matches!(x.ty, DimType::Diameter | DimType::Radius))),
        }
    }

    /// Feature control frame under a dimension: position for features of size, else flatness.
    pub fn add_gtol_on_dim(&mut self, t: DimTarget) {
        let Some((view, size)) = self.dim_view(&t) else { return };
        let si = self.drw.as_ref().map(|d| d.sheet).unwrap_or(0);
        let datums: Vec<String> = self.drw.as_ref().map(|d| d.cur().datums.iter().map(|x| x.letter.clone()).collect()).unwrap_or_default();
        let mut id = 0;
        self.drw_commit(|d| {
            id = d.take_id();
            let (ch, value, diameter) = if size { (GChar::Position, 0.1, true) } else { (GChar::Parallelism, 0.05, false) };
            let refs: Vec<(String, MatCond)> = if datums.is_empty() { vec![("A".into(), MatCond::Rfs)] } else { datums.iter().take(3).map(|l| (l.clone(), MatCond::Rfs)).collect() };
            d.sheets[si].gtols.push(Gtol { id, view, ch, diameter, value, mat: if size { MatCond::Mmc } else { MatCond::Rfs }, datums: refs, target: Attach::Dim(t), pos: V2::ZERO });
        });
        if let Some(d) = &mut self.drw { d.sel = vec![DSel::Gtol(id)]; d.tool = DTool::None; }
        self.tab = Tab::Context;
        self.say("Geometrische Toleranz an der Bemaßung angehängt; im Register Geometrische Toleranz einstellen.", MsgKind::Success);
    }

    /// Previews and clicks of the datum, geometric tolerance and surface finish tools.
    pub fn gdt_tool_frame(&mut self, painter: &egui::Painter, to_s: &dyn Fn(V2) -> Pos2, zoom: f64, m: V2, hits: &[Hit], clicked: bool) -> Action {
        let tool = self.drw.as_ref()?.tool.clone();
        let tol = 8.0 / zoom;
        let blue = Color32::from_rgb(30, 120, 230);
        let anchor = self.anchor_at(m, tol);
        let picked = |app: &mut App, at: &Option<(u32, DimRef)>| -> Option<(u32, DimRef, V2)> {
            let (vid, r) = at.clone()?;
            let v = app.drw.as_ref()?.cur().view(vid)?.clone();
            let g = app.view_geom(vid)?;
            let (p, _, _) = resolve_edge(&r, &g.model.topo);
            Some((vid, r, g.world(v.pos, p)))
        };
        let first = match &tool { DTool::Datum { at } | DTool::Gtol { at } => picked(self, at), _ => None };
        match &first {
            None => if let Some((_, _, s)) = &anchor { painter.circle_stroke(to_s(*s), 5.0, Stroke::new(2.0_f32, blue)); },
            Some((_, _, s)) => { painter.line_segment([to_s(*s), to_s(m)], Stroke::new(1.0_f32, blue)); painter.circle_filled(to_s(*s), 3.0, blue); }
        }
        if !clicked { return None; }
        let dim_hit = hits.iter().rev().find(|h| m.x >= h.min.x && m.y >= h.min.y && m.x <= h.max.x && m.y <= h.max.y).map(|h| h.sel).filter(|s| matches!(s, DSel::Shown(_) | DSel::Dim(_)));
        match (tool, first, anchor) {
            (DTool::Gtol { at: None }, _, _) if dim_hit.is_some() => {
                let sel = dim_hit.unwrap();
                Some(Box::new(move |app: &mut App| {
                    if let Some(d) = &mut app.drw { d.sel = vec![sel]; }
                    if let Some(t) = app.selected_dim() { app.add_gtol_on_dim(t); }
                }))
            }
            (DTool::Datum { at: None }, _, Some((vid, r, _))) => Some(Box::new(move |app: &mut App| { app.set_tool(DTool::Datum { at: Some((vid, r)) }); })),
            (DTool::Gtol { at: None }, _, Some((vid, r, _))) => Some(Box::new(move |app: &mut App| { app.set_tool(DTool::Gtol { at: Some((vid, r)) }); })),
            (DTool::Datum { .. }, Some((vid, r, _)), _) => Some(Box::new(move |app: &mut App| {
                let Some(v) = app.drw.as_ref().and_then(|d| d.cur().view(vid).cloned()) else { return };
                let si = app.drw.as_ref().map(|d| d.sheet).unwrap_or(0);
                let mut id = 0;
                app.drw_commit(|d| {
                    id = d.take_id();
                    let letter = d.sheets[si].next_datum();
                    d.sheets[si].datums.push(DatumSym { id, view: vid, letter, at: r, pos: m - v.pos });
                });
                if let Some(d) = &mut app.drw { d.tool = DTool::None; d.sel = vec![DSel::Datum(id)]; }
            })),
            (DTool::Gtol { .. }, Some((vid, r, _)), _) => Some(Box::new(move |app: &mut App| {
                let Some(v) = app.drw.as_ref().and_then(|d| d.cur().view(vid).cloned()) else { return };
                let si = app.drw.as_ref().map(|d| d.sheet).unwrap_or(0);
                let mut id = 0;
                app.drw_commit(|d| {
                    id = d.take_id();
                    d.sheets[si].gtols.push(Gtol { id, view: vid, ch: GChar::Flatness, diameter: false, value: 0.05, mat: MatCond::Rfs, datums: vec![], target: Attach::Geom(r), pos: m - v.pos });
                });
                if let Some(d) = &mut app.drw { d.tool = DTool::None; d.sel = vec![DSel::Gtol(id)]; }
            })),
            (DTool::Finish, _, Some((vid, r, _))) => Some(Box::new(move |app: &mut App| {
                let si = app.drw.as_ref().map(|d| d.sheet).unwrap_or(0);
                let mut id = 0;
                app.drw_commit(|d| {
                    id = d.take_id();
                    d.sheets[si].finishes.push(Finish { id, view: vid, kind: FinishKind::Removal, value: "Ra 3.2".into(), method: String::new(), at: r, flip: false });
                });
                if let Some(d) = &mut app.drw { d.tool = DTool::None; d.sel = vec![DSel::Finish(id)]; }
            })),
            _ => None,
        }
    }

    // ---------------- contextual tabs ----------------

    pub fn drawing_context_name(&self) -> Option<&'static str> {
        let d = self.drw.as_ref()?;
        if d.sel.len() != 1 { return None; }
        match d.sel[0] {
            DSel::Shown(_) | DSel::Dim(_) => Some("Bemaßung"),
            DSel::Gtol(_) => Some("Geometrische Toleranz"),
            DSel::Datum(_) => Some("Bezugssymbol"),
            DSel::Finish(_) => Some("Oberflächengüte"),
            _ => None,
        }
    }

    /// Opens the contextual tab when an annotation gets selected (as Creo does).
    pub fn sync_context_tab(&mut self) {
        let name = self.drawing_context_name();
        let Some(d) = &mut self.drw else { return };
        if d.sel != d.last_sel {
            d.last_sel = d.sel.clone();
            d.coalesce = false;
            if name.is_some() { self.tab = Tab::Context; }
        }
        if name.is_none() && self.tab == Tab::Context { self.tab = Tab::Annotate; }
    }

    /// A property edit on the current sheet; edits until the selection changes undo together.
    pub fn drw_edit(&mut self, f: impl FnOnce(&mut Sheet)) {
        let Some(d) = &mut self.drw else { return };
        if !d.coalesce {
            d.undo.push(d.data.clone());
            d.redo.clear();
            d.coalesce = true;
        }
        let si = d.sheet;
        f(&mut d.data.sheets[si]);
        d.dirty = true;
    }

    /// Sets the tolerance of a dimension (shown or created).
    pub fn set_dim_tolerance(&mut self, t: &DimTarget, tol: Tolerance) {
        let t = t.clone();
        self.drw_edit(|sh| match &t {
            DimTarget::Shown(k) => if let Some(s) = sh.shown.iter_mut().find(|s| &s.key == k) { s.tol = tol; },
            DimTarget::Drawn(id) => if let Some(x) = sh.dims.iter_mut().find(|x| x.id == *id) { x.tol = tol; },
        });
    }

    pub fn annotation_ribbon(&mut self, ui: &mut egui::Ui) {
        let Some(d) = &self.drw else { return };
        let Some(sel) = d.sel.first().copied() else { return };
        let sh = d.cur().clone();
        let data = d.data.clone();
        let mut done = false;
        match sel {
            DSel::Shown(_) | DSel::Dim(_) => {
                let Some(t) = self.selected_dim() else { return };
                let (tol0, nominal, edit, over) = match (&t, sel) {
                    (DimTarget::Shown(k), _) => {
                        let md = self.model_dims().into_iter().find(|m| &m.key == k);
                        (sh.shown.iter().find(|s| &s.key == k).map(|s| s.tol.clone()).unwrap_or_default(), md.as_ref().map(|m| m.value), md.and_then(|m| m.dk.map(|_| m.edit)), None)
                    }
                    (DimTarget::Drawn(id), _) => {
                        let x = sh.dims.iter().find(|x| x.id == *id);
                        (x.map(|x| x.tol.clone()).unwrap_or_default(), None, None, Some(x.and_then(|x| x.text.clone()).unwrap_or_else(|| "@D".into())))
                    }
                };
                let mut tol = tol0.clone();
                group(ui, "Wert", |ui| {
                    stack(ui, |ui| {
                        if let Some(e) = &edit {
                            ui.label(RichText::new(format!("Nennmaß  {}", e)).size(12.0));
                            if small(ui, Icon::SkModify, "Nennwert ändern", true, false).on_hover_text("Ändert die Modellbemaßung").clicked() {
                                let pos = ui.ctx().input(|i| i.pointer.interact_pos()).unwrap_or(Pos2::new(300.0, 160.0));
                                self.start_edit_pub(sel, pos);
                            }
                        } else if let Some(mut o) = over.clone() {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Text").size(11.0).color(LABEL));
                                if ui.add(egui::TextEdit::singleline(&mut o).desired_width(110.0)).on_hover_text("@D ist der gemessene Wert").changed() {
                                    if let DimTarget::Drawn(id) = t { self.drw_edit(|sh| if let Some(x) = sh.dims.iter_mut().find(|x| x.id == id) { x.text = if o.trim().is_empty() || o.trim() == "@D" { None } else { Some(o.clone()) }; }); }
                                }
                            });
                            ui.label(RichText::new("Abhängige Bemaßung (misst die Ansicht)").size(10.5).color(LABEL));
                        } else {
                            ui.label(RichText::new("Aus einer Normbohrungstabelle").size(11.0).color(LABEL));
                        }
                    });
                });
                group(ui, "Toleranz", |ui| {
                    stack(ui, |ui| {
                        egui::ComboBox::from_id_salt("tolmode").width(120.0).selected_text(tol.mode.name()).show_ui(ui, |ui| {
                            for m in TolMode::ALL { ui.selectable_value(&mut tol.mode, m, m.name()); }
                        });
                        let dv = |ui: &mut egui::Ui, label: &str, v: &mut f64| {
                            ui.horizontal(|ui| { ui.label(RichText::new(label).size(11.0).color(LABEL)); ui.add(egui::DragValue::new(v).speed(0.001).max_decimals(4).min_decimals(3)); });
                        };
                        match tol.mode {
                            TolMode::PlusMinus => { dv(ui, "Oberes", &mut tol.upper); dv(ui, "Unteres", &mut tol.lower); }
                            TolMode::Symmetric => dv(ui, "±", &mut tol.upper),
                            TolMode::Limits => {
                                match nominal {
                                    Some(n) => {
                                        let (mut up, mut lo) = (n + tol.upper, n + tol.lower);
                                        dv(ui, "Höchstmaß", &mut up);
                                        dv(ui, "Mindestmaß", &mut lo);
                                        tol.upper = up - n;
                                        tol.lower = lo - n;
                                    }
                                    None => { dv(ui, "Oberes Abmaß", &mut tol.upper); dv(ui, "Unteres Abmaß", &mut tol.lower); }
                                }
                            }
                            TolMode::Fit => {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Klasse").size(11.0).color(LABEL));
                                    ui.add(egui::TextEdit::singleline(&mut tol.fit).desired_width(42.0));
                                    egui::ComboBox::from_id_salt("fitcls").width(22.0).selected_text("").show_ui(ui, |ui| {
                                        for c in COMMON_FITS { if ui.selectable_label(tol.fit == c, c).clicked() { tol.fit = c.into(); } }
                                    });
                                });
                                let info = nominal.map(|n| match fit_deviations(&tol.fit, n) {
                                    Some((u, l)) => format!("{}  {} / {}", fmt_num(n, 3), data.fmt_dev(u), data.fmt_dev(l)),
                                    None => "nicht in der ISO-286-Tabelle".into(),
                                }).unwrap_or_else(|| "Bohrung: Großbuchstaben, Welle: Kleinbuchstaben".into());
                                ui.label(RichText::new(info).size(10.5).color(LABEL));
                            }
                            TolMode::Basic => { ui.label(RichText::new("Theoretisch genau (eingerahmt)").size(10.5).color(LABEL)); }
                            TolMode::Reference => { ui.label(RichText::new("In Klammern angezeigt").size(10.5).color(LABEL)); }
                            TolMode::Nominal => {}
                        }
                    });
                });
                if tol != tol0 { self.set_dim_tolerance(&t, tol); }
                group(ui, "Genauigkeit", |ui| {
                    stack(ui, |ui| {
                        let (mut dec, mut td) = (data.decimals, data.tol_decimals);
                        ui.horizontal(|ui| { ui.label(RichText::new("Dezimalstellen").size(11.0).color(LABEL)); ui.add(egui::DragValue::new(&mut dec).range(0..=4)); });
                        ui.horizontal(|ui| { ui.label(RichText::new("Toleranz").size(11.0).color(LABEL)); ui.add(egui::DragValue::new(&mut td).range(1..=5)); });
                        if dec != data.decimals || td != data.tol_decimals { if let Some(d) = &mut self.drw { d.data.decimals = dec; d.data.tol_decimals = td; d.dirty = true; } }
                    });
                });
                group(ui, "Geometrische Toleranz", |ui| {
                    let has = sh.gtols.iter().any(|g| g.target == Attach::Dim(t.clone()));
                    if large(ui, Icon::GeomTol, "Geometrische\nToleranz hinzufügen", !has, false).on_hover_text("Toleranzrahmen unter dieser Bemaßung").clicked() { self.add_gtol_on_dim(t.clone()); }
                });
            }
            DSel::Gtol(id) => {
                let Some(g0) = sh.gtols.iter().find(|x| x.id == id).cloned() else { return };
                let mut g = g0.clone();
                group(ui, "Eigenschaft", |ui| {
                    stack(ui, |ui| {
                        egui::ComboBox::from_id_salt("gchar").width(150.0).selected_text(g.ch.name()).show_ui(ui, |ui| {
                            for c in GChar::ALL { ui.selectable_value(&mut g.ch, c, c.name()); }
                        });
                        ui.label(RichText::new(match g.target { Attach::Dim(_) => "An einer Bemaßung platziert", Attach::Geom(_) => "Hinweislinie zum Element" }).size(10.5).color(LABEL));
                    });
                });
                group(ui, "Toleranz", |ui| {
                    stack(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut g.diameter, "⌀-Zone");
                            ui.add(egui::DragValue::new(&mut g.value).speed(0.001).range(0.0..=100.0).max_decimals(4));
                        });
                        egui::ComboBox::from_id_salt("gmat").width(150.0).selected_text(g.mat.name()).show_ui(ui, |ui| {
                            for m in MatCond::ALL { ui.selectable_value(&mut g.mat, m, m.name()); }
                        });
                    });
                });
                group(ui, "Bezüge", |ui| {
                    let form = g.ch.form();
                    while g.datums.len() < 3 { g.datums.push((String::new(), MatCond::Rfs)); }
                    for (k, name) in ["Primär", "Sekundär", "Tertiär"].iter().enumerate() {
                        stack(ui, |ui| {
                            ui.label(RichText::new(*name).size(10.5).color(LABEL));
                            ui.add_enabled(!form, egui::TextEdit::singleline(&mut g.datums[k].0).desired_width(40.0));
                            ui.add_enabled_ui(!form, |ui| {
                                egui::ComboBox::from_id_salt(("dmat", k)).width(40.0).selected_text(g.datums[k].1.letter().map(|l| format!("({})", l)).unwrap_or("—".into())).show_ui(ui, |ui| {
                                    for m in MatCond::ALL { ui.selectable_value(&mut g.datums[k].1, m, m.name()); }
                                });
                            });
                        });
                    }
                    g.datums.retain(|d| !d.0.trim().is_empty());
                    if form { ui.label(RichText::new("Formtoleranzen haben keine Bezüge").size(10.5).color(LABEL)); }
                });
                let mut base = g0.clone();
                base.datums.retain(|d| !d.0.trim().is_empty());
                if g != base { self.drw_edit(|sh| if let Some(x) = sh.gtols.iter_mut().find(|x| x.id == id) { *x = g; }); }
            }
            DSel::Datum(id) => {
                let Some(ds) = sh.datums.iter().find(|x| x.id == id).cloned() else { return };
                let mut letter = ds.letter.clone();
                group(ui, "Bezugselement", |ui| {
                    stack(ui, |ui| {
                        ui.horizontal(|ui| { ui.label(RichText::new("Beschriftung").size(11.0).color(LABEL)); ui.add(egui::TextEdit::singleline(&mut letter).desired_width(40.0)); });
                        ui.label(RichText::new("Kasten ziehen zum Verschieben; das Dreieck bleibt am Element").size(10.5).color(LABEL));
                    });
                });
                let letter = letter.trim().to_uppercase();
                if letter != ds.letter && !letter.is_empty() { self.drw_edit(|sh| if let Some(x) = sh.datums.iter_mut().find(|x| x.id == id) { x.letter = letter; }); }
            }
            DSel::Finish(id) => {
                let Some(f0) = sh.finishes.iter().find(|x| x.id == id).cloned() else { return };
                let mut f = f0.clone();
                group(ui, "Symbol", |ui| {
                    stack(ui, |ui| { for k in FinishKind::ALL { ui.radio_value(&mut f.kind, k, k.name()); } });
                });
                group(ui, "Oberflächenbeschaffenheit", |ui| {
                    stack(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Anforderung").size(11.0).color(LABEL));
                            ui.add(egui::TextEdit::singleline(&mut f.value).desired_width(70.0));
                            egui::ComboBox::from_id_salt("ra").width(22.0).selected_text("").show_ui(ui, |ui| { for r in RA_VALUES { if ui.selectable_label(f.value == r, r).clicked() { f.value = r.into(); } } });
                        });
                        ui.horizontal(|ui| { ui.label(RichText::new("Verfahren").size(11.0).color(LABEL)); ui.add(egui::TextEdit::singleline(&mut f.method).desired_width(90.0)); });
                        if small(ui, Icon::Flip, "Seite umkehren", true, f.flip).clicked() { f.flip = !f.flip; }
                    });
                });
                if f != f0 { self.drw_edit(|sh| if let Some(x) = sh.finishes.iter_mut().find(|x| x.id == id) { *x = f; }); }
            }
            _ => return,
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            ui.add_space(6.0);
            if large(ui, Icon::Delete, "Löschen", true, false).clicked() { self.drw_delete_interactive(); }
            if large(ui, Icon::Ok, "OK", true, false).on_hover_text("Fertig (Auswahl aufheben)").clicked() { done = true; }
        });
        if done { if let Some(d) = &mut self.drw { d.sel.clear(); } self.tab = Tab::Annotate; }
    }
}
