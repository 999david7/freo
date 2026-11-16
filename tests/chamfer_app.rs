//! Chamfer on hole edges through the same calls the UI makes; every step must stay quick.
use freo::app::{App, Pick, PickTarget};
use freo::kernel::math::*;
use freo::model::features::*;
use freo::sketch::sketch::Sketch;
use std::time::Instant;

fn plate_with_holes(app: &mut App) {
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.start_feature(FeatureKind::new_extrude());
    let s = app.sketch.as_ref().unwrap();
    let slot = s.slot;
    app.finish_sketch(Sketch::rect(120.0, 60.0, 0.0, 0.0), slot);
    app.update_editor(|f| {
        if let FeatureKind::Extrude(d) = &mut f.kind {
            d.depth = Dim::v(10.0)
        }
    });
    app.finish_feature();
    app.handle_pick(Some(Pick::Plane(TOP_ID)), false);
    app.start_feature(FeatureKind::new_extrude());
    let slot = app.sketch.as_ref().unwrap().slot;
    let mut sk = Sketch::default();
    for x in [-30.0, 30.0] {
        let c = sk.add_point(V2::new(x, 0.0));
        sk.add_circle(c, 12.0);
    }
    app.finish_sketch(sk, slot);
    app.update_editor(|f| {
        if let FeatureKind::Extrude(d) = &mut f.kind {
            d.remove = true;
            d.depth_type = DepthType::ThroughAll;
        }
    });
    app.finish_feature();
    assert!(app.editor.is_none(), "{:?}", app.messages.last().map(|m| m.text.clone()));
}

fn step<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let s = Instant::now();
    let r = f();
    let dt = s.elapsed();
    eprintln!("{:<40} {:>10.1?}", what, dt);
    assert!(dt.as_secs_f64() < 3.0, "{} takes {:?}", what, dt);
    r
}

#[test]
fn chamfer_hole_edges_stays_responsive() {
    let mut app = App::new(None);
    plate_with_holes(&mut app);
    let t = app.result.topo.clone();
    let circles: Vec<usize> = (0..t.chains.len()).filter(|&i| t.chains[i].closed).collect();
    assert_eq!(circles.len(), 4, "two holes, top and bottom rim");
    // all four rims selected (shift-click), then Fase: the edges go into the chamfer
    step("erste Kante wählen", || app.handle_pick(Some(Pick::Edge(circles[0])), false));
    for &c in &circles[1..] {
        step(&format!("Kante {} dazu wählen", c), || app.handle_pick(Some(Pick::Edge(c)), true));
    }
    step("Fase starten (mit Vorschau)", || app.start_feature(FeatureKind::new_chamfer()));
    app.set_pick_target(Some(PickTarget::Edges));
    for d in [2.0, 5.0, 1.0] {
        step(&format!("Fasenmaß {} eingeben", d), || {
            app.update_editor(|f| {
                if let FeatureKind::Chamfer(c) = &mut f.kind {
                    c.d1 = Dim::v(d)
                }
            })
        });
    }
    let n = match &app.editor.as_ref().unwrap().feature.kind {
        FeatureKind::Chamfer(c) => c.edges.len(),
        _ => 0,
    };
    assert_eq!(n, 4, "all four rims are in the chamfer");
    let v0 = freo::kernel::csg::signed_volume(&app.result.polys);
    step("OK", || app.finish_feature());
    assert!(app.editor.is_none(), "{:?}", app.messages.last().map(|m| m.text.clone()));
    let v1 = freo::kernel::csg::signed_volume(&app.result.polys);
    eprintln!("Volumen {} -> {}", v0, v1);
    assert!(v1 < v0, "a chamfer on hole rims removes material");
    step("Regenerieren", || app.regenerate(true));
}
