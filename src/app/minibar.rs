//! Creo's mini toolbar next to a selection in the graphics window: three rows (edit commands
//! of the owning feature, datums and engineering features, view commands), the "Zugehörige
//! auswählen" list of parents and children, and the Unterdrücken dialog.

use super::icons::{self, Icon};
use super::ribbon::{button_bg, cmd_tip, cmd_tip_ui, theme::*};
use super::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, RichText, Sense, Stroke};
use std::collections::HashMap;

/// State of the mini toolbar's drop-downs.
#[derive(Default)]
pub struct MiniUi {
    /// "Zugehörige auswählen" list open
    pub related: bool,
    /// the list stays open (pin) and follows the selection
    pub pinned: bool,
    /// feature shown as "Aktuell" when one was picked in the list
    pub related_cur: Option<u64>,
    /// ▸ overflow of the second row open
    pub more: bool,
    /// toolbar (and open drop-downs) on screen last frame
    pub rect: Option<Rect>,
    /// selection the toolbar was opened for
    pub pick: Option<Pick>,
}

/// Unterdrücken: the features to suppress and how to handle their children.
pub struct SuppressDlg {
    pub ids: Vec<u64>,
    /// (child, suppress it too)
    pub children: Vec<(u64, bool)>,
    pub keep_all: bool,
    pub details: bool,
}

#[derive(Clone)]
enum A { Dims, Def, Refs, Suppress, Related, Feat(FeatureKind), Zoom, Normal, Hide, ShowAll, HideOthers, Transp, More }

/// One command of the toolbar: icon, name, shortcut, tooltip text, action, enabled.
type Cmd = (Icon, &'static str, &'static str, &'static str, A, bool);

const ROW_BTN: egui::Vec2 = vec2(30.0, 28.0);

fn mini_button(ui: &mut egui::Ui, icon: Icon, enabled: bool, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(ROW_BTN, if enabled { Sense::click() } else { Sense::hover() });
    button_bg(ui, rect, &resp, enabled, selected);
    icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(20.0, 20.0)), icon, enabled);
    resp
}

/// Narrow ▾ / ▸ button next to a command.
fn arrow_button(ui: &mut egui::Ui, down: bool, open: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(18.0, ROW_BTN.y), Sense::click());
    button_bg(ui, rect, &resp, true, open);
    let c = rect.center();
    let col = Color32::from_rgb(70, 74, 80);
    let pts = if down { vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 2.5)] } else { vec![c + vec2(-2.0, -4.5), c + vec2(2.5, 0.0), c + vec2(-2.0, 4.5)] };
    ui.painter().add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
    resp
}

fn row_sep(ui: &mut egui::Ui, w: f32) {
    ui.add_space(3.0);
    let (r, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0_f32, Color32::from_rgb(218, 222, 225)));
    ui.add_space(3.0);
}

fn mini_frame() -> egui::Frame {
    egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, Color32::from_rgb(196, 200, 204))).corner_radius(2.0).inner_margin(egui::Margin::symmetric(5, 4))
        .shadow(egui::epaint::Shadow { offset: [2, 3], blur: 8, spread: 0, color: Color32::from_black_alpha(40) })
}

/// Shows the tooltip of a command, or keeps it open for screenshots (FREO_DEMO_TIP=<name>).
fn tip(resp: egui::Response, c: &Cmd) -> egui::Response {
    if std::env::var("FREO_DEMO_TIP").as_deref() == Ok(c.1) {
        let r = resp.rect;
        egui::Area::new(egui::Id::new("demo_tip")).order(egui::Order::Tooltip).fixed_pos(r.left_bottom() + vec2(14.0, 8.0)).show(&resp.ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| cmd_tip_ui(ui, c.0, c.1, c.2, c.3));
        });
        return resp;
    }
    cmd_tip(resp, c.0, c.1, c.2, c.3)
}

impl App {
    pub(super) fn mini_toolbar(&mut self, ctx: &egui::Context) {
        let Some(mb) = self.mini else { self.creo.mini_ui.more = false; self.creo.mini_ui.related = false; self.creo.mini_ui.rect = None; return };
        if self.busy() || self.edit_dims.is_some() || self.measure || self.creo.suppress.is_some() { self.mini = None; return; }
        // a new selection: the list starts at its owner again (pinned lists reopen)
        if self.creo.mini_ui.pick != Some(mb.pick) {
            let m = &mut self.creo.mini_ui;
            m.pick = Some(mb.pick);
            m.related_cur = None;
            m.more = false;
            m.related = m.pinned;
            m.rect = None;
        }
        // Creo hides the toolbar once the pointer moves well away from it
        let keep = self.creo.mini_ui.related || self.creo.mini_ui.more;
        if let (Some(p), Some(r)) = (ctx.input(|i| i.pointer.hover_pos()), self.creo.mini_ui.rect) {
            if !keep && !r.expand(110.0).contains(p) { self.mini = None; return; }
        } else if let Some(p) = ctx.input(|i| i.pointer.hover_pos()) {
            if !keep && (p - mb.pos).length() > 260.0 { self.mini = None; return; }
        }
        if self.in_asm() { self.asm_mini_toolbar(ctx, mb); return; }

        let owner = self.owner(mb.pick).filter(|o| self.data.feature(*o).is_some());
        let kind = owner.and_then(|o| self.data.feature(o)).map(|f| f.kind.clone());
        let fixed_datum = matches!(&kind, Some(FeatureKind::Csys)) || matches!(&kind, Some(FeatureKind::DatumPlane(d)) if matches!(d.kind, PlaneKind::Front | PlaneKind::Top | PlaneKind::Right));
        let has = owner.is_some();
        let can_hide = kind.as_ref().is_some_and(hideable);
        let any_hidden = self.data.features.iter().any(|f| f.hidden);
        let planar = self.plane_ref(&[mb.pick]).is_some();

        let row1: Vec<Cmd> = vec![
            (Icon::EditDims, "Bemaßungen editieren", "", "Editieren", A::Dims, has),
            (Icon::EditDef, "Definition editieren", "Strg+E", "Definition der ausgewählten Objekts editieren.", A::Def, has && !fixed_datum),
            (Icon::EditRefs, "Referenzen editieren", "", "Referenzen des ausgewählten Elements editieren.", A::Refs, has && !fixed_datum),
            (Icon::Suppress, "Unterdrücken", "", "Ausgewählte KEs unterdrücken.", A::Suppress, has),
            (Icon::SelRelated, "Zugehörige auswählen", "", "Eltern und Kinder des ausgewählten Elements anzeigen und auswählen.", A::Related, has),
        ];
        let plane = (Icon::Plane, "Ebene", "", "Bezugsebene erzeugen.", A::Feat(FeatureKind::new_plane()), true);
        let axis = (Icon::Axis, "Achse", "", "Bezugsachse erzeugen.", A::Feat(FeatureKind::new_axis()), true);
        let point = (Icon::Point, "Punkt", "", "Bezugspunkt erzeugen.", A::Feat(FeatureKind::new_point()), true);
        let csys = (Icon::Csys, "Koordinatensystem", "", "Koordinatensystem erzeugen.", A::Feat(FeatureKind::new_csys()), true);
        let round = (Icon::Round, "Rundung", "R", "Mithilfe eines Radius oder einer Sehne eine Rundung zu Kanten oder dem Raum zwischen Flächen hinzufügen. Rundungen können einen konstanten Radius haben oder variieren und mehrere Radien verwenden.", A::Feat(FeatureKind::new_round()), true);
        let chamfer = (Icon::Chamfer, "Fase", "", "Eine Fase an Kanten erzeugen. Fasen können durch Abstände oder einen Abstand und einen Winkel festgelegt werden.", A::Feat(FeatureKind::new_chamfer()), true);
        let sweep = (Icon::Sweep, "Ziehen", "", "Dreidimensionale Geometrie durch Ziehen eines zweidimensionalen Schnitts entlang einer oder mehrerer Leitkurven erzeugen.", A::Feat(FeatureKind::new_sweep()), true);
        let (row2, more): (Vec<Cmd>, Vec<Cmd>) = match mb.pick {
            Pick::Edge(_) => (vec![plane, axis, point, csys, round, chamfer, sweep], vec![
                (Icon::BoundaryBlend, "Berandungsverbund", "", "Eine Fläche zwischen Randkurven in einer oder zwei Richtungen erzeugen.", A::Feat(FeatureKind::new_surface(SurfOp::Boundary)), true),
                (Icon::Extend, "Verlängern", "", "Eine Sammelfläche entlang ihrer Randkanten verlängern.", A::Feat(FeatureKind::new_surface(SurfOp::Extend)), true),
            ]),
            _ if planar => (vec![
                (Icon::Sketch, "Skizze", "", "Eine Skizze auf der ausgewählten Ebene oder ebenen Fläche erzeugen.", A::Feat(FeatureKind::new_sketch()), true),
                (Icon::Extrude, "Profil", "", "Dreidimensionale Geometrie durch Projizieren eines zweidimensionalen Schnitts in einem angegebenen Abstand senkrecht zur Skizzierebene erzeugen.", A::Feat(FeatureKind::new_extrude()), true),
                (Icon::Revolve, "Drehen", "", "Dreidimensionale Geometrie durch Drehen eines zweidimensionalen Schnitts um eine Mittellinie erzeugen.", A::Feat(FeatureKind::new_revolve()), true),
                (Icon::Hole, "Bohrung", "", "Eine Bohrung zum Modell hinzufügen.", A::Feat(FeatureKind::new_hole()), true),
                plane, axis, point,
            ], vec![
                csys,
                (Icon::Shell, "Schale", "", "Material von einem Volumenkörper entfernen, sodass eine hohle Schale mit Wänden einer festgelegten Dicke verbleibt.", A::Feat(FeatureKind::new_shell()), true),
                (Icon::Draft, "Zugschräge", "", "Eine Zugschräge zu Flächen hinzufügen.", A::Feat(FeatureKind::new_draft()), true),
            ]),
            _ => (vec![plane, axis, point, csys], vec![]),
        };
        let row3: Vec<Cmd> = vec![
            (Icon::ZoomSel, "Auf Auswahl zoomen", "", "Auf die ausgewählten Elemente zoomen.", A::Zoom, true),
            (Icon::NormalView, "Senkrechte Ansicht", "", "Das Modell so ausrichten, dass die ausgewählte Ebene oder Fläche parallel zum Bildschirm liegt.", A::Normal, true),
            (Icon::Hide, "Ausblenden", "", "Ausgewählte Elemente ausblenden.", A::Hide, can_hide),
            (Icon::ShowAll, "Alle einblenden", "", "Alle ausgeblendeten Elemente einblenden.", A::ShowAll, any_hidden),
            (Icon::HideOthers, "Andere ausblenden", "", "Alle Bezugs-KEs außer den ausgewählten ausblenden.", A::HideOthers, true),
            (Icon::Transparent, "Transparent", "", "Volumenkörper transparent anzeigen.", A::Transp, true),
        ];

        // rows are as wide as their buttons (1 px apart), the arrows 18 px
        let bw = ROW_BTN.x + 1.0;
        let width = (row1.len() as f32 * bw + 19.0).max(row2.len() as f32 * bw + if more.is_empty() { 0.0 } else { 19.0 }).max(row3.len() as f32 * bw);
        let mut act: Option<A> = None;
        let (mut related_btn, mut more_btn) = (None::<Rect>, None::<Rect>);
        let (rel_open, more_open, transp) = (self.creo.mini_ui.related, self.creo.mini_ui.more, self.transparent);
        let area = egui::Area::new(egui::Id::new("mini_toolbar")).pivot(Align2::LEFT_BOTTOM).fixed_pos(mb.pos + vec2(10.0, -10.0)).order(egui::Order::Foreground).constrain(true).show(ctx, |ui| {
            mini_frame().show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(1.0, 0.0);
                ui.horizontal(|ui| {
                    for c in &row1 {
                        let r = tip(mini_button(ui, c.0, c.5, matches!(c.4, A::Related) && rel_open), c);
                        if r.clicked() { act = Some(c.4.clone()); }
                        if matches!(c.4, A::Related) { related_btn = Some(r.rect); }
                    }
                    let r = arrow_button(ui, true, rel_open);
                    if r.clicked() && has { act = Some(A::Related); }
                });
                row_sep(ui, width);
                ui.horizontal(|ui| {
                    for c in &row2 { if tip(mini_button(ui, c.0, c.5, false), c).clicked() { act = Some(c.4.clone()); } }
                    if !more.is_empty() {
                        let r = arrow_button(ui, false, more_open);
                        more_btn = Some(r.rect);
                        if r.clicked() { act = Some(A::More); }
                    }
                });
                row_sep(ui, width);
                ui.horizontal(|ui| {
                    for c in &row3 { if tip(mini_button(ui, c.0, c.5, matches!(c.4, A::Transp) && transp), c).clicked() { act = Some(c.4.clone()); } }
                });
            });
        });
        let mut rect = area.response.rect;

        // ▸ overflow: the remaining commands in a column beside the row
        if more_open {
            if let Some(b) = more_btn {
                let fly = egui::Area::new(egui::Id::new("mini_more")).fixed_pos(pos2(b.right() + 2.0, b.top() - 4.0)).order(egui::Order::Foreground).show(ctx, |ui| {
                    mini_frame().inner_margin(egui::Margin::symmetric(3, 3)).show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(0.0, 1.0);
                        for c in &more { if tip(mini_button(ui, c.0, c.5, false), c).clicked() { act = Some(c.4.clone()); } }
                    });
                });
                ctx.move_to_top(fly.response.layer_id);
                rect = rect.union(fly.response.rect);
            }
        }
        if rel_open && has {
            if let Some(b) = related_btn {
                let anchor = pos2(area.response.rect.left() + 6.0, b.bottom() + 3.0);
                if let Some(r) = self.related_popup(ctx, anchor, owner.unwrap()) { rect = rect.union(r); }
            }
        }
        self.creo.mini_ui.rect = Some(rect);

        let Some(a) = act else { return };
        match a {
            A::Related => { let m = &mut self.creo.mini_ui; m.related = !m.related; m.more = false; return; }
            A::More => { let m = &mut self.creo.mini_ui; m.more = !m.more; m.related = false; return; }
            A::Transp => { self.transparent = !self.transparent; return; }
            _ => {}
        }
        self.mini = None;
        match a {
            A::Dims => if let Some(o) = owner { self.start_edit_dims(o) },
            A::Def => if let Some(o) = owner { self.edit_feature(o) },
            A::Refs => if let Some(o) = owner { self.edit_references(o) },
            A::Suppress => if let Some(o) = owner { self.ask_suppress(&[o]) },
            A::Feat(k) => self.start_feature(k),
            A::Zoom => self.zoom_to(&[mb.pick]),
            A::Normal => self.normal_view_of(mb.pick),
            A::Hide => if let Some(o) = owner { self.set_hidden(&[o], true) },
            A::ShowAll => { let ids: Vec<u64> = self.data.features.iter().filter(|f| f.hidden).map(|f| f.id).collect(); self.set_hidden(&ids, false); }
            A::HideOthers => {
                let ids: Vec<u64> = self.data.features.iter().filter(|f| Some(f.id) != owner && !f.hidden && hideable(&f.kind)).map(|f| f.id).collect();
                self.set_hidden(&ids, true);
            }
            A::Related | A::More | A::Transp => {}
        }
    }

    /// Zugehörige auswählen: the model, the owner's body, its direct parents, itself and its
    /// direct children. A click selects that feature; the pin keeps the list open.
    fn related_popup(&mut self, ctx: &egui::Context, at: egui::Pos2, owner: u64) -> Option<Rect> {
        let cur = self.creo.mini_ui.related_cur.filter(|c| self.data.feature(*c).is_some()).unwrap_or(owner);
        let f = self.data.feature(cur)?.clone();
        let parents = self.feature_parents(cur);
        let children = self.feature_children(cur);
        let body = self.data.feature_body.get(&cur).copied().unwrap_or(0);
        let body_name = if body == 0 { "Körper 1".to_string() } else { self.data.feature(body).map(|f| f.name.clone()).unwrap_or_else(|| "Körper".into()) };
        enum Row { Item(usize, Icon, String, Option<u64>, bool), Head(&'static str) }
        let mut rows = vec![Row::Item(0, Icon::Part, format!("{}.PRT", self.data.name.to_uppercase()), None, false), Row::Item(1, Icon::Body, body_name, None, false)];
        let item = |id: u64, me: &App, cur: bool| me.data.feature(id).map(|g| Row::Item(1, icons::for_feature(&g.kind), g.name.clone(), Some(id), cur));
        if !parents.is_empty() { rows.push(Row::Head("Direkte Eltern")); rows.extend(parents.iter().filter_map(|p| item(*p, self, false))); }
        rows.push(Row::Head("Aktuell"));
        rows.push(Row::Item(1, icons::for_feature(&f.kind), f.name.clone(), Some(cur), true));
        if !children.is_empty() { rows.push(Row::Head("Direkte Kinder")); rows.extend(children.iter().filter_map(|c| item(*c, self, false))); }

        let (mut pick, mut pin, mut hover) = (None, false, None);
        let pinned = self.creo.mini_ui.pinned;
        let resp = egui::Area::new(egui::Id::new("related_popup")).fixed_pos(at).order(egui::Order::Foreground).show(ctx, |ui| {
            mini_frame().inner_margin(egui::Margin::same(0)).show(ui, |ui| {
                ui.set_width(290.0);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                // title with the pin
                let (t, _) = ui.allocate_exact_size(vec2(290.0, 34.0), Sense::hover());
                ui.painter().text(t.center(), Align2::CENTER_CENTER, "Zugehörige auswählen", FontId::proportional(14.0), TEXT);
                let pr = Rect::from_center_size(pos2(t.right() - 20.0, t.center().y), vec2(24.0, 24.0));
                let presp = ui.interact(pr, ui.id().with("related_pin"), Sense::click()).on_hover_text(if pinned { "Lösen" } else { "Anheften" });
                button_bg(ui, pr, &presp, true, pinned);
                icons::paint(ui.painter(), pr.shrink(4.0), Icon::Pin, true);
                if presp.clicked() { pin = true; }
                for dy in [0.0, 3.0] {
                    ui.painter().line_segment([pos2(t.left() + 4.0, t.bottom() + dy), pos2(t.right() - 4.0, t.bottom() + dy)], Stroke::new(1.0_f32, Color32::from_rgb(200, 204, 208)));
                }
                ui.add_space(8.0);
                egui::Frame::NONE.stroke(Stroke::new(1.0_f32, Color32::from_rgb(186, 190, 194))).outer_margin(egui::Margin { left: 6, right: 6, top: 0, bottom: 8 }).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(196.0).auto_shrink([false, true]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                        let w = ui.available_width();
                        for row in &rows {
                            let (r, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
                            let p = ui.painter();
                            match row {
                                Row::Head(h) => {
                                    p.rect_filled(r, 0.0, Color32::from_rgb(224, 225, 226));
                                    p.text(pos2(r.left() + 6.0, r.center().y), Align2::LEFT_CENTER, *h, FontId::proportional(13.5), Color32::from_rgb(40, 40, 42));
                                    p.text(pos2(r.left() + 6.4, r.center().y), Align2::LEFT_CENTER, *h, FontId::proportional(13.5), Color32::from_rgb(40, 40, 42));
                                }
                                Row::Item(depth, ic, name, id, current) => {
                                    if id.is_some() && resp.hovered() { p.rect_filled(r, 0.0, HOVER_BG); hover = *id; }
                                    let x = r.left() + 8.0 + *depth as f32 * 16.0;
                                    if *depth > 0 {
                                        let lc = Color32::from_rgb(150, 154, 158);
                                        p.line_segment([pos2(x - 6.0, r.top()), pos2(x - 6.0, r.center().y)], Stroke::new(1.0_f32, lc));
                                        p.line_segment([pos2(x - 6.0, r.center().y), pos2(x + 2.0, r.center().y)], Stroke::new(1.0_f32, lc));
                                    }
                                    icons::paint(p, Rect::from_center_size(pos2(x + 12.0, r.center().y), vec2(17.0, 17.0)), *ic, true);
                                    let font = FontId::proportional(13.5);
                                    let tx = pos2(x + 26.0, r.center().y);
                                    if *current {
                                        let tw = ui.fonts(|fo| fo.layout_no_wrap(name.clone(), font.clone(), TEXT).size().x);
                                        p.rect_filled(Rect::from_min_size(pos2(tx.x - 3.0, r.top() + 2.0), vec2(tw + 8.0, r.height() - 4.0)), 0.0, SEL_BG);
                                        p.text(pos2(tx.x + 0.4, tx.y), Align2::LEFT_CENTER, name, font.clone(), TEXT);
                                    }
                                    p.text(tx, Align2::LEFT_CENTER, name, font, TEXT);
                                    if resp.clicked() { if let Some(i) = id { pick = Some(*i); } }
                                }
                            }
                        }
                    });
                });
            });
        });
        ctx.move_to_top(resp.response.layer_id);
        if hover.is_some() { self.tree_hover = hover; }
        if pin { self.creo.mini_ui.pinned = !pinned; }
        if let Some(id) = pick {
            self.tree_selection = vec![id];
            self.selection = vec![Pick::Feature(id)];
            if let Some(n) = self.data.feature(id).map(|f| f.name.clone()) { self.say(format!("{} ausgewählt.", n), MsgKind::Info); }
            let m = &mut self.creo.mini_ui;
            m.related_cur = Some(id);
            if !m.pinned { m.related = false; }
        }
        Some(resp.response.rect)
    }

    /// Direct parents: datums a feature references and the features owning the faces and
    /// edges it was placed on (matched in the regenerated model).
    pub fn feature_parents(&self, id: u64) -> Vec<u64> {
        let Some(idx) = self.data.index_of(id) else { return vec![] };
        let k = &self.data.features[idx].kind;
        let mut out: Vec<u64> = k.references();
        let r = &self.result;
        let t = &r.topo;
        let tol = (t.bbox.diagonal() * 0.08).max(1.0);
        let owner = |f: usize| r.tag_owner.get(&t.face_tag(f)).copied();
        let mut faces: Vec<FaceRef> = vec![];
        let mut edges: Vec<EdgeRef> = vec![];
        let mut plane = |p: &Option<PlaneRef>| if let Some(PlaneRef::Face(fr)) = p { faces.push(*fr) };
        plane(&k.sketch_plane());
        match k {
            FeatureKind::Hole(d) => plane(&d.plane),
            FeatureKind::DatumPlane(d) => plane(&d.reference),
            FeatureKind::DatumAxis(d) => { plane(&d.p1); plane(&d.p2); }
            FeatureKind::Draft(d) => { plane(&d.hinge); faces.extend(d.faces.iter().copied()); }
            FeatureKind::Shell(d) => faces.extend(d.faces.iter().copied()),
            FeatureKind::Round(d) => edges.extend(d.sets.iter().flat_map(|s| s.edges.iter().copied())),
            FeatureKind::Chamfer(d) => edges.extend(d.edges.iter().copied()),
            FeatureKind::Surface(d) => faces.extend(d.faces.iter().copied()),
            _ => {}
        }
        if !t.faces.is_empty() {
            for fr in &faces {
                let m = t.match_face(fr.point, fr.normal).filter(|f| t.distance_to_face(fr.point, *f) < tol).or_else(|| t.match_surface(fr.point, fr.normal));
                if let Some(o) = m.and_then(owner) { out.push(o); }
            }
            for e in &edges {
                for n in [e.n1, e.n2] {
                    let m = t.match_face(e.m, n).filter(|f| t.distance_to_face(e.m, *f) < tol).or_else(|| t.match_surface(e.m, n));
                    if let Some(o) = m.and_then(owner) { out.push(o); }
                }
            }
        }
        let mut seen = HashSet::new();
        out.retain(|p| *p != id && self.data.index_of(*p).is_some_and(|i| i < idx) && seen.insert(*p));
        out
    }

    /// Features that have `id` as a direct parent.
    pub fn feature_children(&self, id: u64) -> Vec<u64> {
        let Some(idx) = self.data.index_of(id) else { return vec![] };
        self.data.features[idx + 1..].iter().filter(|f| self.feature_parents(f.id).contains(&id)).map(|f| f.id).collect()
    }

    /// All features depending on `ids`, directly or through other children, in model order.
    pub fn feature_descendants(&self, ids: &[u64]) -> Vec<u64> {
        let mut set: HashSet<u64> = ids.iter().copied().collect();
        let mut out = vec![];
        for f in &self.data.features {
            if set.contains(&f.id) { continue; }
            if self.feature_parents(f.id).iter().any(|p| set.contains(p)) { set.insert(f.id); out.push(f.id); }
        }
        out
    }

    /// Unterdrücken: features without children are suppressed right away after confirming;
    /// children are listed with "Alle unterdrücken" / "Alle behalten".
    pub fn ask_suppress(&mut self, ids: &[u64]) {
        let ids: Vec<u64> = ids.iter().copied().filter(|i| self.data.feature(*i).is_some_and(|f| !f.suppressed)).collect();
        if ids.is_empty() { return; }
        let children = self.feature_descendants(&ids).into_iter().filter(|c| !self.data.feature(*c).is_some_and(|f| f.suppressed)).map(|c| (c, true)).collect();
        self.mini = None;
        self.creo.suppress = Some(SuppressDlg { ids, children, keep_all: false, details: false });
    }

    pub(super) fn suppress_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.creo.suppress.take() else { return };
        // the features and the children that go with them are highlighted
        self.selection.clear();
        self.tree_selection = d.ids.iter().copied().chain(d.children.iter().filter(|c| c.1).map(|c| c.0)).collect();
        let (mut ok, mut cancel) = (false, false);
        let names: HashMap<u64, (Icon, String)> = d.children.iter().filter_map(|(c, _)| self.data.feature(*c).map(|f| (*c, (icons::for_feature(&f.kind), f.name.clone())))).collect();
        egui::Window::new("Unterdrücken").id(egui::Id::new("suppress_dlg")).title_bar(false).collapsible(false).resizable(false).order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, vec2(-120.0, -40.0)).show(ctx, |ui| {
                if super::creo::dialog_header(ui, "Unterdrücken") { cancel = true; }
                ui.set_width(600.0);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(26.0, 24.0), Sense::hover());
                    warn_icon(ui.painter(), r);
                    ui.add_space(6.0);
                    let text = if d.children.is_empty() { "Die ausgewählten Elemente werden hervorgehoben." }
                        else { "Die ausgewählten Elemente und deren Kinder werden hervorgehoben. Eine Handhabungsmethode für alle Kinder auswählen oder auf \"Details editieren\" klicken, um für jedes Kind eine Methode auszuwählen." };
                    ui.add(egui::Label::new(RichText::new(text).size(13.5).color(TEXT)).wrap());
                });
                ui.add_space(8.0);
                if !d.children.is_empty() {
                    ui.separator();
                    ui.add_space(4.0);
                    ui.label(RichText::new("Handhabungsmethode").strong().size(14.0).color(TEXT));
                    ui.add_space(4.0);
                    let before = d.keep_all;
                    ui.radio_value(&mut d.keep_all, false, RichText::new("Alle unterdrücken").size(13.5));
                    ui.radio_value(&mut d.keep_all, true, RichText::new("Alle behalten").size(13.5));
                    if before != d.keep_all { for c in &mut d.children { c.1 = !d.keep_all; } }
                    ui.add_space(6.0);
                    if ui.add(egui::Button::new(RichText::new("Details editieren...").size(13.5)).min_size(vec2(150.0, 32.0)).fill(Color32::WHITE)).clicked() { d.details = !d.details; }
                    if d.details {
                        ui.add_space(6.0);
                        egui::Frame::NONE.stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(6.0).show(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                                egui::Grid::new("suppress_children").num_columns(2).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                                    ui.label(RichText::new("Kind").strong());
                                    ui.label(RichText::new("Methode").strong());
                                    ui.end_row();
                                    for (i, c) in d.children.iter_mut().enumerate() {
                                        let (ic, n) = names.get(&c.0).cloned().unwrap_or((Icon::Part, "?".into()));
                                        ui.horizontal(|ui| {
                                            let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                                            icons::paint(ui.painter(), r, ic, true);
                                            ui.label(n);
                                        });
                                        egui::ComboBox::from_id_salt(("suppress_child", i)).width(130.0).selected_text(if c.1 { "Unterdrücken" } else { "Behalten" }).show_ui(ui, |ui| {
                                            ui.selectable_value(&mut c.1, true, "Unterdrücken");
                                            ui.selectable_value(&mut c.1, false, "Behalten");
                                        });
                                        ui.end_row();
                                    }
                                });
                            });
                        });
                        if d.children.iter().all(|c| c.1) { d.keep_all = false; } else if d.children.iter().all(|c| !c.1) { d.keep_all = true; }
                    }
                    ui.add_space(8.0);
                }
                ui.separator();
                ui.add_space(4.0);
                ui.allocate_ui_with_layout(vec2(ui.available_width(), 38.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    if ui.add(egui::Button::new(RichText::new("Abbrechen").size(13.5)).min_size(vec2(120.0, 34.0)).fill(Color32::WHITE)).clicked() { cancel = true; }
                    let okb = egui::Button::new(RichText::new("OK").size(13.5)).min_size(vec2(120.0, 34.0)).fill(Color32::from_rgb(158, 218, 250)).stroke(Stroke::new(1.0_f32, Color32::from_rgb(110, 176, 220)));
                    if ui.add(okb).clicked() { ok = true; }
                });
                ui.add_space(2.0);
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) { cancel = true; }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) { ok = true; }
        if ok {
            let all: Vec<u64> = d.ids.iter().copied().chain(d.children.iter().filter(|c| c.1).map(|c| c.0)).collect();
            self.tree_selection.clear();
            self.set_suppressed(&all, true);
        } else if cancel {
            self.tree_selection = d.ids.clone();
        } else {
            self.creo.suppress = Some(d);
        }
    }

    /// Referenzen editieren: the feature's dashboard opens with its references panel.
    pub fn edit_references(&mut self, id: u64) {
        self.edit_feature(id);
        let Some(ed) = &self.editor else { return };
        let panels = Self::panels_for(&ed.feature.kind);
        let p = panels.iter().find(|p| matches!(**p, "Referenzen" | "Platzierung" | "Sätze")).or(panels.first()).copied();
        self.dash_panel = p;
    }

    /// Auf Auswahl zoomen: fit the view to the picked geometry.
    pub fn zoom_to(&mut self, picks: &[Pick]) {
        let mut b = BBox::default();
        {
            let r = self.displayed();
            let t = &r.topo;
            let face = |f: usize, b: &mut BBox| if let Some(fc) = t.faces.get(f) { for &pi in &fc.polys { for &v in &t.polys[pi] { b.add(t.positions[v]); } } };
            for p in picks {
                match *p {
                    Pick::Edge(e) => if let Some(c) = t.chains.get(e) { for q in &c.points { b.add(*q); } },
                    Pick::Face(f) => face(f, &mut b),
                    Pick::Feature(id) => for f in self.faces_of_feature(id) { face(f, &mut b); },
                    _ => {}
                }
            }
        }
        if b.is_empty() { self.push_view(); self.fit_pending = true; return; }
        let pad = (5.0 - b.diagonal()).max(0.0) * 0.5;
        b.add(b.min - V3::splat(pad));
        b.add(b.max + V3::splat(pad));
        self.push_view();
        let (w, h) = self.viewport_size;
        self.camera.fit(&b, w / h);
    }

    /// Senkrechte Ansicht for a pick; an edge uses one of its two faces that is planar.
    pub fn normal_view_of(&mut self, p: Pick) {
        let cand = match p {
            Pick::Edge(e) => self.displayed().topo.chains.get(e).map(|c| vec![Pick::Face(c.face_a), Pick::Face(c.face_b)]).unwrap_or_default(),
            p => vec![p],
        };
        let saved = std::mem::replace(&mut self.selection, cand);
        self.normal_view();
        self.selection = saved;
    }
}

/// Features the graphics window can hide (datums, sketches, curves).
fn hideable(k: &FeatureKind) -> bool {
    matches!(k, FeatureKind::DatumPlane(_) | FeatureKind::DatumAxis(_) | FeatureKind::DatumPoint(_) | FeatureKind::Csys | FeatureKind::DatumCsys(_) | FeatureKind::Sketch(_) | FeatureKind::Curve(_))
}

/// Creo's yellow warning triangle.
fn warn_icon(p: &egui::Painter, r: Rect) {
    let c = r.center();
    let pts = vec![pos2(c.x, r.top() + 1.0), pos2(r.right() - 1.0, r.bottom() - 1.0), pos2(r.left() + 1.0, r.bottom() - 1.0)];
    p.add(egui::Shape::convex_polygon(pts, Color32::from_rgb(250, 196, 36), Stroke::new(1.0_f32, Color32::from_rgb(196, 140, 10))));
    p.text(pos2(c.x, c.y + 2.5), Align2::CENTER_CENTER, "!", FontId::proportional(15.0), Color32::from_rgb(30, 30, 30));
}
