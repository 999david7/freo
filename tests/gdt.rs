//! Dimension tolerances (including ISO 286 fits) and GD&T annotations on drawings.

use freo::app::drawing::{tol_label, DSel, Prim};
use freo::app::drawui::NewDrawing;
use freo::app::App;
use freo::kernel::math::*;
use freo::model::drawing::*;
use freo::model::features::*;
use freo::model::tolerance::{fit_deviations, it_grade};
use freo::sketch::sketch::Sketch;

fn close(a: (f64, f64), b: (f64, f64)) -> bool { (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9 }

#[test]
fn iso_286_limits_and_fits() {
    assert_eq!(it_grade(7, 25.0), Some(0.021));
    assert_eq!(it_grade(6, 18.0), Some(0.011), "18 belongs to the 10–18 range");
    assert_eq!(it_grade(11, 500.0), Some(0.400));
    assert_eq!(it_grade(7, 501.0), None);
    for (class, size, want) in [
        ("H7", 25.0, (0.021, 0.0)), ("H7", 16.0, (0.018, 0.0)), ("h6", 25.0, (0.0, -0.013)), ("g6", 25.0, (-0.007, -0.020)),
        ("g6", 50.0, (-0.009, -0.025)), ("f7", 10.0, (-0.013, -0.028)), ("k6", 20.0, (0.015, 0.002)), ("n6", 40.0, (0.033, 0.017)),
        ("p6", 60.0, (0.051, 0.032)), ("G7", 12.0, (0.024, 0.006)), ("F8", 30.0, (0.053, 0.020)), ("JS7", 30.0, (0.0105, -0.0105)), ("k8", 20.0, (0.033, 0.0)),
    ] {
        let got = fit_deviations(class, size).unwrap_or_else(|| panic!("{} {}", class, size));
        assert!(close(got, want), "{} at {}: {:?} expected {:?}", class, size, got, want);
    }
    assert_eq!(fit_deviations("X7", 20.0), None);
    assert_eq!(fit_deviations("H", 20.0), None);
}

#[test]
fn tolerance_labels() {
    let d = DrawingData::new("D", "M", SheetSize::A3, Orientation::Landscape);
    let lab = |mode, upper, lower, fit: &str| tol_label(&d, "20.00".into(), 20.0, "", &Tolerance { mode, upper, lower, fit: fit.into() });
    assert_eq!(lab(TolMode::Nominal, 0.0, 0.0, "").main, "20.00");
    assert_eq!(lab(TolMode::Symmetric, -0.05, 0.0, "").main, "20.00 ±0.05");
    let pm = lab(TolMode::PlusMinus, 0.05, -0.02, "");
    assert_eq!(pm.stack, Some(("+0.05".into(), "-0.02".into())));
    let lim = lab(TolMode::Limits, 0.05, -0.02, "");
    assert_eq!(lim.main, "");
    assert_eq!(lim.stack, Some(("20.050".into(), "19.980".into())));
    assert!(lab(TolMode::Basic, 0.0, 0.0, "").boxed);
    assert_eq!(lab(TolMode::Reference, 0.0, 0.0, "").main, "(20.00)");
    let fit = tol_label(&d, "⌀16.00".into(), 16.0, "⌀", &Tolerance { mode: TolMode::Fit, fit: "H7".into(), ..Default::default() });
    assert_eq!(fit.main, "⌀16.00 H7");
    assert_eq!(fit.stack, Some(("+0.018".into(), "0".into())));
}

fn feat(id: u64, name: &str, kind: FeatureKind) -> Feature { Feature { id, name: name.into(), suppressed: false, hidden: false, kind } }

fn block_drawing() -> App {
    let mut m = ModelData::new_part("BLOCK");
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(TOP_ID));
    e.sketch = Sketch::rect(100.0, 60.0, 0.0, 0.0);
    e.depth = Dim::v(20.0);
    m.features.push(feat(10, "Extrude 1", FeatureKind::Extrude(e)));
    let FeatureKind::Hole(mut h) = FeatureKind::new_hole() else { unreachable!() };
    h.plane = Some(PlaneRef::Face(FaceRef { point: V3::new(0.0, 20.0, 0.0), normal: V3::Y }));
    h.diameter = Dim::v(16.0);
    h.through_all = true;
    m.features.push(feat(11, "Hole 1", FeatureKind::Hole(h)));
    let mut app = App::new(None);
    app.data = m;
    app.regenerate(false);
    app.new_drawing(&NewDrawing { name: "DRW".into(), size: SheetSize::A3, orient: Orientation::Landscape, template: true, projection: Projection::Third });
    let ids: Vec<u32> = app.drw.as_ref().unwrap().data.sheets[0].views.iter().map(|v| v.id).collect();
    app.show_model_dims(&ids);
    app
}

fn texts(app: &mut App) -> Vec<String> { app.sheet_scene(0, &[]).0.iter().filter_map(|p| if let Prim::Text { text, .. } = p { Some(text.clone()) } else { None }).collect() }

fn view(app: &App, i: usize) -> DrawView { app.drw.as_ref().unwrap().data.sheets[0].views[i].clone() }

#[test]
fn gdt_annotations_on_a_drawing() {
    let mut app = block_drawing();
    // tolerance the hole as an H7 fit and the depth plus-minus, through the dimension tab API
    let sheet = app.drw.as_ref().unwrap().data.sheets[0].clone();
    let hole = sheet.shown.iter().find(|s| s.key.ends_with("11:f:diameter")).unwrap().key.clone();
    let depth = sheet.shown.iter().find(|s| s.key.ends_with("10:f:depth")).unwrap().key.clone();
    app.set_dim_tolerance(&DimTarget::Shown(hole.clone()), Tolerance { mode: TolMode::Fit, fit: "H7".into(), ..Default::default() });
    app.drw.as_mut().unwrap().coalesce = false;
    app.set_dim_tolerance(&DimTarget::Shown(depth), Tolerance { mode: TolMode::PlusMinus, upper: 0.1, lower: -0.05, ..Default::default() });
    let t = texts(&mut app);
    for want in ["⌀16.00 DURCHG. H7", "+0.018", "0", "+0.1", "-0.05"] { assert!(t.iter().any(|s| s == want), "{} in {:?}", want, t); }
    // datum A on the bottom edge of the front view, datum B on the left edge of the top view
    let (front, top) = (view(&app, 0), view(&app, 1));
    let gf = app.view_geom(front.id).unwrap();
    let gt = app.view_geom(top.id).unwrap();
    let (fv, fa, _) = app.anchor_at(gf.world(front.pos, V3::new(-20.0, 0.0, 30.0)), 1.0).expect("bottom edge");
    let (tv, tb, _) = app.anchor_at(gt.world(top.pos, V3::new(-50.0, 20.0, 10.0)), 1.0).expect("left edge");
    assert!((fa.point - V3::new(-20.0, 0.0, 30.0)).length() < 1e-6 || (fa.point.y.abs() < 1e-6 && (fa.point.x + 20.0).abs() < 1e-6), "{:?}", fa.point);
    let (sv, sa, _) = app.anchor_at(gf.world(front.pos, V3::new(30.0, 20.0, 30.0)), 1.0).expect("top edge");
    app.drw_commit(|d| {
        let id = d.take_id(); d.sheets[0].datums.push(DatumSym { id, view: fv, letter: "A".into(), at: fa, pos: V2::new(-20.0, -25.0) });
        let id = d.take_id(); let letter = d.sheets[0].next_datum(); d.sheets[0].datums.push(DatumSym { id, view: tv, letter, at: tb, pos: V2::new(-70.0, 0.0) });
        let id = d.take_id(); d.sheets[0].finishes.push(Finish { id, view: sv, kind: FinishKind::Removal, value: "Ra 1.6".into(), method: "MILLED".into(), at: sa, flip: false });
    });
    assert_eq!(app.drw.as_ref().unwrap().data.sheets[0].datums[1].letter, "B");
    // position of the hole relative to A and B, riding on its dimension
    app.drw.as_mut().unwrap().sel = vec![DSel::Shown(sheet.shown.iter().position(|s| s.key == hole).unwrap())];
    let target = app.selected_dim().unwrap();
    app.add_gtol_on_dim(target);
    let g = app.drw.as_ref().unwrap().data.sheets[0].gtols[0].clone();
    assert_eq!((g.ch, g.diameter, g.mat), (GChar::Position, true, MatCond::Mmc));
    assert_eq!(g.datums.iter().map(|d| d.0.as_str()).collect::<Vec<_>>(), vec!["A", "B"]);
    let t = texts(&mut app);
    for want in ["⌀0.1", "M", "A", "B", "Ra 1.6", "MILLED"] { assert!(t.iter().any(|s| s == want), "{} in {:?}", want, t); }
    // the frame sits right below the hole callout
    let (_, hits) = app.sheet_scene(0, &[]);
    let dim_box = hits.iter().find(|h| matches!(h.sel, DSel::Shown(_)) && h.min.x < h.max.x && hits.iter().any(|x| matches!(x.sel, DSel::Gtol(_)) && (x.max.y - h.min.y).abs() < 1.0)).expect("frame under a dimension");
    let frame = hits.iter().find(|h| matches!(h.sel, DSel::Gtol(_))).unwrap();
    assert!(frame.max.y <= dim_box.min.y + 1e-6 && frame.min.x >= dim_box.min.x - 1e-6);
    // annotations stay on their edges when the block grows
    app.commit("taller", |d| if let FeatureKind::Extrude(e) = &mut d.features[4].kind { e.depth = Dim::v(30.0); });
    let gf = app.view_geom(front.id).unwrap();
    let f = app.drw.as_ref().unwrap().data.sheets[0].finishes[0].clone();
    let (p, _, ok) = freo::app::drawing::resolve_edge(&f.at, &gf.model.topo);
    assert!(ok && (p.y - 30.0).abs() < 1e-6, "finish follows the top face: {:?}", p);
    // erasing the dimension removes the frame riding on it
    let i = app.drw.as_ref().unwrap().data.sheets[0].shown.iter().position(|s| s.key == hole).unwrap();
    app.drw.as_mut().unwrap().sel = vec![DSel::Shown(i)];
    app.drw_delete_selection();
    assert!(app.drw.as_ref().unwrap().data.sheets[0].gtols.is_empty());
    // the annotations survive a save / load round trip and export to PDF
    let dir = std::env::temp_dir().join(format!("freo_gdt_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("gdt.pfdrw");
    app.drw.as_mut().unwrap().path = Some(path.clone());
    app.save_drawing(false);
    let mut b = App::new(None);
    b.load(path);
    let s = &b.drw.as_ref().unwrap().data.sheets[0];
    assert_eq!((s.datums.len(), s.finishes.len()), (2, 1));
    assert!(s.shown.iter().any(|x| x.tol.mode == TolMode::PlusMinus));
    let prims = b.sheet_scene(0, &[]).0;
    let pdf = freo::app::publish::pdf(&[(420.0, 297.0, prims)], "GDT");
    assert!(pdf.windows(8).any(|w| w == b"(Ra 1.6)"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn feature_control_frame_layout() {
    let d = DrawingData::new("D", "M", SheetSize::A3, Orientation::Landscape);
    let g = Gtol { id: 1, view: 1, ch: GChar::Position, diameter: true, value: 0.05, mat: MatCond::Mmc, datums: vec![("A".into(), MatCond::Rfs), ("B".into(), MatCond::Mmc), ("C".into(), MatCond::Rfs)], target: Attach::Dim(DimTarget::Drawn(1)), pos: V2::ZERO };
    let mut prims = vec![];
    let (lo, hi) = freo::app::drawing::gtol_frame(&mut prims, V2::new(10.0, 10.0), &g, &d, 3.5, [0, 0, 0]);
    assert!((hi.y - lo.y - 7.0).abs() < 1e-9, "frame is two text heights tall");
    // symbol, tolerance and three datum cells: four separators
    let verticals = prims.iter().filter(|p| matches!(p, Prim::Line { pts, .. } if pts.len() == 2 && (pts[0].x - pts[1].x).abs() < 1e-9 && (pts[0].y - pts[1].y).abs() > 6.9)).count();
    assert_eq!(verticals, 4);
    let t: Vec<&str> = prims.iter().filter_map(|p| if let Prim::Text { text, .. } = p { Some(text.as_str()) } else { None }).collect();
    assert_eq!(t.iter().filter(|s| **s == "M").count(), 2, "Ⓜ on the zone and on datum B");
    assert!(t.contains(&"⌀0.05"));
    // form tolerances drop their datums
    let flat = Gtol { ch: GChar::Flatness, diameter: false, ..g };
    let mut p2 = vec![];
    let (lo2, hi2) = freo::app::drawing::gtol_frame(&mut p2, V2::ZERO, &flat, &d, 3.5, [0, 0, 0]);
    assert!(hi2.x - lo2.x < hi.x - lo.x - 15.0);
}
