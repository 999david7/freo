//! 2D drawings (DXF) to 3D parts: drawings Freo makes of known parts must give those parts
//! back (same size and shape), and hand-written AutoCAD-style files with polylines, blocks,
//! line types, dimensions and units must be read correctly.

use freo::app::drawui::NewDrawing;
use freo::app::App;
use freo::kernel::csg::{bbox, signed_volume, subtract, CPlane, CPolygon};
use freo::kernel::math::*;
use freo::model::drawing::{Orientation, SheetSize};
use freo::model::drawing3d::{analyze, build, read_dxf, Options, Projection, Role};
use freo::model::features::*;
use freo::model::regen;
use freo::sketch::sketch::Sketch;

fn ext(m: &mut ModelData, id: u64, plane: PlaneRef, sk: Sketch, depth: f64, remove: bool, through: bool, flip: bool) {
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(plane);
    e.sketch = sk;
    e.depth = Dim::v(depth);
    e.remove = remove;
    e.flip = flip;
    if through {
        e.depth_type = DepthType::ThroughAll;
        e.side2 = Side2::ThroughAll;
    }
    m.features.push(Feature { id, name: format!("F{}", id), suppressed: false, hidden: false, kind: FeatureKind::Extrude(e) });
    m.next_id = id + 1;
}

/// A3 drawing with Freo's template views (front, top, side, isometric) and the model's
/// dimensions, as DXF.
fn dxf_of(m: &ModelData, proj: Projection) -> String {
    let mut app = App::new(None);
    app.data = m.clone();
    app.regenerate(false);
    app.new_drawing(&NewDrawing { name: "D".into(), size: SheetSize::A3, orient: Orientation::Landscape, template: true, projection: proj });
    let ids: Vec<u32> = app.drw.as_ref().unwrap().data.sheets[0].views.iter().map(|v| v.id).collect();
    app.show_model_dims(&ids);
    let (prims, _) = app.sheet_scene(0, &[]);
    freo::app::publish::dxf(&prims)
}

fn moved(polys: &[CPolygon], off: V3) -> Vec<CPolygon> {
    polys
        .iter()
        .map(|p| {
            let mut q = p.clone();
            for v in &mut q.v {
                v.p += off;
            }
            q.plane = CPlane::newell(&q.v.iter().map(|v| v.p).collect::<Vec<_>>()).unwrap_or(q.plane);
            q
        })
        .collect()
}

/// Imports the drawing and checks the part against the original: same size, and the two
/// solids differ by less than `tol` of the volume (tessellation of curves).
fn round_trip(orig: &ModelData, dxf: &str, tol: f64) -> (Vec<String>, ModelData) {
    let r0 = regen::run(orig);
    assert!(r0.errors.is_empty(), "{:?}", r0.errors);
    let d = read_dxf(dxf).unwrap();
    let a = analyze(&d, &Options::default()).unwrap();
    let (m, notes) = build(&a, "T").unwrap();
    let r = regen::run(&m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let (b0, b) = (bbox(&r0.polys), bbox(&r.polys));
    assert!(((b.max - b.min) - (b0.max - b0.min)).length() < 0.01, "size {:?} instead of {:?}", b.max - b.min, b0.max - b0.min);
    let mv = moved(&r.polys, b0.min - b.min);
    let v0 = signed_volume(&r0.polys);
    let extra = signed_volume(&subtract(&mv, &r0.polys));
    let missing = signed_volume(&subtract(&r0.polys, &mv));
    assert!(extra < v0 * tol && missing < v0 * tol, "extra {} missing {} of {}", extra, missing, v0);
    (notes, m)
}

#[test]
fn freo_drawings_give_the_part_back() {
    // a block with a blind pocket in the top (depth only in hidden lines) and a horizontal
    // through hole seen as a circle in the front view, in both projection methods
    let mut m = ModelData::new_part("P1");
    ext(&mut m, 10, PlaneRef::Datum(TOP_ID), Sketch::rect(80.0, 40.0, 0.0, 0.0), 30.0, false, false, false);
    ext(&mut m, 11, PlaneRef::Face(FaceRef { point: V3::new(0.0, 30.0, 0.0), normal: V3::Y }), Sketch::rect(30.0, 16.0, -15.0, 0.0), 10.0, true, false, true);
    ext(&mut m, 12, PlaneRef::Datum(FRONT_ID), Sketch::circle(5.0, 22.0, 12.0), 0.0, true, true, false);
    for proj in [Projection::First, Projection::Third] {
        let (notes, part) = round_trip(&m, &dxf_of(&m, proj), 2e-5);
        assert!(notes.iter().any(|n| n.contains("rahmen")), "{:?}", notes);
        // editable features: a profile and cuts, the pocket at its depth
        assert!(part.features.iter().any(|f| matches!(&f.kind, FeatureKind::DatumPlane(d) if (d.offset.value(&Default::default(), 0.0) - 20.0).abs() < 1e-9)));
    }
}

#[test]
fn round_boss_counterbores_and_scale() {
    // Freo's demo bracket: a round boss with a hole on a plate, four counterbored holes
    let b = freo::app::demo::bracket();
    let (_, part) = round_trip(&b, &dxf_of(&b, Projection::First), 1e-4);
    // the holes are real circles of the right size in the sketches
    let mut radii: Vec<f64> = part
        .features
        .iter()
        .filter_map(|f| if let FeatureKind::Extrude(e) = &f.kind { Some(e.sketch.entities.iter().filter(|x| x.kind == freo::sketch::sketch::SkKind::Circle).map(|x| x.r).collect::<Vec<_>>()) } else { None })
        .flatten()
        .collect();
    radii.sort_by(|a, b| a.partial_cmp(b).unwrap());
    radii.dedup();
    assert_eq!(radii, vec![4.5, 7.5, 8.0, 18.0]);
    // a part the template draws at 1:4
    let mut m = ModelData::new_part("BIG");
    ext(&mut m, 10, PlaneRef::Datum(TOP_ID), Sketch::rect(600.0, 300.0, 0.0, 0.0), 120.0, false, false, false);
    ext(&mut m, 11, PlaneRef::Datum(TOP_ID), Sketch::circle(40.0, 150.0, 0.0), 0.0, true, true, false);
    let dx = dxf_of(&m, Projection::First);
    let a = analyze(&read_dxf(&dx).unwrap(), &Options::default()).unwrap();
    assert!((a.scale - 4.0).abs() < 1e-9, "scale {}", a.scale);
    round_trip(&m, &dx, 2e-5);
}

#[test]
fn uprights_with_slot_chamfers_and_rounded_corners() {
    // like a school bracket: plate with two R17 corners and two holes, two uprights with a
    // U-slot (R10) between them and chamfered tops
    let mut m = ModelData::new_part("U");
    let mut sk = Sketch::default();
    // plate outline (top view: x across, y = -z): back edge straight, front corners R17
    let p = |sk: &mut Sketch, x: f64, y: f64| sk.add_point(V2::new(x, y));
    let a = p(&mut sk, -50.0, 25.0);
    let b = p(&mut sk, 50.0, 25.0);
    let c = p(&mut sk, 50.0, -8.0);
    let d = p(&mut sk, 33.0, -25.0);
    let e = p(&mut sk, -33.0, -25.0);
    let f = p(&mut sk, -50.0, -8.0);
    let c1 = p(&mut sk, 33.0, -8.0);
    let c2 = p(&mut sk, -33.0, -8.0);
    sk.add_line(a, b);
    sk.add_line(b, c);
    sk.add_arc(c1, d, c);
    sk.add_line(d, e);
    sk.add_arc(c2, f, e);
    sk.add_line(f, a);
    ext(&mut m, 10, PlaneRef::Datum(TOP_ID), sk, 12.0, false, false, false);
    // uprights: front view outline from y 12 to 45, 74 wide, slot R10, chamfers 8
    let mut up = Sketch::default();
    let q: Vec<i64> = [(-37.0, 12.0), (37.0, 12.0), (37.0, 37.0), (29.0, 45.0), (10.0, 45.0), (10.0, 30.0), (-10.0, 30.0), (-10.0, 45.0), (-29.0, 45.0), (-37.0, 37.0)].iter().map(|&(x, y)| up.add_point(V2::new(x, y))).collect();
    for i in 0..q.len() {
        if i == 5 {
            continue;
        }
        up.add_line(q[i], q[(i + 1) % q.len()]);
    }
    let cc = up.add_point(V2::new(0.0, 30.0));
    up.add_arc(cc, q[6], q[5]);
    // from a plane 10 mm behind FRONT, 15 mm back to the plate's back edge (z −10 … −25)
    let mut dp = DatumPlaneDef::builtin(PlaneKind::Offset);
    dp.reference = Some(PlaneRef::Datum(FRONT_ID));
    dp.offset = Dim::v(-10.0);
    m.features.push(Feature { id: 14, name: "DTM1".into(), suppressed: false, hidden: false, kind: FeatureKind::DatumPlane(dp) });
    ext(&mut m, 11, PlaneRef::Datum(14), up, 15.0, false, false, true);
    // the holes in front of the uprights (z = +8; top sketch y = −z)
    ext(&mut m, 12, PlaneRef::Datum(TOP_ID), Sketch::circle(6.5, -25.0, -8.0), 0.0, true, true, false);
    ext(&mut m, 13, PlaneRef::Datum(TOP_ID), Sketch::circle(6.5, 25.0, -8.0), 0.0, true, true, false);
    m.next_id = 15;
    let r = regen::run(&m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    for proj in [Projection::First, Projection::Third] {
        round_trip(&m, &dxf_of(&m, proj), 2e-4);
    }
}

// ---------------------------------------------------------------- AutoCAD-style files

/// Minimal AutoCAD-style DXF writer for the tests.
struct Dxf(String);

impl Dxf {
    fn new(units: i32) -> Dxf {
        let mut d = Dxf(String::new());
        d.g(0, "SECTION");
        d.g(2, "HEADER");
        d.g(9, "$INSUNITS");
        d.g(70, &units.to_string());
        d.g(0, "ENDSEC");
        d.g(0, "SECTION");
        d.g(2, "TABLES");
        d.g(0, "TABLE");
        d.g(2, "LAYER");
        for (n, lt) in [("Kontur", "CONTINUOUS"), ("Verdeckt", "HIDDEN2"), ("Mitte", "CENTER"), ("Bemassung", "CONTINUOUS")] {
            d.g(0, "LAYER");
            d.g(2, n);
            d.g(6, lt);
        }
        d.g(0, "ENDTAB");
        d.g(0, "ENDSEC");
        d
    }
    fn g(&mut self, c: i32, v: &str) {
        self.0.push_str(&format!("{}\n{}\n", c, v));
    }
    fn line(&mut self, layer: &str, a: (f64, f64), b: (f64, f64)) {
        self.g(0, "LINE");
        self.g(8, layer);
        self.g(10, &a.0.to_string());
        self.g(20, &a.1.to_string());
        self.g(11, &b.0.to_string());
        self.g(21, &b.1.to_string());
    }
    fn circle(&mut self, layer: &str, c: (f64, f64), r: f64) {
        self.g(0, "CIRCLE");
        self.g(8, layer);
        self.g(10, &c.0.to_string());
        self.g(20, &c.1.to_string());
        self.g(40, &r.to_string());
    }
    /// closed LWPOLYLINE: (x, y, bulge to the next vertex)
    fn poly(&mut self, layer: &str, v: &[(f64, f64, f64)]) {
        self.g(0, "LWPOLYLINE");
        self.g(8, layer);
        self.g(90, &v.len().to_string());
        self.g(70, "1");
        for (x, y, b) in v {
            self.g(10, &x.to_string());
            self.g(20, &y.to_string());
            if *b != 0.0 {
                self.g(42, &b.to_string());
            }
        }
    }
    fn text(&mut self, p: (f64, f64), t: &str) {
        self.g(0, "TEXT");
        self.g(8, "Bemassung");
        self.g(10, &p.0.to_string());
        self.g(20, &p.1.to_string());
        self.g(40, "3.5");
        self.g(1, t);
    }
    fn dim(&mut self, a: (f64, f64), b: (f64, f64), measured: f64) {
        self.g(0, "DIMENSION");
        self.g(8, "Bemassung");
        self.g(70, "1");
        self.g(13, &a.0.to_string());
        self.g(23, &a.1.to_string());
        self.g(14, &b.0.to_string());
        self.g(24, &b.1.to_string());
        self.g(42, &measured.to_string());
    }
    fn finish(mut self) -> String {
        self.g(0, "EOF");
        self.0
    }
}

/// Plate 120 × 60 × 10 with two R10 front corners and two Ø12 holes 80 apart, drawn at 1:2
/// in two views (front above, top below: European), the holes as a block reference.
fn plate_drawing() -> String {
    let mut d = Dxf::new(4);
    // block with one hole circle and its centre lines
    d.g(0, "SECTION");
    d.g(2, "BLOCKS");
    d.g(0, "BLOCK");
    d.g(2, "LOCH");
    d.g(10, "0");
    d.g(20, "0");
    d.circle("Kontur", (0.0, 0.0), 3.0);
    d.line("Mitte", (-4.5, 0.0), (4.5, 0.0));
    d.g(0, "ENDBLK");
    d.g(0, "ENDSEC");
    d.g(0, "SECTION");
    d.g(2, "ENTITIES");
    // front view (half size): 60 × 5 at y 50
    d.poly("Kontur", &[(0.0, 50.0, 0.0), (60.0, 50.0, 0.0), (60.0, 55.0, 0.0), (0.0, 55.0, 0.0)]);
    for x in [7.0, 13.0, 47.0, 53.0] {
        d.line("Verdeckt", (x, 50.0), (x, 55.0));
    }
    // top view below (half size): 60 × 30, front edge at the bottom with R5 corners
    let b = (std::f64::consts::PI / 8.0).tan(); // 90° arc
    d.poly("Kontur", &[(0.0, 40.0, 0.0), (0.0, 15.0, b), (5.0, 10.0, 0.0), (55.0, 10.0, b), (60.0, 15.0, 0.0), (60.0, 40.0, 0.0)]);
    for x in [10.0, 50.0] {
        d.g(0, "INSERT");
        d.g(8, "0");
        d.g(2, "LOCH");
        d.g(10, &x.to_string());
        d.g(20, "25");
    }
    // dimensions: the drawn 60 says 120
    d.dim((0.0, 40.0), (60.0, 40.0), 120.0);
    d.dim((0.0, 10.0), (0.0, 40.0), 60.0);
    d.text((20.0, 0.0), "M 1:2");
    d.finish()
}

#[test]
fn autocad_style_drawing_with_blocks_bulges_and_dimensions() {
    let d = read_dxf(&plate_drawing()).unwrap();
    assert_eq!(d.dims.len(), 2);
    let a = analyze(&d, &Options::default()).unwrap();
    assert!((a.scale - 2.0).abs() < 1e-9, "scale {} ({})", a.scale, a.scale_source);
    assert_eq!(a.projection, Projection::First);
    assert!(a.roles.iter().any(|r| matches!(r.1, Role::Plan { below: true })));
    let (m, _) = build(&a, "PLATTE").unwrap();
    let r = regen::run(&m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let b = bbox(&r.polys);
    assert!(((b.max - b.min) - V3::new(120.0, 10.0, 60.0)).length() < 1e-6, "{:?}", b.max - b.min);
    // plate minus the two R10 corners and the two Ø12 holes
    let corners = 2.0 * (100.0 - std::f64::consts::PI * 100.0 / 4.0);
    let holes = 2.0 * std::f64::consts::PI * 36.0;
    let expect = (120.0 * 60.0 - corners - holes) * 10.0;
    let v = signed_volume(&r.polys);
    assert!((v - expect).abs() < expect * 2e-3, "volume {} expected {}", v, expect);
    // the corners are arcs and the holes circles in the sketches
    let kinds: Vec<_> = m.features.iter().filter_map(|f| if let FeatureKind::Extrude(e) = &f.kind { Some(e.sketch.entities.iter().map(|x| x.kind).collect::<Vec<_>>()) } else { None }).flatten().collect();
    assert_eq!(kinds.iter().filter(|k| **k == freo::sketch::sketch::SkKind::Arc).count(), 2);
    assert_eq!(kinds.iter().filter(|k| **k == freo::sketch::sketch::SkKind::Circle).count(), 2);
    // the first and the last hole lie 80 mm apart, 20 mm from the sides
    let centres: Vec<f64> = m
        .features
        .iter()
        .filter_map(|f| if let FeatureKind::Extrude(e) = &f.kind { e.sketch.entities.iter().find(|x| x.kind == freo::sketch::sketch::SkKind::Circle).map(|c| e.sketch.point(c.pts[0]).x) } else { None })
        .collect();
    assert_eq!(centres.len(), 2);
    assert!(((centres[0] - centres[1]).abs() - 80.0).abs() < 1e-6, "{:?}", centres);
}

#[test]
fn a_single_view_becomes_a_plate_of_the_noted_thickness() {
    let mut d = Dxf::new(1);
    d.g(0, "SECTION");
    d.g(2, "ENTITIES");
    // 4 × 2 inch plate with a Ø1 inch hole, "t=0.25" (inches)
    d.poly("Kontur", &[(0.0, 0.0, 0.0), (4.0, 0.0, 0.0), (4.0, 2.0, 0.0), (0.0, 2.0, 0.0)]);
    d.circle("Kontur", (2.0, 1.0), 0.5);
    d.text((0.0, -1.0), "t=0.25");
    let d = read_dxf(&d.finish()).unwrap();
    let a = analyze(&d, &Options::default()).unwrap();
    assert!((a.scale - 25.4).abs() < 1e-9);
    assert!((a.thickness.unwrap() - 6.35).abs() < 1e-9);
    let (m, notes) = build(&a, "BLECH").unwrap();
    assert!(notes.iter().any(|n| n.contains("Platte")), "{:?}", notes);
    let r = regen::run(&m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let expect = (101.6 * 50.8 - std::f64::consts::PI * 12.7 * 12.7) * 6.35;
    let v = signed_volume(&r.polys);
    assert!((v - expect).abs() < expect * 2e-3, "volume {} expected {}", v, expect);
    // a thickness given in the dialog wins
    let a = analyze(&d, &Options { thickness: Some(3.0), ..Default::default() }).unwrap();
    assert_eq!(a.thickness, Some(3.0));
}

#[test]
fn dwg_needs_a_converter_or_says_how() {
    let dir = std::env::temp_dir().join(format!("freo-dwg-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("teil.dwg");
    std::fs::write(&p, b"AC1032 not really a drawing").unwrap();
    match freo::model::drawing3d::import_file(&p, &Options::default()) {
        Ok(_) => {}
        Err(e) => assert!(e.contains("DWG") || e.contains("DXF"), "{}", e),
    }
    let _ = std::fs::remove_dir_all(&dir);
}
