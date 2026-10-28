//! Edit Dimensions mode: a feature's sketch and feature dimensions drawn on the model in 3D
//! and modified in place (Creo's double-click behaviour).

use super::sketcher::{dim_text, draw_dim, STRONG, WEAKC};
use super::*;
use crate::model::regen::RegenResult;
use crate::sketch::{sketch::Sketch, solver};
use eframe::egui::{self, vec2, Color32, FontId, Pos2, Rect, Shape, Stroke};

#[derive(Clone, PartialEq, Debug)]
pub enum DimKey { Sketch(usize, i64, (crate::sketch::sketch::SkCType, Vec<i64>, Vec<i64>)), Field(&'static str) }

pub struct EditDims {
    pub feature: u64,
    pub ctx: RegenResult,
    pub labels: Vec<(Rect, DimKey, String)>,
    pub editing: Option<(DimKey, String, Pos2, bool)>,
}

struct FieldDim { key: &'static str, text: String, edit: String, line: Option<(V3, V3)>, label: V3 }

fn field_mut<'a>(k: &'a mut FeatureKind, key: &str) -> Option<&'a mut Dim> {
    Some(match (k, key) {
        (FeatureKind::Extrude(d), "depth") => &mut d.depth,
        (FeatureKind::Extrude(d), "depth2") => &mut d.depth2,
        (FeatureKind::Extrude(d), "thickness") => &mut d.thickness,
        (FeatureKind::Revolve(d), "angle") => &mut d.angle,
        (FeatureKind::Hole(d), "diameter") => &mut d.diameter,
        (FeatureKind::Hole(d), "depth") => &mut d.depth,
        (FeatureKind::Hole(d), "x") => &mut d.x,
        (FeatureKind::Hole(d), "y") => &mut d.y,
        (FeatureKind::Round(d), k) if k.starts_with("radius") => {
            let (end, i) = if let Some(r) = k.strip_prefix("radius_end") { (true, r) } else { (false, &k[6..]) };
            let set = d.sets.get_mut(i.parse::<usize>().ok()?)?;
            if end { &mut set.radius_end } else { &mut set.radius }
        }
        (FeatureKind::Chamfer(d), "d1") => &mut d.d1,
        (FeatureKind::Shell(d), "thickness") => &mut d.thickness,
        (FeatureKind::Draft(d), "angle") => &mut d.angle,
        (FeatureKind::Pattern(d), "count1") => &mut d.count1,
        (FeatureKind::Pattern(d), "spacing1") => &mut d.spacing1,
        (FeatureKind::Pattern(d), "spacing2") => &mut d.spacing2,
        (FeatureKind::Pattern(d), "axis_count") => &mut d.axis_count,
        (FeatureKind::Pattern(d), "axis_angle") => &mut d.axis_angle,
        (FeatureKind::DatumPlane(d), "offset") => &mut d.offset,
        (FeatureKind::Rib(d), "thickness") => &mut d.thickness,
        (FeatureKind::Helix(d), "pitch") => &mut d.pitch,
        (FeatureKind::Thread(d), "length") => &mut d.length,
        (FeatureKind::Thread(d), "diameter") => &mut d.diameter,
        (FeatureKind::DatumPlane(d), "angle") => &mut d.angle,
        (FeatureKind::Blend(d), k) if k.starts_with("offset") => {
            let i: usize = k[6..].parse().ok()?;
            d.offsets.get_mut(i)?
        }
        _ => return None,
    })
}

fn sketch_mut(k: &mut FeatureKind, i: usize) -> Option<&mut Sketch> {
    match (k, i) {
        (FeatureKind::Extrude(d), 0) => Some(&mut d.sketch),
        (FeatureKind::Revolve(d), 0) => Some(&mut d.sketch),
        (FeatureKind::Sketch(d), 0) => Some(&mut d.sketch),
        (FeatureKind::Sweep(d), 0) => Some(&mut d.trajectory),
        (FeatureKind::Rib(d), 0) => Some(&mut d.sketch),
        (FeatureKind::Helix(d), 0) => Some(&mut d.profile),
        (FeatureKind::Helix(d), 1) => Some(&mut d.section),
        (FeatureKind::Sweep(d), 1) => Some(&mut d.section),
        (FeatureKind::Blend(d), i) => d.sections.get_mut(i),
        _ => None,
    }
}

/// The sketch as regenerated (relations applied), with weak dimensions filled in for display.
pub(crate) fn prepared(s: &Sketch, vars: &crate::model::expr::Vars) -> Sketch {
    let mut s = s.clone();
    if s.apply_expressions(vars).unwrap_or(false) { solver::solve(&mut s, None); }
    let off = s.bounds().map(|(a, b)| (b - a).length() * 0.06).unwrap_or(5.0).max(1.0);
    solver::refresh_weak(&mut s, off);
    s
}

impl App {
    pub fn start_edit_dims(&mut self, fid: u64) {
        if self.busy() { return; }
        let Some(i) = self.data.index_of(fid) else { return };
        let f = &self.data.features[i];
        if matches!(f.kind, FeatureKind::Csys) { return; }
        let ctx = regen::run_features(&self.data, &self.data.features[..i], None);
        let name = f.name.clone();
        self.edit_dims = Some(EditDims { feature: fid, ctx, labels: vec![], editing: None });
        self.tree_selection = vec![fid];
        self.selection = vec![Pick::Feature(fid)];
        self.mini = None;
        self.say(format!("{}: einen Bemaßungswert doppelklicken, um ihn zu ändern.", name), MsgKind::Prompt);
    }

    fn feature_sketches(&self, f: &Feature, ctx: &RegenResult) -> Vec<(usize, Sketch, Frame)> {
        let vars = &self.result.vars;
        let mut out = vec![];
        match &f.kind {
            FeatureKind::Extrude(ExtrudeDef { plane, sketch, .. }) | FeatureKind::Revolve(RevolveDef { plane, sketch, .. }) | FeatureKind::Sketch(SketchDef { plane, sketch }) => {
                if let Some(fr) = plane.and_then(|p| ctx.frame_for(&p)) { out.push((0, prepared(sketch, vars), fr)); }
            }
            FeatureKind::Sweep(d) => {
                if let Some(fr) = d.plane.and_then(|p| ctx.frame_for(&p)) {
                    let t = prepared(&d.trajectory, vars);
                    if let Ok((pts, _)) = t.trajectory() {
                        let path: Vec<V3> = pts.iter().map(|p| fr.to_world(*p)).collect();
                        out.push((1, prepared(&d.section, vars), regen::sweep_section_frame(&path)));
                    }
                    out.push((0, t, fr));
                }
            }
            FeatureKind::Rib(d) => {
                if let Some(fr) = d.plane.and_then(|p| ctx.frame_for(&p)) { out.push((0, prepared(&d.sketch, vars), fr)); }
            }
            FeatureKind::Helix(d) => {
                if let Some(fr) = d.plane.and_then(|p| ctx.frame_for(&p)) {
                    out.push((0, prepared(&d.profile, vars), fr));
                    out.push((1, prepared(&d.section, vars), fr));
                }
            }
            FeatureKind::Blend(d) => {
                if let Some(fr) = d.plane.and_then(|p| ctx.frame_for(&p)) {
                    let offs: Vec<f64> = d.offsets.iter().map(|o| o.value(vars, 20.0)).collect();
                    let axis = d.sections.first().and_then(|s| s.centerline_axis());
                    let frames = match axis {
                        Some((ap, au)) if d.rotational => regen::rotational_blend_frames(&fr, ap, au, &offs, d.flip).0,
                        _ => regen::blend_frames(&fr, &offs, d.flip),
                    };
                    for (i, s) in d.sections.iter().enumerate() { if let Some(f2) = frames.get(i) { out.push((i, prepared(s, vars), *f2)); } }
                }
            }
            _ => {}
        }
        out
    }

    fn field_dims(&self, f: &Feature, ctx: &RegenResult) -> Vec<FieldDim> {
        let vars = &self.result.vars;
        let v = |d: &Dim| d.value(vars, 0.0);
        let shown = |d: &Dim, prefix: &str| {
            let val = fmt_num(d.value(vars, 0.0), 2);
            if d.0.trim().parse::<f64>().is_ok() { format!("{}{}", prefix, val) } else { format!("{} = {}{}", d.0.trim(), prefix, val) }
        };
        let b = &self.result.topo.bbox;
        let size = if b.is_empty() { 50.0 } else { b.diagonal() };
        let mut out = vec![];
        match &f.kind {
            FeatureKind::Extrude(d) => {
                let Some(fr) = d.plane.and_then(|p| ctx.frame_for(&p)) else { return out };
                let (mn, mx) = d.sketch.bounds().unwrap_or((V2::ZERO, V2::ZERO));
                let base = fr.to_world(V2::new(mx.x, mn.y)) + fr.x * (size * 0.06);
                let n = fr.z() * if d.flip { -1.0 } else { 1.0 };
                match d.depth_type {
                    DepthType::Blind => { let e = base + n * v(&d.depth); out.push(FieldDim { key: "depth", text: shown(&d.depth, ""), edit: d.depth.0.clone(), line: Some((base, e)), label: (base + e) * 0.5 + fr.x * (size * 0.04) }); }
                    DepthType::Symmetric => { let (a, e) = (base - n * (v(&d.depth) / 2.0), base + n * (v(&d.depth) / 2.0)); out.push(FieldDim { key: "depth", text: shown(&d.depth, ""), edit: d.depth.0.clone(), line: Some((a, e)), label: base + fr.x * (size * 0.04) }); }
                    _ => {}
                }
                if d.side2 == Side2::Blind && d.depth_type != DepthType::Symmetric {
                    let e = base - n * v(&d.depth2);
                    out.push(FieldDim { key: "depth2", text: shown(&d.depth2, ""), edit: d.depth2.0.clone(), line: Some((base, e)), label: (base + e) * 0.5 + fr.x * (size * 0.04) });
                }
                if d.thin { out.push(FieldDim { key: "thickness", text: shown(&d.thickness, "T "), edit: d.thickness.0.clone(), line: None, label: fr.to_world(mn) - fr.x * (size * 0.05) }); }
            }
            FeatureKind::Revolve(d) => {
                if let (Some(fr), Some((ap, au))) = (d.plane.and_then(|p| ctx.frame_for(&p)), d.sketch.centerline_axis()) {
                    let p = fr.to_world(ap + au * 10.0) + fr.z() * (size * 0.08);
                    out.push(FieldDim { key: "angle", text: shown(&d.angle, "") + "°", edit: d.angle.0.clone(), line: None, label: p });
                }
            }
            FeatureKind::Hole(d) => {
                if let Some(fr) = d.plane.and_then(|p| ctx.frame_for(&p)) {
                    let c = fr.to_world(V2::new(v(&d.x), v(&d.y)));
                    let r = v(&d.diameter) / 2.0;
                    out.push(FieldDim { key: "diameter", text: shown(&d.diameter, "⌀"), edit: d.diameter.0.clone(), line: Some((c - fr.x * r, c + fr.x * r)), label: c + fr.x * (r + size * 0.06) + fr.z() * (size * 0.02) });
                    if !d.through_all {
                        let dir = if d.flip { fr.z() } else { -fr.z() };
                        let a = c - fr.x * (r + size * 0.03);
                        out.push(FieldDim { key: "depth", text: shown(&d.depth, ""), edit: d.depth.0.clone(), line: Some((a, a + dir * v(&d.depth))), label: a + dir * (v(&d.depth) * 0.5) - fr.x * (size * 0.04) });
                    }
                    if v(&d.x).abs() > 1e-9 { let o = fr.to_world(V2::new(0.0, v(&d.y))); out.push(FieldDim { key: "x", text: shown(&d.x, ""), edit: d.x.0.clone(), line: Some((o, c)), label: (o + c) * 0.5 + fr.y * (size * 0.03) }); }
                    if v(&d.y).abs() > 1e-9 { let o = fr.to_world(V2::new(v(&d.x), 0.0)); out.push(FieldDim { key: "y", text: shown(&d.y, ""), edit: d.y.0.clone(), line: Some((o, c)), label: (o + c) * 0.5 + fr.x * (size * 0.03) }); }
                }
            }
            FeatureKind::Round(d) => {
                const KEYS: [&str; 8] = ["radius0", "radius1", "radius2", "radius3", "radius4", "radius5", "radius6", "radius7"];
                const ENDS: [&str; 8] = ["radius_end0", "radius_end1", "radius_end2", "radius_end3", "radius_end4", "radius_end5", "radius_end6", "radius_end7"];
                for (i, set) in d.sets.iter().enumerate().take(8) {
                    let Some(e) = set.edges.first() else { continue };
                    out.push(FieldDim { key: KEYS[i], text: shown(&set.radius, "R"), edit: set.radius.0.clone(), line: None, label: e.a + (e.n1 + e.n2) * (size * 0.05) });
                    if set.variable { out.push(FieldDim { key: ENDS[i], text: shown(&set.radius_end, "R"), edit: set.radius_end.0.clone(), line: None, label: e.b + (e.n1 + e.n2) * (size * 0.05) }); }
                }
            }
            FeatureKind::Chamfer(d) => if let Some(e) = d.edges.first() { out.push(FieldDim { key: "d1", text: shown(&d.d1, "D "), edit: d.d1.0.clone(), line: None, label: e.m + (e.n1 + e.n2) * (size * 0.05) }); },
            FeatureKind::Shell(d) => {
                let p = d.faces.first().map(|f| f.point).unwrap_or(b.max);
                out.push(FieldDim { key: "thickness", text: shown(&d.thickness, "T "), edit: d.thickness.0.clone(), line: None, label: p });
            }
            FeatureKind::Draft(d) => if let Some(fc) = d.faces.first() { out.push(FieldDim { key: "angle", text: shown(&d.angle, "") + "°", edit: d.angle.0.clone(), line: None, label: fc.point + fc.normal * (size * 0.05) }); },
            FeatureKind::Pattern(d) => {
                let src = d.features.first().and_then(|s| self.result.tools.get(s)).map(|t| crate::kernel::csg::bbox(&t.polys).center()).unwrap_or(V3::ZERO);
                match d.kind {
                    PatternKind::Direction => {
                        let dir = match d.dir1 { AxisRef::WorldX => V3::X, AxisRef::WorldY => V3::Y, AxisRef::WorldZ => V3::Z, AxisRef::Axis(id) => self.result.axis(id).map(|a| a.dir).unwrap_or(V3::X) };
                        let e = src + dir * v(&d.spacing1);
                        out.push(FieldDim { key: "spacing1", text: shown(&d.spacing1, ""), edit: d.spacing1.0.clone(), line: Some((src, e)), label: (src + e) * 0.5 + V3::Y * (size * 0.04) });
                        out.push(FieldDim { key: "count1", text: shown(&d.count1, ""), edit: d.count1.0.clone(), line: None, label: src - dir * (size * 0.06) + V3::Y * (size * 0.04) });
                    }
                    PatternKind::Fill | PatternKind::Curve => {}
                    PatternKind::Axis => {
                        out.push(FieldDim { key: "axis_count", text: shown(&d.axis_count, ""), edit: d.axis_count.0.clone(), line: None, label: src + V3::Y * (size * 0.06) });
                        out.push(FieldDim { key: "axis_angle", text: shown(&d.axis_angle, "") + "°", edit: d.axis_angle.0.clone(), line: None, label: src + V3::Y * (size * 0.12) });
                    }
                }
            }
            FeatureKind::DatumPlane(d) if d.kind == PlaneKind::Offset => {
                if let (Some(base), Some(me)) = (d.reference.and_then(|r| ctx.frame_for(&r)), self.result.plane(f.id)) {
                    let c = if b.is_empty() { V3::ZERO } else { b.center() };
                    let a = base.to_world(base.to_local(c)) + base.x * (size * 0.3);
                    let e = a + base.z() * me.frame.z().dot(me.frame.origin - a);
                    out.push(FieldDim { key: "offset", text: shown(&d.offset, ""), edit: d.offset.0.clone(), line: Some((a, e)), label: (a + e) * 0.5 + base.x * (size * 0.04) });
                }
            }
            FeatureKind::Rib(d) => {
                if let (Some(fr), Some((mn, mx))) = (d.plane.and_then(|p| ctx.frame_for(&p)), d.sketch.bounds()) {
                    out.push(FieldDim { key: "thickness", text: shown(&d.thickness, "T "), edit: d.thickness.0.clone(), line: None, label: fr.to_world((mn + mx) * 0.5) + fr.z() * (v(&d.thickness) * 0.5 + size * 0.03) });
                }
            }
            FeatureKind::Helix(d) => {
                if let (Some(fr), Some((mn, _))) = (d.plane.and_then(|p| ctx.frame_for(&p)), d.profile.bounds()) {
                    out.push(FieldDim { key: "pitch", text: shown(&d.pitch, "P "), edit: d.pitch.0.clone(), line: None, label: fr.to_world(mn) - fr.x * (size * 0.05) });
                }
            }
            FeatureKind::Thread(d) => {
                if let Some(c) = self.result.cosmetics.iter().find(|c| c.feature == f.id) {
                    let side = safe_norm(c.dir.cross(if c.dir.x.abs() < 0.9 { V3::X } else { V3::Y }), V3::X) * (c.radius + size * 0.05);
                    let (a, e) = (c.base + side, c.base + side + c.dir * c.length);
                    out.push(FieldDim { key: "length", text: shown(&d.length, ""), edit: d.length.0.clone(), line: Some((a, e)), label: (a + e) * 0.5 + side * 0.3 });
                    out.push(FieldDim { key: "diameter", text: shown(&d.diameter, "M"), edit: d.diameter.0.clone(), line: None, label: c.base - side });
                }
            }
            FeatureKind::Blend(d) if d.rotational => {
                if let (Some(fr), Some((ap, au))) = (d.plane.and_then(|p| ctx.frame_for(&p)), d.sections.first().and_then(|s| s.centerline_axis())) {
                    let offs: Vec<f64> = d.offsets.iter().map(|o| v(o)).collect();
                    let frames = regen::rotational_blend_frames(&fr, ap, au, &offs, d.flip).0;
                    let keys: [&'static str; 6] = ["offset0", "offset1", "offset2", "offset3", "offset4", "offset5"];
                    for (i, o) in d.offsets.iter().enumerate().take(d.sections.len().saturating_sub(1)).take(6) {
                        let c = d.sections.get(i + 1).and_then(|s| s.bounds()).map(|(a, b)| (a + b) * 0.5).unwrap_or(V2::ZERO);
                        out.push(FieldDim { key: keys[i], text: shown(o, "") + "°", edit: o.0.clone(), line: None, label: frames[i + 1].to_world(c) + frames[i + 1].z() * (size * 0.05) });
                    }
                }
            }
            FeatureKind::Blend(d) => {
                if let Some(fr) = d.plane.and_then(|p| ctx.frame_for(&p)) {
                    let mut acc = 0.0;
                    let offs: [&'static str; 6] = ["offset0", "offset1", "offset2", "offset3", "offset4", "offset5"];
                    for (i, o) in d.offsets.iter().enumerate().take(d.sections.len().saturating_sub(1)).take(6) {
                        let sg = if d.flip { -1.0 } else { 1.0 };
                        let a = fr.origin + fr.x * (size * 0.4) + fr.z() * (acc * sg);
                        acc += v(o);
                        let e = fr.origin + fr.x * (size * 0.4) + fr.z() * (acc * sg);
                        out.push(FieldDim { key: offs[i], text: shown(o, ""), edit: o.0.clone(), line: Some((a, e)), label: (a + e) * 0.5 + fr.x * (size * 0.04) });
                    }
                }
            }
            _ => {}
        }
        out
    }

    /// Draws the dimensions of the feature in Edit Dimensions mode and handles in-place edits.
    pub fn dims_overlay(&mut self, ctx: &egui::Context, painter: &egui::Painter, rect: Rect) {
        let Some(ed) = &self.edit_dims else { return };
        let Some(f) = self.data.feature(ed.feature).cloned() else { self.edit_dims = None; return };
        let size = self.viewport_size;
        let cam = self.camera;
        let to_screen = |p: V3| cam.project(p, size).map(|s| Pos2::new(rect.min.x + s.0, rect.min.y + s.1)).unwrap_or(Pos2::new(-1e5, -1e5));
        let mut labels = vec![];
        for (si, sk, fr) in self.feature_sketches(&f, &ed.ctx) {
            let ts = |p: V2| to_screen(fr.to_world(p));
            for e in &sk.entities {
                let pts: Vec<Pos2> = sk.sample(e, 64).iter().map(|p| ts(*p)).collect();
                if e.construction || e.centerline { painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0_f32, Color32::from_rgb(120, 150, 210)), 5.0, 4.0)); }
                else { painter.add(Shape::line(pts, Stroke::new(1.4_f32, Color32::from_rgb(40, 100, 220)))); }
            }
            for c in &sk.constraints {
                if !c.ty.is_dimension() { continue; }
                let r = draw_dim(painter, &sk, c, &ts, if c.weak { WEAKC } else { STRONG }, &dim_text(c));
                labels.push((r, DimKey::Sketch(si, c.id, c.key()), c.expr.clone().unwrap_or_else(|| fmt_num(c.value, 4))));
            }
        }
        for fd in self.field_dims(&f, &ed.ctx) {
            if let Some((a, b)) = fd.line {
                let (sa, sb) = (to_screen(a), to_screen(b));
                painter.line_segment([sa, sb], Stroke::new(1.0_f32, STRONG));
                let d = sb - sa;
                if d.length() > 4.0 {
                    let u = d.normalized();
                    let n = vec2(-u.y, u.x);
                    for (tip, dir) in [(sb, u), (sa, -u)] {
                        painter.add(Shape::convex_polygon(vec![tip, tip - dir * 9.0 + n * 3.0, tip - dir * 9.0 - n * 3.0], STRONG, Stroke::NONE));
                    }
                }
                let ls = to_screen(fd.label);
                painter.line_segment([ls, sa + (sb - sa) * 0.5], Stroke::new(0.6_f32, STRONG));
            }
            let ls = to_screen(fd.label);
            let g = painter.layout_no_wrap(fd.text.clone(), FontId::proportional(12.5), STRONG);
            let r = Rect::from_center_size(ls, g.size() + vec2(8.0, 3.0));
            painter.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(255, 255, 240, 230));
            painter.rect_stroke(r, 2.0, Stroke::new(1.0_f32, Color32::from_rgb(180, 160, 90)), egui::StrokeKind::Inside);
            painter.galley(ls - g.size() / 2.0, g, STRONG);
            labels.push((r, DimKey::Field(fd.key), fd.edit));
        }
        let editing = ed.editing.clone();
        if let Some(e) = &mut self.edit_dims { e.labels = labels; }
        if let Some((key, mut text, pos, focus)) = editing {
            let (mut commit, mut cancel) = (false, false);
            egui::Area::new(egui::Id::new("dims3d_edit")).fixed_pos(pos - vec2(52.0, 12.0)).order(egui::Order::Foreground).show(ctx, |ui| {
                egui::Frame::popup(ui.style()).inner_margin(3.0).show(ui, |ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(104.0));
                    if focus { r.request_focus(); }
                    if r.lost_focus() { if ui.input(|i| i.key_pressed(egui::Key::Enter)) { commit = true; } else { cancel = true; } }
                });
            });
            if let Some(e) = &mut self.edit_dims {
                e.editing = if commit || cancel { None } else { Some((key.clone(), text.clone(), pos, false)) };
            }
            if commit { self.apply_dim_edit(f.id, key, text); }
        }
    }

    /// Returns true when the click landed on a dimension label (and starts editing it).
    pub fn dims_click(&mut self, pos: Pos2) -> bool {
        let Some(ed) = &mut self.edit_dims else { return false };
        if let Some((r, key, text)) = ed.labels.iter().find(|(r, _, _)| r.contains(pos)).cloned() {
            ed.editing = Some((key, text, r.center(), true));
            return true;
        }
        false
    }

    pub(crate) fn apply_dim_edit(&mut self, fid: u64, key: DimKey, text: String) {
        let t = text.trim().to_string();
        if t.is_empty() { return; }
        let vars = self.result.vars.clone();
        let value = match t.parse::<f64>() { Ok(v) => Ok(v), Err(_) => crate::model::expr::eval(&t, &vars) };
        let Ok(value) = value else { self.say(format!("Ungültiger Wert \"{}\".", t), MsgKind::Error); return };
        let mut ok = true;
        let before = self.data.clone();
        self.commit("", |d| {
            let Some(i) = d.index_of(fid) else { return };
            let k = &mut d.features[i].kind;
            match &key {
                DimKey::Field(name) => if let Some(dm) = field_mut(k, name) { *dm = Dim(t.clone()); },
                DimKey::Sketch(si, cid, ckey) => {
                    if let Some(sk) = sketch_mut(k, *si) {
                        // a weak dimension shown on the model may not be stored yet: regenerate weak
                        // dimensions on the stored sketch and match it by its references
                        if !sk.constraints.iter().any(|c| c.id == *cid && !c.weak) {
                            let off = sk.bounds().map(|(a, b)| (b - a).length() * 0.06).unwrap_or(5.0).max(1.0);
                            solver::refresh_weak(sk, off);
                        }
                        let idx = sk.constraints.iter().position(|c| c.id == *cid && !c.weak).or_else(|| sk.constraints.iter().position(|c| c.key() == *ckey));
                        if let Some(c) = idx.map(|i| &mut sk.constraints[i]) {
                            c.weak = false;
                            c.value = value;
                            c.expr = if t.parse::<f64>().is_ok() { None } else { Some(t.clone()) };
                        }
                        ok = solver::solve(sk, None);
                        solver::refresh_weak(sk, 6.0);
                    }
                }
            }
        });
        if !ok {
            self.data = before;
            self.regenerate(false);
            self.say("Die Skizze kann mit diesem Wert nicht gelöst werden.", MsgKind::Error);
        } else if let Some(e) = self.result.errors.get(&fid).cloned() {
            self.say(format!("Regenerierung fehlgeschlagen: {} (mit Rückgängig zurücksetzen).", e), MsgKind::Error);
        } else {
            self.say("Bemaßung geändert und Modell regeneriert.", MsgKind::Success);
        }
    }
}
