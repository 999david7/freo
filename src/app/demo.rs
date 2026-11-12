//! Scripted demo scenes and automatic screenshots (FREO_DEMO=<scene> FREO_SHOT=<file.png>).
//! Used for documentation images and visual regression checks.

use super::sketcher::{SkSel, Tool};
use super::*;
use crate::sketch::sketch::{SkCType, SkConstraint, SkKind};
use eframe::egui;
use std::sync::atomic::{AtomicU32, Ordering};

static FRAME: AtomicU32 = AtomicU32::new(0);

fn feat(id: u64, name: &str, kind: FeatureKind) -> Feature { Feature { id, name: name.into(), suppressed: false, hidden: false, kind }
}

pub fn bracket() -> ModelData {
    let mut m = ModelData::new_part("BRACKET");
    m.parameters.push(Parameter { name: "W".into(), expr: "120".into(), note: "Plattenbreite".into() });
    m.parameters.push(Parameter { name: "T".into(), expr: "20".into(), note: "Plattendicke".into() });
    let mut sk = Sketch::rect(120.0, 80.0, 0.0, 0.0);
    let l = sk.entities[0].id;
    let (a, b) = (sk.entities[0].pts[0], sk.entities[0].pts[1]);
    let mut d = SkConstraint::new(SkCType::Distance, vec![a, b], vec![l]);
    d.value = 120.0; d.expr = Some("W".into()); d.label = V2::new(0.0, -12.0);
    sk.add_constraint(d);
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(TOP_ID)); e.sketch = sk; e.depth = Dim("T".into());
    m.features.push(feat(10, "Profil 1", FeatureKind::Extrude(e)));
    let FeatureKind::Extrude(mut boss) = FeatureKind::new_extrude() else { unreachable!() };
    boss.plane = Some(PlaneRef::Face(FaceRef { point: V3::new(0.0, 20.0, 0.0), normal: V3::Y }));
    boss.sketch = Sketch::circle(18.0, 0.0, 0.0); boss.depth = Dim::v(30.0);
    m.features.push(feat(11, "Profil 2", FeatureKind::Extrude(boss)));
    let FeatureKind::Hole(mut h) = FeatureKind::new_hole() else { unreachable!() };
    h.plane = Some(PlaneRef::Face(FaceRef { point: V3::new(-40.0, 20.0, 20.0), normal: V3::Y }));
    h.x = Dim::v(-45.0); h.y = Dim::v(25.0); h.ty = HoleType::Counterbore; h.through_all = true; h.diameter = Dim::v(9.0); h.cb_diameter = Dim::v(15.0);
    m.features.push(feat(12, "Bohrung 1", FeatureKind::Hole(h)));
    let FeatureKind::Pattern(mut p) = FeatureKind::new_pattern() else { unreachable!() };
    p.features = vec![12]; p.count1 = Dim::v(2.0); p.spacing1 = Dim::v(90.0); p.use_dir2 = true; p.count2 = Dim::v(2.0); p.spacing2 = Dim::v(50.0);
    m.features.push(feat(13, "Muster 1", FeatureKind::Pattern(p)));
    let FeatureKind::Hole(mut h2) = FeatureKind::new_hole() else { unreachable!() };
    h2.plane = Some(PlaneRef::Face(FaceRef { point: V3::new(0.0, 50.0, 0.0), normal: V3::Y })); h2.diameter = Dim::v(16.0); h2.through_all = true;
    m.features.push(feat(14, "Bohrung 2", FeatureKind::Hole(h2)));
    m.next_id = 20;
    m
}

fn extrude_on(plane: u64, sk: Sketch, depth: f64, remove: bool, symmetric: bool, flip: bool) -> FeatureKind {
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(plane));
    e.sketch = sk;
    e.depth = Dim::v(depth);
    e.remove = remove;
    e.flip = flip;
    if symmetric { e.depth_type = DepthType::Symmetric; }
    FeatureKind::Extrude(e)
}

fn tint(m: &mut ModelData, c: [f32; 3]) { m.appearance.color = c; }

/// Sample assembly: base plate, pillow block, shaft and two bolts, fully constrained.
pub fn sample_assembly() -> crate::model::assembly::AssemblyData {
    use crate::model::assembly::*;
    let mut base = ModelData::new_part("BASE_PLATE");
    base.features.push(feat(10, "Profil 1", extrude_on(TOP_ID, Sketch::rect(200.0, 100.0, 0.0, 0.0), 15.0, false, false, false)));
    let mut holes = Sketch::default();
    for x in [-75.0, 75.0] { let c = holes.add_point(V2::new(x, 0.0)); holes.add_circle(c, 5.25); }
    base.features.push(feat(11, "Profil 2", extrude_on(TOP_ID, holes, 100.0, true, true, false)));
    base.next_id = 20;
    tint(&mut base, [0.62, 0.66, 0.72]);
    let mut block = ModelData::new_part("PILLOW_BLOCK");
    block.features.push(feat(10, "Profil 1", extrude_on(TOP_ID, Sketch::rect(80.0, 50.0, 0.0, 0.0), 60.0, false, false, false)));
    block.features.push(feat(11, "Profil 2", extrude_on(FRONT_ID, Sketch::circle(15.25, 0.0, 35.0), 100.0, true, true, false)));
    block.next_id = 20;
    tint(&mut block, [0.92, 0.70, 0.36]);
    let mut shaft = ModelData::new_part("SHAFT");
    shaft.features.push(feat(10, "Profil 1", extrude_on(TOP_ID, Sketch::circle(15.0, 0.0, 0.0), 160.0, false, false, false)));
    shaft.next_id = 20;
    tint(&mut shaft, [0.55, 0.69, 0.88]);
    let mut bolt = ModelData::new_part("BOLT_M10");
    bolt.features.push(feat(10, "Profil 1", extrude_on(TOP_ID, Sketch::circle(9.0, 0.0, 0.0), 8.0, false, false, false)));
    bolt.features.push(feat(11, "Profil 2", extrude_on(TOP_ID, Sketch::circle(5.0, 0.0, 0.0), 25.0, false, false, true)));
    bolt.next_id = 20;
    tint(&mut bolt, [0.48, 0.74, 0.55]);
    let rb = regen::run(&base);
    let rk = regen::run(&block);
    let rs = regen::run(&shaft);
    let rt = regen::run(&bolt);
    let face = |r: &RegenResult, n: V3, y: f64| { let t = &r.topo; t.face_ref((0..t.faces.len()).find(|&f| t.faces[f].planar && t.faces[f].normal.dot(n) > 0.999 && (t.faces[f].center.y - y).abs() < 1e-6).unwrap()) };
    let cyl = |r: &RegenResult, sx: f64| { let t = &r.topo; t.surface_ref((0..t.faces.len()).find(|&f| !t.faces[f].planar && t.faces[f].center.x * sx >= -1e-6).unwrap()) };
    let surf = |comp: u64, face: FaceRef, planar: bool| Some(AsmRef::Surface { comp, face, planar });
    let con = |ty, a, b, offset| AsmConstraint { ty, a, b, offset, sign: 0.0, enabled: true };
    let mut asm = AssemblyData::new("ASM0001");
    for p in [base.clone(), block.clone(), shaft.clone(), bolt.clone()] { asm.add_part(p, None); }
    let mut c = Component::new(100, "BASE_PLATE", Placement::default());
    c.constraints.push(AsmConstraint::new(AsmCType::Default));
    asm.components.push(c);
    let mut c = Component::new(101, "PILLOW_BLOCK", Placement::translation(V3::new(0.0, 40.0, 0.0)));
    c.constraints = vec![
        con(AsmCType::Coincident, surf(101, face(&rk, -V3::Y, 0.0), true), surf(100, face(&rb, V3::Y, 15.0), true), 0.0),
        con(AsmCType::Coincident, Some(AsmRef::Plane { comp: 101, id: FRONT_ID }), Some(AsmRef::AsmPlane(FRONT_ID)), 0.0),
        con(AsmCType::Coincident, Some(AsmRef::Plane { comp: 101, id: RIGHT_ID }), Some(AsmRef::AsmPlane(RIGHT_ID)), 0.0),
    ];
    asm.components.push(c);
    let mut c = Component::new(102, "SHAFT", Placement { rot: glam::DQuat::from_rotation_x(1.2), pos: V3::new(0.0, 60.0, -60.0) });
    c.constraints = vec![
        con(AsmCType::Coincident, surf(102, cyl(&rs, 1.0), false), surf(101, cyl(&rk, 1.0), false), 0.0),
        con(AsmCType::Distance, Some(AsmRef::Plane { comp: 102, id: TOP_ID }), Some(AsmRef::AsmPlane(FRONT_ID)), -80.0),
    ];
    asm.components.push(c);
    for (id, sx) in [(103u64, -1.0), (104, 1.0)] {
        let mut c = Component::new(id, "BOLT_M10", Placement::translation(V3::new(75.0 * sx, 30.0, 0.0)));
        c.constraints = vec![
            con(AsmCType::Coincident, surf(id, cyl(&rt, 1.0), false), surf(100, cyl(&rb, sx), false), 0.0),
            con(AsmCType::Coincident, surf(id, face(&rt, -V3::Y, 0.0), true), surf(100, face(&rb, V3::Y, 15.0), true), 0.0),
        ];
        asm.components.push(c);
    }
    asm.next_id = 105;
    asm
}

impl App {
    fn add_vertical_rounds(&mut self, id: u64) {
        let r = regen::run(&self.data);
        let FeatureKind::Round(mut rd) = FeatureKind::new_round() else { return };
        rd.sets[0].radius = Dim::v(10.0);
        for (i, c) in r.topo.chains.iter().enumerate() {
            if c.is_straight() && (c.points[0].y - c.points.last().unwrap().y).abs() > 1.0 && c.points.iter().all(|q| q.x.abs() > 59.0) { rd.sets[0].edges.push(r.topo.edge_ref(i)); }
        }
        self.data.features.push(feat(id, "Rundung 1", FeatureKind::Round(rd)));
    }

    pub fn demo_setup(&mut self, scene: &str) {
        // FREO_DEMO=tab:<name> shows a ribbon tab on the bracket, tab0:<name> on an empty part
        let tab_of = |t: &str| match t {
            "model" => Some(Tab::Model), "analysis" => Some(Tab::Analysis), "livesim" => Some(Tab::LiveSim), "annot" => Some(Tab::Annot3d),
            "tools" => Some(Tab::Tools), "view" => Some(Tab::View), "flex" => Some(Tab::Flex), "apps" => Some(Tab::Apps), "htl" => Some(Tab::Htl), _ => None,
        };
        if let Some(which) = scene.strip_prefix("sim:") {
            self.data = bracket(); self.add_vertical_rounds(15); self.regenerate(false);
            self.demo_sim(which);
            self.fit_pending = true;
            return;
        }
        if let Some((pre, t)) = scene.split_once(':') {
            if let Some(tab) = tab_of(t) {
                if pre == "tab" { self.data = bracket(); self.add_vertical_rounds(15); self.regenerate(false); } else { self.new_part_named("PRT0001"); }
                self.tab = tab;
                self.fit_pending = true;
                return;
            }
        }
        match scene {
            "home" => { self.go_home(); }
            "findmodels" => {
                self.go_home();
                let d = std::env::temp_dir().join("freo_demo_find");
                for f in ["SD-Karte/HTL/bad-organizer-1.prt", "SD-Karte/HTL/raspberry-pi-holder.prt.3", "Downloads/tray0001.asm", "Downloads/halter.stp", "freo-work/flansch.pfprt"] {
                    let p = d.join(f); let _ = std::fs::create_dir_all(p.parent().unwrap()); let _ = std::fs::write(&p, "#UGC:2 PART\n");
                }
                self.open_in(None);
                if let Some(o) = &mut self.creo.open {
                    let files = ["SD-Karte/HTL/bad-organizer-1.prt", "SD-Karte/HTL/raspberry-pi-holder.prt.3", "Downloads/tray0001.asm", "Downloads/halter.stp", "freo-work/flansch.pfprt"].iter().map(|f| d.join(f)).collect();
                    o.found = Some(("Creo-, STEP- und Freo-Dateien auf diesem Mac".into(), files));
                    o.selected = Some(d.join("SD-Karte/HTL/bad-organizer-1.prt"));
                    o.name = "bad-organizer-1.prt".into();
                }
            }
            "workdir" | "saveas" => {
                let d = std::env::temp_dir().join("freo_demo_work");
                for sub in ["Projekt A", "Projekt B", "Übungen", "Projekt A/Halterung"] { let _ = std::fs::create_dir_all(d.join(sub)); }
                let _ = std::fs::write(d.join("Projekt A/flansch.pfprt"), "{}");
                self.work_dir = d.join("Projekt A");
                if scene == "workdir" { self.go_home(); self.pick_work_dir(); if let Some(b) = &mut self.creo.browser { b.selected = Some(d.join("Projekt A/Halterung")); } }
                else { self.data = bracket(); self.regenerate(false); self.save(true); }
            }
            "fsmenu" | "newfolder" => {
                self.go_home();
                let d = std::env::temp_dir().join("freo_demo_work");
                for sub in ["Projekt A", "Projekt B", "Übungen"] { let _ = std::fs::create_dir_all(d.join(sub)); }
                self.work_dir = d.clone();
                self.browser_dir = d.clone();
                self.open_in(None);
                if scene == "newfolder" { self.creo.fs_action = Some(super::creo::FsAction::NewFolder(d.join("Projekt A"))); }
                if scene == "fsmenu" { self.creo.fs.clip = Some((d.join("Übungen"), false)); self.creo.fs.trashed = Some((d.join("Alter Entwurf"), d.join(".trash"))); }
            }
            "homefile" => { self.go_home(); self.file_menu = true; }
            "export" => { self.data = bracket(); self.add_vertical_rounds(15); self.regenerate(false); self.export_dialog(None); }
            "filemenu" => { self.data = bracket(); self.add_vertical_rounds(15); self.regenerate(false); self.file_menu = true; }
            "opendlg" => { self.go_home(); self.open_in(None); }
            "newpart" => { self.go_home(); self.confirm_new(); }
            "bracket" | "section" | "tree" | "reflect" => {
                self.data = bracket();
                self.add_vertical_rounds(15);
                self.regenerate(false);
                if scene == "reflect" { self.style = super::render::Style::Reflect; }
                if scene == "section" { self.section = Some(SectionView { plane: FRONT_ID, offset: 0.0, flip: false }); self.tab = Tab::View; }
                if scene == "tree" { self.tree_click(13, false); }
            }
            "round" => {
                self.data = bracket();
                self.regenerate(false);
                self.start_feature(FeatureKind::new_round());
                let r = self.displayed().clone();
                for (i, c) in r.topo.chains.iter().enumerate() {
                    if c.is_straight() && (c.points[0].y - c.points.last().unwrap().y).abs() > 1.0 && c.points.iter().all(|q| q.x.abs() > 59.0) {
                        self.route_pick(Pick::Edge(i), PickTarget::Edges);
                    }
                }
                self.set_pick_target(None);
                self.update_editor(|f| if let FeatureKind::Round(d) = &mut f.kind { d.sets[0].radius = Dim::v(10.0) });
            }
            "sketch" => {
                self.data = bracket();
                self.regenerate(false);
                self.selection = vec![Pick::Plane(FRONT_ID)];
                self.start_feature(FeatureKind::new_revolve());
                if let Some(s) = &mut self.sketch {
                    let sk = &mut s.sketch;
                    let pts = [(10.0, 0.0), (60.0, 0.0), (60.0, 12.0), (25.0, 12.0), (25.0, 45.0), (10.0, 45.0)];
                    let ids: Vec<i64> = pts.iter().map(|&(x, y)| sk.add_point(V2::new(x, y))).collect();
                    let mut lines = vec![];
                    for i in 0..ids.len() { lines.push(sk.add_line(ids[i], ids[(i + 1) % ids.len()])); }
                    for (i, l) in lines.iter().enumerate() { sk.add_constraint(SkConstraint::new(if i % 2 == 0 { SkCType::Horizontal } else { SkCType::Vertical }, vec![], vec![*l])); }
                    let (a, b) = (sk.add_point(V2::new(0.0, -10.0)), sk.add_point(V2::new(0.0, 60.0)));
                    sk.add_entity(SkKind::Line, vec![a, b], 0.0, true, true);
                    for (l, lab) in [(lines[0], V2::new(0.0, -10.0)), (lines[1], V2::new(10.0, 0.0)), (lines[4], V2::new(-10.0, 0.0))] {
                        let e = sk.entity(l).unwrap().clone();
                        let mut d = SkConstraint::new(SkCType::Distance, e.pts.clone(), vec![l]);
                        d.value = sk.measure(&d); d.label = lab;
                        sk.add_constraint(d);
                    }
                    s.tool = Tool::Dimension;
                    s.selection = vec![SkSel::Ent(lines[2])];
                }
            }
            "flange" => {
                self.data = ModelData::new_part("FLANGE");
                let mut s = Sketch::default();
                let pts = [(10.0, 0.0), (60.0, 0.0), (60.0, 12.0), (25.0, 12.0), (25.0, 45.0), (10.0, 45.0)];
                let ids: Vec<i64> = pts.iter().map(|&(x, y)| s.add_point(V2::new(x, y))).collect();
                for i in 0..ids.len() { s.add_line(ids[i], ids[(i + 1) % ids.len()]); }
                let (a, b) = (s.add_point(V2::new(0.0, -10.0)), s.add_point(V2::new(0.0, 60.0)));
                s.add_entity(SkKind::Line, vec![a, b], 0.0, false, true);
                let FeatureKind::Revolve(mut rv) = FeatureKind::new_revolve() else { unreachable!() };
                rv.plane = Some(PlaneRef::Datum(FRONT_ID)); rv.sketch = s;
                self.data.features.push(feat(10, "Drehen 1", FeatureKind::Revolve(rv)));
                let FeatureKind::Hole(mut h) = FeatureKind::new_hole() else { unreachable!() };
                h.plane = Some(PlaneRef::Face(FaceRef { point: V3::new(45.0, 12.0, 0.0), normal: V3::Y })); h.x = Dim::v(45.0); h.diameter = Dim::v(8.0); h.through_all = true;
                self.data.features.push(feat(11, "Bohrung 1", FeatureKind::Hole(h)));
                let FeatureKind::Pattern(mut p) = FeatureKind::new_pattern() else { unreachable!() };
                p.features = vec![11]; p.kind = PatternKind::Axis; p.axis = AxisRef::Axis(10_001); p.axis_count = Dim::v(8.0); p.axis_angle = Dim::v(45.0);
                self.data.features.push(feat(12, "Muster 1", FeatureKind::Pattern(p)));
                let FeatureKind::Chamfer(mut ch) = FeatureKind::new_chamfer() else { unreachable!() };
                let r = regen::run(&self.data);
                for (i, c) in r.topo.chains.iter().enumerate() { if c.closed && c.points.iter().all(|q| (q.y - 45.0).abs() < 1e-6) && c.points[0].x.hypot(c.points[0].z) > 20.0 { ch.edges.push(r.topo.edge_ref(i)); } }
                ch.d1 = Dim::v(3.0);
                self.data.features.push(feat(13, "Fase 1", FeatureKind::Chamfer(ch)));
                self.data.next_id = 20;
                self.regenerate(false);
                self.windows.mass = true;
            }
            "extrude" => {
                self.data = bracket();
                self.regenerate(false);
                let top = (0..self.result.topo.faces.len()).find(|&f| { let fc = &self.result.topo.faces[f]; fc.planar && fc.normal.y > 0.99 && (fc.center.y - 20.0).abs() < 1e-6 });
                if let Some(f) = top { self.selection = vec![Pick::Face(f)]; }
                self.start_feature(FeatureKind::new_extrude());
                if let Some(s) = self.sketch.take() {
                    let mut sk = s.sketch.clone();
                    let c = sk.add_point(V2::new(45.0, -25.0));
                    sk.add_circle(c, 8.0);
                    self.finish_sketch(sk, s.slot);
                }
                self.update_editor(|f| if let FeatureKind::Extrude(d) = &mut f.kind { d.depth = Dim::v(12.0); });
                self.dash_panel = Some("Optionen");
                self.dash_panel_anchor = egui::pos2(8.0, 166.0);
            }
            // Profil on an empty part: dashboard waiting for a sketch plane, its Placement panel,
            // the Sketch dialog (Definieren...), and the sketcher on OBEN
            "profil" | "profilpanel" | "profildlg" | "profildlgplane" | "profilsketch" | "sketchtools" => {
                self.new_part_named("PRT0001");
                self.start_feature(FeatureKind::new_extrude());
                match scene {
                    "profilpanel" => { self.dash_panel = Some("Platzierung"); self.dash_panel_anchor = egui::pos2(355.0, 166.0); }
                    "profildlg" => self.open_sketch_dialog(),
                    "profildlgplane" => { self.open_sketch_dialog(); self.route_pick(Pick::Plane(TOP_ID), PickTarget::SketchPlane); }
                    "profilsketch" => self.route_pick(Pick::Plane(TOP_ID), PickTarget::SketchPlane),
                    "sketchtools" => {
                        self.route_pick(Pick::Plane(TOP_ID), PickTarget::SketchPlane);
                        if let Some(s) = &mut self.sketch {
                            s.set_tool(Tool::Rect);
                            s.click_at(V2::new(-60.0, -40.0)); s.click_at(V2::new(60.0, 40.0));
                            s.set_tool(Tool::FilletEllipTrim);
                            s.click_at(V2::new(60.0, 40.0));
                            s.set_tool(Tool::Conic);
                            for p in [V2::new(-50.0, -30.0), V2::new(-10.0, -30.0), V2::new(-30.0, -15.0)] { s.click_at(p); }
                            s.set_tool(Tool::Csys);
                            s.click_at(V2::new(35.0, -25.0));
                            let (ia, ib) = (s.sketch.add_point(V2::new(-45.0, 5.0)), s.sketch.add_point(V2::new(-45.0, 20.0)));
                            let id = s.sketch.add_entity(SkKind::Text, vec![ia, ib], 0.0, false, false);
                            if let Some(e) = s.sketch.entities.iter_mut().find(|e| e.id == id) { e.text = Some(crate::sketch::sketch::SkText { text: "HTL".into(), font: 0, aspect: 1.0, slant: 0.0, family: String::new() }); }
                            s.set_tool(Tool::Select);
                            s.refs_open = true;
                        }
                    }
                    _ => {}
                }
            }
            "file" => {
                self.data = bracket();
                self.add_vertical_rounds(15);
                self.regenerate(false);
                self.recent = vec!["/Users/demo/Documents/bracket.pfprt".into(), "/Users/demo/Documents/flange.pfprt".into(), "/Users/demo/Documents/housing_v2.pfprt".into()];
                self.file_menu = true;
            }
            "editdims" => {
                self.data = bracket();
                self.add_vertical_rounds(15);
                self.regenerate(false);
                self.start_edit_dims(10);
            }
            "dialog" => {
                self.data = bracket();
                self.regenerate(false);
                self.selection = vec![Pick::Plane(TOP_ID)];
                self.start_feature(FeatureKind::new_plane());
                self.update_editor(|f| if let FeatureKind::DatumPlane(d) = &mut f.kind { d.offset = Dim::v(60.0); });
            }
            "browser" => {
                self.data = bracket();
                self.regenerate(false);
                self.nav_tab = NavTab::Browser;
                if let Ok(d) = std::env::current_dir() { self.browser_dir = d.join("docs"); }
            }
            "mini" => {
                self.data = bracket();
                self.add_vertical_rounds(15);
                self.regenerate(false);
                let top = (0..self.result.topo.faces.len()).find(|&f| { let fc = &self.result.topo.faces[f]; fc.planar && fc.normal.y > 0.99 && (fc.center.y - 20.0).abs() < 1e-6 });
                if let Some(f) = top { self.handle_pick(Some(Pick::Face(f)), false); self.mini = Some(MiniBar { pos: egui::pos2(900.0, 520.0), pick: Pick::Face(f) }); }
            }
            // FREO_DEMO=mini_edge: mini toolbar for a top edge of the bracket (FREO_DEMO_POPUP=mini_related / mini_more / suppress)
            "mini_edge" => {
                self.data = bracket();
                self.add_vertical_rounds(15);
                self.regenerate(false);
                let t = &self.result.topo;
                let e = (0..t.chains.len()).filter(|&i| { let c = &t.chains[i]; c.is_straight() && c.points.iter().all(|q| (q.y - 20.0).abs() < 1e-6) })
                    .max_by(|a, b| t.chains[*a].points[0].z.partial_cmp(&t.chains[*b].points[0].z).unwrap());
                if let Some(e) = e {
                    self.handle_pick(Some(Pick::Edge(e)), false);
                    self.mini = Some(MiniBar { pos: egui::pos2(760.0, 560.0), pick: Pick::Edge(e) });
                    match std::env::var("FREO_DEMO_POPUP").as_deref() {
                        Ok("mini_related") => self.creo.mini_ui = super::minibar::MiniUi { related: true, pick: Some(Pick::Edge(e)), ..Default::default() },
                        Ok("mini_more") => self.creo.mini_ui = super::minibar::MiniUi { more: true, pick: Some(Pick::Edge(e)), ..Default::default() },
                        Ok("suppress") => if let Some(o) = self.owner(Pick::Edge(e)) { self.ask_suppress(&[o]); },
                        _ => {}
                    }
                }
            }
            "roundsets" => {
                self.data = bracket();
                self.regenerate(false);
                self.start_feature(FeatureKind::new_round());
                let r = self.displayed().clone();
                let vertical: Vec<(usize, f64)> = r.topo.chains.iter().enumerate()
                    .filter(|(_, c)| c.is_straight() && (c.points[0].y - c.points.last().unwrap().y).abs() > 1.0 && c.points.iter().all(|q| q.x.abs() > 59.0))
                    .map(|(i, c)| (i, c.points[0].x)).collect();
                for (i, x) in &vertical { if *x > 0.0 { self.route_pick(Pick::Edge(*i), PickTarget::Edges); } }
                self.update_editor(|f| if let FeatureKind::Round(d) = &mut f.kind { d.sets[0].radius = Dim::v(12.0); d.sets.push(RoundSet::new(5.0)); });
                self.round_set = 1;
                for (i, x) in &vertical { if *x < 0.0 { self.route_pick(Pick::Edge(*i), PickTarget::Edges); } }
                self.update_editor(|f| if let FeatureKind::Round(d) = &mut f.kind { d.sets[1].variable = true; d.sets[1].radius_end = Dim::v(14.0); });
                self.set_pick_target(None);
                self.dash_panel = Some("Sätze");
                self.dash_panel_anchor = egui::pos2(8.0, 166.0);
            }
            "stdhole" => {
                self.data = bracket();
                self.regenerate(false);
                let top = (0..self.result.topo.faces.len()).find(|&f| { let fc = &self.result.topo.faces[f]; fc.planar && fc.normal.y > 0.99 && (fc.center.y - 20.0).abs() < 1e-6 });
                if let Some(f) = top { self.selection = vec![Pick::Face(f)]; }
                self.start_feature(FeatureKind::new_hole());
                self.update_editor(|f| if let FeatureKind::Hole(d) = &mut f.kind { d.ty = HoleType::Standard; d.size = "M10".into(); d.tapped = true; d.through_all = true; d.x = Dim::v(0.0); d.y = Dim::v(30.0); });
                self.dash_panel = Some("Form");
                self.dash_panel_anchor = egui::pos2(8.0, 166.0);
            }
            "fillpattern" => {
                let mut m = ModelData::new_part("PERFORATED");
                m.features.push(feat(10, "Profil 1", extrude_on(TOP_ID, Sketch::rect(160.0, 110.0, 0.0, 0.0), 6.0, false, false, false)));
                m.features.push(feat(11, "Profil 2", extrude_on(TOP_ID, Sketch::circle(4.0, 0.0, 0.0), 40.0, true, true, false)));
                m.next_id = 20;
                self.data = m;
                self.regenerate(false);
                self.tree_selection = vec![11];
                self.start_feature(FeatureKind::new_pattern());
                self.update_editor(|f| if let FeatureKind::Pattern(d) = &mut f.kind {
                    d.kind = PatternKind::Fill; d.sketch_plane = Some(PlaneRef::Datum(TOP_ID)); d.fill_grid = FillGrid::Triangle;
                    d.fill_spacing = Dim::v(14.0); d.fill_margin = Dim::v(6.0);
                    let mut sk = Sketch::default();
                    let pts = [(-70.0, -45.0), (70.0, -45.0), (70.0, 10.0), (20.0, 45.0), (-70.0, 45.0)];
                    let ids: Vec<i64> = pts.iter().map(|&(x, y)| sk.add_point(V2::new(x, y))).collect();
                    for i in 0..ids.len() { sk.add_line(ids[i], ids[(i + 1) % ids.len()]); }
                    d.sketch = sk;
                });
                self.set_pick_target(None);
            }
            "tonext" => {
                self.data = bracket();
                self.regenerate(false);
                let FeatureKind::DatumPlane(mut dp) = FeatureKind::new_plane() else { unreachable!() };
                dp.reference = Some(PlaneRef::Datum(TOP_ID)); dp.offset = Dim::v(70.0);
                self.data.features.push(feat(16, "DTM1", FeatureKind::DatumPlane(dp)));
                self.data.next_id = 20;
                self.regenerate(false);
                self.selection = vec![Pick::Plane(16)];
                self.start_feature(FeatureKind::new_extrude());
                if let Some(s) = self.sketch.take() {
                    let mut sk = s.sketch.clone();
                    for x in [-45.0, 45.0] { let c = sk.add_point(V2::new(x, -25.0)); sk.add_circle(c, 6.0); }
                    self.finish_sketch(sk, s.slot);
                }
                self.update_editor(|f| if let FeatureKind::Extrude(d) = &mut f.kind { d.depth_type = DepthType::ToNext; d.flip = true; });
            }
            "drawing" | "drawsection" | "drawpdf" | "drawdialog" | "drawannotate" | "drawgdt" | "drawgdtdim" | "drawgdtframe" => {
                self.data = bracket();
                self.add_vertical_rounds(15);
                self.data.next_id = 20;
                self.regenerate(false);
                let nd = super::drawui::NewDrawing { name: "BRACKET".into(), size: crate::model::drawing::SheetSize::A3, orient: crate::model::drawing::Orientation::Landscape, template: true, projection: crate::model::drawing::Projection::Third };
                self.new_drawing(&nd);
                if let Some(d) = &mut self.drw { d.data.drawn_by = "D. WINKLER".into(); d.data.material = "STAHL".into(); }
                let views: Vec<u32> = self.drw.as_ref().map(|d| d.data.sheets[0].views.iter().map(|v| v.id).collect()).unwrap_or_default();
                if scene == "drawsection" {
                    // section the right view through RIGHT and add a detail of a corner of the front view
                    if let Some(d) = &mut self.drw {
                        let letter = d.data.next_letter();
                        if let Some(v) = d.data.sheets[0].views.iter_mut().find(|v| v.side == Some(crate::model::drawing::ProjSide::Right)) { v.section = Some(RIGHT_ID); v.letter = letter; }
                    }
                }
                self.show_model_dims(&views);
                if scene == "drawsection" {
                    if let Some(d) = &mut self.drw {
                        let front = d.data.sheets[0].views[0].clone();
                        let id = d.data.take_id();
                        let letter = d.data.next_letter();
                        let mut v = crate::model::drawing::DrawView::new(id, crate::model::drawing::ViewKind::Detail, front.rot, V2::new(345.0, 205.0));
                        v.parent = Some(front.id);
                        v.detail = Some(crate::model::drawing::Detail { center: V2::new(-45.0, 14.0), radius: 13.0 });
                        v.scale = Some(d.data.sheets[0].scale * 2.0);
                        v.letter = letter;
                        d.data.sheets[0].views.push(v);
                    }
                }
                if let Some(d) = &mut self.drw {
                    let id = d.data.take_id();
                    d.data.sheets[0].notes.push(crate::model::drawing::Note { id, pos: V2::new(24.0, 60.0), text: "HINWEISE:\n1. ALLE SCHARFEN KANTEN BRECHEN 0.5 x 45°\n2. NICHT TOLERIERTE MASSE ±0.1".into(), leader: None });
                    d.sel.clear();
                }
                if scene == "drawdialog" {
                    let right = self.drw.as_ref().and_then(|d| d.data.sheets[0].views.get(2).map(|v| v.id));
                    if let Some(id) = right {
                        if let Some(d) = &mut self.drw { d.sel = vec![super::drawing::DSel::View(id)]; }
                        self.open_view_dialog_pub(id);
                        if let Some(vd) = self.drw.as_mut().and_then(|d| d.view_dialog.as_mut()) { vd.cat = 2; }
                    }
                }
                if scene.starts_with("drawgdt") { self.demo_gdt(); }
                if scene == "drawgdtdim" {
                    let i = self.drw.as_ref().and_then(|d| d.data.sheets[0].shown.iter().position(|s| s.key.starts_with("14:") && s.key.ends_with(":f:diameter")));
                    if let (Some(d), Some(i)) = (&mut self.drw, i) { d.sel = vec![super::drawing::DSel::Shown(i)]; }
                }
                if scene == "drawgdtframe" {
                    let id = self.drw.as_ref().and_then(|d| d.data.sheets[0].gtols.iter().find(|g| matches!(g.target, crate::model::drawing::Attach::Dim(_))).map(|g| g.id));
                    if let (Some(d), Some(id)) = (&mut self.drw, id) { d.sel = vec![super::drawing::DSel::Gtol(id)]; }
                }
                if scene == "drawannotate" {
                    // created dimensions in the top view: hole centre to hole centre, and a diameter
                    let top = self.drw.as_ref().map(|d| d.data.sheets[0].views[1].clone());
                    if let Some(top) = top {
                        if let Some(g) = self.view_geom(top.id) {
                            let (pa, pb) = (g.world(top.pos, V3::new(0.0, 50.0, 0.0)), g.world(top.pos, V3::new(45.0, 20.0, 25.0)));
                            if let (Some(a), Some(b)) = (self.snap_at(pa, 1.0), self.snap_at(pb, 1.0)) { self.place_dimension(a, Some(b), V2::new((pa.x + pb.x) * 0.5, pb.y - 14.0)); }
                            let on = g.world(top.pos, V3::new(-45.0 + 7.5, 20.0, -25.0));
                            if let Some(c) = self.snap_at(on, 1.0) { self.place_dimension(c, None, on + V2::new(-30.0, 18.0)); }
                        }
                    }
                    let id = self.drw.as_ref().and_then(|d| d.data.sheets[0].dims.last().map(|x| x.id));
                    if let (Some(d), Some(id)) = (&mut self.drw, id) { d.sel = vec![super::drawing::DSel::Dim(id)]; }
                    self.tab = Tab::Annotate;
                }
            }
            "asmdrawing" => {
                let data = sample_assembly();
                self.data = data.skeleton.clone();
                self.asm = Some(super::assembly::AsmSession::new(data));
                self.asm_regenerate(false);
                let nd = super::drawui::NewDrawing { name: "ASM0001".into(), size: crate::model::drawing::SheetSize::A3, orient: crate::model::drawing::Orientation::Landscape, template: true, projection: crate::model::drawing::Projection::Third };
                self.new_drawing(&nd);
                let iso = self.drw.as_ref().and_then(|d| d.data.sheets[0].views.iter().find(|v| v.orient == Some(crate::model::drawing::Orient::Default)).map(|v| v.id));
                if let (Some(d), Some(iso)) = (&mut self.drw, iso) {
                    if let Some(v) = d.data.sheets[0].view_mut(iso) { v.exploded = true; }
                }
                self.insert_bom();
                if let Some(iso) = iso { self.create_balloons(iso); }
                if let Some(d) = &mut self.drw { d.data.drawn_by = "D. WINKLER".into(); d.sel.clear(); }
            }
            "newdialog" => {
                self.data = bracket();
                self.regenerate(false);
                self.new_dialog = Some(super::assembly::NewDialog { assembly: true, drawing: false, name: "asm0001".into(), common: String::new(), template: true });
            }
            "assemble" => {
                let mut data = sample_assembly();
                data.components.retain(|c| c.id != 104);
                self.data = data.skeleton.clone();
                self.asm = Some(super::assembly::AsmSession::new(data));
                self.asm_regenerate(false);
                self.assemble_part("BOLT_M10");
                // pick the new bolt's shank: the Assembly collector then waits for the matching hole
                let id = self.place_editor().map(|e| e.comp.id).unwrap_or(0);
                let r = self.displayed().clone();
                let t = &r.topo;
                if let Some(f) = (0..t.faces.len()).find(|&f| !t.faces[f].planar && r.tag_owner.get(&t.face_tag(f)) == Some(&id) && t.faces[f].center.y < t.bbox.center().y + 1e9) {
                    self.handle_pick(Some(Pick::Face(f)), false);
                }
                if let Some(e) = self.asm.as_mut().and_then(|a| a.place.as_mut()) { e.side = Some(super::assembly::Side::Asm); }
            }
            "assembly" | "placement" | "explode" | "bom" | "asmtree" | "openpart" => {
                let data = sample_assembly();
                self.data = data.skeleton.clone();
                self.asm = Some(super::assembly::AsmSession::new(data));
                self.asm_regenerate(false);
                self.show_planes = scene == "placement";
                match scene {
                    "placement" => {
                        self.edit_placement(102);
                        if let Some(e) = self.asm.as_mut().and_then(|a| a.place.as_mut()) { e.active = 1; }
                        self.dash_panel = Some("Platzierung");
                        self.dash_panel_anchor = egui::pos2(8.0, 158.0);
                    }
                    "explode" => { if let Some(a) = &mut self.asm { a.exploded = true; a.explode = 0.9; } self.asm_regenerate(false); self.tab = Tab::View; }
                    "bom" => { if let Some(a) = &mut self.asm { a.show_bom = true; } self.windows.mass = true; }
                    "asmtree" => { self.expanded.insert(102); self.tree_click(102, false); }
                    "openpart" => self.asm_open_part(101),
                    _ => {}
                }
            }
            _ => {}
        }
        self.fit_pending = true;
        self.dirty = false;
    }

    pub fn demo_tick(&mut self, ctx: &egui::Context) {
        let n = FRAME.fetch_add(1, Ordering::Relaxed);
        // the right-click menu of the Datei öffnen list, drawn where a click on "Projekt A" would open it
        if std::env::var("FREO_DEMO").as_deref() == Ok("fsmenu") && n > 2 {
            let d = self.work_dir.clone();
            let mut act = None;
            egui::Area::new(egui::Id::new("demo_fsmenu")).fixed_pos(egui::pos2(560.0, 300.0)).order(egui::Order::Foreground).show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| { ui.set_min_width(200.0); super::creo::fs_menu(ui, &d, Some(&d.join("Projekt A")), &self.creo.fs, &mut act); });
            });
        }
        if n == 1 { if let Ok(scene) = std::env::var("FREO_DEMO") { self.demo_setup(&scene); } }
        if n == 4 && std::env::var_os("FREO_DEMO_ANNOTS").is_some() { self.demo_annotations(ctx); }
        if n == 4 {
            use super::modelcmds::FileTask;
            match std::env::var("FREO_DEMO_POPUP").as_deref() {
                Ok("regen_mgr") => self.creo.regen_manager = true,
                Ok("udf") => { self.tree_selection = self.data.features.iter().filter(|f| f.name.starts_with("Bohrung")).map(|f| f.id).collect(); self.open_file_task(FileTask::Udf); }
                Ok("shrinkwrap") => self.open_file_task(FileTask::Shrinkwrap),
                Ok("auto_round") => self.auto_round(),
                Ok("surfaces") => self.demo_surfaces(),
                Ok("phase2b") => self.demo_phase2b(),
                Ok("paste") => { self.tree_selection = self.data.features.iter().filter(|f| f.name == "Bohrung 2").map(|f| f.id).collect(); self.copy_features(); self.paste_features(); }
                _ => {}
            }
        }
        let Ok(shot) = std::env::var("FREO_SHOT") else { return };
        if n == 30 { ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default())); }
        if n < 200 { ctx.request_repaint(); }
        let img = ctx.input(|i| i.raw.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }));
        if let Some(img) = img {
            let [w, h] = img.size;
            let rgb = img.pixels.iter().map(|c| [c.r(), c.g(), c.b()]).collect();
            let png = crate::softrender::encode_png(&crate::softrender::Image { w, h, rgb });
            let _ = std::fs::write(&shot, png);
            if std::env::var("FREO_EXIT").is_ok() { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
        }
    }
}

/// Full-window sheet of every icon at 32 px and 16 px (visual check: FREO_DEMO=icons).
pub fn icon_sheet(ctx: &egui::Context) {
    use super::icons::{paint, Icon::*};
    let all = [New, Open, Save, SaveAs, Undo, Redo, Regenerate, Export, Close, Exit, Options, Help, Recent,
        Plane, Axis, Point, Csys, Sketch, Extrude, Revolve, Sweep, Blend, Hole, Round, Chamfer, Shell, Draft, Pattern, Mirror, PatternFill, PatternCurve,
        Parameters, Relations, Measure, MassProps, ModelInfo, Player,
        Refit, ZoomIn, ZoomOut, Repaint, SavedViews, DispShadedEdges, DispShaded, DispHidden, DispNoHidden, DispWire,
        DatumDisplay, SpinCenter, Section, Perspective, Appearance, Transparent,
        SkSelect, SkLine, SkRect, SkCenterRect, SkCircle, SkCircle3, SkArc3, SkArcCenter, SkFillet, SkChamfer, SkCenterline,
        SkPoint, SkPalette, SkSpline, SkEllipse, SkDeleteSeg, SkConstruction, SkDimension, SkModify, SkMirror, SkDivide, SkCorner, SkProject,
        ConV, ConH, ConPerp, ConTan, ConMid, ConCoinc, ConSym, ConEqual, ConPar,
        SketchView, Requirements, OpenEnds, ShadeLoops, Grid, Vertex, DimDisplay, ConDisplay,
        Ok, Cancel, Preview, Flip, RemoveMaterial, Thicken, DepthBlind, DepthSymmetric, DepthThrough, DepthNext, DepthSelected, Taper,
        HoleSimple, HoleCbore, HoleCsink, HoleStandard, Delete, Suppress, Resume, Hide, Show, EditDef, EditDims,
        Part, InsertHere, Failed, TreeTab, FolderTab, FolderItem, FileItem, Up, RegenOk, RegenFail,
        Assembly, Assemble, CreateComp, Explode, Bom, Interference, ReturnAsm, DragComp,
        AsmAuto, AsmDistance, AsmAngle, AsmParallel, AsmCoincident, AsmNormal, AsmDefault, AsmFix,
        Drawing, Sheet, NewSheet, GeneralView, ProjView, DetailView, SectionView, DwgDim, Note, Balloon, ShowAnnot, TitleBlock, Pdf, Dxf, Svg, Cleanup, Lock, ReturnDwg, GeomTol, DatumFeature, SurfFinish, DimTol];
    egui::CentralPanel::default().show(ctx, |ui| {
        let p = ui.painter();
        let origin = ui.max_rect().min + egui::vec2(10.0, 10.0);
        for (i, ic) in all.iter().enumerate() {
            let (cx, cy) = ((i % 16) as f32, (i / 16) as f32);
            let base = origin + egui::vec2(cx * 86.0, cy * 74.0);
            paint(p, egui::Rect::from_min_size(base, egui::vec2(32.0, 32.0)), *ic, true);
            paint(p, egui::Rect::from_min_size(base + egui::vec2(40.0, 8.0), egui::vec2(16.0, 16.0)), *ic, true);
            paint(p, egui::Rect::from_min_size(base + egui::vec2(60.0, 8.0), egui::vec2(16.0, 16.0)), *ic, false);
            p.text(base + egui::vec2(0.0, 40.0), egui::Align2::LEFT_TOP, format!("{:?}", ic), egui::FontId::proportional(10.0), egui::Color32::DARK_GRAY);
        }
    });
}

impl App {
    /// GD&T on the bracket drawing: datums on the base and a side, tolerances and finishes.
    fn demo_gdt(&mut self) {
        use crate::model::drawing::*;
        let Some(d) = &self.drw else { return };
        let vs = d.data.sheets[0].views.clone();
        let (front, top) = (vs[0].clone(), vs[1].clone());
        let (Some(gf), Some(gt)) = (self.view_geom(front.id), self.view_geom(top.id)) else { return };
        let put = |app: &mut App, view: &DrawView, g: &super::drawing::ViewGeom, p: V3| app.anchor_at(g.world(view.pos, p), 1.5).map(|x| (x.0, x.1));
        // datum A: bottom face (front view bottom edge), datum B: left face (top view left edge)
        let a = put(self, &front, &gf, V3::new(-20.0, 0.0, 40.0));
        let b = put(self, &top, &gt, V3::new(-60.0, 20.0, 5.0));
        let f1 = put(self, &front, &gf, V3::new(-40.0, 20.0, 40.0));
        let f2 = put(self, &front, &gf, V3::new(-8.0, 50.0, 0.0));
        let flat = put(self, &front, &gf, V3::new(42.0, 20.0, 40.0));
        let si = 0;
        self.drw_commit(|d| {
            if let Some((v, r)) = a { let id = d.take_id(); d.sheets[si].datums.push(DatumSym { id, view: v, letter: "A".into(), at: r, pos: V2::new(-30.0, -40.0) }); }
            if let Some((v, r)) = b { let id = d.take_id(); d.sheets[si].datums.push(DatumSym { id, view: v, letter: "B".into(), at: r, pos: V2::new(-74.0, 52.0) }); }
            if let Some((v, r)) = f1 { let id = d.take_id(); d.sheets[si].finishes.push(Finish { id, view: v, kind: FinishKind::Removal, value: "Ra 1.6".into(), method: String::new(), at: r, flip: false }); }
            if let Some((v, r)) = f2 { let id = d.take_id(); d.sheets[si].finishes.push(Finish { id, view: v, kind: FinishKind::Removal, value: "Ra 0.8".into(), method: "GESCHLIFFEN".into(), at: r, flip: false }); }
            if let Some((v, r)) = flat { let id = d.take_id(); d.sheets[si].gtols.push(Gtol { id, view: v, ch: GChar::Flatness, diameter: false, value: 0.02, mat: MatCond::Rfs, datums: vec![], target: Attach::Geom(r), pos: V2::new(48.0, 24.0) }); }
            // tolerances on shown dimensions
            for s in &mut d.sheets[si].shown {
                if s.key.ends_with(":f:diameter") && s.key.starts_with("14:") { s.tol = Tolerance { mode: TolMode::Fit, fit: "H7".into(), ..Default::default() }; }
                if s.key.ends_with("10:f:depth") { s.tol = Tolerance { mode: TolMode::PlusMinus, upper: 0.05, lower: -0.02, ..Default::default() }; }
                if s.key.ends_with("11:f:depth") { s.tol = Tolerance { mode: TolMode::Symmetric, upper: 0.1, ..Default::default() }; }
                if s.key.ends_with(":f:spacing1") || s.key.ends_with(":f:spacing2") { s.tol = Tolerance { mode: TolMode::Basic, ..Default::default() }; }
            }
            let targets: Vec<DimTarget> = d.sheets[si].shown.iter().filter(|s| s.key.ends_with(":f:diameter") && s.key.starts_with("12:")).map(|s| DimTarget::Shown(s.key.clone())).collect();
            for t in targets {
                let view = d.sheets[si].shown.iter().find(|s| DimTarget::Shown(s.key.clone()) == t).map(|s| s.view).unwrap_or(0);
                let id = d.take_id();
                d.sheets[si].gtols.push(Gtol { id, view, ch: GChar::Position, diameter: true, value: 0.1, mat: MatCond::Mmc, datums: vec![("A".into(), MatCond::Rfs), ("B".into(), MatCond::Rfs)], target: Attach::Dim(t), pos: V2::ZERO });
            }
        });
    }
}
