//! Library editors of the Leiterplatte application: the symbol editor (pin table → IC box,
//! or editing the pins of an existing symbol) and the footprint editor (generators for chip,
//! dual-row, quad and THT packages plus a free pad table), both with a live preview and
//! export to KiCad libraries.

use super::dlgkit;
use super::ribbon::theme::*;
use super::*;
use crate::pcb::footprints::{self, Footprint, Gen, Pad, PadShape};
use crate::pcb::geom::{p, P};
use crate::pcb::kicad;
use crate::pcb::symbols::{self, PinRow, PinType, Side, Symbol, PIN_TYPES};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Sense, Shape, Stroke, StrokeKind};

#[derive(Clone, Debug)]
pub struct SymEditor {
    pub name: String,
    pub prefix: String,
    pub value: String,
    pub footprint: String,
    pub description: String,
    pub rows: Vec<PinRow>,
    /// editing an existing symbol: keep its drawing, only pins and fields change
    pub base: Option<Symbol>,
}

impl SymEditor {
    pub fn new() -> SymEditor {
        SymEditor {
            name: "MEIN_IC".into(),
            prefix: "U".into(),
            value: "MEIN_IC".into(),
            footprint: String::new(),
            description: String::new(),
            rows: vec![
                PinRow { num: "1".into(), name: "VCC".into(), kind: PinType::PowerIn, side: Side::Top },
                PinRow { num: "2".into(), name: "IN".into(), kind: PinType::Input, side: Side::Left },
                PinRow { num: "3".into(), name: "OUT".into(), kind: PinType::Output, side: Side::Right },
                PinRow { num: "4".into(), name: "GND".into(), kind: PinType::PowerIn, side: Side::Bottom },
            ],
            base: None,
        }
    }
    pub fn from_symbol(s: &Symbol) -> SymEditor {
        SymEditor {
            name: s.short().to_string(),
            prefix: s.prefix.clone(),
            value: s.value.clone(),
            footprint: s.footprint.clone(),
            description: s.description.clone(),
            rows: s.pins.iter().map(|q| PinRow { num: q.num.clone(), name: q.name.clone(), kind: q.kind, side: Side::of(q.dir) }).collect(),
            base: Some(s.clone()),
        }
    }
    /// The symbol as it would be saved.
    pub fn build(&self) -> Symbol {
        let id = format!("Projekt:{}", self.name.trim());
        match &self.base {
            // same pins in the same order: keep the drawing, take numbers, names and types
            Some(b) if b.pins.len() == self.rows.len() && b.pins.iter().zip(&self.rows).all(|(q, r)| Side::of(q.dir) == r.side) => {
                let mut s = b.clone();
                s.id = id;
                s.prefix = self.prefix.clone();
                s.value = self.value.clone();
                s.footprint = self.footprint.clone();
                s.description = self.description.clone();
                for (q, r) in s.pins.iter_mut().zip(&self.rows) {
                    q.num = r.num.clone();
                    q.name = r.name.clone();
                    q.kind = r.kind;
                }
                if !s.footprint.is_empty() && !s.alt_footprints.contains(&s.footprint) {
                    s.alt_footprints.insert(0, s.footprint.clone());
                }
                s
            }
            _ => symbols::box_symbol(&id, &self.prefix, &self.value, &self.footprint, &self.description, &self.rows),
        }
    }
}

impl Default for SymEditor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct FpEditor {
    pub name: String,
    pub description: String,
    pub gen: usize,
    pub gens: [Gen; 4],
    /// free pad table instead of a generator
    pub manual: bool,
    pub pads: Vec<Pad>,
    pub height: f64,
}

impl FpEditor {
    pub fn new() -> FpEditor {
        FpEditor { name: "MEIN_FOOTPRINT".into(), description: String::new(), gen: 1, gens: Gen::defaults(), manual: false, pads: vec![], height: 1.5 }
    }
    pub fn build(&self) -> Footprint {
        let id = format!("Projekt:{}", self.name.trim());
        let mut f = footprints::generate(&id, &self.description, &self.gens[self.gen], self.height);
        if self.manual && !self.pads.is_empty() {
            let mut g = footprints::generate(&id, &self.description, &Gen::Chip { span: 1.0, pw: 0.5, ph: 0.5 }, self.height);
            g.pads = self.pads.clone();
            let (lo, hi) = crate::pcb::geom::bbox(g.pads.iter().flat_map(|q| {
                let (a, b) = q.hull(P::ZERO, 0.0, false).bbox();
                [a, b]
            }))
            .unwrap_or((P::ZERO, P::ZERO));
            g.silk = vec![];
            let c = [lo, p(hi.x, lo.y), hi, p(lo.x, hi.y)].map(|q| q + (q - (lo + hi) / 2.0).signum() * 0.3);
            for i in 0..4 {
                g.silk.push((c[i], c[(i + 1) % 4]));
            }
            g.court = (lo - P::splat(0.25), hi + P::splat(0.25));
            g.body = vec![footprints::Body3::Box { lo, hi, z0: 0.0, z1: self.height, color: [38, 38, 40] }];
            f = g;
        }
        f
    }
}

impl Default for FpEditor {
    fn default() -> Self {
        Self::new()
    }
}

fn num_field(ui: &mut egui::Ui, label: &str, v: &mut f64, lo: f64, hi: f64) {
    ui.label(egui::RichText::new(label).size(13.0).color(TEXT));
    ui.add(egui::DragValue::new(v).speed(0.01).range(lo..=hi).fixed_decimals(3));
}

fn int_field(ui: &mut egui::Ui, label: &str, v: &mut usize, lo: usize, hi: usize) {
    ui.label(egui::RichText::new(label).size(13.0).color(TEXT));
    ui.add(egui::DragValue::new(v).speed(0.2).range(lo..=hi));
}

/// Preview of a footprint in a white box.
pub fn fp_preview(ui: &mut egui::Ui, f: &Footprint, h: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    ui.painter().rect_filled(r, 2.0, Color32::from_rgb(28, 64, 44));
    ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
    let mut pts: Vec<P> = vec![f.court.0, f.court.1];
    for q in &f.pads {
        let (a, b) = q.hull(P::ZERO, 0.0, false).bbox();
        pts.extend([a, b]);
    }
    let Some((lo, hi)) = crate::pcb::geom::bbox(pts) else { return };
    let mut cam = super::pcb::Cam2::default();
    cam.fit(r, lo - P::splat(0.5), hi + P::splat(0.5));
    let pa = ui.painter().with_clip_rect(r);
    let sc = |q: P| cam.scr(r, q);
    let (cl, ch) = f.court;
    pa.rect_stroke(Rect::from_two_pos(sc(cl), sc(ch)), 0.0, Stroke::new(1.0_f32, Color32::from_rgb(220, 60, 220)), StrokeKind::Middle);
    for (a, b) in &f.silk {
        pa.line_segment([sc(*a), sc(*b)], Stroke::new(1.2_f32, Color32::from_rgb(242, 238, 200)));
    }
    for q in &f.pads {
        let poly: Vec<egui::Pos2> = q.hull(P::ZERO, 0.0, false).polygon(20).iter().map(|x| sc(*x)).collect();
        pa.add(Shape::convex_polygon(poly, Color32::from_rgb(214, 160, 70), Stroke::NONE));
        if q.drill > 0.0 {
            pa.circle_filled(sc(q.pos), q.drill as f32 / 2.0 * cam.s, Color32::from_rgb(24, 24, 24));
        }
        let px = (q.size.x.min(q.size.y) as f32 * cam.s * 0.5).min(14.0);
        if px > 6.0 {
            pa.text(sc(q.pos), Align2::CENTER_CENTER, &q.num, FontId::proportional(px), Color32::WHITE);
        }
    }
}

fn buttons(ui: &mut egui::Ui, labels: &[&str]) -> Option<usize> {
    let mut hit = None;
    dlgkit::pad(ui, 10.0, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (i, l) in labels.iter().enumerate().rev() {
                if dlgkit::btn(ui, l, 96.0, if i == 0 { 1 } else { 0 }) {
                    hit = Some(i);
                }
            }
        });
    });
    hit
}

impl App {
    pub fn pcb_open_symbol_editor(&mut self) {
        let base = self.pcb.lib_sel.as_deref().and_then(|id| self.pcb.cur().def(id).cloned()).filter(|d| !d.power);
        self.pcb.sym_ed = Some(match base {
            Some(d) if self.pcb.sel.is_none() => SymEditor::from_symbol(&d),
            _ => match self.pcb.sel {
                Some(super::pcb::PSel::Sym(i)) => self.pcb.cur().symbols.get(i).and_then(|s| self.pcb.cur().def(&s.lib)).filter(|d| !d.power).map(SymEditor::from_symbol).unwrap_or_default(),
                _ => SymEditor::new(),
            },
        });
    }

    pub fn pcb_open_fp_editor(&mut self) {
        self.pcb.fp_ed = Some(match self.pcb.sel {
            Some(super::pcb::PSel::Fp(i)) => {
                let f = &self.pcb.project.board.footprints[i].fp;
                FpEditor { name: f.short().to_string(), description: f.description.clone(), manual: true, pads: f.pads.clone(), height: f.height().max(0.5), ..FpEditor::new() }
            }
            _ => FpEditor::new(),
        });
    }

    /// Saves the project's own symbols and footprints as KiCad libraries.
    pub fn pcb_export_libraries(&mut self) {
        let Some(dir) = rfd::FileDialog::new().set_title("Ordner für die Projektbibliotheken").set_directory(self.pcb.dir.clone().unwrap_or_else(|| self.work_dir())).pick_folder() else { return };
        let name = if self.pcb.project.name.is_empty() { "Projekt".to_string() } else { self.pcb.project.name.clone() };
        // every symbol and footprint the project uses
        let mut syms: Vec<Symbol> = vec![];
        let flat = self.pcb.project.schematic.flatten();
        for x in &flat.symbols {
            if let Some(d) = flat.def(&x.lib) {
                if !d.power && !syms.iter().any(|s| s.id == d.id) {
                    syms.push(d.clone());
                }
            }
        }
        let mut fps: Vec<Footprint> = self.pcb.project.footprints.clone();
        for f in &self.pcb.project.board.footprints {
            if !fps.iter().any(|x| x.id == f.fp.id) {
                fps.push(f.fp.clone());
            }
        }
        let pretty = dir.join(format!("{}.pretty", name));
        let res = (|| -> Result<(), String> {
            std::fs::write(dir.join(format!("{}.kicad_sym", name)), kicad::write_symbol_lib(&syms)).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&pretty).map_err(|e| e.to_string())?;
            for f in &fps {
                std::fs::write(pretty.join(format!("{}.kicad_mod", f.short())), kicad::write_footprint_file(f)).map_err(|e| e.to_string())?;
            }
            Ok(())
        })();
        match res {
            Ok(()) => self.say(format!("Bibliotheken gespeichert: {}.kicad_sym ({} Symbole) und {}.pretty ({} Footprints) in {}.", name, syms.len(), name, fps.len(), dir.display()), MsgKind::Success),
            Err(e) => self.say(format!("Bibliotheken konnten nicht gespeichert werden: {}", e), MsgKind::Error),
        }
    }

    pub fn pcb_editor_windows(&mut self, ctx: &egui::Context) {
        let anchor = ctx.screen_rect().center() - vec2(330.0, 280.0);
        // ---------------------------------------------------------------- symbol editor
        if let Some(mut ed) = self.pcb.sym_ed.clone() {
            let mut hit = None;
            let open = dlgkit::cwin(ctx, "pcb_sym_ed", "Symbol-Editor", true, anchor, 660.0, |ui| {
                dlgkit::pad(ui, 12.0, |ui| {
                    egui::Grid::new("sym_ed_fields").num_columns(4).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                        dlgkit::lbl(ui, "Name");
                        dlgkit::field(ui, &mut ed.name, 200.0, true);
                        dlgkit::lbl(ui, "Präfix");
                        dlgkit::field(ui, &mut ed.prefix, 80.0, true);
                        ui.end_row();
                        dlgkit::lbl(ui, "Wert");
                        dlgkit::field(ui, &mut ed.value, 200.0, true);
                        dlgkit::lbl(ui, "Footprint");
                        dlgkit::field(ui, &mut ed.footprint, 200.0, true);
                        ui.end_row();
                        dlgkit::lbl(ui, "Beschreibung");
                        dlgkit::field(ui, &mut ed.description, 200.0, true);
                        ui.end_row();
                    });
                    if let Some(b) = &ed.base {
                        dlgkit::lbl_dim(ui, &format!("Bearbeitet {} – Zeichnung bleibt, solange Pinzahl und Seiten gleich sind; sonst entsteht ein IC-Kasten.", b.id));
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(380.0);
                            dlgkit::lbl_bold(ui, "Pins");
                            let mut del = None;
                            egui::ScrollArea::vertical().id_salt("sym_ed_pins").max_height(300.0).show(ui, |ui| {
                                egui::Grid::new("sym_ed_pin_grid").num_columns(5).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
                                    dlgkit::lbl_dim(ui, "Nr.");
                                    dlgkit::lbl_dim(ui, "Name");
                                    dlgkit::lbl_dim(ui, "Typ");
                                    dlgkit::lbl_dim(ui, "Seite");
                                    ui.end_row();
                                    for (k, r) in ed.rows.iter_mut().enumerate() {
                                        ui.add_sized(vec2(44.0, 24.0), egui::TextEdit::singleline(&mut r.num));
                                        ui.add_sized(vec2(100.0, 24.0), egui::TextEdit::singleline(&mut r.name));
                                        egui::ComboBox::from_id_salt(("pt", k)).width(110.0).selected_text(r.kind.name()).show_ui(ui, |ui| {
                                            for t in PIN_TYPES {
                                                ui.selectable_value(&mut r.kind, t, t.name());
                                            }
                                        });
                                        egui::ComboBox::from_id_salt(("ps", k)).width(70.0).selected_text(r.side.name()).show_ui(ui, |ui| {
                                            for s in Side::ALL {
                                                ui.selectable_value(&mut r.side, s, s.name());
                                            }
                                        });
                                        if ui.small_button("✕").on_hover_text("Pin löschen").clicked() {
                                            del = Some(k);
                                        }
                                        ui.end_row();
                                    }
                                });
                            });
                            if let Some(k) = del {
                                ed.rows.remove(k);
                            }
                            ui.horizontal(|ui| {
                                if dlgkit::btn(ui, "Pin hinzufügen", 120.0, 0) {
                                    let n = ed.rows.iter().filter_map(|r| r.num.parse::<u32>().ok()).max().unwrap_or(0) + 1;
                                    let side = if ed.rows.iter().filter(|r| r.side == Side::Left).count() <= ed.rows.iter().filter(|r| r.side == Side::Right).count() { Side::Left } else { Side::Right };
                                    ed.rows.push(PinRow { num: n.to_string(), name: format!("P{}", n), kind: PinType::Bidi, side });
                                }
                                let fp = footprints::find(&ed.footprint);
                                if dlgkit::btn(ui, "Pins aus Footprint", 140.0, if fp.is_some() { 0 } else { 2 }) {
                                    if let Some(f) = fp {
                                        let mut nums: Vec<String> = f.pads.iter().map(|q| q.num.clone()).filter(|n| !n.is_empty()).collect();
                                        nums.dedup();
                                        let half = nums.len().div_ceil(2);
                                        ed.rows = nums.iter().enumerate().map(|(i, n)| PinRow { num: n.clone(), name: n.clone(), kind: PinType::Passive, side: if i < half { Side::Left } else { Side::Right } }).collect();
                                        ed.base = None;
                                    }
                                }
                            });
                        });
                        // preview
                        let sym = ed.build();
                        let (r, _) = ui.allocate_exact_size(vec2(240.0, 330.0), Sense::hover());
                        ui.painter().rect_filled(r, 2.0, Color32::WHITE);
                        ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
                        let (lo, hi) = sym.bounds();
                        let mut cam = super::pcb::Cam2::default();
                        cam.fit(r, lo - P::splat(3.0), hi + P::splat(3.0));
                        cam.s = cam.s.min(10.0);
                        super::pcb::draw_symbol_pub(&ui.painter().with_clip_rect(r), &cam, r, &sym);
                    });
                });
                dlgkit::hline(ui);
                hit = buttons(ui, &["Übernehmen", "Als .kicad_sym …", "Abbrechen"]);
            });
            let dup = ed.rows.iter().enumerate().any(|(i, r)| ed.rows[..i].iter().any(|o| o.num == r.num));
            match hit {
                Some(0) | Some(1) if ed.name.trim().is_empty() || dup => {
                    self.say(if dup { "Pinnummern müssen eindeutig sein." } else { "Der Symbolname fehlt." }, MsgKind::Warning);
                    self.pcb.sym_ed = Some(ed);
                }
                Some(0) => {
                    let sym = ed.build();
                    let id = sym.id.clone();
                    self.pcb_push();
                    for sch in [&mut self.pcb.project.schematic] {
                        sch.local_lib.retain(|x| x.id != id);
                        sch.local_lib.push(sym.clone());
                    }
                    let cur = self.pcb.cur_mut();
                    cur.local_lib.retain(|x| x.id != id);
                    cur.local_lib.push(sym.clone());
                    symbols::register(vec![sym]);
                    self.pcb.lib_sel = Some(id.clone());
                    self.pcb.sym_ed = None;
                    self.say(format!("Symbol {} in die Projektbibliothek übernommen – mit „Symbol“ platzieren.", id), MsgKind::Success);
                }
                Some(1) => {
                    let sym = ed.build();
                    if let Some(path) = rfd::FileDialog::new().set_title("Symbol als KiCad-Bibliothek speichern").set_file_name(format!("{}.kicad_sym", ed.name.trim())).add_filter("KiCad-Symbole", &["kicad_sym"]).save_file() {
                        match std::fs::write(&path, kicad::write_symbol_lib(&[sym])) {
                            Ok(()) => self.say(format!("Gespeichert: {}", path.display()), MsgKind::Success),
                            Err(e) => self.say(format!("Speichern fehlgeschlagen: {}", e), MsgKind::Error),
                        }
                    }
                    self.pcb.sym_ed = Some(ed);
                }
                Some(_) => self.pcb.sym_ed = None,
                None if !open => self.pcb.sym_ed = None,
                None => self.pcb.sym_ed = Some(ed),
            }
        }
        // ---------------------------------------------------------------- footprint editor
        if let Some(mut ed) = self.pcb.fp_ed.clone() {
            let mut hit = None;
            let open = dlgkit::cwin(ctx, "pcb_fp_ed", "Footprint-Editor", true, anchor + vec2(20.0, 20.0), 700.0, |ui| {
                dlgkit::pad(ui, 12.0, |ui| {
                    egui::Grid::new("fp_ed_fields").num_columns(4).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                        dlgkit::lbl(ui, "Name");
                        dlgkit::field(ui, &mut ed.name, 220.0, true);
                        dlgkit::lbl(ui, "Bauhöhe");
                        ui.add(egui::DragValue::new(&mut ed.height).speed(0.05).range(0.1..=50.0).suffix(" mm"));
                        ui.end_row();
                        dlgkit::lbl(ui, "Beschreibung");
                        dlgkit::field(ui, &mut ed.description, 220.0, true);
                        ui.end_row();
                    });
                    ui.horizontal(|ui| {
                        if dlgkit::radio(ui, !ed.manual, "Generator", true) {
                            ed.manual = false;
                        }
                        if dlgkit::radio(ui, ed.manual, "Pad-Tabelle", true) {
                            if ed.pads.is_empty() {
                                ed.pads = footprints::generate("x", "", &ed.gens[ed.gen], 1.0).pads;
                            }
                            ed.manual = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(380.0);
                            if !ed.manual {
                                let names: Vec<&str> = ed.gens.iter().map(|g| g.name()).collect();
                                let mut k = ed.gen;
                                if dlgkit::combo(ui, "fp_gen", 300.0, &mut k, &names, true, None) {
                                    ed.gen = k;
                                }
                                egui::Grid::new("fp_gen_grid").num_columns(2).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
                                    match &mut ed.gens[ed.gen] {
                                        Gen::Chip { span, pw, ph } => {
                                            num_field(ui, "Padabstand (Mitte)", span, 0.2, 50.0);
                                            ui.end_row();
                                            num_field(ui, "Padbreite", pw, 0.1, 20.0);
                                            ui.end_row();
                                            num_field(ui, "Padhöhe", ph, 0.1, 20.0);
                                            ui.end_row();
                                        }
                                        Gen::Dual { n, pitch, span, pw, ph } => {
                                            int_field(ui, "Pins", n, 2, 200);
                                            ui.end_row();
                                            num_field(ui, "Raster", pitch, 0.3, 10.0);
                                            ui.end_row();
                                            num_field(ui, "Reihenabstand", span, 0.5, 60.0);
                                            ui.end_row();
                                            num_field(ui, "Padbreite", pw, 0.1, 10.0);
                                            ui.end_row();
                                            num_field(ui, "Padhöhe", ph, 0.1, 10.0);
                                            ui.end_row();
                                        }
                                        Gen::Quad { per_side, pitch, span, pw, ph, ep } => {
                                            int_field(ui, "Pins je Seite", per_side, 1, 100);
                                            ui.end_row();
                                            num_field(ui, "Raster", pitch, 0.3, 5.0);
                                            ui.end_row();
                                            num_field(ui, "Abstand gegenüber", span, 1.0, 60.0);
                                            ui.end_row();
                                            num_field(ui, "Padlänge", pw, 0.1, 5.0);
                                            ui.end_row();
                                            num_field(ui, "Padbreite", ph, 0.1, 5.0);
                                            ui.end_row();
                                            num_field(ui, "Wärmepad (0 = keins)", ep, 0.0, 30.0);
                                            ui.end_row();
                                        }
                                        Gen::Tht { rows, cols, pitch, span, drill, pad } => {
                                            int_field(ui, "Reihen", rows, 1, 4);
                                            ui.end_row();
                                            int_field(ui, "Pins je Reihe", cols, 1, 64);
                                            ui.end_row();
                                            num_field(ui, "Raster", pitch, 0.5, 20.0);
                                            ui.end_row();
                                            num_field(ui, "Reihenabstand", span, 0.5, 30.0);
                                            ui.end_row();
                                            num_field(ui, "Bohrung", drill, 0.2, 6.0);
                                            ui.end_row();
                                            num_field(ui, "Pad-Ø", pad, 0.3, 10.0);
                                            ui.end_row();
                                        }
                                    }
                                });
                            } else {
                                let mut del = None;
                                egui::ScrollArea::vertical().id_salt("fp_pads").max_height(300.0).show(ui, |ui| {
                                    egui::Grid::new("fp_pad_grid").num_columns(8).spacing(vec2(4.0, 3.0)).show(ui, |ui| {
                                        for h in ["Nr.", "X", "Y", "B", "H", "Form", "Bohr.", ""] {
                                            dlgkit::lbl_dim(ui, h);
                                        }
                                        ui.end_row();
                                        for (k, q) in ed.pads.iter_mut().enumerate() {
                                            ui.add_sized(vec2(34.0, 22.0), egui::TextEdit::singleline(&mut q.num));
                                            ui.add(egui::DragValue::new(&mut q.pos.x).speed(0.01).fixed_decimals(3));
                                            ui.add(egui::DragValue::new(&mut q.pos.y).speed(0.01).fixed_decimals(3));
                                            ui.add(egui::DragValue::new(&mut q.size.x).speed(0.01).range(0.05..=50.0).fixed_decimals(2));
                                            ui.add(egui::DragValue::new(&mut q.size.y).speed(0.01).range(0.05..=50.0).fixed_decimals(2));
                                            let shapes = [("Rund", PadShape::Circle), ("Rechteck", PadShape::Rect), ("Oval", PadShape::Oval), ("Abgerundet", PadShape::RoundRect(0.25))];
                                            let cur = shapes.iter().find(|s| std::mem::discriminant(&s.1) == std::mem::discriminant(&q.shape)).map(|s| s.0).unwrap_or("?");
                                            egui::ComboBox::from_id_salt(("pshape", k)).width(80.0).selected_text(cur).show_ui(ui, |ui| {
                                                for (n, s) in shapes {
                                                    if ui.selectable_label(cur == n, n).clicked() {
                                                        q.shape = s;
                                                    }
                                                }
                                            });
                                            ui.add(egui::DragValue::new(&mut q.drill).speed(0.01).range(0.0..=10.0).fixed_decimals(2));
                                            if ui.small_button("✕").clicked() {
                                                del = Some(k);
                                            }
                                            ui.end_row();
                                        }
                                    });
                                });
                                if let Some(k) = del {
                                    ed.pads.remove(k);
                                }
                                if dlgkit::btn(ui, "Pad hinzufügen", 120.0, 0) {
                                    let n = ed.pads.iter().filter_map(|q| q.num.parse::<u32>().ok()).max().unwrap_or(0) + 1;
                                    let at = ed.pads.last().map(|q| q.pos + p(1.27, 0.0)).unwrap_or(P::ZERO);
                                    ed.pads.push(Pad { num: n.to_string(), pos: at, size: p(1.0, 1.0), shape: PadShape::RoundRect(0.25), drill: 0.0, rot: 0.0, npth: false, slot: 0.0, drill_off: P::ZERO });
                                }
                            }
                        });
                        ui.vertical(|ui| {
                            ui.set_width(280.0);
                            let f = ed.build();
                            fp_preview(ui, &f, 280.0);
                            dlgkit::lbl_dim(ui, &format!("{} Pads, {} · Sperrfläche {} × {} mm", f.pads.len(), if f.smd() { "SMD" } else { "THT" }, crate::pcb::geom::mm(f.court.1.x - f.court.0.x), crate::pcb::geom::mm(f.court.1.y - f.court.0.y)));
                        });
                    });
                });
                dlgkit::hline(ui);
                hit = buttons(ui, &["Übernehmen", "Als .kicad_mod …", "Abbrechen"]);
            });
            match hit {
                Some(0) | Some(1) if ed.name.trim().is_empty() => {
                    self.say("Der Footprintname fehlt.", MsgKind::Warning);
                    self.pcb.fp_ed = Some(ed);
                }
                Some(0) => {
                    let f = ed.build();
                    let id = f.id.clone();
                    self.pcb_push();
                    self.pcb.project.footprints.retain(|x| x.id != id);
                    self.pcb.project.footprints.push(f.clone());
                    footprints::register(vec![f]);
                    self.pcb.fp_ed = None;
                    self.say(format!("Footprint {} in die Projektbibliothek übernommen – im Symbol unter „Footprint“ eintragen.", id), MsgKind::Success);
                }
                Some(1) => {
                    let f = ed.build();
                    if let Some(path) = rfd::FileDialog::new().set_title("Footprint speichern").set_file_name(format!("{}.kicad_mod", ed.name.trim())).add_filter("KiCad-Footprint", &["kicad_mod"]).save_file() {
                        match std::fs::write(&path, kicad::write_footprint_file(&f)) {
                            Ok(()) => self.say(format!("Gespeichert: {}", path.display()), MsgKind::Success),
                            Err(e) => self.say(format!("Speichern fehlgeschlagen: {}", e), MsgKind::Error),
                        }
                    }
                    self.pcb.fp_ed = Some(ed);
                }
                Some(_) => self.pcb.fp_ed = None,
                None if !open => self.pcb.fp_ed = None,
                None => self.pcb.fp_ed = Some(ed),
            }
        }
        let _ = (pos2(0.0, 0.0), Sense::hover());
    }
}
