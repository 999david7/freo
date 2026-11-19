//! 3D-Druck: the slicer inside Freo. A ribbon tab with printer, filament and quality, the part
//! on the printer's build plate (lay flat, turn, copies), slicing in the background, a preview
//! of the printed part in the filament's colour (or by feature, speed, flow, fan, …) with a
//! layer and a move slider, statistics, and the G-code export.

use super::icons::{self, Icon};
use super::print_gl::{Beads, Colors, Frame as GlFrame, PlateRenderer, Scene};
use super::ribbon::{drop_menu, group, large, menu_row, small, stack, theme::*, tiny};
use super::*;
use crate::slicer::config::*;
use crate::slicer::gcode::{self, Kind};
use crate::slicer::mesh::{self as smesh, Mesh, Placement};
use crate::slicer::plan::Role;
use crate::slicer::{self, printers, Progress, Settings, Sliced};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind};
use glam::{DMat3, DVec3};
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorMode {
    Realistic,
    Feature,
    Speed,
    Height,
    Width,
    Flow,
    Fan,
    Temp,
    LayerTime,
}

impl ColorMode {
    pub const ALL: [ColorMode; 9] = [
        ColorMode::Realistic,
        ColorMode::Feature,
        ColorMode::Speed,
        ColorMode::Height,
        ColorMode::Width,
        ColorMode::Flow,
        ColorMode::Fan,
        ColorMode::Temp,
        ColorMode::LayerTime,
    ];
    pub fn name(&self) -> &'static str {
        match self {
            ColorMode::Realistic => "Realistisch",
            ColorMode::Feature => "Linientyp",
            ColorMode::Speed => "Geschwindigkeit",
            ColorMode::Height => "Schichthöhe",
            ColorMode::Width => "Linienbreite",
            ColorMode::Flow => "Volumenstrom",
            ColorMode::Fan => "Lüfter",
            ColorMode::Temp => "Temperatur",
            ColorMode::LayerTime => "Schichtzeit",
        }
    }
    fn unit(&self) -> &'static str {
        match self {
            ColorMode::Speed => "mm/s",
            ColorMode::Height | ColorMode::Width => "mm",
            ColorMode::Flow => "mm³/s",
            ColorMode::Fan => "%",
            ColorMode::Temp => "°C",
            ColorMode::LayerTime => "s",
            _ => "",
        }
    }
}

/// A finished slice with the preview geometry built beside it.
pub struct Ready {
    pub sliced: Sliced,
    pub beads: Arc<Beads>,
    pub settings: Settings,
}

type JobOut = Arc<Mutex<Option<Result<Ready, String>>>>;

pub struct Job {
    pub prog: Arc<Progress>,
    out: JobOut,
    key: u64,
}

/// The part as modelled, in the printer frame (Z up), with smooth normals for display.
struct Part {
    gen: u64,
    mesh: Arc<Mesh>,
    /// triangles: 3 × (position, normal)
    shade: Arc<Vec<[(DVec3, DVec3); 3]>>,
}

#[derive(Clone)]
pub struct PrinterDlg {
    pub draft: Printer,
    pub filter: String,
    pub sel: Option<usize>,
}

pub struct PrintUi {
    /// the 3D-Druck application is open (its tab is shown)
    pub open: bool,
    pub settings: Settings,
    pub quality: usize,
    pub placement: Placement,
    pub user_printers: Vec<Printer>,
    pub job: Option<Job>,
    pub ready: Option<Arc<Ready>>,
    pub ready_key: u64,
    pub error: Option<String>,
    pub preview: bool,
    pub mode: ColorMode,
    pub layer_lo: usize,
    pub layer_hi: usize,
    /// moves shown of the top layer (usize::MAX = all)
    pub cut: usize,
    pub show_travel: bool,
    pub show_ghost: bool,
    pub hidden: HashSet<Role>,
    pub camera: Camera,
    cam_init: bool,
    active: bool,
    pub panel_tab: usize,
    pub printer_dlg: Option<PrinterDlg>,
    pub stats_open: bool,
    pub lay_flat_pick: bool,
    drag: Option<[f64; 2]>,
    part: Option<Part>,
    scene: Option<(u64, Arc<Scene>, usize)>,
    colors: Option<(u64, Arc<Colors>)>,
    gpu: Arc<Mutex<Option<PlateRenderer>>>,
    counter: u64,
    hover_part: bool,
    /// a slider is being dragged: 0 none, 1 low, 2 high, 3 move
    slider_drag: u8,
}

impl Default for PrintUi {
    fn default() -> Self {
        let settings = Settings::default();
        PrintUi {
            open: false,
            settings,
            quality: 3,
            placement: Placement::default(),
            user_printers: vec![],
            job: None,
            ready: None,
            ready_key: 0,
            error: None,
            preview: false,
            mode: ColorMode::Realistic,
            layer_lo: 0,
            layer_hi: usize::MAX,
            cut: usize::MAX,
            show_travel: false,
            show_ghost: false,
            hidden: HashSet::new(),
            camera: Camera::default(),
            cam_init: false,
            active: false,
            panel_tab: 0,
            printer_dlg: None,
            stats_open: false,
            lay_flat_pick: false,
            drag: None,
            part: None,
            scene: None,
            colors: None,
            gpu: Arc::new(Mutex::new(None)),
            counter: 1,
            hover_part: false,
            slider_drag: 0,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    settings: Settings,
    quality: usize,
    user_printers: Vec<Printer>,
}

impl PrintUi {
    /// Settings kept between sessions.
    pub fn to_json(&self) -> String {
        serde_json::to_string(&Saved { settings: self.settings.clone(), quality: self.quality, user_printers: self.user_printers.clone() }).unwrap_or_default()
    }
    pub fn load_json(&mut self, s: &str) {
        if let Ok(v) = serde_json::from_str::<Saved>(s) {
            if v.settings.validate().is_empty() {
                self.settings = v.settings;
            }
            self.quality = v.quality.min(5);
            self.user_printers = v.user_printers;
        }
    }
    fn next_gen(&mut self) -> u64 {
        self.counter += 1;
        self.counter
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
}

fn hash_of(parts: &[&str]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in parts {
        p.hash(&mut h);
    }
    h.finish()
}

fn rgbf(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

/// Printer frame → Freo world (Y up).
fn w(p: DVec3) -> [f32; 3] {
    let q = smesh::printer_to_world(p);
    [q.x as f32, q.y as f32, q.z as f32]
}

fn wf(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[2], -p[1]]
}

/// Tubes along the extrusion moves (four corners per point, mitred at the joints) and lines
/// for the travel moves.
pub fn build_beads(moves: &[gcode::Move]) -> Beads {
    let n = moves.len();
    let mut b = Beads { idx_before: vec![0; n + 1], travel_before: vec![0; n + 1], ..Default::default() };
    let same = |a: &gcode::Move, c: &gcode::Move| {
        a.role == c.role && (a.width - c.width).abs() < 1e-4 && (a.height - c.height).abs() < 1e-4 && (a.b[0] - c.a[0]).abs() < 1e-4 && (a.b[1] - c.a[1]).abs() < 1e-4 && (a.b[2] - c.a[2]).abs() < 1e-3
    };
    let seg_len = |m: &gcode::Move| ((m.b[0] - m.a[0]).powi(2) + (m.b[1] - m.a[1]).powi(2)).sqrt();
    let mut i = 0;
    while i < n {
        let m = &moves[i];
        if m.kind == Kind::Extrude && seg_len(m) > 1e-5 {
            let mut j = i + 1;
            while j < n && moves[j].kind == Kind::Extrude && seg_len(&moves[j]) > 1e-5 && same(&moves[j - 1], &moves[j]) {
                j += 1;
            }
            emit_run(&mut b, moves, i, j);
            i = j;
        } else {
            b.idx_before[i] = b.idx.len() as u32;
            b.travel_before[i] = (b.travel.len() / 3) as u32;
            if matches!(m.kind, Kind::Travel | Kind::Wipe) {
                b.travel.extend_from_slice(&wf(m.a));
                b.travel.extend_from_slice(&wf(m.b));
                b.travel_move.push(i as u32);
                b.travel_move.push(i as u32);
            }
            i += 1;
        }
    }
    b.idx_before[n] = b.idx.len() as u32;
    b.travel_before[n] = (b.travel.len() / 3) as u32;
    b
}

fn emit_run(b: &mut Beads, moves: &[gcode::Move], i: usize, j: usize) {
    // points of the run: start of the first move, then every end
    let mut pts: Vec<[f32; 3]> = vec![moves[i].a];
    pts.extend(moves[i..j].iter().map(|m| m.b));
    let (wd, h) = (moves[i].width.max(0.05), moves[i].height.max(0.02));
    let np = pts.len();
    let dir = |k: usize| {
        let (a, c) = (pts[k], pts[k + 1]);
        let (dx, dy) = (c[0] - a[0], c[1] - a[1]);
        let l = (dx * dx + dy * dy).sqrt().max(1e-9);
        [dx / l, dy / l]
    };
    let base = (b.pos.len() / 3) as u32;
    for k in 0..np {
        let d = if k == 0 {
            dir(0)
        } else if k == np - 1 {
            dir(np - 2)
        } else {
            let (d0, d1) = (dir(k - 1), dir(k));
            let s = [d0[0] + d1[0], d0[1] + d1[1]];
            let l = (s[0] * s[0] + s[1] * s[1]).sqrt();
            if l < 1e-4 {
                d1
            } else {
                [s[0] / l, s[1] / l]
            }
        };
        // left of the direction; scaled to keep the width through the turn
        let mut side = [-d[1], d[0]];
        if k > 0 && k < np - 1 {
            let d0 = dir(k - 1);
            let s0 = [-d0[1], d0[0]];
            let c = (side[0] * s0[0] + side[1] * s0[1]).max(0.5);
            side = [side[0] / c, side[1] / c];
        }
        let p = pts[k];
        let hw = wd / 2.0;
        let corners = [
            ([p[0] - side[0] * hw, p[1] - side[1] * hw, p[2] - h / 2.0], [-side[0], -side[1], 0.0]),
            ([p[0], p[1], p[2]], [0.0, 0.0, 1.0]),
            ([p[0] + side[0] * hw, p[1] + side[1] * hw, p[2] - h / 2.0], [side[0], side[1], 0.0]),
            ([p[0], p[1], p[2] - h], [0.0, 0.0, -1.0]),
        ];
        for (q, nrm) in corners {
            b.pos.extend_from_slice(&wf(q));
            let nw = wf(nrm);
            let l = (nw[0] * nw[0] + nw[1] * nw[1] + nw[2] * nw[2]).sqrt().max(1e-6);
            b.nrm.extend_from_slice(&[(nw[0] / l * 127.0) as i8, (nw[1] / l * 127.0) as i8, (nw[2] / l * 127.0) as i8, 0]);
        }
        b.point_move.push(if k == 0 { i as u32 } else { (i + k - 1) as u32 });
    }
    let v = |k: usize, c: u32| base + k as u32 * 4 + c;
    for s in 0..np - 1 {
        let mi = i + s;
        b.idx_before[mi] = b.idx.len() as u32;
        b.travel_before[mi] = (b.travel.len() / 3) as u32;
        if s == 0 {
            b.idx.extend_from_slice(&[v(0, 0), v(0, 1), v(0, 2), v(0, 0), v(0, 2), v(0, 3)]);
        }
        for c in 0..4u32 {
            let c1 = (c + 1) % 4;
            b.idx.extend_from_slice(&[v(s, c), v(s + 1, c), v(s + 1, c1), v(s, c), v(s + 1, c1), v(s, c1)]);
        }
        if s == np - 2 {
            let e = np - 1;
            b.idx.extend_from_slice(&[v(e, 0), v(e, 1), v(e, 2), v(e, 0), v(e, 2), v(e, 3)]);
        }
    }
}

fn ramp_u8(t: f32) -> [u8; 3] {
    let c = super::livesim::ramp(t);
    [(c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8]
}

/// Value of a move in a scalar colour mode.
fn value(mode: ColorMode, m: &gcode::Move, r: &Ready) -> f32 {
    match mode {
        ColorMode::Speed => m.speed,
        ColorMode::Height => m.height,
        ColorMode::Width => m.width,
        ColorMode::Flow => m.flow(&r.settings.filament),
        ColorMode::Fan => m.fan * 100.0,
        ColorMode::Temp => m.temp,
        ColorMode::LayerTime => r.sliced.out.stats.layer_time.get(m.layer as usize).copied().unwrap_or(0.0) as f32,
        _ => 0.0,
    }
}

fn value_range(mode: ColorMode, r: &Ready) -> (f32, f32) {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for m in r.sliced.out.moves.iter().filter(|m| m.kind == Kind::Extrude) {
        let v = value(mode, m, r);
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if !lo.is_finite() {
        (0.0, 1.0)
    } else {
        (lo, hi)
    }
}

fn build_colors(r: &Ready, mode: ColorMode, hidden: &HashSet<Role>, fil: [u8; 3], gen: u64) -> Colors {
    let moves = &r.sliced.out.moves;
    let (lo, hi) = value_range(mode, r);
    let span = (hi - lo).max(1e-6);
    let mut bead = Vec::with_capacity(r.beads.point_move.len() * 16);
    for &mi in &r.beads.point_move {
        let m = &moves[mi as usize];
        let c = match mode {
            ColorMode::Realistic => fil,
            ColorMode::Feature => m.role.color(),
            _ => ramp_u8((value(mode, m, r) - lo) / span),
        };
        let a = if hidden.contains(&m.role) { 0 } else { 255 };
        for _ in 0..4 {
            bead.extend_from_slice(&[c[0], c[1], c[2], a]);
        }
    }
    let mut travel = Vec::with_capacity(r.beads.travel_move.len() * 4);
    for &mi in &r.beads.travel_move {
        let c = if moves[mi as usize].kind == Kind::Wipe { [250, 220, 60, 255] } else { [56, 72, 200, 200] };
        travel.extend_from_slice(&c);
    }
    Colors { gen, bead, travel }
}

/// Plate index for the shader.
fn plate_index(p: Plate) -> i32 {
    match p {
        Plate::TexturedPei => 0,
        Plate::SmoothPei => 1,
        Plate::CoolPlate => 2,
        Plate::EngineeringPlate => 3,
        Plate::Glass => 4,
        Plate::Buildtak => 5,
    }
}

fn push_line(v: &mut Vec<f32>, a: [f32; 3], b: [f32; 3], c: [f32; 4]) {
    v.extend_from_slice(&a);
    v.extend_from_slice(&c);
    v.extend_from_slice(&b);
    v.extend_from_slice(&c);
}

/// A labelled number in the settings panel; true when changed.
fn num(ui: &mut egui::Ui, label: &str, v: &mut f64, lo: f64, hi: f64, unit: &str, dec: usize) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.set_min_height(26.0);
        ui.label(egui::RichText::new(label).size(13.0).color(TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(14.0);
            ui.add_sized(vec2(40.0, 22.0), egui::Label::new(egui::RichText::new(unit).size(12.5).color(DIM_TEXT)));
            let speed = ((hi - lo) / 400.0).max(10f64.powi(-(dec as i32)));
            let r = ui.add_sized(vec2(78.0, 22.0), egui::DragValue::new(v).range(lo..=hi).speed(speed).max_decimals(dec).min_decimals(if dec > 0 { 1.min(dec) } else { 0 }));
            changed = r.changed();
        });
    });
    changed
}

fn int(ui: &mut egui::Ui, label: &str, v: &mut u32, lo: u32, hi: u32, unit: &str) -> bool {
    let mut f = *v as f64;
    let c = num(ui, label, &mut f, lo as f64, hi as f64, unit, 0);
    if c {
        *v = f.round().clamp(lo as f64, hi as f64) as u32;
    }
    c
}

fn pct(ui: &mut egui::Ui, label: &str, v: &mut f64, lo: f64, hi: f64) -> bool {
    let mut f = *v * 100.0;
    let c = num(ui, label, &mut f, lo * 100.0, hi * 100.0, "%", 0);
    if c {
        *v = (f / 100.0).clamp(lo, hi);
    }
    c
}

fn flag(ui: &mut egui::Ui, label: &str, v: &mut bool) -> bool {
    super::dlgkit::chk(ui, v, label, true)
}

fn pick<T: Copy + PartialEq>(ui: &mut egui::Ui, key: &str, label: &str, v: &mut T, all: &[T], name: fn(&T) -> &'static str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(13.0).color(TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(14.0);
            let names: Vec<&str> = all.iter().map(name).collect();
            let mut cur = all.iter().position(|x| x == v).unwrap_or(0);
            if super::dlgkit::combo(ui, key, 168.0, &mut cur, &names, true, None) {
                *v = all[cur];
                changed = true;
            }
        });
    });
    changed
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, Color32::from_rgb(230, 234, 236));
    ui.painter().text(pos2(r.left() + 6.0, r.center().y), Align2::LEFT_CENTER, title, FontId::proportional(13.0), LABEL);
    ui.add_space(2.0);
}

impl App {
    fn print_part_gen(&self) -> u64 {
        self.displayed().generation
    }

    /// Keeps the part mesh in step with the model.
    fn print_sync_part(&mut self) {
        let gen = self.print_part_gen();
        if self.print.part.as_ref().is_some_and(|p| p.gen == gen) {
            return;
        }
        let r = self.displayed();
        let tris = super::export::triangles(r);
        let mesh = Mesh::from_world_triangles(&tris);
        let t = &r.topo;
        let mut shade = Vec::with_capacity(tris.len());
        for (pi, p) in t.polys.iter().enumerate() {
            if p.len() < 3 {
                continue;
            }
            for k in 1..p.len() - 1 {
                let at = |j: usize| {
                    let pos = smesh::world_to_printer(t.positions[p[j]]);
                    let n = t.vnormals.get(pi).and_then(|v| v.get(j)).copied().unwrap_or(V3::Y);
                    (pos, smesh::world_to_printer(n))
                };
                shade.push([at(0), at(k), at(k + 1)]);
            }
        }
        self.print.part = Some(Part { gen, mesh: Arc::new(mesh), shade: Arc::new(shade) });
        self.print.scene = None;
    }

    fn print_key(&self) -> u64 {
        let s = serde_json::to_string(&self.print.settings).unwrap_or_default();
        let p = serde_json::to_string(&self.print.placement).unwrap_or_default();
        hash_of(&[&s, &p, &self.print_part_gen().to_string()])
    }

    /// The result on screen still matches the part and the settings.
    pub fn print_fresh(&self) -> bool {
        self.print.ready.is_some() && self.print.ready_key == self.print_key()
    }

    fn placed_mesh(&self) -> Option<Mesh> {
        let part = self.print.part.as_ref()?;
        Some(self.print.placement.apply(&part.mesh, self.print.settings.printer.bed_size()))
    }

    pub fn print_slice(&mut self) {
        if self.print.job.is_some() {
            return;
        }
        self.print_sync_part();
        let Some(mesh) = self.placed_mesh() else { return };
        if mesh.is_empty() {
            self.say("3D-Druck: das Modell hat noch keine Geometrie zum Drucken.", MsgKind::Warning);
            return;
        }
        let errs = self.print.settings.validate();
        if !errs.is_empty() {
            self.print.error = Some(errs.join("\n"));
            self.say(format!("3D-Druck: {}", errs[0]), MsgKind::Error);
            return;
        }
        let settings = self.print.settings.clone();
        let prog = Arc::new(Progress::default());
        let out: JobOut = Arc::new(Mutex::new(None));
        let (p2, o2) = (prog.clone(), out.clone());
        let key = self.print_key();
        std::thread::spawn(move || {
            let r = slicer::slice(mesh, &settings, &p2).map(|sliced| {
                let beads = Arc::new(build_beads(&sliced.out.moves));
                Ready { sliced, beads, settings }
            });
            if let Ok(mut g) = o2.lock() {
                *g = Some(r);
            }
        });
        self.print.job = Some(Job { prog, out, key });
        self.print.error = None;
    }

    /// Slices on this thread (scripted demos and screenshots).
    pub fn print_slice_blocking(&mut self) {
        self.print_slice();
        while let Some(j) = &self.print.job {
            if j.out.lock().map(|g| g.is_some()).unwrap_or(true) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // collected on the next print_tick; take it now so the first frame shows it
        let ctx = egui::Context::default();
        self.print_tick(&ctx);
    }

    pub fn print_cancel(&mut self) {
        if let Some(j) = &self.print.job {
            j.prog.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Every frame: camera swap when entering/leaving the tab, collecting a finished job.
    pub fn print_tick(&mut self, ctx: &egui::Context) {
        // choosing any tab leaves the slicer
        if self.print.open && self.tab != Tab::Print {
            self.print_cancel();
            self.print.open = false;
            self.print.lay_flat_pick = false;
        }
        let here = self.tab == Tab::Print;
        if here && !self.print.active {
            std::mem::swap(&mut self.camera, &mut self.print.camera);
            self.print.active = true;
            if !self.print.cam_init {
                self.camera = Camera::default();
                self.fit_pending = true;
                self.print.cam_init = true;
            }
        } else if !here && self.print.active {
            std::mem::swap(&mut self.camera, &mut self.print.camera);
            self.print.active = false;
            self.fit_pending = false;
        }
        let done = self.print.job.as_ref().and_then(|j| j.out.lock().ok().and_then(|mut g| g.take()));
        if let Some(res) = done {
            let key = self.print.job.take().map(|j| j.key).unwrap_or(0);
            match res {
                Ok(r) => {
                    let st = &r.sliced.out.stats;
                    let msg = format!(
                        "3D-Druck: {} Schichten, Druckzeit {}, {:.2} m Filament ({:.1} g).",
                        st.layers,
                        gcode::fmt_time(st.time),
                        st.filament_mm / 1000.0,
                        st.grams
                    );
                    for w in &r.sliced.out.warnings {
                        self.say(format!("3D-Druck: {}", w), MsgKind::Warning);
                    }
                    self.say(msg, MsgKind::Success);
                    self.print.layer_lo = 0;
                    self.print.layer_hi = usize::MAX;
                    self.print.cut = usize::MAX;
                    self.print.ready = Some(Arc::new(r));
                    self.print.ready_key = key;
                    self.print.preview = true;
                    self.print.colors = None;
                    self.print.scene = None;
                }
                Err(e) => {
                    self.say(format!("3D-Druck: {}", e.lines().next().unwrap_or("")), if e.contains("abgebrochen") { MsgKind::Info } else { MsgKind::Error });
                    self.print.error = Some(e);
                }
            }
        }
        if self.print.job.is_some() {
            ctx.request_repaint();
        }
    }

    fn print_set_printer(&mut self, p: Printer) {
        let s = &mut self.print.settings;
        let presets = printers::process_presets_for(&p);
        s.process = presets[self.print.quality.min(presets.len() - 1)].clone();
        s.filament.diameter = p.filament_diameter;
        s.printer = p;
        self.print.placement.center = None;
        self.fit_pending = self.tab == Tab::Print;
    }

    fn print_set_quality(&mut self, k: usize) {
        let presets = printers::process_presets_for(&self.print.settings.printer);
        let k = k.min(presets.len() - 1);
        let keep_support = self.print.settings.process.support;
        let keep_density = self.print.settings.process.infill_density;
        let keep_pattern = self.print.settings.process.infill_pattern;
        self.print.settings.process = presets[k].clone();
        self.print.settings.process.support = keep_support;
        self.print.settings.process.infill_density = keep_density;
        self.print.settings.process.infill_pattern = keep_pattern;
        self.print.quality = k;
    }

    fn print_lay_flat_auto(&mut self) {
        self.print_sync_part();
        let Some(part) = self.print.part.as_ref() else { return };
        let m = DMat3::from_cols_array(&self.print.placement.rot);
        let rotated = part.mesh.transformed(m);
        if let Some((n, _)) = rotated.flat_faces().first() {
            self.print.placement.rotate_by(smesh::lay_flat_rotation(*n));
            self.say("3D-Druck: das Teil liegt auf seiner größten ebenen Fläche.", MsgKind::Info);
        }
    }

    // ------------------------------------------------------------------ ribbon

    /// Werkzeuge ▸ Manufacture: starts the 3D-Druck application.
    pub fn manufacture_group(&mut self, ui: &mut egui::Ui) {
        let free = self.editor.is_none() && self.sketch.is_none() && !self.placing();
        group(ui, "Manufacture", |ui| {
            if large(ui, Icon::PrSlice, "Slicer", free, self.print.open)
                .on_hover_text("3D-Druck: das Teil für den Drucker slicen, Vorschau und G-Code")
                .clicked()
            {
                self.print_open();
            }
            if large(ui, Icon::Csys, "CNC", free, self.cnc.open)
                .on_hover_text("CNC-Fräsen: Werkzeugbahnen, Abtragssimulation und Kollisionsprüfung, NC-Code")
                .clicked()
            {
                self.cnc_open();
            }
            if large(ui, Icon::PcbGroup, "Leiterplatte", free, self.pcb.open)
                .on_hover_text("Leiterplatten-Entwurf: Schaltplan, Platine, DRC, Gerber und 3D-Modell (KiCad-kompatibel)")
                .clicked()
            {
                self.pcb_open();
            }
        });
    }

    pub fn print_open(&mut self) {
        self.print.open = true;
        self.tab = Tab::Print;
    }

    pub fn print_close(&mut self) {
        self.print_cancel();
        self.print.open = false;
        self.print.lay_flat_pick = false;
        self.tab = Tab::Tools;
    }

    pub fn print_ribbon(&mut self, ui: &mut egui::Ui) {
        let busy = self.print.busy();
        let has_part = !self.displayed().topo.polys.is_empty();
        // ---- Drucker
        let pname = self.print.settings.printer.name();
        let fname = self.print.settings.filament.name.clone();
        let qname = self.print.settings.process.name.clone();
        let (pr, fr, qr) = group(ui, "Drucker", |ui| {
            if large(ui, Icon::PrPrinter, "Drucker", !busy, self.print.printer_dlg.is_some()).on_hover_text("Drucker wählen, Druckbett und G-Code einstellen").clicked() {
                let sel = None;
                self.print.printer_dlg = Some(PrinterDlg { draft: self.print.settings.printer.clone(), filter: String::new(), sel });
            }
            stack(ui, |ui| {
                let a = small(ui, Icon::PrPrinter, &short(&pname, 26), !busy, false).on_hover_text("Drucker");
                let b = small(ui, Icon::PrFilament, &short(&fname, 26), !busy, false).on_hover_text("Filament");
                let c = small(ui, Icon::PrProcess, &short(&qname, 26), !busy, false).on_hover_text("Qualität (Druckprofil)");
                (a, b, c)
            })
        });
        let mut set_printer = None;
        drop_menu(ui, "pr_printer", &pr, |ui| {
            ui.set_min_width(320.0);
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                let all: Vec<Printer> = self.print.user_printers.iter().cloned().chain(printers::all_printers()).collect();
                let mut vendor = String::new();
                for p in all {
                    if p.vendor != vendor {
                        vendor = p.vendor.clone();
                        ui.label(egui::RichText::new(if vendor.is_empty() { "Eigene Drucker" } else { &vendor }).size(12.0).color(DIM_TEXT));
                    }
                    let sel = p.name() == pname;
                    if menu_row(ui, Some(Icon::PrPrinter), &format!("{}   {:.0}×{:.0}×{:.0}", p.name(), p.bed_x, p.bed_y, p.height), "", true, sel).clicked() {
                        set_printer = Some(p);
                    }
                }
            });
        });
        if let Some(p) = set_printer {
            self.print_set_printer(p);
        }
        let mut set_fil = None;
        drop_menu(ui, "pr_fil", &fr, |ui| {
            ui.set_min_width(300.0);
            for f in printers::filament_presets() {
                let sel = f.name == fname;
                let r = menu_row(ui, Some(Icon::PrFilament), &format!("{}   {:.0} °C / {:.0} °C", f.name, f.nozzle_temp, f.bed_temp), "", true, sel);
                if r.clicked() {
                    set_fil = Some(f);
                }
            }
        });
        if let Some(mut f) = set_fil {
            // keep the colour the user picked
            f.color = self.print.settings.filament.color;
            f.diameter = self.print.settings.printer.filament_diameter;
            self.print.settings.filament = f;
        }
        let mut set_q = None;
        drop_menu(ui, "pr_q", &qr, |ui| {
            ui.set_min_width(260.0);
            for (k, q) in printers::process_presets_for(&self.print.settings.printer).iter().enumerate() {
                if menu_row(ui, Some(Icon::PrProcess), &q.name, "", true, k == self.print.quality).clicked() {
                    set_q = Some(k);
                }
            }
        });
        if let Some(k) = set_q {
            self.print_set_quality(k);
        }

        // ---- Druckbett
        group(ui, "Druckbett", |ui| {
            let can = has_part && !busy && !self.print.preview;
            if large(ui, Icon::PrLayFlat, "Auf Fläche\nlegen", can, self.print.lay_flat_pick).on_hover_text("Eine Fläche des Teils anklicken: sie kommt nach unten auf das Druckbett").clicked() {
                self.print.lay_flat_pick = !self.print.lay_flat_pick;
            }
            stack(ui, |ui| {
                if small(ui, Icon::PrLayFlat, "Automatisch ausrichten", can, false).on_hover_text("Die größte ebene Fläche nach unten").clicked() {
                    self.print_lay_flat_auto();
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let q = std::f64::consts::FRAC_PI_2;
                    if tiny(ui, Icon::PrRotate, "90° um X drehen", can, false, false).clicked() {
                        self.print.placement.rotate_by(DMat3::from_rotation_x(q));
                    }
                    if tiny(ui, Icon::PrRotate, "90° um Y drehen", can, false, false).clicked() {
                        self.print.placement.rotate_by(DMat3::from_rotation_y(q));
                    }
                    if tiny(ui, Icon::PrRotate, "90° um Z drehen (senkrecht)", can, false, false).clicked() {
                        self.print.placement.turn = (self.print.placement.turn + 90.0) % 360.0;
                    }
                    if tiny(ui, Icon::PrCenter, "Auf dem Druckbett zentrieren", can, false, false).clicked() {
                        self.print.placement.center = None;
                    }
                });
                if small(ui, Icon::Undo, "Ausrichtung zurücksetzen", can, false).clicked() {
                    let copies = self.print.placement.copies;
                    self.print.placement = Placement { copies, ..Default::default() };
                }
            });
            stack(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    icons::paint(ui.painter(), Rect::from_center_size(ui.cursor().min + vec2(9.0, 13.0), vec2(18.0, 18.0)), Icon::PrArrange, can);
                    ui.add_space(22.0);
                    ui.label(egui::RichText::new("Anzahl").size(12.5).color(if can { TEXT } else { DIM_TEXT }));
                    let mut n = self.print.placement.copies as f64;
                    if ui.add_enabled(can, egui::DragValue::new(&mut n).range(1.0..=64.0).speed(0.05).max_decimals(0)).changed() {
                        self.print.placement.copies = n.round() as u32;
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    icons::paint(ui.painter(), Rect::from_center_size(ui.cursor().min + vec2(9.0, 13.0), vec2(18.0, 18.0)), Icon::PrScale, can);
                    ui.add_space(22.0);
                    ui.label(egui::RichText::new("Maßstab").size(12.5).color(if can { TEXT } else { DIM_TEXT }));
                    let mut s = self.print.placement.scale * 100.0;
                    if ui.add_enabled(can, egui::DragValue::new(&mut s).range(1.0..=1000.0).speed(0.5).suffix(" %")).changed() {
                        self.print.placement.scale = s / 100.0;
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    icons::paint(ui.painter(), Rect::from_center_size(ui.cursor().min + vec2(9.0, 13.0), vec2(18.0, 18.0)), Icon::PrRotate, can);
                    ui.add_space(22.0);
                    ui.label(egui::RichText::new("Drehung").size(12.5).color(if can { TEXT } else { DIM_TEXT }));
                    let mut t = self.print.placement.turn;
                    if ui.add_enabled(can, egui::DragValue::new(&mut t).range(-360.0..=360.0).speed(0.5).suffix(" °")).changed() {
                        self.print.placement.turn = t;
                    }
                });
            });
        });

        // ---- Stützen
        group(ui, "Stützen", |ui| {
            let mut on = self.print.settings.process.support;
            if large(ui, Icon::PrSupport, "Stützen", !busy, on).on_hover_text("Stützen unter Überhängen erzeugen").clicked() {
                on = !on;
                self.print.settings.process.support = on;
            }
            stack(ui, |ui| {
                let mut bp = self.print.settings.process.support_build_plate_only;
                if super::dlgkit::chk(ui, &mut bp, "Nur auf dem Druckbett", !busy && on) {
                    self.print.settings.process.support_build_plate_only = bp;
                }
                let brim_on = !matches!(self.print.settings.process.brim, Brim::None);
                let mut b = brim_on;
                if super::dlgkit::chk(ui, &mut b, "Rand (Brim)", !busy) {
                    self.print.settings.process.brim = if b { Brim::Outer } else { Brim::None };
                }
                let mut sk = self.print.settings.process.skirt_loops > 0;
                if super::dlgkit::chk(ui, &mut sk, "Schürze", !busy) {
                    self.print.settings.process.skirt_loops = if sk { 1 } else { 0 };
                }
            });
        });

        // ---- Slicen
        group(ui, "Slicen", |ui| {
            let fresh = self.print_fresh();
            if busy {
                if large(ui, Icon::Close, "Abbrechen", true, false).clicked() {
                    self.print_cancel();
                }
            } else if large(ui, Icon::PrSlice, if fresh { "Erneut\nslicen" } else { "Slicen" }, has_part, false).on_hover_text("Das Teil in Schichten schneiden und die Druckbahnen berechnen").clicked() {
                self.print_slice();
            }
        });

        // ---- Vorschau
        let has_result = self.print.ready.is_some();
        let mode_btn = group(ui, "Vorschau", |ui| {
            if large(ui, Icon::Extrude, "Vorbereiten", true, !self.print.preview).on_hover_text("Teil auf dem Druckbett").clicked() {
                self.print.preview = false;
            }
            if large(ui, Icon::PrPreview, "Vorschau", has_result, self.print.preview).on_hover_text("Gedrucktes Teil Schicht für Schicht").clicked() {
                self.print.preview = true;
            }
            stack(ui, |ui| {
                let m = small(ui, Icon::Appearance, &format!("Farbe: {}", self.print.mode.name()), has_result, false);
                if small(ui, Icon::PrTravel, "Leerfahrten", has_result, self.print.show_travel).clicked() {
                    self.print.show_travel = !self.print.show_travel;
                }
                if small(ui, Icon::Transparent, "Modell zeigen", has_result, self.print.show_ghost).clicked() {
                    self.print.show_ghost = !self.print.show_ghost;
                    self.print.scene = None;
                }
                m
            })
        });
        drop_menu(ui, "pr_mode", &mode_btn, |ui| {
            ui.set_min_width(220.0);
            for m in ColorMode::ALL {
                if menu_row(ui, None, m.name(), "", true, m == self.print.mode).clicked() {
                    self.print.mode = m;
                }
            }
        });

        // ---- Ausgabe
        group(ui, "Ausgabe", |ui| {
            let fresh = self.print_fresh();
            if large(ui, Icon::PrGcode, "G-Code\nexportieren", fresh && !busy, false).on_hover_text("Die G-Code-Datei für den Drucker speichern").clicked() {
                self.print_export();
            }
            stack(ui, |ui| {
                if small(ui, Icon::PrStats, "Statistik", has_result, self.print.stats_open).clicked() {
                    self.print.stats_open = !self.print.stats_open;
                }
            });
        });

        // ---- Schließen
        group(ui, "Schließen", |ui| {
            if large(ui, Icon::Close, "Slicer
schließen", true, false).on_hover_text("Zurück zu Werkzeuge").clicked() {
                self.print_close();
            }
        });
    }

    pub fn print_export(&mut self) {
        let Some(r) = self.print.ready.clone() else { return };
        if !self.print_fresh() {
            self.say("3D-Druck: Modell oder Einstellungen wurden geändert. Bitte erst neu slicen.", MsgKind::Warning);
            return;
        }
        let s = &r.settings;
        let name = format!("{}_{}_{}.gcode", self.data.name, s.filament.material.code().replace(' ', ""), s.printer.model.replace(' ', ""));
        let Some(path) = rfd::FileDialog::new().set_title("G-Code exportieren").set_directory(self.work_dir()).set_file_name(&name).add_filter("G-Code", &["gcode"]).save_file() else { return };
        match std::fs::write(&path, r.sliced.out.gcode.as_bytes()) {
            Ok(()) => self.say(format!("G-Code gespeichert: {}", path.display()), MsgKind::Success),
            Err(e) => self.say(format!("G-Code konnte nicht gespeichert werden: {}", e), MsgKind::Error),
        }
    }

    // ------------------------------------------------------------------ settings panel

    pub fn print_panel(&mut self, ui: &mut egui::Ui) {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
        ui.painter().rect_filled(r.expand2(vec2(4.0, 0.0)), 0.0, TITLE_BG);
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(20.0, 20.0)), Icon::PrPrinter, true);
        ui.painter().text(pos2(r.left() + 34.0, r.center().y), Align2::LEFT_CENTER, "Druckeinstellungen", FontId::proportional(14.0), TEXT);
        let busy = self.print.busy();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_enabled_ui(!busy, |ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 3.0);
                self.print_panel_body(ui);
            });
        });
    }

    fn print_panel_body(&mut self, ui: &mut egui::Ui) {
        // printer
        section(ui, "Drucker");
        let all: Vec<Printer> = self.print.user_printers.iter().cloned().chain(printers::all_printers()).collect();
        let names: Vec<String> = all.iter().map(|p| p.name()).collect();
        let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let cur_name = self.print.settings.printer.name();
        let mut cur = names.iter().position(|n| *n == cur_name).unwrap_or(usize::MAX);
        let mut shown = cur;
        let extra = format!("{} (geändert)", cur_name);
        let mut list = refs.clone();
        if cur == usize::MAX {
            list.insert(0, &extra);
            shown = 0;
        }
        ui.horizontal(|ui| {
            let before = shown;
            if super::dlgkit::combo(ui, "pr_printer_combo", ui.available_width() - 44.0, &mut shown, &list, true, Some(Icon::PrPrinter)) && shown != before {
                let k = if cur == usize::MAX { shown.wrapping_sub(1) } else { shown };
                if let Some(p) = all.get(k) {
                    cur = k;
                    let _ = cur;
                    self.print_set_printer(p.clone());
                }
            }
            if super::dlgkit::ibox(ui, Icon::Options, true, "Drucker bearbeiten …").clicked() {
                self.print.printer_dlg = Some(PrinterDlg { draft: self.print.settings.printer.clone(), filter: String::new(), sel: None });
            }
        });
        {
            let p = &mut self.print.settings.printer;
            let mut plate = p.plate;
            if pick(ui, "pr_plate", "Druckbett", &mut plate, &Plate::ALL, |x| x.name()) {
                p.plate = plate;
            }
            ui.label(egui::RichText::new(format!("Bauraum {:.0} × {:.0} × {:.0} mm · Düse {:.2} mm · {}", p.bed_x, p.bed_y, p.height, p.nozzle, p.flavor.name())).size(12.0).color(DIM_TEXT));
        }
        // filament
        section(ui, "Filament");
        let fils = printers::filament_presets();
        let fnames: Vec<&str> = fils.iter().map(|f| f.name.as_str()).collect();
        let mut fi = fils.iter().position(|f| f.name == self.print.settings.filament.name).unwrap_or(0);
        if super::dlgkit::combo(ui, "pr_fil_combo", ui.available_width(), &mut fi, &fnames, true, Some(Icon::PrFilament)) {
            let mut f = fils[fi].clone();
            f.color = self.print.settings.filament.color;
            f.diameter = self.print.settings.printer.filament_diameter;
            self.print.settings.filament = f;
        }
        // colour swatches
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
            for (name, c) in printers::COLORS {
                let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                let sel = self.print.settings.filament.color == *c;
                ui.painter().rect_filled(r.shrink(1.0), 3.0, Color32::from_rgb(c[0], c[1], c[2]));
                ui.painter().rect_stroke(r.shrink(1.0), 3.0, Stroke::new(if sel { 2.0_f32 } else { 1.0 }, if sel { SEL_BORDER } else { BORDER }), StrokeKind::Inside);
                if resp.on_hover_text(*name).clicked() {
                    self.print.settings.filament.color = *c;
                    self.print.colors = None;
                    self.print.scene = None;
                }
            }
            let mut c = self.print.settings.filament.color;
            if ui.color_edit_button_srgb(&mut c).on_hover_text("Eigene Farbe").changed() {
                self.print.settings.filament.color = c;
                self.print.colors = None;
                self.print.scene = None;
            }
        });
        {
            let f = &mut self.print.settings.filament;
            num(ui, "Düse", &mut f.nozzle_temp, 150.0, 450.0, "°C", 0);
            num(ui, "Düse erste Schicht", &mut f.nozzle_temp_first, 150.0, 450.0, "°C", 0);
            num(ui, "Druckbett", &mut f.bed_temp, 0.0, 150.0, "°C", 0);
            num(ui, "Druckbett erste Schicht", &mut f.bed_temp_first, 0.0, 150.0, "°C", 0);
        }
        // quality preset
        section(ui, "Druckprofil");
        let presets = printers::process_presets_for(&self.print.settings.printer);
        let qn: Vec<&str> = presets.iter().map(|q| q.name.as_str()).collect();
        let mut q = self.print.quality.min(qn.len() - 1);
        if super::dlgkit::combo(ui, "pr_q_combo", ui.available_width(), &mut q, &qn, true, Some(Icon::PrProcess)) {
            self.print_set_quality(q);
        }
        ui.add_space(4.0);
        // tabs, wrapped in two rows
        let names = ["Qualität", "Festigkeit", "Geschwindigkeit", "Stützen", "Haftung", "Kühlung", "Weiteres"];
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for (k, t) in names.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(super::dlgkit::tw(ui, t, 13.0) + 18.0, 26.0), Sense::click());
                let sel = self.print.panel_tab == k;
                ui.painter().rect_filled(r, 0.0, if sel { Color32::from_rgb(250, 250, 250) } else if resp.hovered() { HOVER_BG } else { Color32::from_rgb(234, 237, 239) });
                ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0_f32, super::dlgkit::LINE), StrokeKind::Inside);
                if sel {
                    ui.painter().line_segment([r.left_bottom() + vec2(2.0, -1.0), r.right_bottom() + vec2(-2.0, -1.0)], Stroke::new(2.0_f32, ACCENT));
                }
                ui.painter().text(r.center(), Align2::CENTER_CENTER, *t, FontId::proportional(13.0), if sel { TEXT } else { LABEL });
                if resp.clicked() {
                    self.print.panel_tab = k;
                }
            }
        });
        ui.add_space(4.0);
        let n = self.print.settings.printer.nozzle;
        let q = &mut self.print.settings.process;
        match self.print.panel_tab {
            0 => {
                section(ui, "Schichthöhe");
                num(ui, "Schichthöhe", &mut q.layer_height, 0.04, n * 0.85, "mm", 2);
                num(ui, "Erste Schicht", &mut q.first_layer_height, 0.04, n * 0.9, "mm", 2);
                section(ui, "Linienbreite");
                for (l, v) in [
                    ("Standard", &mut q.line_width),
                    ("Erste Schicht", &mut q.first_layer_width),
                    ("Außenwand", &mut q.outer_wall_width),
                    ("Innenwand", &mut q.inner_wall_width),
                    ("Obere Fläche", &mut q.top_width),
                    ("Massive Füllung", &mut q.solid_width),
                    ("Füllung", &mut q.sparse_width),
                    ("Stützen", &mut q.support_width),
                ] {
                    num(ui, l, v, n * 0.6, n * 2.5, "mm", 2);
                }
                section(ui, "Naht");
                pick(ui, "pr_seam", "Nahtposition", &mut q.seam, &Seam::ALL, |x| x.name());
                pct(ui, "Nahtlücke (Düse)", &mut q.seam_gap, 0.0, 0.5);
                section(ui, "Genauigkeit");
                num(ui, "Elefantenfuß-Ausgleich", &mut q.elephant_foot, 0.0, 1.0, "mm", 2);
                num(ui, "Auflösung", &mut q.resolution, 0.001, 0.1, "mm", 3);
                section(ui, "Glätten");
                pick(ui, "pr_iron", "Glätten", &mut q.ironing, &Ironing::ALL, |x| x.name());
                pct(ui, "Fluss", &mut q.ironing_flow, 0.01, 0.5);
                num(ui, "Linienabstand", &mut q.ironing_spacing, 0.05, 1.0, "mm", 2);
                section(ui, "Fuzzy Skin");
                pick(ui, "pr_fuzzy", "Fuzzy Skin", &mut q.fuzzy, &Fuzzy::ALL, |x| x.name());
                num(ui, "Dicke", &mut q.fuzzy_thickness, 0.05, 2.0, "mm", 2);
                num(ui, "Punktabstand", &mut q.fuzzy_point_dist, 0.1, 5.0, "mm", 2);
                section(ui, "Überhänge");
                flag(ui, "Überhangwände erkennen (langsamer, mehr Lüfter)", &mut q.detect_overhang_wall);
            }
            1 => {
                section(ui, "Wände");
                int(ui, "Wandlinien", &mut q.wall_loops, 1, 20, "");
                pick(ui, "pr_order", "Reihenfolge", &mut q.wall_order, &WallOrder::ALL, |x| x.name());
                flag(ui, "Nur eine Wand auf oberen Flächen", &mut q.only_one_wall_top);
                section(ui, "Obere und untere Schichten");
                int(ui, "Obere Schichten", &mut q.top_layers, 0, 50, "");
                int(ui, "Untere Schichten", &mut q.bottom_layers, 0, 50, "");
                pick(ui, "pr_top", "Muster oben", &mut q.top_pattern, &InfillPattern::SOLID, |x| x.name());
                pick(ui, "pr_bot", "Muster unten", &mut q.bottom_pattern, &InfillPattern::SOLID, |x| x.name());
                pick(ui, "pr_solid", "Massive Füllung", &mut q.solid_pattern, &InfillPattern::SOLID, |x| x.name());
                section(ui, "Füllung");
                pct(ui, "Fülldichte", &mut q.infill_density, 0.0, 1.0);
                pick(ui, "pr_inf", "Füllmuster", &mut q.infill_pattern, &InfillPattern::SPARSE, |x| x.name());
                num(ui, "Füllwinkel", &mut q.infill_angle, -180.0, 180.0, "°", 0);
                pct(ui, "Überlappung mit Wänden", &mut q.infill_overlap, 0.0, 1.0);
                num(ui, "Kleinste Füllfläche", &mut q.min_sparse_area, 0.0, 500.0, "mm²", 0);
                flag(ui, "Fülllinien verbinden", &mut q.infill_combine_connect);
                section(ui, "Sondermodus");
                flag(ui, "Spiralvase (eine Wand, durchgehend steigend)", &mut q.spiral_vase);
            }
            2 => {
                section(ui, "Erste Schicht");
                num(ui, "Erste Schicht", &mut q.first_layer_speed, 1.0, 1000.0, "mm/s", 0);
                num(ui, "Füllung erste Schicht", &mut q.first_layer_infill_speed, 1.0, 1000.0, "mm/s", 0);
                num(ui, "Leerfahrt erste Schicht", &mut q.first_layer_travel_speed, 1.0, 2000.0, "mm/s", 0);
                int(ui, "Langsame Schichten", &mut q.slow_down_layers, 0, 20, "");
                section(ui, "Weitere Schichten");
                for (l, v) in [
                    ("Außenwand", &mut q.outer_wall_speed),
                    ("Innenwand", &mut q.inner_wall_speed),
                    ("Füllung", &mut q.sparse_speed),
                    ("Massive Füllung", &mut q.solid_speed),
                    ("Obere Fläche", &mut q.top_speed),
                    ("Brücken", &mut q.bridge_speed),
                    ("Überhänge", &mut q.overhang_speed),
                    ("Stützen", &mut q.support_speed),
                    ("Stützen-Schnittstelle", &mut q.interface_speed),
                    ("Glätten", &mut q.ironing_speed),
                    ("Schürze und Rand", &mut q.skirt_speed),
                    ("Leerfahrt", &mut q.travel_speed),
                ] {
                    num(ui, l, v, 1.0, 2000.0, "mm/s", 0);
                }
                section(ui, "Beschleunigung");
                for (l, v) in [
                    ("Standard", &mut q.default_accel),
                    ("Außenwand", &mut q.outer_wall_accel),
                    ("Obere Fläche", &mut q.top_accel),
                    ("Erste Schicht", &mut q.first_layer_accel),
                    ("Leerfahrt", &mut q.travel_accel),
                ] {
                    num(ui, l, v, 10.0, 100000.0, "mm/s²", 0);
                }
            }
            3 => {
                section(ui, "Stützen");
                flag(ui, "Stützen erzeugen", &mut q.support);
                flag(ui, "Nur auf dem Druckbett", &mut q.support_build_plate_only);
                num(ui, "Überhangwinkel", &mut q.support_threshold, 1.0, 89.0, "°", 0);
                num(ui, "Abstand oben (Z)", &mut q.support_top_z, 0.0, 2.0, "mm", 2);
                num(ui, "Abstand unten (Z)", &mut q.support_bottom_z, 0.0, 2.0, "mm", 2);
                num(ui, "Abstand zum Teil (XY)", &mut q.support_xy, 0.0, 5.0, "mm", 2);
                num(ui, "Linienabstand", &mut q.support_spacing, 0.5, 10.0, "mm", 1);
                num(ui, "Winkel", &mut q.support_angle, -90.0, 90.0, "°", 0);
                int(ui, "Schnittstellen-Schichten", &mut q.interface_layers, 0, 10, "");
                num(ui, "Schnittstellen-Abstand", &mut q.interface_spacing, 0.1, 5.0, "mm", 2);
                flag(ui, "Kleine Überhänge weglassen", &mut q.support_remove_small);
            }
            4 => {
                section(ui, "Schürze");
                int(ui, "Schürzen-Linien", &mut q.skirt_loops, 0, 20, "");
                num(ui, "Abstand", &mut q.skirt_distance, 0.0, 50.0, "mm", 1);
                int(ui, "Höhe (Schichten)", &mut q.skirt_height, 1, 100, "");
                section(ui, "Rand (Brim)");
                pick(ui, "pr_brim", "Rand", &mut q.brim, &Brim::ALL, |x| x.name());
                num(ui, "Breite", &mut q.brim_width, 0.0, 50.0, "mm", 1);
                num(ui, "Abstand zum Teil", &mut q.brim_gap, 0.0, 2.0, "mm", 2);
            }
            5 => {
                let f = &mut self.print.settings.filament;
                section(ui, "Bauteillüfter");
                pct(ui, "Mindestens", &mut f.fan_min, 0.0, 1.0);
                pct(ui, "Höchstens", &mut f.fan_max, 0.0, 1.0);
                pct(ui, "Überhänge und Brücken", &mut f.overhang_fan, 0.0, 1.0);
                int(ui, "Aus für die ersten Schichten", &mut f.fan_off_layers, 0, 20, "");
                flag(ui, "Lüfter immer an", &mut f.fan_always_on);
                num(ui, "Mehr Lüfter unter Schichtzeit", &mut f.fan_cooling_layer_time, 0.0, 600.0, "s", 0);
                section(ui, "Verlangsamen");
                flag(ui, "Für die Schichtkühlung verlangsamen", &mut f.slow_down_for_layer_cooling);
                num(ui, "Mindest-Schichtzeit", &mut f.slow_down_layer_time, 0.0, 120.0, "s", 0);
                num(ui, "Mindestgeschwindigkeit", &mut f.slow_down_min_speed, 1.0, 200.0, "mm/s", 0);
                section(ui, "Material");
                num(ui, "Max. Volumenstrom", &mut f.max_volumetric, 0.5, 100.0, "mm³/s", 1);
                pct(ui, "Flussrate", &mut f.flow_ratio, 0.5, 1.5);
                num(ui, "Dichte", &mut f.density, 0.5, 3.0, "g/cm³", 2);
                num(ui, "Preis", &mut f.cost, 0.0, 1000.0, "€/kg", 2);
                num(ui, "Durchmesser", &mut f.diameter, 1.0, 3.5, "mm", 2);
            }
            _ => {
                section(ui, "Brücken");
                pct(ui, "Brückenfluss", &mut q.bridge_flow, 0.5, 1.5);
                section(ui, "Rückzug");
                flag(ui, "Kein Rückzug innerhalb der Füllung", &mut q.reduce_infill_retraction);
                let p = &mut self.print.settings.printer;
                num(ui, "Rückzugslänge", &mut p.retract_length, 0.0, 15.0, "mm", 2);
                num(ui, "Rückzugsgeschwindigkeit", &mut p.retract_speed, 1.0, 200.0, "mm/s", 0);
                num(ui, "Z-Sprung", &mut p.z_hop, 0.0, 5.0, "mm", 2);
                num(ui, "Kleinste Fahrt mit Rückzug", &mut p.retract_min_travel, 0.0, 20.0, "mm", 1);
                flag(ui, "Rückzug beim Schichtwechsel", &mut p.retract_on_layer_change);
                flag(ui, "Abwischen beim Rückzug", &mut p.wipe);
                num(ui, "Abwischstrecke", &mut p.wipe_distance, 0.0, 10.0, "mm", 1);
            }
        }
        ui.add_space(10.0);
        let errs = self.print.settings.validate();
        for e in errs {
            ui.label(egui::RichText::new(e).size(12.5).color(Color32::from_rgb(200, 40, 30)));
        }
    }

    // ------------------------------------------------------------------ viewport

    fn print_scene(&mut self) -> (Arc<Scene>, usize) {
        let s = self.print.settings.clone();
        let p = &s.printer;
        let preview = self.print.preview && self.print.ready.is_some();
        let ready_id = self.print.ready.as_ref().map(|r| Arc::as_ptr(r) as usize).unwrap_or(0);
        let key = hash_of(&[
            &format!("{:?}{}{}{}{:?}", p.shape, p.bed_x, p.bed_y, p.height, s.filament.color),
            &serde_json::to_string(&self.print.placement).unwrap_or_default(),
            &self.print.part.as_ref().map(|x| x.gen).unwrap_or(0).to_string(),
            &format!("{}{}{}", ready_id, preview, self.print.show_ghost),
        ]);
        if let Some((k, sc, pf)) = &self.print.scene {
            if *k == key {
                return (sc.clone(), *pf);
            }
        }
        let gen = self.print.next_gen();
        let (bx, by) = p.bed_size();
        let mut sc = Scene { gen, ..Default::default() };
        // plate top
        let quad = [[0.0, 0.0], [bx, 0.0], [bx, by], [0.0, by]];
        for &k in &[0usize, 1, 2, 0, 2, 3] {
            let q = quad[k];
            let ww = w(DVec3::new(q[0], q[1], 0.0));
            sc.bed.extend_from_slice(&[ww[0], ww[1], ww[2], q[0] as f32, q[1] as f32]);
        }
        // the plate's slab under it and a dark frame (printer bed carrier)
        let mut tri = |a: DVec3, b: DVec3, c: DVec3, col: [f32; 3]| {
            let n = (b - a).cross(c - a).normalize_or_zero();
            let nw = smesh::printer_to_world(n);
            for v in [a, b, c] {
                let p = w(v);
                sc.solid.extend_from_slice(&[p[0], p[1], p[2], nw.x as f32, nw.y as f32, nw.z as f32, col[0], col[1], col[2]]);
            }
        };
        let slab = |tri: &mut dyn FnMut(DVec3, DVec3, DVec3, [f32; 3]), x0: f64, y0: f64, x1: f64, y1: f64, z0: f64, z1: f64, col: [f32; 3]| {
            let c = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
            let faces = [
                [c(x0, y0, z0), c(x1, y0, z0), c(x1, y0, z1), c(x0, y0, z1)],
                [c(x1, y0, z0), c(x1, y1, z0), c(x1, y1, z1), c(x1, y0, z1)],
                [c(x1, y1, z0), c(x0, y1, z0), c(x0, y1, z1), c(x1, y1, z1)],
                [c(x0, y1, z0), c(x0, y0, z0), c(x0, y0, z1), c(x0, y1, z1)],
                [c(x0, y1, z0), c(x1, y1, z0), c(x1, y0, z0), c(x0, y0, z0)],
            ];
            for f in faces {
                tri(f[0], f[1], f[2], col);
                tri(f[0], f[2], f[3], col);
            }
        };
        let plate_col = match p.plate {
            Plate::TexturedPei | Plate::SmoothPei => [0.20, 0.20, 0.21],
            Plate::Glass => [0.55, 0.62, 0.64],
            _ => [0.16, 0.16, 0.17],
        };
        if matches!(p.shape, BedShape::Rect) {
            slab(&mut tri, 0.0, 0.0, bx, by, -1.2, -0.01, plate_col);
            slab(&mut tri, -6.0, -6.0, bx + 6.0, by + 6.0, -9.0, -1.2, [0.13, 0.135, 0.14]);
        } else {
            let r = bx / 2.0;
            let segs = 72;
            for i in 0..segs {
                let a0 = std::f64::consts::TAU * i as f64 / segs as f64;
                let a1 = std::f64::consts::TAU * (i + 1) as f64 / segs as f64;
                for (rr, z0, z1, col) in [(r, -1.2, -0.01, plate_col), (r + 6.0, -9.0, -1.2, [0.13, 0.135, 0.14])] {
                    let p0 = DVec3::new(r + rr * a0.cos(), r + rr * a0.sin(), 0.0);
                    let p1 = DVec3::new(r + rr * a1.cos(), r + rr * a1.sin(), 0.0);
                    let zz = |p: DVec3, z: f64| DVec3::new(p.x, p.y, z);
                    tri(zz(p0, z0), zz(p1, z0), zz(p1, z1), col);
                    tri(zz(p0, z0), zz(p1, z1), zz(p0, z1), col);
                    tri(DVec3::new(r, r, z0), zz(p1, z0), zz(p0, z0), col);
                }
            }
        }
        let part_from = sc.solid.len() / 9;
        // the part: always when preparing, see-through on request in the preview
        if !preview || self.print.show_ghost {
            if let Some(part) = &self.print.part {
                let ts = self.print.placement.transforms(&part.mesh, (bx, by));
                let inside = self.placed_mesh().map(|m| {
                    let (lo, hi) = m.bbox();
                    hi.z <= p.height + 1e-6 && [[lo.x, lo.y], [hi.x, lo.y], [hi.x, hi.y], [lo.x, hi.y]].iter().all(|c| p.on_bed(c[0], c[1]))
                });
                let col = if inside == Some(false) { [0.92, 0.25, 0.2] } else { rgbf(s.filament.color) };
                for (m, t) in ts {
                    let nm = m.inverse().transpose();
                    for tr in part.shade.iter() {
                        for (pos, n) in tr {
                            let pp = w(m * *pos + t);
                            let nn = smesh::printer_to_world((nm * *n).normalize_or_zero());
                            sc.solid.extend_from_slice(&[pp[0], pp[1], pp[2], nn.x as f32, nn.y as f32, nn.z as f32, col[0], col[1], col[2]]);
                        }
                    }
                }
            }
        }
        if preview {
            if let Some(r) = &self.print.ready {
                // the bead buffers are big: shared, not copied
                sc.beads = r.beads.clone();
            }
        }
        let sc = Arc::new(sc);
        self.print.scene = Some((key, sc.clone(), part_from));
        (sc, part_from)
    }

    fn print_colors(&mut self) -> Arc<Colors> {
        let ready_id = self.print.ready.as_ref().map(|r| Arc::as_ptr(r) as usize).unwrap_or(0);
        let mut hidden: Vec<String> = self.print.hidden.iter().map(|r| format!("{:?}", r)).collect();
        hidden.sort();
        let key = hash_of(&[&format!("{}{:?}{:?}", ready_id, self.print.mode, self.print.settings.filament.color), &hidden.join(",")]);
        if let Some((k, c)) = &self.print.colors {
            if *k == key {
                return c.clone();
            }
        }
        let gen = self.print.next_gen();
        let c = match &self.print.ready {
            Some(r) => build_colors(r, self.print.mode, &self.print.hidden, self.print.settings.filament.color, gen),
            None => Colors { gen, bead: vec![], travel: vec![] },
        };
        let c = Arc::new(c);
        self.print.colors = Some((key, c.clone()));
        c
    }

    /// Visible ranges (indices, travel vertices) of the layer and move sliders.
    fn print_ranges(&self) -> ((u32, u32), (u32, u32), Option<[f32; 3]>) {
        let Some(r) = &self.print.ready else { return ((0, 0), (0, 0), None) };
        let out = &r.sliced.out;
        let nl = out.layer_start.len().saturating_sub(1);
        if nl == 0 {
            return ((0, 0), (0, 0), None);
        }
        let hi = self.print.layer_hi.min(nl - 1);
        let lo = self.print.layer_lo.min(hi);
        let start = out.layer_start[lo];
        let top_from = out.layer_start[hi];
        let top_to = out.layer_start[hi + 1];
        let end = if self.print.cut == usize::MAX { top_to } else { (top_from + self.print.cut).min(top_to) };
        let b = &r.beads;
        let (s, e) = (b.idx_before[start], b.idx_before[end]);
        let (ts, te) = (b.travel_before[start], b.travel_before[end]);
        let tool = end.checked_sub(1).and_then(|k| out.moves.get(k)).map(|m| m.b);
        ((s, e - s), (ts, te - ts), tool)
    }

    pub fn print_viewport(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        super::ui::no_hand(ui.ctx(), resp.id);
        self.viewport_size = (rect.width().max(1.0), rect.height().max(1.0));
        let size = self.viewport_size;
        self.print_sync_part();
        let (bx, by) = self.print.settings.printer.bed_size();
        let bh = self.print.settings.printer.height;
        let mut bb = crate::kernel::math::BBox::default();
        bb.add(V3::new(-10.0, -10.0, 10.0));
        bb.add(V3::new(bx + 10.0, bh * 0.5, -by - 10.0));
        if self.fit_pending {
            self.camera.fit(&bb, size.0 / size.1);
            self.fit_pending = false;
        }
        self.camera.depth = bb.diagonal() * 10.0 + 1000.0;
        // ---- navigation, as in the model window ----
        let (scroll, zoom, mods, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers, i.pointer.clone()));
        let hover = resp.hover_pos();
        let local = |p: Pos2| (p.x - rect.min.x, p.y - rect.min.y);
        let over_slider = hover.is_some_and(|h| h.x > rect.right() - 90.0 || h.y > rect.bottom() - 70.0) && self.print.preview;
        if let Some(hp) = hover {
            if zoom != 1.0 {
                self.camera.zoom_at(1.0 / zoom as f64, local(hp), size);
            }
            if (scroll.y != 0.0 || scroll.x != 0.0) && !over_slider {
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
        let orbit_btn = pointer.middle_down() || pointer.secondary_down() || (pointer.primary_down() && mods.alt);
        if resp.dragged() && orbit_btn && (delta.x != 0.0 || delta.y != 0.0) {
            if mods.shift {
                self.camera.pan(delta.x, delta.y, size.1);
            } else if mods.ctrl {
                self.camera.zoom_at((delta.y as f64 * 0.01).exp(), (size.0 / 2.0, size.1 / 2.0), size);
            } else {
                self.camera.orbit(delta.x, delta.y);
            }
        }
        // ---- the part: pick, drag over the bed, lay flat on a face ----
        let preview = self.print.preview && self.print.ready.is_some();
        let busy = self.print.busy();
        let hit = if !preview && !busy {
            hover.and_then(|hp| {
                let (o, d) = self.camera.ray(local(hp), size);
                let (o, d) = (smesh::world_to_printer(o), smesh::world_to_printer(d));
                let m = self.placed_mesh()?;
                m.raycast(o, d).map(|(ti, t)| (m.normal(ti), o + d * t))
            })
        } else {
            None
        };
        self.print.hover_part = hit.is_some();
        let bed_point = |cam: &Camera, hp: Pos2| -> Option<DVec3> {
            let (o, d) = cam.ray(local(hp), size);
            let (o, d) = (smesh::world_to_printer(o), smesh::world_to_printer(d));
            (d.z.abs() > 1e-9).then(|| o + d * (-o.z / d.z))
        };
        if self.print.lay_flat_pick {
            if resp.clicked() {
                if let Some((n, _)) = hit {
                    let un = DMat3::from_rotation_z(-self.print.placement.turn.to_radians()) * n;
                    self.print.placement.rotate_by(smesh::lay_flat_rotation(un));
                    self.print.lay_flat_pick = false;
                    self.say("3D-Druck: das Teil liegt jetzt auf der gewählten Fläche.", MsgKind::Info);
                }
            }
        } else if !preview && !busy {
            if resp.drag_started_by(egui::PointerButton::Primary) && !mods.alt {
                if let (Some(_), Some(hp)) = (hit, hover) {
                    if let Some(bp) = bed_point(&self.camera, hp) {
                        let c = self.print.placement.center.unwrap_or([bx / 2.0, by / 2.0]);
                        self.print.drag = Some([c[0] - bp.x, c[1] - bp.y]);
                    }
                }
            }
            if let (Some(off), Some(hp)) = (self.print.drag, pointer.interact_pos()) {
                if pointer.primary_down() {
                    if let Some(bp) = bed_point(&self.camera, hp) {
                        let c = [(bp.x + off[0]).clamp(0.0, bx), (bp.y + off[1]).clamp(0.0, by)];
                        self.print.placement.center = Some(c);
                    }
                } else {
                    self.print.drag = None;
                }
            }
        }
        // ---- keys: layers and moves ----
        if preview && hover.is_some() {
            let nl = self.print.ready.as_ref().map(|r| r.sliced.plan.layers.len()).unwrap_or(1);
            let (up, down, left, right) = ui.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight)));
            let hi = self.print.layer_hi.min(nl - 1);
            if up {
                self.print.layer_hi = (hi + 1).min(nl - 1);
                self.print.cut = usize::MAX;
            }
            if down {
                self.print.layer_hi = hi.saturating_sub(1).max(self.print.layer_lo);
                self.print.cut = usize::MAX;
            }
            if left || right {
                let n = self.print_top_moves();
                let c = self.print.cut.min(n);
                self.print.cut = if left { c.saturating_sub(1) } else { (c + 1).min(n) };
            }
        }
        // ---- GL ----
        let (scene, part_from) = self.print_scene();
        let colors = self.print_colors();
        let (beads, travel, tool) = if preview { self.print_ranges() } else { ((0, 0), (0, 0), None) };
        let mut lines = vec![];
        let p = &self.print.settings.printer;
        let lc = [0.45, 0.48, 0.52, 0.55];
        let outline = p.bed_outline();
        for i in 0..outline.len() {
            let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
            push_line(&mut lines, w(DVec3::new(a[0], a[1], bh)), w(DVec3::new(b[0], b[1], bh)), lc);
        }
        if matches!(p.shape, BedShape::Rect) {
            for c in &outline {
                push_line(&mut lines, w(DVec3::new(c[0], c[1], 0.0)), w(DVec3::new(c[0], c[1], bh)), lc);
            }
        }
        let frame = GlFrame {
            mvp: self.camera.proj(size.0 / size.1) * self.camera.view(),
            view: self.camera.view(),
            scene,
            colors,
            plate: plate_index(p.plate),
            bed: [bx as f32, by as f32],
            round: matches!(p.shape, BedShape::Round),
            part_alpha: if preview { 0.22 } else { 1.0 },
            part_from,
            beads,
            travel: if self.print.show_travel { travel } else { (0, 0) },
            lines,
        };
        let painter = ui.painter_at(rect);
        super::ui::graphics_background_bg(&painter, rect, self.creo.bg);
        if self.renderer.is_some() {
            let gpu = self.print.gpu.clone();
            let frame = Arc::new(frame);
            let cb = eframe::egui_glow::CallbackFn::new(move |_info, pp| {
                if let Ok(mut g) = gpu.lock() {
                    let r = g.get_or_insert_with(|| PlateRenderer::new(pp.gl()));
                    r.paint(pp.gl(), &frame);
                }
            });
            painter.add(egui::PaintCallback { rect, callback: Arc::new(cb) });
        }
        // ---- overlays ----
        let cam = self.camera;
        let to_screen = |p: [f32; 3]| cam.project(V3::new(p[0] as f64, p[1] as f64, p[2] as f64), size).map(|s| pos2(rect.min.x + s.0, rect.min.y + s.1));
        // bed name on the front edge
        if let Some(sp) = to_screen(w(DVec3::new(bx / 2.0, -4.0, 0.0))) {
            painter.text(sp + vec2(0.0, 6.0), Align2::CENTER_TOP, format!("{} · {}", p.name(), p.plate.name()), FontId::proportional(12.0), Color32::from_rgb(70, 76, 84));
        }
        if let Some(t) = tool {
            if let Some(sp) = to_screen(wf(t)) {
                // the nozzle above the last move
                let pts = vec![sp, sp + vec2(-7.0, -14.0), sp + vec2(7.0, -14.0)];
                painter.add(egui::Shape::convex_polygon(pts, Color32::from_rgb(214, 160, 50), Stroke::new(1.0_f32, Color32::from_rgb(110, 76, 20))));
                painter.rect_filled(Rect::from_min_size(sp + vec2(-9.0, -26.0), vec2(18.0, 12.0)), 2.0, Color32::from_rgb(90, 96, 104));
            }
        }
        if self.print.lay_flat_pick {
            painter.text(rect.center_top() + vec2(0.0, 14.0), Align2::CENTER_TOP, "Fläche anklicken, die auf dem Druckbett liegen soll", FontId::proportional(14.0), Color32::from_rgb(20, 90, 170));
            if self.print.hover_part {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        } else if self.print.hover_part && !preview {
            ui.ctx().set_cursor_icon(if self.print.drag.is_some() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
        }
        if !preview {
            self.print_part_info(&painter, rect);
        }
        if preview {
            self.print_sliders(ui, rect);
            self.print_legend(ui, &painter, rect);
        }
        if let Some(j) = &self.print.job {
            let k = j.prog.done.load(std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0;
            let stage = j.prog.stage.lock().map(|s| s.clone()).unwrap_or_default();
            let r = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 40.0), vec2(360.0, 34.0));
            painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(250, 251, 252, 236));
            painter.rect_stroke(r, 3.0, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
            let bar = Rect::from_min_size(r.min + vec2(10.0, 22.0), vec2(340.0, 5.0));
            painter.rect_filled(bar, 2.0, Color32::from_rgb(220, 224, 228));
            painter.rect_filled(Rect::from_min_size(bar.min, vec2(340.0 * k, 5.0)), 2.0, FILE_GREEN);
            painter.text(r.min + vec2(10.0, 4.0), Align2::LEFT_TOP, format!("Slicen … {} ({:.0} %)", stage, k * 100.0), FontId::proportional(12.5), TEXT);
        }
        if !self.print_fresh() && self.print.ready.is_some() && preview {
            painter.text(rect.center_top() + vec2(0.0, 12.0), Align2::CENTER_TOP, "Modell oder Einstellungen geändert – die Vorschau ist veraltet. Bitte neu slicen.", FontId::proportional(13.5), Color32::from_rgb(196, 90, 10));
        }
    }

    fn print_top_moves(&self) -> usize {
        let Some(r) = &self.print.ready else { return 0 };
        let out = &r.sliced.out;
        let nl = out.layer_start.len().saturating_sub(1);
        if nl == 0 {
            return 0;
        }
        let hi = self.print.layer_hi.min(nl - 1);
        out.layer_start[hi + 1] - out.layer_start[hi]
    }

    fn print_part_info(&self, painter: &egui::Painter, rect: Rect) {
        let Some(m) = self.placed_mesh() else { return };
        if m.is_empty() {
            painter.text(rect.center(), Align2::CENTER_CENTER, "Kein Modell zum Drucken – erst ein Teil modellieren oder öffnen.", FontId::proportional(15.0), Color32::from_rgb(90, 96, 104));
            return;
        }
        let (lo, hi) = m.bbox();
        let p = &self.print.settings.printer;
        let s = hi - lo;
        let fits = hi.z <= p.height + 1e-6 && [[lo.x, lo.y], [hi.x, lo.y], [hi.x, hi.y], [lo.x, hi.y]].iter().all(|c| p.on_bed(c[0], c[1]));
        let lines = [
            format!("Größe {:.1} × {:.1} × {:.1} mm", s.x, s.y, s.z),
            format!("Volumen {:.2} cm³", m.volume() / 1000.0),
            if fits { "Passt in den Bauraum".to_string() } else { "Passt NICHT in den Bauraum".to_string() },
        ];
        let r = Rect::from_min_size(rect.min + vec2(12.0, 52.0), vec2(230.0, 66.0));
        painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(250, 251, 252, 225));
        painter.rect_stroke(r, 3.0, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
        for (i, l) in lines.iter().enumerate() {
            let col = if i == 2 && !fits { Color32::from_rgb(200, 40, 30) } else { TEXT };
            painter.text(r.min + vec2(10.0, 8.0 + i as f32 * 18.0), Align2::LEFT_TOP, l, FontId::proportional(12.5), col);
        }
    }

    fn print_sliders(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let Some(r) = self.print.ready.clone() else { return };
        let plan = &r.sliced.plan;
        let nl = plan.layers.len();
        if nl == 0 {
            return;
        }
        if self.print.layer_hi >= nl {
            self.print.layer_hi = nl - 1;
        }
        self.print.layer_lo = self.print.layer_lo.min(self.print.layer_hi);
        // ---- layer slider (right) ----
        let x = rect.right() - 40.0;
        let (y0, y1) = (rect.top() + 70.0, rect.bottom() - 80.0);
        let track = Rect::from_min_max(pos2(x - 3.0, y0), pos2(x + 3.0, y1));
        let p = ui.painter_at(rect);
        let ly = |l: usize| y1 - (y1 - y0) * if nl > 1 { l as f32 / (nl - 1) as f32 } else { 1.0 };
        p.rect_filled(track, 3.0, Color32::from_rgb(214, 218, 222));
        p.rect_filled(Rect::from_min_max(pos2(x - 3.0, ly(self.print.layer_hi)), pos2(x + 3.0, ly(self.print.layer_lo))), 3.0, ACCENT);
        let id = ui.id().with("pr_layer_slider");
        let area = Rect::from_min_max(pos2(x - 40.0, y0 - 14.0), pos2(x + 14.0, y1 + 14.0));
        let sr = ui.interact(area, id, Sense::click_and_drag());
        let pick_layer = |py: f32| (((y1 - py) / (y1 - y0)).clamp(0.0, 1.0) * (nl - 1) as f32).round() as usize;
        if sr.drag_started() || sr.clicked() {
            if let Some(pp) = sr.interact_pointer_pos() {
                let dh = (pp.y - ly(self.print.layer_hi)).abs();
                let dl = (pp.y - ly(self.print.layer_lo)).abs();
                self.print.slider_drag = if dl < dh && self.print.layer_lo != self.print.layer_hi { 1 } else { 2 };
            }
        }
        if sr.dragged() || sr.clicked() {
            if let Some(pp) = sr.interact_pointer_pos() {
                let l = pick_layer(pp.y);
                if self.print.slider_drag == 1 {
                    self.print.layer_lo = l.min(self.print.layer_hi);
                } else {
                    self.print.layer_hi = l.max(self.print.layer_lo);
                    self.print.cut = usize::MAX;
                }
            }
        }
        if sr.drag_stopped() {
            self.print.slider_drag = 0;
        }
        if sr.hovered() {
            let s = ui.input(|i| i.raw_scroll_delta.y);
            if s != 0.0 {
                let hi = self.print.layer_hi as i64 + if s > 0.0 { 1 } else { -1 };
                self.print.layer_hi = (hi.clamp(self.print.layer_lo as i64, nl as i64 - 1)) as usize;
                self.print.cut = usize::MAX;
            }
        }
        for (l, top) in [(self.print.layer_hi, true), (self.print.layer_lo, false)] {
            let y = ly(l);
            let c = pos2(x, y);
            p.circle(c, 7.0, Color32::WHITE, Stroke::new(2.0_f32, ACCENT));
            let z = plan.layers[l].z;
            let label = format!("{}\n{:.2}", l + 1, z);
            let g = p.layout(label, FontId::proportional(11.5), TEXT, 60.0);
            let br = Rect::from_min_size(pos2(x - 14.0 - g.size().x - 8.0, y - g.size().y / 2.0 - 3.0), g.size() + vec2(8.0, 6.0));
            if top || self.print.layer_lo != self.print.layer_hi {
                p.rect_filled(br, 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 235));
                p.rect_stroke(br, 2.0, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
                p.galley(br.min + vec2(4.0, 3.0), g, TEXT);
            }
        }
        // ---- move slider (bottom), within the top layer ----
        let n = self.print_top_moves();
        if n == 0 {
            return;
        }
        let (x0, x1) = (rect.left() + 60.0, rect.right() - 120.0);
        let y = rect.bottom() - 30.0;
        p.rect_filled(Rect::from_min_max(pos2(x0, y - 3.0), pos2(x1, y + 3.0)), 3.0, Color32::from_rgb(214, 218, 222));
        let cut = self.print.cut.min(n);
        let cx = x0 + (x1 - x0) * cut as f32 / n as f32;
        p.rect_filled(Rect::from_min_max(pos2(x0, y - 3.0), pos2(cx, y + 3.0)), 3.0, ACCENT);
        let id = ui.id().with("pr_move_slider");
        let mr = ui.interact(Rect::from_min_max(pos2(x0 - 10.0, y - 14.0), pos2(x1 + 10.0, y + 14.0)), id, Sense::click_and_drag());
        if mr.dragged() || mr.clicked() {
            if let Some(pp) = mr.interact_pointer_pos() {
                let k = (((pp.x - x0) / (x1 - x0)).clamp(0.0, 1.0) * n as f32).round() as usize;
                self.print.cut = if k >= n { usize::MAX } else { k };
            }
        }
        p.circle(pos2(cx, y), 7.0, Color32::WHITE, Stroke::new(2.0_f32, ACCENT));
        // what the slider stands on
        let out = &r.sliced.out;
        let hi = self.print.layer_hi.min(nl - 1);
        let mi = out.layer_start[hi] + cut.saturating_sub(1).min(n - 1);
        if let Some(m) = out.moves.get(mi) {
            let what = match m.kind {
                Kind::Extrude => format!("{} · {:.0} mm/s · {:.2} mm", m.role.name(), m.speed, m.width),
                Kind::Travel => format!("Leerfahrt · {:.0} mm/s", m.speed),
                Kind::Wipe => "Abwischen".to_string(),
                Kind::Retract => "Rückzug".to_string(),
                Kind::Unretract => "Vorschub".to_string(),
            };
            let t = gcode::fmt_time(m.t as f64);
            let label = format!("{}  ·  {}", what, t);
            let g = p.layout_no_wrap(label, FontId::proportional(11.5), TEXT);
            let lx = (cx - g.size().x / 2.0).clamp(rect.left() + 8.0, rect.right() - g.size().x - 8.0);
            p.galley(pos2(lx, y - 12.0 - g.size().y), g, TEXT);
        }
    }

    fn print_legend(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, rect: Rect) {
        let Some(r) = self.print.ready.clone() else { return };
        let st = &r.sliced.out.stats;
        let mode = self.print.mode;
        let x = rect.left() + 12.0;
        let mut y = rect.top() + 52.0;
        let w = 350.0;
        let rows_feature: Vec<Role> = Role::ALL.iter().cloned().filter(|ro| st.role_time.contains_key(ro) || st.role_filament.contains_key(ro)).collect();
        let body_h = match mode {
            ColorMode::Feature | ColorMode::Realistic => rows_feature.len() as f32 * 20.0 + 26.0,
            _ => 70.0,
        };
        let total_h = body_h + 112.0;
        let bg = Rect::from_min_size(pos2(x, y), vec2(w, total_h));
        painter.rect_filled(bg, 3.0, Color32::from_rgba_unmultiplied(250, 251, 252, 232));
        painter.rect_stroke(bg, 3.0, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
        y += 8.0;
        painter.text(pos2(x + 10.0, y), Align2::LEFT_TOP, mode.name(), FontId::proportional(14.0), TEXT);
        y += 22.0;
        match mode {
            ColorMode::Feature | ColorMode::Realistic => {
                painter.text(pos2(x + w - 80.0, y), Align2::RIGHT_TOP, "Zeit", FontId::proportional(11.5), DIM_TEXT);
                painter.text(pos2(x + w - 10.0, y), Align2::RIGHT_TOP, "Filament", FontId::proportional(11.5), DIM_TEXT);
                y += 16.0;
                for ro in rows_feature {
                    let rr = Rect::from_min_size(pos2(x + 6.0, y - 2.0), vec2(w - 12.0, 20.0));
                    let resp = ui.interact(rr, ui.id().with(("pr_leg", ro)), Sense::click());
                    let hidden = self.print.hidden.contains(&ro);
                    if resp.hovered() {
                        painter.rect_filled(rr, 2.0, HOVER_BG);
                    }
                    if resp.on_hover_text("Anklicken: ein- oder ausblenden").clicked() {
                        if hidden {
                            self.print.hidden.remove(&ro);
                        } else {
                            self.print.hidden.insert(ro);
                        }
                    }
                    let c = if mode == ColorMode::Realistic { self.print.settings.filament.color } else { ro.color() };
                    let sw = Rect::from_min_size(pos2(x + 10.0, y + 2.0), vec2(12.0, 12.0));
                    painter.rect_filled(sw, 2.0, if hidden { Color32::from_rgb(220, 222, 224) } else { Color32::from_rgb(c[0], c[1], c[2]) });
                    painter.rect_stroke(sw, 2.0, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
                    let tc = if hidden { DIM_TEXT } else { TEXT };
                    painter.text(pos2(x + 30.0, y), Align2::LEFT_TOP, ro.name(), FontId::proportional(12.5), tc);
                    let t = st.role_time.get(&ro).copied().unwrap_or(0.0);
                    let pctv = if st.time > 0.0 { t / st.time * 100.0 } else { 0.0 };
                    painter.text(pos2(x + w - 80.0, y), Align2::RIGHT_TOP, format!("{}  {:.0} %", gcode::fmt_time(t), pctv), FontId::proportional(11.5), tc);
                    let g = st.role_filament.get(&ro).copied().unwrap_or(0.0) * r.settings.filament.area() / 1000.0 * r.settings.filament.density;
                    painter.text(pos2(x + w - 10.0, y), Align2::RIGHT_TOP, format!("{:.2} g", g), FontId::proportional(11.5), tc);
                    y += 20.0;
                }
                y += 6.0;
            }
            _ => {
                let (lo, hi) = value_range(mode, &r);
                let bar = Rect::from_min_size(pos2(x + 12.0, y + 6.0), vec2(w - 24.0, 14.0));
                let mut mesh = egui::Mesh::default();
                let steps = 24;
                for i in 0..=steps {
                    let t = i as f32 / steps as f32;
                    let c = ramp_u8(t);
                    let col = Color32::from_rgb(c[0], c[1], c[2]);
                    let xx = bar.left() + bar.width() * t;
                    mesh.colored_vertex(pos2(xx, bar.top()), col);
                    mesh.colored_vertex(pos2(xx, bar.bottom()), col);
                    if i > 0 {
                        let k = (i * 2) as u32;
                        mesh.add_triangle(k - 2, k - 1, k);
                        mesh.add_triangle(k - 1, k + 1, k);
                    }
                }
                painter.add(egui::Shape::mesh(mesh));
                let f = |v: f32| if (hi - lo) < 10.0 { format!("{:.2}", v) } else { format!("{:.0}", v) };
                painter.text(pos2(bar.left(), bar.bottom() + 4.0), Align2::LEFT_TOP, format!("{} {}", f(lo), mode.unit()), FontId::proportional(12.0), TEXT);
                painter.text(pos2(bar.right(), bar.bottom() + 4.0), Align2::RIGHT_TOP, format!("{} {}", f(hi), mode.unit()), FontId::proportional(12.0), TEXT);
                y += 60.0;
            }
        }
        painter.line_segment([pos2(x + 8.0, y), pos2(x + w - 8.0, y)], Stroke::new(1.0_f32, SEP));
        y += 6.0;
        for (k, v) in [
            ("Druckzeit", gcode::fmt_time(st.time)),
            ("Filament", format!("{:.2} m · {:.2} cm³", st.filament_mm / 1000.0, st.volume_cm3)),
            ("Gewicht · Kosten", format!("{:.1} g · {:.2} €", st.grams, st.cost)),
            ("Schichten", format!("{} · {:.2} mm hoch", st.layers, st.max_z)),
        ] {
            painter.text(pos2(x + 10.0, y), Align2::LEFT_TOP, k, FontId::proportional(12.5), LABEL);
            painter.text(pos2(x + 120.0, y), Align2::LEFT_TOP, v, FontId::proportional(12.5), TEXT);
            y += 18.0;
        }
    }

    // ------------------------------------------------------------------ windows

    pub fn print_windows(&mut self, ctx: &egui::Context) {
        if self.tab != Tab::Print {
            return;
        }
        self.print_printer_dialog(ctx);
        if self.print.stats_open {
            self.print_stats_window(ctx);
        }
    }

    fn print_stats_window(&mut self, ctx: &egui::Context) {
        let Some(r) = self.print.ready.clone() else {
            self.print.stats_open = false;
            return;
        };
        let st = &r.sliced.out.stats;
        let f = &r.settings.filament;
        let open = super::dlgkit::cwin(ctx, "pr_stats", "Druckstatistik", false, pos2(420.0, 200.0), 460.0, |ui| {
            super::dlgkit::pad(ui, 10.0, |ui| {
                egui::Grid::new("pr_stats_grid").striped(true).num_columns(4).spacing(vec2(18.0, 6.0)).show(ui, |ui| {
                    for h in ["Linientyp", "Zeit", "Anteil", "Filament"] {
                        ui.label(egui::RichText::new(h).size(12.5).color(DIM_TEXT));
                    }
                    ui.end_row();
                    for ro in Role::ALL {
                        let t = st.role_time.get(&ro).copied().unwrap_or(0.0);
                        let e = st.role_filament.get(&ro).copied().unwrap_or(0.0);
                        if t <= 0.0 && e <= 0.0 {
                            continue;
                        }
                        ui.label(ro.name());
                        ui.label(gcode::fmt_time(t));
                        ui.label(format!("{:.1} %", if st.time > 0.0 { t / st.time * 100.0 } else { 0.0 }));
                        ui.label(format!("{:.2} m · {:.2} g", e / 1000.0, e * f.area() / 1000.0 * f.density));
                        ui.end_row();
                    }
                    ui.label("Leerfahrten");
                    ui.label(gcode::fmt_time(st.travel_time));
                    ui.label(format!("{:.1} %", if st.time > 0.0 { st.travel_time / st.time * 100.0 } else { 0.0 }));
                    ui.label("");
                    ui.end_row();
                });
                super::dlgkit::hline(ui);
                for (k, v) in [
                    ("Druckzeit", gcode::fmt_time(st.time)),
                    ("Schichten", st.layers.to_string()),
                    ("Filament", format!("{:.2} m ({:.2} cm³)", st.filament_mm / 1000.0, st.volume_cm3)),
                    ("Gewicht", format!("{:.2} g", st.grams)),
                    ("Kosten", format!("{:.2} € ({:.2} €/kg)", st.cost, f.cost)),
                    ("Drucker", r.settings.printer.name()),
                    ("Filament", format!("{} ({})", f.name, f.material.code())),
                    ("Profil", r.settings.process.name.clone()),
                ] {
                    ui.horizontal(|ui| {
                        ui.add_sized(vec2(110.0, 18.0), egui::Label::new(egui::RichText::new(k).color(LABEL)));
                        ui.label(v);
                    });
                }
            });
        });
        if !open {
            self.print.stats_open = false;
        }
    }

    fn print_printer_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.print.printer_dlg.take() else { return };
        let mut keep = true;
        let mut apply = false;
        let mut save_own = false;
        let all: Vec<Printer> = self.print.user_printers.iter().cloned().chain(printers::all_printers()).collect();
        let open = super::dlgkit::cwin(ctx, "pr_printer_dlg", "Drucker", false, pos2(300.0, 140.0), 860.0, |ui| {
            super::dlgkit::pad(ui, 10.0, |ui| {
                ui.horizontal_top(|ui| {
                    // list
                    ui.vertical(|ui| {
                        ui.set_width(300.0);
                        super::dlgkit::lbl(ui, "Suchen");
                        super::dlgkit::field(ui, &mut d.filter, 300.0, true);
                        let (lr, _) = super::dlgkit::white_box(ui, 300.0, 430.0, Color32::WHITE);
                        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(lr.shrink(2.0)).layout(egui::Layout::top_down(egui::Align::Min)));
                        egui::ScrollArea::vertical().id_salt("pr_plist").show(&mut child, |ui| {
                            let q = d.filter.to_lowercase();
                            let mut vendor = None::<String>;
                            for (i, p) in all.iter().enumerate() {
                                let name = p.name();
                                if !q.is_empty() && !name.to_lowercase().contains(&q) {
                                    continue;
                                }
                                if vendor.as_deref() != Some(p.vendor.as_str()) {
                                    vendor = Some(p.vendor.clone());
                                    ui.label(egui::RichText::new(if p.vendor.is_empty() { "Eigene Drucker" } else { &p.vendor }).size(12.0).color(DIM_TEXT));
                                }
                                let sel = d.sel == Some(i) || (d.sel.is_none() && name == d.draft.name());
                                let (rr, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                                if sel {
                                    ui.painter().rect_filled(rr, 0.0, super::dlgkit::ROW_SEL);
                                } else if resp.hovered() {
                                    ui.painter().rect_filled(rr, 0.0, HOVER_BG);
                                }
                                ui.painter().text(pos2(rr.left() + 12.0, rr.center().y), Align2::LEFT_CENTER, &p.model, FontId::proportional(13.0), TEXT);
                                ui.painter().text(pos2(rr.right() - 8.0, rr.center().y), Align2::RIGHT_CENTER, format!("{:.0}×{:.0}×{:.0}", p.bed_x, p.bed_y, p.height), FontId::proportional(11.5), DIM_TEXT);
                                if resp.clicked() {
                                    d.sel = Some(i);
                                    d.draft = p.clone();
                                }
                            }
                        });
                        ui.add_space(6.0);
                        if super::dlgkit::btn(ui, "Neuer eigener Drucker", 300.0, 0) {
                            d.draft = printers::custom_printer(220.0, 220.0, 250.0, false);
                            d.sel = None;
                        }
                    });
                    ui.add_space(12.0);
                    // details
                    ui.vertical(|ui| {
                        ui.set_width(520.0);
                        let p = &mut d.draft;
                        ui.horizontal(|ui| {
                            super::dlgkit::lbl(ui, "Hersteller");
                            super::dlgkit::field(ui, &mut p.vendor, 150.0, true);
                            super::dlgkit::lbl(ui, "Modell");
                            super::dlgkit::field(ui, &mut p.model, 220.0, true);
                        });
                        egui::ScrollArea::vertical().id_salt("pr_pdetail").max_height(470.0).show(ui, |ui| {
                            ui.spacing_mut().item_spacing = vec2(6.0, 3.0);
                            section(ui, "Bauraum");
                            let mut round = matches!(p.shape, BedShape::Round);
                            if flag(ui, "Rundes Druckbett (Delta)", &mut round) {
                                p.shape = if round { BedShape::Round } else { BedShape::Rect };
                                p.origin_center = round;
                            }
                            num(ui, if round { "Durchmesser" } else { "Breite X" }, &mut p.bed_x, 10.0, 2000.0, "mm", 1);
                            if !round {
                                num(ui, "Tiefe Y", &mut p.bed_y, 10.0, 2000.0, "mm", 1);
                            } else {
                                p.bed_y = p.bed_x;
                            }
                            num(ui, "Höhe Z", &mut p.height, 10.0, 2000.0, "mm", 1);
                            flag(ui, "Nullpunkt in der Bettmitte", &mut p.origin_center);
                            pick(ui, "pr_dplate", "Druckplatte", &mut p.plate, &Plate::ALL, |x| x.name());
                            section(ui, "Extruder");
                            num(ui, "Düsendurchmesser", &mut p.nozzle, 0.1, 2.0, "mm", 2);
                            num(ui, "Filamentdurchmesser", &mut p.filament_diameter, 1.0, 3.5, "mm", 2);
                            flag(ui, "Direktantrieb", &mut p.direct_drive);
                            num(ui, "Rückzugslänge", &mut p.retract_length, 0.0, 15.0, "mm", 2);
                            num(ui, "Rückzugsgeschwindigkeit", &mut p.retract_speed, 1.0, 200.0, "mm/s", 0);
                            num(ui, "Z-Sprung", &mut p.z_hop, 0.0, 5.0, "mm", 2);
                            section(ui, "Bewegung");
                            pick(ui, "pr_flavor", "Firmware", &mut p.flavor, &Flavor::ALL, |x| x.name());
                            num(ui, "Max. Geschwindigkeit", &mut p.max_speed, 10.0, 2000.0, "mm/s", 0);
                            num(ui, "Max. Beschleunigung", &mut p.max_accel, 100.0, 100000.0, "mm/s²", 0);
                            num(ui, "Eckengeschwindigkeit", &mut p.corner_velocity, 0.5, 50.0, "mm/s", 1);
                            num(ui, "Max. Z-Geschwindigkeit", &mut p.max_z_speed, 1.0, 1000.0, "mm/s", 0);
                            flag(ui, "Maschinengrenzen in den G-Code schreiben", &mut p.emit_limits);
                            let mut th = p.thumbnail > 0;
                            if flag(ui, "Vorschaubild in den G-Code (300 × 300)", &mut th) {
                                p.thumbnail = if th { 300 } else { 0 };
                            }
                            section(ui, "Start-G-Code");
                            ui.add(egui::TextEdit::multiline(&mut p.start_gcode).code_editor().desired_rows(8).desired_width(500.0));
                            section(ui, "End-G-Code");
                            ui.add(egui::TextEdit::multiline(&mut p.end_gcode).code_editor().desired_rows(6).desired_width(500.0));
                            section(ui, "G-Code beim Schichtwechsel");
                            ui.add(egui::TextEdit::multiline(&mut p.layer_gcode).code_editor().desired_rows(3).desired_width(500.0));
                            ui.label(egui::RichText::new("Platzhalter wie in Orca: [bed_temperature_initial_layer_single], [nozzle_temperature_initial_layer], {if …}…{endif}, {max_layer_z} …").size(11.5).color(DIM_TEXT));
                        });
                    });
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if super::dlgkit::btn(ui, "Als eigenen Drucker speichern", 220.0, 0) {
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
            let mut p = d.draft.clone();
            if p.vendor.is_empty() && p.model.is_empty() {
                p.model = "Eigener Drucker".into();
            }
            self.print.user_printers.retain(|x| x.name() != p.name());
            self.print.user_printers.insert(0, p.clone());
            self.say(format!("Drucker „{}“ gespeichert.", p.name()), MsgKind::Success);
            d.sel = Some(0);
        }
        if apply {
            let errs = Settings { printer: d.draft.clone(), ..self.print.settings.clone() }.validate();
            let p = d.draft.clone();
            let changed_kind = p.nozzle != self.print.settings.printer.nozzle || p.name() != self.print.settings.printer.name();
            if changed_kind {
                self.print_set_printer(p);
            } else {
                self.print.settings.printer = p;
            }
            if !errs.is_empty() && !changed_kind {
                self.say(format!("3D-Druck: {}", errs[0]), MsgKind::Warning);
            }
            keep = false;
        }
        if keep && open {
            self.print.printer_dlg = Some(d);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beads_follow_moves() {
        let mut moves = vec![];
        let mk = |a: [f32; 3], b: [f32; 3], kind: Kind| gcode::Move { a, b, kind, role: Role::OuterWall, width: 0.4, height: 0.2, speed: 50.0, fan: 0.0, temp: 200.0, e: 0.1, layer: 0, t: 0.0 };
        moves.push(mk([0.0, 0.0, 0.2], [10.0, 0.0, 0.2], Kind::Extrude));
        moves.push(mk([10.0, 0.0, 0.2], [10.0, 10.0, 0.2], Kind::Extrude));
        moves.push(mk([10.0, 10.0, 0.2], [20.0, 20.0, 0.2], Kind::Travel));
        moves.push(mk([20.0, 20.0, 0.2], [30.0, 20.0, 0.2], Kind::Extrude));
        let b = build_beads(&moves);
        // two runs: 3 points and 2 points, 4 corners each
        assert_eq!(b.pos.len() / 3, (3 + 2) * 4);
        assert_eq!(b.point_move.len(), 5);
        // caps (2 × 6) + 4 quads (24) per segment
        assert_eq!(b.idx.len(), 2 * 12 + 3 * 24);
        assert_eq!(b.idx_before[0], 0);
        assert_eq!(b.idx_before[4] as usize, b.idx.len());
        assert!(b.idx_before[1] > b.idx_before[0] && b.idx_before[2] == b.idx_before[3]);
        assert_eq!(b.travel.len(), 6);
        assert!(b.idx.iter().all(|&i| (i as usize) < b.pos.len() / 3));
    }
}
