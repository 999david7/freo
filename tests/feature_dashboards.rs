//! Modelling operations exposed by the round, chamfer and hole dashboards.
use freo::app::{App, Pick, PickTarget};
use freo::kernel::csg::signed_volume;
use freo::kernel::math::*;
use freo::model::features::*;
use freo::model::regen;
use freo::sketch::sketch::Sketch;

fn block() -> App {
    let mut app = App::new(None);
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.start_feature(FeatureKind::new_extrude());
    let slot = app.sketch.as_ref().unwrap().slot;
    app.finish_sketch(Sketch::rect(100.0, 60.0, 0.0, 0.0), slot);
    app.update_editor(|feature| {
        let FeatureKind::Extrude(def) = &mut feature.kind else { unreachable!() };
        def.depth = Dim::v(20.0);
    });
    app.finish_feature();
    assert!(app.result.errors.is_empty());
    app
}

#[test]
fn chamfer_sets_pick_independently_and_survive_save_undo_and_cache() {
    let mut app = block();
    let topo = app.result.topo.clone();
    let edges: Vec<usize> = topo.chains.iter().enumerate().filter(|(_, c)| c.is_straight() && (c.points[0].y - c.points.last().unwrap().y).abs() > 19.0).map(|(i, _)| i).collect();
    assert_eq!(edges.len(), 4);
    let volume = signed_volume(&app.result.polys);
    app.handle_pick(Some(Pick::Edge(edges[0])), false);
    app.start_feature(FeatureKind::new_chamfer());
    app.update_editor(|feature| {
        let FeatureKind::Chamfer(def) = &mut feature.kind else { unreachable!() };
        def.d1 = Dim::v(2.0);
        let mut second = def.primary.clone();
        second.edges.clear();
        second.kind = ChamferKind::D1xD2;
        second.d1 = Dim::v(3.0);
        second.d2 = Dim::v(4.0);
        def.additional_sets.push(second);
    });
    app.round_set = 1;
    app.set_pick_target(Some(PickTarget::Edges));
    app.handle_pick(Some(Pick::Edge(edges[1])), false);
    let FeatureKind::Chamfer(def) = &app.editor.as_ref().unwrap().feature.kind else { unreachable!() };
    assert_eq!(def.primary.edges.len(), 1);
    assert_eq!(def.additional_sets[0].edges.len(), 1);
    app.finish_feature();
    assert!(app.editor.is_none(), "{:?}", app.editor_error());
    let removed = volume - signed_volume(&app.result.polys);
    assert!((removed - (2.0 * 2.0 + 3.0 * 4.0) * 0.5 * 20.0).abs() < 0.01, "{removed}");
    let data: ModelData = serde_json::from_str(&serde_json::to_string(&app.data).unwrap()).unwrap();
    assert_eq!(data, app.data);
    let mut cache = regen::Cache::default();
    let cached = cache.run(&data);
    assert!(cached.errors.is_empty());
    assert!((signed_volume(&cached.polys) - signed_volume(&app.result.polys)).abs() < 1e-6);
    app.do_undo();
    assert!((signed_volume(&app.result.polys) - volume).abs() < 1e-6);
    app.do_redo();
    assert!((volume - signed_volume(&app.result.polys) - removed).abs() < 1e-6);
}

#[test]
fn chord_round_matches_the_equivalent_radius() {
    let mut app = block();
    let topo = app.result.topo.clone();
    let index = topo.chains.iter().position(|c| c.is_straight() && (c.points[0].y - c.points.last().unwrap().y).abs() > 19.0).unwrap();
    app.handle_pick(Some(Pick::Edge(index)), false);
    app.start_feature(FeatureKind::new_round());
    app.update_editor(|feature| {
        let FeatureKind::Round(def) = &mut feature.kind else { unreachable!() };
        def.sets[0].radius = Dim::v(4.0);
    });
    let expected = signed_volume(&app.preview.as_ref().unwrap().polys);
    app.update_editor(|feature| {
        let FeatureKind::Round(def) = &mut feature.kind else { unreachable!() };
        def.sets[0].chord = true;
        def.sets[0].radius = Dim::v(4.0 * 2.0f64.sqrt());
    });
    assert!(app.editor_error().is_none(), "{:?}", app.editor_error());
    assert!((signed_volume(&app.preview.as_ref().unwrap().polys) - expected).abs() < 1e-4);
    app.finish_feature();
    let restored: ModelData = serde_json::from_str(&serde_json::to_string(&app.data).unwrap()).unwrap();
    assert_eq!(restored, app.data);
}

#[test]
fn hole_offsets_follow_the_picked_reference_planes() {
    let mut app = block();
    app.start_feature(FeatureKind::new_hole());
    let face = app.result.topo.faces.iter().position(|face| face.planar && face.normal.dot(V3::Y) > 0.99 && face.center.y > 19.0).unwrap();
    app.handle_pick(Some(Pick::Face(face)), false);
    app.set_pick_target(Some(PickTarget::HoleOffsetX));
    app.handle_pick(Some(Pick::Plane(RIGHT_ID)), false);
    app.set_pick_target(Some(PickTarget::HoleOffsetY));
    app.handle_pick(Some(Pick::Plane(FRONT_ID)), false);
    app.update_editor(|feature| {
        let FeatureKind::Hole(def) = &mut feature.kind else { unreachable!() };
        def.x = Dim::v(12.0);
        def.y = Dim::v(8.0);
        def.through_all = true;
    });
    assert!(app.editor_error().is_none(), "{:?}", app.editor_error());
    let feature_id = app.editor.as_ref().unwrap().feature.id;
    app.finish_feature();
    let axis = app.result.axes.iter().find(|axis| axis.feature == feature_id).unwrap();
    assert!((axis.point - V3::new(12.0, 20.0, 8.0)).length() < 1e-6, "{:?}", axis.point);
    let def = &app.data.features.last().unwrap().kind;
    let FeatureKind::Hole(def) = def else { unreachable!() };
    assert!(FeatureKind::Hole(def.clone()).references().contains(&RIGHT_ID));
    assert!(FeatureKind::Hole(def.clone()).references().contains(&FRONT_ID));
    let mut old = serde_json::to_value(def).unwrap();
    old.as_object_mut().unwrap().remove("offset_x");
    old.as_object_mut().unwrap().remove("offset_y");
    let old: HoleDef = serde_json::from_value(old).unwrap();
    assert!(old.offset_x.is_none() && old.offset_y.is_none());
}

#[test]
fn dependent_hole_offset_references_are_reported() {
    let frame = Frame { origin: V3::ZERO, x: V3::X, y: V3::Y };
    let x_reference = Frame::from_normal(V3::ZERO, V3::X);
    assert!(regen::hole_position(&frame, 10.0, 20.0, [Some(x_reference), Some(x_reference)]).is_err());
    assert!(regen::hole_position(&frame, 10.0, 20.0, [Some(frame), None]).is_err());
    let tilted = Frame::from_normal(V3::new(2.0, 3.0, 0.0), (V3::X + V3::Y).normalize());
    let result = regen::hole_position(&frame, 4.0, 5.0, [Some(tilted), None]).unwrap();
    assert!((tilted.z().dot(frame.to_world(result) - tilted.origin) - 4.0).abs() < 1e-9);
    assert!((result.y - 5.0).abs() < 1e-9);
}

#[test]
fn legacy_chamfer_and_round_sets_still_load() {
    let legacy = serde_json::json!({
        "edges": [], "kind": "DxD", "d1": "2", "d2": "2", "angle": "45"
    });
    let chamfer: ChamferDef = serde_json::from_value(legacy).unwrap();
    assert!(chamfer.additional_sets.is_empty() && !chamfer.corner);
    assert_eq!(chamfer.d1, Dim::v(2.0));
    let round: RoundDef = serde_json::from_value(serde_json::json!({"edges": [], "radius": "3"})).unwrap();
    assert!(!round.sets[0].chord);
    let round: RoundDef = serde_json::from_value(serde_json::json!({"sets": [{"edges": [], "radius": "3"}]})).unwrap();
    assert!(!round.sets[0].chord);
}
