//! 3D graphics window: rendering, navigation, picking, highlights, datum display, in-place
//! sketching and the Edit Dimensions overlay.

use super::pick::{axis_segment, plane_quad};
use super::render::FrameData;
use super::ui::datum_tag;
use super::*;
use crate::model::regen::RegenResult;
use eframe::egui::{self, Color32, Pos2, Sense, Stroke};

const SEL: [f32; 4] = [0.10, 0.80, 0.30, 0.50];
const SEL32: Color32 = Color32::from_rgb(20, 200, 70);
const PRE32: Color32 = Color32::from_rgb(40, 175, 255);
const DATUM_FRONT: [f32; 4] = [0.667, 0.506, 0.216, 0.95];
const DATUM_BACK: [f32; 4] = [0.45, 0.45, 0.47, 0.85];
const AXIS: [f32; 4] = [0.60, 0.40, 0.16, 0.95];
const CURVE: [f32; 4] = [0.10, 0.35, 0.95, 1.0];
const TAG: Color32 = Color32::from_rgb(170, 129, 55);

fn push_line(v: &mut Vec<f32>, a: V3, b: V3, c: [f32; 4]) {
    v.extend_from_slice(&[a.x as f32, a.y as f32, a.z as f32, c[0], c[1], c[2], c[3], b.x as f32, b.y as f32, b.z as f32, c[0], c[1], c[2], c[3]]);
}

fn push_tri(v: &mut Vec<f32>, p: [V3; 3], c: [f32; 4]) {
    for q in p { v.extend_from_slice(&[q.x as f32, q.y as f32, q.z as f32, c[0], c[1], c[2], c[3]]); }
}

fn face_tris(r: &RegenResult, faces: &[usize], out: &mut Vec<f32>) {
    let t = &r.topo;
    for &f in faces {
        let Some(face) = t.faces.get(f) else { continue };
        for &pi in &face.polys {
            let p = &t.polys[pi];
            let n = t.planes[pi].n;
            for k in 1..p.len() - 1 {
                for &j in &[0, k, k + 1] {
                    let v = t.positions[p[j]];
                    out.extend_from_slice(&[v.x as f32, v.y as f32, v.z as f32, n.x as f32, n.y as f32, n.z as f32]);
                }
            }
        }
    }
}

/// Dash-dot pattern along a segment (Creo axes).
fn dash_dot(v: &mut Vec<f32>, a: V3, b: V3, c: [f32; 4], n: usize) {
    for k in 0..n {
        let t0 = k as f64 / n as f64;
        let t1 = t0 + 0.62 / n as f64;
        let t2 = t0 + 0.78 / n as f64;
        let t3 = t0 + 0.86 / n as f64;
        push_line(v, a + (b - a) * t0, a + (b - a) * t1, c);
        push_line(v, a + (b - a) * t2, a + (b - a) * t3, c);
    }
}

impl App {
    fn faces_of_feature(&self, fid: u64) -> Vec<usize> {
        let r = self.displayed();
        (0..r.topo.faces.len()).filter(|&f| r.tag_owner.get(&r.topo.face_tag(f)) == Some(&fid)).collect()
    }

    fn highlight_sets(&self) -> (Vec<usize>, Vec<usize>, Vec<u64>, Vec<u64>) {
        let r = self.displayed();
        let (mut faces, mut edges, mut planes, mut axes) = (vec![], vec![], vec![], vec![]);
        for p in &self.selection {
            match *p {
                Pick::Face(f) => faces.push(f),
                Pick::Edge(e) => edges.push(e),
                Pick::Plane(id) => planes.push(id),
                Pick::Axis(id) => axes.push(id),
                Pick::Feature(id) => faces.extend(self.faces_of_feature(id)),
                Pick::Point(_) => {}
            }
        }
        if self.selection.is_empty() { for id in &self.tree_selection { faces.extend(self.faces_of_feature(*id)); } }
        for p in self.place_ref_picks() {
            match p {
                Pick::Face(f) => faces.push(f),
                Pick::Edge(e) => edges.push(e),
                Pick::Plane(id) => planes.push(id),
                Pick::Axis(id) => axes.push(id),
                _ => {}
            }
        }
        if let Some(ed) = &self.editor {
            let t = &r.topo;
            let mut refs: Vec<Option<PlaneRef>> = vec![];
            match &ed.feature.kind {
                FeatureKind::Round(d) => edges.extend(d.sets.iter().flat_map(|s| s.edges.iter()).filter_map(|e| t.match_chain(e))),
                FeatureKind::Chamfer(d) => edges.extend(d.edges.iter().filter_map(|e| t.match_chain(e))),
                FeatureKind::Shell(d) => faces.extend(d.faces.iter().filter_map(|f| t.match_face(f.point, f.normal))),
                FeatureKind::Draft(d) => { faces.extend(d.faces.iter().filter_map(|f| t.match_face(f.point, f.normal))); refs.push(d.hinge); }
                FeatureKind::Hole(d) => refs.push(d.plane),
                FeatureKind::Mirror(d) => refs.push(d.plane),
                FeatureKind::DatumPlane(d) => refs.push(d.reference),
                FeatureKind::DatumAxis(d) => { refs.push(d.p1); refs.push(d.p2); }
                k => refs.push(k.sketch_plane()),
            }
            if self.sketch.is_none() {
                for p in refs {
                    match p {
                        Some(PlaneRef::Datum(id)) => planes.push(id),
                        Some(PlaneRef::Face(fr)) => if let Some(f) = t.match_face(fr.point, fr.normal) { faces.push(f) },
                        None => {}
                    }
                }
            }
        }
        (faces, edges, planes, axes)
    }

    pub fn viewport_ui(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        super::ui::no_hand(ui.ctx(), resp.id);
        self.viewport_size = (rect.width().max(1.0), rect.height().max(1.0));
        let size = self.viewport_size;
        self.rebuild_buffers();
        if self.fit_pending {
            let b = self.displayed().topo.bbox;
            self.camera.fit(&b, size.0 / size.1);
            self.fit_pending = false;
        }
        let b = self.displayed().topo.bbox;
        if !b.is_empty() { self.camera.depth = b.diagonal() * 10.0 + 1000.0; }

        // ---- navigation (middle = spin, ⇧ middle = pan, wheel = zoom, as in Creo) ----
        let (scroll, zoom, mods, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers, i.pointer.clone()));
        let hover_pos = resp.hover_pos();
        let local = |p: Pos2| (p.x - rect.min.x, p.y - rect.min.y);
        if let Some(hp) = hover_pos {
            if zoom != 1.0 { self.camera.zoom_at(1.0 / zoom as f64, local(hp), size); }
            if scroll.y != 0.0 || scroll.x != 0.0 {
                if mods.shift { self.camera.pan(-scroll.x, -scroll.y, size.1); }
                else if mods.alt { self.camera.orbit(-scroll.x, -scroll.y); }
                else { self.camera.zoom_at((-scroll.y as f64 * 0.0025).exp(), local(hp), size); }
            }
        }
        let delta = pointer.delta();
        let orbit_btn = pointer.middle_down() || pointer.secondary_down() || (pointer.primary_down() && mods.alt);
        if resp.dragged() && orbit_btn && (delta.x != 0.0 || delta.y != 0.0) {
            if mods.shift { self.camera.pan(delta.x, delta.y, size.1); }
            else if mods.ctrl { self.camera.zoom_at((delta.y as f64 * 0.01).exp(), (size.0 / 2.0, size.1 / 2.0), size); }
            else { self.camera.orbit(delta.x, delta.y); }
        }
        let dragging = resp.dragged() && orbit_btn;
        // Component Placement: drag moves the component being placed, ⇧ drag rotates it
        if self.placing() && resp.dragged_by(egui::PointerButton::Primary) && !mods.alt && (delta.x != 0.0 || delta.y != 0.0) {
            self.place_drag(delta.x, delta.y, mods.shift);
        }

        if self.sketch.is_some() && self.resetup.is_some() {
            // Skizzen-Setup: a plane or planar face picked in the graphics window becomes the new sketch plane
            self.hover = hover_pos.and_then(|hp| self.pick_at(local(hp))).filter(|p| self.plane_ref(&[*p]).is_some());
            if resp.clicked() {
                if let Some(pr) = self.hover.and_then(|p| self.plane_ref(&[p])) { if let Some(r) = &mut self.resetup { r.0 = pr; } }
            }
        } else if self.sketch.is_some() {
            let cam = self.camera;
            if let Some(s) = &mut self.sketch { s.interact(ui, &resp, rect, &cam); }
            self.hover = None;
        } else {
            // ---- preselection, selection, edit dimensions ----
            if let Some(hp) = hover_pos {
                if !dragging && !pointer.any_down() { self.hover = self.pick_at(local(hp)); }
            } else { self.hover = None; }
            if let Some(fid) = self.tree_hover { if self.hover.is_none() { self.hover = Some(Pick::Feature(fid)); } }
            self.tree_hover_ext = match self.hover { Some(p) if hover_pos.is_some() => self.owner(p), _ => None };
            if resp.clicked() && !mods.alt {
                let consumed = hover_pos.map(|hp| self.dims_click(hp)).unwrap_or(false);
                if !consumed {
                    let p = hover_pos.and_then(|hp| self.pick_at(local(hp)));
                    if self.edit_dims.is_some() {
                        let owner = p.and_then(|p| self.owner(p));
                        if owner != self.edit_dims.as_ref().map(|e| e.feature) { self.edit_dims = None; }
                    }
                    self.handle_pick(p, mods.shift || mods.command);
                    self.mini = match (p, hover_pos) {
                        (Some(pk), Some(hp)) if self.editor.is_none() && !self.measure => Some(MiniBar { pos: hp, pick: pk }),
                        _ => None,
                    };
                }
            }
            if resp.double_clicked() && self.editor.is_none() && !self.placing() {
                if let Some(o) = hover_pos.and_then(|hp| self.pick_at(local(hp))).and_then(|p| self.owner(p)) {
                    if self.in_asm() { if self.component(o).is_some() { self.edit_placement(o); } }
                    else if self.data.feature(o).is_some() { self.start_edit_dims(o); }
                }
            }
            if resp.secondary_clicked() {
                let p = hover_pos.and_then(|hp| self.pick_at(local(hp)));
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("ctx_pick"), p));
                if let Some(p) = p { if !self.selection.contains(&p) && self.editor.is_none() && !self.placing() { self.selection = vec![p]; } }
                self.mini = None;
            }
            resp.context_menu(|ui| self.context_menu(ui));
        }

        // ---- build GL frame ----
        let r = self.displayed().clone();
        let (faces, edges, planes, axes) = self.highlight_sets();
        let mut hl_tris = vec![];
        face_tris(&r, &faces, &mut hl_tris);
        let mut hover_tris = vec![];
        let mut glass = vec![];
        let mut lines = vec![];
        match self.hover {
            Some(Pick::Face(f)) if !faces.contains(&f) => face_tris(&r, &[f], &mut hover_tris),
            Some(Pick::Feature(id)) => { let fs = self.faces_of_feature(id); face_tris(&r, &fs, &mut hover_tris); }
            _ => {}
        }
        let back = self.camera.back();
        if self.show_planes || self.editor.as_ref().map(|e| e.pick.is_some()).unwrap_or(false) {
            for p in &r.planes {
                if self.data.feature(p.id).map(|f| f.hidden).unwrap_or(false) { continue; }
                let q = plane_quad(p, &r);
                let sel = planes.contains(&p.id);
                let hov = self.hover == Some(Pick::Plane(p.id));
                let front = p.frame.z().dot(back) >= 0.0;
                let fill = if sel { [0.10, 0.80, 0.30, 0.20] } else if hov { [0.20, 0.65, 1.0, 0.18] } else if front { [0.86, 0.72, 0.46, 0.07] } else { [0.6, 0.6, 0.62, 0.05] };
                push_tri(&mut glass, [q[0], q[1], q[2]], fill);
                push_tri(&mut glass, [q[0], q[2], q[3]], fill);
                let c = if sel { [0.05, 0.70, 0.25, 1.0] } else if hov { [0.15, 0.60, 1.0, 1.0] } else if front { DATUM_FRONT } else { DATUM_BACK };
                for i in 0..4 { push_line(&mut lines, q[i], q[(i + 1) % 4], c); }
            }
        }
        if self.show_axes {
            for a in &r.axes {
                let (p0, p1) = axis_segment(a, &r);
                let c = if axes.contains(&a.id) { [0.05, 0.70, 0.25, 1.0] } else if self.hover == Some(Pick::Axis(a.id)) { [0.15, 0.60, 1.0, 1.0] } else { AXIS };
                dash_dot(&mut lines, p0, p1, c, 14);
            }
        }
        let s = (if b.is_empty() { 100.0 } else { b.diagonal() }) * 0.015;
        if self.show_points {
            for p in &r.points { for d in [V3::X, V3::Y, V3::Z] { push_line(&mut lines, p.p - d * s, p.p + d * s, AXIS); } }
        }
        let csys_on = self.show_csys && !self.data.feature(CSYS_ID).map(|f| f.hidden || f.suppressed).unwrap_or(true);
        if csys_on {
            let l = s * 4.0;
            for d in [V3::X, V3::Y, V3::Z] {
                push_line(&mut lines, V3::ZERO, d * l, AXIS);
                let o = if d == V3::Y { V3::X } else { V3::Y };
                push_line(&mut lines, d * l, d * (l * 0.86) + o * (l * 0.05), AXIS);
                push_line(&mut lines, d * l, d * (l * 0.86) - o * (l * 0.05), AXIS);
            }
        }
        if self.show_csys {
            for c in &r.csys {
                if self.data.feature(c.id).map(|f| f.hidden).unwrap_or(false) { continue; }
                let l = s * 3.0;
                let (o, f) = (c.frame.origin, c.frame);
                for (d, side) in [(f.x, f.y), (f.y, f.x), (f.z(), f.x)] {
                    push_line(&mut lines, o, o + d * l, AXIS);
                    push_line(&mut lines, o + d * l, o + d * (l * 0.86) + side * (l * 0.05), AXIS);
                    push_line(&mut lines, o + d * l, o + d * (l * 0.86) - side * (l * 0.05), AXIS);
                }
            }
        }
        for c in &r.cosmetics {
            let w = safe_norm(c.dir.cross(if c.dir.x.abs() < 0.9 { V3::X } else { V3::Y }), V3::X);
            let v2 = c.dir.cross(w);
            let ring = |off: f64| -> Vec<V3> { (0..=48).map(|k| { let t = std::f64::consts::TAU * k as f64 / 48.0; c.base + c.dir * off + (w * t.cos() + v2 * t.sin()) * c.radius }).collect() };
            let (r0, r1) = (ring(0.0), ring(c.length));
            let col = [0.92, 0.55, 0.15, 1.0];
            for k in 0..48 {
                push_line(&mut lines, r0[k], r0[k + 1], col);
                push_line(&mut lines, r1[k], r1[k + 1], col);
                push_tri(&mut glass, [r0[k], r0[k + 1], r1[k + 1]], [1.0, 0.62, 0.2, 0.16]);
                push_tri(&mut glass, [r0[k], r1[k + 1], r1[k]], [1.0, 0.62, 0.2, 0.16]);
            }
        }
        if self.show_curves {
            for c in &r.curves {
                if self.data.feature(c.feature).map(|f| f.hidden).unwrap_or(false) { continue; }
                for pl in &c.polylines { for w in pl.windows(2) { push_line(&mut lines, w[0], w[1], CURVE); } }
            }
        }
        if let Some(ed) = &self.editor {
            if self.sketch.is_none() {
                if let Some(fr) = ed.feature.kind.sketch_plane().and_then(|p| r.frame_for(&p)) {
                    if let Some(sk) = self.sketch_for(Slot::Main) {
                        for e in &sk.entities {
                            let pts = sk.sample(e, 64);
                            for w in pts.windows(2) { push_line(&mut lines, fr.to_world(w[0]), fr.to_world(w[1]), [0.95, 0.55, 0.05, 1.0]); }
                        }
                    }
                }
                if ed.pick.is_none() && ed.preview {
                    if let Some(tool) = self.preview.as_ref().and_then(|p| p.tools.get(&ed.feature.id)) {
                        let col = if tool.op == crate::model::regen::Op::Cut { [0.95, 0.35, 0.2, 0.10] } else { [1.0, 0.85, 0.1, 0.10] };
                        for p in tool.polys.iter().take(20000) {
                            for k in 1..p.v.len() - 1 { push_tri(&mut glass, [p.v[0].p, p.v[k].p, p.v[k + 1].p], col); }
                        }
                    }
                }
            }
        }
        if let Some(op) = self.editing_from_asm() { glass.extend_from_slice(&op.ghost); }
        let a = &self.data.appearance;
        let tint = if self.asm_view().is_some() { [1.0, 1.0, 1.0] } else { a.color };
        let section = self.section_plane().map(|(n, w)| [n.x as f32, n.y as f32, n.z as f32, w as f32]);
        let frame = FrameData {
            mvp: self.camera.proj(size.0 / size.1) * self.camera.view(),
            view: self.camera.view(),
            model: self.buffers.clone(),
            color: tint,
            opacity: if self.transparent { 0.45 } else if self.asm_view().is_some() { 1.0 } else { a.opacity },
            style: self.style,
            section,
            hl_tris,
            hl_color: SEL,
            hover_tris,
            lines,
            glass,
            depth_bias: 0.0004,
        };
        let painter = ui.painter_at(rect);
        super::ui::graphics_background_bg(&painter, rect, self.creo.bg);
        if let Some(rend) = &self.renderer {
            let rend = rend.clone();
            let frame = Arc::new(frame);
            let cb = eframe::egui_glow::CallbackFn::new(move |_info, p| { rend.lock().unwrap().paint(p.gl(), &frame); });
            painter.add(egui::PaintCallback { rect, callback: Arc::new(cb) });
        }

        // ---- overlays ----
        let to_screen = |p: V3| self.camera.project(p, size).map(|s| Pos2::new(rect.min.x + s.0, rect.min.y + s.1));
        let chain_line = |ci: usize, col: Color32, w: f32| {
            if let Some(c) = r.topo.chains.get(ci) {
                let mut pts: Vec<Pos2> = c.points.iter().filter_map(|p| to_screen(*p)).collect();
                if c.closed { if let Some(f) = pts.first().copied() { pts.push(f); } }
                painter.add(egui::Shape::line(pts, Stroke::new(w, col)));
            }
        };
        for &e in &edges { chain_line(e, SEL32, 3.0); }
        if let Some(Pick::Edge(e)) = self.hover { if !edges.contains(&e) { chain_line(e, PRE32, 3.0); } }
        if self.show_tags {
            if self.show_planes {
                for p in &r.planes {
                    if self.data.feature(p.id).map(|f| f.hidden).unwrap_or(false) { continue; }
                    let q = plane_quad(p, &r);
                    // tag at the corner nearest the top-left of the screen
                    let corner = q.iter().filter_map(|c| to_screen(*c)).min_by(|a, b| (a.x + a.y).partial_cmp(&(b.x + b.y)).unwrap());
                    if let Some(sp) = corner { datum_tag(&painter, sp + egui::vec2(4.0, 3.0), &p.name, TAG); }
                }
            }
            if self.show_axes {
                for ax in &r.axes {
                    let (_, p1) = axis_segment(ax, &r);
                    if let Some(sp) = to_screen(p1) { datum_tag(&painter, sp + egui::vec2(3.0, -16.0), &ax.name, TAG); }
                }
            }
            if self.show_points {
                for p in &r.points { if let Some(sp) = to_screen(p.p) { datum_tag(&painter, sp + egui::vec2(6.0, -18.0), &p.name, TAG); } }
            }
            if csys_on {
                let l = s * 4.0;
                for (d, n) in [(V3::X, "X"), (V3::Y, "Y"), (V3::Z, "Z")] {
                    if let Some(sp) = to_screen(d * (l * 1.12)) { painter.text(sp, egui::Align2::CENTER_CENTER, n, egui::FontId::proportional(11.0), TAG); }
                }
                let name = self.data.feature(CSYS_ID).map(|f| f.name.clone()).unwrap_or("BASIS".into());
                if let Some(sp) = to_screen(V3::ZERO) { datum_tag(&painter, sp + egui::vec2(6.0, 6.0), &name, TAG); }
            }
            if self.show_csys {
                let l = s * 3.0;
                for c in &r.csys {
                    if self.data.feature(c.id).map(|f| f.hidden).unwrap_or(false) { continue; }
                    let f = c.frame;
                    for (d, n) in [(f.x, "X"), (f.y, "Y"), (f.z(), "Z")] {
                        if let Some(sp) = to_screen(f.origin + d * (l * 1.12)) { painter.text(sp, egui::Align2::CENTER_CENTER, n, egui::FontId::proportional(11.0), TAG); }
                    }
                    if let Some(sp) = to_screen(f.origin) { datum_tag(&painter, sp + egui::vec2(6.0, 6.0), &c.name, TAG); }
                }
            }
        }
        if self.show_spin && self.sketch.is_none() {
            if let Some(c) = to_screen(self.camera.spin_center) {
                for (d, col) in [(V3::X, Color32::from_rgb(220, 40, 40)), (V3::Y, Color32::from_rgb(30, 160, 50)), (V3::Z, Color32::from_rgb(40, 80, 230))] {
                    let v = egui::vec2(d.dot(self.camera.right()) as f32, -d.dot(self.camera.up()) as f32) * 16.0;
                    painter.line_segment([c, c + v], Stroke::new(2.0_f32, col));
                }
            }
        }
        if let Some(s) = &self.sketch {
            s.draw(&painter);
        }
        if let Some(s) = &mut self.sketch { s.overlays(ui.ctx()); }
        if self.edit_dims.is_some() { self.dims_overlay(ui.ctx(), &painter, rect); }
        if self.hover.is_some() && self.sketch.is_none() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
    }

    fn context_menu(&mut self, ui: &mut egui::Ui) {
        use super::icons::Icon;
        use super::ribbon::small;
        let p: Option<Pick> = ui.ctx().data(|d| d.get_temp(egui::Id::new("ctx_pick"))).flatten();
        if self.in_asm() {
            if self.placing() {
                if small(ui, Icon::Ok, "OK", true, false).clicked() { self.place_finish(); ui.close_menu(); }
                if small(ui, Icon::Cancel, "Abbrechen", true, false).clicked() { self.place_cancel(); ui.close_menu(); }
            } else if let Some(o) = p.and_then(|p| self.owner(p)).filter(|o| self.component(*o).is_some()) {
                self.comp_menu(ui, o);
            } else {
                if small(ui, Icon::Assemble, "Zusammenbauen...", true, false).clicked() { ui.close_menu(); self.assemble_from_file(); }
                if small(ui, Icon::CreateComp, "Erzeugen...", true, false).clicked() { ui.close_menu(); self.windows.create_comp = Some(self.asm.as_ref().map(|a| a.data.next_part_name()).unwrap_or_default()); }
            }
            ui.separator();
            if small(ui, Icon::SavedViews, "Standardausrichtung", true, false).clicked() { self.camera.rot = camera::StdView::Standard.rotation(); self.fit_pending = true; ui.close_menu(); }
            if small(ui, Icon::Refit, "Einpassen", true, false).clicked() { self.fit_pending = true; ui.close_menu(); }
            return;
        }
        let owner = p.and_then(|p| self.owner(p)).filter(|o| self.data.feature(*o).is_some());
        if self.editor.is_none() {
            let planar = self.plane_ref(&p.into_iter().collect::<Vec<_>>()).is_some();
            if planar {
                if small(ui, Icon::Sketch, "Skizze", true, false).clicked() { self.start_feature(FeatureKind::new_sketch()); ui.close_menu(); }
                if small(ui, Icon::Extrude, "Profil", true, false).clicked() { self.start_feature(FeatureKind::new_extrude()); ui.close_menu(); }
                if small(ui, Icon::Revolve, "Drehen", true, false).clicked() { self.start_feature(FeatureKind::new_revolve()); ui.close_menu(); }
                if small(ui, Icon::Hole, "Bohrung", true, false).clicked() { self.start_feature(FeatureKind::new_hole()); ui.close_menu(); }
                if small(ui, Icon::Plane, "Versatzebene", true, false).clicked() { self.start_feature(FeatureKind::new_plane()); ui.close_menu(); }
                ui.separator();
            }
            if matches!(p, Some(Pick::Edge(_))) {
                if small(ui, Icon::Round, "Rundung", true, false).clicked() { self.start_feature(FeatureKind::new_round()); ui.close_menu(); }
                if small(ui, Icon::Chamfer, "Fase", true, false).clicked() { self.start_feature(FeatureKind::new_chamfer()); ui.close_menu(); }
                ui.separator();
            }
            if let Some(o) = owner {
                if small(ui, Icon::EditDims, "Bemaßungen editieren", true, false).clicked() { self.start_edit_dims(o); ui.close_menu(); }
                if small(ui, Icon::EditDef, "Definition editieren", true, false).clicked() { self.edit_feature(o); ui.close_menu(); }
                if small(ui, Icon::Suppress, "Unterdrücken", true, false).clicked() { self.set_suppressed(&[o], true); ui.close_menu(); }
                if small(ui, Icon::Hide, "Ausblenden", true, false).clicked() { self.toggle_hidden(&[o]); ui.close_menu(); }
                if small(ui, Icon::Delete, "Löschen", true, false).clicked() { self.delete_features(&[o]); ui.close_menu(); }
                ui.separator();
            }
        } else if small(ui, Icon::Ok, "OK", true, false).clicked() { self.finish_feature(); ui.close_menu(); }
        if small(ui, Icon::SavedViews, "Standardausrichtung", true, false).clicked() { self.camera.rot = camera::StdView::Standard.rotation(); self.fit_pending = true; ui.close_menu(); }
        if small(ui, Icon::Refit, "Einpassen", true, false).clicked() { self.fit_pending = true; ui.close_menu(); }
    }
}
