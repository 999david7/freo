//! End-to-end workflow tests through the same controller paths the UI uses.

use freo::app::sketcher::Tool;
use freo::app::{App, Pick, PickTarget, Slot};
use freo::kernel::csg::signed_volume;
use freo::kernel::math::*;
use freo::model::features::*;
use freo::sketch::sketch::{SkCType, Sketch};

fn vol(app: &App) -> f64 { signed_volume(&app.result.polys) }

fn face_with_normal(app: &App, n: V3, y: f64) -> usize {
    let t = &app.result.topo;
    (0..t.faces.len()).find(|&f| t.faces[f].planar && t.faces[f].normal.dot(n) > 0.999 && (t.faces[f].center.y - y).abs() < 1e-6).expect("face")
}

/// Draws a closed w x h rectangle with the line tool, clicking four corners and the start again.
fn draw_rect_with_lines(app: &mut App, w: f64, h: f64) {
    let s = app.sketch.as_mut().expect("sketcher open");
    s.set_tool(Tool::Line);
    for p in [V2::new(-w / 2.0, -h / 2.0), V2::new(w / 2.0, -h / 2.0), V2::new(w / 2.0, h / 2.0), V2::new(-w / 2.0, h / 2.0), V2::new(-w / 2.0, -h / 2.0)] {
        s.click_at(p);
    }
}

fn finish_sketch(app: &mut App) {
    let s = app.sketch.as_ref().unwrap();
    let (sk, slot) = (s.sketch.clone(), s.slot);
    app.finish_sketch(sk, slot);
}

#[test]
fn extrude_hole_round_undo_redo() {
    let mut app = App::new(None);
    // object-action: select TOP, Extrude -> sketcher opens directly
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.start_feature(FeatureKind::new_extrude());
    assert!(app.sketch.is_some(), "sketcher should open on the preselected plane");
    draw_rect_with_lines(&mut app, 100.0, 60.0);
    let s = app.sketch.as_ref().unwrap();
    assert_eq!(s.sketch.entities.len(), 4, "closed chain of 4 lines");
    let hv = s.sketch.constraints.iter().filter(|c| matches!(c.ty, SkCType::Horizontal | SkCType::Vertical)).count();
    assert_eq!(hv, 4, "auto H/V constraints inferred while drawing");
    assert!(s.sketch.open_ends().is_empty(), "loop must be closed");
    finish_sketch(&mut app);
    app.update_editor(|f| if let FeatureKind::Extrude(d) = &mut f.kind { d.depth = Dim::v(20.0) });
    assert!(app.editor_error().is_none(), "{:?}", app.editor_error());
    app.finish_feature();
    assert_eq!(app.data.features.len(), 5);
    assert!((vol(&app) - 120000.0).abs() < 0.5, "box volume {}", vol(&app));

    // hole on the top face, pre-selected
    let top = face_with_normal(&app, V3::Y, 20.0);
    app.handle_pick(Some(Pick::Face(top)), false);
    app.start_feature(FeatureKind::new_hole());
    app.update_editor(|f| if let FeatureKind::Hole(d) = &mut f.kind { d.through_all = true; d.x = Dim::v(0.0); d.y = Dim::v(0.0) });
    assert!(app.editor_error().is_none(), "{:?}", app.editor_error());
    app.finish_feature();
    let v_hole = vol(&app);
    assert!(v_hole < 120000.0 - 1500.0 && v_hole > 120000.0 - 1600.0, "hole volume {}", v_hole);

    // round four vertical edges picked through the edge collector
    app.start_feature(FeatureKind::new_round());
    assert_eq!(app.editor.as_ref().unwrap().pick, Some(PickTarget::Edges));
    let t = app.displayed().topo.clone();
    let picks: Vec<usize> = (0..t.chains.len()).filter(|&i| t.chains[i].is_straight() && (t.chains[i].points[0].y - t.chains[i].points.last().unwrap().y).abs() > 1.0).collect();
    assert_eq!(picks.len(), 4);
    for i in picks { app.handle_pick(Some(Pick::Edge(i)), false); }
    app.update_editor(|f| if let FeatureKind::Round(d) = &mut f.kind { d.sets[0].radius = Dim::v(5.0) });
    app.finish_feature();
    assert!(app.result.errors.is_empty(), "{:?}", app.result.errors);
    let v_round = vol(&app);
    let expect = v_hole - 4.0 * 20.0 * (25.0 - std::f64::consts::PI * 25.0 / 4.0);
    assert!((v_round - expect).abs() < 30.0, "round {} expected ≈ {}", v_round, expect);

    app.do_undo();
    assert!((vol(&app) - v_hole).abs() < 1e-6);
    app.do_redo();
    assert!((vol(&app) - v_round).abs() < 1e-6);
}

#[test]
fn sketch_dimensions_constraints_and_conflicts() {
    let mut app = App::new(None);
    app.handle_pick(Some(Pick::Plane(FRONT_ID)), false);
    app.start_feature(FeatureKind::new_extrude());
    draw_rect_with_lines(&mut app, 40.0, 20.0);
    let s = app.sketch.as_mut().unwrap();
    // dimension the bottom line: click the line, then empty space
    s.set_tool(Tool::Dimension);
    s.click_at(V2::new(0.0, -10.0));
    s.click_at(V2::new(0.0, -25.0));
    let dim = s.sketch.constraints.iter().find(|c| c.ty == SkCType::Distance && !c.weak).expect("length dimension").clone();
    assert!((dim.value - 40.0).abs() < 1e-3);
    assert!(s.sketch.constraints.iter().any(|c| c.weak), "weak dimensions are generated");
    s.set_dimension(dim.id, "75");
    let bottom = s.sketch.entities[0].clone();
    assert!((s.sketch.point(bottom.pts[1]) - s.sketch.point(bottom.pts[0])).length() - 75.0 < 1e-6);
    // a second, conflicting constraint is rejected: the bottom line is already horizontal
    let before = s.sketch.constraints.iter().filter(|c| !c.weak).count();
    s.set_tool(Tool::Con(SkCType::Horizontal));
    s.click_at(V2::new(20.0, -10.0));
    assert_eq!(s.sketch.constraints.iter().filter(|c| !c.weak).count(), before, "redundant H must be rejected");
    // equal: top and right lines
    s.set_tool(Tool::Con(SkCType::Equal));
    let (r0, r1) = (s.sketch.point(s.sketch.entities[1].pts[0]), s.sketch.point(s.sketch.entities[1].pts[1]));
    s.click_at((r0 + r1) * 0.5);
    let (b0, b1) = (s.sketch.point(s.sketch.entities[0].pts[0]), s.sketch.point(s.sketch.entities[0].pts[1]));
    s.click_at((b0 + b1) * 0.5);
    let e1 = s.sketch.entities[1].clone();
    assert!(((s.sketch.point(e1.pts[1]) - s.sketch.point(e1.pts[0])).length() - 75.0).abs() < 1e-5, "equal makes a 75 square");
    // circle with diameter dimension driven by a parameter expression
    s.set_tool(Tool::Circle);
    s.click_at(V2::new(10.0, 10.0));
    s.click_at(V2::new(15.0, 10.0));
    s.set_tool(Tool::Dimension);
    s.click_at(V2::new(15.0, 10.0));
    s.click_at(V2::new(30.0, 30.0));
    let d = s.sketch.constraints.iter().find(|c| c.ty == SkCType::Diameter && !c.weak).expect("diameter").clone();
    assert!((d.value - 10.0).abs() < 1e-3);
    finish_sketch(&mut app);
    app.finish_feature();
    assert!(app.result.errors.is_empty(), "{:?}", app.result.errors);
    let v = vol(&app);
    assert!((v - (75.0 * 75.0 - std::f64::consts::PI * 25.0) * 20.0).abs() < 60.0, "square with hole volume {}", v);
}

#[test]
fn parameters_relations_drive_geometry() {
    let mut app = App::new(None);
    app.commit("", |d| {
        d.parameters.push(Parameter { name: "W".into(), expr: "50".into(), note: String::new() });
        d.relations = "D = W / 5".into();
        let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
        e.plane = Some(PlaneRef::Datum(TOP_ID));
        e.sketch = Sketch::rect(50.0, 50.0, 0.0, 0.0);
        e.depth = Dim("D".into());
        d.features.push(Feature { id: 10, name: "Extrude 1".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(e) });
    });
    assert!((vol(&app) - 50.0 * 50.0 * 10.0).abs() < 1e-6);
    app.commit("", |d| d.parameters[0].expr = "100".into());
    assert!((vol(&app) - 50.0 * 50.0 * 20.0).abs() < 1e-6, "relation-driven depth");
}

#[test]
fn tree_operations_dependencies_insert_and_files() {
    let mut app = App::new(None);
    // datum plane offset from TOP, then an extrude sketched on it
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.start_feature(FeatureKind::new_plane());
    app.update_editor(|f| if let FeatureKind::DatumPlane(d) = &mut f.kind { d.offset = Dim::v(30.0) });
    app.finish_feature();
    let dtm = app.data.features.last().unwrap().id;
    assert!(app.result.plane(dtm).is_some());
    app.handle_pick(Some(Pick::Plane(dtm)), false);
    app.start_feature(FeatureKind::new_extrude());
    app.sketch.as_mut().unwrap().sketch = Sketch::circle(10.0, 0.0, 0.0);
    finish_sketch(&mut app);
    app.finish_feature();
    let ext = app.data.features.last().unwrap().id;
    let b = app.result.topo.bbox;
    assert!((b.min.y - 30.0).abs() < 1e-6, "extrude starts on the offset plane");

    // pattern of the extrude from a tree selection
    app.tree_click(ext, false);
    app.start_feature(FeatureKind::new_pattern());
    app.update_editor(|f| if let FeatureKind::Pattern(d) = &mut f.kind { d.count1 = Dim::v(4.0); d.spacing1 = Dim::v(30.0) });
    app.finish_feature();
    let one = std::f64::consts::PI * 100.0 * 20.0;
    assert!((vol(&app) - 4.0 * one).abs() / one < 0.02, "pattern of 4: {}", vol(&app));

    // suppress / resume
    let pat = app.data.features.last().unwrap().id;
    app.set_suppressed(&[pat], true);
    assert!((vol(&app) - one).abs() / one < 0.02);
    app.set_suppressed(&[pat], false);

    // insert mode before the pattern: new features go before it
    app.set_insert_after(Some(ext));
    assert!(app.result.rolled_back.contains(&pat));
    app.set_insert_after(None);

    // save / load round trip
    let json = serde_json::to_string(&app.data).unwrap();
    let back: ModelData = serde_json::from_str(&json).unwrap();
    assert_eq!(back, app.data);

    // export
    let dir = std::env::temp_dir().join("freo_test");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("part.stl");
    freo::app::export::write(&app.result, &app.data, freo::app::export::Format::StlBinary, &p).unwrap();
    let n = u32::from_le_bytes(std::fs::read(&p).unwrap()[80..84].try_into().unwrap()) as usize;
    assert_eq!(std::fs::metadata(&p).unwrap().len() as usize, 84 + n * 50);
    let m = freo::app::export::mass_properties(&app.result, 7.85e-6);
    assert!((m.volume - vol(&app)).abs() < 1e-6);

    // deleting the datum plane removes its dependent children
    app.delete_features(&[dtm]);
    assert!(app.data.feature(ext).is_none() && app.data.feature(pat).is_none());
    assert_eq!(app.data.features.len(), 4);
}

#[test]
fn revolve_sweep_blend_mirror_shell_through_controller() {
    let mut app = App::new(None);
    app.handle_pick(Some(Pick::Plane(FRONT_ID)), false);
    app.start_feature(FeatureKind::new_revolve());
    {
        let s = app.sketch.as_mut().unwrap();
        s.set_tool(Tool::Centerline);
        s.click_at(V2::new(0.0, -20.0));
        s.click_at(V2::new(0.0, 40.0));
        s.set_tool(Tool::Rect);
        s.click_at(V2::new(20.0, 0.0));
        s.click_at(V2::new(30.0, 30.0));
    }
    finish_sketch(&mut app);
    app.finish_feature();
    assert!(app.result.errors.is_empty(), "{:?}", app.result.errors);
    let tube = std::f64::consts::PI * (900.0 - 400.0) * 30.0;
    assert!((vol(&app) - tube).abs() / tube < 0.01, "revolved tube {}", vol(&app));

    // shell needs faces -> remove top face
    app.start_feature(FeatureKind::new_shell());
    let top = face_with_normal(&app, V3::Y, 30.0);
    app.handle_pick(Some(Pick::Face(top)), false);
    app.update_editor(|f| if let FeatureKind::Shell(d) = &mut f.kind { d.thickness = Dim::v(2.0) });
    assert!(app.editor_error().is_none(), "{:?}", app.editor_error());
    app.finish_feature();
    assert!(vol(&app) < tube * 0.7 && vol(&app) > 0.0, "shelled {}", vol(&app));

    // mirror all geometry about RIGHT doubles nothing (symmetric) but must regenerate cleanly
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.start_feature(FeatureKind::new_mirror());
    app.finish_feature();
    assert!(app.result.errors.is_empty(), "{:?}", app.result.errors);
    assert!(app.result.topo.bbox.min.y < -29.0, "mirrored below TOP");

    // sweep: trajectory then section are requested in sequence
    let mut app = App::new(None);
    app.handle_pick(Some(Pick::Plane(FRONT_ID)), false);
    app.start_feature(FeatureKind::new_sweep());
    {
        let s = app.sketch.as_mut().unwrap();
        assert_eq!(s.slot, Slot::Trajectory);
        s.set_tool(Tool::Line);
        for p in [V2::new(0.0, 0.0), V2::new(50.0, 0.0), V2::new(50.0, 40.0)] { s.click_at(p); }
        s.end_chain();
    }
    finish_sketch(&mut app);
    assert_eq!(app.sketch.as_ref().map(|s| s.slot), Some(Slot::Section), "section sketch opens next");
    app.sketch.as_mut().unwrap().sketch = Sketch::circle(4.0, 0.0, 0.0);
    finish_sketch(&mut app);
    app.finish_feature();
    let swept = std::f64::consts::PI * 16.0 * 90.0;
    assert!((vol(&app) - swept).abs() / swept < 0.03, "sweep {}", vol(&app));

    // blend with two sections
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.start_feature(FeatureKind::new_blend());
    app.sketch.as_mut().unwrap().sketch = Sketch::rect(30.0, 30.0, 100.0, 0.0);
    finish_sketch(&mut app);
    app.begin_sketch(Slot::Blend(1));
    app.sketch.as_mut().unwrap().sketch = Sketch::circle(8.0, 100.0, 0.0);
    finish_sketch(&mut app);
    assert!(app.editor_error().is_none(), "{:?}", app.editor_error());
    app.finish_feature();
    assert!(app.result.errors.is_empty(), "{:?}", app.result.errors);
}

#[test]
fn profil_sketch_dialog_and_existing_sketch() {
    // Platzierung ▸ Definieren...: the Skizze dialog takes the plane; sketching starts on its Skizze button
    let mut app = App::new(None);
    app.start_feature(FeatureKind::new_extrude());
    assert_eq!(app.editor.as_ref().unwrap().pick, Some(PickTarget::SketchPlane), "Profil waits for a sketch plane");
    app.open_sketch_dialog();
    app.route_pick(Pick::Plane(TOP_ID), PickTarget::SketchPlane);
    assert!(app.sketch.is_none(), "picking in the dialog must not start the sketcher");
    assert_eq!(app.editor.as_ref().unwrap().feature.kind.sketch_plane(), Some(PlaneRef::Datum(TOP_ID)));
    app.cancel_sketch_dialog();
    assert_eq!(app.editor.as_ref().unwrap().feature.kind.sketch_plane(), None, "Abbrechen restores the plane");
    app.open_sketch_dialog();
    app.route_pick(Pick::Plane(FRONT_ID), PickTarget::SketchPlane);
    app.begin_sketch(Slot::Main);
    assert!(app.sketch.is_some() && app.sketch_dlg.is_none(), "Skizze button opens the sketcher");
    app.cancel_feature();

    // a sketch feature selected while Profil waits for its section is used as the section
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.start_feature(FeatureKind::new_sketch());
    app.sketch.as_mut().unwrap().sketch = Sketch::rect(40.0, 30.0, 0.0, 0.0);
    finish_sketch(&mut app);
    let sk_id = app.data.features.last().unwrap().id;
    assert!(matches!(app.data.features.last().unwrap().kind, FeatureKind::Sketch(_)));
    app.selection.clear();
    app.tree_selection.clear();
    app.start_feature(FeatureKind::new_extrude());
    app.tree_click(sk_id, false);
    assert!(app.sketch.is_none(), "an existing sketch needs no sketcher");
    app.update_editor(|f| if let FeatureKind::Extrude(d) = &mut f.kind { d.depth = Dim::v(10.0) });
    assert!(app.editor_error().is_none(), "{:?}", app.editor_error());
    app.finish_feature();
    assert!((vol(&app) - 12000.0).abs() < 0.5, "volume {}", vol(&app));
}
