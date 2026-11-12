//! Live-Simulation solvers checked against textbook results.

use freo::kernel::math::*;
use freo::model::features::*;
use freo::model::regen;
use freo::model::sim::*;
use freo::sim::run::{run_blocking, SimInput, SimOutput};
use freo::sketch::sketch::Sketch;

/// Bar along X: 0..len, section w (Z) × t (Y), bottom on the TOP plane.
fn bar(len: f64, w: f64, t: f64) -> regen::RegenResult {
    let mut m = ModelData::new_part("BAR");
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(TOP_ID));
    e.sketch = Sketch::rect(len, w, len / 2.0, 0.0);
    e.depth = Dim::v(t);
    m.features.push(Feature { id: 10, name: "Profil 1".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(e) });
    let r = regen::run(&m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    r
}

fn face(r: &regen::RegenResult, n: V3) -> SimFace {
    let t = &r.topo;
    let f = (0..t.faces.len()).filter(|&f| t.faces[f].planar && t.faces[f].normal.dot(n) > 0.999).max_by(|&a, &b| t.faces[a].center.dot(n).partial_cmp(&t.faces[b].center.dot(n)).unwrap()).expect("face");
    SimFace::of(t, f)
}

fn item(id: u64, kind: ItemKind, faces: Vec<SimFace>) -> SimItem { let mut i = SimItem::new(id, kind.name().into(), kind); i.faces = faces; i }

fn solve(r: &regen::RegenResult, study: Study, fidelity: f32) -> SimOutput {
    run_blocking(&SimInput { topo: r.topo.clone(), study, fidelity }).expect("solve")
}

fn field<'a>(o: &'a SimOutput, name: &str) -> &'a freo::sim::run::Field { o.fields.iter().find(|f| f.name.starts_with(name)).expect("field") }

#[test]
fn cantilever_deflection_and_stress() {
    let r = bar(100.0, 10.0, 10.0);
    let b = r.topo.bbox;
    assert!((b.size() - V3::new(100.0, 10.0, 10.0)).length() < 1e-6, "{:?}", b);
    let mut s = Study::new(1, "S".into(), StudyKind::Structural);
    s.items.push(item(1, ItemKind::Fixed, vec![face(&r, -V3::X)]));
    let mut f = item(2, ItemKind::Force, vec![face(&r, V3::X)]);
    f.vec = [0.0, -100.0, 0.0];
    s.items.push(f);
    let o = solve(&r, s, 0.3);
    // δ = F L³ / (3 E I) = 0.1905 mm, σ = F L c / I = 60 MPa
    let d = field(&o, "Verschiebung (Betrag)").max as f64;
    assert!((d - 0.1905).abs() < 0.1905 * 0.06, "tip deflection {}", d);
    let vm = field(&o, "Von-Mises").max as f64;
    assert!(vm > 48.0 && vm < 80.0, "root stress {}", vm);
    let sum = |k: &str| o.summary.iter().find(|x| x.0 == k).map(|x| x.1.clone()).unwrap_or_default();
    assert!(sum("Reaktionskraft").starts_with("100"), "reaction {}", sum("Reaktionskraft"));
}

#[test]
fn pressure_and_gravity_resultants() {
    let r = bar(60.0, 20.0, 10.0);
    let mut s = Study::new(1, "S".into(), StudyKind::Structural);
    s.items.push(item(1, ItemKind::Fixed, vec![face(&r, -V3::Y)]));
    let mut p = item(2, ItemKind::Pressure, vec![face(&r, V3::Y)]);
    p.value = 2.0;
    s.items.push(p);
    let o = solve(&r, s.clone(), 0.2);
    // uniaxial compression with the sides free: σ ≈ p, total load p·A = 2400 N
    let applied = o.summary.iter().find(|x| x.0 == "Aufgebrachte Last").unwrap().1.clone();
    assert!(applied.starts_with("2400"), "{}", applied);
    let d = field(&o, "Verschiebung Y").min as f64;
    assert!((d + 2.0 * 10.0 / 210000.0).abs() < 3e-5, "compression {}", d);
    // weight of the bar: 60·20·10 mm³ of steel = 0.0942 kg → 0.924 N
    s.items.pop();
    s.items.push(item(3, ItemKind::Gravity, vec![]));
    let o = solve(&r, s, 0.2);
    let applied = o.summary.iter().find(|x| x.0 == "Aufgebrachte Last").unwrap().1.clone();
    assert!(applied.starts_with("0.92"), "{}", applied);
}

#[test]
fn conduction_is_linear_between_two_temperatures() {
    let r = bar(100.0, 10.0, 10.0);
    let mut s = Study::new(1, "T".into(), StudyKind::Thermal);
    let mut hot = item(1, ItemKind::Temperature, vec![face(&r, -V3::X)]);
    hot.value = 100.0;
    let mut cold = item(2, ItemKind::Temperature, vec![face(&r, V3::X)]);
    cold.value = 0.0;
    s.items = vec![hot, cold];
    let o = solve(&r, s, 0.3);
    let t = field(&o, "Temperatur");
    assert!((t.max - 100.0).abs() < 0.01 && t.min.abs() < 0.01, "{} {}", t.min, t.max);
    for (p, v) in o.surface.pos.iter().zip(&t.vals) { assert!((v - (100.0 - p[0])).abs() < 0.5, "T({}) = {}", p[0], v); }
    // q = k ΔT / L = 50 W/(m·K) · 100 K / 0.1 m = 50 kW/m²
    let q = field(&o, "Wärmefluss");
    assert!((q.max - 50.0).abs() < 1.0 && (q.min - 50.0).abs() < 1.0, "{} {}", q.min, q.max);
}

#[test]
fn convection_balances_heat_flow() {
    let r = bar(50.0, 20.0, 20.0);
    let mut s = Study::new(1, "T".into(), StudyKind::Thermal);
    let mut heat = item(1, ItemKind::HeatFlow, vec![face(&r, -V3::X)]);
    heat.value = 4.0;
    let mut conv = item(2, ItemKind::Convection, vec![face(&r, V3::X)]);
    conv.value = 1000.0;
    conv.value2 = 20.0;
    s.items = vec![heat, conv];
    let o = solve(&r, s, 0.2);
    // 4 W leave through 400 mm² at h = 1000 W/(m²K): surface 10 K above ambient,
    // and 4 W through 50 mm of steel add ΔT = Q L / (k A) = 10 K
    let t = field(&o, "Temperatur");
    assert!((t.min - 30.0).abs() < 0.3, "cold end {}", t.min);
    assert!((t.max - 40.0).abs() < 0.4, "hot end {}", t.max);
}

#[test]
fn cantilever_first_frequency() {
    let r = bar(100.0, 10.0, 10.0);
    let mut s = Study::new(1, "M".into(), StudyKind::Modal);
    s.items.push(item(1, ItemKind::Fixed, vec![face(&r, -V3::X)]));
    s.modes = 3;
    let o = solve(&r, s, 0.4);
    let hz: Vec<f64> = o.summary.iter().filter(|x| x.0.starts_with("Mode")).map(|x| x.1.trim_end_matches(" Hz").parse().unwrap()).collect();
    // Euler–Bernoulli: 835.5 Hz, twice (square section); shear lowers it slightly
    assert_eq!(hz.len(), 3);
    assert!((hz[0] - 835.5).abs() < 835.5 * 0.05 && (hz[1] - 835.5).abs() < 835.5 * 0.05, "{:?}", hz);
    assert!(hz[2] > 3000.0, "{:?}", hz);
}

#[test]
fn thermal_expansion_of_a_free_bar() {
    let r = bar(100.0, 10.0, 10.0);
    let mut s = Study::new(1, "S".into(), StudyKind::Structural);
    s.include_thermal = true;
    s.items.push(item(1, ItemKind::Symmetry, vec![face(&r, -V3::X)]));
    s.items.push(item(2, ItemKind::Symmetry, vec![face(&r, -V3::Y)]));
    s.items.push(item(3, ItemKind::Symmetry, vec![face(&r, -V3::Z)]));
    let mut t = item(4, ItemKind::Temperature, vec![face(&r, V3::X)]);
    t.value = 120.0;
    s.items.push(t);
    let o = solve(&r, s, 0.2);
    // uniform 100 K rise: ΔL = α ΔT L = 1.2e-5 · 100 · 100 = 0.12 mm, no stress
    let ux = field(&o, "Verschiebung X").max as f64;
    assert!((ux - 0.12).abs() < 0.002, "expansion {}", ux);
    assert!(field(&o, "Von-Mises").max < 1.0, "stress {}", field(&o, "Von-Mises").max);
}

#[test]
fn wind_pushes_a_block_downstream() {
    let r = bar(40.0, 40.0, 40.0);
    let mut s = Study::new(1, "F".into(), StudyKind::Fluid);
    let mut w = item(1, ItemKind::Wind, vec![]);
    w.vec = [1.0, 0.0, 0.0];
    w.value = 10.0;
    s.items.push(w);
    let o = solve(&r, s, 0.0);
    let force = o.summary.iter().find(|x| x.0 == "Kraft auf das Teil").unwrap().1.clone();
    let fx: f64 = force.split('(').nth(1).unwrap().split(',').next().unwrap().trim().parse().unwrap();
    // drag of a cube: c_d ≈ 1.05 → ½ ρ v² A c_d = 0.5·1.204·100·0.0016·1.05 ≈ 0.10 N
    assert!(fx > 0.03 && fx < 0.3, "drag {}", force);
    assert!(!o.lines.is_empty(), "stream lines");
    let p = field(&o, "Druck");
    let front: Vec<f32> = o.surface.pos.iter().zip(&p.vals).filter(|x| x.0[0] < 0.01 && x.1.is_finite()).map(|x| *x.1).collect();
    let mean = front.iter().sum::<f32>() / front.len() as f32;
    assert!(mean > 30.0 && mean < 90.0, "pressure on the front face {} (½ρv² = 60 Pa)", mean);
}


/// Plate 60 × 60 × 30 with a Ø20 hole straight through (along Y).
fn drilled_block() -> regen::RegenResult {
    let mut m = ModelData::new_part("ROHR");
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(TOP_ID));
    e.sketch = Sketch::rect(60.0, 60.0, 0.0, 0.0);
    e.depth = Dim::v(30.0);
    m.features.push(Feature { id: 10, name: "Profil 1".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(e) });
    let FeatureKind::Hole(mut h) = FeatureKind::new_hole() else { unreachable!() };
    h.plane = Some(PlaneRef::Face(freo::kernel::topology::FaceRef { point: V3::new(20.0, 30.0, 20.0), normal: V3::Y }));
    h.diameter = Dim::v(20.0);
    h.through_all = true;
    m.features.push(Feature { id: 11, name: "Bohrung 1".into(), suppressed: false, hidden: false, kind: FeatureKind::Hole(h) });
    let r = regen::run(&m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    r
}

#[test]
fn water_flows_through_a_bore() {
    let r = drilled_block();
    let mut s = Study::new(1, "F".into(), StudyKind::Fluid);
    s.fluid = "Wasser".into();
    let mut i = item(1, ItemKind::Inlet, vec![face(&r, V3::Y)]);
    i.value = 2.0;
    s.items.push(i);
    s.items.push(item(2, ItemKind::Outlet, vec![face(&r, -V3::Y)]));
    let o = solve(&r, s, 0.0);
    let get = |k: &str| o.summary.iter().find(|x| x.0.starts_with(k)).map(|x| x.1.clone()).unwrap_or_default();
    assert_eq!(get("Strömung"), "intern");
    let v: f64 = get("Max. Geschwindigkeit").trim_end_matches(" m/s").parse().unwrap();
    assert!(v > 1.5 && v < 5.0, "speed in the bore {}", v);
    let dp: f64 = get("Druckabfall").trim_end_matches(" Pa").parse().unwrap();
    assert!(dp > 0.0, "pressure drop {}", dp);
    assert!(!o.lines.is_empty() && o.see_through);
    // the stream lines stay inside the bore (radius 10 around the Y axis)
    for l in &o.lines { for (p, _) in l { if p.y > 1.0 && p.y < 29.0 { assert!(p.x.hypot(p.z) < 11.5, "stream line leaves the bore at {:?}", p); } } }
}

#[test]
fn study_through_the_app() {
    use freo::app::{App, Pick, Tab};
    let mut app = App::new(None);
    app.data = { let mut m = ModelData::new_part("BAR"); let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() }; e.plane = Some(PlaneRef::Datum(TOP_ID)); e.sketch = Sketch::rect(100.0, 10.0, 50.0, 0.0); e.depth = Dim::v(10.0); m.features.push(Feature { id: 10, name: "Profil 1".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(e) }); m };
    app.regenerate(false);
    app.tab = Tab::LiveSim;
    app.sim_new_study(StudyKind::Structural);
    assert_eq!(app.data.sims.len(), 1);
    let fid = |app: &App, n: V3| { let t = &app.result.topo; (0..t.faces.len()).find(|&f| t.faces[f].planar && t.faces[f].normal.dot(n) > 0.999).unwrap() };
    // object-action: the selected face becomes the reference
    let f0 = fid(&app, -V3::X);
    app.handle_pick(Some(Pick::Face(f0)), false);
    app.sim_new_item(ItemKind::Fixed);
    assert_eq!(app.sim.edit.as_ref().unwrap().item.faces.len(), 1);
    app.sim_finish_item_for_test();
    let f1 = fid(&app, V3::X);
    app.handle_pick(Some(Pick::Face(f1)), false);
    app.sim_new_item(ItemKind::Force);
    app.sim_finish_item_for_test();
    assert_eq!(app.data.sims[0].items.len(), 2);
    // the solve runs beside the UI and arrives through the per-frame tick
    let ctx = eframe::egui::Context::default();
    let t0 = std::time::Instant::now();
    while app.sim.out.as_ref().map_or(true, |o| o.partial) || app.sim.job.is_some() {
        app.sim_tick(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(t0.elapsed().as_secs() < 120, "solve did not finish: {:?}", app.sim.error);
        assert!(app.sim.error.is_none(), "{:?}", app.sim.error);
    }
    let out = app.sim.out.clone().unwrap();
    let d = out.fields.iter().find(|f| f.name.starts_with("Verschiebung (Betrag)")).unwrap().max as f64;
    assert!((d - 0.1905).abs() < 0.02, "tip deflection {}", d);
    // nothing changed: the tick does not solve again
    app.sim_tick(&ctx);
    assert!(app.sim.job.is_none());
    // undo removes the load; the study is solved again and now reports zero
    app.do_undo();
    assert_eq!(app.data.sims[0].items.len(), 1);
    app.sim_tick(&ctx);
    assert!(app.sim.job.is_some(), "a changed study is solved again");
    app.sim_export_csv_to_for_test(&std::env::temp_dir().join("freo_sim_test.csv"));
}
