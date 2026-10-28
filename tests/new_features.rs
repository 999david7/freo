//! Rib, helical sweep, rotational blend, coordinate system, cosmetic thread, datum curve and
//! family table, regenerated straight from the model.

use freo::kernel::csg::signed_volume;
use freo::kernel::math::*;
use freo::model::features::*;
use freo::model::regen;
use freo::sketch::sketch::{SkKind, Sketch};

fn feat(id: u64, name: &str, kind: FeatureKind) -> Feature { Feature { id, name: name.into(), suppressed: false, hidden: false, kind } }

/// Open or closed polyline through `pts` (shared end points, so the chain is connected).
fn polyline(s: &mut Sketch, pts: &[(f64, f64)], closed: bool) {
    let ids: Vec<i64> = pts.iter().map(|p| s.add_point(V2::new(p.0, p.1))).collect();
    for w in ids.windows(2) { s.add_line(w[0], w[1]); }
    if closed { s.add_line(*ids.last().unwrap(), ids[0]); }
}

fn centerline(s: &mut Sketch, a: (f64, f64), b: (f64, f64)) {
    let (p, q) = (s.add_point(V2::new(a.0, a.1)), s.add_point(V2::new(b.0, b.1)));
    s.add_entity(SkKind::Line, vec![p, q], 0.0, true, true);
}

fn extrude(sk: Sketch, plane: u64, depth: f64, symmetric: bool) -> FeatureKind {
    let mut k = FeatureKind::new_extrude();
    if let FeatureKind::Extrude(d) = &mut k {
        d.plane = Some(PlaneRef::Datum(plane));
        d.sketch = sk;
        d.depth = Dim::v(depth);
        if symmetric { d.depth_type = DepthType::Symmetric; }
    }
    k
}

/// L-shaped bracket (FRONT plane profile, 40 deep, symmetric): volume 44000.
fn bracket() -> ModelData {
    let mut m = ModelData::new_part("BRACKET");
    let mut s = Sketch::default();
    polyline(&mut s, &[(0.0, 0.0), (60.0, 0.0), (60.0, 10.0), (10.0, 10.0), (10.0, 60.0), (0.0, 60.0)], true);
    m.features.push(feat(10, "Profil 1", extrude(s, FRONT_ID, 40.0, true)));
    m
}

fn ok(r: &regen::RegenResult) { assert!(r.errors.is_empty(), "{:?}", r.errors); }

fn close(a: f64, b: f64, rel: f64) -> bool { (a - b).abs() <= b.abs() * rel }

#[test]
fn rib_fills_the_corner() {
    let mut m = bracket();
    let base = signed_volume(&regen::run(&m).polys);
    assert!(close(base, 44000.0, 1e-6), "{base}");
    let mut s = Sketch::default();
    polyline(&mut s, &[(10.0, 50.0), (50.0, 10.0)], false);
    m.features.push(feat(11, "Rippe 1", FeatureKind::Rib(RibDef { plane: Some(PlaneRef::Datum(FRONT_ID)), sketch: s.clone(), thickness: Dim::v(4.0), flip: false, side: 0 })));
    let r = regen::run(&m);
    ok(&r);
    let v = signed_volume(&r.polys);
    // triangle 40 x 40 / 2, 4 thick
    assert!(close(v - base, 3200.0, 0.01), "rib adds {}", v - base);

    // one-sided thickness gives the same volume, flipped material side has nothing to close against
    if let FeatureKind::Rib(d) = &mut m.features[5].kind { d.side = 1; }
    let r = regen::run(&m);
    ok(&r);
    assert!(close(signed_volume(&r.polys) - base, 3200.0, 0.01));
    if let FeatureKind::Rib(d) = &mut m.features[5].kind { d.flip = true; }
    let r = regen::run(&m);
    assert!(r.errors.contains_key(&11), "flipped rib must fail: open side");
}

#[test]
fn helical_sweep_spring_volume() {
    let mut m = ModelData::new_part("SPRING");
    let mut prof = Sketch::default();
    centerline(&mut prof, (0.0, 0.0), (0.0, 10.0));
    polyline(&mut prof, &[(20.0, 0.0), (20.0, 50.0)], false);
    let sec = Sketch::circle(2.0, 20.0, 0.0);
    m.features.push(feat(10, "Spiralzug 1", FeatureKind::Helix(HelixDef { plane: Some(PlaneRef::Datum(FRONT_ID)), profile: prof, section: sec, pitch: Dim::v(8.0), left_handed: false, remove: false })));
    let r = regen::run(&m);
    ok(&r);
    let v = signed_volume(&r.polys);
    // Pappus: section area x path of its centroid around the axis (the axial lead is a shear)
    let expect = std::f64::consts::PI * 4.0 * 20.0 * std::f64::consts::TAU * (50.0 / 8.0);
    assert!(close(v, expect, 0.01), "spring {v} vs {expect}");
    let b = freo::kernel::csg::bbox(&r.polys);
    assert!(b.max.y > 49.0 && b.max.y < 53.0 && b.min.y > -3.0, "spring height {:?}", b);
    assert!(r.axes.iter().any(|a| a.feature == 10), "helix axis");

    // too coarse: the section is taller than the pitch
    if let FeatureKind::Helix(d) = &mut m.features[4].kind { d.pitch = Dim::v(3.0); }
    assert!(regen::run(&m).errors.contains_key(&10));
}

#[test]
fn helical_cut_threads_a_shaft() {
    let mut m = ModelData::new_part("SHAFT");
    // shaft r = 10 along Y, 40 long
    m.features.push(feat(10, "Profil 1", extrude(Sketch::circle(10.0, 0.0, 0.0), TOP_ID, 40.0, false)));
    let shaft = signed_volume(&regen::run(&m).polys);
    let mut prof = Sketch::default();
    centerline(&mut prof, (0.0, 0.0), (0.0, 10.0));
    polyline(&mut prof, &[(10.0, 5.0), (10.0, 35.0)], false);
    let mut sec = Sketch::default();
    polyline(&mut sec, &[(10.6, 5.0), (8.8, 4.1), (8.8, 5.9)], true);
    m.features.push(feat(11, "Spiralzug 1", FeatureKind::Helix(HelixDef { plane: Some(PlaneRef::Datum(FRONT_ID)), profile: prof, section: sec, pitch: Dim::v(2.5), left_handed: false, remove: true })));
    let r = regen::run(&m);
    ok(&r);
    let v = signed_volume(&r.polys);
    // the groove: triangle area inside the shaft x its centroid's travel
    assert!(v < shaft - 500.0 && v > shaft - 2500.0, "thread groove removes {}", shaft - v);
}

#[test]
fn rotational_blend_quarter_turn() {
    let mut m = ModelData::new_part("RBLEND");
    let mut s1 = Sketch::default();
    centerline(&mut s1, (0.0, 0.0), (0.0, 10.0));
    polyline(&mut s1, &[(10.0, 0.0), (20.0, 0.0), (20.0, 10.0), (10.0, 10.0)], true);
    let mut s2 = Sketch::default();
    polyline(&mut s2, &[(10.0, 0.0), (20.0, 0.0), (20.0, 10.0), (10.0, 10.0)], true);
    let mut k = FeatureKind::new_rotational_blend();
    if let FeatureKind::Blend(d) = &mut k { d.plane = Some(PlaneRef::Datum(FRONT_ID)); d.sections = vec![s1, s2]; d.offsets = vec![Dim::v(90.0)]; }
    m.features.push(feat(10, "Rotationsverbund 1", k));
    let r = regen::run(&m);
    ok(&r);
    let v = signed_volume(&r.polys);
    let expect = 100.0 * 15.0 * std::f64::consts::FRAC_PI_2;
    assert!(close(v, expect, 0.01), "rotational blend {v} vs {expect}");
    assert_eq!(m.features[4].kind.type_name(), "Rotationsverbund");
}

#[test]
fn coordinate_system_and_curve_through_points() {
    let mut m = ModelData::new_part("DATUMS");
    let mut k = FeatureKind::new_csys();
    if let FeatureKind::DatumCsys(d) = &mut k { d.x = Dim::v(10.0); d.rz = Dim::v(90.0); }
    m.features.push(feat(10, "CS0", k));
    for (i, p) in [(0.0, 0.0), (20.0, 10.0), (40.0, 0.0)].iter().enumerate() {
        m.features.push(feat(11 + i as u64, &format!("PNT{i}"), FeatureKind::DatumPoint(DatumPointDef { x: Dim::v(p.0), y: Dim::v(p.1), z: Dim::v(0.0) })));
    }
    m.features.push(feat(20, "Kurve 1", FeatureKind::Curve(CurveDef { points: vec![11, 12, 13], spline: true })));
    let r = regen::run(&m);
    ok(&r);
    let cs = r.csys.iter().find(|c| c.id == 10).expect("csys");
    assert!((cs.frame.origin - V3::new(10.0, 0.0, 0.0)).length() < 1e-9);
    assert!((cs.frame.x - V3::Y).length() < 1e-9, "X rotated onto Y: {:?}", cs.frame.x);
    let c = r.curves.iter().find(|c| c.feature == 20).expect("curve");
    let line = &c.polylines[0];
    assert!((line[0] - V3::ZERO).length() < 1e-9 && (line[line.len() - 1] - V3::new(40.0, 0.0, 0.0)).length() < 1e-9);
    assert!(line.iter().any(|p| (*p - V3::new(20.0, 10.0, 0.0)).length() < 1e-9), "passes through the middle point");
    assert!(m.features.last().unwrap().kind.references().contains(&12));
}

#[test]
fn cosmetic_thread_on_a_shaft() {
    let mut m = ModelData::new_part("BOLT");
    m.features.push(feat(10, "Profil 1", extrude(Sketch::circle(5.0, 0.0, 0.0), TOP_ID, 30.0, false)));
    let r = regen::run(&m);
    let t = &r.topo;
    let fi = (0..t.faces.len()).find(|&f| !t.faces[f].planar).expect("cylinder face");
    let (_, _, rad, len, internal) = regen::thread_cylinder(t, fi, false).expect("cylinder");
    assert!((rad - 5.0).abs() < 0.05 && (len - 30.0).abs() < 1e-6 && !internal, "{rad} {len} {internal}");
    assert_eq!(regen::thread_for_cylinder(10.0, false), (10.0, 1.5));
    assert_eq!(regen::thread_for_cylinder(8.5, true), (10.0, 1.5));
    let face = t.surface_ref(fi);
    m.features.push(feat(11, "Gewinde 1", FeatureKind::Thread(ThreadDef { face: Some(face), diameter: Dim::v(10.0), pitch: Dim::v(1.5), length: Dim::v(20.0), flip: false })));
    let r = regen::run(&m);
    ok(&r);
    let c = r.cosmetics.iter().find(|c| c.feature == 11).expect("cosmetic thread");
    assert_eq!(c.label, "M10");
    assert!((c.radius - 5.0).abs() < 1e-9 && (c.length - 20.0).abs() < 1e-9);
    assert!(c.base.y.abs() < 1e-6 || (c.base.y - 30.0).abs() < 1e-6, "starts at a shaft end: {:?}", c.base);
}

#[test]
fn family_table_instances() {
    let mut m = bracket();
    m.parameters.push(Parameter { name: "T".into(), expr: "40".into(), note: String::new() });
    if let FeatureKind::Extrude(d) = &mut m.features[4].kind { d.depth = Dim("T".into()); }
    let mut hole = FeatureKind::new_hole();
    if let FeatureKind::Hole(d) = &mut hole { d.plane = Some(PlaneRef::Datum(TOP_ID)); d.x = Dim::v(35.0); d.through_all = true; d.diameter = Dim::v(6.0); }
    m.features.push(feat(11, "Bohrung 1", hole));
    m.family.columns = vec![FamilyColumn::Parameter("T".into()), FamilyColumn::Feature(11)];
    m.family.instances = vec![
        FamilyInstance { name: "BRACKET_20".into(), values: vec!["20".into(), "N".into()] },
        FamilyInstance { name: "BRACKET_GEN".into(), values: vec!["*".into(), "*".into()] },
    ];
    let generic = signed_volume(&regen::run(&m).polys);
    let i0 = m.instance(0).unwrap();
    assert_eq!(i0.name, "BRACKET_20");
    assert!(i0.features.iter().find(|f| f.id == 11).unwrap().suppressed);
    assert!(close(signed_volume(&regen::run(&i0).polys), 22000.0, 1e-6));
    assert!(close(signed_volume(&regen::run(&m.instance(1).unwrap()).polys), generic, 1e-9));
    m.family.active = Some(0);
    assert!(close(signed_volume(&regen::run(&m).polys), 22000.0, 1e-6), "active instance is shown");
    // round trip through the file format, and old files without a family table still load
    let json = serde_json::to_string(&m).unwrap();
    let back: ModelData = serde_json::from_str(&json).unwrap();
    assert_eq!(back.family, m.family);
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v.as_object_mut().unwrap().remove("family");
    let old: ModelData = serde_json::from_value(v).unwrap();
    assert!(old.family.is_empty());
}
