//! G-code: turns planned layers into moves (retraction, wipe, z-hop, cooling slowdown, fan,
//! temperatures), estimates the print time with Klipper's look-ahead model, and writes the
//! text with an Orca-style header, thumbnail and settings block.

use super::config::*;
use super::mesh::Mesh;
use super::plan::{Layer, Plan, Role};
use super::poly::{self, Pt};
use super::template::{self, Val, Vars};
use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Extrude,
    Travel,
    Retract,
    Unretract,
    Wipe,
}

/// One straight move, as the printer will do it (also what the preview draws).
#[derive(Clone, Copy, Debug)]
pub struct Move {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub kind: Kind,
    pub role: Role,
    pub width: f32,
    pub height: f32,
    /// mm/s after cooling slowdown
    pub speed: f32,
    /// part fan 0..1
    pub fan: f32,
    pub temp: f32,
    /// filament length (mm), negative for retraction
    pub e: f32,
    pub layer: u32,
    /// seconds into the print when this move ends
    pub t: f32,
}

impl Move {
    /// mm³/s while extruding
    pub fn flow(&self, f: &Filament) -> f32 {
        if self.kind != Kind::Extrude {
            return 0.0;
        }
        let l = len3(self.a, self.b);
        if l < 1e-6 {
            return 0.0;
        }
        (self.e as f64 * f.area() / l as f64 * self.speed as f64) as f32
    }
}

fn len3(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub time: f64,
    pub layer_time: Vec<f64>,
    pub role_time: BTreeMap<Role, f64>,
    pub travel_time: f64,
    /// filament per feature (mm)
    pub role_filament: BTreeMap<Role, f64>,
    pub filament_mm: f64,
    pub volume_cm3: f64,
    pub grams: f64,
    pub cost: f64,
    pub layers: usize,
    pub max_z: f64,
}

pub struct Output {
    pub gcode: String,
    pub moves: Vec<Move>,
    /// index of the first move of every layer (+ one past the end)
    pub layer_start: Vec<usize>,
    pub stats: Stats,
    pub warnings: Vec<String>,
}

pub fn fmt_time(s: f64) -> String {
    let s = s.max(0.0).round() as u64;
    let (d, h, m, sec) = (s / 86400, (s / 3600) % 24, (s / 60) % 60, s % 60);
    if d > 0 {
        format!("{}d {}h {}m {}s", d, h, m, sec)
    } else if h > 0 {
        format!("{}h {}m {}s", h, m, sec)
    } else if m > 0 {
        format!("{}m {}s", m, sec)
    } else {
        format!("{}s", sec)
    }
}

/// Planner block for the time estimate.
struct Block {
    len: f64,
    v: f64,
    accel: f64,
    dir: [f64; 3],
    /// extruder-only move: starts and ends at rest
    e_only: bool,
}

/// Klipper's look-ahead: junction speeds from the square corner velocity, then
/// trapezoids forward and backward. Returns the time of every block.
fn estimate(blocks: &[Block], scv: f64) -> Vec<f64> {
    let n = blocks.len();
    let mut max_entry2 = vec![0.0; n];
    for i in 1..n {
        let (p, c) = (&blocks[i - 1], &blocks[i]);
        if p.e_only || c.e_only || p.len < 1e-9 || c.len < 1e-9 {
            continue;
        }
        let cos = -(p.dir[0] * c.dir[0] + p.dir[1] * c.dir[1] + p.dir[2] * c.dir[2]);
        let a = c.accel.min(p.accel);
        let jd = scv * scv * (std::f64::consts::SQRT_2 - 1.0) / a;
        let vj2 = if cos > 0.999_999 {
            0.0
        } else if cos < -0.999_999 {
            f64::INFINITY
        } else {
            let s = (0.5 * (1.0 - cos)).sqrt();
            s * jd * a / (1.0 - s)
        };
        max_entry2[i] = vj2.min(c.v * c.v).min(p.v * p.v);
    }
    // backward pass: every block must be able to stop by the end of the path
    let mut entry2 = max_entry2.clone();
    let mut next = 0.0;
    for i in (0..n).rev() {
        let b = &blocks[i];
        let exit2 = if b.e_only { 0.0 } else { next };
        entry2[i] = entry2[i].min(exit2 + 2.0 * b.accel * b.len);
        next = if b.e_only { 0.0 } else { entry2[i] };
    }
    // forward pass and trapezoid times
    let mut t = vec![0.0; n];
    let mut v0_2: f64 = 0.0;
    for i in 0..n {
        let b = &blocks[i];
        if b.len < 1e-12 {
            t[i] = 0.0;
            continue;
        }
        let start2 = if b.e_only { 0.0 } else { v0_2.min(entry2[i]) };
        let exit_cap = if b.e_only || i + 1 >= n || blocks[i + 1].e_only { 0.0 } else { entry2[i + 1] };
        let end2 = exit_cap.min(start2 + 2.0 * b.accel * b.len);
        let (v0, v1, vm, a, l) = (start2.sqrt(), end2.sqrt(), b.v, b.accel, b.len);
        let da = (vm * vm - v0 * v0).max(0.0) / (2.0 * a);
        let dd = (vm * vm - v1 * v1).max(0.0) / (2.0 * a);
        t[i] = if da + dd <= l {
            (vm - v0).max(0.0) / a + (vm - v1).max(0.0) / a + (l - da - dd) / vm
        } else {
            let vp = ((2.0 * a * l + v0 * v0 + v1 * v1) / 2.0).sqrt();
            (vp - v0).max(0.0) / a + (vp - v1).max(0.0) / a
        };
        v0_2 = end2;
    }
    t
}

/// Ops of a layer before they become text.
#[derive(Clone, Debug)]
enum Op {
    Comment(String),
    Raw(String),
    Accel(f64),
    /// x y z, e (mm filament), feed mm/s, role, width, height
    Ext([f64; 3], f64, f64, Role, f64, f64),
    Travel([f64; 3], f64),
    Retract(f64, f64),
    Unretract(f64, f64),
    /// wipe move with retraction
    Wipe([f64; 3], f64, f64),
}

struct Writer<'a> {
    s: &'a Settings,
    pos: [f64; 3],
    retracted: f64,
    hopped: bool,
    accel: f64,
    /// last extruded points (for wipe), newest last
    trail: Vec<[f64; 3]>,
}

impl Writer<'_> {
    fn retract_len(&self) -> f64 {
        self.s.filament.retract_length.unwrap_or(self.s.printer.retract_length).max(0.0)
    }
    fn z_hop(&self) -> f64 {
        self.s.filament.z_hop.unwrap_or(self.s.printer.z_hop).max(0.0)
    }

    fn retract(&mut self, ops: &mut Vec<Op>, wipe: bool) {
        let r = self.retract_len();
        if r <= 0.0 || self.retracted > 0.0 {
            return;
        }
        let p = &self.s.printer;
        if wipe && p.wipe && p.wipe_distance > 0.0 && self.trail.len() >= 2 {
            // 70 % before the wipe, the rest while wiping back over the last line
            let before = r * 0.7;
            ops.push(Op::Retract(before, p.retract_speed));
            let mut left = p.wipe_distance;
            let mut path = vec![];
            let mut cur = self.pos;
            for q in self.trail.iter().rev().skip(1) {
                let d = ((q[0] - cur[0]).powi(2) + (q[1] - cur[1]).powi(2)).sqrt();
                if d < 1e-9 {
                    continue;
                }
                if d >= left {
                    let t = left / d;
                    path.push([cur[0] + (q[0] - cur[0]) * t, cur[1] + (q[1] - cur[1]) * t, self.pos[2]]);
                    left = 0.0;
                    break;
                }
                path.push([q[0], q[1], self.pos[2]]);
                left -= d;
                cur = *q;
            }
            let total = p.wipe_distance - left;
            let rest = r - before;
            let mut from = self.pos;
            let mut done = 0.0;
            for q in path {
                let d = ((q[0] - from[0]).powi(2) + (q[1] - from[1]).powi(2)).sqrt();
                let e = if total > 0.0 { rest * d / total } else { 0.0 };
                done += e;
                ops.push(Op::Wipe(q, e, self.s.process.travel_speed.min(200.0)));
                from = q;
            }
            self.pos = from;
            if rest - done > 1e-6 {
                ops.push(Op::Retract(rest - done, p.retract_speed));
            }
        } else {
            ops.push(Op::Retract(r, p.retract_speed));
        }
        self.retracted = r;
        self.trail.clear();
    }

    fn travel(&mut self, ops: &mut Vec<Op>, to: Pt, z: f64, layer: &Layer, first_layer: bool, force_retract: bool) {
        let d = ((to[0] - self.pos[0]).powi(2) + (to[1] - self.pos[1]).powi(2)).sqrt();
        let dz = (z - self.pos[2]).abs();
        if d < 1e-4 && dz < 1e-6 {
            return;
        }
        let p = &self.s.printer;
        let q = &self.s.process;
        let inside = q.reduce_infill_retraction && !layer.inner.is_empty() && poly::segment_inside(&layer.inner, [self.pos[0], self.pos[1]], to);
        let need = force_retract || (d > p.retract_min_travel && !inside);
        if need {
            self.retract(ops, true);
        }
        let speed = if first_layer { q.first_layer_travel_speed } else { q.travel_speed };
        if self.travel_accel_differs() {
            self.accel = q.travel_accel.min(p.max_accel);
            ops.push(Op::Accel(self.accel));
        }
        let hop = self.z_hop();
        if self.retracted > 0.0 && hop > 0.0 && !self.hopped && d > p.retract_min_travel {
            ops.push(Op::Travel([self.pos[0], self.pos[1], self.pos[2] + hop], p.max_z_speed.min(speed)));
            self.pos[2] += hop;
            self.hopped = true;
        }
        let tz = if self.hopped { z + hop } else { z };
        ops.push(Op::Travel([to[0], to[1], tz], speed));
        self.pos = [to[0], to[1], tz];
        if !self.hopped && (self.pos[2] - z).abs() > 1e-9 {
            ops.push(Op::Travel([to[0], to[1], z], p.max_z_speed));
            self.pos[2] = z;
        }
    }

    fn travel_accel_differs(&self) -> bool {
        let a = self.s.process.travel_accel.min(self.s.printer.max_accel);
        (self.accel - a).abs() > 0.5
    }

    fn before_extrude(&mut self, ops: &mut Vec<Op>, z: f64) {
        if self.hopped {
            ops.push(Op::Travel([self.pos[0], self.pos[1], z], self.s.printer.max_z_speed));
            self.pos[2] = z;
            self.hopped = false;
        }
        if self.retracted > 0.0 {
            ops.push(Op::Unretract(self.retracted, self.s.printer.deretract_speed));
            self.retracted = 0.0;
        }
    }
}

fn vars(s: &Settings, plan: &Plan, bed: (f64, f64)) -> Vars {
    let (p, f, q) = (&s.printer, &s.filament, &s.process);
    let mut v = Vars::new();
    let mut num = |k: &str, x: f64| {
        v.insert(k.into(), Val::Num(x));
    };
    for k in ["bed_temperature_initial_layer_single", "bed_temperature_initial_layer", "first_layer_bed_temperature", "textured_plate_temp_initial_layer", "hot_plate_temp_initial_layer"] {
        num(k, f.bed_temp_first);
    }
    for k in ["bed_temperature", "textured_plate_temp", "hot_plate_temp"] {
        num(k, f.bed_temp);
    }
    for k in ["nozzle_temperature_initial_layer", "first_layer_temperature"] {
        num(k, f.nozzle_temp_first);
    }
    for k in ["nozzle_temperature", "temperature"] {
        num(k, f.nozzle_temp);
    }
    num("nozzle_temperature_range_low", (f.nozzle_temp - 20.0).max(0.0));
    num("nozzle_temperature_range_high", f.nozzle_temp + 20.0);
    num("chamber_temperature", f.chamber_temp);
    num("initial_no_support_extruder", 0.0);
    num("initial_extruder", 0.0);
    num("initial_tool", 0.0);
    num("current_extruder", 0.0);
    num("layer_height", q.layer_height);
    num("initial_layer_height", q.first_layer_height);
    num("first_layer_height", q.first_layer_height);
    num("initial_layer_print_height", q.first_layer_height);
    num("printable_height", p.height);
    num("printable_width", bed.0);
    num("printable_depth", bed.1);
    num("max_layer_z", plan.max_z);
    num("max_print_height", p.height);
    num("z_offset", 0.0);
    num("nozzle_diameter", p.nozzle);
    num("filament_diameter", f.diameter);
    num("total_layer_count", plan.layers.len() as f64);
    num("during_print_exhaust_fan_speed", f.exhaust_fan);
    num("additional_cooling_fan_speed", f.aux_fan);
    num("retraction_length", p.retract_length);
    let off = if p.origin_center { [-bed.0 / 2.0, -bed.1 / 2.0] } else { [0.0, 0.0] };
    num("first_layer_print_min[0]", plan.first_min[0] + off[0]);
    num("first_layer_print_min[1]", plan.first_min[1] + off[1]);
    num("first_layer_print_max[0]", plan.first_max[0] + off[0]);
    num("first_layer_print_max[1]", plan.first_max[1] + off[1]);
    v.insert("filament_type".into(), Val::Str(f.material.code().into()));
    v.insert("printer_model".into(), Val::Str(p.name()));
    v.insert("bed_shape".into(), Val::Str(if matches!(p.shape, BedShape::Round) { "round".into() } else { "rect".into() }));
    v.insert("first_layer_print_min".into(), Val::Str(format!("{:.3},{:.3}", plan.first_min[0] + off[0], plan.first_min[1] + off[1])));
    v
}

/// Orca-like thumbnail: the placed part in its filament colour on transparent ground,
/// seen from the front right above.
pub fn thumbnail(mesh: &Mesh, size: usize, color: [u8; 3]) -> Vec<u8> {
    use glam::DVec3;
    let n = size.max(16);
    let mut rgba = vec![0u8; n * n * 4];
    if mesh.is_empty() {
        return super::super::softrender::encode_png_rgba(n, n, &rgba);
    }
    let view = DVec3::new(0.55, -1.0, 0.75).normalize();
    let right = DVec3::Z.cross(view).normalize();
    let up = view.cross(right);
    let (lo, hi) = mesh.bbox();
    let c = (lo + hi) / 2.0;
    let r = ((hi - lo).length() / 2.0).max(1e-6);
    let scale = n as f64 * 0.46 / r;
    let mut zb = vec![f64::NEG_INFINITY; n * n];
    let light = DVec3::new(0.3, -0.5, 1.0).normalize();
    for ti in 0..mesh.t.len() {
        let tri = mesh.tri(ti);
        let nrm = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
        if nrm == DVec3::ZERO {
            continue;
        }
        let shade = 0.38 + 0.62 * nrm.dot(light).abs();
        let s: Vec<(f64, f64, f64)> = tri
            .iter()
            .map(|p| {
                let d = *p - c;
                (n as f64 / 2.0 + d.dot(right) * scale, n as f64 / 2.0 - d.dot(up) * scale, d.dot(view))
            })
            .collect();
        let (x0, x1) = (s.iter().map(|p| p.0).fold(f64::INFINITY, f64::min).floor().max(0.0) as usize, s.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max).ceil().min(n as f64 - 1.0) as usize);
        let (y0, y1) = (s.iter().map(|p| p.1).fold(f64::INFINITY, f64::min).floor().max(0.0) as usize, s.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max).ceil().min(n as f64 - 1.0) as usize);
        let area = (s[1].0 - s[0].0) * (s[2].1 - s[0].1) - (s[2].0 - s[0].0) * (s[1].1 - s[0].1);
        if area.abs() < 1e-12 {
            continue;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let w0 = ((s[1].0 - px) * (s[2].1 - py) - (s[2].0 - px) * (s[1].1 - py)) / area;
                let w1 = ((s[2].0 - px) * (s[0].1 - py) - (s[0].0 - px) * (s[2].1 - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                // larger dot with the view direction = nearer the camera
                let depth = w0 * s[0].2 + w1 * s[1].2 + w2 * s[2].2;
                let k = y * n + x;
                if depth > zb[k] {
                    zb[k] = depth;
                    for ch in 0..3 {
                        rgba[k * 4 + ch] = (color[ch] as f64 * shade).clamp(0.0, 255.0) as u8;
                    }
                    rgba[k * 4 + 3] = 255;
                }
            }
        }
    }
    super::super::softrender::encode_png_rgba(n, n, &rgba)
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Builds the G-code for a plan. `thumb`: PNG to embed (or None).
pub fn write(plan: &Plan, s: &Settings, thumb: Option<(usize, Vec<u8>)>, now: &str) -> Output {
    let (p, f, q) = (&s.printer, &s.filament, &s.process);
    let bed = p.bed_size();
    let off = if p.origin_center { [-bed.0 / 2.0, -bed.1 / 2.0] } else { [0.0, 0.0] };
    let v = vars(s, plan, bed);
    let mut warnings = vec![];
    let fil_area = f.area();
    let e_per = |w: f64, h: f64, flow: f64| bead_area(w, h) * flow * f.flow_ratio / fil_area;
    let mut w = Writer { s, pos: [0.0, 0.0, 0.0], retracted: 0.0, hopped: false, accel: 0.0, trail: vec![] };
    // the start G-code leaves the nozzle somewhere near the front left; travel from there
    w.pos = [plan.first_min[0], plan.first_min[1], q.first_layer_height + 0.6];
    let mut layer_ops: Vec<Vec<Op>> = Vec::with_capacity(plan.layers.len());
    let mut fan_layer: Vec<f64> = vec![];
    let mut prev_z = 0.0;
    for (li, layer) in plan.layers.iter().enumerate() {
        let mut ops: Vec<Op> = vec![];
        let first = li == 0;
        ops.push(Op::Comment(format!(";LAYER_CHANGE\n;Z:{}\n;HEIGHT:{}", template::fmt_num(layer.z), template::fmt_num(layer.h))));
        if !p.layer_gcode.trim().is_empty() {
            let mut lv = v.clone();
            lv.insert("layer_num".into(), Val::Num(li as f64));
            lv.insert("layer_z".into(), Val::Num(layer.z));
            let (txt, e) = template::render(&p.layer_gcode, &lv);
            if li == 0 {
                warnings.extend(e.into_iter().map(|e| format!("Schichtwechsel-G-Code: {}", e)));
            }
            ops.push(Op::Raw(txt.trim_end().to_string()));
        }
        if matches!(p.flavor, Flavor::Klipper) {
            ops.push(Op::Raw(format!("SET_PRINT_STATS_INFO CURRENT_LAYER={}", li + 1)));
        }
        if li == 1 {
            if (f.nozzle_temp - f.nozzle_temp_first).abs() > 0.1 {
                ops.push(Op::Raw(format!("M104 S{}", template::fmt_num(f.nozzle_temp))));
            }
            if (f.bed_temp - f.bed_temp_first).abs() > 0.1 {
                ops.push(Op::Raw(format!("M140 S{}", template::fmt_num(f.bed_temp))));
            }
        }
        // layer change
        if p.retract_on_layer_change && li > 0 {
            w.retract(&mut ops, true);
        }
        let mut cur_role: Option<Role> = None;
        for path in &layer.paths {
            let start = path.start();
            let z = layer.z;
            if path.spiral {
                // a spiral layer starts where the last one ended, already at its height
                let z0 = prev_z;
                w.travel(&mut ops, start, z0, layer, first, false);
                w.before_extrude(&mut ops, z0);
            } else {
                w.travel(&mut ops, start, z, layer, first, false);
                w.before_extrude(&mut ops, z);
            }
            if cur_role != Some(path.role) {
                ops.push(Op::Comment(format!(";TYPE:{}", path.role.gcode_type())));
                ops.push(Op::Comment(format!(";WIDTH:{}", template::fmt_num((path.width * 1000.0).round() / 1000.0))));
                cur_role = Some(path.role);
            }
            if (w.accel - path.accel).abs() > 0.5 {
                w.accel = path.accel;
                ops.push(Op::Accel(path.accel));
            }
            let mut pts = path.pts.clone();
            if path.closed {
                pts.push(path.pts[0]);
            }
            let e_mm = e_per(path.width, path.height, path.flow);
            let total = path.len().max(1e-9);
            let mut run = 0.0;
            w.trail.clear();
            w.trail.push(w.pos);
            for k in 1..pts.len() {
                let (a, b) = (pts[k - 1], pts[k]);
                let l = poly::dist(a, b);
                if l < 1e-6 {
                    continue;
                }
                run += l;
                let z = if path.spiral { prev_z + (layer.z - prev_z) * (run / total) } else { layer.z };
                ops.push(Op::Ext([b[0], b[1], z], l * e_mm, path.speed, path.role, path.width, path.height));
                w.pos = [b[0], b[1], z];
                w.trail.push(w.pos);
            }
        }
        // fan for the layer, from its estimated time (applied below with the slowdown)
        layer_ops.push(ops);
        prev_z = layer.z;
        fan_layer.push(0.0);
    }
    // ---- cooling: slow down short layers, fan by layer time ----
    let scv = p.corner_velocity.max(0.1);
    let layer_time = |ops: &Vec<Op>, start: [f64; 3], travel_accel: f64| -> (f64, f64) {
        // (extrusion time, other time) of a layer, roughly with acceleration
        let mut blocks = vec![];
        let mut is_ext = vec![];
        let mut pos = start;
        let mut acc = travel_accel;
        for op in ops {
            match op {
                Op::Accel(a) => acc = *a,
                Op::Ext(b, _, v, ..) | Op::Travel(b, v) | Op::Wipe(b, _, v) => {
                    let d = [b[0] - pos[0], b[1] - pos[1], b[2] - pos[2]];
                    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    if l > 1e-9 {
                        blocks.push(Block { len: l, v: *v, accel: acc, dir: [d[0] / l, d[1] / l, d[2] / l], e_only: false });
                        is_ext.push(matches!(op, Op::Ext(..)));
                    }
                    pos = *b;
                }
                Op::Retract(e, v) | Op::Unretract(e, v) => {
                    blocks.push(Block { len: *e, v: *v, accel: 5000.0, dir: [0.0; 3], e_only: true });
                    is_ext.push(false);
                }
                _ => {}
            }
        }
        let t = estimate(&blocks, scv);
        let ext: f64 = t.iter().zip(&is_ext).filter(|(_, e)| **e).map(|(t, _)| t).sum();
        (ext, t.iter().sum::<f64>() - ext)
    };
    let mut start = [plan.first_min[0], plan.first_min[1], q.first_layer_height];
    let travel_accel = q.travel_accel.min(p.max_accel);
    for (li, ops) in layer_ops.iter_mut().enumerate() {
        let (ext, other) = layer_time(ops, start, travel_accel);
        let total = ext + other;
        if f.slow_down_for_layer_cooling && total < f.slow_down_layer_time && ext > 1e-6 {
            // stretch the extrusions so the layer takes the minimum time, not below the
            // minimum speed
            let want = (f.slow_down_layer_time - other).max(ext);
            let k = ext / want;
            for op in ops.iter_mut() {
                if let Op::Ext(_, _, v, ..) = op {
                    *v = (*v * k).max(f.slow_down_min_speed.min(*v));
                }
            }
        }
        let (ext2, other2) = layer_time(ops, start, travel_accel);
        let t = ext2 + other2;
        let fan = if (li as u32) < f.fan_off_layers {
            0.0
        } else if t < f.slow_down_layer_time {
            f.fan_max
        } else if t < f.fan_cooling_layer_time {
            let span = (f.fan_cooling_layer_time - f.slow_down_layer_time).max(1e-6);
            f.fan_max - (f.fan_max - f.fan_min) * (t - f.slow_down_layer_time) / span
        } else if f.fan_always_on {
            f.fan_min
        } else {
            0.0
        };
        fan_layer[li] = fan.clamp(0.0, 1.0);
        for op in ops.iter().rev() {
            if let Op::Ext(b, ..) | Op::Travel(b, _) | Op::Wipe(b, ..) = op {
                start = *b;
                break;
            }
        }
    }
    // ---- moves, times and text ----
    let mut body = String::with_capacity(plan.layers.len() * 4096);
    let mut moves: Vec<Move> = vec![];
    let mut layer_start = vec![];
    let mut blocks: Vec<Block> = vec![];
    let mut block_move: Vec<Option<usize>> = vec![];
    let mut pos = [plan.first_min[0], plan.first_min[1], q.first_layer_height + 0.6];
    let mut acc = travel_accel;
    let mut fan_now = -1.0;
    let mut feed_now = -1.0;
    let mut role_now = Role::OuterWall;
    let mut stats = Stats { layers: plan.layers.len(), max_z: plan.max_z, ..Default::default() };
    let x = |v: f64, k: usize| template::fmt_num(((v + off[k]) * 1000.0).round() / 1000.0);
    let zf = |v: f64| template::fmt_num((v * 1000.0).round() / 1000.0);
    let ef = |e: f64| {
        let s = format!("{:.5}", e);
        let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        if s == "-0" {
            "0".to_string()
        } else {
            s
        }
    };
    let temp_of = |li: usize| if li == 0 { f.nozzle_temp_first } else { f.nozzle_temp };
    for (li, ops) in layer_ops.iter().enumerate() {
        layer_start.push(moves.len());
        let base_fan = fan_layer[li];
        for op in ops {
            match op {
                Op::Comment(c) => {
                    if c.starts_with(";TYPE:") {
                        // bridges and overhangs get the overhang fan
                        let r = Role::ALL.iter().find(|r| c[6..] == *r.gcode_type()).cloned();
                        if let Some(r) = r {
                            role_now = r;
                            let want = if matches!(r, Role::Bridge | Role::OverhangWall) && (li as u32) >= f.fan_off_layers { base_fan.max(f.overhang_fan) } else { base_fan };
                            if (want - fan_now).abs() > 1e-3 {
                                let _ = writeln!(body, "{}", c);
                                let _ = writeln!(body, "M106 S{}", (want * 255.0).round() as i32);
                                fan_now = want;
                                continue;
                            }
                        }
                    }
                    let _ = writeln!(body, "{}", c);
                }
                Op::Raw(r) => {
                    if !r.is_empty() {
                        let _ = writeln!(body, "{}", r);
                    }
                }
                Op::Accel(a) => {
                    acc = *a;
                    match p.flavor {
                        Flavor::Klipper => {
                            let _ = writeln!(body, "SET_VELOCITY_LIMIT ACCEL={}", a.round() as i64);
                        }
                        Flavor::Marlin | Flavor::Bambu => {
                            let _ = writeln!(body, "M204 S{}", a.round() as i64);
                        }
                        Flavor::RepRapFirmware => {
                            let _ = writeln!(body, "M204 P{} T{}", a.round() as i64, a.round() as i64);
                        }
                    }
                }
                Op::Ext(b, e, v, role, wd, h) => {
                    let d = [b[0] - pos[0], b[1] - pos[1], b[2] - pos[2]];
                    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    let mut line = format!("G1 X{} Y{}", x(b[0], 0), x(b[1], 1));
                    if (b[2] - pos[2]).abs() > 1e-6 {
                        let _ = write!(line, " Z{}", zf(b[2]));
                    }
                    let _ = write!(line, " E{}", ef(*e));
                    if (v - feed_now).abs() > 1e-6 {
                        let _ = write!(line, " F{}", (v * 60.0).round() as i64);
                        feed_now = *v;
                    }
                    let _ = writeln!(body, "{}", line);
                    moves.push(Move {
                        a: [pos[0] as f32, pos[1] as f32, pos[2] as f32],
                        b: [b[0] as f32, b[1] as f32, b[2] as f32],
                        kind: Kind::Extrude,
                        role: *role,
                        width: *wd as f32,
                        height: *h as f32,
                        speed: *v as f32,
                        fan: fan_now.max(0.0) as f32,
                        temp: temp_of(li) as f32,
                        e: *e as f32,
                        layer: li as u32,
                        t: 0.0,
                    });
                    *stats.role_filament.entry(*role).or_insert(0.0) += e;
                    stats.filament_mm += e;
                    if l > 1e-9 {
                        blocks.push(Block { len: l, v: *v, accel: acc, dir: [d[0] / l, d[1] / l, d[2] / l], e_only: false });
                        block_move.push(Some(moves.len() - 1));
                    }
                    pos = *b;
                }
                Op::Travel(b, v) | Op::Wipe(b, _, v) => {
                    let is_wipe = matches!(op, Op::Wipe(..));
                    let d = [b[0] - pos[0], b[1] - pos[1], b[2] - pos[2]];
                    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    if l < 1e-9 {
                        continue;
                    }
                    let xy = d[0].abs() > 1e-9 || d[1].abs() > 1e-9;
                    let mut line = String::from(if is_wipe { "G1" } else { "G0" });
                    if xy {
                        let _ = write!(line, " X{} Y{}", x(b[0], 0), x(b[1], 1));
                    }
                    if (b[2] - pos[2]).abs() > 1e-6 {
                        let _ = write!(line, " Z{}", zf(b[2]));
                    }
                    let mut e = 0.0;
                    if let Op::Wipe(_, we, _) = op {
                        e = -we;
                        let _ = write!(line, " E{}", ef(e));
                    }
                    if (v - feed_now).abs() > 1e-6 {
                        let _ = write!(line, " F{}", (v * 60.0).round() as i64);
                        feed_now = *v;
                    }
                    let _ = writeln!(body, "{}", line);
                    stats.filament_mm += e;
                    moves.push(Move {
                        a: [pos[0] as f32, pos[1] as f32, pos[2] as f32],
                        b: [b[0] as f32, b[1] as f32, b[2] as f32],
                        kind: if is_wipe { Kind::Wipe } else { Kind::Travel },
                        role: role_now,
                        width: 0.0,
                        height: 0.0,
                        speed: *v as f32,
                        fan: fan_now.max(0.0) as f32,
                        temp: temp_of(li) as f32,
                        e: e as f32,
                        layer: li as u32,
                        t: 0.0,
                    });
                    blocks.push(Block { len: l, v: *v, accel: acc, dir: [d[0] / l, d[1] / l, d[2] / l], e_only: false });
                    block_move.push(Some(moves.len() - 1));
                    pos = *b;
                }
                Op::Retract(e, v) | Op::Unretract(e, v) => {
                    let sign = if matches!(op, Op::Retract(..)) { -1.0 } else { 1.0 };
                    let _ = writeln!(body, "G1 E{} F{}", ef(sign * e), (v * 60.0).round() as i64);
                    feed_now = *v;
                    stats.filament_mm += sign * e;
                    moves.push(Move {
                        a: [pos[0] as f32, pos[1] as f32, pos[2] as f32],
                        b: [pos[0] as f32, pos[1] as f32, pos[2] as f32],
                        kind: if sign < 0.0 { Kind::Retract } else { Kind::Unretract },
                        role: role_now,
                        width: 0.0,
                        height: 0.0,
                        speed: *v as f32,
                        fan: fan_now.max(0.0) as f32,
                        temp: temp_of(li) as f32,
                        e: (sign * e) as f32,
                        layer: li as u32,
                        t: 0.0,
                    });
                    blocks.push(Block { len: *e, v: *v, accel: 5000.0, dir: [0.0; 3], e_only: true });
                    block_move.push(Some(moves.len() - 1));
                }
            }
        }
    }
    layer_start.push(moves.len());
    // ---- time ----
    let t = estimate(&blocks, scv);
    let mut layer_time = vec![0.0; plan.layers.len()];
    let mut clock = 0.0;
    let mut last_move = 0usize;
    for (bi, tb) in t.iter().enumerate() {
        clock += tb;
        if let Some(mi) = block_move[bi] {
            let m = &mut moves[mi];
            m.t = clock as f32;
            layer_time[m.layer as usize] += tb;
            match m.kind {
                Kind::Extrude => *stats.role_time.entry(m.role).or_insert(0.0) += tb,
                _ => stats.travel_time += tb,
            }
            last_move = mi;
        }
    }
    // moves without a block (zero length) take the time of the one before
    let mut tprev = 0.0f32;
    for m in moves.iter_mut() {
        if m.t == 0.0 {
            m.t = tprev;
        }
        tprev = m.t;
    }
    let _ = last_move;
    stats.time = clock;
    stats.layer_time = layer_time;
    stats.volume_cm3 = stats.filament_mm * fil_area / 1000.0;
    stats.grams = stats.volume_cm3 * f.density;
    stats.cost = stats.grams / 1000.0 * f.cost;
    // progress (M73) per layer, now that times are known
    let mut with_progress = String::with_capacity(body.len() + plan.layers.len() * 24);
    {
        let mut li = 0usize;
        let mut done = 0.0;
        for line in body.split_inclusive('\n') {
            if line.starts_with(";LAYER_CHANGE") {
                let pct = if stats.time > 0.0 { (done / stats.time * 100.0).floor() } else { 0.0 };
                let rem = ((stats.time - done) / 60.0).ceil().max(0.0);
                if !matches!(p.flavor, Flavor::Bambu) {
                    let _ = writeln!(with_progress, "M73 P{} R{}", pct as i64, rem as i64);
                }
                if li < stats.layer_time.len() {
                    done += stats.layer_time[li];
                }
                li += 1;
            }
            with_progress.push_str(line);
        }
    }
    // ---- assemble ----
    let mut out = String::with_capacity(with_progress.len() + 16384);
    let _ = writeln!(out, "; HEADER_BLOCK_START");
    let _ = writeln!(out, "; generated by Freo {} on {}", env!("CARGO_PKG_VERSION"), now);
    let _ = writeln!(out, "; total layer number: {}", plan.layers.len());
    let _ = writeln!(out, "; estimated printing time (normal mode) = {}", fmt_time(stats.time));
    let _ = writeln!(out, "; filament used [mm] = {:.2}", stats.filament_mm);
    let _ = writeln!(out, "; filament used [cm3] = {:.2}", stats.volume_cm3);
    let _ = writeln!(out, "; filament used [g] = {:.2}", stats.grams);
    let _ = writeln!(out, "; filament cost = {:.2}", stats.cost);
    let _ = writeln!(out, "; max_z_height: {:.2}", plan.max_z);
    let _ = writeln!(out, "; HEADER_BLOCK_END\n");
    if let Some((size, png)) = thumb {
        let data = b64(&png);
        let _ = writeln!(out, "; THUMBNAIL_BLOCK_START");
        let _ = writeln!(out, "; thumbnail begin {}x{} {}", size, size, data.len());
        for chunk in data.as_bytes().chunks(78) {
            let _ = writeln!(out, "; {}", std::str::from_utf8(chunk).unwrap_or(""));
        }
        let _ = writeln!(out, "; thumbnail end");
        let _ = writeln!(out, "; THUMBNAIL_BLOCK_END\n");
    }
    let _ = writeln!(out, "; EXECUTABLE_BLOCK_START");
    if matches!(p.flavor, Flavor::Klipper) {
        let _ = writeln!(out, "SET_PRINT_STATS_INFO TOTAL_LAYER={}", plan.layers.len());
    }
    if p.emit_limits {
        match p.flavor {
            Flavor::Klipper => {
                let _ = writeln!(out, "SET_VELOCITY_LIMIT VELOCITY={} ACCEL={} SQUARE_CORNER_VELOCITY={}", p.max_speed.round(), p.max_accel.round(), template::fmt_num(p.corner_velocity));
            }
            Flavor::Marlin | Flavor::RepRapFirmware => {
                let _ = writeln!(out, "M201 X{a} Y{a} ; max acceleration", a = p.max_accel.round());
                let _ = writeln!(out, "M203 X{v} Y{v} Z{z} ; max feedrate (mm/s)", v = p.max_speed.round(), z = p.max_z_speed.round());
            }
            Flavor::Bambu => {}
        }
    }
    let (start, e1) = template::render(&p.start_gcode, &v);
    warnings.extend(e1.into_iter().map(|e| format!("Start-G-Code: {}", e)));
    let _ = writeln!(out, "{}", start.trim_end());
    let _ = writeln!(out, "G90\nM83 ; relative extrusion");
    if let Some(pa) = f.pressure_advance {
        match p.flavor {
            Flavor::Klipper => {
                let _ = writeln!(out, "SET_PRESSURE_ADVANCE ADVANCE={}", template::fmt_num(pa));
            }
            Flavor::Marlin => {
                let _ = writeln!(out, "M900 K{}", template::fmt_num(pa));
            }
            _ => {}
        }
    }
    let _ = writeln!(out, "G92 E0");
    out.push_str(&with_progress);
    let _ = writeln!(out, "M73 P100 R0");
    let (end, e2) = template::render(&p.end_gcode, &v);
    warnings.extend(e2.into_iter().map(|e| format!("End-G-Code: {}", e)));
    let _ = writeln!(out, "; filament used [mm] = {:.2}", stats.filament_mm);
    let _ = writeln!(out, "{}", end.trim_end());
    let _ = writeln!(out, "; EXECUTABLE_BLOCK_END\n");
    let _ = writeln!(out, "; CONFIG_BLOCK_START");
    for (k, val) in [
        ("printer_model", p.name()),
        ("nozzle_diameter", template::fmt_num(p.nozzle)),
        ("filament_type", f.material.code().to_string()),
        ("filament_settings_id", f.name.clone()),
        ("filament_colour", f.hex()),
        ("filament_diameter", template::fmt_num(f.diameter)),
        ("filament_density", template::fmt_num(f.density)),
        ("nozzle_temperature", template::fmt_num(f.nozzle_temp)),
        ("bed_temperature", template::fmt_num(f.bed_temp)),
        ("layer_height", template::fmt_num(q.layer_height)),
        ("initial_layer_print_height", template::fmt_num(q.first_layer_height)),
        ("wall_loops", q.wall_loops.to_string()),
        ("top_shell_layers", q.top_layers.to_string()),
        ("bottom_shell_layers", q.bottom_layers.to_string()),
        ("sparse_infill_density", format!("{}%", template::fmt_num(q.infill_density * 100.0))),
        ("sparse_infill_pattern", q.infill_pattern.name().to_string()),
        ("enable_support", (q.support as i32).to_string()),
        ("spiral_mode", (q.spiral_vase as i32).to_string()),
    ] {
        let _ = writeln!(out, "; {} = {}", k, val);
    }
    let _ = writeln!(out, "; CONFIG_BLOCK_END");
    Output { gcode: out, moves, layer_start, stats, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trapezoids() {
        // 100 mm at 100 mm/s, 1000 mm/s²: 0.1 s up, 0.1 s down, 0.8 s cruise
        let b = vec![Block { len: 100.0, v: 100.0, accel: 1000.0, dir: [1.0, 0.0, 0.0], e_only: false }];
        let t = estimate(&b, 5.0);
        assert!((t[0] - 1.1).abs() < 1e-9, "{}", t[0]);
        // straight continuation keeps the speed: two halves take as long as the whole
        let b2 = vec![
            Block { len: 50.0, v: 100.0, accel: 1000.0, dir: [1.0, 0.0, 0.0], e_only: false },
            Block { len: 50.0, v: 100.0, accel: 1000.0, dir: [1.0, 0.0, 0.0], e_only: false },
        ];
        let t2: f64 = estimate(&b2, 5.0).iter().sum();
        assert!((t2 - 1.1).abs() < 1e-6, "{}", t2);
        // a reversal stops in between
        let b3 = vec![
            Block { len: 50.0, v: 100.0, accel: 1000.0, dir: [1.0, 0.0, 0.0], e_only: false },
            Block { len: 50.0, v: 100.0, accel: 1000.0, dir: [-1.0, 0.0, 0.0], e_only: false },
        ];
        let t3: f64 = estimate(&b3, 5.0).iter().sum();
        assert!((t3 - 1.2).abs() < 1e-6, "{}", t3);
        assert_eq!(fmt_time(3723.0), "1h 2m 3s");
        assert_eq!(b64(b"Man"), "TWFu");
        assert_eq!(b64(b"Ma"), "TWE=");
    }
}
