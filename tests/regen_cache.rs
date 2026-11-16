//! The regeneration cache gives exactly what a full regeneration gives, whatever changed.
use freo::app::demo::bracket;
use freo::kernel::csg::signed_volume;
use freo::model::features::*;
use freo::model::regen;

fn same(a: &regen::RegenResult, b: &regen::RegenResult, what: &str) {
    assert_eq!(a.polys.len(), b.polys.len(), "{}: polygons", what);
    assert_eq!(signed_volume(&a.polys), signed_volume(&b.polys), "{}: volume", what);
    assert_eq!(
        (a.topo.faces.len(), a.topo.chains.len(), a.planes.len(), a.axes.len(), a.errors.len(), a.rolled_back.len()),
        (b.topo.faces.len(), b.topo.chains.len(), b.planes.len(), b.axes.len(), b.errors.len(), b.rolled_back.len()),
        "{}",
        what
    );
    let mut ea: Vec<_> = a.errors.iter().collect();
    ea.sort();
    let mut eb: Vec<_> = b.errors.iter().collect();
    eb.sort();
    assert_eq!(ea, eb, "{}: errors", what);
}

#[test]
fn cache_matches_a_full_regeneration_through_a_series_of_changes() {
    let mut cache = regen::Cache::default();
    let mut m = bracket();
    same(&cache.run(&m), &regen::run(&m), "first run");
    same(&cache.run(&m), &regen::run(&m), "unchanged");
    // a new feature at the end
    let r = regen::run(&m);
    let e = (0..r.topo.chains.len()).find(|&i| r.topo.chains[i].closed && r.topo.chains[i].convex).unwrap();
    let mut k = FeatureKind::new_chamfer();
    if let FeatureKind::Chamfer(c) = &mut k {
        c.edges = vec![r.topo.edge_ref(e)];
        c.d1 = Dim::v(1.0);
    }
    m.features.push(Feature { id: m.next_id, name: "Fase 1".into(), suppressed: false, hidden: false, kind: k });
    m.next_id += 1;
    same(&cache.run(&m), &regen::run(&m), "feature added");
    // the last feature redefined, then made to fail
    if let FeatureKind::Chamfer(c) = &mut m.features.last_mut().unwrap().kind {
        c.d1 = Dim::v(2.0);
    }
    same(&cache.run(&m), &regen::run(&m), "last feature changed");
    if let FeatureKind::Chamfer(c) = &mut m.features.last_mut().unwrap().kind {
        c.d1 = Dim::v(-1.0);
    }
    let bad = cache.run(&m);
    assert_eq!(bad.errors.len(), 1);
    same(&bad, &regen::run(&m), "last feature fails");
    if let FeatureKind::Chamfer(c) = &mut m.features.last_mut().unwrap().kind {
        c.d1 = Dim::v(1.5);
    }
    same(&cache.run(&m), &regen::run(&m), "error gone again");
    // a parameter, an early feature, suppression, the insert position, deleting
    m.parameters[0].expr = "130".into();
    same(&cache.run(&m), &regen::run(&m), "parameter changed");
    let i = m.features.iter().position(|f| matches!(f.kind, FeatureKind::Extrude(_))).unwrap();
    if let FeatureKind::Extrude(d) = &mut m.features[i].kind {
        d.depth = Dim::v(22.0);
    }
    same(&cache.run(&m), &regen::run(&m), "early feature changed");
    let n = m.features.len();
    m.features[n - 2].suppressed = true;
    same(&cache.run(&m), &regen::run(&m), "feature suppressed");
    m.features[n - 2].suppressed = false;
    m.insert_index = Some(n - 2);
    let rolled = cache.run(&m);
    assert_eq!(rolled.rolled_back.len(), 2);
    same(&rolled, &regen::run(&m), "insert position moved back");
    m.insert_index = None;
    same(&cache.run(&m), &regen::run(&m), "insert position at the end");
    m.features.pop();
    same(&cache.run(&m), &regen::run(&m), "last feature deleted");
    m.features.truncate(8);
    same(&cache.run(&m), &regen::run(&m), "several deleted");
}
