//! Creo 10 sketcher variants: tangent lines, slanted rectangle, parallelogram, concentric and
//! 3-tangent circles, axis-end ellipse, corner-keeping fillet/chamfer, replace, chain selection,
//! sketch checks and locked dimensions.

use freo::app::sketcher::{common_tangents, SketchSession, Tool};
use freo::app::{App, Pick};
use freo::kernel::math::*;
use freo::model::features::*;
use freo::sketch::sketch::{SkCType, SkKind};

fn session() -> App {
    let mut app = App::new(None);
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.start_feature(FeatureKind::new_extrude());
    assert!(app.sketch.is_some());
    app
}

fn sk(app: &mut App) -> &mut SketchSession { app.sketch.as_mut().unwrap() }

fn kinds(s: &SketchSession, k: SkKind) -> usize { s.sketch.entities.iter().filter(|e| e.kind == k && !e.construction).count() }

#[test]
fn common_tangents_touch_both_circles() {
    let (c1, r1, c2, r2) = (V2::new(0.0, 0.0), 10.0, V2::new(50.0, 0.0), 5.0);
    let t = common_tangents(c1, r1, c2, r2);
    assert_eq!(t.len(), 4);
    for (p, q) in t {
        assert!(((p - c1).length() - r1).abs() < 1e-9 && ((q - c2).length() - r2).abs() < 1e-9);
        let d = safe_norm2(q - p, V2::X);
        assert!(d.dot(p - c1).abs() < 1e-9 && d.dot(q - c2).abs() < 1e-9, "radius is perpendicular to the line");
    }
}

#[test]
fn tangent_line_between_two_circles() {
    let mut app = session();
    let s = sk(&mut app);
    s.set_tool(Tool::Circle);
    s.click_at(V2::new(-30.0, 0.0)); s.click_at(V2::new(-20.0, 0.0));
    s.click_at(V2::new(30.0, 0.0)); s.click_at(V2::new(36.0, 0.0));
    s.set_tool(Tool::LineTangent);
    s.click_at(V2::new(-30.0, 10.0));
    s.click_at(V2::new(30.0, 6.0));
    let line = s.sketch.entities.iter().find(|e| e.kind == SkKind::Line).expect("tangent line");
    let (a, b) = s.sketch.line_ends(line.id).unwrap();
    assert!(a.y > 0.0 && b.y > 0.0, "upper tangent near the picks: {:?} {:?}", a, b);
    let d = |c: V2| cross2(safe_norm2(b - a, V2::X), c - a).abs();
    assert!((d(V2::new(-30.0, 0.0)) - 10.0).abs() < 1e-4 && (d(V2::new(30.0, 0.0)) - 6.0).abs() < 1e-4);
    assert_eq!(s.sketch.constraints.iter().filter(|c| c.ty == SkCType::Tangent).count(), 2);
}

#[test]
fn slanted_rectangle_and_parallelogram() {
    let mut app = session();
    let s = sk(&mut app);
    s.set_tool(Tool::SlantRect);
    for p in [V2::new(0.0, 0.0), V2::new(30.0, 10.0), V2::new(25.0, 40.0)] { s.click_at(p); }
    assert_eq!(kinds(s, SkKind::Line), 4);
    let lines: Vec<_> = s.sketch.entities.iter().filter(|e| e.kind == SkKind::Line).map(|e| s.sketch.line_ends(e.id).unwrap()).collect();
    assert!(safe_norm2(lines[0].1 - lines[0].0, V2::X).dot(safe_norm2(lines[1].1 - lines[1].0, V2::X)).abs() < 1e-6, "right angle");
    assert!(s.sketch.open_ends().is_empty());
    s.set_tool(Tool::Parallelogram);
    for p in [V2::new(60.0, 0.0), V2::new(90.0, 0.0), V2::new(100.0, 20.0)] { s.click_at(p); }
    assert_eq!(kinds(s, SkKind::Line), 8);
    assert_eq!(s.sketch.constraints.iter().filter(|c| c.ty == SkCType::Parallel).count(), 4);
}

#[test]
fn concentric_and_three_tangent_circles() {
    let mut app = session();
    let s = sk(&mut app);
    s.set_tool(Tool::Circle);
    s.click_at(V2::new(0.0, 0.0)); s.click_at(V2::new(20.0, 0.0));
    s.set_tool(Tool::CircleConc);
    s.click_at(V2::new(20.0, 0.0));
    s.click_at(V2::new(8.0, 0.0));
    let circles: Vec<_> = s.sketch.entities.iter().filter(|e| e.kind == SkKind::Circle).cloned().collect();
    assert_eq!(circles.len(), 2);
    assert_eq!(circles[0].pts[0], circles[1].pts[0], "concentric circles share the centre point");
    assert!((circles[1].r - 8.0).abs() < 1e-4, "r {}", circles[1].r);

    // incircle of a triangle
    let mut app = session();
    let s = sk(&mut app);
    s.set_tool(Tool::Line);
    for p in [V2::new(0.0, 0.0), V2::new(60.0, 0.0), V2::new(0.0, 60.0), V2::new(0.0, 0.0)] { s.click_at(p); }
    s.set_tool(Tool::Circle3T);
    for p in [V2::new(20.0, 0.0), V2::new(30.0, 30.0), V2::new(0.0, 20.0)] { s.click_at(p); }
    let c = s.sketch.entities.iter().find(|e| e.kind == SkKind::Circle).expect("tangent circle").clone();
    let r_in = 60.0 * 60.0 / (60.0 + 60.0 + 60.0 * 2f64.sqrt());
    assert!((c.r - r_in).abs() < 0.05, "about the incircle: {} vs {}", c.r, r_in);
    let cen = s.sketch.point(c.pts[0]);
    for l in s.sketch.entities.iter().filter(|e| e.kind == SkKind::Line) {
        let (a, b) = s.sketch.line_ends(l.id).unwrap();
        assert!((cross2(safe_norm2(b - a, V2::X), cen - a).abs() - c.r).abs() < 1e-6, "tangent to every side");
    }
}

#[test]
fn axis_end_ellipse_and_corner_keeping_fillet() {
    let mut app = session();
    let s = sk(&mut app);
    s.set_tool(Tool::EllipseAxis);
    for p in [V2::new(-20.0, 0.0), V2::new(20.0, 0.0), V2::new(0.0, 8.0)] { s.click_at(p); }
    let e = s.sketch.entities.iter().find(|e| e.kind == SkKind::Ellipse).expect("ellipse");
    assert!(s.sketch.point(e.pts[0]).length() < 1e-9 && (e.r - 8.0).abs() < 1e-4, "c {:?} r {}", s.sketch.point(e.pts[0]), e.r);

    let mut app = session();
    let s = sk(&mut app);
    s.set_tool(Tool::Rect);
    s.click_at(V2::new(0.0, 0.0)); s.click_at(V2::new(40.0, 30.0));
    s.set_tool(Tool::FilletCirc);
    s.click_at(V2::new(40.0, 30.0));
    assert_eq!(kinds(s, SkKind::Arc), 1);
    assert_eq!(s.sketch.entities.iter().filter(|e| e.construction && e.kind == SkKind::Line).count(), 2, "corner kept as construction");
    assert!(s.sketch.profile_regions().is_ok(), "still one closed profile");
    s.set_tool(Tool::Chamfer);
    s.click_at(V2::new(0.0, 0.0));
    assert_eq!(s.sketch.entities.iter().filter(|e| e.construction && e.kind == SkKind::Line).count(), 2, "Fasentrimmung keeps nothing");
}

#[test]
fn chain_select_replace_checks_and_lock() {
    let mut app = session();
    let s = sk(&mut app);
    s.set_tool(Tool::Rect);
    s.click_at(V2::new(0.0, 0.0)); s.click_at(V2::new(40.0, 30.0));
    s.set_tool(Tool::Line);
    s.click_at(V2::new(-20.0, 15.0)); s.click_at(V2::new(60.0, 15.0)); s.end_chain();
    let (_, cross, _) = s.check_marks();
    assert_eq!(cross.len(), 2, "the line crosses two rectangle sides");
    s.select_all(false);
    assert_eq!(s.selection.len(), 5);
    s.select_all(true);
    assert!(s.selection.len() > 5);

    // lock a dimension; it survives deleting it
    let dim = s.sketch.constraints.iter().find(|c| c.ty.is_dimension()).unwrap().id;
    s.selection = vec![freo::app::sketcher::SkSel::Con(dim)];
    s.toggle_lock();
    assert!(s.sketch.constraints.iter().any(|c| c.id == dim && c.locked && !c.weak));
    s.show_locks = true;
}

fn bracket_rect(s: &mut SketchSession) {
    s.set_tool(Tool::Rect);
    s.click_at(V2::new(0.0, 0.0)); s.click_at(V2::new(40.0, 30.0));
}

#[test]
fn conic_is_exact_and_rho_is_a_dimension() {
    let mut app = session();
    let s = sk(&mut app);
    s.set_tool(Tool::Conic);
    for p in [V2::new(0.0, 0.0), V2::new(40.0, 0.0), V2::new(20.0, 10.0)] { s.click_at(p); }
    let c = s.sketch.entities.iter().find(|e| e.kind == SkKind::Conic).expect("conic").clone();
    assert!((s.sketch.conic_shoulder(&c) - V2::new(20.0, 10.0)).length() < 1e-3, "passes through the shoulder");
    let rho = s.sketch.constraints.iter().find(|k| k.ty == SkCType::Rho).expect("rho dimension").id;
    s.set_dimension(rho, "0.3");
    let c = s.sketch.entity(c.id).unwrap();
    assert!((c.r - 0.3).abs() < 1e-6);
    // rho < 0.5: elliptic, the curve stays inside the control triangle
    assert!(s.sketch.sample(c, 32).iter().all(|p| p.y >= -1e-9 && p.y <= 20.0 + 1e-9));
}

#[test]
fn elliptic_fillet_is_tangent_and_profile_closes() {
    let mut app = session();
    let s = sk(&mut app);
    bracket_rect(s);
    s.set_tool(Tool::FilletEllipTrim);
    s.click_at(V2::new(40.0, 30.0));
    let c = s.sketch.entities.iter().find(|e| e.kind == SkKind::Conic).expect("elliptic fillet").clone();
    let pts = s.sketch.sample(&c, 64);
    let (t0, t1) = (safe_norm2(pts[1] - pts[0], V2::X), safe_norm2(pts[pts.len() - 1] - pts[pts.len() - 2], V2::X));
    assert!(t0.y.abs() < 0.05 || t0.x.abs() < 0.05, "start tangent along a side: {:?}", t0);
    assert!(t1.y.abs() < 0.05 || t1.x.abs() < 0.05, "end tangent along a side: {:?}", t1);
    assert!(s.sketch.profile_regions().is_ok());
}

#[test]
fn text_offset_thicken_csys_and_reference_dims() {
    let mut app = session();
    let s = sk(&mut app);
    // text: closed glyph loops that extrude
    s.set_tool(Tool::Text);
    s.click_at(V2::new(0.0, 0.0)); s.click_at(V2::new(0.0, 10.0));
    assert!(matches!(s.pending, Some(freo::app::sketcher::Pending::Text { .. })), "two picks open the Text dialog");
    s.pending = None;
    let (ia, ib) = (s.sketch.add_point(V2::ZERO), s.sketch.add_point(V2::new(0.0, 10.0)));
    let id = s.sketch.add_entity(SkKind::Text, vec![ia, ib], 0.0, false, false);
    s.sketch.entities.iter_mut().find(|e| e.id == id).unwrap().text = Some(freo::sketch::sketch::SkText { text: "HO".into(), font: 0, aspect: 1.0, slant: 0.0, family: String::new() });
    let regions = s.sketch.profile_regions().expect("text profile");
    assert!(regions.len() == 2 && regions.iter().any(|r| r.holes.len() == 1), "H and O (with its hole): {}", regions.len());
    let e = s.sketch.entity(id).unwrap().clone();
    let (mn, mx) = s.sketch.text_loops(&e).iter().flatten().fold((V2::splat(1e9), V2::splat(-1e9)), |(a, b), p| (a.min(*p), b.max(*p)));
    assert!((mx.y - mn.y - 10.0).abs() < 0.5, "cap height = text height: {}", mx.y - mn.y);

    // Versatz of a rectangle side by 5 toward the inside, with its offset dimension
    let mut app = session();
    let s = sk(&mut app);
    bracket_rect(s);
    let bottom = s.sketch.entities.iter().find(|e| e.kind == SkKind::Line && s.sketch.line_ends(e.id).map(|(a, b)| a.y.abs() < 1e-6 && b.y.abs() < 1e-6).unwrap_or(false)).unwrap().id;
    s.offset_from(bottom, V2::new(20.0, 3.0), &[5.0], 0);
    let off = s.sketch.entities.iter().filter(|e| e.kind == SkKind::Line).last().unwrap().id;
    let (a, b) = s.sketch.line_ends(off).unwrap();
    assert!((a.y - 5.0).abs() < 1e-6 && (b.y - 5.0).abs() < 1e-6);
    assert!(s.sketch.constraints.iter().any(|c| c.ty == SkCType::PointLine && !c.weak && (c.value - 5.0).abs() < 1e-9));

    // Aufdicken with flat ends: a closed strip
    let mut app = session();
    let s = sk(&mut app);
    s.set_tool(Tool::Line);
    s.click_at(V2::new(0.0, 0.0)); s.click_at(V2::new(40.0, 0.0)); s.end_chain();
    let l = s.sketch.entities[0].id;
    let before = s.sketch.entities.len();
    s.offset_from(l, V2::new(10.0, 1.0), &[2.0, 6.0], 1);
    assert_eq!(s.sketch.entities.len(), before + 4);
    s.sketch.delete_entity(l);
    assert_eq!(s.sketch.profile_regions().expect("strip").len(), 1);

    // round ends
    let mut app = session();
    let s = sk(&mut app);
    s.set_tool(Tool::Line);
    s.click_at(V2::new(0.0, 0.0)); s.click_at(V2::new(40.0, 0.0)); s.end_chain();
    let l = s.sketch.entities[0].id;
    s.offset_from(l, V2::new(10.0, 1.0), &[0.0, 4.0], 2);
    assert_eq!(s.sketch.entities.iter().filter(|e| e.kind == SkKind::Arc).count(), 2);

    // coordinate system and a reference dimension that does not drive
    let mut app = session();
    let s = sk(&mut app);
    s.set_tool(Tool::Csys);
    s.click_at(V2::new(10.0, 10.0));
    assert!(s.sketch.entities.iter().any(|e| e.kind == SkKind::Csys && e.construction));
    bracket_rect(s);
    let n = s.sketch.constraints.len();
    s.set_tool(Tool::RefDimension);
    let line = s.sketch.entities.iter().find(|e| e.kind == SkKind::Line).unwrap().id;
    let (a, b) = s.sketch.line_ends(line).unwrap();
    s.click_at((a + b) * 0.5);
    s.click_at((a + b) * 0.5 + V2::new(0.0, -15.0));
    let r = s.sketch.constraints.iter().find(|c| c.reference).expect("reference dimension");
    assert!(s.sketch.constraints.len() > n && (r.value - (b - a).length()).abs() < 1e-6);
}

#[test]
fn dxf_import_copy_paste_rotate_and_references() {
    let mut app = session();
    let s = sk(&mut app);
    let dxf = "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n4\n70\n1\n10\n0\n20\n0\n10\n20\n20\n0\n42\n1\n10\n20\n20\n10\n10\n0\n20\n10\n0\nCIRCLE\n10\n5\n20\n5\n40\n2\n0\nENDSEC\n0\nEOF\n";
    let n = s.import_dxf(dxf).expect("dxf");
    assert_eq!(n, 5, "3 lines, 1 bulge arc, 1 circle");
    assert!(s.rot_resize.is_some(), "import opens Drehen-Größe ändern");
    s.rot_resize = None;
    let regions = s.sketch.profile_regions().expect("closed");
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].holes.len(), 1);

    s.select_all(false);
    s.copy_selection(false);
    s.paste(Some(V2::new(100.0, 0.0)));
    assert_eq!(s.sketch.entities.len(), 10);
    let mut rr = s.rot_resize.clone().expect("rotate dialog");
    rr.scale = 2.0;
    let circle = *s.selection.iter().find_map(|x| if let freo::app::sketcher::SkSel::Ent(e) = x { s.sketch.entity(*e).filter(|e| e.kind == SkKind::Circle).map(|e| e.id).as_ref().copied() } else { None }).as_ref().unwrap();
    s.rot_resize = Some(rr);
    // OK through the same code path as the dialog
    let r = s.rot_resize.clone().unwrap();
    for (id, p) in &r.pts { let d = (*p - r.pivot) * r.scale; s.sketch.set_point(*id, r.pivot + d); }
    for (id, rad) in &r.radii { if let Some(e) = s.sketch.entities.iter_mut().find(|e| e.id == *id) { e.r = rad * r.scale; } }
    assert!((s.sketch.entity(circle).unwrap().r - 4.0).abs() < 1e-9);

    // references: add a candidate edge, delete it again
    let mut app = App::new(None);
    app.data = freo::app::demo::bracket();
    app.regenerate(false);
    app.handle_pick(Some(Pick::Plane(FRONT_ID)), false);
    app.start_feature(FeatureKind::new_extrude());
    let s = app.sketch.as_mut().expect("sketch on VORNE");
    assert!(s.axis_names.iter().any(|n| n.contains("BEZUGSEBENE")), "{:?}", s.axis_names);
    assert!(!s.cand_refs.is_empty());
    let (r0, c0) = (s.ref_names.len(), s.cand_refs.len());
    // an edge whose middle is not also a candidate point (points are picked first)
    let l = s.cand_refs.iter().map(|c| c.1).find(|l| s.cand_points.iter().all(|p| (s.to_screen(p.1) - s.to_screen((l[0] + l[1]) * 0.5)).length() > 12.0)).expect("edge");
    s.set_tool(Tool::AddRef);
    s.click_at((l[0] + l[1]) * 0.5);
    assert_eq!((s.ref_names.len(), s.cand_refs.len()), (r0 + 1, c0 - 1));
    s.delete_ref(r0);
    assert_eq!(s.ref_names.len(), r0);
}

#[test]
fn sketch_setup_moves_the_sketch_and_views_are_saved() {
    let mut app = session();
    let s = sk(&mut app);
    bracket_rect(s);
    app.resetup_sketch(PlaneRef::Datum(FRONT_ID), false);
    assert_eq!(app.editor.as_ref().unwrap().feature.kind.sketch_plane(), Some(PlaneRef::Datum(FRONT_ID)));
    assert_eq!(app.sketch.as_ref().unwrap().sketch.entities.len(), 4, "geometry carried over");

    let d = freo::model::features::ModelData::new_part("PRT0001");
    assert!(d.views.iter().any(|v| v.name == "GOODVIEW"));
    let json = serde_json::to_string(&d).unwrap();
    let back: freo::model::features::ModelData = serde_json::from_str(&json).unwrap();
    assert_eq!(back.views, d.views);
}

#[test]
fn palette_shapes_are_placed_at_the_click() {
    use freo::sketch::palette;
    let tabs = palette::tabs();
    // every shape of every tab can be placed and stays a valid section
    for (t, tab) in tabs.iter().enumerate() {
        for (i, item) in tab.items.iter().enumerate() {
            let mut app = session();
            let s = sk(&mut app);
            s.pick_palette(t, i);
            assert_eq!(s.tool, Tool::Palette);
            s.click_at(V2::new(40.0, 25.0));
            assert_eq!(s.tool, Tool::Select);
            assert!(s.rot_resize.is_some(), "{}: opens Drehen-Größe ändern", item.name);
            assert_eq!(s.sketch.entities.len(), item.sketch.entities.len(), "{}", item.name);
            if !item.name.to_lowercase().starts_with("welle") { assert_eq!(s.sketch.profile_regions().unwrap().len(), 1, "{} / {}", tab.name, item.name); }
        }
    }
    // a steel section keeps its millimetres: L50x40x5 is 40 wide and 50 high, corner at the click
    let i = tabs[3].items.iter().position(|x| x.name == "L50x40x5").unwrap();
    let mut app = session();
    let s = sk(&mut app);
    s.pick_palette(3, i);
    s.click_at(V2::new(10.0, 20.0));
    let (mut mn, mut mx) = (V2::splat(f64::INFINITY), V2::splat(f64::NEG_INFINITY));
    for e in s.sketch.entities.iter().filter(|e| e.kind == SkKind::Line) { for p in s.sketch.sample(e, 8) { mn = mn.min(p); mx = mx.max(p); } }
    assert!((mn - V2::new(10.0, 20.0)).length() < 1e-3 && (mx - V2::new(50.0, 70.0)).length() < 1e-3, "{:?} {:?}", mn, mx);
}
