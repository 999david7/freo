//! Navigator pane: Creo-style Model Tree and Folder Browser.

use super::icons::{self, Icon};
use super::ribbon::{check_row, icon_button, menu_entry, menu_spacing, theme::*};
use super::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Response, RichText, Sense, Stroke, StrokeKind};

#[derive(Clone)]
enum Row {
    Part,
    /// Creo 10's "Konstruktionselemente" folder (bodies)
    Design,
    Body,
    Feature(usize),
    Section { fid: u64, slot: Slot, label: String },
    Insert,
    /// the feature being created, below the insert line with Creo's yellow star
    Pending,
}

const ROW_H: f32 = 23.0;

impl App {
    pub fn navigator(&mut self, ui: &mut egui::Ui) {
        super::creo::nav_header(ui, self);
        ui.add_space(4.0);
        match self.nav_tab {
            NavTab::Tree if self.in_asm() => {
                if let Some(name) = self.drw.as_ref().filter(|d| !d.active).map(|d| d.data.name.clone()) {
                    let (r, _) = Self::paint_row(ui, 0, Icon::ReturnDwg, &format!("{}.DRW", name), Color32::from_rgb(30, 100, 200), false, false, None);
                    if r.on_hover_text("Zur Zeichnung zurückkehren").clicked() { self.drawing_return(); }
                }
                self.asm_tree(ui)
            }
            NavTab::Tree => {
                if let Some(name) = self.drw.as_ref().filter(|d| !d.active).map(|d| d.data.name.clone()) {
                    let (r, _) = Self::paint_row(ui, 0, Icon::ReturnDwg, &format!("{}.DRW", name), Color32::from_rgb(30, 100, 200), false, false, None);
                    if r.on_hover_text("Zur Zeichnung zurückkehren").clicked() { self.drawing_return(); }
                }
                if let Some(name) = self.editing_from_asm().and(self.asm.as_ref()).map(|a| a.data.name.clone()) {
                    let (r, _) = Self::paint_row(ui, 0, Icon::ReturnAsm, &format!("{}.ASM", name), Color32::from_rgb(30, 100, 200), false, false, None);
                    if r.on_hover_text("Zur Baugruppe zurückkehren").clicked() { self.asm_return(); }
                }
                self.sim_tree_panel(ui);
                self.model_tree(ui)
            }
            NavTab::Browser => self.folder_browser(ui),
        }
    }

    fn tree_rows(&self) -> Vec<(usize, Row)> {
        let mut rows = vec![(0, Row::Part), (1, Row::Design)];
        if self.creo.design_open { rows.push((2, Row::Body)); }
        let n = self.data.features.len();
        let q = self.creo.tree_filter.trim().to_lowercase();
        let pending = self.editor.as_ref().is_some_and(|e| e.is_new);
        for (i, f) in self.data.features.iter().enumerate() {
            if self.data.insert_index == Some(i) && q.is_empty() {
                rows.push((1, Row::Insert));
                if pending { rows.push((1, Row::Pending)); }
            }
            if f.suppressed && !self.show_suppressed { continue; }
            if !q.is_empty() && !f.name.to_lowercase().contains(&q) { continue; }
            rows.push((1, Row::Feature(i)));
            if self.expanded.contains(&f.id) {
                let kids: Vec<(Slot, String)> = match &f.kind {
                    FeatureKind::Extrude(_) | FeatureKind::Revolve(_) | FeatureKind::Sketch(_) => vec![(Slot::Main, "Querschnitt 1".into())],
                    FeatureKind::Sweep(_) => vec![(Slot::Trajectory, "Leitkurve 1".into()), (Slot::Section, "Querschnitt 1".into())],
                    FeatureKind::Helix(_) => vec![(Slot::Trajectory, "Profil 1".into()), (Slot::Section, "Querschnitt 1".into())],
                    FeatureKind::Rib(_) => vec![(Slot::Main, "Querschnitt 1".into())],
                    FeatureKind::Blend(d) => (0..d.sections.len()).map(|k| (Slot::Blend(k), format!("Querschnitt {}", k + 1))).collect(),
                    _ => vec![],
                };
                for (slot, label) in kids { rows.push((2, Row::Section { fid: f.id, slot, label })); }
            }
        }
        if q.is_empty() && (self.data.insert_index.is_none() || self.data.insert_index == Some(n)) {
            rows.push((1, Row::Insert));
            if pending { rows.push((1, Row::Pending)); }
        }
        rows
    }

    pub(crate) fn paint_row(ui: &mut egui::Ui, depth: usize, icon: Icon, text: &str, color: Color32, strong: bool, selected: bool, expander: Option<bool>) -> (Response, Option<Response>) {
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::click_and_drag());
        super::ui::no_hand(ui.ctx(), resp.id);
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, 0.0, SEL_BG);
            p.rect_stroke(rect, 0.0, Stroke::new(1.0_f32, SEL_BORDER), StrokeKind::Inside);
        } else if resp.hovered() {
            p.rect_filled(rect, 0.0, Color32::from_rgb(232, 241, 250));
        }
        let x = rect.left() + 4.0 + depth as f32 * 16.0;
        let mut exp_resp = None;
        if let Some(open) = expander {
            let er = Rect::from_center_size(pos2(x + 5.0, rect.center().y), vec2(12.0, 12.0));
            let r = ui.interact(er, resp.id.with("exp"), Sense::click());
            let c = Color32::from_rgb(90, 100, 112);
            let pts = if open { vec![pos2(er.left() + 2.0, er.top() + 4.0), pos2(er.right() - 2.0, er.top() + 4.0), pos2(er.center().x, er.bottom() - 3.0)] }
                else { vec![pos2(er.left() + 4.0, er.top() + 2.0), pos2(er.right() - 3.0, er.center().y), pos2(er.left() + 4.0, er.bottom() - 2.0)] };
            ui.painter().add(egui::Shape::convex_polygon(pts, c, Stroke::NONE));
            exp_resp = Some(r);
        }
        icons::paint(ui.painter(), Rect::from_center_size(pos2(x + 21.0, rect.center().y), vec2(16.0, 16.0)), icon, true);
        let font = if strong { FontId::new(12.5, egui::FontFamily::Proportional) } else { FontId::proportional(12.5) };
        let pos = pos2(x + 33.0, rect.center().y);
        ui.painter().text(pos, Align2::LEFT_CENTER, text, font.clone(), color);
        if strong { ui.painter().text(pos + vec2(0.4, 0.0), Align2::LEFT_CENTER, text, font, color); }
        (resp, exp_resp)
    }

    fn model_tree(&mut self, ui: &mut egui::Ui) {
        let (cols, c1, c2) = self.tree_header(ui);
        let rows = self.tree_rows();
        let rolled = self.result.rolled_back.clone();
        let editing = self.editor.as_ref().map(|e| e.feature.id);
        self.tree_hover = None;
        let mut row_rects: Vec<(Rect, Option<u64>)> = vec![];
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (depth, row) in rows {
                match row {
                    Row::Part => {
                        let (r, _) = Self::paint_row(ui, depth, Icon::Part, &format!("{}.PRT", self.data.name), TEXT, false, false, None);
                        if cols {
                            let param = |n: &str| self.data.parameters.iter().find(|p| p.name.eq_ignore_ascii_case(n)).map(|p| p.expr.trim_matches('"').to_string()).unwrap_or_else(|| "-".into());
                            ui.painter().text(pos2(r.rect.left() + c1 + 8.0, r.rect.center().y), Align2::LEFT_CENTER, param("BENENNUNG"), FontId::proportional(12.5), TEXT);
                            ui.painter().text(pos2(r.rect.left() + c2 + 8.0, r.rect.center().y), Align2::LEFT_CENTER, param("SACHNUMMER"), FontId::proportional(12.5), TEXT);
                        }
                        row_rects.push((r.rect, None));
                    }
                    Row::Design => {
                        let (r, exp) = Self::paint_row(ui, depth.saturating_sub(1), Icon::FamilyTable, "Konstruktionselemente", TEXT, false, false, Some(self.creo.design_open));
                        if exp.map(|e| e.clicked()).unwrap_or(false) || r.double_clicked() { self.creo.design_open = !self.creo.design_open; }
                        row_rects.push((r.rect, None));
                    }
                    Row::Body => {
                        // every body; the default one (where new features go) in bold, a click makes a body the default
                        let bodies = if self.result.bodies.is_empty() { vec![(0, self.result.topo.polys.len())] } else { self.result.bodies.clone() };
                        for (bid, n) in bodies {
                            let name = if bid == 0 { "Körper 1".to_string() } else { self.data.feature(bid).map(|f| f.name.clone()).unwrap_or_else(|| "Körper".into()) };
                            let label = if n == 0 { format!("{} (leer)", name) } else { name.clone() };
                            let def = self.data.default_body == bid;
                            let (r, _) = Self::paint_row(ui, depth, Icon::Body, &label, if n > 0 { TEXT } else { DIM_TEXT }, def, false, None);
                            row_rects.push((r.rect, None));
                            let r = r.on_hover_text(if def { "Standardkörper — neue KEs kommen hierher" } else { "Klicken: als Standardkörper festlegen" });
                            if r.clicked() && !def && !self.busy() { self.data.default_body = bid; self.dirty = true; self.say(format!("{} ist der Standardkörper.", name), MsgKind::Info); }
                        }
                    }
                    Row::Insert => {
                        // Creo 10 shows the insert position as a green bar
                        let (rr, r) = ui.allocate_exact_size(vec2(ui.available_width(), 12.0), Sense::click_and_drag());
                        super::ui::no_hand(ui.ctx(), r.id);
                        ui.painter().line_segment([pos2(rr.left() + 6.0, rr.center().y), pos2(rr.right() - 4.0, rr.center().y)], Stroke::new(2.0_f32, Color32::from_rgb(70, 170, 40)));
                        ui.painter().line_segment([pos2(rr.left() + 6.0, rr.top() + 2.0), pos2(rr.left() + 6.0, rr.bottom() - 2.0)], Stroke::new(2.0_f32, Color32::from_rgb(70, 170, 40)));
                        if r.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical); }
                        let r = r.on_hover_text("Hier einfügen — ziehen, um neue KEs weiter vorne im Modell einzufügen");
                        if r.drag_started() { self.insert_drag = Some(r.rect.center().y); }
                        if r.dragged() { if let Some(p) = r.interact_pointer_pos() { self.insert_drag = Some(p.y); } }
                        if r.drag_stopped() {
                            if let Some(y) = self.insert_drag.take() {
                                let target = row_rects.iter().rev().find(|(rr, id)| id.is_some() && rr.center().y <= y).and_then(|x| x.1);
                                let last = self.data.features.last().map(|f| f.id);
                                if target == last { self.set_insert_after(None); } else if target.is_some() { self.set_insert_after(target); }
                            }
                        }
                    }
                    Row::Feature(i) => {
                        let f = self.data.features[i].clone();
                        if let Some((rid, mut name)) = self.renaming.clone() {
                            if rid == f.id {
                                ui.horizontal(|ui| {
                                    ui.add_space(depth as f32 * 16.0 + 30.0);
                                    let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(140.0));
                                    r.request_focus();
                                    if r.lost_focus() { self.renaming = None; if ui.input(|i| i.key_pressed(egui::Key::Enter)) { self.rename(rid, &name); } }
                                    else { self.renaming = Some((rid, name)); }
                                });
                                continue;
                            }
                        }
                        let err = self.result.errors.get(&f.id).cloned();
                        let mut color = TEXT;
                        let mut icon = icons::for_feature(&f.kind);
                        if f.suppressed { color = Color32::from_rgb(140, 146, 154); }
                        else if rolled.contains(&f.id) { color = Color32::from_rgb(165, 170, 176); }
                        else if err.is_some() { color = Color32::from_rgb(200, 36, 28); icon = Icon::Failed; }
                        if editing == Some(f.id) { color = Color32::from_rgb(20, 100, 210); }
                        if f.hidden { color = Color32::from_rgb(130, 136, 144); }
                        let expandable = f.kind.has_sketch();
                        let sel = self.tree_selection.contains(&f.id) || self.tree_hover_ext == Some(f.id);
                        let (r, exp) = Self::paint_row(ui, depth, icon, &f.name, color, false, sel, if expandable { Some(self.expanded.contains(&f.id)) } else { None });
                        if f.suppressed { ui.painter().rect_filled(Rect::from_center_size(pos2(r.rect.left() + 4.0 + depth as f32 * 16.0 + 28.0, r.rect.center().y + 5.0), vec2(5.0, 5.0)), 0.0, Color32::BLACK); }
                        row_rects.push((r.rect, Some(f.id)));
                        if let Some(e) = exp { if e.clicked() { if !self.expanded.remove(&f.id) { self.expanded.insert(f.id); } continue; } }
                        let r = match &err { Some(e) => r.on_hover_text(format!("{} — fehlgeschlagen: {}", f.kind.type_name(), e)), None => r.on_hover_text(f.kind.type_name()) };
                        if r.hovered() { self.tree_hover = Some(f.id); }
                        if r.clicked() { let add = ui.input(|i| i.modifiers.shift || i.modifiers.command); self.tree_click(f.id, add); }
                        if r.double_clicked() { self.edit_feature(f.id); }
                        r.context_menu(|ui| self.tree_context_menu(ui, &f));
                    }
                    Row::Pending => {
                        let Some(ed) = &self.editor else { continue };
                        let (icon, name) = (icons::for_feature(&ed.feature.kind), ed.feature.name.clone());
                        let (r, _) = Self::paint_row(ui, depth, icon, &format!("   {}", name), TEXT, false, false, None);
                        // yellow star in front of the name: not yet created
                        let c = pos2(r.rect.left() + 4.0 + depth as f32 * 16.0 + 38.0, r.rect.center().y - 3.0);
                        for k in 0..4 {
                            let a = k as f32 * std::f32::consts::FRAC_PI_4;
                            let d = vec2(a.cos(), a.sin()) * 3.6;
                            ui.painter().line_segment([c - d, c + d], Stroke::new(1.3_f32, Color32::from_rgb(222, 178, 0)));
                        }
                        r.on_hover_text(format!("{} (wird erzeugt)", ed.feature.kind.type_name()));
                    }
                    Row::Section { fid, slot, label } => {
                        let (r, _) = Self::paint_row(ui, depth, Icon::Sketch, &label, TEXT, false, false, None);
                        row_rects.push((r.rect, Some(fid)));
                        if r.hovered() { self.tree_hover = Some(fid); }
                        if r.clicked() { self.tree_click(fid, false); }
                        let r = r.on_hover_text("Doppelklick, um die Skizze zu editieren");
                        let mut edit = r.double_clicked();
                        r.context_menu(|ui| { menu_spacing(ui); if menu_entry(ui, Icon::Sketch, "Skizze editieren", true, false).clicked() { edit = true; ui.close_menu(); } });
                        if edit && self.editor.is_none() {
                            self.edit_feature(fid);
                            if self.editor.is_some() { self.begin_sketch(slot); }
                        }
                    }
                }
            }
            self.annot_tree_rows(ui);
            if cols {
                let top = ui.min_rect().top();
                let bot = ui.min_rect().bottom().max(ui.clip_rect().bottom());
                let left = ui.min_rect().left();
                for x in [c1, c2] { ui.painter().line_segment([pos2(left + x, top), pos2(left + x, bot)], Stroke::new(1.0_f32, Color32::from_rgb(211, 214, 215))); }
            }
            if let Some(y) = self.insert_drag {
                let left = ui.min_rect().left();
                let right = ui.min_rect().right();
                ui.painter().line_segment([pos2(left, y), pos2(right, y)], Stroke::new(2.0_f32, Color32::from_rgb(30, 150, 50)));
            }
        });
    }

    /// Creo 10 tree header: toolbar, filter box and the BENENNUNG / SACHNUMMER column titles.
    /// Returns whether columns are shown and their x offsets.
    pub(crate) fn tree_header(&mut self, ui: &mut egui::Ui) -> (bool, f32, f32) {
        // Creo 10 tree toolbar
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            let r = icon_button(ui, Icon::Body, "Darstellung", true, false, 18.0);
            let id = ui.id().with("tree_settings");
            if r.clicked() { ui.memory_mut(|m| m.toggle_popup(id)); }
            egui::popup_below_widget(ui, id, &r, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
                menu_spacing(ui);
                if check_row(ui, Some(self.show_suppressed), None, "Unterdrückte Objekte anzeigen") { self.show_suppressed = !self.show_suppressed; }
                if check_row(ui, Some(!self.creo.hide_cols), None, "Spalten BENENNUNG / SACHNUMMER") { self.creo.hide_cols = !self.creo.hide_cols; }
            });
            ui.add_space(6.0);
            if icon_button(ui, Icon::ExpandAll, "Alle erweitern", true, false, 18.0).clicked() {
                self.expanded = match &self.asm { Some(a) => a.data.components.iter().map(|c| c.id).collect(), None => self.data.features.iter().map(|f| f.id).collect() };
                self.creo.design_open = true;
            }
            if icon_button(ui, Icon::Collapse, "Alle reduzieren", true, false, 18.0).clicked() { self.expanded.clear(); self.creo.design_open = false; }
            if icon_button(ui, Icon::Columns, "Baumspalten", true, !self.creo.hide_cols, 18.0).clicked() { self.creo.hide_cols = !self.creo.hide_cols; }
            ui.add_space(6.0);
            if icon_button(ui, Icon::Suppress, "Unterdrückte Objekte anzeigen", true, self.show_suppressed, 18.0).clicked() { self.show_suppressed = !self.show_suppressed; }
            ui.add_space(6.0);
            if icon_button(ui, Icon::InsertHere, "Hier einfügen ans Ende", self.data.insert_index.is_some() && !self.in_asm(), false, 18.0).clicked() { self.set_insert_after(None); }
            icon_button(ui, Icon::Layers, "Ebenenbaum", false, false, 18.0);
            if icon_button(ui, Icon::Search, "Suchen", true, self.creo.search.is_some(), 18.0).clicked() { self.creo.search = Some(String::new()); }
        });
        ui.add_space(4.0);
        // filter box with funnel, clear, ▾ and +
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (fr, _) = ui.allocate_exact_size(vec2(24.0, 26.0), Sense::hover());
            icons::paint(ui.painter(), Rect::from_center_size(fr.center(), vec2(18.0, 18.0)), Icon::Filter, true);
            let w = (ui.available_width() - 56.0).max(60.0);
            egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(egui::Margin::symmetric(4, 3)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.creo.tree_filter).frame(false).desired_width(w - 30.0).hint_text(""));
                    if ui.add(egui::Button::new(RichText::new("×").size(15.0).color(LABEL)).frame(false)).on_hover_text("Filter löschen").clicked() { self.creo.tree_filter.clear(); }
                });
            });
            let (tr, _) = ui.allocate_exact_size(vec2(14.0, 26.0), Sense::hover());
            super::ribbon::tri(ui.painter(), tr.center(), LABEL);
            ui.label(RichText::new("+").size(18.0).color(LABEL));
        });
        ui.add_space(6.0);
        let tree_w = ui.available_width();
        let cols = !self.creo.hide_cols && tree_w > 300.0;
        let (c1, c2) = (tree_w * 0.52, tree_w * 0.77);
        if cols {
            let (hr, _) = ui.allocate_exact_size(vec2(tree_w, 26.0), Sense::hover());
            let p = ui.painter();
            p.text(pos2(hr.left() + c1 + 8.0, hr.center().y), Align2::LEFT_CENTER, "BENENNUNG", FontId::proportional(12.5), TEXT);
            p.text(pos2(hr.left() + c2 + 8.0, hr.center().y), Align2::LEFT_CENTER, "SACHNUMMER", FontId::proportional(12.5), TEXT);
            for x in [c1, c2] { p.line_segment([pos2(hr.left() + x, hr.top()), pos2(hr.left() + x, hr.bottom())], Stroke::new(1.0_f32, Color32::from_rgb(211, 214, 215))); }
            p.line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0_f32, Color32::from_rgb(211, 214, 215)));
        }
        ui.add_space(2.0);
        (cols, c1, c2)
    }

    fn tree_context_menu(&mut self, ui: &mut egui::Ui, f: &Feature) {
        menu_spacing(ui);
        let ids = if self.tree_selection.contains(&f.id) { self.tree_selection.clone() } else { vec![f.id] };
        let item = |ui: &mut egui::Ui, icon: Icon, label: &str| menu_entry(ui, icon, label, true, false).clicked();
        if item(ui, Icon::EditDims, "Bemaßungen editieren") { self.start_edit_dims(f.id); ui.close_menu(); }
        if item(ui, Icon::EditDef, "Definition editieren") { self.edit_feature(f.id); ui.close_menu(); }
        if item(ui, Icon::SkModify, "Umbenennen") { self.renaming = Some((f.id, f.name.clone())); ui.close_menu(); }
        ui.separator();
        if f.suppressed { if item(ui, Icon::Resume, "Zurückholen") { self.set_suppressed(&ids, false); ui.close_menu(); } }
        else if item(ui, Icon::Suppress, "Unterdrücken") { self.ask_suppress(&ids); ui.close_menu(); }
        if item(ui, if f.hidden { Icon::Show } else { Icon::Hide }, if f.hidden { "Einblenden" } else { "Ausblenden" }) { self.toggle_hidden(&ids); ui.close_menu(); }
        if f.kind.produces_tool() {
            if item(ui, Icon::Pattern, "Muster") { self.tree_selection = vec![f.id]; self.start_feature(FeatureKind::new_pattern()); ui.close_menu(); }
            if item(ui, Icon::Mirror, "Spiegeln") { self.tree_selection = vec![f.id]; self.start_feature(FeatureKind::new_mirror()); ui.close_menu(); }
        }
        ui.separator();
        if item(ui, Icon::InsertHere, "Hier einfügen (danach)") { self.set_insert_after(Some(f.id)); ui.close_menu(); }
        if item(ui, Icon::Up, "Nach oben") { self.move_feature(f.id, -1); ui.close_menu(); }
        if item(ui, Icon::Up, "Nach unten") { self.move_feature(f.id, 1); ui.close_menu(); }
        ui.separator();
        if item(ui, Icon::Delete, "Löschen") { self.delete_features(&ids); ui.close_menu(); }
    }

    fn folder_browser(&mut self, ui: &mut egui::Ui) {
        // ⌘⌫ moves the selected entry to the Trash, like Finder
        if !ui.ctx().wants_keyboard_input() && ui.input(|i| i.modifiers.command && (i.key_pressed(egui::Key::Backspace) || i.key_pressed(egui::Key::Delete))) {
            if let Some(p) = self.creo.browser_sel.clone().filter(|p| p.exists()) { self.creo.fs_action = Some(super::creo::FsAction::Trash(p)); }
        }
        ui.horizontal(|ui| {
            if icon_button(ui, Icon::Up, "Einen Ordner nach oben", self.browser_dir.parent().is_some(), false, 14.0).clicked() {
                if let Some(p) = self.browser_dir.parent() { self.browser_dir = p.to_path_buf(); }
            }
            if icon_button(ui, Icon::FolderItem, "Neuer Ordner", self.browser_dir.is_dir(), false, 14.0).clicked() { self.creo.fs_action = Some(super::creo::FsAction::NewFolder(self.browser_dir.clone())); }
            let shown = self.browser_dir.display().to_string();
            ui.add(egui::Label::new(RichText::new(shown).size(11.5)).truncate());
        });
        ui.separator();
        let mut entries: Vec<(String, bool, std::path::PathBuf)> = std::fs::read_dir(&self.browser_dir)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| {
                let p = e.path();
                (e.file_name().to_string_lossy().to_string(), p.is_dir(), p)
            }).filter(|(n, d, _)| !n.starts_with('.') && (*d || n.ends_with(".pfprt") || n.ends_with(".pfasm") || n.ends_with(".pfdrw") || super::creo::is_neutral_file(n) || super::creo::is_creo_file(n) || n.ends_with(".pdf"))).collect())
            .unwrap_or_default();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if entries.is_empty() { ui.label(RichText::new("Keine Ordner oder Freo-Modelle hier").color(LABEL)); }
            for (name, is_dir, path) in entries {
                let icon = if is_dir { Icon::FolderItem } else if name.ends_with(".pfprt") { Icon::FileItem } else if name.ends_with(".pfasm") { Icon::Assembly } else if name.ends_with(".pfdrw") { Icon::Drawing } else { Icon::Export };
                let sel = self.creo.browser_sel.as_ref() == Some(&path) || (self.creo.browser_sel.is_none() && self.path.as_ref() == Some(&path));
                let (r, _) = Self::paint_row(ui, 0, icon, &name, TEXT, false, sel, None);
                if r.clicked() || r.secondary_clicked() { self.creo.browser_sel = Some(path.clone()); }
                let assemble = self.in_asm() && name.ends_with(".pfprt");
                let r = r.on_hover_text(if is_dir { "Doppelklick, um den Ordner zu öffnen" } else if assemble { "Doppelklick, um in die Baugruppe einzubauen" } else { "Doppelklick zum Öffnen" });
                if r.double_clicked() {
                    if is_dir { self.browser_dir = path.clone(); }
                    else if assemble { if !self.placing() { self.assemble_path(path.clone()); } }
                    else if super::creo::is_creo_file(&name) { self.load(path.clone()); }
                    else if (name.ends_with(".pfprt") || name.ends_with(".pfasm") || name.ends_with(".pfdrw") || super::creo::is_neutral_file(&name)) && self.confirm_discard() { self.load(path.clone()); }
                }
                let dir = self.browser_dir.clone();
                let mut act = None;
                let fx = self.creo.fs.clone();
                r.context_menu(|ui| {
                    menu_spacing(ui);
                    if assemble {
                        if menu_entry(ui, Icon::Assemble, "Zusammenbauen", !self.placing(), false).clicked() { self.assemble_path(path.clone()); ui.close_menu(); }
                        ui.separator();
                    }
                    super::creo::fs_menu(ui, &dir, Some(&path), &fx, &mut act);
                });
                if act.is_some() { self.creo.fs_action = act; }
            }
            // right-click on the empty part of the list
            let h = ui.available_height();
            let bg = ui.allocate_response(vec2(ui.available_width(), if h.is_finite() { h.clamp(40.0, 4000.0) } else { 200.0 }), Sense::click());
            super::ui::no_hand(ui.ctx(), bg.id);
            let dir = self.browser_dir.clone();
            let mut act = None;
            let fx = self.creo.fs.clone();
            bg.context_menu(|ui| super::creo::fs_menu(ui, &dir, None, &fx, &mut act));
            if act.is_some() { self.creo.fs_action = act; }
        });
    }
}
