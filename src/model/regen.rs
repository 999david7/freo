//! Regeneration: replays the feature list into a solid, datums and display curves.

use super::expr::Vars;
use super::features::*;
use crate::kernel::builders::{self, TagAllocator};
use crate::kernel::csg::{self, CPolygon};
use crate::kernel::math::*;
use crate::kernel::topology::Topology;
use crate::sketch::{sketch::Sketch, solver};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct PlaneInfo { pub id: u64, pub name: String, pub frame: Frame, /** display half-size around the origin; 0 = sized from the model */ pub size: f64 }
#[derive(Clone, Debug)]
pub struct AxisInfo { pub id: u64, pub name: String, pub point: V3, pub dir: V3, pub feature: u64 }
#[derive(Clone, Debug)]
pub struct PointInfo { pub id: u64, pub name: String, pub p: V3 }
#[derive(Clone, Debug)]
pub struct CsysInfo { pub id: u64, pub name: String, pub frame: Frame }
#[derive(Clone, Debug)]
pub struct CurveInfo { pub feature: u64, pub polylines: Vec<Vec<V3>> }

/// Cosmetic thread: a display-only cylinder at the thread major diameter.
#[derive(Clone, Debug)]
pub struct Cosmetic { pub feature: u64, pub base: V3, pub dir: V3, pub radius: f64, pub length: f64, pub label: String }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op { Add, Cut }

#[derive(Clone)]
pub struct ToolBody { pub polys: Vec<CPolygon>, pub op: Op }

/// A Sammelfläche: the open sheet a surface feature made.
#[derive(Clone)]
pub struct Quilt { pub feature: u64, pub polys: Vec<CPolygon> }

#[derive(Clone, Default)]
pub struct RegenResult {
    pub polys: Vec<CPolygon>,
    pub topo: Arc<Topology>,
    pub planes: Vec<PlaneInfo>,
    pub axes: Vec<AxisInfo>,
    pub points: Vec<PointInfo>,
    pub csys: Vec<CsysInfo>,
    pub curves: Vec<CurveInfo>,
    pub cosmetics: Vec<Cosmetic>,
    pub errors: HashMap<u64, String>,
    pub tag_owner: HashMap<u32, u64>,
    pub param_errors: Vec<String>,
    pub vars: Vars,
    pub tools: HashMap<u64, ToolBody>,
    /// sketch / placement plane of each feature as it was resolved during regeneration
    pub frames: HashMap<u64, Frame>,
    pub rolled_back: HashSet<u64>,
    /// quilts still shown (consumed ones are hidden, as in Creo)
    pub quilts: Vec<Quilt>,
    /// bodies in order: (body id — 0 for Körper 1, else the creating feature — and polygon count)
    pub bodies: Vec<(u64, usize)>,
    pub time: f64,
    pub generation: u64,
}

impl RegenResult {
    pub fn plane(&self, id: u64) -> Option<&PlaneInfo> { self.planes.iter().find(|p| p.id == id) }
    pub fn axis(&self, id: u64) -> Option<&AxisInfo> { self.axes.iter().find(|a| a.id == id) }
    pub fn frame_for(&self, r: &PlaneRef) -> Option<Frame> {
        match r {
            PlaneRef::Datum(id) => self.plane(*id).map(|p| p.frame),
            PlaneRef::Face(f) => self.topo.match_face(f.point, f.normal).and_then(|i| self.topo.face_frame(i)),
        }
    }
}

static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A fresh display generation (invalidates GPU buffers).
pub fn next_generation() -> u64 { GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed) }

struct Regen<'a> {
    data: &'a ModelData,
    vars: Vars,
    polys: Vec<CPolygon>,
    topo: Option<Arc<Topology>>,
    tags: TagAllocator,
    res: RegenResult,
    planes: HashMap<u64, PlaneInfo>,
    axes: HashMap<u64, AxisInfo>,
    auto_axis: usize,
    quilts: Vec<(u64, Vec<CPolygon>)>,
    consumed: HashSet<u64>,
    /// all bodies; the current one's polygons live in `polys`
    bodies: Vec<(u64, Vec<CPolygon>)>,
    cur: usize,
}

/// Regenerates the model as shown (the active family table instance, or the generic).
pub fn run(data: &ModelData) -> RegenResult {
    let shown = data.shown();
    run_features(&shown, &shown.features, shown.insert_index)
}

pub fn run_features(data: &ModelData, features: &[Feature], stop: Option<usize>) -> RegenResult {
    let t0 = std::time::Instant::now();
    let (vars, perr) = data.evaluate_parameters();
    let mut r = Regen { data, vars: vars.clone(), polys: vec![], topo: None, tags: TagAllocator::default(), res: RegenResult::default(), planes: HashMap::new(), axes: HashMap::new(), auto_axis: 0, quilts: vec![], consumed: HashSet::new(), bodies: vec![(0, vec![])], cur: 0 };
    r.res.vars = vars;
    r.res.param_errors = perr;
    for (i, f) in features.iter().enumerate() {
        if stop.map(|s| i >= s).unwrap_or(false) { r.res.rolled_back.insert(f.id); continue; }
        if f.suppressed { continue; }
        // each solid feature works on its own body
        if !matches!(f.kind, FeatureKind::Body(_)) {
            // a feature being created (not yet in the model) goes into the default body
            let b = data.feature_body.get(&f.id).copied().unwrap_or(if data.feature(f.id).is_none() { data.default_body } else { 0 });
            let i = r.bodies.iter().position(|x| x.0 == b).unwrap_or(0);
            r.switch_body(i);
        }
        if let Err(e) = r.apply(f) { r.res.errors.insert(f.id, e); }
    }
    let _ = r.data;
    r.sync_out();
    r.res.bodies = r.bodies.iter().map(|b| (b.0, b.1.len())).collect();
    let all: Vec<CPolygon> = r.bodies.iter_mut().flat_map(|b| std::mem::take(&mut b.1)).collect();
    r.bodies = vec![(0, vec![])];
    r.set_polys(all);
    r.res.quilts = r.quilts.iter().filter(|q| !r.consumed.contains(&q.0)).map(|q| Quilt { feature: q.0, polys: q.1.clone() }).collect();
    let topo = r.topo();
    let mut res = r.res;
    res.polys = r.polys;
    res.topo = topo;
    res.planes = features.iter().filter_map(|f| r.planes.get(&f.id).cloned()).collect();
    let mut axes: Vec<AxisInfo> = r.axes.into_values().collect();
    axes.sort_by_key(|a| a.id);
    res.axes = axes;
    res.tag_owner = r.tags.owner;
    res.time = t0.elapsed().as_secs_f64();
    res.generation = next_generation();
    res
}

/// Frame of the Zug-Verbund's second section: the sweep's moving frame at the trajectory end.
pub fn sweep_end_frame(path: &[V3]) -> Frame {
    let start = sweep_section_frame(path);
    let rings = builders::sweep_rings(path, false, &start);
    let r = &rings[rings.len() - 1];
    let o = r(V2::ZERO);
    Frame { origin: o, x: safe_norm(r(V2::new(1.0, 0.0)) - o, V3::X), y: safe_norm(r(V2::new(0.0, 1.0)) - o, V3::Y) }
}

pub fn sweep_section_frame(path: &[V3]) -> Frame {
    let t = if path.len() >= 2 { safe_norm(path[1] - path[0], V3::Z) } else { V3::Z };
    Frame::from_normal(path.first().copied().unwrap_or(V3::ZERO), t)
}

/// Section planes of a rotational blend: the sketch plane turned about the centerline
/// `(ap, au)` of section 1 by the accumulated angles (degrees). Also returns the angles (radians).
pub fn rotational_blend_frames(base: &Frame, ap: V2, au: V2, angles: &[f64], flip: bool) -> (Vec<Frame>, Vec<f64>) {
    let (p, u) = (base.to_world(ap), base.dir_to_world(au));
    let mut frames = vec![*base];
    let mut rads = vec![0.0];
    let mut acc = 0.0;
    for a in angles {
        acc += a.to_radians() * if flip { -1.0 } else { 1.0 };
        let rot = |v: V3| rotate_vec(v, u, acc);
        frames.push(Frame { origin: p + rot(base.origin - p), x: rot(base.x), y: rot(base.y) });
        rads.push(acc);
    }
    (frames, rads)
}

/// Coordinate system frame (origin, X, Y) from offsets and X → Y → Z rotations (degrees).
pub fn csys_frame(o: V3, rx: f64, ry: f64, rz: f64) -> Frame {
    let r = |v: V3| rotate_vec(rotate_vec(rotate_vec(v, V3::X, rx.to_radians()), V3::Y, ry.to_radians()), V3::Z, rz.to_radians());
    Frame { origin: o, x: r(V3::X), y: r(V3::Y) }
}

/// Cylinder of a curved face for a cosmetic thread: (point on axis at the start, unit axis
/// direction pointing into the material, radius, length, internal).
pub fn thread_cylinder(t: &Topology, fi: usize, flip: bool) -> Option<(V3, V3, f64, f64, bool)> {
    let (p, d, r) = t.surface_axis(fi)?;
    let face = &t.faces[fi];
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut inward = 0.0;
    for &pi in &face.polys {
        for &v in &t.polys[pi] {
            let q = t.positions[v] - p;
            let a = q.dot(d);
            lo = lo.min(a);
            hi = hi.max(a);
        }
        let c = t.polys[pi].iter().map(|&v| t.positions[v]).sum::<V3>() / t.polys[pi].len() as f64 - p;
        inward += (c - d * c.dot(d)).dot(t.planes[pi].n);
    }
    if !(hi > lo) { return None; }
    let (start, dir) = if flip { (p + d * hi, -d) } else { (p + d * lo, d) };
    Some((start, dir, r, hi - lo, inward < 0.0))
}

/// Thread callout: "M10" for ISO coarse threads, otherwise "M10x1.25".
pub fn thread_label(major: f64, pitch: f64) -> String {
    match super::threads::ISO.iter().find(|t| (t.major - major).abs() < 1e-6) {
        Some(t) if (t.pitch - pitch).abs() < 1e-6 => t.name.to_string(),
        _ => format!("M{}x{}", fmt_num(major, 2), fmt_num(pitch, 2)),
    }
}

/// ISO coarse thread for a cylinder of diameter `dia`: the tap drill size for holes, the
/// major diameter for shafts.
pub fn thread_for_cylinder(dia: f64, internal: bool) -> (f64, f64) {
    let key = |t: &super::threads::ThreadSpec| if internal { t.tap } else { t.major };
    let best = super::threads::ISO.iter().min_by(|a, b| (key(a) - dia).abs().partial_cmp(&(key(b) - dia).abs()).unwrap());
    match best {
        Some(t) if (key(t) - dia).abs() <= dia * 0.08 => (t.major, t.pitch),
        _ => (if internal { dia / 0.85 } else { dia }, ((dia * 0.15).max(0.25) * 4.0).round() / 4.0),
    }
}

pub fn blend_frames(base: &Frame, offsets: &[f64], flip: bool) -> Vec<Frame> {
    let mut out = vec![*base];
    let mut acc = 0.0;
    for o in offsets { acc += o; out.push(base.offset(if flip { -acc } else { acc })); }
    out
}

/// Hole profile as (radius, depth) points.
/// Drill diameter of a hole (standard holes come from the thread tables).
pub fn hole_drill(d: &HoleDef, vars: &Vars) -> Result<f64, String> {
    if d.ty == HoleType::Standard {
        let spec = super::threads::find(d.standard, &d.size).ok_or(format!("Unbekannte Gewindegröße {}.", d.size))?;
        return Ok(super::threads::drill(spec, d.tapped, d.fit));
    }
    d.diameter.eval(vars)
}

pub fn hole_profile(d: &HoleDef, vars: &Vars, through: f64) -> Result<Vec<V2>, String> {
    let r = hole_drill(d, vars)? / 2.0;
    if r <= 0.0 { return Err("Der Bohrungsdurchmesser muss positiv sein.".into()); }
    let depth = if d.through_all { through } else { d.depth.eval(vars)? };
    if depth <= 0.0 { return Err("Die Bohrungstiefe muss positiv sein.".into()); }
    let eps = (r * 0.02).max(0.01);
    let tip = |depth: f64| if d.drill_tip && !d.through_all { V2::new(0.0, depth + r / 59f64.to_radians().tan()) } else { V2::new(0.0, depth) };
    Ok(match d.ty {
        HoleType::Simple | HoleType::Standard => vec![V2::new(0.0, -eps), V2::new(r, -eps), V2::new(r, depth), tip(depth)],
        HoleType::Counterbore => {
            let (cr, cd) = (d.cb_diameter.eval(vars)? / 2.0, d.cb_depth.eval(vars)?);
            if cr <= r { return Err("Der Senkungsdurchmesser muss größer als der Bohrungsdurchmesser sein.".into()); }
            vec![V2::new(0.0, -eps), V2::new(cr, -eps), V2::new(cr, cd), V2::new(r, cd), V2::new(r, depth), tip(depth)]
        }
        HoleType::Countersink => {
            let sr = d.cs_diameter.eval(vars)? / 2.0;
            let half = (d.cs_angle.eval(vars)? / 2.0).to_radians();
            if sr <= r { return Err("Der Kegelsenkungsdurchmesser muss größer als der Bohrungsdurchmesser sein.".into()); }
            let h = (sr - r) / half.tan();
            vec![V2::new(0.0, -eps), V2::new(sr + eps * half.tan(), -eps), V2::new(r, h), V2::new(r, depth), tip(depth)]
        }
    })
}

impl<'a> Regen<'a> {
    fn topo(&mut self) -> Arc<Topology> {
        if let Some(t) = &self.topo { return t.clone(); }
        // faces of every body can be referenced
        let t = if self.bodies.len() > 1 {
            let mut all: Vec<CPolygon> = self.bodies.iter().enumerate().filter(|(i, _)| *i != self.cur).flat_map(|(_, b)| b.1.iter().cloned()).collect();
            all.extend(self.polys.iter().cloned());
            Arc::new(Topology::new(&all))
        } else { Arc::new(Topology::new(&self.polys)) };
        self.topo = Some(t.clone());
        t
    }

    fn set_polys(&mut self, p: Vec<CPolygon>) { self.polys = p; self.topo = None; }

    fn sync_out(&mut self) { self.bodies[self.cur].1 = std::mem::take(&mut self.polys); self.topo = None; }
    fn sync_in(&mut self) { self.polys = std::mem::take(&mut self.bodies[self.cur].1); self.topo = None; }
    fn switch_body(&mut self, i: usize) { if i != self.cur && i < self.bodies.len() { self.sync_out(); self.cur = i; self.sync_in(); } }
    fn body_index(&self, id: u64) -> Result<usize, String> { self.bodies.iter().position(|b| b.0 == id).ok_or("Der Körper fehlt oder ist unterdrückt.".into()) }

    /// The polygons of the given quilt features, marking them consumed when `consume`.
    fn quilt_inputs(&mut self, ids: &[u64], consume: bool) -> Result<Vec<CPolygon>, String> {
        let mut out = vec![];
        for id in ids {
            let found: Vec<CPolygon> = self.quilts.iter().filter(|q| q.0 == *id).flat_map(|q| q.1.iter().cloned()).collect();
            if found.is_empty() { return Err("Eine gewählte Sammelfläche fehlt oder ist verbraucht.".into()); }
            out.extend(found);
            if consume { self.consumed.insert(*id); }
        }
        if out.is_empty() { return Err("Eine Sammelfläche wählen.".into()); }
        Ok(out)
    }

    /// The solid's polygons on the given faces.
    fn face_polys(&mut self, faces: &[FaceRef]) -> Result<Vec<CPolygon>, String> {
        let t = self.topo();
        let mut out = vec![];
        for f in faces {
            let i = t.match_face(f.point, f.normal).ok_or("Eine gewählte Fläche wurde nicht gefunden.")?;
            for &pi in &t.faces[i].polys {
                let pts: Vec<V3> = t.polys[pi].iter().map(|&k| t.positions[k]).collect();
                if let Some(mut p) = crate::model::surfaces::poly(&pts, t.tags[pi]) { if p.plane.n.dot(t.faces[i].normal) < 0.0 { p.flip(); } out.push(p); }
            }
        }
        if out.is_empty() { return Err("Flächen wählen.".into()); }
        Ok(out)
    }

    fn apply_surface(&mut self, d: &SurfaceDef, fid: u64) -> Result<(), String> {
        use crate::model::surfaces as sf;
        let tag = self.tags.next(fid);
        let sg = if d.flip { -1.0 } else { 1.0 };
        match d.op {
            SurfOp::Fill => {
                let frame = self.plane(&d.plane)?;
                let s = self.prepare(&d.sketch)?;
                let regions: Vec<(Vec<V2>, Vec<Vec<V2>>)> = s.profile_regions()?.into_iter().map(|r| (r.outer.pts, r.holes.into_iter().map(|h| h.pts).collect())).collect();
                let q = sf::fill(&regions, &frame, tag);
                if q.is_empty() { return Err("Eine geschlossene Skizze zeichnen.".into()); }
                self.quilts.push((fid, q));
            }
            SurfOp::Offset => {
                let src = if d.inputs.is_empty() { self.face_polys(&d.faces)? } else { self.quilt_inputs(&d.inputs, false)? };
                let q = sf::offset(&src, self.e(&d.distance)? * sg).into_iter().map(|mut p| { p.tag = tag; p }).collect();
                self.quilts.push((fid, q));
            }
            SurfOp::Thicken => {
                let src = self.quilt_inputs(&d.inputs, true)?;
                let t = self.e(&d.distance)?;
                if t <= 0.0 { return Err("Die Dicke muss positiv sein.".into()); }
                let solid = csg::orient_outward(sf::thicken(&src, t * sg, None, tag).into_iter().map(|mut p| { p.tag = tag; p }).collect());
                self.apply_tool(solid, d.remove, fid)?;
            }
            SurfOp::Solidify => {
                let src = self.quilt_inputs(&d.inputs, true)?;
                if !sf::is_closed(&src) { return Err("Die Sammelfläche ist nicht geschlossen — zuerst zusammenführen oder Aufdicken verwenden.".into()); }
                let solid = csg::orient_outward(src.into_iter().map(|mut p| { p.tag = tag; p }).collect());
                self.apply_tool(solid, d.remove, fid)?;
            }
            SurfOp::Merge => {
                if d.inputs.len() < 2 { return Err("Zwei Sammelflächen wählen.".into()); }
                let q = self.quilt_inputs(&d.inputs, true)?;
                self.quilts.push((fid, q));
            }
            SurfOp::Trim | SurfOp::Split => {
                let src = self.quilt_inputs(&d.inputs, true)?;
                let frame = self.plane(&d.plane)?;
                let (n, w) = (frame.z(), frame.z().dot(frame.origin));
                let keep = sf::clip(&src, n, w, !d.flip);
                if keep.is_empty() { return Err("Die Ebene schneidet die Sammelfläche nicht.".into()); }
                self.quilts.push((fid, keep));
                if d.op == SurfOp::Split { self.quilts.push((fid, sf::clip(&src, n, w, d.flip))); }
            }
            SurfOp::Intersect => {
                let frame = self.plane(&d.plane)?;
                let src = if d.inputs.is_empty() { self.polys.clone() } else { self.quilt_inputs(&d.inputs, false)? };
                let segs = sf::section(&src, frame.z(), frame.z().dot(frame.origin));
                if segs.is_empty() { return Err("Die Ebene schneidet die Geometrie nicht.".into()); }
                self.res.curves.push(CurveInfo { feature: fid, polylines: segs });
            }
            SurfOp::Project => {
                use crate::sketch::sketch::SkKind;
                let frame = self.plane(&d.plane)?;
                let s = self.prepare(&d.sketch)?;
                let t = self.topo();
                let mut lines = vec![];
                for e in s.entities.iter().filter(|e| !matches!(e.kind, SkKind::Point | SkKind::Csys | SkKind::Text) && !e.construction) {
                    let pts: Vec<V3> = s.sample(e, 64).iter().filter_map(|p| {
                        let w = frame.to_world(*p);
                        t.ray_first_hit(w, -frame.z(), 1e-6).map(|h| w - frame.z() * h).or_else(|| t.ray_first_hit(w, frame.z(), 1e-6).map(|h| w + frame.z() * h))
                    }).collect();
                    if pts.len() >= 2 { lines.push(pts); }
                }
                if lines.is_empty() { return Err("Die Skizze trifft beim Projizieren keine Fläche.".into()); }
                self.res.curves.push(CurveInfo { feature: fid, polylines: lines });
            }
            SurfOp::Extend => {
                let src = self.quilt_inputs(&d.inputs, true)?;
                self.quilts.push((fid, sf::extend(&src, self.e(&d.distance)?, tag)));
            }
            SurfOp::Copy => {
                let q = self.face_polys(&d.faces)?.into_iter().map(|mut p| { p.tag = tag; p }).collect();
                self.quilts.push((fid, q));
            }
            SurfOp::Replace => {
                let faces = self.face_polys(&d.faces)?;
                let target = self.quilt_inputs(&d.inputs, true)?;
                let (n, w) = (target[0].plane.n, target[0].plane.w);
                let nf = faces[0].plane.n;
                let c = faces.iter().map(|p| p.centroid()).fold(V3::ZERO, |a, b| a + b) / faces.len() as f64;
                let den = n.dot(nf);
                if den.abs() < 1e-6 { return Err("Die Ersatzfläche ist zur Fläche parallel ausgerichtet.".into()); }
                let dist = (w - n.dot(c)) / den;
                let prism = csg::orient_outward(sf::thicken(&faces, 0.0, Some(nf * dist), tag).into_iter().map(|mut p| { p.tag = tag; p }).collect());
                self.apply_tool(prism, dist < 0.0, fid)?;
            }
            SurfOp::Boundary => {
                let mut curves = vec![];
                for id in &d.inputs {
                    for c in self.res.curves.iter().filter(|c| c.feature == *id) { curves.extend(c.polylines.iter().filter(|l| l.len() >= 2).cloned()); }
                }
                if curves.len() < 2 { return Err("Zwei oder vier Randkurven (Skizzen oder Kurven) wählen.".into()); }
                curves.truncate(if curves.len() >= 4 { 4 } else { 2 });
                self.quilts.push((fid, sf::boundary_blend(&curves, tag)));
            }
            SurfOp::Style => {
                let frame = self.plane(&d.plane)?;
                self.quilts.push((fid, sf::style_patch(&frame, d.size[0], d.size[1], &d.heights, tag)));
            }
            SurfOp::Freestyle => {
                let frame = self.plane(&d.plane)?;
                self.quilts.push((fid, sf::freestyle(&frame, d.size, d.level, tag)));
            }
        }
        Ok(())
    }

    fn apply_body(&mut self, d: &BodyDef, fid: u64) -> Result<(), String> {
        self.sync_out();
        let r = (|| -> Result<(), String> {
            match d.op {
                BodyOp::New => self.bodies.push((fid, vec![])),
                BodyOp::Union | BodyOp::Subtract | BodyOp::Intersect => {
                    let (ti, oi) = (self.body_index(d.target)?, self.body_index(d.tool)?);
                    if ti == oi { return Err("Zwei verschiedene Körper wählen.".into()); }
                    let (a, b) = (self.bodies[ti].1.clone(), std::mem::take(&mut self.bodies[oi].1));
                    if b.is_empty() { return Err("Der zweite Körper ist leer.".into()); }
                    self.bodies[ti].1 = match d.op { BodyOp::Union => csg::union(&a, &b), BodyOp::Subtract => csg::subtract(&a, &b), _ => csg::intersect(&a, &b) };
                }
                BodyOp::Split => {
                    let ti = self.body_index(d.target)?;
                    let mut frame = self.plane(&d.plane)?;
                    if d.flip { frame = Frame { origin: frame.origin, x: frame.x, y: -frame.y }; }
                    let body = self.bodies[ti].1.clone();
                    if body.is_empty() { return Err("Der Körper ist leer.".into()); }
                    let big = csg::bbox(&body).diagonal() * 2.0 + 10.0;
                    let tag = self.tags.next(fid);
                    let hs = crate::model::surfaces::half_space(&frame, big, tag);
                    let (keep, cut) = (csg::subtract(&body, &hs), csg::intersect(&body, &hs));
                    if keep.is_empty() || cut.is_empty() { return Err("Die Ebene teilt den Körper nicht.".into()); }
                    self.bodies[ti].1 = keep;
                    self.bodies.push((fid, cut));
                }
            }
            Ok(())
        })();
        self.sync_in();
        r
    }

    fn plane(&mut self, r: &Option<PlaneRef>) -> Result<Frame, String> {
        match r {
            None => Err("Die Platzierungsebene ist nicht definiert.".into()),
            Some(PlaneRef::Datum(id)) => self.planes.get(id).map(|p| p.frame).ok_or("Die Referenzbezugsebene fehlt oder ist fehlgeschlagen.".into()),
            Some(PlaneRef::Face(f)) => {
                let t = self.topo();
                t.match_face(f.point, f.normal).and_then(|i| t.face_frame(i)).ok_or("Die Referenzfläche wurde nicht gefunden.".into())
            }
        }
    }

    fn axis(&self, r: &AxisRef) -> Result<(V3, V3), String> {
        match r {
            AxisRef::WorldX => Ok((V3::ZERO, V3::X)),
            AxisRef::WorldY => Ok((V3::ZERO, V3::Y)),
            AxisRef::WorldZ => Ok((V3::ZERO, V3::Z)),
            AxisRef::Axis(id) => self.axes.get(id).map(|a| (a.point, a.dir)).ok_or("Die Referenzachse fehlt.".into()),
        }
    }

    fn e(&self, d: &Dim) -> Result<f64, String> { d.eval(&self.vars) }

    fn through(&self, f: &Frame) -> f64 {
        let b = csg::bbox(&self.polys);
        if b.is_empty() { 1000.0 } else { 2.0 * (b.diagonal() + (f.origin - b.center()).length()) + 10.0 }
    }

    fn prepare(&self, s: &Sketch) -> Result<Sketch, String> {
        let mut s = s.clone();
        if s.constraints.iter().any(|c| c.expr.as_ref().map(|e| !e.trim().is_empty()).unwrap_or(false))
            && s.apply_expressions(&self.vars)?
            && !solver::solve(&mut s, None)
        {
            return Err("Die Skizze konnte mit den aktuellen Parameterwerten nicht gelöst werden.".into());
        }
        Ok(s)
    }

    /// Give copied geometry fresh surface tags owned by `fid` (one new tag per source tag).
    fn retag(&mut self, mut polys: Vec<CPolygon>, fid: u64) -> Vec<CPolygon> {
        let mut map: HashMap<u32, u32> = HashMap::new();
        for p in &mut polys { p.tag = *map.entry(p.tag).or_insert_with(|| self.tags.next(fid)); }
        polys
    }

    fn add_axis(&mut self, feature: u64, k: u64, point: V3, dir: V3) {
        self.auto_axis += 1;
        let id = feature * 1000 + k;
        self.axes.insert(id, AxisInfo { id, name: format!("A_{}", self.auto_axis), point, dir: safe_norm(dir, V3::Z), feature });
    }

    fn apply_tool(&mut self, tool: Vec<CPolygon>, remove: bool, fid: u64) -> Result<(), String> {
        if tool.is_empty() { return Err("Das KE hat keine Geometrie erzeugt.".into()); }
        let new = if remove { csg::subtract(&self.polys, &tool) } else { csg::union(&self.polys, &tool) };
        self.res.tools.insert(fid, ToolBody { polys: tool, op: if remove { Op::Cut } else { Op::Add } });
        self.set_polys(new);
        Ok(())
    }

    fn add_curves(&mut self, s: &Sketch, f: &Frame, fid: u64) {
        use crate::sketch::sketch::SkKind;
        let mut lines: Vec<Vec<V3>> = vec![];
        for e in s.entities.iter().filter(|e| !matches!(e.kind, SkKind::Point | SkKind::Csys)) {
            if e.kind == SkKind::Text {
                for mut l in s.text_loops(e) { if let Some(p) = l.first().copied() { l.push(p); } lines.push(l.iter().map(|p| f.to_world(*p)).collect()); }
            } else { lines.push(s.sample(e, 64).iter().map(|p| f.to_world(*p)).collect()); }
        }
        self.res.curves.push(CurveInfo { feature: fid, polylines: lines });
    }

    fn apply(&mut self, f: &Feature) -> Result<(), String> {
        let fid = f.id;
        let placement = match &f.kind { FeatureKind::Hole(d) => d.plane, k => k.sketch_plane() };
        if placement.is_some() { if let Ok(fr) = self.plane(&placement) { self.res.frames.insert(fid, fr); } }
        match &f.kind {
            FeatureKind::DatumPlane(d) => {
                let frame = match d.kind {
                    PlaneKind::Front => Frame { origin: V3::ZERO, x: V3::X, y: V3::Y },
                    PlaneKind::Top => Frame { origin: V3::ZERO, x: V3::X, y: -V3::Z },
                    PlaneKind::Right => Frame { origin: V3::ZERO, x: -V3::Z, y: V3::Y },
                    PlaneKind::Offset => {
                        let base = self.plane(&d.reference)?;
                        let o = self.e(&d.offset)?;
                        base.offset(if d.flip { -o } else { o })
                    }
                    PlaneKind::Angle => {
                        let base = self.plane(&d.reference)?;
                        let ax = d.axis.ok_or("Eine Achse für die geneigte Ebene wählen.")?;
                        let (a, u) = self.axis(&ax)?;
                        let n0 = safe_norm(base.z() - u * base.z().dot(u), base.z());
                        let ang = self.e(&d.angle)? * if d.flip { -1.0 } else { 1.0 };
                        Frame::plane_through(a, rotate_vec(n0, u, ang.to_radians()))
                    }
                };
                self.planes.insert(fid, PlaneInfo { id: fid, name: f.name.clone(), frame, size: 0.0 });
            }
            FeatureKind::DatumAxis(d) => {
                let (p, dir) = match d.kind {
                    AxisKind::TwoPlanes => {
                        let (f1, f2) = (self.plane(&d.p1)?, self.plane(&d.p2)?);
                        let (n1, n2) = (f1.z(), f2.z());
                        let dd = n1.cross(n2);
                        if dd.length() < 1e-6 { return Err("Die gewählten Ebenen sind parallel.".into()); }
                        let (w1, w2) = (n1.dot(f1.origin), n2.dot(f2.origin));
                        ((w1 * n2.cross(dd) + w2 * dd.cross(n1)) / dd.dot(dd), dd.normalize())
                    }
                    AxisKind::World => (V3::ZERO, [V3::X, V3::Y, V3::Z][d.world.min(2) as usize]),
                    AxisKind::Normal => {
                        let fr = self.plane(&d.p1)?;
                        (fr.to_world(V2::new(self.e(&d.x)?, self.e(&d.y)?)), fr.z())
                    }
                };
                self.axes.insert(fid, AxisInfo { id: fid, name: f.name.clone(), point: p, dir, feature: fid });
            }
            FeatureKind::DatumPoint(d) => {
                let p = V3::new(self.e(&d.x)?, self.e(&d.y)?, self.e(&d.z)?);
                self.res.points.push(PointInfo { id: fid, name: f.name.clone(), p });
            }
            FeatureKind::Csys => {}
            FeatureKind::Sketch(d) => {
                let frame = self.plane(&d.plane)?;
                let s = self.prepare(&d.sketch)?;
                self.add_curves(&s, &frame, fid);
            }
            FeatureKind::Extrude(d) => {
                let frame = self.plane(&d.plane)?;
                let s = self.prepare(&d.sketch)?;
                let regions = if d.thin { thin_regions(&s, self.e(&d.thickness)?)? } else { s.profile_regions()? };
                let sg = if d.flip { -1.0 } else { 1.0 };
                let big = self.through(&frame);
                let mut z0 = 0.0;
                let z1;
                match d.depth_type {
                    DepthType::Blind => { let dep = self.e(&d.depth)?; if dep <= 0.0 { return Err("Die Tiefe muss positiv sein.".into()); } z1 = dep * sg; }
                    DepthType::Symmetric => { let dep = self.e(&d.depth)?; if dep <= 0.0 { return Err("Die Tiefe muss positiv sein.".into()); } z0 = -dep / 2.0 * sg; z1 = dep / 2.0 * sg; }
                    DepthType::ThroughAll | DepthType::ToSelected => z1 = big * sg,
                    DepthType::ToNext => {
                        // farthest first hit of the solid from the section along the extrusion direction
                        let t = self.topo();
                        let dir = frame.z() * sg;
                        let mut best: Option<f64> = None;
                        for r in &regions {
                            for l in std::iter::once(&r.outer).chain(r.holes.iter()) {
                                let step = (l.pts.len() / 64).max(1);
                                for p in l.pts.iter().step_by(step) {
                                    if let Some(h) = t.ray_first_hit(frame.to_world(*p), dir, 1e-4) { best = Some(best.map_or(h, |b: f64| b.max(h))); }
                                }
                            }
                        }
                        z1 = best.ok_or("Bis zur nächsten: keine Fläche in Profilrichtung gefunden.")? * sg;
                    }
                }
                if d.depth_type != DepthType::Symmetric {
                    match d.side2 {
                        Side2::None => if d.remove { z0 = -0.001 * sg },
                        Side2::Blind => z0 = -self.e(&d.depth2)? * sg,
                        Side2::ThroughAll => z0 = -big * sg,
                    }
                }
                let taper = if d.depth_type == DepthType::Symmetric || d.side2 != Side2::None { 0.0 } else { self.e(&d.taper)? };
                let mut tool = builders::extrude(&regions, &frame, z0, z1, taper, &mut self.tags, fid);
                if d.depth_type == DepthType::ToSelected {
                    let target = self.plane(&d.to_ref).map_err(|_| "Bis zur ausgewählten: eine Ebene oder ebene Fläche als Endreferenz wählen.".to_string())?;
                    let h = target.height(frame.origin);
                    let keep_neg = if h.abs() > 1e-9 { h < 0.0 } else { target.z().dot(frame.z() * sg) > 0.0 };
                    let l = big * 2.0;
                    let sq = builders::simple_loop(vec![V2::new(-l, -l), V2::new(l, -l), V2::new(l, l), V2::new(-l, l)]);
                    let region = crate::sketch::sketch::ProfileRegion { outer: sq, holes: vec![] };
                    let half = if keep_neg { builders::extrude(&[region], &target, -l, 0.0, 0.0, &mut self.tags, fid) } else { builders::extrude(&[region], &target, 0.0, l, 0.0, &mut self.tags, fid) };
                    tool = csg::intersect(&tool, &half);
                    if tool.is_empty() { return Err("Bis zur ausgewählten: die Endreferenz wird in Profilrichtung nicht erreicht.".into()); }
                }
                self.apply_tool(tool, d.remove, fid)?;
                let mut k = 0;
                for e in &s.entities {
                    if matches!(e.kind, crate::sketch::sketch::SkKind::Circle | crate::sketch::sketch::SkKind::Arc) && !e.construction {
                        k += 1;
                        self.add_axis(fid, k, frame.to_world(s.point(e.pts[0])), frame.z());
                    }
                }
            }
            FeatureKind::Revolve(d) => {
                let frame = self.plane(&d.plane)?;
                let s = self.prepare(&d.sketch)?;
                let (ap, au) = s.centerline_axis().ok_or("Eine Mittellinie skizzieren, um die Drehachse festzulegen.")?;
                let regions = s.profile_regions()?;
                let mut side = 0.0;
                for r in &regions {
                    for p in &r.outer.pts {
                        let c = cross2(au, *p - ap);
                        if c.abs() < 1e-7 { continue; }
                        if side == 0.0 { side = c } else if side * c < 0.0 { return Err("Der Querschnitt muss auf einer Seite der Drehachse liegen.".into()); }
                    }
                }
                let ang = self.e(&d.angle)?.to_radians();
                if ang <= 0.0 { return Err("Der Winkel muss positiv sein.".into()); }
                let sg = if d.flip { -1.0 } else { 1.0 };
                let (a0, a1) = if d.symmetric { (-ang / 2.0, ang / 2.0) } else { (0.0, ang) };
                let tool = builders::revolve(&regions, &frame, ap, au, a0 * sg, a1 * sg, &mut self.tags, fid);
                self.apply_tool(tool, d.remove, fid)?;
                self.add_axis(fid, 1, frame.to_world(ap), frame.dir_to_world(au));
            }
            FeatureKind::Sweep(d) => {
                let frame = self.plane(&d.plane)?;
                let traj = self.prepare(&d.trajectory)?;
                let (pts, closed) = traj.trajectory()?;
                let path: Vec<V3> = pts.iter().map(|p| frame.to_world(*p)).collect();
                if path.len() < 2 { return Err("Die Leitkurve ist zu kurz.".into()); }
                let sf = sweep_section_frame(&path);
                let sec = self.prepare(&d.section)?;
                let regions = sec.profile_regions()?;
                let tool = match d.mode {
                    SweepMode::Normal => builders::sweep(&regions, &path, closed, &sf, &mut self.tags, fid),
                    SweepMode::Volume => {
                        // the tool keeps the start orientation and only moves along the trajectory
                        let rings: Vec<Box<dyn Fn(V2) -> V3>> = path.iter().map(|p| { let o = *p; let (x, y) = (sf.x, sf.y); Box::new(move |q: V2| o + x * q.x + y * q.y) as Box<dyn Fn(V2) -> V3> }).collect();
                        builders::loft(&regions, &rings, closed, &mut self.tags, fid)
                    }
                    SweepMode::Blend => {
                        if closed { return Err("Ein Zug-Verbund braucht eine offene Leitkurve.".into()); }
                        let sec2 = self.prepare(&d.section2)?;
                        let r2 = sec2.profile_regions().map_err(|_| "Den zweiten Querschnitt am Ende der Leitkurve skizzieren.".to_string())?;
                        let tag = self.tags.next(fid);
                        swept_blend(&regions[0].outer.pts, &r2[0].outer.pts, &path, &sf, tag)?
                    }
                };
                self.apply_tool(tool, d.remove, fid)?;
                self.add_curves(&traj, &frame, fid);
            }
            FeatureKind::Blend(d) => {
                let base = self.plane(&d.plane)?;
                if d.sections.len() < 2 { return Err("Ein Verbund benötigt mindestens zwei Querschnitte.".into()); }
                let offs: Vec<f64> = d.offsets.iter().take(d.sections.len() - 1).map(|o| self.e(o)).collect::<Result<_, _>>()?;
                if offs.len() < d.sections.len() - 1 || offs.iter().any(|o| *o <= 0.0) {
                    return Err(if d.rotational { "Querschnittswinkel müssen positiv sein." } else { "Querschnittsabstände müssen positiv sein." }.into());
                }
                let mut secs = vec![];
                let mut axis = None;
                for (i, s) in d.sections.iter().enumerate() {
                    let s = self.prepare(s)?;
                    if i == 0 { axis = s.centerline_axis(); }
                    let r = s.profile_regions().map_err(|_| format!("Querschnitt {} muss eine einzelne geschlossene Schleife sein.", i + 1))?;
                    secs.push(r[0].outer.pts.clone());
                }
                let tool = if d.rotational {
                    let (ap, au) = axis.ok_or("Im ersten Querschnitt eine Mittellinie als Drehachse skizzieren.")?;
                    if offs.iter().sum::<f64>() >= 360.0 { return Err("Der Gesamtwinkel eines Rotationsverbunds muss kleiner als 360° sein.".into()); }
                    let (_, rads) = rotational_blend_frames(&base, ap, au, &offs, d.flip);
                    let (p, u) = (base.to_world(ap), base.dir_to_world(au));
                    let tool = builders::rotational_blend(&secs, &base, p, u, &rads, &mut self.tags, fid);
                    self.add_axis(fid, 1, p, u);
                    tool
                } else {
                    builders::blend(&secs, &blend_frames(&base, &offs, d.flip), &mut self.tags, fid)
                };
                self.apply_tool(tool, d.remove, fid)?;
            }
            FeatureKind::Hole(d) => {
                let frame = self.plane(&d.plane)?;
                let p = V2::new(self.e(&d.x)?, self.e(&d.y)?);
                let dir = if d.flip { frame.z() } else { -frame.z() };
                let prof = hole_profile(d, &self.vars, self.through(&frame))?;
                let base = frame.to_world(p);
                let tool = builders::hole_tool(&prof, base, dir, frame.x, &mut self.tags, fid);
                self.apply_tool(tool, true, fid)?;
                self.add_axis(fid, 1, base, dir);
                if d.ty == HoleType::Standard && d.tapped {
                    let spec = super::threads::find(d.standard, &d.size).ok_or("Unbekannte Gewindegröße.")?;
                    let extent = self.polys.iter().flat_map(|q| q.v.iter()).map(|v| (v.p - base).dot(dir)).fold(0.0f64, f64::max);
                    let depth = if d.through_all { extent } else { self.e(&d.depth)? };
                    let len = if d.through_all { extent } else { self.e(&d.thread_depth)?.min(depth) };
                    if len > 0.0 {
                        self.res.cosmetics.push(Cosmetic { feature: fid, base, dir, radius: spec.major / 2.0, length: len, label: super::threads::callout(d.standard, spec) });
                    }
                }
            }
            FeatureKind::Round(d) => {
                if d.edge_count() == 0 { return Err("Kanten für die Rundung wählen.".into()); }
                let t = self.topo();
                let mut cur = self.polys.clone();
                for (si, set) in d.sets.iter().enumerate() {
                    if set.edges.is_empty() { continue; }
                    let r0 = self.e(&set.radius)?;
                    let r1 = if set.variable { self.e(&set.radius_end)? } else { r0 };
                    if r0 <= 0.0 || r1 <= 0.0 { return Err(format!("Satz {}: der Radius muss positiv sein.", si + 1)); }
                    for e in &set.edges {
                        let ci = t.match_chain(e).ok_or(format!("Satz {}: eine Kantenreferenz wurde nicht gefunden.", si + 1))?;
                        let c = &t.chains[ci];
                        // keep the variable radius anchored to the end that was picked first
                        let rev = (c.points[0] - e.a).length() > (c.points[c.points.len() - 1] - e.a).length();
                        let (a, b) = if rev { (r1, r0) } else { (r0, r1) };
                        let tool = builders::edge_tool(c, &builders::round_section(a, b, c.convex), a.max(b) * 0.02, &mut self.tags, fid);
                        cur = if c.convex { csg::subtract(&cur, &tool) } else { csg::union(&cur, &tool) };
                    }
                }
                self.set_polys(cur);
            }
            FeatureKind::Chamfer(d) => {
                if d.edges.is_empty() { return Err("Kanten für die Fase wählen.".into()); }
                let d1 = self.e(&d.d1)?;
                let d2 = match d.kind { ChamferKind::DxD => d1, ChamferKind::D1xD2 => self.e(&d.d2)?, ChamferKind::AnglexD => d1 * self.e(&d.angle)?.to_radians().tan() };
                if d1 <= 0.0 || d2 <= 0.0 { return Err("Fasenabstände müssen positiv sein.".into()); }
                let t = self.topo();
                if d.corner {
                    // Eckfase: cut off the corner where the chosen edges meet
                    let tag = self.tags.next(fid);
                    let tool = corner_tool(&t, &d.edges, d1, tag)?;
                    return self.apply_tool(tool, true, fid);
                }
                let mut cur = self.polys.clone();
                for e in &d.edges {
                    let ci = t.match_chain(e).ok_or("Eine Kantenreferenz wurde nicht gefunden.")?;
                    let c = &t.chains[ci];
                    let tool = builders::edge_tool(c, &builders::chamfer_section(d1, d2, c.convex), d1.max(d2) * 0.02, &mut self.tags, fid);
                    cur = if c.convex { csg::subtract(&cur, &tool) } else { csg::union(&cur, &tool) };
                }
                self.set_polys(cur);
            }
            FeatureKind::Shell(d) => {
                if self.polys.is_empty() { return Err("Nichts zu schalen.".into()); }
                let th = self.e(&d.thickness)?;
                if th <= 0.0 { return Err("Die Dicke muss positiv sein.".into()); }
                let t = self.topo();
                let mut removed = HashSet::new();
                for fr in &d.faces { removed.insert(t.match_face(fr.point, fr.normal).ok_or("Eine entfernte Fläche wurde nicht gefunden.")?); }
                let inner = builders::move_planes(&t, &|f, n, w| if removed.contains(&f) { (n, w + th * 1.5) } else { (n, w - th) });
                let new = csg::subtract(&self.polys, &inner);
                self.res.tools.insert(fid, ToolBody { polys: inner, op: Op::Cut });
                self.set_polys(new);
            }
            FeatureKind::Draft(d) => {
                if d.faces.is_empty() { return Err("Flächen für die Formschräge wählen.".into()); }
                let hinge = self.plane(&d.hinge)?;
                let ang = self.e(&d.angle)?.to_radians();
                let t = self.topo();
                let pull = if d.flip { -hinge.z() } else { hinge.z() };
                let mut drafted: HashMap<usize, (V3, f64)> = HashMap::new();
                // Variable Zugschräge: the angle runs from `angle` to `angle2` across the faces along the hinge x direction
                let ang2 = match &d.angle2 { Some(a) => Some(self.e(a)?.to_radians()), None => None };
                let xs: Vec<f64> = d.faces.iter().map(|f| f.point.dot(hinge.x)).collect();
                let (x0, x1) = (xs.iter().cloned().fold(f64::INFINITY, f64::min), xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                for (k, fr) in d.faces.iter().enumerate() {
                    let ang = match ang2 { Some(a2) if x1 > x0 + 1e-9 => ang + (a2 - ang) * (xs[k] - x0) / (x1 - x0), Some(a2) => (ang + a2) / 2.0, None => ang };
                    let fi = t.match_face(fr.point, fr.normal).ok_or("Eine Formschrägenfläche wurde nicht gefunden.")?;
                    let (n, w) = (t.faces[fi].normal, t.faces[fi].offset);
                    let dd = n.cross(hinge.z());
                    if dd.length() < 1e-6 { return Err("Formschrägenflächen dürfen nicht parallel zur Scharnierebene sein.".into()); }
                    let hw = hinge.z().dot(hinge.origin);
                    let q = (w * hinge.z().cross(dd) + hw * dd.cross(n)) / dd.dot(dd);
                    let np = safe_norm(n * ang.cos() + pull * ang.sin(), n);
                    drafted.insert(fi, (np, np.dot(q)));
                }
                let new = builders::move_planes(&t, &|f, n, w| *drafted.get(&f).unwrap_or(&(n, w)));
                self.set_polys(new);
            }
            FeatureKind::Pattern(d) => {
                if d.features.is_empty() { return Err("Ein zu musterndes KE wählen.".into()); }
                let mut xf = vec![];
                match d.kind {
                    PatternKind::Direction => {
                        let (_, u1) = self.axis(&d.dir1)?;
                        let n1 = self.e(&d.count1)?.round().max(1.0) as usize;
                        let s1 = self.e(&d.spacing1)?;
                        let (mut n2, mut s2, mut u2) = (1usize, 0.0, V3::ZERO);
                        if d.use_dir2 {
                            u2 = self.axis(&d.dir2)?.1;
                            n2 = self.e(&d.count2)?.round().max(1.0) as usize;
                            s2 = self.e(&d.spacing2)?;
                        }
                        for i in 0..n1 { for j in 0..n2 { if i != 0 || j != 0 { xf.push(Xform::translation(u1 * (s1 * i as f64) + u2 * (s2 * j as f64))); } } }
                    }
                    PatternKind::Axis => {
                        let (a, u) = self.axis(&d.axis)?;
                        let n = self.e(&d.axis_count)?.round().max(1.0) as usize;
                        let step = self.e(&d.axis_angle)?.to_radians();
                        for k in 1..n { xf.push(Xform::rotation(a, safe_norm(u, V3::Y), step * k as f64)); }
                    }
                    PatternKind::Table => {
                        if d.table.is_empty() { return Err("Die Mustertabelle ist leer.".into()); }
                        for r in &d.table { xf.push(Xform::translation(V3::new(r[0], r[1], r[2]))); }
                    }
                    PatternKind::Fill | PatternKind::Curve => {
                        let fr = self.plane(&d.sketch_plane).map_err(|_| "Zuerst den Füllbereich / die Kurve auf einer Ebene skizzieren.".to_string())?;
                        let sk = self.prepare(&d.sketch)?;
                        let leader = d.features.first().and_then(|s| self.res.tools.get(s)).map(|t| csg::bbox(&t.polys).center()).ok_or("Das gemusterte KE hat keine Geometrie.")?;
                        let l = fr.to_local(leader);
                        if d.kind == PatternKind::Fill {
                            let regions = sk.profile_regions()?;
                            let s = self.e(&d.fill_spacing)?;
                            if s <= 0.0 { return Err("Der Füllabstand muss positiv sein.".into()); }
                            let margin = self.e(&d.fill_margin)?;
                            let rot = self.e(&d.fill_rotation)?.to_radians();
                            let (u, v) = (V2::new(rot.cos(), rot.sin()), V2::new(-rot.sin(), rot.cos()));
                            let (bu, bv) = match d.fill_grid {
                                FillGrid::Square => (u * s, v * s),
                                FillGrid::Diamond => ((u + v) * (s / 2f64.sqrt()), (v - u) * (s / 2f64.sqrt())),
                                FillGrid::Triangle => (u * s, u * (s * 0.5) + v * (s * 3f64.sqrt() * 0.5)),
                            };
                            let mut lo = V2::splat(f64::INFINITY);
                            let mut hi = V2::splat(f64::NEG_INFINITY);
                            for r in &regions { for p in &r.outer.pts { lo = lo.min(*p); hi = hi.max(*p); } }
                            let reach = ((hi - l).length().max((lo - l).length()).max((V2::new(lo.x, hi.y) - l).length()).max((V2::new(hi.x, lo.y) - l).length()) / (s * 0.5)).ceil() as i64 + 1;
                            let reach = reach.min(80);
                            let edge_dist = |g: V2| -> f64 {
                                let mut best = f64::INFINITY;
                                for r in &regions { for lp in std::iter::once(&r.outer).chain(r.holes.iter()) {
                                    let n = lp.pts.len();
                                    for k in 0..n { best = best.min(dist_point_segment2(g, lp.pts[k], lp.pts[(k + 1) % n])); }
                                } }
                                best
                            };
                            'grid: for i in -reach..=reach {
                                for j in -reach..=reach {
                                    if i == 0 && j == 0 { continue; }
                                    let g = l + bu * i as f64 + bv * j as f64;
                                    let inside = regions.iter().any(|r| point_in_polygon2(g, &r.outer.pts) && !r.holes.iter().any(|h| point_in_polygon2(g, &h.pts)));
                                    if inside && (margin <= 0.0 || edge_dist(g) >= margin) {
                                        xf.push(Xform::translation(fr.dir_to_world(g - l)));
                                        if xf.len() > 400 { break 'grid; }
                                    }
                                }
                            }
                        } else {
                            let (mut pts, closed) = sk.trajectory()?;
                            if closed { pts.push(pts[0]); }
                            let mut acc = vec![0.0];
                            for k in 1..pts.len() { let x = acc[k - 1] + (pts[k] - pts[k - 1]).length(); acc.push(x); }
                            let total = *acc.last().unwrap();
                            if total <= 1e-9 { return Err("Die Musterkurve ist zu kurz.".into()); }
                            let eval = |s: f64| -> (V2, V2) {
                                let k = acc.iter().position(|a| *a >= s).unwrap_or(acc.len() - 1).max(1);
                                let seg = acc[k] - acc[k - 1];
                                let t = if seg > 0.0 { (s - acc[k - 1]) / seg } else { 0.0 };
                                (pts[k - 1] + (pts[k] - pts[k - 1]) * t, safe_norm2(pts[k] - pts[k - 1], V2::X))
                            };
                            let params: Vec<f64> = if d.curve_by_spacing {
                                let sp = self.e(&d.curve_spacing)?;
                                if sp <= 0.0 { return Err("Der Kurvenabstand muss positiv sein.".into()); }
                                (1..).map(|k| k as f64 * sp).take_while(|s| *s <= total + 1e-9).take(400).collect()
                            } else {
                                let n = self.e(&d.curve_count)?.round().max(2.0) as usize;
                                if closed { (1..n).map(|k| total * k as f64 / n as f64).collect() } else { (1..n).map(|k| total * k as f64 / (n - 1) as f64).collect() }
                            };
                            let (p0, t0) = eval(0.0);
                            for s in params {
                                let (p, t) = eval(s);
                                let delta = fr.dir_to_world(p - p0);
                                if d.curve_follow {
                                    let ang = cross2(t0, t).atan2(t0.dot(t));
                                    let r = Xform::rotation(fr.to_world(p0), fr.z(), ang);
                                    xf.push(Xform { m: r.m, t: r.t + delta });
                                } else {
                                    xf.push(Xform::translation(delta));
                                }
                            }
                        }
                    }
                }
                if xf.len() > 400 { return Err("Das Muster hat zu viele Elemente (max. 400).".into()); }
                let mut cur = self.polys.clone();
                let mut copies = vec![];
                let mut op = Op::Add;
                let mut k = 0;
                for src in &d.features {
                    let tool = self.res.tools.get(src).cloned().ok_or("Nur Volumen-KEs (Profil, Drehen, Zug, Verbund, Bohrung) können gemustert werden.")?;
                    op = tool.op;
                    for x in &xf {
                        let copy = self.retag(tool.polys.iter().map(|p| p.transformed(x)).collect(), fid);
                        cur = if tool.op == Op::Cut { csg::subtract(&cur, &copy) } else { csg::union(&cur, &copy) };
                        copies.extend(copy);
                    }
                    let src_cos: Vec<Cosmetic> = self.res.cosmetics.iter().filter(|c| c.feature == *src).cloned().collect();
                    for c in src_cos { for x in &xf { self.res.cosmetics.push(Cosmetic { feature: fid, base: x.point(c.base), dir: x.vector(c.dir), ..c.clone() }); } }
                    // copies carry the axes of holes and revolves (extruded arcs keep a single axis, as in Creo)
                    let carries = self.data.feature(*src).map(|f| matches!(f.kind, FeatureKind::Hole(_) | FeatureKind::Revolve(_))).unwrap_or(false);
                    let src_axes: Vec<AxisInfo> = if carries { self.axes.values().filter(|a| a.feature == *src).cloned().collect() } else { vec![] };
                    for a in src_axes { for x in &xf { k += 1; self.add_axis(fid, k, x.point(a.point), x.vector(a.dir)); } }
                }
                self.res.tools.insert(fid, ToolBody { polys: copies, op });
                self.set_polys(cur);
            }
            FeatureKind::Import(d) => {
                // one surface tag per imported face so faces stay selectable
                let mut polys = vec![];
                for face in &d.faces {
                    let tag = self.tags.next(fid);
                    for t in &face.t {
                        let vx = |i: u32| face.v.get(i as usize).map(|q| crate::kernel::csg::CVertex { p: V3::new(q[0] as f64, q[1] as f64, q[2] as f64), n: V3::new(q[3] as f64, q[4] as f64, q[5] as f64) });
                        let (Some(a), Some(b), Some(c)) = (vx(t[0]), vx(t[1]), vx(t[2])) else { continue };
                        if let Some(p) = CPolygon::new(vec![a, b, c], tag) { polys.push(p); }
                    }
                }
                if polys.is_empty() { return Err("Die importierte Datei enthält keine Geometrie.".into()); }
                self.apply_tool(polys, false, fid)?;
            }
            FeatureKind::DatumCsys(d) => {
                let o = V3::new(self.e(&d.x)?, self.e(&d.y)?, self.e(&d.z)?);
                let frame = csys_frame(o, self.e(&d.rx)?, self.e(&d.ry)?, self.e(&d.rz)?);
                self.res.csys.push(CsysInfo { id: fid, name: f.name.clone(), frame });
            }
            FeatureKind::Surface(d) => { let d = d.clone(); self.apply_surface(&d, fid)?; }
            FeatureKind::Body(d) => { let d = d.clone(); self.apply_body(&d, fid)?; }
            FeatureKind::Curve(d) => {
                if d.points.len() < 2 { return Err("Mindestens zwei Bezugspunkte wählen.".into()); }
                let pts: Vec<V3> = d.points.iter().map(|id| self.res.points.iter().find(|p| p.id == *id).map(|p| p.p).ok_or("Ein Bezugspunkt der Kurve fehlt.")).collect::<Result<_, _>>()?;
                let line = if d.spline && pts.len() > 2 { catmull_rom3(&pts, 24) } else { pts };
                self.res.curves.push(CurveInfo { feature: fid, polylines: vec![line] });
            }
            FeatureKind::Rib(d) => {
                let frame = self.plane(&d.plane)?;
                let s = self.prepare(&d.sketch)?;
                let (pts, closed) = s.trajectory()?;
                if closed { return Err("Die Rippenskizze muss eine offene Kette sein.".into()); }
                if self.polys.is_empty() { return Err("Eine Rippe benötigt ein vorhandenes Volumen.".into()); }
                let th = self.e(&d.thickness)?;
                if th <= 0.0 { return Err("Die Dicke muss positiv sein.".into()); }
                let t = self.topo();
                let region = rib_region(&t, &frame, &pts, d.flip)?;
                let (z0, z1) = match d.side { 1 => (0.0, th), 2 => (-th, 0.0), _ => (-th / 2.0, th / 2.0) };
                let tool = builders::extrude(&[region], &frame, z0, z1, 0.0, &mut self.tags, fid);
                self.apply_tool(tool, false, fid)?;
            }
            FeatureKind::Helix(d) => {
                let frame = self.plane(&d.plane)?;
                let prof = self.prepare(&d.profile)?;
                let (ap, au) = prof.centerline_axis().ok_or("Im Profil eine Mittellinie als Spiralachse skizzieren.")?;
                let (pp, closed) = prof.trajectory()?;
                if closed { return Err("Das Spiralzugprofil muss eine offene Kette sein.".into()); }
                let mut samples: Vec<(f64, f64)> = pp.iter().map(|p| ((*p - ap).dot(au), cross2(au, *p - ap))).collect();
                if samples[0].0 > samples[samples.len() - 1].0 { samples.reverse(); }
                if samples.windows(2).any(|w| w[1].0 < w[0].0 - 1e-9) { return Err("Das Profil muss entlang der Achse stetig fortschreiten.".into()); }
                let side = samples.iter().map(|q| q.1).find(|r| r.abs() > 1e-9).map(f64::signum).ok_or("Das Profil darf nicht auf der Achse liegen.")?;
                if samples.iter().any(|q| q.1 * side < -1e-9) { return Err("Das Profil muss auf einer Seite der Achse liegen.".into()); }
                let (s0, len) = (samples[0].0, samples[samples.len() - 1].0 - samples[0].0);
                if len <= 1e-6 { return Err("Das Profil muss eine Länge entlang der Achse haben.".into()); }
                let pitch = self.e(&d.pitch)?;
                if pitch <= 0.0 { return Err("Die Steigung muss positiv sein.".into()); }
                let sec = self.prepare(&d.section)?;
                let regions = sec.profile_regions()?;
                let (mut amin, mut amax) = (f64::INFINITY, f64::NEG_INFINITY);
                for r in &regions { for p in &r.outer.pts { let a = (*p - ap).dot(au); amin = amin.min(a); amax = amax.max(a); } }
                if amax - amin >= pitch { return Err("Der Querschnitt ist höher als die Steigung — die Windungen würden sich überschneiden.".into()); }
                let radius = |a: f64| -> f64 {
                    let k = samples.iter().position(|q| q.0 >= a).unwrap_or(samples.len() - 1).max(1);
                    let (q0, q1) = (samples[k - 1], samples[k]);
                    let t = if q1.0 - q0.0 > 1e-12 { ((a - q0.0) / (q1.0 - q0.0)).clamp(0.0, 1.0) } else { 1.0 };
                    (q0.1 + (q1.1 - q0.1) * t).abs()
                };
                let turns = len / pitch;
                let steps = ((turns * 36.0).ceil() as usize).clamp(8, 3600);
                let (base, axis) = (frame.to_world(ap), frame.dir_to_world(au));
                let radial = frame.dir_to_world(perp_left(au));
                let hand = if d.left_handed { -1.0 } else { 1.0 };
                let r0 = radius(s0);
                let mut rings: Vec<Box<dyn Fn(V2) -> V3>> = Vec::with_capacity(steps + 1);
                for k in 0..=steps {
                    let da = len * k as f64 / steps as f64;
                    let rot = rotate_vec(radial, axis, hand * std::f64::consts::TAU * da / pitch);
                    let dr = side * (radius(s0 + da) - r0);
                    rings.push(Box::new(move |q: V2| {
                        let (a, r) = ((q - ap).dot(au), cross2(au, q - ap));
                        base + axis * (a + da) + rot * (r + dr)
                    }));
                }
                let tool = builders::loft(&regions, &rings, false, &mut self.tags, fid);
                self.apply_tool(tool, d.remove, fid)?;
                self.add_axis(fid, 1, base, axis);
            }
            FeatureKind::Thread(d) => {
                let fr = d.face.ok_or("Eine zylindrische Fläche für das Gewinde wählen.")?;
                let t = self.topo();
                let fi = t.match_surface(fr.point, fr.normal).ok_or("Die Gewindefläche wurde nicht gefunden.")?;
                let (start, dir, _, full, _) = thread_cylinder(&t, fi, d.flip).ok_or("Die gewählte Fläche ist nicht zylindrisch.")?;
                let (dia, pitch) = (self.e(&d.diameter)?, self.e(&d.pitch)?);
                if dia <= 0.0 || pitch <= 0.0 { return Err("Durchmesser und Steigung müssen positiv sein.".into()); }
                let len = self.e(&d.length)?;
                if len <= 0.0 { return Err("Die Gewindelänge muss positiv sein.".into()); }
                self.res.cosmetics.push(Cosmetic { feature: fid, base: start, dir, radius: dia / 2.0, length: len.min(full), label: thread_label(dia, pitch) });
            }
            FeatureKind::Mirror(d) => {
                let frame = self.plane(&d.plane)?;
                let x = Xform::mirror(&frame);
                if d.features.is_empty() {
                    if self.polys.is_empty() { return Err("Nichts zu spiegeln.".into()); }
                    let copy: Vec<CPolygon> = self.polys.iter().map(|p| p.transformed(&x)).collect();
                    let new = csg::union(&self.polys, &copy);
                    self.set_polys(new);
                } else {
                    let mut cur = self.polys.clone();
                    for src in &d.features {
                        let tool = self.res.tools.get(src).cloned().ok_or("Das gespiegelte KE hat keine Geometrie.")?;
                        let copy = self.retag(tool.polys.iter().map(|p| p.transformed(&x)).collect(), fid);
                        cur = if tool.op == Op::Cut { csg::subtract(&cur, &copy) } else { csg::union(&cur, &copy) };
                        let src_cos: Vec<Cosmetic> = self.res.cosmetics.iter().filter(|c| c.feature == *src).cloned().collect();
                        for c in src_cos { self.res.cosmetics.push(Cosmetic { feature: fid, base: x.point(c.base), dir: x.vector(c.dir), ..c }); }
                    }
                    self.set_polys(cur);
                }
            }
        }
        Ok(())
    }
}

/// Thin-wall regions: each profile curve thickened symmetrically.
pub fn thin_regions(s: &Sketch, t: f64) -> Result<Vec<crate::sketch::sketch::ProfileRegion>, String> {
    use crate::sketch::sketch::{ProfileRegion, SkKind};
    let ents = s.profile_entities();
    if ents.is_empty() { return Err("Die Skizze ist leer.".into()); }
    let mut out = vec![];
    for e in ents {
        let pts = s.sample(e, 64);
        if pts.len() < 2 { continue; }
        if e.kind == SkKind::Circle {
            let c = s.point(e.pts[0]);
            let outer: Vec<V2> = pts[..pts.len() - 1].iter().map(|p| c + safe_norm2(*p - c, V2::X) * (e.r + t / 2.0)).collect();
            let inner: Vec<V2> = pts[..pts.len() - 1].iter().rev().map(|p| c + safe_norm2(*p - c, V2::X) * (e.r - t / 2.0).max(0.001)).collect();
            out.push(ProfileRegion { outer: builders::simple_loop(outer), holes: vec![builders::simple_loop(inner)] });
            continue;
        }
        let n = pts.len();
        let (mut left, mut right) = (vec![], vec![]);
        for i in 0..n {
            let d = if i == 0 { pts[1] - pts[0] } else if i == n - 1 { pts[i] - pts[i - 1] } else { safe_norm2(pts[i + 1] - pts[i], V2::X) + safe_norm2(pts[i] - pts[i - 1], V2::X) };
            let nr = safe_norm2(perp_left(d), V2::Y);
            left.push(pts[i] + nr * t / 2.0);
            right.push(pts[i] - nr * t / 2.0);
        }
        left.reverse();
        let mut ring = right;
        ring.extend(left);
        if polygon_area2(&ring) < 0.0 { ring.reverse(); }
        out.push(ProfileRegion { outer: builders::simple_loop(ring), holes: vec![] });
    }
    Ok(out)
}

/// Closed rib section: the sketched chain, then back along the points where the chain,
/// pushed towards the material side, meets the solid (slightly inside it so the rib fuses).
fn rib_region(t: &Topology, frame: &Frame, chain: &[V2], flip: bool) -> Result<crate::sketch::sketch::ProfileRegion, String> {
    let total: f64 = chain.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    if total <= 1e-9 { return Err("Die Rippenskizze ist zu kurz.".into()); }
    let step = total / 160.0;
    let mut pts = vec![chain[0]];
    for w in chain.windows(2) {
        let n = ((w[1] - w[0]).length() / step).ceil().max(1.0) as usize;
        for k in 1..=n { pts.push(w[0] + (w[1] - w[0]) * (k as f64 / n as f64)); }
    }
    let m = pts.len();
    let normal = |i: usize| safe_norm2(perp_left(pts[(i + 1).min(m - 1)] - pts[i.saturating_sub(1)]), V2::Y);
    let overlap = step * 2.0;
    let cast = |sg: f64| -> Option<(Vec<V2>, f64)> {
        let mut out = Vec::with_capacity(m);
        let mut sum = 0.0;
        for i in 0..m {
            let n = normal(i) * sg;
            let h = t.ray_first_hit(frame.to_world(pts[i]), frame.dir_to_world(n), 1e-6)?;
            sum += h;
            out.push(pts[i] + n * (h + overlap));
        }
        Some((out, sum))
    };
    let (a, b) = (cast(1.0), cast(-1.0));
    // the material side is the one where the chain closes against the solid (the nearer one)
    let pick = match (a, b) {
        (Some(a), Some(b)) => if (a.1 <= b.1) != flip { a } else { b },
        (Some(a), None) => if flip { return Err("Auf der anderen Seite der Skizze liegt kein Volumen.".into()) } else { a },
        (None, Some(b)) => if flip { return Err("Auf der anderen Seite der Skizze liegt kein Volumen.".into()) } else { b },
        (None, None) => return Err("Die Rippe kann nicht an das Volumen angeschlossen werden — die Enden der Skizze müssen das Volumen berühren.".into()),
    };
    let mut ring = pts.clone();
    ring.extend(pick.0.into_iter().rev());
    if polygon_area2(&ring) < 0.0 { ring.reverse(); }
    Ok(crate::sketch::sketch::ProfileRegion { outer: builders::simple_loop(ring), holes: vec![] })
}

/// Catmull-Rom spline through 3D points, `per_seg` samples per segment.
pub fn catmull_rom3(p: &[V3], per_seg: usize) -> Vec<V3> {
    let n = p.len();
    let g = |i: i64| p[i.clamp(0, n as i64 - 1) as usize];
    let mut out = vec![];
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (g(i as i64 - 1), g(i as i64), g(i as i64 + 1), g(i as i64 + 2));
        for k in 0..per_seg {
            let t = k as f64 / per_seg as f64;
            let (t2, t3) = (t * t, t * t * t);
            out.push((p1 * 2.0 + (p2 - p0) * t + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2 + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * t3) * 0.5);
        }
    }
    out.push(p[n - 1]);
    out
}

/// Zug-Verbund: section `a` at the start of `path` blending into section `b` at its end.
fn swept_blend(a: &[V2], b: &[V2], path: &[V3], start: &Frame, tag: u32) -> Result<Vec<CPolygon>, String> {
    use crate::model::surfaces as sf;
    const N: usize = 48;
    let rings = builders::sweep_rings(path, false, start);
    // both loops resampled from the point nearest their +x direction, wound the same way
    let prep = |l: &[V2]| -> Vec<V2> {
        let c = l.iter().fold(V2::ZERO, |s, p| s + *p) / l.len() as f64;
        let area: f64 = (0..l.len()).map(|i| { let (p, q) = (l[i], l[(i + 1) % l.len()]); p.x * q.y - q.x * p.y }).sum();
        let mut pts: Vec<V2> = if area < 0.0 { l.iter().rev().copied().collect() } else { l.to_vec() };
        let ang = |p: V2| { let v = p - c; v.y.atan2(v.x).abs() };
        let k = (0..pts.len()).min_by(|i, j| ang(pts[*i]).partial_cmp(&ang(pts[*j])).unwrap()).unwrap_or(0);
        pts.rotate_left(k);
        pts.push(pts[0]);
        let p3: Vec<V3> = pts.iter().map(|p| V3::new(p.x, p.y, 0.0)).collect();
        let mut r: Vec<V2> = sf::resample(&p3, N + 1).iter().map(|p| V2::new(p.x, p.y)).collect();
        r.pop();
        r
    };
    if a.len() < 3 || b.len() < 3 { return Err("Beide Querschnitte müssen geschlossen sein.".into()); }
    let (sa, sb) = (prep(a), prep(b));
    let n = rings.len();
    let stations: Vec<Vec<V3>> = (0..n).map(|k| {
        let t = k as f64 / (n - 1).max(1) as f64;
        (0..N).map(|i| rings[k](sa[i] * (1.0 - t) + sb[i] * t)).collect()
    }).collect();
    let mut out = vec![];
    for k in 0..n - 1 {
        for i in 0..N {
            let j = (i + 1) % N;
            let q = [stations[k][i], stations[k][j], stations[k + 1][j], stations[k + 1][i]];
            out.extend(sf::tris(&q, tag, V3::ZERO).into_iter());
        }
    }
    // caps face back along the path at the start and forward at the end; the side quads keep
    // the sections' common winding, so one global flip makes the solid point outward
    let (d0, d1) = (path[1] - path[0], path[n - 1] - path[n - 2]);
    for (st, sec, facing) in [(&stations[0], &sa, -d0), (&stations[n - 1], &sb, d1)] {
        for t in crate::kernel::triangulate::triangulate(sec, &[]) {
            if let Some(mut p) = sf::poly(&[st[t[0]], st[t[1]], st[t[2]]], tag) { if p.plane.n.dot(facing) < 0.0 { p.flip(); } out.push(p); }
        }
    }
    let side_out = { let p = &out[0]; let c = stations[0].iter().chain(stations[1].iter()).fold(V3::ZERO, |s, q| s + *q) / (2 * N) as f64; p.plane.n.dot(p.centroid() - c) >= 0.0 };
    // flip the sides if they point inward
    let caps = 2 * (N - 2);
    let k = out.len() - caps.min(out.len());
    if !side_out { for p in out.iter_mut().take(k) { p.flip(); } }
    Ok(out)
}

/// Eckfase: a tetrahedron cutting the corner shared by the given edges, `d` along each edge.
fn corner_tool(t: &Topology, edges: &[EdgeRef], d: f64, tag: u32) -> Result<Vec<CPolygon>, String> {
    use crate::model::surfaces as sf;
    if edges.len() < 2 { return Err("Zwei Kanten wählen, die sich in der Ecke treffen.".into()); }
    let chains: Vec<usize> = edges.iter().map(|e| t.match_chain(e).ok_or("Eine Kantenreferenz wurde nicht gefunden.".to_string())).collect::<Result<_, _>>()?;
    let ends = |ci: usize| { let c = &t.chains[ci]; [c.points[0], *c.points.last().unwrap()] };
    let tol = (t.bbox.diagonal() * 1e-5).max(1e-6);
    let corner = ends(chains[0]).into_iter().find(|v| chains[1..].iter().all(|c| ends(*c).iter().any(|w| (*w - *v).length() < tol))).ok_or("Die gewählten Kanten treffen sich nicht in einer Ecke.")?;
    // every edge at the corner (also ones not picked), and the point d along each
    let mut pts = vec![];
    for (ci, c) in t.chains.iter().enumerate() {
        if c.closed { continue; }
        let [s, e] = ends(ci);
        let walk: Vec<V3> = if (s - corner).length() < tol { c.points.clone() } else if (e - corner).length() < tol { c.points.iter().rev().copied().collect() } else { continue };
        let mut left = d;
        let mut p = walk[0];
        for w in walk.windows(2) { let l = (w[1] - w[0]).length(); if l >= left { p = w[0] + (w[1] - w[0]) * (left / l); left = 0.0; break; } left -= l; p = w[1]; }
        if left > 1e-9 { return Err("Der Fasenabstand ist länger als eine Kante an der Ecke.".into()); }
        pts.push(p);
    }
    if pts.len() < 3 { return Err("An der Ecke treffen keine drei Kanten zusammen.".into()); }
    let m = (pts[0] + pts[1] + pts[2]) / 3.0;
    let apex = corner + (corner - m) * 0.25;
    let faces = [[apex, pts[0], pts[1]], [apex, pts[1], pts[2]], [apex, pts[2], pts[0]], [pts[0], pts[1], pts[2]]];
    let c = (apex + pts[0] + pts[1] + pts[2]) / 4.0;
    let tool: Vec<CPolygon> = faces.iter().filter_map(|f| { let mut p = sf::poly(f, tag)?; if p.plane.n.dot(p.centroid() - c) < 0.0 { p.flip(); } Some(p) }).collect();
    Ok(tool)
}
