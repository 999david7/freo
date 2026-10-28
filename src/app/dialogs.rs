//! Floating windows: parameters, relations, mass properties, appearance, model info,
//! measure results, controls, about and the message log.

use super::*;
use eframe::egui::{self, Color32, RichText};

impl App {
    pub fn dialogs(&mut self, ctx: &egui::Context) {
        let mut open = self.windows.parameters;
        egui::Window::new("π Parameter").open(&mut open).default_width(460.0).show(ctx, |ui| self.parameters_ui(ui));
        self.windows.parameters = open;

        let mut open = self.windows.relations;
        egui::Window::new("ƒ Beziehungen").open(&mut open).default_width(460.0).show(ctx, |ui| self.relations_ui(ui));
        self.windows.relations = open;

        let mut open = self.windows.family;
        egui::Window::new("▦ Familientabelle").open(&mut open).default_width(560.0).show(ctx, |ui| self.family_ui(ui));
        self.windows.family = open;

        let mut open = self.windows.mass;
        egui::Window::new("⚖ Masseeigenschaften").open(&mut open).default_width(380.0).show(ctx, |ui| self.mass_ui(ui));
        self.windows.mass = open;

        let mut open = self.windows.appearance;
        egui::Window::new("🎨 Darstellung").open(&mut open).show(ctx, |ui| {
            let mut a = self.data.appearance;
            ui.horizontal(|ui| { ui.label("Farbe"); ui.color_edit_button_rgb(&mut a.color); });
            ui.add(egui::Slider::new(&mut a.opacity, 0.1..=1.0).text("Deckkraft"));
            ui.label("Vorgaben");
            ui.horizontal_wrapped(|ui| {
                for (n, c) in [("Stahl", [0.62, 0.68, 0.76]), ("Aluminium", [0.8, 0.82, 0.85]), ("Messing", [0.85, 0.7, 0.35]), ("Kupfer", [0.8, 0.45, 0.3]),
                    ("ABS schwarz", [0.2, 0.2, 0.22]), ("Rot", [0.8, 0.2, 0.18]), ("Blau", [0.25, 0.45, 0.85]), ("Grün", [0.3, 0.65, 0.35])] {
                    let col = Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8);
                    if ui.add(egui::Button::new(RichText::new(n).color(Color32::WHITE)).fill(col)).clicked() { a.color = c; }
                }
            });
            if a != self.data.appearance { self.data.appearance = a; self.dirty = true; }
        });
        self.windows.appearance = open;

        let mut open = self.windows.info;
        egui::Window::new("ℹ Modellinfo").open(&mut open).default_width(520.0).show(ctx, |ui| self.info_ui(ui));
        self.windows.info = open;

        if self.measure {
            let mut open = true;
            egui::Window::new("📏 Messen").title_bar(false).default_pos(egui::pos2(260.0, 200.0)).show(ctx, |ui| {
                if super::creo::dialog_header(ui, "📏 Messen") { open = false; }
                self.measure_ui(ui)
            });
            if !open { self.measure = false; self.selection.clear(); }
        }

        let mut open = self.windows.controls;
        egui::Window::new("⌨ Steuerung").open(&mut open).show(ctx, |ui| {
            egui::Grid::new("ctl").striped(true).show(ui, |ui| {
                for (a, b) in [
                    ("Drehen", "Mittlere Taste ziehen · Rechte Taste ziehen · ⌥ + ziehen · ⌥ + Zwei-Finger-Scrollen"),
                    ("Verschieben", "⇧ + mittlere/rechte Taste ziehen · ⇧ + Zwei-Finger-Scrollen"),
                    ("Zoomen", "Mausrad · Zusammenziehen · ⌃ + mittlere Taste ziehen"),
                    ("Auswählen / hinzufügen", "Klicken / ⇧-Klick oder ⌘-Klick"),
                    ("Definition editieren", "Fläche oder Baumelement doppelklicken"),
                    ("Kontextmenü", "Rechtsklick"),
                    ("Rückgängig / Wiederholen", "⌘Z / ⇧⌘Z"),
                    ("Regenerieren", "⌘G"),
                    ("Standardausrichtung / Einpassen", "⌘D / ⌘F"),
                    ("KE abschließen / abbrechen", "Enter / Esc"),
                    ("Skizzierer", "Esc beendet eine Kette, dann zurück zu Auswählen; Entf löscht die Auswahl; Bemaßung doppelklicken zum Ändern"),
                ] { ui.label(RichText::new(a).strong()); ui.label(b); ui.end_row(); }
            });
        });
        self.windows.controls = open;

        let mut open = self.windows.about;
        egui::Window::new("Über Freo").open(&mut open).show(ctx, |ui| {
            ui.heading("Freo");
            ui.label("Ein parametrischer, KE-basierter 3D-CAD-Modellierer im Stil von Creo für macOS, geschrieben in Rust.");
            ui.label("Geometriekern: BSP-Baum-CSG-B-Rep mit Topologiereparatur, Skizzierer mit Bedingungslöser, historienbasierte Regenerierung.");
            ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).weak());
        });
        self.windows.about = open;

        let mut open = self.windows.messages;
        egui::Window::new("Meldungsprotokoll").open(&mut open).default_width(520.0).default_height(300.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                for m in &self.messages {
                    let c = match m.kind { MsgKind::Error => Color32::from_rgb(200, 40, 30), MsgKind::Warning => Color32::from_rgb(200, 130, 0), MsgKind::Success => Color32::from_rgb(20, 140, 60), MsgKind::Prompt => Color32::from_rgb(20, 110, 220), MsgKind::Info => Color32::GRAY };
                    ui.label(RichText::new(&m.text).color(c));
                }
            });
        });
        self.windows.messages = open;
    }

    fn parameters_ui(&mut self, ui: &mut egui::Ui) {
        ui.label("Benutzerparameter steuern Bemaßungen: einen Parameternamen oder Ausdruck in ein Bemaßungsfeld eingeben.");
        let vars = self.result.vars.clone();
        let mut params = self.data.parameters.clone();
        let mut remove = None;
        egui::Grid::new("params").striped(true).num_columns(5).show(ui, |ui| {
            ui.label(RichText::new("Name").strong()); ui.label(RichText::new("Wert / Ausdruck").strong()); ui.label(RichText::new("Ergebnis").strong()); ui.label(RichText::new("Beschreibung").strong()); ui.label(""); ui.end_row();
            for (i, p) in params.iter_mut().enumerate() {
                ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(80.0));
                ui.add(egui::TextEdit::singleline(&mut p.expr).desired_width(120.0));
                match vars.get(&p.name.to_uppercase()) { Some(v) => ui.label(fmt_num(*v, 4)), None => ui.label(RichText::new("error").color(Color32::RED)) };
                ui.add(egui::TextEdit::singleline(&mut p.note).desired_width(120.0));
                if ui.small_button("🗑").clicked() { remove = Some(i); }
                ui.end_row();
            }
        });
        if let Some(i) = remove { params.remove(i); }
        ui.horizontal(|ui| {
            if ui.button("＋ Parameter hinzufügen").clicked() {
                let n = (1..).map(|k| format!("P{}", k)).find(|n| !params.iter().any(|p| &p.name == n)).unwrap();
                params.push(Parameter { name: n, expr: "10".into(), note: String::new() });
            }
        });
        if params != self.data.parameters { self.commit("", |d| d.parameters = params); }
        for e in &self.result.param_errors { ui.label(RichText::new(e).color(Color32::RED)); }
    }

    /// Creo's family table editor: instances of the part as rows, chosen parameters and
    /// features as columns (`*` = generic value, J/N = feature present or not).
    fn family_ui(&mut self, ui: &mut egui::Ui) {
        if self.in_asm() || self.drawing_active() { ui.label("Familientabellen werden in Teilen definiert."); return; }
        let mut fam = self.data.family.clone();
        let vars = self.result.vars.clone();
        let mut save = None;
        ui.label(format!("Generisches Modell: {}   •   {} Instanz(en)", self.data.name, fam.instances.len()));
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("famcol").width(180.0).selected_text("＋ Spalte hinzufügen").show_ui(ui, |ui| {
                let mut add = None;
                for p in &self.data.parameters {
                    let c = FamilyColumn::Parameter(p.name.clone());
                    if !fam.columns.contains(&c) && ui.selectable_label(false, format!("Parameter  {}", p.name)).clicked() { add = Some(c); }
                }
                ui.separator();
                // user features (the template's datums and BASIS use ids below 10)
                for f in self.data.features.iter().filter(|f| f.id >= 10) {
                    let c = FamilyColumn::Feature(f.id);
                    if !fam.columns.contains(&c) && ui.selectable_label(false, format!("KE  {}", f.name)).clicked() { add = Some(c); }
                }
                if let Some(c) = add {
                    fam.columns.push(c);
                    for inst in &mut fam.instances { inst.values.push("*".into()); }
                }
            });
            if ui.button("＋ Instanz").on_hover_text("Eine Zeile für eine neue Instanz einfügen").clicked() {
                let base = self.data.name.to_uppercase();
                let name = (1..).map(|k| format!("{}_INST{}", base, k)).find(|n| !fam.instances.iter().any(|i| &i.name == n)).unwrap();
                fam.instances.push(FamilyInstance { name, values: vec!["*".into(); fam.columns.len()] });
            }
            if fam.active.is_some() && ui.button("Generisch anzeigen").clicked() { fam.active = None; }
        });
        if fam.columns.is_empty() {
            ui.label(RichText::new("Zuerst Parameter oder KEs als Spalten hinzufügen; dann je Instanz einen Wert eintragen (* = Wert des generischen Modells, J/N = KE vorhanden).").color(Color32::GRAY));
        }
        let col_name = |c: &FamilyColumn| match c {
            FamilyColumn::Parameter(n) => n.clone(),
            FamilyColumn::Feature(id) => self.data.feature(*id).map(|f| f.name.clone()).unwrap_or(format!("KE {}", id)),
        };
        let mut drop_col = None;
        let mut drop_row = None;
        egui::ScrollArea::horizontal().show(ui, |ui| {
            egui::Grid::new("family").striped(true).spacing([6.0, 4.0]).show(ui, |ui| {
                ui.label("");
                ui.label(RichText::new("Instanzname").strong());
                for (k, c) in fam.columns.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(col_name(c)).strong()).on_hover_text(if matches!(c, FamilyColumn::Feature(_)) { "KE: J = vorhanden, N = unterdrückt" } else { "Parameterwert oder Ausdruck" });
                        if ui.small_button("✕").on_hover_text("Spalte entfernen").clicked() { drop_col = Some(k); }
                    });
                }
                ui.label("");
                ui.end_row();
                if ui.radio(fam.active.is_none(), "").on_hover_text("Generisches Modell anzeigen").clicked() { fam.active = None; }
                ui.label(RichText::new(format!("{} (generisch)", self.data.name)).italics());
                for c in &fam.columns {
                    ui.label(match c {
                        FamilyColumn::Parameter(n) => self.data.parameters.iter().find(|p| p.name.eq_ignore_ascii_case(n)).map(|p| p.expr.clone()).unwrap_or("?".into()),
                        FamilyColumn::Feature(id) => if self.data.feature(*id).map(|f| f.suppressed).unwrap_or(true) { "N".into() } else { "J".into() },
                    });
                }
                ui.label("");
                ui.end_row();
                for (i, inst) in fam.instances.iter_mut().enumerate() {
                    if ui.radio(fam.active == Some(i), "").on_hover_text("Diese Instanz im Grafikfenster anzeigen").clicked() { fam.active = Some(i); }
                    ui.add(egui::TextEdit::singleline(&mut inst.name).desired_width(130.0));
                    while inst.values.len() < fam.columns.len() { inst.values.push("*".into()); }
                    for (k, c) in fam.columns.iter().enumerate() {
                        let v = &mut inst.values[k];
                        let t = v.trim().to_string();
                        let bad = match c {
                            FamilyColumn::Parameter(_) => t != "*" && !t.is_empty() && crate::model::expr::eval(&t, &vars).is_err(),
                            FamilyColumn::Feature(_) => !matches!(t.to_uppercase().as_str(), "*" | "" | "J" | "N" | "Y" | "JA" | "NEIN" | "YES" | "NO" | "0" | "1"),
                        };
                        let mut te = egui::TextEdit::singleline(v).desired_width(60.0);
                        if bad { te = te.text_color(Color32::from_rgb(200, 30, 30)); }
                        ui.add(te);
                    }
                    ui.horizontal(|ui| {
                        if ui.small_button("💾").on_hover_text("Instanz als eigenes Teil speichern").clicked() { save = Some(i); }
                        if ui.small_button("🗑").on_hover_text("Instanz löschen").clicked() { drop_row = Some(i); }
                    });
                    ui.end_row();
                }
            });
        });
        if let Some(k) = drop_col {
            fam.columns.remove(k);
            for inst in &mut fam.instances { if k < inst.values.len() { inst.values.remove(k); } }
        }
        if let Some(i) = drop_row {
            fam.instances.remove(i);
            fam.active = match fam.active { Some(a) if a == i => None, Some(a) if a > i => Some(a - 1), a => a };
        }
        if fam != self.data.family {
            let msg = match fam.active { Some(a) if fam.active != self.data.family.active => format!("Instanz {} wird angezeigt.", fam.instances[a].name), _ => String::new() };
            self.commit(&msg, |d| d.family = fam);
        }
        for (i, inst) in self.data.family.instances.iter().enumerate() {
            if let Err(e) = self.data.instance(i) { ui.label(RichText::new(format!("{}: {}", inst.name, e)).color(Color32::RED)); }
        }
        if let Some(i) = save { self.save_instance(i); }
    }

    /// Writes family table instance `i` as a part file next to the generic.
    pub fn save_instance(&mut self, i: usize) {
        let inst = match self.data.instance(i) { Ok(m) => m, Err(e) => { self.say(e, MsgKind::Error); return; } };
        let dir = self.path.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| self.browser_dir.clone());
        let p = dir.join(format!("{}.pfprt", inst.name.to_lowercase()));
        match serde_json::to_string_pretty(&inst).map_err(|e| e.to_string()).and_then(|s| std::fs::write(&p, s).map_err(|e| e.to_string())) {
            Ok(_) => { self.say(format!("Instanz {} als {} gespeichert.", inst.name, p.display()), MsgKind::Success); self.add_recent(&p); }
            Err(e) => self.say(format!("Speichern fehlgeschlagen: {}", e), MsgKind::Error),
        }
    }

    fn relations_ui(&mut self, ui: &mut egui::Ui) {
        ui.label("Eine Beziehung pro Zeile, z. B.  H = W / 3   oder   N = ceil(L / 25)   (/* Kommentare erlaubt)");
        let mut draft = self.relations_draft.clone().unwrap_or_else(|| self.data.relations.clone());
        ui.add(egui::TextEdit::multiline(&mut draft).code_editor().desired_rows(10).desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            if ui.button("✔ Prüfen").clicked() {
                let mut d = self.data.clone();
                d.relations = draft.clone();
                let (_, errs) = d.evaluate_parameters();
                if errs.is_empty() { self.say("Beziehungen erfolgreich geprüft.", MsgKind::Success); } else { for e in errs { self.say(e, MsgKind::Error); } }
            }
            if ui.button("Anwenden & regenerieren").clicked() {
                let r = draft.clone();
                self.commit("Beziehungen aktualisiert.", |d| d.relations = r);
                self.relations_draft = None;
                return;
            }
            if ui.button("Zurücksetzen").clicked() { draft = self.data.relations.clone(); }
        });
        let mut keys: Vec<_> = self.result.vars.iter().collect();
        keys.sort_by(|a, b| a.0.cmp(b.0));
        ui.collapsing("Aktuelle Werte", |ui| { for (k, v) in keys { ui.label(format!("{} = {}", k, fmt_num(*v, 4))); } });
        self.relations_draft = Some(draft);
    }

    fn mass_ui(&mut self, ui: &mut egui::Ui) {
        if self.in_asm() {
            ui.label(RichText::new("Masseeigenschaften der Baugruppe — jede Komponente verwendet die Dichte ihres Teils.").size(11.5));
            let Some(m) = self.asm_mass() else { ui.label("Die Baugruppe hat keine Volumenkomponenten."); return };
            Self::mass_grid(ui, &m);
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Dichte (kg/mm³)");
            let mut d = self.data.density;
            if ui.add(egui::DragValue::new(&mut d).speed(1e-7).range(1e-9..=1e-3).custom_formatter(|v, _| format!("{:.3e}", v))).changed() { self.data.density = d; self.dirty = true; }
            egui::ComboBox::from_id_salt("mat").selected_text("Werkstoff").show_ui(ui, |ui| {
                for (n, v) in [("Stahl", 7.85e-6), ("Edelstahl", 8.0e-6), ("Aluminium", 2.7e-6), ("Titan", 4.43e-6), ("Messing", 8.5e-6), ("ABS", 1.04e-6), ("PLA", 1.24e-6)] {
                    if ui.button(n).clicked() { self.data.density = v; self.dirty = true; }
                }
            });
        });
        let m = export::mass_properties(&self.result, self.data.density);
        Self::mass_grid(ui, &m);
    }

    fn mass_grid(ui: &mut egui::Ui, m: &export::MassProps) {
        egui::Grid::new("mass").striped(true).show(ui, |ui| {
            let row = |ui: &mut egui::Ui, a: &str, b: String| { ui.label(RichText::new(a).strong()); ui.label(b); ui.end_row(); };
            row(ui, "Volumen", format!("{} mm³", fmt_num(m.volume, 3)));
            row(ui, "Oberfläche", format!("{} mm²", fmt_num(m.area, 3)));
            row(ui, "Masse", format!("{} kg", fmt_num(m.mass, 5)));
            row(ui, "Schwerpunkt", format!("({}, {}, {}) mm", fmt_num(m.cog.x, 3), fmt_num(m.cog.y, 3), fmt_num(m.cog.z, 3)));
            row(ui, "Ixx Iyy Izz @SP", format!("{} {} {} kg·mm²", fmt_num(m.inertia[0], 3), fmt_num(m.inertia[1], 3), fmt_num(m.inertia[2], 3)));
            row(ui, "Ixy Iyz Izx @SP", format!("{} {} {} kg·mm²", fmt_num(m.inertia[3], 3), fmt_num(m.inertia[4], 3), fmt_num(m.inertia[5], 3)));
            let s = m.bbox.size();
            row(ui, "Begrenzungsrahmen", format!("{} × {} × {} mm", fmt_num(s.x, 3), fmt_num(s.y, 3), fmt_num(s.z, 3)));
        });
    }

    fn info_ui(&mut self, ui: &mut egui::Ui) {
        if self.in_asm() { self.asm_info_ui(ui); return; }
        let r = &self.result;
        ui.label(format!("Teil {} — {} KEs, {} Flächen, {} Kanten, {} Polygone. Regeneriert in {:.0} ms.", self.data.name, self.data.features.len(), r.topo.faces.len(), r.topo.chains.len(), r.topo.polys.len(), r.time * 1000.0));
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            egui::Grid::new("info").striped(true).show(ui, |ui| {
                for h in ["#", "Name", "Typ", "Status", "Eltern"] { ui.label(RichText::new(h).strong()); }
                ui.end_row();
                for (i, f) in self.data.features.iter().enumerate() {
                    ui.label(format!("{}", i + 1));
                    ui.label(&f.name);
                    ui.label(f.kind.type_name());
                    let st = if f.suppressed { RichText::new("Unterdrückt").color(Color32::GRAY) } else if r.rolled_back.contains(&f.id) { RichText::new("Zurückgesetzt").color(Color32::GRAY) } else if let Some(e) = r.errors.get(&f.id) { RichText::new(format!("Fehlgeschlagen: {}", e)).color(Color32::RED) } else { RichText::new("Regeneriert").color(Color32::from_rgb(20, 140, 60)) };
                    ui.label(st);
                    let parents: Vec<String> = f.kind.references().iter().filter_map(|id| self.data.feature(*id).map(|p| p.name.clone())).collect();
                    ui.label(parents.join(", "));
                    ui.end_row();
                }
            });
        });
    }

    fn measure_ui(&mut self, ui: &mut egui::Ui) {
        let r = self.displayed();
        let t = &r.topo;
        enum G { Plane(V3, f64), Pts(Vec<V3>), Line(V3, V3) }
        let geo = |p: &Pick| -> Option<(String, G)> {
            match *p {
                Pick::Face(f) => {
                    let face = t.faces.get(f)?;
                    let pts: Vec<V3> = face.polys.iter().flat_map(|&pi| t.polys[pi].iter().map(|&i| t.positions[i])).collect();
                    Some((format!("Fläche — Inhalt {} mm²", fmt_num(face.area, 3)), if face.planar { G::Plane(face.normal, face.offset) } else { G::Pts(pts) }))
                }
                Pick::Edge(e) => {
                    let c = t.chains.get(e)?;
                    let mut s = format!("Kante — Länge {} mm", fmt_num(c.length(), 3));
                    if c.closed && c.points.len() > 8 {
                        let cen = c.points.iter().fold(V3::ZERO, |a, b| a + *b) / c.points.len() as f64;
                        let rad = c.points.iter().map(|p| (*p - cen).length()).sum::<f64>() / c.points.len() as f64;
                        s += &format!(", ⌀ {} mm", fmt_num(rad * 2.0, 3));
                    }
                    Some((s, G::Pts(c.points.clone())))
                }
                Pick::Plane(id) => { let p = r.plane(id)?; Some((format!("Ebene {}", p.name), G::Plane(p.frame.z(), p.frame.z().dot(p.frame.origin)))) }
                Pick::Axis(id) => { let a = r.axis(id)?; Some((format!("Achse {}", a.name), G::Line(a.point, a.dir))) }
                Pick::Point(id) => { let p = r.points.iter().find(|q| q.id == id)?; Some((format!("Punkt {}", p.name), G::Pts(vec![p.p]))) }
                Pick::Feature(_) => None,
            }
        };
        let items: Vec<(String, G)> = self.selection.iter().filter_map(geo).collect();
        if items.is_empty() { ui.label("Ein oder zwei Elemente wählen."); }
        for (s, _) in &items { ui.label(s); }
        if items.len() == 2 {
            let (a, b) = (&items[0].1, &items[1].1);
            let pts_of = |g: &G| -> Vec<V3> { match g { G::Pts(p) => p.clone(), G::Line(p, d) => (-50..=50).map(|k| *p + *d * (k as f64 * 2.0)).collect(), G::Plane(n, w) => vec![*n * *w] } };
            let txt = match (a, b) {
                (G::Plane(n1, w1), G::Plane(n2, w2)) => {
                    let ang = n1.dot(*n2).clamp(-1.0, 1.0).acos().to_degrees();
                    if ang < 0.01 || ang > 179.99 { format!("Abstand {} mm (parallel)", fmt_num((w1 - w2 * n1.dot(*n2).signum()).abs(), 4)) } else { format!("Winkel {}°", fmt_num(ang.min(180.0 - ang), 4)) }
                }
                (G::Plane(n, w), g) | (g, G::Plane(n, w)) => {
                    let d = pts_of(g).iter().map(|p| (p.dot(*n) - w).abs()).fold(f64::INFINITY, f64::min);
                    format!("Abstand {} mm", fmt_num(d, 4))
                }
                (G::Line(p1, d1), G::Line(p2, d2)) => {
                    let c = d1.cross(*d2);
                    if c.length() < 1e-9 { format!("Abstand {} mm (parallel)", fmt_num((*p2 - *p1).cross(*d1).length(), 4)) }
                    else { format!("Abstand {} mm, Winkel {}°", fmt_num(((*p2 - *p1).dot(c) / c.length()).abs(), 4), fmt_num(d1.dot(*d2).abs().clamp(0.0, 1.0).acos().to_degrees(), 3)) }
                }
                (ga, gb) => {
                    let (pa, pb) = (pts_of(ga), pts_of(gb));
                    let mut best = f64::INFINITY;
                    for p in &pa { for q in &pb { best = best.min((*p - *q).length()); } }
                    format!("Minimaler Abstand ≈ {} mm", fmt_num(best, 4))
                }
            };
            ui.separator();
            ui.label(RichText::new(txt).strong().size(15.0));
        }
        if ui.button("Schließen").clicked() { self.measure = false; self.selection.clear(); }
    }
}
