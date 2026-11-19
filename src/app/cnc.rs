//! CNC (Werkzeuge ▸ Manufacture ▸ CNC): machine, material and tools, toolpaths computed in
//! the background, the material removal played back with the spinning cutter, the crash
//! check report, and the NC export — which stays locked until the check is clean.

use super::cnc_gl::{CncRenderer, Frame as GlFrame, Heights, Scene};
use super::icons::{self, Icon};
use super::ribbon::{drop_menu, group, large, menu_row, small, stack, theme::*, tiny};
use super::*;
use crate::cam::field::Field;
use crate::cam::machines::{self, Machine, MachineKind};
use crate::cam::materials::{Group as MatGroup, MATERIALS};
use crate::cam::sim::{Severity, Step};
use crate::cam::tools::{Tool, ToolKind};
use crate::cam::{self, OpKind, Origin, Output, Progress, Setup};
use crate::slicer::mesh::{self as smesh, Mesh};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind};
use glam::{DMat3, DVec3};
use std::hash::{Hash, Hasher};

type JobOut = Arc<Mutex<Option<Result<Output, String>>>>;

pub struct CamJob {
    pub prog: Arc<Progress>,
    out: JobOut,
    key: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Brand,
    Name,
}

#[derive(Clone)]
pub struct MachineDlg {
    pub draft: Machine,
    pub filter: String,
    pub sort: SortBy,
    pub asc: bool,
    pub sel: Option<String>,
}

pub struct CncUi {
    pub open: bool,
    active: bool,
    pub camera: Camera,
    cam_init: bool,
    pub setup: Setup,
    /// orientation of the part on the machine (applied to the model, Z up)
    pub rot: [f64; 9],
    pub user_machines: Vec<Machine>,
    pub job: Option<CamJob>,
    pub out: Option<Arc<Output>>,
    out_key: u64,
    pub error: Option<String>,
    // playback
    pub playing: bool,
    pub speed: f64,
    /// seconds into the program
    pub t: f64,
    last: Option<std::time::Instant>,
    pb: Option<Field>,
    pb_steps: usize,
    pb_t: f64,
    heights: Option<Arc<Heights>>,
    counter: u64,
    pub show_tool: bool,
    pub show_paths: bool,
    pub show_part: bool,
    pub deviation: bool,
    pub machine_dlg: Option<MachineDlg>,
    pub tools_open: bool,
    pub report_open: bool,
    pub panel_op: Option<usize>,
    part: Option<(u64, Arc<Mesh>)>,
    scene: Option<(u64, Arc<Scene>)>,
    gpu: Arc<Mutex<Option<CncRenderer>>>,
}

impl Default for CncUi {
    fn default() -> Self {
        CncUi {
            open: false,
            active: false,
            camera: Camera::default(),
            cam_init: false,
            setup: Setup::default(),
            rot: DMat3::IDENTITY.to_cols_array(),
            user_machines: vec![],
            job: None,
            out: None,
            out_key: 0,
            error: None,
            playing: false,
            speed: 20.0,
            t: 0.0,
            last: None,
            pb: None,
            pb_steps: 0,
            pb_t: 0.0,
            heights: None,
            counter: 1,
            show_tool: true,
            show_paths: true,
            show_part: false,
            deviation: false,
            machine_dlg: None,
            tools_open: false,
            report_open: false,
            panel_op: Some(2),
            part: None,
            scene: None,
            gpu: Arc::new(Mutex::new(None)),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    setup: Setup,
    user_machines: Vec<Machine>,
}

impl CncUi {
    pub fn to_json(&self) -> String {
        serde_json::to_string(&Saved { setup: self.setup.clone(), user_machines: self.user_machines.clone() }).unwrap_or_default()
    }
    pub fn load_json(&mut self, s: &str) {
        if let Ok(v) = serde_json::from_str::<Saved>(s) {
            if v.setup.validate().is_empty() && v.setup.ops.len() == OpKind::ALL.len() {
                self.setup = v.setup;
            }
            self.user_machines = v.user_machines;
        }
    }
    fn gen(&mut self) -> u64 {
        self.counter += 1;
        self.counter
    }
    pub fn total(&self) -> f64 {
        self.out.as_ref().map(|o| o.sim.time).unwrap_or(0.0)
    }
}

fn hash_of(parts: &[&str]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in parts {
        p.hash(&mut h);
    }
    h.finish()
}

/// WCS (Z up) to Freo world (Y up).
fn w(p: DVec3) -> [f32; 3] {
    let q = smesh::printer_to_world(p);
    [q.x as f32, q.y as f32, q.z as f32]
}

fn push_tri(v: &mut Vec<f32>, a: DVec3, b: DVec3, c: DVec3, col: [f32; 3]) {
    let n = (b - a).cross(c - a).normalize_or_zero();
    let nw = smesh::printer_to_world(n);
    for p in [a, b, c] {
        let q = w(p);
        v.extend_from_slice(&[q[0], q[1], q[2], nw.x as f32, nw.y as f32, nw.z as f32, col[0], col[1], col[2]]);
    }
}

fn push_smooth(v: &mut Vec<f32>, p: [DVec3; 3], n: [DVec3; 3], col: [f32; 3]) {
    for k in 0..3 {
        let q = w(p[k]);
        let nw = smesh::printer_to_world(n[k]);
        v.extend_from_slice(&[q[0], q[1], q[2], nw.x as f32, nw.y as f32, nw.z as f32, col[0], col[1], col[2]]);
    }
}

fn boxx(v: &mut Vec<f32>, lo: DVec3, hi: DVec3, col: [f32; 3]) {
    let c = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
    let f = [
        [c(lo.x, lo.y, hi.z), c(hi.x, lo.y, hi.z), c(hi.x, hi.y, hi.z), c(lo.x, hi.y, hi.z)],
        [c(lo.x, lo.y, lo.z), c(hi.x, lo.y, lo.z), c(hi.x, lo.y, hi.z), c(lo.x, lo.y, hi.z)],
        [c(hi.x, lo.y, lo.z), c(hi.x, hi.y, lo.z), c(hi.x, hi.y, hi.z), c(hi.x, lo.y, hi.z)],
        [c(hi.x, hi.y, lo.z), c(lo.x, hi.y, lo.z), c(lo.x, hi.y, hi.z), c(hi.x, hi.y, hi.z)],
        [c(lo.x, hi.y, lo.z), c(lo.x, lo.y, lo.z), c(lo.x, lo.y, hi.z), c(lo.x, hi.y, hi.z)],
    ];
    for q in f {
        push_tri(v, q[0], q[1], q[2], col);
        push_tri(v, q[0], q[2], q[3], col);
    }
}

/// Surface of revolution around the tool axis between heights, radius per (angle, height).
fn revolve(v: &mut Vec<f32>, tip: DVec3, z0: f64, z1: f64, rings: usize, seg: usize, col: [f32; 3], radius: &dyn Fn(f64, f64) -> f64) {
    let pt = |a: f64, z: f64| {
        let r = radius(a, z);
        tip + DVec3::new(r * a.cos(), r * a.sin(), z)
    };
    for i in 0..rings {
        let za = z0 + (z1 - z0) * i as f64 / rings as f64;
        let zb = z0 + (z1 - z0) * (i + 1) as f64 / rings as f64;
        for k in 0..seg {
            let a0 = std::f64::consts::TAU * k as f64 / seg as f64;
            let a1 = std::f64::consts::TAU * (k + 1) as f64 / seg as f64;
            let (p00, p01, p10, p11) = (pt(a0, za), pt(a1, za), pt(a0, zb), pt(a1, zb));
            push_tri(v, p00, p01, p11, col);
            push_tri(v, p00, p11, p10, col);
        }
    }
}

fn disc(v: &mut Vec<f32>, c: DVec3, r: f64, up: bool, col: [f32; 3]) {
    let seg = 32;
    for k in 0..seg {
        let a0 = std::f64::consts::TAU * k as f64 / seg as f64;
        let a1 = std::f64::consts::TAU * (k + 1) as f64 / seg as f64;
        let p0 = c + DVec3::new(r * a0.cos(), r * a0.sin(), 0.0);
        let p1 = c + DVec3::new(r * a1.cos(), r * a1.sin(), 0.0);
        if up {
            push_tri(v, c, p0, p1, col);
        } else {
            push_tri(v, c, p1, p0, col);
        }
    }
}

/// The cutter (fluted, with its real tip), shank, collet nut, holder and spindle nose.
pub fn tool_mesh(t: &Tool, tip: DVec3, spin: f64, router: bool) -> Vec<f32> {
    let mut v = Vec::with_capacity(60_000);
    let r = t.radius();
    let carbide = [0.62, 0.64, 0.68];
    let steel = [0.78, 0.80, 0.83];
    let nut = [0.30, 0.32, 0.36];
    let flutes = t.flutes.max(1) as f64;
    let helix = 1.0 / (r.max(0.3) * 2.5);
    let groove = |a: f64, z: f64| {
        let ph = (a - spin - z * helix) * flutes;
        1.0 - 0.16 * (0.5 + 0.5 * ph.cos()).powi(3)
    };
    let tip_h = match t.kind {
        ToolKind::Ball => r,
        ToolKind::Bull => t.corner.min(r),
        ToolKind::VBit | ToolKind::Drill => r / (t.angle.clamp(1.0, 179.0).to_radians() / 2.0).tan(),
        ToolKind::Flat => 0.0,
    };
    // tip
    match t.kind {
        ToolKind::Flat => disc(&mut v, tip, r * 0.98, false, carbide),
        _ => {
            let prof = |z: f64| -> f64 {
                // radius of the tip at height z above the point
                match t.kind {
                    ToolKind::Ball => (r * r - (r - z).powi(2)).max(0.0).sqrt(),
                    ToolKind::Bull => {
                        let c = t.corner.min(r);
                        (r - c) + (c * c - (c - z).powi(2)).max(0.0).sqrt()
                    }
                    _ => z * (t.angle.clamp(1.0, 179.0).to_radians() / 2.0).tan(),
                }
            };
            revolve(&mut v, tip, 0.0, tip_h.max(1e-3), 10, 32, carbide, &|a, z| prof(z) * groove(a, z).max(0.9));
            if t.kind == ToolKind::Bull {
                disc(&mut v, tip, (r - t.corner).max(0.0), false, carbide);
            }
        }
    }
    let flute_top = t.flute_length.max(tip_h + 0.1);
    revolve(&mut v, tip, tip_h, flute_top, ((flute_top - tip_h) / 0.6).clamp(4.0, 60.0) as usize, 36, carbide, &|a, z| r * groove(a, z));
    // shank
    let sr = t.shank / 2.0;
    if (sr - r).abs() > 1e-3 {
        // step between cutter and shank
        revolve(&mut v, tip, flute_top, flute_top + (sr - r).abs().min(1.0), 1, 32, steel, &|_, z| r + (sr - r) * ((z - flute_top) / (sr - r).abs().min(1.0).max(1e-3)).min(1.0));
    }
    revolve(&mut v, tip, flute_top, t.stickout, 2, 32, steel, &|_, _| sr);
    // collet nut and holder
    let hr = t.holder / 2.0;
    disc(&mut v, tip + DVec3::Z * t.stickout, hr, false, nut);
    revolve(&mut v, tip, t.stickout, t.stickout + 14.0, 1, 6, nut, &|_, _| hr);
    revolve(&mut v, tip, t.stickout + 14.0, t.stickout + t.holder_length, 1, 40, [0.55, 0.57, 0.60], &|_, z| hr * 1.15 + (z - t.stickout - 14.0) * 0.25);
    // spindle nose
    let body = if router { 30.0 } else { 38.0 };
    let base = t.stickout + t.holder_length;
    disc(&mut v, tip + DVec3::Z * base, body, false, if router { [0.16, 0.16, 0.18] } else { [0.62, 0.64, 0.66] });
    revolve(&mut v, tip, base, base + 60.0, 1, 48, if router { [0.16, 0.16, 0.18] } else { [0.62, 0.64, 0.66] }, &|_, _| body);
    v
}

fn op_color(k: OpKind) -> [f32; 4] {
    match k {
        OpKind::Face => [0.55, 0.35, 0.85, 0.9],
        OpKind::Drill => [0.95, 0.75, 0.10, 0.95],
        OpKind::Rough => [0.15, 0.45, 0.95, 0.85],
        OpKind::Contour => [0.10, 0.70, 0.35, 0.95],
        OpKind::Parallel => [0.95, 0.45, 0.15, 0.9],
    }
}

fn num(ui: &mut egui::Ui, label: &str, v: &mut f64, lo: f64, hi: f64, unit: &str, dec: usize) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.set_min_height(26.0);
        ui.label(egui::RichText::new(label).size(13.0).color(TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(14.0);
            ui.add_sized(vec2(48.0, 22.0), egui::Label::new(egui::RichText::new(unit).size(12.5).color(DIM_TEXT)));
            let speed = ((hi - lo) / 400.0).max(10f64.powi(-(dec as i32)));
            changed = ui.add_sized(vec2(78.0, 22.0), egui::DragValue::new(v).range(lo..=hi).speed(speed).max_decimals(dec)).changed();
        });
    });
    changed
}

/// A value that is automatic unless the user overrides it.
fn opt_num(ui: &mut egui::Ui, label: &str, v: &mut Option<f64>, auto: f64, lo: f64, hi: f64, unit: &str, dec: usize) {
    ui.horizontal(|ui| {
        ui.set_min_height(26.0);
        let mut on = v.is_some();
        if super::dlgkit::chk(ui, &mut on, label, true) {
            *v = if on { Some(auto) } else { None };
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(14.0);
            ui.add_sized(vec2(48.0, 22.0), egui::Label::new(egui::RichText::new(unit).size(12.5).color(DIM_TEXT)));
            match v {
                Some(x) => {
                    let speed = ((hi - lo) / 400.0).max(10f64.powi(-(dec as i32)));
                    ui.add_sized(vec2(78.0, 22.0), egui::DragValue::new(x).range(lo..=hi).speed(speed).max_decimals(dec));
                }
                None => {
                    ui.add_sized(vec2(78.0, 22.0), egui::Label::new(egui::RichText::new(format!("auto {}", fmt_dec(auto, dec))).size(12.5).color(DIM_TEXT)));
                }
            }
        });
    });
}

fn fmt_dec(v: f64, dec: usize) -> String {
    format!("{:.*}", dec, v).replace('.', ",")
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, Color32::from_rgb(230, 234, 236));
    ui.painter().text(pos2(r.left() + 6.0, r.center().y), Align2::LEFT_CENTER, title, FontId::proportional(13.0), LABEL);
    ui.add_space(2.0);
}

fn fmt_time(s: f64) -> String {
    crate::slicer::gcode::fmt_time(s)
}

impl App {
    fn cnc_part_gen(&self) -> u64 {
        self.displayed().generation
    }

    pub fn cnc_sync_part(&mut self) {
        let gen = self.cnc_part_gen();
        if self.cnc.part.as_ref().is_some_and(|p| p.0 == gen) {
            return;
        }
        let tris = super::export::triangles(self.displayed());
        let mesh = Mesh::from_world_triangles(&tris);
        // a part with chamfers, slopes or rounds needs 3D finishing, else they stay stepped
        if crate::cam::generate::has_slopes(&mesh) {
            if let Some(op) = self.cnc.setup.ops.iter_mut().find(|o| o.kind == crate::cam::OpKind::Parallel && !o.enabled) {
                op.enabled = true;
                self.say("CNC: das Teil hat schräge oder gekrümmte Flächen – 3D-Schlichten wurde eingeschaltet.", MsgKind::Info);
            }
        }
        self.cnc.part = Some((gen, Arc::new(mesh)));
        self.cnc.scene = None;
    }

    /// The part as it is clamped: the model turned by the chosen orientation.
    fn cnc_model(&self) -> Option<Mesh> {
        let (_, m) = self.cnc.part.as_ref()?;
        Some(m.transformed(DMat3::from_cols_array(&self.cnc.rot)))
    }

    fn cnc_key(&self) -> u64 {
        let s = serde_json::to_string(&self.cnc.setup).unwrap_or_default();
        hash_of(&[&s, &format!("{:?}", self.cnc.rot), &self.cnc_part_gen().to_string()])
    }

    pub fn cnc_fresh(&self) -> bool {
        self.cnc.out.is_some() && self.cnc.out_key == self.cnc_key()
    }

    pub fn cnc_open(&mut self) {
        self.cnc.open = true;
        self.tab = Tab::Cnc;
    }

    pub fn cnc_close(&mut self) {
        self.cnc_cancel();
        self.cnc.open = false;
        self.cnc.playing = false;
        self.tab = Tab::Tools;
    }

    pub fn cnc_cancel(&mut self) {
        if let Some(j) = &self.cnc.job {
            j.prog.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn cnc_compute(&mut self) {
        if self.cnc.job.is_some() {
            return;
        }
        self.cnc_sync_part();
        let Some(model) = self.cnc_model() else { return };
        if model.is_empty() {
            self.say("CNC: das Modell hat noch keine Geometrie.", MsgKind::Warning);
            return;
        }
        let errs = self.cnc.setup.validate();
        if !errs.is_empty() {
            self.say(format!("CNC: {}", errs[0]), MsgKind::Error);
            self.cnc.error = Some(errs.join("\n"));
            return;
        }
        let setup = self.cnc.setup.clone();
        let prog = Arc::new(Progress::default());
        let out: JobOut = Arc::new(Mutex::new(None));
        let (p2, o2) = (prog.clone(), out.clone());
        std::thread::spawn(move || {
            let r = cam::run(&model, &setup, &p2);
            if let Ok(mut g) = o2.lock() {
                *g = Some(r);
            }
        });
        let key = self.cnc_key();
        self.cnc.job = Some(CamJob { prog, out, key });
        self.cnc.error = None;
        self.cnc.playing = false;
    }

    /// Computes on this thread (scripted demos).
    pub fn cnc_compute_blocking(&mut self) {
        self.cnc_compute();
        while let Some(j) = &self.cnc.job {
            if j.out.lock().map(|g| g.is_some()).unwrap_or(true) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let ctx = egui::Context::default();
        self.cnc_tick(&ctx);
    }

    pub fn cnc_tick(&mut self, ctx: &egui::Context) {
        if self.cnc.open && self.tab != Tab::Cnc {
            self.cnc_cancel();
            self.cnc.open = false;
            self.cnc.playing = false;
        }
        let here = self.tab == Tab::Cnc;
        if here && !self.cnc.active {
            std::mem::swap(&mut self.camera, &mut self.cnc.camera);
            self.cnc.active = true;
            if !self.cnc.cam_init {
                self.camera = Camera::default();
                self.fit_pending = true;
                self.cnc.cam_init = true;
            }
        } else if !here && self.cnc.active {
            std::mem::swap(&mut self.camera, &mut self.cnc.camera);
            self.cnc.active = false;
            self.fit_pending = false;
        }
        let done = self.cnc.job.as_ref().and_then(|j| j.out.lock().ok().and_then(|mut g| g.take()));
        if let Some(res) = done {
            let key = self.cnc.job.take().map(|j| j.key).unwrap_or(0);
            match res {
                Ok(o) => {
                    let e = o.sim.errors();
                    let moves: usize = o.program.ops.iter().map(|p| p.moves.len()).sum();
                    if e == 0 {
                        self.say(format!("CNC: {} Arbeitsgänge, {} Bewegungen, Laufzeit {} – Prüfung bestanden, keine Kollisionen.", o.program.ops.len(), moves, fmt_time(o.sim.time)), MsgKind::Success);
                    } else {
                        self.say(format!("CNC: {} Fehler in der Prüfung – NC-Code gesperrt. Siehe Prüfbericht.", e), MsgKind::Error);
                        self.cnc.report_open = true;
                    }
                    self.cnc.t = o.sim.time;
                    self.cnc.out = Some(Arc::new(o));
                    self.cnc.out_key = key;
                    self.cnc.pb = None;
                    self.cnc.heights = None;
                    self.cnc.scene = None;
                }
                Err(e) => {
                    self.say(format!("CNC: {}", e.lines().next().unwrap_or("")), if e.contains("abgebrochen") { MsgKind::Info } else { MsgKind::Error });
                    self.cnc.error = Some(e);
                }
            }
        }
        // playback clock
        if self.cnc.playing {
            let now = std::time::Instant::now();
            let dt = self.cnc.last.map(|l| (now - l).as_secs_f64()).unwrap_or(0.0).min(0.1);
            self.cnc.last = Some(now);
            self.cnc.t += dt * self.cnc.speed;
            let total = self.cnc.total();
            if self.cnc.t >= total {
                self.cnc.t = total;
                self.cnc.playing = false;
            }
            ctx.request_repaint();
        } else {
            self.cnc.last = None;
        }
        if self.cnc.job.is_some() {
            ctx.request_repaint();
        }
    }

    /// Current step and the tool position inside it.
    fn cnc_where(&self) -> Option<(usize, DVec3)> {
        let o = self.cnc.out.as_ref()?;
        let steps = &o.sim.steps;
        if steps.is_empty() {
            return None;
        }
        let t = self.cnc.t;
        let k = steps.partition_point(|s| s.t < t).min(steps.len() - 1);
        let s = &steps[k];
        let t0 = if k == 0 { 0.0 } else { steps[k - 1].t };
        let f = if s.t > t0 { ((t - t0) / (s.t - t0)).clamp(0.0, 1.0) } else { 1.0 };
        Some((k, s.from + (s.to - s.from) * f))
    }

    /// Material state at the playback time (cut incrementally; rewinding starts over).
    fn cnc_heights(&mut self) -> Option<Arc<Heights>> {
        let o = self.cnc.out.clone()?;
        let total = o.sim.time;
        let t = self.cnc.t;
        if (t - total).abs() < 1e-9 {
            // finished: the checker's own result
            if self.cnc.heights.is_none() || (self.cnc.pb_t - total).abs() > 1e-12 || self.cnc.pb.is_some() {
                let gen = self.cnc.gen();
                self.cnc.heights = Some(Arc::new(Heights { gen, nx: o.sim.field.nx, ny: o.sim.field.ny, h: Arc::new(o.sim.field.h.clone()), part: Arc::new(o.sim.part.clone()) }));
                self.cnc.pb_t = total;
                self.cnc.pb = None;
            }
            return self.cnc.heights.clone();
        }
        if (t - self.cnc.pb_t).abs() < 1e-12 && self.cnc.heights.is_some() {
            return self.cnc.heights.clone();
        }
        let f0 = &o.sim.field;
        if self.cnc.pb.is_none() || t < self.cnc.pb_t {
            let mut f = Field::new(f0.x0, f0.y0, f0.x0 + f0.cell * f0.nx as f64, f0.y0 + f0.cell * f0.ny as f64, f0.floor, f0.top, f0.cell);
            if f.nx != f0.nx || f.ny != f0.ny {
                f = f0.clone();
                f.h.iter_mut().for_each(|h| *h = f0.top as f32);
            }
            self.cnc.pb = Some(f);
            self.cnc.pb_steps = 0;
        }
        let (k, p) = self.cnc_where()?;
        let steps: &Vec<Step> = &o.sim.steps;
        let ops = &o.program.ops;
        if let Some(f) = self.cnc.pb.as_mut() {
            while self.cnc.pb_steps < k {
                let s = &steps[self.cnc.pb_steps];
                f.sweep(&ops[s.op as usize].tool, s.from, s.to, None);
                self.cnc.pb_steps += 1;
            }
            let s = &steps[k];
            f.sweep(&ops[s.op as usize].tool, s.from, p, None);
            let gen = self.cnc.counter + 1;
            self.cnc.counter = gen;
            self.cnc.heights = Some(Arc::new(Heights { gen, nx: f.nx, ny: f.ny, h: Arc::new(f.h.clone()), part: Arc::new(o.sim.part.clone()) }));
        }
        self.cnc.pb_t = t;
        self.cnc.heights.clone()
    }

    // ------------------------------------------------------------------ ribbon

    pub fn cnc_ribbon(&mut self, ui: &mut egui::Ui) {
        let busy = self.cnc.job.is_some();
        let has_part = !self.displayed().topo.polys.is_empty();
        let mname = self.cnc.setup.machine.name();
        let mat = self.cnc.setup.material().name;
        // ---- Maschine
        let (mr, matr) = group(ui, "Maschine", |ui| {
            if large(ui, Icon::Csys, "Maschine", !busy, self.cnc.machine_dlg.is_some()).on_hover_text("CNC-Maschine wählen (nach Hersteller oder Name sortierbar)").clicked() {
                let s = &self.cnc.setup;
                self.cnc.machine_dlg = Some(MachineDlg { draft: s.machine.clone(), filter: String::new(), sort: SortBy::Brand, asc: true, sel: Some(s.machine.name()) });
            }
            stack(ui, |ui| {
                let a = small(ui, Icon::Csys, &short(&mname, 28), !busy, false).on_hover_text("Maschine");
                let b = small(ui, Icon::PrFilament, &short(mat, 28), !busy, false).on_hover_text("Werkstoff");
                small(ui, Icon::Options, &format!("Steuerung: {}", self.cnc.setup.machine.controller.name()), false, false);
                (a, b)
            })
        });
        let mut set_m = None;
        drop_menu(ui, "cnc_machine", &mr, |ui| {
            ui.set_min_width(340.0);
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                let mut all: Vec<Machine> = self.cnc.user_machines.iter().cloned().chain(machines::all_machines()).collect();
                all.sort_by(|a, b| a.brand.cmp(&b.brand).then(a.model.cmp(&b.model)));
                for m in all {
                    let sel = m.name() == mname;
                    if menu_row(ui, Some(Icon::Csys), &format!("{}   {:.0}×{:.0}×{:.0}", m.name(), m.x, m.y, m.z), "", true, sel).clicked() {
                        set_m = Some(m);
                    }
                }
            });
        });
        if let Some(m) = set_m {
            self.cnc.setup.machine = m;
        }
        let mut set_mat = None;
        drop_menu(ui, "cnc_mat", &matr, |ui| {
            ui.set_min_width(300.0);
            egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| {
                for g in [MatGroup::Wood, MatGroup::Plastic, MatGroup::Metal] {
                    ui.label(egui::RichText::new(g.name()).size(12.0).color(DIM_TEXT));
                    for (i, m) in crate::cam::materials::by_group(g) {
                        if menu_row(ui, None, m.name, "", true, i == self.cnc.setup.material).clicked() {
                            set_mat = Some(i);
                        }
                    }
                }
            });
        });
        if let Some(i) = set_mat {
            self.cnc.setup.material = i;
        }
        // ---- Aufspannung
        group(ui, "Aufspannung", |ui| {
            let can = !busy && has_part;
            stack(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let q = std::f64::consts::FRAC_PI_2;
                    for (tip, m) in [("Teil 90° um X drehen", DMat3::from_rotation_x(q)), ("Teil 90° um Y drehen", DMat3::from_rotation_y(q)), ("Teil 90° um Z drehen", DMat3::from_rotation_z(q))] {
                        if tiny(ui, Icon::PrRotate, tip, can, false, false).clicked() {
                            self.cnc.rot = (m * DMat3::from_cols_array(&self.cnc.rot)).to_cols_array();
                        }
                    }
                });
                if small(ui, Icon::Undo, "Ausrichtung zurücksetzen", can, false).clicked() {
                    self.cnc.rot = DMat3::IDENTITY.to_cols_array();
                }
                if small(ui, Icon::PrStats, "Werkzeuge …", !busy, self.cnc.tools_open).clicked() {
                    self.cnc.tools_open = !self.cnc.tools_open;
                }
            });
        });
        // ---- Bearbeitung
        group(ui, "Bearbeitung", |ui| {
            for chunk in [&[0usize, 1, 2][..], &[3, 4][..]] {
                stack(ui, |ui| {
                    for &k in chunk {
                        if let Some(op) = self.cnc.setup.ops.get_mut(k) {
                            let mut on = op.enabled;
                            if super::dlgkit::chk(ui, &mut on, op.kind.name(), !busy) {
                                op.enabled = on;
                            }
                        }
                    }
                });
            }
        });
        // ---- Berechnen
        group(ui, "Berechnen", |ui| {
            if busy {
                if large(ui, Icon::Close, "Abbrechen", true, false).clicked() {
                    self.cnc_cancel();
                }
            } else if large(ui, Icon::Regenerate, if self.cnc_fresh() { "Neu\nberechnen" } else { "Bahnen\nberechnen" }, has_part, false)
                .on_hover_text("Werkzeugbahnen erzeugen und jede Bewegung auf Kollisionen prüfen")
                .clicked()
            {
                self.cnc_compute();
            }
        });
        // ---- Simulation
        let has = self.cnc.out.is_some();
        let speed_btn = group(ui, "Simulation", |ui| {
            let playing = self.cnc.playing;
            if large(ui, if playing { Icon::SimPause } else { Icon::Simulate }, if playing { "Pause" } else { "Abspielen" }, has, playing).clicked() {
                if !playing && self.cnc.t >= self.cnc.total() - 1e-9 {
                    self.cnc.t = 0.0;
                }
                self.cnc.playing = !playing;
            }
            let sp = stack(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if tiny(ui, Icon::Undo, "Zum Anfang", has, false, false).clicked() {
                        self.cnc.t = 0.0;
                        self.cnc.playing = false;
                    }
                    if tiny(ui, Icon::Redo, "Zum Ende (fertiges Teil)", has, false, false).clicked() {
                        self.cnc.t = self.cnc.total();
                        self.cnc.playing = false;
                    }
                });
                small(ui, Icon::PerfOptions, &format!("Tempo: {}×", self.cnc.speed), has, false)
            });
            stack(ui, |ui| {
                super::dlgkit::chk(ui, &mut self.cnc.show_tool, "Werkzeug", true);
                super::dlgkit::chk(ui, &mut self.cnc.show_paths, "Bahnen", true);
                if super::dlgkit::chk(ui, &mut self.cnc.show_part, "Fertigteil", true) {
                    self.cnc.scene = None;
                }
            });
            stack(ui, |ui| {
                super::dlgkit::chk(ui, &mut self.cnc.deviation, "Abweichung", has);
            });
            sp
        });
        drop_menu(ui, "cnc_speed", &speed_btn, |ui| {
            for s in [1.0, 5.0, 20.0, 100.0, 500.0, 2000.0] {
                if menu_row(ui, None, &format!("{}×", s), "", true, self.cnc.speed == s).clicked() {
                    self.cnc.speed = s;
                }
            }
        });
        // ---- Prüfung
        group(ui, "Prüfung", |ui| {
            let (icon, label) = match &self.cnc.out {
                None => (Icon::Measure, "Noch nicht\ngeprüft".to_string()),
                Some(o) if o.sim.safe() => (Icon::Measure, "Sicher\nkeine Kollision".to_string()),
                Some(o) => (Icon::Close, format!("{} Fehler\ngefunden", o.sim.errors())),
            };
            if large(ui, icon, &label, has, self.cnc.report_open).on_hover_text("Prüfbericht: alle Kollisionen, Warnungen und Hinweise").clicked() {
                self.cnc.report_open = !self.cnc.report_open;
            }
        });
        // ---- Ausgabe
        group(ui, "Ausgabe", |ui| {
            let ok = self.cnc_fresh() && self.cnc.out.as_ref().is_some_and(|o| o.sim.safe()) && !busy;
            let tip = if !self.cnc_fresh() {
                "Erst die Bahnen berechnen"
            } else if !ok {
                "Gesperrt: die Prüfung hat Fehler gefunden"
            } else {
                "Geprüften NC-Code für die Maschine speichern"
            };
            if large(ui, Icon::PrGcode, "NC-Code\nexportieren", ok, false).on_hover_text(tip).clicked() {
                self.cnc_export();
            }
        });
        group(ui, "Schließen", |ui| {
            if large(ui, Icon::Close, "CNC\nschließen", true, false).on_hover_text("Zurück zu Werkzeuge").clicked() {
                self.cnc_close();
            }
        });
    }

    pub fn cnc_export(&mut self) {
        let Some(o) = self.cnc.out.clone() else { return };
        if !self.cnc_fresh() {
            self.say("CNC: Modell oder Einstellungen geändert – bitte neu berechnen.", MsgKind::Warning);
            return;
        }
        let s = &self.cnc.setup;
        let files = match cam::nc::write(&o, s, &self.data.name, &crate::slicer::now_text()) {
            Ok(f) => f,
            Err(e) => {
                self.say(format!("CNC: {}", e), MsgKind::Error);
                return;
            }
        };
        let ext = s.machine.controller.ext();
        let name = format!("{}.{}", self.data.name, ext);
        let Some(path) = rfd::FileDialog::new().set_title("NC-Code exportieren").set_directory(self.work_dir()).set_file_name(&name).add_filter("NC-Code", &[ext]).save_file() else { return };
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| self.data.name.clone());
        let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let mut written = vec![];
        for (suffix, text) in files {
            let p = dir.join(format!("{}{}.{}", stem, suffix, ext));
            if let Err(e) = std::fs::write(&p, text) {
                self.say(format!("NC-Code konnte nicht gespeichert werden: {}", e), MsgKind::Error);
                return;
            }
            written.push(p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
        }
        if written.len() > 1 {
            self.say(format!("NC-Code gespeichert ({} Dateien, eine je Werkzeug – nach jedem Wechsel Z neu antasten): {}", written.len(), written.join(", ")), MsgKind::Success);
        } else {
            self.say(format!("NC-Code gespeichert: {}", dir.join(&written[0]).display()), MsgKind::Success);
        }
    }

    // ------------------------------------------------------------------ panel

    pub fn cnc_panel(&mut self, ui: &mut egui::Ui) {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
        ui.painter().rect_filled(r.expand2(vec2(4.0, 0.0)), 0.0, TITLE_BG);
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(20.0, 20.0)), Icon::Csys, true);
        ui.painter().text(pos2(r.left() + 34.0, r.center().y), Align2::LEFT_CENTER, "CNC-Einstellungen", FontId::proportional(14.0), TEXT);
        let busy = self.cnc.job.is_some();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_enabled_ui(!busy, |ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 3.0);
                self.cnc_panel_body(ui);
            });
        });
    }

    fn cnc_panel_body(&mut self, ui: &mut egui::Ui) {
        section(ui, "Maschine");
        {
            let m = &self.cnc.setup.machine;
            ui.label(egui::RichText::new(m.name()).size(14.0).color(TEXT));
            ui.label(
                egui::RichText::new(format!(
                    "{} · {:.0} × {:.0} × {:.0} mm · {:.0}–{:.0}/min · {:.0} W · {}",
                    if m.kind == MachineKind::Router { "Portalfräse" } else { "Fräsmaschine" },
                    m.x,
                    m.y,
                    m.z,
                    m.rpm_min,
                    m.rpm_max,
                    m.power,
                    m.controller.name()
                ))
                .size(12.0)
                .color(DIM_TEXT),
            );
        }
        if super::dlgkit::btn(ui, "Maschine wählen …", ui.available_width(), 0) {
            let s = &self.cnc.setup;
            self.cnc.machine_dlg = Some(MachineDlg { draft: s.machine.clone(), filter: String::new(), sort: SortBy::Brand, asc: true, sel: Some(s.machine.name()) });
        }
        section(ui, "Werkstoff");
        let names: Vec<String> = MATERIALS.iter().map(|m| format!("{} – {}", m.group.name(), m.name)).collect();
        let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let mut cur = self.cnc.setup.material;
        if super::dlgkit::combo(ui, "cnc_mat_combo", ui.available_width(), &mut cur, &refs, true, Some(Icon::PrFilament)) {
            self.cnc.setup.material = cur;
        }
        let m = self.cnc.setup.material();
        ui.label(egui::RichText::new(format!("Schnittgeschwindigkeit {:.0} m/min (VHM) · Dichte {:.2} g/cm³{}", m.vc, m.density, if m.coolant { " · Kühlung empfohlen" } else { "" })).size(12.0).color(DIM_TEXT));
        if !m.note.is_empty() {
            ui.label(egui::RichText::new(m.note).size(12.0).color(Color32::from_rgb(196, 90, 10)));
        }
        section(ui, "Rohteil");
        {
            let s = &mut self.cnc.setup;
            num(ui, "Aufmaß seitlich", &mut s.margin_xy, 0.0, 100.0, "mm", 1);
            num(ui, "Aufmaß oben", &mut s.margin_top, 0.0, 100.0, "mm", 1);
            num(ui, "Aufmaß unten (Boden bleibt)", &mut s.margin_bottom, 0.5, 100.0, "mm", 1);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Nullpunkt").size(13.0).color(TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(14.0);
                    let names: Vec<&str> = Origin::ALL.iter().map(|o| o.name()).collect();
                    let mut k = Origin::ALL.iter().position(|o| *o == s.origin).unwrap_or(0);
                    if super::dlgkit::combo(ui, "cnc_origin", 210.0, &mut k, &names, true, None) {
                        s.origin = Origin::ALL[k];
                    }
                });
            });
            num(ui, "Sicherheitshöhe über Rohteil", &mut s.safe_z, 2.0, 100.0, "mm", 1);
            num(ui, "Eilgang endet über Material", &mut s.clearance, 0.5, 20.0, "mm", 1);
        }
        if let Some(model) = self.cnc_model() {
            if let Some(job) = cam::Job::new(&model, &self.cnc.setup) {
                let sz = job.stock_size();
                ui.label(egui::RichText::new(format!("Rohteil {:.1} × {:.1} × {:.1} mm", sz.x, sz.y, sz.z)).size(12.0).color(DIM_TEXT));
            }
        }
        section(ui, "Arbeitsgänge");
        let tool_names: Vec<String> = self.cnc.setup.tools.iter().map(|t| t.label()).collect();
        let tool_nums: Vec<u32> = self.cnc.setup.tools.iter().map(|t| t.number).collect();
        let material = *self.cnc.setup.material();
        let machine = self.cnc.setup.machine.clone();
        for k in 0..self.cnc.setup.ops.len() {
            let open = self.cnc.panel_op == Some(k);
            let (title, enabled, kind) = {
                let op = &self.cnc.setup.ops[k];
                (op.kind.name(), op.enabled, op.kind)
            };
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
            ui.painter().rect_filled(r, 0.0, if resp.hovered() { HOVER_BG } else { Color32::from_rgb(244, 246, 247) });
            super::ribbon::tri(ui.painter(), pos2(r.left() + 10.0, r.center().y), TEXT);
            let c = op_color(kind);
            ui.painter().rect_filled(Rect::from_center_size(pos2(r.left() + 26.0, r.center().y), vec2(10.0, 10.0)), 2.0, Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
            ui.painter().text(pos2(r.left() + 38.0, r.center().y), Align2::LEFT_CENTER, format!("{}{}", title, if enabled { "" } else { "  (aus)" }), FontId::proportional(13.0), if enabled { TEXT } else { DIM_TEXT });
            if resp.clicked() {
                self.cnc.panel_op = if open { None } else { Some(k) };
            }
            if !open {
                continue;
            }
            let op = &mut self.cnc.setup.ops[k];
            ui.label(egui::RichText::new(op.kind.info()).size(12.0).color(DIM_TEXT));
            super::dlgkit::chk(ui, &mut op.enabled, "Ausführen", true);
            if op.kind == OpKind::Drill {
                ui.label(egui::RichText::new("Bohrer werden passend zu jeder runden senkrechten Bohrung gewählt (Bohrer nie größer als die Bohrung).").size(12.0).color(DIM_TEXT));
                continue;
            }
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Werkzeug").size(13.0).color(TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(14.0);
                    let refs: Vec<&str> = tool_names.iter().map(|s| s.as_str()).collect();
                    let mut cur = tool_nums.iter().position(|n| *n == op.tool).unwrap_or(0);
                    if super::dlgkit::combo(ui, &format!("cnc_tool{}", k), 230.0, &mut cur, &refs, true, None) {
                        op.tool = tool_nums[cur];
                    }
                });
            });
            let tool = self.cnc.setup.tools.iter().find(|t| t.number == op.tool).cloned();
            if let Some(t) = tool {
                let (c, _) = cam::cutting(&t, &material, &machine);
                opt_num(ui, "Zustellung", &mut op.stepdown, c.ap, 0.01, 50.0, "mm", 2);
                opt_num(ui, "Schrittweite (Anteil Ø)", &mut op.stepover, c.ae / t.diameter, 0.02, 0.9, "", 2);
                opt_num(ui, "Drehzahl", &mut op.rpm, c.rpm, machine.rpm_min.max(1.0), machine.rpm_max, "1/min", 0);
                opt_num(ui, "Vorschub", &mut op.feed, c.feed, 1.0, machine.feed_max, "mm/min", 0);
                if op.kind == OpKind::Rough {
                    num(ui, "Aufmaß", &mut op.leave, 0.0, 5.0, "mm", 2);
                }
            }
        }
        ui.add_space(8.0);
        for e in self.cnc.setup.validate() {
            ui.label(egui::RichText::new(e).size(12.5).color(Color32::from_rgb(200, 40, 30)));
        }
    }

    // ------------------------------------------------------------------ viewport

    fn cnc_scene(&mut self) -> Arc<Scene> {
        let out = self.cnc.out.clone();
        let key = hash_of(&[&format!("{:?}{}", out.as_ref().map(|o| Arc::as_ptr(o) as usize), self.cnc.show_part)]);
        if let Some((k, s)) = &self.cnc.scene {
            if *k == key {
                return s.clone();
            }
        }
        let gen = self.cnc.gen();
        let mut sc = Scene { gen, ..Default::default() };
        let (lo, hi, part) = match &out {
            Some(o) => (o.job.stock_lo, o.job.stock_hi, Some(o.job.part.clone())),
            None => match self.cnc_model().and_then(|m| cam::Job::new(&m, &self.cnc.setup)) {
                Some(j) => (j.stock_lo, j.stock_hi, Some(j.part)),
                None => (DVec3::new(0.0, 0.0, -10.0), DVec3::new(100.0, 100.0, 0.0), None),
            },
        };
        // stock grid (heights come from the texture)
        // one vertex per simulation cell (up to 1024 per side): walls stay crisp
        let (fx, fy) = out.as_ref().map(|o| (o.sim.field.nx, o.sim.field.ny)).unwrap_or((64, 64));
        let gx = fx.saturating_sub(1).clamp(8, 1024);
        let gy = fy.saturating_sub(1).clamp(8, 1024);
        for j in 0..=gy {
            for i in 0..=gx {
                sc.grid.extend_from_slice(&[i as f32 / gx as f32, j as f32 / gy as f32, 0.0, 0.0, 0.0, 0.0]);
            }
        }
        for j in 0..gy {
            for i in 0..gx {
                let a = (j * (gx + 1) + i) as u32;
                let b = a + 1;
                let c = a + (gx + 1) as u32;
                let d = c + 1;
                sc.grid_idx.extend_from_slice(&[a, b, d, a, d, c]);
            }
        }
        // the four walls of the stock: top edge follows the texture, bottom at the floor
        let mut wall = |pts: Vec<(f32, f32)>, n: [f32; 3]| {
            let base = (sc.grid.len() / 6) as u32;
            for &(u, v) in &pts {
                sc.grid.extend_from_slice(&[u, v, 2.0, n[0], n[1], n[2]]);
                sc.grid.extend_from_slice(&[u, v, 1.0, n[0], n[1], n[2]]);
            }
            for k in 0..pts.len() as u32 - 1 {
                let (t0, b0, t1, b1) = (base + 2 * k, base + 2 * k + 1, base + 2 * k + 2, base + 2 * k + 3);
                sc.grid_idx.extend_from_slice(&[t0, b0, b1, t0, b1, t1]);
            }
        };
        let edge = |n: usize| (0..=n).map(move |k| k as f32 / n as f32);
        // world normals: front −Y → (0, 0, 1) in Y-up world
        wall(edge(gx).map(|u| (u, 0.0)).collect(), [0.0, 0.0, 1.0]);
        wall(edge(gx).map(|u| (u, 1.0)).collect(), [0.0, 0.0, -1.0]);
        wall(edge(gy).map(|v| (0.0, v)).collect(), [-1.0, 0.0, 0.0]);
        wall(edge(gy).map(|v| (1.0, v)).collect(), [1.0, 0.0, 0.0]);
        // machine table under the stock
        let router = self.cnc.setup.machine.kind == MachineKind::Router;
        let m = 40.0;
        let tcol = if router { [0.60, 0.46, 0.30] } else { [0.42, 0.44, 0.47] };
        boxx(&mut sc.solid, DVec3::new(lo.x - m, lo.y - m, lo.z - 18.0), DVec3::new(hi.x + m, hi.y + m, lo.z - 0.02), tcol);
        if !router {
            // T-slots
            let mut y = lo.y - m + 20.0;
            while y < hi.y + m - 10.0 {
                boxx(&mut sc.solid, DVec3::new(lo.x - m, y, lo.z - 0.06), DVec3::new(hi.x + m, y + 8.0, lo.z - 0.015), [0.2, 0.21, 0.22]);
                y += 50.0;
            }
        }
        sc.part_from = sc.solid.len() / 9;
        if self.cnc.show_part {
            if let Some(p) = part {
                for i in 0..p.t.len() {
                    let [a, b, c] = p.tri(i);
                    let n = (b - a).cross(c - a).normalize_or_zero();
                    push_smooth(&mut sc.solid, [a, b, c], [n, n, n], [0.35, 0.65, 0.95]);
                }
            }
        }
        let sc = Arc::new(sc);
        self.cnc.scene = Some((key, sc.clone()));
        sc
    }

    pub fn cnc_viewport(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        super::ui::no_hand(ui.ctx(), resp.id);
        self.viewport_size = (rect.width().max(1.0), rect.height().max(1.0));
        let size = self.viewport_size;
        self.cnc_sync_part();
        let job = self.cnc.out.as_ref().map(|o| o.job.clone()).or_else(|| self.cnc_model().and_then(|m| cam::Job::new(&m, &self.cnc.setup)));
        let (lo, hi) = job.as_ref().map(|j| (j.stock_lo, j.stock_hi)).unwrap_or((DVec3::new(0.0, 0.0, -10.0), DVec3::new(100.0, 100.0, 0.0)));
        let mut bb = crate::kernel::math::BBox::default();
        for p in [lo, hi] {
            let q = smesh::printer_to_world(p);
            bb.add(q);
        }
        bb.add(smesh::printer_to_world(DVec3::new(lo.x, lo.y, hi.z + 40.0)));
        if self.fit_pending {
            self.camera.fit(&bb, size.0 / size.1);
            self.fit_pending = false;
        }
        self.camera.depth = bb.diagonal() * 10.0 + 2000.0;
        // navigation as everywhere in Freo
        let (scroll, zoom, mods, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers, i.pointer.clone()));
        let local = |p: Pos2| (p.x - rect.min.x, p.y - rect.min.y);
        let hover = resp.hover_pos();
        let over_timeline = hover.is_some_and(|h| h.y > rect.bottom() - 70.0) && self.cnc.out.is_some();
        if let Some(hp) = hover {
            if zoom != 1.0 {
                self.camera.zoom_at(1.0 / zoom as f64, local(hp), size);
            }
            if (scroll.y != 0.0 || scroll.x != 0.0) && !over_timeline {
                if mods.shift {
                    self.camera.pan(-scroll.x, -scroll.y, size.1);
                } else if mods.alt {
                    self.camera.orbit(-scroll.x, -scroll.y);
                } else {
                    self.camera.zoom_at((-scroll.y as f64 * 0.0025).exp(), local(hp), size);
                }
            }
        }
        let delta = pointer.delta();
        let orbit_btn = pointer.middle_down() || pointer.secondary_down() || (pointer.primary_down() && (mods.alt || !over_timeline));
        if resp.dragged() && orbit_btn && (delta.x != 0.0 || delta.y != 0.0) {
            if mods.shift {
                self.camera.pan(delta.x, delta.y, size.1);
            } else if mods.ctrl {
                self.camera.zoom_at((delta.y as f64 * 0.01).exp(), (size.0 / 2.0, size.1 / 2.0), size);
            } else {
                self.camera.orbit(delta.x, delta.y);
            }
        }
        if hover.is_some() && self.cnc.out.is_some() && ui.input(|i| i.key_pressed(egui::Key::Space)) {
            if !self.cnc.playing && self.cnc.t >= self.cnc.total() - 1e-9 {
                self.cnc.t = 0.0;
            }
            self.cnc.playing = !self.cnc.playing;
        }
        // ---- frame ----
        let scene = self.cnc_scene();
        let heights = self.cnc_heights().unwrap_or_else(|| {
            // before computing: the raw stock
            let (nx, ny) = (2, 2);
            Arc::new(Heights { gen: 0, nx, ny, h: Arc::new(vec![hi.z as f32; 4]), part: Arc::new(vec![f32::NEG_INFINITY; 4]) })
        });
        let where_ = self.cnc_where();
        let mut dynamic = vec![];
        let mut lines = vec![];
        if let (Some(o), Some((k, p))) = (self.cnc.out.clone(), where_) {
            let step = o.sim.steps[k];
            let tool = &o.program.ops[step.op as usize].tool;
            if self.cnc.show_tool {
                let spin = self.cnc.t * 20.0;
                dynamic = tool_mesh(tool, p, spin, self.cnc.setup.machine.kind == MachineKind::Router);
            }
            if self.cnc.show_paths {
                let steps = &o.sim.steps;
                let cap = 400_000usize;
                let from = k.saturating_sub(cap);
                // the operation running now (earlier ones would cover the part in lines)
                let cur_op = steps[k].op;
                for (si, s) in steps[from..=k].iter().enumerate() {
                    if s.op != cur_op {
                        continue;
                    }
                    let end = if from + si == k { p } else { s.to };
                    let c = if s.rapid { [0.90, 0.18, 0.15, 0.7] } else { op_color(o.program.ops[s.op as usize].kind) };
                    lines.extend_from_slice(&w(s.from));
                    lines.extend_from_slice(&c);
                    lines.extend_from_slice(&w(end));
                    lines.extend_from_slice(&c);
                }
            }
        }
        // stock outline before computing
        if self.cnc.out.is_none() {
            let c = [0.3, 0.33, 0.37, 0.8];
            let cs = [
                DVec3::new(lo.x, lo.y, hi.z),
                DVec3::new(hi.x, lo.y, hi.z),
                DVec3::new(hi.x, hi.y, hi.z),
                DVec3::new(lo.x, hi.y, hi.z),
            ];
            for i in 0..4 {
                lines.extend_from_slice(&w(cs[i]));
                lines.extend_from_slice(&c);
                lines.extend_from_slice(&w(cs[(i + 1) % 4]));
                lines.extend_from_slice(&c);
            }
        }
        let m = self.cnc.setup.material();
        let frame = GlFrame {
            mvp: self.camera.proj(size.0 / size.1) * self.camera.view(),
            view: self.camera.view(),
            scene,
            heights,
            origin: [lo.x as f32, lo.y as f32],
            size: [(hi.x - lo.x) as f32, (hi.y - lo.y) as f32],
            floor: lo.z as f32,
            top: hi.z as f32,
            look: m.look as i32,
            color: [m.color[0] as f32 / 255.0, m.color[1] as f32 / 255.0, m.color[2] as f32 / 255.0],
            deviation: self.cnc.deviation,
            part_alpha: if self.cnc.show_part { 0.28 } else { 0.0 },
            dynamic,
            lines,
        };
        let painter = ui.painter_at(rect);
        super::ui::graphics_background_bg(&painter, rect, self.creo.bg);
        if self.renderer.is_some() {
            let gpu = self.cnc.gpu.clone();
            let frame = Arc::new(frame);
            let cb = eframe::egui_glow::CallbackFn::new(move |_info, pp| {
                if let Ok(mut g) = gpu.lock() {
                    let r = g.get_or_insert_with(|| CncRenderer::new(pp.gl()));
                    r.paint(pp.gl(), &frame);
                }
            });
            painter.add(egui::PaintCallback { rect, callback: Arc::new(cb) });
        }
        // ---- overlays ----
        let cam = self.camera;
        let to_screen = |p: DVec3| cam.project(smesh::printer_to_world(p), size).map(|s| pos2(rect.min.x + s.0, rect.min.y + s.1));
        if let Some(o) = self.cnc.out.clone() {
            for (n, i) in o.sim.issues.iter().filter(|i| i.severity == Severity::Error).enumerate() {
                if let Some(sp) = to_screen(i.at) {
                    painter.circle(sp, 9.0, Color32::from_rgba_unmultiplied(220, 30, 20, 210), Stroke::new(2.0_f32, Color32::WHITE));
                    painter.text(sp, Align2::CENTER_CENTER, format!("{}", n + 1), FontId::proportional(11.0), Color32::WHITE);
                }
            }
            self.cnc_timeline(ui, rect, &o);
            self.cnc_hud(&painter, rect, &o);
        } else {
            let msg = if self.cnc.job.is_some() { "" } else { "Maschine, Werkstoff und Werkzeuge wählen, dann „Bahnen berechnen“." };
            painter.text(rect.center_bottom() - vec2(0.0, 40.0), Align2::CENTER_CENTER, msg, FontId::proportional(14.0), Color32::from_rgb(60, 66, 74));
        }
        if let Some(j) = &self.cnc.job {
            let k = j.prog.done.load(std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0;
            let stage = j.prog.stage.lock().map(|s| s.clone()).unwrap_or_default();
            let r = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 40.0), vec2(380.0, 34.0));
            painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(250, 251, 252, 236));
            painter.rect_stroke(r, 3.0, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
            let bar = Rect::from_min_size(r.min + vec2(10.0, 22.0), vec2(360.0, 5.0));
            painter.rect_filled(bar, 2.0, Color32::from_rgb(220, 224, 228));
            painter.rect_filled(Rect::from_min_size(bar.min, vec2(360.0 * k, 5.0)), 2.0, FILE_GREEN);
            painter.text(r.min + vec2(10.0, 4.0), Align2::LEFT_TOP, format!("{} ({:.0} %)", stage, k * 100.0), FontId::proportional(12.5), TEXT);
        }
        if self.cnc.out.is_some() && !self.cnc_fresh() {
            painter.text(rect.center_top() + vec2(0.0, 52.0), Align2::CENTER_TOP, "Modell oder Einstellungen geändert – Bahnen und Prüfung sind veraltet. Bitte neu berechnen.", FontId::proportional(13.5), Color32::from_rgb(196, 90, 10));
        }
    }

    fn cnc_hud(&self, painter: &egui::Painter, rect: Rect, o: &Output) {
        let Some((k, p)) = self.cnc_where() else { return };
        let s = o.sim.steps[k];
        let op = &o.program.ops[s.op as usize];
        let safe = o.sim.safe();
        let r = Rect::from_min_size(rect.min + vec2(12.0, 52.0), vec2(330.0, 150.0));
        painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(250, 251, 252, 232));
        painter.rect_stroke(r, 3.0, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
        let (badge, bc) = if safe { ("GEPRÜFT – KEINE KOLLISION", Color32::from_rgb(40, 140, 50)) } else { ("FEHLER – NC-CODE GESPERRT", Color32::from_rgb(200, 40, 30)) };
        painter.text(r.min + vec2(10.0, 8.0), Align2::LEFT_TOP, badge, FontId::proportional(12.5), bc);
        let lines = [
            ("Arbeitsgang", op.name.clone()),
            ("Position", format!("X {:.3}  Y {:.3}  Z {:.3}", p.x, p.y, p.z)),
            ("Bewegung", if s.rapid { "Eilgang".to_string() } else { format!("Vorschub {:.0} mm/min", s.feed) }),
            ("Spindel", format!("{:.0} /min", op.rpm)),
            ("Zeit", format!("{} von {}", fmt_time(self.cnc.t), fmt_time(o.sim.time))),
            ("Abgetragen", format!("{:.1} cm³ · Rest {:.1} cm³", o.sim.removed / 1000.0, o.sim.rest / 1000.0)),
        ];
        for (i, (k, v)) in lines.iter().enumerate() {
            let y = r.top() + 30.0 + i as f32 * 19.0;
            painter.text(pos2(r.left() + 10.0, y), Align2::LEFT_TOP, *k, FontId::proportional(12.0), LABEL);
            painter.text(pos2(r.left() + 95.0, y), Align2::LEFT_TOP, v, FontId::proportional(12.0), TEXT);
        }
    }

    fn cnc_timeline(&mut self, ui: &mut egui::Ui, rect: Rect, o: &Output) {
        let total = o.sim.time.max(1e-9);
        let (x0, x1) = (rect.left() + 60.0, rect.right() - 60.0);
        let y = rect.bottom() - 30.0;
        let p = ui.painter_at(rect);
        // operations as coloured bands
        let mut t0 = 0.0;
        let mut cur = o.sim.steps.first().map(|s| s.op).unwrap_or(0);
        let seg = |a: f64, b: f64, op: u16| {
            let c = op_color(o.program.ops[op as usize].kind);
            let xa = x0 + (x1 - x0) * (a / total) as f32;
            let xb = x0 + (x1 - x0) * (b / total) as f32;
            p.rect_filled(Rect::from_min_max(pos2(xa, y - 4.0), pos2(xb.max(xa + 1.0), y + 4.0)), 0.0, Color32::from_rgb((c[0] * 230.0) as u8, (c[1] * 230.0) as u8, (c[2] * 230.0) as u8));
        };
        for s in &o.sim.steps {
            if s.op != cur {
                seg(t0, s.t, cur);
                t0 = s.t;
                cur = s.op;
            }
        }
        seg(t0, total, cur);
        // error marks
        for i in o.sim.issues.iter().filter(|i| i.severity == Severity::Error) {
            let before: usize = o.program.ops[..i.op.min(o.program.ops.len())].iter().map(|p| p.moves.len()).sum();
            if let Some(s) = o.sim.steps.get(before + i.mv) {
                let x = x0 + (x1 - x0) * (s.t / total) as f32;
                p.line_segment([pos2(x, y - 12.0), pos2(x, y + 12.0)], Stroke::new(2.0_f32, Color32::from_rgb(220, 30, 20)));
            }
        }
        let id = ui.id().with("cnc_timeline");
        let r = ui.interact(Rect::from_min_max(pos2(x0 - 10.0, y - 14.0), pos2(x1 + 10.0, y + 14.0)), id, Sense::click_and_drag());
        if r.dragged() || r.clicked() {
            if let Some(pp) = r.interact_pointer_pos() {
                self.cnc.t = (((pp.x - x0) / (x1 - x0)).clamp(0.0, 1.0) as f64) * total;
                self.cnc.playing = false;
            }
        }
        let cx = x0 + (x1 - x0) * (self.cnc.t / total) as f32;
        p.circle(pos2(cx, y), 8.0, Color32::WHITE, Stroke::new(2.0_f32, ACCENT));
        p.text(pos2(x1 + 8.0, y), Align2::LEFT_CENTER, fmt_time(total), FontId::proportional(11.5), TEXT);
        p.text(pos2(x0 - 8.0, y), Align2::RIGHT_CENTER, fmt_time(self.cnc.t), FontId::proportional(11.5), TEXT);
    }

    // ------------------------------------------------------------------ windows

    pub fn cnc_windows(&mut self, ctx: &egui::Context) {
        if self.tab != Tab::Cnc {
            return;
        }
        self.cnc_machine_dialog(ctx);
        if self.cnc.tools_open {
            self.cnc_tools_window(ctx);
        }
        if self.cnc.report_open {
            self.cnc_report(ctx);
        }
    }

    fn cnc_report(&mut self, ctx: &egui::Context) {
        let Some(o) = self.cnc.out.clone() else {
            self.cnc.report_open = false;
            return;
        };
        let mut jump: Option<f64> = None;
        let open = super::dlgkit::cwin(ctx, "cnc_report", "Prüfbericht", false, pos2(420.0, 180.0), 620.0, |ui| {
            super::dlgkit::pad(ui, 10.0, |ui| {
                let e = o.sim.errors();
                if e == 0 {
                    ui.label(egui::RichText::new("Keine Kollisionen: kein Eilgang ins Material, kein Halter- oder Schaftkontakt, keine Verletzung des Teils oder des Tisches, alle Wege im Arbeitsbereich.").size(13.0).color(Color32::from_rgb(40, 140, 50)));
                } else {
                    ui.label(egui::RichText::new(format!("{} Fehler – der NC-Code bleibt gesperrt, bis alle behoben sind.", e)).size(13.5).color(Color32::from_rgb(200, 40, 30)));
                }
                super::dlgkit::hline(ui);
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    let mut n = 0;
                    for i in &o.sim.issues {
                        let (tag, col) = match i.severity {
                            Severity::Error => {
                                n += 1;
                                (format!("Fehler {}", n), Color32::from_rgb(200, 40, 30))
                            }
                            Severity::Warning => ("Warnung".to_string(), Color32::from_rgb(196, 120, 10)),
                            Severity::Info => ("Hinweis".to_string(), DIM_TEXT),
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.label(egui::RichText::new(tag).size(12.5).color(col));
                            ui.label(egui::RichText::new(&i.text).size(12.5).color(TEXT));
                            if i.count > 1 {
                                ui.label(egui::RichText::new(format!("({}×)", i.count)).size(12.0).color(DIM_TEXT));
                            }
                            if i.severity == Severity::Error && ui.small_button("Zeigen").clicked() {
                                let before: usize = o.program.ops[..i.op.min(o.program.ops.len())].iter().map(|p| p.moves.len()).sum();
                                if let Some(s) = o.sim.steps.get(before + i.mv) {
                                    jump = Some(s.t);
                                }
                            }
                        });
                    }
                });
                super::dlgkit::hline(ui);
                for (k, v) in [
                    ("Laufzeit", fmt_time(o.sim.time)),
                    ("Arbeitsgänge", o.program.ops.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(" · ")),
                    ("Abgetragen", format!("{:.1} cm³", o.sim.removed / 1000.0)),
                    ("Restmaterial", format!("{:.2} cm³", o.sim.rest / 1000.0)),
                ] {
                    ui.horizontal(|ui| {
                        ui.add_sized(vec2(100.0, 18.0), egui::Label::new(egui::RichText::new(k).color(LABEL)));
                        ui.label(v);
                    });
                }
            });
        });
        if let Some(t) = jump {
            self.cnc.t = t;
            self.cnc.playing = false;
        }
        if !open {
            self.cnc.report_open = false;
        }
    }

    fn cnc_tools_window(&mut self, ctx: &egui::Context) {
        let mut remove = None;
        let open = super::dlgkit::cwin(ctx, "cnc_tools", "Werkzeuge", false, pos2(360.0, 160.0), 860.0, |ui| {
            super::dlgkit::pad(ui, 10.0, |ui| {
                egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
                    egui::Grid::new("cnc_tool_grid").striped(true).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
                        for h in ["Nr.", "Art", "Ø mm", "Schneiden", "Schneidenl.", "Schaft Ø", "Auskragung", "Halter Ø", "Eckradius / Winkel", ""] {
                            ui.label(egui::RichText::new(h).size(12.0).color(DIM_TEXT));
                        }
                        ui.end_row();
                        for (k, t) in self.cnc.setup.tools.iter_mut().enumerate() {
                            let mut nb = t.number as f64;
                            if ui.add(egui::DragValue::new(&mut nb).range(1.0..=999.0).speed(0.1)).changed() {
                                t.number = nb as u32;
                            }
                            let names: Vec<&str> = ToolKind::ALL.iter().map(|x| x.name()).collect();
                            let mut cur = ToolKind::ALL.iter().position(|x| *x == t.kind).unwrap_or(0);
                            if super::dlgkit::combo(ui, &format!("cnc_tk{}", k), 150.0, &mut cur, &names, true, None) {
                                t.kind = ToolKind::ALL[cur];
                                if t.kind == ToolKind::Ball {
                                    t.corner = t.radius();
                                }
                                if matches!(t.kind, ToolKind::VBit | ToolKind::Drill) && t.angle <= 0.0 {
                                    t.angle = if t.kind == ToolKind::Drill { 118.0 } else { 90.0 };
                                }
                            }
                            ui.add(egui::DragValue::new(&mut t.diameter).range(0.1..=100.0).speed(0.01).max_decimals(3));
                            let mut z = t.flutes as f64;
                            if ui.add(egui::DragValue::new(&mut z).range(1.0..=12.0).speed(0.05)).changed() {
                                t.flutes = z as u32;
                            }
                            ui.add(egui::DragValue::new(&mut t.flute_length).range(0.1..=200.0).speed(0.05));
                            ui.add(egui::DragValue::new(&mut t.shank).range(0.1..=50.0).speed(0.01));
                            ui.add(egui::DragValue::new(&mut t.stickout).range(0.5..=300.0).speed(0.1));
                            ui.add(egui::DragValue::new(&mut t.holder).range(1.0..=200.0).speed(0.1));
                            match t.kind {
                                ToolKind::Bull => {
                                    ui.add(egui::DragValue::new(&mut t.corner).range(0.0..=50.0).speed(0.01).prefix("R "));
                                }
                                ToolKind::VBit | ToolKind::Drill => {
                                    ui.add(egui::DragValue::new(&mut t.angle).range(10.0..=170.0).speed(0.2).suffix(" °"));
                                }
                                _ => {
                                    ui.label("");
                                }
                            }
                            if ui.small_button("Entfernen").clicked() {
                                remove = Some(k);
                            }
                            ui.end_row();
                        }
                    });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if super::dlgkit::btn(ui, "Neuer Schaftfräser", 160.0, 0) {
                        let n = self.cnc.setup.tools.iter().map(|t| t.number).max().unwrap_or(0) + 1;
                        self.cnc.setup.tools.push(Tool::flat(n, 6.0, 2));
                    }
                    if super::dlgkit::btn(ui, "Standardbibliothek", 160.0, 0) {
                        self.cnc.setup.tools = crate::cam::tools::library();
                    }
                });
                for t in &self.cnc.setup.tools {
                    if let Some(e) = t.validate() {
                        ui.label(egui::RichText::new(e).size(12.5).color(Color32::from_rgb(200, 40, 30)));
                    }
                }
            });
        });
        if let Some(k) = remove {
            self.cnc.setup.tools.remove(k);
        }
        if !open {
            self.cnc.tools_open = false;
        }
    }

    fn cnc_machine_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.cnc.machine_dlg.take() else { return };
        let mut keep = true;
        let mut apply = false;
        let mut save_own = false;
        let mut all: Vec<Machine> = self.cnc.user_machines.iter().cloned().chain(machines::all_machines()).collect();
        let open = super::dlgkit::cwin(ctx, "cnc_machine_dlg", "CNC-Maschine", false, pos2(260.0, 120.0), 980.0, |ui| {
            super::dlgkit::pad(ui, 10.0, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(520.0);
                        ui.horizontal(|ui| {
                            super::dlgkit::lbl(ui, "Suchen");
                            super::dlgkit::field(ui, &mut d.filter, 440.0, true);
                        });
                        let q = d.filter.to_lowercase();
                        all.retain(|m| q.is_empty() || m.name().to_lowercase().contains(&q) || m.controller.name().to_lowercase().contains(&q));
                        match d.sort {
                            SortBy::Brand => all.sort_by(|a, b| a.brand.to_lowercase().cmp(&b.brand.to_lowercase()).then(a.model.cmp(&b.model))),
                            SortBy::Name => all.sort_by(|a, b| a.model.to_lowercase().cmp(&b.model.to_lowercase()).then(a.brand.cmp(&b.brand))),
                        }
                        if !d.asc {
                            all.reverse();
                        }
                        // header: click to sort
                        let (hr, _) = ui.allocate_exact_size(vec2(520.0, 26.0), Sense::hover());
                        ui.painter().rect_filled(hr, 0.0, Color32::from_rgb(232, 236, 238));
                        let cols = [(SortBy::Brand, "Hersteller", 0.0), (SortBy::Name, "Name", 130.0)];
                        for (sb, label, x) in cols {
                            let r = Rect::from_min_size(hr.min + vec2(x, 0.0), vec2(125.0, 26.0));
                            let resp = ui.interact(r, ui.id().with(("cnc_sort", label)), Sense::click());
                            let arrow = if d.sort == sb { if d.asc { " ▲" } else { " ▼" } } else { "" };
                            ui.painter().text(r.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, format!("{}{}", label, arrow), FontId::proportional(13.0), if resp.hovered() { ACCENT } else { TEXT });
                            if resp.clicked() {
                                if d.sort == sb {
                                    d.asc = !d.asc;
                                } else {
                                    d.sort = sb;
                                    d.asc = true;
                                }
                            }
                        }
                        for (label, x) in [("Art", 270.0), ("Arbeitsbereich", 340.0), ("Spindel", 450.0)] {
                            ui.painter().text(hr.min + vec2(x, 13.0), Align2::LEFT_CENTER, label, FontId::proportional(13.0), DIM_TEXT);
                        }
                        let (lr, _) = super::dlgkit::white_box(ui, 520.0, 420.0, Color32::WHITE);
                        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(lr.shrink(2.0)).layout(egui::Layout::top_down(egui::Align::Min)));
                        egui::ScrollArea::vertical().id_salt("cnc_mlist").show(&mut child, |ui| {
                            for m in &all {
                                let sel = d.sel.as_deref() == Some(m.name().as_str());
                                let (rr, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                                if sel {
                                    ui.painter().rect_filled(rr, 0.0, super::dlgkit::ROW_SEL);
                                } else if resp.hovered() {
                                    ui.painter().rect_filled(rr, 0.0, HOVER_BG);
                                }
                                let f = FontId::proportional(12.5);
                                ui.painter().with_clip_rect(Rect::from_min_size(rr.min, vec2(125.0, 24.0))).text(rr.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, if m.brand.is_empty() { "Eigene" } else { &m.brand }, f.clone(), TEXT);
                                ui.painter().with_clip_rect(Rect::from_min_size(rr.min + vec2(130.0, 0.0), vec2(135.0, 24.0))).text(rr.left_center() + vec2(138.0, 0.0), Align2::LEFT_CENTER, &m.model, f.clone(), TEXT);
                                ui.painter().text(rr.left_center() + vec2(270.0, 0.0), Align2::LEFT_CENTER, if m.kind == MachineKind::Router { "Portal" } else { "Fräse" }, f.clone(), DIM_TEXT);
                                ui.painter().text(rr.left_center() + vec2(340.0, 0.0), Align2::LEFT_CENTER, format!("{:.0}×{:.0}×{:.0}", m.x, m.y, m.z), f.clone(), DIM_TEXT);
                                ui.painter().text(rr.left_center() + vec2(450.0, 0.0), Align2::LEFT_CENTER, format!("{:.0}k/min", m.rpm_max / 1000.0), f, DIM_TEXT);
                                if resp.clicked() {
                                    d.sel = Some(m.name());
                                    d.draft = m.clone();
                                }
                            }
                        });
                        ui.add_space(6.0);
                        if super::dlgkit::btn(ui, "Neue eigene Maschine", 200.0, 0) {
                            d.draft = machines::custom_machine();
                            d.sel = None;
                        }
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.set_width(400.0);
                        let m = &mut d.draft;
                        ui.horizontal(|ui| {
                            super::dlgkit::lbl(ui, "Hersteller");
                            super::dlgkit::field(ui, &mut m.brand, 120.0, true);
                            super::dlgkit::lbl(ui, "Name");
                            super::dlgkit::field(ui, &mut m.model, 160.0, true);
                        });
                        egui::ScrollArea::vertical().id_salt("cnc_mdetail").max_height(470.0).show(ui, |ui| {
                            ui.spacing_mut().item_spacing = vec2(6.0, 3.0);
                            section(ui, "Bauart");
                            let mut mill = m.kind == MachineKind::Mill;
                            if super::dlgkit::chk(ui, &mut mill, "Steife Fräsmaschine (alle Metalle)", true) {
                                m.kind = if mill { MachineKind::Mill } else { MachineKind::Router };
                            }
                            section(ui, "Verfahrwege");
                            num(ui, "X", &mut m.x, 10.0, 10000.0, "mm", 0);
                            num(ui, "Y", &mut m.y, 10.0, 10000.0, "mm", 0);
                            num(ui, "Z", &mut m.z, 10.0, 2000.0, "mm", 0);
                            num(ui, "Max. Werkstückhöhe", &mut m.clearance, 10.0, 2000.0, "mm", 0);
                            section(ui, "Spindel und Antriebe");
                            num(ui, "Drehzahl min", &mut m.rpm_min, 0.0, 100000.0, "1/min", 0);
                            num(ui, "Drehzahl max", &mut m.rpm_max, 100.0, 100000.0, "1/min", 0);
                            num(ui, "Leistung", &mut m.power, 10.0, 100000.0, "W", 0);
                            num(ui, "Max. Vorschub", &mut m.feed_max, 10.0, 100000.0, "mm/min", 0);
                            num(ui, "Eilgang", &mut m.rapid, 10.0, 100000.0, "mm/min", 0);
                            section(ui, "Steuerung");
                            let names: Vec<&str> = machines::Controller::ALL.iter().map(|c| c.name()).collect();
                            let mut k = machines::Controller::ALL.iter().position(|c| *c == m.controller).unwrap_or(0);
                            if super::dlgkit::combo(ui, "cnc_ctrl", 220.0, &mut k, &names, true, None) {
                                m.controller = machines::Controller::ALL[k];
                            }
                            super::dlgkit::chk(ui, &mut m.atc, "Werkzeugwechsler", true);
                            super::dlgkit::chk(ui, &mut m.coolant, "Kühlmittel (M8/M9)", true);
                            ui.label(egui::RichText::new("Werte laut Hersteller (Nenn-Arbeitsbereich). Vor dem ersten Lauf an der eigenen Maschine prüfen.").size(11.5).color(DIM_TEXT));
                            if let Some(e) = m.validate() {
                                ui.label(egui::RichText::new(e).size(12.5).color(Color32::from_rgb(200, 40, 30)));
                            }
                        });
                    });
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if super::dlgkit::btn(ui, "Als eigene Maschine speichern", 230.0, 0) {
                        save_own = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if super::dlgkit::btn(ui, "Abbrechen", 100.0, 0) {
                            keep = false;
                        }
                        if super::dlgkit::btn(ui, "OK", 100.0, 1) {
                            apply = true;
                        }
                    });
                });
            });
        });
        if save_own {
            let mut m = d.draft.clone();
            if m.model.trim().is_empty() {
                m.model = "Eigene Maschine".into();
            }
            self.cnc.user_machines.retain(|x| x.name() != m.name());
            self.cnc.user_machines.insert(0, m.clone());
            d.sel = Some(m.name());
            self.say(format!("Maschine „{}“ gespeichert.", m.name()), MsgKind::Success);
        }
        if apply {
            match d.draft.validate() {
                Some(e) => self.say(format!("CNC: {}", e), MsgKind::Error),
                None => {
                    self.cnc.setup.machine = d.draft.clone();
                    keep = false;
                }
            }
        }
        if keep && open {
            self.cnc.machine_dlg = Some(d);
        }
    }
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n - 1).collect();
        t.push('…');
        t
    }
}
