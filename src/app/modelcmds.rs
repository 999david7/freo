//! Modell tab commands built on the solid kernel: Regenerierungsmanager, Kopieren / Einfügen,
//! Benutzerdefiniertes KE (UDF), Kopiergeometrie, Shrinkwrap, Automatische Rundung, Geometriemuster.

use super::creo::dialog_header;
use super::icons::{self, Icon};
use super::ribbon::theme::*;
use super::*;
use crate::model::features::{ImportDef, ImportFace, RoundSet};
use eframe::egui::{self, pos2, vec2, RichText};

/// What the work-folder file dialog is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileTask { Udf, CopyGeom, Shrinkwrap }

/// The small dialog listing files of the working folder (UDFs, parts) with a name field.
pub struct FileTaskDialog { pub task: FileTask, pub sel: Option<PathBuf>, pub name: String }

/// A UDF file: features saved from a model, inserted again as copies.
#[derive(serde::Serialize, serde::Deserialize)]
struct UdfFile { name: String, features: Vec<Feature> }

/// The current solid as an import feature (one face per model face, fan triangulated).
fn mesh_import(r: &crate::model::regen::RegenResult, source: &str, format: &str) -> ImportDef {
    let t = &r.topo;
    let faces = t.faces.iter().map(|f| {
        let mut v: Vec<[f32; 6]> = vec![];
        let mut tris: Vec<[u32; 3]> = vec![];
        for &pi in &f.polys {
            let poly = &t.polys[pi];
            let base = v.len() as u32;
            for &i in poly { let p = t.positions[i]; v.push([p.x as f32, p.y as f32, p.z as f32, f.normal.x as f32, f.normal.y as f32, f.normal.z as f32]); }
            for k in 1..poly.len().saturating_sub(1) { tris.push([base, base + k as u32, base + k as u32 + 1]); }
        }
        ImportFace { v, t: tris }
    }).collect();
    ImportDef { source: source.into(), format: format.into(), faces }
}

impl App {
    /// Kopieren: the selected features go to Freo's clipboard.
    pub fn copy_features(&mut self) {
        let ids = self.tree_selection.clone();
        let feats: Vec<Feature> = self.data.features.iter().filter(|f| ids.contains(&f.id) && f.id > CSYS_ID).cloned().collect();
        if feats.is_empty() { self.say("KEs im Modellbaum wählen, dann Kopieren.", MsgKind::Prompt); return; }
        self.say(format!("{} KE(s) kopiert. Einfügen legt Kopien an.", feats.len()), MsgKind::Info);
        self.creo.clip_features = feats;
    }

    /// Inserts copies of features at the insert position, then opens the first for re-placement
    /// (Creo's paste opens the feature's dashboard).
    fn insert_copies(&mut self, feats: Vec<Feature>, label: &str) {
        if feats.is_empty() || self.busy() { return; }
        let mut ids = vec![];
        self.commit(label, |d| {
            for f in feats {
                let id = d.next_id;
                d.next_id += 1;
                let name = d.next_name(&f.kind);
                let nf = Feature { id, name, suppressed: false, hidden: false, kind: f.kind };
                match d.insert_index { Some(i) => { d.features.insert(i, nf); d.insert_index = Some(i + 1); } None => d.features.push(nf) }
                ids.push(id);
            }
        });
        self.tree_selection = ids.clone();
        if let Some(first) = ids.first() { self.edit_feature(*first); }
    }

    pub fn paste_features(&mut self) {
        if self.creo.clip_features.is_empty() { self.say("Zuerst KEs kopieren.", MsgKind::Prompt); return; }
        let feats = self.creo.clip_features.clone();
        let n = feats.len();
        self.insert_copies(feats, &format!("{} KE(s) eingefügt — Platzierung im Dashboard anpassen.", n));
    }

    /// Automatische Rundung: a round on every sharp edge of the solid, opened in its dashboard.
    pub fn auto_round(&mut self) {
        let t = self.result.topo.clone();
        // sharp edges of the outer shape; hole and boss circles are left to a manual round
        let sharp: Vec<_> = t.chains.iter().enumerate().filter(|(_, c)| !(c.closed && c.points.len() > 8)).map(|(i, _)| t.edge_ref(i)).filter(|e| e.n1.dot(e.n2) < 0.96).collect();
        if sharp.is_empty() { self.say("Keine scharfen Kanten gefunden.", MsgKind::Info); return; }
        let n = sharp.len();
        self.selection.clear();
        self.start_feature(FeatureKind::Round(crate::model::features::RoundDef { sets: vec![RoundSet::new(1.0)] }));
        if let Some(ed) = &mut self.editor { if let FeatureKind::Round(d) = &mut ed.feature.kind { d.sets[0].edges = sharp; } }
        self.update_preview();
        self.say(format!("Automatische Rundung: {} Kanten. Radius einstellen, dann OK.", n), MsgKind::Prompt);
    }

    /// Körper: a new body, made at once like in Creo; it becomes the default body.
    pub fn new_body(&mut self) {
        if self.busy() || self.in_asm() { return; }
        self.selection.clear();
        self.start_feature(FeatureKind::new_body(BodyOp::New));
        // Körper 1 is the part's first body
        let n = self.data.features.iter().filter(|f| matches!(&f.kind, FeatureKind::Body(b) if matches!(b.op, BodyOp::New | BodyOp::Split))).count() + 2;
        if let Some(ed) = &mut self.editor { ed.feature.name = format!("Körper {}", n); }
        self.finish_feature();
    }

    /// FREO_DEMO_POPUP=surfaces: a chain of surface and body features on the bracket (screenshots).
    pub fn demo_surfaces(&mut self) {
        use crate::model::features::{SurfOp, BodyOp};
        let t = self.result.topo.clone();
        let Some(top) = t.faces.iter().enumerate().filter(|(_, f)| f.planar && f.normal.y > 0.99).max_by(|a, b| a.1.center.y.partial_cmp(&b.1.center.y).unwrap()).map(|f| f.0) else { return };
        let Some(fr) = self.face_ref(Pick::Face(top)) else { return };
        let add = |app: &mut App, kind: FeatureKind| -> u64 {
            let id = app.data.next_id;
            let name = app.data.next_name(&kind);
            app.commit("", |d| { d.features.push(Feature { id, name, suppressed: false, hidden: false, kind }); d.next_id += 1; });
            id
        };
        let surf = |op: SurfOp, f: &dyn Fn(&mut crate::model::features::SurfaceDef)| { let mut k = FeatureKind::new_surface(op); if let FeatureKind::Surface(d) = &mut k { f(d); } k };
        let copy = add(self, surf(SurfOp::Copy, &|d| d.faces = vec![fr]));
        let off = add(self, surf(SurfOp::Offset, &|d| { d.inputs = vec![copy]; d.distance = Dim::v(15.0); }));
        add(self, surf(SurfOp::Thicken, &|d| { d.inputs = vec![off]; d.distance = Dim::v(3.0); }));
        let style = add(self, surf(SurfOp::Style, &|d| { d.plane = Some(PlaneRef::Datum(RIGHT_ID)); d.heights = vec![0.0, 10.0, 10.0, 0.0, 5.0, 25.0, 25.0, 5.0, 5.0, 25.0, 25.0, 5.0, 0.0, 10.0, 10.0, 0.0]; }));
        let free = add(self, surf(SurfOp::Freestyle, &|d| { d.plane = Some(PlaneRef::Datum(TOP_ID)); d.size = [30.0, 24.0, 70.0]; d.level = 2; }));
        add(self, surf(SurfOp::Intersect, &|d| d.plane = Some(PlaneRef::Datum(FRONT_ID))));
        if std::env::var_os("FREO_DEMO_MORE").is_some() {
            let trim = add(self, surf(SurfOp::Trim, &|d| { d.inputs = vec![style]; d.plane = Some(PlaneRef::Datum(TOP_ID)); }));
            let ext = add(self, surf(SurfOp::Extend, &|d| { d.inputs = vec![copy]; d.distance = Dim::v(6.0); }));
            add(self, surf(SurfOp::Merge, &|d| d.inputs = vec![trim, ext]));
            if std::env::var_os("FREO_DEMO_SOLID").is_some() { add(self, surf(SurfOp::Solidify, &|d| d.inputs = vec![free])); }
        }
        let mut k = FeatureKind::new_body(BodyOp::Split);
        if let FeatureKind::Body(d) = &mut k { d.plane = Some(PlaneRef::Datum(RIGHT_ID)); }
        add(self, k);
        let errs: Vec<String> = self.result.errors.iter().map(|(id, e)| format!("{}: {}", self.data.feature(*id).map(|f| f.name.clone()).unwrap_or_default(), e)).collect();
        for e in errs { eprintln!("demo error {}", e); }
    }

    /// Geometriemuster: a pattern of the selected (or last) solid feature.
    pub fn geometry_pattern(&mut self) {
        self.start_feature(FeatureKind::new_pattern());
        self.say("Geometriemuster: Richtung, Anzahl und Abstand der Kopien festlegen.", MsgKind::Prompt);
    }

    pub fn open_file_task(&mut self, task: FileTask) {
        let name = match task {
            FileTask::Udf => "UDF_1".to_string(),
            FileTask::CopyGeom => String::new(),
            FileTask::Shrinkwrap => format!("{}_SW", self.data.name),
        };
        self.creo.file_task = Some(FileTaskDialog { task, sel: None, name });
    }

    fn work_files(&self, ext: &str) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(&self.work_dir).map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext))).collect()).unwrap_or_default();
        v.sort();
        v
    }

    /// Benutzerdefiniertes KE, Kopiergeometrie and Shrinkwrap dialogs.
    pub fn file_task_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.creo.file_task.take() else { return };
        let (title, ext) = match d.task { FileTask::Udf => ("Benutzerdefiniertes KE", "gph"), FileTask::CopyGeom => ("Kopiergeometrie", "pfprt"), FileTask::Shrinkwrap => ("Shrinkwrap", "pfprt") };
        let files = self.work_files(ext);
        let (mut open, mut run, mut save_udf) = (true, false, false);
        egui::Window::new(title).id(egui::Id::new("file_task")).title_bar(false).collapsible(false).resizable(false).default_pos(pos2(460.0, 160.0)).show(ctx, |ui| {
            if dialog_header(ui, title) { open = false; }
            ui.set_width(380.0);
            ui.add_space(6.0);
            let hint = match d.task {
                FileTask::Udf => "UDF-Dateien (.gph) im Arbeitsverzeichnis. Einfügen legt ihre KEs als Kopien an; die gewählten KEs lassen sich als neues UDF speichern.",
                FileTask::CopyGeom => "Ein Teil aus dem Arbeitsverzeichnis wählen; seine Geometrie wird als Kopiergeometrie-KE eingefügt.",
                FileTask::Shrinkwrap => "Die Außenhaut des Modells wird als neues Teil im Arbeitsverzeichnis gespeichert.",
            };
            ui.label(RichText::new(hint).size(13.0).color(LABEL));
            ui.add_space(4.0);
            if d.task != FileTask::Shrinkwrap {
                egui::Frame::NONE.fill(egui::Color32::WHITE).stroke(egui::Stroke::new(1.0_f32, BORDER)).inner_margin(4.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                        if files.is_empty() { ui.label(RichText::new(format!("Keine .{}-Dateien in {}", ext, self.work_dir.display())).color(DIM_TEXT)); }
                        for f in &files {
                            let name = f.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                                icons::paint(ui.painter(), r, if d.task == FileTask::Udf { Icon::UdfLib } else { Icon::Part }, true);
                                let resp = ui.selectable_label(d.sel.as_ref() == Some(f), name);
                                if resp.clicked() { d.sel = Some(f.clone()); }
                                if resp.double_clicked() { d.sel = Some(f.clone()); run = true; }
                            });
                        }
                    });
                });
            }
            if d.task != FileTask::CopyGeom {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(if d.task == FileTask::Udf { "Neues UDF:" } else { "Name des neuen Teils:" });
                    ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(170.0));
                    if d.task == FileTask::Udf && ui.add_enabled(!self.tree_selection.is_empty(), egui::Button::new("Aus Auswahl speichern")).clicked() { save_udf = true; }
                });
            }
            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("Abbrechen").min_size(vec2(100.0, 28.0))).clicked() { open = false; }
                    let (label, enabled) = match d.task { FileTask::Shrinkwrap => ("Erzeugen", !d.name.trim().is_empty()), _ => ("Einfügen", d.sel.is_some()) };
                    if ui.add_enabled(enabled, egui::Button::new(label).min_size(vec2(100.0, 28.0))).clicked() { run = true; }
                });
            });
        });
        let clean = |s: &str| -> String { s.trim().to_uppercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect() };
        if save_udf {
            let feats: Vec<Feature> = self.data.features.iter().filter(|f| self.tree_selection.contains(&f.id) && f.id > CSYS_ID).cloned().collect();
            let name = clean(&d.name);
            let p = self.work_dir.join(format!("{}.gph", name.to_lowercase()));
            match serde_json::to_string_pretty(&UdfFile { name: name.clone(), features: feats }).map_err(|e| e.to_string()).and_then(|s| std::fs::write(&p, s).map_err(|e| e.to_string())) {
                Ok(_) => { self.say(format!("UDF {} gespeichert ({}).", name, p.display()), MsgKind::Success); d.sel = Some(p); }
                Err(e) => self.say(format!("UDF nicht gespeichert: {}", e), MsgKind::Error),
            }
        }
        if run {
            match d.task {
                FileTask::Udf => {
                    let Some(p) = d.sel.clone() else { return };
                    match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<UdfFile>(&s).map_err(|e| e.to_string())) {
                        Ok(u) => { let n = u.features.len(); self.insert_copies(u.features, &format!("UDF {} eingefügt ({} KE).", u.name, n)); open = false; }
                        Err(e) => self.say(format!("UDF nicht lesbar: {}", e), MsgKind::Error),
                    }
                }
                FileTask::CopyGeom => {
                    let Some(p) = d.sel.clone() else { return };
                    match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<ModelData>(&s).map_err(|e| e.to_string())) {
                        Ok(md) => {
                            let r = crate::model::regen::run(&md);
                            if r.topo.faces.is_empty() { self.say("Das gewählte Teil hat keine Geometrie.", MsgKind::Warning); }
                            else {
                                let def = mesh_import(&r, &format!("{}.pfprt", md.name), "Kopiergeometrie");
                                let id = self.data.next_id;
                                let fname = format!("Kopiergeometrie {}", md.name);
                                self.commit(&format!("Geometrie von {} kopiert.", md.name), |dd| {
                                    let f = Feature { id, name: fname, suppressed: false, hidden: false, kind: FeatureKind::Import(def) };
                                    match dd.insert_index { Some(i) => { dd.features.insert(i, f); dd.insert_index = Some(i + 1); } None => dd.features.push(f) }
                                    dd.next_id += 1;
                                });
                                self.fit_pending = true;
                                open = false;
                            }
                        }
                        Err(e) => self.say(format!("Teil nicht lesbar: {}", e), MsgKind::Error),
                    }
                }
                FileTask::Shrinkwrap => {
                    if self.result.topo.faces.is_empty() { self.say("Das Modell hat noch keine Geometrie.", MsgKind::Warning); }
                    else {
                        let name = clean(&d.name);
                        let mut md = ModelData::new_part(&name);
                        let def = mesh_import(&self.result, &format!("{}.pfprt", self.data.name), "Shrinkwrap");
                        let id = md.next_id;
                        md.features.push(Feature { id, name: "Shrinkwrap 1".into(), suppressed: false, hidden: false, kind: FeatureKind::Import(def) });
                        md.next_id += 1;
                        let p = self.work_dir.join(format!("{}.pfprt", name.to_lowercase()));
                        match serde_json::to_string_pretty(&md).map_err(|e| e.to_string()).and_then(|s| std::fs::write(&p, s).map_err(|e| e.to_string())) {
                            Ok(_) => { self.say(format!("Shrinkwrap {} gespeichert: {}", name, p.display()), MsgKind::Success); open = false; }
                            Err(e) => self.say(format!("Shrinkwrap nicht gespeichert: {}", e), MsgKind::Error),
                        }
                    }
                }
            }
        }
        if open { self.creo.file_task = Some(d); }
    }

    /// FREO_DEMO_POPUP=phase2b: a test part with Eckfase, variable draft, pattern table,
    /// Zug-Verbund and Volumenzug (regeneration errors go to stderr).
    pub fn demo_phase2b(&mut self) {
        use crate::model::features::*;
        use crate::sketch::sketch::Sketch;
        self.new_part_named("TEST2B");
        let add = |app: &mut App, kind: FeatureKind| -> u64 {
            let id = app.data.next_id;
            let name = app.data.next_name(&kind);
            app.commit("", |d| { d.features.push(Feature { id, name, suppressed: false, hidden: false, kind }); d.next_id += 1; });
            id
        };
        let ext = |plane: PlaneRef, sk: Sketch, depth: f64| { let mut k = FeatureKind::new_extrude(); if let FeatureKind::Extrude(d) = &mut k { d.plane = Some(plane); d.sketch = sk; d.depth = Dim::v(depth); } k };
        add(self, ext(PlaneRef::Datum(TOP_ID), Sketch::rect(60.0, 40.0, 0.0, 0.0), 30.0));
        // Eckfase at the corner with the largest x+y+z
        let t = self.result.topo.clone();
        let corner = t.chains.iter().filter(|c| !c.closed).flat_map(|c| [c.points[0], *c.points.last().unwrap()]).max_by(|a, b| (a.x + a.y + a.z).partial_cmp(&(b.x + b.y + b.z)).unwrap());
        if let Some(cv) = corner {
            let at: Vec<usize> = t.chains.iter().enumerate().filter(|(_, c)| !c.closed && ((c.points[0] - cv).length() < 1e-6 || (*c.points.last().unwrap() - cv).length() < 1e-6)).map(|(i, _)| i).take(2).collect();
            let mut k = FeatureKind::new_chamfer();
            if let FeatureKind::Chamfer(c) = &mut k { c.corner = true; c.d1 = Dim::v(8.0); c.edges = at.iter().map(|i| t.edge_ref(*i)).collect(); }
            add(self, k);
        }
        // variable draft on the faces facing ±Z
        let t = self.result.topo.clone();
        let faces: Vec<FaceRef> = t.faces.iter().enumerate().filter(|(_, f)| f.planar && f.normal.z.abs() > 0.99).filter_map(|(i, _)| self.face_ref(Pick::Face(i))).collect();
        let mut k = FeatureKind::new_draft();
        if let FeatureKind::Draft(d) = &mut k { d.faces = faces; d.hinge = Some(PlaneRef::Datum(TOP_ID)); d.angle = Dim::v(2.0); d.angle2 = Some(Dim::v(8.0)); }
        add(self, k);
        // a pin on top, patterned by a table
        let top = PlaneRef::Face(FaceRef { point: V3::new(0.0, 30.0, 0.0), normal: V3::Y });
        let pin = add(self, ext(top, Sketch::circle(4.0, -20.0, 0.0), 10.0));
        let mut k = FeatureKind::new_pattern();
        if let FeatureKind::Pattern(d) = &mut k { d.features = vec![pin]; d.kind = PatternKind::Table; d.table = vec![[15.0, 0.0, 0.0], [25.0, 0.0, 8.0], [40.0, 0.0, -8.0]]; }
        add(self, k);
        // Zug-Verbund: circle into rectangle along a bent path, beside the block
        let mut tr = Sketch::default();
        let (a, b, c) = (tr.add_point(V2::new(80.0, 0.0)), tr.add_point(V2::new(80.0, 40.0)), tr.add_point(V2::new(120.0, 70.0)));
        tr.add_line(a, b); tr.add_line(b, c);
        let mut k = FeatureKind::new_sweep();
        if let FeatureKind::Sweep(d) = &mut k { d.plane = Some(PlaneRef::Datum(FRONT_ID)); d.trajectory = tr.clone(); d.section = Sketch::circle(8.0, 0.0, 0.0); d.section2 = Sketch::rect(20.0, 10.0, 0.0, 0.0); d.mode = SweepMode::Blend; }
        add(self, k);
        // Volumenzug: a square tool moved (not turned) along a bent path, on the other side
        let mut tr = Sketch::default();
        let (a, b, c) = (tr.add_point(V2::new(-80.0, 0.0)), tr.add_point(V2::new(-80.0, 40.0)), tr.add_point(V2::new(-120.0, 70.0)));
        tr.add_line(a, b); tr.add_line(b, c);
        let mut k = FeatureKind::new_sweep();
        if let FeatureKind::Sweep(d) = &mut k { d.plane = Some(PlaneRef::Datum(FRONT_ID)); d.trajectory = tr; d.section = Sketch::rect(12.0, 12.0, 0.0, 0.0); d.mode = SweepMode::Volume; }
        add(self, k);
        self.fit_pending = true;
        let errs: Vec<String> = self.result.errors.iter().map(|(id, e)| format!("{}: {}", self.data.feature(*id).map(|f| f.name.clone()).unwrap_or_default(), e)).collect();
        for e in errs { eprintln!("demo error {}", e); }
    }

    /// Zug-Verbund / Volumenzug-KE: a sweep in that mode (Volumenzug removes material).
    pub fn start_sweep(&mut self, mode: crate::model::features::SweepMode) {
        let mut k = FeatureKind::new_sweep();
        if let FeatureKind::Sweep(d) = &mut k { d.mode = mode; d.remove = mode == crate::model::features::SweepMode::Volume; }
        self.start_feature(k);
    }

    /// Mustertabelle bearbeiten: the selected pattern (or a new one) as a table of instance offsets.
    pub fn pattern_table(&mut self) {
        use crate::model::features::PatternKind;
        let sel = self.tree_selection.iter().copied().find(|id| matches!(self.data.feature(*id).map(|f| &f.kind), Some(FeatureKind::Pattern(_))));
        match sel {
            Some(id) => self.edit_feature(id),
            None => self.start_feature(FeatureKind::new_pattern()),
        }
        if let Some(ed) = &mut self.editor {
            if let FeatureKind::Pattern(d) = &mut ed.feature.kind {
                if d.kind == PatternKind::Direction && d.table.is_empty() {
                    let (n, s) = (d.count1.value(&self.result.vars, 3.0).round().max(2.0) as usize, d.spacing1.value(&self.result.vars, 20.0));
                    let u = match d.dir1 { AxisRef::WorldY => V3::Y, AxisRef::WorldZ => V3::Z, _ => V3::X };
                    d.table = (1..n).map(|k| { let p = u * (s * k as f64); [p.x, p.y, p.z] }).collect();
                }
                d.kind = PatternKind::Table;
            }
        }
        self.update_preview();
        self.say("Mustertabelle: eine Zeile je Instanz mit ihrer Verschiebung.", MsgKind::Prompt);
    }

    /// Komponentenschnittstelle: placement references this part brings into assemblies.
    pub fn interface_dialog(&mut self, ctx: &egui::Context) {
        use crate::model::assembly::AsmCType;
        use crate::model::features::{IfaceItem, IfaceRef};
        if !self.creo.iface_dialog { return; }
        let mut open = true;
        let mut items = self.data.interface.clone();
        let mut add = false;
        egui::Window::new("Komponentenschnittstelle").id(egui::Id::new("iface")).open(&mut open).collapsible(false).resizable(false).default_width(440.0).default_pos(pos2(440.0, 160.0)).show(ctx, |ui| {
            ui.label(RichText::new("Referenzen, mit denen das Teil in Baugruppen platziert wird. Beim Zusammenbauen sind sie vorgewählt; nur die Partner in der Baugruppe werden noch gewählt.").size(12.5).color(LABEL));
            ui.add_space(4.0);
            egui::Grid::new("iface_grid").num_columns(3).striped(true).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
                for h in ["Referenz", "Bedingung", ""] { ui.label(RichText::new(h).strong()); }
                ui.end_row();
                let mut del = None;
                for (i, it) in items.iter_mut().enumerate() {
                    ui.label(&it.name);
                    egui::ComboBox::from_id_salt(("iface_ty", i)).width(150.0).selected_text(it.ty.name()).show_ui(ui, |ui| {
                        for t in [AsmCType::Coincident, AsmCType::Distance, AsmCType::Parallel, AsmCType::Normal, AsmCType::Angle] { ui.selectable_value(&mut it.ty, t, t.name()); }
                    });
                    if ui.small_button("✕").clicked() { del = Some(i); }
                    ui.end_row();
                }
                if let Some(i) = del { items.remove(i); }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.selection.is_empty(), egui::Button::new("Gewählte Referenzen hinzufügen")).on_hover_text("Ebenen, ebene oder zylindrische Flächen, Achsen oder Punkte im Grafikfenster wählen").clicked() { add = true; }
                if self.selection.is_empty() { ui.label(RichText::new("Referenzen im Modell wählen").color(DIM_TEXT)); }
            });
        });
        if add {
            let t = self.result.topo.clone();
            for p in self.selection.clone() {
                let it = match p {
                    Pick::Plane(id) => Some((IfaceRef::Plane(id), self.data.feature(id).map(|f| f.name.clone()).unwrap_or_default())),
                    Pick::Axis(id) => Some((IfaceRef::Axis(id), self.result.axis(id).map(|a| a.name.clone()).unwrap_or_default())),
                    Pick::Point(id) => Some((IfaceRef::Point(id), self.data.feature(id).map(|f| f.name.clone()).unwrap_or_default())),
                    Pick::Face(f) => self.face_ref(p).map(|fr| (IfaceRef::Surface { face: fr, planar: t.faces.get(f).map(|x| x.planar).unwrap_or(true) }, if t.faces.get(f).map(|x| x.planar).unwrap_or(true) { "Ebene Fläche".to_string() } else { "Zylinderfläche".to_string() })),
                    _ => None,
                };
                if let Some((r, name)) = it { if !items.iter().any(|x| x.r == r) { items.push(IfaceItem { ty: AsmCType::Coincident, r, name }); } }
            }
        }
        if items != self.data.interface { self.commit("Komponentenschnittstelle geändert.", |d| d.interface = items); }
        self.creo.iface_dialog = open;
    }

    /// Regenerierungsmanager: every feature with its regeneration status.
    pub fn regen_manager(&mut self, ctx: &egui::Context) {
        if !self.creo.regen_manager { return; }
        let mut open = true;
        let mut edit = None;
        let mut regen = false;
        let rolled = self.result.rolled_back.clone();
        egui::Window::new("Regenerierungsmanager").id(egui::Id::new("regen_mgr")).open(&mut open).collapsible(false).default_width(460.0).default_pos(pos2(440.0, 150.0)).show(ctx, |ui| {
            let fails = self.result.errors.len();
            ui.label(RichText::new(if fails == 0 { "Alle KEs regeneriert.".to_string() } else { format!("{} KE(s) fehlgeschlagen.", fails) }).color(if fails == 0 { egui::Color32::from_rgb(20, 130, 50) } else { egui::Color32::from_rgb(200, 36, 28) }));
            egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                egui::Grid::new("regen_grid").num_columns(3).striped(true).spacing(vec2(14.0, 4.0)).show(ui, |ui| {
                    for h in ["KE", "Typ", "Status"] { ui.label(RichText::new(h).strong()); }
                    ui.end_row();
                    for f in &self.data.features {
                        let (status, col) = if f.suppressed { ("Unterdrückt", DIM_TEXT) }
                            else if let Some(e) = self.result.errors.get(&f.id) { (e.as_str(), egui::Color32::from_rgb(200, 36, 28)) }
                            else if rolled.contains(&f.id) { ("Nach Einfügeposition", DIM_TEXT) }
                            else { ("Regeneriert", egui::Color32::from_rgb(20, 130, 50)) };
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                            icons::paint(ui.painter(), r, icons::for_feature(&f.kind), true);
                            if ui.selectable_label(self.tree_selection.contains(&f.id), &f.name).double_clicked() { edit = Some(f.id); }
                        });
                        ui.label(f.kind.type_name());
                        ui.label(RichText::new(status).color(col));
                        ui.end_row();
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Regenerieren").clicked() { regen = true; }
                let first_fail = self.data.features.iter().find(|f| self.result.errors.contains_key(&f.id)).map(|f| f.id);
                if ui.add_enabled(first_fail.is_some(), egui::Button::new("Fehlerhaftes KE editieren")).clicked() { edit = first_fail; }
            });
        });
        if regen { self.regenerate(true); }
        if let Some(id) = edit { self.creo.regen_manager = false; self.edit_feature(id); return; }
        self.creo.regen_manager = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shrinkwrap and Kopiergeometrie: the copied mesh, saved and read back, rebuilds the same shape.
    #[test]
    fn mesh_import_rebuilds_the_solid() {
        let src = crate::model::regen::run(&super::super::demo::bracket());
        assert!(!src.topo.faces.is_empty());
        let mut md = ModelData::new_part("WRAP");
        let id = md.next_id;
        md.features.push(Feature { id, name: "Shrinkwrap 1".into(), suppressed: false, hidden: false, kind: FeatureKind::Import(mesh_import(&src, "bracket.pfprt", "Shrinkwrap")) });
        md.next_id += 1;
        let md: ModelData = serde_json::from_str(&serde_json::to_string(&md).unwrap()).unwrap();
        let copy = crate::model::regen::run(&md);
        assert!(copy.errors.is_empty(), "{:?}", copy.errors);
        assert!((copy.topo.bbox.diagonal() - src.topo.bbox.diagonal()).abs() < 1e-3);
        assert!((copy.topo.bbox.center() - src.topo.bbox.center()).length() < 1e-3);
    }
}
