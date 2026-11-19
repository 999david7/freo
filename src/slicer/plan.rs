//! From slices to ordered tool paths: walls, top/bottom shells, bridges, sparse infill,
//! support, skirt and brim, ironing, fuzzy skin, seams, overhang detection, speeds, and the
//! order everything is printed in on each layer.

use super::config::*;
use super::infill::{self, Line};
use super::mesh::Mesh;
use super::poly::{self, Contour, Pt, Shape, Shapes};
use super::slice;
use super::support;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum Role {
    OuterWall,
    InnerWall,
    OverhangWall,
    TopSurface,
    BottomSurface,
    InternalSolid,
    SparseInfill,
    Bridge,
    Ironing,
    Skirt,
    Brim,
    Support,
    SupportInterface,
}

impl Role {
    pub const ALL: [Role; 13] = [
        Role::OuterWall,
        Role::InnerWall,
        Role::OverhangWall,
        Role::TopSurface,
        Role::BottomSurface,
        Role::InternalSolid,
        Role::SparseInfill,
        Role::Bridge,
        Role::Ironing,
        Role::Skirt,
        Role::Brim,
        Role::Support,
        Role::SupportInterface,
    ];
    pub fn name(&self) -> &'static str {
        match self {
            Role::OuterWall => "Außenwand",
            Role::InnerWall => "Innenwand",
            Role::OverhangWall => "Überhangwand",
            Role::TopSurface => "Obere Fläche",
            Role::BottomSurface => "Untere Fläche",
            Role::InternalSolid => "Massive Füllung",
            Role::SparseInfill => "Füllung",
            Role::Bridge => "Brücke",
            Role::Ironing => "Glätten",
            Role::Skirt => "Schürze",
            Role::Brim => "Rand",
            Role::Support => "Stützen",
            Role::SupportInterface => "Stützen-Schnittstelle",
        }
    }
    /// G-code `;TYPE:` comment as PrusaSlicer/Orca write it (understood by Klipper/Moonraker
    /// viewers and G-code analysers).
    pub fn gcode_type(&self) -> &'static str {
        match self {
            Role::OuterWall => "Outer wall",
            Role::InnerWall => "Inner wall",
            Role::OverhangWall => "Overhang wall",
            Role::TopSurface => "Top surface",
            Role::BottomSurface => "Bottom surface",
            Role::InternalSolid => "Internal solid infill",
            Role::SparseInfill => "Sparse infill",
            Role::Bridge => "Bridge",
            Role::Ironing => "Ironing",
            Role::Skirt => "Skirt",
            Role::Brim => "Brim",
            Role::Support => "Support",
            Role::SupportInterface => "Support interface",
        }
    }
    /// Orca's feature colours.
    pub fn color(&self) -> [u8; 3] {
        match self {
            Role::OuterWall => [255, 125, 56],
            Role::InnerWall => [255, 230, 77],
            Role::OverhangWall => [0, 0, 255],
            Role::TopSurface => [240, 64, 64],
            Role::BottomSurface => [102, 164, 255],
            Role::InternalSolid => [150, 84, 204],
            Role::SparseInfill => [176, 48, 41],
            Role::Bridge => [77, 128, 186],
            Role::Ironing => [255, 140, 105],
            Role::Skirt => [0, 135, 110],
            Role::Brim => [0, 135, 110],
            Role::Support => [0, 255, 0],
            Role::SupportInterface => [0, 128, 0],
        }
    }
    pub fn is_wall(&self) -> bool {
        matches!(self, Role::OuterWall | Role::InnerWall | Role::OverhangWall)
    }
}

/// One extrusion: a polyline printed at constant width, height, flow and speed.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub role: Role,
    pub pts: Vec<Pt>,
    /// the path returns to its first point
    pub closed: bool,
    pub width: f64,
    pub height: f64,
    /// flow multiplier on top of the filament's flow ratio (bridges, ironing)
    pub flow: f64,
    /// mm/s
    pub speed: f64,
    /// mm/s²
    pub accel: f64,
    /// spiral vase: z climbs one layer height along the path
    pub spiral: bool,
}

impl Path {
    pub fn len(&self) -> f64 {
        let mut l = poly::polyline_len(&self.pts);
        if self.closed && self.pts.len() > 2 {
            l += poly::dist(self.pts[0], *self.pts.last().unwrap());
        }
        l
    }
    pub fn start(&self) -> Pt {
        self.pts[0]
    }
    pub fn end(&self) -> Pt {
        if self.closed {
            self.pts[0]
        } else {
            *self.pts.last().unwrap()
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Layer {
    /// top of the layer (nozzle height)
    pub z: f64,
    pub h: f64,
    pub paths: Vec<Path>,
    /// inside the walls: travel here needs no retraction (Orca "reduce infill retraction")
    pub inner: Shapes,
    /// the part's outline on this layer
    pub outline: Shapes,
}

#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub layers: Vec<Layer>,
    pub warnings: Vec<String>,
    /// lowest corner of the first layer's extrusions (Orca's first_layer_print_min)
    pub first_min: Pt,
    pub first_max: Pt,
    pub max_z: f64,
}

pub struct Progress {
    pub cancel: AtomicBool,
    /// 0..=1000
    pub done: AtomicUsize,
    pub stage: std::sync::Mutex<String>,
}

impl Default for Progress {
    fn default() -> Self {
        Progress { cancel: AtomicBool::new(false), done: AtomicUsize::new(0), stage: std::sync::Mutex::new(String::new()) }
    }
}

impl Progress {
    fn set(&self, stage: &str, permille: usize) {
        if let Ok(mut s) = self.stage.lock() {
            *s = stage.into();
        }
        self.done.store(permille.min(1000), Ordering::Relaxed);
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

const CANCELLED: &str = "Slicen abgebrochen.";

/// Deterministic random numbers (fuzzy skin, random seams).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A fill region with its lines, ready to be ordered.
struct Fill {
    role: Role,
    lines: Vec<Line>,
    /// region the lines may be joined in (line centres)
    region: Shapes,
    width: f64,
    spacing: f64,
    flow: f64,
    /// keep the generated order (monotonic)
    keep_order: bool,
}

struct WallLoop {
    pts: Contour,
    depth: usize,
    closed: bool,
}

struct Island {
    walls: Vec<WallLoop>,
    fills: Vec<Fill>,
    centre: Pt,
}

#[derive(Default)]
struct LayerWork {
    islands: Vec<Island>,
    support: Vec<Fill>,
    ironing: Vec<Fill>,
    inner: Shapes,
}

fn centroid(s: &Shape) -> Pt {
    let c = &s[0];
    let n = c.len().max(1) as f64;
    [c.iter().map(|p| p[0]).sum::<f64>() / n, c.iter().map(|p| p[1]).sum::<f64>() / n]
}

/// Perimeter centre-line insets: outer loop, first inner loop, further inner loops.
fn wall_insets(q: &Process, h: f64, first: bool) -> (f64, f64, f64, f64) {
    let wo = if first { q.first_layer_width } else { q.outer_wall_width };
    let wi = if first { q.first_layer_width } else { q.inner_wall_width };
    let so = q.spacing(wo, h);
    let si = q.spacing(wi, h);
    (wo / 2.0, wo / 2.0 + (so + si) / 2.0, si, wo)
}

/// The area enclosed by the walls of a region and the wall loops themselves.
fn walls_of(region: &Shapes, q: &Process, h: f64, first: bool, loops: usize) -> (Vec<WallLoop>, Shapes) {
    let (d0, d1, si, wo) = wall_insets(q, h, first);
    let so = q.spacing(wo, h);
    let mut out = vec![];
    let mut inner = region.clone();
    for k in 0..loops {
        let d = if k == 0 { d0 } else { d1 + (k - 1) as f64 * si };
        let ring = poly::offset(region, -d);
        if ring.is_empty() {
            break;
        }
        for sh in &ring {
            for c in sh {
                out.push(WallLoop { pts: c.clone(), depth: k, closed: true });
            }
        }
        // inner edge of this loop (spacing wise): where infill or the next loop starts
        inner = poly::offset(region, -(d + if k == 0 { so / 2.0 } else { si / 2.0 }));
    }
    if loops == 0 {
        inner = region.clone();
    }
    (out, inner)
}

fn fill_region(boundary: &Shapes, area: &Shapes, spacing: f64, overlap: f64) -> Shapes {
    // centre lines half a spacing inside the walls, reaching `overlap` back into them
    let r = poly::inter(boundary, area);
    poly::offset(&r, -(spacing / 2.0) + overlap * spacing)
}

/// Splits a closed wall loop (starting at its seam) into runs of normal and overhanging wall.
fn split_overhang(pts: &[Pt], closed: bool, lower: Option<&Shapes>, role: Role) -> Vec<(Role, Vec<Pt>)> {
    let mut ring: Vec<Pt> = pts.to_vec();
    if closed {
        ring.push(pts[0]);
    }
    let Some(lower) = lower else { return vec![(role, ring)] };
    // resample long segments so the classification follows the shape
    let mut fine: Vec<Pt> = vec![ring[0]];
    for w in ring.windows(2) {
        let l = poly::dist(w[0], w[1]);
        let n = (l / 1.5).ceil().max(1.0) as usize;
        for k in 1..=n {
            let t = k as f64 / n as f64;
            fine.push([w[0][0] + (w[1][0] - w[0][0]) * t, w[0][1] + (w[1][1] - w[0][1]) * t]);
        }
    }
    let mut runs: Vec<(Role, Vec<Pt>)> = vec![];
    for w in fine.windows(2) {
        let m = [(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0];
        let r = if poly::in_shapes(lower, m) { role } else { Role::OverhangWall };
        match runs.last_mut() {
            Some((lr, v)) if *lr == r => v.push(w[1]),
            _ => runs.push((r, vec![w[0], w[1]])),
        }
    }
    // merge tiny overhang blips back (less than a millimetre)
    let mut merged: Vec<(Role, Vec<Pt>)> = vec![];
    for (r, v) in runs {
        let short = poly::polyline_len(&v) < 1.0;
        match merged.last_mut() {
            Some((lr, lv)) if *lr == r || short => lv.extend_from_slice(&v[1..]),
            _ => merged.push((r, v)),
        }
    }
    merged.into_iter().map(|(r, v)| (r, poly::simplify_polyline(&v, 0.002))).collect()
}

fn fuzz(pts: &[Pt], closed: bool, dist: f64, thick: f64, rng: &mut Rng) -> Vec<Pt> {
    let n = pts.len();
    if n < 3 || dist <= 0.0 || thick <= 0.0 {
        return pts.to_vec();
    }
    let ccw = poly::contour_area(&pts.to_vec()) > 0.0;
    let mut out = vec![];
    let segs = if closed { n } else { n - 1 };
    for i in 0..segs {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let l = poly::dist(a, b);
        if l < 1e-9 {
            continue;
        }
        let (dx, dy) = ((b[0] - a[0]) / l, (b[1] - a[1]) / l);
        // outward normal: right of the direction on a counter-clockwise loop
        let (nx, ny) = if ccw { (dy, -dx) } else { (-dy, dx) };
        let k = (l / dist).ceil().max(1.0) as usize;
        for j in 0..k {
            let t = j as f64 / k as f64 * l;
            let r = (rng.next() * 2.0 - 1.0) * thick;
            out.push([a[0] + dx * t + nx * r, a[1] + dy * t + ny * r]);
        }
    }
    if !closed {
        out.push(pts[n - 1]);
    }
    out
}

fn rotate_to(c: &Contour, i: usize) -> Contour {
    c[i..].iter().chain(c[..i].iter()).cloned().collect()
}

fn seam_index(c: &Contour, q: &Process, prev: &[Pt], pos: Pt, rng: &mut Rng) -> usize {
    let n = c.len();
    let back = || (0..n).max_by(|&i, &j| c[i][1].partial_cmp(&c[j][1]).unwrap()).unwrap_or(0);
    match q.seam {
        Seam::Back => back(),
        Seam::Random => ((rng.next() * n as f64) as usize).min(n - 1),
        Seam::Nearest => (0..n).min_by(|&i, &j| poly::dist2(c[i], pos).partial_cmp(&poly::dist2(c[j], pos)).unwrap()).unwrap_or(0),
        Seam::Aligned => {
            // the previous layer's seam if it is close to this loop
            let near = prev
                .iter()
                .filter_map(|p| {
                    let (i, d) = (0..n).map(|i| (i, poly::dist2(c[i], *p))).min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())?;
                    (d < 9.0).then_some((i, d))
                })
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
            if let Some((i, _)) = near {
                return i;
            }
            // otherwise the sharpest concave corner (hides the seam), then the back
            let ccw = poly::contour_area(c) > 0.0;
            let mut best = (back(), 0.35);
            for i in 0..n {
                let (a, b, d) = (c[(i + n - 1) % n], c[i], c[(i + 1) % n]);
                let (u, v) = ([b[0] - a[0], b[1] - a[1]], [d[0] - b[0], d[1] - b[1]]);
                let (lu, lv) = ((u[0] * u[0] + u[1] * u[1]).sqrt(), (v[0] * v[0] + v[1] * v[1]).sqrt());
                if lu < 1e-6 || lv < 1e-6 {
                    continue;
                }
                let cross = (u[0] * v[1] - u[1] * v[0]) / (lu * lv);
                // concave: turning right on a counter-clockwise loop
                let concave = if ccw { -cross } else { cross };
                if concave > best.1 {
                    best = (i, concave);
                }
            }
            best.0
        }
    }
}

/// Greedy nearest-neighbour order of open/closed lines, flipping open ones as needed.
fn order_lines(lines: Vec<Line>, pos: &mut Pt) -> Vec<Line> {
    let mut left: Vec<Option<Line>> = lines.into_iter().filter(|l| !l.is_empty()).map(Some).collect();
    let mut out = Vec::with_capacity(left.len());
    for _ in 0..left.len() {
        let mut best: Option<(usize, bool, f64)> = None;
        for (i, l) in left.iter().enumerate() {
            let Some(l) = l else { continue };
            let d0 = poly::dist2(*pos, l.pts[0]);
            if best.is_none_or(|b| d0 < b.2) {
                best = Some((i, false, d0));
            }
            if !l.closed {
                let d1 = poly::dist2(*pos, *l.pts.last().unwrap());
                if best.is_none_or(|b| d1 < b.2) {
                    best = Some((i, true, d1));
                }
            }
        }
        let Some((i, flip, _)) = best else { break };
        let mut l = left[i].take().unwrap();
        if l.closed {
            // start a closed loop at the vertex nearest the nozzle
            let k = (0..l.pts.len()).min_by(|&a, &b| poly::dist2(*pos, l.pts[a]).partial_cmp(&poly::dist2(*pos, l.pts[b])).unwrap()).unwrap();
            l.pts = rotate_to(&l.pts, k);
            *pos = l.pts[0];
        } else {
            if flip {
                l.pts.reverse();
            }
            *pos = *l.pts.last().unwrap();
        }
        out.push(l);
    }
    out
}

/// Joins consecutive open lines whose ends are close by a straight extrusion that runs along
/// the region's edge (like Orca's infill anchors), instead of a travel and retraction for
/// every line. Joins that would cut across the inside are left as travels.
fn connect(lines: Vec<Line>, region: &Shapes, max_gap: f64, width: f64) -> Vec<Line> {
    let mut out: Vec<Line> = vec![];
    let grown = poly::offset(region, 0.02);
    let core = poly::offset(region, -(width * 0.75).max(0.2));
    let grown = poly::diff(&grown, &core);
    for l in lines {
        if let Some(last) = out.last_mut() {
            if !last.closed && !l.closed {
                let a = *last.pts.last().unwrap();
                let b = l.pts[0];
                let d = poly::dist(a, b);
                if d <= max_gap && (d < 1e-6 || poly::segment_inside(&grown, a, b)) {
                    last.pts.extend(l.pts);
                    continue;
                }
            }
        }
        out.push(l);
    }
    out
}

struct Speeds<'a> {
    q: &'a Process,
    f: &'a Filament,
    p: &'a Printer,
}

impl Speeds<'_> {
    fn of(&self, role: Role, layer: usize, w: f64, h: f64) -> (f64, f64) {
        let q = self.q;
        let base = match role {
            Role::OuterWall => q.outer_wall_speed,
            Role::InnerWall => q.inner_wall_speed,
            Role::OverhangWall => q.overhang_speed,
            Role::TopSurface => q.top_speed,
            Role::BottomSurface | Role::InternalSolid => q.solid_speed,
            Role::SparseInfill => q.sparse_speed,
            Role::Bridge => q.bridge_speed,
            Role::Ironing => q.ironing_speed,
            Role::Skirt | Role::Brim => q.skirt_speed,
            Role::Support => q.support_speed,
            Role::SupportInterface => q.interface_speed,
        };
        let first = match role {
            Role::SparseInfill | Role::InternalSolid | Role::BottomSurface | Role::TopSurface => q.first_layer_infill_speed,
            _ => q.first_layer_speed,
        };
        let mut v = if layer == 0 {
            first.min(base)
        } else if (layer as u32) < q.slow_down_layers {
            // Orca: speeds ramp up from the first layer's over the first few layers
            let t = layer as f64 / q.slow_down_layers as f64;
            first.min(base) + (base - first.min(base)) * t
        } else {
            base
        };
        if !matches!(role, Role::Ironing) {
            // the hot end melts only so much plastic per second
            let vol = bead_area(w, h);
            if vol > 0.0 && self.f.max_volumetric > 0.0 {
                v = v.min(self.f.max_volumetric / vol);
            }
        }
        v = v.min(self.p.max_speed).max(1.0);
        let accel = if layer == 0 {
            q.first_layer_accel
        } else {
            match role {
                Role::OuterWall | Role::OverhangWall => q.outer_wall_accel,
                Role::TopSurface | Role::Ironing => q.top_accel,
                _ => q.default_accel,
            }
        };
        (v, accel.min(self.p.max_accel).max(10.0))
    }
}

/// Slices the placed mesh (printer coordinates) and plans every layer's tool paths.
pub fn plan(mesh: &Mesh, s: &Settings, prog: &Progress) -> Result<Plan, String> {
    let errs = s.validate();
    if !errs.is_empty() {
        return Err(errs.join("\n"));
    }
    if mesh.is_empty() {
        return Err("Kein Modell auf dem Druckbett.".into());
    }
    let (p, f, q0) = (&s.printer, &s.filament, &s.process);
    let mut q = q0.clone();
    let mut warnings = vec![];
    if q.spiral_vase {
        // spiral vase: one wall, no top, no infill, no support
        q.wall_loops = 1;
        q.top_layers = 0;
        q.infill_density = 0.0;
        q.support = false;
        q.only_one_wall_top = false;
    }
    let q = &q;
    let (lo, hi) = mesh.bbox();
    if hi.z > p.height + 1e-6 {
        return Err(format!("Das Teil ist {:.1} mm hoch, der Drucker druckt nur {:.0} mm.", hi.z, p.height));
    }
    let bed = p.bed_size();
    let corners = [[lo.x, lo.y], [hi.x, lo.y], [hi.x, hi.y], [lo.x, hi.y]];
    if corners.iter().any(|c| !p.on_bed(c[0], c[1])) {
        warnings.push(format!("Das Teil ragt über das Druckbett ({:.0} × {:.0} mm) hinaus.", bed.0, bed.1));
    }
    prog.set("Schneiden", 10);
    let bounds = slice::layer_bounds(q.first_layer_height, q.layer_height, hi.z);
    if bounds.is_empty() {
        return Err("Das Teil ist zu flach zum Drucken.".into());
    }
    let planes: Vec<f64> = bounds.iter().map(|b| (b.0 + b.1) / 2.0).collect();
    let mut slices = slice::slice_mesh(mesh, &planes, q.resolution);
    if prog.cancelled() {
        return Err(CANCELLED.into());
    }
    // remove empty layers at the top and drop those not touching anything
    while slices.last().is_some_and(|s| s.is_empty()) {
        slices.pop();
    }
    let n = slices.len();
    if n == 0 {
        return Err("Beim Schneiden blieb nichts übrig. Ist das Modell geschlossen?".into());
    }
    if slices[0].is_empty() {
        warnings.push("Die erste Schicht ist leer: das Teil liegt nicht auf dem Druckbett.".into());
    }
    let heights: Vec<f64> = bounds[..n].iter().map(|b| b.1 - b.0).collect();
    // elephant foot compensation on the first layer
    if q.elephant_foot > 0.0 && !slices[0].is_empty() {
        let c = poly::offset(&slices[0], -q.elephant_foot);
        if poly::area(&c) > poly::area(&slices[0]) * 0.5 {
            slices[0] = c;
        }
    }
    prog.set("Stützen", 150);
    let sup = support::generate(&slices, &heights, q, q.support_width);
    if prog.cancelled() {
        return Err(CANCELLED.into());
    }
    prog.set("Wände und Füllung", 250);
    // ---- regions and fills per layer, in parallel ----
    let done = AtomicUsize::new(0);
    let mut work: Vec<LayerWork> = (0..n).map(|_| LayerWork::default()).collect();
    let threads = std::thread::available_parallelism().map(|x| x.get()).unwrap_or(4).clamp(1, 16);
    let chunk = n.div_ceil(threads);
    std::thread::scope(|sc| {
        for (ci, slot) in work.chunks_mut(chunk).enumerate() {
            let (slices, heights, sup, done) = (&slices, &heights, &sup, &done);
            sc.spawn(move || {
                for (k, out) in slot.iter_mut().enumerate() {
                    if prog.cancelled() {
                        return;
                    }
                    let i = ci * chunk + k;
                    *out = layer_work(i, slices, heights, q, f, sup);
                    let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                    prog.done.store(250 + d * 600 / n, Ordering::Relaxed);
                }
            });
        }
    });
    if prog.cancelled() {
        return Err(CANCELLED.into());
    }
    prog.set("Reihenfolge", 860);
    // ---- skirt and brim ----
    let fw = q.first_layer_width;
    let fs = q.spacing(fw, heights[0]);
    let mut brim: Vec<Line> = vec![];
    if !matches!(q.brim, Brim::None) && q.brim_width > 0.0 && !slices[0].is_empty() {
        let loops = (q.brim_width / fs).round().max(1.0) as usize;
        if matches!(q.brim, Brim::Outer | Brim::Both) {
            let outer: Shapes = slices[0].iter().map(|sh| vec![sh[0].clone()]).collect();
            let outer = poly::union(&outer, &vec![]);
            for k in (0..loops).rev() {
                for sh in poly::offset_join(&outer, q.brim_gap + fw / 2.0 + k as f64 * fs, true) {
                    brim.push(Line { pts: sh[0].clone(), closed: true });
                }
            }
        }
        if matches!(q.brim, Brim::Inner | Brim::Both) {
            for sh in &slices[0] {
                for hole in &sh[1..] {
                    let hs: Shapes = vec![vec![hole.iter().rev().cloned().collect()]];
                    for k in 0..loops {
                        for r in poly::offset_join(&hs, -(q.brim_gap + fw / 2.0 + k as f64 * fs), true) {
                            for c in r {
                                brim.push(Line { pts: c, closed: true });
                            }
                        }
                    }
                }
            }
        }
    }
    let mut skirt: Vec<Line> = vec![];
    if q.skirt_loops > 0 && !slices[0].is_empty() {
        let mut base = slices[0].clone();
        base = poly::union(&base, &poly::union(&sup.base[0], &sup.interface[0]));
        if !brim.is_empty() {
            let brim_area = poly::offset_join(&base, q.brim_gap + q.brim_width, true);
            base = poly::union(&base, &brim_area);
        }
        let hull: Shapes = poly::union(&base.iter().map(|sh| vec![sh[0].clone()]).collect::<Shapes>(), &vec![]);
        for k in (0..q.skirt_loops as usize).rev() {
            for sh in poly::offset_join(&hull, q.skirt_distance + fw / 2.0 + k as f64 * fs, true) {
                skirt.push(Line { pts: sh[0].clone(), closed: true });
            }
        }
    }
    // ---- ordering, seams, speeds ----
    let sp = Speeds { q, f, p };
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut pos: Pt = [0.0, 0.0];
    let mut prev_seams: Vec<Pt> = vec![];
    let mut layers = Vec::with_capacity(n);
    let mut z = 0.0;
    let mut spiral_from = usize::MAX;
    if q.spiral_vase {
        spiral_from = (q.bottom_layers as usize).max(1);
    }
    for (i, lw) in work.into_iter().enumerate() {
        let h = heights[i];
        z += h;
        let mut layer = Layer { z, h, paths: vec![], inner: lw.inner, outline: slices[i].clone() };
        let first = i == 0;
        let tol = (q.resolution * 0.5).max(1e-4);
        let mk = |role: Role, pts: Vec<Pt>, closed: bool, w: f64, flow: f64| -> Path {
            let (v, a) = sp.of(role, i, w, h);
            // offsets leave near-duplicate points at every corner of a fine curve: thin them
            // out within the resolution (and never keep zero-length moves)
            // closed loops keep their first point: it is the seam
            let pts = if closed && pts.len() > 3 {
                let mut ring = pts.clone();
                ring.push(pts[0]);
                let mut r = poly::simplify_polyline(&ring, tol);
                r.pop();
                if r.len() >= 3 {
                    r
                } else {
                    pts
                }
            } else {
                poly::simplify_polyline(&pts, tol)
            };
            let mut clean: Vec<Pt> = Vec::with_capacity(pts.len());
            for pt in pts {
                if clean.last().is_none_or(|l| poly::dist2(*l, pt) > 1e-8) {
                    clean.push(pt);
                }
            }
            Path { role, pts: clean, closed, width: w, height: h, flow, speed: v, accel: a, spiral: false }
        };
        if (i as u32) < q.skirt_height.max(1) && !skirt.is_empty() {
            for l in order_lines(skirt.clone(), &mut pos) {
                layer.paths.push(mk(Role::Skirt, l.pts, true, fw, 1.0));
            }
        }
        if first {
            for l in brim.iter() {
                let mut l = l.clone();
                let k = (0..l.pts.len()).min_by(|&a, &b| poly::dist2(pos, l.pts[a]).partial_cmp(&poly::dist2(pos, l.pts[b])).unwrap()).unwrap_or(0);
                l.pts = rotate_to(&l.pts, k);
                pos = l.pts[0];
                layer.paths.push(mk(Role::Brim, l.pts, true, fw, 1.0));
            }
        }
        for fill in lw.support {
            let lines = if fill.keep_order { fill.lines } else { order_lines(fill.lines, &mut pos) };
            let lines = connect(lines, &fill.region, fill.spacing * 3.0 + 0.5, fill.width);
            for l in lines {
                if let Some(e) = if l.closed { Some(l.pts[0]) } else { l.pts.last().cloned() } {
                    pos = e;
                }
                layer.paths.push(mk(fill.role, l.pts, l.closed, fill.width, fill.flow));
            }
        }
        // islands nearest first
        let mut islands: Vec<Option<Island>> = lw.islands.into_iter().map(Some).collect();
        let mut seams_here = vec![];
        let lower = if i > 0 && q.detect_overhang_wall { Some(poly::offset(&slices[i - 1], q.outer_wall_width * 0.1)) } else { None };
        for _ in 0..islands.len() {
            let Some(ii) = (0..islands.len()).filter(|&k| islands[k].is_some()).min_by(|&a, &b| {
                let ca = islands[a].as_ref().unwrap().centre;
                let cb = islands[b].as_ref().unwrap().centre;
                poly::dist2(pos, ca).partial_cmp(&poly::dist2(pos, cb)).unwrap()
            }) else {
                break;
            };
            let isl = islands[ii].take().unwrap();
            let maxd = isl.walls.iter().map(|w| w.depth).max().unwrap_or(0);
            let order: Vec<usize> = match q.wall_order {
                WallOrder::InnerOuter => (0..=maxd).rev().collect(),
                WallOrder::OuterInner => (0..=maxd).collect(),
                WallOrder::InnerOuterInner => {
                    let mut v = vec![];
                    if maxd >= 1 {
                        v.push(1);
                    }
                    v.push(0);
                    v.extend(2..=maxd);
                    v
                }
            };
            let spiral = i >= spiral_from;
            for depth in order {
                let mut loops: Vec<&WallLoop> = isl.walls.iter().filter(|w| w.depth == depth).collect();
                // nearest loop first
                let mut sorted = vec![];
                while !loops.is_empty() {
                    let k = (0..loops.len())
                        .min_by(|&a, &b| {
                            let da = loops[a].pts.iter().map(|p| poly::dist2(*p, pos)).fold(f64::INFINITY, f64::min);
                            let db = loops[b].pts.iter().map(|p| poly::dist2(*p, pos)).fold(f64::INFINITY, f64::min);
                            da.partial_cmp(&db).unwrap()
                        })
                        .unwrap();
                    let wl = loops.swap_remove(k);
                    sorted.push(wl);
                    if let Some(l) = sorted.last() {
                        if let Some(e) = l.pts.first() {
                            pos = *e;
                        }
                    }
                }
                for wl in sorted {
                    let role = if depth == 0 { Role::OuterWall } else { Role::InnerWall };
                    let w = if first { q.first_layer_width } else if depth == 0 { q.outer_wall_width } else { q.inner_wall_width };
                    let mut pts = wl.pts.clone();
                    let fuzzy = !first && match q.fuzzy {
                        Fuzzy::None => false,
                        Fuzzy::Outer => depth == 0,
                        Fuzzy::All => true,
                    };
                    if fuzzy {
                        pts = fuzz(&pts, wl.closed, q.fuzzy_point_dist, q.fuzzy_thickness, &mut rng);
                    }
                    if wl.closed {
                        let si = seam_index(&pts, q, &prev_seams, pos, &mut rng);
                        pts = rotate_to(&pts, si);
                        if depth == 0 {
                            seams_here.push(pts[0]);
                        }
                    }
                    if spiral && depth == 0 {
                        let mut path = mk(Role::OuterWall, pts, true, w, 1.0);
                        path.spiral = true;
                        pos = path.start();
                        layer.paths.push(path);
                        continue;
                    }
                    let runs = split_overhang(&pts, wl.closed, if depth == 0 { lower.as_ref() } else { None }, role);
                    let nruns = runs.len();
                    for (ri, (r, mut v)) in runs.into_iter().enumerate() {
                        // seam gap: stop a little before closing the loop
                        if wl.closed && ri + 1 == nruns && q.seam_gap > 0.0 {
                            trim_end(&mut v, q.seam_gap * p.nozzle);
                        }
                        if v.len() >= 2 {
                            let flow = 1.0;
                            let path = mk(r, v, false, w, flow);
                            pos = path.end();
                            layer.paths.push(path);
                        }
                    }
                }
            }
            for fill in isl.fills {
                let lines = if fill.keep_order { fill.lines } else { order_lines(fill.lines, &mut pos) };
                let lines = if q.infill_combine_connect && !matches!(fill.role, Role::Bridge) {
                    connect(lines, &fill.region, (fill.spacing * 3.0).max(fill.width * 2.5), fill.width)
                } else {
                    lines
                };
                for l in lines {
                    if l.pts.len() < 2 {
                        continue;
                    }
                    let path = mk(fill.role, l.pts, l.closed, fill.width, fill.flow);
                    pos = path.end();
                    layer.paths.push(path);
                }
            }
        }
        for fill in lw.ironing {
            let lines = order_lines(fill.lines, &mut pos);
            let lines = connect(lines, &fill.region, fill.spacing * 3.0, fill.width);
            for l in lines {
                let path = mk(Role::Ironing, l.pts, l.closed, fill.width, fill.flow);
                pos = path.end();
                layer.paths.push(path);
            }
        }
        if !seams_here.is_empty() {
            prev_seams = seams_here;
        }
        // drop degenerate paths
        layer.paths.retain(|p| p.pts.len() >= 2 && p.len() > 1e-4);
        layers.push(layer);
        if prog.cancelled() {
            return Err(CANCELLED.into());
        }
    }
    // first layer extent (for start G-code that moves next to the print)
    let mut fmin = [f64::INFINITY; 2];
    let mut fmax = [f64::NEG_INFINITY; 2];
    for pth in &layers[0].paths {
        for pt in &pth.pts {
            fmin = [fmin[0].min(pt[0]), fmin[1].min(pt[1])];
            fmax = [fmax[0].max(pt[0]), fmax[1].max(pt[1])];
        }
    }
    if !fmin[0].is_finite() {
        fmin = [lo.x, lo.y];
        fmax = [hi.x, hi.y];
    }
    // extrusions off the bed
    let off = layers.iter().flat_map(|l| l.paths.iter()).flat_map(|p| p.pts.iter()).any(|pt| !p.on_bed(pt[0], pt[1]));
    if off && warnings.is_empty() {
        warnings.push("Ein Teil der Bahnen (Rand, Schürze oder Stützen) liegt außerhalb des Druckbetts.".into());
    }
    let max_z = layers.last().map(|l| l.z).unwrap_or(0.0);
    prog.set("Fertig", 900);
    Ok(Plan { layers, warnings, first_min: fmin, first_max: fmax, max_z })
}

fn trim_end(v: &mut Vec<Pt>, mut d: f64) {
    while v.len() >= 2 && d > 0.0 {
        let n = v.len();
        let (a, b) = (v[n - 2], v[n - 1]);
        let l = poly::dist(a, b);
        if l > d + 1e-6 {
            let t = (l - d) / l;
            v[n - 1] = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            return;
        }
        if n == 2 {
            return;
        }
        v.pop();
        d -= l;
    }
}

/// Everything about one layer that does not depend on the order of printing.
fn layer_work(i: usize, slices: &[Shapes], heights: &[f64], q: &Process, _f: &Filament, sup: &support::SupportLayers) -> LayerWork {
    let n = slices.len();
    let h = heights[i];
    let first = i == 0;
    let here = &slices[i];
    let mut out = LayerWork::default();
    if here.is_empty() && sup.base[i].is_empty() && sup.interface[i].is_empty() {
        return out;
    }
    let empty: Shapes = vec![];
    let above = if i + 1 < n { &slices[i + 1] } else { &empty };
    let below = if i > 0 { &slices[i - 1] } else { &empty };
    let spiral_layer = q.spiral_vase && i >= (q.bottom_layers as usize).max(1);
    // regions that need solid fill: not covered by `top_layers` layers above or
    // `bottom_layers` layers below
    let cover = |from: isize, count: u32| -> Shapes {
        if count == 0 {
            return slices[i].clone();
        }
        let mut acc: Option<Shapes> = None;
        for k in 1..=count as isize {
            let j = i as isize + from * k;
            if j < 0 || j >= n as isize {
                return vec![];
            }
            acc = Some(match acc {
                None => slices[j as usize].clone(),
                Some(a) => poly::inter(&a, &slices[j as usize]),
            });
        }
        acc.unwrap_or_default()
    };
    let up = cover(1, q.top_layers);
    let down = cover(-1, q.bottom_layers);
    let top_exposed = poly::diff(here, above);
    let mut inner_all: Shapes = vec![];
    let angle = q.infill_angle + if i % 2 == 1 { 90.0 } else { 0.0 };
    for sh in here {
        let isl: Shapes = vec![sh.clone()];
        let mut loops = if spiral_layer { 1 } else { q.wall_loops as usize };
        if q.spiral_vase && !spiral_layer {
            loops = loops.max(1);
        }
        let (mut walls, inner) = walls_of(&isl, q, h, first, loops);
        // Orca "only one wall on top surfaces": inner walls give way to top infill there
        let mut extra_top: Shapes = vec![];
        if q.only_one_wall_top && loops > 1 && !first && !top_exposed.is_empty() {
            // only top areas at least three lines wide (Orca min_width_top_surface): the
            // narrow ledges of slopes keep their inner walls
            let top_here = poly::open(&poly::inter(&isl, &top_exposed), q.top_width * 1.5);
            if poly::area(&top_here) > (q.top_width * 4.0).powi(2) {
                let (_, after_outer) = walls_of(&isl, q, h, first, 1);
                let mut kept = vec![];
                for w in walls.into_iter() {
                    if w.depth == 0 {
                        kept.push(w);
                        continue;
                    }
                    let mut ring = w.pts.clone();
                    ring.push(w.pts[0]);
                    let outside = poly::clip_lines_rule(&[ring], &top_here, true);
                    for piece in outside {
                        if poly::polyline_len(&piece) > q.inner_wall_width * 2.0 {
                            kept.push(WallLoop { pts: piece, depth: w.depth, closed: false });
                        }
                    }
                }
                walls = kept;
                extra_top = poly::diff(&poly::inter(&after_outer, &top_here), &inner);
            }
        }
        let boundary = poly::union(&inner, &extra_top);
        inner_all = poly::union(&inner_all, &boundary);
        let mut fills = vec![];
        if !spiral_layer && !boundary.is_empty() {
            let solid_w = if first { q.first_layer_width } else { q.solid_width };
            let top_w = if first { q.first_layer_width } else { q.top_width };
            let sparse_w = if first { q.first_layer_width } else { q.sparse_width };
            let ss = q.spacing(solid_w, h);
            let ts = q.spacing(top_w, h);
            let fs = q.spacing(sparse_w, h);
            let ov = q.infill_overlap;
            // classification
            let bottom = if first { boundary.clone() } else { poly::diff(&boundary, below) };
            let top = poly::diff(&poly::diff(&boundary, above), &bottom);
            let solid_all = poly::union(&poly::diff(&boundary, &up), &poly::diff(&boundary, &down));
            let solid_all = poly::union(&solid_all, &poly::union(&top, &bottom));
            let mut internal = poly::diff(&poly::diff(&solid_all, &top), &bottom);
            let mut sparse = poly::diff(&boundary, &solid_all);
            // tiny sparse pockets are printed solid (Orca minimum_sparse_infill_area)
            let (small, big): (Shapes, Shapes) = sparse.into_iter().partition(|s| poly::shape_area(s) < q.min_sparse_area);
            sparse = big;
            if !small.is_empty() {
                internal = poly::union(&internal, &small);
            }
            if q.infill_density <= 1e-4 {
                sparse = vec![];
            }
            if q.infill_density >= 0.999 {
                internal = poly::union(&internal, &sparse);
                sparse = vec![];
            }
            // bottom surfaces over air are bridges
            if !bottom.is_empty() {
                let reg = fill_region(&boundary, &bottom, ss, ov);
                if first {
                    let lines = infill::solid(q.bottom_pattern, &reg, ss, angle);
                    fills.push(Fill { role: Role::BottomSurface, lines, region: reg, width: solid_w, spacing: ss, flow: 1.0, keep_order: matches!(q.bottom_pattern, InfillPattern::Monotonic) });
                } else {
                    for bsh in poly::drop_small(reg, ss * ss) {
                        let r1: Shapes = vec![bsh];
                        let a = infill::bridge_angle(&r1, ss);
                        let lines = infill::monotonic_order(infill::parallel_centred(&r1, a, ss), a).into_iter().map(Line::open).collect();
                        fills.push(Fill { role: Role::Bridge, lines, region: r1, width: solid_w, spacing: ss, flow: q.bridge_flow, keep_order: true });
                    }
                }
            }
            if !internal.is_empty() {
                let reg = poly::drop_small(fill_region(&boundary, &internal, ss, ov), ss * ss * 0.5);
                let lines = infill::solid(q.solid_pattern, &reg, ss, angle);
                fills.push(Fill { role: Role::InternalSolid, lines, region: reg, width: solid_w, spacing: ss, flow: 1.0, keep_order: matches!(q.solid_pattern, InfillPattern::Monotonic | InfillPattern::ZigZag) });
            }
            if !sparse.is_empty() {
                let reg = fill_region(&boundary, &sparse, fs, ov);
                let z = heights[..=i].iter().sum::<f64>();
                let lines = infill::sparse(q.infill_pattern, &reg, fs, q.infill_density, q.infill_angle, z, i);
                fills.push(Fill { role: Role::SparseInfill, lines, region: reg, width: sparse_w, spacing: fs / q.infill_density.max(0.01), flow: 1.0, keep_order: false });
            }
            if !top.is_empty() {
                let reg = fill_region(&boundary, &top, ts, ov);
                let lines = infill::solid(q.top_pattern, &reg, ts, angle);
                fills.push(Fill { role: Role::TopSurface, lines, region: reg.clone(), width: top_w, spacing: ts, flow: 1.0, keep_order: matches!(q.top_pattern, InfillPattern::Monotonic) });
                let iron = match q.ironing {
                    Ironing::None => false,
                    Ironing::Top => true,
                    Ironing::Topmost => i + 1 == n,
                };
                if iron && !reg.is_empty() {
                    let ir = poly::offset(&reg, -q.ironing_spacing);
                    let lines: Vec<Line> = infill::parallel_centred(&ir, angle + 90.0, q.ironing_spacing).into_iter().map(Line::open).collect();
                    out.ironing.push(Fill { role: Role::Ironing, lines, region: ir, width: top_w, spacing: q.ironing_spacing, flow: q.ironing_flow, keep_order: false });
                }
            }
        }
        out.islands.push(Island { walls, fills, centre: centroid(sh) });
    }
    out.inner = inner_all;
    // support lines
    let sw = q.support_width;
    let ssp = q.spacing(sw, h);
    if !sup.base[i].is_empty() {
        let lines: Vec<Line> = infill::monotonic_order(infill::parallel(&sup.base[i], q.support_angle, q.support_spacing.max(ssp), 0.0), q.support_angle).into_iter().map(Line::open).collect();
        out.support.push(Fill { role: Role::Support, lines, region: sup.base[i].clone(), width: sw, spacing: q.support_spacing.max(ssp), flow: 1.0, keep_order: true });
    }
    if !sup.interface[i].is_empty() {
        let a = q.support_angle + 90.0;
        let lines: Vec<Line> = infill::monotonic_order(infill::parallel_centred(&sup.interface[i], a, q.interface_spacing.max(ssp)), a).into_iter().map(Line::open).collect();
        out.support.push(Fill { role: Role::SupportInterface, lines, region: sup.interface[i].clone(), width: sw, spacing: q.interface_spacing.max(ssp), flow: 1.0, keep_order: true });
    }
    out
}
