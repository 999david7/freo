//! Chamfers on circular edges must stay quick and must not cut up the whole model, and the
//! shortcuts that make them quick (local CSG, preview from a snapshot) must give the same result.
use freo::app::demo::bracket;
use freo::kernel::{builders, csg};
use freo::model::features::*;
use freo::model::regen;
use std::time::{Duration, Instant};

/// How much the signed volume changes when the polygons are moved: zero for a closed solid.
fn closed_err(p: &[csg::CPolygon]) -> f64 {
    let moved: Vec<csg::CPolygon> = p
        .iter()
        .map(|q| {
            let mut c = q.clone();
            for v in &mut c.v {
                v.p += freo::kernel::math::V3::new(1000.0, 700.0, -500.0);
            }
            c
        })
        .collect();
    csg::signed_volume(&moved) - csg::signed_volume(p)
}

/// Runs `f` on a thread and fails if it takes longer than `secs`.
fn within<T: Send + 'static>(secs: u64, what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(Duration::from_secs(secs)).unwrap_or_else(|_| panic!("{} hangs (more than {} s)", what, secs))
}

fn with_chamfer_on_all_circles(d: f64) -> ModelData {
    let mut m = bracket();
    let r = regen::run(&m);
    let t = &r.topo;
    let edges: Vec<EdgeRef> = (0..t.chains.len()).filter(|&i| t.chains[i].closed && t.chains[i].convex).map(|i| t.edge_ref(i)).collect();
    assert!(edges.len() > 5, "the bracket has many circular rims");
    let mut k = FeatureKind::new_chamfer();
    if let FeatureKind::Chamfer(c) = &mut k {
        c.edges = edges;
        c.d1 = Dim::v(d);
    }
    m.features.push(Feature { id: m.next_id, name: "Fase 1".into(), suppressed: false, hidden: false, kind: k });
    m.next_id += 1;
    m
}

#[test]
fn chamfer_on_every_circular_rim_is_quick_and_compact() {
    let before = regen::run(&bracket());
    let m = with_chamfer_on_all_circles(1.0);
    let (dt, r) = within(30, "Fase an allen Kreiskanten", move || {
        let s = Instant::now();
        let r = regen::run(&m);
        (s.elapsed(), r)
    });
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(csg::signed_volume(&r.polys) < csg::signed_volume(&before.polys), "a chamfer removes material");
    // before the fix every rim cut the whole model up again (12x the polygons, 17 s)
    assert!(r.polys.len() < before.polys.len() * 6, "{} polygons from {}", r.polys.len(), before.polys.len());
    assert!(dt < Duration::from_secs(5), "took {:?}", dt);
}

#[test]
fn local_csg_matches_the_full_csg_on_every_edge() {
    let r = regen::run(&bracket());
    let mut tags = builders::TagAllocator::default();
    for c in &r.topo.chains {
        for section in [builders::chamfer_section(2.0, 2.0, c.convex), builders::round_section(2.0, 2.0, c.convex)] {
            let tool = builders::edge_tool(c, &section, 0.04, &mut tags, 99);
            let (full, local) =
                if c.convex { (csg::subtract(&r.polys, &tool), csg::subtract_local(&r.polys, &tool)) } else { (csg::union(&r.polys, &tool), csg::union_local(&r.polys, &tool)) };
            let (vf, vl) = (csg::signed_volume(&full), csg::signed_volume(&local));
            // where the full CSG leaves the solid open its volume means little; the local result
            // must then be at least as closed
            let (ef, el) = (closed_err(&full), closed_err(&local));
            if ef.abs() < 1e-3 {
                assert!((vf - vl).abs() <= 1e-6 * vf.abs(), "volume {} vs {}", vf, vl);
            }
            // (where the full result is open itself, its BSP rounding may leave the local one a
            // little more open; a real leak of the local shortcut is far larger)
            let slack = if ef.abs() < 1e-3 { 1e-3 } else { 0.5 };
            assert!(el.abs() <= ef.abs() + slack, "local result is less closed: {} vs {}", el, ef);
            assert!(local.len() <= full.len() + full.len() / 20, "the local result is not cut up more ({} vs {})", local.len(), full.len());
        }
    }
}

#[test]
fn preview_from_a_snapshot_equals_a_full_regeneration() {
    let m = with_chamfer_on_all_circles(1.5);
    let n = m.features.len();
    let full = regen::run_features(&m, &m.features, None);
    for k in [n - 1, n - 3, 7] {
        let (before, extra) = m.features.split_at(k);
        let (_, snap) = regen::run_snapshot(&m, before);
        assert!(snap.fits(&m, before));
        let fast = regen::run_after(&m, &snap, extra);
        assert_eq!(fast.polys.len(), full.polys.len(), "from feature {}", k);
        assert_eq!(csg::signed_volume(&fast.polys), csg::signed_volume(&full.polys));
        assert_eq!((fast.planes.len(), fast.axes.len(), fast.errors.len(), fast.topo.faces.len()), (full.planes.len(), full.axes.len(), full.errors.len(), full.topo.faces.len()));
    }
    // a changed parameter makes the snapshot unusable
    let (_, snap) = regen::run_snapshot(&m, &m.features[..n - 1]);
    let mut m2 = m.clone();
    m2.parameters[0].expr = "130".into();
    assert!(!snap.fits(&m2, &m2.features[..n - 1]));
}
