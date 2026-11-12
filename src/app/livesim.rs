//! Live-Simulation: the ribbon tab, studies with their loads and constraints, the solver job
//! running beside the UI and the result display (fringe plot, deformation, stream lines,
//! legend, probes, report).

use super::icons::{self, Icon};
use super::render::ModelBuffers;
use super::ribbon::{drop_menu, group, group_drop, group_menu, large, menu_row, split, stack, theme::*, tiny};
use super::*;
use crate::model::sim::*;
use crate::sim::run::{self, Shared, SimInput, SimOutput};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, RichText, Sense, Stroke, StrokeKind};
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DefScale { Auto, True, Factor(f32), Off }

pub struct Job { pub shared: Arc<Shared>, pub key: u64, pub started: std::time::Instant }

#[derive(Clone)]
pub struct ItemEdit { pub item: SimItem, pub is_new: bool, pub picking: bool }

#[derive(Clone, Copy)]
pub struct Probe { pub tri: usize, pub bary: [f32; 3] }

pub struct SavedAnalysis { pub name: String, pub when: String, pub out: Arc<SimOutput> }

pub struct SimUi {
    /// Leistungsoptionen: 0 = Geschwindigkeit, 1 = Genauigkeit
    pub fidelity: f32,
    pub paused: bool,
    pub job: Option<Job>,
    pub out: Option<Arc<SimOutput>>,
    /// what `out` was computed for (study, geometry, fidelity)
    pub out_key: u64,
    pub error: Option<(u64, String)>,
    pub field: usize,
    pub show_results: bool,
    pub animate: bool,
    pub scale: DefScale,
    pub show_objects: bool,
    /// Abweichung prüfen: the undeformed outline over the deformed result
    pub deviation: bool,
    pub probing: bool,
    pub probes: Vec<Probe>,
    pub collect: bool,
    pub history: Vec<(String, Vec<(String, String)>)>,
    pub saved: Vec<SavedAnalysis>,
    /// a saved analysis is on screen instead of the live result
    pub viewing_saved: bool,
    pub saved_open: bool,
    pub perf_open: bool,
    pub report_open: bool,
    pub settings_open: bool,
    pub edit: Option<ItemEdit>,
    pub sel_item: Option<u64>,
    buffers: Option<(u64, Arc<ModelBuffers>)>,
    counter: u64,
    clock: f64,
    /// slider position in Leistungsoptionen (applied when released)
    fidelity_ui: f32,
}

impl Default for SimUi {
    fn default() -> Self {
        SimUi {
            fidelity: 0.35, paused: false, job: None, out: None, out_key: 0, error: None, field: 0, show_results: true, animate: false,
            scale: DefScale::Auto, show_objects: true, deviation: false, probing: false, probes: vec![], collect: false, history: vec![], saved: vec![],
            viewing_saved: false, saved_open: false, perf_open: false, report_open: false, settings_open: false, edit: None, sel_item: None, buffers: None, counter: 0, clock: 0.0, fidelity_ui: 0.35,
        }
    }
}

fn sig(v: f32) -> String {
    if !v.is_finite() { return "–".into(); }
    let a = v.abs();
    if a == 0.0 { "0".into() } else if a >= 1e5 || a < 1e-3 { format!("{:.2e}", v) } else if a >= 100.0 { format!("{:.1}", v) } else if a >= 10.0 { format!("{:.2}", v) } else if a >= 1.0 { format!("{:.3}", v) } else { format!("{:.4}", v) }
}

/// Fringe colours from blue (low) over green and yellow to red (high).
pub fn ramp(t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0) * 4.0;
    let (i, f) = (t.floor().min(3.0), t - t.floor().min(3.0));
    match i as i32 { 0 => [0.0, f, 1.0], 1 => [0.0, 1.0, 1.0 - f], 2 => [f, 1.0, 0.0], _ => [1.0, 1.0 - f, 0.0] }
}

fn ramp32(t: f32) -> Color32 { let c = ramp(t); Color32::from_rgb((c[0] * 235.0 + 10.0) as u8, (c[1] * 225.0 + 10.0) as u8, (c[2] * 235.0 + 10.0) as u8) }

fn line(v: &mut Vec<f32>, a: V3, b: V3, c: [f32; 4]) {
    v.extend_from_slice(&[a.x as f32, a.y as f32, a.z as f32, c[0], c[1], c[2], c[3], b.x as f32, b.y as f32, b.z as f32, c[0], c[1], c[2], c[3]]);
}

fn arrow(v: &mut Vec<f32>, from: V3, to: V3, side: V3, c: [f32; 4]) {
    let d = to - from;
    let l = d.length();
    if l < 1e-9 { return; }
    let u = d / l;
    let s = safe_norm(u.cross(side), safe_norm(u.cross(V3::Y), V3::X));
    let t = u.cross(s);
    line(v, from, to, c);
    for w in [s, -s, t, -t] { line(v, to, to - u * (l * 0.28) + w * (l * 0.10), c); }
}

fn text_w(ui: &egui::Ui, t: &str, size: f32) -> f32 { ui.fonts(|f| f.layout_no_wrap(t.to_string(), FontId::proportional(size), Color32::BLACK).size().x) }

/// Menu row with a check box in the icon column (Creo's drop-down check items).
fn menu_check(ui: &mut egui::Ui, checked: bool, label: &str, enabled: bool) -> bool {
    let w = (text_w(ui, label, 14.0) + 62.0).max(ui.available_width()).max(200.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if enabled && resp.hovered() { p.rect_filled(rect, 0.0, HOVER_BG); }
    p.line_segment([pos2(rect.left() + 36.0, rect.top()), pos2(rect.left() + 36.0, rect.bottom())], Stroke::new(1.0_f32, Color32::from_rgb(226, 228, 230)));
    let b = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(14.0, 14.0));
    let col = if enabled { Color32::from_rgb(90, 96, 104) } else { Color32::from_rgb(196, 200, 204) };
    p.rect_filled(b, 1.0, Color32::WHITE);
    p.rect_stroke(b, 1.0, Stroke::new(1.0_f32, col), StrokeKind::Inside);
    if checked { p.add(egui::Shape::line(vec![b.left_center() + vec2(3.0, 0.0), b.center_bottom() + vec2(-1.0, -3.5), b.right_top() + vec2(-3.0, 3.0)], Stroke::new(1.8_f32, if enabled { Color32::from_rgb(40, 44, 50) } else { col }))); }
    p.text(pos2(rect.left() + 46.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(14.0), if enabled { TEXT } else { DIM_TEXT });
    resp.clicked()
}

/// Entry of the "new study" menu: large icon, bold title and a line of explanation.
fn study_row(ui: &mut egui::Ui, icon: Icon, title: &str, text: &str, enabled: bool) -> bool {
    let w = (text_w(ui, text, 14.5) + 96.0).max(ui.available_width());
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 64.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if enabled && resp.hovered() { p.rect_filled(rect, 0.0, HOVER_BG); }
    icons::paint(p, Rect::from_center_size(pos2(rect.left() + 38.0, rect.center().y), vec2(38.0, 38.0)), icon, enabled);
    let (tc, dc) = if enabled { (TEXT, Color32::from_rgb(70, 70, 74)) } else { (DIM_TEXT, DIM_TEXT) };
    let tp = pos2(rect.left() + 80.0, rect.top() + 20.0);
    p.text(tp, Align2::LEFT_CENTER, title, FontId::proportional(15.0), tc);
    p.text(tp + vec2(0.6, 0.0), Align2::LEFT_CENTER, title, FontId::proportional(15.0), tc);
    p.text(pos2(rect.left() + 80.0, rect.top() + 44.0), Align2::LEFT_CENTER, text, FontId::proportional(14.5), dc);
    resp.clicked()
}

fn item_icon(k: ItemKind) -> Icon {
    match k {
        ItemKind::Fixed => Icon::Fixed, ItemKind::Displacement => Icon::Displacement, ItemKind::Frictionless => Icon::Frictionless, ItemKind::Symmetry => Icon::Symmetry,
        ItemKind::Force => Icon::Force, ItemKind::Moment => Icon::Moment, ItemKind::Pressure => Icon::Pressure, ItemKind::Gravity => Icon::Gravity,
        ItemKind::Centrifugal => Icon::Centrifugal, ItemKind::LinAccel => Icon::LinAccel, ItemKind::Temperature => Icon::Temperature, ItemKind::Convection => Icon::Convection,
        ItemKind::HeatFlow => Icon::HeatFlow, ItemKind::HeatFlux => Icon::HeatFlux, ItemKind::Inlet => Icon::Inlet, ItemKind::Outlet => Icon::Outlet, ItemKind::Wind => Icon::Wind,
    }
}

fn study_icon(k: StudyKind) -> Icon {
    match k { StudyKind::Structural => Icon::StudyStruct, StudyKind::Thermal => Icon::StudyThermal, StudyKind::Modal => Icon::StudyModal, StudyKind::Fluid => Icon::StudyFluid }
}

fn item_tip(k: ItemKind) -> &'static str {
    match k {
        ItemKind::Fixed => "Hält die gewählten Flächen in allen Richtungen fest.",
        ItemKind::Displacement => "Gibt den gewählten Flächen eine Verschiebung in X, Y und Z vor.",
        ItemKind::Frictionless => "Die Fläche kann gleiten, aber nicht senkrecht zu sich ausweichen.",
        ItemKind::Symmetry => "Symmetrieebene: keine Verschiebung senkrecht zur Fläche.",
        ItemKind::Force => "Verteilt eine Kraft gleichmäßig auf die gewählten Flächen.",
        ItemKind::Moment => "Bringt ein Moment um den Schwerpunkt der gewählten Flächen auf.",
        ItemKind::Pressure => "Gleichmäßiger Druck senkrecht auf die gewählten Flächen.",
        ItemKind::Gravity => "Eigengewicht des Teils in der gewählten Richtung.",
        ItemKind::Centrifugal => "Fliehkraft durch Drehung um eine Achse durch den Ursprung.",
        ItemKind::LinAccel => "Trägheitslast eines gleichmäßig beschleunigten Teils.",
        ItemKind::Temperature => "Hält die gewählten Flächen auf einer Temperatur.",
        ItemKind::Convection => "Wärmeübergang der gewählten Flächen an die Umgebung.",
        ItemKind::HeatFlow => "Wärmeleistung, die über die gewählten Flächen zufließt.",
        ItemKind::HeatFlux => "Wärmestromdichte auf den gewählten Flächen.",
        ItemKind::Inlet => "Öffnung, durch die das Fluid mit einer Geschwindigkeit einströmt (ebene Stirnfläche um die Öffnung wählen).",
        ItemKind::Outlet => "Öffnung, durch die das Fluid bei Umgebungsdruck ausströmt (ebene Stirnfläche um die Öffnung wählen).",
        ItemKind::Wind => "Externe Strömung: das Teil wird aus einer Richtung angeströmt.",
    }
}

impl App {
    // ------------------------------------------------------------ state

    pub fn sim_study(&self) -> Option<&Study> { self.data.sims.get(self.data.sim_active) }

    fn sim_ready(&self) -> bool { !self.in_asm() && !self.busy() && !self.result.topo.polys.is_empty() }

    /// The item dialog is collecting faces: clicks in the graphics window go to it.
    pub fn sim_picking(&self) -> bool { self.tab == Tab::LiveSim && self.sim.edit.as_ref().is_some_and(|e| e.picking && e.item.kind.needs_faces()) }

    /// Results are drawn in place of the model.
    pub fn sim_showing(&self) -> bool {
        self.tab == Tab::LiveSim && self.sim.show_results && self.sim.out.is_some() && !self.busy() && !self.in_asm() && !self.sim_picking()
    }

    fn sim_key(&self) -> Option<u64> {
        let s = self.sim_study()?;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(s).ok()?.hash(&mut h);
        self.result.generation.hash(&mut h);
        self.sim.fidelity.to_bits().hash(&mut h);
        Some(h.finish())
    }

    fn sim_start(&mut self, key: u64) {
        let Some(study) = self.sim_study().cloned() else { return };
        if let Some(j) = self.sim.job.take() { j.shared.cancel.store(true, std::sync::atomic::Ordering::Relaxed); }
        self.sim.error = None;
        self.sim.viewing_saved = false;
        let shared = run::spawn(SimInput { topo: self.result.topo.clone(), study, fidelity: self.sim.fidelity });
        self.sim.job = Some(Job { shared, key, started: std::time::Instant::now() });
    }

    /// Simulieren: solve now (also after a pause or an error).
    pub fn sim_run_now(&mut self) {
        self.sim.paused = false;
        self.sim.error = None;
        self.sim.out_key = 0;
        self.sim.viewing_saved = false;
    }

    fn sim_take(&mut self, out: SimOutput, key: u64) {
        let partial = out.partial;
        let keep = self.sim.out.as_ref().and_then(|o| o.fields.get(self.sim.field).map(|f| f.name.clone()));
        let out = Arc::new(out);
        // stay on the same quantity when the study is solved again
        self.sim.field = keep.and_then(|n| out.fields.iter().position(|f| f.name == n || (f.name.starts_with("Mode") && n.starts_with(f.name.split(':').next().unwrap_or("?"))))).unwrap_or(0);
        if self.sim.out.as_ref().map_or(true, |o| o.surface.pos.len() != out.surface.pos.len()) { self.sim.probes.clear(); }
        self.sim.out = Some(out.clone());
        self.sim.buffers = None;
        if partial { return; }
        self.sim.out_key = key;
        let name = self.sim_study().map(|s| s.name.clone()).unwrap_or_default();
        let head: Vec<String> = out.summary.iter().filter(|s| s.0.starts_with("Max.") || s.0.starts_with("Mode 1") || s.0.starts_with("Druckabfall") || s.0.starts_with("Kraft auf")).take(2).map(|s| format!("{} {}", s.0, s.1)).collect();
        self.say(format!("{} gelöst ({:.1} s): {}.", name, out.seconds, head.join(", ")), MsgKind::Success);
        for w in out.warnings.clone() { self.say(w, MsgKind::Warning); }
        if self.sim.collect { self.sim.history.push((name, out.summary.clone())); }
    }

    /// Per frame: collects finished results and solves again when the study or the model changed.
    pub fn sim_tick(&mut self, ctx: &egui::Context) {
        if let Some(job) = &self.sim.job {
            let key = job.key;
            let done = job.shared.done.lock().unwrap().take();
            let partial = job.shared.partial.lock().unwrap().take();
            match done {
                Some(Ok(out)) => { self.sim.job = None; self.sim_take(out, key); }
                Some(Err(e)) => {
                    self.sim.job = None;
                    if e != "abgebrochen" { self.say(format!("Simulation: {}", e), MsgKind::Error); self.sim.error = Some((key, e)); }
                }
                None => { if let Some(p) = partial { self.sim_take(p, key); } ctx.request_repaint_after(std::time::Duration::from_millis(60)); }
            }
        }
        if self.tab != Tab::LiveSim { return; }
        if self.sim.animate && self.sim_showing() { ctx.request_repaint(); }
        if self.sim.paused || self.sim.viewing_saved || self.sim.edit.is_some() || !self.sim_ready() { return; }
        let Some(key) = self.sim_key() else { return };
        if self.sim.study_empty(self.sim_study()) { return; }
        let failed = self.sim.error.as_ref().is_some_and(|e| e.0 == key);
        let running = self.sim.job.as_ref().is_some_and(|j| j.key == key);
        if key != self.sim.out_key && !failed && !running { self.sim_start(key); }
    }

    // ------------------------------------------------------------ editing

    pub fn sim_new_study(&mut self, kind: StudyKind) {
        if !self.sim_ready() { return; }
        let id = self.data.sims.iter().map(|s| s.id).max().unwrap_or(0) + 1;
        let n = self.data.sims.iter().filter(|s| s.kind == kind).count() + 1;
        let name = format!("{} {}", kind.short(), n);
        let label = format!("{} erzeugt. Randbedingungen und Lasten hinzufügen.", kind.title());
        self.commit(&label, |d| { d.sims.push(Study::new(id, name, kind)); d.sim_active = d.sims.len() - 1; });
        self.sim_reset_view();
    }

    fn sim_reset_view(&mut self) {
        if let Some(j) = self.sim.job.take() { j.shared.cancel.store(true, std::sync::atomic::Ordering::Relaxed); }
        self.sim.out = None;
        self.sim.out_key = 0;
        self.sim.error = None;
        self.sim.buffers = None;
        self.sim.probes.clear();
        self.sim.field = 0;
        self.sim.viewing_saved = false;
        self.sim.sel_item = None;
        self.sim.show_results = true;
    }

    pub fn sim_activate(&mut self, i: usize) {
        if i >= self.data.sims.len() || i == self.data.sim_active { return; }
        self.data.sim_active = i;
        self.sim_reset_view();
    }

    fn sim_delete_study(&mut self) {
        let i = self.data.sim_active;
        if i >= self.data.sims.len() { return; }
        let name = self.data.sims[i].name.clone();
        self.commit(&format!("{} gelöscht.", name), |d| { d.sims.remove(i); d.sim_active = i.min(d.sims.len().saturating_sub(1)); });
        self.sim_reset_view();
        self.sim.settings_open = false;
    }

    pub fn sim_new_item(&mut self, kind: ItemKind) {
        let Some(st) = self.sim_study() else { return };
        let mut it = SimItem::new(st.next_item.max(1), st.item_name(kind), kind);
        if kind.needs_faces() {
            // object-action: faces selected beforehand become the references
            let topo = &self.result.topo;
            it.faces = self.selection.iter().filter_map(|p| if let Pick::Face(f) = p { Some(SimFace::of(topo, *f)) } else { None }).collect();
            if matches!(kind, ItemKind::Inlet | ItemKind::Outlet) { it.faces.retain(|f| f.planar); }
        }
        let picking = kind.needs_faces();
        self.sim.edit = Some(ItemEdit { item: it, is_new: true, picking });
        self.selection.clear();
        self.mini = None;
        if picking { self.say(format!("{}: Flächen im Grafikfenster wählen (Klicken zum Hinzufügen/Entfernen).", kind.name()), MsgKind::Prompt); }
    }

    pub fn sim_edit_item(&mut self, id: u64) {
        let Some(it) = self.sim_study().and_then(|s| s.items.iter().find(|i| i.id == id)).cloned() else { return };
        self.sim.edit = Some(ItemEdit { item: it, is_new: false, picking: false });
        self.sim.sel_item = Some(id);
    }

    pub fn sim_delete_item(&mut self, id: u64) {
        let a = self.data.sim_active;
        let Some(name) = self.sim_study().and_then(|s| s.items.iter().find(|i| i.id == id)).map(|i| i.name.clone()) else { return };
        self.commit(&format!("{} gelöscht.", name), |d| if let Some(s) = d.sims.get_mut(a) { s.items.retain(|i| i.id != id); });
        self.sim.sel_item = None;
    }

    fn sim_finish_item(&mut self) {
        let Some(ed) = self.sim.edit.take() else { return };
        let a = self.data.sim_active;
        let it = ed.item.clone();
        let label = format!("{} {}.", it.name, if ed.is_new { "erzeugt" } else { "geändert" });
        self.commit(&label, |d| {
            let Some(s) = d.sims.get_mut(a) else { return };
            match s.items.iter_mut().find(|i| i.id == it.id) { Some(slot) => *slot = it.clone(), None => { s.next_item = s.next_item.max(it.id) + 1; s.items.push(it.clone()); } }
        });
        self.sim.sel_item = Some(ed.item.id);
    }

    /// Test hooks: OK of the item dialog, and the CSV export to a given file.
    pub fn sim_finish_item_for_test(&mut self) { self.sim_finish_item(); }
    pub fn sim_export_csv_to_for_test(&mut self, p: &std::path::Path) {
        if let Some(out) = self.sim.out.clone() { let _ = std::fs::write(p, format!("{}\n{} rows", out.table.0.join(";"), out.table.1.len())); }
    }

    /// Faces of an item in the current geometry.
    pub fn sim_item_faces(&self, it: &SimItem) -> Vec<usize> { it.faces.iter().filter_map(|f| f.resolve(&self.result.topo)).collect() }

    /// Faces to highlight: those of the item in the dialog or selected in the simulation tree.
    pub fn sim_highlight(&self) -> Vec<usize> {
        if self.tab != Tab::LiveSim { return vec![]; }
        if let Some(ed) = &self.sim.edit { return self.sim_item_faces(&ed.item); }
        if self.sim_showing() { return vec![]; }
        self.sim.sel_item.and_then(|id| self.sim_study().and_then(|s| s.items.iter().find(|i| i.id == id))).map(|i| self.sim_item_faces(i)).unwrap_or_default()
    }

    /// A click in the graphics window: collects faces for the item dialog or sets a probe.
    pub fn sim_click(&mut self, px: Option<(f32, f32)>) -> bool {
        if self.tab != Tab::LiveSim { return false; }
        let Some(px) = px else { return false };
        if self.sim_picking() {
            if let Some(Pick::Face(f)) = self.pick_at(px) {
                let sf = SimFace::of(&self.result.topo, f);
                let topo = self.result.topo.clone();
                let Some(ed) = &mut self.sim.edit else { return true };
                if matches!(ed.item.kind, ItemKind::Inlet | ItemKind::Outlet) && !sf.planar { self.say("Ein- und Auslässe brauchen eine ebene Fläche.", MsgKind::Warning); return true; }
                match ed.item.faces.iter().position(|x| x.resolve(&topo) == Some(f)) { Some(i) => { ed.item.faces.remove(i); } None => ed.item.faces.push(sf) }
            }
            return true;
        }
        if self.sim.probing && self.sim_showing() {
            if let Some(p) = self.sim_probe_at(px) { self.sim.probes.push(p); } else { self.say("Simulations-Taster: auf das Modell klicken.", MsgKind::Prompt); }
            return true;
        }
        false
    }

    fn sim_probe_at(&self, px: (f32, f32)) -> Option<Probe> {
        let out = self.sim.out.as_ref()?;
        let (o, d) = self.camera.ray(px, self.viewport_size);
        let (fac, set) = self.sim_factor(out);
        let s = &out.surface;
        let at = |i: usize| {
            let mut p = V3::new(s.pos[i][0] as f64, s.pos[i][1] as f64, s.pos[i][2] as f64);
            if let Some(ds) = set.and_then(|k| out.disps.get(k)) { let q = ds.d[i]; p += V3::new(q[0] as f64, q[1] as f64, q[2] as f64) * fac as f64; }
            p
        };
        let mut best: Option<(f64, Probe)> = None;
        for t in 0..s.pos.len() / 3 {
            let (a, b, c) = (at(3 * t), at(3 * t + 1), at(3 * t + 2));
            let (e1, e2) = (b - a, c - a);
            let pv = d.cross(e2);
            let det = e1.dot(pv);
            if det.abs() < 1e-14 { continue; }
            let sv = o - a;
            let u = sv.dot(pv) / det;
            if !(0.0..=1.0).contains(&u) { continue; }
            let q = sv.cross(e1);
            let v = d.dot(q) / det;
            if v < 0.0 || u + v > 1.0 { continue; }
            let tt = e2.dot(q) / det;
            if best.as_ref().map_or(true, |b| tt < b.0) { best = Some((tt, Probe { tri: t, bary: [(1.0 - u - v) as f32, u as f32, v as f32] })); }
        }
        best.map(|b| b.1)
    }

    // ------------------------------------------------------------ display

    /// Deformation factor right now and the displacement set it applies to.
    fn sim_factor(&self, out: &SimOutput) -> (f32, Option<usize>) {
        let Some(set) = out.fields.get(self.sim.field).and_then(|f| f.disp) else { return (0.0, None) };
        let Some(ds) = out.disps.get(set) else { return (0.0, None) };
        let diag = self.result.topo.bbox.diagonal() as f32;
        let auto = if ds.max > 0.0 { 0.06 * diag / ds.max } else { 0.0 };
        let base = match self.sim.scale { DefScale::Auto => auto, DefScale::True if !ds.harmonic => 1.0, DefScale::True => auto, DefScale::Factor(f) if !ds.harmonic => f, DefScale::Factor(f) => auto * f / 10.0, DefScale::Off => 0.0 };
        let t = self.sim_time();
        let f = if !self.sim.animate { 1.0 } else if ds.harmonic { (t * std::f64::consts::TAU / 1.6).sin() as f32 } else { (0.5 - 0.5 * (t * std::f64::consts::TAU / 2.4).cos()) as f32 };
        (base * f, Some(set))
    }

    fn sim_time(&self) -> f64 { self.sim.clock }

    /// Result picture for the graphics window: fringe-coloured (and deformed) surface buffers,
    /// plus stream lines and the load / constraint symbols as lines.
    pub fn sim_frame(&mut self, lines: &mut Vec<f32>) -> Option<(Arc<ModelBuffers>, f32)> {
        if self.tab != Tab::LiveSim || self.in_asm() { return None; }
        if self.sim.show_objects && !self.busy() { self.sim_glyphs(lines); }
        if !self.sim_showing() { return None; }
        let out = self.sim.out.clone()?;
        let field = out.fields.get(self.sim.field)?;
        let (fac, set) = self.sim_factor(&out);
        let s = &out.surface;
        let ds = set.and_then(|k| out.disps.get(k));
        // stream lines in the colours of the speed legend
        if !out.lines.is_empty() {
            let vmax = out.fields.first().map(|f| f.max).unwrap_or(1.0).max(1e-9);
            for l in &out.lines {
                for w in l.windows(2) {
                    let c = ramp((w[0].1 + w[1].1) * 0.5 / vmax);
                    line(lines, w[0].0, w[1].0, [c[0], c[1], c[2], 0.95]);
                }
            }
        }
        if self.sim.deviation && fac != 0.0 {
            for e in s.edges.chunks(2) { line(lines, V3::new(e[0][0] as f64, e[0][1] as f64, e[0][2] as f64), V3::new(e[1][0] as f64, e[1][1] as f64, e[1][2] as f64), [0.1, 0.1, 0.12, 0.55]); }
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (Arc::as_ptr(&out) as usize, self.sim.field, fac.to_bits()).hash(&mut h);
        let key = h.finish();
        if let Some((k, b)) = &self.sim.buffers { if *k == key { return Some((b.clone(), if out.see_through { 0.35 } else { 1.0 })); } }
        let span = (field.max - field.min).max(1e-30);
        let mut solid = Vec::with_capacity(s.pos.len() * 9);
        for i in 0..s.pos.len() {
            let (mut p, n) = (s.pos[i], s.nrm[i]);
            if let Some(d) = ds { let q = d.d[i]; p = [p[0] + q[0] * fac, p[1] + q[1] * fac, p[2] + q[2] * fac]; }
            let v = field.vals[i];
            let c = if v.is_finite() { let t = (v - field.min) / span; ramp(if field.low_is_bad { 1.0 - t } else { t }) } else { [0.62, 0.64, 0.67] };
            solid.extend_from_slice(&[p[0], p[1], p[2], n[0], n[1], n[2], c[0], c[1], c[2]]);
        }
        let mut edges = Vec::with_capacity(s.edges.len() * 3);
        for (i, e) in s.edges.iter().enumerate() {
            let mut p = *e;
            if let Some(d) = ds { let q = d.edge_d[i]; p = [p[0] + q[0] * fac, p[1] + q[1] * fac, p[2] + q[2] * fac]; }
            edges.extend_from_slice(&p);
        }
        self.sim.counter += 1;
        let b = Arc::new(ModelBuffers { generation: (1u64 << 62) + self.sim.counter, solid, edges });
        self.sim.buffers = Some((key, b.clone()));
        Some((b, if out.see_through { 0.35 } else { 1.0 }))
    }

    /// Symbols of the active study's loads and constraints.
    fn sim_glyphs(&self, lines: &mut Vec<f32>) {
        let Some(st) = self.sim_study() else { return };
        let topo = &self.result.topo;
        let b = topo.bbox;
        if b.is_empty() { return; }
        let len = b.diagonal() * 0.11;
        let back = self.camera.back();
        const LOAD: [f32; 4] = [0.86, 0.10, 0.62, 1.0];
        const HOLD: [f32; 4] = [0.05, 0.45, 0.85, 1.0];
        const HEAT: [f32; 4] = [0.90, 0.35, 0.05, 1.0];
        const FLOW: [f32; 4] = [0.05, 0.55, 0.75, 1.0];
        let editing = self.sim.edit.as_ref().map(|e| &e.item);
        for it in st.items.iter().filter(|i| editing.map_or(true, |e| e.id != i.id)).chain(editing) {
            // a few anchor points spread over the item's faces
            let mut anchors: Vec<(V3, V3)> = vec![];
            for f in self.sim_item_faces(it) {
                let face = &topo.faces[f];
                let step = (face.polys.len() / 10).max(1);
                if face.planar { anchors.push((face.center, face.normal)); }
                if !face.planar || face.polys.len() > 6 { for &pi in face.polys.iter().step_by(step) { anchors.push((topo.poly_centroid(pi), topo.planes[pi].n)); } }
            }
            let v = V3::from_array(it.vec);
            match it.kind {
                ItemKind::Fixed | ItemKind::Displacement | ItemKind::Frictionless | ItemKind::Symmetry => {
                    for (p, n) in &anchors {
                        let f = Frame::plane_through(*p, *n);
                        let (s, tip) = (len * 0.16, *p + *n * (len * 0.3));
                        for k in 0..4 { let a = k as f64 * std::f64::consts::FRAC_PI_2; let q = tip + (f.x * a.cos() + f.y * a.sin()) * s; line(lines, *p, q, HOLD); line(lines, q, tip + (f.x * (a + 1.5708).cos() + f.y * (a + 1.5708).sin()) * s, HOLD); }
                        if it.kind != ItemKind::Fixed { line(lines, tip - f.x * s * 1.6, tip + f.x * s * 1.6, HOLD); }
                    }
                }
                ItemKind::Force => for (p, _) in anchors.iter().take(1) { let d = safe_norm(v, -V3::Y); arrow(lines, *p - d * len, *p, back, LOAD); arrow(lines, *p - d * len * 1.02, *p - d * 0.02 * len, back, LOAD); },
                ItemKind::Pressure => for (p, n) in &anchors { let s = if it.value >= 0.0 { 1.0 } else { -1.0 }; if s > 0.0 { arrow(lines, *p + *n * (len * 0.6), *p, back, LOAD); } else { arrow(lines, *p, *p + *n * (len * 0.6), back, LOAD); } },
                ItemKind::Moment => for (p, _) in anchors.iter().take(1) {
                    let ax = safe_norm(v, V3::Z);
                    let f = Frame::plane_through(*p, ax);
                    let r = len * 0.55;
                    let pt = |a: f64| *p + (f.x * a.cos() + f.y * a.sin()) * r;
                    for k in 0..18 { line(lines, pt(k as f64 * 0.27), pt((k + 1) as f64 * 0.27), LOAD); }
                    arrow(lines, pt(17.0 * 0.27), pt(18.6 * 0.27), ax, LOAD);
                    arrow(lines, *p, *p + ax * len * 0.8, back, LOAD);
                },
                ItemKind::Gravity | ItemKind::LinAccel => { let d = safe_norm(v, -V3::Y); let c = b.center(); arrow(lines, c, c + d * len * 1.4, back, LOAD); }
                ItemKind::Centrifugal => {
                    let ax = safe_norm(v, V3::Y);
                    let half = b.diagonal() * 0.6;
                    line(lines, -ax * half, ax * half, LOAD);
                    let f = Frame::plane_through(ax * half * 0.9, ax);
                    let pt = |a: f64| f.origin + (f.x * a.cos() + f.y * a.sin()) * len * 0.5;
                    for k in 0..16 { line(lines, pt(k as f64 * 0.3), pt((k + 1) as f64 * 0.3), LOAD); }
                    arrow(lines, pt(15.0 * 0.3), pt(16.6 * 0.3), ax, LOAD);
                }
                ItemKind::Temperature | ItemKind::Convection | ItemKind::HeatFlow | ItemKind::HeatFlux => {
                    for (p, n) in &anchors {
                        let inward = matches!(it.kind, ItemKind::HeatFlow | ItemKind::HeatFlux) && it.value >= 0.0;
                        if inward { arrow(lines, *p + *n * (len * 0.5), *p, back, HEAT); } else if it.kind == ItemKind::Convection { arrow(lines, *p, *p + *n * (len * 0.5), back, HEAT); }
                        else { let f = Frame::plane_through(*p + *n * 0.01, *n); let s = len * 0.14; line(lines, f.origin - f.x * s, f.origin + f.x * s, HEAT); line(lines, f.origin - f.y * s, f.origin + f.y * s, HEAT); line(lines, *p, *p + *n * (len * 0.3), HEAT); }
                    }
                }
                ItemKind::Inlet => for (p, n) in anchors.iter().take(4) { arrow(lines, *p + *n * len, *p + *n * (len * 0.1), back, FLOW); },
                ItemKind::Outlet => for (p, n) in anchors.iter().take(4) { arrow(lines, *p + *n * (len * 0.1), *p + *n * len, back, FLOW); },
                ItemKind::Wind => {
                    let d = safe_norm(v, V3::X);
                    let f = Frame::plane_through(b.center() - d * (b.diagonal() * 0.75), d);
                    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (0.0, 0.0), (-1.0, 1.0), (1.0, 1.0)] { let o = f.origin + (f.x * x + f.y * y) * len * 1.2; arrow(lines, o, o + d * len * 1.6, back, FLOW); }
                }
            }
        }
    }

    /// Legend, probes, item names and the solver status over the graphics window.
    pub fn sim_overlay(&mut self, ctx: &egui::Context, painter: &egui::Painter, rect: Rect, to_screen: &dyn Fn(V3) -> Option<egui::Pos2>) {
        if self.tab != Tab::LiveSim || self.in_asm() { return; }
        self.sim.clock = ctx.input(|i| i.time);
        // names of the loads and constraints next to their symbols
        if self.sim.show_objects && !self.busy() {
            if let Some(st) = self.sim_study() {
                let b = self.result.topo.bbox;
                for it in &st.items {
                    let anchor = match it.kind {
                        ItemKind::Gravity | ItemKind::LinAccel => Some(b.center() + safe_norm(V3::from_array(it.vec), -V3::Y) * b.diagonal() * 0.16),
                        ItemKind::Centrifugal => Some(safe_norm(V3::from_array(it.vec), V3::Y) * b.diagonal() * 0.56),
                        ItemKind::Wind => Some(b.center() - safe_norm(V3::from_array(it.vec), V3::X) * b.diagonal() * 0.78),
                        _ => it.faces.first().and_then(|f| f.resolve(&self.result.topo)).map(|f| { let fc = &self.result.topo.faces[f]; if fc.planar { fc.center + fc.normal * b.diagonal() * 0.04 } else { it.faces[0].face.point + it.faces[0].face.normal * b.diagonal() * 0.04 } }),
                    };
                    if let Some(sp) = anchor.and_then(|a| to_screen(a)) {
                        let sel = self.sim.sel_item == Some(it.id);
                        let g = painter.layout_no_wrap(it.name.clone(), FontId::proportional(12.0), if sel { Color32::from_rgb(10, 90, 30) } else { Color32::from_rgb(30, 30, 34) });
                        let r = Rect::from_min_size(sp + vec2(8.0, -8.0), g.size()).expand2(vec2(4.0, 1.5));
                        painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(255, 255, 255, 205));
                        painter.galley(r.min + vec2(4.0, 1.5), g, Color32::BLACK);
                    }
                }
            }
        }
        // solver status, bottom left
        let mut y = rect.bottom() - 16.0;
        let mut status = |text: String, col: Color32, frac: Option<f32>| {
            let g = painter.layout(text, FontId::proportional(13.0), col, (rect.width() - 60.0).max(200.0));
            let r = Rect::from_min_size(pos2(rect.left() + 14.0, y - g.size().y - 8.0 - if frac.is_some() { 8.0 } else { 0.0 }), g.size() + vec2(16.0, 10.0 + if frac.is_some() { 8.0 } else { 0.0 }));
            painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(255, 255, 255, 225));
            painter.galley(r.min + vec2(8.0, 5.0), g, col);
            if let Some(f) = frac {
                let bar = Rect::from_min_size(pos2(r.left() + 8.0, r.bottom() - 9.0), vec2(r.width() - 16.0, 4.0));
                painter.rect_filled(bar, 2.0, Color32::from_rgb(214, 219, 225));
                painter.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * f.clamp(0.0, 1.0), 4.0)), 2.0, Color32::from_rgb(60, 150, 60));
            }
            y = r.top() - 6.0;
        };
        if let Some(j) = &self.sim.job {
            let f = j.shared.progress();
            status(format!("Simulation läuft – {} … {:.0} %  ({:.0} s)", j.shared.stage.lock().unwrap(), f * 100.0, j.started.elapsed().as_secs_f32()), Color32::from_rgb(40, 44, 50), Some(f));
        } else if let Some((_, e)) = &self.sim.error {
            status(format!("⚠ {}", e), Color32::from_rgb(190, 30, 24), None);
        } else if self.sim.paused && self.sim_study().is_some() {
            status("Simulation angehalten".into(), Color32::from_rgb(150, 90, 0), None);
        } else if self.sim.viewing_saved {
            status("Gespeicherte Analyse wird angezeigt – „Simulieren“ kehrt zur Live-Simulation zurück".into(), Color32::from_rgb(40, 44, 50), None);
        } else if self.sim_study().is_some() && self.sim.out.is_none() && self.sim.edit.is_none() && self.sim.study_empty(self.sim_study()) {
            status("Studie ist leer: Randbedingungen und Lasten im Ribbon hinzufügen".into(), Color32::from_rgb(40, 44, 50), None);
        }
        if !self.sim_showing() { return; }
        let Some(out) = self.sim.out.clone() else { return };
        if self.sim.field >= out.fields.len() { self.sim.field = 0; }
        let Some(field) = out.fields.get(self.sim.field) else { return };
        let (fac, set) = self.sim_factor(&out);
        // probes
        let s = &out.surface;
        for (k, pr) in self.sim.probes.iter().enumerate() {
            if 3 * pr.tri + 2 >= s.pos.len() { continue; }
            let mut p = V3::ZERO;
            let mut v = 0.0;
            for c in 0..3 {
                let i = 3 * pr.tri + c;
                let mut q = V3::new(s.pos[i][0] as f64, s.pos[i][1] as f64, s.pos[i][2] as f64);
                if let Some(ds) = set.and_then(|x| out.disps.get(x)) { let d = ds.d[i]; q += V3::new(d[0] as f64, d[1] as f64, d[2] as f64) * fac as f64; }
                p += q * pr.bary[c] as f64;
                v += field.vals[i] * pr.bary[c];
            }
            if let Some(sp) = to_screen(p) {
                painter.circle(sp, 4.0, Color32::WHITE, Stroke::new(1.5_f32, Color32::BLACK));
                let g = painter.layout_no_wrap(format!("{}: {} {}", k + 1, sig(v), field.unit), FontId::proportional(12.5), Color32::BLACK);
                let r = Rect::from_min_size(sp + vec2(9.0, -22.0), g.size()).expand2(vec2(5.0, 2.0));
                painter.rect(r, 3.0, Color32::from_rgb(255, 255, 225), Stroke::new(1.0_f32, Color32::from_rgb(110, 110, 90)), StrokeKind::Inside);
                painter.galley(r.min + vec2(5.0, 2.0), g, Color32::BLACK);
            }
        }
        // legend, top left
        let names: Vec<String> = out.fields.iter().map(|f| f.name.clone()).collect();
        let mut pick = self.sim.field;
        let mut clear_probes = false;
        egui::Area::new(egui::Id::new("sim_legend")).fixed_pos(rect.left_top() + vec2(14.0, 14.0)).order(egui::Order::Middle).show(ctx, |ui| {
            egui::Frame::NONE.fill(Color32::from_rgba_unmultiplied(255, 255, 255, 232)).corner_radius(5.0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                egui::ComboBox::from_id_salt("sim_field").width(214.0).selected_text(RichText::new(&field.name).size(13.0).color(TEXT)).show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() { ui.selectable_value(&mut pick, i, RichText::new(n).size(13.0)); }
                });
                let (r, _) = ui.allocate_exact_size(vec2(214.0, 206.0), Sense::hover());
                let p = ui.painter();
                let bar = Rect::from_min_size(r.min + vec2(2.0, 8.0), vec2(18.0, 190.0));
                let mut m = egui::Mesh::default();
                let n = 32;
                for i in 0..=n {
                    let t = i as f32 / n as f32;
                    let c = ramp32(if field.low_is_bad { t } else { 1.0 - t });
                    let yy = bar.top() + bar.height() * t;
                    m.colored_vertex(pos2(bar.left(), yy), c);
                    m.colored_vertex(pos2(bar.right(), yy), c);
                    if i > 0 { let b = (2 * i) as u32; m.add_triangle(b - 2, b - 1, b); m.add_triangle(b - 1, b + 1, b); }
                }
                p.add(egui::Shape::mesh(m));
                p.rect_stroke(bar, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(70, 78, 88)), StrokeKind::Outside);
                for i in 0..=8 {
                    let t = i as f32 / 8.0;
                    let yy = bar.top() + bar.height() * t;
                    p.line_segment([pos2(bar.right(), yy), pos2(bar.right() + 4.0, yy)], Stroke::new(1.0_f32, Color32::from_rgb(70, 78, 88)));
                    p.text(pos2(bar.right() + 8.0, yy), Align2::LEFT_CENTER, sig(field.max - (field.max - field.min) * t), FontId::proportional(12.0), TEXT);
                }
                if !field.unit.is_empty() { p.text(pos2(r.right() - 2.0, bar.top()), Align2::RIGHT_CENTER, &field.unit, FontId::proportional(12.0), LABEL); }
                ui.label(RichText::new(format!("Max {} {}   Min {} {}", sig(field.max), field.unit, sig(field.min), field.unit)).size(12.0).color(TEXT));
                if set.is_some() { ui.label(RichText::new(if fac == 0.0 && !self.sim.animate { "Verformung nicht dargestellt".to_string() } else { format!("Verformung {}× überhöht", sig(fac.abs())) }).size(12.0).color(LABEL)); }
                if out.partial { ui.label(RichText::new("Zwischenstand – Berechnung läuft").size(12.0).color(Color32::from_rgb(150, 90, 0))); }
                if !self.sim.probes.is_empty() && ui.add(egui::Button::new(RichText::new("Taster entfernen").size(12.0))).clicked() { clear_probes = true; }
            });
        });
        if pick != self.sim.field { self.sim.field = pick; self.sim.buffers = None; }
        if clear_probes { self.sim.probes.clear(); }
        if self.sim.probing { ctx.set_cursor_icon(egui::CursorIcon::Crosshair); }
    }

    // ------------------------------------------------------------ ribbon

    pub fn live_sim_ribbon(&mut self, ui: &mut egui::Ui) {
        let ready = self.sim_ready();
        let study = self.sim_study().cloned();
        let kind = study.as_ref().map(|s| s.kind);
        let has = study.is_some() && ready && self.sim.edit.is_none();
        let thermal = study.as_ref().is_some_and(|s| s.include_thermal);
        // without a study the commands stay available: the first one creates a structural study
        let can = ready && self.sim.edit.is_none();

        // ---- Studie
        let (new_btn, title) = group_drop(ui, "Studie", |ui| {
            stack(ui, |ui| {
                let r = tiny(ui, Icon::SimPlus, "Neue Simulationsstudie", ready, false, true);
                if tiny(ui, Icon::Simulate, "Studieneinstellungen (Name, Material …)", can, self.sim.settings_open && has, false).clicked() {
                    if !has { self.sim_new_study(StudyKind::Structural); }
                    self.sim.settings_open = !self.sim.settings_open;
                }
                r
            })
        });
        let mut new_kind = None;
        let mut import = false;
        drop_menu(ui, "sim_new", &new_btn, |ui| {
            ui.set_min_width(600.0);
            for k in StudyKind::ALL { if study_row(ui, study_icon(k), k.title(), k.text(), ready) { new_kind = Some(k); } }
            ui.add_space(3.0);
            let r = ui.available_rect_before_wrap();
            ui.painter().line_segment([pos2(r.left() + 6.0, r.top()), pos2(r.right() - 6.0, r.top())], Stroke::new(1.0_f32, SEP));
            ui.add_space(4.0);
            if menu_row(ui, Some(Icon::SimImport), "Vorhandene Simulation importieren", "", ready, false).clicked() { import = true; }
        });
        if let Some(k) = new_kind { self.sim_new_study(k); }
        if import { self.sim_import(); }
        let mut toggle_thermal = false;
        group_menu(ui, "sim_study", &title, |ui| {
            ui.set_min_width(300.0);
            if menu_row(ui, Some(Icon::PerfOptions), "Leistungsoptionen", "", can, false).clicked() { self.sim.perf_open = true; }
            if menu_check(ui, thermal, "Thermische Objekte einschließen", can && matches!(kind, None | Some(StudyKind::Structural))) { toggle_thermal = true; }
        });
        if toggle_thermal {
            if !has { self.sim_new_study(StudyKind::Structural); }
            let a = self.data.sim_active;
            self.commit(if thermal { "Thermische Objekte werden nicht mehr berücksichtigt." } else { "Thermische Objekte eingeschlossen: Temperaturen belasten die Struktur über die Wärmedehnung." }, |d| if let Some(s) = d.sims.get_mut(a) { s.include_thermal = !s.include_thermal; });
        }

        // ---- Setup, Randbedingungen, Lasten
        let add: std::cell::Cell<Option<ItemKind>> = std::cell::Cell::new(None);
        let fits = |k: ItemKind| can && match kind { Some(sk) => k.fits(sk, true), None => k.fits(StudyKind::Structural, true) };
        let big = |ui: &mut egui::Ui, k: ItemKind, label: &str| {
            let r = super::ribbon::rich_tip(large(ui, item_icon(k), label, fits(k), false), item_icon(k), k.name(), item_tip(k));
            if r.clicked() { add.set(Some(k)); }
        };
        let mut contact = false;
        group(ui, "Setup", |ui| { contact = large(ui, Icon::Contact, "Kontakt", can, false).on_hover_text("Kontakte gibt es zwischen den Komponenten einer Baugruppe.").clicked(); });
        if contact { self.say("Kontakt: ein Teil hat keine Kontaktpartner. Kontakte verbinden die Komponenten einer Baugruppe; Baugruppen werden in der Live-Simulation noch nicht gerechnet.", MsgKind::Warning); }
        match kind {
            Some(StudyKind::Thermal) => {
                group(ui, "Randbedingungen", |ui| { big(ui, ItemKind::Temperature, "Temperatur"); big(ui, ItemKind::Convection, "Konvektion"); });
                group(ui, "Lasten", |ui| { big(ui, ItemKind::HeatFlow, "Wärmestrom"); big(ui, ItemKind::HeatFlux, "Wärmefluss"); });
            }
            Some(StudyKind::Fluid) => {
                group(ui, "Randbedingungen", |ui| { big(ui, ItemKind::Inlet, "Einlass"); big(ui, ItemKind::Outlet, "Auslass"); });
                group(ui, "Lasten", |ui| { big(ui, ItemKind::Wind, "Anströmung"); });
            }
            _ => {
                let mut th_btn = None;
                group(ui, "Randbedingungen", |ui| {
                    big(ui, ItemKind::Fixed, "Fest");
                    big(ui, ItemKind::Displacement, "Verschiebung");
                    stack(ui, |ui| {
                        if super::ribbon::rich_tip(tiny(ui, Icon::Frictionless, "", fits(ItemKind::Frictionless), false, false), Icon::Frictionless, "Reibungsfrei", item_tip(ItemKind::Frictionless)).clicked() { add.set(Some(ItemKind::Frictionless)); }
                        if super::ribbon::rich_tip(tiny(ui, Icon::Symmetry, "", fits(ItemKind::Symmetry), false, false), Icon::Symmetry, "Symmetrie", item_tip(ItemKind::Symmetry)).clicked() { add.set(Some(ItemKind::Symmetry)); }
                        th_btn = Some(tiny(ui, Icon::Temperature, "Thermische Objekte (über Studie ▾ einschließen)", fits(ItemKind::Temperature), false, true));
                    });
                    if let Some(b) = &th_btn {
                        drop_menu(ui, "sim_thermal", b, |ui| {
                            for k in [ItemKind::Temperature, ItemKind::Convection, ItemKind::HeatFlow, ItemKind::HeatFlux] { if menu_row(ui, Some(item_icon(k)), k.name(), "", true, false).clicked() { add.set(Some(k)); } }
                        });
                    }
                });
                group(ui, "Lasten", |ui| {
                    for (k, n) in [(ItemKind::Force, "Kraft"), (ItemKind::Moment, "Moment"), (ItemKind::Pressure, "Druck"), (ItemKind::Gravity, "Gravitation"), (ItemKind::Centrifugal, "Zentrifugal"), (ItemKind::LinAccel, "Lineare\nBeschleunigung")] { big(ui, k, n); }
                });
            }
        }
        if let Some(k) = add.get() {
            if !has { self.sim_new_study(StudyKind::Structural); }
            // a thermal condition in a structural study switches the thermal objects on
            let a = self.data.sim_active;
            if k.is_thermal() && self.data.sims.get(a).is_some_and(|s| s.kind == StudyKind::Structural && !s.include_thermal) {
                self.commit("Thermische Objekte eingeschlossen.", |d| if let Some(s) = d.sims.get_mut(a) { s.include_thermal = true; });
            }
            self.sim_new_item(k);
        }

        // ---- Darstellungsoptionen
        let res = has && self.sim.out.is_some();
        let deforms = res && self.sim.out.as_ref().is_some_and(|o| !o.disps.is_empty());
        let need = |app: &mut App, what: &str| app.say(format!("{}: dafür wird ein Ergebnis gebraucht. Über + ▾ eine Studie anlegen, Randbedingungen und Lasten setzen – die Lösung startet von selbst.", what), MsgKind::Prompt);
        group(ui, "Darstellungsoptionen", |ui| {
            if large(ui, Icon::DraftCheck, "Abweichung\nprüfen", can, self.sim.deviation).on_hover_text("Blendet die unverformte Form als Kontur über das verformte Ergebnis.").clicked() { self.sim.deviation = !self.sim.deviation; if !deforms { need(self, "Abweichung prüfen"); } }
            let (top, bot) = split(ui, Icon::Deform, "Verformung\nanimieren", can, self.sim.animate);
            if top.on_hover_text("Verformung bzw. Schwingungsform bewegt darstellen.").clicked() { self.sim.animate = !self.sim.animate; if !deforms { need(self, "Verformung animieren"); } }
            drop_menu(ui, "sim_deform", &bot, |ui| {
                ui.set_min_width(250.0);
                for (s, n) in [(DefScale::Auto, "Automatisch skaliert"), (DefScale::True, "Wahre Größe (1×)"), (DefScale::Factor(10.0), "10×"), (DefScale::Factor(100.0), "100×"), (DefScale::Factor(1000.0), "1000×"), (DefScale::Off, "Unverformt")] {
                    if menu_check(ui, self.sim.scale == s, n, true) { self.sim.scale = s; self.sim.buffers = None; }
                }
            });
            let r = large(ui, Icon::Probe, "Simulationsobjekt-\nAnzeige", can, self.sim.show_objects);
            if r.on_hover_text("Symbole der Lasten und Randbedingungen ein- oder ausblenden.").clicked() { self.sim.show_objects = !self.sim.show_objects; }
            if large(ui, Icon::SimResults, "Ergebnisse\nanzeigen", can, self.sim.show_results).on_hover_text("Ergebnisdarstellung ein- oder ausblenden.").clicked() { self.sim.show_results = !self.sim.show_results; if !res { need(self, "Ergebnisse anzeigen"); } }
        });

        // ---- Abfrage
        let ((), qt) = group_drop(ui, "Abfrage", |ui| {
            let r = large(ui, Icon::Probe, "Simulations-\nTaster", can, self.sim.probing && res);
            if r.on_hover_text("Auf das Modell klicken, um den Ergebniswert an dieser Stelle abzulesen.").clicked() {
                if !res { need(self, "Simulations-Taster"); } else {
                    self.sim.probing = !self.sim.probing;
                    if self.sim.probing { self.sim.show_results = true; self.say("Simulations-Taster: Punkte auf dem Modell anklicken.", MsgKind::Prompt); }
                }
            }
            if large(ui, Icon::SimReport, "Simulationsbericht", can, self.sim.report_open && res).clicked() { if res { self.sim.report_open = !self.sim.report_open; } else { need(self, "Simulationsbericht"); } }
        });
        let mut export = false;
        group_menu(ui, "sim_query", &qt, |ui| {
            ui.set_min_width(330.0);
            if menu_check(ui, self.sim.collect, "Daten sammeln", can) { self.sim.collect = !self.sim.collect; if self.sim.collect { self.say("Daten sammeln: die Kennwerte jeder Lösung werden im Simulationsbericht mitgeschrieben.", MsgKind::Info); } }
            if menu_row(ui, Some(Icon::SavedAnalysis), "Gespeicherte Analyse", "", !self.in_asm(), false).clicked() { self.sim.saved_open = true; }
            if menu_row(ui, Some(Icon::ExportNum), "Numerische Ergebnisse exportieren", "", can, false).clicked() { if res { export = true; } else { need(self, "Numerische Ergebnisse exportieren"); } }
        });
        if export { self.sim_export_csv(); }
    }

    // ------------------------------------------------------------ tree

    /// Simulation tree under the model tree (Live-Simulation tab only).
    pub fn sim_tree_panel(&mut self, ui: &mut egui::Ui) {
        if self.tab != Tab::LiveSim || self.data.sims.is_empty() || self.in_asm() { return; }
        egui::TopBottomPanel::bottom("sim_tree").resizable(true).default_height(190.0).min_height(80.0).frame(egui::Frame::NONE.fill(PANEL_BG)).show_inside(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0_f32, BORDER));
            ui.add_space(4.0);
            ui.label(RichText::new("  Simulationen").size(12.5).color(LABEL));
            let (mut activate, mut select, mut edit, mut delete, mut settings) = (None, None, None, None, false);
            egui::ScrollArea::vertical().id_salt("sim_tree_scroll").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (si, st) in self.data.sims.iter().enumerate() {
                    let active = si == self.data.sim_active;
                    let (r, _) = Self::paint_row(ui, 0, study_icon(st.kind), &format!("{}  ({}, {})", st.name, st.kind.short(), if st.kind == StudyKind::Fluid { &st.fluid } else { &st.material }), TEXT, active, false, None);
                    if r.clicked() { activate = Some(si); }
                    if r.double_clicked() { activate = Some(si); settings = true; }
                    if !active { continue; }
                    for it in &st.items {
                        let n = if it.kind.needs_faces() { format!("{}  –  {} Fläche(n)", it.name, it.faces.len()) } else { it.name.clone() };
                        let (r, _) = Self::paint_row(ui, 1, item_icon(it.kind), &n, TEXT, false, self.sim.sel_item == Some(it.id), None);
                        if r.clicked() { select = Some(it.id); }
                        if r.double_clicked() { edit = Some(it.id); }
                        r.context_menu(|ui| {
                            super::ribbon::menu_spacing(ui);
                            if menu_row(ui, Some(Icon::EditDef), "Definition editieren", "", true, false).clicked() { edit = Some(it.id); ui.close_menu(); }
                            if menu_row(ui, Some(Icon::Delete), "Löschen", "", true, false).clicked() { delete = Some(it.id); ui.close_menu(); }
                        });
                    }
                }
            });
            if let Some(i) = activate { self.sim_activate(i); }
            if settings { self.sim.settings_open = true; }
            if let Some(id) = select { self.sim.sel_item = if self.sim.sel_item == Some(id) { None } else { Some(id) }; }
            if let Some(id) = edit { self.sim_edit_item(id); }
            if let Some(id) = delete { self.sim_delete_item(id); }
        });
    }

    // ------------------------------------------------------------ dialogs

    pub fn sim_windows(&mut self, ctx: &egui::Context) {
        if self.in_asm() { return; }
        self.sim_item_dialog(ctx);
        self.sim_settings_dialog(ctx);
        self.sim_perf_dialog(ctx);
        self.sim_report_window(ctx);
        self.sim_saved_window(ctx);
    }

    fn sim_item_dialog(&mut self, ctx: &egui::Context) {
        if self.tab != Tab::LiveSim { if self.sim.edit.is_some() { self.sim.edit = None; } return; }
        let Some(mut ed) = self.sim.edit.clone() else { return };
        let (mut ok, mut cancel) = (false, false);
        let face_normal = ed.item.faces.first().map(|f| f.face.normal);
        let kind = ed.item.kind;
        egui::Window::new(kind.name()).id(egui::Id::new("sim_item")).title_bar(false).collapsible(false).resizable(false).default_pos(pos2(430.0, 190.0)).show(ctx, |ui| {
            if super::creo::dialog_header(ui, kind.name()) { cancel = true; }
            ui.set_width(330.0);
            ui.add_space(6.0);
            egui::Grid::new("sim_item_grid").num_columns(2).spacing([10.0, 7.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut ed.item.name).desired_width(220.0));
                ui.end_row();
                if kind.needs_faces() {
                    ui.label("Referenzen");
                    ui.horizontal(|ui| {
                        let txt = if ed.item.faces.is_empty() { "Flächen wählen".to_string() } else { format!("{} Fläche(n)", ed.item.faces.len()) };
                        let fill = if ed.picking { Color32::from_rgb(255, 250, 205) } else { Color32::WHITE };
                        if ui.add(egui::Button::new(RichText::new(txt).color(TEXT)).fill(fill).stroke(Stroke::new(1.0_f32, if ed.picking { SEL_BORDER } else { BORDER })).min_size(vec2(150.0, 24.0))).on_hover_text("Kollektor aktivieren, dann Flächen im Grafikfenster anklicken").clicked() { ed.picking = !ed.picking; }
                        if ui.add_enabled(!ed.item.faces.is_empty(), egui::Button::new("Leeren")).clicked() { ed.item.faces.clear(); ed.picking = true; }
                    });
                    ui.end_row();
                }
                let vec_rows = |ui: &mut egui::Ui, v: &mut [f64; 3], unit: &str, speed: f64| {
                    for (c, n) in ["X", "Y", "Z"].iter().enumerate() {
                        ui.label(*n);
                        ui.add(egui::DragValue::new(&mut v[c]).speed(speed).suffix(format!(" {}", unit)).max_decimals(4));
                        ui.end_row();
                    }
                };
                match kind {
                    ItemKind::Force => {
                        vec_rows(ui, &mut ed.item.vec, "N", 1.0);
                        ui.label("Betrag");
                        ui.horizontal(|ui| {
                            let mut m = V3::from_array(ed.item.vec).length();
                            if ui.add(egui::DragValue::new(&mut m).speed(1.0).suffix(" N").range(0.0..=1e12)).changed() { let d = safe_norm(V3::from_array(ed.item.vec), -V3::Y); ed.item.vec = (d * m).to_array(); }
                            if ui.add_enabled(face_normal.is_some(), egui::Button::new("⟂ auf Fläche")).on_hover_text("Kraft senkrecht auf die erste gewählte Fläche drücken lassen").clicked() {
                                if let Some(n) = face_normal { ed.item.vec = (-n * V3::from_array(ed.item.vec).length().max(1e-9)).to_array(); }
                            }
                        });
                        ui.end_row();
                    }
                    ItemKind::Moment => vec_rows(ui, &mut ed.item.vec, "N·mm", 10.0),
                    ItemKind::Displacement => {
                        for (c, n) in ["X", "Y", "Z"].iter().enumerate() {
                            ui.checkbox(&mut ed.item.fixed[c], *n).on_hover_text("vorgeben (sonst frei)");
                            ui.add_enabled(ed.item.fixed[c], egui::DragValue::new(&mut ed.item.vec[c]).speed(0.01).suffix(" mm").max_decimals(5));
                            ui.end_row();
                        }
                    }
                    ItemKind::Pressure => { ui.label("Druck"); ui.add(egui::DragValue::new(&mut ed.item.value).speed(0.05).suffix(" MPa").max_decimals(5)); ui.end_row(); }
                    ItemKind::Gravity | ItemKind::LinAccel => {
                        vec_rows(ui, &mut ed.item.vec, "mm/s²", 10.0);
                        ui.label("Richtung");
                        ui.horizontal(|ui| {
                            for (n, d) in [("−Y", -V3::Y), ("−Z", -V3::Z), ("−X", -V3::X), ("+Y", V3::Y), ("+Z", V3::Z), ("+X", V3::X)] {
                                if ui.small_button(n).clicked() { ed.item.vec = (d * V3::from_array(ed.item.vec).length().max(9810.0)).to_array(); }
                            }
                        });
                        ui.end_row();
                    }
                    ItemKind::Centrifugal => {
                        ui.label("Drehachse");
                        ui.horizontal(|ui| { for (n, d) in [("X", V3::X), ("Y", V3::Y), ("Z", V3::Z)] { if ui.selectable_label(V3::from_array(ed.item.vec).normalize_or_zero().dot(d) > 0.999, n).clicked() { ed.item.vec = d.to_array(); } } ui.label("durch den Ursprung"); });
                        ui.end_row();
                        ui.label("Drehzahl");
                        ui.add(egui::DragValue::new(&mut ed.item.value).speed(10.0).suffix(" 1/min").range(0.0..=1e7));
                        ui.end_row();
                    }
                    ItemKind::Temperature => { ui.label("Temperatur"); ui.add(egui::DragValue::new(&mut ed.item.value).speed(1.0).suffix(" °C")); ui.end_row(); }
                    ItemKind::Convection => {
                        ui.label("Wärmeübergang"); ui.add(egui::DragValue::new(&mut ed.item.value).speed(1.0).suffix(" W/(m²·K)").range(0.0..=1e7)); ui.end_row();
                        ui.label("Umgebung"); ui.add(egui::DragValue::new(&mut ed.item.value2).speed(1.0).suffix(" °C")); ui.end_row();
                    }
                    ItemKind::HeatFlow => { ui.label("Wärmestrom"); ui.add(egui::DragValue::new(&mut ed.item.value).speed(0.5).suffix(" W")); ui.end_row(); }
                    ItemKind::HeatFlux => { ui.label("Wärmefluss"); ui.add(egui::DragValue::new(&mut ed.item.value).speed(0.001).suffix(" W/mm²").max_decimals(5)); ui.end_row(); }
                    ItemKind::Inlet => { ui.label("Geschwindigkeit"); ui.add(egui::DragValue::new(&mut ed.item.value).speed(0.1).suffix(" m/s").range(0.001..=1000.0)); ui.end_row(); }
                    ItemKind::Wind => {
                        vec_rows(ui, &mut ed.item.vec, "", 0.05);
                        ui.label("Richtung");
                        ui.horizontal(|ui| { for (n, d) in [("+X", V3::X), ("−X", -V3::X), ("+Y", V3::Y), ("−Y", -V3::Y), ("+Z", V3::Z), ("−Z", -V3::Z)] { if ui.small_button(n).clicked() { ed.item.vec = d.to_array(); } } });
                        ui.end_row();
                        ui.label("Geschwindigkeit"); ui.add(egui::DragValue::new(&mut ed.item.value).speed(0.5).suffix(" m/s").range(0.001..=1000.0)); ui.end_row();
                    }
                    ItemKind::Fixed | ItemKind::Frictionless | ItemKind::Symmetry | ItemKind::Outlet => {}
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new(item_tip(kind)).size(12.0).color(LABEL));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("Abbrechen").min_size(vec2(90.0, 26.0))).clicked() { cancel = true; }
                    let valid = !kind.needs_faces() || !ed.item.faces.is_empty();
                    if ui.add_enabled(valid, egui::Button::new("OK").min_size(vec2(70.0, 26.0))).clicked() { ok = true; }
                });
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.wants_keyboard_input() { cancel = true; }
        if cancel { self.sim.edit = None; return; }
        self.sim.edit = Some(ed);
        if ok { self.sim_finish_item(); }
    }

    fn sim_settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.sim.settings_open || self.tab != Tab::LiveSim { return; }
        let Some(mut st) = self.sim_study().cloned() else { self.sim.settings_open = false; return };
        let before = st.clone();
        let (mut close, mut delete) = (false, false);
        egui::Window::new("Studieneinstellungen").id(egui::Id::new("sim_settings")).title_bar(false).collapsible(false).resizable(false).default_pos(pos2(430.0, 190.0)).show(ctx, |ui| {
            if super::creo::dialog_header(ui, &format!("{} – {}", st.kind.title(), st.name)) { close = true; }
            ui.set_width(380.0);
            ui.add_space(6.0);
            egui::Grid::new("sim_set_grid").num_columns(2).spacing([10.0, 7.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut st.name).desired_width(230.0));
                ui.end_row();
                if st.kind == StudyKind::Fluid {
                    ui.label("Fluid");
                    egui::ComboBox::from_id_salt("sim_fluid").width(230.0).selected_text(st.fluid.clone()).show_ui(ui, |ui| { for f in FLUIDS { ui.selectable_value(&mut st.fluid, f.name.to_string(), format!("{}  ({} kg/m³)", f.name, f.rho)); } });
                    ui.end_row();
                } else {
                    ui.label("Material");
                    egui::ComboBox::from_id_salt("sim_mat").width(230.0).selected_text(st.material.clone()).show_ui(ui, |ui| { for m in MATERIALS { ui.selectable_value(&mut st.material, m.name.to_string(), m.name); } });
                    ui.end_row();
                    let m = material(&st.material);
                    ui.label("");
                    ui.label(RichText::new(format!("E = {} MPa, ν = {}, ρ = {} kg/m³\nα = {:.1e} 1/K, λ = {} W/(m·K), Re = {} MPa", m.e, m.nu, m.rho, m.alpha, m.k, m.yield_mpa)).size(12.0).color(LABEL));
                    ui.end_row();
                }
                if st.kind == StudyKind::Modal { ui.label("Anzahl Moden"); ui.add(egui::DragValue::new(&mut st.modes).range(1..=10)); ui.end_row(); }
                if st.kind == StudyKind::Structural && st.include_thermal { ui.label("Bezugstemperatur"); ui.add(egui::DragValue::new(&mut st.ref_temp).speed(1.0).suffix(" °C")).on_hover_text("spannungsfreie Temperatur des Teils"); ui.end_row(); }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("Studie löschen").min_size(vec2(110.0, 26.0))).clicked() { delete = true; }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { if ui.add(egui::Button::new("Schließen").min_size(vec2(90.0, 26.0))).clicked() { close = true; } });
            });
        });
        if delete { self.sim_delete_study(); return; }
        // text edits are committed when the dialog closes or another field takes over
        if st != before && (st.name == before.name || close) {
            let a = self.data.sim_active;
            let s2 = st.clone();
            self.commit("", |d| if let Some(s) = d.sims.get_mut(a) { *s = s2; });
        } else if st.name != before.name {
            // keep the typed name without regenerating on every key
            let a = self.data.sim_active;
            if let Some(s) = self.data.sims.get_mut(a) { s.name = st.name.clone(); self.dirty = true; }
        }
        if close { self.sim.settings_open = false; }
    }

    fn sim_perf_dialog(&mut self, ctx: &egui::Context) {
        if !self.sim.perf_open { return; }
        let mut f = self.sim.fidelity_ui;
        let mut close = false;
        let kind = self.sim_study().map(|s| s.kind).unwrap_or(StudyKind::Structural);
        egui::Window::new("Leistungsoptionen").id(egui::Id::new("sim_perf")).title_bar(false).collapsible(false).resizable(false).default_pos(pos2(430.0, 190.0)).show(ctx, |ui| {
            if super::creo::dialog_header(ui, "Leistungsoptionen") { close = true; }
            ui.set_width(390.0);
            ui.add_space(8.0);
            ui.label("Simulationsauflösung");
            ui.horizontal(|ui| {
                ui.label(RichText::new("Geschwindigkeit").size(12.5).color(LABEL));
                ui.spacing_mut().slider_width = 180.0;
                ui.add(egui::Slider::new(&mut f, 0.0..=1.0).show_value(false));
                ui.label(RichText::new("Genauigkeit").size(12.5).color(LABEL));
            });
            let n = run::budget(kind, f);
            ui.label(RichText::new(format!("{}: etwa {} {}", kind.short(), if n >= 10_000 { format!("{} 000", n / 1000) } else { n.to_string() }, if kind == StudyKind::Fluid { "Gitterzellen" } else { "Elemente" })).size(12.5).color(TEXT));
            ui.label(RichText::new(format!("Gerechnet wird auf {} Prozessorkernen. Eine höhere Auflösung löst Kerben und dünne Wände feiner auf, braucht aber deutlich länger.", crate::sim::mesh::threads())).size(12.0).color(LABEL));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("Standard").min_size(vec2(90.0, 26.0))).clicked() { f = 0.35; }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { if ui.add(egui::Button::new("Schließen").min_size(vec2(90.0, 26.0))).clicked() { close = true; } });
            });
        });
        // the new resolution takes effect when the slider is released
        self.sim.fidelity_ui = f;
        if !ctx.input(|i| i.pointer.any_down()) || close { self.sim.fidelity = (f * 20.0).round() / 20.0; }
        if close { self.sim.perf_open = false; }
    }

    fn sim_report_rows(&self) -> Option<(String, Vec<(String, String)>, Vec<String>, Vec<String>)> {
        let out = self.sim.out.as_ref()?;
        let st = self.sim_study()?;
        let mut items = vec![];
        for it in &st.items {
            let v = it.vec;
            let d = match it.kind {
                ItemKind::Force => format!("({}, {}, {}) N", v[0], v[1], v[2]),
                ItemKind::Moment => format!("({}, {}, {}) N·mm", v[0], v[1], v[2]),
                ItemKind::Displacement => format!("({}, {}, {}) mm", if it.fixed[0] { v[0].to_string() } else { "frei".into() }, if it.fixed[1] { v[1].to_string() } else { "frei".into() }, if it.fixed[2] { v[2].to_string() } else { "frei".into() }),
                ItemKind::Pressure => format!("{} MPa", it.value),
                ItemKind::Gravity | ItemKind::LinAccel => format!("({}, {}, {}) mm/s²", v[0], v[1], v[2]),
                ItemKind::Centrifugal => format!("{} 1/min um ({}, {}, {})", it.value, v[0], v[1], v[2]),
                ItemKind::Temperature => format!("{} °C", it.value),
                ItemKind::Convection => format!("{} W/(m²·K), Umgebung {} °C", it.value, it.value2),
                ItemKind::HeatFlow => format!("{} W", it.value),
                ItemKind::HeatFlux => format!("{} W/mm²", it.value),
                ItemKind::Inlet => format!("{} m/s", it.value),
                ItemKind::Wind => format!("{} m/s in Richtung ({}, {}, {})", it.value, v[0], v[1], v[2]),
                _ => String::new(),
            };
            items.push(format!("{}{}{}", it.name, if it.kind.needs_faces() { format!(" – {} Fläche(n)", it.faces.len()) } else { String::new() }, if d.is_empty() { String::new() } else { format!(": {}", d) }));
        }
        Some((format!("{} – {} ({})", self.data.name, st.name, st.kind.title()), out.summary.clone(), items, out.warnings.clone()))
    }

    fn sim_report_window(&mut self, ctx: &egui::Context) {
        if !self.sim.report_open || self.tab != Tab::LiveSim { return; }
        let Some((title, summary, items, warnings)) = self.sim_report_rows() else { self.sim.report_open = false; return };
        let mut open = true;
        let (mut save, mut clear) = (false, false);
        egui::Window::new("Simulationsbericht").id(egui::Id::new("sim_report")).title_bar(false).collapsible(false).resizable(true).default_width(520.0).default_pos(pos2(470.0, 170.0)).show(ctx, |ui| {
            if super::creo::dialog_header(ui, "Simulationsbericht") { open = false; }
            ui.add_space(6.0);
            ui.label(RichText::new(&title).strong().size(14.5));
            egui::ScrollArea::vertical().max_height(430.0).show(ui, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new("Ergebnisse").strong());
                egui::Grid::new("sim_rep_sum").num_columns(2).striped(true).spacing([18.0, 5.0]).show(ui, |ui| { for (k, v) in &summary { ui.label(k); ui.label(v); ui.end_row(); } });
                ui.add_space(6.0);
                ui.label(RichText::new("Randbedingungen und Lasten").strong());
                for i in &items { ui.label(format!("•  {}", i)); }
                if !warnings.is_empty() { ui.add_space(6.0); ui.label(RichText::new("Hinweise").strong()); for w in &warnings { ui.label(RichText::new(format!("⚠  {}", w)).color(Color32::from_rgb(150, 90, 0))); } }
                if !self.sim.history.is_empty() {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| { ui.label(RichText::new(format!("Gesammelte Daten ({} Läufe)", self.sim.history.len())).strong()); if ui.small_button("Leeren").clicked() { clear = true; } });
                    for (i, (n, rows)) in self.sim.history.iter().enumerate() {
                        let key: Vec<String> = rows.iter().filter(|r| r.0.starts_with("Max.") || r.0.starts_with("Mode") || r.0.starts_with("Min.") || r.0.starts_with("Druckabfall") || r.0.starts_with("Kraft auf")).map(|r| format!("{} {}", r.0, r.1)).collect();
                        ui.label(RichText::new(format!("{}.  {}: {}", i + 1, n, key.join("; "))).size(12.0));
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("Als HTML speichern").min_size(vec2(140.0, 26.0))).clicked() { save = true; }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { if ui.add(egui::Button::new("Schließen").min_size(vec2(90.0, 26.0))).clicked() { open = false; } });
            });
        });
        if clear { self.sim.history.clear(); }
        if save { self.sim_save_report(); }
        if !open { self.sim.report_open = false; }
    }

    fn sim_file(&self, ext: &str) -> PathBuf {
        let st = self.sim_study().map(|s| s.name.clone()).unwrap_or_default();
        let clean = |s: &str| s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect::<String>();
        self.save_dir(self.path.as_ref()).join(format!("{}_{}.{}", clean(&self.data.name), clean(&st), ext))
    }

    fn sim_save_report(&mut self) {
        let Some((title, summary, items, warnings)) = self.sim_report_rows() else { return };
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let mut h = format!("<!doctype html><html lang=\"de\"><head><meta charset=\"utf-8\"><title>{0}</title><style>body{{font:15px -apple-system,Helvetica,Arial,sans-serif;margin:32px;color:#222}}table{{border-collapse:collapse}}td{{padding:5px 18px 5px 0;border-bottom:1px solid #ddd}}h1{{font-size:21px}}h2{{font-size:16px;margin-top:26px}}.w{{color:#965a00}}</style></head><body><h1>Simulationsbericht</h1><p>{0}</p><h2>Ergebnisse</h2><table>", esc(&title));
        for (k, v) in &summary { h += &format!("<tr><td>{}</td><td>{}</td></tr>", esc(k), esc(v)); }
        h += "</table><h2>Randbedingungen und Lasten</h2><ul>";
        for i in &items { h += &format!("<li>{}</li>", esc(i)); }
        h += "</ul>";
        if !warnings.is_empty() { h += "<h2>Hinweise</h2><ul>"; for w in &warnings { h += &format!("<li class=\"w\">{}</li>", esc(w)); } h += "</ul>"; }
        if !self.sim.history.is_empty() {
            h += "<h2>Gesammelte Daten</h2><table>";
            for (i, (n, rows)) in self.sim.history.iter().enumerate() { h += &format!("<tr><td>{}</td><td>{}</td><td>{}</td></tr>", i + 1, esc(n), esc(&rows.iter().filter(|r| r.0.starts_with("Max.") || r.0.starts_with("Mode") || r.0.starts_with("Min.")).map(|r| format!("{} {}", r.0, r.1)).collect::<Vec<_>>().join("; "))); }
            h += "</table>";
        }
        h += "<p style=\"color:#888;margin-top:30px\">Erzeugt mit Freo Live-Simulation (Voxel-Finite-Elemente bzw. Lattice-Boltzmann).</p></body></html>";
        let p = self.sim_file("html");
        match std::fs::write(&p, h) { Ok(_) => self.say(format!("Simulationsbericht gespeichert: {}", p.display()), MsgKind::Success), Err(e) => self.say(format!("Bericht konnte nicht gespeichert werden: {}", e), MsgKind::Error) }
    }

    /// Abfrage ▸ Numerische Ergebnisse exportieren: one CSV row per mesh node (or fluid cell).
    pub fn sim_export_csv(&mut self) {
        let Some(out) = self.sim.out.clone() else { return };
        let mut s = out.table.0.join(";");
        s.push('\n');
        for r in &out.table.1 { s += &r.iter().map(|v| format!("{:.6e}", v)).collect::<Vec<_>>().join(";"); s.push('\n'); }
        let p = self.sim_file("csv");
        match std::fs::write(&p, s) { Ok(_) => self.say(format!("{} Zeilen exportiert: {}", out.table.1.len(), p.display()), MsgKind::Success), Err(e) => self.say(format!("Export fehlgeschlagen: {}", e), MsgKind::Error) }
    }

    fn sim_saved_window(&mut self, ctx: &egui::Context) {
        if !self.sim.saved_open { return; }
        let mut open = true;
        let (mut save, mut show, mut delete) = (false, None, None);
        let can_save = self.sim.out.as_ref().is_some_and(|o| !o.partial) && !self.sim.viewing_saved;
        egui::Window::new("Gespeicherte Analyse").id(egui::Id::new("sim_saved")).title_bar(false).collapsible(false).resizable(false).default_pos(pos2(470.0, 190.0)).show(ctx, |ui| {
            if super::creo::dialog_header(ui, "Gespeicherte Analyse") { open = false; }
            ui.set_width(470.0);
            ui.add_space(6.0);
            if self.sim.saved.is_empty() { ui.label(RichText::new("Noch keine Analyse gespeichert. Ein Ergebnis lässt sich hier festhalten und später mit neuen Ständen vergleichen.").color(LABEL)); }
            for (i, s) in self.sim.saved.iter().enumerate() {
                ui.horizontal(|ui| {
                    let key: Vec<String> = s.out.summary.iter().filter(|r| r.0.starts_with("Max.") || r.0.starts_with("Mode 1")).take(2).map(|r| format!("{} {}", r.0, r.1)).collect();
                    ui.vertical(|ui| { ui.label(RichText::new(&s.name).strong()); ui.label(RichText::new(format!("{} · {}", s.when, key.join(", "))).size(12.0).color(LABEL)); });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Löschen").clicked() { delete = Some(i); }
                        if ui.button("Anzeigen").clicked() { show = Some(i); }
                    });
                });
                ui.separator();
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(can_save, egui::Button::new("Aktuelle Analyse speichern").min_size(vec2(190.0, 26.0))).clicked() { save = true; }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { if ui.add(egui::Button::new("Schließen").min_size(vec2(90.0, 26.0))).clicked() { open = false; } });
            });
            ui.label(RichText::new("Gespeicherte Analysen gelten für diese Sitzung.").size(11.5).color(LABEL));
        });
        if save {
            if let (Some(out), Some(st)) = (self.sim.out.clone(), self.sim_study()) {
                let n = self.sim.saved.len() + 1;
                let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) % 86400;
                self.sim.saved.push(SavedAnalysis { name: format!("{} – Analyse {}", st.name, n), when: format!("{:02}:{:02} UTC", secs / 3600, secs / 60 % 60), out });
                self.say("Analyse gespeichert.", MsgKind::Success);
            }
        }
        if let Some(i) = show {
            if let Some(j) = self.sim.job.take() { j.shared.cancel.store(true, std::sync::atomic::Ordering::Relaxed); }
            self.sim.out = Some(self.sim.saved[i].out.clone());
            self.sim.viewing_saved = true;
            self.sim.show_results = true;
            self.sim.field = 0;
            self.sim.buffers = None;
            self.sim.probes.clear();
            self.tab = Tab::LiveSim;
        }
        if let Some(i) = delete { self.sim.saved.remove(i); }
        if !open { self.sim.saved_open = false; }
    }

    /// Studie ▸ Vorhandene Simulation importieren: the studies of another part file.
    fn sim_import(&mut self) {
        let dir = self.save_dir(self.path.as_ref());
        let Some(p) = rfd::FileDialog::new().set_title("Vorhandene Simulation importieren").set_directory(dir).add_filter("Freo-Teil", &["pfprt"]).pick_file() else { return };
        let other = match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<ModelData>(&s).map_err(|e| e.to_string())) {
            Ok(d) => d,
            Err(e) => { self.say(format!("Import fehlgeschlagen: {}", e), MsgKind::Error); return; }
        };
        if other.sims.is_empty() { self.say(format!("{} enthält keine Simulationsstudie.", p.display()), MsgKind::Warning); return; }
        let n = other.sims.len();
        let base = self.data.sims.iter().map(|s| s.id).max().unwrap_or(0);
        self.commit(&format!("{} Simulationsstudie(n) importiert. Referenzen werden über die Geometrie zugeordnet.", n), |d| {
            for (i, mut s) in other.sims.into_iter().enumerate() { s.id = base + 1 + i as u64; d.sims.push(s); }
            d.sim_active = d.sims.len() - 1;
        });
        self.sim_reset_view();
    }

    /// FREO_DEMO=sim:<kind>: a solved study on the bracket (screenshots).
    pub fn demo_sim(&mut self, which: &str) {
        let topo = self.result.topo.clone();
        let face = |n: V3| (0..topo.faces.len()).filter(|&f| topo.faces[f].planar && topo.faces[f].normal.dot(n) > 0.999).max_by(|&a, &b| topo.faces[a].center.dot(n).partial_cmp(&topo.faces[b].center.dot(n)).unwrap()).map(|f| SimFace::of(&topo, f));
        let kind = match which { "thermal" => StudyKind::Thermal, "modal" => StudyKind::Modal, "fluid" => StudyKind::Fluid, _ => StudyKind::Structural };
        let mut st = Study::new(1, format!("{} 1", kind.short()), kind);
        let mut add = |k: ItemKind, f: Option<SimFace>, set: &dyn Fn(&mut SimItem)| { let mut it = SimItem::new(st.next_item, st.item_name(k), k); it.faces = f.into_iter().collect(); set(&mut it); st.next_item += 1; st.items.push(it); };
        match kind {
            StudyKind::Structural => { add(ItemKind::Fixed, face(-V3::Y), &|_| {}); add(ItemKind::Force, face(V3::Y), &|i| i.vec = [3000.0, 0.0, 0.0]); }
            StudyKind::Modal => add(ItemKind::Fixed, face(-V3::Y), &|_| {}),
            StudyKind::Thermal => { add(ItemKind::Temperature, face(V3::Y), &|i| i.value = 120.0); add(ItemKind::Convection, face(-V3::Y), &|i| { i.value = 400.0; i.value2 = 20.0; }); }
            StudyKind::Fluid => add(ItemKind::Wind, None, &|i| { i.vec = [1.0, 0.0, 0.0]; i.value = 12.0; }),
        }
        self.data.sims = vec![st.clone()];
        self.data.sim_active = 0;
        self.tab = Tab::LiveSim;
        if std::env::var_os("FREO_DEMO_NOSOLVE").is_some() { self.sim.paused = true; return; }
        match run::run_blocking(&SimInput { topo, study: st, fidelity: self.sim.fidelity }) {
            Ok(o) => { let key = self.sim_key().unwrap_or(1); self.sim_take(o, key); }
            Err(e) => self.say(e, MsgKind::Error),
        }
        if which == "modal" || std::env::var_os("FREO_DEMO_PROBE").is_some() { self.sim.field = 0; }
    }
}

impl SimUi {
    /// A study without anything in it cannot be solved yet.
    fn study_empty(&self, st: Option<&Study>) -> bool {
        match st { None => true, Some(s) => s.items.is_empty() && s.kind != StudyKind::Modal }
    }
}
