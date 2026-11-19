//! Kosmetisches Gewinde through the same calls the UI makes.
use freo::app::{App, Pick};
use freo::model::features::*;
use freo::sketch::sketch::Sketch;

fn shaft(app: &mut App) {
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.start_feature(FeatureKind::new_extrude());
    let slot = app.sketch.as_ref().unwrap().slot;
    app.finish_sketch(Sketch::circle(5.0, 0.0, 0.0), slot);
    app.update_editor(|f| {
        if let FeatureKind::Extrude(d) = &mut f.kind {
            d.depth = Dim::v(30.0)
        }
    });
    app.finish_feature();
    assert!(app.editor.is_none());
    app.handle_pick(None, false);
}

fn msgs(app: &App) -> Vec<String> {
    app.messages.iter().map(|m| m.text.clone()).collect()
}

#[test]
fn thread_pick_after_start() {
    let mut app = App::new(None);
    shaft(&mut app);
    app.start_feature(FeatureKind::new_thread());
    eprintln!("pick target {:?}", app.editor.as_ref().unwrap().pick);
    let fi = (0..app.result.topo.faces.len()).find(|&f| !app.result.topo.faces[f].planar).unwrap();
    app.handle_pick(Some(Pick::Face(fi)), false);
    eprintln!("kind {:?}", app.editor.as_ref().map(|e| format!("{:?}", e.feature.kind)));
    app.finish_feature();
    eprintln!("{:?}", msgs(&app));
    assert!(app.editor.is_none(), "{:?}", msgs(&app));
    assert_eq!(app.result.cosmetics.len(), 1, "{:?}", msgs(&app));
    // M10 on the Ø10 shaft: turns between the core and the shaft surface, facing outward
    let c = app.result.cosmetics[0].clone();
    assert!(!c.internal);
    let mesh = freo::model::threads::thread_mesh(&c);
    assert!(!mesh.is_empty());
    let dir = c.dir.normalize();
    for (p, n) in mesh.iter().flatten() {
        let radial = *p - c.base - dir * (*p - c.base).dot(dir);
        assert!(radial.length() < 5.0 + 1e-3 && radial.length() > 5.0 - 0.6 * 1.5 - 1e-6);
        if n.dot(dir).abs() < 0.9 {
            assert!(n.dot(radial) >= -1e-9, "faces away from the axis");
        }
    }
}

#[test]
fn thread_preselected_face() {
    let mut app = App::new(None);
    shaft(&mut app);
    let fi = (0..app.result.topo.faces.len()).find(|&f| !app.result.topo.faces[f].planar).unwrap();
    app.handle_pick(Some(Pick::Face(fi)), false);
    app.start_feature(FeatureKind::new_thread());
    app.finish_feature();
    assert!(app.editor.is_none(), "{:?}", msgs(&app));
    assert_eq!(app.result.cosmetics.len(), 1, "{:?}", msgs(&app));
}

fn block_with_hole(app: &mut App, ty: HoleType) {
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.start_feature(FeatureKind::new_extrude());
    let slot = app.sketch.as_ref().unwrap().slot;
    app.finish_sketch(Sketch::rect(60.0, 40.0, 0.0, 0.0), slot);
    app.update_editor(|f| {
        if let FeatureKind::Extrude(d) = &mut f.kind {
            d.depth = Dim::v(20.0)
        }
    });
    app.finish_feature();
    let top = (0..app.result.topo.faces.len()).find(|&f| app.result.topo.faces[f].planar && app.result.topo.planes[app.result.topo.faces[f].polys[0]].n.y > 0.9).unwrap();
    app.handle_pick(Some(Pick::Face(top)), false);
    app.start_feature(FeatureKind::new_hole());
    app.update_editor(|f| {
        if let FeatureKind::Hole(d) = &mut f.kind {
            d.ty = ty;
            d.through_all = true;
        }
    });
    app.finish_feature();
    assert!(app.editor.is_none(), "{:?}", msgs(&app));
    app.handle_pick(None, false);
}

#[test]
fn thread_in_hole() {
    for ty in [HoleType::Simple, HoleType::Standard] {
        let mut app = App::new(None);
        block_with_hole(&mut app, ty);
        let n0 = app.result.cosmetics.len();
        let t = &app.result.topo;
        let curved: Vec<usize> = (0..t.faces.len()).filter(|&f| !t.faces[f].planar).collect();
        eprintln!("{ty:?}: curved faces {:?}", curved);
        app.start_feature(FeatureKind::new_thread());
        app.handle_pick(Some(Pick::Face(curved[0])), false);
        eprintln!("kind {:?} msgs {:?}", app.editor.as_ref().map(|e| format!("{:?}", e.feature.kind)), msgs(&app));
        app.finish_feature();
        assert!(app.editor.is_none(), "{:?}", msgs(&app));
        assert_eq!(app.result.cosmetics.len(), n0 + 1, "{:?}", msgs(&app));
    }
}

#[test]
fn tapped_hole_shows_thread_turns() {
    let mut app = App::new(None);
    block_with_hole(&mut app, HoleType::Standard);
    let c = app.result.cosmetics[0].clone();
    assert!(c.internal && (c.pitch - 1.0).abs() < 1e-9 && (c.wall - 2.5).abs() < 1e-9, "{c:?}");
    let mesh = freo::model::threads::thread_mesh(&c);
    assert!(mesh.len() > 1000, "{} triangles", mesh.len());
    let dir = c.dir.normalize();
    for tri in &mesh {
        for (p, n) in tri {
            let a = (*p - c.base).dot(dir);
            let radial = *p - c.base - dir * a;
            let r = radial.length();
            assert!(a > -1e-9 && a < c.length + 1e-9, "inside the threaded length: {a}");
            assert!(r > 2.5 - 1e-6 && r < 3.0 + 1e-6, "between tap drill and major: {r}");
            // the visible side of the flanks looks into the hole
            if n.dot(dir).abs() < 0.9 {
                assert!(n.dot(radial) <= 1e-9, "faces the axis: a={a} r={r} n={n:?} ndir={}", n.dot(dir));
            }
        }
    }
    // the smooth drill wall is hidden over the thread
    let t = &app.result.topo;
    let wall = (0..t.faces.len()).find(|&f| !t.faces[f].planar).unwrap();
    let pi = t.faces[wall].polys[0];
    let p = &t.polys[pi];
    let tri = [0, 1, 2].map(|j| (t.positions[p[j]], t.vnormals[pi][j]));
    let kept = freo::model::threads::clip_wall(&c, &tri).expect("drill wall is under the thread");
    let area: f64 = kept.iter().map(|q| (q[1].0 - q[0].0).cross(q[2].0 - q[0].0).length()).sum();
    let full = (tri[1].0 - tri[0].0).cross(tri[2].0 - tri[0].0).length();
    assert!(area < full * 0.9, "{area} of {full} left");
}
