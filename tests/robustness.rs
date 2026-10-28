//! Robustness: edge cases a user can reach must never panic (empty models, invalid values,
//! deleting referenced items, odd files). Each case runs isolated; all failures are reported.

use freo::app::drawing::DSel;
use freo::app::drawui::NewDrawing;
use freo::app::{App, Pick};
use freo::kernel::math::*;
use freo::model::drawing::*;
use freo::model::features::*;
use freo::sketch::sketch::Sketch;
use std::panic::{catch_unwind, AssertUnwindSafe};

fn feat(id: u64, kind: FeatureKind) -> Feature { Feature { id, name: format!("F{}", id), suppressed: false, hidden: false, kind } }

fn block() -> ModelData {
    let mut m = ModelData::new_part("B");
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(TOP_ID));
    e.sketch = Sketch::rect(40.0, 30.0, 0.0, 0.0);
    e.depth = Dim::v(10.0);
    m.features.push(feat(10, FeatureKind::Extrude(e)));
    m
}

fn part_app(m: ModelData) -> App { let mut a = App::new(None); a.data = m; a.regenerate(false); a }

fn drawing(m: ModelData) -> App {
    let mut a = part_app(m);
    a.new_drawing(&NewDrawing { name: "D".into(), size: SheetSize::A4, orient: Orientation::Portrait, template: true, projection: Projection::First });
    a
}

fn scene_all(a: &mut App) {
    let n = a.drw.as_ref().map(|d| d.data.sheets.len()).unwrap_or(0);
    for i in 0..n { let _ = a.sheet_scene(i, &[]); }
}

#[test]
fn edge_cases_do_not_panic() {
    let mut failures: Vec<String> = vec![];
    let mut case = |name: &str, f: &dyn Fn()| {
        if catch_unwind(AssertUnwindSafe(f)).is_err() { failures.push(name.to_string()); }
    };
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|i| eprintln!("panic in case: {}", i)));

    case("empty part undo/redo/regen", &|| { let mut a = App::new(None); a.do_undo(); a.do_redo(); a.regenerate(true); a.handle_pick(None, false); });
    case("every feature started and cancelled on an empty part", &|| {
        let mut a = App::new(None);
        for k in [FeatureKind::new_extrude(), FeatureKind::new_revolve(), FeatureKind::new_sweep(), FeatureKind::new_blend(), FeatureKind::new_hole(), FeatureKind::new_round(), FeatureKind::new_chamfer(), FeatureKind::new_shell(), FeatureKind::new_draft(), FeatureKind::new_pattern(), FeatureKind::new_mirror(), FeatureKind::new_plane(), FeatureKind::new_axis(), FeatureKind::new_point(), FeatureKind::new_sketch()] {
            a.start_feature(k);
            a.finish_feature();
            if a.sketch.is_some() { a.cancel_sketch(); }
            if a.editor.is_some() { a.cancel_feature(); }
        }
    });
    case("invalid dimension values regenerate with errors", &|| {
        let mut a = part_app(block());
        a.commit("", |d| if let FeatureKind::Extrude(e) = &mut d.features[4].kind { e.depth = Dim::v(-5.0); });
        a.commit("", |d| if let FeatureKind::Extrude(e) = &mut d.features[4].kind { e.depth = Dim("1/0".into()); });
        a.commit("", |d| if let FeatureKind::Extrude(e) = &mut d.features[4].kind { e.depth = Dim("nonsense +".into()); });
        let FeatureKind::Round(mut r) = FeatureKind::new_round() else { unreachable!() };
        r.sets[0].radius = Dim::v(1000.0);
        let FeatureKind::Pattern(mut p) = FeatureKind::new_pattern() else { unreachable!() };
        p.features = vec![10]; p.count1 = Dim::v(-3.0); p.spacing1 = Dim::v(0.0);
        let FeatureKind::Shell(mut sh) = FeatureKind::new_shell() else { unreachable!() };
        sh.thickness = Dim::v(500.0);
        let FeatureKind::Hole(mut h) = FeatureKind::new_hole() else { unreachable!() };
        h.plane = Some(PlaneRef::Datum(TOP_ID)); h.diameter = Dim::v(0.0);
        a.commit("", |d| { d.features.push(feat(20, FeatureKind::Round(r))); d.features.push(feat(21, FeatureKind::Pattern(p))); d.features.push(feat(22, FeatureKind::Shell(sh))); d.features.push(feat(23, FeatureKind::Hole(h))); });
        a.start_edit_dims(10);
    });
    case("round with no sets loads and edits", &|| {
        let mut m = block();
        m.features.push(feat(20, FeatureKind::Round(RoundDef { sets: vec![] })));
        let mut a = part_app(m);
        a.edit_feature(20);
        a.finish_feature();
    });
    case("old round file format", &|| {
        let json = r#"{"edges":[],"radius":"3"}"#;
        let r: RoundDef = serde_json::from_str(json).expect("old RoundDef must load");
        assert_eq!(r.sets.len(), 1);
    });
    case("suppress everything, export and mass", &|| {
        let mut a = part_app(block());
        let ids: Vec<u64> = a.data.features.iter().map(|f| f.id).collect();
        a.set_suppressed(&ids, true);
        let _ = freo::app::export::mass_properties(&a.result, 1.0);
        let _ = freo::app::export::step_text(&a.result, "X", "x.stp");
        a.delete_features(&ids);
    });
    case("assembly edge cases", &|| {
        let mut a = App::new(None);
        a.new_assembly("ASM");
        a.asm_regenerate(true);
        a.asm_delete(&[999]);
        a.place_finish();
        a.place_cancel();
        a.asm_open_part(999);
        a.asm_return();
        a.toggle_exploded();
        a.run_interference();
        let _ = a.asm_mass();
        a.assemble_part("NOPE");
        a.assemble_path(std::path::PathBuf::from("/nonexistent/x.pfprt"));
        a.create_component("P1");
        a.asm_return();
        a.create_component("P1");
        a.handle_pick(Some(Pick::Face(9999)), false);
        a.handle_pick(Some(Pick::Plane(u64::MAX)), false);
        a.do_undo(); a.do_undo(); a.do_redo();
        a.new_drawing(&NewDrawing { name: "D".into(), size: SheetSize::A3, orient: Orientation::Landscape, template: true, projection: Projection::Third });
        scene_all(&mut a);
        a.insert_bom();
        let v = a.drw.as_ref().unwrap().data.sheets[0].views[0].id;
        a.create_balloons(v);
        scene_all(&mut a);
    });
    case("placement referencing a deleted component", &|| {
        let mut a = App::new(None);
        let data = freo::app::demo::sample_assembly();
        a.data = data.skeleton.clone();
        a.asm = Some(freo::app::assembly::AsmSession::new(data));
        a.asm_regenerate(false);
        a.asm_delete(&[100]);
        a.edit_placement(101);
        a.place_update();
        a.place_finish();
        a.asm_set_flag(&[102], Some(true), None);
        a.new_drawing(&NewDrawing { name: "D".into(), size: SheetSize::A3, orient: Orientation::Landscape, template: true, projection: Projection::Third });
        a.insert_bom();
        let v = a.drw.as_ref().unwrap().data.sheets[0].views[3].id;
        a.create_balloons(v);
        // the drawing outlives component deletions made in 3D
        a.drawing_open_model();
        a.asm_delete(&[103, 104]);
        a.drawing_return();
        scene_all(&mut a);
    });
    case("drawing of an empty part", &|| {
        let mut a = drawing(ModelData::new_part("EMPTY"));
        let ids: Vec<u32> = a.drw.as_ref().unwrap().data.sheets[0].views.iter().map(|v| v.id).collect();
        a.show_model_dims(&ids);
        a.cleanup_dims(&ids);
        scene_all(&mut a);
        let _ = a.snap_at(V2::new(100.0, 100.0), 5.0);
        let _ = a.anchor_at(V2::new(100.0, 100.0), 5.0);
        let prims = a.sheet_scene(0, &[]).0;
        let _ = freo::app::publish::pdf(&[(210.0, 297.0, prims.clone())], "");
        let _ = freo::app::publish::dxf(&prims);
        let _ = freo::app::publish::svg(210.0, 297.0, &prims);
    });
    case("delete views that others depend on", &|| {
        let mut a = drawing(block());
        let ids: Vec<u32> = a.drw.as_ref().unwrap().data.sheets[0].views.iter().map(|v| v.id).collect();
        a.show_model_dims(&ids);
        // a detail of the front view, then delete the front view (projections and detail go too)
        a.drw_commit(|d| {
            let id = d.take_id();
            let mut v = DrawView::new(id, ViewKind::Detail, Orient::Front.rotation(), V2::new(150.0, 150.0));
            v.parent = Some(ids[0]); v.detail = Some(Detail { center: V2::ZERO, radius: 0.0 }); v.scale = Some(2.0);
            d.sheets[0].views.push(v);
        });
        scene_all(&mut a);
        a.drw.as_mut().unwrap().sel = vec![DSel::View(ids[0])];
        a.drw_delete_selection();
        assert_eq!(a.drw.as_ref().unwrap().data.sheets[0].views.len(), 1, "only the pictorial view is left");
        scene_all(&mut a);
        a.drw_undo(false); a.drw_undo(false); a.drw_undo(false); a.drw_undo(false); a.drw_undo(true);
        a.drw_delete_selection();
        scene_all(&mut a);
    });
    case("sections through odd planes", &|| {
        let mut a = drawing(block());
        // FRONT is not parallel to the top view, an offset plane misses the part entirely
        let FeatureKind::DatumPlane(mut dp) = FeatureKind::new_plane() else { unreachable!() };
        dp.reference = Some(PlaneRef::Datum(RIGHT_ID)); dp.offset = Dim::v(500.0);
        a.commit("", |d| d.features.push(feat(30, FeatureKind::DatumPlane(dp))));
        let vs = a.drw.as_ref().unwrap().data.sheets[0].views.clone();
        a.drw_commit(|d| {
            d.sheets[0].view_mut(vs[1].id).unwrap().section = Some(FRONT_ID);
            d.sheets[0].view_mut(vs[2].id).unwrap().section = Some(30);
            d.sheets[0].view_mut(vs[0].id).unwrap().section = Some(12345);
        });
        scene_all(&mut a);
    });
    case("model edits invalidate shown dimensions and references", &|| {
        let mut a = drawing(block());
        let ids: Vec<u32> = a.drw.as_ref().unwrap().data.sheets[0].views.iter().map(|v| v.id).collect();
        a.show_model_dims(&ids);
        let front = a.drw.as_ref().unwrap().data.sheets[0].views[0].clone();
        let g = a.view_geom(front.id).unwrap();
        if let Some((v, r, _)) = a.anchor_at(g.world(front.pos, V3::new(0.0, 10.0, 15.0)), 2.0) {
            a.drw_commit(|d| {
                let id = d.take_id(); d.sheets[0].finishes.push(Finish { id, view: v, kind: FinishKind::Any, value: String::new(), method: String::new(), at: r, flip: true });
                let id = d.take_id(); d.sheets[0].datums.push(DatumSym { id, view: v, letter: "A".into(), at: r, pos: V2::ZERO });
                let id = d.take_id(); d.sheets[0].gtols.push(Gtol { id, view: v, ch: GChar::TotalRunout, diameter: false, value: 0.0, mat: MatCond::Lmc, datums: vec![(String::new(), MatCond::Mmc)], target: Attach::Geom(r), pos: V2::ZERO });
            });
        }
        // the extrude goes away entirely
        a.commit("", |d| d.features.truncate(4));
        scene_all(&mut a);
        a.do_undo();
        a.commit("", |d| if let FeatureKind::Extrude(e) = &mut d.features[4].kind { e.sketch = Sketch::circle(5.0, 0.0, 0.0); });
        scene_all(&mut a);
        for i in 0..a.drw.as_ref().unwrap().data.sheets[0].shown.len() { a.commit_edit(DSel::Shown(i), "".into()); a.commit_edit(DSel::Shown(i), "abc".into()); }
    });
    case("tolerances with odd values", &|| {
        let mut a = drawing(block());
        let ids: Vec<u32> = a.drw.as_ref().unwrap().data.sheets[0].views.iter().map(|v| v.id).collect();
        a.show_model_dims(&ids);
        let keys: Vec<String> = a.drw.as_ref().unwrap().data.sheets[0].shown.iter().map(|s| s.key.clone()).collect();
        for (k, mode) in keys.iter().zip(TolMode::ALL.iter().cycle()) {
            a.set_dim_tolerance(&DimTarget::Shown(k.clone()), Tolerance { mode: *mode, upper: f64::NAN, lower: 1e12, fit: "zz".into() });
        }
        a.drw.as_mut().unwrap().data.tol_decimals = 9;
        a.drw.as_mut().unwrap().data.decimals = 0;
        scene_all(&mut a);
        let _ = freo::model::tolerance::fit_deviations("H99", 10.0);
        let _ = freo::model::tolerance::fit_deviations("", 10.0);
        let _ = freo::model::tolerance::fit_deviations("H7", f64::NAN);
        let _ = freo::model::tolerance::fit_deviations("7", 10.0);
    });
    case("drawing files that are broken or incomplete", &|| {
        let dir = std::env::temp_dir().join(format!("freo_rob_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut a = App::new(None);
        for (name, body) in [
            ("nosheets.pfdrw", r#"{"drawing":{"name":"X","sheets":[],"next_id":1,"projection":"Third","decimals":2,"text_height":3.5,"title":"","company":"","drawn_by":"","material":"","date":""}}"#),
            ("nomodel.pfdrw", r#"{"drawing":{"name":"X","sheets":[{"size":"A4","orient":"Landscape","scale":1.0,"views":[]}],"next_id":1,"projection":"Third","decimals":2,"text_height":3.5,"title":"","company":"","drawn_by":"","material":"","date":""},"model_path":"/nonexistent.pfprt"}"#),
            ("garbage.pfdrw", "not json"),
            ("garbage.pfasm", "{}"),
            ("garbage.pfprt", "[1,2,3]"),
        ] {
            let p = dir.join(name);
            std::fs::write(&p, body).unwrap();
            a.load(p);
            if a.drw.is_some() { scene_all(&mut a); a.add_sheet_pub(); a.drw_undo(false); }
        }
        let _ = std::fs::remove_dir_all(&dir);
    });
    case("sheet tabs and sheet operations", &|| {
        let mut a = drawing(block());
        a.add_sheet_pub(); a.add_sheet_pub();
        a.delete_sheet_pub(); a.delete_sheet_pub(); a.delete_sheet_pub(); a.delete_sheet_pub();
        a.drw_undo(false); a.drw_undo(false); a.drw_undo(false);
        scene_all(&mut a);
    });

    std::panic::set_hook(prev);
    assert!(failures.is_empty(), "cases that panicked: {:?}", failures);
}
