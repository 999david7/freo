//! IGES and Illustrator import, Umfang / Basislinie dimensions, the extended Referenzen dialog
//! and installed fonts for sketch text.

use freo::app::sketcher::{SketchSession, Tool};
use freo::app::{App, Pick};
use freo::kernel::math::*;
use freo::model::features::*;
use freo::sketch::import2d::{parse_ai, parse_iges, Curve2};
use freo::sketch::sketch::{SkCType, SkKind};

fn session() -> App {
    let mut app = App::new(None);
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.start_feature(FeatureKind::new_extrude());
    app
}
fn sk(app: &mut App) -> &mut SketchSession { app.sketch.as_mut().unwrap() }

/// One IGES record line: 72 data columns, section letter, sequence number.
fn rec(data: &str, sec: char, n: usize) -> String { format!("{:<72}{}{:>7}", data, sec, n) }

fn iges(entities: &[(i32, &str, bool)]) -> String {
    let mut d = vec![];
    let mut p = vec![];
    for (i, (ty, params, dep)) in entities.iter().enumerate() {
        let de = 2 * i + 1;
        let pstart = p.len() + 1;
        // free-format parameters wrap after 64 columns, at a delimiter
        let mut rest = params.to_string();
        while !rest.is_empty() {
            let cut = if rest.len() <= 64 { rest.len() } else { rest[..64].rfind(',').map(|i| i + 1).unwrap_or(64) };
            let n = p.len() + 1;
            p.push(rec(&format!("{:<64} {:>6}", &rest[..cut], de), 'P', n));
            rest = rest[cut..].to_string();
        }
        let status = if *dep { "00010000" } else { "00000000" };
        d.push(rec(&format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}", ty, pstart, 0, 1, 0, 0, 0, 0, status), 'D', de));
        d.push(rec(&format!("{:>8}{:>8}{:>8}{:>8}{:>8}", ty, 0, 0, 1, 0), 'D', de + 1));
    }
    let g = "1H,,1H;,4HTEST,8Htest.igs,4HFREO,4HFREO,32,38,6,308,15,4HTEST,1.0,2,2HMM,1,0.01,13H260101.120000,0.001,1000.0,4Hfreo,4Hfreo,11,0;";
    let mut out = vec![rec("Freo IGES test", 'S', 1), rec(g, 'G', 1)];
    out.extend(d);
    out.extend(p);
    out.push(rec("S      1G      1", 'T', 1));
    out.join("\n")
}

#[test]
fn iges_lines_arcs_bsplines_and_composites() {
    let text = iges(&[
        (110, "110,0.,0.,0.,10.,0.,0.;", false),
        (100, "100,0.,5.,0.,10.,0.,0.,0.;", false),
        (126, "126,2,2,1,0,1,0,0.,0.,0.,1.,1.,1.,1.,1.,1.,20.,0.,0.,25.,10.,0.,30.,0.,0.,0.,1.,0.,0.,1.;", false),
        (110, "110,40.,0.,0.,50.,0.,0.;", true),
        (110, "110,50.,0.,0.,40.,10.,0.;", true),
        (102, "102,2,7,9;", false),
    ]);
    let c = parse_iges(&text).expect("iges");
    assert_eq!(c.len(), 5, "line, arc, spline and the two composite members");
    match &c[1] { Curve2::Arc(cen, r, _, _) => assert!((*cen - V2::new(5.0, 0.0)).length() < 1e-9 && (r - 5.0).abs() < 1e-9), _ => panic!("arc") }
    match &c[2] {
        Curve2::Spline(p) => {
            assert!((p[0] - V2::new(20.0, 0.0)).length() < 1e-9 && (*p.last().unwrap() - V2::new(30.0, 0.0)).length() < 1e-9);
            assert!((p[p.len() / 2] - V2::new(25.0, 5.0)).length() < 1e-6, "quadratic B-spline midpoint");
        }
        _ => panic!("spline"),
    }
    // into a sketch: line + half circle close a region
    let mut app = session();
    let s = sk(&mut app);
    s.import_curves(c.into_iter().take(2).collect()).unwrap();
    assert_eq!(s.sketch.profile_regions().expect("closed").len(), 1);
}

#[test]
fn illustrator_pdf_and_postscript() {
    let content = b"q 1 0 0 1 0 0 cm 0 0 m 72 0 l 72 72 l 0 72 l h f Q 100 0 m 100 50 150 50 150 0 c S";
    let z = miniz_oxide::deflate::compress_to_vec_zlib(content, 6);
    let mut pdf = b"%PDF-1.5\n%AI-test\n1 0 obj\n<< /Length ".to_vec();
    pdf.extend(format!("{} /Filter /FlateDecode >>\nstream\n", z.len()).as_bytes());
    pdf.extend(&z);
    pdf.extend(b"\nendstream\nendobj\n%%EOF\n");
    let c = parse_ai(&pdf).expect("pdf ai");
    assert_eq!(c.iter().filter(|x| matches!(x, Curve2::Line(..))).count(), 4);
    assert_eq!(c.iter().filter(|x| matches!(x, Curve2::Spline(..))).count(), 1);
    if let Curve2::Line(_, b) = c[0] { assert!((b - V2::new(25.4, 0.0)).length() < 1e-9, "72 pt = 25.4 mm"); }

    let ps = b"%!PS-Adobe-3.0\n%%Creator: Adobe Illustrator(R) 8.0\n%%EndSetup\n0 0 m\n72 0 L\n72 72 L\n0 72 L\nf\n";
    let c = parse_ai(ps).expect("ps ai");
    let mut app = session();
    let s = sk(&mut app);
    s.import_curves(c).unwrap();
    let r = s.sketch.profile_regions().expect("closed square");
    assert!((r[0].outer.area().abs() - 25.4 * 25.4).abs() < 1e-6);
}

fn rect(s: &mut SketchSession) {
    s.set_tool(Tool::Rect);
    s.click_at(V2::new(0.0, 0.0)); s.click_at(V2::new(40.0, 30.0));
}

fn line_at(s: &SketchSession, f: impl Fn(V2, V2) -> bool) -> i64 {
    s.sketch.entities.iter().find(|e| e.kind == SkKind::Line && s.sketch.line_ends(e.id).map(|(a, b)| f(a, b)).unwrap_or(false)).unwrap().id
}

fn label_pos(s: &SketchSession, id: i64) -> V2 {
    let c = s.sketch.constraints.iter().find(|c| c.id == id).unwrap();
    freo::app::sketcher::dim_anchor(&s.sketch, c) + c.label
}

#[test]
fn perimeter_with_a_variable_dimension() {
    let mut app = session();
    let s = sk(&mut app);
    rect(s);
    // make width and height strong
    let w = s.sketch.constraints.iter().find(|c| c.weak && c.ty.is_dimension() && (c.value - 40.0).abs() < 1e-4).unwrap().id;
    s.set_dimension(w, "40");
    let h = s.sketch.constraints.iter().find(|c| c.weak && c.ty.is_dimension() && (c.value - 30.0).abs() < 1e-4).unwrap().id;
    s.set_dimension(h, "30");
    let ids: Vec<i64> = s.sketch.entities.iter().filter(|e| e.kind == SkKind::Line).map(|e| e.id).collect();
    s.set_tool(Tool::Perimeter);
    for id in &ids { let (a, b) = s.sketch.line_ends(*id).unwrap(); s.click_at((a + b) * 0.5); }
    let lp = label_pos(s, w);
    s.click_at(lp);
    let per = s.sketch.constraints.iter().find(|c| c.ty == SkCType::Perimeter).expect("perimeter").id;
    assert!(s.sketch.constraints.iter().any(|c| c.id == w && c.var));
    assert!((s.sketch.constraints.iter().find(|c| c.id == per).unwrap().value - 140.0).abs() < 1e-4);
    s.set_dimension(per, "160");
    let bottom = line_at(s, |a, b| a.y.abs() < 1e-6 && b.y.abs() < 1e-6);
    let (a, b) = s.sketch.line_ends(bottom).unwrap();
    assert!(((b - a).length() - 50.0).abs() < 1e-5, "the variable width takes the change: {}", (b - a).length());
    assert!(freo::app::sketcher::dim_text(s.sketch.constraints.iter().find(|c| c.id == w).unwrap()).ends_with("VAR"));
}

#[test]
fn baseline_and_ordinate_dimensions() {
    let mut app = session();
    let s = sk(&mut app);
    rect(s);
    let left = line_at(s, |a, b| a.x.abs() < 1e-6 && b.x.abs() < 1e-6);
    s.set_tool(Tool::Baseline);
    s.click_at(V2::new(0.0, 15.0));
    s.click_at(V2::new(-10.0, 40.0));
    let base = s.sketch.constraints.iter().find(|c| c.ty == SkCType::Baseline).expect("baseline").clone();
    assert_eq!(base.ents, vec![left]);
    assert_eq!(freo::app::sketcher::dim_text(&base), "0.00");
    s.set_tool(Tool::Dimension);
    s.click_at(label_pos(s, base.id));
    s.click_at(V2::new(40.0, 0.0));
    s.click_at(V2::new(40.0, -10.0));
    let ord = s.sketch.constraints.iter().find(|c| c.ty == SkCType::Ordinate).expect("ordinate").clone();
    assert!((ord.value - 40.0).abs() < 1e-4 && ord.base == base.id);
    s.set_dimension(ord.id, "55");
    assert!(s.sketch.points.iter().any(|p| (p.p.x - 55.0).abs() < 1e-5), "the corner moved to 55");
}

#[test]
fn default_references_can_be_deleted_and_points_added() {
    let mut app = App::new(None);
    app.data = freo::app::demo::bracket();
    app.regenerate(false);
    app.handle_pick(Some(Pick::Plane(FRONT_ID)), false);
    app.start_feature(FeatureKind::new_extrude());
    let s = app.sketch.as_mut().unwrap();
    let k = (0..2).find(|k| !s.axis_names[*k].is_empty()).expect("a default reference");
    s.delete_axis_ref(k);
    assert!(s.sketch.axes_off[k]);
    assert!(s.cand_refs.iter().any(|c| c.0 == s.axis_names[k]), "it can be picked again");
    assert!(!s.cand_points.is_empty());
    let p = s.cand_points.iter().find(|c| c.0.contains("BEZUGSACHSE")).or(s.cand_points.first()).unwrap().clone();
    s.set_tool(Tool::AddRef);
    s.click_at(p.1);
    let e = s.sketch.entities.iter().find(|e| !e.ref_name.is_empty()).expect("reference point");
    assert_eq!(e.ref_name, p.0);
    assert!(s.sketch.constraints.iter().any(|c| c.ty == SkCType::Fix && c.pts == e.pts));
    assert!(s.refs_placed());
}

#[test]
fn installed_fonts_are_listed_and_usable() {
    let fonts = freo::sketch::text::system_fonts();
    if fonts.is_empty() { return; }
    let Some((name, _)) = fonts.iter().find(|f| ["Geneva", "Arial Unicode", "Helvetica", "Courier"].contains(&f.0.as_str())) else { return };
    let loops = freo::sketch::text::outline_family("H", name, 0, 1.0, 0.0, V2::ZERO, V2::new(0.0, 10.0));
    assert!(!loops.is_empty(), "outline of H in {}", name);
}
