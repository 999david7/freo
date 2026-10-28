//! Drawings: hidden-line views, projection alignment, shown and created dimensions, BOM
//! tables and balloons, and PDF / SVG / DXF output.

use freo::kernel::hlr;
use freo::kernel::math::*;
use freo::model::features::*;
use freo::model::regen;
use freo::sketch::sketch::Sketch;

fn feat(id: u64, name: &str, kind: FeatureKind) -> Feature { Feature { id, name: name.into(), suppressed: false, hidden: false, kind } }

/// 100 x 60 x 20 block with a Ø16 vertical through hole at the centre.
fn block_with_hole() -> ModelData {
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
    m
}

fn total_len(lines: &[Vec<V2>]) -> f64 { lines.iter().map(|l| l.windows(2).map(|w| (w[1] - w[0]).length()).sum::<f64>()).sum() }

fn axes(r: &regen::RegenResult) -> Vec<(V3, V3)> { r.axes.iter().map(|a| (a.point, a.dir)).collect() }

#[test]
fn front_view_hides_the_hole() {
    let r = regen::run(&block_with_hole());
    // FRONT: looking along -Z, x right, y up
    let v = hlr::project(&r.topo, &axes(&r), glam::DQuat::IDENTITY);
    assert!((v.max.x - v.min.x - 100.0).abs() < 1e-6 && (v.max.y - v.min.y - 20.0).abs() < 1e-6);
    // the outline is the 100 x 20 rectangle
    let outline = total_len(&v.visible);
    assert!((outline - 240.0).abs() < 1e-3, "visible length {}", outline);
    // the hole shows as two hidden silhouette lines, 20 long each, at x = ±8
    let hidden = total_len(&v.hidden);
    assert!((hidden - 40.0).abs() < 0.5, "hidden length {}", hidden);
    for l in &v.hidden { for p in l { assert!((p.x.abs() - 8.0).abs() < 0.3, "{:?}", p); } }
    // and its axis is a centre line spanning the block height
    assert_eq!(v.axes.len(), 1);
    assert!(((v.axes[0][0] - v.axes[0][1]).length() - 20.0).abs() < 1e-6);
}

#[test]
fn top_view_shows_the_hole_as_a_circle() {
    let r = regen::run(&block_with_hole());
    // TOP: the camera rotation of Creo's TOP orientation is Rx(-90°); world → view is its inverse
    let rot = glam::DQuat::from_axis_angle(V3::X, -std::f64::consts::FRAC_PI_2).inverse();
    let v = hlr::project(&r.topo, &axes(&r), rot);
    assert!((v.max.x - v.min.x - 100.0).abs() < 1e-6 && (v.max.y - v.min.y - 60.0).abs() < 1e-6);
    assert!(v.hidden.is_empty() || total_len(&v.hidden) < 1e-6, "nothing hidden from above: {}", total_len(&v.hidden));
    assert_eq!(v.circles.len(), 1);
    assert!((v.circles[0].1 - 8.0).abs() < 0.1 && v.circles[0].0.length() < 1e-6);
    let outline = total_len(&v.visible);
    assert!((outline - (320.0 + std::f64::consts::TAU * 8.0)).abs() < 1.0, "visible {}", outline);
}

// ---------------------------------------------------------------- drawings through the app

use freo::app::drawing::{DSel, Prim, SnapKind};
use freo::app::drawui::NewDrawing;
use freo::app::App;
use freo::model::drawing::*;

fn part_drawing(m: ModelData, proj: Projection) -> App {
    let mut app = App::new(None);
    app.data = m;
    app.regenerate(false);
    app.new_drawing(&NewDrawing { name: "DRW0001".into(), size: SheetSize::A3, orient: Orientation::Landscape, template: true, projection: proj });
    app
}

fn views(app: &App) -> Vec<DrawView> { app.drw.as_ref().unwrap().data.sheets[0].views.clone() }

fn texts(prims: &[Prim]) -> Vec<String> { prims.iter().filter_map(|p| if let Prim::Text { text, .. } = p { Some(text.clone()) } else { None }).collect() }

fn quat_eq(a: glam::DQuat, b: glam::DQuat) -> bool { a.dot(b).abs() > 1.0 - 1e-9 }

#[test]
fn template_views_are_aligned_projections() {
    let mut app = part_drawing(block_with_hole(), Projection::Third);
    let vs = views(&app);
    assert_eq!(vs.len(), 4);
    let (front, top, right) = (&vs[0], &vs[1], &vs[2]);
    assert!(quat_eq(top.rot, Orient::Top.rotation()) && quat_eq(right.rot, Orient::Right.rotation()), "third angle: top above, right to the right");
    assert!(top.pos.y > front.pos.y && (top.pos.x - front.pos.x).abs() < 1e-9);
    assert!(right.pos.x > front.pos.x && (right.pos.y - front.pos.y).abs() < 1e-9);
    let s = app.drw.as_ref().unwrap().data.sheets[0].scale;
    assert_eq!(s, 2.0, "a 100 x 60 x 20 block fits A3 at 2:1 (largest standard scale)");
    // projected edges line up across views
    let (gf, gt, gr) = (app.view_geom(front.id).unwrap(), app.view_geom(top.id).unwrap(), app.view_geom(right.id).unwrap());
    let (rf, rt, rr) = (gf.rect(front.pos), gt.rect(top.pos), gr.rect(right.pos));
    assert!((rf.1.y - rr.1.y).abs() < 1e-9 && (rf.0.y - rr.0.y).abs() < 1e-9, "front and right share heights");
    assert!((rf.0.x - rt.0.x).abs() < 1e-9 && (rf.1.x - rt.1.x).abs() < 1e-9, "front and top share widths");
    assert!(((rr.1.x - rr.0.x) - 60.0 * s).abs() < 1e-6 && ((rt.1.y - rt.0.y) - 60.0 * s).abs() < 1e-6, "right and top views show the 60 depth");
    // moving the front view drags its projections; the right view only slides sideways
    app.move_view(front.id, V2::new(5.0, 7.0));
    let vs2 = views(&app);
    assert!((vs2[2].pos - right.pos - V2::new(5.0, 7.0)).length() < 1e-9);
    app.move_view(right.id, V2::new(10.0, 30.0));
    let vs3 = views(&app);
    assert!((vs3[2].pos.y - vs2[2].pos.y).abs() < 1e-9 && (vs3[2].pos.x - vs2[2].pos.x - 10.0).abs() < 1e-9);
}

#[test]
fn first_angle_places_the_top_view_below() {
    let app = part_drawing(block_with_hole(), Projection::First);
    let vs = views(&app);
    let (front, top, side) = (&vs[0], &vs[1], &vs[2]);
    assert!(top.pos.y < front.pos.y, "seen from above, drawn below");
    assert!(quat_eq(top.rot, Orient::Top.rotation()));
    assert!(side.pos.x < front.pos.x && quat_eq(side.rot, Orient::Right.rotation()), "seen from the right, drawn on the left");
}

#[test]
fn shown_dimensions_drive_and_follow_the_model() {
    let mut app = part_drawing(block_with_hole(), Projection::Third);
    let ids: Vec<u32> = views(&app).iter().map(|v| v.id).collect();
    let n = app.show_model_dims(&ids);
    assert!(n >= 4, "{} dimensions", n);
    let dims = app.model_dims();
    let sheet = app.drw.as_ref().unwrap().data.sheets[0].clone();
    let top_id = views(&app)[1].id;
    let find = |pred: &dyn Fn(&freo::app::drawing::ModelDim) -> bool| sheet.shown.iter().find(|s| dims.iter().any(|m| m.key == s.key && pred(m))).cloned();
    // the 100 wide sketch dimension sits in the top view (sketch plane parallel)
    let w = find(&|m| (m.value - 100.0).abs() < 1e-9 && m.key.contains(":s0:")).expect("width");
    assert_eq!(w.view, top_id);
    // the hole is called out in the view where it is a circle
    let hole = find(&|m| m.key.ends_with(":f:diameter")).expect("hole");
    assert_eq!(hole.view, top_id);
    let (prims, _) = app.sheet_scene(0, &[]);
    let t = texts(&prims);
    assert!(t.iter().any(|s| s == "⌀16.00 DURCHG."), "{:?}", t);
    assert!(t.iter().any(|s| s == "20.00"), "depth: {:?}", t);
    // nothing is shown twice
    let mut keys: Vec<&String> = sheet.shown.iter().map(|s| &s.key).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), sheet.shown.len());
    // modifying the depth in the drawing regenerates the part and the views
    let di = sheet.shown.iter().position(|s| s.key.ends_with("10:f:depth")).expect("depth shown");
    app.commit_edit(DSel::Shown(di), "35".into());
    assert!(app.result.errors.is_empty(), "{:?}", app.result.errors);
    let right = views(&app)[2].clone();
    let g = app.view_geom(right.id).unwrap();
    assert!(((g.lines.max.y - g.lines.min.y) - 35.0).abs() < 1e-6, "right view height follows: {:?}", g.lines.max - g.lines.min);
    let t = texts(&app.sheet_scene(0, &[]).0);
    assert!(t.iter().any(|s| s == "35.00") && !t.iter().any(|s| s == "20.00"), "{:?}", t);
    // undo in the model brings the old depth back to the drawing
    app.do_undo();
    let t = texts(&app.sheet_scene(0, &[]).0);
    assert!(t.iter().any(|s| s == "20.00"));
}

#[test]
fn created_dimensions_measure_and_follow_edits() {
    let mut app = part_drawing(block_with_hole(), Projection::Third);
    let front = views(&app)[0].clone();
    let g = app.view_geom(front.id).unwrap();
    // bottom corners of the front view
    let (pa, pb) = (g.world(front.pos, V3::new(-50.0, 0.0, 30.0)), g.world(front.pos, V3::new(50.0, 0.0, 30.0)));
    let sa = app.snap_at(pa, 1.0).expect("corner a");
    let sb = app.snap_at(pb, 1.0).expect("corner b");
    assert_eq!(sa.kind, SnapKind::Point);
    app.place_dimension(sa, Some(sb), (pa + pb) * 0.5 - V2::new(0.0, 15.0));
    let dd = app.drw.as_ref().unwrap().data.sheets[0].dims[0].clone();
    assert_eq!(dd.ty, DimType::Horizontal);
    assert!(texts(&app.sheet_scene(0, &[]).0).iter().any(|s| s == "100.00"));
    // a diameter on the hole circle in the top view
    let top = views(&app)[1].clone();
    let gt = app.view_geom(top.id).unwrap();
    let on_circle = gt.world(top.pos, V3::new(8.0, 20.0, 0.0));
    let sc = app.snap_at(on_circle, 1.0).expect("circle");
    assert!(matches!(sc.kind, SnapKind::Circle { .. }), "{:?}", sc.kind);
    app.place_dimension(sc, None, on_circle + V2::new(15.0, 15.0));
    assert!(texts(&app.sheet_scene(0, &[]).0).iter().any(|s| s == "⌀16.00"));
    // widen the block: the created dimension re-attaches to the moved corners
    app.commit("wider", |d| if let FeatureKind::Extrude(e) = &mut d.features[4].kind { e.sketch = Sketch::rect(140.0, 60.0, 0.0, 0.0); });
    let t = texts(&app.sheet_scene(0, &[]).0);
    assert!(t.iter().any(|s| s == "140.00"), "{:?}", t);
    // text override keeps the measured value
    let id = dd.id;
    app.commit_edit(DSel::Dim(id), "@D TYP".into());
    assert!(texts(&app.sheet_scene(0, &[]).0).iter().any(|s| s == "140.00 TYP"));
}

#[test]
fn section_and_detail_views() {
    let mut app = part_drawing(block_with_hole(), Projection::Third);
    let vs = views(&app);
    let (front, right) = (vs[0].clone(), vs[2].clone());
    let plain = app.view_geom(right.id).unwrap();
    assert!(plain.hatch.is_empty() && !plain.lines.hidden.is_empty(), "unsectioned: the hole is hidden");
    {
        let d = app.drw.as_mut().unwrap();
        let v = d.data.sheets[0].view_mut(right.id).unwrap();
        v.section = Some(RIGHT_ID);
        v.letter = "A".into();
    }
    let cut = app.view_geom(right.id).unwrap();
    assert!(!cut.hatch.is_empty(), "cut faces are hatched");
    assert!(cut.lines.hidden.iter().map(|l| l.len()).sum::<usize>() < plain.lines.hidden.iter().map(|l| l.len()).sum::<usize>(), "the section exposes the hole");
    // the hatching stays inside the cut material (100 x 20 minus the Ø16 hole, centred on RIGHT)
    for s in &cut.hatch { for p in s { assert!(p.x.abs() <= 30.0 + 1e-6 && p.y >= -1e-6 && p.y <= 20.0 + 1e-6, "{:?}", p); assert!(p.x.abs() >= 8.0 - 0.3, "no hatch in the hole: {:?}", p); } }
    let t = texts(&app.sheet_scene(0, &[]).0);
    assert!(t.iter().any(|s| s == "SCHNITT A-A"));
    // two zone markers carry the letter A too
    assert_eq!(t.iter().filter(|s| *s == "A").count(), 2 + 2, "cutting-plane letters in the parent view");
    // detail of the front view's left corner at twice the scale
    {
        let d = app.drw.as_mut().unwrap();
        let id = d.data.take_id();
        let mut v = DrawView::new(id, ViewKind::Detail, front.rot, V2::new(330.0, 200.0));
        v.parent = Some(front.id);
        v.detail = Some(Detail { center: V2::new(-50.0, 0.0), radius: 10.0 });
        v.scale = Some(2.0);
        v.letter = "B".into();
        d.data.sheets[0].views.push(v);
    }
    let did = views(&app).last().unwrap().id;
    let gd = app.view_geom(did).unwrap();
    for l in gd.lines.visible.iter().chain(gd.lines.hidden.iter()) { for p in l { assert!((*p - V2::new(-50.0, 0.0)).length() <= 10.0 + 1e-6); } }
    let (lo, hi) = gd.rect(V2::new(330.0, 200.0));
    assert!(((hi.x - lo.x) - 40.0).abs() < 1e-9, "20 mm across at 2:1");
    let t = texts(&app.sheet_scene(0, &[]).0);
    assert!(t.iter().any(|s| s == "EINZELHEIT B") && t.iter().any(|s| s == "MASSSTAB 2:1"));
}

#[test]
fn assembly_drawing_bom_and_balloons() {
    let mut app = App::new(None);
    let data = freo::app::demo::sample_assembly();
    app.data = data.skeleton.clone();
    app.asm = Some(freo::app::assembly::AsmSession::new(data));
    app.asm_regenerate(false);
    app.new_drawing(&NewDrawing { name: "ASM0001".into(), size: SheetSize::A3, orient: Orientation::Landscape, template: true, projection: Projection::Third });
    assert!(app.drw.as_ref().unwrap().assembly);
    let rows = app.bom_rows();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows.iter().find(|r| r.1 == "BOLT_M10").unwrap().2, 2);
    assert!(app.insert_bom());
    let iso = views(&app)[3].clone();
    let n = app.create_balloons(iso.id);
    assert_eq!(n, 4, "one balloon per BOM item");
    let g = app.view_geom(iso.id).unwrap();
    let (lo, hi) = g.rect(iso.pos);
    let sheet = app.drw.as_ref().unwrap().data.sheets[0].clone();
    for b in &sheet.balloons {
        let a = g.world(iso.pos, b.anchor);
        assert!(a.x >= lo.x - 1e-6 && a.y >= lo.y - 1e-6 && a.x <= hi.x + 1e-6 && a.y <= hi.y + 1e-6, "anchor on the view");
        let p = iso.pos + b.pos;
        assert!(!(p.x > lo.x && p.y > lo.y && p.x < hi.x && p.y < hi.y), "balloon outside the view");
    }
    let t = texts(&app.sheet_scene(0, &[]).0);
    for want in ["POS.", "TEILENUMMER", "MENGE", "BOLT_M10.PRT", "PILLOW_BLOCK.PRT"] { assert!(t.iter().any(|s| s == want), "{} in {:?}", want, t); }
    for i in 1..=4 { assert!(t.iter().any(|s| *s == i.to_string()), "balloon {}", i); }
    // shown model dimensions are a part-drawing feature
    let ids: Vec<u32> = views(&app).iter().map(|v| v.id).collect();
    assert_eq!(app.show_model_dims(&ids), 0);
    // an exploded view spreads the components
    let plain = app.view_geom(iso.id).unwrap().rect(iso.pos);
    app.drw.as_mut().unwrap().data.sheets[0].view_mut(iso.id).unwrap().exploded = true;
    let ex = app.view_geom(iso.id).unwrap().rect(iso.pos);
    assert!((ex.1.y - ex.0.y) > (plain.1.y - plain.0.y) + 2.0, "{:?} vs {:?}", ex, plain);
}

#[test]
fn pdf_svg_and_dxf_output() {
    let mut app = part_drawing(block_with_hole(), Projection::Third);
    let ids: Vec<u32> = views(&app).iter().map(|v| v.id).collect();
    app.show_model_dims(&ids);
    let (prims, _) = app.sheet_scene(0, &[]);
    let pdf = freo::app::publish::pdf(&[(420.0, 297.0, prims.clone()), (420.0, 297.0, prims.clone())], "BLOCK");
    // work on bytes: the header carries a binary marker comment
    let find = |from: usize, needle: &str| pdf[from..].windows(needle.len()).position(|w| w == needle.as_bytes()).map(|i| i + from);
    let at = |off: usize, needle: &str| pdf[off..].starts_with(needle.as_bytes());
    assert!(at(0, "%PDF-1.4") && pdf.ends_with(b"%%EOF\n"));
    assert!(find(0, "/Count 2").is_some() && find(0, "/BaseFont /Helvetica").is_some());
    // the cross-reference table points at every object
    let sxr = pdf.windows(9).rposition(|w| w == b"startxref").unwrap();
    let tail = String::from_utf8(pdf[sxr + 10..].to_vec()).unwrap();
    let sx: usize = tail.lines().next().unwrap().trim().parse().unwrap();
    assert!(at(sx, "xref"));
    let xref = String::from_utf8(pdf[sx..sxr].to_vec()).unwrap();
    let table: Vec<usize> = xref.lines().skip(3).take_while(|l| l.ends_with(" n ")).map(|l| l[..10].parse().unwrap()).collect();
    assert!(table.len() >= 8);
    for (i, off) in table.iter().enumerate() { assert!(at(*off, &format!("{} 0 obj", i + 1)), "object {} at {}", i + 1, off); }
    // content streams declare their exact length
    let mut from = 0;
    while let Some(i) = find(from, "/Length ") {
        let digits: String = pdf[i + 8..].iter().take_while(|b| b.is_ascii_digit()).map(|b| *b as char).collect();
        let len: usize = digits.parse().unwrap();
        let start = find(i, "stream\n").unwrap() + 7;
        assert!(at(start + len, "endstream"), "stream length at {}", i);
        from = i + 8;
    }
    assert!(find(0, "\\33016.00 DURCHG.").is_some(), "⌀ maps to WinAnsi Ø");
    let svg = freo::app::publish::svg(420.0, 297.0, &prims);
    assert!(svg.starts_with("<?xml") && svg.trim_end().ends_with("</svg>") && svg.contains("viewBox=\"0 0 420 297\""));
    assert!(svg.contains("⌀16.00 DURCHG.") && svg.contains("stroke-dasharray"));
    let dxf = freo::app::publish::dxf(&prims);
    assert!(dxf.contains("ENTITIES") && dxf.trim_end().ends_with("EOF"));
    assert!(dxf.contains("%%c16.00 DURCHG.") && dxf.contains("\nHIDDEN\n") && dxf.contains("\nCENTER\n"));
    let lines = prims.iter().map(|p| if let Prim::Line { pts, .. } = p { pts.len().saturating_sub(1) } else { 0 }).sum::<usize>();
    assert_eq!(dxf.matches("\nLINE\n").count(), lines);
}

#[test]
fn drawing_files_round_trip_and_undo() {
    let dir = std::env::temp_dir().join(format!("freo_drw_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut app = part_drawing(block_with_hole(), Projection::Third);
    let ids: Vec<u32> = views(&app).iter().map(|v| v.id).collect();
    let shown = app.show_model_dims(&ids);
    app.drw_commit(|d| { let id = d.take_id(); d.sheets[0].notes.push(Note { id, pos: V2::new(30.0, 60.0), text: "NOTE 1".into(), leader: None }); });
    let path = dir.join("block.pfdrw");
    app.drw.as_mut().unwrap().path = Some(path.clone());
    app.save_drawing(false);
    assert!(!app.drw.as_ref().unwrap().dirty);
    let mut b = App::new(None);
    b.load(path);
    assert!(b.drawing_active());
    let d = b.drw.as_ref().unwrap();
    assert_eq!(d.data.sheets[0].views.len(), 4);
    assert_eq!(d.data.sheets[0].shown.len(), shown);
    assert_eq!(d.data.sheets[0].notes[0].text, "NOTE 1");
    assert_eq!(b.data.name, "BLOCK", "the embedded part was restored");
    assert!(texts(&b.sheet_scene(0, &[]).0).iter().any(|s| s == "⌀16.00 DURCHG."));
    // undo / redo of drawing edits
    let before = b.drw.as_ref().unwrap().data.sheets[0].notes.len();
    b.drw.as_mut().unwrap().sel = vec![DSel::Note(b.drw.as_ref().unwrap().data.sheets[0].notes[0].id)];
    b.drw_delete_selection();
    assert_eq!(b.drw.as_ref().unwrap().data.sheets[0].notes.len(), before - 1);
    b.drw_undo(false);
    assert_eq!(b.drw.as_ref().unwrap().data.sheets[0].notes.len(), before);
    b.drw_undo(true);
    assert_eq!(b.drw.as_ref().unwrap().data.sheets[0].notes.len(), before - 1);
    // the model can be opened from the drawing and the drawing found again
    b.drawing_open_model();
    assert!(!b.drawing_active());
    b.drawing_return();
    assert!(b.drawing_active());
    let _ = std::fs::remove_dir_all(&dir);
}
