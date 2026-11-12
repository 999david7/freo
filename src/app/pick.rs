//! 3D picking: ray casting against faces, screen-space proximity for edges, axes and points.

use super::*;
use crate::model::regen::{AxisInfo, PlaneInfo, RegenResult};

/// Display rectangle of a datum plane, sized from the model (or from its own size).
pub fn plane_quad(p: &PlaneInfo, r: &RegenResult) -> [V3; 4] {
    let frame = &p.frame;
    let b = &r.topo.bbox;
    let (c, s) = if p.size > 0.0 { (frame.origin, p.size) } else if b.is_empty() { (V3::ZERO, 60.0) } else { (b.center(), (b.diagonal() * 0.42).max(30.0)) };
    let local = frame.to_local(c);
    let o = frame.to_world(local);
    [o - frame.x * s - frame.y * s, o + frame.x * s - frame.y * s, o + frame.x * s + frame.y * s, o - frame.x * s + frame.y * s]
}

pub fn axis_segment(a: &AxisInfo, r: &RegenResult) -> (V3, V3) {
    let b = &r.topo.bbox;
    let (c, s) = if b.is_empty() { (V3::ZERO, 50.0) } else { (b.center(), (b.diagonal() * 0.45).max(20.0)) };
    let p = a.point + a.dir * (c - a.point).dot(a.dir);
    (p - a.dir * s, p + a.dir * s)
}

fn ray_tri(o: V3, d: V3, a: V3, b: V3, c: V3) -> Option<f64> {
    let e1 = b - a;
    let e2 = c - a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 { return None; }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) { return None; }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 { return None; }
    let t = e2.dot(q) * inv;
    if t > 0.0 { Some(t) } else { None }
}

fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-9 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) };
    let (cx, cy) = (a.0 + dx * t, a.1 + dy * t);
    (((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt(), t)
}

impl App {
    pub fn section_plane(&self) -> Option<(V3, f64)> {
        let s = self.section?;
        let f = self.displayed().plane(s.plane)?.frame;
        let n = if s.flip { -f.z() } else { f.z() };
        Some((n, n.dot(f.origin) + s.offset * if s.flip { -1.0 } else { 1.0 }))
    }

    pub fn pick_at(&self, px: (f32, f32)) -> Option<Pick> {
        let size = self.viewport_size;
        let r = self.displayed();
        let target = self.editor.as_ref().and_then(|e| e.pick);
        let (want_face, want_edge, want_plane, want_axis, want_point) = match target {
            Some(PickTarget::SketchPlane) | Some(PickTarget::Plane) | Some(PickTarget::DepthRef) => (true, false, true, false, false),
            Some(PickTarget::Plane2) => (true, false, true, true, false),
            Some(PickTarget::Edges) => (false, true, false, false, false),
            Some(PickTarget::Faces) | Some(PickTarget::Surface) => (true, false, false, false, false),
            Some(PickTarget::Points) => (false, false, false, false, true),
            Some(PickTarget::Axis) => (false, false, false, true, false),
            Some(PickTarget::Direction) => (false, false, true, true, false),
            Some(PickTarget::Features) => (true, true, false, false, false),
            None if self.sim_picking() => (true, false, false, false, false),
            None => match self.filter {
                Filter::Smart | Filter::Feature => (true, true, true, true, true),
                Filter::Surface => (true, false, false, false, false),
                Filter::Edge => (false, true, false, false, false),
                Filter::Datum => (false, false, true, true, true),
            },
        };
        let (o, d) = self.camera.ray(px, size);
        let clip = self.section_plane();
        let t = &r.topo;
        // faces
        let mut best_t = f64::INFINITY;
        let mut best_face = None;
        for (pi, p) in t.polys.iter().enumerate() {
            for k in 1..p.len() - 1 {
                if let Some(tt) = ray_tri(o, d, t.positions[p[0]], t.positions[p[k]], t.positions[p[k + 1]]) {
                    if tt < best_t {
                        let hit = o + d * tt;
                        if let Some((n, w)) = clip { if hit.dot(n) - w > 0.0 { continue; } }
                        best_t = tt;
                        best_face = Some(t.poly_face[pi]);
                    }
                }
            }
        }
        let hit_depth = if best_face.is_some() { (o + d * best_t - self.camera.eye()).dot(-self.camera.back()) } else { f64::INFINITY };
        let tol_depth = self.camera.scale * 0.02;
        // Profil/Drehen: the curves of an existing sketch select that sketch
        let takes_sketch = target == Some(PickTarget::SketchPlane) && self.sketch_dlg.is_none() && self.editor.as_ref().is_some_and(|e| e.feature.kind.accepts_sketch());
        if takes_sketch && self.show_curves {
            let mut best = (7.0f32, None);
            for c in r.curves.iter().filter(|c| matches!(self.data.feature(c.feature).map(|f| &f.kind), Some(FeatureKind::Sketch(_)))) {
                for line in &c.polylines {
                    for w in line.windows(2) {
                        let (Some(pa), Some(pb)) = (self.camera.project(w[0], size), self.camera.project(w[1], size)) else { continue };
                        let (dist, tt) = seg_dist(px, (pa.0, pa.1), (pb.0, pb.1));
                        if dist < best.0 && pa.2 + (pb.2 - pa.2) * tt as f64 <= hit_depth + tol_depth { best = (dist, Some(c.feature)); }
                    }
                }
            }
            if let Some(id) = best.1 { return Some(Pick::Feature(id)); }
        }
        // edges
        if want_edge {
            let mut best = (7.0f32, None);
            for (ci, c) in t.chains.iter().enumerate() {
                let n = c.points.len();
                let segs = if c.closed { n } else { n - 1 };
                for i in 0..segs {
                    let (a, b) = (c.points[i], c.points[(i + 1) % n]);
                    let (Some(pa), Some(pb)) = (self.camera.project(a, size), self.camera.project(b, size)) else { continue };
                    let (dist, tt) = seg_dist(px, (pa.0, pa.1), (pb.0, pb.1));
                    let depth = pa.2 + (pb.2 - pa.2) * tt as f64;
                    if dist < best.0 && depth <= hit_depth + tol_depth { best = (dist, Some(ci)); }
                }
            }
            if let Some(ci) = best.1 { return Some(Pick::Edge(ci)); }
        }
        if want_point && self.show_points {
            for p in &r.points {
                if let Some(s) = self.camera.project(p.p, size) {
                    if ((s.0 - px.0).powi(2) + (s.1 - px.1).powi(2)).sqrt() < 7.0 { return Some(Pick::Point(p.id)); }
                }
            }
        }
        if want_axis && (self.show_axes || target.is_some()) {
            let mut best = (6.0f32, None);
            for a in &r.axes {
                let (p0, p1) = axis_segment(a, r);
                let (Some(s0), Some(s1)) = (self.camera.project(p0, size), self.camera.project(p1, size)) else { continue };
                let (dist, _) = seg_dist(px, (s0.0, s0.1), (s1.0, s1.1));
                if dist < best.0 { best = (dist, Some(a.id)); }
            }
            if let Some(id) = best.1 { return Some(Pick::Axis(id)); }
        }
        if want_face { if let Some(f) = best_face { return Some(Pick::Face(f)); } }
        if want_plane && (self.show_planes || target.is_some()) {
            // nearest datum plane hit inside its display rectangle (border preferred)
            let mut best: (f64, Option<u64>) = (f64::INFINITY, None);
            for p in &r.planes {
                if self.data.feature(p.id).map(|f| f.hidden).unwrap_or(false) && target.is_none() { continue; }
                let n = p.frame.z();
                let den = n.dot(d);
                if den.abs() < 1e-9 { continue; }
                let tt = (n.dot(p.frame.origin) - n.dot(o)) / den;
                if tt <= 0.0 { continue; }
                let q = plane_quad(p, r);
                let hit = o + d * tt;
                let l = p.frame.to_local(hit);
                let (l0, l2) = (p.frame.to_local(q[0]), p.frame.to_local(q[2]));
                if l.x >= l0.x && l.x <= l2.x && l.y >= l0.y && l.y <= l2.y && tt < best.0 { best = (tt, Some(p.id)); }
            }
            if let Some(id) = best.1 { return Some(Pick::Plane(id)); }
        }
        None
    }
}
