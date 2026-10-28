//! Main window layout: panels, keyboard shortcuts, the status bar, the in-graphics toolbar
//! and the selection mini toolbar.

use super::camera::StdView;
use super::icons::{self, Icon};
use super::render::Style;
use super::ribbon::{icon_button, style_icon, theme::*};
use super::sketcher::Outcome;
use super::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, RichText, Stroke};

impl App {
    fn shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.wants_keyboard_input();
        let (cmd, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |k: egui::Key| ctx.input(|i| i.key_pressed(k));
        if cmd && pressed(egui::Key::Z) && !typing {
            if let Some(s) = &mut self.sketch { if shift { s.redo() } else { s.undo() } }
            else if shift { self.do_redo() } else { self.do_undo() }
        }
        if cmd && pressed(egui::Key::S) { self.save(shift); }
        if cmd && shift && pressed(egui::Key::E) { self.export_dialog(None); }
        if cmd && pressed(egui::Key::O) { self.open(); }
        if cmd && pressed(egui::Key::N) { self.confirm_new(); }
        // Anzeigestil: Strg+1 … Strg+6
        if cmd && !typing {
            for (k, st) in [egui::Key::Num1, egui::Key::Num2, egui::Key::Num3, egui::Key::Num4, egui::Key::Num5, egui::Key::Num6].into_iter().zip(Style::ALL) {
                if pressed(k) { self.style = st; }
            }
        }
        if typing || self.sketch.is_some() { return; }
        // keys go to the file dialogs while one is open
        if self.creo.open.is_some() || self.creo.export.is_some() || self.creo.name_dialog.is_some() || self.creo.browser.is_some() { return; }
        if self.placing() {
            if pressed(egui::Key::Escape) {
                let side = self.place_editor().and_then(|e| e.side);
                if self.dash_panel.is_some() { self.dash_panel = None; }
                else if side.is_some() { if let Some(e) = self.asm.as_mut().and_then(|a| a.place.as_mut()) { e.side = None; } }
                else { self.place_cancel(); }
            }
            if pressed(egui::Key::Enter) { self.place_finish(); }
            if cmd && pressed(egui::Key::G) { self.place_update(); }
            return;
        }
        if self.in_asm() && !cmd && (pressed(egui::Key::Delete) || pressed(egui::Key::Backspace)) && !self.tree_selection.is_empty() {
            let ids = self.tree_selection.clone();
            self.asm_delete(&ids);
            return;
        }
        if pressed(egui::Key::Escape) {
            if self.file_menu { self.file_menu = false; }
            else if self.edit_dims.is_some() { self.edit_dims = None; self.selection.clear(); }
            else if let Some(ed) = &self.editor { if ed.pick.is_some() { self.set_pick_target(None); } else if self.dash_panel.is_some() { self.dash_panel = None; } else { self.cancel_feature(); } }
            else { self.selection.clear(); self.tree_selection.clear(); self.measure = false; self.mini = None; }
        }
        if pressed(egui::Key::Enter) && self.editor.is_some() { self.finish_feature(); }
        if !cmd && (pressed(egui::Key::Delete) || pressed(egui::Key::Backspace)) && self.editor.is_none() && !self.tree_selection.is_empty() {
            let ids = self.tree_selection.clone();
            self.delete_features(&ids);
        }
        if cmd && pressed(egui::Key::G) { self.regenerate(true); }
        if cmd && pressed(egui::Key::D) { self.camera.rot = StdView::Standard.rotation(); self.fit_pending = true; }
        if cmd && pressed(egui::Key::F) { self.fit_pending = true; }
    }

    pub fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").exact_height(44.0).frame(egui::Frame::NONE.fill(STATUS_BG).inner_margin(egui::Margin::symmetric(4, 2))).show(ctx, |ui| {
            let r = ui.max_rect();
            ui.painter().line_segment([r.left_top() - vec2(4.0, 2.0), r.right_top() + vec2(4.0, -2.0)], Stroke::new(1.0_f32, BORDER));
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                if icon_button(ui, Icon::TreeTab, "Navigator ein-/ausblenden", true, self.show_nav, 18.0).clicked() { self.show_nav = !self.show_nav; }
                if icon_button(ui, Icon::Globe, "Ordner-Browser", true, self.nav_tab == NavTab::Browser && self.show_nav, 18.0).clicked() { self.nav_tab = NavTab::Browser; self.show_nav = true; }
                let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                if icon_button(ui, Icon::FullScreen, "Vollbild", true, full, 18.0).clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full)); }
                let (sep, _) = ui.allocate_exact_size(vec2(9.0, 36.0), egui::Sense::hover());
                ui.painter().line_segment([sep.center_top(), sep.center_bottom()], Stroke::new(1.0_f32, SEP));
                // Creo's message area: the last two messages, newest at the bottom with a bullet
                let msgs: Vec<&Message> = self.messages.iter().rev().take(2).collect::<Vec<_>>().into_iter().rev().collect();
                let w = (ui.available_width() - 330.0).max(160.0);
                let resp = ui.allocate_ui(vec2(w, 40.0), |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        if msgs.len() < 2 { ui.add_space(19.0); }
                        for (i, m) in msgs.iter().enumerate() {
                            let newest = i + 1 == msgs.len();
                            let (icon, c) = match m.kind {
                                MsgKind::Info => ("•", Color32::from_rgb(40, 110, 200)),
                                MsgKind::Success => ("•", Color32::from_rgb(40, 110, 200)),
                                MsgKind::Warning => ("⚠", Color32::from_rgb(200, 130, 0)),
                                MsgKind::Error => ("⚠", Color32::from_rgb(200, 36, 28)),
                                MsgKind::Prompt => ("➤", Color32::from_rgb(40, 110, 200)),
                            };
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(RichText::new(if newest { icon } else { " " }).color(c).size(12.5));
                                let tc = match m.kind { MsgKind::Error => Color32::from_rgb(190, 30, 24), MsgKind::Warning => Color32::from_rgb(150, 90, 0), _ => Color32::from_rgb(75, 75, 75) };
                                ui.add(egui::Label::new(RichText::new(&m.text).color(tc).size(12.5)).truncate());
                            });
                        }
                    }).response
                }).inner.interact(egui::Sense::click());
                if resp.on_hover_text("Klicken für das Meldungsprotokoll").clicked() { self.windows.messages = true; }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let label = self.filter_label();
                    egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, BORDER)).show(ui, |ui| {
                        let cb = egui::ComboBox::from_id_salt("filter").width(150.0).height(300.0).selected_text(RichText::new(label).size(13.0));
                        if let Some(s) = &mut self.sketch {
                            cb.show_ui(ui, |ui| { for f in super::sketcher::SkFilter::ALL { ui.selectable_value(&mut s.filter, f, f.name()); } }).response.on_hover_text("Auswahlfilter");
                        } else {
                            cb.show_ui(ui, |ui| { for f in Filter::ALL { ui.selectable_value(&mut self.filter, f, f.name()); } }).response.on_hover_text("Auswahlfilter");
                        }
                    });
                    let (sep, _) = ui.allocate_exact_size(vec2(5.0, 32.0), egui::Sense::hover());
                    ui.painter().line_segment([sep.center_top(), sep.center_bottom()], Stroke::new(1.0_f32, SEP));
                    if icon_button(ui, Icon::Search, "Suchen", !self.creo.home, self.creo.search.is_some(), 18.0).clicked() { self.creo.search = Some(String::new()); }
                    if self.creo.home { return; }
                    let failed = !self.result.errors.is_empty() || !self.result.param_errors.is_empty();
                    let r = icon_button(ui, if failed { Icon::RegenFail } else { Icon::RegenOk }, if failed { "Regenerierungsfehler — siehe Modellbaum" } else { "Modell erfolgreich regeneriert" }, true, false, 16.0);
                    if r.clicked() { self.regenerate(true); }
                    if let Some(ed) = self.place_editor() {
                        let st = ed.status.clone();
                        ui.label(RichText::new(format!("Status: {}", st.text())).size(12.0).color(match st {
                            crate::model::assembly::PlaceStatus::Full | crate::model::assembly::PlaceStatus::Assumed => Color32::from_rgb(20, 130, 50),
                            crate::model::assembly::PlaceStatus::Invalid(_) => Color32::from_rgb(200, 36, 28),
                            _ => Color32::from_rgb(190, 120, 0),
                        }));
                    }
                    if let Some(s) = &self.sketch {
                        let weak = s.sketch.constraints.iter().filter(|c| c.weak).count();
                        ui.label(RichText::new(if s.dof == 0 { "Vollständig definiert".to_string() } else { format!("{} schwache Bem.", weak) }).size(12.0).color(LABEL));
                    }
                    let n = if self.sketch.is_some() { self.sketch.as_ref().map(|s| s.selection.len()).unwrap_or(0) } else { self.selection.len() + self.tree_selection.len().min(if self.selection.is_empty() { usize::MAX } else { 0 }) };
                    if n > 0 { ui.label(RichText::new(format!("{} ausgewählt", n)).size(12.0).color(LABEL)); }
                });
            });
        });
    }

    fn graphics_toolbar(&mut self, ctx: &egui::Context, rect: Rect) {
        use super::ribbon::{check_list, menu_row};
        let mut normal_view = false;
        egui::Area::new(egui::Id::new("gfx_toolbar")).pivot(Align2::CENTER_TOP).fixed_pos(pos2(rect.center().x, rect.top() + 6.0)).order(egui::Order::Middle).show(ctx, |ui| {
            egui::Frame::NONE.fill(RIB_TOP).stroke(Stroke::new(1.0_f32, Color32::from_rgb(163, 163, 163))).corner_radius(0.0).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    let sz = 20.0;
                    let popup = |ui: &mut egui::Ui, r: &egui::Response, key: &str, close: egui::PopupCloseBehavior, f: &mut dyn FnMut(&mut egui::Ui)| {
                        let id = ui.id().with(key);
                        if r.clicked() { ui.memory_mut(|m| m.toggle_popup(id)); }
                        super::ribbon::demo_popup(ui, id, key);
                        egui::popup_below_widget(ui, id, r, close, |ui| { ui.spacing_mut().item_spacing.y = 0.0; f(ui) });
                    };
                    if icon_button(ui, Icon::Refit, "Einpassen", true, false, sz).clicked() { self.fit_pending = true; }
                    if icon_button(ui, Icon::ZoomIn, "Vergrößern", true, false, sz).clicked() { self.camera.scale /= 1.25; }
                    if icon_button(ui, Icon::ZoomOut, "Verkleinern", true, false, sz).clicked() { self.camera.scale *= 1.25; }
                    if icon_button(ui, Icon::Repaint, "Neu zeichnen", true, false, sz).clicked() { self.buffers = Arc::new(render::ModelBuffers { generation: 0, solid: vec![], edges: vec![] }); }
                    let r = icon_button(ui, style_icon(self.style), "Anzeigestil", true, false, sz);
                    popup(ui, &r, "gt_style", egui::PopupCloseBehavior::CloseOnClick, &mut |ui| {
                        ui.set_min_width(330.0);
                        for s in Style::ALL { if menu_row(ui, Some(style_icon(s)), s.name(), s.shortcut(), true, self.style == s).clicked() { self.style = s; } }
                    });
                    let r = icon_button(ui, Icon::SavedViews, "Gespeicherte Orientierungen", true, false, sz);
                    popup(ui, &r, "gt_views", egui::PopupCloseBehavior::CloseOnClick, &mut |ui| {
                        ui.set_min_width(330.0);
                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                            // Creo: the two fixed orientations first, then all named views alphabetically
                            let mut named: Vec<(String, glam::DQuat)> = StdView::ALL[2..].iter().map(|v| (v.name().to_string(), v.rotation())).collect();
                            named.extend(self.data.views.iter().map(|v| (v.name.clone(), v.rot)));
                            named.sort_by(|a, b| a.0.cmp(&b.0));
                            let list: Vec<(String, glam::DQuat)> = StdView::ALL[..2].iter().map(|v| (v.name().to_string(), v.rotation())).chain(named).collect();
                            for (n, q) in list { if menu_row(ui, None, &n, "", true, false).clicked() { self.push_view(); self.camera.rot = q; self.fit_pending = true; } }
                        });
                        ui.separator();
                        if menu_row(ui, Some(Icon::Reorient), "Neu orientieren...", "", true, false).clicked() {
                            self.reorient = Some(Reorient { base: self.camera.rot, h: 0.0, v: 0.0, z: 0.0, name: String::new() });
                        }
                        if menu_row(ui, Some(Icon::NormalView), "Senkrechte Ansicht", "", true, false).clicked() { normal_view = true; }
                    });
                    if icon_button(ui, Icon::Section, "Schnitt", true, self.section.is_some(), sz).clicked() { self.toggle_section(); }
                    if icon_button(ui, Icon::SpinCenter, "Drehmittelpunkt", true, self.show_spin, sz).clicked() { self.show_spin = !self.show_spin; }
                    if icon_button(ui, Icon::Perspective, "Perspektivansicht", true, self.camera.perspective, sz).clicked() { self.camera.perspective = !self.camera.perspective; }
                    let r = icon_button(ui, Icon::DatumDisplay, "Bezugsanzeigefilter", true, false, sz);
                    popup(ui, &r, "gt_datum", egui::PopupCloseBehavior::CloseOnClickOutside, &mut |ui| {
                        ui.set_min_width(260.0);
                        check_list(ui, &mut [(Icon::Axis, "Achsenanzeige", &mut self.show_axes), (Icon::Point, "Punkt-Darstellung", &mut self.show_points), (Icon::Csys, "KSys-Anzeige", &mut self.show_csys), (Icon::Plane, "Ebenen-Anzeige", &mut self.show_planes)]);
                    });
                    if icon_button(ui, Icon::Plane, "Ebenen-Anzeige", true, self.show_planes, sz).clicked() { self.show_planes = !self.show_planes; }
                    if icon_button(ui, Icon::Axis, "Achsenanzeige", true, self.show_axes, sz).clicked() { self.show_axes = !self.show_axes; }
                    if icon_button(ui, Icon::Point, "Punkt-Darstellung", true, self.show_points, sz).clicked() { self.show_points = !self.show_points; }
                    if icon_button(ui, Icon::Csys, "KSys-Anzeige", true, self.show_csys, sz).clicked() { self.show_csys = !self.show_csys; }
                    if icon_button(ui, Icon::PlaneTags, "Bezugsbeschriftungen", true, self.show_tags, sz).clicked() { self.show_tags = !self.show_tags; }
                    if icon_button(ui, Icon::Transparent, "Transparent", true, self.transparent, sz).clicked() { self.transparent = !self.transparent; }
                    if self.sketch.is_some() {
                        ui.separator();
                        if icon_button(ui, Icon::SketchView, "Skizzenansicht", true, false, sz).clicked() { self.sketch_view(); }
                        let r = icon_button(ui, Icon::ConDisplay, "Skizzierer-Anzeigefilter", true, false, sz);
                        let r2 = icon_button(ui, Icon::SkCheck, "Skizzierer-Prüfungen", true, false, sz);
                        if let Some(s) = &mut self.sketch {
                            popup(ui, &r, "gt_sk", egui::PopupCloseBehavior::CloseOnClickOutside, &mut |ui| {
                                ui.set_min_width(270.0);
                                check_list(ui, &mut [(Icon::DimDisplay, "Bemaßungsanzeige", &mut s.show_dims), (Icon::ConDisplay, "Bedingungsanzeige", &mut s.show_cons), (Icon::Grid, "Rasterdarstellung", &mut s.grid), (Icon::Vertex, "Eckpunktanzeige", &mut s.show_vertices), (Icon::SkLock, "Sperrenanzeige", &mut s.show_locks)]);
                                ui.separator();
                                if ui.checkbox(&mut s.snap_grid, "Am Raster fangen").changed() && s.snap_grid { s.grid = true; }
                            });
                            popup(ui, &r2, "gt_skcheck", egui::PopupCloseBehavior::CloseOnClickOutside, &mut |ui| {
                                ui.set_min_width(440.0);
                                check_list(ui, &mut [(Icon::HlOverlap, "Überlappende Geometrie hervorheben", &mut s.hl_overlap), (Icon::HlIntersect, "Sich schneidende Geometrie hervorheben", &mut s.hl_intersect), (Icon::HlBranch, "Geometrieabzweigungen hervorheben", &mut s.hl_branch), (Icon::OpenEnds, "Offene Enden hervorheben", &mut s.open_ends), (Icon::ShadeLoops, "Geschlossene Schleifen schattieren", &mut s.shade)]);
                            });
                        }
                    }
                });
            });
        });
        if normal_view { self.normal_view(); }
        self.reorient_dialog(ctx);
    }

    /// Senkrechte Ansicht: look straight at the sketch plane, or at the selected plane / planar face.
    fn normal_view(&mut self) {
        if self.sketch.is_some() { self.sketch_view(); return; }
        let sel = self.selection.clone();
        let Some(fr) = self.plane_ref(&sel).and_then(|p| self.displayed().frame_for(&p)) else {
            self.say("Eine Ebene oder ebene Fläche wählen, dann Senkrechte Ansicht.", MsgKind::Prompt);
            return;
        };
        self.push_view();
        self.camera.rot = glam::DQuat::from_mat3(&glam::DMat3::from_cols(fr.x, fr.y, fr.z()));
        self.fit_pending = true;
    }

    /// Neu orientieren...: dynamic rotation about the screen axes and saving the view by name.
    fn reorient_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut ro) = self.reorient.clone() else { return };
        let (mut ok, mut cancel, mut save, mut open) = (false, false, false, true);
        egui::Window::new("Ansicht").id(egui::Id::new("reorient")).title_bar(false).collapsible(false).resizable(false).default_pos(pos2(420.0, 200.0)).show(ctx, |ui| {
            if super::creo::dialog_header(ui, "Ansicht") { open = false; }
            ui.set_width(330.0);
            ui.add_space(4.0);
            ui.horizontal(|ui| { ui.label("Typ"); ui.add_enabled_ui(false, |ui| { egui::ComboBox::from_id_salt("ro_type").width(200.0).selected_text("Dynamische Orientierung").show_ui(ui, |_| {}); }); });
            ui.label(RichText::new("Drehen").strong());
            egui::Grid::new("ro_grid").num_columns(2).show(ui, |ui| {
                for (l, v) in [("H", &mut ro.h), ("V", &mut ro.v), ("Z", &mut ro.z)] {
                    ui.label(l);
                    ui.add(egui::Slider::new(v, -180.0..=180.0).suffix("°"));
                    ui.end_row();
                }
            });
            ui.separator();
            ui.label(RichText::new("Gespeicherte Orientierungen").strong());
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut ro.name).desired_width(170.0));
                if ui.add_enabled(!ro.name.trim().is_empty(), egui::Button::new("Speichern")).clicked() { save = true; }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("Abbrechen").min_size(vec2(90.0, 26.0))).clicked() { cancel = true; }
                    if ui.add(egui::Button::new("OK").min_size(vec2(70.0, 26.0))).clicked() { ok = true; }
                });
            });
        });
        let q = ro.base * glam::DQuat::from_rotation_y(ro.h.to_radians()) * glam::DQuat::from_rotation_x(-ro.v.to_radians()) * glam::DQuat::from_rotation_z(ro.z.to_radians());
        self.camera.rot = q;
        if save {
            let n = ro.name.trim().to_uppercase();
            self.commit("", |d| {
                d.views.retain(|v| v.name != n);
                d.views.push(crate::model::features::SavedView { name: n.clone(), rot: q });
            });
            self.say(format!("Orientierung {} gespeichert.", n), MsgKind::Success);
        }
        if cancel || !open { self.camera.rot = ro.base; self.reorient = None; }
        else if ok { self.reorient = None; }
        else { self.reorient = Some(ro); }
    }

    fn mini_toolbar(&mut self, ctx: &egui::Context) {
        let Some(mb) = self.mini else { return };
        if self.busy() || self.edit_dims.is_some() || self.measure { self.mini = None; return; }
        if let Some(p) = ctx.input(|i| i.pointer.hover_pos()) {
            if (p - mb.pos).length() > 190.0 { self.mini = None; return; }
        }
        if self.in_asm() { self.asm_mini_toolbar(ctx, mb); return; }
        let planar = self.plane_ref(&[mb.pick]).is_some();
        let owner = self.owner(mb.pick).filter(|o| self.data.feature(*o).is_some());
        enum A { Sketch, Extrude, Revolve, Hole, Plane, Round, Chamfer, Dims, Def, Suppress, Hide, Delete }
        let mut act = None;
        egui::Area::new(egui::Id::new("mini_toolbar")).fixed_pos(mb.pos + vec2(14.0, -46.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(2.0).corner_radius(3.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let mut b = |ui: &mut egui::Ui, icon: Icon, tip: &str, a: A| { if icon_button(ui, icon, tip, true, false, 18.0).clicked() { act = Some(a); } };
                    if planar {
                        b(ui, Icon::Sketch, "Skizze", A::Sketch);
                        b(ui, Icon::Extrude, "Profil", A::Extrude);
                        b(ui, Icon::Revolve, "Drehen", A::Revolve);
                        b(ui, Icon::Hole, "Bohrung", A::Hole);
                        b(ui, Icon::Plane, "Ebene", A::Plane);
                    }
                    if matches!(mb.pick, Pick::Edge(_)) {
                        b(ui, Icon::Round, "Rundung", A::Round);
                        b(ui, Icon::Chamfer, "Fase", A::Chamfer);
                    }
                    if owner.is_some() {
                        ui.separator();
                        b(ui, Icon::EditDims, "Bemaßungen editieren", A::Dims);
                        b(ui, Icon::EditDef, "Definition editieren", A::Def);
                        b(ui, Icon::Suppress, "Unterdrücken", A::Suppress);
                        b(ui, Icon::Hide, "Ausblenden", A::Hide);
                        b(ui, Icon::Delete, "Löschen", A::Delete);
                    }
                });
            });
        });
        let Some(a) = act else { return };
        self.mini = None;
        match a {
            A::Sketch => self.start_feature(FeatureKind::new_sketch()),
            A::Extrude => self.start_feature(FeatureKind::new_extrude()),
            A::Revolve => self.start_feature(FeatureKind::new_revolve()),
            A::Hole => self.start_feature(FeatureKind::new_hole()),
            A::Plane => self.start_feature(FeatureKind::new_plane()),
            A::Round => self.start_feature(FeatureKind::new_round()),
            A::Chamfer => self.start_feature(FeatureKind::new_chamfer()),
            A::Dims => if let Some(o) = owner { self.start_edit_dims(o) },
            A::Def => if let Some(o) = owner { self.edit_feature(o) },
            A::Suppress => if let Some(o) = owner { self.set_suppressed(&[o], true) },
            A::Hide => if let Some(o) = owner { self.toggle_hidden(&[o]) },
            A::Delete => if let Some(o) = owner { self.delete_features(&[o]) },
        }
    }

    /// Startseite: Creo's window before any model is opened.
    fn home_frame(&mut self, ctx: &egui::Context) {
        if let Some(p) = ctx.input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) { self.load(p); return; }
        let (cmd, _) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        if cmd && ctx.input(|i| i.key_pressed(egui::Key::N)) { self.confirm_new(); }
        if cmd && ctx.input(|i| i.key_pressed(egui::Key::O)) { self.open(); }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("Freo".into()));
        self.title_bar(ctx);
        self.tab_strip(ctx);
        if !self.creo.ribbon_min_hidden() {
            egui::TopBottomPanel::top("ribbon").frame(egui::Frame::NONE.fill(RIB_TOP).inner_margin(egui::Margin::symmetric(4, 3))).show(ctx, |ui| {
                self.ribbon_body(ui);
                let r = ui.max_rect();
                ui.painter().line_segment([pos2(r.left() - 4.0, r.bottom() + 2.5), pos2(r.right() + 4.0, r.bottom() + 2.5)], Stroke::new(1.0_f32, BORDER));
            });
        }
        self.status_bar(ctx);
        if self.show_nav {
            egui::SidePanel::left("home_nav").resizable(true).default_width(220.0).width_range(170.0..=420.0)
                .frame(egui::Frame::NONE.fill(PANEL_BG).inner_margin(egui::Margin { left: 4, right: 4, top: 0, bottom: 4 }))
                .show(ctx, |ui| self.home_navigator(ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
            graphics_background_bg(ui.painter(), ui.max_rect(), self.creo.bg);
        });
        self.file_menu_ui(ctx);
        self.dialogs(ctx);
        self.new_dialogs(ctx);
        self.open_dialog_ui(ctx);
        self.creo_file_ui(ctx);
        self.file_browser_ui(ctx);
        self.name_dialog_ui(ctx);
    }

}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if std::env::var("FREO_DEMO").as_deref() == Ok("icons") { super::demo::icon_sheet(ctx); self.demo_tick(ctx); return; }
        // closing the window (File ▸ Exit, the close button, ⌘Q) asks before discarding work
        if ctx.input(|i| i.viewport().close_requested()) && std::env::var("FREO_EXIT").is_err() && !self.confirm_discard() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        // files opened from Finder (double-click, "Öffnen mit", Dock)
        for p in crate::macos_open::take() {
            let n = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if super::creo::is_creo_file(&n) || self.creo.home || self.confirm_discard() { self.load(p); }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        if self.creo.home {
            self.home_frame(ctx);
            hand_cursor(ctx);
            self.demo_tick(ctx);
            return;
        }
        if self.drawing_active() {
            if let Some(p) = ctx.input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) { if self.confirm_discard() { self.load(p); } }
            self.drawing_frame(ctx);
            self.open_dialog_ui(ctx);
            self.creo_file_ui(ctx);
            self.file_browser_ui(ctx);
            self.name_dialog_ui(ctx);
            hand_cursor(ctx);
            self.demo_tick(ctx);
            return;
        }
        self.shortcuts(ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} (Aktiv) - Freo", self.data.name)));
        if let Some(p) = ctx.input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            // a part dropped on an assembly is assembled, like dragging from Creo's browser
            if self.in_asm() && p.extension().map(|e| e == "pfprt").unwrap_or(false) { if !self.placing() { self.assemble_path(p); } }
            else if self.confirm_discard() { self.load(p); }
        }

        self.title_bar(ctx);
        self.tab_strip(ctx);
        let mut outcome = Outcome::None;
        // a minimised ribbon still shows feature dashboards (they are the only way to finish them)
        let show_ribbon = !self.creo.ribbon_min_hidden() || self.tab == Tab::Context;
        if show_ribbon { egui::TopBottomPanel::top("ribbon").frame(egui::Frame::NONE.fill(RIB_TOP).inner_margin(egui::Margin::symmetric(4, 3))).show(ctx, |ui| {
            outcome = self.ribbon_body(ui);
            if self.tab == Tab::Context && self.sketch.is_none() && self.editor.is_some() { self.feature_panel_tabs(ui); }
            if self.tab == Tab::Context && self.placing() { self.placement_panel_tabs(ui); }
            let r = ui.max_rect();
            ui.painter().line_segment([pos2(r.left() - 4.0, r.bottom() + 2.5), pos2(r.right() + 4.0, r.bottom() + 2.5)], Stroke::new(1.0_f32, BORDER));
        }); }
        self.status_bar(ctx);
        if self.show_nav {
            egui::SidePanel::left("model_navigator").resizable(true).default_width(380.0).width_range(220.0..=620.0)
                .frame(egui::Frame::NONE.fill(PANEL_BG).inner_margin(egui::Margin { left: 4, right: 4, top: 0, bottom: 4 }))
                .show(ctx, |ui| self.navigator(ui));
        }
        let mut central = Rect::NOTHING;
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
            central = ui.max_rect();
            self.viewport_ui(ui);
        });
        if let Some(s) = &mut self.sketch {
            for (t, k) in std::mem::take(&mut s.messages) { self.messages.push(Message { text: t, kind: k }); }
        }
        match outcome {
            Outcome::Done(sk) => { let slot = self.sketch.as_ref().map(|s| s.slot).unwrap_or(Slot::Main); self.finish_sketch(sk, slot); }
            Outcome::Cancel => self.cancel_sketch(),
            Outcome::None => {}
        }
        self.graphics_toolbar(ctx, central);
        self.file_menu_ui(ctx);
        self.feature_panel_ui(ctx);
        self.placement_panel_ui(ctx);
        self.datum_dialog(ctx);
        self.sketch_setup_dialog(ctx);
        self.sketch_resetup_dialog(ctx);
        self.dialogs(ctx);
        self.asm_windows(ctx);
        self.new_dialogs(ctx);
        self.tool_windows(ctx);
        self.open_dialog_ui(ctx);
        self.export_dialog_ui(ctx);
        self.creo_file_ui(ctx);
        self.file_browser_ui(ctx);
        self.name_dialog_ui(ctx);
        self.mini_toolbar(ctx);
        hand_cursor(ctx);
        self.demo_tick(ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let list: Vec<String> = self.recent.iter().map(|p| p.display().to_string()).collect();
        if let Ok(s) = serde_json::to_string(&list) { storage.set_string("recent_files", s); }
        storage.set_string("browser_dir", self.browser_dir.display().to_string());
        storage.set_string("work_dir", if self.creo.keep_work_dir { self.work_dir.display().to_string() } else { String::new() });
    }
}

/// Widgets that sense clicks but are work areas, not buttons (3D view, sheet, tree rows,
/// title-bar drag zone): they keep the arrow cursor.
pub fn no_hand(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<egui::Id>>(egui::Id::new("freo_no_hand")).push(id));
}

/// Pointing-hand cursor over everything clickable (buttons, tabs, menus, check boxes …),
/// unless a widget already chose a cursor (text field, resize, drag).
pub fn hand_cursor(ctx: &egui::Context) {
    let skip: Vec<egui::Id> = ctx.data_mut(|d| std::mem::take(d.get_temp_mut_or_default::<Vec<egui::Id>>(egui::Id::new("freo_no_hand"))));
    if ctx.output(|o| o.cursor_icon) != egui::CursorIcon::Default || ctx.dragged_id().is_some() { return; }
    let hovered = ctx.interaction_snapshot(|i| i.hovered.clone());
    let clickable = hovered.iter().any(|id| !skip.contains(id) && ctx.read_response(*id).map(|r| r.enabled() && r.sense.senses_click()).unwrap_or(false));
    if clickable { ctx.set_cursor_icon(egui::CursorIcon::PointingHand); }
}

/// Paint the Creo graphics-area background gradient into `rect`.
pub fn graphics_background(painter: &egui::Painter, rect: Rect) { graphics_background_bg(painter, rect, super::creo::Background::Standard); }

pub fn graphics_background_bg(painter: &egui::Painter, rect: Rect, bg: super::creo::Background) {
    let (top, bot) = bg.colors();
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bot);
    mesh.colored_vertex(rect.left_bottom(), bot);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// Datum names are plain text in Creo 10 (no tag box).
pub fn datum_tag(painter: &egui::Painter, pos: egui::Pos2, text: &str, color: Color32) {
    painter.text(pos + vec2(2.0, 1.0), Align2::LEFT_TOP, text, FontId::proportional(13.0), color);
    let _ = icons::Icon::Plane;
}
