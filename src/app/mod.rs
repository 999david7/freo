//! Application state and controller logic.

pub mod annot3d;
pub mod assembly;
pub mod creo;
pub mod camera;
pub mod combostates;
pub mod demo;
pub mod dims3d;
pub mod drawing;
pub mod drawui;
pub mod gdt;
pub mod gfxbar;
pub mod minibar;
pub mod modelcmds;
pub mod modelgrid;
pub mod featuretab;
pub mod dialogs;
pub mod export;
pub mod icons;
pub mod navigator;
pub mod pick;
pub mod publish;
pub mod render;
pub mod ribbon;
pub mod sketcher;
pub mod ui;
pub mod viewport;

use crate::kernel::math::*;
use crate::model::features::*;
use crate::model::regen::{self, RegenResult};
use crate::sketch::sketch::Sketch;
use camera::Camera;
use render::{ModelBuffers, Renderer, Style};
use sketcher::SketchSession;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Pick { Face(usize), Edge(usize), Plane(u64), Axis(u64), Point(u64), Feature(u64) }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickTarget { SketchPlane, Edges, Faces, Plane, Plane2, Axis, Direction, Features, DepthRef, Points, Surface }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot { Main, Trajectory, Section, Blend(usize), Pattern }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Filter { Smart, Feature, Surface, Edge, Datum }

impl Filter {
    pub const ALL: [Filter; 5] = [Filter::Smart, Filter::Feature, Filter::Surface, Filter::Edge, Filter::Datum];
    pub fn name(&self) -> &'static str {
        match self { Filter::Smart => "Intelligent", Filter::Feature => "KE", Filter::Surface => "Fläche", Filter::Edge => "Kante", Filter::Datum => "Bezug" }
    }
    pub fn icon(&self) -> icons::Icon {
        use icons::Icon;
        match self { Filter::Smart => Icon::SkSelect, Filter::Feature => Icon::Extrude, Filter::Surface => Icon::SelFace, Filter::Edge => Icon::SelEdge, Filter::Datum => Icon::Plane }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MsgKind { Info, Success, Warning, Error, Prompt }

pub struct Message { pub text: String, pub kind: MsgKind }

#[derive(Clone)]
pub struct Editor {
    pub feature: Feature,
    pub is_new: bool,
    pub index: usize,
    pub pick: Option<PickTarget>,
    pub preview: bool,
}

/// Creo's "Skizze" dialog (Platzierung ▸ Definieren...): the sketch plane is picked first and
/// sketching starts only on the dialog's Skizze button. Keeps what to restore on Abbrechen.
#[derive(Clone, Copy, Debug)]
pub struct SketchDlg { pub prev_plane: Option<PlaneRef>, pub prev_flip: bool }

/// "Neu orientieren..." dialog: dynamic rotation from `base`, and a name to save the view under.
#[derive(Clone, Debug)]
pub struct Reorient { pub base: glam::DQuat, pub h: f64, pub v: f64, pub z: f64, pub name: String }

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SectionView { pub plane: u64, pub offset: f64, pub flip: bool }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab { Home, Model, Analysis, LiveSim, Annot3d, Tools, View, Flex, Apps, Htl, Context, Layout, Table, Annotate, Publish }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NavTab { Tree, Browser }

/// Creo's mini toolbar shown next to a selection in the graphics window.
#[derive(Clone, Copy, Debug)]
pub struct MiniBar { pub pos: eframe::egui::Pos2, pub pick: Pick }

#[derive(Default)]
pub struct Windows {
    pub parameters: bool,
    pub relations: bool,
    pub mass: bool,
    pub appearance: bool,
    pub info: bool,
    pub controls: bool,
    pub about: bool,
    pub messages: bool,
    pub family: bool,
    /// Create Component dialog (proposed name)
    pub create_comp: Option<String>,
    pub new_drawing: Option<drawui::NewDrawing>,
}

pub struct App {
    pub data: ModelData,
    pub result: RegenResult,
    pub context: Option<RegenResult>,
    pub preview: Option<RegenResult>,
    pub selection: Vec<Pick>,
    pub hover: Option<Pick>,
    pub editor: Option<Editor>,
    pub sketch: Option<SketchSession>,
    pub sketch_dlg: Option<SketchDlg>,
    /// Setup ▸ Skizzen-Setup while sketching: the plane chosen so far and the view flip
    pub resetup: Option<(PlaneRef, bool)>,
    pub reorient: Option<Reorient>,
    pub messages: Vec<Message>,
    pub style: Style,
    pub show_planes: bool,
    pub show_axes: bool,
    pub show_points: bool,
    pub show_csys: bool,
    pub show_curves: bool,
    pub show_tags: bool,
    pub transparent: bool,
    pub section: Option<SectionView>,
    pub filter: Filter,
    pub tree_selection: Vec<u64>,
    pub measure: bool,
    pub tab: Tab,
    pub windows: Windows,
    pub dirty: bool,
    pub path: Option<PathBuf>,
    pub undo: Vec<ModelData>,
    pub redo: Vec<ModelData>,
    pub camera: Camera,
    pub renderer: Option<Arc<Mutex<Renderer>>>,
    pub buffers: Arc<ModelBuffers>,
    pub fit_pending: bool,
    pub renaming: Option<(u64, String)>,
    pub viewport_size: (f32, f32),
    pub relations_draft: Option<String>,
    pub player: Option<usize>,
    pub screenshot_request: Option<PathBuf>,
    pub file_menu: bool,
    pub show_nav: bool,
    pub nav_tab: NavTab,
    pub browser_dir: PathBuf,
    /// Arbeitsverzeichnis: where new files are saved (~/Documents/freo-work, like Creo's Creo10_Work)
    pub work_dir: PathBuf,
    pub expanded: HashSet<u64>,
    pub tree_hover: Option<u64>,
    pub tree_hover_ext: Option<u64>,
    pub recent: Vec<PathBuf>,
    pub dash_panel: Option<&'static str>,
    pub dash_panel_anchor: eframe::egui::Pos2,
    pub mini: Option<MiniBar>,
    pub edit_dims: Option<dims3d::EditDims>,
    pub saved_camera: Option<Camera>,
    pub show_spin: bool,
    pub dialog_tab: u8,
    pub insert_drag: Option<f32>,
    pub show_suppressed: bool,
    /// active set in the Round dashboard
    pub round_set: usize,
    /// assembly session (also kept while one of its parts is open for editing)
    pub asm: Option<assembly::AsmSession>,
    pub new_dialog: Option<assembly::NewDialog>,
    /// drawing session (kept while its model is open in 3D)
    pub drw: Option<drawing::DrawSession>,
    /// Creo window state: Startseite, ribbon minimised, background, dialogs.
    pub creo: creo::UiState,
    /// decoded Creo preview waiting to be uploaded as a texture
    pub creo_thumb_pending: Option<eframe::egui::ColorImage>,
}

impl App {
    pub fn new(gl: Option<&eframe::glow::Context>) -> App {
        let data = ModelData::new_part("PRT0001");
        let result = regen::run(&data);
        let mut app = App {
            data,
            result,
            context: None,
            preview: None,
            selection: vec![],
            hover: None,
            editor: None,
            sketch: None,
            sketch_dlg: None,
            resetup: None,
            reorient: None,
            messages: vec![Message { text: "Willkommen bei Freo. Eine Bezugsebene wählen, dann mit Skizze oder Profil beginnen.".into(), kind: MsgKind::Info }],
            style: Style::ShadedEdges,
            show_planes: true,
            show_axes: true,
            show_points: true,
            show_csys: true,
            show_curves: true,
            show_tags: true,
            transparent: false,
            section: None,
            filter: Filter::Smart,
            tree_selection: vec![],
            measure: false,
            tab: Tab::Model,
            windows: Windows::default(),
            dirty: false,
            path: None,
            undo: vec![],
            redo: vec![],
            camera: Camera::default(),
            renderer: gl.map(|g| Arc::new(Mutex::new(Renderer::new(g)))),
            buffers: Arc::new(ModelBuffers { generation: 0, solid: vec![], edges: vec![] }),
            fit_pending: true,
            renaming: None,
            viewport_size: (800.0, 600.0),
            relations_draft: None,
            player: None,
            screenshot_request: None,
            file_menu: false,
            show_nav: true,
            nav_tab: NavTab::Tree,
            work_dir: default_work_dir(),
            browser_dir: std::env::var("HOME").map(PathBuf::from).map(|h| { let d = h.join("Documents"); if d.is_dir() { d } else { h } }).unwrap_or_else(|_| PathBuf::from("/")),
            expanded: HashSet::new(),
            tree_hover: None,
            tree_hover_ext: None,
            recent: vec![],
            dash_panel: None,
            dash_panel_anchor: eframe::egui::Pos2::ZERO,
            mini: None,
            edit_dims: None,
            saved_camera: None,
            show_spin: true,
            dialog_tab: 0,
            insert_drag: None,
            show_suppressed: true,
            creo: Default::default(),
            creo_thumb_pending: None,
            round_set: 0,
            asm: None,
            new_dialog: None,
            drw: None,
        };
        app.rebuild_buffers();
        app
    }

    // ---------- derived ----------

    pub fn displayed(&self) -> &RegenResult {
        if let Some(v) = self.asm_view() { return &v.display; }
        if self.sketch.is_some() { return self.context.as_ref().unwrap_or(&self.result); }
        if let Some(ed) = &self.editor {
            if ed.pick.is_some() || !ed.preview { return self.context.as_ref().unwrap_or(&self.result); }
            return self.preview.as_ref().or(self.context.as_ref()).unwrap_or(&self.result);
        }
        &self.result
    }

    pub fn say(&mut self, text: impl Into<String>, kind: MsgKind) {
        self.messages.push(Message { text: text.into(), kind });
        if self.messages.len() > 300 { self.messages.drain(0..100); }
    }

    pub fn busy(&self) -> bool { self.editor.is_some() || self.sketch.is_some() || self.placing() }

    pub fn rebuild_buffers(&mut self) {
        let r = self.displayed();
        if self.buffers.generation == r.generation { return; }
        let t = &r.topo;
        // assembly components carry their own colour; parts are tinted by the frame colour
        let colors = self.asm_view().map(|v| v.body_colors.clone()).unwrap_or_default();
        let mut solid = Vec::with_capacity(t.polys.len() * 27);
        for (pi, p) in t.polys.iter().enumerate() {
            let body = (t.tags[pi] >> crate::kernel::topology::BODY_SHIFT) as usize;
            let c = body.checked_sub(1).and_then(|b| colors.get(b)).copied().unwrap_or([1.0; 3]);
            for k in 1..p.len() - 1 {
                for &j in &[0, k, k + 1] {
                    let v = t.positions[p[j]];
                    let n = t.vnormals[pi][j];
                    solid.extend_from_slice(&[v.x as f32, v.y as f32, v.z as f32, n.x as f32, n.y as f32, n.z as f32, c[0], c[1], c[2]]);
                }
            }
        }
        let mut edges = Vec::with_capacity(t.draw_edges.len() * 6);
        for &(a, b) in &t.draw_edges {
            for v in [t.positions[a], t.positions[b]] { edges.extend_from_slice(&[v.x as f32, v.y as f32, v.z as f32]); }
        }
        // Sammelflächen: Creo's purple sheets with their open edges (the shader lights both sides
        // alike: a blue channel above 1.2 marks a quilt)
        let qc = [1.25_f32, 0.78, 1.32];
        for q in &r.quilts {
            for p in &q.polys {
                let n = p.plane.n;
                for k in 1..p.v.len() - 1 {
                    for &j in &[0, k, k + 1] { let v = p.v[j].p; solid.extend_from_slice(&[v.x as f32, v.y as f32, v.z as f32, n.x as f32, n.y as f32, n.z as f32, qc[0], qc[1], qc[2]]); }
                }
            }
            for (a, b, _) in crate::model::surfaces::boundary(&q.polys) { for v in [a, b] { edges.extend_from_slice(&[v.x as f32, v.y as f32, v.z as f32]); } }
        }
        self.buffers = Arc::new(ModelBuffers { generation: r.generation, solid, edges });
    }

    // ---------- regeneration / undo ----------

    pub fn regenerate(&mut self, announce: bool) {
        if self.in_asm() { self.asm_regenerate(announce); return; }
        self.result = regen::run(&self.data);
        if self.editor.is_some() { self.refresh_context(); }
        let valid: Vec<Pick> = self.selection.iter().copied().filter(|p| self.valid(*p)).collect();
        self.selection = valid;
        if announce {
            let n = self.result.errors.len() + self.result.param_errors.len();
            if n == 0 { self.say(format!("Regenerierung erfolgreich abgeschlossen ({:.0} ms).", self.result.time * 1000.0), MsgKind::Success); }
            else { self.say(format!("Regenerierung mit {} Fehler(n) abgeschlossen.", n), MsgKind::Warning); }
        }
        for e in self.result.param_errors.clone() { self.say(e, MsgKind::Error); }
    }

    fn valid(&self, p: Pick) -> bool {
        let r = self.displayed();
        match p {
            Pick::Face(f) => f < r.topo.faces.len(),
            Pick::Edge(e) => e < r.topo.chains.len(),
            Pick::Plane(id) => r.plane(id).is_some(),
            Pick::Axis(id) => r.axis(id).is_some(),
            Pick::Point(id) => r.points.iter().any(|q| q.id == id),
            Pick::Feature(id) => self.data.feature(id).is_some() || self.component(id).is_some(),
        }
    }

    pub fn commit(&mut self, label: &str, f: impl FnOnce(&mut ModelData)) {
        // in an assembly, model-level edits apply to the assembly's own datums
        if self.in_asm() { self.asm_commit(label, |a| f(&mut a.skeleton)); return; }
        self.undo.push(self.data.clone());
        if self.undo.len() > 100 { self.undo.remove(0); }
        self.redo.clear();
        f(&mut self.data);
        self.dirty = true;
        self.regenerate(false);
        if !label.is_empty() { self.say(label, MsgKind::Success); }
        let errs: Vec<(String, String)> = self.result.errors.iter().filter_map(|(id, e)| self.data.feature(*id).map(|f| (f.name.clone(), e.clone()))).collect();
        for (n, e) in errs { self.say(format!("{} fehlgeschlagen: {}", n, e), MsgKind::Error); }
    }

    pub fn do_undo(&mut self) {
        if self.in_asm() { self.asm_undo(false); return; }
        if self.busy() { return; }
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.data, prev));
            self.selection.clear();
            self.dirty = true;
            self.regenerate(false);
            self.say("Rückgängig gemacht.", MsgKind::Info);
        }
    }

    pub fn do_redo(&mut self) {
        if self.in_asm() { self.asm_undo(true); return; }
        if self.busy() { return; }
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.data, next));
            self.selection.clear();
            self.dirty = true;
            self.regenerate(false);
            self.say("Wiederholt.", MsgKind::Info);
        }
    }

    // ---------- feature lifecycle ----------

    pub fn insert_position(&self) -> usize { self.data.insert_index.unwrap_or(self.data.features.len()) }

    pub fn start_feature(&mut self, mut kind: FeatureKind) {
        if self.sketch.is_some() { return; }
        if self.in_asm() { self.say("KEs werden in Teilen erzeugt: eine Komponente öffnen (Rechtsklick ▸ Öffnen), um KEs hinzuzufügen.", MsgKind::Warning); return; }
        if self.editor.is_some() { self.cancel_feature(); }
        let sel = self.selection.clone();
        let plane = self.plane_ref(&sel);
        let edges: Vec<EdgeRef> = sel.iter().filter_map(|p| self.edge_ref(*p)).collect();
        let faces: Vec<FaceRef> = sel.iter().filter_map(|p| self.face_ref(*p)).collect();
        let solid_feats: Vec<u64> = self.tree_selection.iter().copied().filter(|id| self.data.feature(*id).map(|f| f.kind.produces_tool()).unwrap_or(false)).collect();
        match &mut kind {
            FeatureKind::Extrude(_) | FeatureKind::Revolve(_) | FeatureKind::Sweep(_) | FeatureKind::Blend(_) | FeatureKind::Sketch(_) | FeatureKind::Rib(_) | FeatureKind::Helix(_) => {
                if let Some(p) = plane { kind.set_sketch_plane(p); }
            }
            FeatureKind::Thread(d) => {
                if let Some(Pick::Face(f)) = sel.first() { if let Some(init) = self.thread_init(*f) { *d = init; } }
            }
            FeatureKind::Curve(d) => d.points = sel.iter().filter_map(|p| if let Pick::Point(id) = p { Some(*id) } else { None }).collect(),
            FeatureKind::Hole(d) => {
                d.plane = plane;
                if let (Some(PlaneRef::Face(fr)), Some(fp)) = (plane, plane.and_then(|p| self.displayed().frame_for(&p))) {
                    let q = fp.to_local(fr.point);
                    d.x = Dim::v((q.x * 100.0).round() / 100.0);
                    d.y = Dim::v((q.y * 100.0).round() / 100.0);
                }
            }
            FeatureKind::DatumPlane(d) => d.reference = plane,
            FeatureKind::Surface(d) => {
                // selected quilts (or curves for a boundary blend), faces and plane become the references
                let want_curves = d.op == SurfOp::Boundary;
                d.inputs = self.tree_selection.iter().copied().filter(|id| self.data.feature(*id).is_some_and(|f| if want_curves {
                    matches!(&f.kind, FeatureKind::Sketch(_) | FeatureKind::Curve(_)) || matches!(&f.kind, FeatureKind::Surface(s) if matches!(s.op, SurfOp::Intersect | SurfOp::Project))
                } else { matches!(&f.kind, FeatureKind::Surface(s) if s.op.makes_quilt()) })).collect();
                if matches!(d.op, SurfOp::Offset | SurfOp::Copy | SurfOp::Replace) { d.faces = faces.clone(); }
                if !matches!(d.op, SurfOp::Merge | SurfOp::Extend | SurfOp::Thicken | SurfOp::Solidify | SurfOp::Offset | SurfOp::Copy | SurfOp::Replace | SurfOp::Boundary) { d.plane = plane; }
            }
            FeatureKind::Body(d) => { d.target = self.data.default_body; if d.op == BodyOp::Split { d.plane = plane; } }
            FeatureKind::Round(d) => d.sets[0].edges = edges,
            FeatureKind::Chamfer(d) => d.edges = edges,
            FeatureKind::Shell(d) => d.faces = faces,
            FeatureKind::Draft(d) => d.faces = faces,
            FeatureKind::Mirror(d) => { d.plane = plane; d.features = solid_feats; }
            FeatureKind::Pattern(d) => {
                d.features = solid_feats;
                if d.features.is_empty() {
                    if let Some(last) = self.data.features[..self.insert_position()].iter().rev().find(|f| f.kind.produces_tool() && !f.suppressed) { d.features = vec![last.id]; }
                }
            }
            FeatureKind::DatumAxis(d) => {
                let planes: Vec<PlaneRef> = sel.iter().filter_map(|p| self.plane_ref(&[*p])).collect();
                d.p1 = planes.first().copied();
                d.p2 = planes.get(1).copied();
            }
            _ => {}
        }
        let f = Feature { id: self.data.next_id, name: self.data.next_name(&kind), suppressed: false, hidden: false, kind };
        self.editor = Some(Editor { feature: f, is_new: true, index: self.insert_position(), pick: None, preview: true });
        self.tab = Tab::Context;
        self.dash_panel = None;
        self.edit_dims = None;
        self.mini = None;
        self.round_set = 0;
        self.selection.clear();
        self.refresh_context();
        self.auto_pick_target();
        let go_sketch = {
            let ed = self.editor.as_ref().unwrap();
            ed.pick.is_none() && ed.feature.kind.has_sketch() && ed.feature.kind.sketch_plane().is_some()
        };
        if go_sketch { self.begin_sketch(Slot::Main); }
    }

    pub fn edit_feature(&mut self, id: u64) {
        if self.sketch.is_some() { return; }
        if self.in_asm() { if self.component(id).is_some() { self.edit_placement(id); } return; }
        let Some(idx) = self.data.index_of(id) else { return };
        let f = self.data.features[idx].clone();
        match &f.kind {
            FeatureKind::DatumPlane(d) if matches!(d.kind, PlaneKind::Front | PlaneKind::Top | PlaneKind::Right) => {
                self.say("Standardbezugsebenen haben keine editierbare Definition.", MsgKind::Warning);
                return;
            }
            FeatureKind::Csys => { self.say("Das Standardkoordinatensystem kann nicht neu definiert werden.", MsgKind::Warning); return; }
            _ => {}
        }
        let name = f.name.clone();
        self.editor = Some(Editor { feature: f, is_new: false, index: idx, pick: None, preview: true });
        self.tab = Tab::Context;
        self.dash_panel = None;
        self.edit_dims = None;
        self.mini = None;
        self.round_set = 0;
        self.selection.clear();
        self.refresh_context();
        self.say(format!("Definition von {} wird editiert.", name), MsgKind::Prompt);
    }

    pub fn refresh_context(&mut self) {
        let Some(ed) = &self.editor else { self.context = None; self.preview = None; return };
        let before = &self.data.features[..ed.index.min(self.data.features.len())];
        self.context = Some(regen::run_features(&self.data, before, None));
        self.update_preview();
    }

    pub fn update_preview(&mut self) {
        let Some(ed) = &self.editor else { self.preview = None; return };
        let mut feats = self.data.features[..ed.index.min(self.data.features.len())].to_vec();
        feats.push(ed.feature.clone());
        self.preview = Some(regen::run_features(&self.data, &feats, None));
    }

    pub fn editor_error(&self) -> Option<String> {
        let ed = self.editor.as_ref()?;
        self.preview.as_ref()?.errors.get(&ed.feature.id).cloned()
    }

    pub fn update_editor(&mut self, f: impl FnOnce(&mut Feature)) {
        if let Some(ed) = &mut self.editor { f(&mut ed.feature); }
        self.update_preview();
    }

    pub fn set_pick_target(&mut self, t: Option<PickTarget>) {
        let Some(ed) = &mut self.editor else { return };
        ed.pick = if ed.pick == t { None } else { t };
        if ed.pick.is_some() { self.prompt_pick(); }
    }

    /// Status-bar prompt for the active collector (Creo's wording for the Profil/Drehen sketch).
    fn prompt_pick(&mut self) {
        let Some(ed) = &self.editor else { return };
        let Some(t) = ed.pick else { return };
        let p = match t {
            PickTarget::SketchPlane if self.sketch_dlg.is_some() => "Ebene oder Fläche auswählen, um Skizzierebene zu definieren.",
            PickTarget::SketchPlane if ed.feature.kind.accepts_sketch() => "Ebene oder planare Fläche als Skizzierebene auswählen oder eine Skizze auswählen.",
            _ => prompt_for(t),
        };
        self.say(p, MsgKind::Prompt);
    }

    /// Platzierung ▸ Definieren...: opens the Skizze dialog for the sketch plane.
    pub fn open_sketch_dialog(&mut self) {
        let Some(ed) = &mut self.editor else { return };
        if !ed.feature.kind.has_sketch() { return; }
        let prev_flip = matches!(&ed.feature.kind, FeatureKind::Extrude(d) if d.flip);
        self.sketch_dlg = Some(SketchDlg { prev_plane: ed.feature.kind.sketch_plane(), prev_flip });
        self.dash_panel = None;
        ed.pick = Some(PickTarget::SketchPlane);
        self.prompt_pick();
    }

    /// Skizze dialog: Abbrechen restores the plane the feature had before.
    pub fn cancel_sketch_dialog(&mut self) {
        let Some(dlg) = self.sketch_dlg.take() else { return };
        self.update_editor(|f| {
            match &mut f.kind {
                FeatureKind::Extrude(d) => { d.plane = dlg.prev_plane; d.flip = dlg.prev_flip; }
                FeatureKind::Revolve(d) => d.plane = dlg.prev_plane,
                k => if let Some(p) = dlg.prev_plane { k.set_sketch_plane(p) },
            }
        });
        if let Some(ed) = &mut self.editor { ed.pick = if ed.feature.kind.sketch_plane().is_none() { Some(PickTarget::SketchPlane) } else { None }; }
        self.prompt_pick();
    }

    /// The last sketch plane used before the insert position (Skizze dialog: "Vorherig. verwenden").
    pub fn previous_sketch_plane(&self) -> Option<PlaneRef> {
        self.data.features[..self.insert_position().min(self.data.features.len())].iter().rev().filter(|f| !f.suppressed).find_map(|f| f.kind.sketch_plane())
    }

    fn auto_pick_target(&mut self) {
        let Some(ed) = &mut self.editor else { return };
        ed.pick = match &ed.feature.kind {
            k if k.has_sketch() && k.sketch_plane().is_none() => Some(PickTarget::SketchPlane),
            FeatureKind::Hole(d) if d.plane.is_none() => Some(PickTarget::Plane),
            FeatureKind::Round(d) if d.edge_count() == 0 => Some(PickTarget::Edges),
            FeatureKind::Chamfer(d) if d.edges.is_empty() => Some(PickTarget::Edges),
            FeatureKind::Shell(_) => Some(PickTarget::Faces),
            FeatureKind::Draft(d) if d.faces.is_empty() => Some(PickTarget::Faces),
            FeatureKind::Draft(d) if d.hinge.is_none() => Some(PickTarget::Plane),
            FeatureKind::DatumPlane(d) if d.reference.is_none() => Some(PickTarget::Plane),
            FeatureKind::DatumAxis(d) if d.kind == AxisKind::TwoPlanes && d.p1.is_none() => Some(PickTarget::Plane),
            FeatureKind::Mirror(d) if d.plane.is_none() => Some(PickTarget::Plane),
            FeatureKind::Thread(d) if d.face.is_none() => Some(PickTarget::Surface),
            FeatureKind::Curve(d) if d.points.len() < 2 => Some(PickTarget::Points),
            _ => None,
        };
        if ed.pick.is_some() { self.prompt_pick(); }
    }

    pub fn finish_feature(&mut self) {
        if let Some(err) = self.editor_error() {
            let name = self.editor.as_ref().unwrap().feature.name.clone();
            self.say(format!("{} kann nicht abgeschlossen werden: {}", name, err), MsgKind::Error);
            return;
        }
        let Some(ed) = self.editor.take() else { return };
        self.sketch_dlg = None;
        self.context = None;
        self.preview = None;
        self.tab = Tab::Model;
        self.dash_panel = None;
        let f = ed.feature.clone();
        if ed.is_new {
            self.commit(&format!("{} erzeugt.", f.name), |d| {
                let i = ed.index.min(d.features.len());
                d.features.insert(i, f.clone());
                d.next_id = d.next_id.max(f.id + 1);
                if let Some(ii) = d.insert_index { d.insert_index = Some(ii + 1); }
                // new features go into the default body; a new body becomes the default (Creo)
                match &f.kind {
                    FeatureKind::Body(b) if b.op == BodyOp::New => d.default_body = f.id,
                    FeatureKind::Body(_) => {}
                    _ => if d.default_body != 0 { d.feature_body.insert(f.id, d.default_body); },
                }
            });
        } else {
            self.commit(&format!("{} neu definiert.", f.name), |d| {
                if let Some(i) = d.index_of(f.id) { d.features[i] = f.clone(); }
            });
        }
        self.tree_selection = vec![ed.feature.id];
    }

    pub fn cancel_feature(&mut self) {
        if self.sketch.is_some() { self.restore_camera(); }
        self.sketch = None;
        self.sketch_dlg = None;
        self.dash_panel = None;
        self.tab = Tab::Model;
        self.editor = None;
        self.context = None;
        self.preview = None;
        self.say("KE-Erzeugung abgebrochen.", MsgKind::Info);
    }

    // ---------- references ----------

    pub fn plane_ref(&self, sel: &[Pick]) -> Option<PlaneRef> {
        let r = self.displayed();
        for p in sel {
            match p {
                Pick::Plane(id) => return Some(PlaneRef::Datum(*id)),
                Pick::Face(f) if *f < r.topo.faces.len() && r.topo.faces[*f].planar => return Some(PlaneRef::Face(r.topo.face_ref(*f))),
                _ => {}
            }
        }
        None
    }

    pub fn edge_ref(&self, p: Pick) -> Option<EdgeRef> {
        let r = self.displayed();
        if let Pick::Edge(i) = p { if i < r.topo.chains.len() { return Some(r.topo.edge_ref(i)); } }
        None
    }

    pub fn face_ref(&self, p: Pick) -> Option<FaceRef> {
        let r = self.displayed();
        if let Pick::Face(f) = p { if f < r.topo.faces.len() && r.topo.faces[f].planar { return Some(r.topo.face_ref(f)); } }
        None
    }

    pub fn owner(&self, p: Pick) -> Option<u64> {
        let r = self.displayed();
        match p {
            Pick::Face(f) => r.topo.faces.get(f).and_then(|_| r.tag_owner.get(&r.topo.face_tag(f)).copied()),
            Pick::Edge(e) => r.topo.chains.get(e).and_then(|c| r.tag_owner.get(&r.topo.face_tag(c.face_a)).copied()),
            Pick::Plane(id) | Pick::Point(id) => Some(crate::model::assembly::split_datum_id(id).0.unwrap_or(id)),
            Pick::Feature(id) => Some(id),
            Pick::Axis(id) => r.axis(id).map(|a| a.feature),
        }
    }

    pub fn plane_name(&self, r: &Option<PlaneRef>) -> String {
        match r {
            None => "1 Element wählen".into(),
            Some(PlaneRef::Datum(id)) => self.data.feature(*id).map(|f| f.name.clone()).unwrap_or("Fehlt".into()),
            Some(PlaneRef::Face(_)) => "Fläche".into(),
        }
    }

    pub fn axis_name(&self, a: &AxisRef) -> String {
        match a {
            AxisRef::Axis(id) => self.displayed().axis(*id).or(self.result.axis(*id)).map(|x| x.name.clone()).unwrap_or("Achse".into()),
            AxisRef::WorldX => "X-Achse".into(),
            AxisRef::WorldY => "Y-Achse".into(),
            AxisRef::WorldZ => "Z-Achse".into(),
        }
    }

    // ---------- picking ----------

    pub fn handle_pick(&mut self, item: Option<Pick>, additive: bool) {
        if self.placing() {
            if let Some(it) = item { self.place_pick(it); }
            return;
        }
        if let Some(t) = self.editor.as_ref().and_then(|e| e.pick) {
            if let Some(it) = item { self.route_pick(it, t); }
            return;
        }
        let Some(mut item) = item.filter(|p| self.valid(*p)) else {
            if !additive { self.selection.clear(); self.tree_selection.clear(); }
            return;
        };
        if self.filter == Filter::Feature { if let Some(o) = self.owner(item) { item = Pick::Feature(o); } }
        if additive || self.measure {
            if let Some(i) = self.selection.iter().position(|p| *p == item) { self.selection.remove(i); } else { self.selection.push(item); }
            if self.measure && self.selection.len() > 2 { self.selection.remove(0); }
        } else {
            self.selection = vec![item];
        }
        if let Some(o) = self.owner(item) {
            if additive { if !self.tree_selection.contains(&o) { self.tree_selection.push(o); } } else { self.tree_selection = vec![o]; }
        }
        self.describe(item);
    }

    fn describe(&mut self, p: Pick) {
        let r = self.displayed();
        let owner = self.owner(p).map(|id| self.comp_label(id)).unwrap_or_default();
        let s = match p {
            Pick::Face(f) => { let Some(face) = r.topo.faces.get(f) else { return }; format!("Fläche von {}{}, Inhalt {} mm².", owner, if face.planar { " (eben)" } else { "" }, fmt_num(face.area, 2)) }
            Pick::Edge(e) => { let Some(c) = r.topo.chains.get(e) else { return }; format!("Kante von {}, Länge {} mm{}.", owner, fmt_num(c.length(), 3), if c.closed { " (geschlossen)" } else { "" }) }
            Pick::Plane(id) => format!("Bezugsebene {}.", r.plane(id).map(|x| x.name.clone()).unwrap_or_default()),
            Pick::Axis(id) => format!("Achse {}.", r.axis(id).map(|x| x.name.clone()).unwrap_or_default()),
            Pick::Point(_) | Pick::Feature(_) => format!("{} ausgewählt.", owner),
        };
        self.say(s, MsgKind::Info);
    }

    pub fn route_pick(&mut self, item: Pick, target: PickTarget) {
        let plane = self.plane_ref(&[item]);
        let edge = self.edge_ref(item);
        let face = self.face_ref(item);
        let owner = self.owner(item);
        let frame_of_face = plane.and_then(|p| self.displayed().frame_for(&p));
        let thread = if let (PickTarget::Surface, Pick::Face(f)) = (target, item) { self.thread_init(f) } else { None };
        // Profil/Drehen: selecting an existing sketch uses it as the section (copied into the feature)
        let picked_sketch = match (target, item, self.sketch_dlg) {
            (PickTarget::SketchPlane, Pick::Feature(id), None) => self.data.feature(id).and_then(|f| match &f.kind {
                FeatureKind::Sketch(s) if s.plane.is_some() && !s.sketch.is_empty() => Some((f.name.clone(), s.clone())),
                _ => None,
            }),
            _ => None,
        };
        let mut used_sketch = None;
        let Some(ed) = self.editor.as_mut() else { return };
        let mut done = false;
        let mut warn: Option<&str> = None;
        let mut next_target: Option<PickTarget> = None;
        let fid = ed.feature.id;
        match target {
            PickTarget::SketchPlane => match (plane, picked_sketch) {
                (_, Some((name, s))) if ed.feature.kind.accepts_sketch() => {
                    ed.feature.kind.set_sketch_plane(s.plane.unwrap());
                    match &mut ed.feature.kind { FeatureKind::Extrude(d) => d.sketch = s.sketch, FeatureKind::Revolve(d) => d.sketch = s.sketch, _ => {} }
                    used_sketch = Some(name);
                    done = true;
                }
                (Some(p), _) => { ed.feature.kind.set_sketch_plane(p); done = true; }
                _ => warn = Some("Eine Bezugsebene oder eine ebene Fläche wählen."),
            },
            PickTarget::Plane | PickTarget::Plane2 => {
                match (&mut ed.feature.kind, plane, item) {
                    (FeatureKind::DatumPlane(d), _, Pick::Axis(id)) if target == PickTarget::Plane2 => { d.axis = Some(AxisRef::Axis(id)); done = true; }
                    (FeatureKind::DatumAxis(d), Some(p), _) => {
                        if target == PickTarget::Plane && d.kind == AxisKind::TwoPlanes { d.p1 = Some(p); next_target = Some(PickTarget::Plane2); }
                        else if target == PickTarget::Plane2 { d.p2 = Some(p); done = true; } else { d.p1 = Some(p); done = true; }
                    }
                    (FeatureKind::DatumPlane(d), Some(p), _) => { d.reference = Some(p); done = true; }
                    (FeatureKind::Hole(d), Some(p), _) => {
                        d.plane = Some(p);
                        if let (PlaneRef::Face(fr), Some(fp)) = (p, frame_of_face) {
                            let q = fp.to_local(fr.point);
                            d.x = Dim::v((q.x * 100.0).round() / 100.0);
                            d.y = Dim::v((q.y * 100.0).round() / 100.0);
                        }
                        done = true;
                    }
                    (FeatureKind::Mirror(d), Some(p), _) => { d.plane = Some(p); done = true; }
                    (FeatureKind::Draft(d), Some(p), _) => { d.hinge = Some(p); done = true; }
                    (FeatureKind::Surface(d), Some(p), _) => { d.plane = Some(p); done = true; }
                    (FeatureKind::Body(d), Some(p), _) => { d.plane = Some(p); done = true; }
                    _ => warn = Some("Eine Bezugsebene oder eine ebene Fläche wählen."),
                }
            }
            PickTarget::DepthRef => match (plane, &mut ed.feature.kind) {
                (Some(p), FeatureKind::Extrude(d)) => { d.to_ref = Some(p); done = true; }
                _ => warn = Some("Eine Bezugsebene oder eine ebene Fläche wählen."),
            },
            PickTarget::Edges => match edge {
                Some(er) => {
                    let rs = self.round_set;
                    let list = match &mut ed.feature.kind { FeatureKind::Round(d) => { let i = rs.min(d.sets.len() - 1); Some(&mut d.sets[i].edges) } FeatureKind::Chamfer(d) => Some(&mut d.edges), _ => None };
                    if let Some(l) = list {
                        if let Some(i) = l.iter().position(|e| (e.m - er.m).length() < 1e-6) { l.remove(i); } else { l.push(er); }
                    }
                }
                None => warn = Some("Eine Kante wählen."),
            },
            PickTarget::Faces => match face {
                Some(fr) => {
                    let list = match &mut ed.feature.kind { FeatureKind::Shell(d) => Some(&mut d.faces), FeatureKind::Draft(d) => Some(&mut d.faces), FeatureKind::Surface(d) => Some(&mut d.faces), _ => None };
                    if let Some(l) = list {
                        if let Some(i) = l.iter().position(|f| (f.point - fr.point).length() < 1e-6 && f.normal.dot(fr.normal) > 0.999) { l.remove(i); } else { l.push(fr); }
                    }
                }
                None => warn = Some("Eine ebene Fläche wählen."),
            },
            PickTarget::Axis => match (item, &mut ed.feature.kind) {
                (Pick::Axis(id), FeatureKind::Pattern(d)) => { d.axis = AxisRef::Axis(id); d.kind = PatternKind::Axis; done = true; }
                (Pick::Axis(id), FeatureKind::DatumPlane(d)) => { d.axis = Some(AxisRef::Axis(id)); done = true; }
                _ => warn = Some("Eine Achse wählen."),
            },
            PickTarget::Direction => {
                let dir = match item {
                    Pick::Axis(id) => Some(AxisRef::Axis(id)),
                    Pick::Plane(FRONT_ID) => Some(AxisRef::WorldZ),
                    Pick::Plane(TOP_ID) => Some(AxisRef::WorldY),
                    Pick::Plane(RIGHT_ID) => Some(AxisRef::WorldX),
                    _ => None,
                };
                match (dir, &mut ed.feature.kind) {
                    (Some(a), FeatureKind::Pattern(d)) => { d.dir1 = a; done = true; }
                    _ => warn = Some("Eine Achse oder eine Standardbezugsebene wählen."),
                }
            }
            PickTarget::Points => match (item, &mut ed.feature.kind) {
                (Pick::Point(id), FeatureKind::Curve(d)) => toggle(&mut d.points, id),
                _ => warn = Some("Einen Bezugspunkt wählen."),
            },
            PickTarget::Surface => match (thread, &mut ed.feature.kind) {
                (Some(t), FeatureKind::Thread(d)) => { *d = ThreadDef { flip: d.flip, ..t }; done = true; }
                _ => warn = Some("Eine zylindrische Fläche wählen."),
            },
            PickTarget::Features => {
                let ok = owner.filter(|o| *o != fid).filter(|o| self.data.feature(*o).map(|f| f.kind.produces_tool()).unwrap_or(false));
                match (ok, &mut ed.feature.kind) {
                    (Some(o), FeatureKind::Pattern(d)) => toggle(&mut d.features, o),
                    (Some(o), FeatureKind::Mirror(d)) => toggle(&mut d.features, o),
                    _ => warn = Some("Ein Volumen-KE wählen (Profil, Drehen, Zug, Verbund oder Bohrung)."),
                }
            }
        }
        if let Some(w) = warn { self.say(w, MsgKind::Warning); return; }
        let ed = self.editor.as_mut().unwrap();
        if done { ed.pick = None; }
        if let Some(n) = next_target { ed.pick = Some(n); }
        self.update_preview();
        if let Some(n) = next_target { self.say(prompt_for(n), MsgKind::Prompt); }
        if let Some(n) = used_sketch { self.say(format!("{} wird als Skizze verwendet.", n), MsgKind::Info); return; }
        if done && target == PickTarget::SketchPlane {
            // in the Skizze dialog the plane only fills the collector; the Skizze button starts sketching
            if self.sketch_dlg.is_none() { self.begin_sketch(Slot::Main); }
        }
    }

    /// Cosmetic thread fitted to curved face `f`: the ISO coarse thread that fits the cylinder,
    /// over its full length.
    pub fn thread_init(&self, f: usize) -> Option<ThreadDef> {
        let r = self.displayed();
        let t = &r.topo;
        if t.faces.get(f).map_or(true, |x| x.planar) { return None; }
        let (_, _, rad, len, internal) = regen::thread_cylinder(t, f, false)?;
        let (major, pitch) = regen::thread_for_cylinder(rad * 2.0, internal);
        let round = |x: f64| (x * 100.0).round() / 100.0;
        Some(ThreadDef { face: Some(t.surface_ref(f)), diameter: Dim::v(round(major)), pitch: Dim::v(round(pitch)), length: Dim::v(round(len)), flip: false })
    }

    pub fn tree_click(&mut self, id: u64, additive: bool) {
        if self.in_asm() {
            let is_plane = matches!(self.data.feature(id).map(|f| &f.kind), Some(FeatureKind::DatumPlane(_)));
            if self.placing() {
                if is_plane { self.place_pick(Pick::Plane(id)); }
                return;
            }
            if additive { toggle(&mut self.tree_selection, id); } else { self.tree_selection = vec![id]; }
            let pick = if is_plane { Pick::Plane(id) } else { Pick::Feature(id) };
            if additive { if !self.selection.contains(&pick) { self.selection.push(pick); } } else { self.selection = vec![pick]; }
            return;
        }
        if let Some(t) = self.editor.as_ref().and_then(|e| e.pick) {
            let is_plane = matches!(self.data.feature(id).map(|f| &f.kind), Some(FeatureKind::DatumPlane(_)));
            match t {
                PickTarget::Features => { self.route_pick(Pick::Feature(id), t); return; }
                PickTarget::SketchPlane if matches!(self.data.feature(id).map(|f| &f.kind), Some(FeatureKind::Sketch(_))) => { self.route_pick(Pick::Feature(id), t); return; }
                PickTarget::Points if matches!(self.data.feature(id).map(|f| &f.kind), Some(FeatureKind::DatumPoint(_))) => { self.route_pick(Pick::Point(id), t); return; }
                PickTarget::Plane | PickTarget::Plane2 | PickTarget::SketchPlane | PickTarget::Direction | PickTarget::DepthRef if is_plane => { self.route_pick(Pick::Plane(id), t); return; }
                PickTarget::Axis | PickTarget::Direction if self.displayed().axis(id).is_some() => { self.route_pick(Pick::Axis(id), t); return; }
                _ => {}
            }
        }
        if additive { toggle(&mut self.tree_selection, id); } else { self.tree_selection = vec![id]; }
        let pick = match self.data.feature(id).map(|f| &f.kind) {
            Some(FeatureKind::DatumPlane(_)) => Pick::Plane(id),
            Some(FeatureKind::DatumAxis(_)) => Pick::Axis(id),
            _ => Pick::Feature(id),
        };
        if additive { if !self.selection.contains(&pick) { self.selection.push(pick); } } else { self.selection = vec![pick]; }
    }

    // ---------- sketches ----------

    pub fn sketch_for(&self, slot: Slot) -> Option<Sketch> {
        let k = &self.editor.as_ref()?.feature.kind;
        match (k, slot) {
            (FeatureKind::Extrude(d), _) => Some(d.sketch.clone()),
            (FeatureKind::Revolve(d), _) => Some(d.sketch.clone()),
            (FeatureKind::Sketch(d), _) => Some(d.sketch.clone()),
            (FeatureKind::Sweep(d), Slot::Section) => Some(d.section.clone()),
            (FeatureKind::Sweep(d), Slot::Blend(_)) => Some(d.section2.clone()),
            (FeatureKind::Sweep(d), _) => Some(d.trajectory.clone()),
            (FeatureKind::Blend(d), Slot::Blend(i)) => Some(d.sections.get(i).cloned().unwrap_or_default()),
            (FeatureKind::Blend(d), _) => d.sections.first().cloned(),
            (FeatureKind::Pattern(d), _) => Some(d.sketch.clone()),
            (FeatureKind::Rib(d), _) => Some(d.sketch.clone()),
            (FeatureKind::Surface(d), _) => Some(d.sketch.clone()),
            (FeatureKind::Helix(d), Slot::Section) => Some(d.section.clone()),
            (FeatureKind::Helix(d), _) => Some(d.profile.clone()),
            _ => None,
        }
    }

    pub fn begin_sketch(&mut self, slot_in: Slot) {
        let Some(ed) = &self.editor else { return };
        if ed.feature.kind.sketch_plane().is_some() { self.sketch_dlg = None; }
        let mut slot = slot_in;
        if matches!(ed.feature.kind, FeatureKind::Sweep(_) | FeatureKind::Helix(_)) && slot == Slot::Main { slot = Slot::Trajectory; }
        if matches!(ed.feature.kind, FeatureKind::Blend(_)) && slot == Slot::Main { slot = Slot::Blend(0); }
        if matches!(ed.feature.kind, FeatureKind::Pattern(_)) { slot = Slot::Pattern; }
        let ctx = self.context.clone().unwrap_or_else(|| self.result.clone());
        let Some(pr) = ed.feature.kind.sketch_plane() else { self.set_pick_target(Some(PickTarget::SketchPlane)); return };
        let Some(mut frame) = ctx.frame_for(&pr) else { self.say("Die Referenz der Skizzierebene kann nicht aufgelöst werden.", MsgKind::Error); return };
        let mut ghosts: Vec<Vec<V2>> = vec![];
        let mut title = "Skizze".to_string();
        match (&ed.feature.kind, slot) {
            (FeatureKind::Sweep(d), Slot::Section) => {
                let Ok((pts, _)) = d.trajectory.trajectory() else { self.say("Zuerst die Leitkurve skizzieren.", MsgKind::Warning); return };
                if pts.len() < 2 { self.say("Zuerst die Leitkurve skizzieren.", MsgKind::Warning); return; }
                let path: Vec<V3> = pts.iter().map(|p| frame.to_world(*p)).collect();
                frame = regen::sweep_section_frame(&path);
                title = "Zugquerschnitt".into();
            }
            (FeatureKind::Sweep(d), Slot::Blend(_)) => {
                let Ok((pts, _)) = d.trajectory.trajectory() else { self.say("Zuerst die Leitkurve skizzieren.", MsgKind::Warning); return };
                if pts.len() < 2 { self.say("Zuerst die Leitkurve skizzieren.", MsgKind::Warning); return; }
                let path: Vec<V3> = pts.iter().map(|p| frame.to_world(*p)).collect();
                frame = regen::sweep_end_frame(&path);
                for e in &d.section.entities { ghosts.push(d.section.sample(e, 64)); }
                title = "Zug-Verbund — Querschnitt am Ende".into();
            }
            (FeatureKind::Sweep(_), _) => title = "Zugleitkurve".into(),
            (FeatureKind::Helix(d), Slot::Section) => {
                if d.profile.trajectory().is_err() { self.say("Zuerst das Spiralzugprofil skizzieren.", MsgKind::Warning); return; }
                for e in &d.profile.entities { ghosts.push(d.profile.sample(e, 64)); }
                title = "Spiralzugquerschnitt".into();
            }
            (FeatureKind::Helix(_), _) => title = "Spiralzugprofil".into(),
            (FeatureKind::Rib(_), _) => title = "Rippenprofil".into(),
            (FeatureKind::Surface(d), _) => title = format!("{} — Skizze", d.op.name()),
            (FeatureKind::Blend(d), Slot::Blend(i)) if d.rotational => {
                if i > 0 {
                    let Some((ap, au)) = d.sections.first().and_then(|s| s.centerline_axis()) else { self.say("Im ersten Querschnitt eine Mittellinie als Drehachse skizzieren.", MsgKind::Warning); return };
                    let angs: Vec<f64> = (0..i).map(|k| d.offsets.get(k).map(|o| o.value(&ctx.vars, 45.0)).unwrap_or(45.0)).collect();
                    frame = *regen::rotational_blend_frames(&frame, ap, au, &angs, d.flip).0.last().unwrap();
                    for s in d.sections.iter().take(i) { for e in &s.entities { ghosts.push(s.sample(e, 64)); } }
                }
                title = format!("Rotationsverbund-Querschnitt {}", i + 1);
            }
            (FeatureKind::Blend(d), Slot::Blend(i)) => {
                if i > 0 {
                    let offs: Vec<f64> = (0..i).map(|k| d.offsets.get(k).map(|o| o.value(&ctx.vars, 20.0)).unwrap_or(20.0)).collect();
                    frame = *regen::blend_frames(&frame, &offs, d.flip).last().unwrap();
                    for s in d.sections.iter().take(i) { for e in s.profile_entities() { ghosts.push(s.sample(e, 64)); } }
                }
                title = format!("Verbundquerschnitt {}", i + 1);
            }
            _ => {}
        }
        let sk = self.sketch_for(slot).unwrap_or_default();
        let mut named: Vec<(String, [V2; 2])> = projected_refs_named(&frame, &ctx).into_iter().filter(|r| !sk_saved_hidden(&self.sketch_for(slot), &r.1)).collect();
        if let Some(s) = self.sketch_for(slot) { named.extend(s.extra_refs.iter().cloned()); }
        let refs: Vec<[V2; 2]> = named.iter().map(|r| r.1).collect();
        let requirement = match (&ed.feature.kind, slot) {
            (FeatureKind::Revolve(_), _) => sketcher::Requirement::Revolve,
            (FeatureKind::Sweep(_), Slot::Trajectory) => sketcher::Requirement::Trajectory,
            (FeatureKind::Rib(_), _) => sketcher::Requirement::Trajectory,
            (FeatureKind::Helix(_), Slot::Trajectory) => sketcher::Requirement::Helix,
            (FeatureKind::Blend(d), Slot::Blend(0)) if d.rotational => sketcher::Requirement::Revolve,
            (FeatureKind::Sketch(_), _) => sketcher::Requirement::Any,
            (FeatureKind::Surface(d), _) if d.op == SurfOp::Project => sketcher::Requirement::Any,
            (FeatureKind::Pattern(d), _) if d.kind == PatternKind::Curve => sketcher::Requirement::Trajectory,
            _ => sketcher::Requirement::ClosedLoops,
        };
        let mut session = SketchSession::new(sk, frame, refs, ghosts, slot, title.clone(), ctx.vars.clone());
        let (cand_lines, cand_points) = candidate_refs(&frame, &ctx);
        session.set_ref_info(named.into_iter().map(|r| r.0).collect(), axis_ref_names(&frame, &ctx), cand_lines, cand_points);
        session.requirement = requirement;
        if self.saved_camera.is_none() { self.saved_camera = Some(self.camera); }
        self.sketch = Some(session);
        self.sketch_view();
        self.tab = Tab::Context;
        if let Some(e) = &mut self.editor { e.pick = None; }
        if self.sketch.as_ref().is_some_and(|s| s.sketch.is_empty()) && requirement != sketcher::Requirement::Any {
            self.say("Schnitt muss geometrische Elemente für dieses KE enthalten.", MsgKind::Info);
        } else {
            self.say(format!("Skizzierer — {}. Geschlossene Querschnitte zeichnen; Bedingungen und Bemaßungen werden live gelöst.", title), MsgKind::Prompt);
        }
    }

    /// Orient the view parallel to the sketch plane (Creo's Sketch View).
    pub fn sketch_view(&mut self) {
        let Some(s) = &self.sketch else { return };
        let f = s.frame;
        let (mn, mx) = s.fit_bounds();
        let aspect = (self.viewport_size.0 / self.viewport_size.1.max(1.0)) as f64;
        self.camera.rot = glam::DQuat::from_mat3(&glam::DMat3::from_cols(f.x, f.y, f.z()));
        self.camera.target = f.to_world((mn + mx) * 0.5);
        self.camera.spin_center = self.camera.target;
        let ext = mx - mn;
        self.camera.scale = ((ext.x / aspect.max(0.2)).max(ext.y) * 0.5 * 1.5).max(25.0);
    }

    pub fn restore_camera(&mut self) {
        if let Some(c) = self.saved_camera.take() { self.camera = c; }
    }

    pub fn cancel_sketch(&mut self) {
        self.sketch = None;
        self.restore_camera();
        self.say("Skizze abgebrochen.", MsgKind::Info);
    }

    pub fn add_recent(&mut self, p: &std::path::Path) {
        self.recent.retain(|r| r != p);
        self.recent.insert(0, p.to_path_buf());
        self.recent.truncate(10);
    }

    pub fn finish_sketch(&mut self, s: Sketch, slot: Slot) {
        self.sketch = None;
        self.restore_camera();
        self.store_sketch(s, slot);
        self.finish_sketch_rest(slot);
    }

    /// Writes a sketch into the slot of the feature being edited.
    fn store_sketch(&mut self, s: Sketch, slot: Slot) {
        self.update_editor(|f| match (&mut f.kind, slot) {
            (FeatureKind::Extrude(d), _) => d.sketch = s,
            (FeatureKind::Revolve(d), _) => d.sketch = s,
            (FeatureKind::Sketch(d), _) => d.sketch = s,
            (FeatureKind::Sweep(d), Slot::Section) => d.section = s,
            (FeatureKind::Sweep(d), Slot::Blend(_)) => d.section2 = s,
            (FeatureKind::Sweep(d), _) => d.trajectory = s,
            (FeatureKind::Pattern(d), _) => d.sketch = s,
            (FeatureKind::Rib(d), _) => d.sketch = s,
            (FeatureKind::Surface(d), _) => d.sketch = s,
            (FeatureKind::Helix(d), Slot::Section) => d.section = s,
            (FeatureKind::Helix(d), _) => d.profile = s,
            (FeatureKind::Blend(d), Slot::Blend(i)) => {
                while d.sections.len() <= i { d.sections.push(Sketch::default()); }
                d.sections[i] = s;
                let step = if d.rotational { 45.0 } else { 20.0 };
                while d.offsets.len() + 1 < d.sections.len() { d.offsets.push(Dim::v(step)); }
            }
            _ => {}
        });
    }

    /// Skizzen-Setup while sketching: put the current sketch on another plane (or flip the view
    /// side) and continue sketching there.
    pub fn resetup_sketch(&mut self, plane: PlaneRef, flip: bool) {
        let Some(s) = self.sketch.take() else { return };
        let slot = s.slot;
        self.store_sketch(s.sketch, slot);
        self.update_editor(|f| {
            f.kind.set_sketch_plane(plane);
            if flip { match &mut f.kind { FeatureKind::Extrude(d) => d.flip = !d.flip, FeatureKind::Revolve(d) => d.flip = !d.flip, _ => {} } }
        });
        self.refresh_context();
        self.begin_sketch(slot);
    }

    fn finish_sketch_rest(&mut self, slot: Slot) {
        match self.editor_error() {
            Some(e) => self.say(e, MsgKind::Warning),
            None => self.say("Skizze abgeschlossen.", MsgKind::Success),
        }
        let (is_sketch, is_new, sweep_needs_section) = match &self.editor {
            Some(ed) => (
                matches!(ed.feature.kind, FeatureKind::Sketch(_)),
                ed.is_new,
                (matches!(&ed.feature.kind, FeatureKind::Sweep(d) if d.section.is_empty()) || matches!(&ed.feature.kind, FeatureKind::Helix(d) if d.section.is_empty())) && slot == Slot::Trajectory,
            ),
            None => (false, false, false),
        };
        if is_sketch && is_new && self.editor_error().is_none() { self.finish_feature(); }
        if sweep_needs_section {
            let helix = matches!(self.editor.as_ref().map(|e| &e.feature.kind), Some(FeatureKind::Helix(_)));
            self.say(if helix { "Jetzt den Querschnitt am Anfang des Profils skizzieren (er läuft durch die Achse um)." } else { "Jetzt den Zugquerschnitt skizzieren (seine Ebene steht senkrecht zum Anfang der Leitkurve)." }, MsgKind::Prompt);
            self.begin_sketch(Slot::Section);
        }
    }

    // ---------- tree operations ----------

    pub fn delete_features(&mut self, ids: &[u64]) {
        let mut all: Vec<u64> = ids.iter().copied().filter(|id| ![FRONT_ID, TOP_ID, RIGHT_ID, CSYS_ID].contains(id) || self.data.features.len() > 4).collect();
        if all.is_empty() { return; }
        loop {
            let extra: Vec<u64> = self.data.features.iter().filter(|f| !all.contains(&f.id) && f.kind.references().iter().any(|r| all.contains(r))).map(|f| f.id).collect();
            if extra.is_empty() { break; }
            all.extend(extra);
        }
        let n = all.len();
        let children = n - ids.len().min(n);
        let label = if children > 0 { format!("{} KE(s) gelöscht, einschließlich {} abhängiger Kind-KE(s).", n, children) } else { format!("{} KE(s) gelöscht.", n) };
        self.commit(&label, |d| {
            d.features.retain(|f| !all.contains(&f.id));
            if let Some(ii) = d.insert_index { d.insert_index = Some(ii.min(d.features.len())); }
        });
        self.selection.clear();
        self.tree_selection.clear();
    }

    pub fn set_suppressed(&mut self, ids: &[u64], s: bool) {
        let ids = ids.to_vec();
        self.commit(&format!("{}: {} KE(s).", if s { "Unterdrückt" } else { "Fortgesetzt" }, ids.len()), |d| {
            for f in &mut d.features { if ids.contains(&f.id) { f.suppressed = s; } }
        });
    }

    pub fn toggle_hidden(&mut self, ids: &[u64]) {
        let ids = ids.to_vec();
        self.commit("", |d| for f in &mut d.features { if ids.contains(&f.id) { f.hidden = !f.hidden; } });
    }

    pub fn rename(&mut self, id: u64, name: &str) {
        let n = name.trim().to_string();
        if n.is_empty() { return; }
        self.commit(&format!("Umbenannt in {}.", n), |d| if let Some(i) = d.index_of(id) { d.features[i].name = n.clone(); });
    }

    pub fn move_feature(&mut self, id: u64, delta: i32) {
        let Some(i) = self.data.index_of(id) else { return };
        let j = i as i32 + delta;
        if j < 0 || j as usize >= self.data.features.len() { return; }
        let j = j as usize;
        let (a, b) = (&self.data.features[i], &self.data.features[j]);
        if (delta < 0 && a.kind.references().contains(&b.id)) || (delta > 0 && b.kind.references().contains(&a.id)) {
            let msg = format!("Neuordnen nicht möglich: {} und {} stehen in einer Eltern-Kind-Beziehung.", a.name, b.name);
            self.say(msg, MsgKind::Error);
            return;
        }
        let name = a.name.clone();
        self.commit(&format!("{} neu geordnet.", name), |d| d.features.swap(i, j));
    }

    pub fn set_insert_after(&mut self, id: Option<u64>) {
        self.commit(if id.is_some() { "Einfügemodus aktiv — neue KEs werden am Pfeil eingefügt." } else { "Einfügemodus abgebrochen." }, |d| {
            d.insert_index = id.and_then(|id| d.index_of(id)).map(|i| i + 1).filter(|i| *i < d.features.len());
        });
    }

    // ---------- files ----------

    pub fn new_part(&mut self) { self.new_part_named("PRT0001"); }

    pub fn new_part_named(&mut self, name: &str) {
        self.leave_home();
        self.editor = None; self.sketch = None;
        self.asm = None;
        self.drw = None;
        self.selection.clear(); self.tree_selection.clear();
        self.undo.clear(); self.redo.clear();
        self.data = ModelData::new_part_template(name);
        self.tab = Tab::Model;
        self.path = None;
        self.dirty = false;
        self.regenerate(false);
        self.fit_pending = true;
        self.say("Neues Teil erzeugt.", MsgKind::Info);
    }

    pub fn save(&mut self, save_as: bool) {
        if self.drawing_active() { self.save_drawing(save_as); return; }
        if self.in_asm() { self.asm_save(save_as); return; }
        if self.busy() { self.say("Vor dem Speichern das aktive KE abschließen.", MsgKind::Warning); return; }
        let (cur, file) = (self.path.clone(), format!("{}.pfprt", self.data.name.to_lowercase()));
        let path = self.save_target(cur.as_ref(), save_as, &file, creo::SaveKind::Part);
        if let Some(p) = path { self.save_part_to(p); }
    }

    /// Writes the part to `p` and makes it the part's file.
    pub fn save_part_to(&mut self, p: PathBuf) {
        match serde_json::to_string_pretty(&self.data).map_err(|e| e.to_string()).and_then(|s| std::fs::write(&p, s).map_err(|e| e.to_string())) {
            Ok(_) => {
                self.say(format!("{} gespeichert.", p.display()), MsgKind::Success);
                self.add_recent(&p);
                // a component opened from an assembly now lives in this file
                if let Some(name) = self.editing_from_asm().map(|o| o.part.clone()) {
                    let data = self.data.clone();
                    if let Some(pe) = self.asm.as_mut().and_then(|a| a.data.part_mut(&name)) { pe.source = Some(p.clone()); pe.data = data; pe.modified = false; }
                }
                self.path = Some(p);
                self.dirty = false;
            }
            Err(e) => self.say(format!("Speichern fehlgeschlagen: {}", e), MsgKind::Error),
        }
    }

    /// File ▸ Open: Creo's "Datei öffnen" dialog.
    pub fn open(&mut self) { self.open_in(None); }

    pub fn load(&mut self, p: PathBuf) {
        let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if creo::is_creo_file(&name) { self.show_creo_file(&p); return; }
        self.leave_home();
        if creo::is_neutral_file(&name) { self.open_neutral(&p); return; }
        if p.extension().map(|e| e == "pfasm").unwrap_or(false) { self.asm_load(p); return; }
        if p.extension().map(|e| e == "pfdrw").unwrap_or(false) { self.load_drawing(p); return; }
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<ModelData>(&s).map_err(|e| e.to_string())) {
            Ok(d) => {
                self.editor = None; self.sketch = None;
                self.asm = None;
                self.drw = None;
                self.selection.clear(); self.tree_selection.clear();
                self.undo.clear(); self.redo.clear();
                self.tab = Tab::Model;
                let n = d.features.len();
                self.data = d;
                self.path = Some(p.clone());
                self.regenerate(false);
                self.dirty = false;
                self.fit_pending = true;
                self.say(format!("{} geöffnet ({} KEs).", p.display(), n), MsgKind::Success);
                self.add_recent(&p);
                if let Some(dir) = p.parent() { self.browser_dir = dir.to_path_buf(); }
            }
            Err(e) => self.say(format!("Datei konnte nicht geöffnet werden: {}", e), MsgKind::Error),
        }
    }

    pub fn toggle_section(&mut self) {
        if self.section.is_some() { self.section = None; return; }
        let plane = match self.selection.first() { Some(Pick::Plane(id)) => *id, _ => FRONT_ID };
        self.section = Some(SectionView { plane, offset: 0.0, flip: false });
        let name = self.data.feature(plane).map(|f| f.name.clone()).unwrap_or_default();
        self.say(format!("Schnittansicht durch {}. Den Versatz im Register Ansicht anpassen.", name), MsgKind::Info);
    }
}

/// ~/Documents/freo-work (or ~/freo-work without a Documents folder).
pub fn default_work_dir() -> PathBuf {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/tmp"));
    let docs = home.join("Documents");
    if docs.is_dir() { docs.join("freo-work") } else { home.join("freo-work") }
}

impl App {
    /// Folder for dialogs that write files: the file's own folder, else the Arbeitsverzeichnis.
    pub fn save_dir(&self, current: Option<&PathBuf>) -> PathBuf {
        current.and_then(|p| p.parent().map(PathBuf::from)).filter(|d| d.is_dir()).unwrap_or_else(|| { let _ = std::fs::create_dir_all(&self.work_dir); self.work_dir.clone() })
    }

    /// Where Speichern writes: the model's file, or — for a new model — straight into the
    /// Arbeitsverzeichnis like Creo (asking before replacing an existing file). Otherwise
    /// (Speichern unter, or the user keeps the existing file) Freo's own "Speichern unter"
    /// explorer opens and finishes the save itself; then this returns None.
    pub fn save_target(&mut self, current: Option<&PathBuf>, save_as: bool, file_name: &str, kind: creo::SaveKind) -> Option<PathBuf> {
        if !save_as {
            if let Some(p) = current { return Some(p.clone()); }
            let _ = std::fs::create_dir_all(&self.work_dir);
            let p = self.work_dir.join(file_name);
            let replace = || rfd::MessageDialog::new().set_title("Speichern").set_description(format!("{} existiert bereits im Arbeitsverzeichnis. Ersetzen?", file_name)).set_buttons(rfd::MessageButtons::YesNo).show() == rfd::MessageDialogResult::Yes;
            if !p.exists() || replace() { return Some(p); }
        }
        let dir = self.save_dir(current);
        self.browse(creo::BrowsePurpose::SaveAs(kind), dir, file_name.rsplit_once('.').map(|x| x.0).unwrap_or(file_name));
        None
    }
}

pub fn toggle(v: &mut Vec<u64>, id: u64) {
    if let Some(i) = v.iter().position(|x| *x == id) { v.remove(i); } else { v.push(id); }
}

pub fn prompt_for(t: PickTarget) -> &'static str {
    match t {
        PickTarget::SketchPlane => "Ebene oder planare Fläche als Skizzierebene auswählen.",
        PickTarget::Edges => "Kanten wählen (Klicken zum Hinzufügen/Entfernen).",
        PickTarget::Faces => "Flächen wählen (Klicken zum Hinzufügen/Entfernen).",
        PickTarget::Plane => "Eine Bezugsebene oder ebene Fläche wählen.",
        PickTarget::Plane2 => "Die zweite Referenz wählen.",
        PickTarget::Axis => "Eine Achse wählen.",
        PickTarget::Direction => "Eine Achse wählen oder eine Standardbezugsebene für deren Normale.",
        PickTarget::Features => "Volumen-KEs im Modellbaum oder am Modell wählen.",
        PickTarget::DepthRef => "Eine Ebene oder ebene Fläche als Profilende wählen.",
        PickTarget::Points => "Bezugspunkte in der Reihenfolge der Kurve wählen (Klicken zum Hinzufügen/Entfernen).",
        PickTarget::Surface => "Eine zylindrische Fläche (Bohrung oder Welle) für das Gewinde wählen.",
    }
}

/// Model edges lying in the sketch plane and perpendicular datum planes, as sketch references.
pub fn projected_refs(frame: &Frame, r: &RegenResult) -> Vec<[V2; 2]> { projected_refs_named(frame, r).into_iter().map(|x| x.1).collect() }

/// Creo's names of datum-plane references, e.g. "RECHTS:F1(BEZUGSEBENE)".
fn plane_ref_name(_r: &RegenResult, p: &regen::PlaneInfo) -> String {
    // Creo's feature numbers of the start part: RECHTS F1, OBEN F2, VORNE F3
    let n = match p.id { RIGHT_ID => 1, TOP_ID => 2, FRONT_ID => 3, id => id };
    format!("{}:F{}(BEZUGSEBENE)", p.name, n)
}

/// The datum planes that coincide with the sketch axes (horizontal, vertical): the default references.
pub fn axis_ref_names(frame: &Frame, r: &RegenResult) -> [String; 2] {
    let mut out = [String::new(), String::new()];
    for p in &r.planes {
        let n = p.frame.z();
        if n.dot(frame.z()).abs() > 0.01 || (n.dot(p.frame.origin) - n.dot(frame.origin)).abs() > 1e-7 { continue; }
        let d3 = n.cross(frame.z());
        let d2 = V2::new(d3.dot(frame.x), d3.dot(frame.y));
        if d2.x.abs() > 0.9999 && out[0].is_empty() { out[0] = plane_ref_name(r, p); }
        if d2.y.abs() > 0.9999 && out[1].is_empty() { out[1] = plane_ref_name(r, p); }
    }
    out
}

fn sk_saved_hidden(s: &Option<Sketch>, l: &[V2; 2]) -> bool {
    s.as_ref().is_some_and(|s| s.hidden_refs.iter().any(|h| (h[0] - l[0]).length() < 1e-6 && (h[1] - l[1]).length() < 1e-6))
}

/// Model edges and axes projected onto the sketch plane, and points (datum points, axes
/// perpendicular to the plane, model vertices) for Referenzen ▸ Auswählen.
pub fn candidate_refs(frame: &Frame, r: &RegenResult) -> (Vec<(String, [V2; 2])>, Vec<(String, V2)>) {
    let t = &r.topo;
    let tol = 1e-4 * t.bbox.diagonal().max(1.0);
    let mut pts: Vec<(String, V2)> = vec![];
    for p in &r.points { pts.push((format!("{}:F{}(BEZUGSPUNKT)", p.name, p.id), frame.to_local(p.p))); }
    let mut out: Vec<(String, [V2; 2])> = vec![];
    for a in &r.axes {
        let d = V2::new(a.dir.dot(frame.x), a.dir.dot(frame.y));
        let o = frame.to_local(a.point);
        let name = format!("{}:F{}(BEZUGSACHSE)", a.name, a.feature);
        if d.length() < 1e-6 { pts.push((name, o)); } else { let u = d.normalize(); out.push((name, [o - u * 50.0, o + u * 50.0])); }
    }
    for &(a, b) in &t.draw_edges {
        for q in [frame.to_local(t.positions[a]), frame.to_local(t.positions[b])] {
            if pts.len() < 3000 && !pts.iter().any(|x| (x.1 - q).length() < tol) { pts.push(("Eckpunkt".into(), q)); }
        }
    }
    for &(a, b) in &t.draw_edges {
        let (pa, pb) = (frame.to_local(t.positions[a]), frame.to_local(t.positions[b]));
        if (pb - pa).length() < tol { continue; }
        if frame.height(t.positions[a]).abs() < tol && frame.height(t.positions[b]).abs() < tol { continue; }
        if out.iter().any(|o| ((o.1[0] - pa).length() < tol && (o.1[1] - pb).length() < tol) || ((o.1[0] - pb).length() < tol && (o.1[1] - pa).length() < tol)) { continue; }
        out.push(("Kante (projiziert)".into(), [pa, pb]));
        if out.len() > 1500 { break; }
    }
    (out, pts)
}

pub fn projected_refs_named(frame: &Frame, r: &RegenResult) -> Vec<(String, [V2; 2])> {
    let t = &r.topo;
    let tol = 1e-4 * t.bbox.diagonal().max(1.0);
    let mut out: Vec<(String, [V2; 2])> = vec![];
    for &(a, b) in &t.draw_edges {
        let (pa, pb) = (t.positions[a], t.positions[b]);
        if frame.height(pa).abs() < tol && frame.height(pb).abs() < tol { out.push(("Kante".into(), [frame.to_local(pa), frame.to_local(pb)])); }
        if out.len() > 800 { break; }
    }
    for p in &r.planes {
        let n = p.frame.z();
        if n.dot(frame.z()).abs() > 0.01 { continue; }
        let d3 = n.cross(frame.z());
        let d2 = V2::new(d3.dot(frame.x), d3.dot(frame.y));
        if d2.length() < 1e-6 { continue; }
        let w = n.dot(p.frame.origin) - n.dot(frame.origin);
        let nn = V2::new(n.dot(frame.x), n.dot(frame.y));
        let p0 = nn * (w / nn.dot(nn));
        let dd = d2.normalize();
        if p0.length() < 1e-7 && (dd.x.abs() > 0.9999 || dd.y.abs() > 0.9999) { continue; }
        out.push((plane_ref_name(r, p), [p0 - dd * 50.0, p0 + dd * 50.0]));
    }
    out
}
