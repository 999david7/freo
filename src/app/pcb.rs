//! Leiterplatte (Werkzeuge ▸ Manufacture ▸ Leiterplatte): schematic and board editors in the
//! style of Creo's application ribbons — symbol library, wires, net labels, power symbols,
//! annotation and ERC; footprint placement, ratsnest, manual and automatic routing, vias,
//! copper zones, DRC; Gerber/Excellon, BOM, KiCad files and the board as a Freo assembly.

use super::dlgkit;
use super::icons::{self, Icon};
use super::ribbon::{drop_menu, group, large, menu_row, small, stack, theme::*, tiny};
use super::*;
use crate::pcb::board::{layer_name, layer_title, Board, FpInst, Rules, Track, Via, Zone, B_CU, F_CU};
use crate::pcb::geom::{p, pt_seg, Hull, P};
use crate::pcb::schematic::{ErcItem, Label, LabelKind, Schematic, Severity};
use crate::pcb::symbols::{self, Fill, Gfx, Symbol};
use crate::pcb::{autoroute, body3d, bom, drc, footprints, gerber, kicad, Project};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum PcbView {
    Schematic,
    Board,
}

#[derive(Clone, PartialEq, Debug)]
pub enum PTool {
    Select,
    Place(String),
    Wire,
    Label(LabelKind),
    Power(String),
    NoConnect,
    Route,
    Via,
    Zone,
    Sheet,
    SheetPin,
    Bus,
    BusEntry,
    DiffPair,
    Tune,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PSel {
    Sym(usize),
    Wire(usize),
    Label(usize),
    Nc(usize),
    Sheet(usize),
    Bus(usize),
    Entry(usize),
    Fp(usize),
    Track(usize),
    Via(usize),
    Zone(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct Cam2 {
    pub c: P,
    /// pixels per millimetre
    pub s: f32,
    pub init: bool,
}

impl Default for Cam2 {
    fn default() -> Self {
        Cam2 { c: P::ZERO, s: 4.0, init: false }
    }
}

impl Cam2 {
    pub fn scr(&self, r: Rect, q: P) -> Pos2 {
        r.center() + vec2(((q.x - self.c.x) as f32) * self.s, ((q.y - self.c.y) as f32) * self.s)
    }
    pub fn world(&self, r: Rect, s: Pos2) -> P {
        let d = s - r.center();
        self.c + p((d.x / self.s) as f64, (d.y / self.s) as f64)
    }
    pub fn fit(&mut self, r: Rect, lo: P, hi: P) {
        let w = (hi.x - lo.x).max(1.0) as f32;
        let h = (hi.y - lo.y).max(1.0) as f32;
        self.s = ((r.width() - 60.0) / w).min((r.height() - 60.0) / h).clamp(0.2, 400.0);
        self.c = (lo + hi) / 2.0;
        self.init = true;
    }
}

#[derive(Clone, Debug)]
pub struct Route {
    pub net: String,
    pub layer: u8,
    pub pts: Vec<P>,
    pub width: f64,
    /// differential pair: partner net and the current ends of (this, partner) tracks
    pub pair: Option<(String, P, P, f64)>,
}

#[derive(Clone, Debug)]
struct Drag {
    start: P,
    sel: PSel,
    orig: Project,
    /// wire ends that sat on the dragged symbol's pins: (wire, end b?)
    rubber: Vec<(usize, bool)>,
}

#[derive(Clone, Debug)]
pub struct Props {
    pub i: usize,
    pub reference: String,
    pub value: String,
    pub footprint: String,
}

#[derive(Clone, Debug)]
pub struct ZoneDlg {
    pub net: usize,
    pub layer: u8,
    pub clearance: String,
}

pub struct Vis {
    pub cu: [bool; 4],
    pub silk: bool,
    pub edge: bool,
    pub rats: bool,
    pub zones: bool,
    pub court: bool,
}

pub struct PcbUi {
    pub open: bool,
    pub view: PcbView,
    pub project: Project,
    pub dir: Option<std::path::PathBuf>,
    pub tool: PTool,
    pub sel: Option<PSel>,
    pub sch_cam: Cam2,
    pub brd_cam: Cam2,
    drag: Option<Drag>,
    pub wire: Vec<P>,
    pub route: Option<Route>,
    pub zone_pts: Vec<P>,
    pub zone_cfg: (String, u8, f64),
    pub zone_dlg: Option<ZoneDlg>,
    pub layer: u8,
    pub vis: Vis,
    pub erc: Option<Vec<ErcItem>>,
    pub drc: Option<Vec<drc::Violation>>,
    pub report: bool,
    pub lib_filter: String,
    pub lib_sel: Option<String>,
    pub place_rot: f64,
    pub place_mirror: bool,
    pub props: Option<Props>,
    pub label_dlg: Option<(P, String, LabelKind)>,
    pub rules_dlg: Option<Rules>,
    pub outline_dlg: Option<(String, String)>,
    undo: Vec<Project>,
    redo: Vec<Project>,
    pub hl_net: Option<String>,
    rats: Option<Vec<(P, P, String)>>,
    zone_mesh: Option<Vec<(u8, Vec<[P; 3]>)>>,
    pub mouse: P,
    pub diagonal_first: bool,
    pub fit: bool,
    pub power_kind: String,
    /// sub-sheet being edited (indices from the root)
    pub path: Vec<usize>,
    pub sheet_dlg: Option<(P, String)>,
    pub sheet_pin_dlg: Option<(usize, P, String)>,
    /// loaded KiCad libraries (.kicad_sym files, .pretty folders)
    pub libs: Vec<std::path::PathBuf>,
    pub fp_filter: String,
    pub sym_ed: Option<super::pcb_editors::SymEditor>,
    pub fp_ed: Option<super::pcb_editors::FpEditor>,
    pub router_mode: crate::pcb::route::Mode,
    pub tune_dlg: Option<(String, String)>,
    /// (view + 1, pass) of the ribbon's first appearance; 0 = closed
    pub ribbon_guard: (u8, Option<std::time::Instant>),
}

impl Default for PcbUi {
    fn default() -> Self {
        PcbUi {
            open: false,
            view: PcbView::Schematic,
            project: Project::new("PCB0001"),
            dir: None,
            tool: PTool::Select,
            sel: None,
            sch_cam: Cam2::default(),
            brd_cam: Cam2::default(),
            drag: None,
            wire: vec![],
            route: None,
            zone_pts: vec![],
            zone_cfg: ("GND".into(), B_CU, 0.3),
            zone_dlg: None,
            layer: F_CU,
            vis: Vis { cu: [true; 4], silk: true, edge: true, rats: true, zones: true, court: false },
            erc: None,
            drc: None,
            report: false,
            lib_filter: String::new(),
            lib_sel: None,
            place_rot: 0.0,
            place_mirror: false,
            props: None,
            label_dlg: None,
            rules_dlg: None,
            outline_dlg: None,
            undo: vec![],
            redo: vec![],
            hl_net: None,
            rats: None,
            zone_mesh: None,
            mouse: P::ZERO,
            diagonal_first: false,
            fit: true,
            power_kind: "GND".into(),
            path: vec![],
            sheet_dlg: None,
            sheet_pin_dlg: None,
            libs: vec![],
            fp_filter: String::new(),
            sym_ed: None,
            fp_ed: None,
            router_mode: crate::pcb::route::Mode::Walkaround,
            tune_dlg: None,
            ribbon_guard: (0, None),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    project: Project,
    view: PcbView,
    #[serde(default)]
    dir: Option<std::path::PathBuf>,
    #[serde(default)]
    libs: Vec<std::path::PathBuf>,
}

/// Loads a KiCad library: a .kicad_sym file or a .pretty folder. Returns (symbols, footprints).
pub fn load_library(path: &std::path::Path) -> Result<(usize, usize), String> {
    if path.is_dir() && !path.extension().is_some_and(|e| e == "pretty") {
        // a folder of 3D models (STEP)
        crate::pcb::models::add_model_dir(path.to_path_buf());
        return Ok((0, 0));
    }
    if path.is_dir() {
        let fps = kicad::read_pretty_dir(path)?;
        Ok((0, footprints::register(fps)))
    } else {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        let lib = path.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
        let syms = kicad::read_symbol_lib(&text, &lib)?;
        Ok((symbols::register(syms), 0))
    }
}

impl PcbUi {
    pub fn to_json(&self) -> String {
        serde_json::to_string(&Saved { project: self.project.clone(), view: self.view, dir: self.dir.clone(), libs: self.libs.clone() }).unwrap_or_default()
    }
    pub fn load_json(&mut self, s: &str) {
        if let Ok(v) = serde_json::from_str::<Saved>(s) {
            footprints::register(v.project.footprints.clone());
            self.project = v.project;
            self.view = v.view;
            self.dir = v.dir;
            for l in &v.libs {
                let _ = load_library(l);
            }
            self.libs = v.libs;
        }
    }
    pub fn sch(&self) -> &Schematic {
        &self.project.schematic
    }
    /// The sheet being edited (root or a sub-sheet along `path`).
    pub fn cur(&self) -> &Schematic {
        self.project.schematic.sheet_at(&self.path).unwrap_or(&self.project.schematic)
    }
    pub fn cur_mut(&mut self) -> &mut Schematic {
        if self.project.schematic.sheet_at(&self.path).is_none() {
            self.path.clear();
        }
        self.project.schematic.sheet_at_mut(&self.path).unwrap()
    }
    /// Sheet names from the root to the current sheet ("/Netzteil").
    pub fn sheet_path_name(&self) -> String {
        let mut s = &self.project.schematic;
        let mut out = String::new();
        for k in &self.path {
            let Some(sh) = s.sheets.get(*k) else { break };
            out.push('/');
            out.push_str(&sh.name);
            s = &sh.sch;
        }
        if out.is_empty() {
            "/".into()
        } else {
            out
        }
    }
    pub fn brd(&self) -> &Board {
        &self.project.board
    }
    /// Copper and drawings changed: ratsnest and zone meshes are recomputed; ERC/DRC markers
    /// of the old state are dropped (the report window offers to check again).
    pub fn changed(&mut self) {
        self.rats = None;
        self.zone_mesh = None;
        if self.erc.is_some() || self.drc.is_some() {
            self.erc = None;
            self.drc = None;
            self.report = false;
        }
    }
    pub fn ratsnest(&mut self) -> &Vec<(P, P, String)> {
        if self.rats.is_none() {
            self.rats = Some(self.project.board.ratsnest());
        }
        self.rats.as_ref().unwrap()
    }
    pub fn set_project(&mut self, pr: Project) {
        crate::pcb::models::set_project_dir(self.dir.clone());
        footprints::register(pr.footprints.clone());
        self.path.clear();
        self.project = pr;
        self.undo.clear();
        self.redo.clear();
        self.sel = None;
        self.erc = None;
        self.drc = None;
        self.changed();
        self.sch_cam.init = false;
        self.brd_cam.init = false;
        self.fit = true;
    }
}

// ------------------------------------------------------------------ colours

const SHEET: Color32 = Color32::from_rgb(255, 255, 255);
const FRAME: Color32 = Color32::from_rgb(120, 40, 40);
const SYM: Color32 = Color32::from_rgb(150, 30, 30);
const SYM_FILL: Color32 = Color32::from_rgb(255, 252, 214);
const WIRE: Color32 = Color32::from_rgb(10, 132, 40);
const INK: Color32 = Color32::from_rgb(40, 40, 46);
const NC: Color32 = Color32::from_rgb(40, 80, 220);
const BUS: Color32 = Color32::from_rgb(20, 60, 200);
const SEL_C: Color32 = super::sketcher::SEL;
const PRE_C: Color32 = super::sketcher::PRE;
const BOARD_FILL: Color32 = Color32::from_rgb(28, 64, 44);
const EDGE: Color32 = Color32::from_rgb(232, 204, 40);
const SILK: Color32 = Color32::from_rgb(242, 238, 200);
const RATS: Color32 = Color32::from_rgb(232, 236, 240);
const ERR: Color32 = Color32::from_rgb(232, 40, 32);
const WARN: Color32 = Color32::from_rgb(240, 160, 20);

pub fn layer_color(l: u8) -> Color32 {
    match l {
        0 => Color32::from_rgb(206, 58, 52),
        1 => Color32::from_rgb(70, 124, 210),
        2 => Color32::from_rgb(120, 196, 120),
        _ => Color32::from_rgb(206, 128, 46),
    }
}

fn fade(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0) as u8)
}

fn snap(q: P, g: f64) -> P {
    p((q.x / g).round() * g, (q.y / g).round() * g)
}

fn net_width(rules: &Rules, net: &str) -> f64 {
    rules.class_of(net).track
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    ui.painter().text(pos2(r.left() + 2.0, r.center().y), Align2::LEFT_CENTER, title, FontId::proportional(14.0), TEXT);
    ui.painter().line_segment([pos2(r.left(), r.bottom()), pos2(r.right(), r.bottom())], Stroke::new(1.0_f32, SEP));
    ui.add_space(3.0);
}

/// One clickable list row with an optional colour chip.
fn row(ui: &mut egui::Ui, text: &str, right: &str, selected: bool, chip: Option<Color32>) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    if selected {
        ui.painter().rect_filled(r, 0.0, dlgkit::ROW_SEL);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 0.0, HOVER_BG);
    }
    let mut x = r.left() + 6.0;
    if let Some(c) = chip {
        ui.painter().rect_filled(Rect::from_center_size(pos2(x + 6.0, r.center().y), vec2(12.0, 12.0)), 1.0, c);
        ui.painter().rect_stroke(Rect::from_center_size(pos2(x + 6.0, r.center().y), vec2(12.0, 12.0)), 1.0, Stroke::new(1.0_f32, Color32::from_rgb(90, 90, 90)), StrokeKind::Inside);
        x += 18.0;
    }
    ui.painter().with_clip_rect(r).text(pos2(x, r.center().y), Align2::LEFT_CENTER, text, FontId::proportional(13.0), TEXT);
    if !right.is_empty() {
        ui.painter().text(pos2(r.right() - 6.0, r.center().y), Align2::RIGHT_CENTER, right, FontId::proportional(12.0), Color32::from_rgb(110, 116, 124));
    }
    resp
}

// ------------------------------------------------------------------ drawing helpers

fn seg(p: &egui::Painter, a: Pos2, b: Pos2, w: f32, c: Color32) {
    p.line_segment([a, b], Stroke::new(w, c));
}

fn round_seg(p: &egui::Painter, a: Pos2, b: Pos2, w: f32, c: Color32) {
    p.line_segment([a, b], Stroke::new(w, c));
    p.circle_filled(a, w / 2.0, c);
    p.circle_filled(b, w / 2.0, c);
}

fn text_px(cam: &Cam2, mm: f64) -> f32 {
    (mm as f32 * cam.s * 1.3).clamp(0.0, 60.0)
}

/// Draws a schematic symbol through the placement transform `xf`.
fn draw_symbol(pa: &egui::Painter, cam: &Cam2, r: Rect, d: &Symbol, unit: u32, xf: &dyn Fn(P) -> P, dirf: &dyn Fn(f64) -> f64, col: Color32, fill_col: Color32, alpha: f32) {
    let sc = |q: P| cam.scr(r, xf(q));
    let w = (0.254 * cam.s).max(1.0);
    let col = fade(col, alpha);
    let fill_col = fade(fill_col, alpha);
    for g in d.gfx_of(unit) {
        match g {
            Gfx::Rect(a, b, f) => {
                let pts: Vec<Pos2> = [*a, p(b.x, a.y), *b, p(a.x, b.y)].iter().map(|q| sc(*q)).collect();
                let fc = match f {
                    Fill::Background => fill_col,
                    Fill::Outline => col,
                    Fill::None => Color32::TRANSPARENT,
                };
                pa.add(Shape::convex_polygon(pts, fc, Stroke::new(w, col)));
            }
            Gfx::Poly(v, f) => {
                let pts: Vec<Pos2> = v.iter().map(|q| sc(*q)).collect();
                if *f != Fill::None && pts.len() >= 3 {
                    pa.add(Shape::convex_polygon(pts.clone(), if *f == Fill::Outline { col } else { fill_col }, Stroke::NONE));
                }
                pa.add(Shape::line(pts, Stroke::new(w, col)));
            }
            Gfx::Circle(c, rad, f) => {
                let fc = match f {
                    Fill::Background => fill_col,
                    Fill::Outline => col,
                    Fill::None => Color32::TRANSPARENT,
                };
                pa.circle(sc(*c), *rad as f32 * cam.s, fc, Stroke::new(w, col));
            }
            Gfx::Arc(a, m, b) => {
                let pts: Vec<Pos2> = crate::pcb::geom::arc3(*a, *m, *b, 48).into_iter().map(sc).collect();
                pa.add(Shape::line(pts, Stroke::new(w, col)));
            }
            Gfx::Text(c, t, size) => {
                let px = text_px(cam, *size);
                if px > 3.0 {
                    pa.text(sc(*c), Align2::CENTER_CENTER, t, FontId::proportional(px), col);
                }
            }
        }
    }
    let px = text_px(cam, 1.0);
    for q in d.pins_of(unit) {
        if q.hidden {
            continue;
        }
        let (a, b) = (sc(q.pos), sc(q.inner()));
        seg(pa, a, b, w, col);
        if px < 3.0 {
            continue;
        }
        let dir = dirf(q.dir);
        let horizontal = (dir.to_radians().cos()).abs() > 0.5;
        if d.show_pin_numbers && q.len > 0.1 {
            let m = pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            let off = if horizontal { vec2(0.0, -px * 0.55) } else { vec2(-px * 0.55, 0.0) };
            pa.text(m + off, if horizontal { Align2::CENTER_BOTTOM } else { Align2::RIGHT_CENTER }, &q.num, FontId::proportional(px * 0.9), col);
        }
        if d.show_pin_names && q.name != "~" && !q.name.is_empty() {
            let v = b - a;
            let n = v.normalized();
            let at = b + n * (0.6 * cam.s);
            let al = if horizontal {
                if n.x > 0.0 {
                    Align2::LEFT_CENTER
                } else {
                    Align2::RIGHT_CENTER
                }
            } else if n.y > 0.0 {
                Align2::CENTER_TOP
            } else {
                Align2::CENTER_BOTTOM
            };
            let name = q.name.replace("~{", "").replace('}', "");
            pa.text(at, al, &name, FontId::proportional(px), fade(INK, alpha));
        }
    }
}

/// A library symbol (unit 1) without placement, for previews.
pub fn draw_symbol_pub(pa: &egui::Painter, cam: &Cam2, r: Rect, d: &Symbol) {
    draw_symbol(pa, cam, r, d, 1, &|q| q, &|a| a, SYM, SYM_FILL, 1.0);
}

// ------------------------------------------------------------------ the application

impl App {
    pub fn pcb_open(&mut self) {
        self.pcb.open = true;
        self.tab = Tab::Pcb;
        if self.pcb.project.schematic.symbols.is_empty() && self.pcb.project.board.footprints.is_empty() && self.pcb.project.board.outline.is_empty() {
            self.pcb.project.board.rect_outline(100.0, 100.0, 50.0, 35.0);
        }
        self.pcb.fit = true;
    }

    pub fn pcb_close(&mut self) {
        self.pcb.ribbon_guard = (0, None);
        self.pcb.open = false;
        self.pcb.tool = PTool::Select;
        self.pcb.route = None;
        self.tab = Tab::Tools;
    }

    pub fn pcb_tick(&mut self) {
        if self.pcb.open && self.tab != Tab::Pcb {
            self.pcb.ribbon_guard = (0, None);
            self.pcb.open = false;
            self.pcb.route = None;
            self.pcb.tool = PTool::Select;
        }
    }

    pub fn pcb_push(&mut self) {
        let snap = self.pcb.project.clone();
        self.pcb.undo.push(snap);
        if self.pcb.undo.len() > 100 {
            self.pcb.undo.remove(0);
        }
        self.pcb.redo.clear();
        self.pcb.changed();
    }

    pub fn pcb_undo(&mut self) {
        if let Some(pr) = self.pcb.undo.pop() {
            let cur = std::mem::replace(&mut self.pcb.project, pr);
            self.pcb.redo.push(cur);
            self.pcb.sel = None;
            self.pcb.changed();
        }
    }

    pub fn pcb_redo(&mut self) {
        if let Some(pr) = self.pcb.redo.pop() {
            let cur = std::mem::replace(&mut self.pcb.project, pr);
            self.pcb.undo.push(cur);
            self.pcb.sel = None;
            self.pcb.changed();
        }
    }

    fn pcb_set_view(&mut self, v: PcbView) {
        self.pcb.view = v;
        self.pcb.tool = PTool::Select;
        self.pcb.sel = None;
        self.pcb.route = None;
        self.pcb.wire.clear();
        self.pcb.zone_pts.clear();
    }

    pub fn pcb_annotate(&mut self) {
        self.pcb_push();
        let n = self.pcb.project.schematic.annotate(false);
        self.say(format!("Annotieren: {} Bauteil(e) nummeriert.", n), MsgKind::Success);
    }

    pub fn pcb_run_erc(&mut self) {
        let v = self.pcb.project.schematic.erc();
        let e = v.iter().filter(|x| x.severity == Severity::Error).count();
        let w = v.len() - e;
        if v.is_empty() {
            self.say("ERC: keine Fehler, keine Warnungen.", MsgKind::Success);
        } else {
            self.say(format!("ERC: {} Fehler, {} Warnung(en).", e, w), if e > 0 { MsgKind::Error } else { MsgKind::Warning });
        }
        self.pcb.erc = Some(v);
        self.pcb.report = true;
    }

    pub fn pcb_run_drc(&mut self) {
        let v = drc::run(&self.pcb.project.board, Some(&self.pcb.project.schematic));
        let e = drc::errors(&v);
        let un = v.iter().filter(|x| x.kind == drc::Kind::Unrouted).count();
        if v.is_empty() {
            self.say("DRC: keine Verletzungen, alles verbunden.", MsgKind::Success);
        } else {
            self.say(format!("DRC: {} Fehler ({} nicht verbunden), {} Warnung(en).", e, un, v.len() - e), if e > 0 { MsgKind::Error } else { MsgKind::Warning });
        }
        self.pcb.drc = Some(v);
        self.pcb.report = true;
    }

    pub fn pcb_update_board(&mut self) {
        let unannotated = self.pcb.project.schematic.symbols.iter().enumerate().any(|(i, s)| !self.pcb.project.schematic.is_power(i) && s.reference.ends_with('?'));
        if unannotated {
            self.pcb.project.schematic.annotate(false);
        }
        self.pcb_push();
        let pr = &mut self.pcb.project;
        let rep = pr.board.update_from(&pr.schematic);
        let mut msg = format!("Platine aus Schaltplan aktualisiert: {} neu, {} geändert, {} entfernt.", rep.added.len(), rep.changed.len(), rep.removed.len());
        if !rep.missing_fp.is_empty() {
            msg.push_str(&format!(" Ohne Footprint: {}.", rep.missing_fp.join(", ")));
        }
        self.say(msg, if rep.missing_fp.is_empty() { MsgKind::Success } else { MsgKind::Warning });
        self.pcb.changed();
        self.pcb_set_view(PcbView::Board);
        self.pcb.fit = true;
    }

    pub fn pcb_fill_zones(&mut self) {
        self.pcb_push();
        self.pcb.project.board.fill_zones();
        self.pcb.changed();
        let n: usize = self.pcb.project.board.zones.iter().map(|z| z.fill.len()).sum();
        self.say(format!("Zonen gefüllt ({} Fläche(n)).", n), MsgKind::Success);
    }

    pub fn pcb_autoroute(&mut self) {
        self.pcb_push();
        let rep = autoroute::route(&mut self.pcb.project.board, None);
        self.pcb.project.board.fill_zones();
        self.pcb.changed();
        let left = self.pcb.ratsnest().len();
        if left == 0 {
            self.say(format!("Autorouter: {} Verbindungen geroutet, {} Vias – alles verbunden.", rep.routed, rep.vias), MsgKind::Success);
        } else {
            self.say(format!("Autorouter: {} Verbindungen geroutet, {} offen ({}) – bitte von Hand verlegen.", rep.routed, left, rep.failed.join(", ")), MsgKind::Warning);
        }
    }

    fn pcb_rotate_sel(&mut self) {
        match self.pcb.sel {
            Some(PSel::Sym(i)) => {
                self.pcb_push();
                let s = &mut self.pcb.cur_mut().symbols[i];
                s.rot = (s.rot + 90.0).rem_euclid(360.0);
            }
            Some(PSel::Label(i)) => {
                self.pcb_push();
                let l = &mut self.pcb.cur_mut().labels[i];
                l.rot = (l.rot + 90.0).rem_euclid(360.0);
            }
            Some(PSel::Fp(i)) => {
                self.pcb_push();
                let f = &mut self.pcb.project.board.footprints[i];
                f.rot = (f.rot + 90.0).rem_euclid(360.0);
            }
            _ => {
                if matches!(self.pcb.tool, PTool::Place(_)) {
                    self.pcb.place_rot = (self.pcb.place_rot + 90.0).rem_euclid(360.0);
                }
            }
        }
    }

    fn pcb_mirror_sel(&mut self) {
        match self.pcb.sel {
            Some(PSel::Sym(i)) => {
                self.pcb_push();
                let s = &mut self.pcb.cur_mut().symbols[i];
                s.mirror = !s.mirror;
            }
            Some(PSel::Fp(i)) => {
                self.pcb_push();
                let f = &mut self.pcb.project.board.footprints[i];
                f.bottom = !f.bottom;
                let side = if f.bottom { "Unterseite" } else { "Oberseite" };
                let r = f.reference.clone();
                self.say(format!("{} auf die {} umgedreht.", r, side), MsgKind::Info);
            }
            _ => {
                if matches!(self.pcb.tool, PTool::Place(_)) {
                    self.pcb.place_mirror = !self.pcb.place_mirror;
                }
            }
        }
    }

    fn pcb_delete_sel(&mut self) {
        let Some(sel) = self.pcb.sel else { return };
        self.pcb_push();
        let path = self.pcb.path.clone();
        let pr = &mut self.pcb.project;
        let Some(sch) = pr.schematic.sheet_at_mut(&path) else { return };
        match sel {
            PSel::Sym(i) => {
                sch.symbols.remove(i);
            }
            PSel::Wire(i) => {
                sch.wires.remove(i);
            }
            PSel::Label(i) => {
                sch.labels.remove(i);
            }
            PSel::Nc(i) => {
                sch.no_connects.remove(i);
            }
            PSel::Sheet(i) => {
                sch.sheets.remove(i);
            }
            PSel::Bus(i) => {
                sch.buses.remove(i);
            }
            PSel::Entry(i) => {
                sch.bus_entries.remove(i);
            }
            PSel::Track(i) => {
                pr.board.tracks.remove(i);
            }
            PSel::Via(i) => {
                pr.board.vias.remove(i);
            }
            PSel::Zone(i) => {
                pr.board.zones.remove(i);
            }
            PSel::Fp(i) => {
                let r = pr.board.footprints[i].reference.clone();
                self.pcb.undo.pop();
                self.say(format!("{} kommt aus dem Schaltplan: dort löschen und die Platine aktualisieren.", r), MsgKind::Warning);
                return;
            }
        }
        self.pcb.sel = None;
        self.pcb.changed();
    }

    pub fn pcb_shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.wants_keyboard_input();
        let (cmd, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |k: egui::Key| ctx.input(|i| i.key_pressed(k));
        if typing {
            return;
        }
        if cmd && pressed(egui::Key::Z) {
            if shift {
                self.pcb_redo()
            } else {
                self.pcb_undo()
            }
            return;
        }
        if cmd && pressed(egui::Key::Y) {
            self.pcb_redo();
            return;
        }
        if cmd && pressed(egui::Key::S) {
            self.pcb_save_kicad(false);
            return;
        }
        if cmd {
            return;
        }
        if pressed(egui::Key::Escape) {
            self.pcb_escape();
        }
        if pressed(egui::Key::R) {
            self.pcb_rotate_sel();
        }
        if pressed(egui::Key::Delete) || pressed(egui::Key::Backspace) {
            self.pcb_delete_sel();
        }
        if pressed(egui::Key::E) {
            self.pcb_edit_props();
        }
        match self.pcb.view {
            PcbView::Schematic => {
                if pressed(egui::Key::X) || pressed(egui::Key::Y) {
                    self.pcb_mirror_sel();
                }
                if pressed(egui::Key::W) {
                    self.pcb_tool(PTool::Wire);
                }
                if pressed(egui::Key::L) {
                    self.pcb_tool(PTool::Label(LabelKind::Local));
                }
                if pressed(egui::Key::P) {
                    let k = self.pcb.power_kind.clone();
                    self.pcb_tool(PTool::Power(k));
                }
                if pressed(egui::Key::Q) {
                    self.pcb_tool(PTool::NoConnect);
                }
                if pressed(egui::Key::B) {
                    self.pcb_tool(PTool::Bus);
                }
                if pressed(egui::Key::A) {
                    if let Some(s) = self.pcb.lib_sel.clone() {
                        self.pcb_tool(PTool::Place(s));
                    }
                }
                if pressed(egui::Key::F8) {
                    self.pcb_update_board();
                }
            }
            PcbView::Board => {
                if pressed(egui::Key::F) {
                    self.pcb_mirror_sel();
                }
                if pressed(egui::Key::X) {
                    self.pcb_tool(PTool::Route);
                }
                if pressed(egui::Key::B) {
                    self.pcb_fill_zones();
                }
                if pressed(egui::Key::V) {
                    if self.pcb.route.is_some() {
                        self.pcb_route_via();
                    } else {
                        self.pcb_tool(PTool::Via);
                    }
                }
                if pressed(egui::Key::Slash) {
                    self.pcb.diagonal_first = !self.pcb.diagonal_first;
                }
                if pressed(egui::Key::PageUp) {
                    self.pcb.layer = F_CU;
                }
                if pressed(egui::Key::PageDown) {
                    self.pcb.layer = B_CU;
                }
            }
        }
    }

    fn pcb_escape(&mut self) {
        if !self.pcb.wire.is_empty() {
            self.pcb.wire.clear();
        } else if self.pcb.route.is_some() {
            self.pcb.route = None;
        } else if !self.pcb.zone_pts.is_empty() {
            self.pcb.zone_pts.clear();
        } else if self.pcb.tool != PTool::Select {
            self.pcb.tool = PTool::Select;
        } else {
            self.pcb.sel = None;
            self.pcb.hl_net = None;
        }
    }

    fn pcb_tool(&mut self, t: PTool) {
        self.pcb.wire.clear();
        self.pcb.route = None;
        self.pcb.zone_pts.clear();
        let hint = match &t {
            PTool::Place(id) => format!("Symbol {} platzieren: Klick setzt, R dreht, X spiegelt, Esc beendet.", id.rsplit(':').next().unwrap_or(id)),
            PTool::Wire => "Draht: Klick setzt Eckpunkte; an einem Pin oder Draht endet er, Doppelklick oder Esc beendet.".into(),
            PTool::Label(_) => "Netzlabel: auf ein Drahtende oder einen Pin klicken.".into(),
            PTool::Power(k) => format!("Versorgungssymbol {}: Klick setzt.", k),
            PTool::NoConnect => "Kein Anschluss: auf einen offenen Pin klicken.".into(),
            PTool::Route => "Leiterbahn: an einem Pad beginnen; / wechselt den 45°-Knick, V setzt ein Via und wechselt die Lage, Doppelklick beendet.".into(),
            PTool::Via => "Via: Klick setzt ein Via (Netz von der Leiterbahn darunter).".into(),
            PTool::Zone => "Zone: Eckpunkte klicken, Doppelklick schließt den Umriss.".into(),
            PTool::Sheet => "Hierarchisches Blatt: Klick setzt das Blatt (Name im Dialog), Doppelklick auf ein Blatt öffnet es.".into(),
            PTool::SheetPin => "Blattpin: auf den Rand eines Blatts klicken; im Blatt entsteht das hierarchische Label dazu.".into(),
            PTool::Bus => "Bus: wie ein Draht zeichnen; die Leitungen verbinden sich über Netzlabel (D0 … D7), der Bus trägt ein Label wie D[0..7].".into(),
            PTool::BusEntry => "Buseintrag: auf den Bus klicken; der Eintrag führt schräg zur Leitung.".into(),
            PTool::DiffPair => "Differenzielles Paar: an einem Pad des Paars beginnen (USB_D+/USB_D-, X_P/X_N); beide Bahnen laufen im Klassenabstand, Doppelklick beendet.".into(),
            PTool::Tune => "Länge abstimmen: auf eine Leiterbahn klicken und die Ziellänge eingeben (Mäander).".into(),
            PTool::Select => String::new(),
        };
        if !hint.is_empty() {
            self.say(hint, MsgKind::Info);
        }
        self.pcb.tool = t;
    }

    fn pcb_edit_props(&mut self) {
        if let Some(PSel::Sym(i)) = self.pcb.sel {
            let s = &self.pcb.cur().symbols[i];
            if !self.pcb.cur().is_power(i) {
                self.pcb.props = Some(Props { i, reference: s.reference.clone(), value: s.value.clone(), footprint: s.footprint.clone() });
            }
        }
    }

    // ------------------------------------------------------------------ files

    fn pcb_base_name(&self) -> String {
        let n = self.pcb.project.name.trim();
        if n.is_empty() {
            "PCB".into()
        } else {
            n.to_string()
        }
    }

    pub fn pcb_save_kicad(&mut self, ask: bool) {
        let name = self.pcb_base_name();
        let target = match (&self.pcb.dir, ask) {
            (Some(d), false) => Some(d.join(format!("{}.kicad_pcb", name))),
            _ => rfd::FileDialog::new().set_title("KiCad-Projekt speichern").set_directory(self.pcb.dir.clone().unwrap_or_else(|| self.work_dir())).set_file_name(format!("{}.kicad_pcb", name)).add_filter("KiCad-Platine", &["kicad_pcb"]).save_file(),
        };
        let Some(path) = target else { return };
        let dir = path.parent().map(|x| x.to_path_buf()).unwrap_or_default();
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or(name);
        match self.pcb_write_kicad(&dir, &stem) {
            Ok(files) => {
                self.pcb.dir = Some(dir.clone());
                self.pcb.project.name = stem;
                self.say(format!("KiCad-Projekt gespeichert in {}: {}", dir.display(), files.join(", ")), MsgKind::Success);
            }
            Err(e) => self.say(format!("Speichern fehlgeschlagen: {}", e), MsgKind::Error),
        }
    }

    pub fn pcb_add_library(&mut self, p: std::path::PathBuf) {
        match load_library(&p) {
            Ok((ns, nf)) => {
                if !self.pcb.libs.contains(&p) {
                    self.pcb.libs.push(p.clone());
                }
                self.say(format!("Bibliothek {} geladen: {} Symbole, {} Footprints.", p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default(), ns, nf), MsgKind::Success);
            }
            Err(e) => self.say(format!("Bibliothek konnte nicht geladen werden: {}", e), MsgKind::Error),
        }
    }

    pub fn pcb_write_kicad(&self, dir: &std::path::Path, stem: &str) -> Result<Vec<String>, String> {
        let files = kicad::write_project(&self.pcb.project, stem);
        let mut out = vec![];
        for (n, t) in files {
            std::fs::write(dir.join(&n), t).map_err(|e| e.to_string())?;
            out.push(n);
        }
        Ok(out)
    }

    pub fn pcb_import_kicad(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_title("KiCad-Datei öffnen").set_directory(self.pcb.dir.clone().unwrap_or_else(|| self.work_dir())).add_filter("KiCad", &["kicad_sch", "kicad_pcb", "kicad_pro"]).pick_file() else { return };
        self.pcb_load_kicad(&path);
    }

    /// Opens a KiCad project: the schematic and the board next to it (same name).
    pub fn pcb_load_kicad(&mut self, path: &std::path::Path) {
        match kicad::read_project(path) {
            Ok((pr, got)) => {
                let stem = pr.name.clone();
                let view = if got.contains(&"Schaltplan") { PcbView::Schematic } else { PcbView::Board };
                self.pcb.set_project(pr);
                self.pcb.dir = path.parent().map(|x| x.to_path_buf());
                crate::pcb::models::set_project_dir(self.pcb.dir.clone());
                self.pcb_set_view(view);
                self.say(format!("KiCad-Projekt {} geöffnet ({}).", stem, got.join(" und ")), MsgKind::Success);
            }
            Err(e) => self.say(format!("KiCad-Projekt: {}", e), MsgKind::Error),
        }
    }

    pub fn pcb_export_fab(&mut self) {
        let Some(dir) = rfd::FileDialog::new().set_title("Ordner für Gerber- und Bohrdaten").set_directory(self.pcb.dir.clone().unwrap_or_else(|| self.work_dir())).pick_folder() else { return };
        let name = self.pcb_base_name();
        let v = drc::run(&self.pcb.project.board, None);
        let files = gerber::fab_files(&self.pcb.project.board, &name);
        for (n, t) in &files {
            if let Err(e) = std::fs::write(dir.join(n), t) {
                return self.say(format!("Gerber konnte nicht gespeichert werden: {}", e), MsgKind::Error);
            }
        }
        let e = drc::errors(&v);
        if e > 0 {
            self.say(format!("{} Fertigungsdateien in {} gespeichert – Achtung: DRC meldet {} Fehler.", files.len(), dir.display(), e), MsgKind::Warning);
        } else {
            self.say(format!("{} Fertigungsdateien (Gerber RS-274X, Excellon) in {} gespeichert.", files.len(), dir.display()), MsgKind::Success);
        }
    }

    /// Pick-and-place data (.pos per side and CSV) into a folder.
    pub fn pcb_export_pnp(&mut self) {
        let Some(dir) = rfd::FileDialog::new().set_title("Ordner für die Bestückungsdaten").set_directory(self.pcb.dir.clone().unwrap_or_else(|| self.work_dir())).pick_folder() else { return };
        let name = self.pcb_base_name();
        let b = &self.pcb.project.board;
        let files = [
            (format!("{}-top.pos", name), crate::pcb::fab::pos_file(b, Some(true))),
            (format!("{}-bottom.pos", name), crate::pcb::fab::pos_file(b, Some(false))),
            (format!("{}-pos.csv", name), crate::pcb::fab::pos_csv(b)),
        ];
        for (n, t) in &files {
            if let Err(e) = std::fs::write(dir.join(n), t) {
                return self.say(format!("Bestückungsdaten konnten nicht gespeichert werden: {}", e), MsgKind::Error);
            }
        }
        self.say(format!("Bestückungsdaten gespeichert ({} Bauteile): {}-top.pos, {}-bottom.pos, {}-pos.csv in {}.", b.footprints.len(), name, name, name, dir.display()), MsgKind::Success);
    }

    pub fn pcb_export_ipc(&mut self) {
        let name = self.pcb_base_name();
        let Some(path) = rfd::FileDialog::new().set_title("IPC-2581 speichern").set_directory(self.pcb.dir.clone().unwrap_or_else(|| self.work_dir())).set_file_name(format!("{}.xml", name)).add_filter("IPC-2581", &["xml", "cvg"]).save_file() else { return };
        match std::fs::write(&path, crate::pcb::fab::ipc2581(&self.pcb.project.board, &name)) {
            Ok(()) => self.say(format!("IPC-2581 gespeichert: {}", path.display()), MsgKind::Success),
            Err(e) => self.say(format!("IPC-2581 konnte nicht gespeichert werden: {}", e), MsgKind::Error),
        }
    }

    pub fn pcb_export_bom(&mut self) {
        let name = self.pcb_base_name();
        let Some(path) = rfd::FileDialog::new().set_title("Stückliste speichern").set_directory(self.pcb.dir.clone().unwrap_or_else(|| self.work_dir())).set_file_name(format!("{}-Stueckliste.csv", name)).add_filter("CSV", &["csv"]).save_file() else { return };
        match std::fs::write(&path, bom::csv(&self.pcb.project.schematic)) {
            Ok(()) => self.say(format!("Stückliste gespeichert: {}", path.display()), MsgKind::Success),
            Err(e) => self.say(format!("Stückliste konnte nicht gespeichert werden: {}", e), MsgKind::Error),
        }
    }

    /// The board as a Freo assembly (laminate, copper, components).
    pub fn pcb_make_3d(&mut self) {
        let name = self.pcb_base_name();
        match body3d::assembly(&self.pcb.project.board, &name) {
            Ok(asm) => {
                self.pcb_close();
                self.leave_home();
                self.editor = None;
                self.sketch = None;
                self.drw = None;
                self.selection.clear();
                self.tree_selection.clear();
                self.undo.clear();
                self.redo.clear();
                self.data = asm.skeleton.clone();
                self.asm = Some(super::assembly::AsmSession::new(asm));
                self.path = None;
                self.dirty = true;
                self.tab = Tab::Model;
                self.asm_regenerate(false);
                self.camera.rot = super::camera::StdView::Standard.rotation();
                self.fit_pending = true;
                // every drill would show its axis: off here, back on in the graphics toolbar
                self.show_axes = false;
                self.say(format!("3D-Modell der Leiterplatte als Baugruppe {}_ASM erzeugt (Leiterplatte, Kupfer, Bauteile) – bereit für das Gehäuse.", name.to_uppercase()), MsgKind::Success);
            }
            Err(e) => self.say(format!("3D-Modell: {}", e), MsgKind::Error),
        }
    }

    /// Board outline from the open part: the last sketch (or the profile of the last
    /// extrude) with closed loops.
    pub fn pcb_outline_from_model(&mut self) {
        let mut found = None;
        for f in self.data.features.iter().rev() {
            let sk = match &f.kind {
                FeatureKind::Sketch(s) => Some(&s.sketch),
                FeatureKind::Extrude(e) => Some(&e.sketch),
                _ => None,
            };
            if let Some(sk) = sk {
                if let Ok(regions) = sk.profile_regions() {
                    if let Some(r) = regions.first() {
                        let mut loops = vec![r.outer.pts.clone()];
                        loops.extend(r.holes.iter().map(|h| h.pts.clone()));
                        found = Some((f.name.clone(), loops));
                        break;
                    }
                }
            }
        }
        let Some((name, loops)) = found else {
            return self.say("Kein geschlossener Skizzenumriss im Teil gefunden – erst eine Skizze mit geschlossener Kontur anlegen.", MsgKind::Warning);
        };
        self.pcb_push();
        let b = &mut self.pcb.project.board;
        b.outline_from_loops(&loops);
        // keep the board where it was: move the outline next to the old origin
        if let Some((lo, _)) = b.size() {
            let d = p(100.0, 100.0) - lo;
            for l in &mut b.outline {
                for q in l.iter_mut() {
                    *q += d;
                }
            }
        }
        let (lo, hi) = b.size().unwrap_or((P::ZERO, P::ZERO));
        self.pcb.changed();
        self.pcb.fit = true;
        self.say(format!("Platinenumriss aus {} übernommen ({} × {} mm).", name, crate::pcb::geom::mm(hi.x - lo.x), crate::pcb::geom::mm(hi.y - lo.y)), MsgKind::Success);
    }

    // ------------------------------------------------------------------ ribbon

    pub fn pcb_ribbon(&mut self, ui: &mut egui::Ui) {
        // own id scope per view: ribbon buttons get positional ids, and a click that switches
        // ribbons must not land on the button that takes the same place in the new one
        let view = self.pcb.view == PcbView::Schematic;
        // the click that opened this ribbon (or switched its view) is still in the input of
        // the next passes: ignore clicks there so it cannot hit the button now in its place
        let now = std::time::Instant::now();
        if self.pcb.ribbon_guard.0 != view as u8 + 1 {
            self.pcb.ribbon_guard = (view as u8 + 1, Some(now));
        }
        let fresh = self.pcb.ribbon_guard.1.is_some_and(|t| now.duration_since(t).as_secs_f32() < 0.4);
        if fresh {
            ui.ctx().request_repaint();
        }
        ui.push_id(("pcb_ribbon", view), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if fresh {
                // same look, no interaction
                ui.add_enabled_ui(false, |ui| {
                    self.pcb_ribbon_body(ui)
                });
            } else {
                self.pcb_ribbon_body(ui)
            }
        });
    }

    fn pcb_ribbon_body(&mut self, ui: &mut egui::Ui) {
        let sch = self.pcb.view == PcbView::Schematic;
        group(ui, "Editor", |ui| {
            if large(ui, Icon::PcbSchematic, "Schaltplan", true, sch).on_hover_text("Schaltplan-Editor: Symbole, Drähte, Netzlabel, ERC").clicked() {
                self.pcb_set_view(PcbView::Schematic);
            }
            if large(ui, Icon::PcbBoard, "Platine", true, !sch).on_hover_text("Leiterplatten-Editor: Platzieren, Routen, Zonen, DRC").clicked() {
                self.pcb_set_view(PcbView::Board);
                self.pcb.fit = !self.pcb.brd_cam.init;
            }
        });
        if sch {
            self.pcb_sch_ribbon(ui);
        } else {
            self.pcb_brd_ribbon(ui);
        }
        group(ui, "Bibliothek", |ui| {
            if sch {
                if large(ui, Icon::PcbSymbol, "Symbol-\nEditor", true, self.pcb.sym_ed.is_some()).on_hover_text("Eigenes Symbol aus einer Pintabelle erstellen oder das gewählte bearbeiten").clicked() {
                    self.pcb_open_symbol_editor();
                }
            } else if large(ui, Icon::PcbBoard, "Footprint-\nEditor", true, self.pcb.fp_ed.is_some()).on_hover_text("Footprint mit Generator (Chip, SOIC, QFP/QFN, THT) oder Padtabelle erstellen").clicked() {
                self.pcb_open_fp_editor();
            }
            stack(ui, |ui| {
                if small(ui, Icon::PcbKicad, "Exportieren", true, false).on_hover_text("Alle verwendeten Symbole und Footprints als .kicad_sym und .pretty speichern").clicked() {
                    self.pcb_export_libraries();
                }
            });
        });
        group(ui, "Ausgabe", |ui| {
            if large(ui, Icon::PcbGerber, "Gerber +\nBohrdaten", !self.pcb.project.board.outline.is_empty(), false).on_hover_text("Fertigungsdaten: Gerber RS-274X aller Lagen und Excellon-Bohrdateien").clicked() {
                self.pcb_export_fab();
            }
            if large(ui, Icon::Pcb3d, "3D-\nModell", !self.pcb.project.board.outline.is_empty(), false).on_hover_text("Leiterplatte mit Bauteilen als Freo-Baugruppe – für Gehäuse und Baugruppen").clicked() {
                self.pcb_make_3d();
            }
            stack(ui, |ui| {
                if small(ui, Icon::PcbBom, "Stückliste", true, false).on_hover_text("Stückliste (CSV), nach Wert und Footprint gruppiert").clicked() {
                    self.pcb_export_bom();
                }
                if small(ui, Icon::PcbKicad, "KiCad speichern", true, false).on_hover_text("Als KiCad-Projekt speichern (.kicad_sch, .kicad_pcb)").clicked() {
                    self.pcb_save_kicad(true);
                }
                if small(ui, Icon::Open, "KiCad öffnen", true, false).on_hover_text("KiCad-Schaltplan und -Platine öffnen").clicked() {
                    self.pcb_import_kicad();
                }
            });
            stack(ui, |ui| {
                let has = !self.pcb.project.board.footprints.is_empty();
                if small(ui, Icon::PrArrange, "Bestückung", has, false).on_hover_text("Bestückungsdaten (Pick & Place): .pos je Seite und CSV").clicked() {
                    self.pcb_export_pnp();
                }
                if small(ui, Icon::PcbGerber, "IPC-2581", !self.pcb.project.board.outline.is_empty(), false).on_hover_text("Alle Fertigungsdaten in einer IPC-2581-Datei (XML)").clicked() {
                    self.pcb_export_ipc();
                }
            });
        });
        group(ui, "Schließen", |ui| {
            if large(ui, Icon::Close, "Leiterplatte\nschließen", true, false).on_hover_text("Zurück zu Werkzeuge").clicked() {
                self.pcb_close();
            }
        });
    }

    fn pcb_sch_ribbon(&mut self, ui: &mut egui::Ui) {
        let t = self.pcb.tool.clone();
        group(ui, "Einfügen", |ui| {
            let placing = matches!(t, PTool::Place(_));
            if large(ui, Icon::PcbSymbol, "Symbol", true, placing).on_hover_text("Symbol aus der Bibliothek platzieren (A)").clicked() {
                let id = self.pcb.lib_sel.clone().unwrap_or_else(|| "Device:R".into());
                self.pcb.lib_sel = Some(id.clone());
                self.pcb_tool(PTool::Place(id));
            }
            stack(ui, |ui| {
                if small(ui, Icon::PcbWire, "Draht", true, t == PTool::Wire).on_hover_text("Draht zeichnen (W)").clicked() {
                    self.pcb_tool(PTool::Wire);
                }
                if small(ui, Icon::PcbLabel, "Netzlabel", true, t == PTool::Label(LabelKind::Local)).on_hover_text("Netzlabel setzen (L): gleiche Namen sind verbunden").clicked() {
                    self.pcb_tool(PTool::Label(LabelKind::Local));
                }
                if small(ui, Icon::PcbLabel, "Globales Label", true, t == PTool::Label(LabelKind::Global)).clicked() {
                    self.pcb_tool(PTool::Label(LabelKind::Global));
                }
            });
            let pw = stack(ui, |ui| {
                let r = small(ui, Icon::PcbPower, &format!("Versorgung: {} ▾", self.pcb.power_kind), true, matches!(t, PTool::Power(_))).on_hover_text("Versorgungssymbol (P)");
                if small(ui, Icon::PcbNoConnect, "Kein Anschluss", true, t == PTool::NoConnect).on_hover_text("Pin als absichtlich offen markieren (Q)").clicked() {
                    self.pcb_tool(PTool::NoConnect);
                }
                r
            });
            let mut pick = None;
            drop_menu(ui, "pcb_power", &pw, |ui| {
                ui.set_min_width(200.0);
                for s in symbols::all().into_iter().filter(|s| s.power) {
                    if menu_row(ui, Some(Icon::PcbPower), s.short(), "", true, self.pcb.power_kind == s.short()).clicked() {
                        pick = Some(s.short().to_string());
                    }
                }
            });
            if let Some(k) = pick {
                self.pcb.power_kind = k.clone();
                self.pcb_tool(PTool::Power(k));
            }
        });
        group(ui, "Hierarchie", |ui| {
            if large(ui, Icon::PcbSchematic, "Blatt", true, t == PTool::Sheet).on_hover_text("Hierarchisches Blatt einfügen; Doppelklick öffnet es").clicked() {
                self.pcb_tool(PTool::Sheet);
            }
            stack(ui, |ui| {
                if small(ui, Icon::PcbNoConnect, "Blattpin", true, t == PTool::SheetPin).on_hover_text("Anschluss am Blattrand – verbunden mit dem gleichnamigen hierarchischen Label im Blatt").clicked() {
                    self.pcb_tool(PTool::SheetPin);
                }
                if small(ui, Icon::PcbLabel, "Hier. Label", true, t == PTool::Label(LabelKind::Hier)).on_hover_text("Hierarchisches Label (Verbindung zum übergeordneten Blatt)").clicked() {
                    self.pcb_tool(PTool::Label(LabelKind::Hier));
                }
                let up = !self.pcb.path.is_empty();
                if small(ui, Icon::Undo, "Nach oben", up, false).on_hover_text("Zurück zum übergeordneten Blatt").clicked() {
                    self.pcb_leave_sheet();
                }
            });
            stack(ui, |ui| {
                if small(ui, Icon::PcbWire, "Bus", true, t == PTool::Bus).on_hover_text("Bus zeichnen (B); Mitglieder verbinden sich über Netzlabel").clicked() {
                    self.pcb_tool(PTool::Bus);
                }
                if small(ui, Icon::PcbWire, "Buseintrag", true, t == PTool::BusEntry).on_hover_text("Schräger Eintrag vom Bus zur Leitung").clicked() {
                    self.pcb_tool(PTool::BusEntry);
                }
                let multi = match self.pcb.sel {
                    Some(PSel::Sym(i)) => self.pcb.cur().symbols.get(i).and_then(|s| self.pcb.cur().def(&s.lib)).is_some_and(|d| d.units > 1),
                    _ => false,
                };
                if small(ui, Icon::PcbSymbol, "Einheit +", multi, false).on_hover_text("Weitere Einheit (Gatter) des gewählten Bauteils platzieren").clicked() {
                    self.pcb_add_unit();
                }
            });
        });
        let has_sel = self.pcb.sel.is_some();
        group(ui, "Bearbeiten", |ui| {
            stack(ui, |ui| {
                if small(ui, Icon::PrRotate, "Drehen", has_sel || matches!(t, PTool::Place(_)), false).on_hover_text("Um 90° drehen (R)").clicked() {
                    self.pcb_rotate_sel();
                }
                if small(ui, Icon::MoMirror, "Spiegeln", has_sel || matches!(t, PTool::Place(_)), false).on_hover_text("Spiegeln (X)").clicked() {
                    self.pcb_mirror_sel();
                }
                if small(ui, Icon::Delete, "Löschen", has_sel, false).on_hover_text("Auswahl löschen (Entf)").clicked() {
                    self.pcb_delete_sel();
                }
            });
            stack(ui, |ui| {
                if small(ui, Icon::Options, "Eigenschaften", matches!(self.pcb.sel, Some(PSel::Sym(_))), false).on_hover_text("Referenz, Wert und Footprint (E)").clicked() {
                    self.pcb_edit_props();
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if tiny(ui, Icon::Undo, "Rückgängig (⌘Z)", !self.pcb.undo.is_empty(), false, false).clicked() {
                        self.pcb_undo();
                    }
                    if tiny(ui, Icon::Redo, "Wiederholen (⇧⌘Z)", !self.pcb.redo.is_empty(), false, false).clicked() {
                        self.pcb_redo();
                    }
                });
            });
        });
        group(ui, "Prüfen", |ui| {
            if large(ui, Icon::PcbAnnotate, "Annotieren", true, false).on_hover_text("Referenzen nummerieren (R1, R2 …) von links nach rechts").clicked() {
                self.pcb_annotate();
            }
            let label = match &self.pcb.erc {
                None => "ERC".to_string(),
                Some(v) if v.is_empty() => "ERC\nfehlerfrei".to_string(),
                Some(v) => format!("ERC\n{} Meldung(en)", v.len()),
            };
            if large(ui, Icon::PcbErc, &label, true, self.pcb.report && self.pcb.erc.is_some()).on_hover_text("Elektrische Regelprüfung: offene Pins, nicht versorgte Eingänge, Ausgangskonflikte, Referenzen").clicked() {
                self.pcb_run_erc();
            }
        });
        group(ui, "Netzliste", |ui| {
            if large(ui, Icon::PcbUpdate, "Platine\naktualisieren", true, false).on_hover_text("Footprints und Netze in die Platine übernehmen (F8)").clicked() {
                self.pcb_update_board();
            }
        });
    }

    fn pcb_brd_ribbon(&mut self, ui: &mut egui::Ui) {
        let t = self.pcb.tool.clone();
        group(ui, "Platine", |ui| {
            if large(ui, Icon::PcbOutline, "Umriss", true, self.pcb.outline_dlg.is_some()).on_hover_text("Rechteckigen Platinenumriss festlegen").clicked() {
                let (w, h) = self.pcb.project.board.size().map(|(lo, hi)| (hi.x - lo.x, hi.y - lo.y)).unwrap_or((50.0, 35.0));
                self.pcb.outline_dlg = Some((crate::pcb::geom::mm(w), crate::pcb::geom::mm(h)));
            }
            stack(ui, |ui| {
                if small(ui, Icon::Sketch, "Umriss aus Skizze", true, false).on_hover_text("Geschlossene Kontur der letzten Skizze des offenen Teils übernehmen").clicked() {
                    self.pcb_outline_from_model();
                }
                let four = self.pcb.project.board.layers >= 4;
                if small(ui, Icon::PcbLayers, if four { "4 Lagen" } else { "2 Lagen" }, true, false).on_hover_text("Zwischen 2 und 4 Kupferlagen umschalten").clicked() {
                    self.pcb_push();
                    self.pcb.project.board.layers = if four { 2 } else { 4 };
                    if four && self.pcb.layer > 1 {
                        self.pcb.layer = F_CU;
                    }
                }
                if small(ui, Icon::PcbRules, "Regeln", true, self.pcb.rules_dlg.is_some()).on_hover_text("Abstände, Leiterbahnbreiten, Vias").clicked() {
                    self.pcb.rules_dlg = Some(self.pcb.project.board.rules.clone());
                }
            });
        });
        let has_sel = self.pcb.sel.is_some();
        group(ui, "Platzieren", |ui| {
            if large(ui, Icon::PcbUpdate, "Aus Schaltplan\naktualisieren", true, false).on_hover_text("Footprints und Netze aus dem Schaltplan übernehmen (F8)").clicked() {
                self.pcb_update_board();
            }
            stack(ui, |ui| {
                if small(ui, Icon::PrRotate, "Drehen", has_sel, false).on_hover_text("Bauteil um 90° drehen (R)").clicked() {
                    self.pcb_rotate_sel();
                }
                if small(ui, Icon::PcbFlip, "Umdrehen", matches!(self.pcb.sel, Some(PSel::Fp(_))), false).on_hover_text("Auf die andere Seite der Platine (F)").clicked() {
                    self.pcb_mirror_sel();
                }
                if small(ui, Icon::Delete, "Löschen", has_sel, false).on_hover_text("Leiterbahn, Via oder Zone löschen (Entf)").clicked() {
                    self.pcb_delete_sel();
                }
            });
        });
        let layers: Vec<u8> = if self.pcb.project.board.layers >= 4 { vec![F_CU, 2, 3, B_CU] } else { vec![F_CU, B_CU] };
        let lr = group(ui, "Verdrahten", |ui| {
            if large(ui, Icon::PcbTrack, "Leiterbahn", true, t == PTool::Route).on_hover_text("Leiterbahn von Hand verlegen (X)").clicked() {
                self.pcb_tool(PTool::Route);
            }
            let lr = stack(ui, |ui| {
                if small(ui, Icon::PcbVia, "Via", true, t == PTool::Via).on_hover_text("Durchkontaktierung setzen (V)").clicked() {
                    self.pcb_tool(PTool::Via);
                }
                let lr = small(ui, Icon::PcbLayers, &format!("Lage: {} ▾", layer_name(self.pcb.layer)), true, false).on_hover_text("Aktive Kupferlage (Bild↑/Bild↓)");
                if small(ui, Icon::PcbAutoroute, "Autorouter", !self.pcb.project.board.outline.is_empty(), false).on_hover_text("Offene Verbindungen automatisch verlegen (Raster-Router, alle Kupferlagen)").clicked() {
                    self.pcb_autoroute();
                }
                lr
            });
            let mr = stack(ui, |ui| {
                if small(ui, Icon::PcbTrack, "Diff. Paar", true, t == PTool::DiffPair).on_hover_text("Differenzielles Paar verlegen (USB_D+/USB_D-, X_P/X_N)").clicked() {
                    self.pcb_tool(PTool::DiffPair);
                }
                if small(ui, Icon::PcbRules, "Länge abstimmen", true, t == PTool::Tune).on_hover_text("Leiterbahnlänge mit Mäander verlängern").clicked() {
                    self.pcb_tool(PTool::Tune);
                }
                small(ui, Icon::Options, &format!("{} ▾", self.pcb.router_mode.name()), true, false).on_hover_text("Verhalten bei Hindernissen: anzeigen, ausweichen oder schieben")
            });
            let mut set_mode = None;
            drop_menu(ui, "pcb_router_mode", &mr, |ui| {
                ui.set_min_width(220.0);
                for md in [crate::pcb::route::Mode::Highlight, crate::pcb::route::Mode::Walkaround, crate::pcb::route::Mode::Shove] {
                    if menu_row(ui, None, md.name(), "", true, self.pcb.router_mode == md).clicked() {
                        set_mode = Some(md);
                    }
                }
            });
            if let Some(md) = set_mode {
                self.pcb.router_mode = md;
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if tiny(ui, Icon::Undo, "Rückgängig (⌘Z)", !self.pcb.undo.is_empty(), false, false).clicked() {
                    self.pcb_undo();
                }
                if tiny(ui, Icon::Redo, "Wiederholen (⇧⌘Z)", !self.pcb.redo.is_empty(), false, false).clicked() {
                    self.pcb_redo();
                }
            });
            lr
        });
        let mut set_layer = None;
        drop_menu(ui, "pcb_layer", &lr, |ui| {
            ui.set_min_width(200.0);
            for l in &layers {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                if resp.hovered() || self.pcb.layer == *l {
                    ui.painter().rect_filled(r, 0.0, if self.pcb.layer == *l { SEL_BG } else { HOVER_BG });
                }
                ui.painter().rect_filled(Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(14.0, 14.0)), 1.0, layer_color(*l));
                ui.painter().text(pos2(r.left() + 32.0, r.center().y), Align2::LEFT_CENTER, layer_title(*l), FontId::proportional(14.0), TEXT);
                if resp.clicked() {
                    set_layer = Some(*l);
                    ui.memory_mut(|m| m.close_popup());
                }
            }
        });
        if let Some(l) = set_layer {
            self.pcb.layer = l;
            if let Some(r) = &mut self.pcb.route {
                r.layer = l;
            }
        }
        group(ui, "Zonen", |ui| {
            if large(ui, Icon::PcbZone, "Zone", true, t == PTool::Zone || self.pcb.zone_dlg.is_some()).on_hover_text("Kupferfläche zeichnen (z. B. GND-Fläche)").clicked() {
                let nets = self.pcb.project.board.nets();
                let k = nets.iter().position(|n| *n == self.pcb.zone_cfg.0).or_else(|| nets.iter().position(|n| n == "GND")).unwrap_or(0);
                self.pcb.zone_dlg = Some(ZoneDlg { net: k, layer: self.pcb.zone_cfg.1, clearance: crate::pcb::geom::mm(self.pcb.zone_cfg.2) });
            }
            stack(ui, |ui| {
                let has = !self.pcb.project.board.zones.is_empty();
                if small(ui, Icon::PcbFill, "Zonen füllen", has, false).on_hover_text("Alle Zonen neu füllen (B)").clicked() {
                    self.pcb_fill_zones();
                }
                if small(ui, Icon::Delete, "Füllungen entfernen", has, false).clicked() {
                    self.pcb_push();
                    for z in &mut self.pcb.project.board.zones {
                        z.fill.clear();
                    }
                    self.pcb.changed();
                }
            });
        });
        group(ui, "Prüfen", |ui| {
            let label = match &self.pcb.drc {
                None => "DRC".to_string(),
                Some(v) if v.is_empty() => "DRC\nfehlerfrei".to_string(),
                Some(v) => format!("DRC\n{} Meldung(en)", v.len()),
            };
            if large(ui, Icon::PcbDrc, &label, true, self.pcb.report && self.pcb.drc.is_some()).on_hover_text("Design-Regelprüfung: Abstände, Kurzschlüsse, Breiten, Bohrungen, Rand, offene Verbindungen").clicked() {
                self.pcb_run_drc();
            }
        });
    }

    // ------------------------------------------------------------------ left panel

    pub fn pcb_panel(&mut self, ui: &mut egui::Ui) {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
        ui.painter().rect_filled(r.expand2(vec2(4.0, 0.0)), 0.0, TITLE_BG);
        let sch = self.pcb.view == PcbView::Schematic;
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(20.0, 20.0)), if sch { Icon::PcbSchematic } else { Icon::PcbBoard }, true);
        ui.painter().text(pos2(r.left() + 34.0, r.center().y), Align2::LEFT_CENTER, format!("{} – {}", self.pcb.project.name, if sch { "Schaltplan" } else { "Platine" }), FontId::proportional(14.0), TEXT);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 1.0);
            if sch {
                self.pcb_lib_panel(ui);
            } else {
                self.pcb_board_panel(ui);
            }
        });
    }

    fn pcb_lib_panel(&mut self, ui: &mut egui::Ui) {
        section(ui, "Symbolbibliothek");
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Suchen").size(13.0).color(TEXT));
            ui.add_sized(vec2(ui.available_width() - 4.0, 26.0), egui::TextEdit::singleline(&mut self.pcb.lib_filter).hint_text("z. B. LED, 555, USB"));
        });
        ui.add_space(4.0);
        let f = self.pcb.lib_filter.to_lowercase();
        let mut pick = None;
        egui::ScrollArea::vertical().id_salt("pcb_lib").max_height(300.0).show(ui, |ui| {
            for cat in symbols::categories() {
                let items: Vec<&Symbol> = symbols::all().into_iter().filter(|s| s.category == cat && (f.is_empty() || s.id.to_lowercase().contains(&f) || s.description.to_lowercase().contains(&f) || s.value.to_lowercase().contains(&f))).collect();
                if items.is_empty() {
                    continue;
                }
                ui.label(egui::RichText::new(cat).size(12.0).color(Color32::from_rgb(110, 116, 124)));
                for s in items {
                    let sel = self.pcb.lib_sel.as_deref() == Some(&s.id);
                    if row(ui, s.short(), &s.description.chars().take(28).collect::<String>(), sel, None).on_hover_text(&s.description).clicked() {
                        pick = Some(s.id.clone());
                    }
                }
            }
        });
        if let Some(id) = pick {
            self.pcb.lib_sel = Some(id.clone());
            self.pcb.place_rot = 0.0;
            self.pcb.place_mirror = false;
            self.pcb_tool(PTool::Place(id));
        }
        // preview
        if let Some(d) = self.pcb.lib_sel.as_deref().and_then(symbols::find) {
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 130.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, SHEET);
            ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
            let (lo, hi) = d.bounds();
            let mut cam = Cam2::default();
            cam.fit(r, lo - P::splat(1.0), hi + P::splat(1.0));
            cam.s = cam.s.min(9.0);
            let pa = ui.painter().with_clip_rect(r);
            draw_symbol(&pa, &cam, r, d, 1, &|q| q, &|a| a, SYM, SYM_FILL, 1.0);
            ui.label(egui::RichText::new(format!("{} · {}", d.id, d.description)).size(12.0).color(TEXT));
            if !d.footprint.is_empty() {
                ui.label(egui::RichText::new(format!("Footprint: {}", d.footprint.rsplit(':').next().unwrap_or(""))).size(12.0).color(Color32::from_rgb(110, 116, 124)));
            }
            if let Some(port) = footprints::find(&d.footprint).and_then(|f| f.port.clone()) {
                ui.label(egui::RichText::new(format!("Gehäuseöffnung: Skizzenpalette ▸ Ports ▸ {}", port)).size(12.0).color(ACCENT));
            }
        }
        // KiCad libraries
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if dlgkit::btn(ui, "Symbolbibliothek laden …", 150.0, 0) {
                if let Some(files) = rfd::FileDialog::new().set_title("KiCad-Symbolbibliothek (.kicad_sym)").add_filter("KiCad-Symbole", &["kicad_sym"]).pick_files() {
                    for p in files {
                        self.pcb_add_library(p);
                    }
                }
            }
            if dlgkit::btn(ui, "Footprints laden …", 130.0, 0) {
                if let Some(dir) = rfd::FileDialog::new().set_title("KiCad-Footprintbibliothek (.pretty-Ordner)").pick_folder() {
                    self.pcb_add_library(dir);
                }
            }
            if dlgkit::btn(ui, "3D-Modelle …", 100.0, 0) {
                if let Some(dir) = rfd::FileDialog::new().set_title("Ordner mit 3D-Modellen (STEP)").pick_folder() {
                    self.pcb_add_library(dir);
                }
            }
        });
        let mut drop_lib = None;
        for (k, l) in self.pcb.libs.iter().enumerate() {
            let name = l.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
            if row(ui, &name, "entfernen", false, Some(if l.is_dir() { Color32::from_rgb(214, 160, 70) } else { SYM })).on_hover_text(l.display().to_string()).clicked() {
                drop_lib = Some(k);
            }
        }
        if let Some(k) = drop_lib {
            let l = self.pcb.libs.remove(k);
            self.say(format!("{} wird beim nächsten Start nicht mehr geladen.", l.display()), MsgKind::Info);
        }
        section(ui, "Bauteile");
        let s = self.pcb.cur();
        let mut list: Vec<(usize, String, String, String)> = s.symbols.iter().enumerate().filter(|(i, _)| !s.is_power(*i)).map(|(i, x)| (i, s.shown_ref(i), x.value.clone(), x.footprint.rsplit(':').next().unwrap_or("").to_string())).collect();
        list.sort_by(|a, b| crate::pcb::schematic::natural(&a.1).cmp(&crate::pcb::schematic::natural(&b.1)));
        let mut go = None;
        for (i, r, v, fp) in list {
            if row(ui, &format!("{:<5} {}", r, v), &fp, self.pcb.sel == Some(PSel::Sym(i)), None).clicked() {
                go = Some(i);
            }
        }
        if let Some(i) = go {
            self.pcb.sel = Some(PSel::Sym(i));
            self.pcb.sch_cam.c = self.pcb.cur().symbols[i].pos;
        }
    }

    fn pcb_board_panel(&mut self, ui: &mut egui::Ui) {
        section(ui, "Lagen");
        let layers: Vec<u8> = if self.pcb.project.board.layers >= 4 { vec![F_CU, 2, 3, B_CU] } else { vec![F_CU, B_CU] };
        for l in layers {
            ui.horizontal(|ui| {
                let mut on = self.pcb.vis.cu[l as usize];
                if dlgkit::chk(ui, &mut on, "", true) {
                    self.pcb.vis.cu[l as usize] = on;
                }
                let act = self.pcb.layer == l;
                if row(ui, layer_title(l), if act { "aktiv" } else { "" }, act, Some(layer_color(l))).clicked() {
                    self.pcb.layer = l;
                }
            });
        }
        for (label, chip, which) in [("Bestückungsdruck", SILK, 0), ("Platinenrand (Edge.Cuts)", EDGE, 1), ("Luftlinien", RATS, 2), ("Zonen", Color32::from_rgb(180, 120, 120), 3), ("Sperrflächen", Color32::from_rgb(220, 60, 220), 4)] {
            ui.horizontal(|ui| {
                let v = &mut self.pcb.vis;
                let flag = match which {
                    0 => &mut v.silk,
                    1 => &mut v.edge,
                    2 => &mut v.rats,
                    3 => &mut v.zones,
                    _ => &mut v.court,
                };
                dlgkit::chk(ui, flag, "", true);
                let on = *flag;
                if row(ui, label, "", false, Some(chip)).clicked() {
                    *flag = !on;
                    self.pcb.zone_mesh = None;
                }
            });
        }
        // statistics
        let unrouted = self.pcb.ratsnest().len();
        let b = &self.pcb.project.board;
        section(ui, "Platine");
        let size = b.size().map(|(lo, hi)| format!("{} × {} mm", crate::pcb::geom::mm(hi.x - lo.x), crate::pcb::geom::mm(hi.y - lo.y))).unwrap_or_else(|| "kein Umriss".into());
        for (k, v) in [
            ("Größe", size),
            ("Kupferlagen", format!("{}", b.layers)),
            ("Dicke", format!("{} mm", crate::pcb::geom::mm(b.thickness))),
            ("Bauteile", format!("{}", b.footprints.len())),
            ("Leiterbahnen", format!("{} ({} mm)", b.tracks.len(), crate::pcb::geom::mm((b.tracks.iter().map(|t| (t.a - t.b).length()).sum::<f64>() * 10.0).round() / 10.0))),
            ("Vias", format!("{}", b.vias.len())),
            ("Nicht verbunden", format!("{}", unrouted)),
        ] {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(k).size(13.0).color(Color32::from_rgb(110, 116, 124)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(v).size(13.0).color(if k == "Nicht verbunden" && unrouted > 0 { ERR } else { TEXT }));
                });
            });
        }
        // selection
        if let Some(sel) = self.pcb.sel {
            section(ui, "Auswahl");
            let b = &self.pcb.project.board;
            let lines: Vec<(String, String)> = match sel {
                PSel::Fp(i) if i < b.footprints.len() => {
                    let f = &b.footprints[i];
                    let mut v = vec![
                        ("Referenz".into(), f.reference.clone()),
                        ("Wert".into(), f.value.clone()),
                        ("Footprint".into(), f.fp.short().to_string()),
                        ("Seite".into(), if f.bottom { "Unten".into() } else { "Oben".into() }),
                        ("Position".into(), format!("{} / {} mm", crate::pcb::geom::mm(f.pos.x), crate::pcb::geom::mm(f.pos.y))),
                        ("Drehung".into(), format!("{}°", f.rot)),
                        ("Typ".into(), if f.fp.smd() { "SMD".into() } else { "THT".into() }),
                    ];
                    if let Some(port) = &f.fp.port {
                        v.push(("Gehäuseöffnung".into(), format!("Ports ▸ {}", port)));
                    }
                    v
                }
                PSel::Track(i) if i < b.tracks.len() => {
                    let t = &b.tracks[i];
                    vec![("Netz".into(), t.net.clone()), ("Lage".into(), layer_title(t.layer).into()), ("Breite".into(), format!("{} mm", crate::pcb::geom::mm(t.width))), ("Länge".into(), format!("{} mm", crate::pcb::geom::mm(((t.a - t.b).length() * 100.0).round() / 100.0)))]
                }
                PSel::Via(i) if i < b.vias.len() => {
                    let v = &b.vias[i];
                    vec![("Netz".into(), v.net.clone()), ("Durchmesser".into(), format!("{} mm", crate::pcb::geom::mm(v.dia))), ("Bohrung".into(), format!("{} mm", crate::pcb::geom::mm(v.drill)))]
                }
                PSel::Zone(i) if i < b.zones.len() => {
                    let z = &b.zones[i];
                    vec![("Netz".into(), z.net.clone()), ("Lage".into(), layer_title(z.layer).into()), ("Abstand".into(), format!("{} mm", crate::pcb::geom::mm(z.clearance))), ("Gefüllt".into(), format!("{:.0} mm²", crate::slicer::poly::area(&z.fill)))]
                }
                _ => vec![],
            };
            for (k, v) in lines {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(k).size(13.0).color(Color32::from_rgb(110, 116, 124)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(v).size(13.0).color(TEXT));
                    });
                });
            }
        }
        section(ui, "Netze");
        let nets = self.pcb.project.board.nets();
        let mut hl = None;
        for n in nets {
            let pads = self.pcb.project.board.footprints.iter().flat_map(|f| f.nets.iter()).filter(|x| **x == n).count();
            if row(ui, &n, &format!("{} Pads", pads), self.pcb.hl_net.as_deref() == Some(&n), None).on_hover_text("Netz hervorheben").clicked() {
                hl = Some(n);
            }
        }
        if let Some(n) = hl {
            self.pcb.hl_net = if self.pcb.hl_net.as_deref() == Some(&n) { None } else { Some(n) };
        }
    }

    // ------------------------------------------------------------------ viewport

    pub fn pcb_viewport(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        super::ui::graphics_background(ui.painter(), rect);
        let painter = ui.painter().with_clip_rect(rect);
        let sch = self.pcb.view == PcbView::Schematic;
        // fit on demand (also the graphics toolbar's "Neu einpassen")
        if self.pcb.fit || self.fit_pending || (!sch && !self.pcb.brd_cam.init) || (sch && !self.pcb.sch_cam.init) {
            if sch {
                // the parts with some room around them (the whole sheet when it is empty)
                let (lo, hi) = self.pcb.cur().bounds().map(|(lo, hi)| (lo - P::splat(12.0), hi + P::splat(12.0))).unwrap_or((P::ZERO, p(297.0, 210.0)));
                self.pcb.sch_cam.fit(rect, lo, hi);
            } else {
                let b = &self.pcb.project.board;
                let mut pts: Vec<P> = b.outline.iter().flatten().copied().collect();
                pts.extend(b.footprints.iter().flat_map(|f| f.courtyard()));
                let (lo, hi) = crate::pcb::geom::bbox(pts).unwrap_or((p(90.0, 90.0), p(160.0, 140.0)));
                self.pcb.brd_cam.fit(rect, lo - P::splat(3.0), hi + P::splat(3.0));
            }
            self.pcb.fit = false;
            self.fit_pending = false;
        }
        // navigation: wheel zooms at the cursor, middle or right drag pans
        let (scroll, zoom, mods, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers, i.pointer.clone()));
        let cam = if sch { &mut self.pcb.sch_cam } else { &mut self.pcb.brd_cam };
        if let Some(hp) = resp.hover_pos() {
            let before = cam.world(rect, hp);
            let mut k = zoom;
            if scroll.y != 0.0 && !mods.shift {
                k *= (scroll.y * 0.0025).exp();
            }
            if k != 1.0 {
                cam.s = (cam.s * k).clamp(0.2, 400.0);
                let after = cam.world(rect, hp);
                cam.c += before - after;
            }
            if mods.shift && (scroll.x != 0.0 || scroll.y != 0.0) {
                cam.c -= p((scroll.x / cam.s) as f64, (scroll.y / cam.s) as f64);
            }
        }
        if resp.dragged() && (pointer.middle_down() || pointer.secondary_down()) {
            let d = pointer.delta();
            cam.c -= p((d.x / cam.s) as f64, (d.y / cam.s) as f64);
        }
        let cam = *cam;
        let hover = resp.hover_pos();
        if let Some(h) = hover {
            self.pcb.mouse = cam.world(rect, h);
        }
        if sch {
            self.pcb_sch_input(&resp, rect, &cam);
            self.pcb_draw_sch(&painter, rect, &cam);
        } else {
            self.pcb_brd_input(&resp, rect, &cam);
            self.pcb_draw_brd(&painter, rect, &cam);
        }
        // cursor read-out
        let m = self.pcb.mouse;
        let info = format!("{}X {}  Y {} mm   Raster {} mm   {}", if sch { format!("Blatt {}   ", self.pcb.sheet_path_name()) } else { String::new() }, crate::pcb::geom::mm((m.x * 100.0).round() / 100.0), crate::pcb::geom::mm((m.y * 100.0).round() / 100.0), if sch { "1,27" } else { "0,25" }, if sch { "" } else { layer_title(self.pcb.layer) });
        let at = pos2(rect.left() + 10.0, rect.bottom() - 12.0);
        painter.text(at + vec2(1.0, 1.0), Align2::LEFT_CENTER, &info, FontId::proportional(13.0), Color32::from_black_alpha(120));
        painter.text(at, Align2::LEFT_CENTER, &info, FontId::proportional(13.0), Color32::WHITE);
    }

    // ---------------------------------------------------------------- schematic input

    fn sch_hit(&self, q: P, tol: f64) -> Option<PSel> {
        let s = self.pcb.cur();
        for (i, l) in s.labels.iter().enumerate() {
            let w = l.text.len() as f64 * 1.0 + 1.0;
            if (q.x - l.pos.x) > -0.8 && (q.x - l.pos.x) < w && (q.y - l.pos.y).abs() < 1.5 {
                return Some(PSel::Label(i));
            }
        }
        for (i, n) in s.no_connects.iter().enumerate() {
            if (*n - q).length() < 1.0 {
                return Some(PSel::Nc(i));
            }
        }
        let mut best: Option<(f64, PSel)> = None;
        for (i, w) in s.wires.iter().enumerate() {
            let d = pt_seg(q, w.a, w.b);
            if d < tol && best.is_none_or(|b| d < b.0) {
                best = Some((d, PSel::Wire(i)));
            }
        }
        if best.is_some() {
            return best.map(|b| b.1);
        }
        let mut found: Option<(f64, usize)> = None;
        for (i, inst) in s.symbols.iter().enumerate() {
            let Some(d) = s.def(&inst.lib) else { continue };
            let (lo, hi) = d.unit_bounds(inst.unit);
            let pts: Vec<P> = [lo, p(hi.x, lo.y), hi, p(lo.x, hi.y)].iter().map(|c| Schematic::xform(inst, *c)).collect();
            let (a, b) = crate::pcb::geom::bbox(pts).unwrap();
            let (a, b) = (a - P::splat(0.6), b + P::splat(0.6));
            if q.x >= a.x && q.x <= b.x && q.y >= a.y && q.y <= b.y {
                let area = (b.x - a.x) * (b.y - a.y);
                if found.is_none_or(|f| area < f.0) {
                    found = Some((area, i));
                }
            }
        }
        if let Some(f) = found {
            return Some(PSel::Sym(f.1));
        }
        for (i, w) in s.bus_entries.iter().enumerate() {
            if pt_seg(q, w.a, w.b) < tol {
                return Some(PSel::Entry(i));
            }
        }
        for (i, w) in s.buses.iter().enumerate() {
            if pt_seg(q, w.a, w.b) < tol {
                return Some(PSel::Bus(i));
            }
        }
        s.sheets.iter().position(|sh| sh.contains(q)).map(PSel::Sheet)
    }

    pub fn pcb_enter_sheet(&mut self, i: usize) {
        let Some(name) = self.pcb.cur().sheets.get(i).map(|s| s.name.clone()) else { return };
        self.pcb.path.push(i);
        self.pcb.sel = None;
        self.pcb.tool = PTool::Select;
        self.pcb.sch_cam.init = false;
        self.say(format!("Blatt {} geöffnet – „Übergeordnetes Blatt“ führt zurück.", name), MsgKind::Info);
    }

    pub fn pcb_leave_sheet(&mut self) {
        if self.pcb.path.pop().is_some() {
            self.pcb.sel = None;
            self.pcb.tool = PTool::Select;
            self.pcb.sch_cam.init = false;
        }
    }

    /// Places the next unit of the selected multi-unit part next to it.
    pub fn pcb_add_unit(&mut self) {
        let Some(PSel::Sym(i)) = self.pcb.sel else { return };
        let pos = self.pcb.cur().symbols[i].pos + p(25.4, 0.0);
        self.pcb_push();
        match self.pcb.cur_mut().add_next_unit(i, pos) {
            Some(k) => {
                let r = self.pcb.cur().shown_ref(k);
                self.pcb.sel = Some(PSel::Sym(k));
                self.say(format!("Einheit {} platziert.", r), MsgKind::Info);
            }
            None => {
                self.pcb.undo.pop();
                self.say("Alle Einheiten dieses Bauteils sind schon platziert.", MsgKind::Info);
            }
        }
    }

    /// True when `q` is a connection point (pin end, wire end or a point on a wire).
    fn sch_is_conn(&self, q: P) -> bool {
        let s = self.pcb.cur();
        (0..s.symbols.len()).any(|i| s.pin_positions(i).iter().any(|(pp, _)| (*pp - q).length() < 1e-3)) || s.wires.iter().any(|w| pt_seg(q, w.a, w.b) < 1e-3)
    }

    fn pcb_sch_input(&mut self, resp: &egui::Response, rect: Rect, cam: &Cam2) {
        let m = self.pcb.mouse;
        let g = 1.27;
        let ms = snap(m, g);
        let primary_click = resp.clicked_by(egui::PointerButton::Primary);
        let dbl = resp.double_clicked_by(egui::PointerButton::Primary);
        let tol = (6.0 / cam.s as f64).max(0.4);
        match self.pcb.tool.clone() {
            PTool::Select => {
                if dbl {
                    match self.sch_hit(m, tol) {
                        Some(PSel::Sym(i)) => {
                            self.pcb.sel = Some(PSel::Sym(i));
                            self.pcb_edit_props();
                        }
                        Some(PSel::Sheet(i)) => self.pcb_enter_sheet(i),
                        _ => {}
                    }
                    return;
                }
                if resp.drag_started_by(egui::PointerButton::Primary) {
                    let start = resp.interact_pointer_pos().map(|pp| cam.world(rect, pp)).unwrap_or(m);
                    if let Some(sel) = self.sch_hit(start, tol) {
                        self.pcb.sel = Some(sel);
                        let mut rubber = vec![];
                        if let PSel::Sym(i) = sel {
                            let pins: Vec<P> = self.pcb.cur().pin_positions(i).into_iter().map(|x| x.0).collect();
                            for (wi, w) in self.pcb.cur().wires.iter().enumerate() {
                                if pins.iter().any(|pp| (*pp - w.a).length() < 1e-3) {
                                    rubber.push((wi, false));
                                }
                                if pins.iter().any(|pp| (*pp - w.b).length() < 1e-3) {
                                    rubber.push((wi, true));
                                }
                            }
                        }
                        self.pcb.drag = Some(Drag { start: snap(start, g), sel, orig: self.pcb.project.clone(), rubber });
                    }
                }
                if let Some(d) = self.pcb.drag.clone() {
                    if resp.dragged_by(egui::PointerButton::Primary) {
                        let delta = ms - d.start;
                        let mut pr = d.orig.clone();
                        let Some(s) = pr.schematic.sheet_at_mut(&self.pcb.path) else { return };
                        match d.sel {
                            PSel::Sym(i) => {
                                s.symbols[i].pos += delta;
                                for (wi, end_b) in &d.rubber {
                                    if *end_b {
                                        s.wires[*wi].b += delta;
                                    } else {
                                        s.wires[*wi].a += delta;
                                    }
                                }
                            }
                            PSel::Wire(i) => {
                                s.wires[i].a += delta;
                                s.wires[i].b += delta;
                            }
                            PSel::Label(i) => s.labels[i].pos += delta,
                            PSel::Nc(i) => s.no_connects[i] += delta,
                            PSel::Sheet(i) => {
                                s.sheets[i].pos += delta;
                                for q in &mut s.sheets[i].pins {
                                    q.pos += delta;
                                }
                            }
                            PSel::Bus(i) => {
                                s.buses[i].a += delta;
                                s.buses[i].b += delta;
                            }
                            PSel::Entry(i) => {
                                s.bus_entries[i].a += delta;
                                s.bus_entries[i].b += delta;
                            }
                            _ => {}
                        }
                        self.pcb.project = pr;
                    }
                    if resp.drag_stopped() {
                        let moved = self.pcb.project != d.orig;
                        if moved {
                            self.pcb.undo.push(d.orig);
                            self.pcb.redo.clear();
                            self.pcb.changed();
                        }
                        self.pcb.drag = None;
                    }
                    return;
                }
                if primary_click {
                    self.pcb.sel = self.sch_hit(m, tol);
                }
            }
            PTool::Place(id) => {
                if primary_click {
                    self.pcb_push();
                    let (rot, mir) = (self.pcb.place_rot, self.pcb.place_mirror);
                    // a second click on the same spot (double click) does not stack a copy
                    if self.pcb.cur().symbols.iter().any(|x| x.lib == id && (x.pos - ms).length() < 1e-6) {
                        return;
                    }
                    // symbols from loaded libraries travel with the schematic
                    if !symbols::library().iter().any(|x| x.id == id) {
                        if let Some(d) = symbols::find(&id) {
                            let root = &mut self.pcb.project.schematic;
                            if !root.local_lib.iter().any(|x| x.id == id) {
                                root.local_lib.push(d.clone());
                            }
                            let cur = self.pcb.cur_mut();
                            if !cur.local_lib.iter().any(|x| x.id == id) {
                                cur.local_lib.push(d.clone());
                            }
                        }
                    }
                    let s = self.pcb.cur_mut();
                    if let Some(i) = s.add_symbol(&id, ms) {
                        s.symbols[i].rot = rot;
                        s.symbols[i].mirror = mir;
                        self.pcb.project.schematic.annotate(false);
                        let r = self.pcb.cur().shown_ref(i);
                        self.pcb.sel = Some(PSel::Sym(i));
                        self.say(format!("{} platziert.", r), MsgKind::Info);
                    }
                }
            }
            PTool::Power(k) => {
                if primary_click {
                    self.pcb_push();
                    let s = self.pcb.cur_mut();
                    s.add_symbol(&format!("power:{}", k), ms);
                    s.annotate(false);
                }
            }
            PTool::Sheet => {
                if primary_click {
                    let n = self.pcb.cur().sheets.len() + 1;
                    self.pcb.sheet_dlg = Some((ms, format!("Blatt{}", n)));
                }
            }
            PTool::SheetPin => {
                if primary_click {
                    match self.pcb.cur().sheets.iter().position(|sh| sh.contains(m) || (sh.border_point(m) - m).length() < 2.0) {
                        Some(k) => {
                            let at = self.pcb.cur().sheets[k].border_point(m);
                            let n = self.pcb.cur().sheets[k].pins.len() + 1;
                            self.pcb.sheet_pin_dlg = Some((k, at, format!("SIG{}", n)));
                        }
                        None => self.say("Blattpin: auf den Rand eines hierarchischen Blatts klicken.", MsgKind::Warning),
                    }
                }
            }
            PTool::Bus => {
                if dbl {
                    self.pcb.wire.clear();
                    return;
                }
                if primary_click {
                    if let Some(&last) = self.pcb.wire.last() {
                        let corner = if (ms.x - last.x).abs() >= (ms.y - last.y).abs() { p(ms.x, last.y) } else { p(last.x, ms.y) };
                        self.pcb_push();
                        let s = self.pcb.cur_mut();
                        for (a, b) in [(last, corner), (corner, ms)] {
                            if (a - b).length() > 1e-6 {
                                s.buses.push(crate::pcb::schematic::Wire { a, b });
                            }
                        }
                    }
                    self.pcb.wire.push(ms);
                }
            }
            PTool::BusEntry => {
                if primary_click {
                    self.pcb_push();
                    let s = self.pcb.cur_mut();
                    // the entry leaves the bus at 45° towards the cursor side
                    let on_bus = s.buses.iter().find(|b| pt_seg(ms, b.a, b.b) < 0.1).copied();
                    let dir = match on_bus {
                        Some(b) if (b.a.x - b.b.x).abs() < 1e-6 => p(if m.x >= ms.x { 2.54 } else { -2.54 }, 2.54),
                        _ => p(2.54, if m.y >= ms.y { 2.54 } else { -2.54 }),
                    };
                    s.bus_entries.push(crate::pcb::schematic::Wire { a: ms, b: ms + dir });
                }
            }
            PTool::NoConnect => {
                if primary_click {
                    self.pcb_push();
                    self.pcb.cur_mut().no_connects.push(ms);
                }
            }
            PTool::Label(kind) => {
                if primary_click {
                    let n = self.pcb.cur().labels.len() + 1;
                    self.pcb.label_dlg = Some((ms, format!("NETZ{}", n), kind));
                }
            }
            PTool::Wire => {
                if dbl {
                    self.pcb.wire.clear();
                    return;
                }
                if primary_click {
                    if let Some(&last) = self.pcb.wire.last() {
                        let corner = if (ms.x - last.x).abs() >= (ms.y - last.y).abs() { p(ms.x, last.y) } else { p(last.x, ms.y) };
                        self.pcb_push();
                        let s = self.pcb.cur_mut();
                        s.add_wire(last, corner);
                        s.add_wire(corner, ms);
                        let start = self.pcb.wire[0];
                        if (ms - start).length() > 1e-6 && self.sch_is_conn_except_new(ms) {
                            self.pcb.wire.clear();
                        } else {
                            self.pcb.wire.push(ms);
                        }
                    } else {
                        self.pcb.wire.push(ms);
                    }
                }
            }
            _ => {}
        }
    }

    fn sch_is_conn_except_new(&self, q: P) -> bool {
        let s = self.pcb.cur();
        let pins = (0..s.symbols.len()).any(|i| s.pin_positions(i).iter().any(|(pp, _)| (*pp - q).length() < 1e-3));
        // a wire end or wire other than the two just drawn
        let n = s.wires.len();
        let wires = s.wires.iter().take(n.saturating_sub(2)).any(|w| pt_seg(q, w.a, w.b) < 1e-3);
        let labels = s.labels.iter().any(|l| (l.pos - q).length() < 1e-3);
        let _ = self.sch_is_conn(q);
        pins || wires || labels
    }

    // ---------------------------------------------------------------- schematic drawing

    fn pcb_draw_sch(&mut self, pa: &egui::Painter, rect: Rect, cam: &Cam2) {
        let sc = |q: P| cam.scr(rect, q);
        // sheet A4 landscape with frame and title block (like a Creo drawing format)
        let (w, h) = (297.0, 210.0);
        let sheet = Rect::from_two_pos(sc(p(0.0, 0.0)), sc(p(w, h)));
        pa.rect_filled(sheet.translate(vec2(3.0, 3.0)), 0.0, Color32::from_black_alpha(50));
        pa.rect_filled(sheet, 0.0, SHEET);
        let lw = (0.35 * cam.s).max(1.0);
        pa.rect_stroke(Rect::from_two_pos(sc(p(10.0, 10.0)), sc(p(w - 10.0, h - 10.0))), 0.0, Stroke::new(lw, FRAME), StrokeKind::Middle);
        let tb = [p(w - 110.0, h - 10.0 - 28.0), p(w - 10.0, h - 10.0)];
        pa.rect_stroke(Rect::from_two_pos(sc(tb[0]), sc(tb[1])), 0.0, Stroke::new(lw, FRAME), StrokeKind::Middle);
        for y in [h - 10.0 - 21.0, h - 10.0 - 14.0, h - 10.0 - 7.0] {
            seg(pa, sc(p(w - 110.0, y)), sc(p(w - 10.0, y)), lw * 0.6, FRAME);
        }
        seg(pa, sc(p(w - 60.0, h - 10.0 - 14.0)), sc(p(w - 60.0, h - 10.0)), lw * 0.6, FRAME);
        // zone grid markers
        for i in 0..6 {
            let x = 10.0 + (w - 20.0) * (i as f64 + 0.5) / 6.0;
            let px = text_px(cam, 2.0);
            if px > 4.0 {
                pa.text(sc(p(x, 5.0)), Align2::CENTER_CENTER, format!("{}", i + 1), FontId::proportional(px), FRAME);
                pa.text(sc(p(x, h - 5.0)), Align2::CENTER_CENTER, format!("{}", i + 1), FontId::proportional(px), FRAME);
            }
        }
        for (i, c) in ["A", "B", "C", "D"].iter().enumerate() {
            let y = 10.0 + (h - 20.0) * (i as f64 + 0.5) / 4.0;
            let px = text_px(cam, 2.0);
            if px > 4.0 {
                pa.text(sc(p(5.0, y)), Align2::CENTER_CENTER, *c, FontId::proportional(px), FRAME);
                pa.text(sc(p(w - 5.0, y)), Align2::CENTER_CENTER, *c, FontId::proportional(px), FRAME);
            }
        }
        let px = text_px(cam, 1.8);
        if px > 3.0 {
            let title = if self.pcb.project.schematic.title.is_empty() { self.pcb.project.name.clone() } else { self.pcb.project.schematic.title.clone() };
            pa.text(sc(p(w - 108.0, h - 10.0 - 24.5)), Align2::LEFT_CENTER, format!("Titel: {}", title), FontId::proportional(px * 1.2), INK);
            pa.text(sc(p(w - 108.0, h - 10.0 - 17.5)), Align2::LEFT_CENTER, format!("Projekt: {}", self.pcb.project.name), FontId::proportional(px), INK);
            pa.text(sc(p(w - 108.0, h - 10.0 - 10.5)), Align2::LEFT_CENTER, "Format: A4", FontId::proportional(px), INK);
            pa.text(sc(p(w - 58.0, h - 10.0 - 10.5)), Align2::LEFT_CENTER, "Blatt: 1/1", FontId::proportional(px), INK);
            pa.text(sc(p(w - 108.0, h - 10.0 - 3.5)), Align2::LEFT_CENTER, "Freo Leiterplatte", FontId::proportional(px), INK);
            pa.text(sc(p(w - 58.0, h - 10.0 - 3.5)), Align2::LEFT_CENTER, format!("Rev. {}", env!("CARGO_PKG_VERSION")), FontId::proportional(px), INK);
        }
        // grid dots when zoomed in
        if cam.s > 6.0 {
            let lo = cam.world(rect, rect.min).max(P::ZERO);
            let hi = cam.world(rect, rect.max).min(p(w, h));
            let g = 2.54;
            let mut x = (lo.x / g).ceil() * g;
            while x <= hi.x {
                let mut y = (lo.y / g).ceil() * g;
                while y <= hi.y {
                    pa.circle_filled(sc(p(x, y)), 0.8, Color32::from_rgb(190, 194, 200));
                    y += g;
                }
                x += g;
            }
        }
        let s = self.pcb.cur();
        let hover = if self.pcb.tool == PTool::Select && self.pcb.drag.is_none() { self.sch_hit(self.pcb.mouse, (6.0 / cam.s as f64).max(0.4)) } else { None };
        let col_of = |sel: PSel, base: Color32| {
            if self.pcb.sel == Some(sel) {
                SEL_C
            } else if hover == Some(sel) {
                PRE_C
            } else {
                base
            }
        };
        let ww = (0.25 * cam.s).max(1.3);
        for (i, w) in s.wires.iter().enumerate() {
            seg(pa, sc(w.a), sc(w.b), ww, col_of(PSel::Wire(i), WIRE));
        }
        for j in s.junctions() {
            pa.circle_filled(sc(j), (0.5 * cam.s).max(2.0), WIRE);
        }
        for (i, b) in s.buses.iter().enumerate() {
            round_seg(pa, sc(b.a), sc(b.b), (0.6 * cam.s).max(2.5), col_of(PSel::Bus(i), BUS));
        }
        for (i, e) in s.bus_entries.iter().enumerate() {
            seg(pa, sc(e.a), sc(e.b), ww, col_of(PSel::Entry(i), WIRE));
        }
        for (i, sh) in s.sheets.iter().enumerate() {
            let r = Rect::from_two_pos(sc(sh.pos), sc(sh.pos + sh.size));
            let col = col_of(PSel::Sheet(i), Color32::from_rgb(132, 0, 132));
            pa.rect_filled(r, 0.0, Color32::from_rgb(255, 250, 236));
            pa.rect_stroke(r, 0.0, Stroke::new((0.3 * cam.s).max(1.5), col), StrokeKind::Middle);
            let px = text_px(cam, 1.27).max(7.0);
            pa.text(r.left_top() - vec2(0.0, 3.0), Align2::LEFT_BOTTOM, &sh.name, FontId::proportional(px), col);
            pa.text(r.left_bottom() + vec2(0.0, 3.0), Align2::LEFT_TOP, format!("Datei: {}", crate::pcb::kicad::sheet_file(&self.pcb.project.name, &sh.name)), FontId::proportional(px * 0.85), Color32::from_rgb(120, 110, 60));
            for pin in &sh.pins {
                let c = sc(pin.pos);
                let k = (0.8 * cam.s).max(4.0);
                pa.add(Shape::convex_polygon(vec![c + vec2(-k, -k * 0.6), c + vec2(k, -k * 0.6), c + vec2(k, k * 0.6), c + vec2(-k, k * 0.6)], Color32::from_rgb(255, 224, 160), Stroke::new(1.0_f32, col)));
                let inside_right = (pin.pos.x - sh.pos.x).abs() < 1e-6;
                let at = if inside_right { c + vec2(k + 3.0, 0.0) } else { c - vec2(k + 3.0, 0.0) };
                pa.text(at, if inside_right { Align2::LEFT_CENTER } else { Align2::RIGHT_CENTER }, &pin.name, FontId::proportional(px), col);
            }
            if sh.sch.symbols.is_empty() && sh.sch.wires.is_empty() {
                pa.text(r.center(), Align2::CENTER_CENTER, "leer – Doppelklick öffnet", FontId::proportional(px), Color32::from_rgb(150, 140, 120));
            } else {
                let parts = sh.sch.all_symbols().iter().filter(|x| !x.reference.starts_with('#')).count();
                pa.text(r.center(), Align2::CENTER_CENTER, format!("{} Bauteile", parts), FontId::proportional(px), Color32::from_rgb(150, 140, 120));
            }
        }
        for (i, inst) in s.symbols.iter().enumerate() {
            let Some(d) = s.def(&inst.lib) else { continue };
            let col = col_of(PSel::Sym(i), SYM);
            let xf = |q: P| Schematic::xform(inst, q);
            let df = |a: f64| Schematic::xform_dir(inst, a);
            draw_symbol(pa, cam, rect, d, inst.unit, &xf, &df, col, SYM_FILL, 1.0);
            // reference and value next to the body
            let (lo, hi) = d.unit_bounds(inst.unit);
            let pts: Vec<P> = [lo, p(hi.x, lo.y), hi, p(lo.x, hi.y)].iter().map(|c| xf(*c)).collect();
            let (a, b) = crate::pcb::geom::bbox(pts).unwrap();
            let px = text_px(cam, 1.27);
            if px > 3.0 {
                if d.power {
                    let up = inst.rot.rem_euclid(360.0) == 180.0;
                    let below = d.pins.first().is_some_and(|q| q.dir == 270.0) != up;
                    let at = if below { p((a.x + b.x) / 2.0, b.y + 1.4) } else { p((a.x + b.x) / 2.0, a.y - 1.4) };
                    pa.text(sc(at), Align2::CENTER_CENTER, &inst.value, FontId::proportional(px), col);
                } else {
                    let at = p(b.x + 0.8, (a.y + b.y) / 2.0);
                    pa.text(sc(at) - vec2(0.0, px * 0.55), Align2::LEFT_CENTER, s.shown_ref(i), FontId::proportional(px), if col == SYM { Color32::from_rgb(0, 110, 120) } else { col });
                    pa.text(sc(at) + vec2(0.0, px * 0.55), Align2::LEFT_CENTER, &inst.value, FontId::proportional(px), if col == SYM { INK } else { col });
                }
            }
        }
        for (i, l) in s.labels.iter().enumerate() {
            let col = col_of(PSel::Label(i), INK);
            let px = text_px(cam, 1.27).max(6.0);
            let at = sc(l.pos);
            match l.kind {
                LabelKind::Local => {
                    pa.text(at + vec2(2.0, -2.0), Align2::LEFT_BOTTOM, &l.text, FontId::proportional(px), col);
                }
                LabelKind::Global | LabelKind::Hier => {
                    let gw = pa.layout_no_wrap(l.text.clone(), FontId::proportional(px), col).size().x;
                    let hh = px * 0.7;
                    let pts = vec![at, at + vec2(hh, -hh), at + vec2(hh * 1.6 + gw, -hh), at + vec2(hh * 1.6 + gw, hh), at + vec2(hh, hh)];
                    pa.add(Shape::closed_line(pts, Stroke::new(1.2_f32, col)));
                    pa.text(at + vec2(hh * 1.3, 0.0), Align2::LEFT_CENTER, &l.text, FontId::proportional(px), col);
                }
            }
            pa.circle_filled(at, 1.5, col);
        }
        for (i, n) in s.no_connects.iter().enumerate() {
            let c = sc(*n);
            let k = (0.8 * cam.s).max(4.0);
            let col = col_of(PSel::Nc(i), NC);
            seg(pa, c + vec2(-k, -k), c + vec2(k, k), 1.6, col);
            seg(pa, c + vec2(-k, k), c + vec2(k, -k), 1.6, col);
        }
        // ERC markers
        if let Some(v) = &self.pcb.erc {
            for e in v {
                let c = sc(e.pos);
                let col = if e.severity == Severity::Error { ERR } else { WARN };
                pa.add(Shape::convex_polygon(vec![c, c + vec2(-6.0, -14.0), c + vec2(6.0, -14.0)], col, Stroke::new(1.0_f32, Color32::WHITE)));
                pa.text(c + vec2(0.0, -9.5), Align2::CENTER_CENTER, "!", FontId::proportional(10.0), Color32::WHITE);
            }
        }
        // tool previews
        let ms = snap(self.pcb.mouse, 1.27);
        match &self.pcb.tool {
            PTool::Place(id) => {
                if let Some(d) = s.def(id) {
                    let ghost = crate::pcb::schematic::SymInst { lib: id.clone(), reference: String::new(), value: String::new(), footprint: String::new(), pos: ms, rot: self.pcb.place_rot, mirror: self.pcb.place_mirror, fields: vec![], unit: 1 };
                    draw_symbol(pa, cam, rect, d, 1, &|q| Schematic::xform(&ghost, q), &|a| Schematic::xform_dir(&ghost, a), SYM, SYM_FILL, 0.55);
                }
            }
            PTool::Power(k) => {
                if let Some(d) = s.def(&format!("power:{}", k)) {
                    draw_symbol(pa, cam, rect, d, 1, &|q| q + ms, &|a| a, SYM, SYM_FILL, 0.55);
                }
            }
            PTool::Wire | PTool::Bus => {
                if let Some(&last) = self.pcb.wire.last() {
                    let corner = if (ms.x - last.x).abs() >= (ms.y - last.y).abs() { p(ms.x, last.y) } else { p(last.x, ms.y) };
                    seg(pa, sc(last), sc(corner), ww, fade(WIRE, 0.7));
                    seg(pa, sc(corner), sc(ms), ww, fade(WIRE, 0.7));
                }
                let c = sc(ms);
                seg(pa, c - vec2(6.0, 0.0), c + vec2(6.0, 0.0), 1.0, WIRE);
                seg(pa, c - vec2(0.0, 6.0), c + vec2(0.0, 6.0), 1.0, WIRE);
            }
            PTool::Label(_) | PTool::NoConnect => {
                let c = sc(ms);
                pa.circle_stroke(c, 4.0, Stroke::new(1.2_f32, NC));
            }
            _ => {}
        }
    }

    // ---------------------------------------------------------------- board input

    fn brd_hit(&self, q: P) -> Option<PSel> {
        let b = &self.pcb.project.board;
        let tol = 0.15;
        let vis = |l: u8| self.pcb.vis.cu[l as usize];
        for (i, v) in b.vias.iter().enumerate() {
            if (v.pos - q).length() <= v.dia / 2.0 + tol {
                return Some(PSel::Via(i));
            }
        }
        let mut best: Option<(f64, usize)> = None;
        for pass in 0..2 {
            for (i, t) in b.tracks.iter().enumerate() {
                if !vis(t.layer) || (pass == 0 && t.layer != self.pcb.layer) {
                    continue;
                }
                let d = pt_seg(q, t.a, t.b);
                if d <= t.width / 2.0 + tol && best.is_none_or(|bb| d < bb.0) {
                    best = Some((d, i));
                }
            }
            if best.is_some() {
                break;
            }
        }
        if let Some((_, i)) = best {
            return Some(PSel::Track(i));
        }
        let mut fp: Option<(f64, usize)> = None;
        for (i, f) in b.footprints.iter().enumerate() {
            if crate::pcb::geom::in_poly(q, &f.courtyard()) {
                let (lo, hi) = f.fp.court;
                let area = (hi.x - lo.x) * (hi.y - lo.y) + if f.bottom == (self.pcb.layer == B_CU) { 0.0 } else { 1e4 };
                if fp.is_none_or(|x| area < x.0) {
                    fp = Some((area, i));
                }
            }
        }
        if let Some((_, i)) = fp {
            return Some(PSel::Fp(i));
        }
        for (i, z) in b.zones.iter().enumerate() {
            let n = z.outline.len();
            if (0..n).any(|k| pt_seg(q, z.outline[k], z.outline[(k + 1) % n]) < 0.4) {
                return Some(PSel::Zone(i));
            }
        }
        None
    }

    /// Copper of a net under `q` usable as a route start/end on `layer`: (net, snap point).
    fn brd_copper_at(&self, q: P, layer: u8) -> Option<(String, P)> {
        let b = &self.pcb.project.board;
        for f in &b.footprints {
            for k in 0..f.fp.pads.len() {
                if f.pad_layers(k, b.layers) & (1 << layer) != 0 && f.pad_hull(k).contains(q) {
                    return Some((f.nets[k].clone(), f.pad_pos(k)));
                }
            }
        }
        for v in &b.vias {
            if (v.pos - q).length() <= v.dia / 2.0 {
                return Some((v.net.clone(), v.pos));
            }
        }
        for t in &b.tracks {
            if t.layer == layer && pt_seg(q, t.a, t.b) <= t.width / 2.0 {
                let c = crate::pcb::geom::closest_on_seg(q, t.a, t.b);
                let c = if (c - t.a).length() < 0.3 {
                    t.a
                } else if (c - t.b).length() < 0.3 {
                    t.b
                } else {
                    c
                };
                return Some((t.net.clone(), c));
            }
        }
        None
    }

    /// The 45° route preview from the last point to `m`.
    fn route_preview(&self, r: &Route, m: P) -> Vec<P> {
        let last = *r.pts.last().unwrap();
        let d = m - last;
        let (ax, ay) = (d.x.abs(), d.y.abs());
        let diag = ax.min(ay);
        let sx = d.x.signum();
        let sy = d.y.signum();
        let corner = if self.pcb.diagonal_first {
            last + p(sx * diag, sy * diag)
        } else if ax > ay {
            last + p(sx * (ax - diag), 0.0)
        } else {
            last + p(0.0, sy * (ay - diag))
        };
        let mut v = vec![last];
        if (corner - last).length() > 1e-6 && (corner - m).length() > 1e-6 {
            v.push(corner);
        }
        v.push(m);
        v
    }

    /// Clearance problem of a planned track segment (other-net copper too close).
    fn route_conflict(&self, a: P, b: P, width: f64, layer: u8, net: &str) -> Option<String> {
        let brd = &self.pcb.project.board;
        let h = Hull::capsule(a, b, width / 2.0);
        for c in brd.copper() {
            if c.net == net && !net.is_empty() || c.layers & (1 << layer) == 0 {
                continue;
            }
            if h.gap(&c.hull) < brd.rules.clearance_between(net, &c.net) - 1e-6 {
                return Some(match c.what {
                    crate::pcb::board::Item::Pad(f, k) => format!("Pad {} von {}", brd.footprints[f].fp.pads[k].num, brd.footprints[f].reference),
                    crate::pcb::board::Item::Track(_) => format!("Leiterbahn {}", c.net),
                    crate::pcb::board::Item::Via(_) => format!("Via {}", c.net),
                });
            }
        }
        None
    }

    fn route_target(&self, m: P, net: &str, layer: u8) -> P {
        // snap the end onto a pad or via of the net under the cursor
        match self.brd_copper_at(m, layer) {
            Some((n, q)) if n == net => q,
            _ => snap(m, 0.05),
        }
    }

    fn pcb_route_commit(&mut self, m: P) -> bool {
        let Some(r) = self.pcb.route.clone() else { return false };
        if r.pair.is_some() {
            return self.pcb_pair_commit(m);
        }
        let target = self.route_target(m, &r.net, r.layer);
        let mut pts = self.route_preview(&r, target);
        let conflict = pts.windows(2).find_map(|w| self.route_conflict(w[0], w[1], r.width, r.layer, &r.net));
        let mut shoved = false;
        if let Some(what) = conflict {
            match self.pcb.router_mode {
                crate::pcb::route::Mode::Highlight => {
                    self.say(format!("Leiterbahn zu nah an {} – anders führen (oder Modus „Ausweichen“/„Schieben“).", what), MsgKind::Warning);
                    return false;
                }
                crate::pcb::route::Mode::Walkaround => match crate::pcb::autoroute::find_path(&self.pcb.project.board, &r.net, r.layer, *r.pts.last().unwrap(), target, r.width) {
                    Some(path) => pts = path,
                    None => {
                        self.say(format!("Kein Weg um {} herum auf {}.", what, layer_title(r.layer)), MsgKind::Warning);
                        return false;
                    }
                },
                crate::pcb::route::Mode::Shove => {
                    let new: Vec<Track> = pts.windows(2).map(|w| Track { a: w[0], b: w[1], width: r.width, layer: r.layer, net: r.net.clone() }).collect();
                    let mut trial = self.pcb.project.board.clone();
                    match crate::pcb::route::shove(&mut trial, &new) {
                        Ok(n) => {
                            self.pcb_push();
                            self.pcb.project.board = trial;
                            shoved = true;
                            self.say(format!("{} Leiterbahn(en) beiseitegeschoben.", n), MsgKind::Info);
                        }
                        Err(e) => {
                            self.say(format!("Schieben nicht möglich: {}", e), MsgKind::Warning);
                            return false;
                        }
                    }
                }
            }
        }
        if !shoved {
            self.pcb_push();
        }
        for w in pts.windows(2) {
            if (w[0] - w[1]).length() > 1e-6 {
                self.pcb.project.board.tracks.push(Track { a: w[0], b: w[1], width: r.width, layer: r.layer, net: r.net.clone() });
            }
        }
        if let Some(rr) = &mut self.pcb.route {
            rr.pts.push(target);
        }
        self.pcb.changed();
        // reaching copper of the same net that was not the start finishes the track
        let ends_on = self.brd_copper_at(target, r.layer).is_some_and(|(n, _)| n == r.net) && (target - r.pts[0]).length() > 1e-6;
        if ends_on {
            let tracks = self.pcb.project.board.tracks.len();
            let _ = tracks;
            self.pcb.route = None;
            let left = self.pcb.ratsnest().iter().filter(|x| x.2 == r.net).count();
            self.say(format!("Leiterbahn {} verlegt{}.", r.net, if left == 0 { " – Netz vollständig verbunden".to_string() } else { format!(", noch {} offen", left) }), MsgKind::Info);
        }
        true
    }

    /// Differential pair: both tracks follow the 45° centre line at the class gap.
    fn pcb_pair_commit(&mut self, m: P) -> bool {
        let Some(r) = self.pcb.route.clone() else { return false };
        let Some((partner, ea, eb, gap)) = r.pair.clone() else { return false };
        let center = self.route_preview(&r, snap(m, 0.05));
        let (pa, pb) = crate::pcb::route::diff_pair(&center, r.width, gap);
        // which offset line belongs to which net: the one closer to its current end
        let swap = (pa[0] - ea).length() + (pb[0] - eb).length() > (pa[0] - eb).length() + (pb[0] - ea).length();
        let (la, lb) = if swap { (pb, pa) } else { (pa, pb) };
        let mut segs: Vec<Track> = vec![];
        for (line, end, net) in [(&la, ea, &r.net), (&lb, eb, &partner)] {
            let mut pts = vec![end];
            pts.extend(line.iter().skip(1).copied());
            if (pts[0] - line[0]).length() > 1e-6 && pts.len() > 1 {
                pts.insert(1, line[0]);
            }
            for w in pts.windows(2) {
                if (w[0] - w[1]).length() > 1e-6 {
                    segs.push(Track { a: w[0], b: w[1], width: r.width, layer: r.layer, net: net.clone() });
                }
            }
        }
        for s in &segs {
            if let Some(what) = self.route_conflict(s.a, s.b, s.width, s.layer, &s.net) {
                if !(what.contains(&r.net) || what.contains(&partner)) {
                    self.say(format!("Differenzielles Paar zu nah an {}.", what), MsgKind::Warning);
                    return false;
                }
            }
        }
        self.pcb_push();
        self.pcb.project.board.tracks.extend(segs);
        self.pcb.changed();
        if let Some(rr) = &mut self.pcb.route {
            rr.pts.push(*center.last().unwrap());
            rr.pair = Some((partner.clone(), *la.last().unwrap(), *lb.last().unwrap(), gap));
        }
        true
    }

    /// Starts a differential pair at a pad (or track end) whose net has a partner (X_P/X_N, D+/D-).
    fn pcb_pair_start(&mut self, m: P) {
        let Some((net, at)) = self.brd_copper_at(m, self.pcb.layer) else {
            return self.say("Differenzielles Paar an einem Pad des Paars beginnen.", MsgKind::Warning);
        };
        let Some(partner) = crate::pcb::board::diff_partner(&net).filter(|x| self.pcb.project.board.nets().contains(x)) else {
            return self.say(format!("Netz {} hat keinen Partner (Namen wie USB_D+/USB_D- oder X_P/X_N).", net), MsgKind::Warning);
        };
        // the partner's nearest pad
        let b = &self.pcb.project.board;
        let mut best: Option<(f64, P)> = None;
        for f in &b.footprints {
            for k in 0..f.fp.pads.len() {
                if f.nets[k] == partner {
                    let q = f.pad_pos(k);
                    let d = (q - at).length();
                    if best.is_none_or(|x| d < x.0) {
                        best = Some((d, q));
                    }
                }
            }
        }
        let Some((_, other)) = best else { return };
        let cr = b.rules.class_of(&net);
        self.pcb.hl_net = Some(net.clone());
        self.pcb.route = Some(Route { net, layer: self.pcb.layer, pts: vec![(at + other) / 2.0], width: cr.diff_width, pair: Some((partner, at, other, cr.diff_gap)) });
    }

    fn pcb_route_via(&mut self) {
        let m = self.pcb.mouse;
        if !self.pcb_route_commit(m) {
            return;
        }
        let Some(r) = self.pcb.route.clone() else { return };
        let at = *r.pts.last().unwrap();
        let rules = self.pcb.project.board.rules.clone();
        let other = if r.layer == F_CU { B_CU } else { F_CU };
        if let Some(what) = self.route_conflict(at, at, rules.via_dia, other, &r.net).or_else(|| self.route_conflict(at, at, rules.via_dia, r.layer, &r.net)) {
            return self.say(format!("Kein Platz für ein Via: {} zu nah.", what), MsgKind::Warning);
        }
        self.pcb.project.board.vias.push(Via { pos: at, dia: rules.via_dia, drill: rules.via_drill, net: r.net.clone() });
        self.pcb.changed();
        self.pcb.layer = other;
        if let Some(rr) = &mut self.pcb.route {
            rr.layer = other;
        }
    }

    fn pcb_brd_input(&mut self, resp: &egui::Response, rect: Rect, cam: &Cam2) {
        let m = self.pcb.mouse;
        let primary_click = resp.clicked_by(egui::PointerButton::Primary);
        let dbl = resp.double_clicked_by(egui::PointerButton::Primary);
        match self.pcb.tool.clone() {
            PTool::Select => {
                if resp.drag_started_by(egui::PointerButton::Primary) {
                    let start = resp.interact_pointer_pos().map(|pp| cam.world(rect, pp)).unwrap_or(m);
                    if let Some(sel @ PSel::Fp(_)) = self.brd_hit(start) {
                        self.pcb.sel = Some(sel);
                        self.pcb.drag = Some(Drag { start, sel, orig: self.pcb.project.clone(), rubber: vec![] });
                    }
                }
                if let Some(d) = self.pcb.drag.clone() {
                    if resp.dragged_by(egui::PointerButton::Primary) {
                        if let PSel::Fp(i) = d.sel {
                            let delta = snap(m - d.start, 0.05);
                            let mut pr = d.orig.clone();
                            pr.board.footprints[i].pos += delta;
                            self.pcb.project = pr;
                            self.pcb.changed();
                        }
                    }
                    if resp.drag_stopped() {
                        if self.pcb.project != d.orig {
                            self.pcb.undo.push(d.orig);
                            self.pcb.redo.clear();
                        }
                        self.pcb.drag = None;
                        self.pcb.changed();
                    }
                    return;
                }
                if primary_click {
                    self.pcb.sel = self.brd_hit(m);
                    if let Some(PSel::Track(i)) = self.pcb.sel {
                        self.pcb.hl_net = Some(self.pcb.project.board.tracks[i].net.clone());
                    }
                }
            }
            PTool::DiffPair => {
                if dbl {
                    self.pcb.route = None;
                    return;
                }
                if primary_click {
                    if self.pcb.route.is_none() {
                        self.pcb_pair_start(m);
                    } else {
                        self.pcb_route_commit(m);
                    }
                }
            }
            PTool::Tune => {
                if primary_click {
                    if let Some(PSel::Track(i)) = self.brd_hit(m) {
                        let net = self.pcb.project.board.tracks[i].net.clone();
                        let b = &self.pcb.project.board;
                        let now = b.track_length(&net);
                        // pair partner sets the target, otherwise 5 mm more
                        let target = crate::pcb::board::diff_partner(&net).map(|x| b.track_length(&x)).filter(|l| *l > now).unwrap_or(now + 5.0);
                        self.pcb.tune_dlg = Some((net, crate::pcb::geom::mm((target * 100.0).round() / 100.0)));
                    } else {
                        self.say("Länge abstimmen: auf eine Leiterbahn klicken.", MsgKind::Warning);
                    }
                }
            }
            PTool::Route => {
                if dbl {
                    self.pcb.route = None;
                    return;
                }
                if primary_click {
                    if self.pcb.route.is_none() {
                        match self.brd_copper_at(m, self.pcb.layer) {
                            Some((net, at)) if !net.is_empty() => {
                                let w = net_width(&self.pcb.project.board.rules, &net);
                                self.pcb.hl_net = Some(net.clone());
                                self.pcb.route = Some(Route { net, layer: self.pcb.layer, pts: vec![at], width: w, pair: None });
                            }
                            _ => self.say(format!("Leiterbahn auf {} an einem Pad, Via oder einer Leiterbahn mit Netz beginnen.", layer_title(self.pcb.layer)), MsgKind::Warning),
                        }
                    } else {
                        self.pcb_route_commit(m);
                    }
                }
            }
            PTool::Via => {
                if primary_click {
                    let net = self.brd_copper_at(m, F_CU).or_else(|| self.brd_copper_at(m, B_CU)).map(|x| x.0).unwrap_or_default();
                    let rules = self.pcb.project.board.rules.clone();
                    let at = snap(m, 0.05);
                    self.pcb_push();
                    self.pcb.project.board.vias.push(Via { pos: at, dia: rules.via_dia, drill: rules.via_drill, net: net.clone() });
                    self.say(if net.is_empty() { "Via ohne Netz gesetzt.".to_string() } else { format!("Via im Netz {} gesetzt.", net) }, MsgKind::Info);
                }
            }
            PTool::Zone => {
                let at = snap(m, 0.25);
                if dbl {
                    if self.pcb.zone_pts.len() >= 3 {
                        let (net, layer, clr) = self.pcb.zone_cfg.clone();
                        self.pcb_push();
                        self.pcb.project.board.zones.push(Zone { net: net.clone(), layer, outline: std::mem::take(&mut self.pcb.zone_pts), clearance: clr, min_width: 0.25, thermal: true, fill: vec![], fixed: false });
                        self.pcb.project.board.fill_zones();
                        self.pcb.changed();
                        self.pcb.tool = PTool::Select;
                        self.say(format!("Zone {} auf {} erstellt und gefüllt.", net, layer_title(layer)), MsgKind::Success);
                    }
                    return;
                }
                if primary_click && self.pcb.zone_pts.last().is_none_or(|l| (*l - at).length() > 1e-6) {
                    self.pcb.zone_pts.push(at);
                }
            }
            _ => {}
        }
    }

    // ---------------------------------------------------------------- board drawing

    fn zone_triangles(&mut self) -> Vec<(u8, Vec<[P; 3]>)> {
        if self.pcb.zone_mesh.is_none() {
            let mut out = vec![];
            for z in &self.pcb.project.board.zones {
                let mut tris = vec![];
                for shape in &z.fill {
                    let outer: Vec<glam::DVec2> = shape[0].iter().map(|q| glam::DVec2::new(q[0], q[1])).collect();
                    let holes: Vec<Vec<glam::DVec2>> = shape[1..].iter().map(|h| h.iter().map(|q| glam::DVec2::new(q[0], q[1])).collect()).collect();
                    let all: Vec<P> = outer.iter().chain(holes.iter().flatten()).copied().collect();
                    for t in crate::kernel::triangulate::triangulate(&outer, &holes) {
                        tris.push([all[t[0]], all[t[1]], all[t[2]]]);
                    }
                }
                out.push((z.layer, tris));
            }
            self.pcb.zone_mesh = Some(out);
        }
        self.pcb.zone_mesh.clone().unwrap()
    }

    fn pcb_draw_brd(&mut self, pa: &egui::Painter, rect: Rect, cam: &Cam2) {
        let sc = |q: P| cam.scr(rect, q);
        let zones = self.zone_triangles();
        let rats = if self.pcb.vis.rats { self.pcb.ratsnest().clone() } else { vec![] };
        let b = &self.pcb.project.board;
        // board body
        for (k, lp) in b.outline.iter().enumerate() {
            let outer = b.outer().is_some_and(|o| o == lp);
            let pts: Vec<glam::DVec2> = lp.clone();
            let mut mesh = egui::Mesh::default();
            let col = if outer { BOARD_FILL } else { Color32::TRANSPARENT };
            for q in &pts {
                mesh.colored_vertex(sc(*q), col);
            }
            for t in crate::kernel::triangulate::triangulate(&pts, &[]) {
                mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
            }
            if outer {
                pa.add(Shape::mesh(mesh));
            }
            let _ = k;
        }
        let hl = self.pcb.hl_net.clone();
        let dim = |net: &str, c: Color32| -> Color32 {
            match &hl {
                Some(h) if h != net => fade(c, 0.35),
                Some(_) => Color32::from_rgb(c.r().saturating_add(50), c.g().saturating_add(50), c.b().saturating_add(50)),
                None => c,
            }
        };
        let mut order: Vec<u8> = if b.layers >= 4 { vec![B_CU, 3, 2, F_CU] } else { vec![B_CU, F_CU] };
        // active layer on top
        order.retain(|l| *l != self.pcb.layer);
        order.push(self.pcb.layer);
        let sel = self.pcb.sel;
        for &l in &order {
            if !self.pcb.vis.cu[l as usize] {
                continue;
            }
            let active = l == self.pcb.layer;
            let base = if active { layer_color(l) } else { fade(layer_color(l), 0.55) };
            if self.pcb.vis.zones {
                for (zi, (zl, tris)) in zones.iter().enumerate() {
                    if *zl != l {
                        continue;
                    }
                    let zc = dim(&b.zones[zi].net, fade(base, if active { 0.55 } else { 0.35 }));
                    let mut mesh = egui::Mesh::default();
                    for t in tris {
                        let i0 = mesh.vertices.len() as u32;
                        for q in t {
                            mesh.colored_vertex(sc(*q), zc);
                        }
                        mesh.add_triangle(i0, i0 + 1, i0 + 2);
                    }
                    pa.add(Shape::mesh(mesh));
                }
            }
            for (zi, z) in b.zones.iter().enumerate() {
                if z.layer == l {
                    let c = if sel == Some(PSel::Zone(zi)) { SEL_C } else { fade(base, 0.9) };
                    let pts: Vec<Pos2> = z.outline.iter().map(|q| sc(*q)).collect();
                    pa.add(Shape::closed_line(pts, Stroke::new(1.0_f32, c)));
                }
            }
            for (i, t) in b.tracks.iter().enumerate() {
                if t.layer != l {
                    continue;
                }
                let c = if sel == Some(PSel::Track(i)) { SEL_C } else { dim(&t.net, base) };
                round_seg(pa, sc(t.a), sc(t.b), (t.width as f32 * cam.s).max(1.0), c);
            }
            for (fi, f) in b.footprints.iter().enumerate() {
                for k in 0..f.fp.pads.len() {
                    if f.pad_layers(k, b.layers) & (1 << l) == 0 {
                        continue;
                    }
                    let h = f.pad_hull(k);
                    let pts: Vec<Pos2> = h.polygon(20).iter().map(|q| sc(*q)).collect();
                    let c = if sel == Some(PSel::Fp(fi)) { Color32::from_rgb(120, 220, 120) } else { dim(&f.nets[k], if active { Color32::from_rgb(214, 160, 70) } else { fade(Color32::from_rgb(214, 160, 70), 0.7) }) };
                    pa.add(Shape::convex_polygon(pts, c, Stroke::NONE));
                }
            }
        }
        // drills and vias
        for (i, v) in b.vias.iter().enumerate() {
            let c = if sel == Some(PSel::Via(i)) { SEL_C } else { dim(&v.net, Color32::from_rgb(200, 200, 196)) };
            pa.circle_filled(sc(v.pos), v.dia as f32 / 2.0 * cam.s, c);
            pa.circle_filled(sc(v.pos), v.drill as f32 / 2.0 * cam.s, Color32::from_rgb(30, 30, 30));
        }
        for f in &b.footprints {
            for (k, pad) in f.fp.pads.iter().enumerate() {
                if pad.drill > 0.0 {
                    pa.circle_filled(sc(f.pad_pos(k)), pad.drill as f32 / 2.0 * cam.s, Color32::from_rgb(24, 24, 24));
                }
            }
        }
        // pad numbers when zoomed in
        if cam.s > 14.0 {
            for f in &b.footprints {
                for (k, pad) in f.fp.pads.iter().enumerate() {
                    if pad.num.is_empty() {
                        continue;
                    }
                    let px = (pad.size.x.min(pad.size.y) as f32 * cam.s * 0.45).min(16.0);
                    if px > 6.0 {
                        pa.text(sc(f.pad_pos(k)), Align2::CENTER_CENTER, &pad.num, FontId::proportional(px), Color32::WHITE);
                    }
                }
            }
        }
        // silkscreen
        if self.pcb.vis.silk {
            for (fi, f) in b.footprints.iter().enumerate() {
                let c = if sel == Some(PSel::Fp(fi)) { SEL_C } else if f.bottom { fade(SILK, 0.45) } else { SILK };
                for (a, e) in &f.fp.silk {
                    seg(pa, sc(f.xform(*a)), sc(f.xform(*e)), (0.12 * cam.s).max(1.0), c);
                }
                let (lo, hi) = f.fp.court;
                let at = f.xform(p((lo.x + hi.x) / 2.0, lo.y - 0.5));
                let px = text_px(cam, 0.9);
                if px > 4.0 {
                    pa.text(sc(at), Align2::CENTER_BOTTOM, &f.reference, FontId::proportional(px), c);
                }
            }
            for (at, t, h) in &b.texts {
                let px = text_px(cam, *h);
                if px > 3.0 {
                    pa.text(sc(*at), Align2::CENTER_CENTER, t, FontId::proportional(px), SILK);
                }
            }
        }
        if self.pcb.vis.court {
            for f in &b.footprints {
                let pts: Vec<Pos2> = f.courtyard().iter().map(|q| sc(*q)).collect();
                pa.add(Shape::closed_line(pts, Stroke::new(1.0_f32, Color32::from_rgb(220, 60, 220))));
            }
        }
        if let Some(PSel::Fp(i)) = sel {
            if let Some(f) = b.footprints.get(i) {
                let pts: Vec<Pos2> = f.courtyard().iter().map(|q| sc(*q)).collect();
                pa.add(Shape::closed_line(pts, Stroke::new(1.5_f32, SEL_C)));
            }
        }
        // outline
        if self.pcb.vis.edge {
            for lp in &b.outline {
                let pts: Vec<Pos2> = lp.iter().map(|q| sc(*q)).collect();
                pa.add(Shape::closed_line(pts, Stroke::new((0.15 * cam.s).max(1.5), EDGE)));
            }
        }
        // ratsnest
        for (a, e, net) in &rats {
            let c = if hl.as_deref() == Some(net.as_str()) { Color32::from_rgb(255, 255, 120) } else { RATS };
            seg(pa, sc(*a), sc(*e), 1.0, c);
        }
        // DRC markers
        if let Some(v) = &self.pcb.drc {
            for x in v {
                if x.kind == drc::Kind::Unrouted || x.pos == P::ZERO {
                    continue;
                }
                let c = sc(x.pos);
                let col = if x.kind.is_error() { ERR } else { WARN };
                pa.circle_stroke(c, 7.0, Stroke::new(2.0_f32, col));
                pa.add(Shape::convex_polygon(vec![c + vec2(5.0, -5.0), c + vec2(16.0, -12.0), c + vec2(12.0, -16.0)], col, Stroke::NONE));
            }
        }
        // previews
        if let Some(r) = self.pcb.route.clone().filter(|r| r.pair.is_some()) {
            let center = self.route_preview(&r, snap(self.pcb.mouse, 0.05));
            let (_, ea, eb, gap) = r.pair.clone().unwrap();
            let (la, lb) = crate::pcb::route::diff_pair(&center, r.width, gap);
            let c = fade(layer_color(r.layer), 0.85);
            for (line, end) in [(la, ea), (lb, eb)] {
                let mut pts = vec![end];
                pts.extend(line);
                for w in pts.windows(2) {
                    round_seg(pa, sc(w[0]), sc(w[1]), (r.width as f32 * cam.s).max(1.0), c);
                }
            }
        }
        if let Some(r) = self.pcb.route.clone().filter(|r| r.pair.is_none()) {
            let target = self.route_target(self.pcb.mouse, &r.net, r.layer);
            let pts = self.route_preview(&r, target);
            let bad = pts.windows(2).any(|w| self.route_conflict(w[0], w[1], r.width, r.layer, &r.net).is_some());
            let c = if bad { ERR } else { fade(layer_color(r.layer), 0.85) };
            for w in pts.windows(2) {
                round_seg(pa, sc(w[0]), sc(w[1]), (r.width as f32 * cam.s).max(1.0), c);
            }
            if bad {
                // clearance outline
                for w in pts.windows(2) {
                    let hull = Hull::capsule(w[0], w[1], r.width / 2.0 + self.pcb.project.board.rules.clearance);
                    let poly: Vec<Pos2> = hull.polygon(24).iter().map(|q| sc(*q)).collect();
                    pa.add(Shape::closed_line(poly, Stroke::new(1.0_f32, ERR)));
                }
            }
        }
        if !self.pcb.zone_pts.is_empty() {
            let mut pts: Vec<Pos2> = self.pcb.zone_pts.iter().map(|q| sc(*q)).collect();
            pts.push(sc(snap(self.pcb.mouse, 0.25)));
            pa.add(Shape::line(pts, Stroke::new(1.5_f32, layer_color(self.pcb.zone_cfg.1))));
        }
        if matches!(self.pcb.tool, PTool::Via) {
            let rules = &self.pcb.project.board.rules;
            pa.circle_stroke(sc(snap(self.pcb.mouse, 0.05)), rules.via_dia as f32 / 2.0 * cam.s, Stroke::new(1.5_f32, Color32::WHITE));
        }
    }

    // ------------------------------------------------------------------ dialogs

    pub fn pcb_windows(&mut self, ctx: &egui::Context) {
        if self.tab != Tab::Pcb {
            return;
        }
        self.pcb_editor_windows(ctx);
        let anchor = ctx.screen_rect().center() - vec2(200.0, 160.0);
        // symbol properties
        if let Some(mut pd) = self.pcb.props.clone() {
            let mut ok = false;
            let mut cancel = false;
            let lib = self.pcb.cur().symbols.get(pd.i).map(|s| s.lib.clone()).unwrap_or_default();
            let alts: Vec<String> = self.pcb.cur().def(&lib).map(|d| d.alt_footprints.clone()).unwrap_or_default();
            let open = dlgkit::cwin(ctx, "pcb_props", "Symboleigenschaften", false, anchor, 430.0, |ui| {
                dlgkit::pad(ui, 12.0, |ui| {
                    egui::Grid::new("pcb_props_grid").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                        dlgkit::lbl(ui, "Referenz");
                        dlgkit::field(ui, &mut pd.reference, 280.0, true);
                        ui.end_row();
                        dlgkit::lbl(ui, "Wert");
                        dlgkit::field(ui, &mut pd.value, 280.0, true);
                        ui.end_row();
                        dlgkit::lbl(ui, "Footprint");
                        let mut items: Vec<&str> = alts.iter().map(|s| s.as_str()).collect();
                        if !items.contains(&pd.footprint.as_str()) && !pd.footprint.is_empty() {
                            items.insert(0, &pd.footprint);
                        }
                        let mut k = items.iter().position(|x| *x == pd.footprint).unwrap_or(0);
                        let shorts: Vec<String> = items.iter().map(|s| s.rsplit(':').next().unwrap_or(s).to_string()).collect();
                        let sref: Vec<&str> = shorts.iter().map(|s| s.as_str()).collect();
                        if !sref.is_empty() && dlgkit::combo(ui, "pcb_fp", 280.0, &mut k, &sref, true, None) {
                            pd.footprint = items[k].to_string();
                        }
                        ui.end_row();
                        dlgkit::lbl(ui, "Andere");
                        ui.add_sized(vec2(280.0, 26.0), egui::TextEdit::singleline(&mut self.pcb.fp_filter).hint_text("Footprint suchen (alle Bibliotheken)"));
                        ui.end_row();
                        dlgkit::lbl(ui, "Symbol");
                        dlgkit::readout(ui, &lib, 280.0);
                        ui.end_row();
                    });
                    let q = self.pcb.fp_filter.to_lowercase();
                    if q.len() >= 2 {
                        let hits: Vec<&'static footprints::Footprint> = footprints::all().into_iter().filter(|f| f.id.to_lowercase().contains(&q)).take(8).collect();
                        for f in hits {
                            if row(ui, f.short(), f.id.split(':').next().unwrap_or(""), pd.footprint == f.id, None).clicked() {
                                pd.footprint = f.id.clone();
                            }
                        }
                    }
                    if let Some(port) = footprints::find(&pd.footprint).and_then(|f| f.port.clone()) {
                        ui.label(egui::RichText::new(format!("Gehäuseöffnung: Skizzenpalette ▸ Ports ▸ {}", port)).size(13.0).color(ACCENT));
                    }
                });
                dlgkit::hline(ui);
                dlgkit::pad(ui, 10.0, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if dlgkit::btn(ui, "Abbrechen", 96.0, 0) {
                            cancel = true;
                        }
                        if dlgkit::btn(ui, "OK", 96.0, 1) {
                            ok = true;
                        }
                    });
                });
            });
            if ok {
                self.pcb_push();
                if let Some(s) = self.pcb.cur_mut().symbols.get_mut(pd.i) {
                    s.reference = pd.reference.trim().to_string();
                    s.value = pd.value.trim().to_string();
                    s.footprint = pd.footprint.clone();
                }
                self.pcb.props = None;
            } else if cancel || !open {
                self.pcb.props = None;
            } else {
                self.pcb.props = Some(pd);
            }
        }
        // net label text
        if let Some((at, mut text, kind)) = self.pcb.label_dlg.clone() {
            let mut ok = false;
            let mut cancel = false;
            let open = dlgkit::cwin(ctx, "pcb_label", if kind == LabelKind::Global { "Globales Label" } else { "Netzlabel" }, false, anchor, 340.0, |ui| {
                dlgkit::pad(ui, 12.0, |ui| {
                    dlgkit::lbl(ui, "Netzname");
                    let r = dlgkit::field(ui, &mut text, 310.0, true);
                    r.request_focus();
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                });
                dlgkit::hline(ui);
                dlgkit::pad(ui, 10.0, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if dlgkit::btn(ui, "Abbrechen", 96.0, 0) {
                            cancel = true;
                        }
                        if dlgkit::btn(ui, "OK", 96.0, 1) {
                            ok = true;
                        }
                    });
                });
            });
            if ok && !text.trim().is_empty() {
                self.pcb_push();
                self.pcb.cur_mut().labels.push(Label { pos: at, text: text.trim().to_string(), kind, rot: 0.0 });
                self.pcb.label_dlg = None;
            } else if cancel || !open {
                self.pcb.label_dlg = None;
            } else {
                self.pcb.label_dlg = Some((at, text, kind));
            }
        }
        // design rules
        if let Some(mut r) = self.pcb.rules_dlg.clone() {
            let mut ok = false;
            let mut cancel = false;
            let open = dlgkit::cwin(ctx, "pcb_rules", "Designregeln", false, anchor, 860.0, |ui| {
                dlgkit::pad(ui, 12.0, |ui| {
                    egui::Grid::new("pcb_rules_grid").num_columns(3).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                        for (label, v) in [
                            ("Mindestabstand", &mut r.clearance),
                            ("Leiterbahnbreite", &mut r.track),
                            ("Versorgungsbahnen", &mut r.power_track),
                            ("Via-Durchmesser", &mut r.via_dia),
                            ("Via-Bohrung", &mut r.via_drill),
                            ("Min. Leiterbahnbreite", &mut r.min_track),
                            ("Min. Bohrung", &mut r.min_drill),
                            ("Min. Restring", &mut r.min_annular),
                            ("Abstand zum Rand", &mut r.edge_clearance),
                            ("Bohrungsabstand", &mut r.hole_clearance),
                            ("Max. Längenunterschied Paar", &mut r.max_skew),
                        ] {
                            dlgkit::lbl(ui, label);
                            ui.add(egui::DragValue::new(v).speed(0.01).range(0.05..=10.0).fixed_decimals(2));
                            dlgkit::lbl_dim(ui, "mm");
                            ui.end_row();
                        }
                    });
                    ui.add_space(6.0);
                    dlgkit::lbl_bold(ui, "Netzklassen (erste passende Klasse gilt; * = beliebiger Text)");
                    let mut del = None;
                    egui::Grid::new("pcb_classes").num_columns(9).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
                        for h in ["Name", "Netze", "Abstand", "Breite", "Via-Ø", "Bohrung", "Paar-Breite", "Paar-Spalt", ""] {
                            dlgkit::lbl_dim(ui, h);
                        }
                        ui.end_row();
                        for (k, c) in r.classes.iter_mut().enumerate() {
                            ui.add_sized(vec2(90.0, 24.0), egui::TextEdit::singleline(&mut c.name));
                            let mut pats = c.patterns.join(", ");
                            if ui.add_sized(vec2(200.0, 24.0), egui::TextEdit::singleline(&mut pats)).changed() {
                                c.patterns = pats.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                            }
                            for v in [&mut c.clearance, &mut c.track, &mut c.via_dia, &mut c.via_drill, &mut c.diff_width, &mut c.diff_gap] {
                                ui.add(egui::DragValue::new(v).speed(0.01).range(0.05..=10.0).fixed_decimals(2));
                            }
                            if ui.small_button("✕").clicked() {
                                del = Some(k);
                            }
                            ui.end_row();
                        }
                    });
                    if let Some(k) = del {
                        r.classes.remove(k);
                    }
                    if dlgkit::btn(ui, "Netzklasse hinzufügen", 170.0, 0) {
                        r.classes.push(crate::pcb::board::NetClass { name: format!("Klasse{}", r.classes.len() + 1), patterns: vec![], clearance: r.clearance, track: r.track, via_dia: r.via_dia, via_drill: r.via_drill, diff_width: r.track, diff_gap: r.clearance });
                    }
                });
                dlgkit::hline(ui);
                dlgkit::pad(ui, 10.0, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if dlgkit::btn(ui, "Abbrechen", 96.0, 0) {
                            cancel = true;
                        }
                        if dlgkit::btn(ui, "OK", 96.0, 1) {
                            ok = true;
                        }
                    });
                });
            });
            if ok {
                self.pcb_push();
                self.pcb.project.board.rules = r;
                self.pcb.rules_dlg = None;
                self.say("Designregeln übernommen – DRC erneut ausführen.", MsgKind::Info);
            } else if cancel || !open {
                self.pcb.rules_dlg = None;
            } else {
                self.pcb.rules_dlg = Some(r);
            }
        }
        // outline
        if let Some((mut w, mut h)) = self.pcb.outline_dlg.clone() {
            let mut ok = false;
            let mut cancel = false;
            let mut from_model = false;
            let open = dlgkit::cwin(ctx, "pcb_outline", "Platinenumriss", false, anchor, 360.0, |ui| {
                dlgkit::pad(ui, 12.0, |ui| {
                    egui::Grid::new("pcb_outline_grid").num_columns(3).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                        dlgkit::lbl(ui, "Breite");
                        dlgkit::field(ui, &mut w, 120.0, true);
                        dlgkit::lbl_dim(ui, "mm");
                        ui.end_row();
                        dlgkit::lbl(ui, "Höhe");
                        dlgkit::field(ui, &mut h, 120.0, true);
                        dlgkit::lbl_dim(ui, "mm");
                        ui.end_row();
                    });
                    if dlgkit::btn(ui, "Aus Skizze des offenen Teils", 300.0, 0) {
                        from_model = true;
                    }
                });
                dlgkit::hline(ui);
                dlgkit::pad(ui, 10.0, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if dlgkit::btn(ui, "Abbrechen", 96.0, 0) {
                            cancel = true;
                        }
                        if dlgkit::btn(ui, "OK", 96.0, 1) {
                            ok = true;
                        }
                    });
                });
            });
            let num = |s: &str| s.trim().replace(',', ".").parse::<f64>().ok().filter(|v| *v > 1.0 && *v < 2000.0);
            if from_model {
                self.pcb.outline_dlg = None;
                self.pcb_outline_from_model();
            } else if ok {
                match (num(&w), num(&h)) {
                    (Some(wv), Some(hv)) => {
                        self.pcb_push();
                        let (lo, _) = self.pcb.project.board.size().unwrap_or((p(100.0, 100.0), P::ZERO));
                        self.pcb.project.board.rect_outline(lo.x, lo.y, wv, hv);
                        self.pcb.changed();
                        self.pcb.fit = true;
                        self.pcb.outline_dlg = None;
                    }
                    _ => self.say("Breite und Höhe als Zahlen in mm eingeben.", MsgKind::Warning),
                }
            } else if cancel || !open {
                self.pcb.outline_dlg = None;
            } else {
                self.pcb.outline_dlg = Some((w, h));
            }
        }
        // zone settings
        if let Some(mut zd) = self.pcb.zone_dlg.clone() {
            let mut ok = false;
            let mut cancel = false;
            let nets = self.pcb.project.board.nets();
            let layers: Vec<u8> = if self.pcb.project.board.layers >= 4 { vec![F_CU, 2, 3, B_CU] } else { vec![F_CU, B_CU] };
            let open = dlgkit::cwin(ctx, "pcb_zone", "Kupferzone", false, anchor, 380.0, |ui| {
                dlgkit::pad(ui, 12.0, |ui| {
                    egui::Grid::new("pcb_zone_grid").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                        dlgkit::lbl(ui, "Netz");
                        let names: Vec<&str> = nets.iter().map(|s| s.as_str()).collect();
                        if !names.is_empty() {
                            dlgkit::combo(ui, "pcb_zone_net", 230.0, &mut zd.net, &names, true, None);
                        } else {
                            dlgkit::readout(ui, "keine Netze", 230.0);
                        }
                        ui.end_row();
                        dlgkit::lbl(ui, "Lage");
                        let titles: Vec<&str> = layers.iter().map(|l| layer_title(*l)).collect();
                        let mut k = layers.iter().position(|l| *l == zd.layer).unwrap_or(0);
                        if dlgkit::combo(ui, "pcb_zone_layer", 230.0, &mut k, &titles, true, None) {
                            zd.layer = layers[k];
                        }
                        ui.end_row();
                        dlgkit::lbl(ui, "Abstand (mm)");
                        dlgkit::field(ui, &mut zd.clearance, 230.0, true);
                        ui.end_row();
                    });
                    dlgkit::lbl_dim(ui, "Danach die Ecken klicken; Doppelklick schließt.");
                    if dlgkit::btn(ui, "Ganze Platine", 160.0, 0) {
                        ok = true;
                        zd.clearance = format!("{}#all", zd.clearance);
                    }
                });
                dlgkit::hline(ui);
                dlgkit::pad(ui, 10.0, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if dlgkit::btn(ui, "Abbrechen", 96.0, 0) {
                            cancel = true;
                        }
                        if dlgkit::btn(ui, "Zeichnen", 96.0, 1) {
                            ok = true;
                        }
                    });
                });
            });
            if ok && !nets.is_empty() {
                let whole = zd.clearance.ends_with("#all");
                let clr = zd.clearance.trim_end_matches("#all").trim().replace(',', ".").parse::<f64>().unwrap_or(0.3).clamp(0.1, 5.0);
                let net = nets[zd.net.min(nets.len() - 1)].clone();
                self.pcb.zone_cfg = (net.clone(), zd.layer, clr);
                self.pcb.zone_dlg = None;
                if whole {
                    if let Some((lo, hi)) = self.pcb.project.board.size() {
                        self.pcb_push();
                        self.pcb.project.board.zones.push(Zone { net: net.clone(), layer: zd.layer, outline: vec![lo, p(hi.x, lo.y), hi, p(lo.x, hi.y)], clearance: clr, min_width: 0.25, thermal: true, fill: vec![], fixed: false });
                        self.pcb.project.board.fill_zones();
                        self.pcb.changed();
                        self.say(format!("Zone {} über die ganze Platine auf {} gefüllt.", net, layer_title(zd.layer)), MsgKind::Success);
                    }
                } else {
                    self.pcb_tool(PTool::Zone);
                }
            } else if cancel || !open || (ok && nets.is_empty()) {
                self.pcb.zone_dlg = None;
            } else {
                self.pcb.zone_dlg = Some(zd);
            }
        }
        // length tuning
        if let Some((net, mut target)) = self.pcb.tune_dlg.clone() {
            let mut ok = false;
            let mut cancel = false;
            let now = self.pcb.project.board.track_length(&net);
            let open = dlgkit::cwin(ctx, "pcb_tune", "Länge abstimmen", false, anchor, 360.0, |ui| {
                dlgkit::pad(ui, 12.0, |ui| {
                    dlgkit::lbl(ui, &format!("Netz {}: jetzt {} mm", net, crate::pcb::geom::mm((now * 100.0).round() / 100.0)));
                    if let Some(p2) = crate::pcb::board::diff_partner(&net) {
                        let l2 = self.pcb.project.board.track_length(&p2);
                        if l2 > 0.0 {
                            dlgkit::lbl_dim(ui, &format!("Partner {}: {} mm", p2, crate::pcb::geom::mm((l2 * 100.0).round() / 100.0)));
                        }
                    }
                    ui.horizontal(|ui| {
                        dlgkit::lbl(ui, "Ziellänge");
                        dlgkit::field(ui, &mut target, 120.0, true);
                        dlgkit::lbl_dim(ui, "mm");
                    });
                });
                dlgkit::hline(ui);
                dlgkit::pad(ui, 10.0, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if dlgkit::btn(ui, "Abbrechen", 96.0, 0) {
                            cancel = true;
                        }
                        if dlgkit::btn(ui, "Abstimmen", 96.0, 1) {
                            ok = true;
                        }
                    });
                });
            });
            if ok {
                match target.trim().replace(',', ".").parse::<f64>() {
                    Ok(t) => {
                        let mut trial = self.pcb.project.board.clone();
                        match crate::pcb::route::tune_length(&mut trial, &net, t) {
                            Ok(len) => {
                                self.pcb_push();
                                self.pcb.project.board = trial;
                                self.pcb.changed();
                                self.say(format!("Netz {} auf {} mm abgestimmt.", net, crate::pcb::geom::mm((len * 100.0).round() / 100.0)), MsgKind::Success);
                            }
                            Err(e) => self.say(format!("Länge abstimmen: {}", e), MsgKind::Warning),
                        }
                        self.pcb.tune_dlg = None;
                    }
                    Err(_) => self.say("Ziellänge als Zahl in mm eingeben.", MsgKind::Warning),
                }
            } else if cancel || !open {
                self.pcb.tune_dlg = None;
            } else {
                self.pcb.tune_dlg = Some((net, target));
            }
        }
        // new hierarchical sheet
        if let Some((at, mut name)) = self.pcb.sheet_dlg.clone() {
            let mut ok = false;
            let mut cancel = false;
            let open = dlgkit::cwin(ctx, "pcb_sheet", "Hierarchisches Blatt", false, anchor, 340.0, |ui| {
                dlgkit::pad(ui, 12.0, |ui| {
                    dlgkit::lbl(ui, "Blattname");
                    let r = dlgkit::field(ui, &mut name, 310.0, true);
                    r.request_focus();
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                });
                dlgkit::hline(ui);
                dlgkit::pad(ui, 10.0, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if dlgkit::btn(ui, "Abbrechen", 96.0, 0) {
                            cancel = true;
                        }
                        if dlgkit::btn(ui, "OK", 96.0, 1) {
                            ok = true;
                        }
                    });
                });
            });
            let taken = self.pcb.cur().sheets.iter().any(|s| s.name == name.trim());
            if ok && !name.trim().is_empty() && !taken {
                self.pcb_push();
                self.pcb.cur_mut().sheets.push(crate::pcb::schematic::Sheet { name: name.trim().to_string(), pos: at, size: p(30.48, 20.32), pins: vec![], sch: Schematic::default() });
                self.pcb.sheet_dlg = None;
                self.pcb.tool = PTool::SheetPin;
                self.say("Blatt eingefügt – jetzt Blattpins am Rand setzen, Doppelklick öffnet das Blatt.", MsgKind::Info);
            } else if ok && taken {
                self.say("Ein Blatt mit diesem Namen gibt es schon.", MsgKind::Warning);
                self.pcb.sheet_dlg = Some((at, name));
            } else if cancel || !open {
                self.pcb.sheet_dlg = None;
            } else {
                self.pcb.sheet_dlg = Some((at, name));
            }
        }
        // sheet pin (with its hierarchical label inside the sheet)
        if let Some((k, at, mut name)) = self.pcb.sheet_pin_dlg.clone() {
            let mut ok = false;
            let mut cancel = false;
            let open = dlgkit::cwin(ctx, "pcb_sheet_pin", "Blattpin", false, anchor, 340.0, |ui| {
                dlgkit::pad(ui, 12.0, |ui| {
                    dlgkit::lbl(ui, "Name (= hierarchisches Label im Blatt)");
                    let r = dlgkit::field(ui, &mut name, 310.0, true);
                    r.request_focus();
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                });
                dlgkit::hline(ui);
                dlgkit::pad(ui, 10.0, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if dlgkit::btn(ui, "Abbrechen", 96.0, 0) {
                            cancel = true;
                        }
                        if dlgkit::btn(ui, "OK", 96.0, 1) {
                            ok = true;
                        }
                    });
                });
            });
            if ok && !name.trim().is_empty() {
                self.pcb_push();
                let n = name.trim().to_string();
                if let Some(sh) = self.pcb.cur_mut().sheets.get_mut(k) {
                    sh.pins.push(crate::pcb::schematic::SheetPin { name: n.clone(), pos: at });
                    if !sh.sch.labels.iter().any(|l| l.kind == LabelKind::Hier && l.text == n) {
                        let y = 25.4 + 5.08 * sh.sch.labels.iter().filter(|l| l.kind == LabelKind::Hier).count() as f64;
                        sh.sch.labels.push(Label { pos: p(25.4, y), text: n, kind: LabelKind::Hier, rot: 0.0 });
                    }
                }
                self.pcb.sheet_pin_dlg = None;
            } else if cancel || !open {
                self.pcb.sheet_pin_dlg = None;
            } else {
                self.pcb.sheet_pin_dlg = Some((k, at, name));
            }
        }
        // ERC / DRC report
        if self.pcb.report {
            let sch = self.pcb.view == PcbView::Schematic;
            let items: Vec<(bool, String, String, P)> = if sch {
                self.pcb.erc.as_ref().map(|v| v.iter().map(|e| (e.severity == Severity::Error, if e.severity == Severity::Error { "Fehler".to_string() } else { "Warnung".to_string() }, e.msg.clone(), e.pos)).collect()).unwrap_or_default()
            } else {
                self.pcb.drc.as_ref().map(|v| v.iter().map(|x| (x.kind.is_error(), x.kind.name().to_string(), x.msg.clone(), x.pos)).collect()).unwrap_or_default()
            };
            let has = if sch { self.pcb.erc.is_some() } else { self.pcb.drc.is_some() };
            if has {
                let mut go = None;
                let mut rerun = false;
                let title = if sch { "Elektrische Regelprüfung (ERC)" } else { "Design-Regelprüfung (DRC)" };
                let pos = pos2(ctx.screen_rect().right() - 540.0, ctx.screen_rect().bottom() - 330.0);
                let open = dlgkit::cwin(ctx, "pcb_report", title, false, pos, 520.0, |ui| {
                    dlgkit::pad(ui, 10.0, |ui| {
                        let e = items.iter().filter(|x| x.0).count();
                        dlgkit::lbl_bold(ui, &format!("{} Fehler, {} Warnung(en)", e, items.len() - e));
                        egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            if items.is_empty() {
                                dlgkit::lbl(ui, "Keine Meldungen – alles in Ordnung.");
                            }
                            for (err, kind, msg, at) in &items {
                                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                                if resp.hovered() {
                                    ui.painter().rect_filled(r, 0.0, HOVER_BG);
                                }
                                let c = pos2(r.left() + 12.0, r.center().y);
                                ui.painter().circle_filled(c, 6.0, if *err { ERR } else { WARN });
                                ui.painter().text(c, Align2::CENTER_CENTER, "!", FontId::proportional(10.0), Color32::WHITE);
                                ui.painter().text(pos2(r.left() + 26.0, r.top() + 11.0), Align2::LEFT_CENTER, kind, FontId::proportional(12.0), Color32::from_rgb(110, 116, 124));
                                ui.painter().with_clip_rect(r).text(pos2(r.left() + 26.0, r.top() + 28.0), Align2::LEFT_CENTER, msg, FontId::proportional(13.0), TEXT);
                                if resp.clicked() && *at != P::ZERO {
                                    go = Some(*at);
                                }
                            }
                        });
                    });
                    dlgkit::hline(ui);
                    dlgkit::pad(ui, 10.0, |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if dlgkit::btn(ui, "Schließen", 96.0, 0) {
                                self.pcb.report = false;
                            }
                            if dlgkit::btn(ui, "Erneut prüfen", 120.0, 1) {
                                rerun = true;
                            }
                        });
                    });
                });
                if !open {
                    self.pcb.report = false;
                }
                if let Some(at) = go {
                    let cam = if sch { &mut self.pcb.sch_cam } else { &mut self.pcb.brd_cam };
                    cam.c = at;
                    cam.s = cam.s.max(if sch { 8.0 } else { 20.0 });
                }
                if rerun {
                    if sch {
                        self.pcb_run_erc();
                    } else {
                        self.pcb_run_drc();
                    }
                }
            }
        }
    }
}

/// Demo helper: loads the sample project into the editor.
pub fn demo_project(routed: bool) -> Project {
    if routed {
        crate::pcb::demo::routed()
    } else {
        crate::pcb::demo::project()
    }
}

#[allow(dead_code)]
fn _unused(_: &FpInst) {}
