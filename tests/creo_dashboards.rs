//! What the Creo dashboards for Kantenfase, Rundung and Bohrung offer, checked on the geometry:
//! chamfer schemes and corner transitions, conic / C2 / D1 x D2 / full / through-curve rounds
//! and spherical corners, hole depth options, placements, sketched and lightweight holes.

use freo::kernel::csg::signed_volume;
use freo::kernel::math::*;
use freo::mcp::Session;
use freo::model::features::*;
use serde_json::{json, Value};
use std::f64::consts::PI;

fn call(s: &mut Session, tool: &str, args: Value) -> Value {
    let out = s.call(tool, args.as_object().unwrap()).unwrap_or_else(|e| panic!("{}: {}", tool, e));
    out.value
}

fn try_call(s: &mut Session, tool: &str, args: Value) -> Result<Value, String> {
    s.call(tool, args.as_object().unwrap()).map(|o| o.value)
}

fn volume(s: &Session) -> f64 {
    signed_volume(&s.result.polys)
}

/// 100 x 20 x 60 block on TOP: x -50..50, y 0..20, z -30..30.
fn block() -> Session {
    let mut s = Session::new();
    call(&mut s, "new_part", json!({}));
    call(&mut s, "add_extrude", json!({ "plane": "TOP", "shapes": [{ "type": "rect", "width": 100, "height": 60 }], "depth": 20 }));
    assert!(s.result.errors.is_empty());
    s
}

/// Index of the straight edge whose midpoint is nearest to `m`.
fn edge_at(s: &Session, m: V3) -> usize {
    let t = &s.result.topo;
    (0..t.chains.len())
        .min_by(|&a, &b| {
            let mid = |i: usize| {
                let c = &t.chains[i];
                (c.points[0] + *c.points.last().unwrap()) * 0.5
            };
            (mid(a) - m).length().partial_cmp(&(mid(b) - m).length()).unwrap()
        })
        .unwrap()
}

fn no_errors(s: &Session) {
    assert!(s.result.errors.is_empty(), "{:?}", s.result.errors);
}

#[test]
fn chamfer_offset_and_45_schemes() {
    // the vertical edge at x = 50, z = 30 between two perpendicular faces
    for (kind, d1, d2, removed) in [("OxO", 2.0, 2.0, 0.5 * 2.0 * 2.0 * 20.0), ("O1xO2", 2.0, 3.0, 0.5 * 2.0 * 3.0 * 20.0), ("45xD", 2.5, 2.5, 0.5 * 2.5 * 2.5 * 20.0)] {
        let mut s = block();
        let v0 = volume(&s);
        let e = edge_at(&s, V3::new(50.0, 10.0, 30.0));
        call(&mut s, "add_chamfer", json!({ "edges": [e], "distance": d1, "distance2": d2, "kind": kind }));
        no_errors(&s);
        let got = v0 - volume(&s);
        assert!((got - removed).abs() < 0.05, "{kind}: removed {got}, expected {removed}");
    }
}

#[test]
fn chamfer_45_needs_perpendicular_faces() {
    // a wedge: the sloped face meets the bottom at 45°
    let mut s = Session::new();
    call(&mut s, "new_part", json!({}));
    call(&mut s, "add_extrude", json!({ "plane": "FRONT", "shapes": [{ "type": "polygon", "points": [[0, 0], [40, 0], [0, 40]] }], "depth": 10 }));
    let e = edge_at(&s, V3::new(40.0, 0.0, 5.0));
    assert!(try_call(&mut s, "add_chamfer", json!({ "edges": [e], "distance": 2, "kind": "45xD" })).is_err() || !s.result.errors.is_empty());
}

#[test]
fn conic_c2_and_d1xd2_round_sections() {
    let corner = V3::new(50.0, 10.0, 30.0);
    // a 90° corner with tangent distances a, b: the parabolic conic (rho 0.5) keeps 2/3 of the
    // triangle (corner, t1, t2), so a third of it is removed
    for (section, r, d2, removed) in [
        ("circular", 5.0, 5.0, (25.0 - PI * 25.0 / 4.0) * 20.0),
        ("conic", 5.0, 5.0, 25.0 / 2.0 / 3.0 * 20.0),
        ("d1xd2_conic", 4.0, 6.0, 4.0 * 6.0 / 2.0 / 3.0 * 20.0),
    ] {
        let mut s = block();
        let v0 = volume(&s);
        let e = edge_at(&s, corner);
        call(&mut s, "add_round", json!({ "edges": [e], "radius": r, "distance2": d2, "section": section, "rho": 0.5 }));
        no_errors(&s);
        let got = v0 - volume(&s);
        assert!((got - removed).abs() < removed * 0.02, "{section}: removed {got}, expected {removed}");
    }
    // C2: curvature-continuous; inside the triangle of its tangent points, fuller with a larger rho
    let c2 = |rho: f64| {
        let mut s = block();
        let v0 = volume(&s);
        let e = edge_at(&s, corner);
        call(&mut s, "add_round", json!({ "edges": [e], "radius": 5, "section": "c2", "rho": rho }));
        no_errors(&s);
        v0 - volume(&s)
    };
    let (thin, full) = (c2(0.3), c2(0.7));
    assert!(thin > full && full > 1.0 && thin < 12.5 * 20.0, "c2 removed {thin} (rho 0.3), {full} (rho 0.7)");
    // rho outside 0.05 .. 0.95 is refused
    let mut s = block();
    let e = edge_at(&s, corner);
    let r = try_call(&mut s, "add_round", json!({ "edges": [e], "radius": 5, "section": "conic", "rho": 0.99 }));
    assert!(r.is_err() || !s.result.errors.is_empty());
}

#[test]
fn full_round_replaces_the_top_face() {
    // a 10 mm wide, 30 mm high rib, 40 mm long
    let mut s = Session::new();
    call(&mut s, "new_part", json!({}));
    call(&mut s, "add_extrude", json!({ "plane": "FRONT", "shapes": [{ "type": "rect", "width": 10, "height": 30, "cy": 15 }], "depth": 40 }));
    let v0 = volume(&s);
    let (a, b) = (edge_at(&s, V3::new(-5.0, 30.0, 20.0)), edge_at(&s, V3::new(5.0, 30.0, 20.0)));
    call(&mut s, "add_round", json!({ "edges": [a, b], "radius": 1, "full_round": true }));
    no_errors(&s);
    let removed = v0 - volume(&s);
    let expect = (10.0 * 5.0 - PI * 25.0 / 2.0) * 40.0;
    assert!((removed - expect).abs() < expect * 0.02, "removed {removed}, expected {expect}");
    // the top is a half cylinder: nothing stays at the old height
    assert!(s.result.topo.bbox.max.y < 30.0 - 1e-3 || s.result.topo.positions.iter().all(|p| p.y < 30.0 - 1e-3 || p.x.abs() < 0.2));
    // one edge is no full round
    let mut s2 = Session::new();
    call(&mut s2, "new_part", json!({}));
    call(&mut s2, "add_extrude", json!({ "plane": "FRONT", "shapes": [{ "type": "rect", "width": 10, "height": 30, "cy": 15 }], "depth": 40 }));
    let a = edge_at(&s2, V3::new(-5.0, 30.0, 20.0));
    let r = try_call(&mut s2, "add_round", json!({ "edges": [a], "radius": 1, "full_round": true }));
    assert!(r.is_err() || !s2.result.errors.is_empty());
}

#[test]
fn round_through_curve_takes_its_radius_from_the_curve() {
    let mut s = block();
    // a shallow groove in the top whose near edge runs 4 mm behind the front edge (z = 26)
    call(&mut s, "add_extrude", json!({ "plane": "+Y", "shapes": [{ "type": "rect", "width": 100, "height": 6, "cy": -23 }], "depth": 0.5, "remove": true }));
    no_errors(&s);
    let v0 = volume(&s);
    let front = edge_at(&s, V3::new(0.0, 20.0, 30.0));
    let curve = edge_at(&s, V3::new(0.0, 20.0, 26.0));
    call(&mut s, "add_round", json!({ "edges": [front], "radius": 1, "through_curve": [curve] }));
    no_errors(&s);
    let removed = v0 - volume(&s);
    let expect = (16.0 - PI * 4.0) * 100.0;
    assert!((removed - expect).abs() < expect * 0.02, "removed {removed}, expected {expect} (R 4 from the curve)");
}

/// The vertices of the rounded corner of a 40 mm cube at (20, 40, 20) and the centre of the ball
/// that touches its three faces.
fn cube_corner(transition: &str) -> (Session, Vec<V3>, V3) {
    let mut s = Session::new();
    call(&mut s, "new_part", json!({}));
    call(&mut s, "add_extrude", json!({ "plane": "TOP", "shapes": [{ "type": "rect", "width": 40, "height": 40 }], "depth": 40 }));
    let c = V3::new(20.0, 40.0, 20.0);
    let edges: Vec<usize> = [V3::new(0.0, 40.0, 20.0), V3::new(20.0, 40.0, 0.0), V3::new(20.0, 20.0, 20.0)].iter().map(|m| edge_at(&s, *m)).collect();
    call(&mut s, "add_round", json!({ "edges": edges, "radius": 6, "corner_transition": transition }));
    no_errors(&s);
    let centre = c - V3::new(6.0, 6.0, 6.0);
    let near: Vec<V3> = s.result.topo.positions.iter().copied().filter(|p| p.x > centre.x + 0.3 && p.y > centre.y + 0.3 && p.z > centre.z + 0.3).collect();
    (s, near, centre)
}

#[test]
fn sphere_transition_makes_a_ball_corner() {
    let (s, near, centre) = cube_corner("sphere");
    assert!(!near.is_empty());
    let worst = near.iter().map(|p| ((*p - centre).length() - 6.0).abs()).fold(0.0, f64::max);
    assert!(worst < 0.05, "corner vertices are up to {worst} off the sphere");
    let by_edge = {
        let t = &s.result.topo;
        let mut m: std::collections::HashMap<(usize, usize), usize> = Default::default();
        for p in &t.polys {
            for i in 0..p.len() {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                *m.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        m.into_iter().filter(|((a, b), n)| *n != 2 && (t.positions[*a] - t.positions[*b]).length() > 0.05).count()
    };
    assert_eq!(by_edge, 0, "open edges at the sphere corner");
    // the standard transition is the plain intersection of the three rounds: it sticks out
    let (_, near, centre) = cube_corner("standard");
    let worst = near.iter().map(|p| (*p - centre).length() - 6.0).fold(0.0, f64::max);
    assert!(worst > 0.5, "standard corner should not be a sphere ({worst})");
}

#[test]
fn corner_chamfer_transition_cuts_the_tip() {
    let tip = |transition: &str| {
        let mut s = Session::new();
        call(&mut s, "new_part", json!({}));
        call(&mut s, "add_extrude", json!({ "plane": "TOP", "shapes": [{ "type": "rect", "width": 40, "height": 40 }], "depth": 40 }));
        let edges: Vec<usize> = [V3::new(0.0, 40.0, 20.0), V3::new(20.0, 40.0, 0.0), V3::new(20.0, 20.0, 20.0)].iter().map(|m| edge_at(&s, *m)).collect();
        call(&mut s, "add_chamfer", json!({ "edges": edges, "distance": 4, "corner_transition": transition }));
        no_errors(&s);
        // depth below the corner along the cube diagonal of the outermost vertex
        let c = V3::new(20.0, 40.0, 20.0);
        s.result.topo.positions.iter().map(|p| (c - *p).dot(V3::new(1.0, 1.0, 1.0))).fold(f64::INFINITY, f64::min)
    };
    let standard = tip("standard");
    let cut = tip("corner_chamfer");
    assert!((standard - 6.0).abs() < 1e-3, "three chamfers leave their tip at depth sum 1.5 D: {standard}");
    assert!(cut > 7.9, "the corner chamfer cuts down to depth sum 2 D: {cut}");
}

#[test]
fn hole_depth_options() {
    let area = PI * 25.0;
    // through the 20 mm block, ending at the next surface
    let mut s = block();
    let v0 = volume(&s);
    call(&mut s, "add_hole", json!({ "plane": "+Y", "x": 0, "y": 0, "diameter": 10, "depth_type": "to_next", "profile": "flat" }));
    no_errors(&s);
    assert!(((v0 - volume(&s)) - area * 20.0).abs() < area * 20.0 * 0.02);
    // to a selected face: the bottom
    let mut s = block();
    call(&mut s, "add_hole", json!({ "plane": "+Y", "x": 0, "y": 0, "diameter": 10, "depth_type": "to_selected", "depth_ref": "-Y", "profile": "flat" }));
    no_errors(&s);
    assert!(((v0 - volume(&s)) - area * 20.0).abs() < area * 20.0 * 0.02);
    // symmetric about the top face: only half of it is in the part
    let mut s = block();
    call(&mut s, "add_hole", json!({ "plane": "+Y", "x": 0, "y": 0, "diameter": 10, "depth": 12, "depth_type": "symmetric", "profile": "flat" }));
    no_errors(&s);
    assert!(((v0 - volume(&s)) - area * 6.0).abs() < area * 6.0 * 0.02, "{}", v0 - volume(&s));
}

#[test]
fn lightweight_hole_removes_nothing() {
    let mut s = block();
    let v0 = volume(&s);
    call(&mut s, "add_hole", json!({ "plane": "+Y", "x": 10, "y": 5, "diameter": 8, "through_all": true, "lightweight": true }));
    no_errors(&s);
    assert!((volume(&s) - v0).abs() < 1e-6);
    let id = s.data.features.last().unwrap().id;
    assert!(s.result.axes.iter().any(|a| a.feature == id), "lightweight holes keep their axis");
    let curves = s.result.curves.iter().find(|c| c.feature == id).expect("outline curves");
    assert_eq!(curves.polylines.len(), 2);
    // the outline runs from the top to the bottom of the part
    let ys: Vec<f64> = curves.polylines.iter().map(|l| l[0].y).collect();
    assert!(ys.iter().any(|y| (y - 20.0).abs() < 1e-6) && ys.iter().any(|y| y.abs() < 0.01), "{ys:?}");
}

#[test]
fn sketched_hole_is_revolved_about_its_centreline() {
    let mut s = block();
    let v0 = volume(&s);
    call(
        &mut s,
        "add_hole",
        json!({ "plane": "+Y", "x": 0, "y": 0, "profile": "sketched", "shapes": [
            { "type": "centerline", "from": [0, 5], "to": [0, -20] },
            { "type": "polygon", "points": [[0, 0], [5, 0], [5, -10], [3, -12], [3, -15], [0, -15]] }
        ] }),
    );
    no_errors(&s);
    let expect = PI * 25.0 * 10.0 + PI * 2.0 / 3.0 * (25.0 + 15.0 + 9.0) + PI * 9.0 * 3.0;
    let removed = v0 - volume(&s);
    assert!((removed - expect).abs() < expect * 0.02, "removed {removed}, expected {expect}");
    // it goes down from the top face
    let id = s.data.features.last().unwrap().id;
    let b = freo::kernel::csg::bbox(&s.result.tools[&id].polys);
    assert!(b.max.y > 19.9 && b.min.y > 20.0 - 15.5, "{:?}", b);
}

#[test]
fn radial_diameter_and_coaxial_placement() {
    let mut s = block();
    call(&mut s, "add_hole", json!({ "plane": "+Y", "x": 0, "y": 0, "diameter": 6, "through_all": true, "name": "MITTE" }));
    let place = |s: &mut Session, args: Value| -> V3 {
        call(s, "add_hole", args);
        no_errors(s);
        let id = s.data.features.last().unwrap().id;
        s.result.axes.iter().find(|a| a.feature == id).unwrap().point
    };
    // angle measured from RECHTS: 0° points to -Z on the top face, 90° to -X
    let p = place(&mut s, json!({ "plane": "+Y", "placement": "radial", "axis": "MITTE", "angle_ref": "RIGHT", "x": 20, "angle": 90, "diameter": 4, "depth": 5 }));
    assert!((p - V3::new(-20.0, 20.0, 0.0)).length() < 1e-6, "{p:?}");
    let p = place(&mut s, json!({ "plane": "+Y", "placement": "diameter", "axis": "MITTE", "angle_ref": "RIGHT", "x": 30, "angle": 0, "diameter": 4, "depth": 5 }));
    assert!((p - V3::new(0.0, 20.0, -15.0)).length() < 1e-6, "{p:?}");
    let p = place(&mut s, json!({ "plane": "+Y", "placement": "coaxial", "axis": "MITTE", "diameter": 10, "depth": 5 }));
    assert!((p - V3::new(0.0, 20.0, 0.0)).length() < 1e-6, "{p:?}");
    // a radial hole without its axis is refused
    let r = try_call(&mut s, "add_hole", json!({ "plane": "+Y", "placement": "radial", "x": 10, "diameter": 4, "depth": 5 }));
    assert!(r.is_err() || !s.result.errors.is_empty());
}

#[test]
fn hole_note_reads_like_creo() {
    let FeatureKind::Hole(mut h) = FeatureKind::new_hole() else { unreachable!() };
    h.diameter = Dim::v(12.0);
    h.depth = Dim::v(30.13);
    h.drill_tip = false;
    let vars = Default::default();
    assert_eq!(freo::model::regen::hole_note_text(&h, &vars), "⌀12.000 ↧ 30.130");
    h.set_depth_kind(DepthType::ThroughAll);
    assert_eq!(freo::model::regen::hole_note_text(&h, &vars), "⌀12.000 DURCH ALLE");
}

#[test]
fn new_options_survive_saving() {
    let mut s = block();
    let e = edge_at(&s, V3::new(50.0, 10.0, 30.0));
    call(&mut s, "add_round", json!({ "edges": [e], "radius": 3, "section": "d1xd2_c2", "distance2": 4, "rho": 0.3 }));
    call(&mut s, "add_hole", json!({ "plane": "+Y", "x": 10, "y": 5, "diameter": 8, "depth_type": "symmetric", "depth": 6, "lightweight": true, "note": false }));
    no_errors(&s);
    let text = serde_json::to_string(&s.data).unwrap();
    let back: ModelData = serde_json::from_str(&text).unwrap();
    assert_eq!(back, s.data);
    let r = freo::model::regen::run(&back);
    assert!(r.errors.is_empty());
    assert!((signed_volume(&r.polys) - volume(&s)).abs() < 1e-6);
}

/// 40 x 40 right triangle on FRONT, 10 deep: the hypotenuse meets the bottom at 45° along the
/// edge at x = 40, y = 0.
fn wedge() -> Session {
    let mut s = Session::new();
    call(&mut s, "new_part", json!({}));
    call(&mut s, "add_extrude", json!({ "plane": "FRONT", "shapes": [{ "type": "polygon", "points": [[0, 0], [40, 0], [0, 40]] }], "depth": 10 }));
    no_errors(&s);
    s
}

#[test]
fn chamfer_offset_surfaces_and_tangent_distance_differ_on_oblique_faces() {
    let removed = |method: &str| {
        let mut s = wedge();
        let v0 = volume(&s);
        let e = edge_at(&s, V3::new(40.0, 0.0, 5.0));
        call(&mut s, "add_chamfer", json!({ "edges": [e], "distance": 2, "method": method }));
        no_errors(&s);
        v0 - volume(&s)
    };
    // tangent distance: 2 mm along both faces; offset surfaces: where each face, moved 2 mm
    // inwards, cuts the other one (2 / sin 45° along the faces)
    let s45 = (45f64).to_radians().sin();
    let tangent = 0.5 * 2.0 * 2.0 * s45 * 10.0;
    let offset = 0.5 * (2.0 / s45) * (2.0 / s45) * s45 * 10.0;
    let (t, o) = (removed("tangent_distance"), removed("offset_surfaces"));
    assert!((t - tangent).abs() < 0.05, "tangent {t}, expected {tangent}");
    assert!((o - offset).abs() < 0.05, "offset {o}, expected {offset}");
}

#[test]
fn chamfer_distance_by_reference_point() {
    let mut s = block();
    let v0 = volume(&s);
    call(&mut s, "add_feature_json", json!({ "kind": { "DatumPoint": { "x": "0", "y": "20", "z": "27" } }, "name": "PNT0" }));
    no_errors(&s);
    let e = edge_at(&s, V3::new(0.0, 20.0, 30.0));
    call(&mut s, "add_chamfer", json!({ "edges": [e], "reference_point": "PNT0" }));
    no_errors(&s);
    let removed = v0 - volume(&s);
    assert!((removed - 0.5 * 3.0 * 3.0 * 100.0).abs() < 0.1, "removed {removed}: D must be the 3 mm to the point");
}

#[test]
fn round_past_a_face_border_needs_extended_surfaces() {
    // a 4 mm plate: an R6 round on its top edge does not fit on the 4 mm side face
    let plate = || {
        let mut s = Session::new();
        call(&mut s, "new_part", json!({}));
        call(&mut s, "add_extrude", json!({ "plane": "TOP", "shapes": [{ "type": "rect", "width": 60, "height": 40 }], "depth": 4 }));
        let e = edge_at(&s, V3::new(0.0, 4.0, 20.0));
        (s, e)
    };
    let (mut s, e) = plate();
    let r = try_call(&mut s, "add_round", json!({ "edges": [e], "radius": 6 }));
    assert!(r.is_err() || !s.result.errors.is_empty(), "the round must not silently run over the face border");
    let (mut s, e) = plate();
    call(&mut s, "add_round", json!({ "edges": [e], "radius": 6, "extend_surfaces": true }));
    no_errors(&s);
    // a radius that fits needs nothing
    let (mut s, e) = plate();
    call(&mut s, "add_round", json!({ "edges": [e], "radius": 3 }));
    no_errors(&s);
}

#[test]
fn rolling_ball_keeps_more_than_normal_to_spine() {
    let removed = |method: &str| {
        let mut s = block();
        let v0 = volume(&s);
        let e = edge_at(&s, V3::new(50.0, 10.0, 30.0));
        call(&mut s, "add_round", json!({ "edges": [e], "radius": 4, "method": method }));
        // make it variable 2 -> 10 over the 20 mm edge
        let mut data = s.data.clone();
        if let FeatureKind::Round(d) = &mut data.features.last_mut().unwrap().kind {
            d.sets[0].variable = true;
            d.sets[0].radius = Dim::v(2.0);
            d.sets[0].radius_end = Dim::v(10.0);
        }
        let r = freo::model::regen::run(&data);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        v0 - signed_volume(&r.polys)
    };
    let (ball, spine) = (removed("rolling_ball"), removed("normal_to_spine"));
    let loss = 1.0 - PI / 4.0;
    let spine_exact = loss * 20.0 * (4.0 + 20.0 + 100.0) / 3.0;
    assert!((spine - spine_exact).abs() < spine_exact * 0.03, "normal to spine {spine}, expected {spine_exact}");
    assert!(ball < spine - 1.0, "the union of the rolling balls keeps more material: {ball} vs {spine}");
}

#[test]
fn round_and_chamfer_as_surface_leave_the_solid() {
    for tool in ["add_round", "add_chamfer"] {
        let mut s = block();
        let v0 = volume(&s);
        let e = edge_at(&s, V3::new(0.0, 20.0, 30.0));
        call(&mut s, tool, json!({ "edges": [e], "radius": 5, "distance": 5, "surface": true }));
        no_errors(&s);
        assert!((volume(&s) - v0).abs() < 1e-6, "{tool}: the solid must stay");
        let id = s.data.features.last().unwrap().id;
        let q = s.result.quilts.iter().find(|q| q.feature == id).expect("a new surface");
        let area: f64 = q
            .polys
            .iter()
            .map(|p| (1..p.v.len() - 1).map(|k| (p.v[k].p - p.v[0].p).cross(p.v[k + 1].p - p.v[0].p).length() * 0.5).sum::<f64>())
            .sum();
        let expect = if tool == "add_round" { PI / 2.0 * 5.0 * 100.0 } else { 50f64.sqrt() * 100.0 };
        assert!((area - expect).abs() < expect * 0.02, "{tool}: area {area}, expected {expect}");
    }
}
