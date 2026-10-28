//! End-to-end assembly workflow through the application controller (no window):
//! assemble part files, pick placement references, finish, undo/redo, open a component,
//! edit it, return, save and reopen.

use freo::app::{App, Pick};
use freo::kernel::math::*;
use freo::model::assembly::PlaceStatus;
use freo::model::features::*;
use freo::sketch::sketch::Sketch;

fn feat(id: u64, kind: FeatureKind) -> Feature { Feature { id, name: format!("Extrude {}", id - 9), suppressed: false, hidden: false, kind } }

fn extrude(sk: Sketch, depth: f64) -> FeatureKind {
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(TOP_ID));
    e.sketch = sk;
    e.depth = Dim::v(depth);
    FeatureKind::Extrude(e)
}

fn write_parts(dir: &std::path::Path) {
    let mut base = ModelData::new_part("BASE");
    base.features.push(feat(10, extrude(Sketch::rect(100.0, 60.0, 0.0, 0.0), 20.0)));
    let FeatureKind::Hole(mut h) = FeatureKind::new_hole() else { unreachable!() };
    h.plane = Some(PlaneRef::Face(FaceRef { point: V3::new(0.0, 20.0, 0.0), normal: V3::Y }));
    h.diameter = Dim::v(10.0);
    h.through_all = true;
    base.features.push(Feature { id: 11, name: "Hole 1".into(), suppressed: false, hidden: false, kind: FeatureKind::Hole(h) });
    let mut peg = ModelData::new_part("PEG");
    peg.features.push(feat(10, extrude(Sketch::circle(5.0, 0.0, 0.0), 30.0)));
    std::fs::write(dir.join("base.pfprt"), serde_json::to_string(&base).unwrap()).unwrap();
    std::fs::write(dir.join("peg.pfprt"), serde_json::to_string(&peg).unwrap()).unwrap();
}

/// A face of component `comp` on screen, chosen by `score` (highest wins).
fn face_of(app: &App, comp: u64, planar: bool, score: impl Fn(V3, V3) -> f64) -> usize {
    let r = app.displayed();
    let t = &r.topo;
    (0..t.faces.len())
        .filter(|&f| r.tag_owner.get(&t.face_tag(f)) == Some(&comp) && t.faces[f].planar == planar)
        .max_by(|&a, &b| score(t.faces[a].center, t.faces[a].normal).partial_cmp(&score(t.faces[b].center, t.faces[b].normal)).unwrap())
        .expect("face")
}

#[test]
fn assemble_place_edit_and_save() {
    let dir = std::env::temp_dir().join(format!("freo_asm_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    write_parts(&dir);
    let mut app = App::new(None);
    app.new_assembly("ASM0001");
    assert!(app.in_asm());

    // first component goes to the Default location
    app.assemble_path(dir.join("base.pfprt"));
    assert!(app.placing());
    assert!(app.place_editor().unwrap().status.is_full());
    app.place_finish();
    assert_eq!(app.asm.as_ref().unwrap().data.components.len(), 1);

    // second: coaxial with the hole, then mate the bottom to the top face
    app.assemble_path(dir.join("peg.pfprt"));
    let id = app.place_editor().unwrap().comp.id;
    let shaft = face_of(&app, id, false, |_, _| 0.0);
    app.handle_pick(Some(Pick::Face(shaft)), false);
    let hole = face_of(&app, 100, false, |_, _| 0.0);
    app.handle_pick(Some(Pick::Face(hole)), false);
    assert_eq!(app.place_editor().unwrap().status, PlaceStatus::Partial(2));
    // dragging slides the peg along its free axis; it stays coaxial with the hole
    let axis_x = |app: &App| { let pl = app.place_editor().unwrap().comp.placement; let p = pl.point(V3::ZERO); (p, pl.vector(V3::Y)) };
    let (p0, _) = axis_x(&app);
    app.place_drag(0.0, -40.0, false);
    let (p1, d1) = axis_x(&app);
    assert!((p1.y - p0.y).abs() > 1.0, "moved along the axis: {:?} -> {:?}", p0, p1);
    assert!((V3::new(p1.x, 0.0, p1.z) - V3::new(p0.x, 0.0, p0.z)).length() < 1e-5 && d1.cross(V3::Y).length() < 1e-6, "still coaxial");
    let bottom = face_of(&app, id, true, |_, n| -n.y);
    app.handle_pick(Some(Pick::Face(bottom)), false);
    let top = face_of(&app, 100, true, |c, n| n.y * 1000.0 + c.y);
    app.handle_pick(Some(Pick::Face(top)), false);
    assert_eq!(app.place_editor().unwrap().status, PlaceStatus::Assumed);
    // once constrained, dragging cannot move it off its position
    let before = app.place_editor().unwrap().comp.placement.pos;
    app.place_drag(25.0, 30.0, false);
    assert!((app.place_editor().unwrap().comp.placement.pos - before).length() < 1e-5);
    app.place_finish();
    assert!(!app.placing());
    let asm = app.asm.as_ref().unwrap();
    assert_eq!(asm.data.components.len(), 2);
    assert_eq!(asm.data.components[1].constraints.len(), 2, "the empty trailing constraint is dropped");
    let p = asm.res.placements[&id].point(V3::ZERO);
    assert!((p.y - 20.0).abs() < 1e-5, "peg sits on the top face: {:?}", p);
    // the display holds both bodies
    assert_eq!(asm.res.bodies.len(), 2);

    // undo / redo the placement
    app.do_undo();
    assert_eq!(app.asm.as_ref().unwrap().data.components.len(), 1);
    app.do_redo();
    assert_eq!(app.asm.as_ref().unwrap().data.components.len(), 2);

    // open the peg, lengthen it, return: the assembly picks up the change
    app.asm_open_part(id);
    assert!(!app.in_asm() && app.editing_from_asm().is_some());
    assert!(!app.editing_from_asm().unwrap().ghost.is_empty(), "the base is shown as context");
    app.commit("longer", |d| if let FeatureKind::Extrude(e) = &mut d.features[4].kind { e.depth = Dim::v(45.0); });
    app.asm_return();
    assert!(app.in_asm());
    let asm = app.asm.as_ref().unwrap();
    assert!(asm.data.part("PEG").unwrap().modified);
    let b = app.displayed().topo.bbox;
    assert!((b.max.y - 65.0).abs() < 1e-4, "peg now reaches 20 + 45: {:?}", b);

    // exploded view and BOM
    app.toggle_exploded();
    assert!(app.asm.as_ref().unwrap().res.shown[&id].pos.y > 20.0 + 1.0);
    app.toggle_exploded();

    // save (writes the modified part back to its file) and reopen
    let path = dir.join("asm0001.pfasm");
    app.path = Some(path.clone());
    app.save(false);
    assert!(!app.dirty);
    let peg_back: ModelData = serde_json::from_str(&std::fs::read_to_string(dir.join("peg.pfprt")).unwrap()).unwrap();
    assert!(matches!(&peg_back.features[4].kind, FeatureKind::Extrude(e) if e.depth.0 == "45"));
    let mut app2 = App::new(None);
    app2.load(path);
    assert!(app2.in_asm());
    let a2 = app2.asm.as_ref().unwrap();
    assert_eq!(a2.data.components.len(), 2);
    assert!(a2.res.status[&id].is_full());

    // deleting the base removes the peg's constraints that referenced it
    app2.asm_delete(&[100]);
    let a2 = app2.asm.as_ref().unwrap();
    assert_eq!(a2.data.components.len(), 1);
    assert!(a2.data.components[0].constraints.is_empty());
    assert_eq!(a2.data.parts.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
