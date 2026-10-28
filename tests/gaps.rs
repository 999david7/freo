//! Tests for ellipse profiles, Delete Segment trimming, To Next / To Selected depths and standard holes.

use freo::kernel::csg::signed_volume;
use freo::kernel::math::*;
use freo::model::features::*;
use freo::model::regen;
use freo::sketch::sketch::{SkKind, Sketch};

fn feat(id: u64, kind: FeatureKind) -> Feature { Feature { id, name: format!("F{}", id), suppressed: false, hidden: false, kind } }

fn extrude(plane: PlaneRef, sk: Sketch, dt: DepthType, depth: f64) -> ExtrudeDef {
    let FeatureKind::Extrude(mut d) = FeatureKind::new_extrude() else { unreachable!() };
    d.plane = Some(plane); d.sketch = sk; d.depth_type = dt; d.depth = Dim::v(depth);
    d
}

#[test]
fn ellipse_profile_extrudes() {
    let mut s = Sketch::default();
    let c = s.add_point(V2::ZERO);
    let a = s.add_point(V2::new(30.0, 0.0));
    s.add_entity(SkKind::Ellipse, vec![c, a], 10.0, false, false);
    let mut m = ModelData::new_part("E");
    m.features.push(feat(10, FeatureKind::Extrude(extrude(PlaneRef::Datum(FRONT_ID), s, DepthType::Blind, 10.0))));
    let r = regen::run(&m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let exact = std::f64::consts::PI * 30.0 * 10.0 * 10.0;
    assert!((signed_volume(&r.polys) - exact).abs() / exact < 0.01, "{}", signed_volume(&r.polys));
}

#[test]
fn to_next_and_to_selected() {
    let mut m = ModelData::new_part("D");
    m.features.push(feat(10, FeatureKind::Extrude(extrude(PlaneRef::Datum(TOP_ID), Sketch::rect(100.0, 100.0, 0.0, 0.0), DepthType::Blind, 10.0))));
    // datum plane 50 above TOP; a boss sketched there extrudes down "To Next" onto the block
    let FeatureKind::DatumPlane(mut dp) = FeatureKind::new_plane() else { unreachable!() };
    dp.reference = Some(PlaneRef::Datum(TOP_ID)); dp.offset = Dim::v(50.0);
    m.features.push(feat(11, FeatureKind::DatumPlane(dp)));
    let mut e = extrude(PlaneRef::Datum(11), Sketch::rect(20.0, 20.0, 0.0, 0.0), DepthType::ToNext, 0.0);
    e.flip = true;
    m.features.push(feat(12, FeatureKind::Extrude(e)));
    let r = regen::run(&m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = signed_volume(&r.polys);
    assert!((v - (100000.0 + 400.0 * 40.0)).abs() < 1.0, "to next volume {}", v);
    // a second boss from the block top "To Selected" the datum plane
    let mut e2 = extrude(PlaneRef::Face(FaceRef { point: V3::new(40.0, 10.0, 40.0), normal: V3::Y }), Sketch::rect(10.0, 10.0, 40.0, 0.0), DepthType::ToSelected, 0.0);
    e2.to_ref = Some(PlaneRef::Datum(11));
    m.features.push(feat(13, FeatureKind::Extrude(e2)));
    let r = regen::run(&m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v2 = signed_volume(&r.polys);
    assert!((v2 - v - 100.0 * 40.0).abs() < 1.0, "to selected adds {}", v2 - v);
}

#[test]
fn standard_tapped_hole_has_cosmetic_thread() {
    let mut m = ModelData::new_part("H");
    m.features.push(feat(10, FeatureKind::Extrude(extrude(PlaneRef::Datum(TOP_ID), Sketch::rect(40.0, 40.0, 0.0, 0.0), DepthType::Blind, 20.0))));
    let FeatureKind::Hole(mut h) = FeatureKind::new_hole() else { unreachable!() };
    h.plane = Some(PlaneRef::Face(FaceRef { point: V3::new(0.0, 20.0, 0.0), normal: V3::Y }));
    h.ty = HoleType::Standard; h.size = "M6".into(); h.tapped = true; h.through_all = true;
    m.features.push(feat(11, FeatureKind::Hole(h)));
    let r = regen::run(&m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let drilled = 40.0 * 40.0 * 20.0 - std::f64::consts::PI * 2.5 * 2.5 * 20.0;
    assert!((signed_volume(&r.polys) - drilled).abs() / drilled < 0.002, "{}", signed_volume(&r.polys));
    assert_eq!(r.cosmetics.len(), 1);
    assert!((r.cosmetics[0].radius - 3.0).abs() < 1e-9 && (r.cosmetics[0].length - 20.0).abs() < 1e-6);
    assert_eq!(r.cosmetics[0].label, "M6x1");
}

#[test]
fn step_export_is_a_closed_faceted_brep() {
    let mut m = ModelData::new_part("STEP_BLOCK");
    m.features.push(feat(10, FeatureKind::Extrude(extrude(PlaneRef::Datum(TOP_ID), Sketch::rect(40.0, 30.0, 0.0, 0.0), DepthType::Blind, 10.0))));
    let FeatureKind::Hole(mut h) = FeatureKind::new_hole() else { unreachable!() };
    h.plane = Some(PlaneRef::Face(FaceRef { point: V3::new(0.0, 10.0, 0.0), normal: V3::Y }));
    h.through_all = true;
    m.features.push(feat(11, FeatureKind::Hole(h)));
    let r = regen::run(&m);
    let text = freo::app::export::step_text(&r, &m.name, "block.stp");
    assert!(text.starts_with("ISO-10303-21;") && text.trim_end().ends_with("END-ISO-10303-21;"));
    // every entity reference resolves, ids are unique
    let mut defined = std::collections::HashMap::new();
    let mut points = std::collections::HashMap::new();
    let mut loops: Vec<Vec<usize>> = vec![];
    for line in text.lines().filter(|l| l.starts_with('#')) {
        let (id, body) = line.split_once('=').unwrap();
        let id: usize = id[1..].parse().unwrap();
        assert!(defined.insert(id, body.to_string()).is_none(), "duplicate #{}", id);
        if body.starts_with("CARTESIAN_POINT") {
            let inner = &body[body.rfind('(').unwrap() + 1..body.find(')').unwrap()];
            let c: Vec<f64> = inner.split(',').map(|v| v.parse().unwrap()).collect();
            points.insert(id, V3::new(c[0], c[1], c[2]));
        }
        if body.starts_with("POLY_LOOP") {
            loops.push(body.split('#').skip(1).map(|s| s.trim_end_matches(|c: char| !c.is_ascii_digit()).parse().unwrap()).collect());
        }
    }
    for body in defined.values() {
        for r in body.split('#').skip(1) {
            let n: usize = r.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap();
            assert!(defined.contains_key(&n), "dangling #{}", n);
        }
    }
    assert_eq!(text.matches("=FACE(").count(), r.topo.polys.len());
    // the shell is closed: each directed edge is matched by its reverse, and the loops enclose the solid's volume
    let mut edges = std::collections::HashMap::new();
    let mut vol = 0.0;
    for l in &loops {
        for k in 0..l.len() { *edges.entry((l[k], l[(k + 1) % l.len()])).or_insert(0i32) += 1; }
        for k in 1..l.len() - 1 { vol += points[&l[0]].dot(points[&l[k]].cross(points[&l[k + 1]])) / 6.0; }
    }
    for (&(a, b), &n) in &edges { assert_eq!(n, edges.get(&(b, a)).copied().unwrap_or(0), "open edge #{}-#{}", a, b); }
    assert!((vol - signed_volume(&r.polys)).abs() < 1e-3, "{} vs {}", vol, signed_volume(&r.polys));
    assert!(text.contains("SI_UNIT(.MILLI.,.METRE.)") && text.contains("PRODUCT('STEP_BLOCK'"));
}
