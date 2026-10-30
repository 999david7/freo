//! Builds sample parts through the full regeneration pipeline, prints volumes
//! and renders PNGs: `cargo run --bin kernel_demo -- <out_dir>`.

use freo::kernel::csg::signed_volume;
use freo::kernel::math::*;
use freo::model::features::*;
use freo::model::regen::{self, RegenResult};
use freo::sketch::sketch::Sketch;
use freo::softrender;

fn feat(id: u64, name: &str, kind: FeatureKind) -> Feature { Feature { id, name: name.into(), suppressed: false, hidden: false, kind } }

fn extrude(plane: PlaneRef, sk: Sketch, depth: f64, remove: bool) -> FeatureKind {
    let FeatureKind::Extrude(mut d) = FeatureKind::new_extrude() else { unreachable!() };
    d.plane = Some(plane); d.sketch = sk; d.depth = Dim::v(depth); d.remove = remove;
    FeatureKind::Extrude(d)
}

fn top_face(y: f64) -> PlaneRef { PlaneRef::Face(FaceRef { point: V3::new(0.0, y, 0.0), normal: V3::Y }) }

fn report(name: &str, r: &RegenResult, out: &str) {
    println!("{:<28} polys {:>5}  volume {:>12}  {:>6.0} ms  {}", name, r.polys.len(), fmt_num(signed_volume(&r.polys), 1), r.time * 1000.0,
        if r.errors.is_empty() { String::new() } else { format!("{:?}", r.errors) });
    let img = softrender::render(r, 640, 480, [0.62, 0.68, 0.78]);
    std::fs::write(format!("{}/{}.png", out, name), softrender::encode_png(&img)).unwrap();
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    std::fs::create_dir_all(&out).unwrap();

    // 1. Bracket: plate + hole pattern + rounds + shell
    let mut m = ModelData::new_part("BRACKET");
    m.features.push(feat(10, "Extrude 1", extrude(PlaneRef::Datum(TOP_ID), Sketch::rect(120.0, 80.0, 0.0, 0.0), 20.0, false)));
    report("01_block", &regen::run(&m), &out);
    let FeatureKind::Hole(mut h) = FeatureKind::new_hole() else { unreachable!() };
    h.plane = Some(top_face(20.0)); h.x = Dim::v(-40.0); h.y = Dim::v(20.0); h.ty = HoleType::Counterbore; h.through_all = true;
    m.features.push(feat(11, "Hole 1", FeatureKind::Hole(h)));
    let FeatureKind::Pattern(mut p) = FeatureKind::new_pattern() else { unreachable!() };
    p.features = vec![11]; p.dir1 = AxisRef::WorldX; p.count1 = Dim::v(3.0); p.spacing1 = Dim::v(40.0);
    p.use_dir2 = true; p.dir2 = AxisRef::WorldZ; p.count2 = Dim::v(2.0); p.spacing2 = Dim::v(40.0);
    m.features.push(feat(12, "Pattern 1", FeatureKind::Pattern(p)));
    let r = regen::run(&m);
    report("02_hole_pattern", &r, &out);
    let FeatureKind::Round(mut rd) = FeatureKind::new_round() else { unreachable!() };
    rd.sets[0].radius = Dim::v(8.0);
    for (i, c) in r.topo.chains.iter().enumerate() {
        if c.is_straight() && c.points.iter().all(|q| q.x.abs() > 59.9 || q.z.abs() > 39.9) && (c.points[0].y - c.points.last().unwrap().y).abs() > 1.0 {
            rd.sets[0].edges.push(r.topo.edge_ref(i));
        }
    }
    m.features.push(feat(13, "Round 1", FeatureKind::Round(rd)));
    report("03_rounds", &regen::run(&m), &out);
    let FeatureKind::Shell(mut sh) = FeatureKind::new_shell() else { unreachable!() };
    sh.thickness = Dim::v(3.0); sh.faces = vec![FaceRef { point: V3::new(0.0, 0.0, 0.0), normal: -V3::Y }];
    m.features.push(feat(14, "Shell 1", FeatureKind::Shell(sh)));
    report("04_shell", &regen::run(&m), &out);

    // 2. Revolved flange with axial bolt pattern and chamfer
    let mut m = ModelData::new_part("FLANGE");
    let mut s = Sketch::default();
    let pts = [(10.0, 0.0), (60.0, 0.0), (60.0, 12.0), (25.0, 12.0), (25.0, 45.0), (10.0, 45.0)];
    let ids: Vec<i64> = pts.iter().map(|&(x, y)| s.add_point(V2::new(x, y))).collect();
    for i in 0..ids.len() { s.add_line(ids[i], ids[(i + 1) % ids.len()]); }
    let (a, b) = (s.add_point(V2::new(0.0, -10.0)), s.add_point(V2::new(0.0, 60.0)));
    s.add_entity(freo::sketch::sketch::SkKind::Line, vec![a, b], 0.0, false, true);
    let FeatureKind::Revolve(mut rv) = FeatureKind::new_revolve() else { unreachable!() };
    rv.plane = Some(PlaneRef::Datum(FRONT_ID)); rv.sketch = s;
    m.features.push(feat(10, "Revolve 1", FeatureKind::Revolve(rv)));
    let FeatureKind::Hole(mut h) = FeatureKind::new_hole() else { unreachable!() };
    h.plane = Some(top_face(12.0)); h.x = Dim::v(45.0); h.diameter = Dim::v(8.0); h.through_all = true;
    m.features.push(feat(11, "Hole 1", FeatureKind::Hole(h)));
    let FeatureKind::Pattern(mut p) = FeatureKind::new_pattern() else { unreachable!() };
    p.features = vec![11]; p.kind = PatternKind::Axis; p.axis = AxisRef::Axis(10_001); p.axis_count = Dim::v(6.0); p.axis_angle = Dim::v(60.0);
    m.features.push(feat(12, "Pattern 1", FeatureKind::Pattern(p)));
    report("05_flange", &regen::run(&m), &out);

    // 3. Sweep (pipe) and blend
    let mut m = ModelData::new_part("PIPE");
    let mut t = Sketch::default();
    let p0 = t.add_point(V2::new(0.0, 0.0));
    let p1 = t.add_point(V2::new(60.0, 0.0));
    let p2 = t.add_point(V2::new(60.0, 50.0));
    let p3 = t.add_point(V2::new(110.0, 80.0));
    t.add_line(p0, p1); t.add_line(p1, p2); t.add_line(p2, p3);
    let mut sec = Sketch::circle(8.0, 0.0, 0.0);
    let c = sec.add_point(V2::ZERO);
    sec.add_circle(c, 5.0);
    m.features.push(feat(10, "Sweep 1", FeatureKind::Sweep(SweepDef { plane: Some(PlaneRef::Datum(FRONT_ID)), trajectory: t, section: sec, remove: false, mode: SweepMode::Normal, section2: Sketch::default() })));
    report("06_sweep_pipe", &regen::run(&m), &out);

    let mut m = ModelData::new_part("BLEND");
    m.features.push(feat(10, "Blend 1", FeatureKind::Blend(BlendDef {
        plane: Some(PlaneRef::Datum(TOP_ID)),
        sections: vec![Sketch::rect(60.0, 60.0, 0.0, 0.0), Sketch::circle(15.0, 0.0, 0.0)],
        offsets: vec![Dim::v(50.0)], flip: false, remove: false, rotational: false,
    })));
    report("07_blend", &regen::run(&m), &out);

    // 4. Parametric: same block driven by relations
    let mut m = ModelData::new_part("PARAM");
    m.parameters.push(Parameter { name: "W".into(), expr: "150".into(), note: String::new() });
    m.relations = "H = W / 3".into();
    let FeatureKind::Extrude(mut e) = extrude(PlaneRef::Datum(TOP_ID), Sketch::rect(150.0, 50.0, 0.0, 0.0), 10.0, false) else { unreachable!() };
    e.depth = Dim("H".into());
    m.features.push(feat(10, "Extrude 1", FeatureKind::Extrude(e)));
    let r = regen::run(&m);
    println!("relation-driven depth H = {} -> volume {}", r.vars["H"], fmt_num(signed_volume(&r.polys), 1));
}
