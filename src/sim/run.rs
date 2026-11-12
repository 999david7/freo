//! Runs a study: meshes the part, turns loads and constraints into solver input, solves and
//! samples the results on a fine copy of the part's surface for display.

use super::fea::{self, Ctl, StructIn, ThermalIn};
use super::fluid;
use super::mesh::{tris_of, Mesh};
use crate::kernel::math::*;
use crate::kernel::topology::Topology;
use crate::model::sim::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub struct SimInput {
    pub topo: Arc<Topology>,
    pub study: Study,
    /// Leistungsoptionen: 0 = fast, 1 = accurate
    pub fidelity: f32,
}

/// A result quantity sampled at every vertex of the display surface.
pub struct Field {
    pub name: String,
    pub unit: String,
    pub vals: Vec<f32>,
    pub min: f32,
    pub max: f32,
    /// deformation shown with this field (index into `SimOutput::disps`)
    pub disp: Option<usize>,
    /// low values are the critical ones (Sicherheitsfaktor)
    pub low_is_bad: bool,
}

impl Field {
    fn new(name: &str, unit: &str, vals: Vec<f32>, disp: Option<usize>) -> Field {
        let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
        for v in &vals { if v.is_finite() { min = min.min(*v); max = max.max(*v); } }
        if min > max { min = 0.0; max = 0.0; }
        Field { name: name.into(), unit: unit.into(), vals, min, max, disp, low_is_bad: false }
    }
}

/// Displacement of the display surface and its edges (mm; mode shapes are scaled to 1).
pub struct DispSet { pub d: Vec<[f32; 3]>, pub edge_d: Vec<[f32; 3]>, pub max: f32, pub harmonic: bool }

#[derive(Default)]
pub struct Surface {
    /// triangles, three vertices each
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    /// line segments, two vertices each
    pub edges: Vec<[f32; 3]>,
}

pub struct SimOutput {
    pub kind: StudyKind,
    pub surface: Arc<Surface>,
    pub fields: Vec<Field>,
    pub disps: Vec<DispSet>,
    /// stream lines: (position, speed m/s)
    pub lines: Vec<Vec<(V3, f32)>>,
    pub summary: Vec<(String, String)>,
    pub warnings: Vec<String>,
    /// numeric results for export: column names and one row per mesh node / fluid cell
    pub table: (Vec<String>, Vec<Vec<f32>>),
    /// an intermediate picture of a run still going on
    pub partial: bool,
    pub seconds: f64,
    /// the part is drawn see-through (flow inside it)
    pub see_through: bool,
}

pub struct Shared {
    pub progress: AtomicU32,
    pub cancel: AtomicBool,
    pub stage: Mutex<String>,
    pub partial: Mutex<Option<SimOutput>>,
    pub done: Mutex<Option<Result<SimOutput, String>>>,
}

impl Shared {
    pub fn progress(&self) -> f32 { f32::from_bits(self.progress.load(Ordering::Relaxed)) }
    fn stage(&self, s: &str) { *self.stage.lock().unwrap() = s.into(); }
}

/// Starts the study on its own thread.
pub fn spawn(input: SimInput) -> Arc<Shared> {
    let sh = Arc::new(Shared { progress: AtomicU32::new(0), cancel: AtomicBool::new(false), stage: Mutex::new("Vernetzen".into()), partial: Mutex::new(None), done: Mutex::new(None) });
    let s2 = sh.clone();
    std::thread::spawn(move || {
        let t0 = std::time::Instant::now();
        let r = run(&input, &s2).map(|mut o| { o.seconds = t0.elapsed().as_secs_f64(); o });
        *s2.done.lock().unwrap() = Some(r);
    });
    sh
}

/// Runs the study on the calling thread (tests, MCP).
pub fn run_blocking(input: &SimInput) -> Result<SimOutput, String> {
    let sh = Shared { progress: AtomicU32::new(0), cancel: AtomicBool::new(false), stage: Mutex::new(String::new()), partial: Mutex::new(None), done: Mutex::new(None) };
    run(input, &sh)
}

/// Element budget for a fidelity setting.
pub fn budget(kind: StudyKind, fidelity: f32) -> usize {
    let f = fidelity.clamp(0.0, 1.0) as f64;
    match kind {
        StudyKind::Structural => (3500.0 * 40f64.powf(f)) as usize,
        StudyKind::Modal => (2500.0 * 24f64.powf(f)) as usize,
        StudyKind::Thermal => (8000.0 * 40f64.powf(f)) as usize,
        StudyKind::Fluid => (50_000.0 * 14f64.powf(f)) as usize,
    }
}

fn fmt(v: f64) -> String {
    let a = v.abs();
    if a == 0.0 { "0".into() } else if a >= 1e5 || a < 1e-3 { format!("{:.3e}", v) } else if a >= 100.0 { format!("{:.1}", v) } else if a >= 1.0 { format!("{:.3}", v) } else { format!("{:.4}", v) }
}

fn f3(v: V3) -> [f32; 3] { [v.x as f32, v.y as f32, v.z as f32] }

/// Fine triangles and edge pieces of the part (no edge longer than `max_edge`).
fn build_surface(topo: &Topology, mut max_edge: f64) -> (Surface, Vec<V3>) {
    fn split(p: [V3; 3], n: [V3; 3], max2: f64, depth: u32, out: &mut Surface) {
        let l = [(p[1] - p[0]).length_squared(), (p[2] - p[1]).length_squared(), (p[0] - p[2]).length_squared()];
        let k = if l[0] >= l[1] && l[0] >= l[2] { 0 } else if l[1] >= l[2] { 1 } else { 2 };
        if l[k] <= max2 || depth == 0 {
            for i in 0..3 { out.pos.push(f3(p[i])); out.nrm.push(f3(n[i])); }
            return;
        }
        let (a, b, c) = (k, (k + 1) % 3, (k + 2) % 3);
        let (pm, nm) = ((p[a] + p[b]) * 0.5, (n[a] + n[b]).normalize_or_zero());
        split([p[a], pm, p[c]], [n[a], nm, n[c]], max2, depth - 1, out);
        split([pm, p[b], p[c]], [nm, n[b], n[c]], max2, depth - 1, out);
    }
    let area: f64 = topo.faces.iter().map(|f| f.area).sum();
    // keep the display mesh below roughly 250 000 triangles
    max_edge = max_edge.max((area / 250_000.0 * 2.3).sqrt());
    let mut s = Surface::default();
    let mut face_n = vec![];
    for (pi, p) in topo.polys.iter().enumerate() {
        for k in 1..p.len() - 1 {
            let before = s.pos.len();
            split([topo.positions[p[0]], topo.positions[p[k]], topo.positions[p[k + 1]]], [topo.vnormals[pi][0], topo.vnormals[pi][k], topo.vnormals[pi][k + 1]], max_edge * max_edge, 16, &mut s);
            face_n.resize(face_n.len() + s.pos.len() - before, topo.planes[pi].n);
        }
    }
    for &(a, b) in &topo.draw_edges {
        let (pa, pb) = (topo.positions[a], topo.positions[b]);
        let n = (((pb - pa).length() / (max_edge * 1.5)).ceil() as usize).clamp(1, 400);
        for i in 0..n { s.edges.push(f3(pa.lerp(pb, i as f64 / n as f64))); s.edges.push(f3(pa.lerp(pb, (i + 1) as f64 / n as f64))); }
    }
    (s, face_n)
}

/// Interpolation of nodal results at the display vertices.
struct Sampler { nodes: Vec<[u32; 8]>, w: Vec<[f32; 8]>, edge_nodes: Vec<[u32; 8]>, edge_w: Vec<[f32; 8]> }

impl Sampler {
    fn new(mesh: &Mesh, s: &Surface, face_n: &[V3]) -> Sampler {
        let at = |p: [f32; 3], n: V3| {
            let p = V3::new(p[0] as f64, p[1] as f64, p[2] as f64);
            match mesh.locate(p, n) {
                Some((e, l)) => { let w = Mesh::weights(l); (mesh.elems[e], [w[0] as f32, w[1] as f32, w[2] as f32, w[3] as f32, w[4] as f32, w[5] as f32, w[6] as f32, w[7] as f32]) }
                None => ([0; 8], [0.0; 8]),
            }
        };
        let (mut nodes, mut w) = (Vec::with_capacity(s.pos.len()), Vec::with_capacity(s.pos.len()));
        for (i, p) in s.pos.iter().enumerate() { let (a, b) = at(*p, face_n[i]); nodes.push(a); w.push(b); }
        let (mut edge_nodes, mut edge_w) = (vec![], vec![]);
        for p in &s.edges { let (a, b) = at(*p, V3::ZERO); edge_nodes.push(a); edge_w.push(b); }
        Sampler { nodes, w, edge_nodes, edge_w }
    }
    fn scalar(&self, v: &[f64]) -> Vec<f32> {
        self.nodes.iter().zip(&self.w).map(|(n, w)| (0..8).map(|a| v[n[a] as usize] as f32 * w[a]).sum()).collect()
    }
    fn vector(&self, v: &[V3], edges: bool) -> Vec<[f32; 3]> {
        let (ns, ws) = if edges { (&self.edge_nodes, &self.edge_w) } else { (&self.nodes, &self.w) };
        ns.iter().zip(ws).map(|(n, w)| { let mut s = V3::ZERO; for a in 0..8 { s += v[n[a] as usize] * w[a] as f64; } f3(s) }).collect()
    }
    fn disp(&self, u: &[V3], harmonic: bool) -> DispSet {
        DispSet { d: self.vector(u, false), edge_d: self.vector(u, true), max: u.iter().map(|v| v.length()).fold(0.0, f64::max) as f32, harmonic }
    }
}

/// Faces of an item in the current geometry; an error names the item if none is left.
fn item_faces(topo: &Topology, it: &SimItem, warnings: &mut Vec<String>) -> Result<Vec<usize>, String> {
    let mut ids: Vec<usize> = it.faces.iter().filter_map(|f| f.resolve(topo)).collect();
    ids.sort();
    ids.dedup();
    if ids.is_empty() { return Err(format!("{}: keine Fläche gewählt oder die Referenz fehlt nach der Regenerierung.", it.name)); }
    if ids.len() < it.faces.len() { warnings.push(format!("{}: {} Referenz(en) wurden nicht mehr gefunden.", it.name, it.faces.len() - ids.len())); }
    Ok(ids)
}

fn thermal_input(mesh: &Mesh, topo: &Topology, study: &Study, mat: &Material, warnings: &mut Vec<String>) -> Result<ThermalIn, String> {
    let mut inp = ThermalIn { k: mat.k * 1e-3, ..Default::default() };
    for it in study.items.iter().filter(|i| i.kind.is_thermal()) {
        let map = mesh.map_faces(topo, &item_faces(topo, it, warnings)?);
        let wsum: f64 = map.nodes.iter().map(|n| n.1).sum::<f64>().max(1e-300);
        for (n, w, _) in &map.nodes {
            match it.kind {
                ItemKind::Temperature => inp.fixed.push((*n, it.value)),
                ItemKind::Convection => inp.conv.push((*n, it.value * 1e-6 * w, it.value2)),
                ItemKind::HeatFlow => inp.heat.push((*n, it.value * w / wsum)),
                ItemKind::HeatFlux => inp.heat.push((*n, it.value * w)),
                _ => {}
            }
        }
    }
    // a node keeps a single prescribed temperature
    inp.fixed.sort_by_key(|x| x.0);
    inp.fixed.dedup_by_key(|x| x.0);
    Ok(inp)
}

fn struct_input(mesh: &Mesh, topo: &Topology, study: &Study, mat: &Material, warnings: &mut Vec<String>) -> Result<StructIn, String> {
    let mut inp = StructIn { e: mat.e, nu: mat.nu, rho: mat.rho * 1e-12, alpha: mat.alpha, ..Default::default() };
    for it in &study.items {
        if it.kind.is_thermal() || it.kind.is_fluid() { continue; }
        match it.kind {
            ItemKind::Gravity => { inp.accel += V3::from_array(it.vec); continue; }
            // the body is accelerated: its inertia loads it the other way
            ItemKind::LinAccel => { inp.accel -= V3::from_array(it.vec); continue; }
            ItemKind::Centrifugal => {
                let ax = V3::from_array(it.vec).normalize_or_zero();
                if ax == V3::ZERO { return Err(format!("{}: die Drehachse hat keine Richtung.", it.name)); }
                inp.spin = Some((V3::ZERO, ax, it.value * std::f64::consts::TAU / 60.0));
                continue;
            }
            _ => {}
        }
        let map = mesh.map_faces(topo, &item_faces(topo, it, warnings)?);
        let wsum: f64 = map.nodes.iter().map(|n| n.1).sum::<f64>().max(1e-300);
        match it.kind {
            ItemKind::Fixed => inp.fixed.extend(map.nodes.iter().map(|n| (n.0, [Some(0.0); 3]))),
            ItemKind::Displacement => inp.fixed.extend(map.nodes.iter().map(|n| (n.0, [it.fixed[0].then_some(it.vec[0]), it.fixed[1].then_some(it.vec[1]), it.fixed[2].then_some(it.vec[2])]))),
            ItemKind::Frictionless | ItemKind::Symmetry => inp.slide.extend(map.nodes.iter().map(|n| (n.0, n.2))),
            ItemKind::Force => { let f = V3::from_array(it.vec); inp.forces.extend(map.nodes.iter().map(|n| (n.0, f * (n.1 / wsum)))); }
            ItemKind::Pressure => inp.forces.extend(map.nodes.iter().map(|n| (n.0, n.2 * -it.value))),
            ItemKind::Moment => {
                let m = V3::from_array(it.vec);
                let ax = m.normalize_or_zero();
                let den: f64 = map.nodes.iter().map(|n| n.1 * ax.cross(mesh.nodes[n.0 as usize] - map.centroid).length_squared()).sum();
                if den > 1e-12 {
                    let a = m.length() / den;
                    inp.forces.extend(map.nodes.iter().map(|n| (n.0, ax.cross(mesh.nodes[n.0 as usize] - map.centroid) * (n.1 * a))));
                }
            }
            _ => {}
        }
    }
    Ok(inp)
}

fn mesh_info(mesh: &Mesh, summary: &mut Vec<(String, String)>, warnings: &mut Vec<String>) {
    summary.push(("Netz".into(), format!("{} Elemente, {} Knoten ({} × {} × {} mm)", mesh.elems.len(), mesh.nodes.len(), fmt(mesh.grid.h.x), fmt(mesh.grid.h.y), fmt(mesh.grid.h.z))));
    if mesh.dropped > 0.01 { warnings.push(format!("{:.0} % des Volumens hängen im Netz nicht am Hauptkörper und wurden weggelassen.", mesh.dropped * 100.0)); }
}

fn node_table(mesh: &Mesh, cols: &[(&str, Vec<f32>)]) -> (Vec<String>, Vec<Vec<f32>>) {
    let mut head: Vec<String> = vec!["x_mm".into(), "y_mm".into(), "z_mm".into()];
    head.extend(cols.iter().map(|c| c.0.to_string()));
    let rows = mesh.nodes.iter().enumerate().map(|(i, p)| { let mut r = vec![p.x as f32, p.y as f32, p.z as f32]; r.extend(cols.iter().map(|c| c.1[i])); r }).collect();
    (head, rows)
}

fn run(input: &SimInput, sh: &Shared) -> Result<SimOutput, String> {
    let (topo, study) = (&*input.topo, &input.study);
    let ctl = Ctl { cancel: &sh.cancel, progress: &sh.progress };
    let mat = material(&study.material);
    let mut warnings = vec![];
    let mut summary: Vec<(String, String)> = vec![];
    if study.kind == StudyKind::Fluid { return run_fluid(input, sh, &ctl); }
    sh.stage("Vernetzen");
    let mesh = Mesh::build(topo, budget(study.kind, input.fidelity))?;
    ctl.set(0.03);
    let (surface, face_n) = build_surface(topo, mesh.grid.h.min_element());
    let sampler = Sampler::new(&mesh, &surface, &face_n);
    summary.push(("Material".into(), mat.name.into()));
    mesh_info(&mesh, &mut summary, &mut warnings);
    let mut out = SimOutput { kind: study.kind, surface: Arc::new(surface), fields: vec![], disps: vec![], lines: vec![], summary: vec![], warnings: vec![], table: (vec![], vec![]), partial: false, seconds: 0.0, see_through: false };
    let f32s = |v: &[f64]| v.iter().map(|x| *x as f32).collect::<Vec<f32>>();
    match study.kind {
        StudyKind::Structural => {
            let mut inp = struct_input(&mesh, topo, study, &mat, &mut warnings)?;
            let mut temp = None;
            if study.include_thermal && study.items.iter().any(|i| i.kind.is_thermal()) {
                sh.stage("Temperaturfeld lösen");
                let t = fea::solve_thermal(&mesh, &thermal_input(&mesh, topo, study, &mat, &mut warnings)?, &ctl, (0.03, 0.2))?;
                inp.dtemp = Some(t.t.iter().map(|v| v - study.ref_temp).collect());
                temp = Some(t.t);
            }
            if inp.forces.is_empty() && inp.accel == V3::ZERO && inp.spin.is_none() && inp.dtemp.is_none() && !inp.fixed.iter().any(|f| f.1.iter().any(|v| v.is_some_and(|x| x != 0.0))) {
                warnings.push("Es ist keine Last definiert: das Ergebnis ist null.".into());
            }
            sh.stage("Spannungen lösen");
            let r = fea::solve_struct(&mesh, &inp, &ctl)?;
            sh.stage("Ergebnisse aufbereiten");
            let vm: Vec<f64> = r.stress.iter().map(fea::von_mises).collect();
            let mag: Vec<f64> = r.u.iter().map(|u| u.length()).collect();
            let p1: Vec<f64> = r.stress.iter().map(fea::max_principal).collect();
            out.disps.push(sampler.disp(&r.u, false));
            out.fields.push(Field::new("Von-Mises-Spannung", "MPa", sampler.scalar(&vm), Some(0)));
            out.fields.push(Field::new("Verschiebung (Betrag)", "mm", sampler.scalar(&mag), Some(0)));
            for (c, n) in ["Verschiebung X", "Verschiebung Y", "Verschiebung Z"].iter().enumerate() {
                let comp: Vec<f64> = r.u.iter().map(|u| u[c]).collect();
                out.fields.push(Field::new(n, "mm", sampler.scalar(&comp), Some(0)));
            }
            out.fields.push(Field::new("Max. Hauptspannung", "MPa", sampler.scalar(&p1), Some(0)));
            // the peaks reported are those of the picture: values on the part's surface
            let vmax = out.fields[0].max as f64;
            let sf: Vec<f64> = vm.iter().map(|v| (mat.yield_mpa / v.max(1e-9)).min(15.0)).collect();
            let mut fs = Field::new("Sicherheitsfaktor", "", sampler.scalar(&sf), Some(0));
            fs.low_is_bad = true;
            out.fields.push(fs);
            let mut cols = vec![("ux_mm", r.u.iter().map(|u| u.x as f32).collect::<Vec<f32>>()), ("uy_mm", r.u.iter().map(|u| u.y as f32).collect()), ("uz_mm", r.u.iter().map(|u| u.z as f32).collect()), ("von_mises_MPa", f32s(&vm)), ("max_hauptspannung_MPa", f32s(&p1))];
            if let Some(t) = &temp { out.fields.push(Field::new("Temperatur", "°C", sampler.scalar(t), Some(0))); cols.push(("temperatur_C", f32s(t))); }
            out.table = node_table(&mesh, &cols);
            let umax = out.fields[1].max as f64;
            summary.push(("Max. Von-Mises-Spannung".into(), format!("{} MPa", fmt(vmax))));
            summary.push(("Max. Verschiebung".into(), format!("{} mm", fmt(umax))));
            summary.push(("Min. Sicherheitsfaktor".into(), if vmax > 1e-9 { format!("{} (Streckgrenze {} MPa)", fmt(mat.yield_mpa / vmax), fmt(mat.yield_mpa)) } else { "–".into() }));
            summary.push(("Aufgebrachte Last".into(), format!("{} N  ({}, {}, {})", fmt(r.applied.length()), fmt(r.applied.x), fmt(r.applied.y), fmt(r.applied.z))));
            summary.push(("Reaktionskraft".into(), format!("{} N  ({}, {}, {})", fmt(r.reaction.length()), fmt(r.reaction.x), fmt(r.reaction.y), fmt(r.reaction.z))));
            summary.push(("Löser".into(), format!("{} Iterationen, Restfehler {:.1e}", r.iters, r.resid)));
            if vmax > mat.yield_mpa { warnings.push(format!("Die Spannung überschreitet die Streckgrenze von {} ({} MPa): das lineare Ergebnis gilt dort nicht mehr.", mat.name, fmt(mat.yield_mpa))); }
        }
        StudyKind::Thermal => {
            sh.stage("Temperaturfeld lösen");
            let r = fea::solve_thermal(&mesh, &thermal_input(&mesh, topo, study, &mat, &mut warnings)?, &ctl, (0.05, 0.9))?;
            sh.stage("Ergebnisse aufbereiten");
            // kW/m² = mW/mm²
            let q: Vec<f64> = r.flux.iter().map(|v| v.length() * 1e3).collect();
            out.fields.push(Field::new("Temperatur", "°C", sampler.scalar(&r.t), None));
            out.fields.push(Field::new("Wärmefluss", "kW/m²", sampler.scalar(&q), None));
            out.table = node_table(&mesh, &[("temperatur_C", f32s(&r.t)), ("waermefluss_kW_m2", f32s(&q))]);
            let (tmin, tmax) = r.t.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, v| (a.0.min(*v), a.1.max(*v)));
            summary.push(("Max. Temperatur".into(), format!("{} °C", fmt(tmax))));
            summary.push(("Min. Temperatur".into(), format!("{} °C", fmt(tmin))));
            summary.push(("Max. Wärmefluss".into(), format!("{} kW/m²", fmt(q.iter().cloned().fold(0.0, f64::max)))));
            summary.push(("Zugeführte Wärme".into(), format!("{} W", fmt(r.heat_in))));
            summary.push(("Löser".into(), format!("{} Iterationen", r.iters)));
        }
        StudyKind::Modal => {
            sh.stage("Eigenfrequenzen lösen");
            let inp = struct_input(&mesh, topo, study, &mat, &mut warnings)?;
            let modes = fea::solve_modal(&mesh, &inp, study.modes.clamp(1, 10), &ctl)?;
            sh.stage("Ergebnisse aufbereiten");
            let mut cols = vec![];
            for (i, m) in modes.iter().enumerate() {
                let mag: Vec<f64> = m.shape.iter().map(|u| u.length()).collect();
                out.disps.push(sampler.disp(&m.shape, true));
                out.fields.push(Field::new(&format!("Mode {}: {} Hz", i + 1, fmt(m.freq)), "normiert", sampler.scalar(&mag), Some(i)));
                summary.push((format!("Mode {}", i + 1), format!("{} Hz", fmt(m.freq))));
                cols.push((format!("mode{}_{:.2}Hz", i + 1, m.freq), f32s(&mag)));
            }
            let cref: Vec<(&str, Vec<f32>)> = cols.iter().map(|c| (c.0.as_str(), c.1.clone())).collect();
            out.table = node_table(&mesh, &cref);
            if inp.fixed.is_empty() && inp.slide.is_empty() { warnings.push("Ohne Randbedingung schwingt das Teil frei: die sechs Starrkörpermoden (0 Hz) sind ausgelassen.".into()); }
        }
        StudyKind::Fluid => unreachable!(),
    }
    ctl.set(1.0);
    out.summary = summary;
    out.warnings = warnings;
    Ok(out)
}

fn run_fluid(input: &SimInput, sh: &Shared, ctl: &Ctl) -> Result<SimOutput, String> {
    let (topo, study) = (&*input.topo, &input.study);
    if topo.polys.is_empty() { return Err("Das Modell enthält kein Volumen.".into()); }
    let fl = crate::model::sim::fluid(&study.fluid);
    let tris = tris_of(topo);
    let cells = budget(StudyKind::Fluid, input.fidelity);
    let mut warnings = vec![];
    let inlets: Vec<&SimItem> = study.items.iter().filter(|i| i.kind == ItemKind::Inlet).collect();
    let outlets: Vec<&SimItem> = study.items.iter().filter(|i| i.kind == ItemKind::Outlet).collect();
    let wind = study.items.iter().find(|i| i.kind == ItemKind::Wind);
    sh.stage("Strömungsgitter aufbauen");
    let mut setup = if !inlets.is_empty() || !outlets.is_empty() {
        if inlets.is_empty() { return Err("Für eine interne Strömung wird ein Einlass benötigt.".into()); }
        let mut ops = vec![];
        for it in inlets.iter().chain(outlets.iter()) {
            for f in item_faces(topo, it, &mut warnings)? {
                let face = &topo.faces[f];
                if !face.planar { return Err(format!("{}: Ein- und Auslässe brauchen ebene Flächen.", it.name)); }
                let mut bb = BBox::default();
                for &pi in &face.polys { for &v in &topo.polys[pi] { bb.add(topo.positions[v]); } }
                ops.push(fluid::Opening { point: face.center, normal: face.normal, bbox: bb, inlet: it.kind == ItemKind::Inlet, speed: it.value });
            }
        }
        if wind.is_some() { warnings.push("Die Anströmung wird ignoriert: mit Ein- und Auslass wird die interne Strömung gerechnet.".into()); }
        fluid::internal(&tris, &topo.bbox, &ops, &fl, cells)?
    } else if let Some(w) = wind {
        fluid::external(&tris, &topo.bbox, V3::from_array(w.vec), w.value, &fl, cells)?
    } else {
        return Err("Keine Strömung definiert: Einlass und Auslass (interne Strömung) oder eine Anströmung (externe Strömung) hinzufügen.".into());
    };
    let (surface, face_n) = build_surface(topo, setup.dx);
    let surface = Arc::new(surface);
    let n = setup.lbm.n;
    let nfluid = setup.lbm.kind.iter().filter(|k| **k == fluid::FLUID).count();
    let (re, steps, internal) = (setup.reynolds, setup.total_steps, setup.internal);
    let dx = setup.dx;
    let make = |f: fluid::FlowField, warnings: &[String]| -> SimOutput {
        let (mut p, mut v) = (Vec::with_capacity(surface.pos.len()), Vec::with_capacity(surface.pos.len()));
        for (i, q) in surface.pos.iter().enumerate() {
            let (a, b) = f.near_wall(V3::new(q[0] as f64, q[1] as f64, q[2] as f64), face_n[i]).unwrap_or((f32::NAN, f32::NAN));
            p.push(a);
            v.push(b);
        }
        let lines = f.streamlines();
        let vmax = f.u.iter().zip(&f.kind).filter(|x| *x.1 == fluid::FLUID).map(|(u, _)| (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt()).fold(0.0f32, f32::max);
        let mut fields = vec![Field::new("Geschwindigkeit", "m/s", v, None), Field::new("Druck", "Pa", p, None)];
        // the legend of the speed covers the stream lines too
        fields[0].min = 0.0;
        fields[0].max = vmax;
        let mut summary = vec![
            ("Fluid".to_string(), format!("{} ({} kg/m³)", fl.name, fmt(fl.rho))),
            ("Strömung".into(), if internal { "intern".into() } else { "extern".into() }),
            ("Gitter".into(), format!("{} × {} × {} Zellen à {} mm, {} Fluidzellen", n[0], n[1], n[2], fmt(dx), nfluid)),
            ("Reynolds-Zahl".into(), fmt(re)),
            ("Max. Geschwindigkeit".into(), format!("{} m/s", fmt(vmax as f64))),
        ];
        if internal { summary.push(("Druckabfall Einlass → Auslass".into(), format!("{} Pa", fmt(f.dp)))); }
        summary.push(("Kraft auf das Teil".into(), format!("{} N  ({}, {}, {})", fmt(f.force.length()), fmt(f.force.x), fmt(f.force.y), fmt(f.force.z))));
        summary.push(("Zeitschritte".into(), format!("{} von {}", (f.done * steps as f32) as usize, steps)));
        let mut rows = vec![];
        if f.done >= 1.0 {
            let st = (nfluid / 60_000).max(1);
            let mut c = 0;
            for k in 0..n[2] { for j in 0..n[1] { for i in 0..n[0] {
                let idx = i + n[0] * (j + n[1] * k);
                if f.kind[idx] != fluid::FLUID { continue; }
                c += 1;
                if c % st != 0 { continue; }
                let q = f.origin + V3::new(i as f64 + 0.5, j as f64 + 0.5, k as f64 + 0.5) * f.dx;
                rows.push(vec![q.x as f32, q.y as f32, q.z as f32, f.u[idx][0], f.u[idx][1], f.u[idx][2], f.p[idx]]);
            } } }
        }
        SimOutput {
            kind: StudyKind::Fluid, surface: surface.clone(), fields, disps: vec![], lines, summary, warnings: warnings.to_vec(),
            table: (["x_mm", "y_mm", "z_mm", "u_m_s", "v_m_s", "w_m_s", "p_Pa"].iter().map(|s| s.to_string()).collect(), rows),
            partial: f.done < 1.0, seconds: 0.0, see_through: f.internal,
        }
    };
    if re > 4000.0 { warnings.push("Turbulente Strömung: das grobe Gitter löst die Grenzschicht nicht auf, die Werte sind Richtwerte.".into()); }
    sh.stage("Strömung berechnen");
    let mut last = None;
    fluid::run(&mut setup, ctl, &mut |f| {
        let o = make(f, &warnings);
        if o.partial { *sh.partial.lock().unwrap() = Some(o); } else { last = Some(o); }
    })?;
    ctl.set(1.0);
    last.ok_or_else(|| "Die Strömungsberechnung lieferte kein Ergebnis.".into())
}
