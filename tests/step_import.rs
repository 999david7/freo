//! STEP / STL / OBJ import and Creo file recognition.

use freo::app::App;
use freo::kernel::math::*;
use freo::model::features::*;
use freo::model::regen;
use freo::model::step_import::{import, import_stl, FaceMesh};
use freo::sketch::sketch::Sketch;
use std::collections::HashMap;

fn volume(faces: &[FaceMesh]) -> f64 {
    faces.iter().flat_map(|f| f.tris.iter().map(move |t| { let (a, b, c) = (f.verts[t[0] as usize].0, f.verts[t[1] as usize].0, f.verts[t[2] as usize].0); a.dot(b.cross(c)) / 6.0 })).sum()
}

/// Number of edges not shared by exactly one opposite-direction neighbour.
fn open_edges(faces: &[FaceMesh]) -> usize {
    let key = |v: V3| ((v.x * 1e4).round() as i64, (v.y * 1e4).round() as i64, (v.z * 1e4).round() as i64);
    let mut e: HashMap<_, i32> = HashMap::new();
    for f in faces { for t in &f.tris { for k in 0..3 {
        let (a, b) = (key(f.verts[t[k] as usize].0), key(f.verts[t[(k + 1) % 3] as usize].0));
        *e.entry(if a < b { (a, b) } else { (b, a) }).or_default() += if a < b { 1 } else { -1 };
    } } }
    e.values().filter(|v| **v != 0).count()
}

/// A cylinder r = 10, h = 20 written like CAD systems do: two planar caps bounded by
/// circles and one periodic cylindrical face with a seam edge; units in metres.
const CYLINDER: &str = "ISO-10303-21;
HEADER;
FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#1))REPRESENTATION_CONTEXT('',''));
#10=CARTESIAN_POINT('',(0.,0.,0.));
#11=CARTESIAN_POINT('',(0.,0.,20.));
#12=DIRECTION('',(0.,0.,1.));
#13=DIRECTION('',(1.,0.,0.));
#14=DIRECTION('',(0.,0.,-1.));
#20=AXIS2_PLACEMENT_3D('',#10,#12,#13);
#21=AXIS2_PLACEMENT_3D('',#11,#12,#13);
#22=AXIS2_PLACEMENT_3D('',#10,#14,#13);
#30=CARTESIAN_POINT('',(10.,0.,0.));
#31=CARTESIAN_POINT('',(10.,0.,20.));
#32=VERTEX_POINT('',#30);
#33=VERTEX_POINT('',#31);
#40=CIRCLE('',#20,10.);
#41=CIRCLE('',#21,10.);
#42=VECTOR('',#12,1.);
#43=LINE('',#30,#42);
#50=EDGE_CURVE('',#32,#32,#40,.T.);
#51=EDGE_CURVE('',#33,#33,#41,.T.);
#52=EDGE_CURVE('',#32,#33,#43,.T.);
#60=ORIENTED_EDGE('',*,*,#50,.T.);
#61=ORIENTED_EDGE('',*,*,#52,.T.);
#62=ORIENTED_EDGE('',*,*,#51,.F.);
#63=ORIENTED_EDGE('',*,*,#52,.F.);
#64=EDGE_LOOP('',(#60,#61,#62,#63));
#65=FACE_OUTER_BOUND('',#64,.T.);
#66=CYLINDRICAL_SURFACE('',#20,10.);
#67=ADVANCED_FACE('',(#65),#66,.T.);
#70=ORIENTED_EDGE('',*,*,#51,.T.);
#71=EDGE_LOOP('',(#70));
#72=FACE_OUTER_BOUND('',#71,.T.);
#73=PLANE('',#21);
#74=ADVANCED_FACE('',(#72),#73,.T.);
#80=ORIENTED_EDGE('',*,*,#50,.F.);
#81=EDGE_LOOP('',(#80));
#82=FACE_OUTER_BOUND('',#81,.T.);
#83=PLANE('',#22);
#84=ADVANCED_FACE('',(#82),#83,.T.);
#90=CLOSED_SHELL('',(#67,#74,#84));
#91=MANIFOLD_SOLID_BREP('CYL',#90);
#92=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#91,#20),#2);
#93=PRODUCT('ZYLINDER','ZYLINDER','',());
ENDSEC;
END-ISO-10303-21;
";

#[test]
fn analytic_cylinder_with_seam_is_watertight() {
    let r = import(CYLINDER).unwrap();
    assert_eq!(r.name, "ZYLINDER");
    assert_eq!(r.faces.len(), 3);
    assert_eq!(r.skipped, 0);
    let exact = std::f64::consts::PI * 100.0 * 20.0;
    let v = volume(&r.faces);
    assert!((v - exact).abs() / exact < 0.01, "volume {} vs {}", v, exact);
    assert_eq!(open_edges(&r.faces), 0);
}

#[test]
fn inch_units_are_converted() {
    let text = CYLINDER.replace("#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));",
        "#1=(CONVERSION_BASED_UNIT('INCH',#3)LENGTH_UNIT()NAMED_UNIT(*));\n#3=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#4);\n#4=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));");
    let r = import(&text).unwrap();
    assert_eq!(r.unit, "inch");
    let exact = std::f64::consts::PI * 100.0 * 20.0 * 25.4f64.powi(3);
    assert!((volume(&r.faces) - exact).abs() / exact < 0.01);
}

fn block() -> (ModelData, regen::RegenResult) {
    let mut m = ModelData::new_part("BLOCK");
    let FeatureKind::Extrude(mut d) = FeatureKind::new_extrude() else { unreachable!() };
    d.plane = Some(PlaneRef::Datum(TOP_ID));
    d.sketch = Sketch::rect(40.0, 30.0, 0.0, 0.0);
    d.depth = Dim::v(10.0);
    m.features.push(Feature { id: 10, name: "Profil 1".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(d) });
    let r = regen::run(&m);
    (m, r)
}

#[test]
fn own_step_and_stl_exports_read_back() {
    let (m, r) = block();
    let step = freo::app::export::step_text(&r, &m.name, "block.stp");
    let back = import(&step).unwrap();
    assert!((volume(&back.faces) - 12_000.0).abs() < 1e-3, "{}", volume(&back.faces));
    assert_eq!(open_edges(&back.faces), 0);

    let dir = std::env::temp_dir().join("freo_step_import");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("block.stl");
    freo::app::export::write(&r, &m, freo::app::export::Format::StlBinary, &p).unwrap();
    let stl = import_stl(&std::fs::read(&p).unwrap()).unwrap();
    assert!((volume(&stl.faces) - 12_000.0).abs() < 1e-2);
    // flat regions are merged into one face each, like the block's six sides
    assert_eq!(stl.faces.len(), 6);
}

#[test]
fn opening_a_step_file_creates_a_part_with_an_import_feature() {
    let dir = std::env::temp_dir().join("freo_step_import");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("zylinder.stp");
    std::fs::write(&p, CYLINDER).unwrap();
    let mut app = App::new(None);
    app.load(p);
    assert_eq!(app.data.name, "ZYLINDER");
    let imp = app.data.features.iter().find(|f| matches!(f.kind, FeatureKind::Import(_))).expect("import feature");
    assert!(app.result.errors.is_empty(), "{:?}", app.result.errors);
    assert!(!app.result.topo.polys.is_empty());
    let m = freo::app::export::mass_properties(&app.result, 1.0);
    assert!((m.volume - std::f64::consts::PI * 2000.0).abs() < 70.0, "{}", m.volume);
    // the imported faces can take further features: a cut through the cylinder
    let id = imp.id;
    let FeatureKind::Extrude(mut d) = FeatureKind::new_extrude() else { unreachable!() };
    d.plane = Some(PlaneRef::Datum(FRONT_ID)); d.sketch = Sketch::rect(4.0, 60.0, 0.0, 0.0); d.depth_type = DepthType::Symmetric; d.depth = Dim::v(40.0); d.remove = true;
    app.commit("cut", |m| { let nid = m.next_id; m.features.push(Feature { id: nid, name: "Schnitt".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(d) }); m.next_id += 1; });
    let cut = freo::app::export::mass_properties(&app.result, 1.0);
    assert!(cut.volume < m.volume - 100.0, "cut removed material: {} -> {}", m.volume, cut.volume);
    assert!(app.data.feature(id).is_some());
}

#[test]
fn creo_files_are_recognised() {
    use freo::app::creo::{is_creo_file, read_creo_header};
    assert!(is_creo_file("bad-organizer-1.prt") && is_creo_file("x.prt.12") && is_creo_file("TOP.ASM.3") && !is_creo_file("x.pfprt"));
    let dir = std::env::temp_dir().join("freo_step_import");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("halter.prt.2");
    std::fs::write(&p, "#UGC:2 PART 2169 1860 803 1 1 15 4100 2024014 000002cc \\\n#- MACH _Windows \\\n#- CMNM 00fHALTER \\\n#-END_OF_UGC_HEADER\n#Creo  TM  10.0  (c) 2026 by PTC Inc.  All Rights Reserved. 10.0.4.0\n").unwrap();
    let (kind, model, version, machine, _) = read_creo_header(&p).unwrap();
    assert_eq!((kind.as_str(), model.as_str(), version.as_str(), machine.as_str()), ("PART", "HALTER", "10.0.4.0", "Windows"));
    // opening it explains instead of failing silently, and keeps the current model
    let mut app = App::new(None);
    let before = app.data.name.clone();
    app.load(p);
    assert!(app.creo.creo_file.is_some());
    assert_eq!(app.data.name, before);
}

#[test]
fn creo_files_show_up_where_parts_are_opened() {
    use freo::app::creo::{is_creo_kind, OpenKind};
    // Oeffnen Part / Importieren list Creo files too (with version numbers)
    for n in ["halter.prt", "HALTER.PRT.3", "x.pfprt"] { assert!(OpenKind::Part.accepts(n), "{}", n); }
    assert!(!OpenKind::Part.accepts("baugruppe.asm"));
    assert!(OpenKind::Assembly.accepts("top.asm.12") && OpenKind::Drawing.accepts("halter.drw.2"));
    for n in ["a.stp", "b.STEP", "c.stl", "d.obj", "e.prt.4"] { assert!(OpenKind::Import.accepts(n), "{}", n); }
    assert!(is_creo_kind("bracket.prt.17", "prt") && !is_creo_kind("bracket.prtx", "prt"));
}
