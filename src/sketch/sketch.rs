//! 2D sketch model: points, entities (line/circle/arc/point), constraints and dimensions,
//! plus closed-profile extraction for solid features.

use crate::kernel::math::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const REF_H: i64 = -1;
pub const REF_V: i64 = -2;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkPoint {
    pub id: i64,
    pub p: V2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkKind { Line, Circle, Arc, Point, Spline, Ellipse, Conic, Text, Csys, Corner }

/// Sketch text (Creo's Text tool): the entity's two points give height and direction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkText {
    pub text: String,
    /// 0 = Ubuntu Light, 1 = Hack
    pub font: u8,
    /// Seitenverhältnis (width factor)
    pub aspect: f64,
    /// Neigungswinkel in degrees
    pub slant: f64,
    /// installed font by name (file name without extension); empty = the bundled font `font`
    #[serde(default)]
    pub family: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkEntity {
    pub id: i64,
    pub kind: SkKind,
    /// line [a,b]; circle [c]; arc [c, start, end] (CCW start->end); point [p];
    /// spline [p0..pn] interpolated (closed when first == last); ellipse [c, major-axis end] with r = minor radius;
    /// conic [start, end, tangent-intersection] with r = rho; text [start, height point]; csys [origin];
    /// corner [start, end, corner point]: a curvature-continuous corner blend, r = `CornerShape::code`
    pub pts: Vec<i64>,
    pub r: f64,
    pub construction: bool,
    pub centerline: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<SkText>,
    /// Reference point (Referenzen ▸ Auswählen): its Creo name; the point is fixed
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ref_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkCType {
    Coincident, Horizontal, Vertical, Parallel, Perpendicular, Equal, Tangent, PointOn, Midpoint, Symmetric, Fix,
    Distance, HDistance, VDistance, PointLine, Radius, Diameter, Angle,
    /// rho of a conic
    Rho,
    /// Umfang: total length of the chain `ents`; needs a variable dimension (`var`) that it drives
    Perimeter,
    /// Basislinie (0.00) of ordinate dimensions: on a line `ents[0]`, or through a point `pts[0]`
    /// measuring along x (aux 0) or y (aux 1)
    Baseline,
    /// Ordinate dimension of point `pts[0]` from the baseline constraint `base`
    Ordinate,
}

impl SkCType {
    pub fn is_dimension(&self) -> bool {
        matches!(self, SkCType::Distance | SkCType::HDistance | SkCType::VDistance | SkCType::PointLine | SkCType::Radius | SkCType::Diameter | SkCType::Angle | SkCType::Rho | SkCType::Perimeter | SkCType::Baseline | SkCType::Ordinate)
    }
    pub fn glyph(&self) -> &'static str {
        match self {
            SkCType::Coincident => "●", SkCType::Horizontal => "H", SkCType::Vertical => "V", SkCType::Parallel => "∥",
            SkCType::Perpendicular => "⊥", SkCType::Equal => "=", SkCType::Tangent => "T", SkCType::PointOn => "◦",
            SkCType::Midpoint => "M", SkCType::Symmetric => "↔", SkCType::Fix => "⚓", _ => "",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkConstraint {
    pub id: i64,
    pub ty: SkCType,
    pub pts: Vec<i64>,
    pub ents: Vec<i64>,
    pub value: f64,
    pub expr: Option<String>,
    /// dimension label offset relative to its anchor
    pub label: V2,
    pub aux: f64,
    pub fixed: Option<V2>,
    /// Weak (automatic) dimension: displayed and editable, but never drives the solver.
    #[serde(default)]
    pub weak: bool,
    /// Locked dimension (Operationen ▸ Sperre umschalten): kept when geometry is deleted around it.
    #[serde(default)]
    pub locked: bool,
    /// Reference dimension (Konvertieren in ▸ Referenz): measured, never drives the sketch.
    #[serde(default)]
    pub reference: bool,
    /// Variable dimension of a perimeter dimension ("VAR"): driven, not driving.
    #[serde(default)]
    pub var: bool,
    /// Ordinate dimensions: id of their baseline constraint.
    #[serde(default)]
    pub base: i64,
}

impl SkConstraint {
    pub fn new(ty: SkCType, pts: Vec<i64>, ents: Vec<i64>) -> SkConstraint {
        SkConstraint { id: 0, ty, pts, ents, value: 0.0, expr: None, label: V2::ZERO, aux: 0.0, fixed: None, weak: false, locked: false, reference: false, var: false, base: 0 }
    }

    /// Identity used to keep label positions of regenerated weak dimensions.
    pub fn key(&self) -> (SkCType, Vec<i64>, Vec<i64>) { (self.ty, self.pts.clone(), self.ents.clone()) }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    pub points: Vec<SkPoint>,
    pub entities: Vec<SkEntity>,
    pub constraints: Vec<SkConstraint>,
    pub ref_lines: Vec<[V2; 2]>,
    pub next_id: i64,
    /// references the user added (Referenzen ▸ Auswählen), appended after the model's own
    #[serde(default)]
    pub extra_refs: Vec<(String, [V2; 2])>,
    /// model references the user deleted
    #[serde(default)]
    pub hidden_refs: Vec<[V2; 2]>,
    /// default references (sketch axes from the datum planes) deleted: [horizontal, vertical]
    #[serde(default)]
    pub axes_off: [bool; 2],
}

impl Default for Sketch {
    fn default() -> Self {
        Sketch { points: vec![SkPoint { id: 0, p: V2::ZERO }], entities: vec![], constraints: vec![], ref_lines: vec![], next_id: 1, extra_refs: vec![], hidden_refs: vec![], axes_off: [false; 2] }
    }
}

#[derive(Clone, Debug)]
pub struct ProfileLoop {
    pub pts: Vec<V2>,
    pub seg_ent: Vec<i64>,
    pub seg_rad: Vec<Option<(V2, V2)>>,
}

impl ProfileLoop {
    pub fn area(&self) -> f64 { polygon_area2(&self.pts) }

    pub fn reversed(&self) -> ProfileLoop {
        let n = self.pts.len();
        let mut out = ProfileLoop { pts: vec![], seg_ent: vec![], seg_rad: vec![] };
        for j in 0..n {
            out.pts.push(self.pts[n - 1 - j]);
            let k = (2 * n - 2 - j) % n;
            out.seg_ent.push(self.seg_ent[k]);
            out.seg_rad.push(self.seg_rad[k].map(|(a, b)| (b, a)));
        }
        out
    }

    /// Outward normals at both ends of segment i (material on the left).
    pub fn normals(&self, i: usize) -> (V2, V2) {
        let (a, b) = (self.pts[i], self.pts[(i + 1) % self.pts.len()]);
        let f = safe_norm2(perp_right(b - a), V2::X);
        match self.seg_rad[i] {
            None => (f, f),
            Some((ra, rb)) => { let s = if (ra + rb).dot(f) >= 0.0 { 1.0 } else { -1.0 }; (ra * s, rb * s) }
        }
    }

    pub fn resampled(&self, count: usize) -> Vec<V2> {
        let n = self.pts.len();
        let mut lens = vec![0.0];
        for i in 0..n { let l = lens[i] + (self.pts[(i + 1) % n] - self.pts[i]).length(); lens.push(l); }
        let total = lens[n];
        let mut seg = 0;
        (0..count)
            .map(|k| {
                let s = total * k as f64 / count as f64;
                while seg < n - 1 && lens[seg + 1] < s { seg += 1; }
                let l = lens[seg + 1] - lens[seg];
                let t = if l > 0.0 { (s - lens[seg]) / l } else { 0.0 };
                self.pts[seg] + (self.pts[(seg + 1) % n] - self.pts[seg]) * t
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct ProfileRegion {
    pub outer: ProfileLoop,
    pub holes: Vec<ProfileLoop>,
}

impl Sketch {
    pub fn is_empty(&self) -> bool { self.entities.is_empty() }

    pub fn new_id(&mut self) -> i64 { self.next_id += 1; self.next_id - 1 }

    pub fn point(&self, id: i64) -> V2 {
        if id == 0 { return V2::ZERO; }
        self.points.iter().find(|p| p.id == id).map(|p| p.p).unwrap_or(V2::ZERO)
    }

    pub fn set_point(&mut self, id: i64, p: V2) {
        if let Some(q) = self.points.iter_mut().find(|q| q.id == id) { q.p = p; }
    }

    pub fn entity(&self, id: i64) -> Option<&SkEntity> { self.entities.iter().find(|e| e.id == id) }

    pub fn add_point(&mut self, p: V2) -> i64 {
        let id = self.new_id();
        self.points.push(SkPoint { id, p });
        id
    }

    pub fn add_entity(&mut self, kind: SkKind, pts: Vec<i64>, r: f64, construction: bool, centerline: bool) -> i64 {
        let id = self.new_id();
        self.entities.push(SkEntity { id, kind, pts, r, construction, centerline, text: None, ref_name: String::new() });
        id
    }

    pub fn add_line(&mut self, a: i64, b: i64) -> i64 { self.add_entity(SkKind::Line, vec![a, b], 0.0, false, false) }
    pub fn add_circle(&mut self, c: i64, r: f64) -> i64 { self.add_entity(SkKind::Circle, vec![c], r, false, false) }
    pub fn add_arc(&mut self, c: i64, s: i64, e: i64) -> i64 { self.add_entity(SkKind::Arc, vec![c, s, e], 0.0, false, false) }

    pub fn add_constraint(&mut self, mut c: SkConstraint) -> i64 {
        c.id = self.new_id();
        let id = c.id;
        self.constraints.push(c);
        id
    }

    pub fn line_ends(&self, id: i64) -> Option<(V2, V2)> {
        if id == REF_H { return Some((V2::ZERO, V2::X)); }
        if id == REF_V { return Some((V2::ZERO, V2::Y)); }
        if id <= -10 { return self.ref_lines.get((-10 - id) as usize).map(|l| (l[0], l[1])); }
        let e = self.entity(id)?;
        if e.kind != SkKind::Line { return None; }
        Some((self.point(e.pts[0]), self.point(e.pts[1])))
    }

    pub fn radius(&self, e: &SkEntity) -> f64 {
        match e.kind {
            SkKind::Circle | SkKind::Ellipse => e.r,
            SkKind::Arc => (self.point(e.pts[1]) - self.point(e.pts[0])).length(),
            _ => 0.0,
        }
    }

    pub fn arc_angles(&self, e: &SkEntity) -> (f64, f64) {
        let (c, s, en) = (self.point(e.pts[0]), self.point(e.pts[1]), self.point(e.pts[2]));
        let a0 = (s.y - c.y).atan2(s.x - c.x);
        let mut a1 = (en.y - c.y).atan2(en.x - c.x);
        while a1 <= a0 + 1e-12 { a1 += std::f64::consts::TAU; }
        (a0, a1)
    }

    pub fn delete_entity(&mut self, id: i64) {
        let Some(e) = self.entity(id).cloned() else { return };
        self.entities.retain(|x| x.id != id);
        self.constraints.retain(|c| !c.ents.contains(&id));
        for p in e.pts {
            if p == 0 { continue; }
            if !self.entities.iter().any(|x| x.pts.contains(&p)) {
                self.points.retain(|q| q.id != p);
                self.constraints.retain(|c| !c.pts.contains(&p));
            }
        }
    }

    pub fn merge_point(&mut self, from: i64, into: i64) {
        if from == into { return; }
        for e in &mut self.entities { for p in &mut e.pts { if *p == from { *p = into; } } }
        for c in &mut self.constraints { for p in &mut c.pts { if *p == from { *p = into; } } }
        self.points.retain(|q| q.id != from);
        self.constraints.retain(|c| !(c.ty == SkCType::Coincident && c.pts.len() == 2 && c.pts[0] == c.pts[1]));
    }

    /// Current measured value of a dimension.
    pub fn measure(&self, c: &SkConstraint) -> f64 {
        match c.ty {
            SkCType::Distance => (self.point(c.pts[1]) - self.point(c.pts[0])).length(),
            SkCType::HDistance => (self.point(c.pts[1]).x - self.point(c.pts[0]).x).abs(),
            SkCType::VDistance => (self.point(c.pts[1]).y - self.point(c.pts[0]).y).abs(),
            SkCType::PointLine => {
                let Some((a, b)) = self.line_ends(c.ents[0]) else { return 0.0 };
                cross2(b - a, self.point(c.pts[0]) - a).abs() / (b - a).length().max(1e-12)
            }
            SkCType::Radius => self.entity(c.ents[0]).map(|e| self.radius(e)).unwrap_or(0.0),
            SkCType::Diameter => self.entity(c.ents[0]).map(|e| 2.0 * self.radius(e)).unwrap_or(0.0),
            SkCType::Rho => self.entity(c.ents[0]).map(|e| e.r).unwrap_or(0.0),
            SkCType::Perimeter => c.ents.iter().filter_map(|id| self.entity(*id)).map(|e| self.length(e)).sum(),
            SkCType::Baseline => 0.0,
            SkCType::Ordinate => {
                let Some(b) = self.constraints.iter().find(|k| k.id == c.base) else { return 0.0 };
                ordinate(self.point(c.pts[0]), b, |id| self.line_ends(id), |id| self.point(id))
            }
            SkCType::Angle => {
                let (Some((a1, b1)), Some((a2, b2))) = (self.line_ends(c.ents[0]), self.line_ends(c.ents[1])) else { return 0.0 };
                let d1 = b1 - a1;
                let d2 = if c.aux != 0.0 { a2 - b2 } else { b2 - a2 };
                cross2(d1, d2).abs().atan2(d1.dot(d2)).to_degrees()
            }
            _ => 0.0,
        }
    }

    /// Re-evaluate dimension expressions; returns whether any value changed.
    pub fn apply_expressions(&mut self, vars: &crate::model::expr::Vars) -> Result<bool, String> {
        let mut changed = false;
        for c in &mut self.constraints {
            let Some(e) = &c.expr else { continue };
            if e.trim().is_empty() { continue; }
            let v = crate::model::expr::eval(e, vars)?;
            if (v - c.value).abs() > 1e-12 { c.value = v; changed = true; }
        }
        Ok(changed)
    }

    pub fn sample(&self, e: &SkEntity, per_circle: usize) -> Vec<V2> {
        match e.kind {
            SkKind::Line => vec![self.point(e.pts[0]), self.point(e.pts[1])],
            SkKind::Point => vec![self.point(e.pts[0])],
            SkKind::Circle => {
                let c = self.point(e.pts[0]);
                (0..=per_circle).map(|i| { let t = i as f64 / per_circle as f64 * std::f64::consts::TAU; c + V2::new(t.cos(), t.sin()) * e.r }).collect()
            }
            SkKind::Spline => {
                let pts: Vec<V2> = e.pts.iter().map(|p| self.point(*p)).collect();
                let closed = e.pts.len() > 2 && e.pts.first() == e.pts.last();
                catmull_rom(&pts, 12, closed)
            }
            SkKind::Ellipse => {
                let (c, a) = (self.point(e.pts[0]), self.point(e.pts[1]));
                let rx = (a - c).length();
                let u = safe_norm2(a - c, V2::X);
                let v = perp_left(u);
                (0..=per_circle).map(|i| { let t = i as f64 / per_circle as f64 * std::f64::consts::TAU; c + u * (rx * t.cos()) + v * (e.r * t.sin()) }).collect()
            }
            SkKind::Conic => {
                let (p0, p2, c) = (self.point(e.pts[0]), self.point(e.pts[1]), self.point(e.pts[2]));
                conic_points(p0, c, p2, e.r, per_circle.max(16) / 2)
            }
            SkKind::Corner => {
                let (p0, p1, c) = (self.point(e.pts[0]), self.point(e.pts[1]), self.point(e.pts[2]));
                let (v1, v2) = (p0 - c, p1 - c);
                let half = safe_norm2(v1, V2::X).dot(safe_norm2(v2, V2::Y)).clamp(-1.0, 1.0).acos() / 2.0;
                // the shape is given for the tangent points of the circular round; here the curve's own ends are fixed
                let ab = crate::kernel::corner::CornerShape::from_code(e.r).points(half, per_circle.clamp(16, 48));
                let lead = ab[0].0.max(1e-9);
                ab.into_iter().map(|(a, b)| c + v1 * (a / lead) + v2 * (b / lead)).collect()
            }
            SkKind::Text => self.text_loops(e).into_iter().flatten().collect(),
            SkKind::Csys => vec![self.point(e.pts[0])],
            SkKind::Arc => {
                let c = self.point(e.pts[0]);
                let r = self.radius(e);
                let (a0, a1) = self.arc_angles(e);
                let n = (((a1 - a0) / std::f64::consts::TAU * per_circle as f64).ceil() as usize).max(2);
                let mut out: Vec<V2> = (0..=n).map(|i| { let t = a0 + (a1 - a0) * i as f64 / n as f64; c + V2::new(t.cos(), t.sin()) * r }).collect();
                out[0] = self.point(e.pts[1]);
                out[n] = self.point(e.pts[2]);
                out
            }
        }
    }

    pub fn bounds(&self) -> Option<(V2, V2)> {
        let mut mn = V2::splat(f64::INFINITY);
        let mut mx = V2::splat(f64::NEG_INFINITY);
        let mut any = false;
        for e in &self.entities { for p in self.sample(e, 32) { mn = mn.min(p); mx = mx.max(p); any = true; } }
        if any { Some((mn, mx)) } else { None }
    }

    pub fn profile_entities(&self) -> Vec<&SkEntity> {
        self.entities.iter().filter(|e| !e.construction && !e.centerline && !matches!(e.kind, SkKind::Point | SkKind::Csys)).collect()
    }

    /// Closed outlines of a text entity in sketch coordinates.
    pub fn text_loops(&self, e: &SkEntity) -> Vec<Vec<V2>> {
        let Some(t) = &e.text else { return vec![] };
        let (a, b) = (self.point(e.pts[0]), self.point(e.pts[1]));
        crate::sketch::text::outline_family(&t.text, &t.family, t.font, t.aspect, t.slant, a, b)
    }

    /// Length of an entity (lines, arcs, circles exact; other curves sampled).
    pub fn length(&self, e: &SkEntity) -> f64 {
        match e.kind {
            SkKind::Line => (self.point(e.pts[1]) - self.point(e.pts[0])).length(),
            SkKind::Circle => std::f64::consts::TAU * e.r,
            SkKind::Arc => { let (a0, a1) = self.arc_angles(e); let mut s = a1 - a0; if s < 0.0 { s += std::f64::consts::TAU; } self.radius(e) * s }
            SkKind::Point | SkKind::Csys | SkKind::Text => 0.0,
            _ => self.sample(e, 128).windows(2).map(|w| (w[1] - w[0]).length()).sum(),
        }
    }

    /// Shoulder point of a conic (on the curve, half way): (1-ρ)·M + ρ·C.
    pub fn conic_shoulder(&self, e: &SkEntity) -> V2 {
        let m = (self.point(e.pts[0]) + self.point(e.pts[1])) * 0.5;
        m + (self.point(e.pts[2]) - m) * e.r
    }

    fn canonical(&self) -> HashMap<i64, i64> {
        let mut parent: HashMap<i64, i64> = HashMap::new();
        fn find(p: &HashMap<i64, i64>, mut x: i64) -> i64 { while let Some(&q) = p.get(&x) { if q == x { break; } x = q; } x }
        for c in &self.constraints {
            if c.ty == SkCType::Coincident && c.pts.len() == 2 {
                let (a, b) = (find(&parent, c.pts[0]), find(&parent, c.pts[1]));
                if a != b { parent.insert(a, b); }
            }
        }
        self.points.iter().map(|p| (p.id, find(&parent, p.id))).collect()
    }

    /// Endpoints not connected to exactly one other entity.
    pub fn open_ends(&self) -> Vec<V2> {
        let canon = self.canonical();
        let c = |p: i64| *canon.get(&p).unwrap_or(&p);
        let mut deg: HashMap<i64, i32> = HashMap::new();
        for e in self.profile_entities() {
            match e.kind {
                SkKind::Line => { *deg.entry(c(e.pts[0])).or_default() += 1; *deg.entry(c(e.pts[1])).or_default() += 1; }
                SkKind::Arc => { *deg.entry(c(e.pts[1])).or_default() += 1; *deg.entry(c(e.pts[2])).or_default() += 1; }
                SkKind::Spline => { *deg.entry(c(e.pts[0])).or_default() += 1; *deg.entry(c(*e.pts.last().unwrap())).or_default() += 1; }
                SkKind::Conic | SkKind::Corner => { *deg.entry(c(e.pts[0])).or_default() += 1; *deg.entry(c(e.pts[1])).or_default() += 1; }
                _ => {}
            }
        }
        deg.into_iter().filter(|(_, d)| *d != 2).map(|(p, _)| self.point(p)).collect()
    }

    fn walk(&self, allow_open: bool) -> Result<Vec<(ProfileLoop, bool)>, String> {
        let canon = self.canonical();
        let cp = |p: i64| *canon.get(&p).unwrap_or(&p);
        struct Edge<'a> { a: i64, b: i64, e: &'a SkEntity }
        let mut edges: Vec<Edge> = Vec::new();
        let mut result = Vec::new();
        for e in self.profile_entities() {
            match e.kind {
                SkKind::Line => edges.push(Edge { a: cp(e.pts[0]), b: cp(e.pts[1]), e }),
                SkKind::Arc => edges.push(Edge { a: cp(e.pts[1]), b: cp(e.pts[2]), e }),
                SkKind::Spline => edges.push(Edge { a: cp(e.pts[0]), b: cp(*e.pts.last().unwrap()), e }),
                SkKind::Conic | SkKind::Corner => edges.push(Edge { a: cp(e.pts[0]), b: cp(e.pts[1]), e }),
                SkKind::Text => {
                    for mut s in self.text_loops(e) {
                        if s.len() > 1 && (s[0] - s[s.len() - 1]).length() < 1e-12 { s.pop(); }
                        if s.len() < 3 { continue; }
                        let n = s.len();
                        result.push((ProfileLoop { pts: s, seg_ent: vec![e.id; n], seg_rad: vec![None; n] }, true));
                    }
                }
                SkKind::Ellipse => {
                    let mut s = self.sample(e, 72);
                    s.pop();
                    let n = s.len();
                    let nrm = |i: usize| safe_norm2(perp_right(s[(i + 1) % n] - s[(i + n - 1) % n]), V2::X);
                    let rad = (0..n).map(|i| Some((nrm(i), nrm((i + 1) % n)))).collect();
                    result.push((ProfileLoop { pts: s.clone(), seg_ent: vec![e.id; n], seg_rad: rad }, true));
                }
                SkKind::Circle => {
                    let mut s = self.sample(e, 64);
                    s.pop();
                    let c = self.point(e.pts[0]);
                    let n = s.len();
                    let rad = (0..n).map(|i| Some((safe_norm2(s[i] - c, V2::X), safe_norm2(s[(i + 1) % n] - c, V2::X)))).collect();
                    result.push((ProfileLoop { pts: s, seg_ent: vec![e.id; n], seg_rad: rad }, true));
                }
                _ => {}
            }
        }
        let mut adj: HashMap<i64, Vec<usize>> = HashMap::new();
        for (i, ed) in edges.iter().enumerate() { adj.entry(ed.a).or_default().push(i); adj.entry(ed.b).or_default().push(i); }
        if let Some((p, _)) = adj.iter().find(|(_, l)| l.len() > 2) {
            let q = self.point(*p);
            return Err(format!("Mehr als zwei Elemente treffen sich bei ({}, {}).", fmt_num(q.x, 3), fmt_num(q.y, 3)));
        }
        let mut open: Vec<i64> = adj.iter().filter(|(_, l)| l.len() == 1).map(|(k, _)| *k).collect();
        open.sort();
        if !open.is_empty() && !allow_open {
            return Err(format!("Die Skizze ist nicht geschlossen: {} offene(s) Ende(n). Geschlossene Schleifen sind erforderlich.", open.len()));
        }
        let mut used = vec![false; edges.len()];
        let mut keys: Vec<i64> = adj.keys().copied().collect();
        keys.sort();
        let starts: Vec<i64> = open.iter().copied().chain(keys).collect();
        for sv in starts {
            let Some(first) = adj[&sv].iter().copied().find(|&i| !used[i]) else { continue };
            let mut lp = ProfileLoop { pts: vec![], seg_ent: vec![], seg_rad: vec![] };
            let (mut v, mut ei, mut closed) = (sv, first, false);
            loop {
                used[ei] = true;
                let ed = &edges[ei];
                let fwd = ed.a == v;
                let mut s = self.sample(ed.e, 64);
                if !fwd { s.reverse(); }
                let c = if ed.e.kind == SkKind::Arc { Some(self.point(ed.e.pts[0])) } else { None };
                let spline = matches!(ed.e.kind, SkKind::Spline | SkKind::Conic | SkKind::Corner);
                let m = s.len();
                let tn = |i: usize| safe_norm2(perp_right(s[(i + 1).min(m - 1)] - s[i.saturating_sub(1)]), V2::X);
                for k in 0..m - 1 {
                    lp.pts.push(s[k]);
                    lp.seg_ent.push(ed.e.id);
                    if spline { lp.seg_rad.push(Some((tn(k), tn(k + 1)))); }
                    else { lp.seg_rad.push(c.map(|c| (safe_norm2(s[k] - c, V2::X), safe_norm2(s[k + 1] - c, V2::X)))); }
                }
                v = if fwd { ed.b } else { ed.a };
                if v == sv { closed = true; break; }
                match adj[&v].iter().copied().find(|&i| !used[i]) { Some(n) => ei = n, None => break }
            }
            if !closed { lp.pts.push(self.point(v)); }
            result.push((lp, closed));
        }
        Ok(result)
    }

    pub fn profile_regions(&self) -> Result<Vec<ProfileRegion>, String> {
        let loops: Vec<ProfileLoop> = self.walk(false)?.into_iter().map(|l| l.0).filter(|l| l.pts.len() >= 3 && l.area().abs() > 1e-9).collect();
        if loops.is_empty() { return Err("Die Skizze enthält keine geschlossenen Schleifen.".into()); }
        let mut order: Vec<usize> = (0..loops.len()).collect();
        order.sort_by(|&a, &b| loops[b].area().abs().partial_cmp(&loops[a].area().abs()).unwrap());
        let mut container = vec![usize::MAX; loops.len()];
        let mut depth = vec![0usize; loops.len()];
        for (oi, &i) in order.iter().enumerate() {
            let probe = loops[i].pts[0];
            for &j in order[..oi].iter().rev() {
                if point_in_polygon2(probe, &loops[j].pts) { container[i] = j; depth[i] = depth[j] + 1; break; }
            }
        }
        let mut regions: HashMap<usize, ProfileRegion> = HashMap::new();
        for &i in &order {
            if depth[i] % 2 == 0 {
                let l = if loops[i].area() < 0.0 { loops[i].reversed() } else { loops[i].clone() };
                regions.insert(i, ProfileRegion { outer: l, holes: vec![] });
            }
        }
        for &i in &order {
            if depth[i] % 2 == 1 {
                let l = if loops[i].area() > 0.0 { loops[i].reversed() } else { loops[i].clone() };
                if let Some(r) = regions.get_mut(&container[i]) { r.holes.push(l); }
            }
        }
        Ok(order.iter().filter_map(|i| regions.remove(i)).collect())
    }

    /// A single connected chain for sweep trajectories.
    pub fn trajectory(&self) -> Result<(Vec<V2>, bool), String> {
        let chains = self.walk(true)?;
        if chains.len() != 1 {
            return Err(if chains.is_empty() { "Die Leitkurvenskizze ist leer.".into() } else { "Die Leitkurve muss eine zusammenhängende Kette sein.".into() });
        }
        let (l, c) = chains.into_iter().next().unwrap();
        Ok((l.pts, c))
    }

    pub fn centerline_axis(&self) -> Option<(V2, V2)> {
        let e = self.entities.iter().find(|e| e.centerline && e.kind == SkKind::Line)?;
        let (a, b) = (self.point(e.pts[0]), self.point(e.pts[1]));
        Some((a, safe_norm2(b - a, V2::Y)))
    }

    /// Candidate weak dimensions in priority order (sizes, lengths, angles, positions to references).
    /// `off` is the default label offset in sketch units.
    pub fn weak_candidates(&self, off: f64) -> Vec<SkConstraint> {
        let mut out = vec![];
        let (bmin, bmax) = self.bounds().unwrap_or((V2::splat(-1.0), V2::splat(1.0)));
        let centroid = (bmin + bmax) * 0.5;
        let mk = |ty, pts: Vec<i64>, ents: Vec<i64>, label: V2| { let mut c = SkConstraint::new(ty, pts, ents); c.label = label; c };
        for e in &self.entities {
            match e.kind {
                SkKind::Circle => out.push(mk(SkCType::Diameter, vec![], vec![e.id], V2::new(0.7071, 0.7071) * (e.r + off))),
                SkKind::Ellipse => {
                    let (c, a) = (self.point(e.pts[0]), self.point(e.pts[1]));
                    let u = safe_norm2(a - c, V2::X);
                    out.push(mk(SkCType::Distance, e.pts.clone(), vec![], perp_left(u) * (-off)));
                    out.push(mk(SkCType::Radius, vec![], vec![e.id], perp_left(u) * (e.r + off)));
                }
                SkKind::Arc => {
                    let (a0, a1) = self.arc_angles(e);
                    let m = (a0 + a1) * 0.5;
                    out.push(mk(SkCType::Radius, vec![], vec![e.id], V2::new(m.cos(), m.sin()) * (self.radius(e) + off)));
                }
                SkKind::Conic => out.push(mk(SkCType::Rho, vec![], vec![e.id], V2::new(off, off))),
                _ => {}
            }
        }
        let lines: Vec<&SkEntity> = self.entities.iter().filter(|e| e.kind == SkKind::Line && !e.centerline).collect();
        for e in &lines {
            let (a, b) = (self.point(e.pts[0]), self.point(e.pts[1]));
            let mid = (a + b) * 0.5;
            let mut n = safe_norm2(perp_left(b - a), V2::Y);
            if n.dot(mid - centroid) < 0.0 { n = -n; }
            out.push(mk(SkCType::Distance, e.pts.clone(), vec![e.id], n * off * 1.3));
        }
        for e in &lines {
            let (a, b) = (self.point(e.pts[0]), self.point(e.pts[1]));
            let d = b - a;
            let len = d.length();
            if len < 1e-9 || d.x.abs() < 1e-6 * len || d.y.abs() < 1e-6 * len { continue; }
            let du = d / len;
            let nb = lines.iter().find(|o| {
                if o.id == e.id || !(o.pts.contains(&e.pts[0]) || o.pts.contains(&e.pts[1])) { return false; }
                let od = self.point(o.pts[1]) - self.point(o.pts[0]);
                cross2(du, safe_norm2(od, V2::X)).abs() > 1e-3
            });
            let fallback = if !self.axes_off[0] { Some(REF_H) } else if !self.axes_off[1] { Some(REF_V) } else if self.ref_lines.is_empty() { None } else { Some(-10) };
            let Some(other) = nb.map(|o| o.id).or(fallback) else { continue };
            let mut c = mk(SkCType::Angle, vec![], vec![e.id, other], V2::ZERO);
            if self.measure(&c) > 90.0 { c.aux = 1.0; }
            let (oa, ob) = self.line_ends(other).unwrap_or((V2::ZERO, V2::X));
            let mut d2 = safe_norm2(ob - oa, V2::X);
            if c.aux != 0.0 { d2 = -d2; }
            c.label = safe_norm2(du + d2, du) * off * 2.4;
            out.push(c);
        }
        let mut pts: Vec<(i64, V2)> = vec![];
        for e in &self.entities {
            for &p in &e.pts { if p != 0 && !pts.iter().any(|q| q.0 == p) { pts.push((p, self.point(p))); } }
        }
        pts.sort_by(|a, b| a.1.length().partial_cmp(&b.1.length()).unwrap_or(std::cmp::Ordering::Equal));
        let ref_pts: Vec<i64> = self.entities.iter().filter(|e| !e.ref_name.is_empty()).map(|e| e.pts[0]).collect();
        for (id, p) in pts {
            if ref_pts.contains(&id) { continue; }
            if p.x.abs() > 1e-7 && !self.axes_off[1] { out.push(mk(SkCType::PointLine, vec![id], vec![REF_V], V2::new(0.0, -off * 1.2))); }
            if p.y.abs() > 1e-7 && !self.axes_off[0] { out.push(mk(SkCType::PointLine, vec![id], vec![REF_H], V2::new(-off * 1.2, 0.0))); }
            if self.axes_off[0] || self.axes_off[1] {
                // without the default references, position against the remaining ones
                for (i, l) in self.ref_lines.iter().enumerate() {
                    if cross2(l[1] - l[0], p - l[0]).abs() > 1e-7 * (l[1] - l[0]).length() { out.push(mk(SkCType::PointLine, vec![id], vec![-10 - i as i64], V2::new(off, off))); }
                }
                for &r in &ref_pts {
                    let q = self.point(r);
                    if (p.x - q.x).abs() > 1e-7 { out.push(mk(SkCType::HDistance, vec![r, id], vec![], V2::new(0.0, -off * 1.2))); }
                    if (p.y - q.y).abs() > 1e-7 { out.push(mk(SkCType::VDistance, vec![r, id], vec![], V2::new(-off * 1.2, 0.0))); }
                }
            }
        }
        out
    }

    pub fn rect(w: f64, h: f64, cx: f64, cy: f64) -> Sketch {
        let mut s = Sketch::default();
        let a = s.add_point(V2::new(cx - w / 2.0, cy - h / 2.0));
        let b = s.add_point(V2::new(cx + w / 2.0, cy - h / 2.0));
        let c = s.add_point(V2::new(cx + w / 2.0, cy + h / 2.0));
        let d = s.add_point(V2::new(cx - w / 2.0, cy + h / 2.0));
        let l1 = s.add_line(a, b);
        let l2 = s.add_line(b, c);
        let l3 = s.add_line(c, d);
        let l4 = s.add_line(d, a);
        for (l, t) in [(l1, SkCType::Horizontal), (l2, SkCType::Vertical), (l3, SkCType::Horizontal), (l4, SkCType::Vertical)] {
            s.add_constraint(SkConstraint::new(t, vec![], vec![l]));
        }
        s
    }

    pub fn circle(r: f64, cx: f64, cy: f64) -> Sketch {
        let mut s = Sketch::default();
        let c = s.add_point(V2::new(cx, cy));
        s.add_circle(c, r);
        s
    }
}

/// Centripetal Catmull-Rom spline through `p` with `per_seg` samples per span.
pub fn catmull_rom(p: &[V2], per_seg: usize, closed: bool) -> Vec<V2> {
    let mut pts: Vec<V2> = p.to_vec();
    if closed && pts.len() > 2 { pts.pop(); }
    let n = pts.len();
    if n < 2 { return pts; }
    if n == 2 && !closed { return pts; }
    let get = |i: isize| -> V2 {
        if closed { pts[i.rem_euclid(n as isize) as usize] }
        else if i < 0 { pts[0] * 2.0 - pts[1] }
        else if i as usize >= n { pts[n - 1] * 2.0 - pts[n - 2] }
        else { pts[i as usize] }
    };
    let spans = if closed { n } else { n - 1 };
    let mut out = vec![pts[0]];
    for i in 0..spans as isize {
        let (p0, p1, p2, p3) = (get(i - 1), get(i), get(i + 1), get(i + 2));
        let t0 = 0.0;
        let t1 = t0 + (p1 - p0).length().sqrt().max(1e-9);
        let t2 = t1 + (p2 - p1).length().sqrt().max(1e-9);
        let t3 = t2 + (p3 - p2).length().sqrt().max(1e-9);
        for k in 1..=per_seg {
            let t = t1 + (t2 - t1) * k as f64 / per_seg as f64;
            let a1 = p0 * ((t1 - t) / (t1 - t0)) + p1 * ((t - t0) / (t1 - t0));
            let a2 = p1 * ((t2 - t) / (t2 - t1)) + p2 * ((t - t1) / (t2 - t1));
            let a3 = p2 * ((t3 - t) / (t3 - t2)) + p3 * ((t - t2) / (t3 - t2));
            let b1 = a1 * ((t2 - t) / (t2 - t0)) + a2 * ((t - t0) / (t2 - t0));
            let b2 = a2 * ((t3 - t) / (t3 - t1)) + a3 * ((t - t1) / (t3 - t1));
            out.push(b1 * ((t2 - t) / (t2 - t1)) + b2 * ((t - t1) / (t2 - t1)));
        }
    }
    out
}

/// Rational quadratic Bézier from `p0` to `p2` with control point `c` and conic parameter
/// rho (weight w = rho / (1 - rho)): 0.5 parabola, < 0.5 ellipse, > 0.5 hyperbola.
pub fn conic_points(p0: V2, c: V2, p2: V2, rho: f64, n: usize) -> Vec<V2> {
    let rho = rho.clamp(0.05, 0.95);
    let w = rho / (1.0 - rho);
    (0..=n).map(|i| {
        let t = i as f64 / n as f64;
        let (b0, b1, b2) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t) * w, t * t);
        (p0 * b0 + c * b1 + p2 * b2) / (b0 + b1 + b2)
    }).collect()
}

/// Distance of `q` from a baseline constraint (line baseline: perpendicular distance; point
/// baseline: along x for aux 0, along y for aux 1).
pub fn ordinate(q: V2, b: &SkConstraint, line: impl Fn(i64) -> Option<(V2, V2)>, point: impl Fn(i64) -> V2) -> f64 {
    if let Some(&l) = b.ents.first() {
        let Some((a, e)) = line(l) else { return 0.0 };
        return cross2(e - a, q - a).abs() / (e - a).length().max(1e-12);
    }
    let Some(&p) = b.pts.first() else { return 0.0 };
    let o = point(p);
    if b.aux == 0.0 { (q.x - o.x).abs() } else { (q.y - o.y).abs() }
}
