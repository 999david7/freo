//! Spline / ellipse sketching and Delete Segment trimming through the sketcher, multiple
//! round sets, variable-radius rounds, and fill / curve patterns.

use freo::app::sketcher::Tool;
use freo::app::{App, Pick};
use freo::kernel::csg::signed_volume;
use freo::kernel::math::*;
use freo::model::features::*;
use freo::model::regen;
use freo::sketch::sketch::{SkKind, Sketch};

const ROUND_LOSS: f64 = 1.0 - std::f64::consts::PI / 4.0;

fn vol(app: &App) -> f64 { signed_volume(&app.result.polys) }

fn feat(id: u64, kind: FeatureKind) -> Feature { Feature { id, name: format!("F{}", id), suppressed: false, hidden: false, kind } }

fn finish_sketch(app: &mut App) {
    let s = app.sketch.as_ref().unwrap();
    let (sk, slot) = (s.sketch.clone(), s.slot);
    app.finish_sketch(sk, slot);
}

fn extrude_on_top(depth: f64) -> App {
    let mut app = App::new(None);
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.start_feature(FeatureKind::new_extrude());
    assert!(app.sketch.is_some());
    let _ = depth;
    app
}

#[test]
fn spline_closed_by_a_line_extrudes() {
    let mut app = extrude_on_top(10.0);
    let s = app.sketch.as_mut().unwrap();
    s.set_tool(Tool::Spline);
    let pts = [V2::new(-40.0, 0.0), V2::new(-20.0, 30.0), V2::new(20.0, 30.0), V2::new(40.0, 0.0)];
    for p in pts { s.click_at(p); }
    s.end_chain();
    let sp = s.sketch.entities.iter().find(|e| e.kind == SkKind::Spline).expect("spline").clone();
    assert_eq!(sp.pts.len(), 4);
    // the curve interpolates its points
    let samples = s.sketch.sample(&sp, 24);
    for p in pts { assert!(samples.iter().any(|q| (*q - p).length() < 1e-3), "passes through {:?}", p); }
    s.set_tool(Tool::Line);
    s.click_at(V2::new(40.0, 0.0));
    s.click_at(V2::new(-40.0, 0.0));
    s.end_chain();
    assert!(s.sketch.open_ends().is_empty(), "spline + line form a closed loop");
    let area = polygon_area2(&s.sketch.sample(&sp, 64)).abs();
    finish_sketch(&mut app);
    app.update_editor(|f| if let FeatureKind::Extrude(d) = &mut f.kind { d.depth = Dim::v(10.0) });
    assert!(app.editor_error().is_none(), "{:?}", app.editor_error());
    app.finish_feature();
    let v = vol(&app);
    assert!(area > 1500.0 && area < 2600.0, "area under the spline {}", area);
    assert!((v - area * 10.0).abs() / (area * 10.0) < 0.02, "volume {} vs {}", v, area * 10.0);
}

#[test]
fn ellipse_tool_builds_an_ellipse() {
    let mut app = extrude_on_top(10.0);
    let s = app.sketch.as_mut().unwrap();
    s.set_tool(Tool::Ellipse);
    s.click_at(V2::new(0.0, 0.0));
    s.click_at(V2::new(30.0, 0.0));
    s.click_at(V2::new(0.0, 12.0));
    let e = s.sketch.entities.iter().find(|e| e.kind == SkKind::Ellipse).expect("ellipse").clone();
    assert!((e.r - 12.0).abs() < 1e-4, "minor radius {}", e.r);
    finish_sketch(&mut app);
    app.update_editor(|f| if let FeatureKind::Extrude(d) = &mut f.kind { d.depth = Dim::v(10.0) });
    app.finish_feature();
    let exact = std::f64::consts::PI * 30.0 * 12.0 * 10.0;
    assert!((vol(&app) - exact).abs() / exact < 0.01, "{}", vol(&app));
}

#[test]
fn delete_segment_trims_at_intersections() {
    let mut app = extrude_on_top(10.0);
    let s = app.sketch.as_mut().unwrap();
    s.set_tool(Tool::Line);
    for p in [V2::new(-50.0, -30.0), V2::new(50.0, -30.0), V2::new(50.0, 30.0), V2::new(-50.0, 30.0), V2::new(-50.0, -30.0)] { s.click_at(p); }
    s.end_chain();
    // a vertical line sticking out of the rectangle on both sides
    s.click_at(V2::new(10.0, -50.0));
    s.click_at(V2::new(10.0, 50.0));
    s.end_chain();
    let n = s.sketch.entities.len();
    s.set_tool(Tool::Trim);
    s.click_at(V2::new(10.0, 42.0));
    s.click_at(V2::new(10.0, -42.0));
    assert_eq!(s.sketch.entities.len(), n, "trimmed, not deleted");
    let v = s.sketch.entities.iter().find(|e| e.kind == SkKind::Line && (s.sketch.point(e.pts[0]).x - 10.0).abs() < 1e-3 && (s.sketch.point(e.pts[1]).x - 10.0).abs() < 1e-3).expect("vertical line").clone();
    let (a, b) = (s.sketch.point(v.pts[0]), s.sketch.point(v.pts[1]));
    assert!((a.y.min(b.y) + 30.0).abs() < 1e-3 && (a.y.max(b.y) - 30.0).abs() < 1e-3, "ends on the rectangle: {:?} {:?}", a, b);
    // deleting a whole side between two intersections opens the outline at that side
    s.click_at(V2::new(30.0, 30.0));
    assert!(!s.sketch.open_ends().is_empty(), "top-right segment removed");
}

fn block_with_vertical_edges() -> (ModelData, regen::RegenResult) {
    let mut m = ModelData::new_part("B");
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(TOP_ID));
    e.sketch = Sketch::rect(100.0, 60.0, 0.0, 0.0);
    e.depth = Dim::v(20.0);
    m.features.push(feat(10, FeatureKind::Extrude(e)));
    let r = regen::run(&m);
    (m, r)
}

fn vertical_edges(r: &regen::RegenResult, x_sign: f64) -> Vec<EdgeRef> {
    let t = &r.topo;
    (0..t.chains.len())
        .filter(|&i| { let c = &t.chains[i]; c.is_straight() && (c.points[0].y - c.points.last().unwrap().y).abs() > 1.0 && c.points[0].x * x_sign > 0.0 })
        .map(|i| t.edge_ref(i))
        .collect()
}

#[test]
fn two_round_sets_with_their_own_radii() {
    let (mut m, r) = block_with_vertical_edges();
    let v0 = signed_volume(&r.polys);
    let FeatureKind::Round(mut rd) = FeatureKind::new_round() else { unreachable!() };
    rd.sets[0].edges = vertical_edges(&r, 1.0);
    rd.sets[0].radius = Dim::v(8.0);
    let mut s2 = RoundSet::new(4.0);
    s2.edges = vertical_edges(&r, -1.0);
    rd.sets.push(s2);
    assert_eq!(rd.edge_count(), 4);
    m.features.push(feat(11, FeatureKind::Round(rd)));
    let r2 = regen::run(&m);
    assert!(r2.errors.is_empty(), "{:?}", r2.errors);
    let removed = v0 - signed_volume(&r2.polys);
    let expect = ROUND_LOSS * 20.0 * (2.0 * 64.0 + 2.0 * 16.0);
    assert!((removed - expect).abs() / expect < 0.08, "removed {} expected {}", removed, expect);
}

#[test]
fn variable_radius_round() {
    let (mut m, r) = block_with_vertical_edges();
    let v0 = signed_volume(&r.polys);
    let FeatureKind::Round(mut rd) = FeatureKind::new_round() else { unreachable!() };
    rd.sets[0].edges = vec![vertical_edges(&r, 1.0)[0]];
    rd.sets[0].radius = Dim::v(4.0);
    rd.sets[0].variable = true;
    rd.sets[0].radius_end = Dim::v(10.0);
    m.features.push(feat(11, FeatureKind::Round(rd)));
    let r2 = regen::run(&m);
    assert!(r2.errors.is_empty(), "{:?}", r2.errors);
    let removed = v0 - signed_volume(&r2.polys);
    // ∫ (1 - π/4) r(t)² dt with r linear from 4 to 10 over 20 mm
    let expect = ROUND_LOSS * 20.0 * (16.0 + 40.0 + 100.0) / 3.0;
    assert!((removed - expect).abs() / expect < 0.08, "removed {} expected {}", removed, expect);
    // a constant radius 4 round removes clearly less
    assert!(removed > ROUND_LOSS * 20.0 * 16.0 * 1.5);
}

#[test]
fn corner_shapes_meet_their_continuity_class() {
    use freo::kernel::corner::CornerShape;
    for shape in CornerShape::ALL {
        for half_deg in [30.0f64, 45.0, 60.0] {
            let half = half_deg.to_radians();
            // real coordinates for a unit-radius round: tangent points at 1/tan(half) along the edges
            let d = 1.0 / half.tan();
            let (v1, v2) = (V2::new(d, 0.0), V2::new((2.0 * half).cos(), (2.0 * half).sin()) * d);
            let ab = shape.points(half, 240);
            let pts: Vec<V2> = ab.iter().map(|(a, b)| v1 * *a + v2 * *b).collect();
            let name = format!("{:?} at {}°", shape, half_deg);
            // starts on the first edge, ends on the second, symmetric, through the middle of the round
            assert!(ab[0].1 == 0.0 && ab[240].0 == 0.0 && (ab[0].0 - ab[240].1).abs() < 1e-6, "{}", name);
            assert!((pts[120].length() - (1.0 / half.sin() - 1.0)).abs() < 1e-6, "{}", name);
            let lead = ab[0].0;
            if shape == CornerShape::Circular { assert!((lead - 1.0).abs() < 1e-3, "{}: {}", name, lead); } else { assert!(lead > 1.15 && lead < 2.2, "{}: starts at {}", name, lead); }
            // curvature from three neighbouring points (turning angle over length)
            let curv: Vec<f64> = (1..240).map(|i| {
                let (a, b) = (pts[i] - pts[i - 1], pts[i + 1] - pts[i]);
                -(a.x * b.y - a.y * b.x).atan2(a.dot(b)) / ((a.length() + b.length()) * 0.5)
            }).collect();
            let peak = curv.iter().cloned().fold(0.0, f64::max);
            assert!(curv.iter().all(|k| *k > -1e-6), "{}: curvature changes sign", name);
            if shape != CornerShape::Circular {
                // G2: no curvature where the curve leaves the edges, and a single rise and fall
                assert!(curv[0] < peak * 0.03 && curv[238] < peak * 0.03, "{}: {} of {}", name, curv[0], peak);
                assert!(curv[..120].windows(2).all(|w| w[1] >= w[0] - peak * 0.01) && curv[119..].windows(2).all(|w| w[1] <= w[0] + peak * 0.01), "{}: not one bump", name);
            }
            // G3: the curvature also starts without slope, so its first steps are tiny next to a G2 ramp
            let g3 = matches!(shape, CornerShape::G3One | CornerShape::G3OneRelief | CornerShape::G3Three | CornerShape::G3ThreeRelief);
            if g3 { assert!(curv[4] < peak * 0.02, "{}: curvature rises at once ({} of {})", name, curv[4], peak); }
            if shape == CornerShape::G2Three { assert!(curv[4] > peak * 0.04, "{}", name); }
            // the three-piece shapes have a circular middle, the one-piece shapes a single peak
            let flat = curv.iter().filter(|k| **k > peak * 0.995).count();
            match shape {
                CornerShape::G2Three | CornerShape::G3Three | CornerShape::G3ThreeRelief => assert!(flat > 30, "{}: {}", name, flat),
                CornerShape::G3One | CornerShape::G3OneRelief => assert!(flat < 30, "{}: {}", name, flat),
                CornerShape::Circular => assert!(flat > 230, "{}: {}", name, flat),
            }
        }
    }
}

#[test]
fn smooth_rounds_start_earlier_than_the_circular_one() {
    use freo::kernel::corner::CornerShape;
    let (m, r) = block_with_vertical_edges();
    let v0 = signed_volume(&r.polys);
    let removed = |shape: CornerShape| {
        let mut m = m.clone();
        let FeatureKind::Round(mut rd) = FeatureKind::new_round() else { unreachable!() };
        rd.sets[0].edges = vertical_edges(&r, 1.0);
        rd.sets[0].radius = Dim::v(8.0);
        rd.sets[0].shape = shape;
        m.features.push(feat(11, FeatureKind::Round(rd)));
        let r2 = regen::run(&m);
        assert!(r2.errors.is_empty(), "{:?}: {:?}", shape, r2.errors);
        // the block is 100 × 60: how far the flat +X face still reaches in Z
        let flat = r2.polys.iter().flat_map(|p| p.v.iter()).filter(|v| (v.p.x - 50.0).abs() < 1e-6).map(|v| v.p.z.abs()).fold(0.0, f64::max);
        (v0 - signed_volume(&r2.polys), flat)
    };
    let (circ, flat_c) = removed(CornerShape::Circular);
    assert!((flat_c - 22.0).abs() < 0.05, "circular round ends 8 mm before the corner: {}", flat_c);
    for shape in CornerShape::SMOOTH {
        let (vol, flat) = removed(shape);
        let lead = shape.points(std::f64::consts::FRAC_PI_4, 24)[0].0;
        assert!((flat - (30.0 - 8.0 * lead)).abs() < 0.05, "{:?}: starts {} × 8 mm before the corner, flat to {}", shape, lead, flat);
        assert!(vol > circ * 1.01 && vol < circ * 2.0, "{:?}: circular {} smooth {}", shape, circ, vol);
    }
    // a round saved before shapes existed is circular
    let old: RoundSet = serde_json::from_str(r#"{"edges":[],"radius":"3"}"#).unwrap();
    assert_eq!(old.shape, CornerShape::Circular);
}

fn plate_with_leader_hole(x: f64) -> ModelData {
    let mut m = ModelData::new_part("P");
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(TOP_ID));
    e.sketch = Sketch::rect(120.0, 120.0, 0.0, 0.0);
    e.depth = Dim::v(10.0);
    m.features.push(feat(10, FeatureKind::Extrude(e)));
    let FeatureKind::Extrude(mut c) = FeatureKind::new_extrude() else { unreachable!() };
    c.plane = Some(PlaneRef::Datum(TOP_ID));
    c.sketch = Sketch::circle(3.0, x, 0.0);
    c.depth = Dim::v(100.0);
    c.depth_type = DepthType::Symmetric;
    c.remove = true;
    m.features.push(feat(11, FeatureKind::Extrude(c)));
    m
}

#[test]
fn fill_pattern_inside_a_sketched_region() {
    let mut m = plate_with_leader_hole(0.0);
    let v1 = signed_volume(&regen::run(&m).polys);
    let hole = 120.0 * 120.0 * 10.0 - v1;
    let FeatureKind::Pattern(mut p) = FeatureKind::new_pattern() else { unreachable!() };
    p.features = vec![11];
    p.kind = PatternKind::Fill;
    p.sketch_plane = Some(PlaneRef::Datum(TOP_ID));
    p.sketch = Sketch::rect(90.0, 90.0, 0.0, 0.0);
    p.fill_spacing = Dim::v(20.0);
    m.features.push(feat(12, FeatureKind::Pattern(p.clone())));
    let r = regen::run(&m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let n = ((120.0 * 120.0 * 10.0 - signed_volume(&r.polys)) / hole).round() as usize;
    assert_eq!(n, 25, "5 x 5 square grid");
    // a margin keeps holes away from the region boundary: only the inner 3 x 3 remain
    p.fill_margin = Dim::v(10.0);
    m.features[6].kind = FeatureKind::Pattern(p.clone());
    let r = regen::run(&m);
    let n = ((120.0 * 120.0 * 10.0 - signed_volume(&r.polys)) / hole).round() as usize;
    assert_eq!(n, 9);
    // triangular grid: rows 17.32 apart, alternate rows offset by half a pitch (5 + 2·4 + 2·5)
    p.fill_margin = Dim::v(0.0);
    p.fill_grid = FillGrid::Triangle;
    m.features[6].kind = FeatureKind::Pattern(p);
    let r = regen::run(&m);
    let n = ((120.0 * 120.0 * 10.0 - signed_volume(&r.polys)) / hole).round() as usize;
    assert_eq!(n, 23, "triangle grid count");
}

#[test]
fn curve_pattern_along_a_sketched_line() {
    let mut m = plate_with_leader_hole(-40.0);
    let v1 = signed_volume(&regen::run(&m).polys);
    let hole = 120.0 * 120.0 * 10.0 - v1;
    let FeatureKind::Pattern(mut p) = FeatureKind::new_pattern() else { unreachable!() };
    p.features = vec![11];
    p.kind = PatternKind::Curve;
    p.sketch_plane = Some(PlaneRef::Datum(TOP_ID));
    let mut sk = Sketch::default();
    let (a, b) = (sk.add_point(V2::new(-40.0, 0.0)), sk.add_point(V2::new(40.0, 0.0)));
    sk.add_line(a, b);
    p.sketch = sk;
    p.curve_count = Dim::v(5.0);
    m.features.push(feat(12, FeatureKind::Pattern(p.clone())));
    let r = regen::run(&m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let n = ((120.0 * 120.0 * 10.0 - signed_volume(&r.polys)) / hole).round() as usize;
    assert_eq!(n, 5);
    // the last copy sits at the end of the curve
    let far = r.topo.faces.iter().filter(|f| !f.planar).map(|f| f.center.x).fold(f64::NEG_INFINITY, f64::max);
    assert!((far - 40.0).abs() < 0.5, "last hole at x = {}", far);
    // by spacing: every 16 mm along 80 mm → 5 copies plus the leader
    p.curve_by_spacing = true;
    p.curve_spacing = Dim::v(16.0);
    m.features[6].kind = FeatureKind::Pattern(p);
    let r = regen::run(&m);
    let n = ((120.0 * 120.0 * 10.0 - signed_volume(&r.polys)) / hole).round() as usize;
    assert_eq!(n, 6);
}
