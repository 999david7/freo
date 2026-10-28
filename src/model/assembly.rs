//! Assemblies: parts placed as components by Creo's placement constraints (Coincident,
//! Distance, Parallel, Normal, Angle Offset, Default, Fix). Components are solved one at a
//! time, in model-tree order, by Levenberg–Marquardt over their rigid transform, then merged
//! into one display model in which every component is a separate body.

use super::features::*;
use super::regen::{self, AxisInfo, Cosmetic, CurveInfo, PlaneInfo, PointInfo, RegenResult};
use crate::kernel::csg::{self, CPolygon};
use crate::kernel::math::*;
use crate::kernel::topology::{Topology, BODY_SHIFT};
use glam::{DMat3, DQuat};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Datum ids of a component's part are shown in the assembly as `comp << 32 | id`.
pub const COMP_SHIFT: u64 = 32;
pub const FIRST_COMPONENT_ID: u64 = 100;

pub fn comp_datum_id(comp: u64, local: u64) -> u64 { (comp << COMP_SHIFT) | local }

/// Splits an assembly-level datum id into (component, part-local id).
pub fn split_datum_id(id: u64) -> (Option<u64>, u64) {
    let c = id >> COMP_SHIFT;
    (if c == 0 { None } else { Some(c) }, id & 0xFFFF_FFFF)
}

/// Default component colours, used when a part keeps the default appearance.
pub const PALETTE: [[f32; 3]; 8] = [
    [0.74, 0.76, 0.79],
    [0.56, 0.68, 0.86],
    [0.86, 0.71, 0.46],
    [0.60, 0.78, 0.58],
    [0.86, 0.57, 0.53],
    [0.72, 0.63, 0.85],
    [0.51, 0.76, 0.78],
    [0.88, 0.80, 0.52],
];

// ---------------------------------------------------------------- data

/// Rigid placement: world = rot · local + pos.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement { pub rot: DQuat, pub pos: V3 }

impl Default for Placement { fn default() -> Self { Placement { rot: DQuat::IDENTITY, pos: V3::ZERO } } }

impl Placement {
    pub fn translation(d: V3) -> Placement { Placement { rot: DQuat::IDENTITY, pos: d } }
    pub fn point(&self, p: V3) -> V3 { self.rot * p + self.pos }
    pub fn vector(&self, v: V3) -> V3 { self.rot * v }
    pub fn inverse(&self) -> Placement { let r = self.rot.inverse(); Placement { rot: r, pos: -(r * self.pos) } }
    pub fn xform(&self) -> Xform { Xform { m: DMat3::from_quat(self.rot), t: self.pos } }
    pub fn moved(&self, d: V3) -> Placement { Placement { rot: self.rot, pos: self.pos + d } }
    /// Rotation by `angle` about the world axis through `centre` along `axis`.
    pub fn rotated(&self, centre: V3, axis: V3, angle: f64) -> Placement {
        let q = DQuat::from_axis_angle(safe_norm(axis, V3::Z), angle);
        Placement { rot: (q * self.rot).normalize(), pos: centre + q * (self.pos - centre) }
    }
    pub fn frame(&self, f: &Frame) -> Frame { Frame { origin: self.point(f.origin), x: self.vector(f.x), y: self.vector(f.y) } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AsmCType { Automatic, Distance, Angle, Parallel, Coincident, Normal, Default, Fix }

impl AsmCType {
    pub const MENU: [AsmCType; 8] = [AsmCType::Automatic, AsmCType::Distance, AsmCType::Angle, AsmCType::Parallel, AsmCType::Coincident, AsmCType::Normal, AsmCType::Default, AsmCType::Fix];
    pub fn name(&self) -> &'static str {
        match self {
            AsmCType::Automatic => "Automatisch", AsmCType::Distance => "Abstand", AsmCType::Angle => "Winkelversatz",
            AsmCType::Parallel => "Parallel", AsmCType::Coincident => "Zusammenfallend", AsmCType::Normal => "Normal",
            AsmCType::Default => "Standard", AsmCType::Fix => "Fixieren",
        }
    }
    pub fn needs_refs(&self) -> bool { !matches!(self, AsmCType::Default | AsmCType::Fix) }
    pub fn has_offset(&self) -> bool { matches!(self, AsmCType::Distance | AsmCType::Angle) }
}

/// A placement reference. Component references are stored in part coordinates and are
/// re-resolved geometrically after the part changes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AsmRef {
    /// assembly datum plane (ASM_RIGHT / ASM_TOP / ASM_FRONT)
    AsmPlane(u64),
    /// surface of a component: planar surfaces act as planes, surfaces of revolution as their axis
    Surface { comp: u64, face: FaceRef, planar: bool },
    /// datum plane of a component's part
    Plane { comp: u64, id: u64 },
    /// datum axis or feature axis of a component's part
    Axis { comp: u64, id: u64 },
    /// datum point of a component's part
    Point { comp: u64, id: u64 },
    /// straight edge of a component
    Edge { comp: u64, a: V3, b: V3 },
}

impl AsmRef {
    pub fn comp(&self) -> Option<u64> {
        match *self {
            AsmRef::AsmPlane(_) => None,
            AsmRef::Surface { comp, .. } | AsmRef::Plane { comp, .. } | AsmRef::Axis { comp, .. } | AsmRef::Point { comp, .. } | AsmRef::Edge { comp, .. } => Some(comp),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AsmConstraint {
    pub ty: AsmCType,
    /// reference on the component being placed
    pub a: Option<AsmRef>,
    /// reference on the assembly (a datum or an earlier component)
    pub b: Option<AsmRef>,
    /// Distance in mm, Angle Offset in degrees
    pub offset: f64,
    /// orientation: -1 opposed, +1 aligned, 0 = default (planes decide on the first solve,
    /// axes stay free to keep either sense), 2 = flip the current sense on the next solve
    #[serde(default)]
    pub sign: f64,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool { true }

impl AsmConstraint {
    pub fn new(ty: AsmCType) -> AsmConstraint { AsmConstraint { ty, a: None, b: None, offset: 0.0, sign: 0.0, enabled: true } }
    pub fn complete(&self) -> bool { !self.ty.needs_refs() || (self.a.is_some() && self.b.is_some()) }
    /// Flip button: reverse the orientation (relative to the current one when it is free).
    pub fn flip(&mut self) { self.sign = if self.sign == 0.0 || self.sign == 2.0 { 2.0 } else { -self.sign }; }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub id: u64,
    /// name of the part (key into `AssemblyData::parts`)
    pub part: String,
    pub constraints: Vec<AsmConstraint>,
    /// last solved placement (and the starting point of the next solve)
    pub placement: Placement,
    #[serde(default)]
    pub suppressed: bool,
    #[serde(default)]
    pub hidden: bool,
    /// manual explode offset added to the automatic one
    #[serde(default)]
    pub explode: V3,
    #[serde(default = "yes")]
    pub assumptions: bool,
}

impl Component {
    pub fn new(id: u64, part: &str, placement: Placement) -> Component {
        Component { id, part: part.into(), constraints: vec![], placement, suppressed: false, hidden: false, explode: V3::ZERO, assumptions: true }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartEntry {
    pub data: ModelData,
    /// part file the component was assembled from (saved back when modified)
    #[serde(default)]
    pub source: Option<PathBuf>,
    #[serde(skip)]
    pub modified: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssemblyData {
    pub name: String,
    /// assembly datums ASM_RIGHT, ASM_TOP, ASM_FRONT and ASM_DEF_CSYS
    pub skeleton: ModelData,
    pub parts: Vec<PartEntry>,
    pub components: Vec<Component>,
    pub next_id: u64,
}

impl AssemblyData {
    pub fn new(name: &str) -> AssemblyData {
        AssemblyData { name: name.into(), skeleton: skeleton(name), parts: vec![], components: vec![], next_id: FIRST_COMPONENT_ID }
    }
    pub fn part(&self, name: &str) -> Option<&PartEntry> { self.parts.iter().find(|p| p.data.name == name) }
    pub fn part_mut(&mut self, name: &str) -> Option<&mut PartEntry> { self.parts.iter_mut().find(|p| p.data.name == name) }
    pub fn component(&self, id: u64) -> Option<&Component> { self.components.iter().find(|c| c.id == id) }
    pub fn component_mut(&mut self, id: u64) -> Option<&mut Component> { self.components.iter_mut().find(|c| c.id == id) }
    pub fn index_of(&self, id: u64) -> Option<usize> { self.components.iter().position(|c| c.id == id) }

    /// Adds a part to the part list (reusing an entry of the same name) and returns its name.
    pub fn add_part(&mut self, data: ModelData, source: Option<PathBuf>) -> String {
        let name = data.name.clone();
        if self.part(&name).is_none() { self.parts.push(PartEntry { data, source, modified: false }); }
        name
    }

    /// Next unused part name of the form PRT0001.
    pub fn next_part_name(&self) -> String {
        (1..).map(|i| format!("PRT{:04}", i)).find(|n| self.part(n).is_none() && *n != self.name).unwrap()
    }

    pub fn color_of(&self, part: &str) -> [f32; 3] {
        let i = self.parts.iter().position(|p| p.data.name == part).unwrap_or(0);
        let c = self.parts.get(i).map(|p| p.data.appearance.color).unwrap_or(PALETTE[0]);
        if c == Appearance::default().color { PALETTE[i % PALETTE.len()] } else { c }
    }

    /// Components whose constraints reference `id` (directly), i.e. its children.
    pub fn children_of(&self, id: u64) -> Vec<u64> {
        self.components.iter().filter(|c| c.id != id && c.constraints.iter().any(|k| k.a.and_then(|r| r.comp()) == Some(id) || k.b.and_then(|r| r.comp()) == Some(id))).map(|c| c.id).collect()
    }
}

/// The assembly's own datum features, named as in Creo.
pub fn skeleton(name: &str) -> ModelData {
    let mut m = ModelData::new_part(name);
    for f in &mut m.features {
        f.name = match f.id { RIGHT_ID => "ASM_RIGHT", TOP_ID => "ASM_TOP", FRONT_ID => "ASM_FRONT", CSYS_ID => "ASM_DEF_CSYS", _ => continue }.into();
    }
    m
}

// ---------------------------------------------------------------- geometry of references

#[derive(Clone, Copy, Debug)]
pub enum Geo { Plane { p: V3, n: V3 }, Line { p: V3, d: V3 }, Point(V3) }

impl Geo {
    pub fn placed(&self, pl: &Placement) -> Geo {
        match *self {
            Geo::Plane { p, n } => Geo::Plane { p: pl.point(p), n: pl.vector(n) },
            Geo::Line { p, d } => Geo::Line { p: pl.point(p), d: pl.vector(d) },
            Geo::Point(p) => Geo::Point(pl.point(p)),
        }
    }
    pub fn kind_name(&self) -> &'static str { match self { Geo::Plane { .. } => "plane", Geo::Line { .. } => "axis", Geo::Point(_) => "point" } }
}

/// Resolves a component reference in its part's coordinates.
pub fn local_geo(r: &AsmRef, part: &RegenResult) -> Option<Geo> {
    let t = &part.topo;
    match *r {
        AsmRef::AsmPlane(_) => None,
        AsmRef::Surface { face, planar: true, .. } => {
            let f = t.match_face(face.point, face.normal)?;
            Some(Geo::Plane { p: t.faces[f].center, n: t.faces[f].normal })
        }
        AsmRef::Surface { face, planar: false, .. } => {
            let f = t.match_surface(face.point, face.normal)?;
            let (p, d, _) = t.surface_axis(f)?;
            Some(Geo::Line { p, d })
        }
        AsmRef::Plane { id, .. } => part.plane(id).map(|p| Geo::Plane { p: p.frame.origin, n: p.frame.z() }),
        AsmRef::Axis { id, .. } => part.axis(id).map(|a| Geo::Line { p: a.point, d: a.dir }),
        AsmRef::Point { id, .. } => part.points.iter().find(|q| q.id == id).map(|q| Geo::Point(q.p)),
        AsmRef::Edge { a, b, .. } => Some(Geo::Line { p: a, d: safe_norm(b - a, V3::X) }),
    }
}

fn perp(v: V3, d: V3) -> V3 { v - d * v.dot(d) }

/// Orientation chosen when a constraint is first solved: surfaces mate (opposed),
/// datum planes align, axes keep their current sense.
fn default_sign(c: &AsmConstraint, a: &Geo, b: &Geo) -> f64 {
    let surf = |r: Option<AsmRef>| matches!(r, Some(AsmRef::Surface { planar: true, .. }));
    let datum = |r: Option<AsmRef>| matches!(r, Some(AsmRef::Plane { .. }) | Some(AsmRef::AsmPlane(_)));
    let near = |u: V3, v: V3| if u.dot(v) >= 0.0 { 1.0 } else { -1.0 };
    match (a, b) {
        (Geo::Plane { n: na, .. }, Geo::Plane { n: nb, .. }) => {
            if c.ty == AsmCType::Angle { 1.0 }
            else if surf(c.a) && surf(c.b) { -1.0 }
            else if datum(c.a) && datum(c.b) { 1.0 }
            else { near(*na, *nb) }
        }
        (Geo::Line { d: da, .. }, Geo::Line { d: db, .. }) => if c.ty == AsmCType::Angle { 1.0 } else if c.sign == 2.0 { -near(*da, *db) } else { 0.0 },
        (Geo::Line { d, .. }, Geo::Plane { n, .. }) | (Geo::Plane { n, .. }, Geo::Line { d, .. }) => near(*d, *n),
        _ => 1.0,
    }
}

/// Residuals of one constraint (`a` already placed in world coordinates). Orientation terms
/// are scaled by `l` so they weigh like lengths.
fn residual(ty: AsmCType, sign: f64, off: f64, a: &Geo, b: &Geo, l: f64, out: &mut Vec<f64>) -> Result<(), String> {
    use Geo::*;
    let v3 = |out: &mut Vec<f64>, v: V3, k: f64| out.extend_from_slice(&[v.x * k, v.y * k, v.z * k]);
    // axes with a free sense only need to be parallel
    let along = |u: V3, v: V3| if sign == 0.0 { u.cross(v) } else { u - v * sign };
    let d = if ty == AsmCType::Distance { off } else { 0.0 };
    match (ty, a, b) {
        (AsmCType::Coincident | AsmCType::Distance, Plane { p: pa, n: na }, Plane { p: pb, n: nb }) => {
            v3(out, *na - *nb * sign, l);
            out.push(nb.dot(*pa - *pb) - d);
        }
        (AsmCType::Coincident, Line { p: pa, d: da }, Line { p: pb, d: db }) => { v3(out, along(*da, *db), l); v3(out, perp(*pa - *pb, *db), 1.0); }
        (AsmCType::Distance, Line { p: pa, d: da }, Line { p: pb, d: db }) => { v3(out, along(*da, *db), l); out.push(perp(*pa - *pb, *db).length() - d); }
        (AsmCType::Coincident, Point(pa), Point(pb)) => v3(out, *pa - *pb, 1.0),
        (AsmCType::Distance, Point(pa), Point(pb)) => out.push((*pa - *pb).length() - d),
        (AsmCType::Coincident | AsmCType::Distance, Point(pa), Plane { p: pb, n: nb }) => out.push(nb.dot(*pa - *pb) - d),
        (AsmCType::Coincident | AsmCType::Distance, Plane { p: pa, n: na }, Point(pb)) => out.push(na.dot(*pb - *pa) - d),
        (AsmCType::Coincident, Point(pa), Line { p: pb, d: db }) => v3(out, perp(*pa - *pb, *db), 1.0),
        (AsmCType::Coincident, Line { p: pa, d: da }, Point(pb)) => v3(out, perp(*pb - *pa, *da), 1.0),
        (AsmCType::Distance, Point(pa), Line { p: pb, d: db }) => out.push(perp(*pa - *pb, *db).length() - d),
        (AsmCType::Distance, Line { p: pa, d: da }, Point(pb)) => out.push(perp(*pb - *pa, *da).length() - d),
        (AsmCType::Coincident | AsmCType::Distance, Line { p: pa, d: da }, Plane { p: pb, n: nb }) => { out.push(nb.dot(*da) * l); out.push(nb.dot(*pa - *pb) - d); }
        (AsmCType::Coincident | AsmCType::Distance, Plane { p: pa, n: na }, Line { p: pb, d: db }) => { out.push(na.dot(*db) * l); out.push(na.dot(*pb - *pa) - d); }
        (AsmCType::Parallel, Plane { n: na, .. }, Plane { n: nb, .. }) => v3(out, *na - *nb * sign, l),
        (AsmCType::Parallel, Line { d: na, .. }, Line { d: nb, .. }) => v3(out, along(*na, *nb), l),
        (AsmCType::Parallel, Line { d, .. }, Plane { n, .. }) | (AsmCType::Parallel, Plane { n, .. }, Line { d, .. }) => out.push(n.dot(*d) * l),
        (AsmCType::Normal, Plane { n: na, .. }, Plane { n: nb, .. }) | (AsmCType::Normal, Line { d: na, .. }, Line { d: nb, .. }) => out.push(na.dot(*nb) * l),
        (AsmCType::Normal, Line { d, .. }, Plane { n, .. }) | (AsmCType::Normal, Plane { n, .. }, Line { d, .. }) => v3(out, *d - *n * sign, l),
        (AsmCType::Angle, Plane { n: na, .. }, Plane { n: nb, .. }) | (AsmCType::Angle, Line { d: na, .. }, Line { d: nb, .. }) => {
            out.push((na.dot(*nb) - sign * off.to_radians().cos()) * l)
        }
        _ => return Err(format!("{} kann kein(e) {} mit einem/einer {} verknüpfen.", ty.name(), a.kind_name(), b.kind_name())),
    }
    Ok(())
}

// ---------------------------------------------------------------- placement solver

#[derive(Clone, Debug, PartialEq)]
pub enum PlaceStatus { Packaged, Partial(usize), Full, Assumed, Invalid(String) }

impl PlaceStatus {
    pub fn text(&self) -> String {
        match self {
            PlaceStatus::Packaged => "Keine Bedingungen".into(),
            PlaceStatus::Partial(n) => format!("Teilweise bedingt ({} FG)", n),
            PlaceStatus::Full => "Vollständig bedingt".into(),
            PlaceStatus::Assumed => "Vollständig bedingt (Annahmen)".into(),
            PlaceStatus::Invalid(e) => format!("Bedingungen ungültig — {}", e),
        }
    }
    pub fn is_full(&self) -> bool { matches!(self, PlaceStatus::Full | PlaceStatus::Assumed) }
}

/// A constraint ready to solve: component geometry in part coordinates, assembly geometry in world.
#[derive(Clone, Copy, Debug)]
pub struct Prepared { pub ty: AsmCType, pub sign: f64, pub off: f64, pub a: Geo, pub b: Geo }

fn trial(start: &Placement, centre: V3, x: &[f64; 6]) -> Placement {
    let rot = (DQuat::from_scaled_axis(V3::new(x[0], x[1], x[2])) * start.rot).normalize();
    let cw = start.point(centre) + V3::new(x[3], x[4], x[5]);
    Placement { rot, pos: cw - rot * centre }
}

fn eval(start: &Placement, centre: V3, l: f64, cons: &[Prepared], target: Option<Placement>, x: &[f64; 6]) -> Result<Vec<f64>, String> {
    let pl = trial(start, centre, x);
    let mut r = vec![];
    if let Some(t) = target {
        let mut dq = pl.rot * t.rot.inverse();
        if dq.w < 0.0 { dq = -dq; }
        r.extend_from_slice(&[dq.x * 2.0 * l, dq.y * 2.0 * l, dq.z * 2.0 * l]);
        let d = pl.point(centre) - t.point(centre);
        r.extend_from_slice(&[d.x, d.y, d.z]);
    }
    for c in cons { residual(c.ty, c.sign, c.off, &c.a.placed(&pl), &c.b, l, &mut r)?; }
    Ok(r)
}

/// Solves one component's placement from `start`. `centre` is its local rotation centre and
/// `l` its size; `target` pins the whole placement (Default / Fix). `spin` lists world axes
/// about which a remaining rotation may be assumed fixed (coaxial constraints).
pub fn solve_placement(start: Placement, centre: V3, l: f64, cons: &[Prepared], target: Option<Placement>, spin: &[V3], assumptions: bool) -> (Placement, PlaceStatus) {
    if cons.is_empty() && target.is_none() { return (start, PlaceStatus::Packaged); }
    if let Some(e) = eval(&start, centre, l, cons, target, &[0.0; 6]).err() { return (start, PlaceStatus::Invalid(e)); }
    // a reversed orientation sits on a saddle of the residual: retry from turned starts
    let c = start.point(centre);
    let starts = [
        start,
        start.rotated(c, V3::new(1.0, 1.0, 1.0), 0.6),
        start.rotated(c, V3::X, std::f64::consts::PI),
        start.rotated(c, V3::Y, std::f64::consts::PI),
        start.rotated(c, V3::Z, std::f64::consts::PI),
        start.rotated(c, V3::new(1.0, -1.0, 0.5), 2.0),
    ];
    let mut best: Option<(Placement, PlaceStatus, f64)> = None;
    for s in starts {
        let (pl, st, res) = solve_from(s, centre, l, cons, target, spin, assumptions);
        let ok = !matches!(st, PlaceStatus::Invalid(_));
        if ok { return (pl, st); }
        if best.as_ref().map(|b| res < b.2).unwrap_or(true) { best = Some((pl, st, res)); }
    }
    // nothing satisfies the constraints: the component keeps its position
    (start, best.map(|b| b.1).unwrap_or(PlaceStatus::Packaged))
}

fn solve_from(start: Placement, centre: V3, l: f64, cons: &[Prepared], target: Option<Placement>, spin: &[V3], assumptions: bool) -> (Placement, PlaceStatus, f64) {
    let f = |x: &[f64; 6]| eval(&start, centre, l, cons, target, x);
    let mut x = [0.0f64; 6];
    let mut r = match f(&x) { Ok(r) => r, Err(e) => return (start, PlaceStatus::Invalid(e), f64::INFINITY) };
    let sq = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
    let mut fx = sq(&r);
    let mut lambda = 1e-3;
    let m = r.len();
    let jac = |x: &[f64; 6]| -> Vec<f64> {
        let mut j = vec![0.0; m * 6];
        for k in 0..6 {
            let h = if k < 3 { 1e-7 } else { 1e-7 * l.max(1.0) };
            let (mut xp, mut xm) = (*x, *x);
            xp[k] += h;
            xm[k] -= h;
            let (rp, rm) = (f(&xp).unwrap_or_default(), f(&xm).unwrap_or_default());
            if rp.len() != m || rm.len() != m { continue; }
            for i in 0..m { j[i * 6 + k] = (rp[i] - rm[i]) / (2.0 * h); }
        }
        j
    };
    let tol = 1e-9 * l.max(1.0);
    'outer: for _ in 0..200 {
        if fx.sqrt() < tol { break; }
        let j = jac(&x);
        let mut a = [0.0f64; 36];
        let mut g = [0.0f64; 6];
        for i in 0..m {
            for p in 0..6 {
                g[p] += j[i * 6 + p] * r[i];
                for q in 0..6 { a[p * 6 + q] += j[i * 6 + p] * j[i * 6 + q]; }
            }
        }
        loop {
            let mut aa = a;
            for p in 0..6 { aa[p * 7] += lambda * (a[p * 7] + 1e-6); }
            let Some(dx) = solve_dense(&aa, &g.map(|v| -v), 6) else { break 'outer };
            let mut xn = x;
            for k in 0..6 { xn[k] += dx[k]; }
            let rn = match f(&xn) { Ok(v) => v, Err(_) => break 'outer };
            let fnew = sq(&rn);
            if fnew < fx {
                let step: f64 = dx.iter().map(|v| v * v).sum::<f64>().sqrt();
                x = xn;
                r = rn;
                fx = fnew;
                lambda = (lambda * 0.3).max(1e-12);
                if step < 1e-13 * l.max(1.0) { break 'outer; }
                break;
            }
            lambda *= 10.0;
            if lambda > 1e14 { break 'outer; }
        }
    }
    let pl = trial(&start, centre, &x);
    let res = fx.sqrt();
    if res > 1e-5 * l.max(1.0) { return (pl, PlaceStatus::Invalid("die Referenzen können nicht gleichzeitig erfüllt werden".into()), res); }
    let j = jac(&x);
    let maxabs = j.iter().fold(0.0f64, |a, v| a.max(v.abs())).max(1e-9);
    let rank = matrix_rank(&j, m, 6, 1e-6 * maxabs);
    if rank >= 6 { return (pl, PlaceStatus::Full, res); }
    if assumptions && rank == 5 {
        // a single free rotation about a coaxial reference is assumed fixed (Creo's Allow Assumptions)
        for s in spin {
            let mut jj = j.clone();
            jj.extend_from_slice(&[s.x * maxabs, s.y * maxabs, s.z * maxabs, 0.0, 0.0, 0.0]);
            if matrix_rank(&jj, m + 1, 6, 1e-6 * maxabs) == 6 { return (pl, PlaceStatus::Assumed, res); }
        }
    }
    (pl, PlaceStatus::Partial(6 - rank), res)
}

// ---------------------------------------------------------------- regeneration

/// Regenerated parts, keyed by part name and invalidated by content.
#[derive(Default)]
pub struct PartCache { map: HashMap<String, (u64, Arc<RegenResult>)> }

impl PartCache {
    pub fn get(&mut self, d: &ModelData) -> Arc<RegenResult> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(d).unwrap_or_default().hash(&mut h);
        let key = h.finish();
        if let Some((k, r)) = self.map.get(&d.name) { if *k == key { return r.clone(); } }
        let r = Arc::new(regen::run(d));
        self.map.insert(d.name.clone(), (key, r.clone()));
        r
    }
}

#[derive(Clone, Default)]
pub struct AsmResult {
    /// all components merged in assembly coordinates (one body per component)
    pub display: RegenResult,
    /// solved placements (unexploded)
    pub placements: HashMap<u64, Placement>,
    /// displayed placements (with explode offsets)
    pub shown: HashMap<u64, Placement>,
    pub status: HashMap<u64, PlaceStatus>,
    pub parts: HashMap<String, Arc<RegenResult>>,
    pub skeleton: Arc<RegenResult>,
    /// body index (tag >> BODY_SHIFT) - 1 → component id
    pub bodies: Vec<u64>,
    pub body_colors: Vec<[f32; 3]>,
    pub messages: Vec<String>,
}

impl AsmResult {
    pub fn comp_of_tag(&self, tag: u32) -> Option<u64> { self.bodies.get(((tag >> BODY_SHIFT) as usize).checked_sub(1)?).copied() }
}

/// Resolves a reference in world coordinates, using the placements solved so far.
pub fn world_geo(r: &AsmRef, skel: &RegenResult, parts: &HashMap<String, Arc<RegenResult>>, comps: &[Component], placed: &HashMap<u64, Placement>) -> Option<Geo> {
    match *r {
        AsmRef::AsmPlane(id) => skel.plane(id).map(|p| Geo::Plane { p: p.frame.origin, n: p.frame.z() }),
        _ => {
            let id = r.comp()?;
            let c = comps.iter().find(|c| c.id == id)?;
            if c.suppressed { return None; }
            let pl = placed.get(&c.id).copied().unwrap_or(c.placement);
            local_geo(r, parts.get(&c.part)?).map(|g| g.placed(&pl))
        }
    }
}

pub struct RegenOpts<'a> {
    /// automatic explode factor (0 = assembled)
    pub explode: f64,
    /// component being edited in the placement dashboard (replaces or adds to the list)
    pub editing: Option<&'a Component>,
}

impl Default for RegenOpts<'_> { fn default() -> Self { RegenOpts { explode: 0.0, editing: None } } }

/// Prepares and solves one component; returns its placement, status and the orientation
/// signs decided for constraints that had none.
pub fn place_component(c: &Component, comps: &[Component], parts: &HashMap<String, Arc<RegenResult>>, skel: &RegenResult, placed: &HashMap<u64, Placement>) -> (Placement, PlaceStatus, Vec<f64>) {
    let Some(part) = parts.get(&c.part) else { return (c.placement, PlaceStatus::Invalid(format!("Teil {} fehlt", c.part)), vec![]) };
    let mut prepared = vec![];
    let mut target = None;
    let mut spin = vec![];
    let mut signs = vec![];
    for k in &c.constraints {
        signs.push(k.sign);
        if !k.enabled { continue; }
        match k.ty {
            AsmCType::Default => { target = Some(Placement::default()); continue; }
            AsmCType::Fix => { target = Some(c.placement); continue; }
            _ => {}
        }
        let (Some(mut ra), Some(mut rb)) = (k.a, k.b) else { continue };
        if ra.comp() != Some(c.id) && rb.comp() == Some(c.id) { std::mem::swap(&mut ra, &mut rb); }
        if ra.comp() != Some(c.id) { return (c.placement, PlaceStatus::Invalid("eine Bedingung hat keine Referenz an dieser Komponente".into()), signs); }
        let Some(ga) = local_geo(&ra, part) else { return (c.placement, PlaceStatus::Invalid("eine Komponentenreferenz fehlt".into()), signs) };
        let Some(gb) = world_geo(&rb, skel, parts, comps, placed) else { return (c.placement, PlaceStatus::Invalid("eine Baugruppenreferenz fehlt".into()), signs) };
        let ty = if k.ty == AsmCType::Automatic { if k.offset != 0.0 { AsmCType::Distance } else { AsmCType::Coincident } } else { k.ty };
        let mut kk = k.clone();
        kk.ty = ty;
        kk.a = Some(ra);
        kk.b = Some(rb);
        let sign = if k.sign == 0.0 || k.sign == 2.0 { default_sign(&kk, &ga.placed(&c.placement), &gb) } else { k.sign };
        *signs.last_mut().unwrap() = sign;
        if let (AsmCType::Coincident, Geo::Line { d, .. }) = (ty, gb) { spin.push(d); }
        prepared.push(Prepared { ty, sign, off: k.offset, a: ga, b: gb });
    }
    let b = &part.topo.bbox;
    let centre = if b.is_empty() { V3::ZERO } else { b.center() };
    let l = if b.is_empty() { 100.0 } else { b.diagonal().max(10.0) };
    let (pl, st) = solve_placement(c.placement, centre, l, &prepared, target, &spin, c.assumptions);
    (pl, st, signs)
}

/// Regenerates every part, solves the placements in order and builds the merged display.
pub fn regen(asm: &AssemblyData, cache: &mut PartCache, opts: &RegenOpts) -> AsmResult {
    let t0 = std::time::Instant::now();
    let mut res = AsmResult::default();
    for p in &asm.parts { res.parts.insert(p.data.name.clone(), cache.get(&p.data)); }
    res.skeleton = cache.get(&asm.skeleton);
    let mut comps: Vec<Component> = asm.components.clone();
    if let Some(e) = opts.editing {
        match comps.iter().position(|c| c.id == e.id) { Some(i) => comps[i] = e.clone(), None => comps.push(e.clone()) }
    }
    for c in &comps {
        if c.suppressed { continue; }
        let (pl, st, _) = place_component(c, &comps, &res.parts, &res.skeleton, &res.placements);
        if let PlaceStatus::Invalid(e) = &st { res.messages.push(format!("{}: {}", c.part, e)); }
        res.placements.insert(c.id, pl);
        res.status.insert(c.id, st);
    }
    // explode offsets: away from the assembly centre
    let mut centre_of: HashMap<u64, V3> = HashMap::new();
    let mut all = BBox::default();
    for c in comps.iter().filter(|c| !c.suppressed) {
        let (Some(part), Some(pl)) = (res.parts.get(&c.part), res.placements.get(&c.id)) else { continue };
        if part.topo.bbox.is_empty() { continue; }
        let cc = pl.point(part.topo.bbox.center());
        centre_of.insert(c.id, cc);
        for k in 0..8 {
            let (mn, mx) = (part.topo.bbox.min, part.topo.bbox.max);
            all.add(pl.point(V3::new(if k & 1 == 0 { mn.x } else { mx.x }, if k & 2 == 0 { mn.y } else { mx.y }, if k & 4 == 0 { mn.z } else { mx.z })));
        }
    }
    let ac = if all.is_empty() { V3::ZERO } else { all.center() };
    for c in comps.iter().filter(|c| !c.suppressed) {
        let Some(pl) = res.placements.get(&c.id) else { continue };
        let off = if opts.explode > 0.0 { centre_of.get(&c.id).map(|cc| (*cc - ac) * opts.explode).unwrap_or(V3::ZERO) + c.explode } else { V3::ZERO };
        res.shown.insert(c.id, pl.moved(off));
    }
    // merged display
    let d = &mut res.display;
    d.planes = res.skeleton.planes.clone();
    d.points = res.skeleton.points.clone();
    let mut polys: Vec<CPolygon> = vec![];
    for c in comps.iter().filter(|c| !c.suppressed) {
        let (Some(part), Some(pl)) = (res.parts.get(&c.part), res.shown.get(&c.id)) else { continue };
        for (fid, e) in &part.errors {
            let n = asm.part(&c.part).and_then(|p| p.data.feature(*fid)).map(|f| f.name.clone()).unwrap_or_default();
            res.messages.push(format!("{}: {} fehlgeschlagen: {}", c.part, n, e));
        }
        res.bodies.push(c.id);
        res.body_colors.push(asm.color_of(&c.part));
        if c.hidden { continue; }
        let body = (res.bodies.len() as u32) << BODY_SHIFT;
        let x = pl.xform();
        for p in &part.polys {
            let mut q = p.transformed(&x);
            q.tag = body | (p.tag & ((1 << BODY_SHIFT) - 1));
            d.tag_owner.insert(q.tag, c.id);
            polys.push(q);
        }
        let b = &part.topo.bbox;
        let size = if b.is_empty() { 40.0 } else { (b.diagonal() * 0.36).max(15.0) };
        let lc = if b.is_empty() { V3::ZERO } else { b.center() };
        for p in &part.planes {
            let mut f = p.frame;
            f.origin = lc - f.z() * (lc - f.origin).dot(f.z());
            d.planes.push(PlaneInfo { id: comp_datum_id(c.id, p.id), name: p.name.clone(), frame: pl.frame(&f), size });
        }
        for a in &part.axes {
            d.axes.push(AxisInfo { id: comp_datum_id(c.id, a.id), name: a.name.clone(), point: pl.point(a.point), dir: pl.vector(a.dir), feature: c.id });
        }
        for q in &part.points { d.points.push(PointInfo { id: comp_datum_id(c.id, q.id), name: q.name.clone(), p: pl.point(q.p) }); }
        for cu in &part.curves { d.curves.push(CurveInfo { feature: c.id, polylines: cu.polylines.iter().map(|l| l.iter().map(|v| pl.point(*v)).collect()).collect() }); }
        for co in &part.cosmetics { d.cosmetics.push(Cosmetic { feature: c.id, base: pl.point(co.base), dir: pl.vector(co.dir), radius: co.radius, length: co.length, label: co.label.clone() }); }
    }
    d.topo = Arc::new(Topology::new(&polys));
    d.polys = polys;
    d.time = t0.elapsed().as_secs_f64();
    d.generation = regen::next_generation();
    res
}

/// Component geometry in assembly coordinates (solved, unexploded).
pub fn world_polys(c: &Component, res: &AsmResult) -> Vec<CPolygon> {
    let (Some(part), Some(pl)) = (res.parts.get(&c.part), res.placements.get(&c.id)) else { return vec![] };
    let x = pl.xform();
    part.polys.iter().map(|p| p.transformed(&x)).collect()
}

// ---------------------------------------------------------------- analysis

/// Pairs of components whose solids overlap, with the interference volume (mm³).
pub fn interference(asm: &AssemblyData, res: &AsmResult) -> Vec<(u64, u64, f64)> {
    let comps: Vec<&Component> = asm.components.iter().filter(|c| !c.suppressed && res.placements.contains_key(&c.id)).collect();
    let geo: Vec<(Vec<CPolygon>, BBox)> = comps.iter().map(|c| { let p = world_polys(c, res); let b = csg::bbox(&p); (p, b) }).collect();
    let mut out = vec![];
    for i in 0..comps.len() {
        for j in i + 1..comps.len() {
            let (a, b) = (&geo[i], &geo[j]);
            if a.0.is_empty() || b.0.is_empty() { continue; }
            let overlap = a.1.min.x < b.1.max.x && b.1.min.x < a.1.max.x && a.1.min.y < b.1.max.y && b.1.min.y < a.1.max.y && a.1.min.z < b.1.max.z && b.1.min.z < a.1.max.z;
            if !overlap { continue; }
            let v = csg::signed_volume(&csg::intersect(&a.0, &b.0));
            let scale = csg::signed_volume(&a.0).abs().min(csg::signed_volume(&b.0).abs());
            if v > (scale * 1e-6).max(1e-3) { out.push((comps[i].id, comps[j].id, v)); }
        }
    }
    out
}

#[derive(Clone, Debug)]
pub struct BomRow { pub part: String, pub qty: usize, pub volume: f64, pub mass: f64, pub density: f64, pub source: Option<PathBuf> }

/// Bill of materials: one row per part with its quantity in the assembly.
pub fn bom(asm: &AssemblyData, res: &AsmResult) -> Vec<BomRow> {
    asm.parts.iter().filter_map(|p| {
        let qty = asm.components.iter().filter(|c| c.part == p.data.name && !c.suppressed).count();
        if qty == 0 { return None; }
        let volume = res.parts.get(&p.data.name).map(|r| csg::signed_volume(&r.polys)).unwrap_or(0.0);
        Some(BomRow { part: p.data.name.clone(), qty, volume, mass: volume * p.data.density, density: p.data.density, source: p.source.clone() })
    }).collect()
}

pub fn bom_csv(asm: &AssemblyData, rows: &[BomRow]) -> String {
    let mut s = format!("Baugruppe,{}\nPos.,Teil,Menge,Volumen (mm3),Einzelmasse (kg),Gesamtmasse (kg),Datei\n", asm.name);
    for (i, r) in rows.iter().enumerate() {
        let file = r.source.as_ref().map(|p| p.display().to_string()).unwrap_or_default().replace(',', " ");
        s.push_str(&format!("{},{},{},{:.3},{:.6},{:.6},{}\n", i + 1, r.part, r.qty, r.volume, r.mass, r.mass * r.qty as f64, file));
    }
    let total: f64 = rows.iter().map(|r| r.mass * r.qty as f64).sum();
    let count: usize = rows.iter().map(|r| r.qty).sum();
    s.push_str(&format!(",Summe,{},,,{:.6},\n", count, total));
    s
}

/// Human-readable reference name, e.g. "Surf:Extrude 1:PRT0002" or "ASM_FRONT".
/// `editing` is a component not yet added to the assembly (the one being placed).
pub fn ref_name(r: &AsmRef, asm: &AssemblyData, editing: Option<&Component>, res: Option<&AsmResult>) -> String {
    let comp = r.comp().and_then(|id| asm.component(id).or(editing.filter(|c| c.id == id)));
    let part_name = comp.map(|c| c.part.clone()).unwrap_or_default();
    let part = comp.and_then(|c| asm.part(&c.part));
    let pres = comp.and_then(|c| res.and_then(|x| x.parts.get(&c.part)));
    let feat_of = |tag: u32| part.and_then(|p| pres.and_then(|r| r.tag_owner.get(&tag)).and_then(|fid| p.data.feature(*fid))).map(|f| f.name.clone());
    match *r {
        AsmRef::AsmPlane(id) => asm.skeleton.feature(id).map(|f| f.name.clone()).unwrap_or("BG-Bezug".into()),
        AsmRef::Surface { face, planar, .. } => {
            let f = pres.and_then(|r| if planar { r.topo.match_face(face.point, face.normal) } else { r.topo.match_surface(face.point, face.normal) });
            let feat = f.and_then(|f| pres.map(|r| r.topo.face_tag(f))).and_then(feat_of).unwrap_or_default();
            format!("Fläche:{}:{}", feat, part_name)
        }
        AsmRef::Plane { id, .. } => format!("{}:{}", part.and_then(|p| p.data.feature(id)).map(|f| f.name.clone()).unwrap_or("Ebene".into()), part_name),
        AsmRef::Axis { id, .. } => format!("{}:{}", pres.and_then(|r| r.axis(id)).map(|a| a.name.clone()).unwrap_or("Achse".into()), part_name),
        AsmRef::Point { id, .. } => format!("{}:{}", pres.and_then(|r| r.points.iter().find(|q| q.id == id)).map(|q| q.name.clone()).unwrap_or("Punkt".into()), part_name),
        AsmRef::Edge { .. } => format!("Kante:{}", part_name),
    }
}
