//! Creo's graphics toolbar (Grafiksymbolleiste) over the model window, and its right-click
//! customization menu: one check row per command, Position, Größe, Anpassung übertragen,
//! Auf Standard zurücksetzen.

use super::icons::Icon;
use super::ribbon::{check_list, check_row, demo_popup, icon_button, menu_row, menu_spacing, theme::*};
use super::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Stroke};
use std::sync::Arc;

/// The commands of the graphics toolbar, in Creo's order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GfxItem {
    Refit, ZoomIn, ZoomOut, Repaint, RenderOpts, Style, SavedViews, ViewManager, NormalView, Reorient, Perspective,
    CappedSection, Transparency, DatumFilter, Planes, Axes, Points, Csys, Annotations, SpinCenter, Simulate, SimPause, AnimOptions,
}

impl GfxItem {
    pub const ALL: [GfxItem; 23] = [
        GfxItem::Refit, GfxItem::ZoomIn, GfxItem::ZoomOut, GfxItem::Repaint, GfxItem::RenderOpts, GfxItem::Style, GfxItem::SavedViews,
        GfxItem::ViewManager, GfxItem::NormalView, GfxItem::Reorient, GfxItem::Perspective, GfxItem::CappedSection, GfxItem::Transparency,
        GfxItem::DatumFilter, GfxItem::Planes, GfxItem::Axes, GfxItem::Points, GfxItem::Csys, GfxItem::Annotations, GfxItem::SpinCenter,
        GfxItem::Simulate, GfxItem::SimPause, GfxItem::AnimOptions,
    ];
    pub fn name(self) -> &'static str {
        match self {
            GfxItem::Refit => "Neu einpassen", GfxItem::ZoomIn => "Vergrößern", GfxItem::ZoomOut => "Verkleinern", GfxItem::Repaint => "Bildaufbau",
            GfxItem::RenderOpts => "Renderingoptionen", GfxItem::Style => "Darstellungsstil", GfxItem::SavedViews => "Gespeicherte Orientierungen",
            GfxItem::ViewManager => "Ansichts-Manager", GfxItem::NormalView => "Senkrechte Ansicht", GfxItem::Reorient => "Neu orientieren...",
            GfxItem::Perspective => "Perspektivansicht", GfxItem::CappedSection => "Abgeschlossenen Schnitt anzeigen",
            GfxItem::Transparency => "Transparenzsteuerung", GfxItem::DatumFilter => "Bezugsdarstellungsfilter", GfxItem::Planes => "Ebenen-Anzeige",
            GfxItem::Axes => "Achsenanzeige", GfxItem::Points => "Punkt-Darstellung", GfxItem::Csys => "KSys-Anzeige",
            GfxItem::Annotations => "Anmerkungsanzeige", GfxItem::SpinCenter => "3D-Drehmitte", GfxItem::Simulate => "Simulieren",
            GfxItem::SimPause => "Simulation anhalten", GfxItem::AnimOptions => "Animationsoptionen",
        }
    }
    pub fn icon(self) -> Icon {
        match self {
            GfxItem::Refit => Icon::Refit, GfxItem::ZoomIn => Icon::ZoomIn, GfxItem::ZoomOut => Icon::ZoomOut, GfxItem::Repaint => Icon::Repaint,
            GfxItem::RenderOpts => Icon::RenderOpts, GfxItem::Style => Icon::DisplayStyle, GfxItem::SavedViews => Icon::SavedViews,
            GfxItem::ViewManager => Icon::ViewManager, GfxItem::NormalView => Icon::NormalView, GfxItem::Reorient => Icon::Reorient,
            GfxItem::Perspective => Icon::Perspective, GfxItem::CappedSection => Icon::CappedSection, GfxItem::Transparency => Icon::TranspControl,
            GfxItem::DatumFilter => Icon::DatumDisplay, GfxItem::Planes => Icon::PlaneShow, GfxItem::Axes => Icon::AxisShow,
            GfxItem::Points => Icon::PointShow, GfxItem::Csys => Icon::CsysShow, GfxItem::Annotations => Icon::AnnotShow,
            GfxItem::SpinCenter => Icon::SpinCenter, GfxItem::Simulate => Icon::Simulate, GfxItem::SimPause => Icon::SimPause,
            GfxItem::AnimOptions => Icon::AnimOptions,
        }
    }
    /// commands that open a drop-down (Creo marks them with a small corner triangle)
    fn drops(self) -> bool {
        matches!(self, GfxItem::RenderOpts | GfxItem::Style | GfxItem::SavedViews | GfxItem::Transparency | GfxItem::DatumFilter)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GfxPos { Top, Bottom, Left, Right }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GfxSize { Small, Medium, Large }

impl GfxSize {
    fn px(self) -> f32 { match self { GfxSize::Small => 16.0, GfxSize::Medium => 20.0, GfxSize::Large => 26.0 } }
}

/// What the user customized on the graphics toolbar.
pub struct GfxBar {
    /// shown flag per `GfxItem::ALL` entry
    pub shown: [bool; 23],
    pub pos: GfxPos,
    pub size: GfxSize,
    /// Anpassung übertragen
    pub transfer: bool,
    /// Anmerkungsanzeige: dimensions/annotations drawn in the model
    pub annotations: bool,
    /// right-click menu open at this point
    pub menu_at: Option<egui::Pos2>,
    /// Transparenzsteuerung: percentage for Volumenkörper, Sammelflächen, Trianguliert
    pub transp_pct: [u8; 3],
    /// transparency switched on for Sammelflächen / Trianguliert (Volumenkörper uses `App::transparent`)
    pub transp_other: [bool; 2],
    /// Transparenzsteuerung panel open under this button rect
    pub transp_at: Option<Rect>,
}

impl Default for GfxBar {
    fn default() -> Self {
        GfxBar { shown: [true; 23], pos: GfxPos::Top, size: GfxSize::Medium, transfer: true, annotations: true, menu_at: None, transp_pct: [50, 0, 0], transp_other: [false, false], transp_at: None }
    }
}

impl App {
    pub(super) fn graphics_toolbar(&mut self, ctx: &egui::Context, rect: Rect) {
        let mut normal_view = false;
        let (pos, size) = (self.creo.gfx.pos, self.creo.gfx.size);
        let (pivot, at) = match pos {
            GfxPos::Top => (Align2::CENTER_TOP, pos2(rect.center().x, rect.top() + 6.0)),
            GfxPos::Bottom => (Align2::CENTER_BOTTOM, pos2(rect.center().x, rect.bottom() - 6.0)),
            GfxPos::Left => (Align2::LEFT_CENTER, pos2(rect.left() + 6.0, rect.center().y)),
            GfxPos::Right => (Align2::RIGHT_CENTER, pos2(rect.right() - 6.0, rect.center().y)),
        };
        let vertical = matches!(pos, GfxPos::Left | GfxPos::Right);
        let bar = egui::Area::new(egui::Id::new("gfx_toolbar")).pivot(pivot).fixed_pos(at).order(egui::Order::Middle).show(ctx, |ui| {
            egui::Frame::NONE.fill(RIB_TOP).stroke(Stroke::new(1.0_f32, Color32::from_rgb(163, 163, 163))).corner_radius(0.0).inner_margin(egui::Margin::symmetric(if vertical { 3 } else { 6 }, if vertical { 6 } else { 3 })).show(ui, |ui| {
                // top_down(Min) keeps a vertical bar as narrow as its buttons
                let layout = if vertical { egui::Layout::top_down(egui::Align::Min) } else { egui::Layout::left_to_right(egui::Align::Center) };
                ui.with_layout(layout, |ui| {
                    ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
                    for (k, item) in GfxItem::ALL.into_iter().enumerate() {
                        if self.creo.gfx.shown[k] { self.gfx_button(ui, item, size.px(), &mut normal_view); }
                    }
                    if self.sketch.is_some() { self.sketch_gfx_buttons(ui, size.px()); }
                });
            });
        });
        // right-click on the toolbar opens Creo's customization menu
        if let Some(p) = ctx.input(|i| if i.pointer.secondary_clicked() { i.pointer.interact_pos() } else { None }) {
            if bar.response.rect.contains(p) { self.creo.gfx.menu_at = Some(p); }
        }
        if std::env::var("FREO_DEMO_POPUP").as_deref() == Ok("gfx_customize") && self.creo.gfx.menu_at.is_none() {
            self.creo.gfx.menu_at = Some(bar.response.rect.center_bottom() + vec2(-40.0, 4.0));
        }
        self.gfx_customize_menu(ctx);
        self.transparency_panel(ctx);
        if normal_view { self.normal_view(); }
        self.reorient_dialog(ctx);
    }

    fn gfx_button(&mut self, ui: &mut egui::Ui, item: GfxItem, sz: f32, normal_view: &mut bool) {
        let (enabled, selected) = match item {
            GfxItem::Simulate => (self.tab == Tab::LiveSim && self.sim_study().is_some(), false),
            GfxItem::SimPause => (self.tab == Tab::LiveSim && self.sim_study().is_some(), self.sim.paused),
            GfxItem::RenderOpts | GfxItem::ViewManager | GfxItem::AnimOptions => (false, false),
            GfxItem::Perspective => (true, self.camera.perspective),
            GfxItem::CappedSection => (true, self.section.is_some()),
            GfxItem::Planes => (true, self.show_planes),
            GfxItem::Axes => (true, self.show_axes),
            GfxItem::Points => (true, self.show_points),
            GfxItem::Csys => (true, self.show_csys),
            GfxItem::Annotations => (true, self.creo.gfx.annotations),
            GfxItem::SpinCenter => (true, self.show_spin),
            _ => (true, false),
        };
        let r = icon_button(ui, item.icon(), item.name(), enabled, selected, sz);
        if item.drops() {
            // Creo's corner triangle on drop-down buttons
            let c = r.rect.right_bottom() + vec2(-2.0, -2.0);
            let col = if enabled { Color32::from_rgb(70, 74, 80) } else { Color32::from_rgb(180, 184, 188) };
            ui.painter().add(egui::Shape::convex_polygon(vec![c, c + vec2(-4.0, 0.0), c + vec2(0.0, -4.0)], col, Stroke::NONE));
        }
        let popup = |ui: &mut egui::Ui, key: &str, close: egui::PopupCloseBehavior, f: &mut dyn FnMut(&mut egui::Ui)| {
            let id = ui.id().with(key);
            if r.clicked() { ui.memory_mut(|m| m.toggle_popup(id)); }
            demo_popup(ui, id, key);
            egui::popup_below_widget(ui, id, &r, close, |ui| { menu_spacing(ui); f(ui) });
        };
        match item {
            GfxItem::Refit => if r.clicked() { self.fit_pending = true; },
            GfxItem::ZoomIn => if r.clicked() { self.camera.scale /= 1.25; },
            GfxItem::ZoomOut => if r.clicked() { self.camera.scale *= 1.25; },
            GfxItem::Repaint => if r.clicked() { self.buffers = Arc::new(render::ModelBuffers { generation: 0, solid: vec![], edges: vec![] }); },
            GfxItem::Style => popup(ui, "gt_style", egui::PopupCloseBehavior::CloseOnClick, &mut |ui| self.style_menu(ui)),
            GfxItem::SavedViews => popup(ui, "gt_views", egui::PopupCloseBehavior::CloseOnClick, &mut |ui| { if self.views_menu(ui) { *normal_view = true; } }),
            GfxItem::NormalView => if r.clicked() { *normal_view = true; },
            GfxItem::Reorient => if r.clicked() { self.reorient = Some(Reorient { base: self.camera.rot, h: 0.0, v: 0.0, z: 0.0, name: String::new() }); },
            GfxItem::Perspective => if r.clicked() { self.camera.perspective = !self.camera.perspective; },
            GfxItem::CappedSection => if r.clicked() { self.toggle_section(); },
            GfxItem::Transparency => {
                if r.clicked() { self.creo.gfx.transp_at = if self.creo.gfx.transp_at.is_some() { None } else { Some(r.rect) }; }
                else if self.creo.gfx.transp_at.is_some() || std::env::var("FREO_DEMO_POPUP").is_ok_and(|v| v.starts_with("gt_transp")) { self.creo.gfx.transp_at = Some(r.rect); }
            }
            GfxItem::DatumFilter => popup(ui, "gt_datum", egui::PopupCloseBehavior::CloseOnClickOutside, &mut |ui| {
                check_list(ui, &mut [(Icon::Plane, "Ebenen-Anzeige", &mut self.show_planes), (Icon::Axis, "Achsenanzeige", &mut self.show_axes), (Icon::Point, "Punkt-Darstellung", &mut self.show_points), (Icon::Csys, "KSys-Anzeige", &mut self.show_csys)]);
            }),
            GfxItem::Planes => if r.clicked() { self.show_planes = !self.show_planes; },
            GfxItem::Axes => if r.clicked() { self.show_axes = !self.show_axes; },
            GfxItem::Points => if r.clicked() { self.show_points = !self.show_points; },
            GfxItem::Csys => if r.clicked() { self.show_csys = !self.show_csys; },
            GfxItem::Annotations => if r.clicked() { self.creo.gfx.annotations = !self.creo.gfx.annotations; },
            GfxItem::SpinCenter => if r.clicked() { self.show_spin = !self.show_spin; },
            GfxItem::Simulate => if r.clicked() { self.sim_run_now(); },
            GfxItem::SimPause => if r.clicked() { self.sim.paused = !self.sim.paused; if self.sim.paused { if let Some(j) = self.sim.job.take() { j.shared.cancel.store(true, std::sync::atomic::Ordering::Relaxed); } } },
            GfxItem::RenderOpts | GfxItem::ViewManager | GfxItem::AnimOptions => {}
        }
    }

    /// Transparenzsteuerung: per kind of geometry an on/off switch and the transparency in percent.
    fn transparency_panel(&mut self, ctx: &egui::Context) {
        let Some(anchor) = self.creo.gfx.transp_at else { return };
        // a pick in the percentage list closes that list this frame; remember it was open before
        let inner_open = ctx.memory(|m| m.any_popup_open());
        let panel = egui::Area::new(egui::Id::new("gt_transp")).fixed_pos(anchor.left_bottom() + vec2(0.0, 2.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::menu(ui.style()).inner_margin(4.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 2.0);
                let rows = [(Icon::TranspSolid, "Volumenkörper"), (Icon::TranspQuilt, "Sammelflächen"), (Icon::TranspFacet, "Trianguliert")];
                for (k, (icon, label)) in rows.into_iter().enumerate() {
                    ui.horizontal(|ui| {
                        let on = if k == 0 { self.transparent } else { self.creo.gfx.transp_other[k - 1] };
                        let b = icon_button(ui, icon, &format!("Transparenz für {} ein/aus", label), true, on, 22.0);
                        if !on { ui.painter().rect_stroke(b.rect, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(190, 194, 200)), egui::StrokeKind::Inside); }
                        if b.clicked() {
                            if k == 0 { self.transparent = !on; } else { self.creo.gfx.transp_other[k - 1] = !on; }
                        }
                        let (lr, _) = ui.allocate_exact_size(vec2(120.0, 28.0), egui::Sense::hover());
                        ui.painter().text(pos2(lr.left() + 2.0, lr.center().y), Align2::LEFT_CENTER, label, egui::FontId::proportional(14.5), TEXT);
                        percent_box(ui, k, &mut self.creo.gfx.transp_pct[k]);
                    });
                    if k < 2 { ui.separator(); }
                }
            });
        });
        if panel.response.clicked_elsewhere() && !inner_open && !std::env::var("FREO_DEMO_POPUP").is_ok_and(|v| v.starts_with("gt_transp")) {
            // a click on the toolbar button itself toggles in gfx_button
            if !ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| anchor.contains(p)) { self.creo.gfx.transp_at = None; }
        }
    }

    /// Sketcher additions at the end of the toolbar while sketching.
    fn sketch_gfx_buttons(&mut self, ui: &mut egui::Ui, sz: f32) {
        ui.separator();
        if icon_button(ui, Icon::SketchView, "Skizzenansicht", true, false, sz).clicked() { self.sketch_view(); }
        let r = icon_button(ui, Icon::ConDisplay, "Skizzierer-Anzeigefilter", true, false, sz);
        let r2 = icon_button(ui, Icon::SkCheck, "Skizzierer-Prüfungen", true, false, sz);
        let Some(s) = &mut self.sketch else { return };
        let popup = |ui: &mut egui::Ui, r: &egui::Response, key: &str, f: &mut dyn FnMut(&mut egui::Ui)| {
            let id = ui.id().with(key);
            if r.clicked() { ui.memory_mut(|m| m.toggle_popup(id)); }
            demo_popup(ui, id, key);
            egui::popup_below_widget(ui, id, r, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| { menu_spacing(ui); f(ui) });
        };
        popup(ui, &r, "gt_sk", &mut |ui| {
            check_list(ui, &mut [(Icon::DimDisplay, "Bemaßungsanzeige", &mut s.show_dims), (Icon::ConDisplay, "Bedingungsanzeige", &mut s.show_cons), (Icon::Grid, "Rasterdarstellung", &mut s.grid), (Icon::Vertex, "Eckpunktanzeige", &mut s.show_vertices), (Icon::SkLock, "Sperrenanzeige", &mut s.show_locks)]);
            ui.separator();
            if check_row(ui, Some(s.snap_grid), Some(Icon::Grid), "Am Raster fangen") { s.snap_grid = !s.snap_grid; if s.snap_grid { s.grid = true; } }
        });
        popup(ui, &r2, "gt_skcheck", &mut |ui| {
            check_list(ui, &mut [(Icon::HlOverlap, "Überlappende Geometrie hervorheben", &mut s.hl_overlap), (Icon::HlIntersect, "Sich schneidende Geometrie hervorheben", &mut s.hl_intersect), (Icon::HlBranch, "Geometrieabzweigungen hervorheben", &mut s.hl_branch), (Icon::OpenEnds, "Offene Enden hervorheben", &mut s.open_ends), (Icon::ShadeLoops, "Geschlossene Schleifen schattieren", &mut s.shade)]);
        });
    }

    /// Right-click menu of the graphics toolbar: show or hide each command, Position, Größe,
    /// Anpassung übertragen, Auf Standard zurücksetzen.
    fn gfx_customize_menu(&mut self, ctx: &egui::Context) {
        let Some(at) = self.creo.gfx.menu_at else { return };
        let mut close = false;
        let max_h = (ctx.screen_rect().bottom() - at.y - 16.0).max(200.0);
        let area = egui::Area::new(egui::Id::new("gfx_customize")).fixed_pos(at).order(egui::Order::Foreground).constrain(true).show(ctx, |ui| {
            egui::Frame::menu(ui.style()).inner_margin(2.0).show(ui, |ui| {
                menu_spacing(ui);
                ui.set_min_width(300.0);
                ui.set_max_width(330.0);
                egui::ScrollArea::vertical().max_height(max_h - 160.0).show(ui, |ui| {
                    menu_spacing(ui);
                    for (k, item) in GfxItem::ALL.into_iter().enumerate() {
                        if check_row(ui, Some(self.creo.gfx.shown[k]), Some(item.icon()), item.name()) { self.creo.gfx.shown[k] = !self.creo.gfx.shown[k]; }
                    }
                });
                ui.separator();
                let pos = self.creo.gfx.pos;
                if let Some(p) = submenu(ui, "Position", &[("Oben", pos == GfxPos::Top), ("Unten", pos == GfxPos::Bottom), ("Links", pos == GfxPos::Left), ("Rechts", pos == GfxPos::Right)]) {
                    self.creo.gfx.pos = [GfxPos::Top, GfxPos::Bottom, GfxPos::Left, GfxPos::Right][p];
                    close = true;
                }
                let size = self.creo.gfx.size;
                if let Some(s) = submenu(ui, "Größe", &[("Klein", size == GfxSize::Small), ("Mittel", size == GfxSize::Medium), ("Groß", size == GfxSize::Large)]) {
                    self.creo.gfx.size = [GfxSize::Small, GfxSize::Medium, GfxSize::Large][s];
                    close = true;
                }
                ui.separator();
                if check_row(ui, Some(self.creo.gfx.transfer), None, "Anpassung übertragen") { self.creo.gfx.transfer = !self.creo.gfx.transfer; }
                if menu_row(ui, None, "Auf Standard zurücksetzen", "", true, false).clicked() {
                    self.creo.gfx = GfxBar::default();
                    close = true;
                }
            });
        });
        let on_sub = ctx.pointer_hover_pos().is_some_and(|p| ctx.layer_id_at(p).is_some_and(|l| l.order == egui::Order::Tooltip));
        if close || (area.response.clicked_elsewhere() && !on_sub && !ctx.input(|i| i.pointer.secondary_clicked())) {
            self.creo.gfx.menu_at = None;
            ctx.data_mut(|d| for l in ["Position", "Größe"] { d.remove::<bool>(submenu_id(l)); });
        }
        else { self.creo.gfx.menu_at = Some(at); }
    }
}

/// A "Label ▸" row whose choices fold out while it is hovered; they stay open until the pointer
/// rests on another row of the menu. Returns the chosen index.
fn submenu(ui: &mut egui::Ui, label: &str, items: &[(&str, bool)]) -> Option<usize> {
    let r = menu_row(ui, None, label, "▸", true, false);
    let open_id = submenu_id(label);
    let was = ui.ctx().data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
    let elsewhere = ui.ctx().pointer_hover_pos().is_some_and(|p| ui.max_rect().contains(p) && !r.rect.contains(p));
    let open = r.hovered() || (was && !elsewhere);
    ui.ctx().data_mut(|d| d.insert_temp(open_id, open));
    let mut chosen = None;
    if open {
        // open to the right, or to the left when the window edge is too close
        const SUB_W: f32 = 160.0;
        let at = if r.rect.right() + SUB_W > ui.ctx().screen_rect().right() { r.rect.left_top() - vec2(SUB_W, 0.0) } else { r.rect.right_top() };
        egui::Area::new(open_id.with("area")).fixed_pos(at).order(egui::Order::Tooltip).show(ui.ctx(), |ui| {
            egui::Frame::menu(ui.style()).inner_margin(2.0).show(ui, |ui| {
                menu_spacing(ui);
                ui.set_min_width(SUB_W - 6.0);
                ui.set_max_width(SUB_W - 6.0);
                for (k, (n, on)) in items.iter().enumerate() {
                    if menu_row(ui, None, n, "", true, *on).clicked() { chosen = Some(k); }
                }
            });
        });
    }
    chosen
}

fn submenu_id(label: &str) -> egui::Id { egui::Id::new(("gfx_sub", label)) }

/// Creo's transparency field: glass icon, "50%", and a ▾ that opens a 0–100 % slider.
fn percent_box(ui: &mut egui::Ui, k: usize, pct: &mut u8) {
    let (r, resp) = ui.allocate_exact_size(vec2(130.0, 30.0), egui::Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 0.0, Color32::WHITE);
    p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, if resp.hovered() { Color32::from_rgb(120, 160, 200) } else { Color32::from_rgb(190, 194, 200) }), egui::StrokeKind::Inside);
    super::icons::paint(p, Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(20.0, 20.0)), Icon::Transparent, true);
    p.text(pos2(r.left() + 32.0, r.center().y), Align2::LEFT_CENTER, format!("{}%", pct), egui::FontId::proportional(14.5), TEXT);
    p.line_segment([pos2(r.right() - 24.0, r.top() + 1.0), pos2(r.right() - 24.0, r.bottom() - 1.0)], Stroke::new(1.0_f32, Color32::from_rgb(214, 218, 222)));
    let c = pos2(r.right() - 12.0, r.center().y);
    p.add(egui::Shape::convex_polygon(vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 2.5)], Color32::from_rgb(70, 74, 80), Stroke::NONE));
    let id = ui.id().with(("transp_pct", k));
    if resp.clicked() { ui.memory_mut(|m| m.toggle_popup(id)); }
    if k == 0 { demo_popup(ui, id, "gt_transp_slider"); }
    egui::popup_below_widget(ui, id, &resp, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.spacing_mut().slider_width = r.width() - 16.0;
        ui.add_space(4.0);
        // the field above shows the value, the slider only the position
        ui.add(egui::Slider::new(pct, 0..=100).show_value(false));
        ui.add_space(2.0);
    });
}
