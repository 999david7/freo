//! CNC: toolpaths for real Freo parts must pass the independent crash check, and the check
//! must catch every kind of unsafe move.

use freo::app::export;
use freo::cam::sim::{IssueKind, Severity};
use freo::cam::*;
use freo::kernel::math::*;
use freo::model::features::*;
use freo::model::regen;
use freo::sketch::sketch::Sketch;
use freo::slicer::mesh::Mesh;
use glam::DVec3;

fn extrude(m: &mut ModelData, id: u64, sk: Sketch, depth: f64, remove: bool, plane: PlaneRef) {
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(plane);
    e.sketch = sk;
    e.depth = Dim::v(depth);
    e.remove = remove;
    m.features.push(Feature { id, name: format!("F{}", id), suppressed: false, hidden: false, kind: FeatureKind::Extrude(e) });
}

fn mesh(m: &ModelData) -> Mesh {
    let r = regen::run(m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    Mesh::from_world_triangles(&export::triangles(&r))
}

/// 60 × 40 × 15 block with a 30 × 16 pocket 6 deep and a Ø6 through hole.
fn bracket() -> Mesh {
    let mut m = ModelData::new_part("CAMTEST");
    extrude(&mut m, 10, Sketch::rect(60.0, 40.0, 0.0, 0.0), 15.0, false, PlaneRef::Datum(TOP_ID));
    let FeatureKind::Extrude(mut p) = FeatureKind::new_extrude() else { unreachable!() };
    p.plane = Some(PlaneRef::Face(FaceRef { point: V3::new(0.0, 15.0, 0.0), normal: V3::Y }));
    p.sketch = Sketch::rect(30.0, 16.0, -8.0, 0.0);
    p.depth = Dim::v(6.0);
    p.remove = true;
    p.flip = true;
    m.features.push(Feature { id: 11, name: "Tasche".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(p) });
    let FeatureKind::Extrude(mut h) = FeatureKind::new_extrude() else { unreachable!() };
    h.plane = Some(PlaneRef::Datum(TOP_ID));
    h.sketch = Sketch::circle(3.0, 20.0, 0.0);
    h.depth_type = DepthType::ThroughAll;
    h.remove = true;
    m.features.push(Feature { id: 12, name: "Bohrung".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(h) });
    mesh(&m)
}

fn setup_for(material: &str, machine: &str) -> Setup {
    let mut s = Setup::default();
    s.material = materials::MATERIALS.iter().position(|m| m.name.starts_with(material)).unwrap();
    s.machine = machines::all_machines().into_iter().find(|m| m.name() == machine).unwrap();
    s
}

fn errors(o: &Output) -> Vec<String> {
    o.sim.issues.iter().filter(|i| i.severity == Severity::Error).map(|i| format!("{:?}: {}", i.kind, i.text)).collect()
}

#[test]
fn bracket_machines_safely_in_aluminium() {
    let part = bracket();
    let (lo, hi) = part.bbox();
    assert!((hi.z - lo.z - 15.0).abs() < 1e-6);
    let mut s = setup_for("Aluminium 6061", "Haas Mini Mill");
    s.auto_tools(hi.x - lo.x, hi.y - lo.y);
    let o = run(&part, &s, &Progress::default()).expect("cam");
    assert!(o.sim.safe(), "{:#?}", errors(&o));
    // facing, drilling (Ø6 hole has no Ø6 drill in the library: none), roughing, finishing
    let kinds: Vec<OpKind> = o.program.ops.iter().map(|p| p.kind).collect();
    assert!(kinds.contains(&OpKind::Face) && kinds.contains(&OpKind::Rough) && kinds.contains(&OpKind::Contour), "{:?}", kinds);
    // the part comes out: almost no material left above it, none removed from it
    let f = &o.sim.field;
    let mut worst_rest = 0.0f32;
    let mut cells = 0;
    for k in 0..f.h.len() {
        let p = o.sim.part[k];
        if p.is_finite() {
            assert!(f.h[k] >= p - 0.011, "gouge at cell {}", k);
            worst_rest = worst_rest.max(f.h[k] - p);
            cells += 1;
        }
    }
    assert!(cells > 1000);
    // the pocket floor and walls are finished: only corner radii and the small hole remain
    assert!(o.sim.rest < 600.0, "rest {}", o.sim.rest);
    // NC code: Z up first, tool change, spindle, end
    let files = nc::write(&o, &s, "CAMTEST", "heute").unwrap();
    assert_eq!(files.len(), 1);
    let g = &files[0].1;
    let first_move = g.lines().find(|l| l.starts_with("G0") || l.starts_with("G1")).unwrap();
    assert!(first_move.starts_with("G0 Z"), "{}", first_move);
    assert!(g.contains("M6") && g.contains("M3") && g.contains("M30") && g.starts_with('%'));
    assert!(g.lines().all(|l| l.is_ascii()));
}

#[test]
fn every_rapid_stays_above_material() {
    // the generator's own promise, checked on the bracket in wood on a router
    let part = bracket();
    let mut s = setup_for("MDF", "Carbide 3D Shapeoko 4 XL");
    let (lo, hi) = part.bbox();
    s.auto_tools(hi.x - lo.x, hi.y - lo.y);
    let o = run(&part, &s, &Progress::default()).expect("cam");
    assert!(o.sim.safe(), "{:#?}", errors(&o));
    assert!(!o.sim.issues.iter().any(|i| i.kind == IssueKind::RapidIntoMaterial));
    // GRBL: one file per tool, each starting with Z up
    let files = nc::write(&o, &s, "CAMTEST", "heute").unwrap();
    let tools: std::collections::BTreeSet<u32> = o.program.ops.iter().map(|p| p.tool.number).collect();
    assert!(files.len() >= tools.len());
    for (_, g) in &files {
        assert!(!g.contains("M6"), "GRBL has no tool changer");
        let first = g.lines().find(|l| l.starts_with("G0") || l.starts_with("G1")).unwrap();
        assert!(first.starts_with("G0 Z"));
    }
}

#[test]
fn checker_catches_crashes() {
    let part = bracket();
    let s = setup_for("Aluminium 6061", "Haas Mini Mill");
    let job = Job::new(&part, &s).unwrap();
    let tool = s.tool(6).unwrap().clone();
    let mk = |moves: Vec<CMove>| Program {
        ops: vec![OpPath { name: "Test".into(), kind: OpKind::Rough, tool: tool.clone(), rpm: 8000.0, feed: 500.0, coolant: false, moves }],
        safe_z: 10.0,
        notes: vec![],
    };
    let c = (job.stock_lo + job.stock_hi) / 2.0;
    let rapid = |p: DVec3| CMove::line(p, true, 0.0);
    let feed = |p: DVec3| CMove::line(p, false, 500.0);
    let run_check = |p: &Program| sim::simulate(&job, &s, p, &Progress::default()).unwrap();
    // 1. rapid straight down into the stock
    let p = mk(vec![rapid(DVec3::new(c.x, c.y, 10.0)), rapid(DVec3::new(c.x, c.y, -3.0))]);
    let r = run_check(&p);
    assert!(r.issues.iter().any(|i| i.kind == IssueKind::RapidIntoMaterial && i.severity == Severity::Error));
    // 2. feeding into the finished part
    let p = mk(vec![rapid(DVec3::new(c.x, c.y, 10.0)), feed(DVec3::new(c.x, c.y, job.part_lo.z + 1.0))]);
    let r = run_check(&p);
    assert!(r.issues.iter().any(|i| i.kind == IssueKind::Gouge));
    // 3. into the table
    let out = DVec3::new(job.stock_lo.x + 1.0, job.stock_lo.y + 1.0, 10.0);
    let p = mk(vec![rapid(out), feed(DVec3::new(out.x, out.y, job.stock_lo.z - 1.0))]);
    let r = run_check(&p);
    assert!(r.issues.iter().any(|i| i.kind == IssueKind::Table));
    // 4. a Ø2 cutter on a 3.175 shank, 3 mm flutes, plunged 10 mm: the thicker shank and
    //    then the holder hit the stock (an end mill whose shank equals its diameter could
    //    follow its own hole, which is no collision)
    let mut deep = s.tool(2).unwrap().clone();
    assert!(deep.shank > deep.diameter);
    deep.flute_length = 3.0;
    deep.stickout = 5.0;
    let p = Program {
        ops: vec![OpPath { name: "Kurz".into(), kind: OpKind::Rough, tool: deep, rpm: 8000.0, feed: 500.0, coolant: false, moves: vec![rapid(out), feed(DVec3::new(out.x, out.y, -10.0))] }],
        safe_z: 10.0,
        notes: vec![],
    };
    let r = run_check(&p);
    assert!(r.issues.iter().any(|i| i.kind == IssueKind::Shank) && r.issues.iter().any(|i| i.kind == IssueKind::Holder), "{:?}", r.issues.iter().map(|i| i.kind).collect::<Vec<_>>());
    // 5. no NC code for an unsafe program
    let bad = Output { job: job.clone(), program: p.clone(), sim: r };
    assert!(nc::write(&bad, &s, "X", "heute").is_err());
}

#[test]
fn unsuitable_machine_and_material_is_an_error() {
    let part = bracket();
    let s = setup_for("Titan Grade 5", "Carbide 3D Shapeoko 4 XL");
    let o = run(&part, &s, &Progress::default()).expect("cam");
    assert!(o.sim.issues.iter().any(|i| i.kind == IssueKind::MachineUnsuited && i.severity == Severity::Error));
    assert!(nc::write(&o, &s, "X", "heute").is_err());
}

#[test]
fn curved_part_gets_3d_finish_without_gouges() {
    // a revolved dome: 3D finishing with the ball end mill must follow it, never into it
    let mut m = ModelData::new_part("DOME");
    extrude(&mut m, 10, Sketch::rect(50.0, 50.0, 0.0, 0.0), 5.0, false, PlaneRef::Datum(TOP_ID));
    let FeatureKind::Revolve(mut r) = FeatureKind::new_revolve() else { unreachable!() };
    r.plane = Some(PlaneRef::Datum(FRONT_ID));
    let mut sk = Sketch::default();
    let o = sk.add_point(V2::new(0.0, 5.0));
    let a = sk.add_point(V2::new(15.0, 5.0));
    let t = sk.add_point(V2::new(0.0, 20.0));
    // quarter circle R15 standing on the plate (y from 5), closed along the axis
    sk.add_line(o, a);
    sk.add_arc(o, a, t);
    sk.add_line(t, o);
    let c0 = sk.add_point(V2::new(0.0, 0.0));
    let c1 = sk.add_point(V2::new(0.0, 30.0));
    sk.add_entity(freo::sketch::sketch::SkKind::Line, vec![c0, c1], 0.0, false, true);
    r.sketch = sk;
    m.features.push(Feature { id: 11, name: "Kuppel".into(), suppressed: false, hidden: false, kind: FeatureKind::Revolve(r) });
    let part = mesh(&m);
    let mut s = setup_for("POM-C", "Haas Mini Mill");
    s.ops.iter_mut().for_each(|o| {
        if o.kind == OpKind::Parallel {
            o.enabled = true;
        }
    });
    s.auto_tools(50.0, 50.0);
    let o = run(&part, &s, &Progress::default()).expect("cam");
    assert!(o.sim.safe(), "{:#?}", errors(&o));
    assert!(o.program.ops.iter().any(|p| p.kind == OpKind::Parallel));
}

/// Freo's demo bracket: a 50 mm boss next to counterbored holes and rounded edges, deeper
/// than the standard cutters reach. Everything the holder could hit is left standing.
#[test]
fn demo_bracket_passes_the_check() {
    let mut app = freo::app::App::new(None);
    app.demo_setup("tab:model");
    let r = regen::run(&app.data);
    let part = Mesh::from_world_triangles(&export::triangles(&r));
    for (mat, machine) in [("Aluminium 6061", "Haas Mini Mill"), ("MDF", "Carbide 3D Shapeoko 4 XL"), ("POM-C", "Makera Carvera")] {
        let mut s = setup_for(mat, machine);
        s.ops.iter_mut().for_each(|o| o.enabled = true);
        let (lo, hi) = part.bbox();
        s.auto_tools(hi.x - lo.x, hi.y - lo.y);
        let o = run(&part, &s, &Progress::default()).expect("cam");
        let errs: Vec<String> = errors(&o).into_iter().filter(|e| !e.starts_with("StockTooTall") && !e.starts_with("Travel")).collect();
        assert!(errs.is_empty(), "{} on {}: {:#?}", mat, machine, errs);
    }
}

/// 100 × 50 × 15 plate with a 30 × 20 upright 30 mm tall: the default Ø6 cutter sticks out
/// only 28 mm, so with that length its collet would hit the upright when cutting the plate.
fn plate_with_upright() -> Mesh {
    let mut m = ModelData::new_part("UPRIGHT");
    extrude(&mut m, 10, Sketch::rect(100.0, 50.0, 0.0, 0.0), 15.0, false, PlaneRef::Datum(TOP_ID));
    let FeatureKind::Extrude(mut u) = FeatureKind::new_extrude() else { unreachable!() };
    u.plane = Some(PlaneRef::Face(FaceRef { point: V3::new(0.0, 15.0, 0.0), normal: V3::Y }));
    u.sketch = Sketch::rect(30.0, 20.0, 10.0, 0.0);
    u.depth = Dim::v(30.0);
    m.features.push(Feature { id: 11, name: "Steg".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(u) });
    mesh(&m)
}

#[test]
fn tall_walls_get_a_longer_stickout_instead_of_leftover_material() {
    let part = plate_with_upright();
    let mut s = setup_for("Aluminium 6061", "Haas Mini Mill");
    for op in &mut s.ops {
        if matches!(op.kind, OpKind::Rough | OpKind::Contour) {
            op.tool = 6;
        }
    }
    assert!(s.tool(6).unwrap().stickout < 30.0, "the test needs a short default stick-out");
    let o = run(&part, &s, &Progress::default()).expect("cam");
    assert!(o.sim.safe(), "{:#?}", errors(&o));
    // the plate around the upright is cleared: almost nothing stays (was > 10 cm³)
    assert!(o.sim.rest < 300.0, "rest {} mm³", o.sim.rest);
    let rough = o.program.ops.iter().find(|p| p.kind == OpKind::Rough).unwrap();
    assert!(rough.tool.stickout >= 45.0 - 1e-6, "stick-out {}", rough.tool.stickout);
    assert!(o.program.notes.iter().any(|n| n.contains("Auskraglänge")), "{:?}", o.program.notes);
    let f = &o.sim.field;
    for k in 0..f.h.len() {
        let p = o.sim.part[k];
        if p.is_finite() {
            assert!(f.h[k] >= p - 0.011, "gouge at cell {}", k);
        }
    }
    // the NC header tells the operator how far to clamp the tool out
    let g = &nc::write(&o, &s, "UPRIGHT", "heute").unwrap()[0].1;
    assert!(g.contains(&format!("Auskragung {}", rough.tool.stickout.round() as i64)) || g.contains("Auskragung 47"), "{}", g.lines().take(12).collect::<Vec<_>>().join("\n"));
}

#[test]
fn sloped_faces_are_detected_for_3d_finishing() {
    assert!(!freo::cam::generate::has_slopes(&plate_with_upright()));
    // a wedge has a 45° face
    let mut m = ModelData::new_part("WEDGE");
    let mut sk = Sketch::default();
    let pts: Vec<_> = [(0.0, 0.0), (40.0, 0.0), (0.0, 40.0)].iter().map(|&(x, y)| sk.add_point(V2::new(x, y))).collect();
    for i in 0..3 {
        sk.add_line(pts[i], pts[(i + 1) % 3]);
    }
    extrude(&mut m, 10, sk, 10.0, false, PlaneRef::Datum(FRONT_ID));
    assert!(freo::cam::generate::has_slopes(&mesh(&m)));
}

/// 60 × 40 × 10 plate with rounded vertical corners (R8) and a Ø13 through hole (no Ø13 drill
/// in the library).
fn plate_with_hole() -> Mesh {
    let mut m = ModelData::new_part("BORE");
    extrude(&mut m, 10, Sketch::rect(60.0, 40.0, 0.0, 0.0), 10.0, false, PlaneRef::Datum(TOP_ID));
    let FeatureKind::Extrude(mut h) = FeatureKind::new_extrude() else { unreachable!() };
    h.plane = Some(PlaneRef::Datum(TOP_ID));
    h.sketch = Sketch::circle(6.5, 10.0, 0.0);
    h.depth_type = DepthType::ThroughAll;
    h.remove = true;
    m.features.push(Feature { id: 11, name: "Loch".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(h) });
    let FeatureKind::Round(mut rd) = FeatureKind::new_round() else { unreachable!() };
    let r = regen::run(&m);
    rd.sets[0].edges = (0..r.topo.chains.len())
        .filter(|&i| {
            let c = &r.topo.chains[i];
            c.is_straight() && (c.points[0].y - c.points.last().unwrap().y).abs() > 9.0
        })
        .map(|i| r.topo.edge_ref(i))
        .collect();
    rd.sets[0].radius = Dim::v(8.0);
    m.features.push(Feature { id: 12, name: "Rundung".into(), suppressed: false, hidden: false, kind: FeatureKind::Round(rd) });
    mesh(&m)
}

#[test]
fn holes_are_bored_with_a_helix_and_rounds_cut_as_arcs() {
    let part = plate_with_hole();
    let mut s = setup_for("Aluminium 6061", "Haas Mini Mill");
    for op in &mut s.ops {
        if matches!(op.kind, OpKind::Rough | OpKind::Contour) {
            op.tool = 6;
        }
    }
    let o = run(&part, &s, &Progress::default()).expect("cam");
    assert!(o.sim.safe(), "{:#?}", errors(&o));
    let rough = o.program.ops.iter().find(|p| p.kind == OpKind::Rough).unwrap();
    let job = Job::new(&part, &s).unwrap();
    let hole = freo::cam::generate::find_holes(&job);
    assert_eq!(hole.len(), 1);
    // the hole: a helix (arcs that go down) around its centre
    let helix: Vec<&CMove> = rough.moves.iter().filter(|m| m.arc.is_some_and(|c| (c[0] - hole[0].0).hypot(c[1] - hole[0].1) < 0.05)).collect();
    assert!(helix.len() > 10, "no helical bore: {} arcs at the hole", helix.len());
    let mut prev = DVec3::ZERO;
    let mut descends = 0;
    for m in &rough.moves {
        if m.arc.is_some() && m.to.z < prev.z - 1e-6 {
            descends += 1;
        }
        prev = m.to;
    }
    assert!(descends > 5, "the bore must go down along arcs");
    // the rounded corners are finished with arcs
    let contour = o.program.ops.iter().find(|p| p.kind == OpKind::Contour).unwrap();
    assert!(contour.moves.iter().filter(|m| m.arc.is_some()).count() >= 4, "corner rounds should be arcs");
    // no gouges, the hole is open to the bottom of the part, almost no rest
    let f = &o.sim.field;
    for k in 0..f.h.len() {
        let p = o.sim.part[k];
        if p.is_finite() {
            assert!(f.h[k] >= p - 0.011, "gouge at cell {}", k);
        }
    }
    let (i, j) = (((hole[0].0 - f.x0) / f.cell) as usize, ((hole[0].1 - f.y0) / f.cell) as usize);
    assert!((f.h[j * f.nx + i] as f64) < job.part_lo.z + 1e-3, "the hole centre is cut through");
    assert!(o.sim.rest < 100.0, "rest {}", o.sim.rest);
    // the NC code has the arcs as G2/G3 with I and J
    let g = &nc::write(&o, &s, "BORE", "heute").unwrap()[0].1;
    assert!(g.lines().any(|l| l.starts_with("G3") && l.contains(" I") && l.contains(" J")));
}

#[test]
fn arcs_are_followed_exactly() {
    let from = DVec3::new(10.0, 0.0, 0.0);
    let mv = CMove { to: DVec3::new(-10.0, 0.0, -2.0), rapid: false, feed: 500.0, arc: Some([0.0, 0.0, 1.0]) };
    let pts = mv.points(from);
    assert!(pts.len() > 20);
    for p in &pts {
        assert!(((p.x * p.x + p.y * p.y).sqrt() - 10.0).abs() < 1e-9);
        assert!(p.y >= -1e-9, "counter-clockwise from +x to -x runs through +y");
    }
    assert_eq!(*pts.last().unwrap(), mv.to);
    assert!((CMove::sweep(from, mv.to, [0.0, 0.0, -1.0]) + std::f64::consts::PI).abs() < 1e-9);
}

#[test]
fn finishing_in_3d_only_where_the_part_slopes() {
    // flat floors and vertical walls only: nothing for the ball cutter to do
    let part = plate_with_upright();
    let mut s = setup_for("Aluminium 6061", "Haas Mini Mill");
    for op in &mut s.ops {
        if op.kind == OpKind::Parallel {
            op.enabled = true;
        }
    }
    let o = run(&part, &s, &Progress::default()).expect("cam");
    assert!(o.sim.safe(), "{:#?}", errors(&o));
    let feeds = o.program.ops.iter().filter(|p| p.kind == OpKind::Parallel).flat_map(|p| p.moves.iter()).filter(|m| !m.rapid).count();
    assert_eq!(feeds, 0, "3D finishing ran over a part without slopes");
}
