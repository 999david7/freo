//! Assembly placement constraints, status, BOM, interference and the merged display.

use freo::kernel::csg::signed_volume;
use freo::kernel::math::*;
use freo::model::assembly::*;
use freo::model::features::*;
use freo::model::regen;
use freo::sketch::sketch::Sketch;

fn feat(id: u64, kind: FeatureKind) -> Feature { Feature { id, name: format!("F{}", id), suppressed: false, hidden: false, kind } }

fn block(name: &str, w: f64, d: f64, h: f64) -> ModelData {
    let mut m = ModelData::new_part(name);
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(TOP_ID));
    e.sketch = Sketch::rect(w, d, 0.0, 0.0);
    e.depth = Dim::v(h);
    m.features.push(feat(10, FeatureKind::Extrude(e)));
    m
}

/// 100 x 60 x 20 base with a Ø10 through hole on its top face.
fn base() -> ModelData {
    let mut m = block("BASE", 100.0, 60.0, 20.0);
    let FeatureKind::Hole(mut h) = FeatureKind::new_hole() else { unreachable!() };
    h.plane = Some(PlaneRef::Face(FaceRef { point: V3::new(0.0, 20.0, 0.0), normal: V3::Y }));
    h.x = Dim::v(20.0);
    h.diameter = Dim::v(10.0);
    h.through_all = true;
    m.features.push(feat(11, FeatureKind::Hole(h)));
    m
}

/// Ø10 x 30 peg standing on TOP.
fn peg() -> ModelData {
    let mut m = ModelData::new_part("PEG");
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(TOP_ID));
    e.sketch = Sketch::circle(5.0, 0.0, 0.0);
    e.depth = Dim::v(30.0);
    m.features.push(feat(10, FeatureKind::Extrude(e)));
    m
}

fn cyl_face(r: &regen::RegenResult) -> (usize, FaceRef) {
    let t = &r.topo;
    let f = (0..t.faces.len()).find(|&f| !t.faces[f].planar).expect("curved face");
    (f, t.surface_ref(f))
}

fn planar_face(r: &regen::RegenResult, n: V3, y: f64) -> FaceRef {
    let t = &r.topo;
    let f = (0..t.faces.len()).find(|&f| t.faces[f].planar && t.faces[f].normal.dot(n) > 0.999 && (t.faces[f].center.y - y).abs() < 1e-6).expect("planar face");
    t.face_ref(f)
}

#[test]
fn cylinder_axis_of_a_hole() {
    let r = regen::run(&base());
    let (f, _) = cyl_face(&r);
    let (p, d, radius) = r.topo.surface_axis(f).expect("axis");
    assert!(d.cross(V3::Y).length() < 1e-6, "axis direction {:?}", d);
    assert!((radius - 5.0).abs() < 0.1, "radius {}", radius);
    // the hole is 20 from the centre of the top face, in the face plane
    let off = V3::new(p.x, 0.0, p.z);
    assert!((off.length() - 20.0).abs() < 1e-6, "axis point {:?}", p);
}

#[test]
fn three_planes_fully_constrain_with_offset() {
    let mut asm = AssemblyData::new("ASM0001");
    asm.add_part(block("PLATE", 40.0, 40.0, 10.0), None);
    let mut c = Component::new(100, "PLATE", Placement { rot: glam::DQuat::from_rotation_y(0.4) * glam::DQuat::from_rotation_x(0.3), pos: V3::new(80.0, 30.0, -20.0) });
    let pc = |ty, a: u64, b: u64, off: f64| AsmConstraint { ty, a: Some(AsmRef::Plane { comp: 100, id: a }), b: Some(AsmRef::AsmPlane(b)), offset: off, sign: 0.0, enabled: true };
    c.constraints = vec![pc(AsmCType::Coincident, FRONT_ID, FRONT_ID, 0.0), pc(AsmCType::Distance, RIGHT_ID, RIGHT_ID, 50.0), pc(AsmCType::Coincident, TOP_ID, TOP_ID, 0.0)];
    asm.components.push(c);
    let res = regen(&asm, &mut PartCache::default(), &RegenOpts::default());
    assert_eq!(res.status[&100], PlaceStatus::Full);
    let pl = res.placements[&100];
    assert!((pl.pos - V3::new(50.0, 0.0, 0.0)).length() < 1e-6, "{:?}", pl.pos);
    assert!(pl.rot.angle_between(glam::DQuat::IDENTITY) < 1e-6);
    // the merged display carries the part as body 1, moved by 50 along X
    let b = res.display.topo.bbox;
    assert!((b.min.x - 30.0).abs() < 1e-6 && (b.max.x - 70.0).abs() < 1e-6, "{:?}", b);
    assert!(res.display.planes.iter().any(|p| p.id == comp_datum_id(100, FRONT_ID)));
}

#[test]
fn peg_coaxial_and_mated_on_base() {
    let mut asm = AssemblyData::new("ASM0001");
    asm.add_part(base(), None);
    asm.add_part(peg(), None);
    let mut c1 = Component::new(100, "BASE", Placement::default());
    c1.constraints.push(AsmConstraint::new(AsmCType::Default));
    asm.components.push(c1);
    let (rb, rp) = (regen::run(&base()), regen::run(&peg()));
    let (_, hole) = cyl_face(&rb);
    let (_, shaft) = cyl_face(&rp);
    let top = planar_face(&rb, V3::Y, 20.0);
    let bottom = planar_face(&rp, -V3::Y, 0.0);
    let mut c2 = Component::new(101, "PEG", Placement { rot: glam::DQuat::from_rotation_x(0.5), pos: V3::new(150.0, 60.0, 40.0) });
    c2.constraints.push(AsmConstraint { ty: AsmCType::Coincident, a: Some(AsmRef::Surface { comp: 101, face: shaft, planar: false }), b: Some(AsmRef::Surface { comp: 100, face: hole, planar: false }), offset: 0.0, sign: 0.0, enabled: true });
    let mut cache = PartCache::default();
    asm.components.push(c2.clone());
    let res = regen(&asm, &mut cache, &RegenOpts::default());
    assert_eq!(res.status[&101], PlaceStatus::Partial(2), "coaxial leaves spin and slide");
    // mate the peg's bottom to the base's top (references given in reverse order)
    c2.constraints.push(AsmConstraint { ty: AsmCType::Automatic, a: Some(AsmRef::Surface { comp: 100, face: top, planar: true }), b: Some(AsmRef::Surface { comp: 101, face: bottom, planar: true }), offset: 0.0, sign: 0.0, enabled: true });
    asm.components[1] = c2.clone();
    let res = regen(&asm, &mut cache, &RegenOpts::default());
    assert_eq!(res.status[&101], PlaceStatus::Assumed);
    let pl = res.placements[&101];
    let (hp, hd, _) = rb.topo.surface_axis(cyl_face(&rb).0).unwrap();
    let (pp, pd, _) = rp.topo.surface_axis(cyl_face(&rp).0).unwrap();
    let (wp, wd) = (pl.point(pp), pl.vector(pd));
    assert!(wd.cross(hd).length() < 1e-6, "axes parallel");
    let v = wp - hp;
    assert!((v - hd * v.dot(hd)).length() < 1e-5, "axes coincide");
    // bottom of the peg sits on the top of the base, peg pointing up
    let bottom_world = pl.point(V3::new(pp.x, 0.0, pp.z));
    assert!((bottom_world.y - 20.0).abs() < 1e-5, "{:?}", bottom_world);
    assert!(pl.vector(V3::Y).dot(V3::Y) > 0.999);
    // without assumptions the free spin is reported
    asm.components[1].assumptions = false;
    let res = regen(&asm, &mut cache, &RegenOpts::default());
    assert_eq!(res.status[&101], PlaceStatus::Partial(1));
    // bodies stay separate in the merged topology: no face spans two components
    let t = &res.display.topo;
    for f in &t.faces {
        let body = t.tags[f.polys[0]] >> freo::kernel::topology::BODY_SHIFT;
        assert!(f.polys.iter().all(|&p| t.tags[p] >> freo::kernel::topology::BODY_SHIFT == body));
    }
    let vol = signed_volume(&res.display.polys);
    let expect = signed_volume(&rb.polys) + signed_volume(&rp.polys);
    assert!((vol - expect).abs() < 1e-3, "{} vs {}", vol, expect);
}

#[test]
fn inconsistent_constraints_are_reported() {
    let mut asm = AssemblyData::new("ASM0001");
    asm.add_part(block("PLATE", 40.0, 40.0, 10.0), None);
    let mut c = Component::new(100, "PLATE", Placement::default());
    let pc = |ty, a: u64, b: u64, off: f64| AsmConstraint { ty, a: Some(AsmRef::Plane { comp: 100, id: a }), b: Some(AsmRef::AsmPlane(b)), offset: off, sign: 0.0, enabled: true };
    // the same plane cannot be both coincident with and 10 away from ASM_FRONT
    c.constraints = vec![pc(AsmCType::Coincident, FRONT_ID, FRONT_ID, 0.0), pc(AsmCType::Distance, FRONT_ID, FRONT_ID, 10.0)];
    asm.components.push(c);
    let res = regen(&asm, &mut PartCache::default(), &RegenOpts::default());
    assert!(matches!(res.status[&100], PlaceStatus::Invalid(_)), "{:?}", res.status[&100]);
}

#[test]
fn bom_interference_and_explode() {
    let mut asm = AssemblyData::new("ASM0001");
    asm.add_part(block("PLATE", 40.0, 40.0, 10.0), None);
    let mut a = Component::new(100, "PLATE", Placement::default());
    a.constraints.push(AsmConstraint::new(AsmCType::Default));
    let mut b = Component::new(101, "PLATE", Placement::translation(V3::new(30.0, 0.0, 0.0)));
    b.constraints.push(AsmConstraint::new(AsmCType::Fix));
    asm.components = vec![a, b];
    let mut cache = PartCache::default();
    let res = regen(&asm, &mut cache, &RegenOpts::default());
    assert_eq!(res.status[&101], PlaceStatus::Full);
    let rows = bom(&asm, &res);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].qty, 2);
    assert!((rows[0].volume - 16000.0).abs() < 1e-6);
    let csv = bom_csv(&asm, &rows);
    assert!(csv.contains("1,PLATE,2,16000.000"), "{}", csv);
    // 10 x 40 x 10 overlap
    let hits = interference(&asm, &res);
    assert_eq!(hits.len(), 1);
    assert!((hits[0].2 - 4000.0).abs() < 1e-3, "{:?}", hits);
    // exploding moves the components apart, away from the assembly centre
    let ex = regen(&asm, &mut cache, &RegenOpts { explode: 1.0, editing: None });
    assert!(ex.shown[&100].pos.x < -1.0 && ex.shown[&101].pos.x > 31.0, "{:?} {:?}", ex.shown[&100].pos, ex.shown[&101].pos);
    assert!(interference(&asm, &ex).len() == 1, "interference uses the assembled positions");
}

#[test]
fn sample_assembly_is_fully_constrained_without_interference() {
    let asm = freo::app::demo::sample_assembly();
    let res = regen(&asm, &mut PartCache::default(), &RegenOpts::default());
    for c in &asm.components {
        assert!(res.status[&c.id].is_full(), "{} {:?}", c.part, res.status[&c.id]);
    }
    // the shaft runs along Z through the bore, centred on ASM_FRONT
    let shaft = res.placements[&102];
    let a = shaft.point(V3::ZERO);
    let b = shaft.point(V3::new(0.0, 160.0, 0.0));
    assert!((a.z + 80.0).abs() < 1e-5 && (b.z - 80.0).abs() < 1e-5, "{:?} {:?}", a, b);
    assert!((a.y - 50.0).abs() < 1e-5 && a.x.abs() < 1e-5, "bore axis at y = 15 + 35: {:?}", a);
    // bolts stand in the holes with their heads on the plate
    for (id, x) in [(103u64, -75.0), (104, 75.0)] {
        let p = res.placements[&id].point(V3::ZERO);
        assert!((p - V3::new(x, 15.0, 0.0)).length() < 1e-5, "{:?}", p);
    }
    assert!(interference(&asm, &res).is_empty(), "{:?}", interference(&asm, &res));
    let rows = bom(&asm, &res);
    assert_eq!(rows.iter().find(|r| r.part == "BOLT_M10").unwrap().qty, 2);
    assert_eq!(res.bodies.len(), 5);
}

#[test]
fn flipping_a_free_coaxial_reverses_the_component() {
    let mut asm = AssemblyData::new("ASM0001");
    asm.add_part(base(), None);
    asm.add_part(peg(), None);
    let mut c1 = Component::new(100, "BASE", Placement::default());
    c1.constraints.push(AsmConstraint::new(AsmCType::Default));
    asm.components.push(c1);
    let (rb, rp) = (regen::run(&base()), regen::run(&peg()));
    let mut c2 = Component::new(101, "PEG", Placement::translation(V3::new(20.0, 40.0, 0.0)));
    c2.constraints.push(AsmConstraint { ty: AsmCType::Coincident, a: Some(AsmRef::Surface { comp: 101, face: cyl_face(&rp).1, planar: false }), b: Some(AsmRef::Surface { comp: 100, face: cyl_face(&rb).1, planar: false }), offset: 0.0, sign: 0.0, enabled: true });
    asm.components.push(c2);
    let mut cache = PartCache::default();
    let up = regen(&asm, &mut cache, &RegenOpts::default()).placements[&101].vector(V3::Y);
    assert!(up.dot(V3::Y) > 0.999, "keeps its sense: {:?}", up);
    asm.components[1].constraints[0].flip();
    let down = regen(&asm, &mut cache, &RegenOpts::default()).placements[&101].vector(V3::Y);
    assert!(down.dot(V3::Y) < -0.999, "flipped: {:?}", down);
}
