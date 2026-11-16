//! Fase / Rundung: one edge of an opening picked, the whole loop offered.
use freo::app::edgeloop::find_loop;
use freo::app::{App, Pick, PickTarget};
use freo::kernel::csg::signed_volume;
use freo::model::features::*;
use freo::sketch::sketch::Sketch;

/// A 120 x 60 x 20 block with a 60 x 30 pocket, 10 deep, in its top face.
fn tray() -> ModelData {
    let mut m = ModelData::new_part("WANNE");
    let mut k = FeatureKind::new_extrude();
    if let FeatureKind::Extrude(d) = &mut k {
        d.plane = Some(PlaneRef::Datum(TOP_ID));
        d.sketch = Sketch::rect(120.0, 60.0, 0.0, 0.0);
        d.depth = Dim::v(20.0);
    }
    m.features.push(Feature { id: 10, name: "Profil 1".into(), suppressed: false, hidden: false, kind: k });
    m.next_id = 11;
    m
}

fn app_with_pocket() -> App {
    let mut app = App::new(None);
    app.data = tray();
    app.regenerate(false);
    let t = app.result.topo.clone();
    let top = (0..t.faces.len()).find(|&f| t.faces[f].planar && t.faces[f].normal.y > 0.99).unwrap();
    app.handle_pick(Some(Pick::Face(top)), false);
    app.start_feature(FeatureKind::new_extrude());
    let slot = app.sketch.as_ref().expect("sketcher on the top face").slot;
    app.finish_sketch(Sketch::rect(60.0, 30.0, 0.0, 0.0), slot);
    app.update_editor(|f| {
        if let FeatureKind::Extrude(d) = &mut f.kind {
            d.remove = true;
            d.depth = Dim::v(10.0);
            d.flip = !d.flip;
        }
    });
    if app.editor_error().is_some() || signed_volume(&app.preview.as_ref().unwrap().polys) > 143000.0 {
        app.update_editor(|f| {
            if let FeatureKind::Extrude(d) = &mut f.kind {
                d.flip = !d.flip;
            }
        });
    }
    app.finish_feature();
    assert!(app.editor.is_none());
    assert!((signed_volume(&app.result.polys) - (144000.0 - 18000.0)).abs() < 1.0, "pocket cut: {}", signed_volume(&app.result.polys));
    app
}

/// A rim edge of the pocket (on the top face, 30 long) and an outer top edge (120 long).
fn edges(app: &App) -> (usize, usize) {
    let t = &app.result.topo;
    let at_top = |i: usize| t.chains[i].points.iter().all(|p| (p.y - 20.0).abs() < 1e-6);
    let rim = (0..t.chains.len()).find(|&i| at_top(i) && (t.chains[i].length() - 30.0).abs() < 1e-6).expect("rim edge");
    let outer = (0..t.chains.len()).find(|&i| at_top(i) && (t.chains[i].length() - 120.0).abs() < 1e-6).expect("outer edge");
    (rim, outer)
}

#[test]
fn the_rim_of_an_opening_is_found_as_one_loop() {
    let app = app_with_pocket();
    let (rim, outer) = edges(&app);
    let t = &app.result.topo;
    let (l, opening) = find_loop(t, rim).expect("loop of the pocket rim");
    assert_eq!(l.len(), 4);
    assert!(opening, "the pocket is an opening in the top face");
    assert!(l.iter().all(|&i| t.chains[i].points.iter().all(|p| (p.y - 20.0).abs() < 1e-6)), "the loop stays on the rim");
    let (l, opening) = find_loop(t, outer).expect("outline of a face");
    assert_eq!(l.len(), 4);
    assert!(!opening);
}

#[test]
fn picking_one_rim_edge_offers_the_whole_opening() {
    // edge first, then Fase
    let mut app = app_with_pocket();
    let (rim, _) = edges(&app);
    let v0 = signed_volume(&app.result.polys);
    app.handle_pick(Some(Pick::Edge(rim)), false);
    app.start_feature(FeatureKind::new_chamfer());
    let o = app.loop_offer.clone().expect("the loop is offered");
    assert!(o.opening);
    assert_eq!((o.count, o.edges.len()), (4, 3));
    app.accept_loop();
    assert!(app.loop_offer.is_none());
    let n = match &app.editor.as_ref().unwrap().feature.kind {
        FeatureKind::Chamfer(d) => d.edges.len(),
        _ => 0,
    };
    assert_eq!(n, 4, "all four rim edges are in the chamfer");
    app.update_editor(|f| {
        if let FeatureKind::Chamfer(d) = &mut f.kind {
            d.d1 = Dim::v(2.0)
        }
    });
    assert!(app.editor_error().is_none(), "{:?}", app.editor_error());
    app.finish_feature();
    assert!(app.editor.is_none());
    let v1 = signed_volume(&app.result.polys);
    // 180 mm of rim, 2 x 2 chamfer: about 360 mm³ less (the corners overlap a little)
    assert!(v0 - v1 > 330.0 && v0 - v1 < 365.0, "removed {}", v0 - v1);

    // Fase first, then the edge: asked on the click; Nein keeps the one edge and does not ask again
    let mut app = app_with_pocket();
    app.start_feature(FeatureKind::new_chamfer());
    assert!(app.loop_offer.is_none());
    assert_eq!(app.editor.as_ref().unwrap().pick, Some(PickTarget::Edges));
    let pick = |app: &App, len: f64, skip: usize| {
        let t = &app.displayed().topo;
        (0..t.chains.len())
            .filter(|&i| {
                t.chains[i].points.iter().all(|p| (p.y - 20.0).abs() < 1e-6)
                    && (t.chains[i].length() - len).abs() < 0.5
                    && t.chains[i].points.iter().all(|p| p.x.abs() < 31.0 && p.z.abs() < 16.0)
            })
            .nth(skip)
            .unwrap()
    };
    let e = pick(&app, 30.0, 0);
    app.handle_pick(Some(Pick::Edge(e)), false);
    assert!(app.loop_offer.as_ref().is_some_and(|o| o.opening && o.count == 4));
    app.decline_loop();
    let n = match &app.editor.as_ref().unwrap().feature.kind {
        FeatureKind::Chamfer(d) => d.edges.len(),
        _ => 0,
    };
    assert_eq!(n, 1);
    let e2 = pick(&app, 60.0, 0);
    app.handle_pick(Some(Pick::Edge(e2)), false);
    assert!(app.loop_offer.is_none(), "a declined loop is not offered again");
}

#[test]
fn a_round_gets_the_same_offer_for_a_face_outline() {
    let mut app = app_with_pocket();
    let (_, outer) = edges(&app);
    app.handle_pick(Some(Pick::Edge(outer)), false);
    app.start_feature(FeatureKind::new_round());
    let o = app.loop_offer.clone().expect("the outline is offered");
    assert!(!o.opening);
    app.accept_loop();
    let n = match &app.editor.as_ref().unwrap().feature.kind {
        FeatureKind::Round(d) => d.sets[0].edges.len(),
        _ => 0,
    };
    assert_eq!(n, 4);
    assert!(app.editor_error().is_none(), "{:?}", app.editor_error());
}
