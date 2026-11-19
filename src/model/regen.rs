//! Regeneration: replays the feature list into a solid, datums and display curves.

use super::expr::Vars;
use super::features::*;
use crate::kernel::builders::{self, TagAllocator};
use crate::kernel::csg::{self, CPolygon};
use crate::kernel::math::*;
use crate::kernel::topology::EdgeChain;
use crate::kernel::topology::Topology;
use crate::sketch::{sketch::Sketch, solver};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct PlaneInfo {
    pub id: u64,
    pub name: String,
    pub frame: Frame,
    /** display half-size around the origin; 0 = sized from the model */
    pub size: f64,
}
#[derive(Clone, Debug)]
pub struct AxisInfo {
    pub id: u64,
    pub name: String,
    pub point: V3,
    pub dir: V3,
    pub feature: u64,
}
#[derive(Clone, Debug)]
pub struct PointInfo {
    pub id: u64,
    pub name: String,
    pub p: V3,
}
#[derive(Clone, Debug)]
pub struct CsysInfo {
    pub id: u64,
    pub name: String,
    pub frame: Frame,
}
#[derive(Clone, Debug)]
pub struct CurveInfo {
    pub feature: u64,
    pub polylines: Vec<Vec<V3>>,
}

/// Cosmetic thread: display-only thread turns on a modelled cylinder (the solid keeps the
/// drill / shaft diameter).
#[derive(Clone, Debug)]
pub struct Cosmetic {
    pub feature: u64,
    pub base: V3,
    pub dir: V3,
    /// major (nominal) radius
    pub radius: f64,
    pub length: f64,
    pub label: String,
    pub pitch: f64,
    /// radius of the modelled cylinder the thread sits on (drill or shaft)
    pub wall: f64,
    /// inside a hole (nut thread) rather than on a shaft
    pub internal: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Cut,
}

#[derive(Clone)]
pub struct ToolBody {
    pub polys: Vec<CPolygon>,
    pub op: Op,
}

/// A Sammelfläche: the open sheet a surface feature made.
#[derive(Clone)]
pub struct Quilt {
    pub feature: u64,
    pub polys: Vec<CPolygon>,
}

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
    pub fn plane(&self, id: u64) -> Option<&PlaneInfo> {
        self.planes.iter().find(|p| p.id == id)
    }
    pub fn axis(&self, id: u64) -> Option<&AxisInfo> {
        self.axes.iter().find(|a| a.id == id)
    }
    pub fn frame_for(&self, r: &PlaneRef) -> Option<Frame> {
        match r {
            PlaneRef::Datum(id) => self.plane(*id).map(|p| p.frame),
            PlaneRef::Face(f) => self.topo.match_face(f.point, f.normal).and_then(|i| self.topo.face_frame(i)),
        }
    }
}

static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A fresh display generation (invalidates GPU buffers).
pub fn next_generation() -> u64 {
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

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
    let mut r = Regen::start(data);
    r.steps(features, stop);
    r.finish(features, t0)
}

/// The state of a regeneration after some features, so that the features after them can be
/// regenerated without repeating the work (the preview of a feature being defined).
#[derive(Clone)]
pub struct Snapshot {
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
    bodies: Vec<(u64, Vec<CPolygon>)>,
    cur: usize,
    /// what the snapshot was made from; `run_after` checks it is still the same
    pub features: Vec<Feature>,
    pub parameters: Vec<Parameter>,
    pub relations: String,
    feature_body: HashMap<u64, u64>,
    default_body: u64,
}

/// `run_features(data, features, None)` that also keeps the state for `run_after`.
pub fn run_snapshot(data: &ModelData, features: &[Feature]) -> (RegenResult, Snapshot) {
    let t0 = std::time::Instant::now();
    let mut r = Regen::start(data);
    r.steps(features, None);
    r.topo();
    let snap = Snapshot {
        vars: r.vars.clone(),
        polys: r.polys.clone(),
        topo: r.topo.clone(),
        tags: r.tags.clone(),
        res: r.res.clone(),
        planes: r.planes.clone(),
        axes: r.axes.clone(),
        auto_axis: r.auto_axis,
        quilts: r.quilts.clone(),
        consumed: r.consumed.clone(),
        bodies: r.bodies.clone(),
        cur: r.cur,
        features: features.to_vec(),
        parameters: data.parameters.clone(),
        relations: data.relations.clone(),
        feature_body: data.feature_body.clone(),
        default_body: data.default_body,
    };
    (r.finish(features, t0), snap)
}

impl Snapshot {
    /// The snapshot still describes the start of `data` (same features before, same parameters).
    pub fn fits(&self, data: &ModelData, before: &[Feature]) -> bool {
        self.features.as_slice() == before
            && self.parameters == data.parameters
            && self.relations == data.relations
            && self.feature_body == data.feature_body
            && self.default_body == data.default_body
    }
}

/// Keeps the regeneration state of the last run, so that a change near the end of the model
/// (a new feature, a redefined last feature) regenerates only what comes after the kept state
/// instead of the whole history.
#[derive(Default)]
pub struct Cache {
    snaps: Vec<Snapshot>,
}

impl Cache {
    pub fn clear(&mut self) {
        self.snaps.clear();
    }

    /// The state after all of `features`, if the last run ended there.
    pub fn snapshot(&self, data: &ModelData, features: &[Feature]) -> Option<&Snapshot> {
        self.snaps.iter().find(|s| s.features.len() == features.len() && s.fits(data, features))
    }

    /// Like `run`, starting from the longest kept state that still fits.
    pub fn run(&mut self, data: &ModelData) -> RegenResult {
        let shown = data.shown();
        let n = shown.insert_index.map(|i| i.min(shown.features.len())).unwrap_or(shown.features.len());
        let mut res = self.run_features(&shown, &shown.features[..n]);
        for f in &shown.features[n..] {
            res.rolled_back.insert(f.id);
        }
        res
    }

    /// Like `run_features(data, features, None)`.
    pub fn run_features(&mut self, data: &ModelData, features: &[Feature]) -> RegenResult {
        let t0 = std::time::Instant::now();
        let n = features.len();
        let best = self.snaps.iter().filter(|s| s.features.len() <= n && s.fits(data, &features[..s.features.len()])).max_by_key(|s| s.features.len());
        let (mut r, done) = match best {
            Some(s) => {
                let k = s.features.len();
                let s = s.clone();
                (
                    Regen {
                        data,
                        vars: s.vars,
                        polys: s.polys,
                        topo: s.topo,
                        tags: s.tags,
                        res: s.res,
                        planes: s.planes,
                        axes: s.axes,
                        auto_axis: s.auto_axis,
                        quilts: s.quilts,
                        consumed: s.consumed,
                        bodies: s.bodies,
                        cur: s.cur,
                    },
                    k,
                )
            }
            None => (Regen::start(data), 0),
        };
        let snap = |r: &Regen, k: usize| Snapshot {
            vars: r.vars.clone(),
            polys: r.polys.clone(),
            topo: r.topo.clone(),
            tags: r.tags.clone(),
            res: r.res.clone(),
            planes: r.planes.clone(),
            axes: r.axes.clone(),
            auto_axis: r.auto_axis,
            quilts: r.quilts.clone(),
            consumed: r.consumed.clone(),
            bodies: r.bodies.clone(),
            cur: r.cur,
            features: features[..k].to_vec(),
            parameters: data.parameters.clone(),
            relations: data.relations.clone(),
            feature_body: data.feature_body.clone(),
            default_body: data.default_body,
        };
        // keep the state before the last feature (it is the one most often redefined) and at the end
        let mut keep = vec![];
        if done < n {
            if n >= 1 && done < n - 1 {
                r.steps(&features[done..n - 1], None);
            }
            if n >= 1 && done <= n - 1 {
                keep.push(snap(&r, n - 1));
                r.steps(&features[n - 1..], None);
            }
        } else if let Some(prev) = self.snaps.iter().find(|s| s.features.len() + 1 == n && s.fits(data, &features[..n - 1])) {
            keep.push(prev.clone());
        }
        r.topo();
        keep.push(snap(&r, n));
        self.snaps = keep;
        r.finish(features, t0)
    }
}

/// Regenerates `extra` on top of a snapshot: the same result as `run_features` over the snapshot's
/// features followed by `extra`.
pub fn run_after(data: &ModelData, snap: &Snapshot, extra: &[Feature]) -> RegenResult {
    let t0 = std::time::Instant::now();
    let s = snap.clone();
    let mut r = Regen {
        data,
        vars: s.vars,
        polys: s.polys,
        topo: s.topo,
        tags: s.tags,
        res: s.res,
        planes: s.planes,
        axes: s.axes,
        auto_axis: s.auto_axis,
        quilts: s.quilts,
        consumed: s.consumed,
        bodies: s.bodies,
        cur: s.cur,
    };
    r.steps(extra, None);
    let all: Vec<Feature> = snap.features.iter().chain(extra.iter()).cloned().collect();
    r.finish(&all, t0)
}

impl<'a> Regen<'a> {
    fn start(data: &'a ModelData) -> Regen<'a> {
        let (vars, perr) = data.evaluate_parameters();
        let mut r = Regen {
            data,
            vars: vars.clone(),
            polys: vec![],
            topo: None,
            tags: TagAllocator::default(),
            res: RegenResult::default(),
            planes: HashMap::new(),
            axes: HashMap::new(),
            auto_axis: 0,
            quilts: vec![],
            consumed: HashSet::new(),
            bodies: vec![(0, vec![])],
            cur: 0,
        };
        r.res.vars = vars;
        r.res.param_errors = perr;
        r
    }

    fn steps(&mut self, features: &[Feature], stop: Option<usize>) {
        let data = self.data;
        for (i, f) in features.iter().enumerate() {
            if stop.map(|s| i >= s).unwrap_or(false) {
                self.res.rolled_back.insert(f.id);
                continue;
            }
            if f.suppressed {
                continue;
            }
            // each solid feature works on its own body
            if !matches!(f.kind, FeatureKind::Body(_)) {
                // a feature being created (not yet in the model) goes into the default body
                let b = data.feature_body.get(&f.id).copied().unwrap_or(if data.feature(f.id).is_none() { data.default_body } else { 0 });
                let i = self.bodies.iter().position(|x| x.0 == b).unwrap_or(0);
                self.switch_body(i);
            }
            if let Err(e) = self.apply(f) {
                self.res.errors.insert(f.id, e);
            }
        }
    }

    fn finish(mut self, features: &[Feature], t0: std::time::Instant) -> RegenResult {
        self.sync_out();
        self.res.bodies = self.bodies.iter().map(|b| (b.0, b.1.len())).collect();
        let all: Vec<CPolygon> = self.bodies.iter_mut().flat_map(|b| std::mem::take(&mut b.1)).collect();
        self.bodies = vec![(0, vec![])];
        self.set_polys(all);
        self.res.quilts = self.quilts.iter().filter(|q| !self.consumed.contains(&q.0)).map(|q| Quilt { feature: q.0, polys: q.1.clone() }).collect();
        let topo = self.topo();
        let mut res = self.res;
        res.polys = self.polys;
        res.topo = topo;
        res.planes = features.iter().filter_map(|f| self.planes.get(&f.id).cloned()).collect();
        let mut axes: Vec<AxisInfo> = self.axes.into_values().collect();
        axes.sort_by_key(|a| a.id);
        res.axes = axes;
        res.tag_owner = self.tags.owner;
        res.time = t0.elapsed().as_secs_f64();
        res.generation = next_generation();
        res
    }
}

/// Entfernen: takes the polygons `gone` out of the solid and closes each opening with a flat cap,
/// so the neighbouring faces run on across it (a hole, pocket, boss, round or chamfer disappears).
/// Works when every opening lies in a plane.
pub fn remove_faces(t: &Topology, gone: &HashSet<usize>, new_tag: u32) -> Result<Vec<CPolygon>, String> {
    let poly = |pi: usize| -> Option<CPolygon> {
        let v: Vec<csg::CVertex> =
            t.polys[pi].iter().enumerate().map(|(k, &i)| csg::CVertex { p: t.positions[i], n: t.vnormals[pi].get(k).copied().unwrap_or(t.planes[pi].n) }).collect();
        Some(CPolygon { v, plane: t.planes[pi], tag: t.tags[pi] })
    };
    let mut out: Vec<CPolygon> = (0..t.polys.len()).filter(|pi| !gone.contains(pi)).filter_map(poly).collect();
    if out.is_empty() {
        return Err("Es können nicht alle Flächen des Körpers entfernt werden.".into());
    }
    // directed edges of the removed polygons; the ones without a partner bound the openings
    let mut edges: HashSet<(usize, usize)> = HashSet::new();
    for &pi in gone {
        let p = &t.polys[pi];
        for k in 0..p.len() {
            edges.insert((p[k], p[(k + 1) % p.len()]));
        }
    }
    let mut next: HashMap<usize, usize> = HashMap::new();
    for &(u, v) in &edges {
        if !edges.contains(&(v, u)) {
            if next.insert(u, v).is_some() {
                return Err("Die gewählten Flächen berühren sich nur in einem Punkt; die Öffnung ist nicht eindeutig.".into());
            }
        }
    }
    if next.is_empty() {
        return Err("Die gewählten Flächen bilden den ganzen Körper.".into());
    }
    // tag of the face on the other side of an edge (u -> v as the removed polygon runs it)
    let mut owner: HashMap<(usize, usize), usize> = HashMap::new();
    for pi in (0..t.polys.len()).filter(|pi| !gone.contains(pi)) {
        let p = &t.polys[pi];
        for k in 0..p.len() {
            owner.insert((p[k], p[(k + 1) % p.len()]), pi);
        }
    }
    let mut loops: Vec<Vec<usize>> = vec![];
    while let Some((&start, _)) = next.iter().next() {
        let mut l = vec![start];
        let mut cur = start;
        loop {
            let Some(n) = next.remove(&cur) else { return Err("Die Öffnung ist nicht geschlossen.".into()) };
            if n == start {
                break;
            }
            l.push(n);
            cur = n;
            if l.len() > 200_000 {
                return Err("Die Öffnung ist nicht geschlossen.".into());
            }
        }
        loops.push(l);
    }
    // each loop must be flat; loops in one plane form caps with holes
    struct Lp {
        idx: Vec<usize>,
        n: V3,
        w: f64,
        area: f64,
    }
    let mut lps: Vec<Lp> = vec![];
    for l in loops {
        let pts: Vec<V3> = l.iter().map(|&i| t.positions[i]).collect();
        let Some(pl) = csg::CPlane::newell(&pts) else { continue };
        let size = pts.iter().fold(0.0f64, |m, p| m.max((*p - pts[0]).length())).max(1e-9);
        if pts.iter().any(|p| (pl.n.dot(*p) - pl.w).abs() > 1e-4 * size.max(1.0)) {
            return Err("Die Öffnung, die beim Entfernen entsteht, liegt nicht in einer Ebene. Flächen wählen, deren Rand eben ist (z. B. Bohrung, Tasche, Nabe, Rundung zwischen ebenen Flächen).".into());
        }
        let mut a = V3::ZERO;
        for k in 1..pts.len() - 1 {
            a += (pts[k] - pts[0]).cross(pts[k + 1] - pts[0]);
        }
        lps.push(Lp { idx: l, n: pl.n, w: pl.w, area: a.length() * 0.5 });
    }
    // a cap that lies in the removed faces themselves would change nothing (a chamfer face)
    if lps.iter().all(|l| gone.iter().any(|pi| t.planes[*pi].n.dot(l.n).abs() > 0.9999 && (t.planes[*pi].n.dot(l.n) * t.planes[*pi].w - l.w).abs() < 1e-4)) {
        return Err("Die Öffnung liegt in den entfernten Flächen selbst.".into());
    }
    lps.sort_by(|a, b| b.area.partial_cmp(&a.area).unwrap());
    let mut used = vec![false; lps.len()];
    for i in 0..lps.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let (n, w) = (lps[i].n, lps[i].w);
        let x = safe_norm(if n.x.abs() < 0.9 { V3::X.cross(n) } else { V3::Y.cross(n) }, V3::X);
        let y = n.cross(x);
        let to2 = |p: V3| V2::new(p.dot(x), p.dot(y));
        let outer: Vec<V2> = lps[i].idx.iter().map(|&k| to2(t.positions[k])).collect();
        let inside = |q: V2| {
            let mut c = false;
            let m = outer.len();
            for a in 0..m {
                let (p0, p1) = (outer[a], outer[(a + 1) % m]);
                if (p0.y > q.y) != (p1.y > q.y) && q.x < (p1.x - p0.x) * (q.y - p0.y) / (p1.y - p0.y) + p0.x {
                    c = !c;
                }
            }
            c
        };
        // smaller loops in the same plane that run the other way round are holes of this cap
        let mut ids: Vec<usize> = lps[i].idx.clone();
        let mut holes: Vec<Vec<V2>> = vec![];
        for j in i + 1..lps.len() {
            if used[j] || lps[j].n.dot(n) > -0.999 || (lps[j].w + w).abs() > 1e-4 {
                continue;
            }
            let h: Vec<V2> = lps[j].idx.iter().map(|&k| to2(t.positions[k])).collect();
            if !inside(h[0]) {
                continue;
            }
            used[j] = true;
            ids.extend(lps[j].idx.iter().copied());
            holes.push(h);
        }
        // the cap continues the face beside it when that face lies in the same plane
        let l0 = &lps[i].idx;
        let beside = owner.get(&(l0[1], l0[0])).copied().filter(|pi| t.planes[*pi].n.dot(n) > 0.9999 && (t.planes[*pi].w - w).abs() < 1e-4);
        let tag = beside.map(|pi| t.tags[pi]).unwrap_or(new_tag);
        for tri in crate::kernel::triangulate::triangulate(&outer, &holes) {
            let v: Vec<csg::CVertex> = tri.iter().map(|&k| csg::CVertex { p: t.positions[ids[k]], n }).collect();
            if let Some(mut p) = CPolygon::new(v, tag) {
                if p.plane.n.dot(n) < 0.0 {
                    p.flip();
                }
                out.push(p);
            }
        }
    }
    Ok(out)
}

/// A box given by a corner frame: centre `c`, half sizes along the unit axes.
fn obox(c: V3, ax: [V3; 3], h: [f64; 3], tag: u32) -> Vec<CPolygon> {
    let p = |i: f64, j: f64, k: f64| c + ax[0] * (i * h[0]) + ax[1] * (j * h[1]) + ax[2] * (k * h[2]);
    let quads = [
        [p(-1., -1., -1.), p(-1., 1., -1.), p(1., 1., -1.), p(1., -1., -1.)],
        [p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.)],
        [p(-1., -1., -1.), p(1., -1., -1.), p(1., -1., 1.), p(-1., -1., 1.)],
        [p(-1., 1., -1.), p(-1., 1., 1.), p(1., 1., 1.), p(1., 1., -1.)],
        [p(-1., -1., -1.), p(-1., -1., 1.), p(-1., 1., 1.), p(-1., 1., -1.)],
        [p(1., -1., -1.), p(1., 1., -1.), p(1., 1., 1.), p(1., -1., 1.)],
    ];
    csg::orient_outward(quads.iter().filter_map(|q| CPolygon::flat(q, tag)).collect())
}

/// Entfernen of faces whose opening is not flat (a round or chamfer between planar faces): the
/// planar faces around them are extended until they meet. Returns the body to add (a convex
/// round or chamfer gets its corner back) or to cut away (a concave one loses its fillet).
pub fn extend_over(t: &Topology, gone: &HashSet<usize>, tag: u32) -> Result<(Vec<CPolygon>, bool), String> {
    // faces beside the removed ones
    let mut edges: HashSet<(usize, usize)> = HashSet::new();
    for &pi in gone {
        let p = &t.polys[pi];
        for k in 0..p.len() {
            edges.insert((p[k], p[(k + 1) % p.len()]));
        }
    }
    let mut planes: Vec<(V3, f64)> = vec![];
    for pi in (0..t.polys.len()).filter(|pi| !gone.contains(pi)) {
        let p = &t.polys[pi];
        if !(0..p.len()).any(|k| edges.contains(&(p[(k + 1) % p.len()], p[k]))) {
            continue;
        }
        if !t.faces[t.poly_face[pi]].planar {
            return Err("Entfernen: die Flächen neben den gewählten sind gekrümmt und lassen sich nicht über die Lücke verlängern.".into());
        }
        let (n, w) = (t.planes[pi].n, t.planes[pi].w);
        if !planes.iter().any(|q| q.0.dot(n) > 0.9999 && (q.1 - w).abs() < 1e-5) {
            planes.push((n, w));
        }
    }
    if planes.len() < 2 {
        return Err(String::new());
    }
    let mut bb = BBox::default();
    let (mut c, mut k) = (V3::ZERO, 0.0);
    for &pi in gone {
        for &i in &t.polys[pi] {
            bb.add(t.positions[i]);
            c += t.positions[i];
            k += 1.0;
        }
    }
    let c = c / k;
    // reach beyond the removed faces far enough to hold the corner they replaced
    let size = bb.size();
    let reach = [size.x, size.y, size.z].into_iter().filter(|v| *v > 1e-6).fold(f64::INFINITY, f64::min);
    let m = if reach.is_finite() { reach * 0.5 } else { 1.0 };
    let big = bb.diagonal() * 4.0 + 10.0;
    let mut plug = obox(bb.center(), [V3::X, V3::Y, V3::Z], [size.x / 2.0 + m, size.y / 2.0 + m, size.z / 2.0 + m], tag);
    // keep, of every neighbouring plane, the side the removed faces are on
    let inside = planes.iter().all(|(n, w)| n.dot(c) - w < 1e-7);
    for (n, w) in &planes {
        let keep_back = n.dot(c) - w < 1e-7;
        let dir = if keep_back { -*n } else { *n };
        let x = safe_norm(if n.x.abs() < 0.9 { V3::X.cross(*n) } else { V3::Y.cross(*n) }, V3::X);
        let slab = obox(*n * *w + dir * big, [x, n.cross(x), *n], [big, big, big], tag);
        plug = csg::intersect(&plug, &slab);
        if plug.is_empty() {
            return Err("Entfernen: die Nachbarflächen schließen die Lücke nicht.".into());
        }
    }
    Ok((plug, inside))
}

/// Joins the chosen edges that continue each other tangentially into one chain, as Creo rounds
/// and chamfers a tangent chain as a single edge. Without this each piece gets its own tool, and
/// the tools leave slivers and gaps where they meet.
pub fn merge_tangent(mut chains: Vec<EdgeChain>) -> Vec<EdgeChain> {
    fn reverse(c: &mut EdgeChain) {
        c.points.reverse();
        c.n_a.reverse();
        c.n_b.reverse();
        c.f_a.reverse();
        c.f_b.reverse();
    }
    let near = |a: V3, b: V3| (a - b).length() < 1e-6;
    loop {
        let mut found = None;
        'search: for i in 0..chains.len() {
            for j in 0..chains.len() {
                if i == j || chains[i].closed || chains[j].closed || chains[i].convex != chains[j].convex || chains[i].points.len() < 2 || chains[j].points.len() < 2 {
                    continue;
                }
                let (a, b) = (&chains[i], &chains[j]);
                let (ae, bs, be) = (*a.points.last().unwrap(), b.points[0], *b.points.last().unwrap());
                // j continues at the end of i, either way round
                for rev in [false, true] {
                    let (start, next) = if rev { (be, b.points[b.points.len() - 2]) } else { (bs, b.points[1]) };
                    if !near(ae, start) {
                        continue;
                    }
                    let da = safe_norm(ae - a.points[a.points.len() - 2], V3::X);
                    let db = safe_norm(next - start, V3::X);
                    if da.dot(db) > 0.98 {
                        found = Some((i, j, rev));
                        break 'search;
                    }
                }
            }
        }
        let Some((i, j, rev)) = found else { break };
        let mut b = chains[j].clone();
        if rev {
            reverse(&mut b);
        }
        let a = &mut chains[i];
        // the same face must stay on the same side along the whole chain
        let na = *a.n_a.last().unwrap();
        if b.n_a[0].dot(na) < b.n_b[0].dot(na) {
            std::mem::swap(&mut b.n_a, &mut b.n_b);
            std::mem::swap(&mut b.f_a, &mut b.f_b);
        }
        a.points.extend(b.points.into_iter().skip(1));
        a.n_a.extend(b.n_a.into_iter().skip(1));
        a.n_b.extend(b.n_b.into_iter().skip(1));
        a.f_a.extend(b.f_a.into_iter().skip(1));
        a.f_b.extend(b.f_b.into_iter().skip(1));
        if a.points.len() > 3 && near(a.points[0], *a.points.last().unwrap()) {
            // the chain closes on itself, tangentially
            let d0 = safe_norm(a.points[1] - a.points[0], V3::X);
            let d1 = safe_norm(*a.points.last().unwrap() - a.points[a.points.len() - 2], V3::X);
            if d0.dot(d1) > 0.98 {
                a.points.pop();
                a.n_a.pop();
                a.n_b.pop();
                a.f_a.pop();
                a.f_b.pop();
                a.closed = true;
            }
        }
        chains.remove(j);
    }
    chains
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
    if !(hi > lo) {
        return None;
    }
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
    for o in offsets {
        acc += o;
        out.push(base.offset(if flip { -acc } else { acc }));
    }
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

/// Solve signed offsets to two reference planes in the placement plane.
/// A missing reference keeps the original local X or Y coordinate.
pub fn hole_position(frame: &Frame, x: f64, y: f64, references: [Option<Frame>; 2]) -> Result<V2, String> {
    let mut rows = [(1.0, 0.0, x), (0.0, 1.0, y)];
    for (i, reference) in references.into_iter().enumerate() {
        if let Some(reference) = reference {
            let normal = reference.z();
            rows[i] = (normal.dot(frame.x), normal.dot(frame.y), [x, y][i] + normal.dot(reference.origin - frame.origin));
        }
    }
    let [(a, b, c), (d, e, f)] = rows;
    let determinant = a * e - b * d;
    if determinant.abs() < 1e-8 {
        return Err("Die Versatzreferenzen müssen die Platzierungsebene in zwei unabhängigen Richtungen schneiden.".into());
    }
    Ok(V2::new((c * e - b * f) / determinant, (a * f - c * d) / determinant))
}

/// Creo's hole note ("Notiz"): size and depth of the hole, e.g. "⌀12.000 ↧ 30.130".
pub fn hole_note_text(d: &HoleDef, vars: &Vars) -> String {
    let num = |x: &Dim| format!("{:.3}", x.value(vars, 0.0));
    let depth = match d.depth_kind() {
        DepthType::ThroughAll => " DURCH ALLE".to_string(),
        DepthType::ToNext => " BIS NÄCHSTE".to_string(),
        DepthType::ToSelected => " BIS FLÄCHE".to_string(),
        _ => format!(" ↧ {}", num(&d.depth)),
    };
    if d.sketched {
        return format!("SKIZZIERTE BOHRUNG{}", if d.depth_kind() == DepthType::ThroughAll { depth } else { String::new() });
    }
    match d.ty {
        HoleType::Standard => match super::threads::find(d.standard, &d.size) {
            Some(s) if d.tapped => {
                let td = if d.through_all { String::new() } else { format!(" ↧ {}", num(&d.thread_depth)) };
                format!("{}{}\n⌀{}{}", super::threads::callout(d.standard, s), td, format!("{:.3}", hole_drill(d, vars).unwrap_or(0.0)), depth)
            }
            _ => format!("⌀{}{} ({} DL)", format!("{:.3}", hole_drill(d, vars).unwrap_or(0.0)), depth, d.size),
        },
        HoleType::Counterbore => format!("⌀{}{}\n⌴ ⌀{} ↧ {}", num(&d.diameter), depth, num(&d.cb_diameter), num(&d.cb_depth)),
        HoleType::Countersink => format!("⌀{}{}\n⌵ ⌀{} x {}°", num(&d.diameter), depth, num(&d.cs_diameter), num(&d.cs_angle)),
        HoleType::Simple => format!("⌀{}{}", num(&d.diameter), depth),
    }
}

pub fn hole_profile(d: &HoleDef, vars: &Vars, through: f64) -> Result<Vec<V2>, String> {
    let r = hole_drill(d, vars)? / 2.0;
    if r <= 0.0 {
        return Err("Der Bohrungsdurchmesser muss positiv sein.".into());
    }
    let depth = if d.through_all { through } else { d.depth.eval(vars)? };
    if depth <= 0.0 {
        return Err("Die Bohrungstiefe muss positiv sein.".into());
    }
    let eps = (r * 0.02).max(0.01);
    let tip = |depth: f64| if d.drill_tip && !d.through_all { V2::new(0.0, depth + r / 59f64.to_radians().tan()) } else { V2::new(0.0, depth) };
    Ok(match d.ty {
        HoleType::Simple | HoleType::Standard => vec![V2::new(0.0, -eps), V2::new(r, -eps), V2::new(r, depth), tip(depth)],
        HoleType::Counterbore => {
            let (cr, cd) = (d.cb_diameter.eval(vars)? / 2.0, d.cb_depth.eval(vars)?);
            if cr <= r {
                return Err("Der Senkungsdurchmesser muss größer als der Bohrungsdurchmesser sein.".into());
            }
            vec![V2::new(0.0, -eps), V2::new(cr, -eps), V2::new(cr, cd), V2::new(r, cd), V2::new(r, depth), tip(depth)]
        }
        HoleType::Countersink => {
            let sr = d.cs_diameter.eval(vars)? / 2.0;
            let half = (d.cs_angle.eval(vars)? / 2.0).to_radians();
            if sr <= r {
                return Err("Der Kegelsenkungsdurchmesser muss größer als der Bohrungsdurchmesser sein.".into());
            }
            let h = (sr - r) / half.tan();
            vec![V2::new(0.0, -eps), V2::new(sr + eps * half.tan(), -eps), V2::new(r, h), V2::new(r, depth), tip(depth)]
        }
    })
}

/// Profil Skizziert: the sketch's regions revolved once about its centreline. The centreline
/// becomes the hole axis and the top of the profile (the end the centreline points to) sits on
/// the placement plane, as in Creo.
pub fn sketched_hole_tool(s: &Sketch, base: V3, dir: V3, xdir: V3, tags: &mut TagAllocator, fid: u64) -> Result<Vec<CPolygon>, String> {
    let (ap, au0) = s.centerline_axis().ok_or("Im Bohrungsprofil eine Mittellinie als Bohrungsachse skizzieren.")?;
    let regions = s.profile_regions()?;
    if regions.is_empty() {
        return Err("Das Bohrungsprofil muss geschlossen sein.".into());
    }
    // the axis points up, out of the hole
    let au = if au0.y < 0.0 { -au0 } else { au0 };
    let mut side = 0.0;
    let mut top = f64::NEG_INFINITY;
    for r in &regions {
        for p in &r.outer.pts {
            let c = cross2(au, *p - ap);
            top = top.max((*p - ap).dot(au));
            if c.abs() < 1e-7 {
                continue;
            }
            if side == 0.0 {
                side = c;
            } else if side * c < 0.0 {
                return Err("Das Bohrungsprofil muss auf einer Seite der Mittellinie liegen.".into());
            }
        }
    }
    // sketch (x, y) -> world: au -> -dir, its normal -> xdir
    let perp = V2::new(au.y, -au.x);
    let ex = -dir * au.x + xdir * perp.x;
    let ey = -dir * au.y + xdir * perp.y;
    let anchor = ap + au * top;
    let frame = Frame { origin: base - ex * anchor.x - ey * anchor.y, x: ex, y: ey };
    Ok(builders::revolve(&regions, &frame, ap, au, 0.0, std::f64::consts::TAU, tags, fid))
}

/// Corners where edges of a round or chamfer meet: the shared end point and the indices (into
/// `chains`) of the chains ending there. Only corners with two or more chains are returned.
pub fn edge_corners(chains: &[EdgeChain], tol: f64) -> Vec<(V3, Vec<usize>)> {
    let mut out: Vec<(V3, Vec<usize>)> = vec![];
    for (i, c) in chains.iter().enumerate() {
        if c.closed || c.points.is_empty() {
            continue;
        }
        for p in [c.points[0], *c.points.last().unwrap()] {
            match out.iter_mut().find(|(q, _)| (*q - p).length() < tol) {
                Some((_, l)) => {
                    if !l.contains(&i) {
                        l.push(i)
                    }
                }
                None => out.push((p, vec![i])),
            }
        }
    }
    out.retain(|(_, l)| l.len() >= 2);
    out
}

/// The transition stored for a corner (Standard when none is).
pub fn transition_at(list: &[Transition], p: V3, tol: f64) -> TransitionKind {
    list.iter().find(|t| (t.point - p).length() < tol).map(|t| t.kind).unwrap_or_default()
}

/// Vollrundung: radius of the circle touching the face between two parallel edges and both side
/// faces; rounding both edges with it replaces the face by one round.
pub fn full_round_radius(c1: &EdgeChain, c2: &EdgeChain) -> Result<f64, String> {
    let err = || "Vollrundung: zwei parallele, gerade Kanten einer gemeinsamen Fläche wählen.".to_string();
    if !(c1.is_straight() && c2.is_straight()) || c1.points.len() < 2 || c2.points.len() < 2 {
        return Err(err());
    }
    let e = safe_norm(c1.points[1] - c1.points[0], V3::X);
    if safe_norm(c2.points[1] - c2.points[0], V3::X).cross(e).length() > 1e-3 {
        return Err(err());
    }
    let (p1, p2) = (c1.points[0], c2.points[0]);
    let (a1, b1, a2, b2) = (c1.n_a[0], c1.n_b[0], c2.n_a[0], c2.n_b[0]);
    // the face both edges border
    let same = |u: V3, v: V3| u.dot(v) > 1.0 - 1e-6;
    let (top, s1, s2) = if same(a1, a2) {
        (a1, b1, b2)
    } else if same(a1, b2) {
        (a1, b1, a2)
    } else if same(b1, a2) {
        (b1, a1, b2)
    } else if same(b1, b2) {
        (b1, a1, a2)
    } else {
        return Err(err());
    };
    // centre x and radius r: n.(x - p) = -r for the top and both sides, e.(x - p1) = 0
    let rows = [(top, top.dot(p1)), (s1, s1.dot(p1)), (s2, s2.dot(p2))];
    let mut m = [[0.0f64; 5]; 4];
    for (i, (n, w)) in rows.iter().enumerate() {
        m[i] = [n.x, n.y, n.z, 1.0, *w];
    }
    m[3] = [e.x, e.y, e.z, 0.0, e.dot(p1)];
    for col in 0..4 {
        let piv = (col..4).max_by(|&a, &b| m[a][col].abs().partial_cmp(&m[b][col].abs()).unwrap()).unwrap();
        if m[piv][col].abs() < 1e-9 {
            return Err(err());
        }
        m.swap(col, piv);
        for r in 0..4 {
            if r != col {
                let f = m[r][col] / m[col][col];
                for k in col..5 {
                    m[r][k] -= f * m[col][k];
                }
            }
        }
    }
    let r = m[3][4] / m[3][3];
    if !(r > 1e-6) {
        return Err(err());
    }
    Ok(r)
}

/// Closest distance from `p` to a polyline.
pub fn polyline_distance(p: V3, line: &[V3]) -> f64 {
    let mut best = f64::INFINITY;
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.dot(ab).max(1e-18)).clamp(0.0, 1.0);
        best = best.min((a + ab * t - p).length());
    }
    if line.len() == 1 {
        best = (line[0] - p).length();
    }
    best
}

impl<'a> Regen<'a> {
    fn topo(&mut self) -> Arc<Topology> {
        if let Some(t) = &self.topo {
            return t.clone();
        }
        // faces of every body can be referenced
        let t = if self.bodies.len() > 1 {
            let mut all: Vec<CPolygon> = self.bodies.iter().enumerate().filter(|(i, _)| *i != self.cur).flat_map(|(_, b)| b.1.iter().cloned()).collect();
            all.extend(self.polys.iter().cloned());
            Arc::new(Topology::new(&all))
        } else {
            Arc::new(Topology::new(&self.polys))
        };
        self.topo = Some(t.clone());
        t
    }

    fn set_polys(&mut self, p: Vec<CPolygon>) {
        self.polys = p;
        self.topo = None;
    }

    fn sync_out(&mut self) {
        self.bodies[self.cur].1 = std::mem::take(&mut self.polys);
        self.topo = None;
    }
    fn sync_in(&mut self) {
        self.polys = std::mem::take(&mut self.bodies[self.cur].1);
        self.topo = None;
    }
    fn switch_body(&mut self, i: usize) {
        if i != self.cur && i < self.bodies.len() {
            self.sync_out();
            self.cur = i;
            self.sync_in();
        }
    }
    fn body_index(&self, id: u64) -> Result<usize, String> {
        self.bodies.iter().position(|b| b.0 == id).ok_or("Der Körper fehlt oder ist unterdrückt.".into())
    }

    /// The polygons of the given quilt features, marking them consumed when `consume`.
    fn quilt_inputs(&mut self, ids: &[u64], consume: bool) -> Result<Vec<CPolygon>, String> {
        let mut out = vec![];
        for id in ids {
            let found: Vec<CPolygon> = self.quilts.iter().filter(|q| q.0 == *id).flat_map(|q| q.1.iter().cloned()).collect();
            if found.is_empty() {
                return Err("Eine gewählte Sammelfläche fehlt oder ist verbraucht.".into());
            }
            out.extend(found);
            if consume {
                self.consumed.insert(*id);
            }
        }
        if out.is_empty() {
            return Err("Eine Sammelfläche wählen.".into());
        }
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
                if let Some(mut p) = crate::model::surfaces::poly(&pts, t.tags[pi]) {
                    if p.plane.n.dot(t.faces[i].normal) < 0.0 {
                        p.flip();
                    }
                    out.push(p);
                }
            }
        }
        if out.is_empty() {
            return Err("Flächen wählen.".into());
        }
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
                if q.is_empty() {
                    return Err("Eine geschlossene Skizze zeichnen.".into());
                }
                self.quilts.push((fid, q));
            }
            SurfOp::Offset => {
                let src = if d.inputs.is_empty() { self.face_polys(&d.faces)? } else { self.quilt_inputs(&d.inputs, false)? };
                let q = sf::offset(&src, self.e(&d.distance)? * sg)
                    .into_iter()
                    .map(|mut p| {
                        p.tag = tag;
                        p
                    })
                    .collect();
                self.quilts.push((fid, q));
            }
            SurfOp::Thicken => {
                let src = self.quilt_inputs(&d.inputs, true)?;
                let t = self.e(&d.distance)?;
                if t <= 0.0 {
                    return Err("Die Dicke muss positiv sein.".into());
                }
                let solid = csg::orient_outward(
                    sf::thicken(&src, t * sg, None, tag)
                        .into_iter()
                        .map(|mut p| {
                            p.tag = tag;
                            p
                        })
                        .collect(),
                );
                self.apply_tool(solid, d.remove, fid)?;
            }
            SurfOp::Solidify => {
                let src = self.quilt_inputs(&d.inputs, true)?;
                if !sf::is_closed(&src) {
                    return Err("Die Sammelfläche ist nicht geschlossen — zuerst zusammenführen oder Aufdicken verwenden.".into());
                }
                let solid = csg::orient_outward(
                    src.into_iter()
                        .map(|mut p| {
                            p.tag = tag;
                            p
                        })
                        .collect(),
                );
                self.apply_tool(solid, d.remove, fid)?;
            }
            SurfOp::Merge => {
                if d.inputs.len() < 2 {
                    return Err("Zwei Sammelflächen wählen.".into());
                }
                let q = self.quilt_inputs(&d.inputs, true)?;
                self.quilts.push((fid, q));
            }
            SurfOp::Trim | SurfOp::Split => {
                let src = self.quilt_inputs(&d.inputs, true)?;
                let frame = self.plane(&d.plane)?;
                let (n, w) = (frame.z(), frame.z().dot(frame.origin));
                let keep = sf::clip(&src, n, w, !d.flip);
                if keep.is_empty() {
                    return Err("Die Ebene schneidet die Sammelfläche nicht.".into());
                }
                self.quilts.push((fid, keep));
                if d.op == SurfOp::Split {
                    self.quilts.push((fid, sf::clip(&src, n, w, d.flip)));
                }
            }
            SurfOp::Intersect => {
                let frame = self.plane(&d.plane)?;
                let src = if d.inputs.is_empty() { self.polys.clone() } else { self.quilt_inputs(&d.inputs, false)? };
                let segs = sf::section(&src, frame.z(), frame.z().dot(frame.origin));
                if segs.is_empty() {
                    return Err("Die Ebene schneidet die Geometrie nicht.".into());
                }
                self.res.curves.push(CurveInfo { feature: fid, polylines: segs });
            }
            SurfOp::Project => {
                use crate::sketch::sketch::SkKind;
                let frame = self.plane(&d.plane)?;
                let s = self.prepare(&d.sketch)?;
                let t = self.topo();
                let mut lines = vec![];
                for e in s.entities.iter().filter(|e| !matches!(e.kind, SkKind::Point | SkKind::Csys | SkKind::Text) && !e.construction) {
                    let pts: Vec<V3> = s
                        .sample(e, 64)
                        .iter()
                        .filter_map(|p| {
                            let w = frame.to_world(*p);
                            t.ray_first_hit(w, -frame.z(), 1e-6).map(|h| w - frame.z() * h).or_else(|| t.ray_first_hit(w, frame.z(), 1e-6).map(|h| w + frame.z() * h))
                        })
                        .collect();
                    if pts.len() >= 2 {
                        lines.push(pts);
                    }
                }
                if lines.is_empty() {
                    return Err("Die Skizze trifft beim Projizieren keine Fläche.".into());
                }
                self.res.curves.push(CurveInfo { feature: fid, polylines: lines });
            }
            SurfOp::Extend => {
                let src = self.quilt_inputs(&d.inputs, true)?;
                self.quilts.push((fid, sf::extend(&src, self.e(&d.distance)?, tag)));
            }
            SurfOp::Copy => {
                let q = self
                    .face_polys(&d.faces)?
                    .into_iter()
                    .map(|mut p| {
                        p.tag = tag;
                        p
                    })
                    .collect();
                self.quilts.push((fid, q));
            }
            SurfOp::Remove => {
                if self.polys.is_empty() {
                    return Err("Das Modell hat keine Flächen, die entfernt werden können.".into());
                }
                if d.faces.is_empty() {
                    return Err("Die zu entfernenden Flächen wählen.".into());
                }
                let t = Topology::new(&self.polys);
                let mut gone: HashSet<usize> = HashSet::new();
                for fr in &d.faces {
                    let i = t.match_surface(fr.point, fr.normal).or_else(|| t.match_face(fr.point, fr.normal)).ok_or("Eine gewählte Fläche wurde nicht gefunden.")?;
                    gone.extend(t.faces[i].polys.iter().copied());
                }
                // openings that lie in a plane are capped (hole, pocket, boss); otherwise the
                // neighbouring planar faces are extended across the gap (round, chamfer)
                let new = match remove_faces(&t, &gone, tag) {
                    Ok(p) => p,
                    Err(cap_err) => {
                        let (plug, add) = extend_over(&t, &gone, tag).map_err(|e| if e.is_empty() { cap_err } else { e })?;
                        if add {
                            csg::union_local(&self.polys, &plug)
                        } else {
                            csg::subtract_local(&self.polys, &plug)
                        }
                    }
                };
                self.set_polys(new);
            }
            SurfOp::Replace => {
                let faces = self.face_polys(&d.faces)?;
                let target = self.quilt_inputs(&d.inputs, true)?;
                let (n, w) = (target[0].plane.n, target[0].plane.w);
                let nf = faces[0].plane.n;
                let c = faces.iter().map(|p| p.centroid()).fold(V3::ZERO, |a, b| a + b) / faces.len() as f64;
                let den = n.dot(nf);
                if den.abs() < 1e-6 {
                    return Err("Die Ersatzfläche ist zur Fläche parallel ausgerichtet.".into());
                }
                let dist = (w - n.dot(c)) / den;
                let prism = csg::orient_outward(
                    sf::thicken(&faces, 0.0, Some(nf * dist), tag)
                        .into_iter()
                        .map(|mut p| {
                            p.tag = tag;
                            p
                        })
                        .collect(),
                );
                self.apply_tool(prism, dist < 0.0, fid)?;
            }
            SurfOp::Boundary => {
                let mut curves = vec![];
                for id in &d.inputs {
                    for c in self.res.curves.iter().filter(|c| c.feature == *id) {
                        curves.extend(c.polylines.iter().filter(|l| l.len() >= 2).cloned());
                    }
                }
                if curves.len() < 2 {
                    return Err("Zwei oder vier Randkurven (Skizzen oder Kurven) wählen.".into());
                }
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
                    if ti == oi {
                        return Err("Zwei verschiedene Körper wählen.".into());
                    }
                    let (a, b) = (self.bodies[ti].1.clone(), std::mem::take(&mut self.bodies[oi].1));
                    if b.is_empty() {
                        return Err("Der zweite Körper ist leer.".into());
                    }
                    self.bodies[ti].1 = match d.op {
                        BodyOp::Union => csg::union(&a, &b),
                        BodyOp::Subtract => csg::subtract(&a, &b),
                        _ => csg::intersect(&a, &b),
                    };
                }
                BodyOp::Split => {
                    let ti = self.body_index(d.target)?;
                    let mut frame = self.plane(&d.plane)?;
                    if d.flip {
                        frame = Frame { origin: frame.origin, x: frame.x, y: -frame.y };
                    }
                    let body = self.bodies[ti].1.clone();
                    if body.is_empty() {
                        return Err("Der Körper ist leer.".into());
                    }
                    let big = csg::bbox(&body).diagonal() * 2.0 + 10.0;
                    let tag = self.tags.next(fid);
                    let hs = crate::model::surfaces::half_space(&frame, big, tag);
                    let (keep, cut) = (csg::subtract(&body, &hs), csg::intersect(&body, &hs));
                    if keep.is_empty() || cut.is_empty() {
                        return Err("Die Ebene teilt den Körper nicht.".into());
                    }
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

    fn e(&self, d: &Dim) -> Result<f64, String> {
        d.eval(&self.vars)
    }

    fn through(&self, f: &Frame) -> f64 {
        let b = csg::bbox(&self.polys);
        if b.is_empty() {
            1000.0
        } else {
            2.0 * (b.diagonal() + (f.origin - b.center()).length()) + 10.0
        }
    }

    fn prepare(&self, s: &Sketch) -> Result<Sketch, String> {
        let mut s = s.clone();
        if s.constraints.iter().any(|c| c.expr.as_ref().map(|e| !e.trim().is_empty()).unwrap_or(false)) && s.apply_expressions(&self.vars)? && !solver::solve(&mut s, None) {
            return Err("Die Skizze konnte mit den aktuellen Parameterwerten nicht gelöst werden.".into());
        }
        Ok(s)
    }

    /// Give copied geometry fresh surface tags owned by `fid` (one new tag per source tag).
    fn retag(&mut self, mut polys: Vec<CPolygon>, fid: u64) -> Vec<CPolygon> {
        let mut map: HashMap<u32, u32> = HashMap::new();
        for p in &mut polys {
            p.tag = *map.entry(p.tag).or_insert_with(|| self.tags.next(fid));
        }
        polys
    }

    fn add_axis(&mut self, feature: u64, k: u64, point: V3, dir: V3) {
        self.auto_axis += 1;
        let id = feature * 1000 + k;
        self.axes.insert(id, AxisInfo { id, name: format!("A_{}", self.auto_axis), point, dir: safe_norm(dir, V3::Z), feature });
    }

    fn apply_tool(&mut self, tool: Vec<CPolygon>, remove: bool, fid: u64) -> Result<(), String> {
        if tool.is_empty() {
            return Err("Das KE hat keine Geometrie erzeugt.".into());
        }
        let new = if remove { csg::subtract_local(&self.polys, &tool) } else { csg::union_local(&self.polys, &tool) };
        self.res.tools.insert(fid, ToolBody { polys: tool, op: if remove { Op::Cut } else { Op::Add } });
        self.set_polys(new);
        Ok(())
    }

    fn add_curves(&mut self, s: &Sketch, f: &Frame, fid: u64) {
        use crate::sketch::sketch::SkKind;
        let mut lines: Vec<Vec<V3>> = vec![];
        for e in s.entities.iter().filter(|e| !matches!(e.kind, SkKind::Point | SkKind::Csys)) {
            if e.kind == SkKind::Text {
                for mut l in s.text_loops(e) {
                    if let Some(p) = l.first().copied() {
                        l.push(p);
                    }
                    lines.push(l.iter().map(|p| f.to_world(*p)).collect());
                }
            } else {
                lines.push(s.sample(e, 64).iter().map(|p| f.to_world(*p)).collect());
            }
        }
        self.res.curves.push(CurveInfo { feature: fid, polylines: lines });
    }

    fn apply(&mut self, f: &Feature) -> Result<(), String> {
        let fid = f.id;
        let placement = match &f.kind {
            FeatureKind::Hole(d) => d.plane,
            k => k.sketch_plane(),
        };
        if placement.is_some() {
            if let Ok(fr) = self.plane(&placement) {
                self.res.frames.insert(fid, fr);
            }
        }
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
                        if dd.length() < 1e-6 {
                            return Err("Die gewählten Ebenen sind parallel.".into());
                        }
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
                    DepthType::Blind => {
                        let dep = self.e(&d.depth)?;
                        if dep <= 0.0 {
                            return Err("Die Tiefe muss positiv sein.".into());
                        }
                        z1 = dep * sg;
                    }
                    DepthType::Symmetric => {
                        let dep = self.e(&d.depth)?;
                        if dep <= 0.0 {
                            return Err("Die Tiefe muss positiv sein.".into());
                        }
                        z0 = -dep / 2.0 * sg;
                        z1 = dep / 2.0 * sg;
                    }
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
                                    if let Some(h) = t.ray_first_hit(frame.to_world(*p), dir, 1e-4) {
                                        best = Some(best.map_or(h, |b: f64| b.max(h)));
                                    }
                                }
                            }
                        }
                        z1 = best.ok_or("Bis zur nächsten: keine Fläche in Profilrichtung gefunden.")? * sg;
                    }
                }
                if d.depth_type != DepthType::Symmetric {
                    match d.side2 {
                        Side2::None => {
                            if d.remove {
                                z0 = -0.001 * sg
                            }
                        }
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
                    let half = if keep_neg {
                        builders::extrude(&[region], &target, -l, 0.0, 0.0, &mut self.tags, fid)
                    } else {
                        builders::extrude(&[region], &target, 0.0, l, 0.0, &mut self.tags, fid)
                    };
                    tool = csg::intersect(&tool, &half);
                    if tool.is_empty() {
                        return Err("Bis zur ausgewählten: die Endreferenz wird in Profilrichtung nicht erreicht.".into());
                    }
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
                        if c.abs() < 1e-7 {
                            continue;
                        }
                        if side == 0.0 {
                            side = c
                        } else if side * c < 0.0 {
                            return Err("Der Querschnitt muss auf einer Seite der Drehachse liegen.".into());
                        }
                    }
                }
                let ang = self.e(&d.angle)?.to_radians();
                if ang <= 0.0 {
                    return Err("Der Winkel muss positiv sein.".into());
                }
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
                if path.len() < 2 {
                    return Err("Die Leitkurve ist zu kurz.".into());
                }
                let sf = sweep_section_frame(&path);
                let sec = self.prepare(&d.section)?;
                let regions = sec.profile_regions()?;
                let tool = match d.mode {
                    SweepMode::Normal => builders::sweep(&regions, &path, closed, &sf, &mut self.tags, fid),
                    SweepMode::Volume => {
                        // the tool keeps the start orientation and only moves along the trajectory
                        let rings: Vec<Box<dyn Fn(V2) -> V3>> = path
                            .iter()
                            .map(|p| {
                                let o = *p;
                                let (x, y) = (sf.x, sf.y);
                                Box::new(move |q: V2| o + x * q.x + y * q.y) as Box<dyn Fn(V2) -> V3>
                            })
                            .collect();
                        builders::loft(&regions, &rings, closed, &mut self.tags, fid)
                    }
                    SweepMode::Blend => {
                        if closed {
                            return Err("Ein Zug-Verbund braucht eine offene Leitkurve.".into());
                        }
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
                if d.sections.len() < 2 {
                    return Err("Ein Verbund benötigt mindestens zwei Querschnitte.".into());
                }
                let offs: Vec<f64> = d.offsets.iter().take(d.sections.len() - 1).map(|o| self.e(o)).collect::<Result<_, _>>()?;
                if offs.len() < d.sections.len() - 1 || offs.iter().any(|o| *o <= 0.0) {
                    return Err(if d.rotational { "Querschnittswinkel müssen positiv sein." } else { "Querschnittsabstände müssen positiv sein." }.into());
                }
                let mut secs = vec![];
                let mut axis = None;
                for (i, s) in d.sections.iter().enumerate() {
                    let s = self.prepare(s)?;
                    if i == 0 {
                        axis = s.centerline_axis();
                    }
                    let r = s.profile_regions().map_err(|_| format!("Querschnitt {} muss eine einzelne geschlossene Schleife sein.", i + 1))?;
                    secs.push(r[0].outer.pts.clone());
                }
                let tool = if d.rotational {
                    let (ap, au) = axis.ok_or("Im ersten Querschnitt eine Mittellinie als Drehachse skizzieren.")?;
                    if offs.iter().sum::<f64>() >= 360.0 {
                        return Err("Der Gesamtwinkel eines Rotationsverbunds muss kleiner als 360° sein.".into());
                    }
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
                let z = frame.z();
                // where the axis meets the placement plane
                let on_plane = |p: V3, u: V3| -> Result<V3, String> {
                    let k = u.dot(z);
                    if k.abs() < 1e-6 {
                        return Err("Die Achse liegt parallel zur Platzierungsebene.".into());
                    }
                    Ok(p + u * ((frame.origin - p).dot(z) / k))
                };
                let base = match d.placement {
                    HolePlacement::Linear => {
                        let reference_x = d.offset_x.map(|r| self.plane(&Some(r))).transpose()?;
                        let reference_y = d.offset_y.map(|r| self.plane(&Some(r))).transpose()?;
                        frame.to_world(hole_position(&frame, self.e(&d.x)?, self.e(&d.y)?, [reference_x, reference_y])?)
                    }
                    HolePlacement::Radial | HolePlacement::Diameter => {
                        let (ap, au) = self.axis(d.axis_ref.as_ref().ok_or("Eine Achse als Versatzreferenz wählen.")?)?;
                        let c = on_plane(ap, au)?;
                        let zero = match d.angle_ref {
                            Some(r) => {
                                let rf = self.plane(&Some(r))?;
                                let v = z.cross(rf.z());
                                if v.length() < 1e-6 {
                                    return Err("Die Winkelreferenz darf nicht parallel zur Platzierungsebene sein.".into());
                                }
                                safe_norm(v, frame.x)
                            }
                            None => frame.x,
                        };
                        let rad = self.e(&d.x)? * if d.placement == HolePlacement::Diameter { 0.5 } else { 1.0 };
                        let ang = self.e(&d.angle)?.to_radians();
                        c + (zero * ang.cos() + z.cross(zero) * ang.sin()) * rad
                    }
                    HolePlacement::Coaxial => {
                        let (ap, au) = self.axis(d.axis_ref.as_ref().ok_or("Eine Achse für die koaxiale Bohrung wählen.")?)?;
                        on_plane(ap, au)?
                    }
                    HolePlacement::OnPoint => {
                        let id = d.point_ref.ok_or("Einen Bezugspunkt für die Bohrung wählen.")?;
                        let p = self.res.points.iter().find(|q| q.id == id).map(|q| q.p).ok_or("Der Bezugspunkt fehlt.")?;
                        p - z * (p - frame.origin).dot(z)
                    }
                };
                let dir = if d.flip { z } else { -z };
                // Bohrungsorientierung: turns the hole's own x direction (profile seam, sketch)
                let mut xdir = frame.x;
                if let Some(o) = d.orient_ref {
                    let of = self.plane(&Some(o))?;
                    let target = if d.orient_normal { of.z() } else { of.z().cross(z) };
                    xdir = safe_norm(target - z * target.dot(z), frame.x);
                }
                // depth option -> plain depth
                let kind = d.depth_kind();
                let mut dd = d.clone();
                let mut base = base;
                match kind {
                    DepthType::Symmetric => {
                        let dep = self.e(&d.depth)?;
                        base = base - dir * (dep / 2.0);
                    }
                    DepthType::ToNext => {
                        let t = self.topo();
                        let hit = t.ray_first_hit(base + dir * 1e-3, dir, 1e-4).ok_or("Bis zur nächsten: in Bohrrichtung liegt keine Fläche.")?;
                        dd.depth = Dim::v(hit + 1e-3);
                        dd.depth_type = DepthType::Blind;
                    }
                    DepthType::ToSelected => {
                        let pf = self.plane(&d.depth_ref)?;
                        let k = pf.z().dot(dir);
                        if k.abs() < 1e-6 {
                            return Err("Die Tiefenreferenz liegt parallel zur Bohrrichtung.".into());
                        }
                        let t = (pf.origin - base).dot(pf.z()) / k;
                        if t <= 1e-6 {
                            return Err("Die Tiefenreferenz liegt nicht in Bohrrichtung.".into());
                        }
                        dd.depth = Dim::v(t);
                        dd.depth_type = DepthType::Blind;
                    }
                    _ => {}
                }
                let through = self.through(&frame);
                let depth_shown = if kind == DepthType::ThroughAll { through } else { self.e(&dd.depth)? };
                if d.lightweight {
                    // Geometrielos: axis plus the outline at both ends, no material removed
                    let r = hole_drill(d, &self.vars)? / 2.0;
                    let depth = if kind == DepthType::ThroughAll {
                        let t = self.topo();
                        let mut far = 0.0f64;
                        let mut o = base + dir * 1e-3;
                        while let Some(h) = t.ray_first_hit(o, dir, 1e-4) {
                            far += h + 1e-3;
                            o = o + dir * (h + 1e-3);
                            if far > through {
                                break;
                            }
                        }
                        far.max(r)
                    } else {
                        depth_shown
                    };
                    let w = xdir;
                    let v2 = dir.cross(w);
                    let ring = |off: f64| -> Vec<V3> {
                        (0..=64)
                            .map(|k| {
                                let a = std::f64::consts::TAU * k as f64 / 64.0;
                                base + dir * off + (w * a.cos() + v2 * a.sin()) * r
                            })
                            .collect()
                    };
                    self.res.curves.push(CurveInfo { feature: fid, polylines: vec![ring(0.0), ring(depth)] });
                    self.add_axis(fid, 1, base, dir);
                    return Ok(());
                }
                let tool = if d.sketched {
                    sketched_hole_tool(&self.prepare(&d.sketch)?, base, dir, xdir, &mut self.tags, fid)?
                } else {
                    let prof = hole_profile(&dd, &self.vars, through)?;
                    builders::hole_tool(&prof, base, dir, xdir, &mut self.tags, fid)
                };
                self.apply_tool(tool, true, fid)?;
                self.add_axis(fid, 1, base, dir);
                if d.ty == HoleType::Standard && d.tapped && !d.sketched {
                    let spec = super::threads::find(d.standard, &d.size).ok_or("Unbekannte Gewindegröße.")?;
                    let extent = self.polys.iter().flat_map(|q| q.v.iter()).map(|v| (v.p - base).dot(dir)).fold(0.0f64, f64::max);
                    let depth = if d.through_all { extent } else { self.e(&dd.depth)? };
                    let len = if d.through_all { extent } else { self.e(&d.thread_depth)?.min(depth) };
                    if len > 0.0 {
                        self.res.cosmetics.push(Cosmetic {
                            feature: fid,
                            base,
                            dir,
                            radius: spec.major / 2.0,
                            length: len,
                            label: super::threads::callout(d.standard, spec),
                            pitch: spec.pitch,
                            wall: spec.tap / 2.0,
                            internal: true,
                        });
                    }
                }
            }
            FeatureKind::Round(d) => {
                if d.edge_count() == 0 {
                    return Err("Kanten für die Rundung wählen.".into());
                }
                let t = self.topo();
                let tol = (t.bbox.diagonal() * 1e-5).max(1e-6);
                let mut cur = self.polys.clone();
                // every rounded chain with its (constant) radius, for the transitions
                let mut done: Vec<(EdgeChain, Option<f64>)> = vec![];
                let mut shown: Vec<CPolygon> = vec![];
                let mut surfaces: Vec<CPolygon> = vec![];
                for (si, set) in d.sets.iter().enumerate() {
                    let edges: Vec<&EdgeRef> = set.edges.iter().enumerate().filter(|(i, _)| !set.excluded.contains(i)).map(|(_, e)| e).collect();
                    if edges.is_empty() {
                        continue;
                    }
                    let mut picked: Vec<(EdgeChain, V3)> = vec![];
                    for e in &edges {
                        let ci = t.match_chain(e).ok_or(format!("Satz {}: eine Kantenreferenz wurde nicht gefunden.", si + 1))?;
                        picked.push((t.chains[ci].clone(), e.a));
                    }
                    if set.full {
                        if picked.len() != 2 {
                            return Err(format!("Satz {}: eine Vollrundung braucht genau zwei Kanten.", si + 1));
                        }
                        let r = full_round_radius(&picked[0].0, &picked[1].0).map_err(|e| format!("Satz {}: {}", si + 1, e))?;
                        for (c, _) in &picked {
                            if !c.convex {
                                return Err(format!("Satz {}: eine Vollrundung geht nur über konvexe Kanten.", si + 1));
                            }
                            if d.surface {
                                let q = builders::edge_surface(c, &builders::round_section(r, r, true), &mut self.tags, fid);
                                shown.extend(q.iter().cloned());
                                surfaces.extend(q);
                                continue;
                            }
                            let tool = builders::edge_tool_ends(c, &builders::round_section(r, r, true), t.tool_end_reach(c, r * 0.02), &mut self.tags, fid);
                            shown.extend(tool.iter().cloned());
                            cur = csg::subtract_local(&cur, &tool);
                            done.push((c.clone(), Some(r)));
                        }
                        continue;
                    }
                    let r0 = self.e(&set.radius)?;
                    let r1 = if set.variable { self.e(&set.radius_end)? } else { r0 };
                    if r0 <= 0.0 || r1 <= 0.0 {
                        return Err(format!("Satz {}: der Radius muss positiv sein.", si + 1));
                    }
                    let rho = self.e(&set.rho)?;
                    if set.section.has_rho() && !(0.05..=0.95).contains(&rho) {
                        return Err(format!("Satz {}: der konische Parameter muss zwischen 0.05 und 0.95 liegen.", si + 1));
                    }
                    let d2 = self.e(&set.d2)?;
                    if set.section.two_distances() && d2 <= 0.0 {
                        return Err(format!("Satz {}: D2 muss positiv sein.", si + 1));
                    }
                    let driving: Option<Vec<V3>> = match &set.curve {
                        Some(cr) => Some(t.chains[t.match_chain(cr).ok_or(format!("Satz {}: die Steuerkurve wurde nicht gefunden.", si + 1))?].points.clone()),
                        None => None,
                    };
                    // a constant radius rounds tangent chains as one edge
                    if !set.variable && driving.is_none() {
                        let anchor = picked[0].1;
                        picked = merge_tangent(picked.into_iter().map(|p| p.0).collect()).into_iter().map(|c| (c, anchor)).collect();
                    }
                    for (c, anchor) in &picked {
                        // keep the variable radius anchored to the end that was picked first
                        let rev = (c.points[0] - *anchor).length() > (c.points[c.points.len() - 1] - *anchor).length();
                        let (a, b) = if rev { (r1, r0) } else { (r0, r1) };
                        let curve_kind = |rho| match set.section {
                            RoundSection::Conic | RoundSection::D1xD2Conic => builders::SectionCurve::Conic(rho),
                            _ => builders::SectionCurve::C2(rho),
                        };
                        let section = if let Some(line) = driving.clone() {
                            // Durch Kurve: the round's edge on the surface runs along the curve
                            builders::round_section_by(Box::new(move |p, n1, n2, _| polyline_distance(p, &line) / builders::round_tangent_distance(1.0, n1, n2).max(1e-6)), c.convex)
                        } else if set.chord {
                            builders::round_section_chord(a, b, c.convex, set.shape)
                        } else if set.section.two_distances() {
                            builders::round_section_curve(Box::new(move |_, _, _, frac| (a + (b - a) * frac, d2)), c.convex, curve_kind(rho))
                        } else if set.section.has_rho() {
                            builders::round_section_curve(
                                Box::new(move |_, n1, n2, frac| {
                                    let td = builders::round_tangent_distance(a + (b - a) * frac, n1, n2);
                                    (td, td)
                                }),
                                c.convex,
                                curve_kind(rho),
                            )
                        } else if set.shape == crate::kernel::corner::CornerShape::Circular && set.variable && set.method == RoundMethod::RollingBall && (a - b).abs() > 1e-9 {
                            builders::round_section_rolling(a, b, c.convex, c.points.clone())
                        } else if set.shape == crate::kernel::corner::CornerShape::Circular {
                            builders::round_section(a, b, c.convex)
                        } else {
                            builders::round_section_shape(a, b, c.convex, set.shape)
                        };
                        // the round must stay on its two surfaces; past a surface's border it only
                        // goes on over the surface's extension (Flächen verlängern)
                        if !set.extend_surfaces && driving.is_none() {
                            for w in c.points.windows(2).chain(if c.closed { vec![[*c.points.last().unwrap(), c.points[0]]] } else { vec![] }.iter().map(|x| &x[..])) {
                                let k = c.points.iter().position(|q| *q == w[0]).unwrap_or(0);
                                let (na, nb) = (c.n_a[k], c.n_b[k]);
                                let m = (w[0] + w[1]) * 0.5;
                                let frac = {
                                    let total: f64 = c.points.windows(2).map(|x| (x[1] - x[0]).length()).sum::<f64>().max(1e-9);
                                    let upto: f64 = c.points.windows(2).take(k).map(|x| (x[1] - x[0]).length()).sum::<f64>();
                                    (upto + (w[1] - w[0]).length() * 0.5) / total
                                };
                                let rr = a + (b - a) * frac;
                                let (da, db) = if set.chord {
                                    let half = 0.5 * na.dot(nb).clamp(-0.999, 0.999).acos();
                                    let td = rr / (2.0 * half.sin()).max(1e-6) * half.tan();
                                    (td, td)
                                } else if set.section.two_distances() {
                                    (rr, d2)
                                } else {
                                    let td = builders::round_tangent_distance(rr, na, nb);
                                    (td, td)
                                };
                                let (fa, fb) = (c.f_a[k], c.f_b[k]);
                                let eps = (da.max(db) * 1e-3).max(1e-4);
                                let on_face = |dir: V3, n: V3, dist: f64| t.contains(m + dir * (dist * 0.995) - n * eps);
                                if !on_face(fa, na, da) || !on_face(fb, nb, db) {
                                    return Err(format!(
                                        "Satz {}: die Rundung läuft über den Rand einer angrenzenden Fläche hinaus. „Flächen verlängern“ aktivieren oder den Radius verkleinern.",
                                        si + 1
                                    ));
                                }
                            }
                        }
                        let reach = if set.extend_surfaces && !c.closed { [a.max(b), a.max(b)] } else { t.tool_end_reach(c, a.max(b) * 0.02) };
                        if d.surface {
                            let q = builders::edge_surface(c, &section, &mut self.tags, fid);
                            shown.extend(q.iter().cloned());
                            surfaces.extend(q);
                            done.push((c.clone(), None));
                            continue;
                        }
                        let tool = builders::edge_tool_ends(c, &section, reach, &mut self.tags, fid);
                        shown.extend(tool.iter().cloned());
                        cur = if c.convex { csg::subtract_local(&cur, &tool) } else { csg::union_local(&cur, &tool) };
                        let constant = !set.variable && driving.is_none() && !set.chord && set.section == RoundSection::Circular && set.shape == crate::kernel::corner::CornerShape::Circular;
                        done.push((c.clone(), if constant { Some(a) } else { None }));
                    }
                }
                // Übergänge: spherical corners where three rounds of one radius meet
                let chains: Vec<EdgeChain> = done.iter().map(|x| x.0.clone()).collect();
                for (p, list) in edge_corners(&chains, tol) {
                    if d.surface || transition_at(&d.transitions, p, tol) != TransitionKind::Sphere {
                        continue;
                    }
                    let radii: Vec<Option<f64>> = list.iter().map(|&i| done[i].1).collect();
                    let r = match radii[0] {
                        Some(r) if list.len() == 3 && radii.iter().all(|x| x.map_or(false, |x| (x - r).abs() < 1e-6)) && list.iter().all(|&i| done[i].0.convex) => r,
                        _ => return Err("Kugelecke: drei konvexe Kanten mit gleichem, konstantem Kreisradius müssen sich in der Ecke treffen.".into()),
                    };
                    let mut normals: Vec<V3> = vec![];
                    for &i in &list {
                        let c = &done[i].0;
                        let k = if (c.points[0] - p).length() < tol { 0 } else { c.points.len() - 1 };
                        for n in [c.n_a[k], c.n_b[k]] {
                            if !normals.iter().any(|m| m.dot(n) > 1.0 - 1e-4) {
                                normals.push(n);
                            }
                        }
                    }
                    if normals.len() != 3 {
                        return Err("Kugelecke: in der Ecke müssen genau drei Flächen zusammenkommen.".into());
                    }
                    let tool = builders::sphere_corner_tool(p, [normals[0], normals[1], normals[2]], r, &mut self.tags, fid).ok_or("Kugelecke: die Flächen der Ecke sind nicht unabhängig.")?;
                    cur = csg::subtract_local(&cur, &tool);
                }
                self.res.tools.insert(fid, ToolBody { polys: shown, op: Op::Cut });
                if d.surface {
                    self.quilts.push((fid, surfaces));
                    return Ok(());
                }
                self.set_polys(cur);
            }
            FeatureKind::Chamfer(d) => {
                if d.edge_count() == 0 {
                    return Err("Kanten für die Fase wählen.".into());
                }
                let t = self.topo();
                let tol = (t.bbox.diagonal() * 1e-5).max(1e-6);
                if d.corner {
                    let d1 = self.e(&d.d1)?;
                    if d1 <= 0.0 {
                        return Err("Der Fasenabstand muss positiv sein.".into());
                    }
                    let tag = self.tags.next(fid);
                    let tool = corner_tool(&t, &d.edges, d1, tag)?;
                    return self.apply_tool(tool, true, fid);
                }
                let mut cur = self.polys.clone();
                let mut done: Vec<(EdgeRef, f64)> = vec![];
                let mut shown: Vec<CPolygon> = vec![];
                let mut surfaces: Vec<CPolygon> = vec![];
                for (si, set) in d.sets().enumerate() {
                    let edges: Vec<&EdgeRef> = set.edges.iter().enumerate().filter(|(i, _)| !set.excluded.contains(i)).map(|(_, e)| e).collect();
                    if edges.is_empty() {
                        continue;
                    }
                    let v1 = if set.by_reference {
                        // Referenz: D runs from the edge to the picked datum point
                        let id = set.d_ref.ok_or(format!("Satz {}: einen Bezugspunkt als Referenz für D wählen.", si + 1))?;
                        let q = self.res.points.iter().find(|x| x.id == id).map(|x| x.p).ok_or(format!("Satz {}: der Referenzpunkt fehlt.", si + 1))?;
                        let ci = t.match_chain(edges[0]).ok_or(format!("Satz {}: eine Kantenreferenz wurde nicht gefunden.", si + 1))?;
                        polyline_distance(q, &t.chains[ci].points)
                    } else {
                        self.e(&set.d1)?
                    };
                    let v2 = match set.kind {
                        ChamferKind::D1xD2 | ChamferKind::O1xO2 if !set.by_reference => self.e(&set.d2)?,
                        ChamferKind::AnglexD => {
                            let angle = self.e(&set.angle)?;
                            if !(0.0..90.0).contains(&angle) || angle == 0.0 {
                                return Err(format!("Satz {}: der Fasenwinkel muss zwischen 0° und 90° liegen.", si + 1));
                            }
                            v1 * angle.to_radians().tan()
                        }
                        _ => v1,
                    };
                    if v1 <= 0.0 || v2 <= 0.0 {
                        return Err(format!("Satz {}: Fasenabstände müssen positiv sein.", si + 1));
                    }
                    let mut picked = vec![];
                    for e in &edges {
                        let ci = t.match_chain(e).ok_or(format!("Satz {}: eine Kantenreferenz wurde nicht gefunden.", si + 1))?;
                        picked.push(t.chains[ci].clone());
                        done.push(((*e).clone(), v1.max(v2)));
                    }
                    for c in &merge_tangent(picked) {
                        if set.kind == ChamferKind::Deg45xD && c.n_a.iter().zip(&c.n_b).any(|(a, b)| a.dot(*b).abs() > 0.02) {
                            return Err(format!("Satz {}: 45 x D geht nur zwischen senkrecht aufeinander stehenden Flächen.", si + 1));
                        }
                        let section = match set.kind {
                            ChamferKind::OxO | ChamferKind::O1xO2 => {
                                let (o1, o2) = (v1, v2);
                                // the point at depth o1 under surface 1 and o2 under surface 2,
                                // projected back onto both surfaces
                                builders::chamfer_section_by(
                                    Box::new(move |n1, n2, f1, f2| {
                                        let b = o1 / n1.dot(f2).abs().max(1e-6);
                                        let a = o2 / n2.dot(f1).abs().max(1e-6);
                                        let k = f1.dot(f2);
                                        (a + b * k, a * k + b)
                                    }),
                                    c.convex,
                                )
                            }
                            ChamferKind::AnglexD => builders::chamfer_section(v1, v2, c.convex),
                            // Versatzflächen: each chamfer edge is where the other surface,
                            // offset by its distance, meets this one; Tangentialer Abstand:
                            // the distances run along the surfaces' tangents
                            _ if set.method == ChamferMethod::OffsetSurfaces => {
                                let (o1, o2) = (v1, v2);
                                builders::chamfer_section_by(Box::new(move |n1, n2, f1, f2| (o2 / f1.dot(n2).abs().max(1e-6), o1 / f2.dot(n1).abs().max(1e-6))), c.convex)
                            }
                            _ => builders::chamfer_section(v1, v2, c.convex),
                        };
                        if d.surface {
                            let q = builders::edge_surface(c, &section, &mut self.tags, fid);
                            shown.extend(q.iter().cloned());
                            surfaces.extend(q);
                            continue;
                        }
                        let tool = builders::edge_tool_ends(c, &section, t.tool_end_reach(c, v1.max(v2) * 0.02), &mut self.tags, fid);
                        shown.extend(tool.iter().cloned());
                        cur = if c.convex { csg::subtract_local(&cur, &tool) } else { csg::union_local(&cur, &tool) };
                    }
                }
                // Übergänge: Eckfase cuts the tip where three chamfers meet
                let all: Vec<EdgeChain> = done.iter().filter_map(|(e, _)| t.match_chain(e).map(|ci| t.chains[ci].clone())).collect();
                for (p, list) in edge_corners(&all, tol) {
                    if d.surface || transition_at(&d.transitions, p, tol) != TransitionKind::CornerChamfer {
                        continue;
                    }
                    let refs: Vec<EdgeRef> = list.iter().map(|&i| done[i].0.clone()).collect();
                    let size = list.iter().map(|&i| done[i].1).fold(0.0, f64::max);
                    let tag = self.tags.next(fid);
                    let tool = corner_tool(&t, &refs, 2.0 * size, tag)?;
                    cur = csg::subtract_local(&cur, &tool);
                }
                self.res.tools.insert(fid, ToolBody { polys: shown, op: Op::Cut });
                if d.surface {
                    self.quilts.push((fid, surfaces));
                    return Ok(());
                }
                self.set_polys(cur);
            }
            FeatureKind::Shell(d) => {
                if self.polys.is_empty() {
                    return Err("Nichts zu schalen.".into());
                }
                let th = self.e(&d.thickness)?;
                if th <= 0.0 {
                    return Err("Die Dicke muss positiv sein.".into());
                }
                let t = self.topo();
                let mut removed = HashSet::new();
                for fr in &d.faces {
                    removed.insert(t.match_face(fr.point, fr.normal).ok_or("Eine entfernte Fläche wurde nicht gefunden.")?);
                }
                let inner = builders::move_planes(&t, &|f, n, w| if removed.contains(&f) { (n, w + th * 1.5) } else { (n, w - th) });
                let new = csg::subtract(&self.polys, &inner);
                self.res.tools.insert(fid, ToolBody { polys: inner, op: Op::Cut });
                self.set_polys(new);
            }
            FeatureKind::Draft(d) => {
                if d.faces.is_empty() {
                    return Err("Flächen für die Formschräge wählen.".into());
                }
                let hinge = self.plane(&d.hinge)?;
                let ang = self.e(&d.angle)?.to_radians();
                let t = self.topo();
                let pull = if d.flip { -hinge.z() } else { hinge.z() };
                let mut drafted: HashMap<usize, (V3, f64)> = HashMap::new();
                // Variable Zugschräge: the angle runs from `angle` to `angle2` across the faces along the hinge x direction
                let ang2 = match &d.angle2 {
                    Some(a) => Some(self.e(a)?.to_radians()),
                    None => None,
                };
                let xs: Vec<f64> = d.faces.iter().map(|f| f.point.dot(hinge.x)).collect();
                let (x0, x1) = (xs.iter().cloned().fold(f64::INFINITY, f64::min), xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                for (k, fr) in d.faces.iter().enumerate() {
                    let ang = match ang2 {
                        Some(a2) if x1 > x0 + 1e-9 => ang + (a2 - ang) * (xs[k] - x0) / (x1 - x0),
                        Some(a2) => (ang + a2) / 2.0,
                        None => ang,
                    };
                    let fi = t.match_face(fr.point, fr.normal).ok_or("Eine Formschrägenfläche wurde nicht gefunden.")?;
                    let (n, w) = (t.faces[fi].normal, t.faces[fi].offset);
                    let dd = n.cross(hinge.z());
                    if dd.length() < 1e-6 {
                        return Err("Formschrägenflächen dürfen nicht parallel zur Scharnierebene sein.".into());
                    }
                    let hw = hinge.z().dot(hinge.origin);
                    let q = (w * hinge.z().cross(dd) + hw * dd.cross(n)) / dd.dot(dd);
                    let np = safe_norm(n * ang.cos() + pull * ang.sin(), n);
                    drafted.insert(fi, (np, np.dot(q)));
                }
                let new = builders::move_planes(&t, &|f, n, w| *drafted.get(&f).unwrap_or(&(n, w)));
                self.set_polys(new);
            }
            FeatureKind::Pattern(d) => {
                if d.features.is_empty() {
                    return Err("Ein zu musterndes KE wählen.".into());
                }
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
                        for i in 0..n1 {
                            for j in 0..n2 {
                                if i != 0 || j != 0 {
                                    xf.push(Xform::translation(u1 * (s1 * i as f64) + u2 * (s2 * j as f64)));
                                }
                            }
                        }
                    }
                    PatternKind::Axis => {
                        let (a, u) = self.axis(&d.axis)?;
                        let n = self.e(&d.axis_count)?.round().max(1.0) as usize;
                        let step = self.e(&d.axis_angle)?.to_radians();
                        for k in 1..n {
                            xf.push(Xform::rotation(a, safe_norm(u, V3::Y), step * k as f64));
                        }
                    }
                    PatternKind::Table => {
                        if d.table.is_empty() {
                            return Err("Die Mustertabelle ist leer.".into());
                        }
                        for r in &d.table {
                            xf.push(Xform::translation(V3::new(r[0], r[1], r[2])));
                        }
                    }
                    PatternKind::Fill | PatternKind::Curve => {
                        let fr = self.plane(&d.sketch_plane).map_err(|_| "Zuerst den Füllbereich / die Kurve auf einer Ebene skizzieren.".to_string())?;
                        let sk = self.prepare(&d.sketch)?;
                        let leader =
                            d.features.first().and_then(|s| self.res.tools.get(s)).map(|t| csg::bbox(&t.polys).center()).ok_or("Das gemusterte KE hat keine Geometrie.")?;
                        let l = fr.to_local(leader);
                        if d.kind == PatternKind::Fill {
                            let regions = sk.profile_regions()?;
                            let s = self.e(&d.fill_spacing)?;
                            if s <= 0.0 {
                                return Err("Der Füllabstand muss positiv sein.".into());
                            }
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
                            for r in &regions {
                                for p in &r.outer.pts {
                                    lo = lo.min(*p);
                                    hi = hi.max(*p);
                                }
                            }
                            let reach = ((hi - l).length().max((lo - l).length()).max((V2::new(lo.x, hi.y) - l).length()).max((V2::new(hi.x, lo.y) - l).length()) / (s * 0.5))
                                .ceil() as i64
                                + 1;
                            let reach = reach.min(80);
                            let edge_dist = |g: V2| -> f64 {
                                let mut best = f64::INFINITY;
                                for r in &regions {
                                    for lp in std::iter::once(&r.outer).chain(r.holes.iter()) {
                                        let n = lp.pts.len();
                                        for k in 0..n {
                                            best = best.min(dist_point_segment2(g, lp.pts[k], lp.pts[(k + 1) % n]));
                                        }
                                    }
                                }
                                best
                            };
                            'grid: for i in -reach..=reach {
                                for j in -reach..=reach {
                                    if i == 0 && j == 0 {
                                        continue;
                                    }
                                    let g = l + bu * i as f64 + bv * j as f64;
                                    let inside = regions.iter().any(|r| point_in_polygon2(g, &r.outer.pts) && !r.holes.iter().any(|h| point_in_polygon2(g, &h.pts)));
                                    if inside && (margin <= 0.0 || edge_dist(g) >= margin) {
                                        xf.push(Xform::translation(fr.dir_to_world(g - l)));
                                        if xf.len() > 400 {
                                            break 'grid;
                                        }
                                    }
                                }
                            }
                        } else {
                            let (mut pts, closed) = sk.trajectory()?;
                            if closed {
                                pts.push(pts[0]);
                            }
                            let mut acc = vec![0.0];
                            for k in 1..pts.len() {
                                let x = acc[k - 1] + (pts[k] - pts[k - 1]).length();
                                acc.push(x);
                            }
                            let total = *acc.last().unwrap();
                            if total <= 1e-9 {
                                return Err("Die Musterkurve ist zu kurz.".into());
                            }
                            let eval = |s: f64| -> (V2, V2) {
                                let k = acc.iter().position(|a| *a >= s).unwrap_or(acc.len() - 1).max(1);
                                let seg = acc[k] - acc[k - 1];
                                let t = if seg > 0.0 { (s - acc[k - 1]) / seg } else { 0.0 };
                                (pts[k - 1] + (pts[k] - pts[k - 1]) * t, safe_norm2(pts[k] - pts[k - 1], V2::X))
                            };
                            let params: Vec<f64> = if d.curve_by_spacing {
                                let sp = self.e(&d.curve_spacing)?;
                                if sp <= 0.0 {
                                    return Err("Der Kurvenabstand muss positiv sein.".into());
                                }
                                (1..).map(|k| k as f64 * sp).take_while(|s| *s <= total + 1e-9).take(400).collect()
                            } else {
                                let n = self.e(&d.curve_count)?.round().max(2.0) as usize;
                                if closed {
                                    (1..n).map(|k| total * k as f64 / n as f64).collect()
                                } else {
                                    (1..n).map(|k| total * k as f64 / (n - 1) as f64).collect()
                                }
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
                if xf.len() > 400 {
                    return Err("Das Muster hat zu viele Elemente (max. 400).".into());
                }
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
                    for c in src_cos {
                        for x in &xf {
                            self.res.cosmetics.push(Cosmetic { feature: fid, base: x.point(c.base), dir: x.vector(c.dir), ..c.clone() });
                        }
                    }
                    // copies carry the axes of holes and revolves (extruded arcs keep a single axis, as in Creo)
                    let carries = self.data.feature(*src).map(|f| matches!(f.kind, FeatureKind::Hole(_) | FeatureKind::Revolve(_))).unwrap_or(false);
                    let src_axes: Vec<AxisInfo> = if carries { self.axes.values().filter(|a| a.feature == *src).cloned().collect() } else { vec![] };
                    for a in src_axes {
                        for x in &xf {
                            k += 1;
                            self.add_axis(fid, k, x.point(a.point), x.vector(a.dir));
                        }
                    }
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
                        let vx = |i: u32| {
                            face.v
                                .get(i as usize)
                                .map(|q| crate::kernel::csg::CVertex { p: V3::new(q[0] as f64, q[1] as f64, q[2] as f64), n: V3::new(q[3] as f64, q[4] as f64, q[5] as f64) })
                        };
                        let (Some(a), Some(b), Some(c)) = (vx(t[0]), vx(t[1]), vx(t[2])) else { continue };
                        if let Some(p) = CPolygon::new(vec![a, b, c], tag) {
                            polys.push(p);
                        }
                    }
                }
                if polys.is_empty() {
                    return Err("Die importierte Datei enthält keine Geometrie.".into());
                }
                self.apply_tool(polys, false, fid)?;
            }
            FeatureKind::DatumCsys(d) => {
                let o = V3::new(self.e(&d.x)?, self.e(&d.y)?, self.e(&d.z)?);
                let frame = csys_frame(o, self.e(&d.rx)?, self.e(&d.ry)?, self.e(&d.rz)?);
                self.res.csys.push(CsysInfo { id: fid, name: f.name.clone(), frame });
            }
            FeatureKind::Surface(d) => {
                let d = d.clone();
                self.apply_surface(&d, fid)?;
            }
            FeatureKind::Body(d) => {
                let d = d.clone();
                self.apply_body(&d, fid)?;
            }
            FeatureKind::Curve(d) => {
                if d.points.len() < 2 {
                    return Err("Mindestens zwei Bezugspunkte wählen.".into());
                }
                let pts: Vec<V3> =
                    d.points.iter().map(|id| self.res.points.iter().find(|p| p.id == *id).map(|p| p.p).ok_or("Ein Bezugspunkt der Kurve fehlt.")).collect::<Result<_, _>>()?;
                let line = if d.spline && pts.len() > 2 { catmull_rom3(&pts, 24) } else { pts };
                self.res.curves.push(CurveInfo { feature: fid, polylines: vec![line] });
            }
            FeatureKind::Rib(d) => {
                let frame = self.plane(&d.plane)?;
                let s = self.prepare(&d.sketch)?;
                if self.polys.is_empty() {
                    return Err("Eine Rippe benötigt ein vorhandenes Volumen.".into());
                }
                let th = self.e(&d.thickness)?;
                if th <= 0.0 {
                    return Err("Die Dicke muss positiv sein.".into());
                }
                if d.trajectory {
                    // Rippenleitkurve: the sketched path, seen from above, is given the rib's
                    // thickness and grows from the sketch plane down to the model
                    let regions = thin_regions(&s, th)?;
                    let t = self.topo();
                    let sg = if d.flip { 1.0 } else { -1.0 };
                    let dir = frame.z() * sg;
                    let mut depth: Option<f64> = None;
                    for r in &regions {
                        for p in r.outer.pts.iter().step_by((r.outer.pts.len() / 64).max(1)) {
                            if let Some(h) = t.ray_first_hit(frame.to_world(*p), dir, 1e-4) {
                                depth = Some(depth.map_or(h, |b: f64| b.max(h)));
                            }
                        }
                    }
                    let depth =
                        depth.ok_or("Die Rippenleitkurve liegt nicht über dem Modell: die Skizze auf eine Ebene oberhalb der Geometrie legen (oder die Seite umkehren).")?;
                    let (z0, z1) = if sg > 0.0 { (0.0, depth) } else { (-depth, 0.0) };
                    let tool = builders::extrude(&regions, &frame, z0, z1, 0.0, &mut self.tags, fid);
                    return self.apply_tool(tool, false, fid);
                }
                let (pts, closed) = s.trajectory()?;
                if closed {
                    return Err("Die Rippenskizze muss eine offene Kette sein.".into());
                }
                let t = self.topo();
                let region = rib_region(&t, &frame, &pts, d.flip)?;
                let (z0, z1) = match d.side {
                    1 => (0.0, th),
                    2 => (-th, 0.0),
                    _ => (-th / 2.0, th / 2.0),
                };
                let tool = builders::extrude(&[region], &frame, z0, z1, 0.0, &mut self.tags, fid);
                self.apply_tool(tool, false, fid)?;
            }
            FeatureKind::Helix(d) => {
                let frame = self.plane(&d.plane)?;
                let prof = self.prepare(&d.profile)?;
                let (ap, au) = prof.centerline_axis().ok_or("Im Profil eine Mittellinie als Spiralachse skizzieren.")?;
                let (pp, closed) = prof.trajectory()?;
                if closed {
                    return Err("Das Spiralzugprofil muss eine offene Kette sein.".into());
                }
                let mut samples: Vec<(f64, f64)> = pp.iter().map(|p| ((*p - ap).dot(au), cross2(au, *p - ap))).collect();
                if samples[0].0 > samples[samples.len() - 1].0 {
                    samples.reverse();
                }
                if samples.windows(2).any(|w| w[1].0 < w[0].0 - 1e-9) {
                    return Err("Das Profil muss entlang der Achse stetig fortschreiten.".into());
                }
                let side = samples.iter().map(|q| q.1).find(|r| r.abs() > 1e-9).map(f64::signum).ok_or("Das Profil darf nicht auf der Achse liegen.")?;
                if samples.iter().any(|q| q.1 * side < -1e-9) {
                    return Err("Das Profil muss auf einer Seite der Achse liegen.".into());
                }
                let (s0, len) = (samples[0].0, samples[samples.len() - 1].0 - samples[0].0);
                if len <= 1e-6 {
                    return Err("Das Profil muss eine Länge entlang der Achse haben.".into());
                }
                let pitch = self.e(&d.pitch)?;
                if pitch <= 0.0 {
                    return Err("Die Steigung muss positiv sein.".into());
                }
                let sec = self.prepare(&d.section)?;
                let regions = sec.profile_regions()?;
                let (mut amin, mut amax) = (f64::INFINITY, f64::NEG_INFINITY);
                for r in &regions {
                    for p in &r.outer.pts {
                        let a = (*p - ap).dot(au);
                        amin = amin.min(a);
                        amax = amax.max(a);
                    }
                }
                if amax - amin >= pitch {
                    return Err("Der Querschnitt ist höher als die Steigung — die Windungen würden sich überschneiden.".into());
                }
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
                let (start, dir, wall, full, internal) = thread_cylinder(&t, fi, d.flip).ok_or("Die gewählte Fläche ist nicht zylindrisch.")?;
                let (dia, pitch) = (self.e(&d.diameter)?, self.e(&d.pitch)?);
                if dia <= 0.0 || pitch <= 0.0 {
                    return Err("Durchmesser und Steigung müssen positiv sein.".into());
                }
                let len = self.e(&d.length)?;
                if len <= 0.0 {
                    return Err("Die Gewindelänge muss positiv sein.".into());
                }
                self.res.cosmetics.push(Cosmetic { feature: fid, base: start, dir, radius: dia / 2.0, length: len.min(full), label: thread_label(dia, pitch), pitch, wall, internal });
            }
            FeatureKind::Mirror(d) => {
                let frame = self.plane(&d.plane)?;
                let x = Xform::mirror(&frame);
                if d.features.is_empty() {
                    if self.polys.is_empty() {
                        return Err("Nichts zu spiegeln.".into());
                    }
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
                        for c in src_cos {
                            self.res.cosmetics.push(Cosmetic { feature: fid, base: x.point(c.base), dir: x.vector(c.dir), ..c });
                        }
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
    if ents.is_empty() {
        return Err("Die Skizze ist leer.".into());
    }
    let mut out = vec![];
    for e in ents {
        let pts = s.sample(e, 64);
        if pts.len() < 2 {
            continue;
        }
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
            let d = if i == 0 {
                pts[1] - pts[0]
            } else if i == n - 1 {
                pts[i] - pts[i - 1]
            } else {
                safe_norm2(pts[i + 1] - pts[i], V2::X) + safe_norm2(pts[i] - pts[i - 1], V2::X)
            };
            let nr = safe_norm2(perp_left(d), V2::Y);
            left.push(pts[i] + nr * t / 2.0);
            right.push(pts[i] - nr * t / 2.0);
        }
        left.reverse();
        let mut ring = right;
        ring.extend(left);
        if polygon_area2(&ring) < 0.0 {
            ring.reverse();
        }
        out.push(ProfileRegion { outer: builders::simple_loop(ring), holes: vec![] });
    }
    Ok(out)
}

/// Closed rib section: the sketched chain, then back along the points where the chain,
/// pushed towards the material side, meets the solid (slightly inside it so the rib fuses).
fn rib_region(t: &Topology, frame: &Frame, chain: &[V2], flip: bool) -> Result<crate::sketch::sketch::ProfileRegion, String> {
    let total: f64 = chain.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    if total <= 1e-9 {
        return Err("Die Rippenskizze ist zu kurz.".into());
    }
    let step = total / 160.0;
    let mut pts = vec![chain[0]];
    for w in chain.windows(2) {
        let n = ((w[1] - w[0]).length() / step).ceil().max(1.0) as usize;
        for k in 1..=n {
            pts.push(w[0] + (w[1] - w[0]) * (k as f64 / n as f64));
        }
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
        (Some(a), Some(b)) => {
            if (a.1 <= b.1) != flip {
                a
            } else {
                b
            }
        }
        (Some(a), None) => {
            if flip {
                return Err("Auf der anderen Seite der Skizze liegt kein Volumen.".into());
            } else {
                a
            }
        }
        (None, Some(b)) => {
            if flip {
                return Err("Auf der anderen Seite der Skizze liegt kein Volumen.".into());
            } else {
                b
            }
        }
        (None, None) => return Err("Die Rippe kann nicht an das Volumen angeschlossen werden — die Enden der Skizze müssen das Volumen berühren.".into()),
    };
    let mut ring = pts.clone();
    ring.extend(pick.0.into_iter().rev());
    if polygon_area2(&ring) < 0.0 {
        ring.reverse();
    }
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
        let area: f64 = (0..l.len())
            .map(|i| {
                let (p, q) = (l[i], l[(i + 1) % l.len()]);
                p.x * q.y - q.x * p.y
            })
            .sum();
        let mut pts: Vec<V2> = if area < 0.0 { l.iter().rev().copied().collect() } else { l.to_vec() };
        let ang = |p: V2| {
            let v = p - c;
            v.y.atan2(v.x).abs()
        };
        let k = (0..pts.len()).min_by(|i, j| ang(pts[*i]).partial_cmp(&ang(pts[*j])).unwrap()).unwrap_or(0);
        pts.rotate_left(k);
        pts.push(pts[0]);
        let p3: Vec<V3> = pts.iter().map(|p| V3::new(p.x, p.y, 0.0)).collect();
        let mut r: Vec<V2> = sf::resample(&p3, N + 1).iter().map(|p| V2::new(p.x, p.y)).collect();
        r.pop();
        r
    };
    if a.len() < 3 || b.len() < 3 {
        return Err("Beide Querschnitte müssen geschlossen sein.".into());
    }
    let (sa, sb) = (prep(a), prep(b));
    let n = rings.len();
    let stations: Vec<Vec<V3>> = (0..n)
        .map(|k| {
            let t = k as f64 / (n - 1).max(1) as f64;
            (0..N).map(|i| rings[k](sa[i] * (1.0 - t) + sb[i] * t)).collect()
        })
        .collect();
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
            if let Some(mut p) = sf::poly(&[st[t[0]], st[t[1]], st[t[2]]], tag) {
                if p.plane.n.dot(facing) < 0.0 {
                    p.flip();
                }
                out.push(p);
            }
        }
    }
    let side_out = {
        let p = &out[0];
        let c = stations[0].iter().chain(stations[1].iter()).fold(V3::ZERO, |s, q| s + *q) / (2 * N) as f64;
        p.plane.n.dot(p.centroid() - c) >= 0.0
    };
    // flip the sides if they point inward
    let caps = 2 * (N - 2);
    let k = out.len() - caps.min(out.len());
    if !side_out {
        for p in out.iter_mut().take(k) {
            p.flip();
        }
    }
    Ok(out)
}

/// Eckfase: a tetrahedron cutting the corner shared by the given edges, `d` along each edge.
fn corner_tool(t: &Topology, edges: &[EdgeRef], d: f64, tag: u32) -> Result<Vec<CPolygon>, String> {
    use crate::model::surfaces as sf;
    if edges.len() < 2 {
        return Err("Zwei Kanten wählen, die sich in der Ecke treffen.".into());
    }
    let chains: Vec<usize> = edges.iter().map(|e| t.match_chain(e).ok_or("Eine Kantenreferenz wurde nicht gefunden.".to_string())).collect::<Result<_, _>>()?;
    let ends = |ci: usize| {
        let c = &t.chains[ci];
        [c.points[0], *c.points.last().unwrap()]
    };
    let tol = (t.bbox.diagonal() * 1e-5).max(1e-6);
    let corner = ends(chains[0])
        .into_iter()
        .find(|v| chains[1..].iter().all(|c| ends(*c).iter().any(|w| (*w - *v).length() < tol)))
        .ok_or("Die gewählten Kanten treffen sich nicht in einer Ecke.")?;
    // every edge at the corner (also ones not picked), and the point d along each
    let mut pts = vec![];
    for (ci, c) in t.chains.iter().enumerate() {
        if c.closed {
            continue;
        }
        let [s, e] = ends(ci);
        let walk: Vec<V3> = if (s - corner).length() < tol {
            c.points.clone()
        } else if (e - corner).length() < tol {
            c.points.iter().rev().copied().collect()
        } else {
            continue;
        };
        let mut left = d;
        let mut p = walk[0];
        for w in walk.windows(2) {
            let l = (w[1] - w[0]).length();
            if l >= left {
                p = w[0] + (w[1] - w[0]) * (left / l);
                left = 0.0;
                break;
            }
            left -= l;
            p = w[1];
        }
        if left > 1e-9 {
            return Err("Der Fasenabstand ist länger als eine Kante an der Ecke.".into());
        }
        pts.push(p);
    }
    if pts.len() < 3 {
        return Err("An der Ecke treffen keine drei Kanten zusammen.".into());
    }
    let m = (pts[0] + pts[1] + pts[2]) / 3.0;
    let apex = corner + (corner - m) * 0.25;
    let faces = [[apex, pts[0], pts[1]], [apex, pts[1], pts[2]], [apex, pts[2], pts[0]], [pts[0], pts[1], pts[2]]];
    let c = (apex + pts[0] + pts[1] + pts[2]) / 4.0;
    let tool: Vec<CPolygon> = faces
        .iter()
        .filter_map(|f| {
            let mut p = sf::poly(f, tag)?;
            if p.plane.n.dot(p.centroid() - c) < 0.0 {
                p.flip();
            }
            Some(p)
        })
        .collect();
    Ok(tool)
}
