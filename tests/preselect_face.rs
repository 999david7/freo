//! Object-action: a face selected before Profil (or before Definieren...) becomes the sketch plane.
use freo::app::{App, Pick, PickTarget, Slot};
use freo::kernel::math::*;
use freo::model::features::*;
use freo::sketch::sketch::Sketch;

fn block(app: &mut App) {
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.start_feature(FeatureKind::new_extrude());
    app.sketch.as_mut().unwrap().sketch = Sketch::rect(100.0, 60.0, 0.0, 0.0);
    let s = app.sketch.as_ref().unwrap();
    let (sk, slot) = (s.sketch.clone(), s.slot);
    app.finish_sketch(sk, slot);
    app.update_editor(|f| {
        if let FeatureKind::Extrude(d) = &mut f.kind {
            d.depth = Dim::v(20.0)
        }
    });
    app.finish_feature();
    assert!(app.editor.is_none(), "block built");
}

fn top_face(app: &App) -> usize {
    let t = &app.result.topo;
    (0..t.faces.len()).find(|&f| t.faces[f].planar && t.faces[f].normal.dot(V3::Y) > 0.999).expect("top face")
}

#[test]
fn face_then_profil_sketches_on_that_face() {
    let mut app = App::new(None);
    block(&mut app);
    let f = top_face(&app);
    app.handle_pick(Some(Pick::Face(f)), false);
    app.start_feature(FeatureKind::new_extrude());
    assert!(matches!(app.editor.as_ref().unwrap().feature.kind.sketch_plane(), Some(PlaneRef::Face(_))), "the face is the sketch plane");
    assert!(app.sketch.is_some(), "the sketcher opens on the face");
}

#[test]
fn face_selected_with_the_feature_filter_is_still_used() {
    let mut app = App::new(None);
    block(&mut app);
    let f = top_face(&app);
    app.filter = freo::app::Filter::Feature;
    app.handle_pick(Some(Pick::Face(f)), false);
    app.start_feature(FeatureKind::new_extrude());
    assert!(matches!(app.editor.as_ref().unwrap().feature.kind.sketch_plane(), Some(PlaneRef::Face(_))), "the clicked face is the sketch plane");
}

#[test]
fn definieren_takes_the_face_clicked_before() {
    let mut app = App::new(None);
    block(&mut app);
    let f = top_face(&app);
    // Profil first (nothing selected), then a face is clicked while a panel field is active
    app.start_feature(FeatureKind::new_extrude());
    app.set_pick_target(None);
    app.handle_pick(Some(Pick::Face(f)), false);
    app.open_sketch_dialog();
    assert!(matches!(app.editor.as_ref().unwrap().feature.kind.sketch_plane(), Some(PlaneRef::Face(_))), "Definieren uses the selected face");
    assert_eq!(app.editor.as_ref().unwrap().pick, None);
    app.begin_sketch(Slot::Main);
    assert!(app.sketch.is_some());
    let _ = PickTarget::SketchPlane;
}

#[test]
fn profil_has_a_direction_arrow_that_flips() {
    let mut app = App::new(None);
    block(&mut app);
    let f = top_face(&app);
    app.handle_pick(Some(Pick::Face(f)), false);
    app.start_feature(FeatureKind::new_extrude());
    assert!(app.extrude_direction().is_none(), "no arrow while sketching");
    app.sketch.as_mut().unwrap().sketch = Sketch::rect(30.0, 20.0, 10.0, 5.0);
    let s = app.sketch.as_ref().unwrap();
    let (sk, slot) = (s.sketch.clone(), s.slot);
    app.finish_sketch(sk, slot);
    let (base, dir) = app.extrude_direction().expect("arrow on the section");
    assert!((base.y - 20.0).abs() < 1e-9, "the arrow starts on the sketch plane, got {base:?}");
    assert!((dir.length() - 1.0).abs() < 1e-9 && dir.y.abs() > 0.999, "along the plane normal");
    app.flip_extrude_direction();
    let (base2, dir2) = app.extrude_direction().unwrap();
    assert!((base2 - base).length() < 1e-9 && (dir2 + dir).length() < 1e-9, "flipping turns the arrow around");
}
