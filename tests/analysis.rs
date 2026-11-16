//! The Analyse commands compute real values and every one of them produces a report.
use freo::app::analysis::{edge_kind, section, thickness};
use freo::app::demo::bracket;
use freo::app::{App, Pick};
use freo::kernel::math::*;
use freo::model::features::*;
use freo::model::regen;
use freo::sketch::sketch::Sketch;

fn plate() -> ModelData {
    // 120 x 60 x 10 plate with a through hole of radius 12
    let mut m = ModelData::new_part("PLATTE");
    let mut k = FeatureKind::new_extrude();
    if let FeatureKind::Extrude(d) = &mut k {
        d.plane = Some(PlaneRef::Datum(TOP_ID));
        d.sketch = Sketch::rect(120.0, 60.0, 0.0, 0.0);
        d.depth = Dim::v(10.0);
    }
    m.features.push(Feature { id: 10, name: "Profil 1".into(), suppressed: false, hidden: false, kind: k });
    let mut k = FeatureKind::new_extrude();
    if let FeatureKind::Extrude(d) = &mut k {
        d.plane = Some(PlaneRef::Datum(TOP_ID));
        d.sketch = Sketch::circle(12.0, 0.0, 0.0);
        d.depth_type = DepthType::ThroughAll;
        d.remove = true;
    }
    m.features.push(Feature { id: 11, name: "Profil 2".into(), suppressed: false, hidden: false, kind: k });
    m.next_id = 12;
    m
}

#[test]
fn section_thickness_and_edge_kinds_are_right() {
    let r = regen::run(&plate());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    // a cut beside the hole: the full 60 x 10 rectangle
    let (a, per, c) = section(&r.polys, V3::X, 40.0);
    assert!((a - 600.0).abs() < 1e-6, "area {}", a);
    assert!((per - 140.0).abs() < 1e-6, "perimeter {}", per);
    assert!((c - V3::new(40.0, 5.0, 0.0)).length() < 1e-6, "centroid {:?}", c);
    // through the middle of the hole: two pieces of 18 x 10
    let (a, _, _) = section(&r.polys, V3::X, 0.0);
    assert!((a - 360.0).abs() < 0.5, "area through the hole {}", a);
    // the plate is 10 thick at its thinnest
    let (lo, _, hi, _) = thickness(&r.polys, &r.topo).unwrap();
    assert!((lo - 10.0).abs() < 1e-6, "thinnest {}", lo);
    assert!(hi > 10.0);
    // 12 straight edges and the two rims of the hole
    let kinds: Vec<(&str, Option<f64>)> = r.topo.chains.iter().map(edge_kind).collect();
    assert_eq!(kinds.iter().filter(|k| k.0 == "Linie").count(), 12);
    let circles: Vec<f64> = kinds.iter().filter(|k| k.0 == "Kreis").filter_map(|k| k.1).collect();
    assert_eq!(circles.len(), 2);
    assert!(circles.iter().all(|r| (r - 12.0).abs() < 0.05), "{:?}", circles);
}

#[test]
fn every_analysis_command_reports() {
    let mut app = App::new(None);
    app.data = bracket();
    app.regenerate(false);
    let names = [
        "Leistungsüberwachung",
        "Benutzerdefinierte Analyse",
        "Kurze Kante",
        "Dicke",
        "Schnittdicke",
        "Querschnitt-Masseneigenschaften",
        "Schnitte",
        "Geometriebericht",
        "Kantentyp",
        "Schräge",
        "Steigung",
        "Abweichwinkel",
        "Konstruktionsrichtung",
        "Globaler Abstand",
        "Globale Durchdringung",
        "Volumendurchdringung",
        "Netzfläche",
        "Knoten",
        "Krümmung",
        "Schattierte Krümmung",
        "Radius",
        "Versatz-Analyse",
        "Schatten",
        "Reflexion",
        "Verbindung",
        "Sensitivitätsanalyse",
        "Durchführbarkeit/Optimierung",
        "Multiziel-Konstruktionsstudie",
        "Statistische Konstruktionsstudie",
        "Bemaßungsberandungen",
    ];
    for n in names {
        app.creo.tools.info = None;
        app.selection.clear();
        app.run_analysis(n);
        let info = app.creo.tools.info.clone().unwrap_or_else(|| panic!("{} gives no report: {:?}", n, app.messages.last().map(|m| m.text.clone())));
        assert!(info.1.lines().count() > 3, "{}: {}", n, info.1);
        assert!(!app.messages.last().is_some_and(|m| m.text.contains("noch nicht verfügbar")), "{} is still a placeholder", n);
    }
    // the bracket's parameters drive its size: more width, more mass
    app.run_analysis("Sensitivitätsanalyse");
    let text = app.creo.tools.info.clone().unwrap().1;
    let rows: Vec<f64> = text.lines().filter(|l| l.starts_with("W = ")).filter_map(|l| l.split_whitespace().nth(3).and_then(|x| x.parse().ok())).collect();
    assert_eq!(rows.len(), 9, "{}", text);
    assert!(rows.windows(2).all(|w| w[1] > w[0]), "mass grows with W: {:?}", rows);
    // Kurze Kante selects the edge it found
    app.run_analysis("Kurze Kante");
    assert!(matches!(app.selection.as_slice(), [Pick::Edge(_)]));
    // Analyse keeps the last report; Gespeicherte Analyse lists it
    app.run_analysis("Analyse");
    assert_eq!(app.creo.tools.saved_analyses.len(), 1);
    app.run_analysis("Gespeicherte Analyse");
    assert!(app.creo.tools.info.clone().unwrap().1.contains("KURZE KANTE"));
}

#[test]
fn deviation_and_point_use_the_selection() {
    let mut app = App::new(None);
    app.data = plate();
    app.regenerate(false);
    let t = app.result.topo.clone();
    let top = (0..t.faces.len()).find(|&f| t.faces[f].planar && t.faces[f].normal.y > 0.99).unwrap();
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.handle_pick(Some(Pick::Face(top)), true);
    app.run_analysis("Abweichung");
    let text = app.creo.tools.info.clone().expect("report").1;
    assert!(text.contains("Größter Abstand zur Referenz:   10"), "{}", text);
    app.selection = vec![Pick::Face(top)];
    app.run_analysis("Punkt");
    assert!(app.creo.tools.info.clone().unwrap().1.contains("Mittelpunkt"));
}

#[test]
fn merge_inherit_adds_the_other_parts_geometry() {
    let mut app = App::new(None);
    app.data = plate();
    app.regenerate(false);
    let v0 = freo::kernel::csg::signed_volume(&app.result.polys);
    // a 20 mm cube standing on the plate, away from the hole
    let mut other = ModelData::new_part("WUERFEL");
    let mut k = FeatureKind::new_extrude();
    if let FeatureKind::Extrude(d) = &mut k {
        d.plane = Some(PlaneRef::Datum(TOP_ID));
        d.sketch = Sketch::rect(20.0, 20.0, 40.0, 0.0);
        d.depth = Dim::v(30.0);
    }
    other.features.push(Feature { id: 10, name: "Profil 1".into(), suppressed: false, hidden: false, kind: k });
    let r = regen::run(&other);
    app.inherit_geometry("WUERFEL", &r);
    assert!(app.result.errors.is_empty(), "{:?}", app.result.errors);
    let v1 = freo::kernel::csg::signed_volume(&app.result.polys);
    // the cube adds its 20 x 20 x 20 above the 10 mm plate
    assert!((v1 - v0 - 8000.0).abs() < 1.0, "{} -> {}", v0, v1);
    app.do_undo();
    assert!((freo::kernel::csg::signed_volume(&app.result.polys) - v0).abs() < 1e-6);
}

fn closed_err(p: &[freo::kernel::csg::CPolygon]) -> f64 {
    let moved: Vec<freo::kernel::csg::CPolygon> = p
        .iter()
        .map(|q| {
            let mut c = q.clone();
            for v in &mut c.v {
                v.p += V3::new(1000.0, 700.0, -500.0);
            }
            c
        })
        .collect();
    freo::kernel::csg::signed_volume(&moved) - freo::kernel::csg::signed_volume(p)
}

#[test]
fn entfernen_removes_a_hole_and_a_round() {
    use freo::kernel::csg::signed_volume;
    // the wall of the through hole: the plate becomes solid again
    let mut app = App::new(None);
    app.data = plate();
    app.regenerate(false);
    let t = app.result.topo.clone();
    let wall = (0..t.faces.len()).find(|&f| !t.faces[f].planar).expect("hole wall");
    app.handle_pick(Some(Pick::Face(wall)), false);
    app.remove_faces();
    assert!(app.editor_error().is_none(), "{:?}", app.editor_error());
    app.finish_feature();
    assert!(app.editor.is_none());
    assert!((signed_volume(&app.result.polys) - 72000.0).abs() < 1e-6, "volume {}", signed_volume(&app.result.polys));
    assert!(closed_err(&app.result.polys).abs() < 1e-6);
    assert_eq!(app.result.topo.faces.len(), 6, "a plain box again");
    assert_eq!(app.data.features.last().unwrap().name, "Entfernen 1");
    // it is a feature: undo brings the hole back
    app.do_undo();
    assert!(signed_volume(&app.result.polys) < 70000.0);

    // a round between two planar faces: the sharp edge comes back
    let mut m = plate();
    m.features.pop();
    let r = regen::run(&m);
    let e = (0..r.topo.chains.len())
        .find(|&i| {
            let c = &r.topo.chains[i];
            c.is_straight() && (c.points[0].y - 10.0).abs() < 1e-9 && (c.points[1].y - 10.0).abs() < 1e-9 && (c.points[0].z - c.points[1].z).abs() < 1e-9
        })
        .unwrap();
    let mut k = FeatureKind::new_round();
    if let FeatureKind::Round(d) = &mut k {
        d.sets[0].edges = vec![r.topo.edge_ref(e)];
        d.sets[0].radius = Dim::v(3.0);
    }
    m.features.push(Feature { id: 20, name: "Rundung 1".into(), suppressed: false, hidden: false, kind: k });
    m.next_id = 21;
    let mut app = App::new(None);
    app.data = m;
    app.regenerate(false);
    assert!(signed_volume(&app.result.polys) < 72000.0 - 100.0);
    let t = app.result.topo.clone();
    let round = (0..t.faces.len()).find(|&f| !t.faces[f].planar).expect("round face");
    app.handle_pick(Some(Pick::Face(round)), false);
    app.remove_faces();
    // the faces beside the round are extended: the full block again
    assert!(app.editor_error().is_none(), "{:?}", app.editor_error());
    app.finish_feature();
    assert!((signed_volume(&app.result.polys) - 72000.0).abs() < 1e-3, "volume {}", signed_volume(&app.result.polys));
    assert!(closed_err(&app.result.polys).abs() < 1e-6);

    // the same for a chamfer, whose own face is flat
    let mut m = plate();
    m.features.pop();
    let mut k = FeatureKind::new_chamfer();
    if let FeatureKind::Chamfer(d) = &mut k {
        d.edges = vec![r.topo.edge_ref(e)];
        d.d1 = Dim::v(4.0);
    }
    m.features.push(Feature { id: 20, name: "Fase 1".into(), suppressed: false, hidden: false, kind: k });
    m.next_id = 21;
    let mut app = App::new(None);
    app.data = m;
    app.regenerate(false);
    let v = signed_volume(&app.result.polys);
    assert!((v - (72000.0 - 8.0 * 120.0)).abs() < 1.0, "chamfered volume {}", v);
    let t = app.result.topo.clone();
    let cf = (0..t.faces.len()).find(|&f| t.faces[f].planar && t.faces[f].normal.y > 0.5 && t.faces[f].normal.y < 0.9).expect("chamfer face");
    app.handle_pick(Some(Pick::Face(cf)), false);
    app.remove_faces();
    assert!(app.editor_error().is_none(), "{:?}", app.editor_error());
    app.finish_feature();
    assert!((signed_volume(&app.result.polys) - 72000.0).abs() < 1e-3, "volume {}", signed_volume(&app.result.polys));
}

#[test]
fn rippenleitkurve_grows_from_its_sketch_down_to_the_model() {
    use freo::kernel::csg::signed_volume;
    // an L: a 120 x 60 x 10 plate with a 10 x 60 x 40 wall at one end
    let mut m = plate();
    m.features.pop();
    let mut k = FeatureKind::new_extrude();
    if let FeatureKind::Extrude(d) = &mut k {
        d.plane = Some(PlaneRef::Datum(TOP_ID));
        d.sketch = Sketch::rect(10.0, 60.0, -55.0, 0.0);
        d.depth = Dim::v(50.0);
    }
    m.features.push(Feature { id: 11, name: "Profil 2".into(), suppressed: false, hidden: false, kind: k });
    // a datum plane 30 above the base, and on it a straight rib path across the plate
    let mut pl = FeatureKind::new_plane();
    if let FeatureKind::DatumPlane(d) = &mut pl {
        d.reference = Some(PlaneRef::Datum(TOP_ID));
        d.kind = PlaneKind::Offset;
        d.offset = Dim::v(30.0);
    }
    m.features.push(Feature { id: 12, name: "DTM1".into(), suppressed: false, hidden: false, kind: pl });
    let v0 = signed_volume(&regen::run(&m).polys);
    let mut sk = Sketch::default();
    let (a, b) = (sk.add_point(V2::new(-50.0, 0.0)), sk.add_point(V2::new(20.0, 0.0)));
    sk.add_line(a, b);
    let mut rib = FeatureKind::new_trajectory_rib();
    if let FeatureKind::Rib(d) = &mut rib {
        d.plane = Some(PlaneRef::Datum(12));
        d.sketch = sk;
        d.thickness = Dim::v(4.0);
    }
    m.features.push(Feature { id: 13, name: "Rippe 1".into(), suppressed: false, hidden: false, kind: rib });
    m.next_id = 14;
    let r = regen::run(&m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    // 70 long, 4 thick, from the plane at 30 down to the plate at 10
    let dv = signed_volume(&r.polys) - v0;
    assert!((dv - 70.0 * 4.0 * 20.0).abs() < 1.0, "rib volume {}", dv);
    assert!(closed_err(&r.polys).abs() < 1e-6);
}
