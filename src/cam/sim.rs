//! Independent verification: the program is replayed on a fresh stock with the real tool
//! shapes. Every move is checked; any error blocks the NC export.

use super::field::Field;
use super::generate::part_height;
use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum IssueKind {
    RapidIntoMaterial,
    Holder,
    Shank,
    Gouge,
    Table,
    Travel,
    Spindle,
    Feed,
    StockTooTall,
    MachineUnsuited,
    Coolant,
    Hazard,
    RestMaterial,
    Notes,
}

#[derive(Clone, Debug)]
pub struct Issue {
    pub severity: Severity,
    pub kind: IssueKind,
    /// operation and move (in the operation) where it happens first
    pub op: usize,
    pub mv: usize,
    pub at: DVec3,
    pub count: usize,
    pub text: String,
}

/// One move flattened for playback.
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub op: u16,
    pub from: DVec3,
    pub to: DVec3,
    pub rapid: bool,
    pub feed: f64,
    /// seconds when it ends
    pub t: f64,
}

pub struct SimOut {
    pub field: Field,
    pub part: Vec<f32>,
    pub steps: Vec<Step>,
    pub issues: Vec<Issue>,
    pub time: f64,
    pub removed: f64,
    pub rest: f64,
}

impl SimOut {
    pub fn errors(&self) -> usize {
        self.issues.iter().filter(|i| i.severity == Severity::Error).count()
    }
    pub fn safe(&self) -> bool {
        self.errors() == 0
    }
}

struct Log {
    issues: Vec<Issue>,
}

impl Log {
    fn add(&mut self, severity: Severity, kind: IssueKind, op: usize, mv: usize, at: DVec3, text: String) {
        if let Some(i) = self.issues.iter_mut().find(|i| i.kind == kind && i.op == op) {
            i.count += 1;
            return;
        }
        self.issues.push(Issue { severity, kind, op, mv, at, count: 1, text });
    }
}

/// Seconds for a straight move with acceleration ignored at this level of detail: feed moves
/// at their feed, rapids at the machine's rapid rate.
fn move_time(len: f64, mm_per_min: f64) -> f64 {
    if mm_per_min <= 0.0 {
        0.0
    } else {
        len / (mm_per_min / 60.0)
    }
}

pub fn simulate(job: &Job, s: &Setup, program: &Program, prog: &Progress) -> Result<SimOut, String> {
    let size = job.stock_size();
    let cell = Field::cell_for(size.x, size.y);
    let mut f = Field::new(job.stock_lo.x, job.stock_lo.y, job.stock_hi.x, job.stock_hi.y, job.stock_lo.z, job.stock_hi.z, cell);
    let part = part_height(job, &f, 0.01);
    let mc = &s.machine;
    let m = s.material();
    let mut log = Log { issues: vec![] };
    let tol = 0.01f32;
    // ---- set-up checks ----
    if size.z > mc.clearance {
        log.add(Severity::Error, IssueKind::StockTooTall, 0, 0, job.stock_hi, format!("Das Rohteil ist {:.1} mm hoch, unter die Spindel passen nur {:.0} mm.", size.z, mc.clearance));
    }
    if size.x > mc.x || size.y > mc.y {
        log.add(Severity::Error, IssueKind::Travel, 0, 0, job.stock_hi, format!("Das Rohteil ({:.0} × {:.0} mm) ist größer als der Arbeitsbereich ({:.0} × {:.0} mm).", size.x, size.y, mc.x, mc.y));
    }
    if m.needs_mill && mc.kind == machines::MachineKind::Router {
        log.add(Severity::Error, IssueKind::MachineUnsuited, 0, 0, job.stock_hi, format!("{} lässt sich auf einer Portalfräse ({}) nicht sicher bearbeiten: eine steife Fräsmaschine wählen.", m.name, mc.name()));
    }
    if m.coolant && !mc.coolant {
        log.add(Severity::Warning, IssueKind::Coolant, 0, 0, job.stock_hi, format!("{} braucht Kühlung/Schmierung; die Maschine hat keine Kühlmittelsteuerung – Sprühnebel oder Luft von Hand.", m.name));
    }
    if !m.note.is_empty() {
        log.add(Severity::Warning, IssueKind::Hazard, 0, 0, job.stock_hi, m.note.to_string());
    }
    for (k, n) in program.notes.iter().enumerate() {
        log.add(Severity::Info, IssueKind::Notes, k, 0, job.stock_hi, n.clone());
    }
    // machine travels: the stock sits in the middle of the table, Z0 at its top, Z travel
    // measured from the safe height down
    let zr = program.safe_z;
    let off = DVec3::new(mc.x / 2.0 - (job.stock_lo.x + job.stock_hi.x) / 2.0, mc.y / 2.0 - (job.stock_lo.y + job.stock_hi.y) / 2.0, 0.0);
    let mut steps = vec![];
    let mut clock = 0.0;
    let mut removed = 0.0f64;
    let total: usize = program.ops.iter().map(|o| o.moves.len()).sum::<usize>().max(1);
    let mut done = 0usize;
    let mut pos = DVec3::new(job.stock_lo.x, job.stock_lo.y, program.safe_z);
    for (oi, op) in program.ops.iter().enumerate() {
        let t = &op.tool;
        if op.rpm > mc.rpm_max + 1e-6 || op.rpm < mc.rpm_min - 1e-6 {
            log.add(Severity::Error, IssueKind::Spindle, oi, 0, pos, format!("{}: Drehzahl {:.0}/min liegt außerhalb der Spindel ({:.0}–{:.0}/min).", op.name, op.rpm, mc.rpm_min, mc.rpm_max));
        }
        if op.rpm <= 0.0 {
            log.add(Severity::Error, IssueKind::Spindle, oi, 0, pos, format!("{}: die Spindel steht.", op.name));
        }
        for (mi, mv) in op.moves.iter().enumerate() {
            done += 1;
            if done % 2000 == 0 {
                if prog.cancelled() {
                    return Err("Berechnung abgebrochen.".into());
                }
                prog.set("Simulation und Prüfung", 500 + done * 480 / total);
            }
            if !mv.rapid && (mv.feed <= 0.0 || mv.feed > mc.feed_max + 1e-6) {
                log.add(Severity::Error, IssueKind::Feed, oi, mi, mv.to, format!("{}: Vorschub {:.0} mm/min außerhalb der Maschine (bis {:.0}).", op.name, mv.feed, mc.feed_max));
            }
            if mv.arc.is_some() && mv.rapid {
                log.add(Severity::Error, IssueKind::Feed, oi, mi, mv.to, format!("{}: ein Kreisbogen im Eilgang.", op.name));
            }
            // an arc is followed exactly in short steps (2 µm chord error)
            for b in mv.points(pos) {
            let a = pos;
            let mp = b + off;
            if mp.x < -1e-6 || mp.y < -1e-6 || mp.x > mc.x + 1e-6 || mp.y > mc.y + 1e-6 || zr - b.z > mc.z + 1e-6 {
                log.add(Severity::Error, IssueKind::Travel, oi, mi, b, format!("{}: die Bewegung verlässt den Verfahrbereich der Maschine.", op.name));
            }
            let len = (b - a).length();
            // collision probes along the move (the cut itself is the exact sweep below)
            let n = (len / f.cell.max(t.radius() * 0.25)).ceil().max(1.0) as usize;
            for k in 1..=n {
                let p = a + (b - a) * (k as f64 / n as f64);
                if p.z < f.floor - 1e-3 && p.x >= f.x0 && p.y >= f.y0 && p.x <= f.x0 + f.cell * f.nx as f64 && p.y <= f.y0 + f.cell * f.ny as f64 {
                    log.add(Severity::Error, IssueKind::Table, oi, mi, p, format!("{}: das Werkzeug fährt {:.2} mm in den Maschinentisch.", op.name, f.floor - p.z));
                }
                // holder and shank against material that is not cut away by this move
                let reach = (t.holder / 2.0).max(t.radius());
                if f.max_near(p.x, p.y, reach) > p.z + t.flute_length.min(t.stickout) - 1e-3 {
                    let shank = f.max_exact(p.x, p.y, (t.shank / 2.0).max(t.radius()));
                    if shank > p.z + t.flute_length + 0.01 {
                        log.add(Severity::Error, IssueKind::Shank, oi, mi, p, format!("{}: Material {:.2} mm über der Schneidenlänge – der Schaft würde reiben. Längeres Werkzeug oder weniger Tiefe.", op.name, shank - p.z - t.flute_length));
                    }
                    let holder = f.max_exact(p.x, p.y, t.holder / 2.0);
                    if holder > p.z + t.stickout + 0.01 {
                        log.add(Severity::Error, IssueKind::Holder, oi, mi, p, format!("{}: der Werkzeughalter stößt gegen das Werkstück ({:.2} mm). Auskraglänge erhöhen.", op.name, holder - p.z - t.stickout));
                    }
                }
            }
            let c = f.sweep(t, a, b, Some(&part));
            if c.cells > 0 {
                if mv.rapid && c.depth > tol {
                    log.add(Severity::Error, IssueKind::RapidIntoMaterial, oi, mi, b, format!("{}: Eilgang {:.2} mm tief ins Material – Crashgefahr!", op.name, c.depth));
                }
                if c.gouge < -tol {
                    let (gx, gy) = f.center(c.gouge_at.0, c.gouge_at.1);
                    log.add(Severity::Error, IssueKind::Gouge, oi, mi, DVec3::new(gx, gy, b.z), format!("{}: das Werkzeug schneidet {:.3} mm ins fertige Teil.", op.name, -c.gouge));
                }
            }
            let dt = if mv.rapid { move_time(len, mc.rapid) } else { move_time(len, mv.feed) };
            clock += dt;
            steps.push(Step { op: oi as u16, from: a, to: b, rapid: mv.rapid, feed: if mv.rapid { mc.rapid } else { mv.feed }, t: clock });
            pos = b;
            }
        }
    }
    // removed volume and rest material against the finished part
    let stock_top = f.top as f32;
    let a = f.cell * f.cell;
    let mut rest = 0.0;
    let mut rest_cells = 0usize;
    for k in 0..f.h.len() {
        removed += (stock_top - f.h[k]).max(0.0) as f64 * a;
        if part[k].is_finite() && f.h[k] > part[k] + 0.05 {
            rest += (f.h[k] - part[k]) as f64 * a;
            rest_cells += 1;
        }
    }
    if rest > 1.0 {
        log.add(
            Severity::Info,
            IssueKind::RestMaterial,
            0,
            0,
            job.part_hi,
            format!("Restmaterial: {:.0} mm³ auf {:.0} mm² stehen noch über dem Teil (Ecken und Taschen kleiner als das Werkzeug, Aufmaß).", rest, rest_cells as f64 * a),
        );
    }
    let mut issues = log.issues;
    issues.sort_by_key(|i| (i.severity, i.op, i.mv));
    Ok(SimOut { field: f, part, steps, issues, time: clock, removed, rest })
}
