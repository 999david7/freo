//! Modellraster (Anmerkungen erstellen ▸ Kombinations-Zustände ▾): a labelled grid laid over the
//! model in the current view, set up in Creo's two-tab dialog (Raster / Einstellungen).

use super::creo::{check_box, creo_check, dialog_header, fieldset};
use super::icons::{self, Icon};
use super::ribbon::theme::*;
use super::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke};

/// The grid as applied to the model.
#[derive(Clone)]
pub struct ModelGrid {
    pub shown: bool,
    pub origin: V3,
    /// Abstand: false = Alle (one spacing), true = Nach Achse
    pub per_axis: bool,
    pub all: f64,
    pub xyz: [f64; 3],
    pub offset: f64,
    pub decimals: usize,
    pub neg_prefix: String,
    pub balloon: bool,
    /// Zusätzlichen Text anzeigen (suffixes)
    pub extra_text: bool,
    pub prefix: [String; 3],
    pub suffix: [String; 3],
}

impl Default for ModelGrid {
    fn default() -> Self {
        ModelGrid {
            shown: false, origin: V3::ZERO, per_axis: false, all: 1.5, xyz: [1.5; 3], offset: 0.0, decimals: 0, neg_prefix: "-".into(),
            balloon: true, extra_text: false, prefix: Default::default(), suffix: Default::default(),
        }
    }
}

impl ModelGrid {
    fn spacing(&self, axis: usize) -> f64 { if self.per_axis { self.xyz[axis] } else { self.all } }
    fn label(&self, axis: usize, v: f64) -> String {
        let num = format!("{:.*}", self.decimals, v.abs());
        let sign = if v < -1e-9 && num.chars().any(|c| c.is_ascii_digit() && c != '0') { self.neg_prefix.as_str() } else { "" };
        let suffix = if self.extra_text { self.suffix[axis].as_str() } else { "" };
        format!("{}{}{}{}", self.prefix[axis], sign, num, suffix)
    }
}

/// Dialog state: the grid being edited plus the text of its number fields.
pub struct GridDialog {
    pub draft: ModelGrid,
    /// grid before the dialog opened (Abbrechen restores it)
    pub before: ModelGrid,
    pub tab: u8,
    all: String,
    xyz: [String; 3],
    offset: String,
    decimals: String,
}

fn num(v: f64) -> String { format!("{:.2}", v) }

impl GridDialog {
    pub fn new(g: &ModelGrid) -> Self {
        GridDialog {
            draft: g.clone(), before: g.clone(), tab: 0, all: num(g.all), xyz: g.xyz.map(num),
            offset: if g.offset == 0.0 { String::new() } else { num(g.offset) }, decimals: g.decimals.to_string(),
        }
    }
}

fn parse(t: &str) -> Option<f64> { t.trim().replace(',', ".").parse::<f64>().ok() }

/// Text field the size of Creo's (a white box about 120 × 46 px).
fn field(ui: &mut egui::Ui, text: &mut String, enabled: bool, hint: &str) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| ui.add_sized(vec2(110.0, 34.0), egui::TextEdit::singleline(text).margin(vec2(8.0, 8.0)).hint_text(hint))).inner
}

/// Grey check box with a label, as Creo shows options that cannot be changed.
fn fixed_check(ui: &mut egui::Ui, on: bool, label: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(220.0, 28.0), Sense::hover());
    let b = Rect::from_center_size(pos2(r.left() + 10.0, r.center().y), vec2(15.0, 15.0));
    check_box(ui.painter(), b, on);
    ui.painter().rect_filled(b, 1.0, Color32::from_rgba_unmultiplied(240, 240, 240, 150));
    ui.painter().text(pos2(r.left() + 30.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(14.0), DIM_TEXT);
}

/// Square button with only an icon (Ursprung, Anzeigen).
fn icon_box(ui: &mut egui::Ui, icon: Icon, enabled: bool, tip: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(44.0, 34.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    p.rect_filled(r, 2.0, if !enabled { Color32::from_rgb(236, 237, 238) } else if resp.hovered() { HOVER_BG } else { Color32::from_rgb(248, 248, 248) });
    p.rect_stroke(r, 2.0, Stroke::new(1.0_f32, Color32::from_rgb(190, 194, 200)), egui::StrokeKind::Inside);
    icons::paint(p, Rect::from_center_size(r.center(), vec2(20.0, 20.0)), icon, enabled);
    resp.on_hover_text(tip)
}

fn button(ui: &mut egui::Ui, t: &str, w: f32) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(t).size(14.0)).min_size(vec2(w, 32.0)))
}

impl App {
    pub fn open_model_grid(&mut self) {
        if self.creo.grid_dialog.is_none() { self.creo.grid_dialog = Some(GridDialog::new(&self.creo.grid)); }
    }

    /// Ursprung: a selected datum point or coordinate system becomes the grid origin.
    fn grid_origin_from_selection(&mut self) -> Option<V3> {
        let r = self.displayed();
        let from_pick = self.selection.iter().find_map(|p| match p {
            Pick::Point(id) => r.points.iter().find(|q| q.id == *id).map(|q| q.p),
            _ => None,
        });
        let from_tree = || self.tree_selection.iter().find_map(|id| {
            if *id == CSYS_ID { return Some(V3::ZERO); }
            r.csys.iter().find(|c| c.id == *id).map(|c| c.frame.origin).or_else(|| r.points.iter().find(|q| q.id == *id).map(|q| q.p))
        });
        from_pick.or_else(from_tree)
    }

    pub fn model_grid_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.creo.grid_dialog.take() else { return };
        let (mut open, mut ok, mut cancel, mut pick_origin) = (true, false, false, false);
        egui::Window::new("Modellraster").id(egui::Id::new("model_grid")).title_bar(false).collapsible(false).resizable(false).default_pos(pos2(460.0, 110.0)).show(ctx, |ui| {
            if dialog_header(ui, "Modellraster") { open = false; }
            ui.set_width(340.0);
            ui.add_space(10.0);
            // tab headers
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (k, t) in ["Raster", "Einstellungen"].into_iter().enumerate() {
                    let font = FontId::proportional(14.5);
                    let w = ui.fonts(|f| f.layout_no_wrap(t.to_string(), font.clone(), TEXT).size().x) + 28.0;
                    let sel = d.tab == k as u8;
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 34.0), Sense::click());
                    let p = ui.painter();
                    p.rect_filled(r, 0.0, if sel { ui.visuals().window_fill } else { Color32::from_rgb(232, 234, 236) });
                    p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(196, 200, 206)), egui::StrokeKind::Inside);
                    p.text(r.center(), Align2::CENTER_CENTER, t, font, TEXT);
                    if resp.clicked() { d.tab = k as u8; }
                }
            });
            egui::Frame::NONE.stroke(Stroke::new(1.0_f32, Color32::from_rgb(196, 200, 206))).inner_margin(10.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 6.0;
                if d.tab == 0 { self.grid_tab(ui, &mut d, &mut pick_origin); } else { grid_settings_tab(ui, &mut d); }
            });
            ui.add_space(12.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if button(ui, "Abbrechen", 110.0).clicked() { cancel = true; }
                    if button(ui, "OK", 110.0).clicked() { ok = true; }
                });
            });
        });
        if pick_origin {
            match self.grid_origin_from_selection() {
                Some(o) => { d.draft.origin = o; self.say("Ursprung des Modellrasters gesetzt.", MsgKind::Info); }
                None => self.say("Einen Bezugspunkt oder ein Koordinatensystem wählen, dann Ursprung klicken.", MsgKind::Prompt),
            }
        }
        if ok { self.creo.grid = d.draft.clone(); }
        else if cancel || !open { self.creo.grid = d.before.clone(); }
        if !(ok || cancel || !open) { self.creo.grid_dialog = Some(d); }
    }

    fn grid_tab(&mut self, ui: &mut egui::Ui, d: &mut GridDialog, pick_origin: &mut bool) {
        fieldset(ui, "Ursprung", |ui| {
            if icon_box(ui, Icon::SkSelect, true, "Ursprung wählen: einen Bezugspunkt oder ein KSys markieren, dann hier klicken").clicked() { *pick_origin = true; }
        });
        fieldset(ui, "Anzeige", |ui| {
            ui.horizontal(|ui| {
                if button(ui, "Anzeigen", 120.0).clicked() { d.draft.shown = true; }
                if button(ui, "Löschen", 120.0).clicked() { d.draft.shown = false; }
            });
            fixed_check(ui, true, "Raster");
            fixed_check(ui, true, "Rasterbeschriftungen");
            ui.label(RichText::new("Anzeigen/löschen nach:").size(14.0));
            ui.add_enabled_ui(false, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    for (i, t) in ["Ansicht", "Linie", "Blatt"].into_iter().enumerate() { let _ = ui.radio(i == 0, RichText::new(t).size(14.0)); ui.add_space(8.0); }
                });
            });
        });
        fieldset(ui, "Abstand", |ui| {
            ui.horizontal(|ui| {
                if ui.radio(!d.draft.per_axis, RichText::new("Alle").size(14.0)).clicked() { d.draft.per_axis = false; }
                ui.add_space(40.0);
                if ui.radio(d.draft.per_axis, RichText::new("Nach Achse").size(14.0)).clicked() { d.draft.per_axis = true; }
            });
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        if field(ui, &mut d.all, !d.draft.per_axis, "").changed() { if let Some(v) = parse(&d.all).filter(|v| *v > 0.0) { d.draft.all = v; } }
                    });
                });
                ui.add_space(14.0);
                ui.vertical(|ui| {
                    for (k, a) in ["X:", "Y:", "Z:"].into_iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(a).size(14.0));
                            if field(ui, &mut d.xyz[k], d.draft.per_axis, "").changed() { if let Some(v) = parse(&d.xyz[k]).filter(|v| *v > 0.0) { d.draft.xyz[k] = v; } }
                        });
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Anzeigen:").size(14.0));
                icon_box(ui, Icon::SkSelect, false, "Abstand aus einer Bemaßung übernehmen");
            });
        });
        if button(ui, "Raster löschen", 130.0).on_hover_text("Raster entfernen und alle Einstellungen zurücksetzen").clicked() {
            *d = GridDialog { before: d.before.clone(), tab: d.tab, ..GridDialog::new(&ModelGrid::default()) };
        }
    }

    /// Modellraster over the model: lines in the model plane facing the view most, through the
    /// origin, across the model, numbered in balloons at the line ends.
    pub fn draw_model_grid(&self, painter: &egui::Painter, to_screen: &dyn Fn(V3) -> Option<Pos2>) {
        let g = self.creo.grid_dialog.as_ref().map(|d| &d.draft).unwrap_or(&self.creo.grid);
        if !g.shown { return; }
        let mut bb = self.displayed().topo.bbox;
        if bb.is_empty() { bb.add(g.origin - V3::splat(50.0)); bb.add(g.origin + V3::splat(50.0)); }
        // the axis most along the view direction is the plane normal
        let f = self.camera.rot * V3::Z;
        let n = (0..3).max_by(|a, b| f[*a].abs().partial_cmp(&f[*b].abs()).unwrap()).unwrap();
        let (a, b) = match n { 0 => (1, 2), 1 => (0, 2), _ => (0, 1) };
        let line_col = Color32::from_rgba_unmultiplied(40, 46, 54, 150);
        let mut total = 0usize;
        let mut lines: Vec<(usize, f64, Pos2, Pos2)> = vec![];
        for (u, w) in [(a, b), (b, a)] {
            let s = g.spacing(u);
            let (lo, hi) = (bb.min[u] - s - g.origin[u], bb.max[u] + s - g.origin[u]);
            let (k0, k1) = ((lo / s).ceil() as i64, (hi / s).floor() as i64);
            total += (k1 - k0 + 1).max(0) as usize;
            if total > 400 { break; }
            let sw = g.spacing(w);
            let (w0, w1) = (bb.min[w] - sw, bb.max[w] + sw);
            for k in k0..=k1 {
                let v = k as f64 * s;
                let mut p0 = g.origin;
                p0[u] += v;
                p0[w] = w0;
                let mut p1 = p0;
                p1[w] = w1;
                if let (Some(s0), Some(s1)) = (to_screen(p0), to_screen(p1)) { lines.push((u, v, s0, s1)); }
            }
        }
        if total > 400 {
            if let Some(o) = to_screen(g.origin) { painter.text(o + vec2(10.0, -10.0), Align2::LEFT_BOTTOM, "Rasterabstand zu klein für die Anzeige", FontId::proportional(13.0), Color32::from_rgb(190, 40, 30)); }
            return;
        }
        // labels that would overlap the previous one of the same axis are left out
        let mut last: [Option<Pos2>; 3] = [None; 3];
        for (u, v, s0, s1) in lines {
            painter.line_segment([s0, s1], Stroke::new(1.0_f32, line_col));
            // label at the upper (or left) end, pushed out along the line
            let (end, start) = if (s0.y, s0.x) < (s1.y, s1.x) { (s0, s1) } else { (s1, s0) };
            let dir = (end - start).normalized();
            let text = g.label(u, v);
            let font = FontId::proportional(12.0);
            let tw = painter.layout_no_wrap(text.clone(), font.clone(), TEXT).size().x;
            let r = (tw / 2.0 + 5.0).max(11.0);
            let c = end + dir * (r + 3.0 + g.offset as f32);
            if last[u].is_some_and(|l| (l - c).length() < 2.0 * r + 3.0) { continue; }
            last[u] = Some(c);
            if g.balloon {
                painter.circle(c, r, Color32::from_rgba_unmultiplied(255, 255, 255, 230), Stroke::new(1.0_f32, Color32::from_rgb(40, 46, 54)));
            }
            painter.text(c, Align2::CENTER_CENTER, text, font, Color32::from_rgb(20, 24, 30));
        }
    }
}

fn grid_settings_tab(ui: &mut egui::Ui, d: &mut GridDialog) {
    fieldset(ui, "Setup-Optionen", |ui| {
        egui::Grid::new("grid_setup").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
            let lab = |ui: &mut egui::Ui, t: &str, en: bool| { ui.label(RichText::new(t).size(14.0).color(if en { TEXT } else { DIM_TEXT })); };
            lab(ui, "Ballonradius:", false);
            field(ui, &mut "0.000000".to_string(), false, "");
            ui.end_row();
            lab(ui, "Versatzabstand:", true);
            if field(ui, &mut d.offset, true, "").changed() { d.draft.offset = parse(&d.offset).unwrap_or(0.0); }
            ui.end_row();
            lab(ui, "Dezimalstellen:", true);
            if field(ui, &mut d.decimals, true, "").changed() { if let Ok(n) = d.decimals.trim().parse::<usize>() { d.draft.decimals = n.min(6); } }
            ui.end_row();
            lab(ui, "Negatives Präfix:", true);
            field(ui, &mut d.draft.neg_prefix, true, "");
            ui.end_row();
            for (t, v) in [("Textorientierung:", "Horizontal"), ("Textposition:", "Zentriert")] {
                lab(ui, t, false);
                ui.add_enabled_ui(false, |ui| { egui::ComboBox::from_id_salt(t).width(110.0).selected_text(v).show_ui(ui, |_| {}); });
                ui.end_row();
            }
            lab(ui, "Ballon anzeigen:", true);
            egui::ComboBox::from_id_salt("grid_balloon").width(110.0).selected_text(if d.draft.balloon { "Ja" } else { "Nein" }).show_ui(ui, |ui| {
                ui.selectable_value(&mut d.draft.balloon, true, "Ja");
                ui.selectable_value(&mut d.draft.balloon, false, "Nein");
            });
            ui.end_row();
        });
    });
    fieldset(ui, "Beschriftungstext", |ui| {
        creo_check(ui, &mut d.draft.extra_text, "Zusätzlichen Text anzeigen");
        ui.add_space(4.0);
        egui::Grid::new("grid_text").num_columns(3).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
            ui.label(RichText::new("Achse").size(14.0));
            ui.vertical_centered(|ui| ui.label(RichText::new("Präfix").size(14.0)));
            ui.vertical_centered(|ui| ui.label(RichText::new("Suffix").size(14.0)));
            ui.end_row();
            for (k, a) in ["X:", "Y:", "Z:"].into_iter().enumerate() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(RichText::new(a).size(14.0)));
                field(ui, &mut d.draft.prefix[k], true, "");
                field(ui, &mut d.draft.suffix[k], d.draft.extra_text, ["X", "Y", "Z"][k]);
                ui.end_row();
            }
        });
    });
}
