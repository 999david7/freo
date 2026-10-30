//! Assembly mode: the Component Placement dashboard, the assembly model tree and ribbons,
//! exploded view, bill of materials, global interference, assembly mass properties and
//! opening a component's part for editing (with the rest of the assembly as context).

use super::featuretab::{collector, option_menu};
use super::icons::{self, Icon};
use super::ribbon::{group, icon_button, large, menu_entry, menu_spacing, small, stack, theme::*};
use super::*;
use crate::kernel::csg::CPolygon;
use crate::model::assembly::{self as am, AsmCType, AsmConstraint, AsmRef, AssemblyData, Component, PartCache, PlaceStatus, Placement};
use eframe::egui::{self, pos2, vec2, Color32, Rect, RichText, Stroke};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side { Comp, Asm }

/// The component being placed in the Component Placement dashboard.
#[derive(Clone)]
pub struct PlaceEditor {
    pub comp: Component,
    pub is_new: bool,
    pub active: usize,
    pub status: PlaceStatus,
    /// collector waiting for the next pick; None = decided by the picked component
    pub side: Option<Side>,
}

/// A component's part opened for editing; the assembly state waits here.
pub struct OpenPart {
    pub part: String,
    pub comp: u64,
    /// the other components in the part's coordinates, pos(3) rgba(4) triangles
    pub ghost: Vec<f32>,
    pub undo: Vec<ModelData>,
    pub redo: Vec<ModelData>,
    pub camera: Camera,
    pub path: Option<PathBuf>,
    pub dirty: bool,
}

pub struct AsmSession {
    pub data: AssemblyData,
    pub cache: PartCache,
    pub res: am::AsmResult,
    pub preview: Option<am::AsmResult>,
    pub place: Option<PlaceEditor>,
    pub undo: Vec<AssemblyData>,
    pub redo: Vec<AssemblyData>,
    pub exploded: bool,
    pub explode: f64,
    pub editing: Option<OpenPart>,
    pub show_bom: bool,
    pub interference: Option<Vec<(u64, u64, f64)>>,
}

impl AsmSession {
    pub fn new(data: AssemblyData) -> AsmSession {
        AsmSession { data, cache: PartCache::default(), res: am::AsmResult::default(), preview: None, place: None, undo: vec![], redo: vec![], exploded: false, explode: 0.6, editing: None, show_bom: false, interference: None }
    }
}

/// State of the File ▸ New dialog.
pub struct NewDialog { pub assembly: bool, pub drawing: bool, pub name: String, pub common: String, pub template: bool }

fn status_color(s: &PlaceStatus) -> Color32 {
    match s {
        PlaceStatus::Full | PlaceStatus::Assumed => Color32::from_rgb(20, 130, 50),
        PlaceStatus::Partial(_) | PlaceStatus::Packaged => Color32::from_rgb(190, 120, 0),
        PlaceStatus::Invalid(_) => Color32::from_rgb(200, 36, 28),
    }
}

fn ctype_icon(t: AsmCType) -> Icon {
    match t {
        AsmCType::Automatic => Icon::AsmAuto, AsmCType::Distance => Icon::AsmDistance, AsmCType::Angle => Icon::AsmAngle,
        AsmCType::Parallel => Icon::AsmParallel, AsmCType::Coincident => Icon::AsmCoincident, AsmCType::Normal => Icon::AsmNormal,
        AsmCType::Default => Icon::AsmDefault, AsmCType::Fix => Icon::AsmFix,
    }
}

impl App {
    // ---------- mode ----------

    /// True while the window shows an assembly (not a component opened from it).
    pub fn in_asm(&self) -> bool { self.asm.as_ref().map(|a| a.editing.is_none()).unwrap_or(false) }
    pub fn placing(&self) -> bool { self.asm.as_ref().map(|a| a.place.is_some()).unwrap_or(false) }
    pub fn editing_from_asm(&self) -> Option<&OpenPart> { self.asm.as_ref().and_then(|a| a.editing.as_ref()) }

    /// The assembly result whose display is on screen.
    pub fn asm_view(&self) -> Option<&am::AsmResult> {
        let a = self.asm.as_ref()?;
        if a.editing.is_some() { return None; }
        if a.place.is_some() { if let Some(p) = &a.preview { return Some(p); } }
        Some(&a.res)
    }

    pub fn component(&self, id: u64) -> Option<&Component> { self.asm.as_ref().filter(|a| a.editing.is_none())?.data.component(id) }

    pub fn comp_label(&self, id: u64) -> String {
        match self.component(id) { Some(c) => format!("{}.PRT", c.part), None => self.data.feature(id).map(|f| f.name.clone()).unwrap_or_default() }
    }

    // ---------- lifecycle ----------

    pub fn new_assembly(&mut self, name: &str) {
        self.leave_home();
        self.editor = None; self.sketch = None;
        self.drw = None;
        self.selection.clear(); self.tree_selection.clear();
        self.undo.clear(); self.redo.clear();
        let data = AssemblyData::new(name);
        self.data = data.skeleton.clone();
        self.asm = Some(AsmSession::new(data));
        self.path = None;
        self.dirty = false;
        self.tab = Tab::Model;
        self.asm_regenerate(false);
        self.fit_pending = true;
        self.say(format!("Neue Baugruppe {} erzeugt. Mit Zusammenbauen die erste Komponente hinzufügen.", name), MsgKind::Info);
    }

    pub fn asm_regenerate(&mut self, announce: bool) {
        let Some(a) = &mut self.asm else { return };
        let factor = if a.exploded && a.place.is_none() { a.explode } else { 0.0 };
        a.res = am::regen(&a.data, &mut a.cache, &am::RegenOpts { explode: factor, editing: None });
        self.data = a.data.skeleton.clone();
        self.result = a.res.display.clone();
        let msgs = a.res.messages.clone();
        let time = a.res.display.time;
        let ids: Vec<u64> = a.data.components.iter().map(|c| c.id).collect();
        self.tree_selection.retain(|id| ids.contains(id) || *id <= CSYS_ID);
        self.selection.clear();
        if announce {
            if msgs.is_empty() { self.say(format!("Baugruppe erfolgreich regeneriert ({:.0} ms).", time * 1000.0), MsgKind::Success); }
            else { self.say(format!("Baugruppe mit {} Problem(en) regeneriert.", msgs.len()), MsgKind::Warning); }
        }
        if announce { for m in msgs { self.say(m, MsgKind::Warning); } }
    }

    pub fn asm_commit(&mut self, label: &str, f: impl FnOnce(&mut AssemblyData)) {
        let Some(a) = &mut self.asm else { return };
        a.undo.push(a.data.clone());
        if a.undo.len() > 60 { a.undo.remove(0); }
        a.redo.clear();
        f(&mut a.data);
        self.dirty = true;
        self.asm_regenerate(false);
        if !label.is_empty() { self.say(label, MsgKind::Success); }
        let msgs = self.asm.as_ref().map(|a| a.res.messages.clone()).unwrap_or_default();
        for m in msgs { self.say(m, MsgKind::Warning); }
    }

    pub fn asm_undo(&mut self, redo: bool) {
        if self.busy() { return; }
        let Some(a) = &mut self.asm else { return };
        let (from, to) = if redo { (&mut a.redo, &mut a.undo) } else { (&mut a.undo, &mut a.redo) };
        let Some(prev) = from.pop() else { return };
        to.push(std::mem::replace(&mut a.data, prev));
        self.dirty = true;
        self.asm_regenerate(false);
        self.say(if redo { "Wiederholt." } else { "Rückgängig gemacht." }, MsgKind::Info);
    }

    pub fn asm_save(&mut self, save_as: bool) {
        if self.placing() { self.say("Platzierung der Komponente vor dem Speichern abschließen.", MsgKind::Warning); return; }
        let Some(name) = self.asm.as_ref().map(|a| a.data.name.clone()) else { return };
        let cur = self.path.clone();
        let path = self.save_target(cur.as_ref(), save_as, &format!("{}.pfasm", name.to_lowercase()), super::creo::SaveKind::Assembly);
        if let Some(p) = path { self.asm_save_to(p); }
    }

    /// Writes the assembly (and its modified part files) to `p`.
    pub fn asm_save_to(&mut self, p: PathBuf) {
        let Some(a) = &mut self.asm else { return };
        let mut parts_saved = 0;
        let mut errs = vec![];
        for pe in &mut a.data.parts {
            if !pe.modified { continue; }
            if let Some(src) = &pe.source {
                match serde_json::to_string_pretty(&pe.data).map_err(|e| e.to_string()).and_then(|s| std::fs::write(src, s).map_err(|e| e.to_string())) {
                    Ok(_) => { pe.modified = false; parts_saved += 1; }
                    Err(e) => errs.push(format!("{}: {}", pe.data.name, e)),
                }
            }
        }
        match serde_json::to_string_pretty(&a.data).map_err(|e| e.to_string()).and_then(|s| std::fs::write(&p, s).map_err(|e| e.to_string())) {
            Ok(_) => {
                let extra = if parts_saved > 0 { format!(" und {} geänderte Teiledatei(en)", parts_saved) } else { String::new() };
                self.say(format!("{}{} gespeichert.", p.display(), extra), MsgKind::Success);
                self.add_recent(&p);
                self.path = Some(p);
                self.dirty = false;
            }
            Err(e) => self.say(format!("Speichern fehlgeschlagen: {}", e), MsgKind::Error),
        }
        for e in errs { self.say(format!("Teil {} konnte nicht gespeichert werden", e), MsgKind::Error); }
    }

    pub fn asm_load(&mut self, p: PathBuf) {
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<AssemblyData>(&s).map_err(|e| e.to_string())) {
            Ok(d) => {
                self.editor = None; self.sketch = None;
                self.drw = None;
                self.selection.clear(); self.tree_selection.clear();
                self.undo.clear(); self.redo.clear();
                let n = d.components.len();
                self.data = d.skeleton.clone();
                self.asm = Some(AsmSession::new(d));
                self.path = Some(p.clone());
                self.tab = Tab::Model;
                self.asm_regenerate(false);
                self.dirty = false;
                self.fit_pending = true;
                self.say(format!("Baugruppe {} geöffnet ({} Komponenten).", p.display(), n), MsgKind::Success);
                self.add_recent(&p);
                if let Some(dir) = p.parent() { self.browser_dir = dir.to_path_buf(); }
            }
            Err(e) => self.say(format!("Baugruppe konnte nicht geöffnet werden: {}", e), MsgKind::Error),
        }
    }

    // ---------- adding components ----------

    /// Model ▸ Assemble: pick a part file and place it.
    pub fn assemble_from_file(&mut self) {
        if !self.in_asm() || self.placing() { return; }
        let dir = self.browser_dir.clone();
        self.browse(super::creo::BrowsePurpose::Assemble, dir, "");
    }

    pub fn assemble_path(&mut self, p: PathBuf) {
        let data = match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<ModelData>(&s).map_err(|e| e.to_string())) {
            Ok(d) => d,
            Err(e) => { self.say(format!("Teil konnte nicht gelesen werden: {}", e), MsgKind::Error); return; }
        };
        let Some(a) = &mut self.asm else { return };
        let name = data.name.clone();
        let existing = a.data.part(&name).is_some();
        if !existing { a.data.add_part(data, Some(p.clone())); }
        if existing { self.say(format!("{} ist bereits in Sitzung; eine weitere Instanz wird zusammengebaut.", name), MsgKind::Info); }
        self.assemble_part(&name);
    }

    /// Opens the placement dashboard for a new instance of `part`.
    pub fn assemble_part(&mut self, part: &str) {
        let Some(a) = &mut self.asm else { return };
        if a.place.is_some() || a.editing.is_some() { return; }
        let Some(pdata) = a.data.part(part).map(|p| p.data.clone()) else { return };
        let pres = a.cache.get(&pdata);
        let id = a.data.next_id.max(am::FIRST_COMPONENT_ID);
        let first = a.data.components.iter().all(|c| c.suppressed);
        let b = a.res.display.topo.bbox;
        let pb = pres.topo.bbox;
        let start = if first || b.is_empty() || pb.is_empty() { Placement::default() } else {
            let gap = 0.3 * b.diagonal().max(pb.diagonal());
            Placement::translation(V3::new(b.max.x - pb.min.x + gap, b.center().y - pb.center().y, b.center().z - pb.center().z))
        };
        let mut comp = Component::new(id, part, start);
        comp.constraints.push(AsmConstraint::new(if first { AsmCType::Default } else { AsmCType::Automatic }));
        // Komponentenschnittstelle: the part brings its own references, only the partners are picked
        let iface = pdata.interface.clone();
        if !first && !iface.is_empty() {
            use crate::model::features::IfaceRef;
            comp.constraints = iface.iter().map(|it| {
                let r = match it.r { IfaceRef::Plane(i) => AsmRef::Plane { comp: id, id: i }, IfaceRef::Axis(i) => AsmRef::Axis { comp: id, id: i }, IfaceRef::Point(i) => AsmRef::Point { comp: id, id: i }, IfaceRef::Surface { face, planar } => AsmRef::Surface { comp: id, face, planar } };
                AsmConstraint { a: Some(r), ..AsmConstraint::new(it.ty) }
            }).collect();
        }
        let iface_note = (!first && !iface.is_empty()).then(|| format!("Komponentenschnittstelle: {} Bedingung(en). Partner für „{}“ ({}) in der Baugruppe wählen.", iface.len(), iface[0].name, iface[0].ty.name()));
        a.place = Some(PlaceEditor { comp, is_new: true, active: 0, status: PlaceStatus::Packaged, side: None });
        self.tab = Tab::Context;
        self.dash_panel = None;
        self.selection.clear();
        self.mini = None;
        self.place_update();
        if first { self.say(format!("{} wird platziert: die erste Komponente liegt an der Standardposition. Zum Übernehmen OK klicken.", part), MsgKind::Prompt); }
        else { self.say(format!("{} wird platziert: eine Referenz an der Komponente wählen, dann die passende Referenz an der Baugruppe. Ziehen zum Verschieben (⇧ ziehen dreht).", part), MsgKind::Prompt); }
        if let Some(n) = iface_note { self.say(n, MsgKind::Prompt); }
    }

    /// Model ▸ Create: a new empty part assembled at the default location, opened for editing.
    pub fn create_component(&mut self, name: &str) {
        let Some(a) = &mut self.asm else { return };
        if a.place.is_some() || a.editing.is_some() { return; }
        let name = name.trim().to_uppercase();
        if name.is_empty() || a.data.part(&name).is_some() || name == a.data.name {
            self.say(format!("Ein Modell mit dem Namen {} existiert bereits.", name), MsgKind::Error);
            return;
        }
        let id = a.data.next_id.max(am::FIRST_COMPONENT_ID);
        let nm = name.clone();
        self.asm_commit(&format!("Komponente {}.PRT erzeugt.", name), |d| {
            d.add_part(ModelData::new_part(&nm), None);
            let mut c = Component::new(id, &nm, Placement::default());
            c.constraints.push(AsmConstraint::new(AsmCType::Default));
            d.components.push(c);
            d.next_id = id + 1;
        });
        self.asm_open_part(id);
    }

    // ---------- placement dashboard ----------

    pub fn edit_placement(&mut self, id: u64) {
        let Some(a) = &mut self.asm else { return };
        if a.place.is_some() || a.editing.is_some() { return; }
        let Some(mut comp) = a.data.component(id).cloned() else { return };
        if let Some(pl) = a.res.placements.get(&id) { comp.placement = *pl; }
        let active = comp.constraints.iter().position(|c| !c.complete()).unwrap_or(comp.constraints.len().saturating_sub(1));
        if comp.constraints.is_empty() { comp.constraints.push(AsmConstraint::new(AsmCType::Automatic)); }
        let part = comp.part.clone();
        a.place = Some(PlaceEditor { comp, is_new: false, active, status: PlaceStatus::Packaged, side: None });
        // placement is edited with the assembly put together
        self.tab = Tab::Context;
        self.dash_panel = None;
        self.selection.clear();
        self.mini = None;
        self.place_update();
        self.say(format!("Platzierung von {}.PRT wird editiert.", part), MsgKind::Prompt);
    }

    /// Re-solves the component being placed and refreshes the preview display.
    pub fn place_update(&mut self) {
        let Some(a) = &mut self.asm else { return };
        let Some(ed) = &mut a.place else { return };
        let res = am::regen(&a.data, &mut a.cache, &am::RegenOpts { explode: 0.0, editing: Some(&ed.comp) });
        if let Some(pl) = res.placements.get(&ed.comp.id) { ed.comp.placement = *pl; }
        ed.status = res.status.get(&ed.comp.id).cloned().unwrap_or(PlaceStatus::Packaged);
        // keep the orientation chosen on the first solve so Flip is relative to it
        let comps: Vec<Component> = a.data.components.iter().filter(|c| c.id != ed.comp.id).cloned().chain(std::iter::once(ed.comp.clone())).collect();
        let (_, _, signs) = am::place_component(&ed.comp, &comps, &res.parts, &res.skeleton, &res.placements);
        for (c, s) in ed.comp.constraints.iter_mut().zip(signs) { if (c.sign == 0.0 || c.sign == 2.0) && c.complete() && c.ty.needs_refs() { c.sign = s; } }
        a.preview = Some(res);
        self.preview = None;
    }

    pub fn place_editor(&self) -> Option<&PlaceEditor> { self.asm.as_ref()?.place.as_ref() }

    /// Maps a selection in the assembly display to a placement reference.
    pub fn asm_ref_of(&self, p: Pick) -> Option<AsmRef> {
        let v = self.asm_view()?;
        let t = &v.display.topo;
        let inv = |comp: u64| v.shown.get(&comp).map(|pl| pl.inverse());
        match p {
            Pick::Plane(id) => Some(match am::split_datum_id(id) { (None, l) => AsmRef::AsmPlane(l), (Some(c), l) => AsmRef::Plane { comp: c, id: l } }),
            Pick::Axis(id) => match am::split_datum_id(id) { (Some(c), l) => Some(AsmRef::Axis { comp: c, id: l }), _ => None },
            Pick::Point(id) => match am::split_datum_id(id) { (Some(c), l) => Some(AsmRef::Point { comp: c, id: l }), _ => None },
            Pick::Face(f) => {
                let face = t.faces.get(f)?;
                let comp = *v.display.tag_owner.get(&t.face_tag(f))?;
                let x = inv(comp)?;
                if !face.planar && t.surface_axis(f).is_none() { return None; }
                let fr = t.surface_ref(f);
                Some(AsmRef::Surface { comp, face: FaceRef { point: x.point(fr.point), normal: x.vector(fr.normal) }, planar: face.planar })
            }
            Pick::Edge(e) => {
                let c = t.chains.get(e)?;
                if !c.is_straight() { return None; }
                let comp = *v.display.tag_owner.get(&t.face_tag(c.face_a))?;
                let x = inv(comp)?;
                Some(AsmRef::Edge { comp, a: x.point(c.points[0]), b: x.point(*c.points.last()?) })
            }
            Pick::Feature(_) => None,
        }
    }

    /// The displayed entity of a placement reference (for highlighting).
    pub fn asm_ref_pick(&self, r: &AsmRef) -> Option<Pick> {
        let v = self.asm_view()?;
        let t = &v.display.topo;
        match *r {
            AsmRef::AsmPlane(id) => Some(Pick::Plane(id)),
            AsmRef::Plane { comp, id } => Some(Pick::Plane(am::comp_datum_id(comp, id))),
            AsmRef::Axis { comp, id } => Some(Pick::Axis(am::comp_datum_id(comp, id))),
            AsmRef::Point { comp, id } => Some(Pick::Point(am::comp_datum_id(comp, id))),
            AsmRef::Surface { comp, face, planar } => {
                let pl = v.shown.get(&comp)?;
                let (p, n) = (pl.point(face.point), pl.vector(face.normal));
                let f = if planar { t.match_face(p, n) } else { t.match_surface(p, n) }?;
                (v.display.tag_owner.get(&t.face_tag(f)) == Some(&comp)).then_some(Pick::Face(f))
            }
            AsmRef::Edge { comp, a, b } => {
                let pl = v.shown.get(&comp)?;
                let (wa, wb) = (pl.point(a), pl.point(b));
                let mut best = None;
                let mut best_d = f64::INFINITY;
                for (i, c) in t.chains.iter().enumerate() {
                    if v.display.tag_owner.get(&t.face_tag(c.face_a)) != Some(&comp) { continue; }
                    let d = c.distance_to(wa) + c.distance_to(wb) + c.distance_to((wa + wb) * 0.5);
                    if d < best_d { best_d = d; best = Some(i); }
                }
                best.filter(|_| best_d < 1e-3 * t.bbox.diagonal().max(1.0)).map(Pick::Edge)
            }
        }
    }

    pub fn place_ref_picks(&self) -> Vec<Pick> {
        let Some(ed) = self.place_editor() else { return vec![] };
        ed.comp.constraints.iter().flat_map(|c| [c.a, c.b]).flatten().filter_map(|r| self.asm_ref_pick(&r)).collect()
    }

    pub fn ref_label(&self, r: &Option<AsmRef>) -> String {
        let Some(a) = &self.asm else { return String::new() };
        match r {
            None => "1 Element wählen".into(),
            Some(r) => am::ref_name(r, &a.data, a.place.as_ref().map(|e| &e.comp), self.asm_view()),
        }
    }

    /// A pick while the Component Placement dashboard is open.
    pub fn place_pick(&mut self, p: Pick) {
        let Some(r) = self.asm_ref_of(p) else { self.say("Eine Fläche, Bezugsebene, Achse, einen Punkt oder eine gerade Kante wählen.", MsgKind::Warning); return };
        let Some(a) = &mut self.asm else { return };
        let order: Vec<u64> = a.data.components.iter().map(|c| c.id).collect();
        let Some(ed) = &mut a.place else { return };
        let mine = r.comp() == Some(ed.comp.id);
        let side = ed.side.unwrap_or(if mine { Side::Comp } else { Side::Asm });
        if side == Side::Comp && !mine { self.say("Eine Referenz an der zu platzierenden Komponente wählen.", MsgKind::Warning); return; }
        if side == Side::Asm && mine { self.say("Eine Referenz an der Baugruppe oder einer anderen Komponente wählen.", MsgKind::Warning); return; }
        if let (Side::Asm, Some(c)) = (side, r.comp()) {
            let me = order.iter().position(|x| *x == ed.comp.id).unwrap_or(order.len());
            if order.iter().position(|x| *x == c).map(|i| i > me).unwrap_or(true) {
                self.say("Referenzen müssen von Komponenten stammen, die im Modellbaum früher platziert sind.", MsgKind::Warning);
                return;
            }
        }
        if ed.active >= ed.comp.constraints.len() || !ed.comp.constraints[ed.active].ty.needs_refs() {
            ed.comp.constraints.push(AsmConstraint::new(AsmCType::Automatic));
            ed.active = ed.comp.constraints.len() - 1;
        }
        let c = &mut ed.comp.constraints[ed.active];
        match side { Side::Comp => c.a = Some(r), Side::Asm => c.b = Some(r) }
        c.sign = 0.0;
        ed.side = None;
        let complete = c.complete();
        if complete && c.ty == AsmCType::Automatic { c.ty = if c.offset != 0.0 { AsmCType::Distance } else { AsmCType::Coincident }; }
        self.place_update();
        if !complete { self.say(format!("Jetzt die Referenz auf {} wählen.", if side == Side::Comp { "der Baugruppe" } else { "der Komponente" }), MsgKind::Prompt); return; }
        let Some(ed) = self.asm.as_mut().and_then(|a| a.place.as_mut()) else { return };
        let st = ed.status.clone();
        match st {
            PlaceStatus::Full | PlaceStatus::Assumed => self.say("Status: Vollständig bestimmt. Zum Abschließen OK klicken.", MsgKind::Success),
            PlaceStatus::Invalid(e) => self.say(format!("Bedingungen ungültig: {}. Typ ändern, umkehren oder andere Referenzen wählen.", e), MsgKind::Error),
            _ => {
                // a constraint still open (from the component interface) comes next
                if let Some(i) = ed.comp.constraints.iter().position(|c| !c.complete()) {
                    ed.active = i;
                    let ty = ed.comp.constraints[i].ty.name();
                    self.say(format!("Status: {}. Partner für die Schnittstellen-Bedingung {} ({}) in der Baugruppe wählen.", st.text(), i + 1, ty), MsgKind::Prompt);
                    return;
                }
                ed.comp.constraints.push(AsmConstraint::new(AsmCType::Automatic));
                ed.active = ed.comp.constraints.len() - 1;
                self.say(format!("Status: {}. Referenzen für die nächste Bedingung wählen.", st.text()), MsgKind::Prompt);
            }
        }
    }

    /// Moves the component being placed (screen-plane drag; rotates with `rotate`).
    pub fn place_drag(&mut self, dx: f32, dy: f32, rotate: bool) {
        let wpp = 2.0 * self.camera.scale / self.viewport_size.1.max(1.0) as f64;
        let (right, up) = (self.camera.right(), self.camera.up());
        let Some(a) = &mut self.asm else { return };
        let Some(ed) = &mut a.place else { return };
        let pl = ed.comp.placement;
        ed.comp.placement = if rotate {
            let pres = a.cache.get(&a.data.part(&ed.comp.part).map(|p| p.data.clone()).unwrap_or_default_part());
            let c = pl.point(if pres.topo.bbox.is_empty() { V3::ZERO } else { pres.topo.bbox.center() });
            pl.rotated(c, up, dx as f64 * 0.01).rotated(c, right, dy as f64 * 0.01)
        } else { pl.moved(right * (dx as f64 * wpp) - up * (dy as f64 * wpp)) };
        self.place_update();
    }

    pub fn place_finish(&mut self) {
        let Some(a) = &mut self.asm else { return };
        let Some(mut ed) = a.place.take() else { return };
        a.preview = None;
        ed.comp.constraints.retain(|c| c.complete());
        let invalid = matches!(ed.status, PlaceStatus::Invalid(_));
        self.tab = Tab::Model;
        self.dash_panel = None;
        let comp = ed.comp.clone();
        let label = format!("{}.PRT {} — {}.", comp.part, if ed.is_new { "zusammengebaut" } else { "Platzierung neu definiert" }, ed.status.text());
        self.asm_commit(&label, |d| {
            match d.components.iter().position(|c| c.id == comp.id) { Some(i) => d.components[i] = comp.clone(), None => d.components.push(comp.clone()) }
            d.next_id = d.next_id.max(comp.id + 1);
        });
        if invalid { self.say("Die Komponente behält ihre letzte gültige Position; Definition editieren, um die Bedingungen zu korrigieren.", MsgKind::Warning); }
        self.tree_selection = vec![ed.comp.id];
    }

    pub fn place_cancel(&mut self) {
        if let Some(a) = &mut self.asm { a.place = None; a.preview = None; }
        self.tab = Tab::Model;
        self.dash_panel = None;
        self.say("Komponentenplatzierung abgebrochen.", MsgKind::Info);
    }

    // ---------- tree operations ----------

    pub fn asm_delete(&mut self, ids: &[u64]) {
        let Some(a) = &self.asm else { return };
        let ids: Vec<u64> = ids.iter().copied().filter(|id| a.data.component(*id).is_some()).collect();
        if ids.is_empty() { return; }
        let n_refs: usize = a.data.components.iter().filter(|c| !ids.contains(&c.id)).flat_map(|c| c.constraints.iter())
            .filter(|k| [k.a, k.b].iter().flatten().any(|r| r.comp().map(|x| ids.contains(&x)).unwrap_or(false))).count();
        let label = if n_refs > 0 { format!("{} Komponente(n) gelöscht; {} Bedingung(en) anderer Komponenten mit Referenz darauf entfernt.", ids.len(), n_refs) } else { format!("{} Komponente(n) gelöscht.", ids.len()) };
        self.asm_commit(&label, |d| {
            d.components.retain(|c| !ids.contains(&c.id));
            for c in &mut d.components {
                c.constraints.retain(|k| ![k.a, k.b].iter().flatten().any(|r| r.comp().map(|x| ids.contains(&x)).unwrap_or(false)));
            }
            let used: Vec<String> = d.components.iter().map(|c| c.part.clone()).collect();
            d.parts.retain(|p| used.contains(&p.data.name));
        });
        self.selection.clear();
        self.tree_selection.clear();
    }

    pub fn asm_set_flag(&mut self, ids: &[u64], suppress: Option<bool>, hide: Option<bool>) {
        let ids = ids.to_vec();
        let label = match (suppress, hide) {
            (Some(true), _) => format!("{} Komponente(n) unterdrückt.", ids.len()),
            (Some(false), _) => format!("{} Komponente(n) zurückgeholt.", ids.len()),
            _ => String::new(),
        };
        self.asm_commit(&label, |d| for c in &mut d.components {
            if !ids.contains(&c.id) { continue; }
            if let Some(s) = suppress { c.suppressed = s; }
            if let Some(h) = hide { c.hidden = h; }
        });
    }

    pub fn toggle_exploded(&mut self) {
        let Some(a) = &mut self.asm else { return };
        a.exploded = !a.exploded;
        let on = a.exploded;
        self.asm_regenerate(false);
        self.say(if on { "Explosionsansicht ein: mit dem Explosionsregler im Register Ansicht die Komponenten auseinanderziehen." } else { "Explosionsansicht aus." }, MsgKind::Info);
    }

    // ---------- opening a component ----------

    /// Opens a component's part in this window with the rest of the assembly as context.
    pub fn asm_open_part(&mut self, comp: u64) {
        let Some(a) = &mut self.asm else { return };
        if a.place.is_some() || a.editing.is_some() { return; }
        let Some(c) = a.data.component(comp).cloned() else { return };
        let Some(pe) = a.data.part(&c.part).cloned() else { return };
        let inv = a.res.placements.get(&comp).copied().unwrap_or(c.placement).inverse();
        let mut ghost = vec![];
        for o in a.data.components.iter().filter(|o| o.id != comp && !o.suppressed && !o.hidden) {
            let (Some(part), Some(pl)) = (a.res.parts.get(&o.part), a.res.placements.get(&o.id)) else { continue };
            let col = a.data.color_of(&o.part);
            let x = Placement { rot: inv.rot * pl.rot, pos: inv.point(pl.pos) }.xform();
            for p in &part.polys {
                let q: CPolygon = p.transformed(&x);
                for k in 1..q.v.len() - 1 {
                    for v in [q.v[0].p, q.v[k].p, q.v[k + 1].p] { ghost.extend_from_slice(&[v.x as f32, v.y as f32, v.z as f32, col[0], col[1], col[2], 0.16]); }
                }
            }
        }
        a.editing = Some(OpenPart {
            part: c.part.clone(), comp, ghost,
            undo: std::mem::take(&mut self.undo), redo: std::mem::take(&mut self.redo),
            camera: self.camera, path: self.path.clone(), dirty: self.dirty,
        });
        self.data = pe.data.clone();
        self.path = pe.source.clone();
        self.dirty = false;
        self.selection.clear();
        self.tree_selection.clear();
        self.tab = Tab::Model;
        self.regenerate(false);
        self.fit_pending = true;
        self.say(format!("{}.PRT aus {} geöffnet. Danach Zurück zur Baugruppe verwenden.", c.part, a_name(&self.asm)), MsgKind::Prompt);
    }

    /// Writes the edited part back into the assembly and shows the assembly again.
    pub fn asm_return(&mut self) {
        if self.busy() { self.say("Zuerst das aktive KE abschließen.", MsgKind::Warning); return; }
        let Some(a) = &mut self.asm else { return };
        let Some(op) = a.editing.take() else { return };
        let part = self.data.clone();
        let changed = a.data.part(&op.part).map(|p| p.data != part).unwrap_or(false);
        let part_dirty = self.dirty;
        if changed {
            a.undo.push(a.data.clone());
            a.redo.clear();
            let old = op.part.clone();
            let new = part.name.clone();
            if let Some(pe) = a.data.part_mut(&old) { pe.data = part; pe.modified = part_dirty || pe.modified; }
            if new != old { for c in &mut a.data.components { if c.part == old { c.part = new.clone(); } } }
        }
        self.undo = op.undo;
        self.redo = op.redo;
        self.camera = op.camera;
        self.path = op.path;
        self.dirty = op.dirty || changed;
        self.selection.clear();
        self.tree_selection = vec![op.comp];
        self.asm_regenerate(false);
        self.say(if changed { format!("Zurück zur Baugruppe; {} aktualisiert.", op.part) } else { "Zurück zur Baugruppe.".into() }, MsgKind::Info);
    }

    // ---------- analysis ----------

    /// Assembly mass properties from each component's own density.
    pub fn asm_mass(&self) -> Option<export::MassProps> {
        let a = self.asm.as_ref().filter(|a| a.editing.is_none())?;
        let mut parts = vec![];
        for c in a.data.components.iter().filter(|c| !c.suppressed) {
            let polys = am::world_polys(c, &a.res);
            if polys.is_empty() { continue; }
            let density = a.data.part(&c.part).map(|p| p.data.density).unwrap_or(7.85e-6);
            let r = RegenResult { topo: Arc::new(crate::kernel::topology::Topology::new(&polys)), polys, ..Default::default() };
            parts.push(export::mass_properties(&r, density));
        }
        let m: f64 = parts.iter().map(|p| p.mass).sum();
        if parts.is_empty() || m <= 0.0 { return None; }
        let cog = parts.iter().fold(V3::ZERO, |s, p| s + p.cog * p.mass) / m;
        let mut inertia = [0.0; 6];
        for p in &parts {
            let d = p.cog - cog;
            let add = [d.y * d.y + d.z * d.z, d.x * d.x + d.z * d.z, d.x * d.x + d.y * d.y, -d.x * d.y, -d.y * d.z, -d.z * d.x];
            for k in 0..6 { inertia[k] += p.inertia[k] + p.mass * add[k]; }
        }
        Some(export::MassProps {
            volume: parts.iter().map(|p| p.volume).sum(), area: parts.iter().map(|p| p.area).sum(), mass: m, cog, inertia,
            bbox: a.res.display.topo.bbox,
        })
    }

    // ---------- model tree ----------

    pub fn asm_tree(&mut self, ui: &mut egui::Ui) {
        self.tree_header(ui);
        let q = self.creo.tree_filter.trim().to_lowercase();
        ui.add_space(2.0);
        let Some(a) = &self.asm else { return };
        let data = a.data.clone();
        let status = self.asm_view().map(|v| v.status.clone()).unwrap_or_default();
        let parts = self.asm_view().map(|v| v.parts.clone()).unwrap_or_default();
        let placing = a.place.as_ref().map(|e| (e.comp.id, e.is_new, e.comp.part.clone()));
        self.tree_hover = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let _ = Self::paint_row(ui, 0, Icon::Assembly, &format!("{}.ASM", data.name), TEXT, true, false, None);
            for f in &data.skeleton.features {
                if f.hidden && !self.show_suppressed { continue; }
                if !q.is_empty() && !f.name.to_lowercase().contains(&q) { continue; }
                let sel = self.tree_selection.contains(&f.id);
                let (r, _) = Self::paint_row(ui, 1, icons::for_feature(&f.kind), &f.name, if f.hidden { Color32::from_rgb(130, 136, 144) } else { TEXT }, false, sel, None);
                if r.hovered() { self.tree_hover = Some(f.id); }
                if r.clicked() { let add = ui.input(|i| i.modifiers.shift || i.modifiers.command); self.tree_click(f.id, add); }
            }
            let mut comps: Vec<(Component, bool)> = data.components.iter().map(|c| (c.clone(), false)).collect();
            if let Some((id, true, part)) = &placing { comps.push((Component::new(*id, part, Placement::default()), true)); }
            for (c, new) in comps {
                if c.suppressed && !self.show_suppressed { continue; }
                if !q.is_empty() && !c.part.to_lowercase().contains(&q) { continue; }
                let st = status.get(&c.id).cloned().unwrap_or(PlaceStatus::Packaged);
                let mut color = TEXT;
                let mut icon = Icon::Part;
                if c.suppressed { color = Color32::from_rgb(140, 146, 154); }
                else if let PlaceStatus::Invalid(_) = st { color = Color32::from_rgb(200, 36, 28); icon = Icon::Failed; }
                if c.hidden { color = Color32::from_rgb(130, 136, 144); }
                if placing.as_ref().map(|p| p.0) == Some(c.id) { color = Color32::from_rgb(20, 100, 210); }
                let sel = self.tree_selection.contains(&c.id) || self.tree_hover_ext == Some(c.id);
                let open = self.expanded.contains(&c.id);
                let label = if new { format!("{}.PRT  (wird platziert)", c.part) } else { format!("{}.PRT", c.part) };
                let (r, exp) = Self::paint_row(ui, 1, icon, &label, color, false, sel, Some(open));
                let x = r.rect.left() + 4.0 + 16.0 + 28.0;
                if c.suppressed { ui.painter().rect_filled(Rect::from_center_size(pos2(x, r.rect.center().y + 5.0), vec2(5.0, 5.0)), 0.0, Color32::BLACK); }
                else if !st.is_full() { ui.painter().rect_stroke(Rect::from_center_size(pos2(x, r.rect.center().y + 5.0), vec2(5.0, 5.0)), 0.0, Stroke::new(1.0_f32, Color32::from_rgb(40, 40, 40)), egui::StrokeKind::Inside); }
                if let Some(e) = exp { if e.clicked() { if !self.expanded.remove(&c.id) { self.expanded.insert(c.id); } continue; } }
                let tip = format!("{} — {}", st.text(), data.part(&c.part).and_then(|p| p.source.as_ref()).map(|s| s.display().to_string()).unwrap_or("internes Teil".into()));
                let r = r.on_hover_text(tip);
                if r.hovered() { self.tree_hover = Some(c.id); }
                if r.clicked() { let add = ui.input(|i| i.modifiers.shift || i.modifiers.command); self.tree_click(c.id, add); }
                if r.double_clicked() && !self.placing() { self.edit_placement(c.id); }
                if !new { r.context_menu(|ui| self.comp_menu(ui, c.id)); }
                if !open { continue; }
                // Placement folder with the constraints, then the part's features
                let (pr, _) = Self::paint_row(ui, 2, Icon::AsmCoincident, "Platzierung", TEXT, false, false, None);
                pr.on_hover_text(st.text());
                for k in &c.constraints {
                    let txt = format!("{}  {} → {}", k.ty.name(), k.a.map(|r| am::ref_name(&r, &data, None, self.asm_view())).unwrap_or("—".into()), k.b.map(|r| am::ref_name(&r, &data, None, self.asm_view())).unwrap_or("—".into()));
                    let txt = if k.ty.needs_refs() { txt } else { k.ty.name().to_string() };
                    let (kr, _) = Self::paint_row(ui, 3, ctype_icon(k.ty), &txt, if k.enabled { TEXT } else { Color32::from_rgb(140, 146, 154) }, false, false, None);
                    if kr.double_clicked() && !self.placing() { self.edit_placement(c.id); }
                }
                if let Some(pe) = data.part(&c.part) {
                    let errs = parts.get(&c.part).map(|r| r.errors.clone()).unwrap_or_default();
                    for f in &pe.data.features {
                        let (fr, _) = Self::paint_row(ui, 2, if errs.contains_key(&f.id) { Icon::Failed } else { icons::for_feature(&f.kind) }, &f.name, if f.suppressed { Color32::from_rgb(140, 146, 154) } else { TEXT }, false, false, None);
                        let fr = fr.on_hover_text("Komponente öffnen, um ihre KEs zu bearbeiten");
                        if fr.clicked() {
                            // datum planes of a component can be picked as placement references from the tree
                            if self.placing() && matches!(f.kind, FeatureKind::DatumPlane(_)) { self.place_pick(Pick::Plane(am::comp_datum_id(c.id, f.id))); }
                            else { self.tree_click(c.id, false); }
                        }
                        if fr.double_clicked() && !self.placing() { self.asm_open_part(c.id); }
                    }
                }
            }
        });
    }

    pub fn comp_menu(&mut self, ui: &mut egui::Ui, id: u64) {
        menu_spacing(ui);
        let Some(c) = self.component(id).cloned() else { return };
        let ids = if self.tree_selection.contains(&id) { self.tree_selection.clone() } else { vec![id] };
        let item = |ui: &mut egui::Ui, icon: Icon, label: &str| menu_entry(ui, icon, label, true, false).clicked();
        if item(ui, Icon::EditDef, "Definition editieren") { self.edit_placement(id); ui.close_menu(); }
        if item(ui, Icon::Open, "Öffnen") { self.asm_open_part(id); ui.close_menu(); }
        ui.separator();
        if c.hidden { if item(ui, Icon::Show, "Einblenden") { self.asm_set_flag(&ids, None, Some(false)); ui.close_menu(); } }
        else if item(ui, Icon::Hide, "Ausblenden") { self.asm_set_flag(&ids, None, Some(true)); ui.close_menu(); }
        if c.suppressed { if item(ui, Icon::Resume, "Zurückholen") { self.asm_set_flag(&ids, Some(false), None); ui.close_menu(); } }
        else if item(ui, Icon::Suppress, "Unterdrücken") { self.asm_set_flag(&ids, Some(true), None); ui.close_menu(); }
        if item(ui, Icon::Assemble, "Weitere Instanz zusammenbauen") { let p = c.part.clone(); self.assemble_part(&p); ui.close_menu(); }
        ui.separator();
        if item(ui, Icon::Delete, "Löschen") { self.asm_delete(&ids); ui.close_menu(); }
    }

    // ---------- ribbons ----------

    pub fn asm_model_ribbon(&mut self, ui: &mut egui::Ui) {
        let free = !self.placing();
        self.return_to_drawing_group(ui);
        let (nu, nr) = self.asm.as_ref().map(|a| (!a.undo.is_empty(), !a.redo.is_empty())).unwrap_or_default();
        let comp_sel: Option<u64> = self.tree_selection.iter().copied().find(|id| self.component(*id).is_some());
        group(ui, "Operationen", |ui| {
            if large(ui, Icon::Regenerate, "Regenerieren", true, false).on_hover_text("Baugruppe regenerieren (⌘G)").clicked() { self.regenerate(true); }
            stack(ui, |ui| {
                if small(ui, Icon::Undo, "Rückgängig", nu && free, false).clicked() { self.asm_undo(false); }
                if small(ui, Icon::Redo, "Wiederholen", nr && free, false).clicked() { self.asm_undo(true); }
                if small(ui, Icon::Delete, "Löschen", comp_sel.is_some() && free, false).clicked() { let ids = self.tree_selection.clone(); self.asm_delete(&ids); }
            });
        });
        group(ui, "Komponente", |ui| {
            let (top, bot) = super::ribbon::split(ui, Icon::Assemble, "Zusammenbauen", free, false);
            let id = ui.id().with("assemble_menu");
            if top.on_hover_text("Eine Teiledatei in die Baugruppe einbauen").clicked() { self.assemble_from_file(); }
            if bot.clicked() { ui.memory_mut(|m| m.toggle_popup(id)); }
            egui::popup_below_widget(ui, id, &bot, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                menu_spacing(ui);
                ui.set_min_width(200.0);
                if menu_entry(ui, Icon::Assemble, "Zusammenbauen...", true, false).clicked() { self.assemble_from_file(); }
                if menu_entry(ui, Icon::CreateComp, "Erzeugen...", true, false).clicked() { self.windows.create_comp = Some(self.next_part_name()); }
                if let Some(c) = comp_sel.and_then(|id| self.component(id)).map(|c| c.part.clone()) {
                    if menu_entry(ui, Icon::Assemble, &format!("{} wiederholen", c), true, false).clicked() { self.assemble_part(&c); }
                }
            });
            if large(ui, Icon::CreateComp, "Erzeugen", free, false).on_hover_text("Ein neues Teil in der Baugruppe erzeugen").clicked() { self.windows.create_comp = Some(self.next_part_name()); }
            stack(ui, |ui| {
                if small(ui, Icon::EditDef, "Platzierung editieren", comp_sel.is_some() && free, false).clicked() { if let Some(id) = comp_sel { self.edit_placement(id); } }
                if small(ui, Icon::Open, "Komponente öffnen", comp_sel.is_some() && free, false).clicked() { if let Some(id) = comp_sel { self.asm_open_part(id); } }
                if small(ui, Icon::DragComp, "Komponenten ziehen", comp_sel.is_some() && free, false).on_hover_text("Platzierung editieren, dann die Komponente im Grafikfenster ziehen").clicked() { if let Some(id) = comp_sel { self.edit_placement(id); } }
            });
        });
        group(ui, "Modellanzeige", |ui| {
            let ex = self.asm.as_ref().map(|a| a.exploded).unwrap_or(false);
            if large(ui, Icon::Explode, "Explosions-\nansicht", free, ex).on_hover_text("Explosionszustand der Baugruppe umschalten").clicked() { self.toggle_exploded(); }
        });
        group(ui, "Untersuchen", |ui| {
            let on = self.asm.as_ref().map(|a| a.show_bom).unwrap_or(false);
            if large(ui, Icon::Bom, "Stück-\nliste", true, on).clicked() { if let Some(a) = &mut self.asm { a.show_bom = !a.show_bom; } }
            if large(ui, Icon::Interference, "Globale\nDurchdringung", free, false).clicked() { self.run_interference(); }
        });
    }

    pub fn asm_analysis_ribbon(&mut self, ui: &mut egui::Ui) {
        group(ui, "Messen", |ui| {
            if large(ui, Icon::Measure, "Messen", true, self.measure).on_hover_text("Abstand, Winkel, Länge, Fläche messen").clicked() {
                self.measure = !self.measure;
                self.selection.clear();
                if self.measure { self.say("Messen: ein oder zwei Elemente wählen.", MsgKind::Prompt); }
            }
        });
        group(ui, "Modellbericht", |ui| {
            if large(ui, Icon::MassProps, "Masse-\neigenschaften", true, self.windows.mass).clicked() { self.windows.mass = !self.windows.mass; }
            if large(ui, Icon::Bom, "Stück-\nliste", true, self.asm.as_ref().map(|a| a.show_bom).unwrap_or(false)).clicked() { if let Some(a) = &mut self.asm { a.show_bom = !a.show_bom; } }
        });
        group(ui, "Geometrie prüfen", |ui| {
            if large(ui, Icon::Interference, "Globale\nDurchdringung", !self.placing(), false).clicked() { self.run_interference(); }
        });
    }

    pub fn asm_tools_ribbon(&mut self, ui: &mut egui::Ui) {
        group(ui, "Untersuchen", |ui| {
            if large(ui, Icon::Bom, "Stück-\nliste", true, self.asm.as_ref().map(|a| a.show_bom).unwrap_or(false)).clicked() { if let Some(a) = &mut self.asm { a.show_bom = !a.show_bom; } }
            if large(ui, Icon::ModelInfo, "Modell-\ninfo", true, self.windows.info).clicked() { self.windows.info = !self.windows.info; }
        });
        group(ui, "Dienstprogramme", |ui| {
            if large(ui, Icon::Options, "Steuerung", true, self.windows.controls).clicked() { self.windows.controls = !self.windows.controls; }
            if large(ui, Icon::Help, "Info", true, self.windows.about).clicked() { self.windows.about = !self.windows.about; }
        });
    }

    /// View tab group shown for assemblies.
    pub fn asm_view_group(&mut self, ui: &mut egui::Ui) {
        let Some((ex, mut f)) = self.asm.as_ref().map(|a| (a.exploded, a.explode)) else { return };
        group(ui, "Explosion", |ui| {
            if large(ui, Icon::Explode, "Explosions-\nansicht", !self.placing(), ex).clicked() { self.toggle_exploded(); }
            if ex {
                stack(ui, |ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Explosionsabstand").size(11.0).color(LABEL));
                    ui.spacing_mut().slider_width = 130.0;
                    if ui.add(egui::Slider::new(&mut f, 0.05..=2.0).show_value(false)).changed() {
                        if let Some(a) = &mut self.asm { a.explode = f; }
                        self.asm_regenerate(false);
                    }
                });
            }
        });
    }

    fn next_part_name(&self) -> String { self.asm.as_ref().map(|a| a.data.next_part_name()).unwrap_or("PRT0001".into()) }

    /// The Component Placement dashboard.
    pub fn placement_ribbon(&mut self, ui: &mut egui::Ui) {
        let Some(ed) = self.place_editor().cloned() else { return };
        let mut e = ed.clone();
        let mut ok = false;
        let mut cancel = false;
        let n = e.comp.constraints.len();
        let ai = e.active.min(n.saturating_sub(1));
        let left = ui.available_width();
        ui.allocate_ui_with_layout(vec2(left, super::ribbon::RIB_H + 22.0), egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            group(ui, "Platzierung", |ui| {
                stack(ui, |ui| {
                    if let Some(c) = e.comp.constraints.get_mut(ai) {
                        ui.horizontal(|ui| {
                            let opts: Vec<(AsmCType, Icon, &str)> = AsmCType::MENU.iter().map(|t| (*t, ctype_icon(*t), t.name())).collect();
                            if option_menu(ui, "asm_ctype", &mut c.ty, &opts) { c.sign = 0.0; }
                            ui.label(RichText::new(c.ty.name()).size(12.0));
                            if c.ty.has_offset() {
                                ui.add_space(4.0);
                                let unit = if c.ty == AsmCType::Angle { "°" } else { "" };
                                ui.add(egui::DragValue::new(&mut c.offset).speed(0.5).suffix(unit).max_decimals(3));
                            }
                            if c.ty.needs_refs() && icon_button(ui, Icon::Flip, "Ausrichtung der Bedingung umkehren", c.complete(), false, 18.0).clicked() { c.flip(); }
                        });
                    }
                    ui.label(RichText::new(format!("Status: {}", ed.status.text())).size(11.5).color(status_color(&ed.status)));
                });
            });
            if let Some(c) = e.comp.constraints.get(ai).cloned() {
                if c.ty.needs_refs() {
                    group(ui, "Referenzen", |ui| {
                        stack(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Komponente").size(11.0).color(LABEL));
                                if collector(ui, &self.ref_label(&c.a), e.side == Some(Side::Comp), 150.0).clicked() { e.side = if e.side == Some(Side::Comp) { None } else { Some(Side::Comp) }; }
                            });
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Baugruppe  ").size(11.0).color(LABEL));
                                if collector(ui, &self.ref_label(&c.b), e.side == Some(Side::Asm), 150.0).clicked() { e.side = if e.side == Some(Side::Asm) { None } else { Some(Side::Asm) }; }
                            });
                        });
                    });
                }
            }
            group(ui, "Bedingungen", |ui| {
                stack(ui, |ui| {
                    if small(ui, Icon::New, "Neue Bedingung", true, false).clicked() {
                        e.comp.constraints.push(AsmConstraint::new(AsmCType::Automatic));
                        e.active = e.comp.constraints.len() - 1;
                        e.side = None;
                    }
                    if small(ui, Icon::Delete, "Bedingung löschen", n > 0, false).clicked() && ai < e.comp.constraints.len() {
                        e.comp.constraints.remove(ai);
                        e.active = ai.saturating_sub(1);
                    }
                    ui.horizontal(|ui| {
                        if ui.small_button("◀").clicked() && e.active > 0 { e.active -= 1; e.side = None; }
                        ui.label(RichText::new(format!("{} von {}", ai + 1, n.max(1))).size(11.5));
                        if ui.small_button("▶").clicked() && e.active + 1 < n { e.active += 1; e.side = None; }
                    });
                });
            });
            group(ui, "Optionen", |ui| {
                stack(ui, |ui| {
                    if small(ui, Icon::Requirements, "Annahmen zulassen", true, e.comp.assumptions).on_hover_text("Eine freie Drehung um eine koaxiale Achse als fixiert annehmen").clicked() { e.comp.assumptions = !e.comp.assumptions; }
                    ui.label(RichText::new("Komponente ziehen zum Verschieben,\n⇧ ziehen zum Drehen").size(10.5).color(LABEL));
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.add_space(6.0);
                if large(ui, Icon::Cancel, "Abbrechen", true, false).on_hover_text("Abbrechen (Esc)").clicked() { cancel = true; }
                if large(ui, Icon::Ok, "OK", true, false).on_hover_text("Platzierung abschließen (Enter)").clicked() { ok = true; }
            });
        });
        let changed_geom = e.comp != ed.comp;
        if changed_geom || e.active != ed.active || e.side != ed.side {
            if let Some(x) = self.asm.as_mut().and_then(|a| a.place.as_mut()) { *x = e; }
            if changed_geom { self.place_update(); }
        }
        if ok { self.place_finish(); }
        if cancel { self.place_cancel(); }
    }

    pub fn placement_panel_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(4.0);
            for p in ["Platzierung", "Eigenschaften"] {
                let font = egui::FontId::proportional(13.5);
                let w = ui.fonts(|f| f.layout_no_wrap(p.to_string(), font.clone(), Color32::BLACK).size().x) + 26.0;
                let (rect, resp) = ui.allocate_exact_size(vec2(w, 27.0), egui::Sense::click());
                let active = self.dash_panel == Some(p);
                if active { self.dash_panel_anchor = rect.left_bottom(); }
                let painter = ui.painter();
                if active {
                    painter.rect_filled(rect, egui::CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, Color32::WHITE);
                    painter.rect_stroke(rect, egui::CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, Stroke::new(1.0_f32, BORDER), egui::StrokeKind::Inside);
                } else if resp.hovered() { painter.rect_filled(rect, 3.0, HOVER_BG); }
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, p, font, if active { TEXT } else { Color32::from_rgb(40, 90, 170) });
                if resp.clicked() { self.dash_panel = if active { None } else { Some(p) }; self.dash_panel_anchor = rect.left_bottom(); }
            }
        });
    }

    /// Slide-down Placement / Properties panel of the placement dashboard.
    pub fn placement_panel_ui(&mut self, ctx: &egui::Context) {
        let (Some(ed), Some(panel)) = (self.place_editor().cloned(), self.dash_panel) else { return };
        let mut e = ed.clone();
        let mut close = false;
        egui::Area::new(egui::Id::new("place_panel")).fixed_pos(self.dash_panel_anchor).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(Color32::WHITE).inner_margin(10.0).show(ui, |ui| {
                ui.set_width(430.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(panel).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { if ui.small_button("✕").clicked() { close = true; } });
                });
                ui.separator();
                if panel == "Eigenschaften" {
                    ui.label(format!("Komponente: {}.PRT  (ID {})", e.comp.part, e.comp.id));
                    let src = self.asm.as_ref().and_then(|a| a.data.part(&e.comp.part)).and_then(|p| p.source.clone());
                    ui.label(RichText::new(format!("Datei: {}", src.map(|s| s.display().to_string()).unwrap_or("intern (in der Baugruppe gespeichert)".into()))).size(11.0).color(LABEL));
                    return;
                }
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(220.0);
                        ui.label(RichText::new("Satz 1 (benutzerdefiniert)").size(11.5).color(LABEL));
                        let n = e.comp.constraints.len();
                        let mut remove = None;
                        for i in 0..n {
                            let c = e.comp.constraints[i].clone();
                            ui.horizontal(|ui| {
                                let mut en = c.enabled;
                                if ui.checkbox(&mut en, "").changed() { e.comp.constraints[i].enabled = en; }
                                icons::paint(ui.painter(), Rect::from_min_size(ui.cursor().min + vec2(0.0, 1.0), vec2(16.0, 16.0)), ctype_icon(c.ty), true);
                                ui.add_space(18.0);
                                let txt = if c.complete() || !c.ty.needs_refs() { c.ty.name().to_string() } else { format!("{} (unvollständig)", c.ty.name()) };
                                if ui.selectable_label(e.active == i, txt).clicked() { e.active = i; e.side = None; }
                                if ui.small_button("✕").on_hover_text("Bedingung löschen").clicked() { remove = Some(i); }
                            });
                        }
                        if let Some(i) = remove { e.comp.constraints.remove(i); e.active = e.active.min(e.comp.constraints.len().saturating_sub(1)); }
                        if ui.selectable_label(false, RichText::new("→ Neue Bedingung").color(Color32::from_rgb(30, 100, 200))).clicked() {
                            e.comp.constraints.push(AsmConstraint::new(AsmCType::Automatic));
                            e.active = e.comp.constraints.len() - 1;
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        if let Some(c) = e.comp.constraints.get_mut(e.active) {
                            ui.label(RichText::new("Bedingungstyp").size(11.0).color(LABEL));
                            egui::ComboBox::from_id_salt("pp_type").width(150.0).selected_text(c.ty.name()).show_ui(ui, |ui| {
                                for t in AsmCType::MENU { if ui.selectable_value(&mut c.ty, t, t.name()).changed() { c.sign = 0.0; } }
                            });
                            if c.ty.has_offset() {
                                ui.label(RichText::new(if c.ty == AsmCType::Angle { "Winkel" } else { "Versatz" }).size(11.0).color(LABEL));
                                ui.add(egui::DragValue::new(&mut c.offset).speed(0.5).max_decimals(3));
                            }
                            if c.ty.needs_refs() && ui.add_enabled(c.complete(), egui::Button::new("Umkehren")).clicked() { c.flip(); }
                        }
                        ui.add_space(6.0);
                        ui.label(RichText::new("Status").size(11.0).color(LABEL));
                        ui.label(RichText::new(ed.status.text()).color(status_color(&ed.status)));
                        ui.checkbox(&mut e.comp.assumptions, "Annahmen zulassen");
                    });
                });
            });
        });
        if close { self.dash_panel = None; }
        let changed = e.comp != ed.comp;
        if changed || e.active != ed.active || e.side != ed.side {
            if let Some(x) = self.asm.as_mut().and_then(|a| a.place.as_mut()) { *x = e; }
            if changed { self.place_update(); }
        }
    }

    // ---------- analysis windows ----------

    pub fn run_interference(&mut self) {
        let Some(a) = &mut self.asm else { return };
        let hits = am::interference(&a.data, &a.res);
        let n = hits.len();
        a.interference = Some(hits);
        if n == 0 { self.say("Globale Durchdringung: keine sich durchdringenden Komponenten.", MsgKind::Success); }
        else { self.say(format!("Globale Durchdringung: {} durchdringende(s) Paar(e).", n), MsgKind::Warning); }
    }

    pub fn asm_windows(&mut self, ctx: &egui::Context) {
        let Some(a) = &self.asm else { return };
        if a.editing.is_some() { return; }
        let mut show_bom = a.show_bom;
        if show_bom {
            let rows = am::bom(&a.data, &a.res);
            let name = a.data.name.clone();
            let data = a.data.clone();
            let mut export = false;
            egui::Window::new("Stückliste").open(&mut show_bom).default_width(560.0).show(ctx, |ui| {
                ui.label(RichText::new(format!("Baugruppe {} enthält:", name)).strong());
                ui.add_space(4.0);
                egui::Grid::new("bom").striped(true).min_col_width(40.0).show(ui, |ui| {
                    for h in ["Pos.", "Teil", "Menge", "Volumen (mm³)", "Masse (kg)", "Gesamt (kg)"] { ui.label(RichText::new(h).strong()); }
                    ui.end_row();
                    for (i, r) in rows.iter().enumerate() {
                        ui.label(format!("{}", i + 1));
                        ui.label(format!("{}.PRT", r.part)).on_hover_text(r.source.as_ref().map(|s| s.display().to_string()).unwrap_or("internes Teil".into()));
                        ui.label(format!("{}", r.qty));
                        ui.label(fmt_num(r.volume, 2));
                        ui.label(fmt_num(r.mass, 4));
                        ui.label(fmt_num(r.mass * r.qty as f64, 4));
                        ui.end_row();
                    }
                    let total: f64 = rows.iter().map(|r| r.mass * r.qty as f64).sum();
                    let count: usize = rows.iter().map(|r| r.qty).sum();
                    ui.label("");
                    ui.label(RichText::new("Gesamt").strong());
                    ui.label(RichText::new(format!("{}", count)).strong());
                    ui.label("");
                    ui.label("");
                    ui.label(RichText::new(fmt_num(total, 4)).strong());
                    ui.end_row();
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("CSV exportieren...").clicked() { export = true; }
                    if ui.button("Kopieren").clicked() { ui.ctx().copy_text(am::bom_csv(&data, &rows)); }
                });
            });
            if export {
                let dir = self.save_dir(self.path.as_ref());
                self.browse(super::creo::BrowsePurpose::BomCsv(am::bom_csv(&data, &rows)), dir, &format!("{}_bom", name.to_lowercase()));
            }
            if let Some(a) = &mut self.asm { a.show_bom = show_bom; }
        }
        let Some(a) = &self.asm else { return };
        if let Some(hits) = a.interference.clone() {
            let mut open = true;
            let mut select = None;
            let mut again = false;
            egui::Window::new("Globale Durchdringung").open(&mut open).default_width(380.0).show(ctx, |ui| {
                if hits.is_empty() { ui.label(RichText::new("Keine Durchdringung gefunden.").color(Color32::from_rgb(20, 130, 50))); }
                else {
                    egui::Grid::new("interf").striped(true).show(ui, |ui| {
                        for h in ["Teil 1", "Teil 2", "Volumen (mm³)"] { ui.label(RichText::new(h).strong()); }
                        ui.end_row();
                        for (a1, b1, v) in &hits {
                            if ui.link(self.comp_label(*a1)).clicked() { select = Some((*a1, *b1)); }
                            if ui.link(self.comp_label(*b1)).clicked() { select = Some((*a1, *b1)); }
                            ui.label(RichText::new(fmt_num(*v, 3)).color(Color32::from_rgb(200, 36, 28)));
                            ui.end_row();
                        }
                    });
                }
                ui.add_space(4.0);
                if ui.button("Berechnen").clicked() { again = true; }
            });
            if let Some((x, y)) = select { self.tree_selection = vec![x, y]; self.selection = vec![Pick::Feature(x), Pick::Feature(y)]; }
            if again { self.run_interference(); }
            if !open { if let Some(a) = &mut self.asm { a.interference = None; } }
        }
    }

    /// File ▸ New dialog (Part / Assembly) and Model ▸ Create component dialog.
    pub fn new_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(mut d) = self.new_dialog.take() {
            let mut keep = true;
            let mut ok = false;
            let mut x_open = true;
            egui::Window::new("Neu").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -60.0)).show(ctx, |ui| {
            if super::creo::dialog_header(ui, "Neu") { x_open = false; }
                ui.set_width(400.0);
                let boxed = |ui: &mut egui::Ui, title: &str, w: f32, f: &mut dyn FnMut(&mut egui::Ui)| {
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        ui.label(title);
                        egui::Frame::NONE.stroke(egui::Stroke::new(1.0_f32, super::ribbon::theme::BORDER)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                            ui.set_min_size(vec2(w - 22.0, 250.0));
                            ui.spacing_mut().item_spacing.y = 8.0;
                            f(ui);
                        });
                    });
                };
                ui.horizontal_top(|ui| {
                    let cur = if d.drawing { 2 } else if d.assembly { 1 } else { 0 };
                    boxed(ui, "Typ", 175.0, &mut |ui| {
                        for (n, kind, en, icon) in [("Skizze", 9, false, Icon::Sketch), ("Teil", 0, true, Icon::Part), ("Baugruppe", 1, true, Icon::Assembly), ("Fertigung", 9, false, Icon::Mold), ("Zeichnung", 2, true, Icon::Drawing), ("Format", 9, false, Icon::TitleBlock), ("Notizbuch", 9, false, Icon::Note3d)] {
                            ui.horizontal(|ui| {
                                let r = ui.add_enabled(en, egui::RadioButton::new(en && cur == kind, ""));
                                let (ir, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
                                super::icons::paint(ui.painter(), ir, icon, en);
                                let l = ui.add_enabled(en, egui::Label::new(n).sense(egui::Sense::click()));
                                if (r.clicked() || l.clicked()) && cur != kind {
                                    d.assembly = kind == 1;
                                    d.drawing = kind == 2;
                                    d.name = match kind { 1 => "asm0001", 2 => "drw0001", _ => "prt0001" }.into();
                                }
                            });
                        }
                    });
                    ui.add_space(8.0);
                    boxed(ui, "Untertyp", 205.0, &mut |ui| {
                        let subs: &[&str] = if d.drawing { &[] } else if d.assembly { &["Konstruktion", "Austausch", "Prozessplan", "NC-Baugruppe"] } else { &["Volumenkörper", "Blech", "Massenelement", "Kabelbaum"] };
                        for (i, s) in subs.iter().enumerate() { let _ = ui.add_enabled(i == 0, egui::RadioButton::new(i == 0, *s)); }
                    });
                });
                ui.add_space(8.0);
                egui::Grid::new("new_names").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
                    ui.label("Dateiname:");
                    let r = ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(300.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { ok = true; }
                    ui.end_row();
                    ui.label("Üblicher Name:");
                    ui.add(egui::TextEdit::singleline(&mut d.common).desired_width(300.0));
                    ui.end_row();
                });
                ui.add_space(6.0);
                ui.checkbox(&mut d.template, "Standardschablone verwenden");
                ui.add_space(6.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new("  Abbrechen  ").min_size(vec2(100.0, 28.0))).clicked() { keep = false; }
                        if ui.add(egui::Button::new("OK").min_size(vec2(100.0, 28.0))).clicked() { ok = true; }
                    });
                });
            });
            if ok {
                keep = false;
                let name = d.name.trim().to_uppercase();
                if d.drawing {
                    if !name.is_empty() { self.windows.new_drawing = Some(super::drawui::NewDrawing { name, size: crate::model::drawing::SheetSize::A3, orient: crate::model::drawing::Orientation::Landscape, template: true, projection: crate::model::drawing::Projection::Third }); }
                } else if !name.is_empty() && self.confirm_discard() {
                    if d.assembly { self.new_assembly(&name); } else { self.new_part_named(&name); }
                }
            }
            if keep && x_open { self.new_dialog = Some(d); }
        }
        self.new_drawing_dialog(ctx);
        if let Some(mut name) = self.windows.create_comp.take() {
            let mut keep = true;
            let mut ok = false;
            let mut x_open = true;
            egui::Window::new("Komponente erzeugen").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -60.0)).show(ctx, |ui| {
            if super::creo::dialog_header(ui, "Komponente erzeugen") { x_open = false; }
                ui.set_width(300.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Typ").strong());
                        let _ = ui.radio(true, "Teil");
                        let _ = ui.add_enabled(false, egui::RadioButton::new(false, "Unterbaugruppe"));
                        let _ = ui.add_enabled(false, egui::RadioButton::new(false, "Skelettmodell"));
                    });
                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Untertyp").strong());
                        let _ = ui.radio(true, "Volumenkörper");
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(180.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { ok = true; }
                });
                ui.label(RichText::new("Erzeugungsmethode: Standardbezüge, mit der Bedingung Standard platziert; das neue Teil wird zum Bearbeiten geöffnet.").size(11.0).color(LABEL));
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("  Abbrechen  ").clicked() { keep = false; }
                    if ui.button("    OK    ").clicked() { ok = true; }
                });
            });
            if ok { keep = false; self.create_component(&name); }
            if keep && x_open { self.windows.create_comp = Some(name); }
        }
    }

    /// Asks before discarding unsaved changes; true when it is fine to continue.
    pub fn confirm_discard(&mut self) -> bool {
        let unsaved_part_from_asm = self.editing_from_asm().map(|o| o.dirty).unwrap_or(false);
        let unsaved_drawing = self.drw.as_ref().map(|d| d.dirty).unwrap_or(false);
        if !self.dirty && !unsaved_part_from_asm && !unsaved_drawing { return true; }
        let name = match &self.drw { Some(d) if d.dirty => format!("{} und dessen Modell", d.data.name), _ => self.asm.as_ref().map(|a| a.data.name.clone()).unwrap_or_else(|| self.data.name.clone()) };
        let r = rfd::MessageDialog::new().set_title("Nicht gespeicherte Änderungen").set_description(format!("Nicht gespeicherte Änderungen an {} verwerfen?", name)).set_buttons(rfd::MessageButtons::OkCancel).show();
        r == rfd::MessageDialogResult::Ok
    }
}

fn a_name(a: &Option<AsmSession>) -> String { a.as_ref().map(|a| format!("{}.ASM", a.data.name)).unwrap_or_default() }

trait OrDefaultPart { fn unwrap_or_default_part(self) -> ModelData; }
impl OrDefaultPart for Option<ModelData> { fn unwrap_or_default_part(self) -> ModelData { self.unwrap_or_else(|| ModelData::new_part("NONE")) } }

impl App {
    /// Mini toolbar for a selection in an assembly.
    pub fn asm_mini_toolbar(&mut self, ctx: &egui::Context, mb: MiniBar) {
        let Some(owner) = self.owner(mb.pick).filter(|o| self.component(*o).is_some()) else { return };
        enum A { Def, Open, Hide, Suppress, Delete, Repeat }
        let mut act = None;
        egui::Area::new(egui::Id::new("mini_toolbar")).fixed_pos(mb.pos + vec2(14.0, -46.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(2.0).corner_radius(3.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let mut b = |ui: &mut egui::Ui, icon: Icon, tip: &str, a: A| { if icon_button(ui, icon, tip, true, false, 18.0).clicked() { act = Some(a); } };
                    b(ui, Icon::EditDef, "Definition editieren", A::Def);
                    b(ui, Icon::Open, "Öffnen", A::Open);
                    b(ui, Icon::Assemble, "Weitere Instanz zusammenbauen", A::Repeat);
                    b(ui, Icon::Hide, "Ausblenden", A::Hide);
                    b(ui, Icon::Suppress, "Unterdrücken", A::Suppress);
                    b(ui, Icon::Delete, "Löschen", A::Delete);
                });
            });
        });
        let Some(a) = act else { return };
        self.mini = None;
        match a {
            A::Def => self.edit_placement(owner),
            A::Open => self.asm_open_part(owner),
            A::Repeat => if let Some(p) = self.component(owner).map(|c| c.part.clone()) { self.assemble_part(&p) },
            A::Hide => self.asm_set_flag(&[owner], None, Some(true)),
            A::Suppress => self.asm_set_flag(&[owner], Some(true), None),
            A::Delete => self.asm_delete(&[owner]),
        }
    }
}

impl App {
    /// Model Info for an assembly: its components with placement status.
    pub fn asm_info_ui(&mut self, ui: &mut egui::Ui) {
        let Some(a) = &self.asm else { return };
        let r = &a.res;
        ui.label(format!("Baugruppe {} — {} Komponenten, {} Teile, {} Polygone. Regeneriert in {:.0} ms.", a.data.name, a.data.components.len(), a.data.parts.len(), r.display.topo.polys.len(), r.display.time * 1000.0));
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            egui::Grid::new("asm_info").striped(true).show(ui, |ui| {
                for h in ["#", "Komponente", "Bedingungen", "Status", "Position (mm)"] { ui.label(RichText::new(h).strong()); }
                ui.end_row();
                for (i, c) in a.data.components.iter().enumerate() {
                    ui.label(format!("{}", i + 1));
                    ui.label(format!("{}.PRT", c.part));
                    ui.label(format!("{}", c.constraints.len()));
                    let st = if c.suppressed { RichText::new("Unterdrückt").color(Color32::GRAY) } else {
                        let s = r.status.get(&c.id).cloned().unwrap_or(PlaceStatus::Packaged);
                        RichText::new(s.text()).color(status_color(&s))
                    };
                    ui.label(st);
                    let p = r.placements.get(&c.id).map(|p| p.pos).unwrap_or(c.placement.pos);
                    ui.label(format!("{}, {}, {}", fmt_num(p.x, 2), fmt_num(p.y, 2), fmt_num(p.z, 2)));
                    ui.end_row();
                }
            });
        });
    }
}
