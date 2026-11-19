//! CNC: 3-axis machining of Freo parts. Toolpaths are generated from the part (facing,
//! drilling, roughing, contour finishing, 3D finishing), then an independent simulation
//! removes the material move by move with the real tool shapes and checks every move for
//! crashes (rapids into material, holder and shank collisions, cuts into the part or the
//! table, machine travels, speeds). NC code can only be written when the check is clean.

pub mod dropcut;
pub mod field;
pub mod generate;
pub mod machines;
pub mod materials;
pub mod nc;
pub mod sim;
pub mod tools;

use crate::slicer::mesh::Mesh;
use glam::DVec3;
use machines::Machine;
use materials::{Material, MATERIALS};
use serde::{Deserialize, Serialize};
use tools::{Tool, ToolKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum OpKind {
    Face,
    Drill,
    Rough,
    Contour,
    Parallel,
}

impl OpKind {
    pub const ALL: [OpKind; 5] = [OpKind::Face, OpKind::Drill, OpKind::Rough, OpKind::Contour, OpKind::Parallel];
    pub fn name(&self) -> &'static str {
        match self {
            OpKind::Face => "Planfräsen",
            OpKind::Drill => "Bohren",
            OpKind::Rough => "Schruppen",
            OpKind::Contour => "Schlichten Kontur",
            OpKind::Parallel => "3D-Schlichten",
        }
    }
    pub fn info(&self) -> &'static str {
        match self {
            OpKind::Face => "Oberseite des Rohteils bis zur Teiloberkante abfräsen",
            OpKind::Drill => "Senkrechte runde Bohrungen mit passendem Bohrer, mit Entspanen",
            OpKind::Rough => "Material schichtweise ausräumen, Aufmaß stehen lassen",
            OpKind::Contour => "Wände und ebene Böden auf Endmaß",
            OpKind::Parallel => "Gekrümmte Flächen zeilenweise mit dem Kugelfräser",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Op {
    pub kind: OpKind,
    pub enabled: bool,
    /// tool number
    pub tool: u32,
    /// depth per pass (mm); None = from material and machine
    pub stepdown: Option<f64>,
    /// stepover as a fraction of the diameter; None = automatic
    pub stepover: Option<f64>,
    /// stock to leave on walls and floors (mm)
    pub leave: f64,
    /// overrides (feed mm/min, spindle rpm)
    pub feed: Option<f64>,
    pub rpm: Option<f64>,
}

impl Default for Op {
    fn default() -> Self {
        Op { kind: OpKind::Rough, enabled: true, tool: 6, stepdown: None, stepover: None, leave: 0.3, feed: None, rpm: None }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Origin {
    FrontLeftTop,
    CenterTop,
}

impl Origin {
    pub const ALL: [Origin; 2] = [Origin::FrontLeftTop, Origin::CenterTop];
    pub fn name(&self) -> &'static str {
        match self {
            Origin::FrontLeftTop => "Ecke vorne links, Oberkante",
            Origin::CenterTop => "Mitte, Oberkante",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Setup {
    pub machine: Machine,
    pub material: usize,
    pub tools: Vec<Tool>,
    pub margin_xy: f64,
    pub margin_top: f64,
    pub margin_bottom: f64,
    pub origin: Origin,
    /// safe height above the stock top (mm)
    pub safe_z: f64,
    /// rapids stop this far above the material (mm)
    pub clearance: f64,
    pub ops: Vec<Op>,
}

impl Default for Setup {
    fn default() -> Self {
        let tools = tools::library();
        Setup {
            machine: Machine::default(),
            material: MATERIALS.iter().position(|m| m.name.starts_with("Aluminium 6061")).unwrap_or(0),
            tools,
            margin_xy: 3.0,
            margin_top: 1.0,
            margin_bottom: 2.0,
            origin: Origin::FrontLeftTop,
            safe_z: 10.0,
            clearance: 2.0,
            ops: vec![
                Op { kind: OpKind::Face, tool: 10, leave: 0.0, ..Default::default() },
                Op { kind: OpKind::Drill, tool: 0, leave: 0.0, ..Default::default() },
                Op { kind: OpKind::Rough, tool: 6, leave: 0.3, ..Default::default() },
                Op { kind: OpKind::Contour, tool: 6, leave: 0.0, ..Default::default() },
                Op { kind: OpKind::Parallel, tool: 13, leave: 0.0, enabled: false, ..Default::default() },
            ],
        }
    }
}

impl Setup {
    pub fn material(&self) -> &'static Material {
        &MATERIALS[self.material.min(MATERIALS.len() - 1)]
    }
    pub fn tool(&self, n: u32) -> Option<&Tool> {
        self.tools.iter().find(|t| t.number == n)
    }

    /// Chooses sensible tools for a part of this size (largest flat cutter that fits a
    /// third of the smaller side, …).
    pub fn auto_tools(&mut self, part_w: f64, part_d: f64) {
        let small = part_w.min(part_d);
        let flats: Vec<&Tool> = self.tools.iter().filter(|t| t.kind == ToolKind::Flat).collect();
        let pick = |limit: f64| flats.iter().filter(|t| t.diameter <= limit).max_by(|a, b| a.diameter.partial_cmp(&b.diameter).unwrap()).or_else(|| flats.iter().min_by(|a, b| a.diameter.partial_cmp(&b.diameter).unwrap())).map(|t| t.number);
        let rough = pick((small / 3.0).min(8.0));
        let face = pick(12.0);
        let ball = self
            .tools
            .iter()
            .filter(|t| t.kind == ToolKind::Ball)
            .filter(|t| t.diameter <= (small / 3.0).min(6.0))
            .max_by(|a, b| a.diameter.partial_cmp(&b.diameter).unwrap())
            .map(|t| t.number);
        for op in &mut self.ops {
            match op.kind {
                OpKind::Face => op.tool = face.unwrap_or(op.tool),
                OpKind::Rough | OpKind::Contour => op.tool = rough.unwrap_or(op.tool),
                OpKind::Parallel => op.tool = ball.unwrap_or(op.tool),
                OpKind::Drill => {}
            }
        }
    }

    pub fn validate(&self) -> Vec<String> {
        let mut e = vec![];
        if let Some(m) = self.machine.validate() {
            e.push(m);
        }
        for t in &self.tools {
            if let Some(m) = t.validate() {
                e.push(m);
            }
        }
        let mut nums: Vec<u32> = self.tools.iter().map(|t| t.number).collect();
        nums.sort();
        if nums.windows(2).any(|w| w[0] == w[1]) {
            e.push("Zwei Werkzeuge haben dieselbe Nummer.".into());
        }
        if self.margin_bottom < 0.5 {
            e.push("Rohteil-Aufmaß unten muss mindestens 0,5 mm sein, sonst löst sich das Teil beim letzten Schnitt (Gefahr!).".into());
        }
        if self.margin_xy < 0.0 || self.margin_top < 0.0 {
            e.push("Rohteil-Aufmaße dürfen nicht negativ sein.".into());
        }
        if self.safe_z < 2.0 {
            e.push("Sicherheitshöhe muss mindestens 2 mm über dem Rohteil liegen.".into());
        }
        if self.clearance < 0.5 || self.clearance > self.safe_z {
            e.push("Abstand für Eilgänge muss zwischen 0,5 mm und der Sicherheitshöhe liegen.".into());
        }
        for op in self.ops.iter().filter(|o| o.enabled && o.kind != OpKind::Drill) {
            match self.tool(op.tool) {
                None => e.push(format!("{}: Werkzeug T{} gibt es nicht.", op.kind.name(), op.tool)),
                Some(t) => {
                    let ok = match op.kind {
                        OpKind::Face | OpKind::Rough | OpKind::Contour => matches!(t.kind, ToolKind::Flat | ToolKind::Bull),
                        OpKind::Parallel => matches!(t.kind, ToolKind::Ball | ToolKind::Flat),
                        OpKind::Drill => true,
                    };
                    if !ok {
                        e.push(format!("{}: {} ist dafür nicht geeignet.", op.kind.name(), t.label()));
                    }
                }
            }
            if op.stepdown.is_some_and(|s| !(s > 0.0)) || op.stepover.is_some_and(|s| !(s > 0.0 && s <= 0.9)) || op.leave < 0.0 {
                e.push(format!("{}: Zustellung, Schrittweite oder Aufmaß ungültig.", op.kind.name()));
            }
            if op.feed.is_some_and(|f| !(f > 0.0)) || op.rpm.is_some_and(|f| !(f > 0.0)) {
                e.push(format!("{}: Vorschub und Drehzahl müssen größer als 0 sein.", op.kind.name()));
            }
        }
        e
    }
}

/// Speeds and engagement for a tool in a material on a machine.
#[derive(Clone, Copy, Debug)]
pub struct Cutting {
    pub rpm: f64,
    /// mm/min
    pub feed: f64,
    pub plunge: f64,
    /// depth of cut (mm)
    pub ap: f64,
    /// stepover (mm)
    pub ae: f64,
    pub ramp: f64,
}

pub fn cutting(t: &Tool, m: &Material, mc: &Machine) -> (Cutting, Vec<String>) {
    let mut notes = vec![];
    let d = t.diameter;
    let mut rpm = m.vc * 1000.0 / (std::f64::consts::PI * d);
    if rpm > mc.rpm_max {
        rpm = mc.rpm_max;
    }
    if rpm < mc.rpm_min {
        notes.push(format!("{}: die Spindel dreht mindestens {:.0}/min, mehr als die {:.0}/min für {}; Vorschub wird angepasst.", t.label(), mc.rpm_min, rpm, m.name));
        rpm = mc.rpm_min;
    }
    // chip load from the diameter, smaller for drills' many-edge entry and on light machines
    let fz = (m.fz_k * d).clamp(0.003, 0.4);
    let z = t.flutes.max(1) as f64;
    let mut feed = rpm * z * fz;
    if feed > mc.feed_max {
        feed = mc.feed_max;
    }
    let rig = mc.rigidity();
    let ap = (m.ap_k * d * rig).clamp(0.05, t.flute_length * 0.9);
    let ae = (m.ae_k * d * if rig > 1.0 { 1.3 } else { 1.0 }).clamp(0.05 * d, 0.5 * d);
    let plunge = (feed * 0.3).max(10.0);
    (Cutting { rpm, feed, plunge, ap, ae, ramp: m.ramp }, notes)
}

/// The part in the work coordinate system (Z0 = stock top) and the stock around it.
#[derive(Clone)]
pub struct Job {
    pub part: Mesh,
    pub stock_lo: DVec3,
    pub stock_hi: DVec3,
    pub part_lo: DVec3,
    pub part_hi: DVec3,
    /// where the WCS origin lies in the part's own (model) coordinates
    pub wcs: DVec3,
}

impl Job {
    /// `model`: the part with Z up, as modelled.
    pub fn new(model: &Mesh, s: &Setup) -> Option<Job> {
        if model.is_empty() {
            return None;
        }
        let (lo, hi) = model.bbox();
        let slo = DVec3::new(lo.x - s.margin_xy, lo.y - s.margin_xy, lo.z - s.margin_bottom);
        let shi = DVec3::new(hi.x + s.margin_xy, hi.y + s.margin_xy, hi.z + s.margin_top);
        let wcs = match s.origin {
            Origin::FrontLeftTop => DVec3::new(slo.x, slo.y, shi.z),
            Origin::CenterTop => DVec3::new((slo.x + shi.x) / 2.0, (slo.y + shi.y) / 2.0, shi.z),
        };
        let part = model.translated(-wcs);
        Some(Job { part, stock_lo: slo - wcs, stock_hi: shi - wcs, part_lo: lo - wcs, part_hi: hi - wcs, wcs })
    }
    pub fn stock_size(&self) -> DVec3 {
        self.stock_hi - self.stock_lo
    }
}

/// One move of the machine: straight, or (feed moves) a circular arc in XY with Z changing
/// linearly (a helix), as G2/G3.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CMove {
    pub to: DVec3,
    pub rapid: bool,
    /// mm/min (feed moves)
    pub feed: f64,
    /// arc: centre x, centre y and direction (+1 counter-clockwise G3, -1 clockwise G2)
    pub arc: Option<[f64; 3]>,
}

impl CMove {
    pub fn line(to: DVec3, rapid: bool, feed: f64) -> CMove {
        CMove { to, rapid, feed, arc: None }
    }

    /// Angle swept from `from` to `to` around the arc's centre (signed, ccw positive); a
    /// start equal to the end is a full turn.
    pub fn sweep(from: DVec3, to: DVec3, c: [f64; 3]) -> f64 {
        let a0 = (from.y - c[1]).atan2(from.x - c[0]);
        let a1 = (to.y - c[1]).atan2(to.x - c[0]);
        let tau = std::f64::consts::TAU;
        let mut d = (a1 - a0).rem_euclid(tau);
        if d < 1e-9 {
            d = tau;
        }
        if c[2] < 0.0 {
            d - tau
        } else {
            d
        }
    }

    /// The points the tool passes through, after `from`: the end for a straight move, steps of
    /// at most 2 µm chord error along an arc.
    pub fn points(&self, from: DVec3) -> Vec<DVec3> {
        let Some(c) = self.arc else { return vec![self.to] };
        let r = (from.x - c[0]).hypot(from.y - c[1]);
        let sw = CMove::sweep(from, self.to, c);
        let step = 2.0 * (1.0 - 0.002 / r.max(0.003)).clamp(-1.0, 1.0).acos();
        let n = ((sw.abs() / step.max(1e-3)).ceil() as usize).clamp(2, 4000);
        let a0 = (from.y - c[1]).atan2(from.x - c[0]);
        let r1 = (self.to.x - c[0]).hypot(self.to.y - c[1]);
        (1..=n)
            .map(|k| {
                let t = k as f64 / n as f64;
                if k == n {
                    return self.to;
                }
                let a = a0 + sw * t;
                let rr = r + (r1 - r) * t;
                DVec3::new(c[0] + rr * a.cos(), c[1] + rr * a.sin(), from.z + (self.to.z - from.z) * t)
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct OpPath {
    pub name: String,
    pub kind: OpKind,
    pub tool: Tool,
    pub rpm: f64,
    pub feed: f64,
    pub coolant: bool,
    pub moves: Vec<CMove>,
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub ops: Vec<OpPath>,
    /// absolute safe height (WCS)
    pub safe_z: f64,
    pub notes: Vec<String>,
}

pub struct Progress {
    pub cancel: std::sync::atomic::AtomicBool,
    pub done: std::sync::atomic::AtomicUsize,
    pub stage: std::sync::Mutex<String>,
}

impl Default for Progress {
    fn default() -> Self {
        Progress { cancel: Default::default(), done: Default::default(), stage: std::sync::Mutex::new(String::new()) }
    }
}

impl Progress {
    pub fn set(&self, stage: &str, permille: usize) {
        if let Ok(mut s) = self.stage.lock() {
            *s = stage.into();
        }
        self.done.store(permille.min(1000), std::sync::atomic::Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Everything a finished CAM run produces.
pub struct Output {
    pub job: Job,
    pub program: Program,
    pub sim: sim::SimOut,
}

/// Toolpaths and their verification for a part (Z-up model coordinates).
pub fn run(model: &Mesh, s: &Setup, prog: &Progress) -> Result<Output, String> {
    let errs = s.validate();
    if !errs.is_empty() {
        return Err(errs.join("\n"));
    }
    let job = Job::new(model, s).ok_or("Kein Modell zum Fräsen.")?;
    let program = generate::generate(&job, s, prog)?;
    if prog.cancelled() {
        return Err("Berechnung abgebrochen.".into());
    }
    let sim = sim::simulate(&job, s, &program, prog)?;
    Ok(Output { job, program, sim })
}
